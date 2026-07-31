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
by construction; see §3 below (the former `docs/WAR_OVER_TECHNOLOGY.md`) for the full
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

**On the former `AGGRESSION_FIX.md`'s section B (now [`docs/AGGRESSION_RATE.md`](AGGRESSION_RATE.md#b-aggressions-and-wars-confirmed-and-it-is-the-1-ply-horizon) appendix B).** It ends at "See the next section for
the implementation and the A/B result." and there was no next section in the
original file; it had exactly one commit (`8d24aff`, the diagnosis). Nothing in it was silently
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

## 1. The military pricing seam, and the write-off that outlived its reason (2026-07-30) (merged from the former `MILITARY_SEAM.md`, 2026-07-31)

2026-07-30.  Companion to [`docs/CARD_BLINDNESS.md`](CARD_BLINDNESS.md) (the civil-card census) and
[`docs/CARD_BLINDNESS.md`](CARD_BLINDNESS.md) (what the military hand cannot see).

Three things land here.  Only the first is plumbing; the other two are a
stale write-off and a write-off that is *not* stale and now says so.

---

### 1.1. `hand_mil_potential` never passed the board

`engine/bots/weighted.py:hand_mil_potential` summed
`card_potential(n, w)` -- no `state`, no `idx`.  `card_potential` gates both
of its board branches on

    on_board = state is not None and idx is not None

so **board-aware pricing could not fire for a military card under any weight
vector**.  Not "did not today": could not, structurally.  Anything a later
lane priced onto a military card through `board_yields` would have been dead
on arrival, and the null it produced would have looked like a result.

There was no reason for the omission.  `hand_mil_potential(state, idx, w)` is
handed a state, and its only caller is `evaluate`, which has one.  Nothing
needed threading; the arguments were simply not forwarded.  Fixed by
forwarding them.

**It changes no number today, and that is checkable rather than hoped:**

* `board_yields.board_yields` returns `None` for any type outside
  `SWAP_TYPES = {leader, government, wonder}`.  No military type is in it.
* `board_extra` returns `()` for any name outside `_EXTRA_CARDS`, which is
  three *civil* action cards (Endowment for the Arts, Wave of Nationalism,
  Military Build-Up).
* `_board_credit_key` has no entry for a military type, so a military card's
  board credit is the bare `card_board_credit` -- 0.0 on all three live
  champions, which takes `card_potential`'s early return without consulting
  the state at all.

`tests/test_card_pricing.py:TestTheMilitaryHandPassesTheBoardThrough` asserts
both halves: that the state now reaches `card_potential` for every card in
the military hand, and that the value is unchanged for every one of them.
The second assertion is the attribution: this commit opens a seam, it does
not reprice.  A lane that makes a military type board-aware should expect to
update it, deliberately.

---

### 1.2. STALE: the two bonus keys were written off for a reason that had expired

    _unpriced("military hand: never reaches _card_yields "
              "(hand_potential is civil-only)",
              "defenseBonus", "colonizationBonus")

True when written.  False by the time it was read: `hand_mil_potential`
walks `p.hand_military` and calls `card_potential` -> `_card_yields` on every
card in it.  The proof that the route is live is in the same file -- a
territory is priced from `immediateEffects`/`permanentEffects` through
`_TERR_TO_FEATURE`, reached by exactly it.  `_card_yields` *was* being asked
about a bonus card; it just had no entry and returned `()`.

So the blindness was a leftover write-off, not a limitation.  A comment is
not a test, which is the general lesson: the file's own coverage tests
(`test_no_stale_entries_in_the_unpriced_set`,
`test_a_key_is_not_claimed_both_priced_and_unpriced`) can catch a key no card carries
and a key that is claimed twice, but nothing could catch a *reason* that had
stopped being true.  The same staleness was in
`tools/card_blindness.py:reachable`, whose docstring asserted military-deck
cards are never asked about; it now says under which vector that holds.

#### The three cards, and where their numbers come from

`type: "bonus"`, six copies each at every player count, and these three are
the whole type.  Both keys are on all three:

| card | age | defenseBonus | colonizationBonus |
|---|---|---|---|
| Military Bonus (defense 2 / colonization 1) | I | 2 | 1 |
| Military Bonus (defense 4 / colonization 2) | II | 4 | 2 |
| Military Bonus (defense 6 / colonization 3) | III | 6 | 3 |

Both mappings are the rules engine's own arithmetic, not an opinion:

* **`colonizationBonus` -> `colonize_bonus`.**  `engine/interact.py:
  force_value` adds the card's `colonizationBonus` into the *same sum* as
  `effects.state_stats(p).colonize`, and `features()` already publishes that
  stat as `colonize_bonus`.  One colonization point from a card and one from
  the board are the same point, so they share the weight -- the same
  "same key on both sides" convention `civil_actions` already follows.

* **`defenseBonus` -> `defense_bonus`, priced as `defenseBonus - 1`.**
  `engine/interact.py:defense_points` is the authority (`_defense_move`
  calls it) and it gives **every** military card 1 -- any card can be
  discarded face down for +1 defence -- and these three 2/4/6.  The flat 1
  is already carried by `hand_military`, a count of the military hand, so
  what a bonus card adds that a generic card does not is the increment,
  1/3/5.  Pricing the printed number would count the generic
  face-down-discard value of the card twice.

`defense_bonus` is a new weight at 0.0 (the project's standing rule for a new
channel) and is CARD-ONLY: the card defends by being *spent*, so unlike its
colonization half there is no board state left for `features()` to mirror.
`bonus_card_credit` defaults to 1.0, on the same terms as `territory_credit`:
0.0 recovers the pre-change pricing byte for byte, so the change is
A/B-able against itself in one process.

---

### 1.3. NOT STALE: `cost.militaryActions` stays unpriced, with a better reason

54 cards carry `cost`, always as `{"militaryActions": n}` -- the only subkey
of `cost` anywhere in the database.  The breakdown decides the question:

| cost | types |
|---|---|
| 0 | bonus 3, pact 10, territory 12 (25 cards -- nothing to price) |
| 1 | aggression 5, tactic 15 |
| 2 | aggression 5, war 2 |
| 3 | aggression 1, war 1 |

Every card with a **non-zero** military-action cost is an aggression (11), a
tactic (15) or a war (3) -- which is exactly and exhaustively the set of card
types whose *gain* `_card_yields` deliberately does not hold.  Aggressions
and wars are priced by resolution (`QuiescentBot` drains the defence pending
with real picks; `quiescent.war_value` calls the engine's `resolve_war`), and
a tactic's gain is `tactic_gain`/`tactic_short`, a board query.

Pricing the cost on its own would therefore reproduce, exactly, the worst
pricing defect this project has recorded: the ten unit cards that scored
strictly negative for most of the project's life because `_card_yields` read
their `techCost` and `buildCost` and never their `strength`.  And it would
not be a rounding error -- the live 3p champion carries
`military_actions = 3.48`, so every aggression in hand would price at -3.48 x
credit before a single point of its payoff was counted.

Map it in the change that also prices what the card *buys*, not before.  The
reason in `tests/test_card_pricing.py:TOP_LEVEL_UNPRICED["cost"]` has been
upgraded from "the evaluator sees the action spent in the post-move state"
(true, but not the load-bearing reason) to this one.

---

### 1.4. Conduction: which gate this opens, measured before any games

`tools/conduction_table.py`, run on the three **live** league champions
before touching anything:

| vector | gen | `hand_mil_potential` | verdict |
|---|---|---|---|
| `champion_2p` | 59 | 0.0 | CLOSED |
| `champion_3p` | 1275 | **0.01079** | **OPEN** |
| `champion_4p` | 357 | 0.0 | CLOSED |

(Generations as of 2026-07-30; the league is live and they move.  Re-run the
tool rather than trusting the table -- that is the tool's entire point.)

The premise this work started from -- "the military weight is zero on all
three champions" -- is **wrong for the live 3p champion**.  It is right for
2p and 4p, and right for all three *frozen* champions, which predate the
weight entirely.

**Gate (a), consumer openness: this change opens nothing new; it uses the one
gate that was already open, and only at 3p.**  `hand_mil_potential` is the
only consumer of `card_potential` that can see a military card at all --
`hand_potential` and `rival_hand_potential` walk `hand_civil`,
`wonder_potential` walks `p.wonder`, and `row_pressure` walks
`state.card_row`, which is the *civil* row.

**Gate (b), `row_pressure`'s `card_potential <= 0` skip: it does not apply to
this change at all.**  There is no military row in the base game -- military
cards are drawn blind from a deck -- so `row_pressure` never sees one.
`hand_mil_potential` *sums* the hand with no threshold, and a card that
prices negative subtracts rather than disappearing.  **No card crosses a live
zero threshold as a result of this change.**  (`conduction_table`'s
`visible to row_pressure: n/236` counter does move 44 -> 47 at 3p, because
that counter is deck-blind and now sees the three bonus cards price above
zero.  That is a counter artefact, not a gate: those three cards are not in
the row and never were.  The tool now prints a separate military-deck section
saying so, so the next reader does not have to re-derive it.)

What actually conducts, then, is small and honest:

| vector | bonus card I / II / III `card_potential` | reaches score? |
|---|---|---|
| `champion_2p` | 0.0 / 0.0 / 0.0 (`colonize_bonus` is 0.0) | no -- gate closed |
| `champion_3p` | 0.042 / 0.084 / 0.126 | yes, x 0.01079 |
| `champion_4p` | -0.074 / -0.147 / -0.221 (`colonize_bonus` is -0.074) | no -- gate closed |

and the *defence* half -- the larger and more strategically real of the two
-- conducts **nowhere** today, because `defense_bonus` is a new key sitting
at 0.0 on every vector in the league.  `hillclimb.mutate` perturbs by
`gauss(0, s) * (abs(w) + 0.15)`, so it moves on the first generation that
scatters onto it; until then this half of the change is a channel, not an
effect.  Anyone measuring it must open `defense_bonus` (and, at 2p/4p,
`hand_mil_potential`) by hand, or they will measure an arithmetic identity --
[`docs/CARD_BLINDNESS.md`](CARD_BLINDNESS.md) Sec 5.3's 12,800-game null, again.

### 1.4b. Where the 3p conduction actually shows up

A bonus card has **no move handler at all** ([`docs/CARD_BLINDNESS.md`](CARD_BLINDNESS.md) row 9): it is
never "played", only spent inside the defence and colonization machinery.  So
the only decision its price can reach is the one about *holding* it -- and
there is exactly one: `engine/interact.py:_discard_military`, RULES_SPEC §6.6
step 1, the end-of-turn military discard, which §2 below (the former `docs/MILITARY_DISCARD.md`)
turned from a `pop(0)` into a real `push_choice`.

That function's own docstring names this change's precondition:

> it is load-bearing anyway, because the weighted-family evaluator is
> documented-blind to military card identity beyond age (`hand_mil_value` is
> a sum of age+1) ... so same-age options tie and every argmax in the project
> falls back to option 0.

With the two keys mapped and `hand_mil_potential` open, a Military Bonus and
a same-age war no longer tie: at the live 3p champion the bonus is worth
0.00045 / 0.00091 / 0.00136 eval points more to keep (age I/II/III).  Be
honest about the size of that -- it is small, and `discard_options` already
orders the options least-defensive-first, so the argmax-falls-back-to-0
behaviour was *already* discarding the right card most of the time.  What
changes is that the evaluator now has a reason of its own instead of
inheriting one from presentation order, which is the failure mode that
ordering was explicitly a workaround for.

### 1.5. Fingerprints

All eight `tools/gate.sh` arms play `DEFAULT_WEIGHTS`, in which
`hand_mil_potential` is 0.0, so `card_potential` is never called on a
military card and neither the seam fix nor the bonus mapping can reach a
digest.  Predicted inert before running, and the gate agrees: no digest
moved, and none was re-derived.

## 2. The military discard is a decision (2026-07-30) (merged from the former `MILITARY_DISCARD.md`, 2026-07-31)

`engine/economy.py` end-of-turn step 1 was `hand_military.pop(0)` — first in,
first out, no decision. [`docs/RULES_SPEC.md`](RULES_SPEC.md) §6.6 step 1 says of that exact
step: *"Only step requiring a decision."* The engine was taking the decision
away from the player and answering it with the worst rule of thumb available:
throw away the oldest card.

Everything here is base game (2015 "A New Story of Civilization").

### 2.1. The rule, checked against the rulebook and not against the spec line

`sources/ubg_subsequent-rounds.txt:182` — the End-of-turn Sequence page:

> **Discard Excess Military Cards.** Your number of red tokens defines the
> maximum number of military cards you are allowed to have after this step.
> If necessary, you must discard military cards so that your total is not
> greater than the number of red tokens you have. They are discarded face down
>
> **Streamlining The Game.** Once you have **decided which military cards to
> discard**, the rest of your turn is automatic. That is, it requires no more
> decisions. The next player may start his or her turn as soon as you finish
> discarding.

So the spec line is right, and the rulebook says something slightly stronger
than the spec does: this is not merely *a* decision, it is *the* decision — the
reason the rest of `end_of_turn` may stay straight-line code, and the reason
the next player may not start until the discarding is done. Both of those are
now properties of the engine rather than accidents of it (§2.3).

### 2.2. Size of the violation

`tools/discard_census.py`, 12 games of 2p WeightedBot self-play under
`DEFAULT_WEIGHTS`, 494 player-turns, answering every discard the way the old
engine did:

| | |
|---|---|
| cards discarded by step 1 | 368 — **30.7 per game, 0.75 per player-turn** |
| of which are real decisions (≥2 distinct cards in hand) | 367; exactly 1 auto-resolved |
| mean distinct options offered | 4.17 |
| firings where FIFO pitched a card better than the worst available | 23.7% |
| firings where FIFO pitched the **sole best defender** in hand while a strictly worse card was available | **20.7%** |
| defence points thrown away that way | 200 — **16.7 per game** |

Two notes on provenance, because this document was started from a handoff and
the handoff's numbers do not all replicate. The harm figure does: 20.7% here
against 19% handed over. The **rate** does not — I measure 0.75 firings per
player-turn, not the ~3.2 I was given, i.e. ~31 per game rather than ~129. The
mechanism in the handoff (limit 2 under Despotism against a draw of up to 3)
overstates the churn, because step 4 draws `min(3, military_actions
remaining)` — a bot that spends its military actions draws fewer than 3 — and
because cards also leave the hand by being played. It is still a decision on
three quarters of all player-turns, with four options on average, so nothing
about the conclusion changes; but the number quoted here is the measured one.

### 2.3. The fix

The machinery already existed and was simply never invoked: `_q_discard_military`,
the `push_choice` tag `discard_military`, its resolver `_c_discard_military`,
and BookBot's preference function for the tag. Only the end-of-turn caller was
missing.

The one structural problem is that `end_of_turn` is a phase transition, not an
action: `game.end_turn` ran the whole §6.6 sequence and then advanced the turn,
with no way to suspend in the middle. So:

* `economy.end_of_turn` returns **False** when step 1 pushed a choice and the
  sequence is suspended (steps 2–5 have not run), **True** when it completed.
  Step 1 is idempotent — it re-reads the hand limit — so *re-entry is the whole
  resume mechanism*.
* `game._resume_end_turn` queues an `end_of_turn` deferred item when it gets
  False. `apply_pending` already drains the queue once a decision resolves, so
  `interact._q_end_of_turn` lands back in `_resume_end_turn` and the sequence
  continues — possibly suspending again for the next discard.
* The turn does **not** advance while the decision is outstanding, so
  production, the uprising check and the hand-off all stay strictly after the
  discard, which is what the rulebook's "the next player may start as soon as
  you finish discarding" requires.

`push_choice(auto=True)` still resolves a one-option choice without a decision,
so a hand of five copies of one card discards silently — there was nothing to
choose between.

### 2.4. What policy each bot gets, and why it is mostly not a new policy

The important property is that **four of the five bots need no new code**: they
already score `("choose", i)` by cloning the state, applying the move and
asking the evaluator they already use. A policy derived that way cannot drift
from the bot's own valuation; a hand-written table can.

| bot | how it answers the new choice | source of the valuation |
|---|---|---|
| WeightedBot | clone + apply + `weighted.evaluate` (`weighted.py:1434`) | its own weight vector |
| QuiescentBot | same, then drains pending to quiescence (`quiescent.py:356`) | same evaluator |
| PlanBot / NeuralPlanBot | pending decisions take the 1-ply path (`plan.py:174`) into `evaluate` | same evaluator |
| NeuralBot | clone + apply + `encode` + value net (`neural_bot.py:74`) | the network |
| BookBot | its existing hand-written tag table (`book.py:795`), written for this tag years before it was ever invoked | itself |

BookBot's table was already correct for this decision (pitch events first at
3.0, keep tactics at 0.0, bonus cards at 0.4) and is left alone.

#### 2.4.1 The one place a policy had to be supplied: the tie-break

`weighted.evaluate` sees the military hand only through `hand_mil_value`, a sum
of `age + 1` — every military card of an age is interchangeable to it. That is
a documented blind spot ([`docs/CARD_BLINDNESS.md`](CARD_BLINDNESS.md#3-the-blind-spot-that-remains-written-down) §3 item 5: `hand_potential`
walks `hand_civil` only, so `_card_yields` is never called for a tactic, war,
aggression, territory or bonus card). Same-age options therefore **tie**, and
every argmax in this project resolves a tie to the lowest index.

Option order was `sorted(set(hand))` — alphabetical. Under a tie that means the
discard is chosen by spelling, which would pitch `Military Bonus (defense 6 /
colonization 3)` ahead of a spent event on nothing but the letter M. That is
not FIFO and it is not better than FIFO; it is arbitrary.

`interact.discard_options` therefore orders the options **least defensively
useful first**, using `defense_points` — the same arithmetic `_defense_move`
uses to resolve an actual defence (§5.4.4: bonus cards 2/4/6, every other
military card the flat +1 of a face-down card). `_defense_move` now calls it,
so the two cannot disagree. The fallback becomes "pitch the card that defends
least", which is derived from the engine's own combat rules rather than
invented, and any bot that *can* discriminate overrides it. It also gives
BookBot a free improvement it did not have to be told about: within its `bonus`
bucket the defence-2 card now goes before the defence-6 one.

This is deliberately **not** a new evaluator feature. Adding `defenseBonus` to
`_card_yields` or a defence term to the feature vector would change every
position's score, not just the discard, invalidate the cached pool weights, and
collide with the card-pricing lane that owns those files.

#### 2.4.2 Does it interact with the war/aggression machinery?

Yes, in the direction you would hope and not by much. `quiescent.war_value`
and `plan._score` price a *declared war* at leaf nodes; the aggression path
prices defence through `start_defense`/`finish_aggression`, which spend cards
out of the same hand. The value of holding a defence card is therefore already
priced at the leaf **when an aggression is on the table** — what was missing is
that the engine would take that card away from you before the attack arrived.
The A/B in §2.6 reports the defence outcomes directly.

### 2.5. Digests

All eight fingerprint arms move. That is expected and correct: turning a forced
FIFO discard into a real decision inserts `("choose", i)` moves into the move
stream of every game that has a military deck, so the game log — which is what
`perf_check` hashes — changes for every bot, including the ones that do not
evaluate through `weighted.py`. See `tools/gate.sh` for the before/after table
and the attribution.

### 2.6. Result: a well-powered null on strength, and a decisive one on behaviour

`tools/discard_ab.py`. Both arms run the **same fixed engine** and the same
weight vector (`analysis/frozen/champion_2p.json`); arm B answers every
`discard_military` choice the way the old engine did — pitch the oldest card in
hand. So the duel isolates the *policy*, not the plumbing, and it is a single
in-process head-to-head rather than two builds compared across runs. 600 games
/ 300 deals, 6 disjoint seed blocks, the FIFO arm played in each seat in turn.

#### 2.6.1 Strength

Pooled over the six blocks with `experiments.paired_stats` (block-clustered,
K=6, so the critical value is `t₅ = 2.571`, not 1.96):

| | estimate | z vs null | p |
|---|---|---|---|
| win share | **50.83% ± 3.74pp** | +0.57 | 0.57 |
| culture margin | **+0.88 ± 1.85** | +1.21 | 0.23 |

A null, leaning very slightly positive. Block SE on the win rate is 1.45pp, so
this excludes effects larger than about **4pp** at 80% power: it is a
well-powered null for a large effect, not an underpowered shrug.

#### 2.6.2 Behaviour — and this is not a null at all

| counter (evaluator arm, 6 blocks) | value |
|---|---|
| discard decisions faced | 7018 |
| chose differently from FIFO | **4364 (62.2%)** |
| kept a better defender than FIFO would have | **2409** |
| pitched a better defender than FIFO would have | **8** |
| defence points discarded | 9497, against 17159 under FIFO — **44.7% less** |

The policy does exactly what the rule is for, on nearly two thirds of all
firings, with a 300:1 asymmetry in the right direction. The eight
counter-examples are not a bug: the evaluator is free to prefer a card for
reasons other than defence, and eight times in seven thousand it did.

#### 2.6.3 Why the strength result is flat, and it is not the rule's fault

| | over 600 games |
|---|---|
| aggressions played, both arms | **34 — 0.057 per game** |
| aggressions successfully defended | **0** |

**The defence channel these bots would be paid through is essentially
absent.** The frozen 2p champion attacks about once every eighteen games, and
in 600 games not one aggression was ever held off. Keeping your best defensive
card cannot be worth measurable culture in a population that neither attacks
nor successfully defends, so a flat A/B here is a statement about the
*population*, not about the rule or the policy. §2.4.2's question — does this
interact with the machinery that prices defence at leaf nodes — has an
empirical answer at 2p: it cannot, because that machinery almost never runs.

Per Paul's standing rule, correct modelling lands regardless of measured
strength, and this is a rules violation. But the result is not "shelve it and
hope": the behavioural counters show the fix is live and pointed the right way,
and §2.6.4 says what would actually test it.

#### 2.6.4 What this does not measure

* **2p only.** Aggression is rarer at 2p than at 3p/4p and pacts do not exist
  there at all. The obvious follow-up is the same A/B at 3p, where the defence
  channel is more likely to be live. Not run here.
* **One weight vector.** The frozen 2p champion. A vector that valued strength
  more would attack more and could pay differently.
* **Not a retrained champion.** This is the frozen vector playing under a
  corrected rule, not a champion trained with the decision available. A trainer
  that can now *keep* a defender might learn to use one.

## 3. The victor of a War over Technology chooses (2026-07-30) (merged from the former `WAR_OVER_TECHNOLOGY.md`, 2026-07-31)

[`docs/SCORE_AUDIT.md`](SCORE_AUDIT.md#38-war-over-technologys-alternative-spoil-is-unimplemented) §3.8 left one of the 23 card types short of exact, and
it was the only one where the shortfall was not a wrong number:

> *"The victor takes science equal to the strength advantage, **or takes
> special (blue) technologies of the same total cost**."* `resolve_war`
> always takes science. `orTakesSpecialTechnologiesOfSameTotalScienceCost`
> is the second effect key in the data with no reader.

A player decision that does not exist. This is that decision.

Everything below is the **2015 base game, "A New Story of Civilization"**. No
expansion rule is involved, and §3.4 says which sources were rejected as first
edition or expansion.

---

### 3.1. The rule, from the primary sources

**The card.** The digital edition's own card text, as transcribed in
`data/cards_military_actions.json` and confirmed verbatim in ~40 archived
game journals under `sources/bgo/`:

> "The victor takes science equal to the strength advantage from the defeated
> civilization. **Special (blue) technologies can be stolen instead equal to
> their cost.**"

BoardGameArena splits the same card into two fields
(`sources/bga_throughtheages_material.inc.php:3172-3173`):

> `'loser' => "Loses science equal to strength advantage of winner"`
> `'winner' => "Produces same amount; Special (blue) technologies can be
> stolen instead of science equal to their cost"`

**The rulebook.** The card text says *what*; the official Code of Laws says
*how*. `sources/cge_code_of_laws.pdf` p.3, "Resolve a War" — this is the
whole paragraph the implementation is built from:

> "If one player has a higher strength than the other, that player is the
> victor and the other player is the defeated civilization. The difference
> between their strengths is the **strength advantage**. Follow the text on
> the card.
> − If the victor steals a special technology, the victor takes the card
>   **from the defeated civilization's play area** and puts it into his or her
>   own play area.
> − A player **cannot steal a special technology that is the same as one he
>   or she already has in play or in hand.**
> − If you steal a special technology of the same type as one that you have
>   in play, you **keep the higher level card in play and discard the other.**
> If the players have same strength, the war resolves with no effect."

**The FAQ.** `sources/faq_v15.pdf` p.8 settles the two questions the card
leaves open — whether it is all-or-nothing, and what caps it:

> "**War over Technology:** As long as you win enough Science points you can
> always choose to take **some or all** of them in blue Special Technologies.
> *Exception:* you are not allowed to choose a Technology card which you
> currently have in play or in your hand. The attacker **cannot take more
> Science points than the loser has to lose** (although the Science points he
> has available to lose also includes the Science costs of any blue Special
> Technology cards he has in play which the attacker does not have); and the
> loser indeed does lose actual Science points (as well as virtual Science
> points in the form of stolen Special Technologies)."

**Printed cost, not the discounted cost.** Code of Laws p.4: *"If the effect
refers to the cost of a card, use the cost printed on the card, ignoring any
modifiers."* So `techCost`, not `effects.tech_cost`.

**Either side can be the victor**, so the decision is not always the turn
player's. `sources/faq_v15.pdf` p.16: *"Wars: Either player can win a War."*
The engine already had this right; the choice inherits it.

#### 3.1.1 Answers, one line each

| question | answer | source |
|---|---|---|
| Taken from where? | the **loser's play area** | CoL p.3 |
| One card or several? | **several**, mixed freely with science | FAQ p.8 "some or all" |
| What is "cost"? | the **printed** science cost | CoL p.4 |
| Exactly equal, or at most? | **at most** the strength advantage | FAQ p.8 |
| Who chooses? | the **victor** (contrast: population loss is explicitly the loser's choice, FAQ p.16) | CoL p.3, FAQ p.8 |
| Nothing to take? | just take the science; no decision arises | FAQ p.8's cap, and the parallel War over Territory ruling |
| Does the loser lose the card? | **yes**, even when the victor must discard it | CoL p.3 |

#### 3.1.2 The arithmetic, checked against real games

Nothing here was reasoned into place. The archived digital-edition journals in
`sources/bgo/` contain resolved War over Technology spoils that sum **exactly**
to the strength advantage, mixing cards and science:

| journal | advantage | taken |
|---|---|---|
| `7523662.tsv` 201-206 | 26 − 14 = 12 | Code of Laws (6) + Cartography (4) + 2 science |
| `7522949.tsv` 395-398 | 24 − 10 = 14 | Strategy (8) + 6 science |
| `7521515.tsv` 585-589 | 27 − 14 = 13 | Strategy (8) + 5 science |
| `7522962.tsv` 537-540 | 26 − 17 = 9 | Architecture (6) + 3 science |
| `7522427.tsv` 159-163 | 16 − 7 = 9 | Code of Laws (6) + 3 science |
| `7523466.tsv` 482-484 | 23 − 19 = 4 | Masonry (3) + 1 science |
| `7522967.tsv` 418-421 | 25 − 18 = 7 | Masonry (3) + 4 science |

And one where the cap bit (`7523074.tsv` 221-225): advantage 19, but the loser
held 9 science and Code of Laws, so the victor took all 15 there were and no
more. Also `7522785.tsv` 245-248, where the **defender** (strength 22 against
21) collected.

#### 3.1.3 One place the rules are genuinely ambiguous, and the reading taken

Code of Laws p.3 says a player cannot steal a technology *"that is the same as
one he or she already has in play or in hand"*. "The same as" could mean the
same **card** or the same **icon**. The reading taken is **the same card**,
because the FAQ restates the identical exclusion as *"a Technology card which
you currently have in play or in your hand"* — a card, not a category — and
because the very next bullet in the Code of Laws would be dead text under the
icon reading: it tells you what to do when you steal *"a special technology of
the same type as one that you have in play"*, which the icon reading would have
forbidden outright. Two sentences that only both do work under the card
reading. Flagging it rather than choosing silently, as asked.

---

### 3.2. The implementation

**`engine/events.py:resolve_war`** stops taking the science itself. It emits
the result line as before and then hands the spoils to `interact`, gated on
the **card's own effect key** rather than on the spoils kind:

```python
if eff.get("orTakesSpecialTechnologiesOfSameTotalScienceCost"):
    interact.war_tech_spoils(state, victor, loser, adv, rng)
else:
    interact.take_war_science(state, victor, loser, adv)
```

That key now has a reader, so it comes off the allow-list in
`tests/test_score_audit.py:TestEveryEffectKeyIsRead`. That test is the reason
the gap was visible at all, and removing an entry from it is the durable form
of "this is fixed".

**`engine/interact.py`** gets the decision, on the machinery that already
exists — `push_choice`, the pending stack, `_CHOICE` dispatch — and no second
mechanism:

* `war_tech_options` builds the offer straight from §3.1: blue cards in the
  loser's **play area**, minus anything the victor holds in play or in hand,
  minus anything whose **printed** cost exceeds the remaining advantage.
* `war_tech_spoils` offers **one** steal at a time and re-offers with the
  advantage reduced by what the card cost — which is how "some or all" and
  the mixed sums in §3.1.2 fall out without any special case. The recursion is
  `_c_take_row`'s, verbatim in shape.
* When nothing is stealable it takes the science with **no decision at all**.
  This matters for the cost of the change: the decision does not appear in
  every war, or even in every War over Technology, but only in the ones where
  a steal is actually legal.
* `"science"` is **option 0**, deliberately. Every argmax in this project
  breaks a tie to the lowest index, so a bot that cannot tell the options
  apart falls back to exactly the engine's pre-change behaviour.
* `_steal_special_tech` moves the card by calling
  `actions.put_special_in_play` — the same one-per-icon placement the develop
  path uses (§7.6), which is *also* what Code of Laws p.3 states for a steal.
  One implementation, so the two cannot disagree. It is not a develop: no
  science is paid and `effects.on_develop` (Leonardo / Newton / Einstein)
  does not fire. `on_leave_play` / `on_enter_play` do fire on both sides,
  because the card really does change play areas.

**`engine/game.py:start_turn`** had to learn to wait. The decision arrives
inside the start-of-turn sequence, and a stolen Warfare / Strategy / Military
Theory changes the victor's **military actions** — which is precisely what
`_auto_skip_politics` reads to decide whether passing is the only political
option. Answering that question before the spoils are taken would deny the
victor a politics phase it is owed. So when `resolve_war` leaves a decision
pending, the auto-skip test is deferred behind it as an `auto_skip_politics`
queue item, resumed by `interact._q_auto_skip_politics` once the stack drains.
`state.pending` was measured empty on **all 3737** arrivals at that line across
the fingerprint's 33 games, so this branch is unreachable except through the
new decision.

No new suspend/resume mechanism was invented. `push_choice` + the deferred
queue + idempotent re-entry is `economy.end_of_turn`'s pattern from
§2 above (the former `docs/MILITARY_DISCARD.md`), and it is reused rather than duplicated.

---

### 3.3. What policy each bot gets

**Four of the five need no new code, and that is the point.** WeightedBot,
QuiescentBot, PlanBot/NeuralPlanBot and NeuralBot all already score
`("choose", i)` by cloning, applying and asking the evaluator they already
use, so their policy for this choice is **derived from their own valuation**
and cannot drift from it. Same result as the military-discard lane (commit
`1c08790`), for the same structural reason.

That is only worth anything if the evaluator can actually *see* the
difference, so it is pinned as a test rather than asserted:
`tests/test_combat.py:test_the_evaluator_can_see_the_difference_between_the_options`
applies each branch and requires `weighted.evaluate` to return different
numbers. Stealing `Code of Laws` buys a civil action; 6 science does not.

**BookBot** does no lookahead at all and needs a preference. It is built out
of the tables the book already has, not out of new opinions:

* both branches are paid for from **one** budget, so they are compared **per
  science point** — science is the numeraire at 1.0 a point, and a card
  costing `techCost` has to beat `techCost` science;
* a card's value is the book's existing `SPECIAL_RANK` via `_card_value`, so
  the preference cannot drift from what the book pays for the same card at
  the row;
* a steal that would **not** upgrade an icon the book already fills scores
  0.35 — denial only — because Code of Laws p.3 discards the loser of the
  level comparison, so taking a card sideways or downwards puts nothing in
  our play area and only takes it out of theirs.

**Every bot's search under-declares this war, permanently.** See §3.5.

---

### 3.4. Sources rejected

Two archived sources describe this card and are **first edition (2006)**, not
the 2015 base game. Neither was used:

* `sources/vassal_NewTTA_2.49_buildFile.xml:701` — despite "NewTTA" in the
  filename. Line 700 defines a card **"War over Resources"**, which does not
  exist in 2015; line 702 gives War over Territory as 1 token per **3** points
  of advantage against 2015's **5**; and every war's declaration text lets
  both sides **sacrifice units**, which 2015 forbids outright (CoL p.3, FAQ
  p.16).
* `sources/hypercheat.txt:64-67, 83-86` — "Offense may sacrifice units to
  double their strength", and line 20 sets aside **4** war cards from Military
  Deck II where 2015 has two types.

`sources/namu_military.txt:66` marks its own "Hybrid Wars" entry as expansion
content; ignored. `sources/faq_v15.pdf` covers expansion cards elsewhere in
the document, but the Wars ruling on p.8 and the "Wars:" line on p.16 are
base-game and nothing in either passage is expansion-specific.

`sources/bgg_154670_card_reference_v109.pdf`, which would have been the ideal
source, is a **4-page image-only PDF** — zero extractable text. There is no
photographic transcription of the physical 2015 card face anywhere in
`sources/`. The digital edition's card string (§3.1) is the closest thing to
one, and the Code of Laws supplies everything it leaves out.

---

### 3.5. A named limitation, so nobody has to rediscover it

`interact.settle_war_spoils` settles a lookahead's war by taking the
remainder as **science**. That is a sound lower bound — it is exactly how the
war was priced before this choice existed, and it beats the alternative of
scoring a position where the war has been fought and paid nothing at all.

**It is also a permanent, one-sided bias in search.** Every bot that prices a
declared `War over Technology` — QuiescentBot, PlanBot through the shared
`war_value`, NeuralPlanBot through `_leaf_enc` — now sees the **floor** of
that card and never its ceiling. So the bots will keep under-declaring this
war in exactly the positions where the choice was worth implementing: the ones
where the loser has a fat blue technology to take.

This matters for how the next person reads a null. **Anyone who measures the
choice and finds nothing has measured this lower bound, not the choice.** The
fix, when someone wants it, is to price the best affordable option from
`war_tech_options` instead of the remainder — at the cost of a card valuation
inside a function that runs on every beam node of every candidate move.

---

### 3.6. Conduction, measured before the A/B

`tools/wartech_census.py` counts the four conditions of the conjunction
separately, so a null can be attributed to the step that actually failed
rather than to "wars are rare" in general:

1. a war is declared and resolves at all,
2. it is `War over Technology` and not one of the other two,
3. it is not a draw, and
4. the loser holds a blue technology the victor may take, within budget.

<!-- CENSUS -->

---

### 3.7. The prediction, recorded before measuring

Written down before any game was played, and reproduced here unedited:

1. **NULL at every seat count**, with the mechanism named in advance:
   **conduction**, not indifference. Aggressions run 0.303/game at 2p,
   0.870 at 3p and 3.997 at 4p under real search; war *declarations* are
   rarer still, `War over Technology` is one of two Age II wars, and §3.6's
   condition 4 then has to hold on top of that.
2. Even where it fires, the marginal value is small **by construction**: the
   victor swaps `k` science for a card whose printed cost is `k`, so the
   delta is (card − its own price), a few science-equivalents.
3. A falsifiable side-bet on the digests: all eight gate arms move **if** any
   of the fingerprint's 135 games contains a resolved War over Technology
   with a stealable blue technology; if none does, **no** arm moves — and
   that outcome is itself the conduction measurement, not a failed change.

<!-- RESULT -->

---

### 3.8. Verdict

[`docs/SCORE_AUDIT.md`](SCORE_AUDIT.md)'s table goes from **22 of 23 exact** to **23 of 23**,
and the war row from "2 exact, 1 partial" to exact. This lands on correctness.
It is a rule the engine was not implementing, the cost — a decision point in
some wars — was accepted deliberately, and §3.5 and §3.7 say in advance what it is
and is not expected to buy in strength.

## 4. Why the champions never play pacts (and almost never colonize) (merged from the former `PACTS_DIAGNOSIS.md`, 2026-07-31)

Status: COMPLETE — diagnosis confirmed, fixes landed, **measured** (see
"Did the fix work?" at the bottom).
Date: 2026-07-26

**One-line result: pacts and colonies are fixed; aggression and war are
not.** Pacts went from 0.00 to 1.61 (3p) / 3.21 (4p) offers per player-game
and colony bids from 0.08 to 2.28 per player-game at 3p, but wars are still
*exactly* zero and aggressions did not move at all. Numbers, sample sizes
and the control arm are at the end of this file.

**Summary: it is (2) a bot blind spot, not an engine bug.** Pact moves are
generated and legal in 16% of politics decisions; the champions never take
them because a 1-ply evaluator cannot see any move whose payoff is deferred
to another player's decision, and ties break to the do-nothing option.
Colonies are the same failure plus two aggravating causes. A third,
independent bot bug was found on the way (wrong evaluation perspective on
other players' decisions).

### Verdict (pacts): **BOT BLIND SPOT, not an engine bug.**

`offer_pact` moves *are* generated, and often. Instrumented 4 mirror
self-play games at 3 players with the 3p champion weights
(`experiments/champion_3p.json`):

| quantity | value |
|---|---|
| politics decisions | 218 |
| decisions where `offer_pact` was in `legal_moves` | **35 (16%)** |
| per game: politics decisions with a pact available | 8, 14, 11, 2 |
| `offer_pact` moves actually chosen | **0** |

So pact cards reach hands, the politics phase offers them, the 2p removal
logic is correctly *not* firing at 3p, and the offer/accept/refuse flow is
reachable. The engine is fine. The bot simply never scores a pact above
`pol_pass`.

### Why the bot can never choose a pact (mechanical, not a tuning accident)

Both bots are **1-ply**: `pick()` copies the state, applies the candidate
move, and evaluates the resulting state (`engine/bots/__init__.py:171-195`,
`engine/bots/weighted.py` `WeightedBot.pick`).

`offer_pact` does *not* put a pact into play. `engine/actions.py:979-992`
(`_h_offer_pact`) does exactly three things:

1. `p.hand_military.remove(name)` — the card **leaves your hand**,
2. sets `politics_done` / `phase = "actions"`,
3. `interact.push_choice(state, target, "pact_offer", ...)` — a *pending*
   choice on the **other** player.

The pact object is only created later, in the partner's choice handler
`engine/interact.py:217-228` (`_c_pact_offer`), on `accept`. So in the
trial state the deciding bot evaluates:

* `pacts` feature (`engine/bots/weighted.py:182`, weight `0.5` by default)
  is **unchanged at 0** — the pact does not exist yet;
* `hand_military` and `hand_mil_value` (`weighted.py:203-206`) have gone
  **down** by one card;
* nothing else moved at all.

With any positive hand weight, `offer_pact` is therefore **strictly worse
than `pol_pass` in every position, by a constant**. This is visible in the
probe: every `offer_pact` variant scores identically (same value for side
A, side B and each possible partner — the evaluation is completely blind to
who the partner is and what the pact does), and always below `pol_pass`:

```
round 20, chosen ('prepare_event', 'Impact of Happiness')  160.878
          ('pol_pass',)                                    157.913
          ('offer_pact','Loss of Sovereignty',0,'A')       156.809
          ('offer_pact','Loss of Sovereignty',0,'B')       156.809   <- identical
          ('offer_pact','Loss of Sovereignty',1,'A')       156.809   <- identical
```

Direct proof — diffing the feature vectors of the two successor states of
the *same* position, from the mover's own seat, with the 3p champion
weights:

```
move ('offer_pact', 'International Tourism', 0, '')
feature diff  pol_pass -> offer_pact:
    hand_military   6 -> 5
    hand_mil_value 21 -> 17
weighted delta: -1.10445        # every other feature identical
```

Two features move, both downward. There is no path by which any pact can
ever be chosen.

The champion's `pacts` weight is dead code: no reachable 1-ply successor
state ever has a nonzero `pacts` count for the *mover*, so the hill climb
has never been able to select on it. (It can be nonzero for a player who
*accepted* a pact — but accepting is a `choose` move, and the same 1-ply
horizon applies to whether accepting looks good.)

`GreedyBot`'s 19-feature vector (`engine/bots/__init__.py:80-110`) has no
`pacts` feature at all, so for greedy it is doubly hopeless.

#### The same horizon problem, but worse: aggressions

Note `aggression` is 0.03/game at 3p and 0.11/game at 4p, and `war` is
**0.00** — for the same structural reason: `_h_aggression`
(`actions.py:972-976`) also just pushes a pending defence choice, so the
attacker's 1-ply lookahead sees the military card leave hand and no gain.
The whole politics phase has collapsed to `pol_pass` (9.98/game at 3p,
18.38/game at 4p) plus `prepare_event`, which is the *only* politics move
whose reward (`p.culture += level`, `actions.py:964`) lands immediately
inside the mover's own trial state. That is a strong confirmation of the
diagnosis: the bot plays exactly the politics moves that pay off within
one ply and none of the ones that don't.

### Recommended fixes, ranked by risk

**1. (lowest risk, highest value) Resolve deferred self-choices during the
1-ply trial, or add a "pending-offer credit" term.**
The cleanest minimal version: in `features()`, count pacts the player has
*offered and not yet had resolved* alongside pacts in play, i.e. read
`state.pending` for a `pact_offer` whose `ctx["owner"] == idx` and credit
it (discounted, e.g. 0.5x, for the refusal risk). ~10 lines in
`engine/bots/weighted.py`, no engine change, immediately makes the existing
`pacts` weight live and hill-climbable.

**2. Add pact-quality features, not just a count.**
`pacts` as a bare count cannot distinguish `Peace Treaty` from
`Loss of Sovereignty` (which costs Player B culture). Add features derived
from the pact's own effect block for the side the mover would take —
e.g. `pact_strength_gain`, `pact_culture_gain`, `pact_science_gain`,
`pact_food_gain`, `pact_blocks_attack` — computed by applying
`effects._pact_blocks` for the offered side. Medium effort, medium risk
(new feature keys need to be added to weight files / defaults).

**3. (same fix generalises) Make the accept/refuse choice informed.**
The partner's `choose` move is already evaluated at 1 ply *after* the pact
exists, so accepting is at least visible — but verify the accept branch
isn't being systematically refused for the same hand-value reason. Cheap to
check once fix 1 lands and offers start happening.

**4. (higher risk, engine change — do NOT do this lightly)**
Making `_h_offer_pact` optimistically place the pact and remove it on
refusal would make it 1-ply-visible, but it changes engine semantics and
would break the fingerprint/perf_check determinism. Not recommended;
fix it in the evaluator, not the rules.

**Do not "fix" this by tuning weights.** No weight value can make a move
that produces a strictly-dominated successor state get picked.

### Impact statement

The champions have been trained on a game in which the entire diplomacy
and aggression layer never fires. That is not an engine correctness bug —
the rules are implemented — but it *is* a training-distribution bug: the
78 weights were optimised in a world where politics is "pass or seed an
event", so any weight relating to pacts, colonies, aggression defence or
war is untrained noise, and the derived human-facing advice in
[`docs/HEURISTICS.md`](HEURISTICS.md) cannot say anything about the political game.

### Colonies

#### Verdict: **SAME ROOT CAUSE (1-ply invisibility + tie-break), plus a
second, 4p-only cause upstream. Still not an engine bug.**

The colonization auction is implemented and reachable
(`engine/interact.py:508-572`). Probe of 5 mirror 3p games with the 3p
champion:

| quantity | value |
|---|---|
| auction decisions | 16 |
| ...with 3 bidders still active | 3 |
| ...with 2 bidders still active | 5 |
| ...with 1 bidder still active | 8 |
| `bid` chosen | **1** |
| `bid_pass` chosen | 15 |

##### Cause A — a bid is *literally invisible* while anyone else is still in

`pending_moves` for an auction returns `[("bid_pass",), ("bid", 1), ...]`
(`engine/interact.py:47-53`) — **`bid_pass` is index 0**. Applying `("bid",
n)` when other bidders remain only mutates the `pend` dict
(`_auction_move`, `interact.py:522-542`); no player state changes. The
feature vector is built purely from player state, so **every bid evaluates
to exactly the same number as passing**, and `pick()` breaks ties with
strict `>` (`engine/bots/__init__.py:191`, `weighted.py` likewise), so the
first move — `bid_pass` — always wins. Directly observed:

```
round 12  Inhabited Territory (I)  3 bidders active
  ('bid_pass',) 102.474   ('bid',1) 102.474   ('bid',2) 102.474  ...  ALL EQUAL
round 15  Historic Territory (II)  3 bidders active
  ('bid_pass',) 95.997    ('bid',1) 95.997    ('bid',2) 95.997   ...  ALL EQUAL
```

Because the *first* bidder can never see value in bidding, everyone passes
and the territory goes to the past-events pile unclaimed. This is the same
class of bug as the pact one: a multi-step move whose payoff lands outside
the 1-ply horizon.

##### Cause B — even the visible case is rejected

When only one bidder is left active, a bid resolves the auction immediately
(`interact.py:537-541` → `colonize()`), so the colony *is* inside the trial
state and the evaluation is real. It still loses:

```
round 12  Inhabited Territory (I)   1 bidder active
  ('bid_pass',) 36.474   ('bid',1) 34.586   ('bid',2) 28.984   ('bid',3) 23.382
round 15  Developed Territory (II)  1 bidder active
  ('bid_pass',) 97.374   ('bid',1) 94.685   ('bid',2) 82.282
```

The sacrifice costs real weighted features — `workers` (1.76 at 3p) and
`unit_workers` per unit returned to the yellow bank, plus `yellow_bank`
(-0.28) and the knock-on `pop_cost`/`consumption` — while the gain is a
single `colonies` count feature. That trade is *modelled*, but with an
**untrained coefficient** (see below), and it ignores the colony's
permanent yield entirely except through the count.

##### Cause C (4 players only) — auctions never even start

3 full 4p games with the 4p champion: **zero auction decisions**, and only
16 `prepare_event` moves total. Territory cards only reach the board by
being seeded into the events deck with `prepare_event`
(`engine/actions.py:255-256, 960-969`) and then revealed. The 4p champion
has `hand_military = 0.908` (vs 0.504 at 3p), i.e. it values *holding*
military cards more than the culture `prepare_event` pays, so it passes
politics ~94% of the time and almost never seeds an event. No seeded
events → no revealed territories → no auctions. This is why 4p colony bids
(0.02/game) are even rarer than 3p (0.08/game).

#### The smoking gun: these weights were never under selection

```
              colonies   pacts     (BASE_WEIGHTS default: colonies 2.0, pacts 0.5)
champion 2p    3.311     0.625
champion 3p    2.000     0.644     <- colonies is EXACTLY the untouched default
champion 4p   -0.962     0.469     <- drifted NEGATIVE
```

The 3p champion's `colonies` weight is bit-for-bit the hand-written
default: thousands of hill-climb generations never once moved it, because
no game outcome ever depended on it. The 4p champion's went *negative*,
which is pure random drift on a feature that fires ~0.02 times per game.
Any advice in [`docs/HEURISTICS.md`](HEURISTICS.md) derived from these two coefficients is
noise, and should be marked as such.

### Recommended fixes for colonies, ranked by risk

**1. (lowest risk) Break auction ties toward action, or make bids visible.**
Two cheap options, in preference order:
   a. In `features()`, add an `auction_committed` term: if the top pending
      decision is an `auction` whose `high` is this player, credit the
      expected colony (e.g. `colonies + 1` discounted by the number of
      still-active rivals). ~8 lines in `engine/bots/weighted.py`, no
      engine change. This makes the *first* bid visible and therefore
      possible.
   b. Cheaper still but cruder: in `interact.pending_moves`, put
      `("bid_pass",)` **last** rather than first, so the tie-break falls to
      the smallest legal bid instead of passing. One-line change, but it
      changes the move ordering the fingerprint depends on
      (`tools/fingerprint.json`, `engine/perf_check.py`) — coordinate with
      whoever owns the engine.

**2. Replace the bare `colonies` count with yield-aware features.**
Territories differ hugely (`Historic Territory II` = +2 happy and 11
culture now; `Vast Territory II` = +4 yellow, -1 blue, 4 food). Derive
`colony_yellow`, `colony_blue`, `colony_happy`, `colony_strength` from
`permanentEffects` so the evaluator sees what it actually bought. The
immediate effects already land in the trial state, so those need nothing.

**3. Re-run the hill climb after 1 and 2, and reset `colonies`/`pacts` to
their defaults first** — the current 4p value of -0.96 is drift, and
carrying it into a run where the feature suddenly matters would start the
search in the wrong basin.

**4. Separately, check the 4p `hand_military` weight (0.908).** It is
plausibly a genuine optimum (military cards defend attacks), but combined
with cause C it means the 4p champion opts out of events, territories,
aggressions and pacts all at once. Worth an ablation: does forcing a lower
`hand_military` at 4p change the win rate?

### Rules check (no engine defects found)

Read `engine/actions.py:240-296`, `engine/actions.py:979-1003`,
`engine/interact.py:217-228` and `engine/interact.py:464-586` against
[`docs/RULES_SPEC.md`](RULES_SPEC.md) §5.9, §5.10, §11.1-11.5. Everything matched:

* pacts are gated to 3+ players (`actions.py:258`), and 2p decks drop them
  at build time via the `2p` copy counts in
  `data/cards_military_actions.json` — the 2p zero is expected, correct,
  and not evidence of anything;
* offering costs a political action but no MA (§5.9 / FAQ p.16) — matches;
* refuse returns the card to hand (`interact.py:227`) — matches;
* accepting replaces any previous pact in the owner's own area
  (`interact.py:222`, single-element list) — matches;
* auction order starts from the politics-phase player and goes clockwise
  (`interact.py:511`), passing is permanent, the last bidder must colonize
  (`interact.py:537-541`) — matches §11.2.

**One cosmetic deviation worth a follow-up (not the cause of anything
here):** `actions.py:258` gates on `len(state.active_players()) < 3`, which
is *dynamic*. In a 3-player game where someone resigns (§5.11), pacts
silently become illegal mid-game for the two survivors. The real rule is a
**setup** rule (remove pacts from the deck in a 2-player game), so the
gate should be on the number of seats, not the number of survivors. Low
impact (resign is 0.07/game) but it is a genuine rules mismatch.

### Third finding (found while verifying): WeightedBot scores other
### players' decisions from the WRONG player's point of view

`WeightedBot.pick` uses **`idx = state.current`**
(`engine/bots/weighted.py:357`). `GreedyBot.pick` correctly uses
**`state.decider()`** (`engine/bots/__init__.py:181`). They differ whenever
`state.pending` is non-empty and the pending decision belongs to somebody
other than the player whose turn it is (`engine/state.py:140-144`).

Measured over 5 mirror 3p games with the 3p champion:

| pending decision | total | evaluated from the wrong seat |
|---|---|---|
| `choice` (accept/refuse a pact, defend, lose_colony, annex, …) | 47 | **15 (32%)** |
| `auction` (colony bidding) | 16 | **10 (63%)** |

So the champion resolves most colony bids and a third of all interactive
choices by maximising **a rival's** position. The pact accept/refuse
decision (`_c_pact_offer`) is *always* one of these — the partner is by
definition not the current player — so even if fix #1 makes bots start
offering pacts, the accept side is scored backwards until this is fixed.

**Fix (trivial, do this first):** change `engine/bots/weighted.py:357` to
`idx = state.decider()`. One line. It will change self-play results, so it
invalidates the current champions and any fingerprint that covers
`WeightedBot` — but it is unambiguously a bug, and it is cheap to re-run
the climb. Note `rival_context(state, idx)` on the next line must use the
same `idx`.

### Bottom line

Neither zero-pacts nor near-zero-colonies is an engine bug. Both are the
same architectural limitation: **a 1-ply evaluator cannot see any move
whose effect is deferred to another player's decision**, and the tie-break
sends every such move to the do-nothing option. The consequence is
serious anyway — the champions were tuned in a game with no diplomacy, no
colonization and effectively no aggression, so the political half of
Through the Ages is untrained, and the `colonies`/`pacts`/aggression
weights in `experiments/champion_*.json` are unselected noise that should
not be read as advice.
