# BGO corpus replay-completion analysis (2026-08-18)

Corpus: `sources/bgo/index.tsv` (1011 games) replayed against `/tmp/bgo-journals/journals/`.
Census: `rust/target/release/replaystats sources/bgo/index.tsv /tmp/bgo-journals/journals/`.
Baseline: **863 / 1011 completed** to `state.game_over` (148 not completing).

## Method

Every non-completing game is classified by its **authoritative stop reason** —
the `MismatchKind`/`GameResult` reason the census prints in its
"Stop-reason histogram", NOT the `REPLAY_DEBUG` per-checkpoint drift lines.

## Correction to an earlier (wrong) framing

An earlier pass in this session classified 138/148 games as "upstream science
ledger drift" based on `REPLAY_DEBUG`'s `end-turn science drift` lines. That was
a **false positive**: the `REPLAY_DEBUG` science check
(`replay_common.rs`, the `REPLAY_DEBUG`-gated block right after
`try_apply(Move::EndTurn)`) reads `players[actor].science` *before* a
discard-blocked turn's deferred `resume_end_turn` has actually run, so a turn
that ends with a pending military discard reads as a phantom "drift" that
resolves a few lines later. The **always-on culture oracle** (which compares the
same "(now M)" running total, and is the project's authoritative per-turn
check) matched **100%** in the traced games — the engine's per-turn
reconstruction is correct. The "drift" was a measurement artifact, not an
engine bug. This doc now uses the authoritative stop reasons only.

## Authoritative stop-reason histogram (the real 148)

| count | reason | class |
|---|---|---|
| 39 | IllegalMove: Build | engine legality |
| 15 | IllegalMove: Develop | engine legality |
| 16 | IllegalMove: Upgrade | engine legality |
| 10 | IllegalMove: Revolution | engine legality |
| 8  | StuckPending: decider # != expected actor | engine |
| 8  | IllegalMove: Take | engine legality |
| **6** | **ParserGap: International Agreement pick with no open TakeRow** | **parser** |
| 5  | UnrecoverableHiddenInfo: unpaired client-side undo | hidden-hand |
| 5  | IllegalMove: Pop | engine legality |
| 4  | IllegalMove: PlayTactic | engine legality |
| 4  | IllegalMove: WonderStep | engine legality |
| 4  | IllegalMove: PlayAction | engine legality |
| 4  | IllegalMove: CopyTactic | engine legality |
| 4  | UnrecoverableHiddenInfo: colonization bid contradiction | hidden-hand |
| 3  | IllegalMove: Barbarossa | engine legality |
| **3** | **ParserGap: TakeRow choice does not offer slot (Multimedia)** | **parser** |
| 1  | IllegalMove: Bid | engine legality |
| 1  | UnrecoverableHiddenInfo: Movies build-cost discount | hidden-hand |
| 1  | StuckPending: Breakthrough free-CA not auto-resolved | engine |
| 1  | IllegalMove: PolPass | engine legality |
| 1  | StuckPending: auction decider owes bid/pass | engine |
| 1  | IllegalMove: OfferPact | engine legality |
| **1** | **ParserGap: TakeRow choice does not offer slot (Bill Gates)** | **parser** |
| **1** | **ParserGap: TakeRow choice does not offer slot (Satellites)** | **parser** |
| 1  | IllegalMove: War | engine legality |
| **1** | **ParserGap: TakeRow choice does not offer slot (Reserves III)** | **parser** |

Totals: **12 parser-gap** (the Reserves (I) gap below is now fixed — see
§Parser fixes applied), **10 hidden-hand** (unverifiable from journal text),
**125 engine-legality / StuckPending** (real move the engine rejects — cost,
worker, or state mismatch to chase).

> **Post-fix census (2026-08-18, after commit 052ee88):** 863 / 1011 completed,
> held (no completion lost by either fix). The Reserves (I) parser gap is gone;
> its single game (7521799) now advances to line 336 and stops at a separate
> `IllegalMove: Upgrade` (which is why that bucket reads 16, not 15). The 12
> remaining parser gaps are all "TakeRow"-class (6 + 6) and are diagnosed in
> the next section as row-order drift and multi-line IA logging, not simple
> parser bugs.

## Parser gaps: diagnosis and fixes applied

### Parser fixes applied (committed 052ee88, census 863/1011 held)

**Reserves (I) "spends" logged as "produces" — FIXED (1 game, 7521799).**
`PlayAction {Reserves (I)}` opens a `FoodOrRes` choice; the journal line
"Purple plays Reserves Purple spends 2 resource" carried no trailing
"produces" clause. BGO logged Reserves' **gain** as a `spends` clause on one
line (the only such line in the corpus). Added
`replay_common::trailing_reserves_gain()`, which tries the `produces` form
first (4,157 of 4,158 "plays Reserves" lines use it) and only falls back to
`spends` when that misses — safe because Reserves
(`Special::GainFoodOrResources`) only ever gains, so a trailing "spends" on
its own line cannot be a real cost. The 9,090 ordinary "spends N resource"
cost lines elsewhere are untouched. Verified: the ParserGap is gone and
7521799 now reaches line 336 (a separate `IllegalMove: Upgrade`, previously
masked).

**International Agreement hand-limit bypass — FIXED (neutral, 0 games moved).**
IA takes may exceed the civil-hand limit: BGO logs 4–5 card takes on turns
whose civil hand is already full, and CoL p.12 caps the privilege in civil
**actions**, not hand size. Added `costs::can_take_bypass_hand_limit()`
(`take_gate` with `hand_full` suppressed) and threaded
`bypass_hand_limit=true` through every `TakeRow` offer (queue dispatch and
the re-offer after each pick; `TakeRow` is IA's only entry point). All other
take paths keep the §2.5 hand-full gate. Census-neutral: correct behaviour
but does not by itself fix the 6 "no open TakeRow" games, whose root cause is
row-order drift (below).

### The 12 remaining "TakeRow" parser gaps — NOT simple parser bugs

Both classes trace to the engine's **card-row state diverging from BGO's
row** (or BGO logging the IA session across lines), so they need engine
work, not a one-line parser tweak. Each must be re-derived and measured
individually with no regressions before it lands.

**International Agreement, no open TakeRow (6):** 7522619, 7522268, 7522391,
7522713, 7523218, 7522005. The engine's row has **drifted** from BGO's row
over the game: in every one, the IA picks are mostly **absent** from the
engine's row when the TakeRow opens. Traced 7522268 turn-by-turn: a card
(Multimedia) sat in a swept slot in the engine's r16 row, so the r17
`start_turn` sweep (leftmost 3 for 2-player) discarded it, while BGO kept it
(it was in a later slot in BGO's row). Root cause: row **card-order** drift
accumulating over earlier turns — the sweep removes the *leftmost N*, so a
wrong order sweeps the wrong cards. Fixing it requires a row-ordering /
sweep investigation (deep engine work), not a parser change.

**TakeRow choice does not offer slot (6):** Multimedia ×3 (7522322, 7522649,
7523278), Bill Gates (7523092), Satellites (7521929), Reserves (III)
(7521931). BGO logs the IA session across **multiple lines**: the `;`-joined
IA line (e.g. 7522649 line 267: "Orange takes Air Forces in hand; Orange
takes First Space Flight in hand") is followed by **separate normal-take
lines** (line 268: "Orange takes Multimedia in hand Orange uses 1 civil
action") that are actually part of the same IA session. The engine's
single-line IA branch does not carry the follow-up takes into the open
TakeRow, so the slot the follow-up line names is not offered. Fixing it means
treating the follow-up normal-take line as a continuation of the IA TakeRow
session (engine work), with the caveat that the row must already be in sync
(or it hits the same row-drift wall as the 6 above).

## Provenance

- The `REPLAY_DEBUG` science/resource drift lines are investigation-only and,
  for discard-blocked turns, emit a false "drift"; the authoritative per-turn
  oracle is the always-on **culture oracle** and the census **stop-reason
  histogram**.
- A local `REPLAY_NONCOMPLETED_TSV` helper in `replaystats.rs` is in the
  working tree and is deliberately NOT committed (debug code stays out of
  commits). The git stash is unrelated WIP and must NOT be committed.
