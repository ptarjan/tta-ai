"""A bare numeric constant must say what KIND of number it is.

The bug this exists to prevent, in one sentence: **a fitted number and a rule
are indistinguishable once they are both a float at module scope.**

The sequence, 2026-07-30.  `engine/bots/weighted.py` carried

    CARDS_PER_ROUND = {2: 6.29, 3: 6.73, 4: 5.71}
    _L_ZERO = {2: 27.1, 3: 28.7, 4: 36.1}
    RIVAL_TAKE_P = 0.25

next to

    AGE_IV_ROUNDS = 2.0
    _SWEEP = {2: 3, 3: 2, 4: 1}

and nothing in the file said that the first three were *fitted on 46 self-play
games of a policy that no longer exists* while the last two are the rulebook.
The owner read the list and said "those all seem weird", which is the correct
reaction to a list you cannot classify by looking at it.  `CARDS_PER_ROUND` was
by then measurably stale -- two card-pricing fixes had raised the bot's take
rate 6x and the constant was still planning against the old one.

So the rule is not "do not write constants".  It is: **every module-scope
number carries a CATEGORY, and the allow-list below is where the project keeps
score.**  Adding a constant means adding a line here and choosing one of:

    rule-derived      it is in the rulebook / RULES_SPEC.  Cite the section.
    numerical guard   it stops a divide-by-zero or an outlier.  Nothing inside
                      the guard is shaped by its value.
    measured          it came out of an instrument.  `where` MUST point at the
                      doc or tool that holds the measurement.
    fitted prior      somebody chose it.  It is a guess with a reason.
    training policy   it defines what the hill climb maximises or how it runs.
    enum-or-sentinel  it is an index, a tag, a cache bound or a counter.

`tests/test_play_rate.py` is the model for this file: a cheap structural check
that runs every time and fails on the *class* of mistake, not on one instance.

See docs/MODEL_CONSTANTS.md.
"""
from __future__ import annotations

import ast
import os
import unittest

from engine import game
from engine.bots import weighted as W

HERE = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))

#: Directories swept.  `tools/`, `tests/`, `analysis/` and `advisor/` are
#: instruments and reports rather than things the bot plays through, so a
#: constant there cannot silently become a model claim.
ROOTS = ("engine", "engine/bots", "experiments")

CATEGORIES = frozenset((
    "rule-derived", "numerical guard", "measured", "fitted prior",
    "training policy", "enum-or-sentinel",
))

#: (module, name) -> (category, note).  For "measured", `note` must name where
#: the measurement lives -- a docs/ file or a tools/ script.
CLASSIFIED = {
    # ---------------------------------------------------------- the rules
    ("engine/actions.py", "ROW_SIZE"): (
        "rule-derived", "RULES_SPEC 2.1: the civil row is 13 spaces"),
    ("engine/actions.py", "ROW_COST"): (
        "rule-derived", "RULES_SPEC 2.3 / appendix 11: the 1/2/3 CA ladder"),
    ("engine/game.py", "START_TECHS"): (
        "rule-derived", "RULES_SPEC 1: the printed player board"),
    ("engine/game.py", "SWEEP"): (
        "rule-derived", "RULES_SPEC 2.1: discard the leftmost 3/2/1"),
    ("engine/bots/weighted.py", "_SWEEP"): (
        "rule-derived", "the same table; test_row_features pins the copy"),
    ("engine/bots/weighted.py", "AGE_IV_ROUNDS"): (
        "rule-derived", "RULES_SPEC 12.3: this round or the next, then end"),
    ("engine/bots/neural_encode.py", "_ROW_COST"): (
        "rule-derived", "a copy of actions.ROW_COST for the encoder"),
    ("engine/bots/neural_encode.py", "_ROW_SIZE"): (
        "rule-derived", "a copy of actions.ROW_SIZE for the encoder"),
    ("engine/bots/neural_encode.py", "MAX_PLAYERS"): (
        "rule-derived", "Through the Ages is a 2-4 player game"),
    ("engine/bots/neural_encode.py", "_CIVIL_DISCARD_SCALE"): (
        "numerical guard",
        "divisor that keeps the civil discard count in 0..1; the true "
        "per-age deck size is player-count-dependent and reading it would "
        "trip the hidden-deck guard, so the net learns the denominator"),
    ("engine/bots/neural_encode.py", "_MIL_DISCARD_SCALE"): (
        "numerical guard", "the same, on the military side"),
    ("engine/bots/book.py", "AGE_IDX"): (
        "rule-derived", "the five ages, in order"),
    # ------------------------------------------------------ numeric guards
    ("engine/bots/weighted.py", "_TURNS_CAP"): (
        "numerical guard", "wonder_turns_to_finish is a ratio; caps infinity"),
    ("engine/bots/weighted.py", "_SCORING_MARGIN_CAP"): (
        "numerical guard", "one outlier event margin must not dominate"),
    ("engine/bots/board_yields.py", "_POP_SENTINEL"): (
        "numerical guard", "stands in for an unaffordable population step"),
    ("engine/bots/board_yields.py", "_DELTA_CACHE_MAX"): (
        "enum-or-sentinel", "cache bound"),
    ("engine/bots/board_yields.py", "_UNIT_CACHE_MAX"): (
        "enum-or-sentinel", "cache bound"),
    ("engine/bots/board_yields.py", "_TECH_CACHE_MAX"): (
        "enum-or-sentinel", "cache bound"),
    ("engine/bots/board_yields.py", "_BUILD_CACHE_MAX"): (
        "enum-or-sentinel", "cache bound"),
    ("engine/game.py", "MOVE_CAP"): (
        "numerical guard", "stops a pathological game running forever"),
    ("engine/statediff.py", "MAX_DIFFS"): (
        "numerical guard", "truncates a diff report"),
    ("engine/bots/neural_net.py", "MARGIN_NORM"): (
        "numerical guard",
        "linear normaliser on the value net's regression target; it cancels, "
        "and it is NOT hillclimb_pool.LEAD_SCALE"),
    # ------------------------------------------------------------ measured
    ("engine/bots/board_yields.py", "FREE_POP_UTIL"): (
        "measured",
        "tools/free_pop_rate.py, 316-318 player-turns of 2p self-play on two "
        "vectors, both landing on 0.17; calibrated against a per-position "
        "replayed truth that contains no constant (docs/MODEL_CONSTANTS.md 9)"),
    ("engine/bots/weighted.py", "_TAKE_PRIOR"): (
        "fitted prior",
        "opening-rounds prior only; tools/deal_rate.py measures the live "
        "rate and docs/MODEL_CONSTANTS.md section 2 holds the numbers"),
    ("engine/bots/weighted.py", "_TAKE_PRIOR_W"): (
        "fitted prior", "shrinkage weight in pseudo-replenishes; see above"),
    # ------------------------------------------------------- fitted priors
    ("engine/bots/weighted.py", "PACT_OFFER_CREDIT"): (
        "fitted prior", "docs/COMBAT_AUDIT.md fix 2; docs/OPEN_ITEMS.md"),
    ("engine/bots/weighted.py", "RIVAL_TAKE_SHARE"): (
        "fitted prior",
        "the default for the `rival_take_share` WEIGHT; everything else in "
        "rival_take_p is read off the rival's open board"),
    ("engine/bots/weighted.py", "RIVAL_TAKE_P"): (
        "fitted prior", "RETIRED; kept for LEGACY_RIVAL_TAKE and the A/B"),
    ("engine/bots/weighted.py", "CARDS_PER_ROUND"): (
        "fitted prior", "RETIRED; kept for LEGACY_DEAL_RATE and the A/B"),
    ("engine/bots/weighted.py", "_L_ZERO"): (
        "fitted prior", "RETIRED; kept for LEGACY_LATENESS and the A/B"),
    ("engine/bots/weighted.py", "_L_ONE"): (
        "fitted prior", "RETIRED; kept for LEGACY_LATENESS and the A/B"),
    ("engine/bots/weighted.py", "BASE_WEIGHTS"): (
        "fitted prior", "the hill climb's starting vector"),
    ("engine/bots/weighted.py", "PHASE_WEIGHTS"): (
        "fitted prior", "the hill climb's starting phase pairs"),
    ("engine/bots/__init__.py", "WEIGHTS"): (
        "fitted prior",
        "GreedyBot's, FROZEN ON PURPOSE: it is the fingerprint control and "
        "must not be synced with BASE_WEIGHTS (see the comment there)"),
    ("engine/bots/book.py", "LEADER_RANK"): (
        "fitted prior", "hand-written expert opinion; docs/EXPERT_STRATEGY.md"),
    ("engine/bots/book.py", "WONDER_RANK"): (
        "fitted prior", "hand-written expert opinion; docs/EXPERT_STRATEGY.md"),
    ("engine/bots/book.py", "SPECIAL_RANK"): (
        "fitted prior", "hand-written expert opinion; docs/EXPERT_STRATEGY.md"),
    ("engine/bots/book.py", "TACTIC_RANK"): (
        "fitted prior", "hand-written expert opinion; docs/EXPERT_STRATEGY.md"),
    ("engine/bots/book.py", "V2_TUNABLES"): (
        "fitted prior",
        "plumbed as a parameter but no caller overrides it, so frozen in "
        "practice; docs/EXPERT_STRATEGY.md disagreements table"),
    ("engine/bots/book.py", "V2_LEADER_RANK"): (
        "fitted prior", "tournament CA-spend; docs/EXPERT_STRATEGY.md"),
    ("engine/bots/book.py", "V2_HOMER"): (
        "fitted prior", "tournament CA-spend; docs/EXPERT_STRATEGY.md"),
    ("engine/bots/book.py", "V2_WONDER_RANK"): (
        "fitted prior", "tournament CA-spend; docs/EXPERT_STRATEGY.md"),
    ("engine/bots/book.py", "V2_PRICE_LADDER"): (
        "fitted prior", "convex row-price penalty; docs/EXPERT_STRATEGY.md"),
    ("experiments/analyze_weights.py", "FLOOR"): (
        "fitted prior", "reporting threshold for 'this weight moved'"),
    ("experiments/analyze_weights.py", "NOTABLE"): (
        "fitted prior", "reporting threshold for 'this weight moved a lot'"),
    ("experiments/proxy_check.py", "MARGIN_MIN"): (
        "fitted prior", "docs/PROXY_GUARDRAIL.md"),
    ("experiments/proxy_check.py", "MARGIN_RESOLUTION"): (
        "fitted prior", "docs/PROXY_GUARDRAIL.md"),
    # ------------------------------------------------------ training policy
    ("experiments/hillclimb_pool.py", "DEFAULT_TIER_WEIGHTS"): (
        "training policy", "docs/LEAGUE_POOL.md; LIVE in the running arms"),
    ("experiments/hillclimb_pool.py", "LEGACY_TIER_WEIGHTS"): (
        "training policy", "reproduction fixture for pre-2026-07-27 runs"),
    ("experiments/hillclimb_pool.py", "SAT_LO"): (
        "training policy", "pool saturation band; docs/LEAGUE_POOL.md"),
    ("experiments/hillclimb_pool.py", "SAT_HI"): (
        "training policy", "pool saturation band; docs/LEAGUE_POOL.md"),
    ("experiments/hillclimb_pool.py", "SAT_FLOOR"): (
        "training policy", "pool saturation band; docs/LEAGUE_POOL.md"),
    # 2026-07-30: MARGIN_SCALE became LEAD_SCALE, and CULTURE_SCALE /
    # CULTURE_CENTRE were DELETED rather than re-fitted -- they described "what
    # a typical game scores", which is the thing that went stale.  The objective
    # is now centred on the win/lose boundary, which is rule-derived and needs
    # no constant at all.  docs/LEAGUE_OBJECTIVE.md.
    ("experiments/hillclimb_pool.py", "FALLBACK_LEAD_SCALE"): (
        "numerical guard",
        "only reached when a caller has no player count at all; the base game "
        "has exactly three, so this is a caller bug, not a configuration"),
    ("experiments/hillclimb_pool.py", "LEAD_SCALE"): (
        "measured",
        "PER PLAYER COUNT (2p 145, 3p 115, 4p 135) = 2.5x the sd of the "
        "per-seat culture lead over the 1,011-game human BGO corpus.  "
        "Re-derive: python3 tools/objective_relog.py --derive-scale.  The "
        "corpus is EXTERNAL and FIXED, so unlike the CULTURE_CENTRE it "
        "replaced it cannot go stale as the bot improves.  "
        "docs/LEAGUE_OBJECTIVE.md section 5"),
    ("experiments/hillclimb_pool.py", "DEFAULT_ALPHA"): (
        "training policy", "docs/LEAGUE_OBJECTIVE.md. OWNER'S CALL"),
    ("experiments/hillclimb_league.py", "HIGH_DEATH_RATE"): (
        "training policy", "restart trigger; docs/LEAGUE_TRAINING.md"),
    ("experiments/hillclimb_league.py", "INIT_OVERRIDES"): (
        "training policy", "4p starts with hand_potential off"),
    ("experiments/hillclimb.py", "LEAGUE_KEEP"): (
        "training policy", "how many archived vectors the ladder keeps"),
    ("experiments/arena.py", "DEGENERATE_MATCH_FRACTION"): (
        "training policy", "degenerate-champion guard threshold"),
    ("experiments/gpu_guard.py", "POLL"): (
        "training policy", "seconds between GPU polls"),
    ("experiments/gpu_guard.py", "CONFIRM"): (
        "training policy", "consecutive polls before acting"),
    ("experiments/proxy_check.py", "BUDGET"): (
        "training policy", "cpu budget per player count"),
    # ----------------------------------------------------- enums/sentinels
    ("engine/interact.py", "WAR_TECH_SCIENCE_IDX"): (
        "enum-or-sentinel", "an OPTION INDEX, not an amount of science"),
    ("engine/journal.py", "_ATTR"): ("enum-or-sentinel", "undo record tag"),
    ("engine/journal.py", "_LIST"): ("enum-or-sentinel", "undo record tag"),
    ("engine/journal.py", "_DICT"): ("enum-or-sentinel", "undo record tag"),
    ("engine/journal.py", "_SET"): ("enum-or-sentinel", "undo record tag"),
    ("engine/perf_check.py", "PLAN_WIDTH"): (
        "enum-or-sentinel", "the fingerprint's PlanBot width; part of the arm"),
    ("engine/bots/board_yields.py", "_GAIN"): ("enum-or-sentinel", "tuple slot"),
    ("engine/bots/board_yields.py", "_COST"): ("enum-or-sentinel", "tuple slot"),
    ("engine/bots/weighted.py", "_Y_GAIN"): ("enum-or-sentinel", "yield kind"),
    ("engine/bots/weighted.py", "_Y_COST"): ("enum-or-sentinel", "yield kind"),
    ("engine/bots/weighted.py", "_Y_RATE"): ("enum-or-sentinel", "yield kind"),
    ("engine/bots/weighted.py", "_Y_UNIT"): ("enum-or-sentinel", "yield kind"),
    ("engine/bots/weighted.py", "_Y_TERR"): ("enum-or-sentinel", "yield kind"),
    ("engine/bots/weighted.py", "_Y_BONUS"): ("enum-or-sentinel", "yield kind"),
    ("engine/bots/neural_encode.py", "_PLAYER_SCALARS"): (
        "enum-or-sentinel", "encoder vector width"),
    ("engine/bots/neural_plan.py", "_NO_CTX"): (
        "enum-or-sentinel", "empty rival context"),
    ("engine/bots/plan.py", "_NO_CTX"): (
        "enum-or-sentinel", "empty rival context"),
    ("engine/bots/quiescent.py", "_NO_CTX"): (
        "enum-or-sentinel", "empty rival context"),
    ("engine/bots/pending.py", "_CALLS"): ("enum-or-sentinel", "counter"),
    ("engine/bots/pending.py", "_QUIET_CALLS"): ("enum-or-sentinel", "counter"),
    ("engine/bots/pending.py", "_DET_CALLS"): ("enum-or-sentinel", "counter"),
    ("experiments/paired_stats.py", "_T95"): (
        "rule-derived", "Student's t, two-sided 95%: a textbook table"),
    ("experiments/paired_stats.py", "Z95"): (
        "rule-derived", "the standard normal 97.5th percentile"),
    ("experiments/paired_stats.py", "_CHI2_95"): (
        "rule-derived", "chi-squared 95%: a textbook table"),
    ("experiments/proxy_check.py", "H2H_SEED"): (
        "enum-or-sentinel", "an RNG seed"),
    ("experiments/proxy_check.py", "ANCHOR_SEED"): (
        "enum-or-sentinel", "an RNG seed"),
}


def _is_numeric(node):
    """A literal number, or a container whose every leaf is one."""
    if isinstance(node, ast.Constant):
        return isinstance(node.value, (int, float)) \
            and not isinstance(node.value, bool)
    if isinstance(node, (ast.Tuple, ast.List, ast.Set)):
        return bool(node.elts) and all(_is_numeric(e) for e in node.elts)
    if isinstance(node, ast.Dict):
        return bool(node.values) and all(_is_numeric(v) for v in node.values)
    if isinstance(node, ast.UnaryOp):
        return _is_numeric(node.operand)
    return False


def _module_constants():
    """(relative path, name, lineno) for every module-scope numeric literal."""
    out = []
    for root in ROOTS:
        d = os.path.join(HERE, root)
        for fn in sorted(os.listdir(d)):
            if not fn.endswith(".py"):
                continue
            rel = f"{root}/{fn}"
            with open(os.path.join(d, fn), encoding="utf-8") as fh:
                tree = ast.parse(fh.read())
            for node in tree.body:
                if isinstance(node, ast.Assign):
                    targets = node.targets
                elif isinstance(node, ast.AnnAssign):
                    targets = [node.target]
                else:
                    continue
                if node.value is None or not _is_numeric(node.value):
                    continue
                for t in targets:
                    if isinstance(t, ast.Name):
                        out.append((rel, t.id, node.lineno))
    return out


class ConstantsAreClassified(unittest.TestCase):
    """The standing check.  Cheap, structural, always runs."""

    def test_every_module_constant_has_a_category(self):
        missing = [(m, n, ln) for m, n, ln in _module_constants()
                   if (m, n) not in CLASSIFIED]
        self.assertEqual(missing, [], "\n".join(
            [f"{len(missing)} unclassified module-scope constant(s). "
             "Add each to CLASSIFIED in tests/test_model_constants.py with "
             f"one of: {sorted(CATEGORIES)}."]
            + [f"    (\"{m}\", \"{n}\"),   # {m}:{ln}" for m, n, ln in missing]))

    def test_every_category_is_one_of_the_six(self):
        for key, (cat, note) in sorted(CLASSIFIED.items()):
            self.assertIn(cat, CATEGORIES, key)
            self.assertTrue(note.strip(), key)

    def test_measured_constants_point_at_their_measurement(self):
        """'measured' is the category that makes a claim about the world, so
        it is the one that has to be checkable."""
        for key, (cat, note) in sorted(CLASSIFIED.items()):
            if cat != "measured":
                continue
            self.assertTrue("docs/" in note or "tools/" in note,
                            f"{key}: a 'measured' constant must name where "
                            f"the measurement lives; got {note!r}")

    def test_the_allow_list_has_not_rotted(self):
        """A constant that was deleted or renamed must leave the list."""
        live = {(m, n) for m, n, _ in _module_constants()}
        stale = sorted(k for k in CLASSIFIED if k not in live)
        self.assertEqual(stale, [], "stale entries in CLASSIFIED")

    def test_the_two_margin_scales_are_not_the_same_knob(self):
        """They used to share a name and do not share a job.  See the comment
        on `neural_net.MARGIN_NORM`."""
        from engine.bots import neural_net
        import experiments.hillclimb_pool as P
        self.assertFalse(hasattr(neural_net, "MARGIN_SCALE"))
        self.assertNotIn(neural_net.MARGIN_NORM, set(P.LEAD_SCALE.values()))


class LatenessIsBounded(unittest.TestCase):
    """`1 - L` must never change sign.  docs/CULTURE_GAP.md section 8d
    measured what happens when it does: the 4p champion to 19.9% against a
    25% null, the 3p champion to 13.6% against 33.3%."""

    def _adversarial(self):
        """States a real game cannot produce, which is the point."""
        out = []
        for n in (2, 3, 4):
            for age in ("A", "I", "II", "III", "IV"):
                for deck in (0, 1, 44, 400, 10_000):
                    for turn in (-9, 0, 1, 7, 60, 10_000):
                        st = game.new_game(n, 5)
                        st.age_civil = st.age_military = age
                        st.civil_deck = ["x"] * deck
                        st.turn = turn
                        st.round = max(1, turn // max(1, n))
                        out.append(st)
        return out

    def test_lateness_never_leaves_the_unit_interval(self):
        for st in self._adversarial():
            lv = W.lateness(st)
            self.assertTrue(0.0 <= lv <= 1.0,
                            f"L={lv} at {st.age_civil}/{len(st.civil_deck)}"
                            f"/turn {st.turn}")

    def test_the_legacy_gauge_is_bounded_too(self):
        W.LEGACY_LATENESS = W.LEGACY_DEAL_RATE = True
        try:
            for st in self._adversarial():
                self.assertTrue(0.0 <= W.lateness(st) <= 1.0)
        finally:
            W.LEGACY_LATENESS = W.LEGACY_DEAL_RATE = False

    def test_rounds_left_is_always_at_least_one_round(self):
        for st in self._adversarial():
            self.assertGreaterEqual(W.rounds_left(st), 1.0)

    HATCH_VARS = ("TTA_LEGACY_DEAL_RATE", "TTA_LEGACY_LATENESS",
                  "TTA_LEGACY_ROW_TAKE")

    def test_the_hatches_are_off_in_the_shipped_module(self):
        """They are A/B switches, not configuration.  A tree that ships with
        one on is a tree whose fingerprint means something else."""
        on = [v for v in self.HATCH_VARS if os.environ.get(v)]
        if on:
            self.skipTest(f"deliberately running a legacy arm: {on}")
        self.assertFalse(W.LEGACY_DEAL_RATE)
        self.assertFalse(W.LEGACY_LATENESS)
        self.assertFalse(W.LEGACY_RIVAL_TAKE)

    def test_the_weight_hatch_is_not_part_of_the_trained_vector(self):
        """Like `horizon_age`: an A/B key the trainer must never emit, never
        perturb and never guard."""
        self.assertNotIn("horizon_legacy", W.DEFAULT_WEIGHTS)
        self.assertNotIn("horizon_age", W.DEFAULT_WEIGHTS)


if __name__ == "__main__":
    unittest.main()
