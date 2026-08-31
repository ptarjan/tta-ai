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
//! ## The three families
//!
//! * HAND COMPOSITION -- which cards the actor is holding, cut by type, age,
//!   affordability and cost mass. `phi` prices the hand's SIZE and two
//!   aggregate values and has no feature for its contents.
//! * CARD GRANULARITY -- which card, not which type. `phi` reduces every
//!   card in play to a per-type LEVEL scalar (`BestFarm` .. `BestUnit`,
//!   `TechLevels`, `GovLevel`) or a per-type COUNT (`NumTechs`,
//!   `SpecialTechs`, `Wonders`, `Leader` as 0/1), so Republic and
//!   Constitutional Monarchy are the same number, all twelve special techs
//!   are interchangeable and all twenty-four leaders are a single bit.
//! * OPPONENT-RELATIVE / GAP-CONDITIONAL -- hinges, sign-and-magnitude
//!   buckets and round crosses on the gaps to the best rival. `phi`'s own
//!   relative keys are plain signed differences (`StrengthRel`) or hinges
//!   with only one half present (`RivalCultureDeficit` exists, the matching
//!   lead does not), and no gap key is scaled by the horizon.
//!
//! Each family carries at least two REDUNDANCY CONTROLS: a column that is an
//! exact function of live `phi` columns (`ctrl_a_num_techs`,
//! `ctrl_a_wonders_count`, `ctrl_b_wonder_gap`, `ctrl_b_hand_civil_gap` --
//! asserted to be exactly that by `trial_matches_candidate_features`) and,
//! screen-side, a real candidate column with its row order permuted. Both
//! must land at ~0 or the run is not trustworthy.
//!
//! ## What the columns may read
//!
//! Only what the ACTING seat can legally see at that decision: its own
//! `hand_civil`/`hand_military` (identities it holds), its own hidden-card
//! COUNTS, its own science/actions, and public board state. No rival hand
//! identity, no deck order, no label.
//!
//! The opponent-relative family widens that to public RIVAL state, and only
//! to public rival state: rating-track culture and science, the face-up
//! tableau, government and completed wonders (and the rates
//! `effects::compute` derives from them), civil hand SIZE, and military hand
//! SIZE. A rival's military hand CONTENTS are face down and are never read
//! -- `rival_military_hand_contents_are_not_read` asserts that replacing
//! them, at unchanged hand size, moves no column.

use crate::bots::board_yields::is_levelled_type;
use crate::bots::plan;
use crate::bots::weighted::eval::candidate_features;
use crate::bots::weighted::horizon;
use crate::bots::weighted::weights::Weights;
use crate::cards::{CardId, CardType};
use crate::effects;
use crate::moves::Move;
use crate::state::{GameState, PlayerState};

/// The eight governments, in table order. One-hot over `p.government`.
///
/// `phi` carries a government as `WeightKey::GovLevel`, which is the card's
/// AGE: Monarchy and Theocracy are both 1, and Communism, Fundamentalism and
/// Democracy are all 3. Their printed civil/military actions and urban limit
/// do reach `phi` through `effects::Stats`, so these indicators are expected
/// to be heavily spanned -- that is the measurement.
const GOV_NAMES: [&str; 8] = [
    "Despotism",
    "Monarchy",
    "Theocracy",
    "Constitutional Monarchy",
    "Republic",
    "Communism",
    "Democracy",
    "Fundamentalism",
];

/// The twelve special technologies. `phi` counts them
/// (`WeightKey::SpecialTechs`) and adds each one's age to
/// `WeightKey::TechLevels`; nothing distinguishes Masonry from Warfare.
const SPEC_NAMES: [&str; 12] = [
    "Masonry",
    "Code of Laws",
    "Warfare",
    "Cartography",
    "Architecture",
    "Justice System",
    "Strategy",
    "Navigation",
    "Engineering",
    "Civil Service",
    "Military Theory",
    "Satellites",
];

/// The sixteen wonders. `phi` carries completed wonders as a COUNT
/// (`WeightKey::Wonders`) and the one in progress as a stage arithmetic
/// (`WonderProgress`/`WonderRemaining`/`WonderStagesLeft`/...); identity
/// reaches the score only through the freeze-priced `WonderPotential` and
/// `WonderPromise` scalars.
const WONDER_NAMES: [&str; 16] = [
    "Pyramids",
    "Hanging Gardens",
    "Colossus",
    "Library of Alexandria",
    "Great Wall",
    "St. Peter's Basilica",
    "Universitas Carolina",
    "Taj Mahal",
    "Transcontinental Railroad",
    "Eiffel Tower",
    "Kremlin",
    "Ocean Liners",
    "First Space Flight",
    "Fast Food Chains",
    "Internet",
    "Hollywood",
];

/// The twenty-four leaders. `phi` carries a leader as
/// `WeightKey::Leader`, a 0/1 indicator, plus the Gandhi-only
/// `AttackCostDoubled` flag and the `LeaderReplacement` indicator.
const LEADER_NAMES: [&str; 24] = [
    "Aristotle",
    "Alexander the Great",
    "Julius Caesar",
    "Hammurabi",
    "Homer",
    "Moses",
    "Leonardo da Vinci",
    "Christopher Columbus",
    "Frederick Barbarossa",
    "Genghis Khan",
    "Joan of Arc",
    "Michelangelo",
    "James Cook",
    "Isaac Newton",
    "J. S. Bach",
    "Maximilien Robespierre",
    "Napoleon Bonaparte",
    "William Shakespeare",
    "Albert Einstein",
    "Bill Gates",
    "Charlie Chaplin",
    "Mahatma Gandhi",
    "Sid Meier",
    "Winston Churchill",
];

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
    // ================================================================
    // FAMILY A -- CARD GRANULARITY.
    // Everything in `phi` that describes a card in play is a per-TYPE
    // level scalar (`BestFarm` .. `BestUnit`, `TechLevels`, `GovLevel`)
    // or a per-type count (`NumTechs`, `SpecialTechs`, `Wonders`,
    // `Leader`). These columns split what those lump.
    // ================================================================
    // -- (A1) government identity, one-hot over p.government -----------
    "gran_gov_despotism",
    "gran_gov_monarchy",
    "gran_gov_theocracy",
    "gran_gov_const_monarchy",
    "gran_gov_republic",
    "gran_gov_communism",
    "gran_gov_democracy",
    "gran_gov_fundamentalism",
    // -- (A2) government printed prices (within-age discriminators) -----
    "gran_gov_peaceful_cost",
    "gran_gov_revolution_cost",
    // -- (A3) special-tech identity, one-hot over the tableau ----------
    "gran_spec_masonry",
    "gran_spec_code_of_laws",
    "gran_spec_warfare",
    "gran_spec_cartography",
    "gran_spec_architecture",
    "gran_spec_justice_system",
    "gran_spec_strategy",
    "gran_spec_navigation",
    "gran_spec_engineering",
    "gran_spec_civil_service",
    "gran_spec_military_theory",
    "gran_spec_satellites",
    // -- (A4) unit level BY TYPE (phi has one max over all four) -------
    "gran_best_infantry",
    "gran_best_cavalry",
    "gran_best_artillery",
    "gran_best_air",
    // -- (A5) completed-wonder identity --------------------------------
    "gran_wcomp_pyramids",
    "gran_wcomp_hanging_gardens",
    "gran_wcomp_colossus",
    "gran_wcomp_library_of_alexandria",
    "gran_wcomp_great_wall",
    "gran_wcomp_st_peters_basilica",
    "gran_wcomp_universitas_carolina",
    "gran_wcomp_taj_mahal",
    "gran_wcomp_transcontinental_railroad",
    "gran_wcomp_eiffel_tower",
    "gran_wcomp_kremlin",
    "gran_wcomp_ocean_liners",
    "gran_wcomp_first_space_flight",
    "gran_wcomp_fast_food_chains",
    "gran_wcomp_internet",
    "gran_wcomp_hollywood",
    // -- (A6) in-progress-wonder identity ------------------------------
    "gran_wbuild_pyramids",
    "gran_wbuild_hanging_gardens",
    "gran_wbuild_colossus",
    "gran_wbuild_library_of_alexandria",
    "gran_wbuild_great_wall",
    "gran_wbuild_st_peters_basilica",
    "gran_wbuild_universitas_carolina",
    "gran_wbuild_taj_mahal",
    "gran_wbuild_transcontinental_railroad",
    "gran_wbuild_eiffel_tower",
    "gran_wbuild_kremlin",
    "gran_wbuild_ocean_liners",
    "gran_wbuild_first_space_flight",
    "gran_wbuild_fast_food_chains",
    "gran_wbuild_internet",
    "gran_wbuild_hollywood",
    // -- (A7) leader identity ------------------------------------------
    "gran_leader_aristotle",
    "gran_leader_alexander_the_great",
    "gran_leader_julius_caesar",
    "gran_leader_hammurabi",
    "gran_leader_homer",
    "gran_leader_moses",
    "gran_leader_leonardo_da_vinci",
    "gran_leader_christopher_columbus",
    "gran_leader_frederick_barbarossa",
    "gran_leader_genghis_khan",
    "gran_leader_joan_of_arc",
    "gran_leader_michelangelo",
    "gran_leader_james_cook",
    "gran_leader_isaac_newton",
    "gran_leader_js_bach",
    "gran_leader_maximilien_robespierre",
    "gran_leader_napoleon_bonaparte",
    "gran_leader_william_shakespeare",
    "gran_leader_albert_einstein",
    "gran_leader_bill_gates",
    "gran_leader_charlie_chaplin",
    "gran_leader_mahatma_gandhi",
    "gran_leader_sid_meier",
    "gran_leader_winston_churchill",
    // -- (A8) structure, obsolescence, cost-to-benefit ------------------
    "gran_leader_age",
    "gran_wcomp_stage_total",
    "gran_wbuild_stage_total",
    "gran_wbuild_max_stage",
    "gran_wbuild_num_stages",
    "gran_board_obsolete_levels",
    "gran_board_obsolete_workers",
    "gran_board_age_gap_mean",
    "gran_board_unit_types",
    "gran_board_level_spread",
    "gran_hand_upgrade_best",
    "gran_hand_obsolete_count",
    "gran_hand_costben_max",
    "gran_hand_costben_mean",
    "gran_hand_distinct_types",
    // -- (A9) redundancy controls: EXACT functions of phi columns ------
    "ctrl_a_num_techs",
    "ctrl_a_wonders_count",
    // ================================================================
    // FAMILY B -- OPPONENT-RELATIVE / GAP-CONDITIONAL.
    // `phi`'s relative keys are either plain signed differences
    // (`StrengthRel`, `AttackTargetWeakness`) or one-sided hinges whose
    // OTHER half is missing (`RivalCultureDeficit` and
    // `RivalScienceDeficit` exist; the leads do not). These columns
    // supply the missing halves, the sign/magnitude buckets, and the
    // round crosses -- none of which a linear evaluator can form.
    // ================================================================
    // -- (B1) hinged halves ---------------------------------------------
    "rel_culrate_lead",
    "rel_culrate_deficit",
    "rel_scirate_lead",
    "rel_scirate_deficit",
    "rel_culture_lead",
    "rel_culture_deficit",
    "rel_strength_lead",
    "rel_strength_lead_over_cap",
    "rel_strength_deficit",
    "rel_tech_lead",
    "rel_tech_deficit",
    "rel_scistock_deficit",
    // -- (B2) gap bucketed by sign and magnitude ------------------------
    "rel_bkt_culrate_m2",
    "rel_bkt_culrate_m1",
    "rel_bkt_culrate_0",
    "rel_bkt_culrate_p1",
    "rel_bkt_culrate_p2",
    "rel_bkt_scirate_m2",
    "rel_bkt_scirate_m1",
    "rel_bkt_scirate_0",
    "rel_bkt_scirate_p1",
    "rel_bkt_scirate_p2",
    "rel_bkt_strength_m2",
    "rel_bkt_strength_m1",
    "rel_bkt_strength_0",
    "rel_bkt_strength_p1",
    "rel_bkt_strength_p2",
    // -- (B3) gap crossed with how late the game is ---------------------
    "rel_x_culrate_gap_late",
    "rel_x_culrate_lead_late",
    "rel_x_culrate_def_late",
    "rel_x_scirate_gap_late",
    "rel_x_scirate_lead_late",
    "rel_x_scirate_def_late",
    "rel_x_culrate_gap_round",
    "rel_x_strength_gap_late",
    "rel_x_culture_gap_late",
    "rel_x_tech_gap_late",
    // -- (B4) projection to game end, and scale-free shares -------------
    "rel_proj_final_culture_gap",
    "rel_proj_final_lead",
    "rel_proj_final_deficit",
    "rel_trail_culrate",
    "rel_trail_scirate",
    "rel_share_culrate",
    // -- (B5) gap-CONDITIONAL card value --------------------------------
    "rel_cond_handvalue_x_culdef",
    "rel_cond_handvalue_x_cullead",
    "rel_cond_scishort_x_scitrail",
    // -- (B6) action and hand-size tempo, hinged ------------------------
    "rel_ca_lead",
    "rel_ca_deficit",
    "rel_milhand_lead",
    "rel_milhand_deficit",
    // -- (B7) redundancy controls: EXACT functions of phi columns -------
    "ctrl_b_wonder_gap",
    "ctrl_b_hand_civil_gap",
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

/// Position of `id`'s printed name in `names`, or `None`.
///
/// A linear scan over a list of at most 24 names, the same construction
/// `CardId::by_name` uses and for the same reason: this is measurement code,
/// not the engine, and `card_names_all_resolve` pins every entry of every
/// list to a real card of the expected type, so a typo is a failing test
/// rather than a column that is silently always zero.
fn name_slot(names: &[&str], id: CardId) -> Option<usize> {
    if id.is_none() {
        return None;
    }
    let n = id.name();
    names.iter().position(|w| *w == n)
}

/// The eight upgrade LANES a levelled card can sit in: one per building
/// type, and one shared lane for the four unit types.
///
/// The unit lane mirrors `cards::redundancy_lane`, which folds Infantry,
/// Cavalry, Artillery and Air onto Infantry: a player replaces one army
/// technology with a better one regardless of which of the four it is.
const LANES: usize = 8;

fn lane_index(kind: CardType) -> Option<usize> {
    match kind {
        CardType::Farm => Some(0),
        CardType::Mine => Some(1),
        CardType::Lab => Some(2),
        CardType::Temple => Some(3),
        CardType::Library => Some(4),
        CardType::Arena => Some(5),
        CardType::Theater => Some(6),
        CardType::Infantry | CardType::Cavalry | CardType::Artillery | CardType::Air => Some(7),
        // No upgrade lane: a government is replaced, not upgraded through a
        // level ladder shared with siblings; special techs are one-per-lane
        // by age but are counted by (A3) instead; the rest never sit in
        // `Tableau`. Named rather than wildcarded so a new `CardType` is a
        // compile error here.
        CardType::Government
        | CardType::SpecialTech
        | CardType::Wonder
        | CardType::Leader
        | CardType::Action
        | CardType::Tactic
        | CardType::Aggression
        | CardType::War
        | CardType::Pact
        | CardType::Bonus
        | CardType::Territory
        | CardType::Event => None,
    }
}

/// Printed per-worker output of a card, summed over the six production
/// fields. The crude "what does one worker on this get me" scalar that makes
/// two cards of the same type and level comparable.
fn printed_yield(id: CardId) -> f64 {
    let p = id.get().production;
    f64::from(p.food + p.resources + p.science + p.culture + p.happy + p.strength)
}

/// `WeightKey::TechLevels` for an arbitrary player: the sum of card ages over
/// the levelled tableau plus the government's age.
///
/// Recomputed here rather than read off `phi` because it is needed for the
/// RIVAL, whose tableau `phi` carries no level total for at all. Mirrors
/// `features::sweep_tableau`'s `tech_levels` accumulation plus
/// `features.rs`'s later `+= p.government.level()`.
fn tech_levels(p: &PlayerState) -> i32 {
    let mut t = 0i32;
    for (id, _) in p.techs.iter() {
        if is_levelled_type(id.kind()) {
            t += i32::from(id.level());
        }
    }
    t + i32::from(p.government.level())
}

/// Which side of a gap, and how far: a five-way partition of a signed
/// integer gap at `-far`, `-1`, `0`, `+1`, `+far`.
///
/// The point of the family: a linear evaluator can price `gap` but cannot
/// price "behind at all" differently from "behind by a lot".
fn sign_buckets(gap: i32, far: i32) -> [f64; 5] {
    let mut b = [0.0; 5];
    let i = if gap <= -far {
        0
    } else if gap < 0 {
        1
    } else if gap == 0 {
        2
    } else if gap < far {
        3
    } else {
        4
    };
    b[i] = 1.0;
    b
}

/// `max(0, (best - mine) / best)` -- how far behind the leader I am, as a
/// fraction of the leader's own rate.
///
/// The same formula as `rivals::trailing_fraction`, which exists only on the
/// WEIGHT side (it prices cards through `feature_marginal`) and therefore has
/// no coordinate in `phi` at all.
fn trailing_fraction(mine: i32, best: i32) -> f64 {
    if best <= 0 || mine >= best {
        return 0.0;
    }
    f64::from(best - mine) / f64::from(best)
}

/// What the acting seat may legally read about its opponents, in one struct.
///
/// LEGALITY, column by column. Rating-track positions (`culture`, `science`)
/// and every card in a rival's tableau, government and completed-wonder pile
/// are FACE UP; `effects::compute` over a rival reads exactly those, so the
/// rates derived from them are public arithmetic a human at the table does
/// the same way. Civil hand SIZE is public (§2.5/§2.6 and
/// `features.rs`'s own `RivalHandCivil`). Military hand SIZE is public and
/// its CONTENTS are not -- so `mil_hand` below is `hand_size_military()`, a
/// COUNT, and nothing in this file ever reads a rival's `hand_military`
/// slice. That is the same boundary `horizon::combat_unreachable` draws.
struct RivalMax {
    /// Any live rival at all. Every field below is zero when this is false.
    any: bool,
    culture_rate: i32,
    science_rate: i32,
    strength: i32,
    culture: i32,
    science: i32,
    tech_levels: i32,
    civil_actions: i32,
    hand_civil: i32,
    mil_hand: i32,
    wonders: i32,
}

fn rival_max(trial: &GameState, idx: u8) -> RivalMax {
    let mut r = RivalMax {
        any: false,
        culture_rate: 0,
        science_rate: 0,
        strength: 0,
        culture: 0,
        science: 0,
        tech_levels: 0,
        civil_actions: 0,
        hand_civil: 0,
        mil_hand: 0,
        wonders: 0,
    };
    for q in trial.players[..trial.num_players as usize].iter() {
        if q.idx == idx || q.resigned {
            continue;
        }
        let s = effects::compute(trial, q);
        r.any = true;
        r.culture_rate = r.culture_rate.max(s.culture);
        r.science_rate = r.science_rate.max(s.science);
        r.strength = r.strength.max(s.strength);
        r.culture = r.culture.max(i32::from(q.culture));
        r.science = r.science.max(i32::from(q.science));
        r.tech_levels = r.tech_levels.max(tech_levels(q));
        r.civil_actions = r.civil_actions.max(s.civil_actions);
        r.hand_civil = r.hand_civil.max(q.hand_size_civil() as i32);
        r.mil_hand = r.mil_hand.max(q.hand_size_military() as i32);
        r.wonders = r.wonders.max(q.completed_wonders.len() as i32);
    }
    r
}

/// `WeightKey::StrengthLead`'s cap (`features::STRENGTH_LEAD_CAP`).
/// Duplicated as a literal rather than imported because the column that uses
/// it is measuring what the cap DISCARDS; if the evaluator's cap moves, this
/// column should not silently move with it.
const STRENGTH_LEAD_CAP_AT_FREEZE: f64 = 6.0;

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

    let mut out = vec![
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

    // ================================================================
    // FAMILY A -- CARD GRANULARITY
    // ================================================================

    // (A1) government one-hot, (A2) its printed prices.
    let gov_slot = name_slot(&GOV_NAMES, p.government);
    for i in 0..GOV_NAMES.len() {
        out.push(if gov_slot == Some(i) { 1.0 } else { 0.0 });
    }
    let gov = p.government;
    if gov.is_none() {
        out.push(0.0);
        out.push(0.0);
    } else {
        out.push(f64::from(gov.get().peaceful_cost));
        out.push(f64::from(gov.get().revolution_cost));
    }

    // One pass over the tableau feeds (A3), (A4) and most of (A8).
    let mut spec = [0.0f64; SPEC_NAMES.len()];
    let mut best_by_unit = [0u8; 4];
    let mut lane_best_level = [0i8; LANES];
    let mut lane_best_yield = [0.0f64; LANES];
    let mut lane_seen = [false; LANES];
    let mut unit_types = 0u32;
    let mut age_gap_total = 0.0;
    let mut n_techs = 0.0;
    for (id, _) in p.techs.iter() {
        let kind = id.kind();
        let lv = id.level();
        n_techs += 1.0;
        age_gap_total += f64::from(cur_age - i32::from(lv));
        if let Some(i) = name_slot(&SPEC_NAMES, id) {
            spec[i] = 1.0;
        }
        match kind {
            CardType::Infantry => best_by_unit[0] = best_by_unit[0].max(lv),
            CardType::Cavalry => best_by_unit[1] = best_by_unit[1].max(lv),
            CardType::Artillery => best_by_unit[2] = best_by_unit[2].max(lv),
            CardType::Air => best_by_unit[3] = best_by_unit[3].max(lv),
            CardType::Farm
            | CardType::Mine
            | CardType::Lab
            | CardType::Temple
            | CardType::Library
            | CardType::Arena
            | CardType::Theater
            | CardType::Government
            | CardType::SpecialTech
            | CardType::Wonder
            | CardType::Leader
            | CardType::Action
            | CardType::Tactic
            | CardType::Aggression
            | CardType::War
            | CardType::Pact
            | CardType::Bonus
            | CardType::Territory
            | CardType::Event => {}
        }
        if kind.is_unit() {
            unit_types |= 1 << (kind as u32);
        }
        if let Some(l) = lane_index(kind) {
            if !lane_seen[l] || i8::try_from(lv).unwrap_or(0) > lane_best_level[l] {
                lane_best_level[l] = i8::try_from(lv).unwrap_or(0);
                lane_best_yield[l] = printed_yield(id);
            }
            lane_seen[l] = true;
        }
    }
    // Second pass: obsolescence is only defined once every lane best is known.
    let mut obsolete_levels = 0.0;
    let mut obsolete_workers = 0.0;
    for (id, slot) in p.techs.iter() {
        let Some(l) = lane_index(id.kind()) else { continue };
        let behind = i32::from(lane_best_level[l]) - i32::from(id.level());
        if behind > 0 {
            obsolete_levels += f64::from(behind);
            obsolete_workers += f64::from(slot.workers);
        }
    }
    out.extend_from_slice(&spec);
    for b in best_by_unit {
        out.push(f64::from(b));
    }

    // (A5) completed wonders, (A6) the one in progress.
    let mut wcomp = [0.0f64; WONDER_NAMES.len()];
    let mut wcomp_stage_total = 0.0;
    for &id in p.completed_wonders.as_slice() {
        if let Some(i) = name_slot(&WONDER_NAMES, id) {
            wcomp[i] = 1.0;
        }
        wcomp_stage_total += id.get().stages.iter().map(|&s| f64::from(s)).sum::<f64>();
    }
    out.extend_from_slice(&wcomp);
    let wbuild_slot = name_slot(&WONDER_NAMES, p.wonder);
    for i in 0..WONDER_NAMES.len() {
        out.push(if wbuild_slot == Some(i) { 1.0 } else { 0.0 });
    }

    // (A7) leader identity.
    let leader_slot = name_slot(&LEADER_NAMES, p.leader);
    for i in 0..LEADER_NAMES.len() {
        out.push(if leader_slot == Some(i) { 1.0 } else { 0.0 });
    }

    // (A8) structure, obsolescence, cost-to-benefit.
    // `level + 1` so "no leader" and "an age A leader" are different numbers;
    // phi's `Leader` cannot tell them apart at all.
    out.push(if p.leader.is_none() { 0.0 } else { f64::from(p.leader.level()) + 1.0 });
    out.push(wcomp_stage_total);
    let wb_stages: &[u8] = if p.wonder.is_none() { &[] } else { p.wonder.get().stages };
    out.push(wb_stages.iter().map(|&s| f64::from(s)).sum::<f64>());
    out.push(f64::from(wb_stages.iter().copied().max().unwrap_or(0)));
    out.push(wb_stages.len() as f64);
    out.push(obsolete_levels);
    out.push(obsolete_workers);
    out.push(if n_techs > 0.0 { age_gap_total / n_techs } else { 0.0 });
    out.push(f64::from(unit_types.count_ones()));
    // Spread between my strongest and weakest OCCUPIED lane: "am I even
    // across the board or all-in on one ladder", which no per-type max can say.
    let occupied: Vec<i8> = (0..LANES).filter(|&l| lane_seen[l]).map(|l| lane_best_level[l]).collect();
    let spread = match (occupied.iter().max(), occupied.iter().min()) {
        (Some(hi), Some(lo)) => f64::from(hi - lo),
        _ => 0.0,
    };
    out.push(spread);

    let mut upgrade_best = 0.0f64;
    let mut hand_obsolete = 0.0;
    let mut costben_max = 0.0f64;
    let mut costben_total = 0.0;
    let mut hand_types = 0u32;
    for &id in p.hand_civil.as_slice() {
        hand_types |= 1 << (id.kind() as u32);
        let y = printed_yield(id);
        let ratio = y / (f64::from(id.get().science_cost) + 1.0);
        costben_max = costben_max.max(ratio);
        costben_total += ratio;
        if let Some(l) = lane_index(id.kind()) {
            let held = if lane_seen[l] { lane_best_yield[l] } else { 0.0 };
            upgrade_best = upgrade_best.max(y - held);
            if lane_seen[l] && i32::from(lane_best_level[l]) >= i32::from(id.level()) {
                hand_obsolete += 1.0;
            }
        }
    }
    out.push(upgrade_best);
    out.push(hand_obsolete);
    out.push(costben_max);
    out.push(if n_civil > 0.0 { costben_total / n_civil } else { 0.0 });
    out.push(f64::from(hand_types.count_ones()));

    // (A9) controls. `p.techs.len()` IS `WeightKey::NumTechs` and
    // `p.completed_wonders.len()` IS `WeightKey::Wonders`, both written
    // verbatim by `features.rs`. Ridge is handed a column the base set
    // already contains exactly; anything but ~0 means the screen is lying.
    out.push(p.techs.len() as f64);
    out.push(p.completed_wonders.len() as f64);

    // ================================================================
    // FAMILY B -- OPPONENT-RELATIVE / GAP-CONDITIONAL
    // ================================================================
    let me = effects::compute(trial, p);
    let r = rival_max(trial, idx);
    let late = horizon::lateness(trial);
    let left = horizon::rounds_left(trial, horizon::live_count(trial));
    let round = f64::from(trial.round);

    // Every gap is MINE MINUS THE BEST RIVAL'S, so positive is ahead. With
    // no live rival left there is no gap to speak of and all of them are 0,
    // which is also what phi's own rival keys do.
    let g_cr = if r.any { me.culture - r.culture_rate } else { 0 };
    let g_sr = if r.any { me.science - r.science_rate } else { 0 };
    let g_st = if r.any { me.strength - r.strength } else { 0 };
    let g_cs = if r.any { i32::from(p.culture) - r.culture } else { 0 };
    let g_ss = if r.any { i32::from(p.science) - r.science } else { 0 };
    let g_tl = if r.any { tech_levels(p) - r.tech_levels } else { 0 };
    let g_ca = if r.any { me.civil_actions - r.civil_actions } else { 0 };
    let g_mh = if r.any { p.hand_size_military() as i32 - r.mil_hand } else { 0 };

    let pos = |v: i32| f64::from(v.max(0));
    let neg = |v: i32| f64::from((-v).max(0));

    // (B1) hinged halves.
    out.push(pos(g_cr));
    out.push(neg(g_cr));
    out.push(pos(g_sr));
    out.push(neg(g_sr));
    out.push(pos(g_cs));
    out.push(neg(g_cs));
    out.push(pos(g_st));
    out.push((f64::from(g_st) - STRENGTH_LEAD_CAP_AT_FREEZE).max(0.0));
    out.push(neg(g_st));
    out.push(pos(g_tl));
    out.push(neg(g_tl));
    out.push(neg(g_ss));

    // (B2) sign x magnitude buckets. The `far` thresholds are one step of
    // each quantity's own scale: culture rates run to ~30 and science rates
    // to ~20 by age III, strength to ~40, so 4/3/6 separate "just behind"
    // from "structurally behind" rather than splitting the distribution in
    // half.
    out.extend_from_slice(&sign_buckets(g_cr, 4));
    out.extend_from_slice(&sign_buckets(g_sr, 3));
    out.extend_from_slice(&sign_buckets(g_st, 6));

    // (B3) gap crossed with how late the game is.
    out.push(f64::from(g_cr) * late);
    out.push(pos(g_cr) * late);
    out.push(neg(g_cr) * late);
    out.push(f64::from(g_sr) * late);
    out.push(pos(g_sr) * late);
    out.push(neg(g_sr) * late);
    out.push(f64::from(g_cr) * round);
    out.push(f64::from(g_st) * late);
    out.push(f64::from(g_cs) * late);
    out.push(f64::from(g_tl) * late);

    // (B4) projection to game end, and scale-free shares. The projection is
    // the label's own shape: today's culture gap plus the rate gap run out
    // over the rounds the horizon model says are left.
    let proj = f64::from(g_cs) + f64::from(g_cr) * left;
    out.push(proj);
    out.push(proj.max(0.0));
    out.push((-proj).max(0.0));
    out.push(trailing_fraction(me.culture, r.culture_rate));
    out.push(trailing_fraction(me.science, r.science_rate));
    let cr_sum = f64::from(me.culture.max(0) + r.culture_rate.max(0));
    out.push(if cr_sum > 0.0 { f64::from(me.culture.max(0)) / cr_sum } else { 0.5 });

    // (B5) gap-CONDITIONAL card value: the same hand, priced differently
    // depending on which side of the culture race I am on. `hand_value` is
    // phi's own `WeightKey::HandValue` spelling, sum of (age + 1).
    let hand_value: f64 = p.hand_civil.as_slice().iter().map(|&c| f64::from(c.level()) + 1.0).sum();
    out.push(hand_value * neg(g_cr));
    out.push(hand_value * pos(g_cr));
    out.push(shortfall_total * trailing_fraction(me.science, r.science_rate));

    // (B6) tempo, hinged. Military HAND SIZE only -- never its contents.
    out.push(pos(g_ca));
    out.push(neg(g_ca));
    out.push(pos(g_mh));
    out.push(neg(g_mh));

    // (B7) controls. `Wonders - RivalWonders` and `HandCivil -
    // RivalHandCivil` are exact linear combinations of two live phi columns
    // each: `features.rs` writes all four verbatim from the same
    // expressions used here.
    out.push(p.completed_wonders.len() as f64 - f64::from(r.wonders));
    out.push(p.hand_civil.len() as f64 - f64::from(r.hand_civil));

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

    /// THE GUARD ON THE ONE-HOT BLOCKS. A misspelled card name makes
    /// `name_slot` return `None` forever, which is an all-zero column --
    /// indistinguishable in the screen's output from a real feature that
    /// carries no signal. Every name must resolve to a real card OF THE
    /// EXPECTED TYPE, and each list must be the whole type.
    #[test]
    fn card_names_all_resolve() {
        for (names, kind, whole) in [
            (&GOV_NAMES[..], CardType::Government, 8),
            (&SPEC_NAMES[..], CardType::SpecialTech, 12),
            (&WONDER_NAMES[..], CardType::Wonder, 16),
            (&LEADER_NAMES[..], CardType::Leader, 24),
        ] {
            for n in names {
                let id = CardId::by_name(n).unwrap_or_else(|| panic!("no card named {n:?}"));
                assert_eq!(id.kind(), kind, "{n:?} is not a {kind:?}");
                assert_eq!(name_slot(names, id), names.iter().position(|w| w == n));
            }
            let in_table = crate::card_table::CARDS.iter().filter(|c| c.kind == kind).count();
            assert_eq!(names.len(), whole, "{kind:?} list changed size");
            assert_eq!(names.len(), in_table, "{kind:?} list is not the whole type");
        }
    }

    /// The name-keyed columns must be keyed to the KEY NAMES too: every
    /// `gran_gov_*` / `gran_spec_*` / `gran_wcomp_*` / `gran_wbuild_*` /
    /// `gran_leader_*` prefix must have exactly as many `EXTRA_KEYS` entries
    /// as its list has cards, or a one-hot block is silently misaligned with
    /// the sidecar the screen reads.
    #[test]
    fn one_hot_blocks_match_their_key_prefixes() {
        let n = |pre: &str| EXTRA_KEYS.iter().filter(|k| k.starts_with(pre)).count();
        assert_eq!(n("gran_gov_"), GOV_NAMES.len() + 2, "8 one-hots + 2 printed prices");
        assert_eq!(n("gran_spec_"), SPEC_NAMES.len());
        assert_eq!(n("gran_wcomp_"), WONDER_NAMES.len() + 1, "16 one-hots + gran_wcomp_stage_total");
        assert_eq!(n("gran_wbuild_"), WONDER_NAMES.len() + 3, "16 one-hots + 3 stage scalars");
        assert_eq!(n("gran_leader_"), LEADER_NAMES.len() + 1, "24 one-hots + gran_leader_age");
        let mut sorted = EXTRA_KEYS.to_vec();
        sorted.sort_unstable();
        let before = sorted.len();
        sorted.dedup();
        assert_eq!(sorted.len(), before, "duplicate column name in EXTRA_KEYS");
    }

    /// `sign_buckets` must be a PARTITION -- exactly one bucket lit, at
    /// every gap -- or the "bucketed by sign and magnitude" family is not
    /// measuring what it says.
    #[test]
    fn sign_buckets_partition() {
        for far in [1i32, 3, 4, 6] {
            for gap in -20i32..=20 {
                let b = sign_buckets(gap, far);
                assert_eq!(b.iter().sum::<f64>(), 1.0, "gap {gap} far {far} lit {b:?}");
                let lit = b.iter().position(|&v| v == 1.0).unwrap();
                let want = if gap < 0 { lit <= 1 } else if gap == 0 { lit == 2 } else { lit >= 3 };
                assert!(want, "gap {gap} far {far} landed in bucket {lit}");
            }
        }
    }

    /// The two hinges of one gap must reconstruct the gap: `pos - neg == gap`
    /// and `min(pos, neg) == 0`. Cheap, but it is the property the whole of
    /// family B rests on.
    #[test]
    fn hinges_decompose_the_gap() {
        for g in -30i32..=30 {
            let (p, n) = (f64::from(g.max(0)), f64::from((-g).max(0)));
            assert_eq!(p - n, f64::from(g));
            assert_eq!(p.min(n), 0.0);
        }
        assert_eq!(trailing_fraction(4, 4), 0.0, "not behind");
        assert_eq!(trailing_fraction(5, 4), 0.0, "ahead");
        assert_eq!(trailing_fraction(0, 0), 0.0, "no leader");
        assert_eq!(trailing_fraction(2, 4), 0.5);
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
                        assert_controls_are_exact_phi_functions(&phi, &extras);
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

    fn col(name: &str) -> usize {
        EXTRA_KEYS.iter().position(|k| *k == name).unwrap_or_else(|| panic!("no column {name}"))
    }

    /// THE REDUNDANCY CONTROLS ARE ONLY CONTROLS IF THEY REALLY ARE
    /// FUNCTIONS OF `phi`. Each of the four is asserted here against the
    /// live phi columns it claims to reproduce, on every decision the guard
    /// test visits -- so "the control came back ~0" is evidence about the
    /// SCREEN, not an accident of a control that was never redundant.
    fn assert_controls_are_exact_phi_functions(phi: &[f64], extras: &[f64]) {
        use crate::bots::weighted::weights::WeightKey;
        let k = |w: WeightKey| phi[w as usize];
        assert_eq!(extras[col("ctrl_a_num_techs")], k(WeightKey::NumTechs));
        assert_eq!(extras[col("ctrl_a_wonders_count")], k(WeightKey::Wonders));
        assert_eq!(
            extras[col("ctrl_b_wonder_gap")],
            k(WeightKey::Wonders) - k(WeightKey::RivalWonders)
        );
        assert_eq!(
            extras[col("ctrl_b_hand_civil_gap")],
            k(WeightKey::HandCivil) - k(WeightKey::RivalHandCivil)
        );
    }

    /// LEGALITY, ASSERTED RATHER THAN PROMISED: a rival's military hand is
    /// face down (`horizon.rs`'s own public/private boundary), so replacing
    /// its CONTENTS while leaving its SIZE alone must not move a single
    /// candidate column. If a future column starts reading those cards this
    /// test fails instead of the leak shipping.
    #[test]
    fn rival_military_hand_contents_are_not_read() {
        let w = crate::bots::weighted::weights::Weights::default();
        let seats = vec![Seat { kind: BotKind::Weighted, weights: w, search: Search::None }; 2];
        let mut bots = build_bots(&seats, 5);
        let mut state = game::new_game(2, 5);
        let mut swapped = 0usize;
        let _ = game::play_game(&mut state, MOVE_CAP, |s, _legal| {
            let mv = bots[s.decider() as usize].pick(s);
            let idx = s.decider();
            let rival = 1 - idx as usize;
            let n = s.players[rival].hand_military.len();
            if n > 0 {
                let base = extra_columns(s, idx);
                let mut alt = s.clone();
                // Same size, different cards: reverse the rival's military
                // hand and replace every card with a different real one.
                let subs = [
                    CardId::by_name("War over Culture").unwrap(),
                    CardId::by_name("War over Territory").unwrap(),
                ];
                let hand = alt.players[rival].hand_military.as_mut_slice();
                for slot in hand.iter_mut().take(n) {
                    *slot = *subs.iter().find(|&&c| c != *slot).unwrap();
                }
                assert_eq!(
                    extra_columns(&alt, idx),
                    base,
                    "a candidate column moved when only the RIVAL's military hand contents changed"
                );
                swapped += 1;
            }
            mv
        });
        assert!(swapped > 20, "never saw a rival military hand to swap: {swapped}");
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
