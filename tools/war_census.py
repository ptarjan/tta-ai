r"""Decision-level census: is the bot's high war rate (A) war overpriced, or
(B) everything else underpriced?  docs/WAR_RATE_CENSUS.md is the write-up;
this is the instrument.

Against the 1,011-journal human corpus (docs/HUMAN_BASELINE.md) the bot
declares war at 2.9x the human rate, worse at 3p/4p than 2p, and copies a
tactic (`copy_tactic`) at a 27.3:1 ratio versus play.  Nobody had separated
"war wins because it scores high against a rich field" from "war wins because
half the field was never offered a nonzero score" -- and `row_pressure`'s
`if val <= 0.0: continue` (engine/bots/weighted.py) is a concrete, named
mechanism for the second: a row card whose `card_potential` comes out <= 0
contributes NOTHING to the "value left behind" terms that would otherwise
count against declining to take it, at every position those terms are read on
-- not just "priced low", literally 0.0, indistinguishable from a card that
was never in the row.

METHOD, and how this instrument is bounded (read before quoting a number):

* Instruments the two searches the league actually trains -- `PlanBot`
  (2p, `plan:width=2`) and `QuiescentBot` (3p, `quiesce:levels=1`) -- by
  installing a REPLACEMENT `pick()` that is a byte-for-byte copy of the
  original (same helper calls, same RNG draws, same order) plus one
  additive recording call after the real decision is made and BEFORE it is
  returned.  Nothing about control flow or the returned move changes.  The
  proof is `tools/gate.sh`: all eight fingerprint arms must hold (this
  script is never imported by anything the gate runs, so gate.sh alone does
  not prove it -- see docs/WAR_RATE_CENSUS.md for the before/after run).
* Only the non-journalled path is instrumented (`TTA_JOURNAL` unset, which
  is the league's own default -- `engine/bots/trial.py`).  The journalled
  twin (`_pick_journalled` / `_beam_journalled`) is NOT covered; if the
  league ever trains under `TTA_JOURNAL=1` this census needs a second pass.
* PlanBot's beam is multi-ply and multi-sample; a "candidate score" here is
  the value `pick()` itself uses to rank first-moves (the per-first-move
  average terminal score `_beam` returns), not a one-ply evaluation.  No
  feature-level diff is attempted for PlanBot decisions -- attributing a
  multi-ply terminal score to one feature is not meaningful -- so the
  copy_tactic feature-attribution table is QuiescentBot (3p) only, where
  the loop is a plain one-ply `evaluate` per candidate.
* "Suppressed" is computed over the ROW itself at the moment of the
  war/aggression decision (`_row_alternatives`, gated exactly as
  `row_pressure` gates it, `actions._can_take_gated`) -- NOT over the
  decision's own candidate list.  Found while building this: war/aggression
  is offered in the POLITICS sub-phase (siblings `pol_pass` /
  `offer_pact` / `cancel_pact` / `prepare_event`, `engine/actions.py:288-342`)
  and `("take", idx)` belongs to a separate civil-action decision later the
  same turn (`actions.py:401`) -- they are never candidates at the same node,
  so "which take was suppressed at the war decision" is not a question the
  engine's own decision structure poses. What row_pressure's skip DOES gate
  is exactly what `_row_alternatives` reads directly off `state.card_row`.
  This is also why the check is row-only: `hand_potential` (the hand-card
  path for `develop`/`build`/`play_action`/etc.) sums raw `card_potential`
  with no skip, so a negative-priced HAND card is merely discouraged, not
  invisible, and this script makes no suppression claim about hand cards.
* Every rate in the write-up states its own denominator (decisions with a
  war/aggression move OFFERED, not all decisions; games, not seat-turns)
  because a zero can mean "never offered" or "never chosen" and those are
  different findings.
* Sampling is bounded by `--games` and printed in the header of every run;
  nothing here claims full coverage.

Usage:

    python3.13 -m tools.war_census --spec plan:PATH,width=2 --players 2 \
        --games 300 --out /tmp/war_2p.jsonl
    python3.13 -m tools.war_census --spec quiesce:PATH,levels=1 --players 3 \
        --games 300 --out /tmp/war_3p.jsonl

Each line of `--out` is one JSON decision record; `tools/war_report.py`
folds them into the tables in docs/WAR_RATE_CENSUS.md.
"""
from __future__ import annotations

import argparse
import json
import os
import random
import sys

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

from engine import actions, cards as C                       # noqa: E402
from engine.bots import plan as _plan                        # noqa: E402
from engine.bots import quiescent as _quies                  # noqa: E402
from engine.bots.weighted import (                            # noqa: E402
    card_potential, evaluate, features, rival_context,
)

WAR_KINDS = ("war", "aggression")
TACTIC_KINDS = ("copy_tactic", "play_tactic")
#: production techs whose whole point is a per-turn RATE (food/resource/
#: science/culture production), vs everything else (one-shot culture,
#: military, government, wonder stage, action-card effect...).  Bucket names
#: match SYSTEM_COVERAGE.md's yellow/blue split.
RATE_TYPES = {"farm", "mine", "lab", "temple", "library", "arena", "theater"}

_OUT = None   # set by main(); the JSONL sink every recorder writes to
_WRITTEN = 0  # bytes this process has emitted
_CAPPED = False

#: THE CENSUS HAS TO SHARE A DISK WITH THE RUN IT IS MEASURING.  Left
#: uncapped it writes ~1 MB/minute per climber worker; the league runs for
#: days on a volume that was 96% full when this was written, so an instrument
#: with no ceiling is a way to kill the training run it exists to explain.
#:
#: MEASURED, not assumed (2026-07-31, all three arms up): 61 sinks appeared in
#: 68 minutes -- ~54/hour, NOT one per arm per hour.  The climber respawns its
#: workers per block, every couple of minutes, so a per-process byte cap is
#: nearly inert: it fires for almost nobody, and the directory total still
#: climbs at ~83 MB/hour.  A 144-hour run therefore hits the directory ceiling
#: in about six hours and records NOTHING for the remaining 138 -- exactly the
#: "first hour only" failure the per-process cap was supposed to prevent.
#:
#: So the budget is spent by SAMPLING instead, and the rate is arithmetic, not
#: a round number.  Measured full-rate output is ~60 MB/hour across all three
#: arms.  A worker inherits the climber's already-imported module, so a change
#: here only reaches games at the next hourly climber relaunch -- which left
#: ~130 MB of full-rate records banked before this took effect.  That leaves
#: (500 - 130) MB for ~143 remaining hours, i.e. ~2.6 MB/hour, i.e. a rate
#: under 2.6/60 = 0.043.  0.03 takes it with margin; 0.05 would have run the
#: directory into its ceiling around hour 120 and gone silent for the rest --
#: the same tail-loss the per-process cap already failed on once.
#:
#: Sampling rather than truncating also removes a bias a byte cap cannot avoid:
#: the first N records of a worker are its first N decisions, which are the
#: OPENING of a game, so a truncated sink is a sample of Age A/I and says
#: nothing about Age III.  The byte cap stays as a backstop for a worker that
#: outlives its peers.
#:
#: A file with NO `census_meta` record was written before sampling existed and
#: is full-rate; readers must default to sample=1.0, not to this constant.
_SAMPLE = float(os.environ.get("TTA_WAR_CENSUS_SAMPLE", "0.03"))
_MAX_BYTES = int(float(os.environ.get("TTA_WAR_CENSUS_MAX_MB", "0.25")) * 1e6)
_MAX_DIR_BYTES = int(
    float(os.environ.get("TTA_WAR_CENSUS_MAX_DIR_MB", "500")) * 1e6)

#: Its OWN stream.  Drawing the sampling coin from the `random` module would
#: advance the RNG the game itself draws from, and the one invariant of this
#: instrument is that enabling it cannot change a game.
_RNG = random.Random(os.getpid() * 2654435761 + 12345)


def _emit(rec):
    global _WRITTEN, _CAPPED
    if _OUT is None or _CAPPED:
        return
    if _SAMPLE < 1.0 and _RNG.random() >= _SAMPLE:
        return
    line = json.dumps(rec) + "\n"
    if _WRITTEN + len(line) > _MAX_BYTES:
        _CAPPED = True
        # NO SILENT CAPS.  A truncated sample that does not say it is
        # truncated reads exactly like a complete one, and every rate computed
        # from it is then quietly a rate over "the first N minutes of an hour".
        _OUT.write(json.dumps({
            "kind": "census_capped", "pid": os.getpid(),
            "bytes": _WRITTEN, "max_bytes": _MAX_BYTES}) + "\n")
        return
    _WRITTEN += len(line)
    _OUT.write(line)


def _row_alternatives(state, idx, w):
    """Every row card `row_pressure` would price at THIS decision, gated the
    same way `row_pressure` gates them (`actions._can_take_gated`).

    IMPORTANT STRUCTURAL NOTE, found while building this: a war/aggression
    move is offered in the POLITICS sub-phase, whose sibling candidates are
    `pol_pass` / `offer_pact` / `cancel_pact` / `prepare_event` -- NEVER
    `("take", idx)`, which belongs to a separate civil-action decision later
    in the same turn.  So "which take alternatives were suppressed" cannot be
    read off the war decision's own candidate list (it is empty of takes by
    construction, engine/actions.py:288-342 vs :401) -- the war
    decision and the take decision are different nodes.  What CAN be read off
    the war decision is the row as `row_pressure` itself would price it at
    that moment, which is what this function does; it is the direct analogue
    of "the opportunity cost that is or isn't visible to the position's own
    evaluation", not a re-derivation of a move that never appears.
    """
    row = state.card_row
    if not row:
        return []
    p = state.players[idx]
    try:
        mine = actions._take_gate(state, p, budget=actions.ca_total(state, p))
    except Exception:
        return []
    gated = actions._can_take_gated
    out = []
    for i, name in enumerate(row):
        if name is None:
            continue
        try:
            if not gated(state, p, i, mine, name):
                continue
        except Exception:
            continue
        val = card_potential(name, w, state, idx)
        try:
            typ = C.db().type_of(name)
        except Exception:
            typ = "?"
        out.append({"slot": i, "name": name, "card_potential": val,
                     "suppressed": val <= 0.0, "rate_building": typ in RATE_TYPES,
                     "card_type": typ})
    return out


def _take_info(state, idx, w, mv):
    """(card_name, card_potential, suppressed, rate_building) for a ``take``
    move, or None if the slot is empty (should not happen for a legal move,
    guarded anyway)."""
    i = mv[1]
    row = state.card_row
    if i >= len(row) or row[i] is None:
        return None
    name = row[i]
    val = card_potential(name, w, state, idx)
    suppressed = val <= 0.0
    try:
        typ = C.db().type_of(name)
    except Exception:
        typ = "?"
    return {"name": name, "card_potential": val, "suppressed": suppressed,
            "rate_building": typ in RATE_TYPES, "card_type": typ}


def _move_ident(state, idx, w, mv):
    """Best-effort human-readable identity + suppression info for a move,
    shared by both bots' recorders."""
    kind = mv[0]
    if kind == "take":
        info = _take_info(state, idx, w, mv)
        if info is None:
            return {"kind": kind, "name": None}
        return dict(info, kind=kind)
    if kind in ("war", "aggression"):
        return {"kind": kind, "name": mv[1], "target": mv[2]}
    if kind == "offer_pact":
        # ("offer_pact", name, partner, side) -- three fields distinguish one
        # offer from another, and the census recorded NONE of them until
        # 2026-08-01, so every pact logged as {"name": None} and the 61%
        # exact-tie rate on this kind could not be attributed.  war and
        # aggression carry the identical shape and were always identified;
        # this is the "in one list but not the other" class again.
        return {"kind": kind, "name": mv[1], "target": mv[2],
                "side": mv[3] if len(mv) > 3 else ""}
    if kind == "cancel_pact":
        # ("cancel_pact", owner) -- no card name, the pact is identified by
        # whose it is.
        return {"kind": kind, "name": None, "target": mv[1]}
    if kind in ("copy_tactic", "play_tactic", "build", "develop",
                "play_action", "play_leader", "destroy", "wonder_step",
                "prepare_event"):
        return {"kind": kind, "name": mv[1] if len(mv) > 1 else None}
    return {"kind": kind, "name": None}


def _record_decision(state, idx, w, moves, scored, chosen, feat_by_mv=None):
    """Common recorder for a REAL decision (never a search-internal trial).

    ``scored`` is a list of ``(mv, value)`` in ``moves`` order (whatever
    subset the search actually managed to score -- a candidate whose
    ``apply``/``evaluate`` raised is silently absent, exactly as the real
    ``pick`` treats it).
    """
    kinds_present = {mv[0] for mv in moves}
    has_war = bool(kinds_present & set(WAR_KINDS))
    has_tactic = bool(kinds_present & set(TACTIC_KINDS))
    if not has_war and not has_tactic:
        return
    order = sorted(scored, key=lambda t: -t[1])
    if not order:
        return
    chosen_score = dict(scored).get(chosen)
    runnerup = None
    runnerup_score = None
    for mv, v in order:
        if mv != chosen:
            runnerup, runnerup_score = mv, v
            break
    margin = (chosen_score - runnerup_score
              if chosen_score is not None and runnerup_score is not None
              else None)
    candidates = []
    for mv, v in scored:
        info = _move_ident(state, idx, w, mv)
        info["score"] = v
        candidates.append(info)
    rec = {
        "players": len(state.players),
        "seat": idx,
        "round": state.round,
        "age": state.age_civil,
        "chosen": _move_ident(state, idx, w, chosen) | {"score": chosen_score},
        "runnerup": (_move_ident(state, idx, w, runnerup) | {"score": runnerup_score}
                     if runnerup is not None else None),
        "margin": margin,
        "n_candidates": len(scored),
        "has_war_available": has_war,
        "has_tactic_available": has_tactic,
        "candidates": candidates,
    }
    if has_war:
        rec["row_alternatives"] = _row_alternatives(state, idx, w)
    if feat_by_mv:
        chosen_f = feat_by_mv.get(chosen)
        runnerup_f = feat_by_mv.get(runnerup) if runnerup is not None else None
        if chosen_f is not None and runnerup_f is not None:
            diffs = {}
            keys = set(chosen_f) | set(runnerup_f)
            for k in keys:
                cv, rv = chosen_f.get(k, 0.0), runnerup_f.get(k, 0.0)
                if cv != rv:
                    wk = w.get(k, 0.0)
                    diffs[k] = {"chosen": cv, "runnerup": rv,
                                "weight": wk, "weighted_diff": wk * (cv - rv)}
            rec["feature_diff_chosen_vs_runnerup"] = diffs
    _emit(rec)


# ---------------------------------------------------------------- plumbing
#
# There is no `pick()` monkeypatch here any more.  The recording call lives
# inside the real `PlanBot.pick` / `QuiescentBot.pick`, behind
# `engine.census.ENABLED`, so there is no hand-copy of the search to drift
# out of sync with the search.  This module is now only the recorder plus a
# driver.

record_decision = _record_decision


def open_sink_for_process(dest):
    """Point the recorder at `<dest>/census-<pid>.jsonl`, append mode.

    Per-pid because the league runs its arms in parallel out of one tree; a
    shared handle would interleave partial lines from different games into
    unparseable JSON.  Line-buffered so a killed arm still leaves valid
    records behind -- arms are restarted routinely and losing the tail of
    every run to buffering would quietly bias the sample toward whatever
    happens early in a game.

    Returns None, and records nothing for this process, once the directory as
    a whole reaches `_MAX_DIR_BYTES`.  Deleting the directory is the resume:
    the next relaunch opens fresh sinks with no other action needed.

    Every sink opens with a `census_meta` record naming `_SAMPLE`, because a
    5% sample with no header reads exactly like a full one and every COUNT
    taken off it is then wrong by 20x.  Rates are unaffected (numerator and
    denominator are sampled alike); counts must be divided by `sample`.
    """
    global _OUT
    if _OUT is not None:
        return _OUT
    os.makedirs(dest, exist_ok=True)
    try:
        used = sum(os.path.getsize(os.path.join(dest, f))
                   for f in os.listdir(dest) if f.endswith(".jsonl"))
    except OSError:
        used = 0
    if used > _MAX_DIR_BYTES:
        return None
    _OUT = open(os.path.join(dest, "census-%d.jsonl" % os.getpid()),
                "a", buffering=1)
    _OUT.write(json.dumps({
        "kind": "census_meta", "pid": os.getpid(), "sample": _SAMPLE,
        "max_bytes": _MAX_BYTES, "dir_used_at_open": used}) + "\n")
    return _OUT


def run(spec_text, players, games, out_path, seed0=9000):
    """Offline driver: same recorder, explicit sink, census forced on."""
    global _OUT
    from engine import census
    from experiments import arena
    spec = arena.load_spec(spec_text)
    census.ENABLED = True
    census._loaded = True
    census._impl = record_decision
    from engine import game
    try:
        with open(out_path, "w") as fh:
            _OUT = fh
            for g in range(games):
                seed = seed0 + g
                bots = [arena.make_bot(spec, seed * 131 + i)
                        for i in range(players)]
                game.play_game(bots, num_players=players, seed=seed)
    finally:
        _OUT = None
        census.ENABLED = False
        census._impl = None


def main(argv=None):
    ap = argparse.ArgumentParser()
    ap.add_argument("--spec", required=True)
    ap.add_argument("--players", type=int, required=True)
    ap.add_argument("--games", type=int, default=300)
    ap.add_argument("--seed", type=int, default=9000)
    ap.add_argument("--out", required=True)
    a = ap.parse_args(argv)
    run(a.spec, a.players, a.games, a.out, a.seed)
    print(f"wrote {a.out}  ({a.games} games, {a.players}p, spec={a.spec})")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
