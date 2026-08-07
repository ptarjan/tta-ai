# docs/PANEL.md — champion vs the seven archetypes, round-robin (2026-08-06)

**Yes, decisively.** The trained champion (`weighted` + `rust_champion_{2,3,4}p.json`)
beats every one of the seven hand-written archetype bots (`culture`,
`military`, `science`, `wonder`, `infra`, `tempo`, `book`) at every player
count tested. Win rate ranges 66.7%-96.7%, all well clear of the 1/N null
by Wilson 95% CI. No archetype beat the champion anywhere. Zero
resignations, zero move-cap hits, zero panics across 2,520 games.

## Method

`rust/src/bin/selfplay --bots weighted,<arch>[,<arch>...] --players N --games
120 --weights rust_champion_Np.json --seed 1 --threads 2`, one champion seat
against a table of N-1 copies of one archetype, seats rotated by
`selfplay` itself (every kind plays every seat equally often over the 120
games). n=120 per cell, same seed base (1) for every cell so the games
played are the same across archetypes at a given player count — the
"seed-paired" design asked for. 21 cells total (7 archetypes x 3 player
counts), 2,520 games, run at `--threads 2` to stay inside the 6-core box's
budget while `climb`'s three training arms use the rest. Total measurement
wall-clock: about 2 minutes of actual `selfplay` time (game engine is fast;
120 games at 4p takes ~18s). Confidence intervals below are Wilson
intervals on the win count, not the harness's own (which is not computed by
`selfplay`; win counts were recovered from the printed win-rate% x n and
rounded to the nearest whole game — unambiguous at n=120, 1-decimal
precision).

Raw data: `analysis/panel.jsonl` / `analysis/panel.tsv` (one row per cell),
raw `selfplay` stdout under `analysis/panel_logs/`.

## Results

`share` = champion win rate. `null` = 1/players. `xpar` = share/null.

### 2p (n=120, null=50.0%)

| archetype | share | 95% CI (Wilson) | xpar |
|---|---|---|---|
| tempo | 94.2% | 88.4-97.1% | 1.88x |
| infra | 91.7% | 85.3-95.4% | 1.83x |
| science | 90.0% | 83.3-94.2% | 1.80x |
| wonder | 85.0% | 77.5-90.3% | 1.70x |
| book | 82.5% | 74.7-88.3% | 1.65x |
| military | 87.5% | 80.4-92.3% | 1.75x |
| culture | 66.7% | 57.8-74.5% | 1.33x |

### 3p (n=120, null=33.3%; champion alone vs a table of 2x the archetype)

| archetype | share | 95% CI (Wilson) | xpar |
|---|---|---|---|
| military | 96.7% | 91.7-98.7% | 2.90x |
| infra | 96.7% | 91.7-98.7% | 2.90x |
| tempo | 95.8% | 90.6-98.2% | 2.87x |
| science | 91.7% | 85.3-95.4% | 2.75x |
| wonder | 90.8% | 84.3-94.8% | 2.72x |
| book | 85.0% | 77.5-90.3% | 2.55x |
| culture | 81.7% | 73.8-87.6% | 2.45x |

### 4p (n=120, null=25.0%; champion alone vs a table of 3x the archetype)

| archetype | share | 95% CI (Wilson) | xpar |
|---|---|---|---|
| infra | 94.2% | 88.4-97.1% | 3.77x |
| tempo | 88.8% | 82.3-93.6% | 3.55x |
| book | 89.2% | 82.3-93.6% | 3.57x |
| military | 87.5% | 80.4-92.3% | 3.50x |
| science | 85.0% | 77.5-90.3% | 3.40x |
| culture | 78.3% | 70.1-84.8% | 3.13x |
| wonder | 78.3% | 70.1-84.8% | 3.13x |

## Reading it

- **CultureBot is the champion's toughest opponent at every player count**
  (66.7% / 81.7% / 78.3% — the two lowest shares in the whole table besides
  4p wonder), consistent with `docs/VARIANTS.md`'s framing of culture as
  "deliberately punishable by war" — a table that refuses to fight is the
  shape the champion apparently finds hardest to convert against, not
  easiest. It is still a comfortable win, just the least comfortable one.
- Every other archetype clears 82% at every player count. `tempo` and
  `infra` are the champion's easiest kills (near-shutouts at 3p: infra and
  military both 96.7%, tempo 95.8%).
- `xpar` grows with table size for most archetypes (2p ~1.3-1.9x -> 4p
  ~3.1-3.8x) because the null itself shrinks (1/4 vs 1/2) while the
  champion's share stays roughly flat in the 78-94% band — i.e. the
  champion isn't just beating one archetype copy, it's beating a whole
  table of them, which a neutral player could not do.
- Zero resignations and zero move-cap hits in all 2,520 games (14 cells at
  n=120 for 2p+3p checked first, then the 4p arm, same result) — no
  evidence of a stuck-game or infinite-loop bug in this port.

## What this is not

- Not a claim about relative *archetype* strength (culture vs military
  etc.) — every archetype only ever played against the champion here, never
  against each other.
- Not the arena binary's seat-paired single-deal design (`docs/RUST_LEAGUE.md`);
  `selfplay`'s seat-rotation is a weaker but adequate balance for a
  bot-kind-vs-bot-kind panel, which is what `docs/VARIANTS.md`'s own smoke
  test already established as the right tool for this question.
- Not run: anything beyond n=120 per cell, any archetype-vs-archetype cell,
  any player count outside 2/3/4. Time budget (90 min) was not the binding
  constraint — the whole panel finished with room to spare — breadth over
  depth was still the right call per the task brief, and 21 cells at n=120
  is already a lot more informative than 3 cells at n=1000 would have been.

## Comparison to the stale Python-era number

`docs/BOT_ROSTER.md` (2026-07-30, deleted 2026-08-06 in the doc cull;
recoverable via `git log --oneline -- docs/BOT_ROSTER.md`) had the champion
10th of 12 entrants and losing 15%/85% to CultureBot at 2p, with its 4p
rows separately quarantined as measured against a known-degenerate weight
vector. This panel finds the opposite ordering at 2p — champion 66.7%,
culture 33.3% — a full reversal, and the champion beating culture (its
hardest matchup here) at every player count.

**That reversal is not evidence the champion got better through training
alone.** The two measurements are not comparable inputs to a before/after
story: the 2026-07-30 number was produced by the old Python engine and
whatever champion vector existed then; this one is produced by the Rust
port, ~2,000 further generations of league climbing, and six engine bug
fixes landed since (see `docs/RUST_LEAGUE.md`, `docs/AUDIT_HISTORY.md`).
Any of those three factors alone could account for some or all of the
delta; this measurement cannot separate them. What it does establish
cleanly is the current state: right now, today, the trained champion beats
this archetype roster, including at 2p against culture specifically.

**A second, smaller check worth recording**: this panel's 2p/culture cell
(66.7%, seeds 1-120) does not even reproduce `docs/VARIANTS.md`'s own
60-game smoke number (81.7%, seeds 1-60) on the overlapping seed range. That
looked at first like a sampling artefact (n=60 vs n=120) but is not — rerunning
`selfplay --bots weighted,culture --players 2 --games 60 --weights
rust_champion_2p.json --seed 1` against this panel's *own* copy of the 2p
champion reproduces 70.0%, not 81.7%, on the identical 60-seed range the
smoke test used. The real cause: `rust_champion_2p.json` is the live
league's output file (`experiments/rust_champion_2p.json`, gitignored,
HARD RULES forbid touching the league that writes it) and it is
continuously overwritten while `climb` keeps running — checked directly:
the copy in this clone (taken 19:00) and the live file on disk (checked
19:08, 8 minutes later) already have different MD5s and sizes. The smoke
test's snapshot, this panel's snapshot, and whatever is on disk right now
are three different weight vectors under the same filename. This means:
every number in this document, including the 60-game smoke number it is
being compared to, is a **point-in-time reading of a moving target**, not a
fixed champion's true strength. The panel above is internally consistent
(one snapshot per player count, copied once before any cell ran, used for
every cell of that player count) but is not reproducible against a later
or earlier snapshot, and should not be read as more precise than that.
