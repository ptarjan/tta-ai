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
//! The driver itself -- the RNG, the "which moves may I not play yet" filter
//! and the game loop -- lives in [`mod@common`], shared with
//! `tests/bench_playout.rs` so the benchmark cannot end up measuring a
//! different game than this test plays. `common::blocked_on`'s doc comment is
//! the authority on what is skipped and why. A few of `interact.rs`'s
//! decision-driven halves are still `unimplemented!()`; a random player
//! reaches them within a few turns and `apply.rs`/`combat.rs` panic --
//! correctly and loudly -- when it does.
//!
//! Every skip is attributable: if a position ever offers ONLY blocked moves,
//! [`common::play_random`] returns [`Played::Blocked`] carrying the reasons,
//! and the tests fail with them rather than with a stuck engine. There is no
//! shim here: an earlier revision of this file carried one, [`resolve_pending`]'s
//! doc comment says what it worked around and why it is gone.

use tta::cards::{Age, CardId, CARDS};
use tta::game;
use tta::moves::Move;
use tta::state::{GameState, Phase};

use crate::common::{play_random, Played, Rng};

// ------------------------------------------------------------ the driver

/// Answer whatever is pending with a uniformly random legal response,
/// looping until nothing is left open. §6.6 step 1's genuine-choice case
/// suspends `Move::EndTurn` on a real `Pending::Choice` (`economy::
/// end_of_turn` -> `interact::discard_excess_military`); `legal_moves`
/// returns that decision's options while it is open, and answering it can
/// itself re-suspend on a second discard decision, or -- once the hand is
/// legal -- drain `state.queue`'s `EndOfTurn` continuation and actually
/// complete the turn (`game::resume_end_turn`'s doc comment). Looping on
/// `state.pending` rather than on a fixed number of rounds is what makes
/// this correct either way.
fn resolve_pending(state: &mut GameState, rng: &mut Rng) {
    while !state.pending.is_empty() {
        let options = tta::legal::legal_moves(state);
        assert!(!options.is_empty(), "an open decision with nothing to answer it");
        let n = options.len();
        game::step(state, options.as_slice()[rng.below(n)]);
    }
}

/// End turns until the game is over or `max` turns have passed. Used by the
/// tests that exercise the row/age machinery, which needs many more turns
/// than a random game survives.
fn end_turns(state: &mut GameState, rng: &mut Rng, max: usize) {
    for _ in 0..max {
        if state.game_over {
            return;
        }
        game::step(state, Move::EndTurn);
        resolve_pending(state, rng);
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
/// [`common::blocked_on`] is what it does NOT exercise: the moves waiting on
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
