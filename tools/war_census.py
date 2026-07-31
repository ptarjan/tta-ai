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


def _emit(rec):
    if _OUT is not None:
        _OUT.write(json.dumps(rec) + "\n")


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


# --------------------------------------------------------- PlanBot (2p)

def _install_plan_hook():
    """Replace ``PlanBot.pick`` with a byte-for-byte copy plus one recording
    call.  See module docstring for the fidelity argument."""
    orig = _plan.PlanBot.pick

    def pick(self, state, moves):
        if len(moves) == 1:
            return moves[0]
        me = state.decider()
        w = self.weights
        try:
            ctx = rival_context(state, me)
        except Exception:
            ctx = dict(_plan._NO_CTX)
        if _plan.pending.not_my_turn(state, me):
            root = _plan.pending.prepare_root(
                self, state, _plan.copy_state, _plan.determinize, self.rng)
            return _plan.pending.fallback_pick(
                self, state,
                plain=lambda: self._one_ply(root, moves, me, w, ctx),
                quiet=lambda: self._one_ply_quiet(root, moves, me, w, ctx))
        totals = {mv: 0.0 for mv in moves}
        seen = {mv: 0 for mv in moves}
        drng = random.Random(state.seed * 7919 + state.turn * 31 + me)
        for _s in range(self.samples):
            root = _plan.copy_state(state)
            if self.determinize:
                _plan.determinize(root, drng)
            best = self._beam(root, moves, me, w, ctx)
            for mv, v in best.items():
                totals[mv] += v
                seen[mv] += 1
        scored = [(totals[mv] / seen[mv], mv) for mv in moves if seen[mv]]
        if not scored:
            return moves[0]
        chosen = max(scored, key=lambda t: t[0])[1]
        _record_decision(state, me, w, moves,
                          [(mv, v) for v, mv in scored], chosen)
        return chosen

    _plan.PlanBot.pick = pick
    return orig


# ------------------------------------------------------ QuiescentBot (3p)

def _install_quiescent_hook():
    orig = _quies.QuiescentBot.pick

    def pick(self, state, moves):
        if len(moves) == 1:
            return moves[0]
        idx = state.decider()
        try:
            root_ctx = rival_context(state, idx)
        except Exception:
            root_ctx = _quies._NO_CTX
        w = self.weights
        end_bias = w.get("end_turn_bias", 0.0)
        st = self.stats
        st["decisions"] += 1
        box = [self.MAX_NODES]
        depth = self.MAX_DEPTH
        levels = self.LEVELS
        if _quies.USE_JOURNAL:
            # Not instrumented (module docstring); behaviour is untouched.
            return self._pick_journalled(state, moves, idx, root_ctx, w,
                                         end_bias, box, depth, levels)
        best, best_val = None, None
        scored = []
        feat_by_mv = {}
        want_feat = bool({mv[0] for mv in moves} & set(TACTIC_KINDS))
        for mv in moves:
            st["candidates"] += 1
            trial = _quies.copy_state(state)
            try:
                _quies.actions.apply(trial, mv, _quies._fresh(2 * levels + 1))
            except Exception:
                continue
            ctx = root_ctx
            if levels > 0 and trial.pending:
                st["quiesced"] += 1
                before = box[0]
                quiet = _quies._resolve(trial, w, end_bias, levels - 1, box,
                                        depth)
                st["qnodes"] += before - box[0]
                if not quiet:
                    st["truncated"] += 1
                try:
                    ctx = rival_context(trial, idx, root_ctx.get("root_row"))
                except Exception:
                    ctx = root_ctx
            try:
                f = features(trial, idx, ctx, w) if want_feat else None
                val = evaluate(trial, idx, w, ctx, f=f)
            except Exception:
                continue
            if self.WAR_LOOKAHEAD and mv[0] == "war":
                wv = _quies._war_value(trial, idx, w, ctx)
                if wv is not None:
                    val = wv
                    # war_value resolves on ITS OWN scratch copy; `f` above no
                    # longer corresponds to the returned score, so drop it
                    # rather than record a mismatched feature diff.
                    f = None
            if mv[0] == "end_turn":
                val += end_bias
            scored.append((mv, val))
            if f is not None:
                feat_by_mv[mv] = f
            if best_val is None or val > best_val:
                best, best_val = mv, val
        if best is None:
            best = self.rng.choice(moves)
        _record_decision(state, idx, w, moves, scored, best, feat_by_mv)
        return best

    _quies.QuiescentBot.pick = pick
    return orig


def run(spec_text, players, games, out_path, seed0=9000):
    global _OUT
    from experiments import arena
    spec = arena.load_spec(spec_text)
    _install_plan_hook()
    _install_quiescent_hook()
    from engine import game
    with open(out_path, "w") as fh:
        _OUT = fh
        for g in range(games):
            seed = seed0 + g
            bots = [arena.make_bot(spec, seed * 131 + i) for i in range(players)]
            game.play_game(bots, num_players=players, seed=seed)
    _OUT = None


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
