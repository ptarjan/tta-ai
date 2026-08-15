# Standing hazards — every one of these has already cost this repo a bug

**This is `docs/UNATTENDED.md`, renamed and generalised on 2026-07-30.**  That
file was a handoff note for a 48-hour unattended run whose window has passed;
its trap list was the part worth keeping, and other lanes had accumulated their
own.  They are all here now.

**Trap numbers 1-8 in §1 are unchanged from the old `UNATTENDED.md` and are cited by
number from a dozen places in `tools/`, `tests/` and `experiments/`.  Do not
renumber them.**  New hazards go in the later sections, which are unnumbered.

Open work lives in [`docs/OPEN_ITEMS.md`](OPEN_ITEMS.md), not here.

**Two entries below are now wrong, and are left only because they are cited by
number.** (a) *"Do not run any git command while league arms are running"* — that
was true of the Python league, which executed the working tree directly. The Rust
arms run a compiled `rust/target/release/climb`, so ordinary git operations do not
disturb them; commit and push freely. What still holds is that a rebuild plus a
restart is what makes a change live. (b) The *fingerprint gate* (`tools/gate.sh`,
eight NARROW/WIDE arms) was Python-only and has been deleted; the equivalent
guarantee now comes from `cargo test`, `cargo clippy -D warnings` and — for
anything touching the rules — the corpus sweep in `analysis/GUARD_METHOD.txt`.

**The hazard that replaced them, 2026-08-15:** this repo was for weeks a stale
branch with the entire live engine sitting UNTRACKED on top of it. An agent tidied
up with `git reset --hard origin/master` and destroyed a day of work; the tree
still compiled and all tests still passed. It is a normal checkout now. **Keep it
that way — never leave live code untracked — and prove any rules change with the
corpus sweep, never with a green build.**

---

## 1. Training-loop traps (the original `UNATTENDED.md` numbering)

1. **n=48 full-check rows are unusable at 4p** (+/-6-7 win points).  Do not read
   them generation-to-generation.  A gen-100 row read 50.0% where n=400 said
   27.6%.  Generalised: **any number in this repo below n=200 is provisional** —
   the trainer accepts on n=48 (2p/4p) or n=144 (3p) at a one-sided 90%
   threshold (`--accept-z 1.2816`), which is mechanically a false-acceptance
   machine over hundreds of generations, and is the named root cause of this
   repo's recurring "confident result that later reverses" pattern.
2. **`default`/`greedy`/`random` are saturated.**  100% against them is not
   evidence of anything.
3. **Every pool opponent was a BookBot subclass.**  A large part of the 2p
   champion's margin is threshold effects of that one hand-written family —
   `var:military` is held to 5.5% of turns at its required +3 lead and never gets
   to fight.  Beating the pool is not the same as playing well.  (Partly
   addressed by the `hum:*` archetypes — [`docs/HUMAN_BOTS.md`](HUMAN_BOTS.md) — whose logistic
   gate degrades to x0.42 rather than `var:military`'s x0.18.)
4. **Individual trained weights are not interpretable.**  Champion marginals are
   indistinguishable from a random walk (KS p=0.14-0.80) even though the same
   champion beats its own drift-siblings 0.94-0.99.  The improvement lives in
   joint structure, not in any single coordinate.  "`culture_rate_early` = 0.000"
   is not a strategic statement.  Corollary, from [`docs/OPENING_AUDIT.md`](OPENING_AUDIT.md): **"the
   AI moved this weight, therefore it matters" is never a valid inference unless
   somebody ablated it.**  Mutations move ~19 weights at once and are accepted on
   one bundle-level test.
5. **`--candidate-bot` is not persisted**, and neither are `--objective`,
   `--hall-dir`, `--human-bots`, `--pool-weights`, `--past-k` or `--saturation`.
   Resuming an arm by hand without one silently trains against a different,
   weaker configuration; nothing crashes and nothing in the log says so except
   the startup lines.  `experiments/watchdog.sh` now has

   * `COMMON` — every flag that is the same for all three arms, one array;
   * `arm_flags` — a `case` with one branch per arm, the ONLY place they may
     differ;
   * `REQUIRED` — the non-persisted flags.  `launch` counts each one in the
     assembled command line and **refuses to start the arm** unless it appears
     exactly once, logging the refusal to `experiments/logs/watchdog.log`.

   The refusal is deliberate: a dead arm is loud (`pgrep -f run_league.sh` shows
   two supervisors), a mis-configured arm looks healthy for two days.

   The receipts, after any relaunch, in `experiments/logs/league_Kp.log`:
   `[Kp] objective:`, `[Kp] trained architecture:`, `[Kp] saturation:` and
   `[pool]`.  The log is block-buffered — at PlanBot generation lengths those
   lines can take one full generation to appear.

   Related: **`--init <path>` is ignored once the state dir holds a champion.**
   It resumes, silently.  To genuinely restart clean, move the state dir aside
   first.

6. **A saturated pool is not a strong bot.**  Since 2026-07-29 the pool
   downweights opponents by their measured win rate and skips them in the
   acceptance rotation ([`docs/LEAGUE_POOL.md`](LEAGUE_POOL.md)).  Read the `[pool] informative
   ...` line: at 2p, 8 of 18 opponents are inert.  "The champion beats the pool"
   says less than it used to, not more.  If the `informative` line ever reads a
   small number **and** the `tier share` line moves, something is wrong — those
   two are independent by construction.
7. **The training proxy is not known to track shipped strength.**
   [`docs/PROXY_GUARDRAIL.md`](PROXY_GUARDRAIL.md) runs the check that says whether it does, from its
   own cron entry.  Before quoting any arm's progress as strength, run
   `python3 -m experiments.proxy_check --report` and
   `grep "PROXY DIVERGENCE" experiments/logs/proxy_check.log`.
8. **A generation can complete ZERO games and nothing used to notice.**
   `experiments/arena.py`'s `_play` catches every exception per game on purpose —
   one engine bug must not kill a 40-hour tournament — so a bug that kills
   *every* game presented as a perfectly quiet run: mutants proposed, zero
   completed games, no accepts, hours burned, a generation log with no data in
   it.  Since 2026-07-29:

   * `arena.duel` returns `error_types`, a census keyed by exception type with a
     **repr, the frame that raised and a reproducing seed** per type.  A count
     alone is not diagnosable, which is why it was ignorable.
   * `hillclimb_league.py` folds every duel of a generation into `DeathTally`.
     Any death gets a log line; a death rate at or above `HIGH_DEATH_RATE` (10%)
     gets a loud one and lands in the generation record as `engine_deaths`.
   * **Zero completed games halts the arm.**  It cannot merely crash:
     `run_league.sh` restarts the climber in a loop and `watchdog.sh` relaunches
     the supervisor from cron every 10 minutes, so a bare crash spins forever and
     the alarm scrolls past.  Instead the climber writes
     `experiments/logs/stop_league_Kp.json` (the exception census plus the
     remedy), logs a banner, and exits; `run_league.sh`, `watchdog.sh` **and
     `run()` itself** all refuse to (re)start that arm while the file exists.
     Per-arm, so a dead 2p arm does not stop a healthy 4p one.

   To resume: fix the engine, `rm experiments/logs/stop_league_Kp.json`.  The
   watchdog relaunches the arm within 10 minutes.  `tests/test_zero_game_alarm.py`
   pins it, negative control first — every test there is a pair (break every
   game, require the alarm / break nothing, require silence), because a guard
   that has never been shown to fire is not evidence of anything.

   This has fired for real: both the 3p and 4p arms silently played zero games
   for ~55 minutes (197 and 68 generations burnt) starting 27 seconds after a
   `git pull`, and `hillclimb_league` recorded `n:0` with no error text.
   **"3p accepted nothing since gen 930" was an outage, not convergence.**

## Radioactive files and vectors

* **Never warm-start 4p from `experiments/champion_4p.json`** (the top-level
  file, not the one under `league_state/`).  It holds 8-9 sign-inverted weights
  including `science = −6.089`, and collapses the win rate to 9.7% +/- 2.7%.
  `refuse_if_degenerate_champion` now tests **provenance over the informative
  keys**, not exact content, because the first version tested exact content and a
  six-generations-later frozen copy slipped past it under a different name.
* `analysis/frozen/champion_4p.DEGENERATE.json` and its twin
  `experiments/frozen/champion_4p_strengthcheck.json` reproduce all 62
  informative weights of that vector bit-for-bit.  **Every 4p number measured
  against them is quarantined** — they are left in place in
  [`docs/BOT_ROSTER.md`](BOT_ROSTER.md), [`docs/WASTED_ACTIONS.md`](WASTED_ACTIONS.md), [`docs/STRENGTH_CHECK.md`](STRENGTH_CHECK.md) and
  [`docs/INFORMATION_AUDIT.md`](INFORMATION_AUDIT.md) so they stay auditable, not because they are
  facts.  See [`analysis/frozen/README.md`](../analysis/frozen/README.md).
* `experiments/league_state/` holds the **live** champion and ladder.
  `experiments/champion_{2,3,4}p.json` and `experiments/league_4p/` are stale
  snapshots from an earlier run.  Confusing them once produced a false "the 4p
  arm has plateaued" conclusion when it was in fact the least-converged arm.
  **Any weight snapshot taken for measurement must be copied to `/tmp` first and
  must come from `league_state/`.**
* `experiments/baselines.jsonl` carries no timestamp, generation or seed on any
  row.  Do not quote it; re-run `experiments/evaluate.py`.
* Three tools (`tools/quiesce_bench.py`, `tools/no_credit_check.py`,
  `tools/behaviour_counts.py`) have historically defaulted to the invalidated
  `experiments/champion_4p.json` and printed plausible numbers for a crippled
  vector without erroring.  `tools/culture_probe.py` defaults to the live
  `league_state/` path and is the pattern to copy.

## Do not "fix" these

* **`end_turn_bias` must stay negative and must not be zeroed.**  It is a trained
  correction (−14.44 at gen 344 vs a −3.0 default) for the fact that `end_turn`'s
  production payoff lands inside `apply` while no other candidate's does.
  Removing it has been measured **five separate ways** and made the bot much
  weaker every time (down to 11.0% against a 50% null).  The phantom bonus acts
  as an accidental move-quality confidence filter.  There is a standing warning
  in the code comment; [`docs/WASTED_ACTIONS.md`](WASTED_ACTIONS.md#6-the-obvious-fix-makes-the-bot-worse) §6 is the measurement.  Combining
  the card-valuation fix with same-horizon scoring is *worse* than the card fix
  alone (39.8% +/- 6.7% vs 69.6% +/- 4.5%).
* **The ten phase multipliers are deliberately exempt from sign clamping**
  (`culture_early`, `culture_rate_late`, `science_rate_late`, `food_rate_late`,
  `resource_rate_late`, `workers_late`, `strength_rel_early`, `tech_levels_late`,
  `wonder_progress_late`, `hand_value_late`).  Their sign is not gauge-invariant:
  an affine reparametrisation leaves the policy unchanged.
* **`tests/test_harness_mirror.py::ForcedRivalsAreExact` must not be deleted, and
  must not be "fixed" by appending a new feature name to `RIVAL_FEATURE_KEYS`** —
  that silences the alarm and feeds the bot a zero for a whole game.  It has
  already fired once for real.
* **Do not change the gate's margin metric without re-measuring** — it would
  invalidate every historical vector.
* **`USE_JOURNAL` / `TTA_JOURNAL` default OFF everywhere except
  `experiments/run_league.sh`.**  Deliberately, so the copy path remains the
  independent oracle the journal is checked against.  Do not default it on
  globally.
* **`tests/test_half_priced_cards.py` tracks 13 sanctioned half-priced cards; a
  14th entry is a test failure.  `TestEveryLeaderIsPriced.STILL_FLAT` tracks 4
  leaders; growing that list is a failure.**

## Measurement traps

* **`arena.duel` plays each deal twice with the seats swapped** (`seat = g %
  players`, `seed = seed0 + g // players`), so per-game confidence intervals must
  be **deal-paired**, never treated as independent samples.  Escalate to
  **block**-clustering when blocks are over-dispersed (chi-squared test).
  `experiments/paired_stats.py`.  This has bitten twice: once producing an
  interval too wide, once flipping a "leaders hurt, z = −2.1" headline into a
  null (z = −1.46, p = 0.15) in the same document where a paired check run "as a
  check" had already been right.
* **`--seed-base` is not a run identifier.**  An independent replication must
  move it by at least `games / players`, or it replays nearly the same deals.
* **A shutout is not a null.**  `arena.mean_ci` is a normal approximation over
  per-game shares; when a bot wins every game the variance is 0 and it prints
  `100.0% +/- 0.0%, p=1.0000`.  That p is an artefact of a zero standard error.
  Likewise a 0.000 win rate against an opponent you always lose to is a
  *saturated* statistic, not a null, and cannot distinguish "no effect" from
  "huge effect".
* **Assert the lever conducts before spending games.**
  `arena.assert_lever_conducts()` / `tools/conduction_table.py`.  A 12,800-game
  null was an arithmetic identity because the weight under test was absent from
  the vector under test.
* **"Inert" is a statement about coverage, not correctness.**  A change that
  moves no digest means those 135 games cannot catch a regression in it.
* **A weight the climb never has to PAY for is unconstrained, and it will
  drift.**  A feature the evaluator can read but can never spend anything to
  increase is fitted on a free lunch: nothing punishes a wrong value, so noise
  carries it wherever it likes.  The archived 3p champion reached `strength`
  3.42 and `strength_rel_early` 7.35 — one soldier valued at roughly one
  culture *per turn* — purely because for the whole of this project's history
  no card in the game could sell it a soldier.  The moment
  `weighted.strength_marginal` made it pay its own stated price, that vector
  bought 4.16 unit technologies a game and lost at **14.6% against a 33.3%
  null**.  Before opening a new channel onto an existing weight, ask what has
  ever constrained that weight; if the answer is "nothing", expect the first
  measurement to be a regression *of the weight*, and check it against
  `DEFAULT_WEIGHTS`.  [`docs/CARD_BLINDNESS.md`](CARD_BLINDNESS.md#1452-3p-on-the-archived-champion-a-large-unambiguous-regression) §14.5.2.
* **A card whose cost is priced while its gain sits at 0.0 is biased, not
  inert.**  More generally: *adding a 0.0-default feature for one side of a trade
  whose other side is already priced does not leave the card neutral; it biases
  it, and the direction depends on which side you just made visible.*
  `tests/test_half_priced_cards.py`.  And the corollary that cost a second
  measurement: **un-biasing it is not the same as pricing it.**  The ten unit
  cards were the worst instance of this hazard on record, and flooring their
  `card_potential` at zero — the bias removed, nothing added — moved "is this
  the best card on the row" only from 1 in 437 to 20 in 437.  A card worth
  exactly nothing is still not a card worth taking, so a fix that only removes
  the sign will read as a null.  [`docs/CARD_BLINDNESS.md`](CARD_BLINDNESS.md) §14.1c.
* **A swap diff is exact over `Stats` and blind to everything else**, and it
  *replaces* the static table rather than supplementing it — so any key the static
  path priced that the diff cannot see is silently dropped.  Taj Mahal's blue
  token was a live instance.
* **Two implementations of one rule always drift.**  Paid for four times in one
  night: `build_discount`, the leader hand double-count, the population-cost
  formula (four copies, three missing a term), and the `rankingCulture` block.
* **A validation metric computed on labels the model itself produced is a
  conservatism meter, not a validation metric.**  Always use a random row split;
  the neural v1 loop's shard-sorted split made validation 100% "agreement with the
  incumbent", an anti-metric that rewards inaction.
* **A better predictor is not a better policy.**  Ranking accuracy 0.669 -> 0.812
  while win rate falls 0.53 -> 0.00 on a linear ridge ladder; a Monte-Carlo
  regression net reaches 0.771 validation ranking accuracy while losing to
  everything.  Anyone proposing a learned evaluator in this project should be made
  to run that exact duel before claiming anything.
* **Uniformly-positive blocks are also the signature of a systematic asymmetry.**
  [`docs/EVENT_SEEDING.md`](EVENT_SEEDING.md) ran a seat-bias audit for exactly this reason and found
  a real ~5pp seat effect.
* **The 94.9%-of-`end_turn` figure is a draw count, not a leak measurement.**  It
  counts candidates whose trial `apply` draws a card, on `WeightedBot`, which
  never determinizes at all.  It mis-propagated across four documents before
  being corrected everywhere.  Use `tools/infoleak.py --true-card` for a real
  leak, and `tools/leak_impact.py` to ask whether the chosen *move* changes.
* **A 2ms sampling profiler on the Mac Mini inflates small, frequently-entered C
  frames by one to two orders of magnitude.**  `random.Random(0)` construction
  profiled at 10.8%/13.6% and A/B'd at ~4-6%.  Bound a profiler line item with
  cost x count arithmetic, or a probe that deletes the work, before spending
  effort on it; a line under ~10% on this box is not evidence on its own.  A
  profile also needs hundreds of samples — a 16-sample profile was wrong by 2x.
* **Always profile with the league's own trained champion weights, not
  `DEFAULT_WEIGHTS`.**  Defaults understate search-bot cost substantially
  (quiescent 2p: 0.732 cpu-s/game on the champion vs 0.272 on defaults).
* `tools/bench_interp.py --kinds weighted` silently benchmarks GreedyBot; use
  `engine/perf_check.py --kinds weighted`.

## Engine and determinism

* **CPython 3.12+'s `sum()` uses Neumaier compensated summation; PyPy's is naive
  left-to-right.**  Fixed by using `math.fsum` in `engine/bots/__init__.py`'s
  `evaluate` (`4290459`).  Corollary worth keeping independent of PyPy: the
  engine was never reproducible across CPython 3.11 vs 3.12+ before that fix.
* **THE 9.0 RULE: `tools/gate.sh` digests must be re-derived from scratch on both
  sides (fresh checkout plus worktree) whenever master moves under a branch.**
  Never assumed unchanged, never trusted from before a rebase.  This has bitten
  at least four times.  The digest *values* are not stable facts — they have
  moved at least eight times — so preserve the method, never the numbers.
* **`journal.begin` raises on nesting and `_J`/`_STACK` are process globals**, so
  the harness must stay `multiprocessing`-based and must never go thread-parallel
  without putting both in `threading.local` first.  States corrupt silently
  otherwise.
* **`engine/statediff.py` deliberately compares dict key order**, not just
  equality, because the engine iterates `p.techs`, `state.seeded_by` and
  `p.one_time_discount`, and a non-LIFO rollback can restore correct values in
  the wrong order while comparing `==` equal.
* **Re-run `tools/mutation_coverage.py --bot <greedy|weighted|quiescent|plan>`
  after any engine change that adds a container mutation.**  Different bots reach
  different mutation sites; one bot's coverage is not an audit.
* **`PlanBot`, `NeuralPlanBot` and `NeuralBot` determinize their search root;
  `WeightedBot` and `QuiescentBot` never call `plan.determinize` at all.**  A hard
  architectural split, load-bearing for every leak claim.  Do not "unify" it
  casually.
* **A fitted vector will resign on turn 3 if you duel it without the resign
  guard.**  `WeightedBot` never guarded `("resign",)` the way `RandomBot` always
  has; use `allow_resign=False`.
* **Cost pricing must be clamped `max(0, w)`** — a degenerate weight vector (the
  4p negative `science`) otherwise flips a cost term into a gain and the bot
  chases the most expensive card it can see (`Alchemy` priced at +67.04 under 4p
  weights vs +5.86 under 2p).
* **`rival_culture_rate`, `rival_science_rate` and `rival_strength` are provably
  inert at any weight**, because `rival_context` is computed once at the root and
  shared across all candidates, so the term cancels out of every argmax.  Pinned
  by `TestInertFeatures`.
* **Changing the evaluator invalidates every cached pool-opponent win rate.**
  The halt checklist: write the sentinel files, run `watchdog.sh` once to reap
  supervisors, confirm `pgrep -f run_league.sh` is empty, back up `state_Np.json`
  and delete `last_full_check`, remove the sentinels, confirm `0 opponents
  measured` on restart.  **A running supervisor older than the halt sentinel will
  not obey it** — `run_league.sh`'s while-loop is parsed once at launch, so only
  `watchdog.sh`'s `reap` can kill it.  Any future halt mechanism belongs in the
  watchdog, never in `run_league.sh`.
* **`pkill -f run_league.sh` alone does not stop a run.**  It kills the
  supervisor and leaves the trainer child running for up to an hour, and the cron
  watchdog then relaunches a second supervisor onto the same state dir,
  corrupting it with interleaved writes.  Use `pkill -f run_league.sh` **and**
  `pkill -f hillclimb_league`.
* **`--block` must be a multiple of `--players`** (seat-rotation balance).

## Windows compute node (`micro@100.68.145.15`)

* **`Start-Process -WindowStyle Hidden` does not work for `bash.exe`.**  Git's
  `bash.exe` re-execs `usr\bin\bash.exe`, a console app that allocates its own
  visible console regardless of the parent's window style.  Use
  `tools/hidden_launch.vbs`: `wscript.exe` is a GUI-subsystem host with no console
  of its own, and `WshShell.Run(cmd, 0, True)` creates the child's console
  **already hidden**, which every descendant then inherits.  That is why it fixes
  the whole process tree rather than the first hop.
* **A `LogonTrigger` whose `<Repetition>` has no `<Duration>` is silently dropped
  by Task Scheduler.**  `schtasks /query` reports `Repeat: Every: N/A` and the
  task runs exactly once.  This killed the GPU guard for two days.  Every trigger
  needs an explicit `<Duration>`.
* **An S4U (no interactive token) task is genuinely windowless but runs in
  session 0, where CUDA is not dependable on a consumer WDDM box.**  Do not use
  S4U for anything that needs the GPU.
* **Window-enumeration checks run over SSH are blind** — OpenSSH gets its own
  session and enumeration is session-local, so
  `Get-Process | Where-Object {$_.MainWindowTitle -ne ''}` returns PASS with zero
  results.  A verifier must run in session 1 via a Scheduled Task and print a
  non-zero sentinel count of windowed processes it *can* see, or its verdict is
  worthless (`tools/wincheck.ps1`).
* **Aliveness-by-log-mtime plus a survivor process is a permanent leak.**  The
  gaming guard killed `python.exe` only; the bash driver survived, the watchdog
  saw a stale log and started a second driver trio — one leaked trio per game
  session, forever.  Anything that restarts a worker must first reap the old one
  by stored PID (`taskkill /F /T`), not merely detect staleness.
* **The GPU guard is the sole writer of `PAUSE`; the loop only reads it**, and
  the guard must re-sync its in-memory `paused` flag against the file on every
  poll rather than trusting its own memory — otherwise if `PAUSE` vanishes
  underneath it the guard silently never re-arms, with no log line.  `PAUSE_HOLD`
  lets an operator pin training off without racing it.
* **Checkpoint writes go through an atomic stage-then-rename** (`install_ckpt`)
  because a `kill -9` mid-write on the reap path was observed truncating
  `checkpoints/best_search.pt`.
* `register_tasks.ps1` converts UTF-8 XML to UTF-16 on ingest because
  `schtasks /xml` rejects UTF-8 (`(1,40)::ERROR: unable to switch the encoding`);
  XML comments in those task files must avoid `--`.
* **torch:** pin `--threads 1` (torch otherwise grabs all cores — measured 0.25
  effective core per process when unthrottled on a shared box), and note that
  tensor *conversion*, not the forward pass, dominates neural search cost:
  `torch.from_numpy(np.asarray(...))` beats `torch.tensor(list_of_lists)` by
  roughly 2x.

## Git and multi-agent working

* **Never `git add -A` in this repo.**  A live hillclimb continuously rewrites
  `experiments/champion_*.json`, `generations_*.jsonl` and `league_*/`.  Stage
  explicit paths only.  Copy `champion_*.json` to `/tmp` before analysing it.
* **Do not `git checkout -b` in a shared working tree.**  This has already
  happened: every agent that subsequently committed in that tree landed on the
  feature branch without noticing, while `git push origin master` kept pushing an
  untouched ref.  Recovering it took a full branch audit.
* **If two refs have diverged, neither is necessarily a superset of the other.**
  A naive diff-based apply of the "ahead-looking" branch onto master would have
  deleted 785 lines of [`docs/EXPERT_STRATEGY.md`](EXPERT_STRATEGY.md).  Method: `git log --cherry-mark
  --left-right A...B` to bucket by patch-id, then re-check every "unique" commit
  by comparing **blob hashes** of the files it touched — patch-id over-reports
  across a squash — and use `git diff --stat A B` for the authoritative
  content-level accounting.
* **Never apply a branch cut from an older master as a diff against current
  master.**  Its diff reverts everything master gained since that base.

## Neural loop

* **A self-play improvement loop's target policy must be provably stronger than
  the network generating it.**  v1 was a self-imitation fixed point: scoring the
  untrained incumbent on its own generated training pairs gave **97.6% pair
  accuracy before a single gradient step**, and the loop ran 74 iterations and 41
  hours for zero promotions.  Probe this before the run, and keep a `DISAGREE`
  health meter that must not decay toward zero (v2 holds 0.54-0.58).
* **Kill conditions must be pre-registered** — `DISAGREE < 0.02` for two
  consecutive iterations; no promotion in 15 iterations with the pooled CI
  excluding 0.5; a flat anchor across 10 measurements.  v1 had none and burned 41
  hours past the point of being informative.  A flat self-play curve near 0.5 is
  not "converged": v1 sat at 0.44, reliably *below* the null, and nobody noticed.
* **Anchor hygiene:** `plan:champion_2p` is only a fixed yardstick while
  `weighted.py` underneath it is unchanged.  When it changes (once worth a +59.5%
  head-to-head swing from a single commit), truncate `anchor_best.txt` and write a
  `#`-comment row into `curve.tsv` so old and new anchor scores are never averaged
  or plotted as one series.
* **Training data must be generated with the same determinization the deployed
  bot uses.**  v1's BookBot anchor data had none, teaching the net to price
  `end_turn` — the most-evaluated move in the game — off cards that get reshuffled
  away before the deployed bot ever sees them.

## Ops notes that were the rest of `UNATTENDED.md`

The watchdog (`experiments/watchdog.sh`, cron every 10 min plus `@reboot`)
relaunches any arm whose supervisor has died, with only the time **remaining** on
the original budget, and stops once `experiments/logs/watchdog_deadline` (an
absolute epoch second) has passed.  To extend a run, rewrite the deadline file.
Log: `experiments/logs/watchdog.log`.

The BGO corpus scrape referenced by the old handoff note completed and now lives
in `sources/bgo/` — see [`docs/BGO_CORPUS.md`](BGO_CORPUS.md).  The "no external anchor" item it
carried is answered by [`docs/HUMAN_BASELINE.md`](HUMAN_BASELINE.md) and [`docs/SYSTEM_COVERAGE.md`](SYSTEM_COVERAGE.md);
the remaining open pieces of it moved to [`docs/OPEN_ITEMS.md`](OPEN_ITEMS.md#8-measurement-and-infrastructure) §8.
