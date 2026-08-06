"""Encoder shape / determinism / legality tests (torch-free).

These run on the Mac in the normal `python3 -m unittest` suite: the encoder
has no torch dependency by design.  The neural NET tests live elsewhere and
skip when torch is absent.
"""
import unittest

from engine import game, actions
from engine.bots import neural_encode as E


class TestNeuralEncode(unittest.TestCase):
    def test_card_vec_dim_constant(self):
        self.assertEqual(len(E.card_vec("Pyramids")), E.CARD_VEC_DIM)
        self.assertEqual(len(E.card_vec(None)), E.CARD_VEC_DIM)
        self.assertEqual(len(E.card_vec("no such card")), E.CARD_VEC_DIM)
        # a None/unknown card is the zero vector
        self.assertTrue(all(x == 0.0 for x in E.card_vec(None)))

    def test_fixed_length_across_player_counts(self):
        for n in (2, 3, 4):
            for seed in (0, 1, 2):
                st = game.new_game(n, seed=seed)
                for idx in range(n):
                    v = E.encode(st, idx)
                    self.assertEqual(len(v), E.ENCODING_DIM, (n, seed, idx))
                    self.assertTrue(all(isinstance(x, float) for x in v))

    def test_length_stable_through_a_game(self):
        # advance a real game a few dozen plies and keep checking the length
        from engine.bots import WeightedBot
        bots = [WeightedBot(seed=1), WeightedBot(seed=2)]
        st = game.new_game(2, seed=7)
        import random
        rng = random.Random(0)
        for _ in range(120):
            if st.game_over:
                break
            moves = actions.legal_moves(st)
            if not moves:
                break
            mv = bots[st.decider()].pick(st, moves)
            v = E.encode(st, st.decider())
            self.assertEqual(len(v), E.ENCODING_DIM)
            actions.apply(st, mv, rng)

    def test_deterministic(self):
        st = game.new_game(3, seed=11)
        self.assertEqual(E.encode(st, 0), E.encode(st, 0))

    def test_government_civil_military_grant_is_encoded(self):
        # Regression for the 2026-08-05 fix: a government's civil/military
        # action grant lives in TOP-LEVEL card fields (`civilActions`/
        # `militaryActions`), not inside the generic `effects` dict --
        # `card_vec` used to silently read 0.0 for every government because
        # it only ever looked in `effects`. Despotism grants civilActions=4,
        # militaryActions=2; EFF_KEYS index 0/1 sit right after the type
        # one-hot (23) + level (1) + PROD_KEYS (6) = offset 30/31.
        v = E.card_vec("Despotism")
        # EFF_KEYS starts right after the type one-hot, the level slot and
        # PROD_KEYS -- computed directly rather than importing private
        # constants, matching this test file's existing style.
        eff_start = 23 + 1 + 6
        self.assertGreater(v[eff_start + 0], 0.0, "civilActions must not be the silent-zero bug")
        self.assertGreater(v[eff_start + 1], 0.0, "militaryActions must not be the silent-zero bug")
        self.assertAlmostEqual(v[eff_start + 0], 4.0 / 4.0)
        self.assertAlmostEqual(v[eff_start + 1], 2.0 / 4.0)

    def test_territory_permanent_effects_are_encoded(self):
        # Regression for the 2026-08-05 fix: a colonization territory prints
        # its permanent yellow/blue-token and strength grant in
        # `permanentEffects`, not the generic `effects` dict -- same shape as
        # the government fix above.
        eff_start = 23 + 1 + 6
        v = E.card_vec("Vast Territory (I)")
        self.assertNotEqual(v[eff_start + 12], 0.0, "yellowTokens must not be the silent-zero bug")
        self.assertNotEqual(v[eff_start + 13], 0.0, "blueTokens must not be the silent-zero bug")

        sv = E.card_vec("Strategic Territory (I)")
        self.assertNotEqual(sv[eff_start + 7], 0.0, "permanentEffects.strength must not be the silent-zero bug")

        hv = E.card_vec("Historic Territory (II)")
        # "happiness" in the data, EFF_KEYS index 6 ("happy") -- a key-NAME
        # mismatch on top of the dict-location mismatch the other three fix.
        self.assertNotEqual(hv[eff_start + 6], 0.0, "permanentEffects.happiness must not be the silent-zero bug")

    def test_row_cost_matches_engine(self):
        self.assertEqual(tuple(E._ROW_COST), tuple(actions.ROW_COST))

    def test_describe_consistent(self):
        d = E.describe()
        self.assertEqual(
            d["encoding_dim"],
            d["global_dim"] + d["row_dim"]
            + d["max_players"] * d["player_block_dim"])
        st = game.new_game(2, seed=3)
        self.assertEqual(len(E.encode(st, 0)), d["encoding_dim"])

    def test_does_not_leak_civil_deck_order(self):
        # Shuffling the hidden civil-deck ORDER must not change the encoding:
        # the encoder is only allowed to read the deck's SIZE (via rounds_left),
        # never its order.  (Card identities in the visible ROW are public and
        # ARE encoded; those are untouched here.)
        import random
        st = game.new_game(2, seed=4)
        # advance a little so decks are non-trivial
        from engine.bots import WeightedBot
        bots = [WeightedBot(seed=1), WeightedBot(seed=2)]
        rng = random.Random(0)
        for _ in range(20):
            if st.game_over:
                break
            moves = actions.legal_moves(st)
            mv = bots[st.decider()].pick(st, moves)
            actions.apply(st, mv, rng)
        before = E.encode(st, st.decider())
        random.Random(99).shuffle(st.civil_deck)
        random.Random(98).shuffle(st.military_deck)
        after = E.encode(st, st.decider())
        self.assertEqual(before, after)


class TestDiscardPilesAreEncoded(unittest.TestCase):
    """Card counting is legal (Paul, 2026-07-31): both discard piles are
    public, so the encoder reads them.  `docs/INFORMATION_AUDIT.md` GAP 5."""

    def _advanced(self, plies=140):
        import random
        from engine.bots import WeightedBot
        bots = [WeightedBot(seed=1), WeightedBot(seed=2)]
        st = game.new_game(2, seed=7)
        rng = random.Random(0)
        for _ in range(plies):
            if st.game_over:
                break
            moves = actions.legal_moves(st)
            if not moves:
                break
            actions.apply(st, bots[st.decider()].pick(st, moves), rng)
        return st

    def test_the_block_is_the_right_size_and_in_range(self):
        st = self._advanced()
        block = E._discard_block(st)
        self.assertEqual(len(block), 2 * len(E.C.AGES))
        for x in block:
            self.assertGreaterEqual(x, 0.0)
            self.assertLessEqual(x, 1.0)

    def test_emptying_the_civil_discard_changes_the_encoding(self):
        """The negative control for the whole exposure: if this passes with
        the piles blanked, nothing is actually reading them."""
        st = self._advanced()
        self.assertTrue(st.civil_discard, "no civil sweep happened at all")
        before = E.encode(st, 0)
        st.civil_discard = {}
        self.assertNotEqual(E.encode(st, 0), before)

    def test_emptying_the_military_discard_changes_the_encoding(self):
        st = self._advanced()
        self.assertTrue(st.discarded_military, "nothing was ever discarded")
        before = E.encode(st, 0)
        st.discarded_military = {}
        self.assertNotEqual(E.encode(st, 0), before)

    def test_both_players_see_the_same_piles(self):
        """Public means public: it is not a per-viewpoint field."""
        st = self._advanced()
        self.assertEqual(E._discard_block(st), E._discard_block(st))
        a, b = E.encode(st, 0), E.encode(st, 1)
        tail = 2 * len(E.C.AGES)
        head = E._GLOBAL_DIM - tail
        self.assertEqual(a[head:E._GLOBAL_DIM], b[head:E._GLOBAL_DIM])


if __name__ == "__main__":
    unittest.main()
