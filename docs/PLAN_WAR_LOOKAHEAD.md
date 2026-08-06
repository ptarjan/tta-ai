# Giving PlanBot a war lookahead

Date: 2026-07-27
Branch: `plan-war-lookahead`. Implements option (b) of [`docs/EVALUATOR_HISTORY.md`](EVALUATOR_HISTORY.md)
§8.3. Read that document first; this one is only meaningful as its sequel.

**One-line answer: the inversion is gone, and the production vector did not
regress.** [`docs/EVALUATOR_HISTORY.md`](EVALUATOR_HISTORY.md) measured the quiescent-trained vector Q
losing to the 1-ply-trained vector P by **−97.4 ± 3.7** margin at a **2.5% ±
1.1%** win share under `plan:width=8`. With the war lookahead in PlanBot, the
same 100 deals on the same seeds give **+1.4 ± 5.3** at **52.2% ± 3.7%** — a
null. The 1-ply vector P is *not* worse under the fixed PlanBot: its own
culture against `book` is unchanged at 213.4 (212.6 before), and fixed PlanBot
against unfixed PlanBot on P's own vector is +0.47 ± 3.06 margin at 0.520 ±
0.020 — a flat null in both directions (§5b).

It does **not** follow that the quiescent proxy is now predictive. It has gone
from actively wrong to uninformative: the proxy says Q is +36.3 ± 4.8 better
than P, and under the fixed PlanBot Q and P are indistinguishable. See §6.

---

## 1. The change

`engine/bots/plan.py`, one new method and one flag.

* `quiescent._war_value` was renamed `quiescent.war_value` (public; the old
  private name is kept as an alias so nothing that imported it breaks). Not
  one line of its body changed, and `QuiescentBot` still calls it through the
  alias, so `QuiescentBot`'s behaviour is bit-identical.
* `PlanBot._score(t, me, w, ctx)` replaces the two identical
  `evaluate(t, me, w, ctx)` calls in `_beam`. If `WAR_LOOKAHEAD` is on, the
  game is not over, and `t.players[me].war_declared_by_me` is set, it returns
  `war_value(...)` — the position with the declared war resolved by the
  engine's own `events.resolve_war` on a scratch copy — instead of the plain
  evaluation. Otherwise it is exactly `evaluate`.
* `PlanBot(war_lookahead=False)` / the spec `plan:FILE,width=8,war=0` restores
  the old behaviour exactly, and is what every "before" arm below that was
  re-measured in this worktree used.

### 1a. Three decisions that are not obvious

**Scored at every node, not only at the `war` node.** `QuiescentBot` prices a
war when `mv[0] == "war"`, which is sufficient there because it evaluates
exactly one ply. PlanBot searches whole-turn *sequences*: the war is normally
declared at ply 1 and the states that get scored — and, crucially, the states
that get *ranked for the beam* — are 2-5 plies later. Pricing only the
declaring node would let the war line be ranked as pure cost and pruned before
it ever reached a terminal, i.e. the same bug moved one level down. So the
predicate is a property of the state (`war_declared_by_me is not None`), not
of the move.

**No double counting.** `war_value` resolves on a `copy_state` scratch and
returns a *replacement* score for the position; the spoils never enter the
state the next ply expands. Evaluation stays a pure function of the state, so
a war contributes to any single score exactly once, however many plies later
it is scored. Two structural facts back this up: a player may hold at most one
declared war (`actions.py:300` refuses a second while `war_declared_by_me` is
set), and the beam's horizon is the end of my own turn, so the engine can
never resolve the war *inside* the search either.
`tests/test_plan_war.py::test_scoring_does_not_mutate_the_position` asserts
both halves.

**Skipped when `t.game_over`.** A war declared into a finished game never
resolves, and a game-over position is already scored on final culture, so
awarding spoils there would be inventing points. **The narrower case is NOT
handled and is a known hole**: a war declared in the last round of a line that
has not ended *yet* also never resolves (the declarer gets no next turn), and
`_score` will happily price it. `state.last_round` is available and the guard
would be one line; it was left out deliberately so that everything below
measures "PlanBot prices wars" and nothing else. It is the obvious next
experiment and it is not free of risk — the last round is exactly when a
culture war is most tempting and most wrong.

**`ctx` is still the root's `rival_context`.** PlanBot has always evaluated
every node with the rival aggregates computed at the root; `QuiescentBot`
recomputes them after a resolution. `war_value` inherits whatever `ctx` it is
handed, so a war that strips 30 culture off the rival does not update
`rival_culture_rate` for that node. Fixing that is a separate change to
PlanBot that would confound this measurement, so it was not made.

## 2. Method, and why the before/after is paired

Everything is `tools/transfer_ab.py` at its defaults (`--seed 90210`,
`--players 2`), the same tool, seeds and n as [`docs/EVALUATOR_HISTORY.md`](EVALUATOR_HISTORY.md). n and SE
are over **deals** — one game seed played from every seat — not games; 2p, so
games = 2 × deals. **2p only**, for the reasons in [`docs/EVALUATOR_HISTORY.md`](EVALUATOR_HISTORY.md) §7;
nothing here says anything about 3p or 4p. Zero engine errors in every run.

The previous run's raw per-game series survived in `/tmp/transfer_vec/*.json`,
and its `args` confirm identical `seed`, `deals`, `policy` and `policy_b`. So
the before/after is not two independent runs compared through their SEs — it
is the **same deals**, differenced deal by deal, and the paired SE is much
tighter than either arm's. As a harness check the reloaded BEFORE numbers
reproduce [`docs/EVALUATOR_HISTORY.md`](EVALUATOR_HISTORY.md) to the printed digit (−97.43 vs −97.4;
0.770 / +46.26 vs 0.770 / +46.3), which is the evidence that the two runs are
measuring the same thing.

Two vectors throughout, unchanged from [`docs/EVALUATOR_HISTORY.md`](EVALUATOR_HISTORY.md) §1:

| | file | gen |
|---|---|---|
| **Q** quiescent-trained | `experiments/hall_of_fame/preinfo_2p_gen00188.json` | 188 |
| **P** 1-ply-trained | `experiments/archive_preplan/league_state_1ply_20260726/champion_2p.json` | 355 |

## 3. Head to head, Q against P, under `plan:width=8`

| | n (deals) | Q win share | Q margin | Q culture | P culture |
|---|---|---|---|---|---|
| PlanBot **before** | 100 | 0.025 ± 0.011 | −97.43 ± 3.71 | 53.0 | 150.4 |
| PlanBot **after** | 100 | **0.522 ± 0.037** | **+1.41 ± 5.34** | 95.2 | 93.8 |
| paired after − before | 100 | **+0.497 ± 0.036** | **+98.84 ± 4.84** | | |

The collapse is gone. Under the fixed PlanBot the two vectors are a coin flip
(0.6 SE from the null on win share, 0.26 SE on margin). For scale, the flag is
worth +98.8 ± 4.8 margin to Q here, against the +52.8 ± 4.3 that the *same*
flag was worth inside `QuiescentBot` ([`docs/EVALUATOR_HISTORY.md`](EVALUATOR_HISTORY.md) §2 row 3) — a
whole-turn search can exploit a priced war harder than a 1-ply one can,
presumably because it can also plan the military build-up that wins it.

Q's own culture rises 53.0 → 95.2 and P's falls 150.4 → 93.8. **P falling here
is not a regression**: both seats are the fixed PlanBot, so this row is Q
learning to attack, and P is the thing being attacked. §5 is the row that
answers the regression question, and it says the opposite.

## 4. Both vectors against a common opponent (`book`), paired

The second, independent way of asking. Each vector plays `book` on the same
deals; the difference is taken deal by deal. `default`/`greedy`/`random` are
excluded on purpose — they are saturated.

| | n | Q vs book | P vs book | **paired Q − P** |
|---|---|---|---|---|
| PlanBot **before** | 50 | +62.87 ± 3.73 | +95.38 ± 6.59 | **−32.51 ± 6.94** |
| PlanBot **after** | 50 | **+101.25 ± 3.57** | **+104.19 ± 7.28** | **−2.94 ± 7.84** |
| paired after − before | 50 | **+38.38 ± 3.62** | **+8.81 ± 3.07** | diff-in-diff **+29.57 ± 4.52** |

The −32.5 ± 6.9 that was 4.7 SE below zero is now −2.9 ± 7.8, which is 0.4 SE
below zero: **a null, not a sign flip in the other direction.** The
difference-in-differences is +29.6 ± 4.5, 6.5 SE.

Note the second column. **P also gains, by +8.81 ± 3.07** (2.9 SE). The war
lookahead is not a Q-specific crutch; it is worth real points to the 1-ply
vector too, just 4.4x fewer of them.

### 4a. Absolute own culture, not just margins

A fix that raises Q's margin by making both scores collapse is not a fix.

| own culture vs `book` | before | after |
|---|---|---|
| **P** (1-ply-trained) | 212.6 | **213.4** |
| **Q** (quiescent-trained) | 109.2 | **127.8** |
| `book`'s culture against P | 117.2 | 109.2 |
| `book`'s culture against Q | 46.3 | **26.6** |

P's own score is unchanged (+0.8) and Q's is up 18.6. Nothing collapsed. Q's
gain is still mostly suppression — it holds `book` to 26.6 where before it held
it to 46.3 — which is exactly the style [`docs/TWOP_PROFILE.md`](TWOP_PROFILE.md#4-where-the-points-come-from) §4 describes,
now reachable under PlanBot as well. [`docs/TWOP_PROFILE.md`](TWOP_PROFILE.md#9-what-this-does-and-does-not-support) §9's warning that
`margin_share` overpays for transferred points is **not** addressed by this
change and remains live.

## 5. The regression check: search-only A/B on the same weight file

[`docs/EVALUATOR_HISTORY.md`](EVALUATOR_HISTORY.md) §4a: same vector on both sides, only the search
differs. This is the row that has to not get worse, because `plan:width=8` on
a 1-ply-lineage vector is what [`docs/BOT_ARCHITECTURE.md`](BOT_ARCHITECTURE.md)'s headline is about.

| weights | | PlanBot vs QuiescentBot | margin | n |
|---|---|---|---|---|
| **P** | before | 0.770 ± 0.041 | +46.26 ± 5.64 | 50 |
| **P** | after | **0.790 ± 0.038** | **+57.68 ± 6.50** | 50 |
| **P** | paired delta | **+0.020 ± 0.025** | **+11.42 ± 2.50** | 50 |
| **Q** | before | 0.460 ± 0.045 | −15.33 ± 4.72 | 50 |
| **Q** | after | **0.500 ± 0.052** | **−7.90 ± 5.17** | 50 |
| **Q** | paired delta | **+0.040 ± 0.028** | **+7.43 ± 2.19** | 50 |

**P did not regress.** On win share the paired delta is +0.020 ± 0.025, a null
— it is not evidence of an improvement and should not be reported as one. On
margin it is +11.42 ± 2.50, 4.6 SE positive, which *is* a real improvement:
PlanBot's edge over quiescence on the production vector grows from +46.3 to
+57.7. The honest summary is "no cost, and a small real margin gain that does
not show up in win share because 0.77 against a much weaker search is already
near the top of the win-share range".

But look at where that margin comes from (own / opponent culture in these
duels, from the raw):

| | own culture | opponent's culture |
|---|---|---|
| **P** before | 181.6 | 135.4 |
| **P** after | 183.8 | **126.1** |
| **Q** before | 72.6 | 87.9 |
| **Q** after | 72.4 | **80.3** |

Own culture is flat on both vectors (+2.2 and −0.2); essentially the whole
margin gain is the *opponent's* score falling. That is what a war lookahead
should do — a war is a transfer — but it is also [`docs/TWOP_PROFILE.md`](TWOP_PROFILE.md#9-what-this-does-and-does-not-support) §9's
complaint restated: this change makes a zero-sum move class available to
PlanBot, so it makes `margin_share` a slightly worse proxy for strength, not a
better one. It removes the mismatch between the two searches; it does not
remove the metric problem underneath.

**On Q, PlanBot is still not an upgrade.** 0.500 ± 0.052 is exactly the null
and the margin is still 1.5 SE below it. The fix moved this row from "PlanBot
is a downgrade for Q" to "PlanBot is a wash for Q"; it did not make PlanBot the
large upgrade it is for P. That is a negative result and it matters for §6.

### 5b. The sharpest regression check: fixed PlanBot against unfixed PlanBot

Both seats the same vector, both seats PlanBot, the *only* difference the flag
(`--policy plan:width=8 --policy-b plan:width=8,war=0`). No paired arithmetic
needed — the duel itself is the A/B.

| weights | war=1 win share | margin | own culture | opponent culture | n |
|---|---|---|---|---|---|
| **P** | 0.520 ± 0.020 | **+0.47 ± 3.06** | 174.0 | 173.6 | 50 |
| **Q** | **0.565 ± 0.027** | **+8.24 ± 1.86** | 48.5 | 40.2 | 50 |
| null | 0.500 | 0 | | | |

**On P this is a flat null** — +0.47 ± 3.06 margin is 0.15 SE, and 0.520 ±
0.020 win share is 1.0 SE. Combined with §5 and §4a, the conclusion about the
production vector is: **the war lookahead neither helps nor hurts it.** It does
not regress, and the +11.4 ± 2.5 of §5 should be read as "it took a little more
off the quiescent punching bag", not "P got stronger".

**On Q it is a real but modest gain**: 2.4 SE on win share, 4.4 SE on margin.
Note the size. The fix is worth +8.2 margin to Q *against its own unfixed
self*, and +98.8 to Q *against P* (§3). Those are consistent: in the mirror
both sides can declare wars and the transfers largely cancel, whereas against P
— which under the fixed PlanBot declares few — Q's suppression lands one-sided.
The right reading of §3's +98.8 is "the war class was the whole matchup",
**not** "the fix made Q 99 points stronger".

The low absolute cultures in the Q row (48.5 vs 40.2) are the Q-mirror doing
what [`docs/TWOP_PROFILE.md`](TWOP_PROFILE.md#4-where-the-points-come-from) §4 describes: two suppression engines flattening
each other's boards.

## 6. What this does and does not buy the live training run

**Does:** [`docs/EVALUATOR_HISTORY.md`](EVALUATOR_HISTORY.md) §6 concluded the proxy was *actively wrong* —
training under quiescence produced a vector strictly worse under the ship
policy. That is no longer true. The 48h arms' output is now, as far as 100
deals can say, on par with the 1-ply lineage under `plan:width=8` rather than
losing to it 39:1. The compute is no longer being spent building a strategy the
ship policy discards.

**Does not:** the proxy is still not *predictive*. Under `quiesce:levels=1` it
says Q is worth +36.3 ± 4.8 over P; under the fixed `plan:width=8` the paired
answer is −2.9 ± 7.8 and the head to head is +1.4 ± 5.3. Those are compatible
with zero and incompatible with +36. So a generation that gates in under the
quiescent proxy still cannot be assumed to gate in under PlanBot; the proxy has
gone from wrong to *uninformative about magnitude*. [`docs/EVALUATOR_HISTORY.md`](EVALUATOR_HISTORY.md)
§8.3's options (a) and (c) are not retired by this change. In particular (c) —
scoring the gate on own culture rather than margin — is untouched: §4a shows Q
still earns most of its margin by suppression, and `margin_share` still pays
twice for a stolen point.

§5b sharpens the "does not". In self-play the fix is worth +8.24 ± 1.86 margin
to Q and +0.47 ± 3.06 to P. Those are small numbers. What was large was the
*mismatch* — a search that priced a move class at zero playing one that priced
it correctly. Removing a 99-point matchup artefact is not the same as making
either search 99 points better, and nothing here should be quoted as if it
were.

The residual could also be a generation-count artefact (Q is gen 188, P is gen
355); [`docs/EVALUATOR_HISTORY.md`](EVALUATOR_HISTORY.md) §7 argued that confound does not explain an
*interaction*, and the interaction is what has been removed. What is left is a
main effect, and for a main effect the generation gap is a live confound. This
document cannot separate them.

## 7. Cost

Measured as user CPU on the identical 8-game 2p mirror workload
(`tools/behaviour_counts.py --players 2 --games 8 --spec plan:Q,width=8,war=N`),
back to back on the same box: **124.58 s at `war=0`, 126.79 s at `war=1`,
+1.8%.** The lookahead is one `copy_state` + one `resolve_war` + one
`evaluate` and it only fires on nodes where a war is actually outstanding,
which is a small minority of the beam.

Do **not** read the wall-clock times in the raw JSONs as a cost signal: the
"after" runs shared a 6-core box with five live league workers and two other
agents' jobs, the "before" runs did not, and the ratio (1.5x) is box load, not
search cost. All runs here were `nice -n 19`.

Behavioural check on the same 8-game mirror, which is what makes the mechanism
claim concrete rather than inferred:

| PlanBot on Q, 2p mirror | `war=0` | `war=1` |
|---|---|---|
| wars declared / game | **0.000** | **1.000** |
| aggressions / game | 3.375 | 3.375 |
| bids / game | 18.25 | 18.63 |
| takes / game | 56.25 | 56.00 |
| civil actions spent / turn | 2.994 | 2.991 |

Wars go from *never* to once a game and nothing else moves. n=8 games, so
these are indicative counts with no error bar attached, but 0.000 → 1.000 on
the one class that was changed and flat everywhere else is not a subtle signal.

## 8. Limits

* **2p only, n as stated.** The h2h row is 100 deals (200 games); the vsfield,
  search-only and ablation rows are 50 deals each. 900 games total, ~9 700
  cpu-s, all `nice -n 19` beside five live league workers. The achieved SEs are
  printed above and are what they are: the vsfield paired row is ±7.8, so it
  can exclude −32.5 comfortably and can *not* exclude a true effect anywhere
  in ±15. [`docs/EVALUATOR_HISTORY.md`](EVALUATOR_HISTORY.md) §7's warning applies: n=50 deals at 2p is a
  usable margin instrument and a poor win-share instrument. 3p/4p were not
  attempted.
* **`book` is one opponent and the pool is a monoculture** — every pool
  opponent is a `BookBot` subclass. Unchanged from [`docs/EVALUATOR_HISTORY.md`](EVALUATOR_HISTORY.md) §7.
* **`plan:width=8` only.** `width=1` — the width [`docs/EVALUATOR_HISTORY.md`](EVALUATOR_HISTORY.md)
  §8.3(a) proposes training under — was not tested. Whether the war lookahead
  behaves the same at width 1, where there is no beam to prune the war line
  out of, is unknown.
* **The last-round hole in §1a is real and unmeasured.**
* **Nothing here re-trains anything.** Both vectors were trained under
  searches that are not this one. The claim is about how two *frozen* vectors
  play, not about what a league trained against the fixed PlanBot would
  produce.
* **`--samples 1`.** The determinization is a single sample, as everywhere
  else; a war's value depends on strengths, which are public, so the war
  lookahead itself is not sensitive to the shuffle, but the plan around it is.

## 9. Reproducing

```
# 3
python3 tools/transfer_ab.py h2h --players 2 --policy plan:width=8 --deals 100 \
    --out A_h2h_plan.json
# 4
python3 tools/transfer_ab.py vsfield --players 2 --policy plan:width=8 --deals 50 \
    --out B_vsfield_plan.json
# 5 -- search-only A/B, same weight file on both sides, once per vector
python3 tools/transfer_ab.py h2h --deals 50 \
    --policy plan:width=8 --policy-b quiesce:levels=1 --a <vec> --b <vec>
# 5b -- fixed vs unfixed PlanBot directly, same vector both seats
python3 tools/transfer_ab.py h2h --deals 50 \
    --policy plan:width=8 --policy-b plan:width=8,war=0 --a <vec> --b <vec>
# the "before" arm of any other row, in this worktree, is `,war=0`:
python3 tools/transfer_ab.py h2h --players 2 --policy plan:width=8,war=0 --deals 100
# 7
python3 tools/behaviour_counts.py --players 2 --games 8 \
    --spec plan:<Q>,width=8,war=0        # and war=1
```

Unit tests: `tests/test_plan_war.py` (8 tests), which assert equality against
`quiescent.war_value` rather than "the number went up", so the two searches
cannot silently drift apart again. `bash tools/gate.sh` on this branch: all
four perf digests unmoved (`2fd656b3`, `7fc72fca`, `1169007d`, `9dc0a5a6`) —
neither `GreedyBot` nor `WeightedBot` is on any code path this branch touches,
and `PlanBot` has no digest arm in the gate at all. The one `unittest` failure
is the pre-existing `test_harness_mirror.ForcedRivalsAreExact`, which is not
this branch's.
