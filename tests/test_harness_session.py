"""End-to-end: drive the operator loop with a scripted human.

These are the tests that matter for requirement "a silently wrong game is far
worse than an aborted one".  A perfect operator must get through cleanly; an
operator whose app disagrees with the mirror must NOT be able to keep playing
without the disagreement appearing in the log.
"""

import os
import re
import tempfile
import unittest

import advisor.state_io as S
from advisor.advisor import Advisor, load_bot
from engine import actions as A

from harness import fields as F
from harness import mirror as M
from harness.play import HarnessConsole
from harness.record import GameLog, Setup


class Operator:
    """A scripted human.  Answers prompts from the mirror itself, so by
    construction it is in perfect sync unless `drift` says otherwise."""

    def __init__(self, console_ref, stop_round=4, drift=None, move="",
                 overrides=0, rival_drift=None):
        self.ref = console_ref            # filled in after construction
        self.rival_drift = rival_drift or {}   # {round: {key: delta}}
        self.stop_round = stop_round
        self.drift = drift or {}          # {round: {key: delta}}
        self.move = move
        self.overrides = overrides        # how many times to type `move`
        self.lines = []
        self.desync_answer = "a"
        self.cause = "misread the app"
        self.quit_next = False

    @property
    def board(self):
        return self.ref[0].adv.board

    def __call__(self, prompt):
        self.lines.append(prompt)
        p = prompt.strip()
        if "new cards" in p:
            return "?"
        if "/".join(M.RIVAL_ASK_KEYS) in p:
            idx = int(re.search(r"p(\d+)", p).group(1))
            q = self.board.state.players[idx]
            from engine import effects
            s = effects.compute(self.board.state, q)
            # a perfect operator reads every asked field off the panel,
            # including the wonder count the mirror derives for itself
            vals = {"c": q.culture, "cr": s.culture, "sr": s.science,
                    "str": s.strength, "ca": q.civil_actions,
                    "hc": q.hand_size("civil"),
                    "w": len(q.completed_wonders)}
            for k, delta in self.rival_drift.get(
                    self.board.state.round, {}).items():
                vals[k] += delta
            return "/".join(str(vals[k]) for k in M.RIVAL_ASK_KEYS)
        if "/".join(M.SPINE) in p:
            snap = M.self_snapshot(self.board, M.SPINE)
            rnd = self.board.state.round
            for k, d in self.drift.get(rnd, {}).items():
                snap[k] += d
            return "/".join(str(snap[k]) for k in M.SPINE)
        if p.startswith("a/r>"):
            return self.desync_answer
        if "what caused it" in p or "a cause is required" in p:
            return self.cause
        if p.startswith(">") and self.quit_next:
            return ""
        if "hit YOU" in p:
            return ""
        if "your move>" in p:
            if self.board.state.round >= self.stop_round:
                return "quit"
            if self.overrides > 0:
                # only while a second candidate is actually on offer, or the
                # console (rightly) just re-prompts
                if len(self.ref[0].adv.recommend(2)) > 1:
                    self.overrides -= 1
                    return self.move
            return ""
        if "final scores" in p:
            return "180 200 150"
        if "wall-clock minutes" in p:
            return "70"
        if "notes" in p:
            return ""
        return ""


def build(tmpdir, operator_kw=None, players=3):
    board = S.new_board(players, me=0, seed=3)
    bot = load_bot(players, seed=3)
    adv = Advisor(board, bot, seed=3)
    adv.dealt_slots = list(range(A.ROW_SIZE))
    setup = Setup(game_id="t", players=players, seat=0, difficulty="hard",
                  mode="strict", dlc=False)
    log = GameLog(setup, os.path.join(tmpdir, "g.jsonl"),
                  requirements=F.requirements(
                      {p.id: F.INERT for p in F.PROBES}))
    ref = [None]
    op = Operator(ref, **(operator_kw or {}))
    out = []
    con = HarnessConsole(adv, log, inp=op, out=lambda *a: out.append(" ".join(
        str(x) for x in a)))
    ref[0] = con
    return con, op, out, log


class PerfectOperator(unittest.TestCase):
    def test_a_synced_game_never_reports_a_desync(self):
        with tempfile.TemporaryDirectory() as d:
            con, op, out, log = build(d)
            con.run()
            kinds = [r["type"] for r in log.records]
            self.assertIn("check", kinds)
            self.assertIn("decision", kinds)
            self.assertNotIn("resync", kinds)
            self.assertFalse(any("DESYNC" in line for line in out))
            checks = [r for r in log.records if r["type"] == "check"]
            self.assertTrue(checks)
            self.assertTrue(all(c["ok"] for c in checks))

    def test_it_checks_once_per_round_not_once_per_turn(self):
        """The whole cost saving: one snapshot a round, not one a move."""
        with tempfile.TemporaryDirectory() as d:
            con, op, out, log = build(d)
            con.run()
            rounds = [r["round"] for r in log.records if r["type"] == "check"]
            self.assertEqual(rounds, sorted(set(rounds)))

    def test_decisions_are_logged_with_the_replayable_snapshot(self):
        with tempfile.TemporaryDirectory() as d:
            con, op, out, log = build(d)
            con.run()
            dec = [r for r in log.records if r["type"] == "decision"]
            self.assertTrue(dec)
            for r in dec:
                self.assertIsInstance(r["state"], str)
                self.assertTrue(r["state"].startswith("tta"))
                self.assertIsNotNone(r["played"])
                self.assertIn(r["source"], ("bot", "human"))
            # and the snapshot really does round-trip into a live state
            S.loads(dec[0]["state"])

    def test_effort_is_measured_not_estimated(self):
        with tempfile.TemporaryDirectory() as d:
            con, op, out, log = build(d)
            rec = con.run()
            e = rec["effort"]
            self.assertGreater(e["keystrokes"], 0)
            self.assertGreater(e["rounds"], 0)
            self.assertAlmostEqual(e["keystrokes_per_round"],
                                   e["keystrokes"] / e["rounds"], places=6)


class DriftingOperator(unittest.TestCase):
    """The failure the whole harness exists to catch."""

    def test_a_drifted_board_stops_the_game(self):
        with tempfile.TemporaryDirectory() as d:
            con, op, out, log = build(d, {"stop_round": 9,
                                          "drift": {3: {"c": 11}}})
            rec = con.run()
            self.assertTrue(any("DESYNC" in line for line in out))
            self.assertTrue(rec["aborted"])
            self.assertFalse(rec["trusted"])
            self.assertIn("c", rec["abort_reason"])
            resyncs = [r for r in log.records if r["type"] == "resync"]
            self.assertEqual(len(resyncs), 1)
            self.assertEqual(resyncs[0]["round"], 3)

    def test_a_missed_rival_wonder_stops_the_game(self):
        """The one HARD check on the rival side.

        Everything else about a rival is forced, so it cannot disagree.
        Wonders arrive by name (`p1 built+ ...`) because the name carries the
        effects -- which means the mirror holds a count it can be wrong about,
        and the count on the panel is a real checksum.  An operator who forgot
        a `built+` must not be able to play on.
        """
        with tempfile.TemporaryDirectory() as d:
            con, op, out, log = build(d, {"stop_round": 9,
                                          "rival_drift": {3: {"w": 1}}})
            rec = con.run()
            self.assertTrue(any("DESYNC" in line for line in out))
            self.assertTrue(rec["aborted"])
            self.assertIn("w", rec["abort_reason"])
            bad = [r for r in log.records
                   if r["type"] == "check" and not r["ok"]]
            self.assertEqual([d_["key"] for d_ in bad[0]["discrepancies"]],
                             ["w", "w"])          # one per rival
            self.assertTrue(all(d_["severity"] == M.FAIL
                                for d_ in bad[0]["discrepancies"]))

    def test_the_failing_check_is_in_the_log_with_both_numbers(self):
        with tempfile.TemporaryDirectory() as d:
            con, op, out, log = build(d, {"stop_round": 9,
                                          "drift": {3: {"str": 4}}})
            con.run()
            bad = [r for r in log.records
                   if r["type"] == "check" and not r["ok"]]
            self.assertEqual(len(bad), 1)
            dsc = bad[0]["discrepancies"][0]
            self.assertEqual(dsc["key"], "str")
            self.assertEqual(dsc["severity"], M.FAIL)
            self.assertNotEqual(dsc["expected"], dsc["reported"])

    def test_resync_demands_a_cause_and_taints_the_game(self):
        with tempfile.TemporaryDirectory() as d:
            con, op, out, log = build(d, {"stop_round": 5,
                                          "drift": {3: {"f": 6}}})
            op.desync_answer = "r"
            op.cause = "forgot an event"
            # the correction the operator types to bring the mirror in line.
            # Only ever typed at the resync prompt, which is the one that comes
            # straight after answering 'r' -- the routine "anything to correct
            # on YOUR board" prompt uses the same '>' and must not be hijacked.
            state = {"in_resync": False, "done": False}
            inner = op.__call__

            def answer(prompt):
                p = prompt.strip()
                if p.startswith("a/r>"):
                    state["in_resync"] = True
                    return inner(prompt)
                if state["in_resync"] and p == ">" and not state["done"]:
                    state["done"] = True
                    real = M.self_snapshot(op.board, ["f"])["f"]
                    return f"p0 f={real + 6}"
                return inner(prompt)
            con.inp = answer
            rec = con.run()
            resyncs = [r for r in log.records if r["type"] == "resync"]
            self.assertEqual(len(resyncs), 1)
            self.assertEqual(resyncs[0]["cause"], "forgot an event")
            self.assertTrue(resyncs[0]["patches"])
            self.assertFalse(rec["trusted"])
            self.assertIn("desync", rec["untrusted_reason"])

    def test_there_is_no_ignore_option(self):
        """Requirement: fail loudly.  'i' must not be accepted as a way on."""
        with tempfile.TemporaryDirectory() as d:
            con, op, out, log = build(d, {"stop_round": 9,
                                          "drift": {3: {"c": 11}}})
            answers = iter(["i", "ignore", "", "a"])
            inner = op.__call__

            def answer(prompt):
                if prompt.strip().startswith("a/r>"):
                    return next(answers)
                return inner(prompt)
            con.inp = answer
            rec = con.run()
            self.assertTrue(rec["aborted"])


class Overrides(unittest.TestCase):
    def test_playing_something_other_than_the_star_is_recorded(self):
        with tempfile.TemporaryDirectory() as d:
            con, op, out, log = build(d, {"stop_round": 3, "move": "2",
                                          "overrides": 2})
            rec = con.run()
            dec = [r for r in log.records if r["type"] == "decision"]
            self.assertTrue(any(r["source"] == "human" for r in dec),
                            "an override was invisible in the log")
            self.assertTrue(any("OVERRIDE" in line for line in out))
            self.assertIn("override", rec["notes"])


class MovingTargetAtRuntime(unittest.TestCase):
    def test_a_new_mandatory_field_is_announced_mid_game(self):
        """When the evaluator gains an eye, the operator is told, loudly,
        without anybody editing this harness."""
        with tempfile.TemporaryDirectory() as d:
            con, op, out, log = build(d, {"stop_round": 3})
            self.addCleanup(log.close)
            con.verdicts = {p.id: F.INERT for p in F.PROBES}
            con.mandatory = con._mandatory_ids()
            con._rederive()
            self.assertTrue(any("THE BOT'S EYES CHANGED" in line
                                for line in out),
                            "the live derivation never fired")


if __name__ == "__main__":
    unittest.main()
