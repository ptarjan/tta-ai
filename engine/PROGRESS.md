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
| `tests/test_engine.py` | DONE | 57 tests, green under `python3 -m unittest discover -s tests` (and pytest). |
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

| bot | players | crashes | move-cap hits | mean player-turns | mean rounds | mean final score | max score | games/s |
|---|---|---|---|---|---|---|---|---|
| RandomBot | 2 | **0** | 0 | 41.3 | 21.2 | 12.1 | 72 | 15.9 |
| RandomBot | 3 | **0** | 0 | 63.2 | 21.7 | 13.1 | 114 | 9.6 |
| RandomBot | 4 | **0** | 0 | 103.8 | 26.7 | 20.7 | 175 | 5.5 |
| GreedyBot | 2 | **0** | 0 | 47.4 | 24.2 | 67.0 | 141 | 1.6 |
| GreedyBot | 3 | **0** | 0 | 77.1 | 26.4 | 86.3 | 173 | 0.8 |
| GreedyBot | 4 | **0** | 0 | 148.1 | 37.8 | 106.9 | 228 | 0.5 |

**1200 games, 0 crashes and 0 move-cap hits.**

New-mechanic coverage (log-line counts) showing every closed gap is actually
being exercised in self-play, not merely reachable:

| | RandomBot 2p | 3p | 4p | GreedyBot 2p | 3p | 4p |
|---|---|---|---|---|---|---|
| action cards played | 1557 | 2102 | 3320 | 337 | 522 | 583 |
| colonizations won | 140 | 254 | 502 | 152 | 209 | 244 |
| pacts accepted | 0 (correct, §13) | 559 | 936 | 0 (correct, §13) | 0 | 0 |
| aggressions played | 393 | 804 | 1582 | 2 | 21 | 36 |
| wars declared | 155 | 250 | 527 | 0 | 0 | 0 |

GreedyBot barely uses the political actions (its eval has no military feature
worth spending on), so the RandomBot columns are what stress the new code.

Speed note: RandomBot 4p fell from ~23 games/s to ~5.5 in this pass. About half
of that is games genuinely being longer (mean player-turns 79 → 104, because
colonies, action cards and pacts all extend a civilization's life) and the rest
is the bigger move space plus `_action_card_playable`, which probes the ordered
action of every yellow card in hand. 5.5 games/s is still ~330 4p games/minute
against the ≥50/minute target in ARCHITECTURE.md, and `engine.actions.STRICT =
False` buys back roughly another 25%.

Hand-off note for the bots work: GreedyBot plays only 1.7 action cards per game
versus RandomBot's 7.8, because its linear eval has no feature for them and it
therefore almost never takes one from the row. Worth a `WeightedBot` feature.

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

---

# Performance pass (2026-07-26)

Goal: throughput for self-play hill climbing, with **zero behaviour change**.
The guard rail is `engine/perf_check.py`:

    python3 -m engine.perf_check save /tmp/fp.json   # fingerprint HEAD
    python3 -m engine.perf_check check /tmp/fp.json  # after every change
    python3 -m engine.perf_check bench               # games/second table

The fingerprint is a SHA-256 over the full log, final scores, winners, move
count, turn and round of 33 fixed games (2p/3p/4p x RandomBot/GreedyBot x
seeds).  Every optimisation below kept it at
`3229c4a0f0d6a4a122ee5e16d44cbc99728da4a9e1855e6ceb36532045223ad7`, and the 57
tests stayed green.

### Current determinism digests (re-baselined 2026-07-26 at 15b9764)

`3229c4a0…` above is **stale**. It was invalidated by the *rules* change in
f4bcac0 (yellow action cards resolve their ordered action first, gains after),
not by any performance work. Measured at HEAD, `nice -n 10`, 58 tests green:

| fingerprint | cases | digest |
|---|---|---|
| narrow (`perf_check check tools/fingerprint.json`) | 33 | `c2befef1bb640a05b5862627d7a1fb76134adff562fec748b044d89dc056755a` |
| wide (`perf_check check --wide tools/fingerprint_wide.json`) | 102 | `47e06a41c8a888891a90090272374a0e9b87c237d8be103cb4db29627f4ec46d` |

Both agree with the cross-interpreter baseline recorded in docs/PYPY.md, so
CPython and PyPy still play all 135 fixed games byte-identically.

Note for whoever owns `tools/`: **`tools/fingerprint.json` (3229c4a0…) and
`tools/fingerprint_wide.json` (c7e73ede…) are the pre-f4bcac0 files and will
report MISMATCH on every run** until they are re-saved. The digests above are
what a re-save should produce.

The four bot fixes of 2026-07-26 (6376981 `state.decider()`, 0808b64 deferred
payoff credit, 166867d yield-based pact/colony pricing, 15b9764 weight reset)
**did not move either digest** — `perf_check` fingerprints only `RandomBot`
and `GreedyBot`, and all four changes are confined to
`engine/bots/weighted.py`. `GreedyBot`'s own behaviour last changed at 5575110
(lazy trial-rng reseed), which was verified digest-preserving at the time.
`WeightedBot` behaviour *has* changed a great deal and is deliberately not
under any fingerprint; see docs/PACTS_DIAGNOSIS.md for the measured effect.

## Baseline (commit fce7db8)

| bot | 2p | 3p | 4p |
|---|---|---|---|
| random | 20.07 games/s | 13.35 | 7.73 |
| greedy | 1.98 games/s | 1.06 | 0.47 |

Profile, 60 4p RandomBot games, 25.3 s, 24.5 M calls, `tottime` order:

| tottime | cum | function |
|---|---|---|
| 2.77 | 18.10 | `actions._action_moves` |
| 1.86 | 7.07 | `actions.can_take` (581 k calls) |
| 1.45 | 2.40 | `effects.build_cost` (321 k) |
| 1.35 | 5.13 | `effects.compute` (52 k) |
| 1.34 | 1.99 | `actions.take_cost` (586 k) |
| 1.21 | — | `dict.get` (3.0 M) |
| 0.85 | — | `cards.type_of` (2.55 M) |
| 0.82 | 6.28 | `effects.state_stats` (1.06 M) |
| 0.72 | — | `cards.get` (2.52 M) |
| 0.57 | — | `cards.db()` (2.47 M) |

Read: **move generation is the whole cost.** `legal_moves` is called twice per
move (once by the bot, once by the STRICT assert in `apply`), and inside it the
same card-database facts and the same per-player stats are re-derived hundreds
of times per call.

## Progress log (games/cpu-s, `perf_check bench`)

`time.process_time` of this process, not wall clock: the hill-climb runs keep
every core busy, so wall clock is noise.  30 games per random cell, 4 per
greedy cell.

| commit | rand 2p | rand 3p | rand 4p | greedy 2p | greedy 3p | greedy 4p |
|---|---|---|---|---|---|---|
| fce7db8 baseline | 20.07 | 13.35 | 7.73 | 1.98 | 1.06 | 0.47 |
| 76d691e (STRICT off, card-DB, set unions) | 37.22 | 25.92 | 13.96 | 3.24 | 1.63 | 0.79 |
| 0d71ba0 compiled effect programs | 38.96 | 26.44 | 15.07 | 3.59 | 1.72 | 0.78 |
| c8a70a4 cached tableau scaffolding | 50.70 | 33.09 | **18.60** | 4.22 | 2.14 | 1.01 |

(The tableau-scaffolding change is `engine/actions.py` inside c8a70a4 -- it got
swept into another agent's commit; the engine part of that diff is mine.)

### What each step did

1. **0d71ba0 — compiled effect programs.**  Every `effects` / `production`
   dict in the card DB is classified once into `(flat, modifier, special)`
   tuples, cached by `id(dict)` with a strong ref so ids cannot be recycled.
   `_apply_flat` / `_add_production` became straight-line loops writing
   through `Stats.__dict__` instead of `setattr`/`getattr` plus two dict
   membership tests per key.  Phase 1 of `compute` got `_TECH_PROG`: each
   technology name maps to its per-worker `(attr, value)` contributions.
   `_colony_permanents` is memoized per card (it built a fresh dict per colony
   per compute, which also defeated the id-cache).  Tactic `composition ->
   need` is cached and the non-Genghis army count is a straight-line fast path.
2. **c8a70a4 (engine part) — cached move-generation scaffolding.**
   `_action_moves` used to rebuild `sorted(p.techs)`, the worker-name list,
   the `type -> names` map and the upgrade candidate pairs on *every* call.
   All of it is a pure function of the *set* of tableau names, so it now lives
   in an `lru_cache` keyed by `tuple(p.techs)`: cheap to build, cheap to hash
   (interned strings cache their hash) and self-invalidating.  Upgrade targets
   are precomputed per card, so the inner double loop no longer does a level
   comparison.  `sorted(set(hand))` scans are cached the same way.

### Next step

`effects.compute` is still ~25% of a 4p RandomBot game (52 k calls for 32 k
moves) because `actions.apply` invalidates the per-player stats cache
unconditionally after every move, and `invalidate(state)` full-clears wipe all
four players.  Next: make invalidation precise, guarded by a paranoid mode
that recomputes and compares on every `state_stats` call.
