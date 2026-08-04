"""Being robbed must never raise your own evaluation.

WHAT THIS LOCKS, AND HOW IT WAS FOUND
-------------------------------------
`docs/AGGRESSION_STATUS.md` 3 recorded that the 2p champion gives up defences
it can arithmetically win, and guessed the cause was the search horizon -- the
first defence card leaves the aggression pending, so the outcome is invisible.
That guess was WRONG and the instrument said so: with `QUIET_PENDING` on, the
bot plays the whole four-card defence out in its head, reaches the position
where the aggression has failed, and scores it BELOW `("defend_done",)`.

It was not blind.  It preferred to be robbed:

    champion_2p, fresh 2p board, defender holding 12 food / 12 resources
      lose 4 resources   54.485 -> 54.485   (+0.000)
      lose 3 culture     54.485 -> 55.033   (+0.548)   <== theft HELPS

Two independent inversions, each of which the existing `guard_weights` is
structurally unable to see:

1. `culture` 1.0 with `culture_early` -1.3113 -- net -0.31 for most of the
   game.  The per-key sign guard passes it because `culture` itself is +1.0,
   and the phase multipliers are EXEMPT from that guard by an explicit,
   measured decision (`hillclimb_league`, the EXEMPTION note).  Nothing looked
   at the sum, which is the number `evaluate` actually multiplies by.
2. `resource_stock` 0.0 against `blue_free` 0.4220.  Losing a resource frees
   the blue token it sat on, so the trade was worth +0.42 a resource.  Zero is
   not a sign violation, so again nothing fired.

THE INVARIANT
-------------
This file does not assert either weight.  It asserts the BEHAVIOUR both of
them broke, which is a rule-level fact and survives any retraining: taking
things away from a player, and giving that player nothing back, cannot raise
that player's own evaluation of its position.  A vector that fails this is
not a differently-tuned vector, it is one that will hand over its stuff.

`tests/test_weight_guard.py` covers the per-key sign guard; this covers the
two orderings that guard cannot express.
"""
import glob
import os
import unittest
from unittest import mock

from engine import effects, game
from engine.bots.plan import copy_state
from engine.bots import plan as P
from engine.bots import weighted
from engine.bots.weighted import (DEFAULT_WEIGHTS, dominance_repair, evaluate,
                                  load_weights)
from experiments import hillclimb

HERE = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
LIVE = os.path.join(HERE, "experiments", "league_state")

#: (field, amount).  Every one of these is a pure loss: the defender's stock
#: goes down and nothing anywhere goes up for it.
THEFTS = (("resources", 1), ("resources", 4), ("food", 1), ("food", 4),
          ("science", 3), ("culture", 1), ("culture", 3))


def a_board(seed=5, players=2):
    """A fresh board with something worth stealing in every stock.

    Built by hand rather than played into, so the position cannot drift and
    quietly stop being a position where theft is possible -- a fresh board has
    0 culture and 0 science, under which most of `THEFTS` would be a no-op and
    the test would pass by measuring nothing.
    """
    st = game.new_game(players, seed)
    p = st.players[1]
    p.food, p.resources, p.science, p.culture = 12, 12, 8, 8
    effects.invalidate(st)
    return st


def robbed(st, idx, field, amount):
    t = copy_state(st)
    p = t.players[idx]
    setattr(p, field, max(0, getattr(p, field) - amount))
    effects.invalidate(t)
    return t


def trained_vectors():
    """Every weight vector that plays or has played, live files included."""
    out = [("DEFAULT_WEIGHTS", dict(DEFAULT_WEIGHTS))]
    for path in sorted(glob.glob(os.path.join(LIVE, "champion_*p.json"))):
        out.append((os.path.basename(path), load_weights(path)))
    return out


class TheftNeverHelps(unittest.TestCase):

    def test_the_fixture_really_has_something_to_steal(self):
        """Guard the fixture: a green run must not mean an empty one."""
        st = a_board()
        p = st.players[1]
        for field, amount in THEFTS:
            self.assertGreaterEqual(getattr(p, field), amount, field)
            t = robbed(st, 1, field, amount)
            self.assertEqual(getattr(t.players[1], field),
                             getattr(p, field) - amount)

    def test_no_trained_vector_prefers_being_robbed(self):
        st = a_board()
        ctx = P.rival_context(st, 1)
        for label, w in trained_vectors():
            base = evaluate(st, 1, w, ctx)
            for field, amount in THEFTS:
                v = evaluate(robbed(st, 1, field, amount), 1, w, ctx)
                self.assertLessEqual(
                    v, base + 1e-9,
                    f"{label}: losing {amount} {field} RAISED its own score "
                    f"{base:.4f} -> {v:.4f}.  A bot that scores theft as a "
                    f"gain declines defences it can win and offers no "
                    f"resistance to an aggression.  See "
                    f"`weighted.dominance_repair`.")

    def test_losing_culture_is_always_a_real_loss(self):
        """Culture is the score, so this one is not allowed to be a tie either.

        Separated from the sweep above because indifference is a legitimate
        answer for a stock the vector genuinely does not price (champion_4p
        holds `science` at 0.0), and is never a legitimate answer for the
        thing the game is won on.
        """
        st = a_board()
        ctx = P.rival_context(st, 1)
        for label, w in trained_vectors():
            base = evaluate(st, 1, w, ctx)
            v = evaluate(robbed(st, 1, "culture", 3), 1, w, ctx)
            self.assertLess(v, base, f"{label}: 3 culture cost it nothing")


class TheRepairIsMinimalAndIdempotent(unittest.TestCase):

    def test_a_legal_vector_is_returned_untouched(self):
        """NEGATIVE CONTROL: the guard must not rewrite vectors that are fine."""
        out, viol = dominance_repair(dict(DEFAULT_WEIGHTS))
        self.assertEqual(viol, [])
        self.assertEqual(out, dict(DEFAULT_WEIGHTS))

    def test_repairing_twice_changes_nothing_the_second_time(self):
        bad = dict(DEFAULT_WEIGHTS, blue_free=9.0,
                   wonder_stages_per_action=-2.0)
        once, v1 = dominance_repair(bad)
        twice, v2 = dominance_repair(once)
        self.assertTrue(v1)
        self.assertEqual(v2, [])
        self.assertEqual(once, twice)

    def test_the_net_negative_repair_still_works_if_a_pair_comes_back(self):
        """`NET_NONNEG_PHASE` is empty now that `culture` and
        `wonder_progress` have no phase pair at all, so nothing in the shipped
        vector reaches this branch.  It is still the code the NEXT
        phase-multiplied stock would land on, and an empty guard that has never
        been executed is a guard nobody knows is broken -- so drive it with a
        synthetic key.  Repaired to the BOUNDARY, not to the default: the
        smallest change that makes the vector expressible.
        """
        w = dict(DEFAULT_WEIGHTS, culture_early=-3.0)
        out, viol = weighted.dominance_repair(w)
        self.assertEqual(out["culture_early"], -3.0,
                         "culture has no phase pair any more, so the guard "
                         "must not touch a stray key of that name")
        self.assertEqual(viol, [])

        with mock.patch.object(weighted, "NET_NONNEG_PHASE", ("culture",)):
            out, viol = weighted.dominance_repair(w)
        self.assertEqual(out["culture_early"], -out["culture"])
        self.assertEqual([v["weight"] for v in viol], ["culture_early"])

    def test_the_resource_pair_is_repaired_by_raising_the_dominant_side(self):
        out, viol = dominance_repair(dict(DEFAULT_WEIGHTS, resource_stock=0.0,
                                          blue_free=0.4220))
        self.assertEqual(out["resource_stock"], 0.4220)
        self.assertEqual(out["blue_free"], 0.4220,
                         "the climbed side is what was measured; the repair "
                         "must not throw it away")

    def test_a_phase_multiplier_may_still_go_negative(self):
        """NEGATIVE CONTROL, and the reason this is not a blanket rule.

        The other phase-multiplied terms are entitled to a negative net: more
        workers costs consumption, resource production on the last turn really
        is close to worthless.  Only terms in `NET_NONNEG_PHASE` are pinned,
        and a guard that pinned all of them would be overriding strategy the
        league is allowed to learn.
        """
        out, viol = dominance_repair(dict(DEFAULT_WEIGHTS, workers_early=-9.0,
                                          tech_levels_late=-9.0))
        self.assertEqual(out["workers_early"], -9.0)
        self.assertEqual(out["tech_levels_late"], -9.0)
        self.assertEqual(viol, [])


class PhaseKeysAreNotDoubleModelled(unittest.TestCase):
    """The structural half of the 2026-08-04 cleanup.

    Six phase pairs were retired because the quantity they approximated -- how
    many turns you will still collect a rate for -- is now modelled exactly by
    `rate_horizon`.  Deleting them is only durable if something notices them
    coming back, and a comment does not notice.
    """

    def test_no_rate_is_also_phase_multiplied(self):
        """A RATE_KEY carries the exact horizon (`rate_multiplier`).  Giving it
        a phase pair as well stacks a second, fitted time shape on top of the
        exact one, which is the state this cleanup ended."""
        both = sorted(set(weighted.PHASE_KEYS) & set(weighted.RATE_KEYS))
        self.assertEqual(
            both, [],
            "%s is priced through BOTH `rate_multiplier` and an early/late "
            "pair -- two time models multiplying one value.  Pick one." % both)

    def test_the_numeraire_has_no_phase_pair(self):
        """`culture` is frozen at 1.0 so every other weight is denominated in
        culture points.  A phase pair on it is NOT frozen, so it is a live
        rescale of the objective -- a gauge the search can move instead of
        finding a better move, and it did: `culture_early` reached -1.3113."""
        for k in hillclimb.FROZEN:
            self.assertNotIn(
                k, weighted.PHASE_KEYS,
                f"{k} is FROZEN, but its early/late pair is not -- that is a "
                f"back door onto a weight the trainer is forbidden to touch")

    def test_every_phase_key_has_exactly_one_pair_and_no_orphans(self):
        """The registry half: PHASE_KEYS and the shipped multipliers must be
        the same set in both directions.  A multiplier for a retired key is a
        weight the trainer mutates and `evaluate` never reads."""
        want = {k + s for k in weighted.PHASE_KEYS
                for s in ("_early", "_late")}
        have = {k for k in DEFAULT_WEIGHTS if k.endswith(("_early", "_late"))}
        self.assertEqual(have, want)


if __name__ == "__main__":
    unittest.main()
