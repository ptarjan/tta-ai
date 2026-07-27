"""State mirroring and desync detection.

The property under test is not "the parser works" -- it is that a mirror which
disagrees with the app CANNOT get through a round silently.  A logged game that
is quietly wrong is worse than no game at all, because it is indistinguishable
from a real one afterwards.
"""

import random
import unittest

import advisor.state_io as S
from engine import effects, game as G
from engine.bots import weighted as W

from harness import mirror as M


def midgame(num_players=3, seat=0, seed=5, stop=8):
    """A real position, reached by self-play, wrapped in an advisor Board."""
    bot = W.WeightedBot(seed=seed)
    st = G.new_game(num_players, seed)
    rng = random.Random(seed)
    guard = 0
    while not st.game_over and guard < 6000:
        guard += 1
        if st.round >= stop and st.decider() == seat and st.phase == "actions":
            break
        moves = G.legal_moves(st)
        if not moves:
            break
        G.apply(st, bot.choose(st, moves, rng), rng)
    return S.Board(st, me=seat)


class SelfChecks(unittest.TestCase):
    def setUp(self):
        self.board = midgame()

    def test_snapshot_matches_itself(self):
        snap = M.self_snapshot(self.board)
        self.assertEqual(M.check_self(self.board, snap), [])

    def test_every_self_field_is_a_real_tripwire(self):
        """Perturb each checked quantity; the check must catch each one.

        A check that cannot fail is decoration.
        """
        snap = M.self_snapshot(self.board)
        for key, val in snap.items():
            with self.subTest(key=key):
                bad = dict(snap)
                bad[key] = val + 7
                ds = M.check_self(self.board, bad)
                self.assertTrue(any(d.key == key for d in ds),
                                f"{key} drifted by 7 and nothing noticed")
                self.assertTrue(all(d.severity == M.FAIL for d in ds))

    def test_board_checks_catch_wrong_round_and_age(self):
        snap = M.board_snapshot(self.board)
        self.assertEqual(M.check_board(self.board, snap), [])
        for key, bad in (("round", snap["round"] + 1),
                         ("age", "III" if snap["age"] != "III" else "I"),
                         ("row", snap["row"] - 1)):
            with self.subTest(key=key):
                d = M.check_board(self.board, {**snap, key: bad})
                self.assertEqual([x.key for x in d], [key])

    def test_a_real_drift_is_caught(self):
        """Simulate the classic failure: an event we forgot to enter.

        The app gives us 6 culture, the mirror does not hear about it, and the
        next round's check must fail.
        """
        snap = M.self_snapshot(self.board)
        as_app_sees_it = {**snap, "c": snap["c"] + 6}
        res = M.round_check(self.board, as_app_sees_it)
        self.assertTrue(res.failed)
        self.assertIn("c", [d.key for d in res.discrepancies])

    def test_missing_spine_blocks_the_round(self):
        self.assertEqual(M.missing_spine({k: 1 for k in M.SPINE}), [])
        self.assertEqual(M.missing_spine({"c": 1}),
                         [k for k in M.SPINE if k != "c"])

    def test_absent_keys_are_not_silently_passed(self):
        """Not supplying a field must not read as agreement."""
        res = M.round_check(self.board, {"c": M.self_snapshot(self.board)["c"]})
        self.assertFalse(res.failed)
        self.assertTrue(M.missing_spine(res.reported))


class Parsing(unittest.TestCase):
    def test_positional_spine(self):
        vals, errs = M.parse_line("41/12/9/3/5")
        self.assertEqual(errs, [])
        self.assertEqual(vals, {"c": 41, "s": 12, "str": 9, "f": 3, "r": 5})

    def test_keyed(self):
        vals, errs = M.parse_line("c=41 str=9 age=ii row=13")
        self.assertEqual(errs, [])
        self.assertEqual(vals, {"c": 41, "str": 9, "age": "II", "row": 13})

    def test_partial_spine_with_gaps(self):
        vals, errs = M.parse_line("41//9")
        self.assertEqual(errs, [])
        self.assertEqual(vals, {"c": 41, "str": 9})

    def test_unknown_key_is_an_error_not_a_shrug(self):
        vals, errs = M.parse_line("zz=3")
        self.assertTrue(errs)
        self.assertEqual(vals, {})

    def test_non_numeric_is_an_error(self):
        _, errs = M.parse_line("c=lots")
        self.assertTrue(errs)

    def test_too_many_spine_values(self):
        _, errs = M.parse_line("1/2/3/4/5/6/7")
        self.assertTrue(errs)

    def test_rival_line(self):
        idx, vals, errs = M.parse_rival_line("p1 22/4/3/6")
        self.assertEqual((idx, errs), (1, []))
        self.assertEqual(vals, {"c": 22, "cr": 4, "sr": 3, "str": 6})

    def test_rival_line_keyed(self):
        idx, vals, errs = M.parse_rival_line("p2 c=30 str=0")
        self.assertEqual((idx, errs), (2, []))
        self.assertEqual(vals, {"c": 30, "str": 0})

    def test_ca_slash_total_form(self):
        vals, _ = M.parse_line("ca=3/4")
        self.assertEqual(vals["ca"], 3)


class RivalConsistency(unittest.TestCase):
    def test_plausible_growth_passes(self):
        h = M.RivalHistory()
        self.assertEqual(h.check(1, 5, 40, 6), [])
        self.assertEqual(h.check(1, 6, 46, 6), [])

    def test_transposed_digits_are_flagged(self):
        h = M.RivalHistory()
        h.check(1, 5, 40, 6)
        ds = h.check(1, 6, 4, 6)             # typed "4" for "46"
        self.assertEqual([d.severity for d in ds], [M.WARN])

    def test_a_warning_is_never_a_hard_failure(self):
        h = M.RivalHistory()
        h.check(1, 5, 40, 6)
        board = midgame()
        res = M.round_check(board, M.self_snapshot(board), h,
                            {1: {"c": 4, "cr": 6}})
        self.assertTrue(res.warned)
        self.assertFalse(res.failed)


class ForcedRivalsAreExact(unittest.TestCase):
    """The claim the whole cost estimate rests on.

    We never mirror an opponent's board -- the human types four numbers off the
    app's player panel and `state_io` back-solves.  That is only sound if those
    four numbers pin down every rival-derived feature.  If the card-row /
    opponent-hand feature work adds a rival term that four scalars cannot
    reconstruct, this test fails, and the shortcut has to be revisited BEFORE
    anyone spends ten evenings on it.
    """

    def test_rival_feature_keys_are_the_ones_we_ask_for(self):
        board = midgame()
        st = board.state
        feats = W.features(st, board.me)
        rival_keys = {k for k in feats if k.startswith("rival_")}
        self.assertEqual(
            rival_keys, set(M.RIVAL_FEATURE_KEYS),
            "the evaluator grew or lost a rival feature. harness.mirror asks "
            "for c/cr/sr/str only; check that those four still determine every "
            "rival_* feature, and update RIVAL_FEATURE_KEYS.")

    def test_four_numbers_reconstruct_the_rival_features(self):
        board = midgame()
        st = board.state
        me = board.me
        before = W.features(st, me)

        wanted = {}
        for q in st.players:
            if q.idx == me:
                continue
            s = effects.compute(st, q)
            wanted[q.idx] = {"c": q.culture, "cr": s.culture,
                             "sr": s.science, "str": s.strength}

        # wreck every rival board the way a completely untranscribed opponent
        # would look, then restore ONLY the four reported numbers
        for q in st.players:
            if q.idx == me:
                continue
            for t in q.techs.values():
                t.workers = 0
            q.culture = 0
            q.completed_wonders = []
            q.government = "Despotism"
            q.hand_civil = []
            effects.invalidate(st, q)

        for idx, vals in wanted.items():
            for key, val in vals.items():
                S.patch(board, f"p{idx} {key}={val}")

        after = W.features(st, me)
        for k in M.RIVAL_FEATURE_KEYS:
            self.assertAlmostEqual(
                before[k], after[k], places=6,
                msg=f"{k} could not be restored from c/cr/sr/str alone")


if __name__ == "__main__":
    unittest.main()
