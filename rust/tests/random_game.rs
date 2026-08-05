//! A whole game, start to finish, at 2, 3 and 4 players.
//!
//! This is the port's end-to-end test for `game.rs`: `new_game` deals, a
//! random legal-move driver answers every decision, and the turn loop must
//! carry the game from the Age A row through four ages to §12.5 final
//! scoring without panicking. It is deliberately NOT a differential test --
//! `tests/differential.rs` owns agreement with Python ply by ply, and this
//! owns "the turn loop closes": ages advance, the row refills, rounds wrap,
//! the last round arrives, the game ends and somebody wins.
//!
//! ## The move filter
//!
//! `events.rs` is not ported and several of `interact.rs`'s decision-driven
//! halves are still `unimplemented!()`. A random player reaches all of them
//! within a few turns, and `apply.rs`/`combat.rs` panic -- correctly and
//! loudly -- when it does. [`blocked_on`] is the list of moves the driver may
//! not choose yet, each naming the module it waits on; delete an arm when
//! that module lands and the driver starts exercising it. Nothing else in
//! this file changes.
//!
//! Every skip is attributable: if a position ever offers ONLY blocked moves,
//! [`play_random`] returns [`Played::Blocked`] carrying the reasons, and the
//! tests fail with them rather than with a stuck engine. There is exactly one
//! shim, [`settle_military_discard`], and its doc comment says what has to
//! land for it to be deleted.

use tta::cards::{Age, CardId, Special, CARDS};
use tta::game;
use tta::moves::Move;
use tta::state::{GameState, Phase};

// ------------------------------------------------------------ the driver

/// A deterministic driver RNG. Deliberately not `rng::PyRandom`: this picks
/// the driver's MOVES, which no Python run shares, and reaching for the
/// engine's CPython-compatible stream here would suggest the two were meant
/// to line up. SplitMix64 -- eight lines, no dependency.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

/// Why the random driver may not play `mv` yet, or `None` if it may.
///
/// Everything here is a MISSING MODULE, not a rule -- with one stated
/// exception (`Resign`). Each arm cites the `unimplemented!` it is dodging.
fn blocked_on(state: &GameState, mv: Move) -> Option<&'static str> {
    let me = state.me();
    match mv {
        // apply.rs `apply`: needs `interact::push_choice` -- a pact offer is
        // a decision handed to the other player.
        Move::OfferPact { .. } => Some("interact.rs: a pact offer is the other player's decision"),

        // apply.rs `h_aggression`: the declaration is ported, the defender's
        // response (`interact::start_defense`) is not.
        Move::Aggression { .. } => Some("interact.rs: the aggression defense is a decision"),

        // combat.rs `apply_war_spoils` handles War over Territory and War
        // over Culture. War over Technology's spoils are a CHOICE (science or
        // blue technologies, and the victor may mix them). The panic lands a
        // turn later, at resolution, so the DECLARATION is what to skip.
        Move::War { card, .. } if card.get().base_name == "War over Technology" => {
            Some("interact.rs: War over Technology spoils are a decision")
        }

        // apply.rs `apply`: needs `events::reveal_current_event`.
        Move::PrepareEvent { .. } => Some("events.rs: revealing the current event"),

        // apply.rs `h_play_action`. Three separate gaps, all on action cards:
        // a `freeCivilAction` order with 2+ legal ways to obey it is a real
        // choice; `gainFoodOrResources` is a real choice; and two cards print
        // a per-player-count magnitude the generated card table cannot carry.
        // The first is skipped for EVERY `freeCivilAction` card rather than
        // only the ambiguous ones -- deciding which are ambiguous means
        // reproducing `h_play_action`'s pre-`pay_ca` `revolt_ok` snapshot out
        // here, and a test that re-derives engine internals to predict the
        // engine is worse than one that skips a few playable cards.
        Move::PlayAction { card } if action_card_is_blocked(card) => {
            Some("interact.rs / card table: an action card's ordered choice")
        }

        // apply.rs `one_time_culture`: Hollywood and Internet score
        // "the effective output of a specific set of buildings", which needs
        // a public `effects::building_output`. Only the COMPLETING step
        // panics; skipping every step of those two wonders is the cheap way
        // to stay out of it.
        Move::WonderStep { .. }
            if matches!(me.wonder.get().base_name, "Hollywood" | "Internet") =>
        {
            Some("effects.rs: Hollywood/Internet completion culture needs building_output")
        }

        // Responses to a decision nothing can have opened yet. `legal.rs`
        // does not generate them today; listed so that the day it does, the
        // driver skips them loudly instead of panicking inside `apply`.
        Move::Bid { .. }
        | Move::BidPass
        | Move::Defend { .. }
        | Move::DefendDone
        | Move::SendUnit { .. }
        | Move::SendBonus { .. }
        | Move::SendDone
        | Move::Choose { .. } => Some("interact.rs: a response to an open decision"),

        // Not a missing module: resigning is a legal, fully-ported move that
        // ends the game early (§5.11), which would make "a game played to the
        // end" mean nothing. `resigning_ends_the_game` plays it on purpose.
        Move::Resign => Some("would end the game early; tested separately"),

        _ => None,
    }
}

/// Whether playing this action card would reach one of `h_play_action`'s
/// three `unimplemented!` arms.
fn action_card_is_blocked(card: CardId) -> bool {
    card.get().special.iter().any(|s| {
        matches!(
            s,
            Special::FreeCivilAction(_)
                | Special::GainFoodOrResources(_)
                | Special::CulturePerCivilizationWithMoreCulture
                | Special::ResourcesForMilitaryUnitsPerStrongerCivilization
        )
    })
}

/// §6.6 step 1, settled just before the turn ends.
///
/// TEMPORARY, AND THE ONLY SHIM IN THIS FILE. There are two implementations
/// of "discard down to the military hand limit" in the crate right now:
/// `interact::discard_excess_military`, which opens a real decision the
/// player answers, and `economy::discard_excess_military`, a private copy
/// written before `state.pending` existed which `unimplemented!()`s the
/// multi-option case. `economy::end_of_turn` calls the private one, so a
/// random game panics within a few rounds. `interact.rs`'s own doc comment
/// carries the request to economy.rs's owner to delete the copy and call the
/// real one -- exactly the "two registries, nothing fails when they
/// disagree" bug class DESIGN.md is about.
///
/// Until that lands, the driver runs the real implementation itself, right
/// where §6.6 puts it (immediately before the rest of the end-of-turn
/// sequence, so the player still had the cards for the whole of their turn),
/// and answers the decision with a random legal option. `economy`'s copy then
/// finds the hand already legal and no-ops. Delete this function and its one
/// call site when economy.rs is rewired; nothing else changes.
fn settle_military_discard(state: &mut GameState, rng: &mut Rng) {
    while tta::interact::discard_excess_military(state, state.current) {
        let options = tta::interact::pending_moves(state);
        assert!(!options.is_empty(), "a pending discard with nothing to answer it");
        let n = options.len();
        game::step(state, options.as_slice()[rng.below(n)]);
    }
}

/// How far a driven game got.
enum Played {
    /// Reached §12.5 final scoring.
    Finished(game::Outcome),
    /// Every legal move was blocked on an unported module. Carries the
    /// reasons, so a stop can never be mistaken for a stuck engine.
    Blocked(Vec<&'static str>),
}

/// Play one game with random legal moves until it ends or the driver runs out
/// of moves it is allowed to make.
fn play_random(num_players: u8, seed: u64) -> (GameState, Played) {
    let mut state = game::new_game(num_players, seed);
    let mut rng = Rng(seed ^ 0x5EED);
    let mut moves = 0usize;

    loop {
        if state.game_over {
            return (state, Played::Finished(game::Outcome { moves_played: moves, move_cap_hit: false }));
        }
        assert!(
            moves < game::MOVE_CAP,
            "hit the move cap at turn {} round {}: the turn loop is not closing",
            state.turn,
            state.round
        );
        let legal = tta::legal::legal_moves(&state);
        assert!(
            !legal.is_empty(),
            "no legal move at all for player {} in phase {:?} (turn {}, round {})",
            state.current,
            state.phase,
            state.turn,
            state.round
        );
        let playable: Vec<Move> = legal
            .as_slice()
            .iter()
            .copied()
            .filter(|&m| blocked_on(&state, m).is_none())
            .collect();
        if playable.is_empty() {
            let mut why: Vec<&'static str> = legal
                .as_slice()
                .iter()
                .filter_map(|&m| blocked_on(&state, m))
                .collect();
            why.sort_unstable();
            why.dedup();
            return (state, Played::Blocked(why));
        }
        let n = playable.len();
        let mv = playable[rng.below(n)];
        if mv == Move::EndTurn {
            settle_military_discard(&mut state, &mut rng);
        }
        game::step(&mut state, mv);
        moves += 1;
    }
}

/// End turns (settling §6.6 step 1 as it goes) until the game is over or
/// `max` turns have passed. Used by the tests that exercise the row/age
/// machinery, which needs many more turns than a random game survives.
fn end_turns(state: &mut GameState, rng: &mut Rng, max: usize) {
    for _ in 0..max {
        if state.game_over {
            return;
        }
        settle_military_discard(state, rng);
        game::step(state, Move::EndTurn);
    }
}

// ------------------------------------------------------------- invariants

/// Everything that must hold of a driven game whether it finished or stopped.
/// These are the turn loop's own invariants, so they do not weaken when the
/// unported modules land -- they get MORE coverage, not less.
fn assert_sane(state: &GameState, played: &Played) {
    // Whoever is to move is a real, unresigned player.
    let d = game::current_player(state) as usize;
    assert!(d < state.num_players as usize);

    // The row never holds more than thirteen and never holds a fake card.
    assert_eq!(state.card_row.len(), tta::state::ROW_SIZE);
    for card in state.card_row {
        if !card.is_none() {
            assert!(card.kind().is_civil_row(), "{card:?} is not a civil-row card");
        }
    }

    // Ages only ever move forward, and civil/military stay locked together
    // (`advance_age` sets both; nothing else writes either).
    assert_eq!(state.age_civil, state.age_military);

    // §12.3: a deadline, once set, is in the past or the present -- never
    // silently overshot, which is the only way a game runs forever.
    if let Some(end) = state.final_round_end {
        assert!(state.round <= end + 1, "round {} is past the deadline {end}", state.round);
    }

    if let Played::Finished(outcome) = played {
        assert!(state.game_over);
        assert!(!outcome.move_cap_hit);
        assert_eq!(state.phase, Phase::Done);
        assert!(game::is_over(state));
        assert!(outcome.moves_played > 50, "suspiciously short: {}", outcome.moves_played);
        assert!(
            state.final_round_end.is_some() || state.forced_winner.is_some(),
            "the game ended without a §12.3 deadline or a §5.11 forced winner"
        );
        let scores = game::scores(state);
        assert_eq!(scores.len(), state.num_players as usize);
        let winners = game::winners(state);
        assert!(!winners.is_empty(), "somebody has to win");
        if state.forced_winner.is_none() {
            let best = scores
                .iter()
                .zip(state.players.iter())
                .filter(|(_, p)| !p.resigned)
                .map(|(s, _)| *s)
                .max()
                .unwrap();
            for &w in &winners {
                assert_eq!(scores[w as usize], best);
            }
        }
    }
}

// -------------------------------------------------------------- the tests

/// THE finish line: a fresh game played start to finish with random legal
/// moves, at every supported player count, without panicking.
///
/// This is what makes the port playable end to end. It exercises every part
/// of `game.rs` in anger -- setup, the row sweep and refill, four age
/// advances with antiquation and the §12.2.4 population tax, the politics/
/// actions phase machine, `economy::end_of_turn`, round wrapping, the §12.3
/// deadline and §12.5 scoring -- against move sequences no hand-written test
/// would produce.
///
/// [`blocked_on`] is what it does NOT exercise: the moves waiting on
/// `interact.rs`'s decision-driven halves and on `events.rs`. Those shrink as
/// those modules land, and nothing here changes when they do.
#[test]
fn a_random_game_plays_to_the_end() {
    for num_players in 2..=4u8 {
        for seed in 0..12u64 {
            let (state, played) = play_random(num_players, seed);
            assert_sane(&state, &played);
            match played {
                Played::Finished(_) => {}
                Played::Blocked(why) => panic!(
                    "{num_players}p seed {seed} stopped in round {} of age {:?}: {why:?}",
                    state.round, state.age_civil
                ),
            }
            assert_eq!(state.age_civil, Age::IV, "a finished game is in Age IV");
        }
    }
}

/// Ages must arrive one at a time and in printed order, at every player
/// count. A random game reaches Age IV, but it reaches it by a route that
/// depends on what got taken; ending every turn immediately isolates the row
/// machinery (`replenish`/`deal`/`advance_age`) from everything else, so a
/// failure here names it directly.
#[test]
fn ages_advance_in_printed_order_through_the_whole_supply() {
    for num_players in 2..=4u8 {
        let mut state = game::new_game(num_players, 42);
        let mut seen = vec![state.age_civil];
        let mut rng = Rng(42);
        // Round 2 onwards, one start-of-turn replenish per turn, which is the
        // only thing that draws the civil deck down.
        state.round = 2;
        for _ in 0..2000 {
            if state.game_over {
                break;
            }
            end_turns(&mut state, &mut rng, 1);
            if seen.last() != Some(&state.age_civil) {
                seen.push(state.age_civil);
            }
        }
        assert_eq!(
            seen,
            vec![Age::A, Age::I, Age::II, Age::III, Age::IV],
            "{num_players}p: ages must advance in printed order, one at a time"
        );
        assert!(state.civil_deck.is_empty(), "Age IV has no civil deck (§12.2)");
        assert!(state.military_deck.is_empty());
        // §2.1: every card swept off the left of the row is recorded.
        let swept: usize = state.civil_discard.iter().map(|l| l.len()).sum();
        assert!(swept > 20, "{num_players}p: only {swept} cards recorded as swept");
        // §12.3: Age IV set the deadline, and the game ended on it.
        assert!(state.final_round_end.is_some());
    }
}

/// A game that runs out of rounds ends by itself, with nobody having to ask.
#[test]
fn the_game_ends_when_the_last_round_ends() {
    let mut state = game::new_game(3, 1);
    let mut rng = Rng(1);
    state.round = 2;
    end_turns(&mut state, &mut rng, 3000);
    assert!(state.game_over, "ending every turn immediately must still finish the game");
    assert_eq!(state.phase, Phase::Done);
    assert_eq!(state.age_civil, Age::IV);
    let end = state.final_round_end.expect("§12.3 deadline");
    assert!(state.round >= end);
    assert!(!game::winners(&state).is_empty());
}

/// §5.11: resigning down to one player ends the game at once, and that player
/// wins however far behind they are.
#[test]
fn resigning_ends_the_game() {
    let mut state = game::new_game(2, 3);
    // Round 1 has no politics phase (§1.9), so play into round 2 first.
    while state.round < 2 {
        game::step(&mut state, Move::EndTurn);
    }
    assert_eq!(state.phase, Phase::Politics);
    let me = state.current;
    state.players[(1 - me) as usize].culture = 999;
    game::step(&mut state, Move::Resign);
    assert!(state.players[me as usize].resigned);
    assert!(state.game_over);
    assert_eq!(state.forced_winner, Some(1 - me));
    assert_eq!(game::winners(&state), vec![1 - me]);
}

/// §1.9: round one has no politics phase and taking a card from the row is
/// the only action. A regression here is invisible in a scored game -- the
/// same cards get taken either way, one round later.
#[test]
fn round_one_is_take_or_end_turn_only() {
    let state = game::new_game(3, 17);
    assert_eq!(state.phase, Phase::Actions);
    for mv in tta::legal::legal_moves(&state).as_slice() {
        assert!(
            matches!(mv, Move::Take { .. } | Move::EndTurn),
            "§1.9 allows only take/end_turn in round 1, got {mv:?}"
        );
    }
    assert_eq!(state.players[0].military_actions, 0, "no politics phase to spend them in");
}

/// Nothing may create a civil card. Every copy in play, in a hand, in the
/// deck, in the row or in either record of cards that left play must be a
/// copy the printed deck actually contains at this player count. This is the
/// census `engine/bots/counting` depends on, and the bug class DESIGN.md
/// names -- "present in this registry, absent from that one, and nothing
/// fails when they disagree" -- is exactly what it catches.
#[test]
fn no_civil_card_appears_more_often_than_it_is_printed() {
    for num_players in 2..=4u8 {
        let (state, _) = play_random(num_players, 8);
        let mut seen: Vec<CardId> = Vec::new();
        seen.extend(state.card_row.iter().copied().filter(|c| !c.is_none()));
        seen.extend(state.civil_deck.as_slice().iter().copied());
        for p in state.players[..state.num_players as usize].iter() {
            seen.extend(p.hand_civil.as_slice().iter().copied());
            seen.extend(p.techs.iter().map(|(id, _)| id));
            seen.extend(p.completed_wonders.as_slice().iter().copied());
            for c in [p.leader, p.wonder] {
                if !c.is_none() {
                    seen.push(c);
                }
            }
        }
        for age in 0..5 {
            seen.extend(state.civil_discard[age].as_slice().iter().copied());
            seen.extend(state.civil_removed[age].as_slice().iter().copied());
        }
        for id in (0..CARDS.len()).map(|i| CardId(i as u16)) {
            let printed = id.get().count[num_players as usize - 2] as usize;
            if printed == 0 || !id.kind().is_civil_row() {
                continue;
            }
            let found = seen.iter().filter(|&&c| c == id).count();
            assert!(
                found <= printed,
                "{id:?}: {found} copies accounted for but only {printed} printed at \
                 {num_players}p"
            );
        }
    }
}
