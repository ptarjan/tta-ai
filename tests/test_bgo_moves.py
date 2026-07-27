"""Regression tests for tools/bgo_moves.py, the move-level BGO replayer.

Every case below is a bug that cost real supervision while
`docs/BEHAVIOUR_CLONE.md` was being measured, and every one of them was
silent: a broken replayer does not raise, it moves a clean-turn percentage.
"""
import os
import sys
import tempfile
import unittest

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

from engine import actions                                    # noqa: E402
from tools import bgo_moves as M                              # noqa: E402


def journal(rows):
    """rows: (colour, age, round, text) tuples."""
    fh = tempfile.NamedTemporaryFile("w", suffix=".tsv", delete=False)
    fh.write("date\tplayer_colour\tage\tround\ttext\n")
    for colour, age, rnd, text in rows:
        fh.write("2026-01-01 00:00:00\t%s\t%s\t%s\t%s\n"
                 % (colour, age, rnd, text))
    fh.close()
    return fh.name


END = ("End turn %s scores:; ; %d culture (now %d); %d science (now %d); "
       "%d food - consumption: %d (now %d); %d resources (now %d)")


def end(colour, cul=0, culn=0, sci=1, scin=1, food=2, cons=0, foodn=2,
        res=2, resn=2):
    return END % (colour, cul, culn, sci, scin, food, cons, foodn, res, resn)


def run(rows, collect=False):
    path = journal(rows)
    try:
        r = M.Replay(path, collect=collect)
        r.run()
        return r
    finally:
        os.unlink(path)


class NameResolution(unittest.TestCase):
    """`engine.cards._disambiguate` renames every card whose name repeats.

    The journal prints `Frugality`; the deck holds `Frugality (A)`,
    `Frugality (I)` and `Frugality (II)`.  Before this was handled, EVERY take
    of one of the ~15 repeated action-card names failed the `name in
    _DB.by_name` check and dirtied its turn -- which is most of the corpus's
    most-taken cards (`docs/HUMAN_BASELINE.md` trap 3).
    """

    def test_bare_name_resolves_to_an_age_variant(self):
        self.assertNotIn("Frugality", M._DB.by_name)
        self.assertIn(M.resolve("Frugality", age="II"),
                      ("Frugality (A)", "Frugality (I)", "Frugality (II)"))

    def test_context_beats_the_age_default(self):
        got = M.resolve("Urban Growth", ["Urban Growth (III)"], age="I")
        self.assertEqual(got, "Urban Growth (III)")

    def test_unique_names_pass_through(self):
        self.assertEqual(M.resolve("Bronze", age="A"), "Bronze")


class TurnSegmentation(unittest.TestCase):
    """A turn's round comes off its `End turn` row, not its first row.

    The row that OPENS a seat's turn is often a cross-player consequence line
    left over from the previous round (an event's `<P> produces 2 resources`),
    so keying the turn's round on it interleaves the seats wrongly and hands
    round 1's one-civil-action budget to a round 2 turn.
    """

    def test_round_comes_from_the_end_turn_row(self):
        rows = [("Orange", "A", "1", "Orange produces 2 resources"),
                ("Orange", "I", "2", "Orange takes Bronze in hand "
                                     "Orange uses 1 civil action"),
                ("Orange", "I", "2", end("Orange")),
                ("Purple", "A", "1", end("Purple"))]
        _order, turns = M.parse_turns(journal(rows))
        by_colour = {t.colour: t for t in turns}
        self.assertEqual(by_colour["Orange"].round, 2)
        self.assertEqual(by_colour["Purple"].round, 1)

    def test_seats_are_ordered_by_round_then_seat(self):
        rows = []
        for rnd in (1, 2):
            for c in ("Orange", "Purple"):
                rows.append((c, "A", str(rnd), end(c)))
        _order, turns = M.parse_turns(journal(rows))
        self.assertEqual([(t.colour, t.round) for t in turns],
                         [("Orange", 1), ("Purple", 1),
                          ("Orange", 2), ("Purple", 2)])


class RowImputation(unittest.TestCase):
    """The card row is never printed; the take's CA cost pins only its TIER.

    Injecting every unseen card at the FIRST slot of its tier was worth ~9
    points to a "take the leftmost legal card" baseline -- entirely as an
    artefact of this function, and enough to make that baseline beat the
    fitted vector.  The slot must be uniform inside the band.
    """

    def test_injected_card_lands_in_the_right_cost_tier(self):
        r = M.Replay.__new__(M.Replay)
        r.stat = M.Counter()
        r.rng = M.random.Random(0)
        r.state = type("S", (), {})()
        r.state.card_row = ["Bronze"] * actions.ROW_SIZE
        r.state.civil_deck = []
        for tier in (1, 2, 3):
            i = M.Replay._inject_row(r, "Philosophy", tier)
            self.assertEqual(actions.ROW_COST[i], tier)

    def test_slot_inside_the_tier_is_not_always_the_leftmost(self):
        seen = set()
        for seed in range(40):
            r = M.Replay.__new__(M.Replay)
            r.stat = M.Counter()
            r.rng = M.random.Random(seed)
            r.state = type("S", (), {})()
            r.state.card_row = ["Bronze"] * actions.ROW_SIZE
            r.state.civil_deck = []
            seen.add(M.Replay._inject_row(r, "Philosophy", 1))
        self.assertGreater(len(seen), 1, "injection is deterministic in-band")


class Decisions(unittest.TestCase):
    def test_end_turn_is_recorded_as_a_decision(self):
        """Without this the training set has `end_turn` as a candidate in
        every example and never as an answer, and the fitted vector learns
        never to stop acting."""
        rows = [("Orange", "A", "1", "Orange takes Bronze in hand "
                                     "Orange uses 1 civil action"),
                ("Orange", "A", "1", end("Orange")),
                ("Purple", "A", "1", end("Purple")),
                ("Orange", "I", "2", "Orange increases population "
                                     "Orange spends 2 food"),
                ("Orange", "I", "2", end("Orange")),
                ("Purple", "I", "2", end("Purple"))]
        r = run(rows)
        self.assertGreaterEqual(r.stat["legal:end_turn"], 1)

    def test_a_human_move_our_engine_calls_illegal_is_counted(self):
        rows = [("Orange", "A", "1", "Orange builds Bronze "
                                     "Orange spends 2 resources"),
                ("Orange", "A", "1", end("Orange")),
                ("Purple", "A", "1", end("Purple"))]
        r = run(rows)
        # round 1 is takes-only (RULES_SPEC 1.9), so this build is illegal and
        # must be visible in the stats rather than silently dropped
        self.assertEqual(r.stat["illegal:build"], 1)

    def test_an_illegal_move_is_still_forced_onto_the_tableau(self):
        """One bad reconstruction must not poison the rest of the game: the
        turn is dirty either way, but the tableau has to keep matching BGO or
        every later production check fails too."""
        rows = [("Orange", "A", "1", "Orange builds Bronze "
                                     "Orange spends 2 resources"),
                ("Orange", "A", "1", end("Orange")),
                ("Purple", "A", "1", end("Purple"))]
        r = run(rows)
        self.assertEqual(r.state.players[0].techs["Bronze"].workers, 3)


class Ledger(unittest.TestCase):
    def test_stocks_are_resynced_from_the_journal(self):
        rows = [("Orange", "A", "1", end("Orange", cul=3, culn=7, sci=2,
                                         scin=9, food=1, cons=0, foodn=4,
                                         res=2, resn=5)),
                ("Purple", "A", "1", end("Purple"))]
        r = run(rows)
        p = r.state.players[0]
        self.assertEqual((p.culture, p.science, p.food, p.resources),
                         (7, 9, 4, 5))

    def test_colonisation_returns_sacrificed_units_to_the_bank(self):
        """A sacrificed unit's yellow token goes to the BANK, not the unused
        pool (RULES_SPEC 11.3).  Missing it cost 1-4 bank tokens per
        colonising player, which moves the consumption band and fails the
        food check for the rest of the game."""
        rows = [("Orange", "I", "2", "Orange colonizes a Inhabited Territory "
                                     "Sacrificed Units:; 1 Warrior; "
                                     "1 Colonization card +1"),
                ("Orange", "I", "2", end("Orange")),
                ("Purple", "I", "2", end("Purple"))]
        r = run(rows)
        p = r.state.players[0]
        self.assertEqual(p.techs["Warriors"].workers, 0)
        # 18 at setup + 1 sacrificed Warrior + the territory's own 2 grants
        self.assertEqual(p.yellow_bank, 21)
        self.assertEqual(p.colonies, ["Inhabited Territory (I)"])

    def test_corruption_is_read_off_the_end_turn_line(self):
        text = ("End turn Orange scores:; CORRUPTION! Orange loses 2 resources"
                "; 1 culture (now 3); 2 science (now 6); 1 food - "
                "consumption: 1 (now 3); 3 resources (now 5)")
        _order, turns = M.parse_turns(journal(
            [("Orange", "I", "4", text)]))
        self.assertEqual(turns[0].corruption, 2)


class Emission(unittest.TestCase):
    def test_serialised_example_is_a_delta_against_candidate_zero(self):
        rows = [("Orange", "A", "1", "Orange takes Bronze in hand "
                                     "Orange uses 1 civil action"),
                ("Orange", "A", "1", end("Orange")),
                ("Purple", "A", "1", end("Purple"))]
        r = run(rows, collect=True)
        self.assertTrue(r.examples)
        ex = r.examples[0]
        self.assertEqual(ex["u"][ex["c"][0]], [])       # candidate 0 is zero
        self.assertEqual(len(ex["c"]), ex["n"])
        self.assertLess(ex["y"], ex["n"])
        for flat in ex["u"]:
            self.assertEqual(len(flat) % 2, 0)
            for i in flat[::2]:
                self.assertLess(int(i), len(M.PARAMS))


if __name__ == "__main__":
    unittest.main()
