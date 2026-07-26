# Engine build progress

Last update: 2026-07-26.

## Modules on disk

| file | status | notes |
|---|---|---|
| `engine/cards.py` | DONE | Loads the three part-files, tolerates `"complete": false` (records `db.incomplete_parts`). Duplicated military names (Aggression: Plunder I/II/III, the six territories in Ages I & II) get an age suffix so names stay unique keys; `card["baseName"]` keeps the printed name. `has_military` is now data-driven (all of event/territory/aggression/war/bonus/tactic present) → **True**, so the full military game is live. `has_action_cards` still False (no civil action cards in the data). |
| `engine/state.py` | DONE | Serializable `GameState`/`PlayerState`; `to_dict`/`from_dict` round-trip. |
| `engine/effects.py` | DONE | `compute(state,p) -> Stats` in 3 phases; army/tactics strength incl. outdated + air force + Genghis; blue-token math; cost helpers; leader/wonder triggers; per-state stats cache. |
| `engine/economy.py` | DONE | §6.1/§6.2 tables verified by tests; `end_of_turn()` implements §6.6 in exact order. |
| `engine/actions.py` | DONE | `legal_moves` + `apply` with `STRICT=True` legality assert (every self-play game is a legality fuzz test). |
| `engine/events.py` | DONE | Resolves the real card vocabulary: `allPlayers` / `strongestPlayer(s)` / `weakestPlayer(s)` / ranked blocks, all 15 Age III "Impact of …" scoring events, aggression spoils (plunder/spy/armed intervention/enslave/raid), war spoils by base name. Unknown or prose-valued tags are ignored, never fatal. |
| `engine/game.py` | DONE | `new_game / legal_moves / apply / current_player / is_over / scores / winners / play_game`. Start-of-turn (sweep→slide→refill, war resolution, exclusive tactic goes public), politics phase (auto-skipped when passing is the only option), action phase, end-of-turn, age progression with antiquation + 2 yellow tokens, Age IV last-round trigger, final scoring. |
| `engine/bots/__init__.py` | DONE | `RandomBot` (uniform over `legal_moves`), `GreedyBot` (1-ply lookahead over a 19-feature linear eval), `make_bots`. |
| `tests/test_engine.py` | DONE | 28 tests, green under `python3 -m unittest discover -s tests` (and pytest). |
| `experiments/harness.py` | DONE | `python3 -m experiments.harness` round-robins bots and appends JSONL. |

## Smoke run (2026-07-26, RandomBot, `STRICT=True`)

200 games at 4 players and 200 at 2 players, seeds 0-199:

| | 4p × 200 | 2p × 200 |
|---|---|---|
| crashes | 0 | 0 |
| move-cap hits (cap 20000) | 0 | 0 |
| player-turns per game: mean / median / min / max | 79.0 / 80 / 68 / 88 | 28.3 / 28 / 26 / 30 |
| rounds per game (mean) | 19.8 | 14.2 |
| final score: mean / median / max | 9.8 / 10 / 78 | 7.5 / 4 / 41 |
| speed | 23 games/s | 60 games/s |

Speed target in ARCHITECTURE.md is ≥50 4p games/**minute**; we do ~1400/minute
with the legality assert switched on (set `engine.actions.STRICT = False` for
another ~25%).

Extra fuzz: 600 further RandomBot games (2p/3p/4p × seeds 200-399) — 0
failures, every game reached Age IV and scored.

GreedyBot vs RandomBot, 2p × 30 seeds: greedy wins 30-0, mean culture 39.2 vs
6.4. Random play scores near zero because random bots destroy their own
buildings and never build a temple — expected, and it makes the invariant
fuzzing harsher, not weaker. GreedyBot self-play mean final culture:
30.0 (2p) / 32.9 (3p) / 62.7 (4p), max seen 146.

Round robin (`python3 -m experiments.harness --games 24 --players 4`):
greedy 43.1% win rate at 75.5 mean culture, random 4.4% at 9.1.

## Known gaps / deliberate simplifications
- **Colonization auctions (§11) not implemented**: a territory revealed as the
  current event goes to the past-events pile, so colonies never enter play
  (`culturePerColony`, Annex, colony permanents are all dead code paths).
- **Pacts (§5.9/§5.10) not implemented**: pact cards sit in hand and are
  discarded at age end; no offer/accept/cancel political actions.
- **Resigning (§5.11) not implemented.**
- The Age A civil deck in the data is 10 cards (4 wonders + 6 leaders), fewer
  than the 13 row spaces, so setup deals the whole Age A deck and finishes the
  row from the Age I deck (FAQ p.11 rule for running out mid-fill). Side effect:
  Age A ends during setup instead of at the first replenish — harmless, since
  the A→I transition has no antiquation or token loss.
- No civil ACTION cards in the data yet → the civil decks are ~10 cards short
  per age and `("play_action", n)` never appears.
- Defender decisions in aggressions are automatic (discard cheapest-first
  until safe); the attacker's raid targets the most expensive building.
- Food/resources are scalars; blue-token occupancy (and therefore corruption)
  is derived greedily from the farm/mine denominations in play (§6.4 model).
- Event effects that require a player decision (free builds, one-time
  discounts, "choose", flip a wonder, destroy your own building) are ignored.
- GreedyBot is myopic by construction: it never plans two actions ahead, so it
  under-develops technologies. It is a baseline, not a strong player.

## Next steps
1. `WeightedBot` + hill climbing in `experiments/` over `engine.bots.WEIGHTS`.
2. Colonization auctions and pacts (needs a multi-player decision point in the
   move generator — the first thing in the engine that is not a single-player
   choice).
3. Civil action cards once the data file gains them.
