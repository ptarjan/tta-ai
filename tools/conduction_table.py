"""What can this weight vector actually express?  Print it before you measure.

A reference vector should ship with its conduction table the way a measurement
ships with its error bar.  The failure this exists to make visible is not a
crash and not a wrong number -- it is a clean, well-powered NULL produced by a
coefficient that was multiplied by zero.

`evaluate()` skips whole feature functions when their scale weight is 0.0
(`if hp:`, `if ru or rb:`, ...).  A weight read only from inside
`card_potential` therefore reaches the score through whichever of those
consumers are open and through nothing else -- and for a WONDER, which
`actions.take_card` puts straight into `p.wonder` rather than `hand_civil`,
only through `row_pressure`.  `docs/CARD_BLINDNESS.md` Sec 5.3 spent 12,800
games discovering that the hard way against a vector whose `row_urgency` was
0.0; the line this tool prints would have said so in one second.

There is a SECOND gate, downstream of the first and easier to miss.
`row_pressure` skips any card whose `card_potential` is `<= 0` ("the sweep
destroying a card I do not want is not a loss", `weighted.py`).  So a card can
be invisible to a fully-open `row_pressure` purely because it prices at or
below zero -- and that is a *threshold*, not a slope.  At the live 2p
champion's shipped `card_rate_credit = 0.12812` only 4 of 16 wonders price
above zero; at 1.0, 8 do, and those 8 are exactly the 8 that moved +88%.  The
reprice did not make wonders better, it made them VISIBLE.  Both gates are
reported below, because passing the first tells you nothing about the second.

    python3 tools/conduction_table.py analysis/frozen/champion_2p_gen54_99key.json
    python3 tools/conduction_table.py --md experiments/league_state/champion_3p.json
"""
from __future__ import annotations

import argparse
import json
import os
import sys

_ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..")
sys.path.insert(0, _ROOT)

from engine import cards as C                       # noqa: E402
from engine.bots.weighted import (                  # noqa: E402
    card_potential, load_weights)
from experiments import arena                       # noqa: E402


def visibility(w):
    """(cards, wonders) that clear `row_pressure`'s `card_potential > 0` gate."""
    db = C.db()
    cards = [c["name"] for c in db.cards]
    wonders = [c["name"] for c in db.cards if c["type"] == "wonder"]
    vis = [n for n in cards if card_potential(n, w) > 0.0]
    visw = [n for n in wonders if card_potential(n, w) > 0.0]
    return cards, wonders, vis, visw


def report(path, md=False):
    with open(path) as fh:
        raw = json.load(fh)
    w = load_weights(path)
    meta = {k: raw[k] for k in ("gen", "players", "sigma") if k in raw}
    stored = raw.get("weights", raw)

    out = []
    add = out.append
    add(f"# conduction table -- {os.path.basename(path)}")
    add(f"#   {', '.join(f'{k}={v}' for k, v in meta.items())}, "
        f"{len(stored)} keys stored, {len(w)} after DEFAULT_WEIGHTS fill")
    add("")
    add("## Gate 1: which consumers of `card_potential` are open")
    add("")
    if md:
        add("| consumer | gating weights | value | state |")
        add("|---|---|---|---|")
    for fn in arena.CARD_POTENTIAL_CONSUMERS:
        gates = arena.EVALUATE_GATES[fn]
        vals = ", ".join(f"{g}={w.get(g, 'ABSENT')}" for g in gates)
        state = "OPEN" if any(w.get(g) for g in gates) else "closed"
        if md:
            add(f"| `{fn}` | {', '.join(f'`{g}`' for g in gates)} | {vals} | "
                f"**{state}** |")
        else:
            add(f"   {fn:22s} {state:7s}  ({vals})")
    add("")
    for label, consumers in (
            ("for ANY card", arena.CARD_POTENTIAL_CONSUMERS),
            ("for a WONDER specifically", arena.WONDER_CARD_POTENTIAL_CONSUMERS)):
        open_, _ = arena.lever_conduction(w, consumers)
        add(f"   {label:28s}: {', '.join(open_) if open_ else 'NOTHING'}")
    add("")

    cards, wonders, vis, visw = visibility(w)
    add("## Gate 2: `row_pressure` skips any card with `card_potential <= 0`")
    add("")
    add(f"   visible to row_pressure: {len(vis)}/{len(cards)} cards, "
        f"{len(visw)}/{len(wonders)} wonders")
    add(f"   card_rate_credit = {w.get('card_rate_credit')}   "
        f"row_urgency = {w.get('row_urgency')}   "
        f"row_bargain_forgone = {w.get('row_bargain_forgone')}")
    if visw:
        add(f"   visible wonders: {', '.join(sorted(visw))}")
    add("")
    ru_open = bool(w.get("row_urgency") or w.get("row_bargain_forgone"))
    if not ru_open:
        add("   >> BOTH GATES MOOT: row_pressure is never called. Any A/B whose")
        add("   >> lever is a card_potential multiplier returns an identity, not")
        add("   >> a result. Do not measure wonder pricing against this vector.")
    elif not visw:
        add("   >> row_pressure runs but NO wonder clears the value gate, so a")
        add("   >> wonder's identity still reaches the policy through nothing.")
    return "\n".join(out)


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("paths", nargs="+")
    ap.add_argument("--md", action="store_true", help="markdown tables")
    a = ap.parse_args(argv)
    for i, p in enumerate(a.paths):
        if i:
            print()
        print(report(p, a.md))
    return 0


if __name__ == "__main__":
    sys.exit(main())
