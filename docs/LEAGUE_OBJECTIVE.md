# What the league maximises, and the pool it maximises it on

Date: 2026-07-27. Branch: `league-objective`.
Changed: `experiments/hillclimb_league.py`, `experiments/hillclimb_pool.py`,
`experiments/arena.py` (one additive output key), `experiments/watchdog.sh`.
New: `tools/objective_ab.py`, `tests/test_league_objective.py` (21 tests).
Nothing in `engine/` is touched; `bash tools/gate.sh` is GATE PASS with all
six digests unmoved.

Read `docs/TWOP_PROFILE.md` §9 and `docs/TRANSFER_TEST.md` §8 first — both of
them predicted this in writing and neither acted on it. This document is the
action, and the measurement of whether it was the right one.

## 0. One paragraph

The league accepted a mutant when its **culture margin** (mine − theirs) beat
the champion's. War and aggression *move* culture from the victim to the
attacker, so a stolen point moves the margin by two and a produced point by
one. The climber found that. The resulting champion holds its rival to 26
culture while scoring 65 itself, wins 97.9% of a pool that is a single
hand-written bot family, and scores **64.7 against a human 159.5** on an
engine whose scoring is validated exact against all 1,011 human journals. The
accept criterion is now **own final culture with win share as a small
tiebreak** (`--objective blend`, default), the old modes are preserved behind
the same flag, and the pool went from **69% static hand-written bots to 24%**.

**The check that gated the restart** (§3): 1,632 games played once and
re-scored under every objective, so the objectives are compared with zero
sampling noise between them. The old objective prefers the 64.7-culture
champion over the 139.8-culture vector at 6-8 SE; the new one prefers the
139.8 vector. **Win rate alone would also have kept the theft champion** — so
"just gate on the real objective" was not an available fix, and that is the
most useful thing this exercise turned up.

## 1. The bug, stated exactly

`hillclimb_pool.margin_share(m) = 0.5·(1 + tanh(m / 120))` where `m` is
`arena.duel`'s `per_game_margin` = *A's final culture − the mean of the
defenders'*. That is a **differential of a non-conserved quantity**.

Through the Ages does not conserve culture: `events.py` war spoils pay
`min(5 + advantage, loser.culture)` to the attacker **and take the same from
the loser**, and aggression theft does the same on a smaller scale. So:

| move | own culture | rival culture | margin |
|---|---|---|---|
| produce 20 culture | +20 | 0 | **+20** |
| steal 20 culture | +20 | −20 | **+40** |

The gate metric paid twice for the second row. `tests/test_league_objective.py::
ScoreSeries::test_theft_is_paid_once_not_twice` is that table as an assertion.

You win Through the Ages by having the most culture. You do not win it by
having the biggest gap. The two coincide only if culture is conserved, and it
is not.

### What it produced, measured

`docs/TWOP_PROFILE.md`, n=300 per matchup, causal (move-class ban, not
accounting): 69% of the 2p champion's 85.5-point margin against `book` is the
conflict move class, and the mechanism is **suppression, not scoring** —
banning the fighting barely moves the champion's own total (131.0 → 119.8) and
nearly doubles `book`'s (45.5 → 93.8). The champion is *behind* on tech (8.4 vs
10.3 techs) and on wonders (0.16-0.26 vs 0.80-1.41 completed).

Against the only external anchor this project has:

| | final culture, 2p |
|---|---|
| humans (`docs/HUMAN_BASELINE.md`, n=692 games) | **159.5** [156.0, 163.0] |
| the 1-ply-lineage vector the league replaced | **139.8** [131.6, 148.3] |
| the margin-trained champion | **64.7** |

## 2. The new criterion, and why this one

### The two candidates, and why neither wins alone

**Win rate** is the true objective and is unusable alone as a *gradient*:

* it saturates at the top — 0.94-0.97 against `book` under PlanBot, where
  `docs/TRANSFER_TEST.md` §3 had to fall back to margin because "win share
  cannot discriminate";
* it is flat 0.0 at the bottom — against an opponent the champion never beats,
  the paired edge is exactly 0.0 with se exactly 0.0, which is the degeneracy
  the margin gate was introduced to fix in the first place;
* it is *compressed and noisy at 4p*. `docs/FOURP_GAP.md` §1 pooled 704
  opponent-checks: a +85 culture margin buys ~97% at 2p and ~50% at 4p, and
  the per-game win-share sd is 0.494 at 4p against 0.157 at 2p.

**Own final culture** is dense, exists on every game, prices a stolen point
exactly once — and is not literally what you win on. A vector that scores 150
while letting the table score 200 would be accepted.

### What was chosen

`--objective blend` (the default), a per-game convex combination:

```
score = (1 − alpha) · own_share(own final culture) + alpha · win_share
own_share(c) = 0.5 · (1 + tanh((c − CULTURE_CENTRE) / CULTURE_SCALE))
CULTURE_CENTRE = 100      CULTURE_SCALE = 120      alpha = 0.15
```

Both terms already live in (0, 1) with a paired null of exactly 0, so their
convex combination does too and `weighted_stats` averages them untouched. The
accept test itself is unchanged: the weighted mean of paired per-game score
differences, one-sided `lo > 0`.

**Why the squash is offset, which the margin one is not.** A margin is centred
on 0 by construction; own culture is strictly positive and lives around
40-200. Uncentred, `tanh(c/120)` prices a culture point at a human score
(159.5) at **one third** of a culture point at our score (64.7) — a built-in
bias against ever closing the gap. Centred at 100, the marginal value of a
culture point is 0.00383 at c=65 and 0.00327 at c=160: **flat to 17% across
the whole band we care about**, while a 400-point outlier still saturates.
That 3.1x-vs-1.17x contrast is pinned by
`OwnShare::test_marginal_value_is_flat_across_the_band_we_care_about`.

**Why alpha is small, and why it is not zero.** Per-game win share is a 0/1
step; paired against a reference on the same seeds it is 0 when both arms
agree and ±1 when they disagree, so its paired sd is several times the culture
term's. A large alpha therefore buys *variance*, not objective-alignment: it
widens the accept CI and the climb stalls. Measured on the 1,632 games of §3
— per-game **paired** standard deviation of each objective's score series:

| objective | per-game paired sd | games needed for equal resolution |
|---|---|---|
| `own` | **0.218** | 1.0x |
| `blend` (alpha=0.15) | **0.233** | 1.15x |
| `margin` (the old gate) | 0.419 | 3.7x |
| `winshare` | 0.500 | 5.3x |

So the coarse term costs 5.3x the games for the same accept resolution, and
blending 15% of it in costs 15%. That is the whole argument for a small alpha,
and it is measured rather than assumed.

alpha = 0.15 is the tiebreak weight: it lets win share decide between
candidates the culture term calls equal, and it makes "score more culture but
lose the game" cost something, without letting the coarse term set the step
size. `--objective-alpha 0` is pure own culture, `1` is pure win share; the
whole thing is one flag and no code edit.

There is one non-arbitrary way to read the number. The exchange rate an alpha
implies is *how many culture points a full win/loss flip is worth*:

| alpha | one win flip is worth |
|---|---|
| 0.05 | 12.6 culture |
| **0.15** | **42.4 culture** |
| 0.25 | 80.0 culture |
| 0.50 | 240.0 culture |

The **observed human winner's margin at 2p is 43.2 [40.3, 46.3]** culture
(`docs/HUMAN_BASELINE.md`). So alpha = 0.15 prices a win at almost exactly
what winning a real game is worth in culture, which is the rate that makes the
two halves of the blend mutually consistent rather than one of them a thumb on
the scale. That is a coincidence noticed after the fact, not a fit — but it is
the reason 0.15 was kept rather than rounded to 0.1.

### What is preserved

`--objective margin` and `--objective winshare` are the pre-2026-07-27 modes,
unchanged, and `--gate-metric` is still accepted as an alias so old command
lines work. `margin` is the only mode that scores different tiers on different
things (win share everywhere, margin on the gate tiers) and it keeps doing
exactly that. **Every champion this project has produced was selected under it**;
`PoolMetric::test_legacy_default_is_winshare_with_a_margin_gate` and
`test_legacy_tier_weights_reproduce_the_shipped_pool` assert the mode and the
per-opponent weights the live 2p arm logged on 2026-07-27, so a drift is a test
failure rather than a silent loss of every historical result. Full
reproduction of an old run is

```
--objective margin --pool-weights book=3,floor=0.5,hall=1,mirror=1,past=1,quiescent=2,variant=2.5
```

(`P.legacy_weight_string()` prints that string; `hall` did not exist as a tier
before and its files were added to `past`, which is why it appears set to the
old `past` total.)

### One consequence that would have been a silent bug

`RefCache` shortcut-ed the mirror row: a champion at a table of itself takes
1/players of the wins by construction and has a culture margin of exactly 0 by
symmetry, so the reference needed **no games**. Own culture has no such
symmetry — a champion mirror scores some perfectly ordinary ~65-80 — so under
`own`/`blend` the mirror reference is **played** like any other. It costs one
extra block of games per generation and it converts the mirror row from a
constant into the cleanest paired comparison in the pool:
candidate-in-seat-*s*-against-champions versus
champion-in-seat-*s*-against-champions, identical opposition, same seeds.
`MirrorReference` in the test file pins both halves.

## 3. Does the new objective rank two known vectors the right way round?

This is the cheap, decisive check, and it is the one that gates the restart.
Not "does the new objective train something good" — that takes the 45 hours.
**"Does it rank two vectors whose external scores we already know the right
way round?"** We have exactly that pair:

| | vector | own culture vs `book`, ship policy |
|---|---|---|
| **A** = P | `archive_preplan/league_state_1ply_20260726/champion_2p.json` (gen 355) | **213.4** |
| **B** = Q | `experiments/league_state/champion_2p.json` (gen 240, the live champion) | **127.8** |

(Ship-policy numbers from `docs/PLAN_WAR_LOOKAHEAD.md` §4a. Against the human
corpus the same two vectors read 139.8 and 64.7.)

`tools/objective_ab.py` plays both against **every** pool opponent on
byte-identical seeds — A as candidate, B as reference, exactly the pairing
`score_candidate` accepts on — and then re-scores **that one set of games**
under each objective and each tier-weight preset. Because the games are
shared, the objectives are compared with **zero sampling noise between them**:
any difference in verdict is the objective, not the deal.

2p, n=48 games per opponent per side, 17 opponents, 1,632 games, both vectors
under `quiescent:levels=1`, zero engine errors.

```
objective weights      aggregate edge (A-B)       z   per-game sd   verdict
winshare  new               -0.1967 +/-0.0287    -6.8        0.5003   prefers B
winshare  legacy            -0.1739 +/-0.0205    -8.5        0.4620   prefers B
margin    new               -0.1795 +/-0.0284    -6.3        0.4190   prefers B
margin    legacy            -0.1321 +/-0.0161    -8.2        0.3867   prefers B
own       new               +0.0673 +/-0.0114    +5.9        0.2177   prefers A
own       legacy            +0.1128 +/-0.0090   +12.5        0.2142   prefers A
blend     new               +0.0277 +/-0.0126    +2.2        0.2334   prefers A
blend     legacy            +0.0698 +/-0.0097    +7.2        0.2256   prefers A
```

**The sign inverts.** The old objective prefers the 64.7-culture theft
champion at 6-8 SE; the new one prefers the 139.8-culture production vector.
That is the whole result, and it is not a marginal call in either direction.

### Three things in that table that were not the point and matter anyway

**1. Win rate would NOT have fixed this.** `winshare` prefers B at
−0.1967 ± 0.0287, *more strongly than margin does*. The literal objective —
who wins the game — keeps the theft champion. Q genuinely beats P head to head
under the training search: it wins 74-98% against every pool opponent where P
wins 22-90%. So "just gate on win rate" was not an available fix, and had it
been chosen the arms would have carried on producing the same policy while
looking principled. This is the single most useful thing this measurement
produced and it was not what it was run for.

The reason preferring A is nevertheless right is **not** in this table, it is
in `docs/PLAN_WAR_LOOKAHEAD.md` §3-4a: under the policy we would actually
**ship** (`plan:width=8` with the war lookahead) the two vectors are a
head-to-head **null** — 0.522 ± 0.037 win share, +1.4 ± 5.3 margin — while
their own cultures are 213.4 and 127.8. Q's head-to-head edge is a property of
`QuiescentBot.WAR_LOOKAHEAD`, the training proxy. So choosing the producing
vector costs nothing measurable in strength under the ship policy and buys
~85 culture. If that stops being true, this decision should be revisited.

**2. Own culture is less than half as noisy per game as win share.** Per-game
paired sd: `own` 0.218, `blend` 0.233, `margin` 0.419, `winshare` 0.500. Win
share needs **5.3x the games** for equal resolution. This is the variance
argument of §2 measured rather than asserted, and it is why alpha is 0.15 and
not 0.5 — 0.15 costs only +7% on the sd (0.2177 → 0.2334) while buying the
win-rate tiebreak.

**3. The three saturated dummies are the most opinionated tier in the pool
under own-culture scoring.** Per-opponent blend edges by tier:

| tier | mean d(own) | mean d(win) | mean d(blend) |
|---|---|---|---|
| **floor** (`default`/`greedy`/`random`) | **+0.203** | −0.014 | **+0.170** |
| book | +0.179 | −0.125 | +0.133 |
| variant | +0.161 | −0.224 | +0.103 |
| hall | +0.090 | −0.201 | +0.046 |
| past | +0.066 | −0.151 | +0.033 |
| mirror | −0.043 | −0.271 | −0.077 |

The dummies push harder than `book` does, and their win-share column is
−0.014 — i.e. *they are still saturated on win rate and now loud on culture*.
That is the predicted failure mode, measured: they are the only opponents that
never contest the card row and never attack, so "farm quietly" scores against
them in a way it cannot score against anything that plays. Weighting the whole
pool: blend edge is **+0.0357** with `floor=0` and **+0.0479** with
`floor=0.5` — a third of the signal would have come from three bots that
cannot lose. Dropping them is now a measurement, not an argument.

### And one honest note about the rebalanced pool

The new weights give a *smaller* margin of preference than the legacy ones
(+0.0277 ± 0.0126 vs +0.0698 ± 0.0097). That is the rebalance working, not
failing: the weight moved onto `mirror`, `past` and `hall`, which are the
three tiers where A does **worst** (mirror −0.077, past +0.033, hall +0.046,
against book +0.133 and variant +0.103). The new pool is genuinely harder and
more discriminating, and z=+2.2 is the honest number for it.

## 4. The pool

### What it was

| tier | total | members | each | share of signal |
|---|---|---|---|---|
| `book` | 3.0 | `book`, `book2` | 1.50 | 38% |
| `variant` | 2.5 | six `var:*` | 0.42 | 31% |
| `mirror` | 1.0 | `mirror` | 1.00 | 13% |
| `past` | 1.0 | 2 `past:*` + 3 `hall:*` | 0.20 | 13% |
| `floor` | 0.5 | `default`, `greedy`, `random` | 0.17 | 6% |

**69% of the training signal came from static hand-written bots**, and every
one of them is `BookBot` or a `BookBot` subclass — `docs/TWOP_PROFILE.md` §9's
"the pool is a monoculture". The champion's dominance of it is at least partly
threshold exploitation: `var:military` gates its offence on a hard-coded +3
strength lead and reaches it on **5.5% of turns against the champion versus
41-44% against the rest of its own family**, so its entire offensive plan
never fires. That is exploiting an implementation artefact.

### What it is now

| tier | total | members (2p) | each | share |
|---|---|---|---|---|
| `book` | 0.6 | `book`, `book2` | 0.30 | 12% |
| `variant` | 0.6 | six `var:*` | 0.10 | 12% |
| `mirror` | 1.0 | `mirror` | 1.00 | 20% |
| `past` | 1.2 | 2 `past:*` | 0.60 | 24% |
| **`hall`** | 1.6 | 3-4 `hall:*` | 0.40-0.53 | 32% |
| `floor` | **0.0** | — dropped — | — | 0% |

**Static 24%, opponents that move 76%** — the exact inversion. Four changes:

1. **`hall` is its own tier.** It used to share `past`'s total, so adding a
   frozen champion diluted the anti-cycling ladder and vice versa. They are
   different jobs (`past` rotates and ages out, `hall` never does) and now
   they are different dials.
2. **The static tiers keep the VETO.** `DEFAULT_GATE_TIERS` is still
   `book, variant, quiescent`. Their job changed from *supplying the gradient*
   to *stopping the climber walking off a cliff*, and a veto is exactly that
   job. A self-play tier cannot do it — "do not regress against your own
   parent" is a statement about the lineage, not about play.
3. **`acceptance_subset` gained a third invariant: one LADDER opponent
   (`hall`/`past`) every generation**, alongside mirror and one rotating gate.
   Without it the rotation hands some generations mirror plus three
   0.10-weight variants and **mirror alone decides 77% of that accept** — i.e.
   the mirror-only loop this whole module was built to replace. With it,
   mirror's worst-case share over 40 generations is 58% and its typical share
   is 45-52%. `AcceptanceSubset::test_mirror_never_carries_a_majority_of_a_
   generations_weight` holds the line at 62%.
4. **The three saturated dummies are dropped** (`floor=0`). Under win share
   they were provably inert: the champion beats `default`/`greedy`/`random`
   97.9-100%, so the candidate and the reference both score 1.0 and every
   paired diff is exactly 0.0 with se exactly 0.0 — 6% of the pool weight and
   0 bits of information (`docs/UNATTENDED.md` trap 2). **Under own-culture
   scoring they stop being inert**, which is worse, not better: they are the
   only opponents that never compete for the card row and never attack, so
   "farm quietly" scores against them in a way it cannot score against a real
   opponent, and they would start actively pulling the vector toward a policy
   tuned for an opponent that does not play. They are one flag away
   (`--pool-weights floor=0.5`) and §3 reports what they would have done.

### The human bots landed mid-flight, and they are in

`docs/HUMAN_BOTS.md` merged to master **between** this branch's first merge
and its second, and that agent rebased onto this rebalance rather than around
it: `human` is a new tier at 0.6 (four corpus-fitted archetypes at 0.15 each),
it joins `DEFAULT_GATE_TIERS`, and `watchdog.sh` passes `--human-bots`. The
combined pool at 2p:

```
[pool] book(0.30) book2(0.30) hum:builder(0.15) hum:tempo(0.15)
       hum:warlord(0.15) hum:wonder(0.15) var:*(0.10 x6) mirror(1.00)
       past:*(0.60 x2) hall:*(0.40 x4)
[pool] tier share: book=11%, human=11%, variant=11%, mirror=18%, past=21%,
       hall=29%  (external/fixed 32%, self-play 68%)
```

That changed one thing here: the `tier share` log line used to call only
`book+variant` "static", which after the human tier landed would have
under-reported the fixed side by 11 points. It now reports
**external/fixed vs self-play**, and `human` counts as external — those bots
are fitted and frozen, so they are an anchor like `book`, not a gradient like
`mirror`. External is 32%, not the 24% this document reported before the
merge; self-play is 68%, not 76%. Both still invert the 69/31 that started
this.

This is a strict improvement on the rebalance's weakest point. §7 said "the
pool is still 100% our own artefacts"; four of its nineteen opponents are now
fitted to 1,011 human games and, unlike the `var:*` roster, have no
hand-written threshold to hold shut. That was the exploit
`docs/TWOP_PROFILE.md` §9 could not rule out.

### And one addition to the hall of fame

`experiments/hall_of_fame/` is untracked trainer output, so this is an
operational change rather than a code one:
`archive_preplan/league_state_1ply_20260726/champion_2p.json` (gen 355, "P")
was copied in as `oneply_2p_gen00355.json`. It is the strongest *production*
vector we have (213.4 own culture against `book` under the ship policy) and it
is the only opponent in the pool that is **not** a BookBot subclass and not a
descendant of the current lineage. `docs/TWOP_PROFILE.md` §9 ends by saying
settling the exploitation question "needs an opponent from outside the BookBot
family that is actually strong — which this repo does not currently have". It
does now; it was sitting in an archive directory.

The 3p (gen 27) and 4p (gen 130) members of that lineage were **not** added:
gen 27 is undertrained and neither has been measured on anything.

## 5. The short arm: do the accept decisions look sane?

A real arm, new objective and new pool, into a scratch state dir seeded from a
copy of the live 2p champion (gen 240) so it is a faithful preview of the
restart rather than a toy:

```
python3 -m experiments.hillclimb_league --players 2 --workers 2 --block 12 \
  --subset 4 --candidate-bot quiescent:levels=1 \
  --objective blend --objective-alpha 0.15 \
  --pool-weights book=0.6,variant=0.6,mirror=1.0,past=1.2,hall=1.6,floor=0 \
  --state-dir /tmp/objarm --max-gens 6 --full-check-every 0 --ablate-every 0
```

Startup:

```
[pool] tier share: book=12%, variant=12%, mirror=20%, past=24%, hall=32%
       (static book+variant 24%, self-play 76%)
[2p] objective: blend = OWN CULTURE + win share (alpha=0.15 on win share),
     own_share centre 100 scale 120 -- the whole pool, every tier
```

**Generation 241, candidate 0 — rejected, correctly.** `edge=+0.0105
lo=-0.0033`, i.e. positive but not significant after the full four blocks
(192 games). No accept.

**Generation 241, candidate 1 — accepted**, `op=scatter edge=+0.0316
lo=+0.0147`, after one block:

```
  opponent            tier      w    n    win%  champ%   cult  ccult    marg   cmarg     edge
  mirror              mirror 1.00   12   58.3%   50.0%   78.7   71.7    +5.3    +0.0  +0.0354
  hall:...3p_gen00205 hall   0.53   12  100.0%  100.0%  100.5   97.2   +58.9   +58.7  +0.0115
  hall:...4p_gen00102 hall   0.53   12   91.7%   83.3%   85.2   79.1   +47.7   +38.7  +0.0331
  book2               book   0.30   12   83.3%   83.3%  123.9  107.3   +68.8   +53.8  +0.0523 GATE
```

Three things to read off it, all of them the point of the exercise:

1. **Every row moved own culture UP** — +7.0, +3.3, +6.1, +16.6 — and win rate
   moved up or held on all four. The accepted mutant out-*produces* its parent
   against every opponent it was tested on. That is the behaviour the new
   objective is supposed to select.
2. **It is production, not suppression.** On the `book2` row the candidate's
   own culture went 107.3 → 123.9 while the opponent's went **up** slightly,
   53.5 → 55.1 (back out from `cult − marg`). The old objective would have
   scored that move at +15.0 of margin; the new one scores the +16.6 of own
   culture it actually is. A margin gate is indifferent between this move and
   one that steals 8 points, and this arm's first accept is the former.
3. **The `mirror` row is a real measurement now.** `ccult` reads 71.7, not a
   constant — the champion-vs-champion reference was played, as §2 says it must
   be. Under the old objective that row's reference was 0.500 forever.

### What the short arm does NOT show

Two generations of accepts is a check that the *machinery* is sane and the
decisions point the right way. It is **not** evidence that the objective
trains a better bot, and it is far too short to move
`tools/bgo_botmatch.py`'s own-culture number toward the human 159.5 — that
measurement needs a champion that has actually trained, and it is the
pre-registered check for whoever picks this up after the arms finish. The
arm ran at ~7 minutes per generation on a box at load 14, so the six
generations were not all completed before the restart; the two that were are
reported above and nothing was cherry-picked (they are the first two).

## 6. The restart

### How to stop the arms — `pkill -f run_league.sh` is NOT enough

`docs/UNATTENDED.md` says "to stop everything early: delete [the deadline
file], then `pkill -f run_league.sh`". That kills the **supervisor** and
leaves its `python3 -m experiments.hillclimb_league` child running for up to
an hour. The cron watchdog then sees no supervisor, launches a new one, and
**two processes write the same state dir** — the champion file, the state
file, the ladder and the generation log all get interleaved writes from two
climbers. Kill both, supervisors first so they cannot restart the child:

```
pkill -f run_league.sh          # supervisors
pkill -f hillclimb_league       # the climbers themselves
```

The watchdog (cron, `*/10`) relaunches within ten minutes with only the
**remaining** budget from `experiments/logs/watchdog_deadline`. The deadline
was not touched: it is still 2026-07-29 08:04.

### What the arms resume from, and why the state dirs were NOT wiped

All three arms resume their existing champion. That is deliberate and it is
the opposite of what "the champion scores 64.7, throw it away" suggests:

* **The rebalanced pool needs the ladder.** 56% of the new weight is `past` +
  `hall`, and `past` is populated *from the state dir's own ladder*. A wiped
  state dir starts with one `past` entry — itself, at generation 0 — so
  wiping would put the majority of the training weight on three hall files and
  a mirror of a `DEFAULT_WEIGHTS` bot. The thing that makes the new pool worth
  having is the thing wiping would destroy.
* **The accept test is paired against the incumbent, so a bad incumbent is not
  a trap the way a bad *pool* is.** From generation 1 the new objective prices
  every mutation on own culture; the champion is a starting point, not a
  target.
* And 44 hours is not 48: a cold start would spend a large part of the
  remaining budget re-reaching a place the warm start is already at.

### The restart as it happened, 2026-07-27 13:00

All three arms came back on the cron tick, with **44h** left of the original
48h budget and the deadline file untouched at `1785333865`
(2026-07-29 08:04:25).

```
2026-07-27 13:00:00 watchdog: relaunched 2p (44h left, workers=1 block=12) ... --objective blend --objective-alpha 0.15 --pool-weights book=0.6,variant=0.6,mirror=1.0,past=1.2,hall=1.6,floor=0
2026-07-27 13:00:01 watchdog: relaunched 3p (44h left, workers=2 block=12) ... --objective blend --objective-alpha 0.15 --pool-weights book=0.6,variant=0.6,mirror=1.0,past=1.2,hall=1.6,floor=0
2026-07-27 13:00:01 watchdog: relaunched 4p (44h left, workers=2 block=24) ... --objective blend --objective-alpha 0.15 --pool-weights book=0.6,variant=0.6,mirror=1.0,past=1.2,hall=1.6,floor=0
```

and each arm's own log agrees (2p shown; 3p and 4p are identical but for the
ladder members and `gen=`):

```
[pool] book(w=0.30,blend), book2(w=0.30,blend), var:culture(w=0.10,blend),
       var:infra(w=0.10,blend), var:military(w=0.10,blend),
       var:science(w=0.10,blend), var:tempo(w=0.10,blend),
       var:wonder(w=0.10,blend), mirror(w=1.00,blend),
       past:ladder_2p/gen00000(w=0.60,blend), past:ladder_2p/gen00228(w=0.60,blend),
       hall:hall_of_fame/oneply_2p_gen00355(w=0.40,blend),
       hall:hall_of_fame/preinfo_2p_gen00188(w=0.40,blend),
       hall:hall_of_fame/preinfo_3p_gen00205(w=0.40,blend),
       hall:hall_of_fame/preinfo_4p_gen00102(w=0.40,blend)
[pool] tier share: book=12%, variant=12%, mirror=20%, past=24%, hall=32%
       (static book+variant 24%, self-play 76%)
[2p] league trainer: 15 opponents, gen=242 sigma=0.097
[2p] trained architecture: quiescent {'levels': 1} -- ...
[2p] objective: blend = OWN CULTURE + win share (alpha=0.15 on win share),
     own_share centre 100 scale 120 -- the whole pool, every tier
```

**One log-reading trap, recorded because it nearly produced a false alarm.**
The 2p arm runs `--workers 1` and Python block-buffers stdout into a
redirected file, so for three minutes after the restart `league_2p.log` still
ended with the **old** `[pool] book(w=1.50,margin) ...` line from the previous
process while the new one was already running correctly. The buffer flushes at
the end of the first generation. The checks that are immediate and
authoritative are `experiments/logs/watchdog.log` and, definitively,
`ps -o command= -p $(pgrep -f "hillclimb_league --players K")`.

### The 4p arm's two asymmetries

Both preserved.

* **`--block 24`** — kept. The justification (4p per-game spread is 2.8x the
  2p one, so equal-resolution accepts need more games) is a statement about
  the variance of a per-game quantity and is not specific to which quantity;
  own culture at 4p is at least as spread as margin at 2p. Not re-derived for
  the new objective, and that is a known gap, not a claim.
* **`--init experiments/hall_of_fame/preinfo_2p_gen00188.json`** — kept, and
  the honest reason is that **it is inert**. `--init` is ignored once the
  state dir holds a champion (`load_champion` returns `resume` first), and the
  4p state dir has held one since 2026-07-26. Reconsidering it as asked: the
  measurement behind it (`docs/FOURP_GAP.md` §2a, 57.4% vs 27.6% at 4p,
  paired, z=9.5) is a *win-rate and margin* measurement, so it does not
  transfer to the new objective, and the vector it names is the 64.7-culture
  lineage. If anyone ever starts a genuinely fresh 4p arm, the seed to
  consider is `archive_preplan/.../champion_2p.json` (P) instead — but that
  choice is **unmeasured at 4p in either objective** and should not be made
  without measuring it. Changing an inert flag on the strength of an argument
  would have looked like a decision and been a no-op.

## 7. What this does not establish — read before quoting anything above

* **Nothing here shows the new objective TRAINS a better bot.** It shows that
  it *ranks* two vectors whose external scores we already know the right way
  round, and that a short arm under it accepts sane-looking moves. Whether 44
  hours of hill climbing on it produces a stronger policy is unmeasured and
  will stay unmeasured until the arms finish. The pre-registered check is the
  one in §5: `tools/bgo_botmatch.py` own culture against the human 159.5, plus
  wonders completed and the tier-3 take rate.
* **The A/B in §3 is n=48 games per opponent.** That is ±7 win points and
  ±12-14 culture points per row at 2p. The per-row numbers are illustrative;
  the *aggregate* verdicts are the load-bearing thing, and they pool 700+
  games. `docs/FOURP_GAP.md` §0 is this repo correcting exactly this mistake
  once already.
* **2p only.** The A/B and the short arm are both 2p. The objective is applied
  to all three arms and the 3p/4p behaviour of `own_share` is *inferred*, not
  measured. At 4p the per-game culture spread is 2.8x the 2p one
  (`docs/FOURP_GAP.md` §1), so the 4p arm's accept CI will be correspondingly
  wider under this objective too — the `--block 24` compensation is unchanged
  and unre-derived.
* **`CULTURE_CENTRE = 100` and `CULTURE_SCALE = 120` are reasoned, not fitted.**
  The reasoning (flat marginal value across 65-160, outliers bounded) is
  checked by a unit test; the constants themselves were not swept. A sweep
  would cost one arm-day and nobody has spent it.
* **alpha = 0.15 is a judgement call informed by one variance measurement.**
  §2 shows the noise argument that says it must be small; it does not show
  that 0.15 beats 0.05 or 0.30. If the arms stall, alpha is the first dial to
  suspect, and it is a flag.
* **Own culture is still not the objective.** The objective is *the most*
  culture. Own culture is blind to a candidate that raises its own score while
  raising the table's more — the alpha term is the only thing pushing back on
  that, and 0.15 of a coarse signal is not much push. If a future champion
  turns out to score 170 and lose, this is the sentence that predicted it.
* **The pool is still mostly our own artefacts.** This was written as "100%"
  and the human bots (§4) fixed the worst of it — but fifteen of nineteen
  opponents are still either a BookBot subclass or something this trainer
  produced, and the four that are not are *fitted to* human games rather than
  being humans. Adding the 1-ply-lineage vector to the hall makes it more
  diverse, not diverse.
* **The human bots were not in the pool for the arms' first hour.** They
  merged after the 13:00 relaunch, so the arms ran ~15 minutes without them
  before being restarted a second time. Nothing is contaminated — the second
  restart resumed the same champions — but the first fifteen minutes of
  generations were scored against an 15-opponent pool and everything after
  against a 19-opponent one, which is visible in `generations_Kp.jsonl`'s
  `pool` snapshot if anyone diffs across that boundary.
* **Dropping the floor tier is a judgement, not a measurement.** §3 reports
  what those three opponents would have contributed under each objective, but
  "they would pull the vector toward farming a bot that does not play" is an
  argument, not an experiment. The counter-argument — that they are a cheap
  did-we-catastrophically-break tripwire — is real, and the mitigation is that
  the gate tiers keep the veto.
