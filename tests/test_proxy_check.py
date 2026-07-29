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


def h2h(win_rate, ci, null=0.5, margin=0.0, margin_ci=1.0):
    return {"h2h": {"win_rate": win_rate, "ci": ci, "null": null,
                    "margin": margin, "margin_ci": margin_ci,
                    "culture": 100.0, "opp_culture": 100.0,
                    "games": 40, "deals": 20, "secs": 1.0},
            "anchor": {"opponent": "book", "win_rate": 1.0, "ci": 0.0,
                       "margin": 50.0, "culture": 150.0, "opp_culture": 100.0,
                       "games": 20, "deals": 10, "secs": 1.0}}


class Verdicts(unittest.TestCase):
    """The verdict is on the CULTURE MARGIN, and there are FOUR of them."""

    def v(self, margin, margin_ci):
        return C.verdict_of(h2h(0.5, 0.1, margin=margin,
                                margin_ci=margin_ci)["h2h"])

    def test_four_verdicts(self):
        self.assertEqual(self.v(+30.0, 8.0), "confirms")
        self.assertEqual(self.v(-30.0, 8.0), "INVERTED")
        self.assertEqual(self.v(+1.0, 8.0), "flat")
        self.assertEqual(self.v(+30.0, 40.0), "inconclusive")

    def test_a_bound_sitting_on_the_threshold_is_not_a_confirm(self):
        """The bug this rule exists for: the first real reading had a lower
        bound of 50.03% against a 50% null and printed `confirms`."""
        self.assertNotEqual(self.v(C.MARGIN_MIN + 8.0, 8.0), "confirms")
        self.assertEqual(self.v(C.MARGIN_MIN + 8.001, 8.0), "confirms")

    def test_a_wide_ci_is_never_flat(self):
        """`flat` claims a measurement.  Only a CI that could have SEEN the
        effect is allowed to make it."""
        self.assertEqual(self.v(0.0, C.MARGIN_RESOLUTION + 0.1),
                         "inconclusive")
        self.assertEqual(self.v(0.0, C.MARGIN_RESOLUTION), "flat")

    def test_a_missing_margin_is_inconclusive_not_confirmed(self):
        self.assertEqual(C.verdict_of({"margin": None, "margin_ci": None}),
                         "inconclusive")


class Divergence(unittest.TestCase):
    def hist(self, *verdicts):
        return [{"verdict": v, "accepts_between": 5,
                 "h2h": {"margin_ci": 20.0}} for v in verdicts]

    def test_an_inversion_is_immediately_loud(self):
        d, why = C.divergence(self.hist("confirms", "INVERTED"))
        self.assertTrue(d)
        self.assertIn("WORSE", why)

    def test_a_run_of_flats_is_a_divergence(self):
        self.assertFalse(C.divergence(self.hist("flat", "flat"))[0])
        d, why = C.divergence(self.hist("flat", "flat", "flat"))
        self.assertTrue(d)
        self.assertIn("15 accepted", why)

    def test_inconclusive_readings_are_not_a_divergence(self):
        """They are the INSTRUMENT failing, not the proxy.  Counting them as
        a divergence would make the guardrail cry wolf about the training
        loop when the real fault is its own sample size."""
        h = self.hist("inconclusive", "inconclusive", "inconclusive")
        self.assertFalse(C.divergence(h)[0])
        wide, why = C.unresolved(h)
        self.assertTrue(wide)
        self.assertIn("--deals", why)

    def test_inconclusive_does_not_mask_a_run_of_flats(self):
        h = self.hist("flat", "inconclusive", "flat", "inconclusive", "flat")
        self.assertTrue(C.divergence(h)[0])

    def test_a_resolved_gain_clears_the_unresolved_alarm(self):
        h = self.hist("inconclusive", "inconclusive", "confirms")
        self.assertFalse(C.unresolved(h)[0])

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
        self.result = h2h(0.70, 0.10, margin=30.0, margin_ci=8.0)
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
        self.result = h2h(0.20, 0.05, margin=-30.0, margin_ci=8.0)
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


class Starvation(unittest.TestCase):
    """A monitor that stops monitoring must say so."""

    def test_quiet_when_nothing_is_pending_or_it_is_recent(self):
        self.assertFalse(C.starvation([], 10, 0, 999.0, 24.0)[0])
        self.assertFalse(C.starvation([], 10, 5, 1.0, 24.0)[0])

    def test_loud_when_accepts_pile_up_with_no_reading(self):
        starved, why = C.starvation([], 42, 9, 30.0, 24.0)
        self.assertTrue(starved)
        self.assertIn("NEVER", why)
        self.assertIn("9 champions", why)

    def test_it_names_the_last_reading_when_there_was_one(self):
        hist = [{"champion_gen": 7}]
        starved, why = C.starvation(hist, 42, 9, 30.0, 24.0)
        self.assertTrue(starved)
        self.assertIn("gen 7", why)


class LockWaits(unittest.TestCase):
    """Skip-and-forget is what emptied the guardrail's first day."""

    def test_it_waits_for_a_held_lock_and_then_gives_up_visibly(self):
        d = tempfile.mkdtemp(prefix="proxylock")
        try:
            path = os.path.join(d, "lock")
            with open(path, "w") as fh:
                fh.write("someone else\n")
            lock = C.Lock(path=path, stale_h=99.0, wait_s=0.6, poll_s=0.2)
            with lock as lk:
                self.assertIsNone(lk)
            self.assertGreaterEqual(lock.waited, 0.5)
        finally:
            shutil.rmtree(d, ignore_errors=True)

    def test_a_stale_lock_is_stolen_rather_than_wedging_forever(self):
        d = tempfile.mkdtemp(prefix="proxylock")
        try:
            path = os.path.join(d, "lock")
            with open(path, "w") as fh:
                fh.write("dead holder\n")
            os.utime(path, (0, 0))          # ancient
            with C.Lock(path=path, stale_h=1.0, wait_s=5.0, poll_s=0.1) as lk:
                self.assertIsNotNone(lk)
            self.assertFalse(os.path.exists(path))
        finally:
            shutil.rmtree(d, ignore_errors=True)


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
