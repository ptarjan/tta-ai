# What the league maximises: the lead over the best opponent

2026-07-30. Replaces the 2026-07-27 own-culture objective documented in this
file's previous revision (git history at `8b972ef`). Base game (2015), all
three player counts. Changed: `experiments/hillclimb_pool.py`,
`experiments/hillclimb_league.py`, `experiments/arena.py` (one additive output
key), `experiments/watchdog.sh` (a comment), `tools/objective_ab.py`,
`tools/level_sweep.py` (a docstring), `tests/test_league_objective.py` (29
tests), new `tools/objective_relog.py`. Nothing in `engine/` is touched and all
eight gate digests are unmoved.

## 0. One paragraph

The league scored a candidate on its **absolute own final culture**, squashed
through a tanh centred on `CULTURE_CENTRE = 100` — a fitted guess at what a
typical game scores. That number was fitted in July 2026 and was measurably
stale within the same month, and it would have gone stale again on every
improvement. It is now scored on the **culture lead over the best opponent**,
squashed through a tanh centred on **zero**, because zero lead is exactly the
win/lose boundary and that is a fact about Through the Ages rather than a number
anyone has to fit. **Re-centring would have replaced a stale guess with a fresh
guess. Switching to a differential deletes the parameter.** Re-scoring 3,802
archived candidate evaluations offline, the new objective's ranking correlates
with win rate at **+0.934 / +0.934 / +0.904** (2p/3p/4p) against the old
objective's **+0.850 / +0.861 / +0.824**. It does **not** measurably improve
accept/reject throughput (§6b is a null), and it carries a real risk that §7
quantifies and does not soften.

## 1. What each number in the objective is

The table the rest of this document exists to justify.

| quantity | status | value | where it comes from |
|---|---|---|---|
| the centre of the culture curve | **rule-derived** | **0** | `lead >= 0` iff you won or tied. `arena._play` computes the win share from `max(sc)` and the lead from `max(others)` — the same maximum over the same list, three lines apart. Pinned against the engine by `LeadShare::test_zero_lead_is_exactly_the_win_boundary`. |
| which opponent the lead is against | **rule-derived** | the **best** one | "most culture at the table" is a statement about the maximum, not the mean. §4. |
| `LEAD_SCALE` | **choice**, informed by measurement | 120.0 | How much a blowout counts relative to a close game. No rule decides it. Set by the rule "≈2.5× the measured per-game dispersion", from `experiments/margin_calib.py` (sd ≈ 50 at 3p, ≈ 45 at 4p). §5. |
| `DEFAULT_ALPHA` | **choice**, unchanged | 0.15 | Weight on the win-share tiebreak. §3. |
| ~~`CULTURE_CENTRE`~~ | **deleted** | — | Was 100.0, fitted to observed scores. This is the whole point. §2. |
| ~~`CULTURE_SCALE`~~ | **deleted** | — | Only existed to scale a quantity that no longer exists. |

Two free parameters, one of them a tiebreak weight. `ScoreParams.__slots__` is
`("lead_scale", "alpha")` and a test asserts it, so a third cannot appear
without someone noticing.

```
lead          = own final culture - the BEST opponent's final culture
lead_share(m) = 0.5 * (1 + tanh(m / LEAD_SCALE))            LEAD_SCALE = 120
score         = (1 - alpha) * lead_share + alpha * win_share    alpha = 0.15
```

## 2. Why the old centre had to go, and why re-fitting it was the wrong fix

`own_share(c) = 0.5·(1 + tanh((c − 100) / 120))`. The code's own stated
justification was that the operating band is "65 to 160" and that centring at
100 keeps the marginal value of a culture point flat across it.

**The band was wrong.** Over 3,802 blend-era candidate evaluations
(`tools/objective_relog.py`, and the read-only analysis it grew out of):

| | 2p | 3p | 4p |
|---|---|---|---|
| candidate own-culture median | 108.8 | 122.1 | 134.4 |
| champion own-culture median | 120.8 | 144.1 | 160.6 |
| human corpus median (`docs/HUMAN_BASELINE.md`) | ~156-159 | ~176-180 | ~182-195 |

The centre sat below where games were actually being decided at every player
count, and further below at 3p/4p than at 2p. The marginal-value argument still
roughly held — the band is wide and tanh is forgiving — so this was a drift,
not a catastrophe.

**But the fix is not a new number.** A constant fitted to the policy's observed
scores steers the next policy, whose scores then move, and the constant is
stale again. That is the same failure mode as the fitted table in
`docs/UNIT_TECH_PRICING.md`: a number that cannot say "it depends". Re-fitting
buys one month.

A differential does not need the parameter at all. The distinguished point of
`lead_share` is 0, 0 is where the game is won or lost, and no measurement can
move it. **The parameter is not re-tuned; it is deleted, and a regression test
(`NoFittedCentre`) asserts it stays deleted** — structurally, via the identity
`score(m) + score(−m) == 1`, which any non-zero centre breaks whatever it is
named or however it is spelled.

## 3. Decision: keep the alpha win-share term, at 0.15

**Kept.** The reasoning, since the term is now arguably redundant:

Under the old objective alpha was the only part of the score that knew the
opponents existed at all — own culture is blind to a candidate that scores 150
while letting the table score 200. That job is now done by the main term, so the
question "delete it?" is fair.

It survives because it has a *different* job. **The tanh deliberately blurs the
win/lose step in order to have a gradient**: under `lead_share`, losing by 1
scores 0.4958 and winning by 1 scores 0.5042 — a difference of 0.008, when the
real difference in payoff is the entire game. The squash is what buys density
and outlier control (§5), and it pays for that by flattening the one
discontinuity that genuinely exists. A small win-share term puts a fraction of
it back. That is a real thing for it to do, not a leftover.

**Why small, and why exactly 0.15.** Per-game win share is a 0/1 step; paired
against a reference on the same seeds it is 0 when the arms agree and ±1 when
they disagree, so its paired sd is several times a culture term's (measured
0.500 for win share against 0.419 for a culture margin, on the 1,632 shared
games of the previous revision's §3). A large alpha buys variance, not
alignment: it widens the accept CI and the climb stalls. As for 0.15
specifically — **it is inherited unchanged and deliberately so.** There is no
evidence for a different value, and picking a fresh number without evidence is
exactly how the constant this change deletes got there. The offline sweep says
it is a tiebreak either way: alpha 0 → 0.15 flips 5-9% of decisions and 0.15 →
1.0 flips 16-19%. `--objective-alpha 0` is pure lead, `1` is pure win share, one
flag and no code edit.

**What would change this call:** if the post-relaunch arms stall, alpha is the
first dial to suspect, and dropping it to 0 is a much safer move than it used to
be — the main term is now the win condition, so alpha=0 is no longer "train on
something that is not the objective".

## 4. Decision: the lead is over the BEST opponent, not the mean

At 2p there is nothing to decide: one defender, so the two are the same number.
At 3p/4p:

**Best opponent — and the trade-off runs the opposite way to the obvious one.**
The expected framing is that margin-over-mean is the smoother, more trainable
signal and margin-over-best is correct-but-coarse, so picking best is a
trainability concession. That is not what the two quantities do.

Margin over the mean has a specific pathology: **it pays for beating up a player
who is not contending.** Leader on 180, us on 150, trailing seat on 60 — our
margin over the mean is +30 while we are losing by 30, and we can raise it
further by grinding the trailing seat down to 20. That move does nothing
whatever for winning. Margin over the best is exactly flat in it, because
`max(others)` does not notice.
`ScoreSeries::test_beating_up_a_non_contender_does_not_score` is that scenario
as an assertion. So the smoother signal is also the one with the kingmaker
pathology, and margin-over-best is both the win condition and the better-behaved
reward. This is not a case where correctness and trainability trade off.

**What choosing the max does cost, honestly.** A maximum over noisy quantities
is noisier than a mean over them, so the per-game dispersion of the lead at
3p/4p is higher than that of the margin, which widens the accept CI and slows
the climb. **That cost is real and is not measured** — measuring it needs
per-game series under the new objective, which only exist after relaunch. It is
on the watch list in §7b and in §9.

**Both columns are kept.** `arena.duel` has always reported `per_game_margin`
(over the mean) and still does, as a diagnostic; the new `per_game_lead` key is
over the best. The two side by side are how an operator reads "am I winning by
out-scoring the leader or by flattening the table".

## 5. `LEAD_SCALE` is the one genuine choice, and the code says so

A scale is not a factual claim. It answers "how much should a blowout count
relative to a close game", which nobody can read off the rules. The comment in
`hillclimb_pool.py` says exactly that, in those terms.

It is nevertheless **derived from measured dispersion rather than picked**. The
rule: `scale ≈ 2.5 × the per-game sd of the lead`, which keeps the observed
operating band inside tanh's near-linear core while still bounding the tail.
`experiments/margin_calib.py`'s last run measured a per-game sd of ≈50 at 3p and
≈45 at 4p, and 2.5 × 48 = 120.

Why the rule and not a smaller number: too small and the operating region sits
in the flat tail — at scale 45 a 4p lead of −120 maps to −0.996, where a
15-point improvement moves the score by 0.0004 and the gradient is dead again,
just more quietly than before. Too large and it degenerates toward linear, where
one blowout carries an accept. **Bounding one game's influence is the job the
squash is really doing and it is the thing that must not be traded away**;
`test_bounded_on_adversarial_inputs` and
`test_blend_is_bounded_on_adversarial_series` hold the score in [0,1] against
±1e300 and ±inf.

Two honesty notes on the 120:

1. The sd it is derived from was measured on margin-over-**mean**.
   Margin-over-best is at least as dispersed at 3p/4p, so if anything 120 errs
   toward saturation there. **Re-derive with `experiments/margin_calib.py` from
   the first post-relaunch logs.**
2. Unlike a fitted centre, it does not drift stale in a dangerous direction: as
   the bot improves its leads move **toward zero**, i.e. toward the most linear
   part of the curve. That is the second thing centring on the win boundary
   buys.

## 6. Does it rank the decisions we actually faced better? Offline, n=3,802

`tools/objective_relog.py` re-scores every archived blend-era candidate
evaluation under both objectives. No games: this is arithmetic over
`generations_*.jsonl`, which is why it is cheap enough to be the primary
evidence.

**Read the caveats first.** At **2p the recomputation is exact in the
quantity** — one defender means the logged `margin` column *is* the lead. At
**3p/4p it is a proxy**: the archives never recorded the best opponent's
culture, so those rows substitute margin-over-mean. That tests the main change
(a differential instead of an absolute own score) but **not** the best-vs-mean
refinement of §4, which is unmeasurable from these logs. Both objectives are
aggregated by averaging per-opponent means before the squash (`tanh(mean) ≠
mean(tanh)`); the recomputed old edge reproduces the logged edge at Spearman
0.995-0.998, which is what licenses the comparison. The accept bound `lo` needs
per-game variance the logs do not contain, so the accept-rule rows use an
opponent-level proxy `lo_hat` applied **identically to both objectives** — it
correlates with the trainer's real `lo` at Spearman 0.97, but its absolute level
is not the trainer's.

### 6a. Ranking against win rate — a clear, consistent improvement

Spearman against the candidate's win-rate difference from the champion:

| | n | OLD (logged) | OLD (recomputed) | **NEW** | delta |
|---|---|---|---|---|---|
| 2p (exact) | 1116 | +0.850 | +0.843 | **+0.934** | **+0.091** |
| 3p (proxy) | 1621 | +0.861 | +0.854 | **+0.934** | **+0.080** |
| 4p (proxy) | 533 | +0.824 | +0.816 | **+0.904** | **+0.088** |

The "OLD (logged)" column reproduces the pre-existing read-only analysis
exactly, which is the check that the harness is reading the right thing.
**+0.08 to +0.09 at every player count** — the expected direction, and a larger
effect than expected.

Edge-sign decisions that flip versus the old objective: **15.9% (2p), 11.7%
(3p), 10.3% (4p)**.

### 6b. Accept/reject throughput — essentially unchanged. This is a null.

The old objective's conservative bias: it almost never accepted a
worse-on-winning candidate, but rejected a better-on-winning candidate 16-23% of
the time. The hope was that a better-aligned objective would cut the rejection
rate without raising false accepts. It does not, materially:

| | accepted but WORSE on winning | | rejected but BETTER on winning | |
|---|---|---|---|---|
| | OLD | NEW | OLD | NEW |
| 2p | 0.7% | **0.2%** | 20.8% | **20.1%** |
| 3p | 0.7% | 0.9% | 16.4% | **15.7%** |
| 4p | 0.4% | 0.4% | 18.7% | 19.3% |

Two of three player counts improve by well under a percentage point and 4p gets
slightly worse. **Reported as a null.** The plain reading is that the
conservatism is set by the confidence bound and the block size, not by which
quantity the objective scores — so the throughput lever is `--block` and the
accept `z`, not the objective. Anyone chasing throughput should look there. The
correlation improvement in §6a is the real result of this change.

## 7. THE RISK: this objective pays for dragging opponents down

It does. The honest framing matters, and it is not the one the previous revision
of this document used.

**The double-pay is arithmetically real and is correct with respect to
winning.** War and aggression *move* culture. Taking 20 from the leader moves
the lead by 40; producing 20 moves it by 20. The previous revision called that
factor of two a bug — "you win by having the most culture, not by having the
biggest gap". **That sentence is false as stated**: having the most culture *is*
having a non-negative lead over the best opponent. They are the same statement.
Taking 20 off the leader really does close twice as much of the gap as making
20, and an objective that priced it at once would be misdescribing the game.

**What the previous revision was right about.** The margin-trained 2p champion
scored 64.7 own culture against a human 159.5 and won by suppression;
`docs/TWOP_PROFILE.md` measured 69% of its margin in the conflict move class,
causally, by banning the move class rather than by accounting. Switching to own
culture demonstrably fixed *that*. But that document's own §3 records the
finding which undermines its diagnosis: **pure win share preferred the theft
champion too, more strongly than margin did** (−0.1967 vs −0.1795). The literal
objective kept it. So the degeneracy was not caused by margin mispricing
*winning*; the theft champion genuinely won more under the training proxy, and
the reason to prefer the producing vector lies in `docs/PLAN_WAR_LOOKAHEAD.md`
§3-4a — under the policy we would actually **ship** the two vectors are a
head-to-head null while their own cultures are 213.4 and 127.8. The theft
champion's edge is a property of `QuiescentBot.WAR_LOOKAHEAD`, i.e. of the
*evaluator*.

Own culture was therefore **a thumb on the scale compensating for a suspected
mispricing somewhere else.** That is what "proper always" rules out. If the
correct objective rewards something the bot overdoes, the fix belongs in the
evaluator's war/defence pricing, not in a deliberately wrong training target.
**This change removes the compensation and leaves the underlying problem
exposed, on purpose.**

I am not certain the war pricing is the mechanism. What is documented is the
correlation (`WAR_LOOKAHEAD` is what gives the theft vector its head-to-head
edge, per PLAN_WAR_LOOKAHEAD §3-4a) plus the 6.6-7.9× declaration rate. The
causal claim belongs to whoever opens that lane.

### 7a. How big is the exposure? Offline estimate, and it is not small

The bot already declares wars at **6.6-7.9× the human rate at 3p/4p**
(`docs/OPEN_ITEMS.md`, `docs/AGGRESSION_RATE.md`). So: of the archived rows
where a candidate gained culture differential over its parent, where did the
gain come from — own culture up, or the opponents' down?

| | out-produced (own up, opponents not down) | **pure suppression (own flat/down, opponents down)** | mixed (both) |
|---|---|---|---|
| 2p (n=1423 rows) | 25.8% | **32.5%** | 41.7% |
| 3p (n=1775) | 35.3% | **25.9%** | 38.8% |
| 4p (n=533) | 36.6% | **29.3%** | 34.1% |

**Roughly a quarter to a third of differential gains are pure suppression** —
the candidate's own culture flat or falling while its lead rose. Under the old
objective those rows scored ≤ 0 on the culture term and were not rewarded.
**Under the new objective they are rewarded in full.** That is the size of the
behaviour change, stated as loudly as it deserves.

Three things that stop this being over-read, and they matter:

* **It is measured on candidates produced under own-culture training**, not
  under the new objective. It describes the population of mutations the search
  generates, not the policy margin training would converge to.
* **Conditioning on a differential gain mechanically inflates it.** Noise
  control: opponents' culture fell on **39-44% of *all* rows unconditionally**.
  Selecting rows where the lead rose necessarily over-selects rows where the
  opponent fell. The "pure suppression" cell is the informative one because it
  additionally requires own culture *not* to have risen — but part of even that
  is regression to the mean on 12-24-game blocks.
* **Suppression is a legitimate way to win Through the Ages.** The problem is
  not that the bot does it; it is that it does it 6.6-7.9× more than humans,
  which is evidence about the evaluator, not about the objective.

### 7b. What to watch in the post-relaunch logs, and the number that means trouble

Per the owner's instruction this was **not** re-measured with fresh game
batches: the correct objective lands either way, and post-relaunch logs measure
it under real training, which is the condition that actually matters. The
pre-registered checks, for whoever reads those logs:

1. **War/aggression declarations per seat-game**, via
   `tools/aggression_census.py`, against the human corpus rate recorded in
   `docs/AGGRESSION_RATE.md`. **It is already at 6.6-7.9× human at 3p/4p. If it
   rises above that, this objective is feeding the pathology, and the lane to
   open is the evaluator's war pricing — not this objective.**
2. **The `cult` column against the `lead` column** in the per-opponent report.
   Both are printed on every row precisely for this. Own culture flat or falling
   while the lead rises, sustained over generations, is suppression training.
   Own culture rising with it is production.
3. **`cult` against the human baseline** (`docs/HUMAN_BASELINE.md`: 2p median
   ~159.5). The previous objective existed because this number read 64.7. If it
   heads back down, say so early and loudly.

## 8. Consequences that would have been silent bugs

**The mirror reference is not analytic and must not become one.** A champion at
a table of itself has a mean *margin* of exactly 0 by symmetry, and the tempting
move is to reuse that shortcut for the lead. It is false: over a seat rotation
the leads sum to `sum(sc) − sum(max over the others)`, strictly negative unless
every seat ties — scores 10/5/3 give leads +5, −5, −7. So
`ANALYTIC_MIRROR_METRICS` is `("winshare",)` and the mirror reference is played
like any other opponent. That is not a new cost: the own-culture objective
already had to play it. `MirrorReference` pins both the list and the arithmetic.

**One metric for the whole pool; the per-tier override is gone.** The legacy
`--objective margin` mode scored the gate tiers on culture margin and everything
else on win share. A weighted mean over rows measured in different units is not
a number, and the tier weights that are supposed to apportion the decision stop
meaning anything. `DEFAULT_MARGIN_TIERS` and `Pool.margin_tiers` are deleted;
`PoolMetric::test_one_metric_for_every_tier` holds the line.

**`--objective own` and `--objective margin` are removed, not deprecated.** The
arms relaunch from clean state so there is no compatibility constraint, and a
dead mode misleads the next reader. Historical reproduction is `git checkout
8b972ef`. `tools/objective_relog.py` restates `own_share` once, clearly
labelled, purely to measure against it.

**`--objective winshare` is kept.** It has no fitted constant, it is the literal
objective, and it is useful to score against on demand. It remains a poor
gradient for the documented reasons: flat 0.0 against opponents the champion
never beats, saturated at 0.94-0.97 against `book`, and 2.8× noisier per game at
4p than at 2p.

## 9. What this does NOT establish — read before quoting anything above

* **Nothing here shows the new objective TRAINS a better bot.** It shows it
  *ranks* the 3,802 decisions the arms actually faced in better agreement with
  winning, and that the machinery runs. Whether hill climbing on it produces a
  stronger policy is unmeasured and stays unmeasured until the arms have run.
* **The 3p/4p re-scoring in §6 is a proxy** (margin-over-mean standing in for
  lead-over-best). The 2p column is exact. The §4 best-vs-mean decision is
  argued and unit-tested, **not** measured on logs — no log contains the
  quantity.
* **§6b is a null and is reported as one.** The conservative-rejection rate does
  not improve materially, and 4p gets slightly worse.
* **The per-game dispersion of the lead is not measured.** The variance argument
  in §3 carries over numbers measured for `own` and `margin` under a different
  objective. The lead is *expected* to be noisier per game than own culture was
  — it is a differential of two noisy quantities, measured 0.419 for a margin
  against 0.218 for own culture — which would widen the accept CI and slow the
  climb. **If the arms accept noticeably less often after relaunch, check this
  first; `--block` is the dial.**
* **§7a is an estimate on the wrong population** and is partly a selection
  artefact, as its own noise control shows. It is sized to be alarming rather
  than precise.
* **`LEAD_SCALE = 120` is derived from a dispersion measured on the *mean*
  margin** and has not been re-derived for the lead.
* **The claim that the 2026-07-27 diagnosis was incomplete is an argument from
  that document's own §3**, not a fresh experiment. §7 states it with the
  evidence attached so a reader can disagree.
* **No games were played for this change** beyond the unit tests' handful. That
  was the owner's explicit instruction: an A/B could not have decided anything,
  because the correct model lands either way.

## 10. The pool (unchanged by this change, preserved from the previous revision)

The 2026-07-27 rebalance is untouched here and is documented in
`docs/LEAGUE_POOL.md`. Retained because the tier totals are still live:

| tier | total | members (2p) | each | share |
|---|---|---|---|---|
| `book` | 0.6 | `book`, `book2` | 0.30 | 12% |
| `human` | 0.6 | four `hum:*` corpus-fitted archetypes | 0.15 | 12% |
| `variant` | 0.6 | six `var:*` | 0.10 | 12% |
| `mirror` | 1.0 | `mirror` | 1.00 | 18% |
| `past` | 1.2 | 2 `past:*` | 0.60 | 21% |
| `hall` | 1.6 | 3-4 `hall:*` | 0.40-0.53 | 29% |
| `floor` | **0.0** | — dropped — | — | 0% |

External/fixed 32%, self-play 68% — the inversion of the 69% static the pool
started at. Four load-bearing properties, all still in force:

1. **`hall` is its own tier**, so adding a frozen champion does not dilute the
   anti-cycling ladder.
2. **The static tiers keep the VETO.** `DEFAULT_GATE_TIERS` is `book, variant,
   quiescent, human`. Their job is stopping the climber walking off a cliff, and
   a self-play tier cannot do it.
3. **`acceptance_subset` guarantees mirror + one gate + one ladder opponent
   every generation**, which caps mirror's worst-case share of a generation's
   accept weight at 58% (test holds the line at 62%). Without it mirror alone
   decides 77% of some generations — the mirror-only loop the module replaced.
4. **The three saturated dummies are dropped** (`floor=0`). Under win share they
   were provably inert; under any culture-based objective they are worse than
   inert, because they never contest the card row and never attack, so "farm
   quietly" scores against them in a way it cannot score against a real
   opponent. One flag away: `--pool-weights floor=0.5`.
