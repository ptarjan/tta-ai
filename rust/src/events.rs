//! Events: gain application (§5.3/§5.4.6), §5.2/§5.3 event RESOLUTION during
//! play, and §12.5.2 final scoring. Ports `engine/events.py` end to end for
//! everything except aggressions/wars (`combat.rs`'s job, see that module)
//! and colonization's own auction machinery (`interact.rs`, already landed
//! and reused here, not duplicated).
//!
//! Landed in three passes: `apply_gains` (commit `044fd92`), §12.5.2 final
//! scoring (`3792e10`), and event resolution -- `reveal_current_event`,
//! `_recycle_future_events`, `_sync_current_events_age`, `resolve_event`,
//! `_apply_player_block` and everything under it (`_apply_extras`,
//! `_queue_decisions`, `_conditional_target`, `_extra_production`,
//! `_draw_military`) -- added 2026-08-05, this pass.
//!
//! ## What `apply_gains` operates on
//!
//! Python's `apply_gains(state, p, block, rng, sign)` takes an arbitrary
//! dict -- sometimes a whole card's `effects`, sometimes a nested sub-dict an
//! event prints under `allPlayers`/`weakestPlayer`/etc (§5.3). This module
//! has TWO functions for the two shapes, both `sign`-scaled the same way:
//! [`apply_gains`] reads a whole CARD's own top-level `effects`
//! (`cards::CardEffects`) -- `combat.rs::finish_aggression`'s success-gains
//! call is its only caller, an aggression card's own effects being gains to
//! the attacker; [`apply_gains_block`] reads one [`cards::EventBlock`] --
//! `resolve_event`'s targeting keys and its `gain`/`lose` blocks all read
//! through this one instead, and it is also the first step of
//! [`apply_player_block`] (§5.3's `_apply_player_block`, which layers
//! `scoring_culture`'s one reachable ordinary-event key, `_apply_extras` and
//! `_queue_decisions` on top).
//!
//! Reusing `CardEffects` for [`apply_gains`] (rather than a dedicated
//! struct, the way `PactBlock` exists precisely to avoid overloading
//! `CardEffects`) is safe for a structural reason, not a coincidental one:
//! `effects::compute` only ever reads a `CardEffects` off a `CardId` sitting
//! in one of a player's SLOTS (`p.techs`/`wonder`/`tactic`/`government`/
//! `leader`). Event/aggression/war cards are never placed in any of those
//! slots -- they resolve and are discarded -- so `compute` structurally
//! cannot ever read the very fields [`apply_gains`] is about to interpret as
//! one-shot gains. `EventBlock` gets its own dedicated struct instead
//! (`cards::EventBlock`'s own doc comment) because it needs fields
//! `CardEffects` has none of at all (`decreasePopulation` as a real amount,
//! every `_apply_extras`/`_queue_decisions` field) and is never read by
//! `compute` either way.
//!
//! ## Keys `apply_gains`/`apply_gains_block` do not implement, and why
//!
//! FLAGGED, not routed around (this project's standing rule: reproduce a
//! real gap faithfully and say so, rather than silently drop it). A handful
//! of Python's `apply_gains` key branches have no reachable path through
//! EITHER of this port's two call sites, verified against a full
//! `effects`-key census over all 236 base-2015 cards, at every nesting depth
//! (2026-08-05): `loseScience`, `loseCulture`, `population`/
//! `gainPopulation`, and a BARE (top-level, i.e. not nested inside a
//! targeting dict) `yellowTokens` are printed by ZERO cards anywhere in the
//! base game's data. `gen_cards.py` only emits a `Special`/`EventBlock`
//! field for a key it has actually seen, so there is no variant/field for
//! any of these four and no card's data could ever populate one. A branch
//! against a value that cannot exist would be dead code nothing exercises --
//! the opposite of "a card whose rule the engine cannot interpret is a
//! compile error". If a future data revision (or the expansion, out of
//! scope by standing decision) ever prints one, `gen_cards.py`'s exhaustive
//! key census fails the build and names it, at which point it gets a real
//! field and a real arm here.
//!
//! Every OTHER `apply_gains` key this project's earlier worker flagged as
//! unreachable through the aggression-only path IS reachable through event
//! resolution and IS implemented below: `loseAllStoredFood` (bare, under
//! `allPlayers` -- Rats), a bare `drawMilitaryCards` (Development of
//! Politics/Politics of Strength), `increasePopulation` (Development of
//! Settlement/Immigration/Refugees), and a bare `foodAndOrResources` (the
//! `gain`/`lose` blocks beside `strongestPlayers`/`weakestPlayers` -- Foray/
//! Raiders). `decreasePopulation`/`losePopulation` was already implemented
//! (`Special::DecreasePopulation`, Barbarians' top-level printing) but
//! genuinely unreachable before this pass; [`conditional_target`] is now its
//! live caller.

use crate::apply;
use crate::cards::{Age, CardId, CardType, EventBlock, FinalScoringStat, LastRoundSubstituteBlock, Special};
use crate::economy;
use crate::effects;
use crate::game;
use crate::interact;
use crate::state::{
    FreeBuildSpec, GainOption, GainOptions, GameState, OneTimeDiscount, PlayerState, QueueItem,
};

/// §5.3/§5.4.6: apply one card's own top-level gain effects to player `idx`.
/// Mirrors `engine/events.py::apply_gains` -- see this module's top doc
/// comment for exactly which of Python's key branches are implemented, why
/// the rest are not, and why `card: CardId` stands in for Python's
/// `block: dict`.
///
/// `sign = -1` inverts every gain into a loss (Python's own docstring:
/// "`sign=-1` inverts (lose blocks)"). `combat::finish_aggression` always
/// calls this with `sign = 1` (an aggression's own effects are gains to the
/// ATTACKER, never losses) -- the parameter exists anyway because it is the
/// one thing separating this from a second, near-identical copy of the
/// function the moment a lose-block caller exists, exactly as in Python.
pub fn apply_gains(state: &mut GameState, idx: u8, card: CardId, sign: i32) {
    let eff = card.get().effects;

    // science / gainScience (events.py:47-49) -- both keys apply
    // identically in Python, so both fields are walked the same way. Each
    // is its own statement (not summed first) so a card that somehow
    // printed both would clamp at zero exactly where Python's per-key dict
    // loop would, rather than only after combining them.
    for delta in [eff.science, eff.gain_science] {
        add_clamped(&mut state.players[idx as usize].science, sign * delta as i32);
    }
    // culture / gainCulture (events.py:53-55).
    for delta in [eff.culture, eff.gain_culture] {
        add_clamped(&mut state.players[idx as usize].culture, sign * delta as i32);
    }
    // food / gainFood (events.py:59-64). `produceFood` is the third key
    // spelling Python accepts here, but it is only ever printed as a
    // BOOLEAN flag in the base data (`_num` rejects bools -- see
    // `_apply_extras`, out of this port's scope, which is what actually
    // reads that flag), so it never contributes a magnitude through this
    // path regardless.
    for delta in [eff.food, eff.gain_food] {
        apply_food_delta(state, idx, delta, sign);
    }
    // resources / gainResources (events.py:65-70). Same `produceResources`
    // caveat as food above.
    for delta in [eff.resources, eff.gain_resources] {
        apply_resources_delta(state, idx, delta, sign);
    }
    // blueTokens (events.py:91-93).
    if eff.blue_tokens != 0 {
        let p = &mut state.players[idx as usize];
        p.blue_total = (p.blue_total as i32 + sign * eff.blue_tokens as i32).max(0) as u8;
    }
    // strength (events.py:94-96) -- a ONE-SHOT grant via `strength_extra`,
    // not the recurring per-turn `CardEffects.strength` a card sitting in a
    // player's tableau contributes through `effects::compute`. See this
    // module's top doc comment for why the two meanings cannot collide.
    if eff.strength != 0 {
        state.players[idx as usize].strength_extra += (sign as i16) * eff.strength;
    }
    // happiness / happy (events.py:97-99) -- same one-shot-via-`_extra`
    // reasoning as strength above.
    if eff.happy != 0 {
        state.players[idx as usize].happy_extra += (sign as i16) * eff.happy;
    }
    // decreasePopulation / losePopulation (events.py:82-87) -- §6.5/FAQ
    // p.15: the OWNER chooses which worker to lose, so this only enqueues
    // the decision. Only the `decreasePopulation` spelling is ever printed
    // top-level in the base data (Barbarians); `Special::DecreasePopulation`
    // is the int-shape variant `gen_cards.py` already emits for it.
    for &sp in card.get().special {
        if let Special::DecreasePopulation(n) = sp {
            if n != 0 {
                interact::enqueue(state, QueueItem::LosePop { player: idx, n: n as u8 });
            }
        }
    }
}

/// `p.X = max(0, p.X + delta)` (events.py's repeated `p.science = max(0,
/// p.science + sign * v)` idiom) -- a no-op when `delta` is zero, matching
/// Python's own `if v:` guard on every branch that uses it.
fn add_clamped(field: &mut u16, delta: i32) {
    if delta != 0 {
        *field = (*field as i32 + delta).max(0) as u16;
    }
}

/// One `food`/`gainFood` key application (events.py:59-64): the blue-token-
/// limited [`economy::gain_food`] when gaining, plain floored subtraction
/// when losing -- `sign` chooses which, `p.food` itself is never allowed
/// negative either way.
fn apply_food_delta(state: &mut GameState, idx: u8, delta: i16, sign: i32) {
    if delta == 0 {
        return;
    }
    if sign > 0 {
        economy::gain_food(&mut state.players[idx as usize], delta as u16);
    } else {
        let p = &mut state.players[idx as usize];
        economy::pay_food(p, delta as u16);
    }
}

/// The `resources`/`gainResources` twin of [`apply_food_delta`]
/// (events.py:65-70).
fn apply_resources_delta(state: &mut GameState, idx: u8, delta: i16, sign: i32) {
    if delta == 0 {
        return;
    }
    if sign > 0 {
        economy::gain_resources(&mut state.players[idx as usize], delta as u16);
    } else {
        let p = &mut state.players[idx as usize];
        economy::pay_resources(p, delta as u16);
    }
}

// ENGINE BUG FIX (FOODFIX, 2026-08): §5.4.6/§11.5's bare `foodAndOrResources`
// key used to be applied here by a fixed "resources first" formula --
// mirroring `engine/events.py::_food_or_resources`, itself only ever a
// reference-bot default, not a rule. RULES_SPEC.md §5.3 ("Multiple-player
// decisions resolve clockwise from the revealing player") plus BGO journal
// evidence (game 7522886: "Green choses first", then Green and Orange each
// resolving an IDENTICAL total-2 loss with DIFFERENT real splits) show the
// split is the TARGETED PLAYER's own choice, exactly like Plunder's already-
// fixed `interact::offer_plunder_split` -- just self-directed rather than
// attacker-directed. `apply_gains_block`'s `food_and_or_resources` arm below
// now enqueues `QueueItem::FoodOrResSplit` instead of calling a fixed
// formula (`decrease_population`'s existing `QueueItem::LosePop` enqueue,
// three lines below in that same function, is the shape this copies). The
// old `food_or_resources` function had exactly one caller -- this arm -- so
// it is deleted outright rather than left dead; its resolution logic now
// lives in `interact::resolve_choice`'s `ChoiceKind::FoodOrResSplit` arm.

// ==================================================== §12.5.2 final scoring

/// The Age III scoring events still owed a payout, in scoring order.
/// Mirrors `engine/events.py::pending_final_events`, including its exclusion
/// of `past_events` -- see this module's top doc comment for why an event
/// there already paid out through `_apply_player_block`.
///
/// Python returns `[(name, block), ...]`; this returns just the `CardId`s,
/// since `block` here is `card.get().special`'s own `Special::FinalScoring`
/// payload rather than a separate dict -- [`final_scoring_block`] reads it
/// straight off the card, so there is nothing a second return value would
/// carry that the caller cannot already get from the `CardId` alone.
fn pending_final_events(state: &GameState) -> Vec<CardId> {
    state
        .current_events
        .as_slice()
        .iter()
        .chain(state.future_events.as_slice().iter())
        .copied()
        .filter(|c| !c.is_none() && final_scoring_block(*c).is_some())
        .collect()
}

/// The `Special::FinalScoring` payload off one card, if it has one. `None`
/// for every card except the 15 base-game `scoringEvent` events (see
/// `cards::FinalScoringBlock`'s doc comment).
///
/// `pub(crate)`: `bots::weighted::events::scoring_weights` also needs this,
/// to mirror Python's `_scoring_weights` filtering its event pool down to
/// cards `final_event_awards` will actually score -- reusing this rather
/// than restating "does this card score" as a second predicate.
pub(crate) fn final_scoring_block(card: CardId) -> Option<&'static crate::cards::FinalScoringBlock> {
    card.get().special.iter().find_map(|sp| match sp {
        Special::FinalScoring(block) => Some(block),
        Special::A(_) | Special::AllPlayers(_) | Special::B(_) | Special::BestTheaterDoubleCulture | Special::BothPlayers(_) | Special::BuildDiscount(_) | Special::CancelledIfPartiesAttackEachOther | Special::CannotPlayAggressionOrWar | Special::CivilActionBackOnTechDevelop(_) | Special::CivilActionUpgradeUrbanBuildingToTheater | Special::ColonizeDiscardUpTo2MilitaryCardsForBonus(_) | Special::ColonyImmediateBonusApplies | Special::ColonyPermanentBonusTransfers | Special::ComboFoodDiscount(_) | Special::ComboResourceDiscount(_) | Special::Condition(_) | Special::CultureFirstColony(_) | Special::CultureIfTopTwoStrength(_) | Special::CultureOnLeaveEqualToLabResourceProduction | Special::CultureOnRevolution(_) | Special::CultureOnTechDevelop(_) | Special::CulturePerAdditionalColony(_) | Special::CulturePerCivilizationWithMoreCulture(_) | Special::CulturePerHappyFromTemplesTheatersWonders(_) | Special::CulturePerLabEqualToLevel | Special::CulturePerLibraryTheaterPair(_) | Special::CulturePerTheater(_) | Special::DecreasePopulation(_) | Special::DestroyUrbanBuildings(_) | Special::DoubleBestMine | Special::DoublesTacticBonusOfOneArmy | Special::ExtraHappyPerHappySource(_) | Special::FreeCivilAction(_) | Special::FreePopIncreasePerTurn | Special::Gain(_) | Special::GainCulturePerLevelOfRemovedCard(_) | Special::GainFoodOrResources(_) | Special::GainResources(_) | Special::InfantryCountsAsCavalryForTactics | Special::LastRoundSubstitute(_) | Special::LeaderTakeCivilActionDiscount(_) | Special::LibraryDiscountsIfTheater | Special::Lose(_) | Special::MilitaryActionAsCivilPerTurn(_) | Special::MilitaryActionCombinedPopIncreaseAndUnitBuild | Special::NoAttacksBetweenParties | Special::OnAttackBetweenParties(_) | Special::OnBuildCulture(_) | Special::OnBuildCulturePerTechLevelSum | Special::OnReplacePutUnderCompletedWonderHappy(_) | Special::OncePerGameTwoPoliticalActions | Special::OpponentDecreasesPopulation(_) | Special::OpponentsPayDoubleMilitaryActionsToAttackYou | Special::OrTakesSpecialTechnologiesOfSameTotalScienceCost | Special::PeekTopEventCardInPolitics | Special::PerTurnChoice | Special::PlayerWithLeastCulture(_) | Special::PlayerWithMostCulture(_) | Special::PlayersWithMostDiscontentWorkers(_) | Special::PlayersWithMostHappyFaces(_) | Special::PopIncreaseFoodDiscount(_) | Special::RemoveAsPoliticalActionForYellowToken(_) | Special::RemoveAsPoliticalActionFreeColonize | Special::RemoveFromGame | Special::ResourceOnMilitaryUnitBuildOrUpgrade(_) | Special::ResourceOnTechDevelop(_) | Special::ResourcesForMilitaryUnitsPerStrongerCivilization(_) | Special::ResourcesPerLabEqualToLevel | Special::RevolutionUsesMilitaryActionsInstead | Special::ScienceOnTechCardTake(_) | Special::SciencePerBestLabOrLibraryLevel | Special::SciencePerLab(_) | Special::StealColony(_) | Special::StrengthPerArtillery(_) | Special::StrengthPerInfantry(_) | Special::StrengthPerMilitaryUnit(_) | Special::StrengthPerTempleOrGovernmentHappy(_) | Special::StrengthPerUnitType(_) | Special::StrongestPlayer(_) | Special::StrongestPlayers(_) | Special::TakeCivilActionDiscountIfLeaderReplacedThisTurn(_) | Special::TakeFromOpponent(_) | Special::TheaterResourceDiscountIfLibrary(_) | Special::TheaterScienceDiscountIfLibrary(_) | Special::TheaterTechScienceDiscount(_) | Special::VictorTakesCulture | Special::VictorTakesScienceUpTo(_) | Special::VictorTakesYellowTokens | Special::WeakestPlayer(_) | Special::WeakestPlayers(_) | Special::WonderTakeNoExtraCivilActions => None,
    })
}

/// Live players in clockwise turn order starting at `first_idx` (§5.3
/// tie-break). Mirrors `engine/events.py::_order_from`. [`resolve_event`]/
/// [`conditional_target`] start from the revealer; [`order_from_start`]
/// below is the one other shape this port needs, `interact::start_auction`'s
/// own copy (predating this module) is `interact.rs`'s file to unify, not
/// this one's.
fn order_from(state: &GameState, first_idx: u8) -> Vec<u8> {
    let n = state.num_players;
    (0..n)
        .map(|i| (first_idx + i) % n)
        .filter(|&idx| !state.players[idx as usize].resigned)
        .collect()
}

/// [`order_from`] starting at `state.start_player` -- the one shape
/// [`final_event_awards`] needs (§12.5.2: the starting player counts as the
/// current player for final scoring).
fn order_from_start(state: &GameState) -> Vec<u8> {
    order_from(state, state.start_player)
}

/// The one statistic value `final_event_awards`'s `rankingCulture` ranking
/// ever needs. Mirrors `engine/events.py::_stat_value`, narrowed to the five
/// `FinalScoringStat` targets `_STAT_ALIASES` maps final-scoring's
/// `statistic` key onto -- Python's fuller version also answers
/// `happy`/`discontent`/`culture` for `resolve_event`'s targeting callers,
/// out of scope here.
fn final_scoring_stat_value(state: &GameState, p: &PlayerState, stat: FinalScoringStat) -> i32 {
    let s = effects::state_stats(state, p);
    match stat {
        FinalScoringStat::Strength => s.strength,
        FinalScoringStat::Science => s.science,
        // `s.science` here IS the per-round science PRODUCTION (the marker
        // position) -- the FAQ's "all sources of Science production that
        // contribute to your per-round Science marker position, including
        // Leaders and Wonders and Colonies but never Action Cards":
        // `effects::compute` sums card/worker production, government, leader
        // and colony contributions, and `science_rate_extra` (a permanent
        // board grant), while the SPENDABLE science total lives in
        // `PlayerState::science`, which this function never reads. Action
        // cards are hand-only, so they can never enter it.
        FinalScoringStat::ScienceProduction => s.science,
        FinalScoringStat::CultureRate => s.culture,
        FinalScoringStat::Food => s.food,
        FinalScoringStat::Resources => s.resources,
    }
}

/// `order`'s players, best-`stat`-first, ties broken by position in `order`
/// (§5.3 tie-break: whoever is closer to the start player in turn order).
/// Mirrors `engine/events.py::_rank(state, order, stat, best_first=True)` --
/// the only `best_first` value `final_event_awards` ever passes -- via a
/// stable sort on `order` itself: `order` is already in tie-break order, so
/// a stable sort preserves ties in exactly the position Python's explicit
/// `idx[q.idx]` secondary key does, with no second key needed.
fn rank_by_final_scoring_stat(state: &GameState, order: &[u8], stat: FinalScoringStat) -> Vec<u8> {
    let mut ranked = order.to_vec();
    ranked.sort_by_key(|&idx| -final_scoring_stat_value(state, &state.players[idx as usize], stat));
    ranked
}

/// Culture awarded by ONE "Impact of ..." Age III scoring event to ONE
/// player. Ports `engine/events.py::scoring_culture`'s per-key dispatch --
/// `block`'s fields are [`crate::cards::FinalScoringBlock`]'s zero-default
/// fields rather than a dict entry per printed key, so every branch below
/// runs unconditionally and is a no-op on a card that did not print that
/// key, the same "0 = not printed" convention `TakeFromOpponentBlock`'s/
/// `PactBlock`'s magnitude fields already use.
///
/// Python's `scoring_culture(state, p, block, order)` takes a fourth `order`
/// parameter its own body never reads anywhere (checked 2026-08-05) -- a
/// dead parameter, not ported.
fn scoring_culture(
    state: &GameState,
    p: &PlayerState,
    block: &crate::cards::FinalScoringBlock,
) -> i32 {
    let s = effects::state_stats(state, p);
    let mut total: i32 = 0;

    // culturePerResourceProducedByMines -- "Impact of Industry".
    total += block.culture_per_resource_produced_by_mines as i32 * effects::mine_resources(p);

    // culturePerFoodProducedByFarms (+ bonusIfProductionExceedsConsumption)
    // -- "Impact of Agriculture". The two keys only ever co-occur on this
    // one base-game card (see `FinalScoringBlock`'s doc comment), so gating
    // the bonus on its own nonzero value alone reproduces Python's
    // per-key-branch reading exactly for every card that exists today.
    total += block.culture_per_food_produced_by_farms as i32 * effects::farm_food(p);
    if block.bonus_if_production_exceeds_consumption != 0
        && s.food > economy::consumption(p.yellow_bank) as i32
    {
        total += block.bonus_if_production_exceeds_consumption as i32;
    }

    // culturePerLevelOfMilitaryUnitsAndArenas -- "Impact of Competition".
    for (id, slot) in p.techs.iter() {
        if id.kind().is_unit() || id.kind() == CardType::Arena {
            total += block.culture_per_level_of_military_units_and_arenas as i32
                * id.level() as i32
                * slot.workers as i32;
        }
    }

    // culturePerLevelOfSpecialTechsAndGovernment -- "Impact of Progress".
    // §12.5.2 / card text: "Each civilization scores 2 culture per level of
    // each of its government and special (blue) technologies." The card
    // counts the government's level plus every SpecialTech (blue tech) the
    // player has built. It does NOT count Temples (Religion / Theology /
    // Organized Religion): those are a separate card type in this engine
    // (`CardType::Temple`), not "special (blue) technologies" by the card's
    // own wording -- the printed text (corpus-wide constant, 363/363
    // BGO journal lines) says "special (blue) technologies" exactly, and a
    // Temple's blue back is a card-category marker, not a "special tech".
    // This reading had a WRONG justification on 2026-08-16: the pass's
    // first draft cited game `7521849`'s journal line ("Orange scores 12
    // culture; Purple scores 12 culture") as confirming 12/12 from the
    // engine, but that line's "12" for Purple was a stale IN-PLAY value --
    // the card fired mid-game (Purple's round-17 "plays event" line
    // stated 10/16, before Purple's final-round builds), BGO's own
    // end-of-game line carried the error, and Purple's true end-board
    // (Democracy III + Masonry I + Theology I temple, the last build a
    // Theology->Organized Religion upgrade) scores 14 under this formula.
    // The engine's 14 is the correct reading, and 7521849's final-score
    // cross-check (index 64 = 40 end-of-turn + 14 Progress + 3 Churchill +
    // 7 wonder) proves the engine's 14 is what the game itself recorded.
    let mut tech_and_gov_levels = p.government.level() as i32;
    for (id, _) in p.techs.of_type(CardType::SpecialTech) {
        tech_and_gov_levels += id.level() as i32;
    }
    total += block.culture_per_level_of_special_techs_and_government as i32 * tech_and_gov_levels;

    // culturePerCompletedWonderByAge -- "Impact of Wonders": a per-age
    // table, indexed by `Age as u8` exactly like `Special::BuildDiscount`.
    for &w in p.completed_wonders.as_slice() {
        total += block.culture_per_completed_wonder_by_age[w.get().age as usize] as i32;
    }

    // culturePerContentWorkerAbove10 -- "Impact of Population". FAQ v15:
    // "Count the number of yellow markers that are not in your Population
    // Bank. Subtract from this the number of Discontent Workers that you
    // have. This is your number of Content Workers." `yellow_bank` IS the
    // Population Bank, so the set is on-card workers PLUS `workers_free`.
    // The pool is NOT excluded -- excluding it costs 141 exact score matches
    // over the corpus (see analysis/worker_notes_2026-08-15/
    // impact_of_population_pool_exclusion.txt). And note the same FAQ page
    // says discontent workers are "never associated with any specific yellow
    // cubes", so the old folklore that a discontent worker is a pool token
    // moved onto a happiness track is simply false; it is an abstract count.
    let on_card: i32 = p.techs.iter().map(|(_, slot)| slot.workers as i32).sum::<i32>();
    let workers: i32 = on_card + p.workers_free as i32;
    let disc = economy::discontent(state, p);
    let content = (workers - disc).max(0);
    if std::env::var("SCOREDIV_EVENT_DEBUG").is_ok() && block.culture_per_content_worker_above_10 != 0 {
        eprintln!(
            "SCOREDIV_POPULATION player={} on_card={} workers_free={} workers={} discontent={} \
             content={} yellow_bank={} s.happy={}",
            p.idx, on_card, p.workers_free, workers, disc, content, p.yellow_bank, s.happy
        );
    }
    total += block.culture_per_content_worker_above_10 as i32 * (content - 10).max(0);

    // culturePerColony -- "Impact of Colonies".
    total += block.culture_per_colony as i32 * p.colonies.len() as i32;

    // culturePerCivilAction / culturePerMilitaryAction -- "Impact of
    // Government". Scores the player's current ACTION ALLOWANCE
    // (`state_stats`'s `civil_actions`/`military_actions`), not actions
    // actually spent over the course of the game -- matching Python's own
    // `s.civil_actions`/`s.military_actions` read exactly.
    total += block.culture_per_civil_action as i32 * s.civil_actions;
    total += block.culture_per_military_action as i32 * s.military_actions;

    // culturePerLevelOfUrbanBuildings -- "Impact of Architecture".
    for (id, slot) in p.techs.iter() {
        if id.kind().is_urban() {
            total += block.culture_per_level_of_urban_buildings as i32
                * id.level() as i32
                * slot.workers as i32;
        }
    }

    // culturePerHappyFace (+ maxCultureFromHappyFaces cap) -- "Impact of
    // Happiness". `max_culture_from_happy_faces == 0` means "no cap" -- see
    // `FinalScoringBlock`'s doc comment; no base-game card prints an actual
    // cap of 0.
    let mut happy_gain = block.culture_per_happy_face as i32 * s.happy;
    if block.max_culture_from_happy_faces != 0 {
        happy_gain = happy_gain.min(block.max_culture_from_happy_faces as i32);
    }
    total += happy_gain;

    // culturePerDiscontentWorker -- also "Impact of Happiness" (negative on
    // that card: -2 per discontent worker).
    let disc = economy::discontent(state, p);
    if std::env::var("SCOREDIV_EVENT_DEBUG").is_ok() && block.culture_per_happy_face != 0 {
        eprintln!(
            "SCOREDIV_HAPPINESS player={} s.happy={} discontent={} happy_gain={} yellow_bank={} workers_free={}",
            p.idx, s.happy, disc, happy_gain, p.yellow_bank, p.workers_free
        );
    }
    total += block.culture_per_discontent_worker as i32 * disc;

    // culturePerAgeIIITechnology -- "Impact of Technology".
    let mut n_iii = p.techs.iter().filter(|(id, _)| id.get().age == Age::III).count() as i32;
    if p.government.get().age == Age::III {
        n_iii += 1;
    }
    total += block.culture_per_age_iii_technology as i32 * n_iii;

    // cultureTimesLowestProduction -- "Impact of Balance".
    total +=
        block.culture_times_lowest_production as i32 * s.food.min(s.resources).min(s.science).min(s.culture);

    // culturePerDistinctTypeOfUnitUrbanBuildingAndSpecialTech -- "Impact of
    // Variety". Python builds this by unioning distinct CARD TYPES (staffed
    // urban/unit buildings) with distinct CARD NAMES (special techs) into
    // ONE set -- a special tech is always unique by construction, so that
    // union is exactly "count of distinct types present" + "count of
    // special techs held"; counted that way directly here (a bitmask over
    // `CardType`, DESIGN.md rule 3) instead of replicating the string-set
    // trick.
    let mut urban_or_unit_kinds: u32 = 0;
    for (id, slot) in p.techs.iter() {
        let kind = id.kind();
        if slot.workers > 0 && (kind.is_urban() || kind.is_unit()) {
            urban_or_unit_kinds |= 1 << (kind as u32);
        }
    }
    let distinct = urban_or_unit_kinds.count_ones() as i32
        + p.techs.of_type(CardType::SpecialTech).count() as i32;
    total += block.culture_per_distinct_type_of_unit_urban_building_and_special_tech as i32
        * distinct;

    total
}

/// THE Age III final-scoring calculation (§12.5.2). Everything else calls
/// this. Mirrors `engine/events.py::final_event_awards`.
///
/// Returns one entry per pending scoring event, holding the individual
/// culture awards **in the order the engine applies them** -- for each
/// player in turn order starting at `state.start_player`, the
/// [`scoring_culture`] award and then, if the event carries a ranking table,
/// the `rankingCulture` award. The step list is deliberately not pre-summed:
/// [`evaluate_final_events`] clamps a player's running culture at zero after
/// EACH award, so a player near zero with a net-negative scoring board gets
/// a different total from the pooled sum.
///
/// Python recomputes its `_rank` call fresh on every iteration of the
/// per-player loop (`events.py:583`), even though nothing in the loop body
/// changes it between iterations; this hoists it once per event instead --
/// same result, no wasted work, not a behaviour change.
pub fn final_event_awards(state: &GameState) -> Vec<(CardId, Vec<(u8, i32)>)> {
    let order = order_from_start(state);
    let live = game::live_count(state);
    let mut out = Vec::new();
    for card in pending_final_events(state) {
        let block = final_scoring_block(card).expect(
            "pending_final_events only returns cards final_scoring_block resolves to Some",
        );
        // §12.5.2's ranking tables are defined only for two or more
        // civilizations. A game that ended by resignation (BGO lets a seat
        // "concede" mid-round, and §5.11 ends the game at the first
        // resignation) can have exactly ONE civilization left standing, and
        // the journal's own final lines then state only that player's award
        // (game `7522397`: "Purple scores 10 culture" alone on Impact of
        // Strength/Population, with no 2p table applied). So the gate reads
        // the RAW survivor count, not `game::live_count` -- that one is
        // clamped to 2..=4 because its other consumers index tables with it,
        // and clamping here would invent a two-player ranking nobody played.
        let ranked = if block.has_ranking && state.active().count() >= 2 {
            Some(rank_by_final_scoring_stat(state, &order, block.ranking_stat))
        } else {
            None
        };
        let table: &[i16] = match live {
            2 => &block.ranking_2p,
            3 => &block.ranking_3p,
            _ => &block.ranking_4p,
        };

        let mut steps = Vec::new();
        for &idx in &order {
            let p = &state.players[idx as usize];
            steps.push((idx, scoring_culture(state, p, block)));
            if let Some(ranked) = &ranked {
                if let Some(pos) = ranked.iter().position(|&r| r == idx) {
                    if pos < table.len() {
                        steps.push((idx, table[pos] as i32));
                    }
                }
            }
        }
        out.push((card, steps));
    }
    out
}

/// Applies [`final_event_awards`] -- it does not recompute anything. Mirrors
/// `engine/events.py::evaluate_final_events`; the starting player counts as
/// the current player (`order_from_start`).
///
/// Python's `if state.has_military:` guard is not reproduced: `has_military`
/// is a card-database-completeness flag, always true for this engine's
/// always-complete 236-card table (same reasoning `legal.rs::politics_moves`
/// already documents for the same guard), so there is no `state.has_military`
/// field to read. Python also `state.emit`s a log line per event; there is
/// no journal/emit sink in this port and nothing reads the string (same
/// treatment `economy.rs::end_of_turn` already gives every other dropped
/// `emit` call).
pub fn evaluate_final_events(state: &mut GameState) {
    for (card, steps) in final_event_awards(state) {
        // TEMPORARY score-divergence investigation aid (2026-08-13) -- not
        // part of the shipped diagnostic surface, gated behind an env var so
        // normal runs are silent. Prints every per-card, per-player award
        // this call computes, in application order, so a diverging game's
        // final score can be traced to the exact card+player+amount without
        // re-deriving `scoring_culture`'s formula from the journal by hand.
        if std::env::var("SCOREDIV_EVENT_DEBUG").is_ok() {
            for &(idx, amount) in &steps {
                eprintln!("SCOREDIV_EVENT card={} player={} amount={}", card.name(), idx, amount);
            }
        }
        for (idx, amount) in steps {
            if amount != 0 {
                let p = &mut state.players[idx as usize];
                p.culture = (p.culture as i32 + amount).max(0) as u16;
            }
        }
    }
}

// ===================================================== §5.2/§5.3 event resolution

/// Reveal and resolve the top card of the current events deck (§5.2).
/// Mirrors `engine/events.py::reveal_current_event`.
pub fn reveal_current_event(state: &mut GameState) -> Option<CardId> {
    if state.current_events.is_empty() {
        recycle_future_events(state);
        if state.current_events.is_empty() {
            return None;
        }
    }
    // `current_events` is only ever popped from the end (mirrors Python's
    // `list.pop()`), so this is the "top" card `peek_top_event`/
    // `current_events[-1]` would have read -- Joan of Arc's peek is a bot-
    // facing convenience (`p.peeked_event`) with no rule effect of its own,
    // and is out of this pass's scope.
    let card = state.current_events.pop().expect("just checked non-empty");
    sync_current_events_age(state);
    if card.kind() == CardType::Territory {
        // §11.1: a territory starts a colonization auction instead.
        interact::start_auction(state, card, state.current);
    } else {
        state.past_events.push(card);
        resolve_event(state, card, state.current);
    }
    if state.current_events.is_empty() {
        recycle_future_events(state);
    }
    Some(card)
}

/// Future events deck becomes the new current events deck (§5.2). Mirrors
/// `engine/events.py::_recycle_future_events`.
fn recycle_future_events(state: &mut GameState) {
    if state.future_events.is_empty() {
        return;
    }
    let mut rng = events_rng(state);
    state.current_events = std::mem::take(&mut state.future_events);
    crate::rng::shuffle_cards(rng.get(), state.current_events.as_mut_slice());
    // `pop()` takes from the end, so earlier ages must sit last -- a stable
    // sort after the shuffle, descending by age, same as Python's
    // `deck.sort(key=lambda n: -_DB.level_of(n))` (also stable).
    state
        .current_events
        .as_mut_slice()
        .sort_by_key(|c| std::cmp::Reverse(c.level()));
    sync_current_events_age(state);
}

/// A deterministic stream for [`recycle_future_events`]'s shuffle.
///
/// This is NOT a faithful reproduction of Python's real stream for this
/// shuffle: `game.play_game` threads ONE persistent `random.Random(seed ^
/// 0x5EED)` through every `apply()` call in order (`game.rs`'s "Randomness"
/// doc comment), and `_recycle_future_events` draws from that same object --
/// `game.rs`'s own KNOWN GAP 2 already names this exact function as one of
/// the two reasons a stateless-per-`end_turn` derivation (`game::rng_for`/
/// `economy::deck_rng`, the pattern this function copies) cannot follow it.
/// So: same kind of derived, deterministic-but-Python-divergent stream as
/// those two, with its own multiplier so it does not collide with either.
/// The ONLY caller of `_recycle_future_events` in Python is `_h_prepare_event`
/// (`engine/actions.py`), which -- since `engine/actions.py::_h_prepare_event`
/// grew the same `_rng_for` backfill every OTHER political/turn handler
/// already had (`engine/game.py::_rng_for`, `2026-08-05`) -- always shuffles
/// through `_rng_for(state, rng)`: the caller-supplied stream if there is
/// one, otherwise `Random(seed * 1000003 + turn * 97 + round)` freshly
/// derived from the state right then. `tools/dump_fixtures.py` (the only
/// place that still supplies one explicitly) passes exactly that same
/// `_rng_for(state)` per `apply()` call now too, so there is only one real
/// stream to match: `game::rng_for`, not a second one here.
///
/// A separate, arbitrary formula (`seed * 15485863 + round * 131 + turn`)
/// used to live in this function, on the theory that the recycle-shuffle
/// stream was independent of `game::rng_for`'s. It never was -- Python always
/// re-derives (or reuses) the SAME object `_h_prepare_event` was called
/// with -- but nothing could tell the two formulas apart while the fixtures
/// still had the persistent-stream problem `game.rs`'s KNOWN GAP 2 documents
/// (any recycle point diverged for THAT reason first). Fixed 2026-08-05 once
/// the fixtures were regenerated through per-apply derived streams and this
/// became the one remaining, checkable difference.
fn events_rng(state: &GameState) -> crate::rng::LazyRandom {
    crate::game::rng_for(state)
}

/// Points `state.current_events_age` at the next card to be revealed.
/// Mirrors `engine/events.py::_sync_current_events_age`.
pub(crate) fn sync_current_events_age(state: &mut GameState) {
    if let Some(&top) = state.current_events.as_slice().last() {
        // The pile can legitimately hold `CardId::NONE`. A position the
        // advisor is mirroring knows how many cards are face down without
        // knowing which, and an unknown card cannot be asked for its age --
        // `CardId::get` indexes the card table, so a sentinel is an
        // out-of-bounds panic. Keeping the age the pile was already showing
        // is the honest answer: nothing was learned, so nothing changes.
        // Third site to need this guard, after `bots::counting::event_pool`
        // and `bots::weighted::events::my_seeds`.
        if !top.is_none() {
            state.current_events_age = top.get().age;
        }
    }
}

/// Resolve one revealed event card (§5.3). Mirrors
/// `engine/events.py::resolve_event`.
pub fn resolve_event(state: &mut GameState, card: CardId, revealer_idx: u8) {
    if card.kind() == CardType::Territory {
        // Defensive parity with Python's own redundant check -- `reveal_
        // current_event` above is this function's only caller and already
        // branches on territory-vs-event before calling it, so this never
        // actually fires today.
        interact::start_auction(state, card, revealer_idx);
        return;
    }
    let order = order_from(state, revealer_idx);
    if order.is_empty() {
        return;
    }
    // TIE_CENSUS labelling only (see `tie_context`'s own doc) -- inert unless
    // `TIE_CENSUS` is set. `card.get().name` is `&'static str` off the card
    // table, so this is a plain reference store, not an allocation.
    crate::tie_context::set_card(card.get().name);

    // Politics of Strength (§5.3, `events.py:228-229`): on the last round,
    // the WHOLE targeting set is swapped for the substitute's -- not merged
    // with it. `cards::LastRoundSubstituteBlock` only carries the two keys
    // the one base-game card that has one actually prints
    // (`strongestPlayer`/`weakestPlayer`), which is why only those two are
    // applied here; see that struct's own doc comment.
    if state.last_round {
        if let Some(sub) = last_round_substitute(card) {
            apply_single_target(state, &order, RankStat::Strength, true, true, sub.strongest_player);
            apply_single_target(state, &order, RankStat::Strength, false, false, sub.weakest_player);
            return;
        }
    }

    // `allPlayers`. Always run: a card with no `allPlayers` key gets
    // `EventBlock::EMPTY`, and applying an all-EMPTY block to every player is
    // a proven no-op (every field-level branch inside `apply_player_block`
    // is itself guarded on that field being nonzero/non-default) -- so there
    // is no need to track "was this key printed at all" separately, here or
    // for any of the five single/tied-target keys below.
    let all_players = event_block(card, |sp| match sp {
        Special::AllPlayers(b) => Some(b),
        Special::A(_) | Special::B(_) | Special::BestTheaterDoubleCulture | Special::BothPlayers(_) | Special::BuildDiscount(_) | Special::CancelledIfPartiesAttackEachOther | Special::CannotPlayAggressionOrWar | Special::CivilActionBackOnTechDevelop(_) | Special::CivilActionUpgradeUrbanBuildingToTheater | Special::ColonizeDiscardUpTo2MilitaryCardsForBonus(_) | Special::ColonyImmediateBonusApplies | Special::ColonyPermanentBonusTransfers | Special::ComboFoodDiscount(_) | Special::ComboResourceDiscount(_) | Special::Condition(_) | Special::CultureFirstColony(_) | Special::CultureIfTopTwoStrength(_) | Special::CultureOnLeaveEqualToLabResourceProduction | Special::CultureOnRevolution(_) | Special::CultureOnTechDevelop(_) | Special::CulturePerAdditionalColony(_) | Special::CulturePerCivilizationWithMoreCulture(_) | Special::CulturePerHappyFromTemplesTheatersWonders(_) | Special::CulturePerLabEqualToLevel | Special::CulturePerLibraryTheaterPair(_) | Special::CulturePerTheater(_) | Special::DecreasePopulation(_) | Special::DestroyUrbanBuildings(_) | Special::DoubleBestMine | Special::DoublesTacticBonusOfOneArmy | Special::ExtraHappyPerHappySource(_) | Special::FinalScoring(_) | Special::FreeCivilAction(_) | Special::FreePopIncreasePerTurn | Special::Gain(_) | Special::GainCulturePerLevelOfRemovedCard(_) | Special::GainFoodOrResources(_) | Special::GainResources(_) | Special::InfantryCountsAsCavalryForTactics | Special::LastRoundSubstitute(_) | Special::LeaderTakeCivilActionDiscount(_) | Special::LibraryDiscountsIfTheater | Special::Lose(_) | Special::MilitaryActionAsCivilPerTurn(_) | Special::MilitaryActionCombinedPopIncreaseAndUnitBuild | Special::NoAttacksBetweenParties | Special::OnAttackBetweenParties(_) | Special::OnBuildCulture(_) | Special::OnBuildCulturePerTechLevelSum | Special::OnReplacePutUnderCompletedWonderHappy(_) | Special::OncePerGameTwoPoliticalActions | Special::OpponentDecreasesPopulation(_) | Special::OpponentsPayDoubleMilitaryActionsToAttackYou | Special::OrTakesSpecialTechnologiesOfSameTotalScienceCost | Special::PeekTopEventCardInPolitics | Special::PerTurnChoice | Special::PlayerWithLeastCulture(_) | Special::PlayerWithMostCulture(_) | Special::PlayersWithMostDiscontentWorkers(_) | Special::PlayersWithMostHappyFaces(_) | Special::PopIncreaseFoodDiscount(_) | Special::RemoveAsPoliticalActionForYellowToken(_) | Special::RemoveAsPoliticalActionFreeColonize | Special::RemoveFromGame | Special::ResourceOnMilitaryUnitBuildOrUpgrade(_) | Special::ResourceOnTechDevelop(_) | Special::ResourcesForMilitaryUnitsPerStrongerCivilization(_) | Special::ResourcesPerLabEqualToLevel | Special::RevolutionUsesMilitaryActionsInstead | Special::ScienceOnTechCardTake(_) | Special::SciencePerBestLabOrLibraryLevel | Special::SciencePerLab(_) | Special::StealColony(_) | Special::StrengthPerArtillery(_) | Special::StrengthPerInfantry(_) | Special::StrengthPerMilitaryUnit(_) | Special::StrengthPerTempleOrGovernmentHappy(_) | Special::StrengthPerUnitType(_) | Special::StrongestPlayer(_) | Special::StrongestPlayers(_) | Special::TakeCivilActionDiscountIfLeaderReplacedThisTurn(_) | Special::TakeFromOpponent(_) | Special::TheaterResourceDiscountIfLibrary(_) | Special::TheaterScienceDiscountIfLibrary(_) | Special::TheaterTechScienceDiscount(_) | Special::VictorTakesCulture | Special::VictorTakesScienceUpTo(_) | Special::VictorTakesYellowTokens | Special::WeakestPlayer(_) | Special::WeakestPlayers(_) | Special::WonderTakeNoExtraCivilActions => None,
    });
    for &q in &order {
        apply_player_block(state, q, &all_players);
    }
    // The 15 "Impact of ..." Age III scoring events (§12.5.2) pay out
    // immediately when revealed during play, through the SAME
    // `scoring_culture`/`rankingCulture` arithmetic `final_event_awards`
    // uses at game end -- see this module's top doc comment for why
    // `Special::FinalScoring`'s own presence, not an `EventBlock`, is what
    // signals "this card has an allPlayers key" for these 15 cards.
    if let Some(block) = final_scoring_block(card) {
        apply_final_scoring_block_live(state, &order, block);
    }

    // Barbarians (§5.3): "the player with most culture, if among the
    // weakest" -- the one base-game card that prints a top-level `target`/
    // `condition`/`decreasePopulation` combination. Python gates this on
    // `"target" in eff and "decreasePopulation" in eff`; `target`'s own text
    // is never read (`gen_cards.py`'s `IGNORED_NESTED_EFFECT_KEYS`) and, in
    // the live data, is printed on exactly the same one card as top-level
    // `decreasePopulation` is, so gating on `Special::DecreasePopulation`'s
    // presence alone reproduces it exactly for every card that exists today.
    if let Some(n) = decrease_population_of(card) {
        conditional_target(state, &order, card, n);
    }

    // §5.3's six ranked/tied keys, in Python's own declared order --
    // load-bearing, since an earlier key's population/discontent changes can
    // shift a later key's ranking (Python recomputes `_rank` fresh at each
    // use, never caches it).
    apply_single_target(
        state,
        &order,
        RankStat::Strength,
        true,
        true, // a bonus for the strongest -- favoring the current player means picking them FIRST
        event_block(card, |sp| match sp {
            Special::StrongestPlayer(b) => Some(b),
            Special::A(_) | Special::AllPlayers(_) | Special::B(_) | Special::BestTheaterDoubleCulture | Special::BothPlayers(_) | Special::BuildDiscount(_) | Special::CancelledIfPartiesAttackEachOther | Special::CannotPlayAggressionOrWar | Special::CivilActionBackOnTechDevelop(_) | Special::CivilActionUpgradeUrbanBuildingToTheater | Special::ColonizeDiscardUpTo2MilitaryCardsForBonus(_) | Special::ColonyImmediateBonusApplies | Special::ColonyPermanentBonusTransfers | Special::ComboFoodDiscount(_) | Special::ComboResourceDiscount(_) | Special::Condition(_) | Special::CultureFirstColony(_) | Special::CultureIfTopTwoStrength(_) | Special::CultureOnLeaveEqualToLabResourceProduction | Special::CultureOnRevolution(_) | Special::CultureOnTechDevelop(_) | Special::CulturePerAdditionalColony(_) | Special::CulturePerCivilizationWithMoreCulture(_) | Special::CulturePerHappyFromTemplesTheatersWonders(_) | Special::CulturePerLabEqualToLevel | Special::CulturePerLibraryTheaterPair(_) | Special::CulturePerTheater(_) | Special::DecreasePopulation(_) | Special::DestroyUrbanBuildings(_) | Special::DoubleBestMine | Special::DoublesTacticBonusOfOneArmy | Special::ExtraHappyPerHappySource(_) | Special::FinalScoring(_) | Special::FreeCivilAction(_) | Special::FreePopIncreasePerTurn | Special::Gain(_) | Special::GainCulturePerLevelOfRemovedCard(_) | Special::GainFoodOrResources(_) | Special::GainResources(_) | Special::InfantryCountsAsCavalryForTactics | Special::LastRoundSubstitute(_) | Special::LeaderTakeCivilActionDiscount(_) | Special::LibraryDiscountsIfTheater | Special::Lose(_) | Special::MilitaryActionAsCivilPerTurn(_) | Special::MilitaryActionCombinedPopIncreaseAndUnitBuild | Special::NoAttacksBetweenParties | Special::OnAttackBetweenParties(_) | Special::OnBuildCulture(_) | Special::OnBuildCulturePerTechLevelSum | Special::OnReplacePutUnderCompletedWonderHappy(_) | Special::OncePerGameTwoPoliticalActions | Special::OpponentDecreasesPopulation(_) | Special::OpponentsPayDoubleMilitaryActionsToAttackYou | Special::OrTakesSpecialTechnologiesOfSameTotalScienceCost | Special::PeekTopEventCardInPolitics | Special::PerTurnChoice | Special::PlayerWithLeastCulture(_) | Special::PlayerWithMostCulture(_) | Special::PlayersWithMostDiscontentWorkers(_) | Special::PlayersWithMostHappyFaces(_) | Special::PopIncreaseFoodDiscount(_) | Special::RemoveAsPoliticalActionForYellowToken(_) | Special::RemoveAsPoliticalActionFreeColonize | Special::RemoveFromGame | Special::ResourceOnMilitaryUnitBuildOrUpgrade(_) | Special::ResourceOnTechDevelop(_) | Special::ResourcesForMilitaryUnitsPerStrongerCivilization(_) | Special::ResourcesPerLabEqualToLevel | Special::RevolutionUsesMilitaryActionsInstead | Special::ScienceOnTechCardTake(_) | Special::SciencePerBestLabOrLibraryLevel | Special::SciencePerLab(_) | Special::StealColony(_) | Special::StrengthPerArtillery(_) | Special::StrengthPerInfantry(_) | Special::StrengthPerMilitaryUnit(_) | Special::StrengthPerTempleOrGovernmentHappy(_) | Special::StrengthPerUnitType(_) | Special::StrongestPlayers(_) | Special::TakeCivilActionDiscountIfLeaderReplacedThisTurn(_) | Special::TakeFromOpponent(_) | Special::TheaterResourceDiscountIfLibrary(_) | Special::TheaterScienceDiscountIfLibrary(_) | Special::TheaterTechScienceDiscount(_) | Special::VictorTakesCulture | Special::VictorTakesScienceUpTo(_) | Special::VictorTakesYellowTokens | Special::WeakestPlayer(_) | Special::WeakestPlayers(_) | Special::WonderTakeNoExtraCivilActions => None,
        }),
    );
    apply_single_target(
        state,
        &order,
        RankStat::Strength,
        false,
        false, // a penalty for the weakest -- favoring the current player means picking them LAST (see apply_single_target's doc)
        event_block(card, |sp| match sp {
            Special::WeakestPlayer(b) => Some(b),
            Special::A(_) | Special::AllPlayers(_) | Special::B(_) | Special::BestTheaterDoubleCulture | Special::BothPlayers(_) | Special::BuildDiscount(_) | Special::CancelledIfPartiesAttackEachOther | Special::CannotPlayAggressionOrWar | Special::CivilActionBackOnTechDevelop(_) | Special::CivilActionUpgradeUrbanBuildingToTheater | Special::ColonizeDiscardUpTo2MilitaryCardsForBonus(_) | Special::ColonyImmediateBonusApplies | Special::ColonyPermanentBonusTransfers | Special::ComboFoodDiscount(_) | Special::ComboResourceDiscount(_) | Special::Condition(_) | Special::CultureFirstColony(_) | Special::CultureIfTopTwoStrength(_) | Special::CultureOnLeaveEqualToLabResourceProduction | Special::CultureOnRevolution(_) | Special::CultureOnTechDevelop(_) | Special::CulturePerAdditionalColony(_) | Special::CulturePerCivilizationWithMoreCulture(_) | Special::CulturePerHappyFromTemplesTheatersWonders(_) | Special::CulturePerLabEqualToLevel | Special::CulturePerLibraryTheaterPair(_) | Special::CulturePerTheater(_) | Special::DecreasePopulation(_) | Special::DestroyUrbanBuildings(_) | Special::DoubleBestMine | Special::DoublesTacticBonusOfOneArmy | Special::ExtraHappyPerHappySource(_) | Special::FinalScoring(_) | Special::FreeCivilAction(_) | Special::FreePopIncreasePerTurn | Special::Gain(_) | Special::GainCulturePerLevelOfRemovedCard(_) | Special::GainFoodOrResources(_) | Special::GainResources(_) | Special::InfantryCountsAsCavalryForTactics | Special::LastRoundSubstitute(_) | Special::LeaderTakeCivilActionDiscount(_) | Special::LibraryDiscountsIfTheater | Special::Lose(_) | Special::MilitaryActionAsCivilPerTurn(_) | Special::MilitaryActionCombinedPopIncreaseAndUnitBuild | Special::NoAttacksBetweenParties | Special::OnAttackBetweenParties(_) | Special::OnBuildCulture(_) | Special::OnBuildCulturePerTechLevelSum | Special::OnReplacePutUnderCompletedWonderHappy(_) | Special::OncePerGameTwoPoliticalActions | Special::OpponentDecreasesPopulation(_) | Special::OpponentsPayDoubleMilitaryActionsToAttackYou | Special::OrTakesSpecialTechnologiesOfSameTotalScienceCost | Special::PeekTopEventCardInPolitics | Special::PerTurnChoice | Special::PlayerWithLeastCulture(_) | Special::PlayerWithMostCulture(_) | Special::PlayersWithMostDiscontentWorkers(_) | Special::PlayersWithMostHappyFaces(_) | Special::PopIncreaseFoodDiscount(_) | Special::RemoveAsPoliticalActionForYellowToken(_) | Special::RemoveAsPoliticalActionFreeColonize | Special::RemoveFromGame | Special::ResourceOnMilitaryUnitBuildOrUpgrade(_) | Special::ResourceOnTechDevelop(_) | Special::ResourcesForMilitaryUnitsPerStrongerCivilization(_) | Special::ResourcesPerLabEqualToLevel | Special::RevolutionUsesMilitaryActionsInstead | Special::ScienceOnTechCardTake(_) | Special::SciencePerBestLabOrLibraryLevel | Special::SciencePerLab(_) | Special::StealColony(_) | Special::StrengthPerArtillery(_) | Special::StrengthPerInfantry(_) | Special::StrengthPerMilitaryUnit(_) | Special::StrengthPerTempleOrGovernmentHappy(_) | Special::StrengthPerUnitType(_) | Special::StrongestPlayer(_) | Special::StrongestPlayers(_) | Special::TakeCivilActionDiscountIfLeaderReplacedThisTurn(_) | Special::TakeFromOpponent(_) | Special::TheaterResourceDiscountIfLibrary(_) | Special::TheaterScienceDiscountIfLibrary(_) | Special::TheaterTechScienceDiscount(_) | Special::VictorTakesCulture | Special::VictorTakesScienceUpTo(_) | Special::VictorTakesYellowTokens | Special::WeakestPlayers(_) | Special::WonderTakeNoExtraCivilActions => None,
        }),
    );
    apply_single_target(
        state,
        &order,
        RankStat::Culture,
        true,
        true, // National Pride (the only base-game card): a bonus for the most-cultured, favor current-first
        event_block(card, |sp| match sp {
            Special::PlayerWithMostCulture(b) => Some(b),
            Special::A(_) | Special::AllPlayers(_) | Special::B(_) | Special::BestTheaterDoubleCulture | Special::BothPlayers(_) | Special::BuildDiscount(_) | Special::CancelledIfPartiesAttackEachOther | Special::CannotPlayAggressionOrWar | Special::CivilActionBackOnTechDevelop(_) | Special::CivilActionUpgradeUrbanBuildingToTheater | Special::ColonizeDiscardUpTo2MilitaryCardsForBonus(_) | Special::ColonyImmediateBonusApplies | Special::ColonyPermanentBonusTransfers | Special::ComboFoodDiscount(_) | Special::ComboResourceDiscount(_) | Special::Condition(_) | Special::CultureFirstColony(_) | Special::CultureIfTopTwoStrength(_) | Special::CultureOnLeaveEqualToLabResourceProduction | Special::CultureOnRevolution(_) | Special::CultureOnTechDevelop(_) | Special::CulturePerAdditionalColony(_) | Special::CulturePerCivilizationWithMoreCulture(_) | Special::CulturePerHappyFromTemplesTheatersWonders(_) | Special::CulturePerLabEqualToLevel | Special::CulturePerLibraryTheaterPair(_) | Special::CulturePerTheater(_) | Special::DecreasePopulation(_) | Special::DestroyUrbanBuildings(_) | Special::DoubleBestMine | Special::DoublesTacticBonusOfOneArmy | Special::ExtraHappyPerHappySource(_) | Special::FinalScoring(_) | Special::FreeCivilAction(_) | Special::FreePopIncreasePerTurn | Special::Gain(_) | Special::GainCulturePerLevelOfRemovedCard(_) | Special::GainFoodOrResources(_) | Special::GainResources(_) | Special::InfantryCountsAsCavalryForTactics | Special::LastRoundSubstitute(_) | Special::LeaderTakeCivilActionDiscount(_) | Special::LibraryDiscountsIfTheater | Special::Lose(_) | Special::MilitaryActionAsCivilPerTurn(_) | Special::MilitaryActionCombinedPopIncreaseAndUnitBuild | Special::NoAttacksBetweenParties | Special::OnAttackBetweenParties(_) | Special::OnBuildCulture(_) | Special::OnBuildCulturePerTechLevelSum | Special::OnReplacePutUnderCompletedWonderHappy(_) | Special::OncePerGameTwoPoliticalActions | Special::OpponentDecreasesPopulation(_) | Special::OpponentsPayDoubleMilitaryActionsToAttackYou | Special::OrTakesSpecialTechnologiesOfSameTotalScienceCost | Special::PeekTopEventCardInPolitics | Special::PerTurnChoice | Special::PlayerWithLeastCulture(_) | Special::PlayersWithMostDiscontentWorkers(_) | Special::PlayersWithMostHappyFaces(_) | Special::PopIncreaseFoodDiscount(_) | Special::RemoveAsPoliticalActionForYellowToken(_) | Special::RemoveAsPoliticalActionFreeColonize | Special::RemoveFromGame | Special::ResourceOnMilitaryUnitBuildOrUpgrade(_) | Special::ResourceOnTechDevelop(_) | Special::ResourcesForMilitaryUnitsPerStrongerCivilization(_) | Special::ResourcesPerLabEqualToLevel | Special::RevolutionUsesMilitaryActionsInstead | Special::ScienceOnTechCardTake(_) | Special::SciencePerBestLabOrLibraryLevel | Special::SciencePerLab(_) | Special::StealColony(_) | Special::StrengthPerArtillery(_) | Special::StrengthPerInfantry(_) | Special::StrengthPerMilitaryUnit(_) | Special::StrengthPerTempleOrGovernmentHappy(_) | Special::StrengthPerUnitType(_) | Special::StrongestPlayer(_) | Special::StrongestPlayers(_) | Special::TakeCivilActionDiscountIfLeaderReplacedThisTurn(_) | Special::TakeFromOpponent(_) | Special::TheaterResourceDiscountIfLibrary(_) | Special::TheaterScienceDiscountIfLibrary(_) | Special::TheaterTechScienceDiscount(_) | Special::VictorTakesCulture | Special::VictorTakesScienceUpTo(_) | Special::VictorTakesYellowTokens | Special::WeakestPlayer(_) | Special::WeakestPlayers(_) | Special::WonderTakeNoExtraCivilActions => None,
        }),
    );
    apply_single_target(
        state,
        &order,
        RankStat::Culture,
        false,
        true, // Terrorism (the only base-game card): "destroys one urban building of each OPPONENT" is a bonus for the least-cultured target, favor current-first
        event_block(card, |sp| match sp {
            Special::PlayerWithLeastCulture(b) => Some(b),
            Special::A(_) | Special::AllPlayers(_) | Special::B(_) | Special::BestTheaterDoubleCulture | Special::BothPlayers(_) | Special::BuildDiscount(_) | Special::CancelledIfPartiesAttackEachOther | Special::CannotPlayAggressionOrWar | Special::CivilActionBackOnTechDevelop(_) | Special::CivilActionUpgradeUrbanBuildingToTheater | Special::ColonizeDiscardUpTo2MilitaryCardsForBonus(_) | Special::ColonyImmediateBonusApplies | Special::ColonyPermanentBonusTransfers | Special::ComboFoodDiscount(_) | Special::ComboResourceDiscount(_) | Special::Condition(_) | Special::CultureFirstColony(_) | Special::CultureIfTopTwoStrength(_) | Special::CultureOnLeaveEqualToLabResourceProduction | Special::CultureOnRevolution(_) | Special::CultureOnTechDevelop(_) | Special::CulturePerAdditionalColony(_) | Special::CulturePerCivilizationWithMoreCulture(_) | Special::CulturePerHappyFromTemplesTheatersWonders(_) | Special::CulturePerLabEqualToLevel | Special::CulturePerLibraryTheaterPair(_) | Special::CulturePerTheater(_) | Special::DecreasePopulation(_) | Special::DestroyUrbanBuildings(_) | Special::DoubleBestMine | Special::DoublesTacticBonusOfOneArmy | Special::ExtraHappyPerHappySource(_) | Special::FinalScoring(_) | Special::FreeCivilAction(_) | Special::FreePopIncreasePerTurn | Special::Gain(_) | Special::GainCulturePerLevelOfRemovedCard(_) | Special::GainFoodOrResources(_) | Special::GainResources(_) | Special::InfantryCountsAsCavalryForTactics | Special::LastRoundSubstitute(_) | Special::LeaderTakeCivilActionDiscount(_) | Special::LibraryDiscountsIfTheater | Special::Lose(_) | Special::MilitaryActionAsCivilPerTurn(_) | Special::MilitaryActionCombinedPopIncreaseAndUnitBuild | Special::NoAttacksBetweenParties | Special::OnAttackBetweenParties(_) | Special::OnBuildCulture(_) | Special::OnBuildCulturePerTechLevelSum | Special::OnReplacePutUnderCompletedWonderHappy(_) | Special::OncePerGameTwoPoliticalActions | Special::OpponentDecreasesPopulation(_) | Special::OpponentsPayDoubleMilitaryActionsToAttackYou | Special::OrTakesSpecialTechnologiesOfSameTotalScienceCost | Special::PeekTopEventCardInPolitics | Special::PerTurnChoice | Special::PlayerWithMostCulture(_) | Special::PlayersWithMostDiscontentWorkers(_) | Special::PlayersWithMostHappyFaces(_) | Special::PopIncreaseFoodDiscount(_) | Special::RemoveAsPoliticalActionForYellowToken(_) | Special::RemoveAsPoliticalActionFreeColonize | Special::RemoveFromGame | Special::ResourceOnMilitaryUnitBuildOrUpgrade(_) | Special::ResourceOnTechDevelop(_) | Special::ResourcesForMilitaryUnitsPerStrongerCivilization(_) | Special::ResourcesPerLabEqualToLevel | Special::RevolutionUsesMilitaryActionsInstead | Special::ScienceOnTechCardTake(_) | Special::SciencePerBestLabOrLibraryLevel | Special::SciencePerLab(_) | Special::StealColony(_) | Special::StrengthPerArtillery(_) | Special::StrengthPerInfantry(_) | Special::StrengthPerMilitaryUnit(_) | Special::StrengthPerTempleOrGovernmentHappy(_) | Special::StrengthPerUnitType(_) | Special::StrongestPlayer(_) | Special::StrongestPlayers(_) | Special::TakeCivilActionDiscountIfLeaderReplacedThisTurn(_) | Special::TakeFromOpponent(_) | Special::TheaterResourceDiscountIfLibrary(_) | Special::TheaterScienceDiscountIfLibrary(_) | Special::TheaterTechScienceDiscount(_) | Special::VictorTakesCulture | Special::VictorTakesScienceUpTo(_) | Special::VictorTakesYellowTokens | Special::WeakestPlayer(_) | Special::WeakestPlayers(_) | Special::WonderTakeNoExtraCivilActions => None,
        }),
    );
    apply_tied_targets(
        state,
        &order,
        RankStat::Happy,
        false, // §5.3: "all tied civs affected, no tie-break" -- a 0-happy tie still counts
        event_block(card, |sp| match sp {
            Special::PlayersWithMostHappyFaces(b) => Some(b),
            Special::A(_) | Special::AllPlayers(_) | Special::B(_) | Special::BestTheaterDoubleCulture | Special::BothPlayers(_) | Special::BuildDiscount(_) | Special::CancelledIfPartiesAttackEachOther | Special::CannotPlayAggressionOrWar | Special::CivilActionBackOnTechDevelop(_) | Special::CivilActionUpgradeUrbanBuildingToTheater | Special::ColonizeDiscardUpTo2MilitaryCardsForBonus(_) | Special::ColonyImmediateBonusApplies | Special::ColonyPermanentBonusTransfers | Special::ComboFoodDiscount(_) | Special::ComboResourceDiscount(_) | Special::Condition(_) | Special::CultureFirstColony(_) | Special::CultureIfTopTwoStrength(_) | Special::CultureOnLeaveEqualToLabResourceProduction | Special::CultureOnRevolution(_) | Special::CultureOnTechDevelop(_) | Special::CulturePerAdditionalColony(_) | Special::CulturePerCivilizationWithMoreCulture(_) | Special::CulturePerHappyFromTemplesTheatersWonders(_) | Special::CulturePerLabEqualToLevel | Special::CulturePerLibraryTheaterPair(_) | Special::CulturePerTheater(_) | Special::DecreasePopulation(_) | Special::DestroyUrbanBuildings(_) | Special::DoubleBestMine | Special::DoublesTacticBonusOfOneArmy | Special::ExtraHappyPerHappySource(_) | Special::FinalScoring(_) | Special::FreeCivilAction(_) | Special::FreePopIncreasePerTurn | Special::Gain(_) | Special::GainCulturePerLevelOfRemovedCard(_) | Special::GainFoodOrResources(_) | Special::GainResources(_) | Special::InfantryCountsAsCavalryForTactics | Special::LastRoundSubstitute(_) | Special::LeaderTakeCivilActionDiscount(_) | Special::LibraryDiscountsIfTheater | Special::Lose(_) | Special::MilitaryActionAsCivilPerTurn(_) | Special::MilitaryActionCombinedPopIncreaseAndUnitBuild | Special::NoAttacksBetweenParties | Special::OnAttackBetweenParties(_) | Special::OnBuildCulture(_) | Special::OnBuildCulturePerTechLevelSum | Special::OnReplacePutUnderCompletedWonderHappy(_) | Special::OncePerGameTwoPoliticalActions | Special::OpponentDecreasesPopulation(_) | Special::OpponentsPayDoubleMilitaryActionsToAttackYou | Special::OrTakesSpecialTechnologiesOfSameTotalScienceCost | Special::PeekTopEventCardInPolitics | Special::PerTurnChoice | Special::PlayerWithLeastCulture(_) | Special::PlayerWithMostCulture(_) | Special::PlayersWithMostDiscontentWorkers(_) | Special::PopIncreaseFoodDiscount(_) | Special::RemoveAsPoliticalActionForYellowToken(_) | Special::RemoveAsPoliticalActionFreeColonize | Special::RemoveFromGame | Special::ResourceOnMilitaryUnitBuildOrUpgrade(_) | Special::ResourceOnTechDevelop(_) | Special::ResourcesForMilitaryUnitsPerStrongerCivilization(_) | Special::ResourcesPerLabEqualToLevel | Special::RevolutionUsesMilitaryActionsInstead | Special::ScienceOnTechCardTake(_) | Special::SciencePerBestLabOrLibraryLevel | Special::SciencePerLab(_) | Special::StealColony(_) | Special::StrengthPerArtillery(_) | Special::StrengthPerInfantry(_) | Special::StrengthPerMilitaryUnit(_) | Special::StrengthPerTempleOrGovernmentHappy(_) | Special::StrengthPerUnitType(_) | Special::StrongestPlayer(_) | Special::StrongestPlayers(_) | Special::TakeCivilActionDiscountIfLeaderReplacedThisTurn(_) | Special::TakeFromOpponent(_) | Special::TheaterResourceDiscountIfLibrary(_) | Special::TheaterScienceDiscountIfLibrary(_) | Special::TheaterTechScienceDiscount(_) | Special::VictorTakesCulture | Special::VictorTakesScienceUpTo(_) | Special::VictorTakesYellowTokens | Special::WeakestPlayer(_) | Special::WeakestPlayers(_) | Special::WonderTakeNoExtraCivilActions => None,
        }),
    );
    apply_tied_targets(
        state,
        &order,
        RankStat::Discontent,
        true, // 0 discontent workers means nobody genuinely "has" one
        event_block(card, |sp| match sp {
            Special::PlayersWithMostDiscontentWorkers(b) => Some(b),
            Special::A(_) | Special::AllPlayers(_) | Special::B(_) | Special::BestTheaterDoubleCulture | Special::BothPlayers(_) | Special::BuildDiscount(_) | Special::CancelledIfPartiesAttackEachOther | Special::CannotPlayAggressionOrWar | Special::CivilActionBackOnTechDevelop(_) | Special::CivilActionUpgradeUrbanBuildingToTheater | Special::ColonizeDiscardUpTo2MilitaryCardsForBonus(_) | Special::ColonyImmediateBonusApplies | Special::ColonyPermanentBonusTransfers | Special::ComboFoodDiscount(_) | Special::ComboResourceDiscount(_) | Special::Condition(_) | Special::CultureFirstColony(_) | Special::CultureIfTopTwoStrength(_) | Special::CultureOnLeaveEqualToLabResourceProduction | Special::CultureOnRevolution(_) | Special::CultureOnTechDevelop(_) | Special::CulturePerAdditionalColony(_) | Special::CulturePerCivilizationWithMoreCulture(_) | Special::CulturePerHappyFromTemplesTheatersWonders(_) | Special::CulturePerLabEqualToLevel | Special::CulturePerLibraryTheaterPair(_) | Special::CulturePerTheater(_) | Special::DecreasePopulation(_) | Special::DestroyUrbanBuildings(_) | Special::DoubleBestMine | Special::DoublesTacticBonusOfOneArmy | Special::ExtraHappyPerHappySource(_) | Special::FinalScoring(_) | Special::FreeCivilAction(_) | Special::FreePopIncreasePerTurn | Special::Gain(_) | Special::GainCulturePerLevelOfRemovedCard(_) | Special::GainFoodOrResources(_) | Special::GainResources(_) | Special::InfantryCountsAsCavalryForTactics | Special::LastRoundSubstitute(_) | Special::LeaderTakeCivilActionDiscount(_) | Special::LibraryDiscountsIfTheater | Special::Lose(_) | Special::MilitaryActionAsCivilPerTurn(_) | Special::MilitaryActionCombinedPopIncreaseAndUnitBuild | Special::NoAttacksBetweenParties | Special::OnAttackBetweenParties(_) | Special::OnBuildCulture(_) | Special::OnBuildCulturePerTechLevelSum | Special::OnReplacePutUnderCompletedWonderHappy(_) | Special::OncePerGameTwoPoliticalActions | Special::OpponentDecreasesPopulation(_) | Special::OpponentsPayDoubleMilitaryActionsToAttackYou | Special::OrTakesSpecialTechnologiesOfSameTotalScienceCost | Special::PeekTopEventCardInPolitics | Special::PerTurnChoice | Special::PlayerWithLeastCulture(_) | Special::PlayerWithMostCulture(_) | Special::PlayersWithMostHappyFaces(_) | Special::PopIncreaseFoodDiscount(_) | Special::RemoveAsPoliticalActionForYellowToken(_) | Special::RemoveAsPoliticalActionFreeColonize | Special::RemoveFromGame | Special::ResourceOnMilitaryUnitBuildOrUpgrade(_) | Special::ResourceOnTechDevelop(_) | Special::ResourcesForMilitaryUnitsPerStrongerCivilization(_) | Special::ResourcesPerLabEqualToLevel | Special::RevolutionUsesMilitaryActionsInstead | Special::ScienceOnTechCardTake(_) | Special::SciencePerBestLabOrLibraryLevel | Special::SciencePerLab(_) | Special::StealColony(_) | Special::StrengthPerArtillery(_) | Special::StrengthPerInfantry(_) | Special::StrengthPerMilitaryUnit(_) | Special::StrengthPerTempleOrGovernmentHappy(_) | Special::StrengthPerUnitType(_) | Special::StrongestPlayer(_) | Special::StrongestPlayers(_) | Special::TakeCivilActionDiscountIfLeaderReplacedThisTurn(_) | Special::TakeFromOpponent(_) | Special::TheaterResourceDiscountIfLibrary(_) | Special::TheaterScienceDiscountIfLibrary(_) | Special::TheaterTechScienceDiscount(_) | Special::VictorTakesCulture | Special::VictorTakesScienceUpTo(_) | Special::VictorTakesYellowTokens | Special::WeakestPlayer(_) | Special::WeakestPlayers(_) | Special::WonderTakeNoExtraCivilActions => None,
        }),
    );

    resolve_count_targets(state, &order, card);
}

/// The `EventBlock` payload for ONE `Special` variant on `card`, or
/// [`EventBlock::EMPTY`] if `card` does not carry that variant. `matcher`
/// picks the one variant each call site wants (a non-capturing closure,
/// e.g. `|sp| match sp { Special::AllPlayers(b) => Some(b), _ => None }`).
fn event_block(card: CardId, matcher: impl Fn(Special) -> Option<EventBlock>) -> EventBlock {
    card.get().special.iter().copied().find_map(matcher).unwrap_or(EventBlock::EMPTY)
}

/// The `[i16; 3]` (2p/3p/4p) payload for ONE `Special` variant on `card`, or
/// all-zero if absent. The `Condition`/`StrongestPlayers`/`WeakestPlayers`
/// twin of [`event_block`]. `pub(crate)`: `apply.rs::h_play_action` reuses it
/// for the two per-player-count action-card magnitudes
/// (`CulturePerCivilizationWithMoreCulture`/
/// `ResourcesForMilitaryUnitsPerStrongerCivilization`) rather than
/// reimplementing the same `[i16; 3]` lookup a second time.
pub(crate) fn count_table(card: CardId, matcher: impl Fn(Special) -> Option<[i16; 3]>) -> [i16; 3] {
    card.get().special.iter().copied().find_map(matcher).unwrap_or([0, 0, 0])
}

fn last_round_substitute(card: CardId) -> Option<LastRoundSubstituteBlock> {
    card.get().special.iter().find_map(|&sp| match sp {
        Special::LastRoundSubstitute(b) => Some(b),
        Special::A(_) | Special::AllPlayers(_) | Special::B(_) | Special::BestTheaterDoubleCulture | Special::BothPlayers(_) | Special::BuildDiscount(_) | Special::CancelledIfPartiesAttackEachOther | Special::CannotPlayAggressionOrWar | Special::CivilActionBackOnTechDevelop(_) | Special::CivilActionUpgradeUrbanBuildingToTheater | Special::ColonizeDiscardUpTo2MilitaryCardsForBonus(_) | Special::ColonyImmediateBonusApplies | Special::ColonyPermanentBonusTransfers | Special::ComboFoodDiscount(_) | Special::ComboResourceDiscount(_) | Special::Condition(_) | Special::CultureFirstColony(_) | Special::CultureIfTopTwoStrength(_) | Special::CultureOnLeaveEqualToLabResourceProduction | Special::CultureOnRevolution(_) | Special::CultureOnTechDevelop(_) | Special::CulturePerAdditionalColony(_) | Special::CulturePerCivilizationWithMoreCulture(_) | Special::CulturePerHappyFromTemplesTheatersWonders(_) | Special::CulturePerLabEqualToLevel | Special::CulturePerLibraryTheaterPair(_) | Special::CulturePerTheater(_) | Special::DecreasePopulation(_) | Special::DestroyUrbanBuildings(_) | Special::DoubleBestMine | Special::DoublesTacticBonusOfOneArmy | Special::ExtraHappyPerHappySource(_) | Special::FinalScoring(_) | Special::FreeCivilAction(_) | Special::FreePopIncreasePerTurn | Special::Gain(_) | Special::GainCulturePerLevelOfRemovedCard(_) | Special::GainFoodOrResources(_) | Special::GainResources(_) | Special::InfantryCountsAsCavalryForTactics | Special::LeaderTakeCivilActionDiscount(_) | Special::LibraryDiscountsIfTheater | Special::Lose(_) | Special::MilitaryActionAsCivilPerTurn(_) | Special::MilitaryActionCombinedPopIncreaseAndUnitBuild | Special::NoAttacksBetweenParties | Special::OnAttackBetweenParties(_) | Special::OnBuildCulture(_) | Special::OnBuildCulturePerTechLevelSum | Special::OnReplacePutUnderCompletedWonderHappy(_) | Special::OncePerGameTwoPoliticalActions | Special::OpponentDecreasesPopulation(_) | Special::OpponentsPayDoubleMilitaryActionsToAttackYou | Special::OrTakesSpecialTechnologiesOfSameTotalScienceCost | Special::PeekTopEventCardInPolitics | Special::PerTurnChoice | Special::PlayerWithLeastCulture(_) | Special::PlayerWithMostCulture(_) | Special::PlayersWithMostDiscontentWorkers(_) | Special::PlayersWithMostHappyFaces(_) | Special::PopIncreaseFoodDiscount(_) | Special::RemoveAsPoliticalActionForYellowToken(_) | Special::RemoveAsPoliticalActionFreeColonize | Special::RemoveFromGame | Special::ResourceOnMilitaryUnitBuildOrUpgrade(_) | Special::ResourceOnTechDevelop(_) | Special::ResourcesForMilitaryUnitsPerStrongerCivilization(_) | Special::ResourcesPerLabEqualToLevel | Special::RevolutionUsesMilitaryActionsInstead | Special::ScienceOnTechCardTake(_) | Special::SciencePerBestLabOrLibraryLevel | Special::SciencePerLab(_) | Special::StealColony(_) | Special::StrengthPerArtillery(_) | Special::StrengthPerInfantry(_) | Special::StrengthPerMilitaryUnit(_) | Special::StrengthPerTempleOrGovernmentHappy(_) | Special::StrengthPerUnitType(_) | Special::StrongestPlayer(_) | Special::StrongestPlayers(_) | Special::TakeCivilActionDiscountIfLeaderReplacedThisTurn(_) | Special::TakeFromOpponent(_) | Special::TheaterResourceDiscountIfLibrary(_) | Special::TheaterScienceDiscountIfLibrary(_) | Special::TheaterTechScienceDiscount(_) | Special::VictorTakesCulture | Special::VictorTakesScienceUpTo(_) | Special::VictorTakesYellowTokens | Special::WeakestPlayer(_) | Special::WeakestPlayers(_) | Special::WonderTakeNoExtraCivilActions => None,
    })
}

fn decrease_population_of(card: CardId) -> Option<i16> {
    card.get().special.iter().find_map(|&sp| match sp {
        Special::DecreasePopulation(n) => Some(n),
        Special::A(_) | Special::AllPlayers(_) | Special::B(_) | Special::BestTheaterDoubleCulture | Special::BothPlayers(_) | Special::BuildDiscount(_) | Special::CancelledIfPartiesAttackEachOther | Special::CannotPlayAggressionOrWar | Special::CivilActionBackOnTechDevelop(_) | Special::CivilActionUpgradeUrbanBuildingToTheater | Special::ColonizeDiscardUpTo2MilitaryCardsForBonus(_) | Special::ColonyImmediateBonusApplies | Special::ColonyPermanentBonusTransfers | Special::ComboFoodDiscount(_) | Special::ComboResourceDiscount(_) | Special::Condition(_) | Special::CultureFirstColony(_) | Special::CultureIfTopTwoStrength(_) | Special::CultureOnLeaveEqualToLabResourceProduction | Special::CultureOnRevolution(_) | Special::CultureOnTechDevelop(_) | Special::CulturePerAdditionalColony(_) | Special::CulturePerCivilizationWithMoreCulture(_) | Special::CulturePerHappyFromTemplesTheatersWonders(_) | Special::CulturePerLabEqualToLevel | Special::CulturePerLibraryTheaterPair(_) | Special::CulturePerTheater(_) | Special::DestroyUrbanBuildings(_) | Special::DoubleBestMine | Special::DoublesTacticBonusOfOneArmy | Special::ExtraHappyPerHappySource(_) | Special::FinalScoring(_) | Special::FreeCivilAction(_) | Special::FreePopIncreasePerTurn | Special::Gain(_) | Special::GainCulturePerLevelOfRemovedCard(_) | Special::GainFoodOrResources(_) | Special::GainResources(_) | Special::InfantryCountsAsCavalryForTactics | Special::LastRoundSubstitute(_) | Special::LeaderTakeCivilActionDiscount(_) | Special::LibraryDiscountsIfTheater | Special::Lose(_) | Special::MilitaryActionAsCivilPerTurn(_) | Special::MilitaryActionCombinedPopIncreaseAndUnitBuild | Special::NoAttacksBetweenParties | Special::OnAttackBetweenParties(_) | Special::OnBuildCulture(_) | Special::OnBuildCulturePerTechLevelSum | Special::OnReplacePutUnderCompletedWonderHappy(_) | Special::OncePerGameTwoPoliticalActions | Special::OpponentDecreasesPopulation(_) | Special::OpponentsPayDoubleMilitaryActionsToAttackYou | Special::OrTakesSpecialTechnologiesOfSameTotalScienceCost | Special::PeekTopEventCardInPolitics | Special::PerTurnChoice | Special::PlayerWithLeastCulture(_) | Special::PlayerWithMostCulture(_) | Special::PlayersWithMostDiscontentWorkers(_) | Special::PlayersWithMostHappyFaces(_) | Special::PopIncreaseFoodDiscount(_) | Special::RemoveAsPoliticalActionForYellowToken(_) | Special::RemoveAsPoliticalActionFreeColonize | Special::RemoveFromGame | Special::ResourceOnMilitaryUnitBuildOrUpgrade(_) | Special::ResourceOnTechDevelop(_) | Special::ResourcesForMilitaryUnitsPerStrongerCivilization(_) | Special::ResourcesPerLabEqualToLevel | Special::RevolutionUsesMilitaryActionsInstead | Special::ScienceOnTechCardTake(_) | Special::SciencePerBestLabOrLibraryLevel | Special::SciencePerLab(_) | Special::StealColony(_) | Special::StrengthPerArtillery(_) | Special::StrengthPerInfantry(_) | Special::StrengthPerMilitaryUnit(_) | Special::StrengthPerTempleOrGovernmentHappy(_) | Special::StrengthPerUnitType(_) | Special::StrongestPlayer(_) | Special::StrongestPlayers(_) | Special::TakeCivilActionDiscountIfLeaderReplacedThisTurn(_) | Special::TakeFromOpponent(_) | Special::TheaterResourceDiscountIfLibrary(_) | Special::TheaterScienceDiscountIfLibrary(_) | Special::TheaterTechScienceDiscount(_) | Special::VictorTakesCulture | Special::VictorTakesScienceUpTo(_) | Special::VictorTakesYellowTokens | Special::WeakestPlayer(_) | Special::WeakestPlayers(_) | Special::WonderTakeNoExtraCivilActions => None,
    })
}

/// Index into a `[i16; 3]` (2p/3p/4p) count table for the current game.
/// `pub(crate)` for the same reason as [`count_table`] above.
pub(crate) fn live_count_idx(state: &GameState) -> usize {
    game::live_count(state).saturating_sub(2).min(2)
}

/// The four statistics §5.3's targeting keys rank players by. Mirrors the
/// four `_stat_value` targets `resolve_event`'s own six-key loop reads
/// (`strength`/`culture`/`happy`/`discontent`) -- a SEPARATE, smaller
/// vocabulary from [`FinalScoringStat`] (§12.5.2's `rankingCulture`, which
/// never targets `happy`/`discontent`/banked `culture` and instead has its
/// own `culture_rate`/`food`/`resources`/`science`), so the two are not
/// merged into one enum despite the overlap.
///
/// `pub(crate)`, same as [`rank_players`] below: `bots::weighted::events::
/// my_event_threat` needs both, to rank players by exactly this vocabulary
/// for the `_EVENT_RANKED` targeting keys Python's `my_event_threat` reads
/// -- reusing this module's `_rank`/`_stat_value` port rather than a second
/// copy of the same four-branch dispatch.
#[derive(Clone, Copy, Debug)]
pub(crate) enum RankStat {
    Strength,
    /// Banked culture (`p.culture`) -- NOT `Stats.culture`'s per-turn RATE,
    /// which `FinalScoringStat::CultureRate` names instead.
    Culture,
    Happy,
    Discontent,
}

fn rank_stat_value(state: &GameState, idx: u8, stat: RankStat) -> i32 {
    let p = &state.players[idx as usize];
    match stat {
        RankStat::Strength => effects::state_stats(state, p).strength,
        RankStat::Culture => p.culture as i32,
        RankStat::Happy => effects::state_stats(state, p).happy,
        RankStat::Discontent => economy::discontent(state, p),
    }
}

/// `order`'s players, best-`stat`-first (or worst-first), ties broken by
/// position in `order` -- the exact stable-sort-on-already-tie-broken-order
/// trick [`rank_by_final_scoring_stat`] already uses, generalized to a
/// direction flag since `resolve_event`'s six-key loop needs BOTH
/// directions, unlike final scoring's always-best-first ranking. Mirrors
/// `engine/events.py::_rank(state, order, stat, best_first)`.
pub(crate) fn rank_players(state: &GameState, order: &[u8], stat: RankStat, best_first: bool) -> Vec<u8> {
    let mut ranked = order.to_vec();
    if best_first {
        ranked.sort_by_key(|&idx| -rank_stat_value(state, idx, stat));
    } else {
        ranked.sort_by_key(|&idx| rank_stat_value(state, idx, stat));
    }
    ranked
}

/// Reverses `order` for a targeting selection where landing IN the selected
/// set is BAD for the target (`WeakestPlayer`/`WeakestPlayers`, and
/// Barbarians' own weakest-cutoff group in [`conditional_target`]): §5.3's
/// tie-break ("ties broken in favor of the current player") means
/// PROTECTING the current player from a penalty, not handing them the
/// "weakest" label first. `order` is already clockwise starting from the
/// current player ([`order_from`]), and [`rank_players`]'s sort is stable,
/// so reversing first makes the current player sort LAST among any tie --
/// least likely to fall inside a "weakest" cutoff. This is the exact
/// reversal [`apply_single_target`] already does for its own `favor_current
/// = false` case (`WeakestPlayer`); factored out so the same fix applies to
/// every "weakest" selection, not just the singular one.
fn protect_current_from_bad_tie(order: &[u8]) -> Vec<u8> {
    order.iter().rev().copied().collect()
}

/// `TIE_CENSUS` (see `debugflags::tie_census`'s own doc): one row per
/// superlative-target selection, naming every RAW input (every live seat's
/// `stat` value, in `order` -- the UNMODIFIED clockwise-from-revealer order,
/// never whatever protected/reversed order a caller actually ranked with)
/// plus the engine's own picked seat(s). Deliberately dumps inputs, not a
/// precomputed verdict: a separate script cross-references these rows
/// against the BGO journal's own named outcome and can replay ANY candidate
/// tie-break rule (seating direction, current-player-exclusion, "nobody
/// selected on a cutoff tie", ...) without a recompile. `kind` is a free-form
/// label distinguishing which of `resolve_event`'s targeting keys this row
/// came from (the card name, from `tie_context::card_name`, is printed
/// alongside and is usually enough to disambiguate on its own, since almost
/// every key in the base game is used by exactly one or two named cards --
/// see `apply_single_target`'s own doc comment).
fn tie_census_row(state: &GameState, kind: &str, order: &[u8], stat: RankStat, selected: &[u8]) {
    if !crate::debugflags::tie_census() {
        return;
    }
    let seat_label = |q: u8| crate::corpus::Color::from_seat(q).map_or("?", crate::corpus::Color::as_str);
    let colors: Vec<&str> = order.iter().map(|&q| seat_label(q)).collect();
    let values: Vec<i32> = order.iter().map(|&q| rank_stat_value(state, q, stat)).collect();
    let selected_colors: Vec<&str> = selected.iter().map(|&q| seat_label(q)).collect();
    eprintln!(
        "TIE_ROW game={} line={} card={} kind={kind} stat={stat:?} order={colors:?} values={values:?} selected={selected_colors:?}",
        crate::tie_context::game_id(),
        crate::tie_context::lineno(),
        crate::tie_context::card_name(),
    );
}

/// One of `resolve_event`'s four SINGULAR targeting keys (`strongestPlayer`/
/// `weakestPlayer`/`playerWithMostCulture`/`playerWithLeastCulture`, plus
/// the last-round substitute's own two): the single best-or-worst-`stat`
/// player in `order` gets `block`. `order` is never empty by the time this
/// is called (`resolve_event` returns early otherwise), so there is always
/// exactly one target.
///
/// `favor_current` is the direction half of §5.3's tie-break ("ties broken
/// in favor of the current player, then proximity in clockwise order after
/// the current player" -- `docs/RULES_SPEC.md`). `order` (`order_from`,
/// `resolve_event`'s own doc) is ALREADY clockwise starting from the
/// revealer/current player, and `rank_players`'s sort is stable, so being
/// "favored" by going FIRST among ties is exactly `order` as given -- correct
/// for an effect that's good for its target (`true`: `StrongestPlayer`,
/// `PlayerWithMostCulture`, `PlayerWithLeastCulture` -- every base-game card
/// using the last two is a bonus for its target, Terrorism included: "the
/// player with least culture destroys one urban building of EACH opponent").
/// Being favored by a BAD effect (`false`: `WeakestPlayer` -- every base-game
/// card is a penalty) means the opposite: picked LAST among ties, i.e.
/// protected as long as anyone else ties with them -- reversing `order`
/// first achieves exactly that (`rank_players`'s stable sort then keeps ties
/// in the REVERSED relative order, so the tied player who was originally
/// LAST/farthest-clockwise-from-current now sorts first).
///
/// ENGINE BUG, found chasing the `IllegalMove: Pop` bucket and confirmed by
/// measurement, not reasoning, against the corpus (`docs/REPLAY.md`): every
/// `WeakestPlayer` call previously used the SAME `true`-shaped (un-reversed)
/// order `StrongestPlayer` correctly uses. Of 63 genuine strength ties on a
/// `WeakestPlayer` card across the 1,011-game corpus, the un-reversed pick
/// matched the journal's real target exactly ONCE; the reversed pick (what
/// this parameter now does for `false`) matched 62 of 63. A real human
/// playing a well-timed-current-turn "prepare an event" was therefore
/// wrongly, repeatedly targeted by a penalty tie the rules protect them
/// from -- and, since `resolve_event` runs identically in real self-play,
/// not just in this replayer, every bot game paid the same wrong penalty.
fn apply_single_target(
    state: &mut GameState,
    order: &[u8],
    stat: RankStat,
    best: bool,
    favor_current: bool,
    block: EventBlock,
) {
    let original_order = order;
    let reversed: Vec<u8>;
    let order = if favor_current {
        order
    } else {
        reversed = protect_current_from_bad_tie(order);
        &reversed
    };
    let ranked = rank_players(state, order, stat, best);
    if block != EventBlock::EMPTY {
        // Only census a selection the card actually PRINTS this key for --
        // `resolve_event`'s own doc says every card gets all four singular
        // keys' `apply_single_target` calls unconditionally (an absent key
        // reads as `EventBlock::EMPTY`, a proven no-op), so without this
        // gate every ordinary card with no `strongestPlayer`/etc. clause at
        // all would still contribute four meaningless rows -- selecting a
        // target for a block that changes nothing is not a real-world
        // "who did BGA pick" question.
        tie_census_row(
            state,
            &format!("single:{stat:?}:best={best}:favor_current={favor_current}"),
            original_order,
            stat,
            &ranked[..ranked.len().min(1)],
        );
    }
    if crate::debugflags::replay_debug_all() {
        eprintln!(
            "DEBUG apply_single_target: order={order:?} stat={stat:?} best={best} favor_current={favor_current} ranked={ranked:?} values={:?}",
            order.iter().map(|&q| rank_stat_value(state, q, stat)).collect::<Vec<_>>()
        );
    }
    if let Some(&q) = ranked.first() {
        apply_player_block(state, q, &block);
    }
}

/// The two TIED targeting keys (`playersWithMostHappyFaces`/
/// `playersWithMostDiscontentWorkers`, §5.3's `all_tied` pair): every player
/// tied for the best `stat` gets `block`.
///
/// `require_positive` gates the DISCONTENT case only -- "the players with the
/// most discontent workers" targets nobody when nobody has one (0 discontent
/// workers means nobody genuinely "has" one). It must NOT apply to the HAPPY
/// case (`RankStat::Happy`, Immigration's "all civilizations with the most
/// happy faces gain 1 population"): §5.3 states plainly, with no positivity
/// clause, "'All civilizations' with most/least: all tied civs affected, no
/// tie-break" -- a 0-happy tie is still a tie for "the most happy faces".
/// ENGINE BUG, found chasing the `IllegalMove: Build` bucket's workers_free
/// undercounts: this gate used to apply unconditionally to BOTH keys, so
/// Immigration silently granted nobody a population token whenever every
/// player sat at 0 happy faces. Game 7523355 (round 11) is a corpus-confirmed
/// instance: engine computed `values=[0, 0]` for Orange/Purple and skipped
/// the grant entirely, while the journal shows BOTH players' own lines
/// ("Orange receives a new immigrant; Purple receives a new immigrant") --
/// the real game granted it. Targets are collected BEFORE `block` is applied
/// to any of them (mirrors Python's own comment: the block can move
/// population, which moves discontent, so re-ranking mid-loop would be
/// wrong).
fn apply_tied_targets(state: &mut GameState, order: &[u8], stat: RankStat, require_positive: bool, block: EventBlock) {
    let ranked = rank_players(state, order, stat, true);
    let top = match ranked.first() {
        Some(&q) => rank_stat_value(state, q, stat),
        None => 0,
    };
    if require_positive && top <= 0 {
        return;
    }
    let targets: Vec<u8> =
        ranked.into_iter().filter(|&q| rank_stat_value(state, q, stat) == top).collect();
    for q in targets {
        apply_player_block(state, q, &block);
    }
}

/// Barbarians: "the player with most culture, if among the weakest" (§5.3).
/// Mirrors `engine/events.py::_conditional_target`.
fn conditional_target(state: &mut GameState, order: &[u8], card: CardId, n: i16) {
    let ranked = rank_players(state, order, RankStat::Culture, true);
    tie_census_row(state, "barbarians_culture", order, RankStat::Culture, &ranked[..ranked.len().min(1)]);
    let Some(&q) = ranked.first() else { return };
    let among = count_table(card, |sp| match sp {
        Special::Condition(t) => Some(t),
        Special::A(_) | Special::AllPlayers(_) | Special::B(_) | Special::BestTheaterDoubleCulture | Special::BothPlayers(_) | Special::BuildDiscount(_) | Special::CancelledIfPartiesAttackEachOther | Special::CannotPlayAggressionOrWar | Special::CivilActionBackOnTechDevelop(_) | Special::CivilActionUpgradeUrbanBuildingToTheater | Special::ColonizeDiscardUpTo2MilitaryCardsForBonus(_) | Special::ColonyImmediateBonusApplies | Special::ColonyPermanentBonusTransfers | Special::ComboFoodDiscount(_) | Special::ComboResourceDiscount(_) | Special::CultureFirstColony(_) | Special::CultureIfTopTwoStrength(_) | Special::CultureOnLeaveEqualToLabResourceProduction | Special::CultureOnRevolution(_) | Special::CultureOnTechDevelop(_) | Special::CulturePerAdditionalColony(_) | Special::CulturePerCivilizationWithMoreCulture(_) | Special::CulturePerHappyFromTemplesTheatersWonders(_) | Special::CulturePerLabEqualToLevel | Special::CulturePerLibraryTheaterPair(_) | Special::CulturePerTheater(_) | Special::DecreasePopulation(_) | Special::DestroyUrbanBuildings(_) | Special::DoubleBestMine | Special::DoublesTacticBonusOfOneArmy | Special::ExtraHappyPerHappySource(_) | Special::FinalScoring(_) | Special::FreeCivilAction(_) | Special::FreePopIncreasePerTurn | Special::Gain(_) | Special::GainCulturePerLevelOfRemovedCard(_) | Special::GainFoodOrResources(_) | Special::GainResources(_) | Special::InfantryCountsAsCavalryForTactics | Special::LastRoundSubstitute(_) | Special::LeaderTakeCivilActionDiscount(_) | Special::LibraryDiscountsIfTheater | Special::Lose(_) | Special::MilitaryActionAsCivilPerTurn(_) | Special::MilitaryActionCombinedPopIncreaseAndUnitBuild | Special::NoAttacksBetweenParties | Special::OnAttackBetweenParties(_) | Special::OnBuildCulture(_) | Special::OnBuildCulturePerTechLevelSum | Special::OnReplacePutUnderCompletedWonderHappy(_) | Special::OncePerGameTwoPoliticalActions | Special::OpponentDecreasesPopulation(_) | Special::OpponentsPayDoubleMilitaryActionsToAttackYou | Special::OrTakesSpecialTechnologiesOfSameTotalScienceCost | Special::PeekTopEventCardInPolitics | Special::PerTurnChoice | Special::PlayerWithLeastCulture(_) | Special::PlayerWithMostCulture(_) | Special::PlayersWithMostDiscontentWorkers(_) | Special::PlayersWithMostHappyFaces(_) | Special::PopIncreaseFoodDiscount(_) | Special::RemoveAsPoliticalActionForYellowToken(_) | Special::RemoveAsPoliticalActionFreeColonize | Special::RemoveFromGame | Special::ResourceOnMilitaryUnitBuildOrUpgrade(_) | Special::ResourceOnTechDevelop(_) | Special::ResourcesForMilitaryUnitsPerStrongerCivilization(_) | Special::ResourcesPerLabEqualToLevel | Special::RevolutionUsesMilitaryActionsInstead | Special::ScienceOnTechCardTake(_) | Special::SciencePerBestLabOrLibraryLevel | Special::SciencePerLab(_) | Special::StealColony(_) | Special::StrengthPerArtillery(_) | Special::StrengthPerInfantry(_) | Special::StrengthPerMilitaryUnit(_) | Special::StrengthPerTempleOrGovernmentHappy(_) | Special::StrengthPerUnitType(_) | Special::StrongestPlayer(_) | Special::StrongestPlayers(_) | Special::TakeCivilActionDiscountIfLeaderReplacedThisTurn(_) | Special::TakeFromOpponent(_) | Special::TheaterResourceDiscountIfLibrary(_) | Special::TheaterScienceDiscountIfLibrary(_) | Special::TheaterTechScienceDiscount(_) | Special::VictorTakesCulture | Special::VictorTakesScienceUpTo(_) | Special::VictorTakesYellowTokens | Special::WeakestPlayer(_) | Special::WeakestPlayers(_) | Special::WonderTakeNoExtraCivilActions => None,
    })[live_count_idx(state)];
    if among != 0 {
        // ENGINE BUG (game 7522639): this used to rank by unreversed `order`,
        // which on a strength tie put the CURRENT player first in the
        // "weakest" cutoff -- backwards. §5.3's tie-break protects the
        // current player from a penalty (see `protect_current_from_bad_tie`'s
        // own doc, and `apply_single_target`'s identical `WeakestPlayer`
        // fix); Barbarians' cutoff group is exactly such a penalty selection
        // (whoever falls in it AND has the most culture loses population).
        let protected_order = protect_current_from_bad_tie(order);
        let weakest = rank_players(state, &protected_order, RankStat::Strength, false);
        let cutoff = (among.max(0) as usize).min(weakest.len());
        tie_census_row(state, &format!("barbarians_strength:among={among}"), order, RankStat::Strength, &weakest[..cutoff]);
        let outcome: &[u8] = if weakest[..cutoff].contains(&q) { std::slice::from_ref(&q) } else { &[] };
        tie_census_row(state, "barbarians_outcome", order, RankStat::Culture, outcome);
        if !weakest[..cutoff].contains(&q) {
            return;
        }
    } else {
        tie_census_row(state, "barbarians_outcome", order, RankStat::Culture, std::slice::from_ref(&q));
    }
    interact::enqueue(state, QueueItem::LosePop { player: q, n: n.max(0) as u8 });
}

/// §5.3's `strongestPlayers`/`weakestPlayers`: the top `count` players by
/// strength (a per-live-player-count table, not always 1) each get the
/// card's `gain`/`lose` block through [`apply_gains_block`] ONLY -- NOT the
/// full [`apply_player_block`] (no `_apply_extras`/`_queue_decisions`/
/// `scoring_culture`), exactly like Python's own `apply_gains(state, q,
/// block, rng, sign=sign)` call here. Mirrors
/// `engine/events.py::resolve_event`'s final loop (267-275).
fn resolve_count_targets(state: &mut GameState, order: &[u8], card: CardId) {
    let idx = live_count_idx(state);
    let gain = card.get().special.iter().find_map(|&sp| match sp {
        Special::Gain(b) => Some(b),
        Special::A(_) | Special::AllPlayers(_) | Special::B(_) | Special::BestTheaterDoubleCulture | Special::BothPlayers(_) | Special::BuildDiscount(_) | Special::CancelledIfPartiesAttackEachOther | Special::CannotPlayAggressionOrWar | Special::CivilActionBackOnTechDevelop(_) | Special::CivilActionUpgradeUrbanBuildingToTheater | Special::ColonizeDiscardUpTo2MilitaryCardsForBonus(_) | Special::ColonyImmediateBonusApplies | Special::ColonyPermanentBonusTransfers | Special::ComboFoodDiscount(_) | Special::ComboResourceDiscount(_) | Special::Condition(_) | Special::CultureFirstColony(_) | Special::CultureIfTopTwoStrength(_) | Special::CultureOnLeaveEqualToLabResourceProduction | Special::CultureOnRevolution(_) | Special::CultureOnTechDevelop(_) | Special::CulturePerAdditionalColony(_) | Special::CulturePerCivilizationWithMoreCulture(_) | Special::CulturePerHappyFromTemplesTheatersWonders(_) | Special::CulturePerLabEqualToLevel | Special::CulturePerLibraryTheaterPair(_) | Special::CulturePerTheater(_) | Special::DecreasePopulation(_) | Special::DestroyUrbanBuildings(_) | Special::DoubleBestMine | Special::DoublesTacticBonusOfOneArmy | Special::ExtraHappyPerHappySource(_) | Special::FinalScoring(_) | Special::FreeCivilAction(_) | Special::FreePopIncreasePerTurn | Special::GainCulturePerLevelOfRemovedCard(_) | Special::GainFoodOrResources(_) | Special::GainResources(_) | Special::InfantryCountsAsCavalryForTactics | Special::LastRoundSubstitute(_) | Special::LeaderTakeCivilActionDiscount(_) | Special::LibraryDiscountsIfTheater | Special::Lose(_) | Special::MilitaryActionAsCivilPerTurn(_) | Special::MilitaryActionCombinedPopIncreaseAndUnitBuild | Special::NoAttacksBetweenParties | Special::OnAttackBetweenParties(_) | Special::OnBuildCulture(_) | Special::OnBuildCulturePerTechLevelSum | Special::OnReplacePutUnderCompletedWonderHappy(_) | Special::OncePerGameTwoPoliticalActions | Special::OpponentDecreasesPopulation(_) | Special::OpponentsPayDoubleMilitaryActionsToAttackYou | Special::OrTakesSpecialTechnologiesOfSameTotalScienceCost | Special::PeekTopEventCardInPolitics | Special::PerTurnChoice | Special::PlayerWithLeastCulture(_) | Special::PlayerWithMostCulture(_) | Special::PlayersWithMostDiscontentWorkers(_) | Special::PlayersWithMostHappyFaces(_) | Special::PopIncreaseFoodDiscount(_) | Special::RemoveAsPoliticalActionForYellowToken(_) | Special::RemoveAsPoliticalActionFreeColonize | Special::RemoveFromGame | Special::ResourceOnMilitaryUnitBuildOrUpgrade(_) | Special::ResourceOnTechDevelop(_) | Special::ResourcesForMilitaryUnitsPerStrongerCivilization(_) | Special::ResourcesPerLabEqualToLevel | Special::RevolutionUsesMilitaryActionsInstead | Special::ScienceOnTechCardTake(_) | Special::SciencePerBestLabOrLibraryLevel | Special::SciencePerLab(_) | Special::StealColony(_) | Special::StrengthPerArtillery(_) | Special::StrengthPerInfantry(_) | Special::StrengthPerMilitaryUnit(_) | Special::StrengthPerTempleOrGovernmentHappy(_) | Special::StrengthPerUnitType(_) | Special::StrongestPlayer(_) | Special::StrongestPlayers(_) | Special::TakeCivilActionDiscountIfLeaderReplacedThisTurn(_) | Special::TakeFromOpponent(_) | Special::TheaterResourceDiscountIfLibrary(_) | Special::TheaterScienceDiscountIfLibrary(_) | Special::TheaterTechScienceDiscount(_) | Special::VictorTakesCulture | Special::VictorTakesScienceUpTo(_) | Special::VictorTakesYellowTokens | Special::WeakestPlayer(_) | Special::WeakestPlayers(_) | Special::WonderTakeNoExtraCivilActions => None,
    });
    let lose = card.get().special.iter().find_map(|&sp| match sp {
        Special::Lose(b) => Some(b),
        Special::A(_) | Special::AllPlayers(_) | Special::B(_) | Special::BestTheaterDoubleCulture | Special::BothPlayers(_) | Special::BuildDiscount(_) | Special::CancelledIfPartiesAttackEachOther | Special::CannotPlayAggressionOrWar | Special::CivilActionBackOnTechDevelop(_) | Special::CivilActionUpgradeUrbanBuildingToTheater | Special::ColonizeDiscardUpTo2MilitaryCardsForBonus(_) | Special::ColonyImmediateBonusApplies | Special::ColonyPermanentBonusTransfers | Special::ComboFoodDiscount(_) | Special::ComboResourceDiscount(_) | Special::Condition(_) | Special::CultureFirstColony(_) | Special::CultureIfTopTwoStrength(_) | Special::CultureOnLeaveEqualToLabResourceProduction | Special::CultureOnRevolution(_) | Special::CultureOnTechDevelop(_) | Special::CulturePerAdditionalColony(_) | Special::CulturePerCivilizationWithMoreCulture(_) | Special::CulturePerHappyFromTemplesTheatersWonders(_) | Special::CulturePerLabEqualToLevel | Special::CulturePerLibraryTheaterPair(_) | Special::CulturePerTheater(_) | Special::DecreasePopulation(_) | Special::DestroyUrbanBuildings(_) | Special::DoubleBestMine | Special::DoublesTacticBonusOfOneArmy | Special::ExtraHappyPerHappySource(_) | Special::FinalScoring(_) | Special::FreeCivilAction(_) | Special::FreePopIncreasePerTurn | Special::Gain(_) | Special::GainCulturePerLevelOfRemovedCard(_) | Special::GainFoodOrResources(_) | Special::GainResources(_) | Special::InfantryCountsAsCavalryForTactics | Special::LastRoundSubstitute(_) | Special::LeaderTakeCivilActionDiscount(_) | Special::LibraryDiscountsIfTheater | Special::MilitaryActionAsCivilPerTurn(_) | Special::MilitaryActionCombinedPopIncreaseAndUnitBuild | Special::NoAttacksBetweenParties | Special::OnAttackBetweenParties(_) | Special::OnBuildCulture(_) | Special::OnBuildCulturePerTechLevelSum | Special::OnReplacePutUnderCompletedWonderHappy(_) | Special::OncePerGameTwoPoliticalActions | Special::OpponentDecreasesPopulation(_) | Special::OpponentsPayDoubleMilitaryActionsToAttackYou | Special::OrTakesSpecialTechnologiesOfSameTotalScienceCost | Special::PeekTopEventCardInPolitics | Special::PerTurnChoice | Special::PlayerWithLeastCulture(_) | Special::PlayerWithMostCulture(_) | Special::PlayersWithMostDiscontentWorkers(_) | Special::PlayersWithMostHappyFaces(_) | Special::PopIncreaseFoodDiscount(_) | Special::RemoveAsPoliticalActionForYellowToken(_) | Special::RemoveAsPoliticalActionFreeColonize | Special::RemoveFromGame | Special::ResourceOnMilitaryUnitBuildOrUpgrade(_) | Special::ResourceOnTechDevelop(_) | Special::ResourcesForMilitaryUnitsPerStrongerCivilization(_) | Special::ResourcesPerLabEqualToLevel | Special::RevolutionUsesMilitaryActionsInstead | Special::ScienceOnTechCardTake(_) | Special::SciencePerBestLabOrLibraryLevel | Special::SciencePerLab(_) | Special::StealColony(_) | Special::StrengthPerArtillery(_) | Special::StrengthPerInfantry(_) | Special::StrengthPerMilitaryUnit(_) | Special::StrengthPerTempleOrGovernmentHappy(_) | Special::StrengthPerUnitType(_) | Special::StrongestPlayer(_) | Special::StrongestPlayers(_) | Special::TakeCivilActionDiscountIfLeaderReplacedThisTurn(_) | Special::TakeFromOpponent(_) | Special::TheaterResourceDiscountIfLibrary(_) | Special::TheaterScienceDiscountIfLibrary(_) | Special::TheaterTechScienceDiscount(_) | Special::VictorTakesCulture | Special::VictorTakesScienceUpTo(_) | Special::VictorTakesYellowTokens | Special::WeakestPlayer(_) | Special::WeakestPlayers(_) | Special::WonderTakeNoExtraCivilActions => None,
    });

    let strongest_count = count_table(card, |sp| match sp {
        Special::StrongestPlayers(t) => Some(t),
        Special::A(_) | Special::AllPlayers(_) | Special::B(_) | Special::BestTheaterDoubleCulture | Special::BothPlayers(_) | Special::BuildDiscount(_) | Special::CancelledIfPartiesAttackEachOther | Special::CannotPlayAggressionOrWar | Special::CivilActionBackOnTechDevelop(_) | Special::CivilActionUpgradeUrbanBuildingToTheater | Special::ColonizeDiscardUpTo2MilitaryCardsForBonus(_) | Special::ColonyImmediateBonusApplies | Special::ColonyPermanentBonusTransfers | Special::ComboFoodDiscount(_) | Special::ComboResourceDiscount(_) | Special::Condition(_) | Special::CultureFirstColony(_) | Special::CultureIfTopTwoStrength(_) | Special::CultureOnLeaveEqualToLabResourceProduction | Special::CultureOnRevolution(_) | Special::CultureOnTechDevelop(_) | Special::CulturePerAdditionalColony(_) | Special::CulturePerCivilizationWithMoreCulture(_) | Special::CulturePerHappyFromTemplesTheatersWonders(_) | Special::CulturePerLabEqualToLevel | Special::CulturePerLibraryTheaterPair(_) | Special::CulturePerTheater(_) | Special::DecreasePopulation(_) | Special::DestroyUrbanBuildings(_) | Special::DoubleBestMine | Special::DoublesTacticBonusOfOneArmy | Special::ExtraHappyPerHappySource(_) | Special::FinalScoring(_) | Special::FreeCivilAction(_) | Special::FreePopIncreasePerTurn | Special::Gain(_) | Special::GainCulturePerLevelOfRemovedCard(_) | Special::GainFoodOrResources(_) | Special::GainResources(_) | Special::InfantryCountsAsCavalryForTactics | Special::LastRoundSubstitute(_) | Special::LeaderTakeCivilActionDiscount(_) | Special::LibraryDiscountsIfTheater | Special::Lose(_) | Special::MilitaryActionAsCivilPerTurn(_) | Special::MilitaryActionCombinedPopIncreaseAndUnitBuild | Special::NoAttacksBetweenParties | Special::OnAttackBetweenParties(_) | Special::OnBuildCulture(_) | Special::OnBuildCulturePerTechLevelSum | Special::OnReplacePutUnderCompletedWonderHappy(_) | Special::OncePerGameTwoPoliticalActions | Special::OpponentDecreasesPopulation(_) | Special::OpponentsPayDoubleMilitaryActionsToAttackYou | Special::OrTakesSpecialTechnologiesOfSameTotalScienceCost | Special::PeekTopEventCardInPolitics | Special::PerTurnChoice | Special::PlayerWithLeastCulture(_) | Special::PlayerWithMostCulture(_) | Special::PlayersWithMostDiscontentWorkers(_) | Special::PlayersWithMostHappyFaces(_) | Special::PopIncreaseFoodDiscount(_) | Special::RemoveAsPoliticalActionForYellowToken(_) | Special::RemoveAsPoliticalActionFreeColonize | Special::RemoveFromGame | Special::ResourceOnMilitaryUnitBuildOrUpgrade(_) | Special::ResourceOnTechDevelop(_) | Special::ResourcesForMilitaryUnitsPerStrongerCivilization(_) | Special::ResourcesPerLabEqualToLevel | Special::RevolutionUsesMilitaryActionsInstead | Special::ScienceOnTechCardTake(_) | Special::SciencePerBestLabOrLibraryLevel | Special::SciencePerLab(_) | Special::StealColony(_) | Special::StrengthPerArtillery(_) | Special::StrengthPerInfantry(_) | Special::StrengthPerMilitaryUnit(_) | Special::StrengthPerTempleOrGovernmentHappy(_) | Special::StrengthPerUnitType(_) | Special::StrongestPlayer(_) | Special::TakeCivilActionDiscountIfLeaderReplacedThisTurn(_) | Special::TakeFromOpponent(_) | Special::TheaterResourceDiscountIfLibrary(_) | Special::TheaterScienceDiscountIfLibrary(_) | Special::TheaterTechScienceDiscount(_) | Special::VictorTakesCulture | Special::VictorTakesScienceUpTo(_) | Special::VictorTakesYellowTokens | Special::WeakestPlayer(_) | Special::WeakestPlayers(_) | Special::WonderTakeNoExtraCivilActions => None,
    })[idx]
        .max(0) as usize;
    if strongest_count > 0 {
        let block = gain.unwrap_or(EventBlock::EMPTY);
        let targets = rank_players(state, order, RankStat::Strength, true);
        let selected = &targets[..strongest_count];
        tie_census_row(state, &format!("strongest_count:{strongest_count}"), order, RankStat::Strength, selected);
        // SELECTION is by strength (`targets`, above); RESOLUTION order is
        // not -- RULES_SPEC.md §5.3 ("Multiple-player decisions resolve
        // clockwise from the revealing player") governs whatever decision
        // a selected player's own `apply_gains_block` call opens (Foray's
        // new `FoodOrResSplit` choice, chiefly), so re-walk `order` --
        // already clockwise from the revealer (`order_from`) -- filtered
        // down to just the selected players, instead of iterating
        // `targets` in its own strength-sorted order.
        for &q in order.iter().filter(|q| selected.contains(q)) {
            apply_gains_block(state, q, &block, 1);
        }
    }

    let weakest_count = count_table(card, |sp| match sp {
        Special::WeakestPlayers(t) => Some(t),
        Special::A(_) | Special::AllPlayers(_) | Special::B(_) | Special::BestTheaterDoubleCulture | Special::BothPlayers(_) | Special::BuildDiscount(_) | Special::CancelledIfPartiesAttackEachOther | Special::CannotPlayAggressionOrWar | Special::CivilActionBackOnTechDevelop(_) | Special::CivilActionUpgradeUrbanBuildingToTheater | Special::ColonizeDiscardUpTo2MilitaryCardsForBonus(_) | Special::ColonyImmediateBonusApplies | Special::ColonyPermanentBonusTransfers | Special::ComboFoodDiscount(_) | Special::ComboResourceDiscount(_) | Special::Condition(_) | Special::CultureFirstColony(_) | Special::CultureIfTopTwoStrength(_) | Special::CultureOnLeaveEqualToLabResourceProduction | Special::CultureOnRevolution(_) | Special::CultureOnTechDevelop(_) | Special::CulturePerAdditionalColony(_) | Special::CulturePerCivilizationWithMoreCulture(_) | Special::CulturePerHappyFromTemplesTheatersWonders(_) | Special::CulturePerLabEqualToLevel | Special::CulturePerLibraryTheaterPair(_) | Special::CulturePerTheater(_) | Special::DecreasePopulation(_) | Special::DestroyUrbanBuildings(_) | Special::DoubleBestMine | Special::DoublesTacticBonusOfOneArmy | Special::ExtraHappyPerHappySource(_) | Special::FinalScoring(_) | Special::FreeCivilAction(_) | Special::FreePopIncreasePerTurn | Special::Gain(_) | Special::GainCulturePerLevelOfRemovedCard(_) | Special::GainFoodOrResources(_) | Special::GainResources(_) | Special::InfantryCountsAsCavalryForTactics | Special::LastRoundSubstitute(_) | Special::LeaderTakeCivilActionDiscount(_) | Special::LibraryDiscountsIfTheater | Special::Lose(_) | Special::MilitaryActionAsCivilPerTurn(_) | Special::MilitaryActionCombinedPopIncreaseAndUnitBuild | Special::NoAttacksBetweenParties | Special::OnAttackBetweenParties(_) | Special::OnBuildCulture(_) | Special::OnBuildCulturePerTechLevelSum | Special::OnReplacePutUnderCompletedWonderHappy(_) | Special::OncePerGameTwoPoliticalActions | Special::OpponentDecreasesPopulation(_) | Special::OpponentsPayDoubleMilitaryActionsToAttackYou | Special::OrTakesSpecialTechnologiesOfSameTotalScienceCost | Special::PeekTopEventCardInPolitics | Special::PerTurnChoice | Special::PlayerWithLeastCulture(_) | Special::PlayerWithMostCulture(_) | Special::PlayersWithMostDiscontentWorkers(_) | Special::PlayersWithMostHappyFaces(_) | Special::PopIncreaseFoodDiscount(_) | Special::RemoveAsPoliticalActionForYellowToken(_) | Special::RemoveAsPoliticalActionFreeColonize | Special::RemoveFromGame | Special::ResourceOnMilitaryUnitBuildOrUpgrade(_) | Special::ResourceOnTechDevelop(_) | Special::ResourcesForMilitaryUnitsPerStrongerCivilization(_) | Special::ResourcesPerLabEqualToLevel | Special::RevolutionUsesMilitaryActionsInstead | Special::ScienceOnTechCardTake(_) | Special::SciencePerBestLabOrLibraryLevel | Special::SciencePerLab(_) | Special::StealColony(_) | Special::StrengthPerArtillery(_) | Special::StrengthPerInfantry(_) | Special::StrengthPerMilitaryUnit(_) | Special::StrengthPerTempleOrGovernmentHappy(_) | Special::StrengthPerUnitType(_) | Special::StrongestPlayer(_) | Special::StrongestPlayers(_) | Special::TakeCivilActionDiscountIfLeaderReplacedThisTurn(_) | Special::TakeFromOpponent(_) | Special::TheaterResourceDiscountIfLibrary(_) | Special::TheaterScienceDiscountIfLibrary(_) | Special::TheaterTechScienceDiscount(_) | Special::VictorTakesCulture | Special::VictorTakesScienceUpTo(_) | Special::VictorTakesYellowTokens | Special::WeakestPlayer(_) | Special::WonderTakeNoExtraCivilActions => None,
    })[idx]
        .max(0) as usize;
    if weakest_count > 0 {
        // `weakestPlayers` normally pays the `lose` block with sign -1, but
        // Python overrides to the `gain` block (sign +1) when the card ALSO
        // prints one (`events.py:272-273`) -- unreached by any base-game
        // card (every `weakestPlayers` card prints only `lose`), kept for
        // fidelity anyway.
        let (block, sign) = match gain {
            Some(g) => (g, 1),
            None => (lose.unwrap_or(EventBlock::EMPTY), -1),
        };
        // Same tie-break fix as `conditional_target`'s weakest cutoff above:
        // landing in the `weakestPlayers` group is a penalty (Raiders, Crime
        // Wave -- every base-game card here prints only `lose`), so a
        // strength tie must protect the current player, not prefer them.
        let protected_order = protect_current_from_bad_tie(order);
        let targets = rank_players(state, &protected_order, RankStat::Strength, false);
        let selected = &targets[..weakest_count];
        tie_census_row(state, &format!("weakest_count:{weakest_count}"), order, RankStat::Strength, selected);
        // Same RESOLUTION-order fix as `strongestPlayers` above: `selected`
        // is the correct (tie-protected) SET, `order` gives the clockwise
        // sequence to walk it in for whatever per-player decision this
        // opens (Raiders' new `FoodOrResSplit` choice).
        for &q in order.iter().filter(|q| selected.contains(q)) {
            apply_gains_block(state, q, &block, sign);
        }
    }
}

/// §5.3: apply one event's targeting block to one player. Mirrors
/// `engine/events.py::_apply_player_block` -- `apply_gains` +
/// `scoring_culture`'s one reachable ordinary-event key +
/// `_apply_extras` + `_queue_decisions`, in that order.
///
/// Python's fourth parameter, `order`, is only ever read by `rankingCulture`
/// -- and no non-`scoringEvent` event card in the base game prints that key
/// inside a plain targeting block (verified 2026-08-05; the 15 cards that DO
/// print it are handled separately, through `Special::FinalScoring`, by
/// `resolve_event` itself) -- so it is not threaded through here at all.
fn apply_player_block(state: &mut GameState, idx: u8, block: &EventBlock) {
    apply_gains_block(state, idx, block, 1);

    // `scoring_culture`'s one key an ORDINARY (non-`scoringEvent`) card
    // prints: Civil Unrest's `allPlayers.culturePerDiscontentWorker`. See
    // `cards::EventBlock`'s own doc comment.
    if block.culture_per_discontent_worker != 0 {
        let delta =
            block.culture_per_discontent_worker as i32 * economy::discontent(state, &state.players[idx as usize]);
        if delta != 0 {
            let p = &mut state.players[idx as usize];
            p.culture = (p.culture as i32 + delta).max(0) as u16;
        }
    }

    apply_extras(state, idx, block);
    queue_decisions(state, idx, block);
}

/// §5.3/§5.4.6's `apply_gains`, but reading an [`EventBlock`] rather than a
/// whole card's [`crate::cards::CardEffects`] -- the EXACT gain-block
/// vocabulary [`apply_gains`] already implements (science/culture/food/
/// resources/`foodAndOrResources`/blueTokens/`loseAllStoredFood`/
/// `drawMilitaryCards`/`decreasePopulation`), reusing that function's own
/// private helpers (`add_clamped`/`apply_food_delta`/`apply_resources_delta`)
/// so the arithmetic is not a second copy. Also handles
/// `increasePopulation`, which [`apply_gains`] cannot reach (no base-game
/// aggression card prints it) but a `strongestPlayers`/`weakestPlayers`
/// `gain`/`lose` block or a player-targeting block can (Refugees, Immigration,
/// Development of Settlement).
fn apply_gains_block(state: &mut GameState, idx: u8, block: &EventBlock, sign: i32) {
    if crate::debugflags::replay_debug_all() && (block.food != 0 || block.resources != 0 || block.food_and_or_resources != 0) {
        eprintln!(
            "DEBUG apply_gains_block: idx={idx} sign={sign} block.food={} block.resources={} block.food_and_or_resources={} food_before={} resources_before={} blue_available_before={}",
            block.food, block.resources, block.food_and_or_resources, state.players[idx as usize].food, state.players[idx as usize].resources,
            economy::blue_available(&state.players[idx as usize])
        );
    }
    add_clamped(&mut state.players[idx as usize].science, sign * block.science as i32);
    add_clamped(&mut state.players[idx as usize].culture, sign * block.culture as i32);
    apply_food_delta(state, idx, block.food, sign);
    apply_resources_delta(state, idx, block.resources, sign);
    if crate::debugflags::replay_debug_all() && (block.food != 0 || block.resources != 0) {
        eprintln!(
            "DEBUG apply_gains_block AFTER: idx={idx} food_after={} resources_after={}",
            state.players[idx as usize].food, state.players[idx as usize].resources
        );
    }
    if block.food_and_or_resources != 0 {
        // §5.3: the TARGETED player's own choice of how to split the total
        // between food and resources, clockwise from the revealer when more
        // than one player is targeted -- see `QueueItem::FoodOrResSplit`'s
        // own doc. Exactly `decrease_population`'s shape five lines below
        // (also an "the affected player decides" rule), not a fixed
        // formula applied here directly.
        interact::enqueue(
            state,
            QueueItem::FoodOrResSplit { player: idx, amount: block.food_and_or_resources, lose: sign < 0 },
        );
        if crate::debugflags::replay_debug_all() {
            eprintln!("DEBUG FoodOrResSplit enqueued: idx={idx} amount={} lose={}", block.food_and_or_resources, sign < 0);
        }
    }
    // blueTokens (events.py:91-93). NOT population/workers -- those are
    // YELLOW tokens (`workers_free`/`yellow_bank`), a completely separate
    // pool; blue tokens track food/resource STORAGE capacity (§6.4, CoL
    // p.10: "Your food and resources are represented by blue tokens on the
    // mine and farm technologies you have in play"). A card printing
    // `blueTokens`/"return N blue token(s) to the box" (only Crime Wave and
    // Civil Unrest in the base game -- census confirmed against every
    // `blue_tokens`-nonzero `EventBlock` in `card_table.rs`, 2026-08-14) is
    // CoL p.17's card-symbol token-loss rule, textually distinct from FAQ
    // p.15's "lose 1 population" (`Special::DecreasePopulation`, routed
    // through `QueueItem::LosePop` a few lines below -- correctly a
    // different mechanic, not a second copy of this one): "If you lose blue
    // tokens, you return blue tokens from your blue bank to the box. If
    // there are not enough in your blue bank, you also return tokens from
    // mines and farms of your choice" (CoL p.17). Decrementing the bare
    // `blue_total` counter (bank+cards combined, see its own doc comment)
    // is exactly "return from the bank" WHENEVER the bank can cover it --
    // the ONLY branch a full corpus census (`sources/bgo`, all 241 firings
    // of these two cards, 2026-08-14) ever exercises: `blue_available`
    // (bank) at the moment of loss is never below 1, i.e. never short of
    // the 1 token either card ever takes. The "also strip a token from a
    // farm/mine of your choice" branch this rule describes for an empty
    // bank is real but UNREACHABLE by any data on this machine -- flagged
    // per this module's own top-of-file convention for such branches
    // ("a branch against a value that cannot exist would be dead code
    // nothing exercises"), not implemented blind with no case to test it
    // against. If a future corpus ever shows `blue_available == 0` at one
    // of these firings, that is the signal to build the owner's-choice
    // strip-from-a-card mechanic for real.
    if block.blue_tokens != 0 {
        let p = &mut state.players[idx as usize];
        p.blue_total = (p.blue_total as i32 + sign * block.blue_tokens as i32).max(0) as u8;
    }
    if block.lose_all_stored_food {
        economy::clear_food(&mut state.players[idx as usize]);
    }
    if block.draw_military_cards != 0 {
        draw_military(state, idx, block.draw_military_cards.max(0) as u32);
    }
    if block.decrease_population != 0 {
        // §6.5/FAQ p.15: the OWNER chooses which worker to lose.
        interact::enqueue(
            state,
            QueueItem::LosePop { player: idx, n: block.decrease_population.max(0) as u8 },
        );
    }
    if block.increase_population != 0 {
        free_increase_population(state, idx, block.increase_population.max(0) as u32);
    }
}

/// ENGINE BUG FIX (`docs/REPLAY.md` Finding 1, 2026-08): `increasePopulation`
/// is the effect key on exactly three cards -- Development of Settlement
/// ("Players increase population."), Immigration ("The players with the most
/// happy faces increase population."), Refugees ("The strongest player ...
/// increases population.") -- and on all three the population gain is an
/// unconditional event REWARD, listed in the same terse, cost-free phrasing
/// digital-edition card text uses for every other `EventBlock` gain (Development
/// of Agriculture's "gain 2 food", Development of Crafts' "gain 2 resources",
/// ...); nothing in any of the three cards' text mentions paying food, and
/// namu_events.txt's independent translation phrases Settlement identically
/// ("All civilizations increase their population by 1") with no cost either.
/// This WAS wired to the PAID §6.1 mechanic (`economy::pop_cost`, food
/// deducted, `p.one_time_discount.pop_food` consumed) -- confirmed wrong by
/// replaying real BGO game 7522616 (`sources/bgo/journals`): Purple prepares
/// Development of Settlement, then LATER the same turn performs their own,
/// separately-logged, explicitly PAID "increases population" for "3 food".
/// Reconstructing the turn (yellow_bank 17 entering the turn) proves the
/// event grant must have been FREE: a paid event grant would leave yellow_bank
/// at 16 after paying `pop_cost_base(17) == 2` food, matching the journal's
/// own arithmetic for the LATER paid action only if that first grant charged
/// nothing -- `pop_cost_base(16) == 3`, exactly the "3 food" the human paid.
/// Under the old (paid) code this binary's own reconstruction spent 2 food on
/// the event grant it should not have, leaving only 1 food when Purple's real
/// paid Pop needed 3 -- an `IllegalMove` this binary reported as a mystery
/// "player needs one more civil action than the budget allows" (never actually
/// about civil actions: it's the same-turn PAID Pop failing because an
/// EARLIER free grant had wrongly been billed). See
/// `events::tests::an_event_granted_population_increase_costs_no_food` for
/// the before/after regression.
fn free_increase_population(state: &mut GameState, idx: u8, n: u32) {
    for _ in 0..n {
        // `cost: 0, consume_one_time: false` is the established free-grant
        // shape (`apply.rs::h_pop_free`, Ocean Liners): never reads or
        // consumes the one-time discount, matching a grant that never looked
        // at food in the first place.
        if !economy::increase_population(&mut state.players[idx as usize], 0, false) {
            break; // yellow bank empty; every later call would fail too
        }
    }
}

/// `drawMilitaryCards` (events.py:117-124, `_draw_military`) -- reachable
/// now through a nested targeting block (Development of Politics'
/// `allPlayers`, Politics of Strength's `strongestPlayer`), unlike
/// [`apply_gains`]'s own bare-top-level `drawMilitaryCards`, which no
/// base-game aggression card prints.
fn draw_military(state: &mut GameState, idx: u8, n: u32) {
    // Python's `state.has_military` guard is not reproduced -- same
    // card-database-completeness reasoning `evaluate_final_events`'s own
    // doc comment already gives for the identical guard there.
    if state.age_military == Age::IV {
        return;
    }
    for _ in 0..n {
        match economy::draw_military(state) {
            Some(c) => state.players[idx as usize].hand_military.push(c),
            None => return,
        }
    }
}

/// Event effects with no decision that [`apply_gains_block`] does not cover.
/// Mirrors `engine/events.py::_apply_extras`.
///
/// Good Harvest ("produce food, ignoring corruption and consumption") and New
/// Deposits ("produce resources, ignoring corruption") carry `ignoreCorruption`
/// and `ignoreConsumption` in `data/cards_military_actions.json`, and
/// [`EventBlock`] has no field for either. That is correct, not an omission:
/// corruption and consumption are not haircuts on the production RATING, they
/// are separate end-of-turn payments (`economy.rs` step 3b and the consumption
/// step beside it). Crediting `state_stats().food`/`.resources` here therefore
/// already skips both. Adding an exemption flag would exempt a charge this path
/// never makes.
fn apply_extras(state: &mut GameState, idx: u8, block: &EventBlock) {
    let s = effects::state_stats(state, &state.players[idx as usize]);
    if block.produce_food {
        economy::gain_food(&mut state.players[idx as usize], s.food.max(0) as u16);
    }
    if block.produce_resources {
        economy::gain_resources(&mut state.players[idx as usize], s.resources.max(0) as u16);
    }
    if block.extra_production {
        extra_production(state, idx, &s);
    }
    // "gain X equal to your Y production" -- Python's bare `p.science +=
    // s.science` etc, with NO `max(0, ...)` clamp of its own; `.max(0)` is
    // added here only because `p.science`/`p.culture` are unsigned (`u16`)
    // and every other write to them in this port is already clamped the
    // same way, not because Python clamps this one.
    if block.science_equal_to_science_production {
        let p = &mut state.players[idx as usize];
        p.science = (p.science as i32 + s.science).max(0) as u16;
    }
    if block.culture_equal_to_culture_production {
        let p = &mut state.players[idx as usize];
        p.culture = (p.culture as i32 + s.culture).max(0) as u16;
    }
    if block.culture_equal_to_science_production {
        let p = &mut state.players[idx as usize];
        p.culture = (p.culture as i32 + s.science).max(0) as u16;
    }
    if block.food_equal_to_happy_faces {
        let cap = block.food_equal_to_happy_faces_max;
        let n = if cap != 0 { s.happy.min(cap as i32) } else { s.happy };
        economy::gain_food(&mut state.players[idx as usize], n.max(0) as u16);
    }
    if block.discard_leader_unless_current_age {
        let leader = state.players[idx as usize].leader;
        if !leader.is_none() && leader.get().age != state.age_civil {
            let before = apply::snapshot_action_pools(state, idx);
            apply::on_leave_play(&mut state.players[idx as usize], leader);
            economy::discard_civil(state, leader);
            state.players[idx as usize].leader = CardId::NONE;
            apply::settle_action_pools(state, idx, before);
        }
    }
    if block.take_yellow_tokens_from_weakest != 0 {
        // A FRESH order starting at `state.current`, not the `order`
        // `resolve_event` passed down to `apply_player_block` (which starts
        // at the REVEALER) -- mirrors Python's own `_order_from(state,
        // state.current)` here exactly (`events.py:340`).
        //
        // ENGINE BUG (same shape as `d9e52c6`'s `conditional_target`/
        // `resolve_count_targets` fix): Uncertain Borders' own text is "the
        // STRONGEST civilization takes 1 yellow token from WEAKEST
        // civilization's yellow bank" -- landing in the "weakest" slot here
        // is a penalty (you lose a token) exactly like every other
        // `RankStat::Strength, false` selection in this module, so a
        // strength tie must protect the current player via
        // `protect_current_from_bad_tie`, not hand them the token loss
        // first. This one call site is a separate function from
        // `resolve_event`'s dispatch table (it runs from inside
        // `apply_player_block`, once the STRONGEST player is already
        // decided) and was never covered by that fix. The reference Python
        // (`engine/events.py:340-342`) has the identical unreversed-order
        // bug -- it is not an oracle here, RULES_SPEC.md 5.3 is.
        let fresh_order = protect_current_from_bad_tie(&order_from(state, state.current));
        let weakest = rank_players(state, &fresh_order, RankStat::Strength, false);
        if let Some(&victim) = weakest.iter().find(|&&q| q != idx) {
            let take = (block.take_yellow_tokens_from_weakest.max(0) as u8)
                .min(state.players[victim as usize].yellow_bank);
            state.players[victim as usize].yellow_bank -= take;
            apply::grant_yellow(&mut state.players[idx as usize], take as i32);
        }
    }
    if block.decrease_population_by_half_discontent_workers_rounded_up {
        let n = (economy::discontent(state, &state.players[idx as usize]) + 1) / 2;
        if n > 0 {
            interact::enqueue(state, QueueItem::LosePop { player: idx, n: n as u8 });
        }
    }
    if block.civil_actions_per_discontent_worker != 0 {
        let per = block.civil_actions_per_discontent_worker as i32;
        let loss = -per * economy::discontent(state, &state.players[idx as usize]);
        if loss > 0 {
            // ENGINE BUG (docs/REPLAY.md Take/Bid handoff, 2026-08): this used
            // to ALSO bump `p.ca_penalty_next_turn` -- double-charging the
            // loss, once here and again at `idx`'s own NEXT `economy::
            // end_of_turn` reset a turn later. Direct subtraction alone is
            // the whole effect: by the time this block runs (`resolve_event`/
            // `apply_player_block`, always mid SOME player's Political
            // Phase), every OTHER player's `p.civil_actions` already holds
            // their own pre-loaded, not-yet-spent allotment for their own
            // next turn (their last `end_of_turn` reset ran when their
            // previous Actions phase ended, strictly before this event could
            // fire) -- so subtracting from it right now already IS "lose N
            // CA on your next turn" for them, and for the revealer it is
            // "immediately" per the card's own text. `p.ca_penalty_next_turn`
            // is Rebellion's ONLY writer (grepped `data/*.json`), so this
            // does not touch any other card. Confirmed against game
            // 7522661's raw journal: Rebellion's own text there literally
            // reads "Purple loses 4 civil actions on his next turn", and
            // Purple's reconstructed civil_actions was 1 (of a 5 total) for
            // exactly the one turn the human's journal shows, not two.
            let p = &mut state.players[idx as usize];
            p.civil_actions = (p.civil_actions as i32 - loss).max(0) as i8;
        }
    }
    // `oneTimeDiscount` -- gated on "any of the three sub-amounts nonzero"
    // rather than on the JSON key's own presence (which this port does not
    // separately track): the one base-game card that prints this key
    // (Development of Civil Life) always prints all three sub-amounts
    // nonzero, so the two conditions coincide for every card that exists
    // today. All three fields are banked here at once -- they are three
    // INDEPENDENT one-time grants (see `state::OneTimeDiscount`'s doc
    // comment for the corpus evidence), so a player may spend any or all of
    // them, each clearing only its own field.
    if block.one_time_discount_build_resources != 0
        || block.one_time_discount_develop_science != 0
        || block.one_time_discount_pop_food != 0
    {
        state.players[idx as usize].one_time_discount = OneTimeDiscount {
            build_resources: block.one_time_discount_build_resources,
            develop_science: block.one_time_discount_develop_science,
            pop_food: block.one_time_discount_pop_food,
        };
    }
    if block.destroy_one_urban_building_of_each_opponent {
        for q in 0..state.num_players {
            if q != idx && !state.players[q as usize].resigned {
                interact::enqueue(
                    state,
                    QueueItem::Raid { player: idx, victim: q, max_age: Age::IV, no_loot: true, is_last: true },
                );
            }
        }
    }
    if block.optional_take_cards_with_civil_actions != 0 {
        // International Agreement (CoL p.12): "The strongest civilization
        // MAY immediately take civil cards up to N civil actions BY GIVING
        // UP its next chance to take a political action" -- the forfeit is
        // the cost of USING the privilege, not a tax on merely being
        // offered it. This used to set `skip_next_politics` right here,
        // unconditionally, so a player who fully declined (took zero cards)
        // still lost their own next Politics Phase -- contradicted by real
        // BGO journals logging that player's own subsequent, perfectly
        // legal "<Color> passes Political Phase": the `IllegalMove: PolPass`
        // bucket's 9-game "declines international agreement" subgroup
        // (`docs/REPLAY.md`, 2026-08-14). `interact.rs`'s `ChoiceKind::
        // TakeRow` handler now sets `skip_next_politics` itself, the first
        // (and only) time a `Slot` is actually chosen -- never on `Stop`.
        interact::enqueue(
            state,
            QueueItem::TakeRow { player: idx, budget: block.optional_take_cards_with_civil_actions },
        );
    }
}

/// Economic Progress (2015): corruption, food, consumption, resources.
/// Mirrors `engine/events.py::_extra_production`. The printed
/// `extraProduction.order` key is documentation, not a rule -- Python's own
/// function has this exact sequence hardcoded, never reading `order`
/// (`gen_cards.py`'s `EVENT_BLOCK_IGNORED_KEYS` census).
fn extra_production(state: &mut GameState, idx: u8, s: &effects::Stats) {
    if crate::debugflags::replay_debug_all() {
        eprintln!(
            "DEBUG extra_production BEFORE: idx={idx} food={} resources={} s.food={} s.resources={} yellow_bank={}",
            state.players[idx as usize].food, state.players[idx as usize].resources, s.food, s.resources,
            state.players[idx as usize].yellow_bank
        );
        if crate::debugflags::replay_debug_techs() {
            for (id, slot) in state.players[idx as usize].techs.iter() {
                let card = id.get();
                eprintln!(
                    "  TECH idx={idx} name={} kind={:?} workers={} production.food={}",
                    card.name, card.kind, slot.workers, card.production.food
                );
            }
        }
    }
    let corr = economy::corruption(economy::blue_available(&state.players[idx as usize]));
    let paid = economy::pay_resources(&mut state.players[idx as usize], corr);
    economy::pay_food(&mut state.players[idx as usize], corr - paid);
    economy::gain_food(&mut state.players[idx as usize], s.food.max(0) as u16);
    let need = economy::consumption(state.players[idx as usize].yellow_bank) as u16;
    let p = &mut state.players[idx as usize];
    if p.food >= need {
        economy::pay_food(p, need);
    } else {
        let short = need - p.food;
        p.culture = (p.culture as i32 - 4 * short as i32).max(0) as u16;
        economy::clear_food(p);
    }
    economy::gain_resources(&mut state.players[idx as usize], s.resources.max(0) as u16);
    if crate::debugflags::replay_debug_all() {
        eprintln!(
            "DEBUG extra_production AFTER: idx={idx} food={} resources={}",
            state.players[idx as usize].food, state.players[idx as usize].resources
        );
    }
}

/// Event effects that require this player to choose (§5.3). Mirrors
/// `engine/events.py::_queue_decisions` -- every one of its six keys already
/// has a fully-wired `QueueItem`/`ChoiceKind` in `state.rs`/`interact.rs`
/// (built ahead of this pass), so this is only translation, not new
/// decision machinery.
fn queue_decisions(state: &mut GameState, idx: u8, block: &EventBlock) {
    if block.choose_food != 0 || block.choose_resources != 0 {
        let mut opts = GainOptions::new();
        opts.push(GainOption { food: block.choose_food, resources: 0 });
        opts.push(GainOption { food: 0, resources: block.choose_resources });
        interact::enqueue(state, QueueItem::Choose { player: idx, options: opts });
    }
    if !block.free_build_card.is_none() || block.free_build_kind.is_some() {
        interact::enqueue(
            state,
            QueueItem::FreeBuild {
                player: idx,
                spec: FreeBuildSpec {
                    card: block.free_build_card,
                    age: block.free_build_age,
                    kind: block.free_build_kind,
                    cost: block.free_build_cost.max(0) as u16,
                },
            },
        );
    }
    for _ in 0..block.destroy_own_building.max(0) {
        interact::enqueue(state, QueueItem::DestroyOwn { player: idx });
    }
    for _ in 0..block.lose_colony.max(0) {
        interact::enqueue(state, QueueItem::LoseColony { player: idx });
    }
    if !block.flip_completed_wonder_ages.is_empty() {
        let mut ages = 0u8;
        for &age in block.flip_completed_wonder_ages {
            ages |= 1 << (age as u8);
        }
        interact::enqueue(state, QueueItem::FlipWonder { player: idx, ages });
    }
    if block.discard_military_cards > 0 {
        interact::enqueue(
            state,
            QueueItem::DiscardMilitary { player: idx, n: block.discard_military_cards as u8 },
        );
    }
}

/// One "Impact of ..." Age III scoring event's `scoring_culture`/
/// `rankingCulture` payout, applied immediately (as opposed to
/// [`final_event_awards`], which only computes the steps for the caller to
/// apply, and only at game end). This is `_apply_player_block`'s effect on a
/// `scoringEvent` card's `allPlayers` block: since that block's ONLY
/// recognized keys are the fifteen `scoring_culture` formulas plus
/// `rankingCulture`, and `apply_gains`/`_apply_extras`/`_queue_decisions` all
/// no-op on it (none of their keys ever co-occur with a `scoringEvent`
/// card's, per `gen_cards.py`'s exhaustive `SCORING_BLOCK_FIELDS` census),
/// this one function IS that block's whole effect.
///
/// Deliberately a second, small copy of [`final_event_awards`]'s own
/// ranked/table loop rather than a shared helper: that function is landed,
/// tested, and returns steps for TWO callers (`evaluate_final_events` and
/// the bot evaluator's forecast) to consume identically; reworking it to
/// also serve an immediate-apply caller risked exactly the kind of
/// behaviour-preserving-refactor-that-isn't this project's tests exist to
/// catch, for a few dozen lines.
fn apply_final_scoring_block_live(
    state: &mut GameState,
    order: &[u8],
    block: &crate::cards::FinalScoringBlock,
) {
    let live = game::live_count(state);
    // Same §12.5.2 gate, and for the same reason, as [`final_event_awards`]:
    // this is the twin that actually MUTATES culture, so a gate applied to
    // only one of the two would make the scoring the engine performs disagree
    // with the scoring it reports.
    let ranked = if block.has_ranking && state.active().count() >= 2 {
        Some(rank_by_final_scoring_stat(state, order, block.ranking_stat))
    } else {
        None
    };
    let table: &[i16] = match live {
        2 => &block.ranking_2p,
        3 => &block.ranking_3p,
        _ => &block.ranking_4p,
    };
    for &idx in order {
        let culture = scoring_culture(state, &state.players[idx as usize], block);
        if culture != 0 {
            let p = &mut state.players[idx as usize];
            p.culture = (p.culture as i32 + culture).max(0) as u16;
        }
        if let Some(ranked) = &ranked {
            if let Some(pos) = ranked.iter().position(|&r| r == idx) {
                if pos < table.len() {
                    let amount = table[pos] as i32;
                    if amount != 0 {
                        let p = &mut state.players[idx as usize];
                        p.culture = (p.culture as i32 + amount).max(0) as u16;
                    }
                }
            }
        }
    }
}

// ============================================================== tests ====

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cards::CardId;
    use crate::state::{
        CardList, GameState, PactList, Phase, PlayerState, Tableau, MAX_PLAYERS, ROW_SIZE,
    };

    fn card(name: &str) -> CardId {
        CardId::by_name(name).unwrap_or_else(|| panic!("no such card: {name}"))
    }

    fn blank_player(idx: u8, government: CardId) -> PlayerState {
        PlayerState {
            idx,
            techs: Tableau::new(),
            government,
            leader: CardId::NONE,
            wonder: CardId::NONE,
            wonder_steps: 0,
            completed_wonders: CardList::new(),
            destroyed_wonders: 0,
            homer_wonder: CardId::NONE,
            tactic: CardId::NONE,
            tactic_exclusive: false,
            colonies: CardList::new(),
            flipped_wonders: CardList::new(),
            taken_leader_ages: 0,
            war_declared_by_me: CardId::NONE,
            war_target: 0,
            wars_declared_on_me: [CardId::NONE; MAX_PLAYERS],
            pacts: PactList::new(),
            hand_civil: CardList::new(),
            hand_military: CardList::new(),
            hidden_civil: 0,
            hidden_military: 0,
            yellow_bank: 0,
            yellow_granted: 0,
            workers_free: 0,
            raid_loot_pending: 0,
            blue_total: 0,
            food: 0,
            resources: 0,
            science: 0,
            culture: 0,
            culture_rate_extra: 0,
            science_rate_extra: 0,
            strength_extra: 0,
            happy_extra: 0,
            civil_actions: 0,
            military_actions: 0,
            politics_done: false,
            tactic_action_used: false,
            taken_this_turn: CardList::new(),
            ca_spent_taking: 0,
            hammurabi_used: false,
            hammurabi_replaced_this_turn: false,
            breakthrough_ma_funded: false,
            replaced_leader_this_turn: false,
            trade_food_as_resource_used_this_turn: 0,
            trade_resource_as_food_used_this_turn: 0,
            churchill_used: false,
            homer_used_this_turn: false,
            bach_upgrade_used: false,
            ocean_liners_used: false,
            caesar_double_politics_used: false,
            skip_next_politics: false,
            caesar_second_politics: false,
            peeked_event: CardId::NONE,
            ca_penalty_next_turn: 0,
            mil_discount: 0,
            mil_sci_discount: 0,
            one_time_discount: crate::state::OneTimeDiscount::default(),
            resigned: false,
            food_tokens: crate::state::TokenBank::default(),
            resource_tokens: crate::state::TokenBank::default(),
        }
    }

    fn one_player_state(p0: PlayerState) -> GameState {
        let filler = || blank_player(1, card("Despotism"));
        let mut players = [filler(), filler(), filler(), filler()];
        players[0] = p0;
        GameState {
            num_players: 2,
            seed: 0,
            players,
            current: 0,
            turn: 1,
            round: 2,
            start_player: 0,
            age_civil: crate::cards::Age::A,
            age_military: crate::cards::Age::A,
            civil_deck: CardList::new(),
            military_deck: CardList::new(),
            card_row: [CardId::NONE; ROW_SIZE],
            future_events: CardList::new(),
            current_events: CardList::new(),
            past_events: CardList::new(),
            current_events_age: crate::cards::Age::A,
            seeded_by: [crate::state::NOT_SEEDED; crate::cards::NUM_CARDS],
            available_tactics: CardList::new(),
            civil_discard: [CardList::new(), CardList::new(), CardList::new(), CardList::new(), CardList::new()],
            civil_removed: [CardList::new(), CardList::new(), CardList::new(), CardList::new(), CardList::new()],
            discarded_military: [
                CardList::new(),
                CardList::new(),
                CardList::new(),
                CardList::new(),
                CardList::new(),
            ],
            last_round: false,
            final_round_end: None,
            game_over: false,
            phase: Phase::Actions,
            forced_winner: None,
            pending: crate::state::PendingStack::new(),
            queue: crate::state::Queue::new(),
            last_end_of_turn_culture: [None; crate::state::MAX_PLAYERS],
            last_end_of_turn_science: [None; MAX_PLAYERS],
            last_end_of_turn_resources: [None; MAX_PLAYERS],
            last_end_of_turn_food: [None; MAX_PLAYERS],
        }
    }

    // ------------------------------------------------------------ apply_gains

    #[test]
    fn apply_gains_enslave_grants_food_and_resources_to_the_attacker() {
        // The one base-game aggression card whose top-level effects apply_gains
        // actually does anything with (see this module's top doc comment):
        // "Aggression: Enslave" prints `{gainFood: 2, gainResources: 2,
        // opponentDecreasesPopulation: 1}` -- the last key is combat.rs's job,
        // not apply_gains's.
        let mut p0 = blank_player(0, card("Despotism"));
        p0.blue_total = 10; // enough blue tokens for both gains to land.
        let mut state = one_player_state(p0);
        apply_gains(&mut state, 0, card("Aggression: Enslave"), 1);
        assert_eq!(state.players[0].food, 2);
        assert_eq!(state.players[0].resources, 2);
    }

    #[test]
    fn apply_gains_is_a_no_op_for_a_takefromopponent_only_card() {
        // "Aggression: Spy" prints only `takeFromOpponent`, which apply_gains
        // does not read at all (that dict is combat.rs's own theft loop).
        let p0 = blank_player(0, card("Despotism"));
        let mut state = one_player_state(p0);
        apply_gains(&mut state, 0, card("Aggression: Spy"), 1);
        let p = &state.players[0];
        assert_eq!((p.food, p.resources, p.science, p.culture), (0, 0, 0, 0));
    }

    #[test]
    fn apply_gains_negative_sign_inverts_and_floors_at_zero() {
        let mut p0 = blank_player(0, card("Despotism"));
        p0.food = 1;
        p0.resources = 1;
        let mut state = one_player_state(p0);
        apply_gains(&mut state, 0, card("Aggression: Enslave"), -1);
        // gainFood/gainResources are both 2; losing floors at 0, not -1.
        assert_eq!(state.players[0].food, 0);
        assert_eq!(state.players[0].resources, 0);
    }

    #[test]
    fn an_event_granted_population_increase_costs_no_food() {
        // ENGINE BUG (see `free_increase_population`'s doc comment): the
        // three cards with an `increasePopulation` `EventBlock` key --
        // Development of Settlement, Immigration, Refugees -- grant
        // population the same way every other `EventBlock` gain (food,
        // resources, science, culture) does: unconditionally, matching the
        // terse card text ("Players increase population.") and confirmed by
        // reconstructing real BGO game 7522616's food arithmetic. Before the
        // fix this routed through `economy::pop_cost`/the paid §6.1 formula
        // and silently spent food the player never actually paid.
        let mut p0 = blank_player(0, card("Despotism"));
        p0.yellow_bank = 17;
        p0.food = 3; // enough to pay if (wrongly) charged pop_cost_base(17) == 2
        let mut state = one_player_state(p0);
        resolve_event(&mut state, card("Development of Settlement"), 0);
        assert_eq!(state.players[0].yellow_bank, 16, "population still moves one yellow token");
        assert_eq!(state.players[0].food, 3, "the grant must not touch food at all");
    }

    #[test]
    fn apply_gains_decrease_population_enqueues_lose_pop() {
        // "Barbarians" prints a top-level `decreasePopulation: 1` (never
        // actually reached through apply_gains by Python either -- see this
        // module's top doc comment -- but the branch is real and tested here
        // directly).
        let p0 = blank_player(0, card("Despotism"));
        let mut state = one_player_state(p0);
        apply_gains(&mut state, 0, card("Barbarians"), 1);
        assert_eq!(
            state.queue.pop_front(),
            Some(QueueItem::LosePop { player: 0, n: 1 })
        );
    }

    // ------------------------------------------------------- food_or_resources
    //
    // FOODFIX: §5.3's `foodAndOrResources` gain/lose block (Raiders, Foray)
    // used to be applied here by a fixed "resources first" formula
    // (`events::food_or_resources`, deleted). RULES_SPEC.md line 119 ("Multiple-
    // player decisions resolve clockwise from the revealing player") plus BGO
    // journal evidence (game 7522886: "Green choses first", then Green and
    // Orange resolving an IDENTICAL total-2 loss with DIFFERENT real splits --
    // see /private/tmp/foodchoice/FOODCHOICE.txt) show it is really the
    // targeted player's OWN choice. These tests cover `apply_gains_block`'s
    // enqueue (both directions) and `resolve_event`'s clockwise multi-player
    // ordering; `interact.rs` covers option enumeration (including a player
    // who cannot pay one pool alone) and resolution.

    #[test]
    fn apply_gains_block_food_and_or_resources_enqueues_a_split_choice_for_a_gain() {
        // Foray prints `Special::Gain(EventBlock{food_and_or_resources: 3})`
        // -- exactly `decrease_population`'s "the affected player decides"
        // shape above, not a fixed formula applied inline.
        let p0 = blank_player(0, card("Despotism"));
        let mut state = one_player_state(p0);
        apply_gains_block(&mut state, 0, &EventBlock { food_and_or_resources: 3, ..EventBlock::EMPTY }, 1);
        assert_eq!(state.queue.pop_front(), Some(QueueItem::FoodOrResSplit { player: 0, amount: 3, lose: false }));
    }

    #[test]
    fn apply_gains_block_food_and_or_resources_enqueues_a_split_choice_for_a_loss() {
        // Raiders prints `Special::Lose(EventBlock{food_and_or_resources: 2})`.
        let p0 = blank_player(0, card("Despotism"));
        let mut state = one_player_state(p0);
        apply_gains_block(&mut state, 0, &EventBlock { food_and_or_resources: 2, ..EventBlock::EMPTY }, -1);
        assert_eq!(state.queue.pop_front(), Some(QueueItem::FoodOrResSplit { player: 0, amount: 2, lose: true }));
    }

    #[test]
    fn resolve_event_enqueues_raiders_food_or_res_splits_clockwise_from_the_revealer_not_by_strength() {
        // Raiders' `WeakestPlayers([1, 2, 2])` targets 2 players at 4p.
        // Seats 1 and 3 are tied at the lowest strength (0 vs seats 0/2's
        // 10), so SELECTION genuinely ties between them; revealer is seat
        // 1. Clockwise from seat 1 in a 4p game (`order_from`) is 1, 2, 3,
        // 0 -- so the two `FoodOrResSplit` choices must enqueue seat 1 THEN
        // seat 3, regardless of the strength tie-break `resolve_count_
        // targets` uses to pick WHICH seats are selected (a different
        // concern, `protect_current_from_bad_tie`'s own doc).
        let mut p0 = blank_player(0, card("Despotism"));
        p0.strength_extra = 10;
        let p1 = blank_player(1, card("Despotism"));
        let mut p2 = blank_player(2, card("Despotism"));
        p2.strength_extra = 10;
        let p3 = blank_player(3, card("Despotism"));
        let mut state = multi_player_state(4, &[p0, p1, p2, p3], &[]);
        resolve_event(&mut state, card("Raiders"), 1);
        assert_eq!(
            state.queue.pop_front(),
            Some(QueueItem::FoodOrResSplit { player: 1, amount: 2, lose: true }),
            "seat 1, the revealer, resolves first"
        );
        assert_eq!(
            state.queue.pop_front(),
            Some(QueueItem::FoodOrResSplit { player: 3, amount: 2, lose: true }),
            "seat 3 resolves second -- clockwise from seat 1 is 1, 2, 3, 0"
        );
    }

    #[test]
    fn resolve_event_enqueues_forays_two_gain_splits_at_4p() {
        // Foray's own `StrongestPlayers([1, 2, 2])` mirror of the test
        // above, on the gain side -- covers Foray explicitly (not just
        // Raiders) and the multi-player case for the gain direction too.
        let mut p0 = blank_player(0, card("Despotism"));
        p0.strength_extra = 10;
        let p1 = blank_player(1, card("Despotism"));
        let mut p2 = blank_player(2, card("Despotism"));
        p2.strength_extra = 10;
        let p3 = blank_player(3, card("Despotism"));
        let mut state = multi_player_state(4, &[p0, p1, p2, p3], &[]);
        resolve_event(&mut state, card("Foray"), 0);
        assert_eq!(state.queue.pop_front(), Some(QueueItem::FoodOrResSplit { player: 0, amount: 3, lose: false }));
        assert_eq!(state.queue.pop_front(), Some(QueueItem::FoodOrResSplit { player: 2, amount: 3, lose: false }));
    }

    // -------------------------------------------------------- final scoring

    /// Like `one_player_state`, but with `num_players` real seats (players
    /// beyond `players_in` are filled the same way `one_player_state` fills
    /// player 0's siblings) and a caller-supplied `current_events` list, for
    /// exercising [`final_event_awards`]/[`evaluate_final_events`], which
    /// read across every live player.
    fn multi_player_state(
        num_players: u8,
        players_in: &[PlayerState],
        current_events: &[CardId],
    ) -> GameState {
        let filler = || blank_player(9, card("Despotism"));
        let mut players = [filler(), filler(), filler(), filler()];
        for (i, p) in players_in.iter().enumerate() {
            players[i] = p.clone();
        }
        let mut ce = CardList::new();
        for &c in current_events {
            ce.push(c);
        }
        let mut state = one_player_state(players[0].clone());
        state.num_players = num_players;
        state.players = players;
        state.current_events = ce;
        state
    }

    /// ENGINE BUG regression (`apply_single_target`'s own doc comment, found
    /// chasing the `IllegalMove: Pop` bucket, confirmed against the corpus:
    /// 62/63 of real, genuine `WeakestPlayer` strength ties). §5.3's "ties
    /// broken in favor of the current player" protects the current player
    /// from a PENALTY -- they must be picked LAST among tied players, not
    /// first.
    #[test]
    fn apply_single_target_with_favor_current_false_spares_the_current_player_among_ties() {
        // Three players, all strength 0 (nothing built) -- a genuine 3-way
        // tie with no need to hand-equalize anything.
        let mut state = multi_player_state(
            3,
            &[blank_player(0, card("Despotism")), blank_player(1, card("Despotism")), blank_player(2, card("Despotism"))],
            &[],
        );
        state.current = 1;
        let order = order_from(&state, state.current); // [1, 2, 0]
        let block = EventBlock { science: 5, ..EventBlock::EMPTY };

        apply_single_target(&mut state, &order, RankStat::Strength, false, false, block);

        assert_eq!(state.players[1].science, 0, "the current player must be spared a tied penalty");
        // The tied player farthest from `current` going clockwise (last in
        // `order`) is the one who actually gets it.
        assert_eq!(state.players[0].science, 5, "seat 0 is last in [1, 2, 0], so it is the protected-last target");
        assert_eq!(state.players[2].science, 0);
    }

    /// The mirror of the regression above: a BONUS still favors the current
    /// player by picking them FIRST among ties -- unchanged behaviour,
    /// pinned so a future edit to `apply_single_target` can't quietly flip
    /// this one too.
    #[test]
    fn apply_single_target_with_favor_current_true_picks_the_current_player_among_ties() {
        let mut state = multi_player_state(
            3,
            &[blank_player(0, card("Despotism")), blank_player(1, card("Despotism")), blank_player(2, card("Despotism"))],
            &[],
        );
        state.current = 1;
        let order = order_from(&state, state.current); // [1, 2, 0]
        let block = EventBlock { science: 5, ..EventBlock::EMPTY };

        apply_single_target(&mut state, &order, RankStat::Strength, true, true, block);

        assert_eq!(state.players[1].science, 5, "the current player must win a tied bonus");
        assert_eq!(state.players[0].science, 0);
        assert_eq!(state.players[2].science, 0);
    }

    /// ENGINE BUG regression (game 7522639's actual reveal: 2p, both
    /// players strength 3, revealer holding 15 culture vs 3): `Barbarians`'s
    /// own `conditional_target` computed its "weakest" cutoff group with the
    /// SAME un-reversed, current-player-first tie-break that `apply_single_
    /// target`'s `WeakestPlayer` regression above already covers for a
    /// single target -- but `conditional_target` is a separate function
    /// (Barbarians is the only base-game card with a top-level `target`/
    /// `condition`/`decreasePopulation` combination) and was never fixed
    /// alongside it. On a genuine strength tie this wrongly counted the
    /// revealer -- who also held the most culture -- as one of "the two
    /// weakest civilizations" (read "the weaker" in 2p) and queued a
    /// population loss BGO's own journal says never happened ("No effect").
    #[test]
    fn barbarians_spares_the_current_player_from_a_tied_weakest_cutoff() {
        let mut p0 = blank_player(0, card("Despotism"));
        let mut p1 = blank_player(1, card("Despotism"));
        p0.strength_extra = 3;
        p1.strength_extra = 3; // genuine tie, like the real game's 3-vs-3
        p1.culture = 15; // the revealer holds the most culture...
        p0.culture = 3;
        let mut state = multi_player_state(2, &[p0, p1], &[]);

        resolve_event(&mut state, card("Barbarians"), 1); // seat 1 (Purple) reveals

        assert!(
            state.queue.is_empty(),
            "a tied weakest cutoff must spare the current player -- no LosePop should fire"
        );
    }

    /// The mirror of the regression above: when the most-cultured player is
    /// UNAMBIGUOUSLY (not tied) the weakest, Barbarians must still fire --
    /// pinned so the tie-break fix above can't quietly turn into "never
    /// fires at all".
    #[test]
    fn barbarians_still_fires_when_the_most_cultured_player_is_unambiguously_weakest() {
        let mut p0 = blank_player(0, card("Despotism"));
        let mut p1 = blank_player(1, card("Despotism"));
        p0.strength_extra = 3;
        p1.strength_extra = 1; // revealer is strictly weaker, no tie
        p1.culture = 15; // and holds the most culture
        p0.culture = 3;
        let mut state = multi_player_state(2, &[p0, p1], &[]);

        resolve_event(&mut state, card("Barbarians"), 1); // seat 1 (Purple) reveals

        assert_eq!(state.queue.pop_front(), Some(QueueItem::LosePop { player: 1, n: 1 }));
    }

    /// ENGINE BUG regression (found chasing the `IllegalMove: Build` bucket's
    /// missing-worker games, corpus game 7523355 round 11): `apply_tied_
    /// targets`'s "skip when the tied stat is 0" gate is only correct for
    /// `PlayersWithMostDiscontentWorkers` (0 discontent workers means nobody
    /// genuinely "has" one) -- it must NOT gate `PlayersWithMostHappyFaces`
    /// (Immigration's "all civilizations with the most happy faces gain 1
    /// population"). §5.3: "'All civilizations' with most/least: all tied
    /// civs affected, no tie-break" -- a 0-happy tie is still a tie. Before
    /// the fix, two players both sitting at 0 happy faces (the common case
    /// early in a game) made Immigration silently grant NOBODY a worker,
    /// even though the journal shows both players' own "receives a new
    /// immigrant" line.
    #[test]
    fn immigration_grants_population_to_both_players_tied_at_zero_happy_faces() {
        let mut p0 = blank_player(0, card("Despotism"));
        p0.yellow_bank = 5;
        let mut p1 = blank_player(1, card("Despotism"));
        p1.yellow_bank = 5;
        let mut state = multi_player_state(2, &[p0, p1], &[]);

        resolve_event(&mut state, card("Immigration"), 0);

        assert_eq!(
            state.players[0].workers_free, 1,
            "seat 0 is tied for the most (zero) happy faces and must still be granted a worker"
        );
        assert_eq!(
            state.players[1].workers_free, 1,
            "seat 1 is tied for the most (zero) happy faces and must still be granted a worker"
        );
    }

    /// ENGINE BUG regression (sibling of the two Barbarians regressions
    /// above -- same bug shape, found sweeping for other uncovered
    /// "weakest" selectors after `d9e52c6`): Uncertain Borders' "the
    /// strongest civilization takes 1 yellow token from weakest
    /// civilization's yellow bank" ranked its victim with an unreversed
    /// order, so on a genuine strength tie the CURRENT player -- not
    /// whoever else was tied -- lost the token. Three players: p0
    /// unambiguously strongest (takes the token); p1 and p2 tied weakest;
    /// p2 is `state.current`. §5.3's tie-break must protect p2 (the current
    /// player) from the penalty, leaving p1 as the victim.
    #[test]
    fn uncertain_borders_spares_the_current_player_from_a_tied_weakest_token_loss() {
        let mut p0 = blank_player(0, card("Despotism"));
        let mut p1 = blank_player(1, card("Despotism"));
        let mut p2 = blank_player(2, card("Despotism"));
        p0.strength_extra = 5; // unambiguously strongest
        p1.strength_extra = 1;
        p2.strength_extra = 1; // genuine tie with p1 for weakest
        p1.yellow_bank = 5;
        p2.yellow_bank = 5;
        let mut state = multi_player_state(3, &[p0, p1, p2], &[]);
        state.current = 2; // p2 is the current player -- must be protected

        resolve_event(&mut state, card("Uncertain Borders"), 0);

        assert_eq!(
            state.players[1].yellow_bank, 4,
            "the non-current tied player (p1) must be the one who loses a yellow token"
        );
        assert_eq!(
            state.players[2].yellow_bank, 5,
            "the current player (p2) must be spared from a tied weakest-token-loss selection"
        );
    }

    /// ENGINE BUG regression (`apply_extras`' own doc comment on this arm,
    /// found chasing the `IllegalMove: Take` bucket, confirmed against game
    /// 7522661's raw journal): Rebellion ("Each civilization immediately
    /// spends 2 civil actions ... per discontent worker") used to ALSO write
    /// `p.ca_penalty_next_turn`, double-charging an off-turn target -- once
    /// right here (correctly landing on their own next, not-yet-spent
    /// allotment) and again a whole turn later when THEIR next `economy::
    /// end_of_turn` reset ran and found the leftover penalty still sitting
    /// there. One `apply_player_block` call must cost exactly one turn's CA,
    /// matching the card's own printed "one turn" duration.
    #[test]
    fn civil_actions_per_discontent_worker_costs_exactly_one_turn_not_two() {
        let mut p = blank_player(0, card("Despotism"));
        p.civil_actions = 5;
        p.yellow_bank = 11; // happy_required(11) == 2, happy_extra 0 -> discontent 2
        let mut state = multi_player_state(2, &[p, blank_player(1, card("Despotism"))], &[]);
        let block = EventBlock { civil_actions_per_discontent_worker: -2, ..EventBlock::EMPTY };

        apply_player_block(&mut state, 0, &block);

        assert_eq!(state.players[0].civil_actions, 1, "2 discontent workers * 2 CA lost == 4, 5 - 4 == 1");
        assert_eq!(
            state.players[0].ca_penalty_next_turn, 0,
            "the loss must not ALSO be deferred to the player's next end_of_turn reset -- \
             that would spend it twice, across two different turns"
        );
    }

    #[test]
    fn pending_final_events_only_returns_scoring_event_cards() {
        // "Impact of Industry" (scoringEvent) is pending; an ordinary Age III
        // military card sitting in the same deck is not -- `pending_final_events`
        // must not treat every Age III card in `current_events` as scoring.
        let state = multi_player_state(
            2,
            &[blank_player(0, card("Despotism")), blank_player(1, card("Despotism"))],
            &[card("Impact of Industry")],
        );
        assert_eq!(pending_final_events(&state), vec![card("Impact of Industry")]);
    }

    #[test]
    fn evaluate_final_events_scores_mine_resources_for_impact_of_industry() {
        // Bronze (a starting mine) prints 1 resource/worker; 2 workers on it
        // -> `mine_resources` = 2 -> "Impact of Industry"'s
        // `culturePerResourceProducedByMines: 1` awards 2 culture.
        let mut p0 = blank_player(0, card("Despotism"));
        p0.techs.insert(card("Bronze"), crate::state::TechSlot { workers: 2, stored: 0 });
        let p1 = blank_player(1, card("Despotism"));
        let mut state = multi_player_state(2, &[p0, p1], &[card("Impact of Industry")]);
        evaluate_final_events(&mut state);
        assert_eq!(state.players[0].culture, 2);
        assert_eq!(state.players[1].culture, 0);
    }

    #[test]
    fn evaluate_final_events_applies_the_rankingculture_table_by_live_player_count() {
        // "Impact of Strength" ranks by strength; with nobody's tableau
        // contributing strength, ties break by turn order (start_player
        // first), so at 2p the table [10, 0] goes to seats 0 then 1.
        let p0 = blank_player(0, card("Despotism"));
        let p1 = blank_player(1, card("Despotism"));
        let mut state = multi_player_state(2, &[p0, p1], &[card("Impact of Strength")]);
        evaluate_final_events(&mut state);
        assert_eq!(state.players[0].culture, 10);
        assert_eq!(state.players[1].culture, 0);
    }

    #[test]
    fn impact_of_progress_counts_government_and_special_techs_but_not_temples() {
        // Card text: "2 culture per level of each of its government and
        // special (blue) technologies" -- Temples (Religion / Theology /
        // Organized Religion) are a separate card type and are NOT counted,
        // whatever the blue back of their card. The fixture is game
        // `7521849`'s own final board (2026-08-16 score-divergence pass):
        // Purple's end-game tableau is government Democracy (III, level 3)
        // + one SpecialTech (Masonry, I, level 1) + one Temple (Theology,
        // I, level 1, his very last build before the wonder finish) ->
        // 3+1 = 4 levels -> 8 culture; a version that also counted the
        // Temple would give 10. The earlier pass's test asserted 6/8
        // against a board that was never actually in any journal; the
        // 7521849 numbers below are the real ones.
        let mut p0 = blank_player(0, card("Republic"));
        p0.techs.insert(card("Masonry"), crate::state::TechSlot { workers: 1, stored: 0 });
        let mut p1 = blank_player(1, card("Democracy"));
        p1.techs.insert(card("Masonry"), crate::state::TechSlot { workers: 1, stored: 0 });
        p1.techs.insert(card("Theology"), crate::state::TechSlot { workers: 1, stored: 0 });
        let mut state = multi_player_state(2, &[p0, p1], &[card("Impact of Progress")]);
        evaluate_final_events(&mut state);
        assert_eq!(state.players[0].culture, 6, "seat 0: (gov 2 + special 1) * 2");
        assert_eq!(state.players[1].culture, 8, "seat 1: (gov 3 + special 1) * 2, Temple excluded -- 7521849's true end-board");
    }

    #[test]
    fn impact_of_science_ranks_on_science_production_not_the_science_total() {
        // FAQ v15 ("Impact of Science"): the ranking input is per-round
        // SCIENCE PRODUCTION, "including Leaders and Wonders and Colonies
        // but never Action Cards" -- not the spendable science TOTAL.
        // Seat 1 has a bigger TOTAL (4) but no production; seat 0 produces
        // 2 (two Philosophy workers) and must take the 3p table's 14, with
        // seat 2's single Philosophy worker (production 1) taking the 7.
        // (The old `FinalScoringStat::Science` variant ranked on the TOTAL
        // here and gave the 14 to seat 1 -- the shape of every one of the
        // 7523xxx no-drift divergences, e.g. 7523162.)
        let mut p0 = blank_player(0, card("Despotism"));
        p0.techs.insert(card("Philosophy"), crate::state::TechSlot { workers: 2, stored: 0 });
        let mut p1 = blank_player(1, card("Despotism"));
        p1.science = 4; // a big spendable total, zero production
        let mut p2 = blank_player(2, card("Despotism"));
        p2.techs.insert(card("Philosophy"), crate::state::TechSlot { workers: 1, stored: 0 });
        let mut state = multi_player_state(3, &[p0, p1, p2], &[card("Impact of Science")]);
        evaluate_final_events(&mut state);
        assert_eq!(state.players[0].culture, 14, "most PRODUCTION (2) takes the 14");
        assert_eq!(state.players[1].culture, 0, "a total without production scores nothing");
        assert_eq!(state.players[2].culture, 7, "production 1 takes the 7");
    }

    #[test]
    fn evaluate_final_events_clamps_negative_culture_at_zero_per_award() {
        // "Impact of Happiness" (`culturePerDiscontentWorker: -2`) can drive
        // a player's running culture negative; `evaluate_final_events` clamps
        // after EACH award, matching `p.culture = max(0, p.culture + amount)`.
        let mut p0 = blank_player(0, card("Despotism"));
        p0.techs.insert(card("Bronze"), crate::state::TechSlot { workers: 2, stored: 0 });
        p0.workers_free = 20; // plenty of unused workers to go discontent.
        p0.culture = 1;
        let p1 = blank_player(1, card("Despotism"));
        let mut state = multi_player_state(2, &[p0, p1], &[card("Impact of Happiness")]);
        evaluate_final_events(&mut state);
        assert_eq!(state.players[0].culture, 0);
    }

    #[test]
    fn scoring_culture_counts_unused_workers_as_content_workers() {
        // "Impact of Population" (`culturePerContentWorkerAbove10: 2`). FAQ
        // v15 counts every yellow marker OUTSIDE the Population Bank, so the
        // unused-worker pool is in. An agent once "fixed" this to count only
        // on-card workers, for parity with the deleted Python engine; it cost
        // 141 exact score matches over the corpus. A full `yellow_bank` (18)
        // pins `happy_required` at 0, so discontent is 0 and the count here
        // is exactly 6 + 6 = 12 content workers, 2 above ten, 4 culture.
        let mut p0 = blank_player(0, card("Despotism"));
        p0.yellow_bank = 18;
        p0.techs.insert(card("Bronze"), crate::state::TechSlot { workers: 6, stored: 0 });
        p0.workers_free = 6;
        let block = final_scoring_block(card("Impact of Population")).unwrap();
        let state = one_player_state(p0.clone());
        assert_eq!(scoring_culture(&state, &p0, block), 4);
    }

    #[test]
    fn scoring_culture_is_unchanged_by_moving_a_worker_out_of_the_pool() {
        // Discontent depends only on the yellow bank and happy faces, never
        // on where a token that has left the bank is standing -- so putting a
        // worker to work cannot change an "Impact of Population" award. This
        // is the invariant the on-card-only version broke: under it, the same
        // twelve tokens score 0 in the pool and 4 on a card.
        let mut pooled = blank_player(0, card("Despotism"));
        pooled.yellow_bank = 18;
        pooled.techs.insert(card("Bronze"), crate::state::TechSlot { workers: 6, stored: 0 });
        pooled.workers_free = 6;
        let mut employed = blank_player(0, card("Despotism"));
        employed.yellow_bank = 18;
        employed.techs.insert(card("Bronze"), crate::state::TechSlot { workers: 12, stored: 0 });
        employed.workers_free = 0;
        let block = final_scoring_block(card("Impact of Population")).unwrap();
        assert_eq!(
            scoring_culture(&one_player_state(pooled.clone()), &pooled, block),
            scoring_culture(&one_player_state(employed.clone()), &employed, block),
        );
    }

    #[test]
    fn scoring_culture_counts_completed_wonders_by_their_own_age() {
        // "Impact of Wonders": {A: 5, I: 4, II: 3, III: 2}.
        let mut p0 = blank_player(0, card("Despotism"));
        p0.completed_wonders.push(card("Pyramids")); // Age A wonder -> 5.
        let block = final_scoring_block(card("Impact of Wonders")).unwrap();
        let state = one_player_state(p0.clone());
        assert_eq!(scoring_culture(&state, &p0, block), 5);
    }
}
