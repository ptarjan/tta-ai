# Long league training run — working note

Terse operational note, not a write-up.

## 2026-07-30: 3p and 4p reverted off QuiescentBot

**Superseded, see this entry rather than the "stay on `quiescent:levels=1`"
line right below.** `1fbf128` switched the 3p and 4p challengers back to
`plan:width=2`: QuiescentBot resolves a defender's `defense` decision by
reading the defender's real hidden `hand_military`, which the shipped bot
cannot do against a human, and the gating experiment `docs/DEEPER_SEARCH.md`
§6.2 asked for before promoting it to challenger was never run. 3p gen 1315
and 4p gen 370 were discarded (backed up outside the repo) rather than
carried forward, since every generation trained under that challenger had
tuned weights to exploit hidden information the ship policy will not have.
See `docs/DEEPER_SEARCH.md` §6.2 for the full arc.

## 2026-07-29: the 2p arm now trains under PlanBot

**The three arms are no longer identical.** ~~2p trains under
`plan:width=2`; 3p and 4p stay on `quiescent:levels=1`.~~ SUPERSEDED
2026-07-30 — see the entry above; all three arms are on `plan:width=2` now.

| K  | workers | block | candidate bot | why |
|----|---------|-------|---------------|-----|
| 2p | 1 | 12 | **`plan:width=2`** | converged: 8-10 accepts in its last 100 generations, culture flat in a 128-149 band |
| 3p | 2 | 12 | `quiescent:levels=1` | still productive: 19 accepts in its last 100 |
| 4p | 2 | 24 | `quiescent:levels=1` | still climbing |

A converged arm is spending compute re-measuring a number that has stopped
moving. The retarget buys a far smaller number of generations, each of which
measures a policy much closer to the one we would ship — which is the trade the
convergence makes worth taking, and only for that arm.

### Warm-started from P, the 1-PLY lineage — **not** from its own champion

This was the second decision and it reversed the first one. The obvious move
is to resume the arm's own gen-727 quiescent champion. The measurement says
don't:

| under `plan:width=8`, 2p | own final culture | n |
|---|---|---|
| human corpus reference | 159.5 [156.0, 163.0] | — |
| **P**, the 1-ply lineage vector, mirror | **190.6 [185.5, 196.3]** | 162 |
| **Q**, a league champion (gen 239), mirror | **61.4 [56.6, 65.9]** | 162 |
| P against `book` (`docs/PLAN_WAR_LOOKAHEAD.md` §4a) | 213.4 | 100 |
| the live 2p champion (gen 725) against `book` | **132.8** | 20 |

The first two rows are a replication run on this box on post-scoring-fix
master, cluster-bootstrapped; the last row is the proxy guardrail's own first
reading on the arm's actual champion. They agree in direction and the gap is
large either way: **under the policy this arm now gates on, the quiescent
lineage is 80-130 culture points behind the vector the previous 1-ply run
produced.** Warm-starting a PlanBot arm from it would have spent the whole
48h climbing out of a hole that a file already sitting in the repo does not
have.

So the arm starts from

    experiments/archive_preplan/league_state_1ply_20260726/champion_2p.json
    gen 355, sha256 55c7a3dea72e...   (byte-identical to
    experiments/hall_of_fame/oneply_2p_gen00355.json, which is already a
    `hall` pool opponent)

verified key-by-key in place: all 82 of P's weights identical in the live
`champion_2p.json`, the 7 newer features at `DEFAULT_WEIGHTS` (the same fill
`docs/TRANSFER_TEST.md` §7 describes), `ladder_2p/gen00000.json` equal to the
champion, gen 0.

**`--init` is ignored once the state dir holds a champion**, so this needed a
genuinely fresh state, which is the one thing this document says twice to be
careful about. The old state was archived first and verified recoverable:

    experiments/archive_2p_quiescent_20260729/
      champion_2p.json      gen 727, sigma 0.664, sha256 69e01f781a4a...
      ladder_2p/            105 files
      generations_2p.jsonl  727 rows
      state/fullcheck/ablation/guard/credit, league_2p.log,
      proxy_history_2p.jsonl

That champion remains the best vector we have **under the quiescent proxy** and
nothing about this retarget says otherwise. It is one `--init` away from being
resumed.

Two consequences worth knowing:

* the arm's `past` ladder restarts empty, so for the first generations the
  self-play tier is thin and the `hall` and external tiers carry more of the
  decision. `--past-k 6` fills it back up quickly.
* `hall:oneply_2p_gen00355` **is** the starting champion, so that row reads
  ~50% until the lineage moves. That is informative, not broken.

### The width, and the measurement that picked it

`tools/arch_cost.py --players 2 --games 8 --plan-games 4 --weights <the live
2p champion>` (cpu-seconds per game, `TTA_JOURNAL=1`, `workers=1`). **Measured
at 2p on the champion**, not extrapolated from the 4p `DEFAULT_WEIGHTS` table
further down this document — `--weights` was added for this and the difference
is large: quiescent costs 0.732 cpu-s/game on the champion against 0.272 on the
defaults, because cost rises with how much the vector attacks
(`docs/DEEPER_SEARCH.md` 3.1) and this champion is a war bot.

| architecture | vs `book` | mirror | x quiescent, real mix | generations left in 46h |
|---|---|---|---|---|
| `weighted` (1-ply) | 0.330 | 0.625 | 0.4x | — |
| `quiescent:levels=1` | 0.732 | 1.498 | 1.0x | ~1000 |
| `plan:width=1` | 1.395 | 3.316 | 2.2x | ~450 |
| **`plan:width=2`** | **2.097** | **6.504** | **4.1x** | **~200** |
| `plan:width=4` | 6.116 | 9.702 | 6.7x | ~145 |
| `plan:width=8` (ship) | 9.069 | 15.829 | 10.8x | ~90 |

"real mix" is ~3/4 mirror-shaped duels (mirror + the past/hall ladder) and ~1/4
`book`-shaped, which reproduces the arm's observed 168 s/generation under
quiescence to within 10%. So `width=2` is ~690 s/generation.

Why not the others:

* **`width=8`** — training under the exact ship policy would retire the proxy
  question entirely, and at 2p it is only 10.8x quiescent rather than the
  49-66x the old 4p/default-weights table implies. But ~90 generations at a
  ~10% accept rate is ~9 accepts. That is not a climb.
* **`width=4`** — ~145 generations, ~15 accepts. Too thin to justify against
  `width=2`'s ~200.
* **`width=1`** — cheap, but `docs/BOT_ARCHITECTURE.md` describes it as
  "everything except the multi-action search": no beam at all. It is not the
  shape we ship.
* **`width=2`** is the cheapest configuration that is still a beam search.

**This does not close the proxy gap, it narrows it** — from *quiescence vs
PlanBot* to *a narrow beam vs a wide one*, and `width=2`'s strength has never
been measured (`docs/BOT_ARCHITECTURE.md` has `width=1` at 62.3% and `width=8`
at 85.1% against 1-ply, nothing between). The residual is exactly what
`docs/PROXY_GUARDRAIL.md` measures.

Three flags come with the retarget, all in the 2p branch of
`experiments/watchdog.sh`: `--full-check-every 25 --check-games 24
--ablate-every 0`. A full check plays every pool opponent and an ablation
cycle plays four duels per weight; at ~10x the per-game cost the arm would
otherwise spend more time checking than training. Ablation is off rather than
merely rarer — single trained weights are not interpretable anyway
(`docs/UNATTENDED.md` trap 4) and this arm exists to climb.

### Two other changes landed the same day

* **`docs/LEAGUE_POOL.md`** — the pool now downweights opponents by their
  measured win rate and deepens the self-ladder (`--past-k 6`, newest-biased).
  15 of the 2p arm's 18 opponents were between 87.5% and 100%.
* **`docs/PROXY_GUARDRAIL.md`** — every few accepted champions, the new
  champion is played under `plan:width=8` against the previously validated one
  and against `book`, and the result is appended to a time series that answers
  "is proxy progress producing real progress". Separate cron entry, `nice -n
  19`, never touches an arm.

## Previous run: the QUIESCENT arms, launched 2026-07-27 from `train/loop-fix`

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
| 2p | 1       | 98146          | `experiments/logs/league_2p.log` |
| 3p | 2       | 98147          | `experiments/logs/league_3p.log` |
| 4p | 2       | 98148          | `experiments/logs/league_4p.log` |

Five workers, not the six the previous run used: the box is a 6-core Mac mini
and a BGO scraper has one. 2p gets the single worker because its games are the
cheapest, so it still ends up with the most generations of the three.

### These arms run from the MAIN CHECKOUT

    /Users/pt/tta-ai                   <- branch master

They were launched from `/Users/pt/tta-ai-trainfix` (PIDs 96921/96935/96947),
because `--candidate-bot` did not exist on `master` yet. Once `train/loop-fix`
merged (`7e3b7d5`) that reason was gone, and running from a worktree is
actively harmful: `run_league.sh` does `cd "$(dirname "$0")/.."`, so the arms
would keep restarting into the worktree's frozen snapshot and never pick up
anything landing on `master` — which defeats the entire point of the hourly
restart.

So at gen ~2 the worktree arms were stopped, `experiments/league_state/` was
copied into the main checkout, and they were relaunched from there with
`--candidate-bot` still on the command line. Confirmed at launch by the
`[Kp] trained architecture: quiescent {'levels': 1}` line in each log — that
line is the only thing that tells you the architecture, because the flag is
not persisted in the champion file.

The worktree arms' state was a genuinely clean start from `DEFAULT_WEIGHTS`,
and copying it forward preserves that lineage. The previous 1-ply run's state
is in `/Users/pt/tta-ai/experiments/archive_preplan/` rather than deleted.

`/Users/pt/tta-ai-trainfix` is now idle and safe to `git worktree remove`.

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

   Since 2026-07-29 the arms differ from each other, so `experiments/
   watchdog.sh` carries per-arm overrides in one `arm_flags` function and
   **refuses to launch an arm** whose assembled command line is missing any of
   `--candidate-bot --objective --hall-dir --human-bots --pool-weights
   --past-k --saturation`. A dead arm is loud; a silently mis-configured one
   is not. See `docs/UNATTENDED.md` trap 5.

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
