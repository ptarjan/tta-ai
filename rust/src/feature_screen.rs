//! Candidate feature columns for the CHEAP SCREEN that ranks proposed
//! evaluator features before anyone spends arena compute on them.
//!
//! Nothing here is part of the bot. The champion's leaf evaluation is
//! `dot(w, phi)` over [`WeightKey::ALL`] and this module does not touch it,
//! does not add a `WeightKey`, and is never called from `evaluate`,
//! `rank_moves` or `pick`. It exists so `bin/phidump` can write, alongside
//! each decision's `phi`, a block of EXTRA quantities the evaluator has no
//! feature for -- so that "would feature X have helped?" can be answered by
//! a held-out R2 delta on an existing dump instead of by an arena run.
//!
//! ## Why the trial state is rebuilt here
//!
//! [`candidate_features`] returns `(move, phi)` and nothing else, so the
//! post-move state `phi` was priced on is not observable from outside it.
//! The extra columns have to be read off THAT state -- a hand-composition
//! column measured one move earlier than the `phi` it is being compared
//! against would be a different predictor, not a better one.
//!
//! So [`candidate_row`] rebuilds the trial exactly as `candidate_features`
//! does (clone, `determinize_current_events` at `plan_rng`, then `apply`
//! unless the move is `EndTurn`, which is scored on the unmoved root) and
//! reads the extras off it -- while still taking `phi` itself from
//! `candidate_features`, which is never re-derived. `eval.rs`'s own warning
//! about hand-rolled copies of this machinery is answered by
//! `trial_matches_candidate_features`: it asserts that
//! `linear_features` over the trial rebuilt here reproduces
//! `candidate_features`' `phi` bit for bit over real self-play positions, so
//! the two constructions cannot drift apart silently.
//!
//! ## What the columns may read
//!
//! Only what the ACTING seat can legally see at that decision: its own
//! `hand_civil`/`hand_military` (identities it holds), its own hidden-card
//! COUNTS, its own science/actions, and public board state. No rival hand
//! identity, no deck order, no label.

use crate::bots::board_yields::is_levelled_type;
use crate::bots::plan;
use crate::bots::weighted::eval::candidate_features;
use crate::bots::weighted::weights::Weights;
use crate::cards::{CardId, CardType};
use crate::moves::Move;
use crate::state::GameState;

/// Column names for [`extra_columns`], in the same order. Written to the
/// `<out>.extra_keys` sidecar so a reader that gains or loses a column
/// cannot silently misalign, the same contract `<out>.keys` has for `phi`.
pub const EXTRA_KEYS: &[&str] = &[
    // -- (A) civil hand by type family --------------------------------
    "hand_prod_count",
    "hand_urban_count",
    "hand_unit_count",
    "hand_gov_count",
    "hand_leader_count",
    "hand_action_count",
    "hand_specialtech_count",
    // -- (B) civil hand by age ----------------------------------------
    "hand_age_a_count",
    "hand_age_i_count",
    "hand_age_ii_count",
    "hand_age_iii_count",
    "hand_age_stale_count",
    "hand_age_ahead_count",
    "hand_age_mean_gap",
    // -- (C) playable now ---------------------------------------------
    "hand_affordable_count",
    "hand_playable_now_count",
    "hand_science_shortfall_total",
    "hand_science_shortfall_min",
    "hand_unaffordable_count",
    // -- (D) cost mass -------------------------------------------------
    "hand_science_cost_total",
    "hand_science_cost_max",
    "hand_resource_cost_total",
    "hand_science_cover_ratio",
    // -- (E) military hand composition ---------------------------------
    "handmil_tactic_count",
    "handmil_aggression_count",
    "handmil_war_count",
    "handmil_pact_count",
    "handmil_bonus_territory_count",
    "handmil_playable_now_count",
    "handmil_ma_cost_total",
    // -- (F) hidden-card counts ----------------------------------------
    "hand_hidden_civil",
    "hand_hidden_military",
    // -- (G) redundancy controls (already in phi; expected ~0 gain) -----
    "ctrl_hand_civil_size",
    "ctrl_hand_military_size",
];

/// How many extra columns [`extra_columns`] emits.
pub const EXTRA_DIMS: usize = EXTRA_KEYS.len();

/// Printed science price of a civil-hand card, or `None` for the types that
/// genuinely cost no science to play from hand.
///
/// Printed costs only, deliberately: `costs::tech_cost` is discount- and
/// pact-aware and would fold the rest of the board into a column that is
/// supposed to be measuring HAND COMPOSITION. Same reasoning
/// `features::hand_card_affordable` gives for its own printed-cost rule --
/// this is a screen, and a column that smuggles in the discount state would
/// score for the wrong reason.
fn science_price(card: CardId) -> Option<u8> {
    let kind = card.kind();
    if is_levelled_type(kind) {
        return Some(card.get().science_cost);
    }
    match kind {
        CardType::Government => Some(card.get().peaceful_cost),
        // Leaders and Action cards print zero for every cost field and cost
        // nothing to play out of hand.
        CardType::Leader | CardType::Action => None,
        // Cannot reach `hand_civil` (wonders go straight to
        // `PlayerState::wonder`; every military-deck type is drafted into
        // `hand_military`). Inert answer rather than a panic: this module
        // measures, it does not enforce. Named rather than wildcarded so a
        // new `CardType` is a compile error here, not a silent `None`.
        CardType::Wonder
        | CardType::Tactic
        | CardType::Aggression
        | CardType::War
        | CardType::Pact
        | CardType::Bonus
        | CardType::Territory
        | CardType::Event => None,
        // Unreachable: `is_levelled_type` above already returned for every
        // one of these.
        CardType::Farm
        | CardType::Mine
        | CardType::Lab
        | CardType::Temple
        | CardType::Library
        | CardType::Arena
        | CardType::Theater
        | CardType::Infantry
        | CardType::Cavalry
        | CardType::Artillery
        | CardType::Air
        | CardType::SpecialTech => unreachable!("is_levelled_type already handled this type above"),
    }
}

/// The extra candidate columns for one decision, read off the post-move
/// state `trial` from the point of view of seat `idx`.
///
/// Length is always [`EXTRA_DIMS`] and the order always matches
/// [`EXTRA_KEYS`].
pub fn extra_columns(trial: &GameState, idx: u8) -> Vec<f64> {
    let p = &trial.players[idx as usize];
    let science = f64::from(p.science);
    let ca = f64::from(p.civil_actions);
    let ma = f64::from(p.military_actions);
    let cur_age = trial.age_civil as u8 as i32;

    let mut prod = 0.0;
    let mut urban = 0.0;
    let mut unit = 0.0;
    let mut gov = 0.0;
    let mut leader = 0.0;
    let mut action = 0.0;
    let mut spec = 0.0;

    let mut age_a = 0.0;
    let mut age_i = 0.0;
    let mut age_ii = 0.0;
    let mut age_iii = 0.0;
    let mut stale = 0.0;
    let mut ahead = 0.0;
    let mut age_gap_sum = 0.0;

    let mut affordable = 0.0;
    let mut unaffordable = 0.0;
    let mut shortfall_total = 0.0;
    let mut shortfall_min = f64::INFINITY;
    let mut sci_total = 0.0;
    let mut sci_max = 0.0f64;
    let mut res_total = 0.0;

    for &id in p.hand_civil.as_slice() {
        let card = id.get();
        match id.kind() {
            CardType::Farm | CardType::Mine => prod += 1.0,
            CardType::Lab | CardType::Temple | CardType::Library | CardType::Arena | CardType::Theater => {
                urban += 1.0
            }
            CardType::Infantry | CardType::Cavalry | CardType::Artillery | CardType::Air => unit += 1.0,
            CardType::Government => gov += 1.0,
            CardType::Leader => leader += 1.0,
            CardType::Action => action += 1.0,
            CardType::SpecialTech => spec += 1.0,
            // Not reachable in `hand_civil`; counted nowhere rather than
            // panicking, for the reason `science_price` gives. Named, not
            // wildcarded, so the type-family partition below stays a
            // partition by construction.
            CardType::Wonder
            | CardType::Tactic
            | CardType::Aggression
            | CardType::War
            | CardType::Pact
            | CardType::Bonus
            | CardType::Territory
            | CardType::Event => {}
        }

        let age = card.age as u8 as i32;
        match age {
            0 => age_a += 1.0,
            1 => age_i += 1.0,
            2 => age_ii += 1.0,
            _ => age_iii += 1.0,
        }
        if age < cur_age {
            stale += 1.0;
        } else if age > cur_age {
            ahead += 1.0;
        }
        age_gap_sum += f64::from(age - cur_age);

        res_total += f64::from(card.resource_cost);
        match science_price(id) {
            Some(cost) => {
                let cost = f64::from(cost);
                sci_total += cost;
                sci_max = sci_max.max(cost);
                if cost <= science {
                    affordable += 1.0;
                } else {
                    unaffordable += 1.0;
                    let gap = cost - science;
                    shortfall_total += gap;
                    shortfall_min = shortfall_min.min(gap);
                }
            }
            // Free to play out of hand: affordable by construction.
            None => affordable += 1.0,
        }
    }

    let n_civil = p.hand_civil.len() as f64;
    let mean_gap = if n_civil > 0.0 { age_gap_sum / n_civil } else { 0.0 };
    let shortfall_min = if shortfall_min.is_finite() { shortfall_min } else { 0.0 };
    // "How much of what I am holding can I pay for right now", bounded in
    // [0, 1] and 1 for an empty hand -- a scale-free companion to the raw
    // shortfall, which grows with hand size.
    let cover = if sci_total > 0.0 { (science / sci_total).min(1.0) } else { 1.0 };
    // The CA gate is shared by every civil-hand play, so it multiplies
    // rather than filters: with no civil action left nothing in hand is
    // playable this turn however cheap it is.
    let playable_now = if ca >= 1.0 { affordable } else { 0.0 };

    let mut tactic = 0.0;
    let mut aggression = 0.0;
    let mut war = 0.0;
    let mut pact = 0.0;
    let mut bonus_terr = 0.0;
    let mut mil_playable = 0.0;
    let mut ma_cost_total = 0.0;
    for &id in p.hand_military.as_slice() {
        match id.kind() {
            CardType::Tactic => tactic += 1.0,
            CardType::Aggression => aggression += 1.0,
            CardType::War => war += 1.0,
            CardType::Pact => pact += 1.0,
            CardType::Bonus | CardType::Territory => bonus_terr += 1.0,
            // Not reachable in `hand_military` (the civil deck's types are
            // drafted into `hand_civil`; `Event` goes to the event pile).
            // Named for the same reason as the civil loop above.
            CardType::Event
            | CardType::Farm
            | CardType::Mine
            | CardType::Lab
            | CardType::Temple
            | CardType::Library
            | CardType::Arena
            | CardType::Theater
            | CardType::Infantry
            | CardType::Cavalry
            | CardType::Artillery
            | CardType::Air
            | CardType::Government
            | CardType::SpecialTech
            | CardType::Wonder
            | CardType::Leader
            | CardType::Action => {}
        }
        let cost = f64::from(id.get().military_action_cost);
        ma_cost_total += cost;
        if cost <= ma {
            mil_playable += 1.0;
        }
    }

    let out = vec![
        prod,
        urban,
        unit,
        gov,
        leader,
        action,
        spec,
        age_a,
        age_i,
        age_ii,
        age_iii,
        stale,
        ahead,
        mean_gap,
        affordable,
        playable_now,
        shortfall_total,
        shortfall_min,
        unaffordable,
        sci_total,
        sci_max,
        res_total,
        cover,
        tactic,
        aggression,
        war,
        pact,
        bonus_terr,
        mil_playable,
        ma_cost_total,
        f64::from(p.hidden_civil),
        f64::from(p.hidden_military),
        p.hand_size_civil() as f64,
        p.hand_size_military() as f64,
    ];
    debug_assert_eq!(out.len(), EXTRA_DIMS, "extra_columns and EXTRA_KEYS disagree on width");
    out
}

/// Rebuild the exact post-move state `candidate_features` priced `mv` on.
///
/// Kept as its own function only so the test below can assert it against
/// [`candidate_features`]; nothing else should call it.
fn trial_state(state: &GameState, mv: Move) -> GameState {
    let idx = state.decider();
    let mut trial = state.clone();
    plan::determinize_current_events(&mut trial, &mut plan::plan_rng(state, idx));
    // `Move::EndTurn` is scored on the UNMOVED root, its price carried by
    // the `EndTurnBias` indicator -- `candidate_features`' own rule.
    if !matches!(mv, Move::EndTurn) {
        crate::apply::apply(&mut trial, mv);
    }
    trial
}

/// `(phi, extras)` for one decision: the champion's own feature vector for
/// `mv`, and the extra candidate columns read off the same post-move state.
///
/// `None` when `mv` is filtered out by `candidate_features` (a resignation),
/// which is correct: a resignation is not a position anyone evaluates.
pub fn candidate_row(state: &GameState, mv: Move, freeze: &Weights) -> Option<(Vec<f64>, Vec<f64>)> {
    let phi = candidate_features(state, &[mv], false, freeze).into_iter().next()?.1;
    let idx = state.decider();
    let extras = extra_columns(&trial_state(state, mv), idx);
    Some((phi, extras))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bots::greedy::{build_bots, BotKind, Search, Seat};
    use crate::bots::weighted::eval::linear_features;
    use crate::bots::weighted::rivals;
    use crate::game::{self, MOVE_CAP};

    #[test]
    fn extra_keys_and_columns_agree_on_width() {
        let state = game::new_game(2, 7);
        assert_eq!(extra_columns(&state, 0).len(), EXTRA_KEYS.len());
    }

    /// THE GUARD on this module's one hand-rolled copy of `eval.rs`'s
    /// root/trial machinery: the state [`trial_state`] rebuilds must be the
    /// state [`candidate_features`] actually scored, or the extra columns
    /// describe a different position than the `phi` they are dumped next to.
    ///
    /// Asserted the only way that can be checked from outside `eval.rs`:
    /// `linear_features` over the rebuilt trial must reproduce
    /// `candidate_features`' vector exactly, on real self-play positions
    /// rather than a synthetic state.
    #[test]
    fn trial_matches_candidate_features() {
        let w = crate::bots::weighted::weights::Weights::default();
        let seats = vec![Seat { kind: BotKind::Weighted, weights: w, search: Search::None }; 2];
        let mut checked = 0usize;
        for seed in 1..=3u64 {
            let mut bots = build_bots(&seats, seed as i64);
            let mut state = game::new_game(2, seed);
            let _ = game::play_game(&mut state, MOVE_CAP, |s, _legal| {
                let mv = bots[s.decider() as usize].pick(s);
                if checked < 400 {
                    if let Some((phi, extras)) = candidate_row(s, mv, &w) {
                        let idx = s.decider();
                        let ctx = rivals::rival_context(s, idx, None, None);
                        let mut f = linear_features(&trial_state(s, mv), idx, Some(&ctx), &w);
                        if matches!(mv, Move::EndTurn) {
                            f[crate::bots::weighted::weights::WeightKey::EndTurnBias as usize] += 1.0;
                        }
                        assert_eq!(
                            f, phi,
                            "trial_state drifted from candidate_features' trial (seed {seed}, move {mv:?})"
                        );
                        assert_eq!(extras.len(), EXTRA_DIMS);
                        assert!(
                            extras.iter().all(|v| v.is_finite()),
                            "non-finite extra column (seed {seed}, move {mv:?}): {extras:?}"
                        );
                        checked += 1;
                    }
                }
                mv
            });
        }
        assert!(checked > 100, "test played too few decisions to be a guard: {checked}");
    }

    /// Counts must add up: the per-family and per-age decompositions each
    /// partition the same civil hand, so both sum to its size.
    #[test]
    fn decompositions_partition_the_hand() {
        let w = crate::bots::weighted::weights::Weights::default();
        let seats = vec![Seat { kind: BotKind::Weighted, weights: w, search: Search::None }; 2];
        let mut bots = build_bots(&seats, 11);
        let mut state = game::new_game(2, 11);
        let mut seen_nonempty = 0usize;
        let _ = game::play_game(&mut state, MOVE_CAP, |s, _legal| {
            let mv = bots[s.decider() as usize].pick(s);
            let idx = s.decider();
            let trial = trial_state(s, mv);
            let c = extra_columns(&trial, idx);
            let n = trial.players[idx as usize].hand_civil.len() as f64;
            let by_type: f64 = c[0..7].iter().sum();
            let by_age: f64 = c[7..11].iter().sum();
            let by_afford = c[14] + c[18];
            assert_eq!(by_type, n, "type families do not partition the civil hand");
            assert_eq!(by_age, n, "age buckets do not partition the civil hand");
            assert_eq!(by_afford, n, "affordable + unaffordable != civil hand size");
            if n > 0.0 {
                seen_nonempty += 1;
            }
            mv
        });
        assert!(seen_nonempty > 50, "never saw a non-empty civil hand: {seen_nonempty}");
    }
}
