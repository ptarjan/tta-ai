# Unattended run — state as of 2026-07-27 08:05, handoff note

Written because the arms are set to run 48h with nobody watching. If you are
picking this up cold, read this first.

## The arms

Budget **48h from 2026-07-27 08:04**, i.e. until **2026-07-29 08:04**.

| K  | workers | block | init                                    |
|----|---------|-------|-----------------------------------------|
| 2p | 1       | 12    | `default` (resumes existing state)       |
| 3p | 2       | 12    | `default` (resumes existing state)       |
| 4p | 2       | **24**| warm start from the **2p** champion      |

Two deliberate asymmetries at 4p, both from `docs/FOURP_GAP.md`:

* **block 24, not 12.** Every arm was buying the same 48 games per accept
  decision while 4p carries **2.8x the per-game spread** (sd 107.2 vs 38.8).
  One block's SE was +/-5.6 culture at 2p and +/-15.5 at 4p, with optional
  stopping on top. The 4p arm was accepting noise.
* **warm start from `hall_of_fame/preinfo_2p_gen00188.json`.** At matched
  generation count the 2p vector scores **57.4% +/- 2.5%** at 4p where the
  4p-trained vector scored **27.6% +/- 2.2%** (paired, z=9.5). The 4p arm was
  not struggling with a hard table, it was climbing toward a bad policy. Note
  this is NOT the forbidden warm start from `experiments/champion_4p.json`.

`--init` is ignored once a state dir holds a champion, so on 2p/3p it is inert
and those arms resumed their existing lineage.

## The watchdog

`experiments/watchdog.sh`, from cron every 10 min plus `@reboot`. Relaunches
any arm whose supervisor has died, with only the time **remaining** on the
original budget. It stops relaunching once
`experiments/logs/watchdog_deadline` (an absolute epoch second) has passed;
after that the cron entry is a harmless no-op. Log:
`experiments/logs/watchdog.log`.

To extend the run: rewrite the deadline file. To stop everything early: delete
it, then `pkill -f run_league.sh`.

## What changed today, and what it means for these numbers

* **Feature set grew 82 -> 89 weights** (`c2a4246`): the card row, row cost
  pressure, and public rival state. All seven new weights start at **0.0** and
  are trainable; existing champion vectors load unchanged and all four gate
  digests were unmoved by the addition. `docs/INFORMATION_AUDIT.md` has the
  proof that the evaluator previously read **none** of this — deleting the
  entire card row left the feature vector bit-identical.
* **The 2p champion is a war bot** (`docs/TWOP_PROFILE.md`): 1.98 wars/game,
  never loses one, and war/aggression transfers are 62.0 +/- 2.0 of its
  85.5 +/- 2.5 margin against book. Critically, **the fighting comes from
  quiescence, not the weights** — the identical vector at 1 ply declares 0.00
  wars.
* Therefore **the arms are training a policy whose strength is newly search-
  dependent, on a feature set that is one commit old.** Early generations of
  this run are not comparable to yesterday's.

## Traps, all of which have already bitten this repo

1. **n=48 full-check rows are unusable at 4p** (+/-6-7 win points). Do not read
   them generation-to-generation. A gen-100 row read 50.0% where n=400 said
   27.6%.
2. **`default`/`greedy`/`random` are saturated.** 100% against them is not
   evidence of anything.
3. **Every pool opponent is a BookBot subclass.** A large part of the 2p
   champion's margin is threshold effects of that one hand-written family —
   `var:military` is held to 5.5% of turns at its required +3 lead and never
   gets to fight. Beating the pool is not the same as playing well.
4. **Individual trained weights are not interpretable.** Champion marginals are
   indistinguishable from a random walk (KS p=0.14-0.80).
5. **`--candidate-bot` is not persisted**, and neither are `--objective`,
   `--hall-dir`, `--human-bots`, `--pool-weights`, `--past-k` or
   `--saturation`. Resuming an arm by hand without one silently trains against
   a different, weaker configuration; nothing crashes and nothing in the log
   says so except the startup lines. **Updated 2026-07-29:** this used to be
   enforced by "ONE `COMMON` array, no per-arm copies", which stopped working
   the moment the 2p arm was retargeted to PlanBot and the arms stopped being
   identical. `experiments/watchdog.sh` now has

   * `COMMON` — every flag that is the same for all three arms, still one
     array;
   * `arm_flags` — a `case` with one branch per arm, the ONLY place they may
     differ;
   * `REQUIRED` — the non-persisted flags. `launch` counts each one in the
     assembled command line and **refuses to start the arm** unless it appears
     exactly once, logging the refusal to `experiments/logs/watchdog.log`.

   The refusal is deliberate: a dead arm is loud (`pgrep -f run_league.sh`
   shows two supervisors), a mis-configured arm looks healthy for two days.

   The receipts, after any relaunch, in `experiments/logs/league_Kp.log`:
   `[Kp] objective:`, `[Kp] trained architecture:`, `[Kp] saturation:` and
   `[pool]`. Note the log is block-buffered — at PlanBot generation lengths
   those lines can take one full generation to appear.

6. **A saturated pool is not a strong bot.** Since 2026-07-29 the pool
   downweights opponents by their measured win rate and skips them in the
   acceptance rotation (`docs/LEAGUE_POOL.md`). Read the
   `[pool] informative ...` line: at 2p, 8 of 18 opponents are inert. "The
   champion beats the pool" says less than it used to, not more.

7. **The training proxy is not known to track shipped strength.**
   `docs/PROXY_GUARDRAIL.md` runs the check that says whether it does, from
   its own cron entry. Before quoting any arm's progress as strength, run
   `python3 -m experiments.proxy_check --report` and
   `grep "PROXY DIVERGENCE" experiments/logs/proxy_check.log`.

8. **A generation can complete ZERO games and nothing used to notice.**
   `experiments/arena.py`'s `_play` catches every exception per game on
   purpose — one engine bug must not kill a 40-hour tournament — so a bug that
   kills *every* game presented as a perfectly quiet run: mutants proposed,
   zero completed games, no accepts, hours burned, a generation log with no
   data in it. Since 2026-07-29:

   * `arena.duel` returns `error_types`, a census keyed by exception type with
     a **repr, the frame that raised and a reproducing seed** per type (plus
     the old `errors` count and `error_sample` strings). A count alone is not
     diagnosable, which is why it was ignorable.
   * `hillclimb_league.py` folds every duel of a generation into `DeathTally`.
     Any death at all gets a log line; a death rate at or above
     `HIGH_DEATH_RATE` (10%) gets a loud one and lands in the generation
     record as `engine_deaths`. Partial death only warns — it shrinks the
     accept sample without proving the arm is useless.
   * **Zero completed games halts the arm.** It cannot merely crash:
     `run_league.sh` restarts the climber in a loop and `watchdog.sh`
     relaunches the supervisor from cron every 10 minutes, so a bare crash
     spins forever and the alarm scrolls past. Instead the climber writes
     `experiments/logs/stop_league_Kp.json` (the exception census plus the
     remedy), logs a banner, and exits; `run_league.sh`, `watchdog.sh` **and
     `run()` itself** all refuse to (re)start that arm while the file exists.
     Per-arm, so a dead 2p arm does not stop a healthy 4p one.

   To resume: fix the engine, `rm experiments/logs/stop_league_Kp.json`. The
   watchdog relaunches the arm within 10 minutes; nothing else needs touching.
   `tests/test_zero_game_alarm.py` pins it, negative control first — every
   test there is a pair (break every game, require the alarm / break nothing,
   require silence), because a guard that has never been shown to fire is not
   evidence of anything.

## Open, ranked

1. **No external anchor.** Everything above is our bots playing our bots. The
   app harness (`harness/`, `docs/APP_HARNESS.md`) is the remedy and needs a
   human at a keyboard. Re-derived after the GAP-3 rival features added three
   observables: **~52-83 min/game, ~11-18h for ten usable games** (was 50-80
   and 10-16h). Three of the seven rival fields exist for features weighted
   0.0 today; if the league leaves them there, drop them and take the ~4
   min/game back.
2. **`tests/test_harness_mirror.py`** — fixed on `harness-tripwire`. The
   tripwire fired correctly: `rival_free_ca` / `rival_hand_civil` /
   `rival_wonders` were not reconstructible from the four numbers we asked
   for, and `hc=` in particular wrote into an advisor-side dict `features()`
   never read. Hidden-card counts now live on `PlayerState`, the ask grew to
   seven, and rival wonder *count* is the first hard check on the rival side.
3. **The gate's margin metric may double-count theft.** War moves culture from
   victim to attacker, so a margin of (mine - theirs) counts a steal twice, and
   the league gates accepts on margin. Under investigation; do not change the
   metric without re-measuring, it would invalidate every historical vector.
4. **Does a quiescent-tuned vector transfer to PlanBot?** Still unmeasured.
   PlanBot is 49-66x to train and remains the strongest policy we have.
5. **`has-unit`** — 9 lines, still needs its 3p/4p A/B before it earns a merge.

## BGO corpus

Scrape finished 2026-07-27 ~08:04: **1011 games** (692 2p / 133 3p / 186 4p).
Skips were principled, not failures — 717 empty journals, 277 edition
unconfirmed or expansion, 139 incomplete/resigned, 115 below skill cutoff, 114
solitaire. Lives in the `bgo-corpus` worktree, **not yet merged or analysed**.
Credentials and cookie jar were deleted automatically on completion.
