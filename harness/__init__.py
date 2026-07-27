"""Human-in-the-loop harness for playing our bot against the CGE app AI.

The one path to an *external* measurement of bot strength: everything else we
measure is our own bots playing our own bots.

Three pieces, deliberately separable:

* `harness.fields`  -- derives, from the live evaluator, which parts of the
  board a human must actually transcribe.  Not a hardcoded list: it perturbs
  the mirror and watches whether the bot's move changes.
* `harness.mirror`  -- checksums the mirror against what the app shows, so a
  drifting mirror fails loudly instead of silently invalidating the game.
* `harness.record`  -- the machine-readable per-game record (JSONL).

`harness.play` wires them to `advisor.Advisor` and a terminal.

See `docs/APP_HARNESS.md` for the operator's manual and `docs/EXTERNAL_AIS.md`
sections 1 and 6 for why this exists at all.
"""
