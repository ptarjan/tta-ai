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
| `experiments/hillclimb.py` | DONE | (1+lambda) ES with a **league** field, a **paired** sequential accept test, four mutation operators, 1/5th-success step adaptation + stall kicks, per-generation checkpointing. |
| `experiments/run_hillclimb.sh` | DONE | Supervisor: restarts the climber hourly, backs off 60s on an instant exit. |
| `experiments/measure_champions.sh` | DONE | Snapshots all three champions vs `random` / `greedy` / `default` into `baselines.jsonl`; run it detached with 1 worker alongside the climbs. |
| `experiments/league_{K}p/` | DONE | Archive of past champions (founder + newest 8) that forms the field a mutant must beat. |
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

Two levers are encoded by a *pair* of features rather than a single named
one, because the bot scores resulting states and not moves:

* **Corruption margin** = `blue_free` (+, the headroom you are buying) plus
  `corruption_loss` (−, the resources §6.2 actually takes at the current
  band). Their ratio is what the search tunes: a large `blue_free` relative
  to `corruption_loss` means "bank blue tokens before you need them", a small
  one means "only pay when you are about to cross a band".
* **Card-row cost bands**: a candidate `take_card` move is scored by the
  *resulting* state, so the civil-action cost of a far-right card shows up as
  a lower `ca_left` and the card's own worth as `hand_value` /
  `hand_mil_value`. `ca_left` vs `hand_value` is therefore the "is this card
  worth its row price?" trade-off, and both are free to move.

The move vocabulary is never enumerated. `pick` applies every legal move to a
fast copy and scores the child state, so new move types (colonization bids,
pacts, civil action cards) are picked up automatically; an unscorable
candidate is skipped rather than crashing.

## Measured strength

`WeightedBot` at `DEFAULT_WEIGHTS` (`--a default`), one challenger against a
table of the named bot, seat-rotated, 96 games each, 95% CIs. Measured
2026-07-26 06:16 against the engine at commit `c25f34b`.

| table of | 2p (null 50%) | 3p (null 33.3%) | 4p (null 25%) |
|---|---|---|---|
| `RandomBot` | **96.9%** ± 3.5 | **96.9%** ± 3.5 | **93.8%** ± 4.9 |
| `GreedyBot` | **89.6%** ± 6.1 | **75.5%** ± 8.6 | **54.7%** ± 10.0 |

Mean final culture, challenger vs table mean: 107 vs 63 (2p), 119 vs 75 (3p),
148 vs 100 (4p) against greedy; 95/13, 93/18, 130/26 against random. Every
result is far outside its CI, so **requirement 1 is met: WeightedBot beats
GreedyBot at default weights at every player count.**

Two things to read out of that table:

* The margin shrinks as the table grows. That is mostly arithmetic — with
  three greedy opponents somebody else gets a good draw more often — but it
  also means 4p is where the hill climb has the most room, which is why it
  gets the largest game budget per mutant.
* These numbers are *lower* than the ones in the first two lines of
  `baselines.jsonl` (93-95% at every count), which were taken before the
  engine gained colonization auctions and pacts. The new subsystems gave
  `GreedyBot` moves whose one-ply value it can see, so the gap narrowed.
  Re-measure after any large engine change; the old rows are kept in the
  JSONL for exactly this comparison.

Reproduce:

```bash
for K in 2 3 4; do for B in random greedy; do
  python3 -m experiments.evaluate --a default --b $B --games 96 --players $K \
      --out experiments/baselines.jsonl
done; done
```

Swap `--a default` for `--a experiments/champion_4p.json` to score a champion.

**Self-play control.** `--a default --b default` returns *exactly* 50.0% at
2p, 33.3% at 3p and 25.0% at 4p over 96 games each. That is the expected result and it is
the load-bearing check on the whole accept test: `arena.duel` plays each seed
once per seat with the challenger rotated through all K seats, so when both
sides run the same deterministic policy the challenger wins exactly one of the
K rotations of every seed. There is **no seat or asymmetry bias** — a mutant's
win rate above `1/K` is real signal, not a artifact of being the odd one out.

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

### League mode (landed 2026-07-26 07:10)

The ladder above is a pure mirror: a mutant only ever had to beat *its own
parent*, so the search finds the best response to one policy rather than a
strong policy. The failure mode showed up in the data (3p gen 10 beat
`default` 52.1% while sliding to 60.4% against `greedy`, *below* what
`default` itself scores). Four changes, all now live:

1. **League field.** Every accepted champion is archived to
   `experiments/league_{K}p/gen000NN.json`; the founder plus the newest 8 are
   kept. A generation's field is `[champion]*4 + 3 sampled ancestors +
   ["default", "greedy"]`, and each defender seat is drawn from that pool.
   The draw is keyed on the game seed only, never on the challenger, so two
   duels on the same seeds face byte-identical opposition.
2. **Paired accept test.** Against a mixed field `1/K` is no longer the right
   null, so the statistic is now the mutant's *edge over the champion on
   identical games*: for every (seed, seat) the champion replays the same
   game and we accumulate the difference in win share. The null is exactly 0
   whatever the field looks like, and pairing removes seed variance, which is
   the dominant term — so a decision needs far fewer games than the unpaired
   test did. Cost is 2 games per paired sample; a losing mutant is abandoned
   after one screening block, a winner runs to `--max-games`.
3. **Coherent group mutations.** Four operators, sampled per mutant:
   `scatter` (45%, the old 25%-of-weights move), `group` (33%, perturb one or
   two whole feature groups *including* their `_early`/`_late` phase copies —
   "care more about this strategic axis at every age"), `rescale` (12%,
   multiply a whole group by `exp(N(0, sigma))` — changes the axis's weight
   relative to everything else without disturbing its internal shape), and
   `kick` (10%, 60% of the weights at 3x sigma — a deliberate restart from a
   perturbed champion). The accepted operator is recorded per generation as
   `op`, so which move types actually pay is measurable.
4. **Adaptive sigma with stall kicks.** The 1/5th rule still shrinks sigma
   (x0.85) below a 12% accept rate and grows it (x1.25) above 25%, bounded to
   [0.05, 0.8]; on top of that, after 15 consecutive rejections the next
   mutant is *forced* to be a `kick` and sigma is re-opened to at least 0.5.
   Without this, 2p annealed to the 0.05 floor and ground on the same
   neighbourhood for 15+ generations (gens 20-33 of the pre-league run).

`--mode mirror` still reproduces the old behaviour for comparison. The switch
is recorded as a `{"event": "search_update", ...}` line in each
`generations_{K}p.jsonl`; **`wr` before that line and `edge` after it are
different statistics** — `edge` is centred on 0 by construction, so an accept
at `edge=+0.04` means "four points of win share better than the parent
against the same field", not a 4% win rate.

**Restart safety.** After every generation the champion is written atomically
to `experiments/champion_{K}p.json` and one line is appended (with `fsync`) to
`experiments/generations_{K}p.jsonl`. Re-running the same command resumes from
the checkpoint, so a kill costs at most the generation in flight.

Every 10th generation the champion is re-measured against `default` and
`greedy` and the result is recorded as `vs_default` / `vs_greedy` in the
generation log — an absolute anchor, so drift caused by the sequential accept
test's inflated false-positive rate is visible.

### Running climbs

Relaunched **2026-07-26 07:11** in league mode against engine `a29e625`,
10-hour budget each:

```
cd ~/tta-ai
nohup experiments/run_hillclimb.sh 2 10 1 1 48 192 > experiments/logs/sup_2p.out 2>&1 &
nohup experiments/run_hillclimb.sh 3 10 1 1 48 192 > experiments/logs/sup_3p.out 2>&1 &
nohup experiments/run_hillclimb.sh 4 10 2 1 48 192 > experiments/logs/sup_4p.out 2>&1 &
```

Arguments are `PLAYERS HOURS WORKERS LAMBDA SCREEN MAXGAMES`.

**Machine budget.** The Mac mini has 6 cores. Worker counts now sum to **4**,
leaving 2 free (one for the concurrent engine/advisor agents, one for
`measure_champions.sh`, which is deliberately run at `--workers 1`). 4p gets
two workers because its games are the slowest; 2p and 3p get one each. Do not
raise these past 4 total — oversubscription makes every climb slower without
raising throughput, and the parallelism is inside `arena.duel`, so a climb
with `--workers 1` still uses a full core in the parent process.

A mutant is screened on 48 games, needs >=96 paired games before it can be
accepted, and is abandoned after 192. In league mode each paired sample costs
2 games, so a generation is 96-192 games and runs roughly 1-4 minutes.

### The engine-update cut (2026-07-26 06:56)

The engine agent landed all 33 civil action cards plus colonization/pact/
defence fixes and several strength and legality bug fixes (tests 28 -> 57).
**Generations before the cut were measured on the pre-action-card engine and
their win rates are not comparable to later ones.** The cut is recorded as an
`{"event": "engine_update", ...}` line in each `generations_{K}p.jsonl` (after
gen 19 / 18 / 12 for 2p / 3p / 4p) and `summarize.py` prints it.

The learned weights were **kept as a warm start** — they still transfer.
Re-measured on the new engine, champion vs a table of `default`, 96 games:

| | 2p (null 50%) | 3p (null 33.3%) | 4p (null 25%) |
|---|---|---|---|
| champion vs `default` | 44.8% ± 9.9 (n.s.) | **45.8% ± 10.0** (p=0.015) | **34.9% ± 9.5** (p=0.042) |

3p and 4p are real gains that survived the rules change; 2p is a wash, which
is consistent with it having accepted only one mutant in 19 generations.

`sigma` was reset to 0.25 for all three at the cut. The annealed step sizes
(2p had reached the 0.05 floor) described the *old* fitness landscape; with 33
new playable cards the search needs to explore again rather than fine-tune a
converged point.

Speed reference at the cut (RandomBot): 15.9 / 9.6 / 5.5 games per second at
2p / 3p / 4p. `WeightedBot` is far slower — it applies every legal move to a
copy of the state — so budget from measured generation times in the logs, not
from these. Another agent is optimizing engine speed; if generations get
noticeably faster, raise `--max-games` rather than `--lambda`, since the
accept test's power is what limits the climb.

### Check / restart commands

```bash
# alive?
ps aux | grep -E 'hillclimb|run_hillclimb' | grep -v grep

# live progress
tail -f experiments/logs/hc_4p.log

# generations, accepts, anchor series and which weights moved
python3 -m experiments.summarize            # all three player counts
python3 -m experiments.summarize --players 4 --top 25

# restart one (safe at any time -- it resumes from champion_Kp.json)
cd ~/tta-ai && nohup experiments/run_hillclimb.sh 4 10 2 1 48 192 \
    > experiments/logs/sup_4p.out 2>&1 &

# absolute strength of all three champions (detached, 1 core, ~15 min)
cd ~/tta-ai && nohup experiments/measure_champions.sh 96 1 >/dev/null 2>&1 &
tail -f experiments/logs/measure.log

# stop everything
pkill -f run_hillclimb.sh; pkill -f experiments.hillclimb
```

A climb is **healthy** when new `gen` lines keep appearing in
`experiments/logs/hc_{K}p.log` every few minutes *and* an accept lands every
10-20 generations. Two failure signatures to watch for:

* `no playable games this generation -- engine likely mid-edit` repeated: the
  engine is broken or mid-edit; the climber sleeps 60s and retries by design,
  but if it persists for >10 minutes check `python3 -c "import engine.game"`.
* a long rejection streak with `sigma` pinned at 0.05: the stall kick should
  now break this within 15 generations. If it does not, the accept test is
  the bottleneck -- raise `--max-games`, not `--lambda`.

The supervisor restarts the climber every hour on purpose: each restart picks
up the latest engine code (another agent is editing the engine concurrently)
and a crash costs at most one generation.

## What the search is favoring

Refresh this section with `python3 -m experiments.summarize`; it prints the
weights that have drifted furthest from `DEFAULT_WEIGHTS`, tagged by feature
group. Snapshot at 2026-07-26 06:30 (2p gen 9, 3p gen 5, 4p gen 6 — **early,
treat as direction not magnitude**):

| lever | default | 3p champion | 4p champion | reading |
|---|---|---|---|---|
| `strength_rel` | 0.35 | **1.14** | −0.04 | at 3p, being stronger *than the strongest rival* is worth ~3x what the hand-set weight assumed; at 4p the raw relative term is being replaced by the asymmetric `strength_deficit`/`strength_lead` pair |
| `culture_rate` | 5.0 | **8.66** | — | production rate beats stock even harder than assumed |
| `science` (stock) | 0.5 | **−0.19** | — | *hoarding* science is a negative; banked science is dead weight until spent |
| `leader` | 1.5 | **3.44** | — | having any leader out is undervalued by hand-set weights |
| `military_actions` | 0.7 | **−0.09** | — | military action capacity is nearly worthless once relative strength is priced correctly |
| `workers` / `prod_workers` | 1.4 / 0.3 | — | **3.63 / 0.90** | at 4p the search pushes hard on population and on farms+mines specifically |
| `wonder_remaining` | −0.30 | — | **+0.32** | sign flip: at 4p a big *unfinished* wonder is an asset, not a liability |
| `rival_mean_culture` | −0.10 | — | **−0.44** | at 4p you must suppress the *field*, not just the leader; at 3p this weight went slightly positive instead |
| `rival_culture` | −0.35 | −0.60 | −0.60 | both counts agree: denying the leader is worth roughly twice the hand-set weight |
| `uprising` | −12.0 | **−19.9** | — | uprisings are even more catastrophic than assumed |
| `discontent` | −3.0 | — | +0.57 | 4p only, and almost certainly noise — flag to re-check once the anchor series is longer |

Mean relative drift by group, which is the coarse "where is the signal?"
answer:

* **3p** rivals 0.77x > military 0.73x > wonders 0.58x > economy 0.51x >
  actions 0.49x > cards 0.45x > tech 0.37x > happiness 0.30x
* **4p** rivals 0.91x > happiness 0.64x > economy 0.57x > cards 0.52x >
  military 0.40x > tech 0.36x > actions 0.22x

Both player counts agree on the headline: **the rival-relative terms are the
most mis-set part of the hand-written evaluation.** The hand-set weights score
your own board too much in absolute terms; the search keeps buying more
"relative to the field" and less "absolute output". The second theme is
player-count-dependent — 3p rewards military pressure (`strength_rel`,
`leader`), 4p rewards raw economic scale (`workers`, `prod_workers`) — which
is the main argument for keeping three separate champions rather than one.

2p has accepted nothing in 9 generations and its sigma has already annealed
0.25 → 0.21, which is the expected signature of a starting point that is
already near a local optimum for that player count (consistent with the
89.6% default-weights win rate against greedy at 2p, the highest of the
three).

## Known limitations

* The sequential accept test (screen, then extend, then accept on a one-sided
  90% lower bound) has an inflated false-accept rate. The periodic anchor
  measurement is the guard; if `vs_default` stops rising while generations
  keep being accepted, the climb is chasing noise and `--max-games` should go
  up.
* The champion is only ever evaluated against *itself*, so the climb finds a
  local best response, not a globally strong policy. A population/league would
  fix this and is the natural next step. **This is already visible**: at 3p
  gen 10 the champion beat `default` 52.1% (null 33.3%, a real gain) while its
  score against `greedy` read 60.4%, *below* the 75.5% that `default` itself
  scores. Rock-paper-scissors against a bot the climb never sees is exactly
  the failure mode a self-play-only ladder produces. The anchor sample was
  raised from 48 to >=96 games (with CIs recorded) so this can be read as a
  trend rather than guessed at; if `vs_greedy` keeps sliding while
  `vs_default` climbs, the fix is to make the climb play a mixed table
  (champion + greedy + an older champion) rather than a pure mirror.
* Search is strictly 1-ply. The evaluation's phase weights are the only
  substitute for planning ahead.
