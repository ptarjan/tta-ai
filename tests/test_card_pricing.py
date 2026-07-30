"""Every printed effect is either priced or explicitly written off.

The bug this exists to prevent: `_EFF_TO_FEATURE` in engine/bots/weighted.py
had no entry for `culture` or `science`, so ten cards' culture production and
two cards' science production were silently dropped by `_card_yields` -- for
most of the project's life, on the win condition of the game.  Nothing failed.
Seven of the sixteen wonders, including Library of Alexandria and Universitas
Carolina, priced out at nothing beyond "it is a wonder".

`test_every_effect_key_is_accounted_for` turns that class of omission into a
test failure.  It does NOT require the blind spot to be empty -- most of what
is on a Through the Ages card genuinely cannot be priced by a board-independent
per-card table.  It requires the blind spot to be WRITTEN DOWN, so that adding
a card, or noticing a key that should have been mapped, is a visible event.

See docs/CARD_BLINDNESS.md for the census these numbers come from.
"""
import unittest

from engine import cards as C
from engine.bots import board_yields as BY, weighted as W


# The blocks the coverage walk opens.
#
# `immediateEffects` / `permanentEffects` were NOT here originally, and that
# omission is why the census reported all 12 territories as "0 dropped keys,
# zero visible gain" -- a territory's `effects` block is `{}` and its entire
# content lives in these two.  A test that only looks where the values already
# are cannot report a value in the wrong place, which is the same failure as
# the one the file exists to prevent, one level out.
_BLOCKS = ("production", "effects", "immediateEffects", "permanentEffects")


def _blocks(card):
    for block in _BLOCKS:
        yield block, (card.get(block) or {})


def _all_priced_keys():
    """Every effect key any pricing path in the evaluator reads.

    One definition, because three separate tests need it and they were
    starting to drift apart as each lane added its own table -- which is the
    same failure mode this file exists to prevent, in the file itself.
    """
    return (set(W._PROD_TO_FEATURE) | set(W._EFF_TO_FEATURE)
            | set(W._EFF_SPECIAL) | set(W._EFF_CHOICE)
            | set(W._TERR_TO_FEATURE) | set(BY.BOARD_PRICED))


# ------------------------------------------------- the top-level card fields
#
# The second place a printed value can hide, and the one that cost the ten
# unit cards: a military unit's yield is a TOP-LEVEL `strength`, not
# `production: {"strength": n}`, so no block walk could ever see it.  Every
# top-level key on every card is either named below as carrying a priced
# value, or written off with a reason, on exactly the same terms as
# DELIBERATELY_UNPRICED.
TOP_LEVEL_PRICED = {
    # read by `_card_yields`
    "strength": "unit per-worker strength / tactic per-army bonus",
    "techCost": "priced as a science cost",
    "buildCost": "priced as a resource cost",
    "stages": "wonder: priced as the summed resource cost",
    "composition": "tactic: what fills an army (engine/effects.tactic_outlook)",
    "obsoleteStrength": "tactic: the outdated per-army bonus",
}

TOP_LEVEL_UNPRICED = {
    # --- identity / bookkeeping, no value in them at all
    "name": "card identity", "baseName": "printed name before disambiguation",
    "age": "priced through the age level, not as a yield",
    "type": "priced through the type, not as a yield",
    "deck": "which deck the card comes from",
    "count": "copies of the card per player count",
    "effects": "walked as a block", "production": "walked as a block",
    "immediateEffects": "walked as a block",
    "permanentEffects": "walked as a block",
    # --- prose and provenance
    "text": "printed card text for human readers",
    "note": "a rules clarification for human readers",
    "aka": "alternate printed name for human readers",
    "source": "provenance of the card data, not a game value",
    "countSource": "provenance of the copy count, not a game value",
    "uncertain": "a data-confidence note, not a game value",
    # --- genuinely unpriced, with the reason
    "cost": "military-action cost of playing the card; the evaluator sees "
            "the action spent in the post-move state, not on the card",
    "target": "addressing: names who an aggression/war/event applies to",
    "scoringEvent": "end-of-age scoring, resolved by the rules engine",
    "sides": "pact structure; priced by deferred_credit, which reads inside",
    "urbanLimitCategory": "which urban cap the building counts against, a "
                          "rule constraint rather than a yield",
    # --- governments.  Same bug class as the unit `strength` above and NOT
    # fixed here: a government's action counts are top-level, so `_card_yields`
    # prices a government from its `production`/`effects` and never sees them.
    # Written off rather than mapped because governments are another lane's
    # card type; this entry is the visible record that it is still open.
    "civilActions": "government: top-level action count _card_yields does "
                    "not read -- the unit-strength bug, still open",
    "militaryActions": "government: top-level action count _card_yields does "
                       "not read -- the unit-strength bug, still open",
    "urbanBuildingLimit": "government: a cap on what is legal, not a yield",
    "peacefulCost": "government: science cost of the peaceful change route",
    "revolutionCost": "government: science cost of the revolution route",
}


class TestEffectCoverage(unittest.TestCase):

    def test_every_effect_key_is_accounted_for(self):
        priced = _all_priced_keys()
        known = priced | set(W.DELIBERATELY_UNPRICED)
        missing = {}
        for name, card in C.db().by_name.items():
            for block, body in _blocks(card):
                for k, v in body.items():
                    if k not in known:
                        missing.setdefault(k, []).append((name, block, v))
        self.assertEqual(
            missing, {},
            "effect key(s) neither mapped in _PROD_TO_FEATURE / "
            "_EFF_TO_FEATURE / _EFF_SPECIAL nor listed in "
            "DELIBERATELY_UNPRICED (engine/bots/weighted.py).  Map it to a "
            "feature, or add it to DELIBERATELY_UNPRICED with a reason -- do "
            "not leave it to be dropped silently:\n" + repr(missing))

    def test_no_stale_entries_in_the_unpriced_set(self):
        """A written-off key that no card carries any more is rot."""
        seen = set()
        for card in C.db().by_name.values():
            for _block, body in _blocks(card):
                seen |= set(body)
        stale = sorted(set(W.DELIBERATELY_UNPRICED) - seen)
        self.assertEqual(
            stale, [],
            "DELIBERATELY_UNPRICED names key(s) no card carries; delete "
            "them so the set keeps meaning what it says")

    def test_no_stale_entries_in_the_board_priced_set(self):
        """Same rule for the board-aware side: a key nothing carries is rot."""
        seen = set()
        for card in C.db().by_name.values():
            for _block, body in _blocks(card):
                seen |= set(body)
        self.assertEqual(sorted(set(BY.BOARD_PRICED) - seen), [])

    def test_no_key_is_both_priced_and_written_off(self):
        """The stale check above catches a write-off no card carries any more.
        It does NOT catch a write-off whose key someone has since MAPPED --
        the set would still name a live key, and the file would be asserting
        two contradictory things about it.  That is the shape of the mistake a
        lane makes fixing its own card type: map the key, forget the entry."""
        both = sorted(_all_priced_keys() & set(W.DELIBERATELY_UNPRICED))
        self.assertEqual(
            both, [],
            "key(s) mapped to a feature AND listed as deliberately unpriced; "
            "delete the DELIBERATELY_UNPRICED entry")

    def test_unpriced_keys_all_carry_a_reason(self):
        for k, why in W.DELIBERATELY_UNPRICED.items():
            self.assertTrue(isinstance(why, str) and len(why) > 20,
                            f"{k!r} needs a real reason, got {why!r}")

    def test_board_priced_keys_all_carry_a_reason(self):
        """A key claimed as board-priced has to say WHERE it is priced, or
        the set becomes a second, quieter place to hide a dropped key."""
        for k, why in BY.BOARD_PRICED.items():
            self.assertTrue(isinstance(why, str) and len(why) > 20,
                            f"{k!r} needs a real reason, got {why!r}")

    def test_a_key_is_not_claimed_both_priced_and_unpriced(self):
        overlap = sorted(set(BY.BOARD_PRICED) & set(W.DELIBERATELY_UNPRICED))
        self.assertEqual(overlap, [], "claimed in both directions")

    def test_every_priced_feature_key_has_a_weight(self):
        """A yield pointing at a key absent from DEFAULT_WEIGHTS is dead: it
        would be silently skipped by `card_potential`'s `w.get(k, 0.0)` and by
        `evaluate`'s `if wk:`, i.e. the same failure one level down."""
        targets = (set(W._PROD_TO_FEATURE.values())
                   | set(W._EFF_TO_FEATURE.values())
                   | set(W._EFF_SPECIAL.values())
                   | set(W._TERR_TO_FEATURE.values())
                   | {f for _a, f in BY._STATS_FEATURES}
                   | {"hand_limit", "build_discount", "no_aggression",
                      "gov_action_cost", "restricted_resources", "leader"})
        # `happy` is the deferred-credit map's spelling, resolved inside
        # `features()` into `happy_margin`; it is never a `_card_yields` key.
        absent = sorted(k for k in targets if k not in W.DEFAULT_WEIGHTS)
        self.assertEqual(absent, [])


class TestTheCensusStillReproducesMaster(unittest.TestCase):
    """`tools/card_blindness.py --legacy` is what the "master" column of
    docs/CARD_BLINDNESS.md's census is generated from, so it has to keep
    reproducing it: 171 cards with a dropped key, 168 with zero visible gain.

    This caught a real bug rather than being written for tidiness.  Every
    handler added since must be gated on its registry (`_EFF_SPECIAL`,
    `_EFF_CHOICE`) so `use_legacy_maps` can switch it off; `_card_choices`
    read the card data directly at first and `--legacy` therefore silently
    stopped reproducing the "before" numbers, quietly rewriting the baseline
    every later result is measured against."""

    def test_legacy_reproduces_the_published_before_numbers(self):
        from tools import card_blindness as cb
        # `use_legacy_maps` mutates module-level registries in place, so this
        # snapshots and restores them rather than reloading the module: a
        # reload makes a SECOND weighted module while `engine.bots` keeps a
        # reference to the first, and the two then disagree about what is
        # priced -- which showed up as unrelated tests failing depending on
        # the order they ran in.
        save = (dict(W._EFF_TO_FEATURE), dict(W._EFF_SPECIAL),
                dict(W._EFF_CHOICE), dict(W._UNIT_TO_FEATURE),
                dict(W._TERR_TO_FEATURE))
        try:
            cb.use_legacy_maps()
            types, dropped, zero, _k, _n, _c = cb.scan()
            self.assertEqual(sum(types.values()), 236)
            self.assertEqual(sum(dropped.values()), 171)
            self.assertEqual(sum(zero.values()), 168)
        finally:
            for reg, kept in zip((W._EFF_TO_FEATURE, W._EFF_SPECIAL,
                                  W._EFF_CHOICE, W._UNIT_TO_FEATURE,
                                  W._TERR_TO_FEATURE), save):
                reg.clear()
                reg.update(kept)
            W._card_yields.cache_clear()
            W._card_choices.cache_clear()


class TestTheCardsTheOmissionCost(unittest.TestCase):
    """Regression cases named individually, because 'seven wonders' is the
    part of this that is easy to break again without noticing."""

    CULTURE_RATE = {
        "Eiffel Tower": 4.0, "Taj Mahal": 3.0, "St. Peter's Basilica": 2.0,
        "Kremlin": 2.0, "Hanging Gardens": 1.0, "Great Wall": 1.0,
        "Library of Alexandria": 1.0, "Universitas Carolina": 1.0,
        "Joan of Arc": 1.0, "Mahatma Gandhi": 2.0,
    }
    SCIENCE_RATE = {"Library of Alexandria": 1.0, "Universitas Carolina": 2.0}

    def _yield_of(self, name, feature):
        return sum(a for k, a, _c in W._card_yields(name) if k == feature)

    def test_effect_culture_is_priced_as_culture_per_turn(self):
        for name, want in self.CULTURE_RATE.items():
            self.assertEqual(self._yield_of(name, "culture_rate"), want, name)

    def test_effect_science_is_priced_as_science_per_turn(self):
        for name, want in self.SCIENCE_RATE.items():
            self.assertEqual(self._yield_of(name, "science_rate"), want, name)

    def test_the_short_spelling_matches_the_engine(self):
        """`culture`/`science` map to the RATE features because that is what
        engine/effects.py does with them: FLAT_KEYS sends both to the same
        Stats slot as `cultureProduction`/`scienceProduction`."""
        from engine import effects
        self.assertEqual(effects.FLAT_KEYS["culture"],
                         effects.FLAT_KEYS["cultureProduction"])
        self.assertEqual(effects.FLAT_KEYS["science"],
                         effects.FLAT_KEYS["scienceProduction"])
        self.assertEqual(W._EFF_TO_FEATURE["culture"],
                         W._EFF_TO_FEATURE["cultureProduction"])
        self.assertEqual(W._EFF_TO_FEATURE["science"],
                         W._EFF_TO_FEATURE["scienceProduction"])

    def test_no_wonder_is_priced_at_nothing_but_being_a_wonder(self):
        """The headline number from the census: this was 7 of 16."""
        blind = []
        for name, card in C.db().by_name.items():
            if card["type"] != "wonder":
                continue
            gains = [(k, a) for k, a, kind in W._card_yields(name)
                     if kind != W._Y_COST and k != "wonders"]
            if not gains:
                blind.append(name)
        # Five remain, and every one is a text effect (DELIBERATELY_UNPRICED
        # bucket 2) rather than an omission: all four Age III wonders score
        # by a formula over the board ("2*workers(farm,mine)+...") and Ocean
        # Liners' whole card is `freePopIncreasePerTurn: True`.  Mapping them
        # needs a board-aware card evaluator, not another table entry.
        self.assertEqual(sorted(blind), [
            "Fast Food Chains", "First Space Flight", "Hollywood",
            "Internet", "Ocean Liners"])

    def test_masonry_and_friends_price_their_wonder_help(self):
        self.assertEqual(self._yield_of("Masonry", "build_discount"), 3.0)
        self.assertEqual(
            self._yield_of("Masonry", "wonder_stages_per_action"), 1.0)


class TestFinishDiscipline(unittest.TestCase):
    """`wonder_overrun` has to be the thing that separates a wonder you will
    finish from one you will not.  Across 120 logged games the bot started
    Pyramids 13 times and finished it 0 times, and went 0-for-58 on the three
    12-resource Age II wonders (docs/HEURISTICS.md, "Wonders, by age")."""

    def _feats(self, wonder=None, steps=0, plies=40, seed=11):
        import random
        from engine import actions as A, game as G
        from engine.state import WonderInProgress
        from engine.bots import WeightedBot
        st = G.new_game(2, seed)
        rng = random.Random(seed)
        bots = [WeightedBot(seed=seed + i) for i in range(2)]
        for _ in range(plies):
            if st.game_over:
                break
            A.apply(st, bots[st.decider()].pick(st, A.legal_moves(st)), rng)
        p = st.players[0]
        if wonder is None:
            p.wonder = None
        else:
            p.wonder = WonderInProgress(wonder)
            p.wonder.steps_built = steps
        return W.features(st, 0), st

    def test_all_three_are_zero_with_nothing_in_progress(self):
        f, _ = self._feats(None)
        for k in ("wonder_stages_left", "wonder_turns_to_finish",
                  "wonder_overrun"):
            self.assertEqual(f[k], 0.0, k)

    def test_a_cheap_early_wonder_and_an_expensive_one_differ(self):
        """Colossus is 6 resources over 2 stages; Fast Food Chains is 16 over
        4.  On master both were `wonder_remaining`, linear in resources, with
        nothing to say about whether the game would last long enough."""
        cheap, _ = self._feats("Colossus")
        dear, _ = self._feats("Fast Food Chains")
        self.assertLess(cheap["wonder_stages_left"],
                        dear["wonder_stages_left"])
        self.assertLess(cheap["wonder_turns_to_finish"],
                        dear["wonder_turns_to_finish"])

    def test_finishing_a_stage_reduces_every_term(self):
        none_built, _ = self._feats("Fast Food Chains", steps=0)
        two_built, _ = self._feats("Fast Food Chains", steps=2)
        for k in ("wonder_stages_left", "wonder_turns_to_finish"):
            self.assertLess(two_built[k], none_built[k], k)

    def test_completion_returns_them_to_zero(self):
        """The penalty is on the unfinished project, so finishing removes it.
        That is what makes a negative weight price STARTING rather than
        price wonders."""
        f, _ = self._feats("Colossus", steps=2)
        for k in ("wonder_stages_left", "wonder_turns_to_finish",
                  "wonder_overrun"):
            self.assertEqual(f[k], 0.0, k)

    def test_overrun_fires_only_when_the_game_will_end_first(self):
        f, st = self._feats("Fast Food Chains")
        rl = W.rounds_left(st)
        if f["wonder_turns_to_finish"] > rl:
            self.assertGreater(f["wonder_overrun"], 0.0)
        else:
            self.assertEqual(f["wonder_overrun"], 0.0)
        # and it is exactly the shortfall, not a rescaling of it
        self.assertAlmostEqual(
            f["wonder_overrun"],
            max(0.0, f["wonder_turns_to_finish"] - rl))


class TestNewWeightsAreInert(unittest.TestCase):
    """Everything added for docs/CARD_BLINDNESS.md except the two mappings
    defaults to 0.0, so a champion trained before them is unchanged."""

    INERT = ("wonder_stages_left", "wonder_turns_to_finish", "wonder_overrun",
             "wonder_stages_per_action", "hand_limit", "colonize_bonus",
             "build_discount", "free_civil_action", "resource_discount")

    def test_defaults_are_zero(self):
        for k in self.INERT:
            self.assertIn(k, W.DEFAULT_WEIGHTS)
            self.assertEqual(W.DEFAULT_WEIGHTS[k], 0.0, k)

    def test_the_frozen_champions_do_not_carry_them(self):
        import json
        import os
        here = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
        for n in (2, 3, 4):
            path = os.path.join(here, "analysis", "frozen",
                                f"champion_{n}p.json")
            if not os.path.exists(path):
                continue
            with open(path) as fh:
                d = json.load(fh)
            w = d.get("weights", d)
            for k in self.INERT:
                self.assertNotIn(k, w, f"{path}: {k}")

    def test_group_of_names_every_new_key(self):
        """experiments/summarize.py raises on an ungrouped key, and the whole
        point of that exception is that it fires in the commit that adds the
        feature -- not silently in six months."""
        from experiments import summarize
        for k in self.INERT:
            self.assertTrue(summarize.group_of(k))


# ===================================================================
# Lane B: territories, military units and tactics.  All 37 of these cards
# were "zero visible gain" with ZERO dropped keys, which is a contradiction
# only until you notice that neither the census nor this test looked anywhere
# except `production` and `effects`.  Units keep their yield in a top-level
# `strength`; territories keep theirs in `immediateEffects` /
# `permanentEffects`; a tactic's is a board query.  See docs/CARD_BLINDNESS.md.
# ===================================================================

class TestTopLevelFieldsAreAccountedFor(unittest.TestCase):
    """The block walk above cannot see a value printed OUTSIDE a block.

    That is how all ten unit cards priced out as pure cost for the whole
    project: `_card_yields` read their `techCost` and `buildCost` and never
    their `strength`, because `strength` is not in `production`.  The same
    hole is still open on governments (see TOP_LEVEL_UNPRICED), and this test
    is what keeps that fact visible instead of silent.
    """

    def test_every_top_level_key_is_priced_or_written_off(self):
        known = set(TOP_LEVEL_PRICED) | set(TOP_LEVEL_UNPRICED)
        missing = {}
        for name, card in C.db().by_name.items():
            for k in card:
                if k not in known:
                    missing.setdefault(k, []).append(name)
        self.assertEqual(
            missing, {},
            "top-level card field(s) neither in TOP_LEVEL_PRICED nor in "
            "TOP_LEVEL_UNPRICED.  A value printed outside `production` / "
            "`effects` is invisible to `_card_yields` unless it is read "
            "explicitly -- that is the bug that cost the ten unit cards:\n"
            + repr(missing))

    def test_no_stale_top_level_entries(self):
        seen = set()
        for card in C.db().by_name.values():
            seen |= set(card)
        stale = sorted((set(TOP_LEVEL_PRICED) | set(TOP_LEVEL_UNPRICED))
                       - seen)
        self.assertEqual(stale, [])

    def test_top_level_entries_carry_a_reason(self):
        for d in (TOP_LEVEL_PRICED, TOP_LEVEL_UNPRICED):
            for k, why in d.items():
                self.assertTrue(isinstance(why, str) and len(why) > 12,
                                f"{k!r} needs a real reason, got {why!r}")


class TestUnitsArePricedByWhatTheyContribute(unittest.TestCase):

    def _yield_of(self, name, feature):
        return sum(a for k, a, _c in W._card_yields(name) if k == feature)

    def test_every_unit_card_prices_its_printed_strength(self):
        units = [n for n, c in C.db().by_name.items()
                 if c["type"] in C.UNIT_TYPES]
        self.assertEqual(len(units), 10)
        for n in units:
            printed = C.db().get(n).get("strength") or 0
            self.assertEqual(self._yield_of(n, "strength"), float(printed), n)
            self.assertGreater(printed, 0, n)

    def test_unit_strength_matches_what_the_engine_does_with_it(self):
        """The same agreement argument as `culture` -> `culture_rate`:
        `effects._tech_prog` puts a unit card's TOP-LEVEL `strength` into the
        per-worker programme, in the slot it puts a farm's `production.food`
        into.  So pricing it as the `strength` feature is not an opinion."""
        from engine import effects
        for n, c in C.db().by_name.items():
            if c["type"] not in C.UNIT_TYPES:
                continue
            items, _eff = effects._tech_prog(n)
            self.assertEqual(dict(items).get("strength"),
                             c.get("strength"), n)
        self.assertEqual(W._PROD_TO_FEATURE["strength"], "strength")

    def test_an_age_ii_cavalry_and_artillery_are_no_longer_the_same_card(self):
        """Not that they must DIFFER -- Cavalrymen and Cannon are genuinely
        the same numbers -- but that the pricing now comes from those numbers
        instead of from `age + 1`, so a card whose strength changed would
        move.  Riflemen (3) against Modern Infantry (5) is the live case."""
        self.assertGreater(self._yield_of("Modern Infantry", "strength"),
                           self._yield_of("Riflemen", "strength"))
        on = dict(W.DEFAULT_WEIGHTS, unit_strength_credit=1.0)
        off = dict(W.DEFAULT_WEIGHTS, unit_strength_credit=0.0)
        self.assertLess(W.card_potential("Modern Infantry", off),
                        W.card_potential("Modern Infantry", on))

    def test_the_credit_recovers_the_pre_fix_pricing_exactly(self):
        w = dict(W.DEFAULT_WEIGHTS, unit_strength_credit=0.0)
        for n, c in C.db().by_name.items():
            if c["type"] not in C.UNIT_TYPES:
                continue
            # only the cost terms survive, so every unit is <= 0: the bug
            self.assertLessEqual(W.card_potential(n, w), 0.0, n)


class TestTerritoriesArePricedFromTheAppliedEffect(unittest.TestCase):

    def _terrs(self):
        return [n for n, c in C.db().by_name.items()
                if c["type"] == "territory"]

    def test_no_territory_has_zero_visible_gain(self):
        blind = []
        for n in self._terrs():
            gains = [k for k, _a, kind in W._card_yields(n)
                     if kind != W._Y_COST]
            if not gains:
                blind.append(n)
        self.assertEqual(sorted(blind), [], "was all 12")

    def test_the_map_cannot_drift_from_the_auction_path(self):
        """`deferred_credit` prices a territory you hold the high bid on by
        pushing the SAME two blocks through `_YIELD_TO_FEATURE`.  The hand
        path must agree with it, or the evaluator values the identical card
        differently depending on which code found it."""
        for k, v in W._YIELD_TO_FEATURE.items():
            if k in ("happy", "happiness"):
                # the one allowed difference, and it is required: "happy" is
                # not a weight, `features()` resolves it by hand
                self.assertEqual(W._TERR_TO_FEATURE[k], "happy_margin")
                self.assertNotIn(v, W.DEFAULT_WEIGHTS)
                continue
            self.assertEqual(W._TERR_TO_FEATURE[k], v, k)
        self.assertEqual(set(W._TERR_TO_FEATURE), set(W._YIELD_TO_FEATURE))

    def test_every_key_the_engine_applies_is_priced(self):
        """Derived from what `interact.gain_colony` actually does, not from a
        re-reading of the card text."""
        for n in self._terrs():
            card = C.db().get(n)
            for block in ("immediateEffects", "permanentEffects"):
                for k in (card.get(block) or {}):
                    self.assertIn(k, W._TERR_TO_FEATURE, f"{n}.{block}.{k}")

    def test_a_big_territory_outprices_a_small_one(self):
        w = dict(W.DEFAULT_WEIGHTS)
        self.assertGreater(W.card_potential("Historic Territory (II)", w),
                           W.card_potential("Historic Territory (I)", w))
        self.assertGreater(W.card_potential("Historic Territory (I)", w),
                           W.card_potential("Inhabited Territory (I)", w))

    def test_the_credit_recovers_the_pre_fix_pricing_exactly(self):
        w = dict(W.DEFAULT_WEIGHTS, territory_credit=0.0)
        for n in self._terrs():
            self.assertEqual(W.card_potential(n, w), 0.0, n)


class TestTacticPricing(unittest.TestCase):

    def test_the_duplicate_spelling_agrees_with_the_engine(self):
        """`effects.tacticBonus` is written off partly because it is a
        DUPLICATE of the top-level `strength` the engine actually reads
        (`_army_value`).  If the two ever disagree, the written-off reason is
        a lie and the card is mispriced."""
        n = 0
        for name, c in C.db().by_name.items():
            if c["type"] != "tactic":
                continue
            n += 1
            eff = c.get("effects") or {}
            self.assertEqual(eff.get("tacticBonus"), c.get("strength"), name)
            self.assertEqual(eff.get("tacticBonusObsolete"),
                             c.get("obsoleteStrength"), name)
        self.assertEqual(n, 15)

    def _board(self, units, tactic=None, hand=()):
        import random
        from engine import game as G
        st = G.new_game(2, 7)
        p = st.players[0]
        for name, k in units.items():
            if name not in p.techs:
                from engine.state import TechCard
                p.techs[name] = TechCard(name)
            p.techs[name].workers = k
        p.tactic = tactic
        p.hand_military = list(hand)
        del random
        return st, p

    def test_no_tactic_no_units_is_zero(self):
        from engine import effects
        st, p = self._board({})
        self.assertEqual(effects.tactic_outlook(st, p, []), (0, 0))

    def test_it_reports_the_units_still_owed(self):
        """Heavy Cavalry is 3 cavalry for +4.  With two, the tactic is worth
        nothing yet and the shortfall is exactly one -- which is the gradient
        that gets the third built while the payoff is still flat at zero."""
        from engine import effects
        st, p = self._board({"Knights": 2})
        val, short = effects.tactic_outlook(st, p, ["Heavy Cavalry"])
        self.assertEqual((val, short), (0, 1))
        st, p = self._board({"Knights": 3})
        val, short = effects.tactic_outlook(st, p, ["Heavy Cavalry"])
        self.assertEqual(val, 4)
        self.assertEqual(short, 3)      # a SECOND army needs three more

    def test_building_the_missing_unit_is_what_moves_the_feature(self):
        """The deadlock, as a test.  Holding Heavy Cavalry with two cavalry,
        neither playing the tactic (+0 strength) nor building the cavalry
        (+2 printed) shows the +4 that doing both is worth.  `tactic_gain`
        is the term that does."""
        st2, _ = self._board({"Knights": 2}, hand=["Heavy Cavalry"])
        st3, _ = self._board({"Knights": 3}, hand=["Heavy Cavalry"])
        self.assertEqual(W.tactic_terms(st2, 0), (0.0, 1.0))
        self.assertEqual(W.tactic_terms(st3, 0)[0], 4.0)
        self.assertEqual(W.features(st3, 0)["strength"],
                         W.features(st2, 0)["strength"] + 2)

    def test_playing_the_tactic_converts_the_gain_into_strength(self):
        """`tactic_gain` must go to 0 when the tactic is in play, or a
        positive weight would price holding the card rather than using it."""
        played, _ = self._board({"Knights": 3}, tactic="Heavy Cavalry")
        unplayed, _ = self._board({"Knights": 3}, hand=["Heavy Cavalry"])
        self.assertEqual(W.tactic_terms(played, 0)[0], 0.0)
        self.assertEqual(W.features(played, 0)["strength"],
                         W.features(unplayed, 0)["strength"] + 4)

    def test_the_terms_cost_nothing_when_their_weights_are_zero(self):
        """The gate, not the value: `tactic_terms` is +19% on `features()`,
        so a vector that does not use it must never call it."""
        called = []
        real = W.tactic_terms
        try:
            W.tactic_terms = lambda *a: called.append(a) or (0.0, 0.0)
            st, _ = self._board({"Knights": 3}, hand=["Heavy Cavalry"])
            W.evaluate(st, 0, dict(W.DEFAULT_WEIGHTS))
            self.assertEqual(called, [])
            W.evaluate(st, 0, dict(W.DEFAULT_WEIGHTS, tactic_gain=0.4))
            self.assertEqual(len(called), 1)
        finally:
            W.tactic_terms = real


class TestLaneBWeightsAreInert(unittest.TestCase):
    """The whole lane is inert: every fingerprint digest is byte-identical to
    master's and every trained vector evaluates exactly as it did.

    That is a choice, and the reason is measured -- see the comment on
    `unit_strength_credit` in weighted.py.  This test is what stops the choice
    being undone by accident: flipping any of these to a non-zero default
    moves six fingerprint digests, and the gate should not be the first place
    anybody finds that out.
    """

    INERT = ("tactic_gain", "tactic_short", "hand_mil_potential",
             "unit_strength_credit")

    def test_defaults_are_zero(self):
        for k in self.INERT:
            self.assertIn(k, W.DEFAULT_WEIGHTS)
            self.assertEqual(W.DEFAULT_WEIGHTS[k], 0.0, k)

    def test_credit_fallbacks_match_the_defaults(self):
        """`card_potential` is called both with a vector `load_weights` has
        filled in and with a raw champion dict.  If `_CREDIT_OF`'s fallback
        disagrees with `DEFAULT_WEIGHTS`, the same vector prices the same card
        differently depending on how it was loaded."""
        for _kind, (key, fallback) in W._CREDIT_OF.items():
            self.assertIn(key, W.DEFAULT_WEIGHTS)
            self.assertEqual(W.DEFAULT_WEIGHTS[key], fallback, key)

    def test_a_raw_champion_dict_prices_as_the_loaded_one_does(self):
        import json
        import os
        here = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
        path = os.path.join(here, "analysis", "frozen", "champion_2p.json")
        if not os.path.exists(path):
            self.skipTest("no frozen 2p champion")
        with open(path) as fh:
            d = json.load(fh)
        raw = d.get("weights", d)
        filled = dict(W.DEFAULT_WEIGHTS)
        filled.update(raw)
        for n in ("Swordsmen", "Air Forces", "Historic Territory (II)",
                  "Eiffel Tower"):
            self.assertEqual(W.card_potential(n, raw),
                             W.card_potential(n, filled), n)

    def test_territory_credit_is_believed_but_gated(self):
        """`territory_credit` is 1.0 -- a territory's printed effect is worth
        what it says -- and inert anyway, because `hand_mil_potential` is 0.0
        and nothing else calls `card_potential` on a military card."""
        self.assertEqual(W.DEFAULT_WEIGHTS["territory_credit"], 1.0)
        self.assertEqual(W.DEFAULT_WEIGHTS["hand_mil_potential"], 0.0)

    def test_territory_pricing_is_inert_without_the_military_hand_term(self):
        """`territory_credit` is 1.0, i.e. not itself inert -- but nothing
        calls `card_potential` on a military card until `hand_mil_potential`
        is non-zero, so the change costs a trained vector nothing."""
        import random
        from engine import actions as A, game as G
        from engine.bots import WeightedBot
        w0 = dict(W.DEFAULT_WEIGHTS)
        w1 = dict(W.DEFAULT_WEIGHTS, territory_credit=0.0)
        st = G.new_game(2, 5)
        rng = random.Random(5)
        bots = [WeightedBot(seed=1), WeightedBot(seed=2)]
        seen = 0
        for _ in range(160):
            if st.game_over:
                break
            i = st.decider()
            self.assertEqual(W.evaluate(st, i, w0), W.evaluate(st, i, w1))
            if st.players[i].hand_military:
                seen += 1
            A.apply(st, bots[i].pick(st, A.legal_moves(st)), rng)
        self.assertGreater(seen, 0, "no military hand ever held; vacuous")

    def test_group_of_names_every_new_key(self):
        from experiments import summarize
        for k in self.INERT + ("unit_strength_credit", "territory_credit"):
            self.assertTrue(summarize.group_of(k))


if __name__ == "__main__":
    unittest.main()


def _numeric(v):
    return isinstance(v, (int, float)) and v is not True and v is not False


class TestNonNumericValuesAreNotDroppedSilently(unittest.TestCase):
    """The NEXT class of blindness after the `culture`/`science` omission.

    `_card_yields` skips any effect value that is `True`, `False` or a string:

        if amt is True or amt is False or not isinstance(amt, (int, float)):
            continue

    That line is correct -- there is no number to multiply a weight by -- and
    it is also exactly how a card can be MAPPED and still priced at nothing.
    Ocean Liners (`freePopIncreasePerTurn: True`), Transcontinental Railroad
    (`doubleBestMine: True`) and the three `onBuildCulture` wonders all sat in
    that hole while the coverage test above was green, because that test only
    ever asks whether the KEY is known, never whether the VALUE survives the
    trip.

    So: every boolean and every string value must be reachable by something
    that can cope with it -- `board_yields`, an `_EFF_SPECIAL` entry, or an
    explicit write-off.  A bare `_EFF_TO_FEATURE` mapping is NOT enough and
    this test says so.
    """

    def test_every_boolean_or_string_value_has_a_non_table_home(self):
        table_only = set(W._PROD_TO_FEATURE) | set(W._EFF_TO_FEATURE)
        can_cope = (set(BY.BOARD_PRICED) | set(W._EFF_SPECIAL)
                    | set(W.DELIBERATELY_UNPRICED))
        dropped = {}
        for name, card in C.db().by_name.items():
            for block, body in _blocks(card):
                for k, v in body.items():
                    if _numeric(v) or isinstance(v, dict):
                        continue
                    if k in can_cope or (name, k) in W.UNPRICED_VALUES:
                        continue
                    why = ("mapped into a table that only reads numbers"
                           if k in table_only else "not accounted for at all")
                    dropped.setdefault(k, []).append((name, block, v, why))
        self.assertEqual(
            dropped, {},
            "effect key(s) whose value is a bool or a string and which "
            "nothing can price: an `_EFF_TO_FEATURE` entry is not enough, "
            "because `_card_yields` drops non-numeric values.  Price it in "
            "engine/bots/board_yields.py, give it an `_EFF_SPECIAL` entry, "
            "or add a DELIBERATELY_UNPRICED / UNPRICED_VALUES reason:\n"
            + repr(dropped))

    def test_unpriced_values_carry_a_reason_and_are_not_stale(self):
        by_name = C.db().by_name
        stale = []
        for (name, k), why in W.UNPRICED_VALUES.items():
            self.assertTrue(isinstance(why, str) and len(why) > 20,
                            f"{(name, k)!r} needs a real reason")
            card = by_name.get(name)
            v = None if card is None else dict(
                (card.get("effects") or {}),
                **(card.get("production") or {})).get(k)
            if v is None or _numeric(v):
                stale.append((name, k))
        self.assertEqual(sorted(stale), [],
                         "written off as prose but no longer prose")

    def test_the_drop_that_motivates_this_is_still_real(self):
        """Negative control: if `_card_yields` ever copes with non-numeric
        values on its own, this class is measuring nothing."""
        self.assertEqual(
            [y for y in W._card_yields("Ocean Liners")
             if y[0] not in ("resource_stock", "wonders")], [],
            "the static table now prices Ocean Liners; this class was "
            "written on the premise that it cannot")


class TestOneImplementation(unittest.TestCase):
    """The scorer and the evaluator must never be two implementations.

    `engine/effects.py:1197-1202` is the note left by the last person who let
    that happen. `board_yields._on_build_culture` prices Hollywood by calling
    `effects.wonder_completion_culture`, and `effects.on_wonder_complete`
    pays it out by calling the same function. Shared source of truth without
    a divergence test decays back into two implementations the first time
    somebody optimises one side, so this is that test.
    """

    def _rich(self, seed=5):
        from engine import game as G
        from engine.state import TechCard
        st = G.new_game(2, seed)
        p = st.players[0]
        for n, wk in (("Warriors", 2), ("Legions", 2), ("Bronze", 2),
                      ("Iron", 1), ("Agriculture", 2), ("Philosophy", 2),
                      ("Religion", 1), ("Drama", 2), ("Printing Press", 1)):
            if n in C.db().by_name:
                p.techs[n] = TechCard(n, workers=wk)
        p.workers_free = 2
        return st, p

    AGE_III = ("Fast Food Chains", "Hollywood", "Internet",
               "First Space Flight")

    def test_the_payout_is_exactly_what_the_forecast_said(self):
        """Play the wonder for real and check `p.culture` moved by exactly
        the number the bot was quoted a moment earlier."""
        from engine import effects
        for name in self.AGE_III:
            st, p = self._rich()
            quoted = sum(a for f, a, _k in BY.board_yields(name, st, 0)
                         if f == "culture")
            before = p.culture
            paid = effects.on_wonder_complete(st, p, name)
            self.assertEqual(p.culture - before, paid, name)
            self.assertEqual(float(paid), quoted,
                             f"{name}: the bot was quoted {quoted} culture "
                             f"and the scorer paid {paid} -- the forecast and "
                             "the payout have diverged")
            self.assertGreater(paid, 0, f"{name}: board too poor to test")

    def test_on_wonder_complete_still_routes_through_the_shared_function(self):
        """The structural half: monkeypatch the shared function and the real
        payout must change with it.  A copy-paste of the formula back into
        `on_wonder_complete` would pass the numeric test above on the day it
        was written and drift afterwards; this fails immediately."""
        from engine import effects
        st, p = self._rich()
        was = effects.wonder_completion_culture
        effects.wonder_completion_culture = lambda s, pl, n: 4242
        try:
            paid = effects.on_wonder_complete(st, p, "Hollywood")
        finally:
            effects.wonder_completion_culture = was
        self.assertEqual(
            paid, 4242,
            "engine.effects.on_wonder_complete no longer calls "
            "wonder_completion_culture -- there are two implementations of "
            "the Age III wonder formulas again")


class TestWondersAreBoardPriced(unittest.TestCase):
    """Lane A: wonders go through the same swap diff as leaders.

    They were excluded when `board_yields` landed, on the reasoning that a
    wonder accumulates rather than replaces.  True, and it makes the diff
    simpler rather than wrong: append to `completed_wonders`, compute,
    restore, and the delta is a pure gain.
    """

    W1 = dict(W.DEFAULT_WEIGHTS, card_board_credit=1.0)
    RATINGS = {"culture_rate": "culture", "science_rate": "science",
               "food_rate": "food", "resource_rate": "resources",
               "strength": "strength", "happy_margin": "happy"}
    #: the one wonder whose value is IMPUTED, not mirrored: a free population
    #: increase is a bool in `Stats`, not a rating, so there is no engine
    #: number to check the rider against.
    NOT_A_RATING = frozenset({"Ocean Liners"})

    def _rich(self, seed=5):
        return TestOneImplementation._rich(self, seed)

    def test_no_wonder_is_blind_on_a_board_that_feeds_it(self):
        """The census headline. Five wonders had zero visible gain under the
        static table -- all four Age III ones and Ocean Liners."""
        st, _p = self._rich()
        blind = [n for n, c in C.db().by_name.items() if c["type"] == "wonder"
                 and not [t for t in BY.board_yields(n, st, 0)
                          if t[2] != BY._COST and t[0] != "wonders"]]
        self.assertEqual(blind, [])

    def test_the_claim_equals_what_the_engine_does_with_the_card(self):
        """For every wonder, the board yields on the six ratings must equal
        the delta `effects.compute` reports when that wonder is actually put
        into play on the same board.  This is what makes the swap diff
        provable rather than merely plausible."""
        from engine import effects
        for name, card in C.db().by_name.items():
            if card["type"] != "wonder" or name in self.NOT_A_RATING:
                continue
            st, p = self._rich()
            # ask FIRST: the swap diff answers "what if I had this", so a
            # board that already holds the wonder answers "nothing".
            claimed = dict.fromkeys(self.RATINGS, 0.0)
            for k, amt, _kind in BY.board_yields(name, st, 0):
                if k in claimed:
                    claimed[k] += amt
            before = effects.compute(st, p)
            p.completed_wonders.append(name)
            effects.invalidate(st, p)
            after = effects.compute(st, p)
            for feat, attr in self.RATINGS.items():
                self.assertEqual(
                    claimed[feat],
                    float(getattr(after, attr) - getattr(before, attr)),
                    f"{name}/{feat}: evaluator and engine have diverged")

    def test_a_wonder_is_not_free(self):
        """`card_potential` prices a swap card by the diff ALONE, so the
        stage cost has to be added back or every wonder becomes free."""
        st, _p = self._rich()
        for name, card in C.db().by_name.items():
            if card["type"] != "wonder":
                continue
            ys = dict(((f, k), a) for f, a, k in BY.board_yields(name, st, 0))
            self.assertEqual(ys.get(("wonders", BY._GAIN)), 1.0, name)
            self.assertEqual(ys.get(("resource_stock", BY._COST)),
                             -float(sum(card["stages"])), name)

    def test_ocean_liners_is_worthless_with_an_empty_yellow_bank(self):
        """The board fact a table cannot express: a free population increase
        is worth nothing to a player with no population left to increase."""
        st, p = self._rich()
        rider = [t for t in BY.board_yields("Ocean Liners", st, 0)
                 if t[0] in ("civil_actions", "food_rate")]
        self.assertTrue(rider)
        p.yellow_bank = 0
        self.assertEqual(
            [t for t in BY.board_yields("Ocean Liners", st, 0)
             if t[0] in ("civil_actions", "food_rate")], [])

    def test_the_swap_leaves_the_board_untouched(self):
        """`completed_wonders` is rebound, not mutated: a journal watching
        the original list must never see a write."""
        from engine import statediff
        from engine.bots.fastcopy import copy_state
        st, p = self._rich()
        original = p.completed_wonders
        before = copy_state(st, keep_log=True)
        for n, c in C.db().by_name.items():
            if c["type"] == "wonder":
                BY.board_yields(n, st, 0)
        self.assertIs(p.completed_wonders, original)
        self.assertEqual(statediff.diff(before, st), [])

    def test_the_credit_weight_switches_it_off(self):
        st, _p = self._rich()
        for name in ("Fast Food Chains", "Ocean Liners", "Great Wall"):
            self.assertEqual(W.card_potential(name, W.DEFAULT_WEIGHTS, st, 0),
                             W.card_potential(name, W.DEFAULT_WEIGHTS), name)
            self.assertNotEqual(W.card_potential(name, self.W1, st, 0),
                                W.card_potential(name, W.DEFAULT_WEIGHTS),
                                name)
