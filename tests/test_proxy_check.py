"""The proxy guardrail: does it fire when it should, and shout when it must.

No games are played here -- `proxy_check.measure` is stubbed.  What is tested
is the decision logic around it, which is the part that has to be right for the
guardrail to be worth having: when a reading is DUE, what the verdict is, and
whether a run of readings that never confirm is reported as a divergence.
"""
import json
import os
import shutil
import sys
import tempfile
import unittest

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

from experiments import proxy_check as C  # noqa: E402


def h2h(win_rate, ci, null=0.5):
    return {"h2h": {"win_rate": win_rate, "ci": ci, "null": null,
                    "margin": 0.0, "culture": 100.0, "opp_culture": 100.0,
                    "games": 40, "deals": 20, "secs": 1.0},
            "anchor": {"opponent": "book", "win_rate": 1.0, "ci": 0.0,
                       "margin": 50.0, "culture": 150.0, "opp_culture": 100.0,
                       "games": 20, "deals": 10, "secs": 1.0}}


class Verdicts(unittest.TestCase):
    def test_three_verdicts(self):
        self.assertEqual(C.verdict_of(h2h(0.70, 0.10)["h2h"]), "confirms")
        self.assertEqual(C.verdict_of(h2h(0.52, 0.10)["h2h"]), "flat")
        self.assertEqual(C.verdict_of(h2h(0.30, 0.10)["h2h"]), "INVERTED")

    def test_the_null_is_the_seat_share_not_one_half(self):
        """At 4p a challenger's null is 25%, so 40% is a CONFIRM there."""
        self.assertEqual(C.verdict_of(h2h(0.40, 0.05, null=0.25)["h2h"]),
                         "confirms")


class Divergence(unittest.TestCase):
    def hist(self, *verdicts):
        return [{"verdict": v, "accepts_between": 5} for v in verdicts]

    def test_an_inversion_is_immediately_loud(self):
        d, why = C.divergence(self.hist("confirms", "INVERTED"))
        self.assertTrue(d)
        self.assertIn("WORSE", why)

    def test_a_run_of_flats_is_a_divergence(self):
        self.assertFalse(C.divergence(self.hist("flat", "flat"))[0])
        d, why = C.divergence(self.hist("flat", "flat", "flat"))
        self.assertTrue(d)
        self.assertIn("15 accepted", why)

    def test_a_confirm_clears_it(self):
        self.assertFalse(C.divergence(
            self.hist("flat", "flat", "confirms"))[0])

    def test_an_empty_history_is_not_a_divergence(self):
        self.assertFalse(C.divergence([])[0])


class Scheduling(unittest.TestCase):
    def setUp(self):
        self.dir = tempfile.mkdtemp(prefix="proxytest")
        self.ladder = os.path.join(self.dir, "ladder_2p")
        os.makedirs(self.ladder)
        self.calls = []
        self.result = h2h(0.70, 0.10)
        self._real = C.measure
        C.measure = self.fake
        self._log = C.LOG
        C.LOG = os.path.join(self.dir, "proxy.log")

    def tearDown(self):
        C.measure = self._real
        C.LOG = self._log
        shutil.rmtree(self.dir, ignore_errors=True)

    def fake(self, new_path, base_path, *a, **kw):
        self.calls.append((os.path.basename(new_path),
                           os.path.basename(base_path)))
        return dict(self.result)

    def accept(self, gen):
        with open(os.path.join(self.ladder, f"gen{gen:05d}.json"), "w") as fh:
            json.dump({"weights": {}}, fh)

    def check(self, **kw):
        kw.setdefault("every_accepts", 3)
        kw.setdefault("max_hours", 1e9)
        return C.check_arm(2, state_dir=self.dir, log=lambda *_a: None, **kw)

    def test_nothing_to_do_with_one_champion(self):
        self.accept(1)
        self.assertIsNone(self.check())
        self.assertEqual(self.calls, [])

    def test_the_first_reading_is_taken_as_soon_as_there_is_a_comparison(self):
        """A monitor that says nothing until N accepts have gone by is a
        monitor with a blind spot exactly where a retarget lands."""
        for g in (1, 2, 3):
            self.accept(g)
        rec = self.check()
        self.assertIsNotNone(rec)
        self.assertEqual(self.calls, [("gen00003.json", "gen00001.json")])
        self.assertEqual(rec["accepts_between"], 2)
        self.assertEqual(rec["verdict"], "confirms")

    def test_the_next_reading_chains_from_the_last_validated_champion(self):
        for g in range(1, 5):
            self.accept(g)
        self.check()
        for g in (5, 6):
            self.accept(g)
        self.assertIsNone(self.check())     # only 2 accepts since gen 4
        self.accept(7)
        self.check()
        self.assertEqual(self.calls[-1], ("gen00007.json", "gen00004.json"))
        hist = C.read_history(self.dir, 2)
        self.assertEqual([r["champion_gen"] for r in hist], [4, 7])
        self.assertEqual([r["baseline_gen"] for r in hist], [1, 4])

    def test_max_hours_forces_a_reading_for_a_slow_arm(self):
        for g in range(1, 5):
            self.accept(g)
        self.check()
        self.accept(5)
        self.assertIsNone(self.check())
        self.assertIsNotNone(self.check(max_hours=0.0))

    def test_a_history_record_is_machine_readable_and_complete(self):
        for g in range(1, 5):
            self.accept(g)
        self.check()
        with open(C.history_path(self.dir, 2)) as fh:
            line = fh.read().strip()
        rec = json.loads(line)
        for key in ("at", "ts", "players", "policy", "champion_gen",
                    "baseline_gen", "accepts_between", "gens_between",
                    "proxy_edge_sum", "h2h", "anchor", "verdict"):
            self.assertIn(key, rec)

    def test_the_log_shouts_on_an_inversion(self):
        for g in range(1, 5):
            self.accept(g)
        self.result = h2h(0.20, 0.05)
        self.check()
        with open(C.LOG) as fh:
            text = fh.read()
        self.assertIn("INVERTED", text)
        self.assertIn("PROXY DIVERGENCE", text)

    def test_the_log_is_quiet_when_the_proxy_is_confirmed(self):
        for g in range(1, 5):
            self.accept(g)
        self.check()
        with open(C.LOG) as fh:
            self.assertNotIn("PROXY DIVERGENCE", fh.read())


class AcceptedEdges(unittest.TestCase):
    def test_only_accepted_rows_in_range_are_counted(self):
        d = tempfile.mkdtemp(prefix="proxyedge")
        try:
            with open(os.path.join(d, "generations_2p.jsonl"), "w") as fh:
                for gen, acc, edge in ((1, True, 0.10), (2, False, None),
                                       (3, True, 0.20), (4, True, 0.30)):
                    fh.write(json.dumps({"gen": gen, "accepted": acc,
                                         "edge": edge}) + "\n")
            self.assertEqual(C.accepted_edges(d, 2, 1, 4), [0.20, 0.30])
            self.assertEqual(C.accepted_edges(d, 2, 0, 4), [0.10, 0.20, 0.30])
        finally:
            shutil.rmtree(d, ignore_errors=True)


if __name__ == "__main__":
    unittest.main()
