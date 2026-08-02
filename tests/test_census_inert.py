"""The census must not be able to change how the bots play.

WHY THIS TEST CARRIES MORE WEIGHT THAN USUAL.  `engine.census.record` is
called from inside the hot path of `PlanBot.pick` and `QuiescentBot.pick` --
the two searches the league actually trains -- and the eight-fingerprint
replay (`tools/gate.sh`) that used to be this repo's regression net for
exactly this class of change is no longer run (see the validation note in
docs/SYSTEM_COVERAGE.md).  So this test IS the net.  If it goes red, the
instrument is altering the thing it measures and must not ship.

WHAT IT COMPARES, AND WHY IT CHANGED (2026-08-02).  It used to play two whole
games per spec, census off and then on, and compare the FINAL SCORES.  That
cost 4m34s -- by itself more than the rest of the suite put together in
parallel -- and a final score is a weak place to look: it is one number at the
end of two hundred decisions, and a diverged decision has to survive all the
way to it to be seen.

It now compares the MOVE SEQUENCE, move by move, which fails at the first
divergent decision instead of hoping it reaches the scoreboard, and it does so
from two different starting points -- a fresh game and a cached mid-game
position -- so the early and late halves of the game are both walked.  Density
per second is higher and the failure it reports is far more useful; what it
gives up is the tail of a full game, which is bought back by
`test_no_single_decision_changes`, a paired-pick sweep over positions from
every age and every player count that no two-player game ever visited.

An RNG draw consumed inside the census is still caught: both runs of a
sequence share one seeded `random.Random`, so a stolen draw desynchronises the
rest of the sequence and the move lists stop matching.
"""

import json
import os
import random
import sys
import tempfile

sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))

from engine import actions as A, census, game                   # noqa: E402
from experiments import arena                                   # noqa: E402

import corpus                                                   # noqa: E402

SEQUENCE_PLIES = 45


def _bots(spec_text, players, seed):
    spec = arena.load_spec(spec_text)
    return [arena.make_bot(spec, seed * 131 + i) for i in range(players)]


def _play(spec_text, players, seed, start=None, plies=SEQUENCE_PLIES):
    """Play `plies` decisions and return the moves, not the outcome.

    `start` is a state to continue from (a fresh copy each call), so the same
    machinery walks the opening and the middlegame."""
    bots = _bots(spec_text, players, seed)
    st = start if start is not None else game.new_game(players, seed=seed)
    rng = random.Random(seed)
    moves = []
    for _ in range(plies):
        if st.game_over:
            break
        i = st.decider()
        if i is None:
            break
        mv = bots[i].pick(st, A.legal_moves(st))
        if mv is None:
            break
        moves.append((i, repr(mv)))
        game.apply(st, mv, rng)
    return tuple(moves)


def _with_census(fn, dest):
    """Run `fn()` with the census switched on the way the league switches it
    on, and hand back whatever it returned."""
    from tools import war_census as wc
    prev_enabled, prev_impl, prev_loaded = (
        census.ENABLED, census._impl, census._loaded)
    prev_out = wc._OUT
    prev_sample = wc._SAMPLE
    try:
        wc._OUT = None
        # Record EVERY decision, not the league's 5% sample.  Under sampling a
        # short game can legitimately emit nothing, which would let the
        # "recorded something" assertion below pass or fail by coin flip; at
        # 1.0 the hot path is also exercised on every single decision, which is
        # the path this test exists to prove inert.
        wc._SAMPLE = 1.0
        wc.open_sink_for_process(dest)
        census.ENABLED = True
        census._loaded = True
        census._impl = wc.record_decision
        return fn()
    finally:
        if wc._OUT is not None:
            wc._OUT.close()
        wc._OUT = prev_out
        wc._SAMPLE = prev_sample
        census.ENABLED, census._impl, census._loaded = (
            prev_enabled, prev_impl, prev_loaded)


def _first_difference(off, on):
    for k, (a, b) in enumerate(zip(off, on)):
        if a != b:
            return "ply %d: census off played %r, census on played %r" % (
                k, a, b)
    return "one sequence stopped early: %d moves off, %d on" % (
        len(off), len(on))


def _check(spec_text, players, starts):
    """Compare the move sequence, move by move, from each of `starts`.

    `starts` is a list of "where to begin" -- `None` for a fresh game, or a
    mid-game position.  The sequences are what is compared, not the outcome, so
    the assertion fires at the first decision that differs.
    """
    with tempfile.TemporaryDirectory() as d:
        for n, start in enumerate(starts):
            seed = 4001 + n
            # A copy PER RUN, taken before either of them plays: `_play`
            # advances the state it is handed, so reusing the object would let
            # the census run start from where the off run finished -- two
            # different games, compared, failing for the wrong reason.  (It did
            # exactly that on the first attempt, and the move-by-move report is
            # what made it obvious: the two runs disagreed at ply 0 about whose
            # turn it was.)
            first = None if start is None else corpus.copy_of(start)
            again = None if start is None else corpus.copy_of(start)
            off = _play(spec_text, players, seed, first)
            on = _with_census(
                lambda: _play(spec_text, players, seed, again), d)
            assert off == on, (
                "census changed play: spec=%s seed=%d -- %s"
                % (spec_text, seed, _first_difference(off, on)))
            assert len(off) > 5, (
                "only %d decisions were played from start %d, so the "
                "comparison proves almost nothing" % (len(off), n))
        wrote = [f for f in os.listdir(d) if f.endswith(".jsonl")]
        assert wrote, (
            "census was on but opened no sink -- inert for the wrong reason, "
            "so the comparison above proves nothing")
        # DECISION records, not just the `census_meta` header.  Every sink now
        # opens with that header, so "file is non-empty" would be satisfied by
        # a census that recorded not one decision -- which is precisely the
        # failure this assertion exists to catch.
        decisions = 0
        for f in wrote:
            with open(os.path.join(d, f)) as fh:
                for line in fh:
                    if json.loads(line).get("kind") != "census_meta":
                        decisions += 1
        assert decisions, (
            "census sink holds only its header: no decision was recorded, so "
            "the identical sequences above prove nothing; see above")


def _midgame(players, seed=21):
    """A position deep enough to be past the opening, from the shared cache."""
    pos = corpus.positions(players, seed=seed, every=40, limit=2000)
    return pos[len(pos) // 2] if len(pos) > 1 else None


def test_plan_bot_play_is_identical_with_census_on():
    _check("plan:default,width=2", 2, [None, _midgame(2)])


def test_quiescent_bot_play_is_identical_with_census_on():
    _check("quiesce:default,levels=1", 3, [None, _midgame(3)])


def test_no_single_decision_changes():
    """The wide half: one paired pick per cached position.

    The sequence tests above walk two openings and two middlegames deeply.
    This one walks shallowly but across every age and every player count the
    shared corpus holds -- positions two 2-player games never reach -- and
    requires the identical move out of each.  Cheap, because it is one pick
    per position rather than a game per position.
    """
    checked = 0
    with tempfile.TemporaryDirectory() as d:
        for players in (2, 3, 4):
            for st in corpus.positions(players, seed=21, every=25,
                                       limit=2000):
                if st.game_over or st.decider() is None:
                    continue
                moves = A.legal_moves(st)
                if len(moves) < 2:
                    continue          # nothing to choose, nothing to diverge
                i = st.decider()
                bot = _bots("plan:default,width=2", players, 99)[i]
                off = repr(bot.pick(corpus.copy_of(st), moves))
                bot = _bots("plan:default,width=2", players, 99)[i]
                on = _with_census(
                    lambda: repr(bot.pick(corpus.copy_of(st), moves)), d)
                assert off == on, (
                    "census changed a single decision at age %s with %d "
                    "players: off=%s on=%s" % (st.age_civil, players, off, on))
                checked += 1
    assert checked > 20, (
        "only %d positions offered a real choice, so this sweep is not "
        "covering what it claims to" % checked)


def test_census_is_off_by_default():
    """The default import must cost nothing and record nothing."""
    assert census.ENABLED is False or bool(os.environ.get("TTA_WAR_CENSUS"))
