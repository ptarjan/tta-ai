"""Self-play tournament + hill-climbing harness.

Rules-agnostic: depends only on the engine exposing
    new_game(num_players: int, seed: int) -> state
    legal_moves(state) -> list[move]
    apply(state, move, rng) -> state
    current_player(state) -> int
    is_over(state) -> bool
    scores(state) -> list[float]   # final culture per player
and bots exposing  choose(state, moves, rng) -> move.

Results are appended to JSONL immediately (restart-safe).
"""
from __future__ import annotations

import json
import os
import random
import sys
import time
from dataclasses import dataclass, field

sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))


def play_game(engine, bots, seed):
    """Play one full game; bots is a list matching player count."""
    rng = random.Random(seed)
    state = engine.new_game(len(bots), seed)
    moves_played = 0
    while not engine.is_over(state):
        p = engine.current_player(state)
        moves = engine.legal_moves(state)
        if not moves:
            raise RuntimeError(f"no legal moves for player {p}")
        move = bots[p].choose(state, moves, rng)
        state = engine.apply(state, move, rng)
        moves_played += 1
        if moves_played > 100_000:
            raise RuntimeError("game did not terminate (100k moves)")
    return engine.scores(state), moves_played


def round_robin(engine, bot_factories, num_players, games, seed0, log_path,
                names=None):
    """Each game: sample bots (with rotation for seat fairness). Appends JSONL."""
    results = []
    names = names or [str(i) for i in range(len(bot_factories))]
    with open(log_path, "a") as f:
        for g in range(games):
            rng = random.Random(seed0 + g)
            idxs = [g % len(bot_factories)] + [
                rng.randrange(len(bot_factories)) for _ in range(num_players - 1)
            ]
            rng.shuffle(idxs)
            bots = [bot_factories[i]() for i in idxs]
            t0 = time.time()
            try:
                scores, nmoves = play_game(engine, bots, seed0 + g)
            except Exception as e:  # log engine bugs, keep tournament running
                rec = {"game": g, "players": num_players,
                       "bots": [names[i] for i in idxs], "error": repr(e)}
                f.write(json.dumps(rec) + "\n"); f.flush()
                results.append(rec)
                continue
            best = max(range(num_players), key=lambda i: scores[i])
            rec = {
                "game": g, "players": num_players,
                "bots": [names[i] for i in idxs], "scores": scores,
                "moves": nmoves, "winner": names[idxs[best]],
                "winner_seat": best,
                "secs": round(time.time() - t0, 2),
            }
            f.write(json.dumps(rec) + "\n"); f.flush()
            results.append(rec)
    return results


@dataclass
class HillClimb:
    """(1+lambda) evolution strategy over a weight dict.

    Champion vs mutants: each generation, mutate the champion's weights,
    play head-to-head tables at 2p/3p/4p; a mutant replaces the champion
    if its win rate exceeds `threshold` (guards vs noise).
    State checkpoints to disk each generation (restart-safe).
    """
    engine: object
    make_bot: object            # weights -> bot
    base_weights: dict
    games_per_eval: int = 60
    sigma: float = 0.3
    threshold: float = 0.55
    checkpoint: str = "experiments/champion.json"
    history: list = field(default_factory=list)

    def load(self):
        if os.path.exists(self.checkpoint):
            with open(self.checkpoint) as fh:
                data = json.load(fh)
            self.base_weights = data["weights"]
            self.history = data.get("history", [])
        return self

    def save(self, gen, winrate):
        self.history.append({"gen": gen, "winrate": winrate,
                             "at": time.strftime("%F %T")})
        with open(self.checkpoint, "w") as fh:
            json.dump({"weights": self.base_weights,
                       "history": self.history}, fh, indent=1)

    def mutate(self, rng):
        w = dict(self.base_weights)
        # perturb a random subset; occasional big jumps escape local optima
        keys = list(w)
        for k in rng.sample(keys, max(1, len(keys) // 4)):
            scale = self.sigma * (4 if rng.random() < 0.1 else 1)
            w[k] = w[k] + rng.gauss(0, scale) * (abs(w[k]) + 0.1)
        return w

    def eval_matchup(self, wa, wb, seed0):
        """Win rate of weights wa vs table of wb, across 2/3/4p."""
        wins = tot = 0
        g = 0
        for np_ in (2, 3, 4):
            for _ in range(self.games_per_eval // 3):
                rng = random.Random(seed0 + g); g += 1
                seat = rng.randrange(np_)
                bots = [self.make_bot(wa if i == seat else wb)
                        for i in range(np_)]
                try:
                    scores, _ = play_game(self.engine, bots, seed0 + g * 7919)
                except Exception:
                    continue
                best = max(range(np_), key=lambda i: scores[i])
                wins += (best == seat)
                tot += 1
        return wins / max(tot, 1), tot

    def run(self, generations, seed0=0, log=print):
        rng = random.Random(seed0)
        for gen in range(len(self.history), len(self.history) + generations):
            cand = self.mutate(rng)
            wr, n = self.eval_matchup(cand, self.base_weights,
                                      seed0 + gen * 1_000_003)
            # a lone challenger at a table of champions must beat 1/np baseline
            baseline = 1 / 3  # avg of 1/2,1/3,1/4
            if wr > max(self.threshold * baseline * 3, baseline + 0.08):
                self.base_weights = cand
                log(f"gen {gen}: ACCEPT wr={wr:.2f} (n={n})")
            else:
                log(f"gen {gen}: reject wr={wr:.2f} (n={n})")
            self.save(gen, wr)
        return self.base_weights


# --------------------------------------------------------------- CLI

def summarize(results, names):
    """Win rate and mean score per bot kind."""
    stats = {n: {"games": 0, "wins": 0, "score": 0.0} for n in names}
    errors = 0
    for r in results:
        if "error" in r:
            errors += 1
            continue
        for seat, bot in enumerate(r["bots"]):
            stats[bot]["games"] += 1
            stats[bot]["score"] += r["scores"][seat]
            stats[bot]["wins"] += (seat == r["winner_seat"])
    out = []
    for n, s in stats.items():
        g = max(1, s["games"])
        out.append(f"{n}: {s['games']} seats, win rate {s['wins'] / g:.1%}, "
                   f"mean culture {s['score'] / g:.1f}")
    if errors:
        out.append(f"errors: {errors}")
    return "\n".join(out)


def main(argv=None):
    import argparse

    from engine import bots as bots_mod
    from engine import game as engine

    ap = argparse.ArgumentParser(description="TtA bot round robin")
    ap.add_argument("--games", type=int, default=40)
    ap.add_argument("--players", type=int, default=2, choices=(2, 3, 4))
    ap.add_argument("--seed", type=int, default=0)
    ap.add_argument("--out", default="experiments/results.jsonl")
    ap.add_argument("--bots", default="random,greedy")
    args = ap.parse_args(argv)

    names = [b.strip() for b in args.bots.split(",")]
    kinds = {"random": bots_mod.RandomBot, "greedy": bots_mod.GreedyBot}
    counter = [0]

    def factory(name):
        def make():
            counter[0] += 1
            return kinds[name](seed=args.seed * 7919 + counter[0])
        return make

    os.makedirs(os.path.dirname(os.path.abspath(args.out)), exist_ok=True)
    t0 = time.time()
    results = round_robin(engine, [factory(n) for n in names], args.players,
                          args.games, args.seed, args.out, names=names)
    print(f"{args.games} games, {args.players}p, "
          f"{time.time() - t0:.1f}s -> {args.out}")
    print(summarize(results, names))
    return results


if __name__ == "__main__":
    main()
