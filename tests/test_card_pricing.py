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
from engine.bots import weighted as W


def _blocks(card):
    for block in ("production", "effects"):
        yield block, (card.get(block) or {})


class TestEffectCoverage(unittest.TestCase):

    def test_every_effect_key_is_accounted_for(self):
        priced = (set(W._PROD_TO_FEATURE) | set(W._EFF_TO_FEATURE)
                  | set(W._EFF_SPECIAL))
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

    def test_unpriced_keys_all_carry_a_reason(self):
        for k, why in W.DELIBERATELY_UNPRICED.items():
            self.assertTrue(isinstance(why, str) and len(why) > 20,
                            f"{k!r} needs a real reason, got {why!r}")

    def test_every_priced_feature_key_has_a_weight(self):
        """A yield pointing at a key absent from DEFAULT_WEIGHTS is dead: it
        would be silently skipped by `card_potential`'s `w.get(k, 0.0)` and by
        `evaluate`'s `if wk:`, i.e. the same failure one level down."""
        targets = (set(W._PROD_TO_FEATURE.values())
                   | set(W._EFF_TO_FEATURE.values())
                   | set(W._EFF_SPECIAL.values()))
        # `happy` is the deferred-credit map's spelling, resolved inside
        # `features()` into `happy_margin`; it is never a `_card_yields` key.
        absent = sorted(k for k in targets if k not in W.DEFAULT_WEIGHTS)
        self.assertEqual(absent, [])


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


if __name__ == "__main__":
    unittest.main()
