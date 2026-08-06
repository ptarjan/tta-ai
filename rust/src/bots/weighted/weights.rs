//! `engine/bots/weighted.py` lines 3548-4103: the weight vector `evaluate`
//! is linear over -- 130 named knobs, almost all defaulting to 0.0 so a new
//! channel changes nothing until the league climbs it away from zero. See
//! that Python range's own extensive per-weight commentary for the "why"
//! behind each fitted number; it is the source of the rationale and is
//! deliberately not reproduced line-for-line here -- this module ports the
//! SHAPE and the VALUES (pinned against Python byte-for-byte by
//! `rust/tests/weighted_horizon.rs`), not the prose.
//!
//! ## Representation: an enum, not a string-keyed map
//!
//! Python threads `w: dict[str, float]` through every function in this file;
//! a Rust `HashMap<String, f64>` would be the same shape with an allocation
//! and a hash on every read, on what `evaluate` calls for every candidate
//! move of every 1-ply search -- exactly the "Python in Rust" shape flagged
//! in an earlier batch of this port. Instead:
//!
//! * [`WeightKey`] is a fieldless enum, one variant per weight -- built,
//!   along with its printed name and its default value, from ONE macro
//!   invocation (`weight_keys!` below) so the three can never list a
//!   different set of keys. A misspelled or renamed key is a Rust compile
//!   error, not a `KeyError` five functions downstream or (worse) a silent
//!   `dict.get(k, 0.0)` that returns the wrong number forever.
//! * [`Weights`] is `[f64; N]` indexed by `key as usize` -- `#[repr(usize)]`
//!   on a fieldless enum with no explicit discriminants assigns 0..N-1 in
//!   declaration order, so [`Weights::get`]/[`Weights::set`] are a plain
//!   bounds-checked array index, not a hash lookup.
//!
//! ## The phase-suffixed keys
//!
//! Python builds `PHASE_WEIGHTS` BY COMPREHENSION over `PHASE_KEYS` (`{k + s:
//! _PHASE_PRIOR[k + s] for k in PHASE_KEYS for s in ("_early", "_late")}`)
//! rather than writing the eight keys out by hand, specifically so
//! `PHASE_KEYS` and the phase table can never name a different set of
//! features (see that constant's own comment: "a pair for a key that is no
//! longer phase-multiplied would be a weight the trainer mutates, the guard
//! checks and `evaluate` never reads"). The flat `weight_keys!` table below
//! still needs eight literal entries for the `*_early`/`*_late` variants -- a
//! `macro_rules!` cannot synthesize a new identifier by string concatenation
//! without a proc-macro, and this crate deliberately has none (`Cargo.toml`'s
//! `[dependencies]` is empty on purpose) -- so the "can never disagree"
//! guarantee is reconstructed a different way: [`PHASE_KEYS`] lists only the
//! four BASE keys, and [`WeightKey::early`]/[`WeightKey::late`] are the ONLY
//! way to reach a phase partner -- both `match` on exactly those four
//! variants and panic on any other input, so a caller can never silently read
//! a phase pair for a key that is not one. `tests::
//! phase_keys_and_the_flat_table_agree` below checks the converse direction:
//! every `*_early`/`*_late` entry that IS in the flat table is reachable from
//! `PHASE_KEYS`, and nothing else is. The two (the macro table and the
//! `early`/`late` match arms) can now drift only if both are edited to agree
//! on the same new lie, which is a narrower window than Python's single
//! dict comprehension leaves open, but it is the honest cost of this crate
//! having no macro-generated identifiers.
//!
//! ## `RETIRED_KEYS`
//!
//! Twelve weight names a future `eval.rs::load_weights` must drop from a
//! loaded champion JSON, exactly as Python's `load_weights` does today. They
//! are deliberately NOT [`WeightKey`] variants -- adding one back as a
//! variant would make it indexable again, which is the opposite of retired.
//! Bare `&'static str`s are therefore the only representation that fits; see
//! [`RETIRED_KEYS`]'s own doc comment for the two reasons this has to be a
//! named, visible list rather than simply deleting the key everywhere.

/// One knob in the linear evaluator. See this module's top doc comment for
/// the representation rationale. `#[repr(usize)]` with no explicit
/// discriminants assigns 0..N-1 in declaration order, which is what makes
/// `self as usize` a valid [`Weights`] index.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(usize)]
pub enum WeightKey {
    RateHorizon,
    Culture,
    CultureRate,
    Science,
    ScienceRate,
    FoodRate,
    ResourceRate,
    FoodStock,
    ResourceStock,
    BlueFree,
    CorruptionLoss,
    Consumption,
    PopCost,
    YellowBank,
    FreeWorkers,
    Workers,
    ProdWorkers,
    UrbanWorkers,
    UnitWorkers,
    HappyMargin,
    Discontent,
    Uprising,
    CivilActions,
    MilitaryActions,
    CaLeft,
    MaLeft,
    TakeCostPaid,
    RowUrgency,
    RowBargainForgone,
    RowLastCopy,
    RivalDesire,
    RivalTakeShare,
    RivalFreeCa,
    RivalHandCivil,
    RivalWonders,
    RivalHandPotential,
    RivalScienceStock,
    RivalFoodStock,
    RivalResourceStock,
    RivalFreeWorkers,
    RivalYellowBank,
    RivalColonies,
    RivalMilActions,
    RivalBuildingWonder,
    MySeededPending,
    MyEventThreat,
    AttackTargetLead,
    AttackTargetWeakness,
    PactPartnerLead,
    Strength,
    StrengthRel,
    StrengthDeficit,
    StrengthLead,
    TacticLevel,
    TacticGain,
    TacticShort,
    Colonies,
    Pacts,
    PactBlocksAttack,
    AuctionCommitted,
    AuctionBid,
    TechLevels,
    GovLevel,
    BestFarm,
    BestMine,
    BestLab,
    BestTemple,
    BestTheater,
    BestLibrary,
    BestArena,
    BestUnit,
    NumTechs,
    SpecialTechs,
    Wonders,
    WonderProgress,
    WonderRemaining,
    WonderStagesLeft,
    WonderTurnsToFinish,
    WonderOverrun,
    WonderStagesPerAction,
    WonderPotential,
    Leader,
    HandLimit,
    ColonizeBonus,
    BuildDiscount,
    FreeCivilAction,
    ResourceDiscount,
    DefenseBonus,
    UrbanLimit,
    GovActionCost,
    NoAggression,
    RestrictedResources,
    CardBoardCredit,
    EventScoringMargin,
    CardBoardLeader,
    CardBoardGovernment,
    CardBoardAction,
    CardBoardWonder,
    HandSwapExtra,
    CardRateCredit,
    UnitStrengthCredit,
    UnitTechCredit,
    TechBoardCredit,
    ActionBoardCredit,
    GovBoardCredit,
    BuildFreshCredit,
    RestrictedResourceCredit,
    FreeActionCredit,
    TerritoryCredit,
    BonusCardCredit,
    HandCivil,
    HandValue,
    HandPotential,
    HandMilitary,
    HandMilValue,
    HandMilPotential,
    RivalCulture,
    RivalMeanCulture,
    RivalCultureRate,
    RivalScienceRate,
    RivalStrength,
    EndTurnBias,
    WorkersEarly,
    WorkersLate,
    StrengthRelEarly,
    StrengthRelLate,
    TechLevelsEarly,
    TechLevelsLate,
    HandValueEarly,
    HandValueLate,
}

/// Generates `WeightKey::ALL`, `WeightKey::name` and `WeightKey::
/// default_weight` from ONE list of `(Variant, "printed_name", default)`
/// triples, so the three can never disagree about which keys exist -- see
/// this module's top doc comment.
macro_rules! weight_key_table {
    ( $( $variant:ident => $name:literal, $default:expr );+ $(;)? ) => {
        impl WeightKey {
            /// Every key, in declaration order -- the one place all 130 are
            /// listed together outside the enum declaration itself.
            pub const ALL: &'static [WeightKey] = &[ $( WeightKey::$variant, )+ ];

            /// The exact string Python's `DEFAULT_WEIGHTS` uses for this key.
            /// The I/O boundary (JSON weight files, the differential test) --
            /// nothing inside the evaluator itself should ever need this.
            pub const fn name(self) -> &'static str {
                match self { $( WeightKey::$variant => $name, )+ }
            }

            /// `DEFAULT_WEIGHTS[self.name()]`.
            pub const fn default_weight(self) -> f64 {
                match self { $( WeightKey::$variant => $default, )+ }
            }
        }
    };
}

weight_key_table! {
    RateHorizon => "rate_horizon", 1.0;
    Culture => "culture", 1.0;
    CultureRate => "culture_rate", 5.0;
    Science => "science", 0.5;
    ScienceRate => "science_rate", 4.0;
    FoodRate => "food_rate", 1.2;
    ResourceRate => "resource_rate", 1.6;
    FoodStock => "food_stock", 0.2;
    ResourceStock => "resource_stock", 0.3;
    BlueFree => "blue_free", 0.15;
    CorruptionLoss => "corruption_loss", -0.9;
    Consumption => "consumption", -0.5;
    PopCost => "pop_cost", -0.4;
    YellowBank => "yellow_bank", -0.1;
    FreeWorkers => "free_workers", 0.4;
    Workers => "workers", 1.4;
    ProdWorkers => "prod_workers", 0.3;
    UrbanWorkers => "urban_workers", 0.5;
    UnitWorkers => "unit_workers", 0.1;
    HappyMargin => "happy_margin", 1.2;
    Discontent => "discontent", -3.0;
    Uprising => "uprising", -12.0;
    CivilActions => "civil_actions", 2.0;
    MilitaryActions => "military_actions", 0.7;
    CaLeft => "ca_left", 0.05;
    MaLeft => "ma_left", 0.05;
    TakeCostPaid => "take_cost_paid", 0.0;
    RowUrgency => "row_urgency", 0.0;
    RowBargainForgone => "row_bargain_forgone", 0.0;
    RowLastCopy => "row_last_copy", 0.0;
    RivalDesire => "rival_desire", 0.0;
    RivalTakeShare => "rival_take_share", 0.5;
    RivalFreeCa => "rival_free_ca", 0.0;
    RivalHandCivil => "rival_hand_civil", 0.0;
    RivalWonders => "rival_wonders", 0.0;
    RivalHandPotential => "rival_hand_potential", 0.0;
    RivalScienceStock => "rival_science_stock", 0.0;
    RivalFoodStock => "rival_food_stock", 0.0;
    RivalResourceStock => "rival_resource_stock", 0.0;
    RivalFreeWorkers => "rival_free_workers", 0.0;
    RivalYellowBank => "rival_yellow_bank", 0.0;
    RivalColonies => "rival_colonies", 0.0;
    RivalMilActions => "rival_mil_actions", 0.0;
    RivalBuildingWonder => "rival_building_wonder", 0.0;
    MySeededPending => "my_seeded_pending", 0.0;
    MyEventThreat => "my_event_threat", 0.0;
    AttackTargetLead => "attack_target_lead", 0.0;
    AttackTargetWeakness => "attack_target_weakness", 0.0;
    PactPartnerLead => "pact_partner_lead", 0.0;
    Strength => "strength", 0.35;
    StrengthRel => "strength_rel", 0.35;
    StrengthDeficit => "strength_deficit", -0.6;
    StrengthLead => "strength_lead", 0.3;
    TacticLevel => "tactic_level", 0.5;
    TacticGain => "tactic_gain", 0.0;
    TacticShort => "tactic_short", 0.0;
    Colonies => "colonies", 2.0;
    Pacts => "pacts", 0.5;
    PactBlocksAttack => "pact_blocks_attack", 0.5;
    AuctionCommitted => "auction_committed", 2.0;
    AuctionBid => "auction_bid", -0.4;
    TechLevels => "tech_levels", 1.0;
    GovLevel => "gov_level", 2.0;
    BestFarm => "best_farm", 0.5;
    BestMine => "best_mine", 0.5;
    BestLab => "best_lab", 0.8;
    BestTemple => "best_temple", 0.6;
    BestTheater => "best_theater", 0.6;
    BestLibrary => "best_library", 0.5;
    BestArena => "best_arena", 0.3;
    BestUnit => "best_unit", 0.5;
    NumTechs => "num_techs", 0.3;
    SpecialTechs => "special_techs", 0.8;
    Wonders => "wonders", 3.0;
    WonderProgress => "wonder_progress", 1.0;
    WonderRemaining => "wonder_remaining", -0.3;
    WonderStagesLeft => "wonder_stages_left", 0.0;
    WonderTurnsToFinish => "wonder_turns_to_finish", 0.0;
    WonderOverrun => "wonder_overrun", 0.0;
    WonderStagesPerAction => "wonder_stages_per_action", 0.0;
    WonderPotential => "wonder_potential", 0.0;
    Leader => "leader", 1.5;
    HandLimit => "hand_limit", 0.0;
    ColonizeBonus => "colonize_bonus", 0.0;
    BuildDiscount => "build_discount", 0.0;
    FreeCivilAction => "free_civil_action", 0.0;
    ResourceDiscount => "resource_discount", 0.0;
    DefenseBonus => "defense_bonus", 0.0;
    UrbanLimit => "urban_limit", 0.0;
    GovActionCost => "gov_action_cost", 0.0;
    NoAggression => "no_aggression", 0.0;
    RestrictedResources => "restricted_resources", 0.0;
    CardBoardCredit => "card_board_credit", 0.0;
    EventScoringMargin => "event_scoring_margin", 0.0;
    CardBoardLeader => "card_board_leader", 0.0;
    CardBoardGovernment => "card_board_government", 0.0;
    CardBoardAction => "card_board_action", 0.0;
    CardBoardWonder => "card_board_wonder", 0.0;
    HandSwapExtra => "hand_swap_extra", 0.0;
    CardRateCredit => "card_rate_credit", 1.0;
    UnitStrengthCredit => "unit_strength_credit", 0.0;
    UnitTechCredit => "unit_tech_credit", 1.0;
    TechBoardCredit => "tech_board_credit", 1.0;
    ActionBoardCredit => "action_board_credit", 1.0;
    GovBoardCredit => "gov_board_credit", 1.0;
    BuildFreshCredit => "build_fresh_credit", 0.0;
    RestrictedResourceCredit => "restricted_resource_credit", 1.0;
    FreeActionCredit => "free_action_credit", 0.0;
    TerritoryCredit => "territory_credit", 1.0;
    BonusCardCredit => "bonus_card_credit", 1.0;
    HandCivil => "hand_civil", 0.3;
    HandValue => "hand_value", 0.25;
    HandPotential => "hand_potential", 0.125;
    HandMilitary => "hand_military", 0.3;
    HandMilValue => "hand_mil_value", 0.15;
    HandMilPotential => "hand_mil_potential", 0.0;
    RivalCulture => "rival_culture", -0.35;
    RivalMeanCulture => "rival_mean_culture", -0.1;
    RivalCultureRate => "rival_culture_rate", -1.0;
    RivalScienceRate => "rival_science_rate", -0.6;
    RivalStrength => "rival_strength", -0.15;
    EndTurnBias => "end_turn_bias", -3.0;

    // phase-suffixed pairs -- see this module's top doc comment for why
    // these eight are still spelled out by hand here even though PHASE_KEYS
    // below is the honest source of "which base keys get a pair".
    WorkersEarly => "workers_early", 0.8;
    WorkersLate => "workers_late", -0.6;
    StrengthRelEarly => "strength_rel_early", -0.1;
    StrengthRelLate => "strength_rel_late", 0.5;
    TechLevelsEarly => "tech_levels_early", 0.5;
    TechLevelsLate => "tech_levels_late", -0.4;
    HandValueEarly => "hand_value_early", 0.2;
    HandValueLate => "hand_value_late", -0.2;
}

impl WeightKey {
    /// Parse a printed name. **I/O only** -- JSON weight files, the
    /// differential test -- mirrors `CardId::by_name`'s linear scan and the
    /// same reasoning: making this fast would mean a second, hashed index
    /// nothing on the evaluation hot path is allowed to need.
    pub fn by_name(name: &str) -> Option<WeightKey> {
        WeightKey::ALL.iter().copied().find(|k| k.name() == name)
    }

    /// The early-phase partner of a [`PHASE_KEYS`] member -- `w[k + "_early"]`
    /// in Python, blended into `evaluate` as `(1 - lateness) * w[k_early]`.
    ///
    /// # Panics
    /// If `self` is not one of the four [`PHASE_KEYS`] members -- mirrors
    /// Python's `_PHASE_PRIOR[k + "_early"]`, a `KeyError` for the same
    /// misuse. Restricting the match to exactly those four (rather than
    /// falling back to `self` or to `0.0`) is what makes it impossible to
    /// silently read a phase pair for a key that is not phase-multiplied.
    pub const fn early(self) -> WeightKey {
        match self {
            WeightKey::Workers => WeightKey::WorkersEarly,
            WeightKey::StrengthRel => WeightKey::StrengthRelEarly,
            WeightKey::TechLevels => WeightKey::TechLevelsEarly,
            WeightKey::HandValue => WeightKey::HandValueEarly,
            _ => panic!("WeightKey::early called on a key outside PHASE_KEYS"),
        }
    }

    /// The late-phase partner of a [`PHASE_KEYS`] member. See [`Self::early`].
    ///
    /// # Panics
    /// If `self` is not one of the four [`PHASE_KEYS`] members.
    pub const fn late(self) -> WeightKey {
        match self {
            WeightKey::Workers => WeightKey::WorkersLate,
            WeightKey::StrengthRel => WeightKey::StrengthRelLate,
            WeightKey::TechLevels => WeightKey::TechLevelsLate,
            WeightKey::HandValue => WeightKey::HandValueLate,
            _ => panic!("WeightKey::late called on a key outside PHASE_KEYS"),
        }
    }
}

/// Mirrors Python's `PHASE_KEYS`: which BASE features additionally carry an
/// early-game/late-game pair, blended by `lateness()` as `w[k] + (1 - L) *
/// w[k.early()] + L * w[k.late()]`. The four that stay, and why six others
/// (`culture`, `culture_rate`, `science_rate`, `food_rate`, `resource_rate`,
/// `wonder_progress`) were retired on 2026-08-04 -- `rate_multiplier` now
/// prices the four RATE_KEYS through the exact `rounds_left`-derived horizon
/// instead of this affine shape, and `culture`/`wonder_progress` are
/// numeraire/stock terms a phase blend must not rescale -- are explained at
/// length in the Python source's own comment on this constant; not
/// reproduced here.
pub const PHASE_KEYS: &[WeightKey] = &[
    WeightKey::Workers,
    WeightKey::StrengthRel,
    WeightKey::TechLevels,
    WeightKey::HandValue,
];

/// Weight names that USED to be [`WeightKey`] variants and were deliberately
/// retired -- mirrors Python's `RETIRED_KEYS`. A name here is a promise that
/// the key is GONE, not renamed; re-adding one means taking it out of this
/// list in the same commit that adds the [`WeightKey`] variant back.
///
/// Two reasons this has to be a named, visible list rather than simply
/// dropping the key everywhere and saying nothing:
///
/// 1. Every champion JSON on disk predates the removal and still carries
///    these. A future `eval.rs::load_weights` must drop them on load (mirrors
///    Python's `load_weights`) so a hill climb never spends part of its
///    mutation budget perturbing a weight nothing reads.
/// 2. A coordinate-registry check elsewhere in this project flags any key a
///    loaded vector carries that this module does not recognise. A
///    deliberate retirement is neither a typo nor an orphan, and listing
///    twelve keys here (once) is what keeps that check from being unable to
///    tell the two apart.
///
/// Bare `&'static str`s, not [`WeightKey`] variants -- giving a retired key a
/// variant would make it indexable again, which is the opposite of retired.
pub const RETIRED_KEYS: &[&str] = &[
    "culture_early",
    "culture_late",
    "culture_rate_early",
    "culture_rate_late",
    "science_rate_early",
    "science_rate_late",
    "food_rate_early",
    "food_rate_late",
    "resource_rate_early",
    "resource_rate_late",
    "wonder_progress_early",
    "wonder_progress_late",
];

/// [`WeightKey::ALL`]'s length -- every [`Weights`] array is exactly this
/// wide.
const N: usize = WeightKey::ALL.len();

/// A full weight vector -- Python's `dict[str, float]`, e.g. `DEFAULT_WEIGHTS`
/// or a loaded/mutated champion. Backed by `[f64; N]` indexed by `key as
/// usize`: [`Weights::get`]/[`Weights::set`] are a bounds-checked array
/// index, not a hash lookup, which is what buys back the string-keyed dict's
/// flexibility without its cost on `evaluate`'s hot path.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Weights([f64; N]);

impl Weights {
    /// `DEFAULT_WEIGHTS`: every key at [`WeightKey::default_weight`].
    pub fn defaults() -> Weights {
        let mut out = [0.0; N];
        for &k in WeightKey::ALL {
            out[k as usize] = k.default_weight();
        }
        Weights(out)
    }

    #[inline]
    pub fn get(&self, key: WeightKey) -> f64 {
        self.0[key as usize]
    }

    #[inline]
    pub fn set(&mut self, key: WeightKey, value: f64) {
        self.0[key as usize] = value;
    }
}

impl Default for Weights {
    fn default() -> Self {
        Weights::defaults()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    /// Every [`WeightKey`] round-trips through its printed name -- the
    /// invariant the differential test (`rust/tests/weighted_horizon.rs`)
    /// relies on to compare Rust's defaults against Python's by name.
    #[test]
    fn every_weight_key_name_round_trips() {
        for &k in WeightKey::ALL {
            assert_eq!(WeightKey::by_name(k.name()), Some(k), "{}", k.name());
        }
    }

    /// No two keys share a printed name -- if they did, `by_name` would
    /// silently return the first and never the second.
    #[test]
    fn every_weight_key_name_is_unique() {
        let names: HashSet<&str> = WeightKey::ALL.iter().map(|k| k.name()).collect();
        assert_eq!(names.len(), WeightKey::ALL.len());
    }

    /// The generation-honesty check this module's top doc comment promises:
    /// `PHASE_KEYS` and the flat table's `*_early`/`*_late` entries name
    /// EXACTLY the same set of features, checked in both directions.
    #[test]
    fn phase_keys_and_the_flat_table_agree() {
        for &k in PHASE_KEYS {
            assert!(WeightKey::ALL.contains(&k.early()), "{}: early() not in ALL", k.name());
            assert!(WeightKey::ALL.contains(&k.late()), "{}: late() not in ALL", k.name());
            assert_eq!(k.early().name(), format!("{}_early", k.name()));
            assert_eq!(k.late().name(), format!("{}_late", k.name()));
        }
        for &k in WeightKey::ALL {
            let name = k.name();
            let base = name.strip_suffix("_early").or_else(|| name.strip_suffix("_late"));
            if let Some(base) = base {
                assert!(
                    PHASE_KEYS.iter().any(|&p| p.name() == base),
                    "{name}: phase-suffixed key with no PHASE_KEYS base"
                );
            }
        }
    }

    /// A retired key must not have quietly grown back into a live
    /// [`WeightKey`] variant.
    #[test]
    fn retired_keys_are_not_weight_keys() {
        for &name in RETIRED_KEYS {
            assert_eq!(WeightKey::by_name(name), None, "{name} is retired but still a WeightKey");
        }
    }

    /// `RETIRED_KEYS` itself has no duplicates and no overlap with `ALL`'s
    /// names -- both are read together by a future `load_weights`.
    #[test]
    fn retired_keys_has_no_duplicates() {
        let set: HashSet<&str> = RETIRED_KEYS.iter().copied().collect();
        assert_eq!(set.len(), RETIRED_KEYS.len());
    }

    #[test]
    fn weights_default_matches_every_keys_default_weight() {
        let w = Weights::default();
        for &k in WeightKey::ALL {
            assert_eq!(w.get(k), k.default_weight(), "{}", k.name());
        }
    }

    #[test]
    fn weights_get_set_round_trips() {
        let mut w = Weights::default();
        w.set(WeightKey::EndTurnBias, 42.0);
        assert_eq!(w.get(WeightKey::EndTurnBias), 42.0);
        // an unrelated key is untouched
        assert_eq!(w.get(WeightKey::Culture), WeightKey::Culture.default_weight());
    }
}
