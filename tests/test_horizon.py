"""The game horizon: `lateness()` must track remaining rounds, not the age.

docs/CULTURE_GAP.md section 4: the old `lateness()` was
`min(1.0, C.level(state.age_civil) / 3.0)`, saturated at 1.0 from Age III on,
so a production rate bought in Age III (6.2 rounds left, measured) was priced
exactly the same as one bought in Age IV (2.0 rounds left).
`test_age_iii_and_age_iv_are_not_the_same_price` is that defect and fails
against the old function.
"""
import os
import sys
import unittest

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

from engine import cards as C, game  # noqa: E402
from engine.bots import weighted as W  # noqa: E402


def _state(n=4, age="II", deck=20, final=None, rnd=10):
    st = game.new_game(n, 17)
    st.age_civil = age
    st.age_military = age
    st.civil_deck = ["x"] * deck
    st.final_round_end = final
    st.round = rnd
    return st


class Horizon(unittest.TestCase):

    # ------------------------------------------------ rounds_left estimator

    def test_exact_once_age_iv_has_begun(self):
        st = _state(age="IV", deck=0, final=24, rnd=23)
        self.assertEqual(W.rounds_left(st), 2.0)
        st.round = 24
        self.assertEqual(W.rounds_left(st), 1.0)

    def test_never_below_one_round(self):
        st = _state(age="IV", deck=0, final=20, rnd=99)
        self.assertEqual(W.rounds_left(st), 1.0)

    def test_monotone_in_cards_still_to_deal(self):
        prev = None
        for age in ("A", "I", "II", "III"):
            for deck in (40, 30, 20, 10, 0):
                st = _state(age=age, deck=deck)
                rl = W.rounds_left(st)
                if prev is not None:
                    self.assertLessEqual(rl, prev + 1e-9,
                                         f"{age}/{deck} went back up")
                prev = rl

    def test_a_full_game_is_the_right_order_of_magnitude(self):
        """Measured game lengths: ~23 rounds at 2p/3p, ~29 at 4p."""
        for n, lo, hi in ((2, 18, 30), (3, 18, 32), (4, 22, 38)):
            st = game.new_game(n, 17)
            self.assertTrue(lo <= W.rounds_left(st) <= hi,
                            f"{n}p opening horizon {W.rounds_left(st):.1f} "
                            f"outside [{lo}, {hi}]")

    def test_resignations_shrink_the_decks(self):
        st = _state(n=4, age="I", deck=30)
        four = W.rounds_left(st)
        st.players[3].resigned = True
        st.players[2].resigned = True
        self.assertNotEqual(W.rounds_left(st), four)

    # -------------------------------------------------------- lateness()

    def test_age_iii_and_age_iv_are_not_the_same_price(self):
        """THE defect.  The old age bucket returned 1.0 for both."""
        early_iii = _state(age="III", deck=44)     # a full Age III deck ahead
        age_iv = _state(age="IV", deck=0, final=24, rnd=23)
        self.assertGreater(W.lateness(age_iv), W.lateness(early_iii) + 0.15,
                           "Age IV must be priced later than early Age III")
        # ... and the function it replaces did NOT do this
        self.assertEqual(W.lateness_by_age(early_iii),
                         W.lateness_by_age(age_iv))

    def test_lateness_varies_inside_a_single_age(self):
        wide = [W.lateness(_state(age="II", deck=d)) for d in (44, 30, 15, 0)]
        self.assertEqual(wide, sorted(wide))
        self.assertGreater(wide[-1] - wide[0], 0.15)
        flat = {W.lateness_by_age(_state(age="II", deck=d))
                for d in (44, 30, 15, 0)}
        self.assertEqual(len(flat), 1)

    def test_never_leaves_the_unit_interval(self):
        """`1 - L` must not change sign: see the docstring on `lateness`.
        The 4p champion's `culture_early` is 8.792, so a negative `1 - L`
        prices its own frozen culture weight at 0.156 instead of 1.000."""
        for n in (2, 3, 4):
            for age, deck, final, rnd in (("A", 7, None, 1),
                                          ("I", 44, None, 4),
                                          ("III", 0, None, 20),
                                          ("IV", 0, 24, 24)):
                st = _state(n=n, age=age, deck=deck, final=final, rnd=rnd)
                self.assertTrue(0.0 <= W.lateness(st) <= 1.0,
                                f"{n}p {age}: L={W.lateness(st)}")

    def test_zero_at_the_start_and_about_one_at_the_end(self):
        for n in (2, 3, 4):
            st = game.new_game(n, 17)
            self.assertLessEqual(W.lateness(st), 0.25)
            last = _state(n=n, age="IV", deck=0, final=24, rnd=24)
            self.assertEqual(W.lateness(last), 1.0)

    def test_the_gauge_is_the_exact_supply_fraction(self):
        """SUPERSEDES `test_calibration_against_the_old_schedule`.

        That test asserted the new gauge stayed within 0.10 of the OLD age
        bucket at the measured per-age mean decks, because the gauge was
        deliberately spent on not moving three already-trained champions.
        Two of those three (3p, 4p) are back at generation 0 on
        `DEFAULT_WEIGHTS`, so there is nothing left to protect and the gauge
        is spent on being right instead: L is now the exact share of the
        game's civil card supply already dealt.  No fit, no per-player-count
        table, no dependence on the ESTIMATED `rounds_left`.

        Deck sizes below are the same measured per-age means from 46 self-play
        games -- i.e. this pins the honest quantity at exactly the positions
        the retired affine map was least-squares fitted on.
        """
        db = C.db()
        for n, seen in ((2, {"I": 20.9, "II": 21.8, "III": 22.2}),
                        (3, {"I": 23.1, "II": 24.8, "III": 24.8}),
                        (4, {"I": 22.4, "II": 26.9, "III": 26.1})):
            supply = sum(len(db.civil_deck(a, n))
                         for a in ("A", "I", "II", "III"))
            prev = -1.0
            for age in ("I", "II", "III"):
                deck = int(round(seen[age]))
                st = _state(n=n, age=age, deck=deck)
                want = 1.0 - (deck + W._tail(n, age)) / float(supply)
                self.assertAlmostEqual(W.lateness(st), want, places=12,
                                       msg=f"{n}p age {age}")
                self.assertGreater(want, prev, f"{n}p age {age} went back")
                prev = want

    def test_the_legacy_hatch_restores_the_fitted_affine_gauge(self):
        """`LEGACY_LATENESS` is the A/B switch and the fingerprint control:
        with it on, `lateness` is the retired `(z - rounds_left)/(z - 5)` to
        the bit, which is what makes the gate attribution a one-cause claim."""
        st = _state(n=2, age="II", deck=22)
        W.LEGACY_LATENESS = W.LEGACY_DEAL_RATE = True
        try:
            got = W.lateness(st)
            z = W._L_ZERO[2]
            want = (z - W.rounds_left(st, 2)) / (z - W._L_ONE)
        finally:
            W.LEGACY_LATENESS = W.LEGACY_DEAL_RATE = False
        self.assertAlmostEqual(got, want, places=12)
        self.assertNotAlmostEqual(got, W.lateness(st), places=3)

    # ------------------------------------------- the measured deal rate

    def test_the_take_rate_is_measured_not_assumed(self):
        """A row that has been drained hard reports a higher take rate than
        one nobody has touched, from the SAME turn counter -- which is the
        whole point: the estimator tracks the policy at the table.

        Both states are the same number of replenishes in; they differ only
        in how many cards have left the decks, which is public.
        """
        lazy = _state(n=2, age="II", deck=40)
        lazy.turn, lazy.round = 12, 6
        keen = _state(n=2, age="II", deck=10)
        keen.turn, keen.round = 12, 6
        self.assertGreater(W.take_rate(keen), W.take_rate(lazy))
        self.assertLess(W.rounds_left(keen), W.rounds_left(lazy))

    def test_the_take_rate_is_never_negative_and_never_absurd(self):
        for n in (2, 3, 4):
            for deck in (0, 20, 44, 200):
                for turn in (-5, 0, 1, 5, 40, 400):
                    st = _state(n=n, age="II", deck=deck)
                    st.turn = turn
                    self.assertGreaterEqual(W.take_rate(st), 0.0)
                    self.assertGreaterEqual(W.rounds_left(st), 1.0)

    def test_the_sweep_half_of_the_rate_is_the_rule_not_a_fit(self):
        """`n * SWEEP[n]` cards leave the decks per round by rule (2.1), so
        the estimator can never predict a game longer than the sweep alone
        allows.  This is the property the fitted constant could not offer:
        with a card-hungry policy the fit runs long, and this cannot."""
        for n in (2, 3, 4):
            st = _state(n=n, age="I", deck=44)
            st.turn, st.round = 3 * n, 4
            cards = W.cards_unseen(st, n)
            self.assertLessEqual(W.rounds_left(st, n),
                                 cards / float(n * W._SWEEP[n])
                                 + W.AGE_IV_ROUNDS + 1e-9)

    # ---------------------------------------------------- the A/B hatch

    def test_horizon_age_escape_hatch_selects_the_old_schedule(self):
        st = _state(age="III", deck=44)
        w_new = dict(W.DEFAULT_WEIGHTS)
        w_old = dict(W.DEFAULT_WEIGHTS, horizon_age=1.0)
        self.assertNotEqual(W.evaluate(st, 0, w_new), W.evaluate(st, 0, w_old))
        # and the hatch is NOT part of the trained vector
        self.assertNotIn("horizon_age", W.DEFAULT_WEIGHTS)

    def test_the_hatch_is_inert_when_the_two_schedules_agree(self):
        """No phase weights -> the horizon cannot change the evaluation."""
        st = _state(age="III", deck=44)
        flat = {k: v for k, v in W.DEFAULT_WEIGHTS.items()
                if not (k.endswith("_early") or k.endswith("_late"))}
        self.assertAlmostEqual(W.evaluate(st, 0, dict(flat)),
                               W.evaluate(st, 0, dict(flat, horizon_age=1.0)))

    # -------------------------------------------------------- book-keeping

    def test_tail_table_matches_the_card_data(self):
        db = C.db()
        for n in (2, 3, 4):
            for age in ("A", "I", "II", "III"):
                lv = C.level(age)
                want = sum(len(db.civil_deck(a, n))
                           for a in C.AGES[lv + 1:] if a != "IV")
                self.assertEqual(W._tail(n, age), want, f"{n}p after {age}")
            self.assertEqual(W._tail(n, "III"), 0)


if __name__ == "__main__":
    unittest.main()
