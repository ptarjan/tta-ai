"""No `except Exception:` in engine/ may swallow a programmer error.

The bug this guards (docs/INFORMATION_AUDIT.md 6.3):

    dctx = rival_context(st, d, ctx.get("root_row"))   # no `ctx` in this scope
    except Exception:
        dctx = dict(_NO_CTX)

`NameError` on every call, swallowed, so PlanBot played every quiesced
opponent decision with no rival context at all.  550 unit tests passed.  The
only thing that caught it was a 4p fingerprint digest, and only because the
damage happened to change the final scores -- a quieter version of the same
mistake would have shipped.

`tools/bug_audit.py` watches `sys.monitoring`'s RAISE event, which fires
before any `except` gets a chance, so it sees swallowed exceptions without
touching the ~56 deliberate `except Exception:` blocks that make an
unattended 40-hour league run survivable.

This is the cheap arm: 2p, two bots, one seed, a few seconds.  `python3 -m
tools.bug_audit --games 3 --players 4` is the thorough one and runs in
tools/gate.sh.  `ruff check` (ruff.toml) is cheaper still and catches the
undefined-name case statically, without playing a game at all -- three layers,
because none of them subsumes the others.
"""
import unittest

from engine import perf_check
from tools.bug_audit import BUG_TYPES, RaiseAudit


class NoSwallowedProgrammerErrors(unittest.TestCase):

    def test_a_real_game_raises_no_programmer_errors(self):
        """Play real games and require the strict set to stay at zero.

        `weighted` and `plan` rather than `greedy`: the bug lived in PlanBot's
        `_quiesce`, and a bot whose search never enters the code under test
        cannot fail this (docs/PYPY.md 9.14, the same coverage argument that
        put the weighted/quiescent/plan arms in the gate).
        """
        audit = RaiseAudit()
        with audit:
            for bot in ("weighted", "plan"):
                perf_check._play(2, bot, 0)
        if audit.hits:
            lines = ["%s in %s:%d %s() -- %s (x%d)"
                     % (name, path, line, func, msg, n)
                     for (name, path, func, line, msg), n
                     in sorted(audit.summary().items(), key=lambda kv: -kv[1])]
            self.fail("a programmer error was raised (and probably swallowed "
                      "by an `except Exception`) inside engine/:\n  "
                      + "\n  ".join(lines))

    def test_the_audit_can_fail(self):
        """Negative control: the instrument must see a swallowed NameError.

        Without this, the test above passes just as happily when
        `sys.monitoring` silently stops delivering RAISE events -- which is
        exactly the failure mode that makes a green check worse than no check.
        """
        def swallows_a_typo():
            try:
                return undefined_name_on_purpose.get("x")   # noqa: F821
            except Exception:
                return "swallowed"

        audit = RaiseAudit()
        with audit:
            self.assertEqual(swallows_a_typo(), "swallowed")
        names = [h[0] for h in audit.hits]
        self.assertIn("NameError", names,
                      "the raise audit saw nothing, so a green "
                      "test_a_real_game_raises_no_programmer_errors proves "
                      "nothing")

    def test_the_strict_set_excludes_the_control_flow_types(self):
        """AttributeError/KeyError are load-bearing control flow here.

        `effects.state_stats` initialises its cache off a caught
        AttributeError (23k per 4p batch) and `actions.cost_of` probes names
        with a caught KeyError (~15k).  If either lands in BUG_TYPES the audit
        fails on every clean tree, which is the "gate that cries wolf" failure
        this repo has been bitten by before.  Pin it so a well-meaning widening
        of the set has to come here and read why.
        """
        for t in (AttributeError, KeyError):
            self.assertNotIn(t, BUG_TYPES)
        for t in (NameError, TypeError):
            self.assertIn(t, BUG_TYPES)


if __name__ == "__main__":
    unittest.main()
