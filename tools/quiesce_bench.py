"""Cost of QuiescentBot vs the 1-ply WeightedBot, on identical games.

Both bots play the SAME seeds at the same table size, so the comparison is
per-game CPU time on the same work, not a throughput estimate that a busy
machine could distort.  Timing is `time.process_time`, because the hill climbs
saturate the box and wall clock is meaningless there.

    nice -n 15 python3 tools/quiesce_bench.py --players 4 --games 6 \
        --weights experiments/league_state/champion_4p.json

`--weights` defaults to "" (DEFAULT_WEIGHTS) rather than any champion file, so
this tool never silently benchmarks a stale/degenerate vector. It also
refuses (see experiments.arena.refuse_if_degenerate_champion) if pointed at
experiments/champion_4p.json -- the pre-horizon-fix vector
docs/TRAINING_RUN.md says never to warm-start from -- by path or by content,
so a copy of that file is refused too.
"""
from __future__ import annotations

import argparse
import json
import os
import sys
import time

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

from engine import game                                    # noqa: E402
from experiments.arena import (                            # noqa: E402
    load_spec, make_bot, refuse_if_degenerate_champion)


def run(spec, players, games, seed0, move_cap=20000):
    cpu = 0.0
    moves = 0
    stats = {}
    for g in range(games):
        bots = [make_bot(spec, 1000 + i) for i in range(players)]
        t = time.process_time()
        st = game.play_game(bots, num_players=players,
                            seed=(seed0 + g) * 7919 + 17, move_cap=move_cap)
        cpu += time.process_time() - t
        moves += len(st.log)
        for b in bots:
            for k, v in getattr(b, "stats", {}).items():
                stats[k] = stats.get(k, 0) + v
    return {"cpu_s": cpu, "games": games, "moves": moves,
            "s_per_game": cpu / games, "games_per_cpu_s": games / cpu,
            "stats": stats}


def main(argv=None):
    ap = argparse.ArgumentParser()
    ap.add_argument("--players", type=int, default=4, choices=(2, 3, 4))
    ap.add_argument("--games", type=int, default=6)
    ap.add_argument("--seed", type=int, default=500)
    ap.add_argument("--weights", default="")
    ap.add_argument("--extra", default="", help="e.g. ',depth=8,nodes=300'")
    ap.add_argument("--json", default="")
    a = ap.parse_args(argv)

    base = a.weights or "default"
    refuse_if_degenerate_champion(base, "quiesce_bench.py")
    one = load_spec(base)
    qui = load_spec("quiesce:" + base + a.extra)

    r1 = run(one, a.players, a.games, a.seed)
    rq = run(qui, a.players, a.games, a.seed)
    out = {"players": a.players, "weights": base,
           "one_ply": r1, "quiescent": rq,
           "cost_ratio": rq["cpu_s"] / r1["cpu_s"]}
    s = rq["stats"]
    if s.get("candidates"):
        out["quiesce_rate"] = s["quiesced"] / s["candidates"]
        out["nodes_per_candidate"] = s["qnodes"] / s["candidates"]
        out["truncation_rate"] = (s["truncated"] / s["quiesced"]
                                  if s["quiesced"] else 0.0)
    print(json.dumps(out, indent=1))
    if a.json:
        with open(a.json, "w") as fh:
            json.dump(out, fh, indent=1)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
