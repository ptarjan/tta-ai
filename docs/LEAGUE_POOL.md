# Pruning the pool and deepening the self-ladder

Date: 2026-07-29. Code: `experiments/hillclimb_pool.py` (the rule),
`experiments/hillclimb_league.py` (the wiring), `experiments/watchdog.sh` (the
flags), `tests/test_pool_saturation.py`.

**One-line answer: an opponent the champion beats 98% of the time is not an
opponent, it is a bill.** The pool now prices every opponent by its measured
win rate and hands the freed weight to the opponents that can still lose, and
the self-ladder is deep and newest-biased instead of two entries spread across
seven hundred generations.

---

## 1. The problem, in the arm's own numbers

The 2p full pool check at gen 720, which the league already computes every
`--full-check-every` generations and writes to `fullcheck_2p.jsonl`:

| opponent | win rate | | opponent | win rate |
|---|---|---|---|---|
| `var:tempo` | 100.0% | | `var:culture` | 91.7% |
| `hum:warlord` | 97.9% | | `hall:preinfo_3p_gen00205` | 89.6% |
| `var:infra` | 97.9% | | `book` | 88.5% |
| `var:military` | 97.9% | | `hum:tempo` | 87.5% |
| `hall:preinfo_4p_gen00102` | 97.9% | | `hum:wonder` | 87.5% |
| `book2` | 95.8% | | `var:wonder` | 87.5% |
| `var:science` | 95.8% | | `hall:oneply_2p_gen00355` | 71.9% |
| `past:ladder_2p/gen00000` | 95.8% | | `hall:preinfo_2p_gen00188` | 63.5% |
| `hum:builder` | 93.8% | | **`past:ladder_2p/gen00715`** | **50.0%** |

Fifteen of eighteen sit between 87.5% and 100%. **A 98% win rate cannot go up.**
The paired statistic the league accepts on is
`candidate_score - champion_score` per game, and against a saturated opponent
both terms are pinned, so the row contributes ~0 edge with ~0 variance whatever
the mutation did. Those games are not a weak signal; they are a *bill*. At 2p
they were most of the pool.

Exactly one opponent — the newest archived self — was in a band where a
mutation could show. That is not a coincidence, it is the `--past-k 2` design:
`_spread` keeps the ENDPOINTS, so the past tier was the founder (long since
saturated at 95.8%) plus the newest. There was nothing in between because
nothing was *selected* in between.

## 2. The rule

`experiments/hillclimb_pool.py::saturation_multiplier`. An entry's share of its
tier is scaled by

    1.0                                    win rate <= LO   (default 0.70)
    linear from 1.0 down to FLOOR          LO .. HI
    FLOOR                                  win rate >= HI   (default 0.95)

with `FLOOR` = 0.15. `--saturation LO,HI,FLOOR`; `--saturation 0,1,1` turns the
whole thing off.

Four properties, each of which is the answer to a way this could have gone
wrong:

**The input is measured, not hand-listed.** The win rates come from
`state["last_full_check"]`, i.e. the full pool check the league already pays
for, and the pool is rebuilt immediately after every check. So the rule is
self-maintaining in both directions: an opponent that becomes saturated fades
without anyone editing anything, and an opponent the champion starts *losing*
to comes back at the next check. A hand-edited drop list would have been stale
within a day and would need a human every time the champion moved. An opponent
that has never been measured (a freshly archived self) counts as fully
informative, which is the safe direction.

**The tier total never changes.** The multiplier redistributes weight WITHIN a
tier. `docs/LEAGUE_OBJECTIVE.md` §3 set the external/self-play split
deliberately (32% fixed external, 68% opponents that improve) after the pool
spent a long time as a 69%-external monoculture the champion learned to farm.
A rule that deleted external opponents because we currently beat them would
have walked back into `docs/HAZARDS.md` trap 3 from the other side.
`tests/test_pool_saturation.py::test_the_external_share_cannot_be_eroded_by_saturation`
asserts that beating **every** external opponent 100% leaves the split
unmoved.

**The floor is not zero.** `_aggregate` skips rows with weight <= 0, and a
zero-weight row cannot VETO. The gate tiers' job since the rebalance is not to
supply gradient, it is to stop the climber walking off a cliff — "you may not
regress against BookBot" is a statement we want to keep being able to make even
while BookBot is being beaten 88% of the time.

**Saturated does not mean absent.** An entry at the floor is marked `inert`,
which means `acceptance_subset` will not spend a generation's games on it in
the free slots. It is still in the pool, still re-measured by the full check
(at half the games — see §4), and the two subset invariants still hold:

* **mirror is always in**;
* **one gate opponent is always in**, rotating over the live ones and falling
  back to the whole gate list when every one of them is saturated — which at
  2p is now the case. This is the anchor. There is no state of the world in
  which this pool becomes pure self-play, and
  `tests/test_pool_saturation.py::test_an_external_opponent_is_in_every_subset_even_when_all_saturated`
  is the assertion.
* **one ladder opponent is always in**, same rule.

## 3. The deeper, newest-biased self-ladder

`--past-k` 2 -> **6**, and the selection changed from `_spread` (even) to
`_recent_spread` (newest-biased, founder retained): offsets 0, 1, 3, 7, 15 back
from the newest, plus index 0.

The founder stays because the past tier's original job — the anti-cycling
tripwire, "does this champion lose to something it descends from" — needs the
most *different* archived opponent, and that is the founder. Everything else
is chosen for informativeness instead of for coverage. On the live 2p ladder
(105 archived champions) that turns

    past:gen00000 (95.8%), past:gen00715 (50.0%)

into

    past:gen00000, gen00604, gen00637, gen00712, gen00721, gen00725

i.e. one tripwire and five recent selves, which land in the 50-70% band by
construction: they are between 3 and 120 generations behind the champion.

The old help text argued for `k=2` on cost grounds — a `past:*` duel is the
wall-clock hog because every seat searches. That argument is now handled by the
saturation rule rather than by keeping the tier small: a deep ladder member
costs its weight only for as long as it is worth something, and drops out of
the acceptance rotation automatically when it is not.

## 4. Cost

The full check is the most expensive thing the loop does — at 2p it was ~730s
against ~1680s of training per ten generations. `full_check` now plays a
saturated opponent for `check_games // 2` games rather than `check_games`; it
is still measured (that is what lets it come back) but not to four significant
figures. That roughly pays for the four extra ladder members.

## 5. What it actually did, per arm

Run on each arm's own latest full check, threshold 95%:

| arm | opponents at/above 95% | reading |
|---|---|---|
| **2p** (converged) | **8 of 18** | most of the pool was dead weight |
| **3p** (productive) | **3 of 18** | trims the founder and two variants |
| **4p** (behind) | **0 of 18** | **no change at all** |

That table is the argument for the rule being automatic rather than a
configuration decision. The same code prunes hard where an arm has saturated
its pool and does nothing where it has not, and nobody has to notice which arm
is which.

## 6. Reading the log

`build_pool` prints three lines at every pool build (startup, after every
accept, after every full check):

```
[pool] book(w=0.43,blend,89%), book2(w=0.17,blend,96%,INERT), ...
[pool] saturated at >= 95% (weight cut to 0.15 of an even share, ...): book2 96%, ...
[pool] informative (win rate < 95% or unmeasured): 14 of 22 -- book 89%, ...
[pool] tier share: ... (external/fixed 32%, self-play 68%)
```

`[Kp] saturation: ...` at startup records the thresholds this run used. If the
`informative` line ever reads a small number **and** the `tier share` line
moves, something is wrong — those two are independent by construction.

## 7. Limits

* **The thresholds are not measured, they are chosen.** 0.70/0.95/0.15 come
  from looking at the shape of the table in §1, not from an experiment on how
  much gradient a 90% opponent carries. The defensible part is the *direction*
  and the fact that the rule is continuous and self-correcting; the exact knee
  is a guess. `--saturation 0,1,1` reproduces the old behaviour exactly if it
  turns out to matter.
* **A win rate is not the metric the league accepts on.** Accepts are on
  `blend` (own culture + a win-share tiebreak), and an opponent can be
  saturated on win share while still discriminating on own culture. The floor
  weight and the mandatory gate slot are what limit the damage from that
  mismatch. Using the check's own-culture column instead is the obvious
  refinement and was not done: the win-rate column is the one every historical
  full check has, so the rule works on arms that started before today.
* **The 3p/4p arms keep `--past-k 2` until their supervisors next restart.**
  The saturation rule reaches them within the hour (it is the module default,
  and the hourly climber restart picks up new code), but `--past-k` is on the
  supervisor's own command line, and those supervisors were deliberately not
  killed. The watchdog will pass 6 the next time it relaunches them.
