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
//! rather than writing the keys out by hand, specifically so `PHASE_KEYS`
//! and the phase table can never name a different set of features (see that
//! constant's own comment: "a pair for a key that is no longer
//! phase-multiplied would be a weight the trainer mutates, the guard checks
//! and `evaluate` never reads"). The flat `weight_keys!` table below still
//! needs literal entries for the `*_early`/`*_late` variants -- a
//! `macro_rules!` cannot synthesize a new identifier by string concatenation
//! without a proc-macro, and this crate deliberately has none (`Cargo.toml`'s
//! `[dependencies]` is empty on purpose) -- so the "can never disagree"
//! guarantee is reconstructed a different way: [`PHASE_KEYS`] lists only the
//! four BASE keys, and [`WeightKey::early`]/[`WeightKey::late`] are the ONLY
//! way to reach a phase partner -- both `match` on exactly the keys that
//! still have one and panic on any other input, so a caller can never
//! silently read a phase pair for a key that is not one. `tests::
//! phase_keys_and_the_flat_table_agree` below checks the converse direction:
//! every `*_early`/`*_late` entry that IS in the flat table is reachable from
//! `PHASE_KEYS`, and nothing else is. The two (the macro table and the
//! `early`/`late` match arms) can now drift only if both are edited to agree
//! on the same new lie, which is a narrower window than Python's single
//! dict comprehension leaves open, but it is the honest cost of this crate
//! having no macro-generated identifiers.
//!
//! PHASECUT.txt (2026-08-13, T1-A/C/D) collapsed three of the four
//! `PHASE_KEYS` triples (`Workers`/`TechLevels`/`HandValue`) from the old
//! 3-parameter `{base, early, late}` shape to a non-redundant 2-parameter
//! `{start, end}` one -- the old blend had only 2 real degrees of freedom
//! for 3 raw numbers, a proven, exact, data-independent dead direction. Only
//! `StrengthRel` still has a real `_early` key (excluded from the collapse
//! -- a parallel fix makes its triple genuinely identifiable via a
//! round-gated blend, so collapsing it would delete a distinction that fix
//! depends on; see [`WeightKey::early`]'s own doc comment). [`WeightKey::
//! late`] still resolves for all four -- see that method's own doc comment
//! for what it means for each.
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
    HasColony,
    Pacts,
    PactBlocksAttack,
    WarImmune,
    AttackCostDoubled,
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
    /// The half of an unfinished wonder's value that is still AHEAD of the
    /// player, priced by `cards::wonder_promise` through
    /// [`horizon::WonderOutlook::promise_share`](super::horizon::WonderOutlook::promise_share).
    ///
    /// [`WonderPotential`](Self::WonderPotential) multiplies a wonder's
    /// printed effects by `paid_fraction * collect_fraction`, and
    /// `paid_fraction` is exactly `0.0` on the move that TAKES a wonder off
    /// the row -- so the only identity-aware wonder channel the evaluator has
    /// is switched off at precisely the moment it has to choose one. Two
    /// wonders with the same stage multiset (Pyramids `[3,2,1]` and Hanging
    /// Gardens `[2,2,2]`, both 6 resources over 3 stages) are therefore
    /// interchangeable to every weight in this table, whatever the weights
    /// are; `cards::tests::two_wonders_with_different_powers_score_
    /// identically_at_the_moment_they_are_taken` is that claim as a test.
    ///
    /// This key closes it with NO per-card free parameter: what it scales is
    /// the same `board_yields` swap diff `wonder_potential` scales, summed
    /// through the same shared weights, so Pyramids beats Hanging Gardens by
    /// `w[civil_actions] * 1` versus a happy face -- four numbers cover all
    /// 16 wonders and generalise to one the league has never seen. Gated at
    /// 0.0 so landing it moves no game until the league prices it, and
    /// `eval::DOMINATES` keeps it at or below `wonder_potential` so that
    /// PAYING a stage stays strictly rewarded (value moves out of this term
    /// and into that one as `paid_fraction` rises).
    WonderPromise,
    /// `max(0, turns_to_finish - rounds_to_antiquation)` -- the part of the
    /// wonder's outstanding cost that falls past the deadline THE RULES
    /// impose (RULES_SPEC 12.2: an unfinished wonder older than the age that
    /// just ended is removed from play) rather than past the end of the game.
    ///
    /// [`WonderOverrun`](Self::WonderOverrun) measures the same shortfall
    /// against `horizon::rounds_left`, which for an Age A wonder taken in Age
    /// A overstates the real deadline by two whole ages. 66.5% of every wonder
    /// started in the 200-game 2p census died to the deadline this key is the
    /// first coordinate to see. Added as a NEW key rather than as a correction
    /// to `wonder_overrun` so that no champion on disk evaluates differently
    /// the day this lands.
    WonderAgeOverrun,
    Leader,
    WonderInProgress,
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
    // `CardBoardGovernment`/`CardBoardAction`/`CardBoardWonder` retired
    // 2026-08-13 -- see `RETIRED_KEYS`'s own entry for why.
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
    /// How much of the civil hand is about to expire:
    /// `Σ_{c in hand_civil} clamp01(1 - rounds_to_antiquation(c) / rounds_left)`.
    ///
    /// `0.0` for a hand of fresh cards, and → 1.0 per card for one the next
    /// age boundary is about to discard (RULES_SPEC 12.2, `game::antiquate`
    /// culls hands as well as the board). [`HandCivil`](Self::HandCivil)
    /// counts the cards and [`HandValue`](Self::HandValue)/
    /// [`HandPotential`](Self::HandPotential) price them, but all three are
    /// blind to remaining useful LIFETIME: an Age A card held into Age I is
    /// worth what it is worth only if it gets played first. One coordinate
    /// covers the whole hand and every card in it -- no per-card parameter,
    /// because `rounds_to_antiquation` is arithmetic on the deck and is
    /// identical for every card of the same age.
    HandPerishable,
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
    // `WorkersEarly`/`TechLevelsEarly`/`HandValueEarly` retired 2026-08-13
    // (PHASECUT.txt, T1-A/C/D): the OLD three-parameter blend `w[base] +
    // (1-L)*w[early] + L*w[late]` has only two degrees of freedom (its
    // value at L=0 and L=1), so the direction (base,early,late) +=
    // t*(1,-1,-1) changed nothing `evaluate` ever computed -- a proven,
    // exact, data-independent dead parameter for these three keys (NOT
    // `StrengthRel`, whose triple a parallel fix, commit 578ee9e
    // "earlymil", makes genuinely identifiable via a round-gated blend --
    // see PHASECUT.txt's scope note for the full argument). `Workers`/
    // `TechLevels`/`HandValue` themselves are repurposed to carry the FULL
    // early-extreme ("start") coefficient (`old_base + old_early`), and
    // `WorkersLate`/`TechLevelsLate`/`HandValueLate` now carry the FULL
    // late-extreme ("end") coefficient (`old_base + old_late`), blended as
    // `start*(1-L) + end*L` -- the identical 2-df curve, spanned by a basis
    // with no redundant direction. See `eval::load_weights`/`parse_weights`
    // for the exact, lossless, load-time conversion of every file on disk.
    WorkersLate,
    StrengthRelEarly,
    StrengthRelLate,
    TechLevelsLate,
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
    /// `ca_spent_taking / costs::ca_total` -- the share of this turn's WHOLE
    /// civil-action allowance that reaching into the row consumed.
    ///
    /// [`TakeCostPaid`](Self::TakeCostPaid) (the numerator) and
    /// [`CaLeft`](Self::CaLeft) already price the row's 1/2/3-action cost
    /// bands, but only in absolute terms: on the live 2p champion the gap
    /// between the cheapest and the dearest slot is worth 0.89 evaluation
    /// points unconditionally, the same 0.89 to a player with 4 civil actions
    /// as to one with 7. A QUOTIENT is deliberately not in the linear span of
    /// its own numerator and denominator, which is the entire point of this
    /// coordinate: it can say "3 of my 4" differs from "3 of my 7" where no
    /// assignment of the two existing weights can. That threshold is one
    /// experts state outright (`docs/EXPERT_STRATEGY.md:526`: "I don't use 3
    /// civil actions until I have 5 or 6").
    ///
    /// A quotient rather than a second LEVEL, and the live 2p champion is why
    /// that distinction is load-bearing rather than stylistic. It prices
    /// `civil_actions` at -0.520 and `civil_action_surplus` at -1.324 -- an
    /// unspent civil action reads as a PENALTY, so spending more of them
    /// scores better. Confirmed by a swap test on a recorded position, not
    /// inferred: with Pyramids in a 3-action slot and Hanging Gardens in a
    /// 1-action slot it takes Pyramids and rates Hanging Gardens 13.6 worse,
    /// and swapping the two cards between those slots flips the choice. It is
    /// choosing a PRICE, not a card -- which it can only do because the two
    /// wonders score bit-identically at take time, the zero row
    /// [`WonderPromise`](Self::WonderPromise) closes. A second LEVEL would sit
    /// inside the span of the coordinates that produced that behaviour and
    /// could be absorbed by re-pricing them; a ratio cannot. Repairing the
    /// SIGN is a pathology of a vector the climb produced rather than a gap in
    /// this basis, and is deliberately not attempted here.
    ///
    /// Deliberately ungated in `eval`'s dominance tables, matching
    /// `take_cost_paid`: spending actions on a card worth having is not a
    /// rules-level loss, so its sign is the league's to find.
    TakeCostShare,
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
    // T1-A collapse (PHASECUT.txt, 2026-08-13): was `1.4` (the old
    // always-on flat term); now the FOLDED early-extreme ("start")
    // coefficient `old_base(1.4) + old_workers_early(0.8) = 2.2`, so a
    // freshly-authored vector scores every existing champion's L=0
    // position identically to before the collapse. See `eval::
    // parse_weights` for the load-time conversion every file on disk gets.
    Workers => "workers", 2.2;
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
    // RULES_SPEC 5.4 cliff, not a slope -- `legal.rs`'s
    // `aggression_target_qualifies` gates Annex on `q.colonies.is_empty()`,
    // the printed target clause "one opponent who owns at least one
    // colony" [RULES_SPEC.md:123]. `Colonies` (the raw count above) already
    // carries every colony's production/culture/science/strength benefit
    // through the other priced coordinates it feeds, plus exposure to the
    // "Impact of Colonies" event via `EventScoringMargin` -- what neither
    // of those can express is that going from 0 to 1 colonies alone makes
    // you a legal Annex target, a step function no linear count can price.
    // Seeded at 0.0, same as `HasUnit`, and left `Free` in `sign_intent`
    // below: the engine states the rulebook fact, the league prices it.
    HasColony => "has_colony", 0.0;
    Pacts => "pacts", 0.5;
    PactBlocksAttack => "pact_blocks_attack", 0.5;
    // RULES_SPEC 5.6 cliff, not a slope -- "declare a war ... illegal if a
    // pact forbids it" [RULES_SPEC.md:131] covers `pact_forbids_attack`
    // (already priced by `PactBlocksAttack` above), but `combat::
    // war_forbidden` ORs in a second, independent gate:
    // `effects::state_stats(.., defender).war_immune`, set by a pact side
    // printing `cannotBeDeclaredWarOnByAnyone` (`Special::B`/`A`/
    // `BothPlayers`'s `PactBlock.war_immune`, e.g. "Loss of Sovereignty"'s B
    // side). `cards.rs`'s `pact_value` and `rivals.rs`'s `add_pact_block`
    // both name `war_immune` explicitly as one of the `PactBlock` fields
    // deliberately left OUT of the RATE-shaped decomposition (not a
    // per-turn yield, a boolean gate on legality) -- this is that gate's
    // home. Holding it makes War against you categorically illegal, not
    // merely less attractive, the same all-or-nothing shape `HasColony`/
    // `HasUnit` already price elsewhere.
    WarImmune => "war_immune", 0.0;
    // A cost MULTIPLIER cliff, not a slope: `legal.rs`'s `action_moves`
    // (Aggression and War branches alike) and `combat::start_aggression`
    // all read `leader_is(q, "Mahatma Gandhi")` to double the military-
    // action cost an OPPONENT pays to attack `q` -- the printed leader
    // effect `Special::OpponentsPayDoubleMilitaryActionsToAttackYou`
    // (`card_table.rs`), currently the only card carrying it. Computed
    // here off the SPECIAL on the player's own leader card (mirroring the
    // rules fact, not the engine's hardcoded name check) so a future card
    // sharing the special is priced automatically. Distinct from
    // `NoAggression` (`Special::CannotPlayAggressionOrWar`, same leader):
    // that flag restricts what ITS OWN holder may play; this one restricts
    // what OPPONENTS may afford to do to the holder -- the opposite
    // direction, and the one this audit exists to catch.
    AttackCostDoubled => "attack_cost_doubled", 0.0;
    AuctionCommitted => "auction_committed", 2.0;
    AuctionBid => "auction_bid", -0.4;
    // T1-C collapse (PHASECUT.txt): folded start = 1.0 + 0.5 = 1.5. See
    // `Workers`'s own comment above for the full reasoning.
    TechLevels => "tech_levels", 1.5;
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
    // Both 0.0, and both have to be: a champion JSON written before these
    // keys existed keeps its default for them (`eval::parse_weights` starts
    // from `Weights::defaults()`), so any other seed would silently move
    // every frozen gauntlet member and the anchor itself. See
    // `tests::a_champion_file_saved_before_these_keys_existed_still_loads_
    // with_them_at_zero`.
    WonderPromise => "wonder_promise", 0.0;
    WonderAgeOverrun => "wonder_age_overrun", 0.0;
    Leader => "leader", 1.5;
    // RULES_SPEC 5.5 cliff, not a slope -- "Infiltrate 2 (remove rival's
    // leader or unfinished wonder from game...)" [RULES_SPEC.md:129].
    // `legal.rs`'s `aggression_target_qualifies` gates Infiltrate
    // (`Special::RemoveFromGame`) on `q.leader.is_none() && q.wonder.is_none()`
    // -- `Leader` (above) already prices the leader half of that OR, but
    // nothing priced the wonder half: `WonderProgress` is a RATE-shaped
    // sunk-cost magnitude tuned to value how much a wonder is worth
    // finishing, not whether starting one alone made its owner Infiltrate-
    // able, the same conflation `Colonies` alone left for `HasColony`
    // (going from no wonder to the first resource invested is a step no
    // linear progress count can express). Seeded at 0.0 and left `Free`,
    // same as `HasColony`.
    WonderInProgress => "wonder_in_progress", 0.0;
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
    // `CardBoardLeader`'s sibling for the OTHER type with no dedicated
    // board-aware pricing function of its own -- see `cards.rs::
    // board_credit_key`'s own doc comment for why a Military Bonus card
    // (`defenseBonus`/`colonizationBonus`) needed one. 0.0, matching
    // `CardBoardLeader`'s own default. (Government/Action/Wonder used to
    // have the identical per-type shape here; all three were retired
    // 2026-08-13 -- their dedicated `gov_board_credit`/`action_board_credit`/
    // `wonder_board_credit` branches in `card_potential_core` unconditionally
    // intercept before this per-type path is ever reached whenever nonzero,
    // which is every trained champion sampled, so the per-type key was a
    // live-looking knob wired to nothing. See `RETIRED_KEYS`.)
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
    // T1-D collapse (PHASECUT.txt): folded start = 0.25 + 0.2 = 0.45. See
    // `Workers`'s own comment above for the full reasoning. Also now
    // classified `SignIntent::NonNegative` (was `Free`, guarded instead by
    // the composite `NET_NONNEG_PHASE` mechanism) -- see `sign_intent`'s
    // own `HandValue` arm below.
    HandValue => "hand_value", 0.45;
    HandPotential => "hand_potential", 0.125;
    HandMilitary => "hand_military", 0.3;
    HandMilValue => "hand_mil_value", 0.15;
    HandMilPotential => "hand_mil_potential", 0.0;
    HandPerishable => "hand_perishable", 0.0;
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
    // these five are still spelled out by hand here even though PHASE_KEYS
    // below is the honest source of "which base keys get a pair". Only
    // `StrengthRel` still has a separate `_early` key -- `Workers`/
    // `TechLevels`/`HandValue`'s `_early` keys were retired 2026-08-13
    // (PHASECUT.txt, T1-A/C/D collapse); their base key above now carries
    // the folded early-extreme default (`old_base + old_early`) and their
    // `_late` key below carries the folded late-extreme default
    // (`old_base + old_late`) -- see each base key's own default for the
    // arithmetic (workers: 1.4+0.8=2.2, tech_levels: 1.0+0.5=1.5,
    // hand_value: 0.25+0.2=0.45) and PHASECUT.txt for the full derivation.
    WorkersLate => "workers_late", 0.8; // 1.4 + (-0.6), the old base+late
    StrengthRelEarly => "strength_rel_early", -0.1;
    StrengthRelLate => "strength_rel_late", 0.5;
    TechLevelsLate => "tech_levels_late", 0.6; // 1.0 + (-0.4)
    HandValueLate => "hand_value_late", 0.05; // 0.25 + (-0.2)

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
    // 0.0 for the same reason `wonder_promise`/`wonder_age_overrun` are --
    // an old champion file inherits whatever is written here.
    TakeCostShare => "take_cost_share", 0.0;
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

    /// The early-phase partner of the ONE remaining [`PHASE_KEYS`] member
    /// that still has one -- `w[k + "_early"]` in Python, blended into
    /// `evaluate` as `(1 - lateness) * w[k_early]`.
    ///
    /// PHASECUT.txt (2026-08-13, T1-A/C/D): `Workers`/`TechLevels`/
    /// `HandValue` USED to have an `_early` partner too, but their triple
    /// was a proven, exact, data-independent redundancy (the old blend
    /// `w[base] + (1-L)*w[early] + L*w[late]` has only 2 degrees of
    /// freedom for 3 raw numbers) -- collapsed to a 2-parameter
    /// `{start, end}` basis, so those three keys no longer have a
    /// separate early partner AT ALL (`self` now IS the early-extreme
    /// value). `StrengthRel` is the sole survivor of the old 3-parameter
    /// shape: a PARALLEL fix (commit 578ee9e, "earlymil") replaced its
    /// blend with a round-gated piecewise formula that makes the triple
    /// genuinely identifiable again (not the flat line the collapse
    /// argument depends on) -- collapsing it too would delete the exact
    /// distinction that fix relies on. See PHASECUT.txt for the full
    /// argument in both directions.
    ///
    /// # Panics
    /// If `self` is not [`WeightKey::StrengthRel`] -- restricting the match
    /// to exactly that one key (rather than falling back to `self` or to
    /// `0.0`) is what makes it impossible to silently read a phase pair for
    /// a key that is not phase-multiplied this way.
    pub const fn early(self) -> WeightKey {
        match self {
            WeightKey::StrengthRel => WeightKey::StrengthRelEarly,
            WeightKey::RateHorizon | WeightKey::Culture | WeightKey::CultureRate | WeightKey::Science | WeightKey::ScienceRate | WeightKey::FoodRate | WeightKey::ResourceRate | WeightKey::FoodStock | WeightKey::ResourceStock | WeightKey::BlueFree | WeightKey::CorruptionHeadroom | WeightKey::ConsumptionHeadroom | WeightKey::PopCost | WeightKey::YellowBank | WeightKey::FreeWorkers | WeightKey::Workers | WeightKey::ProdWorkers | WeightKey::UrbanWorkers | WeightKey::UnitWorkers | WeightKey::HappyMargin | WeightKey::Discontent | WeightKey::Uprising | WeightKey::CivilActions | WeightKey::MilitaryActions | WeightKey::CaLeft | WeightKey::MaLeft | WeightKey::TakeCostPaid | WeightKey::RowUrgency | WeightKey::RowBargainForgone | WeightKey::RowLastCopy | WeightKey::RivalDesire | WeightKey::RivalTakeShare | WeightKey::RivalFreeCa | WeightKey::RivalHandCivil | WeightKey::RivalWonders | WeightKey::RivalHandPotential | WeightKey::RivalScienceStock | WeightKey::RivalFoodStock | WeightKey::RivalResourceStock | WeightKey::RivalFreeWorkers | WeightKey::RivalYellowBank | WeightKey::RivalColonies | WeightKey::RivalMilActions | WeightKey::RivalBuildingWonder | WeightKey::MySeededPending | WeightKey::MyEventThreat | WeightKey::AttackTargetLead | WeightKey::AttackTargetWeakness | WeightKey::PactPartnerLead | WeightKey::Strength | WeightKey::StrengthDeficit | WeightKey::StrengthLead | WeightKey::TacticLevel | WeightKey::TacticGain | WeightKey::TacticShort | WeightKey::HasUnit | WeightKey::Colonies | WeightKey::HasColony | WeightKey::Pacts | WeightKey::PactBlocksAttack | WeightKey::WarImmune | WeightKey::AttackCostDoubled | WeightKey::AuctionCommitted | WeightKey::AuctionBid | WeightKey::TechLevels | WeightKey::GovLevel | WeightKey::BestFarm | WeightKey::BestMine | WeightKey::BestLab | WeightKey::BestTemple | WeightKey::BestTheater | WeightKey::BestLibrary | WeightKey::BestArena | WeightKey::BestUnit | WeightKey::NumTechs | WeightKey::SpecialTechs | WeightKey::Wonders | WeightKey::WonderProgress | WeightKey::WonderRemaining | WeightKey::WonderStagesLeft | WeightKey::WonderTurnsToFinish | WeightKey::WonderOverrun | WeightKey::WonderStagesPerAction | WeightKey::WonderPotential | WeightKey::WonderPromise | WeightKey::WonderAgeOverrun | WeightKey::Leader | WeightKey::WonderInProgress | WeightKey::HandLimit | WeightKey::ColonizeBonus | WeightKey::BuildDiscount | WeightKey::FreeCivilAction | WeightKey::ResourceDiscount | WeightKey::DefenseBonus | WeightKey::UrbanLimit | WeightKey::GovActionCost | WeightKey::NoAggression | WeightKey::RestrictedResources | WeightKey::CardBoardCredit | WeightKey::EventScoringMargin | WeightKey::CardBoardLeader | WeightKey::CardBoardBonus | WeightKey::HandSwapExtra | WeightKey::CardRateCredit | WeightKey::UnitStrengthCredit | WeightKey::UnitTechCredit | WeightKey::TechBoardCredit | WeightKey::ActionBoardCredit | WeightKey::GovBoardCredit | WeightKey::WonderBoardCredit | WeightKey::BuildFreshCredit | WeightKey::RestrictedResourceCredit | WeightKey::FreeActionCredit | WeightKey::TerritoryCredit | WeightKey::BonusCardCredit | WeightKey::TacticBoardCredit | WeightKey::AggressionBoardCredit | WeightKey::WarBoardCredit | WeightKey::PactBoardCredit | WeightKey::EventBoardCredit | WeightKey::TacticShortfallCost | WeightKey::TacticReachCredit | WeightKey::HandCivil | WeightKey::HandValue | WeightKey::HandPotential | WeightKey::HandMilitary | WeightKey::HandMilValue | WeightKey::HandMilPotential | WeightKey::HandPerishable | WeightKey::RivalCulture | WeightKey::RivalMeanCulture | WeightKey::RivalCultureRate | WeightKey::RivalScienceRate | WeightKey::RivalStrength | WeightKey::EndTurnBias | WeightKey::CultureRateTrailing | WeightKey::ScienceRateTrailing | WeightKey::WorkersLate | WeightKey::StrengthRelEarly | WeightKey::StrengthRelLate | WeightKey::TechLevelsLate | WeightKey::HandValueLate | WeightKey::FoodGap | WeightKey::FoodSurplus | WeightKey::ResourceGap | WeightKey::ResourceSurplus | WeightKey::ScienceGap | WeightKey::ScienceSurplus | WeightKey::CultureGap | WeightKey::CultureSurplus | WeightKey::HappySurplus | WeightKey::CivilActionGap | WeightKey::CivilActionSurplus | WeightKey::TakeCostShare | WeightKey::MilitaryActionGap | WeightKey::MilitaryActionSurplus | WeightKey::WorkerGap | WeightKey::WorkerSurplus | WeightKey::TechRedundancyDiscount => panic!("WeightKey::early called on a key with no _early partner (only StrengthRel has one post PHASECUT.txt's T1-A/C/D collapse)"),
        }
    }

    /// The late-phase partner of a [`PHASE_KEYS`] member -- for `Workers`/
    /// `TechLevels`/`HandValue` this is the "end" (late-extreme) half of
    /// the collapsed `{start, end}` basis (PHASECUT.txt); for `StrengthRel`
    /// it keeps its original, untouched meaning. See [`Self::early`].
    ///
    /// # Panics
    /// If `self` is not one of the four [`PHASE_KEYS`] members.
    pub const fn late(self) -> WeightKey {
        match self {
            WeightKey::Workers => WeightKey::WorkersLate,
            WeightKey::StrengthRel => WeightKey::StrengthRelLate,
            WeightKey::TechLevels => WeightKey::TechLevelsLate,
            WeightKey::HandValue => WeightKey::HandValueLate,
            WeightKey::RateHorizon | WeightKey::Culture | WeightKey::CultureRate | WeightKey::Science | WeightKey::ScienceRate | WeightKey::FoodRate | WeightKey::ResourceRate | WeightKey::FoodStock | WeightKey::ResourceStock | WeightKey::BlueFree | WeightKey::CorruptionHeadroom | WeightKey::ConsumptionHeadroom | WeightKey::PopCost | WeightKey::YellowBank | WeightKey::FreeWorkers | WeightKey::ProdWorkers | WeightKey::UrbanWorkers | WeightKey::UnitWorkers | WeightKey::HappyMargin | WeightKey::Discontent | WeightKey::Uprising | WeightKey::CivilActions | WeightKey::MilitaryActions | WeightKey::CaLeft | WeightKey::MaLeft | WeightKey::TakeCostPaid | WeightKey::RowUrgency | WeightKey::RowBargainForgone | WeightKey::RowLastCopy | WeightKey::RivalDesire | WeightKey::RivalTakeShare | WeightKey::RivalFreeCa | WeightKey::RivalHandCivil | WeightKey::RivalWonders | WeightKey::RivalHandPotential | WeightKey::RivalScienceStock | WeightKey::RivalFoodStock | WeightKey::RivalResourceStock | WeightKey::RivalFreeWorkers | WeightKey::RivalYellowBank | WeightKey::RivalColonies | WeightKey::RivalMilActions | WeightKey::RivalBuildingWonder | WeightKey::MySeededPending | WeightKey::MyEventThreat | WeightKey::AttackTargetLead | WeightKey::AttackTargetWeakness | WeightKey::PactPartnerLead | WeightKey::Strength | WeightKey::StrengthDeficit | WeightKey::StrengthLead | WeightKey::TacticLevel | WeightKey::TacticGain | WeightKey::TacticShort | WeightKey::HasUnit | WeightKey::Colonies | WeightKey::HasColony | WeightKey::Pacts | WeightKey::PactBlocksAttack | WeightKey::WarImmune | WeightKey::AttackCostDoubled | WeightKey::AuctionCommitted | WeightKey::AuctionBid | WeightKey::GovLevel | WeightKey::BestFarm | WeightKey::BestMine | WeightKey::BestLab | WeightKey::BestTemple | WeightKey::BestTheater | WeightKey::BestLibrary | WeightKey::BestArena | WeightKey::BestUnit | WeightKey::NumTechs | WeightKey::SpecialTechs | WeightKey::Wonders | WeightKey::WonderProgress | WeightKey::WonderRemaining | WeightKey::WonderStagesLeft | WeightKey::WonderTurnsToFinish | WeightKey::WonderOverrun | WeightKey::WonderStagesPerAction | WeightKey::WonderPotential | WeightKey::WonderPromise | WeightKey::WonderAgeOverrun | WeightKey::Leader | WeightKey::WonderInProgress | WeightKey::HandLimit | WeightKey::ColonizeBonus | WeightKey::BuildDiscount | WeightKey::FreeCivilAction | WeightKey::ResourceDiscount | WeightKey::DefenseBonus | WeightKey::UrbanLimit | WeightKey::GovActionCost | WeightKey::NoAggression | WeightKey::RestrictedResources | WeightKey::CardBoardCredit | WeightKey::EventScoringMargin | WeightKey::CardBoardLeader | WeightKey::CardBoardBonus | WeightKey::HandSwapExtra | WeightKey::CardRateCredit | WeightKey::UnitStrengthCredit | WeightKey::UnitTechCredit | WeightKey::TechBoardCredit | WeightKey::ActionBoardCredit | WeightKey::GovBoardCredit | WeightKey::WonderBoardCredit | WeightKey::BuildFreshCredit | WeightKey::RestrictedResourceCredit | WeightKey::FreeActionCredit | WeightKey::TerritoryCredit | WeightKey::BonusCardCredit | WeightKey::TacticBoardCredit | WeightKey::AggressionBoardCredit | WeightKey::WarBoardCredit | WeightKey::PactBoardCredit | WeightKey::EventBoardCredit | WeightKey::TacticShortfallCost | WeightKey::TacticReachCredit | WeightKey::HandCivil | WeightKey::HandPotential | WeightKey::HandMilitary | WeightKey::HandMilValue | WeightKey::HandMilPotential | WeightKey::HandPerishable | WeightKey::RivalCulture | WeightKey::RivalMeanCulture | WeightKey::RivalCultureRate | WeightKey::RivalScienceRate | WeightKey::RivalStrength | WeightKey::EndTurnBias | WeightKey::CultureRateTrailing | WeightKey::ScienceRateTrailing | WeightKey::WorkersLate | WeightKey::StrengthRelEarly | WeightKey::StrengthRelLate | WeightKey::TechLevelsLate | WeightKey::HandValueLate | WeightKey::FoodGap | WeightKey::FoodSurplus | WeightKey::ResourceGap | WeightKey::ResourceSurplus | WeightKey::ScienceGap | WeightKey::ScienceSurplus | WeightKey::CultureGap | WeightKey::CultureSurplus | WeightKey::HappySurplus | WeightKey::CivilActionGap | WeightKey::CivilActionSurplus | WeightKey::TakeCostShare | WeightKey::MilitaryActionGap | WeightKey::MilitaryActionSurplus | WeightKey::WorkerGap | WeightKey::WorkerSurplus | WeightKey::TechRedundancyDiscount => panic!("WeightKey::late called on a key outside PHASE_KEYS"),
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
            WeightKey::RateHorizon | WeightKey::Culture | WeightKey::Science | WeightKey::FoodRate | WeightKey::ResourceRate | WeightKey::FoodStock | WeightKey::ResourceStock | WeightKey::BlueFree | WeightKey::CorruptionHeadroom | WeightKey::ConsumptionHeadroom | WeightKey::PopCost | WeightKey::YellowBank | WeightKey::FreeWorkers | WeightKey::Workers | WeightKey::ProdWorkers | WeightKey::UrbanWorkers | WeightKey::UnitWorkers | WeightKey::HappyMargin | WeightKey::Discontent | WeightKey::Uprising | WeightKey::CivilActions | WeightKey::MilitaryActions | WeightKey::CaLeft | WeightKey::MaLeft | WeightKey::TakeCostPaid | WeightKey::RowUrgency | WeightKey::RowBargainForgone | WeightKey::RowLastCopy | WeightKey::RivalDesire | WeightKey::RivalTakeShare | WeightKey::RivalFreeCa | WeightKey::RivalHandCivil | WeightKey::RivalWonders | WeightKey::RivalHandPotential | WeightKey::RivalScienceStock | WeightKey::RivalFoodStock | WeightKey::RivalResourceStock | WeightKey::RivalFreeWorkers | WeightKey::RivalYellowBank | WeightKey::RivalColonies | WeightKey::RivalMilActions | WeightKey::RivalBuildingWonder | WeightKey::MySeededPending | WeightKey::MyEventThreat | WeightKey::AttackTargetLead | WeightKey::AttackTargetWeakness | WeightKey::PactPartnerLead | WeightKey::Strength | WeightKey::StrengthRel | WeightKey::StrengthDeficit | WeightKey::StrengthLead | WeightKey::TacticLevel | WeightKey::TacticGain | WeightKey::TacticShort | WeightKey::HasUnit | WeightKey::Colonies | WeightKey::HasColony | WeightKey::Pacts | WeightKey::PactBlocksAttack | WeightKey::WarImmune | WeightKey::AttackCostDoubled | WeightKey::AuctionCommitted | WeightKey::AuctionBid | WeightKey::TechLevels | WeightKey::GovLevel | WeightKey::BestFarm | WeightKey::BestMine | WeightKey::BestLab | WeightKey::BestTemple | WeightKey::BestTheater | WeightKey::BestLibrary | WeightKey::BestArena | WeightKey::BestUnit | WeightKey::NumTechs | WeightKey::SpecialTechs | WeightKey::Wonders | WeightKey::WonderProgress | WeightKey::WonderRemaining | WeightKey::WonderStagesLeft | WeightKey::WonderTurnsToFinish | WeightKey::WonderOverrun | WeightKey::WonderStagesPerAction | WeightKey::WonderPotential | WeightKey::WonderPromise | WeightKey::WonderAgeOverrun | WeightKey::Leader | WeightKey::WonderInProgress | WeightKey::HandLimit | WeightKey::ColonizeBonus | WeightKey::BuildDiscount | WeightKey::FreeCivilAction | WeightKey::ResourceDiscount | WeightKey::DefenseBonus | WeightKey::UrbanLimit | WeightKey::GovActionCost | WeightKey::NoAggression | WeightKey::RestrictedResources | WeightKey::CardBoardCredit | WeightKey::EventScoringMargin | WeightKey::CardBoardLeader | WeightKey::CardBoardBonus | WeightKey::HandSwapExtra | WeightKey::CardRateCredit | WeightKey::UnitStrengthCredit | WeightKey::UnitTechCredit | WeightKey::TechBoardCredit | WeightKey::ActionBoardCredit | WeightKey::GovBoardCredit | WeightKey::WonderBoardCredit | WeightKey::BuildFreshCredit | WeightKey::RestrictedResourceCredit | WeightKey::FreeActionCredit | WeightKey::TerritoryCredit | WeightKey::BonusCardCredit | WeightKey::TacticBoardCredit | WeightKey::AggressionBoardCredit | WeightKey::WarBoardCredit | WeightKey::PactBoardCredit | WeightKey::EventBoardCredit | WeightKey::TacticShortfallCost | WeightKey::TacticReachCredit | WeightKey::HandCivil | WeightKey::HandValue | WeightKey::HandPotential | WeightKey::HandMilitary | WeightKey::HandMilValue | WeightKey::HandMilPotential | WeightKey::HandPerishable | WeightKey::RivalCulture | WeightKey::RivalMeanCulture | WeightKey::RivalCultureRate | WeightKey::RivalScienceRate | WeightKey::RivalStrength | WeightKey::EndTurnBias | WeightKey::CultureRateTrailing | WeightKey::ScienceRateTrailing | WeightKey::WorkersLate | WeightKey::StrengthRelEarly | WeightKey::StrengthRelLate | WeightKey::TechLevelsLate | WeightKey::HandValueLate | WeightKey::FoodGap | WeightKey::FoodSurplus | WeightKey::ResourceGap | WeightKey::ResourceSurplus | WeightKey::ScienceGap | WeightKey::ScienceSurplus | WeightKey::CultureGap | WeightKey::CultureSurplus | WeightKey::HappySurplus | WeightKey::CivilActionGap | WeightKey::CivilActionSurplus | WeightKey::TakeCostShare | WeightKey::MilitaryActionGap | WeightKey::MilitaryActionSurplus | WeightKey::WorkerGap | WeightKey::WorkerSurplus | WeightKey::TechRedundancyDiscount => panic!("WeightKey::trailing called on a key outside STANDING_KEYS"),
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
            // `TakeCostShare` is `TakeCostPaid` over the whole allowance, so
            // it belongs beside it -- a group move that says "care more about
            // what a row card costs me in actions" must reach both.
            | TakeCostShare
            | MilitaryActionSurplus => {
                WeightGroup::Actions
            }

            UrbanLimit | GovActionCost | NoAggression | RestrictedResources
            | CardBoardCredit | CardBoardLeader | CardBoardBonus => WeightGroup::Board,

            HandCivil | HandValue | HandValueLate | HandPotential
            // `HandPerishable` is a property OF the hand (how much of it is
            // about to expire), so it moves with the rest of the hand axis.
            | HandPerishable
            | HandMilitary | HandMilValue | HandMilPotential | HandSwapExtra => {
                WeightGroup::Cards
            }

            RateHorizon | Culture | CultureRate | CultureRateTrailing | Science | ScienceRate
            | ScienceRateTrailing | FoodRate
            | ResourceRate | FoodStock | ResourceStock | BlueFree | CorruptionHeadroom
            | ConsumptionHeadroom | PopCost | YellowBank | FreeWorkers | Workers
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
            | StrengthLead | TacticLevel | TacticGain | TacticShort | HasUnit | Colonies
            | HasColony | Pacts | PactBlocksAttack | WarImmune | AttackCostDoubled
            | AuctionCommitted | AuctionBid => {
                WeightGroup::Military
            }

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

            TechLevels | TechLevelsLate | GovLevel | BestFarm | BestMine
            | BestLab | BestTemple | BestTheater | BestLibrary | BestArena | BestUnit
            | NumTechs | SpecialTechs => WeightGroup::Tech,

            Wonders | WonderProgress | WonderRemaining | WonderStagesLeft
            | WonderTurnsToFinish | WonderOverrun | WonderStagesPerAction | WonderPotential
            // The two new wonder coordinates: the value still ahead of the
            // player, and the deadline the rules impose on reaching it.
            | WonderPromise | WonderAgeOverrun
            | Leader | WonderInProgress => WeightGroup::Wonders,
        }
    }

    /// What the ARITHMETIC forces on this weight's coefficient sign, derived
    /// once, here, from the enum itself -- the structural fix for the bug
    /// class `card_board_leader = -15.0` was: `card_potential` priced every
    /// leader's board benefit through `card_board_credit + card_board_leader`,
    /// and the only sign gate that existed (`eval::BENEFIT_GATES`, a
    /// hand-typed list) never named the per-type key, so a helpful leader
    /// priced as a LOSS for months with nothing failing. `dominance_repair`
    /// closed that ONE key; this closes the SHAPE, the same way
    /// [`WeightKey::group`] above closes "which strategic axis" instead of
    /// leaving it to a hand-maintained list a new variant can silently miss.
    ///
    /// Deliberately NO wildcard `_ =>` arm, for [`group`](Self::group)'s own
    /// reason restated: a 163rd variant is a compile error here until a
    /// human puts it in one of [`SignIntent`]'s three buckets on purpose.
    /// [`SignIntent::Free`] is not a fallback a classifier falls into by
    /// omission -- every arm below names it explicitly, with the evidence
    /// for why (a doc citation, a feature's proven sign, or "no RULES_SPEC
    /// citation forces a direction, matches this project's own existing
    /// conservatism") right next to it, because a WRONG gate silently
    /// overrides a real fitted value and is worse than a missing one
    /// (`SIGNAUDIT.txt`'s own framing). [`eval::dominance_repair`] reads
    /// this match directly (no second, copied table), so a key reclassified
    /// here is repaired at both load time and `bin/climb.rs` mutation time
    /// automatically -- see that function's own doc comment.
    pub const fn sign_intent(self) -> SignIntent {
        use SignIntent::*;
        use WeightKey::*;
        match self {
            // ---------------------------------------------------- NonNegative
            // Scales a PRINTED per-card benefit and nothing else -- the ONLY
            // channel its class has, and a card that prints one is never
            // worse than the same card without it (RULES_SPEC never makes a
            // grant compulsory to use). Matches the former `BENEFIT_GATES`.
            BuildDiscount | CardBoardCredit | DefenseBonus | FreeCivilAction | HandLimit
            | ResourceDiscount | RestrictedResources | UnitStrengthCredit
            | WonderStagesPerAction => NonNegative("scales a printed benefit"),
            // A redundant card getting MORE valuable the more of its lane is
            // already covered would invert the discount's own premise, not
            // just leave a direction unmeasured. Former `REDUNDANCY_NONNEG_GATES`.
            TechRedundancyDiscount => {
                NonNegative("discounts a redundant card, never rewards one")
            }
            // The SOLE identity-aware channel pricing what completing THIS
            // in-progress wonder would do -- gains-only by construction
            // (`gains_only_sum`/`gains_only_board_sum` drop every Cost-kind
            // triple before either function ever sees one). Former
            // `WONDER_VALUE_GATES`.
            WonderPotential | WonderPromise => {
                NonNegative("prices the in-progress wonder's completion value")
            }
            // The only channel `Special::FreeCivilAction` has, and unlike its
            // `*BoardCredit` bucket-mates it is provably gains-only without
            // auditing a whole card-pricing function: `cards::action_value`'s
            // branch for it multiplies this scale by ONE non-negative
            // marginal (`CivilActions`, itself gated below), never a sum over
            // printed effects that could carry a Cost. A card granting a free
            // civil action is never worse than the same card without it --
            // the same sentence that gates `FreeCivilAction`, which prices
            // the identical printed ability through the typed-field path.
            FreeActionCredit => NonNegative("scales a printed benefit"),
            // Raw board STOCKS the rules only ever ADD effects for, cited
            // chapter and verse (RULES_SPEC, see `eval::dominance_repair`'s
            // own doc comment on this bucket): an available civil action, its
            // whole-turn surplus, and a completed wonder. Former
            // `STOCK_NONNEG_GATES` -- deliberately NOT extended to every
            // other stock-shaped key in this table (`workers`, `culture`,
            // `science_rate`, ...): those lack an equally citable rule and
            // stay `Free` below, matching that gate's own existing refusal to
            // guess.
            CivilActions | CivilActionSurplus | Wonders => {
                NonNegative("prices an available/completed stock the rules never subtract for")
            }
            // `cards::tactic_terms`: `gain = max(0, best_army -
            // army_strength(p))`, an available army-strength IMPROVEMENT --
            // never negative by construction, and forming it is optional, so
            // a bigger available gain is never worse. NEW this audit: found
            // negative in 8 of 10 champion snapshots on disk (as low as
            // -60.0), the identical inversion `card_board_leader` had --
            // see SIGNAUDIT.txt.
            TacticGain => NonNegative("prices an available army-strength improvement as a downside"),

            // ---------------------------------------------------- NonPositive
            // `max(0, need - have)` marginal-need SHORTFALLS -- a bigger gap
            // is never an improvement under any reading of the rules. Former
            // `SHORTFALL_GATES`. The matching `*Surplus` siblings are `Free`
            // below on purpose -- see that bucket's own comment.
            FoodGap | ResourceGap | ScienceGap | CultureGap | CivilActionGap
            | MilitaryActionGap | WorkerGap => NonPositive("prices a marginal-need shortfall"),
            // Penalties the rules IMPOSE, larger the worse off the player is
            // -- `discontent = max(0, -happy_margin)`, `uprising` a 0/1
            // indicator, `strength_deficit = max(0, -relative_strength)`.
            // Former `LOSS_GATES`.
            Discontent | Uprising | StrengthDeficit => {
                NonPositive("prices a penalty the rules impose")
            }
            // `economy::pop_food_cost` -- the RULEBOOK's own cost table for
            // the next population increase, always >= 0 by construction
            // (`unwrap_or(8)`, never negative). Larger the further a stage
            // is from paid, exactly `corruption`/`consumption`'s own shape
            // (both netted OUT of separate weights for this identical
            // confound: a big civilization pays more of a rulebook cost than
            // a small one, so a strictly-bad coordinate correlates with
            // strength, and a climb chasing win rate charges the correlation
            // to the "penalty"). `pop_cost` is still a live, separate
            // `WeightKey` (never netted), so it still needs the gate its
            // netted siblings no longer do. NEW this audit: found +13.34 in
            // one live 4p champion snapshot (authored default -0.4) -- see
            // SIGNAUDIT.txt.
            PopCost => NonPositive("prices a rulebook population-growth cost as an upside"),
            // What an UNFINISHED wonder still owes -- non-negative magnitudes
            // that fall on exactly the move that pays a stage, so a positive
            // price turns paying into a loss. Former `WONDER_DEBT_GATES`.
            WonderRemaining | WonderStagesLeft | WonderTurnsToFinish | WonderOverrun
            | WonderAgeOverrun => NonPositive("prices an unpaid wonder debt as an upside"),
            // How much of the civil hand the next age boundary is about to
            // discard for nothing (RULES_SPEC 12.2) -- never an upside.
            // Former `PERISHABLE_GATES`.
            HandPerishable => NonPositive("prices a hand about to expire as an upside"),
            // `effects::tactic_outlook`'s per-type shortfall to the NEXT
            // army -- `TacticGain`'s own shortfall half, same shape as
            // `SHORTFALL_GATES` above (ported from the identical "whole-hand
            // analogue" `TacticShortfallCost`'s own doc comment already
            // names). NEW this audit: found positive in 6 of 10 champion
            // snapshots on disk (as high as +10.8) -- see SIGNAUDIT.txt.
            TacticShort => NonPositive("prices a marginal-need shortfall"),

            // -------------------------------------------------------- Free
            // Composite constraints living in a DIFFERENT mechanism, not a
            // simple per-key sign -- classifying either `NonNegative` here
            // would be WRONG (over-constraining a coordinate whose base term
            // is legally allowed to be negative as long as the SUM is not):
            //
            // * `CardBoardLeader`/`CardBoardBonus`: the effective multiplier
            //   `cards::card_potential` scales a leader/bonus swap diff by is
            //   `CardBoardCredit + <this key>`, not either term alone --
            //   `eval::dominance_repair`'s `card_board_credit_keys()` loop
            //   (itself derived from `cards::board_credit_key`, not hand
            //   copied) gates the SUM. `CardBoardGovernment`/`CardBoardAction`/
            //   `CardBoardWonder` used to sit alongside these two here; all
            //   three are RETIRED as of this audit (`RETIRED_KEYS`) --
            //   `cards::card_potential_core`'s dedicated
            //   `gov_value`/`action_value`/wonder swap-diff branches
            //   unconditionally intercept and `return` before the generic
            //   per-type path is ever reached whenever their own
            //   `GovBoardCredit`/`ActionBoardCredit`/`WonderBoardCredit` is
            //   nonzero -- which is every trained champion sampled, since
            //   the first two default nonzero and are "measured effective"
            //   -- so the three retired keys were live-looking knobs wired
            //   to nothing, the same shape `card_yields`'s own deleted
            //   static action formula was retired for (see
            //   `cards.rs::tests::card_yields_never_reprices_the_action_
            //   boards_ring_fenced_coordinates`'s doc comment for that
            //   precedent). See SIGNAUDIT.txt.
            CardBoardLeader | CardBoardBonus => Free,
            // `HandValue`/`HandValueLate`: the feature `hand_value` (`Σ
            // level()+1` over the civil hand) is always >= 0, so the
            // coefficient `evaluate` actually applies at ANY lateness must
            // never be negative either. Until 2026-08-13 this was `Free`
            // (both individually unsigned) with a SEPARATE composite
            // mechanism (`eval::NET_NONNEG_PHASE`) enforcing the NET
            // `base + phase-blend >= 0` at load/mutation time, because the
            // three-parameter blend made "the base alone" and "the net"
            // different things. PHASECUT.txt's T1-D collapse removed that
            // distinction: `HandValue` now directly holds the L=0 value and
            // `HandValueLate` the L=1 value of `start*(1-L) + end*L`, a
            // CONVEX combination of the two -- so the net is >= 0 at every
            // lateness in [0,1] if and only if BOTH endpoints are >= 0
            // individually, exactly what a plain per-key `NonNegative` gate
            // on each already checks. Strictly simpler, not weaker (the old
            // composite constraint set and this one are identical once the
            // redundant degree of freedom is gone) -- `eval::
            // NET_NONNEG_PHASE` is now empty. Found net-negative (as low as
            // `0.2 + (-27.68) = -27.48` under the OLD basis) in every
            // champion snapshot on disk carrying nonzero phase pairs -- see
            // SIGNAUDIT.txt.
            HandValue | HandValueLate => {
                NonNegative("hand_value's feature is always >= 0 (a convex blend of two such endpoints is too)")
            }

            // Every remaining key: a genuine trade-off / preference
            // coordinate the league prices empirically, with no RULES_SPEC
            // citation or gains-only proof forcing a direction -- grouped by
            // WHY below rather than left to speak for itself, so a future
            // reader auditing one bucket does not have to re-derive the
            // reasoning `SIGNAUDIT.txt` already wrote down.
            //
            // Raw economy/board magnitudes and rates with no rulebook
            // citation that a bigger number is never a downside -- the same
            // conservatism `STOCK_NONNEG_GATES`'s own doc comment already
            // states outright for `science_rate` ("no RULES_SPEC citation
            // establishing that more science production can never be a
            // downside... guessing its sign is exactly what this table
            // exists to refuse to do"), generalised to every sibling stock/
            // rate/level here rather than re-litigated key by key.
            RateHorizon | Culture | CultureRate | Science | ScienceRate | FoodRate
            | ResourceRate | FoodStock | ResourceStock | BlueFree | YellowBank | FreeWorkers
            | Workers | ProdWorkers | UrbanWorkers | UnitWorkers | HappyMargin
            | MilitaryActions | TechLevels | GovLevel | BestFarm | BestMine | BestLab
            | BestTemple | BestTheater | BestLibrary | BestArena | BestUnit | NumTechs
            | SpecialTechs | WonderProgress | Leader | Strength | StrengthRel | StrengthLead
            | TacticLevel | HasUnit | Colonies | HasColony | Pacts | PactBlocksAttack
            | WarImmune | AttackCostDoubled | WonderInProgress => Free,
            // `CorruptionHeadroom`/`ConsumptionHeadroom`: `features.rs`'s own
            // comment on these two, verbatim -- "headroom is a deterministic
            // function of `BlueFree`/`YellowBank`, so 'good all else equal'
            // is vacuous here -- the two coordinates cannot move
            // independently, and the league is left to price the pair."
            CorruptionHeadroom | ConsumptionHeadroom => Free,
            // `CaLeft`: `weights.rs`'s own extensive VERDICT comment on this
            // key already establishes it is deliberately uncapped/unsigned
            // (mid-turn option value, no rulebook conversion to approximate)
            // -- SIGNAUDIT.txt's task explicitly keeps it out of scope
            // ("measured: net regression, loses at 38.2%"), so it is not
            // revisited here. `MaLeft`'s end-of-turn draw is a cliff already
            // capped elsewhere (`board_yields::MA_DRAW_CAP`), not by a sign
            // gate on this coefficient.
            CaLeft | MaLeft => Free,
            // Already explicitly declared free by `TakeCostShare`'s own doc
            // comment: "spending actions on a card worth having is not a
            // rules-level loss, so its sign is the league's to find" --
            // restated here for `TakeCostPaid`, its numerator.
            TakeCostPaid | TakeCostShare => Free,
            // Row-reading and rival-row preference coordinates -- how much to
            // weigh a rival wanting a card is a strategic read, not a rules
            // fact. `RowLastCopy`'s units defect ("its defect is units, not
            // sign", once out of scope for the sign audit) is fixed now --
            // `row::row_last_copy` no longer sums `card_potential(w) * gone`
            // (a composite `w` already prices, feeding a second, outer
            // coefficient with no fixed meaning); it sums `gone` alone,
            // `card_potential` used only as the `> 0.0` "is this even wanted"
            // gate. Still `Free` -- fixing the units did not establish a sign
            // any rule cites, so this classification itself is untouched.
            RowUrgency | RowBargainForgone | RowLastCopy | RivalDesire | RivalTakeShare => Free,
            // Raw RIVAL board facts -- whether more of a rival's stock is
            // something to race against, ignore, or exploit is itself the
            // strategic judgement being fit; unlike `StrengthDeficit`
            // (`Military`, already gated), none of these have an established
            // "this magnitude is always bad for ME" reading.
            RivalFreeCa | RivalHandCivil | RivalWonders | RivalHandPotential
            | RivalScienceStock | RivalFoodStock | RivalResourceStock | RivalFreeWorkers
            | RivalYellowBank | RivalColonies | RivalMilActions | RivalBuildingWonder
            | RivalCulture | RivalMeanCulture | RivalCultureRate | RivalScienceRate
            | RivalStrength => Free,
            // Event-timing and targeting comparisons -- a "lead" or
            // "weakness" measure is a signed difference by construction, not
            // a one-directional magnitude.
            MySeededPending | MyEventThreat | AttackTargetLead | AttackTargetWeakness
            | PactPartnerLead => Free,
            // Auction bidding is a genuine trade-off (paying more to win a
            // colony/aggression slot), not a rules-imposed penalty the
            // player never chooses.
            AuctionCommitted | AuctionBid => Free,
            // Board-state indicator features (`features()` writes a raw
            // count/flag for each) rather than a per-card printed benefit in
            // `BuildDiscount`'s sense -- no dedicated citation established
            // this audit pass; flagged in SIGNAUDIT.txt as a follow-up
            // candidate, not gated here (a wrong gate is worse than a
            // missing one).
            ColonizeBonus | UrbanLimit | NoAggression => Free,
            // `docs/OPEN_ITEMS.md` item 1: a real, live-reading coordinate
            // whose drift is documented as "signal vs noise, not yet
            // determined" -- explicitly not concluded to be rules-forced
            // either way.
            GovActionCost => Free,
            // A scoring MARGIN -- signed by construction (ahead of or behind
            // the field), not a one-directional magnitude.
            EventScoringMargin => Free,
            // A spare single-slot card's (leader/government) incremental
            // value beyond the best one already counted -- its own doc
            // comment in `cards.rs` calls it out as "a free 0.0-default
            // WEIGHT, not a constant", i.e. deliberately the league's to find.
            HandSwapExtra => Free,
            // The "how much to trust this dedicated function's board-aware
            // estimate" multiplier family (`registry.rs`'s own
            // characterization) -- NOT a printed-benefit magnitude the way
            // `BuildDiscount`'s siblings are, so `BENEFIT_GATES`'s reasoning
            // does not automatically transfer. Whether each dedicated
            // function (`tech_value`/`action_value`/`gov_value`/...) is
            // provably gains-only the way `wonder_potential` is, and so
            // deserves a `NonNegative` gate of its own, was NOT verified
            // within this audit's budget -- flagged in SIGNAUDIT.txt as the
            // top follow-up candidate, not guessed at here.
            CardRateCredit | UnitTechCredit | TechBoardCredit | ActionBoardCredit
            | GovBoardCredit | WonderBoardCredit | BuildFreshCredit | RestrictedResourceCredit
            | TerritoryCredit | BonusCardCredit | TacticBoardCredit
            | AggressionBoardCredit | WarBoardCredit | PactBoardCredit | EventBoardCredit
            | TacticShortfallCost | TacticReachCredit => Free,
            // Hand-content magnitudes other than `HandValue` (handled above)
            // -- plausible `NonNegative` candidates by the same "a card in
            // hand is never a rules-level cost" logic, but not individually
            // verified this pass; left `Free` rather than guessed at.
            HandCivil | HandPotential | HandMilitary | HandMilValue | HandMilPotential => Free,
            // `WeightedBot::choose`'s own doc comment: "DO NOT fix this
            // asymmetry ... measured (twice, two different ways) against
            // every alternative and is strictly stronger" -- an empirically
            // tuned search bias, not a board-position coefficient at all.
            EndTurnBias => Free,
            // Standing hinges, gated at `0.0` by design so landing them moves
            // no game until the league prices them (`STANDING_KEYS`'s own
            // doc comment) -- whether trailing in culture/science makes a
            // marginal point worth MORE or LESS is exactly what the hinge
            // exists to answer, not a foregone conclusion.
            CultureRateTrailing | ScienceRateTrailing => Free,
            // Phase modifiers for the `PHASE_KEYS` members other than
            // `HandValue` (whose own `_late`/base pair moved to
            // `NonNegative` above, T1-D) -- `Workers`/`StrengthRel`/
            // `TechLevels` themselves are `Free` above (no rules citation
            // forces their own sign), so their late-extreme/phase partners
            // have nothing non-arbitrary to anchor a sign gate to either.
            // `WorkersEarly`/`TechLevelsEarly` no longer exist post T1-A/C
            // collapse (folded into `Workers`/`TechLevels` themselves,
            // classified above) -- see PHASECUT.txt.
            WorkersLate | StrengthRelEarly | StrengthRelLate | TechLevelsLate => Free,
            // The matching SURPLUS half of every gated `*Gap` shortfall above
            // -- deliberately NOT gated, straight from `SHORTFALL_GATES`'s
            // own former doc comment: "whether banking more than you need is
            // worth something or nothing is not unambiguous the way a
            // shortfall's sign is, so the league prices it unconstrained."
            // `CivilActionSurplus` is the one exception (see `NonNegative`
            // above) -- a DIFFERENT, RULES-cited constraint
            // (`STOCK_NONNEG_GATES`'s own reasoning), not this one.
            FoodSurplus | ResourceSurplus | ScienceSurplus | CultureSurplus | HappySurplus
            | MilitaryActionSurplus | WorkerSurplus => Free,
        }
    }
}

/// What [`WeightKey::sign_intent`] concludes about one coefficient's legal
/// sign -- see that method's own doc comment for the derivation and
/// [`super::eval::dominance_repair`] for where it is spent.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SignIntent {
    /// Scales a quantity unambiguously GOOD for the player and never
    /// subtracted for by the rules -- the coefficient must never be
    /// negative. The `&'static str` is the WHY, logged verbatim by
    /// [`super::eval::dominance_repair`]'s `Violation::rule` when it fires --
    /// carried on the classification itself (not a second, parallel lookup)
    /// so the direction and the reason can never drift apart. Repaired UP to
    /// `0.0` ("unpriced"), never to an invented positive replacement.
    NonNegative(&'static str),
    /// Scales a quantity unambiguously BAD for the player -- the coefficient
    /// must never be positive. See [`Self::NonNegative`] for what the
    /// `&'static str` is for. Repaired DOWN to `0.0`.
    NonPositive(&'static str),
    /// A trade-off/preference coordinate the league prices empirically, or a
    /// coefficient whose real constraint is a COMPOSITE one (a sum with
    /// another key, or a net across a phase pair) enforced by a different
    /// mechanism -- see the specific [`WeightKey::sign_intent`] match arm for
    /// which case applies. Not a fallback: every arm chooses this
    /// explicitly, there is no wildcard feeding it.
    Free,
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

/// Mirrors Python's `PHASE_KEYS`: which BASE features additionally carry a
/// lateness-blended pair. The four that stay, and why six others (`culture`,
/// `culture_rate`, `science_rate`, `food_rate`, `resource_rate`,
/// `wonder_progress`) were retired on 2026-08-04 -- `rate_multiplier` now
/// prices the four RATE_KEYS through the exact `rounds_left`-derived horizon
/// instead of this affine shape, and `culture`/`wonder_progress` are
/// numeraire/stock terms a phase blend must not rescale -- are explained at
/// length in the Python source's own comment on this constant; not
/// reproduced here.
///
/// Two different blends now live behind this one list (PHASECUT.txt,
/// 2026-08-13, T1-A/C/D): `Workers`/`TechLevels`/`HandValue` were collapsed
/// from the old 3-parameter `w[k] + (1-L)*w[k.early()] + L*w[k.late()]`
/// (only 2 real degrees of freedom for 3 raw numbers -- a proven dead
/// direction) to the equivalent, non-redundant 2-parameter
/// `w[k]*(1-L) + w[k.late()]*L`, where `k` itself now holds the
/// early-extreme ("start") value and `k.late()` the late-extreme ("end")
/// value -- see `eval::evaluate`'s own phase-blended body for exactly where
/// this is computed. `StrengthRel` keeps the OLD 3-parameter shape (with a
/// parallel fix, commit 578ee9e, further round-gating it) -- excluded from
/// the collapse because that fix makes its triple genuinely identifiable
/// again; see `WeightKey::early`'s own doc comment and PHASECUT.txt for the
/// full argument.
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
    // Retired 2026-08-13 (SIGNAUDIT.txt): `cards::card_potential_core`
    // dispatches Government/Action/Wonder cards through a DEDICATED
    // board-aware function (`gov_value`/`action_value`, and Wonder's own
    // `board_yields::board_yields` swap-diff branch) gated on
    // `gov_board_credit`/`action_board_credit`/`wonder_board_credit`, and
    // that branch unconditionally `return`s once its own credit is nonzero
    // -- it never falls through to the generic per-type path these three
    // keys used to offset. `gov_board_credit`/`action_board_credit` default
    // NONZERO (1.0, "measured effective from the start"), so
    // `card_board_government`/`card_board_action` were dead on every
    // trained champion by construction, not merely by drift;
    // `wonder_board_credit` defaults 0.0 but is climbed away from it in
    // practice, making `card_board_wonder` dead the same way once trained
    // (measured: nonzero and wrong-signed in most champion snapshots on
    // disk while simultaneously unreachable). Same shape as `card_yields`'s
    // own deleted static action formula (see `cards.rs::tests::
    // card_yields_never_reprices_the_action_boards_ring_fenced_
    // coordinates`'s doc comment) -- a provably-unreachable pricing path is
    // deleted, not pinned or left live for a future mutation to rediscover.
    // `CardBoardLeader`/`CardBoardBonus` are NOT retired alongside these:
    // Leader and Bonus have no dedicated top-level branch in
    // `card_potential_core` at all, so their per-type offset is the ONLY
    // board-aware pricing channel either type has.
    "card_board_government",
    "card_board_action",
    "card_board_wonder",
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
    /// `PHASE_KEYS` and the flat table's `*_late` entries (every member) and
    /// `*_early` entry (`StrengthRel` alone, post PHASECUT.txt's T1-A/C/D
    /// collapse -- see `WeightKey::early`'s own doc comment) name EXACTLY
    /// the expected set of features, checked in both directions.
    #[test]
    fn phase_keys_and_the_flat_table_agree() {
        for &k in PHASE_KEYS {
            assert!(WeightKey::ALL.contains(&k.late()), "{}: late() not in ALL", k.name());
            assert_eq!(k.late().name(), format!("{}_late", k.name()));
        }
        assert!(WeightKey::ALL.contains(&WeightKey::StrengthRel.early()));
        assert_eq!(WeightKey::StrengthRel.early().name(), "strength_rel_early");
        for &k in WeightKey::ALL {
            let name = k.name();
            if let Some(base) = name.strip_suffix("_late") {
                assert!(
                    PHASE_KEYS.iter().any(|&p| p.name() == base),
                    "{name}: phase-suffixed key with no PHASE_KEYS base"
                );
            }
            if let Some(base) = name.strip_suffix("_early") {
                assert_eq!(
                    base, "strength_rel",
                    "{name}: only strength_rel has an _early key post PHASECUT.txt's T1-A/C/D collapse"
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

    /// A `_late` phase key is in the SAME group as its base key, for all
    /// four `PHASE_KEYS` -- the property `hillclimb.py`'s `GROUP_KEYS`
    /// comment relies on ("a group move is 'care more/less about this whole
    /// strategic axis', at every age"). `StrengthRel`'s `_early` partner
    /// (the only one left post PHASECUT.txt's T1-A/C/D collapse) is checked
    /// too.
    #[test]
    fn phase_key_shares_its_base_keys_group() {
        for &k in PHASE_KEYS {
            assert_eq!(k.late().group(), k.group(), "{}_late", k.name());
        }
        assert_eq!(WeightKey::StrengthRel.early().group(), WeightKey::StrengthRel.group());
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
            ("card_board_bonus", "board"),                 // GROUPS["board"]
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

    /// A key classified [`SignIntent::NonNegative`] must be authored with a
    /// non-negative default, and one classified [`SignIntent::NonPositive`]
    /// with a non-positive one -- if the crate's own [`WeightKey::
    /// default_weight`] disagreed with [`WeightKey::sign_intent`], that would
    /// mean the classification and the author's own intent contradict each
    /// other, which is a bug to fix at the source, not something
    /// `eval::dominance_repair` should paper over on every load. This is the
    /// single check that used to be split across `eval.rs`'s
    /// `no_gated_wonder_debt_weight_is_authored_as_an_upside` and
    /// `no_gated_non_negative_weight_is_authored_as_a_downside` -- one test
    /// over the derived classification now covers every key either of those
    /// hand-typed lists could ever have named, plus every key added since.
    #[test]
    fn every_sign_intent_classification_agrees_with_its_own_authored_default() {
        for &k in WeightKey::ALL {
            match k.sign_intent() {
                SignIntent::NonNegative(why) => assert!(
                    k.default_weight() >= 0.0,
                    "{} is classified NonNegative ({why}) but defaults to {}",
                    k.name(),
                    k.default_weight()
                ),
                SignIntent::NonPositive(why) => assert!(
                    k.default_weight() <= 0.0,
                    "{} is classified NonPositive ({why}) but defaults to {}",
                    k.name(),
                    k.default_weight()
                ),
                SignIntent::Free => {}
            }
        }
    }

    /// Regression pin for this audit's three NEW classifications
    /// (SIGNAUDIT.txt): `tactic_gain` scales an always-non-negative available
    /// army-strength improvement (`NonNegative`), `tactic_short` and
    /// `pop_cost` scale always-non-negative rulebook shortfalls/costs
    /// (`NonPositive`). Pinned by name (not just by not-panicking) so a
    /// future edit that silently reclassifies one of these three back to
    /// `Free` fails a test instead of quietly reopening the exact hole
    /// measured wrong-signed in most champion snapshots on disk.
    #[test]
    fn the_three_new_gates_this_audit_added_are_classified_as_expected() {
        assert!(matches!(WeightKey::TacticGain.sign_intent(), SignIntent::NonNegative(_)));
        assert!(matches!(WeightKey::TacticShort.sign_intent(), SignIntent::NonPositive(_)));
        assert!(matches!(WeightKey::PopCost.sign_intent(), SignIntent::NonPositive(_)));
    }

    /// The two keys pricing `Special::FreeCivilAction` -- `FreeCivilAction`
    /// through the typed field, `FreeActionCredit` through
    /// `cards::action_value`'s special-list branch -- scale the SAME printed
    /// ability and must therefore carry the SAME gate. Left split, the credit
    /// side is free to go negative and price Rich Land's and Engineering
    /// Genius's whole headline grant as a penalty, which is what every 2p
    /// champion on disk had learned (-0.24 on the deployed one).
    #[test]
    fn both_keys_pricing_a_free_civil_action_are_gated_non_negative() {
        assert!(matches!(WeightKey::FreeCivilAction.sign_intent(), SignIntent::NonNegative(_)));
        assert!(matches!(WeightKey::FreeActionCredit.sign_intent(), SignIntent::NonNegative(_)));
    }

    /// `card_board_government`/`card_board_action`/`card_board_wonder` are
    /// RETIRED (SIGNAUDIT.txt: `cards::card_potential_core`'s dedicated
    /// `gov_value`/`action_value`/wonder branches unconditionally shadow the
    /// generic per-type path whenever their own credit is nonzero, which is
    /// every trained champion sampled) -- confirm they cannot be resolved
    /// back to a live `WeightKey` by name, the same guarantee
    /// `retired_keys_are_not_weight_keys` already checks for every other
    /// retired key, pinned by name here so a reviewer of THIS audit sees the
    /// three card-board retirements called out specifically rather than
    /// folded anonymously into the general list.
    #[test]
    fn the_three_retired_card_board_per_type_keys_have_nowhere_to_land() {
        for name in ["card_board_government", "card_board_action", "card_board_wonder"] {
            assert_eq!(WeightKey::by_name(name), None, "{name}: retired but still resolvable");
            assert!(RETIRED_KEYS.contains(&name), "{name}: retired but missing from RETIRED_KEYS");
        }
    }
}
