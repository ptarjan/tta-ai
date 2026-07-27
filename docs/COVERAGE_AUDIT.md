# Coverage audit: what the bots never do, and why

Branch `coverage-audit`, worktree `/Users/pt/tta-ai-coverage-audit`, off master
`8e751cb`. Base game 2015 only.

Two questions, deliberately kept apart, because they need different fixes:

* **engine-wrong** — the mechanic is mis-implemented, so the bots are right to
  avoid a thing that does not work;
* **search-blind** — the engine is right, but no feature or weight can express
  the mechanic's value, so the hill climb can never learn to use it.

Both turned out to be present, and a third category matters too: some things
*should* be rare, and saying so is a real answer.

The wars / aggressions / pacts / military-card / defence-strength cluster is
another agent's (`docs/COMBAT_AUDIT.md`). Where the census touches it the
numbers are reported and the analysis is handed off, not duplicated.

## What was built

| tool | what it measures |
|---|---|
| `tools/coverage_census.py` | per mechanic, in how many decisions it was **legal** and in how many it was **taken**, over real self-play at 2p/3p/4p, with the live champions and with `DEFAULT_WEIGHTS` |
| `tools/feature_variance.py` | per feature, how often it **varies across the candidates of a decision** (`varying`), the mean spread (`mean_range`), and how often zeroing its weight **changes the chosen move** (`flip`) |

Both are one-worker, `TTA_JOURNAL=1`, and both are pinned by
`tests/test_coverage_tools.py` — in particular `feature_variance.score_from`
is asserted equal to `weighted.evaluate` to 1e-9 on real candidate states, so
if the evaluator changes shape the tool fails loudly instead of lying quietly.

Rulebook conformance for everything found is in `tests/test_coverage_audit.py`
(15 tests, all constructed positions — self-play cannot test a mechanic the
bots never use).

Test count: master `8e751cb` = 156. This branch = 176, all green. Four of the
new tests fail against `8e751cb`'s `engine/actions.py`; that is the point.

## 1. The census

40 games per cell, all seats the same bot. Cell reads `taken / decisions where
legal (take-rate)`. A "decision where legal" is one decision point at which at
least one move of that kind was in `legal_moves`.

| mechanic | 2p champ | 2p def | 3p champ | 3p def | 4p champ | 4p def |
|---|---|---|---|---|---|---|
| `destroy:urban` | 26/4511 (1%) | 0/3183 (0%) | 21/4936 (0%) | 2/4896 (0%) | 1/11294 (0%) | 6/8488 (0%) |
| `resign` | 0/1331 (0%) | 0/1507 (0%) | 0/2460 (0%) | 0/2462 (0%) | 14/2789 (1%) | 5/4488 (0%) |
| `take:infantry` | 4/855 (0%) | 1/650 (0%) | 3/1331 (0%) | 0/1302 (0%) | 7/3000 (0%) | 2/2338 (0%) |
| `build:farm` | 0/767 (0%) | 0/498 (0%) | 7/848 (1%) | 3/971 (0%) | 445/3720 (12%) | 6/1254 (0%) |
| `take:arena` | 0/718 (0%) | 15/509 (3%) | 44/804 (5%) | 41/762 (5%) | 19/2319 (1%) | 62/1729 (4%) |
| `cancel_pact` | — | — | 0/1276 (0%) | 0/993 (0%) | 10/761 (1%) | 0/2215 (0%) |
| `take:air` | 0/428 (0%) | 0/353 (0%) | 0/483 (0%) | 1/514 (0%) | 7/1512 (0%) | 4/1291 (0%) |
| `war` | 0/377 (0%) | 0/412 (0%) | 0/736 (0%) | 0/591 (0%) | 0/606 (0%) | 0/1513 (0%) |
| `upgrade:mine` | 0/68 (0%) | 24/58 (41%) | 30/110 (27%) | 30/81 (37%) | 17/81 (21%) | 128/250 (51%) |
| `upgrade:unit` | 1/4 (25%) | 0/4 (0%) | 1/1 (100%) | — | 2/4 (50%) | — |
| `take:lab` | 4/1251 (0%) | 100/613 (16%) | 1/1385 (0%) | 107/821 (13%) | 271/390 (69%) | 165/1356 (12%) |
| `take:wonder` | 94/2151 (4%) | 2/2476 (0%) | 9/3582 (0%) | 4/3546 (0%) | 259/3450 (8%) | 13/6153 (0%) |
| `destroy:farm` | 20/4376 (0%) | 7/2541 (0%) | 4/4658 (0%) | 14/4147 (0%) | 373/10648 (4%) | 7/6449 (0%) |
| `aggression` | 1/300 (0%) | 2/400 (0%) | 1/859 (0%) | 4/735 (1%) | 6/760 (1%) | 6/1892 (0%) |
| `take:cavalry` | 5/1067 (0%) | 1/804 (0%) | 7/1363 (1%) | 2/1262 (0%) | 12/3193 (0%) | 5/2438 (0%) |
| `take:artillery` | 2/627 (0%) | 2/439 (0%) | 2/809 (0%) | 4/785 (1%) | 24/2218 (1%) | 5/1658 (0%) |
| `take:farm` | 6/917 (1%) | 13/675 (2%) | 15/1224 (1%) | 17/1174 (1%) | 9/3102 (0%) | 44/2258 (2%) |
| `destroy:mine` | 24/4502 (1%) | 23/3161 (1%) | 26/4934 (1%) | 29/4875 (1%) | 98/10909 (1%) | 31/8383 (0%) |
| `take:special-tech` | 170/1647 (10%) | 8/1395 (1%) | 26/2324 (1%) | 18/2168 (1%) | 312/3931 (8%) | 49/4089 (1%) |
| `take:mine` | 6/873 (1%) | 13/683 (2%) | 21/1274 (2%) | 20/1184 (2%) | 41/3168 (1%) | 59/2258 (3%) |
| `destroy:unit` | 230/1156 (20%) | 75/1413 (5%) | 46/3111 (1%) | 78/2235 (3%) | 931/8350 (11%) | 69/3078 (2%) |
| `upgrade:farm` | 3/92 (3%) | 12/35 (34%) | 17/43 (40%) | 12/19 (63%) | 2/3 (67%) | 31/127 (24%) |
| `play_tactic` | 287/1607 (18%) | 101/1482 (7%) | 128/2493 (5%) | 124/2327 (5%) | 259/3183 (8%) | 160/4541 (4%) |
| `develop:urban` | 387/1204 (32%) | 91/2256 (4%) | 130/2432 (5%) | 138/3310 (4%) | 412/6244 (7%) | 250/5939 (4%) |
| `prepare_event:territory` | 90/357 (25%) | 162/413 (39%) | 268/604 (44%) | 264/567 (47%) | 37/797 (5%) | 412/799 (52%) |
| `build:mine` | 186/767 (24%) | 124/498 (25%) | 226/848 (27%) | 209/971 (22%) | 182/3720 (5%) | 261/1254 (21%) |
| `take:government` | 130/1122 (12%) | 78/942 (8%) | 103/1556 (7%) | 92/1522 (6%) | 236/2982 (8%) | 150/2736 (5%) |
| `take:leader` | 192/1571 (12%) | 89/1556 (6%) | 161/2282 (7%) | 144/2196 (7%) | 395/3311 (12%) | 252/3524 (7%) |
| `play_action` | 286/3304 (9%) | 141/1697 (8%) | 171/2750 (6%) | 146/2498 (6%) | 709/6422 (11%) | 281/4683 (6%) |
| `develop:unit` | 6/15 (40%) | 1/17 (6%) | 4/50 (8%) | 5/14 (36%) | 27/45 (60%) | 7/76 (9%) |
| `prepare_event:event` | 555/985 (56%) | 894/1115 (80%) | 1304/1605 (81%) | 1306/1589 (82%) | 135/2007 (7%) | 1660/2032 (82%) |
| `pop` | 371/1415 (26%) | 170/2338 (7%) | 357/4145 (9%) | 318/3588 (9%) | 1433/3421 (42%) | 403/5884 (7%) |
| `develop:mine` | 5/6 (83%) | 12/44 (27%) | 10/138 (7%) | 14/127 (11%) | 29/41 (71%) | 44/331 (13%) |
| `build:unit` | 157/913 (17%) | 51/518 (10%) | 111/845 (13%) | 73/985 (7%) | 1049/4002 (26%) | 93/1262 (7%) |
| `copy_tactic` | 693/4025 (17%) | 179/1843 (10%) | 269/3415 (8%) | 242/3092 (8%) | 1756/8166 (22%) | 497/6428 (8%) |
| `wonder_step` | 240/751 (32%) | 4/10 (40%) | 21/33 (64%) | 10/18 (56%) | 103/1103 (9%) | 35/73 (48%) |
| `upgrade:urban` | 200/1915 (10%) | 92/266 (35%) | 79/410 (19%) | 110/418 (26%) | 311/951 (33%) | 222/878 (25%) |
| `take:temple` | 110/235 (47%) | 35/332 (11%) | 89/480 (19%) | 78/486 (16%) | 146/584 (25%) | 110/649 (17%) |
| `develop:government` | 9/82 (11%) | 18/71 (25%) | 18/64 (28%) | 41/92 (45%) | 34/316 (11%) | 49/360 (14%) |
| `play_leader` | 160/1388 (12%) | 83/267 (31%) | 148/656 (23%) | 133/551 (24%) | 364/2470 (15%) | 227/1344 (17%) |
| `take:library` | 173/317 (55%) | 120/433 (28%) | 105/900 (12%) | 148/662 (22%) | 235/297 (79%) | 187/943 (20%) |
| `take:action` | 646/2637 (24%) | 349/2124 (16%) | 487/3215 (15%) | 418/3052 (14%) | 1199/5747 (21%) | 736/5180 (14%) |
| `pop_free` | — | — | — | — | 1/7 (14%) | — |
| `develop:special-tech` | 129/208 (62%) | 6/19 (32%) | 13/90 (14%) | 13/75 (17%) | 250/405 (62%) | 37/132 (28%) |
| `develop:farm` | 3/7 (43%) | 10/42 (24%) | 11/60 (18%) | 12/78 (15%) | 5/12 (42%) | 39/170 (23%) |
| `take:theater` | 191/266 (72%) | 85/481 (18%) | 142/728 (20%) | 127/757 (17%) | 236/427 (55%) | 165/1020 (16%) |
| `end_turn` | 1522/7172 (21%) | 1718/4153 (41%) | 2772/6284 (44%) | 2775/6208 (45%) | 3143/16286 (19%) | 4890/10446 (47%) |
| `offer_pact` | — | — | 288/870 (33%) | 203/779 (26%) | 458/700 (65%) | 679/1687 (40%) |
| `pol_pass` | 766/1412 (54%) | 522/1580 (33%) | 740/2601 (28%) | 826/2603 (32%) | 2318/2978 (78%) | 1908/4670 (41%) |
| `revolution` | 83/190 (44%) | 52/84 (62%) | 74/114 (65%) | 49/77 (64%) | 125/422 (30%) | 92/215 (43%) |
| `bid_pass` | 15/25 (60%) | 30/95 (32%) | 148/418 (35%) | 82/236 (35%) | 31/90 (34%) | 189/445 (42%) |
| `bid` | 10/24 (42%) | 65/77 (84%) | 270/288 (94%) | 154/169 (91%) | 59/61 (97%) | 256/276 (93%) |
| `churchill` | 7/11 (64%) | 1/2 (50%) | 6/6 (100%) | 10/12 (83%) | 36/78 (46%) | 24/27 (89%) |
| `build:urban` | 404/622 (65%) | 247/333 (74%) | 369/668 (55%) | 397/647 (61%) | 781/844 (93%) | 549/883 (62%) |
| `choice:pact_offer` | — | — | 288/288 (100%) | 203/203 (100%) | 458/458 (100%) | 679/679 (100%) |
| `choice:lose_pop` | 31/31 (100%) | 65/65 (100%) | 84/84 (100%) | 77/77 (100%) | 3/3 (100%) | 181/181 (100%) |
| `choice:discard_military` | 27/27 (100%) | 51/51 (100%) | 95/95 (100%) | 73/73 (100%) | 3/3 (100%) | 108/108 (100%) |
| `choice:gain_block` | 30/30 (100%) | 30/30 (100%) | 60/60 (100%) | 60/60 (100%) | 50/50 (100%) | 92/92 (100%) |
| `choice:raid` | 7/7 (100%) | 12/12 (100%) | 31/31 (100%) | 52/52 (100%) | 6/6 (100%) | 120/120 (100%) |
| `choice:take_row` | 15/15 (100%) | 51/51 (100%) | 51/51 (100%) | 48/48 (100%) | 1/1 (100%) | 60/60 (100%) |
| `choice:food_or_res` | 28/28 (100%) | 8/8 (100%) | 14/14 (100%) | 12/12 (100%) | 121/121 (100%) | 33/33 (100%) |
| `choice:free_civil` | 16/16 (100%) | 18/18 (100%) | 4/4 (100%) | 11/11 (100%) | 130/130 (100%) | 30/30 (100%) |
| `choice:destroy_own` | 6/6 (100%) | 18/18 (100%) | 25/25 (100%) | 28/28 (100%) | 2/2 (100%) | 34/34 (100%) |
| `choice:free_build` | 11/11 (100%) | 6/6 (100%) | 4/4 (100%) | 4/4 (100%) | 57/57 (100%) | 11/11 (100%) |
| `choice:lose_colony` | — | — | 3/3 (100%) | 1/1 (100%) | — | 3/3 (100%) |

Caveats on reading this:

* `choice:*` rows are decisions the engine **forces** — some option must be
  picked — so a 100% take-rate there means "this decision happened N times",
  not "the bot liked it". They are in the table as an exposure count.
* Move kinds absent from a column never became legal in those 40 games
  (`offer_pact` / `cancel_pact` / `choice:pact_offer` at 2p is the rulebook:
  no pacts in a two-player game, RULES_SPEC 13).
* Take-rate is per *decision*, and `end_turn` is a candidate at essentially
  every action-phase decision, so low take-rates are normal. What is
  diagnostic is a hard zero over thousands of legal decisions, and a large
  gap between the champion column and the default column for the same arm.

### Engine-level outcomes over the same 40 games

| | 2p champ | 2p def | 3p champ | 3p def | 4p champ | 4p def |
|---|---|---|---|---|---|---|
| colonies held at end (all seats) | 7 | 40 | 113 | 75 | 13 | 114 |
| wonders completed | 53 | 1 | 8 | 4 | 13 | 12 |
| wonders **unfinished** at end | 12 | 0 | 1 | 0 | 86 | 0 |
| pacts in play at end | 0 | 0 | 58 | 48 | 50 | 73 |

## 2. Bugs found

All three are in my half of the audit (nothing here touches wars, aggressions,
pacts or defence strength). All three are **fixed** on this branch, each with a
test that fails without the fix.

### 2.1 Revolution threw away every action the new government granted

`engine/actions.py:833` (master), `_h_revolution`:

```python
p.military_actions = min(p.military_actions, s.military_actions)
```

That is a **cap**, and the rule is an **update**. RB p.13, quoted verbatim in
`sources/ubg_the-second-round.txt:271` and RULES_SPEC 8.3.4:

> Your military actions are not affected. You may spend any of them before or
> after the revolution, and any that you gain from the new government will be
> available to spend.

Measured on a constructed position: revolt from Despotism (2 MA) to Monarchy
(3 MA) with nothing spent, and you end the turn with **2** military actions,
not 3. The peaceful change of the *same* government on the *same* position
correctly gives 3 — `_set_government` (`actions.py:804`) already does the right
arithmetic, so the two paths disagreed with each other.

The Robespierre branch (`actions.py:829`) is the mirror image and had the
mirror bug: he pays with military actions, so it is the *civil* actions that
must carry over and pick up the new government's extras, and they did not
(revolt to Monarchy with Robespierre: 4 civil actions instead of 5).

Fixed by computing what was already spent *before* the government changes and
subtracting that from the *new* total, exactly as the peaceful path does.

Tests: `TestRevolution.test_revolution_grants_the_new_governments_military_actions`,
`...test_revolution_keeps_military_actions_already_spent`,
`...test_robespierre_revolution_grants_the_new_civil_actions`, plus
`...test_peaceful_change_already_grants_them` as the contrast that makes it
unambiguous.

Impact: revolution is *not* an underused mechanic (30–65% take-rate when
legal), so this was silently making a well-used action worse than the rules
say for the whole training run. Every arm's champion was trained against it.

### 2.2 The one-per-name rule was applied to action cards

`engine/actions.py:148` (master), `_can_take_gated`:

```python
if name in p.hand_civil or name in p.techs or name == p.government:
    return False
```

RULES_SPEC 2.5, and `sources/ubg_the-second-round.txt:83`:

> You may never take a **technology** card with the same name as a technology
> you already have in your hand or in play.

A technology is a civil card with a science cost (RULES_SPEC 7.1). Yellow
action cards have none. Seven of them exist in two or three copies in the same
deck (Rich Land, Urban Growth, Frugality, Breakthrough, Reserves, Efficient
Upgrade, Revolutionary Idea), and holding one blocked taking the other.

Fixed; the name test now applies to everything except `type == "action"`.
Taking that fix alone would have introduced a second bug — `taken_this_turn`
is a list of *names*, so a second copy taken this turn would have locked up the
copy already in hand — so the "not in the phase it was taken" gate
(`actions.py:463`) is now a **count** comparison rather than a membership test.

Test: `TestOnePerName.*` (four cases, including the three that must still be
blocked: a technology in hand, a technology in play, the current government).

### 2.3 Not a bug, checked and cleared

Recorded so nobody re-derives them:

* `lose_colony` (`interact.py:590`) removes fewer yellow tokens than the colony
  granted if the owner already spent them. That is **correct**: RULES_SPEC 6.5,
  "losing yellow tokens beyond what the bank holds: lose only what is there".
* `reveal_current_event` recycles the future-events deck *before* popping, which
  in principle could reveal a card the current player just prepared, violating
  RULES_SPEC 5.2 ("an event you prepare is always revealed on a LATER turn").
  It is unreachable: preparing adds exactly one card and revealing removes
  exactly one, so `current + future` is invariant at `players + 2` from setup
  (`game.py:91`) and the leading branch never fires.
* `p.destroyed_wonders` is never incremented anywhere. It is not needed:
  Ravages of Time *flips* a completed wonder, which stays in `completed_wonders`,
  so the take surcharge (RULES_SPEC 2.4) already counts it. Dead field, not a
  dead rule.
* All 33 action cards are mechanically playable — none is stranded in hand by
  an effect the engine cannot express (`ACTION_CARD_KEYS` covers every one).
* All 15 Age III "Impact of …" events carry an `allPlayers` block, so
  `evaluate_final_events` (`events.py:450`) scores every one of them; none is
  silently skipped.

## 3. Colonies, explicitly

The user named this one. Verdict up front: **the engine is right; the mechanic
is not search-blind either — it is starved of opportunities at 2p, and its
dedicated feature is a dead coordinate.**

### 3.1 Conformance, rule by rule

| RULES_SPEC | code | agree? |
|---|---|---|
| 11.1 a Territory revealed as the current event starts an auction | `events.py:138` → `interact.start_auction` | yes |
| 11.2 bidding starts with the player resolving their politics phase, clockwise | `start_auction(state, name, state.current)`, `_order_from` | yes |
| 11.2 bid > 0 and > previous, capped by the force you can actually send | `interact.py:52`, `max_force` | yes |
| 11.2 pass = out permanently; last remaining bidder wins and must colonise | `_auction_move`, `interact.py:524` | yes |
| 11.2 no bids → territory to past events | `interact.py:535` | yes |
| 11.3 force = printed unit strength + armies formed **by the sacrificed units** + ship icons in play + bonus cards | `force_value`, `interact.py:488` | yes |
| 11.3 ≥ 1 unit mandatory | `_build_force`, `interact.py:565` | yes |
| 11.3 strength-*rating* modifiers excluded | uses `army_strength_units`, never `s.strength` | yes (already tested) |
| 11.4 sacrificed tokens go to the **yellow bank**, not the worker pool | `interact.py:552` | yes (already tested) |
| 11.5 permanent effects first, then the immediate one | `gain_colony`, `interact.py:577` | yes |
| 11.5 permanents include rating symbols, not just tokens | `_colony_permanents` + `COLONY_PERMANENT_KEYS`, `effects.py:397` | yes — Strategic Territory's +2/+4 strength and Historic Territory's happy faces do apply |
| 11.5 stealing a colony moves the permanents and never the one-time effect | `_c_annex`, `interact.py:192` | yes |
| 11.6 bonus cards discarded **before** a Strategic Territory draws | `colonize` discards, then `gain_colony` draws | yes |
| immediate `drawMilitaryCards` is nothing in Age IV | `_draw_military`, `events.py:117` | yes |
| 5.2 preparing a territory scores culture equal to its level | `_h_prepare_event`, `actions.py:965` | yes |

Six of these had no test; they do now (`TestColonyEffects`,
`TestColonyForceRules`). One soft spot, not a rules violation: **which** units
the winner sacrifices is chosen by a fixed heuristic (`_build_force`, cheapest
unit first, bonus cards before extra units) rather than being a decision the
bot makes. Any force ≥ the bid is legal, so this conforms; it does mean the bot
cannot trade off "lose a Warriors" against "lose a Knights", and that sub-choice
is invisible to the search.

### 3.2 Is it deferred-credit'ed? Yes, already

The brief's suspicion was that colonies have the deferred-payoff /
committed-cost shape that `deferred_credit()` handles only for `pact_offer` and
`auction`. **The auction *is* the colony**: `weighted.py:160` already credits a
live high bid at `1/(1 + rivals still in)` of the territory's own permanent and
immediate effects, priced through the ordinary economy features, and charges
`auction_bid` for the sacrifice. Nothing further is needed there. The one thing
`deferred_credit` does *not* price is which specific units will be spent
(§3.1 above) — `auction_bid` is a scalar proxy for that.

### 3.3 Why 2p sees almost no colonies

The 2p champion holds `colonies` = **0.000** and the prior weight-credit report
scored it at mean_edge −0.0007. Both are explained, and neither is "colonies
are worthless":

1. **The feature is a dead coordinate.** Measured over 2347 decisions of 2p
   champion self-play, `colonies` varies across the candidates of a decision
   in **0.2%** of them (4p: 0.0% over 5491). A term that is identical across
   the candidate set cancels out of the argmax exactly, so its weight has
   almost no gradient — it drifts, and the two-sided guard eventually pins it
   at 0. This is the `unit_workers` = 0.000 trap, and it is not the mechanic's
   fault: the *decision* about a colony is the `bid`, and that is priced by
   `auction_committed` / `auction_bid` / the deferred yields, which do work
   (3p bid take-rate 94%, 4p 97%).
2. **At 2p the auctions mostly have no bidders at all.** The 2p champion
   prepared **90** territories across 40 games and there were only **25
   auction decision points** in total (`bid_pass` is legal at every one of
   them, and it was legal 25 times). `start_auction` (`interact.py:514`) only
   admits players with `max_force > 0`, i.e. with at least one military unit on
   the board — and the 2p champion *disbands* units (`destroy:unit` take-rate
   20%, 230 of 1156). So most territories it prepares are auctioned to an empty
   room and go straight to past events. The engine's own (truncated, so
   lower-bound) log agrees: 70 "nobody can colonize", 13 "no bids", 7
   colonised — 90 in total, which is exactly the number prepared.

So at 2p the mechanic is real, implemented correctly, and its own bot has
built a position in which it cannot be used. The 3p champion, which weights
`colonies` at 2.76 and keeps units, ends with 113 colonies over 40 games
against the default bot's 75.

**Verdict: engine-correct. `colonies` the feature is search-blind (dead
coordinate). The mechanic itself is reachable and used wherever units exist.**

## 4. The other underused mechanics

Ranked by how legal-but-untaken they are. "Genuinely correctly ignored" is a
real verdict and is used where it is the honest one.

| mechanic | measurement | verdict |
|---|---|---|
| declaring a **war** | 0 taken out of 377 / 412 / 736 / 591 / 606 / 1513 legal decisions — a hard zero in all six cells | handed to `COMBAT_AUDIT.md` |
| **aggressions** | 1–6 taken out of 300–1892 | handed to `COMBAT_AUDIT.md` |
| **cancel a pact** | 0/1276 and 0/993 at 3p; 10/761 at 4p | handed to `COMBAT_AUDIT.md` |
| **resign** | 4p champion took it 14 times in 40 census games; in a 12-game probe 7 games contained a resignation and the resigning seat won **0 of 9**, scoring 0–25 against winners of 23–90 | **search-blind, actively harmful** — §4.1 |
| **wonders** (finishing one) | 4p: 86 unfinished vs 13 completed over 40 games; `wonder_step` taken 103/1103. The bot passes over the best available wonder stage by a mean of **51.8 eval points** (4p, 221 samples); at 2p, where it does finish them, the gap is 22.0 and the take-rate is 26–32% | **search-blind** — §4.2 |
| **building a farm** | 2p: **0 of 767** legal decisions, while `build:mine` on the *same* 767 decisions is 186. In 230 decisions where both were legal, the best mine outscored the best farm **230 times out of 230**, by a constant mean of 0.63 eval points | **search-blind** (linear tie-break degeneracy) — §4.3 |
| **military unit technologies from the row** | `take:infantry` / `cavalry` / `artillery` 0.2–1% take-rate everywhere; `take:air` 0/428 and 0/483 | engine correct; search-blind, and overlaps `COMBAT_AUDIT.md` |
| **arenas** | `take:arena` 0/718 for the 2p champion, 3–5% elsewhere | champion-specific blind spot; `best_arena` `varying` 0.002 |
| **labs** (2p champion only) | `take:lab` 4/1251 for the 2p champion against 100/613 for the 2p default bot — and `best_lab`, weighted **2.636**, measures `varying` = **0.000**: it never takes a lab, so the feature never moves, so the weight is free to be anything | **search-blind, self-reinforcing** |
| **colonies** | see §3 | engine-correct; the `colonies` feature is a dead coordinate; the mechanic is starved at 2p |
| **preparing events at 4p** | `prepare_event:event` 135/2007 (7%) for the 4p champion against 1660/2032 (82%) for the 4p default bot; territories 37/797 vs 412/799. `pol_pass` 78% | champion-specific; note this also starves 4p colonisation |
| **destroy / disband** | ~0–1% everywhere except the 4p champion (931 units + 373 farms). Not uprising management: only 4% of those destroys happened with an uprising pending | **explained by `blue_free`** — §4.4 |
| **revolutions** | 30–65% take-rate when legal in every arm | well used — but the engine was **wrong** about it (§2.1) |
| **peaceful government change** | 11–45% when legal | fine |
| **action cards** | taken from the row 14–24%, played 6–11%, all 33 mechanically playable | fine — one taking bug, fixed (§2.2) |
| **tactics** | `play_tactic` 4–18%, `copy_tactic` 8–22% | fine |
| **leaders** | played 12–31% when legal | fine; the `leader` indicator feature is near-dead (`varying` 0.08–0.12) but a leader's value shows up through its effects, so that is correct |
| **Age III final-scoring events** | all 15 carry an `allPlayers` block and are scored by `evaluate_final_events` | fine |
| **`pop_free` (Ocean Liners)** | legal 7 times in 240 player-games | **genuinely correctly ignored** — it needs one specific wonder |
| **`churchill`** | 46–100% when legal, 2–78 legal decisions | **genuinely correctly ignored when absent**; used when present |
| **upgrading units** | 1–4 legal decisions per 40 games | **genuinely correctly rare** — upgrading needs both levels in play |

### 4.1 `resign`

Engine: correct (RULES_SPEC 5.11, already tested). The bot's problem is that
**nothing in the feature vector reads `p.resigned`**. A resigned player's
culture is frozen and `game.winners()` scores them −1, so resigning is a
guaranteed loss of the game; the evaluator sees only that the hand emptied and
the turn passed.

Decomposed against `pol_pass` (the alternative it beat) over 8 games, 4
samples: mean advantage of resigning **+0.065 eval points** on a total of ~200.
It is a numerical tie, and the largest single term is `food_rate` (+0.236),
which moves because resigning drops the live player count from 4 to 3 and
`lateness()` is player-count dependent (`_L_ZERO`, `CARDS_PER_ROUND`,
`_tail`) — so the phase multipliers, including `food_rate_early` = 6.381, are
rescaled. The bot is throwing away the game over a rounding artifact in the
game-horizon estimate.

Four samples is too few to put an error bar on the margin, and I say so; the
*structural* claim (no feature reads `resigned`) needs no sample at all, and
the outcome claim (0 wins from 9 resignations) is exact.

Not fixed: the fix is a bot policy change (drop `resign` from the candidate
set, or give it a large negative bias) and needs an n ≥ 200 A/B I did not run.

### 4.2 Wonders, and 4.4 `blue_free`, are the same finding

The 4p champion holds `blue_free` = **6.627** against a default of 0.15 — 44x —
and it is that arm's dominant coordinate: zeroing it changes the chosen move in
**58.2%** of decisions (5491 decisions). `blue_free` is
`blue_total − blue_used`, and `blue_used` counts the tokens holding your stored
food and resources **and the stages already built on your unfinished wonder**
(`effects.blue_used`, `effects.py:761`).

So at 6.627 per token the 4p champion is paid for having an empty warehouse.
Decomposing the moves it chooses against `end_turn`:

* **destroying a worker**: mean advantage +21.9, of which `blue_free` is
  **+25.8** and `workers` is −6.9. It is not choosing to destroy; it is
  refusing to end the turn, because ending the turn runs the production phase
  and production converts blue-bank tokens into stored goods. 201 of 299 were
  Warriors, 78 Agriculture.
* **taking a wonder**: mean advantage +25.5, of which `blue_free` is **+24.9**.
  Same mechanism. Taking a wonder is a cheap way to burn a civil action;
  `wonder_remaining` is weighted **0.000**, so carrying a dead wonder is free —
  even though it blocks every future wonder (RULES_SPEC 9.2) and nothing in the
  feature vector represents that.
* **building a wonder stage** is then doubly penalised: the stage's resources
  cost `resource_stock` 1.385 each *and* the blue token covering the stage
  costs `blue_free` 6.627, against `wonder_progress` 0.068 and
  `wonder_progress_late` −1.523. The payoff (`wonders` +3.799 plus the card's
  effects) arrives only on the last stage, which a 1-ply search cannot see from
  the first. Result: 259 wonders taken, 13 finished, 86 abandoned.

The 2p champion is the control: `wonder_progress` 2.299 against
`resource_stock` 1.331 makes a stage net positive, and it completes 53 wonders
in 40 games with a 32% `wonder_step` take-rate.

**Verdict: engine-correct, search-blind.** A multi-stage investment whose whole
payoff lands on the last step is invisible to a 1-ply linear evaluator unless
`wonder_progress` happens to be tuned above `resource_stock`, and whether it is
is an accident of each arm's trajectory. Hand-in-hand with `docs/DEEPER_SEARCH.md`.

### 4.3 Farms

`build:farm` is taken 0 times in 767 legal decisions at 2p, and `build:mine`
186 times on the same 767. Measured directly: in 230 decisions where both were
legal, the best farm scored below the best mine **230 times out of 230**, mean
difference −0.627.

This is not a bug and not a tie-break accident — it is exactly what a linear
evaluator does. A farm and a mine cost the same civil action and a similar
number of resources; the farm adds `food_rate` (2.010) and the mine adds
`resource_rate` (2.518). 2.518 > 2.010, unconditionally and in every position,
so the mine wins every time, forever. Nothing in a linear evaluation can
express "I have enough resources and not enough food". The 4p champion, whose
`food_rate` is 0.063 and `food_rate_early` 6.381, flips between them (farm won
420 of 1024) — which is the same degeneracy pointing the other way at a
different phase of the game.

**Verdict: engine-correct, search-blind** (the feature basis cannot express
diminishing returns, so the evaluator cannot diversify).

## 5. The dead-coordinate census

`tools/feature_variance.py`, 12 games per cell. Read `varying` first: it is
weight-independent and answers "can this coordinate ever matter". `flip` is
conditional on the vector supplied — a weight already at 0 trivially never
flips — so `flip` answers "does it matter *to this champion*".

### 5.1 Provably inert, in every arm, at any weight

`rival_culture_rate`, `rival_science_rate`, `rival_strength` measured
`varying` = **0.000** in every run (2347 / 1711 / 2842 / 5491 decisions at 2p
champ, 2p default, 3p champ, 4p champ).

This is structural, not a sampling accident. `WeightedBot.pick`
(`weighted.py:792`) computes `rival_context` **once at the root, on the unmoved
board**, and passes the same dict to every candidate — a deliberate ~30x saving
documented at `weighted.py:176`. `features()` copies those three numbers
straight out of `ctx`. Every candidate therefore gets the same value, the term
cancels out of the argmax, and the weight cannot change a single move. The 4p
champion carries `rival_strength` = −0.632 and `rival_science_rate` = −0.545;
both are noise.

(`rival_culture` and `rival_mean_culture` are *not* in this class — they read
`q.culture` off the trial state, so an aggression or war that moves a rival's
score does show up. Measured `varying` 0.09 at 2p, 0.003 at 4p.)

Pinned by `tests/test_coverage_tools.py::TestInertFeatures`, which fails the
moment somebody makes them live — so the fix, if it is ever made, cannot be
made silently.

### 5.2 The full tables

Decisions per cell: 2p champ 2347, 2p def 1711, 3p champ 2842, 4p champ 5491, 4p def 4817.

| feature | 2p champ w / varying / flip | 2p def w / varying / flip | 3p champ w / varying / flip | 4p champ w / varying / flip | 4p def w / varying / flip |
|---|---|---|---|---|---|
| `rival_culture_rate` | -0.002 / 0.000 / 0.0000 | -1.000 / 0.000 / 0.0000 | -1.000 / 0.000 / 0.0000 | 0.000 / 0.000 / 0.0000 | -1.000 / 0.000 / 0.0000 |
| `rival_science_rate` | 0.000 / 0.000 / 0.0000 | -0.600 / 0.000 / 0.0000 | -0.600 / 0.000 / 0.0000 | -0.545 / 0.000 / 0.0000 | -0.600 / 0.000 / 0.0004 |
| `rival_strength` | -0.127 / 0.000 / 0.0000 | -0.150 / 0.000 / 0.0000 | -0.150 / 0.000 / 0.0003 | -0.632 / 0.000 / 0.0000 | -0.150 / 0.000 / 0.0004 |
| `hand_potential` | 0.052 / 0.000 / 0.2075 | 0.125 / 0.000 / 0.1730 | 0.125 / 0.000 / 0.1263 | 0.375 / 0.000 / 0.3120 | 0.125 / 0.000 / 0.1283 |
| `end_turn_bias` | -14.439 / 0.000 / 0.5386 | -3.000 / 0.000 / 0.1707 | -3.546 / 0.000 / 0.1918 | -0.498 / 0.000 / 0.0064 | -3.000 / 0.000 / 0.1532 |
| `best_unit` | 0.742 / 0.002 / 0.0000 | 0.500 / 0.003 / 0.0012 | 0.500 / 0.000 / 0.0000 | 1.102 / 0.002 / 0.0002 | 0.500 / 0.004 / 0.0004 |
| `colonies` | 0.000 / 0.002 / 0.0000 | 2.000 / 0.010 / 0.0023 | 2.760 / 0.007 / 0.0014 | 0.650 / 0.000 / 0.0000 | 2.000 / 0.004 / 0.0010 |
| `best_farm` | 0.032 / 0.000 / 0.0000 | 0.500 / 0.013 / 0.0006 | 0.500 / 0.008 / 0.0014 | 0.564 / 0.000 / 0.0000 | 0.500 / 0.004 / 0.0006 |
| `best_mine` | 0.045 / 0.003 / 0.0000 | 0.500 / 0.005 / 0.0000 | 0.500 / 0.019 / 0.0000 | 1.896 / 0.002 / 0.0000 | 0.500 / 0.016 / 0.0013 |
| `special_techs` | 0.022 / 0.019 / 0.0000 | 0.800 / 0.011 / 0.0018 | 0.800 / 0.014 / 0.0007 | 1.101 / 0.019 / 0.0004 | 0.800 / 0.010 / 0.0019 |
| `gov_level` | 0.220 / 0.019 / 0.0000 | 2.000 / 0.024 / 0.0018 | 2.000 / 0.016 / 0.0011 | 0.998 / 0.031 / 0.0006 | 2.000 / 0.010 / 0.0008 |
| `auction_bid` | -0.081 / 0.001 / 0.0000 | -0.400 / 0.004 / 0.0000 | -0.400 / 0.033 / 0.0000 | -0.060 / 0.002 / 0.0000 | -0.400 / 0.016 / 0.0000 |
| `auction_committed` | 1.590 / 0.001 / 0.0000 | 2.000 / 0.004 / 0.0012 | 2.000 / 0.033 / 0.0081 | 0.000 / 0.002 / 0.0000 | 2.000 / 0.016 / 0.0066 |
| `civil_actions` | 0.000 / 0.023 / 0.0000 | 2.000 / 0.018 / 0.0006 | 2.590 / 0.024 / 0.0039 | 0.071 / 0.038 / 0.0000 | 2.000 / 0.026 / 0.0027 |
| `best_arena` | 0.381 / 0.002 / 0.0000 | 0.300 / 0.018 / 0.0012 | 0.300 / 0.051 / 0.0014 | 0.127 / 0.002 / 0.0000 | 0.300 / 0.047 / 0.0006 |
| `wonders` | 4.240 / 0.074 / 0.0000 | 3.000 / 0.000 / 0.0000 | 3.000 / 0.001 / 0.0003 | 3.799 / 0.001 / 0.0000 | 3.000 / 0.001 / 0.0006 |
| `best_temple` | 0.855 / 0.017 / 0.0017 | 0.600 / 0.051 / 0.0023 | 0.600 / 0.087 / 0.0032 | 0.751 / 0.015 / 0.0000 | 0.600 / 0.058 / 0.0039 |
| `uprising` | 0.000 / 0.039 / 0.0000 | -12.000 / 0.005 / 0.0006 | -12.000 / 0.007 / 0.0007 | -41.784 / 0.089 / 0.0324 | -12.000 / 0.011 / 0.0033 |
| `rival_culture` | 0.000 / 0.059 / 0.0000 | -0.350 / 0.090 / 0.0006 | -0.350 / 0.049 / 0.0011 | 0.000 / 0.003 / 0.0000 | -0.350 / 0.030 / 0.0010 |
| `rival_mean_culture` | -0.224 / 0.059 / 0.0017 | -0.100 / 0.090 / 0.0000 | -0.216 / 0.057 / 0.0003 | -1.397 / 0.003 / 0.0000 | -0.100 / 0.044 / 0.0004 |
| `pact_blocks_attack` | 0.000 / 0.000 / 0.0000 | 0.500 / 0.000 / 0.0000 | 0.500 / 0.067 / 0.0011 | 1.029 / 0.038 / 0.0000 | 0.500 / 0.109 / 0.0000 |
| `wonder_progress` | 2.299 / 0.121 / 0.0869 | 1.000 / 0.000 / 0.0000 | 1.000 / 0.006 / 0.0007 | 0.068 / 0.092 / 0.0026 | 1.000 / 0.006 / 0.0013 |
| `leader` | 0.000 / 0.123 / 0.0000 | 1.500 / 0.077 / 0.0140 | 1.500 / 0.082 / 0.0127 | 0.004 / 0.101 / 0.0002 | 1.500 / 0.065 / 0.0093 |
| `military_actions` | 0.936 / 0.127 / 0.0149 | 0.700 / 0.045 / 0.0023 | 0.665 / 0.098 / 0.0060 | 3.794 / 0.141 / 0.0115 | 0.700 / 0.060 / 0.0023 |
| `consumption` | -0.036 / 0.135 / 0.0008 | -0.500 / 0.183 / 0.0047 | -0.500 / 0.173 / 0.0046 | 0.000 / 0.090 / 0.0000 | -0.500 / 0.168 / 0.0027 |
| `pop_cost` | -0.906 / 0.135 / 0.0328 | -0.400 / 0.183 / 0.0035 | -0.400 / 0.174 / 0.0035 | -0.083 / 0.090 / 0.0009 | -0.400 / 0.169 / 0.0027 |
| `best_lab` | 2.636 / 0.000 / 0.0000 | 0.800 / 0.168 / 0.0064 | 0.800 / 0.004 / 0.0000 | 0.001 / 0.184 / 0.0000 | 0.800 / 0.143 / 0.0033 |
| `best_theater` | 0.005 / 0.102 / 0.0000 | 0.600 / 0.195 / 0.0018 | 0.606 / 0.186 / 0.0011 | 2.630 / 0.103 / 0.0049 | 0.600 / 0.201 / 0.0014 |
| `pacts` | 0.000 / 0.000 / 0.0000 | 0.500 / 0.000 / 0.0000 | 0.500 / 0.205 / 0.0028 | 0.118 / 0.098 / 0.0002 | 0.500 / 0.217 / 0.0014 |
| `strength_lead` | 0.012 / 0.263 / 0.0000 | 0.300 / 0.211 / 0.0053 | 0.130 / 0.212 / 0.0039 | 0.065 / 0.241 / 0.0002 | 0.300 / 0.135 / 0.0037 |
| `best_library` | 9.939 / 0.026 / 0.0072 | 0.500 / 0.279 / 0.0018 | 0.500 / 0.128 / 0.0018 | 0.000 / 0.191 / 0.0000 | 0.500 / 0.209 / 0.0017 |
| `tactic_level` | 0.133 / 0.326 / 0.0230 | 0.500 / 0.240 / 0.0099 | 0.500 / 0.226 / 0.0067 | 1.058 / 0.249 / 0.0239 | 0.500 / 0.251 / 0.0087 |
| `num_techs` | 0.000 / 0.142 / 0.0000 | 0.300 / 0.393 / 0.0058 | 0.300 / 0.304 / 0.0042 | 0.111 / 0.315 / 0.0007 | 0.300 / 0.380 / 0.0069 |
| `tech_levels` | 3.426 / 0.157 / 0.0405 | 1.000 / 0.397 / 0.0152 | 1.000 / 0.311 / 0.0169 | 18.752 / 0.328 / 0.0403 | 1.000 / 0.383 / 0.0179 |
| `discontent` | -0.537 / 0.122 / 0.0026 | -3.000 / 0.067 / 0.0012 | -3.041 / 0.056 / 0.0003 | -13.137 / 0.410 / 0.0430 | -3.000 / 0.074 / 0.0010 |
| `wonder_remaining` | -0.025 / 0.380 / 0.0111 | -0.300 / 0.436 / 0.0333 | -0.300 / 0.360 / 0.0169 | 0.000 / 0.308 / 0.0000 | -0.300 / 0.383 / 0.0278 |
| `yellow_bank` | -1.435 / 0.239 / 0.0396 | -0.100 / 0.430 / 0.0012 | -0.100 / 0.481 / 0.0035 | 0.000 / 0.229 / 0.0000 | -0.100 / 0.428 / 0.0025 |
| `strength_deficit` | -0.046 / 0.150 / 0.0004 | -0.600 / 0.159 / 0.0088 | -0.600 / 0.360 / 0.0158 | -3.566 / 0.518 / 0.0457 | -0.600 / 0.322 / 0.0166 |
| `happy_margin` | 0.721 / 0.553 / 0.0349 | 1.200 / 0.537 / 0.0316 | 1.200 / 0.483 / 0.0289 | 0.412 / 0.463 / 0.0038 | 1.200 / 0.488 / 0.0218 |
| `corruption_loss` | 0.000 / 0.590 / 0.0000 | -0.900 / 0.481 / 0.0865 | -0.900 / 0.468 / 0.0940 | -0.414 / 0.547 / 0.0062 | -0.900 / 0.426 / 0.0789 |
| `unit_workers` | 0.025 / 0.279 / 0.0004 | 0.100 / 0.296 / 0.0047 | 0.100 / 0.381 / 0.0039 | 0.046 / 0.594 / 0.0006 | 0.100 / 0.267 / 0.0033 |
| `culture_rate` | 32.246 / 0.588 / 0.1167 | 5.000 / 0.516 / 0.0497 | 5.000 / 0.593 / 0.0774 | 35.574 / 0.444 / 0.0614 | 5.000 / 0.606 / 0.0781 |
| `resource_rate` | 2.518 / 0.604 / 0.0473 | 1.600 / 0.584 / 0.0579 | 1.600 / 0.573 / 0.0482 | 2.517 / 0.618 / 0.0330 | 1.600 / 0.604 / 0.0718 |
| `food_rate` | 2.010 / 0.540 / 0.0081 | 1.200 / 0.484 / 0.0105 | 1.200 / 0.532 / 0.0123 | 0.063 / 0.625 / 0.0370 | 1.200 / 0.471 / 0.0108 |
| `prod_workers` | 0.286 / 0.604 / 0.0013 | 0.300 / 0.583 / 0.0035 | 0.307 / 0.538 / 0.0042 | 1.241 / 0.626 / 0.0129 | 0.300 / 0.534 / 0.0050 |
| `urban_workers` | 0.002 / 0.613 / 0.0000 | 0.500 / 0.583 / 0.0029 | 0.500 / 0.537 / 0.0032 | 1.592 / 0.630 / 0.0000 | 0.500 / 0.536 / 0.0019 |
| `science_rate` | 0.213 / 0.546 / 0.0064 | 4.000 / 0.577 / 0.0526 | 0.000 / 0.323 / 0.0049 | 39.223 / 0.632 / 0.0493 | 4.000 / 0.490 / 0.0394 |
| `strength` | 0.031 / 0.376 / 0.0004 | 0.350 / 0.337 / 0.0140 | 0.350 / 0.512 / 0.0165 | 0.582 / 0.645 / 0.0117 | 0.350 / 0.387 / 0.0143 |
| `strength_rel` | 0.000 / 0.376 / 0.0349 | 0.350 / 0.337 / 0.0181 | 0.350 / 0.512 / 0.0331 | 0.833 / 0.645 / 0.0315 | 0.350 / 0.387 / 0.0305 |
| `food_stock` | 0.003 / 0.386 / 0.0004 | 0.200 / 0.514 / 0.0327 | 0.200 / 0.589 / 0.0331 | 0.140 / 0.657 / 0.0049 | 0.200 / 0.513 / 0.0301 |
| `workers` | 1.641 / 0.695 / 0.0366 | 1.400 / 0.636 / 0.0573 | 2.334 / 0.606 / 0.0897 | 6.730 / 0.758 / 0.0821 | 1.400 / 0.576 / 0.0428 |
| `free_workers` | 0.030 / 0.697 / 0.0004 | 0.400 / 0.644 / 0.0245 | 0.400 / 0.611 / 0.0222 | 0.866 / 0.759 / 0.0133 | 0.400 / 0.584 / 0.0168 |
| `science` | 0.089 / 0.724 / 0.0285 | 0.500 / 0.681 / 0.1730 | 0.500 / 0.544 / 0.1017 | 0.340 / 0.795 / 0.0273 | 0.500 / 0.588 / 0.1547 |
| `ca_left` | 0.000 / 0.795 / 0.0000 | 0.050 / 0.673 / 0.0210 | 0.050 / 0.627 / 0.0239 | 0.169 / 0.791 / 0.0291 | 0.050 / 0.600 / 0.0241 |
| `hand_civil` | 0.243 / 0.765 / 0.0405 | 0.300 / 0.811 / 0.0304 | 0.300 / 0.765 / 0.0338 | 0.000 / 0.772 / 0.0000 | 0.300 / 0.793 / 0.0255 |
| `hand_value` | 0.270 / 0.765 / 0.0967 | 0.250 / 0.812 / 0.0649 | 0.218 / 0.765 / 0.0647 | 0.125 / 0.773 / 0.0519 | 0.250 / 0.793 / 0.0529 |
| `culture` | 1.000 / 0.832 / 0.1031 | 1.000 / 0.781 / 0.4278 | 1.000 / 0.718 / 0.3920 | 1.000 / 0.655 / 0.0472 | 1.000 / 0.628 / 0.3816 |
| `resource_stock` | 1.331 / 0.778 / 0.1542 | 0.300 / 0.604 / 0.0468 | 0.300 / 0.522 / 0.0528 | 1.385 / 0.838 / 0.0377 | 0.300 / 0.514 / 0.0575 |
| `ma_left` | 0.274 / 0.852 / 0.0712 | 0.050 / 0.758 / 0.0082 | 0.022 / 0.759 / 0.0021 | 0.005 / 0.847 / 0.0000 | 0.050 / 0.767 / 0.0071 |
| `blue_free` | 0.336 / 0.808 / 0.0622 | 0.150 / 0.695 / 0.0374 | 0.150 / 0.653 / 0.0405 | 6.627 / 0.884 / 0.5817 | 0.150 / 0.618 / 0.0280 |
| `hand_military` | 0.000 / 0.890 / 0.0000 | 0.300 / 0.886 / 0.0415 | 0.300 / 0.836 / 0.0447 | 0.262 / 0.826 / 0.0391 | 0.300 / 0.801 / 0.0394 |
| `hand_mil_value` | 0.069 / 0.908 / 0.0375 | 0.150 / 0.897 / 0.0585 | 0.079 / 0.836 / 0.0285 | 0.000 / 0.845 / 0.0000 | 0.150 / 0.807 / 0.0548 |

### 5.3 What the tables say

* **`end_turn_bias` is the dominant coordinate of the 2p champion.** Zeroing it
  changes the chosen move in **53.9%** of decisions (2p champion, −14.439). At
  its default −3.0 it is 17.1% (2p) and 19.2% (3p). The 4p champion has trained
  it almost away (−0.498, flip 0.6%) and has `blue_free` = 6.627 flipping 58.2%
  of its decisions instead. The three arms are not small variations on one
  strategy; they are three different evaluators.
* **A whole tier of features varies constantly and is weighted to nothing.**
  At 2p: `ca_left` varies in 79.5% of decisions with a mean spread of 3.0
  actions, weight 0.000. `hand_military` 89.0%, weight 0.000.
  `corruption_loss` 59.0%, weight 0.000. `urban_workers` 61.3%, weight 0.002.
  At 4p: `hand_civil` 77.2% and `hand_mil_value` 84.5%, both weight 0.000;
  `wonder_remaining` 30.8% with a mean spread of 2.6 resources, weight 0.000.
  These are live coordinates that the training has switched off.
* **A second tier barely varies at all, so its weight is meaningless whatever
  it says.** `uprising` varies in 0.5% (2p default) / 0.7% (3p) / 8.9% (4p) of
  decisions; `civil_actions` in 1.8% / 2.4% / 3.8%; `colonies` in 1.0% / 0.7% /
  0.0%; `best_lab` in 0.0% (2p champion) — that last one carries weight 2.636.
* **`wonders` = 3.799 (4p) and 3.000 (2p default) never flips a decision**
  (`varying` 0.001 and 0.000). Completing a wonder is a candidate in a fraction
  of a percent of decisions, and when it is, other terms decide.
* **The two biggest coordinates in the whole exercise are not strategy terms.**
  `end_turn_bias` is an acknowledged search hack; `hand_potential` is the
  card-identity patch. Both show `varying` = 0.000 in the table because neither
  is a linear feature — `end_turn_bias` is a constant added to one candidate and
  `hand_potential` is priced through `w` itself — so read only their `flip`
  column. `hand_potential` flips 12.6–31.2% of decisions across the four
  vectors. Between them they decide more moves than any feature except
  `culture` and, at 4p, `blue_free`.

## 6. The civil-actions inversion, specifically

The 2p champion prices civil actions at exactly nothing:
`uprising` 0.0 (default −12.0), `civil_actions` 0.0 (default +2.0),
`ca_left` 0.0 (default +0.05), `discontent` −0.537 (default −3.0). All four
zeros are the guard's clamp value.

The hypothesis to test was: this is the 1-ply evaluation asymmetry, and
`end_turn_bias` is the existing hack for it. **Measured: yes, for `ca_left`;
no, for `uprising` and `civil_actions`, which are simply dead.**

### 6.1 What the two features actually are

They are different things and only one of them is implicated.

* `civil_actions` = `effects.compute(p).civil_actions`, the **capacity** the
  government and cards grant. Not "remaining". It moves only when the
  government, leader, wonder or special tech in play changes.
* `ca_left` = `p.civil_actions`, the actions **remaining this turn**.

Both are read from the **post-move trial state** — `evaluate` is called after
`actions.apply`.

### 6.2 `ca_left` is a bonus for ending the turn

`economy.end_of_turn` step 5 resets `p.civil_actions` to the full total
(`economy.py:159`). The `end_turn` candidate has therefore already been
refilled, while every other candidate has spent 0 or 1. Measured over one 2p
champion game, 165 action-phase decisions that had both `end_turn` and a real
alternative:

* `end_turn` had the **highest `ca_left` of any candidate in 165 of 165** —
  100.0%;
* mean `ca_left(end_turn) − mean ca_left(others)` = **+2.95 actions**.

So a positive `ca_left` weight pays you ~3 × w for passing. It is the *same
axis* as `end_turn_bias`, with the opposite sign, and the two double-count.

The trajectory in `experiments/league_state/ladder_2p/` matches: `ca_left`
drifted 0.050 → 0.022 → 0.048 → **0.000 at generation 164** and has sat at
exactly 0.000 for the 170 generations since, while `end_turn_bias` went
−3.000 → −5.869 → −9.294 → −10.782 → **−14.439**. The search bought the same
correction it was allowed to buy, five times over, on the coordinate the guard
does not lock.

Honest limit on the "the search *wants* it negative" claim: `guard_2p.jsonl`
records 54 `ca_left` clamps, but once a weight is at 0 roughly half of all
gaussian mutations cross zero and get clamped, so most of those 54 are an
artifact of already being there. What is not an artifact is the **first**
crossing at gen 164 and the fact that 170 generations of drift have not moved
it off zero in the positive direction either.

**Verdict: `ca_left` is the 1-ply pass asymmetry, it is real, and the guard's
`NONNEG` rule is stopping the search from expressing the correction on this
coordinate.** I have not changed the guard: whether `ca_left` should be allowed
negative is a search-policy change that needs an n ≥ 200 A/B, and it belongs
next to the `end_turn_bias` work in `docs/WASTED_ACTIONS.md` §6, whose
DO-NOT-FIX warning I read and am not overriding. **Handed to the DEEPER_SEARCH
agent**: this is the same 1-ply dominance they are measuring, and a deeper
search is the fix that makes both hacks unnecessary.

### 6.3 `uprising` and `civil_actions` are not the inversion — they are dead

This is the part the hypothesis does not cover, and it is a different finding.

* `uprising` is a 0/1 flag. At its **default −12.0** it changes the chosen move
  in **0.06%** of decisions (1 in 1711 at 2p, 2 in 2842 at 3p). It varies at
  all in 0.5–0.7%. A coordinate with that little leverage gets no gradient from
  a win-rate signal, so it random-walks: the 2p ladder shows it going
  −12.000 → **+5.317** (gen 36, positive, accepted, and held for 35
  generations under the then one-sided guard) → −0.040 → −0.437 → 0.000. That
  is not a strategic discovery in either direction; it is a drunkard's walk.
* The mechanism behind the low leverage is that **the 1-ply search already
  prices an uprising exactly, without the flag**: the `end_turn` candidate has
  run `economy.end_of_turn`, and an uprising there simply skips the production
  phase, so the missing culture, science, food and resources are visible in
  `culture`, `science_rate`, `food_stock` and `resource_stock` directly. The
  flag is a forecast of something the search can already see.
* `civil_actions` (capacity) varies in 1.8–3.8% of decisions and flips 0.06%
  (2p default) to 0.4% (3p). It is nearly constant because a candidate set is
  almost never a choice between two different governments.

So "the trained bot prices civil actions at zero" is two different things
wearing one coat: `ca_left` is an actively harmful coordinate pinned at its
floor, and `civil_actions` / `uprising` are coordinates with almost no
gradient whose value carries no information. Neither reading is "the search
discovered that civil actions do not matter".

One caveat worth stating plainly: `discontent` at 4p is **not** dead
(−13.137, `varying` 0.410, flip 4.3%), and the 4p champion's `uprising`
= −41.784 does flip 3.2% of decisions. The happiness axis is alive at 4p and
dead at 2p. Whether that is a genuine player-count difference or an artifact of
each arm's own trajectory is not something 12 games can answer.

## 7. Handoffs

* **wars / aggressions / pacts** → `docs/COMBAT_AUDIT.md`. From this census, for
  their use: `war` was declared **0 times** out of 377 / 412 / 736 / 591 / 606 /
  1513 decisions where it was legal, in every arm and for both the champions
  and the default weights. `aggression` 1–6 times out of 300–1892.
  `cancel_pact` 0/1276 and 0/993 at 3p. `offer_pact` is well used (26–65%).
* **1-ply dominance / `end_turn_bias`** → `docs/DEEPER_SEARCH.md` agent, §6.2
  above. Two concrete numbers for them: `end_turn_bias` flips **53.9%** of the
  2p champion's decisions, and the multi-stage wonder blindness in §4.2 is a
  textbook 1-ply failure — the whole payoff is on the last step.
* **The `resign` blunder** (§4.1) is mine and is documented but **not fixed**:
  the fix is a policy change to the bot and needs an n ≥ 200 A/B I did not run.
* **`guard_weights`'s `NONNEG` clamp on `ca_left`** (§6.2) is a search-policy
  question, not a correctness one. Left alone deliberately.

Three things somebody should decide, none of which I changed:

1. Should `rival_culture_rate` / `rival_science_rate` / `rival_strength` be
   made live (recompute `ctx` per candidate), or deleted? Today they are three
   weights that do nothing, and the hill climb spends mutations on them.
2. Should `wonder_remaining` be sign-locked, or replaced by a term that prices
   "an unfinished wonder blocks every future wonder"? At 4p it is 0.000 and 86
   wonders were abandoned in 40 games.
3. Should the evaluator have any term at all for `p.resigned`?

## 8. What I did not measure

* No strength claims. Nothing here is an n ≥ 200 head-to-head; the three engine
  fixes are justified by the rulebook and by constructed positions, not by win
  rate. Their effect on the champions is unmeasured, and the revolution fix in
  particular makes a well-used action stronger, so the trained vectors are now
  slightly off-distribution.
* Census cells are 40 games and variance cells 12 games, one bot type per
  table. Rates below ~1% are indicative, not precise; I have quoted counts
  alongside every rate so the reader can see the denominator.
* `log_*` counters in the census JSON are read off `state.log`, which
  self-truncates at 400 lines, so they are lower bounds. Every number quoted in
  this document comes from `legal_moves`/final state instead.
* I did not audit the event deck card-by-card, the tactic composition tables,
  or the blue-token change-making beyond what the existing suite covers.
* The `resign` decomposition in §4.1 rests on **4** samples. The margin
  (+0.065) is real but its size is not something 4 samples establishes; the
  mechanism is established by reading `features()`, and the outcome (0 wins
  from 9 resignations) is exact.
* Whether `blue_free` = 6.627 is *wrong* is a strategy judgement I did not make.
  What is measured is its consequence: it dominates 58.2% of the 4p champion's
  decisions and it is the term that makes ending a turn look bad.
* The `choice:*` rows count forced decisions, so they cannot show a preference.
  A follow-up that recorded *which option* was chosen (which colony to annex,
  which building to raid) would be a genuine extension of the census; I did
  not build it.

## How to reproduce

```
TTA_JOURNAL=1 python3 tools/coverage_census.py  --players 4 --games 40 \
    --champ experiments/league_state/champion_4p.json --out /tmp/c4.json
TTA_JOURNAL=1 python3 tools/coverage_census.py  --players 4 --games 40 --bot default
TTA_JOURNAL=1 python3 tools/feature_variance.py --players 4 --games 12 \
    --champ experiments/league_state/champion_4p.json --out /tmp/v4.json
python3 -m unittest discover -s tests -q      # 176
```
