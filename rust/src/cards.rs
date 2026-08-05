//! Card identity and the static card table.
//!
//! DESIGN.md rule 1: a card is a `CardId`, which is an index. Names exist at
//! the I/O boundary and nowhere else. Every lookup in this module is an array
//! index, which is the difference between this engine and the Python one --
//! `dict.get` is the single largest entry in the Python profile at 9%, and the
//! profile is otherwise flat, so there is no hot loop to fix instead.

use core::fmt;

/// A card, as an index into [`CARDS`].
///
/// `u16` rather than `u8` because there are 236 cards today and the base game
/// is fixed, but the expansion is not being ported and a future `u8` overflow
/// would be a silent wrap. `u16` also keeps `Tableau::ids` aligned sanely.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct CardId(pub u16);

impl CardId {
    /// Sentinel for "no card". `Option<CardId>` is the same size (niche-less
    /// `u16`), so the explicit sentinel keeps the fixed-size arrays in
    /// `state.rs` plain `Copy` data with no `Option` unwrapping in hot loops.
    pub const NONE: CardId = CardId(u16::MAX);

    #[inline]
    pub fn is_none(self) -> bool {
        self == CardId::NONE
    }

    #[inline]
    pub fn get(self) -> &'static Card {
        &CARDS[self.0 as usize]
    }

    #[inline]
    pub fn name(self) -> &'static str {
        self.get().name
    }

    #[inline]
    pub fn kind(self) -> CardType {
        self.get().kind
    }

    /// Age as a number: A=0, I=1, II=2, III=3, IV=4. Mirrors `cards.level`.
    #[inline]
    pub fn level(self) -> u8 {
        self.get().age as u8
    }

    /// Parse a printed name. **I/O only** -- fixture loading, transcripts,
    /// tests. Linear scan is deliberate: making this fast would mean a
    /// `HashMap<String, CardId>`, and a fast path here would invite callers to
    /// use names in the engine, which rule 1 exists to prevent.
    pub fn by_name(name: &str) -> Option<CardId> {
        CARDS
            .iter()
            .position(|c| c.name == name)
            .map(|i| CardId(i as u16))
    }
}

impl fmt::Debug for CardId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_none() {
            f.write_str("CardId::NONE")
        } else {
            write!(f, "{}", self.name())
        }
    }
}

/// Which age a card belongs to. `Age::A` is the starting-tableau age.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
#[repr(u8)]
pub enum Age {
    A = 0,
    I = 1,
    II = 2,
    III = 3,
    IV = 4,
}

/// The 23 card types in the base game, exactly as `data/*.json` spells them.
///
/// Exhaustive by construction: the generator fails if `data/*.json` grows a
/// type not listed here, rather than defaulting it to something plausible.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum CardType {
    // production
    Farm,
    Mine,
    // urban
    Lab,
    Temple,
    Library,
    Arena,
    Theater,
    // military units
    Infantry,
    Cavalry,
    Artillery,
    Air,
    // other civil
    Government,
    SpecialTech,
    Wonder,
    Leader,
    Action,
    // military deck
    Tactic,
    Aggression,
    War,
    Pact,
    Bonus,
    Territory,
    Event,
}

impl CardType {
    #[inline]
    pub fn is_urban(self) -> bool {
        use CardType::*;
        matches!(self, Lab | Temple | Library | Arena | Theater)
    }

    #[inline]
    pub fn is_unit(self) -> bool {
        use CardType::*;
        matches!(self, Infantry | Cavalry | Artillery | Air)
    }

    #[inline]
    pub fn is_production(self) -> bool {
        matches!(self, CardType::Farm | CardType::Mine)
    }

    /// Types that can hold yellow tokens. Mirrors `cards.WORKER_TYPES`.
    #[inline]
    pub fn takes_workers(self) -> bool {
        self.is_urban() || self.is_unit() || self.is_production()
    }

    /// Mirrors `cards.DEVELOPABLE_TYPES`.
    #[inline]
    pub fn is_developable(self) -> bool {
        self.takes_workers() || matches!(self, CardType::SpecialTech | CardType::Government)
    }

    /// Mirrors `cards.CIVIL_ROW_TYPES` -- what appears in the 13-slot row.
    #[inline]
    pub fn is_civil_row(self) -> bool {
        use CardType::*;
        self.takes_workers()
            || matches!(self, Government | SpecialTech | Wonder | Leader | Action)
    }
}

/// The numeric effects that RECUR across cards.
///
/// DESIGN.md: 113 effect keys exist, but 60 of them appear exactly once. The
/// recurring ones live here as fields -- read on every stats recomputation, so
/// they must be a field load. The one-offs are [`Special`], which is code.
///
/// Signed because several cards subtract (Despotism's civil actions, the
/// obsolete-tactic penalty). Sixteen bits throughout: nothing in the base game
/// approaches 32k, and keeping the struct small keeps `CARDS` in cache.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CardEffects {
    /// Per-turn yield while this card is in play.
    pub culture: i16,
    pub science: i16,
    pub strength: i16,
    pub happy: i16,
    pub civil_actions: i16,
    pub military_actions: i16,

    /// One-shot gains when the card resolves (action cards, events).
    pub gain_culture: i16,
    pub gain_science: i16,
    pub gain_food: i16,
    pub gain_resources: i16,

    /// Discounts, in the units of the thing discounted. The per-age
    /// construction discount is NOT here: every card that prints
    /// `buildDiscount` prints a per-AGE dict, so it lives on
    /// [`Special::BuildDiscount`] as an `[i16; 5]` indexed by [`Age`]. A
    /// `build_discount: i16` field sat here until 2026-08-05 and was zero on
    /// all 236 cards -- the generator could only ever have written it from a
    /// scalar, and no such card exists.
    pub resource_discount: i16,
    pub resources_for_military_units: i16,

    // A tactic's army strength is NOT here. The data prints it twice -- once
    // as top-level `strength`/`obsoleteStrength` and again as
    // `effects.tacticBonus`/`tacticBonusObsolete` -- and the Python engine
    // reads only the first pair (`effects.py:991`); `bots/weighted.py:2029`
    // documents the second pair as "a duplicate spelling" and excludes it.
    // Carrying both would put the same fact in two registries with nothing
    // failing when they disagree, which is the bug class this rewrite exists
    // to close. `Card.effects.strength` and `Card.obsolete_strength` are the
    // single source, and `gen_cards.py` asserts the duplicate agrees with them
    // rather than storing it.
    pub defense_bonus: i16,
    pub colonization_bonus: i16,
    pub colonize_bonus: i16,
    pub blue_tokens: i16,
    pub on_build_culture: i16,
    pub wonder_stages_per_action: i16,
    pub civil_hand_limit: i16,
    pub military_hand_limit: i16,
    pub free_civil_action: i16,

    /// Printed on governments only (§1.4): distinct urban buildings
    /// (lab/temple/library/arena/theater) a player may have in play at once.
    /// Zero for every non-government card. Added post-generation-audit: the
    /// original generator only read a card's `effects` dict plus the printed
    /// `strength`/`civilActions`/`militaryActions`, and silently never read
    /// this key at all, so every government reported urban_limit as
    /// "missing" rather than its printed 2/3/3/3/3/4/4/4 -- see gen_cards.py.
    pub urban_building_limit: i16,

    /// Per-turn food/resources. Absent from every ordinary card's `effects`
    /// dict (farms/mines print through [`Production`] instead, and one-shot
    /// gains are `gain_food`/`gain_resources` above) -- these two exist only
    /// because a territory's `permanentEffects` and a pact's `A`/`B`/
    /// `bothPlayers` block both print bare `food`/`resources` keys
    /// (`engine/effects.py` `COLONY_PERMANENT_KEYS` / `FLAT_KEYS`'s
    /// "aliases used by colony permanents and pact effects"). Nonzero only
    /// on territory cards today; `PactBlock` carries its own copies for pact
    /// blocks rather than reusing these (see `PactBlock`'s doc comment).
    pub food: i16,
    pub resources: i16,

    /// Yellow tokens granted while entering play. Mirrors `blue_tokens`
    /// above but for the other pool -- added for territory cards'
    /// `permanentEffects.yellowTokens` (§11.5), which `compute()` never
    /// reads (only `gain_colony`/`lose_colony`, combat.rs, do) but which
    /// must not be silently dropped just because combat.rs isn't written
    /// yet. Zero on every non-territory card in the base game today.
    pub yellow_tokens: i16,
}

/// One-time gains a colonization territory card pays out the moment it is
/// claimed (§11.5, `data/*.json` `immediateEffects`, `engine/interact.py`
/// `gain_colony` -> `events.apply_gains`). Distinct from [`CardEffects`]
/// deliberately: `CardEffects.food`/`resources` are ONGOING per-turn deltas
/// (a colony's `permanentEffects`), where these are a ONE-SHOT amount paid
/// once -- the two must never be added into the same field or a colony's
/// permanent bonus and its welcome gift become the same number by accident.
/// Not consumed anywhere in the port yet (combat.rs / colonization is not
/// written), captured so `gen_cards.py` does not silently drop it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ImmediateEffects {
    pub food: i16,
    pub resources: i16,
    pub culture: i16,
    pub science: i16,
    pub population: i16,
    pub draw_military_cards: i16,
}

/// The numeric grants inside ONE side of a pact card (§5.9): the `A`, `B`,
/// `bothPlayers` or `onAttackBetweenParties` block in the printed data. A
/// pact block prints a small, closed vocabulary that overlaps
/// [`CardEffects`]'s (culture/food/resources/strength/militaryActions) plus
/// five keys that exist ONLY for pacts (`engine/effects.py::Stats`'
/// `tech_discount`/`war_immune`/`food_as_resource`/`resource_as_food`/
/// `science_partners`, plus `attackerStrength` and
/// `cultureProductionPerCompletedWonderOfTheOtherParty`, both read directly
/// by bespoke pact functions rather than through the generic flat-field
/// dispatch). A dedicated struct rather than reusing `CardEffects`: mixing
/// the two would let a stray `CardEffects` field silently mean "add this to
/// every stats recomputation" on a struct that is actually gated behind
/// pact partnership, which is exactly the kind of same-fact-two-registries
/// bug this project keeps a list of.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PactBlock {
    pub culture: i16,
    pub food: i16,
    pub resources: i16,
    pub strength: i16,
    pub military_actions: i16,
    /// `technologyScienceDiscount`: science off every technology `p` develops.
    pub tech_discount: i16,
    /// `cannotBeDeclaredWarOnByAnyone`: magnitude ignored by Python too --
    /// presence is the whole rule (`Stats.war_immune`).
    pub war_immune: bool,
    /// `mayUseFoodAsResource` / `mayUseResourceAsFood`: food/resources
    /// spendable as the other pool, up to this many per turn.
    pub food_as_resource: i16,
    pub resource_as_food: i16,
    /// `otherPartyPaysScience`: the OTHER party must pay 1 science for `p`'s
    /// technology to be developed at all. Magnitude ignored by Python
    /// (`Stats.science_partners` is a presence list, not an amount) --
    /// `bool`, matching `war_immune` above.
    pub other_party_pays_science: bool,
    pub culture_per_wonder_of_other_party: i16,
    /// `onAttackBetweenParties.attackerStrength`: bonus to the attacker's
    /// strength ONLY when the two pact parties fight each other (§5.4.2).
    pub attacker_strength: i16,
}

/// The three keys `effects.takeFromOpponent` prints in the base game
/// (§5.4.6, `engine/events.py::finish_aggression` lines 651-667): what the
/// attacker steals from the defender when this aggression succeeds. A
/// dedicated struct rather than folding these into [`CardEffects`] -- same
/// reasoning as [`PactBlock`]'s own doc comment: these three numbers are
/// read by ONE bespoke function (`combat::finish_aggression`'s theft loop),
/// not by `effects::compute`, so mixing them into the struct every card's
/// stats recomputation walks would invite exactly the kind of stray field
/// this project keeps a list of.
///
/// Exhaustive against the live data (2026-08-05: three base-game aggression
/// cards use this key, one field each -- Plunder prints
/// `foodAndOrResources`, Spy prints `science`, Armed Intervention prints
/// `culture` -- but nothing stops a card from printing more than one, so all
/// three fields are always present, zero when unprinted).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TakeFromOpponentBlock {
    pub food_and_or_resources: i16,
    pub science: i16,
    pub culture: i16,
}

/// Which statistic `FinalScoringBlock.ranking_stat` ranks players by
/// (`effects.allPlayers.statistic`, §12.5.2 "Impact of Strength"/"Impact of
/// Science"). Mirrors `engine/events.py::_STAT_ALIASES`'s five target names
/// (`strength`/`science`/`culture_rate`/`food`/`resources`) exactly -- a
/// closed, hand-named enum for the same reason `FreeCivilActionValue` et al
/// are (gen_cards.py's `STRING_EFFECT_VALUES`), even though only the first
/// two are ever printed in the base game today.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum FinalScoringStat {
    #[default]
    Strength,
    Science,
    CultureRate,
    Food,
    Resources,
}

/// One Age III "Impact of ..." event's `effects.allPlayers` block (§12.5.2)
/// -- the fifteen final-scoring formulas `engine/events.py::scoring_culture`
/// dispatches over. A dedicated struct rather than folding into
/// [`CardEffects`]: same reasoning as [`PactBlock`]/[`TakeFromOpponentBlock`]
/// -- these fields are read exactly once per game, by ONE bespoke function
/// (`events::scoring_culture`), never by `effects::compute`'s per-turn scan.
///
/// Every numeric field follows the same "0 = key not printed" convention
/// [`TakeFromOpponentBlock`] already uses, EXCEPT `max_culture_from_happy_faces`:
/// Python distinguishes "no cap" (`None`) from a hypothetical printed cap of
/// `0` (`if cap is not None: gained = min(gained, cap)`), and those two would
/// clamp very differently. `0` stands in for "no cap" here anyway -- no
/// base-game card ever prints an actual cap of 0 (the one card that prints
/// this key at all prints 16) -- but a future data revision that ever did
/// would need a real presence flag, not a bigger sentinel.
///
/// `bonus_if_production_exceeds_consumption` and `max_culture_from_happy_faces`
/// are each only ever printed alongside `culture_per_food_produced_by_farms`/
/// `culture_per_happy_face` respectively in the base data (one card each:
/// "Impact of Agriculture", "Impact of Happiness") -- Python's own dispatch
/// only reads them while handling that sibling key's `elif` branch, which
/// this port's unconditional zero-safe arithmetic reproduces exactly for
/// every card that exists today without needing to encode that pairing as a
/// rule.
///
/// `ranking_2p`/`ranking_3p`/`ranking_4p` hold the `rankingCulture` table
/// (`has_ranking` false and all-zero on the thirteen events that do not
/// print one), one fixed-width array per player count rather than a map,
/// per DESIGN.md rule 3 -- the consumer indexes by `game::live_count`
/// directly instead of looking a key up.
///
/// Exhaustive against the live data (2026-08-05: 15 base-game cards print
/// `scoringEvent: true`, one `effects.allPlayers` block each -- gen_cards.py
/// asserts every key inside every one of those 15 blocks is classified here,
/// in `SCORING_BLOCK_IGNORED_KEYS`, or in `IGNORED_KEYS`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FinalScoringBlock {
    /// "Impact of Industry".
    pub culture_per_resource_produced_by_mines: i16,
    /// "Impact of Agriculture", paired with `bonus_if_production_exceeds_consumption`.
    pub culture_per_food_produced_by_farms: i16,
    pub bonus_if_production_exceeds_consumption: i16,
    /// "Impact of Competition".
    pub culture_per_level_of_military_units_and_arenas: i16,
    /// "Impact of Progress".
    pub culture_per_level_of_special_techs_and_government: i16,
    /// "Impact of Wonders" -- per age, indexed by `Age as u8` exactly like
    /// [`Special::BuildDiscount`].
    pub culture_per_completed_wonder_by_age: [i16; 5],
    /// "Impact of Population".
    pub culture_per_content_worker_above_10: i16,
    /// "Impact of Colonies".
    pub culture_per_colony: i16,
    /// "Impact of Government", alongside `culture_per_military_action`.
    pub culture_per_civil_action: i16,
    pub culture_per_military_action: i16,
    /// "Impact of Architecture".
    pub culture_per_level_of_urban_buildings: i16,
    /// "Impact of Happiness", paired with `max_culture_from_happy_faces`.
    pub culture_per_happy_face: i16,
    pub max_culture_from_happy_faces: i16,
    /// Also "Impact of Happiness" (negative: -2 per discontent worker).
    pub culture_per_discontent_worker: i16,
    /// "Impact of Technology".
    pub culture_per_age_iii_technology: i16,
    /// "Impact of Balance".
    pub culture_times_lowest_production: i16,
    /// "Impact of Variety".
    pub culture_per_distinct_type_of_unit_urban_building_and_special_tech: i16,
    /// "Impact of Strength"/"Impact of Science" -- the other fifteen events
    /// leave this `false` and every `ranking_*` array zeroed.
    pub has_ranking: bool,
    pub ranking_stat: FinalScoringStat,
    pub ranking_2p: [i16; 2],
    pub ranking_3p: [i16; 3],
    pub ranking_4p: [i16; 4],
}

/// One event's targeting-key block (§5.3): the nested dict `resolve_event`
/// hands to `_apply_player_block` for `allPlayers`/`weakestPlayer`/
/// `strongestPlayer`/`playerWithMostCulture`/`playerWithLeastCulture`/
/// `playersWithMostHappyFaces`/`playersWithMostDiscontentWorkers`, and ALSO
/// the narrower block `resolve_event` hands straight to `apply_gains` for
/// the `gain`/`lose` keys beside `strongestPlayers`/`weakestPlayers` -- one
/// struct for all nine call sites (`events.rs`'s `apply_player_block`/
/// `apply_gains_block`), same reasoning `PactBlock`'s own doc comment gives:
/// a dedicated struct rather than folding into `CardEffects`, because these
/// fields are read by bespoke event-resolution functions only, never by
/// `effects::compute`'s per-turn scan, and an event card is never in a
/// player's tableau slots either. Not reused for the *other* seven
/// `DEFERRED_DICT_EFFECT_KEYS` this pass leaves alone (`victorTakes*`,
/// `resourcesForMilitaryUnitsPerStrongerCivilization`,
/// `culturePerCivilizationWithMoreCulture`, `perTurnChoice`) -- those belong
/// to `combat.rs`/`actions.rs`, out of this pass's scope.
///
/// Zero-default convention throughout, exactly like `FinalScoringBlock`/
/// `TakeFromOpponentBlock`: `0`/`false`/`CardId::NONE`/`None`/an empty slice
/// means the source card did not print that key. [`EMPTY`](Self::EMPTY) is
/// the all-absent value (not `#[derive(Default)]`: `CardId`'s own `Default`
/// is `CardId(0)`, a REAL card, which would make an unprinted `freeBuild`
/// silently name the first card in the table -- see `CardId::NONE`'s own
/// doc comment).
///
/// A handful of fields hold a primitive mirror of a `state.rs` queue-item
/// payload (`choose_food`/`choose_resources` for `state::GainOption`,
/// `free_build_*` for `state::FreeBuildSpec`, `one_time_discount_*` for
/// `state::OneTimeDiscount`) rather than that payload type directly:
/// `cards.rs` is the base data layer (this module's own top doc comment:
/// "the Rust engine therefore has no dependencies") and does not import
/// `state.rs`, so `events::apply_player_block` builds the real queue-item
/// value from these primitives at the point of use instead.
///
/// Exhaustive against the live data (2026-08-05 census of every
/// non-`scoringEvent` event card's `allPlayers`/`weakestPlayer`/.../`gain`/
/// `lose` sub-dict, 40 cards): a sub-key outside this vocabulary stops the
/// build (`gen_cards.py::build_event_block`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EventBlock {
    // ---- apply_gains-shape (events.py::apply_gains, one-shot, `sign`-scaled)
    pub science: i16,
    pub culture: i16,
    pub food: i16,
    pub resources: i16,
    pub food_and_or_resources: i16,
    pub blue_tokens: i16,
    pub lose_all_stored_food: bool,
    pub draw_military_cards: i16,
    /// `decreasePopulation` NESTED inside a targeting dict (Pestilence/Reign
    /// of Terror/Refugees) -- a DIFFERENT printing from the top-level
    /// `decreasePopulation` Barbarians alone has, which stays
    /// `Special::DecreasePopulation` (see that variant's own doc comment in
    /// `card_table.rs`).
    pub decrease_population: i16,
    /// `increasePopulation` -- the PAID population increase (§6.1's normal
    /// cost, `economy::increase_population`/`economy::pop_cost`), never the
    /// free grant the unprinted bare `population`/`gainPopulation` keys
    /// would be (`events.py:74-78`, still unreachable -- see `events.rs`'s
    /// top doc comment).
    pub increase_population: i16,

    // ---- events.py::_apply_extras (312-375)
    pub produce_food: bool,
    pub produce_resources: bool,
    pub extra_production: bool,
    pub science_equal_to_science_production: bool,
    pub culture_equal_to_culture_production: bool,
    pub culture_equal_to_science_production: bool,
    pub food_equal_to_happy_faces: bool,
    pub food_equal_to_happy_faces_max: i16,
    pub discard_leader_unless_current_age: bool,
    pub take_yellow_tokens_from_weakest: i16,
    pub decrease_population_by_half_discontent_workers_rounded_up: bool,
    pub civil_actions_per_discontent_worker: i16,
    /// `oneTimeDiscount` -- see `state::OneTimeDiscount`'s own doc comment
    /// for the "never cleared" defect this mirrors on purpose.
    pub one_time_discount_build_resources: i16,
    pub one_time_discount_develop_science: i16,
    pub one_time_discount_pop_food: i16,
    pub destroy_one_urban_building_of_each_opponent: bool,
    pub optional_take_cards_with_civil_actions: i16,
    /// `culturePerDiscontentWorker` -- the one `scoring_culture` key an
    /// ORDINARY (non-`scoringEvent`) card prints, Civil Unrest's own
    /// `allPlayers` block (`events.py::_apply_player_block` calls
    /// `scoring_culture` unconditionally on every block, not only on the 15
    /// `scoringEvent` cards' -- see `events.rs`'s top doc comment). Not the
    /// full 15-key `scoring_culture` vocabulary: `FinalScoringBlock` already
    /// carries that, for the cards that print `scoringEvent: true`.
    pub culture_per_discontent_worker: i16,

    // ---- events.py::_queue_decisions (393-414)
    /// `choose` -- the base game's one `choose` block is always exactly
    /// `[{food: N}, {resources: M}]` (`state::GainOption`'s own doc comment
    /// already narrows to this shape); `0, 0` means not printed.
    pub choose_food: i16,
    pub choose_resources: i16,
    /// `freeBuild.card` -- a real card name (`CardId::by_name`) when the
    /// spec names one exactly (`Development of Warfare`: "Warriors"),
    /// `CardId::NONE` when it instead names a card TYPE (`Development of
    /// Religion`: "Temple", captured in `free_build_kind` below).
    pub free_build_card: CardId,
    pub free_build_age: Option<Age>,
    pub free_build_kind: Option<CardType>,
    pub free_build_cost: i16,
    pub destroy_own_building: i16,
    pub lose_colony: i16,
    /// `flipCompletedWonder.ages` -- same `&'static [Age]` shape as
    /// `Special::DestroyUrbanBuildings`'s per-raid ages, for the same reason
    /// (DESIGN.md rule 3: a fixed-size array/slice, not a map). Empty means
    /// not printed.
    pub flip_completed_wonder_ages: &'static [Age],
    pub discard_military_cards: i16,
}

impl EventBlock {
    /// The all-absent value: every targeting key `resolve_event` checks and
    /// finds missing on a given card is this. See this struct's own doc
    /// comment for why it is a hand-written const rather than
    /// `#[derive(Default)]`.
    pub const EMPTY: EventBlock = EventBlock {
        science: 0,
        culture: 0,
        food: 0,
        resources: 0,
        food_and_or_resources: 0,
        blue_tokens: 0,
        lose_all_stored_food: false,
        draw_military_cards: 0,
        decrease_population: 0,
        increase_population: 0,
        produce_food: false,
        produce_resources: false,
        extra_production: false,
        science_equal_to_science_production: false,
        culture_equal_to_culture_production: false,
        culture_equal_to_science_production: false,
        food_equal_to_happy_faces: false,
        food_equal_to_happy_faces_max: 0,
        discard_leader_unless_current_age: false,
        take_yellow_tokens_from_weakest: 0,
        decrease_population_by_half_discontent_workers_rounded_up: false,
        civil_actions_per_discontent_worker: 0,
        one_time_discount_build_resources: 0,
        one_time_discount_develop_science: 0,
        one_time_discount_pop_food: 0,
        destroy_one_urban_building_of_each_opponent: false,
        optional_take_cards_with_civil_actions: 0,
        culture_per_discontent_worker: 0,
        choose_food: 0,
        choose_resources: 0,
        free_build_card: CardId::NONE,
        free_build_age: None,
        free_build_kind: None,
        free_build_cost: 0,
        destroy_own_building: 0,
        lose_colony: 0,
        flip_completed_wonder_ages: &[],
        discard_military_cards: 0,
    };
}

/// `lastRoundSubstitute` (Politics of Strength, the only base-game card that
/// prints it): when `state.last_round`, `resolve_event` swaps its ENTIRE
/// targeting set for this one instead (`events.py:228-229`), so this holds
/// exactly the two targeting keys the one card that exists prints inside
/// its substitute dict -- `strongestPlayer`/`weakestPlayer`, both plain
/// culture blocks. Exhaustive against the live data (2026-08-05: one card):
/// a substitute dict printing any OTHER targeting key stops the build
/// (`gen_cards.py::build_last_round_substitute`) rather than silently
/// dropping it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LastRoundSubstituteBlock {
    pub strongest_player: EventBlock,
    pub weakest_player: EventBlock,
}

/// Printed per-worker production. A struct, not a single scalar, because one
/// card routinely prints more than one field at once -- Religion is
/// `{culture: 1, happy: 1}`, Printing Press is `{science: 1, culture: 1}` --
/// and a scalar can only ever hold the last one written.
///
/// Distinct from [`CardEffects`]: this is generally scaled by the card's
/// worker count (`effects.py` `_add_production` / `_tech_prog`), where
/// `CardEffects` fields are flat, unscaled numbers. The same shape is reused
/// for a government's production (`effects.py:423`) and, once colonies are
/// ported, a colony's `permanent` block (`effects.py:457`) -- one struct
/// wherever the source data prints a `production`-shaped dict, per
/// `PRODUCTION_EFFECT_FIELDS` in `gen_cards.py`.
///
/// `food`/`resources` double as the blue-token denomination for §6.4 (farms
/// store food, mines store resources); `science`/`culture`/`happy`/`strength`
/// are urban buildings' printed output. A card prints from at most one of
/// {food, resources} and at most two of {science, culture, happy, strength}
/// in the base game today, but nothing here assumes that stays true.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Production {
    pub food: i16,
    pub resources: i16,
    pub science: i16,
    pub culture: i16,
    pub happy: i16,
    pub strength: i16,
}

/// One card's static data. Immutable for the process lifetime.
#[derive(Clone, Copy, Debug)]
pub struct Card {
    pub name: &'static str,
    /// Printed name, before `_disambiguate` appended an age suffix to the
    /// handful of military cards that share a name across ages. Rules that key
    /// on the printed name (war spoils, one-per-name) read THIS, not `name`.
    pub base_name: &'static str,
    pub kind: CardType,
    pub age: Age,
    /// Science to develop / resources to build, from the printed card.
    pub science_cost: u8,
    pub resource_cost: u8,
    /// Copies in the deck at 2 / 3 / 4 players. Zero means "not in the deck"
    /// (wonders, leaders and starting techs are dealt by another route).
    pub count: [u8; 3],
    /// Printed `production` dict verbatim (§ farms/mines/urban buildings).
    /// Zero-valued (`Production::default()`) for everything else.
    pub production: Production,
    pub effects: CardEffects,
    /// Military actions this card costs to play. Printed inside a `cost` dict
    /// in the data rather than alongside the other costs, which is why it took
    /// a second pass to notice it was missing.
    pub military_action_cost: u8,
    /// Tactic cards only (§10): the army this tactic forms, and what it is
    /// worth once obsolete. `Composition::is_empty()` for every non-tactic.
    pub composition: Composition,
    /// Strength of an army built from units a full age behind the tactic
    /// (§10.4). Zero on the early tactics, which have no reduced rate.
    pub obsolete_strength: u8,
    /// The card's unique rules. Empty for the majority whose whole behaviour is
    /// `effects`. A slice rather than one value because several cards carry two
    /// or three unrelated one-offs, and because events additionally carry their
    /// targeting this way until the events port gives targeting its own type.
    pub special: &'static [Special],

    /// Wonders only (§9): resources to build each stage, in order --
    /// Pyramids is `[3, 2, 1]`. Empty for every non-wonder. `&'static [u8]`
    /// rather than a fixed array for the same reason `special` above is a
    /// slice: every wonder in the base game has 2-5 stages and a
    /// fixed-length array would need to be the max width, padded, for the
    /// rest -- ambiguous with a real zero-cost stage, which never happens
    /// but would be unrepresentable-vs-undetectable if it did.
    pub stages: &'static [u8],
    /// Governments only (§8.3): science cost to develop this government
    /// peacefully, through the normal `develop` action. Zero (Despotism,
    /// and every non-government card) means "not developable this way" --
    /// distinct from `science_cost` above, which is populated from the
    /// data's `techCost` key and is 0 for every government (a government's
    /// `techCost` is always null: it is never taken like an ordinary
    /// technology). See `revolution_cost` for the other price a government
    /// has.
    pub peaceful_cost: u8,
    /// Governments only (§8.3.4): science cost to seize this government by
    /// violent revolution instead, gated on every civil action this turn
    /// still being unspent (`engine/actions.py::_can_revolt`). A DIFFERENT
    /// number from `peaceful_cost` for the same government -- Paul has
    /// ruled both paths must be representable at once, so this is its own
    /// field, not a fallback for `peaceful_cost`.
    pub revolution_cost: u8,
    /// Territory (colonization) cards only (§11.5): the one-shot gain paid
    /// out the moment this colony is claimed. See [`ImmediateEffects`]'s
    /// doc comment for why this is not folded into `effects`.
    pub immediate_effects: ImmediateEffects,
}

/// The units a tactic's army is made of (§10.2).
///
/// The data prints this as a list -- `["infantry", "infantry", "cavalry"]` --
/// but every rule that reads it treats it as a multiset: `army_strength`
/// accumulates the player's units by type and asks how many complete copies of
/// the composition they can field. Counts per type say that directly, so the
/// consumer does not re-derive them on every stats recomputation, and an army
/// with a unit type nobody prints is unrepresentable rather than merely absent.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Composition {
    pub infantry: u8,
    pub cavalry: u8,
    pub artillery: u8,
    pub air: u8,
}

impl Composition {
    #[inline]
    pub fn is_empty(self) -> bool {
        self == Composition::default()
    }

    /// Units of one type this army needs. Returns 0 for a non-unit type, which
    /// is what every caller wants -- an army never requires a farm.
    #[inline]
    pub fn need(self, kind: CardType) -> u8 {
        match kind {
            CardType::Infantry => self.infantry,
            CardType::Cavalry => self.cavalry,
            CardType::Artillery => self.artillery,
            CardType::Air => self.air,
            _ => 0,
        }
    }
}

/// A rule that belongs to exactly one card (or a handful).
///
/// This is the enum that makes the port's central guarantee: the `match` on it
/// in `effects.rs` is exhaustive, so **a card whose rule the engine cannot
/// interpret is a compile error**. Python's equivalent is name-dispatch over a
/// `"text"` field, where an unhandled card silently does nothing -- the exact
/// bug class ("in this registry, not in that one, and nothing fails when they
/// disagree") that this rewrite exists to close.
///
/// Variants are GENERATED into `card_table.rs` by `tools/gen_cards.py`, one per
/// distinct one-off effect key, so this enum cannot drift from the card data.
/// Do not hand-edit the variant list; edit the generator.
pub use crate::card_table::Special;

pub use crate::card_table::{CARDS, NUM_CARDS};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_round_trip_through_names() {
        for (i, c) in CARDS.iter().enumerate() {
            assert_eq!(CardId::by_name(c.name), Some(CardId(i as u16)));
        }
    }

    #[test]
    fn none_is_not_a_real_card() {
        assert!(NUM_CARDS < CardId::NONE.0 as usize);
    }
}
