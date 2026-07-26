# advisor/ progress

Goal: let a human at a physical Through the Ages board get move
recommendations from the trained bot, and report back what happened between
turns with minimal typing.

## Done

- `advisor/state_io.py` — terse text format for the board.
  - `Board` = engine `GameState` + `me` seat + hidden-card counts + set of
    fields the human declared unknown.
  - `dumps` / `loads`: full snapshot, exact text round-trip (tested on real
    mid-game states from a 220-move greedy self-play game).
  - `patch(board, line)`: the between-turn one-liners (`deal`, `row`, `take`,
    `p1 c=34 str=7`, `p1 tech+ ...`, `p1 wonder ...`, `age II`, `?` for
    unknown). Every bad input raises `PatchError` — never crashes.
  - `resolve_card`: fuzzy card matching (exact / prefix / initials /
    subsequence) scoped by pool, with `AmbiguousCard` listing the options.
  - `render(board)`: human-checkable board summary.
- `advisor/tests/test_state_io.py` — 26 tests, green.

## Next step

Write `advisor/advisor.py`: the interactive loop.
  - `Advisor` class wrapping a `Board` + a bot, with `recommend()` returning
    the top 3 (move, score, plain-English reason) tuples using
    `engine.bots.weighted.WeightedBot` + `experiments/champion_{N}p.json`
    when present, else `DEFAULT_WEIGHTS`.
  - `describe_move(state, move)` -> plain English ("take Philosophy from
    slot 3 (2 civil actions)").
  - REPL: show recommendation, human confirms (`y`) or overrides (`take 4`),
    apply to mirror, then collect opponent updates as `patch` lines.

Then: `advisor/README.md` worked transcript, `advisor/tests/test_session.py`
scripted end-to-end session.

## Rules

- Never edit anything under `engine/` or `experiments/` (other agents own
  them). Use only the public API: `new_game, legal_moves, apply,
  current_player, is_over, scores`.
- Run tests with `python3 -m unittest discover -s advisor/tests -t .`
