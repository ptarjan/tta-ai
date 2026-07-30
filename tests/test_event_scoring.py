"""The evaluator's Age III forecast must agree with the engine's payout.

`engine/bots/weighted.event_scoring_margin` tells the bot what the pending
"Impact of ..." events are worth to it.  If that number and the culture
`engine/events.evaluate_final_events` actually awards can drift apart, the
bot is optimising a quantity the game does not pay -- which is the same class
of bug as the dropped `effects.culture` mapping in docs/CARD_BLINDNESS.md,
one level up.

They cannot drift here because both call `events.final_event_culture`.  These
tests pin that: `test_forecast_equals_payout` plays real games and asserts the
forecast taken one step before scoring equals the culture the engine then
adds, and `test_margin_is_a_difference_of_the_forecast` asserts the feature is
exactly that forecast differenced against the best rival.

The remaining tests are the ones that would have caught the two ways this
feature can be wrong in a way no game would notice: double counting after the
payout has happened, and counting events that already paid out on reveal.
"""
import unittest

from engine import cards as C, events, game
from engine.bots import weighted as W
from engine.bots.fastcopy import copy_state


def _play(players=2, seed=7, bot="weighted"):
    from engine.bots import make_bots
    bots = make_bots(bot, players, seed=seed)
    return game.play_game(bots, players, seed=seed)


class TestForecastMatchesPayout(unittest.TestCase):

    def test_forecast_equals_payout(self):
        """`final_event_culture` predicts what `evaluate_final_events` adds."""
        checked = 0
        for seed in range(6):
            st = _play(2, seed=seed)
            # rewind is not available, so rebuild the pre-scoring position by
            # scoring a copy: the copy's delta IS the payout.
            before = [p.culture for p in st.players]
            scratch = copy_state(st)
            forecast = events.final_event_culture(scratch)
            events.evaluate_final_events(scratch)
            paid = [q.culture - b for q, b in zip(scratch.players, before)]
            # `evaluate_final_events` clamps at zero; the forecast is raw, so
            # compare only where no clamp could have fired.
            for f, got, b in zip(forecast, paid, before):
                if b + f >= 0:
                    self.assertEqual(f, got)
                    checked += 1
        self.assertGreater(checked, 0, "no games produced a comparison")

    def test_margin_is_a_difference_of_the_forecast(self):
        for seed in range(4):
            st = _play(2, seed=seed)
            st.game_over = False          # the feature is gated on this
            owed = events.final_event_culture(st)
            got = W.event_scoring_margin(st, 0)
            want = owed[0] - owed[1]
            self.assertAlmostEqual(got, max(-60.0, min(60.0, float(want))))


class TestSingleImplementation(unittest.TestCase):
    """The payout and the forecast must stay ONE calculation.

    `final_event_awards` is the only place the fifteen scoring formulas are
    evaluated; `evaluate_final_events` applies its steps and
    `final_event_culture` sums them.  Shared source of truth decays back into
    two implementations the first time somebody optimises one side, so these
    tests fail on divergence rather than trusting the arrangement.
    """

    def test_payout_is_exactly_the_awards(self):
        """Every culture point `evaluate_final_events` adds comes from a step.

        Fails if `evaluate_final_events` ever grows its own scoring branch
        again: any award it applies that `final_event_awards` did not produce
        shows up as a mismatch here.
        """
        for seed in range(6):
            st = _play(2, seed=seed)
            scratch = copy_state(st)
            awards = events.final_event_awards(scratch)
            before = [p.culture for p in scratch.players]
            # replay the clamp the engine applies, from the steps alone
            want = list(before)
            for _name, steps in awards:
                for idx, amount in steps:
                    if amount:
                        want[idx] = max(0, want[idx] + amount)
            events.evaluate_final_events(scratch)
            got = [p.culture for p in scratch.players]
            self.assertEqual(want, got)

    def test_culture_is_the_sum_of_the_awards(self):
        for seed in range(6):
            st = _play(2, seed=seed)
            awards = events.final_event_awards(st)
            want = [0] * len(st.players)
            for _name, steps in awards:
                for idx, amount in steps:
                    want[idx] += amount
            self.assertEqual(want, events.final_event_culture(st))

    def test_awards_cover_exactly_the_pending_events(self):
        for seed in range(6):
            st = _play(2, seed=seed)
            self.assertEqual([n for n, _ in events.final_event_awards(st)],
                             [n for n, _ in events.pending_final_events(st)])


class TestNoDoubleCount(unittest.TestCase):

    def test_zero_once_the_game_is_over(self):
        """`_finish_game` has already banked this culture into `p.culture`.

        The event names stay in the decks afterwards, so a forecast that did
        not check `game_over` would count the endgame twice at every leaf of
        a search that reached the end of the game.
        """
        for seed in range(4):
            st = _play(2, seed=seed)
            self.assertTrue(st.game_over)
            for i in range(2):
                self.assertEqual(W.event_scoring_margin(st, i), 0.0)

    def test_past_events_are_excluded(self):
        """An Age III event revealed during play already paid on reveal.

        `_apply_player_block` runs `scoring_culture` when the card comes up,
        so counting `past_events` in the forecast would promise culture the
        player has already been given.
        """
        db = C.db()
        st = _play(2, seed=3)
        pending = {n for n, _ in events.pending_final_events(st)}
        age3_past = {n for n in st.past_events
                     if n in db.by_name and db.age_of(n) == "III"}
        self.assertTrue(age3_past, "seed 3 revealed no Age III event")
        self.assertFalse(pending & age3_past)


class TestFeatureIsWired(unittest.TestCase):

    def test_feature_present_and_default_is_inert(self):
        self.assertIn("event_scoring_margin", W.DEFAULT_WEIGHTS)
        self.assertEqual(W.DEFAULT_WEIGHTS["event_scoring_margin"], 0.0)
        st = _play(2, seed=1)
        st.game_over = False
        f = W.features(st, 0, W.rival_context(st, 0))
        self.assertIn("event_scoring_margin", f)

    def test_feature_moves_when_a_scoring_event_is_pending(self):
        """Guard against shipping a dead coordinate (CARD_BLINDNESS 5.1).

        If this feature were always zero on real boards it would be exactly
        the `wonder_stages_per_action` outcome: mapped, tested, and unable to
        change a decision.
        """
        seen_nonzero = False
        for seed in range(8):
            st = _play(2, seed=seed)
            st.game_over = False
            if W.event_scoring_margin(st, 0) != 0.0:
                seen_nonzero = True
                break
        self.assertTrue(seen_nonzero,
                        "event_scoring_margin was 0.0 in every game")


if __name__ == "__main__":
    unittest.main()
