# Behaviour cloning from the BGO human corpus (2026-07-27)

> **BANNER 2026-08-06: the tooling this used is gone; the finding is not.**
> `tools/bgo_moves.py`, `tools/bgo_fit.py` and `weighted.evaluate`'s Python
> conditional-logit fit were all deleted with `engine/` and the Python half
> of `experiments/` on 2026-08-06. There is no Rust behaviour-cloning
> pipeline today. The result — human move agreement and playing strength are
> monotonically *anti-correlated* for a linear weighted evaluator, so
> cloning recovers *how* humans play but not *what for* — is a durable
> negative finding about that class of evaluator, not about the deleted
> code, and is worth reading before anyone proposes cloning again against
> the current Rust `WeightedBot` or the neural bot.

Branch: `behaviour-clone`. New: `tools/bgo_moves.py` (move-level replayer),
`tools/bgo_fit.py` (conditional-logit fit), `tests/test_bgo_moves.py` (14
tests). Nothing in `engine/` is touched and `tools/gate.sh` is GATE PASS with
every digest at its master value.

**Timing caveat, and it is a real one.** Every number below was measured on the
engine as of `e9cb000`. While this branch ran, master gained
`4037c17 Fix four scoring bugs against the BGO corpus; all four gate digests
move`. [`docs/SCORE_AUDIT.md`](SCORE_AUDIT.md#103-three-engine-bugs-all-small-none-of-them-the-score-gap) §10.3 sizes those bugs at single-digit culture
per position, so nothing here changes sign or ordering — the smallest gap this
document leans on is 39 points — but the **absolute** culture figures in §3
predate the fix and will shift by a few points if re-run. [`docs/HUMAN_BASELINE.md`](HUMAN_BASELINE.md)'s
159.5 human reference is a journal fact and is unaffected.

The brief was the AlphaGo recipe: supervised learning from human moves first,
self-play improvement second, so hill climbing starts from a competent prior
instead of discovering suppression from scratch. **The supervised half works;
whether the prior is competent depends entirely on which search you run it
under, and the answer flips.** Read §3.1 and §4 before quoting anything.

---

## 0. One-paragraph answer

152,248 human decisions with a non-trivial choice can be reconstructed from the
691 two-player journals with a real `GameState` and the engine's own legal move
list; **90.1% of the moves a human actually played are legal in our
reconstruction**, and 27,181 of the decisions (17.9%) survive a gate that requires our engine to reproduce BGO's printed
production *and* stocks for every seat at the table. Fitting
`weighted.evaluate`'s weight vector to those by conditional logit gives
**36.1% [34.8, 37.8] top-1 move agreement on held-out GAMES**, against 19.0%
for `DEFAULT_WEIGHTS`, 17.4% for our champion, 14.2% for the 1-ply lineage
vector, 11.1% for uniform and 30.3% for the best trivial heuristic. That vector
then **scores 7.4 final culture** in a 2p mirror against a human 159.5 and our
champion's 110.5 at the same search. Across a five-point regularisation sweep
**human move agreement and playing strength are monotonically anti-correlated**:
every step toward the human costs score, over a 36-to-27 point range in move
agreement and a 105-to-7 point range in culture. The most human-like vector
closes the wonder gap **completely** — 2.58 wonders and 8.99 stages per player
against a human 2.74 / 8.77, both CIs overlapping, against our champion's
0.28 / 1.49 — and scores 13. The reason is identifiable and is the most
transferable thing here: **move choice cannot identify a weight on a feature
that does not vary between the candidates of a decision**, and the culture stock
is exactly such a feature, so the clone is a style model that does not know what
the game is for (§4.2). **But the ordering is search-dependent and it flips**:
under `plan:width=8`, the policy we would ship, the most heavily regularised
clone scores **108.3 [98.6, 118.2] against our champion's 69.0 [59.2, 77.8]**,
takes 34.6 civil cards against a human 34.3, and pays 3 CA for 3.1% of them
against a human 4.5% — while the same two vectors are 30.3 ± 5.0 margin the
*other* way at 1 ply (§3.1).

---

## 1. Stage 1: how much supervision is actually there

### 1.1 `tools/bgo_rescore.py` does not produce it, and could not

The brief assumed `bgo_rescore.py` "already replays all 1011 journals into real
`GameState`s". It does not. It rebuilds a **tableau** — workers per card,
government, leader, wonders, colonies, yellow bank — and hands that to
`effects.state_stats` and `events.scoring_culture`. Its `build_state` never
deals a card row, never fills a hand, never sets an action budget and has no
turn structure. That is exactly right for its job (diffing the end-of-game
scorer) and useless for behaviour cloning, which needs a position you can *move*
in.

`tools/bgo_moves.py` is a second replayer with a different contract: for every
human decision, produce a `GameState`, `actions.legal_moves()` on it, and which
of those moves the human played. Three design choices carry it:

* **The engine's turn loop is not used.** `engine.game` deals its own cards,
  draws its own events and resolves its own wars, none of which match the human
  game. The turn loop is local; the only engine code on the critical path is
  `legal_moves` / `apply` / `effects` / `features`, which is the code behaviour
  cloning consumes.
* **Every stock is resynced from BGO at the end of every turn.** The `End turn`
  line prints culture, science, food *and* resources, both the production and
  the resulting stock. `bgo_rescore` reads only the production half.
* **The card row is imputed.** The row is the one thing the journal never
  prints. Cards are dealt from a correctly-composed shuffled age deck; when a
  human takes a card that is not in our row, it is swapped into a **uniformly
  chosen** slot of the civil-action cost BGO logged, and the card it displaced
  goes back to the deck.

### 1.2 What survives, and the honest gate

A turn is **clean** only if, at the end of it, our five production numbers equal
BGO's five printed ones, our four stocks equal BGO's four printed ones *before*
the resync, yellow tokens are conserved, and no line in the turn needed a manual
patch (war, aggression, annex/infiltrate/raid, an unparsed line, or a
reconstructed move our engine calls illegal). Two levels are emitted, tagged, so
the fit can choose: **own-seat clean**, and **whole-table clean** (every seat
clean at its last settle) — the second matters because `features()` reads every
rival's culture, rates and strength.

2p corpus, 691 games, `python3 tools/bgo_moves.py --players 2`:

| | |
|---|---|
| human moves replayed | 171,758 |
| ... **legal in our reconstruction** | **154,725 (90.1%)** |
| decisions with a non-trivial (2+ move) legal list | **152,248** |
| ... own-seat clean | **54,840 (36.0%)** |
| ... whole-table clean | **27,181 (17.9%)** |
| turn snapshots | 24,678 |
| ... production exact (all five) | 69.7% |
| ... stocks exact (all four) | 59.5% |

3p and 4p are worse, as expected — the whole-table gate is a conjunction over
more seats: 3p own-seat 31.7% / table 10.9%, 4p 26.1% / 6.1% (40-game samples).
**Everything below is 2p only.**

### 1.3 It is an early-game-weighted dataset, and you must say so

| rounds | clean turns | production exact | stocks exact |
|---|---|---|---|
| 1-4 | **45.1%** | 98.1% | 74.6% |
| 5-8 | 18.4% | 83.5% | 57.9% |
| 9-12 | 16.3% | 66.0% | 66.2% |
| 13-16 | 9.4% | 53.1% | 57.3% |
| 17-20 | **2.3%** | 36.4% | 33.2% |

Against a uniform-in-round sample the emitted set is ~2x over-weighted on rounds
1-4 and ~4x under-weighted on 17-20. It is *not* an opening-only dataset — 36%
of held-out examples are from round 9 or later — but it is skewed, and §4.3
tests whether that skew is the explanation for §3 (it is not).

### 1.4 Can the drift be cheaply reduced? Partly, and not the part that matters

[`docs/SCORE_AUDIT.md`](SCORE_AUDIT.md#101-method-replay-the-journal-ask-our-engine-diff-against-bgo) §10.1 records `bgo_rescore`'s per-turn agreement as
99.1% on turns 1-5 decaying to 58.1% at turn 16+, and calls that replayer drift.
This replayer resyncs all four stocks from BGO every single turn, which bounds
the *stock* half of the drift to one turn. Like for like (their buckets are
5 turns, ours 4 rounds; at 2p those are close):

| | bgo_rescore all-5 exact | bgo_moves production exact |
|---|---|---|
| early | 99.1% (t1-5) | 98.1% (r1-4) |
| | 82.4% (t6-10) | 83.5% (r5-8) |
| | 67.0% (t11-15) | 66.0% (r9-12) |
| late | 58.1% (t16+) | 53.1% (r13-16), 36.4% (r17-20) |

**The resync does not help the production check at all.** That is the finding:
the residual drift is not in the stocks, it is in the **tableau** — which worker
is standing on which card — and no amount of stock resyncing touches it. The
journal prints every stock and no worker placement, so the only lever left is
better line handling. Four things measurably moved it while this was built, in
descending order of size, and are pinned in `tests/test_bgo_moves.py`:

1. **Card names repeat across ages and the journal prints only the base name.**
   `engine.cards._disambiguate` renames every repeated card, so `Frugality` is
   `Frugality (A)`/`(I)`/`(II)` in the DB and `"Frugality" in db.by_name` is
   False. Before `resolve()`, *every* take of one of the ~15 repeated
   action-card names — the most-taken cards in the game — dirtied its turn.
2. **A yellow action card that ORDERS an action is logged only as a `using`
   clause on the ordered action's own line** (`upgrades Bronze to Iron using
   Rich Land`); there is no `plays Rich Land` line. The ordered action can also
   land on the *next* line (`discovers Riflemen using Breakthrough`).
3. **A colonisation's sacrificed units return their yellow tokens to the BANK**
   (§11.3), not to the unused pool. Missing that cost 1-4 bank tokens per
   colonising player, which moves the consumption band; adding it took
   consumption agreement from 76.4% to 89.6% and clean turns from 27.5% to
   35.5% (own-seat, 60-game sample).
4. **A turn's round comes off its `End turn` row**, not off the row that opened
   it — the opening row is often a cross-player consequence line from the
   previous round, and keying on it interleaves the seats wrongly.

Residual illegal-move rate by kind (9.9% overall): `take` 7,434, `build` 1,901,
`pop` 1,858, `play_action` 1,474, `develop` 1,304, `upgrade` 1,284,
`wonder_step` 1,098. Most of these are one CA or one resource short, i.e. the
same tableau/budget drift. An illegal move is still forced onto the tableau by
hand (`_force`) so that one bad reconstruction does not poison the rest of the
game; its turn is dirty either way.

### 1.5 What the row imputation costs, stated plainly

18,509 of 51,561 takes (35.9%) hit a card we had already dealt; 8,044 (15.6%)
hit it at the right cost tier. The rest are injected. Consequences:

* the human's own move is always available, so no example is lost;
* some counterfactual takes are cards the human never saw;
* which slot inside a tier is a guess. **This was the single worst methodology
  bug in this work**: injecting at the leftmost slot of the tier was worth ~9
  points to a "take the leftmost legal card" baseline (43.5% → 30.3%), purely as
  an artefact of where the replayer chose to put the card, and it was enough to
  make that baseline beat the fitted vector. Uniform-in-band injection is now
  pinned by a test.

---

## 2. Stage 2: fitting the weights to predict the human

### 2.1 Why a conditional logit and not hill climbing

`evaluate` is linear in the weights over 64 features, 20 phase copies and
`end_turn_bias`, so "make the argmax agree with the human" is a convex softmax
over candidate scores with a closed-form gradient. Argmax agreement itself is
piecewise constant — the one objective gradient descent cannot use and hill
climbing is worst at — and the log-loss is a proper scoring rule, so a vector
that ranks the human's move second everywhere beats one that ranks it last,
which argmax agreement cannot see.

The four terms `evaluate` prices through `w` itself (`hand_potential`,
`rival_hand_potential`, `row_urgency`, `row_bargain_forgone`) are **not** linear
in the weights. They are emitted priced through `DEFAULT_WEIGHTS` and fitted as
ordinary scales. That linearisation is approximate — the shipped file prices
them through the *fitted* `w` — and it is the one place where the model in
`bgo_fit.py` is not exactly the model in `weighted.py`. Without it the evaluator
is completely card-identity-blind and every take in a tier is byte-identical
([`docs/WASTED_ACTIONS.md`](WASTED_ACTIONS.md#4-the-yellow-card-question-specifically) §4).

**Split by GAME, never by position.** 547 training games, 136 held-out games,
26,991 examples at the whole-table gate. A dev slice is carved out of the
*training* games for early stopping; the test games are never scored during
fitting.

### 2.2 Held-out move-match, against baselines that mean something

136 held-out games, 4,656 decisions, first-index tie-break (the same rule
`WeightedBot.pick` uses, so ties are not flattered). CIs are a cluster bootstrap
over games.

| | top-1 | top-3 | right move *kind* | logloss |
|---|---|---|---|---|
| uniform over legal moves | 0.1109 | | | |
| always `end_turn` | 0.2090 | | | |
| always `build`, else `end_turn` | 0.1654 | | | |
| always `pop`, else `end_turn` | 0.2878 | | | |
| **take leftmost legal card, else `end_turn`** | **0.3030** | | | |
| `DEFAULT_WEIGHTS` | 0.1896 [0.176, 0.208] | 0.407 | 0.301 | 5.76 |
| **our champion, gen 239** | **0.1742** [0.165, 0.185] | 0.377 | 0.341 | 15.84 |
| the 1-ply lineage vector (P) | 0.1422 [0.130, 0.159] | 0.394 | 0.250 | 14.73 |
| **cloned vector** | **0.3608** [0.348, 0.378] | **0.629** | 0.436 | 1.98 |

Three things worth keeping:

* the clone is **2.07x our champion** and 3.3x uniform, and beats every trivial
  heuristic;
* **our champion is a worse human-move predictor than the untuned default**
  (0.174 vs 0.190) and the 1-ply lineage vector is worse still (0.142) — 200+
  generations of margin-gated hill climbing moved our policy *away* from human
  play, which is the same finding [`docs/HUMAN_BASELINE.md`](HUMAN_BASELINE.md) reaches from
  behaviour statistics, now measured move by move;
* the champion's and P's log-losses (15.8, 14.7) are catastrophic next to the
  clone's 1.98 — they are not merely wrong about the top move, they are
  confidently wrong.

### 2.3 Where the agreement lives, and where the ceiling is

| human move | n | clone top-1 |
|---|---|---|
| `end_turn` | 709 | **0.827** ± 0.014 |
| `play_leader` | 229 | **0.856** ± 0.023 |
| `pop` | 372 | 0.648 ± 0.025 |
| `pol_pass` | 264 | 0.511 ± 0.031 |
| `wonder_step` | 184 | 0.489 ± 0.037 |
| `develop` | 267 | 0.337 ± 0.029 |
| `upgrade` | 230 | 0.261 ± 0.029 |
| `build` | 364 | 0.209 ± 0.021 |
| `play_action` | 316 | 0.152 ± 0.020 |
| **`take`** | **1,642** | **0.091** ± 0.007 |

`take` is 35% of all human decisions and the model is at chance on it (there are
~10 legal takes in a typical position). **The evaluator has no channel that can
tell one card in the row from another** beyond the `hand_potential`
linearisation, and behaviour cloning cannot invent one — this is a feature gap,
not a fitting gap, and it caps top-1 at roughly 0.36 whatever you do. That is
the single highest-value thing to fix if anyone wants a better human model:
[`docs/WASTED_ACTIONS.md`](WASTED_ACTIONS.md#4-the-yellow-card-question-specifically) §4 predicted it and this measures it.

By round, held out: 0.436 (r1-4), 0.332 (r5-8), 0.269 (r9-12), 0.310 (r13-16),
0.315 (r17-20) — it degrades but does not collapse outside the opening.

---

## 3. Stage 3: does the cloned vector play better? No. It plays far worse.

`tools/bgo_botmatch.py --players 2 --games 60 --seed 7000`, 2p mirror, the same
protocol and seeds as [`docs/SCORE_AUDIT.md`](SCORE_AUDIT.md#105-the-score-gap-is-a-property-of-the-vector-not-of-the-engine) §10.5 — which this run reproduces
exactly on the two reference vectors (Q at 1 ply 110.48 here vs 110.5 there;
P at 1 ply 139.80 vs 139.8), so the harness is measuring what it claims to.

All rows are 1-ply `weighted` unless stated. `l2` is the strength of an L2
penalty toward `DEFAULT_WEIGHTS` (§4.2); `l2 -> 0` is the pure clone.

| vector | held-out top-1 | **final culture** | wonders | stages | civil takes | % takes at 3 CA | wars/game | first gov |
|---|---|---|---|---|---|---|---|---|
| **human corpus** | — | **159.5** [156.0,163.0] | **2.74** | **8.77** | **34.3** | **4.51** | **0.51** | **11.8** |
| clone, `l2 -> 0` | **0.361** | **7.4** [4.6,10.3] | 2.26 | 7.36 | 27.0 | 0.20 | 0.00 | 11.1 |
| clone, anchored l2=0.003 | 0.359 | 10.4 [7.3,13.8] | 2.54 | 8.44 | 27.3 | 0.61 | 0.00 | 12.2 |
| clone, anchored l2=0.01 | 0.353 | 12.6 [9.0,16.4] | 2.45 | 8.41 | 26.7 | 0.58 | 0.00 | 12.8 |
| clone, anchored l2=0.03 | 0.343 | 22.8 [18.7,26.9] | 2.04 | 7.96 | 25.1 | 0.78 | 0.00 | 12.5 |
| clone, anchored l2=0.1 | 0.321 | 39.0 [33.3,45.2] | 1.43 | 7.00 | 25.4 | 0.66 | 0.00 | 13.0 |
| clone, anchored l2=0.3 | 0.271 | 104.6 [97.3,112.2] | 1.24 | 5.38 | 22.6 | 1.53 | 0.00 | 12.8 |
| `DEFAULT_WEIGHTS` | 0.190 | 102.6 [95.8,109.2] | 0.01 | 0.03 | 11.3 | 20.0 | 0.00 | 10.1 |
| Q champion, 1 ply | 0.174 | 110.5 [104.8,116.5] | 0.28 | 1.49 | 25.4 | 22.0 | 0.00 | 8.3 |
| P 1-ply lineage | 0.142 | **139.8** [131.6,148.3] | 0.76 | 3.12 | 22.9 | 23.2 | 0.00 | 8.8 |
| Q champion, `quiesce:levels=1` | — | 64.7 (docs/SCORE_AUDIT.md §10, the former SCORE_VALIDATION) | 0.41 | 1.86 | 22.2 | 22.3 | 0.98 | — |

### 3.1 Under the ship policy the ordering is different, and the clone beats the champion

[`docs/TRANSFER_TEST.md`](TRANSFER_TEST.md) is the standing warning that a conclusion drawn under
one search need not hold under another, so all three searches were run. 2p
mirror, same seeds; the clone row is n=60, the two reference rows n=30 (PlanBot
is expensive and the box was carrying three live league arms).

| `plan:width=8`, 2p mirror | **final culture** | wonders | stages | civil takes | % at 3 CA | wars |
|---|---|---|---|---|---|---|
| **human corpus** | **159.5** [156.0,163.0] | 2.74 | 8.77 | 34.3 | 4.51 | 0.25 |
| P 1-ply lineage | **189.0** [176.5,202.4] | 0.70 | 2.43 | 24.9 | 13.1 | 0.77 |
| **clone, anchored 0.3** | **108.3** [98.6,118.2] | 1.78 | 6.66 | **34.6** | **3.08** | 0.69 |
| Q champion | **69.0** [59.2,77.8] | **0.00** | **0.00** | 18.1 | 29.5 | 1.03 |

Two things here are worth more than the rest of this document.

* **Under the policy we would actually ship, the cloned vector produces 39.3
  more culture than our champion with CIs nowhere near each other** (108.3 vs
  69.0), having lost to it by 30.3 ± 5.0 margin at 1 ply and by 70.0 ± 4.1
  under quiescence. Head to head under PlanBot the sign flips too, though at
  n=20 deals that row is a null on its own (+16.5 ± 9.7; see the table below).
  This is [`docs/TRANSFER_TEST.md`](TRANSFER_TEST.md)'s interaction again, from a third vector
  family: *which vector is better is a question about the search.*
* **The 1-ply lineage vector under PlanBot scores 189.0, above the human 159.5**,
  and its CI is clear of the human one. As far as this repo has measured,
  that is the first bot configuration that beats the human mean on score —
  and it does it with 0.70 wonders and 24.9 cards, i.e. by playing nothing like
  a human. (n=30; it wants replicating before anyone leans on it.)

The clone's *behaviour* under PlanBot is the closest to human this project has
produced. Five axes have overlapping CIs with the human corpus — wonders
started (2.76 vs 2.78), government changes (1.07 vs 1.14), round of first
government (11.3 vs 11.8), **civil cards taken (34.6 vs 34.3)** and age II/III
takes — and the 3-CA rate is 3.08% against a human 4.51% and the champion's
29.5%. [`docs/HUMAN_BASELINE.md`](HUMAN_BASELINE.md) finding 2 ("a smaller civil-action budget spent
impatiently") is, on this vector under this search, simply gone.

Other searches on the cloned vector:

| | final culture | wonders | stages | civil takes |
|---|---|---|---|---|
| clone `l2 -> 0` under `quiesce:levels=1` | 9.7 [7.0,12.6] | 2.20 | 7.10 | 25.7 |
| clone anchored 0.3 under `quiesce:levels=1` | 98.7 [91.1,107.0] | 1.26 | 5.22 | 24.3 |
| clone anchored 0.3 under `plan:width=8` | 108.3 [98.6,118.2] | 1.78 | 6.66 | **34.6** |

Note also that at 1 ply the clone's own culture (104.6 [97.3,112.2]) and the
champion's (110.5 [104.8,116.5]) barely differ — the champion's 30.3-point
head-to-head margin over it is *transfer*, not production, which is the same
mechanism [`docs/TWOP_PROFILE.md`](TWOP_PROFILE.md#9-what-this-does-and-does-not-support) §9 and [`docs/TRANSFER_TEST.md`](TRANSFER_TEST.md#5-why-the-two-vectors-are-different-animals) §5 describe.

Head to head, `tools/transfer_ab.py`, n over deals, ± one SE. `duel` rotates the
challenger through both seats on the same seed, so the deal is the independent
unit:

| clone (anchored 0.3) vs Q champion | n deals | margin | win share (null 0.500) |
|---|---|---|---|
| under `weighted` (1 ply) | 50 | **−30.3 ± 5.0** | 0.220 ± 0.046 |
| under `quiesce:levels=1` | 50 | **−70.0 ± 4.1** | 0.040 ± 0.019 |
| under `plan:width=8` | 20 | **+16.5 ± 9.7** | 0.575 ± 0.083 |

**Read that last row carefully.** On its own it is not a win: +16.5 ± 9.7 is
1.7 SE from zero and the win share is 0.9 SE from the null, i.e. at n=20 deals
this is "not distinguishable from even", not "the clone is better". What *is*
unambiguous is the **interaction**: the same two vectors, on the same engine,
differ by 86.5 ± 10.6 margin points between the quiescence row and the PlanBot
row — eight standard errors. This is [`docs/TRANSFER_TEST.md`](TRANSFER_TEST.md)'s finding
reproduced on a third vector family, and it is the reason no `--init` is
proposed here without naming the search first (§6).

Against a common opponent (`book`, 40 deals, paired by deal):

| | vs `book` margin | win share |
|---|---|---|
| clone (anchored 0.3) | −28.4 ± 8.7 | 0.362 ± 0.051 |
| clone (pure) | −44.0 ± 4.9 | 0.156 ± 0.037 |
| Q champion | +66.5 ± 5.2 | 0.925 ± 0.029 |
| **paired clone(0.3) − Q** | **−94.9 ± 10.2** | −0.562 ± 0.054 |

**The cloned vector loses to `book`.** That is not a marginal result and it is
not a noise result: 9 SE from the null on the paired difference.

### 3.2 The one thing the clone does that nothing else we have does

The wonder gap is [`docs/HUMAN_BASELINE.md`](HUMAN_BASELINE.md)'s largest and most consistent
finding — 6.9x fewer wonders, 4.6x fewer stages, holding at 3p and 4p, and
untouched by everything tried since. **The clone closes it completely.**

| | wonders/player | stages/player |
|---|---|---|
| human | 2.74 [2.69, 2.79] | 8.77 [8.60, 8.94] |
| clone, round-reweighted (§4.3) | **2.58** [2.42, 2.72] *(overlap)* | **8.99** [8.53, 9.41] *(overlap)* |
| clone, anchored 0.003 | 2.54 [2.43, 2.67] | 8.44 [8.09, 8.82] |
| Q champion | 0.28 [0.21, 0.37] | 1.49 [1.26, 1.75] |

It also fixes the 3-CA impatience — 0.6-1.0% of takes at 3 CA against a human
4.5% and our champion's 22.0% — though it now overshoots to the *patient* side,
and takes 27 civil cards against a human 34.3 and our champion's 25.4.

And it scores 10.

**At 1 ply this is direct evidence against the story that the wonder deficit and
the impatience deficit are the score gap.** [`docs/SCORE_AUDIT.md`](SCORE_AUDIT.md#1062-the-scripted-ab-forcing-wonders) §10.6.2 reached
the same place by forcing wonders on a strong bot and finding it cost 34.3 ± 7.0
margin. This reaches it from the opposite direction: a bot that arrives at human
wonder counts *by choice*, because it was fitted to human move choices, scores
7-13. Two independent methods, same answer. [`docs/HUMAN_BASELINE.md`](HUMAN_BASELINE.md) findings 1
and 2 are real descriptions and are **not** the mechanism of finding 5.

Where the clone is still nothing like a human: it never declares a war
(0.00/game vs 0.51), never plays an aggression, and **never bids for a colony**
(0.00 vs 3.22 bids/player). The last one is an artefact of §1.2, not of the fit:
auctions dirty their turn, so almost no bid decision reaches the training set
and the vector has never been told that colonies exist. That is a known,
fixable hole, not a discovery.

---

## 4. Why it plays badly, which is the useful part

### 4.1 It is not a bug in the pipeline

The harness reproduces [`docs/SCORE_AUDIT.md`](SCORE_AUDIT.md#105-the-score-gap-is-a-property-of-the-vector-not-of-the-engine) §10.5 to a decimal on two
reference vectors, 0 engine errors in every run, games run 21.5 rounds (not
short), and the clone's *behaviour* is human on the axes it was fitted on.
The vector does what it was asked to do.

### 4.2 Move choice does not identify the weights that decide the game

Here is the whole mechanism. A weight is identified by move data only if the
feature it multiplies **varies between the candidates of a decision**. Several
of the most important ones barely do. A player's culture *stock* is the same
number whichever of this turn's moves they make — it changes only when a wonder
completes or a card pays out — so the log-likelihood is nearly flat in `culture`
and an L2-to-zero penalty drives it to zero. Fitted values against defaults:

| | fitted (pure clone) | default |
|---|---|---|
| `culture` | **−0.061** | 1.000 |
| `culture_rate` | **0.431** | 5.000 |
| `culture_late` | −0.139 | 1.500 |
| `uprising` | −1.719 | −12.000 |
| `discontent` | −0.617 | −3.000 |
| `take_cost_paid` | **+0.573** | 0.000 |
| `consumption` | **+0.779** | −0.500 |
| `wonders` | 7.885 | 3.000 |
| `end_turn_bias` | −5.585 | −3.000 |

The clone learned that humans build wonders (`wonders` 7.9), keep acting rather
than passing (`end_turn_bias` −5.6) and elect leaders — and it did not learn
that **culture wins the game**, because no human move it ever saw was a decision
about the culture stock. It is a style model. Behaviour cloning on move choice
recovers *how* to play and cannot recover *what for*.

The direct test: take the anchored-0.01 clone and restore **only** the six
culture weights from `DEFAULT_WEIGHTS`, changing nothing else.

| | final culture | wonders | stages | civil takes |
|---|---|---|---|---|
| clone anchored 0.01 | 12.6 | 2.45 | 8.41 | 26.7 |
| **same + `DEFAULT` culture terms** | **100.2** | 0.93 | 3.02 | **8.4** |

Six weights are worth 88 culture. But look at the other columns: the graft also
takes wonders from 2.45 to 0.93 and civil cards taken from 26.7 to **8.4**, and
pushes first government to round 17. Within this linear evaluator the human
style and the culture objective are in genuine tension, and you cannot have
both by editing weights. *(Caveat: the fitted vector's overall scale is set by
the softmax temperature and is not commensurate with `DEFAULT_WEIGHTS`'s, so
this graft is scale-inconsistent and the numbers are directional, not exact.
It is a demonstration of the mechanism, not a candidate vector.)*

Anchoring the regulariser on `DEFAULT_WEIGHTS` instead of on zero — so
unidentified directions stay where the prior put them — is the principled
version of the same idea, and it produces the monotone trade-off curve in §3
rather than a free lunch. There is no setting on that curve that is both
human-like and strong.

### 4.3 It is not the early-game skew either

§1.3 shows the dataset is ~2x over-weighted on rounds 1-4. The obvious
hypothesis is that the clone is an opening book being asked to play 21 rounds.
`bgo_fit.py --flat-rounds` reweights every example so the round distribution is
flat, which is a direct test:

| | held-out top-1 | final culture |
|---|---|---|
| anchored 0.01, as-sampled | 0.3533 | 12.6 [9.0,16.4] |
| anchored 0.01, **flat rounds** | 0.3540 | **13.4** [9.8,16.7] |

No effect on either. The skew is real and it is not the explanation.

---

## 5. What this says about the gate metric

The brief asked for a read on [`docs/TRANSFER_TEST.md`](TRANSFER_TEST.md) §8.3(c) — gate the league
on own culture rather than margin. **That decision was taken while this branch
was running**: the now-deleted `docs/LEAGUE_OBJECTIVE.md` (Python-era, git
history) switched the accept criterion to own final culture with win share as
a tiebreak. So this is corroboration, not a
recommendation, and it is worth recording because it is independent of the
1,632-game rescoring that gated that change.

The anchored clone and the champion are ranked **oppositely** by the two
metrics, and not marginally. Own culture in a mirror under `plan:width=8`:
clone 108.3 [98.6,118.2], champion 69.0 [59.2,77.8] — 39 points apart with
clear CIs. Margin head to head: the champion beats the clone by 30.3 ± 5.0 at
1 ply and 70.0 ± 4.1 under quiescence. At 1 ply the two produce almost the same
own culture (104.6 vs 110.5, overlapping) and **the champion's entire margin
over the clone is transfer, not production** — which is the same arithmetic
the now-deleted `docs/LEAGUE_OBJECTIVE.md` §1 made explicit, showing up in a
pair of vectors neither of us designed for the purpose.

One caution in the other direction, since this document is not in the business
of only confirming things. A gate on own culture would rank the *pure* clone
(7.4 own culture, 0 margin, opponent held to the same 7.4) as terrible, which
is right — but so would the margin gate, which also sees a null there. The pure
clone is a **zero-margin, zero-culture** policy and neither metric distinguishes
it from a strong mirror. And the mechanism found here is an **unidentified
weight, not an overpaid one**, so nothing in this document says the gate change
fixes anything by itself.

## 6. What to do with this, and what NOT to do

Stated as findings, not instructions. **No training was restarted and no gate
metric was changed.**

1. **Do not warm-start a league arm from the pure clone.** It loses to `book` by
   44.0 ± 4.9 margin at a 15.6% win share. A hill climber starting there is
   starting below the pool, and [`docs/HAZARDS.md`](HAZARDS.md)'s trap 5 is the reminder
   that a bad `--init` is worth many wasted generations. Stage 4 of the brief
   was conditional on stages 1-3 landing; stage 3 landed only under one of the
   three searches, so **no `--init` file is committed and no training was
   restarted.** What a `--init` proposal would have to clear, stated so that
   someone can decide rather than guess:
   * the vector to use is `anchor=DEFAULT_WEIGHTS, l2=0.3` (§3), reproducible
     in about four minutes from the emitted data;
   * it beats the live champion by 39.3 culture in a PlanBot mirror and loses
     to it by 30.3 ± 5.0 margin at 1 ply, so the gate for warm-starting it is
     **which search the arm trains under**. Under a `plan:width=1` arm
     ([`docs/TRANSFER_TEST.md`](TRANSFER_TEST.md) §8.3a) this is a candidate; under the current
     `quiescent:levels=1` arms it is not, and the −70.0 ± 4.1 head-to-head
     under exactly that policy is why.
   * a pre-registered kill condition is worth having either way: if a warm
     start from it has not passed the champion on the arm's own gate metric
     within ~20 generations, it is not a better basin, it is a worse one with
     a nicer profile.
2. **The wonder finding is the asset, and it plugs into [`docs/HUMAN_BOTS.md`](HUMAN_BOTS.md).**
   We now have a vector that reaches human wonder counts *by choice* rather
   than by being forced (`tools/wonder_ab.py`), and it costs four minutes to
   reproduce from the emitted data. [`docs/HUMAN_BOTS.md`](HUMAN_BOTS.md) built the human tier
   of the pool by fitting hand-written archetypes' knobs to corpus statistics;
   this is the same target reached from the opposite direction — a vector
   fitted to human *moves*, whose statistics then come out human — and it
   would drop into that tier as a pool opponent with no new machinery, forcing
   the champion to beat something that builds three wonders a game. It is also
   the right vector for pricing a *competent* wonder policy, which
   [`docs/SCORE_AUDIT.md`](SCORE_AUDIT.md#1063-what-this-does-and-does-not-license) §10.6.3 names as the obvious follow-up and which the
   crude forcing override could not test.
3. **The take feature gap is the highest-value fix for any future human model.**
   35% of human decisions are takes and the evaluator is at chance on them.
   Until `features()` can distinguish two cards in the row, top-1 is capped near
   0.36 and no amount of data or optimiser moves it.
4. **If you want a human prior that plays, the objective has to contain the
   outcome.** The AlphaGo analogy breaks at a place worth naming: AlphaGo's
   supervised network predicted moves *and* a separate value network predicted
   the outcome, and the policy net was then improved by self-play against a
   result. Cloning move choice into a *value* function is a category error —
   the value function's job is to rank positions and the move data only
   constrains it up to the directions in which candidate positions differ. The
   fixable version is to fit the same vector against **game outcome** on the
   same reconstructed corpus (the journals carry every player's final score),
   i.e. a value-regression rather than a policy-clone. [`docs/BGO_CORPUS.md`](BGO_CORPUS.md)
   already names that dataset and this branch now provides the replay that
   makes per-turn positions available. That is the experiment this one implies.
5. **The colony hole is worth 30 minutes.** Auctions dirty their turn, so the
   clone has never seen a bid and never makes one. Modelling the auction (the
   journal prints every bid and every sacrificed unit) would both raise the
   clean-turn rate and remove one obviously-wrong behaviour.

---

## 7. Reproducing

```
tar xzf sources/bgo/journals.tar.gz -C /tmp/bgo
python3 tools/bgo_parse.py --journals /tmp/bgo/journals \
    --index sources/bgo/index.tsv --out /tmp/human.tsv

# Stage 1
python3 tools/bgo_moves.py --players 2

# Stage 1 -> 2: extract (about 30 min at 2p, sharded three ways)
for i in 0 1 2; do
  nice -n 19 python3 tools/bgo_moves.py --players 2 --shard $i/3 \
      --emit /tmp/bc2p_$i.jsonl &
done; wait

# Stage 2
python3 tools/bgo_fit.py --data "/tmp/bc2p_*.jsonl" --epochs 8 \
    --out /tmp/clone_pure_2p.json \
    --compare "Q_champion=/tmp/Q2p.json" --compare "P_1ply=/tmp/P2p.json"
for l2 in 0.003 0.01 0.03 0.1 0.3; do
  python3 tools/bgo_fit.py --data "/tmp/bc2p_*.jsonl" --epochs 6 \
      --l2 $l2 --anchor default --out /tmp/clone_a$l2.json
done

# Stage 3
nice -n 19 python3 tools/bgo_botmatch.py --players 2 --games 60 --seed 7000 \
    --spec /tmp/clone_a0.3.json --out /tmp/pm.tsv
python3 tools/bgo_stats.py --tsv /tmp/human.tsv --vs /tmp/pm.tsv --players 2
nice -n 19 python3 tools/transfer_ab.py h2h --policy weighted --deals 50 \
    --a /tmp/clone_a0.3.json --b /tmp/Q2p.json
nice -n 19 python3 tools/transfer_ab.py vsfield --policy weighted --deals 40 \
    --a /tmp/clone_a0.3.json --b /tmp/Q2p.json
```

Everything ran `nice -n 19` alongside three live league arms (five workers) on a
6-core box.

## 8. Limits

* **2p only.** 3p/4p fidelity was measured (§1.2) and nothing was fitted or
  played there.
* **The row is imputed** (§1.5). 64% of takes present the human's card against
  alternatives they may never have seen. This biases the `take` numbers in an
  unknown direction and is the weakest joint in the whole pipeline.
* **The military hand is imputed too.** Identities are unknown, so `hand_mil_value`
  is approximately right (the age is right) and no better.
* **Happy faces are unverifiable** — as [`docs/SCORE_AUDIT.md`](SCORE_AUDIT.md#108-limits) §10.8 says, the
  journal never prints them, so the gate cannot check that half of the tableau.
* **The four non-linear terms are linearised** (§2.1). The shipped weight file
  prices them through the fitted `w`, not through `DEFAULT_WEIGHTS`, so a
  cloned vector's behaviour is not exactly the model that was fitted.
* **n = 60 games per Stage 3 row, n = 30 for the two PlanBot reference rows,
  40-50 deals per head-to-head.** The 40-to-100-point score differences are
  many SE and safe; the differences *between* adjacent rows of the
  regularisation sweep (10.4 vs 12.6) are not, and only the monotone trend and
  the endpoints should be leaned on. **The PlanBot duel is n=20 deals** — that
  configuration costs ~1 min/game on this box under load — and at that size
  +16.5 ± 9.7 is a null on its own; it wants roughly 4x the deals to resolve.
  §3.1's PlanBot comparison therefore rests on two mirror matches on the same
  seeds, which shows the clone *produces* more culture than the champion, not
  that it beats it. Resolving that duel is the first thing to run next.
* **The move-match number is not a skill measurement.** It is agreement with a
  Prince-to-Emperor club population, on positions our replayer could verify,
  which is not the same population as "on all positions" — the clean gate
  selects turns where nothing complicated happened, and complicated turns are
  plausibly where the interesting decisions are.
