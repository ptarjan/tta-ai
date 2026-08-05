//! The random-legal-move driver, shared by every test binary that needs one.
//!
//! ## Why this is a module and not a copy
//!
//! Each file directly under `tests/` is compiled as its own crate with no
//! access to another test file's private items, so for a while `Rng`,
//! [`blocked_on`] and [`play_random`] existed twice -- once in
//! `tests/random_game.rs`, once in `tests/bench_playout.rs`
//! -- with a comment in each asking the next person to mirror their changes
//! into the other. That is the bug class DESIGN.md names: the same rule
//! written in two registries, and nothing that fails when they disagree. They
//! did disagree. On 2026-08-05 `random_game.rs` unblocked `Move::PrepareEvent`,
//! `Move::Bid`/`Move::BidPass`/`Move::Send*`, `Move::OfferPact`,
//! `Move::Aggression` and `Move::Defend`/`Move::DefendDone` as those modules
//! landed; `bench_playout.rs` kept blocking all six, so the benchmark was
//! silently measuring a much smaller game than the test suite played -- which
//! is the one thing a throughput number must not do.
//!
//! Cargo does not treat a SUBDIRECTORY of `tests/` as a test target, so this
//! file is a plain module both binaries can `mod common;`. There is now one
//! copy. Change it here and both move together.
//!
//! ## The remaining third copy
//!
//! `tools/bench_python_playout.py` applies the same filter to the Python
//! engine's move list so that "Rust games/sec vs Python games/sec" compares
//! the same restricted game rather than two different ones. That copy cannot
//! share this code and is kept in sync by hand; its module docstring points
//! back here. It goes away with the Python engine.
//!
//! ## What the filter is for
//!
//! Everything [`blocked_on`] lists is a MISSING MODULE, not a rule -- with
//! one stated exception (`Move::Resign`). Each arm cites the `unimplemented!`
//! it is dodging. Delete an arm when that module lands and the driver starts
//! exercising it; nothing else changes.
//!
//! Every skip is attributable: if a position ever offers ONLY blocked moves,
//! [`play_random`] returns [`Played::Blocked`] carrying the reasons, so a stop
//! can never be mistaken for a stuck engine.

use tta::game;
use tta::moves::Move;
use tta::state::GameState;

/// A deterministic driver RNG. Deliberately not `rng::PyRandom`: this picks
/// the driver's MOVES, which no Python run shares, and reaching for the
/// engine's CPython-compatible stream here would suggest the two were meant
/// to line up. SplitMix64 -- eight lines, no dependency.
pub struct Rng(pub u64);

impl Rng {
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    pub fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

/// Why the random driver may not play `mv` yet, or `None` if it may.
pub fn blocked_on(mv: Move) -> Option<&'static str> {
    match mv {
        // `Move::OfferPact` and `Move::Aggression` were both here, on the
        // claim that the decision they hand to the other player was unported.
        // Both claims went stale: `interact::push_choice` takes the pact offer
        // and `interact::start_defense` takes the aggression defense, and the
        // differential fixtures now play aggressions through to resolution
        // with zero divergences. Removed 2026-08-05 after checking that
        // random play reaches and resolves both. `Move::Defend`/
        // `Move::DefendDone` went with them: they were blocked only because
        // `Move::Aggression` was, so `Pending::Defense` never arose and
        // neither could ever be legal.
        //
        // `Move::PrepareEvent` was here too, on `events.rs`. `apply_gains`
        // and §12.5.2 final scoring landed in `044fd92`/`3792e10`, and
        // `reveal_current_event`/`resolve_event`/`_apply_player_block` landed
        // 2026-08-05. Revealing a territory event now genuinely opens a
        // colonization auction (`interact::start_auction`/`colonize`, both
        // already fully wired and merely unreachable before), so
        // `Move::Bid`/`Move::BidPass`/`Move::SendUnit`/`Move::SendBonus`/
        // `Move::SendDone` came off with it.
        //
        // `Move::Choose` was never here: since `economy::end_of_turn` was
        // rewired onto `interact::discard_excess_military`, a genuine
        // multi-card §6.6 step-1 discard is the one already-reachable
        // `Pending::Choice`, and `interact::resolve_choice`'s dispatch is
        // exhaustive over every `ChoiceKind` -- there is no unported resolver
        // behind it to fall into.

        // combat.rs `apply_war_spoils` handles War over Territory and War
        // over Culture. War over Technology's spoils are a CHOICE (science or
        // blue technologies, and the victor may mix them). The panic lands a
        // turn later, at resolution, so the DECLARATION is what to skip.
        Move::War { card, .. } if card.get().base_name == "War over Technology" => {
            Some("interact.rs: War over Technology spoils are a decision")
        }

        // `Move::PlayAction` was here, with an `action_card_is_blocked` helper
        // naming three gaps in `apply.rs::h_play_action`. All three are shut,
        // and the arm outlived every one of them:
        //
        //   * "two cards print a per-player-count magnitude the generated card
        //     table cannot carry" closed in `2b5c4a3` -- `gen_cards.py` emits
        //     the same `[i16; 3]` count table it already used for
        //     `strongestPlayers`.
        //   * a `freeCivilAction` order with 2+ legal ways to obey it is a
        //     real choice -- and it is offered as one: `h_play_action` pushes
        //     `QueueItem::FreeCivil` and `interact::resolve_choice` answers
        //     `ChoiceKind::FreeCivil`.
        //   * `gainFoodOrResources` is a real choice -- `interact::
        //     apply_card_gains` opens `ChoiceKind::FoodOrRes` for it.
        //
        // `rust/src/apply.rs` has no `unimplemented!` left at all. Removed
        // 2026-08-05 after confirming random play at 2/3/4p reaches and
        // resolves both choice kinds.

        // Not a missing module: resigning is a legal, fully-ported move that
        // ends the game early (§5.11), which would make "a game played to the
        // end" mean nothing. `random_game.rs::resigning_ends_the_game` plays
        // it on purpose.
        Move::Resign => Some("would end the game early; tested separately"),

        _ => None,
    }
}

/// How far a driven game got.
pub enum Played {
    /// Reached §12.5 final scoring.
    Finished(game::Outcome),
    /// Every legal move was blocked on an unported module. Carries the
    /// reasons, so a stop can never be mistaken for a stuck engine.
    Blocked(Vec<&'static str>),
}

/// Play one game with random legal moves until it ends or the driver runs out
/// of moves it is allowed to make.
pub fn play_random(num_players: u8, seed: u64) -> (GameState, Played) {
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
            .filter(|&m| blocked_on(m).is_none())
            .collect();
        if playable.is_empty() {
            let mut why: Vec<&'static str> = legal
                .as_slice()
                .iter()
                .filter_map(|&m| blocked_on(m))
                .collect();
            why.sort_unstable();
            why.dedup();
            return (state, Played::Blocked(why));
        }
        let n = playable.len();
        let mv = playable[rng.below(n)];
        game::step(&mut state, mv);
        moves += 1;
    }
}
