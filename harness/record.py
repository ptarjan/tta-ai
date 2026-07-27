"""The machine-readable per-game record.

Append-only JSONL, flushed after every write, so a game abandoned in round 9
still leaves everything up to round 9 on disk.

Two things in here are not book-keeping and must not be trimmed:

* **`setup`** -- player count, difficulty, and *whether the New Leaders &
  Wonders DLC was off*.  Our engine does not implement the DLC
  (`docs/EXTERNAL_AIS.md` section 1), so a DLC game is not a weak measurement,
  it is a mislabelled one.  `SetupError` refuses to open a log for a game that
  is not the game we implement.
* **`limitations`** -- repeated on the header *and* the result record, because
  whoever reads the result later may never see the header.  The load-bearing
  one is the pact bias: CGE's AI never offers a pact and refuses every pact
  offered (section 1), so the entire pact branch of our policy
  (`offer_pact` / `cancel_pact`) is dead weight in these games.  A win rate
  measured here is a win rate on a strictly smaller game than the one we train
  on, and nobody may report it otherwise.
"""

from __future__ import annotations

import json
import os
import platform
import subprocess
import time
from dataclasses import asdict, dataclass, field

SCHEMA = 1

DIFFICULTIES = ("training", "easy", "medium", "hard")
MODES = ("strict", "free")


class SetupError(ValueError):
    """The game being logged is not the game our engine implements."""


# The bias register.  Each entry says what a number from this harness does NOT
# measure.  `applies` is evaluated against the setup so that, e.g., the pact
# note is stated as structural at 3-4 players and as merely redundant at 2
# (pacts are disabled at 2 players by the rules anyway).
def _limitations(setup):
    out = [{
        "id": "no_pacts",
        "severity": "high" if setup["players"] > 2 else "low",
        "text": (
            "CGE's AI never offers a pact and refuses every pact offered, so "
            "the entire pact branch of our policy is never exercised, rewarded "
            "or punished in these games. This result measures the bot on a "
            "STRICTLY SMALLER GAME than self-play. Any pact-related weight must "
            "be validated by self-play only."
            if setup["players"] > 2 else
            "Pacts are disabled at 2 players by the rules, so the pact branch "
            "of our policy is untested here -- as it is in any 2p game."),
    }, {
        "id": "single_opponent_policy",
        "severity": "medium",
        "text": ("Every opponent is the same AI at the same difficulty, so "
                 "opponent-model variance is zero. Do not read the margin as a "
                 "population estimate."),
    }, {
        "id": "no_dlc",
        "severity": "info",
        "text": ("Base 2015 game only. New Leaders & Wonders is not "
                 "implemented by our engine and must be off."),
    }]
    if setup["mode"] == "free":
        out.append({
            "id": "free_mode",
            "severity": "high",
            "text": ("FREE MODE: the human chose the moves. The score measures "
                     "the human, not the bot. Only the override rate is a "
                     "signal about the bot."),
        })
    return out


def _git_rev():
    try:
        return subprocess.run(["git", "rev-parse", "--short", "HEAD"],
                              capture_output=True, text=True,
                              timeout=5).stdout.strip() or None
    except Exception:
        return None


@dataclass
class Setup:
    game_id: str
    players: int = 3
    seat: int = 0
    difficulty: str = "hard"
    mode: str = "strict"
    dlc: bool = False                 # New Leaders & Wonders -- MUST be False
    edition: str = "2015-base"
    src: str = "cge-app"
    app_version: str = ""
    platform_: str = ""
    weights: str = ""
    operator: str = ""
    personalities: list = field(default_factory=list)   # "world leader" AIs
    notes: str = ""

    def validate(self):
        if self.dlc:
            raise SetupError(
                "New Leaders & Wonders is ON. Our engine does not implement it "
                "(docs/EXTERNAL_AIS.md section 1); this game cannot be logged. "
                "Restart the app game with the expansion off.")
        if self.difficulty not in DIFFICULTIES:
            raise SetupError(f"difficulty must be one of {DIFFICULTIES}, "
                             f"got {self.difficulty!r}")
        if self.mode not in MODES:
            raise SetupError(f"mode must be one of {MODES}, got {self.mode!r}")
        if not 2 <= self.players <= 4:
            raise SetupError(f"players must be 2-4, got {self.players}")
        if not 0 <= self.seat < self.players:
            raise SetupError(f"seat {self.seat} is not in a "
                             f"{self.players}-player game")
        if self.personalities and self.difficulty != "hard":
            raise SetupError("'world leader' personalities are a different "
                             "experiment; label them, do not mix them into the "
                             "headline difficulty measurement")
        return self

    def to_json(self):
        d = asdict(self)
        d["platform"] = d.pop("platform_") or platform.platform()
        return d


class GameLog:
    """One game, one file.  Every write is flushed."""

    def __init__(self, setup: Setup, path, requirements=None, _now=None):
        self.setup = setup.validate()
        self.path = path
        self.now = _now or (lambda: time.strftime("%Y-%m-%dT%H:%M:%SZ",
                                                  time.gmtime()))
        self.records = []
        self.closed = False
        self.resyncs = []
        self.ply = 0
        self._fh = None
        if path:
            os.makedirs(os.path.dirname(os.path.abspath(path)) or ".",
                        exist_ok=True)
            self._fh = open(path, "a", encoding="utf-8")
        self._write({
            "v": SCHEMA, "type": "game", "id": setup.game_id,
            "started": self.now(),
            "engine_rev": _git_rev(),
            "setup": setup.to_json(),
            "limitations": _limitations(setup.to_json()),
            # what the bot could actually SEE in this game, derived from the
            # live evaluator rather than assumed (harness.fields).  Without
            # this a re-scored log is uninterpretable: you would not know
            # whether a field is absent because it did not matter or because
            # nobody typed it.
            "observables": [r.to_json() for r in (requirements or [])],
        })

    # -- writing

    def _write(self, rec):
        self.records.append(rec)
        if self._fh:
            self._fh.write(json.dumps(rec, sort_keys=True) + "\n")
            self._fh.flush()
        return rec

    def decision(self, state_text, ranked, played, source, round_, age,
                 latency_s=None, note=""):
        """One of *our* decisions.  `ranked` is the full candidate list."""
        self.ply += 1
        return self._write({
            "v": SCHEMA, "type": "decision", "game": self.setup.game_id,
            "ply": self.ply, "round": round_, "age": age,
            "actor": f"p{self.setup.seat}",
            "state": state_text,
            "ranked": ranked,
            "played": list(played) if played is not None else None,
            "source": source,          # bot | human | forced
            "latency_s": latency_s,
            "note": note,
        })

    def observed(self, round_, dealt, rivals, patches):
        """What the human reported about the shared board and the rivals.

        `patches` keeps the literal lines typed. When the mirror turns out to
        have drifted, these are the only forensic trail (section 6b).
        """
        return self._write({
            "v": SCHEMA, "type": "observed", "game": self.setup.game_id,
            "round": round_, "dealt": dealt, "rivals": rivals,
            "patches": list(patches),
        })

    def check(self, result, state_text=None):
        rec = {"v": SCHEMA, "type": "check", "game": self.setup.game_id}
        rec.update(result.to_json())
        rec["ok"] = not result.failed
        if state_text:
            rec["state"] = state_text
        return self._write(rec)

    def resync(self, round_, discrepancies, patches, cause):
        self.resyncs.append(round_)
        return self._write({
            "v": SCHEMA, "type": "resync", "game": self.setup.game_id,
            "round": round_, "cause": cause, "patches": list(patches),
            "discrepancies": [
                {"key": d.key, "expected": d.expected, "reported": d.reported,
                 "severity": d.severity, "where": d.where}
                for d in discrepancies],
        })

    # -- closing

    def result(self, scores, rounds, human_minutes=None, notes="",
               aborted=False, abort_reason="", effort=None, observables=None):
        """The footer.  `trusted` is the field aggregate analysis must filter on.

        A game that needed a resync had the bot choosing moves in a position
        that provably disagreed with the app for an unknown number of plies.
        We keep the data (it is still a real position stream) but it is not
        admissible as a strength measurement without a human looking at the
        resync records first.
        """
        scores = {str(k): v for k, v in (scores or {}).items()}
        me = f"p{self.setup.seat}"
        winner = None
        margin = None
        if scores and not aborted:
            winner = max(scores, key=lambda k: scores[k])
            others = [v for k, v in scores.items() if k != me]
            if me in scores and others:
                margin = scores[me] - max(others)
        trusted = (not aborted) and not self.resyncs and bool(scores)
        why = []
        if aborted:
            why.append(f"aborted ({abort_reason})" if abort_reason
                       else "aborted")
        if self.resyncs:
            why.append(f"mirror desynced and was resynced at round(s) "
                       f"{self.resyncs}")
        if not scores:
            why.append("no final scores recorded")
        rec = self._write({
            "v": SCHEMA, "type": "result", "game": self.setup.game_id,
            "finished": self.now(),
            "scores": scores, "winner": winner,
            "margin": margin,
            "won": (winner == me) if winner else None,
            "rounds": rounds, "decisions": self.ply,
            "human_minutes": human_minutes,
            # measured, not estimated: this is how the cost of the programme
            # gets revised after game 1 instead of after game 10
            "effort": effort or {},
            # the observable set as it stood at the END of the game -- it can
            # grow mid-game when the evaluator gains a feature
            "observables_final": observables or [],
            "aborted": aborted, "abort_reason": abort_reason,
            "resyncs": list(self.resyncs),
            "trusted": trusted,
            "untrusted_reason": "; ".join(why),
            "limitations": _limitations(self.setup.to_json()),
            "notes": notes,
        })
        self.close()
        return rec

    def close(self):
        if self._fh and not self.closed:
            self._fh.close()
        self.closed = True


# ------------------------------------------------------------ reading back


def load(path):
    out = []
    with open(path, encoding="utf-8") as fh:
        for line in fh:
            line = line.strip()
            if line:
                out.append(json.loads(line))
    return out


def summarize(paths):
    """Aggregate over game logs.  Untrusted games are counted, never pooled."""
    games, dropped = [], []
    for p in paths:
        recs = load(p)
        head = next((r for r in recs if r.get("type") == "game"), None)
        res = next((r for r in recs if r.get("type") == "result"), None)
        if not head or not res:
            dropped.append({"path": p, "why": "no result record"})
            continue
        row = {"path": p, "id": head["id"], **head["setup"],
               "margin": res.get("margin"), "won": res.get("won"),
               "rounds": res.get("rounds"),
               "human_minutes": res.get("human_minutes"),
               "effort": res.get("effort", {}),
               "limitations": res.get("limitations", []),
               "trusted": res.get("trusted"),
               "why_not": res.get("untrusted_reason", "")}
        (games if res.get("trusted") else dropped).append(row)
    margins = [g["margin"] for g in games if g.get("margin") is not None]
    wins = [g["won"] for g in games if g.get("won") is not None]
    mins = [g["human_minutes"] for g in games if g.get("human_minutes")]
    n = len(margins)
    mean = sum(margins) / n if n else None
    sd = None
    if n > 1:
        sd = (sum((m - mean) ** 2 for m in margins) / (n - 1)) ** 0.5
    # games run under different setups are different experiments; pooling them
    # is the quiet way to turn ten honest games into one dishonest number
    arms = sorted({(g["players"], g["difficulty"], g["mode"], g["dlc"],
                    tuple(g.get("personalities") or ())) for g in games})
    keys = [g["effort"].get("keystrokes") for g in games
            if g.get("effort", {}).get("keystrokes")]
    return {
        "trusted_games": len(games),
        "dropped_games": len(dropped),
        "dropped": dropped,
        "arms": [{"players": a[0], "difficulty": a[1], "mode": a[2],
                  "dlc": a[3], "personalities": list(a[4])} for a in arms],
        "poolable": len(arms) <= 1,
        "mean_margin": mean,
        "sd_margin": sd,
        "stderr_margin": (sd / n ** 0.5) if sd and n else None,
        "win_rate": (sum(1 for w in wins if w) / len(wins)) if wins else None,
        "mean_human_minutes": (sum(mins) / len(mins)) if mins else None,
        "mean_keystrokes": (sum(keys) / len(keys)) if keys else None,
        "limitations": games[0].get("limitations") if games else None,
        "caveat": ("Win rate and margin here exclude the pact subsystem "
                   "entirely; see each game's `limitations`."
                   + ("" if len(arms) <= 1 else
                      "  MIXED SETUPS: these games are NOT one experiment "
                      f"({len(arms)} distinct arms); do not report the pooled "
                      "number.")),
    }
