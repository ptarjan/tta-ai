"""What the league's accept decision maximises, and the pool it maximises it on.

These pin the 2026-07-30 objective change (docs/LEAGUE_OBJECTIVE.md): the
culture term is the LEAD OVER THE BEST OPPONENT, whose zero is the win/lose
boundary, replacing the absolute own-culture term whose zero point had to be
guessed and went stale.

Three things here are worth more than the rest and are the reason the file
exists in this shape:

1. `LeadShare::test_zero_lead_is_exactly_the_win_boundary` checks the claim
   that makes the design honest -- that the objective's centre is a fact about
   Through the Ages -- against the ENGINE, not against a comment.
2. `NoFittedCentre` is a regression pin against the failure mode that produced
   this change: a constant fitted to one month's play steering the next.
3. `PoolMetric::test_one_metric_for_every_tier` keeps the aggregate in one
   unit.  A weighted mean over rows measured in different units is not a
   number, and until 2026-07-30 the gate tiers were scored on something
   different from everything else.
"""
import math
import os
import sys
import unittest

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

from experiments import hillclimb_pool as P
from experiments import hillclimb_league as L


class LeadShare(unittest.TestCase):
    def test_zero_maps_to_half_with_no_fitted_constant(self):
        """The centre is the win boundary, so it is 0 at every scale."""
        for scale in (1.0, 12.0, 120.0, 1200.0):
            self.assertEqual(P.lead_share(0.0, scale), 0.5, scale)

    def test_symmetric_about_zero(self):
        """`lead_share(-m) == 1 - lead_share(m)` EXACTLY, every m, every scale.

        Losing by 30 must be worth as much below the null as winning by 30 is
        above it.  This identity is also the structural check that no fitted
        centre exists: an offset c != 0 breaks it on the first sample.
        """
        for scale in (30.0, 120.0, 500.0):
            for m in (0.0, 0.5, 1.0, 7.0, 30.0, 119.0, 400.0, 1e5):
                a = P.lead_share(m, scale)
                b = P.lead_share(-m, scale)
                self.assertAlmostEqual(a + b, 1.0, places=15, msg=(scale, m))

    def test_bounded_on_adversarial_inputs(self):
        """Never outside [0, 1], never NaN, for anything at all.

        The tier weighting and the accept/veto bounds all assume a paired edge
        in [-1, +1]; an unbounded score would let ONE blowout game carry an
        accept, which is the job the squash is really doing.

        Mathematically the range is the OPEN (0, 1) -- tanh maps R onto
        (-1, 1) -- and that strictness is asserted below over every lead the
        game can physically produce.  At |lead| > ~2200 a float double rounds
        tanh to exactly 1.0 and the endpoint is attained; that is a property
        of IEEE754, not of the objective, and it cannot occur because final
        culture is bounded far below it.
        """
        extreme = [1e9, -1e9, 1e18, -1e18, 1e300, -1e300,
                   float("inf"), float("-inf")]
        for scale in (1e-6, 1.0, 120.0, 1e9):
            for m in extreme:
                v = P.lead_share(m, scale)
                self.assertFalse(math.isnan(v), (scale, m))
                self.assertTrue(0.0 <= v <= 1.0, (scale, m, v))
        # Strictly inside the open interval across the whole physically
        # possible range: no real game ends more than a few hundred culture
        # apart, and 2000 is an order of magnitude past that.
        for m in range(-2000, 2001, 25):
            v = P.lead_share(float(m))
            self.assertTrue(0.0 < v < 1.0, (m, v))

    def test_strictly_monotone(self):
        prev = -1.0
        for m in range(-400, 401, 5):
            v = P.lead_share(float(m))
            self.assertGreater(v, prev, m)
            prev = v

    def test_none_passes_through(self):
        self.assertIsNone(P.lead_share(None))

    def test_zero_lead_is_exactly_the_win_boundary(self):
        """The load-bearing claim, checked against the engine.

        `arena._play` derives the win share from `max(sc)` and the lead from
        `max(others)`.  They are the same maximum over the same list, so
        `lead >= 0` and "A won or tied" must be the same statement about every
        game.  That is what lets the objective be centred on 0 without anyone
        choosing 0.
        """
        from experiments import arena
        for players in (2, 3):
            res = arena.duel("random", "random", players, players * 2,
                             seed0=11, workers=1)
            live = [(w, m) for w, m in zip(res["per_game"],
                                           res["per_game_lead"])
                    if w is not None]
            self.assertTrue(live, players)
            for w, m in live:
                self.assertEqual(m >= 0.0, w > 0.0,
                                 f"{players}p: lead {m} vs win share {w}")

    def test_the_scale_is_the_only_free_parameter(self):
        """`ScoreParams` carries two numbers: a scale and the blend weight.

        Every other constant the objective used to need described "roughly
        what a game scores", which is what went stale.  If a third slot
        appears here, the question to ask is which rule it comes from.
        """
        self.assertEqual(set(P.ScoreParams.__slots__), {"lead_scale", "alpha"})


class NoFittedCentre(unittest.TestCase):
    """No 'typical score' constant may exist anywhere in the objective path.

    The previous objective scored ABSOLUTE own culture and therefore needed a
    `CULTURE_CENTRE` fitted to observed scores.  It was set to 100 in July
    2026 and by the end of that month candidate own-culture medians had moved
    to 108.8 / 122.1 / 134.4 at 2p/3p/4p with champions at 120.8 / 144.1 /
    160.6 -- a number fitted to yesterday's policy steering today's, and one
    that would go stale again on every improvement.  This class is the pin.
    """

    def test_the_named_constants_are_gone(self):
        for name in ("CULTURE_CENTRE", "CULTURE_SCALE", "own_share",
                     "margin_share", "MARGIN_SCALE", "DEFAULT_MARGIN_TIERS"):
            self.assertFalse(hasattr(P, name),
                             f"{name} is back in the objective module")
        for name in ("centre", "center"):
            self.assertFalse([a for a in P.ScoreParams.__slots__
                              if name in a.lower()])

    def test_the_objective_has_no_typical_score_constant(self):
        """Structural, not textual: the score path is odd-symmetric about 0.

        Any fitted centre c enters as `f(x - c)`.  For every metric in the
        default objective path, scoring a game and scoring its exact mirror
        image (the same game with the result reversed) must sum to 1.  A
        c != 0 breaks that for every sample, whatever it is named and however
        it is spelled, so this catches a re-introduction that a grep would
        not.
        """
        leads = [0.0, 3.0, 17.5, 60.0, 140.0, 900.0]
        for metric in ("lead", "blend"):
            for m in leads:
                fwd = {"per_game": [1.0], "per_game_lead": [m]}
                rev = {"per_game": [0.0], "per_game_lead": [-m]}
                a = P.score_series(fwd, metric)[0]
                b = P.score_series(rev, metric)[0]
                self.assertAlmostEqual(a + b, 1.0, places=12,
                                       msg=f"{metric} at lead {m}")


class ScoreSeries(unittest.TestCase):
    RES = {
        "per_game": [1.0, 0.0, 0.5, None],
        "per_game_lead": [40.0, -40.0, 0.0, None],
        "per_game_margin": [55.0, -30.0, 10.0, None],
        "per_game_culture": [150.0, 50.0, 100.0, None],
    }

    def test_winshare_is_the_raw_list(self):
        self.assertEqual(P.score_series(self.RES, "winshare"),
                         self.RES["per_game"])

    def test_lead_uses_the_best_opponent_not_the_mean(self):
        """`per_game_margin` is present and deliberately NOT what is scored."""
        got = P.score_series(self.RES, "lead")
        want = [0.5 * (1 + math.tanh(m / P.LEAD_SCALE))
                if m is not None else None
                for m in self.RES["per_game_lead"]]
        self.assertEqual(got, want)
        self.assertEqual(got[2], 0.5)                  # lead 0 -> the null
        self.assertIsNone(got[3])

    def test_blend_is_a_convex_combination(self):
        p = P.ScoreParams(alpha=0.25)
        lead = P.score_series(self.RES, "lead", p)
        win = self.RES["per_game"]
        got = P.score_series(self.RES, "blend", p)
        for g, o, w in zip(got[:3], lead[:3], win[:3]):
            self.assertAlmostEqual(g, 0.75 * o + 0.25 * w, places=12)
            self.assertTrue(0.0 <= g <= 1.0)
        self.assertIsNone(got[3])

    def test_blend_endpoints_are_the_pure_objectives(self):
        self.assertEqual(P.score_series(self.RES, "blend",
                                        P.ScoreParams(alpha=0.0))[:3],
                         P.score_series(self.RES, "lead")[:3])
        self.assertEqual(P.score_series(self.RES, "blend",
                                        P.ScoreParams(alpha=1.0))[:3],
                         self.RES["per_game"][:3])

    def test_blend_is_bounded_on_adversarial_series(self):
        res = {"per_game": [1.0, 0.0, 1.0, 0.0],
               "per_game_lead": [1e9, -1e9, -1e9, 1e9]}
        for a in (0.0, 0.15, 0.5, 1.0):
            got = P.score_series(res, "blend", P.ScoreParams(alpha=a))
            self.assertEqual(len(got), 4, a)     # not vacuous
            for v in got:
                self.assertTrue(0.0 <= v <= 1.0, (a, v))

    def test_beating_up_a_non_contender_does_not_score(self):
        """The specific 3p/4p failure of the margin over the MEAN.

        Leader 180, us 150, trailing seat 60.  Crushing the trailing seat down
        to 20 raises our margin over the mean by 20 and does nothing whatever
        for winning -- we are still 30 behind the leader.  Scoring the lead
        over the BEST seat is flat in that move by construction, because
        `max(others)` does not notice it.
        """
        before = {"per_game": [0.0], "per_game_lead": [150.0 - 180.0],
                  "per_game_margin": [150.0 - 120.0]}
        after = {"per_game": [0.0], "per_game_lead": [150.0 - 180.0],
                 "per_game_margin": [150.0 - 100.0]}
        self.assertEqual(P.score_series(after, "lead")[0],
                         P.score_series(before, "lead")[0])
        # ... and the mean-based quantity really does move, so this is a
        # difference between the two choices and not a tautology.
        self.assertGreater(after["per_game_margin"][0],
                           before["per_game_margin"][0])

    def test_taking_from_the_leader_is_paid_at_twice_producing(self):
        """Stated as a test because the sign of this was once called a bug.

        Two candidates on the same board: one PRODUCES 20 culture, the other
        TAKES 20 from the seat that is leading.  The lead moves 20 for the
        producer and 40 for the taker, and that factor of two is CORRECT with
        respect to winning -- taking 20 off the leader really does close twice
        as much of the gap as making 20.  docs/LEAGUE_OBJECTIVE.md section 3
        is the history of why this used to be scored the other way.
        """
        base = {"per_game": [0.0], "per_game_lead": [-60.0]}
        produce = {"per_game": [0.0], "per_game_lead": [-40.0]}
        take = {"per_game": [0.0], "per_game_lead": [-20.0]}
        d_produce = (P.score_series(produce, "lead")[0]
                     - P.score_series(base, "lead")[0])
        d_take = (P.score_series(take, "lead")[0]
                  - P.score_series(base, "lead")[0])
        self.assertGreater(d_take, 1.8 * d_produce)


class PoolMetric(unittest.TestCase):
    def build(self, **kw):
        return P.build_pool(2, ladder_dirs=(), past_k=0,
                            log=lambda *_a: None, **kw)

    def test_one_metric_for_every_tier(self):
        """The aggregate is a weighted mean, so it must be in one unit.

        Before 2026-07-30 the gate tiers were scored on culture margin and
        everything else on win share, which makes both the aggregate and the
        tier weights meaningless.  There is no per-tier override any more.
        """
        for metric in ("lead", "blend", "winshare"):
            pool = self.build(metric=metric)
            for e in pool.entries:
                self.assertEqual(e.metric, metric, e.label)

    def test_the_default_is_the_blend(self):
        for e in self.build().entries:
            self.assertEqual(e.metric, "blend", e.label)

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
    """The mirror shortcut is valid for win share and for nothing else."""

    class FakeArena:
        def __init__(self):
            self.calls = []

        def duel(self, a, b, players, games, seed0=0, workers=None, **kw):
            self.calls.append((players, games, seed0))
            return {"per_game": [1.0] * games,
                    "per_game_lead": [7.0] * games,
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

    def test_winshare_needs_no_games(self):
        fake, ref, out = self.run_with("winshare")
        self.assertEqual(fake.calls, [])
        self.assertEqual(ref.games, 0)
        self.assertEqual(out["win"], [0.5] * 4)

    def test_lead_and_blend_play_the_reference(self):
        """A mirror's mean LEAD is not 0, unlike its mean margin.

        Over a seat rotation of one identical policy the leads sum to
        `sum(sc) - sum(max over the others)`, which is strictly negative
        unless every seat ties: scores 10/5/3 give leads +5, -5, -7.  Taking
        the analytic shortcut here would silently score the champion's mirror
        row at a constant it does not have.
        """
        for metric in ("lead", "blend"):
            fake, ref, out = self.run_with(metric)
            self.assertEqual(len(fake.calls), 1, metric)
            self.assertEqual(ref.games, 4, metric)
            self.assertEqual(out["lead"], [7.0] * 4, metric)
            # ... and it is scored, not left at the analytic constant
            self.assertAlmostEqual(out["score"][0],
                                   P.score_series(
                                       {"per_game": [1.0],
                                        "per_game_lead": [7.0]},
                                       metric)[0], places=12)

    def test_the_analytic_list_does_not_contain_the_lead(self):
        self.assertEqual(P.ANALYTIC_MIRROR_METRICS, ("winshare",))

    def test_a_mirror_rotation_does_not_have_zero_mean_lead(self):
        """The arithmetic behind the previous test, without any games."""
        sc = [10.0, 5.0, 3.0]
        leads = [sc[s] - max(v for i, v in enumerate(sc) if i != s)
                 for s in range(3)]
        self.assertEqual(leads, [5.0, -5.0, -7.0])
        self.assertNotEqual(sum(leads), 0.0)
        # while the margin over the MEAN does cancel, which is the trap
        margins = [sc[s] - sum(v for i, v in enumerate(sc) if i != s) / 2.0
                   for s in range(3)]
        self.assertAlmostEqual(sum(margins), 0.0, places=12)


class ArenaLeadSeries(unittest.TestCase):
    def test_duel_reports_per_game_lead_over_the_best_opponent(self):
        """Lead scoring is impossible without this list, and it is new."""
        from experiments import arena
        res = arena.duel("random", "random", 3, 3, seed0=7, workers=1)
        self.assertIn("per_game_lead", res)
        self.assertEqual(len(res["per_game_lead"]), 3)
        live = [(c, m, ld) for c, m, ld in zip(res["per_game_culture"],
                                               res["per_game_margin"],
                                               res["per_game_lead"])
                if c is not None]
        self.assertTrue(live)
        for c, m, ld in live:
            self.assertIsInstance(ld, float)
            # the best opponent is never below the mean opponent, so the lead
            # is never above the margin -- and at 3p+ they are usually apart
            self.assertLessEqual(ld, m + 1e-9)
        cultures = [c for c, _m, _l in live]
        self.assertAlmostEqual(sum(cultures) / len(cultures), res["culture_a"],
                               places=6)
        leads = [ld for _c, _m, ld in live]
        self.assertAlmostEqual(sum(leads) / len(leads), res["lead"], places=6)


class AcceptLoop(unittest.TestCase):
    """Drive `score_candidate` end to end on synthetic per-game series.

    This is the smoke test for the whole accept path under the new metric --
    the trainer's aggregate, its one-sided bound and its gate veto -- without
    playing games.  It is a permanent test rather than a one-off run because a
    training run that "looked sane once" is not a check anyone can repeat.
    """

    def duels(self, cand_lead, champ_lead, cand_win=0.5, champ_win=0.5):
        """A fake arena where the candidate and the champion post fixed leads."""
        champ = {"culture": 1.0}

        def duel(a, b, players, games, seed0=0, workers=None, **kw):
            mine = a is not champ
            lead = cand_lead if mine else champ_lead
            win = cand_win if mine else champ_win
            return {"per_game": [win] * games,
                    "per_game_lead": [lead] * games,
                    "per_game_margin": [lead] * games,
                    "per_game_culture": [100.0 + lead] * games}
        return champ, duel

    def score(self, cand_lead, champ_lead, **kw):
        champ, duel = self.duels(cand_lead, champ_lead, **kw)
        entries = [P.PoolEntry("book", "book", "book", 1.0, "blend"),
                   P.PoolEntry("hall:x", {"h": 1}, "hall", 1.0, "blend")]
        real = L.arena.duel
        L.arena.duel = duel
        try:
            ref = L.RefCache(champ, 2, 1, 12, 5)
            cand = {"culture": 2.0}
            return L.score_candidate(cand, entries, ref, 1.2816, 1, 2, 1.0,
                                     ("book",))
        finally:
            L.arena.duel = real

    def test_a_candidate_that_leads_by_more_is_accepted(self):
        m, se, lo, per, games, veto = self.score(+30.0, -10.0)
        self.assertGreater(m, 0.0)
        self.assertGreater(lo, 0.0, "a clearly better candidate must clear "
                                    "the accept bound")
        self.assertEqual(veto, [])
        self.assertGreater(games, 0)
        for r in per.values():
            self.assertEqual(r["lead"], 30.0)
            self.assertEqual(r["champ_lead"], -10.0)

    def test_a_candidate_that_leads_by_less_is_rejected_and_vetoed(self):
        m, se, lo, per, games, veto = self.score(-30.0, +10.0)
        self.assertLess(m, 0.0)
        self.assertLess(lo, 0.0)
        self.assertIn("book", veto, "a gate opponent it is clearly worse "
                                    "against must veto")

    def test_an_identical_candidate_is_an_exact_null(self):
        """The invariant the whole paired design rests on."""
        m, se, lo, per, games, veto = self.score(+17.0, +17.0)
        self.assertEqual(m, 0.0)
        self.assertEqual(veto, [])
        for r in per.values():
            self.assertEqual(r["edge"], 0.0)

    def test_the_aggregate_stays_inside_plus_or_minus_one(self):
        """Bounded scores mean a bounded paired edge, at any lead."""
        for cl, chl in ((1e6, -1e6), (-1e6, 1e6), (400.0, -400.0)):
            m, _se, _lo, _per, _g, _v = self.score(cl, chl)
            self.assertTrue(-1.0 <= m <= 1.0, (cl, chl, m))



if __name__ == "__main__":
    unittest.main()
