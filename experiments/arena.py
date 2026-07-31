"""Head-to-head match machinery shared by evaluate.py and hillclimb.py.

A "duel" puts one challenger (A) at a table of defenders (B) and rotates the
challenger through every seat, so seat order is exactly balanced.  With K
players the null hypothesis is a win rate of 1/K.

Ties share the win: a 2-way tie for first counts 0.5 for each.

Everything is process-parallel (`multiprocessing`): a worker plays one whole
game and returns only the final scores, so nothing large crosses the pipe.
"""
from __future__ import annotations

import json
import math
import multiprocessing as mp
import os
import random
import sys

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

from experiments import paired_stats  # noqa: E402  (needs the path insert)
from engine.bots.weighted import DEFAULT_WEIGHTS  # noqa: E402  (ditto)

BUILTINS = ("random", "greedy", "default")

#: docs/TRAINING_RUN.md:39-44 -- this file holds the pre-horizon-fix 4p
#: champion (science=-6.089) and says explicitly "never warm-start from it".
#: docs/CULTURE_GAP.md Sec 8f measured it at 20.1% against a 25% null once the
#: turns-remaining horizon fix (`e990920`) landed.  Three tools (quiesce_bench,
#: no_credit_check, behaviour_counts) used to default or example their
#: --weights/--spec argument straight to this file and printed numbers for it
#: without warning.  `refuse_if_degenerate_champion` below is the one place
#: any of them -- or anything written later that loads a spec through
#: `load_spec` -- can be routed through so this cannot recur through a
#: different path.
DEGENERATE_CHAMPION_PATH = os.path.join(
    os.path.dirname(os.path.abspath(__file__)), "champion_4p.json")


#: Fraction of the degenerate vector's INFORMATIVE keys a candidate must
#: reproduce exactly before it is treated as a copy of it.  See
#: `_degenerate_match` for why the test is a fraction and not equality.
DEGENERATE_MATCH_FRACTION = 0.5


def _weights_of(path):
    with open(path) as fh:
        d = json.load(fh)
    return d.get("weights", d)


def _informative_keys(known, defaults):
    """The keys of `known` that a hill climb actually MOVED off the default.

    Keys still sitting at their `DEFAULT_WEIGHTS` value carry no provenance --
    every vector agrees with every other vector on those (`DEFAULT_WEIGHTS`
    itself matches the degenerate champion on 20.5% of all keys, purely
    through untouched entries).  Restricting the comparison to the moved keys
    is what makes the match fraction a provenance signal instead of a
    similarity score.
    """
    return [k for k, v in known.items() if v != defaults.get(k)]


def _degenerate_match(mine, known):
    """Fraction of the degenerate vector's informative keys `mine` reproduces.

    WHY THIS IS NOT AN EQUALITY TEST.  The original guard asked for exact
    agreement on every key, which is the one thing a live climb reliably
    breaks: `analysis/frozen/champion_4p.json` is the degenerate vector six
    generations later, differs from it on exactly two keys (`colonies`,
    `pacts` -- and only because 15b9764 reset those two to defaults in the
    committed copy while the on-disk climb kept going), and so walked straight
    through the guard into every 4p number measured against it.

    Exact float agreement is a provenance fingerprint, not a similarity
    measure: two vectors from different climbs essentially never agree
    bit-for-bit on a moved weight.  Measured on the real corpus the split is
    total -- both descendants of the degenerate vector score 1.000, and every
    other champion in the repo (2p/3p frozen, the committed 2p/3p champions,
    all three LIVE league champions) and `DEFAULT_WEIGHTS` itself score
    0.000.  Any threshold in (0, 1) separates them; 0.5 is the middle.
    """
    known_info = _informative_keys(known, DEFAULT_WEIGHTS)
    if not known_info:
        return 0.0
    hits = sum(1 for k in known_info if mine.get(k) == known[k])
    return hits / len(known_info)


def _spec_weight_path(spec):
    """Pull the on-disk weights path out of a raw --weights/--spec string,
    unwrapping a ``quiesce:PATH,opt=1,...`` prefix the same way `load_spec`
    does. Returns None for a builtin ('random'/'greedy'/'default') or an
    empty/absent spec -- neither can be the degenerate file."""
    if not spec or spec in BUILTINS:
        return None
    if spec.startswith("quiesce:"):
        spec = spec[len("quiesce:"):].split(",")[0]
    if not spec or spec in BUILTINS:
        return None
    return spec


def refuse_if_degenerate_champion(spec, tool_name):
    """Hard-refuse (``SystemExit``) if `spec` resolves to the known-degenerate
    ``experiments/champion_4p.json`` vector -- checked by PATH and by
    PROVENANCE (`_degenerate_match` >= `DEGENERATE_MATCH_FRACTION`), so a
    copy, a rename, OR A LATER GENERATION OF THE SAME CLIMB is caught. The
    provenance test replaced an exact-content test that
    `analysis/frozen/champion_4p.json` defeated by differing on two keys. See
    `DEGENERATE_CHAMPION_PATH`'s comment for why this exists. A no-op for
    builtins, empty specs, and any path that isn't descended from that
    vector."""
    path = _spec_weight_path(spec)
    if path is None or not os.path.exists(path):
        return
    known_path = DEGENERATE_CHAMPION_PATH
    if not os.path.exists(known_path):
        return
    same_path = os.path.samefile(path, known_path)
    match = 0.0
    if not same_path:
        try:
            match = _degenerate_match(_weights_of(path),
                                      _weights_of(known_path))
        except (OSError, ValueError):
            match = 0.0
    if same_path or match >= DEGENERATE_MATCH_FRACTION:
        sys.stderr.write(
            "\n" + "!" * 70 + "\n"
            f"! {tool_name}: REFUSING to load {path!r}\n"
            f"! ({'same file as' if same_path else f'{match:.0%} of the moved weights of'})\n"
            "! experiments/champion_4p.json, the\n"
            "! pre-horizon-fix vector docs/TRAINING_RUN.md says never to\n"
            "! warm-start from (science=-6.089; docs/CULTURE_GAP.md Sec 8f\n"
            "! measured it at 20.1% against a 25% null after the horizon\n"
            "! fix landed). Pass a different --weights/--spec -- e.g. a file\n"
            "! under experiments/league_state/ (the live league champion) --\n"
            "! or omit the flag entirely for DEFAULT_WEIGHTS.\n"
            + "!" * 70 + "\n\n")
        raise SystemExit(
            f"{tool_name}: refusing degenerate weights vector {path!r}")


# --------------------------------------------------- is the lever plugged in?
#
# The failure this exists to stop is NOT a crash. `evaluate()` skips whole
# feature functions when their scale weight is 0.0 (`if hp:`, `if ru or rb:`,
# ...), so an A/B whose lever only reaches the score through a closed gate
# returns a clean, well-powered NULL. It is indistinguishable from a real
# negative result and it is not one -- it is an arithmetic identity, and no
# sample size touches it.
#
# That is what happened to the 12,800-game wonder measurement in
# `docs/CARD_BLINDNESS.md` Sec 5.3: both arms differed only in
# `card_rate_credit`, which is read inside `card_potential`, which for a WONDER
# is only ever reached from `row_pressure`, which `evaluate()` gates on
# `row_urgency`/`row_bargain_forgone` -- both 0.0 in the frozen champion the
# run used. See `analysis/frozen/README.md`.

#: Each `evaluate()` feature function that is gated, and the weights that open
#: it. Any one of them being non-zero opens the gate.
EVALUATE_GATES = {
    "hand_potential": ("hand_potential",),
    "wonder_potential": ("wonder_potential",),
    "hand_mil_potential": ("hand_mil_potential",),
    "tactic_terms": ("tactic_gain", "tactic_short"),
    "rival_hand_potential": ("rival_hand_potential",),
    "row_pressure": ("row_urgency", "row_bargain_forgone"),
}

#: A weight read only from inside `card_potential` reaches the score through
#: these consumers and through nothing else.
CARD_POTENTIAL_CONSUMERS = ("hand_potential", "wonder_potential",
                            "hand_mil_potential", "rival_hand_potential",
                            "row_pressure")

#: ...and for a WONDER, only through these. `engine/actions.py:take_card` puts
#: a wonder straight into `p.wonder`, so it never enters `hand_civil` and
#: `hand_potential` never sees one.
WONDER_CARD_POTENTIAL_CONSUMERS = ("wonder_potential", "row_pressure")


def lever_conduction(weights, consumers=CARD_POTENTIAL_CONSUMERS):
    """Split `consumers` into the gates this vector leaves OPEN and CLOSED.

    Returns ``(open_, closed)``, two tuples of feature-function names.
    """
    open_, closed = [], []
    for fn in consumers:
        gates = EVALUATE_GATES.get(fn, ())
        (open_ if any(weights.get(g) for g in gates) else closed).append(fn)
    return tuple(open_), tuple(closed)


def assert_lever_conducts(weights, lever, tool_name,
                          consumers=CARD_POTENTIAL_CONSUMERS):
    """Hard-refuse (``SystemExit``) if `lever` cannot reach the score at all.

    Call this at the top of any A/B whose treatment is a `card_potential`
    multiplier (`card_rate_credit`, `card_board_credit`, `territory_credit`,
    `unit_strength_credit`, ...). Pass `WONDER_CARD_POTENTIAL_CONSUMERS` when
    the question is specifically about wonders.

    A no-op when at least one consumer is open -- it does not check that the
    effect is *large*, only that it is not identically zero.
    """
    open_, closed = lever_conduction(weights, consumers)
    if open_:
        return open_
    gates = sorted({g for fn in closed for g in EVALUATE_GATES.get(fn, ())})
    sys.stderr.write(
        "\n" + "!" * 70 + "\n"
        f"! {tool_name}: REFUSING to measure {lever!r} against this vector.\n"
        "! Every path by which it could reach the score is gated shut:\n"
        + "".join(f"!     {fn}  (needs one of: "
                  f"{', '.join(EVALUATE_GATES.get(fn, ()))})\n"
                  for fn in closed)
        + "! and all of those weights are 0.0 or absent here:\n"
        + "".join(f"!     {g} = {weights.get(g, 'ABSENT')}\n" for g in gates)
        + "! This A/B would return a NULL that is an arithmetic identity, not\n"
        "! a result -- exactly the way the 12,800-game wonder measurement in\n"
        "! docs/CARD_BLINDNESS.md Sec 5.3 did. Use a vector that carries the\n"
        "! gating weights (e.g. a live league champion), or measure something\n"
        "! this vector can actually express.\n"
        + "!" * 70 + "\n\n")
    raise SystemExit(
        f"{tool_name}: {lever!r} cannot reach the score under this vector "
        f"(all of {', '.join(gates)} are 0.0/absent)")


# ------------------------------------------------------------ bot specs

def load_spec(spec):
    """Turn a CLI bot spec into something picklable.

    ``random`` / ``greedy`` / ``default`` are the built-in bots; anything
    else is a path to a JSON weight file (either a bare dict or a
    ``{"weights": {...}}`` checkpoint).

    A ``quiesce:`` prefix runs the SAME weights under
    :class:`engine.bots.quiescent.QuiescentBot` instead of the 1-ply
    ``WeightedBot``, so ``--a quiesce:experiments/league_state/champion_4p.json
    --b experiments/league_state/champion_4p.json`` is an exact search-only
    A/B.  Optional
    tuning follows the path, comma-separated: ``quiesce:FILE,depth=8,nodes=300,
    war=0``.  The returned spec is a plain tuple/dict, still picklable.
    """
    if spec in BUILTINS:
        return spec
    if spec.startswith("neural:"):
        # `neural:CKPT.pt,det=1,etb=0` -- the value net inside the 1-ply search
        # (engine/bots/neural_bot.py).  Torch is imported lazily in make_bot,
        # so this branch and load_spec stay torch-free on the Mac.  NOTE: for a
        # multi-game duel prefer experiments/neural_eval.py (single process,
        # one GPU-resident model); through arena's process pool each worker
        # reloads the checkpoint per game.
        rest = spec[len("neural:"):].split(",")
        path, opts = rest[0], {}
        for kv in rest[1:]:
            if not kv:
                continue
            k, _, v = kv.partition("=")
            opts[k.strip()] = v
        return ("neural", path, opts)
    if spec.startswith("nplan:"):
        # `nplan:CKPT.pt,width=8,det=1,war=1,nodes=1200` -- the value net as the
        # LEAF of PlanBot's whole-turn beam (engine/bots/neural_plan.py).  So
        # `--a nplan:c.pt --b neural:c.pt` is an exact search-only A/B on one
        # evaluator, and `--a nplan:c.pt --b plan:champ.json` is an exact
        # evaluator-only A/B at one search.  Torch is imported lazily in
        # make_bot, so this branch stays torch-free on the Mac.
        rest = spec[len("nplan:"):].split(",")
        path, opts = rest[0], {}
        for kv in rest[1:]:
            if not kv:
                continue
            k, _, v = kv.partition("=")
            opts[k.strip()] = v
        return ("nplan", path, opts)
    if spec.startswith("plan:"):
        # `plan:FILE,width=8,samples=1,det=1,war=1` -- whole-turn beam search under
        # the SAME weights, so `--a plan:champ.json --b champ.json` is an
        # exact search-only A/B (engine/bots/plan.py).
        rest = spec[len("plan:"):].split(",")
        path, opts = rest[0], {}
        for kv in rest[1:]:
            if not kv:
                continue
            k, _, v = kv.partition("=")
            opts[k.strip()] = int(v)
        inner = "default" if path in ("", "default") else load_spec(path)
        return ("plan", inner, opts)
    if spec.startswith("quiesce:"):
        rest = spec[len("quiesce:"):].split(",")
        path, opts = rest[0], {}
        for kv in rest[1:]:
            if not kv:
                continue
            k, _, v = kv.partition("=")
            opts[k.strip()] = int(v)
        inner = "default" if path in ("", "default") else load_spec(path)
        return ("quiescent", inner, opts)
    from engine.bots.weighted import load_weights
    return load_weights(spec)


def make_bot(spec, seed):
    from engine import bots as B
    if isinstance(spec, tuple) and spec and spec[0] == "neural":
        # lazy: torch only imported when a neural spec is actually built
        from engine.bots.neural_net import NeuralValue
        from engine.bots.neural_bot import NeuralBot
        _, path, opts = spec
        import os
        device = opts.get("device") or ("cuda" if os.environ.get(
            "NEURAL_DEVICE", "cuda") == "cuda" else "cpu")
        value = NeuralValue.from_checkpoint(path, device)
        return NeuralBot(value, seed=seed,
                         determinize=bool(int(opts.get("det", 1))),
                         end_turn_bias=float(opts.get("etb", 0.0)))
    if isinstance(spec, tuple) and spec and spec[0] == "nplan":
        from engine.bots.neural_net import NeuralValue
        from engine.bots.neural_plan import NeuralPlanBot
        _, path, opts = spec
        import os
        device = opts.get("device") or os.environ.get("NEURAL_DEVICE", "cuda")
        value = NeuralValue.from_checkpoint(path, device)
        return NeuralPlanBot(value, seed=seed,
                             width=(int(opts["width"]) if "width" in opts
                                    else None),
                             samples=(int(opts["samples"]) if "samples" in opts
                                      else None),
                             max_nodes=(int(opts["nodes"]) if "nodes" in opts
                                        else None),
                             determinize=bool(int(opts.get("det", 1))),
                             war_lookahead=(None if "war" not in opts
                                            else bool(int(opts["war"]))),
                             quiet_pending=(None if "qp" not in opts
                                            else bool(int(opts["qp"]))))
    if isinstance(spec, tuple) and spec and spec[0] == "plan":
        from engine.bots.plan import PlanBot
        _, inner, opts = spec
        w = None if inner == "default" else inner
        return PlanBot(weights=w, seed=seed,
                       width=opts.get("width"),
                       samples=opts.get("samples"),
                       determinize=bool(opts.get("det", 1)),
                       war_lookahead=(None if "war" not in opts
                                      else bool(opts["war"])),
                       quiet_pending=(None if "qp" not in opts
                                      else bool(opts["qp"])),
                       pending_determinize=(None if "qd" not in opts
                                            else bool(opts["qd"])))
    if isinstance(spec, tuple) and spec and spec[0] == "quiescent":
        from engine.bots.quiescent import QuiescentBot
        _, inner, opts = spec
        w = None if inner == "default" else inner
        return QuiescentBot(weights=w, seed=seed,
                            levels=opts.get("levels"),
                            max_depth=opts.get("depth"),
                            max_nodes=opts.get("nodes"),
                            war_lookahead=(None if "war" not in opts
                                           else bool(opts["war"])))
    if spec == "random":
        return B.RandomBot(seed=seed)
    if spec == "greedy":
        return B.GreedyBot(seed=seed)
    if spec == "default":
        return B.WeightedBot(seed=seed)
    return B.WeightedBot(weights=spec, seed=seed)


def spec_name(spec, fallback):
    if isinstance(spec, tuple) and spec and spec[0] in ("quiescent", "plan"):
        return spec[0]
    return spec if isinstance(spec, str) else fallback


# --------------------------------------------------------------- worker

_W = {}


def _init(a, b, num_players, move_cap):
    _W["a"], _W["b"] = a, b
    _W["n"] = num_players
    _W["cap"] = move_cap


def _play(task):
    """task = (game_index, seed, seat_of_A)
    -> (share_a, culture_a, mean_culture_others, best_culture_other, moves)

    `best_culture_other` is `max` over the defender seats, not the mean.  It is
    the quantity the WIN CONDITION is defined on: `share > 0` iff A's culture
    is >= the best other seat's, so `culture_a - best_culture_other >= 0` and
    "A won or tied" are the same statement about the same numbers, computed
    three lines apart in this function.  See `duel`'s `per_game_lead`.
    """
    from engine import game
    gi, seed, seat = task
    n = _W["n"]
    b = _W["b"]
    if isinstance(b, list):
        # A *field*: the defender seats are drawn from a pool.  The draw is
        # keyed only on `seed`, never on the challenger, so two duels run with
        # the same seeds face byte-identical opposition and can be paired.
        r = random.Random(seed * 31 + 7)
        others = [b[r.randrange(len(b))] for _ in range(n - 1)]
    else:
        others = [b] * (n - 1)
    specs = others[:seat] + [_W["a"]] + others[seat:]
    bots = [make_bot(s, seed * 97 + i * 13 + 1) for i, s in enumerate(specs)]
    try:
        st = game.play_game(bots, n, seed=seed, move_cap=_W["cap"])
        sc = game.scores(st)
        moves = getattr(st, "moves_played", 0)
    except Exception as e:  # engine bug: report, do not kill the tournament
        # What travels back is a DIAGNOSABLE record, not a bare "a game died":
        # the exception type, its repr, the deepest frame that raised, and the
        # seed that reproduces it.  A count alone is what let a fully-broken
        # engine look like a quiet run (docs/HAZARDS.md trap 8); the repr is
        # what turns the alarm into a fix.  Plain str/int only, because this
        # crosses a multiprocessing pipe and an arbitrary exception object may
        # not pickle.
        tb, where = e.__traceback__, ""
        while tb is not None:
            where = (f"{os.path.basename(tb.tb_frame.f_code.co_filename)}"
                     f":{tb.tb_lineno}")
            tb = tb.tb_next
        return (None, {"type": type(e).__name__, "repr": repr(e),
                       "where": where, "seed": seed}, None, None, 0)
    best = max(sc)
    tied = [i for i, v in enumerate(sc) if v == best]
    share = (1.0 / len(tied)) if seat in tied else 0.0
    others = [sc[i] for i in range(n) if i != seat]
    return (share, sc[seat], sum(others) / len(others), max(others), moves)


# ----------------------------------------------------------------- stats

def mean_ci(xs, z=1.96):
    """Mean and half-width of a normal-approximation confidence interval."""
    n = len(xs)
    if n == 0:
        return 0.0, 0.0
    m = sum(xs) / n
    if n < 2:
        return m, 1.0
    var = sum((x - m) ** 2 for x in xs) / (n - 1)
    return m, z * math.sqrt(var / n)


def p_value(mean, half, null):
    """Two-sided normal p-value that the observed mean differs from `null`."""
    if half <= 0:
        return 1.0
    se = half / 1.96
    z = abs(mean - null) / se
    return math.erfc(z / math.sqrt(2))


# ------------------------------------------------------- error reporting

def census(errors):
    """Error records -> ``{exception type: {count, repr, where, seed}}``.

    One worked example per distinct type, plus how many games that type ate.
    Grouping by type rather than keeping every repr keeps the record small
    enough to log and to store in a generation record, while still naming the
    thing that has to be fixed.
    """
    out = {}
    for e in errors:
        slot = out.setdefault(e["type"], {"count": 0, "repr": e["repr"],
                                          "where": e["where"],
                                          "seed": e["seed"]})
        slot["count"] += 1
    return out


def error_brief(types):
    """A one-line human summary of a `census` mapping, worst type first."""
    if not types:
        return ""
    return "; ".join(
        f"{v['repr']} at {v['where']} x{v['count']} (seed {v['seed']})"
        for _, v in sorted(types.items(), key=lambda kv: (-kv[1]["count"],
                                                          kv[0])))


# ------------------------------------------------------------------ duel

def duel(a, b, num_players, games, seed0=0, workers=None, move_cap=20000,
         chunk=4):
    """Play `games` games of A-vs-table-of-B, seat-rotated.

    `b` may be a single spec (every defender seat plays it -- then the null
    win rate really is 1/num_players) or a *list* of specs, a "field", from
    which each defender seat is drawn.  Against a field 1/num_players is no
    longer the right null, so a field duel is only meaningful when compared
    against a second duel run on the same seeds: see `hillclimb.challenge`.

    Returns a dict with the win share, its confidence interval, mean
    cultures and the number of games actually completed.  `per_game` is the
    task-ordered share list (None for a game the engine could not finish),
    which is what makes two duels on the same seeds pairable.

    `per_game_margin` is the same list in the same order for the CULTURE
    MARGIN, ``A's final culture - the mean of the defenders'``.  Win share is
    a step function -- against an opponent nobody beats it is 0.0 on every
    game and two policies that lose by 8 and by 90 are indistinguishable --
    whereas the margin is a dense signal that exists on every single game.
    `experiments/hillclimb_league.py` scores its gate tier on it for exactly
    that reason (docs/LEAGUE_TRAINING.md, "The pool is too hard at the
    bottom").  The mean of this list is `culture_a - culture_b`; it is
    returned per game because pairing against a reference duel has to happen
    game by game.

    `errors` is how many games the engine could not finish, `error_types` maps
    each distinct exception type name to ``{count, repr, where, seed}`` -- one
    worked example per type -- and `error_sample` keeps the first three reprs
    as plain strings.  Callers that run unattended (`hillclimb_league.py`) act
    on `error_types`: a swallowed exception the caller cannot see is the same
    thing as no exception at all.

    `per_game_culture` is the same list again for **A's own final culture**,
    which is the quantity the human baseline is quoted in.  It is not
    derivable from the others: a DIFFERENCE cannot tell "I scored 140, they
    scored 60" from "I scored 80, they scored 0", so it is reported on every
    row as a diagnostic whatever the objective is.

    `per_game_lead` is **A's final culture minus the BEST defender's**, and it
    is what `experiments/hillclimb_league.py` trains on
    (docs/LEAGUE_OBJECTIVE.md).  Its sign is the game result exactly:
    `per_game_lead[i] >= 0` iff `per_game[i] > 0`, because `_play` derives both
    from the same `max` over the same score list.  Note this is NOT
    `per_game_margin`: that one is A minus the *mean* of the defenders, whose
    sign is not the win condition at 3p/4p (you can be above the mean and
    third), and which moves when you beat up a player who is not contending.
    """
    tasks = []
    for g in range(games):
        seat = g % num_players
        seed = seed0 + g // num_players
        tasks.append((g, seed * 7919 + 17, seat))

    workers = workers or max(1, min(len(os.sched_getaffinity(0))
                                    if hasattr(os, "sched_getaffinity")
                                    else (os.cpu_count() or 2),
                                    (os.cpu_count() or 2)) - 1)
    out = []
    if workers <= 1:
        _init(a, b, num_players, move_cap)
        out = [_play(t) for t in tasks]
    else:
        ctx = mp.get_context("fork" if sys.platform != "win32" else "spawn")
        with ctx.Pool(workers, initializer=_init,
                      initargs=(a, b, num_players, move_cap)) as pool:
            out = pool.map(_play, tasks, chunksize=chunk)

    shares, ca, cb, cbest, moves, errors = [], [], [], [], [], []
    per_game = []                      # task-ordered, None where the game died
    per_game_margin = []               # ditto, culture_a - mean(defenders)
    per_game_culture = []              # ditto, culture_a on its own
    per_game_lead = []                 # ditto, culture_a - max(defenders)
    for share, x, y, zb, m in out:
        per_game.append(share)
        per_game_margin.append(None if share is None else float(x - y))
        per_game_culture.append(None if share is None else float(x))
        per_game_lead.append(None if share is None else float(x - zb))
        if share is None:
            errors.append(x)
            continue
        shares.append(share)
        ca.append(x)
        cb.append(y)
        cbest.append(zb)
        moves.append(m)
    m, half = mean_ci(shares)
    null = 1.0 / num_players
    # `ci` below is the LEGACY independent-samples half-width, kept byte-stable
    # because the halted league arms and every historical log are calibrated to
    # it.  It is wrong for this design: the task list above deals each seed
    # `num_players` times with the seats swapped, so the independent unit is
    # the deal, not the game.  `ci_paired` is the correct one -- prefer it, and
    # see experiments/paired_stats.py for why the correction is not a blanket
    # sqrt(2) (rho is usually negative here, so pairing usually TIGHTENS it).
    pe = paired_stats.paired(per_game, num_players)
    return {
        "error_types": census(errors),
        "players": num_players,
        "games": len(shares),
        "requested": games,
        "win_rate": m,
        "ci": half,
        "null": null,
        "p": p_value(m, half, null),
        "culture_a": (sum(ca) / len(ca)) if ca else 0.0,
        "culture_b": (sum(cb) / len(cb)) if cb else 0.0,
        "moves": (sum(moves) / len(moves)) if moves else 0.0,
        "errors": len(errors),
        "error_sample": [e["repr"] for e in errors[:3]],
        "margin": ((sum(ca) / len(ca)) - (sum(cb) / len(cb))) if ca else 0.0,
        "culture_best": (sum(cbest) / len(cbest)) if cbest else 0.0,
        "lead": ((sum(ca) / len(ca)) - (sum(cbest) / len(cbest))) if ca else 0.0,
        "shares": shares,
        "per_game": per_game,
        "per_game_margin": per_game_margin,
        "per_game_culture": per_game_culture,
        "per_game_lead": per_game_lead,
        # --- correct, deal-clustered interval (see paired_stats.py) ---
        "ci_paired": pe.half,
        "ci_naive": half,
        "p_paired": pe.p_against(null),
        "deals": pe.n_clusters,
        "rho_deal": pe.rho,
        "deff": pe.deff,
    }


def fmt(res, name_a="A", name_b="B"):
    # Headline the deal-clustered interval; keep the legacy one visible in
    # brackets so this can still be reconciled against older logs.  Rows read
    # back from pre-2026-07-30 jsonl have no `ci_paired`, hence the fallback.
    half = res.get("ci_paired", res["ci"])
    p = res.get("p_paired", res["p"])
    legacy = ""
    if "ci_paired" in res:
        legacy = (f" [deals={res['deals']}, rho={res['rho_deal']:+.2f}; "
                  f"legacy per-game ci {res['ci_naive']:.1%}]")
    return (f"{name_a} vs {name_b} @{res['players']}p: "
            f"win rate {res['win_rate']:.1%} +/- {half:.1%} "
            f"(null {res['null']:.1%}, p={p:.4f}, n={res['games']}){legacy} "
            f"culture {res['culture_a']:.0f} vs {res['culture_b']:.0f}"
            + (f" [{res['errors']} engine errors: "
               f"{error_brief(res.get('error_types'))}]"
               if res["errors"] else ""))
