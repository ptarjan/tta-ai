"""The census must not be able to change how the bots play.

WHY THIS TEST CARRIES MORE WEIGHT THAN USUAL.  `engine.census.record` is
called from inside the hot path of `PlanBot.pick` and `QuiescentBot.pick` --
the two searches the league actually trains -- and the eight-fingerprint
replay (`tools/gate.sh`) that used to be this repo's regression net for
exactly this class of change is no longer run (see the validation note in
docs/SYSTEM_COVERAGE.md).  So this test IS the net.  If it goes red, the
instrument is altering the thing it measures and must not ship.

It plays identical seeds with the census off and then on, and requires the
final scores to match exactly.  Scores are a whole-game observable: any
diverged decision, any consumed RNG draw, any mutated state anywhere in the
search shows up in them.
"""

import os
import sys
import tempfile

sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))

from engine import census, game                                # noqa: E402
from experiments import arena                                  # noqa: E402


def _play(spec_text, players, seed):
    spec = arena.load_spec(spec_text)
    bots = [arena.make_bot(spec, seed * 131 + i) for i in range(players)]
    st = game.play_game(bots, num_players=players, seed=seed)
    return tuple(sorted(p.culture for p in st.players))


def _with_census(spec_text, players, seed, dest):
    """Same game, census switched on the way the league switches it on."""
    from tools import war_census as wc
    prev_enabled, prev_impl, prev_loaded = (
        census.ENABLED, census._impl, census._loaded)
    prev_out = wc._OUT
    try:
        wc._OUT = None
        wc.open_sink_for_process(dest)
        census.ENABLED = True
        census._loaded = True
        census._impl = wc.record_decision
        return _play(spec_text, players, seed)
    finally:
        if wc._OUT is not None:
            wc._OUT.close()
        wc._OUT = prev_out
        census.ENABLED, census._impl, census._loaded = (
            prev_enabled, prev_impl, prev_loaded)


def _check(spec_text, players):
    with tempfile.TemporaryDirectory() as d:
        for seed in (4001, 4002):
            off = _play(spec_text, players, seed)
            on = _with_census(spec_text, players, seed, d)
            assert off == on, (
                "census changed play: spec=%s seed=%d off=%r on=%r"
                % (spec_text, seed, off, on))
        wrote = [f for f in os.listdir(d) if f.endswith(".jsonl")]
        assert wrote, "census was on but recorded nothing -- inert for the "
        "wrong reason, so the comparison above proves nothing"
        assert any(os.path.getsize(os.path.join(d, f)) > 0 for f in wrote), (
            "census sink exists but is empty; see above")


def test_plan_bot_play_is_identical_with_census_on():
    _check("plan:default,width=2", 2)


def test_quiescent_bot_play_is_identical_with_census_on():
    _check("quiesce:default,levels=1", 3)


def test_census_is_off_by_default():
    """The default import must cost nothing and record nothing."""
    assert census.ENABLED is False or bool(os.environ.get("TTA_WAR_CENSUS"))
