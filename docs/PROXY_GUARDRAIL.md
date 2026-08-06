# The proxy guardrail: is the number the league climbs the number we ship?

> **BANNER 2026-08-06: the machinery this describes is gone; the §8 finding
> is not.** `experiments/proxy_check.py`, `experiments/proxy_watch.sh` and
> the `*/20 * * * *` cron entry that ran them were Python and were deleted
> with `engine/` and the Python half of `experiments/` on 2026-08-06 — the
> current tree has no `.py` files outside one unrelated data-source script,
> no `blend`/`own_share` league objective anywhere, and no continuous
> ship-policy monitor in `rust/`. Nothing today checks whether the Rust
> `climb` league's win-share-vs-anchor accept criterion predicts strength
> under a deeper search policy; that gap is currently unmonitored. What
> survives and is still worth reading: [`docs/TRANSFER_TEST.md`](TRANSFER_TEST.md)
> and [`docs/PLAN_WAR_LOOKAHEAD.md`](PLAN_WAR_LOOKAHEAD.md) — the findings
> this monitor was built to watch for — are unaffected by the deletion; and
> §8 below's replicated result — the 3p arm's ship-policy strength fell
> **−76.6 ± 17.8** culture over 918 generations of proxy-approved accepts —
> is a real, otherwise-unrecorded finding about what an unguarded proxy
> target can do.

Date: 2026-07-29. Code: `experiments/proxy_check.py`,
`experiments/proxy_watch.sh` (cron, every 20 min),
`tests/test_proxy_check.py`. Output: `experiments/logs/proxy_check.log` and
`experiments/league_state/proxy_history_<K>p.jsonl`.

Read [`docs/TRANSFER_TEST.md`](TRANSFER_TEST.md) and [`docs/PLAN_WAR_LOOKAHEAD.md`](PLAN_WAR_LOOKAHEAD.md) first. This
document is the monitor those two findings imply and nobody had built.

---

## 1. Why

The league accepts a champion when it beats its parent on a paired score
measured under the **training** architecture (`--candidate-bot`). That is a
proxy for what we care about: how the weight vector plays under the policy we
would ship, `plan:width=8`. The two have already come apart once, and nothing
in the loop noticed:

* [`docs/TRANSFER_TEST.md`](TRANSFER_TEST.md) — the quiescent-trained vector Q is **+36.3 ± 4.8**
  margin better than the 1-ply-trained vector P under the training proxy, and
  **−32.5 ± 6.9 worse** under `plan:width=8`, against a common opponent. Head
  to head under PlanBot it lost at a **2.5% ± 1.1%** win share. The proxy did
  not merely mis-state the size of an improvement; it got the **sign** wrong.
* [`docs/PLAN_WAR_LOOKAHEAD.md`](PLAN_WAR_LOOKAHEAD.md) — giving PlanBot a war lookahead removed the
  inversion (the same head to head is now **52.2% ± 3.7%**, **+1.4 ± 5.3**).
  It did **not** make the proxy predictive: the proxy still says +36.3 ± 4.8
  where the ship policy says a null. §6 of that document states it plainly —
  the proxy went from *actively wrong* to *uninformative about magnitude*.

Both of those are one-off measurements on frozen vectors, made by a human-driven
agent. Neither is a monitor. So an arm can climb for two days, accept a hundred
champions, and there is no artefact anywhere that answers "did any of that
reach the policy we would ship". This is that artefact.

It matters more, not less, after the 2026-07-29 retarget of the 2p arm
([`docs/TRAINING_RUN.md`](TRAINING_RUN.md)): that arm now trains under `plan:width=2`, which
shrinks the proxy gap from *quiescence vs PlanBot* to *a narrow beam vs a wide
one* — but shrinking a gap is not closing it, and `width=2`'s strength has
never been measured ([`docs/BOT_ARCHITECTURE.md`](BOT_ARCHITECTURE.md) has `width=1` at 62.3% and
`width=8` at 85.1% against 1-ply, and nothing in between).

**The guardrail's very first reading is also the evidence that this was a real
risk and not a hypothetical.** It measured the live 2p champion at **132.8**
own culture against `book` under `plan:width=8`, next to
[`docs/PLAN_WAR_LOOKAHEAD.md`](PLAN_WAR_LOOKAHEAD.md#4a-absolute-own-culture-not-just-margins) §4a's **127.8** for the frozen quiescent-trained
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
   written down — human 2p median **159.5** ([`docs/HUMAN_BASELINE.md`](HUMAN_BASELINE.md)), and
   under `plan:width=8` against `book`, **213.4** for the 1-ply lineage vector
   and **127.8** for the quiescent-trained one
   ([`docs/PLAN_WAR_LOOKAHEAD.md`](PLAN_WAR_LOOKAHEAD.md#4a-absolute-own-culture-not-just-margins) §4a).

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
| `INVERTED` | margin upper bound < **−5** | a real **loss**: the champion the proxy chose is worse under the ship policy than the one it replaced. [`docs/TRANSFER_TEST.md`](TRANSFER_TEST.md)'s failure, live |
| `flat` | half-width ≤ **15** and the CI covers the no-effect band | measured, and there is nothing there |
| `inconclusive` | half-width > **15** | **not measured.** Not reassurance, not a divergence — an instrument problem |

The statistic is the paired **culture margin**, not win share. Win share is a
0/1 step with ~10x the paired variance, it saturates against `book` at
0.94-0.97 under PlanBot ([`docs/TRANSFER_TEST.md`](TRANSFER_TEST.md#3-the-second-way-of-asking-both-vectors-against-a-common-opponent) §3), and every finding in
[`docs/TRANSFER_TEST.md`](TRANSFER_TEST.md) and [`docs/PLAN_WAR_LOOKAHEAD.md`](PLAN_WAR_LOOKAHEAD.md) is quoted in margin
for exactly that reason. Win share is still printed, as a secondary.

**`inconclusive` is the verdict that keeps this file honest, and the first
version did not have it.** That version's first real reading — 2p, gen 657 →
725, 5 accepts, 20 deals — came back at a win-share lower bound of **50.03%**
against a 50% null and printed `confirms`. A coin flip that landed right,
reported as reassurance: [`docs/HAZARDS.md`](HAZARDS.md) trap 1 (an n=48 row read 50.0%
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

If it fires, the options are the ones [`docs/TRANSFER_TEST.md`](TRANSFER_TEST.md) §8.3 already
enumerated, and they are *decisions*, not fixes:

1. **Retarget the arm's `--candidate-bot` closer to the ship policy**, as the
   2p arm was on 2026-07-29. Costs generations; `tools/arch_cost.py --players
   <K> --weights <the arm's champion>` is how you price it. Measure at the
   player count you are retargeting, on the champion — `DEFAULT_WEIGHTS`
   understates the search bots badly (2p quiescent: 0.732 cpu-s/game on the
   champion against 0.272 on the defaults).
2. **Fix the search asymmetry** that the proxy and the target disagree about,
   which is what [`docs/PLAN_WAR_LOOKAHEAD.md`](PLAN_WAR_LOOKAHEAD.md) did for wars. Cheapest when it
   exists, and it is a change to a bot rather than to the trainer.
3. **Fix the objective**, if the divergence is the metric overpaying for
   something the ship policy cannot cash — `margin_share` paying twice for a
   stolen culture point was exactly that, and `--objective blend` is the
   standing fix.

A `flat` run is *not* on its own evidence for any of the three. Check the
achieved CI first: at 20 deals a 2p reading resolves a ~15-point win-share
effect and no better, and [`docs/TRANSFER_TEST.md`](TRANSFER_TEST.md#7-limits--what-this-does-not-establish) §7 makes the same point about
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
against `book`, 15.83 in a mirror; [`docs/TRAINING_RUN.md`](TRAINING_RUN.md) has 4p at 17.4 and
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
  warning [`docs/TRANSFER_TEST.md`](TRANSFER_TEST.md#7-limits--what-this-does-not-establish) §7 gives. `flat` frequently means "not
  resolved", and the log prints the CI so you can see which.
* **`book` is one opponent, and the pool is a monoculture** — every pool
  opponent is a `BookBot` subclass ([`docs/TWOP_PROFILE.md`](TWOP_PROFILE.md#9-what-this-does-and-does-not-support) §9). The anchor is
  an anchor, not an absolute standard.
* **It validates the vector, not the search.** If `plan:width=8` itself is not
  the right thing to ship, this guardrail will happily confirm progress toward
  it.
* **It cannot separate "the proxy decoupled" from "the arm stopped
  improving".** A converged arm produces `flat` readings honestly. Cross-read
  it against the arm's own accept rate before concluding anything about the
  proxy.

---

## 8. The first `INVERTED` reading, replicated — 2026-07-29

The 3p arm's first-ever guardrail reading came back `INVERTED` at
`gen 821 -> 918`, and it was one reading. This section is the follow-up
measurement that was run to settle it. Raw per-game series and the driver
script are not committed (trainer-output convention); every number below is
reproducible from the commands in §8.11.

**Three findings, and they do not all point the same way — read all three.**

1. **The reading replicates.** At 2x the n the margin is −16.1 ± 9.9, upper
   bound −6.2, still `INVERTED` under this file's own rule (§8.3). It is not
   small-n noise.
2. **But the 821 → 918 range is NOT a monotone slide, and it is not one bad
   accept either.** Gen 850 is *better* than gen 821 by +27.2 ± 11.7 culture
   on the absolute anchor — a real, resolved gain the proxy correctly found —
   and the range then falls away from it (§8.4, §8.5). The resolved damage is
   in the last step, 900 → 918.
3. **The finding that dwarfs the alarm is the long series.** Own culture
   against a fixed opponent under the ship policy falls **−79.8 ± 21.5** from
   gen 0 to gen 930, paired on identical deals, at **−7.38 ± 1.71 culture per
   100 generations** (§8.5). The arm's best-scoring vector under the policy we
   would ship is the untrained one it started from.

The synthesis in §8.6 is that "the 3p proxy is inverted" is the wrong shape of
claim. The proxy is close to *uninformative* about the ship policy from one
accept to the next — gen 850's accepts were genuinely good and gen 918's were
genuinely bad — with a systematic downward bias on top. That is a random walk
with drift, not a sign flip, and it needs a different fix from the one an
inversion would need.

### 8.1 What the original reading actually was

The log line does not state its n. It is:

| | n | what the CI is over |
|---|---|---|
| head to head, `-20.3 +/- 11.9` | **60 games = 20 deals x 3 seats** | `arena.mean_ci` over the **60 games** |
| anchor, `own culture 141.0` | **24 games = 8 deals x 3 seats** | — |

Two things follow that are worth knowing before quoting it. First, the null it
is measured against is 0 culture margin, and the win-share null at 3p is
**33.3%**, not 50%. Second, `measure()` computes the margin CI over *games*,
but a deal at 3p is one seed played from all three seats **against the same
opponent vector**, which makes a deal an internally paired unit: the
challenger's gain in one seat is partly the defender's loss in another. So the
games inside a deal are **negatively** correlated, and the deal-clustered CI
comes out *tighter* than the game-level one, not wider (§8.3). The guardrail's
CI is therefore conservative here. That is the safe direction, and it is the
opposite of what clustering usually does — do not assume it holds at 2p or
against `book`, where the pairing is not symmetric.

### 8.2 Harness check: the same deals give the same answer

The replication uses `proxy_check.py`'s own `H2H_SEED = 5150` and
`ANCHOR_SEED = 90210`, so its first 20 deals **are** the reading's 20 deals.
On those deals it returns, to the digit:

```
margin -20.3 +/- 11.9   own culture 65.5 vs 85.9   win share 20.6% +/- 10.2%
```

That matters for a second reason: the reading was taken with the pre-`7ef6ac8`
PlanBot and the replication with the post-`7ef6ac8` one (`QuiescentBot and
PlanBot search by undo stack, nested`). That commit claims byte-identical gate
fingerprints; this is an independent confirmation of the claim on 60 live 3p
games, and it means §8.3-8.5 are not measuring an engine change.

### 8.3 The replication: gen 918 vs gen 821 at 2x the n

Both vectors under `plan:width=8`, seat-rotated on the same deals, zero engine
errors. `blend` was the Python league's own accept objective (the now-deleted
`docs/LEAGUE_OBJECTIVE.md`, git history: `own_share` centred 100, scale 120,
alpha 0.15 on win share — since replaced by the Rust `climb` binary's plain
win-share-plus-anchor-veto design), scored per game for the challenger minus
the defenders' mean and clustered by deal.

| | n | culture margin | +/- (game) | +/- (deal) | own culture | opp culture | win share (null 33.3%) | d(blend) |
|---|---|---|---|---|---|---|---|---|
| guardrail's reading | 60 / 20 deals | −20.3 | 11.9 | 10.1 | 65.5 | 85.9 | 20.6% ± 10.2% | −0.093 ± 0.050 |
| **this replication** | **120 / 40 deals** | **−16.1** | **9.9** | **8.5** | **70.9** | **86.9** | **24.4% ± 7.7%** | **−0.069 ± 0.039** |

**The verdict is unchanged: `INVERTED`.** The margin's upper bound is **−6.2**
game-clustered and **−7.6** deal-clustered, both below the file's own −5
threshold. Own culture and win share move the *same* way — 70.9 against 86.9
and 24.4% against a 33.3% null — so this is not a margin artefact dressed up as
a regression, which is the specific failure the now-deleted
`docs/LEAGUE_OBJECTIVE.md` §1 existed to prevent. The league's own `blend`
objective agrees at −0.069 ± 0.039.

### 8.4 Bracketing it: not one bad accept

The same head to head from three intermediate rungs of `ladder_3p`, every one
of them against the **same** gen 821 on the **same** deals:

| challenger | n | culture margin | +/- (deal) | own culture | win share |
|---|---|---|---|---|---|
| gen 876 | 60 / 20 deals | −4.4 | 8.8 | 84.4 | 30.0% ± 11.7% |
| gen 900 | 60 / 20 deals | −4.5 | 13.6 | 83.2 | 23.3% ± 10.8% |
| gen 918 | 120 / 40 deals | **−16.1** | 8.5 | 70.9 | 24.4% ± 7.7% |

Every rung is negative and none of the intermediates resolves on its own at
n=60. Read alone this table says "a slow drift with the resolved damage in the
tail", and that is as far as a chain of head-to-heads can take you — §7's first
limit, that a chain is relative and drifts, applies to this table too.

### 8.5 The absolute anchor settles it, and it is worse than the chain suggests

Own culture against `book` under `plan:width=8`, **every rung on byte-identical
deals**, so the columns are paired game by game. `n = 48 games / 16 deals` per
row; the CI on the paired difference is clustered by deal.

| gen | own culture | +/- | **paired d vs gen 0** | +/- | `book`'s culture | win share |
|---|---|---|---|---|---|---|
| **0** (untrained `DEFAULT_WEIGHTS`) | **217.8** | 14.5 | — | — | 130.2 | 85.4% ± 10.1% |
| 309 | 191.7 | 15.7 | −26.1 | 23.4 | 87.7 | 91.7% ± 7.9% |
| 603 | 174.1 | 13.2 | −43.7 | 21.0 | 85.4 | 85.4% ± 10.1% |
| 711 | 171.4 | 19.2 | −46.3 | 25.1 | 93.6 | 70.8% ± 13.0% |
| 821 | 152.2 | 13.0 | −65.6 | 20.1 | 87.8 | 79.2% ± 11.6% |
| **918** | **141.1** | 10.9 | **−76.6** | **17.8** | 88.9 | 72.9% ± 12.7% |

OLS on the 96 deal means: **−7.83 ± 1.88 culture per 100 generations**, i.e.
4.2 SE from a flat line, monotone from the first rung to the last.

**The 3p arm's best vector under the policy we would ship is the untrained one
it started from.** 918 generations of proxy progress — 151 accepted champions,
every accept requiring a positive one-sided lower bound on the training metric
— bought **−76.6 ± 17.8** culture against a fixed external opponent. The
`book` column is the mechanism and it is [`docs/TRANSFER_TEST.md`](TRANSFER_TEST.md#5-why-the-two-vectors-are-different-animals) §5 verbatim:
the arm did not raise its own score, it learned to hold `book` down (130.2 →
88), which `margin`-era intuitions read as progress and `own`/`blend` under
the *ship* policy does not.

The weight vectors say the same thing in the open. Across 821 → 930 the
movement is monotone and it is military: `strength_lead` 0.530 → 3.211,
`strength` 4.76 → 7.53, `pact_blocks_attack` 0.239 → 1.216, while
`culture_rate` peaks at 12.3 (gen 850) and falls back to 9.1. That is precisely
the move class [`docs/TRANSFER_TEST.md`](TRANSFER_TEST.md#6-answering-the-question-docstraining_runmd-asked) §6 identifies as the one the proxy
prices and the ship policy does not.

**So the answer to "a bad accept, or an inverted proxy" is the second one, and
for the whole run rather than the last 97 generations.** The 2026-07-29
retarget of the 2p arm ([`docs/TRAINING_RUN.md`](TRAINING_RUN.md)) was made on exactly this
evidence shape at 2p; 3p is the same finding with a bigger effect and a longer
lever arm.

### 8.6 3p has never had a passing transfer check. Not once.

This was checked directly rather than assumed:

* `experiments/league_state/proxy_history_3p.jsonl` has **one** record — the
  `INVERTED` one. `grep '\[3p\]' experiments/logs/proxy_check.log` returns one
  block. It is the first and only ship-policy reading the arm has ever had.
* The in-loop `FULL POOL CHECK` is **not** a transfer check.
  `hillclimb_league.full_check` calls `as_spec`, which applies the module-level
  `CANDIDATE_ARCH` — i.e. `quiescent:levels=1`. All 113 of the 3p arm's full
  checks measure the proxy against itself.
* `grep -niE "plan|width=8|transfer" experiments/logs/league_3p.log` returns
  **nothing**. The arm has never played a `plan` game in its life.
* Nothing in `docs/` carries a 3p `plan:width=8` number.
  [`docs/TRANSFER_TEST.md`](TRANSFER_TEST.md#7-limits--what-this-does-not-establish) §7 and [`docs/PLAN_WAR_LOOKAHEAD.md`](PLAN_WAR_LOOKAHEAD.md) both say in
  terms that they are 2p only and that 3p/4p were deliberately not attempted.

**The 3p arm ran 1 131 generations and 151 accepted champions with no transfer
check of any kind**, and the first one ever taken says the direction was wrong
the whole way. That is the blind spot this whole document was built to close,
found on its first look at this arm.

### 8.7 Against the human baseline — and why it is not head to head

[`docs/HUMAN_BASELINE.md`](HUMAN_BASELINE.md): the human **3p** median final culture is **180**
[140-211] over n=133 games. (The 159.5 quoted throughout this file is the **2p**
figure and should not be used for a 3p reading.)

Two of our numbers sit either side of it and **neither is comparable to it**:

* **141.1 against `book`** — one weak opponent, seated twice. `book` is a
  hand-written `BookBot` and the pool is a monoculture (§7). This flatters us.
* **70.9 in the gen-918-vs-gen-821 mirror** — three strong searchers splitting
  one table, which is *structurally* the shape the human 180 comes from (three
  humans at a table). This is the more honest comparison of the two and it is
  brutal: 71-95 against 180.

Neither is a head-to-head result against humans, and this repo has no way to
produce one. The human corpus is games humans played against each other, in
which our vector never sat. **Suggestive, not equivalent.** What can be said
without stretching: gen 0 at 217.8 against `book` and gen 918 at 141.1 are on
opposite sides of the human 3p median, and the arm moved from the first to the
second.

### 8.8 What it costs to act

`tools/arch_cost.py --players 3 --weights experiments/league_state/ladder_3p/
gen00930.json`, i.e. measured **on the arm's own champion**, not on
`DEFAULT_WEIGHTS` ([`docs/TRAINING_RUN.md`](TRAINING_RUN.md) warns the difference is large, and it
is: quiescent costs 1.504 cpu-s/game in a mirror here against 0.357 on the
defaults). cpu-seconds per game, `TTA_JOURNAL=1`, `workers=1`:

| architecture | vs `book` | mirror | "real mix" (3/4 mirror) | x quiescent | 3p generations in 12h |
|---|---|---|---|---|---|
| `weighted` (1-ply) | 0.403 | 0.927 | 0.796 | 0.6x | ~270 |
| `quiescent:levels=1` (today) | 0.591 | 1.504 | 1.276 | 1.0x | **~168** |
| `plan:width=1` | 1.608 | 5.419 | 4.466 | 3.5x | ~48 |
| **`plan:width=2`** | **2.880** | **7.836** | **6.597** | **5.2x** | **~32** |
| `plan:width=4` | — | — | — | — | — |
| `plan:width=8` (ship) | 8.056 | 30.374 | 24.795 | 19.4x | ~9 |

So retargeting 3p to `plan:width=2` — the change made to 2p on 2026-07-29 —
costs about **5.2x**, taking the arm from ~168 generations per 12h to ~32. At
the arm's observed ~13% accept rate that is ~4 accepts per 12h against ~22.

### 8.9 Limits — read before quoting §8.5

* **`book` is one opponent and the pool is a monoculture.** §7's warning is
  unchanged. The §8.5 series is a *paired* difference on a common opponent,
  which is robust to that opponent being weak, but it is not evidence about a
  human or the official app AI. It is possible in principle for a vector to
  lose ground against `book` and gain it against something else; nothing here
  rules that out, and the only cheap way to check would be to repeat §8.5
  against `hall:oneply_2p_gen00355`, which was not done.
* **`plan:width=8` is one point on a curve, and it is the *assumed* ship
  policy.** §7's "it validates the vector, not the search" applies in full. If
  `width=8` is not what we ship at 3p, §8.5 is measuring the wrong target.
* **n is 48 games / 16 deals per anchor row.** The row-level CIs are ±11 to
  ±19 culture and the *paired* differences are the load-bearing column. The
  gen-930 row was measured at 8 deals only and is omitted from the OLS.
* **The h2h rungs in §8.4 do not individually resolve.** Only gen 918 does.
  The claim "a steady decline, not one bad accept" rests on §8.5, not on §8.4.
* **Six rungs is a coarse grid over 918 generations.** The decline is monotone
  on the rungs measured; it is not established that it is monotone between
  them, and a single catastrophic accept somewhere inside a gap would look the
  same at this resolution.
* **This does not say `quiescent:levels=1` is a bad search**, or that gen 918
  is a bad vector. Under its own proxy the arm's full pool check has it winning
  73-96% and scoring 147-190 own culture. [`docs/TRANSFER_TEST.md`](TRANSFER_TEST.md) §8.4's
  sentence applies unchanged: it is the better vector under everything except
  the thing we would ship.

### 8.10 Found on the way: both arms silently played zero games for ~55 minutes

Unrelated to the proxy, and worse operationally. Reconstructed from
`generations_3p.jsonl` / `generations_4p.jsonl` (`per_opponent[*].n == 0` on
every candidate):

| arm | dead from | to | generations burnt |
|---|---|---|---|
| 3p | gen 933, 11:33:00 | gen 1130, 12:29:32 | **197** |
| 4p | gen 235, 11:33:36 | gen 303, 12:16:03 | **68** |

Every game raised, so every candidate scored `edge 0.0, lo -1.0`, so nothing
could be accepted. The window opens **27 seconds** after a
`pull -q --ff-only origin master` at 11:32:33 (git reflog) that landed the
undo-stack commits, and closes at each arm's next hourly process restart. The
mechanism was not confirmed and should not be quoted as fact; the coincidence
is exact.

**Nothing anywhere said so**, and that is the part to fix:

* `arena._play` catches every exception and returns `share=None`
  ("engine bug: report, do not kill the tournament"), which is right;
* but `hillclimb_league` records the result as `n: 0` with **no error text**,
  and `league_3p.log` contains zero occurrences of `error`;
* `run_league.sh`'s 60-second backoff triggers on "the engine won't
  **import**", which a runtime failure is not;
* and the proxy guardrail cannot see it either — it reads ladder files, and a
  dead arm simply stops writing them, which is indistinguishable from a
  converged one.

[`docs/HAZARDS.md`](HAZARDS.md)'s standing warning is that silence from a monitor reads as
good news. Here the arm's *own* log was the silent monitor. A one-line
"n=0 on every opponent, `arena` reported N errors, first: `<repr>`" would have
caught it in one generation. **This also means "3p has accepted nothing since
gen 930" is the outage, not convergence** — it accepted again at gen 1132 as
soon as it recovered.

### 8.11 Reproducing

```
# the guardrail's own reading, forced
nice -n 19 python3 -m experiments.proxy_check --players 3 --force

# the replication and the bracket: arena.duel on ladder files under
# plan:width=8, H2H_SEED=5150 / ANCHOR_SEED=90210 as proxy_check.py uses,
# via experiments.hillclimb_league.parse_candidate_bot + as_spec
#   h2h    : duel(spec(X), spec(821), 3, deals*3, seed0=5150)
#   anchor : duel(spec(X), "book",    3, deals*3, seed0=90210)
# for X in 850 876 900 918 930 and, for the anchor, also 0 309 603 711.

# the cost table
python3 tools/arch_cost.py --players 3 --games 6 --plan-games 3 --cores 2 \
    --weights experiments/league_state/ladder_3p/gen00930.json \
    --arches "weighted,quiescent:levels=1,plan:width=1,plan:width=2,plan:width=8"
```

Run at normal priority against five `nice -n 19` league workers on a 6-core
box; ~20 000 cpu-seconds total, zero engine errors in every duel reported
above. The live arms were not stopped, reconfigured or touched, and no engine
code was changed.
