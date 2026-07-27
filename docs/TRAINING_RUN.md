# Long league training run — working note

Terse operational note, not a write-up. Launched 2026-07-26 16:29 MDT from the
main checkout on master.

## What is running

Three detached supervisors, one per player count, 12h budget each:

    experiments/run_league.sh <K> 12 2 2 12 4 1.2816 \
        --init default --weight-guard clamp --past-k 2

Positional args are `PLAYERS HOURS WORKERS LAMBDA BLOCK SUBSET ACCEPT_Z`.

Workers is **2** per run, not the 3 the gradient measurement specified: three
concurrent runs share one 6-core box, so 3x3=9 would oversubscribe. 3x2=6 is
exactly the core count. Everything else is the measured config verbatim.

| K  | supervisor PID | log                             |
|----|----------------|---------------------------------|
| 2p | 26277          | `experiments/logs/league_2p.log` |
| 3p | 26278          | `experiments/logs/league_3p.log` |
| 4p | 26279          | `experiments/logs/league_4p.log` |

Relaunched 2026-07-26 18:41 on the journal engine (`17c03ea`, `docs/PYPY.md`
9.14-9.16). The first launch (PIDs 97887/97888/97889) was stopped at 2p gen
131 / 3p gen 85 / 4p gen 49 and **resumed** rather than reset — `--init
default` is ignored once the state dir holds a champion, which is the
documented gotcha below working in our favour for once. The generation
counters continuing past those numbers is the proof it resumed.

State (champion, run state, generation log, ladder) is under
`experiments/league_state/`. The supervisor restarts the climber every hour;
that is normal and every restart picks up the latest engine code and any new
`engine/bots/variants/`.

## The one thing not to get wrong

`--init default` is load-bearing. `experiments/champion_4p.json` (the old
top-level file, *not* the one in `league_state/`) holds a degenerate vector with
`science=-6.089`. Never warm-start from it.

Note `--init` is **ignored once the state dir already has a champion**. So
"restart with --init default" does NOT reset a run — it resumes. To genuinely
start clean you must move `experiments/league_state/` aside first.

Verified at launch: all three champions started at `science=+0.5`, matching
`DEFAULT_WEIGHTS`. 4p additionally applies its documented init override
`hand_potential: 0.125 -> 0.0` (known 4p regression; the pool re-decides it).

## Resuming after a restart

Nothing to do if the supervisors survived — check with:

    pgrep -fl run_league.sh

To resume a dead arm (this RESUMES from the state dir, it does not restart):

    cd /Users/pt/tta-ai
    nohup experiments/run_league.sh <K> 12 2 2 12 4 1.2816 \
        --init default --weight-guard clamp --past-k 2 >/dev/null 2>&1 &

Progress at a glance:

    python3 -m experiments.hillclimb_league --players <K> --report
    tail -f experiments/logs/league_<K>p.log

## Accept rate

This is the first run where the gate opponents can **veto** (culture-margin
scoring on the book/variant tiers). Vetoes are visible per candidate in the
`veto` field of `experiments/league_state/generations_<K>p.jsonl`.

Early reads are recorded below. If accepts stall completely — many generations,
zero accepts, most candidates carrying a `veto` — the documented lever is
`--veto-z` (a gate vetoes when `edge + z*se < 0`; lower z = harder to veto).

Early read, first ~4 minutes (8 generations across the three arms):

    2p  0 accept / 3 reject   best_lo -0.0615 -> -0.0469 -> -0.0229
    3p  0 accept / 3 reject   best_lo -0.0785 -> -0.1053 -> -0.0335
    4p  1 accept / 1 reject   gen 2 ACCEPT edge=+0.0694 lo=+0.0098 op=group:actions

**Accepts are not stalled.** 1/8 this early is fine, and `best_lo` is trending
up toward the accept threshold on 2p and 3p rather than sitting flat. The 4p
accept moved `civil_actions` 2.0 -> 1.593 and archived
`league_state/ladder_4p/gen00002.json`.

Gate vetoes ARE firing (var:military, var:infra, var:culture have each vetoed a
candidate), which is the intended new behaviour, not a fault. Revisit `--veto-z`
only if an arm goes many tens of generations with no accept at all.

Generations get slower as the run proceeds — the climber spends more evaluation
blocks per candidate (48 -> 144 -> 192 games). 16s/gen early, ~90-120s by gen 3.
That is by design, not a hang.


## Do not commit

The trainer constantly rewrites `experiments/champion_*.json`,
`experiments/league_state/**` and `experiments/league_*p/`. None of it is for
git. Never `git add -A` in this repo while a run is live.
