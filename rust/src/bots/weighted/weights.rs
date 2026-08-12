//! `engine/bots/weighted.py` lines 3548-4103: the weight vector `evaluate`
//! is linear over -- 139 named knobs, almost all defaulting to 0.0 so a new
//! channel changes nothing until the league climbs it away from zero. See
//! that Python range's own extensive per-weight commentary for the "why"
//! behind each fitted number; it is the source of the rationale and is
//! deliberately not reproduced line-for-line here -- this module ports the
//! SHAPE and the VALUES (pinned against Python byte-for-byte during the
//! port by the since-retired `rust/tests/weighted_horizon.rs`), not the
//! prose.
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
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
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
    CorruptionHeadroom,
    ConsumptionHeadroom,
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
    HasUnit,
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
    CardBoardBonus,
    HandSwapExtra,
    CardRateCredit,
    UnitStrengthCredit,
    UnitTechCredit,
    TechBoardCredit,
    ActionBoardCredit,
    GovBoardCredit,
    WonderBoardCredit,
    BuildFreshCredit,
    RestrictedResourceCredit,
    FreeActionCredit,
    TerritoryCredit,
    BonusCardCredit,
    // The military-deck classes docs/OPEN_ITEMS.md item 2 priced at exactly
    // 0.0 -- Tactic/Aggression/War/Pact/Event each dispatch through a
    // dedicated board-aware valuation function (`cards::tactic_value`/
    // `aggression_value`/`war_hand_value`/`pact_value`/`event_prepare_value`),
    // gated by its own credit exactly like `tech_board_credit`/
    // `action_board_credit`/`gov_board_credit`/`wonder_board_credit` already
    // gate their own dedicated functions.
    TacticBoardCredit,
    AggressionBoardCredit,
    WarBoardCredit,
    PactBoardCredit,
    EventBoardCredit,
    // `cards::tactic_value`'s per-unit-worker-still-owed penalty -- the
    // gradient `tactic_gain`'s step-function alone lacks, ported from the
    // same shape `tactic_short` (the whole-hand analogue) already uses.
    TacticShortfallCost,
    // `cards::tactic_value`'s "how much to trust the projected value of a
    // tactic that cannot form even one army YET" -- gates the reachability
    // estimate (`cards::unit_type_reach_cost`) that replaced a flat "0.0
    // until formable" cliff. Distinct from `TacticShortfallCost` (which
    // prices the NEXT army once the first is already formable): this one
    // prices reaching the FIRST army at all, off real board facts (owned
    // tech, the visible card row, printed build/develop costs) rather than
    // a fitted constant -- see that function's own doc comment.
    TacticReachCredit,
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

    /// Standing hinge on [`CultureRate`](Self::CultureRate): what one more
    /// point of culture PRODUCTION is worth ON TOP of the flat rate, scaled
    /// by how far behind the best rival this player currently is. Gated at
    /// 0.0 so it changes nothing until the league prices it.
    CultureRateTrailing,
    /// Standing hinge on [`ScienceRate`](Self::ScienceRate). See
    /// [`CultureRateTrailing`](Self::CultureRateTrailing).
    ScienceRateTrailing,
    WorkersEarly,
    WorkersLate,
    StrengthRelEarly,
    StrengthRelLate,
    TechLevelsEarly,
    TechLevelsLate,
    HandValueEarly,
    HandValueLate,

    // ------------------------------------------------- marginal need (gap/surplus)
    // A card is not worth a fixed amount -- it is worth how much it closes
    // the gap a player actually has right now (Paul's own framing: Moses is
    // huge on turn one and irrelevant once farms are already stacked; Iron
    // makes Coal nearly worthless; a missing Lab changes Age II priorities).
    // A linear model over a single stock ("I have 4 farms") cannot express
    // that -- it prices the 5th farm the same as the 1st. Putting the
    // nonlinearity in the FEATURE instead of the weight buys diminishing
    // returns and conditionality back for free: for each axis below,
    // `features()` computes a live "need" off real board facts (the cost of
    // the next population increase, unfilled worker slots, the cheapest
    // unbuilt tech's resource cost, ...) and emits the SHORTFALL
    // (`max(0, need - have)`) and the SURPLUS (`max(0, have - need)`) as two
    // separate coordinates, never a single signed difference -- the climb
    // can then price a shortfall steeply and a surplus cheaply, which is the
    // actual shape of the game. All default 0.0 -- unmeasured by
    // construction, same "trust nothing until the league finds it" rule
    // every other 0.0-seeded key in this table follows.
    //
    // One axis this project already covers by this exact shape is
    // deliberately NOT duplicated here: military strength relative to the
    // strongest rival (`StrengthDeficit`/`StrengthLead`, above). Card
    // redundancy within a `CardType` lane (Iron/Coal) is NOT already
    // covered, despite `board_yields::tech_upgrade`'s `best_feature`
    // delta-over-incumbent `dev` credit looking at first glance like it
    // would be: that function's sibling `staff` term can move MULTIPLE
    // lower-level same-lane workers onto one new card in a single
    // hypothetical, which GROWS `card_potential`'s total with how much the
    // player already has in the lane rather than shrinking it -- checked
    // directly (`cards.rs::tests::
    // owning_a_stronger_mine_already_makes_a_further_mine_upgrade_worth_far_less`),
    // not assumed. `TechRedundancyDiscount` below is the real fix: an
    // independent, always-non-positive term `card_potential` subtracts,
    // gated at 0.0 like every other key here so it changes nothing until
    // the league prices it.
    FoodGap,
    FoodSurplus,
    ResourceGap,
    ResourceSurplus,
    ScienceGap,
    ScienceSurplus,
    // Culture's "need" is not an absolute threshold the way food/happiness
    // have one (no rule ever converts a stock of culture into a cost) -- the
    // live pressure is competitive: how far behind (or ahead of) the
    // strongest rival's culture this player is, the same "relative to the
    // field" shape `StrengthRel`/`StrengthDeficit`/`StrengthLead` already
    // use for military.
    CultureGap,
    CultureSurplus,
    // Happiness already has a shortfall coordinate (`Discontent`, above,
    // `max(0, -margin)`) -- this is only the missing surplus half of that
    // same hinge, kept as a new key rather than reshaping `HappyMargin`
    // (which mixes the negative tail back in and is `.min(3.0)`-capped for
    // reasons unrelated to this batch) so no existing champion's evaluation
    // moves.
    HappySurplus,
    CivilActionGap,
    CivilActionSurplus,
    MilitaryActionGap,
    MilitaryActionSurplus,
    WorkerGap,
    WorkerSurplus,

    // ------------------------------------------------------------- redundancy
    // `cards::redundancy_discount`'s gate -- how much of a card's value to
    // discount by how well its `CardType` lane is already covered (Iron/Coal,
    // generalised off the lane rather than the two named cards; see that
    // function's own doc comment for the full derivation, including why
    // `board_yields::tech_upgrade`'s existing delta-over-incumbent credit is
    // NOT a duplicate of this). 0.0 by default, matching every other
    // unmeasured gate in this table.
    TechRedundancyDiscount,
}

/// Generates `WeightKey::ALL`, `WeightKey::name` and `WeightKey::
/// default_weight` from ONE list of `(Variant, "printed_name", default)`
/// triples, so the three can never disagree about which keys exist -- see
/// this module's top doc comment.
macro_rules! weight_key_table {
    ( $( $variant:ident => $name:literal, $default:expr );+ $(;)? ) => {
        impl WeightKey {
            /// Every key, in declaration order -- the one place all 139 are
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
    CorruptionHeadroom => "corruption_headroom", 0.25;
    ConsumptionHeadroom => "consumption_headroom", 0.25;
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
    // VERDICT (2026-08-06 audit, docs/RULES_SPEC.md 6.7 vs 8.x): unlike
    // `ma_left` below, `ca_left` needs NO saturating cap. Civil actions have
    // no end-of-turn conversion at all (§6.6 lists no "draw a card per
    // unspent CA" step -- they simply expire), so a naive reading of a
    // positive weight here looks like the bot being paid to hoard civil
    // actions it will never spend. It is not: `features()` reads
    // `p.civil_actions` on WHATEVER state a candidate move resolves to,
    // which for every move except `EndTurn` is still mid-turn -- a real
    // remaining CA there has real option value (something still worth doing
    // with it later this same turn) that a 1-ply search cannot see any
    // other way, and `board_yields::government_routes` prices a government
    // swap's CA delta the identical way (`Feature::CaLeft` as an immediate
    // in-turn pool change, never as an end-of-turn reward). The one state
    // where "remaining CA" is NOT mid-turn potential -- the trial built by
    // applying `Move::EndTurn` -- pre-loads `p.civil_actions` to the FRESH
    // allotment for the player's next turn (`economy::end_of_turn` step 5
    // runs before the search ever calls `evaluate` on that trial), not to
    // whatever was wasted; that refresh is exactly the asymmetry
    // `end_turn_bias` was measured against and is deliberately left alone
    // (see `eval.rs::choose`'s own "DO NOT fix this asymmetry" comment) --
    // `ca_left` inherits that same calibration rather than fighting it.
    // Net: no cliff for `ca_left` to saturate at, because there is no
    // card-draw conversion function to approximate in the first place.
    CaLeft => "ca_left", 0.05;
    // RULES_SPEC 6.7 / the summary line "Unspent MAs at end of turn each
    // draw 1 military card (max 3)": unlike `ca_left` above, a military
    // action DOES have an end-of-turn conversion, and that conversion is a
    // CLIFF, not a slope -- the 4th-and-later unused action draws nothing.
    // `features()` and `board_yields::government_routes` cap this
    // feature's value at `board_yields::MA_DRAW_CAP` (3.0) for exactly that
    // reason; see those call sites' own comments. `military_actions`
    // (0.7, above) is untouched by that cap -- it carries the general
    // standing value of having military actions (attacking, defending,
    // forming armies), which keeps scaling past 3 and has nothing to do
    // with the end-of-turn draw this weight prices.
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
    // Owning a unit is a CLIFF (RULES_SPEC 11.3), not a slope: colonizing
    // requires sacrificing at least one military unit "even if other
    // bonuses would cover the bid", so a player at zero units is dropped
    // from every aggression/colony auction before it ever reaches a
    // decision, and `unit_workers` (linear) cannot express that unit #1 is
    // worth far more than unit #5. Ported from the parked, never-merged
    // `origin/has-unit-ab` branch (commit 2713037) -- that branch seeded
    // this at 1.0 pending a 3p/4p no-harm A/B that never ran; this port
    // seeds 0.0 instead and lets the league price it rather than carrying
    // over an unmeasured guess (docs/OPEN_ITEMS.md).
    HasUnit => "has_unit", 0.0;
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
    // The military-deck sibling of the other four `board_credit_key` offsets
    // -- see `cards.rs::board_credit_key`'s own doc comment for why a
    // Military Bonus card (`defenseBonus`/`colonizationBonus`) needed one
    // too. 0.0, matching every other per-type offset's default.
    CardBoardBonus => "card_board_bonus", 0.0;
    HandSwapExtra => "hand_swap_extra", 0.0;
    CardRateCredit => "card_rate_credit", 1.0;
    UnitStrengthCredit => "unit_strength_credit", 0.0;
    UnitTechCredit => "unit_tech_credit", 1.0;
    TechBoardCredit => "tech_board_credit", 1.0;
    ActionBoardCredit => "action_board_credit", 1.0;
    GovBoardCredit => "gov_board_credit", 1.0;
    // Seeded at 0.0, unlike its tech/action/gov siblings above (which default
    // 1.0): those three were measured effective at 1.0 from the start, but
    // no equivalent measurement exists for wonders yet -- see this key's own
    // doc comment on `card_potential`'s wonder branch in `cards.rs`. 0.0 lets
    // the league climb it rather than inheriting a guessed value.
    WonderBoardCredit => "wonder_board_credit", 0.0;
    BuildFreshCredit => "build_fresh_credit", 0.0;
    RestrictedResourceCredit => "restricted_resource_credit", 1.0;
    FreeActionCredit => "free_action_credit", 0.0;
    TerritoryCredit => "territory_credit", 1.0;
    BonusCardCredit => "bonus_card_credit", 1.0;
    // Seeded nonzero (unlike `wonder_board_credit`'s 0.0): docs/OPEN_ITEMS.md
    // item 2's own warning is that a gated valuation path seeded at 0.0 stays
    // exactly as invisible as no path at all until some later climb
    // stumbles onto it -- these five are new estimates with no prior
    // measurement to seed AT, so 0.3 is a deliberately modest "trust this
    // some, not fully" starting point (not a fitted number, not 1.0's "this
    // was already measured effective" claim tech/action/gov's defaults
    // make) -- live from the first evaluation, refined by the league from
    // there rather than waiting to be found.
    TacticBoardCredit => "tactic_board_credit", 0.3;
    AggressionBoardCredit => "aggression_board_credit", 0.3;
    WarBoardCredit => "war_board_credit", 0.3;
    PactBoardCredit => "pact_board_credit", 0.3;
    EventBoardCredit => "event_board_credit", 0.3;
    TacticShortfallCost => "tactic_shortfall_cost", 0.1;
    // Seeded at the same modest 0.3 "trust this some, not fully" starting
    // point as `tactic_board_credit`/`aggression_board_credit`/etc above --
    // a new estimate with no prior measurement to seed at, not a fitted
    // number (see this key's own comment on its `WeightKey` declaration).
    TacticReachCredit => "tactic_reach_credit", 0.3;
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

    // standing-hinged pairs -- see `STANDING_KEYS` below for the honest
    // source of "which base keys get a hinge". Both gated at 0.0: landing
    // this must not move a single game until the climb prices it.
    CultureRateTrailing => "culture_rate_trailing", 0.0;
    ScienceRateTrailing => "science_rate_trailing", 0.0;

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

    // marginal-need gap/surplus coordinates -- see the enum declaration's
    // own comment above this block. All 0.0: unmeasured by construction, so
    // this is the same "trust nothing until the league finds it" default
    // every other 0.0-seeded key above already uses, not a fitted number.
    FoodGap => "food_gap", 0.0;
    FoodSurplus => "food_surplus", 0.0;
    ResourceGap => "resource_gap", 0.0;
    ResourceSurplus => "resource_surplus", 0.0;
    ScienceGap => "science_gap", 0.0;
    ScienceSurplus => "science_surplus", 0.0;
    CultureGap => "culture_gap", 0.0;
    CultureSurplus => "culture_surplus", 0.0;
    HappySurplus => "happy_surplus", 0.0;
    CivilActionGap => "civil_action_gap", 0.0;
    CivilActionSurplus => "civil_action_surplus", 0.0;
    MilitaryActionGap => "military_action_gap", 0.0;
    MilitaryActionSurplus => "military_action_surplus", 0.0;
    WorkerGap => "worker_gap", 0.0;
    WorkerSurplus => "worker_surplus", 0.0;

    TechRedundancyDiscount => "tech_redundancy_discount", 0.0;
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

    /// The standing-hinged partner of a [`STANDING_KEYS`] member, blended in
    /// by `rivals::feature_marginal` as `trailing_fraction * w[k_trailing]`.
    ///
    /// This is the answer to "one scalar per card TYPE cannot be right": a
    /// card's worth is its printed yields times these marginals, so once the
    /// marginal knows the player's POSITION, a science card is automatically
    /// worth more to a player behind on science than to the one leading it,
    /// with no per-card weight and no archetype label anywhere. The 1011-game
    /// BGO corpus is what says this is the right conditioning variable:
    /// culture gained is worth ~3x more to a trailing player than a leading
    /// one, and an Age I wonder is +15.9pp when behind on science but mildly
    /// NEGATIVE when leading.
    ///
    /// # Panics
    /// If `self` is not one of the [`STANDING_KEYS`] members -- same
    /// reasoning as [`Self::early`]: restricting the match to exactly those
    /// is what makes it impossible to silently read a hinge for a key that
    /// is not standing-multiplied.
    pub const fn trailing(self) -> WeightKey {
        match self {
            WeightKey::CultureRate => WeightKey::CultureRateTrailing,
            WeightKey::ScienceRate => WeightKey::ScienceRateTrailing,
            _ => panic!("WeightKey::trailing called on a key outside STANDING_KEYS"),
        }
    }

    /// The strategic axis this weight belongs to -- ports
    /// `experiments/summarize.py`'s `GROUPS`/`group_of` (verified key-for-key
    /// against that source in `tests::rust_grouping_agrees_with_python_groups`
    /// below). A `_early`/`_late` phase key is placed in the SAME arm as its
    /// base key, never a wildcard fallback to it, so the compiler -- not a
    /// convention -- is what keeps them together; see
    /// `tests::phase_key_shares_its_base_keys_group`.
    ///
    /// Deliberately NO `_ =>` wildcard arm: all 139 variants are named here
    /// by hand. Python's `group_of` raises `KeyError` rather than falling
    /// through to a "?" label for exactly this reason -- its own docstring
    /// records that a silent fallback is how four features (including
    /// `hand_potential`, the single most load-bearing 2p weight in the
    /// ablation ledger) vanished from every generated weight table before
    /// that guard existed. A wildcard arm here would be that same silent
    /// fallback, just moved from runtime to never-caught-at-all: a brand
    /// new `WeightKey` variant would compile straight into an (arbitrary,
    /// wrong) group instead of failing the build until a human decides
    /// where it belongs.
    pub const fn group(self) -> WeightGroup {
        use WeightKey::*;
        match self {
            CivilActions | MilitaryActions | CaLeft | MaLeft | TakeCostPaid
            // The civil/military-action marginal-need pair -- hand backlog
            // (cards queued to play) versus actions on hand to play them
            // with, the same axis `CivilActions`/`CaLeft` and
            // `MilitaryActions`/`MaLeft` already live in, not a new one.
            | CivilActionGap | CivilActionSurplus | MilitaryActionGap
            | MilitaryActionSurplus => {
                WeightGroup::Actions
            }

            UrbanLimit | GovActionCost | NoAggression | RestrictedResources
            | CardBoardCredit | CardBoardLeader | CardBoardGovernment | CardBoardAction
            | CardBoardWonder | CardBoardBonus => WeightGroup::Board,

            HandCivil | HandValue | HandValueEarly | HandValueLate | HandPotential
            | HandMilitary | HandMilValue | HandMilPotential | HandSwapExtra => {
                WeightGroup::Cards
            }

            RateHorizon | Culture | CultureRate | CultureRateTrailing | Science | ScienceRate
            | ScienceRateTrailing | FoodRate
            | ResourceRate | FoodStock | ResourceStock | BlueFree | CorruptionHeadroom
            | ConsumptionHeadroom | PopCost | YellowBank | FreeWorkers | Workers | WorkersEarly
            | WorkersLate | ProdWorkers | UrbanWorkers | UnitWorkers
            // The marginal-need gap/surplus pairs for food, resources,
            // science, culture and workers -- each stays in the SAME group
            // as the raw stock/rate it is a hinged version of (food/
            // resources/science/culture alongside `FoodStock`/`ResourceStock`/
            // `Science`/`Culture` above, workers alongside `FreeWorkers`/
            // `Workers`), never a new axis of its own -- see the enum
            // declaration's own doc comment on this block.
            // `CultureGap`/`CultureSurplus` land here, alongside `Culture`,
            // for the identical reason `StrengthDeficit`/`StrengthLead` stay
            // in `Military` rather than `Rivals` below even though their
            // "need" is rival-relative too: a derived comparison for MY axis
            // stays in that axis's own group -- `Rivals` is reserved for raw
            // rival board facts, not comparisons computed off them. See the
            // enum declaration's own doc comment on `CultureGap` for why
            // culture's need is competitive rather than an absolute
            // threshold.
            | FoodGap | FoodSurplus | ResourceGap | ResourceSurplus | ScienceGap
            | ScienceSurplus | CultureGap | CultureSurplus | WorkerGap | WorkerSurplus => {
                WeightGroup::Economy
            }

            EventScoringMargin | MySeededPending | MyEventThreat => WeightGroup::Events,

            // `HappySurplus` alongside `Discontent` (its gap half) and
            // `HappyMargin` -- the same axis, not a new one.
            HappyMargin | Discontent | Uprising | HappySurplus => WeightGroup::Happiness,

            Strength | StrengthRel | StrengthRelEarly | StrengthRelLate | StrengthDeficit
            | StrengthLead | TacticLevel | TacticGain | TacticShort | HasUnit | Colonies | Pacts
            | PactBlocksAttack | AuctionCommitted | AuctionBid => WeightGroup::Military,

            HandLimit | ColonizeBonus | BuildDiscount | FreeCivilAction | ResourceDiscount
            | DefenseBonus | CardRateCredit | UnitStrengthCredit | TerritoryCredit
            | BonusCardCredit | UnitTechCredit | TechBoardCredit | ActionBoardCredit
            | FreeActionCredit | GovBoardCredit | WonderBoardCredit | BuildFreshCredit
            | RestrictedResourceCredit | TacticBoardCredit | AggressionBoardCredit
            | WarBoardCredit | PactBoardCredit | EventBoardCredit | TacticShortfallCost
            | TacticReachCredit | TechRedundancyDiscount => WeightGroup::Priced,

            RivalCulture | RivalMeanCulture | RivalCultureRate | RivalScienceRate
            | RivalStrength | RivalFreeCa | RivalHandCivil | RivalWonders
            | RivalHandPotential | RivalScienceStock | RivalFoodStock | RivalResourceStock
            | RivalFreeWorkers | RivalYellowBank | RivalColonies | RivalMilActions
            | RivalBuildingWonder => WeightGroup::Rivals,

            RowUrgency | RowBargainForgone | RivalTakeShare | RowLastCopy | RivalDesire => {
                WeightGroup::Row
            }

            EndTurnBias => WeightGroup::Search,

            AttackTargetLead | AttackTargetWeakness | PactPartnerLead => WeightGroup::Targeting,

            TechLevels | TechLevelsEarly | TechLevelsLate | GovLevel | BestFarm | BestMine
            | BestLab | BestTemple | BestTheater | BestLibrary | BestArena | BestUnit
            | NumTechs | SpecialTechs => WeightGroup::Tech,

            Wonders | WonderProgress | WonderRemaining | WonderStagesLeft
            | WonderTurnsToFinish | WonderOverrun | WonderStagesPerAction | WonderPotential
            | Leader => WeightGroup::Wonders,
        }
    }
}

/// A coherent strategic axis of the weight vector -- e.g. every economy
/// coefficient, or every military one -- so a hill-climb mutation operator
/// can move a whole axis together instead of scattering onto unrelated
/// coefficients. Mirrors `experiments/summarize.py`'s `GROUPS` dict, which
/// `experiments/hillclimb.py` (`GROUP_KEYS`, lines 53-58) buckets every
/// mutable `DEFAULT_WEIGHTS` key into via `group_of(k).split("/")[0]` (the
/// `/phase` suffix `group_of` appends for a `_early`/`_late` key is an
/// annotation for `summarize.py`'s own printed output, not a second group --
/// `hillclimb.py` strips it with that same `.split("/")[0]`, which is why
/// [`WeightKey::group`] below folds a phase key straight into its base
/// key's group with no separate "/phase" concept at all).
///
/// Variant order matches [`WeightGroup::ALL`] (alphabetical by
/// [`WeightGroup::name`]); nothing depends on the order, it's just a
/// convention so a diff that adds a group is a one-line insert, not a
/// reshuffle.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum WeightGroup {
    Actions,
    Board,
    Cards,
    Economy,
    Events,
    Happiness,
    Military,
    Priced,
    Rivals,
    Row,
    Search,
    Targeting,
    Tech,
    Wonders,
}

impl WeightGroup {
    /// Every group, alphabetical by [`WeightGroup::name`] -- matches the
    /// enum declaration order above.
    pub const ALL: &'static [WeightGroup] = &[
        WeightGroup::Actions,
        WeightGroup::Board,
        WeightGroup::Cards,
        WeightGroup::Economy,
        WeightGroup::Events,
        WeightGroup::Happiness,
        WeightGroup::Military,
        WeightGroup::Priced,
        WeightGroup::Rivals,
        WeightGroup::Row,
        WeightGroup::Search,
        WeightGroup::Targeting,
        WeightGroup::Tech,
        WeightGroup::Wonders,
    ];

    /// The exact group name Python's `summarize.GROUPS` uses -- the I/O
    /// boundary for anything that prints or diffs against the Python
    /// tooling, and what the anti-drift test below checks against.
    pub const fn name(self) -> &'static str {
        match self {
            WeightGroup::Actions => "actions",
            WeightGroup::Board => "board",
            WeightGroup::Cards => "cards",
            WeightGroup::Economy => "economy",
            WeightGroup::Events => "events",
            WeightGroup::Happiness => "happiness",
            WeightGroup::Military => "military",
            WeightGroup::Priced => "priced",
            WeightGroup::Rivals => "rivals",
            WeightGroup::Row => "row",
            WeightGroup::Search => "search",
            WeightGroup::Targeting => "targeting",
            WeightGroup::Tech => "tech",
            WeightGroup::Wonders => "wonders",
        }
    }

    /// Every key in this group, base keys AND their `_early`/`_late` phase
    /// partners -- a group move is "care more about this axis at every
    /// age", not just at its default blend. Filters [`WeightKey::ALL`]
    /// through [`WeightKey::group`] rather than being kept as a second,
    /// parallel list of keys indexed by group: two lists that are supposed
    /// to agree are exactly the shape that silently drifts apart (see this
    /// file's own remarks on `PHASE_KEYS` vs the flat table for a case of
    /// that being unavoidable; here it isn't, so there's no excuse).
    pub fn keys(self) -> Vec<WeightKey> {
        WeightKey::ALL.iter().copied().filter(|k| k.group() == self).collect()
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

/// The base keys that carry a standing hinge -- the honest source of "which
/// keys get a `_trailing` partner", exactly as [`PHASE_KEYS`] is for
/// `_early`/`_late`. Culture and science first because those are the two
/// the human corpus is clearest on: relative standing in them is what wins
/// games, and the effect GROWS with the age (2p culture rank is worth
/// +7.4pp at the end of Age I and +39.8pp at the end of Age III).
pub const STANDING_KEYS: &[WeightKey] = &[WeightKey::CultureRate, WeightKey::ScienceRate];

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
    // Retired 2026-08-09: both are EXACT rulebook step tables
    // (`economy::corruption`, `economy::consumption`) computed from state the
    // evaluator already holds, so they are netted straight into `FoodRate`
    // and `ResourceRate` by `features()` and there is nothing left for a
    // league to price. Retired rather than pinned to zero deliberately: a
    // live `WeightKey` variant is indexable, and anything indexable can be
    // mutated back off the rulebook's answer. Removing the variant is what
    // makes that unwritable.
    "corruption_loss",
    "consumption",
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
    /// invariant the (since-retired) differential test
    /// `rust/tests/weighted_horizon.rs` relied on to compare Rust's
    /// defaults against Python's by name.
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

    /// Every key belongs to exactly one group's `keys()` -- not zero (which
    /// would mean `WeightKey::group` and `WeightGroup::keys` disagree, since
    /// `keys()` is filtered straight off `group()`, so this is really a
    /// sanity check on the filter) and not more than one, which is only
    /// possible if `WeightGroup::ALL` itself has a duplicate entry.
    #[test]
    fn every_weight_key_lands_in_exactly_one_groups_keys() {
        for &k in WeightKey::ALL {
            let hits: Vec<WeightGroup> =
                WeightGroup::ALL.iter().copied().filter(|g| g.keys().contains(&k)).collect();
            assert_eq!(hits.len(), 1, "{}: expected exactly 1 group, got {:?}", k.name(), hits);
        }
    }

    /// A group with no keys would be dead weight in `WeightGroup::ALL` -- a
    /// mutation operator that picked it could never do anything.
    #[test]
    fn every_weight_group_has_a_non_empty_keys_list() {
        for &g in WeightGroup::ALL {
            assert!(!g.keys().is_empty(), "{}", g.name());
        }
    }

    #[test]
    fn weight_group_name_round_trips_through_all() {
        for &g in WeightGroup::ALL {
            assert!(WeightGroup::ALL.iter().any(|&g2| g2.name() == g.name() && g2 == g));
        }
        // no two groups share a printed name
        let names: HashSet<&str> = WeightGroup::ALL.iter().map(|g| g.name()).collect();
        assert_eq!(names.len(), WeightGroup::ALL.len());
    }

    /// A `_early`/`_late` phase key is in the SAME group as its base key,
    /// for all four `PHASE_KEYS` -- the property `hillclimb.py`'s
    /// `GROUP_KEYS` comment relies on ("a group move is 'care more/less
    /// about this whole strategic axis', at every age").
    #[test]
    fn phase_key_shares_its_base_keys_group() {
        for &k in PHASE_KEYS {
            assert_eq!(k.early().group(), k.group(), "{}_early", k.name());
            assert_eq!(k.late().group(), k.group(), "{}_late", k.name());
        }
    }

    /// Anti-drift check: hardcodes a handful of (weight name, group name)
    /// pairs read directly out of `experiments/summarize.py`'s `GROUPS`
    /// dict -- at least one per group -- so that a future edit to either
    /// side's grouping (Python's `GROUPS` or Rust's `WeightKey::group`)
    /// that isn't mirrored in the other fails a test instead of silently
    /// making the hill climber's Rust and Python mutation operators bucket
    /// the same weight two different ways.
    #[test]
    fn rust_grouping_agrees_with_python_groups() {
        let pairs: &[(&str, &str)] = &[
            ("civil_actions", "actions"),                 // GROUPS["actions"]
            ("card_board_wonder", "board"),                // GROUPS["board"]
            ("hand_mil_value", "cards"),                   // GROUPS["cards"]
            ("rate_horizon", "economy"),                   // GROUPS["economy"]
            ("my_event_threat", "events"),                 // GROUPS["events"]
            ("uprising", "happiness"),                     // GROUPS["happiness"]
            ("tactic_short", "military"),                  // GROUPS["military"]
            ("restricted_resource_credit", "priced"),      // GROUPS["priced"]
            ("rival_building_wonder", "rivals"),           // GROUPS["rivals"]
            ("rival_desire", "row"),                       // GROUPS["row"]
            ("end_turn_bias", "search"),                   // GROUPS["search"]
            ("pact_partner_lead", "targeting"),            // GROUPS["targeting"]
            ("special_techs", "tech"),                     // GROUPS["tech"]
            ("wonder_stages_per_action", "wonders"),       // GROUPS["wonders"]
        ];
        for &(name, group) in pairs {
            let k = WeightKey::by_name(name).unwrap_or_else(|| panic!("no WeightKey {name}"));
            assert_eq!(k.group().name(), group, "{name}");
        }
    }
}
