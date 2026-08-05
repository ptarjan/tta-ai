"""`state.current_events_age` used to be a declared field the engine never
wrote -- see docs/OPEN_ITEMS.md §9.3 -- which froze five of
`neural_encode`'s input slots on age 'A' for the life of every game.

`engine.events._sync_current_events_age` now writes it wherever
`state.current_events` changes (initial deal, every reveal, every recycle).
This file is the positive proof: play a real game and check the field
actually moves.
"""
from __future__ import annotations

import os
import random
import sys
import unittest

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

from engine import actions, cards as C, game            # noqa: E402
from engine.bots import RandomBot                        # noqa: E402
from engine.bots import neural_encode as NE               # noqa: E402

actions.STRICT = True


class CurrentEventsAgeTracksReality(unittest.TestCase):
    def test_the_field_changes_across_a_real_game(self):
        """A 4p random-bot game with the full move cap sees more than one
        age come through the current-events pile."""
        st = game.new_game(4, seed=5)
        self.assertTrue(st.has_military,
                         "this card DB has no military side; the field "
                         "can't move without it")
        bots = [RandomBot(seed=5 + i) for i in range(4)]
        rng = random.Random(5)
        seen = {st.current_events_age}
        steps = 0
        while not st.game_over and steps < 20000:
            actions.apply(st, bots[st.decider()](st), rng)
            seen.add(st.current_events_age)
            steps += 1
        self.assertTrue(st.game_over, "game did not finish within the cap")
        self.assertGreater(
            len(seen), 1,
            f"current_events_age never left {seen} across a whole game")
        # Every value the field ever took is a real age, so the encoder's
        # one-hot never silently drops it into the zero vector.
        self.assertTrue(seen <= set(C.AGES), seen - set(C.AGES))

    def test_the_field_is_always_a_known_age(self):
        """`_onehot_age` only fires a slot for values in `C.AGES`; anything
        else would encode as a silent all-zero, which is the same failure
        mode this whole defect started as."""
        st = game.new_game(3, seed=9)
        bots = [RandomBot(seed=9 + i) for i in range(3)]
        rng = random.Random(9)
        steps = 0
        while not st.game_over and steps < 20000:
            actions.apply(st, bots[st.decider()](st), rng)
            self.assertIn(st.current_events_age, C.AGES)
            onehot = NE._onehot_age(st.current_events_age)
            self.assertEqual(sum(onehot), 1.0)
            steps += 1
        self.assertTrue(st.game_over)


if __name__ == "__main__":
    unittest.main()
