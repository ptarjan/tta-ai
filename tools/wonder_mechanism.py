"""Does pricing `effects.culture`/`effects.science` change WONDER building?

docs/CARD_BLINDNESS.md section 5.3.  The section 5 A/B reprices ten cards --
eight wonders and two leaders -- and cannot say which of them carries the
+9.5pp.  This tool answers that by counting behaviour instead of wins.

    # 3200-game contended duel: both arms at the same table, seat-rotated
    nice -n 19 python3 tools/wonder_mechanism.py --mode duel \
      --a analysis/cardblind/champ2p_credit1.json \
      --b analysis/cardblind/champ2p_credit0.json --deals 1600 --out /tmp/duel.jsonl

    # mirror tables on IDENTICAL deals, one run per arm
    nice -n 19 python3 tools/wonder_mechanism.py --mode mirror \
      --a analysis/cardblind/champ2p_credit1.json --deals 3200 \
      --tag m_credit1 --out /tmp/mirror.jsonl
    nice -n 19 python3 tools/wonder_mechanism.py --mode mirror \
      --a analysis/cardblind/champ2p_credit0.json --deals 3200 \
      --tag m_credit0 --out /tmp/mirror.jsonl

    nice -n 19 python3 tools/wonder_mechanism.py --report \
      --duel /tmp/duel.jsonl --mirror /tmp/mirror.jsonl

TWO DESIGNS, and the difference matters:

* `duel` puts both arms at the same table.  It is the regime section 5's
  59.53% was measured in, and this tool reproduces that headline (59.08%,
  culture 151.0 vs 140.9) which is the validation that it measures the same
  thing.  But the two arms COMPETE for the same wonder row: if one takes fewer
  wonders the other mechanically gets more, so any difference is inflated.
* `mirror` runs every seat on one arm and runs the two arms over identical
  deals and bot seeds.  No cross-arm contention.  This is the honest
  behavioural number and the one section 5.3 leads with.

THE TRAP THIS TOOL EXISTS TO AVOID.  The first version of this probe
monkeypatched `engine.actions.take_card` and reported **57 wonder takes per
game**.  The 1-ply search applies every candidate move to a COPY of the state,
so an engine-level patch counts hypotheticals.  The real number is ~0.15.
Everything here is therefore read from one of two contamination-proof sources:

* the REAL final state (`p.completed_wonders`, `p.wonder`), which the search
  cannot touch; and
* a wrapper on the seat's bot that records only the move actually CHOSEN --
  the only way to see a programme that was started and then antiquated away,
  and the civil actions spent on stages.
"""

from __future__ import annotations

import argparse
import collections
import json
import math
import multiprocessing as mp
import os
import sys
import time

_ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..")
sys.path.insert(0, _ROOT)

from experiments import paired_stats  # noqa: E402  (needs the path insert)

DUEL_PATH = MIRROR_PATH = ""

_W = {}


class Rec:
    """Wraps one seat's bot; tallies only the moves it actually plays."""

    def __init__(self, bot, idx, db):
        self.bot, self.idx, self.db = bot, idx, db
        self.takes = []        # wonder names taken, in order
        self.stage_moves = 0   # ("wonder_step", k) chosen == 1 civil action
        self.stages = 0        # sum of k
        self.stage_by_name = {}
        self.first_take_round = {}
        self.all_takes = []      # every card name taken from the row
        self.leaders_played = []

    def _note(self, state, mv):
        if not mv:
            return mv
        k = mv[0]
        if k == "take":
            name = state.card_row[mv[1]]
            if name:
                self.all_takes.append(name)
            if name and (self.db.by_name.get(name) or {}).get("type") == "wonder":
                self.takes.append(name)
                self.first_take_round.setdefault(name, state.round)
        elif k == "play_leader":
            self.leaders_played.append(mv[1])
        elif k == "wonder_step":
            p = state.players[self.idx]
            self.stage_moves += 1
            self.stages += int(mv[1])
            if p.wonder:
                self.stage_by_name[p.wonder.name] = (
                    self.stage_by_name.get(p.wonder.name, 0) + int(mv[1]))
        return mv

    def __call__(self, state):
        return self._note(state, self.bot(state))

    def choose(self, state, moves, rng=None):
        return self._note(state, self.bot.choose(state, moves, rng))


def _init(a, b, n, cap, mode):
    _W.update(a=a, b=b, n=n, cap=cap, mode=mode)


def _play(task):
    from engine import game, cards
    from experiments.arena import make_bot
    db = cards.db()
    gi, seed, seat = task
    n, mode = _W["n"], _W["mode"]
    if mode == "duel":
        specs = [_W["b"]] * n
        specs[seat] = _W["a"]
        arm = ["b"] * n
        arm[seat] = "a"
    else:
        specs = [_W["a"]] * n
        arm = ["a"] * n
    recs = [Rec(make_bot(s, seed * 97 + i * 13 + 1), i, db)
            for i, s in enumerate(specs)]
    try:
        st = game.play_game(recs, n, seed=seed, move_cap=_W["cap"])
        sc = game.scores(st)
    except Exception as e:  # noqa: BLE001
        return {"gi": gi, "seed": seed, "error": repr(e)[:200]}

    best = max(sc)
    tied = [i for i, v in enumerate(sc) if v == best]
    seats = []
    for i, p in enumerate(st.players):
        r = recs[i]
        done = list(p.completed_wonders)
        # stages that reached the board: full stage count of every completed
        # wonder, plus the part-built one still standing.
        done_stages = sum(len((db.by_name.get(w) or {}).get("stages") or ())
                          for w in done)
        inprog = ([p.wonder.name, p.wonder.steps_built] if p.wonder else None)
        rem = list(r.takes)
        for d in done:
            if d in rem:
                rem.remove(d)
        seats.append({
            "arm": arm[i],
            "takes": r.takes,               # wonder programmes begun
            "completed": done,
            "unfinished": rem,              # begun and not on the board at end
            "inprog_end": inprog,
            "flipped": list(getattr(p, "flipped_wonders", []) or []),
            "destroyed": int(getattr(p, "destroyed_wonders", 0) or 0),
            "stage_moves": r.stage_moves,   # civil actions spent on stages
            "stages": r.stages,
            "stage_by_name": r.stage_by_name,
            "done_stages": done_stages,
            "take_round": r.first_take_round,
            "all_takes": r.all_takes,
            "leaders_played": r.leaders_played,
            "score": sc[i],
            "win": (1.0 / len(tied)) if i in tied else 0.0,
        })
    return {"gi": gi, "seed": seed, "seat_a": seat, "seats": seats,
            "rounds": getattr(st, "round", None)}



Z, K = 1.96, 2.80

NEW6 = ["Hanging Gardens", "Great Wall", "St. Peter's Basilica",
        "Taj Mahal", "Eiffel Tower", "Kremlin"]
FROM_NOTHING = ["Library of Alexandria", "Universitas Carolina"]
UNPRICED5 = ["Ocean Liners", "First Space Flight", "Fast Food Chains",
             "Internet", "Hollywood"]
PRICED_UNCHANGED = ["Pyramids", "Colossus", "Transcontinental Railroad"]


DEFAULT_ARMS = ("analysis/cardblind/champ2p_credit1.json",
                "analysis/cardblind/champ2p_credit0.json")


def deltas(arms=None):
    from engine.bots.weighted import card_potential, load_weights
    from engine import cards as C
    a1, a0 = arms or DEFAULT_ARMS
    w1 = load_weights(a1 if os.path.isabs(a1) else os.path.join(_ROOT, a1))
    w0 = load_weights(a0 if os.path.isabs(a0) else os.path.join(_ROOT, a0))
    return {c["name"]: card_potential(c["name"], w1) - card_potential(c["name"], w0)
            for c in C.db().cards if c["type"] == "wonder"}


def stats(dv):
    """Interval on a list of PAIRED per-unit differences (arm A - arm B).

    Routed through `experiments.paired_stats.cluster_ci` rather than an
    open-coded `1.96 * sqrt(var/n)` so there is one estimator in the repo and
    it is the corrected one (commit `6d6fec1`, `tests/test_paired_stats.py`).
    In `mirror` mode each element is already one DEAL -- both arms play the
    same deal and the per-seat values are averaged before they get here -- so
    clustering on the element is clustering on the deal, which is the unit the
    design randomises. The change from the previous open-coded version is the
    t correction, worth ~0.1% at n=1600.

    NOTE for `duel` mode: there each element is one GAME, and a 2p duel is
    seat-paired, so the independent unit is still the deal and this is one
    level too fine. The duel columns are kept for continuity with the
    published table; the mirror columns are the ones to quote.
    """
    est = paired_stats.cluster_ci(dv, use_t=True, unit="deal")
    return est.mean, est.half, est.p_against(0.0), K * est.se


def row(lab, av, bv, extra=""):
    n = len(av)
    m, h, p, mde = stats([x - y for x, y in zip(av, bv)])
    print(f"{lab:34s} {sum(av)/n:8.4f} {sum(bv)/n:8.4f} {m:+8.4f} "
          f"[{m-h:+7.4f},{m+h:+7.4f}] {p:7.4f} {mde:7.4f} {extra}")


def hdr(title, n, unit):
    print(f"\n{'='*104}\n{title}   n = {n} {unit}\n{'='*104}")
    print(f"{'metric':34s} {'credit1':>8s} {'credit0':>8s} {'diff':>8s} "
          f"{'95% CI':>17s} {'p':>7s} {'MDE':>7s}")


def load_duel():
    rs = [json.loads(l) for l in open(DUEL_PATH)]
    rs = [r for r in rs if "error" not in r]
    A = [next(s for s in r["seats"] if s["arm"] == "a") for r in rs]
    B = [next(s for s in r["seats"] if s["arm"] == "b") for r in rs]
    return A, B


MIRROR_TAGS = ("m_credit1", "m_credit0")


def load_mirror():
    rs = [json.loads(l) for l in open(MIRROR_PATH)]
    rs = [r for r in rs if "error" not in r]
    by = collections.defaultdict(dict)
    for r in rs:
        by[r["seed"]][r["tag"]] = r
    ta, tb = MIRROR_TAGS
    A, B = [], []          # per-deal, values SUMMED over the 2 seats
    for _, d in sorted(by.items()):
        if ta in d and tb in d:
            A.append(d[ta]["seats"])
            B.append(d[tb]["seats"])
    if not A:
        seen = sorted({r["tag"] for r in rs})
        raise SystemExit(
            f"no deal carries both {ta!r} and {tb!r}; tags present: {seen}. "
            f"Pass --tag-a/--tag-b.")
    return A, B


def agg(seats_list, fn):
    """seats_list is either a list of seat dicts, or a list of lists of seats."""
    out = []
    for s in seats_list:
        out.append(fn(s) if isinstance(s, dict)
                   else sum(fn(x) for x in s) / len(s))
    return out


def block(name, A, B, n_unit, delt):
    hdr(name, len(A), n_unit)
    row("wonders COMPLETED", agg(A, lambda s: len(s["completed"])),
        agg(B, lambda s: len(s["completed"])))
    row("wonders STARTED (taken)", agg(A, lambda s: len(s["takes"])),
        agg(B, lambda s: len(s["takes"])))
    row("started but NOT FINISHED", agg(A, lambda s: len(s["unfinished"])),
        agg(B, lambda s: len(s["unfinished"])))
    row("  of which still in progress", agg(A, lambda s: 1.0 if s["inprog_end"] else 0.0),
        agg(B, lambda s: 1.0 if s["inprog_end"] else 0.0))
    row("finish rate (completed/started)",
        agg(A, lambda s: len(s["completed"]) / max(1e-9, len(s["takes"])) if s["takes"] else 0.0),
        agg(B, lambda s: len(s["completed"]) / max(1e-9, len(s["takes"])) if s["takes"] else 0.0))
    row("CIVIL ACTIONS on wonder stages", agg(A, lambda s: s["stage_moves"]),
        agg(B, lambda s: s["stage_moves"]))
    row("wonder stages built", agg(A, lambda s: s["stages"]),
        agg(B, lambda s: s["stages"]))
    print()
    groups = [("newly priced (6, gained culture_rate)", NEW6),
              ("from nothing to real numbers (2)", FROM_NOTHING),
              ("  -> all 8 REPRICED", NEW6 + FROM_NOTHING),
              ("still unpriced (5, text-effect)", UNPRICED5),
              ("priced but UNCHANGED (3)", PRICED_UNCHANGED),
              ("  -> all 8 UNREPRICED", UNPRICED5 + PRICED_UNCHANGED)]
    for lab, g in groups:
        gs = set(g)
        av = agg(A, lambda s, gs=gs: sum(1 for w in s["completed"] if w in gs))
        bv = agg(B, lambda s, gs=gs: sum(1 for w in s["completed"] if w in gs))
        rel = ((sum(av) / max(1e-9, sum(bv))) - 1) * 100 if sum(bv) else 0.0
        row("COMPLETED " + lab, av, bv, f"{rel:+6.1f}%")
    print()
    for lab, g in groups:
        gs = set(g)
        av = agg(A, lambda s, gs=gs: sum(1 for w in s["takes"] if w in gs))
        bv = agg(B, lambda s, gs=gs: sum(1 for w in s["takes"] if w in gs))
        rel = ((sum(av) / max(1e-9, sum(bv))) - 1) * 100 if sum(bv) else 0.0
        row("STARTED " + lab, av, bv, f"{rel:+6.1f}%")

    print(f"\n-- per-wonder, {n_unit} --")
    print(f"{'wonder':27s} {'reprice':>8s} {'compl1':>8s} {'compl0':>8s} "
          f"{'diff':>8s} {'p':>7s} | {'start1':>7s} {'start0':>7s} {'diff':>8s} {'p':>7s}")
    for grp, names in (("NEWLY PRICED (6)", NEW6),
                       ("FROM NOTHING (2)", FROM_NOTHING),
                       ("STILL UNPRICED (5)", UNPRICED5),
                       ("PRICED, UNCHANGED (3)", PRICED_UNCHANGED)):
        print(f"  --- {grp} ---")
        for w in names:
            ca = agg(A, lambda s, w=w: s["completed"].count(w))
            cb = agg(B, lambda s, w=w: s["completed"].count(w))
            sa = agg(A, lambda s, w=w: s["takes"].count(w))
            sb = agg(B, lambda s, w=w: s["takes"].count(w))
            mc, _, pc, _ = stats([x - y for x, y in zip(ca, cb)])
            ms, _, ps, _ = stats([x - y for x, y in zip(sa, sb)])
            n = len(ca)
            print(f"{w:27s} {delt[w]:8.1f} {sum(ca)/n:8.4f} {sum(cb)/n:8.4f} "
                  f"{mc:+8.4f} {pc:7.4f} | {sum(sa)/n:7.4f} {sum(sb)/n:7.4f} "
                  f"{ms:+8.4f} {ps:7.4f}")


def distribution(label, A, B):
    """A, B are flat lists of seat dicts."""
    print(f"\n-- {label}: distribution of wonders completed, per SEAT-GAME --")
    n = len(A)
    ca = collections.Counter(min(len(s["completed"]), 3) for s in A)
    cb = collections.Counter(min(len(s["completed"]), 3) for s in B)
    print(f"{'wonders':>8s} {'credit1':>16s} {'credit0':>16s}")
    for k in range(4):
        lab = "3+" if k == 3 else str(k)
        print(f"{lab:>8s} {ca[k]:7d} ({ca[k]/n:6.2%}) {cb[k]:7d} ({cb[k]/n:6.2%})")
    print(f"{'total':>8s} {n:7d}          {n:7d}")


def report_main(arms=None):
    d = deltas(arms)
    label = ("the frozen 2p champion" if arms is None
             else f"{os.path.basename(arms[0])} vs {os.path.basename(arms[1])}")
    print("docs/CARD_BLINDNESS.md section 5.3.  Engine read-only.")
    print(f"\ncard_potential delta (credit1 - credit0) under {label}:")
    for w in sorted(d, key=lambda x: -d[x]):
        tag = ("NEWLY PRICED" if w in NEW6 else
               "FROM NOTHING" if w in FROM_NOTHING else
               "still unpriced" if w in UNPRICED5 else "priced, unchanged")
        print(f"   {w:27s} {d[w]:+7.2f}   {tag}")

    if DUEL_PATH:
        A, B = load_duel()
        block("DUEL (contended): credit1 vs credit0 at the SAME table, "
              "seat-rotated", A, B, "games", d)
        distribution("DUEL", A, B)

    if MIRROR_PATH:
        MA, MB = load_mirror()
        block("MIRROR (uncontended): every seat one arm, paired on the deal. "
              "Per-seat means.", MA, MB, "deals", d)
        distribution("MIRROR", [s for g in MA for s in g],
                     [s for g in MB for s in g])






def main(argv=None):
    ap = argparse.ArgumentParser(
        description="wonder-behaviour probe for CARD_BLINDNESS 5.3")
    ap.add_argument("--a", default="")
    ap.add_argument("--b", default="")
    ap.add_argument("--mode", choices=("duel", "mirror"), default="")
    ap.add_argument("--report", action="store_true")
    ap.add_argument("--duel", default="")
    ap.add_argument("--mirror", default="")
    ap.add_argument("--players", type=int, default=2)
    ap.add_argument("--deals", type=int, default=100)
    ap.add_argument("--seed0", type=int, default=0)
    ap.add_argument("--workers", type=int, default=2)
    ap.add_argument("--tag", default="")
    ap.add_argument("--out", default="")
    ap.add_argument("--tag-a", default="", help="mirror tag for the credit1 "
                    "arm (default m_credit1)")
    ap.add_argument("--tag-b", default="", help="mirror tag for the credit0 "
                    "arm (default m_credit0)")
    ap.add_argument("--no-lever-check", action="store_true",
                    help="run even if the weight vector gates shut every path "
                         "a wonder's card_potential could use. Only for "
                         "reproducing the original (identically-zero) result.")
    args = ap.parse_args(argv)

    global DUEL_PATH, MIRROR_PATH, MIRROR_TAGS
    if args.report:
        DUEL_PATH, MIRROR_PATH = args.duel, args.mirror
        if args.tag_a or args.tag_b:
            MIRROR_TAGS = (args.tag_a or MIRROR_TAGS[0],
                           args.tag_b or MIRROR_TAGS[1])
        arms = (args.a, args.b) if (args.a and args.b) else None
        return report_main(arms)
    if not (args.a and args.mode and args.out):
        ap.error("--a, --mode and --out are required unless --report")

    from experiments import arena
    a = arena.load_spec(args.a)
    b = arena.load_spec(args.b) if args.b else a
    n = args.players

    # This probe's whole subject is whether a WONDER's `card_potential` moves
    # the policy. If the vector gates shut every path by which it could, the
    # run returns a null that is an arithmetic identity -- which is exactly
    # what the original 12,800-game run did (see docs/CARD_BLINDNESS.md 5.3
    # and analysis/frozen/README.md). Refuse rather than produce it again.
    if not args.no_lever_check:
        from engine.bots.weighted import load_weights
        for spec in (args.a, args.b):
            path = arena._spec_weight_path(spec)
            if path and os.path.exists(path):
                arena.assert_lever_conducts(
                    load_weights(path), "card_rate_credit",
                    "wonder_mechanism.py",
                    arena.WONDER_CARD_POTENTIAL_CONSUMERS)

    tasks = []
    if args.mode == "duel":
        # exactly arena.duel: games = deals * players, seat rotated
        for g in range(args.deals * n):
            d = args.seed0 + g // n
            tasks.append((g, d * 7919 + 17, g % n))
    else:
        for d in range(args.seed0, args.seed0 + args.deals):
            tasks.append((d, d * 7919 + 17, 0))

    t0 = time.time()
    ctx = mp.get_context("fork")
    with ctx.Pool(args.workers, initializer=_init,
                  initargs=(a, b, n, 20000, args.mode)) as pool:
        out = pool.map(_play, tasks, chunksize=4)
    os.makedirs(os.path.dirname(os.path.abspath(args.out)), exist_ok=True)
    with open(args.out, "a") as fh:
        for r in out:
            r["tag"] = args.tag
            r["mode"] = args.mode
            fh.write(json.dumps(r) + "\n")
    errs = sum(1 for r in out if "error" in r)
    print(f"{args.tag or args.mode}: {len(out)} games, {errs} errors, "
          f"{time.time() - t0:.0f}s -> {args.out}")


if __name__ == "__main__":
    main()
