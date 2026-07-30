# The military discard is a decision (2026-07-30)

`engine/economy.py` end-of-turn step 1 was `hand_military.pop(0)` — first in,
first out, no decision. `docs/RULES_SPEC.md` §6.6 step 1 says of that exact
step: *"Only step requiring a decision."* The engine was taking the decision
away from the player and answering it with the worst rule of thumb available:
throw away the oldest card.

Everything here is base game (2015 "A New Story of Civilization").

## 1. The rule, checked against the rulebook and not against the spec line

`sources/ubg_subsequent-rounds.txt:182` — the End-of-turn Sequence page:

> **Discard Excess Military Cards.** Your number of red tokens defines the
> maximum number of military cards you are allowed to have after this step.
> If necessary, you must discard military cards so that your total is not
> greater than the number of red tokens you have. They are discarded face down
>
> **Streamlining The Game.** Once you have **decided which military cards to
> discard**, the rest of your turn is automatic. That is, it requires no more
> decisions. The next player may start his or her turn as soon as you finish
> discarding.

So the spec line is right, and the rulebook says something slightly stronger
than the spec does: this is not merely *a* decision, it is *the* decision — the
reason the rest of `end_of_turn` may stay straight-line code, and the reason
the next player may not start until the discarding is done. Both of those are
now properties of the engine rather than accidents of it (§3).

## 2. Size of the violation

`tools/discard_census.py`, 12 games of 2p WeightedBot self-play under
`DEFAULT_WEIGHTS`, 494 player-turns, answering every discard the way the old
engine did:

| | |
|---|---|
| cards discarded by step 1 | 368 — **30.7 per game, 0.75 per player-turn** |
| of which are real decisions (≥2 distinct cards in hand) | 367; exactly 1 auto-resolved |
| mean distinct options offered | 4.17 |
| firings where FIFO pitched a card better than the worst available | 23.7% |
| firings where FIFO pitched the **sole best defender** in hand while a strictly worse card was available | **20.7%** |
| defence points thrown away that way | 200 — **16.7 per game** |

Two notes on provenance, because this document was started from a handoff and
the handoff's numbers do not all replicate. The harm figure does: 20.7% here
against 19% handed over. The **rate** does not — I measure 0.75 firings per
player-turn, not the ~3.2 I was given, i.e. ~31 per game rather than ~129. The
mechanism in the handoff (limit 2 under Despotism against a draw of up to 3)
overstates the churn, because step 4 draws `min(3, military_actions
remaining)` — a bot that spends its military actions draws fewer than 3 — and
because cards also leave the hand by being played. It is still a decision on
three quarters of all player-turns, with four options on average, so nothing
about the conclusion changes; but the number quoted here is the measured one.

## 3. The fix

The machinery already existed and was simply never invoked: `_q_discard_military`,
the `push_choice` tag `discard_military`, its resolver `_c_discard_military`,
and BookBot's preference function for the tag. Only the end-of-turn caller was
missing.

The one structural problem is that `end_of_turn` is a phase transition, not an
action: `game.end_turn` ran the whole §6.6 sequence and then advanced the turn,
with no way to suspend in the middle. So:

* `economy.end_of_turn` returns **False** when step 1 pushed a choice and the
  sequence is suspended (steps 2–5 have not run), **True** when it completed.
  Step 1 is idempotent — it re-reads the hand limit — so *re-entry is the whole
  resume mechanism*.
* `game._resume_end_turn` queues an `end_of_turn` deferred item when it gets
  False. `apply_pending` already drains the queue once a decision resolves, so
  `interact._q_end_of_turn` lands back in `_resume_end_turn` and the sequence
  continues — possibly suspending again for the next discard.
* The turn does **not** advance while the decision is outstanding, so
  production, the uprising check and the hand-off all stay strictly after the
  discard, which is what the rulebook's "the next player may start as soon as
  you finish discarding" requires.

`push_choice(auto=True)` still resolves a one-option choice without a decision,
so a hand of five copies of one card discards silently — there was nothing to
choose between.

## 4. What policy each bot gets, and why it is mostly not a new policy

The important property is that **four of the five bots need no new code**: they
already score `("choose", i)` by cloning the state, applying the move and
asking the evaluator they already use. A policy derived that way cannot drift
from the bot's own valuation; a hand-written table can.

| bot | how it answers the new choice | source of the valuation |
|---|---|---|
| WeightedBot | clone + apply + `weighted.evaluate` (`weighted.py:1434`) | its own weight vector |
| QuiescentBot | same, then drains pending to quiescence (`quiescent.py:356`) | same evaluator |
| PlanBot / NeuralPlanBot | pending decisions take the 1-ply path (`plan.py:174`) into `evaluate` | same evaluator |
| NeuralBot | clone + apply + `encode` + value net (`neural_bot.py:74`) | the network |
| BookBot | its existing hand-written tag table (`book.py:795`), written for this tag years before it was ever invoked | itself |

BookBot's table was already correct for this decision (pitch events first at
3.0, keep tactics at 0.0, bonus cards at 0.4) and is left alone.

### 4.1 The one place a policy had to be supplied: the tie-break

`weighted.evaluate` sees the military hand only through `hand_mil_value`, a sum
of `age + 1` — every military card of an age is interchangeable to it. That is
a documented blind spot (`docs/CARD_BLINDNESS.md` §3 item 5: `hand_potential`
walks `hand_civil` only, so `_card_yields` is never called for a tactic, war,
aggression, territory or bonus card). Same-age options therefore **tie**, and
every argmax in this project resolves a tie to the lowest index.

Option order was `sorted(set(hand))` — alphabetical. Under a tie that means the
discard is chosen by spelling, which would pitch `Military Bonus (defense 6 /
colonization 3)` ahead of a spent event on nothing but the letter M. That is
not FIFO and it is not better than FIFO; it is arbitrary.

`interact.discard_options` therefore orders the options **least defensively
useful first**, using `defense_points` — the same arithmetic `_defense_move`
uses to resolve an actual defence (§5.4.4: bonus cards 2/4/6, every other
military card the flat +1 of a face-down card). `_defense_move` now calls it,
so the two cannot disagree. The fallback becomes "pitch the card that defends
least", which is derived from the engine's own combat rules rather than
invented, and any bot that *can* discriminate overrides it. It also gives
BookBot a free improvement it did not have to be told about: within its `bonus`
bucket the defence-2 card now goes before the defence-6 one.

This is deliberately **not** a new evaluator feature. Adding `defenseBonus` to
`_card_yields` or a defence term to the feature vector would change every
position's score, not just the discard, invalidate the cached pool weights, and
collide with the card-pricing lane that owns those files.

### 4.2 Does it interact with the war/aggression machinery?

Yes, in the direction you would hope and not by much. `quiescent.war_value`
and `plan._score` price a *declared war* at leaf nodes; the aggression path
prices defence through `start_defense`/`finish_aggression`, which spend cards
out of the same hand. The value of holding a defence card is therefore already
priced at the leaf **when an aggression is on the table** — what was missing is
that the engine would take that card away from you before the attack arrived.
The A/B in §6 reports the defence outcomes directly.

## 5. Digests

All eight fingerprint arms move. That is expected and correct: turning a forced
FIFO discard into a real decision inserts `("choose", i)` moves into the move
stream of every game that has a military deck, so the game log — which is what
`perf_check` hashes — changes for every bot, including the ones that do not
evaluate through `weighted.py`. See `tools/gate.sh` for the before/after table
and the attribution.

## 6. Result: a well-powered null on strength, and a decisive one on behaviour

`tools/discard_ab.py`. Both arms run the **same fixed engine** and the same
weight vector (`analysis/frozen/champion_2p.json`); arm B answers every
`discard_military` choice the way the old engine did — pitch the oldest card in
hand. So the duel isolates the *policy*, not the plumbing, and it is a single
in-process head-to-head rather than two builds compared across runs. 600 games
/ 300 deals, 6 disjoint seed blocks, the FIFO arm played in each seat in turn.

### 6.1 Strength

Pooled over the six blocks with `experiments.paired_stats` (block-clustered,
K=6, so the critical value is `t₅ = 2.571`, not 1.96):

| | estimate | z vs null | p |
|---|---|---|---|
| win share | **50.83% ± 3.74pp** | +0.57 | 0.57 |
| culture margin | **+0.88 ± 1.85** | +1.21 | 0.23 |

A null, leaning very slightly positive. Block SE on the win rate is 1.45pp, so
this excludes effects larger than about **4pp** at 80% power: it is a
well-powered null for a large effect, not an underpowered shrug.

### 6.2 Behaviour — and this is not a null at all

| counter (evaluator arm, 6 blocks) | value |
|---|---|
| discard decisions faced | 7018 |
| chose differently from FIFO | **4364 (62.2%)** |
| kept a better defender than FIFO would have | **2409** |
| pitched a better defender than FIFO would have | **8** |
| defence points discarded | 9497, against 17159 under FIFO — **44.7% less** |

The policy does exactly what the rule is for, on nearly two thirds of all
firings, with a 300:1 asymmetry in the right direction. The eight
counter-examples are not a bug: the evaluator is free to prefer a card for
reasons other than defence, and eight times in seven thousand it did.

### 6.3 Why the strength result is flat, and it is not the rule's fault

| | over 600 games |
|---|---|
| aggressions played, both arms | **34 — 0.057 per game** |
| aggressions successfully defended | **0** |

**The defence channel these bots would be paid through is essentially
absent.** The frozen 2p champion attacks about once every eighteen games, and
in 600 games not one aggression was ever held off. Keeping your best defensive
card cannot be worth measurable culture in a population that neither attacks
nor successfully defends, so a flat A/B here is a statement about the
*population*, not about the rule or the policy. §4.2's question — does this
interact with the machinery that prices defence at leaf nodes — has an
empirical answer at 2p: it cannot, because that machinery almost never runs.

Per Paul's standing rule, correct modelling lands regardless of measured
strength, and this is a rules violation. But the result is not "shelve it and
hope": the behavioural counters show the fix is live and pointed the right way,
and §6.4 says what would actually test it.

### 6.4 What this does not measure

* **2p only.** Aggression is rarer at 2p than at 3p/4p and pacts do not exist
  there at all. The obvious follow-up is the same A/B at 3p, where the defence
  channel is more likely to be live. Not run here.
* **One weight vector.** The frozen 2p champion. A vector that valued strength
  more would attack more and could pay differently.
* **Not a retrained champion.** This is the frozen vector playing under a
  corrected rule, not a champion trained with the decision available. A trainer
  that can now *keep* a defender might learn to use one.
