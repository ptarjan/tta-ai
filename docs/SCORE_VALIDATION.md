# Does our engine score the same game BGO scored? (2026-07-27)

Branch: `score-validation`. New tools: `tools/bgo_rescore.py`, `tools/wonder_ab.py`.
Nothing in `engine/` is touched by this branch; `bash tools/gate.sh` is green
(GATE PASS, all six digests unmoved) and `python3 -m unittest discover -s
tests -q` is 393 tests OK (381 + the 12 new ones in
`tests/test_bgo_rescore.py`).

This answers proposals 1 and 2 of `docs/HUMAN_BASELINE.md`. That document's
own "What this cannot tell you" says the 84-vs-160 score comparison "is not a
clean skill measurement… nothing here independently verifies that our
end-of-game scoring matches BGO's." It does now.

## One-paragraph answer

**Our engine scores the same game.** On 43,847 turn snapshots reconstructed
from the 1,011 human journals it reproduces BGO's own printed culture,
science, food, consumption and resource numbers exactly on 34,733 of them
(99.1% on turns 1-5, falling only as the *replayer* drifts, not the engine),
every one of the 16 wonders' stage costs, the `+1 CA per completed wonder`
take surcharge including the Michelangelo exemption, and, of the fifteen Age
III scoring events, six at 100% and eleven at 86% or better on verified
reconstructions. **Our games are not short**: 20.0 rounds
against a human 19.4. Three real engine bugs fell out, all worth single-digit
culture — they do not explain a 76-point gap. **The score gap is a policy
fact, not a scoring fact**: the same engine, run with the 1-ply-lineage vector
instead of the quiescent champion, scores 139.8 [131.6, 148.3] against a human
159.5 [156.0, 163.0], and does it with 0.76 wonders per player. **Wonders are
neither broken nor a free lunch**: costs and surcharge are exactly right,
benefits are if anything *under*-implemented, and forcing a strong bot to
build human numbers of them costs it 34.3 ± 7.0 margin.

---

## 1. Method: replay the journal, ask our engine, diff against BGO

`docs/HUMAN_BASELINE.md` proposal 1 says "reconstruct one finished human
position by hand". Hand-reconstructing one position tests one position, and a
19-round game has ~40 actions per player, so the hand is at least as likely to
be wrong as the engine. `tools/bgo_rescore.py` does it mechanically instead,
for every seat of every game, and gets three independent oracles out of the
journal that a hand reconstruction would not have:

1. **Every `End turn` line prints that player's production**
   (`N culture (now C); N science (now S); N food - consumption: K; N
   resources`). That is five engine outputs per player per turn, 43,847 of
   them, on positions we can rebuild.
2. **Every `Impact of ...` line at game end prints each player's award** —
   i.e. `engine/events.py::scoring_culture` as computed by somebody else.
3. **The `End of game` line prints the final totals**, and BGO's own
   arithmetic (last culture + end-of-game impacts = printed score) checks out
   on 71.9% of rows without any modelling at all, which is the parse sanity
   check.

The replayer rebuilds each seat's tableau (workers per card, government,
leader, completed/flipped wonders, colonies, yellow bank) from the action
lines, builds a real `GameState`, and calls `effects.state_stats`,
`events.scoring_culture`, `effects.on_wonder_complete` and
`effects.end_of_game_bonus` on it.

### The cleanliness gate, and why it is the whole design

A disagreement between the replayer and BGO is ambiguous: it can be the
replayer losing a worker as easily as an engine bug. So a row only counts as
evidence about the *scorer* when the replay of that row is independently
verified:

* all five production numbers match BGO's own print-out that turn, **and**
* the seat's yellow tokens are conserved
  (`bank + unused + on-cards == 25 − 2 per age end + grants`), **and**
* no line the replayer cannot model (Annex, Infiltrate, Iconoclasm, Raid
  casualties, Terrorists, Barbarossa) touched the game.

That leaves 405 of 2,525 final positions (16.0%). Small, but they are *known
good* rather than assumed good, and the ranking events additionally require
every seat in the game to be clean.

**The gate is also a measurement, not just a filter.** Running our engine
against BGO's numbers on every turn of every game, with no filtering at all:

| quantity | our engine == BGO |
|---|---|
| culture production | 40,280 / 43,847 (91.9%) |
| science production | 42,322 (96.5%) |
| food production | 39,703 (90.5%) |
| food consumption | 40,183 (91.6%) |
| resource production | 42,259 (96.4%) |
| **all five at once** | **34,733 (79.2%)** |

and by turn index:

| | all five exact |
|---|---|
| turns 1-5 | 12,514 / 12,625 (**99.1%**) |
| turns 6-10 | 10,354 / 12,571 (82.4%) |
| turns 11-15 | 7,758 / 11,582 (67.0%) |
| turns 16+ | 4,107 / 7,069 (58.1%) |

**That decay is the signature of a drifting replayer, not of an engine bug.**
An engine that computed culture wrongly would be wrong on turn 3 too. An
engine that agrees with BGO on 99.1% of early positions and degrades smoothly
as reconstruction error accumulates is an engine that agrees.

### Two things this method found about the replayer, recorded so the next
### person does not rediscover them

* **The `-2 yellow tokens at each age end` rule is real and BGO applies it.**
  On one hand-traced game it looked like BGO did *not*, which would have been
  a large economy bug in `engine/game.py:164`. Run as a whole-corpus A/B over
  43,847 end-turn lines it is not close: consumption predicted correctly on
  **91.6% at `age_loss=2`, 68.7% at 1, 52.2% at 0**, and the residuals at 0
  are systematically one band low while at 2 they are symmetric (+1 × 1,351,
  −1 × 2,146). The rule stays. This is written down because the single-game
  version of this check produced a confident wrong answer.
* Four replay bugs each looked like an engine bug first, and each is pinned by
  a case in `tests/test_bgo_rescore.py`: leader names truncated at the first
  word (`elects William Shakespeare Leonardo Da Vinci dies` → `William`, the
  same failure `docs/HUMAN_BASELINE.md` records costing 39% of elections); an
  upgrade routing the worker through the unused pool and minting a yellow
  token per upgrade; `Warrior` (BGO's singular) not resolving to `Warriors`,
  923 lines in a 150-game sample, and invisible to a production check because
  units produce only strength; and `The Pyramids crumble` not resolving to
  `Pyramids`, so **no Ravages of Time flip was ever applied** — fixing that one
  alone moved culture agreement from 89.4% to 91.9% and turn-16+ agreement
  from 50.8% to 58.1%, which is a fair measure of how much of the residual in
  this document is still replayer and not engine.

---

## 2. Result: the end-of-game scorer

Clean rows only (n is the number of clean player-awards; "all n" is every row
including unverified reconstructions, for scale).

| Age III event | clean n | exact | % | all n | exact |
|---|---|---|---|---|---|
| Impact of Wonders | 78 | 78 | **100.0** | 565 | 565 |
| Impact of Government | 92 | 92 | **100.0** | 647 | 643 |
| Impact of Progress | 103 | 103 | **100.0** | 688 | 673 |
| Impact of Balance | 86 | 86 | **100.0** | 580 | 561 |
| Impact of Agriculture | 66 | 66 | **100.0** | 528 | 505 |
| Impact of Science (ranking) | 49 | 49 | **100.0** | 759 | 737 |
| Impact of Technology | 83 | 82 | 98.8 | 606 | 602 |
| Impact of Architecture | 73 | 68 | 93.2 | 554 | 464 |
| Impact of Variety | 87 | 81 | 93.1 | 585 | 508 |
| Impact of Competition | 66 | 61 | 92.4 | 455 | 397 |
| Impact of Colonies | 67 | 58 | 86.6 | 513 | 429 |
| **Impact of Industry** | 81 | 61 | **75.3** | 542 | 452 |
| Impact of Happiness | 94 | 66 | 70.2 | 640 | 442 |
| **Impact of Population** | 81 | 43 | **53.1** | 584 | 322 |
| Impact of Strength (ranking) | 40 | 26 | 65.0 | 742 | 510 |

Plus `effects.end_of_game_bonus` (Bill Gates): **411 / 420 exact**, with no
cleanliness filter at all.

Reading the rows that are not 100%:

* **Colonies (±3, symmetric)** and **Architecture / Variety / Competition
  (small, mixed sign)** are the replayer: a stolen colony (Annex) and missing
  military workers, which the five-rate gate cannot see because units produce
  *strength*, and strength is never printed outside a war.
* **Impact of Strength** residuals are ±10 × 7, exactly the 2p ranking table.
  The replayer models no tactics and therefore no armies, so it cannot rank
  strength. Not evidence about the engine either way. Contrast **Impact of
  Science**, the other ranking card, which is 43/43 once every seat is clean —
  the ranking machinery itself is right.
* **Happiness** is 70.2% with mixed-sign residuals (+2 × 10, −2 × 6). Happy
  faces are the one input the journal never prints, so the gate cannot verify
  them and this row is **unresolved**, not exonerated. Restricted to rows
  where our engine says discontent is 0 (which removes the other unverifiable
  input) it is **61/75 (81.3%)**, residuals +2 × 9, −2 × 4, +4 × 1 — still
  mixed sign, still open.
* **Industry and Population are real engine bugs.** They are the only two rows
  whose residuals are large and all one sign, and for both the corrected
  formula matches BGO nearly perfectly. See §3.

---

## 3. Three engine bugs, all small, none of them the score gap

Nothing was fixed on this branch — `tools/gate.sh` digests are unmoved on
purpose. These are handed over as findings.

### 3.1 `Impact of Industry` scores the resource *rating*, not mine production

`engine/events.py:393` reads `culturePerResourceProducedByMines` as
`v * s.resources`, the whole resource rating. The card says "the resources
produced by their mines (ignoring other bonuses)". Two things add resources
outside mines: **Bill Gates** (`resourcesPerLabEqualToLevel`) and
`Transcontinental Railroad`'s doubled mine worker (which per the FAQ *does*
count, being a mine).

Scoring mines-only + the Railroad's double against BGO: **81 / 81 exact**,
against our engine's 61 / 81. Residuals of the current code are +6 × 9,
+9 × 6, +4 × 2, +12 × 2, +7 × 1 — all positive, all Bill Gates lab levels.
**We over-score this card, by 4-12 culture, only for Bill Gates players.**

### 3.2 `Impact of Population` does not count unused workers

`engine/events.py:412` computes content workers as
`sum(t.workers for t in p.techs.values()) - discontent`, i.e. workers standing
on cards. Yellow tokens in the worker pool are workers too.

Adding `p.workers_free`: **68 / 81 exact** against our engine's 43 / 81 on
clean rows, and restricted further to the rows where our engine says
discontent is 0 (removing the one input the replayer cannot check),
**63 / 66 against 43 / 66**, with the alternative's only residuals being
+2 × 2 and −2 × 1 — one worker, mixed sign, i.e. replay noise. Current-code
residuals are −2 × 21, −4 × 8, −6 × 5, −8 × 2, −10 × 1 — every one negative and
every one an exact multiple of 2, i.e. a whole number of uncounted workers.
**We under-score this card by 2 culture per unused worker.**

### 3.3 Age III wonder completion bonuses ignore leader modifiers

`effects._one_time_culture` builds Hollywood and Internet from *printed*
`production` values. BGO uses the buildings' **effective** output. Against the
corpus (clean seats only, at the moment of completion):

| wonder | exact | residuals (ours − BGO) |
|---|---|---|
| Fast Food Chains | 131 / 139 | −2 × 6, −1 × 2 (replay drift) |
| First Space Flight | 167 / 178 | −1 × 10, −2 × 1 (replay drift) |
| **Hollywood** | **30 / 72** | −8 × 22, −6 × 11, −12 × 6 |
| **Internet** | **69 / 105** | −3 × 17, −4 × 6, −6 × 4 |

and the mismatches are perfectly explained by *which leader was in play*:

* Hollywood: **every** Charlie Chaplin (32/32) and William Shakespeare (7/7)
  completion is wrong; with any other leader it is exact.
* Internet: every Charlie Chaplin (13), William Shakespeare (4) and **Albert
  Einstein** (14/14) completion is wrong; **Sid Meier is 38/38 exact** —
  because `_one_time_culture` already special-cases Sid Meier and nobody else.

Chaplin doubles the best theater's culture, Shakespeare pays 2 per
library/theater pair, Einstein adds science to the best lab/library — all of
them modify exactly the per-building output these two wonders sum.
**We under-score the two biggest wonder payoffs in the game, by ~4.4 culture
per Hollywood and ~1.4 per Internet on average.** Note the sign: this bug
makes wonders *worse* in our engine than in the real game, which is the
direction that matters for §5.

### Size check

All three together are worth single-digit culture per game to a typical
position. They cannot make 84 into 160, and they do not change any conclusion
in `docs/HUMAN_BASELINE.md` about behaviour.

---

## 4. Game length: our games are not short

`docs/HUMAN_BASELINE.md` already reported this as "overlap"; it is confirmed
on a bigger sample and it is *not* the direction a scoring bug would want.

| | rounds |
|---|---|
| human corpus (1,011 games, from the journal's own round column) | median 19, mean **19.27** |
| human corpus (`tools/bgo_stats.py`, 2p only) | **19.43** [19.38, 19.49] |
| bot, 1-ply lineage vector, 1-ply search, 2p mirror (n=60) | **20.02** [19.87, 20.17] |
| bot, 1-ply lineage vector, quiescence (n=60) | 20.10 [19.95, 20.25] |
| bot, quiescent champion, 1-ply search (n=60) | 20.05 [19.92, 20.18] |
| bot, quiescent champion, quiescence `levels=1` (n=60) | **17.32** [16.40, 18.15] |

Three of the four configurations run *longer* than humans. The one short row
is the quiescent champion under its own training search, and the same vector
under 1-ply search runs 20.05 — so it is that policy's play, not the engine's
age/end-trigger timing, that shortens the game. The mechanism is not
established here. **Game length is not the gap**, and in the one place it does
move it moves for a bot whose score is *lowest*, i.e. it cannot be doing the
explanatory work either way for the other three.

---

## 5. The score gap is a property of the vector, not of the engine

This is the finding that reframes `docs/HUMAN_BASELINE.md` §"Bot vs human".
That document measured **one** policy: the quiescent champion at
`quiesce:...,levels=1`. `docs/TRANSFER_TEST.md` had already established that
this vector is a *suppression* engine that scores 111-125 while holding its
rival to 43-84, and that the 1-ply lineage vector is a *production* engine
scoring 160-212. Nobody had run the human-corpus comparison on the second one.

`tools/bgo_botmatch.py`, 2p mirror, n=60 games each, same seeds:

| | human | Q champion, quiescence (the HUMAN_BASELINE config) | Q champion, 1 ply | P 1-ply lineage, 1 ply | P, quiescence |
|---|---|---|---|---|---|
| **final culture** | **159.5** [156.0,163.0] | **64.7** [56.2,72.6] | 110.5 [104.8,116.5] | **139.8** [131.6,148.3] | 130.3 [121.9,138.6] |
| rounds | 19.43 | 17.32 | 20.05 | 20.02 | 20.10 |
| wonders completed | 2.74 | 0.41 | 0.28 | 0.76 | 0.53 |
| wonder stages | 8.77 | 1.86 | 1.49 | 3.12 | 2.45 |
| civil cards taken | 34.3 | 22.2 | 25.4 | 22.9 | 23.1 |
| % of takes at 3 CA | 4.5 | 22.3 | 22.0 | 23.2 | 24.1 |
| wars declared /player | 0.25 | 0.49 | 0.00 | 0.00 | 0.72 |
| colony bids | 3.22 | 1.83 | 11.41 | 0.07 | 0.07 |

Three things follow.

1. **"Our bot scores half what humans score" is a statement about one
   vector.** Swap the vector and the same engine, same search family, same
   scoring code produces 139.8 against a human 159.5 — a 20-point gap, not a
   76-point one. (The CIs still do not overlap; our best bot is genuinely
   below the human mean. But it is not half.)
2. **The wonder gap is in every configuration; the score gap is not.** All
   four build 0.28-0.76 wonders — 10% to 28% of the human 2.74 — while score
   ranges over 65-140 across exactly those four. The two do move together a
   little (the 139.8 bot has the most wonders), but nothing like enough:
   whatever is suppressing wonders in our ecosystem is mostly not what is
   suppressing score.
3. **So is the card-take profile.** 22-25 takes and 22-24% at 3 CA in all four
   configurations, across a 75-point score range. `docs/HUMAN_BASELINE.md`
   finding 2 ("a smaller civil-action budget spent impatiently") is real and
   universal in our bots — but this run gives no evidence that it is what
   costs the points, because it does not vary while score does.

Two smaller notes: our reproduction of the quiescent champion's score is
**64.7 [56.2, 72.6]** where `docs/HUMAN_BASELINE.md` reported **84.1
[73.7, 95.2]** on n=40. Those CIs do not overlap. Different generation
(231 vs 224) and different seeds; either the champion drifted downward on this
axis in seven generations or one of the two samples is unlucky. It is flagged,
not explained. And `P` at 1 ply essentially **stops colonising** (0.07 bids
against a human 3.22) while `Q` at 1 ply over-colonises (11.41) — the two
vectors are off-distribution in opposite directions on that axis.

---

## 6. Part 2: are wonders weak, or invisible?

### 6.1 The wonder rules and data are right

* **Stage costs.** Extracted from 18,307 human stage lines: for each wonder
  and each stage index, the maximum resource cost anybody paid (discounts only
  reduce, so the max is the printed price). **All 16 wonders, all 53 stages,
  match `data/cards_wonders_leaders.json` exactly. Zero mismatches.**
* **The `+1 CA per completed wonder` take surcharge.** For all 7,395 wonder
  takes in the corpus, `logged CA − (completed wonders at that moment)` lands
  in the legal row-slot range 1-3 for **6,980 (94.4%)**, and the 415 that do
  not are dominated by **Michelangelo** (380 of them) — the leader
  `actions.take_cost` already exempts. So the surcharge is implemented exactly
  as BGO implements it, exemption included, and it is **not** over-charging.
  (`engine/actions.py:79-89` also charges `p.destroyed_wonders`, which §2.4
  requires and which the corpus cannot test — Ravages of Time flips a wonder
  rather than destroying it.)
* **Benefits.** `Impact of Wonders` is 61/61 exact on clean rows and 565/565
  on *all* rows. The Age III one-time bombs are 94% exact for Fast Food Chains
  and First Space Flight and **under**-scored for Hollywood and Internet
  (§3.3).

So there is no rules or cost bug making wonders bad. If anything our engine
pays slightly less for them than the real game does.

### 6.2 The scripted A/B: forcing wonders

`tools/wonder_ab.py` wraps a policy and overrides it with probability
`--force` whenever a `wonder_step` is legal (largest available) or a wonder
sits in the row and none is in progress (cheapest slot). `--force 0` is the
unmodified bot. Seats are mirrored on the same deal, so the margin is paired
and the deal is the unit of error; ± is one SE.

**P, the 1-ply-lineage production vector, 1-ply search, 40 deals = 80 games per row:**

| force | own culture | rival | margin | win share | wonders | overrides/game |
|---|---|---|---|---|---|---|
| 0.00 | 155.2 ± 5.2 | 155.2 ± 5.2 | 0.0 ± 6.1 | 0.512 ± 0.056 | 0.71 | 0 |
| 0.10 | 145.2 ± 4.4 | 156.0 ± 5.0 | **−10.8 ± 5.6** | 0.412 ± 0.055 | 1.45 | 3.0 |
| 0.20 | 147.4 ± 4.5 | 155.8 ± 4.7 | −8.4 ± 6.0 | 0.388 ± 0.055 | 1.8 | 5.7 |
| 0.40 | 148.0 ± 5.0 | 154.1 ± 4.8 | −6.1 ± 6.4 | 0.463 ± 0.056 | 2.4 | 9.9 |
| 0.70 | 139.5 ± 5.2 | 162.9 ± 4.9 | −23.4 ± 6.1 | 0.312 ± 0.052 | 3.1 | 14.6 |
| 1.00 | 125.2 ± 5.6 | 159.4 ± 4.7 | **−34.3 ± 7.0** | 0.300 ± 0.052 | 3.8 | 17.7 |

**Q, the quiescent champion, `quiesce:levels=1`, 25 deals = 50 games per row:**

| force | own culture | rival | margin | win share | wonders |
|---|---|---|---|---|---|
| 0.00 | 64.9 ± 6.4 | 64.9 ± 6.4 | 0.0 ± 5.5 | 0.540 ± 0.071 | 0.40 |
| 0.20 | 71.6 ± 6.9 | 72.3 ± 5.8 | −0.7 ± 6.1 | 0.480 ± 0.071 | 0.88 |
| 0.50 | 80.8 ± 5.9 | 81.1 ± 5.4 | −0.3 ± 5.8 | 0.520 ± 0.071 | 1.46 |
| 1.00 | **85.7 ± 6.7** | 81.4 ± 5.8 | **+4.3 ± 7.0** | 0.560 ± 0.071 | 1.9 |

**The two vectors answer the question in opposite directions, and that is the
finding.**

* On **P** every dose is negative and the sign is consistent across six
  points; at full force it reaches human-scale wonder counts (3.8) and pays
  **34.3 ± 7.0 margin and 30 points of its own culture** for them. There is no
  hidden payoff here for a `levels=1` evaluator to be blind to. For this
  vector the answer is closer to **(a)**: at the economy this bot actually
  runs, wonders are worse than what it does instead.
* On **Q** forcing wonders **raises its own culture by 20.8** (64.9 → 85.7)
  and its margin never leaves zero: −0.7 ± 6.1, −0.3 ± 5.8, +4.3 ± 7.0 across
  the three doses, every one inside one SE of the null.
  Q's evaluator genuinely cannot see the wonder (**(b)** for this vector), but
  what it gains in culture it gives back in suppression: the *rival's* score
  rises by the same 16 points, because the actions went into wonders instead
  of into wars and aggressions. The league gates on `margin_share`
  (`docs/TRANSFER_TEST.md` §5), which pays twice for a stolen point and once
  for a produced one, so a change that is +21 own culture and 0 margin is
  invisible to the trainer by construction.

### 6.3 What this does and does not license

* Wonders are **not broken**: costs exact, surcharge exact, `Impact of
  Wonders` exact, one-time bombs correct for two of four and *understated* for
  the other two.
* Wonders are **not a free lunch our search is missing**, at least not for the
  strongest vector we have.
* **Wonders are not the score gap.** At full forcing Q reaches 1.9 wonders and
  85.7 culture; P at zero forcing has 0.71 wonders and 155. The correlation
  between wonders and score across these ten rows is not the story.
* **The override is crude and this bounds the claim.** It builds a stage
  whenever one is legal, including on turns where the wonder cannot finish,
  and it takes the leftmost wonder in the row rather than the best one. A
  competent wonder policy could plausibly do better than this one. What the
  A/B rules out is "there is a large payoff sitting there that a 1-ply search
  cannot reach"; it does not rule out "a *good* wonder plan is worth points".
  Pricing a *hand-written competent* wonder script, rather than a random
  override, is the obvious follow-up.
* **A plausible mechanism nobody has tested:** a wonder costs 1 civil action
  per stage, so a human's 8.8 stages is ~9 civil actions plus the take. Our
  bots take 22-25 cards against a human 34.3 on the same 19-20 rounds, i.e.
  they are ~10 civil actions poorer over the game — which is roughly the
  entire cost of a human's wonder programme. Under that story the wonder
  deficit is *downstream* of the civil-action deficit and forcing wonders
  without fixing the budget is exactly the wrong order of operations, which is
  what the P table looks like. This document does not test it.

---

## 7. Reproducing

```
tar xzf sources/bgo/journals.tar.gz -C /tmp/bgo

# §1-§3: replay the corpus through our engine
python3 tools/bgo_rescore.py --journals /tmp/bgo/journals
python3 tools/bgo_rescore.py --game 7520718 --trace Orange   # per-turn diff
for al in 0 1 2; do python3 tools/bgo_rescore.py --age-loss $al; done

# §4-§5: the four bot configurations (champions copied out of the live
# training dir first, because the trainer rewrites them mid-run)
cp experiments/league_state/champion_2p.json /tmp/Q2p.json
cp experiments/archive_preplan/league_state_1ply_20260726/champion_2p.json /tmp/P2p.json
for s in quiesce:/tmp/Q2p.json,levels=1 /tmp/Q2p.json /tmp/P2p.json quiesce:/tmp/P2p.json,levels=1; do
  nice -n 19 python3 tools/bgo_botmatch.py --players 2 --games 60 --seed 7000 \
      --spec "$s" --out /tmp/bm.tsv
  python3 tools/bgo_stats.py --tsv /tmp/human.tsv --vs /tmp/bm.tsv --players 2
done

# §6.2: the wonder A/B
nice -n 19 python3 tools/wonder_ab.py --spec /tmp/P2p.json --deals 40 \
    --force 0 --force 0.1 --force 0.2 --force 0.4 --force 0.7 --force 1.0
nice -n 19 python3 tools/wonder_ab.py --spec quiesce:/tmp/Q2p.json,levels=1 \
    --deals 25 --force 0 --force 0.2 --force 0.5 --force 1.0
```

Everything above ran `nice -n 19` alongside five live training workers and
another agent's PlanBot experiments on a 6-core box.

## 8. Limits

* **The replayer is the weak side of every comparison, deliberately.** 16.0%
  of final positions survive the cleanliness gate. Everything reported as an
  engine result is gated; everything gated out is gated out because the
  *replay* could not be verified, not because the engine disagreed.
* **Happy faces are unverifiable.** The journal never prints them, so
  `Impact of Happiness` (70.2%) is genuinely open and the strength ranking
  (65.0%) is untestable without modelling tactics, which the replayer does
  not do. If a third engine bug is hiding anywhere in the scorer, it is behind
  one of those two.
* **n = 60 games per bot configuration, 40/25 deals per A/B row.** The score
  differences quoted between vectors are 3-10 SE and safe; the *within*-vector
  dose response in §6.2 is not clean (P's −10.8, −8.4, −6.1 are within noise
  of each other) and only the sign and the endpoints should be leaned on.
* **2p only.** Nothing here was run at 3p or 4p.
* **`docs/HUMAN_BASELINE.md`'s behavioural findings are untouched.** This
  document validates the *arithmetic* and reframes the *score* comparison. It
  does not dispute that our bots build 3-7x fewer wonders, take 10 fewer
  cards, pay 3 CA five times as often, or revolt four rounds early — every one
  of those reproduced here on new samples and on a second vector.
