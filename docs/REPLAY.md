# Human-game reconstruction (`rust/src/bin/replay.rs`) — status and fidelity

Companion to [`HUMAN_PLAY.md`](HUMAN_PLAY.md) (the corpus and the play-rate
census `corpuscensus.rs` computes over it — pure counting, no game state) and
to the shared parser both binaries use, `rust/src/corpus.rs`. This doc covers
the harder thing: **standing up an actual `GameState` from a human game's
journal and validating every move through the real engine**
(`legal::legal_moves` / `apply::apply`), which is the prerequisite for a
move-agreement analysis (stand at each decision point in a real game, ask the
bot what it would play, measure how often it agrees) that nothing in this
repo does yet. This doc is the spike's result, not that analysis.

## Status: not production-ready. 0/24 sampled games replay to completion.

That number is the headline, not a footnote — see "What did NOT complete"
below for why, and treat every category there as a punch list, not a verdict
on the approach. The binary DOES correctly validate a substantial prefix of
every sampled game (mean 33 actions, roughly rounds 1-5, before the first
stop) against the real engine, and along the way found and fixed three real
bugs in its own reconstruction logic (see "Bugs found and fixed during this
pass"). None of the stops below is a confirmed **engine rules bug** — see
that section for exactly what was and wasn't ruled out.

```text
tar -xzf sources/bgo/journals.tar.gz -C /tmp/bgo-journals
cargo run --profile difftest --bin replay -- \
    sources/bgo/index.tsv /tmp/bgo-journals/journals <game_id> [game_id ...]
```

## What is RECONSTRUCTED vs SIMULATED (read this before trusting any output)

The journal cannot tell the engine everything it needs: in particular the
true civil/military deck shuffle order, and what sat in a player's hand
before it was played. `replay.rs` draws a hard line between two kinds of
state:

- **RECONSTRUCTED**: every card identity a human is ever observed to take,
  build, develop, play, elect, declare, propose, colonize, bid on, or
  destroy. These come straight from the journal text via `tta::corpus::
  classify` (the same classifier `corpuscensus.rs` validated at 99.99% line
  coverage over the full 1,011-game corpus) and are applied as real engine
  `Move`s, checked against `legal_moves` at every step.
- **SIMULATED**: everything the engine needs to hold a legal, complete
  `GameState` that the journal never reveals — unrevealed card-row slots,
  the civil/military deck order beyond what's been drawn, and a player's
  hand contents before they are observed playing them. `replay.rs` seeds
  these from `game::new_game`'s ordinary (fictional, fixed-seed) shuffle and
  overwrites a slot/hand entry with the real observed card the instant that
  slot/card is ever taken or played ("grounding" it). An ungrounded slot's
  identity is never validated against anything and is never historically
  accurate — only its existence (some card was there, costing some action)
  is real.

**What a later agreement analysis is allowed to claim, given this**: any
decision point reached with the ACTING player's own hand/tableau fully
grounded (which by round 3-5 is essentially everything they've built,
developed, elected, played, or currently hold in civil hand) is a real
position the bot's `legal_moves` output can be meaningfully compared against.
Anything conditioned on an UNGROUNDED slot (what's still sitting unrevealed
in the row, an opponent's un-played military hand) is not, and any
agreement-rate tool built on top of this must gate on grounding state, not
just "did replay reach this line."

## Event/Territory preparation: the one inference this file makes, and why

`Move::PrepareEvent` (the Politics-phase action that queues a drawn Event or
Territory card to fire later) has **no journal line of its own** — BGO logs
only the resolution (`"X plays event ..."`), never the preparation. Every
`PrepareEvent` call causes exactly one `events::reveal_current_event`
(`rust/src/events.rs`), so `replay.rs` pre-scans each journal once,
collecting the exact card named in every `"...Current event:; <Age> /
<Name>; ..."` line into a FIFO. Whenever a player's Politics-phase decision
cannot be explained by an explicit textual action (pass, revolution, war,
aggression, pact offer), the binary infers a hidden `PrepareEvent`, grants
that player a placeholder Event-kind card (never checked against anything —
see SIMULATED above), and forces `state.current_events` to reveal exactly
the next journal-observed event/territory so the resolution the journal
shows next lines up.

This reproduces the right cards firing in the right ORDER, but **not** on
the historically correct turn and **not** by the historically correct
preparer — both are permanently unrecoverable from BGO's journal format. It
is the single biggest reason a "decider != expected actor" stop happens (see
below): the inference can fire for a player one turn "early" relative to the
true game, and by the time that shows up as a contradiction it's several
lines later and harder to trace back to its cause. This is a real,
structural limitation of the approach, not a bug to be fixed by more
parsing — the information is not in the journal.

One text-derivable fact was needed to keep this inference from
over-firing: BGO deals no military cards at all until a player's first
end-of-turn `"draws N military card(s)"` credit. Before that, a Politics
decision with no explicit action is a forced, sometimes-silently-logged pass
(no Event/Territory card can possibly exist in an empty hand), not a hidden
preparation — see "Bugs found and fixed" below, this was found empirically
on the first real game tested.

## What this file gives up on outright, and why

- **Discard** (§6.6 hand-limit, and any other forced military discard): BGO
  logs only a count (`"<Color> discards N cards"`), never which cards.
  Genuinely unrecoverable — stops the game rather than guess.
- **Aggression defense** with any committed defense cards: BGO logs only a
  count (`"<Color> defends N Defense card(s) played"`), never which. Zero
  committed cards is unambiguous (`DefendDone` immediately, the common case
  per the rulebook — committing defense cards is rare and costly); any
  positive count stops the game.
- **`PutBack`** (a human undoing their own `Take` via BGO's client-side
  undo): there is no `Move` for this in the engine at all — `moves.rs`'s
  variant list has no "untake". The common case (an undo immediately
  following its own take, ~8% of raw takes per `HUMAN_PLAY.md`'s earlier
  finding) is erased before the replay loop ever sees it: both journal lines
  are simply never applied, which IS what "take it back" means. An
  **unpaired** `PutBack` (no matching preceding take by the same actor —
  e.g. take, do something else, take back) is not currently handled and
  stops the game; this is a real gap in `replay.rs`, not an engine question.
- **Colonization sacrifice specifics**: `"Sacrificed Units:; ..."` DOES name
  exact identities, but resolving `Pending::Colonize`'s branching
  `SendUnit`/`SendBonus`/`SendDiscard` choices against that list is not
  implemented in this pass — the binary auto-drains colonization by picking
  the engine's own first-offered option at each step until the force
  clears. This keeps the game running and gets the reveal's own
  culture/resource totals right, but does not verify which units were
  spent; flagged per-game (`colonize_approximated`) whenever it fires. None
  of the 24 sampled games reached a colonization, so this was never
  exercised against real data in this pass.

## Sample: 24 games, 8 each of 2p/3p/4p

Selected as the first 8 games of each player count in `index.tsv` order — no
cherry-picking. Full run: `cargo run --profile difftest --bin replay --
sources/bgo/index.tsv /tmp/bgo-journals/journals <24 ids>`.

**0/24 replayed to completion with every human action legal.** Mean 33
actions consumed before the first stop (median similar), almost all in age
I round 3-5 — i.e. every game gets through the opening (round 1's take-only
restriction, the first political decisions, several rounds of real
take/build/develop/pop/elect/tactic play) validated against the real engine
before hitting one of the categories below. **No game's final score was
compared to `index.tsv`** (score(`game::scores`) is only computed for a
completed replay) — the single strongest end-to-end check this project has
available was never exercised in this pass. That is the most important
thing to fix next, not by chasing individual mismatches further but by
closing the categories below, roughly in the order listed (biggest count
first).

### Mismatch categories, ranked by frequency in this sample

| # games | category | representative line | verdict |
|---|---|---|---|
| 6 | `decider != expected actor` — the event-timing collapse (see above) leaves `state.current` on the wrong player once a hidden-`PrepareEvent` inference fires one turn early/late | `Purple builds Warrior` (StuckPending, decider 0 != expected 1) | Structural consequence of the documented inference, not a fresh bug. Only real fix is more precise turn attribution, which the journal format does not support directly — see "Event/Territory preparation" above. |
| 5 | Military discard, identity unrecoverable (4 via the plain `Discard` line, 1 via a `Pending::Choice(DiscardMilitary)` reached through a different code path — same root cause) | `Purple discards 1 card` | Genuine journal limitation (BGO logging quirk), documented, excluded on purpose. |
| 3 | Unpaired `PutBack` (no matching preceding take found by the adjacency-only prescan) | `Green puts Aristotle back in the row Green gets 2 civil action` | Parser gap in `replay.rs` (`prescan_putback_skips` only pairs an IMMEDIATELY preceding take of the same card by the same actor) — fixable; the corpus doc for this pattern already flagged it might not always be adjacent. |
| 3 | `Take{slot}` illegal — this binary's row-slot cost-matching (`ground_row_slot`) picked a slot whose `take_cost` doesn't match the journal's stated action-point cost | `Orange takes Breakthrough in hand Orange uses 1 civil action; Orange uses 1 military action` | Parser/reconstruction gap in `replay.rs`, not an engine question — the SIMULATED row content is this binary's own responsibility to place correctly. |
| 2 | `PolPass` illegal because `state.phase` is already `Actions` | `Green passes Political Phase` (legal_moves has no PolPass) | Downstream consequence of the event-timing collapse: this player's Politics decision was already (wrongly) resolved by an earlier hidden-`PrepareEvent` inference, so their real, explicit pass line arrives at a state that's moved on. |
| 2 | `WonderStep` illegal, blocked by an unidentified 2-option `Choose` | `Orange builds 1 stage of Pyramids Orange spends 3 resources` (legal_moves = `[Choose{0}, Choose{1}]`) | Not diagnosed in this pass — some `Pending::Choice` opens ahead of a wonder-stage build that this binary doesn't recognize (not `FreeBuild`, ruled out by testing). Flagged open, not guessed at. |
| 2 | `Pop`/`PopFree` both illegal (food or yellow-bank shortfall) | `Green increases population Green spends 2 food` | Economy drift from an earlier build/develop this binary priced differently than the true game — likely a second unmodeled discount mechanic, same shape as the Rich Land/Urban Growth bug below but not yet found. |
| 1 | `PlayAction` for a `FreeCivilAction` card opened no `Pending::Choice` | `Orange builds Religion using Urban Growth Orange spends 2 resources` | Edge case of the fix below: `push_choice` silently no-ops when the option list would be empty (mirrors `FreeBuild`'s documented behaviour) — this binary doesn't yet handle that no-op case, so it reports "no pending opened" instead of falling back sensibly. |

### Bugs found and fixed during this pass (in `replay.rs`, not the engine)

Found by testing against real games, in order of discovery — kept here
because each is a genuine "obvious in hindsight, invisible until tested
against real data" trap for anyone extending this binary:

1. **Row-slot grounding never expired.** A card-row slot marked "grounded"
   (forced to hold an observed card) stayed grounded forever, even after
   that card was taken and the slot refilled with new, unobserved filler.
   Later takes of the same action-point cost got force-placed into
   increasingly wrong slots as the pool of "still fresh" slots shrank. Fixed
   by ungrounding a slot the instant its card is taken.
2. **A coincidental row match short-circuited cost-based placement.**
   `new_game`'s fictional shuffle can, by chance, already contain the exact
   card a human is later observed taking (13 cards drawn from a 236-card
   table collides more often than intuition suggests). The original code
   trusted ANY row match; fixed to trust only slots THIS binary itself
   grounded, so an accidental match at the wrong (wrong-cost) slot doesn't
   short-circuit forced placement.
3. **Hidden-`PrepareEvent` inference over-fired in round 2.** Before a
   player's first military-card draw, their hand is genuinely empty and a
   missing "passes Political Phase" line is a forced pass, not a hidden
   preparation — the original inference couldn't tell the two apart and
   consumed a real event off the `event_reveals` queue one player-turn too
   early, misattributing every event downstream. Fixed by tracking each
   player's first `"draws N military card(s)"` credit and refusing to infer
   a preparation before it.
4. **An unmodeled build-discount mechanic (Rich Land, Urban Growth) was
   silently mispriced.** `"builds X using Y"` lines (11,773 occurrences
   corpus-wide, ~2.6% of all lines) name an Action card, in hand, that
   grants `Special::FreeCivilAction` — "build or upgrade a farm/mine for 1
   less" is Rich Land's printed text. The engine already implements this
   correctly (`ChoiceKind::FreeCivil`, reached via `Move::PlayAction`), but
   the original code applied a bare `Move::Build` and paid full price,
   draining the reconstructed economy by exactly the missed discount every
   time it fired — which doesn't fail AT that build, it fails several
   actions later when the shortfall finally blocks something, far from its
   real cause. This was BY FAR the largest single blocker before the fix
   (14/24 games in an earlier run of this same sample); fixed by detecting
   the `"using Y"` suffix and driving `PlayAction{Y}` → `Choose{n}` (the
   option that IS `Move::Build{card}`) instead. Also added a general
   stated-cost-vs-computed-cost cross-check on every plain build, so any
   FUTURE unmodeled discount source fails immediately at its source with a
   clear label, not several actions later as a confusing cascading failure.

## Suspected ENGINE rules bugs: none confirmed in this pass

**Zero** of the mismatch categories above are asserted to be an engine
legality decision that contradicts a legally-taken human action once state
was accurately reconstructed. Every category traces to one of: genuinely
unrecoverable hidden information (discard, defense), a documented
simplification in this binary's own approach (the event-timing collapse),
or an as-yet-unfixed gap in this binary's own move-translation coverage
(unpaired `PutBack`, the `Take`-slot cost-matcher, the unidentified 2-option
`WonderStep` blocker, the second unmodeled discount). The one item closest
to "worth a second look at the engine itself" is the `PlayAction`/
`FreeCivilAction` no-pending-opened case (1 game) — `push_choice`'s
documented silent-no-op-on-empty-options behaviour is plausible but not
confirmed as the actual cause here, and until it is this is filed as an
open question, not a finding. **This is a real result, not a failure to
look hard enough**: the standing project rule that "the rulebook is the
oracle and a human's legal move the engine rejects is worth more than the
replayer itself" was kept in mind at every stop above, and none qualified.

## Final-score cross-check: not exercised

`game::scores(&state)` (post-`finish_game`, matching what `index.tsv`'s
`results` column records) is only meaningful once a game reaches
`state.game_over` — which requires a FULL completion. Since 0/24 sampled
games completed, **the single strongest end-to-end check available
(matching final culture scores) was never run against real data in this
pass.** Running it is the natural next milestone once enough of the
categories above are closed that some games complete; `replay.rs` already
does the comparison (sorted-multiset, since seat order isn't guaranteed to
match `index.tsv`'s name order) the moment a game does.

## What to do next, roughly in priority order

1. Close the `decider != expected actor` / `PolPass`-already-Actions
   categories (8/24 games combined) — these are the SAME root cause
   (event-timing collapse) manifesting two ways; a more careful ordering of
   WHEN the hidden-`PrepareEvent` inference is allowed to fire (e.g. never
   ahead of an explicit political action for a DIFFERENT player whose own
   turn is chronologically due first) would likely close most of them
   without needing new journal information.
2. Fix the `Take{slot}` cost-matcher's remaining edge cases (3/24) and the
   non-adjacent `PutBack` case (3/24) — both bounded parsing problems, no
   new design needed.
3. Find the second unmodeled discount source behind the `Pop`/`PopFree`
   stops (2/24) the same way Rich Land/Urban Growth was found: instrument,
   run against a real game, read the numbers.
4. Diagnose the unidentified `WonderStep`-blocking `Choice` (2/24).
5. Only then: run the final-score cross-check for real, and scale the
   sample past 24 games.
