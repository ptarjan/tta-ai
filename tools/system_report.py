"""Turn `tools/system_census.py` blobs into the per-system tables.

    python3 tools/system_report.py /tmp/sys_2p.json /tmp/sys_3p.json /tmp/sys_4p.json

Everything is printed **per player per game** (`/seat`) and **per game**
(`/game`), because the human corpus in `docs/HUMAN_BASELINE.md` is quoted both
ways and mixing them is the single easiest way to get a factor of N wrong.
"""
from __future__ import annotations

import json
import sys
from collections import Counter

sys.path.insert(0, "/tmp/behavcensus")
from engine import cards                                       # noqa: E402

_DB = cards.db()
WONDERS = [c["name"] for c in _DB.cards if c["type"] == "wonder"]
LEADERS = [c["name"] for c in _DB.cards if c["type"] == "leader"]
GOVS = [c["name"] for c in _DB.cards if c["type"] == "government"]
TERR = [c["name"] for c in _DB.cards if c["type"] == "territory"]
WARS = [c["name"] for c in _DB.cards if c["type"] == "war"]


def load(paths):
    out = []
    for p in paths:
        b = json.load(open(p))
        b["t"] = Counter(b["totals"])
        b["n"] = {k: Counter(v) for k, v in b["names"].items()}
        out.append(b)
    return out


def main(argv):
    blobs = load(argv[1:])
    rows = []

    def line(label, key, scale="seat", fn=None):
        vals = []
        for b in blobs:
            v = fn(b) if fn else b["t"][key]
            d = b["t"]["seats"] if scale == "seat" else b["t"]["games"]
            vals.append(v / d if d else 0.0)
        rows.append((label + " /" + scale, vals))

    for b in blobs:
        print("== %dp  spec=%s  games=%d seats=%d rounds/game=%.1f "
              "score/seat=%.1f final_age=%s"
              % (b["players"], b["spec"], b["t"]["games"], b["t"]["seats"],
                 b["t"]["rounds"] / b["t"]["games"],
                 b["t"]["score"] / b["t"]["seats"],
                 {k[10:]: v for k, v in b["t"].items()
                  if k.startswith("final_age:")}))

    line("wonders started", "wonders_started")
    line("wonders completed", "wonders_completed")
    line("wonder stages", "wonder_steps")
    line("wonder unfinished at end", "wonder_unfinished_at_end")
    line("gov changes (rev+dev)", "", fn=lambda b: sum(b["n"].get("gov_change", {}).values()))
    line("  revolutions", "mv:revolution")
    line("wars declared", "mv:war")
    line("wars resolved", "war_resolved", "game")
    line("  attacker won", "war_att_won", "game")
    line("  attacker lost", "war_att_lost", "game")
    line("aggressions played", "mv:aggression")
    line("aggressions resolved", "aggr_resolved", "game")
    line("  succeeded", "aggr_succeeded", "game")
    line("  held off", "aggr_held_off", "game")
    line("defences faced", "defense_started", "game")
    line("defence cards spent", "defend_card_spent", "game")
    line("colony auctions", "auction_started", "game")
    line("colony bids", "mv:bid")
    line("colonies held at end", "colonies_held")
    line("pacts offered", "mv:offer_pact")
    line("pacts held at end", "pacts_held")
    line("pacts cancelled", "mv:cancel_pact")
    line("events prepared", "mv:prepare_event")
    line("  event cards", "prep:event")
    line("  territory cards", "prep:territory")
    line("events revealed", "revealed:event", "game")
    line("  age A revealed", "revealed_age:A", "game")
    line("  age I revealed", "revealed_age:I", "game")
    line("  age II revealed", "revealed_age:II", "game")
    line("  age III revealed", "revealed_age:III", "game")
    line("leaders played", "mv:play_leader")
    line("civil cards taken", "mv:take")
    for col in ("yellow", "blue", "red", "green"):
        line("tech taken: " + col, "tech:" + col)
    for age in ("A", "I", "II", "III", "IV"):
        line("  take age " + age, "take_age:" + age)
    line("military discard decisions", "pend:discard_military")
    line("units disbanded", "disband_unit")
    line("tactics played", "mv:play_tactic")
    line("tactics copied", "mv:copy_tactic")

    w = max(len(r[0]) for r in rows)
    print("\n%-*s %s" % (w, "metric",
                         " ".join("%8s" % ("%dp" % b["players"])
                                  for b in blobs)))
    for label, vals in rows:
        print("%-*s %s" % (w, label,
                           " ".join("%8.3f" % v for v in vals)))

    for bucket, pool in (("wonder_completed", WONDERS),
                         ("wonder_taken", WONDERS),
                         ("leader_played", LEADERS),
                         ("gov_change", GOVS),
                         ("colony_held", TERR),
                         ("war_declared", WARS),
                         ("auction", TERR)):
        print("\n-- %s" % bucket)
        for b in blobs:
            c = b["n"].get(bucket, {})
            never = [x for x in pool if not c.get(x)]
            print("  %dp n=%d distinct=%d/%d  NEVER: %s"
                  % (b["players"], sum(c.values()), len(pool) - len(never),
                     len(pool), ", ".join(never) or "(none)"))
            top = sorted(c.items(), key=lambda kv: -kv[1])[:6]
            print("      top: %s" % ", ".join("%s %d" % t for t in top))
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
