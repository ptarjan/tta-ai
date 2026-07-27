"""NeuralBot: the existing 1-ply search machinery with a neural evaluator.

This is the Stage-1 deliverable (docs/NEURAL_EVAL.md): it is byte-for-byte the
same search shape as :class:`engine.bots.weighted.WeightedBot` -- apply every
legal move to a trial copy, evaluate, keep the best -- with exactly one thing
swapped: the linear ``evaluate(trial, idx, w)`` scalar is replaced by the value
network's prediction of the final-culture margin.

Two honesty measures, both load-bearing:

* **Batched eval.** All candidate trial states of one decision are encoded and
  scored in a SINGLE forward pass, so the GPU sees a batch rather than one
  state at a time.  This changes nothing about the policy (still argmax over
  the same candidates); it only makes it affordable.

* **Determinization at the root.** `neural_encode.encode` reads card identity
  in the row, which makes the `end_turn` information leak (docs/BOT_ARCHITECTURE
  section 2.3: an `end_turn` trial refills the row from the REAL civil deck)
  live for this bot in a way it never was for the row-blind linear evaluator.
  So the decks are re-shuffled once at the root before candidates are scored
  (`plan.determinize`), exactly as PlanBot does.  Without this the bot would be
  peeking at cards a human cannot see and its head-to-head numbers would be
  inflated.  `determinize=False` disables it for an A/B that measures the leak.
"""
from __future__ import annotations

import random

from .. import actions
from .fastcopy import copy_state
from .plan import determinize as _determinize
from .neural_encode import encode


class NeuralBot:
    name = "neural"

    def __init__(self, value, seed=None, rng=None, name=None,
                 determinize=True, end_turn_bias=0.0, allow_resign=False):
        # `value` is an engine.bots.neural_net.NeuralValue (or anything with a
        # `.value(list_of_encodings) -> list[float]` method), injected so this
        # class never imports torch itself.
        self.value = value
        self.rng = rng or random.Random(seed)
        self.determinize = determinize
        self.end_turn_bias = end_turn_bias
        self.allow_resign = allow_resign
        if name:
            self.name = name
        # instrumentation
        self.decisions = 0
        self.candidates = 0

    # -- harness adapters ---------------------------------------------------
    def choose(self, state, moves, rng=None):
        return self.pick(state, moves)

    def __call__(self, state):
        return self.pick(state, actions.legal_moves(state))

    def pick(self, state, moves):
        if not self.allow_resign and len(moves) > 1:
            live = [m for m in moves if m[0] != "resign"]
            if live:
                moves = live
        if len(moves) == 1:
            return moves[0]
        idx = state.decider()
        self.decisions += 1

        root = copy_state(state)
        if self.determinize:
            _determinize(root, self.rng)

        encs, valid = [], []
        for mv in moves:
            trial = copy_state(root)
            try:
                actions.apply(trial, mv, self.rng)
                enc = encode(trial, idx)
            except Exception:
                # engine grows move types under us: skip an unscorable
                # candidate, never crash the game (mirrors WeightedBot)
                continue
            encs.append(enc)
            valid.append(mv)
        self.candidates += len(valid)
        if not encs:
            return self.rng.choice(moves)

        scores = self.value.value(encs)
        best, best_val = None, None
        for mv, val in zip(valid, scores):
            if mv[0] == "end_turn":
                val += self.end_turn_bias
            if best_val is None or val > best_val:
                best, best_val = mv, val
        return best if best is not None else self.rng.choice(moves)
