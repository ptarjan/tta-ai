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

## Status: not production-ready. 0/24 sampled games replay to completion —
## but the wall is now overwhelmingly genuine hidden information, not gaps

That number is the headline, not a footnote — see "What did NOT complete"
below for why. **This is the second pass over this binary.** The first pass
landed 0/24 complete at a mean of 33 actions consumed (roughly rounds 1-5)
before the first stop, with the categories blocking it dominated by this
binary's own coverage gaps. This pass closed every gap that pass named,
**found and fixed one confirmed engine rules bug** (see below, with a
before/after test), and found one **corpus data gap** (a card-name alias)
that turned out to be the single largest cause of the "event-timing
collapse" category — see "Bugs found and fixed" for all of them. The result:
still 0/24 complete on this exact sample, but the mean action count before a
stop rose from 33 to **42.75** (+30%), and of the 24 stops, **14 (58%) are
now genuinely unrecoverable hidden information (a forced military discard)**
— up from 7/24 in the first pass, not because discards got MORE common but
because fixing the other categories let more games run deep enough to reach
one. The remaining 10 stops are two **open, honestly-unresolved** structural
questions (see "What remains open" below) — not guessed at, not papered
over.

```text
tar -xzf sources/bgo/journals.tar.gz -C /tmp/bgo-journals
cargo run --profile difftest --bin replay -- \
    sources/bgo/index.tsv /tmp/bgo-journals/journals <game_id> [game_id ...]
```

Two env vars gate optional stderr tracing added during this pass, useful for
any future debugging session of the same shape (a "why did legal_moves not
offer this" hunt): `REPLAY_DEBUG=1` prints the acting player's civil/military
action counts, government, leader, phase, and top-of-pending-stack the
instant an attempted move turns out illegal; `REPLAY_DEBUG_ALL=1` additionally
prints that same state after every SUCCESSFUL move, plus a line per
`resolve_intervening` loop iteration and per hidden-`PrepareEvent` inference.
Both are silent (zero output, and the underlying values are cheap `Copy`
reads) unless the env var is set.

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
parsing — the information is not in the journal. **This pass fixed one
concrete, confirmed way this inference collapsed downstream state (the
`PlayEvent`-line premature-drain bug, below) but did NOT fully close this
category** — see "What remains open."

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
  Genuinely unrecoverable — stops the game rather than guess. **This is now
  the dominant stop reason: 14/24 (58%) of the sample** (11 via the plain
  `Discard` line, 3 via a `Pending::Choice(DiscardMilitary)` reached through
  a different code path — same root cause). This did not get WORSE — it got
  MORE VISIBLE, because every other category that used to stop a game
  earlier now lets it run deep enough to hit a real discard instead. Given
  BGO games run 15-20+ rounds and a forced discard fires whenever a
  player's military hand exceeds its limit (common, not rare), **most real
  games likely contain at least one discard eventually, meaning most
  100%-legal-move completions may be structurally unreachable regardless of
  how many more categories below get closed** — this is the single most
  important scoping fact for anyone continuing this work: chasing 24/24
  complete is very likely NOT achievable without either recovering discard
  identities (impossible, BGO doesn't log them) or accepting an
  approximation (which the project's own standing rule — "don't guess
  hidden cards, an honest low number beats a rosy fake one" — rules out).
- **Aggression defense** with any committed defense cards: BGO logs only a
  count (`"<Color> defends N Defense card(s) played"`), never which. Zero
  committed cards is unambiguous (`DefendDone` immediately, the common case
  per the rulebook — committing defense cards is rare and costly); any
  positive count stops the game. None of the 24 sampled games reached a
  contested defense in this pass.
- **`PutBack`** (a human undoing their own `Take` via BGO's client-side
  undo): there is no `Move` for this in the engine at all — `moves.rs`'s
  variant list has no "untake". **Fixed in this pass** for the common
  shapes (nested take/take-back stacks, and takes interrupted by an
  unrelated committed action before their own take-back) — see "Bugs found
  and fixed." A take-back with NO matching preceding take by the same actor
  anywhere in the game (a genuine parser gap, or a BGO logging artifact)
  would still stop the game; none of the 24 sampled games hit this residual
  case in this pass.
- **Colonization sacrifice specifics**: `"Sacrificed Units:; ..."` DOES name
  exact identities, but resolving `Pending::Colonize`'s branching
  `SendUnit`/`SendBonus`/`SendDiscard` choices against that list is not
  implemented in this pass — the binary auto-drains colonization by picking
  the engine's own first-offered option at each step until the force
  clears. This keeps the game running and gets the reveal's own
  culture/resource totals right, but does not verify which units were
  spent; flagged per-game (`colonize_approximated`) whenever it fires. None
  of the 24 sampled games reached a colonization, so this was never
  exercised against real data in either pass.

## Sample: 24 games, 8 each of 2p/3p/4p

Selected as the first 8 games of each player count in `index.tsv` order — no
cherry-picking. Full run: `cargo run --profile difftest --bin replay --
sources/bgo/index.tsv /tmp/bgo-journals/journals <24 ids>`.

**0/24 replayed to completion with every human action legal** (same as the
first pass), but the composition of WHY changed substantially:

| | first pass | this pass |
|---|---|---|
| games complete | 0/24 | 0/24 |
| mean actions before stop | 33 | 42.75 (+30%) |
| stopped on genuinely unrecoverable hidden info (discard) | 7/24 (29%) | 14/24 (58%) |
| stopped on a `replay.rs` coverage gap or open question | 17/24 (71%) | 10/24 (42%) |

**No game's final score was compared to `index.tsv`** (`game::scores` is
only computed for a completed replay) — see "Final-score cross-check" below
for why this is now expected to stay true for MOST games in this corpus
regardless of further `replay.rs` work, and what would actually move it.

### Mismatch categories in this pass, ranked by frequency

| # games | category | representative line | verdict |
|---|---|---|---|
| 14 | Military discard, identity unrecoverable (11 via the plain `Discard` line, 3 via a `Pending::Choice(DiscardMilitary)` reached through a different code path — same root cause) | `Purple discards 1 card` | Genuine journal limitation (BGO logging quirk), documented, excluded on purpose. Grew from 7/24 in the first pass purely because other fixes let more games run deep enough to reach one — see "What this file gives up on" above. |
| 5 | `decider != expected actor` — a DIFFERENT player's single action appears interleaved into the acting player's turn with **no intervening `EndTurn` line at all** | `Purple builds Philosophy` mid-Orange's-turn (StuckPending, decider 0 != expected 1), game `7523354` line 26 | **Open, unresolved — see "What remains open" below.** Distinct from (and a smaller residue of) the first pass's "event-timing collapse" diagnosis: closing the `PlayEvent`-drain bug and the `Development of Civilization` alias (both below) closed most of the ORIGINAL 8/24 in this category, but left this smaller, structurally different puzzle — the interleaved player has NO pending state open at all when `resolve_intervening` reaches them, and `state.current` genuinely still names the OTHER player. Not guessed at. |
| 3 | `Pop`/`PopFree` both illegal, `civil_actions == 0` with a full budget's worth of actions already correctly accounted for | `Green increases population Green spends 2 food` (legal_moves has no Take/Pop/Build at all) | **Open, unresolved — see "What remains open" below.** Same OBSERVED shape as the Pyramids engine bug this pass fixed (a real player using one MORE civil-costing action than `Despotism`'s printed 4 plus every currently-modeled bonus source explains) but confirmed NOT explained by that fix, by Hammurabi's MA-as-CA conversion, or by Michelangelo's/any other in-play card's `CardEffects`. A genuinely unidentified extra source, or a different bug entirely. |
| 1 | `Take{slot}` illegal, same `civil_actions == 0` shape as the `Pop`/`PopFree` row above | `Purple takes Colossus in hand Purple uses 1 civil action` (legal_moves has no Take at all) | Same open question as the row above — this is the SAME category (a global action-budget shortfall, not a slot-cost mismatch: `legal_moves` excludes every row slot, not just one), split into its own row only because `replay.rs` reports it as a different `MismatchKind` variant. The original "row-slot cost-matcher" diagnosis from the first pass (a `ground_row_slot` placement bug) is CLOSED — every instance of it in this pass's original 3/24 was actually this same action-budget issue in disguise, confirmed by the Pyramids fix (below) resolving all three. This residual instance is a DIFFERENT, still-open cause of the same symptom. |
| 1 | `PlayAction` for a `FreeCivilAction` card opened no `Pending::Choice` | `Orange builds Religion using Urban Growth Orange spends 2 resources` | Unchanged from the first pass. Edge case of the Rich Land/Urban Growth fix (below): `push_choice` silently no-ops when the option list would be empty (mirrors `FreeBuild`'s documented behaviour) — this binary doesn't yet handle that no-op case, so it reports "no pending opened" instead of falling back sensibly. Not investigated further this pass (a single occurrence, and the fix would be in `replay.rs`'s own translation, not the engine). |

Every category the first pass's table listed that is NOT in the table above
is **CLOSED** this pass: unpaired `PutBack` (was 3/24), the `Take{slot}`
row-slot cost-matcher as originally diagnosed (was 3/24 — see above, folded
into the action-budget question instead), the unidentified 2-option
`WonderStep`-blocking `Choice` (was 2/24, identified as `ChoiceKind::
GainBlock` and fixed), and `PolPass` illegal because `state.phase` was
already `Actions` (was 2/24, a downstream symptom of the `PlayEvent`-drain
bug, fixed at its source).

## Bugs found and fixed during this pass (`replay.rs` unless marked ENGINE)

In order of discovery, kept here for the same reason the first pass's list
is kept: each is a genuine "obvious in hindsight, invisible until tested
against real data" trap.

1. **Unpaired take-backs were only matched if immediately adjacent.**
   `prescan_putback_skips` tracked a single `last_take` slot, reset by ANY
   intervening classified line — but BGO's UI lets a player hold several
   tentative takes at once (take A, take B, put B back, put A back — a
   real, observed nested pattern) and freely interleave unrelated committed
   actions (a build, another take) between a take and its own later
   take-back. Fixed by replacing the single slot with a per-card stack of
   still-open takes (`HashMap<CardId, Vec<(usize, Color)>>`): a `PutBack`
   pops the most recent same-actor entry for that EXACT card (safe because
   every row/hand card is a unique instance in the table — the same
   assumption `ground_row_slot` already relies on), and any OTHER
   classified line for that same (actor, card) — i.e. the take got
   committed some other way — removes it from the stack defensively, so a
   theoretical later same-named "put back" can never wrongly erase a real
   action. Closed 3/24 games in the original sample.

2. **ENGINE BUG (confirmed, fixed, tested): a civil/military action bonus
   from a card entering play mid-turn was not usable until the NEXT
   turn.** `p.civil_actions`/`p.military_actions` are a decrementing-only
   per-turn pool, set once when a player's Politics phase closes
   (`effects::state_stats`'s `civil_actions`/`military_actions`, which
   sums the government's base plus every currently-in-play card's
   `CardEffects.civil_actions`/`military_actions` — e.g. Pyramids' printed
   `+1 civil action`, Code of Laws/Justice System/Civil Service/Kremlin,
   Julius Caesar/Napoleon/Joan of Arc/Robespierre's `+N military actions`).
   `set_government` already correctly topped up `p.civil_actions` the
   instant a mid-turn revolution changed government (`p.civil_actions =
   (s.civil_actions - spent).max(0)`) — but `on_enter_play`/`on_leave_play`
   (`apply.rs`, the shared hook every wonder-completion, tech-development,
   and leader-election handler already calls for blue/yellow tokens) never
   applied the SAME top-up for a card's own `civil_actions`/
   `military_actions` effect. Found by testing against a real 2p game
   (`7523350`): a human completed Pyramids mid-turn (their `civil_actions`
   was 0 at that instant, having just paid for the completing `WonderStep`)
   and immediately took a 5th action that same turn using the wonder's
   freshly-granted `+1`; the reconstructed engine rejected it — a REAL
   rules disagreement, not a parser gap, since `ground_row_slot`'s SLOT
   placement was already correct (`legal_moves` excluded every row slot,
   not just the wrong one). **Fixed** by adding the same `+= ` (on enter)
   `/ -=` (on leave, symmetric, for a leader swap) bump to
   `on_enter_play`/`on_leave_play` for these two `CardEffects` fields,
   clamped at 0. **Test**: `apply::tests::
   do_wonder_step_completing_pyramids_grants_the_extra_civil_action_this_turn`
   (`rust/src/apply.rs`) — builds a player with `civil_actions: 1`, pays
   the final Pyramids stage down to 0, asserts it's back to 1 after
   completion. Confirmed to fail before the fix (reverted the `on_enter_play`
   bump behind a `false &&` guard, reran: `left: 0, right: 1` — the
   assertion failure the bug predicts) and pass after (restored). This ALSO
   fully closed the first pass's "row-slot cost-matcher" category (3/24) —
   every instance was this same budget shortfall, not a slot-placement bug;
   `ground_row_slot`'s cost-based placement was correct all along.

3. **A `ChoiceKind::GainBlock` pending (an event's "Each civilization gains
   2 resources or 2 food, player's choice" — e.g. Development of Markets)
   blocked a LATER, unrelated action because nothing ever resolved it.**
   BGO logs each player's own pick as a standalone bookkeeping line
   (`"<Color> produces N food"` / `"<Color> produces N resources"`) that
   `corpus.rs` correctly treats as non-actionable for census purposes — but
   `replay.rs` needs a real `Move::Choose` to actually clear the pending.
   Found by testing against a real 3p game (`7522632`): a `WonderStep`
   several lines later failed with `legal_moves = [Choose{0}, Choose{1}]`
   because the ACTING player's own choice from an earlier event had never
   been cleared. **Fixed** by pre-scanning every standalone `"produces"`
   line into a per-seat FIFO (`prescan_gain_produces`) and draining any open
   `GainBlock` pending against it (matching by comparing the printed amount
   to each `Gain` option, not by position) the moment `resolve_intervening`
   sees one — regardless of whose turn it nominally is, mirroring the
   existing `FreeBuild` pattern. Closed 2/24 games.

4. **The single biggest fix this pass: `resolve_intervening`'s call for
   the `ActionClass::PlayEvent` line itself wrongly auto-drained EVERY
   qualifying player's `FreeBuild`/`GainBlock` choice from the SAME
   event before any of them were ever read.** `"X plays event ..."` is
   BGO's journal-side CONFIRMATION that an event already resolved — the
   engine resolves the event's ENTIRE effect (gains, and a
   `FreeBuild`/`GainBlock` choice queued for every qualifying player at
   once) synchronously inside `h_prepare_event`, the instant the hidden
   `PrepareEvent` is inferred, and `apply_one`'s own `PlayEvent` arm is a
   bare `Ok(())` that touches no state. But `resolve_intervening` was still
   being CALLED for that line, with `upcoming = (PlayEvent, None)` — and
   `FreeBuild`'s existing "if the upcoming line doesn't match one of my
   options, assume `Skip`" fallback (a correct heuristic for a player who
   genuinely declines) fired for EVERY qualifying player, because a
   `PlayEvent` line can never match a build. Found by testing against a
   real 3p game (`7522652`) where "Development of Religion" opened a
   `FreeBuild` choice for all 3 players and every one of them was silently,
   wrongly auto-`Skip`ped before their own real `"builds Religion"` lines —
   which follow two lines later — were ever read; the FIRST player's own
   later free build then fell through to a normal, FULL-PRICE `Move::Build`
   (which happened to still be legal, paying real resources this binary's
   simulated economy had, silently draining it) instead of the free
   `Move::Choose`. **Fixed** by skipping the `resolve_intervening` call
   entirely for `ActionClass::PlayEvent` lines — safe because `apply_one`'s
   handling of that class reads no state either, so nothing is lost;
   resolution (including the hidden-`PrepareEvent` inference itself, if it
   hasn't fired yet) is simply deferred to whatever the NEXT real line's own
   `resolve_intervening` call needs, which has the correct `upcoming` to
   match against. This one fix alone raised the sample's mean actions
   consumed noticeably and closed most of the first pass's 8/24
   `decider != expected actor` / `PolPass`-illegal combined category (5/24
   remain — see "What remains open").

5. **CORPUS DATA GAP (not an engine bug): BGO's own display name for the
   Age A event "Development of Civil Life" is "Development of
   Civilization" — an alias missing from `corpus.rs::ALIASES`.** Found by
   testing against a real 2p game (`7523354`): `current_event_name`'s
   lookup for `"Development of Civilization"` silently failed (returned
   `None`), so `prescan_event_reveals` dropped that event from the
   `event_reveals` FIFO entirely — shifting every LATER event in that same
   game one slot out of alignment, the exact mechanism the module doc's
   "Event/Territory preparation" section describes as the single biggest
   cause of `decider != expected actor`. Confirmed by exact text match:
   the journal's printed flavour text ("Immediately, each civilization may
   either: increase its population; or build a farm, mine or urban
   building; or develop a technology. It costs 1 [resource] less than
   usual.") is verbatim `state.rs`'s own doc-comment quote of "Development
   of Civil Life"'s real card text. **This card appears in 471 of the
   corpus's 1,011 games (47%) and in 14 of this sample's 24 (58%)** — by a
   wide margin the single highest-leverage fix in this pass, and the module
   doc's own prior claim ("zero games contain a card name outside the 2015
   base game — checked, not just trusted") turns out to have verified a
   NARROWER property than advertised: it covered every card identity
   `corpuscensus.rs` actually resolves for its own purposes, not every name
   embedded in a `"Current event:; ..."` clause specifically — this is the
   first time anything read that clause's name against `card_index` for
   real. **Fixed** by adding `("Development of Civilization", "Development
   of Civil Life")` to `ALIASES`. Whether this fully explains any GIVEN
   game's `decider != expected actor` stop varies (5/24 remain, on games
   where the interleaving has a different, still-open cause — see next
   section) but it measurably deepened every game that reaches this event,
   and is the correct, permanent fix regardless.

### Bugs found and fixed in the FIRST pass (kept for continuity)

1. Row-slot grounding never expired after a take (fixed).
2. A coincidental row match short-circuited cost-based placement (fixed).
3. The hidden-`PrepareEvent` inference over-fired before a player's first
   military draw (fixed).
4. An unmodeled build-discount mechanic (Rich Land, Urban Growth) was
   silently mispriced — `"builds X using Y"` needed `PlayAction{Y}` →
   `Choose{n}`, not a bare `Move::Build` (fixed).

See the first pass's git history for the full detail on these four; nothing
in this pass touched them further.

## What remains open — two genuinely unresolved structural questions

**Superseded by "Fourth pass" (below the Third pass section, further down
this file) — kept here for continuity/history, not as the current status.**
Both questions this section names were diagnosed in the fourth pass: #1
(interleaving) was root-caused to Development of Civil Life's out-of-turn
grant and fixed in `replay.rs` (9/24 → 1/24 residual); #2 (budget shortfall)
turned out to be two confirmed engine bugs plus a genuinely unidentified
residual (8/24 → 2/24). The `politics_done`-vs-global-`phase` theory #1
proposes below was checked and does NOT explain the dominant shape — see
the fourth pass's own section for why. Read on for the ORIGINAL diagnosis
as it stood after the second pass, then skip to "Fourth pass" for what is
actually still true today.

Both are reported here, honestly, as OPEN rather than guessed at or papered
over, per this project's standing rule that a confirmed rules disagreement
(or an honestly-flagged open question) is worth more than forcing a
completion.

### 1. A single action from a DIFFERENT player interleaves mid-turn, with no `EndTurn` between (5/24)

In every other game examined across both passes, one player's full turn
(Politics decision, then every one of their Actions-phase moves, then
`EndTurn`) is strictly self-contained before the next player's turn begins
in the journal. In the 5 remaining `decider != expected actor` games, this
breaks: e.g. `7523354` line 24-26 —

```text
Orange plays event ... Development of Civilization ...
Orange increases population Orange spends 1 food
Purple builds Philosophy Purple spends 2 resources     <- interjects, no EndTurn for Orange
Orange takes Hanging Gardens in hand Orange uses 1 civil action   <- Orange's turn resumes
...
Orange End turn ...
```

At the point of failure, `resolve_intervening` finds `state.pending` EMPTY
and `state.decider()` (== `state.current`, since nothing is pending)
genuinely still names Orange — there is no queued choice, no stale pending,
nothing this pass's `GainBlock`/`FreeBuild`/`PlayEvent` fixes address. Two
candidate explanations were considered but NOT confirmed:

- **A per-player Politics/Actions tracking gap.** `PlayerState::
  politics_done` is a genuine PER-PLAYER field (distinct from the single
  GLOBAL `state.phase`, which `end_politics` sets to `Actions` the moment
  ANY one player finishes their own political decision). `resolve_
  intervening`'s existing fallback for an out-of-turn political decision
  gates on the GLOBAL `state.phase == Phase::Politics`, which is false here
  (it reflects Orange, who is mid-Actions) — so if Purple's OWN unresolved
  political decision is what's actually blocking things, checking `!state.
  players[decider].politics_done` instead of the global phase might be the
  right fix. This was NOT implemented or tested this pass — `state.
  decider()` reporting Orange (not Purple) when nothing is pending means
  there is no live signal that Purple even has anything outstanding, so
  this theory is unconfirmed, not a diagnosis.
- **A genuine async-submission artifact in BGO's journal**, given this
  corpus is a correspondence-style (real-world-hours-to-days between moves)
  game and its timestamps for the interjecting line sit strictly between
  the two halves of the "interrupted" player's turn — plausible, but not
  verified against a second, independent signal.

**Do not paper over this by hand-mutating `state.current`** — that would
mask a real question (is the engine's per-player turn tracking correct
here, or is this genuinely an artifact BGO's journal format cannot
resolve?) behind a forced match. Flagged open.

### 2. A player needs one MORE civil action than the computed budget provides, with no known bonus source (4/24)

Same OBSERVED shape as the Pyramids engine bug this pass fixed and
confirmed (`legal_moves` excludes every Take/Pop/Build — a genuine
`civil_actions == 0` shortfall, not a slot- or cost-specific mismatch) but
**confirmed NOT explained by that fix** in at least one case examined in
detail (`7523087`, Purple electing Michelangelo mid-turn, replacing
Hammurabi): every civil-costing action that turn was individually correct
(the leader-replacement refund nets to zero exactly as §9.1 and the
existing `h_play_leader` code predict; neither Hammurabi's nor
Michelangelo's `CardEffects`/`Special` grant any `civil_actions` bonus per
`card_table.rs`), yet the human still took one MORE action than
`Despotism`'s printed 4 explains. Whether this is:

- a THIRD unmodeled action-economy source (some other card's ability not
  yet identified),
- the SAME root cause as the interleaving mystery above (i.e. an action
  the engine attributes to the wrong player's budget), or
- something else entirely,

is **not diagnosed** — flagged here as a confirmed, reproducible symptom
(4/24 games, `REPLAY_DEBUG=1`/`REPLAY_DEBUG_ALL=1` reproduce the full
civil-action trace for any of them) rather than guessed at. **Do not paper
over this by force-granting an extra action** — see the same standing rule
as above.

## Suspected ENGINE rules bugs

**Superseded by "Fourth pass" (further down this file)** — the fourth pass
confirmed and fixed TWO MORE engine bugs beyond the one below: event-granted
population increases wrongly charging food (`events::tests::
an_event_granted_population_increase_costs_no_food`), and Development of
Civil Life's ordered action never getting the same CA exemption Rich
Land/Urban Growth's identical grant already has (`apply::tests::
do_build_spends_no_civil_action_when_civil_life_banked_a_build_discount`,
`legal::tests::
build_move_is_offered_with_zero_civil_actions_when_civil_life_banked_a_discount`).
Three confirmed engine bugs total across this file's history, all fixed and
tested. The historical text below is kept for continuity.

**One confirmed and fixed in the second pass** — see "Bugs found and fixed"
item 2 above (the mid-turn `civil_actions`/`military_actions` top-up), with
a before/after test (`do_wonder_step_completing_pyramids_grants_the_extra_
civil_action_this_turn`, `rust/src/apply.rs`).

**One still open, NOT confirmed, as of the second pass**: item 2 under "What
remains open" above (the unexplained extra civil action) is the closest
remaining candidate for a SECOND engine bug — the OBSERVED shape (budget
shortfall with every known source individually verified correct) is
identical to the one that turned out to be real, but a second bonus source
has not been found or ruled out, and might instead be a `replay.rs`
reconstruction gap (a build this binary priced differently than the true
game, echoing the first pass's Rich Land/Urban Growth discovery) or the same
interleaving mystery in disguise. Filed open per the standing rule, not
asserted as a finding. **Resolved by the fourth pass** (see above) — mostly
Finding 1b (Civil Life's missing CA exemption), with a genuinely
unidentified 2/24 residual still open.

Every OTHER category in this pass's mismatch table traces to one of:
genuinely unrecoverable hidden information (discard), or an as-yet-unfixed
`replay.rs` translation gap (the single `FreeCivilAction` no-pending-opened
case). None qualifies as a confirmed engine disagreement.

## Final-score cross-check: not exercised, and unlikely to be reachable for most of this corpus without a scoping decision

`game::scores(&state)` (post-`finish_game`, matching what `index.tsv`'s
`results` column records) is only meaningful once a game reaches
`state.game_over` — which requires a FULL completion. Since 0/24 sampled
games completed in either pass, **the single strongest end-to-end check
available (matching final culture scores) was never run against real data.**
`replay.rs` already does the comparison (sorted-multiset, since seat order
isn't guaranteed to match `index.tsv`'s name order) the moment a game does.

**Scoping reality after this pass**: 58% of this sample now stops on a
genuinely unrecoverable forced discard — a full BGO game (15-20+ rounds)
gives a forced discard many chances to fire, so closing the two open
questions above would very likely still leave MOST games blocked on a
discard reached slightly later, not complete. Reaching a meaningful number
of full completions most likely requires an explicit, honestly-labelled
scoping decision (e.g. sampling for games that happen to avoid a discard
entirely, or accepting that "verified-legal-prefix depth" rather than "full
completion count" is this method's real deliverable) rather than more
`replay.rs` fixes of the same shape as this pass's.

## What to do next, roughly in priority order

1. Diagnose the two open items above ("What remains open") — both are
   reproducible on a small, named set of real games with `REPLAY_DEBUG_ALL=1`
   already wired up to show the exact state at the point of failure. The
   `politics_done`-vs-global-`phase` theory for item 1 is the most concrete
   unstarted lead.
2. Fix the single remaining `FreeCivilAction` no-pending-opened case (1/24)
   — a bounded `replay.rs` translation gap, not urgent.
3. Make an explicit scoping decision about the discard wall (see "Final-score
   cross-check" above) before investing further in completion count as the
   headline metric — it may be the wrong metric for this journal format.
4. Only then: run the final-score cross-check for real on whatever games DO
   complete under the current or a rescoped sample, and scale the sample
   past 24 games. Re-run `corpuscensus.rs`-style census over the newly
   confirmed alias to see if any OTHER corpus-wide name gaps exist (the
   `Development of Civilization` alias, found in this pass, was previously
   invisible to every prior validation pass).

## Third pass: military discard, solved by constraint propagation — not given up on

The prior section calls the forced military discard "genuinely
unrecoverable hidden information" and reports it as 14/24 (58%) of the
sample's stops, with the framing that most real games likely contain one
eventually and closing it was probably not worth attempting. **That framing
was wrong about recoverability, right about difficulty, and the fix was
worth doing anyway** — this section reports it plainly, including the parts
that didn't work out the way a first read of the idea suggests they should.

### The argument, and where it actually lands

BGO's journal never names which card a `"<Color> discards N cards"` line
removed — only the count. But a card a player is later observed **playing**
(`Move::War`/`Move::Aggression`/`Move::OfferPact`/`Move::PlayTactic` all
name the card on their own journal line) was, by definition, still in their
hand at the time of an earlier discard — it cannot have been discarded
there. `rust/src/discard_solver.rs` (new module, kept separate from
`replay.rs`/`apply.rs` on purpose — see below) implements exactly this:
pre-scan the whole journal once for every FUTURE such named play per seat
(`prescan_future_military_needs` in `replay.rs`), and at each discard
decision, rule out any of the engine's own currently-offered
`discard_options` candidates that reappears later. Three honestly-separated
outcomes, matching this project's standing rule that a completion rate
built mostly on guesses is weaker evidence than one built on facts:

- **Solved**: exactly one candidate survives the filter — not a guess.
- **Chosen**: more than one survives (genuine ambiguity) — picks the
  least-valuable one (`interact::discard_options` already sorts
  worst-defender-first), matching a rational real player's own incentive,
  per the explicit instruction: "if there really is some ambiguity that's
  fine, just of the valid possibilities choose one."
- **Forced collision**: EVERY candidate reappears later (the filter
  couldn't help at all) — still picks one (the least-valuable), but this is
  the "detect and report rather than silently let it pass" case: it means
  either this game's simulated hand undercounts what the real hand held (an
  earlier divergence), or a duplicate-named military card's second copy was
  the real discard. Counted and surfaced separately, never folded into
  "solved."

`DiscardSolver::choose` is pure and has 7 unit tests covering all three
buckets, including that a future need BEFORE the current line does NOT
exclude a candidate (already left the hand, so it's fair game again), and
that needs are scoped per-seat (another player's future play must not
exclude this player's candidate). Wired into `replay.rs` at two points:
`resolve_intervening` drains any OTHER player's stale `DiscardMilitary`
pending immediately (closing the "reached through a different code path"
shape the previous pass reported as 3/24 of the discard total), and
`apply_one`'s `ActionClass::Discard` arm resolves the CURRENT actor's own
discard via `Replayer::resolve_discard`. Those had to be split, not merged
into one unconditional drain, because of a real trap: resolving the LAST
queued discard can itself finish that player's end of turn
(`interact::QueueItem::DiscardMilitary` resumes `game::resume_end_turn`
once it clears) and advance `state.current` to the next player — draining
unconditionally ahead of the decider-equality check made `resolve_
intervening` see `decider != expected_actor` on a line that had, in fact,
just been fully and correctly consumed, and report every such line as
stuck. Found by testing against real games (the first commit of this pass
regressed to 0 stops recovered at all until this was fixed) — see the
second commit's own message for the full trace.

### What this closes, honestly, including why it mostly does NOT produce "Solved"

On the same 24-game sample (8 each of 2p/3p/4p, first-in-`index.tsv` order,
identical selection to both prior passes):

| | second pass | third pass (this one) |
|---|---|---|
| games complete | 0/24 | 0/24 |
| mean actions before stop | 42.75 | **51.0 (+19%)** |
| stopped ON a military discard | 14/24 (58%) | **0/24** |
| military discards resolved (not stopped on) | 0 | **63** (0 solved, 63 chosen, 0 forced collisions) |

Every one of the 24 games individually reached the same or a strictly
later stop point than the second pass; none regressed. **Discard is no
longer this sample's bottleneck at all** — every game that used to stop on
one now runs straight through it into whatever the NEXT real issue is.

The honest disappointment: **of 63 discards resolved, 0 were "Solved" —
all 63 landed in "Chosen."** This is not a bug in the solver (the unit
tests confirm `choose` correctly narrows to one, or flags a forced
collision, whenever the input data supports it) — it is a real, structural
fact about this corpus that the task's original framing (draw analogy: "you
know every card taken from the row, and every card later played") does not
quite fit for MILITARY cards specifically, and is worth stating precisely
so nobody re-derives it the hard way: **military-hand cards are never named
at draw time.** Unlike a civil-row `Take`, which BGO logs with the card's
name every time, a military card enters a hand via an anonymous
end-of-turn draw (`"<Color> draws N military card(s)"` — a count only, see
`corpus.rs`). `replay.rs`'s reconstructed `hand_military` is therefore
SIMULATED filler for essentially its entire content at any moment a discard
decision is open — `Replayer::ground_military_hand` only ever grounds a
card's real identity in the SAME function call that immediately consumes it
(war/aggression/pact/tactic all ground-then-remove atomically), so no
journal-verified card is ever sitting in hand at a discard boundary to be
excluded from consideration in the first place. The exclusion this module
performs is real and correctly implemented, but it can only fire when a
piece of SIMULATED filler already in hand happens to coincide, by CardId,
with a card independently named by a later play — a coincidence, not a
structural certainty, and this sample was simply too small (and the games
too short before hitting the NEXT open issue) to hit many. This is reported
as a scoping fact for anyone tempted to expect "Solved" to dominate at
scale: it will remain a minority category under this reconstruction model
regardless of sample size, because the underlying journal signal for
military cards specifically is thinner than for civil ones. **Zero forced
collisions** occurred either, for the same reason (the exclusion rarely
fires at all, so it rarely fires universally).

### Why 0/24 still complete, and what actually blocks each game now

Discard being solved does not, by itself, reach completion — it just moves
the wall. Re-triaging all 24 stops on this pass's output:

| # games | category | status |
|---|---|---|
| 9 | `decider != expected actor` (interleaving, no `EndTurn` between) | Same OPEN question as the second pass — see "What remains open" above. Grew from 5/24 to 9/24 purely because more games now run deep enough to reach it, the same "closing one wall exposes the next" pattern that grew the discard category from 7/24 to 14/24 across the first two passes. |
| 8 | Civil-action-budget shortfall (`Pop`/`PopFree`/`Take`/`Build` all illegal, `civil_actions == 0`) | Same OPEN question as the second pass — see "What remains open" above. Grew from 4/24 to 8/24, same reason. |
| 2 | `"plays Frugality ... increases population"` — `PlayAction{Frugality}` not offered by `legal_moves` at all | **Newly reached, NOT diagnosed.** Not investigated this pass (out of scope: this is a `replay.rs`/engine question unrelated to discards, and this repo has a second agent actively working `apply.rs`'s action-economy bug concurrently with this pass — deliberately left alone rather than risk stepping on that work). Plausibly the same general shape as the already-known "`FreeCivilAction` no-pending-opened" gap (row below) since Frugality also grants a `FreeCivilAction`, but for `IncreasePopulation` rather than `Build` — NOT confirmed, flagged as a lead only. |
| 2 | `"builds X using Urban Growth"` opens no `Pending::Choice` | Same OPEN gap the second pass reported as 1/24 (`FreeCivilAction` no-pending-opened) — grew to 2/24, same "runs deeper now" reason. Not investigated further this pass. |
| 1 | `WonderStep` illegal completing a wonder | **Newly reached, NOT diagnosed.** A single occurrence; not investigated this pass for the same reason as the Frugality row above. |
| 1 | `PolPass` illegal (`state.phase` apparently already past Politics) | **Newly reached, NOT diagnosed.** A single occurrence, same shape as a `PlayEvent`-drain bug the second pass fixed at its source — possibly a NEW instance of a similar timing issue, not confirmed. |
| 1 | Build cost mismatch (unmodeled discount, `Warriors`) | Unchanged from the second pass's own "gives up on" list (an unmodeled discount source, not a parser gap). |

No new engine (`apply.rs`/`legal.rs`) bug is claimed or confirmed by this
pass — every mismatch above is either a previously-open question growing
because games run deeper (expected, not new information) or a genuinely
new-looking `replay.rs`-level gap flagged but explicitly NOT chased down,
both to stay in scope (this pass's job was discard, not the action-economy
investigation already underway elsewhere in this repo) and because a
single occurrence each is too thin a sample to diagnose responsibly.

### Final-score cross-check: still not reached

`game::scores(&state)` vs `index.tsv`'s `results` column still could not be
run — 0/24 games in this sample reached `state.game_over`. This remains
true for the same structural reason the second pass named (a full BGO game
gives every one of the categories above, not just discard, many chances to
fire over 15-20+ rounds) — closing discard alone was never expected to be
sufficient, and this pass confirms that prediction rather than
contradicting it. The two OPEN questions ("What remains open," above)
are now this project's highest-leverage remaining unknowns for reaching any
completions at all.

## Fourth pass: both "What remains open" questions diagnosed — one is a confirmed second ENGINE bug, the other a replayer/turn-model gap, both fixed

This pass had one job: diagnose the two structural open questions above,
verify rather than assume, and fix whatever turned out to be real. Both
were root-caused with hard evidence from real games, both are now fixed and
tested, and the sample's composition moved a lot as a direct result —
reported here exactly as it landed, including the parts that are STILL open.

### Finding 1 (civil-action-budget shortfall, was 8/24): a SECOND confirmed engine bug, plus one already-fixed bug's blast radius

The "civil_actions == 0 shortfall" shape (§9.2's original framing) turned
out to be at least two, and possibly three, unrelated things wearing the
same symptom:

**1a. ENGINE BUG (confirmed, fixed, tested): event-granted population
increases were charged food as if they were the PAID §6.1 action.**
`Development of Settlement`/`Immigration`/`Refugees` are the only three
base-game cards whose `EventBlock` carries an `increasePopulation` key —
`events.rs::apply_gains_block` routed it through `paid_increase_population`
(the same food-costing, one-time-discount-consuming path as a normal
`Move::Pop`), but the digital-edition card text for all three
("Players increase population.", "The players with the most happy faces
increase population.", "...gains 3 culture and increases population.") is
phrased exactly like every other unconditional `EventBlock` gain (food,
resources, science, culture) — none of which cost anything — with no
mention of a food payment. **Found by replaying real BGO game `7522616`**:
Purple prepares Development of Settlement, then LATER the same turn pays an
explicit, separately-logged `"Purple increases population Purple spends 3
food"` for their own real Pop action. Reconstructing the turn proves the
event grant had to be free: entering the turn at `yellow_bank == 17`, a
FREE grant moves it to 16 (`pop_cost_base(16) == 3`, exactly the food the
human paid for their OWN LATER Pop); the old PAID code additionally spent
`pop_cost_base(17) == 2` food on the event grant itself, leaving only 1 food
when the human's real Pop needed 3 — an `IllegalMove` this binary reported
as the "budget shortfall" mystery, for at least this one game. **Fixed** by
renaming `paid_increase_population` to `free_increase_population` and
switching it to `economy::increase_population`'s established free-grant
shape (`cost: 0, consume_one_time: false` — the same shape `apply.rs::
h_pop_free`/Ocean Liners already use). **Test**:
`events::tests::an_event_granted_population_increase_costs_no_food`
(`rust/src/events.rs`) — confirmed to fail before the fix (food dropped
3 → 1, the exact BGO-observed shortfall) and pass after.

**1b. ENGINE BUG (confirmed, fixed, tested): Development of Civil Life's
ordered action was never wired to the "ordered free action" CA exemption
every other card of its shape gets.** `Development of Civil Life`
("Development of Civilization" in BGO's UI) reads "Immediately, each
civilization may either: increase its population; or build a farm, mine or
urban building; or develop a technology. It costs 1 [resource] less than
usual." — textually near-identical to Rich Land's "Build or upgrade a farm
or mine; pay 1 less resource" and Frugality's own `freeCivilAction` text,
both of which rule item 11 already governs: "if it orders an action,
perform it under normal rules but paying no civil ... action for it." Rich
Land/Urban Growth/Frugality are wired to that exemption
(`Special::FreeCivilAction`, `apply::apply_free_civil_move`, called with
`free: true`) — but Civil Life's identical grant, banked in `PlayerState::
one_time_discount` (`build_resources`/`develop_science`/`pop_food`), was
only ever read for its RESOURCE discount, never for the CA exemption: the
normal `Move::Pop`/`Move::Build`/`Move::Develop` dispatch (`apply.rs::
apply`) always calls `h_pop`/`do_build`/`h_develop` with `free: false`, and
`legal.rs::action_moves` gated Pop/non-unit-Build/Develop on `ca >= 1` with
no exception. **Found by replaying real BGO game `7523355`**: Purple's
round-3 turn (Despotism, 4 CA) spends 1 CA on their own Civil-Life-discounted
build, 1 on a wonder stage, 2 on a `Take` — a correctly-priced 4 CA total —
then a SECOND, real `Take` fails with `civil_actions == 0`: 5 CA-costing
actions on a 4-CA budget, and the one action that shouldn't have cost
anything was the Civil-Life build. **Fixed**, three sites, mirroring the
existing pattern exactly (`costs::civil_life_ca_free`, a one-line documented
helper, is the single source of truth all of them read):
- `legal.rs::action_moves` — Pop, non-unit-Build, and both Develop gates now
  read `ca >= 1 || civil_life_ca_free(p.one_time_discount.<field>)`. NOT
  applied to Upgrade or Destroy — Civil Life's text does not cover them.
- `apply.rs::h_pop`/`do_build`/`h_develop` — each now reads its own
  `one_time_discount` field BEFORE the existing code zeroes it (consuming
  the grant), and skips `costs::pay_ca`/`military_actions -= 1` when it was
  set, exactly mirroring `free`'s existing effect from a DIFFERENT free
  source (an action card) — the two stay independent, since a card in hand
  and a banked event grant can both be live at once.

  **Tests**: `apply::tests::
  do_build_spends_no_civil_action_when_civil_life_banked_a_build_discount`
  and `legal::tests::
  build_move_is_offered_with_zero_civil_actions_when_civil_life_banked_a_discount`
  (both confirmed to fail before the fix — the `apply.rs` one via a genuine
  `debug_assert` panic, "paid more civil actions than available" — and pass
  after).

**Verified against real data, not just unit tests**: re-running the 24-game
sample after 1a+1b, `7523355`/`7523354`/`7523350`/`7522616` (all feature
Development of Civil Life, all previously stopped on the exact
`civil_actions == 0` shortfall shape) each ran dramatically deeper (e.g.
`7523355`: 27 → 62 actions before its NEXT, unrelated stop) — while
`7523087` (no Civil Life event anywhere in that game's journal) stayed
EXACTLY at its previous stop, unaffected, the clean negative control this
fix predicts.

**What is still open, honestly**: **2/24 games (`7522632`, `7523087`)
still show the exact `civil_actions == 0`, everything-blocked shape**, and
neither is explained by 1a, 1b, Hammurabi's MA-as-CA conversion, or any
`CardEffects`/`Special` on a card either player has. `7522632`'s Civil Life
event does not even fire until round 5 — three rounds AFTER this game's
round-4 shortfall — so it is provably unrelated for that game specifically.
`7523087` (Purple electing Michelangelo mid-turn, replacing Hammurabi) was
already individually verified in an earlier pass: every civil-costing
action that turn is independently correct, and the leader-replacement
refund nets to zero exactly as §9.1 predicts. **A genuinely unidentified
THIRD source, or something else — not diagnosed, not guessed at.**
`REPLAY_DEBUG=1`/`REPLAY_DEBUG_ALL=1` reproduce the full trace for both.

### Finding 2 (interleaving with no `EndTurn`, was 9/24): root-caused to Development of Civil Life's OUT-OF-TURN grant — a replayer/turn-model gap, not an engine rules bug, fixed in `replay.rs`

**Root cause, confirmed against real games, not the `politics_done`-vs-
global-`phase` theory this doc previously named as the most concrete lead**
(that theory was checked and does not explain the dominant shape — see
below). Development of Civil Life's grant (immediately above) is NOT scoped
to whoever prepared it, or to their own turn — it is banked on `p.
one_time_discount` for EVERY qualifying player the instant the event
resolves, and a real BGO player may spend their own banked grant WHENEVER
they like, including mid-ANOTHER-player's live turn, since the grant itself
carries no timing restriction once banked. **Every one of the 8 sample
games examined in detail that stopped on `decider != expected actor`
contains this event** (`7523354`, `7523355`, `7523809`, `7523350`,
`7522619`, `7523082`, `7523357`, `7522668`) — not a coincidence: e.g.
`7523355` line 34-35 —

```text
Purple builds Philosophy Purple spends 2 resources          <- Purple's own turn
Orange builds Philosophy Orange spends 2 resources          <- interjects, no EndTurn for Purple
Purple builds 1 stage of Library of Alexandria ...           <- Purple's turn resumes
```

— and the SAME shape covers BGO's `"<Color> discovers <Card> <Color> loses
N science"` phrasing (`corpus.rs`'s `"discovers "` prefix — `develop`, not
`build`), the dominant remaining sub-case once build/pop were handled: e.g.
`7523809` line 55, `"Orange discovers Alchemy Orange loses 3 science"` mid
Purple's turn, Alchemy having been taken into Orange's hand normally,
turns earlier.

**Why this is a replayer/turn-model gap, not an engine rules bug**:
`legal::legal_moves` (correctly, matching the real engine's design) only
ever computes ACTIONS for `state.current`/`state.decider()` — it has no
concept of "yes, but not your turn," because the base game has almost no
mechanic that needs one (every other action-phase move genuinely does
belong to whoever's turn it is). Development of Civil Life is the one
exception, and self-play never needs to reproduce an out-of-turn grant
faithfully (a bot with a banked, un-timed discount simply spends it on its
own next qualifying action, which is a safe, non-exploitable
simplification, not a strategy-relevant divergence) — the gap only shows up
when trying to replay a REAL human's exact, out-of-turn action sequence.

**Fixed in `replay.rs` only, no engine turn-model changes**: `civil_life_
move` (new helper) recognizes, for the journal's stated actor, whether they
have a live `one_time_discount` matching an `IncreasePopulation`/
`BuildBuilding`/`DevelopTechnology` line; if so, the main per-line loop
applies it directly via `apply::apply_free_civil_move` (bumped from
`pub(crate)` to `pub` — already actor-explicit and `state.current`-agnostic
by construction, since it existed for a DIFFERENT free-civil source, an
action card's ordered move) BEFORE `resolve_intervening`/`apply_one`, which
both assume the acting player IS the decider. Deliberately does NOT cover
`ActionClass::Develop` when the card is not already in the interjecting
player's grounded hand (`p.hand_civil.contains`) — an honest, narrower
residual left unhandled rather than guessed at.

**Verified against real data**: re-running the 24-game sample, `decider !=
expected actor` dropped from 9/24 to 1/24. The one remaining case
(`7523791`, line 89, `"Grey builds Religion"`) is a DIFFERENT, narrower bug
in the pre-existing `Development of Religion` (`ChoiceKind::FreeBuild`)
per-player draining order — not Civil Life, not diagnosed further this
pass (a single occurrence).

**The `politics_done`-vs-global-`phase` theory this doc previously flagged
as the most concrete lead was checked and does NOT explain the dominant
shape**: in every game examined, `resolve_intervening` reaches the
interjecting line with `state.pending` genuinely empty and `state.
decider()` genuinely still naming the ORIGINAL turn's player — there is no
live per-player political-decision signal being missed, because nothing
about this is a political decision at all. That theory may still explain
some OTHER, rarer interleaving shape (untested — no sample game needed it
once Civil Life's cases were excluded), but it is not the reason this
sample's stops happened. Documented here so nobody re-chases it as the
primary lead.

### Updated sample numbers after this pass

| | third pass | fourth pass (this one) |
|---|---|---|
| games complete | 0/24 | 0/24 |
| mean actions before stop | 51.0 | **63.7 (+25%)** |
| `decider != expected actor` (interleaving) | 9/24 | **1/24** |
| civil-action-budget shortfall (`civil_actions == 0`, unexplained) | 8/24 | **2/24** |
| build cost mismatch (unmodeled discount source) | 1/24 | 8/24 (pre-existing category, `docs/REPLAY.md`'s "gives up on" shape from the first pass — more visible now that games run deeper, not new; NOT investigated this pass) |
| `WonderStep` illegal (two different unresolved shapes) | 1/24 | 5/24 (newly reached this deep; NOT diagnosed this pass — likely a wonder-completion `Pending::Choice` and/or a blue-token/multi-stage cost gap, out of this pass's scope) |
| `FreeCivilAction` no-`Pending::Choice`-opened (Urban Growth) | ~2/24 | 2/24 (unchanged, pre-existing, still not investigated) |
| other, single-occurrence, not diagnosed | 3/24 | 3/24 (`PolPass` illegal, `PlayAction` illegal, a `Take` with many other CA-costing options still legal — each a single occurrence this pass did not chase) |

Every game reached the same or a strictly later stop point than the third
pass; none regressed. As with every prior pass, closing two walls exposed
the next ones (`WonderStep`/build-cost-mismatch categories, both
pre-existing shapes that simply could not fire before games ran this deep)
— expected, not new information, and reported honestly rather than
claimed as newly discovered bugs.

### What remains open going into a fifth pass

1. **The residual 2/24 civil-action-budget shortfall** (`7522632`,
   `7523087`) — confirmed NOT Civil Life, NOT Hammurabi, NOT any known
   `CardEffects`/`Special`. The single highest-value remaining lead: a
   THIRD unidentified action-economy source, reproducible with
   `REPLAY_DEBUG_ALL=1` on either game.
2. **The `build cost mismatch` category (8/24)**: an unmodeled discount
   source, echoing the first pass's Rich Land/Urban Growth discovery and
   this pass's own Finding 1b — worth checking whether it is ALSO a Civil
   Life-shaped gap (a build discount this binary isn't crediting) before
   assuming it is something new. Representative lines available for all 8
   games via this pass's own re-run.
3. **The two `WonderStep`-illegal shapes (5/24)**, one blocked by a live
   2-option `Pending::Choice`, one with many other CA-costing moves still
   legal — not investigated, not even loosely diagnosed, this pass.
4. **The single residual `decider != expected actor` case (`7523791`)** —
   a narrower bug in `Development of Religion`'s existing `FreeBuild`
   per-player draining order, not Civil Life.
5. Scaling the sample past 24 games remains blocked on the same scoping
   question the third pass raised: 0/24 complete, and a full BGO game gives
   every category above many chances to fire over 15-20+ rounds.

## Fifth pass: the "build cost mismatch" category fully closed (two real fixes), the two named residuals investigated further but still open, one new well-evidenced lead found

This pass's job was the fourth pass's open-items list, in priority order. Two
were closed outright (both confirmed causes, both fixed and tested); two
(the budget-shortfall residuals, the `Development of Religion` interleave)
were investigated in real depth but are reported honestly as STILL open,
with new evidence attached rather than a forced fix. **0/24 still complete**
(as every prior pass), but mean actions before a stop rose from 63.7 to
**73.5 (+15%)**, and the `WonderStep`/`build cost mismatch` blocker
categories the fourth pass left unresolved are gone -- replaced by new,
deeper stops (expected, the same "closing one wall exposes the next"
pattern every prior pass reports).

### Finding A (ENGINE BUG, confirmed, fixed, tested): Development of Civil Life's discount was modeled as three independent grants; the real card is ONE mutually-exclusive choice

This is a correction of a bug the fourth pass itself introduced while fixing
a different bug. The card ("Development of Civil Life", "Development of
Civilization" in BGO's UI) reads "Players may **either** increase its
population; **or** build...; **or** develop a technology... 1 [resource]
less" -- an either/or/or list, one choice among three, not a grant of three
independent one-time discounts. `state.rs`'s prior doc comment (added
2026-08-05) read the JSON schema's three sub-keys as license for three
independent grants, each cleared only when its own field was spent -- a
plausible-looking misreading that this pass found and reverted.

**Confirmed wrong by replaying three separate real BGO games**, each
showing the same shape: a human spends ONE of the three discounts (pop,
build, or develop) and later, the same turn or a later one, pays FULL,
undiscounted price for an action of a DIFFERENT type that the old
independent-grants model predicted should still be discounted --

- `7523357`: Grey spends the `pop_food` discount (`"increases population
  ... spends 1 food"`, not the turn's usual 2), then later pays full price
  (2, not 1) for a Bronze mine build the old model said should still be
  discounted.
- `7523350`: Orange spends `develop_science` on Printing Press (2 science,
  not the printed 3), then pays full price for a later Bronze build.
- `7522619`: Green spends `pop_food` (1 food, not 2), then pays full price
  for a later Religion build (3 resources, not the discounted 2).

**Fixed** via `OneTimeDiscount::exhaust()` (`state.rs`), called from all
three consumption sites (`economy::increase_population`,
`apply::do_build`, `apply::h_develop`) whenever THAT site's own field was
actually live -- clearing all three fields together, not just one, and
specifically NOT clearing anything when the acting site's own field was
already 0 (so an ordinary, unrelated action never wipes a sibling grant
this player hasn't spent yet). **Tests** (both confirmed to fail against the
reverted old behaviour and pass after, by temporarily restoring the old
independent-clearing code, rerunning, and reverting back):
`economy::tests::increase_population_exhausts_the_whole_civil_life_grant_not_just_pop_food`,
`apply::tests::spending_any_one_civil_life_discount_exhausts_the_whole_grant`
(this one REPLACES a test that asserted the old, wrong behaviour by name --
`one_time_discount_categories_are_consumed_independently` no longer exists).

**Sibling grep**: `oneTimeDiscount`/`gainFoodOrResources`-shaped "list of
alternatives, one choice" card text is rare -- grepped `card_table.rs` and
`data/*.json` for every other "either/or" multi-field one-time grant;
Development of Civil Life is the ONLY base-game card with an
`oneTimeDiscount` payload at all, so there is no sibling site sharing this
exact shape to fix. (A structurally adjacent but semantically DIFFERENT
case, `ChoiceKind::FoodOrRes`, is Finding C below -- a replayer gap, not
this bug.)

### Finding B (`replay.rs`, not engine): a unit build/upgrade's `p.mil_discount`-funded portion is a SEPARATE journal clause, never summed into the parsed cost

Closes the other half of the fourth pass's 8/24 "build cost mismatch"
category (`7523341`, `7522668`, `7522616` -- the three where the engine's
computed cost was HIGHER than what the journal showed paid, the opposite
direction from Finding A's cases). BGO logs a unit build/upgrade's total
resource payment as up to two clauses: `"loses N military resource"` (the
portion covered by `p.mil_discount`, the real per-turn pool Patriotism /
Wave of Nationalism / Military Build-Up grant, `costs::spend_mil_discount`)
and `"spends N resource"` (the rest, from the ordinary pool) -- e.g. `"Purple
builds Warrior Purple loses 1 military resource; Purple spends 1
resource"`. `replay.rs`'s build-cost cross-check was reading `spends` alone,
silently under-counting the total by exactly the `"loses"` amount (which
`costs::build_cost_for` never subtracts for units on purpose -- Civil
Life's `build_resources` discount is farm/mine/urban only, and `mil_discount`
is netted off at APPLY time, not at this check's comparison baseline).
Confirmed by exact arithmetic across every sampled instance: `loses` +
`spends` always equals `build_cost_for`'s raw printed cost, including lines
with NO Patriotism-style grant visible anywhere earlier in that player's
turn (an unexplained baseline case -- see "What remains open," below).

**Fixed** by `total_paid_for_build` (`lost_military_resource` +
`spent_resources`, summed). **Tests**:
`lost_military_resource_reads_a_loses_military_resource_clause`,
`lost_military_resource_ignores_an_unrelated_loses_clause`,
`total_paid_for_build_sums_the_military_resource_and_spends_clauses`
(`rust/src/bin/replay.rs`).

**Together, Findings A and B closed all 8/24 of the fourth pass's "build
cost mismatch" games** -- 5 were Finding A (Bronze/Bronze/Philosophy/
Religion/Religion), 3 were Finding B (Warriors/Swordsmen/Warriors).

### Finding C (`replay.rs`, not engine): Reserves' `FoodOrRes` choice was never resolved -- the exact GainBlock bug shape, missed at the one OTHER site it applies

The fourth pass fixed `ChoiceKind::GainBlock` (an event's "gain 2 resources
or 2 food, player's choice") going unresolved and blocking a later action.
`ChoiceKind::FoodOrRes` is the SAME journal shape -- a standalone-looking
`"produces N food"` / `"produces N resources"` bookkeeping pick -- but is
opened by a DIFFERENT mechanic (Reserves, `Special::GainFoodOrResources`, an
action card's own ordered gain, §3.11) and was never covered. Grepped
`card_table.rs` for every `GainFoodOrResources` site to confirm Reserves is
the ONLY base-game card with this shape (never an event, so never opened
for "every qualifying player" the way GainBlock is -- always scoped to the
one player who played it). **Found by replaying `7523818`**: an unresolved
`FoodOrRes` pending blocked a `WonderStep` several lines later.

This did NOT get folded into the existing `GainBlock` drain code, because
the journal shape is subtly different in a way that matters: GainBlock's
pick is always its OWN standalone journal ROW (`"<Color> produces N
food"`, nothing else on the line); Reserves' pick is glued onto the SAME
row as the `"plays Reserves"` line with no separating punctuation
(`"Orange plays Reserves Orange produces 2 resources"` -- confirmed
against the FULL corpus: 4157 of 4158 `"plays Reserves"` lines have this
exact glued shape). **Fixed** in `ActionClass::PlayActionCard`'s own
handler (which has the raw line text `GainBlock`'s prescan-based drain
doesn't need), via a new `trailing_produces` helper that reads the LAST
`"produces"` clause anywhere in the line. **Tests**:
`trailing_produces_reads_a_produces_clause_glued_onto_a_play_line`,
`trailing_produces_is_none_with_no_produces_clause_at_all`.

### The two residual budget-shortfall games (`7522632`, `7523087`): still open, but a new, corpus-wide lead found

Re-examined both with `REPLAY_DEBUG_ALL=1` from scratch (not assuming the
fourth pass's diagnosis). Confirmed, again, NOT Civil Life (in `7522632`
the event doesn't even fire until 3 rounds after the shortfall), NOT
Hammurabi's MA-as-CA conversion (the leader is gone by the time of the
shortfall in both games), NOT Michelangelo's wonder-surcharge waiver (it's
individually verified correct in `7523087` -- Colossus's own take costs
exactly `row_cost`, matching its printed clause). Every individual
CA-costing action's OWN printed cost matches this engine's own formula; the
SUM simply exceeds the government's printed budget by exactly 1 in both
games.

**A new, well-evidenced lead**: in `7522632`, the specific failing action
is `"Orange takes Taj Mahal in hand"` -- with **no `"uses N civil action"`
clause at all**, unlike the overwhelming majority of civil-row takes.
Widening the search to the full corpus (all 1,011 games, not just the
sample) turns this from a one-off oddity into a real, structured signal:

| card | total takes | no CA clause | rate |
|---|---|---|---|
| Taj Mahal | 317 | 150 | 47% |
| Leonardo Da Vinci | 700 | 123 | 18% |
| Michelangelo | 414 | 57 | 14% |
| Great Wall | 318 | 2 | 0.6% |
| Colossus | 274 | 7 | 2.6% |
| Hanging Gardens | 511 | 2 | 0.4% |
| Pyramids | 820 | 3 | 0.4% |
| Hammurabi | 716 | 0 | 0% |

This is not noise: three specific cards (two Leaders, one Wonder) show the
missing clause 14-47% of the time; every other sampled Leader/Wonder shows
it under 3% of the time, close to zero. `7523347` (this SAME sample's
`Take{Taj Mahal}` slot-specific mismatch, distinct from the two named
budget-shortfall games) hits the identical `"takes Taj Mahal in hand"`
no-clause line, a THIRD independent occurrence. When a clause IS present
for Taj Mahal, the cost varies (1/2/3/4 CA observed), ruling out "Taj Mahal
has a fixed printed take cost BGO sometimes omits." **Not diagnosed**:
several candidate explanations were considered and none confirmed --
Michelangelo being in play does NOT explain it alone (one of the five
sampled no-clause instances has no leader-election anywhere nearby in the
same turn); it is not simply "first action of the turn" (checked against
counter-examples on both sides). Flagged here, with the full cross-card
frequency table, as the single most promising remaining lead for the two
residual budget-shortfall games specifically -- **whatever this pattern
turns out to mean, it directly overlaps both `7522632`'s and `7523347`'s
proximate failing lines**, though `7523087`'s own failure (a Colossus take
WITH a normal, correctly-priced clause) shows it cannot be the WHOLE
explanation for that game. Per the standing rule, NOT guessed at or
force-fixed.

### The `Development of Religion` interleave (`7523791`): confirmed to be a FOUR-PLAYER interleave, not a two-player one -- root cause still open

The fourth pass's framing ("a narrower bug in the FreeBuild per-player
draining order") undersold the shape: the real journal shows FOUR
consecutive `"<Color> builds Religion"` lines from four DIFFERENT players
(Green, Grey, Orange, Purple in that order) with **no `EndTurn` anywhere
between any of them** -- every qualifying player spending their own
`Development of Religion`-granted `FreeBuild` the instant the event
resolves, not in turn order and not gated on whose turn it nominally is
(same "banked, spendable whenever" shape Finding 2 of the fourth pass
established for Development of Civil Life, but for a DIFFERENT event/
mechanic). `resolve_intervening`'s existing `ChoiceKind::FreeBuild` handling
already drains regardless of whose turn it is, matching by whether the
UPCOMING line's card is among the pending's own options -- and the first
three players' builds (Green, Grey, Orange) resolve fine; only the FOURTH
(Purple) is where `state.decider()` stops naming the actor the journal
names next. Not root-caused this pass: the leading candidate (the
FreeBuild queue's internal order not matching the journal's per-player
resolution order once more than 2-3 players are queued at once) was not
confirmed against the actual `Pending` stack ordering in time. Flagged
open, not guessed at.

### Newly reached, not diagnosed this pass (expected -- closing walls exposes new ones)

Every one of these is a stop point only reachable now that Findings A/B/C
let games run substantially deeper; none was reachable before this pass on
this sample. Each is a single or double occurrence, too thin to responsibly
diagnose in the time this pass had left:

- **Colonize bidding** (`7523818`, newly reached): `Bid { n: 3 }` illegal,
  only `BidPass` offered -- likely related to the pre-existing "colonize
  sacrifice specifics" approximation this file already documents as
  unexercised against real data until now.
- **Aggression defense with a committed defense card** (`7523355`):
  exactly the ALREADY-DOCUMENTED "gives up on" case (BGO logs only a
  count, never identities) -- genuinely unrecoverable, not a bug.
- **`Rich Land` building Iron, `ParserGap`** (`7523354`): `free_civil_build_move`
  does not recognize an Iron build/upgrade as one of Rich Land's offered
  options.
- **`Upgrade` blocked by a live 2-3 option `Pending::Choice`** (`7522668`,
  `7522652`): same general SHAPE as Finding C (a stale, un-drained pending
  blocking a later action) but for `Upgrade` specifically -- not confirmed
  to be the same root cause.
- **`Pop` blocked by a live 5-option `Pending::Choice`** (`7522619`): same
  shape again, not confirmed.
- **`decider != expected actor`** (`7523350`, newly reached): a fresh
  instance in a DIFFERENT game, immediately after a Civil-Life-discounted
  `Develop`; not confirmed to be the same root cause as `7523791`'s.

### Updated sample numbers after this pass

| | fourth pass | fifth pass (this one) |
|---|---|---|
| games complete | 0/24 | 0/24 |
| mean actions before stop | 63.7 | **73.5 (+15%)** |
| `build cost mismatch` (unmodeled discount) | 8/24 | **0/24 (closed)** |
| `WonderStep` illegal (2 shapes) | 5/24 | **0/24 as originally shaped** (Finding C fixed the 3 Choice-blocked cases; the 2 no-option cases are unchanged, still open, see fourth pass) |
| civil-action-budget shortfall, unexplained | 2/24 | 2/24 (same two games, new lead found, not resolved) |
| `decider != expected actor` (interleaving) | 1/24 | 2/24 (`7523791` root-caused further but not fixed; `7523350` newly reached) |

Every game reached the same or a strictly later stop point than the fourth
pass; none regressed.

### Final-score cross-check: still not reached

`game::scores(&state)` vs `index.tsv`'s `results` column still could not be
run -- 0/24 games in this sample reached `state.game_over`. Unchanged from
every prior pass's own note on this.

### What remains open going into a sixth pass

1. **The two residual budget-shortfall games** (`7522632`, `7523087`) --
   see the Taj Mahal/Leonardo Da Vinci/Michelangelo "missing CA clause"
   lead above; the single highest-value unstarted thread (a corpus-wide,
   not sample-local, pattern spanning hundreds of occurrences).
2. **The `Development of Religion` four-player interleave** (`7523791`) --
   root-caused to a `FreeBuild` queue/drain-order question but not fixed.
3. **The `Rich Land` building Iron `ParserGap`** (`7523354`) and the two
   remaining no-option `WonderStep` shapes (`7523809`, `7523353`) -- single
   occurrences, not investigated.
4. **The new `Upgrade`/`Pop` Choice-blocked shapes** (`7522668`, `7522652`,
   `7522619`) -- worth checking whether they share Finding C's root shape
   (an un-drained `Pending::Choice` from an action card's own ordered gain)
   before assuming they are something new.
5. Colonize bidding (`7523818`) and the colonize-approximation's own
   accuracy remain entirely unexercised against real bid amounts.
6. Scaling the sample past 24 games remains blocked on the same scoping
   question the third pass raised.

