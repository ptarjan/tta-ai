# Why the bot wastes civil actions

**Question asked (2026-07-26):** *"I'm really surprised they ever waste an
action. Isn't taking or playing a yellow card almost always worth it?"*

**Verdict: MIXED, but overwhelmingly a BUG at 2p and 3p.** The player's
instinct is right. At 2 players **98.4%** of the turns where the champion
throws away a civil action had at least one affordable, legal action it could
have taken instead, and in **60.1%** of them the bot declined a move *its own
evaluation scored as an improvement*. That is not judgement, it is a search
artifact. At 4 players the picture is genuinely different and the root cause
is a different (also real) defect — see §5.

Evidence: `analysis/wasted_actions.py` (probe) + `analysis/wasted_summary.py`
(aggregation), 200 self-play games per player count with the current
champions, 8531 wasted-action turns in total.

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

## 6. Trying to fix it — and why the obvious fix makes the bot WORSE

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
| (null) | 50.0% | |

**Every fix that removes the artifact makes the bot significantly weaker.**
Mean culture drops from 127.5 to 113.2. That is not noise; it is 10+ points
outside the interval.

### Why: the bug is load-bearing

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

The deeper reason is that **the flattery was accidentally doing a useful job:
it is a move-quality filter.** With a +12 phantom in front of it, only moves
the evaluation is *confident* about get played — `develop` (+10.7),
`wonder_step` (+8.9), `build` (+2.9). Everything the evaluation cannot
actually rank — `take` (−0.16, and identical for every card in the row),
`pop` (−0.06), `destroy` (−4.95) — is filtered out. Drop the threshold to
zero and the bot starts acting on evaluation *noise*.

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
worth. **The user's instinct is correct about the game and the bot is wrong;
but the wasted action is a symptom, and deleting the symptom without curing
the cause loses 10 culture a game.**

## 7. Ranked fix

1. **Make the evaluation see what a card does (root cause).** Score a card in
   hand by the features it would add if played — its tech's production, its
   wonder's culture, its action card's gains — discounted for the actions and
   science it still needs. Until this exists, no `end_turn` change can help,
   because there is nothing accurate to spend the freed actions on. This
   subsumes the `hand_value_late = −0.78` pathology, which is the hill climb
   correctly learning "cards this bot holds never become anything".
2. **Then remove the horizon artifact**, preferably as `HorizonBot`'s
   same-horizon scoring rather than a constant, and **re-run the hill climb**.
   `end_turn_bias` must be retrained, not carried over: its current −8.28 is
   fitted to cancel a +12 phantom that would no longer exist. Doing step 2
   without step 1 is a measured 10-culture regression.
3. **Investigate the 4p champion's weight vector separately** (§5). `science`
   = −6.09 makes gaining 4 science score −24, which is why playing
   `Revolutionary Idea` is valued at −36.85. `workers` = −1.94 and
   `civil_actions` = −2.86 are equally indefensible. This looks like a
   degenerate hill-climb basin, not a search artifact, and it should be
   re-seeded from defaults.
4. **Do not** simply retune `end_turn_bias`. It is a constant fighting a term
   that scales with the economy (+7.05 in Age I, +26.28 in Age IV); no value
   of it is right for more than one age.

## 8. Reproducing

```bash
python3 analysis/wasted_actions.py --players 2 --games 200 \
    --champion analysis/frozen/champion_2p.json --out /tmp/wasted_2p.json
python3 analysis/wasted_summary.py /tmp/wasted_2p.json
python3 analysis/passfix_duel.py --players 2 --games 400 \
    --champion analysis/frozen/champion_2p.json --mode horizon --eps -0.01
```

`python3 -m unittest discover -s tests -q` → 58 tests, OK (there is no pytest
in this environment). No file under `engine/` was modified by this work.

---

## 9. Verdict

**Is the "declines its own improvement" finding still real? Yes.** 60.1% of
2p wasted-action turns (44.9% at 3p) are turns where the bot's *own*
evaluation scored an available move above doing nothing, and it threw the
action away instead. That number is a direct comparison of the bot's stated
preference against its actual choice, so no later result can explain it away.
Only 1.6% of 2p wasted actions had genuinely nothing legal to spend on. The
player's instinct — *taking or playing a card is almost always worth it* —
is correct about Through the Ages, and the bot is wrong.

**But the cause is not the thing that looks like the cause.** The visible
defect is that `end_turn` is scored on a child state that has already banked
a production phase (+12.6 eval points on average at 2p, +26.3 in Age IV),
against which real moves worth fractions of a point cannot compete. Removing
that asymmetry — by either of the two principled methods, at four different
thresholds — makes the bot **significantly weaker** (29.8%–39.8% win rate vs
a 50% null, ~15 culture per game). Measured, not assumed.

**The actual root cause is card-identity blindness.** `features()` compresses
the civil hand to a count and a sum of age levels. Two different cards are
literally the same feature vector, so taking any card scores ≈ 0 and the
search has no basis to prefer a good one. The production flattery was
accidentally functioning as a *move-quality filter*: it admitted only moves
the evaluation could confidently price (`develop` +10.7, `wonder_step` +8.9,
`build` +2.9) and screened out the ones it could not (`take` −0.16, `pop`
−0.06). Delete the filter without fixing the blindness and the bot spends its
newly freed actions on noise. Two bugs were partially cancelling; removing
one alone is a regression.

**What would fix it properly**, in order — full detail in §7:

1. Value a card in hand by *what it does* (its production, its effects, its
   gains), priced through the existing weight vector. `analysis/cardvalue_duel.py`
   prototypes this; the estimates discriminate sensibly (Theology +6.45,
   Pyramids +6.11, Bronze +0.92, Warriors −0.57) where `hand_value` gives
   every Age I card the same number.
2. *Then* remove the horizon artifact via same-horizon scoring, and **re-run
   the hill climb** — `end_turn_bias` (−8.28) and `hand_value_late` (−0.78)
   are fitted to the bug and must not be carried over.
3. Re-seed the 4p champion; `science` = −6.09, `workers` = −1.94 and
   `civil_actions` = −2.86 are a degenerate basin, not a search artifact (§5).

**Do not** ship step 2 alone. It is a measured ~15-culture-per-game
regression, and this document exists because that is not obvious from the
symptom.

**What is NOT yet established:** that step 1 actually recovers the loss. The
prototype in `analysis/cardvalue_duel.py` is written and its card estimates
are sane, but its A/B against the champion had not finished when this was
written, and in any case the honest test of steps 1+2 is a *re-trained*
champion, not the current weights with a new scorer bolted on.
