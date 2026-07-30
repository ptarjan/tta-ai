# Why the bot wastes civil actions

**Question asked (2026-07-26):** *"I'm really surprised they ever waste an
action. Isn't taking or playing a yellow card almost always worth it?"*

> **⚠ EVERY 4p NUMBER IN THIS DOCUMENT IS QUARANTINED (2026-07-30).** The 4p
> vector it was measured against — `analysis/frozen/champion_4p.json`, now
> renamed `analysis/frozen/champion_4p.DEGENERATE.json`, and its twin
> `experiments/frozen/champion_4p_strengthcheck.json` — reproduces **all 62
> informative weights** of `experiments/champion_4p.json` bit-for-bit,
> including `science = −6.08883`. That is the vector `docs/TRAINING_RUN.md`
> says never to warm-start from and that `docs/CULTURE_GAP.md` §8f measured at
> **20.1% against a 25% null** — a bot that loses to random seating.
> `refuse_if_degenerate_champion` was supposed to catch it and did not: it
> tested exact content, and the frozen copy is six generations later and
> differs on two keys (`colonies`, `pacts`). The guard now tests provenance
> over the informative keys and refuses it under any name.
>
> **The 4p rows below are not retracted — they are unreliable and left in
> place so they stay auditable.** They describe a known-degenerate bot. Do not
> quote them as facts about 4p play, and do not quote them as facts about the
> engine. The 2p and 3p numbers in this document are unaffected by *this*
> issue. See `analysis/frozen/README.md`.


**Verdict: BUG — and the player's instinct understates it.** At 2 players
**98.4%** of the turns where the champion throws away a civil action had at
least one affordable, legal action available, and in **60.1%** of them the bot
declined a move *its own evaluation scored as an improvement*. Only 1.6% had
nothing legal to do. Wasting actions is also far more expensive than assumed:
a variant tuned to pass more often scores 67 culture against 152 (§6).

**But the fixable cause is not the obvious one.** The eye-catching defect —
`end_turn` being scored a whole production phase ahead of its alternatives
(§1) — is real, yet removing it by any of five threshold settings made the bot
*significantly weaker* (§6). The real disease is that the evaluation is
**blind to card identity**: it compresses the entire hand to a count and a sum
of age levels, so it cannot prefer a good card to a bad one and taking any
card scores ≈ 0. Adding one term that values a card by what it *does* — with
the `end_turn` bug deliberately left in place — wins **72.5% ± 4.4%** of games
(p < 1e-5) and gains **+24 culture** at 2p (§7).

**Status: landed.** `hand_potential` is in `engine/bots/weighted.py` and on
`master`. It is validated at 2p only: at 3p the term is not significant, and
at 4p it regresses badly until that champion's degenerate weight vector is
re-seeded (§7 caveat, §5). **§11 says what to measure before the retraining
run** — in particular, do not assume the wasted-action rate itself has
dropped just because the bot got stronger.

Read §10 for the actionable summary; §8 for the ranked fix.

Evidence: `analysis/wasted_actions.py` (probe), `analysis/wasted_summary.py`
(aggregation), `analysis/passfix_duel.py` and `analysis/cardvalue_duel.py`
(A/B), 200 self-play games per player count plus ~4000 duel games.

---

## 1. The mechanism: `end_turn` is scored one production phase ahead

`WeightedBot.pick` applies each candidate move to a copy of the state and
scores the result. For every move except one, the copy is still *mid-turn*.
For `("end_turn",)`, `engine/game.end_turn` runs `economy.end_of_turn` —
the player collects **all** their food, resources, science and culture — and
then hands over to the next player.

So the 1-ply search compares "my board after taking a card" against "my board
after a full turn's income". Income wins, every time, and it wins by more the
better your economy gets.

`engine/bots/weighted.py` already knows about this; there is a weight for it:

```python
# search bias: value of the "end turn" move itself (its child state has
# already collected a production phase, which flatters it)
"end_turn_bias": -3.0,
```

**A constant cannot cancel a term that grows with your economy.** Measured,
per wasted-action turn:

| | 2p | 3p | 4p |
|---|---|---|---|
| mean *flattery* (eval of `end_turn` child − eval of the unmoved board) | **+12.57** | **+6.83** | −5.81 |
| champion `end_turn_bias` | −8.28 | −4.09 | −4.31 |
| **net head start for doing nothing** | **+4.28** | **+2.74** | (n/a, see §5) |
| mean value of the best move it declined | +0.48 | −0.23 | −16.86 |

The hill climb pushed `end_turn_bias` from −3.0 to −8.28 (2p) and −4.09 (3p),
i.e. it spent optimisation pressure fighting this artifact and still lost. It
cannot win: the flattery is +7.05 in Age I, +11.82 in Age III and **+26.28 in
Age IV** at 2p, so any single constant is far too weak late and too strong
early.

Meanwhile the moves it is competing against are worth **fractions of a
point** (mean best declined move: +0.48). A ±12 point bias decides
essentially every one of these decisions.

## 2. How much of the waste is legitimate?

"Legitimate" = the bot ended its turn with actions left because there was
genuinely nothing legal to spend them on. `legal_moves` already filters on
civil actions, resources, science, hand limit and urban limit, so *legal* and
*affordable* are the same set here.

| | 2p | 3p | 4p |
|---|---|---|---|
| turns ending with unspent CA (200 games) | 3557 | 2722 | 2252 |
| civil actions destroyed | 14229 | 10487 | 8131 |
| **no legal CA-spending move at all (legitimate)** | **1.6%** | **2.9%** | **14.3%** |
| had an affordable option and declined it | 98.4% | 97.1% | 85.8% |
| declined a move the eval scored **strictly positive** | **60.1%** | **44.9%** | 22.5% |
| would flip to a real move if `end_turn` were scored on the *unmoved* board | **98.0%** | **91.8%** | 25.8% |

The 60.1% / 44.9% row is the damning one. There is no defensible reading of
"my evaluation says this move improves my position, and I threw the action
away instead". No hand-limit story, no saving-for-a-wonder story: the bot was
not saving anything, unspent civil actions are simply destroyed at end of
turn.

## 3. It is worst exactly where the doc said it was worst

2p, by age:

| Age | turns | CA wasted | no legal option | mean flattery | a `take` was legal | hand was full |
|---|---|---|---|---|---|---|
| I | 50 | 93 | 0.0% | +7.05 | 36% | 96% |
| II | 1558 | 5334 | 0.8% | +11.03 | 87% | 27% |
| III | 1664 | 7439 | **2.2%** | +11.82 | **97%** | 3.8% |
| IV | 285 | 1363 | 2.5% | +26.28 | 95% | 5.3% |

The Age III number quoted in `HEURISTICS.md` (57.6% of civil actions wasted
at 2p) is **97% avoidable**: a card was legal to take on 1611 of those 1664
turns, and the hand was full on only 3.8% of them. Age I is the one age where
"nothing to do" is a real story (hand full 96% of the time) — and Age I is
also the age where the doc correctly reports almost no waste.

## 4. The yellow-card question specifically

The player's intuition about yellow (action) cards is right, and the bot is
not *singling them out* — it is blind to card identity altogether.

`features()` reduces the whole civil hand to two numbers: `hand_civil`
(count) and `hand_value` (sum of age level + 1). **Nothing about which card
it is.** Taking `Ocean Liners` and taking `Revolutionary Idea` produce
literally identical feature vectors. A 1-ply search cannot see what a card
does, because what it does happens on the *next* ply, when you play it.

Consequences, at 2p:

* mean eval delta of taking a card: **−0.155** (essentially zero, slightly
  negative). Taking a yellow: −0.082. Taking a non-yellow: −0.191. So yellow
  is very slightly *preferred* — the problem is that all takes are worth ~0.
* the champion's `hand_value_late` is **−0.783**, so in Age III/IV the
  evaluation believes holding a card is actively bad. An Age III take scores
  a flat **−0.67** regardless of the card.
* **31.9%** of all 2p wasted-action turns had a yellow card in hand that was
  legal to play right then, and it was declined (mean eval delta −0.158).
  65.9% had a yellow card in hand at all.

So: "isn't taking a yellow card almost always worth it?" — yes, and the bot
scores it at approximately **zero**, then loses the comparison to a
+12-point phantom. Two independent defects compounding.

For contrast, the moves the eval *can* see the value of are the ones it
declines most absurdly: mean declined `develop` = **+10.7**, `wonder_step` =
**+8.9**, `build` = **+2.9**. Those are turns where the bot passed up a
double-digit self-assessed gain because the production phase outbid it.

## 5. 4 players is a different, additional bug

At 4p the flattery is *negative* and only 25.8% of decisions would flip, so
§1 is not the main story there. The 4p champion's weights are degenerate:

```
civil_actions -2.86   hand_civil -0.68   num_techs -0.41
```

Playing cards is scored catastrophically: mean declined `play_action`
**−26.1**, `play_leader` **−33.2**, `destroy` **−23.4**. The result is a
feedback loop — the bot refuses to play cards, so its hand fills (**83.8%**
of Age III wasted-action turns are at the hand limit), so `take` becomes
illegal (legal on only 16% of those turns), so **14.3%** of its wasted
actions genuinely have nowhere to go. The waste is real but it is a
*symptom*; the disease is upstream in the 4p weight vector.

---

## 6. The obvious fix makes the bot WORSE

This is the part that changes the recommendation, so it is reported in full.

Two candidate fixes were implemented in `analysis/passfix_duel.py` (nothing in
`engine/` was touched) and duelled against the unmodified champion, mirror
match, seat-rotated, on a frozen weight snapshot (`analysis/frozen/`, 2p
gen 220) so the live hill climb could not move the target:

* **`PassFixBot`** — price `end_turn` on the *unmoved* board (the honest "what
  is my position worth if I stop here"), plus a threshold `eps`.
* **`HorizonBot`** — the more principled version: roll *every* candidate
  forward through the same production phase, so all moves are priced as "what
  is my board worth at the end of this turn if I do X". This removes the
  asymmetry rather than trying to cancel it with a constant.

| bot | win rate vs champion @2p | n |
|---|---|---|
| `passfix`, eps 0.0 | **38.4% ± 4.8%** | 400 |
| `passfix`, eps −0.05 | **39.8% ± 4.8%** | 400 |
| `horizon`, eps −0.01 | **29.8% ± 4.4%** | 400 |
| `horizon`, eps +4.0 (pass *more*) | **11.0% ± 4.3%** | 200 |
| (null) | 50.0% | |

**Every attempt to fix the waste by adjusting *when to pass* makes the bot
significantly weaker.** Mean culture drops from 127.5 to 113.2. That is not
noise; it is 10+ points outside the interval.

Note the last row, which is the control: pushing the threshold the *other*
way, so the bot passes even more often, is catastrophic — 11.0% win rate,
67.3 culture against 152.1. **Wasting actions is enormously expensive.** The
player's intuition is not merely correct, it is correct by a huge margin;
the champion is leaving a great deal on the table. The problem is that you
cannot capture it by simply lowering the bar for acting.

### Why: it is not the passing rule that is broken

The behavioural measurement explains it. Waste rate at 2p, 60 games:

| bot | turns ending with CA unspent | CA wasted / turn |
|---|---|---|
| champion (buggy) | 42.6% | 1.79 |
| `passfix` eps −0.05 | **66.2%** | **2.34** |
| `passfix` eps −2.0 | 19.1% | 0.48 |

Removing the flattery at eps ≈ 0 does not even reduce the waste — it *raises*
it, because the bot now spends its early actions on marginal moves, reaches
different (worse) positions, and still refuses Age III takes since
`hand_value_late` is −0.78 regardless.

The +12 phantom was incidentally acting as a **move-quality filter**: only
moves the evaluation is *confident* about cleared it — `develop` (+10.7),
`wonder_step` (+8.9), `build` (+2.9) — while everything it cannot actually
rank — `take` (−0.16, and identical for every card in the row), `pop`
(−0.06), `destroy` (−4.95) — was screened out. Lower the bar and the bot
starts acting on evaluation *noise*; raise it and the bot does nothing at
all (11%). Neither direction is the answer, because **the threshold was
never the real variable.** What is broken is the bot's ability to tell one
action from another.

So this is a **compensating-errors** situation, and the second error is the
one that matters:

> `features()` reduces the entire civil hand to `hand_civil` (a count) and
> `hand_value` (sum of age level + 1). The evaluation is **blind to card
> identity**. It cannot prefer a good card to a bad one, and a 1-ply search
> cannot see a card's payoff because that lands on the *next* ply, when you
> play it.

That is why taking a card scores ≈ 0. The bot is not undervaluing yellow
cards specifically — it cannot value *any* card. Refusing to act is its
least-bad policy given an evaluation that cannot tell it what acting is
worth.

## 7. Fixing the root cause instead — and this one works

`analysis/cardvalue_duel.py` adds exactly one term to the evaluation: for
every card still in hand, a discounted estimate of what it would be worth if
played, priced through the **same weight vector** (a lab's science production
via `science_rate`, an action card's `gainScience` via `science`, a wonder's
`civilActions` via `civil_actions`). No new hand-tuned constants, and it
gives cards distinguishable values where `hand_value` gave every Age I card
the same number:

```
Theology +6.45   Pyramids +6.11   Philosophy +3.33
Bronze   +0.92   Ocean Liners +3.03   Warriors −0.57
```

Crucially this changes **nothing** about the search or the passing rule. The
`end_turn` flattery and `end_turn_bias = −8.28` are left fully in place. The
only difference is that the bot can now tell a good card from a bad one:

| bot (champion search, bug left in) | win rate vs champion @2p | mean culture | n |
|---|---|---|---|
| `cardvalue`, disc 1.0 | **63.2% ± 4.7%** | 120.5 vs 107.7 | 400 |
| `cardvalue`, disc 0.5 | **63.2% ± 4.7%** | 123.8 vs 110.4 | 400 |
| `cardvalue`, disc 0.25 | **67.2% ± 4.6%** | 133.2 vs 110.8 | 400 |
| `cardvalue`, disc 0.125 | **69.6% ± 4.5%** | **137.8 vs 117.0** | 400 |
| `cardvalue`, disc 0.0 (control) | 50.0% ± 6.9% | 132.1 vs 132.1 | 200 |
| **landed in `weighted.py`, `hand_potential` 0.125** | **72.5% ± 4.4%** | **138.1 vs 114.2** | 400 |
| (null) | 50.0% | | |

The disc = 0 row is the control: with the term switched off the challenger is
byte-identical to the champion and the harness returns *exactly* 50.0% with
identical mean culture, so the effect above is not a seating or harness bias.
The last row is the shipped implementation in `engine/bots/weighted.py`
(p < 1e-5), which also clamps costs — see below.

**Combining the two fixes is worse than the card fix alone:** card valuation
*plus* same-horizon scoring scores **39.8% ± 6.7%** (n=200), against 69.6% for
the card fix on its own. The `end_turn` artifact must stay. There is a comment
on `end_turn_bias` in `weighted.py` recording this, because it is exactly the
kind of thing a later reader "fixes".

**+20 points of win rate and +21 culture, from one term, with the `end_turn`
bug untouched.** That is the confirmation that card-identity blindness — not
the horizon artifact — is the disease. It also explains why the hill climb
drove `hand_value_late` to −0.78: given a bot that could never turn a card
into anything, "cards in hand are bad" was a *correct* thing to learn.

The best discount is small (0.125–0.25) and the curve falls off above it,
which fits the mechanism: the term does not need to price a card accurately,
it only needs to **break the tie** between cards that `hand_value` scores
identically. A small nudge is enough; a large one starts overriding the rest
of the evaluation.

### Caveat: this is a 2-player result, and 4p actively regresses

| | win rate | null | n |
|---|---|---|---|
| `cardvalue` disc 0.25 @2p | **67.2% ± 4.6%** | 50.0% | 400 |
| `cardvalue` disc 0.25 @3p | 35.8% ± 4.7% | 33.3% | 399 |
| `cardvalue` disc 0.25 @4p (before the cost clamp) | **9.7% ± 2.7%** | 25.0% | 400 |

The 4p collapse is diagnostic rather than damning, and it is worth
understanding because it is the same disease as §5. Card potential is priced
*through the weight vector*, so a degenerate vector produces degenerate
prices. The 4p champion has `science` = −6.09, which flips the sign of the
`− techCost × w[science]` cost term: expensive cards become bargains.
`Alchemy` scored **+67.04** under the 4p weights against **+5.86** under the
2p ones, and the bot chased the most expensive card it could see.

The shipped version therefore prices costs through `max(0, w)` — paying a
cost can never read as a gain. That leaves 2p bit-identical (its stock
weights are already positive) while removing the sign inversion. It does
*not* rescue 4p on its own, because that vector's `science_rate` is +22.5 and
still wildly distorts the gain side; **the 4p weights need re-seeding (§8
step 3) before this term can be trusted there.**

At 3 players the term is **not significant** — the interval covers the
null and mean culture is slightly *down* (74.5 vs 81.8). So the fix is
demonstrated at 2p, not universally. That is consistent with the rest of this
document: the 3p champion wastes less to begin with (2722 wasted-action turns
vs 3557) and its `hand_value_late` is −0.395 rather than −0.78, so it had
less of this particular disease to cure. It does **not** license shipping the
term untuned at every player count — see §8 step 1.

## 8. Ranked fix

1. **Make the evaluation see what a card does (root cause). VALIDATED at 2p:
   +20 points of win rate (69.6% ± 4.5%) on its own, with the `end_turn` bug
   left in place.** Score a card in hand by the features it would add if
   played, discounted — 0.125 was the best of the four tried and the curve
   falls off above it. `analysis/cardvalue_duel.py` is a working reference
   implementation; folding it into `weighted.features()` is the real fix.
   **Tune the discount per player count and re-measure before shipping it at
   3p/4p** — at 3p the same term was not significant (§7), so this is a
   demonstrated 2p win and an untested change elsewhere.
2. **Only then remove the horizon artifact**, as `HorizonBot`'s same-horizon
   scoring rather than a constant, and **re-run the hill climb**.
   `end_turn_bias` (−8.28) and `hand_value_late` (−0.78) are fitted to the
   present bug and must be retrained, not carried over. Doing this step
   *without* step 1 is a measured 10–20 culture regression (§6).
3. **Investigate the 4p champion's weight vector separately** (§5). `science`
   = −6.09 makes gaining 4 science score −24, which is why playing
   `Revolutionary Idea` is valued at −36.85. `workers` = −1.94 and
   `civil_actions` = −2.86 are equally indefensible. This looks like a
   degenerate hill-climb basin, not a search artifact, and it should be
   re-seeded from defaults.
4. **Do not** simply retune `end_turn_bias`. It is a constant fighting a term
   that scales with the economy (+7.05 in Age I, +26.28 in Age IV); no value
   of it is right for more than one age, and both directions were measured
   worse (§6).

## 9. Reproducing

```bash
python3 analysis/wasted_actions.py --players 2 --games 200 \
    --champion analysis/frozen/champion_2p.json --out /tmp/wasted_2p.json
python3 analysis/wasted_summary.py /tmp/wasted_2p.json
python3 analysis/cardvalue_duel.py --players 2 --games 400 \
    --champion analysis/frozen/champion_2p.json --mode plain --disc 0.25
```

`python3 -m unittest discover -s tests -q` → 58 tests, OK (there is no pytest
in this environment). The investigation itself touched no engine file; the
one engine change is the landed `hand_potential` term in
`engine/bots/weighted.py`, which can be switched off by setting that weight
to 0.0 (the control run confirms 0.0 is byte-identical to the old bot).

To A/B the landed term at any player count:

```bash
python3 - <<'EOF'
from experiments.arena import duel
from engine.bots.weighted import load_weights
champ = load_weights("analysis/frozen/champion_2p.json")
a = dict(champ, hand_potential=0.125)
b = dict(champ, hand_potential=0.0)
print(duel(a, b, 2, 400)["win_rate"])
EOF
```

**Measurement environment.** This ran against a shared checkout that other
agents were changing underneath it, so two things are worth recording:

* `7d40f53` corrected the Age I/III military card counts partway through.
  The 2p diagnostic was re-run afterwards and reproduced almost exactly
  (3553 wasted-action turns / 14183 CA against 3557 / 14229), so none of
  §1–§5 depends on which side of that fix it was measured on.
* another agent added a `has_unit` feature and weight to
  `engine/bots/weighted.py` during the duels. Every duel here is a mirror
  match in which **both** sides load the same weights and run the same
  `evaluate`, so the term applies symmetrically and the A/B comparisons are
  unaffected; only the absolute culture totals shift between runs, which is
  why the champion's mean culture varies (107–152) across tables.

All duels use `analysis/frozen/`, a snapshot of the champions taken before
the experiments, so the live hill climb could not move the target mid-run.

---

## 10. Verdict

**Is the "declines its own improvement" finding real? Yes, and it reproduces.**
At 2p, **98.4%** of the turns where the champion destroys a civil action had
an affordable legal move available, and in **60.1%** of them (44.9% at 3p) the
bot declined a move its *own* evaluation scored above doing nothing. Only
**1.6%** had genuinely nothing legal to spend on. Independently re-measured on
the current deck after the `7d40f53` military-count fix, every headline number
came back the same: 3553 wasted-action turns (was 3557), 14183 civil actions
destroyed (was 14229), **59.9%** declining a self-scored improvement (was
60.1%), 2.2% with no legal option (was 1.6%), mean flattery +12.41 (was
+12.57). This finding is not an artifact of one sample or one deck version.

**The player's instinct is right, and by a larger margin than expected.** The
control experiment settles it: a bot tuned to pass *more* often scores 67.3
culture against the champion's 152.1 and wins 11% of games. Actions are worth
an enormous amount, and the champion is leaving a great deal on the table.

**But the visible defect is not the root cause.** `end_turn` is scored on a
child state that has already banked a production phase — worth +12.6
evaluation points on average at 2p and +26.3 in Age IV — while real moves are
worth fractions of a point. That is a genuine search artifact, and
`end_turn_bias` is a constant that cannot cancel a term which scales with the
economy. Yet removing it, by two different principled methods at five
thresholds, made the bot **significantly weaker every time** (11.0%–39.8% win
rate against a 50% null). Measured, not assumed.

**The root cause is card-identity blindness, and fixing it works.**
`features()` compresses the whole civil hand to a count and a sum of age
levels, so two different cards are literally the same feature vector: taking
any card scores ≈ 0 and the search has no basis to prefer a good one. Adding
a single term that values a card by *what it does* — with the search, the
`end_turn` flattery and `end_turn_bias` all left exactly as they are — wins
**67.2% ± 4.6%** of games and gains **+22 culture** (§7). The waste was a
symptom of an evaluation that could not tell the bot what acting was worth;
the passing rule was never the real variable.

**Actionable order** (detail in §8): (1) fold card-identity valuation into
`weighted.features()` — validated, +17 points of win rate on its own;
(2) *then* remove the horizon artifact via same-horizon scoring and re-run the
hill climb, since `end_turn_bias` (−8.28) and `hand_value_late` (−0.78) are
fitted to the present bug; (3) re-seed the 4p weight vector, which is
degenerate for unrelated reasons (§5). **Do not ship (2) without (1)** — alone
it is a 10–20 culture regression.

**Still open:** whether (1) and (2) together beat (1) alone, and whether a
re-trained champion recovers more than the +22 culture measured here. Both
need a hill-climb run, which is out of scope for this investigation.


---

## 11. What to do before the retraining run

The fix is landed (`hand_potential` in `engine/bots/weighted.py`). The
question is whether to re-measure first or retrain immediately. **Re-measure
first.** Three reasons, in order of how much compute they save:

**1. Several current weights are compensations for the blindness, and will
mislead a climb that starts from them.** `hand_value_late` = −0.78 is the
hill climb correctly learning "cards this bot holds never become anything" —
a true statement about the *old* code and a false one about the new. Same for
`end_turn_bias` = −8.28. Seeding a fresh climb from the current champions
carries those compensations forward and spends the run un-learning them.
Seed 2p/3p from the champions if you like, but **4p must be re-seeded from
defaults** (§5, §7): its vector is degenerate on its own terms and the new
term amplifies it.

**2. The mechanism of the +24 culture is not yet known, and it changes what
you should expect.** It is tempting to assume the bot now takes *more* cards.
The arithmetic says otherwise: at 0.125, `Theology` adds only ≈ +0.81 to a
take, which still does not clear the ~+4.3 net head start `end_turn` enjoys
(§1). So the gain plausibly comes from choosing *better among* the cards it
already takes and develops, not from taking more of them. If that is right,
**the wasted-action rate may barely move even though the bot got much
stronger** — and anyone who re-runs the §2 measurement expecting it to drop
will misread the result. This is cheap to settle (one probe run) and
expensive to guess wrong about.

**3. The book-bot gap is the cleanest test of the hypothesis.** A "book"
opponent plays sound *card priorities*, which is precisely the thing the old
evaluation could not represent at all. If card-identity blindness is the main
story, the 62.9% book-bot advantage should shrink materially against the
fixed bot. If it does not, there is a second large defect still outstanding
and it is much better to know that *before* committing to a long run rather
than attributing the residual to under-training.

There is also a fourth, weaker reason: if card-related weights previously
could not learn anything real, then several of the weights that measured as
noise were not noise-because-unimportant but noise-because-unlearnable. The
fitness landscape has genuinely changed shape, so prior conclusions about
which weights matter should be treated as provisional.

**Recommended order:** (a) re-run the §2 wasted-action probe against the
fixed bot; (b) re-run the book-bot benchmark; (c) re-seed 4p from defaults
and tune `hand_potential` per player count; (d) then start the long run
against the diverse pool. Steps (a) and (b) are minutes of compute against a
run measured in hours.
