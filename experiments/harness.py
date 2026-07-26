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


def round_robin(engine, bot_factories, num_players, games, seed0, log_path):
    """Each game: sample bots (with rotation for seat fairness). Appends JSONL."""
    results = []
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
                rec = {"game": g, "bots": idxs, "error": repr(e)}
                f.write(json.dumps(rec) + "\n"); f.flush()
                results.append(rec)
                continue
            rec = {
                "game": g, "bots": idxs, "scores": scores, "moves": nmoves,
                "winner": idxs[max(range(num_players), key=lambda i: scores[i])],
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
