# Experiments / bot-strength progress

Last update: 2026-07-26.

Scope of this file: everything under `experiments/` and `engine/bots/`. The
rules engine itself is tracked in `engine/PROGRESS.md`.

## What exists

| file | status | notes |
|---|---|---|
| `engine/bots/__init__.py` | DONE | `RandomBot`, `GreedyBot` (19-feature 1-ply), `WeightedBot` re-export, `make_bots`. |
| `engine/bots/fastcopy.py` | DONE | Hand-rolled `copy_state`; `deepcopy` was ~78% of a lookahead bot's runtime. Drops `GameState.log` and `_stats_cache`, falls back to `deepcopy` for unknown field types so it stays correct as the engine grows. |
| `engine/bots/weighted.py` | DONE | `WeightedBot`: 1-ply search under a **fully JSON-parameterized** linear evaluation. 58 base features + 20 phase weights = **78 weights**. |
| `experiments/arena.py` | DONE | Seat-rotated, process-parallel duel machinery + normal-approx CIs and p-values. Per-game exceptions are counted, never fatal. |
| `experiments/evaluate.py` | DONE | `python3 -m experiments.evaluate --a X --b Y --games N --players K`. |
| `experiments/hillclimb.py` | DONE | (1+lambda) ES with a two-stage sequential accept test, 1/5th-success step adaptation, per-generation checkpointing. |
| `experiments/run_hillclimb.sh` | DONE | Supervisor: restarts the climber hourly, backs off 60s on an instant exit. |
| `experiments/harness.py` | DONE | Round-robin tournament (older, still useful for >2 distinct bots). |
| `experiments/baselines.jsonl` | DONE | Appended by `evaluate --out`. |

## WeightedBot's weight vocabulary (78 weights)

`engine.bots.weighted.DEFAULT_WEIGHTS`. Feature groups, all read through the
public engine surface (`effects.compute` + `PlayerState` fields):

* **economy (18)** `culture`, `culture_rate`, `science`, `science_rate`,
  `food_rate`, `resource_rate`, `food_stock`, `resource_stock`, `blue_free`,
  `corruption_loss`, `consumption`, `pop_cost`, `yellow_bank`,
  `free_workers`, `workers`, `prod_workers`, `urban_workers`, `unit_workers`
* **happiness (3)** `happy_margin` (capped at +3), `discontent`, `uprising`
* **actions (4)** `civil_actions`, `military_actions`, `ca_left`, `ma_left`
* **military (7)** `strength`, `strength_rel`, `strength_deficit` (uncapped
  penalty), `strength_lead` (capped bonus), `tactic_level`, `colonies`,
  `pacts`
* **technology / tech curve (11)** `tech_levels`, `gov_level`, `best_farm`,
  `best_mine`, `best_lab`, `best_temple`, `best_theater`, `best_library`,
  `best_arena`, `best_unit`, `num_techs`, `special_techs`
* **wonders (4)** `wonders`, `wonder_progress`, `wonder_remaining`, `leader`
* **cards / hand value (4)** `hand_civil`, `hand_value`, `hand_military`,
  `hand_mil_value`
* **rivals (5)** `rival_culture`, `rival_mean_culture`, `rival_culture_rate`,
  `rival_science_rate`, `rival_strength`
* **search bias (1)** `end_turn_bias`
* **age multipliers (20)** ten of the above features get an `_early` and a
  `_late` copy; the contribution is
  `w[k] + (1-L)*w[k_early] + L*w[k_late]` with `L = min(1, age_level/3)`.
  Covered: culture, culture rate, science rate, food rate, resource rate,
  workers, relative strength, tech levels, wonder progress, hand value.

Card-row cost bands are handled implicitly: a candidate `take_card` move is
scored by the *resulting* state, so the civil-action cost of a far-right card
shows up as a lower `ca_left` and the card's own value as `hand_value`.

The move vocabulary is never enumerated. `pick` applies every legal move to a
fast copy and scores the child state, so new move types (colonization bids,
pacts, civil action cards) are picked up automatically; an unscorable
candidate is skipped rather than crashing.

## Measured strength

See "Baselines" below (refreshed after the engine gained colonization/pacts).

## Hill climbing

`(1+lambda)` evolution strategy over the weight dict:

1. mutate ~25% of the weights, gaussian step scaled by each weight's own
   magnitude, 10% chance of a 4x jump; `culture` is frozen (it is the score,
   and it anchors the evaluation's units);
2. screen the mutant against a table of champions over `--screen` games,
   seat-rotated; drop it immediately if it is below the null win rate;
3. otherwise keep playing up to `--max-games` and accept only if the lower
   bound of a one-sided 90% CI is still above `1/players`;
4. checkpoint.

**Restart safety.** After every generation the champion is written atomically
to `experiments/champion_{K}p.json` and one line is appended (with `fsync`) to
`experiments/generations_{K}p.jsonl`. Re-running the same command resumes from
the checkpoint, so a kill costs at most the generation in flight.

Every 10th generation the champion is re-measured against `default` and
`greedy` and the result is recorded as `vs_default` / `vs_greedy` in the
generation log — an absolute anchor, so drift caused by the sequential accept
test's inflated false-positive rate is visible.

### Running climbs

Launched 2026-07-26 06:14, 10-hour budget each:

```
nohup experiments/run_hillclimb.sh 2 10 1 1 48 288 > experiments/logs/sup_2p.out 2>&1 &
nohup experiments/run_hillclimb.sh 3 10 2 1 48 288 > experiments/logs/sup_3p.out 2>&1 &
nohup experiments/run_hillclimb.sh 4 10 2 1 48 288 > experiments/logs/sup_4p.out 2>&1 &
```

Arguments are `PLAYERS HOURS WORKERS LAMBDA SCREEN MAXGAMES`. Worker counts
sum to 5 of the 6 cores.

### Check / restart commands

```bash
# alive?
ps aux | grep -E 'hillclimb|run_hillclimb' | grep -v grep

# live progress
tail -f experiments/logs/hc_4p.log

# generations accepted so far, per player count
for k in 2 3 4; do
  printf '%sp: ' "$k"
  python3 - "$k" <<'EOF'
import json,sys
k=sys.argv[1]
rows=[json.loads(l) for l in open(f"experiments/generations_{k}p.jsonl")]
acc=[r for r in rows if r["accepted"]]
anch=[r for r in rows if "vs_greedy" in r]
print(f"{len(rows)} gens, {len(acc)} accepted, sigma={rows[-1]['sigma']}",
      f"last anchor vs_default={anch[-1]['vs_default']} vs_greedy={anch[-1]['vs_greedy']}" if anch else "")
EOF
done

# restart one (safe at any time -- it resumes from champion_Kp.json)
cd ~/tta-ai && nohup experiments/run_hillclimb.sh 4 10 2 1 48 288 \
    > experiments/logs/sup_4p.out 2>&1 &

# stop everything
pkill -f run_hillclimb.sh; pkill -f experiments.hillclimb
```

The supervisor restarts the climber every hour on purpose: each restart picks
up the latest engine code (another agent is editing the engine concurrently)
and a crash costs at most one generation.

## Known limitations

* The sequential accept test (screen, then extend, then accept on a one-sided
  90% lower bound) has an inflated false-accept rate. The periodic anchor
  measurement is the guard; if `vs_default` stops rising while generations
  keep being accepted, the climb is chasing noise and `--max-games` should go
  up.
* The champion is only ever evaluated against *itself*, so the climb finds a
  local best response, not a globally strong policy. A population/league would
  fix this and is the natural next step.
* Search is strictly 1-ply. The evaluation's phase weights are the only
  substitute for planning ahead.
