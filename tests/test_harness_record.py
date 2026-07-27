"""The per-game record: setup gates, the bias register, and aggregation.

A logged game is only useful if, months later, somebody can tell what it
measured.  Two things have to survive that trip: the *setup* (a DLC game is a
mislabelled log, not a weak one) and the *limitations* (no pacts were possible,
so this is not a full-game result).
"""

import json
import os
import tempfile
import unittest

from harness.record import GameLog, Setup, SetupError, load, summarize


def setup(**kw):
    base = dict(game_id="t1", players=3, seat=0, difficulty="hard",
                mode="strict", dlc=False)
    base.update(kw)
    return Setup(**base)


class SetupGates(unittest.TestCase):
    def test_dlc_game_is_refused(self):
        with self.assertRaises(SetupError) as cm:
            setup(dlc=True).validate()
        self.assertIn("New Leaders", str(cm.exception))

    def test_base_game_is_accepted(self):
        self.assertIs(setup().validate().dlc, False)

    def test_unknown_difficulty_is_refused(self):
        with self.assertRaises(SetupError):
            setup(difficulty="nightmare").validate()

    def test_seat_must_exist(self):
        with self.assertRaises(SetupError):
            setup(players=2, seat=2).validate()

    def test_player_count_bounds(self):
        with self.assertRaises(SetupError):
            setup(players=5).validate()

    def test_personalities_are_a_different_experiment(self):
        with self.assertRaises(SetupError):
            setup(difficulty="medium", personalities=["Napoleon"]).validate()

    def test_setup_is_recorded_verbatim(self):
        log = GameLog(setup(app_version="2.4.1"), None)
        head = log.records[0]
        self.assertEqual(head["setup"]["difficulty"], "hard")
        self.assertEqual(head["setup"]["app_version"], "2.4.1")
        self.assertIs(head["setup"]["dlc"], False)
        self.assertEqual(head["setup"]["edition"], "2015-base")


class PactBias(unittest.TestCase):
    """Requirement: nobody may later report this as a full-game result."""

    def test_header_carries_the_pact_note(self):
        log = GameLog(setup(), None)
        ids = {l["id"] for l in log.records[0]["limitations"]}
        self.assertIn("no_pacts", ids)

    def test_result_repeats_it(self):
        """Whoever reads the result may never see the header."""
        log = GameLog(setup(), None)
        rec = log.result({"p0": 180, "p1": 200, "p2": 150}, 20)
        note = next(l for l in rec["limitations"] if l["id"] == "no_pacts")
        self.assertEqual(note["severity"], "high")
        self.assertIn("STRICTLY SMALLER GAME", note["text"])

    def test_two_player_games_downgrade_it_honestly(self):
        """At 2p the rules disable pacts anyway, so the bias is not an
        app-specific distortion and must not be overstated."""
        log = GameLog(setup(players=2), None)
        note = next(l for l in log.records[0]["limitations"]
                    if l["id"] == "no_pacts")
        self.assertEqual(note["severity"], "low")

    def test_free_mode_says_the_score_is_not_the_bot(self):
        log = GameLog(setup(mode="free"), None)
        ids = {l["id"] for l in log.records[0]["limitations"]}
        self.assertIn("free_mode", ids)


class Trust(unittest.TestCase):
    def test_a_clean_game_is_trusted(self):
        log = GameLog(setup(), None)
        rec = log.result({"p0": 200, "p1": 180, "p2": 150}, 20)
        self.assertTrue(rec["trusted"])
        self.assertTrue(rec["won"])
        self.assertEqual(rec["margin"], 20)

    def test_a_resynced_game_is_not_trusted(self):
        log = GameLog(setup(), None)
        log.resync(7, [], ["p1 c=40"], "misread the culture track")
        rec = log.result({"p0": 200, "p1": 180, "p2": 150}, 20)
        self.assertFalse(rec["trusted"])
        self.assertIn("7", rec["untrusted_reason"])

    def test_an_aborted_game_has_no_winner(self):
        log = GameLog(setup(), None)
        rec = log.result({}, 9, aborted=True, abort_reason="desync")
        self.assertFalse(rec["trusted"])
        self.assertIsNone(rec["winner"])

    def test_margin_is_against_the_best_opponent(self):
        log = GameLog(setup(), None)
        rec = log.result({"p0": 150, "p1": 180, "p2": 200}, 20)
        self.assertEqual(rec["margin"], -50)
        self.assertFalse(rec["won"])


class RoundTrip(unittest.TestCase):
    def test_jsonl_is_flushed_per_record(self):
        """A game abandoned in round 9 must still leave rounds 1-9 on disk."""
        with tempfile.TemporaryDirectory() as d:
            path = os.path.join(d, "g.jsonl")
            log = GameLog(setup(), path)
            log.decision("tta 1\n...", [], ["take", 3], "bot", 4, "I")
            # deliberately do NOT close
            recs = load(path)
            self.assertEqual([r["type"] for r in recs], ["game", "decision"])
            log.close()

    def test_every_record_is_json_serialisable(self):
        with tempfile.TemporaryDirectory() as d:
            path = os.path.join(d, "g.jsonl")
            log = GameLog(setup(), path)
            log.decision("snap", [{"move": ["take", 1], "score": 1.5}],
                         ["take", 1], "bot", 3, "I", latency_s=2.0)
            log.observed(3, [10, 11], {"1": {"c": 20}}, ["take p1 4"])
            log.result({"p0": 1, "p1": 2, "p2": 3}, 18, human_minutes=64)
            with open(path) as fh:
                for line in fh:
                    json.loads(line)

    def test_the_state_snapshot_is_stored_as_a_string(self):
        """Section 6b: embed `state_io.dumps()` verbatim so every logged
        position can be replayed by a future bot."""
        log = GameLog(setup(), None)
        rec = log.decision("tta 1\ngame 3p ...", [], ["end_turn"], "bot", 5, "I")
        self.assertIsInstance(rec["state"], str)


class Aggregate(unittest.TestCase):
    def _write(self, d, name, scores, **kw):
        path = os.path.join(d, name)
        log = GameLog(setup(game_id=name, **kw.pop("setup_kw", {})), path)
        log.result(scores, 20, **kw)
        return path

    def test_untrusted_games_are_counted_but_never_pooled(self):
        with tempfile.TemporaryDirectory() as d:
            good = self._write(d, "a.jsonl", {"p0": 200, "p1": 180, "p2": 150})
            path = os.path.join(d, "b.jsonl")
            log = GameLog(setup(game_id="b"), path)
            log.resync(4, [], [], "unknown")
            log.result({"p0": 100, "p1": 300, "p2": 150}, 20)
            s = summarize([good, path])
            self.assertEqual(s["trusted_games"], 1)
            self.assertEqual(s["dropped_games"], 1)
            self.assertEqual(s["mean_margin"], 20)

    def test_mixed_setups_are_not_poolable(self):
        with tempfile.TemporaryDirectory() as d:
            a = self._write(d, "a.jsonl", {"p0": 200, "p1": 180, "p2": 150})
            b = self._write(d, "b.jsonl", {"p0": 200, "p1": 180, "p2": 150},
                            setup_kw={"difficulty": "medium"})
            s = summarize([a, b])
            self.assertFalse(s["poolable"])
            self.assertIn("MIXED SETUPS", s["caveat"])

    def test_same_setup_is_poolable(self):
        with tempfile.TemporaryDirectory() as d:
            a = self._write(d, "a.jsonl", {"p0": 200, "p1": 180, "p2": 150})
            b = self._write(d, "b.jsonl", {"p0": 100, "p1": 180, "p2": 150})
            s = summarize([a, b])
            self.assertTrue(s["poolable"])
            self.assertEqual(s["win_rate"], 0.5)
            self.assertEqual(s["mean_margin"], -30)

    def test_the_caveat_always_mentions_pacts(self):
        with tempfile.TemporaryDirectory() as d:
            a = self._write(d, "a.jsonl", {"p0": 200, "p1": 1, "p2": 1})
            self.assertIn("pact", summarize([a])["caveat"])

    def test_a_log_without_a_result_is_dropped_not_ignored(self):
        with tempfile.TemporaryDirectory() as d:
            path = os.path.join(d, "c.jsonl")
            log = GameLog(setup(), path)
            log.decision("s", [], ["end_turn"], "bot", 1, "I")
            log.close()
            s = summarize([path])
            self.assertEqual(s["dropped_games"], 1)
            self.assertEqual(s["dropped"][0]["why"], "no result record")


if __name__ == "__main__":
    unittest.main()
