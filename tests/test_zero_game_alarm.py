"""The zero-game alarm: a generation in which EVERY game died must halt.

The failure mode (docs/HAZARDS.md trap 8).  `arena._play` catches every
exception per game, deliberately, so one engine bug cannot kill a 40-hour
tournament.  The price is that a bug which kills EVERY game is invisible:
`hillclimb_league.py` kept proposing mutants against zero completed games,
accepted nothing, logged nothing unusual, and burned hours producing a
generation log with no data in it.  It has cost this project real time.

Every test here is a matched pair, because the only interesting property of a
guard is that it FIRES.  A test that the happy path stays quiet proves the
alarm is not deafening; it does not prove the alarm exists.  So each pair is

    negative control -- break every game, require the alarm
    positive control -- do not break anything, require silence

The negative control breaks games the way the real bug did: `engine.game`'s
`play_game` raises.  `arena._play` looks the function up on the module at call
time, so patching the module attribute is exactly the observable an engine bug
would present, without a fake arena in the middle.
"""
import contextlib
import json
import os
import shutil
import sys
import tempfile
import unittest

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

from engine import game                                       # noqa: E402
from experiments import arena                                  # noqa: E402
from experiments import hillclimb_league as L                   # noqa: E402


@contextlib.contextmanager
def every_game_raises(exc=None):
    """Make `engine.game.play_game` raise, i.e. kill every game arena plays.

    `exc` may be a callable taking the seed, so one run can produce several
    distinct exception types.
    """
    saved = game.play_game

    def boom(bots, n, seed=0, move_cap=20000):
        raise (exc(seed) if callable(exc)
               else (exc or RuntimeError("engine is broken")))

    game.play_game = boom
    try:
        yield
    finally:
        game.play_game = saved


class ArenaReportsWhatDied(unittest.TestCase):
    """Arm 1: arena must hand the CALLER a diagnosable census, not a count."""

    def test_negative_control_every_game_dies_with_a_repr(self):
        with every_game_raises(ValueError("root_row is not defined")):
            res = arena.duel("greedy", "greedy", 2, 4, workers=1)
        self.assertEqual(res["games"], 0)          # nothing completed
        self.assertEqual(res["errors"], 4)         # all four died
        self.assertEqual(res["requested"], 4)
        self.assertEqual(list(res["error_types"]), ["ValueError"])
        rec = res["error_types"]["ValueError"]
        self.assertEqual(rec["count"], 4)
        # The repr is the deliverable: a bare count is not actionable.
        self.assertIn("root_row is not defined", rec["repr"])
        # ... and so are the frame that raised and a seed that reproduces it.
        self.assertIn("test_zero_game_alarm.py:", rec["where"])
        self.assertIsInstance(rec["seed"], int)
        # The old string-list contract still holds for existing consumers
        # (experiments/hillclimb.py stores it in its generation record).
        self.assertTrue(all(isinstance(s, str) for s in res["error_sample"]))
        self.assertIn("root_row", res["error_sample"][0])
        self.assertIn("ValueError", arena.fmt(res))

    def test_distinct_types_are_counted_separately(self):
        """Two bugs at once must not be reported as one bug."""
        def pick(seed):
            return (KeyError("Alchemy") if seed % 2 else TypeError("nope"))

        with every_game_raises(pick):
            res = arena.duel("greedy", "greedy", 2, 6, workers=1)
        types = res["error_types"]
        self.assertEqual(sorted(types), ["KeyError", "TypeError"])
        self.assertEqual(sum(v["count"] for v in types.values()), 6)
        self.assertIn("Alchemy", types["KeyError"]["repr"])
        brief = arena.error_brief(types)
        self.assertIn("KeyError", brief)
        self.assertIn("TypeError", brief)

    def test_positive_control_healthy_duel_reports_nothing(self):
        res = arena.duel("random", "random", 2, 2, workers=1)
        self.assertEqual(res["games"], 2)
        self.assertEqual(res["errors"], 0)
        self.assertEqual(res["error_types"], {})
        self.assertEqual(res["error_sample"], [])
        self.assertNotIn("engine errors", arena.fmt(res))


class Tally(unittest.TestCase):
    """Arm 2: the per-generation census, and what counts as fatal."""

    def test_fatal_only_when_games_were_asked_for_and_none_came_back(self):
        t = L.DeathTally()
        self.assertFalse(t.is_fatal())            # nothing asked for yet
        t.add({"requested": 0, "games": 0, "errors": 0})
        self.assertFalse(t.is_fatal())            # an empty pool is not a death
        t.add({"requested": 4, "games": 0, "errors": 4,
               "error_types": {"ValueError": {"count": 4, "repr": "ValueError()",
                                              "where": "game.py:1", "seed": 7}}})
        self.assertTrue(t.is_fatal())
        self.assertEqual(t.death_rate, 1.0)
        t.add({"requested": 4, "games": 4, "errors": 0})
        self.assertFalse(t.is_fatal())            # one game back is not fatal
        self.assertAlmostEqual(t.death_rate, 0.5)
        self.assertIn("ValueError()", t.brief())
        self.assertEqual(t.record()["types"]["ValueError"]["count"], 4)
        t.reset()
        self.assertEqual((t.requested, t.completed, t.errors, t.types),
                         (0, 0, 0, {}))

    def test_counts_accumulate_across_duels(self):
        t = L.DeathTally()
        for _ in range(3):
            t.add({"requested": 2, "games": 1, "errors": 1,
                   "error_types": {"KeyError": {"count": 1, "repr": "KeyError('x')",
                                                "where": "actions.py:9", "seed": 1}}})
        self.assertEqual((t.requested, t.completed, t.errors), (6, 3, 3))
        self.assertEqual(t.types["KeyError"]["count"], 3)
        self.assertGreaterEqual(t.death_rate, L.HIGH_DEATH_RATE)


class Halt(unittest.TestCase):
    """Arm 3: the halt is loud, it names the bug, and the watchdog cannot
    silently undo it."""

    def setUp(self):
        self.tmp = tempfile.mkdtemp(prefix="zga")
        self.stop = L.stop_path(2, log_dir=self.tmp)

    def tearDown(self):
        shutil.rmtree(self.tmp, ignore_errors=True)

    def test_sentinel_names_the_exception_and_the_remedy(self):
        t = L.DeathTally()
        t.add({"requested": 8, "games": 0, "errors": 8,
               "error_types": {"NameError": {"count": 8,
                                             "repr": "NameError('ctx')",
                                             "where": "plan.py:412", "seed": 3}}})
        lines = []
        msg = L.halt_dead_generation(2, 91, t, self.stop, log=lines.append)
        self.assertIn("ZERO COMPLETED GAMES", msg)
        self.assertIn("NameError('ctx')", msg)
        blob = "\n".join(lines)
        self.assertIn("HALTING", blob)
        self.assertIn("plan.py:412", blob)
        with open(self.stop) as fh:
            rec = json.load(fh)
        self.assertEqual(rec["gen"], 91)
        self.assertEqual(rec["players"], 2)
        self.assertEqual(rec["deaths"]["types"]["NameError"]["count"], 8)
        self.assertEqual(rec["deaths"]["completed"], 0)
        self.assertIn("delete this file", rec["remedy"])

    def test_the_launchers_check_the_sentinel_path_the_climber_writes(self):
        """The halt only sticks if all three agree on the filename.

        `run_league.sh` restarts the climber in a loop and `watchdog.sh`
        relaunches the supervisor from cron every 10 minutes, so a halt that
        is merely an exit code is undone within 10 minutes.  Both scripts test
        for this exact path; a rename in `stop_path` that missed them would
        leave a crash-loop and no alarm.
        """
        name = os.path.basename(L.stop_path(2))
        self.assertEqual(name, "stop_league_2p.json")
        pattern = name.replace("2p", "${K}p")
        here = os.path.dirname(os.path.abspath(__file__))
        for script in ("run_league.sh", "watchdog.sh"):
            with open(os.path.join(here, "..", "experiments", script)) as fh:
                body = fh.read()
            self.assertIn(pattern, body, script)
        # ... and the directory matches too.
        self.assertEqual(os.path.basename(os.path.dirname(L.stop_path(2))),
                         "logs")


class LeagueGeneration(unittest.TestCase):
    """Arm 4, the deliverable: one generation of the real loop, with every
    game broken, must halt instead of burning the next 47 hours."""

    def setUp(self):
        self.tmp = tempfile.mkdtemp(prefix="zga_state")
        self.stop = os.path.join(self.tmp, "stop_league_2p.json")
        self.lines = []

    def tearDown(self):
        shutil.rmtree(self.tmp, ignore_errors=True)

    def _run(self, hours=1.0):
        # hours=0 runs no generation at all, which is how the resume check
        # exercises the STARTUP refusal without paying for a block of games.
        return L.run(players=2, hours=hours, workers=1, lam=1, block=2,
                     subset=1, max_gens=1, state_dir=self.tmp,
                     full_check_every=0, ablate_every=0, legacy_ladders=False,
                     human_bots=("none",), objective="winshare",
                     stop_file=self.stop, log=self.lines.append)

    def test_negative_control_a_dead_generation_halts(self):
        with every_game_raises(NameError("name 'ctx' is not defined")):
            with self.assertRaises(SystemExit) as cm:
                self._run()
        self.assertIn("ZERO COMPLETED GAMES", str(cm.exception))
        blob = "\n".join(self.lines)
        self.assertIn("ZERO COMPLETED GAMES", blob)
        self.assertIn("'ctx' is not defined", blob)   # the repr, not just a count
        self.assertIn("HALTING", blob)
        self.assertTrue(os.path.exists(self.stop))
        with open(self.stop) as fh:
            rec = json.load(fh)
        self.assertEqual(rec["deaths"]["completed"], 0)
        self.assertEqual(list(rec["deaths"]["types"]), ["NameError"])
        # The generation log records the halt, so the reason survives the
        # process exiting.
        with open(os.path.join(self.tmp, "generations_2p.jsonl")) as fh:
            last = json.loads(fh.readlines()[-1])
        self.assertTrue(last["halted"])
        self.assertEqual(last["engine_deaths"]["errors"],
                         last["engine_deaths"]["requested"])

    def test_the_halt_is_not_undone_by_a_relaunch(self):
        """A crash-halt would be relaunched forever by the cron watchdog.  The
        sentinel has to refuse the NEXT start too -- even a hand-launched one
        that never looks at the shell scripts."""
        with every_game_raises(NameError("name 'ctx' is not defined")):
            with self.assertRaises(SystemExit):
                self._run()
        # Engine "fixed"; the sentinel still stands until a human clears it.
        # Note hours=0: even a run that would play no games at all is refused,
        # because the refusal happens before any work.
        with self.assertRaises(SystemExit) as cm:
            self._run(hours=0.0)
        self.assertIn("REFUSING", str(cm.exception))
        os.remove(self.stop)
        # ... and clearing it is the whole resume procedure.
        self.assertIsInstance(self._run(hours=0.0), dict)

    def test_positive_control_a_healthy_generation_is_silent(self):
        champion = self._run()
        self.assertIsInstance(champion, dict)
        self.assertFalse(os.path.exists(self.stop))
        blob = "\n".join(self.lines)
        self.assertNotIn("ZERO COMPLETED GAMES", blob)
        self.assertNotIn("games died", blob)
        with open(os.path.join(self.tmp, "generations_2p.jsonl")) as fh:
            rows = [json.loads(x) for x in fh if x.strip()]
        self.assertTrue(rows)
        self.assertNotIn("engine_deaths", rows[-1])
        self.assertNotIn("halted", rows[-1])


if __name__ == "__main__":
    unittest.main()
