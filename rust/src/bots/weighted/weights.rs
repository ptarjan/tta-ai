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
    RivalWonderDeficit,
    RivalScienceDeficit,
    RivalCultureDeficit,
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
    ResourceDiscount,
    DefenseBonus,
    UrbanLimit,
    // This key looks unreachable and is not. Its sole production reader,
    // `board_yields::government_cost`, is on the generic fallback that
    // `cards::card_potential_core` takes only when `gov_board_credit == 0.0`
    // EXACTLY -- a value no trained champion carries. But `GovBoardCredit`
    // is one of the keys `sign_intent` gates `NonNegative`, and
    // `dominance_repair` repairs a violator by RAISING it to EXACTLY 0.0, so
    // any champion trained to a negative `gov_board_credit` (the 4p one is)
    // lands on that condition on every load and prices its government cards
    // through here. See `sign_intent`'s trust-multiplier block for the
    // general shape: 0.0 is a DISPATCH SWITCH for that family, not a neutral
    // "drop the term" value.
    GovActionCost,
    NoAggression,
    CardBoardCredit,
    EventScoringMargin,
    CardBoardLeader,
    // `CardBoardGovernment`/`CardBoardAction`/`CardBoardWonder` retired
    // 2026-08-13 (SIGNAUDIT.txt); `CardBoardBonus` retired 2026-08-24 -- see
    // `RETIRED_KEYS`'s own entry for why.
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

    /// Need hinge on [`FoodStock`](Self::FoodStock): what one more food is
    /// worth ON TOP of the flat stock weight, scaled by how far SHORT of the
    /// rulebook threshold this player is -- the fraction of the population-
    /// increase price (`economy::pop_food_cost`) they still cannot pay. Gated
    /// at 0.0 so it changes nothing until the league prices it.
    ///
    /// The multiplier is a DIMENSIONLESS FRACTION in `[0, 1]`, never the raw
    /// shortfall: [`FoodGap`](Self::FoodGap) already carries the raw level as
    /// a trained coordinate of `evaluate`'s dot product, so a second raw-level
    /// term here would double-count against it. `FoodGap` answers "how bad is
    /// this position"; this key answers "what is the next unit worth in it",
    /// and those are different questions about the same threshold.
    ///
    /// The four need hinges cover exactly the axes a rule converts a stock
    /// into a cost -- food feeds population, resources build, science
    /// develops, a free worker is what building consumes. Culture
    /// deliberately has none: nothing ever converts a stock of culture into a
    /// cost, so it has no threshold to be short of, and its pressure is
    /// competitive and already carried by
    /// [`CultureRateTrailing`](Self::CultureRateTrailing) -- see
    /// [`CultureGap`](Self::CultureGap)'s own doc comment.
    FoodStockNeeded,
    /// Need hinge on [`ResourceStock`](Self::ResourceStock), against the
    /// cheapest unstaffed tableau slot's printed resource cost. See
    /// [`FoodStockNeeded`](Self::FoodStockNeeded).
    ResourceStockNeeded,
    /// Need hinge on [`Science`](Self::Science), against the cheapest
    /// developable card in the civil hand. See
    /// [`FoodStockNeeded`](Self::FoodStockNeeded).
    ScienceNeeded,
    /// Need hinge on [`FreeWorkers`](Self::FreeWorkers), against the count of
    /// unstaffed tableau slots. See
    /// [`FoodStockNeeded`](Self::FoodStockNeeded).
    FreeWorkersNeeded,
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

    // ---------------------------------------------- leaf-eval conditioning
    // Two coordinates that split a fact an existing key already conflates
    // (`Leader`'s +17.9/-16.2 2p/4p sign flip, the motivating case) into a
    // narrower one a league can price on its own. Appended at the end
    // (rather than inserted near `Leader`/`WonderRemaining`) so `repr(usize)`
    // assignment for every key that already exists on disk is unchanged --
    // see `weight_keys!`'s own module doc comment on why position matters.
    /// `1.0` iff the player's CURRENT leader (`p.leader`) is a replacement
    /// for an earlier one, `0.0` if the slot is empty or still holds the
    /// player's first-ever leader. Derived from `p.taken_leader_ages`
    /// (state.rs:456), a bitmask over `Age` set the first time a leader of
    /// that age is taken and never cleared -- by §2.5/§9.1's one-leader-
    /// per-age rule, `count_ones()` is the exact number of leader cards this
    /// player has EVER taken, so `count_ones() >= 2` while `p.leader` is
    /// occupied can only mean at least one earlier leader was swapped out.
    /// Deliberately NOT `replaced_leader_this_turn` (state.rs:589), which
    /// `economy::end_of_turn` clears every turn -- that field answers "did a
    /// swap happen just now," this one answers "is a swap the reason I hold
    /// what I hold," which is the persistent fact a leaf evaluation needs.
    LeaderReplacement,
    /// The raw count (0..=4) of `state.age_civil` wonders sitting in the
    /// `completed_wonders` list of players OTHER than the one being
    /// evaluated -- rivals-only, by construction. The evaluated player's OWN
    /// completed-wonder count is already priced by [`Self::Wonders`]; folding
    /// it in here too would reproduce, one level down, the exact sign-
    /// averaging defect `LeaderReplacement` above exists to split apart (a
    /// player who claimed 2 of the 4 wonders themselves would read a HIGH
    /// value that is actually good news for them). Counts COMPLETED wonders
    /// only (`p.wonder`, in-progress, is a different, noisier signal already
    /// touched by `WonderInProgress`/`RivalBuildingWonder`), keyed against
    /// `card_table.rs`'s fixed per-card `age` (four wonders per age, sixteen
    /// total) rather than `age_military` -- wonders are civil cards.
    WonderPoolRivalClaimed,

    /// `max(0, hand_civil - K)`, where `K` is how many cards in `p.hand_
    /// civil` this player could pay to play RIGHT NOW, off PRINTED costs
    /// only (never `costs::tech_cost`/`costs::build_cost_for`, for the same
    /// discount-precision reason [`MarginalNeed::resource`] already gives).
    /// `features::hand_card_affordable` is the one true classifier, matched
    /// exhaustively over every civil-hand-eligible `CardType`:
    ///
    /// * a levelled/tech type (`board_yields::is_levelled_type`, which
    ///   already covers `SpecialTech`) is paid for by DEVELOPING it -- its
    ///   printed `science_cost` against the player's science.
    /// * `Government` is NOT `is_levelled_type`, and its `science_cost` is
    ///   always 0 (`Card::science_cost`'s own doc comment says so
    ///   outright) -- its real price is `Card::peaceful_cost`, paid in
    ///   SCIENCE through the ordinary develop action (RULES_SPEC 8.3,
    ///   `costs::tech_cost`). A first pass at this key read `Card::
    ///   resource_cost` for every non-levelled type instead (also always 0
    ///   for a government in `card_table.rs`), which would have made an
    ///   unaffordable government invisible to the one feature meant to
    ///   catch exactly that -- caught before landing, not after.
    /// * `Leader`/`Action` print zero for both cost fields and genuinely
    ///   cost nothing to play from hand -- always affordable.
    /// * `Wonder` cannot physically be sitting in `p.hand_civil` at all: a
    ///   taken wonder goes straight to `PlayerState::wonder` and never
    ///   touches the hand (`apply.rs`'s take-move branch; RULES_SPEC 2.4,
    ///   "wonders bypass hand entirely", and 6.7). Every military-deck type
    ///   (`Tactic`/`Aggression`/`War`/`Pact`/`Bonus`/`Territory`/`Event`) is
    ///   drafted into `hand_military`, a different field, never this one.
    ///   Both groups are named explicitly in the classifier rather than
    ///   folded into a wildcard, matching `sweep_tableau`'s own precedent
    ///   for a state nothing enforces but the rules make impossible: a
    ///   silent, inert answer is the correct response if either ever fires.
    ///
    /// Counts CARDS, not actions -- 0.0 for a player who could afford to
    /// play everything they hold. `civil_action_gap`/`hand_civil` already
    /// compare hand SIZE against this turn's remaining civil actions; this
    /// is the orthogonal question `docs/`'s own round-1 measurement raises
    /// (analysis/feature_design_gap_conditional_2026-08-26.txt, proposal
    /// 3.4): a card can be legally reachable and still be dead weight this
    /// player cannot afford to DO anything with yet.
    HandOverCapacity,

    /// `max(0, -(margin after one more population increase))` -- the
    /// discontent the NEXT worker would create, computed under UNCHANGED
    /// staffing (proposal 3.5's own QUANTITY text asks for exactly this,
    /// not "increase population AND place the worker", which is a second,
    /// separate choice this key does not price).
    ///
    /// `effects::Stats.happy` is STAFFING-driven, not population-COUNT-
    /// driven (`effects::compute`'s `add_production`/`happy_from`: flat
    /// per-card grants plus `production.happy * slot.workers`) -- a freshly
    /// born worker lands in `p.workers_free` (`economy::increase_population`),
    /// unassigned, staffed nowhere, so it contributes zero to `Stats.happy`
    /// until a later, separate placement action. That means the ONLY thing a
    /// population increase changes for this formula is `p.yellow_bank`
    /// (`economy::increase_population` decrements it, or floors at 0 once the
    /// bank is already empty -- the design note's own verification, section
    /// "QUESTION 2", read this off `economy.rs` directly). So this key needs
    /// no second `effects::compute`/tableau resweep: it reads the SAME
    /// `s.happy` [`features::features`] already computed once, against
    /// [`economy::happy_required`] evaluated one token lower.
    /// `economy::happy_required`'s band table is the rulebook's own (VERIFIED
    /// CORRECT, `economy.rs`'s own `happy_required_bands` test) -- this key
    /// does not duplicate or alter it, only calls it a second time with
    /// `yellow_bank.saturating_sub(1)`.
    ///
    /// Same `max(0, -margin)` hinge shape [`WeightKey::Discontent`] already
    /// uses for the CURRENT board; this is the identical shortfall one
    /// population increase forward. `happy_margin`/`discontent`/
    /// `happy_surplus` all describe the board as it stands, so none of them
    /// lets the climb learn that THIS increase is the one that tips a
    /// player unhappy -- see the design note's own round-1/2-5 population
    /// action census (analysis/feature_design_gap_conditional_2026-08-26.txt,
    /// proposal 3.5).
    HappyMarginAfterNextPop,

    /// `(wonder_remaining + sum of printed resource costs of every
    /// developed-but-unstaffed tableau slot) / max(resource_rate, 1)`. How
    /// many turns of this player's ENTIRE resource production are already
    /// spoken for by standing obligations -- a ratio, deliberately outside
    /// the linear span of the two coordinates (`WonderRemaining`,
    /// `ResourceRate`) it is built from, the same reason `TakeCostShare`
    /// exists as its own coordinate rather than two separate ones.
    ///
    /// `wonder_remaining` is [`features::features`]'s own `remaining` local
    /// (`horizon::wonder_outlook`); the unstaffed-slot sum is a NEW
    /// accumulator folded into [`features::sweep_tableau`]'s existing loop,
    /// summing the exact printed `resource_cost` field
    /// [`features::TableauSweep::unbuilt_min_resource_cost`] already reads
    /// per slot -- not `costs::build_cost_for`, for the identical discount-
    /// precision reason [`HandOverCapacity`](Self::HandOverCapacity) and
    /// [`crate::bots::weighted::features::MarginalNeed::resource`] already
    /// give. This is a RESOURCE cost read off `p.techs` (developed-but-
    /// unbuilt slots), never a SCIENCE develop cost, so the Government/
    /// `costs::tech_cost` trap that bit [`HandOverCapacity`](Self::
    /// HandOverCapacity) does not apply here -- and `sweep_tableau`'s own
    /// match arms already establish a `Government` can never occupy a
    /// `Tableau` slot at all (governments live in `PlayerState::government`,
    /// not `Tableau`), so there is no such card to misprice in this loop
    /// even in principle. `resource_rate` reads back
    /// [`WeightKey::ResourceRate`] AFTER [`features::features`] sets it,
    /// i.e. the NET rate (corruption and pending gains already folded in),
    /// the same "one true computation, not a second copy of it" idiom
    /// [`HandOverCapacity`](Self::HandOverCapacity) uses for `Science`.
    ///
    /// Answers the owner's own sentence (design note section 2c): 11
    /// resources outstanding at 6/turn (two turns) and 11 outstanding at
    /// 2/turn (six turns) are identical under `WonderRemaining`/
    /// `ResourceRate` alone -- this is their ratio, the quantity that tells
    /// the evaluator whether starting another wonder or another building is
    /// safe or reckless given how thin production already is.
    ResourceCommitmentTurns,

    /// 1.0 if a wonder is in progress with EXACTLY one stage left, else 0.0
    /// -- design note section 3.3, "the completion cliff". A comparison
    /// against [`horizon::WonderOutlook::stages_left`]
    /// ([`features::features`]'s own `stages_left` local, already computed
    /// for [`WonderStagesLeft`](Self::WonderStagesLeft) above), never a
    /// re-scan of the hand or tableau -- `stages_left` is 0.0 both before a
    /// wonder is taken and the instant its last stage is paid (`horizon.rs`
    /// only enters its `stages_left` branch when `remaining > 0`), so this
    /// key reads 0.0 in both of those states and 1.0 in exactly the one
    /// state between them.
    ///
    /// Evaluation is of the POST-MOVE state, so the move that pays the
    /// final stage moves the vector from `(wonder_one_stage_short = 1,
    /// wonders = n)` to `(wonder_one_stage_short = 0, wonders = n+1)` in one
    /// step. A negative coefficient (sitting one stage short banks value
    /// not yet earned) turns that step into a cliff -- the completing move
    /// gains `w[wonders] + |w[wonder_one_stage_short]|` while every earlier
    /// stage only gains its own `earned_share` slice -- the identical
    /// discontinuity idiom `HasUnit`/`HasColony`/`WarImmune` already use for
    /// a rule that flips instead of accruing. Deliberately left
    /// [`SignIntent::Free`] rather than [`SignIntent::NonPositive`]: unlike
    /// the shortfall keys above, a POSITIVE weight here is informative
    /// rather than wrong (it would mean the league values being poised to
    /// finish, and the cliff should then be sought as a convex transform of
    /// `stages_left` instead) -- the design note says outright "let the
    /// climb decide; do not pick the shape by hand".
    WonderOneStageShort,

    /// `max(0, T - science_have_rate)` -- design note proposal 3.1. `T` is
    /// the MINIMUM `costs::tech_cost(state, p, id)` over every occupied
    /// `card_row` slot for which that call returns `Some` (0.0 when no
    /// slot qualifies); `science_have_rate` is [`features::features`]'s own
    /// `science_have_rate` local, the same "have + rate" read-back
    /// (`WeightKey::ScienceRate.max(0) + WeightKey::Science`) [`Self::
    /// ResourceCommitmentTurns`] already uses for the resource side.
    ///
    /// THE TRAP: every one of the 8 base-game Governments prints
    /// `science_cost: 0` (its real develop price is `Card::peaceful_cost`,
    /// RULES_SPEC 8.3) -- reading the printed field for `T` would read a
    /// Government-only row as "free to develop" and collapse this key to
    /// 0.0 the instant one sits in the row. `costs::tech_cost` handles the
    /// Government branch internally (and the standing `tech_discount` pool,
    /// the one-time develop-science discount, and the Bach/Shakespeare
    /// Theater/Library adjustments), so it is called, never restated --
    /// [`Self::HandOverCapacity`]'s own doc comment names the identical
    /// trap for the hand-affordability twin of this key.
    ///
    /// `costs::tech_cost` returns `None` for a card with no develop price
    /// at all (Despotism, the starting government whose `peaceful_cost` is
    /// 0; action cards, leaders, wonders -- all print `science_cost: 0`
    /// too). A `None` slot is FILTERED out of the minimum, never mapped to
    /// 0 -- mapping it to 0 would make Despotism sitting in the row read as
    /// "developable for nothing", pulling this key's floor down to 0.0
    /// regardless of what else is in the row.
    ///
    /// This is a SEPARATE coordinate from [`Self::ScienceGap`] on purpose:
    /// that threshold is fitted into every champion on disk today, and
    /// changing it to fold in a row-shaped term would move all of them at
    /// once. This is the gap half only -- a surplus half is not computed
    /// (see [`SignIntent::NonPositive`] below); the design note: "paired
    /// with a surplus half only if the learner needs it".
    ScienceNeedRow,

    /// The COUNT of `card_row` slots that are BOTH legally takeable right
    /// now (`costs::can_take_gated`, the CA/legality side -- row cost +
    /// completed-wonder surcharge + leader discounts against spare CA, the
    /// hand-limit gate, one-per-name, leader-age, and the
    /// wonder-in-progress gate) AND affordable to play in the near term
    /// (design note proposal 3.6) -- a bare COUNT, not a valuation: no
    /// `card_potential`, no sweep probability, no multiplying by
    /// `row_cost`. Multiplying by either of those would make this
    /// `row_pressure`, a coordinate the design note's own verification
    /// proved DISTINCT from this one for exactly that reason.
    ///
    /// The affordability half prices a levelled/tech card and a Government
    /// through [`costs::tech_cost`] (never the printed `science_cost`
    /// field -- [`Self::ScienceNeedRow`]'s own doc comment has the
    /// Government trap this dodges) against `science_have_rate`, and a
    /// Wonder through its printed first stage (`Card::stages[0]`) against
    /// `resource_have_rate`; a `None` `tech_cost` (Despotism, and every
    /// non-developable card) is NOT payable, filtered rather than mapped to
    /// 0 -- the identical C7 trap. Leader/Action cards cost nothing to
    /// play and are always payable, same as `hand_card_affordable`.
    ///
    /// Computed in the SAME shared `card_row` pass as
    /// [`Self::ScienceNeedRow`] (design note section 5's mandate: one
    /// bounded, allocation-free walk, not two) -- both keys pay for one
    /// `costs::tech_cost` call per developable row slot together, not
    /// twice over.
    RowPlayableCount,
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
    // The hinged sibling of `RivalWonders` -- `max(0, best rival's completed
    // wonder count - mine)`, the same `StrengthRel`/`StrengthDeficit` shape
    // (see `features.rs`'s own comment at the `--- rivals` emit block).
    // `RivalWonders` alone is identical on every candidate move (it does not
    // depend on this player's own action), so it drops out of the argmax;
    // netting it against MY OWN wonder count -- which candidates DO vary --
    // is the only way this fact enters the search at all.
    RivalWonderDeficit => "rival_wonder_deficit", 0.0;
    // The hinged sibling of `RivalScienceRate` -- `max(0, best rival's
    // science rate - mine)`, the same shape as `RivalWonderDeficit` above.
    // `RivalScienceRate` is measured class B (`features.rs`'s `--- rivals`
    // block never consults the candidate move) at 2p/3p/4p, so it is
    // constant across every candidate and drops out of the argmax; my own
    // rate (`WeightKey::ScienceRate`, already computed earlier in the same
    // function) DOES vary across candidates, so netting against it is the
    // only way this fact enters search at all.
    RivalScienceDeficit => "rival_science_deficit", 0.0;
    // The hinged sibling of `RivalCultureRate` -- `max(0, best rival's
    // culture rate - mine)`, the same shape as `RivalScienceDeficit` above.
    // `RivalCultureRate` is measured class B (`features.rs`'s `--- rivals`
    // block never consults the candidate move) at 2p/3p/4p, so it is
    // constant across every candidate and drops out of the argmax; my own
    // rate (`WeightKey::CultureRate`, already computed earlier in the same
    // function) DOES vary across candidates, so netting against it is the
    // only way this fact enters search at all.
    RivalCultureDeficit => "rival_culture_deficit", 0.0;
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
    ResourceDiscount => "resource_discount", 1.0;
    DefenseBonus => "defense_bonus", 0.0;
    UrbanLimit => "urban_limit", 0.0;
    GovActionCost => "gov_action_cost", 0.0;
    NoAggression => "no_aggression", 0.0;
    CardBoardCredit => "card_board_credit", 0.0;
    EventScoringMargin => "event_scoring_margin", 0.0;
    CardBoardLeader => "card_board_leader", 0.0;
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

    // need-hinged pairs -- see `NEED_KEYS` below for the honest source of
    // "which base keys get a hinge". All four gated at 0.0, for the same
    // reason the two standing hinges above are: landing this must not move a
    // single game until the climb prices it.
    FoodStockNeeded => "food_stock_needed", 0.0;
    ResourceStockNeeded => "resource_stock_needed", 0.0;
    ScienceNeeded => "science_needed", 0.0;
    FreeWorkersNeeded => "free_workers_needed", 0.0;

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

    // Both new leaf-eval coordinates: 0.0, matching every other unmeasured
    // key in this table -- this is the mechanism `parse_weights` relies on
    // (see `Weights::defaults`'s own doc comment) to keep every champion
    // file on disk, including everything under `analysis/frozen/`, playing
    // bit-identically the day these two land: a name absent from an old
    // file simply never overwrites the 0.0 seeded here.
    LeaderReplacement => "leader_replacement", 0.0;
    WonderPoolRivalClaimed => "wonder_pool_rival_claimed", 0.0;

    // 0.0, matching every other unmeasured key in this table -- a champion
    // JSON saved before this key existed keeps its default here
    // (`eval::parse_weights` starts from `Weights::defaults()`), so landing
    // it moves no existing champion until the league climbs it.
    HandOverCapacity => "hand_over_capacity", 0.0;

    // Same reasoning as HandOverCapacity immediately above: 0.0 so every
    // champion on disk today, which has neither name, keeps playing
    // bit-identically until the climb learns a value for them.
    HappyMarginAfterNextPop => "happy_margin_after_next_pop", 0.0;
    ResourceCommitmentTurns => "resource_commitment_turns", 0.0;

    // Same reasoning as the two immediately above: 0.0 so every champion on
    // disk today, which has no `wonder_one_stage_short` name at all, keeps
    // playing bit-identically until the climb learns a value for it.
    WonderOneStageShort => "wonder_one_stage_short", 0.0;

    // The final two leaf-eval coordinates (design note proposals 3.1/3.6):
    // 0.0, the same "no champion on disk names this key yet" reasoning as
    // every entry immediately above -- landing them moves no existing
    // champion's play until the climb learns a value.
    ScienceNeedRow => "science_need_row", 0.0;
    RowPlayableCount => "row_playable_count", 0.0;
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
            WeightKey::RateHorizon | WeightKey::Culture | WeightKey::CultureRate | WeightKey::Science | WeightKey::ScienceRate | WeightKey::FoodRate | WeightKey::ResourceRate | WeightKey::FoodStock | WeightKey::ResourceStock | WeightKey::BlueFree | WeightKey::CorruptionHeadroom | WeightKey::ConsumptionHeadroom | WeightKey::PopCost | WeightKey::YellowBank | WeightKey::FreeWorkers | WeightKey::Workers | WeightKey::ProdWorkers | WeightKey::UrbanWorkers | WeightKey::UnitWorkers | WeightKey::HappyMargin | WeightKey::Discontent | WeightKey::Uprising | WeightKey::CivilActions | WeightKey::MilitaryActions | WeightKey::CaLeft | WeightKey::MaLeft | WeightKey::TakeCostPaid | WeightKey::RowUrgency | WeightKey::RowBargainForgone | WeightKey::RowLastCopy | WeightKey::RivalDesire | WeightKey::RivalTakeShare | WeightKey::RivalFreeCa | WeightKey::RivalHandCivil | WeightKey::RivalWonders | WeightKey::RivalWonderDeficit | WeightKey::RivalScienceDeficit | WeightKey::RivalCultureDeficit | WeightKey::RivalHandPotential | WeightKey::RivalScienceStock | WeightKey::RivalFoodStock | WeightKey::RivalResourceStock | WeightKey::RivalFreeWorkers | WeightKey::RivalYellowBank | WeightKey::RivalColonies | WeightKey::RivalMilActions | WeightKey::RivalBuildingWonder | WeightKey::MySeededPending | WeightKey::MyEventThreat | WeightKey::AttackTargetLead | WeightKey::AttackTargetWeakness | WeightKey::PactPartnerLead | WeightKey::Strength | WeightKey::StrengthDeficit | WeightKey::StrengthLead | WeightKey::TacticLevel | WeightKey::TacticGain | WeightKey::TacticShort | WeightKey::HasUnit | WeightKey::Colonies | WeightKey::HasColony | WeightKey::Pacts | WeightKey::PactBlocksAttack | WeightKey::WarImmune | WeightKey::AttackCostDoubled | WeightKey::AuctionCommitted | WeightKey::AuctionBid | WeightKey::TechLevels | WeightKey::GovLevel | WeightKey::BestFarm | WeightKey::BestMine | WeightKey::BestLab | WeightKey::BestTemple | WeightKey::BestTheater | WeightKey::BestLibrary | WeightKey::BestArena | WeightKey::BestUnit | WeightKey::NumTechs | WeightKey::SpecialTechs | WeightKey::Wonders | WeightKey::WonderProgress | WeightKey::WonderRemaining | WeightKey::WonderStagesLeft | WeightKey::WonderTurnsToFinish | WeightKey::WonderOverrun | WeightKey::WonderStagesPerAction | WeightKey::WonderPotential | WeightKey::WonderPromise | WeightKey::WonderAgeOverrun | WeightKey::Leader | WeightKey::WonderInProgress | WeightKey::HandLimit | WeightKey::ColonizeBonus | WeightKey::BuildDiscount | WeightKey::ResourceDiscount | WeightKey::DefenseBonus | WeightKey::UrbanLimit | WeightKey::GovActionCost | WeightKey::NoAggression | WeightKey::CardBoardCredit | WeightKey::EventScoringMargin | WeightKey::CardBoardLeader | WeightKey::HandSwapExtra | WeightKey::CardRateCredit | WeightKey::UnitStrengthCredit | WeightKey::UnitTechCredit | WeightKey::TechBoardCredit | WeightKey::ActionBoardCredit | WeightKey::GovBoardCredit | WeightKey::WonderBoardCredit | WeightKey::BuildFreshCredit | WeightKey::RestrictedResourceCredit | WeightKey::FreeActionCredit | WeightKey::TerritoryCredit | WeightKey::BonusCardCredit | WeightKey::TacticBoardCredit | WeightKey::AggressionBoardCredit | WeightKey::WarBoardCredit | WeightKey::PactBoardCredit | WeightKey::EventBoardCredit | WeightKey::TacticShortfallCost | WeightKey::TacticReachCredit | WeightKey::HandCivil | WeightKey::HandValue | WeightKey::HandPotential | WeightKey::HandMilitary | WeightKey::HandMilValue | WeightKey::HandMilPotential | WeightKey::HandPerishable | WeightKey::RivalCulture | WeightKey::RivalMeanCulture | WeightKey::RivalCultureRate | WeightKey::RivalScienceRate | WeightKey::RivalStrength | WeightKey::EndTurnBias | WeightKey::CultureRateTrailing | WeightKey::ScienceRateTrailing | WeightKey::WorkersLate | WeightKey::StrengthRelEarly | WeightKey::StrengthRelLate | WeightKey::TechLevelsLate | WeightKey::HandValueLate | WeightKey::FoodGap | WeightKey::FoodSurplus | WeightKey::ResourceGap | WeightKey::ResourceSurplus | WeightKey::ScienceGap | WeightKey::ScienceSurplus | WeightKey::CultureGap | WeightKey::CultureSurplus | WeightKey::HappySurplus | WeightKey::CivilActionGap | WeightKey::CivilActionSurplus | WeightKey::TakeCostShare | WeightKey::MilitaryActionGap | WeightKey::MilitaryActionSurplus | WeightKey::WorkerGap | WeightKey::WorkerSurplus | WeightKey::TechRedundancyDiscount | WeightKey::LeaderReplacement | WeightKey::WonderPoolRivalClaimed | WeightKey::FoodStockNeeded | WeightKey::ResourceStockNeeded | WeightKey::ScienceNeeded | WeightKey::FreeWorkersNeeded | WeightKey::HandOverCapacity | WeightKey::HappyMarginAfterNextPop | WeightKey::ResourceCommitmentTurns | WeightKey::WonderOneStageShort | WeightKey::ScienceNeedRow | WeightKey::RowPlayableCount => panic!("WeightKey::early called on a key with no _early partner (only StrengthRel has one post PHASECUT.txt's T1-A/C/D collapse)"),
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
            WeightKey::RateHorizon | WeightKey::Culture | WeightKey::CultureRate | WeightKey::Science | WeightKey::ScienceRate | WeightKey::FoodRate | WeightKey::ResourceRate | WeightKey::FoodStock | WeightKey::ResourceStock | WeightKey::BlueFree | WeightKey::CorruptionHeadroom | WeightKey::ConsumptionHeadroom | WeightKey::PopCost | WeightKey::YellowBank | WeightKey::FreeWorkers | WeightKey::ProdWorkers | WeightKey::UrbanWorkers | WeightKey::UnitWorkers | WeightKey::HappyMargin | WeightKey::Discontent | WeightKey::Uprising | WeightKey::CivilActions | WeightKey::MilitaryActions | WeightKey::CaLeft | WeightKey::MaLeft | WeightKey::TakeCostPaid | WeightKey::RowUrgency | WeightKey::RowBargainForgone | WeightKey::RowLastCopy | WeightKey::RivalDesire | WeightKey::RivalTakeShare | WeightKey::RivalFreeCa | WeightKey::RivalHandCivil | WeightKey::RivalWonders | WeightKey::RivalWonderDeficit | WeightKey::RivalScienceDeficit | WeightKey::RivalCultureDeficit | WeightKey::RivalHandPotential | WeightKey::RivalScienceStock | WeightKey::RivalFoodStock | WeightKey::RivalResourceStock | WeightKey::RivalFreeWorkers | WeightKey::RivalYellowBank | WeightKey::RivalColonies | WeightKey::RivalMilActions | WeightKey::RivalBuildingWonder | WeightKey::MySeededPending | WeightKey::MyEventThreat | WeightKey::AttackTargetLead | WeightKey::AttackTargetWeakness | WeightKey::PactPartnerLead | WeightKey::Strength | WeightKey::StrengthDeficit | WeightKey::StrengthLead | WeightKey::TacticLevel | WeightKey::TacticGain | WeightKey::TacticShort | WeightKey::HasUnit | WeightKey::Colonies | WeightKey::HasColony | WeightKey::Pacts | WeightKey::PactBlocksAttack | WeightKey::WarImmune | WeightKey::AttackCostDoubled | WeightKey::AuctionCommitted | WeightKey::AuctionBid | WeightKey::GovLevel | WeightKey::BestFarm | WeightKey::BestMine | WeightKey::BestLab | WeightKey::BestTemple | WeightKey::BestTheater | WeightKey::BestLibrary | WeightKey::BestArena | WeightKey::BestUnit | WeightKey::NumTechs | WeightKey::SpecialTechs | WeightKey::Wonders | WeightKey::WonderProgress | WeightKey::WonderRemaining | WeightKey::WonderStagesLeft | WeightKey::WonderTurnsToFinish | WeightKey::WonderOverrun | WeightKey::WonderStagesPerAction | WeightKey::WonderPotential | WeightKey::WonderPromise | WeightKey::WonderAgeOverrun | WeightKey::Leader | WeightKey::WonderInProgress | WeightKey::HandLimit | WeightKey::ColonizeBonus | WeightKey::BuildDiscount | WeightKey::ResourceDiscount | WeightKey::DefenseBonus | WeightKey::UrbanLimit | WeightKey::GovActionCost | WeightKey::NoAggression | WeightKey::CardBoardCredit | WeightKey::EventScoringMargin | WeightKey::CardBoardLeader | WeightKey::HandSwapExtra | WeightKey::CardRateCredit | WeightKey::UnitStrengthCredit | WeightKey::UnitTechCredit | WeightKey::TechBoardCredit | WeightKey::ActionBoardCredit | WeightKey::GovBoardCredit | WeightKey::WonderBoardCredit | WeightKey::BuildFreshCredit | WeightKey::RestrictedResourceCredit | WeightKey::FreeActionCredit | WeightKey::TerritoryCredit | WeightKey::BonusCardCredit | WeightKey::TacticBoardCredit | WeightKey::AggressionBoardCredit | WeightKey::WarBoardCredit | WeightKey::PactBoardCredit | WeightKey::EventBoardCredit | WeightKey::TacticShortfallCost | WeightKey::TacticReachCredit | WeightKey::HandCivil | WeightKey::HandPotential | WeightKey::HandMilitary | WeightKey::HandMilValue | WeightKey::HandMilPotential | WeightKey::HandPerishable | WeightKey::RivalCulture | WeightKey::RivalMeanCulture | WeightKey::RivalCultureRate | WeightKey::RivalScienceRate | WeightKey::RivalStrength | WeightKey::EndTurnBias | WeightKey::CultureRateTrailing | WeightKey::ScienceRateTrailing | WeightKey::WorkersLate | WeightKey::StrengthRelEarly | WeightKey::StrengthRelLate | WeightKey::TechLevelsLate | WeightKey::HandValueLate | WeightKey::FoodGap | WeightKey::FoodSurplus | WeightKey::ResourceGap | WeightKey::ResourceSurplus | WeightKey::ScienceGap | WeightKey::ScienceSurplus | WeightKey::CultureGap | WeightKey::CultureSurplus | WeightKey::HappySurplus | WeightKey::CivilActionGap | WeightKey::CivilActionSurplus | WeightKey::TakeCostShare | WeightKey::MilitaryActionGap | WeightKey::MilitaryActionSurplus | WeightKey::WorkerGap | WeightKey::WorkerSurplus | WeightKey::TechRedundancyDiscount | WeightKey::LeaderReplacement | WeightKey::WonderPoolRivalClaimed | WeightKey::FoodStockNeeded | WeightKey::ResourceStockNeeded | WeightKey::ScienceNeeded | WeightKey::FreeWorkersNeeded | WeightKey::HandOverCapacity | WeightKey::HappyMarginAfterNextPop | WeightKey::ResourceCommitmentTurns | WeightKey::WonderOneStageShort | WeightKey::ScienceNeedRow | WeightKey::RowPlayableCount => panic!("WeightKey::late called on a key outside PHASE_KEYS"),
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
            WeightKey::RateHorizon | WeightKey::Culture | WeightKey::Science | WeightKey::FoodRate | WeightKey::ResourceRate | WeightKey::FoodStock | WeightKey::ResourceStock | WeightKey::BlueFree | WeightKey::CorruptionHeadroom | WeightKey::ConsumptionHeadroom | WeightKey::PopCost | WeightKey::YellowBank | WeightKey::FreeWorkers | WeightKey::Workers | WeightKey::ProdWorkers | WeightKey::UrbanWorkers | WeightKey::UnitWorkers | WeightKey::HappyMargin | WeightKey::Discontent | WeightKey::Uprising | WeightKey::CivilActions | WeightKey::MilitaryActions | WeightKey::CaLeft | WeightKey::MaLeft | WeightKey::TakeCostPaid | WeightKey::RowUrgency | WeightKey::RowBargainForgone | WeightKey::RowLastCopy | WeightKey::RivalDesire | WeightKey::RivalTakeShare | WeightKey::RivalFreeCa | WeightKey::RivalHandCivil | WeightKey::RivalWonders | WeightKey::RivalWonderDeficit | WeightKey::RivalScienceDeficit | WeightKey::RivalCultureDeficit | WeightKey::RivalHandPotential | WeightKey::RivalScienceStock | WeightKey::RivalFoodStock | WeightKey::RivalResourceStock | WeightKey::RivalFreeWorkers | WeightKey::RivalYellowBank | WeightKey::RivalColonies | WeightKey::RivalMilActions | WeightKey::RivalBuildingWonder | WeightKey::MySeededPending | WeightKey::MyEventThreat | WeightKey::AttackTargetLead | WeightKey::AttackTargetWeakness | WeightKey::PactPartnerLead | WeightKey::Strength | WeightKey::StrengthRel | WeightKey::StrengthDeficit | WeightKey::StrengthLead | WeightKey::TacticLevel | WeightKey::TacticGain | WeightKey::TacticShort | WeightKey::HasUnit | WeightKey::Colonies | WeightKey::HasColony | WeightKey::Pacts | WeightKey::PactBlocksAttack | WeightKey::WarImmune | WeightKey::AttackCostDoubled | WeightKey::AuctionCommitted | WeightKey::AuctionBid | WeightKey::TechLevels | WeightKey::GovLevel | WeightKey::BestFarm | WeightKey::BestMine | WeightKey::BestLab | WeightKey::BestTemple | WeightKey::BestTheater | WeightKey::BestLibrary | WeightKey::BestArena | WeightKey::BestUnit | WeightKey::NumTechs | WeightKey::SpecialTechs | WeightKey::Wonders | WeightKey::WonderProgress | WeightKey::WonderRemaining | WeightKey::WonderStagesLeft | WeightKey::WonderTurnsToFinish | WeightKey::WonderOverrun | WeightKey::WonderStagesPerAction | WeightKey::WonderPotential | WeightKey::WonderPromise | WeightKey::WonderAgeOverrun | WeightKey::Leader | WeightKey::WonderInProgress | WeightKey::HandLimit | WeightKey::ColonizeBonus | WeightKey::BuildDiscount | WeightKey::ResourceDiscount | WeightKey::DefenseBonus | WeightKey::UrbanLimit | WeightKey::GovActionCost | WeightKey::NoAggression | WeightKey::CardBoardCredit | WeightKey::EventScoringMargin | WeightKey::CardBoardLeader | WeightKey::HandSwapExtra | WeightKey::CardRateCredit | WeightKey::UnitStrengthCredit | WeightKey::UnitTechCredit | WeightKey::TechBoardCredit | WeightKey::ActionBoardCredit | WeightKey::GovBoardCredit | WeightKey::WonderBoardCredit | WeightKey::BuildFreshCredit | WeightKey::RestrictedResourceCredit | WeightKey::FreeActionCredit | WeightKey::TerritoryCredit | WeightKey::BonusCardCredit | WeightKey::TacticBoardCredit | WeightKey::AggressionBoardCredit | WeightKey::WarBoardCredit | WeightKey::PactBoardCredit | WeightKey::EventBoardCredit | WeightKey::TacticShortfallCost | WeightKey::TacticReachCredit | WeightKey::HandCivil | WeightKey::HandValue | WeightKey::HandPotential | WeightKey::HandMilitary | WeightKey::HandMilValue | WeightKey::HandMilPotential | WeightKey::HandPerishable | WeightKey::RivalCulture | WeightKey::RivalMeanCulture | WeightKey::RivalCultureRate | WeightKey::RivalScienceRate | WeightKey::RivalStrength | WeightKey::EndTurnBias | WeightKey::CultureRateTrailing | WeightKey::ScienceRateTrailing | WeightKey::WorkersLate | WeightKey::StrengthRelEarly | WeightKey::StrengthRelLate | WeightKey::TechLevelsLate | WeightKey::HandValueLate | WeightKey::FoodGap | WeightKey::FoodSurplus | WeightKey::ResourceGap | WeightKey::ResourceSurplus | WeightKey::ScienceGap | WeightKey::ScienceSurplus | WeightKey::CultureGap | WeightKey::CultureSurplus | WeightKey::HappySurplus | WeightKey::CivilActionGap | WeightKey::CivilActionSurplus | WeightKey::TakeCostShare | WeightKey::MilitaryActionGap | WeightKey::MilitaryActionSurplus | WeightKey::WorkerGap | WeightKey::WorkerSurplus | WeightKey::TechRedundancyDiscount | WeightKey::LeaderReplacement | WeightKey::WonderPoolRivalClaimed | WeightKey::FoodStockNeeded | WeightKey::ResourceStockNeeded | WeightKey::ScienceNeeded | WeightKey::FreeWorkersNeeded | WeightKey::HandOverCapacity | WeightKey::HappyMarginAfterNextPop | WeightKey::ResourceCommitmentTurns | WeightKey::WonderOneStageShort | WeightKey::ScienceNeedRow | WeightKey::RowPlayableCount => panic!("WeightKey::trailing called on a key outside STANDING_KEYS"),
        }
    }

    /// The need-hinged partner of a [`NEED_KEYS`] member, blended in by
    /// `rivals::feature_marginal` as `need_fraction * w[k_needed]`.
    ///
    /// The companion to [`Self::trailing`], on the other conditioning axis.
    /// `trailing` conditions a marginal on POSITION relative to the field;
    /// this one conditions it on DISTANCE FROM A RULEBOOK THRESHOLD, which
    /// is why it covers exactly the four stocks a rule converts into a cost
    /// (food feeds population, resources build, science develops, a free
    /// worker is what building consumes) and not culture, which has no such
    /// threshold -- see [`Self::FoodStockNeeded`].
    ///
    /// # Panics
    /// If `self` is not one of the [`NEED_KEYS`] members -- same reasoning as
    /// [`Self::early`]: restricting the match to exactly those is what makes
    /// it impossible to silently read a hinge for a key that is not
    /// need-multiplied.
    pub const fn needed(self) -> WeightKey {
        match self {
            WeightKey::FoodStock => WeightKey::FoodStockNeeded,
            WeightKey::ResourceStock => WeightKey::ResourceStockNeeded,
            WeightKey::Science => WeightKey::ScienceNeeded,
            WeightKey::FreeWorkers => WeightKey::FreeWorkersNeeded,
            WeightKey::RateHorizon | WeightKey::Culture | WeightKey::CultureRate | WeightKey::ScienceRate | WeightKey::FoodRate | WeightKey::ResourceRate | WeightKey::BlueFree | WeightKey::CorruptionHeadroom | WeightKey::ConsumptionHeadroom | WeightKey::PopCost | WeightKey::YellowBank | WeightKey::StrengthRel | WeightKey::Workers | WeightKey::ProdWorkers | WeightKey::UrbanWorkers | WeightKey::UnitWorkers | WeightKey::HappyMargin | WeightKey::Discontent | WeightKey::Uprising | WeightKey::CivilActions | WeightKey::MilitaryActions | WeightKey::CaLeft | WeightKey::MaLeft | WeightKey::TakeCostPaid | WeightKey::RowUrgency | WeightKey::RowBargainForgone | WeightKey::RowLastCopy | WeightKey::RivalDesire | WeightKey::RivalTakeShare | WeightKey::RivalFreeCa | WeightKey::RivalHandCivil | WeightKey::RivalWonders | WeightKey::RivalWonderDeficit | WeightKey::RivalScienceDeficit | WeightKey::RivalCultureDeficit | WeightKey::RivalHandPotential | WeightKey::RivalScienceStock | WeightKey::RivalFoodStock | WeightKey::RivalResourceStock | WeightKey::RivalFreeWorkers | WeightKey::RivalYellowBank | WeightKey::RivalColonies | WeightKey::RivalMilActions | WeightKey::RivalBuildingWonder | WeightKey::MySeededPending | WeightKey::MyEventThreat | WeightKey::AttackTargetLead | WeightKey::AttackTargetWeakness | WeightKey::PactPartnerLead | WeightKey::Strength | WeightKey::StrengthDeficit | WeightKey::StrengthLead | WeightKey::TacticLevel | WeightKey::TacticGain | WeightKey::TacticShort | WeightKey::HasUnit | WeightKey::Colonies | WeightKey::HasColony | WeightKey::Pacts | WeightKey::PactBlocksAttack | WeightKey::WarImmune | WeightKey::AttackCostDoubled | WeightKey::AuctionCommitted | WeightKey::AuctionBid | WeightKey::TechLevels | WeightKey::GovLevel | WeightKey::BestFarm | WeightKey::BestMine | WeightKey::BestLab | WeightKey::BestTemple | WeightKey::BestTheater | WeightKey::BestLibrary | WeightKey::BestArena | WeightKey::BestUnit | WeightKey::NumTechs | WeightKey::SpecialTechs | WeightKey::Wonders | WeightKey::WonderProgress | WeightKey::WonderRemaining | WeightKey::WonderStagesLeft | WeightKey::WonderTurnsToFinish | WeightKey::WonderOverrun | WeightKey::WonderStagesPerAction | WeightKey::WonderPotential | WeightKey::WonderPromise | WeightKey::WonderAgeOverrun | WeightKey::Leader | WeightKey::WonderInProgress | WeightKey::HandLimit | WeightKey::ColonizeBonus | WeightKey::BuildDiscount | WeightKey::ResourceDiscount | WeightKey::DefenseBonus | WeightKey::UrbanLimit | WeightKey::GovActionCost | WeightKey::NoAggression | WeightKey::CardBoardCredit | WeightKey::EventScoringMargin | WeightKey::CardBoardLeader | WeightKey::HandSwapExtra | WeightKey::CardRateCredit | WeightKey::UnitStrengthCredit | WeightKey::UnitTechCredit | WeightKey::TechBoardCredit | WeightKey::ActionBoardCredit | WeightKey::GovBoardCredit | WeightKey::WonderBoardCredit | WeightKey::BuildFreshCredit | WeightKey::RestrictedResourceCredit | WeightKey::FreeActionCredit | WeightKey::TerritoryCredit | WeightKey::BonusCardCredit | WeightKey::TacticBoardCredit | WeightKey::AggressionBoardCredit | WeightKey::WarBoardCredit | WeightKey::PactBoardCredit | WeightKey::EventBoardCredit | WeightKey::TacticShortfallCost | WeightKey::TacticReachCredit | WeightKey::HandCivil | WeightKey::HandValue | WeightKey::HandPotential | WeightKey::HandMilitary | WeightKey::HandMilValue | WeightKey::HandMilPotential | WeightKey::HandPerishable | WeightKey::RivalCulture | WeightKey::RivalMeanCulture | WeightKey::RivalCultureRate | WeightKey::RivalScienceRate | WeightKey::RivalStrength | WeightKey::EndTurnBias | WeightKey::CultureRateTrailing | WeightKey::ScienceRateTrailing | WeightKey::WorkersLate | WeightKey::StrengthRelEarly | WeightKey::StrengthRelLate | WeightKey::TechLevelsLate | WeightKey::HandValueLate | WeightKey::FoodGap | WeightKey::FoodSurplus | WeightKey::ResourceGap | WeightKey::ResourceSurplus | WeightKey::ScienceGap | WeightKey::ScienceSurplus | WeightKey::CultureGap | WeightKey::CultureSurplus | WeightKey::HappySurplus | WeightKey::CivilActionGap | WeightKey::CivilActionSurplus | WeightKey::TakeCostShare | WeightKey::MilitaryActionGap | WeightKey::MilitaryActionSurplus | WeightKey::WorkerGap | WeightKey::WorkerSurplus | WeightKey::TechRedundancyDiscount | WeightKey::LeaderReplacement | WeightKey::WonderPoolRivalClaimed | WeightKey::FoodStockNeeded | WeightKey::ResourceStockNeeded | WeightKey::ScienceNeeded | WeightKey::FreeWorkersNeeded | WeightKey::HandOverCapacity | WeightKey::HappyMarginAfterNextPop | WeightKey::ResourceCommitmentTurns | WeightKey::WonderOneStageShort | WeightKey::ScienceNeedRow | WeightKey::RowPlayableCount => panic!("WeightKey::needed called on a key outside NEED_KEYS"),
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
            // `RowPlayableCount` prices whether a CIVIL ACTION spent taking
            // a row card is worth it -- the same axis `CaLeft`/
            // `TakeCostShare` already live in, not `Row` (reserved for raw
            // row facts like `RowUrgency`/`RowBargainForgone`, not a count
            // of usable actions).
            | RowPlayableCount
            | MilitaryActionSurplus => {
                WeightGroup::Actions
            }

            UrbanLimit | GovActionCost | NoAggression
            | CardBoardCredit | CardBoardLeader => WeightGroup::Board,

            HandCivil | HandValue | HandValueLate | HandPotential
            // `HandPerishable` is a property OF the hand (how much of it is
            // about to expire), so it moves with the rest of the hand axis.
            | HandPerishable
            // `HandOverCapacity` is the affordability twin of `HandPerishable`
            // above -- both are properties OF the civil hand computed in the
            // same pass over it (`features.rs`), so both stay on this axis.
            | HandOverCapacity
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
            | ScienceSurplus | CultureGap | CultureSurplus | WorkerGap | WorkerSurplus
            // The need hinges, each in the SAME group as the stock it hinges
            // -- same rule as the gap/surplus pairs above, and as
            // `CultureRateTrailing` sitting beside `CultureRate`.
            // `ScienceNeedRow` is a second science shortfall (against the
            // row's cheapest developable card, not `ScienceGap`'s own
            // threshold) -- the same axis as `ScienceGap`/`ScienceNeeded`,
            // not a new one.
            | FoodStockNeeded | ResourceStockNeeded | ScienceNeeded | FreeWorkersNeeded
            | ScienceNeedRow => {
                WeightGroup::Economy
            }

            EventScoringMargin | MySeededPending | MyEventThreat => WeightGroup::Events,

            // `HappySurplus` alongside `Discontent` (its gap half) and
            // `HappyMargin` -- the same axis, not a new one. `HappyMarginAfter
            // NextPop` is the identical discontent shape one population
            // increase forward, so it joins the same axis rather than a new
            // one -- see the enum declaration's own doc comment.
            HappyMargin | Discontent | Uprising | HappySurplus
            | HappyMarginAfterNextPop => WeightGroup::Happiness,

            Strength | StrengthRel | StrengthRelEarly | StrengthRelLate | StrengthDeficit
            | StrengthLead | TacticLevel | TacticGain | TacticShort | HasUnit | Colonies
            | HasColony | Pacts | PactBlocksAttack | WarImmune | AttackCostDoubled
            | AuctionCommitted | AuctionBid => {
                WeightGroup::Military
            }

            HandLimit | ColonizeBonus | BuildDiscount | ResourceDiscount
            | DefenseBonus | CardRateCredit | UnitStrengthCredit | TerritoryCredit
            | BonusCardCredit | UnitTechCredit | TechBoardCredit | ActionBoardCredit
            | FreeActionCredit | GovBoardCredit | WonderBoardCredit | BuildFreshCredit
            | RestrictedResourceCredit | TacticBoardCredit | AggressionBoardCredit
            | WarBoardCredit | PactBoardCredit | EventBoardCredit | TacticShortfallCost
            | TacticReachCredit | TechRedundancyDiscount => WeightGroup::Priced,

            RivalCulture | RivalMeanCulture | RivalCultureRate | RivalScienceRate
            | RivalStrength | RivalFreeCa | RivalHandCivil | RivalWonders
            | RivalWonderDeficit | RivalScienceDeficit | RivalCultureDeficit
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
            | Leader | WonderInProgress
            // The two leaf-eval conditioning coordinates: both are leader/
            // wonder facts, so both join the same strategic axis as the keys
            // they split apart from.
            | LeaderReplacement | WonderPoolRivalClaimed
            // `ResourceCommitmentTurns` reads `WonderRemaining` as half its
            // numerator and prices the SAME decision -- "is starting/
            // continuing a wonder safe" -- that `WonderRemaining`/
            // `WonderOverrun`/`WonderStagesLeft` already anchor, even though
            // it also folds in the tableau's unstaffed-slot costs. See the
            // enum declaration's own doc comment.
            | ResourceCommitmentTurns
            // `WonderOneStageShort` is a comparison against `WonderStagesLeft`
            // itself, so it joins that key's own axis rather than a new one.
            | WonderOneStageShort => WeightGroup::Wonders,
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
            BuildDiscount | CardBoardCredit | DefenseBonus | HandLimit
            | ResourceDiscount | UnitStrengthCredit
            | WonderStagesPerAction => NonNegative("scales a printed benefit"),
            // A redundant card getting MORE valuable the more of its lane is
            // already covered would invert the discount's own premise, not
            // just leave a direction unmeasured. Former `REDUNDANCY_NONNEG_GATES`.
            TechRedundancyDiscount => {
                NonNegative("discounts a redundant card, never rewards one")
            }
            // The SOLE identity-aware channel pricing what completing THIS
            // in-progress wonder would do. `gains_only_sum`/
            // `gains_only_board_sum` drop every Cost-kind triple, but that
            // does NOT floor the sum at zero: `cards::push` admits a triple on
            // `amount != 0` alone and never checks its sign, so a Gain-kind
            // triple can carry a negative amount -- Kremlin (II) prints
            // `happy: -1` beside its three gains and reaches the sum as
            // `(HappyMargin, -1.0, YieldKind::Gain)`. The gate rests on what
            // the COEFFICIENT means instead: it scales an estimate of what
            // finishing this wonder is worth, so a negative one would price a
            // better wonder as worse than a poorer one. A genuinely
            // unattractive wonder is already expressed by the sum itself
            // coming out negative. Former `WONDER_VALUE_GATES`.
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
            // The four `*BoardCredit`-family keys whose pricing function is
            // provably non-negative WITHOUT auditing it for gains-only-ness:
            // each multiplies a magnitude that is already floored at zero, so
            // a negative credit could only invert an established gain.
            //
            // `pact_value`'s `best` starts at 0.0 and is only ever replaced by
            // a strictly bigger candidate -- `max(0, ...)` by construction --
            // and the credit is applied outside that floor.
            PactBoardCredit => NonNegative("scales a max(0, ..)-floored pact candidate value"),
            // Every return path of `tactic_value` is a literal 0.0 or is
            // `.max(0.0)` as the return expression itself.
            TacticBoardCredit => NonNegative("scales tactic_value, floored at 0 on every path"),
            // The one term this scales is SUBTRACTED from that value, against
            // a rules-derived shortfall count that is itself `max(0, ..)` per
            // unit type. Mirror image of the `*Gap` keys, which are
            // `NonPositive` because they are ADDED: a bigger shortfall has to
            // reduce a tactic's value, never inflate it.
            TacticShortfallCost => NonNegative("prices a shortfall count that is subtracted, never added"),
            // `cards::priced_marginal`'s restricted-resource reroute clamps
            // the marginal `.max(0.0)` before this credit multiplies it --
            // structurally identical to the `ResourceDiscount` gate above,
            // through the sibling `yield_marginal` reroute `priced_marginal`
            // delegates to for every ordinary key. `restricted_resources`
            // itself no longer needs (or has) a gate of its own: it is not a
            // `WeightKey` any more (retired, see `RETIRED_KEYS`), so there is
            // nothing here for a hill climb to have driven negative.
            RestrictedResourceCredit => {
                NonNegative("scales an already-clamped marginal, same reroute as the gate above")
            }
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
            // A COUNT of row cards immediately usable (legally takeable AND
            // affordable) -- the note's own EXPECTED SIGN: "a row I can use
            // makes spending actions good; the learner should trade it
            // against `end_turn_bias` and `ca_left`". A bigger usable-row
            // count is never worse, the same "available stock, rules never
            // subtract for it" shape as the bucket immediately above, so it
            // joins the same intent even though it lives in a different
            // `WeightGroup`.
            RowPlayableCount => NonNegative("counts row cards immediately usable; a bigger count is never worse"),
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
            | MilitaryActionGap | WorkerGap
            // `max(0, hand_civil - K)`, the identical `max(0, need - have)`
            // shortfall shape -- see the enum declaration's own doc comment.
            | HandOverCapacity
            // `max(0, -(margin after the next pop))`, the identical
            // `max(0, -margin)` shortfall `Discontent` already prices, one
            // population increase forward -- see the enum declaration's own
            // doc comment.
            | HappyMarginAfterNextPop
            // `max(0, T - science_have_rate)`, the identical shortfall
            // shape against the row's cheapest developable card instead of
            // `ScienceGap`'s own threshold -- see the enum declaration's own
            // doc comment. The gap half is live; a surplus half is not
            // computed.
            | ScienceNeedRow => NonPositive("prices a marginal-need shortfall"),
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
            // `(wonder_remaining + unstaffed printed resource costs) /
            // max(resource_rate, 1)` -- turns of production already spoken
            // for, always >= 0 by construction. Same "outstanding obligation,
            // larger the worse" shape as the wonder-debt group immediately
            // above, just measured in turns and spanning more than one
            // wonder's own debt -- see the enum declaration's own doc
            // comment.
            ResourceCommitmentTurns => {
                NonPositive("prices an outstanding resource obligation, in turns, as an upside")
            }
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
            // * `CardBoardLeader`: the effective multiplier
            //   `cards::card_potential` scales a leader swap diff by is
            //   `CardBoardCredit + <this key>`, not either term alone --
            //   `eval::dominance_repair`'s `card_board_credit_keys()` loop
            //   (itself derived from `cards::board_credit_key`, not hand
            //   copied) gates the SUM. `CardBoardGovernment`/`CardBoardAction`/
            //   `CardBoardWonder` used to sit alongside this key here; all
            //   three are RETIRED (`RETIRED_KEYS`) -- `cards::
            //   card_potential_core`'s dedicated `gov_value`/`action_value`/
            //   wonder swap-diff branches unconditionally intercept and
            //   `return` before the generic per-type path is ever reached
            //   whenever their own `GovBoardCredit`/`ActionBoardCredit`/
            //   `WonderBoardCredit` is nonzero -- which is every trained
            //   champion sampled, since the first two default nonzero and are
            //   "measured effective" -- so the three retired keys were
            //   live-looking knobs wired to nothing, the same shape
            //   `card_yields`'s own deleted static action formula was retired
            //   for (see `cards.rs::tests::card_yields_never_reprices_the_
            //   action_boards_ring_fenced_coordinates`'s doc comment for that
            //   precedent). `CardBoardBonus` used to sit alongside
            //   `CardBoardLeader` in this SAME composite-constraint bucket;
            //   it is RETIRED too as of 2026-08-24 (`RETIRED_KEYS`) for a
            //   different, stronger reason than the government/action/wonder
            //   trio -- not merely shadowed by a dedicated function, but
            //   structurally unreachable: `cards::board_credit_key(Bonus)`
            //   now answers `None`, the same bucket Government/Action/Wonder
            //   already sit in, because `credit_board`'s only two consumers
            //   (`board_yields::board_yields`'s swap diff, and
            //   `board_yields::board_extra`) never produce a nonzero result
            //   for a `CardType::Bonus` card -- see that retirement's own
            //   `RETIRED_KEYS` entry for the full proof.
            CardBoardLeader => Free,
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
            // Several of these now have a CITED counterexample rather than
            // merely a missing citation, and they are not to be re-proposed:
            // `GovLevel`/`TechLevels` (which sums `p.government.level()` in
            // directly) are blocked by Communism's `happy: -1` and
            // Fundamentalism's `science: -2`; `Colonies`/`HasColony` by Vast
            // Territory's `blueTokens: -1`; `Leader` by Sid Meier's
            // `sciencePerLab: -1`; `PactBlocksAttack` because
            // `NoAttacksBetweenParties` is symmetric and costs the holder its
            // own attack option; `TacticLevel` by RULES_SPEC 10.4, where a
            // higher-age tactic makes units 2+ levels below it outdated.
            // `StrengthLead` is the near-miss worth naming: unlike its
            // siblings it IS floored (`rel.clamp(0.0, 6.0)`) and only ever
            // added, which is the shape that earned other keys a gate -- but
            // `Special::StrongestPlayer` (RULES_SPEC 5.3) makes being ranked
            // strongest a targetable liability that nothing else here prices.
            | Workers | ProdWorkers | UrbanWorkers | UnitWorkers | HappyMargin
            | TechLevels | GovLevel | BestFarm | BestMine | BestLab
            | BestTemple | BestTheater | BestLibrary | BestArena | BestUnit | NumTechs
            | SpecialTechs | WonderProgress | Leader | Strength | StrengthRel | StrengthLead
            | TacticLevel | HasUnit | Colonies | HasColony | Pacts | PactBlocksAttack
            | WonderInProgress
            // `LeaderReplacement`: no RULES_SPEC citation makes holding a
            // REPLACEMENT leader rather than a first one strictly good or
            // bad -- it inherits every counterexample `Leader` itself
            // already carries (Sid Meier's negative `sciencePerLab` attaches
            // to whichever leader is held), and a replacement additionally
            // means the OLD leader's benefit is gone, a loss on some boards.
            // No citation supports a floor or a ceiling either way.
            | LeaderReplacement
            // `WonderPoolRivalClaimed`: no citation makes "more of this
            // age's wonder pool claimed by RIVALS" uniformly good or bad --
            // it shrinks the evaluated player's own remaining build options
            // (bad) but can also mean a rival sank cost into a wonder this
            // player never wanted (irrelevant or good). The direction only
            // exists once combined with who is ahead in that age's race,
            // which this single coordinate does not measure.
            | WonderPoolRivalClaimed => Free,
            // The military twin of the `CivilActions` gate above, on the same
            // citations: `features.rs` sets it from `s.military_actions`, the
            // government-derived allowance for the turn, which RULES_SPEC 3
            // never compels a player to spend. Every mechanic reading the
            // total only benefits from more of it -- the military hand limit
            // IS the MA total (6.7), and defence capacity scales with it
            // (5.4). `cards::unit_type_reach_cost` already hard-clamps this
            // very weight with `.max(0.0)` beside the identical clamp on
            // `CivilActions`, so the invariant was already assumed in code
            // before it was stated here.
            MilitaryActions => NonNegative("prices an action allowance the rules never compel spending"),
            // Two purely PROTECTIVE booleans. `WarImmune`
            // (`cannotBeDeclaredWarOnByAnyone`, RULES_SPEC 5.6) only ever
            // enters combat legality on the ATTACKER's side, removing no
            // option from its holder; the one pact granting it also carries
            // `cultureProduction: -2`, but that is priced by the culture
            // coordinates and floored at 0 by the rulebook's rating limits.
            // `AttackCostDoubled` reads the DEFENDER's leader in
            // `combat::start_aggression` -- it means "rivals pay double to
            // attack me", not "my attacks cost double"; Gandhi's matching
            // self-restriction is `NoAggression` above.
            WarImmune | AttackCostDoubled => {
                NonNegative("a protective flag that removes no option from its holder")
            }
            // `CorruptionHeadroom`/`ConsumptionHeadroom`: `features.rs`'s own
            // comment on these two, verbatim -- "headroom is a deterministic
            // function of `BlueFree`/`YellowBank`, so 'good all else equal'
            // is vacuous here -- the two coordinates cannot move
            // independently, and the league is left to price the pair."
            CorruptionHeadroom | ConsumptionHeadroom => Free,
            // `WonderOneStageShort`: the enum declaration's own doc comment
            // has the full derivation -- design note section 3.3 expects
            // negative (a completion cliff, banking value not yet earned)
            // but says outright that a POSITIVE weight would be
            // INFORMATIVE rather than wrong, and the cliff should then be
            // sought as a convex transform of `stages_left` instead. No
            // citation forces either sign; the league is left to price it.
            WonderOneStageShort => Free,
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
            //
            // `RivalWonderDeficit` is the hinged `max(0, rival_wonders -
            // my_wonders)`, the same shape as `StrengthDeficit`, but it does
            // NOT inherit that key's gate: `StrengthDeficit` is NonPositive
            // because RULES_SPEC's combat chapter ties relative military
            // weakness to a rules-imposed outcome (losing fights). No
            // citation ties trailing a rival's completed-wonder COUNT to any
            // rules-imposed penalty the same way -- a player who spent
            // civil actions elsewhere instead of racing wonders is not
            // punished by a rule for it, and `WonderPoolRivalClaimed` (this
            // same file, above) already carries the identical "no
            // established direction" verdict for the neighboring "wonders a
            // rival claimed" fact.
            //
            // `RivalScienceDeficit` is the same `max(0, rival_science_rate -
            // my ScienceRate)` shape, netting `RivalScienceRate` (already
            // `Free`, two lines below) against my own rate. RULES_SPEC has
            // no combat-style chapter for science production the way it does
            // for military strength -- a trailing science rate costs the
            // player nothing by rule, only by the opportunity a faster rival
            // gets to reach later techs first, which is exactly the
            // strategic race `WonderPoolRivalClaimed` and `RivalWonderDeficit`
            // are already classified `Free` for.
            //
            // `RivalCultureDeficit` is the same `max(0, rival_culture_rate -
            // my CultureRate)` shape, netting `RivalCultureRate` (already
            // `Free`, three lines below) against my own rate. Culture scores
            // points and paces age advancement, but no RULES_SPEC chapter
            // ties trailing a rival's culture RATE to a rules-imposed
            // outcome the way the combat chapter does for military strength
            // -- it costs only the same kind of opportunity
            // `RivalScienceDeficit` immediately above already reads as
            // `Free`.
            RivalFreeCa | RivalHandCivil | RivalWonders | RivalWonderDeficit
            | RivalScienceDeficit | RivalCultureDeficit | RivalHandPotential
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
            // `s.colonize` is the printed colonization-force bonus, and the
            // card data prints only 1..=4 for it -- no card anywhere in
            // `data/` prints a negative colonization bonus. Its sole consumer
            // adds it to the player's bid force (`interact.rs`), so a bigger
            // bonus is strictly more force at the same cost.
            ColonizeBonus => NonNegative("scales a printed colonization-force bonus, never a malus"),
            // Mahatma Gandhi's flag, and the name reads backwards: this is the
            // DOWNSIDE half of the leader. `legal.rs` uses it to strip the
            // holder's OWN `Move::Aggression` and `Move::War` from its legal
            // set; the upside half (rivals paying double to attack) is
            // `AttackCostDoubled`, priced separately. Strictly fewer legal
            // moves at the same board state is never better, so the
            // coefficient may not reward the flag. Web-checked against the
            // published card text, which agrees the restriction is
            // self-imposed.
            NoAggression => NonPositive("strips the holder's own aggression and war moves"),
            // `s.urban_limit` is a deterministic function of which government
            // the player holds -- the eight base-game governments span exactly
            // {2, 3, 4} -- and `GovLevel` already prices the government. The
            // two coordinates cannot move independently, so "a bigger urban
            // limit, all else equal" is not a reachable comparison and a
            // negative here is a legitimate correction rather than an
            // inversion. Same reasoning as `CorruptionHeadroom`/
            // `ConsumptionHeadroom` below.
            UrbanLimit => Free,
            // `docs/OPEN_ITEMS.md` item 1: a real, live-reading coordinate
            // whose drift is documented as "signal vs noise, not yet
            // determined" -- explicitly not concluded to be rules-forced
            // either way. Briefly RETIRED 2026-08-24 on the reasoning that
            // its only reader is unreachable whenever `gov_board_credit !=
            // 0.0`, true on every SAMPLED champion; un-retired the same day
            // once gating `GovBoardCredit` itself `NonNegative` a few lines
            // below made `gov_board_credit == 0.0` a live, reachable POST-
            // REPAIR state (the 4p champion's `-0.1722` gets raised to
            // exactly `0.0` on every load) -- see the enum declaration's own
            // comment on this variant for the full account, and
            // `NonNegative`'s doc comment on the trust-multiplier block
            // below for the general hazard.
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
            // characterization). A PRIOR pass here reasoned that because
            // `tech_value`/`gov_value`/`sum_board_triples` compute NET values
            // -- gains minus real printed and rules costs, unclamped -- "a
            // bad card can legitimately price negative", and left all
            // thirteen `Free` on that basis. That reasoning is a category
            // error: these keys are not the value, they are the SCALE
            // multiplying it. A signed inner function is exactly why the
            // scale must be constrained, not a reason to free it -- a
            // negative scale does not let a bad card price negative (the
            // inner function already does that), it INVERTS good and bad,
            // scrambling the ranking among every card the dedicated function
            // prices. `eval::dominance_repair` already accepts precisely
            // this argument one family over, for `CardBoardCredit`'s
            // per-type offsets: "a negative EFFECTIVE scale ... does not
            // mis-price one card, it inverts the entire ranking of that card
            // TYPE", added after the leadersign investigation priced
            // Hammurabi at -13.28 on a +0.885 raw board benefit.
            //
            // Re-audited 2026-08-24 (analysis/multiplier_decisiveness_all_
            // counts_2026-08-24.txt) per key, not by category: eleven of the
            // thirteen are a pure `w.get(key) * <dedicated function's
            // signed output>` scale with no other role and are gated
            // `NonNegative` below. The remaining two are NOT: `CardRateCredit`
            // scales `CardEffects.culture`/`.science` (`sum_yields`'s
            // `YieldKind::Rate` branch), and `BonusCardCredit` scales
            // `CardEffects.defense_bonus - 1`/`.colonization_bonus`
            // (`sum_yields`'s `YieldKind::Bonus` branch) -- both printed,
            // per-card magnitudes that are never negative anywhere in
            // `card_table.rs`'s base-game data (checked directly, not
            // inferred), so gating them would be a guess this audit's
            // measurement does not support. They stay `Free`.
            CardRateCredit | BonusCardCredit => Free,
            // The eleven PROVEN multipliers: each is `w.get(key) *
            // <dedicated function's signed output>` with no other use site.
            // `UnitTechCredit`/`TechBoardCredit`/`ActionBoardCredit`/
            // `GovBoardCredit`/`WonderBoardCredit` scale
            // `tech_value`/`action_value`/`gov_value`/the wonder swap diff
            // directly (`cards::card_potential_core`'s dispatch, each an
            // unconditional `return credit * dedicated_fn(...)`).
            // `AggressionBoardCredit`/`WarBoardCredit`/`EventBoardCredit`
            // scale `aggression_value`/`war_hand_value`/
            // `event_prepare_value` the identical way. `TacticReachCredit`
            // scales `(potential * strength_marginal - cost)` in
            // `tactic_value`'s reach branch -- clamped to `>= 0.0` only
            // AFTER the scale is applied, so a negative credit still lets a
            // reach-cost-exceeds-benefit tactic (which should price at 0)
            // read as a fabricated positive. `BuildFreshCredit` scales
            // `b_net` (`tech_value`'s build-fresh alternative, itself signed:
            // a resource cost term minus staffing gains) before a `max`
            // against the other staffing plan -- a negative credit can make
            // the WORSE plan win the max. `TerritoryCredit` scales every
            // `YieldKind::Territory` triple's printed amount
            // (`sum_yields`/`gains_only_sum`) -- the previous version of
            // this very comment cited its own counterexample (Vast
            // Territory (I)/(II) print `blue_tokens: -1`) as a reason to
            // leave it `Free`; that signed printed amount is exactly what
            // makes the scale need gating, not what excuses it.
            //
            // THE HAZARD THIS GATE CARRIES: in `cards::card_potential_core`,
            // `UnitTechCredit`/`TechBoardCredit`/`ActionBoardCredit`/
            // `GovBoardCredit`/`WonderBoardCredit`/`AggressionBoardCredit`/
            // `WarBoardCredit`/`EventBoardCredit` are dispatched as `if
            // credit != 0.0 { return credit * <dedicated function>(...) }`.
            // `0.0` is therefore NOT a neutral value for these eight the way
            // it is for an ordinary additive term -- it is a DISPATCH
            // SWITCH. Raising a negative violator to exactly `0.0` (what
            // `dominance_repair` does for a `NonNegative` gate) does not
            // merely drop the dedicated function's contribution to zero; it
            // routes that entire card TYPE to a DIFFERENT pricing
            // implementation (the generic `card_board_credit` fallback, or
            // the static `card_yields` table). A repair is only safe if
            // that fallback independently prices everything the dedicated
            // function priced. `GovBoardCredit` is the one to watch: its
            // fallback runs through `board_yields::government_cost`, and a
            // champion carrying a negative `gov_board_credit` (the 4p one
            // does) is repaired to exactly `0.0` on every load, so that
            // fallback is REACHED IN PRODUCTION and its civil-action cost
            // term has to stay live -- see `GovActionCost`'s own doc comment
            // above. `BuildFreshCredit`/`TerritoryCredit`/
            // `TacticReachCredit` do NOT share this dispatch-switch shape --
            // each is a plain multiplicative scale inline in its own
            // function (no alternate code path to lose), so `0.0` there is
            // an ordinary, safe "no credit" state, the same as their own
            // authored defaults.
            UnitTechCredit | TechBoardCredit | ActionBoardCredit | GovBoardCredit
            | WonderBoardCredit | BuildFreshCredit | TerritoryCredit
            | AggressionBoardCredit | WarBoardCredit | EventBoardCredit
            | TacticReachCredit => {
                NonNegative("scales a dedicated function's signed output; a negative scale inverts good and bad rather than repricing one card")
            }
            // The exact military twin of the `HandValue` gate above:
            // `features.rs` sums `level + 1` over the military hand, so every
            // term is >= 1 and the feature can never go below zero. A card
            // sitting in hand is never a rules-level cost -- §2.5's hand limit
            // is enforced by `costs::can_take` making the take ILLEGAL, so a
            // full hand is a legality constraint and can never show up as an
            // evaluation penalty.
            HandMilValue => NonNegative("sums level + 1 over the military hand, so the feature is always >= 0"),
            // `HandCivil`/`HandMilitary` are RESIDUALS, not independent
            // quantities: `hand_value` is `hand_civil` count plus the sum of
            // levels, and `hand_mil_value` decomposes the same way, so the
            // count is the intercept term of a feature already gated above and
            // may legitimately price as a negative correction. `HandMilitary`
            // additionally reaches `events.rs` multiplied by a `sign` that is
            // negative when the event hurts its target, so it is not a
            // one-directional magnitude there either.
            //
            // `HandPotential`/`HandMilPotential` price hand cards by what
            // playing them would do, and that value is not floored: Vast
            // Territory (I and II) prints `blueTokens: -1` inside its yields,
            // so a card in hand can price negative on real card data.
            HandCivil | HandPotential | HandMilitary | HandMilPotential => Free,
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
            // The need hinges (`NEED_KEYS`), `Free` for the same reason and
            // one of its own. "Short of X makes X worth more" is the
            // intuition, but it is an intuition, not a rules citation: no
            // rule says a marginal food is worth MORE at 0 food than at 4
            // when the population costs 5, and there is a real argument the
            // other way (a player who cannot reach the threshold this turn
            // gains nothing from creeping toward it and should spend the
            // action elsewhere). The gates in this function exist for signs
            // the RULEBOOK forces; a gate that only encodes a plausible
            // reading would silently clamp whatever the league finds. Which
            // way these four point is exactly what the hinge exists to
            // answer.
            FoodStockNeeded | ResourceStockNeeded | ScienceNeeded | FreeWorkersNeeded => Free,
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

    /// The magnitude ceiling this coordinate may not exceed at `players`
    /// players.
    ///
    /// The league used to hold every one of the 162 weights to the same flat
    /// `60.0`, which is not one rule but 162 different rules wearing a
    /// costume: what a coefficient of 60 DOES depends entirely on how far
    /// the feature it scales swings between the moves on offer. `culture`
    /// swings about 13 points between candidates at two players, so 60 on it
    /// moves the score by ~780. `take_cost_share` swings 0.8, so 60 on it
    /// moves the score by 48. The same rail let one coordinate dominate
    /// every decision it touched and left another unable to matter at all,
    /// and a coordinate that fires on 2% of decisions could carry a weight
    /// fitted entirely to noise on the other 98% -- the defect this method
    /// exists to close.
    ///
    /// The bound is therefore stated in units of WHOLE DECISIONS. Divide the
    /// p95 swing of the full evaluation score across a decision's candidate
    /// set ([`P95_TOTAL_SPREAD`]) by this key's own p95 swing when it fires
    /// at all ([`Self::p95_candidate_spread`]) and no single coordinate can
    /// command more than [`CLAMP_T`] whole typical decisions on its own.
    /// Both quantities are MEASURED, by `bin/featspread`; neither is chosen.
    ///
    /// Two deliberate conservatisms:
    ///
    /// - The result is capped at [`CLAMP_BLIND`], the historical flat rail,
    ///   so this can only ever TIGHTEN a coordinate and never hand one more
    ///   room than it has today. 80 of the 365 measurable (key, count) pairs
    ///   tighten; the rest are held where they already were.
    /// - A key reading `0.0` in the p95 row is not "measured zero"; it is
    ///   INVISIBLE TO THE INSTRUMENT, and invisibility comes in four
    ///   distinct kinds (which is why the row is prose here and never
    ///   re-derived from the table):
    ///
    ///   1. The hinge keys. Their coordinates are read inside
    ///      [`super::rivals::feature_marginal`] as a multiplier of a
    ///      STATE-ONLY fraction in `[0.0, 1.0]` -- the trailing/need
    ///      fractions carry no candidate set of their own -- so the
    ///      "p95 swing across the candidate set, over the decisions where
    ///      the key fires" that every other row measures is not defined
    ///      for them. The meaningful quantity is how often they fire, and
    ///      that is already measured (the 2026-08-24 `multcheck` 4p
    ///      run's `term_nonzero` rates, e.g. the culture trailing hinge at
    ///      0.2045 against a flip rate of 0.000073).
    ///   2. The rate horizon key. Same category error for its own row: its
    ///      coordinate is the multiplier of the four rate keys inside
    ///      [`super::horizon::rate_multiplier`], and the rates carry no
    ///      candidate set either. The meaningful quantity is the fire rate
    ///      of a horizon scale different from 1.0.
    ///   3. The credit keys. NOT a category error: their zero is an
    ///      unmeasured-but-definable gap. The credit multiplies a per-card
    ///      value that depends on board state, not on the weight vector,
    ///      so with the pricer's inner sub-pricing frozen at the champion
    ///      (the same freeze discipline as the identity-aware gates in
    ///      [`super::eval`]), the candidate-set swing of that value is a
    ///      p95-spread-shaped quantity in the same units as every measured
    ///      row. MEASURED on 2026-08-26 by `bin/creditspread.rs` and landed:
    ///      the credit rows above are its readings, not `featspread`'s.
    ///      Its probe DISPLACES the key from the champion's own value
    ///      (`w_k + c`, `c` = 1.0 and 2.0) rather than setting it to a fixed
    ///      absolute `c`; that distinction is the whole measurement. A
    ///      set-to-`c` probe measures `h(c) - h(w_k)`, so its linearity test
    ///      is a second-difference test over a stretch of the weight axis
    ///      the champion never occupies, and it passes if and only if
    ///      `w_k == 0.0`. Two confident negative results were produced that
    ///      way before the probe was fixed; see
    ///      `analysis/credit_spread_finding_2026-08-26.txt`.
    ///      Rows still at zero are the ones this instrument could not see
    ///      either: `card_rate_credit` (never fires, at any count) and 3p
    ///      `tech_board_credit`/`wonder_board_credit` (gated). They keep
    ///      [`CLAMP_BLIND`].
    ///   4. The rival-context keys. A genuinely different zero: they ARE
    ///      written by [`super::features`] into the linear feature vector,
    ///      so the row is well-defined for them -- it reads 0.0 only
    ///      because the self-play sample produced no spread (rival context
    ///      is often identical across the candidates on offer). A plain
    ///      `featspread` rerun can fill them; if one still reads 0.0
    ///      after, `featspread`'s decisive mode separates structurally
    ///      dead from alive-but-quiet.
    ///
    ///   In every case the fallback is the flat rail rather than an
    ///   invented number. Several of these rows carry live champion weights
    ///   between 6 and 27 (the 3p `tech_board_credit` sits at -27.05),
    ///   which is why "invisible" is a statement about the instrument,
    ///   never about the coordinate.
    ///
    /// The bound is per (key, PLAYER COUNT) and never collapsed across
    /// counts. `hand_potential` swings 815 between candidates at three
    /// players and 11.5 at two; a single cross-count bound for it is off by
    /// seventy-fold at one end or the other, and taking the maximum is
    /// exactly the arithmetic that once produced a reported "107x runaway"
    /// in a healthy 2p coordinate.
    pub fn clamp_bound(self, players: u8) -> f64 {
        let spread = self.p95_candidate_spread()[player_index(players)];
        if spread <= 0.0 {
            return CLAMP_BLIND;
        }
        let bound = CLAMP_T * P95_TOTAL_SPREAD[player_index(players)] / spread;
        bound.min(CLAMP_BLIND)
    }

    /// This key's own p95 candidate-set spread, over the decisions where it
    /// moves at all, at 2/3/4 players -- the denominator of
    /// [`Self::clamp_bound`].
    ///
    /// MEASURED, not authored. Regenerate the whole body with
    /// `featspread <games> <seed> <champion_dir> emit`, which prints these
    /// arms and [`P95_TOTAL_SPREAD`] as compilable Rust from the same values
    /// its report prints. Do not edit a number here by hand: hand-transcribing
    /// this table out of a text report is how the 107x error happened.
    ///
    /// These are a property of the CHAMPION as much as of the game -- the
    /// sample is that champion's own self-play, and `linear_features` prices
    /// the helper keys at it. Rerunning after a few thousand generations of
    /// climbing moves the noisier rows by tens of percent, which is fine for
    /// a safety rail and would not be fine for a fitted parameter. It is a
    /// rail.
    fn p95_candidate_spread(self) -> [f64; 3] {
    match self {
        WeightKey::RateHorizon => [0.000000, 0.000000, 0.000000],
        WeightKey::Culture => [9.000000, 8.000000, 8.000000],
        WeightKey::CultureRate => [2.616341, 2.494558, 2.061760],
        WeightKey::Science => [11.000000, 6.000000, 8.000000],
        WeightKey::ScienceRate => [3.032107, 2.322596, 3.054936],
        WeightKey::FoodRate => [3.638703, 2.583029, 2.939278],
        WeightKey::ResourceRate => [5.716622, 4.110245, 3.121540],
        WeightKey::FoodStock => [4.000000, 4.000000, 5.000000],
        WeightKey::ResourceStock => [6.000000, 5.000000, 6.000000],
        WeightKey::BlueFree => [7.000000, 7.000000, 6.000000],
        WeightKey::CorruptionHeadroom => [4.000000, 4.000000, 4.000000],
        WeightKey::ConsumptionHeadroom => [3.000000, 3.000000, 3.000000],
        WeightKey::PopCost => [1.000000, 1.000000, 2.000000],
        WeightKey::YellowBank => [2.000000, 2.000000, 2.000000],
        WeightKey::FreeWorkers => [2.000000, 2.000000, 2.000000],
        WeightKey::Workers => [1.644737, 1.458824, 1.296089],
        WeightKey::ProdWorkers => [2.000000, 2.000000, 2.000000],
        WeightKey::UrbanWorkers => [2.000000, 2.000000, 1.000000],
        WeightKey::UnitWorkers => [2.000000, 2.000000, 2.000000],
        WeightKey::HappyMargin => [2.000000, 2.000000, 2.000000],
        WeightKey::Discontent => [1.000000, 1.000000, 2.000000],
        WeightKey::Uprising => [15.000000, 15.000000, 15.000000],
        WeightKey::CivilActions => [3.000000, 2.000000, 3.000000],
        WeightKey::MilitaryActions => [3.000000, 2.000000, 2.000000],
        WeightKey::CaLeft => [4.000000, 5.000000, 6.000000],
        WeightKey::MaLeft => [3.000000, 3.000000, 3.000000],
        WeightKey::TakeCostPaid => [4.000000, 4.000000, 4.000000],
        WeightKey::RowUrgency => [150.624847, 138.595198, 321.148062],
        WeightKey::RowBargainForgone => [8.000000, 4.440252, 5.000000],
        WeightKey::RowLastCopy => [9.214286, 5.000000, 7.000000],
        WeightKey::RivalDesire => [0.000000, 0.000000, 0.000000],
        WeightKey::RivalTakeShare => [0.000000, 0.000000, 0.000000],
        WeightKey::RivalFreeCa => [0.000000, 2.000000, 4.000000],
        WeightKey::RivalHandCivil => [4.000000, 3.000000, 3.000000],
        WeightKey::RivalWonders => [0.000000, 0.000000, 0.000000],
        // Measured: `featspread 40 0 <frozen champions> emit`, 2026-08-27,
        // against `rust_champion_{2,3,4}p.json` frozen under
        // `analysis/rivalwonderdeficit_champions/`. Fires on 1.24%/1.14%/
        // 2.13% of decisions at 2p/3p/4p (rare -- only when candidates
        // differ in whether THIS move completes a wonder while a rival
        // already leads), p95 spread exactly 1.0 at every count when
        // firing -- unlike `RivalWonders` immediately above (measured
        // [0,0,0], identical on every candidate by construction), the hinge
        // genuinely enters the argmax.
        WeightKey::RivalWonderDeficit => [1.000000, 1.000000, 1.000000],
        // Measured: `featspread 40 0 <frozen champions> emit`, 2026-08-27,
        // against `rust_champion_{2,3,4}p.json` frozen under
        // `analysis/rivalsciencedeficit_champions/`. Fires on 21.37%/35.77%/
        // 44.03% of decisions at 2p/3p/4p -- far more than `RivalWonderDeficit`
        // above (1-2%), because a science-rate gap between players is
        // continuous and common, unlike a rare wonder-completion race. p95
        // spread 1.0/2.0/2.0, mean spread when firing 1.01/1.06/1.17: the
        // hinge genuinely enters the argmax at every count.
        WeightKey::RivalScienceDeficit => [1.000000, 2.000000, 2.000000],
        // Placeholder -- measured and replaced with `featspread`'s emit row
        // in the follow-up pass (see the commit that lands this key).
        WeightKey::RivalCultureDeficit => [0.000000, 0.000000, 0.000000],
        WeightKey::RivalHandPotential => [147.637444, 110.078904, 11.859491],
        WeightKey::RivalScienceStock => [6.000000, 5.000000, 5.000000],
        WeightKey::RivalFoodStock => [4.000000, 3.000000, 3.000000],
        WeightKey::RivalResourceStock => [5.000000, 4.000000, 4.000000],
        WeightKey::RivalFreeWorkers => [1.000000, 1.000000, 1.000000],
        WeightKey::RivalYellowBank => [4.000000, 2.000000, 2.000000],
        WeightKey::RivalColonies => [1.000000, 1.000000, 1.000000],
        WeightKey::RivalMilActions => [2.000000, 2.000000, 2.000000],
        WeightKey::RivalBuildingWonder => [1.000000, 1.000000, 3.000000],
        WeightKey::MySeededPending => [3.000000, 3.000000, 3.000000],
        WeightKey::MyEventThreat => [16.000000, 368.000000, 23.015989],
        WeightKey::AttackTargetLead => [0.000000, 92.000000, 108.000000],
        WeightKey::AttackTargetWeakness => [11.000000, 16.000000, 14.000000],
        WeightKey::PactPartnerLead => [0.000000, 110.000000, 120.000000],
        WeightKey::Strength => [6.000000, 5.000000, 6.000000],
        WeightKey::StrengthRel => [6.000000, 5.000000, 6.000000],
        WeightKey::StrengthDeficit => [4.000000, 5.000000, 5.000000],
        WeightKey::StrengthLead => [4.000000, 4.000000, 5.000000],
        WeightKey::TacticLevel => [2.000000, 3.000000, 2.000000],
        WeightKey::TacticGain => [4.000000, 4.000000, 4.000000],
        WeightKey::TacticShort => [2.000000, 2.000000, 2.000000],
        WeightKey::HasUnit => [1.000000, 1.000000, 1.000000],
        WeightKey::Colonies => [1.000000, 1.000000, 1.000000],
        WeightKey::HasColony => [1.000000, 1.000000, 1.000000],
        WeightKey::Pacts => [0.000000, 1.500000, 1.500000],
        WeightKey::PactBlocksAttack => [0.000000, 1.500000, 1.000000],
        WeightKey::WarImmune => [0.000000, 1.000000, 1.000000],
        WeightKey::AttackCostDoubled => [1.000000, 1.000000, 1.000000],
        WeightKey::AuctionCommitted => [1.000000, 1.000000, 1.000000],
        WeightKey::AuctionBid => [10.000000, 23.000000, 25.000000],
        WeightKey::TechLevels => [0.769737, 0.705882, 0.698324],
        WeightKey::GovLevel => [3.000000, 2.000000, 2.000000],
        WeightKey::BestFarm => [0.000000, 0.000000, 3.000000],
        WeightKey::BestMine => [0.000000, 0.000000, 2.000000],
        WeightKey::BestLab => [0.000000, 0.000000, 2.000000],
        WeightKey::BestTemple => [2.000000, 0.000000, 0.000000],
        WeightKey::BestTheater => [3.000000, 0.000000, 0.000000],
        WeightKey::BestLibrary => [3.000000, 0.000000, 0.000000],
        WeightKey::BestArena => [3.000000, 0.000000, 0.000000],
        WeightKey::BestUnit => [3.000000, 2.000000, 3.000000],
        WeightKey::NumTechs => [1.000000, 1.000000, 1.000000],
        WeightKey::SpecialTechs => [1.000000, 1.000000, 1.000000],
        WeightKey::Wonders => [1.000000, 1.000000, 1.000000],
        WeightKey::WonderProgress => [9.000000, 8.000000, 8.000000],
        WeightKey::WonderRemaining => [16.000000, 16.000000, 16.000000],
        WeightKey::WonderStagesLeft => [4.000000, 5.000000, 5.000000],
        WeightKey::WonderTurnsToFinish => [5.000000, 5.500000, 5.000000],
        WeightKey::WonderOverrun => [6.500000, 12.528100, 6.500000],
        WeightKey::WonderStagesPerAction => [1.000000, 2.000000, 2.000000],
        WeightKey::WonderPotential => [12.567994, 78.700422, 7.252326],
        WeightKey::WonderPromise => [14.347283, 71.709601, 9.545458],
        WeightKey::WonderAgeOverrun => [5.578059, 11.902926, 5.617754],
        WeightKey::Leader => [1.000000, 1.000000, 1.000000],
        WeightKey::WonderInProgress => [1.000000, 1.000000, 1.000000],
        WeightKey::HandLimit => [2.000000, 2.000000, 2.000000],
        WeightKey::ColonizeBonus => [4.000000, 3.000000, 1.000000],
        WeightKey::BuildDiscount => [3.000000, 5.000000, 5.000000],
        WeightKey::ResourceDiscount => [0.000000, 0.000000, 0.000000],
        WeightKey::DefenseBonus => [0.000000, 0.000000, 0.000000],
        WeightKey::UrbanLimit => [2.000000, 1.000000, 1.000000],
        WeightKey::GovActionCost => [0.000000, 0.000000, 0.000000],
        WeightKey::NoAggression => [1.000000, 1.000000, 1.000000],
        WeightKey::CardBoardCredit => [440.293944, 257.969239, 555.766676],
        WeightKey::EventScoringMargin => [12.000000, 14.000000, 12.000000],
        WeightKey::CardBoardLeader => [444.221547, 75.852318, 639.120835],
        WeightKey::HandSwapExtra => [0.000000, 0.000000, 0.000000],
        WeightKey::CardRateCredit => [0.000000, 0.000000, 0.000000],
        WeightKey::UnitStrengthCredit => [0.000000, 0.000000, 503.905097],
        WeightKey::UnitTechCredit => [157.532462, 12.531155, 796.642824],
        WeightKey::TechBoardCredit => [141.213174, 0.000000, 340.149556],
        WeightKey::ActionBoardCredit => [30.951381, 60.270880, 8.980789],
        WeightKey::GovBoardCredit => [103.608714, 40.765199, 175.810309],
        WeightKey::WonderBoardCredit => [5.008904, 0.000000, 35.990065],
        WeightKey::BuildFreshCredit => [5.088772, 163.578218, 26.668300],
        WeightKey::RestrictedResourceCredit => [1.222799, 12.011099, 3.456802],
        WeightKey::FreeActionCredit => [1.358827, 0.006957, 0.027570],
        WeightKey::TerritoryCredit => [1.153157, 13.357448, 63.607113],
        WeightKey::BonusCardCredit => [0.323400, 0.058798, 1.135809],
        WeightKey::TacticBoardCredit => [0.711767, 4.537967, 247.104292],
        WeightKey::AggressionBoardCredit => [0.376610, 7.967209, 2.273367],
        WeightKey::WarBoardCredit => [0.367623, 0.401146, 3.999759],
        WeightKey::PactBoardCredit => [0.000000, 1.955373, 26.975213],
        WeightKey::EventBoardCredit => [0.117639, 0.071633, 0.974300],
        WeightKey::TacticShortfallCost => [0.000000, 0.000000, 0.000000],
        WeightKey::TacticReachCredit => [0.781131, 0.211469, 24.407310],
        WeightKey::HandCivil => [2.000000, 2.000000, 2.000000],
        WeightKey::HandValue => [2.789474, 2.847059, 2.860335],
        WeightKey::HandPotential => [197.027878, 674.348432, 304.224789],
        WeightKey::HandMilitary => [4.000000, 4.000000, 4.000000],
        WeightKey::HandMilValue => [11.000000, 12.000000, 11.000000],
        WeightKey::HandMilPotential => [185.417899, 240.000000, 780.000000],
        WeightKey::HandPerishable => [1.431818, 1.392328, 1.393517],
        WeightKey::RivalCulture => [30.000000, 34.000000, 38.000000],
        WeightKey::RivalMeanCulture => [30.000000, 26.500000, 27.666667],
        WeightKey::RivalCultureRate => [0.000000, 1.000000, 1.000000],
        WeightKey::RivalScienceRate => [0.000000, 0.000000, 0.000000],
        WeightKey::RivalStrength => [0.000000, 2.000000, 2.000000],
        WeightKey::EndTurnBias => [1.000000, 1.000000, 1.000000],
        WeightKey::CultureRateTrailing => [0.000000, 0.000000, 0.000000],
        WeightKey::ScienceRateTrailing => [0.000000, 0.000000, 0.000000],
        WeightKey::FoodStockNeeded => [0.000000, 0.000000, 0.000000],
        WeightKey::ResourceStockNeeded => [0.000000, 0.000000, 0.000000],
        WeightKey::ScienceNeeded => [0.000000, 0.000000, 0.000000],
        WeightKey::FreeWorkersNeeded => [0.000000, 0.000000, 0.000000],
        WeightKey::WorkersLate => [1.592105, 1.494118, 1.050279],
        WeightKey::StrengthRelEarly => [2.157895, 2.523529, 2.636872],
        WeightKey::StrengthRelLate => [4.769737, 4.000000, 4.055866],
        WeightKey::TechLevelsLate => [2.980263, 1.129412, 1.430168],
        WeightKey::HandValueLate => [6.217105, 5.723529, 7.016760],
        WeightKey::FoodGap => [4.000000, 4.000000, 4.000000],
        WeightKey::FoodSurplus => [4.000000, 4.000000, 5.000000],
        WeightKey::ResourceGap => [5.000000, 3.000000, 4.000000],
        WeightKey::ResourceSurplus => [6.000000, 5.000000, 5.000000],
        WeightKey::ScienceGap => [6.000000, 6.000000, 11.000000],
        WeightKey::ScienceSurplus => [10.000000, 4.000000, 10.000000],
        WeightKey::CultureGap => [7.000000, 8.000000, 7.000000],
        WeightKey::CultureSurplus => [9.000000, 9.000000, 8.000000],
        WeightKey::HappySurplus => [2.000000, 2.000000, 2.000000],
        WeightKey::CivilActionGap => [4.000000, 4.000000, 4.000000],
        WeightKey::CivilActionSurplus => [4.000000, 4.000000, 5.000000],
        WeightKey::TakeCostShare => [1.000000, 0.800000, 0.750000],
        WeightKey::MilitaryActionGap => [3.000000, 3.000000, 3.000000],
        WeightKey::MilitaryActionSurplus => [3.000000, 3.000000, 2.000000],
        WeightKey::WorkerGap => [2.000000, 2.000000, 2.000000],
        WeightKey::WorkerSurplus => [2.000000, 2.000000, 2.000000],
        WeightKey::TechRedundancyDiscount => [0.000000, 0.000000, 0.000000],
        WeightKey::LeaderReplacement => [1.000000, 1.000000, 1.000000],
        WeightKey::WonderPoolRivalClaimed => [1.000000, 2.000000, 1.000000],
        // Brand new this batch, never yet run through `featspread` -- `0.0`
        // is not a guessed number (this doc comment's own warning against
        // hand-transcribing a plausible-looking value applies here), it is
        // the documented "unmeasured" state this function already defines:
        // `clamp_bound` falls back to `CLAMP_BLIND` whenever spread `<= 0.0`,
        // exactly the fallback `RateHorizon`/`TechRedundancyDiscount` above
        // The six gap-conditional keys below were seeded all-zero when they
        // landed and are now MEASURED, from `featspread 40 0 ../experiments
        // emit` run against the champion the climb was carrying on 2026-08-26.
        // Every one of them fires, so none of them falls back to CLAMP_BLIND
        // any more.
        WeightKey::HandOverCapacity => [1.000000, 1.000000, 1.000000],
        WeightKey::HappyMarginAfterNextPop => [2.000000, 2.000000, 2.000000],
        WeightKey::ResourceCommitmentTurns => [16.000000, 16.000000, 16.000000],
        WeightKey::WonderOneStageShort => [1.000000, 1.000000, 1.000000],
        WeightKey::ScienceNeedRow => [5.000000, 4.000000, 4.000000],
        WeightKey::RowPlayableCount => [12.000000, 12.000000, 13.000000],
    }
    }
}

/// How many whole typical decisions a single coordinate is allowed to
/// command on its own -- the one free parameter in
/// [`WeightKey::clamp_bound`], and dimensionless by construction because
/// both sides of its ratio are score ranges over the same candidate sets.
///
/// `1.0` is the pick. Measured against the live champions, `2.0` forces no
/// weight anywhere to move and so constrains nothing, while `0.5` would
/// pull fifteen live coordinates in at once. `1.0` binds on the handful
/// that had run to the old flat rail and leaves everything else alone,
/// which is what a safety rail is for: it is not a fitted parameter and
/// must never become one.
pub const CLAMP_T: f64 = 1.0;

/// The flat magnitude rail the league used for every weight before
/// [`WeightKey::clamp_bound`] existed, kept as the CEILING on every derived
/// bound and as the fallback for coordinates the spread instrument cannot
/// see. Because it is a ceiling and not a floor, the per-key bound can only
/// tighten a coordinate relative to the old behaviour.
pub const CLAMP_BLIND: f64 = 60.0;

/// The p95, over decisions, of the range the FULL evaluation score takes
/// across a decision's candidate set, at 2/3/4 players -- "what one typical
/// decision is worth" and the numerator of [`WeightKey::clamp_bound`].
///
/// MEASURED by `bin/featspread`, which emits this line and the whole body of
/// [`WeightKey::p95_candidate_spread`] together; regenerate them together or
/// the ratio compares two different samples.
pub const P95_TOTAL_SPREAD: [f64; 3] = [823.689416, 483.787396, 545.274556];

/// Where a player count sits in [`P95_TOTAL_SPREAD`] and in
/// [`WeightKey::p95_candidate_spread`]'s triples.
fn player_index(players: u8) -> usize {
    match players {
        2 => 0,
        3 => 1,
        4 => 2,
        other => unreachable!("player counts are validated to 2..=4 before any weight is priced, got {other}"),
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

/// The base keys that carry a need hinge -- the honest source of "which keys
/// get a `_needed` partner", exactly as [`STANDING_KEYS`] is for `_trailing`.
///
/// Membership is a RULES question, not a tuning one: a key belongs here iff
/// some rule converts a stock of it into a cost, so that "how far short of
/// that cost am I" is a real quantity. Food buys population increase, resources
/// buy a build, science buys a develop, and a build consumes a free worker.
/// Culture is deliberately absent -- no rule ever spends culture, so it has no
/// threshold, and its pressure is competitive and already carried by
/// [`STANDING_KEYS`]. Every threshold here is computed once, in
/// `features::marginal_needs`, and shared with the `*Gap`/`*Surplus`
/// coordinates so the pricer and the evaluator cannot disagree about it.
pub const NEED_KEYS: &[WeightKey] = &[
    WeightKey::FoodStock,
    WeightKey::ResourceStock,
    WeightKey::Science,
    WeightKey::FreeWorkers,
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
///    every retired key here (once) is what keeps that check from being
///    unable to tell the two apart.
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
    // `CardBoardLeader` is NOT retired alongside these: Leader has no
    // dedicated top-level branch in `card_potential_core` at all, so its
    // per-type offset is the ONLY board-aware pricing channel it has.
    // `CardBoardBonus` was in the same boat at the time of THIS retirement
    // (2026-08-13) -- Bonus had no dedicated top-level branch either -- but
    // it is retired separately below (2026-08-24), for a different reason:
    // not a dedicated function shadowing it, but its two possible consumers
    // being structurally unreachable/zero for a Bonus card regardless.
    "card_board_government",
    "card_board_action",
    "card_board_wonder",
    // Retired 2026-08-24: `Special::FreeCivilAction` is priced ONLY by
    // `cards::action_value`'s special-list branch, through
    // `free_action_credit`. Nothing anywhere reads `w[free_civil_action]` --
    // `card_yields`' own flat-lookup branch for it was excised when it
    // measurably disagreed with the board-aware reroute, and the typed
    // `CardEffects.free_civil_action` field it was named for is 0 on all 236
    // base-game cards, so there was never a second reader to fall back to.
    // Same shape as the three keys above, and the same reason for deleting
    // rather than pinning: an indexable coordinate no reader consults is a
    // free random walk. The climb had duly walked it to 8.61 on the 4p
    // champion and 0.53 on the 2p one, numbers that changed no decision and
    // spent mutation budget every generation to reach.
    "free_civil_action",
    // Retired 2026-08-24, alongside `free_civil_action` above, same shape:
    // `restricted_resources` was never read as a NUMBER anywhere, only used
    // as a DISPATCH TAG by `cards::restricted_to_feature` to decide whether
    // to reroute through `resource_stock` * `restricted_resource_credit`
    // instead. `cards::Priced::RestrictedResources` is the tag that
    // survives it -- see that enum's own doc comment -- with no `WeightKey`
    // behind it, so the coordinate a hill climb used to be free to
    // random-walk (it had no gradient: nothing compared one setting of it
    // to another, since the number itself was never multiplied by
    // anything) no longer exists to walk.
    "restricted_resources",
    // Retired 2026-08-24 (analysis/multiplier_decisiveness_all_counts_
    // 2026-08-24.txt). The proof is STRUCTURAL -- it holds for every weight
    // vector, not merely "no trained champion happens to reach it", which is
    // the bar a retirement has to clear (contrast `GovActionCost`, whose
    // reachability turns on another key's value). It only ever entered
    // `cards::
    // card_potential_core`'s `credit_board` sum for a `CardType::Bonus`
    // card, and `credit_board` there has exactly two consumers --
    // `board_yields::board_yields`'s swap diff, which requires
    // `is_swap_type` (Leader/Government/Wonder, never Bonus), and
    // `board_yields::board_extra`, which only ever emits a triple for a card
    // carrying `Special::CulturePerCivilizationWithMoreCulture` or
    // `Special::ResourcesForMilitaryUnitsPerStrongerCivilization` -- the
    // three cards with either special (Endowment for the Arts, Wave of
    // Nationalism, Military Build-Up) are all `CardType::Action`, never
    // Bonus. So both consumers are unreachable/zero for every real Bonus
    // card for EVERY weight vector, not conditioned on any other key's
    // value the way `gov_action_cost`'s reachability turned out to be --
    // `cards::board_credit_key` answers `None` for `Bonus` now, joining
    // Government/Action/Wonder in that bucket, and `eval::
    // card_board_credit_keys()` (derived from it, not hand-copied) shrinks
    // accordingly with no edit needed there.
    "card_board_bonus",
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
            // `card_board_bonus` was this row's pin until it was retired
            // 2026-08-24 (`RETIRED_KEYS`); `card_board_leader` is its
            // still-live `WeightGroup::Board` sibling.
            ("card_board_leader", "board"),                // GROUPS["board"]
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

    /// `Special::FreeCivilAction` is priced through ONE coordinate now, and
    /// that one must stay gated non-negative: it scales Rich Land's and
    /// Engineering Genius's whole headline grant, and left free to go
    /// negative it prices that grant as a penalty -- which is what every 2p
    /// champion on disk had learned (-0.24 on the deployed one).
    ///
    /// The gate used to be asserted on a PAIR, because `free_civil_action`
    /// existed alongside it. That key is retired (see [`RETIRED_KEYS`]): no
    /// reader ever consulted it, so its gate was protecting nothing while its
    /// slot took mutations every generation.
    #[test]
    fn the_only_key_pricing_a_free_civil_action_is_gated_non_negative() {
        assert!(matches!(WeightKey::FreeActionCredit.sign_intent(), SignIntent::NonNegative(_)));
        assert!(WeightKey::by_name("free_civil_action").is_none());
        assert!(RETIRED_KEYS.contains(&"free_civil_action"));
    }

    /// The four `*BoardCredit`-family keys the follow-up audit could prove
    /// non-negative: each scales a magnitude already floored at zero, so the
    /// only thing a negative value can buy is an inverted gain.
    /// `TacticShortfallCost` is the odd one by name -- it is `NonNegative`
    /// precisely BECAUSE it is subtracted, so a bigger shortfall must cost
    /// more, not less.
    ///
    /// The "thirteen bucket-mates stay `Free`" claim this test originally
    /// pinned is gone: a LATER audit (2026-08-24,
    /// analysis/multiplier_decisiveness_all_counts_2026-08-24.txt) found
    /// eleven of those thirteen are provable `w.get(key) * <dedicated
    /// function's signed output>` scales -- see `sign_intent`'s own doc
    /// comment on that match arm for the full per-key proof -- and gated
    /// them `NonNegative` too, including `TechBoardCredit`/`GovBoardCredit`/
    /// `TerritoryCredit` this test used to assert stayed `Free`. Only
    /// `CardRateCredit`/`BonusCardCredit` remain genuinely free: both scale
    /// a printed `CardEffects` magnitude confirmed non-negative on every
    /// base-game card, so a negative scale would not invert anything (there
    /// is nothing signed to invert) -- gating them would be an unsupported
    /// guess, not a repair.
    #[test]
    fn the_four_board_credits_with_a_zero_floored_magnitude_are_gated_non_negative() {
        for key in [
            WeightKey::PactBoardCredit,
            WeightKey::TacticBoardCredit,
            WeightKey::TacticShortfallCost,
            WeightKey::RestrictedResourceCredit,
        ] {
            assert!(
                matches!(key.sign_intent(), SignIntent::NonNegative(_)),
                "{key:?} must be gated non-negative"
            );
        }
        for key in [WeightKey::CardRateCredit, WeightKey::BonusCardCredit] {
            assert!(
                matches!(key.sign_intent(), SignIntent::Free),
                "{key:?} scales a printed magnitude confirmed non-negative on every base-game card and must stay free"
            );
        }
    }

    /// The 2026-08-24 trust-multiplier audit's own positive claim: all
    /// eleven keys it reclassified from `Free` to `NonNegative` (see
    /// `sign_intent`'s doc comment on that match arm) actually landed there,
    /// not merely that the two survivors stayed `Free` (checked above).
    #[test]
    fn the_eleven_trust_multiplier_keys_the_2026_08_24_audit_proved_signed_are_gated_non_negative() {
        for key in [
            WeightKey::UnitTechCredit,
            WeightKey::TechBoardCredit,
            WeightKey::ActionBoardCredit,
            WeightKey::GovBoardCredit,
            WeightKey::WonderBoardCredit,
            WeightKey::BuildFreshCredit,
            WeightKey::TerritoryCredit,
            WeightKey::AggressionBoardCredit,
            WeightKey::WarBoardCredit,
            WeightKey::EventBoardCredit,
            WeightKey::TacticReachCredit,
        ] {
            assert!(
                matches!(key.sign_intent(), SignIntent::NonNegative(_)),
                "{key:?} must be gated non-negative"
            );
        }
    }

    /// The third sign-audit pass, which swept every remaining key rather than
    /// a bucket. Each gate here is either a twin of one already landed
    /// (`HandMilValue` of `HandValue`, `MilitaryActions` of `CivilActions`) or
    /// a flag whose rules effect is one-directional on its holder. The `Free`
    /// half is the point of the test: these are the keys the same pass looked
    /// at and REFUSED, each for a cited reason in `sign_intent`'s own comments,
    /// so re-proposing one needs to answer the counterexample first.
    #[test]
    fn the_third_audit_pass_gates_six_more_keys_and_deliberately_refuses_the_rest() {
        for key in [
            WeightKey::HandMilValue,
            WeightKey::ColonizeBonus,
            WeightKey::MilitaryActions,
            WeightKey::WarImmune,
            WeightKey::AttackCostDoubled,
        ] {
            assert!(
                matches!(key.sign_intent(), SignIntent::NonNegative(_)),
                "{key:?} must be gated non-negative"
            );
        }
        assert!(
            matches!(WeightKey::NoAggression.sign_intent(), SignIntent::NonPositive(_)),
            "NoAggression strips its own holder's aggression and war moves, so it may never reward"
        );
        for key in [
            // Deterministic function of the government `GovLevel` prices.
            WeightKey::UrbanLimit,
            // Residuals of the two gated `*Value` sums.
            WeightKey::HandCivil,
            WeightKey::HandMilitary,
            // Vast Territory prints `blueTokens: -1` inside a hand card's yields.
            WeightKey::HandPotential,
            WeightKey::HandMilPotential,
            // Communism `happy: -1`, Fundamentalism `science: -2`.
            WeightKey::GovLevel,
            WeightKey::TechLevels,
            // Vast Territory again, as a permanent colony effect.
            WeightKey::Colonies,
            WeightKey::HasColony,
            // Sid Meier prints `sciencePerLab: -1`.
            WeightKey::Leader,
            // Symmetric: it costs the holder its own attack option too.
            WeightKey::PactBlocksAttack,
            // RULES_SPEC 10.4 outdated armies.
            WeightKey::TacticLevel,
            // `Special::StrongestPlayer` makes the lead a targetable liability.
            WeightKey::StrengthLead,
        ] {
            assert!(
                matches!(key.sign_intent(), SignIntent::Free),
                "{key:?} has a cited counterexample and must stay free"
            );
        }
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

    /// The per-key bound is a CEILING-limited tightening, never a loosening.
    /// Whatever the measurement says, no coordinate comes out of
    /// `clamp_bound` with more room than the flat rail the league used
    /// before it existed -- so landing this can only ever constrain the
    /// climb, and a stale or badly-regenerated spread table cannot silently
    /// hand a coordinate the run of the evaluation.
    #[test]
    fn no_key_is_ever_bounded_more_loosely_than_the_old_flat_rail() {
        for &k in WeightKey::ALL {
            for players in [2u8, 3, 4] {
                let bound = k.clamp_bound(players);
                assert!(
                    bound <= CLAMP_BLIND,
                    "{} at {}p is bounded at {}, looser than the {} rail",
                    k.name(),
                    players,
                    bound,
                    CLAMP_BLIND
                );
                assert!(bound > 0.0, "{} at {}p is bounded at {}", k.name(), players, bound);
            }
        }
    }

    /// A key measuring zero spread is INVISIBLE to the instrument, not
    /// harmless, and it keeps the flat rail rather than an invented number.
    /// `featspread` cannot see a credit key at all -- `linear_features`
    /// prices those at the caller's frozen vector, so their candidate-set
    /// spread is zero by construction there. `creditspread`'s displacement
    /// probe can, and the credit rows above are its readings; the ones still
    /// at zero are the ones IT could not see either, which is a stronger
    /// statement than "no instrument existed".
    ///
    /// `card_rate_credit` is the clean case: zero firing decisions at every
    /// player count, because the board-aware branch of `cards.rs` returns
    /// before the key is ever read whenever `card_board_credit != 0.0` --
    /// true on all three live champions. `tech_board_credit` at 3p is the
    /// other kind: it fires 11909 times but is GATED, its response sublinear
    /// across the probe, so no slope is defensible and the rail stands.
    #[test]
    fn a_key_the_spread_instrument_cannot_see_keeps_the_flat_rail() {
        assert_eq!(WeightKey::CardRateCredit.p95_candidate_spread(), [0.0, 0.0, 0.0]);
        assert_eq!(WeightKey::CardRateCredit.clamp_bound(3), CLAMP_BLIND);
        assert_eq!(WeightKey::TechBoardCredit.clamp_bound(3), CLAMP_BLIND);
    }

    /// Every key must have a measured triple, and the whole table must have
    /// come from one run: a key silently emitted as zeros because a
    /// regeneration only covered part of the enum would quietly fall back to
    /// the flat rail and look exactly like a legitimately blind coordinate.
    /// Anchor on a handful whose spread is structural (a card row is thirteen
    /// slots; `culture` moves in whole points) so a partial regeneration
    /// cannot pass.
    #[test]
    fn the_measured_spread_table_covers_the_visible_keys() {
        let visible = WeightKey::ALL
            .iter()
            .filter(|k| k.p95_candidate_spread().iter().any(|s| *s > 0.0))
            .count();
        assert!(visible > 100, "only {visible} of {} keys have a measured spread", WeightKey::ALL.len());
        assert!(WeightKey::Culture.p95_candidate_spread()[0] > 1.0);
        assert!(WeightKey::HandPotential.p95_candidate_spread()[1] > 100.0);
    }

    /// Every authored default is legal under the measured bounds. The lone
    /// former exception was `uprising` at 2p, authored at -12 against a bound
    /// of 9.1, and it came inside at 54.9 when the table was remeasured
    /// against the current champions: `P95_TOTAL_SPREAD[2p]` rose 300.4 ->
    /// 823.7 while `uprising`'s own 2p swing fell 33 -> 15. Both ends moved
    /// because `p95_candidate_spread` measures `phi`, and `phi` is a function
    /// of the weight vector it was measured under -- so the whole table ages
    /// out from under a promoted champion and has to be regenerated with it,
    /// not just topped up. If a regenerated table pushes any default back
    /// outside, that is a finding, not a number to update quietly: name the
    /// key here and say why its authored value is worth more than one whole
    /// typical decision.
    #[test]
    fn no_authored_default_starts_outside_its_measured_bound() {
        // Named, with the reason, per this test's own doc comment. Landing
        // `creditspread`'s table on 2026-08-26 put exactly one default back
        // outside: `unit_tech_credit` at 4p, authored 1.0 against a bound of
        // 0.68. Its 4p slope, 796.64, is the largest in the credit class, so
        // the authored 1.0 would command 1.46 whole typical decisions on its
        // own -- above the `CLAMP_T` budget by construction. The 1.0 is the
        // NEUTRAL multiplier (price unit-tech gains at face value), not a
        // fitted value, so it is not evidence about the right magnitude; and
        // the live 4p champion already carries 0.0000 on this key, so the
        // rail trimming the start point to 0.68 takes nothing the 4p climb
        // was using. A SECOND entry appearing here is a fresh finding and
        // must be justified the same way, not appended to.
        const NAMED: [(&str, u8); 1] = [("unit_tech_credit", 4)];
        let w = Weights::defaults();
        let outside: Vec<(&str, u8)> = WeightKey::ALL
            .iter()
            .flat_map(|k| [2u8, 3, 4].map(|p| (k, p)))
            .filter(|(k, p)| w.get(**k).abs() > k.clamp_bound(*p))
            .map(|(k, p)| (k.name(), p))
            .collect();
        assert_eq!(outside, NAMED, "authored defaults outside their bounds");
    }
}
