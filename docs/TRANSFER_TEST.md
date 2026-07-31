# Does a quiescent-trained vector transfer to PlanBot?

Date: 2026-07-27
Branch: `transfer-test`. Tool: `tools/transfer_ab.py` (additive; this branch
touches nothing in `engine/`).

[`docs/TRAINING_RUN.md`](TRAINING_RUN.md) names this test — "play the quiescent-trained vector
under PlanBot against the 1-ply-trained vector under PlanBot" — and says the
answer "is unknown and must not be assumed". It had never been run. It has now.

**One-line answer: it does not transfer. It inverts.** The quiescent-trained
vector is the better vector under 1-ply search and under quiescence, and the
*worse* vector under PlanBot. The sign of the difference flips with the search
policy, measured two independent ways.

---

## 1. What was played

Two frozen vectors, both 82-key over the identical feature set (so
`load_weights` fills the same 8 post-hoc features from `DEFAULT_WEIGHTS` for
each — see §7 for why that matters):

| | file | gen | sha256 (first 12) |
|---|---|---|---|
| **Q** quiescent-trained | `experiments/hall_of_fame/preinfo_2p_gen00188.json` | 188 | `4882fa880380` |
| **P** 1-ply-trained | `experiments/archive_preplan/league_state_1ply_20260726/champion_2p.json` | 355 | `55c7a3dea72e` |

Three policies, each reading the *same* vectors:

* `weighted` — 1-ply `WeightedBot`, what every run before 2026-07-27 trained.
* `quiesce:levels=1` — `QuiescentBot`, what the live 48h arms train under
  (`--candidate-bot quiescent:levels=1`; `parse_candidate_bot` leaves
  `WAR_LOOKAHEAD` at its default `True`, so `quiesce:...,levels=1` is the
  identical configuration).
* `plan:width=8` — `PlanBot`, the policy we would actually ship.

All duels are `experiments.arena.duel`, which rotates the challenger through
every seat on the same game seed. **n and SE below are over DEALS, not games**:
a deal is one game seed played from every seat, which is the independent unit.
2p, so games = 2 x deals. Zero engine errors in every run reported here.

```
python3 tools/transfer_ab.py h2h     --policy plan:width=8 --deals 100
python3 tools/transfer_ab.py vsfield --policy plan:width=8 --deals 50
```

## 2. Head to head: Q against P, same vectors, three searches

Q as challenger, P as defender, null win share 0.500, null margin 0.
Margin is Q's final culture minus P's, per game.

| search policy | n (deals) | Q win share | Q margin | Q culture | P culture |
|---|---|---|---|---|---|
| 1-ply `weighted` | 150 | 0.140 ± 0.021 | **−58.4 ± 3.1** | 95.6 | 154.0 |
| `quiesce:levels=1` **(training policy)** | 150 | 0.677 ± 0.026 | **+27.7 ± 3.3** | 111.5 | 83.7 |
| `quiesce:levels=1,war=0` | 150 | 0.287 ± 0.026 | **−25.0 ± 2.7** | 94.3 | 119.3 |
| `plan:width=8` **(ship policy)** | 100 | **0.025 ± 0.011** | **−97.4 ± 3.7** | 53.0 | 150.4 |

Q wins under exactly one search: the one it was trained under. Under PlanBot it
wins 2.5% of deals and its own score collapses to 53.

Row 3 is the mechanism, and it is a one-flag ablation. `QuiescentBot.
WAR_LOOKAHEAD` is a special case that evaluates a `war` candidate with the
declared war *already fought* (`quiescent.py::_war_value`), because a war's
loot resolves on the declarer's **next** turn and so is outside the pending
stack. Turn that single flag off and nothing else, and Q goes from winning
67.7% to losing 28.7% — a 52.8 ± 4.3 point swing in margin. **PlanBot has no
war lookahead.** It drains `state.pending` (`plan.py::_quiesce`) so aggressions,
pacts and colony bids are priced, but a war is still scored as pure cost.

## 3. The second way of asking: both vectors against a common opponent

Head-to-head results can be intransitive, so the same question was asked
without the two vectors ever meeting: each plays `book` on the **same deals**,
and the difference is taken deal by deal. `book` is the pool opponent
[`docs/TWOP_PROFILE.md`](TWOP_PROFILE.md) uses; `default`/`greedy`/`random` are excluded on
purpose.

| search policy | n | Q vs book | P vs book | **paired Q − P** |
|---|---|---|---|---|
| 1-ply `weighted` | 100 | +69.5 ± 3.0 | +35.8 ± 4.9 | **+33.6 ± 5.6** |
| `quiesce:levels=1` | 100 | +82.1 ± 3.0 | +45.7 ± 4.6 | **+36.3 ± 4.8** |
| `plan:width=8` | 50 | +62.9 ± 3.7 | +95.4 ± 6.6 | **−32.5 ± 6.9** |

(margins; win share against `book` saturates at 0.94-0.97 under PlanBot and
cannot discriminate, which is why margin is the reported signal.)

**Same conclusion, opposite method.** Q is worth +34 to +36 margin over P under
the two cheap searches and **−32.5 ± 6.9 under PlanBot**. The flip is
4.7 SE away from zero; this is not a null being dressed up.

The Q vs book numbers also reproduce [`docs/TWOP_PROFILE.md`](TWOP_PROFILE.md#8-the-strategy-is-a-property-of-the-search-not-of-the-weights-alone) §8 independently
(that document: +65.2 at 1 ply, +85.5 under quiescence; here +69.5 and +82.1 on
a different seed set and a slightly later gen), which is the harness check that
these two runs are measuring what they claim to.

## 4. What PlanBot does to each vector, in absolute score

The margins hide the interesting half. Own culture against `book`:

| | 1-ply | quiescence | **PlanBot** |
|---|---|---|---|
| **P** (1-ply-trained) | 161.1 | 160.6 | **212.6** |
| **Q** (quiescent-trained) | 138.2 | 125.2 | **109.2** |

PlanBot is worth **+51.5** to P's own score and **−29.0** to Q's. It is not that
PlanBot is a weaker search — 212.6 is squarely in line with
[`docs/BOT_ARCHITECTURE.md`](BOT_ARCHITECTURE.md)'s 194.5 / 202.3 for PlanBot on that same 1-ply
lineage. It is that the two vectors want different searches.

### 4a. "PlanBot is the strongest policy we have" is vector-dependent

[`docs/BOT_ARCHITECTURE.md`](BOT_ARCHITECTURE.md)'s headline — PlanBot beats the champion
**88.6% ± 3.1%** on identical weights — was measured on
`experiments/champion_2p.json` gen 209 and, replicated, gen 344. **Both are
members of the 1-ply lineage, i.e. P's family.** So the claim is established
*for a 1-ply-trained vector*. Run the identical search-only A/B on each of our
two vectors — same weight file on both sides, only the search differs:

| weights | PlanBot vs QuiescentBot | margin | n (deals) |
|---|---|---|---|
| **P** (1-ply-trained) | **0.770 ± 0.041** | **+46.3 ± 5.6** | 50 |
| **Q** (quiescent-trained) | **0.460 ± 0.045** | **−15.3 ± 4.7** | 50 |
| null | 0.500 | 0 | |

On P, PlanBot is the large upgrade the architecture doc describes (and 0.770
against `QuiescentBot` rather than against 1-ply is the right order of size —
quiescence is itself worth +20.3 over 1 ply on trained weights,
[`docs/TWOP_PROFILE.md`](TWOP_PROFILE.md#8-the-strategy-is-a-property-of-the-search-not-of-the-weights-alone) §8). **On Q, PlanBot is not an upgrade at all**: win
share is 0.9 SE from the null and margin is 3.2 SE *below* it. The difference
between the two rows is 0.310 ± 0.061 in win share and 61.6 ± 7.3 in margin.

The honest reading of the Q row on its own: on win share it is a null at n=50
deals, and it would take roughly 4x the games to resolve a 5-point win-share
effect there. It is the *contrast with the P row*, which is enormous and
unambiguous, that carries the finding. Whether to ship PlanBot is not a
question with one answer; it depends on which vector you are shipping.

## 5. Why the two vectors are different animals

The scores in §2 and §4 are the whole story and they agree with
[`docs/TWOP_PROFILE.md`](TWOP_PROFILE.md#4-where-the-points-come-from) §4 and §9.

* **P is a production engine.** It scores 160 against `book` and lets `book`
  score 115-125. Its `culture_rate` weight is 33.91.
* **Q is a suppression engine.** It scores 125-138 but holds `book` to 43-69.
  Its `culture_rate` is 0.304. [`docs/TWOP_PROFILE.md`](TWOP_PROFILE.md) measures where its points
  come from: 62.0 ± 2.0 of its 85.5 margin is war and aggression *transfers*,
  and banning the conflict move class barely moves its own total (131.0 → 119.8)
  while nearly doubling `book`'s (45.5 → 93.8).

That is exactly the asymmetry [`docs/TWOP_PROFILE.md`](TWOP_PROFILE.md#9-what-this-does-and-does-not-support) §9 flagged as a risk and
this document now measures as realised: **the league gates on `margin_share`,
and war/aggression transfers are zero-sum by construction, so stealing 25 points
earns 50 points of margin while adding nothing to your own board.** Retargeting
the trainer to `quiescent:levels=1` made that move class selectable for the
first time ([`docs/DEEPER_SEARCH.md`](DEEPER_SEARCH.md#40-first-the-defect-itself-measured-rather-than-argued) §4.0: `aggression` ranked first at 0 of 72
decisions at 1 ply, 23 after quiescence), and the climber walked straight into
the term the metric overpays for.

Head-to-head that is a fine strategy against the thing you were tuned against,
and it is genuinely better against `book` under the cheap searches. Under a
search that cannot price a war it is a strategy with the payoff removed and the
cost left in, and Q's own score falls to 53 (§2).

## 6. Answering the question [`docs/TRAINING_RUN.md`](TRAINING_RUN.md) asked

> If the quiescent-trained vector is not better *under PlanBot*, the proxy
> failed and the retargeting bought nothing.

It is not better under PlanBot. It is 32.5 ± 6.9 margin points **worse** against
a common opponent and loses 97.4 ± 3.7 head to head at a 2.5% ± 1.1% win share.

This is the second of the three pre-registered outcomes: **the proxy is
actively wrong**, not merely weak. The retargeting did not buy a neutral or
slightly-noisy approximation of the PlanBot-tuned vector; it bought a vector
that is worse under PlanBot than the one the previous, cheaper run produced.

The argument that was offered for expecting transfer — that both bots fix the
same root cause, `apply()` stopping at a pending decision — is *half* right, and
the half it gets wrong is the half that carries the points. Both searches
resolve the pending stack, so pacts, colony bids and action cards do transfer.
But the single largest source of the quiescent champion's edge is **war**, and a
war does not live in the pending stack: it resolves a full round later and only
`QuiescentBot.WAR_LOOKAHEAD` prices it. §2 row 3 measures that flag alone as
worth 52.8 ± 4.3 margin points to Q. The proxy and the target disagree on
precisely the move class the proxy taught the climber to build a strategy around.

## 7. Limits — what this does not establish

* **2p only.** PlanBot costs ~7.2 cpu-s/game with one searching seat and ~15
  in a 2p mirror; the §2 PlanBot row alone was 3 100 cpu-seconds. At 3p/4p the
  same design is 2-4x that *and* the 1-ply arm's 3p champion only reached
  gen 27 against Q's gen 205, which would confound the comparison beyond
  rescue. 3p/4p were not attempted, deliberately: better one table measured
  properly than three measured badly.
* **Different generation counts.** Q is gen 188, P is gen 355. That is a real
  confound for any *main effect* of "which vector is better". It is not a
  confound for the finding here, which is an **interaction**: Q beats P under
  two searches and loses under a third. A vector that is simply undertrained
  does not win by +36 under one policy and lose by −32 under another.
* **Engine drift.** Both vectors were trained when `features()` emitted 82
  keys; master now emits 89. `load_weights` fills the 7 newer features from
  `DEFAULT_WEIGHTS` identically for both, so the comparison is fair, but both
  are being played slightly off the distribution they were tuned on.
* **`book` is one opponent, and the pool is a monoculture.** `docs/TWOP_
  PROFILE.md` §9's warning applies unchanged: every pool opponent is a
  `BookBot` subclass. The §3 result is a paired difference on a common
  opponent, which is robust to that opponent being weak, but it is not
  evidence about a human or the official app AI.
* **`plan:width=8` is one point on a curve.** `width=1` was not tested here.
  [`docs/BOT_ARCHITECTURE.md`](BOT_ARCHITECTURE.md) measures `width=1` at 62.3% vs 85.1% for `width=8`
  on the 1-ply lineage; whether the flip in §3 is width-dependent is unknown.

## 8. What follows for the live 48h run

Stated as findings, not as instructions:

1. The run is training under a search whose distinguishing feature —
   `WAR_LOOKAHEAD` — the ship policy does not have, and that feature is worth
   52.8 ± 4.3 margin points to the vector the run has produced. Every
   generation spent this way optimises a term PlanBot will not honour.
2. The gate metric is the amplifier, not the search. `margin_share` pays twice
   for a transferred point and once for a produced point. Quiescence did not
   create that; it removed the thing that was hiding it.
3. Three ways out, in rough order of cost: (a) train under `plan:width=1`,
   which costs 9.1x rather than 49-66x and does have the fixed horizon and the
   determinization, at ~21 generations per 12h at 4p; (b) give PlanBot a war
   lookahead so the two searches price the same move class, which makes the
   proxy honest and is a change to `engine/bots/plan.py` rather than to the
   trainer; (c) score the gate on own-culture rather than margin, so theft is
   paid once, which [`docs/TWOP_PROFILE.md`](TWOP_PROFILE.md#9-what-this-does-and-does-not-support) §9 already proposed.
   (b) is the cheapest and is the one this document's §2 row 3 argues for
   directly: the gap between the two searches is a *single flag*.
4. Nothing here says quiescence is a worse search than 1 ply, or that Q is a
   bad vector. Q is the better vector under both cheap searches by 34-36 margin
   points. The finding is narrower and worse: it is the better vector under
   everything except the thing we would ship.

## Reproducing

```
# §2
for pol in weighted quiesce:levels=1 quiesce:levels=1,war=0 plan:width=8; do
  python3 tools/transfer_ab.py h2h --players 2 --policy $pol --deals 100
done
# §3
for pol in weighted quiesce:levels=1 plan:width=8; do
  python3 tools/transfer_ab.py vsfield --players 2 --policy $pol --deals 50
done
# §4a -- search-only A/B, same weight file on both sides
python3 tools/transfer_ab.py h2h --deals 50 \
    --policy plan:width=8 --policy-b quiesce:levels=1 \
    --a <vector> --b <same vector>
```

Every run above was `nice -n 19` on a 6-core box also carrying the five live
training workers, which is why the PlanBot rows use smaller n than the cheap
ones. All were run from a `transfer-test` worktree; the live arms were not
touched.

Defaults point at the two vectors in §1 in the main checkout; both live in
untracked directories (`hall_of_fame/`, `archive_preplan/`) because the
trainer's output is never committed, so the tool holds absolute paths the way
`tools/champ_vs_drift.py` does. Raw per-game series go to `--out`.
