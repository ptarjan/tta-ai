"""Round-trip and parsing tests for the terse board format."""
import os
import random
import sys
import unittest

sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", ".."))

from advisor import state_io as S               # noqa: E402
from engine import game as G                    # noqa: E402
from engine.bots import GreedyBot               # noqa: E402


class TestResolve(unittest.TestCase):
    def test_exact(self):
        self.assertEqual(S.resolve_card("Bronze"), "Bronze")

    def test_case_insensitive(self):
        self.assertEqual(S.resolve_card("bronze"), "Bronze")

    def test_prefix(self):
        self.assertEqual(S.resolve_card("philos", "tech"), "Philosophy")

    def test_initials(self):
        self.assertEqual(S.resolve_card("hg", "wonder"), "Hanging Gardens")

    def test_subsequence(self):
        self.assertEqual(S.resolve_card("hanggard", "wonder"),
                         "Hanging Gardens")

    def test_pool_narrows(self):
        # "a" alone is hopeless, but leaders are a small pool
        self.assertEqual(S.resolve_card("julius", "leader"), "Julius Caesar")

    def test_ambiguous_lists_options(self):
        with self.assertRaises(S.AmbiguousCard) as cm:
            S.resolve_card("a", "leader")
        self.assertTrue(cm.exception.options)
        self.assertIn("ambiguous", str(cm.exception))

    def test_unknown(self):
        with self.assertRaises(S.UnknownCard):
            S.resolve_card("zzzzzz")

    def test_errors_are_patch_errors(self):
        # the interactive loop only catches PatchError
        self.assertTrue(issubclass(S.UnknownCard, S.PatchError))
        self.assertTrue(issubclass(S.AmbiguousCard, S.PatchError))


class TestRoundTrip(unittest.TestCase):
    def _boards(self):
        for n in (2, 3, 4):
            yield S.new_board(n, me=0, seed=n)

    def test_fresh_game_round_trips(self):
        for b in self._boards():
            text = S.dumps(b)
            again = S.dumps(S.loads(text))
            self.assertEqual(text, again)

    def test_midgame_round_trips(self):
        """Play a real game a while, then round-trip the text form."""
        for seed in (1, 7):
            st = G.new_game(3, seed)
            bots = [GreedyBot(random.Random(i)) for i in range(3)]
            rng = random.Random(seed)
            for _ in range(220):
                if st.game_over:
                    break
                G.apply(st, bots[st.decider()](st), rng)
            b = S.Board(st, me=0)
            b.set_hidden(1, "civil", 2)
            b.unknown.add("p2.culture")
            text = S.dumps(b)
            b2 = S.loads(text)
            self.assertEqual(text, S.dumps(b2))
            # semantic spot checks
            self.assertEqual(b2.state.round, st.round)
            self.assertEqual(b2.state.card_row, st.card_row)
            self.assertEqual(b2.state.players[0].culture, st.players[0].culture)
            self.assertEqual(b2.state.players[0].techs.keys(),
                             st.players[0].techs.keys())
            self.assertEqual(b2.hidden_count(1, "civil"), 2)
            # the counts are STATE now, so the evaluator can see them
            self.assertEqual(b2.state.players[1].hand_size("civil"),
                             len(b2.state.players[1].hand_civil) + 2)
            self.assertIn("p2.culture", b2.unknown)

    def test_hand_and_wonders_survive(self):
        b = S.new_board(2, seed=3)
        p = b.state.players[0]
        p.hand_civil = ["Bronze", "Irrigation"]
        p.leader = "Julius Caesar"
        p.wonder = None
        S.patch(b, "p0 wonder Pyramids 2")
        S.patch(b, "p0 built+ Colossus")
        b2 = S.loads(S.dumps(b))
        q = b2.state.players[0]
        self.assertEqual(sorted(q.hand_civil), ["Bronze", "Irrigation"])
        self.assertEqual(q.leader, "Julius Caesar")
        self.assertEqual(q.wonder.name, "Pyramids")
        self.assertEqual(q.wonder.steps_built, 2)
        self.assertEqual(q.completed_wonders, ["Colossus"])


class TestPatch(unittest.TestCase):
    def setUp(self):
        self.b = S.new_board(3, me=0, seed=11)

    def test_scalars(self):
        S.patch(self.b, "p1 c=34 s=9 f=4 r=6")
        p = self.b.state.players[1]
        self.assertEqual((p.culture, p.science, p.food, p.resources),
                         (34, 9, 4, 6))

    def test_forced_strength(self):
        S.patch(self.b, "p1 str=11")
        from engine import effects
        self.assertEqual(effects.compute(self.b.state,
                                         self.b.state.players[1]).strength, 11)

    def test_unknown_value_records_and_keeps(self):
        before = self.b.state.players[1].culture
        S.patch(self.b, "p1 c=?")
        self.assertEqual(self.b.state.players[1].culture, before)
        self.assertIn("p1.c", self.b.unknown)

    def test_tech_add_remove(self):
        S.patch(self.b, "p1 tech+ irrigation:2")
        self.assertEqual(self.b.state.players[1].techs["Irrigation"].workers, 2)
        S.patch(self.b, "p1 tech- warriors")
        self.assertNotIn("Warriors", self.b.state.players[1].techs)

    def test_take_removes_row_card_and_counts_hand(self):
        name = self.b.state.card_row[4]
        S.patch(self.b, "take p2 4")
        self.assertIsNone(self.b.state.card_row[4])
        self.assertEqual(self.b.hand_size(2, "civil"), 1)
        self.assertIsNotNone(name)

    def test_deal_sweeps_and_appends(self):
        old = list(self.b.state.card_row)
        S.patch(self.b, "deal bronze, irrigation")
        row = self.b.state.card_row
        self.assertEqual(row[0], old[2])          # 3p sweep = 2
        self.assertIn("Bronze", row)
        self.assertIn("Irrigation", row)
        self.assertEqual(len(row), 13)

    def test_row_full_retype(self):
        S.patch(self.b, "row bronze, ., philosophy")
        self.assertEqual(self.b.state.card_row[:3],
                         ["Bronze", None, "Philosophy"])
        self.assertEqual(len(self.b.state.card_row), 13)

    def test_government_and_leader(self):
        S.patch(self.b, "p1 gov=monarchy")
        self.assertEqual(self.b.state.players[1].government, "Monarchy")
        S.patch(self.b, "p1 leader caesar")
        self.assertEqual(self.b.state.players[1].leader, "Julius Caesar")
        S.patch(self.b, "p1 leader -")
        self.assertIsNone(self.b.state.players[1].leader)

    def test_hand_sizes(self):
        S.patch(self.b, "p1 hc=4 hm=2")
        self.assertEqual(self.b.hand_size(1, "civil"), 4)
        self.assertEqual(self.b.hand_size(1, "military"), 2)

    def test_bad_input_raises_patch_error_not_crash(self):
        for bad in ["zzz", "p1 c=x", "p9 c=1", "take p1", "take p1 99",
                    "p1 tech+ notacard", "p1 nosuchkey=3", "age Z",
                    "p1 gov=zzzz", "row"]:
            with self.assertRaises(S.PatchError, msg=bad):
                S.patch(self.b, bad)

    def test_blank_and_comment_are_noops(self):
        self.assertEqual(S.patch(self.b, ""), "")
        self.assertEqual(S.patch(self.b, "  # note"), "")

    def test_patch_all_collects_errors(self):
        msgs, errs = S.patch_all(self.b, "p1 c=5\nnonsense\np2 s=3")
        self.assertEqual(len(msgs), 2)
        self.assertEqual(len(errs), 1)

    def test_render_does_not_crash(self):
        txt = S.render(self.b)
        self.assertIn("card row", txt)
        self.assertIn("p0", txt)

    def test_engine_still_runs_after_patching(self):
        """A patched mirror must remain a legal engine state."""
        S.patch(self.b, "p1 c=34 str=7")
        S.patch(self.b, "p1 tech+ irrigation:2")
        S.patch(self.b, "deal bronze, irrigation")
        moves = G.legal_moves(self.b.state)
        self.assertTrue(moves)
        G.apply(self.b.state, moves[-1], random.Random(0))


if __name__ == "__main__":
    unittest.main()
