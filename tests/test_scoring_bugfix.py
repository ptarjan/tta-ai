"""Four scoring bugs found against the 1,011-game BGO corpus, pinned.

`docs/SCORE_VALIDATION.md` replayed every human journal into a real
`GameState` and diffed our scorer against BGO's own printed numbers.  Three
of the fifteen `Impact of ...` rows and two of the four Age III wonder
bonuses came out systematically wrong -- one sign, one size, one leader.
`docs/SCORE_BUGFIX.md` records the corpus counts before and after.

Every test below is written as a position, not as a call into the corpus
tooling, so it keeps meaning if the journals are not on disk.  The corpus
count each one comes from is in the docstring, because the *reason to
believe* the expected number is the corpus, not this file.
"""
from __future__ import annotations

import os
import sys
import unittest

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

from engine import cards as C, economy, effects, events, game   # noqa: E402
from engine.state import TechCard                               # noqa: E402

_DB = C.db()


def position(techs, *, leader=None, wonders=(), flipped=(), free=1,
             government="Despotism", bank=18):
    """A 2p game whose player 0 has exactly this tableau."""
    st = game.new_game(2, seed=1)
    p = st.players[0]
    p.techs = {n: TechCard(name=n, workers=w) for n, w in techs.items()}
    p.government = government
    p.leader = leader
    p.completed_wonders = list(wonders)
    p.flipped_wonders = list(flipped)
    p.colonies = []
    p.workers_free = free
    p.yellow_bank = bank
    effects.invalidate(st, p)
    return st, p


def impact(st, p, name):
    block = (_DB.get(name).get("effects") or {}).get("allPlayers") or {}
    return events.scoring_culture(st, p, block, st.players)


# ------------------------------------------------------------- Bug 1

class ImpactOfIndustry(unittest.TestCase):
    """"...culture equal to the amount of resources its MINES produce.
    (Ignore any production from other sources.)"

    We scored `s.resources`, the whole resource rating.  Corpus: 61/81 exact
    before, 95/95 after (every residual was positive and every one was a Bill
    Gates lab level).
    """

    def test_scores_mine_production(self):
        st, p = position({"Bronze": 2})          # Age A mine, 1 resource each
        self.assertEqual(impact(st, p, "Impact of Industry"), 2)

    def test_bill_gates_labs_are_not_mines(self):
        """The card's own text: "labs affected by Bill Gates are not mines".

        Bill Gates makes each lab produce resources equal to its level, so the
        resource RATING moves and the mines' output does not.
        """
        techs = {"Bronze": 2, "Philosophy": 1, "Alchemy": 2}   # labs lv 0, 1
        st, p = position(techs, leader="Bill Gates")
        self.assertEqual(effects.state_stats(st, p).resources, 2 + 0 + 2)
        self.assertEqual(impact(st, p, "Impact of Industry"), 2)

    def test_the_transcontinental_railroad_doubles_one_mine_worker(self):
        """The Railroad IS a mine effect and does count (FAQ v1.5 p.9), but
        it doubles one worker on the best mine, not the whole card."""
        st, p = position({"Bronze": 1, "Iron": 2},        # Iron: 2 resources
                         wonders=["Transcontinental Railroad"])
        self.assertEqual(impact(st, p, "Impact of Industry"), 1 + 4 + 2)

    def test_a_flipped_railroad_is_ruins_and_doubles_nothing(self):
        st, p = position({"Iron": 2}, wonders=["Transcontinental Railroad"],
                         flipped=["Transcontinental Railroad"])
        self.assertEqual(impact(st, p, "Impact of Industry"), 4)


# ------------------------------------------------------------- Bug 2

class ImpactOfPopulation(unittest.TestCase):
    """"2 culture per content worker above 10."

    We counted only workers standing on cards.  A yellow token in the worker
    pool is a worker too -- a discontent worker is physically an *unused*
    worker moved onto the happiness track.  Corpus: 43/81 exact before,
    73/88 after, and 68/72 on the rows where our engine also says discontent
    is 0 (the only rows where the replay can verify every input).
    """

    def test_unused_workers_are_population(self):
        # 12 workers on cards, 3 in the pool, and a bank of 18 (nothing spent
        # into the population track, so no happy faces are required and there
        # are no discontent workers).  15 - 10 = 5 content workers above ten.
        st, p = position({"Bronze": 6, "Agriculture": 6}, free=3, bank=18)
        self.assertEqual(economy.discontent(st, p), 0)
        self.assertEqual(impact(st, p, "Impact of Population"), 10)
        # what the old code scored: 12 on cards only
        self.assertEqual(2 * max(0, 12 - 10), 4)

    def test_a_pool_only_difference_is_worth_2_culture_each(self):
        """The residual signature that found this: every one a multiple of 2."""
        techs = {"Bronze": 6, "Agriculture": 6}
        a = position(techs, free=0, bank=18)
        b = position(techs, free=4, bank=18)
        self.assertEqual(impact(*a, "Impact of Population")
                         + 2 * 4,
                         impact(*b, "Impact of Population"))

    def test_discontent_workers_are_still_not_content(self):
        """Population is workers on cards + unused MINUS discontent.

        Corpus caveat (`docs/SCORE_BUGFIX.md`): on the rows where our engine
        says discontent > 0 we are still only 5/16 against BGO, and the
        alternative that ignores discontent entirely is 7/16 -- neither
        reading fits, and happy faces are the one input the journal never
        prints.  The card says "content worker", so discontent is subtracted.
        """
        st, p = position({"Bronze": 8, "Agriculture": 8}, free=2, bank=8)
        self.assertEqual(economy.discontent(st, p), 4)   # 4 required, 0 happy
        self.assertEqual(impact(st, p, "Impact of Population"),
                         2 * (16 + 2 - 4 - 10))


# ------------------------------------------------------------- Bug 3

class WonderOneTimeCulture(unittest.TestCase):
    """Hollywood and the Internet score the buildings' EFFECTIVE output.

    They were built from printed `production` values with a single ad-hoc Sid
    Meier special case, so every completion under Chaplin, Shakespeare,
    Newton or Einstein was under-scored.  Corpus, at the instant of
    completion: Hollywood 20/35 -> 44/44, Internet 46/65 -> 63/68.
    """

    def bonus(self, st, p, wonder):
        return effects.on_wonder_complete(st, p, wonder)

    def test_hollywood_doubles_theater_and_library_culture(self):
        st, p = position({"Drama": 2})           # Age I theater, 2 culture
        self.assertEqual(self.bonus(st, p, "Hollywood"), 2 * 4)

    def test_hollywood_counts_chaplins_doubled_theater(self):
        st, p = position({"Drama": 2}, leader="Charlie Chaplin")
        # 2 workers x 2 culture, +2 for the one doubled building, x2
        self.assertEqual(self.bonus(st, p, "Hollywood"), 2 * 6)

    def test_hollywood_counts_shakespeares_pairs(self):
        st, p = position({"Drama": 1, "Printing Press": 1},
                         leader="William Shakespeare")
        # theater 2 + library 1 + one library/theater pair 2, x2
        self.assertEqual(self.bonus(st, p, "Hollywood"), 2 * 5)

    def test_hollywood_ignores_a_lab(self):
        st, p = position({"Drama": 1, "Philosophy": 1}, leader="Sid Meier")
        self.assertEqual(self.bonus(st, p, "Hollywood"), 2 * 2)

    def test_internet_keeps_sid_meier_exact(self):
        """Sid Meier was the one leader the old code handled, and the one
        leader the corpus said was already right (38/38).  He must stay so."""
        st, p = position({"Alchemy": 2}, leader="Sid Meier")   # lv 1 lab, 2 sci
        # per worker: 2 science - 1 (Sid Meier) + 1 culture (level 1) = 2
        self.assertEqual(self.bonus(st, p, "Internet"), 4)

    def test_internet_counts_einsteins_best_lab(self):
        st, p = position({"Alchemy": 1}, leader="Albert Einstein")
        self.assertEqual(self.bonus(st, p, "Internet"), 2 + 1)

    def test_internet_counts_newtons_best_lab(self):
        st, p = position({"Alchemy": 1}, leader="Isaac Newton")
        self.assertEqual(self.bonus(st, p, "Internet"), 2 + 1)

    def test_internet_counts_an_arenas_strength(self):
        st, p = position({"Bread and Circuses": 1})
        self.assertEqual(
            self.bonus(st, p, "Internet"),
            (_DB.get("Bread and Circuses").get("production") or {})
            .get("strength", 0))

    def test_internet_ignores_a_colony_and_a_government(self):
        """Only what the URBAN BUILDINGS give."""
        st, p = position({"Philosophy": 1}, government="Monarchy")
        self.assertEqual(self.bonus(st, p, "Internet"), 1)

    def test_fast_food_chains_is_unchanged(self):
        st, p = position({"Bronze": 2, "Philosophy": 1, "Warriors": 1})
        self.assertEqual(self.bonus(st, p, "Fast Food Chains"), 2 * 2 + 2)


# ------------------------------------------------------------- Bug 4

class ChaplinDoublesOneBuilding(unittest.TestCase):
    """"Your best theater produces twice as much culture" -- ONE building.

    We doubled every worker on the best theater CARD.  This is a culture
    *rating* bug, so it is the one of the four with an oracle outside the
    scoring code: fixing it moved our agreement with BGO's printed per-turn
    culture from 40,280/43,847 (91.9%) to 40,718 (92.9%), all-five-rates
    agreement from 79.2% to 80.0%, and turn-16+ agreement from 58.1% to
    62.1%.  It is the same shape as the Transcontinental Railroad's "one of
    your best mines", which the engine already read as one worker.
    """

    def test_one_worker_is_doubled_not_the_whole_card(self):
        st, p = position({"Drama": 3})                  # 3 workers x 2 culture
        base = effects.state_stats(st, p).culture
        st2, p2 = position({"Drama": 3}, leader="Charlie Chaplin")
        self.assertEqual(effects.state_stats(st2, p2).culture, base + 2)

    def test_the_best_theater_is_the_highest_level_one_with_workers(self):
        st, p = position({"Drama": 1, "Opera": 0}, leader="Charlie Chaplin")
        # Opera (Age II, 3 culture) has no workers, so Drama is "best"
        self.assertEqual(effects.state_stats(st, p).culture, 2 + 2)

    def test_it_matches_the_railroads_reading_of_the_same_phrase(self):
        st, p = position({"Iron": 3}, wonders=["Transcontinental Railroad"])
        self.assertEqual(effects.state_stats(st, p).resources, 3 * 2 + 2)


class BuildingOutputHelper(unittest.TestCase):
    """`effects.building_output` is what makes the three cards agree."""

    def test_a_modifier_that_straddles_two_types_needs_both(self):
        """Shakespeare's library/theater pair counts for Hollywood (which asks
        about both) but not for a theaters-only question."""
        st, p = position({"Drama": 1, "Printing Press": 1},
                         leader="William Shakespeare")
        both = effects.building_output(
            p, frozenset({"theater", "library"}), ("culture",))
        theat = effects.building_output(
            p, frozenset({"theater"}), ("culture",))
        self.assertEqual(both, 2 + 1 + 2)
        self.assertEqual(theat, 2)

    def test_mine_resources_is_the_industry_reading(self):
        st, p = position({"Bronze": 2, "Philosophy": 2}, leader="Bill Gates")
        self.assertEqual(effects.mine_resources(p), 2)

    def test_it_agrees_with_the_rating_when_nothing_else_produces(self):
        """A tableau with no non-mine resource source: the card's answer and
        the resource rating must be the same number."""
        st, p = position({"Bronze": 2, "Iron": 1})
        self.assertEqual(effects.mine_resources(p),
                         effects.state_stats(st, p).resources)


if __name__ == "__main__":
    unittest.main()
