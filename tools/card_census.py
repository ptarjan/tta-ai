"""Per-card play-frequency census, measured from real games.

`tools/card_blindness.py` asks a *static* question -- what can `_card_yields`
see on this card -- and it answers it with the wrong instrument for half the
deck (it walks `hand_potential`, which is civil-only, so it reported all 55
events, 11 aggressions, 10 pacts and 3 wars as blind without ever inspecting
the path they actually take).  This module asks the *dynamic* question that
would have caught the wonder bug on its own:

    of the times this card was on offer, how often did the bot take it, and
    of the times it took it, how often did it ever get played?

The wonder defect is exactly a hole in that table.  Wonders were priced
"correctly" after docs/CARD_BLINDNESS.md and completions did not move
(0.0997 -> 0.1047, p=0.12), because a wonder never enters `hand_civil`:
`actions.take_card` puts it straight into `p.wonder`, so its `card_potential`
reaches the policy only through `row_urgency`, a take-timing heuristic, and
never through `hand_potential`, which the search optimises at every decision.
Correct pricing in a pipe that does not reach the policy is worth nothing.
A census is how you notice; see docs/CARD_CENSUS.md for the plumbing map that
says *why* each hole is there.

WHAT IS MEASURED

Nothing in the engine is touched.  `_Observer` replays `game.play_game`'s own
loop and diffs a compact snapshot of the public containers across each real
`apply`, so trial states inside the bot's search are never seen.  Per card
name, per player count:

    offered     player-turns the card sat in the civil row and the player to
                move could LEGALLY take it (`actions._can_take_gated`, the
                real rule).  This is the denominator that matters: a card
                that appears rarely and is always taken is healthy; a card
                that is offered constantly and never taken is the signal.
    dealt       times an instance entered the civil row
    taken       times an instance left the row into a player
    swept       times an instance was destroyed off the left of the row
    drawn       times an instance entered a military hand (military deck)
    played      times the card's effect actually reached the board.  Type
                specific and written down in `PLAYED_BY`: a wonder is played
                when it COMPLETES, a tech when it is developed, an event
                when it is prepared into the future deck, an aggression when
                it is thrown, a leader when it comes into play, and so on.
    scored      times it was in a scoring position in the final state.

`report` divides by the right denominator per deck: `taken / offered` and
`played / taken` for a civil card, `played / drawn` for a military one.

The `probe` subcommand answers the OTHER half -- whether the card's value can
reach the policy at all -- by reproducing `WeightedBot.pick` exactly and
asking whether the score of a candidate depends on which card it is.  See its
section below, and docs/CARD_CENSUS.md section 1.2.

USAGE

    # measure (writes one JSON line per game, as it finishes)
    python3 -m tools.card_census run --players 2 --games 6000 \
        --champion analysis/frozen/champion_2p.json --out /tmp/census2p.jsonl

    # report from the raw file
    python3 -m tools.card_census report /tmp/census2p.jsonl --cards wonder

    # can a card's identity move the score?
    python3 -m tools.card_census probe --players 2 --games 40 \
        --champion analysis/frozen/champion_2p.json

    # regression gate: fail loudly if a type's play rate has collapsed, or if
    # a type that used to be played reaches zero
    python3 -m tools.card_census check /tmp/census2p.jsonl \
        --baseline analysis/census/baseline.json
"""
from __future__ import annotations

import argparse
import collections
import json
import math
import multiprocessing as mp
import os
import random
import sys
import time

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

from engine import cards as C  # noqa: E402

# Counter keys, in report order.  `offered` first because it is the
# denominator every rate in this file is conditional on.
# --------------------------------------------------------------- snapshot
#
# A snapshot is a plain tuple of immutable containers, so a diff is a pile of
# cheap multiset comparisons and nothing can alias into a state the engine is
# about to mutate.  Everything read here is PUBLIC (the row, tableaux, played
# leaders/governments, resolved events); hidden hands are read too, but only
# to count, never to feed a bot.

def _multiset(seq):
    return collections.Counter(seq)


def _flat(d):
    out = []
    for v in (d or {}).values():
        out.extend(v)
    return out


def _snap(state):
    ps = []
    for p in state.players:
        ps.append((
            _multiset(p.hand_civil),
            _multiset(p.hand_military),
            {n: t.workers for n, t in p.techs.items()},
            None if p.wonder is None else p.wonder.name,
            _multiset(p.completed_wonders),
            p.government,
            p.leader,
            p.tactic,
            _multiset(p.colonies),
            _multiset(pc["name"] for pc in p.pacts),
            p.idx,
        ))
    return {
        "row": _multiset(n for n in state.card_row if n is not None),
        "players": ps,
        "future": _multiset(state.future_events),
        "current": _multiset(state.current_events),
        "past": _multiset(state.past_events),
        "scoring": _multiset(state.scoring_events),
        "tactics": _multiset(state.available_tactics),
        "cdisc": _multiset(_flat(state.civil_discard)),
        "mdisc": _multiset(_flat(state.discarded_military)),
    }


def _gained(a, b):
    """Names whose count went UP from snapshot-part `a` to `b`, with amount."""
    out = []
    for k, v in b.items():
        d = v - a.get(k, 0)
        if d > 0:
            out.append((k, d))
    return out


def _lost(a, b):
    out = []
    for k, v in a.items():
        d = v - b.get(k, 0)
        if d > 0:
            out.append((k, d))
    return out


# ------------------------------------------------------------- play events
#
# "Played" is not one thing, and getting it wrong in either direction ruins
# the census.  For each type, PLAYED below is the single moment the card's
# effect actually reaches the board -- the thing a play rate is a rate OF --
# and it is read off the MOVE TUPLE wherever a move exists, because that is
# exact.  Diffing containers is reserved for the transitions that have no
# move of the mover's own: a wonder completing, a pact being accepted by
# somebody else, a colony being won at auction, a bonus card being spent
# inside the resolution machinery.
#
# The traps this table encodes, each of which the first draft got wrong:
#   * a tactic can enter play with NO card, via `("copy_tactic", n)`, so
#     counting `p.tactic` transitions double-counts against `drawn`;
#   * a territory is prepared like an event and only later colonized -- two
#     different rates, and the second is not the player's decision;
#   * a REFUSED pact comes back to the hand (`interact.py:228`), so a hand
#     departure is not a play;
#   * a bonus card has no move handler AT ALL and is only ever spent by the
#     defense / colonization machinery.
PLAYED_BY = {
    # civil row -> hand_civil -> a move of the owner's
    "farm": "develop", "mine": "develop", "lab": "develop",
    "temple": "develop", "library": "develop", "arena": "develop",
    "theater": "develop", "infantry": "develop", "cavalry": "develop",
    "artillery": "develop", "air": "develop", "special-tech": "develop",
    "government": "develop",           # or `revolution`, folded in below
    "leader": "play_leader",
    "action": "play_action",
    # civil row -> p.wonder, and the play is the COMPLETION, not the take
    "wonder": "completed",
    # military hand -> a move of the owner's
    "event": "prepare_event",
    "territory": "prepare_event",
    "tactic": "play_tactic",
    "aggression": "aggression",
    "war": "war",
    "pact": "offer_pact",
    # military hand -> spent inside the resolution machinery, no move
    "bonus": "spent",
}

# move kind -> (counter name, index of the card name in the move tuple)
_MOVE_CARD = {
    "develop": ("played", 1),
    "revolution": ("played", 1),
    "play_leader": ("played", 1),
    "play_action": ("played", 1),
    "prepare_event": ("played", 1),
    "play_tactic": ("played", 1),
    "aggression": ("played", 1),
    "war": ("played", 1),
    "offer_pact": ("played", 1),
    "copy_tactic": ("copied", 1),
    "build": ("built", 1),
    "destroy": ("destroyed", 1),
}


class _Observer:
    """Counts card-lifecycle transitions across the real moves of one game."""

    def __init__(self, state):
        self.db = C.db()
        self.type_of = self.db.type_by_name
        self.c = collections.defaultdict(collections.Counter)
        self.prev = _snap(state)
        self._turn_key = None
        # player idx -> {pact name: copies currently out on offer}
        self.offered_out = collections.defaultdict(dict)
        # every name in the opening row counts as dealt
        for n, k in self.prev["row"].items():
            self.c[n]["dealt"] += k
        self.errors = []

    def note_move(self, state, mv):
        """Read the exact card off the chosen move, BEFORE it is applied."""
        try:
            kind = mv[0] if isinstance(mv, tuple) else mv
            if kind == "take":
                name = state.card_row[mv[1]]
                if name:
                    self.c[name]["taken"] += 1
                    if self.type_of.get(name) == "wonder":
                        self.c[name]["started"] += 1
                return
            if kind == "upgrade":
                self.c[mv[2]]["built"] += 1
                self.c[mv[1]]["upgraded_away"] += 1
                return
            if kind == "wonder_step":
                p = state.me()
                if p.wonder is not None:
                    self.c[p.wonder.name]["steps"] += mv[1]
                return
            if kind == "offer_pact" and len(mv) > 1:
                d = self.offered_out[state.decider()]
                d[mv[1]] = d.get(mv[1], 0) + 1
            hit = _MOVE_CARD.get(kind)
            if hit:
                key, i = hit
                if len(mv) > i and isinstance(mv[i], str):
                    self.c[mv[i]][key] += 1
        except Exception as e:
            self.errors.append(repr(e))

    # -- the availability denominator ---------------------------------
    def sample_offer(self, state):
        """Once per player-turn: which row cards can the mover legally take?

        Uses `actions._can_take_gated`, i.e. the engine's own legality rule
        (reach, hand limit, duplicate leader age, mid-wonder), so `offered`
        means "this bot could have had it and did not", not "it was on the
        table somewhere".
        """
        from engine import actions
        key = (state.turn, state.current)
        if key == self._turn_key or state.pending:
            return
        self._turn_key = key
        p = state.me()
        try:
            budget = actions.ca_total(state, p)
            mine = actions._take_gate(state, p, budget=budget)
            gated = actions._can_take_gated
            for i, name in enumerate(state.card_row):
                if name is not None and gated(state, p, i, mine, name):
                    self.c[name]["offered"] += 1
        except Exception as e:                # bookkeeping must never kill a game
            self.errors.append(repr(e))

    # -- the transition diff -------------------------------------------
    def observe(self, state):
        try:
            self._observe(state)
        except Exception as e:
            self.errors.append(repr(e))
            self.prev = _snap(state)

    def _observe(self, state):
        now = _snap(state)
        old = self.prev
        c = self.c
        type_of = self.type_of

        # --- the civil row.  `taken` is counted from the move tuple in
        # `note_move`, so all that is left here is DEALT and SWEPT.  A name
        # that left the row was swept iff it landed in `civil_discard`, which
        # the engine records for exactly this reason (state.py:158).
        swept = _gained_dict(old["cdisc"], now["cdisc"])
        for name, k in _lost(old["row"], now["row"]):
            s = min(k, swept.get(name, 0))
            if s:
                c[name]["swept"] += s
        for name, k in _gained(old["row"], now["row"]):
            c[name]["dealt"] += k

        mdisc = _gained_dict(old["mdisc"], now["mdisc"])
        for pn, po in zip(now["players"], old["players"]):
            # --- military deck draws (economy.py:154, events.py:124, and a
            # REFUSED pact coming back at interact.py:228 -- that last one is
            # a return, not a draw, and would inflate the denominator, so it
            # is netted out by `returned` below).
            for name, k in _gained(po[1], pn[1]):
                # A REFUSED pact comes back to the offerer's hand
                # (interact.py:228).  Counting that as a draw would inflate
                # the denominator of every pact rate, so it is netted out
                # against the offers this player still has outstanding.
                out = self.offered_out[pn[10]]
                back = min(k, out.get(name, 0)) if out else 0
                if back:
                    out[name] -= back
                    if not out[name]:
                        del out[name]
                    c[name]["refused"] += back
                if k - back:
                    c[name]["drawn"] += k - back
            for name, k in _lost(po[1], pn[1]):
                d = min(k, mdisc.get(name, 0))
                if d:
                    c[name]["discarded"] += d
                # A bonus card has no move handler at all: it is spent inside
                # the defense / colonization machinery, so a hand departure
                # that is not a discard is the only way to see it played.
                if k - d and type_of.get(name) == "bonus":
                    c[name]["played"] += k - d

            # --- board arrivals with no move of the mover's own
            for name, k in _gained(po[4], pn[4]):
                c[name]["completed"] += k               # wonder COMPLETED
                c[name]["played"] += k
            for name, k in _gained(po[8], pn[8]):
                c[name]["colonized"] += k               # auction won
            for name, k in _gained(po[9], pn[9]):
                c[name]["signed"] += k                  # pact ACCEPTED
                # the owner's outstanding offer is now consumed
                for d in self.offered_out.values():
                    if d.get(name):
                        d[name] -= k
                        if d[name] <= 0:
                            del d[name]
                        break
            if pn[7] != po[7] and pn[7]:
                c[pn[7]]["in_play"] += 1                # tactic in play

        # --- events: resolution is not the holder's decision
        for name, k in _gained(old["past"], now["past"]):
            c[name]["resolved"] += k

        self.prev = now

    # -- terminal state --------------------------------------------------
    def finish(self, state):
        c = self.c
        for p in state.players:
            for name in p.techs:
                c[name]["scored"] += 1
            for name in p.completed_wonders:
                c[name]["scored"] += 1
            if p.government:
                c[p.government]["scored"] += 1
            if p.leader:
                c[p.leader]["scored"] += 1
            if p.tactic:
                c[p.tactic]["scored"] += 1
            for name in p.colonies:
                c[name]["scored"] += 1
            for pc in p.pacts:
                c[pc["name"]]["scored"] += 1
        for name in state.past_events:
            c[name]["scored"] += 1

    def result(self):
        return {n: dict(v) for n, v in self.c.items() if v}


def _gained_dict(a, b):
    out = {}
    for k, v in b.items():
        d = v - a.get(k, 0)
        if d > 0:
            out[k] = d
    return out


# ------------------------------------------------------------------ runner

_W = {}


def _init(spec, n, cap):
    _W["spec"], _W["n"], _W["cap"] = spec, n, cap


def _play(task):
    from engine import game
    from experiments.arena import make_bot
    gi, seed = task
    n, spec = _W["n"], _W["spec"]
    bots = [make_bot(spec, seed * 97 + i * 13 + 1) for i in range(n)]
    t0 = time.time()
    try:
        state = game.new_game(n, seed)
        rng = random.Random(seed ^ 0x5EED)
        obs = _Observer(state)
        moves = 0
        while not state.game_over:
            if moves >= _W["cap"]:
                state.move_cap_hit = True
                game._finish_game(state)
                break
            obs.sample_offer(state)
            mv = bots[state.decider()](state)
            obs.note_move(state, mv)
            game.apply(state, mv, rng)
            obs.observe(state)
            moves += 1
        obs.finish(state)
        return {"game": gi, "seed": seed, "players": n, "moves": moves,
                "rounds": state.round, "secs": round(time.time() - t0, 2),
                "cards": obs.result(), "errors": obs.errors[:3]}
    except Exception as e:
        return {"game": gi, "seed": seed, "players": n, "error": repr(e)}


def _guard(args, tool):
    """Route the spec through the repo's own degenerate-champion guard.

    `analysis/frozen/champion_4p.json` is the pre-horizon-fix 4p vector
    (`science = -6.089`, measured at 20.1% against a 25% null) with two
    weights nudged, and `experiments/arena.py` exists partly to stop tools
    printing numbers for it without saying so.  A census of a weak bot is
    still worth having -- "what does the shipped 4p champion do" is a real
    question -- so this WARNS and continues under an explicit flag rather
    than refusing outright, but it cannot be passed silently.
    """
    from experiments import arena
    # The repo guard compares CONTENT EXACTLY, and
    # `analysis/frozen/champion_4p.json` differs from the degenerate vector in
    # exactly two weights (`colonies`, `pacts`) while keeping the thing that
    # makes it degenerate -- `science = -6.089`.  So it walks straight through
    # `refuse_if_degenerate_champion`.  Near-identity is checked here as well,
    # because "the same broken vector with two knobs moved" is the case that
    # actually turns up.
    _warn_near_degenerate(args.champion)
    if getattr(args, "allow_degenerate", False):
        try:
            arena.refuse_if_degenerate_champion(args.champion, tool)
        except SystemExit:
            print(f"WARNING: {args.champion} is the known-degenerate vector; "
                  f"every number from this run inherits that caveat.",
                  file=sys.stderr)
        except Exception:
            pass
    else:
        arena.refuse_if_degenerate_champion(args.champion, tool)


def _warn_near_degenerate(path):
    from experiments import arena
    try:
        known = arena._weights_of(arena.DEGENERATE_CHAMPION_PATH)
        mine = arena._weights_of(path)
    except Exception:
        return
    keys = set(known) | set(mine)
    if not keys:
        return
    same = sum(1 for k in keys if known.get(k) == mine.get(k))
    if same / len(keys) >= 0.95:
        diff = sorted(k for k in keys if known.get(k) != mine.get(k))
        print(f"WARNING: {path} is {same}/{len(keys)} identical to the known-"
              f"degenerate 4p vector (differs only in {diff}), including "
              f"science={mine.get('science')}. Every number from this run "
              f"inherits that caveat.", file=sys.stderr)


def run(args):
    from experiments.arena import load_spec
    _guard(args, "tools/card_census.py run")
    spec = load_spec(args.champion)
    tasks = [(i, args.seed + i) for i in range(args.games)]
    n_done = 0
    t0 = time.time()
    # Raw to disk AS IT ARRIVES.  A census is hours of games and the analysis
    # is seconds; losing the games to a crash in the analysis (or to a killed
    # process) is the one failure mode worth engineering against.
    with open(args.out, "a") as fh:
        if args.workers == 1:
            _init(spec, args.players, args.move_cap)
            it = map(_play, tasks)
            for rec in it:
                fh.write(json.dumps(rec) + "\n")
                fh.flush()
                n_done += 1
                _tick(n_done, args.games, t0)
        else:
            ctx = mp.get_context("fork" if hasattr(os, "fork") else "spawn")
            with ctx.Pool(args.workers, _init,
                          (spec, args.players, args.move_cap)) as pool:
                for rec in pool.imap_unordered(_play, tasks, chunksize=1):
                    fh.write(json.dumps(rec) + "\n")
                    fh.flush()
                    n_done += 1
                    _tick(n_done, args.games, t0)
    print(f"\n{n_done} games -> {args.out}", file=sys.stderr)


def _tick(done, total, t0):
    if done % 10 and done != total:
        return
    el = time.time() - t0
    rate = done / el if el else 0
    eta = (total - done) / rate if rate else 0
    print(f"\r{done}/{total} games  {rate:.2f}/s  eta {eta/60:.1f}m",
          end="", file=sys.stderr, flush=True)


# ------------------------------------------------------- identity sensitivity
#
# The census says WHAT the bot does.  This says WHY, and it is the part that
# turns "seldom played" into "wonder-class defect" or "just a bad card".
#
# The question is narrow and answerable: at a real decision, when the bot is
# choosing between two candidates of the SAME move kind and the SAME card
# type, does the evaluation depend on WHICH CARD it is?  If two different
# wonders in the row score byte-identically as `("take", i)` candidates, then
# no amount of correct wonder pricing can change the choice, and the type is
# blind by construction rather than by taste.
#
# It reproduces `WeightedBot.pick` exactly -- same `ctx` captured once at the
# root on the unmutated state, same `copy_state` + `actions.apply` +
# `evaluate` -- so the numbers are the ones the policy actually saw, not a
# reconstruction of them.  `card_potential` is recorded alongside as the
# reference: a type where `card_potential` varies and the SCORE does not is a
# severed pipe, and that difference is the whole finding.


def _probe_name(state, mv):
    """The card a candidate move is about, or None."""
    k = mv[0]
    if k == "take":
        return state.card_row[mv[1]]
    if k == "upgrade":
        return mv[2]
    if k == "wonder_step":
        p = state.me()
        return p.wonder.name if p.wonder is not None else None
    if len(mv) > 1 and isinstance(mv[1], str):
        return mv[1]
    return None


def _probe_game(spec, n, seed, cap):
    from engine import game, actions
    from engine.bots import weighted as W
    from engine.bots.fastcopy import copy_state
    from engine.bots.trial import fresh_trial_rng
    from experiments.arena import make_bot

    bots = [make_bot(spec, seed * 97 + i * 13 + 1) for i in range(n)]
    w = spec if isinstance(spec, dict) else None
    state = game.new_game(n, seed)
    rng = random.Random(seed ^ 0x5EED)
    db = C.db()
    type_of = db.type_by_name
    # (move kind, card type) -> [n_decisions, n_flat, sum_score_range,
    #                            sum_cp_range, n_cp_varies, n_cp_varies_flat,
    #                            concordant_pairs, comparable_pairs]
    acc = collections.defaultdict(lambda: [0, 0, 0.0, 0.0, 0, 0, 0, 0])
    moves_done = 0
    while not state.game_over and moves_done < cap:
        idx = state.decider()
        legal = actions.legal_moves(state)
        wt = getattr(bots[idx], "weights", w) or W.DEFAULT_WEIGHTS
        if len(legal) > 1:
            try:
                ctx = W.rival_context(state, idx)
            except Exception:
                ctx = None
            groups = collections.defaultdict(dict)
            for mv in legal:
                name = _probe_name(state, mv)
                if name is None:
                    continue
                trial = copy_state(state)
                try:
                    actions.apply(trial, mv, fresh_trial_rng())
                    val = W.evaluate(trial, idx, wt, ctx)
                except Exception:
                    continue
                groups[(mv[0], type_of.get(name, "?"))].setdefault(
                    name, (val, W.card_potential(name, wt)))
            for key, byname in groups.items():
                if len(byname) < 2:
                    continue
                vals = [v for v, _ in byname.values()]
                cps = [c for _, c in byname.values()]
                srange = max(vals) - min(vals)
                crange = max(cps) - min(cps)
                a = acc[key]
                a[0] += 1
                a[2] += srange
                a[3] += crange
                if srange < 1e-9:
                    a[1] += 1
                if crange > 1e-9:
                    a[4] += 1
                    if srange < 1e-9:
                        a[5] += 1
                # Kendall concordance between the SCORE order and the
                # card_potential order, over the pairs where both differ.
                # 0.5 is "no relationship"; 1.0 is "the policy ranks these
                # cards exactly as their priced value does".  This is the
                # number that separates a severed pipe from a live one, and
                # it survives the case where the score varies for a reason
                # unrelated to the card's value -- a wonder's score moves
                # with its COST whatever its value, and that reads here as
                # concordance at or below 0.5.
                items = list(byname.values())
                for i in range(len(items)):
                    for j in range(i + 1, len(items)):
                        dv = items[i][0] - items[j][0]
                        dc = items[i][1] - items[j][1]
                        if abs(dv) > 1e-9 and abs(dc) > 1e-9:
                            a[7] += 1
                            if (dv > 0) == (dc > 0):
                                a[6] += 1
        mv = bots[idx](state)
        game.apply(state, mv, rng)
        moves_done += 1
    return {f"{k[0]}|{k[1]}": v for k, v in acc.items()}


def _probe_task(t):
    gi, seed = t
    try:
        return _probe_game(_W["spec"], _W["n"], seed, _W["cap"])
    except Exception as e:
        import traceback
        print("PROBE ERROR", repr(e), traceback.format_exc()[-600:],
              file=sys.stderr)
        return {}


def probe(args):
    from experiments.arena import load_spec
    _guard(args, "tools/card_census.py probe")
    spec = load_spec(args.champion)
    tasks = [(i, args.seed + i) for i in range(args.games)]
    tot = collections.defaultdict(lambda: [0, 0, 0.0, 0.0, 0, 0, 0, 0])
    ctxm = mp.get_context("fork" if hasattr(os, "fork") else "spawn")
    with ctxm.Pool(args.workers, _init, (spec, args.players, args.move_cap)) as pool:
        for res in pool.imap_unordered(_probe_task, tasks, chunksize=1):
            for k, v in res.items():
                if k == "__repr__":
                    continue
                a = tot[k]
                for i in range(8):
                    a[i] += v[i]
    rows = []
    for k, (nd, flat, sr, cr, cvar, cvar_flat, conc, pairs) in sorted(tot.items()):
        if nd < 5:
            continue
        kind, typ = k.split("|", 1)
        rows.append({
            "kind": kind, "type": typ, "decisions": nd,
            "flat_frac": flat / nd,
            "mean_score_range": sr / nd,
            "mean_cp_range": cr / nd,
            "cp_varies": cvar,
            "severed": (cvar_flat / cvar) if cvar else None,
            "pairs": pairs,
            "concordance": (conc / pairs) if pairs else None,
        })
    print(f"# identity sensitivity -- {args.games} games at {args.players}p, "
          f"{args.champion}\n")
    print("| move | card type | decisions | mean score spread | mean "
          "card_potential spread | flat | SEVERED | pairs | concordance |")
    print("|---|---|---|---|---|---|---|---|---|")
    for r in sorted(rows, key=lambda r: -r["decisions"]):
        sev = "-" if r["severed"] is None else f"{r['severed']:.3f}"
        con = "-" if r["concordance"] is None else f"{r['concordance']:.3f}"
        print(f"| {r['kind']} | {r['type']} | {r['decisions']} | "
              f"{r['mean_score_range']:.4f} | {r['mean_cp_range']:.4f} | "
              f"{r['flat_frac']:.3f} | {sev} | {r['pairs']} | {con} |")
    print("\n`flat` = fraction of decisions where every candidate of that "
          "(move, type) scored IDENTICALLY.\n`SEVERED` = of the decisions "
          "where `card_potential` DID differ across the candidates, the "
          "fraction where the score still did not. 1.000 means the card's "
          "priced value cannot reach the policy at all.\n`concordance` = "
          "over candidate PAIRS where both the score and `card_potential` "
          "differ, how often they agree on which card is better. 0.5 is a "
          "coin flip -- the score is moving for some reason other than the "
          "card's value.")
    if args.json:
        with open(args.json, "w") as fh:
            json.dump({"games": args.games, "players": args.players,
                       "champion": args.champion, "rows": rows}, fh, indent=1)
        print(f"\nwrote {args.json}", file=sys.stderr)


# ---------------------------------------------------------------- analysis

def load(paths, split=False):
    """Aggregate raw JSONL into (per-card counters, games, meta).

    With `split=True` the first element is instead `{players: counters}`.
    Rates are NOT comparable across player counts -- territory plays at 0.708
    at 2p and 0.146 at 4p -- so anything that compares a run to a baseline
    has to do it per player count or it will report a change in the MIX as a
    change in the bot.  That is not hypothetical: it was the first thing the
    finished gate got wrong.
    """
    tot = collections.defaultdict(collections.Counter)
    per = collections.defaultdict(
        lambda: collections.defaultdict(collections.Counter))
    games = 0
    bad = 0
    by_players = collections.Counter()
    for path in paths:
        with open(path) as fh:
            for line in fh:
                line = line.strip()
                if not line:
                    continue
                rec = json.loads(line)
                if rec.get("error"):
                    bad += 1
                    continue
                games += 1
                n = rec.get("players")
                by_players[n] += 1
                for name, counts in rec["cards"].items():
                    tot[name].update(counts)
                    if split:
                        per[n][name].update(counts)
    meta = {"bad": bad, "by_players": dict(by_players)}
    return (per if split else tot), games, meta


def _check_played_by():
    """Every card type must say what "played" MEANS for it.

    `PLAYED_BY` is the semantic core of this tool and would otherwise be
    documentation that silently goes stale the moment a card type is added.
    Checking coverage makes it load-bearing: a new type fails here instead of
    quietly getting a play rate whose definition nobody wrote down.
    """
    known = {c["type"] for c in C.db().by_name.values()}
    missing = sorted(known - set(PLAYED_BY))
    if missing:
        raise SystemExit(
            f"card_census: no PLAYED_BY entry for {missing} -- decide what "
            f"'played' means for that type before measuring it")
    stale = sorted(set(PLAYED_BY) - known)
    if stale:
        raise SystemExit(f"card_census: PLAYED_BY has stale types {stale}")


def _rows(tot):
    """Per-card counters plus the three rates, each with its own denominator.

    The denominator is what this whole exercise turns on, so it is chosen
    from `card["deck"]` (the engine's own field) rather than guessed from
    whichever counter happens to be non-zero:

    * a CIVIL card is acquired from the open row, so its availability is
      `dealt` (instances that entered the row) and the interesting rate is
      `taken / offered` -- offers being player-turns on which the mover could
      LEGALLY have taken it;
    * a MILITARY card is dealt straight into a hand, so there is no row and no
      "offer": availability is `drawn` and the interesting rate is
      `played / drawn`.

    `play_given_held` is the second rate and the one that catches the wonder
    defect on its own: of the copies this bot actually acquired, how many ever
    reached the board.  A wonder that is taken and never finished, an
    aggression that is drawn and never thrown, and a bonus card that rots in
    hand all show up here and nowhere else.
    """
    _check_played_by()
    db = C.db()
    out = []
    for name, card in db.by_name.items():
        c = tot.get(name, {})
        typ = card["type"]
        civil = card.get("deck") != "military"
        offered = c.get("offered", 0)
        dealt = c.get("dealt", 0)
        taken = c.get("taken", 0)
        drawn = c.get("drawn", 0)
        played = c.get("played", 0)
        avail = dealt if civil else drawn
        held = taken if civil else drawn
        out.append({
            "name": name, "type": typ, "age": card.get("age"),
            "deck": "civil" if civil else "military",
            "offered": offered, "dealt": dealt, "taken": taken,
            "swept": c.get("swept", 0), "drawn": drawn, "played": played,
            "discarded": c.get("discarded", 0), "scored": c.get("scored", 0),
            "completed": c.get("completed", 0), "started": c.get("started", 0),
            "colonized": c.get("colonized", 0), "signed": c.get("signed", 0),
            "resolved": c.get("resolved", 0), "built": c.get("built", 0),
            "copied": c.get("copied", 0), "in_play": c.get("in_play", 0),
            "played_by": PLAYED_BY.get(typ, "?"),
            "avail": avail, "held": held,
            "take_per_offer": (taken / offered) if offered else None,
            "play_given_held": (played / held) if held else None,
            "play_per_avail": (played / avail) if avail else None,
        })
    return out


def report(args):
    tot, games, meta = load(args.paths)
    rows = _rows(tot)
    print(f"# card census -- {games} games "
          f"({', '.join(f'{k}p:{v}' for k, v in sorted(meta['by_players'].items()))})"
          f"{', %d failed' % meta['bad'] if meta['bad'] else ''}\n")

    by_type = collections.defaultdict(list)
    for r in rows:
        by_type[r["type"]].append(r)

    print("## by type\n")
    print("| type | deck | n | offered | taken/drawn | take/offer | played "
          "| play/held | never played | never seen |")
    print("|---|---|---|---|---|---|---|---|---|---|")
    for typ, rs in sorted(by_type.items(), key=lambda kv: -len(kv[1])):
        off = sum(r["offered"] for r in rs)
        held = sum(r["held"] for r in rs)
        av = sum(r["avail"] for r in rs)
        pl = sum(r["played"] for r in rs)
        never = sum(1 for r in rs if r["avail"] and not r["played"])
        unseen = sum(1 for r in rs if not r["avail"])
        print(f"| {typ} | {rs[0]['deck']} | {len(rs)} | {off} | {held} | "
              f"{(held/off if off else float('nan')):.3f} | {pl} | "
              f"{(pl/held if held else float('nan')):.3f} | "
              f"{never}/{len(rs)} | {unseen}/{len(rs)} |")

    if args.cards:
        rs = [r for r in rows if r["type"] == args.cards]
        print(f"\n## {args.cards}\n")
        print("| card | age | offered | dealt | drawn | taken | take/offer "
              "| played | play/held |")
        print("|---|---|---|---|---|---|---|---|---|")
        for r in sorted(rs, key=lambda r: (r["play_given_held"] is None,
                                           r["play_given_held"] or 0)):
            tpo = "-" if r["take_per_offer"] is None else f"{r['take_per_offer']:.3f}"
            pr = "-" if r["play_given_held"] is None else f"{r['play_given_held']:.3f}"
            print(f"| {r['name']} | {r['age']} | {r['offered']} | {r['dealt']} "
                  f"| {r['drawn']} | {r['taken']} | {tpo} | {r['played']} | {pr} |")

    if args.json:
        with open(args.json, "w") as fh:
            json.dump({"games": games, "meta": meta, "rows": rows}, fh, indent=1)
        print(f"\nwrote {args.json}", file=sys.stderr)


# ------------------------------------------------------------------- gate
#
# The point of landing the tool rather than its output: after an evaluator
# change, re-run and `check` against the recorded baseline.  A type whose play
# rate collapses is the wonder bug happening again, and this is the thing that
# is supposed to notice.

MIN_AVAIL = 30          # below this a rate is noise, not a regression

# A gate that cries wolf gets ignored, which is worse than no gate.  Both
# tests below are therefore gated on EXPECTED COUNT (`held x baseline_rate`),
# not on `held` alone.  The case that forced this: `aggression` has a true
# rate of 0.013, so a 6-game sample with 79 acquisitions expects ONE play and
# sees zero perfectly often -- a fixed `MIN_AVAIL` reported that as a
# regression.  Under Poisson, expecting >= 5 and seeing 0 is p < 0.01, which
# is the right bar for "this collapsed"; expecting >= 10 is the bar for
# trusting a ratio.
MIN_EXPECTED_ZERO = 5.0
MIN_EXPECTED_RATIO = 10.0

# ...and a ratio drop must ALSO be significant, not just large.  23 types are
# tested per arm, so a fixed "35% below baseline" rule fires on ordinary
# binomial noise roughly every other run: territory at 3p came in at 0.317
# against a 0.497 baseline on n=41, which looks alarming and is z = -2.3, a
# 2% event that 23 tests produce for free.  Requiring z <= -3 puts the
# family-wise false-alarm rate near 3% per arm, which is a gate somebody will
# still be reading in six months.
MAX_Z = -3.0


def _z(played, held, rate):
    """Standard score of `played` against Binomial(held, rate)."""
    var = held * rate * (1.0 - rate)
    if var <= 0.0:
        return 0.0
    return (played - held * rate) / math.sqrt(var)


def check(args):
    """Fail if a type's play rate collapsed, OR if a type went to zero.

    Compared PER PLAYER COUNT against the matching baseline arm, because
    rates are not comparable across counts (see `load`).

    Two tests, and the second one matters as much as the first and is easy to
    leave out.  A pure ratio test cannot fail a type whose baseline is
    already 0.000 -- `rate < 0 * (1 - tol)` is never true -- so a naive gate
    would permanently BLESS the types this census found broken; `war` at 0
    declarations in 71,229 draws would pass forever.  So each arm records its
    zero types BY NAME in `known_zero`, reports them as a standing `ZERO`
    defect, and FAILS any type that reaches zero and is not on that list.
    Fixing war means deleting it from `known_zero`, after which a regression
    fails.

    Both tests are gated on EXPECTED COUNT rather than sample size, so the
    gate does not cry wolf on a short run -- see `MIN_EXPECTED_*`.
    """
    per, games, meta = load(args.paths, split=True)
    with open(args.baseline) as fh:
        base = json.load(fh)
    arms = base.get("arms")
    if arms is None:
        print("baseline is in the old pooled format; regenerate it with "
              "`card_census.py baseline`", file=sys.stderr)
        return 2
    fails, notes, zeros = [], [], []
    for n in sorted(per, key=lambda x: (x is None, x)):
        arm = arms.get(str(n))
        if arm is None:
            notes.append(f"{n}p: no baseline arm, skipped ({games} games)")
            continue
        bl = {r["type"]: r for r in arm["types"]}
        known_zero = set(arm.get("known_zero", ()))
        print(f"--- {n}p ---")
        for row in _type_rows(per[n]):
            typ, held, pl = row["type"], row["held"], row["played"]
            b = bl.get(typ)
            if b is None:
                notes.append(f"{n}p {typ}: no baseline (new type?)")
                continue
            if held < MIN_AVAIL:
                notes.append(f"{n}p {typ}: only {held} acquisitions, skipped")
                continue
            expected = held * b["play_rate"]
            rate = pl / held
            if rate <= 0.0:
                if typ in known_zero:
                    zeros.append(f"{n}p:{typ}")
                    print(f"ZERO {typ:14s} play/held 0.000  n={held}  "
                          f"(known defect)")
                elif expected < MIN_EXPECTED_ZERO:
                    need = MIN_EXPECTED_ZERO / max(b["play_rate"], 1e-9)
                    notes.append(f"{n}p {typ}: 0 played but only "
                                 f"{expected:.1f} expected -- under-powered, "
                                 f"need ~{need:.0f} acquisitions")
                else:
                    fails.append(f"{n}p {typ}: play/held is ZERO in {held} "
                                 f"acquisitions, expected {expected:.1f} "
                                 f"(baseline {b['play_rate']:.3f})")
                    print(f"FAIL {typ:14s} play/held 0.000  baseline "
                          f"{b['play_rate']:.3f}  n={held}  "
                          f"expected {expected:.1f}")
                continue
            if typ in known_zero:
                print(f"ok+  {typ:14s} play/held {rate:.3f}  was a known ZERO"
                      f" -- drop it from known_zero to lock the fix in")
                continue
            if expected < MIN_EXPECTED_RATIO:
                notes.append(f"{n}p {typ}: only {expected:.1f} plays "
                             f"expected, under-powered for a ratio test")
                continue
            floor = b["play_rate"] * (1.0 - args.tol)
            z = _z(pl, held, b["play_rate"])
            mark = "ok "
            if rate < floor and z <= MAX_Z:
                mark = "FAIL"
                fails.append(f"{n}p {typ}: play/held {rate:.3f} < floor "
                             f"{floor:.3f} (baseline {b['play_rate']:.3f}, "
                             f"tol {args.tol:.0%}, z={z:.1f})")
            elif rate < floor:
                notes.append(f"{n}p {typ}: {rate:.3f} is below the "
                             f"{floor:.3f} floor but only z={z:.1f} -- noise "
                             f"at n={held}, not a regression")
            print(f"{mark} {typ:14s} play/held {rate:.3f}  baseline "
                  f"{b['play_rate']:.3f}  n={held}  z={z:+.1f}")
    for m in notes:
        print("  note:", m)
    if fails:
        print("\nCARD CENSUS FAIL")
        for f in fails:
            print(" ", f)
        return 1
    print("\nCARD CENSUS PASS"
          + (f" ({len(zeros)} known-zero: {', '.join(zeros)})"
             if zeros else ""))
    return 0


def _type_rows(tot):
    by_type = collections.defaultdict(list)
    for r in _rows(tot):
        by_type[r["type"]].append(r)
    out = []
    for typ, rs in sorted(by_type.items()):
        held = sum(r["held"] for r in rs)
        pl = sum(r["played"] for r in rs)
        out.append({"type": typ, "n_cards": len(rs),
                    "avail": sum(r["avail"] for r in rs),
                    "held": held, "played": pl,
                    "play_rate": (pl / held) if held else 0.0,
                    "offered": sum(r["offered"] for r in rs),
                    "taken": sum(r["taken"] for r in rs)})
    return out


def baseline(args):
    """Freeze the per-type rates, PER PLAYER COUNT, as the baseline."""
    per, games, meta = load(args.paths, split=True)
    arms, zero_all = {}, {}
    for n in sorted(per, key=lambda x: (x is None, x)):
        out = _type_rows(per[n])
        # Types that are not played AT ALL go in a named list, not silently
        # into a 0.000 rate that no ratio test can ever fail.  See `check`.
        zero = sorted(r["type"] for r in out
                      if r["held"] >= MIN_AVAIL and r["played"] == 0)
        arms[str(n)] = {"types": out, "known_zero": zero}
        zero_all[str(n)] = zero
    doc = {"games": games, "meta": meta, "arms": arms}
    with open(args.out, "w") as fh:
        json.dump(doc, fh, indent=1)
    print(f"wrote {args.out} ({games} games, {len(arms)} player counts, "
          f"known_zero={zero_all})")


# -------------------------------------------------------------------- cli

def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    sub = ap.add_subparsers(dest="cmd", required=True)

    r = sub.add_parser("run", help="play games and write raw per-game counts")
    r.add_argument("--players", type=int, default=2)
    r.add_argument("--games", type=int, default=100)
    r.add_argument("--seed", type=int, default=0)
    r.add_argument("--workers", type=int, default=1)
    r.add_argument("--move-cap", type=int, default=20000)
    r.add_argument("--champion", default="analysis/frozen/champion_2p.json")
    r.add_argument("--out", required=True)
    r.add_argument("--allow-degenerate", action="store_true",
                   help="measure the known-degenerate 4p vector anyway")
    r.set_defaults(fn=run)

    q = sub.add_parser("probe", help="does card identity move the score?")
    q.add_argument("--players", type=int, default=2)
    q.add_argument("--games", type=int, default=40)
    q.add_argument("--seed", type=int, default=0)
    q.add_argument("--workers", type=int, default=1)
    q.add_argument("--move-cap", type=int, default=20000)
    q.add_argument("--champion", default="analysis/frozen/champion_2p.json")
    q.add_argument("--json")
    q.add_argument("--allow-degenerate", action="store_true")
    q.set_defaults(fn=probe)

    p = sub.add_parser("report", help="aggregate raw files into tables")
    p.add_argument("paths", nargs="+")
    p.add_argument("--cards", help="also dump every card of this type")
    p.add_argument("--json", help="write the full per-card rows here")
    p.set_defaults(fn=report)

    b = sub.add_parser("baseline", help="freeze per-type rates")
    b.add_argument("paths", nargs="+")
    b.add_argument("--out", required=True)
    b.set_defaults(fn=baseline)

    k = sub.add_parser("check", help="fail if a type's play rate collapsed")
    k.add_argument("paths", nargs="+")
    k.add_argument("--baseline", required=True)
    k.add_argument("--tol", type=float, default=0.35,
                   help="fractional drop tolerated before failing")
    k.set_defaults(fn=check)

    args = ap.parse_args(argv)
    return args.fn(args) or 0


if __name__ == "__main__":
    sys.exit(main())
