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

import corpus


def _play(players=2, seed=7, bot="weighted"):
    """One played game, cached across the whole file (see tests/corpus.py).

    Nine tests in here wanted the same finished game and each played it from
    scratch, which was most of this file's half-minute per test.  The caller
    still gets its own copy, so the tests that mutate what they are handed --
    and several do -- cannot reach each other."""
    return corpus.played(players, seed, bot)


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
        """WHEN THE PILE HOLDS NOTHING HIDDEN, the feature is exactly the
        engine's own forecast, differenced.

        This test used to make that assertion on the raw finished game, and in
        doing so it PINNED A LEAK: `event_scoring_margin` agreed with
        `final_event_culture` only because both walked the whole face-down
        politics pile by name, including the Age III events the *opponent* had
        prepared (RULES_SPEC 5.3 puts them face down).  A test that asserts a
        forecast equals an omniscient payout is a test that requires the
        forecaster to be omniscient.

        So the position is first made one with no hidden information in it --
        every pending event is credited to me -- and only then are the two
        required to agree.  The masking is the subject of
        `TestForecastSeesOnlyWhatItMay` below; this test's job is to prove that
        the arithmetic on top of the mask is still the engine's.
        """
        checked = 0
        for seed in range(4):
            st = _play(2, seed=seed)
            st.game_over = False          # the feature is gated on this
            for name in list(st.current_events) + list(st.future_events):
                st.seeded_by[name] = 0            # I prepared all of them
            owed = events.final_event_culture(st)
            got = W.event_scoring_margin(st, 0)
            want = owed[0] - owed[1]
            self.assertAlmostEqual(got, max(-60.0, min(60.0, float(want))))
            checked += 1
        self.assertGreater(checked, 0)


class TestForecastSeesOnlyWhatItMay(unittest.TestCase):
    """The feature must not read a card its opponent put face down.

    The rule the project owner set is that anything a human at the table could
    see is fair game and nothing else is.  A prepared event goes into the pile
    FACE DOWN, so its name is not one of those things -- but the pile's height
    is, and so is the printed composition of the Age III deck, which is why
    `engine.bots.counting.event_pool` can still say something useful about it.
    """

    def test_swapping_a_rival_seed_for_another_impact_card_is_invisible(self):
        """THE DIRECT TEST.  Replace the event my opponent hid with a
        different one and my forecast must not move.

        If it moves, I am reading the back of a card.  Substituting a card of
        the same age keeps every public quantity identical -- the pile is the
        same height, the same events have been revealed, the same cards are
        discarded -- so any change in the number came from the name.
        """
        db = C.db()
        moved = same = 0
        for seed in range(8):
            st = _play(2, seed=seed)
            st.game_over = False
            pile = list(st.current_events) + list(st.future_events)
            theirs = [n for n in pile
                      if st.seeded_by.get(n) == 1 and db.age_of(n) == "III"]
            if not theirs:
                continue
            alt = [n for n in db.by_name
                   if db.age_of(n) == "III" and db.type_of(n) == "event"
                   and n not in pile and n not in st.past_events]
            if not alt:
                continue
            base = W.event_scoring_margin(st, 0)
            victim, replacement = theirs[0], alt[0]
            for lst in (st.current_events, st.future_events):
                if victim in lst:
                    lst[lst.index(victim)] = replacement
            st.seeded_by.pop(victim, None)
            st.seeded_by[replacement] = 1
            after = W.event_scoring_margin(st, 0)
            same += 1
            if abs(after - base) > 1e-9:
                moved += 1
        self.assertGreater(same, 0, "no game ever had a rival-seeded Age III "
                                    "event, so this test proved nothing")
        self.assertEqual(moved, 0, f"{moved}/{same} positions changed their "
                         "forecast when a FACE-DOWN rival event was swapped "
                         "for another -- the feature is reading hidden names")

    def test_my_own_seed_is_still_seen(self):
        """The other half: masking must not blind the bot to its OWN plan.

        A mask that returns a constant would pass the test above and destroy
        the feature, which is the "an instrument returning null everywhere has
        two causes" failure.  Swapping a card *I* prepared has to move the
        number, because I know exactly what I put there.
        """
        db = C.db()
        moved = same = 0
        for seed in range(8):
            st = _play(2, seed=seed)
            st.game_over = False
            pile = list(st.current_events) + list(st.future_events)
            mine = [n for n in pile
                    if st.seeded_by.get(n) == 0 and db.age_of(n) == "III"]
            alt = [n for n in db.by_name
                   if db.age_of(n) == "III" and db.type_of(n) == "event"
                   and n not in pile and n not in st.past_events]
            if not mine or not alt:
                continue
            base = W.event_scoring_margin(st, 0)
            for replacement in alt:
                for lst in (st.current_events, st.future_events):
                    if mine[0] in lst:
                        lst[lst.index(mine[0])] = replacement
                st.seeded_by.pop(mine[0], None)
                st.seeded_by[replacement] = 0
                same += 1
                if abs(W.event_scoring_margin(st, 0) - base) > 1e-9:
                    moved += 1
                    break
                mine[0] = replacement
        self.assertGreater(same, 0, "no game gave me an Age III seed")
        self.assertGreater(moved, 0, "swapping an event I prepared MYSELF "
                           "never changed the forecast -- the mask is not a "
                           "mask, it is an off switch")


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

        A SAMPLING test, and it says so: `_play` is self-play, so any pricing
        change re-rolls the move stream and a single seed can stop revealing
        an Age III event without the property having moved at all.  Seed 3
        did exactly that when docs/CARD_BLINDNESS.md landed.  So scan
        seeds for one that actually reveals one, and fail only if NO seed
        does -- which would mean the fixture, not the property, is broken.
        """
        db = C.db()
        checked = 0
        for seed in range(12):
            st = _play(2, seed=seed)
            pending = {n for n, _ in events.pending_final_events(st)}
            age3_past = {n for n in st.past_events
                         if n in db.by_name and db.age_of(n) == "III"}
            if not age3_past:
                continue
            checked += 1
            self.assertFalse(pending & age3_past)
        self.assertTrue(checked, "no seed in 0..11 revealed an Age III event")


class TestFeatureIsWired(unittest.TestCase):

    def test_feature_present_and_default_is_inert(self):
        self.assertIn("event_scoring_margin", W.DEFAULT_WEIGHTS)
        self.assertEqual(W.DEFAULT_WEIGHTS["event_scoring_margin"], 0.0)
        st = _play(2, seed=1)
        st.game_over = False
        f = W.features(st, 0, W.rival_context(st, 0))
        self.assertIn("event_scoring_margin", f)

    def test_it_costs_nothing_when_it_is_not_priced(self):
        """Zero weight must mean zero WORK, not just zero contribution.

        This is the most expensive entry in `features` by a wide margin -- it
        asks `events.final_event_awards` to evaluate fifteen scoring formulas --
        and profiling caught it burning 22% of every board evaluation on a
        weight vector that prices it at 0.0.  `evaluate` now asks `features`
        for the priced entries only; this fails if that gate is ever removed or
        stops covering this term.

        The second half is the half that matters: a gate that skipped the term
        even when it IS priced would be a silent strategy change, so a vector
        that buys it must still pay for it.
        """
        st = corpus.positions(2, seed=7, every=200, limit=400)[-1]
        st.game_over = False
        calls = []
        real = W.event_scoring_margin
        W.event_scoring_margin = (
            lambda *a, **k: (calls.append(1), real(*a, **k))[1])
        try:
            W.evaluate(st, 0, W.DEFAULT_WEIGHTS, W.rival_context(st, 0))
            self.assertEqual(calls, [], "event_scoring_margin was computed at "
                             "its 0.0 default and the result discarded")
            priced = dict(W.DEFAULT_WEIGHTS, event_scoring_margin=0.05)
            W.evaluate(st, 0, priced, W.rival_context(st, 0))
            self.assertTrue(calls, "a vector that PRICES the feature did not "
                            "compute it -- the gate is not a speed switch, it "
                            "is an off switch")
            # instruments still get the whole vector, priced or not
            calls.clear()
            f = W.features(st, 0, W.rival_context(st, 0), W.DEFAULT_WEIGHTS)
            self.assertTrue(calls, "features() skipped an unpriced entry, so "
                            "every instrument reading it now sees a constant")
            self.assertIn("event_scoring_margin", f)
        finally:
            W.event_scoring_margin = real

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


class EveryEventYieldNamesARealWeight(unittest.TestCase):
    """`_EVENT_YIELD`'s feature keys must all be real weights.

    Found while porting this section to Rust.  `_EVENT_YIELD["happiness"]`
    named `"happy"`, which is not a key of `DEFAULT_WEIGHTS` -- only
    `"happy_margin"` is -- so `_event_block_value`'s bare `w.get(fk, 0.0)`
    priced it at zero no matter what the weight said.  Three sibling tables
    (`_TERR_TO_FEATURE` and the two hand tables) already substituted
    `happy_margin` and documented why; this one did not, and nothing failed
    when they disagreed.

    A name check, not a value check: it is the disagreement between the two
    registries that is the bug, and it is the thing no other test could see.
    """

    def test_every_feature_key_is_a_weight(self):
        for raw, (fk, _sign) in W._EVENT_YIELD.items():
            if fk is None:          # `loseAllStoredFood`, priced from the board
                continue
            self.assertIn(
                fk, W.DEFAULT_WEIGHTS,
                f"_EVENT_YIELD[{raw!r}] prices through {fk!r}, which is not a "
                f"weight -- _event_block_value's w.get(fk, 0.0) will silently "
                f"score it 0.0")


if __name__ == "__main__":
    unittest.main()
