"""The derivation of "what must the human type".

The point of these tests is that the field list tracks the *evaluator* rather
than a comment.  The card-row and opponent-hand features are being written
right now; when they land, the harness must start demanding those observables
without anybody editing the harness.
"""

import unittest

from engine import game as G
from engine.bots import weighted as W

from harness import fields as F
from tests.test_harness_mirror import midgame


class Verdicts(unittest.TestCase):
    def test_ordering(self):
        self.assertEqual(F.worse(F.INERT, F.MOVE), F.MOVE)
        self.assertEqual(F.worse(F.RANK, F.SCORE), F.RANK)
        self.assertEqual(F.worse(F.SCORE, F.SCORE), F.SCORE)

    def test_move_and_rank_are_mandatory_score_and_inert_are_not(self):
        probe = F.PROBES_BY_ID["rival.food"]
        self.assertTrue(F.Requirement(probe, F.MOVE, "").mandatory)
        self.assertTrue(F.Requirement(probe, F.RANK, "").mandatory)
        self.assertFalse(F.Requirement(probe, F.SCORE, "").mandatory)
        self.assertFalse(F.Requirement(probe, F.INERT, "").mandatory)

    def test_always_fields_are_mandatory_whatever_the_verdict(self):
        """Some things are needed for legality or for the result record even
        when the evaluator is blind to them."""
        probe = F.PROBES_BY_ID["row.contents"]
        self.assertTrue(probe.always)
        self.assertTrue(F.Requirement(probe, F.INERT, "").mandatory)


class Derivation(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.board = midgame()
        cls.state = cls.board.state
        cls.weights = W.DEFAULT_WEIGHTS

    def test_it_tracks_the_evaluator_not_a_hardcoded_list(self):
        """A bot that values nothing must be told to transcribe nothing.

        With an empty weight vector every position scores 0.0, so no
        perturbation can change an *evaluation* -- and the derivation must say
        so rather than fall back on a list somebody typed.  Legality is a
        separate matter and is allowed to survive: you still cannot take a card
        that is not on the table.
        """
        v = F.probe_position(self.state, self.board.me, {})
        read = {pid for pid, ver in v.items() if ver in F.EVALUATED}
        self.assertEqual(read, set(),
                         "an eval that reads nothing still claimed to read "
                         f"{sorted(read)}")

    def test_a_real_weight_vector_demands_something(self):
        v = F.probe_position(self.state, self.board.me, self.weights)
        read = {pid for pid, ver in v.items() if ver in F.EVALUATED}
        self.assertTrue(read, "no observable at all reached the evaluation")

    def test_rival_strength_is_decision_relevant(self):
        """The clamps (`strength_lead`, `strength_deficit`) make it nonlinear,
        so unlike rival culture it can genuinely move the argmax.

        Whether it does depends on the position (you have to be near a clamp),
        which is exactly why `harness.play` re-derives every round and unions
        rather than deciding once.
        """
        from advisor.advisor import load_bot
        w = load_bot(3).weights
        seen = set()
        for stop in (8, 14, 20):
            b = midgame(stop=stop)
            seen.add(F.probe_position(b.state, b.me, w)["rival.strength"])
        self.assertNotEqual(seen, {F.INERT},
                            "rival strength never reached the evaluation")
        self.assertTrue(seen & set(F.MANDATORY),
                        f"rival strength never changed a decision: {seen}")

    def test_the_expensive_opponent_board_is_never_mandatory_today(self):
        """`rival.techs` is the single most expensive thing a human could type.

        The audit says the evaluator sees rival boards only through three
        derived rates, which the app prints. If this ever becomes mandatory,
        the per-game cost roughly doubles and the operator doc is wrong.
        """
        v = F.probe_position(self.state, self.board.me, self.weights)
        self.assertNotIn(v["rival.techs"], F.MANDATORY)

    def test_inert_means_byte_identical(self):
        """An INERT verdict is a strong claim; check one directly."""
        v = F.probe_position(self.state, self.board.me, self.weights)
        inert = [pid for pid, ver in v.items() if ver == F.INERT]
        self.assertTrue(inert)
        base = F._score_moves(self.state, self.weights)
        for pid in inert:
            with self.subTest(probe=pid):
                trial = self.state.copy()
                F.PROBES_BY_ID[pid].mutate(trial, self.board.me)
                after = F._score_moves(trial, self.weights)
                self.assertEqual(set(base), set(after))
                for mv in base:
                    self.assertEqual(base[mv], after[mv])

    def test_probing_is_side_effect_free(self):
        """Probes mutate copies. If one leaked, the live game would be
        corrupted by the very code meant to protect it."""
        before = G.legal_moves(self.state)
        culture = [p.culture for p in self.state.players]
        row = list(self.state.card_row)
        F.probe_position(self.state, self.board.me, self.weights)
        self.assertEqual(G.legal_moves(self.state), before)
        self.assertEqual([p.culture for p in self.state.players], culture)
        self.assertEqual(list(self.state.card_row), row)

    def test_unapplicable_probe_never_reads_as_inert(self):
        """A probe that blows up must not be mistaken for 'nothing to type'."""
        def boom(state, seat):
            raise RuntimeError("nope")
        probe = F.Probe("x.boom", "", "", "shared", boom)
        v = F.probe_position(self.state, self.board.me, self.weights, [probe])
        self.assertNotEqual(v["x.boom"], F.INERT)

    def test_cost_only_counts_mandatory_and_scales_by_rivals(self):
        reqs = [F.Requirement(F.PROBES_BY_ID["rival.culture_rate"], F.MOVE, ""),
                F.Requirement(F.PROBES_BY_ID["row.order"], F.MOVE, ""),
                F.Requirement(F.PROBES_BY_ID["rival.food"], F.INERT, "")]
        two = F.transcription_cost(reqs, 2)
        one = F.transcription_cost(reqs, 1)
        self.assertGreater(two, one)
        self.assertEqual(two - one,
                         F.PROBES_BY_ID["rival.culture_rate"].seconds)

    def test_requirements_are_serialisable(self):
        v = F.probe_position(self.state, self.board.me, self.weights)
        for r in F.requirements(v):
            d = r.to_json()
            self.assertEqual(set(d) >= {"id", "verdict", "mandatory", "ask"},
                             True)


class MovingTarget(unittest.TestCase):
    """When the evaluator gains an eye, the mandatory set must grow by itself."""

    def test_growing_verdicts_grow_the_mandatory_set(self):
        blind = {p.id: F.INERT for p in F.PROBES}
        sighted = dict(blind, **{"row.order": F.MOVE,
                                 "rival.hand_civil_ids": F.RANK})
        before = {r.probe.id for r in F.requirements(blind) if r.mandatory}
        after = {r.probe.id for r in F.requirements(sighted) if r.mandatory}
        self.assertEqual(after - before, {"row.order", "rival.hand_civil_ids"})

    def test_union_over_positions_takes_the_strongest_verdict(self):
        p = F.PROBES[0]
        merged = {}
        for v in (F.INERT, F.SCORE, F.MOVE, F.INERT):
            merged[p.id] = F.worse(merged.get(p.id, F.INERT), v)
        self.assertEqual(merged[p.id], F.MOVE)


if __name__ == "__main__":
    unittest.main()
