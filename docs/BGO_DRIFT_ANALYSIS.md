# BGO corpus replay-completion analysis (2026-08-18)

Corpus: `sources/bgo/index.tsv` (1011 games) replayed against `/tmp/bgo-journals/journals/`.
Census: `rust/target/release/replaystats sources/bgo/index.tsv /tmp/bgo-journals/journals/`.
Baseline at time of this analysis: **863 / 1011 completed** to `state.game_over`.

## Method

Every one of the 148 non-completing games was traced inline with
`REPLAY_DEBUG=1 replaystats ... --game <ID>` to obtain the per-checkpoint
`end-turn ... drift` lines and the `try_apply fail` line, plus the raw journal
line at the stop. Each game is classified by the root cause of its failure.

## Result (corrected after re-verification)

The 13 games previously bucketed as "parser-gap" were re-traced against the
*current* binary. All 13 show **persistent upstream science drift from the very
first end-turn checkpoint** (4–32 drift lines each, first delta −1 to −6). None
has a clean ledger followed by a parse failure. The "TakeRow no slot" /
"IntlAgreement no TakeRow" / "Reserves produces" failures were downstream
symptoms: the science ledger is already short, so the final card's cost (or the
worker/population slot it needs) cannot be met.

**Conclusion: there is no distinct parser bug to fix.** All 148 non-completing
games are the same class — upstream ledger drift, dominated by science.

| category | count |
|---|---|
| upstream ledger drift (science-dominant) | 138 |
| hidden-hand (unverifiable from journal) | 10 |
| clean parse / card-model bug | 0 |

(The 10 hidden-hand: 4 colonization-bid contradictions, 5 unpaired client-side
undos, 1 Movies build-cost discount — none reproducible from journal text alone.)

## The 138 upstream-drift games (full list)

Each row: `<id> | stop reason (as bucketed at first pass) | stop line`. The
"parser-gap" rows are included here — they are upstream-drift, not parse bugs.

### Previously "upstream-drift" (125)
See `/tmp/bgo-drift-complete.md` section "upstream-drift" for the 125 rows with
raw journal text.

### Re-bucketed from "parser-gap" (13) — all upstream science drift

| id | first-pass label | stop line | first drift delta |
|---|---|---|---|
| 7521799 | Reserves(I) no produces clause | 107 | −1 |
| 7521929 | TakeRow no slot (Satellites) | 323 | −4 |
| 7521931 | TakeRow no slot (Reserves III) | 265 | −2 |
| 7522005 | IntlAgreement no TakeRow | 428 | −1 |
| 7522268 | IntlAgreement | 333 | −3 |
| 7522322 | TakeRow no slot (Multimedia) | 310 | −6 |
| 7522391 | IntlAgreement (Genghis) | 332 | −2 |
| 7522619 | IntlAgreement | 405 | −2 |
| 7522649 | TakeRow no slot (Multimedia) | 268 | −2 |
| 7522713 | IntlAgreement | 448 | −2 |
| 7523092 | TakeRow no slot (BillGates) | 290 | −1 |
| 7523218 | IntlAgreement | 423 | −1 |
| 7523278 | TakeRow no slot (Multimedia) | 391 | −2 |

## Drift signature

In every traced game the journal's running "now N" science total is HIGHER than
what the engine computes, and the gap appears at the first end-turn and grows
(or persists). The engine is *under-counting* science by 1–6 points per actor.
The desync seeds from an earlier step that the journal does not make
reproducible (a hidden draw, a wonder/event scored slightly differently, or
corruption timing). No single shared card or cost constant explains it.

## Next step

Chase the *first* desync line of a small sample of the drift games to name the
exact earlier step that seeds the science under-count (hidden draw, wonder,
event, or corruption timing). That is the honest path to closing the gap —
not parser surgery.

## Provenance notes

- Debug env vars used: `REPLAY_DEBUG=1` (drift + try_apply + cost lines),
  `REPLAY_NONCOMPLETED_TSV=1` (one clean row per non-completing game). The TSV
  dump var is a local working-tree helper and is deliberately NOT committed.
- The git stash (`stash@{0}` … `stash@{3}`) is unrelated WIP and must NOT be
  committed.
