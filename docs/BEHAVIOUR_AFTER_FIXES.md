# Behaviour after the pacts/colonies/war fixes: baseline vs now

Date: 2026-07-26

Four fixes landed to stop the bot from ignoring pacts, colonies, wars and
aggressions:

- `6376981` Fix WeightedBot scoring the WRONG player on pending decisions
- `5575110` GreedyBot: lazy trial-rng reseed instead of Random(0) per candidate
- `166867d` Price deferred pacts and colonies by their yield, not by counting them
- `15b9764` Reset colonies/pacts weights to defaults (fix #4)

This re-runs the same behaviour measurement on current code, using the
**current champion weights** (`experiments/champion_{2,3,4}p.json`, gen 158
at time of run) via `analysis/behaviour_run.py` (the working wrapper around
`experiments/behaviour.py` — `experiments/behaviour.py` itself is still
broken standalone, see its module docstring).

## Method

- 240 games total, split evenly 80/80/80 across 2p/3p/4p, matching the
  baseline's total sample size.
- Mirror self-play (`--opponent self`), current champion vs itself, one
  champion seat rotated per game.
- `seed0=500000`, `--workers 6`, `move_cap` default (20000).
- 0 engine errors across all 240 games.
- Commands run:
  - `python3 analysis/behaviour_run.py --players 2 --games 80 --workers 6 --champion experiments/champion_2p.json --out /tmp/behaviour_after_2p.json --seed0 500000`
  - same for `--players 3` and `--players 4`.
- Metric sources in the harness output: pacts = `moves_per_game["offer_pact"]`;
  colony bids = `moves_per_game["bid"]`; wars/aggressions =
  `conflict.wars_started_per_game` / `conflict.aggressions_started_per_game`.

## Results

| metric | baseline (240 games, pre-fix) | now (240 games, post-fix) | change |
|---|---|---|---|
| pacts offered / game (2p) | 0.00 | 0.00 | none (2p decks exclude pacts by rule — expected, not a bug) |
| pacts offered / game (3p) | 0.00 | **1.80** | fixed |
| pacts offered / game (4p) | 0.00 | **3.21** | fixed |
| colony bids / game (2p) | 0.18 | **0.42** | ~2.3x, improved |
| colony bids / game (3p) | 0.08 | **2.17** | ~27x, fixed |
| colony bids / game (4p) | 0.02 | 0.01 | unchanged / still ~zero |
| wars declared / game (all) | 0.00 | 0.00 | **no change** |
| aggressions / game (2p) | ~0.03 (low end of range) | 0.00 | no change (small samples, still ~zero) |
| aggressions / game (3p) | ~0.03–0.11 | 0.00 | no change |
| aggressions / game (4p) | ~0.03–0.11 | 0.11 | no change |

Baseline aggression was only given as a range (0.03–0.11 across player
counts) rather than broken out per player count; the "now" column is broken
out here for clarity. Nothing in the aggression numbers moved outside
sampling noise at any player count.

## Verdict

**Partly worked.** Pacts are fixed at 3p/4p (zero to a real, substantial
rate — the bot now actually offers them) and colonies are fixed at 2p/3p
(colony bids up sharply, 27x at 3p), but colonies at 4p are still
effectively zero, and wars and aggressions did not move at all — they
remain exactly (wars) or statistically (aggressions) unchanged from
baseline. This matches the root-cause analysis in
`docs/PACTS_DIAGNOSIS.md`: the fixes addressed the 1-ply-invisibility
problem for pacts and (mostly) colonies, but the 4p colony auction still
never starts (no events get seeded), and the war/aggression 1-ply horizon
problem was never separately fixed.
