"""Pinning tests for two `engine/bots/book.py` fixes: `_r_play_leader` was
reading the bare v1 `LEADER_RANK` table even under `version=2` (every OTHER
leader-ranking call site goes through the version-aware `_leader_rank`
helper), and Winston Churchill's once-per-turn choice was never referenced
by any of the 12 action-phase rules, so neither bot version ever selected
it even though it costs no civil or military action.  Both are fixed in
`engine/bots/book.py` and mirrored in `rust/src/bots/book.rs` -- see that
module's own doc comment.

These tests drive `BookBot.choose` with a hand-CRAFTED move list rather
than a fully legal `actions.legal_moves(state)` result.  `_action_phase`
only ever groups moves by `m[0]` and looks each kind up with `by_kind.get`,
so a move list that only contains the kinds under test is sufficient to
isolate the rule being pinned -- every other rule safely falls through to
`None` on a missing key, exactly as it would if `legal_moves` had genuinely
found nothing else to do this decision.
"""
from __future__ import annotations

import os
import sys
import unittest

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

from engine import game  # noqa: E402
from engine.bots.book import BookBot  # noqa: E402


def _turn(leader=None, seed=21):
    """A round-3 state sitting in P0's action phase."""
    st = game.new_game(2, seed=seed)
    st.round = 3
    st.phase = "actions"
    st.has_military = True
    p = st.players[0]
    p.leader = leader
    p.politics_done = True
    return st, p


# ======================================================================
# _r_play_leader: version-aware leader ranking
# ======================================================================

class PlayLeaderUsesTheVersionAwareTable(unittest.TestCase):
    """v1's `LEADER_RANK` ranks Julius Caesar (8) well above Alexander the
    Great (5); v2's tournament-derived `V2_LEADER_RANK` says the opposite
    -- Caesar 3.0 ("in the new TTA he is terrible"), Alexander 5.5.  A
    version=2 bot must pick from the v2 table like every other leader
    decision it makes, not fall back to the v1 opinion table just because
    this one rule forgot to route through `_leader_rank`."""

    CANDIDATES = [("play_leader", "Julius Caesar"), ("play_leader", "Alexander the Great")]

    def test_v1_picks_caesar(self):
        st, _p = _turn()
        bot = BookBot(version=1, seed=1)
        self.assertEqual(bot.choose(st, self.CANDIDATES), ("play_leader", "Julius Caesar"))

    def test_v2_picks_alexander_not_caesar(self):
        """Before the fix this returned Caesar too -- `_r_play_leader` read
        `LEADER_RANK` (v1) directly regardless of `ctx.version`."""
        st, _p = _turn()
        bot = BookBot(version=2, seed=1)
        self.assertEqual(bot.choose(st, self.CANDIDATES), ("play_leader", "Alexander the Great"))

    def test_v2_upgrade_threshold_also_uses_the_v2_table(self):
        """Replacing an already-played leader requires a >= 2 point gain on
        whichever table is in force.  Michelangelo is v1's best leader (9)
        but v2's worst (2.0, "last in every list found"); with Caesar (v2
        rank 3.0) already played, v2 must NOT treat Michelangelo as the
        upgrade v1 would."""
        st, _p = _turn(leader="Julius Caesar")
        bot = BookBot(version=2, seed=1)
        # A second move is needed so `choose()`'s "only one legal move"
        # shortcut does not short-circuit before `_r_play_leader` runs.
        mv = bot.choose(st, [("play_leader", "Michelangelo"), ("end_turn",)])
        self.assertEqual(mv, ("end_turn",), "v2 should not swap into a leader it ranks worse")


# ======================================================================
# Winston Churchill's once-per-turn choice
# ======================================================================

class ChurchillsFreeBonusIsAlwaysTaken(unittest.TestCase):
    """`_h_churchill` never decrements `p.civil_actions`/`p.military_actions`
    -- the move costs nothing but the once-per-turn flag itself, which is
    lost for the turn whether spent or not.  Never taking a free,
    no-downside bonus was a plain bug, not a strategy call."""

    MOVES = [("churchill", "culture"), ("churchill", "military"), ("end_turn",)]

    def test_taken_over_end_turn_when_nothing_else_is_available(self):
        st, _p = _turn(leader="Winston Churchill")
        for version in (1, 2):
            bot = BookBot(version=version, seed=1)
            self.assertEqual(bot.choose(st, self.MOVES), ("churchill", "culture"),
                              f"version={version}")

    def test_culture_is_preferred_over_military(self):
        """The military flavour's discount is wiped at end of turn if
        unused; culture is unconditional value.  Choosing WHICH flavour to
        take when a military purchase is imminent is an open strategy
        question (not resolved here) -- this only pins the safe default."""
        bot = BookBot(version=1, seed=1)
        by_kind = {"churchill": [("churchill", "culture"), ("churchill", "military")]}
        self.assertEqual(bot._r_churchill(None, None, None, by_kind), ("churchill", "culture"))

    def test_absent_when_not_offered(self):
        bot = BookBot(version=1, seed=1)
        self.assertIsNone(bot._r_churchill(None, None, None, {}))


if __name__ == "__main__":
    unittest.main()
