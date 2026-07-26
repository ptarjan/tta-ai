"""End-to-end tests: a scripted advised session, driven through the same
console the human uses."""
import os
import sys
import unittest

sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", ".."))

from advisor import advisor as AD               # noqa: E402
from advisor import state_io as S               # noqa: E402
from engine import game as G                    # noqa: E402


class Script:
    """A canned keyboard: answers from the list, then always 'quit'."""

    def __init__(self, lines, default=""):
        self.lines = list(lines)
        self.default = default
        self.asked = []

    def __call__(self, prompt=""):
        self.asked.append(prompt)
        if self.lines:
            return self.lines.pop(0)
        if len(self.asked) > 4000:
            return "quit"
        return self.default


def run_console(lines, players=2, seat=0, seed=4, default="", board=None):
    board = board or S.new_board(players, me=seat, seed=seed)
    adv = AD.Advisor(board, AD.load_bot(players, seed=seed), seed=seed)
    adv.dealt_slots = []
    out = []
    con = AD.Console(adv, inp=Script(lines, default), out=lambda *a: out.append(
        " ".join(str(x) for x in a)))
    con.run()
    return adv, "\n".join(out)


class TestBotAndRanking(unittest.TestCase):
    def test_loads_champion_when_present(self):
        bot = AD.load_bot(3)
        self.assertTrue(bot.source)
        path = AD.CHAMPION.format(n=3)
        if os.path.exists(path):
            self.assertIn("champion_3p", bot.source)

    def test_falls_back_to_defaults(self):
        bot = AD.load_bot(3, path="/nonexistent/champion.json")
        self.assertIn("default", bot.source)
        self.assertEqual(bot.weights["culture"],
                         AD.W.DEFAULT_WEIGHTS["culture"])

    def test_rank_returns_several_scored_candidates(self):
        b = S.new_board(3, me=0, seed=5)
        cands = AD.rank_moves(b, AD.load_bot(3), top=3)
        self.assertEqual(len(cands), 3)
        for c in cands:
            self.assertTrue(c.text)
            self.assertTrue(c.reason)
            self.assertIsInstance(c.score, float)
        # scores are gaps from the best move, so the first is 0 and the
        # rest are no better
        self.assertAlmostEqual(cands[0].score, 0.0)
        self.assertLessEqual(cands[1].score, 1e-9)
        self.assertLessEqual(cands[2].score, cands[1].score + 1e-9)

    def test_top_candidate_matches_the_bot(self):
        b = S.new_board(2, me=0, seed=9)
        bot = AD.load_bot(2)
        best = AD.rank_moves(b, bot, top=1)[0].move
        self.assertEqual(tuple(best), tuple(bot(b.state)))

    def test_describe_every_legal_move_midgame(self):
        """Descriptions must never blow up, whatever the move vocabulary."""
        import random
        st = G.new_game(3, 2)
        rng = random.Random(2)
        bot = AD.load_bot(3)
        seen = set()
        for _ in range(400):
            if st.game_over:
                break
            for mv in G.legal_moves(st):
                txt = AD.describe_move(st, mv)
                self.assertTrue(txt)
                seen.add(mv[0])
            G.apply(st, bot(st), rng)
        self.assertIn("take", seen)
        self.assertIn("end_turn", seen)


class TestParseMove(unittest.TestCase):
    def setUp(self):
        self.b = S.new_board(3, me=0, seed=5)

    def test_verb_and_index(self):
        mv = AD.parse_move(self.b.state, "take 2")
        self.assertEqual(mv, ("take", 2))

    def test_abbreviated_verb(self):
        self.assertEqual(AD.parse_move(self.b.state, "t 0"), ("take", 0))
        self.assertEqual(AD.parse_move(self.b.state, "end"), ("end_turn",))

    def test_card_name_argument(self):
        name = self.b.state.card_row[3]
        mv = AD.parse_move(self.b.state, f"take {name[:4]}")
        self.assertEqual(mv[0], "take")

    def test_illegal_verb_is_a_patch_error(self):
        for bad in ["", "frobnicate", "take 99", "build bronze"]:
            with self.assertRaises(S.PatchError, msg=bad):
                AD.parse_move(self.b.state, bad)

    def test_ambiguous_lists_options(self):
        with self.assertRaises(S.PatchError) as cm:
            AD.parse_move(self.b.state, "take")
        self.assertIn("which one?", str(cm.exception))


class TestScriptedSession(unittest.TestCase):
    def test_accepting_every_recommendation_advances_the_game(self):
        """Press Enter for everything: the advisor plays your side, the
        opponents' turns are skipped over, and the game moves on."""
        adv, out = run_console([], players=2, seat=0, seed=4, default="")
        self.assertGreaterEqual(adv.state.round, 3)
        self.assertIn("your turn", out)
        self.assertIn("TAKE", out)
        self.assertTrue(adv.log)

    def test_reporting_opponent_state_between_turns(self):
        board = S.new_board(2, me=0, seed=6)
        adv = AD.Advisor(board, AD.load_bot(2, seed=6), seed=6)
        # my whole first turn
        while adv.my_turn():
            ok, _ = adv.play(adv.recommend(1)[0].move)
            self.assertTrue(ok)
        # opponent's turn: they took a card and their culture went up
        slot = next(i for i, n in enumerate(adv.state.card_row) if n)
        adv.patch(f"take p1 {slot}")
        adv.patch("p1 c=7 s=3")
        self.assertIsNone(adv.state.card_row[slot])
        self.assertEqual(adv.state.players[1].culture, 7)
        adv.skip_opponent_turn()
        self.assertEqual(adv.board.hand_size(1, "civil"), 1)
        self.assertEqual(adv.state.round, 2)
        self.assertTrue(adv.my_turn())

    def test_new_cards_are_taken_from_the_human(self):
        board = S.new_board(3, me=0, seed=8)
        adv = AD.Advisor(board, AD.load_bot(3, seed=8), seed=8)
        # play on until the row is actually replenished (round 2 onwards)
        for _ in range(12):
            while adv.my_turn():
                adv.play(adv.recommend(1)[0].move)
            if adv.dealt_slots:
                break
            adv.skip_opponent_turn()
            if adv.dealt_slots:
                break
        slots = adv.dealt_slots
        self.assertTrue(slots, "the row should have been replenished")
        got = adv.set_dealt(["bronze"])
        self.assertEqual(got, ["Bronze"])
        self.assertEqual(adv.state.card_row[slots[0]], "Bronze")

    def test_garbage_input_never_crashes(self):
        junk = ["!!!", "take 99", "p9 c=1", "zzz", "?", "help", "board",
                "state", "more", "undo", "set p1 c=notanumber",
                "set nonsense", ""]
        adv, out = run_console(junk * 6, players=3, seat=1, seed=3,
                               default="")
        self.assertIn("!", out)
        self.assertFalse(adv.state.game_over and adv.state.round < 2)

    def test_unknown_values_are_tolerated(self):
        board = S.new_board(2, me=0, seed=2)
        adv = AD.Advisor(board, AD.load_bot(2, seed=2), seed=2)
        before = adv.state.players[1].culture
        adv.patch("p1 c=? s=? str=?")
        self.assertEqual(adv.state.players[1].culture, before)
        self.assertEqual(len(adv.board.unknown), 3)

    def test_quit_at_the_new_cards_prompt_stops(self):
        """Regression: 'quit' at the 'which cards were dealt' prompt used to
        loop forever re-asking."""
        board = S.new_board(2, me=0, seed=7)
        adv = AD.Advisor(board, AD.load_bot(2, seed=7), seed=7)
        adv.dealt_slots = [0, 1]
        out = []
        con = AD.Console(adv, inp=Script(["quit"], default="quit"),
                         out=lambda *a: out.append(" ".join(map(str, a))))
        con.run()
        self.assertIn("bye", "\n".join(out))

    def test_new_cards_prompt_accepts_update_lines(self):
        board = S.new_board(2, me=0, seed=7)
        adv = AD.Advisor(board, AD.load_bot(2, seed=7), seed=7)
        adv.dealt_slots = [0]
        out = []
        con = AD.Console(adv, inp=Script(["p1 c=13", "bronze"]),
                         out=lambda *a: out.append(" ".join(map(str, a))))
        con.check_dealt()
        self.assertEqual(adv.state.players[1].culture, 13)
        self.assertEqual(adv.state.card_row[0], "Bronze")

    def test_only_freshly_dealt_slots_are_asked_about(self):
        """The row slides left when replenished; only the genuinely new
        cards should be asked for."""
        board = S.new_board(3, me=0, seed=21)
        adv = AD.Advisor(board, AD.load_bot(3, seed=21), seed=21)
        for _ in range(20):
            while adv.my_turn():
                adv.play(adv.recommend(1)[0].move)
            if adv.state.round > 1:
                break
            adv.skip_opponent_turn()
        adv.skip_opponent_turn()
        self.assertLessEqual(len(adv.dealt_slots), 4)

    def test_update_lines_work_at_the_move_prompt(self):
        board = S.new_board(3, me=0, seed=5)
        adv = AD.Advisor(board, AD.load_bot(3, seed=5), seed=5)
        con = AD.Console(adv, inp=Script([]), out=lambda *a: None)
        con.handle_move_input("p1 c=44", adv.recommend(1))
        self.assertEqual(adv.state.players[1].culture, 44)
        # ... while 'take 4' at the same prompt is still a move
        con._snapshot = S.dumps(adv.board)
        con.handle_move_input("take 4", adv.recommend(1))
        self.assertEqual(len(adv.state.players[0].hand_civil), 1)

    def test_take_p1_vs_take_4_are_told_apart(self):
        self.assertTrue(AD._looks_like_patch("take p1 3"))
        self.assertFalse(AD._looks_like_patch("take 3"))
        self.assertTrue(AD._looks_like_patch("p2 c=9"))
        self.assertFalse(AD._looks_like_patch("build bronze"))
        self.assertFalse(AD._looks_like_patch("pop"))

    def test_quit_stops_cleanly(self):
        adv, out = run_console(["quit"], players=2, seed=1)
        self.assertFalse(adv.state.game_over)

    def test_undo_restores_the_start_of_turn(self):
        board = S.new_board(2, me=0, seed=12)
        adv = AD.Advisor(board, AD.load_bot(2, seed=12), seed=12)
        con = AD.Console(adv, inp=Script([]), out=lambda *a: None)
        con._snapshot = S.dumps(adv.board)
        row_before = list(adv.state.card_row)
        con.handle_move_input("take 0", adv.recommend(1))
        self.assertNotEqual(adv.state.card_row, row_before)
        con.handle_move_input("undo", adv.recommend(1))
        self.assertEqual(adv.state.card_row, row_before)

    def test_snapshot_of_a_live_session_reloads(self):
        adv, _ = run_console(["", "", "", "quit"], players=3, seed=15)
        text = S.dumps(adv.board)
        again = S.loads(text)
        self.assertEqual(text, S.dumps(again))
        adv2 = AD.Advisor(again, AD.load_bot(3))
        self.assertTrue(adv2.recommend(2))


if __name__ == "__main__":
    unittest.main()
