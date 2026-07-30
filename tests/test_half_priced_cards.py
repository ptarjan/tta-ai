"""A card whose COST is priced and whose GAIN is not is biased, not neutral.

THE RULE THIS FILE MAKES MECHANICAL
-----------------------------------
Adding a 0.0-default feature for one side of a trade whose other side is
already priced does not leave the card neutral.  It BIASES it, and the
direction depends on which side you just made visible.

"Inert" is a claim about the weight VECTOR -- a champion trained before the
change scores identically, which is true and worth having.  It is not a claim
about the CARD.  A card whose `techCost` is priced through `science` (a trained
weight) and whose entire benefit is priced through weights that default to 0.0
resolves to "pay this, receive nothing".  It is not un-modelled, it is
MIS-modelled, and it is mis-modelled in a known direction: the bot will refuse
to take it.

The bug class has now been found twice in opposite directions:

* benefit at 0.0, cost trained -> the card reads as pure cost and is never
  taken.  The three Construction special technologies below, measured at a
  0.87% take rate across 1,606 offers with six of the twelve special techs
  never taken once in 40 player-games (docs/UNCOVERED_TYPES.md section 2).
* cost at 0.0, benefit trained -> the card reads as too cheap.  The
  governments, whose action counts are top-level fields `_card_yields` never
  read (docs/CARD_PRICING_LEADERS.md, and `TOP_LEVEL_UNPRICED` in
  tests/test_card_pricing.py).

WHY THIS IS A CENSUS AND NOT AN ASSERTION OF ZERO
-------------------------------------------------
The set below is not required to be empty and should not be.  Deferring "how
much of this to believe" to a weight the league can find is the project's
convention and is usually right (see `unit_strength_credit`'s note in
engine/bots/weighted.py for the reasoning, which is measured rather than
cautious).  What is required is that the set is WRITTEN DOWN, so that deferring
one more card's value is a visible event with a name attached rather than a
silent behavioural change wearing an inert label.

WHY NOT JUST CLAMP `card_potential` AT ZERO
--------------------------------------------
Because the negative is load-bearing INSTRUMENTATION.  Developing a technology
is optional, so "the value of holding a card is max(0, value if played)" is a
sound argument about the develop decision -- but
`tests/test_card_pricing.py::test_an_age_ii_cavalry_and_artillery_are_no_longer
_the_same_card` proves the cost of acting on it: with a floor, Modern Infantry
prices 0.000 at `unit_strength_credit` 0.0 AND 0.000 at 1.0, so a real pricing
improvement becomes unmeasurable.  A clamp would hide this file's subject
matter instead of reporting it.  Measured, not assumed: see
docs/UNCOVERED_TYPES.md section 2.4.
"""
import unittest

from engine import cards as C
from engine.bots import weighted as W


#: Cards whose priced GAIN contributes exactly 0.0 under `DEFAULT_WEIGHTS`
#: while their priced COST does not.  Each entry names the weight the value is
#: waiting behind, so that "who is meant to fix this" is never a guess.
HALF_PRICED = {
    # --- the ten military units.  Their strength IS mapped by `_card_yields`;
    # only how much of it to believe is deferred, behind a credit whose 0.0
    # default is argued from measurement in engine/bots/weighted.py.
    "Warriors": "unit_strength_credit",
    "Swordsmen": "unit_strength_credit",
    "Riflemen": "unit_strength_credit",
    "Modern Infantry": "unit_strength_credit",
    "Knights": "unit_strength_credit",
    "Cavalrymen": "unit_strength_credit",
    "Tanks": "unit_strength_credit",
    "Cannon": "unit_strength_credit",
    "Rockets": "unit_strength_credit",
    "Air Forces": "unit_strength_credit",
    # --- the three Construction special technologies.  Both of their effect
    # keys are mapped and both weights default to 0.0, so the whole card
    # resolves to its science cost.  `build_discount` is denominated in
    # RESOURCES -- the same unit `resource_stock` already prices `buildCost`
    # in -- so it is the one of the two a future change could convert rather
    # than train; what is unknown is not the unit but the count of urban
    # buildings the discount will still apply to, measured at 4.45 per
    # player-game in docs/UNCOVERED_TYPES.md section 2.6.
    "Masonry": "build_discount / wonder_stages_per_action",
    "Architecture": "build_discount / wonder_stages_per_action",
    "Engineering": "build_discount / wonder_stages_per_action",
}


def _gain_and_cost(name, w):
    """(gain, cost) contribution of a card under `w`, split by yield kind.

    Uses `_sum_yields` rather than re-implementing the weighting, so this
    cannot drift from what `card_potential` actually computes -- which is the
    failure mode tests/test_card_pricing.py exists to prevent, and it would be
    embarrassing to reproduce it here.
    """
    triples = W._card_yields(name)
    credit = w.get("card_rate_credit", 1.0)
    gain = W._sum_yields([t for t in triples if t[2] != W._Y_COST], w, credit)
    cost = W._sum_yields([t for t in triples if t[2] == W._Y_COST], w, credit)
    return gain, cost


def _half_priced(w):
    out = {}
    for name in C.db().by_name:
        gain, cost = _gain_and_cost(name, w)
        if cost < -1e-12 and abs(gain) < 1e-12:
            out[name] = round(cost, 3)
    return out


class TestHalfPricedCards(unittest.TestCase):

    def test_the_set_is_exactly_what_is_written_down(self):
        got = _half_priced(W.DEFAULT_WEIGHTS)
        self.assertEqual(
            sorted(got), sorted(HALF_PRICED),
            "the set of cards whose cost is priced and whose gain is not has "
            "changed.\n"
            "If you ADDED one: you have not made a card inert, you have made "
            "the bot refuse to take it.  Either map its gain to a feature "
            "with a trained weight, or add it to HALF_PRICED naming the "
            "weight its value is waiting behind.\n"
            "If you REMOVED one: delete it from HALF_PRICED.\n"
            f"computed: {got}")

    def test_every_entry_names_the_weight_it_waits_behind(self):
        for name, weight in HALF_PRICED.items():
            self.assertIsInstance(weight, str)
            self.assertGreater(len(weight), 8, name)
            for key in weight.replace("/", " ").split():
                self.assertIn(key, W.DEFAULT_WEIGHTS, f"{name}: {key}")
                self.assertEqual(W.DEFAULT_WEIGHTS[key], 0.0,
                                 f"{name}: {key} is no longer 0.0, so this "
                                 f"card is no longer waiting on it")

    def test_no_entry_is_stale(self):
        """Same discipline as DELIBERATELY_UNPRICED: an entry naming a card
        that no longer exists is rot."""
        names = set(C.db().by_name)
        self.assertEqual(sorted(set(HALF_PRICED) - names), [])

    def test_turning_a_credit_on_moves_the_cards_that_wait_on_it(self):
        """The set is not a constant of the code, it is a function of the
        weights -- which is the whole point.  Believing unit strength takes
        all ten units out of it."""
        on = dict(W.DEFAULT_WEIGHTS, unit_strength_credit=1.0)
        got = _half_priced(on)
        self.assertEqual(sorted(got),
                         ["Architecture", "Engineering", "Masonry"])
