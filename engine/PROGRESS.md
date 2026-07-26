# Engine build progress

Last update: 2026-07-26.

## Modules on disk

| file | status | notes |
|---|---|---|
| `engine/cards.py` | DONE | Loads the three part-files. Duplicated military names (Aggression: Plunder I/II/III, the six territories in Ages I & II) get an age suffix so names stay unique keys; `card["baseName"]` keeps the printed name. `has_military` and `has_action_cards` are both **True** — the card data is complete (142 cards). |
| `engine/state.py` | DONE | Serializable `GameState`/`PlayerState`; `to_dict`/`from_dict` round-trip. Carries `pending` (decision stack) and `queue` (deferred sub-effects) so non-current-player decisions stay serializable. |
| `engine/effects.py` | DONE | `compute(state,p) -> Stats` in 3 phases; army/tactics strength incl. outdated + air force + Genghis; blue-token math; cost helpers; leader/wonder/pact triggers; per-state stats cache. |
| `engine/economy.py` | DONE | §6.1/§6.2 tables verified by tests; `end_of_turn()` implements §6.6 in exact order. |
| `engine/actions.py` | DONE | `legal_moves` + `apply` with `STRICT=True` legality assert (every self-play game is a legality fuzz test). Politics phase covers prepare-event / aggression / war / offer-pact / cancel-pact / resign. |
| `engine/interact.py` | DONE | The decision points that are NOT the current player's: colonization auctions (§11), aggression defense (§5.4.4), pact offers (§5.9), action-card ordered actions (§3.11) and every "each player chooses" event effect (§5.3). |
| `engine/events.py` | DONE | Resolves the real card vocabulary: `allPlayers` / `strongestPlayer(s)` / `weakestPlayer(s)` / ranked blocks, all 15 Age III "Impact of …" scoring events, aggression spoils, war spoils. **Every effect key in the event/territory data is now handled** (checked by a coverage script; only prose `note` and qualifier keys like `order`/`statistics`/`ignoreCorruption` are deliberately inert). |
| `engine/game.py` | DONE | `new_game / legal_moves / apply / current_player / is_over / scores / winners / play_game`; `decider()` drives the turn loop so pending decisions are handled uniformly. Age progression with antiquation + 2 yellow tokens, Age IV last-round trigger, final scoring, resignation handling. |
| `engine/bots/__init__.py` | DONE (other agent) | `RandomBot`, `GreedyBot`, `WeightedBot`, `make_bots`. |
| `tests/test_engine.py` | DONE | 56 tests, green under `python3 -m unittest discover -s tests` (and pytest). |
| `experiments/` | DONE (other agent) | harness / arena / hillclimb. |

## Gaps closed in this pass

1. **Colonization (§11)** — a territory revealed as the current event opens a real
   bidding round (`("bid", n)` / `("bid_pass",)`), clockwise from the revealing
   player, bids capped by the bidder's actual maximum colonization force. The
   winner must colonize: units are sacrificed to the YELLOW BANK (not the worker
   pool), bonus cards are discarded before any card draw, then the permanent
   effects apply and then the immediate effect. Colonization force deliberately
   excludes every strength-rating modifier (§11.3) and only counts air units
   actually sacrificed into the force.
2. **Pacts (§5.9/§5.10) and resigning (§5.11)** — `offer_pact` hands a real
   accept/refuse decision to the named partner; a refusal returns the card to
   hand and still burns the political action; accepting replaces any previous
   pact in the offerer's area. Pacts are suppressed at 2 players (§13) and
   antiquate away at age end. `cancel_pact` is legal for either party.
   Resigning discards everything, drops the player's pacts, pays 7 culture to
   each declarer of a war against them, re-trims future-age decks and the sweep
   count for the surviving player count, and ends the game when one player is
   left.
3. **Civil (yellow) action cards (§3.11)** — all 33 age-variants play. Cost 1 CA,
   cannot be played the turn they were taken, leave the game after resolving.
   The card's gains resolve first and the ordered action second (so
   Breakthrough's +science pays for the technology it develops and Frugality's
   +food for the population increase), the ordered action pays no civil/military
   action, and `resourceDiscount` comes off its cost with a floor of 0. A card
   that orders an action you cannot perform is not a legal move.
   `Patriotism` / `Wave of Nationalism` / `Military Build-Up` grant a per-turn
   resource-discount pool for military unit builds and upgrades; the
   per-player-count values (`{2p,3p,4p}`) are read against the LIVE player count.
4. **Event effects needing a decision** — free builds, "choose one", destroy your
   own building, lose population, lose a colony, flip a wonder, discard military
   cards and International Agreement's optional take-cards are all real moves on
   the pending stack, resolved clockwise from the revealing player (§5.3).
   Non-decision computed gains (`scienceEqualToScienceProduction`,
   `cultureEqualToCultureProduction`, `cultureEqualToScienceProduction`,
   Prosperity's `foodEqualToHappyFaces` with its cap) were added too.
5. **Aggression defense (§5.4.4)** — the defender, not the engine, chooses which
   military bonus cards to play (printed defense value) and which other cards to
   discard face down (+1 each), up to their military action TOTAL. Ties favour
   the defender. Raid/Annex/Infiltrate targeting is the attacker's decision.

## Smoke run (2026-07-26)

`python3 /tmp/smoke.py` — 200 games per configuration, seeds 0-199, `STRICT=True`
(so every one of the 1200 games is also a move-legality fuzz test).

SMOKE_TABLE_PLACEHOLDER

## Known gaps / deliberate simplifications
- The winner of a colonization auction sacrifices a greedy minimal force
  (weakest units first, bonus cards before extra units) rather than choosing;
  the choice of WHAT to sacrifice is not exposed as a move.
- The Age A civil deck in the data is 20 cards; setup deals 13 to the row, so
  Age A ends during setup instead of at the first replenish — harmless, since
  the A→I transition has no antiquation or token loss.
- Food/resources are scalars; blue-token occupancy (and therefore corruption)
  is derived greedily from the farm/mine denominations in play (§6.4 model).
- **Assumption**: `resourcesForMilitaryUnits` (Patriotism etc.) is modelled as a
  TOTAL discount pool for the turn, not a per-unit discount. The printed text
  ("build or upgrade military units; pay N fewer resources", plural) supports
  the pool reading, but it is not confirmed by the rulebook — see
  docs/OPEN_QUESTIONS.md.
- **Assumption**: an action card's gains resolve BEFORE its ordered action.
  This makes the 2015 wording of Breakthrough/Frugality equivalent to the older
  editions' "pay N less" phrasing, which is almost certainly the intent.
- `Aggression: Plunder`'s food/resource mix is split by the engine rather than
  chosen by the attacker.
- GreedyBot is myopic by construction: a baseline, not a strong player.

## Next steps
1. Expose the colonization sacrifice as a decision (currently greedy).
2. Confirm the two assumptions above against the rulebook/FAQ and record them in
   docs/OPEN_QUESTIONS.md.
3. Teach `WeightedBot` about the new move kinds (`bid`, `defend`, `choose`,
   `play_action`) — it currently sees them but has no dedicated features.
