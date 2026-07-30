# The military pricing seam, and the write-off that outlived its reason

2026-07-30.  Companion to docs/CARD_BLINDNESS.md (the civil-card census) and
docs/CARD_BLINDNESS_MILITARY.md (what the military hand cannot see).

Three things land here.  Only the first is plumbing; the other two are a
stale write-off and a write-off that is *not* stale and now says so.

---

## 1. `hand_mil_potential` never passed the board

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

## 2. STALE: the two bonus keys were written off for a reason that had expired

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
`test_no_key_is_both_priced_and_unpriced`) can catch a key no card carries
and a key that is claimed twice, but nothing could catch a *reason* that had
stopped being true.  The same staleness was in
`tools/card_blindness.py:reachable`, whose docstring asserted military-deck
cards are never asked about; it now says under which vector that holds.

### The three cards, and where their numbers come from

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

## 3. NOT STALE: `cost.militaryActions` stays unpriced, with a better reason

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

## 4. Conduction: which gate this opens, measured before any games

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
docs/CARD_BLINDNESS.md Sec 5.3's 12,800-game null, again.

## 4b. Where the 3p conduction actually shows up

A bonus card has **no move handler at all** (docs/CARD_CENSUS.md row 9): it is
never "played", only spent inside the defence and colonization machinery.  So
the only decision its price can reach is the one about *holding* it -- and
there is exactly one: `engine/interact.py:_discard_military`, RULES_SPEC §6.6
step 1, the end-of-turn military discard, which `docs/MILITARY_DISCARD.md`
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

## 5. Fingerprints

All eight `tools/gate.sh` arms play `DEFAULT_WEIGHTS`, in which
`hand_mil_potential` is 0.0, so `card_potential` is never called on a
military card and neither the seam fix nor the bonus mapping can reach a
digest.  Predicted inert before running, and the gate agrees: no digest
moved, and none was re-derived.
