# The Rust engine — design rules

This file is the contract every porting worker is handed. It exists because the
port is a **rewrite, not a transliteration**: the Python source is the *spec for
the rules*, never the spec for the *representation*. A port that reads like
Python with semicolons would be slower than Python is worth replacing, and would
lock in the dynamic-lookup costs that are the entire reason for the rewrite.

Measured baseline (2026-08-02): one 3p self-play game is 3.45s / 372 decisions /
9.3ms per decision, and the profile is **flat** — the largest single entry is
`dict.get` at 9%. There is no hot loop to fix. The cost *is* the dynamic
lookups, so the port only pays off if the lookups go away.

## The five rules

**1. Cards are integers, not strings.** `CardId(u16)` indexes a static table.
A name is an I/O concern: it appears when parsing `data/*.json`, when printing,
and nowhere else. Any `HashMap<String, _>` in the engine is a bug.

**2. Static data is baked in, not parsed at start-up.** `src/card_table.rs`'s
`CARDS` table is checked in, hand-verified against `data/*.json` by
`card_table.rs`'s own `#[cfg(test)]` module rather than parsed at start-up. The
core crate therefore has **zero dependencies** and does no start-up work. (The
base game's 236 cards are frozen forever -- the expansion is out of scope by
standing decision -- so the `tools/gen_cards.py` generator that originally
produced this table has served its purpose and been deleted; the test is what
keeps `data/*.json` and `card_table.rs` from silently drifting apart now that
nothing regenerates the second from the first.)

**3. State is flat, fixed-size and `Clone`.** No `Vec` inside the per-turn hot
path, no `HashMap`, no `Option<Box<_>>` chains. A `GameState` clone must be a
memcpy of a few kilobytes, because the search clones states constantly and that
copy is what `copy_state` costs today.

**4. No lifetimes in the state types.** Everything is owned; cross-references
are `CardId` / player index / dense-slot index, never `&'a T`. This is not a
concession — arena-and-index is the native idiom for this shape of code. It is
also what keeps a porting worker out of a fight with the borrow checker, and a
worker that fights the borrow checker burns its whole context re-reading the
fight.

**5. Choices are enums, not tuples of tagged strings.** Python spells moves
`("take", 3)` and `("upgrade", "Bronze", "Iron")`. Rust spells them
`Move::Take { slot: u8 }` and `Move::Upgrade { from: CardId, to: CardId }`. An
illegal *shape* must be unrepresentable; `legal_moves` then only has to decide
legality, not well-formedness.

## The effect system, and why it is two things

236 cards carry 113 distinct effect keys. **Sixty of those keys appear exactly
once.** That distribution is the whole design:

- The ~25 keys that recur are **numeric fields** on a `CardEffects` struct in
  the static table — `culture`, `strength`, `happy`, `civil_actions`,
  `military_actions`, `resource_discount`, and so on. These are read on every
  stats recomputation and must be a field load, never a lookup.
- The ~60 one-offs are a **`Special` enum**, one variant per card that has a
  unique rule, dispatched by `match`. `Ozymandias`-style cards are not data;
  they are code. Python hides this behind name-dispatch in `effects.py` and a
  `"text"` field; Rust makes it an exhaustive `match`, so **adding a card that
  the engine cannot interpret is a compile error** rather than a silently
  ignored dict key.

That last sentence is the point of the rewrite beyond speed. The recurring bug
class in this project is *"present in this registry, absent from that one, and
nothing fails when they disagree."* An exhaustive `match` over `Special` is that
class made impossible for card effects.

## Scope

Ported: `engine/` and `engine/bots/` (~42k lines of Python). Rust owns
self-play, training and the league when it is done.

Not ported: `tools/` and `experiments/` stay Python — glue where speed is
irrelevant — plus `analysis/`, which is the experiment record.

There is **no half-shipped value**. Until the engine is whole, Rust cannot run a
league. That is the cost of doing it natively instead of as a `pyo3` extension
that lets Python keep driving, and it is the trade that was chosen deliberately
on 2026-08-04.

## How correctness is established

Differential testing against the Python engine, with the oracle **offline**
because there is no in-process bridge:

1. Python replays seeded self-play games and dumps, per ply, the state digest,
   the legal-move list and the chosen move (`tools/dump_fixtures.py`).
2. Rust replays the same fixtures and asserts identical digests, identical legal
   moves in identical order, and identical resulting states.
3. A divergence names one ply and one field. That is a cheap, precise target.

"Make the tests pass" is where a port's budget dies; "state diverges at ply 41,
field `food`" does not. The Python `engine/statediff.py` already exists for
exactly this comparison and its output format is what the Rust side reproduces.

Move ordering is part of the contract, not an implementation detail: the bots
break ties by index, so a reordered `legal_moves` silently changes play.

Ordering is therefore load-bearing all the way down into the containers.
Python's tableau is a `dict`, so it iterates in build order; `Tableau::remove`
is order-preserving for that reason and not because removal is hot. The first
casualty found was `economy.lose_population`, which takes a worker off the
first worker-holding card it walks — a swap-remove would have weakened a
different card than Python does, but only in games where something had already
left play, which is the kind of divergence that shows up late and reads as a
logic bug. Any container the engine iterates in a play-affecting order gets the
Python container's ordering semantics, and says so at the definition.

## Division of labour

The **types are written up front, by hand, before any module is ported** — this
file, `cards.rs`, `state.rs`, `moves.rs`. The nativeness lives in the types; a
worker handed a finished type layout writes idiomatic Rust by construction and
the job stays mechanical.

Workers then fill in module bodies against those types, one short worker per
module, each finishing at a green differential run. Short workers, never long
ones: a subagent never compacts, so its context only grows and every step
re-reads all of it.
