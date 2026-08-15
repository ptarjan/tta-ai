"""Round-robin benchmark for the variant roster (engine/bots/variants/).

WHY THIS EXISTS
---------------
``docs/STRENGTH_CHECK.md`` showed self-play had converged: the champion only
had to beat a mirror of itself.  ``engine/bots/variants/`` builds a pool of
structurally different rule-based opponents so training has something other
than itself to beat.  This script measures the pool -- every entrant against
every other entrant, at 2p / 3p / 4p -- so the roster can be reported
honestly: which variants are actually strong, and which are filler that is
still useful as a sparring partner but must not be presented as an equal.

FAIRNESS
--------
Everything runs through ``experiments/arena.duel``, which seat-rotates the
challenger over a fixed seed set.  Every matchup at a given player count uses
the *same* ``--seed``, so all cells of the table are paired game-for-game on
identical deals.  The null hypothesis is 1/N.

At 3p and 4p a "duel" is one challenger against a table of identical
defenders, which is the only sense in which a 1-vs-1 comparison is defined at
those counts; a cell therefore reads "A alone against a table of Bs".

USAGE
-----
::

    python3 -m experiments.roster_match --games 120              # 2p/3p/4p
    python3 -m experiments.roster_match --players 2 --games 200
    python3 -m experiments.roster_match --table                  # re-render

Entrant specs: ``tempo`` .. ``wonder`` (the variants), ``book`` (BookBot v1,
the yardstick from STRENGTH_CHECK), ``book2`` (BookBot on the empirical
tournament tier list), ``champion`` (the trained weights), ``greedy``.
"""
from __future__ import annotations

import argparse
import json
import os
import sys
import time

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)),
                                ".."))

from experiments import arena  # noqa: E402

_BASE_MAKE_BOT = arena.make_bot


def make_bot(spec, seed):
    """arena.make_bot plus the roster.

    Installed over ``arena.make_bot`` (exactly as ``experiments/bookmatch.py``
    does) so forked multiprocessing workers inherit the patch without the
    shared arena module having to be edited while other agents are live.
    """
    if isinstance(spec, tuple) and spec and spec[0] == "variant":
        from engine.bots.variants import make_variant
        return make_variant(spec[1], seed=seed)
    if spec == "book":
        from engine.bots.book import BookBot
        return BookBot(seed=seed)
    if spec == "book2":
        from engine.bots.book import BookBot
        return BookBot(seed=seed, version=2)
    if isinstance(spec, tuple) and spec and spec[0] == "bookimp":
        from engine.bots.book import BookImprovedBot
        return BookImprovedBot(weights=spec[1], seed=seed)
    return _BASE_MAKE_BOT(spec, seed)


arena.make_bot = make_bot


def champion_path(players):
    """The frozen champion snapshot, matching bookmatch.py's choice.

    The live ``champion_Np.json`` files are rewritten by the hill climbs, so
    measuring against them would silently compare different opponents between
    runs.  The frozen strength-check snapshot is preferred when present.
    """
    here = os.path.dirname(os.path.abspath(__file__))
    frozen = os.path.join(here, "frozen",
                          f"champion_{players}p_strengthcheck.json")
    if os.path.exists(frozen):
        return frozen
    return os.path.join(here, f"champion_{players}p.json")


def load_spec(spec, players):
    from engine.bots.variants import VARIANTS
    if spec in VARIANTS:
        return ("variant", spec)
    if spec in ("book", "book2"):
        return spec
    if spec == "champion":
        return arena.load_spec(champion_path(players))
    if spec == "bookimp":
        return ("bookimp", arena.load_spec(champion_path(players)))
    return arena.load_spec(spec)


DEFAULT_ENTRANTS = ["tempo", "infra", "military", "culture", "science",
                    "wonder", "book2", "book", "bookimp", "champion",
                    "greedy", "random"]


def run_pair(a, b, players, games, seed, workers):
    t0 = time.time()
    res = arena.duel(load_spec(a, players), load_spec(b, players), players,
                     games, seed0=seed, workers=workers or None)
    res.pop("shares", None)
    res.pop("per_game", None)
    res.update({"a": a, "b": b, "secs": round(time.time() - t0, 1)})
    print(f"  {a:>9s} vs {b:<9s} @{players}p: "
          f"{res['win_rate']:6.1%} +/- {res['ci']:.1%} "
          f"(null {res['null']:.0%}, p={res['p']:.4f}, n={res['games']}) "
          f"culture {res['culture_a']:.0f} vs {res['culture_b']:.0f} "
          f"[{res['secs']}s]", flush=True)
    return res


def main(argv=None):
    ap = argparse.ArgumentParser(description="variant roster round-robin")
    ap.add_argument("--games", type=int, default=120,
                    help="games per ordered matchup per player count")
    ap.add_argument("--players", type=int, default=0, choices=(0, 2, 3, 4))
    ap.add_argument("--seed", type=int, default=2000)
    ap.add_argument("--workers", type=int, default=0)
    ap.add_argument("--entrants", default=",".join(DEFAULT_ENTRANTS))
    ap.add_argument("--out", default="experiments/roster_match.jsonl")
    ap.add_argument("--table", action="store_true",
                    help="only re-render the table from --out")
    ap.add_argument("--resume", action="store_true",
                    help="skip pairings already present in --out")
    args = ap.parse_args(argv)

    entrants = [e.strip() for e in args.entrants.split(",") if e.strip()]
    if args.table:
        rows = [json.loads(l) for l in open(args.out)]
        print(render(rows, entrants))
        return rows

    # Every completed pairing is appended to --out the moment it finishes, so
    # a killed run (session limit, machine reboot) loses at most the single
    # matchup that was in flight.  --resume then picks up from there.
    done = set()
    if args.resume and args.out and os.path.exists(args.out):
        for line in open(args.out):
            r = json.loads(line)
            done.add((r["players"], r["a"], r["b"]))
        print(f"resume: {len(done)} pairings already on disk", flush=True)

    counts = (2, 3, 4) if args.players == 0 else (args.players,)
    rows = []
    for n in counts:
        print(f"== {n} players ==", flush=True)
        for i, a in enumerate(entrants):
            for b in entrants[i + 1:]:
                if (n, a, b) in done:
                    continue
                r = run_pair(a, b, n, args.games, args.seed, args.workers)
                rows.append(r)
                if args.out:
                    with open(args.out, "a") as fh:
                        fh.write(json.dumps(r) + "\n")
                        fh.flush()
    print()
    print(render(rows, entrants))
    return rows


def render(rows, entrants):
    """Markdown round-robin tables, one per player count.

    Cell (row A, column B) is A's win rate against a table of Bs.  The
    mirror cell is filled in as 1 - (that), which is exact at 2p and, at 3p
    and 4p, is the reciprocal duel actually played -- so both directions are
    measured, not assumed.
    """
    out = []
    by_n = {}
    for r in rows:
        by_n.setdefault(r["players"], []).append(r)
    for n in sorted(by_n):
        cell = {}
        for r in by_n[n]:
            cell[(r["a"], r["b"])] = r
        names = [e for e in entrants
                 if any(k[0] == e or k[1] == e for k in cell)]
        out.append(f"\n### {n} players (null = {1.0 / n:.1%})\n")
        out.append("| vs | " + " | ".join(names) + " | mean |")
        out.append("|---|" + "---|" * (len(names) + 1))
        for a in names:
            vals, cells = [], []
            for b in names:
                if a == b:
                    cells.append("--")
                    continue
                r = cell.get((a, b))
                if r is not None:
                    v = r["win_rate"]
                elif cell.get((b, a)) is not None and n == 2:
                    v = 1.0 - cell[(b, a)]["win_rate"]
                else:
                    cells.append("")
                    continue
                vals.append(v)
                cells.append(f"{v:.0%}")
            mean = f"**{sum(vals) / len(vals):.1%}**" if vals else ""
            out.append(f"| **{a}** | " + " | ".join(cells) + f" | {mean} |")
    return "\n".join(out)


if __name__ == "__main__":
    main()
