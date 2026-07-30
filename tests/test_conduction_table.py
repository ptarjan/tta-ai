"""The conduction table is a safety label, so it has to be checked like one.

`docs/CARD_BLINDNESS.md` Sec 5.3 spent 12,800 games measuring a lever that was
multiplied by zero. `tools/conduction_table.py` exists to print, in one
second, the sentence that would have stopped it -- "for a WONDER
specifically: NOTHING". These tests fail if that sentence stops being printed
for a vector that deserves it, or starts being printed for one that does not.
"""
import os
import sys
import unittest

_ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..")
sys.path.insert(0, _ROOT)
sys.path.insert(0, os.path.join(_ROOT, "tools"))

import conduction_table as CT                        # noqa: E402
from engine.bots.weighted import DEFAULT_WEIGHTS     # noqa: E402


def frozen(name):
    return os.path.join(_ROOT, "analysis", "frozen", name)


def _live_or_skip(case, players):
    """The 99-key reference for `players`, from the frozen copy if it has been
    cut, else from the live league state. Skips if neither is present."""
    import glob
    from engine.bots.weighted import load_weights
    hits = sorted(glob.glob(frozen(f"champion_{players}p_gen*_99key.json")))
    if hits:
        return load_weights(hits[0])
    live = os.path.join(_ROOT, "experiments", "league_state",
                        f"champion_{players}p.json")
    if os.path.exists(live):
        return load_weights(live)
    case.skipTest(f"no 99-key {players}p reference available")


class TheSentenceThatWouldHavePreventedIt(unittest.TestCase):

    def test_frozen_2p_reports_nothing_for_a_wonder(self):
        p = frozen("champion_2p.json")
        if not os.path.exists(p):
            self.skipTest("frozen 2p reference retired")
        txt = CT.report(p)
        self.assertIn("for a WONDER specifically   : NOTHING", txt)
        self.assertIn("BOTH GATES MOOT", txt)

    def test_frozen_2p_still_conducts_for_a_leader(self):
        """The same vector, the same lever, a different card class. This
        asymmetry is why Sec 5's +9.5pp headline is real and its wonder null
        is not."""
        p = frozen("champion_2p.json")
        if not os.path.exists(p):
            self.skipTest("frozen 2p reference retired")
        self.assertIn("for ANY card                : hand_potential",
                      CT.report(p))

    def test_a_99key_reference_conducts_for_a_wonder(self):
        for n in (2, 3, 4):
            for cand in (f"champion_{n}p_gen*_99key.json",):
                import glob
                hits = glob.glob(frozen(cand))
                if not hits:
                    continue
                with self.subTest(players=n):
                    txt = CT.report(hits[0])
                    self.assertIn("row_pressure", txt)
                    self.assertNotIn("BOTH GATES MOOT", txt)
                    self.assertNotIn(
                        "for a WONDER specifically   : NOTHING", txt)


class GateTwoIsReportedSeparatelyFromGateOne(unittest.TestCase):
    """Passing gate 1 tells you nothing about gate 2, which is a threshold:
    `row_pressure` drops any card whose `card_potential` is <= 0."""

    def test_raising_the_credit_only_ever_adds_visible_wonders(self):
        """The universal property: `card_rate_credit` scales a non-negative
        contribution, so the visible set is monotone in it. True of every
        vector -- unlike the SIZE of the effect, which is not (see below)."""
        for base in (dict(DEFAULT_WEIGHTS), _live_or_skip(self, 2)):
            _, _, _, hi = CT.visibility(dict(base, card_rate_credit=1.0))
            _, _, _, lo = CT.visibility(dict(base, card_rate_credit=0.0))
            self.assertTrue(set(lo).issubset(set(hi)))

    def test_the_threshold_bites_on_the_live_2p_vector_but_not_on_defaults(self):
        """This asymmetry is the whole point and it is easy to get backwards.

        Under `DEFAULT_WEIGHTS` the other terms are large enough that 11 of 16
        wonders already price above zero and the credit changes NOTHING -- so
        a probe run against defaults would have shown no threshold at all.
        Under the live 2p champion, whose trained `card_rate_credit` is
        0.12812, the same knob moves the visible set 0 -> 8. The gate is a
        property of the vector, not of the code.
        """
        d = dict(DEFAULT_WEIGHTS)
        _, _, _, d_hi = CT.visibility(dict(d, card_rate_credit=1.0))
        _, _, _, d_lo = CT.visibility(dict(d, card_rate_credit=0.0))
        self.assertEqual(len(d_hi), len(d_lo),
                         "defaults are supposed to be the insensitive case")

        live = _live_or_skip(self, 2)
        _, _, _, l_hi = CT.visibility(dict(live, card_rate_credit=1.0))
        _, _, _, l_lo = CT.visibility(dict(live, card_rate_credit=0.0))
        self.assertEqual(len(l_lo), 0)
        self.assertEqual(len(l_hi), 8)

    def test_report_names_both_gates(self):
        p = frozen("champion_2p.json")
        if not os.path.exists(p):
            self.skipTest("frozen 2p reference retired")
        txt = CT.report(p)
        self.assertIn("Gate 1", txt)
        self.assertIn("Gate 2", txt)

    def test_every_consumer_is_covered_by_the_gate_map(self):
        from experiments import arena
        for fn in arena.CARD_POTENTIAL_CONSUMERS:
            self.assertIn(fn, arena.EVALUATE_GATES)


if __name__ == "__main__":
    unittest.main()
