# TtA AI Engine — Architecture

Goal: full-game 2–4 player Through the Ages (2015 "New Story" edition) engine +
self-play-trained AI + human advisor mode. Built 2026-07-25/26.

## Pipeline stages
1. **Rules research** → `docs/RULES_SPEC.md`, `data/cards.json` (background agent, running)
2. **Engine** → `engine/` Python package: immutable-ish GameState, legal move
   generator, deterministic transition function, full turn loop (politics →
   civil actions → end-of-turn production/corruption/military draw), war/pact/
   event resolution, end-of-game scoring. Seeded RNG for reproducibility.
3. **Bots** → `engine/bots/`: RandomBot (legality fuzzer), GreedyBot
   (1-ply eval), WeightedBot (linear eval over hand-designed features with a
   weight vector — the hill-climbing substrate).
4. **Self-play** → `experiments/`: round-robin + mutate-and-select
   (evolution strategy) over WeightedBot weight vectors, 2p/3p/4p tables,
   thousands of games, results logged to JSONL.
5. **Distillation** → `docs/HEURISTICS.md`: human-readable strategy rules
   extracted from winning weight vectors + game logs (opening priorities,
   tempo rules, military thresholds, when to change government, etc.)
6. **Advisor** → `advisor/`: CLI where a human reports observable state
   (their board, card row, opponents' visible state) and the engine returns
   recommended actions for the turn. State entry must be terse (few keystrokes).

## Engine design notes
- Python 3.11+, stdlib only. `dataclasses` for state, no OO card classes —
  cards are data (from cards.json), effects dispatch on effect tags.
- GameState fully serializable (JSON) → enables advisor mode to reconstruct
  state and self-play checkpointing.
- Move = small tagged tuple, e.g. ("take_card", row_idx), ("build", tech, n),
  ("play_leader", card_id). Legal move generator is the single source of truth;
  bots choose only among generated moves.
- Determinism: all randomness through a passed-in `random.Random(seed)`.
- Hidden information: engine supports full-information self-play; advisor mode
  works from one player's observable view (opponent hands unknown — advisor
  only needs visible state anyway).
- Speed target: ≥50 full 4p games/minute for RandomBot (needed for hill climb).

## Testing
- `tests/`: rules unit tests keyed to RULES_SPEC sections; full-game smoke
  tests (RandomBot x4 to completion, thousands of seeds, invariant checks:
  non-negative resources, worker conservation, culture monotonicity where
  required, game always terminates).
