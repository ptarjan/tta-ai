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


def _read_the_panel(st, me):
    """Exactly what the operator types, for every rival, this round.

    Returns {idx: ({forced key: value}, {name channel: names})} -- the two
    kinds of channel the harness really has: numbers off the player panel, and
    card NAMES transcribed as they are played (wonders completed, a wonder
    started, a colony taken).  A name is a separate channel because it carries
    printed effects a count cannot.
    """
    out = {}
    for q in st.players:
        if q.idx == me:
            continue
        s = effects.compute(st, q)
        out[q.idx] = ({"c": q.culture, "cr": s.culture, "sr": s.science,
                       "str": s.strength, "ca": q.civil_actions,
                       "hc": q.hand_size("civil"),
                       "s": q.science, "f": q.food, "r": q.resources,
                       "fw": q.workers_free, "y": q.yellow_bank,
                       "ma": q.military_actions},
                      {"built+": list(q.completed_wonders),
                       "colony+": list(q.colonies),
                       "wonder": ([f"{q.wonder.name} {q.wonder.steps_built}"]
                                  if q.wonder else [])})
    return out


def _wreck(st, me):
    """Every rival board as a completely untranscribed opponent looks: no
    workers, no score, no stocks, no wonders, no colonies, no government, no
    cards, no actions.

    Every field the reconstruction claims to restore must be destroyed HERE.
    A field left standing makes the rebuild test pass for free -- which is how
    the eight rival facts added by the information audit could have been
    waved through without the operator ever being asked for them.
    """
    for q in st.players:
        if q.idx == me:
            continue
        for t in q.techs.values():
            t.workers = 0
        q.culture = 0
        q.completed_wonders = []
        q.colonies = []
        q.wonder = None
        q.government = "Despotism"
        q.hand_civil = []
        q.hidden_civil = 0
        q.civil_actions = 0
        q.military_actions = 0
        q.science = 0
        q.food = 0
        q.resources = 0
        q.workers_free = 0
        q.yellow_bank = 0
        effects.invalidate(st, q)


class ForcedRivalsAreExact(unittest.TestCase):
    """The claim the whole cost estimate rests on.

    We never mirror an opponent's board -- the human reads a few numbers off
    the app's player panel, names the wonders they complete, and `state_io`
    back-solves.  That is only sound if what we ask for pins down every
    rival-derived feature.  If the feature work adds a rival term the operator
    is not asked for, this test fails, and the shortcut has to be revisited
    BEFORE anyone spends ten evenings on it.

    It has fired for real once (the GAP-3 rival features), which is the whole
    reason it exists.  The fix is never to append a name to RIVAL_FEATURE_KEYS
    -- that silences the alarm and leaves the harness feeding the bot a zero
    for the whole game, so the advice logged against the app AI comes from a
    policy nobody trained.
    """

    def test_rival_feature_keys_are_the_ones_we_ask_for(self):
        board = midgame()
        st = board.state
        feats = W.features(st, board.me)
        rival_keys = {k for k in feats if k.startswith("rival_")}
        self.assertEqual(
            rival_keys, set(M.RIVAL_FEATURE_KEYS),
            "the evaluator grew or lost a rival feature. harness.mirror asks "
            f"for {'/'.join(M.RIVAL_ASK_KEYS)}; check that those still "
            "determine every rival_* feature -- and if they do not, grow the "
            "ASK, do not just grow RIVAL_FEATURE_KEYS.")

    def test_what_we_ask_for_reconstructs_the_rival_features(self):
        board = midgame()
        st = board.state
        me = board.me
        # a rival with a completed wonder, so the wonder arm of this test is
        # exercised rather than decorative (self-play has not built one by
        # round 8).  This is exactly the state the operator's `built+` leaves.
        S.patch(board, f"p{(me + 1) % st.num_players} built+ Colossus")
        before = W.features(st, me)
        for k in ("rival_free_ca", "rival_hand_civil", "rival_wonders"):
            self.assertGreater(before[k], 0,
                               f"{k} is 0 here, so restoring it proves nothing")
        panel = _read_the_panel(st, me)
        _wreck(st, me)

        for idx, (vals, names) in panel.items():
            # ...in the order the table happens: cards are named as they are
            # played, the panel numbers are forced at the round check.
            for verb, cards in names.items():
                if cards:
                    S.patch(board, f"p{idx} {verb} " + ", ".join(cards))
            for key in M.RIVAL_FORCE_KEYS:
                S.patch(board, f"p{idx} {key}={vals[key]}")

        after = W.features(st, me)
        for k in M.RIVAL_FEATURE_KEYS:
            self.assertAlmostEqual(
                before[k], after[k], places=6,
                msg=f"{k} could not be restored from what the operator types "
                    f"({'/'.join(M.RIVAL_ASK_KEYS)} + wonder names)")

    def test_every_asked_key_is_applicable_or_checkable(self):
        """No key may be prompted for that goes nowhere.

        That is precisely the bug this branch fixed: `hc=` was asked for every
        round, parsed, logged -- and written into an advisor-side dict that
        `features()` never reads.
        """
        self.assertEqual(set(M.RIVAL_ASK_KEYS),
                         set(M.RIVAL_FORCE_KEYS) | set(M.RIVAL_CHECK_BY_KEY))
        board = midgame()
        for key in M.RIVAL_FORCE_KEYS:
            with self.subTest(key=key):
                S.patch(board, f"p1 {key}=3")        # must not raise

    def test_each_forced_key_actually_moves_its_feature(self):
        """A forced key that changes no feature is unpaid data entry; a key
        that is asked for but does not reach the feature is worse."""
        moves = {"c": "rival_culture", "cr": "rival_culture_rate",
                 "sr": "rival_science_rate", "str": "rival_strength",
                 "ca": "rival_free_ca", "hc": "rival_hand_civil",
                 "s": "rival_science_stock", "f": "rival_food_stock",
                 "r": "rival_resource_stock", "fw": "rival_free_workers",
                 "y": "rival_yellow_bank", "ma": "rival_mil_actions"}
        self.assertEqual(sorted(moves), sorted(M.RIVAL_FORCE_KEYS),
                         "a forced key with no entry here is unproven data "
                         "entry -- add the feature it is supposed to move")
        for key, feat in moves.items():
            with self.subTest(key=key):
                board = midgame()
                st = board.state
                base = W.features(st, board.me)[feat]
                for q in st.players:
                    if q.idx != board.me:
                        S.patch(board, f"p{q.idx} {key}={int(base) + 9}")
                self.assertAlmostEqual(W.features(st, board.me)[feat],
                                       base + 9, places=6)

    def test_a_missed_wonder_is_caught_by_the_count(self):
        """Wonders come in by NAME, so the mirror can be wrong about them --
        and therefore the count on the panel is a real check, the only hard
        one on the rival side."""
        board = midgame()
        st = board.state
        idx = next(q.idx for q in st.players if q.idx != board.me)
        mirror_says = len(st.players[idx].completed_wonders)
        self.assertEqual(M.check_rivals(board, {idx: {"w": mirror_says}}), [])
        ds = M.check_rivals(board, {idx: {"w": mirror_says + 1}})
        self.assertEqual([(d.key, d.severity) for d in ds], [("w", M.FAIL)])
        # and typing the name that was missed clears it
        S.patch(board, f"p{idx} built+ Colossus")
        self.assertEqual(
            M.check_rivals(board, {idx: {"w": mirror_says + 1}}), [])

    def test_every_asked_key_has_a_probe_watching_it(self):
        """The ask is static, the derivation is dynamic.  Every asked field
        must have a probe, or it can stop mattering (or start) unseen."""
        from harness import fields as F
        for key, pid in M.RIVAL_PROBE_IDS.items():
            with self.subTest(key=key):
                self.assertIn(pid, F.PROBES_BY_ID)
        self.assertEqual(set(M.RIVAL_PROBE_IDS), set(M.RIVAL_ASK_KEYS))

    def test_an_unasked_rival_key_is_not_silently_dropped(self):
        _, vals, errs = M.parse_rival_line("p1 c=10 zz=3")
        self.assertTrue(errs)
        self.assertEqual(vals, {"c": 10})


if __name__ == "__main__":
    unittest.main()
