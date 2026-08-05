"""Tests for `tools/dump_fixtures.py`, the Python side of the Rust port's
offline differential-testing oracle (`rust/DESIGN.md`).

Determinism is the entire point of a fixture: a Rust replay is only a
meaningful check against "state diverges at ply 41" if regenerating the
fixture a second time would have produced the same ply 41. These tests are
the self-check the harness's own design calls for.
"""
from __future__ import annotations

import json
import os
import sys
import unittest

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

from tools import dump_fixtures as DF                            # noqa: E402


class DeterminismSelfCheck(unittest.TestCase):
    """`--verify`'s underlying function, run directly (no subprocess)."""

    def test_two_p_greedy(self):
        n = DF.verify_determinism(2, seed=1, max_plies=150)
        self.assertGreater(n, 0)

    def test_three_p_greedy(self):
        n = DF.verify_determinism(3, seed=7, max_plies=150)
        self.assertGreater(n, 0)

    def test_random_bot_is_also_deterministic(self):
        # The random bot draws from its OWN seeded rng (`seed * 131 + i`),
        # separate from the game's rng stream; this is the check that both
        # streams replay identically together.
        n = DF.verify_determinism(4, seed=133, max_plies=150, bot_name="random")
        self.assertGreater(n, 0)


class FixtureFileIsByteIdentical(unittest.TestCase):
    """The CLI writes byte-identical files across two independent runs --
    not just equal digests, the whole JSON-Lines file, since the fixture
    IS the artifact a Rust replay reads."""

    def test_two_runs_produce_identical_bytes(self):
        import tempfile
        with tempfile.TemporaryDirectory() as d1, tempfile.TemporaryDirectory() as d2:
            p1 = os.path.join(d1, "g.jsonl")
            p2 = os.path.join(d2, "g.jsonl")
            DF.dump_game(p1, 2, seed=42, max_plies=120, state_every=10)
            DF.dump_game(p2, 2, seed=42, max_plies=120, state_every=10)
            with open(p1, "rb") as f:
                a = f.read()
            with open(p2, "rb") as f:
                b = f.read()
            self.assertEqual(a, b)


class FixtureSchema(unittest.TestCase):
    """The shape `rust/src/fixtures.rs` is written against."""

    def setUp(self):
        self.header, self.plies, self.footer = DF.play_fixture(
            3, seed=3, max_plies=80, state_every=10, bot_name="greedy")

    def test_header_fields(self):
        h = self.header
        self.assertEqual(h["kind"], "header")
        self.assertEqual(h["players"], 3)
        self.assertEqual(h["seed"], 3)
        self.assertIn("bot", h)
        self.assertIn("state_every", h)
        self.assertIn("max_plies", h)
        self.assertIn("engine_rev", h)  # may be None, but must be present

    def test_every_record_is_json_serializable_and_round_trips(self):
        for rec in (self.header, *self.plies, self.footer):
            s = json.dumps(rec, sort_keys=True)
            self.assertEqual(json.loads(s), rec)

    def test_ply_fields_and_move_shape(self):
        self.assertTrue(self.plies)
        for p in self.plies:
            self.assertEqual(p["kind"], "ply")
            self.assertIsInstance(p["ply"], int)
            self.assertIsInstance(p["decider"], int)
            self.assertIn(p["phase"], ("politics", "actions", "done"))
            self.assertIsInstance(p["legal"], list)
            self.assertTrue(p["legal"], "legal-move list must never be empty")
            for mv in p["legal"]:
                self.assertIsInstance(mv, list)
                self.assertIsInstance(mv[0], str)
            self.assertIn(p["chosen"], p["legal"])
            self.assertIsInstance(p["digest"], str)
            self.assertEqual(len(p["digest"]), 128)  # blake2b hex digest

    def test_state_present_on_state_every_boundary_and_final_ply(self):
        for p in self.plies:
            if p["ply"] % 10 == 0:
                self.assertIn("state", p)
        self.assertIn("state", self.plies[-1])

    def test_move_serialization_is_canonical_and_lossless(self):
        self.assertEqual(DF._move_json(("upgrade", "Bronze", "Iron")),
                          ["upgrade", "Bronze", "Iron"])
        self.assertEqual(DF._move_json(("end_turn",)), ["end_turn"])
        self.assertEqual(DF._move_json(("take", 3)), ["take", 3])

    def test_footer_fields(self):
        f = self.footer
        self.assertEqual(f["kind"], "footer")
        self.assertEqual(f["plies"], len(self.plies))
        self.assertIsInstance(f["game_over"], bool)
        self.assertIsInstance(f["truncated"], bool)
        self.assertIsInstance(f["scores"], list)


class StateDigestExcludesLog(unittest.TestCase):
    def test_log_not_in_digested_payload(self):
        from engine import game
        st = game.new_game(2, seed=1)
        st.log.append("this text must never affect the digest")
        d = st.to_dict()
        self.assertNotIn("log", {k for k in d if k not in DF._DIGEST_EXCLUDE})
        # Direct check: two states differing ONLY in `log` digest identically.
        st2 = game.new_game(2, seed=1)
        self.assertEqual(DF.state_digest(st), DF.state_digest(st2))

    def test_digest_changes_when_real_state_changes(self):
        from engine import game
        st_a = game.new_game(2, seed=1)
        st_b = game.new_game(2, seed=2)
        self.assertNotEqual(DF.state_digest(st_a), DF.state_digest(st_b))


class GitRevIsReadNotShelled(unittest.TestCase):
    """`_engine_git_rev` must never invoke `git` as a subprocess (this tree
    can be a live league arm's working tree; see the standing operational
    rule this was built under). It reads `.git/HEAD` and the ref file
    directly and returns `None` rather than raising if anything about that
    layout is unexpected."""

    def test_returns_a_plausible_sha_or_none(self):
        rev = DF._engine_git_rev()
        self.assertTrue(rev is None or (isinstance(rev, str) and len(rev) == 40))

    def test_missing_git_dir_returns_none_not_raise(self):
        self.assertIsNone(DF._engine_git_rev(repo_root="/nonexistent/path/xyz"))


if __name__ == "__main__":
    unittest.main()
