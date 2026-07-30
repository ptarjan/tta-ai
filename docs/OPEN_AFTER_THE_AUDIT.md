# Open questions after the 2026-07-29/30 card audit

Written at the end of the session that fixed nine scoring bugs, repriced most
of the deck, and reversed its own headline twice.  Everything here is a real
loose end, recorded so it is not rediscovered from scratch.

## 1. `wonder_potential`'s scale has no trustworthy evidence

Measured effect at 0.125 vs 0.0 (`tools/wonder_mechanism.py`, mirror, 500
deals): wonder completions **0.408 vs 0.051**, started 0.674 vs 0.083, finish
rate 0.369 vs 0.051, civil actions on stages 1.372 vs 0.167.  Eiffel Tower goes
from **zero completions in 1000 seat-games** to 0.082/deal.

At 0.5 it turns pathological -- 2.69 started, 2.09 abandoned, finish rate 0.23
-- so 0.125 is the top of the usable range.  **But the accompanying strength
null (50.34% +/- 2.43pp, n=1600) was measured against the frozen 78-key
champion missing `row_urgency`**, which `docs/CARD_CENSUS.md` §10 shows is a
broken yardstick: the reprice changed `evaluate()` on 0 of 480 wonder-in-row
states.  The behavioural numbers survive; the strength conclusion does not.
Re-run against a live reference vector before quoting any of it.

The right answer is probably not a hand-set constant at all: leave the weight
at 0.0 and let the league find it, which is what the restart is for.

## 2. Abandoned wonder programmes regressed in absolute terms

Started-but-unfinished rose **0.032 -> 0.271 per deal** at `wonder_potential`
0.125.  The finish *rate* improved (5% -> 37%), but the absolute count of
abandoned programmes went the wrong way, against the standing 23-44%
improvement recorded in `docs/CARD_BLINDNESS.md` §5.3.  Unresolved: is a bot
that starts eight times as many wonders and finishes 37% of them better or
worse than one that starts almost none?  The objective should answer this and
nobody has asked it.

## 3. The three bonus cards are priced now; the seam is closed; the weights mostly aren't

~~`defenseBonus` and `colonizationBonus` have no reader.  A mapping and the
`hand_mil_potential` seam were designed and dropped to avoid a collision
between two lanes.  Note `hand_mil_potential` is 0.0 on all three live
champions and calls `card_potential` **without a state**, so board pricing
cannot fire for military cards at all -- pricing these without fixing that seam
would be inert by construction.~~ **SUPERSEDED 2026-07-30.** Both now have a
reader: `_BONUS_TO_FEATURE` in `engine/bots/weighted.py` maps `defenseBonus`
-> `defense_bonus` and `colonizationBonus` -> `colonize_bonus`, and
`_card_yields` prices them (gated by `bonus_card_credit`, which is 1.0 -- the
printed number is fully believed -- on all three live champions). The seam is
also closed: `hand_mil_potential` is now `def hand_mil_potential(state, idx,
w)` and calls `card_potential(n, w, state, idx)` with both, so board-aware
pricing is no longer blocked for military cards by construction (see the
function's own docstring in `weighted.py` for what still keeps it inert
today -- `board_yields`/`board_extra`/`_board_credit_key` have no entries for
a military type, so a military card's board credit falls through to the bare
`card_board_credit`, which is 0.0 on all three live champions).

Reading `experiments/league_state/champion_{2,3,4}p.json` (118-key vectors,
2026-07-30) directly: **`defense_bonus` is 0.0 on all three** -- pricing
exists but the league has not put weight on it, so the defence increment
these cards carry is still effectively unpriced, exactly the state this
section used to describe for all three cards. `colonize_bonus` is different:
**0.0 at 2p, but 0.04196 at 3p and -0.07368 at 4p** -- nonzero, so the
colonization increment is live at two of three player counts. `hand_mil_potential`
is **0.0 at 2p and 4p, 0.01079 at 3p** -- also live, but only at 3p. So: two of
the three cards have a real, nonzero champion weight today, and it is only
`defenseBonus` that is still priced-but-inert everywhere.

## 4. `cost.militaryActions` is read by no bot code

Re-checked 2026-07-30 against the current `engine/bots/` and this is still
true: 54 cards carry it. The rules engine gates legality on it
(`actions.py:269,1083`, `events.py:493`) and nothing under `engine/bots/`
reads `card.get("cost")` at all (`grep` for it across `weighted.py`,
`book.py`, `board_yields.py`, `neural_encode.py` returns nothing), so War
over Culture (3 MA) and War over Territory (2 MA) are still the same card to
every pricing path. `_EFF_TO_FEATURE`'s `militaryActions` -> `military_actions`
entry (nonzero on all three live champions, e.g. 3.47652 at 3p) is a
different thing: it prices a card's `effects`/production GRANT of military
actions (governments, action cards), not a war card's `cost` to play. §3's
`hand_mil_potential` now takes a state, but that closes the board-pricing
seam for military cards in general -- it does not add a reader for `cost`,
which no function in `engine/bots/` touches.

## 5. The defence drain is landed and now ON (`a214804`, 2026-07-30)

`556ad85`.  PlanBot prices its own defence differently from the identical
position inside its own search: `pick` short-circuits on `state.pending` to
`_one_ply` with no drain (`plan.py:174`).  Across 1,549 defences faced and
**1,104 winnable by arithmetic, zero were ever held off**, while cards were
spent in 335 hopeless ones.  588 of 589 winnable defences need 2+ cards, so the
first `defend` always looks like pure cost.

The fix takes held-off defences 0 -> 332 over 200 games at 4p, with every
attempt winnable.

**RESOLVED 2026-07-30 in `a214804`: `QUIET_PENDING = True`.**  It landed as a
CONSISTENCY FIX, not on a strength measurement -- the beam already drains
before scoring and the live decision did not.  The A/B is in
`docs/DRAIN_AB.md`; read it there rather than from the bullets below, because
it is uneven: 3p is decisive (pure-`qp` pool 0.5217 own-win share against a
0.3333 null over 600 games, z = 9.26) and **4p is NOT independently
established** (one pure-`qp` block, 0.3000 against a 0.2500 null, z = 1.54,
p ~ 0.12).  The leak objection in the second bullet below was answered, not
argued away -- see that bullet.  `DETERMINIZE` was deliberately held at `False`
so the digest movement is attributable to exactly one constant; it is still
open and gets its own commit.

Scope note, checked rather than assumed: only `plan.py` and `neural_plan.py`
import `engine/bots/pending.py`, so this changes PlanBot and NeuralPlanBot
only.  The live 3p/4p league arms climb `quiescent:levels=1`, and QuiescentBot
resolves the pending stack to quiescence by construction -- it never had the
defect.  The flip makes PlanBot behave the way QuiescentBot always did.

**Written 2026-07-30 before the flip, and the item was bigger than it looked.**
Retained because the diagnosis is still the record of what the defect was; the
"do not flip" instruction in it is superseded by the paragraph above.  See
`docs/AGGRESSION_RATE.md` 8-11.

* Not mainly about defence: the short-circuit never tested the pending *kind*,
  and **auctions** are 71.6% of the decisions the drain moves (455 seen, 326
  moved at 3p) against defence's 37.8%.  The bot was pricing a colony/pact bid
  on a position where the bid had not resolved.  That is the same defect
  `docs/CARD_CENSUS.md` 10 reached from the territory end.
* ~~**Do not flip `QUIET_PENDING` as shipped.**~~ SUPERSEDED -- the objection
  was real and was then answered.  Neither pending path determinizes, so a
  trial `apply` draws the REAL next deck card, and the drain adds `apply`
  calls: master leaks on 24.0% of candidate evaluations at 3p and the drained
  arm on 34.7% (`tools/pending_leak.py`), so the first paired block (53.28%
  +/- 5.89pp vs a 33.3% null) was contaminated.  The leak-neutral contrast
  `qp=1,qd=1` vs plain was then run at the same seed as `qp=1` vs `qp=0` and
  returned the SAME numbers to every printed digit (0.5325 win, +26.01
  margin).  Removing the peek changes nothing, so the win is not the peek --
  a second instrument agreeing with the 1,346-pick census.  `docs/DRAIN_AB.md`
  3.
* **The determinization leak is its own, older defect** and probably the larger
  prize: it is live in every league game today with no flag.  Scoped and costed
  in `docs/AGGRESSION_RATE.md` 9; not started.
* `neural_plan.py:163`'s copy is **fixed**, by sharing one implementation
  (`engine/bots/pending.py`) with a divergence test, not by patching the copy.
  The copy was not faithful: it already determinized where `PlanBot` did not.
* Flipping the default moves **two** digests, `PNARROW`/`PWIDE` -- not eight,
  and not "plan and quiescent"; verified by recomputing all three narrow arms.

## 6. War over Technology's alternative spoil is unimplemented

The only remaining inexactness in end-of-game scoring (22 of 23 types are
exact).  The victor may take blue technologies instead of science; that is a
player *choice* the engine does not offer, not a number it gets wrong.  Adding
it adds a decision point to every war.

## 7. 3p `row_urgency` has an arbitrary sign

`+0.163` on the live 3p champion where the semantically correct sign is
negative (`row_pressure` is evaluated post-move and measures urgency *left
behind*).  It is active on 35% of 3p decisions, but flipping it is worth
`+0.0025 +/- 0.0305` over n=600 -- no usable gradient, so the climb drifted to
a wrong sign without ever paying for it.  **Any 3p measurement that reads card
ordering is reading an arbitrary sign.**  Win rate and margin are unaffected.

## 8. The human corpus cannot validate what it cannot vary

`docs/SCORE_AUDIT.md` §2.  At 2 players every pact is removed from the game and
the corpus is 2p only, so "food your farms produce" and "your food rating" are
identically equal in all 2,525 positions -- which is how a broken card scored
66/66 exact.  Five of the nine bugs sit inside four documented blind spots.
The corpus is decisive exactly where it has variation and silent, while
reporting perfection, everywhere else.  Before quoting a corpus percentage, ask
what inputs produced it and whether they could have distinguished the
alternative.

## 9. Standing hazards, each of which has already cost real bugs

* **A card whose cost is priced while its gain sits at 0.0 is biased, not
  inert.**  `tests/test_half_priced_cards.py`.
* **Two implementations of one rule always drift.**  Paid for four times in one
  night: `build_discount`, the leader hand double-count, the population-cost
  formula (four copies, three missing a term), and the `rankingCulture` block.
* **A swap diff is exact over `Stats` and blind to everything else**, and it
  *replaces* the static table rather than supplementing it -- so any key the
  static path priced that the diff cannot see is silently dropped.  Taj Mahal's
  blue token was a live instance.
* **Assert the lever conducts before spending games.**
  `arena.assert_lever_conducts()` / `tools/conduction_table.py`.  A 12,800-game
  null was an arithmetic identity because the weight under test was absent from
  the vector under test.
* **"Inert" is a statement about coverage, not correctness.**  A change that
  moves no digest means those 135 games cannot catch a regression in it.
