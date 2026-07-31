# Combat audit: wars, aggressions, pacts

Date: 2026-07-26
Branch: `combat-audit` (worktree `/Users/pt/tta-ai-combat-audit`), off master `8e751cb`
Scope: **base game only** — *Through the Ages: A New Story of Civilization* (2015).

The question this answers, asked directly: **"are you positive war and
aggression and pacts work in your models?"**

Short answer: **no, not before this audit.** Three engine bugs, all confirmed
against the printed rules, all now fixed with tests. But none of them explains
the 312-legal-0-taken result — see [Verdict](#verdict).

## Method

Everything below is checked against the printed rules, not against
[`docs/RULES_SPEC.md`](RULES_SPEC.md). Sources, all in `sources/`:

* `[CoL p.N]` — `cge_code_of_laws.pdf`, the full rulebook. Pages 3–4 are the
  Start-of-Turn Sequence and the entire Politics Phase.
* `[FAQ p.N]` — `faq_v15.pdf`.
* `[card]` — printed card text as transcribed in
  `data/cards_military_actions.json`, cross-checked against
  `sources/namu_military.txt`.

[`docs/RULES_SPEC.md`](RULES_SPEC.md) was then checked *against* those sources rather than
trusted. It holds up: §5.4–§5.11 and §13 are accurate. Two of the three bugs
below are cases where the code failed to implement what the spec already said
correctly (§5.6 "remove attack-ending pact"; §5.4.2 "exclude pact bonuses that
end if you attack"). The spec is not where the error was.

Tests are in `tests/test_combat.py` — 55 tests that build positions by hand.
That is deliberate: self-play cannot reach any of this (see
[Reachability](#reachability-engine-wrong-vs-bots-blind)), so a self-play-driven test would have proved
nothing. Every test names the rule it checks.

* Before the fixes: 4 of the 55 failed (commit `1b7f1c9`).
* After the fixes: **211/211 pass** (`python3 -m unittest discover -s tests -q`),
  of which the 156 pre-existing tests are untouched.

A single-process random-bot probe (8 games 3p, 12 games 4p, one worker) was
used to confirm the paths execute at all and to size the bugs. Random bots
declare wars constantly, so they exercise what the trained bots never touch.

---

## Conformance table

Legend: **OK** = code matches the printed rule, with a test.
**BUG** = divergence, now fixed. **GAP** = knowingly incomplete, not fixed.

### Wars

| Mechanic | Rule | Code | Verdict |
|---|---|---|---|
| Declaration cost | printed MA cost, paid on reveal `[CoL p.4]` | `actions.py:1044-1063` | OK |
| Gandhi doubles the cost | `[card]` | `actions.py:1049-1050`, `:297` | OK |
| Not during the last round | `[CoL p.4]` | `actions.py:291` `not state.last_round` | OK — and `_set_last_round` (`game.py:201`) makes that exactly "you will get another turn" |
| Target: blocked by a no-attack pact | `[CoL p.4]`, `[FAQ p.11]` (Peace Treaty / Loss of Sovereignty / Acceptance of Supremacy only) | `effects.war_forbidden` `:505` | OK |
| Loss of Sovereignty side B: nobody may declare war | `[card]` | `Stats.war_immune`, `effects.py:507` | OK |
| **Declaration removes a pact that ends on attack** | `[CoL p.4]`, `[FAQ p.11]` | `_h_war` did **not** call `cancel_attack_pacts` | **BUG 1** |
| Not restricted by relative strength | `[CoL p.4]` | no strength test on the war branch | OK |
| One outstanding war at a time | implied (it resolves before your next Politics) | `actions.py:292` | OK |
| Resolution timing: start of the declarer's next turn, after the row replenish, before tactics/politics | `[CoL p.3]` | `game.start_turn:215-225` | OK |
| Compare current strengths, declarer's when-attacking bonuses included | `[CoL p.3]` | `events.resolve_war:568-570` | OK |
| No bonus cards, no discards, no sacrifices in a war | `[CoL p.3]`, `[FAQ p.11]` | no `defense` pending is pushed | OK |
| Tie = no effect | `[CoL p.3]`, `[FAQ p.11]` | `events.py:572-573` | OK |
| Either side can win | `[FAQ p.11]` | `events.py:574-575` | OK |
| War over Territory: 1 token + 1 per full 5 advantage, capped by the loser's bank | `[card]`, `[FAQ p.11]` | `events.py:579-582` | OK |
| War over Culture: 5 + advantage, capped by the victim's culture | `[card]`, `[FAQ p.11]` | `events.py:587-590` | OK |
| War over Technology: science = advantage, capped by the loser's science | `[card]`, `[FAQ p.11]` | `events.py:583-586` | OK for the science branch |
| War over Technology: victor may take blue special techs instead | `[CoL p.3]`, `[FAQ p.11]` | ~~not implemented~~ `interact.war_tech_spoils` (`a7a5ef1`, 2026-07-30) | ~~**GAP 1**~~ RESOLVED |
| Card discarded either way | `[CoL p.3]` | `events.py:571` | OK |
| Declared wars survive antiquation; pacts do not | `[CoL p.3]` | `game._antiquate:176-198` | OK |
| A pact accepted after declaration but before resolution counts | `[FAQ p.11]` | live `state_stats` at resolution | OK |
| A no-attack pact does not cancel an already declared war | `[CoL p.4]`, `[FAQ p.11]` | `resolve_war` does not consult pacts | OK |
| Several players may declare war on the same civ | `[FAQ p.11]` | `wars_declared_on_me` is a list | OK |
| Resolve a war and then aggress the same rival in one turn | `[FAQ p.11]` | nothing blocks it | OK |
| Resigning: declarers remove the card and score 7 culture | `[CoL p.4]` | `_h_resign:1023-1030` | OK |

### Aggressions

| Mechanic | Rule | Code | Verdict |
|---|---|---|---|
| Cost in military actions, paid on reveal, not refunded on failure | `[CoL p.4]` | `events.start_aggression:485-489` | OK |
| Gandhi doubles it; Gandhi may not attack | `[card]` | `events.py:487-488`, `Stats.no_aggression` | OK |
| Illegal if a pact forbids attacking them | `[CoL p.4]` | `effects.pact_forbids_attack:493` | OK |
| Illegal if the rival's strength ≥ yours | `[CoL p.4]` | `actions.py:288-290` | OK |
| **…excluding strength from a pact that ends if you attack — on both sides** | `[CoL p.4]`, `[FAQ p.11]` | attacker's side was excluded, defender's was not | **BUG 3** |
| A pact that ends on attack is removed before resolving | `[CoL p.4]` | `events.py:492` | OK |
| Defence: bonus cards at their printed defence value (2 / 4 / 6) | `[CoL p.4]`, `[card]` | `interact._defense_move:616-631` | OK |
| Defence: any other military card = +1 | `[CoL p.4]` | same | OK |
| Defence budget = the defender's military action **total** | `[CoL p.4]`, `[FAQ p.11]` | `interact.start_defense:605-609` | OK (includes a pact's +1 MA) |
| Defender total ≥ attacker: fails, no effect; ties favour the defender | `[FAQ p.11]` | `events.finish_aggression:503-507` | OK |
| No unit sacrificing by either side (2015 change) | `[RB p.24]`, `[FAQ p.11]` | not offered | OK |
| Plunder: take up to N food and/or resources, never more than the victim has, no blue tokens change hands | `[FAQ p.7]` | `events.py:515-519` | OK — but the split is chosen greedily, see **GAP 2** |
| Spy / Armed Intervention: take up to N science / culture | `[card]` | `events.py:520-527` | OK |
| Enslave: +2 food +2 resources, victim decreases population (victim chooses the worker; token to the yellow bank) | `[card]`, `[FAQ p.15]` | `events.py:528-531` → `interact._q_lose_pop` | OK |
| Raid: destroy urban buildings within the printed ages; attacker chooses; worker to the **worker pool** | `[card]`, `[FAQ p.7]` | `interact._q_raid:400`, `_c_raid:177` | OK |
| Raid loot = half the **printed** build cost, rounded up, ignoring modifiers | `[CoL p.4]`, `[FAQ p.7]` | `interact.py:187-188` | OK |
| Annex: take a colony, permanent bonus transfers, immediate bonus does not | `[card]`, `[FAQ p.7]` | `interact._c_annex:192` | OK |
| Infiltrate: remove a leader or unfinished wonder, 3 culture per level | `[card]` | `interact._c_infiltrate:203` | OK |
| Annex needs a target with a colony; Infiltrate needs a leader or unfinished wonder | printed `target` line on the card | not enforced — the aggression resolves to nothing | **GAP 3** |
| Aggressions are legal in Age IV and in the last round | `[CoL p.4]` (only wars are restricted) | no restriction | OK |

### Pacts

| Mechanic | Rule | Code | Verdict |
|---|---|---|---|
| No pacts in a game set up for 2 players (removed from the decks) | `[CoL p.2]`, `[card]` counts `2p: 0` | `cards.py` deck build + `actions.py:266` | OK |
| **A resignation down to 2 players does not strip pacts from the current decks or from hands** | `[FAQ p.11]`, `[CoL p.4]` | gated on `len(active_players()) < 3` | **BUG 2** |
| Future-age decks re-trimmed for the surviving count | `[CoL p.4]`, `[FAQ p.11]` | `game._advance_age:165-171` via `live_count` | OK |
| Offer: reveal, name the partner, name side A/B | `[CoL p.4]` | `_h_offer_pact:987-1000` | OK |
| Refusal returns it to hand and still uses the political action | `[CoL p.4]` | `interact._c_pact_offer:218-229` | OK |
| Acceptance: applies immediately, ends any other pact **in your own play area only** | `[CoL p.4]`, `[FAQ p.11]` | `interact.py:222-226` | OK |
| You may be party to several pacts in other players' areas | `[CoL p.4]` | `effects.pacts_for:423` | OK |
| Costs no military actions to offer / accept / cancel | `[FAQ p.11]` | no MA touched on any pact path | OK |
| Cancel any pact you are a party to, wherever it sits | `[CoL p.4]` | `_h_cancel_pact:1003` | OK |
| Each pact's effects: Open Borders +1 MA both and +2 to the attacker; Trade Routes food↔resource; Acceptance of Supremacy / Peace Treaty / Loss of Sovereignty forbid attacks; Scientific Cooperation −2 science and the partner pays 1; Promise of Military Protection +4 to B; Military Alliance +3 both; International Tourism per the other's wonders | `[card]` | `effects._apply_pacts:449` + `FLAT_KEYS` | OK |
| Only "Promise of Military Protection" and "Military Alliance" are cancelled by an attack | `[FAQ p.11]` | `cancelledIfPartiesAttackEachOther` on exactly those two | OK |
| Antiquated pacts leave play | `[CoL p.3]` | `game._antiquate:196-197` | OK |
| Resigning removes every pact you are party to | `[CoL p.4]` | `effects.drop_pacts_of:559` | OK |

### Military cards / bonus strength (the gate on everything above)

| Mechanic | Rule | Code | Verdict |
|---|---|---|---|
| Bonus card defence values 2 / 4 / 6 by age | `[card]` | data + `_defense_move` | OK |
| Colonization values 1 / 2 / 3 by age, bonus cards discarded | `[card]`, `[CoL p.7]` | `interact.bonus_pool:479` | OK |
| Colonization force excludes strength-rating bonuses; ≥1 unit mandatory | `[FAQ p.16]` | `interact.force_value:488` | OK (already covered by `tests/test_engine.py`) |
| Strength rating = units + tactics armies + card bonuses, floored at 0 | `[CoL p.9]` | `effects.compute` + `army_strength:598` | OK |
| Military hand limit = military action total, enforced only at end of turn | `[CoL p.8]` | `economy.end_of_turn` | OK (pre-existing test) |
| Military deck exhaustion reshuffles the discards; the age does not end | `[CoL p.8]` | `economy.draw_military` | OK (pre-existing test) |

---

## Bugs found

### BUG 1 — declaring war did not cancel a pact that ends on attack

**Rule.** `[CoL p.4]`, under *Declare a War*, in the same words it uses for
*Play an Aggression*:

> If you and your rival have a pact that says it ends if you attack, remove
> that pact from play.

`[FAQ p.11]` removes any doubt about what that means for the strength
comparison:

> **Pacts Cancelled by Attacks**: The only Pacts that will be canceled by one
> civilization attacking the other (either by Aggression **or by declaring
> War**) are the "Promise of Military Protection" and the "Military Pact"
> Pacts. **The Military Strength given by either Pact will not affect any War
> or Aggression which is declared between the two civilizations** — for the
> Pact is cancelled immediately.

**Code.** `engine/events.py:492` (`start_aggression`) called
`effects.cancel_attack_pacts`. `engine/actions.py:_h_war` never did. The pact
stayed in play forever, and `events.resolve_war:568-570` reads both players'
live `state_stats(...).strength`, so the defender kept the pact's strength in
the very war that should have destroyed it.

Concretely: P0 (strength 5) holds Promise of Military Protection with P1 as
side B (P1 base strength 3, +4 from the pact). P0 declares war. Correct play:
the pact dies, 5 vs 3, P0 wins by 2 and takes a yellow token. Old engine: pact
lives, 5 vs 7, **P1 wins** and takes a token off P0. Military Alliance was less
severe (+3 to both, so the advantage cancelled) but the pact still wrongly
survived and kept paying out.

**Failing tests on `1b7f1c9`:**
`TestWarDeclaration.test_declaring_war_cancels_a_pact_that_ends_on_attack`
and
`TestWarResolution.test_a_pact_strength_cancelled_by_the_declaration_does_not_apply`.

**Fix.** `engine/actions.py:1057` — `_h_war` now calls
`effects.cancel_attack_pacts(state, p, state.players[target])` after paying
and before placing the war, exactly where `start_aggression` does it.

**Measured frequency.** In 12 four-player random-bot games there were 47 war
declarations and **0** of them had a cancellable pact between the parties (71
aggressions, 1 did). So this bug is rare under random play. It is not rare
under skilled play: the standard use of Promise of Military Protection is a
third party handing +4 to one side of a war, and `sources/namu_military.txt`
§5.2 describes exactly that line.

### BUG 2 — a resignation made pacts in hand unplayable

**Rule.** Pacts are removed from the military **decks** when a game is set up
for two players `[CoL p.2]`. That is a setup rule. `[FAQ p.11]`, on resigning:

> Do not remove any Pacts or 3+ or 4-player cards from the **current**-Age
> decks; but do remove them from any future-Age decks.

`[CoL p.4]` says the same and adds "So some cards designed for more players may
appear in this age." The survivors therefore keep *drawing* pact cards from the
current-age deck. Neither source lists "pacts become unplayable" among the
rules that switch on when the table drops to two — the enumerated ones are the
card-row sweep count, future-deck trimming, and the two-player reading of
"strongest/weakest".

**Code.** `engine/actions.py:259` on master:

```python
elif typ == "pact":
    if len(state.active_players()) < 3:          # §13: no pacts in 2p
        continue
```

A *dynamic* test standing in for a *setup* rule. In a real 2-player game it
never fires (the deck holds no pacts), so its only effect was after a
resignation, where it turned every pact in hand into a dead card that still
counted against the military hand limit — while `cancel_pact` (`actions.py:250`)
stayed legal, so the engine was also inconsistent with itself.

**Failing test on `1b7f1c9`:**
`TestPacts.test_a_resignation_does_not_make_a_pact_in_hand_unplayable`.

**Fix.** `engine/actions.py:266` — keyed on `state.num_players`, the setup
count. `tests/test_combat.py` also pins both halves of the FAQ sentence: 2p
decks contain no pacts, and `_advance_age` still re-trims future decks after a
resignation.

**Same-shape check elsewhere.** Every other use of a player count was checked.
`game.live_count` (`game.py:99`) is used for the sweep count, for future-deck
trimming and, via `events._pkey`, for the per-player-count tables on event
cards — all three are explicitly *live* counts per `[CoL p.4]` / `[FAQ p.11]`,
so they are right. `game.new_game`'s `num_players + 2` event seeding and
`_seat_index` are setup counts and are right. The pact gate was the only one
using the wrong kind of count.

### BUG 3 — aggression legality counted pact strength that the attack destroys

**Rule.** `[CoL p.4]`, the strength test for an aggression:

> You cannot attack a player whose strength equals or exceeds yours.
> * Remember to include any bonuses that trigger when you attack the other player.
> * **Do not include bonuses from pacts that end if you attack.**

Read together with `[FAQ p.11]` above ("will not affect any War or Aggression
… the Pact is cancelled immediately"), the pact's strength is off the table for
both players.

**Code.** `engine/effects.attack_strength` already subtracted the *attacker's*
share. `actions.py:281-283` then compared it against the defender's **raw**
`state_stats(...).strength`, which still contained the defender's share. The
engine also disagreed with itself: `interact.start_defense:606` computes the
defender's strength *after* `cancel_attack_pacts` has run, so a move the
generator called illegal would in fact have resolved in the attacker's favour.

Worst case is Military Alliance, which gives +3 to both: the engine subtracted
3 from the attacker and added 3 to the defender, so attacking your own alliance
partner needed a **6-point** strength edge instead of 1.

**Failing test on `1b7f1c9`:**
`TestAggressionLegality.test_pact_strength_that_ends_on_attack_is_not_counted`.

**Fix.** New `effects.defense_strength` (`effects.py:527`), the mirror of
`attack_strength`, both built on a shared `_doomed_pact_strength`
(`effects.py:494`); `actions.py:288` uses it.

---

## Gaps left unfixed (documented, not repaired)

**GAP 1 — ~~War over Technology cannot take blue special technologies.~~
RESOLVED 2026-07-30 (`a7a5ef1`).** ~~`[CoL p.3]` and `[FAQ p.11]` let the
victor take special techs instead of some or all of the science, with the
no-duplicate and higher-level-replaces rules. `events.resolve_war:583-586`
implements only the science branch. Because the choice belongs to the victor,
taking science is always available, so this is an under-implementation rather
than a wrong answer — except when the loser has little science but valuable
blue techs, where the engine pays the victor less than the rules do. Left
alone: it needs a new choice point plus special-tech transfer, and no bot has
ever declared a war.~~ `resolve_war` now branches on the card's own
`orTakesSpecialTechnologiesOfSameTotalScienceCost` key and offers the steal
through `interact.war_tech_options`/`war_tech_spoils` on the existing
pending-stack machinery. Every evaluator-driven bot gets the choice for free
by construction; see [`docs/WAR_OVER_TECHNOLOGY.md`](WAR_OVER_TECHNOLOGY.md) for the full
implementation and which bot needed new policy (BookBot).

**GAP 2 — Plunder's food/resource split is chosen greedily, not by the
attacker.** `[FAQ p.7]` says the aggressor chooses the mix.
`events._food_or_resources` takes resources first from the victim and gives
resources first to the attacker. Totals are right and the cap is right; only
the mix is not a decision. Low impact, but it is a missing choice.

**GAP 3 — Annex and Infiltrate can be played at a target that cannot pay.**
The printed target line on the cards ("one opponent who owns at least one
colony", "one opponent with a leader in play or a wonder under construction")
is in `data/cards_military_actions.json` and the engine ignores it: the
aggression is legal, costs 2 MA and a card, resolves "successfully", and does
nothing. `[CoL p.5]`'s "you cannot perform an action unless you are able to
perform all the required steps" is written for the Action Phase, so the printed
rules do not settle it; the digital edition does not offer the target. Flagged
as an engine-versus-own-data inconsistency rather than a certain rules
violation, and deliberately **not** changed, because changing `legal_moves`
without a firm citation is worse than a rare void play.

---

## Reachability: engine wrong vs bots blind

These are different problems with different fixes, so they are reported
separately.

**The engine paths all execute.** Measured, single process, `RandomBot` (which
does take these moves), 8 games at 3p: 43 pact offers, 15 pact cancellations,
25 aggressions (12 successful / 13 repelled), **19 war declarations and 19 war
resolutions** — a clean 1:1, so no declared war is ever silently dropped. At
4p, 12 games: 47 declarations, 71 aggressions. The engine models all of it.

**The trained bots reach almost none of it.** From a parallel read of the bot
layer (`engine/bots/`, `experiments/`), cross-checked against the committed
measurements:

| move | legal per game (measured) | taken per game (measured) | what stops it |
|---|---|---|---|
| declare war | 6.50 at 4p, 6.25 at 2p | **0.00 everywhere** | no feature exists that could see it |
| play aggression | 7.35 at 4p | 0.08 at 4p, 0.00 at 2p | 1-ply horizon: the payoff lands in the defender's decision |
| offer pact | — | 1.80 at 3p, 3.21 at 4p | works, since `deferred_credit` handles it |
| accept pact | — | works (inferred) | resolves inline, so the trial state shows it |
| defend an aggression | — | reachable | see the note below |
| colony bid at 4p | 2.38 auctions/game | 0.01 | 14 of 19 auctions die with **zero eligible bidders** — the 4p champion owns 0.07 military units per player, so `max_force == 0` excludes it at the door |

The decisive fact is structural, not statistical: `_h_war` writes exactly
`war_declared_by_me`, `wars_declared_on_me`, the spent MA and the discarded
card, and **no feature in `weighted.features()` reads either war field**. So the
evaluation delta of a war declaration is `-hand_military_weight −
ma_weight × cost` under *every possible weight vector*. Zero wars is not a
training failure, it is arithmetic. [`docs/CULTURE_GAP.md`](CULTURE_GAP.md):143-155 measures the
gap at 0.224–0.225 points, exactly the 4p champion's `hand_military` weight.

**On [`docs/AGGRESSION_FIX.md`](AGGRESSION_FIX.md#b-aggressions-and-wars-confirmed-and-it-is-the-1-ply-horizon) section B.** It ends at "See the next section for
the implementation and the A/B result." and there is no next section; the file
has exactly one commit (`8d24aff`, the diagnosis). Nothing in it was silently
assumed done in `weighted.py` — `deferred_credit` (`weighted.py:121-173`) still
handles only `pact_offer` and `auction`, exactly as [`docs/CULTURE_GAP.md`](CULTURE_GAP.md)
states. But the tree is not empty either: `engine/bots/quiescent.py` exists,
with quiescence for the aggression/pact/bid pendings and a `WAR_LOOKAHEAD`
(`:231`, `:298-301`) that calls the engine's own `events.resolve_war` on a
scratch copy (`_war_value`, `:201-214`). It is **opt-in only**
(`arena.py`'s `quiesce:` prefix; `hillclimb_pool.py`'s `--with-quiescent`,
default off), it is **not used by any of the three running trainers**, and its
strength has **never been measured** ([`docs/DEEPER_SEARCH.md`](DEEPER_SEARCH.md#4-strength-ab) §4/§5/§6 all read
"RESULTS PENDING"; branch `quiesce-ab` has no commits). So the fix was written
as an alternate bot class and never gated in. That is worth knowing before
anyone concludes the war channel is unfixable.

**One bot-side finding on the engine boundary, inferred not measured.**
`interact._defense_move:626-627` returns early, leaving the `defense` pending
in place, whenever the defender still has budget and cards. That is correct
rules — the defender may keep adding cards — but it means a 1-ply bot scores
`("defend", card)` on a trial that shows the discarded card and *not* the
averted loss, while scoring `("defend_done",)` on a trial that *does* show the
full loss. The same asymmetry as the aggression bug, in the opposite direction
(it over-values the first defence card). Nobody's diagnosis covers it.

---

## Verdict

**Is the 312-legal-0-taken result explained by an engine defect, a bot blind
spot, or both?**

**A bot blind spot, essentially entirely.** I went looking for an engine fault
and found three real ones, but none of them can produce that number:

* **Bug 1** only fires when a Military Alliance or Promise of Military
  Protection exists between the two parties — measured at 0 of 47 war
  declarations in random 4p play. It cannot suppress 312 legal declarations,
  and it does not touch legality at all: those 312 moves were *offered* to the
  bot and refused.
* **Bug 2** only fires after a resignation, and affects pacts, not wars.
* **Bug 3** does affect legality, but only for aggressions and only against a
  pact partner. It cannot make wars illegal, and wars were the 312.

The 312 were **legal moves the bot declined**, and the reason is that the
bot's evaluation function has no coordinate that changes when it declares war.
`_h_war` moves two features, both downward. That is a representation hole in
`weighted.features()`, not an engine defect. The engine resolves every war it
is given — 19 declared, 19 resolved, spoils correct to the token in every case
the tests check.

So the honest split is:

* **Engine wrong:** yes, in three places, now fixed, with tests. Two of them
  (the war pact cancellation, the aggression legality asymmetry) would have
  distorted real play. None is the cause of 312/0.
* **Engine right, bots blind:** this is the 312/0 story, and it stays true
  after the fixes. The repair is a bot-side one: either add
  `war_declared_by_me` / `wars_declared_on_me` features to
  `weighted.features()`, or measure and gate in the `QuiescentBot`
  `WAR_LOOKAHEAD` that already exists and has been sitting unused.

**Answer to the question as asked.** Wars, aggressions and pacts are now
modelled correctly, to the level of every rule I could find printed text for,
with 55 tests pinning it. They were not, before today, in three specific ways.
And "the model is correct" and "the bot uses it" remain two different claims —
the second is still false for wars.
