"""The rate horizon: is a per-turn rate priced as `rate x turns remaining`?

`engine/bots/weighted.py` used to price every per-turn rate through a flat
weight plus a [0, 1] phase shape, and `rounds_left` -- the state's own estimate
of the horizon -- was read by exactly ONE feature in the whole vector,
`wonder_overrun`.  `rate_horizon` scales the `RATE_KEYS` features by
`rounds_left / mean rounds_left` instead.  See docs/RATE_HORIZON.md.

These tests pin the four things that can silently rot:

  1. the credit at 0.0 is byte-identical to no credit at all, which is what
     lets the gate digests hold and what makes the A/B paired;
  2. `features()` and `feature_marginal()` apply the SAME multiplier -- the
     "one path prices it, the other does not" bug class that
     docs/CARD_BLINDNESS.md was written about;
  3. the multiplier is derived from state and actually falls over a game,
     rather than being a constant with a horizon-shaped name;
  4. it can never flip the sign of a rate, whatever credit the league proposes.
"""
from __future__ import annotations

import os
import sys
import unittest

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

import random                                                  # noqa: E402

from engine import actions as A, game as G                      # noqa: E402
from engine.bots import weighted as W                           # noqa: E402
from engine.bots import WeightedBot                             # noqa: E402


def _off():
    w = dict(W.DEFAULT_WEIGHTS)
    w["rate_horizon"] = 0.0
    return w


def _on(c=1.0):
    w = dict(W.DEFAULT_WEIGHTS)
    w["rate_horizon"] = c
    return w


def _play_to(n, seed, plies):
    """A reachable mid/late-game state, `plies` decisions in."""
    st = G.new_game(n, seed=seed)
    rng = random.Random(seed ^ 0x5EED)
    bots = [WeightedBot(seed=seed * 7 + i) for i in range(n)]
    for _ in range(plies):
        if st.game_over:
            break
        G.apply(st, bots[st.decider()](st), rng)
    return st


class CreditOffIsMasterExactly(unittest.TestCase):
    """0.0 must be a no-op to the last bit, or the gate cannot hold."""

    def test_zero_credit_equals_no_key_at_all(self):
        no_key = dict(W.DEFAULT_WEIGHTS)
        no_key.pop("rate_horizon", None)
        for n, seed, plies in ((2, 1, 60), (3, 2, 90), (4, 1, 40)):
            st = _play_to(n, seed, plies)
            for idx in range(n):
                a = W.features(st, idx, None, _off())
                b = W.features(st, idx, None, no_key)
                self.assertEqual(a, b, f"{n}p seed{seed} seat{idx}")
                self.assertEqual(W.evaluate(st, idx, _off()),
                                 W.evaluate(st, idx, no_key))

    def test_zero_credit_multiplier_is_exactly_one(self):
        st = _play_to(2, 3, 50)
        self.assertEqual(W.rate_multiplier(st, _off()), 1.0)
        self.assertEqual(W.rate_multiplier(st, None), 1.0)

    def test_a_precomputed_feature_dict_prices_the_same_as_a_fresh_one(self):
        """Callers may compute `f` once and hand it to `evaluate`.  Because the
        horizon is on the PRICE and not on the board, that is safe whatever
        vector `f` was built under -- which it would not have been had
        `features()` carried the scaling."""
        st = _play_to(2, 4, 70)
        for w in (_off(), _on(0.5), _on(1.0)):
            f = W.features(st, 0, None, None)
            self.assertAlmostEqual(W.evaluate(st, 0, w, f=dict(f)),
                                   W.evaluate(st, 0, w), places=9)


class TheTwoPathsAgree(unittest.TestCase):
    """`features()` scales the rate; `feature_marginal()` prices one unit of
    the card's PRINTED rate.  If only one of them carries the horizon, a
    theatre is worth a different number to `evaluate` than to `card_potential`
    -- which is exactly the defect docs/CARD_BLINDNESS.md found for the phase
    blend and this test exists so it cannot come back one function over."""

    def test_marginal_is_the_numerical_derivative_of_evaluate(self):
        """`feature_marginal` is the SINGLE definition of what one unit of a
        feature is worth, and the horizon has to be inside it or card pricing
        and `evaluate` disagree about every rate.  Same assertion
        `tests/test_yellow_pricing.py` makes, stated from the horizon's side
        and at three credits."""
        for c in (0.0, 0.5, 1.0):
            w = _on(c)
            for n, seed, plies in ((2, 5, 80), (3, 6, 60)):
                st = _play_to(n, seed, plies)
                for key in ("culture_rate", "science_rate", "food_rate",
                            "resource_rate"):
                    f = W.features(st, 0, None, w)
                    f2 = dict(f)
                    f2[key] = f[key] + 1.0
                    got = (W.evaluate(st, 0, w, f=f2)
                           - W.evaluate(st, 0, w, f=f))
                    self.assertAlmostEqual(
                        got, W.feature_marginal(key, st, 0, w), places=9,
                        msg=f"{key} c={c} {n}p")

    def test_features_report_the_BOARD_and_not_the_price(self):
        """A civilisation producing 5 culture a turn produces 5 culture a turn
        however much game is left.  `board_yields` emits a card's PRINTED
        yield and `tests/test_build_fresh.py` asserts it against the real
        `features()` delta, so scaling the feature would silently break that
        invariant -- which is exactly what the first cut of this change did."""
        st = _play_to(2, 9, 60)
        for key in ("culture_rate", "science_rate", "food_rate",
                    "resource_rate"):
            self.assertEqual(W.features(st, 0, None, _off())[key],
                             W.features(st, 0, None, _on(1.0))[key], key)

    def test_the_horizon_actually_moves_the_card_price(self):
        """A theatre priced under the credit must differ from one priced
        without it, or the whole change is inert plumbing."""
        st = _play_to(2, 7, 120)
        early = _play_to(2, 7, 20)
        for state in (early, st):
            a = W.feature_marginal("culture_rate", state, 0, _off())
            b = W.feature_marginal("culture_rate", state, 0, _on(1.0))
            self.assertNotAlmostEqual(a, b, places=6)


class TheMultiplierIsDerivedAndFalls(unittest.TestCase):

    def test_it_is_high_at_the_deal_and_low_at_the_end(self):
        for n in (2, 3, 4):
            st = G.new_game(n, seed=11)
            deal = W.horizon_scale(st, n)
            self.assertGreater(deal, 1.5, f"{n}p at the deal")
            self.assertLess(deal, 2.1, f"{n}p at the deal")

    def test_it_falls_monotonically_enough_over_a_real_game(self):
        """Not strictly monotone -- `take_rate` is re-measured every decision
        and can revise the estimate up -- but the first quarter must price a
        rate strictly higher than the last quarter, at every player count."""
        for n, seed in ((2, 12), (3, 13), (4, 14)):
            st = G.new_game(n, seed=seed)
            rng = random.Random(seed ^ 0x5EED)
            bots = [WeightedBot(seed=seed * 3 + i) for i in range(n)]
            seq = []
            while not st.game_over and len(seq) < 4000:
                seq.append(W.horizon_scale(st, n))
                G.apply(st, bots[st.decider()](st), rng)
            self.assertGreater(len(seq), 200)
            q = len(seq) // 4
            first = sum(seq[:q]) / q
            last = sum(seq[-q:]) / q
            self.assertGreater(first, last * 3.0,
                               f"{n}p: first quarter {first:.3f} last {last:.3f}")

    def test_the_mean_over_a_game_is_about_one(self):
        """The normalisation claim, and the reason `w['culture_rate']` keeps
        meaning roughly what it means today.  `ref` is the mean of rounds-left
        over a game by construction, so the mean of the ratio should sit near
        1.0; the tolerance is wide because decisions are not uniform over
        rounds (late rounds carry more moves)."""
        for n, seed in ((2, 21), (3, 22), (4, 23)):
            st = G.new_game(n, seed=seed)
            rng = random.Random(seed ^ 0x5EED)
            bots = [WeightedBot(seed=seed * 5 + i) for i in range(n)]
            seq = []
            while not st.game_over and len(seq) < 4000:
                seq.append(W.horizon_scale(st, n))
                G.apply(st, bots[st.decider()](st), rng)
            mean = sum(seq) / len(seq)
            self.assertGreater(mean, 0.6, f"{n}p mean {mean:.3f}")
            self.assertLess(mean, 1.4, f"{n}p mean {mean:.3f}")

    def test_last_round_prices_a_rate_far_below_the_stock(self):
        """The arithmetic the change exists for.  `culture` is FROZEN at 1.0,
        so with the horizon on, a +1 culture RATE on the last round must be
        worth a small multiple of one culture point -- not the ~6x
        (DEFAULT_WEIGHTS) or ~32x (the live 2p champion) a flat weight pays."""
        n, seed = 2, 31
        st = G.new_game(n, seed=seed)
        rng = random.Random(seed ^ 0x5EED)
        bots = [WeightedBot(seed=seed + i) for i in range(n)]
        last = None
        while not st.game_over:
            if st.last_round:
                last = W.feature_marginal("culture_rate", st, 0, _on(1.0))
            G.apply(st, bots[st.decider()](st), rng)
        self.assertIsNotNone(last, "never reached the last round")
        flat = W.feature_marginal("culture_rate", st, 0, _off())
        self.assertLess(last, flat,
                        "the horizon must discount a last-round rate")
        self.assertLess(last, 2.0,
                        f"a last-round +1 culture/turn priced at {last:.3f} "
                        "culture points is still above its own ceiling")


class ItCanNeverInvertARate(unittest.TestCase):
    """A negative or over-unity credit is a strategy the league is entitled to
    propose.  Turning a rate into a LIABILITY is not: docs/CULTURE_GAP.md
    section 8b(i) measured what happened the last time a horizon term was
    allowed out of range (`1 - L` went negative and flipped every `_early`
    term, costing the 4p champion 5 points of win rate)."""

    def test_the_multiplier_is_never_negative(self):
        st = _play_to(2, 41, 100)
        for c in (-5.0, -1.0, -0.25, 0.0, 0.5, 1.0, 2.0, 10.0, 100.0):
            self.assertGreaterEqual(W.rate_multiplier(st, _on(c)), 0.0,
                                    f"credit {c}")

    def test_a_positive_rate_never_prices_negative(self):
        st = _play_to(2, 42, 140)
        for c in (-5.0, 0.0, 1.0, 50.0):
            m = W.feature_marginal("culture_rate", st, 0, _on(c))
            self.assertGreaterEqual(
                m, 0.0, f"credit {c} made +1 culture/turn a liability")


class TheHorizonIsReadByMoreThanOneFeature(unittest.TestCase):
    """The finding this lane opened with, pinned as a regression test.

    Before `rate_horizon`, `weighted.rounds_left` -- the only state-derived
    estimate of how much game is left -- was consumed by exactly one feature,
    `wonder_overrun`.  Every rate went through a [0, 1] shape instead.  If a
    future change makes the rate channel horizon-blind again this fails."""

    def test_rates_respond_to_the_horizon_and_stocks_do_not(self):
        early = _play_to(2, 51, 20)
        late = _play_to(2, 51, 220)
        w = _on(1.0)
        e = W.features(early, 0, None, w)
        l_ = W.features(late, 0, None, w)
        e0 = W.features(early, 0, None, _off())
        l0 = W.features(late, 0, None, _off())
        # the BOARD is untouched at both ends, stocks and rates alike
        for k in ("culture", "science", "resource_stock", "workers",
                  "culture_rate", "food_rate"):
            self.assertEqual(e[k], e0[k], k)
            self.assertEqual(l_[k], l0[k], k)
        # ...and the PRICE of a rate is inflated early and discounted late
        self.assertGreater(W.rate_multiplier(early, w), 1.3)
        self.assertLess(W.rate_multiplier(late, w), 0.8)


class DeadCodeFoundWhilePortingToRust(unittest.TestCase):
    """Two dead-code artifacts found 2026-08-05 while porting this module to
    Rust (`rust/src/bots/weighted/horizon.rs`) and fixed HERE, in the Python
    original, per the repo owner's ruling that a fix found while porting
    belongs in both engines -- not carried forward in Rust alone for shape
    fidelity, which would turn the differential dump into noise."""

    def test_row_constant_was_removed(self):
        """`_ROW = actions.ROW_SIZE` used to sit next to `_SWEEP`/
        `AGE_IV_ROUNDS` in the horizon section.  Unlike those two, nothing in
        this file -- or anywhere else in the tree, grepped -- ever read it.
        Was: a real constant nobody consumed. Now: gone, not ported."""
        self.assertFalse(hasattr(W, "_ROW"))

    def test_horizon_scale_no_longer_accepts_an_unread_weights_argument(self):
        """`horizon_scale(state, n=None, w=None)` never read `w` in its
        body -- `rate_multiplier` and three tests in this file all built a
        full weight dict just to hand it to a parameter the function
        discarded. Was: a parameter with no effect on the return value. Now:
        the signature only carries what it uses."""
        import inspect
        params = list(inspect.signature(W.horizon_scale).parameters)
        self.assertEqual(params, ["state", "n"])


if __name__ == "__main__":
    unittest.main()
