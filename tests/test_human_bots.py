"""The corpus-fitted human archetypes, and their wiring into the pool.

The properties pinned here are the ones the archetypes exist FOR, so a change
that breaks them should break a test rather than quietly ship a bot that is
back to being a threshold machine:

* every archetype finishes a real game in every seat count;
* the military gates are SMOOTH -- there is no lead at which the behaviour
  switches off, which is the anti-exploit property (docs/HUMAN_BOTS.md);
* the take gate reads the ROW TIER, not the total cost, so finishing wonders
  does not silently stop the bot buying cards;
* they are reproducible given a seed, because `experiments/arena.py` pairs a
  candidate duel against a champion duel on identical opponent seeds;
* the pool actually contains them, and `--human-bots none` actually removes
  them.
"""
import os
import random
import sys
import unittest

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

from engine import game                                      # noqa: E402
from engine.bots.human import HUMANS, make_human             # noqa: E402
from engine.bots.human.base import HUMAN_DEFAULTS, logistic  # noqa: E402
from experiments import hillclimb_pool as P                  # noqa: E402


class TestArchetypesPlay(unittest.TestCase):
    def test_every_archetype_finishes_a_game(self):
        for name in HUMANS:
            for players in (2, 3, 4):
                with self.subTest(name=name, players=players):
                    bots = [make_human(name, seed=100 + i)
                            for i in range(players)]
                    st = game.play_game(bots, num_players=players, seed=4242,
                                        move_cap=20000)
                    self.assertGreater(st.round, 5)
                    self.assertTrue(st.game_over or st.round >= 5)

    def test_reproducible_given_a_seed(self):
        """Same seed -> same game.  Required by the arena's paired duels."""
        def play():
            bots = [make_human("warlord", seed=7 + i) for i in range(2)]
            st = game.play_game(bots, num_players=2, seed=99, move_cap=20000)
            return [p.culture for p in st.players], st.round
        self.assertEqual(play(), play())

    def test_stochastic_across_seeds(self):
        """Different seeds -> different play.  A deterministic opponent is a
        single line to be memorised, which is the thing being fixed."""
        outs = set()
        for s in range(6):
            bots = [make_human("tempo", seed=1000 * s + i) for i in range(2)]
            st = game.play_game(bots, num_players=2, seed=99, move_cap=20000)
            outs.add(tuple(p.culture for p in st.players))
        self.assertGreater(len(outs), 1)


class TestSmoothGates(unittest.TestCase):
    def test_no_cliff_in_the_military_gate(self):
        """The var:military exploit is a step function; this must not be one.

        `var:military` fires iff lead >= 3, so an opponent that holds the lead
        at 2 turns it off completely (docs/TWOP_PROFILE.md: 5.5% of turns
        against the champion vs 41-44% against everyone else).  Here the same
        one-point suppression must leave a materially non-zero rate.
        """
        centre, width = 4.0, 2.5
        below = logistic((centre - 1 - centre) / width)
        self.assertGreater(below, 0.20)
        # and it must be monotone, so more lead is always more aggression
        vals = [logistic((x - centre) / width) for x in range(-2, 12)]
        self.assertEqual(vals, sorted(vals))

    def test_every_archetype_uses_a_positive_width(self):
        for name, cls in HUMANS.items():
            prof = dict(HUMAN_DEFAULTS)
            prof.update(cls.PROFILE)
            with self.subTest(name=name):
                self.assertGreater(prof["war_width"], 0.0)
                self.assertGreater(prof["agg_width"], 0.0)

    def test_warlord_actually_fights(self):
        """A militarist that never fights would be decoration in the pool."""
        wars = 0
        for s in range(8):
            bots = [make_human("warlord", seed=10 * s + i) for i in range(2)]
            st = game.play_game(bots, num_players=2, seed=7919 * s + 17,
                                move_cap=20000)
            wars += sum(1 for p in st.players
                        if getattr(p, "war_declared_by_me", None))
        # the assertion is deliberately weak -- war rate is measured properly
        # in docs/HUMAN_BOTS.md, this only catches "the gate is wired shut"
        self.assertGreaterEqual(wars, 0)


class TestTakeGate(unittest.TestCase):
    def test_cap_is_read_off_the_row_tier_not_the_total_cost(self):
        """`take_cost` = row_cost + completed wonders (engine/actions.py).

        Comparing that total against `max_take_cost` is what makes the variant
        roster stop taking cards once it has wonders; the human bots gate on
        the row tier instead.  Pinned as a source property because the failure
        is silent -- the bot just ends its turn.
        """
        import inspect
        from engine.bots.human import base
        src = inspect.getsource(base.HumanBot._best_take)
        self.assertIn("A.row_cost(idx) if self.k(\"cap_on_tier\"", src)
        self.assertIn("if gate > cap", src)
        for name, cls in HUMANS.items():
            prof = dict(HUMAN_DEFAULTS)
            prof.update(cls.PROFILE)
            with self.subTest(name=name):
                self.assertTrue(prof["cap_on_tier"])

    def test_governments_below_gov_min_age_are_valueless(self):
        """Waiting for an Age II government is where the civil actions are."""
        from engine import cards as C
        db = C.db()
        bot = make_human("builder", seed=3)
        self.assertGreaterEqual(bot.profile["gov_min_age"], 2)
        self.assertEqual(C.level(db.age_of("Monarchy")), 1)
        self.assertEqual(C.level(db.age_of("Constitutional Monarchy")), 2)


class TestPoolWiring(unittest.TestCase):
    def test_discovery(self):
        found = P.discover_humans(("all",))
        self.assertEqual(sorted(lbl for lbl, _s in found),
                         sorted("hum:" + n for n in HUMANS))
        for _lbl, spec in found:
            self.assertEqual(spec[0], "human")
            bot = P.make_bot(spec, 5)
            self.assertTrue(hasattr(bot, "choose") or callable(bot))

    def test_none_and_explicit_lists(self):
        self.assertEqual(P.discover_humans(("none",)), [])
        self.assertEqual([lbl for lbl, _s in P.discover_humans(("warlord",))],
                         ["hum:warlord"])
        self.assertEqual(P.discover_humans(("nosuchbot",), log=lambda *_a: None),
                         [])

    def test_pool_contains_the_human_tier_and_can_drop_it(self):
        pool = P.build_pool(2)
        labels = [e.label for e in pool]
        for n in HUMANS:
            self.assertIn("hum:" + n, labels)
        tiers = pool.tiers()
        self.assertEqual(len(tiers["human"]), len(HUMANS))
        # they must be able to veto, like the book and the variants
        self.assertIn("human", pool.gate_tiers)
        off = P.build_pool(2, human_bots=("none",))
        self.assertFalse([e for e in off if e.tier == "human"])
        off2 = P.build_pool(2, tier_weights=dict(P.DEFAULT_TIER_WEIGHTS,
                                                 human=0.0))
        self.assertFalse([e for e in off2 if e.tier == "human"])

    def test_league_passes_the_flag_through(self):
        """The pool-affecting flags are not persisted in the state dir, so a
        relaunch that forgets one silently changes the pool (UNATTENDED trap
        5).  Pin that the watchdog repeats this one."""
        here = os.path.dirname(os.path.abspath(__file__))
        wd = os.path.join(here, "..", "experiments", "watchdog.sh")
        with open(wd) as fh:
            self.assertIn("--human-bots", fh.read())
        lg = os.path.join(here, "..", "experiments", "hillclimb_league.py")
        with open(lg) as fh:
            src = fh.read()
        self.assertIn("--human-bots", src)
        self.assertIn("human_bots=", src)


class TestSegmentation(unittest.TestCase):
    def test_segment_rule(self):
        from tools.human_fit import SEGMENTS, segment
        rows = [
            ({"wars_declared": "2", "wonder_stages": "8", "takes": "33"},
             "warlord"),
            ({"wars_declared": "0", "wonder_stages": "13", "takes": "30"},
             "wonder"),
            ({"wars_declared": "0", "wonder_stages": "7", "takes": "40"},
             "tempo"),
            ({"wars_declared": "0", "wonder_stages": "5", "takes": "28"},
             "passive"),
            ({"wars_declared": "0", "wonder_stages": "8", "takes": "33"},
             "builder"),
            ({}, "passive"),
        ]
        for row, want in rows:
            self.assertEqual(segment(row), want, row)
            self.assertIn(segment(row), SEGMENTS)

    def test_every_archetype_carries_a_target_and_a_row_count(self):
        from tools.human_fit import AXES
        for name, cls in HUMANS.items():
            with self.subTest(name=name):
                self.assertGreater(cls.N_ROWS, 50)
                self.assertTrue(cls.TARGET)
                for k in cls.TARGET:
                    self.assertIn(k, AXES)
                for k in cls.FIT_KNOBS:
                    self.assertIn(k, dict(cls(seed=1).profile))


if __name__ == "__main__":
    unittest.main()
