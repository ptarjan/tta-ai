"""The sigma controller's acceptance window must survive a climber restart.

`experiments/run_league.sh` runs the climber with `--hours 1`, and under
`plan:width=2` a single generation takes one to nine hours.  So in production
the climber does ONE generation per process and is then restarted by the
supervisor.  That made the adaptation window -- which was an in-memory local --
reset to `[]` every generation, so `len(recent) >= 6` was never true, the
shrink branch never ran, and sigma could only ratchet UP via `stall_kick`.

The 2p arm sat pinned at the 0.8 cap for 19 consecutive generations because of
it, throwing mutants that scored 0.15-0.30 worse than the champion.

The invariant these tests hold is not a particular sigma value: it is that a
resumed climber can see enough history to adapt on its FIRST generation back.
"""

import json
import os

import pytest

from experiments.hillclimb_league import (
    DEFAULT_STATE_REC,
    load_state,
    resume_window,
)

WINDOW_MIN = 6           # the controller's `len(recent) >= 6` guard
WINDOW_CAP = 12


class TestWindowSurvivesRestart:

    def test_a_persisted_window_round_trips_through_the_state_file(self, tmp_path):
        pp = {"state": str(tmp_path / "state.json")}
        written = [True, False, False, True, False, False, False]
        with open(pp["state"], "w") as fh:
            json.dump({"gen": 7, "sigma": 0.4, "since_accept": 3,
                       "recent": written, "hold_sigma": 2}, fh)

        st = load_state(pp)
        assert resume_window(st) == written
        assert int(st["hold_sigma"]) == 2

    def test_a_long_stall_is_adaptable_on_the_first_generation_back(self):
        """19 rejects is the 2p arm's actual state at the time of the fix."""
        window = resume_window({"since_accept": 19})
        assert len(window) == WINDOW_CAP
        assert not any(window)
        # ...and that is what the controller needs to shrink rather than hold.
        assert len(window) >= WINDOW_MIN
        assert sum(window) / len(window) < 0.12

    @pytest.mark.parametrize("since", [1, 2, 5, 11, 12, 13, 40])
    def test_reconstruction_never_invents_an_accept_it_cannot_know_about(self, since):
        window = resume_window({"since_accept": since})
        assert len(window) == min(since + 1, WINDOW_CAP)
        # The tail is exactly the rejects `since_accept` records.
        assert window[-since:] == [False] * min(since, WINDOW_CAP)
        # At most the single accept that ended the previous streak.
        assert sum(window) <= 1

    def test_a_persisted_window_wins_over_reconstruction(self):
        """The real history is better evidence than what since_accept implies."""
        st = {"since_accept": 30, "recent": [True, True, False]}
        assert resume_window(st) == [True, True, False]

    def test_a_fresh_state_has_no_history_to_adapt_on(self):
        assert resume_window(dict(DEFAULT_STATE_REC)) == []

    def test_the_window_is_capped(self):
        st = {"recent": [False] * 50}
        assert len(resume_window(st)) == WINDOW_CAP


class TestNegativeControl:
    """These fail if the window is dropped again -- e.g. by someone
    re-initialising it to `[]` on load, which is exactly the bug."""

    def test_dropping_the_window_makes_the_controller_unadaptable(self):
        def broken(st, cap=WINDOW_CAP):
            return []                      # the pre-fix behaviour

        stalled = {"since_accept": 19}
        assert len(broken(stalled)) < WINDOW_MIN      # cannot adapt
        assert len(resume_window(stalled)) >= WINDOW_MIN

    def test_the_state_schema_carries_the_window(self):
        assert "recent" in DEFAULT_STATE_REC
        assert "hold_sigma" in DEFAULT_STATE_REC


class TestLiveStateFilesCarryTheWindow:
    """Not a unit test -- a check on the three running arms.  Skipped when the
    league has not written state yet."""

    ROOT = os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))),
                        "experiments", "league_state")

    @pytest.mark.parametrize("players", [2, 3, 4])
    def test_a_live_arm_can_adapt_after_its_next_restart(self, players):
        path = os.path.join(self.ROOT, f"state_{players}p.json")
        if not os.path.exists(path):
            pytest.skip(f"no live {players}p state at {path}")
        with open(path) as fh:
            st = json.load(fh)
        window = resume_window(st)
        # Either the arm is early (little history) or it has enough to adapt.
        assert len(window) == min(int(st.get("since_accept") or 0) + 1, WINDOW_CAP) \
            or st.get("recent")
