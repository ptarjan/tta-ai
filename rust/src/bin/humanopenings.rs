//! `humanopenings` -- the human-corpus twin of `behavcensus.rs`'s opening
//! section, but sourced from REPLAYED human games (`replay_common::
//! replay_game`, `record_decisions: true`) instead of self-play, and scoped
//! to the opening only: each player's own moves from the start of the game
//! through the end of round 3 (Age A plus the first couple of Age I rounds
//! for most games), described the way a human would describe them --
//! per the "Canonical openings" measurement task.
//!
//! Covers every player count in the corpus (2p/3p/4p, 692/133/186 of 1011
//! games). Results are SEGMENTED by player count throughout -- an n-player
//! game is a structurally different game (card-row width, take-cost
//! economics, seat count all vary with `players`), so pooling counts into
//! one undifferentiated number is invalid; see the per-count loop and the
//! per-count cost-distribution summary below.
//!
//! ```text
//! cargo run --profile difftest --bin humanopenings -- \
//!     ../sources/bgo/index.tsv /tmp/bgo-journals/journals > openings.tsv
//! ```
//!
//! One TSV line per player-game on stdout (N lines per N-player game),
//! columns:
//! `game_id  seat  first_take_name  first_take_cost  first_build_kind
//!  first_build_name  leader_by_r3  pop_by_r3  ca_unused_by_r3  outcome`
//!
//! `outcome` is `win`/`loss`/`tie`/`unknown`, determined from the SAME
//! text-only "WINNER IS ... AS <COLOR> (<N> PTS); 2nd is ... as <Color>
//! (<n> pts)" parse `bin/humanwinners.rs` already uses for its own
//! winner/loser split (`corpus::parse_winner_line`, moved there from that
//! binary so both can call it) -- reused rather than gating outcome on full
//! engine-replay completion (`GameResult::engine_scores`, `Some` only for
//! the minority of games whose reconstruction reaches the literal end, see
//! `docs/CHAMPION_VS_HUMANS.md`'s "Completion" note); text coverage is far
//! higher. Seat->colour uses `corpus::Color::from_seat`, BGO's fixed
//! empirically-confirmed seating convention (Orange seat 0, Purple seat 1
//! for every 2p game) -- the same convention `replay_common`'s own
//! `target_actor_color` and `humanwinners::color_for_seat` already encode.

use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::process::ExitCode;

use tta::corpus::{self, parse_winner_line, Color};
use tta::replay_common::{build_card_index, replay_game};
use tta::{CardType, Move};

/// First-take civil-action-cost tally for one player count, accumulated
/// across every player-game at that count.
#[derive(Default)]
struct CostStats {
    /// cost -> number of player-games whose first take had that cost.
    hist: BTreeMap<i32, u64>,
    sum: i64,
    n: u64,
}

impl CostStats {
    fn record(&mut self, cost: i32) {
        *self.hist.entry(cost).or_insert(0) += 1;
        self.sum += i64::from(cost);
        self.n += 1;
    }

    fn report(&self, players: u8) {
        if self.n == 0 {
            eprintln!("first-take cost, {players}p: n=0");
            return;
        }
        eprintln!("first-take cost, {players}p (n={}):", self.n);
        for (cost, count) in &self.hist {
            let pct = 100.0 * (*count as f64) / (self.n as f64);
            eprintln!("  cost {cost} = {count}/{} = {pct:.1}%", self.n);
        }
        let mean = (self.sum as f64) / (self.n as f64);
        eprintln!("  mean = {mean:.3}");
    }
}

fn build_kind(kind: CardType) -> &'static str {
    match kind {
        CardType::Farm => "Farm",
        CardType::Mine => "Mine",
        CardType::Temple => "Temple",
        CardType::Lab => "Lab",
        CardType::Library => "Library",
        CardType::Arena => "Arena",
        CardType::Theater => "Theater",
        CardType::Infantry | CardType::Cavalry | CardType::Artillery | CardType::Air => "Military",
        CardType::Wonder => "WonderStage",
        CardType::Government | CardType::SpecialTech | CardType::Leader | CardType::Action | CardType::Tactic | CardType::Aggression | CardType::War | CardType::Pact | CardType::Bonus | CardType::Territory | CardType::Event => "Other",
    }
}

/// This game's per-colour outcome, from the journal's own "WINNER IS" line
/// (`corpus::parse_winner_line`): `("win"|"loss"|"tie", Color)` pairs for
/// every colour a rank clause was found for. Rank 1 alone is "win" (base
/// game 2p has no ties at rank 1 in the corpus scanned for
/// `docs/HUMAN_WINNERS.md`-adjacent work); every other parsed rank is
/// "loss". Empty if no "WINNER IS" clause parsed at all.
fn parse_outcomes(text: &str) -> Vec<(Color, &'static str)> {
    let Some(pos) = text.find("WINNER IS") else { return Vec::new() };
    parse_winner_line(&text[pos..])
        .into_iter()
        .map(|(rank, color, _score)| (color, if rank == 1 { "win" } else { "loss" }))
        .collect()
}

fn run(index_path: &str, journals_dir: &str) -> Result<(), String> {
    let card_index = build_card_index();
    let games = corpus::parse_index(index_path)?;

    // Cost distributions are segmented by player count (`meta.players`) and
    // reported separately per count -- never pooled -- because take-cost
    // economics differ by card-row width, which is itself a function of
    // player count; a pooled number would silently mix populations.
    let mut cost_stats: BTreeMap<u8, CostStats> = BTreeMap::new();

    for meta in games.iter() {
        let n = meta.players as usize;
        let path = format!("{journals_dir}/{}.tsv", meta.id);
        let text = match fs::read_to_string(&path) {
            Ok(t) => t,
            Err(_) => continue,
        };
        let outcomes_by_color = parse_outcomes(&text);
        let result = replay_game(meta, &text, &card_index, true);

        // Per-seat opening trackers, round <= 3 only, one slot per seat.
        let mut first_take: Vec<Option<(&'static str, i32)>> = vec![None; n];
        let mut first_build: Vec<Option<(&'static str, &'static str)>> = vec![None; n];
        let mut took_leader: Vec<bool> = vec![false; n];
        let mut increased_pop: Vec<bool> = vec![false; n];
        let mut ca_unused: Vec<i32> = vec![0; n];

        for d in &result.decisions {
            if d.state.round > 3 {
                continue;
            }
            let seat = d.state.current as usize;
            if seat >= n {
                continue;
            }
            match d.human_move {
                Move::Take { slot } => {
                    if first_take[seat].is_none() {
                        let card = d.state.card_row[slot as usize];
                        if !card.is_none() {
                            let cost = tta::costs::take_cost(&d.state, &d.state.players[seat], slot as usize);
                            first_take[seat] = Some((card.get().name, cost));
                        }
                    }
                }
                Move::Build { card } | Move::Develop { card, .. } => {
                    if first_build[seat].is_none() {
                        first_build[seat] = Some((build_kind(card.get().kind), card.get().name));
                    }
                }
                Move::Upgrade { to, .. } => {
                    if first_build[seat].is_none() {
                        first_build[seat] = Some((build_kind(to.get().kind), to.get().name));
                    }
                }
                Move::PlayLeader { .. } => {
                    took_leader[seat] = true;
                }
                Move::Pop { .. } | Move::PopFree => {
                    increased_pop[seat] = true;
                }
                Move::EndTurn => {
                    let ca = d.state.players[seat].civil_actions;
                    if ca > 0 {
                        ca_unused[seat] += ca as i32;
                    }
                }
                Move::WonderStep { .. } | Move::Revolution { .. } | Move::PlayAction { .. } | Move::Destroy { .. } | Move::PlayTactic { .. } | Move::CopyTactic { .. } | Move::Aggression { .. } | Move::War { .. } | Move::OfferPact { .. } | Move::CancelPact { .. } | Move::PrepareEvent { .. } | Move::RemoveLeaderYellow | Move::ColumbusColonize { .. } | Move::Barbarossa { .. } | Move::BachTheater { .. } | Move::TradeFoodAsResource | Move::TradeResourceAsFood | Move::Bid { .. } | Move::BidPass | Move::Defend { .. } | Move::DefendDone | Move::SendUnit { .. } | Move::SendBonus { .. } | Move::SendDiscard { .. } | Move::SendDone | Move::Choose { .. } | Move::Churchill { .. } | Move::PolPass | Move::Resign => {}
            }
        }

        for seat in 0..n {
            let (take_name, take_cost) = first_take[seat].unwrap_or(("none", -1));
            let (build_kind_s, build_name) = first_build[seat].unwrap_or(("none", "none"));
            let outcome = Color::from_seat(seat as u8)
                .and_then(|c| outcomes_by_color.iter().find(|(oc, _)| *oc == c))
                .map(|(_, o)| *o)
                .unwrap_or("unknown");
            println!(
                "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
                meta.id,
                seat,
                take_name,
                take_cost,
                build_kind_s,
                build_name,
                took_leader[seat],
                increased_pop[seat],
                ca_unused[seat],
                outcome,
            );
            if first_take[seat].is_some() {
                cost_stats.entry(meta.players).or_default().record(take_cost);
            }
        }
    }

    for (players, stats) in &cost_stats {
        stats.report(*players);
    }
    Ok(())
}

fn main() -> ExitCode {
    let argv: Vec<String> = env::args().skip(1).collect();
    if argv.len() < 2 {
        eprintln!("usage: humanopenings <index.tsv> <journals_dir>");
        return ExitCode::FAILURE;
    }
    match run(&argv[0], &argv[1]) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A real 2p BGO end-of-game clause: rank 1 must map to "win", rank 2 to
    /// "loss" -- this is the whole contract `run`'s outcome lookup relies
    /// on, so a regression here (e.g. an off-by-one on `rank`) would flip
    /// every win-rate number in `OPENINGS.txt` silently.
    #[test]
    fn parse_outcomes_maps_rank_1_to_win_and_rank_2_to_loss() {
        let text = "...\tWINNER IS PLAYER AS ORANGE (195 PTS); 2nd is PLAYER as Purple (160 pts)\n";
        let outcomes = parse_outcomes(text);
        assert_eq!(outcomes, vec![(Color::Orange, "win"), (Color::Purple, "loss")]);
    }

    /// No "WINNER IS" clause anywhere in the text (e.g. a game journal that
    /// stops mid-play with no recorded ending) must yield an empty outcome
    /// list, not a panic -- `run`'s per-seat lookup then correctly falls
    /// back to "unknown" via its own `unwrap_or`.
    #[test]
    fn parse_outcomes_is_empty_when_no_winner_clause_is_present() {
        assert_eq!(parse_outcomes("no ending here\n"), Vec::new());
    }
}
