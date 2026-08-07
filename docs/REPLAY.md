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
- ~~**Aggression defense** with any committed defense cards: BGO logs only a
  count, never which.~~ **This was false — see "Tenth pass" below.** BGO's
  `"<Color> defends ..."` line is not a bare count; it is one clause PER
  committed card, and every clause fully identifies its card (a printed
  `+2`/`+4`/`+6` bonus number for the age I/II/III bonus cards, one card per
  value; any other hand card for a plain "military card played", and which
  one is provably irrelevant to any observable outcome). Fixed in the tenth
  pass, `resolve_aggression_defense`/`parse_defense_clauses` in
  `replay_common.rs`.
- **`PutBack`** (a human undoing their own `Take` via BGO's client-side
  undo): there is no `Move` for this in the engine at all — `moves.rs`'s
  variant list has no "untake". **Fixed in this pass** for the common
  shapes (nested take/take-back stacks, and takes interrupted by an
  unrelated committed action before their own take-back) — see "Bugs found
  and fixed." A take-back with NO matching preceding take by the same actor
  anywhere in the game (a genuine parser gap, or a BGO logging artifact)
  would still stop the game; none of the 24 sampled games hit this residual
  case in this pass.
- **Colonization sacrifice specifics** used to be listed here as an
  unimplemented approximation ("the binary auto-drains colonization by
  picking the engine's own first-offered option at each step until the
  force clears"). **No longer true** — see the fourteenth pass below. The
  `"Sacrificed Units:; ..."` list is one clause per committed piece and is
  now applied as real `SendUnit`/`SendBonus`/`SendDiscard` moves; only
  James Cook's `"1 Military card +1"` clause leaves its card unnamed.
  `Replayer::approximate_colonize` survives as the fallback for the ~2% the
  journal's own list cannot be applied to, and still sets
  `colonize_approximated`.

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

## Sixth pass: the civil-action-budget bucket, root-caused to a THIRD engine bug -- and the wonder-surcharge rules question settled against the corpus

This pass had one job: the largest single stop bucket, `IllegalMove: Take`
at **143 of 1,011 games** (`rust/src/bin/replaystats.rs`, the ranked
histogram every number below comes from). The whole bucket was reported as
a civil-action budget shortfall -- the replayer believing the human had no
civil actions left while the journal shows them taking a card anyway.

| | fifth pass | sixth pass (this one) |
|---|---|---|
| mean rounds reached (of 19.27 played) | 5.41 | **5.72 after the engine fix, 5.71 after the replayer fix** |
| decisions recorded in Age II or later | 2.4% | **3.5%** |
| `IllegalMove: Take` | 143 | **17** |
| `ParserGap: Taj Mahal's take cost ...` | 8 | 81 (see below -- this GREW on purpose) |

### Finding 1 (ENGINE BUG, confirmed, fixed, tested): Hammurabi's MA-as-CA conversion was forfeited by replacing him mid-turn

`costs::pay_ca` spends Hammurabi's once-per-turn conversion **lazily** --
it only reaches for a military action once the printed civil-action pool
runs dry -- and both it and `costs::spare_ca` gated the conversion on
`p.leader` still BEING Hammurabi. A player who spent their last printed
civil action ON the leader replacement therefore lost the conversion
before ever being offered it.

The rulebook is explicit that this is legal (RB, "Replacing a Leader",
transcribed in `sources/ubg_the-second-round.txt`): **"You are allowed to
use the benefit of a leader and then replace him or her on the same
turn."** Hammurabi's own 2015 text is "On your turn, you may use one
military action as one civil action" (`sources/namu_heroes.txt`,
`data/cards_wonders_leaders.json`).

**The corpus evidence, which is what found it.** Of the 143 games in the
bucket, 109 stopped **short by exactly one civil action** against a cost
the journal itself printed -- and **103 of those 109 are turns in which
the human replaced Hammurabi**, against Hammurabi being only 39%
(577/1495) of all Age A leader replacements corpus-wide. 107 of the 109
still had a military action in hand to convert. A leader-agnostic cause
(e.g. the replacement refund being mis-netted, the standing alternative
hypothesis) predicts 39%, not 94.5%.

**Fixed** with `PlayerState::hammurabi_replaced_this_turn`, set in
`apply::h_play_leader` when the leader being replaced is Hammurabi and
cleared alongside `hammurabi_used` in `economy::end_of_turn`;
`costs::hammurabi_conversion_available` is now the single source of truth
both `spare_ca` and `pay_ca` read. `take_gate`'s SEPARATE
`leaderTakeCivilActionDiscount` deliberately still keys off the live
leader -- that one is a continuous in-play effect, not a once-per-turn
use. **Test**: `apply::tests::replacing_hammurabi_mid_turn_keeps_his_
military_action_as_civil_action_conversion_for_the_rest_of_the_turn`,
confirmed to fail with the `h_play_leader` flag reverted (`left: 1,
right: 2`) and pass after.

This one fix took the bucket from 143 to 57 games, mean rounds from 5.41
to 5.72, and the Age II+ decision share from 2.4% to 3.5%.

### The wonder take-surcharge: SETTLED. The rule is exactly as modeled -- a wonder completed EARLIER THE SAME TURN still counts

The fifth pass left this open on two contradicting games, with `7523353`
suggesting the `+1 CA per already-completed wonder` surcharge (§2.4)
might not apply to a wonder completed earlier in the same turn. Two data
points are not a ruling, so this pass measured the whole population:
**every wonder take in all 1,011 journals that carries an explicit cost
clause, 6,757 of them**, excluding takes made with Michelangelo in play
(his printed text waives the surcharge). Summing BOTH cost clauses on the
line (`"uses N civil action; ... uses N military action"` -- a Hammurabi
conversion splits a take's cost across two clauses, and reading only the
civil one manufactures fake violations) and subtracting each player's
completed-wonder count leaves the implied ROW POSITION cost, which the
rules confine to 1, 2 or 3:

| implied position cost | Model A: every completed wonder counts (current engine) | Model B: wonders completed earlier the same turn are exempt |
|---|---|---|
| -1 (impossible) | 10 | 5 |
| 0 (impossible) | 21 | 22 |
| 1 | 4758 | 4421 |
| 2 | 1604 | 1885 |
| 3 | 364 | 415 |
| 4 (impossible) | 0 | **9** |
| **violations** | **31 (0.46%)** | 36 (0.53%) |

Model A -- what the engine already does -- is consistent with **6,726 of
6,757** real human wonder takes. Model B is not merely no better, it is
**affirmatively refuted**: it implies 9 takes cost more civil actions
than the most expensive row position exists to charge. So the surcharge
counts a wonder completed earlier the same turn, `costs::take_cost` is
right, and **nothing was changed here**.

And the residual is not about the surcharge at all: **all 31 of Model A's
violations are Taj Mahal**, and 22 of the 31 involve no same-turn
completion whatsoever, so the same-turn theory would not have explained
them either. Every other wonder in the game -- Pyramids, Hanging Gardens,
Colossus, Library of Alexandria, Great Wall, St. Peter's Basilica,
Universitas Carolina, and all the Age II-IV wonders -- is 100% consistent
across all 6,726 of its takes.

### Finding 2 (replayer, not engine): a take line with NO cost clause cost ZERO actions -- it is not an unknown cost

`ground_row_slot` took `Option<i32>` for the journal's stated cost and
read a missing `"uses N ... action"` clause as "unknown", falling through
to its "first ungrounded slot" path -- a silent guess at which slot the
human paid for, the exact shape the fifth pass removed for the
known-cost case.

No clause means zero, and there is a printed card ability that produces
zero. Of the corpus's 88,432 take lines, **483 carry no clause at all**,
and **1,639 of the 1,641 no-civil-clause lines fall in game-age I** --
Hammurabi's window. 333 of the 483 are LEADER takes and **every single
one of the 333 has Hammurabi in play**: his
`leaderTakeCivilActionDiscount` cancels the 1 CA of a leader sitting in
one of the row's five cheapest slots. The engine already priced this
correctly; only the replayer could not read it. **Test**:
`replay_common::tests::a_take_line_with_no_uses_clause_at_all_cost_zero_
actions_not_an_unknown_cost`.

Mean rounds and the Age II+ share are FLAT across this change (5.72 ->
5.71, 3.5% -> 3.5%) and that is the honest report: it trades a lucky
guess for a truthful stop rather than buying depth. What it does buy is
attribution -- `IllegalMove: Take` drops 57 -> 17, and the Taj Mahal
anomaly stops hiding inside mis-grounded row slots.

### Open, with a documented population: Taj Mahal is priced below anything the rules allow, and only Taj Mahal

The single biggest remaining thread out of this bucket, and the reason
the `ParserGap: Taj Mahal` bucket deliberately grew from 8 games to 81.
**181 of the corpus's 317 Taj Mahal takes (57%) cost less than the
cheapest row position plus this player's own wonder surcharge**:

- **150 with no cost clause at all**, i.e. 0 civil actions -- impossible
  for a wonder, whose cheapest possible take is 1. Reconstructing the CA
  ledger by hand for four of them (`7521302` r4, `7521361` r5, `7521377`
  r4, `7523353` r5) confirms the action really was free: in each, every
  OTHER civil-costing action that turn already exactly consumes the
  government's printed budget plus every identified bonus, with nothing
  left for the Taj Mahal.
- **31 more with an explicit clause below the minimum**, the Model A
  violations above.

The one strong signal found: **149 of the 150 clause-less Taj Mahal takes
happen in a turn in which that player elected a leader**, usually on the
line immediately before. Candidate explanations checked and rejected: it
is not Michelangelo's surcharge waiver (only 49 of the 150 have him, and
his waiver cannot take a cost below 1 anyway); it is not Hammurabi's
leader-take discount (Taj Mahal is a wonder, not a leader, and these
players' leaders are Age I ones); it is not BGO omitting the surcharge
generally (takes costing 4+ CA occur, and no row position charges 4); and
it is not a general clause-omission bug, since 6,726 other wonder takes
price perfectly. **Not diagnosed, not guessed at.** The next pass should
start here: whatever this is, it is card-specific, it is worth 81 games,
and it is the only thing left in the take bucket that is not already
attributed.

### Also in this bucket, but NOT budget shortfalls (re-triaged, left alone)

15 of the original 143 turned out not to be action-economy problems at
all: the acting player had ample civil actions (4-6, against an observed
cost of 1-3) and `legal_moves` offered no `Take` because the whole move
list was a live `Pending` -- a colonize `Bid { n }` list, or an
un-drained 2-option `Choose`. Those belong to the colonize-bidding and
stale-pending threads other passes own, not to this one, and they are
**exactly the 17 games left in the bucket** (15 of the 17 are that
identified set; `7522905` and `7523281` were newly reached and not
triaged). In other words the budget-shortfall shape this bucket was
named for is now fully accounted for: fixed (Finding 1), or moved into
the Taj Mahal `ParserGap` bucket above, or pushed deeper into other
categories. Reproducible with `REPLAY_DEBUG=1`.

## Seventh pass: the Taj Mahal anomaly, SETTLED -- it is a printed card ability the engine never had

The sixth pass closed its section with the only thing left in the take
bucket that was not attributed: **181 of Taj Mahal's 317 corpus takes (57%)
cost less than the cheapest row position plus that player's own wonder
surcharge allows**, 150 of them free outright, with the one strong signal
being that 149 of those 150 fall in a turn where the player elected a
leader. It is not a replayer bug, not a BGO quirk, and not the surcharge.

### The mechanism: Taj Mahal's own 2015 card text

> **"If you replaced your leader this turn, taking this wonder costs you 2
> civil actions less."**

That clause is printed on the card in *A New Story of Civilization* and was
missing from `data/cards_wonders_leaders.json` entirely (the entry carried
only "+3 culture production, +1 blue token", which is the whole of the 2006
card). Two independent sources already in `sources/`, neither of them this
corpus:

- **`sources/bga_throughtheages_material.inc.php`, card id 98** -- Board
  Game Arena's own implementation of the 2015 edition. The sentence above is
  its `text` field verbatim, alongside `'culture' => 3` and `'tokendelta' =>
  array('blue' => 1)`.
- **`sources/namu_wonders.txt`** -- the Korean wiki's new-edition entry:
  "(New Edition) Score increase rate +3, blue tokens +1. If you switched
  leaders this turn, take 2 fewer actions to draw this card", and separately
  "The play of picking up Michelangelo cheaply as Hammurabi and then using
  Michelangelo as a hero to pick up the Taj Mahal for free or for 1 token is
  quite intense" -- i.e. the free take is a known, deliberate line of play,
  not an artifact.

This is the ONLY thing in the base game that can make a wonder take cost
zero. Michelangelo's waiver cancels the surcharge but can never go below the
row's own minimum of 1; Hammurabi's `leaderTakeCivilActionDiscount` is
leaders-only. Taj Mahal's is a flat subtraction off the whole cost and
routinely hits the `max(0)` clamp.

### The population, and how completely the clause explains it

All 317 Taj Mahal takes in the 1,011-game corpus, cross-tabulated by whether
the acting player had already replaced a leader earlier in that same turn
(the journal's `"<Colour> elects <New> <Old> dies"` shape -- a *first* leader
prints no `dies` clause and is not a replacement):

| earlier in the turn | takes | free (no cost clause) | paid |
|---|---|---|---|
| replaced a leader | 184 | **149** | 35 |
| elected a FIRST leader (no replacement) | 8 | 0 | 8 |
| no election at all | 125 | 1 | 124 |

The single free take in the bottom row (`7523665`) is a journal ordering tie,
not a counterexample: its line and the replacement that licenses it carry the
**same timestamp to the second**, and the same turn contains a `"puts Taj
Mahal back in the row"` printed *before* the take it undoes. BGO's journal is
sorted by a one-second-resolution timestamp and is not stably ordered within
a second.

Scoring every take against the two models -- implied row position must be 1,
2 or 3, and a free take is consistent whenever the model's cost lands at or
below 0 -- with Michelangelo's waiver applied where he is in play:

| | violations of the 1/2/3 row-position bound |
|---|---|
| sixth pass's model (`row + surcharge`) | **181 of 317 (57%)** |
| with Taj Mahal's printed clause | **1 of 317 (0.3%)** |

The 35 paid takes made after a replacement are not counterexamples either:
under the clause they simply imply a more expensive row slot (cost + 2 -
surcharge), and their implied positions land inside 1-3.

**The negative control the corpus supplies for free**: of ~6,700 takes of the
other fifteen wonders, **zero** carry no cost clause -- including the 129
Taj-Mahal-adjacent case of a wonder taken on the line immediately after an
election. Whatever this is, it is printed on exactly one card, and the data
says so before the card text does.

Two secondary facts fall out of the same table, both consistent with the
clause being a real and known line of play: Taj Mahal is taken in a turn
containing an election **64%** of the time, against 7.5-27% for every other
wonder; and this is the only wonder whose takes cluster in rounds 4-7 behind
an Age I leader swap.

### Finding (ENGINE BUG, confirmed, fixed, tested)

`costs::take_cost` and `costs::can_take_gated` both priced the row without
the clause, so `legal::legal_moves` refused to offer a Taj Mahal take the
human could demonstrably afford -- the `ParserGap: Taj Mahal's take cost ...`
bucket the sixth pass deliberately grew to 81 games.

**Fixed** by giving the card its ability in the data
(`takeCivilActionDiscountIfLeaderReplacedThisTurn: 2` ->
`Special::TakeCivilActionDiscountIfLeaderReplacedThisTurn`, the only card in
the base game that carries it) and reading it in both pricing paths off the
card **sitting in the row**, gated on a new per-turn
`PlayerState::replaced_leader_this_turn` (set in `apply::h_play_leader` only
when a leader was actually swapped out, cleared in `economy::end_of_turn`
beside `hammurabi_replaced_this_turn`, which it is strictly weaker than).

**Tests**, all four confirmed to fail with the fix reverted and pass with it:
`costs::tests::taking_taj_mahal_costs_two_civil_actions_less_when_a_leader_was_replaced_this_turn`
(2 -> 0, the clamp case),
`costs::tests::taking_taj_mahal_from_an_expensive_slot_after_a_replacement_still_costs_the_remainder`
(3 -> 1, proving it is a flat subtraction and not a surcharge waiver),
`costs::tests::move_generation_offers_taj_mahal_with_no_civil_actions_left_after_a_replacement`
(the `can_take_gated` half -- wiring only `take_cost` leaves the move
ungenerated), and
`apply::tests::playing_a_first_leader_into_an_empty_slot_is_not_a_replacement_but_swapping_one_is`.
Two more pass either way, on purpose, pinning the negative controls:
`no_wonder_other_than_taj_mahal_is_discounted_by_a_leader_replacement` and
`taking_taj_mahal_costs_full_price_when_no_leader_was_replaced_this_turn`.

### Measurement (`replaystats`, full 1,011-game corpus)

| | sixth pass | seventh pass (this one) |
|---|---|---|
| mean rounds reached (of 19.27 played) | 5.71 | **5.84** |
| decisions recorded in Age II or later | 3.5% | **3.9%** |
| decision points recorded | 67,574 | **69,720** |
| `ParserGap: Taj Mahal's take cost ...` | 81 | **0 (bucket gone)** |

Every other bucket grew, which is this file's usual pattern and not a
regression: `StuckPending: decider != expected actor` 137 -> 148,
`IllegalMove: PlayAction` 113 -> 122, `Pop` 110 -> 117, `WonderStep` 102 ->
111, `Build` 94 -> 103, `Upgrade` 58 -> 63, `Take` 17 -> 22. The 81 games
that used to stop on Taj Mahal now run past it into whatever was next.

### What this says about the method, and what to do next

The clause was sitting in `sources/` the whole time, in two files, and no
prior pass looked -- every pass instead reasoned from the corpus about what
mechanic *could* produce a free take. What made the difference here was
running the search in both directions at once: the corpus said "only this
one card, only after a leader swap, and by 2", which is specific enough to
recognise the right sentence the moment you read it.

That generalises directly: **the card data is a plausible suspect whenever a
single named card misbehaves**, and it is cheap to check.

**And it was checked, this pass, rather than left as a suggestion**: every
card in `sources/bga_throughtheages_material.inc.php` that carries a `text`
field (205 of them) was diffed against `data/*.json` -- all 16 wonders and
all 24 leaders read individually, the rest filtered for an our-side text
materially shorter than BGA's. **Taj Mahal was the only card with a
mechanical clause missing.** The four apparent misses are name spellings
(`J.S. Bach`, `Leonardo Da Vinci`, `Maximillien Robespierre`, `Ocean Liner
Service`) and the three apparent text gaps are terser paraphrases of the same
rule (Development of Markets, Iconoclasm, Impact of Technology). That is a
real negative result and worth recording so nobody re-runs it: the remaining
`replaystats` buckets are not more missing card abilities.

## Eighth pass: the hidden-`PrepareEvent` wall, closed — BGO logs every preparation

The premise every earlier pass built on ("BGO's journal never logs who
prepared an event") was simply **wrong**, and it cost this binary a lot. Every
preparation is one journal line:

```text
Orange plays event Orange scores 1 culture; Current event:; A / Development of Settlement; ...
```

which names **who** (the line's actor), **which age of card** they prepared
(`apply::h_prepare_event` scores exactly `card.level()`, so the culture clause
IS the age — never 0 anywhere in the corpus, Age A events being setup-only),
and **what the reveal turned up**. 17,889 `"plays event"` lines, 17,889
`"Current event:"` clauses, never one without the other.

`replay.rs` had been guessing forward instead: a hidden `PrepareEvent` at
every Politics decision no line explained, satisfied by popping the next
observed reveal off a FIFO. Most of those decisions are ordinary passes, so
events fired turns early for the wrong player (2p game `7522647`: Development
of Science at round 4, journal says round 10) and everything downstream
desynchronised.

### The pile model, verified against the whole corpus

`rust/src/event_plan.rs` now solves the record as one whole-game constraint
problem. BGO logs each recycle (`"Future events are now current events."`,
rendered before the `"Current event:"` clause on the line whose pop emptied
the pile — i.e. `reveal_current_event`'s own TRAILING recycle), which cuts the
reveal sequence into piles. Each pile is by construction exactly the cards
prepared while the previous one was consumed, so:

> the multiset of ages revealed in pile `b+1` must equal the multiset of
> `"scores N culture"` values on the preparations made during pile `b`.

Over the 1,011 games that holds in **3,291 of 3,291** windows (2,283 complete
piles exact, 1,008 truncated final piles consistent), with the setup pile
measuring `num_players + 2` in every game. Nothing is fitted; a violation is
`MismatchKind::EventPlanInfeasible` and stops the game. **Zero games hit it.**

What stays underdetermined: within one pile and one age the recycle shuffle
destroys the order, so which same-age preparation became which same-age reveal
is unrecoverable — and irrelevant, since the pile is a set and the reveal
ORDER is read from the journal. Tie-break is positional; the only state it can
touch is `seeded_by`. The setup pile and every recycle are now GROUNDED to the
journal's reveal order (contents from the engine, only the never-logged
shuffle order replaced), which turns the pre-reveal "is the right card on
top?" check into a real test of the model rather than a re-forcing of it.

Also fixed in passing: territory families recur across ages under one printed
name (`"Vast Territory"` at Age I AND Age II) and `card_table.rs`
disambiguates with a `" (I)"`/`" (II)"` suffix BGO never prints — but BGO does
print the age in the same clause, so the lookup uses both now. The old prescan
silently resolved to whichever age came first.

### ENGINE BUG: Julius Caesar's once-per-game was spent by DECLINING it

Chasing the residual `PolPass` bucket turned up a real rules bug. Printed text
(`sources/bga_throughtheages_material.inc.php`): *"After you play a political
action, you may play another political action. This ability can be used only
once per game."* `apply::end_politics` got both halves wrong — it spent the
once-per-game whenever the second political action resolved, **including when
that action was a pass**, and it armed a second action after a pass as the
FIRST action, which the text does not grant.

The discriminating evidence is game `7523338`: the same player is offered and
declines the second political action on rounds 3, 4, 5 and 7 and still holds
the ability, which is impossible under the old model. (The corpus-wide counts
— 142 player-games with exactly one double, no offer ever recurring after a
double is used — corroborate but do not discriminate: they are consistent with
once-per-game either way.) `end_politics` now takes a
`PoliticalAction::Played/Passed`. Python has the same bug; the printed card is
the oracle.

Two consequences for `replay.rs`, both fixed with the engine change:

- a human who declines the second action leaves BGO's `"passes Political
  Phase"` line wherever they clicked it, routinely AFTER some of their own
  Action-phase lines — which the engine cannot make legal until politics
  closes. Such a line is now that pass's late confirmation, tracked per seat
  and consumed one-for-one.
- the `"plays event"` line is skipped as a confirmation, so its preparation is
  applied at the NEXT line — which with Caesar armed is routinely that same
  player's own pass line. An owed preparation now outranks
  `resolve_intervening`'s explicit-political-line fast path, instead of the
  pass being applied as the FIRST political action and the preparation being
  stranded at the head of the queue (game `7522650`).

### Measurement (`replaystats`, full 1,011-game corpus)

| | seventh pass | this pass |
|---|---|---|
| mean rounds reached (of 19.27) | 5.90 | **6.37** |
| decisions in Age II or later | 4.6% | **5.5%** |
| `StuckPending: decider != expected actor` | 94 | **14** |
| `IllegalMove: PolPass` | 64 | **9** |
| `EventPlanInfeasible` | — | **0** |
| games completed | 0 | 0 |

Still 0 complete, so `analysis/index.tsv`'s final scores remain un-cross-
checked. The two residual politics stops are no longer event attribution:
`7523338` line 174 is an aggression's spoils `Choose` left unresolved, and
`7523657` line 143 is a colonize force left undrained when the acting player
IS the decider (`auto_drain_colonize` only runs on the `decider != expected
actor` branch). Both are in other buckets' territory.

### ENGINE BUG: Trade Routes Agreement's food<->resource substitution was computed and discarded

Found chasing the `IllegalMove: Pop` bucket (166 games, `docs/RULES_SPEC.md`
§5.9). `effects::compute` already summed the pact's grant into
`Stats.food_as_resource`/`resource_as_food` ("Civilization A can use 1 food
as 1 resource during its turn" / the B-side mirror,
`bga_throughtheages_material.inc.php`), but nothing ever read those two
fields — no payment path could use them, and `bots/board_yields.rs` valued
the pact at zero. Fixed engine-side with two new opt-in moves
(`Move::TradeFoodAsResource`/`TradeResourceAsFood` — not folded silently into
`Pop`/`Build`/`Upgrade`'s own cost, since this is the player's choice, not an
automatic discount) and a per-turn allowance
(`PlayerState::trade_*_used_this_turn`, cleared in `economy::end_of_turn`
like `replaced_leader_this_turn`). Confirmed against real games (7522168,
7521605, 7523242, 7523541: a human visibly paying part of a Pop cost in
resources). `rust/src/costs.rs::tech_cost` has no resource component to
substitute into, so `develop`/technology payment is untouched by design, not
by omission.

Also root-caused but deliberately NOT fixed here — it belongs to the
`PlayAction`/Frugality/Engineering Genius bucket, not this one:
`corpus.rs::build_card_index` collapses every multi-age action card to its
EARLIEST age variant whenever BGO's journal text carries no age suffix (it
never does) — e.g. resolving a real Frugality (I) play as Frugality (A),
understating its own `gainFood`. That single root cause explains roughly 123
of the 166 `IllegalMove: Pop` failures as a downstream symptom (wrong food
carried forward from an earlier turn's mis-resolved card), not a
population-cost bug at all — worth knowing before re-measuring this bucket.

## Ninth pass: free-civil-action cards (Urban Growth, Rich Land, Efficient
## Upgrade, Breakthrough, Frugality, Engineering Genius) -- one root cause,
## one engine bug, both replayer-side

`IllegalMove: PlayAction` (the single largest bucket, 168 games) and most of
this cluster's `ParserGap`/cost-mismatch entries turned out to be ONE bug, not
two: nine card families (these six plus Territories/Aggressions/Military
Bonuses) print the SAME name once per age with a stronger effect each time,
and BGO's journal text never carries an age tag. `corpus::build_card_index`'s
bare-name lookup (`HashMap::or_insert`) therefore always resolved to whichever
age iterates first in `CARDS` -- Age A, in practice always wrong once a game
is past its first age. That made both symptoms look real at once (an
under-priced discount looks like both "too little discount" and "the option
we needed isn't offered") without there being a second cause -- widening the
option set, the tempting fix, would only have hidden it further. Fixed with
`corpus::best_age_sibling`/`family_siblings` plus
`replay_common::resolve_named_card_by_effect`, which re-resolves a card
against the SAME journal line's own numbers (a discount's implied payment, a
science bonus, a food bonus) wherever that evidence is available, falling
back to "closest age not newer than the deck's current age" only when it
isn't, and corrects an earlier `TakeCard` guess in the player's hand rather
than trusting it blindly.

**ENGINE BUG, confirmed, fixed, tested:** Breakthrough's "may spend its order
on a revolution instead" (RB p.15) computed its own eligibility as a flat
"no civil action spent this turn" in two places (`legal::action_card_playable`,
`apply::h_play_action`) -- a straight port of the same simplification in the
Python oracle, which never applies leader Maximilien Robespierre's variant
(no MILITARY action spent instead). A Robespierre-led player who had spent a
civil action but no military action was illegally denied the option (games
`7523216`, `7523482`). Fixed in Rust only, deliberately diverging from Python
(correctness over parity) -- both call sites now share `legal::
revolt_pool_ok`, the same predicate `can_revolt` itself uses, rather than
each keeping an independent copy (the fix is the shared function, not just
the missing branch: three independently-recomputed copies of one rule is the
recurring bug class here, not this one missing conditional).

### Measurement (`replaystats`, full 1,011-game corpus)

| | eighth pass | this pass |
|---|---|---|
| mean rounds reached (of 19.27) | 6.37 | **7.83** |
| decisions in Age II or later | 5.5% | **19.1%** |
| `IllegalMove: PlayAction` | 168 | **57** |
| games completed | 0 | 0 |

The residual 57 `PlayAction`s and handful of `ParserGap`s are Breakthrough
lines blocked by an unrelated open `Pending::Colonize` (one game) or a
civil-action-count drift this pass did not chase further (one game,
`7523341` -- Breakthrough itself now resolves to the right age; the
remaining gap is in per-turn action-pool bookkeeping, not this cluster).
Everything else this cluster owned -- the Urban Growth/Rich Land `ParserGap`s
and the Urban-Growth/Rich-Land-attributed `UnrecoverableHiddenInfo` cost
mismatches -- is gone. Still 0 complete.

## Tenth pass: "aggression defense" (122 games) was never unrecoverable -- BGO logs one clause per committed card, not a bare count

The `UnrecoverableHiddenInfo: aggression defense: N committed defense
card(s), BGO logs only the count, never identities` bucket (122 games, the
largest `UnrecoverableHiddenInfo` bucket in the corpus) turned out to be a
parser bug, not real hidden information -- found by reading the raw
`"<Color> defends ..."` lines instead of trusting this file's own prior
claim. The old `defends_count` read only the leading digit of the FIRST
clause and discarded the rest of the line; the rest of the line names every
committed card. A `"Defense card +2/+4/+6 played"` clause names its printed
bonus directly, and `data/cards_military_actions.json`'s `bonus`-type cards
have exactly one card per value (one per age I/II/III, six functionally
identical physical copies each) -- so the number alone is a complete,
unambiguous identity; "which of the six" was never a real question. A
`"military card played"` clause is any hand card with `defense_bonus == 0`
(`interact::defense_points`'s flat +1 branch, which is every non-`Bonus`
military-deck card) -- resolved the same way `discard_solver::DiscardSolver`
already resolves a forced hand-limit discard, because it is the same
underlying fact (a specific card permanently leaves the hand). When the
replay's own fictional simulated hand happens to hold no zero-bonus
candidate at all (observed in the real corpus, on hands as small as 1-2
cards), a filler card is grounded instead of stopping the game: identity is
provably irrelevant here, since every non-`Bonus` card defends for the same
flat +1 no matter which one it is.

New code: `DefenseClause`/`parse_defense_clauses`/`defense_bonus_card`/
`flat_defense_filler` in `replay_common.rs`; `resolve_aggression_defense`
rewritten to apply one `Move::Defend` per clause instead of returning
`UnrecoverableHiddenInfo`. Not an engine bug -- `interact::defense_points`
and `Pending::Defense`'s move generation were already correct; only the
journal parser was throwing information away.

### Measurement (`replaystats`, full 1,011-game corpus)

| | ninth pass | this pass |
|---|---|---|
| mean rounds reached (of 19.27) | 7.83 | **8.18** |
| decisions in Age II or later | 19.1% | **21.3%** |
| `UnrecoverableHiddenInfo: aggression defense` | 122 | **0** |
| games completed | 0 | 0 |

All 122 games that used to stop here now run deeper into the same journal
before hitting a DIFFERENT, pre-existing stop reason (the usual pattern in
this file: closing one wall exposes the next one, visible as small increases
across `Pop`/`Take`/`Bid`/`Build`/`Destroy`/`WonderStep`/`Upgrade`/`Develop`
and a few others). None of those buckets belong to this pass and none were
touched.

## Eleventh pass: `IllegalMove: Bid` (103 games) -- 95 reclassified as honest hidden info, not fixed; 8 left open with a different, already-known cause

Not an engine bug. `interact::max_force`'s ceiling depends on `bonus_pool`,
which reads `p.hand_military` directly -- and this file's own top doc
comment already establishes that hand is SIMULATED filler for essentially
its entire content: a military bonus card enters a real player's hand via
an anonymous end-of-turn draw and is never grounded to its true identity
unless the journal later shows it PLAYED. A bidder can genuinely be holding
one this binary has never observed, so its computed ceiling is a LOWER
bound, not an exact figure. Sub-histogram of the 103 (by shape, not by
mechanic -- there is only one mechanic here):

| shape | count | 
|---|---|
| a real raise (`n` > current high bid) that exceeds this binary's own computed ceiling by 1-6 (mode 1) | 95 |
| the auction itself is missing (`legal_moves` shows ordinary Action-phase moves, no `Pending::Auction` at all) | 7 |
| a `Pending::Colonize` from a DIFFERENT, still-open auction is on top | 1 |

The 95: confirmed by direct inspection (`REPLAY_DEBUG`) that the bidder's
reconstructed hand at the failure point holds zero `Bonus`-type cards in
every case, while the attempted bid consistently exceeds the computed
ceiling by a small amount (1-6, matching "one or two hidden bonus cards
worth 1-3 each," not a large or systematic offset that would indicate a
missed rule). `bid_ceiling_mismatch` (`replay_common.rs`) now reports these
as `MismatchKind::UnrecoverableHiddenInfo` instead of `IllegalMove` --
narrowly, only when the bid is a genuine raise against the correctly
identified bidder and exceeds their own computed ceiling, so an actual
engine defect elsewhere still reports as `IllegalMove` unchanged. This is a
RECLASSIFICATION, not a fix: these games still stop in the same place: it
replaces a label that implied "possible engine bug" with the honest reason,
matching this file's own `UnrecoverableHiddenInfo: build cost mismatch`
bucket's precedent.

The remaining 8 are NOT the same bug. Traced one (`7521428`) end to end:
`interact::start_auction` correctly computes the eventual bidder's force as
0 and silently files the territory to `past_events`, skipping the auction
entirely -- and the true player, per the journal, really did have 0 units
built at that exact moment... EXCEPT that two colonize auctions earlier in
the SAME game, `Replayer::auto_drain_colonize` (the already-documented
"does NOT verify which units were spent" approximation) always picks the
engine's cheapest offered `SendUnit` first, which is not necessarily the
unit the real human sacrificed. Over multiple colonizations this can
deplete a cheap unit type (Warriors) that the real player still had in
hand by choosing bonus cards instead. Fixing this needs the same
`"Sacrificed Units:; ..."`-grounding this file's own "gives up on" section
already flags as unimplemented, not something new to this pass -- left
open.

### Measurement (`replaystats`, full 1,011-game corpus)

| | before this pass | after |
|---|---|---|
| `IllegalMove: Bid` | 103 | **8** |
| `UnrecoverableHiddenInfo: colonization bid ...` | 0 | **95** |
| mean rounds reached / decisions Age II+ | unchanged (95 are a reclassification, not a fix) | unchanged |

## Twelfth pass: REPLAYER BUG -- `resolve_intervening` treated "decider == expected_actor" as "nothing left to resolve," even with a live `Pending::Colonize`/`Pending::Auction` still open for that same player

Root cause of 79 of the 178 `IllegalMove: Take` stops (44% of that bucket) and a chunk of the still-open `IllegalMove: Bid` gap. `resolve_intervening`'s job is to auto-resolve anything standing between "control returned to `state.current`" and the next real journal line; its one shortcut, `if decider == expected_actor { ...; return Ok(()); }`, was written for the ordinary case (a live political decision) and silently applied to two others it was never meant to cover:

- A **`Pending::Colonize`** has no real `Move` anywhere in the journal's vocabulary at all -- this file always auto-drains it (`Replayer::auto_drain_colonize`). `decider == expected_actor` here just means the colonizer also happens to be up next for something unrelated (their own `Take`, on 72 games; found first on `7523355`, where Purple's own colonize sits undrained right up to their own next `Take Scientific Method` line) -- not that there is nothing to resolve.
- A **`Pending::Auction`** still owed a real `Bid`/`BidPass` from `decider`, but the very next journal line is unrelated (7 games) -- because that decision is FORCED (their own `interact::max_force` ceiling no longer clears the standing bid) and BGO's UI auto-passes with no click to log, the same shape as `Pending::Defense`'s forced 0-defender `DefendDone`. Root-caused on `7523347`: a 4-way auction where the second-to-last bidder, already outbid past their own ceiling, has no "passes" line anywhere in the journal at all.

Both are now handled explicitly, ahead of the shortcut, mirroring the `Pending::Choice(GainBlock/FreeBuild/DiscardMilitary)` cases already there: `Colonize` always drains; `Auction` defers to the real line only when the upcoming line actually is that decider's own `Bid`/`Pass`, otherwise auto-passes ONLY if `BidPass` is provably their sole legal move, and fails loudly (`StuckPending`) if a real raise was still available -- never guessing a human's decision. New tests pinning each shape, confirmed to fail with the fix reverted.

### Measurement (`replaystats`, full 1,011-game corpus)

| | before this pass | after |
|---|---|---|
| mean rounds reached (of 19.27) | 8.51 | **9.11** |
| decisions in Age II or later | 23.1% | **28.2%** |
| `IllegalMove: Take` | 178 | **148** |
| games completed | 0 | 0 |

Closing this wall exposed the usual next ones (small increases across every
other bucket, including the `UnrecoverableHiddenInfo: colonization bid`
count above -- more games now reach a real bid decision at all). Two new
honest `StuckPending` reasons appeared (15 games total) where a bidder
genuinely owed a real, un-loggable decision this file correctly refuses to
guess at, rather than silently mis-resolving as before.

**A `>=` -> `>` change to `costs::take_gate`'s hand-limit comparison landed
and was reverted the same day** (70 of the remaining `IllegalMove: Take`
games showed `hand_civil_size == civil_hand_limit` exactly, zero
counterexamples, which read at the time like a boundary-off-by-one). Coordinator
review caught it: RULES_SPEC §2.5 and independent community sources both
read `>=`, and a wrong loosening of `legal_moves()` is worse than the stall
it fixes. Verification of the two more likely explanations (simulated hand
filler; an undercounted CA total) was attempted but not completed cleanly
in the time available -- a hand-rolled journal-arithmetic cross-check kept
producing false "exceeds" signals that dissolved on inspection into script
bugs (unpaired `Take`/`PutBack` undo lines, military-vs-civil-build text
confusion, and the "Development of Civilization" event's one-time free
civil action, which this project's own `costs::civil_life_ca_free`
already models but a quick Python reimplementation did not). Left open for
whoever picks up `IllegalMove: Take` next: the two live leads are worth
checking with the REAL engine's own cost functions instrumented, not a
reimplementation.

## Fourteenth pass: `IllegalMove: Pop` -- two more silently-dropped leader lines, Trade Routes wired into Pop, and one ENGINE BUG (WeakestPlayer's tie-break was backwards)

(Landed concurrently with the Take/Bid pass just above -- the two were
worked in parallel by different passes on different buckets, so the
before/after numbers here are relative to THIS pass's own before/after
runs, not to the Take pass's; see each section's own measurement table for
what it actually held constant.)

Re-measured at 175 (a freshly de-confounded number, per an earlier pass's own
note that most of the previous count was downstream of a card-age bug since
fixed). Four fixes, in landing order:

1. **REPLAYER BUG**: "Alexander dies after building his great Empire" was
   classified as pure flavour `Bookkeeping` and dropped -- it is really
   `Move::RemoveLeaderYellow`, and dropping it lost the yellow token it always
   carries, drifting `pop_cost` for the rest of the game.
2. **REPLAYER BUG**: the Pop handler never tried `Move::TradeFoodAsResource`/
   `TradeResourceAsFood` (Trade Routes Agreement), and `ActionClass::Destroy`
   never recognised a `ChoiceKind::LosePop` pending (only `DestroyOwn`) --
   both wired in, gated on the journal's own stated numbers so neither can
   mask an unrelated mismatch.
3. **ENGINE BUG**: `events::apply_single_target`'s tie-break used the SAME
   current-player-first order for `WeakestPlayer` (a penalty) as
   `StrongestPlayer` (a bonus) -- backwards for the penalty half. RULES_SPEC
   §5.3 "ties broken in favor of the current player" is directional: favoring
   the current player means picking them FIRST for a bonus, LAST for a
   penalty. Settled by measurement, not argument: of 1,011 games, 63 had a
   genuine `WeakestPlayer` strength tie; the old (un-reversed) pick matched
   the journal's real target once, the reversed pick matches 62. Confirmed
   correct on real self-play too, not just this replayer -- flagged and the
   climb was halted/restarted for it.
4. **REPLAYER BUG**: "Christopher Columbus discovers &lt;Age&gt; / &lt;Territory&gt;" (his
   printed "remove Columbus to colonize a territory for free" ability) was
   also silently dropped -- "Christopher Columbus" is itself a known card
   name, so the line matched `classify`'s generic "known card name leads the
   line" `Bookkeeping` catch-all. This is the one line in the whole corpus
   with neither a leading colour nor a trailing consequence clause naming the
   actor; `Line` gained a `color` field (column 2, previously parsed and
   discarded) to read it. The territory also needed grounding into
   `hand_military` before applying (same pattern as `DeclareWar`/
   `PlayAggression`), since it is routinely the FIRST evidence of that exact
   card.

### Sub-histogram (rebuilt fresh each time, not inherited from the earlier pass)

Before any fix: 175 failures, split roughly 24 games "pending sits open and
blocks the actor's own Pop" (most from the WeakestPlayer bug above -- a
tied penalty landing on the wrong, currently-acting player), ~11 the
Alexander line, ~28 the Columbus line (all correlated 1:1 with a preceding
"Christopher Columbus discovers" line), and the rest a long tail of small
food/yellow-bank drift this pass did not chase further (`docs/REPLAY.md`'s
long-standing "gives up on" list). After all four fixes: 90 remain, of
which 56 are that same drift (`stated == our pop_cost`, but our `food` is a
few short -- no single dominant cause; leaders/rounds/player-counts spread
evenly, unlike the Columbus cluster), 17 are a genuinely different pending
(`PlunderSplit`/`Raid` choices, aggression-defense territory, left alone),
11 a residual cost-tier mismatch with no shared leader this time, and 5 a
plain civil-action shortage from an upstream bucket.

### Measurement (`replaystats`, full 1,011-game corpus, all four fixes plus a
### concurrent worker's unrelated Colonize/Auction fix landed in between)

| | before this pass | after |
|---|---|---|
| mean rounds reached (of 19.27) | 7.83 | **9.84** |
| decisions in Age II or later | 19.1% | **33.9%** |
| `IllegalMove: Pop` | 175 | **90** |
| games completed | 0 | **4** |

The four completions are the first this project has ever replayed a whole
game with every human action legal -- but NOT, yet, anything to check
`analysis/index.tsv`'s real final scores against: `replay_game`'s
`completed` flag is set purely by reaching the journal's own `"End of
game"` marker line, independent of whether the RECONSTRUCTED engine state
ever actually flips `state.game_over` (`game::finish_game`, only called
from `advance_turn`'s own final-round wrap). All four completions have
`state.game_over == false` at that point (`replay`'s own `n_score_checked`
stays 0), so `GameResult::engine_scores` is `None` and there is still
nothing to compare. Left open -- outside this pass's bucket -- but worth
knowing before the next pass that reaches for `analysis/index.tsv`.

## Fifteenth pass: REPLAYER BUG -- the colonization sacrifice was approximated away, and the approximation ate army units the human never spent

`"colonization bid of N exceeds this binary's computed force ceiling"` was
this corpus's third-largest bucket (121 games) and carried an
`UnrecoverableHiddenInfo` label blaming hidden hand information. **The label
was false**, for the third time on this project, and in the same shape as
the previous two (event attribution, aggression defense): the journal names
the thing the comment said it never names.

`"<Color> colonizes a <Territory> Sacrificed Units:; 1 Warrior; 1
Colonization card +2; Colonization bonus: +2; Total force: 6; ..."` is one
clause PER COMMITTED PIECE, not a bare force total:

- `"1 <Unit>"` — a sacrificed army token. Each of the ten unit cards has a
  distinct name in exactly one age, so the name alone is a full identity
  (BGO prints `Warriors` in the singular, and only that one).
- `"1 Colonization card +N"` — `N` is 1, 2 or 3, and `data/
  cards_military_actions.json` has exactly one `bonus` card per value, one
  per age I/II/III. Same argument as `"Defense card +6 played"`.
- `"1 Military card +1"` — James Cook's discard-for-force. The ONE piece
  whose identity really is withheld; only its count is claimed.

Two things were wrong, and the second is what actually drove the bucket:

1. The sacrifice was never applied. `auto_drain_colonize` took the engine's
   first offered move at each step, i.e. weakest unit first. A human force
   of "one Knight plus a +3 bonus card" replayed as four sacrificed
   Warriors — army tokens permanently gone from a board this file otherwise
   tracks exactly. Every later colonization ceiling, military strength and
   bid of that player was computed against a smaller army than they had.
2. A bonus card in the winner's hand has to be grounded while the auction
   is still OPEN. `interact::colonize` snapshots the hand into
   `Pending::Colonize::bpool` the instant the auction settles, so anything
   grounded later can never be sent and the engine is forced to make the
   difference up out of units.

Both are fixed: `prescan_colonize_sacrifices` reads the whole record up
front (the `event_plan`/`prescan_future_military_needs` idiom),
`Replayer::ground_auction_winner_hand` grounds the winner's named bonus
cards before any move is applied against the open auction, and
`Replayer::drain_colonize` plays the journal's own list. 818 of 837
colonizations in the corpus now replay from the journal; the other 19 fall
back to the old approximation and still flag the game.

## Build/Upgrade/WonderStep cost-mismatch cluster: sub-categorisation, and two REPLAYER fixes (Barbarossa, Bach)

Owned bucket: `IllegalMove: Build`/`Upgrade`/`WonderStep` plus
`UnrecoverableHiddenInfo: build cost mismatch` -- all "resource-payment
shaped," ~275-300 games depending on which pass's numbers you read them
against (other workers' fixes land concurrently and shift the raw counts
underneath this one, per this file's own "closing one wall exposes the
next" pattern -- read mean rounds / Age II+ %, not raw bucket counts).

### Method: a scratch dump beats another round of guessing

Added a throwaway `src/bin/costdebug.rs` (not committed -- delete it if you
find it lying around) that reuses `replay_game` and prints every failing
game in this cluster; paired with a REPLAY_DEBUG extension (kept, see
below) that prints the ACTUAL computed cost for the attempted
`Build`/`Upgrade`/`WonderStep` alongside the journal's own stated payment.
Categorising all ~300 failures by (computed cost vs journal-stated payment)
gave a clean split:

| category | count (first pass) |
|---|---|
| cost computation matches the journal exactly, but `resources` is short by 1-5 (mode 1-2, roughly geometric) | ~180 |
| `Build` specifically: cost matches, but `workers_free == 0` (no free worker for a fresh unit/building) | 22 |
| `Upgrade` specifically: cost matches, but `civil_actions`/`military_actions` is 0 (a different, unexplored gate) | ~55 |
| genuine cost MISMATCH (computed != journal) | a handful (2-3) |
| unparsed by the scratch tool (multi-line mismatch text) | 25 |

The dominant shape -- cost formula correct, `resources` short by a small,
often-compounding amount -- says the bug is NOT in `costs.rs` at all; it is
resource-ACCOUNTING drift from an earlier turn. Traced by extending
`REPLAY_DEBUG_ALL` (not new ad hoc prints, per this file's own convention)
with a per-end-of-turn `resources`/`food`/`science`/`culture` total, then
diffing that sequence against every End Turn line's own `"N resources (now
M)"` clause in the raw journal (BGO prints the ground truth on every single
turn, for free) to find the FIRST round where they disagree -- much faster
than reasoning about the failing line itself, which is usually many turns
downstream of the real cause.

### Root cause found (two sibling bugs, same shape)

Two journal line shapes `corpus.rs::classify` matched only far enough to
return `Bookkeeping` -- silently dropping the line instead of applying it.
Both are leader abilities BGO logs under the LEADER'S name, not the
player's colour (the same "no leading colour" shape `RemoveLeaderYellow`
already gets special-cased for), and both already have a fully-implemented
engine `Move` (`legal.rs`/`apply.rs`) that the bot side uses correctly --
only the replayer never constructed it:

- **`"Barbarossa enlists a <Unit>; <Color> spends N food[; <Color> loses N
  military resource][; <Color> spends M resource(s)]"`** -- Frederick
  Barbarossa's leader ability, a free population increase immediately spent
  building the named unit (`Move::Barbarossa`). **135 games / 425 lines**
  corpus-wide. Dropping it under-counts the player's `yellow_bank` spend
  (and therefore `resources`/`food` and every `consumption`/`corruption`
  band derived from it) for the rest of the game, compounding turn over
  turn -- exactly the shortfall shape above.
- **`"Johannes Sebastian Bachupgrades <From> to <To> ..."`** -- J. S.
  Bach's leader ability, a cross-family Temple/Library -> Theater
  conversion (`Move::BachTheater`; an ordinary `Move::Upgrade` only ever
  offers same-family targets). **79 games / 111 lines** corpus-wide. Also
  the one line in the whole corpus BGO glues the leader's name directly
  onto the verb with no space at all (`corpus.rs` already had a comment
  flagging this as the "one confirmed exception" to its space-delimited
  assumption -- it just then threw the line away instead of parsing it).

Both fixed the same way: `classify` now resolves the target card and
returns a real `ActionClass` (`Barbarossa`, `BachTheater`); the "no leading
colour" dispatch in `replay_game` resolves the actor (Barbarossa from the
trailing "<Color> spends" clause, mirroring `RemoveLeaderYellow`; Bach as
`state.current`, since an action-phase ability can only ever be the current
player's own move, mirroring `EndTurn`) and applies the real `Move`. New
tests in `corpus.rs` (classification) and `replay_common.rs` (the actual
`try_apply`/`apply_one` dispatch, values mirrored from the existing
`apply.rs` direct-engine tests) -- both confirmed to fail with the fix
reverted (classify's two arms put back to `Bookkeeping`) before landing.

REPLAYER, not ENGINE: `legal.rs`/`apply.rs` already had these right for bot
play; only the journal parser was throwing the lines away.

### Measurement (`replaystats`, full 1,011-game corpus)

| | before this pass | after |
|---|---|---|
| mean rounds reached | 9.45 | **9.57** |
| decisions in Age II or later | 30.8% | **31.8%** |

Raw `IllegalMove: Build`/`Upgrade`/`WonderStep` counts are roughly flat
(expected: games run deeper and surface previously-unreached failures in
the SAME bucket, not just other buckets). Re-running the same
cost-vs-journal categorisation after this fix shows the "`resources` short
by a small amount" shape is still the dominant one (~170 of ~300) -- these
two bugs were real and worth fixing, but at least one more root cause of
the same shape remains.

### Open, with a concrete next lead

Traced one post-fix example (`7522625`, Purple, round II8) all the way to
its exact turn of divergence the same way as above: the player's own
individual "spends" clauses that turn all reconcile exactly against this
binary's cost formulas (no bug there), but the real journal's End Turn line
carries NO `"CORRUPTION!"` clause while this binary's reconstruction
computes `corruption(blue_available) == 2`. `economy::corruption`'s bands
are independently verified correct against
`sources/bga_throughtheages_material.inc.php`'s own
`$this->resource_corruption` table (byte-for-byte match) -- the bug, if
there is one, is upstream in `blue_total`/`blue_used`/`Denoms`, not the
corruption formula itself. Ruled out so far: the specific farm/mine cards
Purple has built by that point (Bronze/Agriculture, both denomination 1 in
this codebase's own `Denoms::of`, so building them changes nothing here);
`blue_total` staying flat at 16 all game (no `blueTokens`-granting
card/wonder involved in this specific example). NOT yet checked: whether
`blue_total`'s starting value of 16 (`game.rs`) is even the right constant
per player count, and whether some OTHER effect should be adding to it by
this point. Left open -- ran low on context before finishing this trace;
the `REPLAY_DEBUG_ALL` extensions this pass added (`blue_used`'s own
denom/token breakdown, `end_of_turn`'s ENTRY/pre-corruption/POST prints)
are enough to pick this back up without re-instrumenting anything.

The `Build: workers_free == 0` (22 games) and `Upgrade: ca/ma == 0` (~55
games) sub-buckets from the table above were NOT investigated this pass --
untouched, no lead yet. Worth checking first whether they share a root
cause with each other (both are "a different gate than resources blocked
the move, despite the cost matching") before assuming they're two separate
bugs.

### Measurement, Fifteenth pass (colonize sacrifice) -- kept with its own section despite landing between two other passes' entries above

| | before this pass | after |
|---|---|---|
| mean rounds reached (of 19.27) | 9.45 | **9.98** |
| decisions in Age II or later | 30.8% | **34.7%** |
| games completed | 2 | **10** |
| `UnrecoverableHiddenInfo: colonization bid ...` | 121 | **53** |

(Measured against a tree that already had the concurrent Take/Pop passes
above landed, hence the different baseline from theirs.)

## Take/Bid handoff (this worker's assignment): what's fixed, what isn't, what to try next

Owned `IllegalMove: Take` and `IllegalMove: Bid` for one pass. Current
corpus numbers as of landing: **245 `IllegalMove: Take`, 55
`UnrecoverableHiddenInfo: colonization bid ...`, 17 `StuckPending: auction
decider ... not a forced pass`** (all three move around as OTHER buckets'
fixes land and push more games deeper into the journal -- re-measure before
trusting these).

**Bid -- landed, holding up:**
- 95→55 (now, after the Fifteenth-pass colonize-sacrifice fix reduced it
  further): a bidder's own hidden hand can legitimately contain a bonus
  card this file never observed (military-hand cards are only grounded
  once PLAYED, `replay_common.rs`'s own top doc comment). Reclassified
  from `IllegalMove` to `UnrecoverableHiddenInfo` via
  `bid_ceiling_mismatch` -- narrow, only fires for a genuine raise against
  the correctly-identified bidder that exceeds their own computed ceiling,
  so a real engine defect elsewhere still reports `IllegalMove`. This is
  NOT a fix (games still stop at the same line), just an honest label.
- REPLAYER BUG, fixed: `resolve_intervening` left a `Pending::Colonize` or
  `Pending::Auction` open across an actor boundary whenever `decider ==
  expected_actor` happened to also hold (it read that as "nothing to
  resolve," true for a political decision, false here). Both now drain
  unconditionally except when the upcoming line is genuinely that
  decider's own `Bid`/`Pass`. This is what took Take from 178 to 148 and
  is very likely also why Bid's `StuckPending: ... not a forced pass`
  bucket (17 games) exists at all -- it's the SAME "decider's only legal
  move never gets a logged click" shape as `Pending::Defense`'s forced
  0-defender `DefendDone`, just for `BidPass` specifically. Not
  independently re-verified against the corpus by this pass; worth a
  sub-histogram check before assuming it's all one thing.
- 8→? left open at handoff time: `interact::start_auction` sees force 0
  where a real bidder had force > 0, traced to the (now-fixed) colonize
  approximation eating units it shouldn't have. Should shrink a lot on its
  own now that the Fifteenth pass grounds colonize sacrifices from the
  journal -- re-measure before chasing this further.

**Take -- one REAL fix landed, one attempted fix REVERTED, majority still open:**
- REPLAYER BUG, fixed (same commit as the Bid fix above, `resolve_intervening`):
  72 of the original 178 were a `Pending::Colonize` left open blocking the
  SAME player's own next, unrelated action (their own Take, in this
  bucket's case).
- **Reverted same day, DO NOT re-attempt without new evidence**:
  `costs::take_gate`'s `hand_full` (`hand_size >= civil_hand_limit`) looked
  wrong from one angle -- of the "no `Take` offered at all" shape (151 of
  245 as of this writing), the large majority show `hand_civil_size ==
  civil_hand_limit` exactly at the failure point, zero counterexamples of
  hand exceeding the limit. That pattern is real and worth someone
  re-opening. But `docs/RULES_SPEC.md` §2.5 and multiple independent
  community sources read `>=` (block AT the limit, not only over it), and
  a wrong loosening of `legal_moves()` is worse than the stall it leaves:
  a bot would then play a move a real BGO game would have refused it, and
  self-play can't catch that because both sides would cheat identically.
  Two more likely explanations were proposed and NOT cleanly ruled out in
  the time available:
  1. **Hand size is inflated.** Structurally unlikely but not fully
     excluded: `p.hand_civil` is only ever pushed to at ONE production call
     site (`apply.rs`'s `take_card`, always behind a real observed `"takes
     X"` journal line) plus one net-zero same-turn identity swap
     (`replay_common.rs::correct_hand_family`) -- there is no filler
     mechanism for civil hand the way there is for military hand or row
     slots. Re-check this claim still holds before trusting it (a
     concurrent pass may have added a new push site).
  2. **`civil_hand_limit` under-counts the true CA total.** The live lead:
     a from-scratch Python cross-check (summing "uses N civil action"
     journal clauses per round, independent of this binary's own
     computation) kept producing apparent proof of undercounting, and each
     one dissolved on inspection into a script bug -- an unpaired `Take`/
     `PutBack` undo line, a military-unit build/upgrade mis-typed as
     civil, and (the one that didn't fully resolve before time ran out)
     the "Development of Civilization" event's one-time free civil action,
     which `costs::civil_life_ca_free`/`OneTimeDiscount` already models
     correctly in the real engine but a quick reimplementation did not.
     **Next step for whoever picks this up**: don't reimplement the rules
     in a side script again -- instrument `costs::take_gate`/`civil_hand_
     limit` directly (a debug print of every contributing `Stats` field,
     or a temporary `eprintln!` in `effects::compute`) against 3-5 of the
     boundary cases and hand-verify against the raw journal line by line,
     the way `docs/REPLAY.md`'s very first Taj Mahal trace did. That is
     slower per-case but the ONLY way to fully account for one-time
     discounts and ordered free actions without re-deriving `costs.rs`
     from scratch.
- 48 "a `Take` IS offered, just not for the attempted slot" (row-slot /
  cost-formula mismatch) and 46 "some other pending still blocks it" (a
  `Choice`/`Auction`/`Colonize` that reappeared, possibly a NEW pending
  kind from a concurrent pass, e.g. `TakeRow` seen in the current
  histogram) -- neither sub-bucket was investigated this pass at all.

## `IllegalMove: Develop` / `IllegalMove: PlayAction` handoff (this worker's
## assignment): one root cause independently confirmed (already fixed by a
## concurrent pass), one NEW un-fixed pattern found and characterized, no
## code fix landed this pass -- diagnostics only

Owned the two "science-payment shaped" buckets (the journal prints the
exact science paid on the develop/play line, so every failure names its own
expected-vs-computed pair) plus their handful of `ParserGap` siblings
(`Breakthrough (I)`/`(II)`, `Urban Growth (I)`/`(II)`/`(III)` "free-civil-
action options ... do not include" gaps). Corpus numbers as of landing:
**41 `IllegalMove: Develop`, 48 `IllegalMove: PlayAction`, 3 `ParserGap:
... free-civil-action options ... do not include Build { ... }`** (all
Urban Growth). Mean rounds 10.41, Age II+ 37.8%, 13/1011 complete --
**unchanged by this pass**: no engine/replayer fix was authored this
session (see "why" below). Re-measure before trusting the sub-bucket
percentages below; they were counted against this exact baseline.

**The dominant root cause this pass traced (Columbus) was ALREADY FIXED,
independently, by a concurrent pass on the `IllegalMove: Pop` bucket
(Fourteenth pass, item 4, this doc) before this session rebased onto it.**
This is worth recording anyway because the trace method and the exact
mechanism are useful precedent, and because it independently CONFIRMS that
fix's stated justification with a second, unrelated example:

- Traced game `7522302` line 160 (`Purple discovers Coal using
  Breakthrough Purple loses 7 science; Purple gets 2 science`, the exact
  `ParserGap: Breakthrough (I)` example this task's brief named) end to
  end: extended `REPLAY_DEBUG` to print every `Move::EndTurn`'s
  reconstructed science total against the journal's own `"N science (now
  M)"` running total (`trailing_now_science`, new helper -- ground truth
  BGO prints for free, every single turn, not just at the eventual spend
  that trips a shortfall many lines later) and to dump the acting player's
  full `techs` tableau with each slot's worker count on every applied move
  (`REPLAY_DEBUG_ALL`). Bisected a real, growing science deficit (computed
  3, true 8, by round 8) to one turn where the ONLY unaccounted event was
  `"Christopher Columbus discovers I / Developed Territory"` -- `Developed
  Territory (I)`'s own `immediateEffects.science: 3` (`card_table.rs`)
  exactly closes the gap. Root cause: this line has NO leading actor
  colour AND no trailing one either (unlike Alexander's death line), so
  `corpus::classify`'s old "a known card name leads the line ->
  Bookkeeping" catch-all silently swallowed it whole -- not just the
  science, the leader removal and the territory grant too. Confirmed fixed
  on this session's rebased tree: this exact `ParserGap` no longer appears
  in the corpus (`Move::ColumbusColonize` now dispatches correctly, reading
  the actor off the journal's own column-2 colour, `Line::color`, since
  it's the one line in the whole corpus that needs it).
- **Do not re-attempt this fix** -- it is `6179767`/Fourteenth-pass item 4
  on `origin/master` already.

**NEW, un-fixed pattern found this pass: 41/89 (46%) of the remaining
Develop/PlayAction failures are blocked by an open `Pending::Choice` that
`resolve_intervening` silently defers instead of resolving OR honestly
reporting.** Sub-histogram by pending kind, gathered by extending the
`try_apply` failure debug print (`REPLAY_DEBUG`) with the `pending_top`
already captured, over all 89 current failures:

| pending kind | count |
|---|---|
| `PlunderSplit` | 12 |
| `Raid` | 10 |
| `TakeRow` | 6 |
| `LosePop` | 6 |
| `LoseColony` | 3 |
| `FlipWonder` | 4 |

**Mechanism** (`replay_common.rs::resolve_intervening`): the function's
`GainBlock`/`FreeBuild`/`DiscardMilitary` cases are drained or matched
UNCONDITIONALLY, before the `decider == expected_actor` check even runs.
Every OTHER `Pending::Choice` kind is not -- it falls through to `if
decider == expected_actor { ...; return Ok(()); }`, which returns `Ok`
**regardless of what's still pending**, on the (correct, for a live
political decision, but NOT generally true) assumption that "decider
matches who's supposed to act next" means "nothing left to resolve." When
it doesn't hold -- a `PlunderSplit`/`Raid`/`TakeRow`/`LosePop`/
`LoseColony`/`FlipWonder` choice is open for the SAME player who is also
next up for an unrelated `Develop`/`PlayAction` line -- this returns `Ok`
anyway, `apply_one`'s `Develop`/`PlayAction` arms have no idea a `Choice`
is open (neither inspects `r.state.pending.top()` at all, unlike `Build`'s
own `FreeBuild` check), and the bare `Move::Develop`/`Move::PlayAction`
they then attempt is unconditionally illegal (`legal_moves()` only offers
`Choose` while a `Pending::Choice` sits open). Confirmed via
`REPLAY_DEBUG`: e.g. game `7522188` line 189 (`Orange discovers
Constitutional Monarchy`) fails with `pending_top=Some(Choice(Choice {
player: 0, kind: LosePop, ... }))` open for the SAME player who's
attempting the develop.

This is the SAME root-cause SHAPE as the Twelfth pass's fix
(`resolve_intervening` treating `decider == expected_actor` as "nothing
left to resolve" even with a live `Pending::Colonize`/`Pending::Auction`
open) -- just for six MORE pending kinds that fix never touched, and
surfacing here because Develop/PlayAction's own arms are exactly the ones
with no fallback handling for an unrelated open `Choice`.

**Why this pass did NOT land that fix**: `resolve_intervening` is the one
function every bucket's dispatch runs through -- changing its core
`decider == expected_actor` branch is cross-cutting by construction, and
correctly distinguishing "safe to defer" (the three cases already
unconditional above) from "must report `StuckPending` instead of silently
returning `Ok`" for six DIFFERENT pending kinds, each with its own
resolution shape and its own owning bucket (`LosePop`/`DestroyOwn` is
already partially wired for the `Pop`/`Destroy` buckets per the
Fourteenth pass; `Raid`/`TakeRow`/`FlipWonder`/`PlunderSplit`/`LoseColony`
are not), needs full-corpus re-verification across EVERY bucket, not just
this one -- more than this pass's remaining budget could responsibly
cover. **Also note this would very likely convert these 41 `IllegalMove`s
into 41 more `StuckPending: no auto-resolution for pending choice ...`s at
the SAME line -- an honest relabelling, not a depth improvement, unless
someone also implements the actual per-kind resolution** (reading the
choice off its own resolving journal line, the way `LosePop`+`Destroy` and
`FreeBuild`+`Build` already do). **Next step for whoever picks this up**:
pick ONE pending kind (probably `LosePop`, best understood -- see the
open lead below) and wire its resolution into `apply_one`'s `Develop`/
`PlayAction`/etc. arms the same way `Build`'s own `FreeBuild` check does,
rather than touching `resolve_intervening`'s shared dispatch at all.

**Open lead, NOT resolved: a `LosePop` pending in game `7522188` may be
targeting the WRONG player.** `Refugees` (`II` event, `Special::
WeakestPlayer`) resolved at that game's line 189 with `apply_single_target`
computing player 0 (Orange) as weakest by `RankStat::Strength` (values
`[8, 10]`, NOT a tie -- so this is unrelated to the Fourteenth pass's
`WeakestPlayer` tie-break fix, which only reverses ORDER among ties). The
journal's own text at that line ("Orange gains 3 culture and 1 population;
Purple loses 3 culture and 1 population") says PURPLE lost population, the
opposite of what this binary computed. Two explanations, NEITHER confirmed:
(1) this binary's own reconstructed strength for one of the two players has
drifted from the true value by round 12 (a REPLAYER bug, likely -- strength
depends on the whole built-unit history, easy to drift), or (2) `Refugees`'
targeting stat is genuinely not `RankStat::Strength` (an ENGINE bug, would
need checking against `sources/`' BGA implementation and RB p.15's exact
text before believing it over the current, deliberately-verified-elsewhere
`apply_single_target` machinery). **Do not assume either without checking**
-- this is exactly the shape of claim `docs/REPLAY.md`'s "verify a
documented impossibility" lesson warns about, just inverted (a claim
un-checked, not a claim wrongly believed). Reproduce via `REPLAY_DEBUG_ALL`
on game `7522188` and watch the `apply_single_target`/`rank_stat_value`
values leading up to line 189 (both now emit `REPLAY_DEBUG_ALL` traces,
added this pass) against a hand recount of Orange's and Purple's true
built-unit strength from the raw journal.

**Remaining 48 (pending_top=None -- no open Choice, a genuine legality/
affordability question) split further, by whether the target card prices
via `costs::tech_cost_net` (a real Government/tech `Move::Develop`) or not
(an `Action` card's own `Move::PlayAction`, which `tech_cost_net` always
reads as `None` since it isn't science-priced at all -- not itself a
finding)**:

- **23 `PlayAction` on an Action card, no pending, not investigated this
  pass.** `action_card_playable` (`legal.rs`) gates these on `free_action_
  moves` being non-empty for the card's own `FreeCivilActionValue` kind
  (Frugality → `Move::Pop` affordable, Engineering Genius → a wonder stage
  affordable, ...) OR `action_card_has_any_gain`. Worth checking whether
  these are civil-action-budget shaped too (see next bullet) before
  assuming a science/food-specific cause.
- **25 `Develop` on a Government/tech card, no pending.** Split cleanly by
  sign of `tech_cost_net(landed) - science`:
  - **16 of the 25: `civil_actions == 0` at the failure point, and science
    is NOT the blocker at all** -- e.g. game `7523347`, `Develop { card:
    Iron }`, `science=14` against a `tech_cost_net` of 5 (nine to spare),
    yet illegal because `civil_actions=0`. This is the ALREADY-DOCUMENTED,
    still-open civil-action-budget bucket from the Take/Bid handoff notes
    just above ("`civil_hand_limit` under-counts the true CA total" /
    "hand size is inflated" leads) -- these 16 are mislabeled "science
    shaped" by surface appearance only; the real cause is upstream CA
    tracking, out of this bucket's scope, and belongs with whoever picks
    up that handoff's own "next step."
  - **9 of the 25: a genuine small science shortfall (deficit of 1-4)
    with civil actions available** -- e.g. game `7522617`, `Develop {
    card: Monarchy }`, `science=7` against a cost of 8 (short exactly 1).
    Small, consistent deficits are exactly the Columbus bug's own shape
    (a missed one-off grant, not a formula error) -- worth the SAME
    per-game `REPLAY_DEBUG` end-turn science trace this pass used for
    Columbus, on each of these 9, before assuming they share one cause.
    Not traced this pass; flagged as the most promising remaining lead in
    this bucket.

**The 3 remaining `ParserGap: Urban Growth ... free-civil-action options
... do not include Build { ... }` cases are NOT a card-age resolution bug**
(this task brief's own top-priority check, per the "card-age ambiguity"
method note) -- `resolve_named_card_by_effect` already resolves the
correct age instance in all 3 (`Urban Growth (I)`/`(II)`/`(III)` each
correctly matched by the line's own printed resource discount). The real
gap: BGO phrases an urban-PRODUCTION-CHAIN upgrade (`Philosophy ->
Alchemy`, an already-built Lab card upgrading in place) as `"builds
Alchemy"`, the exact same verb it uses for a genuinely fresh build from
hand -- `corpus::classify_builds` always constructs `Move::Build`, never
considering `Move::Upgrade`, so when the target card is only reachable via
an in-place upgrade (not sitting in `hand_civil` at all -- confirmed via
the new `free_civil_action_move` gap debug: `hand_civil` for game
`7523200` names `["Bread and Circuses", "Justice System", "Breakthrough
(II)", "Cannon"]`, no `Alchemy` anywhere) the offered `FreeCivil` options
correctly include `Upgrade { from: Philosophy, to: Alchemy }` but the
constructed `wanted` move (`Build { card: Alchemy }`) never matches it.
Likely fix shape: when `free_civil_action_move`'s options list contains an
`Upgrade` targeting the same `landed_in_techs` card the `wanted` `Build`
was aimed at, prefer that reading over reporting a gap -- not attempted
this pass (only 3 games, low priority next to the 41-game pending-Choice
pattern above).

**Diagnostics added this pass (kept, all `REPLAY_DEBUG`/`REPLAY_DEBUG_ALL`-
gated, zero behaviour change, `cargo test --profile difftest --lib`: 1026
passed)**:
- `try_apply`'s existing failure print now also prints `science`, and for
  `Move::Develop`/`Move::PlayAction` specifically, whether the card is
  really in `hand_civil` and its `costs::tech_cost_net`.
- `trailing_now_science` (new helper) + an end-turn science cross-check
  against BGO's own printed running total, at the ONE real `EndTurn`
  dispatch site (`replay_game`'s own no-leading-colour special case, NOT
  `apply_one`'s -- that arm is dead code, `actor_and_rest` never matches an
  `"End turn ..."` line). **Known false-positive shape, read the doc
  comment before trusting a single instance**: a queued military discard
  defers `economy::end_of_turn`'s scoring step until `resume_end_turn`
  runs on a LATER line, so this check fires (and can misfire) before that
  resume completes whenever one is pending -- cross-check against a later
  reading (e.g. the next real spend) before concluding a real drift.
- `free_civil_action_move`'s own "options do not include" gap now dumps
  `science`, `hand_civil`, and `tech_cost_net(landed_in_techs)`.
- `economy::end_of_turn`'s uprising check now prints `s.science`,
  `s.happy`, `discontent`, `workers_free`, `uprising` (`REPLAY_DEBUG_ALL`).
- `events::apply_single_target` now prints `order`/`stat`/`ranked`/
  per-player `values` (`REPLAY_DEBUG_ALL`) -- `RankStat` gained `Debug`.

## Sixteenth pass: REPLAYER BUG -- `state.game_over` never flips on a clean
## replay (two compounding causes), and the final-score cross-check is wired
## into `replaystats`

The gap the Fourteenth pass left open, closed. By this pass's start the
corpus had grown to 12/1,011 sampled completions (a concurrent worker's
Take/Bid work); all 12 still had `state.game_over == false`. Two independent
REPLAYER bugs, both in `replay_common.rs`, neither in the engine:

1. **This binary's card row is grounded, not drawn.** `Replayer::
   ground_row_slot` forces each row slot to match the exact card an observed
   "takes ... in hand" line names, rather than drawing it through
   `civil_deck`/`game::deal`. Real `Take`s still refill through the normal
   engine path afterward, so the deck DOES shrink over a game -- but not
   reliably in step with the real one, and on every sampled completion this
   reconstruction's Age III deck never actually emptied. `game::advance_age`
   never reached its `nxt == Age::IV` branch, so its one call to
   `game::set_last_round` (the ONLY thing that ever sets `state.
   final_round_end`) never fired, and `game::advance_turn`'s round-wrap check
   -- the only path to `game::finish_game` short of every player resigning --
   had nothing to compare `state.round` against. BGO's own journal states the
   same §12.3 fact directly and unambiguously, in two lines `corpus::classify`
   was already dropping as pure flavour text: `"Last turn Game ends at the
   end of the starting round"`, one per surviving player, logged the instant
   Age IV begins. `replay_game`'s main loop now calls `game::set_last_round`
   (widened from `fn` to `pub(crate)` for this one call) directly when it
   sees that line, using this reconstruction's own -- at that point still
   accurate -- `current`/`round`/`start_player` to run the IDENTICAL formula
   `advance_age` would have. This is reading an authoritative fact the
   journal already states, not changing what the rule computes; `game.rs`'s
   own pre-existing test for the formula (`age_iv_sets_the_last_round_from_
   the_seat_that_triggered_it`) is untouched.

2. **The true final turn is logged twice, and the old code only handled the
   first copy.** BGO logs `"End of game ..."` BEFORE the last turn's own
   end-of-turn processing, not after: the final "Impact of `<Event>`"
   scoring lines and the actual `"End turn <Color> scores: ..."` line for
   the player whose turn ends the game both come AFTER the marker, and that
   `"End turn"` line (plus its discard/"No Discard Phase" follow-up) is
   itself printed TWICE, once mislabelled a round ahead (identical score
   deltas both times, confirmed on all 12 sampled completions). The old code
   just `break`s the instant it saw `"End of game"`, so none of this ever
   ran and `finish_game` was never reachable even in principle. Fixed in
   three parts, all in `replay_common.rs`: the main loop now `continue`s
   instead of `break`ing on the marker (letting the real trailing lines
   replay through the SAME `EndTurn` dispatch every ordinary end-of-turn
   already uses -- `classify` already resolves `"End of game"` and every
   `"Impact of ..."` line to `Bookkeeping`, so nothing extra was needed
   there); `resolve_intervening` now checks `self.state.game_over` at the
   top of its loop and returns `Ok(())` immediately once it is set (a queued
   discard drained mid-call can itself finish the turn and run
   `finish_game` as a side effect, which used to surface as a `decider !=
   expected_actor` `StuckPending` once `state.current` had already moved on
   to whoever's turn was next); and the `EndTurn` dispatch arm skips
   `try_apply` entirely once `state.game_over` is set (rather than trying to
   re-apply `Move::EndTurn` against a finished game, which `legal_moves`
   would legally and correctly reject). Each of the three pieces was
   confirmed load-bearing by reverting it alone and re-running the full
   corpus: without (1), only 3/1,011 games even reach the marker and 0 ever
   flip `game_over`; with (1) but not (2)/(3), 3/12 flip `game_over` and 9
   report a bogus `StuckPending` on the duplicate line; with all three,
   12/12.

### Measurement (`replaystats`, full 1,011-game corpus, measured against the
### tree this pass actually landed on -- rebased onto the concurrent
### colonize-sacrifice fix above, hence 13 completions here, not the 12 this
### pass's own three reverts-to-confirm were measured against just before)

| | before this pass | after |
|---|---|---|
| games completed (journal's own marker) | 13 | 13 (unchanged) |
| of those, `state.game_over` actually true | **0** | **13** |
| mean rounds reached / decisions in Age II+ | 10.37 / 37.8% | unchanged (confirms no other bucket regressed) |

(The revert-to-confirm numbers in points 1 and 2 above -- 3/1,011, 3/12,
12/12 -- were measured one commit earlier, before rebasing onto the
concurrent colonize-sacrifice fix; the shape they confirm is unchanged by
the rebase, only the corpus's total completion count is.)

### Final-score cross-check: wired into `replaystats`, first real numbers

`bin/replaystats.rs` now prints the same sorted-score-list comparison
`bin/replay.rs` already used per-game (neither side is known to line engine
seat `i` up with `index.tsv` column `i` -- `corpus::GameMeta::names` is
index.tsv's own column order, not seating order -- so an exact SORTED
multiset match is what "the reconstructed final scores agree with the real
ones" means here), plus a delta distribution for the games that don't:

**0/13 completed games matched exactly.** Delta distribution (engine minus
index.tsv, one entry per player per non-matching game, sorted): `[-27, -26,
-25, -20, -16, -14, -9, -9, -6, -5, -4, -3, -2, -2, -2, 0, 0, 3, 4, 4, 6, 7,
9, 9, 9, 20]` -- mean -3.81, spread on BOTH sides of zero (two exact 0s
sitting alongside a same-game nonzero partner, so even those aren't quite a
match), no single dominant magnitude or sign. Read together with how these
13 games got this far at all: NONE needed the colonize approximation any
more (`GameResult::colonize_approximated` -- the concurrent colonize-
sacrifice fix landed in between and closed that source out for this exact
set of games), yet only 3 of 739 military discards across them were
uniquely `solved`, the other 736 `chosen` arbitrarily among valid candidates
(`discard_solver`) -- so with colonize noise now gone, arbitrary military
discards read as the dominant remaining hidden-information approximation,
not a new scoring bug in `finish_game`/`events::evaluate_final_events` this
pass can point to: no game is close to an exact match, but none is wildly
further off than its own approximation load would predict either. Left for
whichever pass next reduces the discard-arbitrary-choice bucket (see
`docs/REPLAY.md`'s `discard_solver` section) -- the cleaner the
reconstructed hand, the more this comparison will actually test scoring
rather than hidden-info noise.

## Sixteenth pass: REPLAYER -- a logged bid IS evidence about the bidder's hidden hand

The rest of the colonization-bid bucket (55 games after the sacrifice fix
above). Every one had the same shape: a small shortfall (1-6) against a
bidder holding 3-8 unrevealed military cards. The `UnrecoverableHiddenInfo`
label was still reading the situation backwards.

§11.2 caps a bid at the bidder's own maximum colonization force, and BGO
enforces that cap in its own client -- a human cannot click a bid they could
not pay. So `"<Color> bids N"` is a JOURNAL FACT about a hand this binary
cannot see: their max force was at least `N`. It is public information,
shouted at the table, in exactly the sense a `"Defense card +6 played"`
clause is.

`Replayer::ground_bid_ceiling` converts SIMULATED filler in the bidder's
hand into military bonus cards until the ceiling clears, keeping the claim
as small as the fact allows:

- Cards are CONVERTED, never added. Hand size is modelled exactly (every
  draw and discard is logged); growing it to explain a bid would trade a
  known fact for a guess. Running out of filler is an honest failure.
- Never a card `DiscardSolver::needed_after` rules out -- an identity the
  journal later shows this player playing is one of the few hand slots that
  is not filler at all. Same predicate the forced-discard solver uses,
  called rather than re-implemented.
- Fewest cards, then smallest printed value.
- Never a bonus card newer than the military deck's own current age.

Reported, not swallowed: `GameResult::bid_ceilings_grounded` counts every
converted slot, and both `replay` and `replaystats` print it. Corpus-wide
that is **78 hand cards across 55 games**, against 152,073 recorded
decisions.

Two games survive as genuine contradictions -- no hand at all reaches the
logged bid. Both are downstream of an unrelated replayer gap outside this
pass's bucket: `"Barbarossa enlists a <Unit>"` (Frederick Barbarossa's
combined pop-increase-and-build, 425 lines across 135 corpus games) is
classified `Bookkeeping` and dropped in `corpus.rs`, so the unit is never
built and the bidder's army is short for the rest of the game.

### Measurement (`replaystats`, full 1,011-game corpus)

| | before this pass | after |
|---|---|---|
| mean rounds reached (of 19.27) | 10.41 | **10.55** |
| decisions in Age II or later | 37.8% | **38.8%** |
| `UnrecoverableHiddenInfo: colonization bid ...` | 55 | **2** |
## Six-pending-kind pass (dedicated owner, per-kind, incremental): `PlunderSplit`
## resolved -- REPLAYER, 10.41->10.53 mean rounds, 37.8%->38.4% Age II+, 13->14
## completed

Picked up the handoff two sections above: `resolve_intervening`'s
`decider == expected_actor` branch returns `Ok(())` **regardless of what's
still pending**, so `PlunderSplit`/`Raid`/`TakeRow`/`LosePop`/`LoseColony`/
`FlipWonder` were all silently treated as resolved even when a real decision
sat open. Doing this one kind at a time, each its own commit with its own
full-corpus before/after measurement (this project's standing rule for this
shared function).

**`PlunderSplit` (Aggression: Plunder's attacker-chosen food/resources
split) -- RESOLVED.** The resolving line is real and present in the corpus:
`"<Attacker> produces <N> food; <Attacker> produces <M> resources; <Victim>
spends <N> food; <Victim> spends <M> resources"` (either clause omitted
entirely when its amount is 0, singular `"1 resource"` vs plural `"N
resources"` both occur). `corpus::classify` files this whole shape as
`Bookkeeping` (correct for census purposes) and `replay_game`'s main loop
skips any `Bookkeeping` line outright -- so this evidence was never even
looked at, let alone used to resolve the choice. New `parse_plunder_split_
line`/`prescan_plunder_splits` (`replay_common.rs`) read it into a
per-attacker-seat FIFO, and `resolve_intervening` drains a
`Pending::Choice(PlunderSplit)` unconditionally (same tier as `GainBlock`/
`FreeBuild`/`DiscardMilitary`, before the `decider == expected_actor` check),
matching the popped `(food, resources)` against the choice's own `Gain`
options.

**Two traps found chasing this, both worth flagging for whoever does the
next kind:**
1. **A same-shaped-but-unrelated line exists and must NOT match.** Foray/
   Refugees' `Special::WeakestPlayers`/`StrongestPlayers` "and/or" grant
   (`events::food_or_resources`, sign > 0) prints the IDENTICAL `"<Color>
   produces X food; <Color> produces Y resources"` shape -- but it is a
   DETERMINISTIC computation (resources first, food for the remainder,
   capped by blue tokens), never a `Pending::Choice` at all, and critically
   never has a following victim `"spends"` clause (nothing is taken FROM
   anyone). That trailing `"; <OtherColor> spends "` is therefore the
   signature `parse_plunder_split_line` requires to tell the two apart --
   confirmed against real corpus lines of both shapes (game `7521158`'s
   Foray line has none; every Plunder resolution sampled does). Skipping
   this check would have meant occasionally feeding a Foray grant into a
   live PlunderSplit choice as if it were the attacker's answer.
2. **A single-option `PlunderSplit` never opens a `Pending` at all**
   (`interact::offer_plunder_split`'s `auto: true`, matching `push_choice`'s
   own auto-resolve-if-len-1 rule) -- but BGO still logs the resolving
   `"produces .../spends ..."` line for that deterministic outcome exactly
   like a real multi-option choice. A naive per-attacker FIFO popped
   strictly in journal order would therefore hand a LATER genuine choice the
   WRONG split whenever an earlier auto-resolved one for the same attacker
   sits ahead of it in the queue. Fixed by validating each popped entry
   against the live choice's own `options` and skipping (not trusting
   position) past any that don't match -- the entry belongs to an earlier
   silent auto-resolution the queue can't otherwise distinguish.

**Singular/plural bug caught by re-measuring, not by review**: the first
landed version used `tail.strip_prefix(" resource")` before checking the
plural `" resources"` -- since `"resources"` starts with `"resource"`, this
silently left a stray `"s"` glued onto the parse cursor and broke every
resources-valued split. Went undetected by `cargo test` (the unit tests only
covered singular/plural in isolation, never back-to-back parsing continuing
past a resources-clause) and only surfaced as a NEW `StuckPending` bucket
(48 occurrences) in the full-corpus measurement -- exactly why this
project's rule is measure the corpus, not trust the diff. Fixed and a
regression test added that parses a real multi-clause corpus line end to
end (`parse_plunder_split_line_reads_every_real_corpus_split_shape`).

**24 `StuckPending: PlunderSplit ... no journal-observed Plunder resolution
left` remain, and are legitimate, not a fixable gap in this parsing.**
Traced one to ground truth (game `7522629` line 186, single-game repro via a
one-line `index.tsv`): the journal's own resolving line is `"Purple produces
3 food; Purple produces 2 resources; Orange spends 3 food; Orange spends 2
resources"` (sums to 5, matching the card's printed "up to 5"), but this
binary's own reconstructed `Pending::Choice(PlunderSplit)` at that point
only offers options summing to 3 (`REPLAY_DEBUG_ALL`'s `resolve_intervening
loop` trace: `options: [Gain(food:0,res:3), Gain(food:1,res:2),
Gain(food:2,res:1), Gain(food:3,res:0)]`) -- the defender's own reconstructed
food+resources total has already drifted low by the time this Plunder
resolves, from some EARLIER, unrelated state-tracking gap (candidate: the
"Good Harvest" event a few lines earlier, `"Each civilization produces food
immediately"` -- not traced further, out of this kind's scope). Correctly
refusing to guess a split the choice doesn't actually offer (rather than
picking the closest option) turns what used to be silent, wrong corruption
into an honest, loud stop -- exactly the "honest relabelling" the prior
handoff predicted. **Do not "fix" this by loosening the option match** --
the real bug, if there is one, is upstream in food/resources tracking, not
in this choice's resolution.

Full-corpus (`replaystats`, 1011 games): **mean rounds 10.41 -> 10.53,
decisions in Age II+ 37.8% -> 38.4%, games completed 13 -> 14.** `IllegalMove:
Take` 245 -> 230, `Pop` 96 -> 88, `PlayAction` 48 -> 42, `Develop` 41 -> 38,
`Aggression` 25 -> 24 all dropped (real fixes, not just later stops);
`Build`/`WonderStep` rose slightly (90->107 combined-ish), consistent with
games reaching further and surfacing previously-unreached failures, not a
regression -- per this project's own "read mean rounds / Age II+ % /
completed, not raw counts" rule.

Remaining five kinds (`Raid`, `TakeRow`, `LosePop`, `LoseColony`,
`FlipWonder`) not yet attempted this pass -- see the six-kind table two
sections above for their journal-evidence status as of before this pass.

## HANDOFF (checkpoint, mid-pass): status of all six kinds, concrete next step

Forced checkpoint before this session's context ran out. Status of the six
`resolve_intervening`-deferred pending kinds this pass owns:

- **`PlunderSplit`: DONE** (see the section immediately above). Landed,
  measured, pushed (commit `b0f705a` plus two merge-fallout fixups for
  `Replayer::new`'s new `plunder_splits` parameter colliding with concurrent
  test additions, `29f5de0`/`9515dea`).
- **`Raid`, `TakeRow`, `LosePop`, `LoseColony`, `FlipWonder`: NOT started**
  this pass beyond the investigation already written up in the "NEW,
  un-fixed pattern" section above (the six-kind table, corpus counts as of
  before this pass: `Raid` 10, `TakeRow` 6, `LosePop` 6, `LoseColony` 3,
  `FlipWonder` 4, in the 89-game Develop/PlayAction sample -- true corpus
  totals are larger, see that section).

**Single most concrete next step: `TakeRow` is the cheapest remaining kind
and needs NO new prescan at all.** International Agreement's `QueueItem::
TakeRow` choice picks row slots one at a time (`ChoiceOption::Slot(u8)`,
plus a `Word(Stop)` to end early) and BGO logs each pick with the SAME
`"<Color> takes <Card> in hand <Color> uses N civil action"` text an
ordinary `Move::Take` uses -- so, exactly like `FreeBuild`'s existing
`matches_upcoming` pattern (`resolve_intervening`, right next to where
`PlunderSplit`'s new block now sits), the fix is:
1. In `resolve_intervening`'s unconditional block, add a `ChoiceKind::
   TakeRow { .. }` arm: if `decider == expected_actor`, `upcoming.0 ==
   ActionClass::TakeCard`, and `upcoming.1`'s card matches
   `self.state.card_row[slot]` for one of the choice's `Slot` options,
   `return Ok(())` (defer, like `FreeBuild`). Otherwise auto-select the
   `Word(Stop)` option and `continue` (also like `FreeBuild`'s `Skip`
   fallback) -- covers the `decider != expected_actor` StuckPending cases
   too, on the same "no journal trace for a silent decline" precedent
   already established for Politics-phase passes and `FreeBuild`.
2. In `apply_one`'s `ActionClass::TakeCard` arm, add a check for `Pending::
   Choice(TakeRow)` on top (mirroring the existing `DestroyOwn | LosePop`
   check in the `Destroy | Disband` arm): translate the observed card into
   `Move::Choose { n }` for the matching `Slot` option instead of a bare
   `Move::Take { slot }` (which is illegal while the pending sits open).
Find `ActionClass::TakeCard`'s current handling in `apply_one` before
starting -- not yet located this pass.

**Second most concrete: `Raid`, more work but well-understood.** Two
DIFFERENT journal shapes resolve it, neither prescanned yet:
- Terrorism event (`no_loot: true`): `"Terrorists destroy a <Color>
  <Building>"`, one line per victim, currently classified `Bookkeeping`
  and special-cased by name in `corpus::classify` (grep that string) --
  the victim's card is right there, just discarded today.
- Aggression: Raid card (`no_loot: false`, 1-2 `QueueItem::Raid`s per use,
  one per printed age tier): `"Raid casualties <N1> <Building1>[; <N2>
  <Building2>]; <Attacker> produces <M> resources"` -- currently
  `Unclassified`, also unused.
Plan (same shape as `PlunderSplit`'s fix): a GLOBAL (not per-player, since
Terrorism's line never names the attacker) `VecDeque<CardId>` prescan
reading both line shapes in journal order, drained with the SAME
validate-against-`c.options`-and-skip pattern `PlunderSplit` uses (a
single-candidate Raid choice also auto-resolves with no `Pending`, so the
same misalignment risk applies -- confirmed real for `PlunderSplit`, not
yet checked for `Raid` but assume it applies). The resource-gain amount
needs no separate parsing -- `resolve_choice`'s `ChoiceKind::Raid` arm
already computes it deterministically (`printed.div_ceil(2)`) as a side
effect of applying the right `Move::Choose`.

**`LosePop`: partially wired already** (`ActionClass::Destroy`'s `DestroyOwn
| LosePop` check, landed Fourteenth pass) -- only the `decider !=
expected_actor` gap remains (2 StuckPending in the baseline sample). Same
general shape as `Raid`'s Terrorism case: the resolving `"<Color> destroys
<Card>"` line exists but may be reached out of journal order relative to
whichever OTHER player's line `resolve_intervening` is currently trying to
reach. Needs the same kind of drain-with-lookahead-or-prescan treatment;
NOT yet designed this pass.

**`LoseColony`/`FlipWonder`: hardest, NOT recommended next.** Their
resolving text (`"<Territory> declares its independence from <Color>"` for
`LoseColony`; not yet even located for `FlipWonder`) is GLUED onto the same
journal line as the triggering event's own `"plays event"` preparation
line, resolved deep inside `resolve_political_decision`/`PrepareEvent`
machinery rather than as a freestanding later line -- a naive per-player
prescan FIFO has a worse version of `PlunderSplit`'s auto-resolve
misalignment problem here (an auto-resolved single-colony case's text is
indistinguishable in shape from a real multi-colony choice's, both glued to
the SAME kind of line, so validate-and-skip against `c.options` is the only
know fix and hasn't been designed for this shape yet). Do not start here
without first re-reading `resolve_political_decision`'s own code to see
whether `current_lineno`/raw text is already accessible at the point the
`LoseColony`/`FlipWonder` queue item resolves -- if so, resolving it
inline (from THAT call site, not a prescan) may be simpler than a FIFO.

Full corpus at this checkpoint (`PlunderSplit` fix + concurrent
`state.game_over`/Barbarossa-Bach/colonization-bid fixes, all pushed):
mean rounds reached and completions were last measured per-commit above;
re-run `replaystats` fresh before trusting a number here, none was taken
on this exact final rebased tree before the checkpoint.

## Seventeenth pass: the `7522625` corruption/blue-token lead re-localised to a YELLOW-BANK (population) drift, not blue tokens -- diagnosed but NOT fixed, checkpointed mid-trace

Picked up the open lead two sections above ("Open, with a concrete next
lead", `7522625`/Purple/round II8, `corruption(blue_available)==2` with no
journal `CORRUPTION!` line). Cut short by an infrastructure restart mid-trace
-- **no code was changed this pass** (tree is clean); this is a pure
diagnosis checkpoint, written so the next worker does not re-walk the same
40 minutes of log-reading.

### The corruption symptom is downstream of a FOOD/YELLOW_BANK drift, not a blue-token bug

`economy.rs::blue_used`/`blue_available`/`corruption` are all *derived* fresh
from `p.food`/`p.resources`/`p.wonder_steps`/`p.blue_total` on every call --
there is no separate "blue token ledger" to drift independently. So a wrong
`corruption` value at a given instant is *necessarily* downstream of a wrong
`p.food`/`p.resources`/`p.blue_total` at that instant, not a bug in the
derivation itself (confirmed correct, again, this pass by inspection of
`Denoms::of`/`tokens_for`/`blue_used`).

Traced `7522625` player Purple (idx=1, 2p game) round-by-round using
`REPLAY_DEBUG_ALL`'s `end_of_turn POST` line (`resources`/`food`/`science`/
`culture`, the values immediately after that turn's full production phase)
against the real journal's own `"<Color> scores:; ...; N food -
consumption: M (now Z)"` clause for every one of Purple's `End turn` lines,
rounds 1-8:

| round | sim POST food | real "now" food | match? |
|---|---|---|---|
| 1-6 | 2, 2, 4, 3, 4, 2 | 2, 2, 4, 3, 4, 2 | all match exactly |
| 7 | 3 | 2 | **first divergence: sim +1** |
| 8 | (corr=2 fires here) | (no CORRUPTION! line) | symptom reported by the original lead |

So the ACTUAL first divergence is round 7, one round before the corruption
symptom this file previously pointed at (round 8) -- the corruption mismatch
is a round-later *consequence* of the round-7 food drift, not itself where
the bug lives. `s.food` (this turn's computed production, from
`effects::state_stats`) is a flat, constant `2` in this binary's
reconstruction for ALL of Purple's rounds 1-8 (`REPLAY_DEBUG_ALL`'s
`blue_used`/pre-corruption prints all show `s.food=2`), which happens to be
right for rounds 1-3 (Purple's only farm the whole game is the 2-worker
starting `Agriculture`, `food_denoms=[1]` throughout -- confirmed no other
Farm tech is ever built, `Bronze`/`Alchemy`/etc are Mines/Labs) but the REAL
game's own production, reconstructed by hand from the journal's per-round
delta clauses, is **2, 2, 2, 1, 1, 1, 0, 0** -- genuinely DECREASING despite
Purple's population only ever growing and Agriculture never being
destroyed/upgraded/touched. This decrease is not explained by anything this
pass identified: no `Destroy`/`Upgrade` ever targets Agriculture, no
uprising fires (`REPLAY_DEBUG_ALL`'s uprising-check line shows
`uprising=false` every round), and `CardType::Farm` production is scaled
purely by `slot.workers` in `effects::compute` with no other modifier this
pass found. **This asymmetry (real production falls, sim production stays
flat) was not root-caused before the checkpoint -- it is the single most
concrete unexplained fact from this pass and the most promising next
thread**, more promising than the yellow_bank chase below because it's a
DIRECT read from the journal's own numbers, not an inference.

### A second, harder-to-explain thread: `yellow_bank` itself silently drops by 2 during the OPPONENT's turn, with no traceable call site

Cross-checking `consumption` (`economy::consumption(yellow_bank)`, bands documented
in `economy.rs`: `>=17:0, 13-16:1, 9-12:2, 5-8:3, 1-4:4, 0:6`) against the
real journal's own `"consumption: M"` clause gives a SECOND, independent
signal that should track `yellow_bank` exactly:

- Round 6 (both sim and real): `yellow_bank=14` (traced by hand-counting
  every real population-increasing event up to that point: start 18, -1
  round 2 `Take`... `increases population`, -1 round 4 `Development of
  Settlement` event (applies to BOTH players, confirmed via
  `events::resolve_event`'s `for &q in &order` loop -- this part of the
  engine IS correct), -1 round 4 Frugality free-civil pop, -1 round 6
  `increases population` = 18-4 = 14) -- `consumption(14)=1`, matches real
  `"consumption: 1"` for both rounds 5 and 6. **Sim's own `yellow_bank`
  matches this hand count exactly through round 6** (confirmed via
  `REPLAY_DEBUG_ALL`'s `uprising check` line, which prints `yellow_bank`
  directly).
- Round 7 (real): `consumption: 2` -- requires real `yellow_bank` in
  `[9,12]`, i.e. **at least 2 more decrements than the round-6 value of 14**,
  despite ZERO population-related lines anywhere in Purple's OR Orange's
  round-7 turn (checked the full journal text for both players' round 7:
  no `increases population`, no population-granting event, no war/aggression
  with a population effect). **Not explained.**
- Round 7 (sim): stays at 14 the whole of Purple's own round-7
  `end_of_turn` (confirmed via the `uprising check`/`pre-corruption` prints,
  both read `yellow_bank=14`) -- consistent with sim's own bookkeeping (no
  pop actions this round either), but this means sim's `consumption` used
  for round 7's production was `1`, not the real `2` -- **a second,
  independent confirmation that something is wrong by round 7**, corroborating
  the food-production divergence above rather than explaining it (a
  lower-than-real consumption number, on its own, would make sim's food
  HIGHER than real, which is the DIRECTION we see -- but the MAGNITUDE from
  consumption alone (extra 1 food kept) does not fully explain the round-8
  corruption trigger without ALSO the production-side drop `7522625` shows).
- **A separate, so-far-unexplained observation, possibly a RED HERRING**:
  sim's OWN `yellow_bank` for Purple (idx=1) jumps from 14 to 12 sometime
  during **Orange's** entire round-8 turn (Aggression: Raid against Purple
  [confirmed FAILS on the tie, `combat.rs::finish_aggression`'s
  `ctx.dfn >= ctx.atk` guard is correct and was independently re-verified
  this pass -- `dfn` reaches exactly `11 == atk`'s `11`, so no raid effect
  ever fires], Take, Develop, `PlayAction Urban Growth`, Take, EndTurn) --
  `REPLAY_DEBUG_ALL`'s per-move `applied mv=... yellow_bank=...` trace shows
  Purple's `yellow_bank` is unchanged (14) through Purple's own round-7
  `end_of_turn`, then reads 12 at the very FIRST line of Purple's round-8
  turn (`PolPass`), with no intervening move that should plausibly touch
  Purple's fields. Every `yellow_bank`-mutating call site in the codebase
  was enumerated (`grep -n "yellow_bank" src/*.rs`, excluding tests) and
  manually checked against Orange's round-8 move list -- none obviously
  fire: `economy::increase_population`/`lose_population` (no `Pop` move
  either player), `apply::grant_yellow` (no card/wonder/colony gain this
  turn), `interact::apply_card_gains`/`apply_immediate_effects` (no action
  card with a `population` `CardGains` field played, no colony gained),
  `interact::gain_colony`/`lose_colony` (no colonization this game yet by
  round 8), `events.rs`'s `take_yellow_tokens_from_weakest` (no such event
  card this game), `combat.rs`'s `War over Territory` yellow-token transfer
  (no war declared). **This was NOT resolved before the checkpoint** -- the
  next step is source-level instrumentation (a one-line `eprintln!` at
  every mutation site above, gated on `REPLAY_DEBUG_ALL`, naming the call
  site) rather than more manual log-reading, since exhaustive manual
  cross-referencing of `resolve_intervening`'s calls for the whole of
  Orange's round-8 turn did not surface an explanation. **Caveat**: this
  yellow_bank jump might itself be a second, unrelated symptom of whatever
  causes the food-production mismatch above (e.g. if `state_stats`
  recomputation after some hidden state change also perturbs something
  read alongside `yellow_bank` in a shared code path) rather than an
  independent bug -- not established either way.

### What is RULED OUT this pass (don't re-check these)

- `economy::corruption`'s bands and `blue_used`/`blue_available`'s
  derivation (re-confirmed correct by inspection, this pass and the prior
  one).
- `blue_total`'s flat starting value of 16 (`game.rs`) being wrong for this
  game specifically -- Purple's `blue_total` never changes all game
  (`REPLAY_DEBUG_ALL`'s `blue_used` prints show `blue_total=16` constant),
  and no card/wonder/colony this player has grants `blueTokens`.
- `Move::Destroy` (`h_destroy`, `apply.rs`) touching `p.resources`/`p.food`
  when a Farm/Mine is destroyed (`Purple destroys Bronze`, round 7) -- it
  only decrements `workers`/increments `workers_free`, confirmed by reading
  the function; the derived `blue_used` recomputation via `Denoms::of` after
  a destroy is a designed, correct consequence of the derivation-not-ledger
  model, not a bug.
- `combat.rs::finish_aggression`'s tie-handling (`dfn >= atk` returns false,
  i.e. defender wins ties) -- correct per the observed `atk=11 dfn=11` in
  this exact game, and the real journal independently confirms no building
  was destroyed that turn (no `"Orange destroys ..."` line follows the
  Raid).
- `events::resolve_event`'s `allPlayers` loop (`for &q in &order { apply_player_block(...) }`)
  correctly applies `Development of Settlement`'s population grant to BOTH
  players, not just the revealer -- verified by hand-counting round 4's
  yellow_bank change (17→16→15, matching sim exactly).
- Development of Civil Life / "Development of Civilization" -- not present
  anywhere in this game's journal (`grep`-confirmed), so Finding 1b/2's
  fixes are irrelevant to this specific trace.

### Concrete next step for whoever picks this back up

1. **Chase the food-production decline first** (2,2,2,1,1,1,0,0 real vs flat
   2 in sim) -- it is a DIRECT journal-derived fact, not an inference chain,
   and it's the most likely root cause: something removes an effective
   worker (or its production) from Purple's Agriculture over time in the
   real game that this binary's `effects::compute` never models. Candidates
   not yet checked: whether `Bronze`'s `Destroy` (round 7) or ANY other
   tableau change has a side effect on a DIFFERENT card's worker count in
   the real BGA engine that this port doesn't reproduce; whether corruption
   ITSELF (paid in resources first, food for shortfall, per `end_of_turn`
   step 3b) was actually firing in EARLIER rounds in the real game in a way
   that silently reduced food we're not crediting (re-check every prior
   round's real journal line for a `CORRUPTION!` clause this pass did not
   look for outside round 7-8); whether the printed "N food" in BGO's own
   `End turn` line is a NET figure (production minus something already
   subtracted) rather than gross production, which would invalidate this
   whole pass's arithmetic and needs checking against a SIMPLER example
   game with an isolated, unambiguous production number before trusting
   this diagnosis further.
2. **Instrument every `yellow_bank`-mutating call site** (list above) with a
   `REPLAY_DEBUG_ALL`-gated `eprintln!` naming the site, then rerun `replay`
   on `7522625` and grep for the FIRST such print between Purple's round-7
   `end_of_turn` and Purple's round-8 `PolPass` -- faster than more manual
   `resolve_intervening` log reading.
3. Do NOT assume the yellow_bank jump and the food-production decline are
   the same bug until one of them is actually root-caused -- treat them as
   two separate threads until proven otherwise.
4. The `Build: workers_free == 0` (22 games) and `Upgrade: ca/ma == 0`
   (~55 games) sub-buckets mentioned two sections up remain completely
   untouched -- still worth checking whether they share a cause with each
   other before assuming three separate bugs exist.

## Take/Bid handoff continued: gate-by-gate breakdown, one ENGINE bug fixed, HandFull left open (2026-08, urgent checkpoint)

Picked up the Take/Bid handoff above. Per its own advice, instrumented the
ENGINE directly instead of reimplementing take-cost rules in a script:
`costs::TakeRejection`/`costs::take_rejection` (new, mirrors
`can_take_gated`'s exact branch order, cross-checked by a test against every
existing `can_take*` fixture) names WHICH gate rejects a take, and
`replay_common.rs`'s `TakeCard` handler dumps it under `REPLAY_DEBUG`
(`DEBUG TAKE REJECT: card=... reason=... our_take_cost=... journal_cost=...
gate_have=... civil_actions=... hand_civil_size=... civil_hand_limit=...
hand_civil=[...]`). Run: `REPLAY_DEBUG=1 ./target/difftest/replaystats
sources/bgo/index.tsv /tmp/bgo-journals/journals 2>debug.txt`, then `grep -o
"reason=[A-Za-z]*" debug.txt | sort | uniq -c`.

**Gate breakdown of the 245 `IllegalMove: Take`** (before any fix this pass):
`HandFull` 157, `Budget` 37, `WonderInProgress` 3, `WonderBudget` 3 — leaving
45 with NO `DEBUG TAKE REJECT` line at all, meaning `take_rejection` says the
move IS legal by the take-gate's own logic and something else (a stale
pending/phase mismatch) blocks it instead — not yet investigated, matches the
old handoff's "some other pending still blocks it" sub-bucket.

**ENGINE BUG fixed, commit `24cb8bf`**: Rebellion's
`civil_actions_per_discontent_worker` handler in `events.rs::apply_extras`
was double-charging its own penalty — once via an immediate
`p.civil_actions -=`, and AGAIN a whole turn later via `p.ca_penalty_next_turn`
(consumed by `economy::end_of_turn`'s reset). Confirmed against game
7522661's raw journal, which literally prints "Purple loses 4 civil actions
on his next turn" — singular. Fix removes the redundant deferred write (kept
the immediate subtraction, which is provably correct: by the time this block
runs, every off-turn player's `p.civil_actions` already holds their
pre-loaded NEXT turn's allotment, since their own last `end_of_turn` reset
already ran). New test
`civil_actions_per_discontent_worker_costs_exactly_one_turn_not_two`,
confirmed red/green by reverting. Corpus effect: Take 245→239, the `Budget`
sub-bucket within it 37→19 (WonderBudget 3→0 too — same shape, wonders pay CA
same as everything else), mean rounds 10.41→10.57, Age II+ decisions
37.8%→38.8%, completed games 13→14. `ca_penalty_next_turn`/its
`economy::end_of_turn` consumer are now dead in practice (Rebellion was the
field's only writer, grepped `data/*.json` for
`civilActionsPerDiscontentWorker`) — left in place since the reset formula
is still correct machinery and removing the field is a bigger, riskier
mechanical change (~9 zero-initializer call sites); flagging for a future
cleanup pass rather than doing it under this checkpoint's time pressure.

**HandFull (157, the majority) — investigated, NOT fixed, explicit
instruction is not to guess here.** Hand-traced two full cases end-to-end
against raw journals with zero shortcuts (every hand card's `takes`/
build/discover/upgrade history individually verified present-or-absent, CA
total independently re-derived from government+wonder+tech card JSON, not
from the engine's own number):
- Game 7523354 line 219 (`Purple takes Air Forces`, the task's own example):
  hand_civil_size=6, civil_hand_limit=6 (Despotism 4 + Pyramids 1 + Code of
  Laws 1, confirmed built by line 98). All 6 hand cards individually
  journal-confirmed present with no build/discover between their `takes` and
  line 219.
- Game 7523073 line 127 (`Purple takes Cartography`): hand_civil_size=5,
  civil_hand_limit=5 (Despotism 4 + Library of Alexandria's `civilHandLimit:
  1`, a SEPARATE bonus from `civilActions` — confirmed via `costs::
  civil_hand_limit`'s existing two-field design). All 5 hand cards
  individually confirmed present, including catching my own transient
  tracing error (conflating this game's Irrigation/Alchemy history with the
  other example game) before trusting the result.

Both cases: hand == limit exactly, engine correctly computed both numbers
independently two different ways, and the REAL human still took a card,
directly contradicting `RULES_SPEC.md` §2.5/§6.7's `>=`-blocks reading. This
is the SAME shape the previous pass found in 70 games and could not fully
account for; now confirmed via full provenance tracing (not just symptom
counting) in 2/2 cases with zero counterexamples across the full 157.

**Explicit instruction from the task brief: do not change the `>=` gate
without new evidence, and even strong evidence is "a finding to report, not
a change to make" if it disagrees with the rulebook — so this fix was
deliberately NOT applied**, despite passing the bar the previous handoff
set ("dump how many hand cards are synthesised vs journal-observed, and our
CA total vs what the journal's own `uses N civil action` lines imply" — done,
for 2 cases, both clean). Flagging this up rather than shipping it.

**Concrete next step for whoever picks this up**: either (a) get explicit
sign-off to loosen `costs::take_gate`'s `hand_full` to strict `>` given this
now-doubled evidence (2 fully-traced, zero-counterexample cases plus the
previous pass's 70), scored over the full corpus before trusting it, or (b)
if the rule must stay `>=`, look for a THIRD explanation neither pass has
tested: BGO's own client might be buggy/lenient at exactly the boundary (a
real digital-implementation quirk, not a rulebook-vs-engine disagreement) —
in which case the correct fix is not a rule change at all but modeling BGO's
actual (possibly non-canonical) accepted behavior, which changes the
argument against loosening the gate. Either way, don't re-derive the CA math
from scratch — `costs::civil_hand_limit`'s two-field design (`civil_actions`
+ `civil_hand_limit` bonus, added but never combined inside `effects::
compute`) is correct and both traces above confirm it independently.

The remaining `Budget` (19), `WonderInProgress` (3), and the 45 "no
REJECT line" cases are unexamined this pass — re-run the `REPLAY_DEBUG`
grep above first since the counts have already moved once from this same
fix and may move again as other buckets land.

## Six-pending-kind pass, continued: `LosePop` resolved -- REPLAYER

Picked up the mid-pass checkpoint above (`PlunderSplit` done; `Raid`,
`TakeRow`, `LosePop`, `LoseColony`, `FlipWonder` still open). Took the
checkpoint's own advice and picked `LosePop` next (`DestroyOwn`-shaped,
already partially wired).

**The remaining gap was NOT what the checkpoint guessed.** It framed this as
"the resolving line exists but may be reached out of journal order relative
to whichever OTHER player's line `resolve_intervening` is currently trying
to reach" -- true, but the mechanism is more specific: a `LosePop` pending
for player D can open as a SIDE EFFECT of resolving a totally DIFFERENT
player's political decision (`resolve_political_decision`, called from this
function's own `None`/politics branch while catching up through outstanding
political turns before `expected_actor`'s own can proceed) -- e.g. an event
like Refugees/Pestilence that penalises "the weakest civilization", which
need not be whoever is currently deciding anything. Confirmed on two real
games: `7521344` (player 3's own political reveal opens a `LosePop` for
player 3 while player 1 is up next for an unrelated `Destroy` -- player 3's
own resolution, `"Grey destroys Religion"`, doesn't appear until several
journal lines later) and `7522639` (same shape, but `decider == expected_
actor` for the OWNER of the pending too -- the gap isn't only a
cross-player one: it also hit `expected_actor`'s own pending when their very
next line ISN'T the resolving destroy, e.g. `DevelopTechnology` intervenes
first).

**Fix**: new `prescan_lose_pop_destroys`/`Replayer::lose_pop_destroys`, a
per-actor FIFO of `(line index, card)` off every journal `"<Color> destroys
<Card>"` line (mirroring `prescan_gain_produces`/`prescan_plunder_splits`),
drained by a new `ChoiceKind::LosePop` arm in `resolve_intervening` --
added at the SAME unconditional tier as `GainBlock`/`PlunderSplit`/
`DiscardMilitary` (ahead of the `decider == expected_actor` shortcut, not
gated on it), with the SAME `matches_upcoming` escape hatch `DiscardMilitary`
uses (`c.player == expected_actor && upcoming.0 == ActionClass::Destroy`
defers to `apply_one`'s pre-existing `DestroyOwn | LosePop` check, unchanged)
and the SAME validate-against-`c.options`-and-skip pattern `PlunderSplit`
uses for a popped entry that doesn't match (this player's own unrelated,
separately-resolved voluntary destroy).

**New trap, not present in `PlunderSplit`'s shape**: unlike a Plunder
resolution line (`Bookkeeping`-classified, always skipped by the main
per-line loop) a `"destroys"` line is an ordinary `ActionClass::Destroy`
action line the main loop WILL translate again when its own pointer reaches
it -- draining the FIFO early, out of order, would double-apply the same
destroy the second time the main loop got there. Fixed with a new
`Replayer::claimed_destroy_lines: HashSet<usize>` (line INDEX, matching the
main loop's own `journal.iter().enumerate()` index, not `Line::lineno`),
recorded the instant an entry is actually consumed (not for skipped,
non-matching entries -- those still need their own normal in-order
processing later) and checked by the main loop exactly like the pre-existing
`putback_skips`, right next to it.

Two new tests (`resolve_intervening_drains_a_lose_pop_pending_open_for_a_
different_player_than_expected_actor`, `resolve_intervening_skips_a_lose_
pop_destroy_entry_that_does_not_match_the_live_choices_options`), both
confirmed red (`StuckPending: no auto-resolution for pending choice
LosePop`) with the new `ChoiceKind::LosePop` arm stubbed out to `if false &&
...`, then green again once restored.

**Full corpus (`replaystats`, 1011 games), measured immediately before and
after this fix, nothing else changed**: mean rounds reached 10.98 -> 10.90,
Age II+ decisions 41.8% -> 41.3%, completed games 17 -> 17 (unchanged). The
old `StuckPending: no auto-resolution for pending choice LosePop` bucket (4
games) is gone; a NEW, more specific `StuckPending: LosePop choice open for
player # but no journal-observed destroy line left to resolve it with`
bucket appeared (20 games). **This is the SAME "honest relabelling" trade-off
`PlunderSplit`'s own section documents, just running in the opposite
direction on the headline numbers**: previously, the `decider == expected_
actor` shortcut silently returned `Ok(())` for a `LosePop` pending whenever
the very next line WASN'T the matching destroy (most of these 20 cases),
letting the replay continue for a while on top of an incorrectly-cleared
pending before failing later, elsewhere, on a symptom far from the real
cause -- which is exactly why 20 is bigger than the previous pass's small
sample-based estimate of 2. Traced one (`7522639` line 116) by hand: the
`LosePop` choice opens with 6 real options (a genuine, unresolvable
ambiguity, not a single-option auto-resolve case) but the player who owns it
never has ANY `"destroys"` line anywhere later in this specific journal --
plausibly the SAME kind of upstream player-ranking drift the "Seventeenth
pass" section documents for an unrelated `yellow_bank`/food-production
chase (this game's own weakest/strongest computation may already be corrupted
by that separate, already-tracked bug by this point), not a gap in this
kind's own parsing. Not fixed here -- per this project's own precedent, an
honest stop with no fabricated guess is correct behavior, not a regression,
even though the corpus summary numbers move slightly the "wrong" way.

Remaining four kinds (`Raid`, `TakeRow`, `LoseColony`, `FlipWonder`) still
open -- see the checkpoint above for `TakeRow`'s and `Raid`'s own concrete
next-step notes, both unaffected by this change.

## Build/Upgrade/WonderStep handoff (this worker's assignment): two age-sibling
## card-identity bugs fixed (Patriotism, Reserves), one concrete unexplained
## mil_discount lead left for the next pass

Picked up the "Build/Upgrade/WonderStep cost-mismatch cluster" section
above. Its own baseline categorisation (`workers_free == 0` for Build,
`ca/ma == 0` for Upgrade) was re-derived fresh this pass by parsing
`REPLAY_DEBUG=1`'s existing `try_apply fail`/`cost detail` prints (no new ad
hoc script -- exactly the method the task brief asked for), fixing a
game-ID-attribution bug in the throwaway categoriser along the way (the
debug stream prints `DEBUG game=X` at the START of each game's block, not
the end -- pairing a fail line with the FOLLOWING `game=` line silently
attributes it to the WRONG game; only matters for picking single-game
repros, the aggregate category counts were unaffected).

### Current sub-bucket shape (measured fresh on the landed tree, full corpus)

| bucket | resource short by 1-2 | workers_free==0 (Build only) | ca/ma==0 | pending still open | other |
|---|---|---|---|---|---|
| Build (109) | 80 | 15 | 4 | 3 | ~7 |
| Upgrade (80) | 65 | -- | 7 | 5 | ~3 |
| WonderStep (81) | 73 | 1 | 1 | 5 | ~1 |

"Resource short by a small amount" (the same dominant shape the fourth/fifth
passes above already named) is still, by far, the majority in all three
buckets even after this pass's two fixes -- there is at least one more
unidentified contributing cause behind it (see "Concrete next lead" below).

### Two REPLAYER fixes, both the SAME shape: an age-sibling misidentified at
### take-time, never corrected before being PLAYED

`ActionClass::PlayActionCard`'s card-identity resolution already
cross-checked a played card's own printed magnitude against its age-siblings
for Frugality/Engineering Genius (via the `kind` match on
`Special::FreeCivilAction`) -- but two OTHER recurring action-card families
with the same "printed magnitude scales by age" shape had no such check,
because they carry no `FreeCivilAction` special to route through that match
at all:

- **Patriotism** (`resourcesForMilitaryUnits` 1/2/3/4 for age A/I/II/III).
  Commit `cfa9e64`. New `trailing_gets_military_resource` reads the "gets N
  military resource" clause (NOT the line's last "gets" clause -- a trailing
  "gets 1 military action" always follows it, so `rfind(" gets ")`,
  `trailing_gets_science`'s own approach, would grab the wrong number).
- **Reserves** (`Special::GainFoodOrResources`, 2/3/4 for age I/II/III).
  Commit `a59cca8`. Reused `trailing_produces`'s already-parsed magnitude
  (previously only used to resolve the food-vs-resources CHOICE, never the
  CARD identity) against `family_siblings`.

Both: `solved` stayed `None` before the fix (no `kind` match at all), so the
code fell back to trusting whatever `best_age_sibling` guessed at TAKE time
-- age-blind, gated only on `age_civil`, and simply wrong whenever the
row/deck actually dealt an OLDER-age copy. The wrong-age card's bonus then
either over- or under-credits `mil_discount`/`resources`/`food` by the
difference between the guessed and real age's printed magnitude, which
compounds turn over turn into a much-later `IllegalMove` (both confirmed
against real single-game repros, `7521776` for Patriotism). REPLAYER, not
ENGINE -- `legal.rs`/`apply.rs` already had the actual payment math right;
only the journal parser was crediting the wrong card's magnitude. Both fixes
have full-sentence tests in `replay_common.rs`'s `#[cfg(test)] mod tests`,
confirmed red/green by reverting.

**Every OTHER age-scaled action-card family was checked and is NOT affected**
(`python3` swept `data/*.json` for every action-card name with more than one
age-sibling whose `effects` differ): Rich Land, Urban Growth, Engineering
Genius, Efficient Upgrade, Breakthrough, Frugality all carry
`FreeCivilAction` and are already routed through the existing `kind` match
(or `resolve_named_card_by_effect`'s `Build`/`Upgrade`/`Develop` arms for the
"using <Card>" ordered-action shape). Two remaining unchecked families,
NOT investigated this pass (low prevalence, not chased for time): Cultural
Heritage (`gainScience`/`gainCulture` differ age A vs I) and Revolutionary
Idea (`gainScience` 4 vs 6, age II vs III) -- same shape, worth a five-minute
check with the same method if the resource-short cluster is still large
after the next lead below is chased.

### Measurement (`replaystats`, full 1,011-game corpus, both fixes + a
### concurrently-landed `LosePop` fix from another worker, all on this
### commit)

| | before this pass | after |
|---|---|---|
| mean rounds reached | 10.98 | 10.94* |
| games completed | 17 | 18 |
| `IllegalMove: Build` | 116 | 109 |
| `IllegalMove: Upgrade` | 87 | 80 |
| `IllegalMove: WonderStep` | 84 | 81 |

\* mean rounds dipped slightly rather than rising -- expected per this
file's own "closing one wall exposes the next" pattern (the concurrent
`LosePop` fix and this pass's fixes both let MORE games run deeper into
territory with OTHER, not-yet-fixed stops); read the raw `Build`/`Upgrade`/
`WonderStep` counts and completions, both of which did improve, not the
single rounds-reached average in isolation.

### What was ruled out this pass (don't re-chase these)

- **`workers_free == 0` (15/109 of Build) is a DIFFERENT bug from the
  resource-short shape**, not investigated to a fix this pass, but
  confirmed NOT to share `resources`/`mil_discount` tracking's own cause:
  traced one example (`7523355`, round 13, `Build{Religion}`) -- the
  player's `resources` figure was CORRECT and plentiful (13), only
  `workers_free` was 0. Farm/mine/unit `Move::Build` on an
  ALREADY-developed card adds ANOTHER worker each time it's called
  (`apply::do_build`'s `p.techs.get_mut(id).workers += 1; p.workers_free
  -= 1;` -- confirmed this is correct engine behaviour, not a bug: e.g. a
  tableau print showing `Bronzex3` for a player who only ever issued ONE
  journal `"builds Bronze"` line is CORRECT, not a drift -- every player
  starts the game with 2 workers already on Agriculture/Bronze,
  `game.rs`'s own `("Agriculture", 2), ("Bronze", 2)` starting setup, so
  1 explicit build = 2 starting + 1 = 3 total, matching the print exactly).
  Whether `workers_free` itself under-counts (missed a population increase
  somewhere) or over-spends (an extra phantom build/upgrade) was NOT
  determined -- no root cause found, just confirmed it's a worker-count
  question, not a currency-amount one.
- **The mysterious `yellow_bank` jump / food-production-decline lead from
  the "Seventeenth pass" section above (game `7522625`) was NOT re-picked-up
  this pass** -- deliberately left for whoever traces the NEW lead below
  first, since it's a much more concrete, narrower repro.

### Concrete next lead: an unexplained `mil_discount` grant with no
### journal-visible source (game `7521819`, round 4)

Cross-checking sim's own `end_of_turn POST` resources/food/science/culture
against the real journal's own `"N food - consumption: M (now Z)"` /
`"K resources (now R)"` clauses (this pass's method: parse BOTH into
per-player `(round, resources, food, science, culture)` sequences -- a
throwaway script, `/tmp/crosscheck.py` in the sandbox this pass ran in, NOT
committed; the shape is simple enough to rewrite in five minutes: regex
`^\S+\s+\S+\s+(color)\s+(age)\s+(round)\s+End turn ` for the row header,
then find `f"{color} scores:"` as a plain substring anywhere later on the
SAME line, since a leader's own "X scores N culture." clause can precede it
and breaks a single anchored regex) finds Orange's `resources` first
diverging (sim +1 high) at round 6 of `7521819`, one round AFTER a
suspicious journal line at round 4:

```
Orange builds Warrior Orange loses 1 military resource; Orange spends 1 resource
```

-- i.e. this build's cost was PARTLY paid via `mil_discount` (per this
file's own established reading: `total_paid_for_build`'s doc comment, "loses
N military resource" = `costs::spend_mil_discount`'s pool being spent) --
but grepping the WHOLE journal for `"Orange.*military resource"` finds NO
preceding `"Orange gets N military resource"` grant anywhere before this
line. Every currently-modeled source of `p.mil_discount` was checked and
ruled out for this game: no Patriotism/Wave of Nationalism/Military
Build-Up play by Orange before round 4 (`grep`-confirmed); Orange's leader
at this point is unelected (`"Orange elects Genghis Khan"` doesn't happen
until round 6, AFTER this build, and Genghis Khan's own ability
(`cultureIfTopTwoStrength`) is a culture bonus with no `mil_discount`
component anyway, `data/cards_wonders_leaders.json`); Frederick Barbarossa's
`comboResourceDiscount` is a different mechanism entirely (`Move::Barbarossa`
combo, not the `mil_discount` pool) and not this player's leader anyway.

**Two live hypotheses, NEITHER confirmed:**
1. A genuine parser gap: some OTHER card/event/mechanic grants
   `mil_discount` that this file's `card_gains_of`/`h_play_action` doesn't
   model at all (i.e. a THIRD data-driven source beyond
   `resourcesForMilitaryUnits`/Barbarossa's combo, not yet found in
   `data/*.json`'s `effects` keys -- this pass's `python3` sweep of
   `resourcesForMilitaryUnits` specifically found only Patriotism/Wave of
   Nationalism/Military Build-Up/Churchill, but did NOT sweep for a
   differently-named key that might cover the same mechanic under another
   spelling).
2. `p.mil_discount` is not actually being reset to 0 every end-of-turn for
   BOTH players correctly (`economy.rs`'s own `p.mil_discount = 0` inside
   `end_of_turn` -- confirm it runs for the RIGHT player index on every
   call site, not just the common case) -- i.e. a real ENGINE-facing bug in
   the reset rather than a replayer parsing gap, which would be a bigger
   deal and should be reported loudly, not quietly patched, per the task's
   own standing rule.

**Concrete next step**: instrument `costs::spend_mil_discount` and every
`mil_discount +=`/`mil_discount = 0` call site with a `REPLAY_DEBUG_ALL`-
gated `eprintln!` naming the site and the before/after value, rerun on
`7521819` alone, and read the trace between Orange's round-4 `PolPass` and
the `Build{Warriors}` line at journal line 44 -- should immediately show
whether `mil_discount` was already nonzero on entry to round 4 (a stale
carryover / reset bug) or freshly set by some code path this pass's `grep`
missed (an unmodeled grant). Whichever it is, this is the single most
promising next lead for the resource-short cluster: it is a DIRECT,
minimal, single-game repro (round 4, not many turns deep), unlike the
`7522625` food-production lead above which needed 6-7 rounds of hand-tracing
before this pass's own predecessor ran out of context chasing it.

### Not investigated this pass (still open, no lead)

- `Upgrade`'s `ma_zero`/`ca_zero` (7/80) and `WonderStep`'s `pending_open`
  (5/81, mostly a live `Raid`/other `Pending::Choice` blocking a later
  action -- likely the SAME shape as this file's existing `Raid` handoff
  two sections above, not re-examined here).
- Cultural Heritage / Revolutionary Idea age-sibling checks (see above --
  same method as Patriotism/Reserves, five minutes each, just not done).

## Take bucket handoff continued: gate-by-gate breakdown re-run, one more
## ENGINE bug found and fixed (Robespierre + Breakthrough revolution), the
## no-REJECT-line 37 fully explained (not mine to fix), HandFull still
## untouched per standing instruction

Re-ran the `REPLAY_DEBUG` gate breakdown against a fresh baseline (244
`IllegalMove: Take`, mean round 11.6): `HandFull` 180, `Budget` 24,
`WonderInProgress` 3, and 37 with no `DEBUG TAKE REJECT` line at all — i.e.
`costs::take_rejection` says the take IS legal and something else blocks
it.

**The 37 "no REJECT line" cases are fully explained, and are NOT this
bucket's to fix.** Correlating each one's preceding `DEBUG try_apply fail`
line (parse the `DEBUG game=` markers plus the try_apply-failure dump under
`REPLAY_DEBUG`, no side script needed) shows every single one has
`pending_top=Some(Choice(Choice { kind: Raid | LoseColony | FlipWonder |
TakeRow | Infiltrate, ... }))` open for the SAME player who's attempting the
Take. This is the exact same `resolve_intervening` root cause the
Develop/PlayAction handoff (above, this doc) already diagnosed in detail:
`decider == expected_actor` is read as "nothing left to resolve" even with
a live `Pending::Choice` open. Four of the five pending kinds
(`Raid`/`LoseColony`/`FlipWonder`/`TakeRow`) are explicitly another
worker's assignment per this task's brief; `Infiltrate` is a sixth kind not
in that list, found here, and should be folded into whoever owns that fix
since it's the identical mechanism. **Reported via `mcp__discord__
message_agent` rather than fixed here** — this bucket only fixes what's
actually a Take-shaped problem.

**`HandFull` (180) — NOT touched, per explicit standing instruction.**
Re-confirmed the shape holds at the new sample size: every `HandFull`
rejection has `hand_civil_size == civil_hand_limit` exactly (never over),
zero counterexamples. Did not re-derive the CA math from scratch or
re-trace individual cases this pass (two prior passes already did that
work, 2/2 clean); instead checked the STRUCTURAL sources a third time for
completeness:
- `civil_hand_limit`'s only bonus source in the card data
  (`civilHandLimit`) is Library of Alexandria; every OTHER contribution
  comes through `civil_actions`, which `effects::compute` builds from
  government + techs + wonders + leader + colonies + pacts + events, the
  same total `p.civil_actions` gets reset from every turn (`economy.rs`'s
  `end_of_turn`) — so hand limit and turn allotment share one source of
  truth by construction, not two that could drift.
  `p.hidden_civil` (the ONE other `hand_size_civil` contributor) is never
  written anywhere outside a zero-initializer — grepped every
  non-test/non-init site, confirmed still true.
- The free-civil-action-card hand-inflation bug the Ninth pass fixed
  (`free_civil_action_move`, Breakthrough/Rich Land/Urban Growth/Efficient
  Upgrade left in `hand_civil` after their own "using" line) is still
  landed and still the only hand-removal gap ever found; no new one
  turned up.
Still recommend NOT loosening `>=` to `>` without new evidence beyond what
two passes have already gathered (see the two explanations above this
section) — leaving this open for whoever next has bandwidth to do the
per-card provenance trace at a THIRD, larger sample, or to pursue the
"BGO client is lenient at the boundary" explanation the previous pass
raised (which would mean modeling BGO's own quirk, not changing the
rulebook-sourced gate).

**ENGINE BUG found and fixed, `Budget` 24 → 14 (Robespierre × Breakthrough
revolution): see the commit for full detail, this is the summary.** 10 of
the 24 `Budget` rejections had `leader=Maximilien Robespierre`; 9 of those
10 raw journals show the SAME shape — Robespierre revolts "using
Breakthrough" mid-turn, then a LATER civil action the journal records as
succeeding fails here at `civil_actions=0`, exactly 1 short. Traced game
`7523661` line 286 end-to-end (`REPLAY_DEBUG_ALL` plus a temporary
`eprintln!` inside `h_revolution`, since removed) and hand-verified every
civil-action cost in the turn against the raw journal text: total CA
budget after the revolution (Democracy 7 + Pyramids 1 = 8) came up exactly
1 short of what the turn's own actions needed (9), in EVERY case by
exactly 1, always in the same direction.

Root cause: `apply.rs::h_revolution`'s "only the pool that PAYS for the
revolution is emptied; the other behaves exactly as in a peaceful change"
logic (RB p.13) computes the unaffected pool's carry-over as `new_total -
(old_total - current_remaining)`. Under Robespierre, civil is that
unaffected pool. But when the revolution is funded via Breakthrough (RB
p.15's exception — `legal.rs::free_action_moves`'s `DevelopTechnology` arm
already has a comment flagging this exact subtlety), Breakthrough's own
`Move::PlayAction` has ALREADY spent 1 CA from that same civil pool by the
time `h_revolution` runs — and the formula has no way to distinguish that
1 CA (which RB p.15 treats as the revolution's own declaration cost, the
same role a bare revolution's full-pool wipe plays with no separate charge)
from ordinary this-turn spending, so it gets double-charged.

Fix: `h_revolution` now takes a `via_ordered_action: bool` (`apply_free_
civil_move`'s call passes `true`; the bare `Move::Revolution` dispatch
passes `false`). When true and the leader is Robespierre, `spent` is
reduced by 1 to back out Breakthrough's own pre-spent CA. Bare revolutions
are unaffected: RB p.15's own precondition ("all CAs must still be
available") means civil is always fully unspent going into a bare
revolution, so `spent` is already 0 and the compensation is a no-op — a
dedicated test (`h_revolution_bare_robespierre_still_gets_full_new_civil_
total_when_nothing_was_spent`) pins this. The non-Robespierre branch is
untouched (civil is unconditionally zeroed regardless of funding method,
matching RB p.13's base "you end with 0 available CAs this turn" — no
double-charge is possible there since nothing is being carried over).

Two tests in `apply.rs`, both CONFIRMED by reverting the fix and
re-running (`h_revolution_via_breakthrough_does_not_charge_robespierres_
civil_pool_for_breakthroughs_own_ca` fails 6≠7 without the fix; the bare
one still passes, confirming the fix is narrowly scoped).

**Measurement (`replaystats`, full 1,011-game corpus):** `IllegalMove:
Take` 244 → 236, mean round reached 10.98 → 11.02, decisions recorded
161637 → 162365, Age II+ share 41.8% → 42.1%. Full test suite: 1044
passed, 0 failed.

**What's left in `Budget` (14, down from 24):** the 10 Robespierre cases
should now be gone or reduced (re-measure before assuming zero — some may
have a second, independent shortfall further into the same game). The
remaining ~14 non-Robespierre `Budget` cases are unexamined — worth
checking whether they cluster around another leader/mechanic the same way
Robespierre did here, using the same method (grep `reason=Budget` in the
`REPLAY_DEBUG` trace, correlate to game IDs, hand-trace the raw journal's
own CA arithmetic for the turn).

**`WonderInProgress` (3): unexamined, small, next up if anyone has budget
left.**

## Six-pending-kind pass, continued: `TakeRow` resolved -- REPLAYER

Picked up the checkpoint's own "cheapest remaining kind, needs NO new
prescan" pointer, exactly as designed there: International Agreement's
`ChoiceKind::TakeRow { budget }` (`interact::offer_take_row`) picks
`card_row` slots one at a time, and BGO logs each pick with the SAME
`"<Color> takes <Card> in hand <Color> uses N civil action"` text an
ordinary `Move::Take` uses.

Two-part fix, both exactly as the checkpoint specified:
1. `resolve_intervening` gets a new `ChoiceKind::TakeRow` arm, same
   unconditional tier as `FreeBuild` right above it (and the SAME shape --
   `matches_upcoming` defers to `apply_one` when `c.player == expected_actor`
   and the upcoming `TakeCard` line's card is still among the choice's own
   `Slot` options; otherwise auto-select `Word(Stop)` and keep draining, the
   same "no journal trace for a silent decline" precedent already used for
   Politics-phase passes and `FreeBuild`). This one `Stop`-fallback covers
   BOTH the `decider == expected_actor`-but-different-action case AND the
   `decider != expected_actor` `StuckPending` case in one arm -- no
   lookahead/prescan needed, unlike `LosePop`.
2. `apply_one`'s `ActionClass::TakeCard` arm gets a new check (mirroring the
   pre-existing `DestroyOwn | LosePop` check in `Destroy | Disband`): if a
   `TakeRow` pending sits on top after `ground_row_slot` resolves the
   observed card to a slot, translate into `Move::Choose` naming that slot's
   option instead of a bare `Move::Take` (illegal while any pending sits
   open).

Three new tests (the `resolve_intervening` defer case, its `Stop` auto-
decline companion, and the `apply_one` `Choose`-not-bare-`Take` translation),
each confirmed red (with its own code path stubbed to `if false { ... }` /
`if false && ...`) before being restored green.

**Full corpus (`replaystats`, 1011 games), measured immediately before and
after, nothing else changed**: mean rounds 10.90 -> 10.95, Age II+ decisions
41.3% -> 41.6%, completed games 17 -> 17. The `StuckPending: no auto-
resolution for pending choice TakeRow` bucket (9 games at this pass's start)
is gone entirely -- unlike `LosePop`, no new `TakeRow`-specific bucket
appeared to replace it (consistent with there being no lookahead/prescan
step that could itself run out of evidence).

Remaining kinds after this fix: `Raid`, `LoseColony`, `FlipWonder` (per the
checkpoint above), plus a SIXTH kind, `Infiltrate`, flagged mid-pass by the
concurrent Take-bucket worker as sharing the identical `decider ==
expected_actor` gap -- see the section below.

## Six-pending-kind pass, continued: `Infiltrate` resolved -- REPLAYER
## (sixth kind, flagged mid-pass by the concurrent Take-bucket worker)

The Take-bucket worker reported (via `mcp__discord__message_agent`) that 37
of its own `IllegalMove: Take` bucket had a legal take blocked by an open
`Pending::Choice` of exactly this pass's kinds, for the SAME player
attempting the take -- and separately flagged a SIXTH kind sharing the
identical gap, `Infiltrate` (Aggression: Infiltrate, "remove your rival's
leader or incomplete wonder from play"), not in this pass's original
five-kind list. Folded in here rather than starting a new pass, per the
task's own "exhaustive match" preference.

**Journal evidence, more subtle than it first looked.** `"<Attacker> plays
Infiltrate against <Victim> ..."` is `ActionClass::PlayAggression`, handled
normally, but its resolution (which of the victim's leader/wonder is
removed) is glued onto a LATER, `Bookkeeping`-classified `"concedes
defeat"`/`"Operation successful"` line the main loop already skips outright
-- `resolve_aggression_defense` (called right after `Move::Aggression`)
only drains a live `Pending::Defense`, returning `Ok(())` immediately for
anything else, so an `Infiltrate` pending is left open exactly like
`PlunderSplit` was. Two real complications found by pairing every real
corpus `"plays Infiltrate against"` line with whatever resolves it,
downstream (a throwaway Python correlation script, not committed -- same
five-minute shape as this file's other ad hoc corpus checks):
1. **Two different LEADING phrases carry the identical trailing shape.**
   Usually the VICTIM's own line carries both: `"concedes defeat <Card> is
   killed; <Attacker> scores N culture"` (leader) or `"...is destroyed;
   ..."` (wonder). But when the victim has genuinely nothing to answer with
   (mirroring `Pending::Defense`'s own forced 0-defender shape), BGO splits
   it: a BARE `"concedes defeat"` from the victim (nothing to parse) is
   immediately followed by the ATTACKER's own `"Operation successful <Card>
   is killed/destroyed; <Attacker> scores N culture"` line carrying the
   real information. `parse_infiltrate_line` reads BOTH prefixes uniformly
   (checked unambiguous: no other line in the sampled corpus contains "is
   killed"/"is destroyed" at all, and nothing else ever leads with
   "Operation successful") -- the bare half simply parses to `None` and
   contributes nothing, which is correct: the split's actual information
   lives entirely on the OTHER line, wherever it lands.
2. **The same auto-resolve-contamination trap `PlunderSplit` already
   found.** A victim with only a leader OR only a wonder (not both) never
   opens a real `Pending` at all (`push_choice`'s own auto-resolve-if-len-1
   rule) -- but BGO still prints the identical resolving text for that
   deterministic outcome. Same fix: `resolve_intervening`'s new
   `ChoiceKind::Infiltrate` arm (added right after `PlunderSplit`, same
   unconditional tier, safe from double-consumption for the same reason --
   `Bookkeeping` lines are never separately consumed by the main loop)
   drains a per-attacker `infiltrates: HashMap<u8, VecDeque<bool>>` FIFO
   (`is_wonder`), validating each popped entry against the live choice's
   own options and skipping (not trusting position) past any that don't
   match.

Four parser tests plus two `resolve_intervening` tests (drain + skip-on-
mismatch, mirroring `PlunderSplit`'s own pair), all confirmed red (`if false
&& ...`) before being restored green.

**Full corpus (`replaystats`, 1011 games), measured on the SAME rebased tree
immediately before and after this fix (stash/pop, nothing else changed)**:
mean rounds 10.99 -> 11.01, Age II+ decisions 41.8% -> 42.0%, completed games
18 -> 19. The `StuckPending: no auto-resolution for pending choice Infiltrate`
occurrences are gone (folded into the Take bucket's own count before this
fix, per the concurrent worker's report -- not separately tracked in this
pass's own histograms before landing).

All SIX pending kinds `resolve_intervening` used to silently defer via the
`decider == expected_actor` shortcut are now resolved: `PlunderSplit`,
`LosePop`, `TakeRow`, `Infiltrate` (this pass) plus `Raid`/`LoseColony`/
`FlipWonder` still open -- see the HANDOFF section below for their status
and concrete next steps.

## HANDOFF (checkpoint): four of six pending kinds done, `Raid`/`LoseColony`/`FlipWonder` remain

Forced checkpoint (coordinator-requested, this session ran long). Status of
all six kinds `resolve_intervening`'s `decider == expected_actor` shortcut
used to silently defer, regardless of what was still pending:

- **`PlunderSplit`, `LosePop`, `TakeRow`, `Infiltrate`: DONE**, landed,
  measured, pushed. See their own sections above for the full detail; do not
  re-investigate these.
- **`Raid`, `LoseColony`, `FlipWonder`: NOT started this session** beyond
  the investigation already written up in the "Six-pending-kind pass:
  `PlunderSplit`... HANDOFF" section well above (search for "Second most
  concrete: `Raid`" and "`LoseColony`/`FlipWonder`: hardest" in this file) --
  that investigation is UNCHANGED and still the best starting point. Current
  corpus counts (`replaystats`, full 1011 games, on top of all four landed
  fixes): `Raid` 14, `FlipWonder` 4, `LoseColony` 3.

**New tool available for whoever picks these up**: `replaystats` now
supports `REPLAY_DUMP_BUCKET=<substring>` (env var) to print EVERY game/
line/text in a matching bucket to stderr, not just the histogram's one
example -- e.g. `REPLAY_DUMP_BUCKET=Raid ./target/difftest/replaystats
sources/bgo/index.tsv /tmp/bgo-journals/journals 2>&1 | grep '^DUMP'`. Used
throughout this session to correlate single-game repros; much faster than
re-deriving game IDs from `REPLAY_DEBUG_ALL` output by hand.

**Restated, so it isn't lost**: `Raid`'s two journal shapes (Terrorism
event's `"Terrorists destroy a <Color> <Building>"`, currently `Bookkeeping`
and special-cased by name in `corpus::classify`; Aggression: Raid card's
`"Raid casualties <N1> <Building1>[; <N2> <Building2>]; <Attacker> produces
<M> resources"`, currently `Unclassified`) both need a GLOBAL (not
per-player, Terrorism's line never names the attacker) prescan, same
validate-and-skip pattern as `PlunderSplit`/`LosePop`/`Infiltrate`'s FIFOs
-- by now there are FOUR worked examples of this exact pattern in this file
to copy from, `PlunderSplit`'s being the clearest first read. `LoseColony`/
`FlipWonder` are flagged "hardest, NOT recommended next" in that same
section -- their resolving text is glued INSIDE `resolve_political_
decision`/`PrepareEvent` machinery, not a freestanding later line, and no
one has yet re-read that call site to check whether it's simpler to resolve
inline from there instead of a prescan FIFO. Start with `Raid` first.

**Full corpus (`replaystats`, 1011 games) after all four landed fixes**:
mean rounds reached 11.03, decisions recorded 162453 (42.1% in Age II+),
completed games 19 (from the very first baseline this session took --
before ANY of this session's four fixes, on top of `PlunderSplit` alone --
10.98 mean rounds, 161637 decisions/41.8%, 17 completed).

**No ENGINE bugs found this session** -- all four kinds were pure replayer
gaps (the resolving journal evidence exists; this file just wasn't reading
or trusting it). Two ENGINE bugs landed in the meantime by OTHER concurrent
workers on this same shared function's neighbourhood (Robespierre/
Breakthrough revolution CA double-charge, `815b94f`; Homer's leader
discount applied per-build instead of once per turn, `ea372e7`) -- neither
touched by this pass, picked up only via `git rebase`.

**Cross-bucket causes reported, not fixed, per standing instruction**: the
Infiltrate discovery (37 Take-bucket failures, and the sixth kind itself)
was reported to the Take-bucket worker via `mcp__discord__message_agent`
rather than fixed by that worker directly -- it landed here instead, in this
function's own dedicated ownership, exactly as the standing "shared code
gets a dedicated owner" rule prescribes.

## Take/HandFull handoff, resolved: this reconstruction's civil age lagged
## the journal's own age column, delaying antiquation -- REPLAYER bug, fixed,
## `HandFull` 180 -> 78 corpus-wide

Picked up the `HandFull` (157 -> 180 across the last two handoffs above)
lead with an explicit brief to test hypothesis (a) -- "the reconstructed
hand is inflated" -- FIRST, since a filler-style bug was the favoured
explanation, before touching the `>=` gate again (still forbidden: see the
two sections above, both explicit that a previous worker reverted a `>=`
-> `>` loosening and it must not be re-attempted on corpus evidence alone).

**Re-verified the "no filler mechanism for civil hand" claim a FOURTH time,
this time at full-corpus scale rather than n=2, and it still holds, but for
a narrower reason than assumed.** Extended `costs::take_rejection`'s
existing `REPLAY_DEBUG` dump (`replay_common.rs`'s `TakeCard` handler,
commit `399b195`) with the journal line number and `r.state.age_civil` at
each rejected Take (both cheap, both already available on `Replayer`), then
wrote a throwaway correlation script -- **reading the engine's own printed
state and cross-referencing it against the raw journal text, NOT
reimplementing any game rule** (the exact trap the "Live lead" note two
sections up warns about) -- that checked, for every card in every rejected
`HandFull` hand, whether that same actor has an earlier develop/upgrade/
elect/play/revolution line for that EXACT card anywhere before the
rejection point. First pass falsely flagged 21 cases; every one dissolved
into either (a) a substring collision (`"Engineering"` the hand card
matching inside `"Engineering Genius"`, a different card entirely) or (b) a
wrong journal-line anchor -- naively matching the LAST occurrence of an
identical raw-text string (`"Purple takes Urban Growth in hand ..."`,
repeated verbatim across ages for the nine free-civil-action families) picks
the wrong physical occurrence when the same line text repeats. Anchoring on
the exact `lineno` the engine itself was AT when it rejected the Take (the
new debug field) instead of re-deriving position from text fixed this
completely: re-run across the full corpus, ZERO of the 180 `HandFull`
hands have a card with any earlier removal-shaped line for that actor. This
independently reconfirms, at n=180 instead of n=2, what the structural
"only one push site" argument already implied: nothing is pushing a card
into `hand_civil` that a real "takes" line didn't put there, and nothing
that plays/develops/upgrades/elects/revolts a card is failing to remove it
from `hand_civil` either.

**The actual gap was the OTHER removal path the task brief named: "the
age-change hand-limit discard".** Cross-referenced every `HandFull` hand's
cards against their OWN printed age (`data/cards_civil.json`) versus the
journal's own age column (`Line::age`, already parsed, unused for this
purpose before now) at the rejection line: 141 cards across 108 of the 180
games were more than one age older than the journal said the game currently
was -- e.g. an Age I `Swordsmen`/`Iron`/`Irrigation` tech still sitting in
hand while the journal's own column reads `III`. Per RB/CoL §12.2
(`game.rs::antiquate`, already correct and already used by self-play), a
card that old should already have been discarded at whichever age
transition it fell behind at. Since every `HandFull` rejection has
`hand_civil_size == civil_hand_limit` EXACTLY (both prior passes' finding,
re-confirmed again this pass with zero counterexamples), any ONE stale card
is sufficient to explain a wrongly-rejected Take.

**Root cause, traced end-to-end on `7523625` line 109 (`REPLAY_DEBUG_ALL`,
a temporary per-line `hand_civil_before` dump added to `apply_one`'s own
entry, kept -- see below): `game::advance_age`'s only trigger
(`civil_deck.is_empty()`, checked inside `deal`) fires LATE during replay,
because `Replayer::ground_row_slot` forces row identities to match each
observed "takes ... in hand" line directly rather than draining
`civil_deck` through the ordinary `deal` path in lockstep with every real
draw** -- already flagged, for the Age-IV/last-round special case only, by
`game.rs::set_last_round`'s own doc comment (which this pass's fix directly
mirrors). Any place a real draw can happen without this reconstruction
popping `civil_deck` to match (a `TakeRow` free take, a `PutBack`
client-side undo, ...) lets this reconstruction's own deck run behind the
true one's depletion -- and since `antiquate` only ever runs from inside
`advance_age`, a late-triggering age transition means a late hand cull, not
a wrong one: `7523625`'s Purple genuinely still held `Iron` un-discarded
several rounds after the real BGO client had already antiquated it away,
because this reconstruction's own `age_civil` hadn't caught up to `II` yet
at the point the rejection fired.

**Fix**: `game::force_civil_age_at_least(state, target)` (new,
`pub(crate)`, same visibility precedent as `set_last_round`) loops the
EXISTING `advance_age` until `state.age_civil >= target` -- antiquation,
the two-unborn-population deduction, and the full deck rebuild all run
exactly as a real deck-driven transition would, because it IS the same
function, not a re-derived approximation. A bounded loop, not a single
call: the journal can jump more than one age between two consecutive lines
this parser actually stops on (an entire age with none of its own cards
ever named in a line this file dispatches on), and every intervening age's
own antiquation must still fire, not just the final one's -- pinned by
`force_civil_age_at_least_antiquates_every_intervening_age_not_just_the_
final_one`, which starts a state at `Age::A` and targets `Age::III`
directly. Wired into `replay_common.rs`'s main loop at the top of every
line (`parse_age(line.age)`, new, matches every value the corpus actually
contains -- `cut -f3 *.tsv | sort -u` across all 1,011 journals is
literally `A`/`I`/`II`/`III`/`IV` plus the header, which never reaches
this code), reading the journal's own authoritative age fact instead of
approximating this reconstruction's own lagging deck-depletion timing --
the identical precedent `set_last_round` already established for §12.3's
last-round fact, just generalised to every age transition instead of only
the final one.

Two tests in `game.rs`
(`force_civil_age_at_least_antiquates_every_intervening_age_not_just_the_
final_one`, confirmed red -- `left: A, right: III` -- against a temporarily
stubbed no-op body before restoring the real implementation;
`force_civil_age_at_least_is_a_no_op_when_already_caught_up`, confirms no
double-antiquation and no backwards movement), plus `parse_age_*` in
`replay_common.rs`.

**REPLAYER, not ENGINE**: `advance_age`/`antiquate` were already correct
for self-play (a normal game's own `civil_deck` empties in real time, no
lag possible) -- only the replayer's OWN substitute trigger for calling
them was late. No path into `bots/` or the evaluator: `force_civil_age_at_
least` is `pub(crate)` with its only call site inside `replay_common.rs`.

### Measurement (`replaystats`, full 1,011-game corpus)

| | before this pass | after |
|---|---|---|
| `IllegalMove: Take` | 233 | **124** |
| `HandFull` gate rejections (`REPLAY_DEBUG` breakdown) | 180 | **78** |
| mean rounds reached | 10.98 | **11.01** |
| decisions recorded | 161,409 | **165,106** |
| Age II+ share | 41.8% | **43.1%** |

A second per-card age check re-run against the new 78-case baseline finds
**zero** remaining stale-age cards in any of them -- the fix's own
correctness claim (it closes exactly the stale-antiquation shape, not a
superset) is confirmed, not just asserted. The leftover 78 are the SAME
shape the previous two passes already traced and reported rather than
fixed: `hand_civil_size == civil_hand_limit` exactly, every card real and
current, `RULES_SPEC.md`'s `>=` gate blocking a move the real human still
made. **Still not this pass's to change** -- see the "Reverted same day"
and "explicit instruction is not to guess here" notes two sections above,
neither superseded by this pass's findings. The two remaining explanations
those sections leave open (a THIRD, larger-sample per-card provenance
trace, or the "BGO client is lenient at the boundary" theory) are both
still live for whoever picks this up next; this pass did not investigate
either.

**Side effects on the corpus summary, checked and NOT a regression**:
`state.game_over` completions dropped 18 -> 13 and the final-score delta
mean got worse (-3.53 -> -12.63). Diffed the two runs' completed-game ID
sets directly (`DEBUG completed:` under `REPLAY_DEBUG`) rather than
assuming: 15 games that used to complete no longer do, 10 new ones now do.
Hand-traced one regressed game (`7521984`) to its new stop point with a
temporary `src/bin/debug_one.rs` (built, used, deleted -- not committed):
it now stops at round 8 on an `IllegalMove: Upgrade` (`"Purple upgrades
Agriculture to Irrigation"`) with an ordinary-looking tableau (`Agriculturex1`,
`Irrigationx1`, cost detail matches the journal's own stated resources) --
no antiquated or stale card anywhere in sight, i.e. a PRE-EXISTING
`Upgrade`-bucket bug (not this pass's to fix) that this fix's own
downstream state-timing shift (every subsequent `civil_actions`/
`yellow_bank`/worker computation shifts once antiquation fires at the
CORRECT time instead of a lagging one) now exposes a few lines earlier in
this one game than before. This is the same "mean rounds can dip after a
correct fix, because games run deeper and surface a DIFFERENT bucket's
bug" effect this doc's own task brief warns about, just manifesting as an
earlier stop in one already-broken game rather than a later one -- not
evidence against this fix's own correctness, which the zero-remaining-
stale-cards check above verifies directly.

**Cross-bucket cause, reported via `mcp__discord__message_agent`, not
fixed here**: `best_age_sibling` (used to resolve which physical copy of a
same-name-across-ages card is meant, e.g. `Urban Growth (A)` vs `(I)` vs
`(II)` vs `(III)`) and any cost/legality path gated on `state.age_civil`
were ALL reading this same lagging value before this fix, not just the
`Take` gate -- so the `Build`/`Upgrade`/`WonderStep` cost-mismatch buckets
(explicitly another worker's assignment) plausibly share this exact root
cause for at least some of their own failures, now corrected as a side
effect of this fix landing. Worth a re-measurement of those buckets'
categorisation before assuming their existing counts/shapes still hold.

**Not investigated this pass, left for whoever picks up `HandFull` next**:
the remaining `Budget` (7, down from 14) and `WonderInProgress` (1, down
from 3) gate rejections within `IllegalMove: Take` -- both shrank as a
side effect of this fix (fewer stale-hand-blocked games means more games
reach these gates' own failure points and get past them too) but neither
was traced. The 36-ish "no `REJECT` line" sub-bucket (a stale/cross-actor
`Pending::Choice` blocking the Take, per the previous handoff's own
diagnosis) was also not re-measured this pass.

## Build/Upgrade/WonderStep handoff continued: an ENGINE bug found and fixed
## (Homer's discount had no per-turn cap), Cultural Heritage/Revolutionary
## Idea checked and fixed, cross-bucket re-verification against the
## age-lag fix, one new lead left (game 7521984)

Picked up the previous pass's own handoff two sections above: the
`mil_discount` lead (game `7521819` round 4/6) and the two unchecked
families (Cultural Heritage, Revolutionary Idea).

### The `mil_discount` lead: NOT a mil_discount bug at all -- resolved two
### different ways, one of them a real ENGINE bug

Instrumented every `mil_discount +=`/`-=`/`=0` call site
(`REPLAY_DEBUG_ALL`, `apply.rs`/`costs.rs`/`economy.rs`) and traced game
`7521819` round 4 first: the flagged line ("Orange builds Warrior Orange
loses 1 military resource; Orange spends 1 resource") turned out to be
fully explained by Homer (elected the line before) -- `costs.rs`'s own
`homer_unit_discount` doc comment already established BGO reuses the exact
same `"loses N military resource"` phrasing for Homer's discount as for
the Patriotism-style `mil_discount` pool. Not a `mil_discount` bug at all,
and not a bug of any kind at that specific line.

The REAL bug was one round later, round 6: Orange (still Homer-led)
upgrades Warrior->Swordsmen TWICE in the same turn. The journal shows the
FIRST upgrade discounted (`"loses 1 military resource"`) and the SECOND at
full price (`"spends 1 resource"`, no discount at all). `costs::
homer_unit_discount` had no per-turn cap -- it returned 1 for every single
unit build/upgrade, unconditionally, for as long as Homer was leader.
Confirmed against the leader's own official text
(`sources/bga_throughtheages_material.inc.php`): "On your turn, you have
an **extra 1 resource** for building and upgrading military units" -- an
extra 1 resource per turn, not per action. Swept the full 1,011-game
corpus for every turn with Homer active and 2+ same-turn unit
build/upgrade lines: 45 such turns, the `"loses N military resource"`
clause on AT MOST ONE of them every single time (35 on the first action;
10 where the first action was already free via an unrelated mechanism and
the discount showed up on the next one instead) -- 0 counterexamples.

**This is an ENGINE bug, not a replayer parsing gap** (unlike the
Patriotism/Reserves fixes above): `legal.rs`'s own affordability check
reads `costs::homer_unit_discount` directly, so self-play/training games
with Homer as leader were letting the bot double- (or triple-, ...) dip
this discount within a single turn -- something no real opponent's engine
permits. Fixed (commit `ea372e7`): a new `p.homer_used_this_turn` flag
(`state.rs`, same shape as `churchill_used`/`bach_upgrade_used`/
`ocean_liners_used` right above it), reset in `economy::end_of_turn`,
gating `homer_unit_discount`; `spend_homer_unit_discount` now takes `&mut
PlayerState` and sets the flag, but only when the discount actually
reduced a nonzero payment (matching the 10 "first action was already free"
corpus cases above -- a fully-free build must not burn the turn's
allowance). New tests confirmed red/green by reverting:
`do_build_homers_discount_applies_only_once_per_turn` and
`a_second_same_turn_unit_build_one_resource_short_is_illegal_once_homers_allowance_is_spent`
(`apply.rs`), plus `end_of_turn_resets_the_per_turn_state` extended.
Repro game `7521819` itself: was `IllegalMove: Build` at round 10 before
this fix; now runs to round 15 (a different, StuckPending stop in a
different bucket), confirming the fix in isolation, not just aggregate
counts.

### Cultural Heritage / Revolutionary Idea: same age-sibling shape,
### fixed (commit `4db14b7`)

Both flagged by the previous pass as unchecked. Neither has any `Special`
at all (`special: &[]`, plain `gainScience`/`gainCulture` `CardEffects`),
so neither routes through the existing `kind` match, and a wrong
take-time age guess (`best_age_sibling`, age-blind) would silently apply
the wrong age's science/culture gain when later played -- the SAME "no
kind match, `solved` stays `None`, trust whatever the take-time guess put
in hand" gap the Patriotism/Reserves fixes closed for their own families.
Fixed with a new `.or_else()` branch, gated on `base_name` (a bare
`gainScience` number has no self-gating signal the way Patriotism's
"military resource" text or Reserves' `Special` does, so an ungated
version could misfire on an unrelated card with a coincidentally-matching
"gets N science" clause), reusing the existing `trailing_gets_science`
helper (already used for Breakthrough's `Develop`/`Revolution` case) rather
than writing a new parser. Two new tests, both confirmed red/green by
reverting.

### Cross-bucket re-verification against the Take/HandFull worker's
### age-lag fix (`8edfea7`), per their own report

The Take/HandFull worker found `state.age_civil` was lagging real draws
and flagged that `best_age_sibling`/every age-gated cost path in THIS
bucket could share the root cause. Rebased onto their fix (clean rebase,
no conflicts) and re-verified rather than assumed:

- **Re-measured the bucket on the rebased tree**: `IllegalMove: Build` 67,
  `Upgrade` 55, `WonderStep` 56 (down sharply from this pass's own earlier
  109/80/81 baseline, taken before the age-lag fix landed -- consistent
  with their report that the lag was a large SHARED cause). Games
  completed 14 (down from 18, same drop they reported and explained: more
  games now run past the stale-hand block and surface OTHER, real,
  pre-existing bugs sooner).
- **Verified the Patriotism/Reserves fixes are still correct and still
  necessary**, not just "still passing their own unit tests": temporarily
  disabled both `.or_else()` clauses (`if false { ... } else { None }`,
  reverted after) and re-ran the full corpus on the age-fixed tree.
  Without them: Build 81, Upgrade 59, WonderStep 62. With them (current):
  67/55/56. That is -14/-4/-6 attributable to these two fixes SPECIFICALLY
  even with `age_civil` now correct -- confirms the age-lag fix and the
  take-time-guess fix are two independent bug classes that both produce a
  wrong age-sibling, exactly as flagged, and neither supersedes the other.
- **Re-ran the `mil_discount` lead's own repro game (`7521819`) on the
  rebased tree first**, before trusting the investigation above: it still
  needed the Homer fix (the lead did not evaporate under the age fix) --
  confirmed by re-tracing after rebasing, then landing the fix as
  described above.

### New lead, not chased this pass: game `7521984`'s `IllegalMove:
### Upgrade`, possibly an uprising misfire

The Take/HandFull worker's own handoff named this as the pre-existing
`Upgrade`-bucket bug their fix now exposes a few lines earlier (round 8,
`"Purple upgrades Agriculture to Irrigation"`, resources short: sim has 1,
journal implies the human had enough to spend 2). Traced one level
further this pass: sim's Purple has `resources=1` heading into round 8
because round 7's END-OF-TURN PRODUCTION step never ran --
`REPLAY_DEBUG_ALL`'s `uprising check` print shows `uprising=true`
(`discontent=2 > workers_free=1`, RULES_SPEC.md §6.3's own trigger), which
per `economy::end_of_turn`'s own step-2 skips the entire Production Phase
(score/corruption/production/consumption all skipped, RB p.24). But the
REAL journal's round-7 End Turn line for Purple shows completely NORMAL
production (`"2 food - consumption: 1 (now 6); 3 resources (now 4)"`) --
no uprising happened in the real game.

Two live hypotheses, NEITHER confirmed (ran out of pass budget chasing
this): (1) Purple's `happy`/`discontent` computation is wrong at this
point (an unmodeled happy source, or a stale worker-placement count --
same *shape* of bug as the age-lag fix, different field); or (2) upstream
of the uprising check, the SAME end-of-turn attempt opens a
`Pending::Choice(DiscardMilitary)` for Purple that the real journal does
NOT have (it says `"No Discard Phase"` at this exact point) -- the trace
shows this choice gets resolved via the discard-solver's "arbitrary" path
at journal line 107 with `decider=1 expected_actor=0` (an actor
MISMATCH), meaning it may be consuming a journal line that actually
belongs to Orange's turn, corrupting sync from there. Worth determining
which of the two by checking Purple's military hand size against the
journal's own count right before this point, and RE-READING (not
re-deriving) `discontent`'s inputs (`s.happy`, `p.yellow_bank`,
`p.workers_free`) against a hand-computed value from the real tableau.
Same instrumentation approach as this pass's `mil_discount` trace
(`REPLAY_DEBUG_ALL`, single-game `replay` binary) should work directly --
no new diagnostic facility needed.

### Measurement (`replaystats`, full 1,011-game corpus, cumulative state
### at the end of this pass, including the age-lag fix from the other
### worker)

| | before the age-lag fix | after age-lag fix + this pass's fixes |
|---|---|---|
| games completed | 18 | 14 (explained above, not a regression) |
| mean rounds reached | 10.94 | 11.07 |
| `IllegalMove: Build` | 109 | **67** |
| `IllegalMove: Upgrade` | 80 | **55** |
| `IllegalMove: WonderStep` | 81 | **56** |

## Civil-action-TOTAL question, resolved: NOT undercounted -- clean negative
## result, corpus-wide, the remaining 78 `HandFull` need a different cause

Owned the one open hypothesis the "Take/HandFull handoff, resolved" section
above left untested: not "is the hand-size CAP wrong relative to the total"
(both prior passes traced that structurally and found it correct), but "is
`costs::ca_total` itself undercounted" -- if so, the engine would be running
EVERY game, including self-play, with the wrong civil-action allotment, a
much bigger finding than a replayer gap.

**Method, per the task brief's own instruction not to reimplement game
rules in a side script**: journals record civil-action usage directly.
Every `TakeCard` line carries BGO's own explicit `"<Color> uses N civil
action"` clause, and a Take is NEVER a free action -- grepped exhaustively:
`legal::free_action_moves`'s `FreeActionKind` enum (what an action card like
Breakthrough can order for free) has no Take variant, and `civil_life_move`
(Development of Civilization's one-time discount) only ever offers
Pop/Build/Develop. So this printed `N` is unconditional ground truth for
civil actions a real human spent, independent of anything this
reconstruction computes -- summed since a player's last `EndTurn`, it is a
hard LOWER BOUND on their true civil-action total that turn. Instrumented
`replay_common.rs`'s main loop directly (new `civil_and_military_uses`/
`trailing_gets_civil_action` helpers, `REPLAY_DEBUG`-gated `CA_TOTAL_CHECK`/
`CA_TOTAL_UNDERCOUNT` prints) to compare this running sum against
`costs::ca_total` -- the exact function `costs::civil_hand_limit` (the
`HandFull` gate) is built from -- at every Take, across the full 1,011-game
corpus. No side script: this reads the engine's own already-classified
`ActionClass`/already-computed `costs::ca_total`, not a reimplementation.

**First pass found 20 apparent "undercounts", ALL false positives from the
check's OWN two confounds, not from `costs::ca_total`** -- exactly the trap
the task brief warned a previous side-script attempt fell into, now caught
by hand-tracing each one against the raw journal before trusting it:
1. **Hammurabi's once-per-turn MA-for-CA conversion.** A `TakeCard` line
   occasionally carries BOTH a civil AND a trailing military `"uses"` clause
   on the SAME line (`"... uses 1 civil action; ... uses 1 military
   action"`) -- not a take costing 2 combined action points, but the printed
   civil price paid out of the military pool instead. The naive combined
   sum (reusing the existing `total_action_cost` helper, which deliberately
   sums both) double-charged this every time; fixed by summing the two
   clauses SEPARATELY (`civil_and_military_uses`) and netting the converted
   amount out of the civil side. 19 of the 20 false positives were this
   shape (confirmed leader was Hammurabi at the flagged point in every one).
2. **Two in-turn refunds that top up the remaining POOL without changing
   the standing TOTAL**: §3 item 7's leader-replacement refund (`"<Color>
   elects <New> <Old> dies; <Color> gets 1 civil action"`, `apply.rs`'s own
   "Replacing a leader refunds one civil action" comment) and a client-side
   `PutBack` undo's refund. `costs::ca_total` is correctly blind to both (a
   refund isn't part of the standing total by rule, §3 item 7 and RB p.8's
   undo clause are both distinct FROM the total), so a naive running sum
   that never accounts for them over-counts. Netted via a new
   `trailing_gets_civil_action` parser triggered on `ElectLeader`/`PutBack`
   lines -- and the first version of THIS fix reintroduced 4 of the same 20
   false positives by flooring the running sum at 0 on each refund (losing
   a credit that arrived before any Take that turn, e.g. a leader replaced
   as the turn's very first action); removing the floor (the running sum is
   allowed to go negative -- a banked credit -- since only the sign of
   `spend - ca_total` is ever checked) fixed all 4.

**Final result, full 1,011-game corpus, 42,401 Take-cost data points
checked**: exactly **one** residual case (`7522905`, actor 3, round 1) out
of 42,401, and it dissolves too -- two same-named-but-different-instance
"Frugality" cards (different ages, same display string) were both taken
then one put back; `prescan_putback_skips`'s per-CARD-NAME stack (not
per-instance) pairs the `PutBack` with whichever Frugality was taken most
recently, which does not always match the one the refund amount actually
describes. This is a known, scoped simplification of THIS check's own
accumulator, not of the replayer's actual legality engine -- confirmed by
running the real replayer (not this side accumulator) over that exact
game/turn: it replays with ZERO mismatches, i.e. the engine's own
budget-enforced `p.civil_actions` (which handles the same PutBack pairing
through its established, tested mechanism, not my simplified text sum) had
no problem affording every take in that turn.

**Conclusion: `costs::ca_total` is NOT undercounted, anywhere in the
corpus, by this measurement.** This eliminates the last untested hypothesis
for the 78 remaining `HandFull` rejections (`hand_civil_size ==
civil_hand_limit` exactly, human still took a card, `RULES_SPEC.md`'s `>=`
gate blocking a real move) -- of the two explanations the "Take/HandFull
handoff, resolved" section left open (a third, larger-sample provenance
trace, or "BGO's own client is lenient at the boundary"), this result
weighs AGAINST the provenance-trace direction (three passes now, hand size
and both total-computation paths are independently confirmed correct) and
FOR either the BGO-leniency theory or a fourth explanation nobody has
proposed yet. Still not this pass's call to make -- the `>=` gate itself
remains untouched, per every prior handoff's explicit instruction.

**Two side fixes landed as a direct result of doing this measurement
properly (both required to even GET a full-corpus number, not scope
creep)**:
- **REPLAYER robustness, not a rules or cost change**: `REPLAY_DEBUG_ALL`'s
  own `WonderStep` diagnostic (`replay_common.rs`, right where it names
  WHICH move was illegal) called `costs::wonder_stage_cost` unconditionally
  to print its cost, even for an attempted `WonderStep` with NO wonder in
  progress -- exactly the shape of the illegal move it was describing. That
  function's own `debug_assert!(!p.wonder.is_none())` then aborted the
  WHOLE `replaystats` process (`panic = "abort"`, inherited from
  `[profile.release]` by `[profile.difftest]` -- `catch_unwind` cannot
  recover an abort-strategy panic, confirmed by trying it in
  `bin/replaystats.rs` and reverting when it didn't help), discarding every
  bucket's count for every game after the panicking one. Found via two real
  games, `7522899` and `7521762`, both mid-corpus. Fixed by guarding the
  cost call with the SAME `p.wonder.is_none()` check the very next line
  already used for the name -- both games now replay past the point that
  used to crash (confirmed individually, `REPLAY_DEBUG_ALL=1` against each
  in isolation, exit 0 either way). This is not the `WonderStep` cost
  bucket's own bug (that bucket, `IllegalMove: WonderStep`, is unaffected
  in shape or count -- only the earlier PANIC blocking measurement is
  fixed); flagging in case the WonderStep bucket's own owner wants the two
  repro game IDs.
- New tests: `civil_and_military_uses_*` (3), `trailing_gets_civil_action_*`
  (3), covering both confounds above and their real corpus text shapes.

**Full corpus (`replaystats`, 1,011 games, now runs clean to completion
with no exclusions needed)**: `IllegalMove: Take` 121, `HandFull` 79,
`Budget` 8, `WonderInProgress` 1 -- all within noise of the pre-existing
124/78/7/1 baseline (the `WonderStep`-diagnostic fix lets a couple more
games run a little further before hitting their own next, unrelated stop).
Full test suite: 1,071+ passed, 0 failed.

## HANDOFF: all seven pending kinds done -- Raid/LoseColony/FlipWonder landed, one small residual Raid sub-bucket left

Picked up the "four of six done, `Raid`/`LoseColony`/`FlipWonder` remain"
checkpoint above. **All three are now landed, tested, and pushed** (commits
`866b95e` Raid + the exhaustive-match refactor, `dfca327` LoseColony +
FlipWonder). Do not re-investigate these three kinds from scratch.

**The exhaustive-match refactor, landed alongside Raid**: `resolve_
intervening`'s old shape -- a chain of `if matches!(c.kind, X) { ... }`
checks for the unconditionally-drained kinds, falling through at the bottom
to a generic `Some(Pending::Choice(c)) => StuckPending("no auto-resolution
...")` -- is now a single `match c.kind { ... }` over every `ChoiceKind`
variant, with NO wildcard arm. Every kind not yet given real handling
(`FreeCivil`/`FoodOrRes`/`DestroyOwn`/`Annex`/`PactOffer`/`WarTech`) is
listed EXPLICITLY as its own no-op arm (behaviour identical to before --
they still fall through to the `decider == expected_actor` shortcut or the
bottom match's `PactOffer` arm, exactly as they always did). The point: a
FUTURE eighth `ChoiceKind` variant now fails this match at COMPILE time
instead of silently inheriting a catch-all's behaviour -- exactly the shape
that let all seven kinds this multi-session pass worked through go quietly
unresolved for as long as they did (nothing ever failed loudly when a kind
went unhandled; `StuckPending` only fired once a game actually reached that
pending). If a future ChoiceKind variant is added and this file fails to
build, that is this refactor working as intended -- add its own arm (even a
`{}` no-op with a doc comment explaining why) rather than reaching for `_`.

**`Raid`** (Aggression: Raid card, and the Terrorism event's identically-
shaped forced destruction): both journal shapes -- Terrorism's `"Terrorists
destroy a <Color> <Building>"` (`Bookkeeping`, the card discarded until
now) and Aggression: Raid's own `"Raid casualties 1 <Building>[; 1
<Building>]; <Attacker> produces <M> resources"` (previously
`Unclassified`) -- feed a GLOBAL (not per-player, since Terrorism never
names an attacker) `VecDeque<CardId>` prescan (`prescan_raid_destroys`),
drained with the same validate-against-`c.options`-and-skip pattern
`PlunderSplit`/`Infiltrate`/`LosePop` already use. One correction needed
mid-session, caught by its own unit test: `longest_known_card_prefix`'s
matched span swallows a glued-on trailing `;` (it's part of the same
whitespace-delimited word as the card name, e.g. `"Alchemy;"`), so the
remainder after each casualty clause starts with a bare space, not `"; "` --
the doc comment on `parse_raid_casualties_line` spells this out so the next
reader doesn't re-derive it.

**`Raid`'s residual 2-game sub-bucket, NOT fixed, flagged not force-fit**:
after landing the above, `replaystats` still shows 2 games (was 14 before
this pass) with `StuckPending: Raid choice open for player # but no
journal-observed Raid/Terrorism destroy line left to resolve it with`.
Sampled both (`7522790`, `7522608`) via `REPLAY_DUMP_BUCKET`. Two different
symptoms, neither understood well enough to fix without more evidence:
- `7522608` line 333: `"Raid casualties 1 Journalism; 1 Drama; Orange
  spends 6 resource"` -- note **"spends"**, not "produces". Every other
  sampled `"Raid casualties"` line in the corpus (hundreds) reads
  `"<Attacker> produces <M> resources"`. `RULES_SPEC.md` 5.5 is explicit
  that a Raid attacker always GAINS resources (never spends) -- so this is
  either a BGO logging quirk unique to this line, or this line is not
  actually the Raid's own resolution at all (maybe an unrelated coincidence
  glued on by BGO's own log formatting). `parse_raid_casualties_line`
  correctly returns `None` for this shape rather than guessing --
  RULEBOOK BEATS CORPUS, do not loosen the parser to accept "spends" without
  first understanding why this one line differs from every other sample.
- The OTHER residual game showed a tied Aggression-defense strength (e.g.
  `"Purple strength: 4; Orange strength: 4"`) with NO following `"Attack
  fails"` or `"Raid casualties"` line at all -- unlike a losing margin,
  which DOES get an explicit `"Attack fails ..."` line (confirmed on
  `7523809`). A tie may resolve as a forced, single-legal-option "attack
  fails" the same way `Pending::Auction`'s forced `BidPass` and
  `Pending::Defense`'s forced 0-defender case do (no journal trace for a
  deterministic outcome) -- but this hasn't been confirmed against
  `RULES_SPEC.md`'s own tie-breaking rule for Aggression strength
  comparisons, and PARKED's "whether an open pending should block the whole
  table" is adjacent territory. Worth a fresh look with `RULES_SPEC.md`
  open, not a guess.

**`LoseColony`/`FlipWonder`**: the earlier handoff's "hardest, NOT
recommended next" verdict turned out to apply only to the AUTO-RESOLVED
single-candidate case (glued onto the triggering `"plays event"` line,
`push_choice`'s own auto-resolve-if-len-1 rule) -- re-checking against the
real corpus this session (not re-deriving from theory) found that the REAL,
multi-candidate choice each event opens resolves on its own separate,
clean, freestanding later line the earlier passes never looked for:
- `LoseColony`: `"<Color> loses <Territory family> (<Age numeral>)"`, e.g.
  `"Purple loses Historic Territory (I)"` -- and that printed string is
  already the EXACT full card `name` `build_card_index` keys by (territory
  cards are the one family whose `name` bakes the age suffix straight in),
  so no roman-numeral parsing was needed at all, contrary to what the
  earlier handoff assumed would be required.
- `FlipWonder`: `"Ravages of Time <Wonder> crumble(s)"`, no leading colour
  in the text at all -- `Line::color` (column 2) is the only place the
  actor is, the same shape `ColumbusColonize` already established a
  precedent for reading. A leading `"The "` in the flavour text (present
  for some wonders, absent for others, e.g. `"St. Peter's Basilica"`) has
  to be stripped before `longest_known_card_prefix` runs, or the dictionary
  lookup fails outright (`"The"` alone is never a card name).

Both drain a per-actor FIFO, validated against the live choice's own
options and skipped exactly like `Raid`/`PlunderSplit`/`Infiltrate`/
`LosePop` -- confirmed by `grep` over the full corpus that neither
standalone line shape ever carries a trailing `;` continuation or collides
with its own auto-resolved shape, so in practice the skip path is
defensive here rather than load-bearing (unlike `PlunderSplit`, where it
fires for real). `LoseColony`/`FlipWonder` StuckPending are both fully gone
from the histogram, 0 residual games for either.

**Ten new tests this pass** (two resolve/skip pairs each for Raid/
LoseColony/FlipWonder, plus one parser test per new line shape), all in
`replay_common.rs`'s `#[cfg(test)] mod tests`, all CONFIRMED red (temporarily
neutered the relevant match arm(s) to `{}` no-ops, reran, saw the generic
`StuckPending`) before being restored green.

**Full corpus measurements, isolated per commit (same rebased tree,
immediately before/after, nothing else changed)**:
- Exhaustive match + `Raid`: mean rounds 11.03 -> 11.16, decisions 162453
  -> 166586 (42.1% -> 43.3% Age II+), completed 19 -> 30 (this was measured
  BEFORE a concurrent rebase landed unrelated fixes from other workers).
- `LoseColony` + `FlipWonder`, measured AFTER that same rebase (so the
  "before" number here already includes the concurrent work, isolating just
  this commit's own delta): mean rounds 11.26 -> 11.27, decisions 171395 ->
  172808 (44.9% -> 45.4% Age II+), completed 23 -> 24.
- **Do not naively diff the first "after" (30 completed) against the second
  "before" (23 completed) and conclude a regression** -- that gap is
  concurrent, unrelated work (three other workers' fixes, including the
  `wonder_stage_cost` panic fix documented in the section just above this
  one) landing via `git rebase` in between the two measurements, not
  anything this pass's own commits touched. Each commit's own isolated
  before/after (using the SAME tree, same-fix-neutered-vs-not) is the number
  that means something.
- **Current state, full corpus, this exact landed tree**: 24 completed
  games, mean rounds 11.27, 172808 decisions (45.4% Age II+). The `Raid`
  StuckPending bucket is down to 2 games (see above); `LoseColony`/
  `FlipWonder` are both at 0.

**No ENGINE bugs found by this pass.** All three kinds (`Raid`,
`LoseColony`, `FlipWonder`) were pure replayer gaps, same shape as the four
kinds landed earlier this multi-session pass -- the resolving journal
evidence existed the whole time, `resolve_intervening` just wasn't reading
or trusting it yet. The one live rule question this pass surfaced (`Raid`'s
"spends" vs "produces" line, above) is flagged for a rulebook-literate
follow-up, not resolved either way -- do not assume it's an engine bug or a
parser bug without more evidence; it could be neither (a BGO client
quirk).

**Ruled out, so it isn't re-tried**: a roman-numeral parser for `LoseColony`
(the printed line already carries the exact aged card name, see above); any
attempt to accept `Raid`'s "spends 6 resource" line as if it meant
"produces" (contradicts `RULES_SPEC.md` 5.5's explicit rule that the
attacker only ever gains).

## Actor-mismatch handoff: `WinWar` fixed (216 -> ~171), the dominant
## remaining sub-shape (discard-triggered, ~130 games) traced to a real
## hand-SIZE-inflating grounding bug -- diagnosed, a fix attempted and
## REVERTED after corpus-wide regression, two more distinct root causes
## found in the sibling `PlunderSplit`/`LosePop` buckets, neither fixed

Picked up the largest and deepest bucket in the whole corpus:
`StuckPending: decider != expected actor ..., phase Actions, no pending`
(216 games, mean stop round 13.5 -- deep into Age II/III), plus the three
buckets flagged as plausibly the same defect seen from the other side:
`PlunderSplit` (32), `LosePop` (24), `DestroyOwn` (5). **They are NOT one
bug** -- the unifying hypothesis was tested early and explicitly falsified;
this pass found at least THREE distinct, unrelated root causes across the
four buckets, one of them fixed, two diagnosed but deliberately left open.
Read the "Method" section before re-deriving any of this from scratch.

### Fixed and landed: `WinWar` needed `is_pure_confirmation_line`, exactly like `WinAuction`

`game::start_turn`'s own doc says war resolution (`combat::
resolve_war_outcome`/`apply_war_spoils`) fires automatically at the START
of the ATTACKER'S NEXT TURN -- not from the journal's `"<Color> wins War
over ..."` line, which `apply_one`'s `WinWar` arm already treats as a bare
`Ok(())` "validation checkpoint only". `resolve_intervening` was still
being called for that line, sending `expected_actor` to the line's named
WINNER (attacker or defender, whichever the strength favoured) while
`decider()` was still mid a completely different player's turn, with
nothing pending to explain the gap.

Confirmed on real game `7523809` line 342: `"Orange wins War over
Culture"` carries the IDENTICAL timestamp as, and is printed one line
BEFORE, an unrelated Purple's own trailing `"End turn"` line, with no
`EndTurn` in between -- the exact "not stably ordered within a second"
artifact already documented and fixed for `WinAuction`/Taj Mahal. Added
`WinWar` to `is_pure_confirmation_line` (commit `0207194`), mirroring the
`WinAuction` fix exactly. Two new tests in `replay_common.rs` (one pins
the exact confirmation-line set, confirmed red with `WinWar` removed then
restored green; one documents `resolve_intervening`'s own `StuckPending`
if ever called directly for this line shape).

**Measurement, isolated to this one commit** (before any other change this
pass): the bucket drops **216 -> 167** (59 of the 216 shared this exact
shape); completed games **24 -> 28**; mean rounds 11.27 -> 11.34; decisions
172808 -> 174551 (45.4% -> 45.9% Age II+). Clean win, no regression by any
measure (completed count only went UP). **REPLAYER bug, not ENGINE**:
`game::start_turn`'s own war-resolution timing was already correct; only
this reconstruction's journal-line interpretation was wrong.

A concurrent worker's unrelated fix (`deea9a0`, Foray/Raiders) landed via
rebase mid-pass; **current landed-tree numbers, this exact commit
(`dfc0299`)**: 1011 games, 29 completed, mean rounds 11.48, 177645
decisions (46.7% Age II+), actor-mismatch bucket at **171**, `PlunderSplit`
at 41, `LosePop` at 26, `DestroyOwn` at 5.

### Method: instrumented the engine directly, per the standing instruction

Two new `REPLAY_DEBUG_ALL`-gated prints, both landed (commit `dfc0299`,
pure diagnostics, zero behaviour change, safe to keep):
- `interact::discard_excess_military`: prints `idx`, hand length, the
  computed limit and its two summands, and the hand's own contents, every
  time it is called (not just when a discard actually fires).
- `economy::end_of_turn`'s draw-military step: prints `idx`, unused
  military actions, and cards actually drawn.

These, plus the pre-existing `REPLAY_DEBUG_ALL` trace already covering
`resolve_intervening`'s loop (expected actor, decider, upcoming line,
pending top) and `resolve_one_discard_choice`'s own pick, were enough to
fully trace every finding below without writing a single standalone
script. `REPLAY_DUMP_BUCKET` (substring-matched against the bucket key)
plus `comm` on two sorted game-ID lists was the tool used to confirm the
reverted fix's regression was real, not noise -- see below.

### Stall-signature distribution built for the 216-game actor-mismatch bucket (pre-`WinWar`-fix)

`REPLAY_DUMP_BUCKET="phase Actions, no pending"`, then grouping the raw
journal text of the stalling line by its leading verb:

| count | shape | outcome this pass |
|---|---|---|
| 87 | `"<Color> discards 1 card"` | root-caused (hand-SIZE drift), NOT fixed -- see below |
| 59 | `"<Color> wins War over ..."` | **FIXED** (`WinWar` confirmation-line, above) |
| 34 | `"<Color> discards 2 cards"` | same root cause as the 87, above |
| 12 | `"<Color> disbands <Unit>"` | not investigated this pass |
| 10 | `"<Color> discards 3 cards"` | same root cause as the 87 |
| 6 | `"<Color> destroys <Card>"` | not investigated this pass |
| ~8 | assorted singles (`Pop`, `Develop`, 1 discards-5) | not investigated this pass |

The discard-shaped rows (87+34+10+1 = 132 of the original 216, before
`WinWar`) are the single largest sub-signature in the whole bucket -- far
bigger than `WinWar`'s 59. They are STILL the largest chunk of the
post-fix 171.

### The unifying hypothesis, tested and falsified

The brief's hypothesis was that all four assigned buckets share ONE root
cause, the same shape as the seven already-fixed `ChoiceKind`s: "a choice
opens as a side effect of a different player's action, and the resolving
journal line is read out of order." Traced one concrete example from each
of the three still-open buckets (`7523355` for the discard shape,
`7523350` for `PlunderSplit`, `7522639` for `LosePop`) with
`REPLAY_DEBUG_ALL`. **All three are real, but they are three DIFFERENT
bugs**, in three different subsystems:

1. **Discard shape (largest, ~130 games): `Replayer::ground_military_hand`
   inflates hand SIZE.** Diagnosed in full, fix attempted, REVERTED. See
   its own section below.
2. **`PlunderSplit` (41 games): a FOOD/RESOURCE economy drift, not a
   military-hand or ordering bug at all.** See its own section below.
3. **`LosePop` (26 games), at least on the one game sampled: an EVENT
   CONDITION evaluation mismatch (`Barbarians`'s "weakest civilization"
   tie-break), not a turn-order or grounding bug.** See its own section
   below -- UPDATE: confirmed as a genuine ENGINE BUG and fixed in a later
   pass (root cause 3's own section now reflects this); not the sole
   cause of the whole 26-game bucket, just the one sampled game's cause
   and two sibling cards (`Raiders`, `Crime Wave`) sharing the same
   selection code.

None of these three is "a resolution line read out of order" -- the
`resolve_intervening` machinery this multi-session pass has been fixing
kind-by-kind is not where any of them live. **Do not keep hunting for a
fourth `ChoiceKind`-shaped fix here** -- the remaining work is in hand
tracking, resource tracking, and event-condition evaluation respectively.

### Root cause 1 (diagnosed, fix REVERTED): `ground_military_hand` grows hand SIZE instead of replacing filler

`Replayer::ground_military_hand(actor, card)` (`replay_common.rs`,
`~1474`) is called the instant a player's military-hand card identity is
first revealed by `DeclareWar`/`PlayAggression`/`ProposePact`/
`PlayTactic`/`ColumbusColonize`. Before this pass it was a bare:

```rust
if !hand.contains(card) { hand.push(card); }
```

This ADDS a new slot instead of REPLACING one of `new_game`'s SIMULATED
filler cards that, by construction, is not this real identity -- the true
hand never changed size, only which identity sits in one of its slots.
`ground_bid_ceiling` (a sibling grounder, for bonus cards specifically)
already established the correct pattern for this exact problem, with its
own doc AND a dedicated test (`a_logged_bid_is_taken_as_proof_the_bidder_
held_the_force_to_pay_it`, asserting `"hand SIZE is modelled exactly and
must not grow"`): find a "filler" victim already in hand (not needed for
a later observed play, per `DiscardSolver::needed_after`), `remove_first`
it, then `push` the real identity. `ground_military_hand` was the one
remaining grounder that still just pushed.

**Confirmed mechanism, traced on real game `7523355`** (`REPLAY_DEBUG_ALL`
on `discard_excess_military`'s new print, cross-checked against every
`"Purple draws/discards"` line in the raw journal by hand):
- Draw counts are NOT the cause -- ruled out first. Every one of this
  game's `economy::end_of_turn` draw-step counts (the new `draw_military_
  step` print) matches the journal's own printed `"draws N military
  cards"` EXACTLY, every single round from 2 through 12. The military
  draw step is not where the drift comes from.
- The true Purple never discards before round 10 (`"No Discard Phase"` on
  every earlier round's own line, explicit both ways -- BGO never leaves
  a discard silent, unlike `FreeBuild`/`GainBlock`'s "decline leaves no
  trace" precedent). This reconstruction's Purple hits its own computed
  `military_hand_limit` and force-discards as early as round 4 -- SIX
  rounds before the real game ever needed to.
- Because `resolve_intervening`'s `ChoiceKind::DiscardMilitary` "anything
  else" branch drains ANY open discard unconditionally, using
  `DiscardSolver`'s own ARBITRARY "Chosen" pick (there is no journal line
  it is reading to justify WHEN a discard should fire, only WHICH card,
  by design -- see the `discard_solver` module doc), these premature
  pendings get silently resolved at journal lines that are not discard
  lines AT ALL: `"discard: player 1 line 147 picked ... (Chosen)"` fired
  while the line actually being translated was `"Orange passes Political
  Phase"`; another fired at `"Orange bids 6"`. Five phantom forced
  discards happen this way before round 10's first REAL one.
- By round 12, the reconstruction's hand has been silently trimmed back
  under the limit by these phantom discards, so when the REAL forced
  discard is due (`"Purple discards 2 cards"`, matching a real `"Discard
  Phase 2 military cards must be discarded"` line), no pending ever
  reopens -- exactly the `decider != expected actor ... no pending` shape.

**Fix attempted**: replace `ground_military_hand`'s bare push with the
`ground_bid_ceiling` "swap a filler victim" pattern (excluding `Bonus`-
type cards from the victim pool, same as `ground_bid_ceiling`; excluding
cards `DiscardSolver::needed_after` already knows are needed for a later
observed War/Aggression/Pact/Tactic play). Compiled clean, all 1084
existing tests passed unmodified, and it DID fix the one traced example
(`7523355`'s round-4 phantom discard at line 58 disappeared).

**REVERTED after corpus-wide measurement showed a net regression.** Not a
naive count comparison -- diffed the EXACT game-ID sets in the bucket
before/after (`REPLAY_DUMP_BUCKET` + `comm`), per the standing instruction
to verify a count change rather than assume "deeper play, different bug":
the bucket grew **167 -> 204** (38 games newly IN the bucket, only 1
newly OUT), and sampling five of the newly-added games directly against
the unfixed binary found real harm, not exposure -- e.g. game `7522054`
flipped from a full **COMPLETE** replay (302 actions, real engine scores
computed) to a brand-new mid-game stall at round 19; games `7521671`/
`7521724` now stop at round 4/5 instead of round 6/7 they previously
reached (FEWER actions consumed, not more). Total corpus decisions also
fell slightly (174551 -> 173485) -- a real regression, not a shift.

**Working theory for why the naive fix regresses, not chased further this
pass**: `DiscardSolver::needed_after` only protects a card needed by a
LATER `DeclareWar`/`PlayAggression`/`ProposePact`/`PlayTactic` (its own
`future_military_needs` prescan's scope). It knows nothing about a card a
LATER `LosePop`/`PlunderSplit`/`Raid`/`Infiltrate`/`LoseColony`/
`FlipWonder` FIFO also expects to still find in that exact identity --
swapping such a card out as a "victim" would silently break one of THOSE
resolutions instead, trading one bug for another. There is also a SECOND,
inconsistent `hand_military.push` site this pass found but did not touch:
`resolve_political_decision`'s own `self.state.players[decider as
usize].hand_military.push(prep.card)` (`~1214`, granting the card a
player is preparing an event with) -- fixing `ground_military_hand` alone
while this sibling site still bare-pushes may itself be part of the
regression (a filler victim `ground_military_hand` swapped out could be
exactly the card THIS site later assumes is still present in its
original, un-swapped slot). **Next attempt should fix both sites
together** and extend the "needed later" check to cover every FIFO this
file maintains, not just `DiscardSolver`'s.

**Ruled out**: the military draw-count formula (`economy::end_of_turn`'s
`military_actions.clamp(0,3)`) -- confirmed exact against the journal's
own printed counts, not the source of the drift.

### Root cause 2 (diagnosed, NOT fixed): `PlunderSplit`'s cap is capped by a drifted food/resource economy, not a missing FIFO entry

Traced game `7523350`'s stop (line 326, bucket example) with
`REPLAY_DEBUG_ALL`. The live `Pending::Choice(PlunderSplit)` reached at
the stall offers four options -- `(food, resources)` pairs `(0,3) (1,2)
(2,1) (3,0)` -- every one summing to **3**. But the journal's own
"Plunder against Purple" line (`~323`) prints `"Your rival loses a total
of up to 7 resource and/or food"`, and its real resolution line (`~325`,
correctly parsed by `parse_plunder_split_line` -- confirmed by hand
against that function's own test table) is `"Orange produces 7 resources;
Purple spends 7 resources"` -- a **7**-total split. The FIFO's one real
entry, `(food: 0, resources: 7)`, never matches ANY of the live choice's
four (food+resources=3) options, so the validate-and-skip loop (built for
the legitimate "this entry belongs to an earlier, already-auto-resolved
single-option split" case) discards it as non-matching, finds the FIFO
then empty, and reports `StuckPending` -- even though the real resolving
line genuinely is sitting right there in the journal.

`interact::offer_plunder_split`'s own cap is meant to be `min(printed
value, what the VICTIM actually has to give)` -- but Purple's OWN
end-of-turn line one turn earlier (`~321`) prints `food: 13 (now 13),
resources: 24 (now 24)` -- nowhere near small enough to justify a cap of
3. This points at a genuine food/resource-economy drift in THIS
reconstruction's own tracking of Purple's stock at the moment the split
is offered (same FAMILY of issue, not the same bug, as the already-
documented "yellow-bank/food drift" lead from the Seventeenth pass,
`7522625`) -- OR a bug in `offer_plunder_split`'s own cap computation
(possibly reading the wrong player's stock, or a stale/mismodelled field).
**Not chased further this pass** -- this is a resource-tracking
investigation, not a `resolve_intervening`/grounding one, and is likely
adjacent to territory other workers (Build/Upgrade/WonderStep,
`IllegalMove: Pop`) are already independently poking at from the "cost
mismatch" angle. Concrete next step: instrument `interact::
offer_plunder_split`'s own cap computation directly (which field, whose
stock) and cross-check against the SAME end-of-turn `resources`/`food`
prints this pass already added for the discard investigation.

### Root cause 3 (diagnosed AND FIXED): `LosePop`'s `Barbarians` event mis-evaluated a tied "weakest civilization" -- ENGINE BUG, confirmed and landed

Traced game `7522639`'s stop with `REPLAY_DEBUG_ALL`. The triggering
event line reads (verbatim, BGO's own text): `"Barbarians: If the
civilization with the most culture points is one of the two weakest
civilizations it loses 1 population. **No effect**"` -- the real game
explicitly states the condition did NOT fire. This reconstruction's own
trace for the SAME reveal shows a strength TIE (3 vs 3) between the two
2p players, with the revealer (Purple) also holding the most culture (15
vs 3).

**Resolved by reading the actual card text, not by guessing.** Checked
`sources/bga_throughtheages_material.inc.php` (the BGA oracle) directly:
the printed Barbarians text is exactly the "if the civilization with the
most culture points is one of the two weakest, it loses 1 population"
wording above -- there is no separate "ties mean no effect" carve-out on
the card itself. So `RULES_SPEC.md` 5.3's general tie-break ("ties broken
in favor of the current player") DOES apply here; the question was never
whether the rule applies, but which DIRECTION "favor" points for a
penalty selection.

That direction was already settled and fixed once before, for a
different function: the Fourteenth pass above found `apply_single_target`
picking the current player FIRST among ties for `WeakestPlayer` (a
penalty) -- backwards, confirmed by measurement (62/63 real corpus ties
matched the reversed/protective pick). **`Barbarians` has its own,
separate "weakest N" computation (`events::conditional_target`) that the
Fourteenth-pass fix never touched**, and it had the exact same bug:
ranked the weakest cutoff group by unreversed `order`, so a strength tie
put the CURRENT player first in the "weakest" group instead of
protecting them. Game `7522639`'s tie let Purple (the revealer, tied at
strength 3 and holding the most culture) count as the weaker civilization
and queued a `LosePop` BGO's own journal never issued.

**A second sibling had the identical bug**: `resolve_count_targets`'s
`weakestPlayers` branch (the plural, multi-target key -- `Raiders` and
`Crime Wave` are the two base-game cards that use it) also ranked its
weakest group by unreversed `order`, same defect, same fix needed. This
is the recurring shape this project keeps finding: a rule fixed in one
place and silently absent from a sibling that does the same kind of
selection.

**Fix**: factored the reversal `apply_single_target` already did for its
own `favor_current = false` case into a shared `protect_current_from_bad_tie`
helper, and applied it to both `conditional_target`'s weakest-cutoff
group and `resolve_count_targets`'s `weakestPlayers` branch. Two new
regression tests in `rust/src/events.rs`
(`barbarians_spares_the_current_player_from_a_tied_weakest_cutoff`,
`barbarians_still_fires_when_the_most_cultured_player_is_unambiguously_weakest`)
pin both directions; confirmed red against the pre-fix code, green after.
Re-running game `7522639` through `bin/replay` after the fix shows the
false `LosePop` pending choice gone and the replay progressing from 94 to
252 actions before its next (unrelated) stop. Corpus-wide, `replaystats`
shows the `StuckPending: LosePop` bucket drop from 26 to 22 games and
`IllegalMove: Pop` drop from 152 to 147 -- consistent with a handful of
`Barbarians`/`Raiders`/`Crime Wave` tie games, not a bucket-wide fix (most
of the remaining 22/147 have other causes, not sampled here).

**Since `resolve_event` runs identically in real self-play, not just this
replayer, every bot game with a genuine weakest-cutoff strength tie on
one of these three cards paid/dodged the wrong penalty too. Flagged loudly:
the self-play climb should be restarted to pick this fix up**, same as the
Fourteenth pass's `WeakestPlayer` fix was.

### What was NOT investigated this pass (open, no lead)

- The 12 `"<Color> disbands <Unit>"` and 6 `"<Color> destroys <Card>"`
  actor-mismatch sub-shapes (from the pre-fix 216-game signature table
  above) -- not sampled at all.
- Whether `LosePop`'s `Barbarians` finding generalises to the bucket's
  other 25 games, or whether they have their own distinct causes each.
- `DestroyOwn`'s 5-game bucket -- not sampled this pass.

### Cross-bucket note

Root cause 2 (`PlunderSplit`'s resource-cap drift) and, less directly,
root cause 3 (`Barbarians`'s tie-break) both look adjacent to territory
other workers are independently chasing under `IllegalMove: Pop`/`Build`/
`Upgrade`'s own "cost mismatch"/"unmodeled discount source" framing --
flagged to the coordinator via `mcp__discord__message_agent` rather than
raced on, per the standing instruction that duplicated work has cost this
effort before.

## `IllegalMove: Pop` handoff: two fixes landed (184 -> 144), full breakdown of what's left, one concrete unfixed lead

Picked up the bucket at 184 (mean stop round 10.1), per the earlier pass's own
"generic food-cost drift" label -- which this pass replaced with a real,
code-derived breakdown rather than trusting it as a conclusion.

### Method

The existing `REPLAY_DEBUG=1` "DEBUG Pop fail" print (`replay_common.rs`'s
`ActionClass::IncreasePopulation` handler) already dumps `food`/`yellow_bank`/
`civil_actions`/`pop_cost`/the raw journal line for every one of the bucket's
failures -- no new tool was built. Ran the full corpus once with
`REPLAY_DEBUG=1`, captured all 184 (later 152, 144) "DEBUG Pop fail" lines
paired with their game ID (`DEBUG game=...` printed once per game), and
bucketed by signature: does the journal's own stated payment (food clause
plus, when present, a second resource-conversion clause -- see below) equal
this binary's computed `pop_cost`? If yes, the mismatch is a pure `food`
shortfall (production/consumption drift, NOT the yellow-bank tier). If no,
it's a genuine cost-TIER mismatch (yellow-bank/token count itself is wrong).

For the largest sub-groups, traced backwards from the failure using
`REPLAY_DEBUG_ALL`'s existing `end_of_turn POST` line (`economy::end_of_turn`,
prints `resources`/`food`/`science`/`culture` after that turn's full
production phase) against the journal's own per-round `"N food -
consumption: M (now Z)"` clause, round by round, to find the FIRST round
sim and journal disagree -- the standing method this file's earlier passes
established, reused rather than reinvented.

**One methodology trap worth flagging explicitly**: `economy::end_of_turn`
can be entered up to N times for one real turn when
`interact::discard_excess_military` interrupts it (returns early, `false`,
before ANY production runs) -- and `replay_common.rs`'s own "applied
mv=EndTurn"/"end_turn totals" debug print fires on the FIRST (interrupted,
pre-production) call, not the final one. Trusting that print as "the turn's
final food" gives a systematically wrong trace with a phantom "drift"
starting at whatever round first needed a discard -- burned real time on
this before switching to `economy::end_of_turn`'s own `POST` print, which
only fires on the call that actually completes production. If you're
comparing sim food to the journal round-by-round, use `end_of_turn POST`,
never the replayer's own generic per-move dump.

### Fixes landed this pass (commits `deea9a0`, `5154e08`)

1. **REPLAYER BUG: Foray/Raiders' "gains/loses N resources and/or food, your
   choice" event grant was resolved via a deterministic formula
   (`events::food_or_resources`, "resources first, food for the remainder,"
   mirroring the Python reference BOT's own fixed policy) instead of the
   journal's real human split.** Confirmed wrong on game `7523357` round 8:
   engine computed 3 resources / 0 food with 13 blue tokens free the whole
   time (not a capacity effect); the journal's own line reads `"Grey produces
   2 food; Grey produces 1 resource"`, and the triggering event line's own
   `"Grey choses first"` clause confirms this is a genuine per-player choice,
   not a rule. Fixed at the REPLAYER level only (`parse_produces_grant_line`/
   `prescan_produces_grants`, new `Replayer::produces_grants` FIFO):
   `resolve_political_decision`'s `PrepareEvent` handling now overwrites the
   engine's default split with the journal's own, gated on the revealed
   card actually being a `Special::StrongestPlayers`/`WeakestPlayers` +
   `Gain`/`Lose(food_and_or_resources != 0)` shape AND the popped FIFO
   entry's total matching what the engine's own formula just granted.
   `events::food_or_resources` itself is UNCHANGED -- giving the ENGINE
   (not just the replayer's reconstruction) a real choice here is a
   bot-decision-modeling change out of this bucket's scope, flagged not
   fixed. **Gating on the revealed card's own effects, not just the delta
   total, is load-bearing**: a first version of this fix gated on the delta
   total alone and REGRESSED the corpus (`Pop` 184 -> 281) by occasionally
   consuming an unrelated `ChoiceKind::GainBlock` FIFO entry for a card that
   never called `food_or_resources` at all -- caught by re-measuring after
   landing, not by review. Corpus: `Pop` 184 -> 151, mean rounds 11.27 ->
   11.41, Age II+ 45.4% -> 46.1%, completed 24 -> 25.
2. **REPLAYER BUG: a Pop partly paid via a live Trade Routes Agreement grant
   (RULES_SPEC.md 5.9) logs a SECOND `"; <Color> spends M resource"` clause
   on the SAME line as the Pop's own food clause** -- e.g. `"Purple increases
   population Purple spends 2 food; Purple spends 1 resource"` (thousands of
   occurrences corpus-wide). `spent_food`'s own doc comment asserted a Pop
   line "has no resource component" -- FALSE, and the existing
   `TradeResourceAsFood` gate (`ActionClass::IncreasePopulation`'s handler)
   compared the food-clause-ONLY amount against `pop_cost`, so it silently
   never fired for this real, common shape. Fixed by adding
   `spent_resource_after_food` (reads the second clause, `0` when absent)
   and summing it into the existing gate -- the gate's own safety property
   (only fires when the journal's OWN total exactly matches this binary's
   computed `pop_cost`) is unchanged. Corpus: `Pop` 152 -> 144 (measured
   against a tree that had already picked up one concurrent commit from
   another worker in between -- see commit `5154e08`'s own message for the
   isolated numbers), mean rounds 11.48 -> 11.50, Age II+ 46.7% -> 46.8%,
   completed 29 -> 29 (no drop -- these games run into a DIFFERENT wall a
   few lines later, not a net loss).

Five new tests, all in `replay_common.rs`'s `#[cfg(test)] mod tests`, each
CONFIRMED red by reverting its own fix to a no-op and re-running before being
restored green: the Foray correction fires and reads the journal's split; the
negative case (an unrelated card/FIFO entry is left untouched); a
`parse_produces_grant_line` parser unit test (accepts Foray's shape, rejects
Plunder's); a `spent_resource_after_food` parser unit test.

### Breakdown of the 144 that remain (fresh `REPLAY_DEBUG=1` run against the
### landed tree, `d216885`)

| count | signature | meaning |
|---|---|---|
| 85 | cost matches journal's stated TOTAL, `food` short by 1 | pure food-accounting drift, NOT a yellow-bank/tier bug |
| 36 | same, short by 2 | same |
| 11 | same, short by 3 | same |
| 1 | same, short by 4 | same |
| 5 | `TIER MISMATCH`: computed `pop_cost` one tier ABOVE the journal's stated cost (e.g. computed 4, journal says 3) | a genuine yellow-bank/token-count drift -- OUR bank is lower than the real one |
| 3 | same, computed 5 vs stated 4 | same |
| 2 | `civil_actions == 0`, no civil-life free-pop discount either | a civil-action-budget shortfall, same open question the Take/Bid handoff's "What remains open" section already documents -- likely the SAME root cause, not specific to Pop |
| 1 | `pop_cost = None` (yellow_bank already fully exhausted, i.e. 0) | a genuinely empty bank -- check whether the journal's own Pop is even legal at that point, or whether this is really a `PopFree` case this binary mis-gates |

**133 of 144 (92%) are the food-short shape, NOT the tier-mismatch shape.**
This is the opposite of what the task brief's own hypothesis predicted going
in ("a shortfall of exactly one tier points at a missing token event" was
expected to dominate) -- worth stating plainly so the next pass doesn't
re-assume it. The two fixes landed this pass were BOTH found by chasing
food-short games, not tier-mismatch games, and there is no evidence yet that
the tier-mismatch 8 share a cause with each other, let alone with the
food-short 133 -- treat them as a SEPARATE, smaller, unstarted lead.

### Concrete unfixed lead for the next worker: `yellow_bank` drifts by whole
### units mid-turn with no Pop/event/combat move firing

Traced game `7522648` round 7 (2p, actor Orange/idx 0) end-to-end:
`economy::end_of_turn`'s own `POST` food matches the journal exactly for
rounds 1-6, then falls 1 short at round 7 and stays 1 short through round 8
(the eventual Pop failure at round 9 is the LAST hop of this same drift, not
a new one). Root-caused the round-7 divergence to a **yellow_bank change
with no attributable move**: right after that turn's own `Move::Pop`,
`yellow_bank=13` (`consumption(13)=1`); by the time the SAME turn's
`end_of_turn` finally completes (after being interrupted twice by
`discard_excess_military`, see the methodology trap above),
`yellow_bank=11` (`consumption(11)=2`) -- ONE MORE food eaten by consumption
than the Pop-time value would predict, with nothing but two military-card
discards in between (confirmed `discard_excess_military`/the discard
resolution path never touch `yellow_bank` -- read the whole function, it's
pure hand-size bookkeeping). **Not root-caused before this pass ended** --
every `yellow_bank`-mutating call site was enumerated (`grep -n
"yellow_bank\s*[+-]?="`, listed in the commit `d216885` message) but not
individually checked against this game's exact turn. A
`REPLAY_DEBUG_ALL`-gated print between `end_of_turn`'s production and
consumption steps (`economy.rs`, right after `gain_food`) is already landed
(commit `d216885`) so the next worker does not need to re-add it -- rerun
`REPLAY_DEBUG_ALL=1 ./target/difftest/replay ... 7522648` and grep
`yellow_bank=` across Orange's round-7 turn to see the exact drift point.

**This is the single most promising next thread for this bucket**: it is a
DIRECT, reproducible, single-game repro with known-good "before" and
known-bad "after" values (13 -> 11, no attributable move), the same shape
that resolved BOTH fixes this pass landed, and -- if it turns out to be
generic rather than specific to this one game -- would plausibly explain a
large share of the remaining 133 food-short games at once (yellow_bank
drives BOTH the Pop-cost tier via `pop_cost_base` AND the food-consumption
rate via `consumption`, so a silent extra decrement compounds through both).

### What was RULED OUT this pass (don't re-check these)

- **The yellow-token bank/tier theory as the DOMINANT cause.** Tested
  directly, corpus-wide, per the task brief's own instruction: only 8/144
  (5.6%) of the remaining bucket show a genuine cost-TIER mismatch (computed
  `pop_cost` one tier above the journal's stated cost); 133/144 (92%) have
  the cost tier EXACTLY RIGHT and a pure `food` shortfall instead. The
  earlier "generic food-cost drift" label undersold how lopsided this split
  is -- worth stating for anyone tempted to keep hunting for missing
  token-grant events as the majority explanation.
- **`events::food_or_resources` as a still-open bug.** Fixed (see above).
  Its own doc comment and the new `parse_produces_grant_line` tests are the
  reference if a similar "gains N resources and/or food" shape shows up on
  a different card later (Raiders, the WeakestPlayers/loss twin, was NOT
  separately traced against a real corpus example this pass -- it goes
  through the exact same `food_or_resources` call site and should already
  be covered by the fix, but no Raiders-specific game was walked end-to-end
  to confirm the SIGN (loss) direction resolves correctly against a real
  "loses"/"spends" journal line, which uses different text than Foray's
  "produces" -- worth a quick spot-check, not re-deriving from scratch).
- **The Trade Routes Agreement double-clause shape as a still-open bug.**
  Fixed (see above); `spent_resource_after_food`'s own test is the
  reference.
- **A quick grep for sibling "engine computes a value the journal actually
  states" call sites** (the same shape both fixes above turned out to be):
  `food_or_resources`'s only call site was the one already fixed;
  `choose_food`/`choose_resources` (`ChoiceKind::GainBlock`) is already
  correctly wired to a real `Pending::Choice` reading the journal's own
  line; the colonize-sacrifice auto-drain and `PlunderSplit` instances of
  this same pattern were already fixed in earlier passes (see the
  "Fifteenth pass" and six-pending-kind sections above). Not exhaustive --
  a targeted grep across `economy.rs`/`events.rs`/`interact.rs`/`combat.rs`
  for `"deterministic"`/`"arbitrarily"`/similar markers, not a full audit.
- **`interact::discard_excess_military` and its discard-resolution path as
  the yellow_bank drift's own cause** -- read in full, confirmed it never
  touches `yellow_bank`, `p.food`, or `p.resources` at all (pure hand-size
  bookkeeping). The drift happens SOMEWHERE in the same window but not
  there; still open.
- **The `civil_actions == 0` (2 games) and `pop_cost = None` (1 game)
  sub-buckets** -- NOT investigated this pass beyond classification. The
  first is very likely the same open civil-action-budget question the
  Take/Bid handoff already documents (`docs/REPLAY.md`, "What remains
  open"), not something specific to Pop -- worth checking there first
  rather than re-deriving.

### Cross-bucket note (already reported live, recorded here for the record)

Chasing a lead from the actor-mismatch worker (game `7523350`,
`StuckPending: PlunderSplit`, "the victim's food/resources pool is badly
below what their own end-of-turn journal line states") found a THIRD,
separate food/resource drift mechanism: `events::extra_production`
("Economic Progress", `Special::AllPlayers` + `extra_production: true`)
computes `effects::state_stats(state, &players[idx]).food` ONE HIGHER than
the same player's own regular end-of-turn production for an unchanged board
one round earlier (game `7523350`, Purple, round 17: journal's own event
line states "3 food - consumption: 1", `s.food` reads 4 at that exact call
site). NOT fixed here -- outside this bucket, reported to the actor-mismatch
worker with the specific game/values, and a `REPLAY_DEBUG_ALL`-gated
`extra_production` before/after dump is already landed (bundled in commit
`5154e08`) as a head start for whoever picks it up. Do not re-chase this
from the Pop bucket; it is not yet known whether it explains any of the 144
remaining Pop games specifically.

### Final numbers, this pass

| | before this pass | after |
|---|---|---|
| `IllegalMove: Pop` | 184 | **144** |
| mean rounds reached | 11.27 | **11.50** (measured against `d216885`, includes concurrent unrelated work from other passes) |
| decisions in Age II or later | 45.4% | **46.8%** |
| games completed | 24 | **29** |

**Concrete first move for whoever picks this up**: reproduce the
`yellow_bank` drift on game `7522648` (`REPLAY_DEBUG_ALL=1 ./target/difftest/replay
sources/bgo/index.tsv /tmp/bgojournals/journals 7522648`, grep `yellow_bank=`
across Orange's round 7) using the already-landed `end_of_turn
post-production` print, and check EACH enumerated mutation site (commit
`d216885`'s message has the full `grep` list) against what actually ran in
that window -- not by reasoning about what SHOULD run, the way this pass's
own dead ends were caused by trusting a stale debug print instead of the one
that reflects the turn's real final state.

## Root cause 1, follow-up: the swap fix's own regression traced to ground
## on real game `7522054` -- it exposes a SEPARATE, pre-existing hand-SIZE
## UNDERCOUNT that the growth bug was accidentally masking; the doc's own
## "next attempt" (fix both push sites together) tried and FALSIFIED --
## makes it far worse (171 -> 305, not better), NOT landed; a concrete,
## unused, journal-native lead identified for whoever picks this up next

Picked up exactly where the previous pass left off: re-derived nothing,
started from "reproduce the regression, trace `7522054` specifically."

### Reproduced the regression exactly, as a method sanity check

Applied the documented "swap a filler victim" fix to `ground_military_hand`
verbatim (mirroring `ground_bid_ceiling`: exclude `Bonus`-type cards and
`DiscardSolver::needed_after` cards from the victim pool, worst-defender-
first). Corpus-wide, exact `REPLAY_DUMP_BUCKET` + `sort`/`comm` set diff
against the current landed baseline (171 games, IDs saved to a file, not
just the count): **39 games newly IN the bucket, 1 newly OUT** (`7523415`).
`7522054` is confirmed among the 39 newly-broken -- same shape as the prior
pass's `167 -> 204` finding, different exact numbers only because a
concurrent Foray/Raiders fix (`deea9a0`) had already landed in between.
Method confirmed sound before spending any further effort on it.

### Traced `7522054` line by line with `REPLAY_DEBUG_ALL` -- the swap fix is not wrong, it is just no longer lucky

Added a temporary debug print inside the swapped `ground_military_hand`
(`actor`, the card being grounded, the chosen victim, the whole filler
pool) and replayed `7522054` alone (a one-line `index.tsv` copy makes this
trivial and fast -- no need for a full corpus pass to inspect one game).

**The swap itself, for every one of the 5 times it fires in this game, has
GENUINELY CORRECT victims.** Traced each one by hand against the raw
journal: e.g. line 190's `"Purple plays Infiltrate against Orange"`
requires Orange to spend a SECOND physical copy of the age-I `Military
Bonus (defense 2 / colonization 1)` card as part of a 3-card, 5-point
defense (`atk:13`, two `+2` Bonus spends and one `+1` flat spend = `13`
exactly, confirmed against `interact::defense_points`/`resolve_aggression_
defense`'s own doc) -- the simulated hand only had one physical copy of
that value, so the swap correctly grounds a second one, evicting `"Civil
Unrest"`. `"Civil Unrest"` never appears ANYWHERE else in this game's raw
journal (`grep` confirmed zero other matches) -- it really is pure,
un-observed `new_game` filler, exactly the kind of card `ground_bid_
ceiling`'s pattern is supposed to sacrifice. Every other swap in this game
(`Mechanized Army` for `"Aggression: Plunder (III)"`, `Aggression: Armed
Intervention` for `"Impact of Balance"`, two more for player 1) is the same
shape: a genuine, never-otherwise-named filler card evicted for a genuine
reveal. **The victim selection is not the bug.**

**What actually flips `7522054` from COMPLETE to STALLED**, found by diffing
`idx=0`'s `discard_excess_military` checkpoints between the unmodified
binary and the swapped one, side by side, round by round: at the round-19
checkpoint (`"Orange discards 2 cards"`, journal line 365, the real
resolution line the whole replay is heading for):

| | reconstructed hand len | `military_hand_limit`+`military_actions` | excess |
|---|---|---|---|
| unfixed (push, current landed code) | 8 | 7 | **1** |
| swapped (the reverted fix) | 7 | 7 | **0** |
| truth (journal says `"discards 2 cards"`) | implied 9 | 7 | **2** |

**Both reconstructions already undercount the true hand size at this exact
round** -- the unfixed one by 1, the swapped one by 2. Neither is right.
But `interact::discard_excess_military` only opens a `Pending::Choice
(DiscardMilitary)` at ALL when `hand.len() > limit`: the unfixed binary's
accidental +1 (one card, `"International Agreement"`, that the growth bug
never lets go of, permanently, from round 12 onward -- confirmed by diffing
every subsequent hand-content checkpoint between the two binaries, it is
the ONLY sustained difference besides the swap's own intended targets) is
*just* enough surplus to cross that `> limit` line and open SOME pending,
which is all `resolve_intervening`'s `ChoiceKind::DiscardMilitary`
"anything else" branch needs to keep the replay moving (it does not check
the pending's own size against the journal's printed `"2 cards"` -- see the
prior pass's own finding that this branch drains unconditionally). The
swapped binary's cleaner, non-inflating accounting lands EXACTLY on the
limit -- zero excess, no pending opens at all, `decider` has already rolled
over to the other player by the time journal line 365 arrives, and that is
the literal `decider != expected actor ..., no pending` shape this whole
bucket is named for.

**In other words: the growth bug's own over-counting was accidentally
compensating for a second, independent, pre-existing UNDER-counting bug,**
close enough that the two errors canceled out for this game specifically.
Fixing the over-count alone removes the compensation without touching the
under-count, so games sitting close to that cancellation boundary flip from
"accidentally correct" to "newly broken." This is a REPLAYER bug either
way (both errors are in this binary's own hand-size reconstruction, not in
the engine's `discard_excess_military`, which was independently confirmed
correct against §6.6 -- see its own module doc in `interact.rs`, and the
military DRAW count was already ruled out exact by the prior pass).

**The under-count predates every `ground_military_hand` call in this game
and cannot be this function's fault.** Checked directly: `7522054`'s FIRST
`ground_military_hand` call (either binary) fires at journal line 190,
round 12. But the reconstruction's own `idx=0` hand already disagrees with
the journal at ROUND 4 (line ~54, `"Legion"` discarded) -- the real
journal's line 52 states outright `"No Discard Phase"` for that exact
round, while this binary's own `hand_military_len=3` against `limit=2`
forces a discard anyway, at a completely unrelated line (`"Purple builds 1
stage of Library of Alexandria"`) via the same unconditional-drain
mechanism described above. Confirmed byte-for-byte IDENTICAL between the
unfixed and swapped binaries (neither had made a single grounding call yet
at that point in the game) -- so whatever produces this round-4 hand
oversized by (at least) one card is a THIRD bug, upstream of both the push
and the swap, most likely in `new_game`'s own initial fictional deal size
or in the `military_hand_limit`/`military_actions` formula's interaction
with the very first few rounds, not in any grounding function at all. Not
chased further this pass -- flagging it as the real next lead, below.

### The prior pass's own "next attempt" theory, tried and FALSIFIED: do not fix `resolve_political_decision`'s push site the same way

The prior handoff's working theory was that `resolve_political_decision`'s
own `self.state.players[decider].hand_military.push(prep.card)` (`~1214`,
now `~1227`) -- a second, still-bare push site granting a card being
prepared for a future event play -- was "part of the regression" and that
"the next attempt should fix both sites together."

Tried it directly: routed `resolve_political_decision`'s push through the
same (swapped) `ground_military_hand` instead of a bare push. Corpus-wide
result, same exact-ID-diff method: **the bucket gets far WORSE, 171 -> 305**
(with the military-hand swap already applied on top -- i.e. worse than the
single-site swap's own 171 -> 209), completed games **29 -> 28**, mean
rounds down, decisions down (177645 -> 174343). **This theory is
falsified, not just unconfirmed -- do not retry it as written.**

Working explanation, not chased further: unlike a combat-defense `Bonus`
reveal (rare, and `ground_bid_ceiling`/`defense_bonus_card` already
established that each `Bonus` value is genuinely unique-per-age, so a
repeat reveal legitimately means "no-op, already have it"), a political-
phase event PREPARATION fires roughly once per player per TURN -- far more
often -- and there is no equivalent proof that `Tactic`-family cards are
each a single unique physical instance the way `Bonus` cards are. Making
this site a silent no-op whenever the card is already present (which is
what routing it through `ground_military_hand` does) most likely papers
over many genuine "a second physical copy of this Tactic identity was
drawn" cases, turning what was over-counting (bare push, always grows) into
severe under-counting at a MUCH higher frequency than the combat-defense
site. **Do not extend the swap/no-op pattern to this site without first
confirming, per-card, whether repeated Tactic/event identities are
actually unique-per-age like `Bonus` cards -- this pass did not check.**

### Net result this pass: reverted to the landed baseline, nothing shipped

Both attempted fixes (swap-only: 171 -> 209; swap-both-sites: 171 -> 305)
regress the corpus relative to the current landed code. Per the standing
instruction that a drop in completions is not automatically damage but
must be traced -- both WERE traced, on real games, and both are genuine
regressions, not "deeper play hitting a different bug": `7522054` really
does go from a full COMPLETE 302-action replay with real engine scores to
a brand-new stall, for the reason detailed above, in both attempts.
Working tree reverted to the exact landed baseline (`git checkout --
rust/src/replay_common.rs`); rebuilt and reconfirmed byte-identical to the
pre-pass numbers: **29 completed, mean rounds 11.48, 177645 decisions,
actor-mismatch bucket at 171.** No commit needed for this session --
nothing changed in the landed tree.

### The concrete, unused lead for whoever picks this up next: the journal already PRINTS the true discard count, and this binary throws it away

`corpus::classify` (`corpus.rs:658`) matches every `"Discard Phase N
military card(s) must be discarded"` line -- and every `"No Discard
Phase"` line -- and classifies BOTH as bare `LineOutcome::Bookkeeping`,
i.e. skipped with the count `N` never parsed or looked at again anywhere
in this file. This is PUBLIC information (an on-screen phase announcement,
not a rival's hand or deck order -- fully legal to use per this task's own
legality rule) that states, in the journal's own words, exactly how many
cards this binary's reconstruction OUGHT to need to discard at that turn,
independent of whatever this binary's own `hand_military.len()` happens to
compute.

This cannot directly force `interact::discard_excess_military` to open a
pending (that function is real engine logic, gated on `state`'s own hand
length vs. limit, not on anything the replayer can inject after the fact)
-- but it is exactly the missing cross-check the round-4 and round-19
findings above both point at: right now this binary has NO way to notice
that its own reconstructed hand size has drifted from the truth until a
`StuckPending` several rounds later makes it obvious. A worker chasing the
real (pre-existing, upstream-of-grounding) hand-SIZE undercount/overcount
bug should:

1. Parse `"Discard Phase N ..."` / `"No Discard Phase"` into a per-player,
   per-turn expected-discard-count FIFO (the same shape as every other
   `prescan_*` function in this file), separate from the actual `"<Color>
   discards N cards"` resolution line already used for `DiscardSolver`.
2. Cross-check it against `interact::discard_excess_military`'s own
   `REPLAY_DEBUG_ALL` print (`hand_military_len`, `limit`) at the matching
   turn -- an exact, game-by-game, round-by-round oracle for whether this
   binary's reconstructed hand size is right, BEFORE the `StuckPending`
   several rounds later, rather than reasoning about it after the fact the
   way this pass had to.
3. Use it to find where the drift is actually introduced -- candidates
   this pass did NOT check: `new_game`'s initial fictional military deal
   size (the round-4 divergence in `7522054` predates every grounding call
   in the game, so this is the most likely single place), and the
   `military_hand_limit`/`military_actions` formula's own interaction with
   early rounds (already ruled out as a DRAW-count problem by the prior
   pass, but never checked as a LIMIT problem).

This is a materially different, and more promising, lead than either the
grow-vs-swap question this pass exhausted or the sibling-bucket root cause
(`PlunderSplit`'s resource-cap drift) still left open above -- `LosePop`'s
own sibling root cause (`Barbarians`'s tie-break) was landed as an ENGINE
bug fix by a concurrent worker mid-pass, see that section above this one.

## Game `7521984` lead: CLOSED (already fixed upstream, no action needed)

Picked up the standing assignment: game `7521984`'s `IllegalMove: Upgrade`
at round 8 (`discontent=2 > workers_free=1` firing an uprising the real
journal never had). Independently traced it to the exact same root cause
`62befa4` ("REPLAYER: defer force_civil_age_at_least while a pending
choice/deferred effect is still open") fixes: `force_civil_age_at_least`
(called once per journal line, unconditionally, off that line's own age
column) was running `advance_age`'s §12.2.4 "-2 yellow_bank" population
loss for EVERY player one whole age transition early, whenever the
PRECEDING line's own end-of-turn was still suspended on a `DiscardMilitary`
choice -- exactly `7521984`'s own shape (Purple's round-7 end-of-turn opens
a `DiscardMilitary` choice the real journal never needed, `"No Discard
Phase"`; the very next line is Orange's round-8 `"passes Political Phase"`,
already tagged age `II`; forcing the age-and-penalty there ran the
population loss on Purple BEFORE her own suspended uprising check had used
the pre-transition `yellow_bank`).

Was independently re-deriving this exact fix (own `catch_up_civil_age`
method, own two tests) when the workspace was re-synced onto a fresh
`origin/master` that already had `62befa4` landed, apparently found via a
different repro (`7522648`, chasing the `IllegalMove: Pop` bucket) by
another pass. Confirmed both fixes are the same mechanism (mine gated on
`state.pending.is_empty()` alone; the landed one gates on
`state.pending.is_empty() && state.queue.is_empty()`, a strict superset --
no functional gap). Discarded my own copy rather than land a duplicate.

**Verified the fix against `7521984` specifically** (not just trusting the
aggregate count): `replay` now runs it to round 14 / 199 actions (up from
the round-8 `IllegalMove: Upgrade` stop) before hitting a *different*,
later `StuckPending: decider 0 != expected actor 1, phase Actions, no
pending` on a `"Purple discards 1 card"` line -- the discard-phase-count
bucket explicitly owned by another live worker per this pass's brief. This
lead is CLOSED; nothing further to do here.

## New lead found chasing "attack the next-largest Build/Upgrade/WonderStep
## sub-bucket": the strongest/weakest-civilization "and/or" gain/lose block
## is a real per-player CHOICE, not the deterministic formula this binary
## implements -- likely explains roughly half of `IllegalMove: Build`'s
## `resources_short` sub-bucket (NOT fixed this pass, documented instead)

Re-measured the full corpus after the `62befa4` re-sync (`replaystats`,
1,011 games): 48 completed, mean round 12.14. Current
`Build`/`Upgrade`/`WonderStep` counts: `Build` 102 (now the largest of the
three, up from this doc's earlier 67 baseline -- expected exposure, not a
regression, per the "closing one wall exposes the next" pattern), `Upgrade`
57, `WonderStep` 51. Per the standing instruction, attacked `Build`, the
current largest.

**Sub-categorised all 102 `IllegalMove: Build` games** (`REPLAY_DUMP_BUCKET`
for the game IDs, then `REPLAY_DEBUG=1 ./replay` per game, reading each
`try_apply fail`'s own `resources=`/`workers_free=` fields against the same
line's `costs::build_cost_for` print -- the existing diagnostics named in
this pass's brief, nothing new built):

| sub-bucket | count | shape |
|---|---|---|
| `resources_short` | 62 | `resources < build_cost_for(card)` -- the build's OWN cost is right (otherwise it would already be an `UnrecoverableHiddenInfo: build cost mismatch`, a *different*, already-working bucket that fires when the journal's stated cost disagrees with `build_cost_for`), but this reconstruction's `p.resources` is short by 1-3. |
| `workers_free_zero_but_resources_ok` | 31 | Same shape this doc's earlier passes already ruled out as a DIFFERENT, unexplained bug (`workers_free == 0` while `resources` is plentiful) -- not re-chased this pass, still open. |
| other | 9 | Not yet sub-categorised. |

**Traced the `resources_short` sub-bucket's root cause on game `7522886`
end to end**, and it is a real, confirmed **ENGINE BUG**, not a replayer
parsing gap:

Round 6 end (`"End turn Orange scores: ...; 3 resources (now 3)"`, a
running total, trustworthy) leaves Orange with 3 resources, matched exactly
by this binary's own `end_of_turn POST` print. Grey's very next political
action reveals `Raiders` (`"Each of the two weakest civilizations lose a
total of 2 resources and/or food.; Green and Orange each lose 2 resources
and/or food; Green choses first"`) -- **the player-SELECTION half of this
is confirmed correct**: added a one-line `REPLAY_DEBUG_ALL` print inside
`events::resolve_count_targets`'s `weakestPlayers` branch (reverted after
confirming, not shipped) showing this binary's own computed strengths
`[Grey=2, Orange=0, Purple=1, Green=1]` and its resulting `targets=[Orange,
Green, Purple, Grey]` -- `[Orange, Green]` for `weakest_count=2` matches the
journal's own `"Green and Orange"` exactly. The already-landed
`d9e52c6`/`a008990` tie-break fix (this Raiders/Crime Wave/Barbarians/
Uncertain Borders sibling-rule sweep) is doing its job; this is not that
bug reappearing.

**The SPLIT is wrong.** `events::food_or_resources` (called by
`apply_gains_block`'s `food_and_or_resources` arm, which is what
`resolve_count_targets` uses for every `strongestPlayers`/`weakestPlayers`
block) is a fixed, deterministic formula: "drains resources first, floors
at zero, spills into food" for a loss (the mirror for a gain). Its own doc
comment cites `§5.4.6/§11.5` and explicitly contrasts it with Plunder's
FAQ-p.7-established real choice. But the SAME journal that names Orange and
Green as targets also logs their OWN, DIFFERENT resolutions immediately
after: `"Green spends 1 food; Green spends 1 resource"` (a genuine 1/1
SPLIT of the identical total-2 loss) and `"Orange spends 2 food"` (ALL
food, zero resources -- the exact opposite of "resources first"). The
Foray gain two lines earlier shows the same shape on the GAIN side:
`"Purple produces 3 resources"` vs `"Grey produces 3 food"` for an
identical "gains a total of 3 resources and/or food" block. **The literal
word in the journal is "choses" (chooses)** -- `"Green choses first"` /
`"Purple choses first"` -- this is BGO rendering a real, sequential,
per-player CHOICE (each affected player independently picks their own
food/resources split, in `targets`-order, same shape as `ChoiceKind::
PlunderSplit` but self-directed rather than an attacker choosing for a
victim), not a deterministic formula. This binary's fixed "resources first"
substitute is measurably wrong whenever a player's actual choice disagrees
with it, and per this trace, real players clearly do NOT all default to
"protect food, drain resources" -- Orange did the opposite.

**Confirmed by full arithmetic reconciliation, not just the raw clause
text** (the `"3 food" proven wrong against the log's own running total`
caveat this pass's brief opens with applies to `EventBlock` clause text
in general, so this was checked against a THIRD, independent number, not
just the two conflicting clauses): round 7's own end-of-turn line
(`"3 resources (now 3)"`) proves Orange's PRE-production round-7 resources
were `3 - 3 = 0`; round 7's own spending (`"Orange builds Warrior ...
spends 1 resource"` [Homer-discounted] + `"Orange builds Warrior ... spends
2 resources"` [full price] = 3 total) proves Orange ENTERED round 7 with
`0 + 3 = 3` resources -- i.e. Orange's resources were completely untouched
by the Raiders loss the round before, exactly matching `"Orange spends 2
food"` (all-food) and contradicting this binary's own "resources first"
computation (which drains 3 -> 1 immediately on the Raiders line, then
correctly tracks 1 -> 0 -> **fails needing 2 for the second Warrior**,
exactly the corpus stop).

**This also means an EXISTING doc comment in this file's own source is
wrong** and should be corrected whenever this is picked up:
`replay_common.rs`'s `parse_plunder_split_line` doc (~line 2438) calls
Foray/Refugees' "and/or" grant "a DIFFERENT and deterministic kind" of
"produces" line, contrasting it with Plunder's real choice -- that
characterization is what this trace falsifies. The line-SHAPE
distinction the comment draws (no trailing victim `"spends"` clause) is
still correct and still needed for parsing; only the "deterministic"
claim is wrong.

**Scale check, not full proof for every instance**: of the 62
`resources_short` games, 50 have a `Foray`/`Raiders`/`Crime Wave`/
`Refugees` event somewhere in their own journal (`grep` over
`Current event:; [AI]+ / (Foray|Raiders|Crime Wave|Refugees)`) -- a strong
correlation, not a confirmed cause for all 50 individually (some events
may be far from the eventual failure point, or coincidental). Extrapolating
loosely, this ONE mechanism could plausibly explain something on the order
of half of `IllegalMove: Build`'s `resources_short` sub-bucket alone, and
is very likely NOT specific to `Build` -- any bucket downstream of a
`strongestPlayers`/`weakestPlayers` "and/or" gain/lose block (`Upgrade`,
`Develop`, `PlayAction`, `WonderStep`'s own resource-shortfall shapes) is a
plausible sibling, unmeasured this pass.

**NOT fixed this pass -- deliberately, not from lack of trying.** A correct
fix needs one of two shapes, both larger than this pass's remaining budget
could safely implement AND confirm-by-revert-test in one sitting:

1. **Real engine choice**: give `resolve_count_targets`'s `food_and_or_
   resources` arm a genuine `Pending::Choice`, opened per targeted player
   in `targets` order (matching `"<Color> choses first"`), offering every
   valid food/resources split summing to the block's total -- structurally
   the same shape `interact::offer_plunder_split` already builds (reuse its
   `ChoiceOption::Gain` split-enumeration, generalised to a self-directed
   decider instead of an attacker). Needs: a new `ChoiceKind` (or a
   `PlunderSplit`-shaped variant with `victim` optional/self), self-play's
   own default answering policy for when nobody scripted a real choice
   (existing "resources first" behaviour is a safe, compatible default --
   climb is paused, so bot-policy quality here is not urgent), AND the
   replayer's own `resolve_intervening` arm to drain it -- reading the
   REAL split off the observed `"<Color> produces/spends N food[; N
   resources]"` lines this file already parses the GAIN half of
   (`parse_standalone_produces`) but not yet the LOSS half (`"spends"`,
   standalone, no leading action verb -- needs its own parser, mirroring
   `parse_standalone_produces`'s exact "whole line, no other clauses" care
   so it cannot misfire on an ordinary build/develop's own embedded
   `"spends"` clause).
2. **Replayer-only post-hoc correction** (smaller, does not touch
   self-play/engine, ONLY improves reconstruction fidelity): since `events::
   food_or_resources`'s split is a pure, deterministic function of
   (total, sign, the player's OWN food/resources at that instant), the
   replayer can recompute exactly what the engine already did the instant
   it reads the player's own observed `"produces"/"spends"` line, diff it
   against the real split, and apply just the delta (move tokens between
   `p.food`/`p.resources`, net zero) -- no new `Pending`/`ChoiceKind`, no
   self-play change. Needs the same new "standalone spends" parser as
   option 1, plus recognising WHICH recent bookkeeping lines are this
   family's own gain/loss (distinguishing a `strongestPlayers`/
   `weakestPlayers` block's "and/or" resolution from an ordinary flat
   `gainFood`/`gainResources` event, which prints the identical
   `"<Color> produces N food"` shape for a DIFFERENT, genuinely
   deterministic reason and must NOT be corrected).

Whoever picks this up next: start from game `7522886`, lines 189 (`Raiders`
reveal) through 207 (Orange's own round-7 `End turn`) in
`sources/bgo/journals`-equivalent `/private/tmp/bgojournals/journals/
7522886.tsv` -- this trace is already fully reconciled and does not need
re-deriving, only implementing against.

## HandFull handoff, closed with a full negative: every card in every rejected hand has sound provenance -- no replayer bug found, `>=` gate untouched

Picked up the "third, larger-sample per-card provenance trace" the
"result, corpus-wide, the remaining 78 `HandFull` need a different cause"
section above explicitly left open, after that section's own
`ca_total`-undercount check came back clean and argued (correctly, this
pass confirms) AGAINST the provenance direction being fruitful. Did the
trace anyway, per this task's own explicit brief, rather than taking that
steer as settled -- a negative result confirmed twice, independently, is
worth more than a steer taken on faith.

**Re-measured first, as instructed, before trusting the stale 78/79
number**: a dozen unrelated commits landed mid-pass (`62befa4` deferring
`game::force_civil_age_at_least` while a pending choice is still open,
among others -- see this doc's own commit log). Full corpus, HEAD at
`5aa032f`: 48 games completing (was 29/30), mean rounds 12.14, and the
`HandFull` gate count itself moved from 78 to **98**
(`REPLAY_DEBUG`'s `reason=HandFull` count, not a bucket-histogram row --
same measurement convention every prior `HandFull` section in this doc
used). This is a SIZE change from deeper replay reaching more rejections,
not a shape change -- re-verified below that `hand_civil_size ==
civil_hand_limit` exactly still holds for all 98, same as every prior
pass found for its own smaller count.

**Method**: no side script reimplementing game rules (the exact trap a
much earlier `HandFull` pass's own accumulator fell into, per this task's
standing warning). Instead, three layers of cross-checked, purely
observational tooling, all reading the engine's own already-computed
state:

1. **`REPLAY_DEBUG`'s existing `costs::take_rejection` dump** gives the
   exact rejected hand (`hand_civil`, already age-disambiguated card
   names) and the exact journal `lineno` of the rejection, for all 98
   games in one corpus pass.
2. **`REPLAY_DEBUG_ALL`'s existing `DEBUG APPLY_ONE ENTRY` dump** (one
   line per journal line actually dispatched, printing `hand_civil_before`
   for that line's own actor) gives a literal, already-resolved trace of
   every add/remove `hand_civil` underwent for that actor, in order. Diffing
   consecutive same-actor entries (`hand_civil_before[k+1] -
   hand_civil_before[k]`) recovers, for every card ever held, the EXACT
   journal line that added it and the exact line (if any) that removed it
   -- fully sidestepping the age-sibling ambiguity that sank an earlier
   attempt at this same check (see below).
3. **A raw-journal cross-check**, independent of the engine's own
   classification, scanning for any `"<Color> plays "` (excluding `"plays
   event"`, a global board trigger unrelated to any specific hand card)/
   `"elects "`/`"discovers "`/`"revolutions ... Change government to "`
   line for the SAME actor, in the window between a card's diffed
   acquisition line and the rejection line, that the diff-based trace
   (step 2) does NOT already show as a removal at that exact line number.

**Two dead ends hit and fixed while building this, both worth naming
explicitly since they are exactly the "checker disagrees with engine, so
suspect the checker" shape this task warned about**:

- **First attempt matched a bare `"<Color> plays Reserves ..."`/`"<Color>
  discovers Iron ..."` line against ANY card of that base name currently
  or ever held by that actor, without checking WHICH age-sibling instance
  the bare (age-blind) journal text refers to.** This produced ~25 false
  "unaccounted disposal" hits, almost all on the nine free-civil-action
  families (`Rich Land`, `Urban Growth`, `Engineering Genius`,
  `Breakthrough`, `Reserves`, `Patriotism`, `Frugality`, `Cultural
  Heritage`, `Revolutionary Idea`) plus ordinary techs like `Iron`/
  `Knights` a DIFFERENT player of the same game had also taken and
  developed. Root cause: these action-card families can legally be held
  in TWO copies at once (`costs::can_take_gated`'s `DuplicateCard` check
  explicitly exempts `CardType::Action`), and a bare `"plays Reserves"`
  line never says which of two same-named copies (sometimes different
  ages) got used -- the engine's own `resolve_named_card_by_effect`/
  `correct_hand_family` (`replay_common.rs`) already disambiguate this
  correctly at replay time; a naive text-only checker cannot. Fixed by
  reading the diff (step 2 above) instead of re-deriving age from the row
  age column, which is exactly the same "trust the engine's own resolved
  identity, not a re-derived approximation" precedent this doc's own
  age-lag fix (`8edfea7`) already established. Confirmed on `7522267`:
  `"Purple plays Reserves"` at line 167 is the engine's own
  `PlayActionCard card=Some("Reserves (I)")` -- it disposes the age-I
  copy, leaving the age-II copy (taken separately, later) genuinely and
  correctly still in the final rejected hand.
- **Second attempt trusted `DEBUG APPLY_ONE ENTRY`'s own `card=` field for
  `TakeCard` lines directly as the pushed identity.** Wrong: that field is
  printed from `apply_one`'s own function argument, captured BEFORE the
  `TakeCard` arm's internal `best_age_sibling(card, r.state.age_civil)`
  re-resolution runs, so it can show the classifier's default (often the
  Age-A sibling) even when the card that actually lands in `hand_civil`
  moments later is a different age. Fixed by reading `hand_civil_before`
  diffs instead of the `card=` field for identity (same fix as above,
  same lesson: the diff over real state beats any single printed field
  when a field's OWN doc doesn't guarantee it reflects the post-adjustment
  value).

**Result, full 98-game `HandFull` set, after both fixes**: every one of
517 (card, held-instance) pairs across all 98 rejected hands has a
verified acquisition line (0 "never acquired" cases -- the number of
`TakeCard` diff-events for a given name is never less than the number
currently held) AND zero unaccounted disposal-shaped lines (0/517
flagged by the independent raw-journal cross-check, step 3). A separate
full reconstruction check (replay every diffed add/remove for each actor
from scratch and confirm it lands exactly on the engine's own printed
rejected hand) also came back with zero mismatches across all 98 games --
the engine's own `hand_civil` bookkeeping is internally consistent with
itself, and independently, sound against the raw journal text.

**This is a genuine, doubly-cross-checked structural finding, not just an
absence of counterexamples**: `hand_civil` has exactly one push site
(`apply.rs::take_card`, already established) and exactly four pop sites
in the whole tree -- `h_play_leader`, `h_develop`, `h_revolution`,
`h_play_action` (plus `h_resign`'s full wipe on resignation, and
`game::antiquate`'s age cull, both irrelevant here: none of these 98
games' rejected actors have resigned, and 5.5's sibling-rule sweep
(`docs/AUDIT_HISTORY.md`) already re-confirmed the age-transition funnel
is a single, undivergeable code path). **No aggression, event, or
"discard" mechanic anywhere in this codebase ever touches `hand_civil`**
-- grepped exhaustively (`grep -n "hand_civil\." src/*.rs`, unchanged
across this pass's own re-check against the dozen new commits) -- which
matches the actual rules: the civil hand is fully private and immune to
opponent interference; only USING a card (play/develop/revolt/elect) or
outliving its age (antiquate) removes it. The `"Discard Phase N military
card(s) must be discarded"` stated-fact oracle a concurrent pass is
building for the MILITARY hand (this doc, `IllegalMove: Pop`/actor-mismatch
sections above) has **no civil analogue to build**: grepped every journal
for any civil-hand-size/limit fact BGO ever prints in-band, and there is
none -- `"Discard Phase"` is exclusively a military-hand phase; the ONLY
"civil"-adjacent text in any journal is the ordinary `"uses N civil
action"` cost clause, already fully consumed by the `ca_total` check two
sections up.

**Conclusion: the provenance-trace direction is now closed, not just
under-explored.** Combined with the four earlier eliminations (the `>=`
rule itself, filler/phantom cards, stale age-lagged cards, `ca_total`
undercount) and this pass's own two independent zero results (diff-based
acquisition/disposal trace, and a from-scratch reconstruction check), all
five explanations this bucket has ever had are now checked and closed.
**`RULES_SPEC.md`'s `>=` gate remains untouched**, per every prior
handoff's explicit instruction -- nothing in this pass's findings
supports loosening it, and the task's own standing "a clean negative is a
valid result" applies in full: the hand is not too big for any reason
this reconstruction can see, and the limit is not too small. The two
explanations that were already flagged as living OUTSIDE the replayer
(BGO client-side leniency at the exact `>=` boundary, or a rule subtlety
this doc has mis-scoped -- e.g. whether some other, not-yet-modeled source
of hand-limit capacity applies) are the only ones left; both are outside
this file's own reach (the first needs BGO's own source/behaviour, not
this journal corpus; the second needs a rules citation this pass did not
find). No code change landed this pass -- this is a documentation-only
handoff. `HandFull`'s own count (98) will keep moving as other buckets'
fixes land and let more games run deeper into it; that is expected and is
not, by itself, evidence of anything new.

## Second `IllegalMove: Pop` pass: two more root causes found and fixed (144 -> 22), the food-text-vs-stock-delta trap avoided, one well-evidenced new lead left

Picked up the bucket at 144 (this file's own prior handoff, "`IllegalMove: Pop`
handoff: two fixes landed (184 -> 144)"), starting from that handoff's own
"concrete unfixed lead": game `7522648` round 7's unexplained mid-turn
`yellow_bank` drop of 2, already diagnosed but not root-caused.

### Fix 1 (`replay_common.rs`, commit `62befa4`): `force_civil_age_at_least` fired before a stalled turn's own `end_of_turn` had actually completed

Root-caused the `7522648` lead: `economy::end_of_turn` can be interrupted by
`interact::discard_excess_military` (returns early, `false`, BEFORE running
its own production/consumption steps) and only resume once the discard
choice(s) it queued are drained by `resolve_intervening` -- which, on a
stalled turn, happens while processing the FOLLOWING journal line, and BGO
tags that following line with the age it actually occurred in, which is
routinely already the NEW age. The age catch-up
(`game::force_civil_age_at_least`, called once per line off that line's own
age column, added in the prior pass to compensate for this reconstruction's
`civil_deck` lagging the real one) ran unconditionally at the top of the
replay loop's line iteration -- so it applied `advance_age`'s §12.2.4 "-2
yellow_bank" deduction to every player BEFORE the stalled player's own
end-of-turn food consumption had run, one whole age transition early.
Confirmed on `7522648`: the journal's own "End turn ... 1 food - consumption:
1" for Orange's round-7 turn requires the pre-deduction `yellow_bank` (13,
consumption 1); this binary was computing consumption off the post-deduction
value (11, consumption 2).

Fixed by extracting the call into a new `catch_up_civil_age` helper, gated on
both `state.pending` and `state.queue` being empty -- deferring the
catch-up by at most one line (the very next line, once whatever was
outstanding has drained under the OLD age, re-fires the same still-otherwise-
unconditional call and brings `state.age_civil` up to date before anything
belonging to the new age applies). Two new tests, both confirmed RED against
a reverted (unconditional) version before being restored green.

Corpus: `Pop` 139 -> 78 (measured against the tree this pass started from,
which already had one concurrent worker's commit past the 144 baseline --
see the commit message for the exact isolated numbers), games completed 29 ->
48, mean rounds reached 11.55 -> 12.11, decisions in Age II+ 47.2% -> 49.5%.
Set-diffed by exact game ID, not just the count: 63 games cleared, 2 new
entrants (`7521963`, `7522909`) -- both traced end-to-end and confirmed to run
MUCH deeper before hitting a genuinely later, different Pop shortfall (round
11/144 actions -> round 18/271 actions; round 8/104 actions -> round 19/334
actions), not a regression.

### Fix 2 (`replay_common.rs`, commit `b257fd7`): the `produces_grants` FIFO gets permanently desynced by one foreign entry

Re-bucketed the remaining 78 by the same food-short-vs-tier-mismatch
signature the prior pass established (still ~92% food-short, ~5% tier
mismatch, matching that pass's own finding that the split is real and not an
artifact). Traced game `7523052` round 9 (short by 1, cost tier right) back
to its first divergence using `economy::end_of_turn`'s own `POST` print
against the journal's `"(now N)"` totals (never the printed per-step
production text -- see the note on that trap below) and found: Foray's own
grant (`"Purple produces 1 food; Purple produces 2 resources"`, journal
round 9) never applied. `events::food_or_resources` had run its
deterministic "resources first" guess (0 food / 3 resources) instead of the
prior pass's own correction firing.

Root cause: `prescan_produces_grants` queues EVERY standalone `"<Color>
produces N food[; ...]"` line in the WHOLE journal into one shared per-seat
FIFO, but only Foray/Raiders' exact `Special::StrongestPlayers`/
`WeakestPlayers` + `Gain`/`Lose(food_and_or_resources)` shape ever consumes
it. An `AllPlayers`-shaped grant like Development of Markets ("gains 2
resources or 2 food, player's choice") resolves through a real
`Pending::Choice` (`ChoiceKind::FoodOrRes`) that never reads `produces_grants`
at all, so ITS OWN matching-shaped line sits at the front of the queue
forever once queued. The consumer only ever PEEKED the front entry and gave
up on a total mismatch (a deliberate safety property from the prior pass,
to avoid consuming an unrelated entry) -- but never POPPED the mismatched
entry either, so one foreign line permanently blocked every later, real
Foray/Raiders correction for that player for the rest of the game. Confirmed
on `7523052`: a stray `(2, 0)` from round 5's Development of Markets sat in
front of round 9's real Foray `(1, 2)` split, whose total (3) never matched
the stale entry's total (2).

Fixed by scanning forward through the FIFO for the first entry whose OWN
total matches, removing it from wherever it sits (not just the front) --
the sibling `PlunderSplit` consumer already does exactly this, for the
identical reason, per its own doc comment (`prescan_plunder_splits`). One new
test, confirmed RED against the old peek-only version before being restored
green.

Corpus: `Pop` 75 -> 22 (the 78 above further reduced to 75 by an unrelated
concurrent worker's fix landed in between -- see the git log). Set-diffed by
exact game ID: 53 games cleared, **0 new entrants** -- a pure improvement,
no regression trace needed. Games completed 48 -> 49, mean rounds reached
12.14 -> 12.35, decisions in Age II+ 49.8% -> 51.0%.

### The food-text-vs-stock-delta trap, and how this pass avoided it

A concurrent worker (chasing `events::extra_production`) found and reported:
**BGO's journal display text for "X food" is not always equal to the real
production number, and is not ground truth** -- confirmed on a case where
the printed production text was off by one but the journal's OWN recorded
stock delta (`entering + production - consumption = "(now N)"`) reconciled
exactly against the engine's correct value. Both fixes in this pass were
built entirely on stock deltas and `"(now N)"` totals (the established
method from the prior pass's own "methodology trap" section, re-used rather
than reinvented) -- neither fix trusted a printed production-text number as
an oracle anywhere in its reasoning, so this trap did not need retrofitting
here. Worth restating for whoever picks up the remaining bucket: the
distinction is between a RECORDED fact (a `"<Color> spends/produces N
food"` clause that reconciles against other numbers in the log -- both this
pass's fixes read exactly such clauses) and DESCRIPTIVE display text (a
reveal line's own trailing summary, which does not have to reconcile and
sometimes does not).

### Breakdown of the 22 that remain (fresh `REPLAY_DEBUG=1` run against the landed tree, `b257fd7`)

| count | signature |
|---|---|
| 8 | cost matches journal's stated TOTAL, `food` short by 1 |
| 6 | same, short by 2 |
| 2 | same, short by 3 |
| 4 | `TIER MISMATCH`: computed `pop_cost` above the journal's stated cost |
| 2 | `civil_actions == 0` (unchanged from the prior pass -- still believed to be the Take/Bid handoff's own open civil-action-budget question, not re-investigated this pass) |

### New lead, evidenced but NOT fixed this pass: the age can still advance TOO EARLY, via the PRIMARY (non-catch-up) trigger, not the one Fix 1 addressed

Traced one of the 4 remaining tier-mismatch games, `7523449` round 8 (2p),
end-to-end: Orange's own Pop at journal line 128 is tagged age `"I"` (the
journal's age column does not reach `"II"` until Purple's NEXT line, `132`),
and the journal states Orange paid 3 food -- but this binary computed
`pop_cost=4`, meaning its own `yellow_bank` had already dropped from 14 to 12
(exactly the §12.2.4 "-2" shape again) DURING Purple's PRECEDING turn, one
whole real turn before the journal's own age column ever says `"II"`.

This is NOT Fix 1's bug re-appearing: `catch_up_civil_age` is only ever
handed the CURRENT line's own age, and every line up to and including
Orange's round-8 Pop is tagged `"I"` -- `force_civil_age_at_least`'s `while
age_civil < target` loop cannot advance PAST what it is asked for, so the
catch-up mechanism structurally cannot have caused this. That leaves the
PRIMARY, original trigger: `game::deal()`'s own `advance_age` call, fired
when THIS reconstruction's OWN `state.civil_deck` runs dry during an ordinary
row refill after a `Take` -- independent of the journal's age column
entirely. `Replayer::ground_row_slot` places a card's IDENTITY directly into
a row slot without draining `civil_deck` (this file's own long-standing
documented lag risk, the reason `force_civil_age_at_least`/Fix 1 exist at
all) -- but evidently, in at least this game, the direction can run the
OTHER way: this reconstruction's deck emptying SOONER than the real one did,
racing ahead of the journal's own age column by at least one full turn.

**Not root-caused further this pass**: the exact mechanism that makes THIS
binary's `civil_deck` run dry early was not isolated -- candidates not yet
checked include whether every `Take` (including ones later grounded to a
DIFFERENT identity by a subsequent line, or ones inside an auction's own
ceiling-grounding path) triggers exactly one `deal_row` refill the same way
a real draw would, or whether some path calls it more than once per real
draw. `prescan_putback_skips` was checked and ruled out as a cause -- a
paired `Take`/`PutBack` is skipped as a matched set, never applied to the
engine at all, so it cannot touch `civil_deck` either way.

**Concrete first move for whoever picks this up**: `7523449`, round 7-8
(2p) is a small, already-isolated repro (Orange's `yellow_bank` reads 14 at
the round-7 `end_of_turn post-production` print, then 12 by the very next
`applied mv=PolPass -> current=0` print at the start of round 8, with only
Purple's own round-7 actions -- two `Take`s among them -- in between). Add a
temporary `REPLAY_DEBUG_ALL`-gated print of `state.age_civil`/
`state.civil_deck.len()` around `game::deal`'s own `advance_age` call site
(`game.rs`, inside `fn deal`) and re-run this one game to catch the EXACT
`Take` that empties this reconstruction's deck, then compare that count
against how many real Age I cards the rulebook's own deck-size table says a
2-player game holds.

### What was RULED OUT this pass (don't re-check these)

- **`interact::discard_excess_military`/the discard-resolution path as the
  cause of a mid-turn `yellow_bank` drop** -- re-confirmed (again) never
  touches `yellow_bank`/`food`/`resources`; the drop was always the
  age-advance deduction landing at the wrong TIME, not a discard side effect.
- **A blind, delta-only gate as a safe way to consume `produces_grants`** --
  already ruled out by the prior pass (the 184 -> 281 regression); this
  pass's fix (scan forward for a matching total, still gated on the revealed
  card's own shape from the prior pass) does not reopen that risk, since the
  shape gate is unchanged and only the SEARCH WIDTH inside an
  already-shape-gated FIFO changed.
- **`prescan_putback_skips` as a `civil_deck` desync cause** -- read in full;
  a matched Take/PutBack pair is never applied to the engine at all (both
  lines skipped as a set), so neither can touch `civil_deck` or trigger a
  refill.
- **The printed per-step production TEXT as ground truth** -- explicitly
  avoided this pass per a concurrent worker's finding (see above); both
  fixes here read only recorded spend/produce clauses and `"(now N)"`
  stock totals, never a reveal line's own descriptive summary.

### Final numbers, this pass

| | before this pass | after |
|---|---|---|
| `IllegalMove: Pop` | 144 (139 on the tree this pass actually started from) | **22** |
| mean rounds reached | 11.55 | **12.35** |
| decisions in Age II or later | 47.2% | **51.0%** |
| games completed | 29 | **49** |

**Concrete first move for whoever picks this up**: game `7523449`'s
early-age-advance repro, above -- it is the single largest un-diagnosed
mechanism left in this bucket (4/22 tier-mismatch games share its
signature) and, if it turns out to be a systemic `civil_deck`-refill bug
rather than specific to this game, would plausibly also explain some of the
remaining 16 food-short games (the same `yellow_bank` value drives both the
Pop-cost tier and the food-consumption rate, exactly the compounding shape
both of this pass's fixes turned out to be).

## The discard-phase hand-size oracle: landed, validated, and USED -- a sharp, corpus-wide signature for the pre-existing hand-SIZE undercount, root cause still open

Picked up exactly where the previous pass's own "concrete, unused lead"
left off: `corpus::classify` (`corpus.rs:658`, now with the new code around
it) matched every `"Discard Phase N military card(s) must be discarded"` /
`"No Discard Phase"` line and threw the count `N` away as bare
`LineOutcome::Bookkeeping`. That count is PUBLIC, legal-to-use information
stating exactly how many cards a player's hand-military reconstruction
OUGHT to need to discard at that turn -- a direct oracle for the hand-size
drift this file's last several passes have chased indirectly, several
rounds after the fact, via `StuckPending`.

### Validated before trusting it -- a live caveat from a concurrent worker paid off

Mid-pass, the coordinator relayed a live finding from a different worker on
this project: a DIFFERENT journal field (a food total) turned out to be
descriptive RENDERING text that could disagree with the engine's own
correct value by one -- not every printed number in this journal is a
recorded fact. Checked directly, per the coordinator's own suggested
method, before building anything further on this lead:

- **BGO logs the discard-phase fact TWICE, independently, every turn**: the
  `"Discard Phase N ..."`/`"No Discard Phase"` announcement, and a separate,
  unambiguously-real-action `"<Color> discards N cards"` resolution line
  (`corpus::classify` already recognised THIS shape too, as
  `ActionClass::Discard` -- and, same bug, independently, also threw its own
  count away, `card: None`, since resolving `Pending::Choice(DiscardMilitary)`
  never needed the number). Corpus-wide, a pure-text reconciliation of the
  two (no Rust replay involved) found they agree on 45,590 of 46,009 (99.1%)
  individual `(actor, round)` entries; the 0.9% that disagree are not spread
  evenly -- concentrated in patterns consistent with the ALREADY-documented
  "BGO logs the true final turn's End-of-turn lines twice" artifact
  (`replay_game`'s own `EndTurn` handling), not a third, independent flaw.
- **On real game `7522614`** (one of the games that currently replays clean
  through to `state.game_over`): every one of its 30 announcement/resolution
  pairs agree with EACH OTHER end to end, AND the resolution line's own
  count (a real action, not descriptive text) disagrees with THIS BINARY's
  own reconstruction at round 4 specifically -- Orange's real hand needed
  exactly 1 discard there (both BGO renderings independently say so); this
  binary's own `hand_military_len` computes 2 short of the true limit at
  that checkpoint, entering `discard_excess_military`'s loop TWICE and
  evicting a card the real game never touched.

This is decisive: the journal field is corroborated by an independent,
unarguably-real action, on a game this binary otherwise replays correctly
end to end, and it disagrees with THIS BINARY's own state -- not with
itself. `prescan_discard_phase_oracle` (`replay_common.rs`) encodes this
validation as a permanent GATE, not a one-off check: an `(actor, round)`
entry only enters the trusted oracle map when both journal renderings
agree; the ~0.9% that don't are silently dropped (not reported as a hand-
size divergence, since a journal self-inconsistency is not evidence about
this binary's own reconstruction).

### The instrument, landed on its own (commit `805c9ac`)

`Replayer::check_discard_phase_oracle` is called from the `EndTurn` dispatch
arm, at the exact point (right after `resolve_intervening` succeeds, right
before `try_apply(Move::EndTurn, ...)`) where `hand_military.len()` is
guaranteed to equal exactly what `interact::discard_excess_military` --
the very next thing that runs, as step 1 of `economy::end_of_turn` -- is
about to read, mirroring that function's own `limit` formula exactly
(`military_actions + military_hand_limit`). Read-only: never mutates
`self.state`, only this struct's own oracle bookkeeping. `GameResult` gained
three new fields (`discard_oracle_divergence` -- the FIRST diverging
checkpoint only, `discard_oracle_checked`, `discard_oracle_agreed`);
`replaystats` prints a new "Discard-phase hand-size oracle" report section.

**Confirmed behaviour-preserving** before landing: the stop-reason histogram
(every bucket's count and reason string) is byte-identical between a run
before this instrument and a run after it -- this changes no replay
decision, it only observes. Seven new tests (`replay_common.rs`), full-
sentence names, covering the parser shapes and the agree/disagree gating
logic directly (`discard_phase_oracle_drops_an_entry_where_the_two_journal_
renderings_disagree` pins the exact validation property above).

### Corpus-wide result: a very sharp, very early, very consistent OVERCOUNT

Run against the corpus (1,011 games, tree at commit `805c9ac`):
**28,694 checkpoints had a trusted journal count to check against; 17,970
(62.6%) matched exactly. 992/1,011 games (98.1%) have at least one
checkpoint disagree.** Bucketing the FIRST divergence per game by
signature:

| round of first divergence | count | share |
|---|---|---|
| 3 | 101 | 10.2% |
| **4** | **769** | **77.5%** |
| 5 | 98 | 9.9% |
| 6 | 17 | 1.7% |
| 7-12 | 7 | 0.7% |

Rounds 3-5 alone account for 968/992 (97.6%) of every first divergence in
the whole corpus -- this is not a diffuse "hand tracking drifts eventually"
problem, it is a sharp, early, near-universal signature. The divergence's
own sign and size:

| `reconstructed_excess - journal_excess` | count |
|---|---|
| +1 | 660 |
| +2 | 304 |
| +3 | 26 |
| +4 | 2 |

**Always positive** (this binary's own reconstruction is never LOWER than
the journal at the first divergence, only higher) -- a systematic OVERCOUNT,
not noise scattered around zero. By actor: Orange 584, Purple 363, Green 40,
Grey 5 -- roughly proportional to how often each seat appears across 2p/3p/4p
games in the corpus (not concentrated in one seat, ruling out a "only the
first-to-move player" theory).

### Two games hand-traced end to end -- draws and the limit are BOTH confirmed correct, and the paradox is real

`7522614` (Orange, first divergence round 4: journal excess 0, this binary
computes 2, `hand_military_len=4 limit=2`): every `economy::end_of_turn`
`draw_military_step` print for rounds 2 and 3 (`military_actions_unused=2
n_drawn=2`, both) matches the journal's own `"draws 2 military cards"`
EXACTLY -- the draw side is not the cause, confirmed independently a second
time (the Sixteenth pass already ruled this out for a different game).
`military_actions=2` at this checkpoint is Despotism's own base value
(`RULES_SPEC.md` 1.2/8.1: "4 CA, 2 MA", matching `effects::Stats::default`'s
own tested value, `military_hand_limit=0` since no card/government grants
extra) -- the LIMIT formula is also not the cause, confirmed directly
against the rulebook rather than assumed. No `DeclareWar`/`PlayAggression`/
`ProposePact`/`PlayTactic` reveal or any other `hand_military`-mutating call
fires for this player before round 4's checkpoint (checked against every
non-test `hand_military.push` site in the engine, `interact.rs`/`combat.rs`/
`legal.rs`/`economy.rs`/`events.rs` -- none apply here). So `hand=4` is
EXACTLY what this binary's own draws predict (0 + 2 + 2), and yet the real
game only needed 1 discard at this checkpoint (both BGO renderings agree),
implying the true hand was 3, not 4. **Draws are right, the limit is right,
and the hand is still one too many -- this is the open paradox, not yet
resolved.**

`7523353` (Orange, first divergence round 5, not round 4 -- this game's
rounds 2/3 each only draw 1 card, not 2, because the player spent 1 of their
2 military actions on a `"takes ... uses 1 military action"` civil-hand
Take that round, correctly reducing THIS binary's own `military_actions_
unused` and its draw count to match -- confirms the draw-reduction-from-MA-
spending mechanic is ALSO already modelled correctly): pre-draw hand reaches
3 only at round 5 (1+1+1), one round later than `7522614`'s game, but the
SAME shape once it does -- `hand_military_len=3 limit=2`, this binary
computes excess 1, journal says 0, implying true hand is 2, not 3. Tested
and FALSIFIED the "current turn's unused military-action spending also
lowers THIS turn's discard limit, not just the draw count" theory directly
against this game (round 5 has no MA-spending line at all, so that theory
predicts no divergence here -- it still diverges).

### What was ruled out chasing the mechanism (do not re-check these)

- **The draw count formula** (`economy::end_of_turn`'s `military_actions.
  clamp(0,3)` / `military_actions_unused`) -- confirmed exact against the
  journal's own printed counts on two separate games this pass, a third
  confirmation after the Sixteenth pass's own.
- **The military hand limit formula** (`military_actions + military_hand_
  limit`) -- confirmed exact against `RULES_SPEC.md`'s own cited Despotism
  values, not merely assumed unchanged from a previous pass.
- **A single-seat artifact** (e.g. "only the player who moves first is
  wrong") -- the actor breakdown above is proportional to seat frequency
  across the corpus, not concentrated in seat 0.
- **The current turn's own military-action spending changing THAT turn's
  discard limit** (as opposed to just its draw count) -- falsified directly
  on `7523353` round 5 (no MA spent that turn, still diverges).
- **A pure-Python, journal-only reconstruction with a FIXED `limit=2`**
  as a cross-check tool: diverges rapidly and unboundedly by round 15+ in
  BOTH games sampled, because `military_actions` genuinely grows over the
  game (government changes, techs, wonders) and a fixed baseline does not
  track it -- this is a dead end for validating rounds much past the
  opening, NOT evidence the round-4/5 signature itself is spurious (this
  binary's own Rust-computed `limit`, which DOES track government changes
  via `effects::state_stats`, is what the real oracle check above uses, and
  IT stays right far more often after the opening rounds -- 62.6% overall
  agreement despite 98% of games having failed at least once early).
- **Deliberately NOT re-litigated**: whether BGO's real step order is
  discard-before-production/draw or the reverse -- `RULES_SPEC.md` section 9
  already marks this "RESOLVED" with rulebook/Chronicle-of-Life/FAQ page
  citations, and the raw journal's own TEXTUAL line order (`"Discard Phase
  N"` sometimes before, `"No Discard Phase"` sometimes after, the following
  `"End turn"` line in the SAME game) is a UI-submission artifact, not
  evidence of engine step order -- do not use it to argue for reordering
  `economy::end_of_turn`'s steps.

### Concrete next step for whoever picks this up: audit the FIRST few rounds card-by-card on a minimal repro, not the whole engine

`7522614` is about as clean a repro as this project gets: 2 players, no
military plays, no reveals, no events before the first divergence, draws
and limit both independently confirmed correct, and the paradox is fully
isolated to "4 real draws should be exactly 4 real cards, and the real
game's own two independent facts say otherwise." The next move is a
card-by-card audit of what `interact::discard_options`/`DiscardSolver`/
`ground_*` do NOT touch here (none of them fire before round 4 in this
game) versus what a closer reading of BGO's OWN client behaviour around
the very first military draw might reveal -- e.g. whether Age A (round 1,
which never draws) secretly reserves or pre-assigns something that Age I's
first draw (round 2) should NOT re-grant, or whether `draw_military`'s
reshuffle/seed logic has an off-by-one specific to a fresh age's first call
that a larger, noisier game would never expose as cleanly. The oracle
(`REPLAY_DUMP`-free -- just run `replaystats` and read the new "Discard-
phase hand-size oracle" section, or grep the divergence list for a specific
game) makes any future theory falsifiable in minutes rather than a full
pass: land a candidate fix, rerun `replaystats`, and check whether the
992-game divergence count and the round-4 concentration actually drop,
using the same exact-game-ID-set-diff discipline the actor-mismatch
bucket's own passes established (a raw count drop is not automatically
progress; diff the sets).

### Follow-up: the `food_or_resources` loss-side gap above IS fixed (`1df71b9`)

The "not fixed this pass" lead two sections up was picked back up the same
session, once `b257fd7` (a concurrent worker's fix for the SAME underlying
bug's GAIN half, chasing `IllegalMove: Pop`) landed and made the remaining
gap small and concrete rather than requiring new choice machinery from
scratch: `resolve_political_decision`'s correction loop already gated on
`WeakestPlayers`/`Lose`, but its own delta check unconditionally skipped
every NEGATIVE delta, so a loss (Raiders/Crime Wave) was never corrected,
only a gain was. Added the mirror-image `parse_spends_grant_line`/
`prescan_spends_grants` (same shape as the existing `produces` pair, for
standalone `"<Color> spends N food[; N resources]"` lines) and extended the
correction loop's delta-sign branch to use it. New test confirmed red
against the old unconditional skip. Full corpus, set-diffed by exact game
ID: 49 -> 57 completed, 8 gained, 0 lost. `IllegalMove: Build` 109 -> 88,
`Upgrade` 59 -> 32, `WonderStep` 54 -> 36.

## Cost-cluster pass: one shared disease confirmed and fixed (Trade Routes food-as-resource fold), one ENGINE bug fixed (Shakespeare built-not-developed gate), and the dominant remaining member (`IllegalMove: Take`'s `HandFull` sub-bucket) investigated to a well-evidenced open lead, NOT fixed

Assigned the largest aggregate failure cluster: `IllegalMove: Take`/`Build`/
`Upgrade`/`WonderStep` plus the `UnrecoverableHiddenInfo: build cost
mismatch` and `ParserGap: <card>'s take cost` tails, with an explicit
instruction to establish FIRST whether these are one disease or several.

**Verdict: at least two distinct diseases inside this cluster**, not one and
not four. Bucketed by signature per the standing method
(`REPLAY_DUMP_BUCKET`, `REPLAY_DEBUG`'s `TAKE REJECT` dump) before forming
any theory, per this doc's own prior lessons.

### Disease 1 (found and FIXED, commit `753b7ba`): Trade Routes Agreement's food-as-resource conversion was parsed off ONE clause short on Build/Upgrade/WonderStep lines

The `build cost mismatch` tail's single-card names (Warriors, Printing
Press, Knights, Swordsmen, Drama, Philosophy, Religion, Bread and Circuses,
Bronze, Agriculture, Alchemy, Movies, Journalism, Organized Religion -- 14
distinct names, ~39 games) were exactly the "gift" the brief promised:
almost every one of them shared the identical two-clause journal shape
`"<Color> builds/upgrades X <Color> spends N resource(s); <Color> spends 1
food"`. Traced end to end on game `7523070` (round 6, `"Green builds
Warrior Green spends 1 resource; Green spends 1 food"` for a 2-resource
Warrior): Green is Trade Routes Agreement side A (`"Green proposes Trade
Routes Agreement to Orange Green is A; ... Once per round A can use 1 food
instead of 1 resource"`, line 80), and had 6 resources in hand at the time
-- not short, this is a deliberate once-per-round conversion, not a
shortfall workaround. `total_paid_for_build` (the function backing the
`want`/`got` mismatch check) read only the FIRST `"spends"` clause, silently
dropping the food clause and under-counting the true payment by exactly the
converted amount -- exactly the "multiple `spends` clauses on one journal
line, checker reads only the first" bug shape the brief called out, and the
build/upgrade/wonder-stage mirror of the ALREADY-fixed `IncreasePopulation`
food-then-resource fold (`spent_resource_after_food`,
`Move::TradeResourceAsFood`).

Fix: new `spent_food_after_resource` (reads the trailing food clause,
whatever precedes it) and a shared `convert_trade_food_shortfall` helper
(mirrors `IncreasePopulation`'s existing `TradeResourceAsFood` fold, applies
`Move::TradeFoodAsResource` the OTHER direction, gated on the journal's own
stated total matching this binary's own cost EXACTLY so a genuinely wrong
cost still surfaces honestly rather than being masked -- same safety gate
the Pop fix established), wired into `Build`/`Upgrade`/`WonderStep` alike so
the rule lives in ONE place, not three. Also fixed the SAME-pass Shakespeare
Library/Theater discount bug below in the same commit (unrelated mechanism,
same cluster).

**Pitfall hit and fixed before landing**: the first version called
`costs::wonder_stage_cost` unconditionally in the WonderStep arm, and its
`debug_assert!(!p.wonder.is_none())` fired on game `7522899` (a
reconstruction whose state had already diverged enough to think no wonder
was in progress at that line) -- under `[profile.difftest]`'s INHERITED
`panic = "abort"` (it `inherits = "release"` and never overrides `panic`),
this aborted the ENTIRE corpus run, not just that one game, exactly the
standing warning this project's brief carries. Guarded behind
`!p.wonder.is_none()` first; a reconstruction that's already wrong there
just falls through to its previous, honest `IllegalMove` instead.

Measured, set-diffed by exact game ID, full 1,011-game corpus (base
`df5cfa0`, after a concurrent worker's `food_or_resources` loss-side fix):
completed games 57 -> 58 (`7521742` newly completes, **zero regressions** --
`comm -23`/`comm -13` on the sorted completed-ID lists confirmed this
exactly, not by count alone). `build cost mismatch` tail: 14 distinct card
names (~39 games) -> 3 (`Journalism`, `Organized Religion`, `Movies`, all
sharing a DIFFERENT, separate `"using Urban Growth"` shape -- NOT chased
this pass, a plausible next lead but small, 3 games). `IllegalMove: Build`
88 -> 94, `Upgrade` 32 -> 34 (both grew -- expected exposure from games
running deeper, not a regression, per this doc's own standing pattern).
`WonderStep` 36 -> 34. `Take` unaffected (civil-action cost, not resource
cost).

### Disease 2 (found and FIXED, same commit): William Shakespeare's Library/Theater discount checked "developed" instead of "built" -- ENGINE BUG

Chasing one of the 3 residual `build cost mismatch` outliers turned up an
unrelated second bug on game `7520718`: Orange developed Printing Press (a
Library-kind tech, `"discovers"`, paying its science cost) at round 13 but
did not BUILD one (place a worker, pay its resource cost) until round 15;
in between, at round 14, Orange correctly paid FULL price for a Drama
(Theater) build per the journal, but this binary's `costs::build_cost_for`
computed a Shakespeare-discounted price one build too early. Root cause:
`build_cost_for`'s (and `tech_cost`'s, same pattern, the develop-cost side
of the identical leader ability) `has_library`/`has_theater` checks tested
mere presence in `p.techs` (every DEVELOPED technology, worker or not) --
developing and building are two separate payments (RULES_SPEC §3.5 vs
§3.7) and Shakespeare's own text needs the counterpart building actually IN
PLAY. `effects.rs`'s sibling Shakespeare ability
(`CulturePerLibraryTheaterPair`, via its own `workers_on` helper) already
gated on `slot.workers > 0` correctly -- this cost-side check was the one
place that didn't match it.

**ENGINE BUG, confirmed**: `costs::build_cost_for`/`costs::tech_cost` are
shared by `legal.rs` (legality) and `apply.rs` (application), so this
corrupted both self-play legality and training whenever Shakespeare was in
play with a developed-but-unbuilt Library or Theater -- exactly the
highest-value kind of cost bug this project's brief calls out. Fixed by
adding `&& slot.workers > 0` to all four checks (build-cost Library check,
build-cost Theater check, tech-cost Theater check, tech-cost Library
check); two new regression tests
(`build_cost_for_shakespeare_ignores_a_library_that_is_developed_but_not_built`,
`tech_cost_shakespeare_ignores_a_theater_that_is_developed_but_not_built`),
both confirmed RED against the reverted pre-fix code, then green.

### Disease 3 (investigated, NOT fixed -- open lead): `IllegalMove: Take`'s `HandFull` sub-bucket, now the cluster's dominant remaining member

Post-fix corpus counts: `Take` 129 (up from the brief's 119 baseline --
unrelated growth, see the concurrent `food_or_resources` fix's own report),
now clearly the largest of the four `IllegalMove` buckets (`Build` 94,
`Upgrade` 34, `WonderStep` 34). Bucketed the REJECTION REASON, not just the
move kind, via the existing `REPLAY_DEBUG`/`TAKE REJECT` dump
(`costs::take_rejection`, already purpose-built for exactly this): of 123
named rejections corpus-wide, 109 are `HandFull`, 12 `Budget`, 1 each
`WonderInProgress`/`WonderBudget`. `HandFull` is overwhelmingly the
dominant shape inside the dominant bucket.

**Enriched the existing `TAKE REJECT` debug line** (this commit) with
`government`, `s_civil_actions`/`s_civil_hand_limit` (the two
`effects::state_stats` components `costs::civil_hand_limit` sums) and
`completed_wonders`, per the standing "use diagnostics that already exist"
method -- this is a refinement of an existing print, not a new instrument.

**The pattern, confirmed corpus-wide**: `hand_civil_size == civil_hand_limit`
in ALL 109 `HandFull` rejections, never strictly greater (expected under the
SETTLED `>=` gate, but confirms every one is a boundary case, not a wild
overcount). Government distribution: 70 Despotism, 17 Monarchy, 14
Constitutional Monarchy, 4 Theocracy, 3 Republic, 1 Democracy -- i.e. the
STARTING government alone accounts for nearly two thirds, and 5 of the 109
have `completed_wonders: []` (zero wonders, zero leader/pact complexity at
all). This rules out any theory that needs a leader/wonder/pact edge case as
the general explanation.

**Traced two full examples end to end, by hand, against the journal's own
recorded actions (not just the failing line):**

1. Game `7523357` line 442 (`"Green takes Scientific Method"`, hand size 7,
   limit 7): government IS genuinely Constitutional Monarchy at this point
   (Green really did `"discovers Constitutional Monarchy"` at round 9, no
   separate `"revolutions"` line needed -- RULES_SPEC §3 item 9 confirms
   developing a government via the ordinary `"discovers"` action, paying the
   HIGHER science cost, **IS** the peaceful government change; the SEPARATE
   `"X revolutions Change government to Y"` line is only the VIOLENT path,
   paying the LOWER cost and burning all CAs. This doc's author initially
   suspected an auto-switch bug here and it was a false lead -- ruled out).
   Base `s_civil_actions=6` (Constitutional Monarchy's own printed `CA=6`,
   cross-checked exactly against `sources/bga_throughtheages_material.inc.php`)
   plus `s_civil_hand_limit=1` (Library of Alexandria's printed "1 extra
   civil card in hand", built and completed round 5, also cross-checked
   against the same source) = limit 7, matching this binary's computation
   exactly. Manually re-derived Green's full civil-hand history from the
   journal's own take/discover/play lines (all 7 cards accounted for, none
   of them played/discovered/discarded before line 442) -- hand really is 7
   by the journal's own record too. No leader-effect explains the gap
   (leader shows `none`: Joan of Arc, elected round 6, is apparently
   ALREADY dead by round 12's `Iconoclasm` -- but Joan of Arc grants no
   CA/hand-limit bonus either way, so this is a dead end for THIS bug, not
   pursued further here).
2. Game `7523003` line 222 (`"Green takes Swordsmen"`, hand size 4, limit
   4): pure Despotism (`CA=4`), zero wonders, zero pacts (grepped the whole
   journal for `"Green proposes"` -- none), leader Michelangelo (no
   CA/hand-limit effect). Hand `["Urban Growth (I)", "Drama", "Rich Land
   (I)", "Rich Land (I)"]` -- the duplicate `Rich Land (I)` is real, not a
   double-count bug (`count: [2,2,2]` in `card_table.rs`, confirmed against
   the same official source: two genuine physical copies at every player
   count). Manually re-derived Green's full hand history again: all 4 cards
   independently confirmed against the journal's own take/build/discover
   lines, including which of two ambiguous `"Urban Growth"` copies a
   `"using Urban Growth"` line consumed (does not change the COUNT either
   way). The just-colonized `"Strategic Territory"` (line 220, one line
   before the failure) grants only `+2 strength` and a one-time military
   card bonus, no CA/hand-limit effect (cross-checked against the official
   source).

**Conclusion: ruled out, corpus-wide or on these two examples specifically**:
government mis-identification, wonder-bonus mis-modeling, leader-ability
mis-modeling, pacts, colony/territory bonuses, `hidden_civil` interference
(always 0 in every replayer construction site, confirmed by grep), and
double-counting of legitimate same-named multi-copy action cards. The base
CA/hand-limit numbers for every government and wonder involved were
cross-checked against `sources/bga_throughtheages_material.inc.php` and
matched this binary's `card_table.rs` exactly in every case checked.

**NOT fixed.** Both worked examples show this binary's computed hand size
AND its computed limit both independently correct against the journal's own
recorded actions and the official card data -- yet the real BGA game
allowed the take anyway. Per the standing instruction, `RULES_SPEC.md`'s
hand-limit rule and `costs::take_gate`'s `>=` are SETTLED and were NOT
touched or second-guessed. Whoever picks this up next: the two example
games above are already fully reconciled and do not need re-deriving, only
extending -- a good next step is checking whether BGA's REAL rule differs
from RULES_SPEC's text in some narrow way `HandFull`'s prior (military-hand,
different bucket, already fixed) passes did not need to consider, e.g.
whether the take is legal specifically when it's the LAST civil action the
player will spend this turn (a "you may exceed the limit by exactly the
card you're about to immediately play down" carve-out), or whether
`civil_hand_limit` should be evaluated against a DIFFERENT snapshot of
`ca_total` than `effects::state_stats` currently returns. The enriched
`TAKE REJECT` debug line (this commit) already gives government/wonder/CA
breakdown for free on every future rejection.

### What was NOT investigated this pass

`Budget` (12) and the two `Wonder*` (1 each) `Take` sub-reasons; the
`ParserGap: <card>'s take cost` tail (Engineering, Reserves, Satellites,
Civil Service, Internet, Multimedia, Air Forces, Tanks, Engineering Genius
-- 9 games, all single-card, a plausible "gift" lead per the brief's own
framing but not opened this pass); the residual 3-game `"using Urban
Growth"` `build cost mismatch` shape.

### Card-by-card audit on `7522614`: `ground_military_hand` DEFINITIVELY
### ruled out for this repro; root cause FOUND AND FIXED
### (`PrepareEvent`'s net-zero push, below) -- 992 -> 941 divergent games,
### 49 -> 66 games completing

Picked up the prior pass's own "concrete first move": named every card in
Orange's `hand_military` at the round-4 checkpoint on `7522614` and traced
each one's origin via `REPLAY_DEBUG_ALL`, rather than inferring from counts.

**The four cards, individually traced**: `["Legion", "Aggression: Enslave",
"Military Bonus (defense 2 / colonization 1)", "Uncertain Borders"]`.
- Round-2 checkpoint (before round 2's draw): `hand=[]` (0 cards).
- Round-2 draw (`economy.rs:689`, the ordinary `draw_military_step` site,
  `military_actions_unused=2 n_drawn=2`): adds `Legion`, `Aggression:
  Enslave`.
- Round-3 checkpoint (before round 3's draw): `hand=[Legion, Aggression:
  Enslave]` (2 cards) -- matches "No Discard Phase" trivially (2 <= limit 2).
- Round-3 draw (same site, `unused=2 n_drawn=2`): adds `Military Bonus
  (defense 2 / colonization 1)`, `Uncertain Borders`.
- Round-4 checkpoint: all 4 cards present, exactly the 2+2 sum, no more no
  less.

**`ground_military_hand` (the task's hypothesis 1, the "known suspect") is
definitively ruled out for this specific repro**, not by inference but by
exhaustive site coverage: every non-test `hand_military.push`/
`ground_military_hand` call site in the engine (`economy.rs:689`,
`interact.rs:522` pact-refuse, `interact.rs:1492`/`events.rs:1173`
event-immediate-effect draws, `replay_common.rs:1253` `PrepareEvent`
grounding, and every `ground_military_hand` call gated on a
`DeclareWar`/`PlayAggression`/`ProposePact`/`PlayTactic` reveal) was
checked against the raw journal for `7522614` rounds 1-4: **zero** War/
Aggression/Pact/Tactic reveals fire for Orange (or target Orange) before the
round-4 checkpoint (`awk` over the raw journal for
`war|aggress|against|defend|tactic` across rounds 1-4 returns nothing).
There is no reveal event for the suspect mechanism to fire on -- the whole
4-card hand is ordinary `draw_military_step` output, verified card-by-card,
not inferred from a count.

**A second candidate, chased and also ruled out**: `CardType::Event` cards
(both the per-round "Development of X" global events AND the
player-drawable ones like `Rebellion`/`Uncertain Borders`/`Civil Unrest`)
share the `is_civil_row() == false` bucket with real military cards in
`game::build_deck`, so they get shuffled into `military_deck` and are
drawable via the ordinary `draw_military()` path -- `Uncertain Borders`
(one of the four cards above) is exactly this type, which looked at first
like a deck-composition bug (event cards leaking into a hand where they do
not belong). It is not a bug: `apply.rs::h_prepare_event` (§5.2) confirms
this is the REAL mechanic -- a player draws these event cards into their
military hand exactly like any other military card, and may later "prepare"
one from hand into `future_events` during a Politics Phase, banking culture
(`state.players[idx].hand_military.remove_first(card)` there is the intended
removal path, gated on that specific political action, which does not fire
for Orange in this game either). Confirmed correct, not touched.

**The sharper empirical signature**: cross-referencing the discard-phase
oracle divergence list against `REPLAY_DEBUG_ALL` traces on three 2-player
games with different draw magnitudes (`7522614`: draws 2+2=4, true hand 3;
`7522612`/Purple: draws 1+2+3=6 at its first divergence (round 5), true
hand 4, AND the identical shape recurs independently at that same game's
rounds 6 and 7 with fresh draws each time, not carried-over drift;
`7523353`/Orange: draws 1+1+1=3, true hand <=2, unverified via the
blindly-trusted "No Discard Phase" case but consistent) fits, exactly, in
every checkpoint tested: **every `draw_military_step` call for a player,
except their literal first-ever one (always round 2, since round 1 never
draws for anyone under any government), yields exactly 1 fewer real card
than the formula/journal-draw-text says.** This is NOT a "slow leak that
accumulates over many rounds" -- the deficit is fully present the moment it
becomes visible (matching the coordinator's own independent derivation: with
Despotism's real 2 MA, round 2 and round 3 checkpoints are mathematically
uninformative regardless of ground truth, since hand can only reach 0 and 2
against a limit of 2 -- round 4 is the first checkpoint that can show
anything at all, so "-1 by round 4" and "-1 out of the gate" are the same
statement, not a contradiction).

A competing theory -- "the *limit* is undercounted by a flat +1, not the
hand" -- was tested and **falsified** against `7522612`: a flat `limit+1`
predicts a constant diff of exactly 1 between reconstructed and journal
excess regardless of hand size, but `7522612`/Purple's round-5 checkpoint
shows diff=2 (reconstructed excess 3, journal excess 1), which only the
hand-side "-1 per draw after the first" theory predicts correctly (deficit
accumulates as `x_3 + x_4 = 1 + 1 = 2` over the two draw rounds since the
first).

**The coordinator's MA-charging theory (a human action whose military-action
cost this binary fails to charge, inflating `military_actions_unused` and
therefore the draw count) was checked directly against `7522614` and
does NOT apply there**: Orange's rounds 1-3 contain zero military-action-
consuming activity of any kind -- no unit build/upgrade, no tactic play, no
aggression/war declared by or against Orange, hence no defense either.
Every action line in that window is a plain civil Take/Build/increase-
population. The deficit in this specific repro fires with a completely idle
military side, so whatever causes it cannot be *only* a missed MA-charge on
an action Orange never took. (The theory may still be real and may explain
OTHER games in the corpus that do have a relevant action in the window --
that was not re-checked here for lack of budget -- but it cannot be the
whole story, since `7522614` diverges with nothing to charge.)

**Not located by draw-side review**: the draw formula
(`military_actions.clamp(0,3)`, `economy.rs:689`), the reset-actions step
(`economy.rs` end of `end_of_turn`, step 5), `effects::state_stats`'s
government/tech/leader bonus computation, and the RULES_SPEC citations for
both the draw rule (§6.6.4) and the hand-limit rule (§6.7) were all read in
full and match the code exactly -- because the mechanism was never on the
draw side at all. See the fix below.

### FOUND AND FIXED: `resolve_political_decision`'s own `PrepareEvent` push
### is a net-zero wash where a real player nets -1

The coordinator spotted it from the trace above, not from the draw code:
`replay_common.rs`'s `resolve_political_decision` (the handler for a
`"<Color> plays event"` line -- §5.2, preparing an event card from hand into
`future_events`) reads:

```rust
self.state.players[decider as usize].hand_military.push(prep.card);
let mv = Move::PrepareEvent { card: prep.card };
...
apply::apply(&mut self.state, mv); // apply.rs::h_prepare_event: hand_military.remove_first(card)
```

A real player who prepares an event plays a card **they already held** --
their hand goes from N to N-1. This binary's own `push` immediately followed
by `apply`'s own removal of that same identity is a WASH: N -> N+1 -> N.
Net zero, every single preparation, permanently overcounting the
reconstructed hand by one card each time it happens -- and preparing an
event is an ordinary Politics-phase action taken most turns by most
players, which is exactly why `7522614`'s "-1" fires with a completely idle
MILITARY side (no war/aggression/tactic in sight): the action responsible
is a *civil-looking* Politics-phase click, not a military one. Confirmed on
`7522614` directly: "Orange plays event ... Current event: A / Development
of Crafts" is a `PrepareEvent` line inside round 4's own action phase,
before that round's discard checkpoint -- exactly one preparation, exactly
the `-1` the oracle demanded.

**Why this also explains the multi-round recurrence on `7522612`/Purple**
(the same shape reappearing fresh at rounds 5, 6 and 7, not carried-over
drift): that game's Purple journal shows a `"Purple plays event ... scores
1 culture"` line at rounds 4 and 6 too -- one preparation, one stray `+1`,
each time, independently, matching the per-occurrence (not per-round, not
accumulating) shape the data actually showed.

**The fix** (`replay_common.rs`, `resolve_political_decision`, right before
the existing `push`): pop exactly one card of **unknown provenance** from
`hand_military` first -- never a card `DiscardSolver::needed_after` says
this player is later observed playing by name (the identical "never touch a
card with known identity" rule `ground_bid_ceiling`, a few dozen lines
above in the same file, already uses for the same reason) -- so the whole
push-then-apply-removal sequence lands on N-1 instead of N. Leaves the old
net-zero behaviour alone (does not touch/underflow anything) when no
disposable filler exists. **This is a REPLAYER-only fix**: the push site it
touches (`resolve_political_decision`) exists only to reconstruct a hidden
hand from a public journal during replay; the real engine's own
`apply::h_prepare_event` (used identically in real self-play) was never
wrong -- it already does a plain `remove_first`, which is correct against a
REAL hand that actually had the card. The bug was replay-side bookkeeping
inventing a phantom card to remove instead of removing a real one.

**IMPORTANT, for whoever re-attempts a fix in this area**: this exact push
site was tried once before, routed through `ground_military_hand`'s own
swap-instead-of-grow logic, and it REGRESSED badly (the stall bucket went
171 -> 305) -- see this file's earlier "military hand" sections. This pass
did NOT reuse that helper; it uses the narrower `needed_after`-gated pop
described above instead, specifically because the earlier regression's own
diagnosis was that swapping there exposes a SEPARATE, pre-existing
undercount elsewhere. That risk was taken seriously and MEASURED, not
assumed away -- see the corpus numbers below.

**Two new tests** (`replay_common.rs`), confirmed RED against a reverted
version of the fix before being restored green:
`preparing_an_event_the_player_already_held_shrinks_their_hand_by_one_not_zero`
and its companion,
`preparing_an_event_with_no_filler_in_hand_leaves_the_old_net_zero_behaviour_alone`.
Full `cargo test --lib` (1104 tests): all green.

### Corpus result -- judged on the oracle first, per the coordinator's own
### instruction, not on the stall-bucket count alone

| | before | after |
|---|---|---|
| discard-phase oracle: checkpoints matched | 17970/28694 (62.6%) | **24640/28695 (85.9%)** |
| games with at least one oracle divergence | 992/1011 | **941/1011** |
| games completing to `state.game_over` | 49 | **66** |
| mean rounds reached | 12.35 | 12.29 |

Set-diffed by exact game ID (not just the count, per this file's own
standing discipline): of the 49 games completing before this fix, **3**
(`7521328`, `7522454`, `7523090`) now stop slightly earlier -- all three at
a NEW `StuckPending: decider != expected actor ... no pending` a few lines
before their old stopping point, consistent with this fix occasionally
popping a filler that turns out to have been needed for an UNTYPED future
discard (`DiscardSolver::needed_after` only knows about cards later
observed being PLAYED by name -- a plain `"discards N cards"` line never
names an identity, so it cannot protect a filler a real discard will need
later). **20** games newly complete that did not before. Net +17,
overwhelmingly one-directional.

The remaining 941 divergent games were also checked for DIRECTION, not just
count, since introducing a NEW opposite-signed error would be exactly as
bad as the one being fixed: of their first divergences, **920 are still
the pre-existing OVERCOUNT direction** (reconstructed excess > journal
excess -- the mechanism this pass did not fully close) and **21 are a NEW
UNDERCOUNT direction** (reconstructed excess < journal excess -- this
fix's own filler-popping now removing one card too many in a minority of
games, plausibly the same untyped-future-discard gap noted above, or a
game with multiple preparations draining the filler pool faster than it
refills). 21 new undercounts against 51 fewer total divergent games and a
23-point jump in oracle agreement is a clear net win, not a wash -- but the
21 are a real, novel, narrower open item, not zero-cost.

### What was RULED OUT this pass (don't re-check these)

- **`ground_military_hand` growing hand size instead of replacing filler,
  for `7522614` specifically** -- there is no War/Aggression/Pact/Tactic
  reveal anywhere in the first four rounds of this game for this to fire
  on; the entire hand is provably ordinary-draw output, checked card by
  card, not inferred.
- **`CardType::Event` cards in `military_deck` as a deck-composition bug**
  -- confirmed intentional (`apply.rs::h_prepare_event`, §5.2): drawing an
  event card to military hand and later "preparing" it into `future_events`
  during a Politics Phase is the real game mechanic, not a leak.
- **A flat `military_hand_limit` undercount (limit should be +1)** as an
  alternative to a hand-size overcount -- falsified against `7522612`'s
  round-5 numbers, which only fit a hand-side, round-cumulative "-1 per draw
  after the first" model, not a constant limit offset.
- **The coordinator's missed-MA-charge theory as a *complete* explanation**
  -- correctly predicts a real, general shape (an action whose cost isn't
  charged inflates `military_actions_unused`) and may still be real for
  OTHER games (not re-checked this pass), but it was not `7522614`'s own
  cause: that repro diverges with zero relevant player actions in the
  window. The actual cause (below) turned out to be a Politics-phase action
  that spends no military action at all.
- **The "-1 per draw after the first" characterization as the mechanism
  itself** -- it was always a correct DESCRIPTION of the symptom (in a
  typical undisturbed game, one `PrepareEvent` fires roughly once per
  round, so "one stray `+1` per preparation" and "one stray `+1` per draw
  after the first" are nearly indistinguishable from outside), but the
  actual trigger is the Politics-phase preparation action, not the draw
  step -- do not go back to auditing `economy.rs`'s draw code for this.

### Concrete next step for whoever picks this up

The fix above closed the dominant OVERCOUNT mechanism (round-4-and-later
divergences dropped from 992 to 941 games, oracle agreement 62.6% ->
85.9%) but did not close everything:

1. **The 21-game new UNDERCOUNT direction** (this fix popping a filler a
   later UNTYPED discard needed) is the most concrete lead: teach
   `DiscardSolver` (or a sibling structure) to also track cards that will
   later be needed to SATISFY a future `"<Color> discards N cards"` line's
   own count -- not by identity (discards don't name one), just by making
   sure this fix's own pop never drops `hand_military`'s size low enough
   that a later real discard requirement can no longer be met. The 3
   regressed completions (`7521328`, `7522454`, `7523090`) are small,
   already-isolated repros for this.
2. **The 920 remaining OVERCOUNT-direction divergences** are the leftover
   share of the original mechanism this pass did not fully close -- rerun
   the same card-by-card `REPLAY_DEBUG_ALL` audit this pass used on
   `7522614`, on a FRESH first-divergence game post-fix, to see whether a
   second `PrepareEvent`-shaped bug remains (e.g. a preparation whose own
   card was never drawn at all, so there is no filler to sacrifice and the
   old net-zero wash still applies) or whether an entirely different
   mechanism is now the dominant one.

## `Take`/`HandFull` handoff, closed with a SECOND full negative: a third independent from-scratch trace (including matched Take/PutBack pairs) confirms hand size and limit are both exactly right -- every remaining lead now funnels back to the previously-reverted `>=`/`>` boundary, NOT touched per standing instruction

Picked up the still-open `HandFull` lead from the "Cost-cluster pass" section
above (`Take` 129, `HandFull` 109 of 123 named rejections corpus-wide as of
`38bcaa5`, unchanged in shape from the prior pass's 109/123). Assigned
explicitly to rule out a MISSING POP -- a card the real player no longer
held that this reconstruction's `hand_civil` still carries -- as the
remaining explanation, since the limit side (`civil_hand_limit`) was already
independently cross-checked against the official card source twice over.

**Re-verified the starting point first, on latest `master` (`38bcaa5`, the
concurrent civil-age-drift instrument landed since `cfe7d9f`)**: both of the
prior pass's own worked examples (`7523357` line 442, `7523003` line 222)
reproduce byte-for-byte identically under the enriched `TAKE REJECT` debug
line -- the age-drift instrumentation that landed in between does not touch
this bucket at all (confirmed, not assumed: `REPLAY_DEBUG=1 ./target/
difftest/replay` against both game IDs, diffed against the prior pass's own
quoted output).

**Method** (per the brief's own instruction, and this doc's repeated
lesson: don't reimplement the rules in a side script, trace by hand against
the raw journal text): for `hand_civil` to be right, EVERY push must trace
to a real `"takes X in hand"` line and every card that should have LEFT by
the failure point must trace to a real play/develop/discover/revolution
line, a `"using Y"` free-civil-action discount play, a leader replacement,
OR a matched `Take`+`PutBack` client-side-undo pair (`corpus.rs`'s
`ActionClass::PutBack` -- BGO logs a human's own take-back as a second
journal line; `replay_common.rs`'s own doc: "a matched Take/PutBack pair is
never applied to the engine at all"). Re-derived this by hand from the raw
`.tsv`, line by line, for a THIRD example not covered by either prior
pass's trace, deliberately picked to stress-test the matched-pair and
free-civil-action paths the first two examples barely touched:

**Game `7522481` line 189** (`"Purple takes Engineering Genius in hand"`,
2p, Purple, round 11, government Monarchy, leader Isaac Newton not yet
elected -- `hand_civil_size=6`, `civil_hand_limit=6`): re-derived Purple's
ENTIRE civil-hand history from scratch across 182 prior journal lines,
tracking every push (`"takes"`), the four known pop sites (leader replace,
develop, revolution, play-action -- none used this game), every
`"using <Card>"` free-civil-action discount play (three: `"builds
Philosophy using Urban Growth"` line 35, `"plays Engineering Genius"` line
50 ordering a wonder-stage build, `"upgrades Bronze to Iron using Rich
Land"` line 142 -- all three correctly pop their discount card), and TWO
matched `Take`+`PutBack` pairs (`"takes Knights"`/`"puts Knights back"`
lines 89-90, `"takes Patriotism"`/`"puts Patriotism back"` lines 164-165,
plus a THIRD at lines 185-187 for a re-taken `Engineering Genius`). The
hand-derived-by-hand result at line 189, independent of this binary's own
bookkeeping: `["Swordsmen", "Cavalrymen", "Efficient Upgrade (II)",
"Reserves (II)", "Organized Religion", "Isaac Newton"]` -- **six cards,
exactly matching this binary's own `hand_civil` dump, in composition (not
just count)**. `civil_hand_limit`: Monarchy's own printed `CA=6`
(cross-checked again against `sources/bga_throughtheages_material.inc.php`)
plus zero hand-limit bonus (only Pyramids is completed, which grants none;
Library of Alexandria, the one card in the whole data set with
`civil_hand_limit: 1`, is not in play) = 6, matching exactly.

**Result: THIRD independent trace, THIRD exact match, zero discrepancy.**
Combined with the prior pass's two examples (`7523357`, pure `hand_civil`
provenance, no matched pairs; `7523003`, one free-civil-action pop), this
now covers every hand-mutating mechanism this codebase models (all four
pop sites, three flavors of `"using <Card>"` free-civil-action play, and
three matched Take/PutBack undo pairs) with zero counterexamples across
three separately-chosen games. The 98-game push/pop provenance audit two
passes back (`docs/REPLAY.md`, "HandFull handoff, closed with a full
negative") already established this structurally (`hand_civil` has exactly
one push site, four pop sites, 517 accounted pairs); this pass adds
line-by-line agreement against the raw journal text itself, independent of
this binary's own bookkeeping, on a THIRD game chosen specifically to
exercise the two mechanisms (`PutBack`, `"using"` free-civil-action plays)
the provenance audit's own push/pop-count method could not directly see
into. **The missing-pop hypothesis is now ruled out as thoroughly as this
project's own standing methods allow.**

**Also ruled out this pass, cleanly, with counter-evidence from the SAME
two examples**: the previous handoff's own suggested "last civil action of
the turn" carve-out (take something you're about to immediately spend your
final CA playing down). `7523357` line 442 is followed by `"Green builds
Knights"` (a DIFFERENT card, not the one just taken) with the turn
continuing after; `7522481` line 189 fires with `civil_actions=2` still
unspent (`gate_have=2` in the `TAKE REJECT` dump) -- two CAs left, nowhere
near the turn's last action. Both counterexamples independently kill this
theory; not pursued further.

**Also re-verified, independently, this pass**: the "action cards might be
exempt from the hand limit" theory this pass's own worked example initially
suggested (`7523003`'s rejected hand is 3/4 action cards: two `Rich Land`,
one `Urban Growth`) -- checked against `docs/RULES_SPEC.md` §6.7 ("civil
hand limit = civil action total ... enforced only when taking cards", no
type exception stated) and independently against outside community sources
(BoardGameGeek rules-question threads on this exact mechanic): "action cards
are civil cards ... they follow the same hand limit rules as other civil
cards when drafted from the card row -- they are not exempt." `7523357`'s
own hand (`["Frugality (II)", "Wave of Nationalism", "Riflemen", "Coal",
"Journalism", "Military Build-Up", "Rich Land (II)"]`) is majority
TECHNOLOGY cards anyway, independently falsifying the theory even without
the outside sources. Ruled out.

**Where this leaves the bucket**: every avenue this doc's own accumulated
method can reach is now exhausted with a negative result. Hand size is
provably right (three independent from-scratch traces). The limit is
provably right (government `CA` and every hand-limit-granting card's bonus
cross-checked against `sources/bga_throughtheages_material.inc.php` on all
three examples spanning Despotism, Constitutional Monarchy, and Monarchy).
No carve-out for imminent replay, no action-card exemption. **Every
remaining explanation funnels back to `costs::take_gate`'s `hand_full =
hand_size_civil() >= civil_hand_limit` itself** -- exactly the comparison a
previous pass changed to `>` (commit `b8f84aa`, RULES_SPEC updated to
match, 70 games fixed, corpus-wide **first-ever full game completions** --
8 of 1,011), then reverted the same day (`9d02057`) on the strength of
`docs/RULES_SPEC.md`'s own `>=` citation plus outside community
cross-checks. This pass's own outside-source check (previous paragraph)
independently reproduces that same `>=` reading ("at or above the limit,
you may not add another civil card ... by any means") -- so the textbook
rule, cross-checked twice now by two different passes, really does appear
to be `>=`, and yet three separately hand-verified real games each show a
human taking a card with their hand ALREADY at that exact boundary, with no
error or undo logged afterward. **Per this project's explicit standing
instruction this pass did NOT re-open or re-touch that comparison** --
still `>=`, unchanged, per the hard rule this brief and the prior revert
both state. Not fixed.

**A structural note for whoever next has standing to revisit the "settled"
call**: `costs::take_gate`/`can_take_gated`/`take_rejection` are the SAME
shared function serving both `legal::legal_moves` (what the self-play
engine believes is legal, and therefore what it trains bots against) and
this replayer's own `try_apply` gate (what a real, already-happened BGO
game is allowed to have done). Those are two different jobs with two
different costs of being wrong: a bot that believes it can exceed the real
rule's hand limit would be training against a rule no live opponent
actually enforces (bad); a replayer that refuses to reproduce a game a real
human actually played (the entire 109-game `HandFull` bucket, now the
single largest named stall in the corpus) is failing at its one job today.
If BGO's own live server enforcement genuinely is `>` rather than the
rulebook's textbook `>=` -- plausible; BGO is a digital implementation, not
the rulebook itself, and this project has already found one confirmed
digital-vs-rulebook mismatch this pass (Shakespeare's Library/Theater
discount, `753b7ba`) -- then the SELF-PLAY-facing gate and the
REPLAYER-facing gate arguably have license to diverge on purpose (train
bots against the stricter textbook `>=`; replay real BGO games against
BGO's own observed `>`), which `costs::take_gate` being one shared function
cannot express today. That would be a deliberate architectural decision
(a second, replayer-only gate, or a documented parameter), not a
one-line comparison flip re-litigated on corpus evidence alone -- exactly
the distinction the previous revert's own coordinator review drew a line
at. This pass surfaces the evidence and the shape of that decision but does
not make it.

### Measurement (`replaystats`, full 1,011-game corpus, `38bcaa5`)

No code changed this pass (the standing instruction blocked the one lever
every trace points at) -- re-measured anyway to confirm the corpus is
otherwise unchanged from this pass's own starting point: 58/1,011 games
complete, `IllegalMove: Take` 129, `HandFull` share unchanged (109/123
named rejections). No regression, no improvement -- expected for an
investigate-only pass.

## `Take`/`HandFull`: the `>=`/`>` boundary is now SETTLED PERMANENTLY by primary source; the redirected civil-action-TOTAL audit also comes back clean -- both provably-correct quantities really are correct, corpus-wide, and the bucket remains genuinely open

The coordinator extracted `sources/cge_code_of_laws.pdf` directly
(`pdftotext -layout`) and settled the question the last two sections above
kept circling back to. Recorded here so it is not re-opened a third time:

> "The number of civil cards in your hand is limited by your civil action
> total. When you are **at or above** the limit, you may not add another
> civil card to your hand **by any means**. However, if you are above the
> limit for some reason, you do not have to discard excess civil cards."
> -- *Code of Laws*, official rules companion, verbatim.

**`costs::take_gate`'s `hand_full = hand_size_civil() >= civil_hand_limit`
is correct and this boundary is closed.** "At or above" plus "by any
means" leaves no exception to construct. The `>` version (`b8f84aa`) was
wrong despite fixing 70 games and reaching this project's first-ever full
completions -- a reminder that a stall-bucket count moving in the right
direction is not proof a change is RULES-correct, only that it changes
what the corpus exercises. Do not re-touch this comparison again without a
primary-source citation of equal or greater weight than the one above.

### Redirected: audit every source that raises `state_stats(...).civil_actions` (the TOTAL `civil_hand_limit` reads), since by elimination the total, not the hand, must be the wrong side

Checked every mechanism `effects::compute` sums into `stats.civil_actions`,
and every `on_enter_play`/`on_leave_play` runtime top-up
(`apply.rs`, the asymmetry class the coordinator named -- a value
maintained in a decrementing runtime counter, `p.civil_actions`, separately
from a value fully RECOMPUTED from current game state,
`state_stats(...).civil_actions`, with nothing structurally forcing the two
to agree):

- **Government (base + card-granted CA)**: `compute` reads `p.government`
  directly every call (`stats.civil_actions = ge.civil_actions` or the `4`
  default) -- not cached, not staged, so a government that already changed
  earlier this turn is reflected on every subsequent call with no lag
  possible by construction. Cross-checked the full government table
  against `sources/bga_throughtheages_material.inc.php`'s own `CA` field:
  Despotism 4, Monarchy 5, Theocracy 4, Constitutional Monarchy 6, Republic
  7, Communism 7, Fundamentalism 6, Democracy 7 -- matches `card_table.rs`
  exactly, all eight.
- **A government changed THIS SAME TURN (peaceful `discovers` or a violent
  `revolutions`), immediate effect for hand-limit purposes**: the specific
  scenario the coordinator flagged as the prime suspect. Grepped all 109
  `HandFull` rejections against their own raw journal text for an earlier,
  same-round, same-actor `"discovers <Government>"` or `"revolutions"` line
  -- **zero of 109** have one. (First attempt at this grep mis-associated
  ~30% of rows with the WRONG game -- an off-by-one in a throwaway Python
  script pairing each `TAKE REJECT` debug line with the FOLLOWING game's
  `STOPPED` summary instead of correctly buffering until it, not an engine
  bug; caught by a raw-line spot-check against `7521815` before trusting
  the first pass's result, redone correctly below.) Also checked
  `set_government` (`apply.rs`) directly: it recomputes `p.civil_actions`
  from a fresh `state_stats` call immediately, so even where this DID occur
  the runtime counter would already reflect the new total -- moot here
  since it never occurs in this bucket, but confirmed correct regardless.
- **A leader replaced this same turn** (the corpus's own `"... dies;
  <Color> gets 1 civil action"` phrasing the coordinator quoted): that `+1`
  is the RB p.11 replacement REFUND (a spent action point given back, not a
  change to the total pool) -- `h_play_leader` computes it as `total.min
  (p.civil_actions + 1)`, clamped against a fresh `costs::ca_total` call,
  never added to `state_stats`'s own total. Grepped the same 109 for a
  same-round, same-actor `"elects "` line before the rejection: **zero of
  109**. No leader-swap staleness is even exercised in this bucket.
- **Wonders, completed vs partial**: `compute` only iterates
  `p.completed_wonders`; `p.wonder` (in-progress) never contributes,
  matching RULES_SPEC item 8 ("When the last stage is covered the wonder is
  completed ... effects begin"). Only two base-game wonders carry a
  nonzero `civil_actions` field at all (Pyramids `+1`, Kremlin `+1`,
  confirmed by scanning every `Card { ... }` entry in `card_table.rs` for a
  nonzero `civil_actions` field, not just the wonders -- see below). A
  wonder completing THIS SAME TURN, before the rejected take: **6 of 109**
  do have one (`7522265`, `7523408`, `7521213`, `7522671`, `7522705`,
  `7523228`) -- but the wonders involved (Great Wall, Ocean Liners,
  Universitas Carolina, cross-checked against `card_table.rs`) each carry
  `civil_actions: 0`. None of the 6 occurrences involve a CA-granting
  wonder. Dead end, but a real one to have checked rather than assumed.
- **Colonies / territory cards with a permanent CA symbol**: `effects.rs`'s
  own doc comment confirms a colony's bonuses are pre-merged into the SAME
  `CardEffects` struct at codegen time (`gen_cards.py`), so the exhaustive
  `card_table.rs` scan below already covers them -- there is no separate
  code path to miss.
- **Pacts**: `cards::PactBlock` (the struct every pact's `A`/`B`/
  `BothPlayers` special block deserializes into, read by
  `effects::apply_pact_block`) has NO `civil_actions` field at all --
  `culture`, `food`, `resources`, `strength`, `military_actions`,
  `tech_discount`, `war_immune`, `food_as_resource`, `resource_as_food`,
  `other_party_pays_science`, `culture_per_wonder_of_other_party`, nothing
  else. No base-game pact grants civil actions (a real rules fact, not a
  porting gap -- `gen_cards.py` would have needed the field if any pact
  card's text required it), so there is structurally nothing for this
  source to miss.
- **Exhaustive card-data scan**: every `Card { ... }` entry in
  `card_table.rs` with a nonzero `civil_actions` field, by kind: Despotism
  4, Monarchy 5, Theocracy 4, Constitutional Monarchy 6, Republic 7,
  Communism 7, Fundamentalism 6, Democracy 7 (all `Government`, the base
  value `compute` already reads specially); Code of Laws `+1`, Justice
  System `+1`, Civil Service `+2` (all `SpecialTech`, summed via
  `add_flat` in `compute`'s `p.techs` loop -- confirmed that loop's
  `SpecialTech` arm, unlike its `Farm`/`Mine`/unit arms, does call
  `add_flat`); Pyramids `+1`, Kremlin `+1` (both `Wonder`, summed via
  `add_flat` in `compute`'s `completed_wonders` loop). **No `Leader`,
  `Action`, `Government`-adjacent-but-not-`Government`, or other kind
  carries a nonzero `civil_actions` field anywhere in the base game's
  data** -- so there is no leader-granted standing CA bonus to even check
  for a `compute`-vs-`on_enter_play` mismatch; the mechanism the coordinator
  named (a runtime top-up with no `state_stats` counterpart) has no card
  in the actual data set that could trigger it.
- **`civil_life_ca_free`/`ca_penalty_next_turn`**: neither is read by
  `costs::civil_hand_limit` at all (grepped both call sites) -- the former
  waives ONE action's own cost (Pop/Build/Develop only, never a Take, and
  never changes the TOTAL either way), the latter reduces next turn's
  runtime budget (`economy.rs:701`, applied only to `p.civil_actions`, the
  remaining-this-turn counter, never to `state_stats`). Structurally
  incapable of affecting the hand-limit total in either direction.

### The concrete test: `p.civil_actions` vs `state_stats(...).civil_actions` vs an independent, from-scratch journal reconciliation, on all three previously-traced examples

The enriched `TAKE REJECT` line already prints both (`civil_actions=` is
the runtime remaining-this-turn counter, `s_civil_actions=` is the fully
recomputed total `civil_hand_limit` actually uses). Reconciled each against
a by-hand sum of every `"uses N civil action"` clause in that SAME player's
CURRENT turn (from their own `"passes Political Phase"`/`"Action Phase
begins"` line up to the rejection), independent of this binary's own
bookkeeping:

| game | line | gov | `s_civil_actions` (total) | this-turn spend, summed from the raw journal | `s_civil_actions` − spend | `civil_actions` (runtime, this binary) |
|---|---|---|---|---|---|---|
| `7523003` | 222 | Despotism | 4 | 1 (pop) | 3 | **3** |
| `7523357` | 442 | Constitutional Monarchy | 6 | 3+1+1=5 (Military Build-Up take, pop, Rich Land take) | 1 | **1** |
| `7522481` | 189 | Monarchy | 6 | 1+3−1+1=4 (Engineering Genius take, Eiffel Tower take, `PutBack` refund, Isaac Newton take) | 2 | **2** |

All three: exact agreement, three ways at once -- the runtime counter, the
recomputed total, and an independent hand-reconciliation of the raw
journal text. `7522481` is the sharpest of the three: it contains BOTH a
wonder take (surcharge-priced, 3 CA) AND a matched `Take`+`PutBack` pair
(whose journal line explicitly prints `"Purple gets 1 civil action"` for
the refund) in the same turn, and the arithmetic still closes exactly.

**Result: the civil-action TOTAL is also provably correct in all three
examples, by the same standard of evidence (independent, from-scratch,
against the raw journal) this project used to clear the hand-size side.**
Both of the "one of your two provably-correct quantities must be wrong"
quantities remain independently verified correct. The redirect closes with
a second clean negative, not a third culprit.

### Landed this pass (small, no behavior change)

`can_take_a_wonder_even_with_a_full_civil_hand` (`costs.rs`): pins the
"cheap check" the coordinator also asked for -- `can_take_gated`'s wonder
arm already returns before ever reaching `gate.hand_full` (RULES_SPEC
§2.4/§2.5: a wonder never enters hand, the taking-limits rule is scoped to
"(non-wonder)"), confirmed already correct by direct code reading, now
pinned by a test so a future reordering of the branches can't silently
regress it. Full `cargo test --lib`: 1,114 passed (was 1,113), 0 failed.

### Where this leaves the bucket

Every lead named in this doc's now-three `Take`/`HandFull` passes has been
checked and closed negative: missing pops (three independent from-scratch
hand traces, zero discrepancies), the "last CA of the turn" carve-out
(falsified by two counterexamples), "action cards exempt" (falsified by
outside sources and by `7523357`'s own majority-technology hand), the
`>=`/`>` boundary (now closed permanently by primary source), and every
named source of a civil-action-total undercount (government/leader/wonder/
colony/pact/one-time-discount, checked both structurally against the code
and empirically against all 109 real corpus occurrences). **This is now a
genuinely unexplained discrepancy between a primary-source rules citation
and three independently, exhaustively verified real BGO games** -- not a
sign either side of the earlier "which quantity is wrong" framing was
right, since neither was. Two possibilities remain, neither chased this
pass:

1. **BGA's live server implementation itself diverges from the printed
   rule in this one spot.** Not unprecedented for this project: the
   Shakespeare Library/Theater discount (`753b7ba`, this same doc, Disease
   2) was a CONFIRMED digital-vs-rulebook mismatch, checked against the
   game's own `CulturePerLibraryTheaterPair` special (built, not merely
   developed) and fixed as an ENGINE bug. If true here, this project's own
   two goals -- train bots against the TRUE rule, replay real BGO games
   faithfully -- would need the replayer and self-play legality to
   deliberately diverge on this one gate, which `costs::take_gate` being a
   single shared function cannot express today (see the architectural note
   two sections up, still unactioned).
2. **A fourth mechanism not yet named, in neither "hand" nor "total".**
   E.g., something about HOW `civil_hand_limit` was worded across CoL
   printings/errata, a distinction between "civil action total" (a fixed
   per-turn allotment) and some OTHER quantity BGA's UI actually gates on,
   or a rule this project's card data does not yet model at all. Nothing
   concrete to point to -- flagged as genuinely open, not a lead.

No code change to `costs::take_gate`. Corpus unaffected by this pass's
findings (58/1,011 complete, `Take` 129, `HandFull` 109/123 -- the one new
test does not change any runtime behavior).

## Civil deck model: root-caused and closed -- option (a), lockstep draining, is provably NOT achievable; option (b), journal-driven age with the deck's own SIZE decoupled from control flow, is what landed (`38bcaa5`, `b3f099c`)

Picked up the shared-root-cause task this file's own two prior sections named
(the `HandFull` handoff's civil-age LAG and the Second `IllegalMove: Pop`
pass's civil-age EARLY-advance lead, game `7523449`): is `civil_deck`'s own
SIZE fixable so `game::advance_age`'s deck-empty trigger fires at the right
time on its own, or is the honest answer that it cannot be, and the journal's
own `Line::age` should be the sole authority instead?

**Answer: NOT achievable, confirmed structurally, not by giving up early.**
Traced why with `TMP_DECK_TRACE`-style instrumentation (temporary, not
committed) on `7523449`: `Replayer::ground_row_slot` reports a same-cost TIE
-- several candidate row slots that all reproduce the journal's own stated
civil-action cost for a Take -- on very nearly every single Take in the
corpus, not a rare edge case. The reason is structural: `costs::row_cost`'s
three price bands (1/2/3 civil actions) are wide (5/4/4 slots), but
`game::replenish`'s own mandatory sweep -- discard the leftmost `sweep_n`
slots every turn, UNCONDITIONALLY, `sweep_n` is 3/2/1 for 2/3/4 players --
always falls entirely inside the CHEAPEST band. BGO's journal states the
COST a Take paid (a real, recorded fact), which narrows the true row slot to
its TIER, but never to the exact slot within it -- and whether the real slot
sat on the swept side or the safe side of that boundary is EXACTLY what
determines whether that card's own vacancy is absorbed free by the next
sweep or costs an extra draw from `civil_deck`. That distinction is private
information (the unshuffled deck's own physical layout) that a cost-tier
observation cannot recover, at any player count, for any Take. So
`civil_deck`'s own draw COUNT cannot be reconstructed exactly from the
journal -- not a parsing gap this file can close, an information-theoretic
one.

Given that, the SIZE was never a fact to reconstruct at all, so it should
never have had the power to trigger `game::advance_age` (via
`game::deal`'s embedded `civil_deck.is_empty()` check) in the first place.
Two commits:

**`38bcaa5` -- the instrument, landed on its own first, per this task's own
brief.** `GameResult::civil_deck_premature_advance`: the first journal line,
if any, where `state.age_civil` reads STRICTLY AHEAD of what `Line::age`
proves the real game had reached, with a later REAL (non-wrap-up) decision
still proving the OLD age was still current. Built entirely from a journal-
stated fact (`Line::age`), reconciled against itself (a later real decision's
own age column), never a rules reimplementation. First pass false-positived
on 828/1,011 games -- traced to BGO's own well-known "next turn's marker
logged before the previous turn's own trailing summary, same timestamp"
quirk (already documented in this file for `EndTurn`), fixed by excluding
`EndTurn` from the "still counts as evidence of the old age" line set,
re-measured at 53/1,011. A SECOND instance of the same quirk (this checker's
own false positive, caught by suspecting the checker before the engine, per
this project's rule) was found chasing the real fix down: `ActionClass::
Discard` (resolving a queued military discard can itself finish a turn and
fire the real transition, per its own `apply_one` doc) can ALSO trail a
later-tagged marker in file order -- confirmed on `7522652` line 430,
excluded the same way, landed in `b3f099c` with a RED-confirmed test.
Final, validated baseline: **7/1,011 games** genuinely diverge before the
fix.

**`b3f099c` -- `top_up_civil_deck`, the fix.** Keeps `civil_deck` topped up
with extra, never-observed filler drawn from the same age's own card pool
(`game::build_deck`, reshuffled) whenever it drops under a floor (`2 *
ROW_SIZE`, comfortably above one line's worth of engine activity) -- so
`game::deal`'s embedded `advance_age` trigger becomes structurally
UNREACHABLE during replay, checked by a `debug_assert` at the call site
(safe under `panic = "abort"`: a direct, cheap consequence of the line
immediately above it, not a speculative check on unrelated state).
`catch_up_civil_age`, reading `Line::age` directly, is left as the ONE
mechanism for every civil age transition during replay -- not a primary
approximation plus a corrective snap-forward that can disagree with each
other, which is exactly what let `7523449`'s early advance slip past the
original snap-forward-only mitigation (it only ever moves the age UP TO the
journal's column, so it structurally could not have caused an early
advance). Self-play untouched: the function's only call site is this file's
own per-line loop; `game::deal`/`advance_age`/`replenish` keep their
existing behaviour, still correct for a real game whose `civil_deck` empties
in real time with no lag possible.

### Measurement (`replaystats`, full 1,011-game corpus, exact game-ID set diffs, not bucket counts alone)

| | before (`38bcaa5`, instrument only) | after (`b3f099c`) |
|---|---|---|
| `civil_deck_premature_advance` | 7 games | **0 games** |
| `IllegalMove: Pop` | 29 | **27** |
| games completed | 58 | **60** |
| mean rounds reached | -- | 12.35 -> 12.91 (this pass's own before/after) |
| decisions in Age II+ | -- | 51.0% -> 53.0% |

`IllegalMove: Pop` diffed by exact ID: **2 cleared (`7523449`, the
documented repro, and `7522252`), 0 new entrants** -- a pure improvement.
`completed` diffed by exact ID: 3 new completions, 1 regressed
(`7523486`) -- traced, not assumed: WITHOUT the fix it reached "End of
game" but its own final score badly mismatched `index.tsv` (`engine=[66,
35]` vs `index=[62,43]`) -- i.e. it was silently completing on a WRONG
reconstructed state. WITH the fix it correctly stops instead at a
pre-existing, unrelated `StuckPending` (a decider/actor mismatch during
discard resolution -- not this pass's area) rather than running through to
a wrong result. Not a regression: a false completion traded for an honest
stop, exactly the "clean negative over a manufactured fix" this project's
own rule asks for. Final-score exact-match rate is unchanged either way
(0/58 -> 0/60, a pre-existing and entirely separate problem from this
pass's own scope -- mean delta -11.39 -> -11.29, essentially flat).

(A later, full-corpus re-run on top of everything else landed by concurrent
workers by the time this section was written shows 76 completed, `Pop`
still 27, `civil_deck_premature_advance` still **0/1,011** -- the fix holds
cleanly composed with unrelated concurrent work, not just in isolation.)

**REPLAYER, not ENGINE**: `game::deal`/`advance_age`/`game::replenish` are
already correct for self-play, where `civil_deck` empties in real time with
no lag possible; only this file's own substitute trigger was ever wrong.
`top_up_civil_deck` is a private fn with its only call site inside this
file's own per-line loop -- no path into `bots/` or the evaluator, and it
never reads or infers anything about the unshuffled deck's real order
(the padding cards are fictional filler, exactly like `game::deal`'s
existing filler already is, just more of it).

**Not investigated this pass**: whether any OTHER cost-tier/row-position
ambiguity (beyond the sweep-boundary one this pass root-caused) still
affects `Build`/`Upgrade`/`WonderStep` cost buckets that also read
`state.card_row`'s content -- those were flagged as plausibly sharing the
OLD lagging-age root cause by an earlier pass (`8edfea7`'s handoff); this
pass did not re-check whether the EARLY-advance direction reaches them too.

## `Take`/`HandFull`: landed as a documented REPLAYER-ONLY divergence, not a fix -- the engine stays rulebook-correct, the replayer reproduces the real BGO implementation

Every lead this file's now-four `Take`/`HandFull` passes could name has been
checked and closed. Both quantities `costs::take_gate`'s `hand_full` gate
reads -- civil hand size and civil action total -- are independently,
exhaustively verified correct, corpus-wide, by three from-scratch traces
each. The `>=`/`>` boundary is settled PERMANENTLY by primary source:

> "The number of civil cards in your hand is limited by your civil action
> total. When you are **at or above** the limit, you may not add another
> civil card to your hand **by any means**. However, if you are above the
> limit for some reason, you do not have to discard excess civil cards."
> -- *Code of Laws*, official rules companion, verbatim.

**`costs::take_gate`'s `hand_full = hand_size_civil() >= civil_hand_limit`
remains correct and is NOT touched by this section.** Yet ~109 corpus games
show BGO's own journal logging a human legally taking a civil card at
exactly that limit. This is not a bug to fix in either direction -- it is a
genuine, primary-source-confirmed divergence between the printed rule and
the real BGO server's own live implementation (not unprecedented for this
project: the Shakespeare Library/Theater discount, this same doc's Disease
2, was a confirmed digital-vs-rulebook mismatch of the same shape). This
project has two goals that pull apart at exactly this one gate: train bots
against the TRUE rule (the engine, self-play legality, `legal.rs` +
`costs::take_gate`), and replay real BGO games faithfully (the replayer,
`replay_common.rs`). A single shared `take_gate` could not express both at
once.

**What landed**: the two jobs are now split. `costs::take_gate` and
`legal.rs` are BYTE-IDENTICAL to before this section -- self-play legality
is completely unaffected; `engine_legality_still_refuses_a_take_at_the_
civil_hand_limit_after_the_replayer_hand_full_override_exists` (`costs.rs`)
pins that a take at the limit is still refused by the engine's own
`take_gate`/`can_take_gated`/`take_rejection` primitives directly, not by a
reimplementation.

The REPLAYER alone gets a narrow carve-out, `replay_common::Replayer::
try_apply_take` (the new sole call site for a journal-observed `Move::
Take`, replacing a bare `try_apply`): when `legal::legal_moves` refuses a
take, `take_blocked_only_by_hand_full` decides whether `hand_full` is the
ONLY reason, by calling the existing `costs::take_rejection` TWICE (never a
bespoke reimplementation of the gate order) -- once with the real gate
(must name `HandFull` specifically; any other named reason leaves the
ordinary honest mismatch untouched), once more with a copy whose
`hand_full` is forced `false` (must then return `None`, proving no OTHER
gate -- cost, duplicate-name, one-leader-per-age, wonder rules -- also
blocks this exact take). Only then does the replayer accept the take (via
`apply::apply`, the same primitive an ordinary legal move goes through) and
count it, rather than raising `IllegalMove: Take`.

**Counted, never swallowed**: `Replayer::hand_full_takes_overridden` /
`GameResult::hand_full_takes_overridden`, aggregated and printed by
`replaystats` right next to the existing "hand cards grounded from a bid's
own force ceiling" line, explicitly captioned with the known ~109-game
shape so a future blowout is caught as a sign the override has gone too
loose, not celebrated as a bigger win.

Tests (`RED`-confirmed by temporarily short-circuiting `try_apply_take`'s
new branch to `false` and re-running, then reverted): `costs.rs`'s
`engine_legality_still_refuses_a_take_at_the_civil_hand_limit_after_the_
replayer_hand_full_override_exists`, and four in `replay_common.rs`:
`take_blocked_only_by_hand_full_is_true_when_hand_full_is_the_sole_
rejecting_gate`, `..._is_false_when_a_second_gate_also_rejects_the_same_
take` (the narrowness guarantee, via a duplicate-named row card), `..._is_
false_when_the_cost_alone_is_unaffordable` (the short-circuit-order
guarantee), and `try_apply_take_accepts_and_counts_a_hand_full_only_take_
the_engine_refuses` (end-to-end against real `legal::legal_moves`). Full
`cargo test --lib`: 1,124 passed (was 1,119), 0 failed.

**Measurement (`replaystats`, full 1,011-game corpus, exact game-ID set
diff, not bucket counts alone)**:

| | before | after |
|---|---|---|
| games completed | 76 | **95** |
| mean rounds reached | 12.85 | **13.39** |
| decisions in Age II+ | 53.3% | **55.6%** |
| `IllegalMove: Take` (bucket total) | 126 | **23** |
| hand_full-only takes accepted | -- | **313, in 107 games** |

`completed` diffed by exact game ID: **19 new completions, 0 regressed** --
a pure improvement, every one of the 19 traced to running past a take the
engine would have refused but the real game logged (`comm` set-diff against
the pre-change ID list, not eyeballed). The 107-game shape of where the
override actually fired lines up almost exactly with the ~109-game
`HandFull` bucket this file has tracked across four passes -- the 313 total
FIRINGS (average ~2.9 per game) is not a red flag: a game that gets past
one hand_full-only take by definition keeps playing, and a deeper replay
naturally exercises more turns, so more chances for the SAME rule to apply
again to the SAME player later in the SAME game. The signal to watch is the
game count, not the raw firing count, and it did not blow out.

No other bucket's individual composition was audited this pass beyond
confirming zero completed-game regressions -- the usual "a stall bucket
count moving is not proof of correctness on its own" caveat applies to
every downstream number in the table above exactly as it has in every
prior pass; only the completed-game ID diff is a strong claim here.

## Final scores: 0/76 exact matches traced to the replayer's hand-tracking gap, NOT a scoring-formula bug

`replaystats` (this file's own `bin/replaystats.rs`) now cross-checks every
completed game's `game::scores` against `index.tsv`'s own recorded final
scores (sorted per-game multiset compare, so a seat/column ordering mismatch
between the engine and `index.tsv` cannot be confused for a real error --
see that binary's own module doc). On the current 1,011-game corpus: 76/76
completed games flip `state.game_over`, and **0/76 score exactly**. The 160
non-matching player-scores have mean delta -9.11 (engine minus `index.tsv`),
a long negative tail to -48 and a shorter positive one to +21.

**The bottom-line answer, first, because it is the one thing worth reading
this section for**: this is a **REPLAYER bug, not an engine bug**. `game::
scores`, `game::finish_game`, `economy::end_of_turn`'s culture-accrual step,
and `events::evaluate_final_events` were all read in full against
`RULES_SPEC.md` §12.5 and found correct -- ordering, clamping, and the Bill
Gates leader-bonus special case all matched the rulebook and this file's own
existing tests. **A self-play game (`game::play_game`) tracks its own hands
exactly and never goes through the machinery below at all**, so there is no
reason to believe self-play climb has been optimising against a wrong
scoring objective. The residual doubt, stated plainly rather than smoothed
over: this was checked by fully decomposing TWO of the 76 mismatching
games end to end (both reconciled to zero residual, see below) plus the new
structural instrument covering the rest (`politics_false_skips`, below) --
it directly implicates 16/76. The other 60/76 were NOT individually
decomposed; they are presumed to be the same `hand_military` under-tracking
class (the still-open "920 remaining OVERCOUNT-direction divergences" `docs/
REPLAY.md`'s own prior section on `d4ad0f5` names as unclosed) manifesting
as a wrong-amount PrepareEvent rather than a fully-dropped one, but that is
an inference from the shared root cause and this pass's own time budget,
not a second independent confirmation.

### The mechanism, traced start to finish on `7522166`

Picked as the minimal repro: 2p, 76 actions from game-over, `civil_deck_
premature_advance` flagged at line 314 (an already-known, already-
instrumented gap -- see below for why that flag is NOT the dominant cause
across the other 75 games). Engine scores `[162, 163]` (sum 325) against
`index.tsv`'s real `[211, 187]` (sum 398) -- a game-total deficit of 73,
fully reconciled below to zero residual.

The real journal's last 15 lines (`sources/bgo/index.tsv`'s companion
journal, `7522166.tsv`) show the real game's own end-of-game announcement:

```
Orange IV 19  End of game Check the journal to get the final impacts effects :;
              Impact of Architecture; Impact of Variety; Impact of Progress; Impact of Strength;
              WINNER IS ... AS ORANGE (211 PTS); 2nd is ... AS PURPLE (187 pts)
Purple IV 19  Bill Gates scoring Purple scores 9 culture
Purple IV 19  Impact of Strength ... Orange scores 10 culture
Purple IV 19  Impact of Progress ... Orange scores 26 culture; Purple scores 10 culture
Purple IV 19  Impact of Variety ... Orange scores 22 culture; Purple scores 12 culture
Purple IV 19  Impact of Architecture ... Orange scores 18 culture; Purple scores 15 culture
```

This binary's own reconstruction, instrumented with a one-off `eprintln!`
inside `events::evaluate_final_events` (not landed -- diagnostic only) to
print `state.past_events`/`current_events`/`future_events` right before it
runs, shows it firing a COMPLETELY DIFFERENT set of four cards: `Impact of
Industry`, `Impact of Happiness`, `Impact of Agriculture`, `Impact of
Competition` -- only the leader bonus (Bill Gates, `game::end_of_game_
bonus`, handled separately from the event deck and unaffected) matches the
real game.

**Why the set differs**: `Impact of Happiness` was NOT still pending in the
real game -- it was drawn and resolved MID-GAME, at journal line 331
(`"Purple plays event Purple scores 3 culture; Current event:; III / Impact
of Happiness; ...; Orange scores 10 culture; Purple scores 14 culture"`).
This binary's own `event_plan::solve` correctly parses that line and builds
a `Preparation { lineno: 331, actor: 1 (Purple), revealed: Impact of
Happiness, ... }` entry in `plan.preparations` -- confirmed via
`REPLAY_DEBUG_ALL`, which shows 14 of the game's 15 real preparations
successfully claimed via `Replayer::resolve_political_decision`, and this
15th one never claimed at all (no `resolve_political_decision` call is ever
logged for `decider=1` past round 18). `ActionClass::PlayEvent` lines are
treated as pure confirmations (`apply_one`'s `PlayEvent => Ok(())` arm,
"resolved automatically when the triggering `PrepareEvent` was inferred") --
so when the real `PrepareEvent` is never inferred, the confirmation line is
silently a no-op: neither the 3-culture reveal bonus nor `Impact of
Happiness`'s own 10/14 effect ever lands on either player, AND the card is
never popped off `state.current_events`, so it survives to fire a SECOND
time at game end via `evaluate_final_events` -- computed against the WRONG
(end-of-game) board state, alongside three other cards that are also wrong
because the correct four (`Strength`/`Progress`/`Variety`/`Architecture`)
never got the chance to become "the last four standing" the way the real
game's draw order made them.

**Root cause, confirmed directly**: added a one-off `eprintln!` inside
`game::auto_skip_politics` (not landed) and re-ran. It fires for Purple at
round 18 with:

```
hand_military = [("Shock Troops", Tactic), ("Mechanized Army", Tactic),
                 ("War over Culture", War), ("War over Culture", War)]
```

Zero `CardType::Event`/`Territory` cards -- so `legal::legal_moves` offers
only `Move::PolPass` (`legal.rs::politics_moves` only ever pushes
`PrepareEvent` for a card actually present in `hand_military`, confirmed
correct against the rule and left untouched), `auto_skip_politics` closes
the phase (`state.phase = Phase::Actions`) BEFORE the per-line replay loop
(`Replayer::resolve_intervening`) ever gets a chance to check whether
Purple owes a preparation, and the real preparation is stranded. This is
the exact `hand_military` under-tracking class `d4ad0f5`'s discard-phase
oracle instruments (62.6% -> 85.9% agreement, still 941/1011 games with at
least one divergence) -- not a new bug, a NEW, higher-stakes SYMPTOM of the
same one: previously it was only visible as a wrong discard count; here it
silently drops a real game action and corrupts the final score. **This
pass deliberately did not touch `hand_military` reconstruction itself** --
`d4ad0f5` landed minutes before this pass started, is explicitly still-open
work (see its own "concrete next step" section, immediately above this
one), and its own commit message documents a prior attempt at this exact
area regressing the stall bucket 171 -> 305. Colliding with in-flight work
on shared, fragile state was judged higher-risk than the value of a rushed
second fix here.

**Full reconciliation, zero residual** (this repro's own 73-point deficit,
accounted for completely): -27 from the dropped mid-game `Impact of
Happiness` (Orange +10, Purple +3 reveal +14 effect, all silently no-op'd)
plus -46 from the wrong final-four-card substitution (real non-leader final
total 76 + 37 = 113; this binary's 14 + 26 + 14 + 13 = 67; 67 - 113 = -46).
-27 + -46 = -73, exactly the observed `sum(engine) - sum(index)` for this
game. Bill Gates' leader bonus (`game::end_of_game_bonus`) matched exactly
on both sides (+9 to Purple) and is confirmed uninvolved.

### Second repro, same shape: `7522625`

Real final events (from the journal's own "End of game" line): `Impact of
Balance`, `Impact of Progress`, `Impact of Happiness`, `Impact of
Architecture`. This binary's reconstruction instead fires `Impact of
Agriculture`, `Impact of Progress`, `Impact of Industry`, `Impact of
Competition` -- only `Impact of Progress` shared, the identical "wrong SET,
not just a wrong total" signature as `7522166`. Not fully decomposed to a
zero-residual reconciliation for lack of remaining budget, but corroborates
that this is not a `7522166`-specific coincidence.

### `civil_deck_premature_advance` overlap: real, but a MINORITY cause

`7522166` happens to also be one of the (separately, already-instrumented)
`civil_deck_premature_advance` games. Checked whether that flag predicts the
final-score mismatches directly: of the 76 score-checked games, only
**11/76** are also flagged `civil_deck_premature_advance` -- so while it is
plausibly involved in `7522166` specifically (an `Age III` vs `Age IV` civil
deck desync a few lines before the stranded preparation could easily shift
which political options `legal_moves` considers), it is not the dominant
mechanism across the other 65 mismatching games, which have NO civil-age
desync flagged at all. The dominant mechanism is `hand_military` under-
tracking broadly (of which a civil-age desync is one, but not the only,
trigger), consistent with `d4ad0f5`'s own 941/1011-games-still-divergent
number.

### New structural instrument: `GameResult::politics_false_skips`

Added the same always-on shape as `civil_deck_premature_advance`: counts,
per game, how many times `game::auto_skip_politics` closed a player's
Politics phase while `event_plan::solve`'s own plan says that player had a
real preparation waiting whose journal line had already been reached
(`Replayer::resolve_intervening`, checked every loop iteration, flagged at
most once per `plan.preparations` index via `false_skip_flagged_prep` so
the loop's own up-to-200-iteration retries against a stuck decision cannot
inflate the count). Corpus-wide (1,011 games): **53 false skips across 50
games**. Cross-referenced directly against the 76 score-checked games: **16/
76** have at least one `politics_false_skips > 0` -- a DIRECT, mechanism-
confirmed subset of the 0/76 mismatches (not an estimate). The other 60/76
are presumed the same `hand_military`-under-tracking family manifesting
differently (e.g. a WRONG but still type-matching card being offered/
consumed, rather than a phase that closes entirely) -- see the residual-
doubt paragraph at the top of this section for why that is an inference,
not a second confirmation. This is a detector, not a fix: it makes the gap
visible in `replaystats`'s own output (a new `politics false-skips: N
across M games` line) without touching the shared, fragile `hand_military`
machinery `d4ad0f5` is already mid-repair on.

### What was NOT investigated this pass

Any fix to `hand_military` reconstruction itself (deliberately, to avoid
colliding with `d4ad0f5`'s own in-flight follow-up); a full card-by-card
reconciliation of `7522625` (only the wrong-final-set signature was
confirmed, not a zero-residual accounting like `7522166`'s); whether any of
the 60/76 score-mismatched games NOT flagged by `politics_false_skips` have
a genuinely different (non-hand-tracking) cause -- plausible but unchecked,
since every game actually decomposed this pass fit the same mechanism
cleanly.
