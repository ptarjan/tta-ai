"""The two audit instruments, and the structural facts they measured.

`tools/coverage_census.py` and `tools/feature_variance.py` are only worth
keeping if they stay in step with the engine and the evaluator.  The tests
below pin the one invariant each of them relies on, plus the structural
finding of docs/COVERAGE_AUDIT.md that must not be allowed to reappear
silently: three of `WeightedBot`'s features cannot vary between the
candidates of a decision, so their weights are inert by construction.
"""
from __future__ import annotations

import importlib.util
import os
import random
import sys
import unittest

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, ROOT)

from engine import actions, cards as C, effects, game, journal   # noqa: E402
from engine.bots import weighted as W                            # noqa: E402
from engine.bots.trial import fresh_trial_rng                    # noqa: E402


def _load(name):
    path = os.path.join(ROOT, "tools", name + ".py")
    spec = importlib.util.spec_from_file_location(name, path)
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


fv = _load("feature_variance")
census = _load("coverage_census")


def _walk(players=3, seed=7, steps=120):
    """Yield (state, idx, ctx, candidate move) over a short real game."""
    st = game.new_game(players, seed=seed)
    bot = W.WeightedBot(seed=1)
    rng = random.Random(0)
    for _ in range(steps):
        if st.game_over:
            return
        moves = actions.legal_moves(st)
        idx = st.decider()
        ctx = W.rival_context(st, idx)
        yield st, idx, ctx, moves
        actions.apply(st, bot(st), rng)


class TestFeatureVarianceTool(unittest.TestCase):
    def test_score_from_reproduces_evaluate_exactly(self):
        """The tool re-scores cached feature vectors instead of calling
        `evaluate`, so that it can zero one weight at a time.  If the two ever
        disagree every number the tool prints is wrong."""
        n = 0
        for st, idx, ctx, moves in _walk():
            for mv in moves:
                j = journal.begin(st)
                try:
                    try:
                        actions.apply(st, mv, fresh_trial_rng())
                        f = W.features(st, idx, ctx)
                        got = fv.score_from(f, W.DEFAULT_WEIGHTS,
                                            W.lateness(st),
                                            W.hand_potential(
                                                st, idx, W.DEFAULT_WEIGHTS))
                        want = W.evaluate(st, idx, W.DEFAULT_WEIGHTS, ctx)
                    except Exception:                     # noqa: BLE001
                        continue
                finally:
                    journal.rollback(j)
                self.assertAlmostEqual(got, want, places=9)
                n += 1
        self.assertGreater(n, 500, "walked too few candidates to mean anything")


class TestInertFeatures(unittest.TestCase):
    """docs/COVERAGE_AUDIT.md: `rival_context` is computed once per decision
    on the unmoved board and reused for every candidate (a deliberate ~30x
    saving), so `rival_culture_rate`, `rival_science_rate` and
    `rival_strength` take the SAME value in every candidate of a decision.

    A term that is constant across the candidate set cancels out of the
    argmax exactly.  These three weights therefore cannot change a single
    move, whatever they are set to -- measured at `varying` = 0.000 over
    2347 / 1711 / 2842 decisions at 2p and 3p.

    This test does not call that a bug to be fixed here (making them live
    costs a full opponent recomputation per candidate).  It pins the
    semantics so the next person to read a rival weight knows it is inert.
    """

    INERT = ("rival_culture_rate", "rival_science_rate", "rival_strength")

    def test_rival_rate_features_ignore_the_trial_state(self):
        st = game.new_game(3, seed=3)
        ctx = W.rival_context(st, 0)
        before = W.features(st, 0, ctx)
        rival = st.players[1]
        rival.techs["Philosophy"].workers += 5     # a huge science swing
        rival.strength_extra += 20
        effects.invalidate(st, rival)
        after = W.features(st, 0, ctx)
        for k in self.INERT:
            self.assertEqual(before[k], after[k],
                             f"{k} is no longer inert -- re-run "
                             f"tools/feature_variance.py and update "
                             f"docs/COVERAGE_AUDIT.md")

    def test_they_are_constant_across_a_real_candidate_set(self):
        seen = 0
        for st, idx, ctx, moves in _walk(steps=60):
            if len(moves) < 2:
                continue
            vals = {k: set() for k in self.INERT}
            for mv in moves:
                j = journal.begin(st)
                try:
                    try:
                        actions.apply(st, mv, fresh_trial_rng())
                        f = W.features(st, idx, ctx)
                    except Exception:                     # noqa: BLE001
                        continue
                    for k in self.INERT:
                        vals[k].add(f[k])
                finally:
                    journal.rollback(j)
            for k in self.INERT:
                self.assertLessEqual(len(vals[k]), 1, k)
            seen += 1
        self.assertGreater(seen, 20)


class TestCensusTool(unittest.TestCase):
    def test_labels_split_the_mechanics_that_matter(self):
        st = game.new_game(2, seed=5)
        st.card_row = [None] * actions.ROW_SIZE
        st.card_row[0] = "Irrigation"
        st.card_row[1] = "Masonry"
        self.assertEqual(census.label(st, ("take", 0)), "take:farm")
        self.assertEqual(census.label(st, ("take", 1)), "take:special-tech")
        self.assertEqual(census.label(st, ("build", "Bronze")), "build:mine")
        self.assertEqual(census.label(st, ("build", "Warriors")), "build:unit")
        self.assertEqual(census.label(st, ("develop", "Philosophy")),
                         "develop:urban")
        self.assertEqual(census.label(st, ("prepare_event",
                                           "Vast Territory (I)")),
                         "prepare_event:territory")
        self.assertEqual(census.label(st, ("end_turn",)), "end_turn")

    def test_every_move_kind_the_engine_can_emit_gets_a_label(self):
        st = game.new_game(3, seed=5)
        for kind in actions._HANDLERS:
            probe = {"take": ("take", 0), "build": ("build", "Bronze"),
                     "destroy": ("destroy", "Bronze"),
                     "upgrade": ("upgrade", "Bronze", "Iron"),
                     "develop": ("develop", "Philosophy"),
                     "play_leader": ("play_leader", "Julius Caesar"),
                     "revolution": ("revolution", "Monarchy"),
                     "play_action": ("play_action", "Rich Land (A)"),
                     "play_tactic": ("play_tactic", "Legion"),
                     "copy_tactic": ("copy_tactic", "Legion"),
                     "prepare_event": ("prepare_event", "Vast Territory (I)"),
                     "aggression": ("aggression", "Aggression: Plunder (I)", 1),
                     "war": ("war", "War over Culture", 1),
                     "offer_pact": ("offer_pact", "Peace Treaty", 1, ""),
                     "cancel_pact": ("cancel_pact", 1),
                     "churchill": ("churchill", "culture"),
                     "wonder_step": ("wonder_step", 1),
                     }.get(kind, (kind,))
            st.card_row = ["Irrigation"] + [None] * (actions.ROW_SIZE - 1)
            tag = census.label(st, probe)
            self.assertTrue(tag and isinstance(tag, str), kind)


if __name__ == "__main__":
    unittest.main()
