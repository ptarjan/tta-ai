# Why the trained champion does not beat the book bots at 4 players

Date: 2026-07-27. Diagnosis only — **nothing is fixed here.**

Brief: at 4p the gen-99 champion scored `book` 18.8% / `book2` 18.8% against a
25% null (n=48), while at 2p the same trainer beats `book` 97.9%. The
architecturally different 1-ply run reached the same place, so the failure
predates the 1-ply → quiescent switch. Question: structural, training
pathology, or just distance-to-go?

**Answer: step-1 outcome 2 — 4p training is converging somewhere actively
bad.** At a *matched* generation count the 2p arm's vector plays 4-player TtA
more than twice as well as the 4p arm's does. Headline numbers, all vs `book`
at 4 players, n=400 each, null 25%:

| vector | win rate | culture margin |
|---|---|---|
| `ladder_2p/gen00099` (2p arm, 99 gens) | **57.4% ± 2.5%** | +70.8 |
| `champion_2p.json` (2p arm, 180 gens) | **57.9% ± 2.5%** | +54.6 |
| `champion_3p.json` (3p arm, 199 gens) | 33.6% ± 2.4% | +33.9 |
| `champion_4p.json` (4p arm, 99 gens) | 27.6% ± 2.2% | +20.8 |

Contents: §0 corrects the brief's numbers; §1 is the step-1 2×2 and a metric
caveat; §2 is the matched-generation control and one refuted weight-level lead;
§3 is what the mechanism is *not*; §4 is the recommended next action.

All measurements below are `TTA_JOURNAL=1`, seat-rotated, `n=400` per arm,
challenger wrapped in `QuiescentBot(levels=1)` — exactly the architecture the
live arms train under (`run_league.sh ... --candidate-bot quiescent:levels=1`),
so nothing here is confounded by an architecture mismatch. **Error bars are
standard errors, not 95% CIs.** The null at 4p is 25.0%; at 2p it is 50.0%.
Champion vectors were snapshotted to `/tmp/fourp/` at 06:54 so the live
trainers could not move the target mid-run; the 4p snapshot is the gen-95
accepted vector (the champion file reads `gen 99, since_accept 4`), which is
the same vector the gen-100 full check scored.

---

## 0. First: the brief's numbers are stale, and the full-check column is unreadable at 4p

The eleven win rates in the brief are **byte-identical to the `gen 90` record**
of `experiments/league_state/fullcheck_4p.jsonl` (written 06:15:34), not to
gen 99. That record scored the **gen-89** accepted vector. Two accepts landed
after it — `gen 94` (`op: scatter`, edge +0.1204) and `gen 95` (`op: kick`,
edge +0.0951) — and the next full check, `gen 100` at 06:55:13, scored the
resulting vector at **book 50.0%, book2 43.8%, margin +60.8**.

So the log appears to say the champion went 18.8% → 50.0% against `book` in
seventeen minutes. I measured **both** vectors at `n=400`, seat-rotated:

| vs `book` @4p | full check, n=48 | this run, n=400 |
|---|---|---|
| gen-89 vector (the brief's row) | 18.8% ± 5.6%, margin +10.7 ± 13.2 | **14.9% ± 1.8%, margin −8.9** |
| gen-95 vector (the live champion) | 50.0% ± 7.2%, margin +60.8 ± 13.2 | **27.6% ± 2.2%, margin +20.8 ± 4.6** |

Two things fall out, and they point in opposite directions:

* **The brief's headline is directionally right for the vector it measured.**
  14.9% ± 1.8% against a 25% null, at a *negative* culture margin. If anything
  the n=48 read flattered it.
* **It is stale, and the 50.0% that replaced it is a fluke.** The champion has
  moved twice since (`gen 94`, `gen 95`) and the move was real — +12.7 win
  points (4.5σ) and +29.7 culture points (3.6σ) on n=400 apiece — but it landed
  at 27.6%, not 50.0%. The gen-100 full check sits 3.1σ high on win rate and
  3.0σ high on margin.

At 4p the per-game culture margin has a standard deviation of **91.4 points**
(measured, n=400), so n=48 buys ±13 culture points and ±5.6–7.2 win-rate
points. **The 4p full-check columns cannot resolve anything smaller than a
20-point move and must not be read generation-to-generation in either
direction.** That is not a new class of error for this repo —
`docs/CULTURE_GAP.md` §0 opens by correcting a brief that quoted a stale n=48
full check, and its §4 labels its own n=48 counterfactuals as nulls.

The gate tier's *margin* column is the usable one and it tells a consistent,
monotone story: −137 (gen 10) → −57 → −68 → −27 → −17 → −26 → −30 → −5 → +11 →
+61 (gen 100; +21 by my n=400 remeasure). The 4p arm has been improving the
whole time. That is a different question from whether it is improving *towards
the right place*, which is section 2.

---

## 1. Step 1 — the decisive test, and it is outcome 2

The brief's step 1 was: play the **2p-trained** vector at 4p against `book`.
I ran the 2×2 — both trained vectors at both player counts, against both book
bots — because the 2p cell turns out to be the load-bearing one.

Every arm below is `n=400` at 4p and `n=200` at 2p, seat-rotated, challenger
under `QuiescentBot(levels=1)`, defenders unwrapped (exactly as the trainer
plays them).

**Win rate vs `book` (null 25% at 4p, 50% at 2p):**

| vector | at 2 players | at 4 players |
|---|---|---|
| `champion_2p.json` (gen 180) | **97.5% ± 1.1%** | **57.9% ± 2.5%** |
| `champion_4p.json` (gen 99 = the gen-95 vector) | 56.0% ± 3.5% | 27.6% ± 2.2% |

**Culture margin (challenger − mean defender), same games:**

| vector | at 2 players | at 4 players |
|---|---|---|
| `champion_2p.json` | **+85.1** | **+54.6** |
| `champion_4p.json` | +10.7 | +20.8 |

Replicated against a second, independent opponent (`book2`, n=400 at 4p):
2p vector **59.1% ± 2.5%** (margin +56.1), 4p vector 32.6% ± 2.3% (margin
+36.1). Same ordering, same size.

The 3p vector is in between — **33.6% ± 2.4%** at 4p (margin +33.9), also
ahead of the 4p vector. Full ranking at 4 players against `book`, every arm's
live champion, n=400 each:

| vector | generations | win rate @4p | margin @4p |
|---|---|---|---|
| `champion_2p.json` | 180 | **57.9% ± 2.5%** | +54.6 |
| `champion_3p.json` | 199 | 33.6% ± 2.4% | +33.9 |
| `champion_4p.json` | 99 | 27.6% ± 2.2% | +20.8 |

Note the ranking is **not** monotone in generation count: the 3p arm has the
most generations of the three and is 24 points behind the 2p arm at 4p. So
"the 4p arm just needs more generations" is not the whole story even before
the matched-generation control below.

Because both arms of each 4p comparison were run on the same seed set they are
paired game-for-game against byte-identical opposition:

| paired, 2p vector − 4p vector, at 4p | win share | culture margin |
|---|---|---|
| vs `book` (n=400) | **+0.3025 ± 0.0331** (z=9.1) | **+33.8 ± 7.0** (z=4.9) |
| vs `book2` (n=400) | **+0.2650 ± 0.0342** (z=7.8) | **+20.0 ± 7.3** (z=2.8) |

**Harness validation.** The 2p vector at 2p vs `book` reads 97.5% ± 1.1% on my
driver against the trainer's own 95.8–100% over the last six 2p full checks
(the brief's 97.9% is the `gen 170` record) — so the measurement path
(`/tmp/fourp/duel.py`, which is `experiments/arena.duel` plus `bookmatch`'s
`make_bot` patch) reproduces the number the brief quoted, on a different seed
set. Zero engine errors in every arm.

### What that table says

This is **step-1 outcome 2, in its strongest form.** The 2p vector, which has
never seen a 4-player game, beats `book` at 4 players more than twice as often
as the vector that was trained on nothing else — on the same deals, against the
same opponents, under the same search.

It also kills the brief's framing. "The player count is the variable" is not
what the 2×2 shows. The 4p vector is *also* the weaker player at 2 players
(56.0% vs 97.5%, margin +10.7 vs +85.1). The 4p arm has not produced a
4-player specialist that happens to be mediocre; it has produced a **weak
policy at every player count**, and the 2p arm's vector dominates it at the 4p
arm's own game. The variable is the arm, not the table size.

### A reporting caveat that is real but is not the story

Win share is a much flatter function of playing strength at 4p than at 2p, so
the 4p column of any report *looks* far worse than the same amount of skill
does at 2p. Pooled over every gate-tier opponent in every full check ever
logged in this run and the archived 1-ply run (704 opponent-checks):

| culture margin band | 2p win rate | 3p win rate | 4p win rate |
|---|---|---|---|
| −20 … 0 | 0.431 | 0.258 | 0.157 |
| 0 … +20 | 0.568 | 0.407 | 0.208 |
| +20 … +40 | 0.688 | 0.491 | 0.297 |
| +40 … +60 | 0.800 | 0.632 | 0.414 |
| +60 … +90 | 0.919 | 0.722 | 0.500 |

A margin of +85 buys ~97% at 2p and ~50% at 4p, because at 4p you have to beat
the *best* of three draws, not the mean of one. `docs/LEAGUE_TRAINING.md`
already measured the extreme of this ("at 4p win share cannot tell that a bot
with every weight set to zero is worse than the champion at all") and it is why
the gate tier is scored on margin. So some of the apparent 2p-vs-4p
catastrophe is metric shape.

**But it does not explain this finding**, because the paired *margin* test is
metric-independent and still gives the 2p vector +33.8 ± 7.0 culture points at
4p. The gap is real in the units the trainer itself optimises.

### The 4p arm's structural handicap, measured

Two mundane factors compound, and both are worth having as numbers because any
"just let it run" recommendation has to be priced against them.

**1. A 4p game is a much noisier sample.** Same policy (`champion_2p.json`),
same opponent family (`book`), only the table size differs:

| | per-game culture margin sd | per-game win-share sd |
|---|---|---|
| at 2 players (n=200) | **38.8** | 0.157 |
| at 4 players (n=400) | **107.2** | 0.494 |

2.8× the spread per game. Equal-resolution training therefore needs ~7.6× the
games at 4p.

**2. The 4p arm gets fewer games per hour.** Over the same 6.9-hour window:
2p 26.2 generations/h, 3p 29.0, **4p 14.7**.

Together: to reach the same statistical resolution per generation, the 4p arm
needs roughly **7.6 × 1.8 ≈ 14× the wall clock** of the 2p arm. It has had the
same 6.9 hours. That is a real and sufficient reason for the 4p arm to be
*behind*. Whether it is only behind, or also pointed somewhere wrong, is
section 2.

---

## 2. Step 3 — is it the generation budget, or the training itself?

### 2a. It is not the generation budget — matched-generation control

`experiments/league_state/ladder_2p/` happens to contain `gen00099.json`, the
2p arm's accepted champion at **exactly the generation count the 4p arm is at
now**. Same trainer, same pool structure, same guard, same `--block 12
--subset 4 --accept-z 1.2816`, same 99 generations of budget. Played at 4
players against `book`, n=400:

| vector | generations | win rate @4p (null 25%) | margin @4p |
|---|---|---|---|
| `ladder_2p/gen00099` | **99** | **57.4% ± 2.5%** | **+70.8** |
| `champion_4p.json` (gen 95 accepted) | **99** | 27.6% ± 2.2% | +20.8 |

Paired game-for-game (same seeds, same three BookBots):

| paired, 2p gen-99 − 4p gen-99, at 4p | value |
|---|---|
| win share | **+0.2975 ± 0.0315** (z = 9.5) |
| culture margin | **+50.02 ± 5.23** (z = 9.6) |

**Ninety-nine generations of 2-player training produce a vector that plays
4-player Through the Ages more than twice as well as ninety-nine generations of
4-player training does, measured against the 4p arm's own hardest pool
opponent.** The generation budget is fully controlled and the entire 30-point
gap survives. The noise and throughput handicaps in section 1 are real, but
they are not the explanation.

(Also worth noting: the 2p arm's 4p strength is flat from gen 99 to gen 180 —
57.4% → 57.9% win, +70.8 → +54.6 margin. Its later generations specialised
into 2p without gaining anything at 4p, and possibly lost a little margin. So
the 2p vector is not "a better bot that keeps getting better at everything";
it reached this level of 4p play by gen 99 and stopped.)

This is the finding. **The 4p training signal is producing a worse policy per
generation at its own game.**

### 2b. Weight-level: what the 4p arm drove somewhere the other arms did not

Standing caveat, from this repo's own history: individual trained weights are
not interpretable in isolation, and champion weight marginals are
indistinguishable from a random walk. So the observation below is offered
*only* as a lead, and the head-to-head that tests it is reported honestly
whichever way it came out.

`culture` is in `hillclimb.FROZEN` at 1.0 — it is the numeraire the whole
evaluation is denominated in. But the value a `WeightedBot` actually puts on a
point of its own score is the **phase blend**

    w[culture] + (1 - L) * w[culture_early] + L * w[culture_late]

and nothing pins that. `_PHASE_MULT` deliberately exempts every `_early` /
`_late` multiplier from `guard_weights`, on the (correct) gauge argument that a
phase multiplier's own sign carries no information. The blended coefficient,
however, **is** gauge-invariant, and no guard looks at it.

Effective coefficient on the champion's own culture, by age (L values from the
horizon calibration in `docs/CULTURE_GAP.md` §8b):

| vector | Age A | Age I | Age II | Age III |
|---|---|---|---|---|
| `DEFAULT_WEIGHTS` | +0.90 | +1.30 | +1.84 | +2.41 |
| `champion_2p` | +1.00 | +1.02 | +1.05 | +1.08 |
| `champion_3p` | +1.50 | +1.34 | +1.13 | +0.89 |
| **`champion_4p`** | **−0.87** | **−0.28** | +0.50 | +1.34 |

The 4p champion prices its own victory points **negatively through Age A and
Age I**, crossing zero at L = 0.470, roughly the middle of Age II. Traced
through the ladders, the two arms walked in opposite directions from the same
`culture_early = −0.400` start:

| | gen 0 | gen 20 | gen 60 | gen 99 |
|---|---|---|---|---|
| `ladder_2p` effective culture @L=0 | +0.600 | +0.600 | +0.995 | **+0.912** |
| `ladder_4p` effective culture @L=0 | +0.600 | **−0.216** | −0.772 | **−1.318** |

The 4p arm crossed zero at gen 20 and has stayed negative for 75 generations.
The 2p arm moved monotonically the other way.

**The head-to-head test.** Revert `culture_early` alone on the live 4p
champion, from −2.318 to its `DEFAULT_WEIGHTS` value of −0.400 (effective
coefficient at L=0 goes −1.318 → +0.600), change nothing else, and replay
against `book` at 4p on the same seeds, n=400:

| vector | win rate @4p (null 25%) | margin @4p |
|---|---|---|
| `champion_4p` as trained (`culture_early = −2.318`) | 27.6% ± 2.2% | +20.8 |
| `champion_4p` with `culture_early := −0.400` | **23.1% ± 2.1%** | **+14.3** |

Paired game-for-game on identical deals and identical opposition:

| reverted − as-trained | value |
|---|---|
| win share | **−0.0450 ± 0.0292** (z = −1.5) |
| culture margin | **−6.50 ± 5.16** (z = −1.3) |

**The lead is refuted.** Reverting the one weight does not recover any of the
30-point gap; it is a null trending slightly *negative*. This is exactly the
outcome this repo's standing rule about weight marginals predicts, and it is
recorded here so nobody re-derives the arithmetic and reaches for it again: the
negative early-culture coefficient is a striking, exactly-computable,
gauge-invariant sign inversion on the frozen numeraire, it is unique to the 4p
arm, it appeared at gen 20 and persisted for 75 generations — **and undoing it
in isolation does nothing.** Either it is a symptom of the vector rather than
a cause, or it is load-bearing in combination with the other 77 weights that
were fitted around it.

### 2c. So the mechanism is at the level of the training signal, not the vector

Given 2a (matched generations, 30-point gap) and 2b (a null on the one
weight-level lead), the defensible statement is about the *loop*, not the
*vector*. See section 4.

---

## 3. What the mechanism is NOT

**Not a 4-player engine bug.** The 2×2 rules this out on its own: the 2p
vector plays the same engine, the same 4-player rules and the same three
BookBots and scores 57.9%. Whatever the engine does at 4p, it is not what is
holding the 4p champion to 27.6%. (Spot-checked anyway: `engine/game.py`'s
`SWEEP = {2: 3, 3: 2, 4: 1}` matches `docs/RULES_SPEC.md` §2 [RB p.6/8, CoL
p.3]; `cards.civil_deck` / `military_deck` return 20/53/53/53 civil and
10/45/50/45 military at 4p, matching `docs/OPEN_QUESTIONS.md` items 2, 17 and
18. `docs/CULTURE_GAP.md` §3 audited the war/aggression path at 4p in detail
and found no engine bug; I did not re-derive it.)

**Not the search architecture.** Both vectors in every comparison above were
played under identical `QuiescentBot(levels=1)`, so the architecture cancels
out of the paired differences. The brief's own observation that the archived
1-ply run landed in the same place is consistent: the arm, not the searcher, is
what differs.

**Not the floor tier.** `default`/`greedy`/`random` read 97.9–100% at 4p and
are worth exactly nothing here — they are saturated. The 4p vector's weakness
shows up plainly the moment you point it at a non-saturated opponent, including
at 2 players where the floor tier would still read ~100%.

**Not, apparently, the specific defects the previous diagnosis found.** The
gen-61 4p champion `docs/CULTURE_GAP.md` §2c dissected had `rival_culture` at
+5.611 (an inverted sign worth −41 evaluation points on a 10-culture theft).
The two-sided guard landed and the current 4p champion has `rival_culture` =
0.000. The horizon fix (`e990920`) landed too. The gap survived both.

---

## 4. Mechanism, and the next action

### The mechanism I can defend

The 4p accept gate is **2.8× less sensitive per generation than the 2p one, and
nothing in the configuration compensates for it.** `run_league.sh` passes
`--block 12 --subset 4` at every player count, so every arm buys the same
48 games per accept decision — while the quantity being measured has 2.8× the
per-game spread at 4p (sd 107.2 vs 38.8, measured on the identical policy).
The standard error of one 48-game block is ±5.6 culture points at 2p and
**±15.5 at 4p**. Matching 2p's resolution would take ~7.6× the games.

A hill climb whose accept test is that much fuzzier does not merely converge
more slowly — it converges *somewhere worse*, because it accepts proposals that
are not improvements and never un-accepts them. The 4p arm has the **highest**
accept rate of the three (21.4% vs 15.9% / 17.8%) while producing the weakest
vector, which is the shape you would expect. `score_candidate` compounds it:
`min_blocks=1`, `max_blocks=4`, and the one-sided `lo > 0` bound is re-tested
after **every** block, so a candidate gets up to four looks at a nominal
z=1.2816 bound and stops at the first one it clears. That is optional stopping,
its realised false-accept rate is well above nominal, and it costs most where
the per-block noise is largest.

I have measured the sensitivity gap and the accept-rate ordering. I have
**not** measured the false-accept rate directly, so the last paragraph is a
hypothesis consistent with the data rather than an established mechanism. The
recommendation below does not depend on it.

### The single highest-value next action

**Start a second 4p arm warm-started from `experiments/league_state/champion_2p.json`,
into a fresh `--state-dir`, and leave the existing 4p arm running untouched.**

    nohup experiments/run_league.sh 4 12 2 2 12 4 1.2816 \
        --state-dir experiments/league_state_4p_warm \
        --init experiments/league_state/champion_2p.json \
        --weight-guard clamp --past-k 2 --candidate-bot quiescent:levels=1 \
        >/dev/null 2>&1 &

Why this one:

* It converts a **measured, paired** +29.8 ± 3.2 win-point / +50.0 ± 5.2
  culture-point advantage (z = 9.5 / 9.6) into the 4p arm's starting position, today, for zero training cost.
  The 4p arm needed 99 generations to reach 27.6%; this starts at 57.4%.
* It is the only action here whose payoff does not depend on my being right
  about *why* 4p training is weaker. Even if the cause is purely the gate
  sensitivity, a better starting point is still worth 30 points.
* It costs one process and no code change.

Two things to be careful about, both explicit:

1. **This is not the warm-start `docs/TRAINING_RUN.md` and
   `docs/LEAGUE_TRAINING.md` forbid.** That prohibition is about
   `experiments/champion_4p.json`, the pre-horizon-fix vector with
   `science = −6.089` that `arena.refuse_if_degenerate_champion` hard-refuses.
   `champion_2p.json` is the opposite: an externally validated vector at 97.5%
   ± 1.1% vs `book` at 2p and 57.4–57.9% at 4p. Note `hillclimb_league.run`
   applies `guard_weights` to a warm-started champion on load, and
   `INIT_OVERRIDES` only fires on a clean `default` start, so
   `hand_potential = 0.725` would carry over from the 2p vector against the
   4p-specific override of 0.0 — worth watching the first full check for.
2. **Do not kill the existing 4p arm.** It is the control. If the warm-started
   arm cannot improve on 57.4% over ~50 generations while the cold arm keeps
   climbing, that is itself the answer (a 2p-optimised basin that 4p hill
   climbing cannot leave), and you only learn it by running both.

Second and third, in order, if there is appetite for more:

* **Scale `--block` with the player count.** 48 games per accept decision is
  ±5.6 culture points at 2p and ±15.5 at 4p. `run_league.sh` should not pass
  the same block size to all three arms. This is a launch-flag change with a
  measurable target (the block's standard error) rather than a guessed one --
  though note it trades directly against generations per hour, which is
  already the 4p arm's scarcest resource, so it wants its own A/B rather than
  being applied on this document's say-so.
* **Have `guard_weights` look at the blended coefficient, not just the raw
  weight.** The `_PHASE_MULT` exemption is right about gauge invariance for a
  multiplier's own sign, but `w[k] + (1-L)·w[k_early] + L·w[k_late]` *is*
  gauge-invariant and is currently unguarded at both ends of the L range. At
  minimum, log it when the blend on `culture` — the FROZEN numeraire — changes
  sign, which for the 4p arm happened at gen 20 and was never reported.

### Things I could not determine

* **Why** the 4p signal is weaker in a way that produces this specific
  policy shape. The gate-sensitivity argument explains "worse", not "worse in
  this direction".
* Whether the negative early-culture coefficient is a cause or a symptom
  (see 2c).
* Whether the same result holds under the 1-ply `WeightedBot` architecture.
  Every comparison here is architecture-controlled (both sides
  `QuiescentBot(levels=1)`), so the *comparison* is safe, but I did not measure
  the absolute 1-ply numbers.

---

## 5. Reproducing

Everything here was run read-only against the main checkout while the three
live arms kept training; nothing under `experiments/` was written. Driver is a
throwaway wrapper around `experiments.arena.duel` plus `experiments.bookmatch`'s
`make_bot` patch (which is what teaches `arena` the `book` / `book2` specs).
Raw per-game results, including the paired `per_game` and `per_game_margin`
series every paired test above is computed from, are in `/tmp/fourp/*.jsonl`.

    # snapshot the moving targets first
    cp experiments/league_state/champion_{2,3,4}p.json /tmp/fourp/
    cp experiments/league_state/ladder_2p/gen00099.json /tmp/fourp/champ2p_gen99.json
    cp experiments/league_state/ladder_4p/gen00089.json /tmp/fourp/champ4p_gen89.json

    # one arm, e.g. the matched-generation control
    TTA_JOURNAL=1 python3 -m experiments.evaluate \
        --a quiesce:/tmp/fourp/champ2p_gen99.json,levels=1 --b book \
        --players 4 --games 400 --workers 3

(`experiments/evaluate.py` does not know the `book` spec; import
`experiments.bookmatch` first, or use `experiments/bookmatch.py`'s own
`--matchups champ_vs_book`. The wrapper I used is reproduced in the branch
history of this file's commit message only — it is 40 lines and not worth
committing.)

Full arm list, all `TTA_JOURNAL=1`, all seat-rotated, all zero engine errors:

| arm | n | win rate | margin |
|---|---|---|---|
| `champion_4p` vs `book` @4p | 400 | 27.6% ± 2.2% | +20.8 |
| `champion_2p` vs `book` @4p | 400 | 57.9% ± 2.5% | +54.6 |
| `champion_4p` vs `book2` @4p | 400 | 32.6% ± 2.3% | +36.1 |
| `champion_2p` vs `book2` @4p | 400 | 59.1% ± 2.5% | +56.1 |
| `champion_2p` vs `book` @2p (harness check) | 200 | 97.5% ± 1.1% | +85.1 |
| `champion_4p` vs `book` @2p | 200 | 56.0% ± 3.5% | +10.7 |
| `ladder_4p/gen00089` vs `book` @4p | 400 | 14.9% ± 1.8% | −8.9 |
| `champion_3p` vs `book` @4p | 400 | 33.6% ± 2.4% | +33.9 |
| `ladder_2p/gen00099` vs `book` @4p | 400 | 57.4% ± 2.5% | +70.8 |
| `champion_4p` + `culture_early := −0.4` vs `book` @4p | 400 | 23.1% ± 2.1% | +14.3 |

Not run, and worth running if anyone picks this up: the same 2×2 under the
1-ply `WeightedBot` (architecture is controlled in every comparison here, but
the absolute 1-ply numbers are unmeasured), and `DEFAULT_WEIGHTS` vs `book` at
4p (the arm's own starting point; the full check's `past:ladder_4p/gen00000`
row puts the current champion at 97.9% against it, so the 4p arm has clearly
travelled a long way from `default` — it has just travelled somewhere worse
than the 2p arm reached in the same number of generations).
