"""Hot-path hook for the war-rate decision census (docs/WAR_RATE_CENSUS.md).

WHY THIS EXISTS, and why it is a shim rather than the instrument itself.

The first version of the census worked by replacing `PlanBot.pick` /
`QuiescentBot.pick` at runtime with a *byte-for-byte copy* of the original
plus one recording call.  That copy is the fragile part: it duplicates the
real search's control flow, so every future edit to `pick()` silently
un-syncs the instrument from the thing it claims to measure, and nothing
fails when it does.  That is this project's recurring bug class -- a
coordinate present in one registry and absent from another, with nothing
that fails when they disagree.

So the recording call now lives inside the real `pick()`, once, and there is
no copy.  This module is the seam.  It is deliberately tiny: `engine` must
not depend on `tools`, so the actual recorder (which is an *instrument*, not
engine logic, and pulls in the whole card/actions surface) is imported
lazily and ONLY when the census is switched on.  With `TTA_WAR_CENSUS`
unset -- the default, and what every ordinary game runs -- `record()` costs
one attribute lookup and returns, and `tools.war_census` is never imported.

THE ONE INVARIANT.  A census must never be able to change or break a game.
`record()` is called AFTER the decision is made and its return value is
discarded, so it cannot alter the chosen move; and it swallows every
exception, because a broken instrument must degrade to "no data", never to a
crashed league arm.  The broad `except` here is intentional and is not a
lint miss.

Switch it on by setting `TTA_WAR_CENSUS` to a directory; each process
appends to its own `census-<pid>.jsonl` inside it, so the league's parallel
arms cannot interleave into one another's records.
"""

import os

#: Read once at import.  A per-call `os.environ` lookup in the hot path of
#: every decision of every arm is exactly the kind of cost that is invisible
#: in a microbenchmark and real over a league run.
ENABLED = bool(os.environ.get("TTA_WAR_CENSUS"))

_impl = None      # the recorder, once resolved
_loaded = False   # "we have already tried"; a failed load must not retry


def _load():
    """Resolve the recorder on first use.  Never raises."""
    global _impl, _loaded
    _loaded = True
    dest = os.environ.get("TTA_WAR_CENSUS")
    if not dest:
        return
    try:
        from tools import war_census as _wc
        _wc.open_sink_for_process(dest)
        _impl = _wc.record_decision
    except Exception:
        _impl = None


def record(state, idx, w, moves, scored, chosen):
    """Record one decision.  No-op unless the census is switched on.

    `scored` is ``[(move, value), ...]`` -- note the order, which is the
    opposite of the ``(value, move)`` tuples `PlanBot.pick` sorts on.
    """
    if not ENABLED:
        return
    if not _loaded:
        _load()
    if _impl is None:
        return
    try:
        _impl(state, idx, w, moves, scored, chosen)
    except Exception:
        pass          # see THE ONE INVARIANT above
