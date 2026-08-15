#!/usr/bin/env python3
"""Step 6: did the paranoid suite actually EXECUTE every mutation site?

docs/PYPY.md 9.5 names this as the residual risk section 6.5 did not:
`JOURNAL_PARANOID=1` proves the sites the games *reach*, and says nothing at
all about the ones they do not.  A site that never runs is not verified; it is
merely unobserved, and it will fire the first time a rare card comes up in a
hill-climb run months from now.

So the audit is: take the full site census from `tools/find_mutations.py`, run
real journalled games, record which of those exact lines executed, and print
the ones that did not.  Two lists come out and they mean opposite things:

  * an unexecuted CONVERTED site -- `journal.touch(...)` is there and neither
    a game nor a test ever ran it, so the conversion is untested.  Read it,
    then write a test that drives the path (which is what
    `RareSitesRollBackExactly` in tests/test_journal.py is).
  * an unexecuted UNCONVERTED site -- claimed to be a local in the 5a-5f
    commit messages, and that claim was never exercised either.  Read it.

Executed-and-unconverted is the one combination that needs no follow-up: the
paranoid differ ran that line under a `copy_state` oracle and found no
difference, which is a proof rather than an argument.

Implementation note: `coverage.py` is not installable on this box (PEP 668),
and it would be the wrong tool anyway -- we do not want a percentage over the
whole engine, we want a verdict on 166 specific lines.  `sys.monitoring`
(3.12+) gives exactly that, and returning DISABLE from the callback retires
each location after its first hit, so the run costs little more than an
uninstrumented one.

    python3 tools/mutation_coverage.py [--games N] [--players 2,3,4]
                                       [--bot greedy|weighted]

`--bot` is not cosmetic.  Coverage is a claim about the sites the *search*
drives, and GreedyBot and WeightedBot pick different moves; a site covered
under one may be unreached under the other.  9.14 runs both.

Exit status is 1 if any CONVERTED site went unexecuted -- that is the
condition step 6 gates on.
"""
from __future__ import annotations

import os
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, ROOT)
sys.path.insert(0, os.path.join(ROOT, "tools"))

import find_mutations                                    # noqa: E402


def collect_sites():
    """{abs_path: {lineno: (kind, root, converted)}} for the 8 engine files."""
    out = {}
    for rel in find_mutations.DEFAULT_FILES:
        path = os.path.join(ROOT, rel)
        sites, _lines = find_mutations.scan(rel)
        d = out.setdefault(path, {})
        for line, kind, root, conv in sites:
            # one line can host two sites (`a[i] = b.pop()`); a line is
            # "converted" only if EVERY site on it is.
            if line in d:
                k, r, c = d[line]
                d[line] = (k + "+" + kind, r, c and conv)
            else:
                d[line] = (kind, root, conv)
    return out


def run(games, players, run_tests=True, bot="greedy"):
    """Play journalled games with LINE monitoring on, return hit lines.

    `bot` is the *searching* bot.  It matters: coverage is a claim about which
    mutation sites the search actually drives, and GreedyBot and WeightedBot
    pick different moves, so they reach different sites.  docs/PYPY.md 9.14.
    """
    os.environ["TTA_JOURNAL"] = "1"
    from engine import game
    from engine.bots import make_bots
    import engine.bots as bots_mod
    assert bots_mod.USE_JOURNAL, "TTA_JOURNAL must be set before importing bots"

    sites = collect_sites()
    watched = {p: set(d) for p, d in sites.items()}
    hit = {p: set() for p in sites}

    mon = sys.monitoring
    TOOL = mon.COVERAGE_ID
    mon.use_tool_id(TOOL, "mutation_coverage")

    def on_line(code, lineno):
        f = code.co_filename
        w = watched.get(f)
        if w is not None and lineno in w:
            hit[f].add(lineno)
        return mon.DISABLE          # retire this location; keeps the run fast

    mon.register_callback(TOOL, mon.events.LINE, on_line)
    mon.set_events(TOOL, mon.events.LINE)
    try:
        for n in players:
            for seed in range(games):
                game.play_game(make_bots(bot, n, seed=seed), n, seed=seed)
        if run_tests:
            # A site the games never reach can still be verified -- by a test
            # that drives it directly against a copy_state oracle (see
            # tests/test_journal.py RareSitesRollBackExactly).  Those count,
            # so they have to be traced too, or the audit reports sites as
            # unverified that are in fact the best-verified in the file.
            import importlib
            import unittest
            sys.path.insert(0, os.path.join(ROOT, "tests"))
            ldr, suite = unittest.TestLoader(), unittest.TestSuite()
            for fn in sorted(os.listdir(os.path.join(ROOT, "tests"))):
                if fn.startswith("test_") and fn.endswith(".py"):
                    suite.addTests(ldr.loadTestsFromModule(
                        importlib.import_module(fn[:-3])))
            with open(os.devnull, "w") as devnull:
                unittest.TextTestRunner(verbosity=0, stream=devnull).run(suite)
    finally:
        mon.set_events(TOOL, 0)
        mon.free_tool_id(TOOL)
    return sites, hit


def main(argv):
    games = 3
    players = (2, 3, 4)
    if "--games" in argv:
        games = int(argv[argv.index("--games") + 1])
    if "--players" in argv:
        players = tuple(int(x) for x in argv[argv.index("--players") + 1].split(","))
    bot = argv[argv.index("--bot") + 1] if "--bot" in argv else "greedy"

    sites, hit = run(games, players, run_tests="--no-tests" not in argv, bot=bot)

    miss_conv, miss_unconv, n_conv, n_unconv = [], [], 0, 0
    for path in sorted(sites):
        rel = os.path.relpath(path, ROOT)
        src = open(path, encoding="utf-8").read().splitlines()
        for line in sorted(sites[path]):
            kind, root, conv = sites[path][line]
            n_conv += conv
            n_unconv += not conv
            if line in hit[path]:
                continue
            rec = f"{rel}:{line:<5} {kind:<14} {root:<12} | {src[line-1].strip()[:64]}"
            (miss_conv if conv else miss_unconv).append(rec)

    print(f"games: {games} per player count {players}, searching bot: {bot}")
    print(f"sites: {n_conv} converted, {n_unconv} unconverted (local)\n")

    print(f"CONVERTED sites never executed ({len(miss_conv)}/{n_conv}) "
          "-- untested conversions, audit by hand:")
    for r in miss_conv or ["    (none)"]:
        print("    " + r if r.strip() != "(none)" else r)

    print(f"\nUNCONVERTED sites never executed ({len(miss_unconv)}/{n_unconv}) "
          "-- 'it is a local' never exercised, audit by hand:")
    for r in miss_unconv or ["    (none)"]:
        print("    " + r if r.strip() != "(none)" else r)

    return 1 if miss_conv else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
