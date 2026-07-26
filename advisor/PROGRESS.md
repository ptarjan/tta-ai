# advisor/ progress

Goal: let a human at a physical Through the Ages board get move
recommendations from the trained bot, and report back what happened between
turns with minimal typing.

## Status: complete and working

- `advisor/state_io.py` — terse text format for the board.
  `Board` = engine `GameState` + `me` seat + hidden-card counts + unknown
  fields. `dumps`/`loads` round-trip exactly (tested on real mid-game
  states). `patch(board, line)` applies the between-turn one-liners
  (`deal`, `row`, `take p1 3`, `p1 c=34 str=7`, `p1 tech+ ...`, `age II`,
  `?` for unknown). `resolve_card` does fuzzy matching (exact / prefix /
  initials / subsequence) scoped by pool. `render` prints the board.
  Every bad input raises `PatchError` — never a crash.
- `advisor/advisor.py` — `load_bot` (champion_{N}p.json else defaults),
  `rank_moves` (top-N with score gap + plain-English reason from the
  feature delta), `describe_move`, `parse_move` (fuzzy verb+args),
  `Advisor` (mirror + `play` / `skip_opponent_turn` / `set_dealt`),
  `Console` (the REPL, `--load` to resume).
- `advisor/README.md` — usage tables plus a captured transcript of a real
  advised turn.
- `advisor/tests/` — 49 tests, green:
  `python3 -m unittest discover -s advisor/tests -t .`

## Possible follow-ups

- Deeper search than the bot's 1 ply (e.g. full-turn rollout) for the
  recommendation, which matters most in the last round.
- Track rival hand *contents* when a take is public, to warn about
  aggressions they can afford.
- A `--log` flag writing every snapshot to disk for post-game review.

## Rules for whoever picks this up

- Never edit anything under `engine/` or `experiments/` (other agents own
  them). Use only the public API: `new_game, legal_moves, apply,
  current_player, is_over, scores`.
- Never assume a fixed move vocabulary: `describe_move` and `parse_move`
  both fall back gracefully on move kinds they have not seen.
