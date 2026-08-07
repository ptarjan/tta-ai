# Human-game reconstruction (`rust/src/bin/replay.rs`) — status and fidelity

Companion to [`HUMAN_PLAY.md`](HUMAN_PLAY.md) (the corpus and the play-rate
census `corpuscensus.rs` computes over it — pure counting, no game state) and
to the shared parser both binaries use, `rust/src/corpus.rs`. This doc covers
the harder thing: **standing up an actual `GameState` from a human game's
journal and validating every move through the real engine**
(`legal::legal_moves` / `apply::apply`), which is the prerequisite for a
move-agreement analysis (stand at each decision point in a real game, ask the
bot what it would play, measure how often it agrees) that nothing in this
repo does yet. This doc is the spike's result, not that analysis.

## Status: not production-ready. 0/24 sampled games replay to completion —
## but the wall is now overwhelmingly genuine hidden information, not gaps

That number is the headline, not a footnote — see "What did NOT complete"
below for why. **This is the second pass over this binary.** The first pass
landed 0/24 complete at a mean of 33 actions consumed (roughly rounds 1-5)
before the first stop, with the categories blocking it dominated by this
binary's own coverage gaps. This pass closed every gap that pass named,
**found and fixed one confirmed engine rules bug** (see below, with a
before/after test), and found one **corpus data gap** (a card-name alias)
that turned out to be the single largest cause of the "event-timing
collapse" category — see "Bugs found and fixed" for all of them. The result:
still 0/24 complete on this exact sample, but the mean action count before a
stop rose from 33 to **42.75** (+30%), and of the 24 stops, **14 (58%) are
now genuinely unrecoverable hidden information (a forced military discard)**
— up from 7/24 in the first pass, not because discards got MORE common but
because fixing the other categories let more games run deep enough to reach
one. The remaining 10 stops are two **open, honestly-unresolved** structural
questions (see "What remains open" below) — not guessed at, not papered
over.

```text
tar -xzf sources/bgo/journals.tar.gz -C /tmp/bgo-journals
cargo run --profile difftest --bin replay -- \
    sources/bgo/index.tsv /tmp/bgo-journals/journals <game_id> [game_id ...]
```

Two env vars gate optional stderr tracing added during this pass, useful for
any future debugging session of the same shape (a "why did legal_moves not
offer this" hunt): `REPLAY_DEBUG=1` prints the acting player's civil/military
action counts, government, leader, phase, and top-of-pending-stack the
instant an attempted move turns out illegal; `REPLAY_DEBUG_ALL=1` additionally
prints that same state after every SUCCESSFUL move, plus a line per
`resolve_intervening` loop iteration and per hidden-`PrepareEvent` inference.
Both are silent (zero output, and the underlying values are cheap `Copy`
reads) unless the env var is set.

## What is RECONSTRUCTED vs SIMULATED (read this before trusting any output)

The journal cannot tell the engine everything it needs: in particular the
true civil/military deck shuffle order, and what sat in a player's hand
before it was played. `replay.rs` draws a hard line between two kinds of
state:

- **RECONSTRUCTED**: every card identity a human is ever observed to take,
  build, develop, play, elect, declare, propose, colonize, bid on, or
  destroy. These come straight from the journal text via `tta::corpus::
  classify` (the same classifier `corpuscensus.rs` validated at 99.99% line
  coverage over the full 1,011-game corpus) and are applied as real engine
  `Move`s, checked against `legal_moves` at every step.
- **SIMULATED**: everything the engine needs to hold a legal, complete
  `GameState` that the journal never reveals — unrevealed card-row slots,
  the civil/military deck order beyond what's been drawn, and a player's
  hand contents before they are observed playing them. `replay.rs` seeds
  these from `game::new_game`'s ordinary (fictional, fixed-seed) shuffle and
  overwrites a slot/hand entry with the real observed card the instant that
  slot/card is ever taken or played ("grounding" it). An ungrounded slot's
  identity is never validated against anything and is never historically
  accurate — only its existence (some card was there, costing some action)
  is real.

**What a later agreement analysis is allowed to claim, given this**: any
decision point reached with the ACTING player's own hand/tableau fully
grounded (which by round 3-5 is essentially everything they've built,
developed, elected, played, or currently hold in civil hand) is a real
position the bot's `legal_moves` output can be meaningfully compared against.
Anything conditioned on an UNGROUNDED slot (what's still sitting unrevealed
in the row, an opponent's un-played military hand) is not, and any
agreement-rate tool built on top of this must gate on grounding state, not
just "did replay reach this line."

## Event/Territory preparation: the one inference this file makes, and why

`Move::PrepareEvent` (the Politics-phase action that queues a drawn Event or
Territory card to fire later) has **no journal line of its own** — BGO logs
only the resolution (`"X plays event ..."`), never the preparation. Every
`PrepareEvent` call causes exactly one `events::reveal_current_event`
(`rust/src/events.rs`), so `replay.rs` pre-scans each journal once,
collecting the exact card named in every `"...Current event:; <Age> /
<Name>; ..."` line into a FIFO. Whenever a player's Politics-phase decision
cannot be explained by an explicit textual action (pass, revolution, war,
aggression, pact offer), the binary infers a hidden `PrepareEvent`, grants
that player a placeholder Event-kind card (never checked against anything —
see SIMULATED above), and forces `state.current_events` to reveal exactly
the next journal-observed event/territory so the resolution the journal
shows next lines up.

This reproduces the right cards firing in the right ORDER, but **not** on
the historically correct turn and **not** by the historically correct
preparer — both are permanently unrecoverable from BGO's journal format. It
is the single biggest reason a "decider != expected actor" stop happens (see
below): the inference can fire for a player one turn "early" relative to the
true game, and by the time that shows up as a contradiction it's several
lines later and harder to trace back to its cause. This is a real,
structural limitation of the approach, not a bug to be fixed by more
parsing — the information is not in the journal. **This pass fixed one
concrete, confirmed way this inference collapsed downstream state (the
`PlayEvent`-line premature-drain bug, below) but did NOT fully close this
category** — see "What remains open."

One text-derivable fact was needed to keep this inference from
over-firing: BGO deals no military cards at all until a player's first
end-of-turn `"draws N military card(s)"` credit. Before that, a Politics
decision with no explicit action is a forced, sometimes-silently-logged pass
(no Event/Territory card can possibly exist in an empty hand), not a hidden
preparation — see "Bugs found and fixed" below, this was found empirically
on the first real game tested.

## What this file gives up on outright, and why

- **Discard** (§6.6 hand-limit, and any other forced military discard): BGO
  logs only a count (`"<Color> discards N cards"`), never which cards.
  Genuinely unrecoverable — stops the game rather than guess. **This is now
  the dominant stop reason: 14/24 (58%) of the sample** (11 via the plain
  `Discard` line, 3 via a `Pending::Choice(DiscardMilitary)` reached through
  a different code path — same root cause). This did not get WORSE — it got
  MORE VISIBLE, because every other category that used to stop a game
  earlier now lets it run deep enough to hit a real discard instead. Given
  BGO games run 15-20+ rounds and a forced discard fires whenever a
  player's military hand exceeds its limit (common, not rare), **most real
  games likely contain at least one discard eventually, meaning most
  100%-legal-move completions may be structurally unreachable regardless of
  how many more categories below get closed** — this is the single most
  important scoping fact for anyone continuing this work: chasing 24/24
  complete is very likely NOT achievable without either recovering discard
  identities (impossible, BGO doesn't log them) or accepting an
  approximation (which the project's own standing rule — "don't guess
  hidden cards, an honest low number beats a rosy fake one" — rules out).
- ~~**Aggression defense** with any committed defense cards: BGO logs only a
  count, never which.~~ **This was false — see "Tenth pass" below.** BGO's
  `"<Color> defends ..."` line is not a bare count; it is one clause PER
  committed card, and every clause fully identifies its card (a printed
  `+2`/`+4`/`+6` bonus number for the age I/II/III bonus cards, one card per
  value; any other hand card for a plain "military card played", and which
  one is provably irrelevant to any observable outcome). Fixed in the tenth
  pass, `resolve_aggression_defense`/`parse_defense_clauses` in
  `replay_common.rs`.
- **`PutBack`** (a human undoing their own `Take` via BGO's client-side
  undo): there is no `Move` for this in the engine at all — `moves.rs`'s
  variant list has no "untake". **Fixed in this pass** for the common
  shapes (nested take/take-back stacks, and takes interrupted by an
  unrelated committed action before their own take-back) — see "Bugs found
  and fixed." A take-back with NO matching preceding take by the same actor
  anywhere in the game (a genuine parser gap, or a BGO logging artifact)
  would still stop the game; none of the 24 sampled games hit this residual
  case in this pass.
- **Colonization sacrifice specifics**: `"Sacrificed Units:; ..."` DOES name
  exact identities, but resolving `Pending::Colonize`'s branching
  `SendUnit`/`SendBonus`/`SendDiscard` choices against that list is not
  implemented in this pass — the binary auto-drains colonization by picking
  the engine's own first-offered option at each step until the force
  clears. This keeps the game running and gets the reveal's own
  culture/resource totals right, but does not verify which units were
  spent; flagged per-game (`colonize_approximated`) whenever it fires. None
  of the 24 sampled games reached a colonization, so this was never
  exercised against real data in either pass.

## Sample: 24 games, 8 each of 2p/3p/4p

Selected as the first 8 games of each player count in `index.tsv` order — no
cherry-picking. Full run: `cargo run --profile difftest --bin replay --
sources/bgo/index.tsv /tmp/bgo-journals/journals <24 ids>`.

**0/24 replayed to completion with every human action legal** (same as the
first pass), but the composition of WHY changed substantially:

| | first pass | this pass |
|---|---|---|
| games complete | 0/24 | 0/24 |
| mean actions before stop | 33 | 42.75 (+30%) |
| stopped on genuinely unrecoverable hidden info (discard) | 7/24 (29%) | 14/24 (58%) |
| stopped on a `replay.rs` coverage gap or open question | 17/24 (71%) | 10/24 (42%) |

**No game's final score was compared to `index.tsv`** (`game::scores` is
only computed for a completed replay) — see "Final-score cross-check" below
for why this is now expected to stay true for MOST games in this corpus
regardless of further `replay.rs` work, and what would actually move it.

### Mismatch categories in this pass, ranked by frequency

| # games | category | representative line | verdict |
|---|---|---|---|
| 14 | Military discard, identity unrecoverable (11 via the plain `Discard` line, 3 via a `Pending::Choice(DiscardMilitary)` reached through a different code path — same root cause) | `Purple discards 1 card` | Genuine journal limitation (BGO logging quirk), documented, excluded on purpose. Grew from 7/24 in the first pass purely because other fixes let more games run deep enough to reach one — see "What this file gives up on" above. |
| 5 | `decider != expected actor` — a DIFFERENT player's single action appears interleaved into the acting player's turn with **no intervening `EndTurn` line at all** | `Purple builds Philosophy` mid-Orange's-turn (StuckPending, decider 0 != expected 1), game `7523354` line 26 | **Open, unresolved — see "What remains open" below.** Distinct from (and a smaller residue of) the first pass's "event-timing collapse" diagnosis: closing the `PlayEvent`-drain bug and the `Development of Civilization` alias (both below) closed most of the ORIGINAL 8/24 in this category, but left this smaller, structurally different puzzle — the interleaved player has NO pending state open at all when `resolve_intervening` reaches them, and `state.current` genuinely still names the OTHER player. Not guessed at. |
| 3 | `Pop`/`PopFree` both illegal, `civil_actions == 0` with a full budget's worth of actions already correctly accounted for | `Green increases population Green spends 2 food` (legal_moves has no Take/Pop/Build at all) | **Open, unresolved — see "What remains open" below.** Same OBSERVED shape as the Pyramids engine bug this pass fixed (a real player using one MORE civil-costing action than `Despotism`'s printed 4 plus every currently-modeled bonus source explains) but confirmed NOT explained by that fix, by Hammurabi's MA-as-CA conversion, or by Michelangelo's/any other in-play card's `CardEffects`. A genuinely unidentified extra source, or a different bug entirely. |
| 1 | `Take{slot}` illegal, same `civil_actions == 0` shape as the `Pop`/`PopFree` row above | `Purple takes Colossus in hand Purple uses 1 civil action` (legal_moves has no Take at all) | Same open question as the row above — this is the SAME category (a global action-budget shortfall, not a slot-cost mismatch: `legal_moves` excludes every row slot, not just one), split into its own row only because `replay.rs` reports it as a different `MismatchKind` variant. The original "row-slot cost-matcher" diagnosis from the first pass (a `ground_row_slot` placement bug) is CLOSED — every instance of it in this pass's original 3/24 was actually this same action-budget issue in disguise, confirmed by the Pyramids fix (below) resolving all three. This residual instance is a DIFFERENT, still-open cause of the same symptom. |
| 1 | `PlayAction` for a `FreeCivilAction` card opened no `Pending::Choice` | `Orange builds Religion using Urban Growth Orange spends 2 resources` | Unchanged from the first pass. Edge case of the Rich Land/Urban Growth fix (below): `push_choice` silently no-ops when the option list would be empty (mirrors `FreeBuild`'s documented behaviour) — this binary doesn't yet handle that no-op case, so it reports "no pending opened" instead of falling back sensibly. Not investigated further this pass (a single occurrence, and the fix would be in `replay.rs`'s own translation, not the engine). |

Every category the first pass's table listed that is NOT in the table above
is **CLOSED** this pass: unpaired `PutBack` (was 3/24), the `Take{slot}`
row-slot cost-matcher as originally diagnosed (was 3/24 — see above, folded
into the action-budget question instead), the unidentified 2-option
`WonderStep`-blocking `Choice` (was 2/24, identified as `ChoiceKind::
GainBlock` and fixed), and `PolPass` illegal because `state.phase` was
already `Actions` (was 2/24, a downstream symptom of the `PlayEvent`-drain
bug, fixed at its source).

## Bugs found and fixed during this pass (`replay.rs` unless marked ENGINE)

In order of discovery, kept here for the same reason the first pass's list
is kept: each is a genuine "obvious in hindsight, invisible until tested
against real data" trap.

1. **Unpaired take-backs were only matched if immediately adjacent.**
   `prescan_putback_skips` tracked a single `last_take` slot, reset by ANY
   intervening classified line — but BGO's UI lets a player hold several
   tentative takes at once (take A, take B, put B back, put A back — a
   real, observed nested pattern) and freely interleave unrelated committed
   actions (a build, another take) between a take and its own later
   take-back. Fixed by replacing the single slot with a per-card stack of
   still-open takes (`HashMap<CardId, Vec<(usize, Color)>>`): a `PutBack`
   pops the most recent same-actor entry for that EXACT card (safe because
   every row/hand card is a unique instance in the table — the same
   assumption `ground_row_slot` already relies on), and any OTHER
   classified line for that same (actor, card) — i.e. the take got
   committed some other way — removes it from the stack defensively, so a
   theoretical later same-named "put back" can never wrongly erase a real
   action. Closed 3/24 games in the original sample.

2. **ENGINE BUG (confirmed, fixed, tested): a civil/military action bonus
   from a card entering play mid-turn was not usable until the NEXT
   turn.** `p.civil_actions`/`p.military_actions` are a decrementing-only
   per-turn pool, set once when a player's Politics phase closes
   (`effects::state_stats`'s `civil_actions`/`military_actions`, which
   sums the government's base plus every currently-in-play card's
   `CardEffects.civil_actions`/`military_actions` — e.g. Pyramids' printed
   `+1 civil action`, Code of Laws/Justice System/Civil Service/Kremlin,
   Julius Caesar/Napoleon/Joan of Arc/Robespierre's `+N military actions`).
   `set_government` already correctly topped up `p.civil_actions` the
   instant a mid-turn revolution changed government (`p.civil_actions =
   (s.civil_actions - spent).max(0)`) — but `on_enter_play`/`on_leave_play`
   (`apply.rs`, the shared hook every wonder-completion, tech-development,
   and leader-election handler already calls for blue/yellow tokens) never
   applied the SAME top-up for a card's own `civil_actions`/
   `military_actions` effect. Found by testing against a real 2p game
   (`7523350`): a human completed Pyramids mid-turn (their `civil_actions`
   was 0 at that instant, having just paid for the completing `WonderStep`)
   and immediately took a 5th action that same turn using the wonder's
   freshly-granted `+1`; the reconstructed engine rejected it — a REAL
   rules disagreement, not a parser gap, since `ground_row_slot`'s SLOT
   placement was already correct (`legal_moves` excluded every row slot,
   not just the wrong one). **Fixed** by adding the same `+= ` (on enter)
   `/ -=` (on leave, symmetric, for a leader swap) bump to
   `on_enter_play`/`on_leave_play` for these two `CardEffects` fields,
   clamped at 0. **Test**: `apply::tests::
   do_wonder_step_completing_pyramids_grants_the_extra_civil_action_this_turn`
   (`rust/src/apply.rs`) — builds a player with `civil_actions: 1`, pays
   the final Pyramids stage down to 0, asserts it's back to 1 after
   completion. Confirmed to fail before the fix (reverted the `on_enter_play`
   bump behind a `false &&` guard, reran: `left: 0, right: 1` — the
   assertion failure the bug predicts) and pass after (restored). This ALSO
   fully closed the first pass's "row-slot cost-matcher" category (3/24) —
   every instance was this same budget shortfall, not a slot-placement bug;
   `ground_row_slot`'s cost-based placement was correct all along.

3. **A `ChoiceKind::GainBlock` pending (an event's "Each civilization gains
   2 resources or 2 food, player's choice" — e.g. Development of Markets)
   blocked a LATER, unrelated action because nothing ever resolved it.**
   BGO logs each player's own pick as a standalone bookkeeping line
   (`"<Color> produces N food"` / `"<Color> produces N resources"`) that
   `corpus.rs` correctly treats as non-actionable for census purposes — but
   `replay.rs` needs a real `Move::Choose` to actually clear the pending.
   Found by testing against a real 3p game (`7522632`): a `WonderStep`
   several lines later failed with `legal_moves = [Choose{0}, Choose{1}]`
   because the ACTING player's own choice from an earlier event had never
   been cleared. **Fixed** by pre-scanning every standalone `"produces"`
   line into a per-seat FIFO (`prescan_gain_produces`) and draining any open
   `GainBlock` pending against it (matching by comparing the printed amount
   to each `Gain` option, not by position) the moment `resolve_intervening`
   sees one — regardless of whose turn it nominally is, mirroring the
   existing `FreeBuild` pattern. Closed 2/24 games.

4. **The single biggest fix this pass: `resolve_intervening`'s call for
   the `ActionClass::PlayEvent` line itself wrongly auto-drained EVERY
   qualifying player's `FreeBuild`/`GainBlock` choice from the SAME
   event before any of them were ever read.** `"X plays event ..."` is
   BGO's journal-side CONFIRMATION that an event already resolved — the
   engine resolves the event's ENTIRE effect (gains, and a
   `FreeBuild`/`GainBlock` choice queued for every qualifying player at
   once) synchronously inside `h_prepare_event`, the instant the hidden
   `PrepareEvent` is inferred, and `apply_one`'s own `PlayEvent` arm is a
   bare `Ok(())` that touches no state. But `resolve_intervening` was still
   being CALLED for that line, with `upcoming = (PlayEvent, None)` — and
   `FreeBuild`'s existing "if the upcoming line doesn't match one of my
   options, assume `Skip`" fallback (a correct heuristic for a player who
   genuinely declines) fired for EVERY qualifying player, because a
   `PlayEvent` line can never match a build. Found by testing against a
   real 3p game (`7522652`) where "Development of Religion" opened a
   `FreeBuild` choice for all 3 players and every one of them was silently,
   wrongly auto-`Skip`ped before their own real `"builds Religion"` lines —
   which follow two lines later — were ever read; the FIRST player's own
   later free build then fell through to a normal, FULL-PRICE `Move::Build`
   (which happened to still be legal, paying real resources this binary's
   simulated economy had, silently draining it) instead of the free
   `Move::Choose`. **Fixed** by skipping the `resolve_intervening` call
   entirely for `ActionClass::PlayEvent` lines — safe because `apply_one`'s
   handling of that class reads no state either, so nothing is lost;
   resolution (including the hidden-`PrepareEvent` inference itself, if it
   hasn't fired yet) is simply deferred to whatever the NEXT real line's own
   `resolve_intervening` call needs, which has the correct `upcoming` to
   match against. This one fix alone raised the sample's mean actions
   consumed noticeably and closed most of the first pass's 8/24
   `decider != expected actor` / `PolPass`-illegal combined category (5/24
   remain — see "What remains open").

5. **CORPUS DATA GAP (not an engine bug): BGO's own display name for the
   Age A event "Development of Civil Life" is "Development of
   Civilization" — an alias missing from `corpus.rs::ALIASES`.** Found by
   testing against a real 2p game (`7523354`): `current_event_name`'s
   lookup for `"Development of Civilization"` silently failed (returned
   `None`), so `prescan_event_reveals` dropped that event from the
   `event_reveals` FIFO entirely — shifting every LATER event in that same
   game one slot out of alignment, the exact mechanism the module doc's
   "Event/Territory preparation" section describes as the single biggest
   cause of `decider != expected actor`. Confirmed by exact text match:
   the journal's printed flavour text ("Immediately, each civilization may
   either: increase its population; or build a farm, mine or urban
   building; or develop a technology. It costs 1 [resource] less than
   usual.") is verbatim `state.rs`'s own doc-comment quote of "Development
   of Civil Life"'s real card text. **This card appears in 471 of the
   corpus's 1,011 games (47%) and in 14 of this sample's 24 (58%)** — by a
   wide margin the single highest-leverage fix in this pass, and the module
   doc's own prior claim ("zero games contain a card name outside the 2015
   base game — checked, not just trusted") turns out to have verified a
   NARROWER property than advertised: it covered every card identity
   `corpuscensus.rs` actually resolves for its own purposes, not every name
   embedded in a `"Current event:; ..."` clause specifically — this is the
   first time anything read that clause's name against `card_index` for
   real. **Fixed** by adding `("Development of Civilization", "Development
   of Civil Life")` to `ALIASES`. Whether this fully explains any GIVEN
   game's `decider != expected actor` stop varies (5/24 remain, on games
   where the interleaving has a different, still-open cause — see next
   section) but it measurably deepened every game that reaches this event,
   and is the correct, permanent fix regardless.

### Bugs found and fixed in the FIRST pass (kept for continuity)

1. Row-slot grounding never expired after a take (fixed).
2. A coincidental row match short-circuited cost-based placement (fixed).
3. The hidden-`PrepareEvent` inference over-fired before a player's first
   military draw (fixed).
4. An unmodeled build-discount mechanic (Rich Land, Urban Growth) was
   silently mispriced — `"builds X using Y"` needed `PlayAction{Y}` →
   `Choose{n}`, not a bare `Move::Build` (fixed).

See the first pass's git history for the full detail on these four; nothing
in this pass touched them further.

## What remains open — two genuinely unresolved structural questions

**Superseded by "Fourth pass" (below the Third pass section, further down
this file) — kept here for continuity/history, not as the current status.**
Both questions this section names were diagnosed in the fourth pass: #1
(interleaving) was root-caused to Development of Civil Life's out-of-turn
grant and fixed in `replay.rs` (9/24 → 1/24 residual); #2 (budget shortfall)
turned out to be two confirmed engine bugs plus a genuinely unidentified
residual (8/24 → 2/24). The `politics_done`-vs-global-`phase` theory #1
proposes below was checked and does NOT explain the dominant shape — see
the fourth pass's own section for why. Read on for the ORIGINAL diagnosis
as it stood after the second pass, then skip to "Fourth pass" for what is
actually still true today.

Both are reported here, honestly, as OPEN rather than guessed at or papered
over, per this project's standing rule that a confirmed rules disagreement
(or an honestly-flagged open question) is worth more than forcing a
completion.

### 1. A single action from a DIFFERENT player interleaves mid-turn, with no `EndTurn` between (5/24)

In every other game examined across both passes, one player's full turn
(Politics decision, then every one of their Actions-phase moves, then
`EndTurn`) is strictly self-contained before the next player's turn begins
in the journal. In the 5 remaining `decider != expected actor` games, this
breaks: e.g. `7523354` line 24-26 —

```text
Orange plays event ... Development of Civilization ...
Orange increases population Orange spends 1 food
Purple builds Philosophy Purple spends 2 resources     <- interjects, no EndTurn for Orange
Orange takes Hanging Gardens in hand Orange uses 1 civil action   <- Orange's turn resumes
...
Orange End turn ...
```

At the point of failure, `resolve_intervening` finds `state.pending` EMPTY
and `state.decider()` (== `state.current`, since nothing is pending)
genuinely still names Orange — there is no queued choice, no stale pending,
nothing this pass's `GainBlock`/`FreeBuild`/`PlayEvent` fixes address. Two
candidate explanations were considered but NOT confirmed:

- **A per-player Politics/Actions tracking gap.** `PlayerState::
  politics_done` is a genuine PER-PLAYER field (distinct from the single
  GLOBAL `state.phase`, which `end_politics` sets to `Actions` the moment
  ANY one player finishes their own political decision). `resolve_
  intervening`'s existing fallback for an out-of-turn political decision
  gates on the GLOBAL `state.phase == Phase::Politics`, which is false here
  (it reflects Orange, who is mid-Actions) — so if Purple's OWN unresolved
  political decision is what's actually blocking things, checking `!state.
  players[decider].politics_done` instead of the global phase might be the
  right fix. This was NOT implemented or tested this pass — `state.
  decider()` reporting Orange (not Purple) when nothing is pending means
  there is no live signal that Purple even has anything outstanding, so
  this theory is unconfirmed, not a diagnosis.
- **A genuine async-submission artifact in BGO's journal**, given this
  corpus is a correspondence-style (real-world-hours-to-days between moves)
  game and its timestamps for the interjecting line sit strictly between
  the two halves of the "interrupted" player's turn — plausible, but not
  verified against a second, independent signal.

**Do not paper over this by hand-mutating `state.current`** — that would
mask a real question (is the engine's per-player turn tracking correct
here, or is this genuinely an artifact BGO's journal format cannot
resolve?) behind a forced match. Flagged open.

### 2. A player needs one MORE civil action than the computed budget provides, with no known bonus source (4/24)

Same OBSERVED shape as the Pyramids engine bug this pass fixed and
confirmed (`legal_moves` excludes every Take/Pop/Build — a genuine
`civil_actions == 0` shortfall, not a slot- or cost-specific mismatch) but
**confirmed NOT explained by that fix** in at least one case examined in
detail (`7523087`, Purple electing Michelangelo mid-turn, replacing
Hammurabi): every civil-costing action that turn was individually correct
(the leader-replacement refund nets to zero exactly as §9.1 and the
existing `h_play_leader` code predict; neither Hammurabi's nor
Michelangelo's `CardEffects`/`Special` grant any `civil_actions` bonus per
`card_table.rs`), yet the human still took one MORE action than
`Despotism`'s printed 4 explains. Whether this is:

- a THIRD unmodeled action-economy source (some other card's ability not
  yet identified),
- the SAME root cause as the interleaving mystery above (i.e. an action
  the engine attributes to the wrong player's budget), or
- something else entirely,

is **not diagnosed** — flagged here as a confirmed, reproducible symptom
(4/24 games, `REPLAY_DEBUG=1`/`REPLAY_DEBUG_ALL=1` reproduce the full
civil-action trace for any of them) rather than guessed at. **Do not paper
over this by force-granting an extra action** — see the same standing rule
as above.

## Suspected ENGINE rules bugs

**Superseded by "Fourth pass" (further down this file)** — the fourth pass
confirmed and fixed TWO MORE engine bugs beyond the one below: event-granted
population increases wrongly charging food (`events::tests::
an_event_granted_population_increase_costs_no_food`), and Development of
Civil Life's ordered action never getting the same CA exemption Rich
Land/Urban Growth's identical grant already has (`apply::tests::
do_build_spends_no_civil_action_when_civil_life_banked_a_build_discount`,
`legal::tests::
build_move_is_offered_with_zero_civil_actions_when_civil_life_banked_a_discount`).
Three confirmed engine bugs total across this file's history, all fixed and
tested. The historical text below is kept for continuity.

**One confirmed and fixed in the second pass** — see "Bugs found and fixed"
item 2 above (the mid-turn `civil_actions`/`military_actions` top-up), with
a before/after test (`do_wonder_step_completing_pyramids_grants_the_extra_
civil_action_this_turn`, `rust/src/apply.rs`).

**One still open, NOT confirmed, as of the second pass**: item 2 under "What
remains open" above (the unexplained extra civil action) is the closest
remaining candidate for a SECOND engine bug — the OBSERVED shape (budget
shortfall with every known source individually verified correct) is
identical to the one that turned out to be real, but a second bonus source
has not been found or ruled out, and might instead be a `replay.rs`
reconstruction gap (a build this binary priced differently than the true
game, echoing the first pass's Rich Land/Urban Growth discovery) or the same
interleaving mystery in disguise. Filed open per the standing rule, not
asserted as a finding. **Resolved by the fourth pass** (see above) — mostly
Finding 1b (Civil Life's missing CA exemption), with a genuinely
unidentified 2/24 residual still open.

Every OTHER category in this pass's mismatch table traces to one of:
genuinely unrecoverable hidden information (discard), or an as-yet-unfixed
`replay.rs` translation gap (the single `FreeCivilAction` no-pending-opened
case). None qualifies as a confirmed engine disagreement.

## Final-score cross-check: not exercised, and unlikely to be reachable for most of this corpus without a scoping decision

`game::scores(&state)` (post-`finish_game`, matching what `index.tsv`'s
`results` column records) is only meaningful once a game reaches
`state.game_over` — which requires a FULL completion. Since 0/24 sampled
games completed in either pass, **the single strongest end-to-end check
available (matching final culture scores) was never run against real data.**
`replay.rs` already does the comparison (sorted-multiset, since seat order
isn't guaranteed to match `index.tsv`'s name order) the moment a game does.

**Scoping reality after this pass**: 58% of this sample now stops on a
genuinely unrecoverable forced discard — a full BGO game (15-20+ rounds)
gives a forced discard many chances to fire, so closing the two open
questions above would very likely still leave MOST games blocked on a
discard reached slightly later, not complete. Reaching a meaningful number
of full completions most likely requires an explicit, honestly-labelled
scoping decision (e.g. sampling for games that happen to avoid a discard
entirely, or accepting that "verified-legal-prefix depth" rather than "full
completion count" is this method's real deliverable) rather than more
`replay.rs` fixes of the same shape as this pass's.

## What to do next, roughly in priority order

1. Diagnose the two open items above ("What remains open") — both are
   reproducible on a small, named set of real games with `REPLAY_DEBUG_ALL=1`
   already wired up to show the exact state at the point of failure. The
   `politics_done`-vs-global-`phase` theory for item 1 is the most concrete
   unstarted lead.
2. Fix the single remaining `FreeCivilAction` no-pending-opened case (1/24)
   — a bounded `replay.rs` translation gap, not urgent.
3. Make an explicit scoping decision about the discard wall (see "Final-score
   cross-check" above) before investing further in completion count as the
   headline metric — it may be the wrong metric for this journal format.
4. Only then: run the final-score cross-check for real on whatever games DO
   complete under the current or a rescoped sample, and scale the sample
   past 24 games. Re-run `corpuscensus.rs`-style census over the newly
   confirmed alias to see if any OTHER corpus-wide name gaps exist (the
   `Development of Civilization` alias, found in this pass, was previously
   invisible to every prior validation pass).

## Third pass: military discard, solved by constraint propagation — not given up on

The prior section calls the forced military discard "genuinely
unrecoverable hidden information" and reports it as 14/24 (58%) of the
sample's stops, with the framing that most real games likely contain one
eventually and closing it was probably not worth attempting. **That framing
was wrong about recoverability, right about difficulty, and the fix was
worth doing anyway** — this section reports it plainly, including the parts
that didn't work out the way a first read of the idea suggests they should.

### The argument, and where it actually lands

BGO's journal never names which card a `"<Color> discards N cards"` line
removed — only the count. But a card a player is later observed **playing**
(`Move::War`/`Move::Aggression`/`Move::OfferPact`/`Move::PlayTactic` all
name the card on their own journal line) was, by definition, still in their
hand at the time of an earlier discard — it cannot have been discarded
there. `rust/src/discard_solver.rs` (new module, kept separate from
`replay.rs`/`apply.rs` on purpose — see below) implements exactly this:
pre-scan the whole journal once for every FUTURE such named play per seat
(`prescan_future_military_needs` in `replay.rs`), and at each discard
decision, rule out any of the engine's own currently-offered
`discard_options` candidates that reappears later. Three honestly-separated
outcomes, matching this project's standing rule that a completion rate
built mostly on guesses is weaker evidence than one built on facts:

- **Solved**: exactly one candidate survives the filter — not a guess.
- **Chosen**: more than one survives (genuine ambiguity) — picks the
  least-valuable one (`interact::discard_options` already sorts
  worst-defender-first), matching a rational real player's own incentive,
  per the explicit instruction: "if there really is some ambiguity that's
  fine, just of the valid possibilities choose one."
- **Forced collision**: EVERY candidate reappears later (the filter
  couldn't help at all) — still picks one (the least-valuable), but this is
  the "detect and report rather than silently let it pass" case: it means
  either this game's simulated hand undercounts what the real hand held (an
  earlier divergence), or a duplicate-named military card's second copy was
  the real discard. Counted and surfaced separately, never folded into
  "solved."

`DiscardSolver::choose` is pure and has 7 unit tests covering all three
buckets, including that a future need BEFORE the current line does NOT
exclude a candidate (already left the hand, so it's fair game again), and
that needs are scoped per-seat (another player's future play must not
exclude this player's candidate). Wired into `replay.rs` at two points:
`resolve_intervening` drains any OTHER player's stale `DiscardMilitary`
pending immediately (closing the "reached through a different code path"
shape the previous pass reported as 3/24 of the discard total), and
`apply_one`'s `ActionClass::Discard` arm resolves the CURRENT actor's own
discard via `Replayer::resolve_discard`. Those had to be split, not merged
into one unconditional drain, because of a real trap: resolving the LAST
queued discard can itself finish that player's end of turn
(`interact::QueueItem::DiscardMilitary` resumes `game::resume_end_turn`
once it clears) and advance `state.current` to the next player — draining
unconditionally ahead of the decider-equality check made `resolve_
intervening` see `decider != expected_actor` on a line that had, in fact,
just been fully and correctly consumed, and report every such line as
stuck. Found by testing against real games (the first commit of this pass
regressed to 0 stops recovered at all until this was fixed) — see the
second commit's own message for the full trace.

### What this closes, honestly, including why it mostly does NOT produce "Solved"

On the same 24-game sample (8 each of 2p/3p/4p, first-in-`index.tsv` order,
identical selection to both prior passes):

| | second pass | third pass (this one) |
|---|---|---|
| games complete | 0/24 | 0/24 |
| mean actions before stop | 42.75 | **51.0 (+19%)** |
| stopped ON a military discard | 14/24 (58%) | **0/24** |
| military discards resolved (not stopped on) | 0 | **63** (0 solved, 63 chosen, 0 forced collisions) |

Every one of the 24 games individually reached the same or a strictly
later stop point than the second pass; none regressed. **Discard is no
longer this sample's bottleneck at all** — every game that used to stop on
one now runs straight through it into whatever the NEXT real issue is.

The honest disappointment: **of 63 discards resolved, 0 were "Solved" —
all 63 landed in "Chosen."** This is not a bug in the solver (the unit
tests confirm `choose` correctly narrows to one, or flags a forced
collision, whenever the input data supports it) — it is a real, structural
fact about this corpus that the task's original framing (draw analogy: "you
know every card taken from the row, and every card later played") does not
quite fit for MILITARY cards specifically, and is worth stating precisely
so nobody re-derives it the hard way: **military-hand cards are never named
at draw time.** Unlike a civil-row `Take`, which BGO logs with the card's
name every time, a military card enters a hand via an anonymous
end-of-turn draw (`"<Color> draws N military card(s)"` — a count only, see
`corpus.rs`). `replay.rs`'s reconstructed `hand_military` is therefore
SIMULATED filler for essentially its entire content at any moment a discard
decision is open — `Replayer::ground_military_hand` only ever grounds a
card's real identity in the SAME function call that immediately consumes it
(war/aggression/pact/tactic all ground-then-remove atomically), so no
journal-verified card is ever sitting in hand at a discard boundary to be
excluded from consideration in the first place. The exclusion this module
performs is real and correctly implemented, but it can only fire when a
piece of SIMULATED filler already in hand happens to coincide, by CardId,
with a card independently named by a later play — a coincidence, not a
structural certainty, and this sample was simply too small (and the games
too short before hitting the NEXT open issue) to hit many. This is reported
as a scoping fact for anyone tempted to expect "Solved" to dominate at
scale: it will remain a minority category under this reconstruction model
regardless of sample size, because the underlying journal signal for
military cards specifically is thinner than for civil ones. **Zero forced
collisions** occurred either, for the same reason (the exclusion rarely
fires at all, so it rarely fires universally).

### Why 0/24 still complete, and what actually blocks each game now

Discard being solved does not, by itself, reach completion — it just moves
the wall. Re-triaging all 24 stops on this pass's output:

| # games | category | status |
|---|---|---|
| 9 | `decider != expected actor` (interleaving, no `EndTurn` between) | Same OPEN question as the second pass — see "What remains open" above. Grew from 5/24 to 9/24 purely because more games now run deep enough to reach it, the same "closing one wall exposes the next" pattern that grew the discard category from 7/24 to 14/24 across the first two passes. |
| 8 | Civil-action-budget shortfall (`Pop`/`PopFree`/`Take`/`Build` all illegal, `civil_actions == 0`) | Same OPEN question as the second pass — see "What remains open" above. Grew from 4/24 to 8/24, same reason. |
| 2 | `"plays Frugality ... increases population"` — `PlayAction{Frugality}` not offered by `legal_moves` at all | **Newly reached, NOT diagnosed.** Not investigated this pass (out of scope: this is a `replay.rs`/engine question unrelated to discards, and this repo has a second agent actively working `apply.rs`'s action-economy bug concurrently with this pass — deliberately left alone rather than risk stepping on that work). Plausibly the same general shape as the already-known "`FreeCivilAction` no-pending-opened" gap (row below) since Frugality also grants a `FreeCivilAction`, but for `IncreasePopulation` rather than `Build` — NOT confirmed, flagged as a lead only. |
| 2 | `"builds X using Urban Growth"` opens no `Pending::Choice` | Same OPEN gap the second pass reported as 1/24 (`FreeCivilAction` no-pending-opened) — grew to 2/24, same "runs deeper now" reason. Not investigated further this pass. |
| 1 | `WonderStep` illegal completing a wonder | **Newly reached, NOT diagnosed.** A single occurrence; not investigated this pass for the same reason as the Frugality row above. |
| 1 | `PolPass` illegal (`state.phase` apparently already past Politics) | **Newly reached, NOT diagnosed.** A single occurrence, same shape as a `PlayEvent`-drain bug the second pass fixed at its source — possibly a NEW instance of a similar timing issue, not confirmed. |
| 1 | Build cost mismatch (unmodeled discount, `Warriors`) | Unchanged from the second pass's own "gives up on" list (an unmodeled discount source, not a parser gap). |

No new engine (`apply.rs`/`legal.rs`) bug is claimed or confirmed by this
pass — every mismatch above is either a previously-open question growing
because games run deeper (expected, not new information) or a genuinely
new-looking `replay.rs`-level gap flagged but explicitly NOT chased down,
both to stay in scope (this pass's job was discard, not the action-economy
investigation already underway elsewhere in this repo) and because a
single occurrence each is too thin a sample to diagnose responsibly.

### Final-score cross-check: still not reached

`game::scores(&state)` vs `index.tsv`'s `results` column still could not be
run — 0/24 games in this sample reached `state.game_over`. This remains
true for the same structural reason the second pass named (a full BGO game
gives every one of the categories above, not just discard, many chances to
fire over 15-20+ rounds) — closing discard alone was never expected to be
sufficient, and this pass confirms that prediction rather than
contradicting it. The two OPEN questions ("What remains open," above)
are now this project's highest-leverage remaining unknowns for reaching any
completions at all.

## Fourth pass: both "What remains open" questions diagnosed — one is a confirmed second ENGINE bug, the other a replayer/turn-model gap, both fixed

This pass had one job: diagnose the two structural open questions above,
verify rather than assume, and fix whatever turned out to be real. Both
were root-caused with hard evidence from real games, both are now fixed and
tested, and the sample's composition moved a lot as a direct result —
reported here exactly as it landed, including the parts that are STILL open.

### Finding 1 (civil-action-budget shortfall, was 8/24): a SECOND confirmed engine bug, plus one already-fixed bug's blast radius

The "civil_actions == 0 shortfall" shape (§9.2's original framing) turned
out to be at least two, and possibly three, unrelated things wearing the
same symptom:

**1a. ENGINE BUG (confirmed, fixed, tested): event-granted population
increases were charged food as if they were the PAID §6.1 action.**
`Development of Settlement`/`Immigration`/`Refugees` are the only three
base-game cards whose `EventBlock` carries an `increasePopulation` key —
`events.rs::apply_gains_block` routed it through `paid_increase_population`
(the same food-costing, one-time-discount-consuming path as a normal
`Move::Pop`), but the digital-edition card text for all three
("Players increase population.", "The players with the most happy faces
increase population.", "...gains 3 culture and increases population.") is
phrased exactly like every other unconditional `EventBlock` gain (food,
resources, science, culture) — none of which cost anything — with no
mention of a food payment. **Found by replaying real BGO game `7522616`**:
Purple prepares Development of Settlement, then LATER the same turn pays an
explicit, separately-logged `"Purple increases population Purple spends 3
food"` for their own real Pop action. Reconstructing the turn proves the
event grant had to be free: entering the turn at `yellow_bank == 17`, a
FREE grant moves it to 16 (`pop_cost_base(16) == 3`, exactly the food the
human paid for their OWN LATER Pop); the old PAID code additionally spent
`pop_cost_base(17) == 2` food on the event grant itself, leaving only 1 food
when the human's real Pop needed 3 — an `IllegalMove` this binary reported
as the "budget shortfall" mystery, for at least this one game. **Fixed** by
renaming `paid_increase_population` to `free_increase_population` and
switching it to `economy::increase_population`'s established free-grant
shape (`cost: 0, consume_one_time: false` — the same shape `apply.rs::
h_pop_free`/Ocean Liners already use). **Test**:
`events::tests::an_event_granted_population_increase_costs_no_food`
(`rust/src/events.rs`) — confirmed to fail before the fix (food dropped
3 → 1, the exact BGO-observed shortfall) and pass after.

**1b. ENGINE BUG (confirmed, fixed, tested): Development of Civil Life's
ordered action was never wired to the "ordered free action" CA exemption
every other card of its shape gets.** `Development of Civil Life`
("Development of Civilization" in BGO's UI) reads "Immediately, each
civilization may either: increase its population; or build a farm, mine or
urban building; or develop a technology. It costs 1 [resource] less than
usual." — textually near-identical to Rich Land's "Build or upgrade a farm
or mine; pay 1 less resource" and Frugality's own `freeCivilAction` text,
both of which rule item 11 already governs: "if it orders an action,
perform it under normal rules but paying no civil ... action for it." Rich
Land/Urban Growth/Frugality are wired to that exemption
(`Special::FreeCivilAction`, `apply::apply_free_civil_move`, called with
`free: true`) — but Civil Life's identical grant, banked in `PlayerState::
one_time_discount` (`build_resources`/`develop_science`/`pop_food`), was
only ever read for its RESOURCE discount, never for the CA exemption: the
normal `Move::Pop`/`Move::Build`/`Move::Develop` dispatch (`apply.rs::
apply`) always calls `h_pop`/`do_build`/`h_develop` with `free: false`, and
`legal.rs::action_moves` gated Pop/non-unit-Build/Develop on `ca >= 1` with
no exception. **Found by replaying real BGO game `7523355`**: Purple's
round-3 turn (Despotism, 4 CA) spends 1 CA on their own Civil-Life-discounted
build, 1 on a wonder stage, 2 on a `Take` — a correctly-priced 4 CA total —
then a SECOND, real `Take` fails with `civil_actions == 0`: 5 CA-costing
actions on a 4-CA budget, and the one action that shouldn't have cost
anything was the Civil-Life build. **Fixed**, three sites, mirroring the
existing pattern exactly (`costs::civil_life_ca_free`, a one-line documented
helper, is the single source of truth all of them read):
- `legal.rs::action_moves` — Pop, non-unit-Build, and both Develop gates now
  read `ca >= 1 || civil_life_ca_free(p.one_time_discount.<field>)`. NOT
  applied to Upgrade or Destroy — Civil Life's text does not cover them.
- `apply.rs::h_pop`/`do_build`/`h_develop` — each now reads its own
  `one_time_discount` field BEFORE the existing code zeroes it (consuming
  the grant), and skips `costs::pay_ca`/`military_actions -= 1` when it was
  set, exactly mirroring `free`'s existing effect from a DIFFERENT free
  source (an action card) — the two stay independent, since a card in hand
  and a banked event grant can both be live at once.

  **Tests**: `apply::tests::
  do_build_spends_no_civil_action_when_civil_life_banked_a_build_discount`
  and `legal::tests::
  build_move_is_offered_with_zero_civil_actions_when_civil_life_banked_a_discount`
  (both confirmed to fail before the fix — the `apply.rs` one via a genuine
  `debug_assert` panic, "paid more civil actions than available" — and pass
  after).

**Verified against real data, not just unit tests**: re-running the 24-game
sample after 1a+1b, `7523355`/`7523354`/`7523350`/`7522616` (all feature
Development of Civil Life, all previously stopped on the exact
`civil_actions == 0` shortfall shape) each ran dramatically deeper (e.g.
`7523355`: 27 → 62 actions before its NEXT, unrelated stop) — while
`7523087` (no Civil Life event anywhere in that game's journal) stayed
EXACTLY at its previous stop, unaffected, the clean negative control this
fix predicts.

**What is still open, honestly**: **2/24 games (`7522632`, `7523087`)
still show the exact `civil_actions == 0`, everything-blocked shape**, and
neither is explained by 1a, 1b, Hammurabi's MA-as-CA conversion, or any
`CardEffects`/`Special` on a card either player has. `7522632`'s Civil Life
event does not even fire until round 5 — three rounds AFTER this game's
round-4 shortfall — so it is provably unrelated for that game specifically.
`7523087` (Purple electing Michelangelo mid-turn, replacing Hammurabi) was
already individually verified in an earlier pass: every civil-costing
action that turn is independently correct, and the leader-replacement
refund nets to zero exactly as §9.1 predicts. **A genuinely unidentified
THIRD source, or something else — not diagnosed, not guessed at.**
`REPLAY_DEBUG=1`/`REPLAY_DEBUG_ALL=1` reproduce the full trace for both.

### Finding 2 (interleaving with no `EndTurn`, was 9/24): root-caused to Development of Civil Life's OUT-OF-TURN grant — a replayer/turn-model gap, not an engine rules bug, fixed in `replay.rs`

**Root cause, confirmed against real games, not the `politics_done`-vs-
global-`phase` theory this doc previously named as the most concrete lead**
(that theory was checked and does not explain the dominant shape — see
below). Development of Civil Life's grant (immediately above) is NOT scoped
to whoever prepared it, or to their own turn — it is banked on `p.
one_time_discount` for EVERY qualifying player the instant the event
resolves, and a real BGO player may spend their own banked grant WHENEVER
they like, including mid-ANOTHER-player's live turn, since the grant itself
carries no timing restriction once banked. **Every one of the 8 sample
games examined in detail that stopped on `decider != expected actor`
contains this event** (`7523354`, `7523355`, `7523809`, `7523350`,
`7522619`, `7523082`, `7523357`, `7522668`) — not a coincidence: e.g.
`7523355` line 34-35 —

```text
Purple builds Philosophy Purple spends 2 resources          <- Purple's own turn
Orange builds Philosophy Orange spends 2 resources          <- interjects, no EndTurn for Purple
Purple builds 1 stage of Library of Alexandria ...           <- Purple's turn resumes
```

— and the SAME shape covers BGO's `"<Color> discovers <Card> <Color> loses
N science"` phrasing (`corpus.rs`'s `"discovers "` prefix — `develop`, not
`build`), the dominant remaining sub-case once build/pop were handled: e.g.
`7523809` line 55, `"Orange discovers Alchemy Orange loses 3 science"` mid
Purple's turn, Alchemy having been taken into Orange's hand normally,
turns earlier.

**Why this is a replayer/turn-model gap, not an engine rules bug**:
`legal::legal_moves` (correctly, matching the real engine's design) only
ever computes ACTIONS for `state.current`/`state.decider()` — it has no
concept of "yes, but not your turn," because the base game has almost no
mechanic that needs one (every other action-phase move genuinely does
belong to whoever's turn it is). Development of Civil Life is the one
exception, and self-play never needs to reproduce an out-of-turn grant
faithfully (a bot with a banked, un-timed discount simply spends it on its
own next qualifying action, which is a safe, non-exploitable
simplification, not a strategy-relevant divergence) — the gap only shows up
when trying to replay a REAL human's exact, out-of-turn action sequence.

**Fixed in `replay.rs` only, no engine turn-model changes**: `civil_life_
move` (new helper) recognizes, for the journal's stated actor, whether they
have a live `one_time_discount` matching an `IncreasePopulation`/
`BuildBuilding`/`DevelopTechnology` line; if so, the main per-line loop
applies it directly via `apply::apply_free_civil_move` (bumped from
`pub(crate)` to `pub` — already actor-explicit and `state.current`-agnostic
by construction, since it existed for a DIFFERENT free-civil source, an
action card's ordered move) BEFORE `resolve_intervening`/`apply_one`, which
both assume the acting player IS the decider. Deliberately does NOT cover
`ActionClass::Develop` when the card is not already in the interjecting
player's grounded hand (`p.hand_civil.contains`) — an honest, narrower
residual left unhandled rather than guessed at.

**Verified against real data**: re-running the 24-game sample, `decider !=
expected actor` dropped from 9/24 to 1/24. The one remaining case
(`7523791`, line 89, `"Grey builds Religion"`) is a DIFFERENT, narrower bug
in the pre-existing `Development of Religion` (`ChoiceKind::FreeBuild`)
per-player draining order — not Civil Life, not diagnosed further this
pass (a single occurrence).

**The `politics_done`-vs-global-`phase` theory this doc previously flagged
as the most concrete lead was checked and does NOT explain the dominant
shape**: in every game examined, `resolve_intervening` reaches the
interjecting line with `state.pending` genuinely empty and `state.
decider()` genuinely still naming the ORIGINAL turn's player — there is no
live per-player political-decision signal being missed, because nothing
about this is a political decision at all. That theory may still explain
some OTHER, rarer interleaving shape (untested — no sample game needed it
once Civil Life's cases were excluded), but it is not the reason this
sample's stops happened. Documented here so nobody re-chases it as the
primary lead.

### Updated sample numbers after this pass

| | third pass | fourth pass (this one) |
|---|---|---|
| games complete | 0/24 | 0/24 |
| mean actions before stop | 51.0 | **63.7 (+25%)** |
| `decider != expected actor` (interleaving) | 9/24 | **1/24** |
| civil-action-budget shortfall (`civil_actions == 0`, unexplained) | 8/24 | **2/24** |
| build cost mismatch (unmodeled discount source) | 1/24 | 8/24 (pre-existing category, `docs/REPLAY.md`'s "gives up on" shape from the first pass — more visible now that games run deeper, not new; NOT investigated this pass) |
| `WonderStep` illegal (two different unresolved shapes) | 1/24 | 5/24 (newly reached this deep; NOT diagnosed this pass — likely a wonder-completion `Pending::Choice` and/or a blue-token/multi-stage cost gap, out of this pass's scope) |
| `FreeCivilAction` no-`Pending::Choice`-opened (Urban Growth) | ~2/24 | 2/24 (unchanged, pre-existing, still not investigated) |
| other, single-occurrence, not diagnosed | 3/24 | 3/24 (`PolPass` illegal, `PlayAction` illegal, a `Take` with many other CA-costing options still legal — each a single occurrence this pass did not chase) |

Every game reached the same or a strictly later stop point than the third
pass; none regressed. As with every prior pass, closing two walls exposed
the next ones (`WonderStep`/build-cost-mismatch categories, both
pre-existing shapes that simply could not fire before games ran this deep)
— expected, not new information, and reported honestly rather than
claimed as newly discovered bugs.

### What remains open going into a fifth pass

1. **The residual 2/24 civil-action-budget shortfall** (`7522632`,
   `7523087`) — confirmed NOT Civil Life, NOT Hammurabi, NOT any known
   `CardEffects`/`Special`. The single highest-value remaining lead: a
   THIRD unidentified action-economy source, reproducible with
   `REPLAY_DEBUG_ALL=1` on either game.
2. **The `build cost mismatch` category (8/24)**: an unmodeled discount
   source, echoing the first pass's Rich Land/Urban Growth discovery and
   this pass's own Finding 1b — worth checking whether it is ALSO a Civil
   Life-shaped gap (a build discount this binary isn't crediting) before
   assuming it is something new. Representative lines available for all 8
   games via this pass's own re-run.
3. **The two `WonderStep`-illegal shapes (5/24)**, one blocked by a live
   2-option `Pending::Choice`, one with many other CA-costing moves still
   legal — not investigated, not even loosely diagnosed, this pass.
4. **The single residual `decider != expected actor` case (`7523791`)** —
   a narrower bug in `Development of Religion`'s existing `FreeBuild`
   per-player draining order, not Civil Life.
5. Scaling the sample past 24 games remains blocked on the same scoping
   question the third pass raised: 0/24 complete, and a full BGO game gives
   every category above many chances to fire over 15-20+ rounds.

## Fifth pass: the "build cost mismatch" category fully closed (two real fixes), the two named residuals investigated further but still open, one new well-evidenced lead found

This pass's job was the fourth pass's open-items list, in priority order. Two
were closed outright (both confirmed causes, both fixed and tested); two
(the budget-shortfall residuals, the `Development of Religion` interleave)
were investigated in real depth but are reported honestly as STILL open,
with new evidence attached rather than a forced fix. **0/24 still complete**
(as every prior pass), but mean actions before a stop rose from 63.7 to
**73.5 (+15%)**, and the `WonderStep`/`build cost mismatch` blocker
categories the fourth pass left unresolved are gone -- replaced by new,
deeper stops (expected, the same "closing one wall exposes the next"
pattern every prior pass reports).

### Finding A (ENGINE BUG, confirmed, fixed, tested): Development of Civil Life's discount was modeled as three independent grants; the real card is ONE mutually-exclusive choice

This is a correction of a bug the fourth pass itself introduced while fixing
a different bug. The card ("Development of Civil Life", "Development of
Civilization" in BGO's UI) reads "Players may **either** increase its
population; **or** build...; **or** develop a technology... 1 [resource]
less" -- an either/or/or list, one choice among three, not a grant of three
independent one-time discounts. `state.rs`'s prior doc comment (added
2026-08-05) read the JSON schema's three sub-keys as license for three
independent grants, each cleared only when its own field was spent -- a
plausible-looking misreading that this pass found and reverted.

**Confirmed wrong by replaying three separate real BGO games**, each
showing the same shape: a human spends ONE of the three discounts (pop,
build, or develop) and later, the same turn or a later one, pays FULL,
undiscounted price for an action of a DIFFERENT type that the old
independent-grants model predicted should still be discounted --

- `7523357`: Grey spends the `pop_food` discount (`"increases population
  ... spends 1 food"`, not the turn's usual 2), then later pays full price
  (2, not 1) for a Bronze mine build the old model said should still be
  discounted.
- `7523350`: Orange spends `develop_science` on Printing Press (2 science,
  not the printed 3), then pays full price for a later Bronze build.
- `7522619`: Green spends `pop_food` (1 food, not 2), then pays full price
  for a later Religion build (3 resources, not the discounted 2).

**Fixed** via `OneTimeDiscount::exhaust()` (`state.rs`), called from all
three consumption sites (`economy::increase_population`,
`apply::do_build`, `apply::h_develop`) whenever THAT site's own field was
actually live -- clearing all three fields together, not just one, and
specifically NOT clearing anything when the acting site's own field was
already 0 (so an ordinary, unrelated action never wipes a sibling grant
this player hasn't spent yet). **Tests** (both confirmed to fail against the
reverted old behaviour and pass after, by temporarily restoring the old
independent-clearing code, rerunning, and reverting back):
`economy::tests::increase_population_exhausts_the_whole_civil_life_grant_not_just_pop_food`,
`apply::tests::spending_any_one_civil_life_discount_exhausts_the_whole_grant`
(this one REPLACES a test that asserted the old, wrong behaviour by name --
`one_time_discount_categories_are_consumed_independently` no longer exists).

**Sibling grep**: `oneTimeDiscount`/`gainFoodOrResources`-shaped "list of
alternatives, one choice" card text is rare -- grepped `card_table.rs` and
`data/*.json` for every other "either/or" multi-field one-time grant;
Development of Civil Life is the ONLY base-game card with an
`oneTimeDiscount` payload at all, so there is no sibling site sharing this
exact shape to fix. (A structurally adjacent but semantically DIFFERENT
case, `ChoiceKind::FoodOrRes`, is Finding C below -- a replayer gap, not
this bug.)

### Finding B (`replay.rs`, not engine): a unit build/upgrade's `p.mil_discount`-funded portion is a SEPARATE journal clause, never summed into the parsed cost

Closes the other half of the fourth pass's 8/24 "build cost mismatch"
category (`7523341`, `7522668`, `7522616` -- the three where the engine's
computed cost was HIGHER than what the journal showed paid, the opposite
direction from Finding A's cases). BGO logs a unit build/upgrade's total
resource payment as up to two clauses: `"loses N military resource"` (the
portion covered by `p.mil_discount`, the real per-turn pool Patriotism /
Wave of Nationalism / Military Build-Up grant, `costs::spend_mil_discount`)
and `"spends N resource"` (the rest, from the ordinary pool) -- e.g. `"Purple
builds Warrior Purple loses 1 military resource; Purple spends 1
resource"`. `replay.rs`'s build-cost cross-check was reading `spends` alone,
silently under-counting the total by exactly the `"loses"` amount (which
`costs::build_cost_for` never subtracts for units on purpose -- Civil
Life's `build_resources` discount is farm/mine/urban only, and `mil_discount`
is netted off at APPLY time, not at this check's comparison baseline).
Confirmed by exact arithmetic across every sampled instance: `loses` +
`spends` always equals `build_cost_for`'s raw printed cost, including lines
with NO Patriotism-style grant visible anywhere earlier in that player's
turn (an unexplained baseline case -- see "What remains open," below).

**Fixed** by `total_paid_for_build` (`lost_military_resource` +
`spent_resources`, summed). **Tests**:
`lost_military_resource_reads_a_loses_military_resource_clause`,
`lost_military_resource_ignores_an_unrelated_loses_clause`,
`total_paid_for_build_sums_the_military_resource_and_spends_clauses`
(`rust/src/bin/replay.rs`).

**Together, Findings A and B closed all 8/24 of the fourth pass's "build
cost mismatch" games** -- 5 were Finding A (Bronze/Bronze/Philosophy/
Religion/Religion), 3 were Finding B (Warriors/Swordsmen/Warriors).

### Finding C (`replay.rs`, not engine): Reserves' `FoodOrRes` choice was never resolved -- the exact GainBlock bug shape, missed at the one OTHER site it applies

The fourth pass fixed `ChoiceKind::GainBlock` (an event's "gain 2 resources
or 2 food, player's choice") going unresolved and blocking a later action.
`ChoiceKind::FoodOrRes` is the SAME journal shape -- a standalone-looking
`"produces N food"` / `"produces N resources"` bookkeeping pick -- but is
opened by a DIFFERENT mechanic (Reserves, `Special::GainFoodOrResources`, an
action card's own ordered gain, §3.11) and was never covered. Grepped
`card_table.rs` for every `GainFoodOrResources` site to confirm Reserves is
the ONLY base-game card with this shape (never an event, so never opened
for "every qualifying player" the way GainBlock is -- always scoped to the
one player who played it). **Found by replaying `7523818`**: an unresolved
`FoodOrRes` pending blocked a `WonderStep` several lines later.

This did NOT get folded into the existing `GainBlock` drain code, because
the journal shape is subtly different in a way that matters: GainBlock's
pick is always its OWN standalone journal ROW (`"<Color> produces N
food"`, nothing else on the line); Reserves' pick is glued onto the SAME
row as the `"plays Reserves"` line with no separating punctuation
(`"Orange plays Reserves Orange produces 2 resources"` -- confirmed
against the FULL corpus: 4157 of 4158 `"plays Reserves"` lines have this
exact glued shape). **Fixed** in `ActionClass::PlayActionCard`'s own
handler (which has the raw line text `GainBlock`'s prescan-based drain
doesn't need), via a new `trailing_produces` helper that reads the LAST
`"produces"` clause anywhere in the line. **Tests**:
`trailing_produces_reads_a_produces_clause_glued_onto_a_play_line`,
`trailing_produces_is_none_with_no_produces_clause_at_all`.

### The two residual budget-shortfall games (`7522632`, `7523087`): still open, but a new, corpus-wide lead found

Re-examined both with `REPLAY_DEBUG_ALL=1` from scratch (not assuming the
fourth pass's diagnosis). Confirmed, again, NOT Civil Life (in `7522632`
the event doesn't even fire until 3 rounds after the shortfall), NOT
Hammurabi's MA-as-CA conversion (the leader is gone by the time of the
shortfall in both games), NOT Michelangelo's wonder-surcharge waiver (it's
individually verified correct in `7523087` -- Colossus's own take costs
exactly `row_cost`, matching its printed clause). Every individual
CA-costing action's OWN printed cost matches this engine's own formula; the
SUM simply exceeds the government's printed budget by exactly 1 in both
games.

**A new, well-evidenced lead**: in `7522632`, the specific failing action
is `"Orange takes Taj Mahal in hand"` -- with **no `"uses N civil action"`
clause at all**, unlike the overwhelming majority of civil-row takes.
Widening the search to the full corpus (all 1,011 games, not just the
sample) turns this from a one-off oddity into a real, structured signal:

| card | total takes | no CA clause | rate |
|---|---|---|---|
| Taj Mahal | 317 | 150 | 47% |
| Leonardo Da Vinci | 700 | 123 | 18% |
| Michelangelo | 414 | 57 | 14% |
| Great Wall | 318 | 2 | 0.6% |
| Colossus | 274 | 7 | 2.6% |
| Hanging Gardens | 511 | 2 | 0.4% |
| Pyramids | 820 | 3 | 0.4% |
| Hammurabi | 716 | 0 | 0% |

This is not noise: three specific cards (two Leaders, one Wonder) show the
missing clause 14-47% of the time; every other sampled Leader/Wonder shows
it under 3% of the time, close to zero. `7523347` (this SAME sample's
`Take{Taj Mahal}` slot-specific mismatch, distinct from the two named
budget-shortfall games) hits the identical `"takes Taj Mahal in hand"`
no-clause line, a THIRD independent occurrence. When a clause IS present
for Taj Mahal, the cost varies (1/2/3/4 CA observed), ruling out "Taj Mahal
has a fixed printed take cost BGO sometimes omits." **Not diagnosed**:
several candidate explanations were considered and none confirmed --
Michelangelo being in play does NOT explain it alone (one of the five
sampled no-clause instances has no leader-election anywhere nearby in the
same turn); it is not simply "first action of the turn" (checked against
counter-examples on both sides). Flagged here, with the full cross-card
frequency table, as the single most promising remaining lead for the two
residual budget-shortfall games specifically -- **whatever this pattern
turns out to mean, it directly overlaps both `7522632`'s and `7523347`'s
proximate failing lines**, though `7523087`'s own failure (a Colossus take
WITH a normal, correctly-priced clause) shows it cannot be the WHOLE
explanation for that game. Per the standing rule, NOT guessed at or
force-fixed.

### The `Development of Religion` interleave (`7523791`): confirmed to be a FOUR-PLAYER interleave, not a two-player one -- root cause still open

The fourth pass's framing ("a narrower bug in the FreeBuild per-player
draining order") undersold the shape: the real journal shows FOUR
consecutive `"<Color> builds Religion"` lines from four DIFFERENT players
(Green, Grey, Orange, Purple in that order) with **no `EndTurn` anywhere
between any of them** -- every qualifying player spending their own
`Development of Religion`-granted `FreeBuild` the instant the event
resolves, not in turn order and not gated on whose turn it nominally is
(same "banked, spendable whenever" shape Finding 2 of the fourth pass
established for Development of Civil Life, but for a DIFFERENT event/
mechanic). `resolve_intervening`'s existing `ChoiceKind::FreeBuild` handling
already drains regardless of whose turn it is, matching by whether the
UPCOMING line's card is among the pending's own options -- and the first
three players' builds (Green, Grey, Orange) resolve fine; only the FOURTH
(Purple) is where `state.decider()` stops naming the actor the journal
names next. Not root-caused this pass: the leading candidate (the
FreeBuild queue's internal order not matching the journal's per-player
resolution order once more than 2-3 players are queued at once) was not
confirmed against the actual `Pending` stack ordering in time. Flagged
open, not guessed at.

### Newly reached, not diagnosed this pass (expected -- closing walls exposes new ones)

Every one of these is a stop point only reachable now that Findings A/B/C
let games run substantially deeper; none was reachable before this pass on
this sample. Each is a single or double occurrence, too thin to responsibly
diagnose in the time this pass had left:

- **Colonize bidding** (`7523818`, newly reached): `Bid { n: 3 }` illegal,
  only `BidPass` offered -- likely related to the pre-existing "colonize
  sacrifice specifics" approximation this file already documents as
  unexercised against real data until now.
- **Aggression defense with a committed defense card** (`7523355`):
  exactly the ALREADY-DOCUMENTED "gives up on" case (BGO logs only a
  count, never identities) -- genuinely unrecoverable, not a bug.
- **`Rich Land` building Iron, `ParserGap`** (`7523354`): `free_civil_build_move`
  does not recognize an Iron build/upgrade as one of Rich Land's offered
  options.
- **`Upgrade` blocked by a live 2-3 option `Pending::Choice`** (`7522668`,
  `7522652`): same general SHAPE as Finding C (a stale, un-drained pending
  blocking a later action) but for `Upgrade` specifically -- not confirmed
  to be the same root cause.
- **`Pop` blocked by a live 5-option `Pending::Choice`** (`7522619`): same
  shape again, not confirmed.
- **`decider != expected actor`** (`7523350`, newly reached): a fresh
  instance in a DIFFERENT game, immediately after a Civil-Life-discounted
  `Develop`; not confirmed to be the same root cause as `7523791`'s.

### Updated sample numbers after this pass

| | fourth pass | fifth pass (this one) |
|---|---|---|
| games complete | 0/24 | 0/24 |
| mean actions before stop | 63.7 | **73.5 (+15%)** |
| `build cost mismatch` (unmodeled discount) | 8/24 | **0/24 (closed)** |
| `WonderStep` illegal (2 shapes) | 5/24 | **0/24 as originally shaped** (Finding C fixed the 3 Choice-blocked cases; the 2 no-option cases are unchanged, still open, see fourth pass) |
| civil-action-budget shortfall, unexplained | 2/24 | 2/24 (same two games, new lead found, not resolved) |
| `decider != expected actor` (interleaving) | 1/24 | 2/24 (`7523791` root-caused further but not fixed; `7523350` newly reached) |

Every game reached the same or a strictly later stop point than the fourth
pass; none regressed.

### Final-score cross-check: still not reached

`game::scores(&state)` vs `index.tsv`'s `results` column still could not be
run -- 0/24 games in this sample reached `state.game_over`. Unchanged from
every prior pass's own note on this.

### What remains open going into a sixth pass

1. **The two residual budget-shortfall games** (`7522632`, `7523087`) --
   see the Taj Mahal/Leonardo Da Vinci/Michelangelo "missing CA clause"
   lead above; the single highest-value unstarted thread (a corpus-wide,
   not sample-local, pattern spanning hundreds of occurrences).
2. **The `Development of Religion` four-player interleave** (`7523791`) --
   root-caused to a `FreeBuild` queue/drain-order question but not fixed.
3. **The `Rich Land` building Iron `ParserGap`** (`7523354`) and the two
   remaining no-option `WonderStep` shapes (`7523809`, `7523353`) -- single
   occurrences, not investigated.
4. **The new `Upgrade`/`Pop` Choice-blocked shapes** (`7522668`, `7522652`,
   `7522619`) -- worth checking whether they share Finding C's root shape
   (an un-drained `Pending::Choice` from an action card's own ordered gain)
   before assuming they are something new.
5. Colonize bidding (`7523818`) and the colonize-approximation's own
   accuracy remain entirely unexercised against real bid amounts.
6. Scaling the sample past 24 games remains blocked on the same scoping
   question the third pass raised.

## Sixth pass: the civil-action-budget bucket, root-caused to a THIRD engine bug -- and the wonder-surcharge rules question settled against the corpus

This pass had one job: the largest single stop bucket, `IllegalMove: Take`
at **143 of 1,011 games** (`rust/src/bin/replaystats.rs`, the ranked
histogram every number below comes from). The whole bucket was reported as
a civil-action budget shortfall -- the replayer believing the human had no
civil actions left while the journal shows them taking a card anyway.

| | fifth pass | sixth pass (this one) |
|---|---|---|
| mean rounds reached (of 19.27 played) | 5.41 | **5.72 after the engine fix, 5.71 after the replayer fix** |
| decisions recorded in Age II or later | 2.4% | **3.5%** |
| `IllegalMove: Take` | 143 | **17** |
| `ParserGap: Taj Mahal's take cost ...` | 8 | 81 (see below -- this GREW on purpose) |

### Finding 1 (ENGINE BUG, confirmed, fixed, tested): Hammurabi's MA-as-CA conversion was forfeited by replacing him mid-turn

`costs::pay_ca` spends Hammurabi's once-per-turn conversion **lazily** --
it only reaches for a military action once the printed civil-action pool
runs dry -- and both it and `costs::spare_ca` gated the conversion on
`p.leader` still BEING Hammurabi. A player who spent their last printed
civil action ON the leader replacement therefore lost the conversion
before ever being offered it.

The rulebook is explicit that this is legal (RB, "Replacing a Leader",
transcribed in `sources/ubg_the-second-round.txt`): **"You are allowed to
use the benefit of a leader and then replace him or her on the same
turn."** Hammurabi's own 2015 text is "On your turn, you may use one
military action as one civil action" (`sources/namu_heroes.txt`,
`data/cards_wonders_leaders.json`).

**The corpus evidence, which is what found it.** Of the 143 games in the
bucket, 109 stopped **short by exactly one civil action** against a cost
the journal itself printed -- and **103 of those 109 are turns in which
the human replaced Hammurabi**, against Hammurabi being only 39%
(577/1495) of all Age A leader replacements corpus-wide. 107 of the 109
still had a military action in hand to convert. A leader-agnostic cause
(e.g. the replacement refund being mis-netted, the standing alternative
hypothesis) predicts 39%, not 94.5%.

**Fixed** with `PlayerState::hammurabi_replaced_this_turn`, set in
`apply::h_play_leader` when the leader being replaced is Hammurabi and
cleared alongside `hammurabi_used` in `economy::end_of_turn`;
`costs::hammurabi_conversion_available` is now the single source of truth
both `spare_ca` and `pay_ca` read. `take_gate`'s SEPARATE
`leaderTakeCivilActionDiscount` deliberately still keys off the live
leader -- that one is a continuous in-play effect, not a once-per-turn
use. **Test**: `apply::tests::replacing_hammurabi_mid_turn_keeps_his_
military_action_as_civil_action_conversion_for_the_rest_of_the_turn`,
confirmed to fail with the `h_play_leader` flag reverted (`left: 1,
right: 2`) and pass after.

This one fix took the bucket from 143 to 57 games, mean rounds from 5.41
to 5.72, and the Age II+ decision share from 2.4% to 3.5%.

### The wonder take-surcharge: SETTLED. The rule is exactly as modeled -- a wonder completed EARLIER THE SAME TURN still counts

The fifth pass left this open on two contradicting games, with `7523353`
suggesting the `+1 CA per already-completed wonder` surcharge (§2.4)
might not apply to a wonder completed earlier in the same turn. Two data
points are not a ruling, so this pass measured the whole population:
**every wonder take in all 1,011 journals that carries an explicit cost
clause, 6,757 of them**, excluding takes made with Michelangelo in play
(his printed text waives the surcharge). Summing BOTH cost clauses on the
line (`"uses N civil action; ... uses N military action"` -- a Hammurabi
conversion splits a take's cost across two clauses, and reading only the
civil one manufactures fake violations) and subtracting each player's
completed-wonder count leaves the implied ROW POSITION cost, which the
rules confine to 1, 2 or 3:

| implied position cost | Model A: every completed wonder counts (current engine) | Model B: wonders completed earlier the same turn are exempt |
|---|---|---|
| -1 (impossible) | 10 | 5 |
| 0 (impossible) | 21 | 22 |
| 1 | 4758 | 4421 |
| 2 | 1604 | 1885 |
| 3 | 364 | 415 |
| 4 (impossible) | 0 | **9** |
| **violations** | **31 (0.46%)** | 36 (0.53%) |

Model A -- what the engine already does -- is consistent with **6,726 of
6,757** real human wonder takes. Model B is not merely no better, it is
**affirmatively refuted**: it implies 9 takes cost more civil actions
than the most expensive row position exists to charge. So the surcharge
counts a wonder completed earlier the same turn, `costs::take_cost` is
right, and **nothing was changed here**.

And the residual is not about the surcharge at all: **all 31 of Model A's
violations are Taj Mahal**, and 22 of the 31 involve no same-turn
completion whatsoever, so the same-turn theory would not have explained
them either. Every other wonder in the game -- Pyramids, Hanging Gardens,
Colossus, Library of Alexandria, Great Wall, St. Peter's Basilica,
Universitas Carolina, and all the Age II-IV wonders -- is 100% consistent
across all 6,726 of its takes.

### Finding 2 (replayer, not engine): a take line with NO cost clause cost ZERO actions -- it is not an unknown cost

`ground_row_slot` took `Option<i32>` for the journal's stated cost and
read a missing `"uses N ... action"` clause as "unknown", falling through
to its "first ungrounded slot" path -- a silent guess at which slot the
human paid for, the exact shape the fifth pass removed for the
known-cost case.

No clause means zero, and there is a printed card ability that produces
zero. Of the corpus's 88,432 take lines, **483 carry no clause at all**,
and **1,639 of the 1,641 no-civil-clause lines fall in game-age I** --
Hammurabi's window. 333 of the 483 are LEADER takes and **every single
one of the 333 has Hammurabi in play**: his
`leaderTakeCivilActionDiscount` cancels the 1 CA of a leader sitting in
one of the row's five cheapest slots. The engine already priced this
correctly; only the replayer could not read it. **Test**:
`replay_common::tests::a_take_line_with_no_uses_clause_at_all_cost_zero_
actions_not_an_unknown_cost`.

Mean rounds and the Age II+ share are FLAT across this change (5.72 ->
5.71, 3.5% -> 3.5%) and that is the honest report: it trades a lucky
guess for a truthful stop rather than buying depth. What it does buy is
attribution -- `IllegalMove: Take` drops 57 -> 17, and the Taj Mahal
anomaly stops hiding inside mis-grounded row slots.

### Open, with a documented population: Taj Mahal is priced below anything the rules allow, and only Taj Mahal

The single biggest remaining thread out of this bucket, and the reason
the `ParserGap: Taj Mahal` bucket deliberately grew from 8 games to 81.
**181 of the corpus's 317 Taj Mahal takes (57%) cost less than the
cheapest row position plus this player's own wonder surcharge**:

- **150 with no cost clause at all**, i.e. 0 civil actions -- impossible
  for a wonder, whose cheapest possible take is 1. Reconstructing the CA
  ledger by hand for four of them (`7521302` r4, `7521361` r5, `7521377`
  r4, `7523353` r5) confirms the action really was free: in each, every
  OTHER civil-costing action that turn already exactly consumes the
  government's printed budget plus every identified bonus, with nothing
  left for the Taj Mahal.
- **31 more with an explicit clause below the minimum**, the Model A
  violations above.

The one strong signal found: **149 of the 150 clause-less Taj Mahal takes
happen in a turn in which that player elected a leader**, usually on the
line immediately before. Candidate explanations checked and rejected: it
is not Michelangelo's surcharge waiver (only 49 of the 150 have him, and
his waiver cannot take a cost below 1 anyway); it is not Hammurabi's
leader-take discount (Taj Mahal is a wonder, not a leader, and these
players' leaders are Age I ones); it is not BGO omitting the surcharge
generally (takes costing 4+ CA occur, and no row position charges 4); and
it is not a general clause-omission bug, since 6,726 other wonder takes
price perfectly. **Not diagnosed, not guessed at.** The next pass should
start here: whatever this is, it is card-specific, it is worth 81 games,
and it is the only thing left in the take bucket that is not already
attributed.

### Also in this bucket, but NOT budget shortfalls (re-triaged, left alone)

15 of the original 143 turned out not to be action-economy problems at
all: the acting player had ample civil actions (4-6, against an observed
cost of 1-3) and `legal_moves` offered no `Take` because the whole move
list was a live `Pending` -- a colonize `Bid { n }` list, or an
un-drained 2-option `Choose`. Those belong to the colonize-bidding and
stale-pending threads other passes own, not to this one, and they are
**exactly the 17 games left in the bucket** (15 of the 17 are that
identified set; `7522905` and `7523281` were newly reached and not
triaged). In other words the budget-shortfall shape this bucket was
named for is now fully accounted for: fixed (Finding 1), or moved into
the Taj Mahal `ParserGap` bucket above, or pushed deeper into other
categories. Reproducible with `REPLAY_DEBUG=1`.

## Seventh pass: the Taj Mahal anomaly, SETTLED -- it is a printed card ability the engine never had

The sixth pass closed its section with the only thing left in the take
bucket that was not attributed: **181 of Taj Mahal's 317 corpus takes (57%)
cost less than the cheapest row position plus that player's own wonder
surcharge allows**, 150 of them free outright, with the one strong signal
being that 149 of those 150 fall in a turn where the player elected a
leader. It is not a replayer bug, not a BGO quirk, and not the surcharge.

### The mechanism: Taj Mahal's own 2015 card text

> **"If you replaced your leader this turn, taking this wonder costs you 2
> civil actions less."**

That clause is printed on the card in *A New Story of Civilization* and was
missing from `data/cards_wonders_leaders.json` entirely (the entry carried
only "+3 culture production, +1 blue token", which is the whole of the 2006
card). Two independent sources already in `sources/`, neither of them this
corpus:

- **`sources/bga_throughtheages_material.inc.php`, card id 98** -- Board
  Game Arena's own implementation of the 2015 edition. The sentence above is
  its `text` field verbatim, alongside `'culture' => 3` and `'tokendelta' =>
  array('blue' => 1)`.
- **`sources/namu_wonders.txt`** -- the Korean wiki's new-edition entry:
  "(New Edition) Score increase rate +3, blue tokens +1. If you switched
  leaders this turn, take 2 fewer actions to draw this card", and separately
  "The play of picking up Michelangelo cheaply as Hammurabi and then using
  Michelangelo as a hero to pick up the Taj Mahal for free or for 1 token is
  quite intense" -- i.e. the free take is a known, deliberate line of play,
  not an artifact.

This is the ONLY thing in the base game that can make a wonder take cost
zero. Michelangelo's waiver cancels the surcharge but can never go below the
row's own minimum of 1; Hammurabi's `leaderTakeCivilActionDiscount` is
leaders-only. Taj Mahal's is a flat subtraction off the whole cost and
routinely hits the `max(0)` clamp.

### The population, and how completely the clause explains it

All 317 Taj Mahal takes in the 1,011-game corpus, cross-tabulated by whether
the acting player had already replaced a leader earlier in that same turn
(the journal's `"<Colour> elects <New> <Old> dies"` shape -- a *first* leader
prints no `dies` clause and is not a replacement):

| earlier in the turn | takes | free (no cost clause) | paid |
|---|---|---|---|
| replaced a leader | 184 | **149** | 35 |
| elected a FIRST leader (no replacement) | 8 | 0 | 8 |
| no election at all | 125 | 1 | 124 |

The single free take in the bottom row (`7523665`) is a journal ordering tie,
not a counterexample: its line and the replacement that licenses it carry the
**same timestamp to the second**, and the same turn contains a `"puts Taj
Mahal back in the row"` printed *before* the take it undoes. BGO's journal is
sorted by a one-second-resolution timestamp and is not stably ordered within
a second.

Scoring every take against the two models -- implied row position must be 1,
2 or 3, and a free take is consistent whenever the model's cost lands at or
below 0 -- with Michelangelo's waiver applied where he is in play:

| | violations of the 1/2/3 row-position bound |
|---|---|
| sixth pass's model (`row + surcharge`) | **181 of 317 (57%)** |
| with Taj Mahal's printed clause | **1 of 317 (0.3%)** |

The 35 paid takes made after a replacement are not counterexamples either:
under the clause they simply imply a more expensive row slot (cost + 2 -
surcharge), and their implied positions land inside 1-3.

**The negative control the corpus supplies for free**: of ~6,700 takes of the
other fifteen wonders, **zero** carry no cost clause -- including the 129
Taj-Mahal-adjacent case of a wonder taken on the line immediately after an
election. Whatever this is, it is printed on exactly one card, and the data
says so before the card text does.

Two secondary facts fall out of the same table, both consistent with the
clause being a real and known line of play: Taj Mahal is taken in a turn
containing an election **64%** of the time, against 7.5-27% for every other
wonder; and this is the only wonder whose takes cluster in rounds 4-7 behind
an Age I leader swap.

### Finding (ENGINE BUG, confirmed, fixed, tested)

`costs::take_cost` and `costs::can_take_gated` both priced the row without
the clause, so `legal::legal_moves` refused to offer a Taj Mahal take the
human could demonstrably afford -- the `ParserGap: Taj Mahal's take cost ...`
bucket the sixth pass deliberately grew to 81 games.

**Fixed** by giving the card its ability in the data
(`takeCivilActionDiscountIfLeaderReplacedThisTurn: 2` ->
`Special::TakeCivilActionDiscountIfLeaderReplacedThisTurn`, the only card in
the base game that carries it) and reading it in both pricing paths off the
card **sitting in the row**, gated on a new per-turn
`PlayerState::replaced_leader_this_turn` (set in `apply::h_play_leader` only
when a leader was actually swapped out, cleared in `economy::end_of_turn`
beside `hammurabi_replaced_this_turn`, which it is strictly weaker than).

**Tests**, all four confirmed to fail with the fix reverted and pass with it:
`costs::tests::taking_taj_mahal_costs_two_civil_actions_less_when_a_leader_was_replaced_this_turn`
(2 -> 0, the clamp case),
`costs::tests::taking_taj_mahal_from_an_expensive_slot_after_a_replacement_still_costs_the_remainder`
(3 -> 1, proving it is a flat subtraction and not a surcharge waiver),
`costs::tests::move_generation_offers_taj_mahal_with_no_civil_actions_left_after_a_replacement`
(the `can_take_gated` half -- wiring only `take_cost` leaves the move
ungenerated), and
`apply::tests::playing_a_first_leader_into_an_empty_slot_is_not_a_replacement_but_swapping_one_is`.
Two more pass either way, on purpose, pinning the negative controls:
`no_wonder_other_than_taj_mahal_is_discounted_by_a_leader_replacement` and
`taking_taj_mahal_costs_full_price_when_no_leader_was_replaced_this_turn`.

### Measurement (`replaystats`, full 1,011-game corpus)

| | sixth pass | seventh pass (this one) |
|---|---|---|
| mean rounds reached (of 19.27 played) | 5.71 | **5.84** |
| decisions recorded in Age II or later | 3.5% | **3.9%** |
| decision points recorded | 67,574 | **69,720** |
| `ParserGap: Taj Mahal's take cost ...` | 81 | **0 (bucket gone)** |

Every other bucket grew, which is this file's usual pattern and not a
regression: `StuckPending: decider != expected actor` 137 -> 148,
`IllegalMove: PlayAction` 113 -> 122, `Pop` 110 -> 117, `WonderStep` 102 ->
111, `Build` 94 -> 103, `Upgrade` 58 -> 63, `Take` 17 -> 22. The 81 games
that used to stop on Taj Mahal now run past it into whatever was next.

### What this says about the method, and what to do next

The clause was sitting in `sources/` the whole time, in two files, and no
prior pass looked -- every pass instead reasoned from the corpus about what
mechanic *could* produce a free take. What made the difference here was
running the search in both directions at once: the corpus said "only this
one card, only after a leader swap, and by 2", which is specific enough to
recognise the right sentence the moment you read it.

That generalises directly: **the card data is a plausible suspect whenever a
single named card misbehaves**, and it is cheap to check.

**And it was checked, this pass, rather than left as a suggestion**: every
card in `sources/bga_throughtheages_material.inc.php` that carries a `text`
field (205 of them) was diffed against `data/*.json` -- all 16 wonders and
all 24 leaders read individually, the rest filtered for an our-side text
materially shorter than BGA's. **Taj Mahal was the only card with a
mechanical clause missing.** The four apparent misses are name spellings
(`J.S. Bach`, `Leonardo Da Vinci`, `Maximillien Robespierre`, `Ocean Liner
Service`) and the three apparent text gaps are terser paraphrases of the same
rule (Development of Markets, Iconoclasm, Impact of Technology). That is a
real negative result and worth recording so nobody re-runs it: the remaining
`replaystats` buckets are not more missing card abilities.

## Eighth pass: the hidden-`PrepareEvent` wall, closed — BGO logs every preparation

The premise every earlier pass built on ("BGO's journal never logs who
prepared an event") was simply **wrong**, and it cost this binary a lot. Every
preparation is one journal line:

```text
Orange plays event Orange scores 1 culture; Current event:; A / Development of Settlement; ...
```

which names **who** (the line's actor), **which age of card** they prepared
(`apply::h_prepare_event` scores exactly `card.level()`, so the culture clause
IS the age — never 0 anywhere in the corpus, Age A events being setup-only),
and **what the reveal turned up**. 17,889 `"plays event"` lines, 17,889
`"Current event:"` clauses, never one without the other.

`replay.rs` had been guessing forward instead: a hidden `PrepareEvent` at
every Politics decision no line explained, satisfied by popping the next
observed reveal off a FIFO. Most of those decisions are ordinary passes, so
events fired turns early for the wrong player (2p game `7522647`: Development
of Science at round 4, journal says round 10) and everything downstream
desynchronised.

### The pile model, verified against the whole corpus

`rust/src/event_plan.rs` now solves the record as one whole-game constraint
problem. BGO logs each recycle (`"Future events are now current events."`,
rendered before the `"Current event:"` clause on the line whose pop emptied
the pile — i.e. `reveal_current_event`'s own TRAILING recycle), which cuts the
reveal sequence into piles. Each pile is by construction exactly the cards
prepared while the previous one was consumed, so:

> the multiset of ages revealed in pile `b+1` must equal the multiset of
> `"scores N culture"` values on the preparations made during pile `b`.

Over the 1,011 games that holds in **3,291 of 3,291** windows (2,283 complete
piles exact, 1,008 truncated final piles consistent), with the setup pile
measuring `num_players + 2` in every game. Nothing is fitted; a violation is
`MismatchKind::EventPlanInfeasible` and stops the game. **Zero games hit it.**

What stays underdetermined: within one pile and one age the recycle shuffle
destroys the order, so which same-age preparation became which same-age reveal
is unrecoverable — and irrelevant, since the pile is a set and the reveal
ORDER is read from the journal. Tie-break is positional; the only state it can
touch is `seeded_by`. The setup pile and every recycle are now GROUNDED to the
journal's reveal order (contents from the engine, only the never-logged
shuffle order replaced), which turns the pre-reveal "is the right card on
top?" check into a real test of the model rather than a re-forcing of it.

Also fixed in passing: territory families recur across ages under one printed
name (`"Vast Territory"` at Age I AND Age II) and `card_table.rs`
disambiguates with a `" (I)"`/`" (II)"` suffix BGO never prints — but BGO does
print the age in the same clause, so the lookup uses both now. The old prescan
silently resolved to whichever age came first.

### ENGINE BUG: Julius Caesar's once-per-game was spent by DECLINING it

Chasing the residual `PolPass` bucket turned up a real rules bug. Printed text
(`sources/bga_throughtheages_material.inc.php`): *"After you play a political
action, you may play another political action. This ability can be used only
once per game."* `apply::end_politics` got both halves wrong — it spent the
once-per-game whenever the second political action resolved, **including when
that action was a pass**, and it armed a second action after a pass as the
FIRST action, which the text does not grant.

The discriminating evidence is game `7523338`: the same player is offered and
declines the second political action on rounds 3, 4, 5 and 7 and still holds
the ability, which is impossible under the old model. (The corpus-wide counts
— 142 player-games with exactly one double, no offer ever recurring after a
double is used — corroborate but do not discriminate: they are consistent with
once-per-game either way.) `end_politics` now takes a
`PoliticalAction::Played/Passed`. Python has the same bug; the printed card is
the oracle.

Two consequences for `replay.rs`, both fixed with the engine change:

- a human who declines the second action leaves BGO's `"passes Political
  Phase"` line wherever they clicked it, routinely AFTER some of their own
  Action-phase lines — which the engine cannot make legal until politics
  closes. Such a line is now that pass's late confirmation, tracked per seat
  and consumed one-for-one.
- the `"plays event"` line is skipped as a confirmation, so its preparation is
  applied at the NEXT line — which with Caesar armed is routinely that same
  player's own pass line. An owed preparation now outranks
  `resolve_intervening`'s explicit-political-line fast path, instead of the
  pass being applied as the FIRST political action and the preparation being
  stranded at the head of the queue (game `7522650`).

### Measurement (`replaystats`, full 1,011-game corpus)

| | seventh pass | this pass |
|---|---|---|
| mean rounds reached (of 19.27) | 5.90 | **6.37** |
| decisions in Age II or later | 4.6% | **5.5%** |
| `StuckPending: decider != expected actor` | 94 | **14** |
| `IllegalMove: PolPass` | 64 | **9** |
| `EventPlanInfeasible` | — | **0** |
| games completed | 0 | 0 |

Still 0 complete, so `analysis/index.tsv`'s final scores remain un-cross-
checked. The two residual politics stops are no longer event attribution:
`7523338` line 174 is an aggression's spoils `Choose` left unresolved, and
`7523657` line 143 is a colonize force left undrained when the acting player
IS the decider (`auto_drain_colonize` only runs on the `decider != expected
actor` branch). Both are in other buckets' territory.

### ENGINE BUG: Trade Routes Agreement's food<->resource substitution was computed and discarded

Found chasing the `IllegalMove: Pop` bucket (166 games, `docs/RULES_SPEC.md`
§5.9). `effects::compute` already summed the pact's grant into
`Stats.food_as_resource`/`resource_as_food` ("Civilization A can use 1 food
as 1 resource during its turn" / the B-side mirror,
`bga_throughtheages_material.inc.php`), but nothing ever read those two
fields — no payment path could use them, and `bots/board_yields.rs` valued
the pact at zero. Fixed engine-side with two new opt-in moves
(`Move::TradeFoodAsResource`/`TradeResourceAsFood` — not folded silently into
`Pop`/`Build`/`Upgrade`'s own cost, since this is the player's choice, not an
automatic discount) and a per-turn allowance
(`PlayerState::trade_*_used_this_turn`, cleared in `economy::end_of_turn`
like `replaced_leader_this_turn`). Confirmed against real games (7522168,
7521605, 7523242, 7523541: a human visibly paying part of a Pop cost in
resources). `rust/src/costs.rs::tech_cost` has no resource component to
substitute into, so `develop`/technology payment is untouched by design, not
by omission.

Also root-caused but deliberately NOT fixed here — it belongs to the
`PlayAction`/Frugality/Engineering Genius bucket, not this one:
`corpus.rs::build_card_index` collapses every multi-age action card to its
EARLIEST age variant whenever BGO's journal text carries no age suffix (it
never does) — e.g. resolving a real Frugality (I) play as Frugality (A),
understating its own `gainFood`. That single root cause explains roughly 123
of the 166 `IllegalMove: Pop` failures as a downstream symptom (wrong food
carried forward from an earlier turn's mis-resolved card), not a
population-cost bug at all — worth knowing before re-measuring this bucket.

## Ninth pass: free-civil-action cards (Urban Growth, Rich Land, Efficient
## Upgrade, Breakthrough, Frugality, Engineering Genius) -- one root cause,
## one engine bug, both replayer-side

`IllegalMove: PlayAction` (the single largest bucket, 168 games) and most of
this cluster's `ParserGap`/cost-mismatch entries turned out to be ONE bug, not
two: nine card families (these six plus Territories/Aggressions/Military
Bonuses) print the SAME name once per age with a stronger effect each time,
and BGO's journal text never carries an age tag. `corpus::build_card_index`'s
bare-name lookup (`HashMap::or_insert`) therefore always resolved to whichever
age iterates first in `CARDS` -- Age A, in practice always wrong once a game
is past its first age. That made both symptoms look real at once (an
under-priced discount looks like both "too little discount" and "the option
we needed isn't offered") without there being a second cause -- widening the
option set, the tempting fix, would only have hidden it further. Fixed with
`corpus::best_age_sibling`/`family_siblings` plus
`replay_common::resolve_named_card_by_effect`, which re-resolves a card
against the SAME journal line's own numbers (a discount's implied payment, a
science bonus, a food bonus) wherever that evidence is available, falling
back to "closest age not newer than the deck's current age" only when it
isn't, and corrects an earlier `TakeCard` guess in the player's hand rather
than trusting it blindly.

**ENGINE BUG, confirmed, fixed, tested:** Breakthrough's "may spend its order
on a revolution instead" (RB p.15) computed its own eligibility as a flat
"no civil action spent this turn" in two places (`legal::action_card_playable`,
`apply::h_play_action`) -- a straight port of the same simplification in the
Python oracle, which never applies leader Maximilien Robespierre's variant
(no MILITARY action spent instead). A Robespierre-led player who had spent a
civil action but no military action was illegally denied the option (games
`7523216`, `7523482`). Fixed in Rust only, deliberately diverging from Python
(correctness over parity) -- both call sites now share `legal::
revolt_pool_ok`, the same predicate `can_revolt` itself uses, rather than
each keeping an independent copy (the fix is the shared function, not just
the missing branch: three independently-recomputed copies of one rule is the
recurring bug class here, not this one missing conditional).

### Measurement (`replaystats`, full 1,011-game corpus)

| | eighth pass | this pass |
|---|---|---|
| mean rounds reached (of 19.27) | 6.37 | **7.83** |
| decisions in Age II or later | 5.5% | **19.1%** |
| `IllegalMove: PlayAction` | 168 | **57** |
| games completed | 0 | 0 |

The residual 57 `PlayAction`s and handful of `ParserGap`s are Breakthrough
lines blocked by an unrelated open `Pending::Colonize` (one game) or a
civil-action-count drift this pass did not chase further (one game,
`7523341` -- Breakthrough itself now resolves to the right age; the
remaining gap is in per-turn action-pool bookkeeping, not this cluster).
Everything else this cluster owned -- the Urban Growth/Rich Land `ParserGap`s
and the Urban-Growth/Rich-Land-attributed `UnrecoverableHiddenInfo` cost
mismatches -- is gone. Still 0 complete.

## Tenth pass: "aggression defense" (122 games) was never unrecoverable -- BGO logs one clause per committed card, not a bare count

The `UnrecoverableHiddenInfo: aggression defense: N committed defense
card(s), BGO logs only the count, never identities` bucket (122 games, the
largest `UnrecoverableHiddenInfo` bucket in the corpus) turned out to be a
parser bug, not real hidden information -- found by reading the raw
`"<Color> defends ..."` lines instead of trusting this file's own prior
claim. The old `defends_count` read only the leading digit of the FIRST
clause and discarded the rest of the line; the rest of the line names every
committed card. A `"Defense card +2/+4/+6 played"` clause names its printed
bonus directly, and `data/cards_military_actions.json`'s `bonus`-type cards
have exactly one card per value (one per age I/II/III, six functionally
identical physical copies each) -- so the number alone is a complete,
unambiguous identity; "which of the six" was never a real question. A
`"military card played"` clause is any hand card with `defense_bonus == 0`
(`interact::defense_points`'s flat +1 branch, which is every non-`Bonus`
military-deck card) -- resolved the same way `discard_solver::DiscardSolver`
already resolves a forced hand-limit discard, because it is the same
underlying fact (a specific card permanently leaves the hand). When the
replay's own fictional simulated hand happens to hold no zero-bonus
candidate at all (observed in the real corpus, on hands as small as 1-2
cards), a filler card is grounded instead of stopping the game: identity is
provably irrelevant here, since every non-`Bonus` card defends for the same
flat +1 no matter which one it is.

New code: `DefenseClause`/`parse_defense_clauses`/`defense_bonus_card`/
`flat_defense_filler` in `replay_common.rs`; `resolve_aggression_defense`
rewritten to apply one `Move::Defend` per clause instead of returning
`UnrecoverableHiddenInfo`. Not an engine bug -- `interact::defense_points`
and `Pending::Defense`'s move generation were already correct; only the
journal parser was throwing information away.

### Measurement (`replaystats`, full 1,011-game corpus)

| | ninth pass | this pass |
|---|---|---|
| mean rounds reached (of 19.27) | 7.83 | **8.18** |
| decisions in Age II or later | 19.1% | **21.3%** |
| `UnrecoverableHiddenInfo: aggression defense` | 122 | **0** |
| games completed | 0 | 0 |

All 122 games that used to stop here now run deeper into the same journal
before hitting a DIFFERENT, pre-existing stop reason (the usual pattern in
this file: closing one wall exposes the next one, visible as small increases
across `Pop`/`Take`/`Bid`/`Build`/`Destroy`/`WonderStep`/`Upgrade`/`Develop`
and a few others). None of those buckets belong to this pass and none were
touched.

## Eleventh pass: `IllegalMove: Bid` (103 games) -- 95 reclassified as honest hidden info, not fixed; 8 left open with a different, already-known cause

Not an engine bug. `interact::max_force`'s ceiling depends on `bonus_pool`,
which reads `p.hand_military` directly -- and this file's own top doc
comment already establishes that hand is SIMULATED filler for essentially
its entire content: a military bonus card enters a real player's hand via
an anonymous end-of-turn draw and is never grounded to its true identity
unless the journal later shows it PLAYED. A bidder can genuinely be holding
one this binary has never observed, so its computed ceiling is a LOWER
bound, not an exact figure. Sub-histogram of the 103 (by shape, not by
mechanic -- there is only one mechanic here):

| shape | count | 
|---|---|
| a real raise (`n` > current high bid) that exceeds this binary's own computed ceiling by 1-6 (mode 1) | 95 |
| the auction itself is missing (`legal_moves` shows ordinary Action-phase moves, no `Pending::Auction` at all) | 7 |
| a `Pending::Colonize` from a DIFFERENT, still-open auction is on top | 1 |

The 95: confirmed by direct inspection (`REPLAY_DEBUG`) that the bidder's
reconstructed hand at the failure point holds zero `Bonus`-type cards in
every case, while the attempted bid consistently exceeds the computed
ceiling by a small amount (1-6, matching "one or two hidden bonus cards
worth 1-3 each," not a large or systematic offset that would indicate a
missed rule). `bid_ceiling_mismatch` (`replay_common.rs`) now reports these
as `MismatchKind::UnrecoverableHiddenInfo` instead of `IllegalMove` --
narrowly, only when the bid is a genuine raise against the correctly
identified bidder and exceeds their own computed ceiling, so an actual
engine defect elsewhere still reports as `IllegalMove` unchanged. This is a
RECLASSIFICATION, not a fix: these games still stop in the same place: it
replaces a label that implied "possible engine bug" with the honest reason,
matching this file's own `UnrecoverableHiddenInfo: build cost mismatch`
bucket's precedent.

The remaining 8 are NOT the same bug. Traced one (`7521428`) end to end:
`interact::start_auction` correctly computes the eventual bidder's force as
0 and silently files the territory to `past_events`, skipping the auction
entirely -- and the true player, per the journal, really did have 0 units
built at that exact moment... EXCEPT that two colonize auctions earlier in
the SAME game, `Replayer::auto_drain_colonize` (the already-documented
"does NOT verify which units were spent" approximation) always picks the
engine's cheapest offered `SendUnit` first, which is not necessarily the
unit the real human sacrificed. Over multiple colonizations this can
deplete a cheap unit type (Warriors) that the real player still had in
hand by choosing bonus cards instead. Fixing this needs the same
`"Sacrificed Units:; ..."`-grounding this file's own "gives up on" section
already flags as unimplemented, not something new to this pass -- left
open.

### Measurement (`replaystats`, full 1,011-game corpus)

| | before this pass | after |
|---|---|---|
| `IllegalMove: Bid` | 103 | **8** |
| `UnrecoverableHiddenInfo: colonization bid ...` | 0 | **95** |
| mean rounds reached / decisions Age II+ | unchanged (95 are a reclassification, not a fix) | unchanged |

## Twelfth pass: REPLAYER BUG -- `resolve_intervening` treated "decider == expected_actor" as "nothing left to resolve," even with a live `Pending::Colonize`/`Pending::Auction` still open for that same player

Root cause of 79 of the 178 `IllegalMove: Take` stops (44% of that bucket) and a chunk of the still-open `IllegalMove: Bid` gap. `resolve_intervening`'s job is to auto-resolve anything standing between "control returned to `state.current`" and the next real journal line; its one shortcut, `if decider == expected_actor { ...; return Ok(()); }`, was written for the ordinary case (a live political decision) and silently applied to two others it was never meant to cover:

- A **`Pending::Colonize`** has no real `Move` anywhere in the journal's vocabulary at all -- this file always auto-drains it (`Replayer::auto_drain_colonize`). `decider == expected_actor` here just means the colonizer also happens to be up next for something unrelated (their own `Take`, on 72 games; found first on `7523355`, where Purple's own colonize sits undrained right up to their own next `Take Scientific Method` line) -- not that there is nothing to resolve.
- A **`Pending::Auction`** still owed a real `Bid`/`BidPass` from `decider`, but the very next journal line is unrelated (7 games) -- because that decision is FORCED (their own `interact::max_force` ceiling no longer clears the standing bid) and BGO's UI auto-passes with no click to log, the same shape as `Pending::Defense`'s forced 0-defender `DefendDone`. Root-caused on `7523347`: a 4-way auction where the second-to-last bidder, already outbid past their own ceiling, has no "passes" line anywhere in the journal at all.

Both are now handled explicitly, ahead of the shortcut, mirroring the `Pending::Choice(GainBlock/FreeBuild/DiscardMilitary)` cases already there: `Colonize` always drains; `Auction` defers to the real line only when the upcoming line actually is that decider's own `Bid`/`Pass`, otherwise auto-passes ONLY if `BidPass` is provably their sole legal move, and fails loudly (`StuckPending`) if a real raise was still available -- never guessing a human's decision. New tests pinning each shape, confirmed to fail with the fix reverted.

### Measurement (`replaystats`, full 1,011-game corpus)

| | before this pass | after |
|---|---|---|
| mean rounds reached (of 19.27) | 8.51 | **9.11** |
| decisions in Age II or later | 23.1% | **28.2%** |
| `IllegalMove: Take` | 178 | **148** |
| games completed | 0 | 0 |

Closing this wall exposed the usual next ones (small increases across every
other bucket, including the `UnrecoverableHiddenInfo: colonization bid`
count above -- more games now reach a real bid decision at all). Two new
honest `StuckPending` reasons appeared (15 games total) where a bidder
genuinely owed a real, un-loggable decision this file correctly refuses to
guess at, rather than silently mis-resolving as before.
