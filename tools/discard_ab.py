"""Is CHOOSING the military discard worth anything? (docs/COMBAT_AUDIT.md)

RULES_SPEC §6.6 step 1 used to be `hand_military.pop(0)` -- FIFO, no decision,
a rules violation: the rulebook makes this the player's choice and says it is
the only decision in the end-of-turn sequence.  It is now a real choice, and
every clone-and-evaluate bot answers it with the evaluator it already uses.

This A/Bs the POLICY, not the plumbing.  Both arms run the SAME (fixed) engine
and the same weight vector; arm B answers every `discard_military` choice the
way the old engine did -- pitch the oldest card in hand -- so the only
difference between the arms is the answer to the new question.  Doing it this
way keeps it a single-process, seat-paired, head-to-head duel instead of two
runs of two builds, exactly as docs/CARD_BLINDNESS.md §4 argues for.

Every deal is played `players` times with the FIFO arm in each seat in turn, so
the unit of error is the DEAL, not the game (experiments/paired_stats.py).
Behavioural counters come out alongside the win rate: how often the two
policies actually disagree, what that does to the defence value in hand, and
how the games' aggressions resolved for each side.

    nice -n 19 python3 -m tools.discard_ab --spec analysis/frozen/champion_2p.json \
        --deals 200 --players 2
"""
from __future__ import annotations

import argparse
import json
import os
import random
import re
import sys

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

from engine import actions, game                              # noqa: E402
from engine.interact import defense_points                    # noqa: E402
from experiments.arena import load_spec, make_bot             # noqa: E402

try:
    from experiments import paired_stats as PS                # noqa: E402
except ImportError:                                           # pragma: no cover
    PS = None

_AGG = re.compile(r"aggression .* vs P(\d+) (failed|succeeded)")


def blank_counts():
    return {"fires": 0, "differs": 0, "def_chosen": 0, "def_fifo": 0,
            "kept_better": 0, "pitched_better": 0, "auto": 0}


class DiscardWatch:
    """Wrap a policy; optionally force the pre-fix FIFO answer, always count.

    `fifo=True` reproduces the old engine exactly: `pop(0)` discarded
    `hand_military[0]`, so the FIFO answer is the option equal to the oldest
    card in hand.
    """

    def __init__(self, inner, idx, fifo, counts):
        self.inner = inner
        self.idx = idx
        self.fifo = fifo
        self.counts = counts

    def _pending(self, state):
        if not state.pending:
            return None
        pend = state.pending[-1]
        if (pend.get("kind") == "choice"
                and pend.get("tag") == "discard_military"
                and pend.get("player") == self.idx):
            return pend
        return None

    def __call__(self, state):
        pend = self._pending(state)
        if pend is None:
            return self.inner(state)
        opts = pend["options"]
        hand = state.players[self.idx].hand_military
        oldest = hand[0] if hand else None
        fifo_i = opts.index(oldest) if oldest in opts else 0
        if self.fifo:
            mv = ("choose", fifo_i)
        else:
            mv = self.inner(state)
        c = self.counts
        c["fires"] += 1
        i = mv[1] if mv and mv[0] == "choose" else fifo_i
        c["def_chosen"] += defense_points(opts[i])
        c["def_fifo"] += defense_points(opts[fifo_i])
        if i != fifo_i:
            c["differs"] += 1
            if defense_points(opts[fifo_i]) > defense_points(opts[i]):
                c["kept_better"] += 1        # saved a better defence card
            elif defense_points(opts[fifo_i]) < defense_points(opts[i]):
                c["pitched_better"] += 1     # threw a better one away
        return mv


def _defence_log(state, players):
    """(faced, held) aggressions per seat, read off the game log."""
    faced = [0] * players
    held = [0] * players
    for line in state.log:
        m = _AGG.search(line)
        if not m:
            continue
        d = int(m.group(1))
        if d < players:
            faced[d] += 1
            if m.group(2) == "failed":
                held[d] += 1
    return faced, held


def run(spec, deals, seed0, players):
    """FIFO arm in each seat in turn; returns per-GAME lists in deal order."""
    win, marg, own = [], [], []
    cf, ce = blank_counts(), blank_counts()      # fifo arm, evaluator arm
    dfn = {"fifo_faced": 0, "fifo_held": 0, "eval_faced": 0, "eval_held": 0}
    for d in range(deals):
        seed = (seed0 + d) * 7919 + 17
        for seat in range(players):
            bots = []
            for i in range(players):
                inner = make_bot(spec, 1000 + i)
                bots.append(DiscardWatch(inner, i, i == seat,
                                         cf if i == seat else ce))
            st = game.new_game(players, seed)
            game.play_game(bots, num_players=players, seed=seed,
                           move_cap=20000, state=st)
            sc = list(st.final_scores or [p.culture for p in st.players])
            # the EVALUATOR arm is every seat except `seat`; report from its
            # point of view, so >0.5 means choosing beats FIFO.
            others = [sc[i] for i in range(players) if i != seat]
            top = max(sc)
            winners = [i for i in range(players) if sc[i] == top]
            share = sum(1 for i in winners if i != seat) / len(winners)
            win.append(share)
            marg.append(sum(others) / len(others) - sc[seat])
            own.append(sum(others) / len(others))
            faced, held = _defence_log(st, players)
            dfn["fifo_faced"] += faced[seat]
            dfn["fifo_held"] += held[seat]
            dfn["eval_faced"] += sum(faced) - faced[seat]
            dfn["eval_held"] += sum(held) - held[seat]
    return win, marg, own, cf, ce, dfn


def _report(name, per_game, players, pct):
    if PS is None:
        n = len(per_game)
        m = sum(per_game) / n
        var = sum((x - m) ** 2 for x in per_game) / (n - 1)
        half = 1.96 * (var / n) ** 0.5
        print(f"  {name:14s} {m:8.4f} +- {half:.4f}  (NAIVE per-game CI: "
              f"experiments/paired_stats.py absent)")
        return {"mean": m, "half": half, "estimator": "naive"}
    est = PS.paired(per_game, players)
    print(f"  {name:14s} {est.fmt(pct=pct)}   (naive half {est.naive_half:.4f},"
          f" rho {est.rho:+.3f}, n={est.n_games} games / {est.n_clusters}"
          f" deals)")
    return {"mean": est.mean, "half": est.half, "naive_half": est.naive_half,
            "rho": est.rho, "n_deals": est.n_clusters,
            "n_games": est.n_games,
            "estimator": "paired-on-the-deal"}


def main(argv=None):
    ap = argparse.ArgumentParser()
    ap.add_argument("--spec", required=True)
    ap.add_argument("--deals", type=int, default=100)
    ap.add_argument("--players", type=int, default=2)
    ap.add_argument("--seed", type=int, default=5000)
    ap.add_argument("--out")
    a = ap.parse_args(argv)
    spec = load_spec(a.spec)
    win, marg, own, cf, ce, dfn = run(spec, a.deals, a.seed, a.players)
    print(f"spec={a.spec} players={a.players} deals={a.deals} "
          f"n={len(win)} games (FIFO arm in every seat in turn)")
    print("EVALUATOR discard vs FIFO discard (>0.5 / >0 favours choosing):")
    out = {"spec": a.spec, "players": a.players, "deals": a.deals,
           "seed": a.seed}
    out["win"] = _report("win share", win, a.players, True)
    out["margin"] = _report("culture margin", marg, a.players, False)
    out["own"] = _report("own culture", own, a.players, False)
    for label, c in (("evaluator", ce), ("fifo", cf)):
        fires = max(1, c["fires"])
        print(f"  [{label}] decisions faced {c['fires']}, "
              f"differ from FIFO {c['differs']} ({c['differs']/fires:.1%}), "
              f"kept the better defender {c['kept_better']}, "
              f"pitched the better defender {c['pitched_better']}, "
              f"defence discarded {c['def_chosen']} vs {c['def_fifo']} FIFO")
    print(f"  [defence] evaluator arm faced {dfn['eval_faced']} aggressions, "
          f"held {dfn['eval_held']} "
          f"({dfn['eval_held']/max(1,dfn['eval_faced']):.1%}); "
          f"fifo arm faced {dfn['fifo_faced']}, held {dfn['fifo_held']} "
          f"({dfn['fifo_held']/max(1,dfn['fifo_faced']):.1%})")
    out["counts"] = {"evaluator": ce, "fifo": cf, "defence": dfn}
    if a.out:
        with open(a.out, "a") as fh:
            fh.write(json.dumps(out) + "\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
