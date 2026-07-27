"""What the league's accept decision maximises, and the pool it maximises it on.

These pin the 2026-07-27 objective change (docs/LEAGUE_OBJECTIVE.md).  Two
classes of thing are tested and the second is the reason this file exists:

1. The NEW behaviour -- own-culture scoring, the blend, the rebalanced pool,
   and the mirror reference no longer being analytic.
2. The OLD behaviour, byte for byte.  Every champion this project has ever
   produced was selected under `--objective margin` on the old tier weights.
   If that mode drifts, no historical result can be reproduced, and a drift
   would be silent.
"""
import os
import sys
import unittest

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

from experiments import hillclimb_pool as P
from experiments import hillclimb_league as L


class OwnShare(unittest.TestCase):
    def test_range_and_monotone(self):
        prev = -1.0
        for c in range(0, 401, 5):
            v = P.own_share(c)
            self.assertTrue(0.0 < v < 1.0, (c, v))
            self.assertGreater(v, prev, c)      # strictly increasing
            prev = v

    def test_centre_maps_to_half(self):
        self.assertAlmostEqual(P.own_share(P.CULTURE_CENTRE), 0.5, places=12)

    def test_none_passes_through(self):
        self.assertIsNone(P.own_share(None))

    def test_marginal_value_is_flat_across_the_band_we_care_about(self):
        """A culture point must be worth about the same at 65 as at 160.

        This is the whole reason the squash is OFFSET.  Uncentred, one point
        of culture at a human score (159.5) is worth a third of one at our
        score (64.7), which is a built-in bias against ever closing the gap.
        """
        def slope(c):
            return (P.own_share(c + 0.5) - P.own_share(c - 0.5))
        ratio = slope(64.7) / slope(159.5)
        self.assertLess(ratio, 1.35, f"marginal value varies {ratio:.2f}x "
                                     f"across 65..160; re-tune CULTURE_CENTRE")
        # ... and the uncentred version really is as bad as claimed, so the
        # offset is not cargo cult.
        def raw(c):
            return P.own_share(c, centre=0.0)
        bad = (raw(65.2) - raw(64.2)) / (raw(160.0) - raw(159.0))
        self.assertGreater(bad, 2.5)


class ScoreSeries(unittest.TestCase):
    RES = {
        "per_game": [1.0, 0.0, 0.5, None],
        "per_game_margin": [40.0, -40.0, 0.0, None],
        "per_game_culture": [150.0, 50.0, 100.0, None],
    }

    def test_winshare_is_the_raw_list(self):
        self.assertEqual(P.score_series(self.RES, "winshare"),
                         self.RES["per_game"])

    def test_margin_is_unchanged_from_the_legacy_implementation(self):
        got = P.score_series(self.RES, "margin")
        want = [0.5 * (1 + __import__("math").tanh(m / P.MARGIN_SCALE))
                if m is not None else None
                for m in self.RES["per_game_margin"]]
        self.assertEqual(got, want)

    def test_own_uses_own_culture_not_the_margin(self):
        got = P.score_series(self.RES, "own")
        self.assertEqual(got[2], 0.5)                 # 100 == the centre
        self.assertGreater(got[0], got[2])
        self.assertLess(got[1], got[2])
        self.assertIsNone(got[3])

    def test_blend_is_a_convex_combination(self):
        p = P.ScoreParams(alpha=0.25)
        own = P.score_series(self.RES, "own", p)
        win = self.RES["per_game"]
        got = P.score_series(self.RES, "blend", p)
        for g, o, w in zip(got[:3], own[:3], win[:3]):
            self.assertAlmostEqual(g, 0.75 * o + 0.25 * w, places=12)
            self.assertTrue(0.0 <= g <= 1.0)
        self.assertIsNone(got[3])

    def test_blend_endpoints_are_the_pure_objectives(self):
        self.assertEqual(P.score_series(self.RES, "blend",
                                        P.ScoreParams(alpha=0.0))[:3],
                         P.score_series(self.RES, "own")[:3])
        self.assertEqual(P.score_series(self.RES, "blend",
                                        P.ScoreParams(alpha=1.0))[:3],
                         self.RES["per_game"][:3])

    def test_theft_is_paid_once_not_twice(self):
        """The bug, stated as a test.

        Two candidates on the same board: one PRODUCES 20 culture, the other
        STEALS 20 from the rival.  Own culture moves identically for both --
        which is what the rules say.  Margin moves twice as far for the thief.
        """
        base = {"per_game": [1.0], "per_game_margin": [0.0],
                "per_game_culture": [100.0]}
        produce = {"per_game": [1.0], "per_game_margin": [20.0],
                   "per_game_culture": [120.0]}
        steal = {"per_game": [1.0], "per_game_margin": [40.0],
                 "per_game_culture": [120.0]}
        own_p = P.score_series(produce, "own")[0] - P.score_series(base, "own")[0]
        own_s = P.score_series(steal, "own")[0] - P.score_series(base, "own")[0]
        self.assertAlmostEqual(own_p, own_s, places=12)
        mar_p = P.score_series(produce, "margin")[0] - P.score_series(base, "margin")[0]
        mar_s = P.score_series(steal, "margin")[0] - P.score_series(base, "margin")[0]
        self.assertGreater(mar_s, 1.9 * mar_p)


class PoolMetric(unittest.TestCase):
    def build(self, **kw):
        return P.build_pool(2, ladder_dirs=(), past_k=0,
                            log=lambda *_a: None, **kw)

    def test_legacy_default_is_winshare_with_a_margin_gate(self):
        pool = self.build()
        got = {e.label: e.metric for e in pool.entries}
        self.assertEqual(got["book"], "margin")
        self.assertEqual(got["var:culture"], "margin")
        self.assertEqual(got["mirror"], "winshare")

    def test_own_and_blend_apply_to_every_tier(self):
        for metric in ("own", "blend"):
            pool = self.build(metric=metric)
            for e in pool.entries:
                self.assertEqual(e.metric, metric, e.label)

    def test_legacy_tier_weights_reproduce_the_shipped_pool(self):
        """The exact per-opponent weights the live 2p arm logged on 2026-07-27."""
        pool = self.build(tier_weights=P.parse_tier_weights(
            P.legacy_weight_string()))
        w = {e.label: round(e.weight, 2) for e in pool.entries}
        self.assertEqual(w["book"], 1.50)
        self.assertEqual(w["var:culture"], 0.42)
        self.assertEqual(w["mirror"], 1.00)
        self.assertEqual(w["greedy"], 0.17)

    def test_the_saturated_floor_tier_is_off_by_default(self):
        labels = {e.label for e in self.build().entries}
        self.assertNotIn("greedy", labels)
        self.assertNotIn("random", labels)
        self.assertNotIn("default", labels)

    def test_the_majority_of_the_weight_is_on_opponents_that_improve(self):
        hall = os.path.join(os.path.dirname(os.path.abspath(__file__)),
                            "..", "engine")     # any dir with .json files? no
        pool = P.build_pool(2, ladder_dirs=(), past_k=0, metric="blend",
                            log=lambda *_a: None)
        # With no ladder and no hall dir on this machine only book/variant/
        # mirror exist, so assert on the TIER TOTALS, which is the dial.
        tw = P.DEFAULT_TIER_WEIGHTS
        static = tw["book"] + tw["variant"]
        improving = tw["mirror"] + tw["past"] + tw["hall"]
        self.assertGreater(improving, 2.5 * static)
        self.assertEqual(tw["floor"], 0.0)
        self.assertIn("mirror", {e.label for e in pool.entries})
        del hall


class AcceptanceSubset(unittest.TestCase):
    def pool(self):
        entries = [P.PoolEntry("mirror", P.MIRROR, "mirror"),
                   P.PoolEntry("book", "book", "book"),
                   P.PoolEntry("book2", "book2", "book")]
        entries += [P.PoolEntry(f"var:{i}", f"v{i}", "variant") for i in range(6)]
        entries += [P.PoolEntry(f"past:{i}", {"a": i}, "past") for i in range(2)]
        entries += [P.PoolEntry(f"hall:{i}", {"b": i}, "hall") for i in range(3)]
        return P.Pool(entries, metric="blend")

    def test_every_generation_gets_mirror_a_gate_and_a_ladder(self):
        pool = self.pool()
        for gen in range(40):
            sub = pool.acceptance_subset(gen, 4)
            tiers = [e.tier for e in sub]
            self.assertEqual(len(sub), 4, gen)
            self.assertEqual(len(set(e.label for e in sub)), 4, gen)
            self.assertIn("mirror", tiers, gen)
            self.assertTrue(set(tiers) & set(pool.gate_tiers), gen)
            self.assertTrue(set(tiers) & set(pool.ladder_tiers), gen)

    def test_mirror_never_carries_a_majority_of_a_generations_weight(self):
        """The ladder invariant exists to stop exactly this.

        Without it the rotation hands some generations mirror plus three
        0.10-weight variants, and mirror alone decides ~77% of the accept --
        i.e. the mirror-only loop this whole module replaced.
        """
        pool = self.pool()
        pool.tier_weights = dict(P.DEFAULT_TIER_WEIGHTS)
        pool.renormalise()
        worst = 0.0
        for gen in range(40):
            sub = pool.acceptance_subset(gen, 4)
            tot = sum(e.weight for e in sub)
            share = max(e.weight for e in sub if e.tier == "mirror") / tot
            worst = max(worst, share)
        self.assertLess(worst, 0.62, f"mirror reached {worst:.0%} of a "
                                     f"generation's accept weight")

    def test_ladder_invariant_can_be_switched_off(self):
        """`ladder_tiers=()` restores the pre-rebalance rotation exactly."""
        pool = self.pool()
        pool.ladder_tiers = ()
        # size 2 is now mirror + one gate and nothing else...
        for gen in range(10):
            tiers = [e.tier for e in pool.acceptance_subset(gen, 2)]
            self.assertEqual(tiers[0], "mirror", gen)
            self.assertIn(tiers[1], pool.gate_tiers, gen)
            self.assertEqual(len(tiers), 2, gen)
        # ...and some generation's size-4 subset has no ladder opponent at all,
        # which is precisely the hole the invariant plugs.
        holes = [gen for gen in range(40)
                 if not any(e.tier in ("past", "hall")
                            for e in pool.acceptance_subset(gen, 4))]
        self.assertTrue(holes)


class MirrorReference(unittest.TestCase):
    """The mirror shortcut is only valid for win share and margin."""

    class FakeArena:
        def __init__(self):
            self.calls = []

        def duel(self, a, b, players, games, seed0=0, workers=None, **kw):
            self.calls.append((players, games, seed0))
            return {"per_game": [1.0] * games,
                    "per_game_margin": [0.0] * games,
                    "per_game_culture": [123.0] * games}

    def run_with(self, metric):
        fake = self.FakeArena()
        real = L.arena.duel
        L.arena.duel = fake.duel
        try:
            e = P.PoolEntry("mirror", P.MIRROR, "mirror", 1.0, metric)
            ref = L.RefCache({"culture": 1.0}, 2, 1, 4, 99)
            out = ref.get(e, 0)
        finally:
            L.arena.duel = real
        return fake, ref, out

    def test_winshare_and_margin_need_no_games(self):
        for metric in ("winshare", "margin"):
            fake, ref, out = self.run_with(metric)
            self.assertEqual(fake.calls, [], metric)
            self.assertEqual(ref.games, 0, metric)
            self.assertEqual(out["win"], [0.5] * 4, metric)

    def test_own_and_blend_play_the_reference(self):
        for metric in ("own", "blend"):
            fake, ref, out = self.run_with(metric)
            self.assertEqual(len(fake.calls), 1, metric)
            self.assertEqual(ref.games, 4, metric)
            self.assertEqual(out["culture"], [123.0] * 4, metric)
            # ... and it is scored, not left at the analytic constant
            self.assertAlmostEqual(out["score"][0],
                                   P.score_series(
                                       {"per_game": [1.0],
                                        "per_game_culture": [123.0]},
                                       metric)[0], places=12)


class ArenaCultureSeries(unittest.TestCase):
    def test_duel_reports_per_game_own_culture(self):
        """`own` scoring is impossible without this list, and it is new."""
        from experiments import arena
        res = arena.duel("random", "random", 2, 2, seed0=7, workers=1)
        self.assertIn("per_game_culture", res)
        self.assertEqual(len(res["per_game_culture"]), 2)
        for c, m in zip(res["per_game_culture"], res["per_game_margin"]):
            if c is not None:
                self.assertIsInstance(c, float)
                self.assertIsNotNone(m)
        live = [c for c in res["per_game_culture"] if c is not None]
        if live:
            self.assertAlmostEqual(sum(live) / len(live), res["culture_a"],
                                   places=6)


if __name__ == "__main__":
    unittest.main()
