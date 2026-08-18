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
| 15 | IllegalMove: Upgrade | engine legality |
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
| **1** | **ParserGap: Reserves (I) FoodOrRes choice, no "produces" clause** | **parser** |
| **1** | **ParserGap: TakeRow choice does not offer slot (Reserves III)** | **parser** |

Totals: **13 parser-gap**, **10 hidden-hand** (unverifiable from journal text),
**125 engine-legality / StuckPending** (real move the engine rejects — cost,
worker, or state mismatch to chase).

## The 13 parser gaps (the fixable class)

**International Agreement, no open TakeRow (6):** the strongest player's
event takes are logged, but a further pick lands on a separate line (or the
TakeRow was already consumed), so the engine has no open `TakeRow` choice to
offer it.

**TakeRow choice does not offer slot (6):** Multimedia ×3, Bill Gates,
Satellites, Reserves (III). The engine's open `TakeRow` choice was built from
a card-row state that does not include the slot the journal says was taken —
the "Replenish the card row" clause of International Agreement (or a
replenish timing) is the likely cause.

**Reserves (I) no "produces" clause (1):** `PlayAction {Reserves (I)}` opens a
`FoodOrRes` choice but the journal line "Purple plays Reserves Purple spends 2
resource" carries no trailing "produces" clause, so the choice can't be
resolved.

## Provenance

- The `REPLAY_DEBUG` science/resource drift lines are investigation-only and,
  for discard-blocked turns, emit a false "drift"; the authoritative per-turn
  oracle is the always-on **culture oracle** and the census **stop-reason
  histogram**.
- A local `REPLAY_NONCOMPLETED_TSV` helper in `replaystats.rs` is in the
  working tree and is deliberately NOT committed (debug code stays out of
  commits). The git stash is unrelated WIP and must NOT be committed.
