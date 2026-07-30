# The victor of a War over Technology chooses (2026-07-30)

`docs/SCORE_AUDIT.md` §3.8 left one of the 23 card types short of exact, and
it was the only one where the shortfall was not a wrong number:

> *"The victor takes science equal to the strength advantage, **or takes
> special (blue) technologies of the same total cost**."* `resolve_war`
> always takes science. `orTakesSpecialTechnologiesOfSameTotalScienceCost`
> is the second effect key in the data with no reader.

A player decision that does not exist. This is that decision.

Everything below is the **2015 base game, "A New Story of Civilization"**. No
expansion rule is involved, and §4 says which sources were rejected as first
edition or expansion.

---

## 1. The rule, from the primary sources

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

### 1.1 Answers, one line each

| question | answer | source |
|---|---|---|
| Taken from where? | the **loser's play area** | CoL p.3 |
| One card or several? | **several**, mixed freely with science | FAQ p.8 "some or all" |
| What is "cost"? | the **printed** science cost | CoL p.4 |
| Exactly equal, or at most? | **at most** the strength advantage | FAQ p.8 |
| Who chooses? | the **victor** (contrast: population loss is explicitly the loser's choice, FAQ p.16) | CoL p.3, FAQ p.8 |
| Nothing to take? | just take the science; no decision arises | FAQ p.8's cap, and the parallel War over Territory ruling |
| Does the loser lose the card? | **yes**, even when the victor must discard it | CoL p.3 |

### 1.2 The arithmetic, checked against real games

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

### 1.3 One place the rules are genuinely ambiguous, and the reading taken

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

## 2. The implementation

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

* `war_tech_options` builds the offer straight from §1: blue cards in the
  loser's **play area**, minus anything the victor holds in play or in hand,
  minus anything whose **printed** cost exceeds the remaining advantage.
* `war_tech_spoils` offers **one** steal at a time and re-offers with the
  advantage reduced by what the card cost — which is how "some or all" and
  the mixed sums in §1.2 fall out without any special case. The recursion is
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
`docs/MILITARY_DISCARD.md`, and it is reused rather than duplicated.

---

## 3. What policy each bot gets

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

**Every bot's search under-declares this war, permanently.** See §5.

---

## 4. Sources rejected

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
`sources/`. The digital edition's card string (§1) is the closest thing to
one, and the Code of Laws supplies everything it leaves out.

---

## 5. A named limitation, so nobody has to rediscover it

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

## 6. Conduction, measured before the A/B

`tools/wartech_census.py` counts the four conditions of the conjunction
separately, so a null can be attributed to the step that actually failed
rather than to "wars are rare" in general:

1. a war is declared and resolves at all,
2. it is `War over Technology` and not one of the other two,
3. it is not a draw, and
4. the loser holds a blue technology the victor may take, within budget.

<!-- CENSUS -->

---

## 7. The prediction, recorded before measuring

Written down before any game was played, and reproduced here unedited:

1. **NULL at every seat count**, with the mechanism named in advance:
   **conduction**, not indifference. Aggressions run 0.303/game at 2p,
   0.870 at 3p and 3.997 at 4p under real search; war *declarations* are
   rarer still, `War over Technology` is one of two Age II wars, and §6's
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

## 8. Verdict

`docs/SCORE_AUDIT.md`'s table goes from **22 of 23 exact** to **23 of 23**,
and the war row from "2 exact, 1 partial" to exact. This lands on correctness.
It is a rule the engine was not implementing, the cost — a decision point in
some wars — was accepted deliberately, and §5 and §7 say in advance what it is
and is not expected to buy in strength.
