# Aggressions are not rare; defences are never won (2026-07-30)

[`docs/COMBAT_AUDIT.md`](COMBAT_AUDIT.md#263-why-the-strength-result-is-flat-and-it-is-not-the-rules-fault) §2.6.3 reported **34 aggressions across 600 games —
0.057 per game — and not one ever successfully defended**, and read it as a
statement about the population: the channel the military-discard rule would be
paid through is essentially absent, so a flat A/B on strength says nothing
about the rule.

Both halves of that sentence turn out to be true measurements of two
completely different things. **The rate is a 1-ply artefact and search repairs
it 7–9× at every player count** — [`docs/CARD_CENSUS.md`](CARD_CENSUS.md)'s conditional answer
is confirmed, not contradicted, and there is no bug in the aggression rate.
**The zero is not an artefact.** Under the search the league actually trains,
1,549 defences were faced across 2p/3p/4p, 1,104 of them were winnable by
arithmetic, and **zero were won** — and this document ends with the one-line
cause and a measured fix that takes that number from 0 to 332.

## 1. Which regime produced 0.057 — it is 1 ply, and the league does not train there

`tools/discard_ab.py` builds its bots with `experiments.arena.make_bot`, and
`load_spec` on a **bare path** (`--spec analysis/frozen/champion_2p.json`)
falls through every prefix to `arena.py:232`:

    return B.WeightedBot(weights=spec, seed=seed)

No `plan:` or `quiesce:`. That number is a **1-ply `WeightedBot`** number.
Reproduced independently at n=300 with `tools/aggression_census.py`: 1-ply, 2p,
same frozen champion → **0.040 aggressions/game**, the same quantity.

The league does not train there. `tools/gate.sh:224` records that
`experiments/run_league.sh` trains `--candidate-bot plan:width=2` at 2p and
`--candidate-bot quiescent:levels=1` at 3p/4p.

## 2. The instrument

`tools/aggression_census.py` splits the death of an aggression into stages,
each with the previous as its denominator, counted only at **real** decisions —
the wrapper sits outside the bot, so trial states inside the bot's own search
are never counted. That separation is the whole point: it is what
[`docs/CARD_CENSUS.md`](CARD_CENSUS.md) had to fix in its own probe (`22e6dd3`, "the discard
probe was counting the search").

1. **held** — a politics decision where the player holds an aggression card.
2. **offered** — and `actions._politics_moves` listed an `("aggression", …)`
   move. The gap is the rules gate, not the policy: `actions.py:302` refuses to
   offer an aggression against a target the attacker does not already beat,
   which is [`docs/RULES_SPEC.md`](RULES_SPEC.md) §5.4 step 2 verbatim. **Rule-fact.**
3. **chosen** — and the policy picked it over everything else.
4. **won** — and the defender failed to hold it off.

The defence side is split the same way, because "0 defended" has three
different causes and only one is a defect: **impossible** (even spending every
legal card the defender cannot reach the attacker's strength — exact
arithmetic, the top `budget − spent` `defense_points` in hand added to standing
strength), **unattempted** (reachable and the policy played `defend_done`), and
**rare** (attempted and still short).

## 3. The rate: a 1-ply artefact, repaired by search. No bug here.

300 games per cell, frozen champion for the player count, every seat counted.

| | 2p | 3p | 4p |
|---|---|---|---|
| aggressions / game, **1 ply** | 0.040 | 0.097 | 0.563 |
| aggressions / game, **`plan:width=2`** | **0.303** | **0.870** | **3.997** |
| repair | 7.5× | 9.0× | 7.1× |
| wars declared, 1 ply | **0** / 2,396 offered | **0** / 2,727 | **0** / 5,277 |
| wars declared, `plan:width=2` | 316 | 668 | 2,251 |

Under the search the league trains, aggression is a routine part of the game —
four per game at 4p. [`docs/CARD_CENSUS.md`](CARD_CENSUS.md)'s control was right and its demotion
of war and aggression from the top of its list was right. **This is an
all-clear on the rate**, now measured per game rather than per draw, at all
three player counts rather than 2p only, and at n=300 rather than n=350.

**Generation is not the bug either**, which was the other open possibility: at
1 ply and 2p the move was in the legal-move list **2,066 times** and taken 12.
A card that is never offered cannot be played; this one is offered constantly.

## 4. Why 1 ply plays *any* aggression, and why none of them was ever defended

The two halves of §6.3's sentence have the same cause, and it is a selection
effect rather than a second defect.

`engine/interact.py:691`, at the top of `start_defense`:

    if budget <= 0 or not defender.hand_military:
        _finish_defense(state, ctx, rng)
        return

When the target *cannot* defend, the aggression resolves **inside**
`actions.apply` and a 1-ply `evaluate` sees the whole payoff. When the target
*can* defend, `apply` returns with a `kind="defense"` decision sitting on
`state.pending`: the attacker has paid a military action and a card out of hand
and gained nothing yet, so the move scores as pure cost and is declined.

**A 1-ply bot can therefore only ever play the aggressions that resolve inside
`apply` — exactly the ones against a defenceless target.** The prediction is
sharp and the census confirms it without a single exception: across **210
aggressions played at 1 ply** (12 at 2p, 29 at 3p, 169 at 4p), the defender got
a decision **zero times**. Under `plan:width=2` at 4p, where `_child` drains the
stack before scoring, the same counter reads `hand empty 0 / no military
action 0 / HAD a say 782`.

So "0 of 34 ever defended" at 1 ply is not a defence failure. The policy only
ever attacked opponents who could not defend at all.

## 5. Under search, defence is reached — and still never won. This one is real.

`plan:width=2`, 300 games per cell, pooled over 2p/3p/4p: **1,549 defences
faced, 1,104 winnable by arithmetic, 1,070 given up, 0 aggressions held off.**

And the direction is worse than indifference. Defence cards were spent in
**335 hopeless defences and 34 winnable ones** — the policy was ten times more
likely to spend a card on a defence it could not win than on one it could.

### 5.1 The cause, in one line

`engine/bots/plan.py`, in `pick`:

    if state.pending or state.current != me:
        return self._one_ply(state, moves, me, w, ctx)

`_child` scores every node **inside** the beam as `copy → apply → _quiesce →
_score`. At a **real** decision `pick` short-circuits to `_one_ply`, which is
`copy → apply → evaluate` with no drain. The identical position is priced two
different ways depending on whether the bot is the one being searched or the
one deciding, and the defender's own defence is exactly such a decision.

`interact._defense_move` keeps the decision on `state.pending` while the
defender still has room and cards, so after `("defend", card)` the aggression
has **not** resolved — `evaluate` sees a position one military card poorer with
the attack still hanging — while `("defend_done",)` pops the stack and calls
`finish_aggression` at once, showing the full loss. Nothing in
`weighted.features` reads `pend["atk"]` or `pend["dfn"]`.

So the choice cannot be about whether the defence succeeds. What is left is a
choice about whether to **defer the bad news**, and it points the wrong way:
spend a card when the impending loss looks large (which correlates with
hopeless), keep it when the loss looks small (which correlates with winnable).

The measurement that closes it: of 589 winnable defences at 4p, **588 needed
two or more cards**. The first `defend` therefore *always* leaves the outcome
pending and invisible. This is not an edge case in the tail; it is every
defence in the game.

### 5.2 The fix, and what it buys

`PlanBot._one_ply_quiet`: drain the pending stack before scoring, using the
same `_quiesce` this class already calls on every node of its own beam. No new
knowledge, no new weight, no new pricing table — the one place the drain was
being skipped.

Same 200 games of `plan:width=2` at 4p, same seeds, same weights, one process:

| | `qp=0` (today) | `qp=1` (drained) |
|---|---|---|
| aggressions resolved | 782 | 834 |
| **held off by the defender** | **0** | **332** (39.9%) |
| defences faced | 782 | 832 |
| ..winnable by arithmetic | 589 | 652 |
| defences attempted | 160 | 332 |
| ..**of which winnable** | **15** (9.4%) | **332** (100%) |
| winnable defences given up | 574 | 320 |
| defence cards spent | 271 | 344 |

The behaviour goes from anti-correlated with winnability to perfectly
correlated: **every** attempt is now in a defence that can be won and **none**
in one that cannot, for 27% more cards spent. Nothing else about the bot moved
— aggressions offered (5,718 → 5,776) and taken (782 → 834) are unchanged
within noise, as expected, since the attacker already quiesced.

### 5.3 Rule-fact or judgement call

* **The drain is a rule-fact.** §5.4 step 5 is a *threshold*: a defender whose
  total reaches the attacker's strength takes no effect at all, and one that
  falls short takes the full effect. A card spent below the threshold buys
  literally nothing. A decision procedure that cannot see the threshold is not
  approximating the rule, it is ignoring it. `tests/test_plan_defends_when_it
  _can_win.py` locks both halves — a winnable defence is carried through, a
  hopeless one spends nothing.
* **The default is a judgement call, and it is currently `False`.**
  `QUIET_PENDING` defaults off, so master's behaviour is byte-for-byte
  unchanged and every fingerprint arm holds — the `card_rate_credit`
  convention, so the two can be duelled paired in one process on the same deal.
  Turning it on changes `PlanBot` and `plan:` is a gated arm, so flipping the
  default moves gate digests and wants its own before/after table and
  attribution, the way [`docs/COMBAT_AUDIT.md`](COMBAT_AUDIT.md#25-digests) §2.5 did. **It should be
  flipped**; it was not flipped here because doing it at the end of a session
  while master is moving would land moved digests on other lanes without a
  strength A/B behind them. §8-§11 are that follow-up; read §9 before flipping
  anything, because the flip as written above is **not** the thing to land.

  To flip: set `pending.QUIET_PENDING = True` and re-derive **`PNARROW` and
  `PWIDE`** (`tools/gate.sh:304-305`). That is **two** arms, not eight and not
  "plan and quiescent" — an earlier draft of this section said both of those
  and both were wrong. Verified by setting the default `True` and recomputing:
  `PNARROW` 85c06781 → e3016dc2, `QNARROW` ad62a4e5 → **ad62a4e5**, `WNARROW`
  f0b240da → **f0b240da**. `QuiescentBot` and `WeightedBot` are different
  classes and never route through `PlanBot.pick`. (The "eight digests" figure
  belongs to the nine scoring fixes, `6efe8ba`.) Duel it paired first:

      python3 -m tools.aggression_census \
          --spec plan:analysis/frozen/champion_4p_gen350_99key.json,width=2,qp=1 \
          --players 4 --games 200 --workers 10

  — and note the vector: an earlier draft of this file used
  `analysis/frozen/champion_4p.json`, which is now
  `champion_4p.DEGENERATE.json` and which `arena._degenerate_match` refuses
  under any name.

## 6. Ruled out, with the evidence, so nobody re-runs these

* **"Does the resolution run where an aggression is *played*?"** Yes, under
  search. `plan.py:_child` calls `_quiesce` on every child, and `_quiesce`
  drains `state.pending` with 1-ply picks for whoever decides, defender
  included. The candidate is true only of the bare 1-ply `WeightedBot`, and
  that is §4. `QuiescentBot` never had the `pick`-time gap at all: its
  `pick` quiesces pending decisions already (`quiescent.py:364`).
* **`cost.militaryActions` unpriced (54 cards).** Real — `engine/bots/` reads
  the *effect* key `militaryActions` (`weighted.py:176`, `:814`), never
  `cost.militaryActions`, so War over Culture (3 MA) and War over Territory
  (2 MA) are the same card to every pricer. It is **not** this rate's cause,
  for two independent reasons. The only path it could reach is
  `card_potential` → `hand_mil_potential`, and `hand_mil_potential` **defaults
  to 0.0 and is absent from all three frozen champions**, so the whole
  military-hand pricing channel is multiplied by zero. And the decision to
  *play* an aggression never consults `card_potential`: it applies the move and
  evaluates the position, where the engine has already spent the actions.
* **`hand_mil_potential` calls `card_potential(n, w)` with no `state`/`idx`**
  (`weighted.py:1498`), so board-aware pricing can never fire for a military
  card. Confirmed by reading; inert today for the same 0.0-weight reason. Real
  plumbing defect, wrong suspect for this rate — and note it is the *severed
  wire* shape, so fixing the signature buys nothing until the weight is on.
* **The objective over-paying for stolen points.** `margin_share` pays twice
  for a stolen point and once for a produced one, which makes aggression *more*
  attractive, not less. Not a candidate for rarity.

## 7. What this does not measure

* **Strength.** Every number here is behavioural. Whether holding 332 more
  aggressions off is worth 73 more military cards spent is a duel nobody has
  run; §5.2 is a correctness result, not a win-rate result.
* **`quiescent:levels=1`, which is what 3p/4p actually trains.** The A/B in
  §5.2 is `plan:width=2`. `QuiescentBot` quiesces pending decisions already, so
  the prediction is that it does **not** have this defect and its defence
  numbers should look like the `qp=1` column — untested, and the obvious next
  measurement, because if it holds it means 3p/4p league training has been
  seeing a defence channel that 2p training has not.
* **The neural bots — but the defect is there too, and it is a copy.**
  `NeuralPlanBot` does not inherit `PlanBot`; it is its own class with the same
  short-circuit written out again at `engine/bots/neural_plan.py:163`:

      if state.pending or state.current != me:
          ...
          return self._one_ply_neural(root, moves, me)

  So the neural loop's plan bot answers its own defence undrained as well. Not
  fixed here — the neural loop is live out of `C:\Users\micro\tta-ai` and this
  lane must not disturb it — and not measured. Fixing `PlanBot` alone leaves
  the duplicate in place, which is the argument for doing them together rather
  than for doing neither.

  **Fixed in §10 by sharing one implementation rather than patching the copy —
  and the copy turned out not to be faithful: it already disagreed about
  determinization (§9).**

## 8. The bigger half: this was never mainly about defence, it was about AUCTIONS

§5 found the defect by counting defences, because a defence has a countable
outcome ("0 of 1,104 winnable ones held off"). That framing understated it.
`PlanBot.pick`'s short-circuit never tested the *kind* of the pending decision —
it fired on `state.pending`, and `engine/interact.py` pushes three kinds onto
that stack:

    "defense"   the defender's card-by-card answer to an aggression
    "auction"   a colony or pact bid, resolved round-robin
    "choice"    everything else, carrying a `tag` (which military card to
                discard, which sacrifice, which branch of an event, ...)

So **every** nested decision the bot owns was priced on a position with its own
resolution still hanging, while the identical position inside the bot's own beam
was priced after `_quiesce`. `tools/pending_divergence.py` measures the extent:
it plays real games with the drain on and, at every real decision of its own
where the stack is non-empty, prices the candidates both ways and records
whether the two disagree. 3p, `champion_3p_gen1255_99key`, `width=2`, 24 games:

| kind / choice tag | seen | drain moved the pick | rate |
|---|---:|---:|---:|
| **auction** (colony/pact bids) | 455 | 326 | **71.6%** |
| **defense** | 82 | 31 | **37.8%** |
| choice:discard_military | 1728 | 104 | 6.0% |
| choice:take_row | 6 | 1 | 16.7% |
| 11 other choice tags (`food_or_res`, `free_civil`, `pact_offer`, `raid`, `lose_pop`, `lose_colony`, `free_build`, `gain_block`, `destroy_own`, `infiltrate`, `annex`) | 356 | 0 | 0.0% |
| **all own pending decisions** | **2,627** (109.46/game) | **462** | **17.6%** |

**Auctions are the dominant surface, at nearly twice defence's rate on five
times the volume.** The mechanism is the same defect stated in its worst form:
an `auction` pend resolves round-robin, so an undrained position after
`("bid", n)` shows the money committed and *not* who won the territory. The bot
was choosing what to pay without the position it scored ever showing whether it
won the colony. `_quiesce` resolves the rest of the bidding, so the drained
position shows the outcome — which is what `_child` has always seen inside the
beam.

This is the same defect [`docs/CARD_CENSUS.md`](CARD_CENSUS.md#10-the-territory-suspect-and-the-defence-drain-are-one-defect-2026-07-30) §10 reached from the other end
when it ranked **territories** its number-one suspect: the census saw
territories mispriced and looked for a missing feature, and the missing thing
was not a feature but the position the feature was read on. Two lanes, opposite
directions, one defect. Do not treat them as separate problems.

`choice:discard_military` deserves its own line: 1,728 occurrences, the largest
single volume, and only 6.0% moved. That is the shape you expect from a decision
whose consequence is mostly visible immediately (a card leaves the hand), which
is a useful control — the drain is not just perturbing picks at random, it moves
them where the outcome is deferred and leaves them alone where it is not.

## 9. The deck peek at the pending path: real, older, and INERT on picks

This section was written twice. The first version said the drain's win rate was
confounded by an information leak. **That was wrong, and the way it was wrong is
worth keeping**, because the check that produced the retraction is the same
check that produced the finding.

**The suspicion.** The first paired block of `qp=1` vs `qp=0` at 3p came back at
**53.28% ± 5.89pp against a 33.3% null** (culture margin +25.59 ± 5.84,
z = 6.76, n = 200, deal-clustered K = 66, rho = −0.154). That is far too large
for a defence fix. `fastcopy.copy_state` copies the hidden piles verbatim — it
is a copier, that is its job — so a trial `apply` that draws would draw the
**real next card**. `pick`'s beam path is protected from that because it
re-shuffles those piles into its root before `_beam` ever sees it. The pending
short-circuit did not determinize at all — and the drain *adds* `apply` calls,
so it should add peeking. `tools/pending_leak.py` confirms the mechanism
exists, per candidate evaluation at the bot's own pending decisions:

| | 3p (1,805 evals) | 4p (3,917 evals) |
|---|---:|---:|
| master's `apply` consumed real deck cards | 24.0% | 19.1% |
| the drain consumed real deck cards | 34.7% | 32.0% |
| master's `apply` changed the visible row | 25.5% | 20.2% |
| the drain changed the visible row | 40.3% | 26.8% |

**The retraction.** Counting card consumption is not measuring exploitation, and
the conduction rule applies to a confound exactly as it applies to a treatment:
*show that the lever moves something before you attribute a result to it.*
`PENDING_DETERMINIZE` (`qd`) removes the peek — verified directly: the root is a
copy, the real deck is untouched, the copy's order really changes, the multiset
is preserved. Then `tools/pending_divergence.py --lever det` asks the only
question that matters, at every one of the bot's own pending decisions: does
removing the peek change the pick?

    3p, champion_3p_gen1255_99key, 12 games:
    own pending decisions 1346 (112.17/game) -- LEVER CHANGED PICK 0 (0.0%)

**Zero, on every kind.** A `qp=1` vs `qp=1,qd=1` duel on identical deals is
byte-identical in win rate, per-game shares and culture margin. So the peek
cannot be the cause of anything, and §5.2's result stands as a result about
play.

**Why zero, and why that is not a fluke.** The drain resolves the *same*
pending stack for every candidate move, so it draws the same cards in the same
order whichever candidate is being priced. The peeked cards enter every
candidate's score as the same additive term, and an argmax over candidates is
invariant to a common offset. The peek is **common-mode**. It would stop
cancelling only where candidates differ in how many cards the drain consumes;
that is rare enough not to appear in 1,346 decisions.

**It is still a defect and should still be closed.** The bot reads the true next
deck card on 24% of its candidate evaluations at 3p today, with no flag, in the
same family as the `end_turn` row leak this repo has fixed twice. The argument
for closing it is correctness, not strength: a common-mode leak is one
refactor away from being a differential one, and the next person to make
candidates draw different amounts inherits a live exploit with no test failing.
`qd` exists, is shared through `pending.prepare_root`, and is on for
`NeuralPlanBot` already. **The measured cost of turning it on for `PlanBot` is
zero decisions changed**, which is the cheapest correctness fix in this
document — but it is not free to *land*, because it moves `PNARROW`/`PWIDE` and
restarts the league arms, so it ships with the drain rather than on its own.

**CLOSED 2026-07-30.** `pending.DETERMINIZE` is `True` and both bots resolve it
through `None` class attributes, so there is one answer instead of two.

**What this episode is evidence for.** Two spectacular numbers tonight turned
out to be instruments rather than results. This one turned out to be a result,
and the only reason that is known is that it was attacked as hard as the other
two. Attack the confound with the same conduction test you would apply to the
treatment; "there is a mechanism by which this could be fake" is a hypothesis,
not a finding.

## 9a. The bigger leak was in the sentence above, not the one below it

Section 9 was read, correctly, as saying two incompatible things: that the beam
prices on a determinized root, *and* that a trial `apply` in the beam draws the
real next card. Resolved by reading `engine/bots/plan.py` rather than the prose:
**`pick` determinizes its root before `_beam` sees it, and has since PlanBot was
written** (`root = copy_state(state)`, `if self.determinize: determinize(root,
drng)`). `determinize=True` is the default and `experiments/arena.py` builds
every `plan:` and `nplan:` spec with `det=1` unless a run asks otherwise, so the
league has never played the beam un-determinized. There was no un-scoped beam
leak. The garbled sentence was one clause of causation away from the truth and
is fixed above.

**But the determinization was incomplete, and that WAS an un-scoped leak.**
`determinize` shuffled `civil_deck` and `military_deck`. It never touched
`current_events`, and `events.reveal_current_event` pops that pile at the top of
every turn — so every `end_turn` the beam expanded revealed the **true next
event**, inside a search that believed it had determinized.

The reason this survived is that `tools/infoleak.py` could not see it. Its
headline number — 94.9% of `end_turn` candidates at 2p — counts candidates that
**draw**. Determinization does not change how often a candidate draws, only
what it draws, so that number is identical on a leaking and a clean root. It is
also measured on `WeightedBot`, which does not determinize at all. It was
quoted here and in `plan.py`'s docstring as though it described the beam.

`tools/infoleak.py --true-card` asks the question that separates them: was the
card consumed the card that was really on top? It applies every candidate twice,
once from the real state and once from a determinized copy. 2p, 8 games, 18,762
candidates:

| pile | draws | true-top, real root | true-top, determinized root (before) | after |
|---|---:|---:|---:|---:|
| `civil_deck` | 800 | 100.0% | 28.6% | 28.6% |
| `military_deck` | 639 | 100.0% | 16.9% | 17.2% |
| `current_events` | 209 | 100.0% | **100.0%** | 38.3% |

100.0% is not a leak *rate*. It is the signature of a field nobody is
shuffling. The two card decks were already being sampled and stay sampled;
`current_events` goes from the truth to a ~33% chance floor for a 3-card pile.

Both columns are the *same script on the same 18,762 candidates*, run once
against a tree with the old two-deck `determinize` and once against this one —
not two different measurements pasted side by side. The 16.9 → 17.2 wobble on
`military_deck` is the third shuffle consuming different `rng` draws, and
`civil_deck` does not move at all, which is what "this change touches the event
pile and nothing else" should look like.

**The event pile is age-ordered and that order is public.**
`events._recycle_future_events` shuffles the pile and then sorts it by
descending age level, because `pop()` takes from the end and the oldest age must
come out first. Everyone at the table knows an Age I event precedes an Age II
one. A flat shuffle would hide private information by destroying public
information, and would let the search see Age III events arrive early. So
`determinize` repeats the engine's own two lines and permutes only within each
age band.

**The guarantee against this recurring** is
`tests/test_search_root_is_determinized.py`, and it is deliberately not "is
`determinize` called" — a call covering two of three piles passes that. It pins
`plan.HIDDEN_ORDER` as a written-down decision, asserts each listed pile is
really permuted, asserts every *other* container on the state is untouched
(the visible row and the players' own hands are information a human legitimately
has), asserts the multiset and the age bands survive, and then plays real games
and asserts on tracked state that the pile handed to each search did not have
the true next card on top. Negative control: reverting the one-line event
shuffle fails it at **349/349, 100.0%**, against a ~33% chance floor.

**What this episode is evidence for**, and it is the same lesson one turn
further on: an instrument that returns the same number whether or not the
defect is present is not evidence about the defect. Check that the measurement
*can* move before quoting it as a measurement.

### 9a.1 What is still open, written down rather than half-done

**`WeightedBot` and `QuiescentBot` do not determinize at all.** Neither calls
`plan.determinize`, so every trial draw they make reads the true next card —
`tools/infoleak.py --true-card` puts all three piles at 100.0% for them. That
matters more than it used to, because [`docs/BOT_ARCHITECTURE.md`](BOT_ARCHITECTURE.md) line 1085
states a hard precondition — *"M2 must not ship without M1's determinization…
Today it is inert; after M2 it is a cheat"* — and M2 **has** shipped, as
`weighted.hand_mil_potential`, which prices `hand_military` by card identity
and carries 0.01079 on the live 3p champion. So a trial `end_turn` in those two
bots draws the real next military card and then prices it by name.

The precondition is satisfied for `PlanBot`/`NeuralPlanBot`/`NeuralBot`, which
is what M1 refers to and which do determinize. It is violated for the two that
do not.

**Measured, before anyone panics.** `tools/leak_impact.py` at 3p, 6 games, K=8,
with the live `champion_3p.json`: the honest determinization changed
`WeightedBot`'s chosen move on **0 of 2138** decisions, and the `end_turn` eval
delta was **−0.004 ± 0.038**, against a within-decision spread across
determinizations of 0.015. The cheat is *latent*, not active: at
`hand_mil_potential = 0.01079` the identity term is far smaller than the
sampling noise it would have to beat. It becomes active if that weight grows,
and hill climbing is free to grow it. (Caveat on that number: `leak_impact.py`
has its own local `determinize` that shuffles the two decks only, so it does
not measure the event component at all. It is a lower bound.)

**Hidden *contents*, as opposed to hidden order, are not sampled anywhere.**
`determinize` permutes piles; it does not re-deal them. The rivals'
`hand_civil`/`hand_military` and the face-down `future_events` are unknown in
their membership, not merely in their order, and no bot models that. Today
`weighted.features` reads only public rival aggregates, so nothing reads the
identities — which is the same "inert but one refactor from live" shape as
everything else in this section.

## 10. The duplicate, fixed by sharing (`engine/bots/pending.py`)

`neural_plan.py:163` had `plan.py`'s short-circuit copied out, which is the
fifth instance in one session of one rule living in two places (the build
discount, the hand double-count, the population cost, the `rankingCulture`
block). It is fixed by extracting the policy, not by patching the copy.

**The copy was not even faithful, which is the argument in miniature.**
`NeuralPlanBot`'s pending path always determinized; `PlanBot`'s never did. The
two bots disagreed about the *leak* as well as the drain, and nobody knew,
because there was no single place where the answer lived. Closed 2026-07-30:
both classes now carry `PENDING_DETERMINIZE = None` and there is one answer.
The bot-wide `determinize` A/B switch moved into `wants_determinize` at the same
time, because `NeuralPlanBot` spelled it at its own call site and `PlanBot` did
not spell it at all — a third place for the same two to drift.

`engine/bots/pending.py` owns three things and no scoring:

* `not_my_turn(state, me)` — the predicate both bots had written out.
* `wants_quiet(bot, state)` / `QUIET_PENDING` — the drain, defaulting in ONE
  place. Both classes carry `QUIET_PENDING = None`, meaning "ask the module",
  so the two cannot be flipped apart.
* `wants_determinize(bot, state)` / `prepare_root(...)` — the root preparation
  from §9. The two classes differ here **on purpose** (`PlanBot` `False` =
  master's leak, `NeuralPlanBot` `True` = correct), so the value is pinned by a
  test with the reason attached rather than left to drift.
* `fallback_pick(bot, state, plain, quiet)` — takes the two scorers as bound
  zero-argument callables, so the evaluator-specific half (PlanBot's serial
  linear dot product and its journalled variant; NeuralPlanBot's one batched
  encode per ply) stays in each bot.

`tests/test_pending_fallback_is_shared.py` (15 tests) pins the three ways they
could drift apart again: **re-inlining** (the shared counters stop moving),
**a second default** (a bool on either class fails), and **a different drain**
(with the drain on, every position either bot prices at a real pending decision
must have an empty pending stack; with it off at least one must not — the
control that stops the first assertion from passing vacuously). The counters are
tracked state, not a regex over source, so a bot that reimplements the branch
*identically* still fails: the point is that there is one implementation.

Verified the test can go red: re-inlining the neural copy by hand fails 2 of the
15 with `NeuralPlanBot: {'calls': 0, 'quiet': 0}`.

`NeuralPlanBot` gained the drain it was missing (`_one_ply_neural(..., quiet=)`,
calling the same `_quiesce` its own `_beam` already runs on every node), and
`arena.make_bot` threads `qp`/`qd` into the `nplan:` spec, so the neural arm is
measurable by the same lever. **Nothing here changes behaviour:** with the
shipped defaults `python3 -m engine.perf_check hash --plan` is **85c06781**,
i.e. `PNARROW` unchanged, and the full suite is 993 tests OK.

## 11. The measurement that decides the flip (in flight)

Pre-registered before any block landed, because the number in §9 is what
happens when you decide after looking.

* **ON** = `plan:REF,width=2,qp=1,qd=1` — price a pending decision exactly the
  way this bot's own beam prices one: determinize, apply, quiesce, score.
* **OFF** = `plan:REF,width=2` — master, byte-for-byte.
* `REF` = `champion_3p_gen1255_99key` / `champion_4p_gen350_99key`, the live
  league references. Conduction table run on both first (Gate 1 open on
  `hand_potential`, `rival_hand_potential`, `row_pressure` for each), and a
  behavioural conduction probe run before any A/B: `aggression_census` at 4p,
  30 games, identical deals, aggressions held off **0 → 26**, defence cards
  played **88 → 40**, attempted defences that were reachable **4/41 → 26/26**.
  A lever that moves those is not going to return an arithmetic identity.
* 6 blocks × 200 games at each of 3p and 4p, `arena.duel` seat-rotated so both
  arms see the same deals; pooled with `tools/ab_summary.py`
  (`experiments/paired_stats.pooled`, deal-clustered, t not z).
* **Caveat to read with the result:** this is ONE drained seat against
  undrained opponents. The league trains self-play, where every seat would
  drain; that is a different question and the census in §5.2/§11 answers it
  behaviourally, not on strength.

Result: **pending** — this section is the placeholder the number lands in.

---

## Appendix: the original `AGGRESSION_FIX.md` investigation (2026-07-26), merged in full 2026-07-31

Folded in here — and the standalone file deleted — because §§1-11 above
supersede its headline rates. Every rate below ("aggressions 0.00 at 2p/3p,
0.11 at 4p; wars 0.00 everywhere") is a **1-ply `WeightedBot` measurement**;
§1 above shows that is an artefact of the evaluation horizon, not a fact
about the game. Under the search the league actually trains (`plan:width=2`)
the real rates are the **0.303 / 0.870 / 3.997 aggressions per game** and
**~1.05 / 2.23 / 7.50 wars per game** at 2p/3p/4p reported at the top of this
document. Kept in full for two things nothing else restates: **Part A's
refutation** of the "4p colony auctions never start because events aren't
seeded" hypothesis, and the **Part B mechanism** (payoff lands in the
defender's decision, therefore invisible at 1 ply) that is the seed of this
document's own analysis.

Date: 2026-07-26
Owner: aggression-fix agent (`engine/bots/`, colony/military paths of
`engine/actions.py`, this file)

Follow-up to [`docs/COMBAT_AUDIT.md`](COMBAT_AUDIT.md).
Two open items from the post-fix measurement:

* **A.** colony bids at 4p went 0.02 -> 0.01 (essentially never), with the
  hypothesis "4p auctions never start because events aren't seeded";
* **B.** aggressions read 0.00 at 2p/3p and 0.11 at 4p, wars 0.00
  everywhere, i.e. the war/aggression layer never fired.

Both were reproduced before any code was touched. **A's hypothesis is
refuted** — the events *are* seeded and the auctions *do* start. B is
confirmed, with a precise mechanism and a measured size.

---

## A. 4-player colony auctions: hypothesis REFUTED

### What was measured

8 mirror 4p self-play games with `experiments/champion_4p.json`
(seeds 900000-900007), instrumenting `events.reveal_current_event` and
`interact.start_auction`; same for 3p as a control.

| quantity (per game) | 3p | 4p |
|---|---|---|
| Age A current events seeded at setup | 5 | **6** |
| ...of which are territories | 0 | 0 (correct, §1.6) |
| `prepare_event` reveals | 75.5 | **121.6** |
| territories revealed | 7.88 | **2.38** |
| **auctions started** | 7.88 | **2.38** |
| auctions with **0** eligible bidders | 0.38 | **1.75** |
| auctions with 1 eligible bidder | 0.38 | 0.62 |
| auctions with 2+ eligible bidders | 7.12 | **0.00** |
| colonies gained (all seats) | 1.88 | 0.12 |

So at 4p the auction fires 19 times in 8 games and **14 of those 19 die at
`interact.start_auction` with "nobody can colonize"** — before any bot ever
gets a decision. The remaining 5 have exactly one eligible bidder.

### The real cause: the 2p and 4p champions own no military units

`start_auction` (`engine/interact.py:508-519`) builds its bidder list from
`max_force(state, p) > 0`, and `max_force` returns 0 when `unit_pool(p)` is
empty. That is correct rules: §11.3 requires sacrificing **at least one
military unit**, "even if other bonuses would cover the bid". A player with
no units in play genuinely cannot colonize.

Measured mean military units in play per player (sampled every 200 moves,
4 games per count, champion mirror):

| | 2p | 3p | 4p |
|---|---|---|---|
| units in play / player | **0.00** | 2.00 | **0.07** |
| players with zero units | **100%** | 25% | **92.5%** |
| max colonization force | 0.00 | 4.00 | 0.28 |
| strength | 1.50 | 4.25 | 1.77 |
| `unit_workers` weight | 0.051 | 0.063 | 0.132 |
| `colonies` weight | 3.311 | 1.443 | **-0.962** |

The 2p and 4p champions simply never put a worker on a military unit, so
they are excluded from every auction at the door. The 3p champion does,
and its auctions work (7.9 started, 1.9 colonies per game).

### Verdict

**Not an engine bug, and not the seeding.** `state.current_events =
age_a[:num_players + 2]` is exactly §1.6 (2p:4, 3p:5, 4p:6) and the Age A
military deck is all events by design, so no territory can *or should* be
in the setup deck at any player count; territories arrive later via
`prepare_event` -> `future_events` -> recycle, and they demonstrably do.
The engine paths are right; the 4p champion's army is the blocker.

This is self-reinforcing and cannot be fixed by pricing a deferred payoff:
a 1-ply search evaluating "put a worker on Warriors" cannot see that the
unit is what buys a colony ten turns later, and the 4p `colonies` weight
(-0.962) is pure hill-climb drift on a feature that fired 0.02 times per
game. The fix is a **weights/training** one — reset `colonies` (already
done in `15b9764`) and re-run the climb now that `auction_committed` makes
bids visible — not a code one. Recorded here; the climb restart is the
main agent's call.

---

## B. Aggressions and wars: confirmed, and it is the 1-ply horizon

### The moves are legal constantly and are essentially never taken

6 mirror games per player count, champion mirror, every politics decision
instrumented:

| per game | 2p | 3p | 4p |
|---|---|---|---|
| politics decisions | 42.0 | 54.8 | 85.3 |
| holding an aggression card | 25.0 | 23.3 | 34.5 |
| **`aggression` in `legal_moves`** | **12.3** | **11.2** | **21.3** |
| **`war` in `legal_moves`** | **13.7** | **6.7** | **15.2** |
| aggressions chosen (all seats) | 0.17 | 0.00 | 1.17 |
| wars chosen (all seats) | **0.00** | **0.00** | **0.00** |

(The behaviour harness's 0.00/0.11 numbers count one *champion seat*, not
the table — `experiments/behaviour.py`'s Watcher wraps a single bot — so
the 2p/3p zeros are the same near-zero rate as the baseline seen through a
smaller window, **not a regression**. Wars are a true structural zero.)

### Why: the payoff lands in the defender's decision

`_h_aggression` (`engine/actions.py:972`) -> `events.start_aggression`
spends the military action, discards the card, and hands a `defense`
decision to the target (`engine/interact.py:601`). The trial state the
attacker evaluates therefore shows **only the cost**. Direct probe, best
attack vs the chosen move in the same position:

```
2p  best ('prepare_event','Rebellion')  69.936 | best_attack ('aggression','Plunder (I)',0)  68.274 | pol_pass 68.504
4p  best ('pol_pass',)                  64.253 | best_attack ('aggression','Enslave',0)      62.578 | pol_pass 64.253
3p  best ('prepare_event','Impact of Industry') 134.220 | best_attack ('war','War over Culture',1) 125.399 | pol_pass 126.417
```

In every one of 357 sampled positions across the three player counts the
best attack scored **below `pol_pass`**, usually by 0.2-5 points — the
constant hand/military-action penalty, exactly as predicted for pacts.

Wars are worse still: `_h_war` (`actions.py:1036`) only writes
`war_declared_by_me` / `wars_declared_on_me`, and the spoils are taken a
whole turn later in `events.resolve_war` (`events.py:557`, called from
`game.start_turn`). No feature in `weighted.features()` read either field,
so declaring war was *literally* a pure cost with no representable
benefit — hence exactly 0.00 wars, forever, at every player count.

### Fix (same shape as the pact/colony fix, `166867d`)

The implementation and the A/B result are not in this section — they
landed in [`docs/PLAN_WAR_LOOKAHEAD.md`](PLAN_WAR_LOOKAHEAD.md) and in
§§1-11 of this document (this dangling pointer is original to the
2026-07-26 write-up; noted rather than silently fixed).
