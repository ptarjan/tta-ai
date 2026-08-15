//! The CGE-app harness: derive what a human must type ([`fields`]), catch a
//! drifting mirror before it fabricates a game ([`mirror`]), keep an
//! append-only per-game record ([`record`]), and drive the operator loop
//! ([`play`]). Built on [`crate::advisor`], which already does the hard part
//! -- mirroring the board, ranking moves, fuzzy card names, never crashing on
//! bad input.
//!
//! Ported from the Python `harness/` package; see each submodule's own doc
//! comment for the file it mirrors.

pub mod fields;
pub mod mirror;
pub mod play;
pub mod record;

/// Test-only fixtures shared across this package's test modules, the same
/// way `tests/test_harness_mirror.py::midgame` is imported by the fields,
/// mirror and session test files in Python.
#[cfg(test)]
pub(crate) mod test_support {
    use crate::advisor::state_io::Board;
    use crate::bots::weighted::eval::WeightedBot;
    use crate::state::Phase;
    use crate::{apply, game, legal};

    /// A real position, reached by cheap self-play, wrapped in an advisor
    /// [`Board`]. Mirrors `midgame`. Python seeds `WeightedBot(seed=seed)`;
    /// this port's bot has no rng field to seed (see `advisor::session`'s
    /// top doc comment on why), so `seed` here only ever reaches the deal.
    pub(crate) fn midgame(num_players: u8, seat: u8, seed: u64, stop: u16) -> Board {
        let bot = WeightedBot::default();
        let mut st = game::new_game(num_players, seed);
        let mut guard = 0;
        while !st.game_over && guard < 6000 {
            guard += 1;
            if st.round >= stop && st.decider() == seat && st.phase == Phase::Actions {
                break;
            }
            let moves = legal::legal_moves(&st);
            if moves.is_empty() {
                break;
            }
            let mv = bot.choose(&st, moves.as_slice());
            apply::apply(&mut st, mv);
        }
        Board { state: st, me: seat, unknown: Default::default(), confirmed_events: 0 }
    }
}
