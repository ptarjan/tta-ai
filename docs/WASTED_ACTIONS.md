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

*(§6 fix ranking and the A/B validation follow; see git history for the
in-progress version.)*
