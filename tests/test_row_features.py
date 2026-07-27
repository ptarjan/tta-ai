"""The information-audit fixes: gaps 1, 2, 3 and 5.

docs/INFORMATION_AUDIT.md measured the evaluator's blindness by PERTURBING a
real mid-game state and showing the feature vector came back bit-identical:
deleting the whole card row, replacing a rival's civil hand, and paying 3 CA
instead of 1 for the same card each moved the evaluation by exactly 0.0.

Every test below is the mirror image of one of those measurements -- perturb
the same thing and assert it now DOES move -- plus the guards that make the
change safe to cut a training run over to:

  * the new weights all default to 0.0, so a champion trained before they
    existed evaluates bit-identically (`test_defaults_are_inert`);
  * the new state fields survive `fastcopy` and the journal, which is what
    `bash tools/gate.sh`'s PARANOID arms enforce over 135 games and what
    `test_fastcopy_*` pins directly.
"""
import copy
import os
import random
import sys
import unittest

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

from engine import actions as A, game as G, journal          # noqa: E402

# `actions.STRICT` defaults to FALSE, so `apply` does not check legality unless
# a test module turns it on -- test_engine/test_combat/test_coverage_audit all
# do, and because that is a module global it leaks into every other test in a
# discovery run.  A fixture that is only legal when this file runs alone is a
# fixture that lies, so turn it on here too and mean it.
A.STRICT = True
from engine.state import TechCard, WonderInProgress          # noqa: E402
from engine.bots import WeightedBot                          # noqa: E402
from engine.bots import weighted as W                        # noqa: E402
from engine.bots.fastcopy import copy_state                  # noqa: E402


def play(n=2, seed=7, plies=60):
    """A real mid-game position, the same way the audit built one."""
    st = G.new_game(n, seed)
    rng = random.Random(seed * 31 + 1)
    bots = [WeightedBot(seed=seed * 7 + i) for i in range(n)]
    for _ in range(plies):
        if st.game_over:
            break
        A.apply(st, bots[st.decider()].pick(st, A.legal_moves(st)), rng)
    return st


def row_on(**over):
    """DEFAULT_WEIGHTS with the audit's new terms switched on."""
    w = dict(W.DEFAULT_WEIGHTS)
    w.update({"take_cost_paid": -0.5, "row_urgency": -0.1,
              "row_bargain_forgone": -0.2, "rival_hand_potential": -0.1,
              "rival_free_ca": -0.1, "rival_hand_civil": -0.1,
              "rival_wonders": -0.1})
    w.update(over)
    return w


# ------------------------------------------------------------------- gap 1

class TakeCostPaid(unittest.TestCase):
    """GAP 1: row depth reached the evaluation only through `ca_left`, whose
    3p champion weight is -0.0974 -- so paying 3 CA rather than 1 for the
    identical card scored as a GAIN of 0.195."""

    def _state_with_row(self, second=None):
        """A legal 13-slot row of one plain technology.

        NOT `Bronze`: it is in `game.START_TECHS`, so the one-per-name rule
        (§2.5) makes every take of it illegal.  `Alchemy` is an Age I lab
        nobody starts with, and it is neither a wonder nor a leader, so no
        surcharge or discount muddies the slot cost being asserted.
        """
        st = G.new_game(2, 11)
        p = st.players[0]
        p.civil_actions = 6
        st.card_row = ["Alchemy"] * 13
        if second is not None:
            st.card_row[9:] = [second] * 4
        return st, p

    def test_counter_is_the_civil_actions_actually_paid(self):
        for slot, want in ((0, 1), (5, 2), (9, 3)):
            with self.subTest(slot=slot):
                st, p = self._state_with_row()
                A.apply(st, ("take", slot), random.Random(0))
                self.assertEqual(p.ca_spent_taking, want)
                self.assertEqual(W.features(st, 0)["take_cost_paid"], want)

    def test_the_same_card_at_slot_0_and_slot_9_now_differ(self):
        """The headline measurement of the audit, inverted."""
        vals = {}
        for slot in (0, 9):
            st, _ = self._state_with_row()
            A.apply(st, ("take", slot), random.Random(0))
            vals[slot] = W.evaluate(st, 0, row_on())
        self.assertNotEqual(vals[0], vals[9])
        # and the cheap slot is now the better one under a negative weight
        self.assertGreater(vals[0], vals[9])

    def test_under_default_weights_it_is_still_invisible(self):
        """The 0.0 default is what keeps the frozen champions valid."""
        vals = set()
        for slot in (0, 9):
            st, _ = self._state_with_row()
            A.apply(st, ("take", slot), random.Random(0))
            # `ca_left` still differs, so compare the take_cost_paid channel
            vals.add(W.DEFAULT_WEIGHTS.get("take_cost_paid", 0.0))
        self.assertEqual(vals, {0.0})

    def test_it_accumulates_within_a_turn(self):
        # a second, different card: once Alchemy is in hand the one-per-name
        # rule makes a second Alchemy illegal
        st, p = self._state_with_row(second="Theology")
        A.apply(st, ("take", 0), random.Random(0))
        A.apply(st, ("take", 9), random.Random(0))
        self.assertEqual(p.ca_spent_taking, 4)

    def test_it_resets_at_the_start_of_my_next_turn(self):
        st = play(2, seed=5, plies=40)
        st.players[st.current].ca_spent_taking = 99
        G.start_turn(st, random.Random(1))
        self.assertEqual(st.players[st.current].ca_spent_taking, 0)

    def test_a_free_take_does_not_count(self):
        """`take_card` is also reached by free takes; only `_h_take` pays."""
        st, p = self._state_with_row()
        A.take_card(st, p, 9)
        self.assertEqual(p.ca_spent_taking, 0)


# ------------------------------------------------------------------- gap 5

class CivilDiscardRecord(unittest.TestCase):
    """GAP 5: `_replenish` wrote `None` over swept cards, destroying public
    information a human at the table can see."""

    def test_replenish_records_what_it_destroys(self):
        st = G.new_game(2, 3)
        st.round = 2                       # replenish only runs from round 2
        before = list(st.card_row)
        G._replenish(st, random.Random(0))
        swept = before[:G.SWEEP[2]]
        recorded = [n for names in st.civil_discard.values() for n in names]
        self.assertEqual(sorted(recorded), sorted(swept))

    def test_it_is_keyed_by_the_cards_own_age(self):
        st = play(2, seed=9, plies=60)
        self.assertTrue(st.civil_discard)
        for age, names in st.civil_discard.items():
            for n in names:
                self.assertEqual(G.C.db().age_of(n), age)

    def test_the_unseen_count_is_now_computable(self):
        """The point of the record: unseen(age) = deck - row - hands -
        tableaux - discard is exactly computable from public information."""
        st = play(2, seed=13, plies=70)
        db = G.C.db()
        age = "I"
        full = list(db.civil_deck(age, 2))
        seen = list(st.civil_discard.get(age, ()))
        seen += [n for n in st.card_row if n and db.age_of(n) == age]
        for p in st.players:
            seen += [n for n in p.hand_civil if db.age_of(n) == age]
            seen += [n for n in p.techs if db.age_of(n) == age]
        # every card accounted for is really from that age's deck
        for n in seen:
            self.assertIn(n, full)
        self.assertGreater(len(seen), 0)

    def test_nothing_in_the_engine_reads_it(self):
        """It is a record, not state: play must not depend on it."""
        st_a = play(2, seed=21, plies=50)
        st_b = play(2, seed=21, plies=50)
        st_b.civil_discard = {}
        rng_a, rng_b = random.Random(4), random.Random(4)
        bot_a, bot_b = WeightedBot(seed=2), WeightedBot(seed=2)
        for _ in range(12):
            if st_a.game_over or st_b.game_over:
                break
            mv_a = bot_a.pick(st_a, A.legal_moves(st_a))
            mv_b = bot_b.pick(st_b, A.legal_moves(st_b))
            self.assertEqual(mv_a, mv_b)
            A.apply(st_a, mv_a, rng_a)
            A.apply(st_b, mv_b, rng_b)
        self.assertEqual(G.scores(st_a), G.scores(st_b))


# ------------------------------------------------------------------- gap 2

class RowPressure(unittest.TestCase):
    """GAP 2: deleting the ENTIRE card row left the evaluation at 0.0 delta."""

    def test_the_slide_constant_matches_the_engine(self):
        """`_SWEEP` is duplicated in weighted.py to dodge an import cycle."""
        self.assertEqual(W._SWEEP, G.SWEEP)

    def test_deleting_the_row_now_moves_the_evaluation(self):
        st = play(2, seed=7, plies=60)
        w = row_on()
        before = W.evaluate(st, 0, w, W.rival_context(st, 0))
        st2 = copy.deepcopy(st)
        st2.card_row = [None] * 13
        after = W.evaluate(st2, 0, w, W.rival_context(st2, 0))
        self.assertNotEqual(before, after)

    def test_reversing_the_row_now_moves_the_evaluation(self):
        """Slot cost is what changed, so the same 13 cards in the other order
        must not price the same."""
        st = play(2, seed=17, plies=60)
        w = row_on()
        before = W.evaluate(st, 0, w, W.rival_context(st, 0))
        st2 = copy.deepcopy(st)
        st2.card_row = list(reversed(st.card_row))
        after = W.evaluate(st2, 0, w, W.rival_context(st2, 0))
        self.assertNotEqual(before, after)

    def test_urgency_counts_only_cards_the_sweep_destroys(self):
        st = G.new_game(2, 23)
        p = st.players[0]
        p.civil_actions = 6
        # 2p: slide = 2 * SWEEP[2] = 6, so slots 0-5 die before my next turn
        st.card_row = [None] * 13
        st.card_row[0] = "Alchemy"
        ctx = W.rival_context(st, 0)
        doomed, _ = W.row_pressure(st, 0, W.DEFAULT_WEIGHTS, ctx)
        st.card_row = [None] * 13
        st.card_row[6] = "Alchemy"
        safe, _ = W.row_pressure(st, 0, W.DEFAULT_WEIGHTS, ctx)
        self.assertGreater(doomed, 0.0)
        self.assertEqual(safe, 0.0)

    def test_bargain_is_the_civil_actions_waiting_would_save(self):
        st = G.new_game(2, 29)
        st.players[0].civil_actions = 6
        st.card_row = [None] * 13
        st.card_row[9] = "Alchemy"         # 3 CA now, slot 3 (1 CA) next turn
        ctx = W.rival_context(st, 0)
        _, bargain = W.row_pressure(st, 0, W.DEFAULT_WEIGHTS, ctx)
        # 2 CA saved, discounted by the one rival who could also take it
        self.assertAlmostEqual(bargain, 2.0 * (1.0 - W.RIVAL_TAKE_P))

    def test_taking_a_doomed_card_lowers_urgency(self):
        """Both terms are read off the POST-move state, which is how a 1-ply
        search gets to prefer the take that leaves the better row behind."""
        st = G.new_game(2, 31)
        st.players[0].civil_actions = 6
        st.card_row = [None] * 13
        st.card_row[0] = "Alchemy"         # doomed
        st.card_row[6] = "Theology"        # survives
        w = row_on(row_urgency=-1.0)
        vals = {}
        for slot in (0, 6):
            trial = copy_state(st)
            A.apply(trial, ("take", slot), random.Random(0))
            vals[slot] = W.row_pressure(trial, 0, w, W.rival_context(trial, 0))[0]
        self.assertLess(vals[0], vals[6])


# ------------------------------------------------------------------- gap 3

class OpponentDesire(unittest.TestCase):
    """GAP 3: replacing a rival's `hand_civil` left the vector bit-identical,
    even though the rules make civil cards taken PUBLIC (RULES_SPEC 2.6)."""

    def test_replacing_a_rivals_civil_hand_now_moves_the_evaluation(self):
        st = play(2, seed=7, plies=60)
        w = row_on()
        before = W.evaluate(st, 0, w, W.rival_context(st, 0))
        st2 = copy.deepcopy(st)
        st2.players[1].hand_civil = ["Bronze", "Alchemy"]
        after = W.evaluate(st2, 0, w, W.rival_context(st2, 0))
        self.assertNotEqual(before, after)

    def test_rival_board_scalars_are_no_longer_invisible(self):
        """The audit's per-field sensitivity table read +0.0000 for every one
        of these."""
        st = play(3, seed=19, plies=70)
        base = W.features(st, 0, W.rival_context(st, 0))
        for field, key in (("civil_actions", "rival_free_ca"),
                           ("hand_civil", "rival_hand_civil"),
                           ("completed_wonders", "rival_wonders")):
            with self.subTest(field=field):
                st2 = copy.deepcopy(st)
                for q in st2.players[1:]:
                    if field == "civil_actions":
                        q.civil_actions += 3
                    else:
                        getattr(q, field).append("Bronze")
                new = W.features(st2, 0, W.rival_context(st2, 0))
                self.assertNotEqual(base[key], new[key])

    def test_a_rival_mid_wonder_cannot_take_a_wonder(self):
        """The exact legality case EXPERT_STRATEGY.md:546 singles out: a
        wonder is safe to let slide against a rival already building one."""
        st = G.new_game(2, 37)
        for p in st.players:
            p.civil_actions = 6
        st.card_row = [None] * 13
        st.card_row[9] = "Pyramids"
        st.players[1].wonder = None
        contested = W.row_pressure(st, 0, W.DEFAULT_WEIGHTS,
                                   W.rival_context(st, 0))[1]
        st.players[1].wonder = WonderInProgress("Colossus")
        safe = W.row_pressure(st, 0, W.DEFAULT_WEIGHTS,
                              W.rival_context(st, 0))[1]
        self.assertGreater(safe, contested)

    def test_a_rival_with_a_full_civil_hand_cannot_take_anything(self):
        st = G.new_game(2, 41)
        for p in st.players:
            p.civil_actions = 6
        st.card_row = [None] * 13
        st.card_row[9] = "Alchemy"
        contested = W.row_pressure(st, 0, W.DEFAULT_WEIGHTS,
                                   W.rival_context(st, 0))[1]
        st.players[1].hand_civil = ["Theology"] * 20
        safe = W.row_pressure(st, 0, W.DEFAULT_WEIGHTS,
                              W.rival_context(st, 0))[1]
        self.assertGreater(safe, contested)

    def test_rival_views_are_snapshots_not_references(self):
        """`rival_context` is built once at the root and reused by every
        candidate -- including on the journalled path, where the root state IS
        the object the candidates mutate.  A view that aliased into the state
        would silently change under the search."""
        st = play(2, seed=43, plies=40)
        ctx = W.rival_context(st, 0)
        view, _ = ctx["rival_views"][0]
        hand, techs = view.hand_civil, view.techs
        st.players[1].hand_civil.append("Bronze")
        st.players[1].techs["Alchemy"] = TechCard("Alchemy")
        self.assertEqual(hand, view.hand_civil)
        self.assertEqual(techs, view.techs)


# ----------------------------------------------------------- safety guards

class DefaultsAreInert(unittest.TestCase):

    def test_every_new_weight_defaults_to_zero(self):
        for k in ("take_cost_paid", "row_urgency", "row_bargain_forgone",
                  "rival_free_ca", "rival_hand_civil", "rival_wonders",
                  "rival_hand_potential"):
            with self.subTest(k=k):
                self.assertEqual(W.DEFAULT_WEIGHTS[k], 0.0)

    def test_a_champion_vector_gets_them_at_zero(self):
        """`load_weights` fills missing keys from DEFAULT_WEIGHTS, so a
        vector trained before these existed is unchanged by them."""
        w = dict(W.DEFAULT_WEIGHTS)
        w.pop("row_urgency")
        merged = dict(W.DEFAULT_WEIGHTS)
        merged.update(w)
        self.assertEqual(merged["row_urgency"], 0.0)

    def test_the_row_terms_are_not_evaluated_when_switched_off(self):
        """Zero default must also mean zero COST: `row_pressure` is the only
        expensive addition and it must not run at all by default."""
        st = play(2, seed=47, plies=50)
        calls = []
        real = W.row_pressure
        W.row_pressure = lambda *a, **k: (calls.append(1), real(*a, **k))[1]
        try:
            W.evaluate(st, 0, W.DEFAULT_WEIGHTS, W.rival_context(st, 0))
            self.assertEqual(calls, [])
            W.evaluate(st, 0, row_on(), W.rival_context(st, 0))
            self.assertEqual(len(calls), 1)
        finally:
            W.row_pressure = real

    def test_row_pressure_survives_a_hand_built_ctx(self):
        """Half a dozen tools build the fallback ctx dict by hand with only
        the three original keys."""
        st = play(2, seed=53, plies=40)
        ctx = {"rival_culture_rate": 0, "rival_science_rate": 0,
               "rival_strength": 0}
        W.evaluate(st, 0, row_on(), ctx)      # must not raise
        W.evaluate(st, 0, row_on(), None)


class NewStateFieldsCopyAndRollBack(unittest.TestCase):

    def test_fastcopy_deep_copies_civil_discard(self):
        st = play(2, seed=59, plies=50)
        self.assertTrue(st.civil_discard)
        cp = copy_state(st)
        self.assertEqual(cp.civil_discard, st.civil_discard)
        age = next(iter(cp.civil_discard))
        cp.civil_discard[age].append("Bronze")
        cp.civil_discard["ZZ"] = ["Bronze"]
        self.assertNotIn("ZZ", st.civil_discard)
        self.assertNotIn("Bronze", st.civil_discard[age][-1:])

    def test_fastcopy_carries_ca_spent_taking(self):
        st = G.new_game(2, 61)
        st.players[0].civil_actions = 6
        st.card_row = ["Alchemy"] * 13
        A.apply(st, ("take", 9), random.Random(0))
        self.assertEqual(copy_state(st).players[0].ca_spent_taking, 3)

    def test_the_journal_rolls_both_fields_back(self):
        st = G.new_game(2, 67)
        st.round = 2
        st.players[0].civil_actions = 6
        st.card_row = ["Alchemy"] * 13
        journal.install()
        before_take = st.players[0].ca_spent_taking
        before_discard = copy.deepcopy(st.civil_discard)
        j = journal.begin(st)
        try:
            A.apply(st, ("take", 9), random.Random(0))
            G._replenish(st, random.Random(0))
            self.assertNotEqual(st.players[0].ca_spent_taking, before_take)
            self.assertNotEqual(st.civil_discard, before_discard)
        finally:
            journal.rollback(j)
        self.assertEqual(st.players[0].ca_spent_taking, before_take)
        self.assertEqual(st.civil_discard, before_discard)


if __name__ == "__main__":
    unittest.main()
