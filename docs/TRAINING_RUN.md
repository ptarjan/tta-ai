# Long league training run — working note

Terse operational note, not a write-up.

## Current run: the QUIESCENT arms, launched 2026-07-27 from `train/loop-fix`

**This run trains a different policy from every run before it.** The weight
vector is identical in shape, but it is now scored by `QuiescentBot`
(`--candidate-bot quiescent:levels=1`) instead of the 1-ply `WeightedBot`. See
"why" below; see `docs/DEEPER_SEARCH.md` for the architecture.

    experiments/run_league.sh <K> 12 <W> 2 12 4 1.2816 \
        --init default --weight-guard clamp --past-k 2 \
        --candidate-bot quiescent:levels=1

Positional args are `PLAYERS HOURS WORKERS LAMBDA BLOCK SUBSET ACCEPT_Z`.

| K  | workers | supervisor PID | log                             |
|----|---------|----------------|---------------------------------|
| 2p | 1       | 96921          | `experiments/logs/league_2p.log` |
| 3p | 2       | 96935          | `experiments/logs/league_3p.log` |
| 4p | 2       | 96947          | `experiments/logs/league_4p.log` |

Five workers, not the six the previous run used: the box is a 6-core Mac mini
and a BGO scraper has one. 2p gets the single worker because its games are the
cheapest, so it still ends up with the most generations of the three.

### !! These arms run from the WORKTREE, not from the main checkout

    /Users/pt/tta-ai-trainfix          <- branch train/loop-fix

They have to: `--candidate-bot` does not exist on `master`. `run_league.sh`
does `cd "$(dirname "$0")/.."`, so the state dir, the logs and the engine code
are all the worktree's. Consequences, in order of how much they will hurt:

* **Do not `git worktree remove` this while the arms are running.** They die.
* Every hourly restart picks up the worktree's current code, so committing to
  `train/loop-fix` changes what the running arms do at the next restart.
* When `train/loop-fix` is integrated into `master`, the arms should be
  stopped, `experiments/league_state/` copied into the main checkout, and the
  arms relaunched from there **with `--candidate-bot` still on the command
  line** (see the resume gotcha below).

State is `/Users/pt/tta-ai-trainfix/experiments/league_state/`, which was empty
at launch, so this is a genuinely clean start from `DEFAULT_WEIGHTS` and not a
resume. The previous 1-ply run's state was moved to
`/Users/pt/tta-ai/experiments/archive_preplan/` rather than deleted.

## Why quiescent and not plan, in numbers

`tools/arch_cost.py` (cpu-seconds per game, `workers=1`, `TTA_JOURNAL=1` as
`run_league.sh` sets it; CPU rather than wall clock because the box is shared
and wall clock there measures the neighbours). Against `book`, i.e. one
searching seat; `mirror` is every seat searching and is the `past` tier's shape
too:

| architecture | 2p | 3p | 4p | 4p mirror | x 1-ply |
|---|---|---|---|---|---|
| `weighted` (1-ply) | 0.138 | 0.195 | 0.265 | 0.861 | 1.0x |
| `quiescent:levels=1` | 0.272 | 0.357 | 0.672 | 2.048 | **2.0-2.7x** |
| `plan:width=1` | — | — | 2.402 | — | 9.1x |
| `plan:width=2` | — | — | 4.159 | — | 15.7x |
| `plan:width=4` | — | — | 8.656 | — | 32.7x |
| `plan:width=8` | 7.154 | 9.616 | 17.423 | 51.298 | **49-66x** |

Converted to the trainer's own budget, using the previous run's median games
and seconds per generation at 1-ply (`generations_<K>p.jsonl`, last 40 gens,
2 workers): 132 games / 60.7 s at 2p, 216 / 128.6 at 3p, 300 / 224.6 at 4p.

| | 2p games/h | 3p games/h | 4p games/h | generations in 12h (2p/3p/4p) |
|---|---|---|---|---|
| 1-ply `weighted` | 7 830 | 6 050 | 4 810 | **712 / 336 / 192** |
| `quiescent:levels=1` | 3 920 | 3 020 | 1 850 | **356 / 168 / 74** |
| `plan:width=8` | 142 | 106 | 78 | **13 / 6 / 3** |

### !! "PlanBot is unaffordable" is a TRAINING statement only

**PlanBot remains the strongest policy we have and it is entirely affordable at
play time.** A human game is a few hundred decisions; 17 cpu-seconds a game is
nothing to *play* and hopeless to *train*, because training spends millions of
games. Nothing here is a reason to stop shipping PlanBot, and nobody should
read this table as "PlanBot is too slow".

For training it is not close. A hill-climber needs hundreds of generations;
three at 4p is nothing. Even the `width=1` ablation — which
`docs/BOT_ARCHITECTURE.md` scores at only 62.3% against 1-ply, versus 88.6% at
`width=8` — costs 9.1x, i.e. ~21 generations in 12h at 4p. There is no point on
the PlanBot cost curve that trains.

QuiescentBot at 2.0-2.7x scores +5.8/+9.5/+16.7 points over 1-ply at 2p/3p/4p
(`docs/DEEPER_SEARCH.md` 4) and is the best strength-per-cpu-second available.
`LEVELS=1`, not 2: 2 is a measured regression because it models the rivals as
quiescent when they are 1-ply.

### The open question this run does NOT answer

**We are training a QuiescentBot-tuned vector as an affordable PROXY for the
PlanBot-tuned vector we actually want. Whether it transfers is unknown and must
not be assumed.**

The argument for expecting transfer is that both bots fix the SAME root cause —
`apply()` stopping at a pending decision, which made whole move classes (pacts,
aggressions, colony bids, action cards) look strictly dominated at 1 ply, so
their weights got no gradient at all. Under quiescence those weight dimensions
become live. `docs/DEEPER_SEARCH.md` 4.0 measures the mechanism directly:
`aggression` is ranked first at **0** of 72 decisions at 1 ply and at **23**
after quiescence.

That is an argument, not a measurement. The measurement that would settle it is
cheap and should be the first thing done when this run has a champion worth
testing: **play the quiescent-trained vector under PlanBot against the
1-ply-trained vector under PlanBot.** If the quiescent-trained vector is not
better *under PlanBot*, the proxy failed and the retargeting bought nothing.

### Cost caveat, and a trap in the existing docs

**`docs/BOT_ARCHITECTURE.md`'s "~16x current" for PlanBot is wrong under the
trainer's own flags — the real figure is 49-66x.** The 16x was computed against
a `TTA_JOURNAL=0` 1-ply baseline (~0.47 cpu-s/game at 2p). `run_league.sh`
exports `TTA_JOURNAL=1`, which speeds up `WeightedBot` by 1.2-1.5x and does
nothing for the search bots (they hold several live trial states and must stay
on `copy_state`), so the ratio against the baseline that actually runs is
3-4x larger. This is the identical mistake `docs/DEEPER_SEARCH.md` 3.1 already
caught and corrected for QuiescentBot (1.2x claimed, 1.65-2.65x real); it was
never applied to the PlanBot figure. Do not budget from the 16x.

`docs/DEEPER_SEARCH.md` 3.1: quiescent cost rises with how much the vector
attacks (4.05% of candidates went pending under the old champion, 9.76% under
`DEFAULT_WEIGHTS`). The table above was measured on `DEFAULT_WEIGHTS`, so a
*trained* quiescent champion will cost more than 2.7x, not less. Expect the
generation counts to come in under the estimates.

## The pre-registered observable

This run is also a partial test of `docs/CULTURE_GAP.md`'s recorded prediction:
that unless fixes #1 and #2 land, restarted arms re-inflate `culture_rate` and
re-flatten the `*_early`/`*_late` phase multipliers within a few hundred
generations.

**Fix #1 landed. Fix #2 was STOOD DOWN and the gate's scoring is unchanged** —
its premise did not survive a head-to-head measurement (CULTURE_GAP 23: the
supposedly perverse `culture_rate = 35.574` beats the vector it was inflated
from 41.7% ± 7.8% against a 25.0% null). So this run tests the prediction with
only one of its two fixes in place, and **`culture_rate` re-inflating here is
NOT evidence that anything is broken** — on current evidence that axis is one
the gate is right to pay for. Read observable 1 below, which is clean; treat
observables 2 and 3 as descriptive.

So, on the record, to be read off `league_state/ladder_<K>p/`:

1. **No phase multiplier should sit at exactly 0.000.** Under the old
   one-sided clamp that happened to 15.9% of positive-default multipliers under
   pure drift alone. Any exact zero now is a real finding, not the guard.
2. **`culture_rate` should not reach the 20-35 range** that both the old 2p and
   4p champions reached. It will still random-walk — `mutate`'s step is
   proportional to `|w|` and nothing in this run adds a restoring force
   (CULTURE_GAP fix #4, not landed) — so the honest bar is the drift null in
   CULTURE_GAP 16b, not zero movement.
3. **Shape retention `|late - early| / 4.0` should stay near 1.0**, not collapse
   to 0.08-0.11 as it did at 2p and 4p.

Check with `python3 tools/drift_sim.py` for the null and
`python3 -m experiments.hillclimb_league --players <K> --report` for the arm.

## The two things not to get wrong

1. **`--init default` is IGNORED once the state dir holds a champion.** It
   RESUMES. To start genuinely clean you must move `experiments/league_state/`
   aside first. `experiments/champion_4p.json` (the old top-level file, not the
   one in `league_state/`) holds a degenerate vector with `science=-6.089` —
   never warm-start from it.
2. **`--candidate-bot` is NOT persisted.** The architecture is a property of
   the run, not of the vector, and `champion_<K>p.json` records only the
   vector. `run_league.sh` forwards its extra args to every hourly restart, so
   passing it once is enough — but **resuming an arm by hand without the flag
   silently reverts it to 1-ply**, and nothing will complain. The startup line
   `[Kp] trained architecture: ...` in the log is how you check.

## Resuming

    cd /Users/pt/tta-ai-trainfix
    nohup experiments/run_league.sh <K> 12 <W> 2 12 4 1.2816 \
        --init default --weight-guard clamp --past-k 2 \
        --candidate-bot quiescent:levels=1 >/dev/null 2>&1 &

Progress:

    pgrep -fl run_league.sh
    python3 -m experiments.hillclimb_league --players <K> --report
    tail -f experiments/logs/league_<K>p.log

## Do not commit

The trainer constantly rewrites `experiments/champion_*.json`,
`experiments/league_state/**` and `experiments/league_*p/`. None of it is for
git. Never `git add -A` in this repo while a run is live.

---

## Previous run (1-ply), 2026-07-26 — superseded

Three detached supervisors, 12h each, `run_league.sh <K> 12 2 2 12 4 1.2816
--init default --weight-guard clamp --past-k 2`, PIDs 26277/26278/26279,
relaunched 18:41 on the journal engine (`17c03ea`) after an earlier launch
(97887/97888/97889) was stopped and resumed. Reached 2p gen 355, 3p gen 42,
4p gen 130 before being stood down. That state is now in
`/Users/pt/tta-ai/experiments/archive_preplan/`.

Operational notes from it that still apply:

* Generations get slower as a run proceeds — the climber spends more evaluation
  blocks per candidate (48 -> 144 -> 192 games). That is by design, not a hang.
* Gate opponents can veto (culture-margin scoring on the book/variant tiers);
  vetoes appear in the `veto` field of `generations_<K>p.jsonl`. If accepts
  stall completely for many tens of generations, the lever is `--veto-z` (a
  gate vetoes when `edge + z*se < 0`; lower z = harder to veto).
