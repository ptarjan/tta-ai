# Engine build progress

Last update: 2026-07-26.

## Modules on disk

| file | status | notes |
|---|---|---|
| `engine/cards.py` | DONE | Tolerates incomplete part-files: loads them anyway, records `db.incomplete_parts`, exposes `db.has_military` (all required military types present AND file complete) and `db.has_action_cards`. `db()` singleton, `level()/level_of()`, type sets (URBAN/UNIT/PRODUCTION/WORKER_TYPES). Currently `has_military=False` (military file only has tactics) → engine runs CIVIL-ONLY. |
| `engine/state.py` | DONE | Added fields: `blue_total` (total blue tokens owned; bank level is derived), `taken_leader_ages`, `tactic_exclusive`, per-turn flags (`politics_done`, `tactic_action_used`, `taken_this_turn`, `hammurabi_used`, `churchill_used`, `bach_upgrade_used`), `destroyed_wonders`, `round`, `start_player`, `age_military`, `past_events`, `has_military`, `last_round`. `to_dict`/`from_dict` round-trip. |
| `engine/effects.py` | DONE | `compute(state,p) -> Stats` in 3 phases (gov+workers → flat card effects → per-X modifiers). Army/tactics strength incl. outdated + air force + Genghis. Blue-token math (`tokens_for`, `blue_used`, `blue_available`, `gain_food/gain_resources/pay_resources`). Cost helpers (`build_cost`, `tech_cost` with Masonry/Architecture/Engineering/Bach/Shakespeare discounts). Triggers: `on_take_card` (Aristotle), `on_develop` (Leonardo/Newton/Einstein), `on_build_unit` (Homer), `on_wonder_complete` (Age III one-time culture: First Space Flight / Fast Food Chains / Internet / Hollywood), `end_of_game_bonus` (Bill Gates). Per-state stats cache with `invalidate()`. |
| `engine/economy.py` | DONE | Tables verified vs §6.1/§6.2: `pop_cost_base` 2/3/4/5/7, `consumption` 0/1/2/3/4/6, `happy_required` 0..8, `corruption` 0/2/4/6. `end_of_turn()` implements §6.6 exactly (discard excess military → uprising check → score/corruption/food prod/consumption/resource prod → military draw (max 3, not in age IV, not round 1) → reset actions). `increase_population`, `lose_population`, military deck draw/reshuffle. |
| `engine/actions.py` | DONE (untested) | `legal_moves(state)` + `apply(state, move, rng)` with `STRICT=True` legality assert. Moves: `("take",idx) ("pop",) ("pop_free",) ("build",n) ("upgrade",lo,hi) ("destroy",n) ("wonder_step",k) ("play_leader",n) ("develop",n) ("revolution",n) ("churchill",opt) ("play_tactic",n) ("copy_tactic",n) ("play_action",n) ("end_turn",)` and politics `("pol_pass",) ("prepare_event",n) ("aggression",n,tgt) ("war",n,tgt)`. Round-1 restriction (§1.9: takes only). Hammurabi MA-as-CA, Michelangelo wonder discount, urban limits, special-tech one-per-icon replacement, peaceful vs revolution government change (Robespierre/Newton). |
| `engine/events.py` | DONE (untested, dormant) | Generic event/aggression/war resolvers over a small effect-tag vocabulary; unknown tags logged + ignored. War spoils per §5.8. Dormant until `has_military` flips true. |
| `engine/game.py` | **NEXT** | not written yet |
| `engine/bots/__init__.py` | TODO | RandomBot |
| `tests/test_engine.py` | TODO | |

## Exact next step
Write `engine/game.py`:
`new_game(num_players, seed)` (starting tableau Warriors/Agriculture/Bronze/
Philosophy/Religion + Despotism, 18 yellow bank, 1 free worker, 16 blue,
science 1 rating; deal 13-card row from Age A civil deck = 10 cards today);
`start_turn` (sweep 1/2/3 by player count → slide → refill; resolve my
declared war; exclusive tactic goes public), `end_turn` (economy.end_of_turn
→ advance current player/round → start_turn), age advance when the last card
of the civil deck is dealt (antiquation: hands, leaders, unfinished wonders,
−2 yellow bank tokens), Age IV → last round → `is_over`/`scores`
(culture + `effects.end_of_game_bonus`). Then `engine/bots/__init__.py`
(RandomBot), then `tests/test_engine.py`, then the 200-game smoke run.

## Known gaps / deliberate simplifications
- Civil-only mode until `data/cards_military_actions.json` is complete: no
  politics phase, no military draws, no events/aggressions/wars, no tactics.
  Everything is gated on `db.has_military`, so it switches on automatically.
- No civil ACTION cards in the data yet → the civil decks are ~10 cards
  short per age; `("play_action", n)` only appears for cards whose effects
  use the known tag vocabulary.
- Colonization auctions (§11) not implemented; revealed territories go to
  the past-events pile.
- "Discard for resources" from the build order is not a base-game action
  (§3 note: there is no action to discard civil cards) — omitted on purpose;
  `("destroy", n)` / disband covers the worker-removal case.
- Food/resources are scalars; blue-token occupancy (and thus corruption) is
  derived greedily from the farm/mine denominations in play (§6.4 model).
- Defender decisions in aggressions are automatic (discard until safe).
