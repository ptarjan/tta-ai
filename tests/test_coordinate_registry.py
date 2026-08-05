"""A coordinate must appear in EVERY registry it needs, and nothing checks that.

The bug this exists to prevent, in one sentence: **a weight that exists in one
registry and is missing from another is silently dead, and the tree is green.**

THE HISTORY, because every one of these was found by hand and none of them by a
test
-------------------------------------------------------------------------------
* `free_civil_action`, `resource_discount`, `restricted_resources` were in
  `DEFAULT_WEIGHTS` **and** spent by `card_potential` **and emitted by no
  feature vector**.  `evaluate` therefore never multiplied them by anything, no
  game could produce a gradient on them, and they read 0.0 on every champion --
  not weights the trainer zeroed, weights the trainer never had information
  about.  **16 of the 33 action cards priced at exactly 0.000.**  Fixed in
  `5ab4943`; nothing prevented a recurrence.
* `unit_strength_credit` and `tech_levels`: the feature existed, `evaluate`
  could read it, and the weight sat at 0.0 on all three champions for days.
  Priced and inert, and four separate audits walked past it.
* Units, then yellow/technology cards: `row_pressure` carries
  `if val <= 0.0: continue`, so a card class whose `card_potential` prices at or
  below zero is **invisible** -- the bot cannot see it exists.  That hid two
  whole card classes (`d8a2172`, `8b972ef`).
* `MARGIN_SCALE` was defined in `experiments/hillclimb_pool.py` (120.0) **and**
  `engine/bots/neural_net.py` (100.0): same name, same apparent job, different
  value, different actual job.  A genuine name collision nobody noticed.
* Three hand-set weight tables -- `weighted.BASE_WEIGHTS`, `bots.WEIGHTS`
  (live for GreedyBot, `culture_rate` 6.0 against 5.0), `book.V2_TUNABLES` --
  and nothing compared them.
* A vacuity trap: `tests/test_row_features.py` once dealt its probe card at a
  slot scored through a quantity that saturates at 1.0, so the guard could stop
  guarding without anything failing.
* `state.scoring_events` is declared, never written, never read back -- and it
  is a permanently-zero neural-net input on top.  `PlayerState.destroyed_
  wonders` is read by the take surcharge and never incremented.  Both were
  already in `docs/OPEN_ITEMS.md`, found by hand, with nothing to stop the next
  one.

THE INVARIANT
-------------
A **bijection across every registry a coordinate must appear in**, asserted in
BOTH directions -- the missing direction is always where the bug hides.  Each
registry pair is one test below.  There are FOUR registries, not two:

1. `weighted.features()`      the linear evaluator's feature vector
2. `weighted.DEFAULT_WEIGHTS` the coordinate it is paid through, plus the
                              champion/hall-of-fame vectors on disk
3. `weighted.card_potential`  what a card in hand is claimed to be worth
4. `neural_encode.encode()`   a THIRD, independent representation that imports
                              neither of the others and can therefore go
                              blind on its own

A quantity can be live in the linear evaluator and absent from the encoding, or
encoded and permanently zero, and before this file nothing failed either way.

THE RATCHET
-----------
Dead coordinates exist today.  They are enumerated in `KNOWN_DEAD`, each with
the registry it is missing from, why it is dead, and where the open item lives.
Every check is a **set equality** against that list, so the test fails in both
directions:

* something NEW joins the dead list  -> regression, the real guard;
* a listed entry is no longer dead   -> the entry must be deleted.

The second half is what makes this a ratchet and not a graveyard.  The list can
only shrink.

ANTI-VACUITY
------------
`tests/test_model_constants.py` is the model for the shape of this file, and
`tests/test_row_features.py`'s old trap is the model for what to be afraid of.
`CorpusIsNotVacuous` asserts the reachable-state corpus is non-empty, covers all
five ages, all three player counts, and a non-empty card row, with **numeric
floors** -- so a corpus that silently degrades to one trivial state fails rather
than passing everything.

See docs/COORDINATE_REGISTRY.md.
"""
from __future__ import annotations

import ast
import collections
import dataclasses
import glob
import json
import os
import random
import unittest

from engine import actions as A, cards as C, effects, game as G, state as S
from engine import bots as B
from engine.bots import board_yields as BY
from engine.bots import fastcopy
from engine.bots import book
from engine.bots import neural_encode as NE
from engine.bots import weighted as W

HERE = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))

#: Weight vectors on disk.  `experiments/league_state/` and
#: `experiments/hall_of_fame/` are the LIVE league's, untracked and absent from
#: a fresh clone; the other two globs are committed and must exist.
VECTOR_GLOBS = (
    ("experiments/champion_*.json", True),
    ("analysis/frozen/*.json", True),
    ("experiments/league_state/champion_*.json", False),
    ("experiments/hall_of_fame/*.json", False),
)

#: Modules that price a card.  The AST sweep walks the call graph from
#: `card_potential` through these two files only; everything else is a caller.
PRICING_MODULES = ("engine/bots/weighted.py", "engine/bots/board_yields.py")

#: EMPTY SINCE 2026-08-04.  This held the two A/B escape hatches (`horizon_age`,
#: `horizon_legacy`), which were deliberately absent from `DEFAULT_WEIGHTS` so
#: the trainer never emitted them.  Both were deleted with the rest of the A/B
#: apparatus -- nothing set them and no vector on disk carried them.  Kept as an
#: empty set rather than deleted because the four uses below are the ratchet
#: that says "a key not in DEFAULT_WEIGHTS is an ORPHAN"; an empty exemption
#: list makes that ratchet total, which is the point.  Re-populate ONLY if a
#: hatch comes back, and expect `test_model_constants` to want it too.
AB_HATCHES = frozenset()


# --------------------------------------------------------------------------
# The declared non-feature weights.
#
# `features()` is not the only emitter.  A weight may legitimately be a SCALE on
# a non-linear term that `evaluate` computes itself, or a CREDIT on how much of
# a board-derived card price to believe, or a SEARCH knob a bot reads outside
# `evaluate` entirely.  Those are not bugs -- but "not a feature" is exactly the
# shape of the `free_civil_action` bug, so the exemption has to be DECLARED,
# with what multiplies it, and it has to be checkable.
#
#   reader "evaluate"  -> `EveryParameterConducts` perturbs it and requires the
#                         evaluation to move on at least one corpus state.
#   reader "pricing"   -> a credit inside `card_potential`; same check, through
#                         `card_potential` rather than `evaluate`.
#   reader "search"    -> read by a bot outside `evaluate`; the AST sweep must
#                         find `w.get("<key>")` in `engine/bots/`.
#
# key -> (reader, what multiplies it)
# --------------------------------------------------------------------------
PARAMETERS = {
    # scales on non-linear terms `evaluate` computes itself
    "hand_potential": ("evaluate", "scale on weighted.hand_potential()"),
    "hand_mil_potential": ("evaluate", "scale on weighted.hand_mil_potential()"),
    "rival_hand_potential": (
        "evaluate", "scale on weighted.rival_hand_potential()"),
    "wonder_potential": ("evaluate", "scale on weighted.wonder_potential()"),
    "row_urgency": ("evaluate", "row_pressure()[0]"),
    "row_bargain_forgone": ("evaluate", "row_pressure()[1]"),
    "rival_desire": (
        "evaluate",
        "shape parameter inside row_pressure()'s rival model, not a term: it "
        "blends the uniform per-rival `reach` against _rival_desire()'s "
        "value-weighted competition.  It emits no feature of its own for the "
        "same reason `rival_take_share` does not -- it reweights a "
        "probability, and the thing it reweights is already reported as "
        "row_bargain_forgone"),
    "row_last_copy": (
        "evaluate",
        "scale on row_last_copy(): the row value that will never be dealt "
        "again, from engine.bots.counting.civil_outlook.  Priced through `w` "
        "(it multiplies card_potential) and non-linear in it, so it lives in "
        "evaluate for the same reason row_urgency does"),
    "tactic_gain": ("evaluate", "tactic_terms()[0], gated in evaluate"),
    "tactic_short": ("evaluate", "tactic_terms()[1], gated in evaluate"),
    "rival_take_share": (
        "evaluate",
        "MODEL PARAMETER inside rival_take_p, not a value term; a weight so "
        "the league can fit it (docs/MODEL_CONSTANTS.md 3.4)"),
    "hand_swap_extra": ("evaluate", "the spare-slot fraction in _hand_total"),
    "my_event_threat": (
        "evaluate",
        "scale on weighted.my_event_threat(): what the events I SEEDED MYSELF "
        "will pay out on the board as it stands, which is non-linear (it ranks "
        "the players) and priced through `w`, so it lives in evaluate for the "
        "same reason hand_potential does.  The linear half of the pair, "
        "`my_seeded_pending`, IS emitted by features()"),
    "rate_horizon": (
        "evaluate",
        "MODEL PARAMETER inside weighted.rate_multiplier, not a value term: "
        "how much of `rounds_left / mean rounds_left` to multiply the "
        "RATE_KEYS features (and their card marginals) by.  A weight so the "
        "league can fit it (docs/RATE_HORIZON.md)"),
    # credits: how much of a board-derived card price to believe
    "card_rate_credit": ("pricing", "credit on _Y_RATE yields"),
    "card_board_credit": ("pricing", "shared credit on board_yields"),
    "card_board_leader": ("pricing", "per-type offset on card_board_credit"),
    "card_board_government": ("pricing", "per-type offset"),
    "card_board_action": ("pricing", "per-type offset"),
    "card_board_wonder": ("pricing", "per-type offset"),
    "unit_strength_credit": ("pricing", "credit on the STATIC unit table"),
    "unit_tech_credit": ("pricing", "credit on unit tech_value()"),
    "tech_board_credit": ("pricing", "credit on tech_value()"),
    "action_board_credit": ("pricing", "credit on action_value()"),
    "gov_board_credit": ("pricing", "credit on gov_value()"),
    "build_fresh_credit": (
        "pricing", "credit on the build-one-fresh branch of tech_value()"),
    "restricted_resource_credit": (
        "pricing", "credit on the ring-fenced resource pseudo-feature"),
    "free_action_credit": ("pricing", "credit on the ordered action in action_value()"),
    "territory_credit": ("pricing", "credit on a territory's printed effect"),
    "bonus_card_credit": ("pricing", "credit on the three Military Bonus cards"),
    # search knobs, read outside evaluate
    "end_turn_bias": (
        "search",
        "weighted.WeightedBot.pick / quiescent; a move-quality filter, NOT a "
        "board feature.  DO NOT 'fix' -- see the comment on the default"),
}


# --------------------------------------------------------------------------
# THE RATCHET.  Every entry is a coordinate that is dead TODAY.
#
# kind:
#   "unpriced-feature"  spent by card_potential, emitted by no feature, so
#                       `evaluate` never pays for what the card buys
#   "inert-vector"      default 0.0 AND 0.0 in every on-disk vector carrying
#                       it, so it cannot change any decision as configured
#   "never-nonzero"     emitted by features() but exactly 0.0 on every state
#                       in the corpus
#   "invisible-class"   a whole card class `card_potential` never prices above
#                       zero, so row_pressure's `val <= 0.0` skip makes it
#                       unreachable forever
#   "stale-vector-key"  a key in a weight file on disk that DEFAULT_WEIGHTS no
#                       longer has
#
# name -> (kind, why it is dead, where the open item lives)
# --------------------------------------------------------------------------
KNOWN_DEAD = {
    # ---------------------------------------------- spent, never earned back
    "free_civil_action": (
        ("unpriced-feature",),
        "the STATIC table's spelling of a card's free civil action.  The live "
        "board path stopped using it in 5ab4943 -- action_value prices the "
        "free action at the civil_actions marginal evaluate itself uses -- but "
        "_card_yields still emits the key, so `action_board_credit` = 0.0, or "
        "any caller with no board, falls back onto a coordinate evaluate "
        "never pays for",
        "docs/OPEN_ITEMS.md: retire the static action table"),
    "resource_discount": (
        ("unpriced-feature",),
        "the same shape one effect over: `resourceDiscount` -> "
        "`resource_discount` in the static table, where the live path uses "
        "`resource_stock`.  NOT inert on the live champions -- the climber "
        "scatters onto it (mutate's 0.15 floor) and it random-walks, because "
        "no game can produce a gradient to pull it back",
        "docs/OPEN_ITEMS.md: retire the static action table"),
    "restricted_resources": (
        ("unpriced-feature",),
        "the same shape again: `resourcesForMilitaryUnits`.  Superseded on the "
        "live path by `restricted_resource_credit` x `resource_stock`, kept as "
        "the stateless answer, and random-walking on the champions for the "
        "same reason `resource_discount` is",
        "docs/OPEN_ITEMS.md: retire the static action table"),
    "defense_bonus": (
        ("unpriced-feature",),
        "a Military Bonus card's defence half is worth something precisely "
        "BECAUSE it is still in hand, so there is genuinely no board state for "
        "features() to mirror.  The consequence is real anyway: it is the only "
        "coordinate the three bonus cards have, which is why the whole `bonus` "
        "class is in the invisible-class list below.  Two entries, one defect",
        "docs/OPEN_ITEMS.md: price the military hand"),
    "gov_action_cost": (
        ("unpriced-feature",),
        "FOUND BY THIS TEST.  board_yields prices the civil-action pool a "
        "revolution burns at `gov_action_cost`, which features() does not "
        "emit -- while it DOES emit `civil_actions`, which is the same "
        "quantity in the same units.  Exactly the free_civil_action shape, "
        "and exactly the fix 5ab4943 applied one card type over",
        "docs/OPEN_ITEMS.md: price a revolution through civil_actions"),
    # -------------------------------- 0.0 on every COMMITTED vector carrying it
    "wonder_stages_left": (
        ("inert-vector",),
        "one of the three finish-discipline terms, all deliberately 0.0 so a "
        "trained vector was unchanged when they landed; the league was meant "
        "to price them and has not",
        "docs/CARD_BLINDNESS.md; docs/OPEN_ITEMS.md"),
    "wonder_turns_to_finish": (
        ("inert-vector",),
        "the second finish-discipline term, same history and same standing",
        "docs/CARD_BLINDNESS.md; docs/OPEN_ITEMS.md"),
    "wonder_stages_per_action": (
        ("inert-vector",),
        "Masonry and friends: stages per build action above the base of one.  "
        "0.0 everywhere it is carried, so the whole Masonry channel is off",
        "docs/CARD_BLINDNESS.md; docs/OPEN_ITEMS.md"),
    "wonder_overrun": (
        ("inert-vector", "never-nonzero"),
        "DOUBLY dead, and this is the pair worth reading together: the weight "
        "is 0.0 on every committed vector AND the feature is exactly 0.0 on "
        "every one of the corpus states, so neither half can wake the other.  "
        "docs/MODEL_CONSTANTS.md 6.1 already leans on the weight being 0.0 to "
        "argue the deal-rate fix is inert",
        "docs/OPEN_ITEMS.md: wonder_overrun never fires"),
    # -------------------------------------------- emitted, never non-zero yet
    "hand_limit": (
        ("never-nonzero",),
        "`Library of Alexandria` is the ONLY card in the game that grants a "
        "hand limit, and the bot completes 0.05 wonders per seat-game -- so "
        "this coordinate is one wonder completion in one of the six corpus "
        "games away from being live, and the government reprice re-rolled it "
        "away (295 of 2033 states before, 0 of 2004 after).  CORPUS-FRAGILE, "
        "not structurally dead",
        "docs/OPEN_ITEMS.md section 9.5: the corpus-pinned entries re-roll"),
    # ------------------------------------------------ whole classes invisible
    "class:tactic": (
        ("invisible-class",),
        "all 15 tactic cards price at exactly 0.0 even with every credit at "
        "1.0: `_card_yields` has no mapping for a tactic's unit requirement or "
        "its strength table, so the military hand cannot see one at all",
        "docs/OPEN_ITEMS.md: price the military hand"),
    "class:aggression": (
        ("invisible-class",),
        "10 of the 11 aggressions price at exactly 0.0 -- a one-shot steal has "
        "no printed production for the static table to read",
        "docs/OPEN_ITEMS.md: price the military hand"),
    "class:war": (
        ("invisible-class",),
        "all 3 wars price at exactly 0.0, from the same missing mapping",
        "docs/OPEN_ITEMS.md: price the military hand"),
    "class:pact": (
        ("invisible-class",),
        "all 10 pacts price at exactly 0.0: `pacts` and `pact_blocks_attack` "
        "are BOARD features, and nothing maps a pact card in hand onto them",
        "docs/OPEN_ITEMS.md: price the military hand"),
    "class:bonus": (
        ("invisible-class",),
        "all 3 Military Bonus cards price at exactly 0.0 because their only "
        "coordinate is `defense_bonus`, which is in the unpriced-feature list "
        "above.  Two entries, one defect",
        "docs/OPEN_ITEMS.md: price the military hand"),
    # ------------------------- declared state fields the engine never writes
    #
    # Dead in EVERY representation at once, which is why they are listed under
    # the FIELD and not once per registry.
    "scoring_events": (
        ("never-written",),
        "THE RECORDED CASE (docs/OPEN_ITEMS.md).  Declared at state.py:163, "
        "never written by anything in engine/, and read in exactly one place: "
        "neural_encode's `1.0 if state.scoring_events else 0.0` -- so it is "
        "also a permanently-zero neural input, which is the entry below",
        "docs/OPEN_ITEMS.md: dead state fields"),
    # `current_events_age` WAS HERE and is not any more.  It was found by
    # this test, declared, never written, and read in exactly one place --
    # neural_encode's third age one-hot, frozen on 'A' for the life of every
    # game.  `events._sync_current_events_age` now writes it (age of
    # `current_events[-1]`, the next card `reveal_current_event` will pop)
    # at every point the pile changes: initial deal, each reveal, each
    # recycle.  See docs/OPEN_ITEMS.md §9.3.
    # `caesar_double_politics_used` WAS HERE and is not any more.  It was
    # found by this test, checked against the rules as the entry asked, and
    # turned out to be the missing rule rather than the dead field: Julius
    # Caesar's "once per game, you may take two political actions in your
    # politics phase" was declared on the card, written off in
    # `weighted.DELIBERATELY_UNPRICED` as something "the rules engine
    # expresses", and expressed by nothing.  `actions._end_politics` writes it
    # now.  The list can only shrink; this is one of those.
    "used_leader_ability": (
        ("never-written",),
        "FOUND BY THIS TEST.  The generic once-per-game/turn leader flag, "
        "declared and referenced nowhere else; the leaders that have such an "
        "ability each carry their own flag instead",
        "docs/OPEN_ITEMS.md: dead state fields"),
    "culture_rate_extra": (
        ("never-written",),
        "READ by effects.compute (`s.culture += p.culture_rate_extra`) and "
        "written by nothing, so event-granted per-turn culture is a channel "
        "that exists and is always zero",
        "docs/OPEN_ITEMS.md: dead state fields"),
    "science_rate_extra": (
        ("never-written",),
        "the exact twin of culture_rate_extra, read by effects.compute and "
        "never written",
        "docs/OPEN_ITEMS.md: dead state fields"),
    "destroyed_wonders": (
        ("never-written",),
        "already recorded in docs/OPEN_ITEMS.md: read by the take surcharge "
        "(actions.py:90 and :124) and never incremented, so a destroyed "
        "wonder never raises the cost it is there to raise",
        "docs/OPEN_ITEMS.md: dead state fields"),
    "hidden_civil": (
        ("never-written",),
        "DELIBERATE and documented at state.py: it exists for the app harness "
        "(docs/APP_HARNESS.md), which mirrors a rival whose hand SIZE is "
        "public but whose contents were not transcribed.  Always 0 in a real "
        "game and in every self-play line -- listed so it is a stated "
        "exception rather than an unexamined zero",
        "none -- deliberate, docs/APP_HARNESS.md"),
    "hidden_military": (
        ("never-written",),
        "the military twin of hidden_civil, deliberate for the same reason and "
        "listed for the same reason",
        "none -- deliberate, docs/APP_HARNESS.md"),
    # ------------------------------- encoding slots that can carry no signal
    # `encode:global.current_events_age` WAS HERE and is not any more: it was
    # the one-hot of the field above, five slots frozen on 'A'.  Now that
    # `current_events_age` is written, the slot varies across the corpus.
    "encode:global.scoring_events": (
        ("constant-encoding",),
        "the flag on `state.scoring_events`, which is never written; 0.0 in "
        "every state of every game",
        "docs/OPEN_ITEMS.md: dead state fields"),
    "encode:row.cost_ladder": (
        ("constant-encoding",),
        "`_ROW_COST[i] / 3.0` is a POSITIONAL CONSTANT -- slot i always costs "
        "the same -- so all 13 slots are compile-time constants and the net "
        "absorbs them into its bias.  Harmless but it is 13 inputs that can "
        "never carry a bit; the cost only becomes informative relative to what "
        "is IN the slot",
        "docs/OPEN_ITEMS.md: the encoder's constant inputs"),
    "encode:me.present": (
        ("constant-encoding",),
        "the padding marker: 1.0 in every REAL player block by construction, "
        "0.0 only in the blocks this check deliberately excludes.  It is not "
        "dead -- it is the flag that tells the net a rival block is padding -- "
        "and it is listed because the constancy check cannot see the padded "
        "blocks it is informative in",
        "none -- structural, see docs/COORDINATE_REGISTRY.md on padding"),
    "encode:rival.present": (
        ("constant-encoding",),
        "the same padding marker in the rival blocks, and the same reason: "
        "it is 1.0 wherever this check is allowed to look",
        "none -- structural, see docs/COORDINATE_REGISTRY.md on padding"),
    # `encode:me.uprising` / `encode:rival.uprising` came OFF this list on
    # 2026-08-04 (third time: deleted 2026-07-31, re-listed the same session
    # after the government reprice, deleted again here).  Retiring the six
    # phase pairs changed what `DEFAULT_WEIGHTS` bots do, the corpus re-rolled,
    # and the `discontent > workers_free` flag now fires inside the one-ply-in-
    # 40 encoding sample.  The flag was never dead -- it fires on 3 of 2004
    # corpus states -- so what flaps is the SAMPLE, not the encoder, which is
    # docs/OPEN_ITEMS.md section 9.5 and is a defect in this ratchet rather
    # than in anything it watches.  Do not re-list them on the next reprice;
    # fix 9.5.
    "encode:rival.seeded_events_n": (
        ("constant-encoding",),
        "DELIBERATE and it is the information-legality guarantee, not a bug: "
        "`_player_block` emits the seeded-event summary only for `full` "
        "(idx == me), so a rival's seeds are zeroed.  I know what I put in the "
        "politics deck; I do not know what they put in.  A constancy check "
        "that stopped seeing this constant would be reporting a leak",
        "none -- deliberate, RULES_SPEC / INFORMATION_AUDIT"),
    "encode:rival.seeded_events_level": (
        ("constant-encoding",),
        "the second half of the same deliberately-zeroed rival summary",
        "none -- deliberate, RULES_SPEC / INFORMATION_AUDIT"),
    "encode:rival.hand_military_vec": (
        ("constant-encoding",),
        "DELIBERATE, same argument, and the sharpest one: a rival's military "
        "hand CONTENTS are hidden (RULES_SPEC 196), so `_player_block` writes "
        "a zero-vector there and encodes the COUNT instead.  This entry is the "
        "machine-checkable form of that promise",
        "none -- deliberate, RULES_SPEC 196"),
    # -------------------------------------------------------- registry drift
    "has_unit@analysis/frozen/default_weights.json": (
        ("stale-vector-key",),
        "an archived snapshot of a DEFAULT_WEIGHTS that still had a `has_unit` "
        "coordinate.  The file is a FROZEN ARTEFACT and must not be edited; it "
        "is listed so the sweep that would otherwise flag it stays switched on "
        "for every other file",
        "none -- deliberately frozen"),
}

#: Coordinates that reached a live champion file only when its arm last
#: re-execed, and so have not been offered to the climber for a single
#: generation yet.  `inert-live` reads `experiments/league_state/`, which the
#: running league REWRITES: the moment a new key lands in DEFAULT_WEIGHTS the
#: next arm to restart seeds it at its 0.0 default and carries it, and the
#: ratchet -- correctly, by its own rule -- reports a coordinate that is 0.0 on
#: every champion that carries it.  On 2026-08-02 the 3p champion was the only
#: one re-execed since these fourteen landed, so it was the only carrier and
#: all fourteen read dead.
#:
#: They are listed rather than exempted by a rule, because there IS no honest
#: rule: "carried by only one champion" would go quiet again as soon as the
#: other two arms restart, still at 0.0, which is the case the ratchet exists
#: for.  Listing them is also self-cleaning -- the assertion is an equality, so
#: the first one the league actually climbs turns the test red the other way
#: and asks for its line back.  Do not add to this without checking the arm
#: restart times against when the key landed.
#: Eight came off this list on 2026-08-04 -- attack_target_weakness,
#: my_event_threat, my_seeded_pending, rival_building_wonder,
#: rival_free_workers, rival_resource_stock, rival_science_stock and
#: row_last_copy are all off 0.0 on a live champion now.  That is the
#: equality assertion doing its job: the list only ever shrinks, and leaving
#: climbed coordinates in it would let a genuinely dead one hide behind them.
_NOT_YET_CLIMBED = ('attack_target_lead',
                    'pact_partner_lead',
                    'rival_colonies',
                    'rival_food_stock',
                    'rival_mil_actions',
                    'rival_yellow_bank')
for _k in _NOT_YET_CLIMBED:
    KNOWN_DEAD[_k] = (
        ("inert-live",),
        "landed in DEFAULT_WEIGHTS on 2026-08-02 at a 0.0 default and reached "
        "the 3p champion when that arm re-execed the same day; no generation "
        "has priced it yet, so 0.0 here means UNTRIED, not measured-and-dead",
        "delete this line once the league climbs it -- the equality assertion "
        "in test_the_live_champions_leave_exactly_the_listed_ones_at_zero "
        "will ask")

#: `rival_desire` is a SELECTOR, not a value term, and that is the whole
#: difference.  It is clamped to [0, 1] and read once in `row_pressure`:
#: `want = _rival_desire(...) if desire > 0.0 else None`, and `want is None`
#: makes the rival model score competition by UNIFORM per-rival reach instead
#: of by value-weighted desire.  Both branches are models the code runs.  So
#: 0.0 here is an ANSWER -- "use the uniform model" -- where for a value term
#: 0.0 means the coordinate was never priced at all, which is the thing this
#: ratchet exists to catch.  Listing it does not weaken the ratchet, because
#: the sibling selector in the same mutation group (`rival_take_share`, group
#: `row`) has been moved off its 0.5 default on two of the three arms (2p to
#: 0.0, 3p to 0.5734) -- the climber demonstrably reaches this group and has
#: had every chance at this key.  Not-measured: whether the value-weighted
#: branch is BETTER; nobody has run it at desire=1.0 against a champion.
KNOWN_DEAD["rival_desire"] = (
    ("inert-live",),
    "a [0, 1] blend selector inside row_pressure()'s rival model, not a "
    "priced term: 0.0 selects the uniform-reach competition model, which is "
    "a branch the evaluator actually executes, so 0.0 is a chosen setting "
    "rather than an unpriced coordinate",
    "delete this line if the league ever climbs it, or run the desire=1.0 "
    "branch against a champion and record which model is better")

#: Card types `card_potential` is never asked to price, so their pricing says
#: nothing.  Events are seeded into the event deck rather than taken from the
#: civil row; `event_scoring_margin` is the coordinate that sees them.
CLASSES_NOT_PRICED = frozenset(("event",))

#: Individual cards that price at <= 0.0 and SHOULD.  These are not a class
#: failure -- `invisible-class` above is the guard that matters -- but they are
#: pinned so the count cannot drift silently.
KNOWN_ZERO_CARDS = frozenset((
    # the five level-1 starting technologies: you already own them
    "Agriculture", "Bronze", "Philosophy", "Religion", "Warriors",
    # the starting government: likewise
    "Despotism",
    # genuinely negative on this board and that is a judgement, not blindness
    "Code of Laws",
    # 11 science for three technology levels and a `special_techs` step, with
    # no staffing half at all (a special technology carries no worker), so it
    # is the develop half against `w["science"]` and loses on this corpus.
    # Swapped in for `Sid Meier` when the same-type `unit_upgrade` fix
    # re-rolled the corpus -- neither card is priced by that change, both sit
    # within an eval point of zero, and which of the two is on the wrong side
    # of it is a property of the six games, not of the pricing.
    "Military Theory",
    # BACK on 2026-08-04, and this time the cause IS the pricing rather than
    # the corpus: retiring `culture_late` (+1.5 in the defaults) removed the
    # late-game premium on a culture point, and Sid Meier's whole payoff is
    # culture.  Both leaders still sit within an eval point of zero, so the
    # pair going in and out together is the same 9.5 fragility -- but the
    # DIRECTION here was predictable from the change and is not noise.  A
    # trained vector prices him off `culture` at its own weight; this list is
    # about DEFAULT_WEIGHTS.
    "Sid Meier",
))

# --------------------------------------------------------------------------
# The three hand-set weight tables.  `bots.WEIGHTS` is GreedyBot's and is
# FROZEN ON PURPOSE: NARROW and WIDE are GreedyBot and nothing else, and their
# job is to hold still while evaluator changes move the other six arms.  A
# control whose weights drift with the thing it controls for is not a control.
# Every divergence is listed with its value on both sides, so syncing them --
# the obvious move on reading the table -- fails this test first.
# --------------------------------------------------------------------------
DELIBERATE_DIVERGENCE = {
    "culture_rate": (6.0, 5.0),
    "free_workers": (0.3, 0.4),
    "happy_margin": (1.5, 1.2),
    "resource_rate": (1.5, 1.6),
    "strength": (0.8, 0.35),
    "tech_levels": (1.5, 1.0),
    "workers": (1.5, 1.4),
    "yellow_bank": (0.15, -0.1),
}

#: `bots.WEIGHTS` keys that are not `DEFAULT_WEIGHTS` keys at all.  GreedyBot's
#: `features(state, p)` is a DIFFERENT vocabulary in a different module; these
#: three are its spellings of quantities `weighted.features` calls
#: `food_stock`, `resource_stock` and `hand_civil`.
GREEDY_ONLY = {
    "food": "weighted spells it food_stock",
    "resources": "weighted spells it resource_stock",
    "hand": "weighted spells it hand_civil",
}

#: Module-scope constant NAMES that legitimately appear in more than one file
#: with a DIFFERENT literal value.  `MARGIN_SCALE` is the case this list exists
#: for and it is deliberately NOT here -- it was renamed to `MARGIN_NORM`.
ALLOWED_NAME_COLLISIONS = {
    "_DICT": "engine/journal.py's undo tag and fastcopy's are private to each",
    "_LIST": "likewise",
    "GROUP_KEYS": "assigned twice in one file, not across files",
}


# ==========================================================================
# THE FOURTH REGISTRY: engine/bots/neural_encode.py
#
# `encode()` is a THIRD, independent representation of a game state.  It does
# not import `features()` or `card_potential`; it builds its own fixed-length
# float vector.  So a quantity can be live in the linear evaluator and absent
# from the encoding, or encoded and permanently zero, and nothing fails either
# way -- which is this file's bug class one representation over.
#
# `neural_encode` is deliberately torch-free so the stdlib suite can exercise
# it.  This file imports `neural_encode` and NEVER `neural_net`, which would
# drag torch in.
# ==========================================================================

#: The 56 entries of `_player_block`'s `scal` list, in order.  Duplicated
#: rather than derived because there is nothing to derive it from -- and that
#: is exactly why `test_the_scalar_names_still_line_up` asserts the length
#: against `neural_encode._PLAYER_SCALARS`, the same device the encoder's own
#: test uses.
SCALAR_NAMES = (
    "present",
    "culture", "science", "food", "resources",
    "culture_rate", "science_rate", "food_rate", "resource_rate",
    "strength", "happy",
    "civil_actions", "military_actions",
    "yellow_bank", "workers_free", "blue_available",
    "corruption", "consumption", "pop_cost",
    "happy_margin", "discontent", "uprising",
    "civil_actions_left", "military_actions_left", "ca_spent_taking",
    "best_farm", "best_mine", "best_lab", "best_temple", "best_theater",
    "best_library", "best_arena", "best_unit",
    "prod_workers", "urban_workers", "unit_workers", "workers",
    "num_techs", "special_techs", "tech_levels",
    "wonders", "wonder_progress", "wonder_remaining", "has_wonder",
    "tactic_level", "colonies", "pacts",
    "strength_rel", "strength_deficit", "strength_lead",
    "hand_civil", "hand_military",
    "seeded_events_n", "seeded_events_level",
    "gov_level", "has_leader",
)

#: `_global_block`, in order.  A one-hot is ONE slice: it is dead when the
#: whole thing never moves, which is what "the age never changed" means.
GLOBAL_SLICES = (
    ("age_civil", 5), ("age_military", 5), ("current_events_age", 5),
    ("num_players", 3),
    ("round", 1), ("turn", 1), ("rounds_left", 1), ("lateness", 1),
    ("last_round", 1), ("scoring_events", 1),
    ("current_events", 1), ("future_events", 1), ("row_occupancy", 1),
    ("phase", 3),
    # both discard piles, one entry per age.  Public information under the
    # 2026-07-31 card-counting ruling (docs/INFORMATION_AUDIT.md 0c); these
    # two left NOT_ENCODED when they landed, which is exactly the direction
    # that list is allowed to move.
    ("civil_discard", 5), ("discarded_military", 5),
)

#: the five `card_vec` blocks each player block carries, in order
PLAYER_CARD_VECS = ("government", "leader", "wonder",
                    "hand_civil", "hand_military")


def _encode_slices():
    """name -> (indices, players_required).

    A NAMED slice rather than a raw index, because the raw vector is 1897 wide
    and "component 7 of rival 2's government card_vec is constant" is not a
    finding, it is noise -- while "rival.hand_military_vec is constant" is the
    information-legality guarantee in one line.

    `players_required` is how the zero-padding is handled: the rival blocks are
    laid out [me, rival0, rival1, rival2] and padded up to `MAX_PLAYERS`, so
    rival block k only carries a real player when `num_players >= k + 1`.  A
    slice is judged ONLY over the player counts where it is real.
    """
    cv = NE.CARD_VEC_DIM
    out = collections.OrderedDict()
    at = 0
    for name, width in GLOBAL_SLICES:
        out[f"global.{name}"] = (tuple(range(at, at + width)), 2)
        at += width
    assert at == NE._GLOBAL_DIM, (at, NE._GLOBAL_DIM)
    present, cost, vecs = [], [], []
    for slot in range(NE._ROW_SIZE):
        present.append(at)
        cost.append(at + 1)
        vecs.append((slot, tuple(range(at + 2, at + 2 + cv))))
        at += 2 + cv
    for slot, ix in enumerate(present):
        out[f"row{slot}.present"] = ((ix,), 2)
    # the cost ladder is one thing: `_ROW_COST[i] / 3.0`, a positional
    # constant, so it is one entry and not thirteen identical ones.
    out["row.cost_ladder"] = (tuple(cost), 2)
    for slot, ix in vecs:
        out[f"row{slot}.card_vec"] = (ix, 2)
    assert at == NE._GLOBAL_DIM + NE._ROW_DIM
    # [me, rival0, rival1, rival2].  The three rival blocks share one set of
    # slice names -- they are the same information channel three times, and a
    # channel that is dead is dead in all three.
    for block in range(NE.MAX_PLAYERS):
        role = "me" if block == 0 else "rival"
        need = 2 if block <= 1 else block + 1
        base = at + block * NE.PLAYER_BLOCK_DIM
        for j, nm in enumerate(SCALAR_NAMES):
            key = f"{role}.{nm}"
            ix, _ = out.get(key, ((), need))
            out[key] = (ix + (base + j,), need)
        for b, nm in enumerate(PLAYER_CARD_VECS):
            key = f"{role}.{nm}_vec"
            span = tuple(range(base + NE._PLAYER_SCALARS + b * cv,
                               base + NE._PLAYER_SCALARS + (b + 1) * cv))
            ix, _ = out.get(key, ((), need))
            out[key] = (ix + span, need)
    return out


#: Which player counts each rival block is real at, keyed by block index.  Kept
#: separate from `_encode_slices` because the rival slice names are SHARED
#: across the three blocks, so the requirement has to be per index.
def _index_requirement():
    req = {}
    at = NE._GLOBAL_DIM + NE._ROW_DIM
    for block in range(NE.MAX_PLAYERS):
        need = 2 if block <= 1 else block + 1
        for i in range(at + block * NE.PLAYER_BLOCK_DIM,
                       at + (block + 1) * NE.PLAYER_BLOCK_DIM):
            req[i] = need
    return req


#: `GameState` / `PlayerState` fields the ENGINE writes and the ENCODER does
#: not read.  Not bugs -- but "not encoded" is exactly the shape of a dead
#: coordinate, so the exemption is declared with a reason and pinned by set
#: equality, which is also what makes it an INFORMATION-LEGALITY guard: a field
#: that silently starts being encoded leaves this list and fails.
NOT_ENCODED = {
    # hidden by the rules -- the module docstring's own list
    "civil_deck": "HIDDEN: deck ORDER; only the COUNT feeds rounds_left",
    "military_deck": "HIDDEN: deck ORDER; count only",
    "past_events": "HIDDEN: resolved events are not a current-state channel",
    # engine plumbing that is not a board fact
    "seed": "the RNG seed; encoding it would be encoding the future deal",
    "current": "whose turn it is; the encoding is already from `idx`'s seat",
    "start_player": "seating, folded into the [me, rival...] rotation",
    "pending": "engine.interact's decision queue, not a board fact",
    "queue": "likewise",
    "log": "the human-readable move log",
    "has_military": "a data-completeness flag on the card DB, not the game",
    # terminal / scoring state: the net is only ever asked about live states
    "game_over": "terminal flag; encode() is never called on a decided game",
    "final_scores": "terminal",
    "final_round_end": "terminal; `last_round` and `rounds_left` carry it",
    "forced_winner": "terminal",
    # already encoded through effects.compute, so encoding it again would be a
    # duplicate channel -- the `pop_food_discount` mistake one module over
    "strength_extra": "folded into effects.compute().strength, which IS encoded",
    "happy_extra": "folded into effects.compute().happy",
    "blue_total": "folded into effects.blue_available, which IS encoded",
    "yellow_granted": "bookkeeping only; the bank itself is encoded",
    # once-per-turn / once-per-game flags.  ALL of these are genuinely missing
    # channels rather than duplicates, and they are listed together because
    # they are one open item, not seventeen.
    "politics_done": "per-turn flag; docs/OPEN_ITEMS.md neural per-turn flags",
    "tactic_action_used": "per-turn flag; same item",
    "tactic_exclusive": "per-turn flag; same item",
    "hammurabi_used": "leader once-per-turn flag; same item",
    "churchill_used": "leader once-per-turn flag; same item",
    "bach_upgrade_used": "leader once-per-turn flag; same item",
    "ocean_liners_used": "wonder once-per-turn flag; same item",
    "caesar_double_politics_used": "leader once-per-GAME flag; same item",
    "caesar_second_politics": "per-turn flag (Caesar's second political "
                              "action is owed); same item",
    "peeked_event": "INFORMATION: the event card Joan of Arc looked at.  It "
                    "is hers and not the table's, and the player blocks share "
                    "one set of encoding slots -- encoding it would put a "
                    "face-down card into every rival's block, which is the net "
                    "cheating rather than a missing channel",
    "skip_next_politics": "one-shot event flag; same item",
    "ca_penalty_next_turn": "one-shot event penalty; same item",
    "mil_discount": "one-turn ring-fenced resources; same item",
    "mil_sci_discount": "one-turn ring-fenced science; same item",
    "one_time_discount": "one-shot event discount; same item",
    "taken_this_turn": "action cards played this turn; same item",
    "homer_wonder": "which wonder Homer sits under; same item",
    "flipped_wonders": "Ravages of Time; same item",
    "taken_leader_ages": "which ages I have taken a leader in; same item",
    "war_declared_by_me": "war bookkeeping; docs/OPEN_ITEMS.md neural war state",
    "wars_declared_on_me": "war bookkeeping; same item",
    # public records nothing in the rules reads back.  `civil_discard` and
    # `discarded_military` were both here until 2026-07-31 and are now
    # ENCODED (`neural_encode._discard_block`): the card-counting ruling in
    # docs/INFORMATION_AUDIT.md 0c settled that both piles are public.
    "available_tactics": "the common tactic area; docs/OPEN_ITEMS.md",
}

#: Phrases the `neural_encode` docstring must still carry, mapped to the field
#: whose exclusion they justify.  This is what stops the code and its own
#: documentation drifting apart -- and it is an information-legality assertion,
#: not a style one: a field leaving this list means the net can see something a
#: player cannot.
HIDDEN_BY_THE_RULES = {
    "civil_deck": "civil_deck / military_deck ORDER",
    "military_deck": "civil_deck / military_deck ORDER",
    "hand_military": "rival military-hand CONTENTS (count only)",
    "seeded_by": "other players' event seeds and the current-events order",
}

#: Fields the encoder may read ONLY through `len()`.  Reading the list itself
#: would be reading its order, which is the hidden part.
COUNT_ONLY_FIELDS = frozenset(("current_events", "future_events"))


def _state_fields():
    """(tag, class, field name) for every declared dataclass field."""
    out = []
    for cls, tag in ((S.GameState, "state"), (S.PlayerState, "player")):
        for f in dataclasses.fields(cls):
            out.append((f"{tag}.{f.name}", cls, f.name))
    return out


def _field_probes(st):
    """(tag, object, field name) for one live state, players included."""
    for f in dataclasses.fields(S.GameState):
        if f.name != "players":
            yield f"state.{f.name}", st, f.name
    for p in st.players:
        for f in dataclasses.fields(S.PlayerState):
            yield f"player.{f.name}", p, f.name


_MUTATORS = frozenset((
    "append", "extend", "add", "pop", "update", "clear", "insert", "remove",
    "discard", "setdefault", "sort", "reverse",
))
_STATE_CTORS = frozenset(("GameState", "PlayerState", "TechCard",
                          "WonderInProgress"))


def _fields_the_engine_writes():
    """Field names assigned, mutated or constructed anywhere in `engine/`.

    Three shapes, and the third is why a naive sweep gets this wrong:
    `p.completed_wonders` is never a Store target -- the engine writes it as
    `journal.touch(p.completed_wonders).append(name)`, so the mutating call
    has to be walked whole."""
    out = set()
    for fn in sorted(os.listdir(os.path.join(HERE, "engine"))):
        if not fn.endswith(".py"):
            continue
        for node in ast.walk(_module_ast(f"engine/{fn}")):
            if isinstance(node, ast.Attribute) and \
                    isinstance(node.ctx, (ast.Store, ast.Del)):
                out.add(node.attr)
            elif isinstance(node, ast.Subscript) and \
                    isinstance(node.ctx, (ast.Store, ast.Del)):
                out |= {s.attr for s in ast.walk(node.value)
                        if isinstance(s, ast.Attribute)}
            elif isinstance(node, ast.Call):
                fnode = node.func
                if isinstance(fnode, ast.Attribute) and \
                        fnode.attr in _MUTATORS:
                    out |= {s.attr for s in ast.walk(fnode.value)
                            if isinstance(s, ast.Attribute)}
                elif isinstance(fnode, ast.Name) and fnode.id in _STATE_CTORS:
                    out |= {kw.arg for kw in node.keywords if kw.arg}
    return out


def _fields_the_encoder_reads():
    tree = _module_ast("engine/bots/neural_encode.py")
    return {n.attr for n in ast.walk(tree)
            if isinstance(n, ast.Attribute) and isinstance(n.ctx, ast.Load)}


# ==========================================================================
# the reachable-state corpus
# ==========================================================================

#: Deterministic and small: 9 self-play games, three per player count.
#: `CorpusIsNotVacuous` asserts what this actually covers.
#:
#: Widened from 6 to 9 on 2026-07-31.  At 6 games the rate horizon
#: (docs/RATE_HORIZON.md) made `me.discontent` and `rival.discontent`
#: CONSTANT AT 0.0 on every corpus state, and the ratchet correctly flagged
#: two newly-dead encoding slices.  Discontent is a live game mechanic the
#: bot simply stopped reaching in six games, so the honest fix is a corpus
#: that reaches it rather than a KNOWN_DEAD entry claiming it cannot exist.
#: If a future change makes it constant again at nine, check whether the bot
#: really never goes unhappy before widening this further.
CORPUS_SEEDS = ((2, 1), (2, 2), (2, 3), (3, 1), (3, 2), (3, 3),
                (4, 1), (4, 2), (4, 3))
PLY_CAP = 400
#: how often the expensive per-card sweep runs; every ply is 236 cards x N.
CARD_STRIDE = 40

#: Floors.  A corpus that degrades below any of these is not testing anything.
MIN_STATES = 1200
MIN_CARD_STATES = 30
MIN_AGES = 5
MIN_PLAYER_COUNTS = 3
MIN_ROW_FRACTION = 0.9


def _credits_on():
    """`DEFAULT_WEIGHTS` with every declared CREDIT at 1.0 and every scale
    opened, and no feature weight touched.

    This is the vector the invisible-class check runs under, and the
    distinction is load-bearing: a class that only prices at zero because a
    credit is zero is CONFIGURED off, while a class that still prices at zero
    with every credit at 1.0 is BLIND.  Only the second is a bug.
    """
    w = dict(W.DEFAULT_WEIGHTS)
    for k, (reader, _) in PARAMETERS.items():
        if reader in ("pricing", "evaluate"):
            w[k] = 1.0 if not w[k] else w[k]
    return w


def _all_on():
    """Every coordinate non-zero, so a gated term cannot hide a dead one.

    Conduction is measured against THIS and not against `DEFAULT_WEIGHTS`,
    because half the parameters are gates on each other: `rival_take_share`
    multiplies nothing while `row_bargain_forgone` is 0.0, and reporting it
    dead for that reason would be reporting the configuration, not the code.
    """
    return {k: (v if v else 0.37) for k, v in W.DEFAULT_WEIGHTS.items()}


class Corpus:
    """One pass of self-play, with every registry observation taken inline.

    Deliberately does NOT retain states: `actions.apply` mutates in place, so a
    list of states is a list of aliases of the last one -- which is the shape
    of the vacuity trap this file is guarding against.  Everything is
    accumulated as it is seen.
    """

    def __init__(self):
        self.emitted = set()
        self.nonzero = collections.Counter()
        self.ages = collections.Counter()
        self.player_counts = collections.Counter()
        self.states = 0
        self.card_states = 0
        self.encode_states = 0
        self.row_nonempty = 0
        self.card_best = collections.defaultdict(lambda: -1e18)
        self.card_best_default = collections.defaultdict(lambda: -1e18)
        self.conducts = set()
        #: player count -> encoding index -> [min, max]
        self.enc = {n: {} for n in (2, 3, 4)}
        #: "state.<field>" / "player.<field>" -> the first value seen (repr)
        self._field_seen = {}
        #: ...and the ones that were later seen to differ from it
        self.field_varies = set()
        self._run()

    def _note_fields(self, st):
        """Which declared state fields ever CHANGE value.

        The static AST sweep says what the ENGINE writes; this says what the
        GAME actually moves, and the two disagree in interesting places.  Only
        fields that have not yet varied are re-read, so this collapses to
        nothing after the first few plies."""
        seen, varies = self._field_seen, self.field_varies
        for tag, obj, name in _field_probes(st):
            if tag in varies:
                continue
            r = repr(getattr(obj, name))
            if tag not in seen:
                seen[tag] = r
            elif r != seen[tag]:
                varies.add(tag)

    def _note_encoding(self, st, players):
        """min/max per encoding slot, SEGMENTED BY PLAYER COUNT.

        The segmentation is the whole trick: `encode` zero-pads the unused
        rival blocks so the vector length is fixed across 2p/3p/4p, so a slot
        in rival block 2 is legitimately 0.0 in every 2p state and reading
        those in would make the constancy check permanently red.  See
        `_encode_slices`, which records how many players a slot needs before
        it is real."""
        d = self.enc[players]
        for idx in range(players):
            for i, x in enumerate(NE.encode(st, idx)):
                cur = d.get(i)
                if cur is None:
                    d[i] = [x, x]
                elif x < cur[0]:
                    cur[0] = x
                elif x > cur[1]:
                    cur[1] = x
        self.encode_states += 1

    def _run(self):
        # `board_yields` memoises on `(card, effects.stats_key(state, p))` in
        # three PROCESS-GLOBAL dicts.  Any test module that played a game
        # before this one leaves entries in them, and `card_potential` then
        # takes hits computed on somebody else's board -- so the corpus, and
        # every verdict in this file derived from it, depended on which
        # modules `unittest discover` happened to import first.  Observed for
        # real: `Sid Meier` prices -0.4 when this module runs alone and above
        # 0.0 when `test_board_yields` has run first, which flipped
        # `test_the_individual_zero_cards_are_pinned` in the full suite only.
        # A registry whose answers depend on process history is not a
        # registry, so start from cold caches.
        #
        # THAT DIFFERENCE IS NOW EXPLAINED (2026-08-02).  It was not
        # `stats_key` being incomplete -- it is complete for `effects.compute`,
        # and `test_board_yields.TestStatsKeyIsACompleteMemoKey` still proves
        # it.  These caches do not hold a `compute` result; they hold a PLAN,
        # which is priced by the rules as well as the ratings, so the key has
        # to name `yellow_bank`, `workers_free`, `mil_discount` and
        # `one_time_discount` too.  It now does: `board_yields._plan_key`,
        # guarded by `TestThePlanKeyIsComplete`.  The clearing below stays --
        # a complete key makes a warm cache harmless, not free of eviction,
        # and this file's whole job is to depend on nothing it did not build.
        BY._DELTA_CACHE.clear()
        BY._UNIT_CACHE.clear()
        BY._TECH_CACHE.clear()
        BY._BUILD_CACHE.clear()      # the fourth one; missed when this was written
        names = [c["name"] for c in C.db().cards]
        credits_on = _credits_on()
        all_on = _all_on()
        keys = sorted(W.DEFAULT_WEIGHTS)
        for players, seed in CORPUS_SEEDS:
            st = G.new_game(players, seed)
            rng = random.Random(seed)
            bot = W.WeightedBot(seed=seed)
            ply = 0
            while not st.game_over and ply < PLY_CAP:
                self.states += 1
                self._note_fields(st)
                self.ages[st.age_civil] += 1
                self.player_counts[players] += 1
                if any(c is not None for c in st.card_row):
                    self.row_nonempty += 1
                for idx in range(players):
                    f = W.features(st, idx, None, W.DEFAULT_WEIGHTS)
                    self.emitted |= set(f)
                    for k, v in f.items():
                        if v:
                            self.nonzero[k] += 1
                self._probe_unhappy(st)
                self._probe_attack(st, players)
                if ply % CARD_STRIDE == 0:
                    self.card_states += 1
                    for nm in names:
                        v = W.card_potential(nm, credits_on, st, 0)
                        if v > self.card_best[nm]:
                            self.card_best[nm] = v
                        v0 = W.card_potential(nm, W.DEFAULT_WEIGHTS, st, 0)
                        if v0 > self.card_best_default[nm]:
                            self.card_best_default[nm] = v0
                    self._probe_conduction(st, all_on, keys)
                    self._note_encoding(st, players)
                A.apply(st, bot.pick(st, A.legal_moves(st)), rng)
                ply += 1
            # the terminal state, which the loop above steps past: `game_over`,
            # `final_scores` and `forced_winner` only ever move here, and a
            # corpus that stops one ply early reports all three dead.
            self._note_fields(st)
            self._note_encoding(st, players)

    def _probe_unhappy(self, st):
        """The anti-vacuity half of the FEATURE sweep, matching the one
        `_probe_conduction` already does for weights.

        `discontent` is `max(0, happy_required(yellow_bank) - happy)` and
        `uprising` needs it to exceed `workers_free` on top.  A `WeightedBot`
        that plays competently never lets either happen, so whether these two
        fire at all was down to whether SOME seat in SOME corpus game got
        itself into trouble -- which held until the 2026-07-31 colonization
        change perturbed play, and then did not.  Two live, heavily-weighted
        coordinates (-3.0 and -12.0 in DEFAULT_WEIGHTS) were about to be
        declared dead because six games of decent play avoided the state they
        describe.  So construct it: one seat, an EMPTIED yellow bank (RULES
        6.3 counts the empty subsections, so 0 in the bank is the maximum
        happiness requirement, 8) and no spare workers to absorb it.
        Restored immediately; skipped once both have fired.
        """
        if self.nonzero["uprising"] and self.nonzero["discontent"]:
            return
        p = st.players[0]
        bank, free = p.yellow_bank, p.workers_free
        p.yellow_bank, p.workers_free = 0, 0
        effects.invalidate(st, p)
        try:
            for k, v in W.features(st, 0, None, W.DEFAULT_WEIGHTS).items():
                if v:
                    self.nonzero[k] += 1
        finally:
            p.yellow_bank, p.workers_free = bank, free
            # MUST invalidate on the way back out as well: `effects.compute`
            # is memoised per player, so a probe that restores the field and
            # not the cache leaves every later observation reading the
            # probe's board.  That is how a constructed probe silently
            # becomes a corpus corruption.
            effects.invalidate(st, p)

    def _probe_attack(self, st, players):
        """The same anti-vacuity construction as `_probe_unhappy`, for the
        state that exists only WHILE an attack is in flight.

        `attack_target_lead` / `attack_target_weakness` answer "which opponent
        am I attacking", so they are non-zero only in a position that carries
        a `defense` pending or a declared war.  Measured before adding this:
        across three corpus-shaped games, 1279 states, those two features were
        0.0 everywhere -- and the reason was NOT that they are broken.  The
        `WeightedBot` on `DEFAULT_WEIGHTS` never plays an `aggression` or a
        `war` at all in those games (`pol_pass` 66 times, `aggression` and
        `war` zero; see docs/WAR_RATE_CENSUS.md), so the corpus never reaches
        an attacked position.  At the same 97 plies where an attack was LEGAL,
        applying one made `weighted.attack_targets` non-empty 97 times out of
        97.

        That is the "a rate of zero has two causes" rule paying off: the
        honest reading of the zero was "the sampler never stands where the
        term lives", and the fix is to stand there rather than to declare the
        term dead.  Applied to a COPY -- `actions.apply` mutates, and a probe
        that advanced the real game would corrupt every later observation.
        Skipped once both have fired.
        """
        if self.nonzero["attack_target_lead"] \
                and self.nonzero["attack_target_weakness"]:
            return
        idx = st.decider()
        if idx is None:
            return
        mv = next((m for m in A.legal_moves(st)
                   if m[0] in ("aggression", "war") and len(m) > 2), None)
        if mv is None:
            return
        trial = fastcopy.copy_state(st)
        try:
            G.apply(trial, mv, random.Random(1))
        except Exception:                                      # noqa: BLE001
            return
        for _k, v in W.features(trial, idx, None, W.DEFAULT_WEIGHTS).items():
            if v:
                self.nonzero[_k] += 1

    def _probe_conduction(self, st, base, keys):
        """Conduction on the board as dealt, and on a stacked civil hand.

        The second half is deliberate and it is the anti-vacuity half.  Some
        coordinates only conduct through a shape the corpus rarely deals --
        `hand_swap_extra` needs TWO cards of one single-slot class in hand at
        once -- and a probe that never produces that shape would report a live
        coordinate dead, which is the same error in the other direction.
        `rival_context` is built for the same reason: without it `row_pressure`
        sees no rival views and `rival_take_share` multiplies nothing.
        """
        if all(k in self.conducts for k in keys):
            return
        ctx = W.rival_context(st, 0)
        self._conduction(st, base, keys, ctx)
        db = C.db()
        spare = [c["name"] for c in db.cards
                 if c["type"] in ("leader", "government")][:4]
        hand = st.players[0].hand_civil
        st.players[0].hand_civil = list(hand) + spare
        try:
            self._conduction(st, base, keys, ctx)
        finally:
            st.players[0].hand_civil = hand
        self._probe_wonder_and_tactic(st, base, keys, ctx)

    def _probe_wonder_and_tactic(self, st, base, keys, ctx):
        """The third shape, and it exists for the same reason the stacked hand
        does: two coordinates only conduct through a board the corpus reaches
        by luck rather than by construction.

        `wonder_potential` multiplies `card_potential` on `p.wonder`, and a
        wonder is the rarest thing this policy takes (0.05 per seat-game);
        `tactic_gain` is zero unless a REACHABLE tactic would add army
        strength, which needs unit workers and a tactic at once.  Both
        conducted here until 2026-07-31 purely because the six games happened
        to produce those boards, and a pricing change that re-rolled self-play
        made them read as dead coordinates -- a probe reporting the policy
        rather than the code, which is exactly the failure `hand_swap_extra`
        already forced this function to fix once.
        """
        p = st.players[0]
        if all(k in self.conducts for k in keys):
            return
        db = C.db()
        wonder = next(c["name"] for c in db.cards if c["type"] == "wonder")
        tactic = next(c["name"] for c in db.cards if c["type"] == "tactic")
        old_w, old_m = p.wonder, p.hand_military
        old_units = {n: t.workers for n, t in p.techs.items()}
        p.wonder = S.WonderInProgress(wonder, 0)
        p.hand_military = list(old_m) + [tactic]
        for n, t in p.techs.items():
            if db.type_by_name.get(n) in C.UNIT_TYPES:
                t.workers += 4
        effects.invalidate(st, p)
        try:
            self._conduction(st, base, keys, ctx)
        finally:
            p.wonder, p.hand_military = old_w, old_m
            for n, t in p.techs.items():
                t.workers = old_units[n]
            effects.invalidate(st, p)

    def _conduction(self, st, base, keys, ctx):
        """Which coordinates can move `evaluate` at all, on this board.

        Short-circuited: a key that has already conducted anywhere is never
        re-tested, which is what keeps this affordable.
        """
        todo = [k for k in keys if k not in self.conducts]
        if not todo:
            return
        ref = W.evaluate(st, 0, base, ctx)
        for k in todo:
            bumped = dict(base)
            bumped[k] = base[k] + 1.0
            if W.evaluate(st, 0, bumped, ctx) != ref:
                self.conducts.add(k)


_CORPUS = None


def corpus():
    global _CORPUS
    if _CORPUS is None:
        _CORPUS = Corpus()
    return _CORPUS


# ==========================================================================
# the static registries
# ==========================================================================

def _module_ast(rel):
    with open(os.path.join(HERE, rel), encoding="utf-8") as fh:
        return ast.parse(fh.read())


def _called_names(node):
    for sub in ast.walk(node):
        if isinstance(sub, ast.Call):
            fn = sub.func
            if isinstance(fn, ast.Name):
                yield fn.id
            elif isinstance(fn, ast.Attribute):
                yield fn.attr


def _loaded_names(node):
    for sub in ast.walk(node):
        if isinstance(sub, ast.Name) and isinstance(sub.ctx, ast.Load):
            yield sub.id


def _pricing_reachable():
    """Function bodies and module-level data reachable from `card_potential`.

    A call graph, not a regex: start at `card_potential`, follow every call by
    name through `PRICING_MODULES`, and pull in every module-scope assignment
    those functions read by name -- which is how `_EFFECT_KEYS`,
    `_BONUS_TO_FEATURE` and `_RESTRICTED_TO_FEATURE` get swept even though the
    keys they carry never appear inside a function body.
    """
    funcs, consts = {}, {}
    for rel in PRICING_MODULES:
        for node in _module_ast(rel).body:
            if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef)):
                funcs.setdefault(node.name, []).append(node)
            elif isinstance(node, ast.Assign):
                for t in node.targets:
                    if isinstance(t, ast.Name):
                        consts.setdefault(t.id, []).append(node.value)
    seen, out, queue = set(), [], ["card_potential"]
    while queue:
        name = queue.pop()
        if name in seen:
            continue
        seen.add(name)
        for node in funcs.get(name, ()):
            out.append(node)
            queue.extend(_called_names(node))
            for load in _loaded_names(node):
                if load in consts and load not in seen:
                    seen.add(load)
                    out.extend(consts[load])
    return out


def _ast_pricing_keys():
    """Every `DEFAULT_WEIGHTS` key spelled as a string literal in that graph.

    Exact membership, so a docstring cannot match: prose is one long constant,
    and a weight name is not.
    """
    known = set(W.DEFAULT_WEIGHTS) | AB_HATCHES
    out = set()
    for node in _pricing_reachable():
        for sub in ast.walk(node):
            if isinstance(sub, ast.Constant) and isinstance(sub.value, str):
                if sub.value in known:
                    out.add(sub.value)
    return out


class _Recording(dict):
    """A weight vector that remembers which coordinates were asked for."""

    def __init__(self, *a, **k):
        super().__init__(*a, **k)
        self.seen = set()

    def get(self, k, d=None):
        self.seen.add(k)
        return super().get(k, d)

    def __getitem__(self, k):
        self.seen.add(k)
        return super().__getitem__(k)


def _runtime_pricing_keys(state=None, idx=None):
    """Every coordinate `card_potential` actually looks up, over all 236 cards.

    The dynamic half of the sweep.  The AST half can miss a key reached through
    data the graph walk did not resolve; this cannot, for the paths it runs.
    """
    names = [c["name"] for c in C.db().cards]
    out = set()
    for base in (W.DEFAULT_WEIGHTS, _credits_on(), _all_on()):
        rec = _Recording(base)
        for nm in names:
            W.card_potential(nm, rec)
            if state is not None:
                W.card_potential(nm, rec, state, idx)
        out |= rec.seen
    return out


def _vector_files(committed_only=False):
    """(relative path, weights dict) for every weight vector on disk.

    `committed_only` drops `experiments/league_state/` and
    `experiments/hall_of_fame/`, which are the LIVE league's and are untracked
    -- present on the training box, absent in a clone.  Any check whose answer
    is a RATCHET has to use the committed set, or the allow-list means two
    different things in two trees.
    """
    out = []
    for pattern, required in VECTOR_GLOBS:
        if committed_only and not required:
            continue
        hits = sorted(glob.glob(os.path.join(HERE, pattern)))
        if required and not hits:
            raise AssertionError(f"no weight vectors matched {pattern}")
        for path in hits:
            rel = os.path.relpath(path, HERE)
            try:
                with open(path, encoding="utf-8") as fh:
                    blob = json.load(fh)
            except (OSError, ValueError):
                if required:
                    raise
                continue        # a live file mid-write; untracked by definition
            if isinstance(blob, dict) and isinstance(blob.get("weights"), dict):
                out.append((rel, blob["weights"]))
    return out


def _module_literals(roots=("engine", "engine/bots", "experiments")):
    """name -> [(relative path, literal value)] for module-scope constants.

    Literal-valued only, exactly as `tests/test_model_constants.py` scopes its
    sweep: a name bound to an expression (`HERE = os.path.dirname(...)`) is
    plumbing, and two files computing the same path is not a collision.
    """
    seen = collections.defaultdict(list)
    for root in roots:
        d = os.path.join(HERE, root)
        for fn in sorted(os.listdir(d)):
            if not fn.endswith(".py"):
                continue
            rel = f"{root}/{fn}"
            for node in _module_ast(rel).body:
                if isinstance(node, ast.Assign):
                    targets = node.targets
                elif isinstance(node, ast.AnnAssign):
                    targets = [node.target]
                else:
                    continue
                if node.value is None:
                    continue
                try:
                    value = ast.literal_eval(node.value)
                except (ValueError, TypeError, SyntaxError):
                    continue
                for t in targets:
                    # `__all__` and friends are per-module by definition, so a
                    # dunder sharing a name across files is not a collision.
                    if isinstance(t, ast.Name) and not t.id.startswith("__"):
                        seen[t.id].append((rel, value))
    return seen


def _dead(kind):
    """Every listed name carrying `kind`.

    An entry may carry more than one -- `wonder_overrun` is both `inert-vector`
    and `never-nonzero`, and reading those two together is the point: neither
    half can wake the other."""
    return {k for k, (kinds, _, _) in KNOWN_DEAD.items() if kind in kinds}


def _fmt(missing, extra, what, how):
    lines = []
    if missing:
        lines.append(f"{len(missing)} NEW dead {what}: {sorted(missing)}")
        lines.append(f"  -> {how}, or add each to KNOWN_DEAD with a reason.")
    if extra:
        lines.append(f"{len(extra)} KNOWN_DEAD entr(ies) are NO LONGER dead: "
                     f"{sorted(extra)}")
        lines.append("  -> DELETE them from KNOWN_DEAD.  The list only shrinks.")
    return "\n".join(lines)


# ==========================================================================
# the tests
# ==========================================================================

class CorpusIsNotVacuous(unittest.TestCase):
    """A structural test that silently stops testing is worse than none.

    Every floor here is a number, so a corpus that degrades to one trivial
    state FAILS instead of passing everything downstream."""

    def test_the_corpus_has_states_in_it(self):
        c = corpus()
        self.assertGreaterEqual(c.states, MIN_STATES)
        self.assertGreaterEqual(c.card_states, MIN_CARD_STATES)

    def test_the_corpus_covers_every_age(self):
        c = corpus()
        self.assertEqual(sorted(c.ages), ["A", "I", "II", "III", "IV"])
        self.assertGreaterEqual(len(c.ages), MIN_AGES)
        for age, n in c.ages.items():
            self.assertGreater(n, 0, age)

    def test_the_corpus_covers_every_player_count(self):
        c = corpus()
        self.assertEqual(sorted(c.player_counts), [2, 3, 4])
        self.assertGreaterEqual(len(c.player_counts), MIN_PLAYER_COUNTS)
        for n in (2, 3, 4):
            self.assertGreater(c.player_counts[n], 100, n)

    def test_the_card_row_is_actually_populated(self):
        """The `test_row_features.py` trap: a probe that lands where the
        quantity under test cannot vary is a probe that stops probing."""
        c = corpus()
        self.assertGreater(c.row_nonempty / c.states, MIN_ROW_FRACTION)

    def test_the_registries_are_all_non_empty(self):
        self.assertGreater(len(W.DEFAULT_WEIGHTS), 100)
        self.assertGreater(len(corpus().emitted), 60)
        self.assertGreater(len(_ast_pricing_keys()), 30)
        self.assertGreater(len(_runtime_pricing_keys()), 30)
        self.assertGreater(len(_vector_files()), 4)
        self.assertGreater(len(_module_literals()), 100)

    def test_the_ast_sweep_finds_the_historically_dead_coordinates(self):
        """The sweep's own negative control.  These five are the reason this
        file exists; a graph walk that stopped finding them would make every
        `unpriced-feature` assertion below vacuously green."""
        found = _ast_pricing_keys()
        for k in ("free_civil_action", "resource_discount",
                  "restricted_resources", "defense_bonus", "gov_action_cost"):
            self.assertIn(k, found, f"the AST sweep lost {k}")


class FeaturesAndWeightsAreABijection(unittest.TestCase):
    """The core invariant, asserted in both directions."""

    def test_every_emitted_feature_has_a_weight(self):
        """Otherwise the feature is computed on every evaluation and silently
        discarded."""
        orphan = sorted(corpus().emitted - set(W.DEFAULT_WEIGHTS))
        self.assertEqual(orphan, [], f"{len(orphan)} feature(s) emitted by "
                         "features() with no weight in DEFAULT_WEIGHTS")

    def test_every_weight_is_emitted_or_declared(self):
        """**This is the direction that would have caught the action-card
        bug.**  A weight nothing emits is a weight the trainer can never get
        information about."""
        unmatched = (set(W.DEFAULT_WEIGHTS) - set(W.PHASE_WEIGHTS)
                     - corpus().emitted)
        declared = set(PARAMETERS) | _dead("unpriced-feature")
        self.assertEqual(
            unmatched, declared,
            _fmt(unmatched - declared, declared - unmatched,
                 "weight(s) with no feature",
                 "emit the feature from features()"))

    def test_no_coordinate_is_both_a_parameter_and_unpriced(self):
        """The one contradiction the two lists can express: a key cannot be
        exempt because it is a credit AND dead because it is a price."""
        for k in sorted(set(PARAMETERS) & set(KNOWN_DEAD)):
            self.assertNotIn(
                "unpriced-feature", KNOWN_DEAD[k][0],
                f"{k} cannot be both a declared parameter and an unpriced "
                "feature; pick one")


class PhaseWeightsMatchTheirBases(unittest.TestCase):
    """`evaluate` reads `w[k + "_early"]` and `w[k + "_late"]` for every
    `PHASE_KEYS` member, so a stem with no pair is a silent no-op and a pair
    with no stem is a weight nothing multiplies."""

    def test_phase_and_base_are_disjoint(self):
        self.assertEqual(sorted(set(W.PHASE_WEIGHTS) & set(W.BASE_WEIGHTS)), [])

    def test_every_phase_key_has_both_halves(self):
        for k in W.PHASE_KEYS:
            self.assertIn(k + "_early", W.PHASE_WEIGHTS, k)
            self.assertIn(k + "_late", W.PHASE_WEIGHTS, k)

    def test_every_phase_weight_names_a_real_phase_key(self):
        stems = set()
        for k in W.PHASE_WEIGHTS:
            self.assertTrue(k.endswith("_early") or k.endswith("_late"), k)
            stems.add(k.rsplit("_", 1)[0])
        self.assertEqual(sorted(stems), sorted(W.PHASE_KEYS))

    def test_every_phase_key_is_a_real_feature(self):
        c = corpus()
        for k in W.PHASE_KEYS:
            self.assertIn(k, W.BASE_WEIGHTS, k)
            self.assertIn(k, c.emitted, f"{k} is phase-blended but not emitted")

    def test_the_phase_blend_covers_the_whole_table(self):
        self.assertEqual(len(W.PHASE_WEIGHTS), 2 * len(W.PHASE_KEYS))


class CardPricingSpendsRealCoordinates(unittest.TestCase):
    """`card_potential` claims to return *eval-points a card would be worth if
    it were played*.  A coordinate `features()` never emits makes that claim
    false: playing the card cannot produce those points."""

    def test_every_priced_coordinate_is_a_weight(self):
        keys = _ast_pricing_keys() | _runtime_pricing_keys()
        orphan = sorted(keys - set(W.DEFAULT_WEIGHTS) - AB_HATCHES)
        self.assertEqual(orphan, [],
                         f"{len(orphan)} coordinate(s) spent by card_potential "
                         "with no entry in DEFAULT_WEIGHTS")

    def test_every_priced_coordinate_is_a_feature_or_a_credit(self):
        keys = _ast_pricing_keys() | _runtime_pricing_keys()
        c = corpus()
        unmatched = {k for k in keys
                     if k not in c.emitted and k not in AB_HATCHES
                     and k not in W.PHASE_WEIGHTS}
        declared = set(PARAMETERS) | _dead("unpriced-feature")
        stray = unmatched - declared
        self.assertEqual(
            stray, set(),
            _fmt(stray, set(), "priced coordinate(s)",
                 "emit the feature, or declare the key in PARAMETERS"))

    def test_the_dead_priced_coordinates_are_exactly_the_listed_ones(self):
        """The ratchet on this category, both directions."""
        keys = _ast_pricing_keys() | _runtime_pricing_keys()
        c = corpus()
        live_dead = {k for k in keys
                     if k not in c.emitted and k not in AB_HATCHES
                     and k not in W.PHASE_WEIGHTS
                     and k not in PARAMETERS}
        listed = _dead("unpriced-feature")
        self.assertEqual(
            live_dead, listed,
            _fmt(live_dead - listed, listed - live_dead,
                 "priced coordinate(s) with no feature",
                 "emit the feature from features()"))


class EveryParameterConducts(unittest.TestCase):
    """`PARAMETERS` is the exemption list, so it is the one that can rot.

    An exemption that stops being read by anything is indistinguishable from a
    dead coordinate, which is the bug -- so each entry has to prove it can
    still move something."""

    def test_every_evaluate_parameter_moves_the_evaluation(self):
        c = corpus()
        stuck = sorted(k for k, (r, _) in PARAMETERS.items()
                       if r == "evaluate" and k not in c.conducts)
        self.assertEqual(stuck, [], f"{len(stuck)} declared evaluate "
                         "parameter(s) cannot move evaluate() on any state in "
                         "the corpus; they are dead, not exempt")

    def test_every_pricing_parameter_is_read_by_card_potential(self):
        keys = _ast_pricing_keys() | _runtime_pricing_keys()
        stuck = sorted(k for k, (r, _) in PARAMETERS.items()
                       if r == "pricing" and k not in keys)
        self.assertEqual(stuck, [], f"{len(stuck)} declared pricing "
                         "parameter(s) that card_potential never reads")

    def test_every_search_parameter_is_read_by_a_bot(self):
        want = {k for k, (r, _) in PARAMETERS.items() if r == "search"}
        found = set()
        d = os.path.join(HERE, "engine", "bots")
        for fn in sorted(os.listdir(d)):
            if not fn.endswith(".py"):
                continue
            for node in ast.walk(_module_ast(f"engine/bots/{fn}")):
                if isinstance(node, ast.Constant) and node.value in want:
                    found.add(node.value)
        self.assertEqual(sorted(want - found), [])

    def test_the_parameter_readers_are_one_of_the_three(self):
        for k, (reader, note) in sorted(PARAMETERS.items()):
            self.assertIn(reader, ("evaluate", "pricing", "search"), k)
            self.assertTrue(note.strip(), k)
            self.assertIn(k, W.DEFAULT_WEIGHTS, k)

    def test_most_of_the_vector_conducts(self):
        """Anti-vacuity on the conduction probe itself: if it stopped
        detecting conduction, every assertion above would pass trivially."""
        c = corpus()
        self.assertGreater(len(c.conducts), 0.8 * len(W.DEFAULT_WEIGHTS))


class NoCoordinateIsInertOnEveryVector(unittest.TestCase):
    """`unit_strength_credit` and `tech_levels` sat at 0.0 on all three
    champions for days while four audits walked past them.  A weight that is
    0.0 by default and 0.0 in every vector on disk cannot change any decision
    anywhere in the project."""

    @staticmethod
    def _inert(files):
        """0.0 by default, CARRIED by at least one of `files`, and 0.0 in every
        one of them that carries it.

        "Carried by at least one" is load-bearing.  Without it a coordinate no
        vector on disk has ever heard of is trivially "0.0 everywhere", and
        `experiments/champion_*.json` are 78-key files against a 124-key
        registry -- so 46 coordinates would be reported inert for the opposite
        reason to the one this test is about."""
        vals = collections.defaultdict(list)
        for _, weights in files:
            for k, v in weights.items():
                vals[k].append(v)
        return {k for k in W.DEFAULT_WEIGHTS
                if W.DEFAULT_WEIGHTS[k] == 0.0 and vals.get(k)
                and all(v == 0.0 for v in vals[k])}

    def test_the_inert_coordinates_are_exactly_the_listed_ones(self):
        """The ratchet runs over the COMMITTED vectors only, so it gives the
        same answer in a bare clone as it does on the training box."""
        live_dead = self._inert(_vector_files(committed_only=True))
        listed = _dead("inert-vector")
        self.assertEqual(
            live_dead, listed,
            _fmt(live_dead - listed, listed - live_dead,
                 "coordinate(s) inert on every committed vector",
                 "give it a non-zero default or let the league climb it"))

    def test_the_live_champions_leave_exactly_the_listed_ones_at_zero(self):
        """The same ratchet against the three vectors that are actually being
        climbed.  Skipped in a bare clone, because `experiments/league_state/`
        is untracked -- so this is the half of the check that only the training
        box can run, and `inert-vector` above is the half everybody can."""
        live = [(rel, w) for rel, w in _vector_files() if "league_state" in rel]
        if not live:
            self.skipTest("no live league state in this tree")
        live_dead = self._inert(live)
        listed = _dead("inert-live")
        self.assertEqual(
            live_dead, listed,
            _fmt(live_dead - listed, listed - live_dead,
                 "coordinate(s) at exactly 0.0 on every live champion",
                 "let the league climb it"))


class NoFeatureIsAlwaysZero(unittest.TestCase):
    """Emitted is not the same as live.  A feature that is exactly 0.0 on every
    reachable state is a column of zeros: the weight beside it can be anything
    and no game will ever notice."""

    def test_the_always_zero_features_are_exactly_the_listed_ones(self):
        c = corpus()
        live_dead = {k for k in c.emitted if not c.nonzero[k]}
        listed = _dead("never-nonzero")
        self.assertEqual(
            live_dead, listed,
            _fmt(live_dead - listed, listed - live_dead,
                 "feature(s) that are 0.0 on every corpus state",
                 "make the feature fire, or delete it"))


class NoCardClassIsInvisible(unittest.TestCase):
    """`row_pressure` skips any slot whose `card_potential` is `<= 0.0`, so a
    card class that never prices above zero is not merely mispriced -- the bot
    cannot see it exists.  That is how units and then yellow/technology cards
    each disappeared as a whole class (`d8a2172`, `8b972ef`).

    Grouped BY CLASS, because both historical bugs took out a whole class and a
    per-card report buries that under three hundred lines."""

    def _by_class(self):
        db = C.db()
        types = {c["name"]: c["type"] for c in db.cards}
        c = corpus()
        blind = collections.defaultdict(list)
        for name, typ in types.items():
            if typ in CLASSES_NOT_PRICED:
                continue
            if c.card_best[name] <= 0.0:
                blind[typ].append(name)
        return types, blind

    def test_no_whole_class_prices_at_zero(self):
        types, blind = self._by_class()
        total = collections.Counter(types.values())
        invisible = {f"class:{t}" for t, names in blind.items()
                     if len(names) == total[t]
                     or len(names) >= total[t] - 1}
        listed = _dead("invisible-class")
        detail = "\n".join(
            f"    class:{t}  {len(blind[t])}/{total[t]} at <= 0.0  "
            f"e.g. {blind[t][:3]}"
            for t in sorted(blind))
        self.assertEqual(
            invisible, listed,
            _fmt(invisible - listed, listed - invisible,
                 "card class(es) invisible to row_pressure",
                 "price the class so card_potential is > 0 somewhere")
            + "\n  full per-class report:\n" + detail)

    def test_the_individual_zero_cards_are_pinned(self):
        types, blind = self._by_class()
        total = collections.Counter(types.values())
        singles = {n for t, names in blind.items() for n in names
                   if len(names) < total[t] - 1}
        self.assertEqual(
            singles, set(KNOWN_ZERO_CARDS),
            f"cards priced <= 0.0 outside a blind class changed:\n"
            f"    new: {sorted(singles - set(KNOWN_ZERO_CARDS))}\n"
            f"    gone: {sorted(set(KNOWN_ZERO_CARDS) - singles)}")

    def test_the_credit_gated_classes_are_not_counted_as_blind(self):
        """The distinction the two probe vectors exist for: leaders and
        governments price at 0.0 under `DEFAULT_WEIGHTS` because
        `card_board_credit` is 0.0, and price positive with the credits on.
        That is configuration, not blindness -- and if that ever stopped being
        true, `_credits_on` would have stopped doing anything."""
        c = corpus()
        db = C.db()
        for typ in ("leader", "government"):
            names = [x["name"] for x in db.cards if x["type"] == typ]
            dark = [n for n in names if c.card_best_default[n] <= 0.0]
            lit = [n for n in names if c.card_best[n] > 0.0]
            self.assertGreater(len(dark), 0, typ)
            self.assertGreater(len(lit), len(dark), typ)


class VectorFilesMatchTheRegistry(unittest.TestCase):
    """Every champion, hall-of-fame and frozen vector on disk is a registry
    too, and `load_weights` fills a missing key from `DEFAULT_WEIGHTS` without
    saying so -- which is exactly how a key can rot out of a file unnoticed."""

    @staticmethod
    def _stray(files):
        """Keys in a vector file that `DEFAULT_WEIGHTS` does not have.

        `W.RETIRED_KEYS` is excluded because this ratchet is for TYPOS and
        ORPHANS -- a key that arrived by accident and is read by nothing.  A
        deliberate retirement is neither: it is named in `weighted.py`, it is
        dropped by `load_weights`, and every champion written before it was
        retired legitimately still carries it.  Listing twelve retired keys
        across forty files individually would bury the one signal this test
        exists to give.
        """
        skip = set(W.DEFAULT_WEIGHTS) | set(W.RETIRED_KEYS)
        return {f"{k}@{rel}" for rel, weights in files
                for k in set(weights) - skip}

    def test_no_committed_vector_carries_a_coordinate_that_does_not_exist(self):
        stray = self._stray(_vector_files(committed_only=True))
        listed = _dead("stale-vector-key")
        self.assertEqual(
            stray, listed,
            _fmt(stray - listed, listed - stray,
                 "vector key(s) with no weight",
                 "delete the key from the file, or add the weight"))

    def test_no_live_vector_carries_a_coordinate_that_does_not_exist(self):
        live = [(rel, w) for rel, w in _vector_files()
                if "league_state" in rel or "hall_of_fame" in rel]
        if not live:
            self.skipTest("no live league state in this tree")
        stray = sorted(self._stray(live) - _dead("stale-vector-key"))
        self.assertEqual(stray, [], f"{len(stray)} live vector key(s) that "
                         "DEFAULT_WEIGHTS no longer has; load_weights would "
                         "drop them silently")

    def test_the_live_champions_carry_every_climbable_coordinate(self):
        """A live champion missing a key is a coordinate the league is not
        climbing -- but a brand new weight is legitimately absent until the
        climber next writes the file, so absence alone cannot be the failure.

        What matters is WHICH keys are absent.  `load_weights` back-fills from
        `DEFAULT_WEIGHTS`, so a missing key whose default is 0.0 is a silent
        no-op: the champion evaluates bit-identically and picks the coordinate
        up on its next accepted generation.  A missing key with a NON-ZERO
        default is the real defect -- the champion is being fed a value it
        never trained against, and it changes its play the moment it loads.

        This replaces an earlier "at most 8 missing" count.  A count is the
        wrong instrument twice over: it fails a legitimate change that adds
        nine dark keys at once (the information-audit work added thirteen),
        and it passes a single missing `culture` -- which would be a disaster
        -- as comfortably inside the bound.
        """
        live = [(rel, w) for rel, w in _vector_files()
                if "league_state" in rel]
        if not live:
            self.skipTest("no live league state in this tree")
        for rel, weights in live:
            missing = sorted(set(W.DEFAULT_WEIGHTS) - set(weights))
            lit = sorted(k for k in missing if W.DEFAULT_WEIGHTS[k] != 0.0)
            self.assertEqual(
                lit, [], f"{rel} is missing {len(lit)} coordinate(s) whose "
                f"default is NOT 0.0: {lit}.  load_weights will back-fill "
                "them with a value this champion never trained against, so "
                "its play changes on the next load.")


class TheThreeWeightTablesAgreeOrSayWhyNot(unittest.TestCase):
    """`weighted.BASE_WEIGHTS`, `bots.WEIGHTS` and `book.V2_TUNABLES` are three
    hand-set tables and nothing compared them.  `bots.WEIGHTS` is GreedyBot's
    and is frozen ON PURPOSE -- it is the fingerprint control for NARROW and
    WIDE -- so the divergence is listed with both values rather than removed."""

    def test_greedy_weights_diverge_exactly_as_declared(self):
        found = {k: (B.WEIGHTS[k], W.DEFAULT_WEIGHTS[k])
                 for k in B.WEIGHTS
                 if k in W.DEFAULT_WEIGHTS
                 and B.WEIGHTS[k] != W.DEFAULT_WEIGHTS[k]}
        self.assertEqual(
            found, DELIBERATE_DIVERGENCE,
            "bots.WEIGHTS vs DEFAULT_WEIGHTS divergences changed.  These are "
            "FROZEN ON PURPOSE (GreedyBot is the NARROW/WIDE fingerprint "
            "control); syncing them moves two gate arms.")

    def test_greedy_only_keys_are_declared(self):
        extra = set(B.WEIGHTS) - set(W.DEFAULT_WEIGHTS)
        self.assertEqual(extra, set(GREEDY_ONLY))

    def test_every_greedy_weight_is_emitted_by_greedy_features(self):
        """The same bijection one module over: `bots.features` is a different
        vocabulary, so it needs its own check rather than borrowing this
        file's."""
        f = B.features(G.new_game(2, 1), G.new_game(2, 1).players[0])
        self.assertEqual(sorted(f), sorted(B.WEIGHTS))

    def test_the_book_tunables_are_a_separate_namespace(self):
        """`V2_TUNABLES` is BookBot's hand-written expert opinion and shares no
        coordinate with the evaluator.  If it ever does, the two must be
        reconciled rather than left to disagree silently."""
        overlap = sorted(set(book.V2_TUNABLES) & set(W.DEFAULT_WEIGHTS))
        self.assertEqual(overlap, [])


class NoModuleConstantNameCollides(unittest.TestCase):
    """`MARGIN_SCALE` was 120.0 in `experiments/hillclimb_pool.py` and 100.0 in
    `engine/bots/neural_net.py`: one name, two different jobs, and the reader
    who noticed would have "fixed" the disagreement.  Two files may share a
    name only if they share a value, or the collision is declared."""

    def test_no_two_modules_bind_one_name_to_different_literals(self):
        clash = {}
        for name, occ in _module_literals().items():
            files = {rel for rel, _ in occ}
            if len(files) < 2:
                continue
            values = {repr(v) for _, v in occ}
            if len(values) > 1:
                clash[name] = sorted(occ)
        stray = sorted(set(clash) - set(ALLOWED_NAME_COLLISIONS))
        self.assertEqual(
            stray, [],
            "\n".join([f"{len(stray)} module constant name(s) bound to "
                       "different values in different files:"]
                      + [f"    {n}: {clash[n]}" for n in stray]
                      + ["  -> rename one (MARGIN_SCALE -> MARGIN_NORM is the "
                         "precedent), or declare it in "
                         "ALLOWED_NAME_COLLISIONS."]))

    def test_margin_scale_stays_split(self):
        """The named instance, pinned by name so it cannot come back."""
        names = _module_literals()
        self.assertNotIn("MARGIN_SCALE", names)
        self.assertIn("MARGIN_NORM", names)


class NoEncodingSliceIsConstant(unittest.TestCase):
    """`encode()` is the neural net's whole view of the game, and a slot that
    is identical in every state is an input it can never learn from -- the
    exact analogue of a weight `features()` never emits.

    The 1897-wide vector is judged as NAMED slices (see `_encode_slices`), and
    each slice is judged only over the player counts where it is real, because
    the rival blocks are legitimately zero-padded at 2p and 3p."""

    def _constant(self):
        c = corpus()
        req = _index_requirement()
        out = {}
        for name, (indices, _) in _encode_slices().items():
            # PER INDEX, not per slice.  A one-hot is constant when each of
            # its five components is individually constant, which is a
            # different -- and much sharper -- statement than "min over the
            # block equals max over the block": the latter calls a frozen
            # one-hot live, because one component sits at 1.0 and the rest at
            # 0.0.  That was the first version of this check, and it silently
            # missed `current_events_age`.
            values = []
            for i in indices:
                need = req.get(i, 2)
                lo = hi = None
                for n in (2, 3, 4):
                    if n < need:
                        continue
                    a, b = c.enc[n][i]
                    lo = a if lo is None else min(lo, a)
                    hi = b if hi is None else max(hi, b)
                if lo != hi:
                    values = None
                    break
                values.append(lo)
            if values is not None:
                out[name] = values[0] if len(set(values)) == 1 else "frozen"
        return out

    def test_the_constant_slices_are_exactly_the_listed_ones(self):
        found = self._constant()
        live_dead = {f"encode:{k}" for k in found}
        listed = _dead("constant-encoding")
        detail = "\n".join(f"    encode:{k} = {v}"
                           for k, v in sorted(found.items()))
        self.assertEqual(
            live_dead, listed,
            _fmt(live_dead - listed, listed - live_dead,
                 "encoding slice(s) constant on every corpus state",
                 "make the slot vary, or stop encoding it")
            + "\n  full constant-slice report:\n" + detail)

    def test_the_scalar_names_still_line_up(self):
        """Anti-vacuity on the layout map itself: the names are duplicated
        from `_player_block`, so a scalar added there and not here would
        silently shift every name in the report by one."""
        self.assertEqual(len(SCALAR_NAMES), NE._PLAYER_SCALARS)
        self.assertEqual(len(set(SCALAR_NAMES)), len(SCALAR_NAMES))
        self.assertEqual(sum(w for _, w in GLOBAL_SLICES), NE._GLOBAL_DIM)
        covered = set()
        for indices, _ in _encode_slices().values():
            covered |= set(indices)
        self.assertEqual(len(covered), NE.ENCODING_DIM,
                         "the slice map does not cover the whole vector")

    def test_the_encoding_probe_is_not_vacuous(self):
        c = corpus()
        self.assertGreaterEqual(c.encode_states, MIN_CARD_STATES)
        for n in (2, 3, 4):
            self.assertEqual(len(c.enc[n]), NE.ENCODING_DIM, n)
        moved = sum(1 for _, (indices, _) in _encode_slices().items()
                    for i in indices
                    if c.enc[4][i][0] != c.enc[4][i][1])
        self.assertGreater(moved, 0.3 * NE.ENCODING_DIM,
                           "almost nothing in the encoding moved; the probe "
                           "has stopped probing")

    def test_the_padded_rival_blocks_really_are_padded(self):
        """The premise the whole segmentation rests on.  If a 2p game started
        writing into rival block 2, judging slices only where they are 'real'
        would be hiding data rather than excluding padding."""
        st = G.new_game(2, 3)
        vec = NE.encode(st, 0)
        base = NE._GLOBAL_DIM + NE._ROW_DIM
        for block in (2, 3):
            lo = base + block * NE.PLAYER_BLOCK_DIM
            chunk = vec[lo:lo + NE.PLAYER_BLOCK_DIM]
            self.assertEqual(set(chunk), {0.0}, f"rival block {block} at 2p")


class StateFieldsExistInBothRepresentations(unittest.TestCase):
    """A declared field the engine never writes is dead in EVERY
    representation at once, and `state.scoring_events` is the recorded case:
    declared, never written, never read back -- and a permanently-zero neural
    input on top."""

    def test_the_never_written_fields_are_exactly_the_listed_ones(self):
        declared = {name for _, _, name in _state_fields()}
        never = declared - _fields_the_engine_writes()
        listed = _dead("never-written")
        self.assertEqual(
            never, listed,
            _fmt(never - listed, listed - never,
                 "declared state field(s) the engine never writes",
                 "write it, or delete the field"))

    def test_every_written_field_is_encoded_or_declared(self):
        written = {name for _, _, name in _state_fields()} \
            & _fields_the_engine_writes()
        unencoded = written - _fields_the_encoder_reads()
        self.assertEqual(
            unencoded, set(NOT_ENCODED),
            _fmt(unencoded - set(NOT_ENCODED),
                 set(NOT_ENCODED) - unencoded,
                 "state field(s) the engine writes and encode() ignores",
                 "encode it, or add it to NOT_ENCODED with a reason.  An "
                 "entry LEAVING this list means a field started being "
                 "encoded, which needs an information-legality read"))

    def test_the_docstring_still_carries_its_hidden_information_list(self):
        """`neural_encode`'s own docstring is the allow-list of record for what
        is deliberately not encoded.  Assert the code matches it, so the two
        cannot drift -- and note this is an INFORMATION guard: a hidden field
        becoming visible is the net cheating."""
        doc = NE.__doc__ or ""
        self.assertIn("Deliberately NOT encoded", doc)
        for field, phrase in sorted(HIDDEN_BY_THE_RULES.items()):
            self.assertIn(phrase, doc,
                          f"the justification for not encoding {field} has "
                          "left the neural_encode docstring")

    def test_the_hidden_decks_are_not_read_at_all(self):
        reads = _fields_the_encoder_reads()
        for field in ("civil_deck", "military_deck"):
            self.assertNotIn(field, reads,
                             f"encode() reads {field}; deck ORDER is hidden")

    def test_the_event_decks_are_read_only_as_counts(self):
        """`current_events` order is hidden, its LENGTH is public.  Pinned
        structurally rather than by reading the code, because "we only use
        len()" is precisely the kind of claim that rots."""
        tree = _module_ast("engine/bots/neural_encode.py")
        wrapped = set()
        for node in ast.walk(tree):
            if isinstance(node, ast.Call) and isinstance(node.func, ast.Name) \
                    and node.func.id == "len":
                for sub in node.args:
                    if isinstance(sub, ast.Attribute):
                        wrapped.add((sub.attr, id(sub)))
        bare = []
        for node in ast.walk(tree):
            if isinstance(node, ast.Attribute) \
                    and node.attr in COUNT_ONLY_FIELDS \
                    and (node.attr, id(node)) not in wrapped:
                bare.append(node.attr)
        self.assertEqual(bare, [], f"{sorted(set(bare))} read outside len(); "
                         "that is reading the deck's ORDER")


class TheRatchetIsWellFormed(unittest.TestCase):
    """The allow-list is the project's scoreboard, so it has to be readable and
    it has to be honest about its own shape."""

    KINDS = frozenset(("unpriced-feature", "inert-vector", "inert-live",
                       "never-nonzero", "invisible-class", "stale-vector-key",
                       "constant-encoding", "never-written"))

    def test_every_entry_has_a_kind_a_reason_and_a_pointer(self):
        for name, entry in sorted(KNOWN_DEAD.items()):
            self.assertEqual(len(entry), 3, name)
            kinds, why, where = entry
            self.assertIsInstance(kinds, tuple, name)
            self.assertTrue(kinds, name)
            for kind in kinds:
                self.assertIn(kind, self.KINDS, name)
            self.assertGreater(len(why.strip()), 30,
                               f"{name}: the reason must say why, not what")
            self.assertTrue(where.strip(), name)

    def test_every_kind_is_actually_checked(self):
        """A kind nobody asserts is a kind that can grow without limit."""
        with open(os.path.abspath(__file__), encoding="utf-8") as fh:
            source = fh.read()
        for kind in self.KINDS:
            self.assertIn(f'_dead("{kind}")', source,
                          f"kind {kind!r} has no assertion behind it")

    def test_the_names_are_well_formed(self):
        fields = {name for _, _, name in _state_fields()}
        for name, (kinds, _, _) in sorted(KNOWN_DEAD.items()):
            if "invisible-class" in kinds:
                self.assertTrue(name.startswith("class:"), name)
            elif "stale-vector-key" in kinds:
                self.assertIn("@", name)
            elif "constant-encoding" in kinds:
                self.assertTrue(name.startswith("encode:"), name)
            elif "never-written" in kinds:
                self.assertIn(name, fields, name)
            else:
                self.assertIn(name, W.DEFAULT_WEIGHTS, name)

    def test_the_list_is_not_empty_and_not_enormous(self):
        """It is empty when the work is done, and if it is enormous the guard
        has been turned into a graveyard."""
        self.assertGreater(len(KNOWN_DEAD), 0)
        self.assertLess(len(KNOWN_DEAD), 60)


if __name__ == "__main__":
    unittest.main()
