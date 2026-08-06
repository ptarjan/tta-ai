# Human-derived pool opponents (2026-07-27)

> **BANNER 2026-08-06: the machinery this describes is gone; the corpus
> finding is not.** `engine/bots/human/`, `tools/bgo_cluster.py`,
> `tools/human_fit.py` and the `human` pool tier in `hillclimb_pool.py` /
> `hillclimb_league.py` were all Python and were deleted with `engine/` and
> the Python half of `experiments/` on 2026-08-06. The Rust league
> (`rust/src/bin/climb.rs`) has no opponent pool of any kind — mutant vs.
> champion mirror plus a fixed anchor — so there is no live equivalent of a
> "human bot" pool opponent to point this at. What survives and is still
> worth reading: §1's negative result that the 1,011-game BGO corpus does
> not cluster into discrete human archetypes (k-means barely beats a
> permutation null) — that is a fact about the corpus, independent of any
> bot implementation.

Branch `human-bots`. New code: `engine/bots/human/`, `tools/bgo_cluster.py`,
`tools/human_fit.py`, `experiments/human_exploit.py`,
`experiments/human_strength.py`, `tests/test_human_bots.py`, plus a `human`
tier and a `--human-bots` flag in `experiments/hillclimb_pool.py` /
`hillclimb_league.py` / `watchdog.sh`.

This is a working note, not a report. Negatives are in it because they are the
useful part.

## Why

[`docs/HAZARDS.md`](HAZARDS.md) trap 3: **every pool opponent is a BookBot subclass**, and
[`docs/TWOP_PROFILE.md`](TWOP_PROFILE.md) measured the consequence — `var:military` needs a +3
strength lead to fire, and the champion holds it under +3 on 94.5% of turns.
The champion does not beat that bot, it switches it off. Then
[`docs/HUMAN_BASELINE.md`](HUMAN_BASELINE.md) and [`docs/SCORE_AUDIT.md`](SCORE_AUDIT.md) established that the
policy this produced is not one any human plays and scores less than half what
humans score *on a scorer verified exact against BGO*.

So: build opponents from the 1,011 real games, with no threshold to hold shut.

## 1. What the corpus actually contains: a continuum, not archetypes

**This is the main negative result and it shaped everything else.**
`tools/bgo_cluster.py` runs k-means over twelve behavioural axes (wonders,
stages, takes, tier-3 rate, wars, aggressions, bids, science, first-government
round, leaders, age-I and age-III takes) against a **permutation null** — the
same data with each column independently shuffled, which keeps every marginal
distribution and destroys only the co-occurrence between axes. A real
archetype is co-occurring behaviour, so that is the null it has to beat.

| players | k | silhouette | null | ratio | split-half ARI |
|---|---|---|---|---|---|
| 2p (n=692 games) | 2 | 0.138 | 0.101 | **1.37** | 0.53 |
| | 3 | 0.136 | 0.101 | 1.35 | 0.33 |
| | 4 | 0.110 | 0.107 | **1.03** | 0.27 |
| | 5 | 0.097 | 0.089 | 1.09 | 0.69 |
| | 6 | 0.099 | 0.084 | 1.17 | 0.43 |
| 3p (n=133) | 2 | 0.131 | 0.070 | 1.86 | 0.53 |
| | 3 | 0.138 | 0.080 | 1.72 | 0.29 |
| 4p (n=186) | 2 | 0.132 | 0.078 | 1.70 | 0.68 |
| | 3 | 0.115 | 0.083 | 1.39 | 0.31 |

A silhouette of 0.10-0.14 is weak in absolute terms, and 1.03-1.37x a null
that has no clusters in it *by construction* is not evidence of types. The
split-half ARI (cluster two random halves independently, re-assign everyone by
nearest centroid, compare) is 0.24-0.69 and does not improve with k.

**Human play in this corpus is one blob with directions in it.** There is
exactly one genuinely discrete behaviour: war. 83% of 2p players declare zero
wars all game and the rest declare 1-2, which is bimodal in a way nothing else
is.

The same three directions do recover at every k, which is why four archetypes
and not seven:

* economy size (how much civilization you build at all),
* cards versus wonders (what you spend the economy on),
* the militarist minority.

### The segmentation actually used

Given that, a rule you can read beats a centroid you cannot.
`tools/human_fit.py:segment()` cuts at the corpus's own quantiles:

    wars_declared >= 1                     -> warlord
    else wonder_stages >= 11               -> wonder
    else takes >= 37 and wonder_stages <= 9-> tempo
    else wonder_stages <= 6 and takes <= 31-> passive
    else                                      builder

2p rows (n=1,383), all means per player per game:

| segment | rows | score | wonders | stages | takes | tier3% | wars | aggr | 1st gov | won |
|---|---|---|---|---|---|---|---|---|---|---|
| **builder** | 534 (38.6%) | 156.5 | 2.57 | 8.11 | 33.0 | 4.48 | 0 | 0.58 | 12.0 | 40% |
| **wonder** | 307 (22.2%) | **183.8** | 3.76 | 12.73 | 35.7 | 4.59 | 0 | 0.40 | 12.0 | 53% |
| **tempo** | 194 (14.0%) | 156.1 | 2.42 | 7.22 | **39.7** | 4.28 | 0 | 0.53 | 11.4 | 57% |
| **warlord** | 237 (17.1%) | 158.3 | 2.60 | 8.36 | 33.9 | 4.53 | **1.48** | **1.46** | 11.2 | **73%** |
| *passive* | 111 (8.0%) | 115.5 | 1.59 | 4.49 | 27.6 | 4.72 | 0 | 0.70 | 12.0 | 26% |

Two things to be careful with:

* **The win column is endogenous.** The cut is on behaviour that is downstream
  of who is already winning — a human declares war *because* they are ahead
  and want to close. 73% for the warlord segment is mostly selection, not a
  measurement that war is good. §4 measures the bots, which is the clean
  version of the question.
* **`tier3_pct` is flat at 4.3-4.7% across every segment**, exactly as
  [`docs/HUMAN_BASELINE.md`](HUMAN_BASELINE.md) found it flat across skill. It is a universal
  convention of human play, not a style axis, so no archetype varies it.

**`passive` was deliberately not built.** It is the losing tail (26% against a
50% null). A bot fitted to it would be pool decoration, which is
[`docs/HAZARDS.md`](HAZARDS.md) trap 2.

### Does skill predict style? Yes — and against the obvious direction

Segment mix within each BGO level, 2p rows:

| level | builder | wonder | tempo | warlord | passive |
|---|---|---|---|---|---|
| Emperor (662) | 37.5% | 19.3% | 15.0% | **18.1%** | 10.1% |
| King (204) | 35.3% | 21.6% | 17.2% | 21.1% | 4.9% |
| Prince (185) | 42.7% | **27.0%** | 16.2% | 9.7% | 4.3% |
| Warlord (332) | 40.7% | 25.6% | 9.0% | 16.9% | 7.8% |

Stronger players fight more and take more cards; weaker players build more
wonders. Combined with [`docs/HUMAN_BASELINE.md`](HUMAN_BASELINE.md)'s finding that Emperor games
*score lower* than Prince games, there is no clean "imitate the top tier"
target here, so every archetype is fitted to its **whole 2p segment**, not to
an Emperor-only slice. The brief's instruction to prefer the higher-skill
subset was checked and not followed, for that reason.

## 2. The bots

`engine/bots/human/` — `HumanBot(VariantBot)` plus four archetypes. Technique
(a) from the brief: statistics-targeted, fitted with `tools/human_fit.py`
against `tools/bgo_botmatch.py`'s output in the corpus schema. Technique (b),
behaviour cloning, was **not attempted**: [`docs/HUMAN_BASELINE.md`](HUMAN_BASELINE.md) already
records that the card row is never printed in the journals, so the state at
the decision that matters most — which card did they pass up, and at what
price — is not reconstructible. That is a fact about the corpus, not a
shortcut.

Three things distinguish these from the variant roster:

1. **Every military decision is a logistic, not a threshold.**
   `p(fire) = rate * logistic((lead - centre) / width)` with `width = 2.5`.
   There is no lead at which the behaviour switches off. §4 is the test.
2. **They are stochastic.** Softmax over the acceptable takes (`take_temp`),
   Gaussian noise on every scored option, and a per-action coin flip on
   whether the take rule is tried before or after the build rules. Every
   variant in the roster is deterministic by design; a deterministic opponent
   is one line to be memorised. They remain *reproducible*: the seed comes
   from `arena.py`'s `seed * 97 + i * 13 + 1`, which depends only on the game
   seed and the seat, so a candidate duel and its paired champion duel meet
   the same opponent draw.
3. **Their numbers are fitted, not chosen.** Each class carries its `TARGET`
   (the corpus segment mean) and `FIT_KNOBS` (what the fitter was allowed to
   move).

### Two engine-level facts the fit turned up, which matter beyond this branch

**(a) The whole bot family throws away civil actions, and it is a
`take_cost`-vs-`row_cost` confusion.** `engine/actions.py:take_cost` is
`row_cost(idx) + completed wonders`. The variant roster compares that *total*
against its `max_take_cost` cap, so a bot that finishes three wonders finds
every slot in the row priced at 4+, its take rule silently stops producing
candidates, and it ends its turn. Measured at 2p:

| bot | mean civil actions unused at `end_turn` | turns wasting >= 1 |
|---|---|---|
| BookBot | **1.68** | **56%** |
| HumanBuilderBot | 1.31 | 32% |

1.68 x 19 rounds is ~32 wasted civil actions a game. This is the same trap
[`docs/HUMAN_BASELINE.md`](HUMAN_BASELINE.md) names as trap 2 on the analysis side, appearing on
the policy side. `HumanBot` gates on the row tier (`cap_on_tier`).

**(b) The family buys the wrong government.** BookBot revolts to the first
affordable Age I government — Monarchy, **5** civil actions — around round 9,
and never revolts again. Humans skip Age I governments and go to
Constitutional Monarchy (**6** CA, 35% of players) or Republic (**7** CA, 22%)
at a median round of 12. Over the back eight rounds that is 12-16 extra civil
actions. `gov_min_age` makes the human bots wait; it moves first-government
round from 9.5-10.4 to 12.4, matching the corpus 12.0.

### Fidelity: the same table [`docs/HUMAN_BASELINE.md`](HUMAN_BASELINE.md) uses

All bot columns are a mirror table, n=72 games, seeds 900+ (a different block
from the fit's, so this is a held-out measurement). The `champion` column is
`quiesce:champion_2p.json,levels=1`, n=40, copied from the same measurement
[`docs/HUMAN_BASELINE.md`](HUMAN_BASELINE.md) uses. Every human bot's own `TARGET` (its corpus
segment mean) is the `tgt` column. **Bold = the bot; the point is the bold
number's distance from the tgt beside it versus the champion's distance from
it.**

| axis | build tgt | build bot | wondr tgt | wondr bot | tempo tgt | tempo bot | war tgt | war bot | champion |
|---|---|---|---|---|---|---|---|---|---|
| score | 156.5 | **147.9** | 183.8 | **146.6** | 156.1 | **155.8** | 158.4 | **82.4** | 70.1 |
| wonders completed | 2.57 | **2.65** | 3.76 | **3.08** | 2.42 | **2.57** | 2.60 | **1.42** | 0.38 |
| wonder stages | 8.11 | **8.15** | 12.73 | **10.24** | 7.22 | **7.79** | 8.36 | **5.35** | 1.82 |
| civil cards taken | 33.0 | **27.5** | 35.7 | **27.7** | 39.7 | **28.0** | 33.9 | **24.1** | 22.2 |
| % takes at 3 CA | 4.48 | **2.83** | 4.59 | **4.40** | 4.28 | **1.76** | 4.53 | **1.66** | **23.66** |
| wars declared | 0.00 | **0.02** | 0.00 | **0.00** | 0.00 | **0.01** | 1.48 | **1.47** | 0.57 |
| aggressions | 0.58 | **0.15** | 0.40 | **0.04** | 0.53 | **0.12** | 1.46 | **0.93** | 0.90 |
| 1st gov round | 11.97 | **12.46** | 11.95 | **12.04** | 11.42 | **12.31** | 11.22 | **12.23** | 7.70 |
| unspent science | 15.5 | **10.9** | 17.8 | **9.2** | 16.1 | **10.1** | 13.2 | **5.5** | 8.6 |
| colony bids | 3.24 | **3.26** | 3.38 | **3.22** | 3.50 | **3.15** | 3.02 | **4.55** | 1.76 |
| leaders elected | 3.65 | **3.61** | 3.69 | **3.56** | 3.90 | **3.60** | 3.75 | **3.28** | 2.69 |
| rounds | 19.4 | **20.1** | 19.5 | **19.9** | 19.2 | **20.4** | 19.6 | **20.6** | 17.4 |

Distance to target, as a weighted mean squared residual in units of human
standard deviations (`tools/human_fit.py:loss`, EVERY axis weighted equally so
the number is not tuned to the bot), each archetype scored against its OWN
segment target:

| target segment | its bot | champion | book | book2 |
|---|---|---|---|---|
| builder | **0.15** | 3.33 | 1.35 | 1.21 |
| wonder | **0.37** | 4.75 | 2.32 | 2.36 |
| tempo | **0.57** | 3.89 | 2.03 | 1.85 |
| warlord | **0.78** | 3.55 | 1.82 | 1.67 |

And against the whole-corpus 2p mean, so the families are on one scale:

    builder 0.23   wonder 0.24   tempo 0.25   warlord 1.25
    book 1.50   book2 1.38   var:wonder 0.98   var:military 2.83
    champion 3.49

**Every human bot lands materially closer to its human archetype than the
champion does — the brief's bar — and closer than book/book2 too.** The
one that clears it least is warlord, for a reason that is not a fit failure:
see the note in `archetypes.py`, two warring bots on a mirror table take each
other's culture and land near 80 culture whatever their war knobs say, so its
score (82 vs a corpus 158) and science are held out of its loss. On the axes
it exists for — 1.47 wars against a target 1.48, 0.93 aggressions against 1.46
— it is faithful.

**Where every bot still misses, in the same direction:** takes are ~27-28
against targets of 33-40, and unspent science is low. The take gap survived
everything (a priority-order promotion, a soft price ladder, a tier-based cap,
`take_bias` up to 20). At 2p the row is dealt 145 cards a game across ~19
rounds and both seats draw from it, and past ~28 takes the accept-worthy cards
are simply not on offer when a civil action is free. Humans reach 34 partly
via take-backs ([`docs/HUMAN_BASELINE.md`](HUMAN_BASELINE.md): 6,786 of them, ~8% of takes,
reconsidered *with* full cost information) — an affordance the engine does not
give a bot. So the ceiling here looks structural, not a knob left un-tuned,
and it is called out rather than hidden. It still moves the family from the
champion's 22 toward the human 34, which is the direction that matters.

## 3. Are they real opponents?

`experiments/human_strength.py`, seat-rotated and seed-paired like
[`docs/BOT_ROSTER.md`](BOT_ROSTER.md), null = 50%.

| bot | vs book (n=120) | vs book2 (n=120) | vs champion (n=60) |
|---|---|---|---|
| hum:builder | **75.0% ± 7.8%** | **66.7% ± 8.5%** | 10.0% ± 7.7% |
| hum:wonder | **69.6% ± 8.2%** | **67.5% ± 8.4%** | 9.2% ± 7.2% |
| hum:tempo | **71.7% ± 8.1%** | **59.2% ± 8.8%** | 16.7% ± 9.5% |
| hum:warlord | 53.8% ± 8.9% | 59.2% ± 8.8% | 1.7% ± 3.3% |
| *var:culture* (ref) | *80% (roster)* | — | 5.0% ± 5.6% |

Read this honestly:

* **Three of the four beat both book bots convincingly** (60-75%, CIs clear of
  50%). They are above par, i.e. real gate opponents by the
  [`docs/BOT_ROSTER.md`](BOT_ROSTER.md) standard, not floor filler. builder is the strongest.
* **hum:warlord is a sparring partner, not a gate.** It only ties book (54%,
  CI touches 50%). Fitting it to human war rates costs it economy — it takes
  fewer cards, builds fewer wonders and scores 82 — exactly the "bought an
  army it never converted" failure [`docs/BOT_ROSTER.md`](BOT_ROSTER.md) flags for the roster's
  own MilitaryBot. It earns its pool slot on **behaviour**, not strength: it
  is the only opponent in the pool that declares wars at a human rate without
  a threshold, which is the whole point.
* **All four lose to the champion**, warlord almost totally (1.7%). This is
  the same shape as the existing roster: var:culture, the strongest variant in
  [`docs/BOT_ROSTER.md`](BOT_ROSTER.md), wins only 5% against this champion. The champion is
  strong; a pool opponent does not have to beat it to be worth training
  against — it has to be a *different, unexploitable* thing to beat, which §4
  is about. A margin of -55 to -81 culture against the champion is dense
  gradient for the league's margin-scored gate, not a flat 0/1 loss.

## 4. Exploitability — the headline

`experiments/human_exploit.py`. The subject bot sits in seat 0 against a table
of the foe; at every politics phase it records `trigger` — the probability its
own military gate says fire (for a HumanBot the logistic value; for a
threshold bot the 0/1 of `lead >= agg_lead`, so the two are on one axis) — and
`lead`, the mean strength lead it holds over the rival at those decisions. The
suppression ratio is `trigger vs champion / trigger vs book`. n=30 games/cell.

| subject | vs book | vs book2 | vs **champion** | lead vs champ | ratio | wars+aggr/g vs champ |
|---|---|---|---|---|---|---|
| **var:military** (the known exploit) | 0.452 | 0.477 | **0.079** | **−3.5** | **×0.18** | 0.30 |
| var:culture | 0.113 | 0.130 | 0.050 | −6.6 | ×0.44 | 0.17 |
| **hum:warlord** | 0.146 | 0.151 | **0.062** | **−4.0** | **×0.42** | 0.33 |
| hum:builder | 0.028 | 0.026 | 0.007 | −5.4 | ×0.25 | 0.00 |
| hum:tempo | 0.027 | 0.028 | 0.009 | −5.3 | ×0.33 | 0.03 |
| hum:wonder | 0.012 | 0.012 | 0.003 | −6.0 | ×0.22 | 0.03 |

**The verdict, stated plainly.** [`docs/TWOP_PROFILE.md`](TWOP_PROFILE.md)'s exploit reproduces
exactly: `var:military` fires on 45-48% of its decisions against book and
**7.9%** against the champion — a ×0.18 collapse — because the champion learns
to hold it 3.5 points behind, and `var:military`'s gate is a hard step at +3
that a bot held below +3 simply never crosses. That is the monoculture
failure the whole task exists to kill.

The human warlord is **not** killed the same way. The champion still suppresses
its military *position* just as hard — it holds it 4.0 points behind, a deeper
hole than it puts `var:military` in — but the warlord's gate is a logistic with
no zero, so at 4 points behind it still fires at 6.2% and declares **0.33
wars+aggressions per game against the champion, more than `var:military`'s
0.30.** There is no lead at which it switches off, so there is no threshold for
the champion to sit under. The suppression that remains (×0.42) is the smooth,
bounded kind: pushing the bot's lead down moves its firing rate a few percent
along a curve, it never falls off a cliff to zero. **That is the property the
pool was missing, and it holds.**

Two honest caveats on this table, both pointing the same way:

* **The champion has never trained against these bots**, so this is the
  *pre*-training exploitability. The real test is whether, after the arms train
  against the new pool, the champion can drive a human bot's firing to
  `var:military`-like levels. It structurally cannot drive a no-zero logistic
  to zero, but it can push the lead further negative; §6 and the next run will
  measure the post-training rate. What this table establishes is that there is
  no *cliff* to find, which is the necessary condition.
* **The non-warlord human bots barely fight at all** (trigger 0.01-0.03), so
  their suppression ratios are computed on tiny rates and are noisy. That is
  fine — they are not military bots, they are not supposed to fight, and
  matching the human 0-war segments is itself the fidelity result. The
  exploitability claim rests on the warlord, which is the bot built to carry
  it.

## 5. Wiring

They join the pool as their **own tier**, `human`, not folded into `variant`.
Two reasons: the `variant` tier's weight is split evenly across its members
(`experiments/hillclimb_pool.py` docstring), so adding four bots to it would
have quietly demoted every existing variant from 0.42 to 0.25; and the human
tier is a gate and margin tier in its own right, so losing to it can veto an
acceptance the way losing to book or a variant does.

* `experiments/hillclimb_pool.py`: `discover_humans()`, a `human` entry in
  `DEFAULT_TIER_WEIGHTS` (2.5, equal to the whole variant roster on purpose —
  they are the only external-anchored opponents in the pool), and `human` added
  to `DEFAULT_GATE_TIERS` and `DEFAULT_MARGIN_TIERS`. `make_bot` grows a
  `("human", module, cls)` spec.
* `experiments/hillclimb_league.py`: `--human-bots LIST` (`all` default,
  `none`, or a comma list), threaded through `run()` into `build_pool`.
* `experiments/watchdog.sh`: passes `--human-bots all` on every relaunch. This
  is load-bearing — like `--candidate-bot` and `--hall-dir`, the pool
  composition is **not** persisted in the state dir, so a relaunch that forgot
  the flag would keep training but against the old monoculture, with nothing
  but the `[pool]` line to show it ([`docs/HAZARDS.md`](HAZARDS.md) trap 5). A test pins
  that the flag is in both files.

The `[pool]` line this produces (verified from `build_pool`):

    [pool] book(w=1.50,margin), book2(w=1.50,margin),
           hum:builder(w=0.62,margin), hum:tempo(w=0.62,margin),
           hum:warlord(w=0.62,margin), hum:wonder(w=0.62,margin),
           var:culture(w=0.42,margin) ... mirror(w=1.00) ...
           hall:* ... default/greedy/random(w=0.17)

**The training arms were NOT restarted by this branch.** A separate agent owns
the single coordinated restart and will pick these up from master. Default-on
(`--human-bots all`) means that restart cannot silently drop them.

## 6. What this cannot tell you

* **Pre-training exploitability only** (§4). The champion has never seen these
  bots. The claim proven here is the necessary condition — there is no
  threshold cliff to exploit — not the sufficient one, which the next run
  measures.
* **The take ceiling is structural at ~28.** Every human bot undershoots the
  human 33-40 takes and none of the four knobs that should have fixed it did.
  The leading suspect (§2) is take-backs, which humans have and bots do not; it
  is not proven, and if a future engine change gives bots more takes the fit
  should be re-run.
* **Fitted at 2p only.** The corpus segmentation was checked at 3p and 4p
  (`tools/bgo_cluster.py`, silhouette ratios 1.4-1.9, same three directions)
  and the bots run and finish at all three counts (`tests/test_human_bots.py`),
  but their `TARGET`s and knobs are the 2p segment means. The `PROFILE` knobs
  that are player-count-keyed (`unit_cap`, `max_take_cost`) fall back to the 2p
  value at 3p/4p. A 3p/4p fit is the obvious next step and is cheap
  (`human_fit.py --players 3`).
* **n on every bot measurement here is 30-120 games.** The strength CIs are
  ±7-10 win points and the exploit rates on the quiet bots are noisy (§4). The
  fidelity table is n=72. None of these support a sub-10% claim; the findings
  that carry weight are the multiples (×0.18 vs ×0.42) and the loss-column gaps
  (0.15-0.78 vs the champion's 3.3-4.7), not any single win rate.
* **The fit objective is a mirror table.** `tools/bgo_botmatch.py` plays each
  bot against itself, which is the right "what does this policy do when nobody
  is exploitable" measurement and matches how the human corpus was generated
  (humans vs humans), but it means an archetype's *score* is not a target worth
  fitting for the warring bot (two warlords cannibalise each other), which is
  why warlord's score and science are zero-weighted.

## 6. What this cannot tell you

<!-- CAVEATS -->
