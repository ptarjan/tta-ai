//! Card-coverage ratchet (rebuilt 2026-08-06): every card in the base game
//! must be CHOSEN by a real decision in some self-play game this test plays
//! itself, or be named on [`STRUCTURAL_EXCLUSIONS`] with a reason.
//!
//! ## The hole this closes
//!
//! `random_game.rs` and `bench_playout.rs` drive whole games to completion,
//! and `legal.rs`'s own unit tests check individual moves are OFFERED
//! correctly -- but nothing in the crate asserts that every card's `apply()`
//! effect has ever actually RUN. A card whose Rust behaviour is simply
//! wrong, but that no game this suite plays ever exercises, passes every
//! other test today. That is exactly the shape of gap that hid the
//! Barbarossa/Bach/Cook leader abilities until they were found by hand (see
//! the commit history around 2026-08 for those fixes). This test turns "no
//! game exercises card X" from silence into a build failure, and -- because
//! [`STRUCTURAL_EXCLUSIONS`] is checked both ways (missing-but-required AND
//! present-but-no-longer-needed) -- it is a ratchet: total required coverage
//! can only go up over time, never quietly regress by growing the allowlist.
//!
//! ## Where this corpus comes from, and why that is the whole rebuild
//!
//! Until 2026-08-06 this file measured coverage over nine frozen
//! `tests/fixtures/*.jsonl` recordings, dumped once from the Python engine
//! and checked in because they could never be regenerated (the generator
//! was Python code this project is retiring). That corpus is gone with the
//! Python engine it came from. This version measures coverage over games
//! THIS TEST PLAYS ITSELF, in-process, with [`game::play_game`] and the same
//! bot layer `arena.rs`/`selfplay.rs` use ([`make_seats`]/[`build_bots`]) --
//! the self-play capability this crate already had for training and
//! evaluation, pointed at a fixed, checked-in [`corpus`] of (player count,
//! bot spec, seed) triples instead of a live weight vector. Nothing here
//! reads a file; a clean checkout regenerates the exact same 236-card
//! verdict from source, forever, with no dependency on Python at all.
//!
//! ## What "exercised" means here, and why not the weaker reading
//!
//! A card counts as covered when it is the card named by some chosen
//! [`Move`] ([`Move::card`]) during self-play -- i.e. a real game actually
//! PLAYED it, not merely had the option to -- OR, for the handful of cards
//! no `Move` variant can ever name at all (below), when the game's own
//! post-move STATE proves a real game actually reached the equivalent
//! effect. Both readings share the same bar: a real, played game, not an
//! offered option. "Chosen" is the stricter, more useful bar precisely
//! because Barbarossa/Bach/Cook were all `apply()`-side effect bugs, not
//! legality bugs -- a card sitting in a deck or a hand, never actually
//! applied, would not have caught any of them, and this file does not count
//! that anywhere below.
//!
//! ## Granularity: `CardId`, not the printed/base name
//!
//! A handful of base names are printed on more than one physical card
//! across ages (e.g. `Aggression: Plunder` at I/II/III, each a DIFFERENT
//! `CardId` with a different magnitude -- `Card::base_name` vs `Card::name`,
//! `cards.rs`). Rolling those up to the base name for this test would let
//! "Plunder (I) got chosen once" excuse "Plunder (III)", which prices
//! completely differently, from ever being checked. Every `CardId` -- the
//! table's actual unit of identity, and the granularity a real per-card port
//! bug lives at -- is checked separately, in every coverage source below
//! (move-chosen and state-observed alike).
//!
//! ## Cards no `Move` can ever name, and the honest state-based bar for each
//!
//! Three families of card are never named by any `Move` variant, no matter
//! how many games are played -- traced through `game.rs`/`events.rs`/
//! `legal.rs` (not guessed from a raw "never appears" measurement, which
//! conflates real gaps with structural impossibilities) to confirm each
//! genuinely cannot be a `Move::card()`, not merely that some games didn't
//! happen to reach it:
//!
//! * **All 16 wonders** ([`Sets::wonder`]). A wonder is drafted with
//!   `Move::Take{slot}` -- a ROW SLOT INDEX, not a card name -- and
//!   `apply.rs::take_card` writes it straight to `p.wonder`
//!   (`state.rs::PlayerState::wonder`) the instant it is taken, without ever
//!   entering `hand_civil`. No `Move` variant carries a wonder's `CardId` at
//!   all: `Move::card()`'s match arms name every civil-hand card type
//!   EXCEPT `Take`. But a wonder-typed row slot is only ever offered while
//!   `p.wonder.is_none()` (a player is never already mid-wonder at the
//!   moment they take one), so the OWN ply's post-apply state --
//!   `state.players[decider].wonder` -- unambiguously names the wonder
//!   `Move::Take{slot}` just took. Unlike the Python-era version of this
//!   file, there is no name to parse here at all: `PlayerState::wonder` is
//!   already a `CardId` (DESIGN.md rule 1), so this read is a plain field
//!   access, not a string lookup.
//! * **All 10 Age A events** ([`Sets::events`]), `Development of
//!   Agriculture` .. `Development of Warfare`. `game::new_game` seeds them
//!   directly into `state.current_events` (bypassing `state.military_deck`,
//!   which Age A explicitly builds empty -- "no military DECK in Age A"),
//!   taking only the top `players + 2` of the shuffled ten and discarding
//!   the rest ("the rest are simply not in the game" -- `game.rs`'s own
//!   comment on that line) -- so a SINGLE game exposes at most 4-6 of the
//!   ten; only a corpus spanning several seeds reaches every one. The only
//!   way a card becomes nameable by a `Move` at all is `Move::PrepareEvent`,
//!   which requires the card to be in `p.hand_military` -- reachable only by
//!   drawing from `state.military_deck`, which never contains an Age A
//!   event. But `events::reveal_current_event` -- fired as a side effect of
//!   any player's `Move::PrepareEvent` popping the top of `current_events`,
//!   whatever age it belongs to -- pushes the popped card onto
//!   `state.past_events` (append-only, `events.rs`, in the same branch that
//!   applies its effect) whenever it is not a territory. So
//!   `state.past_events` containing a card is exactly "a real game applied
//!   this event's effect", the auto-resolved equivalent of "chosen": there
//!   is no decision that NAMES the Age A card itself, but there is still a
//!   real, played game that really reached and really applied it, and the
//!   state is where that fact lives. (This is not restricted to Age A --
//!   any event's real resolution shows up here the same way, only ever
//!   ADDING coverage on top of `Sets::chosen`, e.g. for age I/II/III events
//!   reached the ordinary way via `Move::PrepareEvent` naming themselves
//!   directly. Note `reveal_current_event`'s territory branch deliberately
//!   does NOT push to `past_events` -- only the non-territory branch does --
//!   so this cannot and does not accidentally paper over a territory that
//!   was only ever drawn, never actually colonized/prepared; territories are
//!   ordinary `Sets::chosen` material via `Move::PrepareEvent`/
//!   `Move::ColumbusColonize`/`Move::SendUnit`/`Move::SendBonus`, all of
//!   which name a territory `CardId` directly.)
//! * **Despotism** ([`Sets::government`]). The one card in the base game
//!   with `count == [0, 0, 0]` whose `CardType` (`Government`) is not
//!   `takes_workers()` (`cards.rs::CardType::takes_workers`: urban, unit or
//!   production only) -- it is assigned directly (`blank_player`, called
//!   from `game::new_game`) and never enters a deck, a hand or the card row,
//!   so it can never be the target of `Move::Develop` or `Move::Revolution`
//!   (needs to be drawn/in the row) OR of the §3.6/§4.3 "disband"
//!   `Move::Destroy` (needs `takes_workers()`, which `Government` never is).
//!   Every player's `government` field (`state.rs::PlayerState::government`,
//!   also a `CardId` directly) is set from the first ply of every game, so
//!   `state.players[i].government` naming Despotism at the start of any
//!   played game is a real game that was really assigned it and really
//!   played at least the opening of a turn under it -- the direct analogue
//!   of "a real game chose it", for a card that was never anybody's choice.
//!
//! ## The five other `count == [0, 0, 0]` cards are NOT in this list
//!
//! `Warriors`, `Agriculture`, `Bronze`, `Philosophy` and `Religion` -- §1.4's
//! starting tableau, `game::START_TECHS` -- are ALSO dealt directly by
//! `game::new_game`, exactly like Despotism, and never enter a deck either.
//! The difference is their `CardType` (`Infantry`/`Farm`/`Mine`/`Lab`/
//! `Temple`) IS `takes_workers()`, so unlike Despotism they ARE legal
//! `Move::Destroy` targets the moment a worker sits on them (the §3.6/§4.3
//! "disband a card you own" action, `legal.rs`'s `action_moves`, offered to
//! ANY worker-holding card with the matching action pool available, not
//! specially gated on how the card entered play) -- a real, ordinary
//! civil/military action, not a structural workaround. `Move::Destroy`'s
//! `card()` names the destroyed card directly, so this is normal
//! `Sets::chosen` material. It needed no special code path here: the
//! corpus below already plays `Move::Destroy` on all five without help,
//! confirmed by `covered_only_by_structural_reads` below excluding all five
//! (only the 27-card wonder/event/Despotism families ever show up there).
//! Nothing here relies on that continuing to be true by luck rather than
//! rule -- if a future change ever made disbanding a starting tech stop
//! being legal, `every_base_game_card_is_chosen_by_some_self_play_game`
//! would fail with these five names, which is the correct, actionable
//! outcome: grow [`corpus`], or promote the affected card to
//! [`STRUCTURAL_EXCLUSIONS`] with the trace for why it changed.
//!
//! As of 2026-08-06, [`corpus`] (120 games: every combination of 2/3/4
//! players, four bot specs, and ten seeds -- see its own doc comment) covers
//! all 236 cards with no exclusions needed, so [`STRUCTURAL_EXCLUSIONS`] is
//! empty. The allowlist mechanism -- and both ratchet-direction checks below
//! -- stay in place rather than being deleted: if some future card is found
//! to be GENUINELY unnameable by any `Move` AND unobservable in the played
//! state (nothing like that is known to exist today, past the three
//! families above), it belongs here with that trace as the reason, not
//! silently uncovered and not hidden behind an invented state signal that
//! doesn't actually prove the effect ran.

use std::collections::BTreeSet;

use tta::bots::greedy::{build_bots, make_seats};
use tta::bots::weighted::weights::Weights;
use tta::cards::{Card, CardId, CARDS};
use tta::game::{self, MOVE_CAP};
use tta::moves::Move;

/// `(printed card name, one-line reason, a cheap structural check of that
/// reason)`. The check is re-verified every run so a `card_table.rs` change
/// that quietly makes a reason false would fail loudly here instead of the
/// allowlist just going stale. Empty today (see the top doc comment) -- kept
/// as live infrastructure, not deleted, because the ratchet this file
/// implements only works if a future genuinely-unnameable-and-unobservable
/// card has somewhere honest to go.
struct Exclusion {
    name: &'static str,
    reason: &'static str,
    still_true: fn(&'static Card) -> bool,
}

const STRUCTURAL_EXCLUSIONS: &[Exclusion] = &[];

/// One self-play game to source coverage from: a player count, a
/// [`make_seats`] bot spec (comma-separated kinds, cycled round-robin over
/// the seats -- see `bots/greedy.rs`), and the seed [`game::new_game`] deals
/// from. Every field of every game here is fixed, so this whole file is
/// exactly as deterministic as any frozen fixture was -- rerunning it
/// replays the identical 120 games and gets the identical verdict, forever.
#[derive(Clone, Copy)]
struct GameSpec {
    players: u8,
    bots: &'static str,
    seed: u64,
}

/// 3 player counts x 4 bot specs x 10 seeds = 120 games, chosen to be the
/// smallest corpus this crate found (by widening from a smaller one until
/// the uncovered list emptied out) that reaches all 236 cards -- see the
/// commit this file landed in for the search. `random` is included at every
/// player count specifically because a uniform bot has no preference that
/// could avoid a weak-looking card the way an evaluator-driven bot might;
/// `greedy`/`weighted` add realistic play (contested wonders, wars actually
/// worth declaring); the mixed table exercises interactions a single-kind
/// table cannot (an aggression against a genuinely different play style, a
/// pact between two evaluators that disagree). Running on a fast difftest
/// build, this corpus plays in a few seconds -- comfortably inside this
/// file's budget as a test that runs on every `cargo test`. If a future card
/// needs more games to reach, widen this (more seeds first -- cheapest lever
/// -- then more specs) rather than reaching for [`STRUCTURAL_EXCLUSIONS`];
/// the standing rule for every allowlist in this suite is empty.
fn corpus() -> Vec<GameSpec> {
    let mut v = Vec::new();
    for players in [2u8, 3, 4] {
        for bots in ["random", "greedy", "weighted", "random,greedy,weighted"] {
            for seed in 1..=10u64 {
                v.push(GameSpec { players, bots, seed });
            }
        }
    }
    v
}

/// Every coverage source this file trusts, kept separate rather than
/// pre-unioned so [`covered_only_by_structural_reads`] can report which
/// cards would be uncovered under move-chosen coverage alone -- the fact
/// that check depends on, see the top doc comment's "five other cards"
/// section.
#[derive(Default)]
struct Sets {
    /// Every card some chosen [`Move::card()`] ever named.
    chosen: BTreeSet<CardId>,
    /// Wonders actually taken -- see the top doc comment's wonder bullet.
    wonder: BTreeSet<CardId>,
    /// Events actually resolved -- see the top doc comment's events bullet.
    events: BTreeSet<CardId>,
    /// Governments actually assigned -- see the top doc comment's Despotism
    /// bullet (also catches every OTHER government trivially, since a
    /// government a game later develops into shows up in `chosen` too, but
    /// there is no harm in a card appearing in both sets).
    government: BTreeSet<CardId>,
}

impl Sets {
    fn union(&self) -> BTreeSet<CardId> {
        self.chosen
            .iter()
            .chain(self.wonder.iter())
            .chain(self.events.iter())
            .chain(self.government.iter())
            .copied()
            .collect()
    }
}

/// Play one [`GameSpec`] to the end, folding every coverage source it
/// produces into `sets`. All four sources are read from the SAME playthrough
/// rather than four separate passes (unlike the old fixture-reading
/// version, which had four independent file scans to make): a self-play
/// game is not a file that can be re-read for free, so this reads each
/// signal off the one pass that already has to happen.
fn play_and_record(spec: &GameSpec, sets: &mut Sets) {
    let seats = make_seats(spec.bots, spec.players, Weights::defaults())
        .unwrap_or_else(|e| panic!("bad bot spec {:?} in corpus(): {e}", spec.bots));
    let mut bots = build_bots(&seats, spec.seed as i64);
    let mut state = game::new_game(spec.players, spec.seed);

    // §1.4: every player's starting government, from ply 0 -- the
    // Despotism bullet's read.
    for p in &state.players[..spec.players as usize] {
        sets.government.insert(p.government);
    }

    let mut moves = 0usize;
    while !state.game_over {
        assert!(
            moves < MOVE_CAP,
            "hit the move cap ({} players, {:?}, seed {}) -- the turn loop is not closing",
            spec.players,
            spec.bots,
            spec.seed
        );
        let decider = state.current as usize;
        let mv = bots[decider].pick(&state);
        if let Some(card) = mv.card() {
            sets.chosen.insert(card);
        }
        // Read AFTER `step`: `Move::Take` never carries a wonder's `CardId`
        // itself (the wonder bullet above), so the only way to name it is
        // the post-apply state the take just produced.
        let taking_wonder = matches!(mv, Move::Take { .. });
        game::step(&mut state, mv);
        moves += 1;
        if taking_wonder {
            let w = state.players[decider].wonder;
            if !w.is_none() {
                sets.wonder.insert(w);
            }
        }
    }

    // `past_events` is append-only for the whole game (`events.rs`), so
    // reading it once at game-over sees every event this game ever
    // resolved -- the events bullet's read.
    for &card in state.past_events.as_slice() {
        sets.events.insert(card);
    }
}

/// Cards covered by [`Sets::wonder`]/[`Sets::events`]/[`Sets::government`]
/// but NOT by [`Sets::chosen`] -- i.e. the cards that would silently regress
/// to uncovered if move-chosen coverage were the only source this file
/// trusted. Used only to keep the top doc comment's "five other cards"
/// claim (that `Warriors`/`Agriculture`/`Bronze`/`Philosophy`/`Religion` are
/// covered by an ordinary `Move::Destroy`, not by a state read) checked
/// rather than asserted from memory: if it ever grew to include one of
/// those five, the doc comment above would be wrong and this file's
/// [`STRUCTURAL_EXCLUSIONS`] reasoning would need to grow a fourth family to
/// match.
fn covered_only_by_structural_reads(sets: &Sets) -> BTreeSet<CardId> {
    sets.wonder
        .iter()
        .chain(sets.events.iter())
        .chain(sets.government.iter())
        .filter(|id| !sets.chosen.contains(id))
        .copied()
        .collect()
}

#[test]
fn every_base_game_card_is_chosen_by_some_self_play_game() {
    let mut sets = Sets::default();
    for spec in corpus() {
        play_and_record(&spec, &mut sets);
    }
    let covered = sets.union();

    // The top doc comment's "five other cards" section claims
    // `Warriors`/`Agriculture`/`Bronze`/`Philosophy`/`Religion` are covered
    // by an ordinary `Move::Destroy`, not by a state read, and are
    // therefore never in this set. Checked here rather than trusted from
    // memory: if a future change made disbanding a starting tech illegal,
    // this corpus would still cover the other 231 cards fine and the
    // failure below would otherwise be the only signal -- this turns it
    // into a targeted one naming exactly which of the five regressed.
    let structural_only = covered_only_by_structural_reads(&sets);
    let unexpectedly_structural_only: Vec<&str> = structural_only
        .iter()
        .map(|id| id.name())
        .filter(|name| {
            matches!(*name, "Warriors" | "Agriculture" | "Bronze" | "Philosophy" | "Religion")
        })
        .collect();
    assert!(
        unexpectedly_structural_only.is_empty(),
        "these starting techs are no longer being chosen via an ordinary Move::Destroy \
         (only covered here by the government/wonder/events state reads) -- either \
         legal.rs's §3.6/§4.3 disband rule changed under them, or this corpus's bots \
         stopped choosing to disband a starting tech; re-trace the top doc comment's \
         \"five other cards\" section and either fix the code or move the affected \
         card(s) into a new STRUCTURAL_EXCLUSIONS entry with the new reason: \
         {unexpectedly_structural_only:?}"
    );

    // Every exclusion must resolve to a real card, must still satisfy the
    // structural fact its reason claims, and -- ratchet direction 1 -- must
    // still actually be uncovered. An allowlist entry for a card this
    // corpus now covers is exactly the rot this test exists to prevent: it
    // would hide a REAL regression (that card silently losing its only
    // exercise) behind "it was never covered anyway".
    let mut stale_but_now_covered = Vec::new();
    let mut reason_no_longer_true = Vec::new();
    let mut excluded_ids: BTreeSet<CardId> = BTreeSet::new();
    for ex in STRUCTURAL_EXCLUSIONS {
        let id = CardId::by_name(ex.name)
            .unwrap_or_else(|| panic!("STRUCTURAL_EXCLUSIONS: no such card {:?}", ex.name));
        if !(ex.still_true)(id.get()) {
            reason_no_longer_true.push(ex.name);
        }
        if covered.contains(&id) {
            stale_but_now_covered.push(ex.name);
        }
        excluded_ids.insert(id);
    }
    assert!(
        reason_no_longer_true.is_empty(),
        "STRUCTURAL_EXCLUSIONS entries whose stated structural reason no longer holds \
         (card_table.rs changed under them -- re-verify and fix the entry, don't just \
         delete it): {:#?}",
        reason_no_longer_true
            .iter()
            .map(|name| {
                let reason = STRUCTURAL_EXCLUSIONS.iter().find(|e| &e.name == name).unwrap().reason;
                format!("{name}: {reason}")
            })
            .collect::<Vec<_>>()
    );
    assert!(
        stale_but_now_covered.is_empty(),
        "STRUCTURAL_EXCLUSIONS entries for cards this corpus now actually covers -- \
         remove these from the allowlist, coverage only ratchets up: {stale_but_now_covered:?}"
    );

    // Ratchet direction 2: every OTHER card must be covered.
    let mut uncovered: Vec<&'static str> = CARDS
        .iter()
        .enumerate()
        .filter_map(|(i, c)| {
            let id = CardId(i as u16);
            if excluded_ids.contains(&id) || covered.contains(&id) {
                None
            } else {
                Some(c.name)
            }
        })
        .collect();
    uncovered.sort_unstable();

    eprintln!(
        "card coverage: {}/{} cards covered by self-play (move-chosen or state-observed; \
         {} of those only by a state read -- see covered_only_by_structural_reads), \
         {} structurally excluded, {} uncovered",
        covered.len(),
        CARDS.len(),
        structural_only.len(),
        STRUCTURAL_EXCLUSIONS.len(),
        uncovered.len(),
    );
    assert!(
        uncovered.is_empty(),
        "{} card(s) are neither chosen/state-observed by any game in corpus() nor named on \
         STRUCTURAL_EXCLUSIONS above -- either widen corpus() (more seeds first, then more bot \
         specs) so some game plays one, or, if tracing game.rs/legal.rs/events.rs shows it is \
         genuinely unreachable as a Move AND unobservable in the played state, add it to \
         STRUCTURAL_EXCLUSIONS with that trace as the reason:\n    {}",
        uncovered.len(),
        uncovered.join("\n    ")
    );
}
