"""`--candidate-bot`: train the weights under the search that will run them.

Every trained vector used to reach the arena as a bare dict, and
`arena.make_bot`'s fallthrough turns a bare dict into a 1-ply `WeightedBot` --
so the loop could only train weights for greedy 1-ply play even though
`PlanBot` and `QuiescentBot` read the identical vector.  These tests pin the
three things that make the wrapping safe:

  * a trained vector becomes the requested bot, carrying its own weights;
  * a mirror / past-ladder opponent (also a dict) is wrapped too, so those
    tiers measure a weight gap and not an architecture gap;
  * an EXTERNAL opponent spec (str or tuple) is never wrapped, so the pool
    does not silently get more expensive.
"""
import os
import sys
import unittest

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

from engine.bots.weighted import DEFAULT_WEIGHTS, WeightedBot  # noqa: E402
from experiments import arena  # noqa: E402
from experiments import hillclimb_league as HL  # noqa: E402


class ParseCandidateBot(unittest.TestCase):

    def test_default_is_one_ply(self):
        for text in (None, "", "weighted", "1ply", "default"):
            with self.subTest(text=text):
                self.assertIsNone(HL.parse_candidate_bot(text))

    def test_kind_and_opts(self):
        self.assertEqual(HL.parse_candidate_bot("quiescent"), ("quiescent", {}))
        self.assertEqual(HL.parse_candidate_bot("quiescent:levels=1"),
                         ("quiescent", {"levels": 1}))
        self.assertEqual(HL.parse_candidate_bot("plan:width=8,samples=2,det=1"),
                         ("plan", {"width": 8, "samples": 2, "det": 1}))

    def test_unknown_architecture_is_fatal(self):
        with self.assertRaises(SystemExit):
            HL.parse_candidate_bot("mcts")


class AsSpec(unittest.TestCase):

    def setUp(self):
        self._saved = HL.CANDIDATE_ARCH

    def tearDown(self):
        HL.CANDIDATE_ARCH = self._saved

    def test_one_ply_is_a_bare_dict_passthrough(self):
        HL.CANDIDATE_ARCH = None
        w = dict(DEFAULT_WEIGHTS)
        self.assertIs(HL.as_spec(w), w)
        self.assertIsInstance(arena.make_bot(HL.as_spec(w), 1), WeightedBot)

    def test_a_trained_vector_becomes_the_requested_bot(self):
        w = dict(DEFAULT_WEIGHTS, culture_rate=7.5)
        for kind, opts, mod, cls in (
                ("quiescent", {"levels": 1}, "engine.bots.quiescent",
                 "QuiescentBot"),
                ("plan", {"width": 2, "samples": 1}, "engine.bots.plan",
                 "PlanBot")):
            with self.subTest(kind=kind):
                HL.CANDIDATE_ARCH = (kind, opts)
                spec = HL.as_spec(w)
                self.assertEqual(spec[0], kind)
                self.assertEqual(spec[1]["culture_rate"], 7.5)
                bot = arena.make_bot(spec, 1)
                self.assertEqual(type(bot).__module__, mod)
                self.assertEqual(type(bot).__name__, cls)
                # the trained vector must actually reach the bot, not just
                # the class -- a searcher on DEFAULT_WEIGHTS trains nothing
                self.assertEqual(bot.weights["culture_rate"], 7.5)

    def test_opts_reach_the_constructor(self):
        HL.CANDIDATE_ARCH = ("quiescent", {"levels": 1})
        bot = arena.make_bot(HL.as_spec(dict(DEFAULT_WEIGHTS)), 1)
        self.assertEqual(bot.LEVELS, 1)
        HL.CANDIDATE_ARCH = ("plan", {"width": 3})
        bot = arena.make_bot(HL.as_spec(dict(DEFAULT_WEIGHTS)), 1)
        self.assertEqual(bot.width, 3)

    def test_external_opponent_specs_are_never_wrapped(self):
        """Wrapping `book` or `var:culture` would change the opponent AND the
        cost of the pool.  Only a plain dict -- a trained vector -- is ours."""
        HL.CANDIDATE_ARCH = ("plan", {"width": 8})
        for spec in ("book", "book2", "greedy", "random", "default",
                     ("variant", "culture", "CultureBot"),
                     ("quiescent", "default", {})):
            with self.subTest(spec=spec):
                self.assertIs(HL.as_spec(spec), spec)

    def test_a_mirror_or_past_champion_is_wrapped(self):
        """`PoolEntry.resolve` hands back the champion DICT for the mirror
        entry, and the past ladder loads old champions as dicts.  Both are the
        same policy family and must play the architecture being trained."""
        HL.CANDIDATE_ARCH = ("quiescent", {})
        from experiments.hillclimb_pool import PoolEntry, MIRROR
        e = PoolEntry("mirror", MIRROR, "mirror", 1.0, "winshare")
        champ = dict(DEFAULT_WEIGHTS)
        self.assertEqual(HL.as_spec(e.resolve(champ, champ))[0], "quiescent")

    def test_wrapping_does_not_mutate_the_trained_vector(self):
        HL.CANDIDATE_ARCH = ("plan", {"width": 8})
        w = dict(DEFAULT_WEIGHTS)
        spec = HL.as_spec(w)
        spec[1]["culture_rate"] = 99.0
        spec[2]["width"] = 1
        self.assertEqual(w["culture_rate"], DEFAULT_WEIGHTS["culture_rate"])
        self.assertEqual(HL.CANDIDATE_ARCH[1]["width"], 8)


if __name__ == "__main__":
    unittest.main()
