# Aggressions are not rare; defences are never won (2026-07-30)

`docs/MILITARY_DISCARD.md` §6.3 reported **34 aggressions across 600 games —
0.057 per game — and not one ever successfully defended**, and read it as a
statement about the population: the channel the military-discard rule would be
paid through is essentially absent, so a flat A/B on strength says nothing
about the rule.

Both halves of that sentence turn out to be true measurements of two
completely different things. **The rate is a 1-ply artefact and search repairs
it 7–9× at every player count** — `docs/CARD_CENSUS.md`'s conditional answer
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
`docs/CARD_CENSUS.md` had to fix in its own probe (`22e6dd3`, "the discard
probe was counting the search").

1. **held** — a politics decision where the player holds an aggression card.
2. **offered** — and `actions._politics_moves` listed an `("aggression", …)`
   move. The gap is the rules gate, not the policy: `actions.py:302` refuses to
   offer an aggression against a target the attacker does not already beat,
   which is `docs/RULES_SPEC.md` §5.4 step 2 verbatim. **Rule-fact.**
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
four per game at 4p. `docs/CARD_CENSUS.md`'s control was right and its demotion
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
  attribution, the way `docs/MILITARY_DISCARD.md` §5 did. **It should be
  flipped**; it was not flipped here because doing it at the end of a session
  while master is moving would land eight moved digests on other lanes without
  a strength A/B behind them.

  To flip: set `QUIET_PENDING = True`, re-derive the `plan`/`quiescent` arms of
  `tools/gate.sh` with attribution, and duel it paired first:

      python3 -m tools.aggression_census --spec plan:analysis/frozen/champion_4p.json,width=2,qp=1 \
          --players 4 --games 200 --workers 10

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
