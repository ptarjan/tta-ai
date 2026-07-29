"""Catch programmer-error exceptions that a `except Exception` swallowed.

Why this exists
---------------
`engine/` has ~56 `except Exception:` blocks.  Nearly all are deliberate: a
search speculatively `apply`s a move that turns out to be illegal, and the
right answer is to skip that candidate, not to crash a 40-hour league run.
That defensiveness has a cost, and it was paid in full once:

    dctx = rival_context(st, d, ctx.get("root_row"))   # `ctx` not in scope
    except Exception:
        dctx = dict(_NO_CTX)

`ctx` was a free variable that did not exist in `_quiesce`, so every call
raised `NameError`, the bare `except` turned it into "no rival context", and
PlanBot silently played every quiesced opponent decision blind.  550 unit
tests passed.  Only a 4p fingerprint digest noticed, and only because the
resulting play was different enough to move the final scores.

The lesson is not "write fewer excepts".  It is that a *swallowed*
`NameError` / `AttributeError` / `ImportError` is never a legitimate
game-state failure -- it is always a bug in the code -- and nothing in this
repo could see one.  This module makes them visible without touching a single
one of those 56 sites: `sys.monitoring`'s RAISE event fires when an exception
is raised, before anyone gets a chance to catch it.

Two layers guard this bug class now, cheapest first:

  1. `ruff check` (see ruff.toml, wired into tools/gate.sh) statically finds
     F821 undefined-name.  It flags the line above in ~200ms with no game
     played at all, and would have caught this before the first reproducer
     run.  Static analysis cannot see a name that exists but is None, or an
     `AttributeError` on a state field renamed in one place -- hence layer 2.
  2. This audit, which plays real games and reports what actually raised.

Usage
-----
    python3 -m tools.bug_audit                  # default: 1 game per bot
    python3 -m tools.bug_audit --games 3        # more games per bot
    python3 -m tools.bug_audit --all            # every exception type, not
                                                # just the programmer errors

Exit status is 1 if anything in the strict set raised inside this repo, so it
can be a gate arm.  `tests/test_no_swallowed_bugs.py` is the cheap version of
the same check, and carries the negative control.
"""
import argparse
import os
import sys

# Exception types that are ALWAYS a bug in our code, never a legitimate "that
# move was illegal, try the next one".
#
# The membership here is MEASURED, not guessed.  `--all` over 36 games (2p/3p/4p
# x seeds 0,1,2 x greedy/weighted/quiescent/plan) swallows 44k exceptions per 4p
# batch and every single one of them is `KeyError` or `AttributeError`.  Nothing
# else raises at all, so everything else can sit in the strict set for free:
#
#   NameError            a typo, or a variable used outside its scope.  THE bug
#                        this file exists for.  UnboundLocalError is a subclass,
#                        so it comes along.
#   TypeError            wrong arity or wrong type -- e.g. a monkeypatched or
#                        overridden method whose signature drifted from the real
#                        one, which is precisely how the `_replay` fix broke a
#                        test in tests/test_journal_search_bots.py.
#   ImportError          a module or symbol that moved.
#   IndexError           off-the-end indexing; the codebase guards lengths.
#   ZeroDivisionError    a rate/average with an unguarded denominator.
#   ValueError           bad literal / bad argument.  Zero across all 36 games.
#
# DELIBERATELY OUT, because both are load-bearing control flow here and
# requiring zero would be a gate that cries wolf (docs/PYPY.md 9.0 on how much
# damage a misleading gate does on this project):
#
#   AttributeError  23,305 per 4p batch, almost all `effects.state_stats`
#                   lazily initialising `_stats_cache` via a caught
#                   AttributeError, plus the dict-or-object card accessors'
#                   `getattr(c, "name", c)`.
#   KeyError        ~15k per 4p batch, `actions.cost_of` probing a card name.
#
# Those two counts are a PERFORMANCE smell worth its own look -- 38k exceptions
# per four games is not free -- but they are not correctness bugs and they are
# not this file's business.
#
# If a member of the strict set ever does fire legitimately, the report names the
# file, function and message, so the choice is an informed one: fix it, or move
# the type out of here WITH the reason written down.  Do not silence it blind.
BUG_TYPES = (NameError, TypeError, ImportError, IndexError,
             ZeroDivisionError, ValueError)

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))


class RaiseAudit:
    """Record every exception raised inside REPO, caught or not.

    Not reentrant and not thread-safe: it claims a single `sys.monitoring`
    tool id for the duration of the `with` block.
    """

    def __init__(self, types=BUG_TYPES, tool_id=None):
        self.types = types
        self.hits = []          # (typename, message, file, function, lineno)
        self._mon = sys.monitoring
        self._tid = (self._mon.PROFILER_ID if tool_id is None else tool_id)

    def _on_raise(self, code, offset, exc):
        if not isinstance(exc, self.types):
            return
        path = code.co_filename
        # Only our own code.  A NameError raised and handled inside the
        # standard library or a dependency is their business, not ours.
        if not path.startswith(REPO):
            return
        self.hits.append((type(exc).__name__, str(exc),
                          os.path.relpath(path, REPO), code.co_name,
                          code.co_firstlineno))

    def __enter__(self):
        mon, tid = self._mon, self._tid
        mon.use_tool_id(tid, "tta-bug-audit")
        mon.register_callback(tid, mon.events.RAISE, self._on_raise)
        mon.set_events(tid, mon.events.RAISE)
        return self

    def __exit__(self, *exc):
        mon, tid = self._mon, self._tid
        mon.set_events(tid, 0)
        mon.register_callback(tid, mon.events.RAISE, None)
        mon.free_tool_id(tid)
        return False

    def summary(self):
        """Distinct (type, file, function, message) -> count."""
        out = {}
        for name, msg, path, func, line in self.hits:
            out[(name, path, func, line, msg)] = \
                out.get((name, path, func, line, msg), 0) + 1
        return out


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--games", type=int, default=1,
                    help="games per bot (default 1)")
    ap.add_argument("--players", type=int, default=4,
                    help="seat count (default 4 -- the widest search)")
    ap.add_argument("--bots", default="greedy,weighted,quiescent,plan",
                    help="comma-separated bot names to exercise")
    ap.add_argument("--all", action="store_true",
                    help="report every exception type, not just BUG_TYPES")
    args = ap.parse_args(argv)

    from engine import perf_check

    types = (Exception,) if args.all else BUG_TYPES
    audit = RaiseAudit(types)
    with audit:
        for bot in args.bots.split(","):
            for seed in range(args.games):
                perf_check._play(args.players, bot.strip(), seed)

    rows = sorted(audit.summary().items(), key=lambda kv: -kv[1])
    if not rows:
        print("bug audit: clean -- no %s raised inside the repo"
              % "/".join(t.__name__ for t in types))
        return 0
    print("bug audit: %d raise(s), %d distinct site(s)"
          % (len(audit.hits), len(rows)))
    for (name, path, func, line, msg), n in rows:
        print("  %6d  %-18s %s:%d in %s()\n          %s"
              % (n, name, path, line, func, msg))
    return 0 if args.all else 1


if __name__ == "__main__":
    sys.exit(main())
