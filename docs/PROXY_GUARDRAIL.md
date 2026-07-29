# The proxy guardrail: is the number the league climbs the number we ship?

Date: 2026-07-29. Code: `experiments/proxy_check.py`,
`experiments/proxy_watch.sh` (cron, every 20 min),
`tests/test_proxy_check.py`. Output: `experiments/logs/proxy_check.log` and
`experiments/league_state/proxy_history_<K>p.jsonl`.

Read `docs/TRANSFER_TEST.md` and `docs/PLAN_WAR_LOOKAHEAD.md` first. This
document is the monitor those two findings imply and nobody had built.

---

## 1. Why

The league accepts a champion when it beats its parent on a paired score
measured under the **training** architecture (`--candidate-bot`). That is a
proxy for what we care about: how the weight vector plays under the policy we
would ship, `plan:width=8`. The two have already come apart once, and nothing
in the loop noticed:

* `docs/TRANSFER_TEST.md` — the quiescent-trained vector Q is **+36.3 ± 4.8**
  margin better than the 1-ply-trained vector P under the training proxy, and
  **−32.5 ± 6.9 worse** under `plan:width=8`, against a common opponent. Head
  to head under PlanBot it lost at a **2.5% ± 1.1%** win share. The proxy did
  not merely mis-state the size of an improvement; it got the **sign** wrong.
* `docs/PLAN_WAR_LOOKAHEAD.md` — giving PlanBot a war lookahead removed the
  inversion (the same head to head is now **52.2% ± 3.7%**, **+1.4 ± 5.3**).
  It did **not** make the proxy predictive: the proxy still says +36.3 ± 4.8
  where the ship policy says a null. §6 of that document states it plainly —
  the proxy went from *actively wrong* to *uninformative about magnitude*.

Both of those are one-off measurements on frozen vectors, made by a human-driven
agent. Neither is a monitor. So an arm can climb for two days, accept a hundred
champions, and there is no artefact anywhere that answers "did any of that
reach the policy we would ship". This is that artefact.

It matters more, not less, after the 2026-07-29 retarget of the 2p arm
(`docs/TRAINING_RUN.md`): that arm now trains under `plan:width=2`, which
shrinks the proxy gap from *quiescence vs PlanBot* to *a narrow beam vs a wide
one* — but shrinking a gap is not closing it, and `width=2`'s strength has
never been measured (`docs/BOT_ARCHITECTURE.md` has `width=1` at 62.3% and
`width=8` at 85.1% against 1-ply, and nothing in between).

**The guardrail's very first reading is also the evidence that this was a real
risk and not a hypothetical.** It measured the live 2p champion at **132.8**
own culture against `book` under `plan:width=8`, next to
`docs/PLAN_WAR_LOOKAHEAD.md` §4a's **127.8** for the frozen quiescent-trained
vector and **213.4** for the 1-ply lineage vector. 725 generations of proxy
progress had produced a suppression engine holding `book` to 43.2, not a
production one — and that single number is what the 2p arm's warm start was
changed on the strength of. Note that the 2p series therefore **restarts** with
the P lineage: readings before 2026-07-29 08:14 are archived in
`experiments/archive_2p_quiescent_20260729/proxy_history_2p.jsonl` and describe
a different lineage.

## 2. What it measures

Every `--every-accepts` accepted champions, **or** `--max-hours` since the last
reading (whichever comes first, so a slow arm still produces a series), for one
arm:

1. **Head to head under the ship policy.** The newly accepted champion against
   the *previously validated* champion, both played by `plan:width=8`, seat
   rotated over the same deals. Null is 1/players.
2. **An absolute anchor.** The same new champion against `book` under the same
   policy, reported as **own final culture**. A head-to-head chain is
   relative, and a chain drifts; own culture against a fixed external opponent
   is comparable across the whole series and against the numbers already
   written down — human 2p median **159.5** (`docs/HUMAN_BASELINE.md`), and
   under `plan:width=8` against `book`, **213.4** for the 1-ply lineage vector
   and **127.8** for the quiescent-trained one
   (`docs/PLAN_WAR_LOOKAHEAD.md` §4a).

It also records the proxy's own claim for the same interval: the number of
accepts and the sum of their accepted edges. That number is positive by
construction — an accept requires a positive one-sided lower bound — which is
the whole point. The proxy always claims progress. The question is only ever
whether the ship policy agrees.

## 3. How to read the output

`experiments/logs/proxy_check.log`, one block per reading:

```
==============================================================================
[2p] PROXY CHECK <time>  policy=plan:width=8  gen A -> B (N accepts, M gens)
  proxy claim   : N accepted champions, summed accepted edge +0.1687
  ship policy   : culture margin +11.7 +/- 11.9 (95% CI [-0.2, +23.6],
                  needs +5 to confirm, +/-15 to resolve)
                  own culture 103.3 vs 91.6; win share 65.0% +/- 15.0% vs
                  null 50.0% (secondary: a 0/1 step with ~10x the variance)
                  over 40 games (20 deals, 1529s)
  anchor vs book: own culture 132.8 (book 43.2), win share 90.0% +/- 13.5%
  VERDICT       : inconclusive   (NOT MEASURED -- the CI is too wide to call)
  history (every reading for this arm):
    at                champ   base  acc   margin    +/-   win%  own cult  vs book  verdict
    ...
==============================================================================
```

### The four verdicts, and why the statistic is culture and not win share

| verdict | rule | meaning |
|---|---|---|
| `confirms` | margin lower bound > **+5** | a real gain, resolved |
| `INVERTED` | margin upper bound < **−5** | a real **loss**: the champion the proxy chose is worse under the ship policy than the one it replaced. `docs/TRANSFER_TEST.md`'s failure, live |
| `flat` | half-width ≤ **15** and the CI covers the no-effect band | measured, and there is nothing there |
| `inconclusive` | half-width > **15** | **not measured.** Not reassurance, not a divergence — an instrument problem |

The statistic is the paired **culture margin**, not win share. Win share is a
0/1 step with ~10x the paired variance, it saturates against `book` at
0.94-0.97 under PlanBot (`docs/TRANSFER_TEST.md` §3), and every finding in
`docs/TRANSFER_TEST.md` and `docs/PLAN_WAR_LOOKAHEAD.md` is quoted in margin
for exactly that reason. Win share is still printed, as a secondary.

**`inconclusive` is the verdict that keeps this file honest, and the first
version did not have it.** That version's first real reading — 2p, gen 657 →
725, 5 accepts, 20 deals — came back at a win-share lower bound of **50.03%**
against a 50% null and printed `confirms`. A coin flip that landed right,
reported as reassurance: `docs/UNATTENDED.md` trap 1 (an n=48 row read 50.0%
where n=400 said 27.6%) committed by the very thing meant to catch it. Under
the current rule that reading is `inconclusive`, which is what it always was.

Two things changed together so the cost per hour did not move: `--deals`
roughly doubled (2p 20 → 40, 3p 10 → 20, 4p 6 → 12) **and** the cadence went
sparse (2p every 5 accepts → 12, 3p 15 → 20, 4p 12 → 15).

### Three alarms, because they need three different responses

| alarm | fires when | whose problem |
|---|---|---|
| `!! PROXY DIVERGENCE !!` | any `INVERTED` reading, or `--divergence-run` (3) consecutive **resolved** readings with no gain while accepts pile up | the training loop |
| `!! GUARDRAIL NOT RESOLVING !!` | 3 consecutive `inconclusive` readings | the guardrail: raise `--deals` and `--every-accepts` together |
| `!! PROXY GUARDRAIL STARVED` | `--stale-hours` (24) with accepted champions and no successful reading at all | the box: a lock that never clears, or a dead cron entry |

`inconclusive` readings deliberately do **not** count toward a divergence.
They are the instrument failing, not the proxy failing, and conflating the two
is how a guardrail ends up crying wolf about the training loop when the real
fault is its own sample size.

Grep for all three:

```
grep -nE "PROXY DIVERGENCE|NOT RESOLVING|STARVED" experiments/logs/proxy_check.log
grep -n "gave up after waiting" experiments/logs/proxy_watch.log
python3 -m experiments.proxy_check --report        # the whole series, all arms
```

The **absolute trend** line at the bottom of each block is the one to read if
you only read one thing. It is the answer to "is proxy progress producing real
progress": own culture against a fixed opponent under the ship policy, first
reading to last, alongside the number of accepted champions that bought it.

## 4. What to do about a divergence

Nothing automatic. The guardrail deliberately has **no** authority over the
arms — it cannot stop, restart or reconfigure one, and it holds no lock any
arm waits on. It reports.

If it fires, the options are the ones `docs/TRANSFER_TEST.md` §8.3 already
enumerated, and they are *decisions*, not fixes:

1. **Retarget the arm's `--candidate-bot` closer to the ship policy**, as the
   2p arm was on 2026-07-29. Costs generations; `tools/arch_cost.py --players
   <K> --weights <the arm's champion>` is how you price it. Measure at the
   player count you are retargeting, on the champion — `DEFAULT_WEIGHTS`
   understates the search bots badly (2p quiescent: 0.732 cpu-s/game on the
   champion against 0.272 on the defaults).
2. **Fix the search asymmetry** that the proxy and the target disagree about,
   which is what `docs/PLAN_WAR_LOOKAHEAD.md` did for wars. Cheapest when it
   exists, and it is a change to a bot rather than to the trainer.
3. **Fix the objective**, if the divergence is the metric overpaying for
   something the ship policy cannot cash — `margin_share` paying twice for a
   stolen culture point was exactly that, and `--objective blend` is the
   standing fix.

A `flat` run is *not* on its own evidence for any of the three. Check the
achieved CI first: at 20 deals a 2p reading resolves a ~15-point win-share
effect and no better, and `docs/TRANSFER_TEST.md` §7 makes the same point about
n=50 deals. If the CI is wide, the honest reading is "not measured", and the
lever is `--deals`, not the trainer.

## 5. Cost, and why it cannot hurt an arm

| | 2p | 3p | 4p |
|---|---|---|---|
| head-to-head deals | 40 | 20 | 12 |
| anchor deals | 15 | 8 | 5 |
| a reading is due every | 12 accepts / 12h | 20 accepts / 16h | 15 accepts / 20h |

The deal counts fall with player count because a `plan:width=8` game gets much
more expensive with it (measured at 2p on the 2p champion: 9.07 cpu-s/game
against `book`, 15.83 in a mirror; `docs/TRAINING_RUN.md` has 4p at 17.4 and
51.3) while the arms' generations get *slower*, so the ratio of guardrail cost
to arm throughput stays in the same few percent.

Five structural reasons it cannot damage a run:

* it is a **separate process on a separate cron entry** — it cannot block,
  slow or restart an arm, and if it dies the arms do not notice;
* `nice -n 19` and `--workers 1`;
* it reads only **ladder files**, which are written once when a champion is
  accepted and never rewritten. `champion_<K>p.json` is rewritten every
  generation and reading it would race a live arm; the ladder cannot tear;
* it writes only its own log and history file;
* a lock file means two arms never measure at once -- and it is **waited**
  on, not skipped. The first version skipped, a neighbouring agent's
  replication job held the lock every time cron looked, and
  `proxy_watch.log` filled with six "another measurement holds the lock,
  skipping" lines while nothing was ever measured. A monitor that goes quiet
  when the box is busy goes quiet exactly when you need it. Each arm now
  waits up to 5 minutes (`--lock-wait`), a stale lock is stolen after 6h, a
  second `proxy_watch.lock` stops two watcher invocations overlapping, the
  arm order rotates every 20 minutes so a slow 4p reading cannot starve the
  arm behind it, and `--stale-hours` shouts if all of that still fails.

## 6. Operating it

```
# what would happen, without playing anything
python3 -m experiments.proxy_check --players 2 --dry-run

# take a reading now regardless of schedule
nice -n 19 python3 -m experiments.proxy_check --players 2 --force

# the whole series for every arm
python3 -m experiments.proxy_check --report
```

The cron entry is

```
*/20 * * * * /bin/bash /Users/pt/tta-ai/experiments/proxy_watch.sh >/dev/null 2>&1
```

and it keeps running for 12h past the arms' own deadline
(`experiments/logs/watchdog_deadline`), because the last accept of a run is the
one you most want validated and it usually lands near the end.

**The first reading is taken as soon as an arm has two champions to compare**,
looking `--every-accepts` back rather than waiting. A monitor that says nothing
for its first N accepts has a blind spot exactly where a retarget lands.

## 7. Limits

* **It is a chain.** Each reading compares against the previously *validated*
  champion, not against a fixed reference, so k readings of "flat" do not add
  up to a measurement of the whole interval. The `book` anchor exists because
  of this and is the series to trust for absolute movement.
* **The n is small on purpose.** A 2p reading is 40 games. It is a usable
  margin/culture instrument and a poor win-share instrument — the same
  warning `docs/TRANSFER_TEST.md` §7 gives. `flat` frequently means "not
  resolved", and the log prints the CI so you can see which.
* **`book` is one opponent, and the pool is a monoculture** — every pool
  opponent is a `BookBot` subclass (`docs/TWOP_PROFILE.md` §9). The anchor is
  an anchor, not an absolute standard.
* **It validates the vector, not the search.** If `plan:width=8` itself is not
  the right thing to ship, this guardrail will happily confirm progress toward
  it.
* **It cannot separate "the proxy decoupled" from "the arm stopped
  improving".** A converged arm produces `flat` readings honestly. Cross-read
  it against the arm's own accept rate before concluding anything about the
  proxy.
