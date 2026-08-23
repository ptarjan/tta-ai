//! Plain English for one move, and for why the bot likes it.
//!
//! Ported from `advisor/advisor.py`'s `_cost_note`/`_describe`/`describe_move`
//! (a table of Python `if kind == "...":` branches keyed on a move tuple's
//! first element) and `explain`/`FEATURE_WORDS` (turning a feature-vector
//! delta into a short human reason). Both become an exhaustive `match` over
//! [`Move`] here -- DESIGN.md rule 5 already made an ill-shaped move
//! unrepresentable, so unlike Python's `describe_move` (which wraps the whole
//! thing in `try/except` and falls back to `" ".join(str(x) for x in move)`
//! because a stringly-typed tuple can always be malformed), there is no
//! failure mode left to catch: every [`Move`] the compiler accepts is one
//! this module already knows how to describe, so it needs no fallback arm at
//! all.
//!
//! ## A bug fixed crossing into Rust: `develop`'s cost note
//!
//! Python's `_cost_note` prices `develop` with `effects.tech_cost`, the RAW
//! science cost -- but `engine/actions.py::_h_develop` actually charges
//! through the military-tech science-discount pool (Churchill's leader
//! ability) when the technology is a unit, so a Churchill player developing
//! a unit tech saw a cost note that overstated what they were about to pay.
//! [`cost_note`] here uses [`costs::tech_cost_net`] instead, which is what
//! [`crate::apply::h_develop`] (`apply.rs`) actually spends -- see this
//! module's test `developing_a_unit_tech_under_churchills_discount_prices_the_net_cost_not_the_raw_one`.
//! Every other move kind already priced net in Python (`build_cost_net`,
//! `upgrade_cost_net`), so `develop` alone was the gap.

use crate::advisor::state_io::Board;
use crate::bots::weighted::features::Features;
use crate::bots::weighted::weights::{WeightKey, Weights};
use crate::costs;
use crate::economy;
use crate::legal;
use crate::moves::{ChurchillChoice, Move, PactSide};
use crate::state::{ChoiceOption, GameState, Pending};

// ------------------------------------------------------------- cost notes

/// The price tag of a move, in the units a player pays at the table -- the
/// `[...]` tail on a candidate line. Empty when the move has no interesting
/// price (`end_turn`, `pol_pass`, every response to an already-open
/// decision: the cost was already paid when the decision opened).
///
/// Reads costs off the same functions [`crate::apply::apply`] itself spends
/// from (`costs::take_cost`, `build_cost_net`, ...), never a second copy of
/// a formula -- see this module's top doc comment for the one place that
/// mattered (`develop`).
pub fn cost_note(state: &GameState, p: &crate::state::PlayerState, mv: Move) -> String {
    match mv {
        Move::Take { slot } => {
            format!("{} civil action(s)", costs::take_cost(state, p, slot as usize))
        }
        Move::Pop { .. } => {
            // `Move::Pop { full: None }` is only ever legal when the yellow bank has food
            // left to spend (`legal.rs`'s own gate), so `pop_cost` returning
            // `None` (empty bank) cannot happen here -- `unwrap_or(0)` is a
            // defensive floor, not a real fallback path.
            format!("{} food, 1 civil action", economy::pop_cost(state, p).unwrap_or(0))
        }
        Move::Build { card } => {
            let c = costs::build_cost_net(state, p, card).unwrap_or(0);
            // Same question `apply::do_build` asks before it decides whether
            // to call `costs::pay_ca` at all: is this a non-unit build with
            // Development of Civil Life's one-shot discount still banked
            // (`p.one_time_discount.build_resources`)? If so `do_build`
            // charges NO civil action for it, so the note must say that too
            // -- both sites go through `costs::build_civil_life_free`
            // instead of each re-deriving the condition, so they cannot
            // silently disagree the way they once did (docs/REPLAY.md
            // Finding 1, 2026-08; see this module's test
            // `a_civil_life_exempt_build_is_described_as_costing_no_civil_action`).
            if costs::build_civil_life_free(p, card) {
                format!("{c} resources, no civil action")
            } else {
                let word = if costs::is_unit(card) { "military" } else { "civil" };
                format!("{c} resources, 1 {word} action")
            }
        }
        Move::Barbarossa { card } => {
            let (food_disc, res_disc) = legal::barbarossa_discounts(p);
            let food = (economy::pop_cost(state, p).unwrap_or(0) - food_disc).max(0);
            let res = (costs::build_cost_net(state, p, card).unwrap_or(0) - res_disc).max(0);
            format!("{food} food + {res} resources, 1 military action (both halves)")
        }
        Move::BachTheater { from, to } => {
            format!("{} resources, 1 civil action", costs::upgrade_cost(state, p, from, to))
        }
        Move::Upgrade { from, to } => {
            let c = costs::upgrade_cost_net(state, p, from, to);
            let word = if costs::is_unit(to) { "military" } else { "civil" };
            format!("{c} resources, 1 {word} action")
        }
        Move::WonderStep { steps } => {
            format!("{} resources, 1 civil action", costs::wonder_stage_cost(state, p, steps))
        }
        Move::Develop { card, .. } => {
            // See this module's top doc comment: net, not raw -- this is the
            // one place Python priced the wrong number.
            format!("{} science, 1 civil action", costs::tech_cost_net(state, p, card).unwrap_or(0))
        }
        Move::Revolution { card } => {
            format!("{} science, all civil actions", card.get().revolution_cost)
        }
        Move::PlayLeader { .. } | Move::PlayAction { .. } => "1 civil action".to_string(),
        Move::PlayTactic { .. } => "1 military action".to_string(),
        Move::CopyTactic { .. } => "2 military actions".to_string(),
        Move::Aggression { card, .. } | Move::War { card, .. } => {
            format!("{} military action(s)", card.get().military_action_cost)
        }
        Move::TradeFoodAsResource => "1 food".to_string(),
        Move::TradeResourceAsFood => "1 resource".to_string(),
        Move::PopFree
        | Move::Destroy { .. }
        | Move::Churchill { .. }
        | Move::EndTurn
        | Move::PolPass
        | Move::PrepareEvent { .. }
        | Move::RemoveLeaderYellow
        | Move::ColumbusColonize { .. }
        | Move::Bid { .. }
        | Move::BidPass
        | Move::Defend { .. }
        | Move::DefendDone
        | Move::SendUnit { .. }
        | Move::SendBonus { .. }
        | Move::SendDiscard { .. }
        | Move::SendDone
        | Move::Choose { .. }
        | Move::OfferPact { .. }
        | Move::CancelPact { .. }
        | Move::Resign => String::new(),
    }
}

fn tail(note: &str) -> String {
    if note.is_empty() {
        String::new()
    } else {
        format!("  [{note}]")
    }
}

/// `p{idx}`, or `p{idx} (you)` when `board` is given and `idx` is the
/// human's own seat. Mirrors `_who`.
fn who(board: Option<&Board>, idx: u8) -> String {
    if let Some(b) = board {
        if idx == b.me {
            return format!("p{idx} (you)");
        }
    }
    format!("p{idx}")
}

fn option_text(opt: ChoiceOption) -> String {
    match opt {
        ChoiceOption::Card(id) => id.name().to_string(),
        ChoiceOption::Slot(s) => format!("row slot {s}"),
        ChoiceOption::Move(mv) => format!("{mv:?}"),
        ChoiceOption::Gain(g) => format!("+{} food, +{} resources", g.food, g.resources),
        ChoiceOption::Word(kw) => format!("{kw:?}").to_lowercase(),
    }
}

/// Plain English for one move. Mirrors `describe_move`/`_describe`; see this
/// module's top doc comment for why there is no fallback arm.
pub fn describe_move(state: &GameState, mv: Move, board: Option<&Board>) -> String {
    let p = state.actor();
    let t = tail(&cost_note(state, p, mv));
    match mv {
        Move::Take { slot } => {
            let id = state.card_row[slot as usize];
            let card = id.get();
            format!(
                "TAKE '{}' ({:?}, age {:?}) from row slot {slot}{t}",
                id.name(),
                card.kind,
                card.age
            )
        }
        Move::Pop { .. } => format!("INCREASE POPULATION: move a yellow token to your unused pile{t}"),
        Move::PopFree => {
            "INCREASE POPULATION for free (Ocean Liners / leader ability)".to_string()
        }
        Move::Barbarossa { card } => format!(
            "BARBAROSSA: increase population AND build '{}' in one military action{t}",
            card.name()
        ),
        Move::BachTheater { from, to } => format!(
            "BACH: upgrade '{}' to the theater '{}' (once per turn){t}",
            from.name(),
            to.name()
        ),
        Move::Build { card } => format!("BUILD '{}': put an unused worker on it{t}", card.name()),
        Move::Upgrade { from, to } => format!("UPGRADE '{}' -> '{}'{t}", from.name(), to.name()),
        Move::Destroy { card } => {
            let verb = if costs::is_unit(card) { "DISBAND" } else { "DESTROY" };
            format!("{verb} '{}': the worker goes back to your unused pile", card.name())
        }
        Move::WonderStep { steps } => {
            let w = p.wonder;
            let (name, stages_len, step_num) = if w.is_none() {
                ("?", 0, 1)
            } else {
                (w.name(), w.get().stages.len(), p.wonder_steps as usize + 1)
            };
            let mult = if steps > 1 { format!(" x{steps}") } else { String::new() };
            format!("BUILD WONDER '{name}' step {step_num}/{stages_len}{mult}{t}")
        }
        Move::PlayLeader { card } => format!("PLAY LEADER '{}'{t}", card.name()),
        Move::Develop { card, .. } => {
            let c = card.get();
            format!("DEVELOP '{}' ({:?}, age {:?}){t}", card.name(), c.kind, c.age)
        }
        Move::Revolution { card } => format!("REVOLUTION to '{}'{t}", card.name()),
        Move::Churchill { choice } => {
            let which = match choice {
                ChurchillChoice::Culture => "culture",
                ChurchillChoice::Military => "military",
            };
            format!("CHURCHILL: take the {which} bonus")
        }
        Move::PlayTactic { card } => format!("PLAY TACTIC '{}'{t}", card.name()),
        Move::CopyTactic { card } => {
            format!("COPY TACTIC '{}' from the common area{t}", card.name())
        }
        Move::PlayAction { card } => format!("PLAY ACTION CARD '{}'{t}", card.name()),
        Move::EndTurn => "END YOUR TURN (production, then pass the board on)".to_string(),
        Move::PolPass => "PASS on politics (play no military card this turn)".to_string(),
        Move::PrepareEvent { card } => {
            format!("PREPARE EVENT '{}' (into the future events deck)", card.name())
        }
        Move::RemoveLeaderYellow => "REMOVE ALEXANDER from the game: take 1 yellow token from \
             the box into your yellow bank"
            .to_string(),
        Move::ColumbusColonize { card } => format!(
            "REMOVE COLUMBUS from the game: colonize '{}' from your hand, sacrificing nothing",
            card.name()
        ),
        Move::Aggression { card, target } => {
            format!("AGGRESSION '{}' against {}{t}", card.name(), who(board, target))
        }
        Move::War { card, target } => {
            format!("DECLARE WAR '{}' on {}{t}", card.name(), who(board, target))
        }
        Move::OfferPact { card, target, side } => {
            let side_txt = match side {
                PactSide::Unspecified => String::new(),
                PactSide::A => " (side A)".to_string(),
                PactSide::B => " (side B)".to_string(),
            };
            format!("OFFER PACT '{}' to {}{side_txt}", card.name(), who(board, target))
        }
        Move::CancelPact { owner } => format!("CANCEL the pact owned by {}", who(board, owner)),
        Move::Resign => "RESIGN from the game".to_string(),
        Move::Choose { n } => match state.pending.top() {
            Some(Pending::Choice(c)) => match c.options.get(n as usize) {
                Some(opt) => format!("CHOOSE: {}", option_text(opt)),
                None => format!("CHOOSE option {n}"),
            },
            _ => format!("CHOOSE option {n}"),
        },
        Move::Bid { n } => format!("BID {n} military strength"),
        Move::BidPass => "PASS on the bid".to_string(),
        Move::Defend { card } => format!("ADD '{}' to your defence", card.name()),
        Move::DefendDone => "DEFENCE DONE (play no more military cards)".to_string(),
        Move::SendUnit { card } => {
            format!("SACRIFICE a '{}' unit to the colonization force", card.name())
        }
        Move::SendBonus { card } => format!("PLAY '{}' for its colonization value", card.name()),
        Move::SendDiscard { card } => format!(
            "COOK: discard '{}' for +1 colonization force (2 cards maximum)",
            card.name()
        ),
        Move::SendDone => "FORCE COMPLETE (send it and take the colony)".to_string(),
        Move::TradeFoodAsResource => format!("TRADE ROUTES: convert 1 food into 1 resource{t}"),
        Move::TradeResourceAsFood => format!("TRADE ROUTES: convert 1 resource into 1 food{t}"),
    }
}

// -------------------------------------------------------------- reasoning

/// `feature -> how a human would say it`, ported from `FEATURE_WORDS`. Not
/// every [`WeightKey`] has a hand-picked phrase -- Python's dict is a curated
/// subset too -- so the fallback arm is [`WeightKey::name`] with underscores
/// turned to spaces, exactly `FEATURE_WORDS.get(k, k.replace("_", " "))`.
/// That fallback is the reason this `match` is allowed a wildcard arm despite
/// DESIGN.md's rule against them: exhaustiveness is not the safety property
/// here, a readable label for literally every key is, and the fallback
/// already provides one for free.
///
/// `#[allow(clippy::wildcard_enum_match_arm)]`: spelling that fallback out
/// literally (`WeightKey::RateHorizon | ... | WeightKey::TechRedundancyDiscount`)
/// would also name the seven phase-suffixed keys (`CultureRateTrailing` etc.)
/// that `registry.rs`'s `every_weight_key_is_named_by_production_source_
/// outside_its_own_declaration` ratchets as reachable ONLY via `.early()`/
/// `.late()` indirection -- a literal mention here would trip that check for
/// a label fallback, not a real new reader. Two independently-justified
/// mechanical rules collide on this one arm; this allow is the reviewed
/// resolution, not a bypass of either.
#[allow(clippy::wildcard_enum_match_arm)]
fn feature_word(key: WeightKey) -> String {
    let label = match key {
        WeightKey::Culture => "culture",
        WeightKey::CultureRate => "culture/turn",
        WeightKey::Science => "science",
        WeightKey::ScienceRate => "science/turn",
        WeightKey::Strength => "military strength",
        WeightKey::StrengthRel => "strength vs the leader",
        WeightKey::StrengthLead => "military lead",
        WeightKey::StrengthDeficit => "military deficit",
        WeightKey::FoodRate => "food/turn after consumption",
        WeightKey::ResourceRate => "resources/turn after corruption",
        WeightKey::FoodStock => "food",
        WeightKey::ResourceStock => "resources",
        WeightKey::Workers => "workers on cards",
        WeightKey::ProdWorkers => "farm/mine workers",
        WeightKey::UrbanWorkers => "urban workers",
        WeightKey::UnitWorkers => "military units",
        WeightKey::FreeWorkers => "unused workers",
        WeightKey::YellowBank => "population left",
        WeightKey::CivilActions => "civil actions",
        WeightKey::MilitaryActions => "military actions",
        WeightKey::CaLeft => "unspent civil actions",
        WeightKey::MaLeft => "unspent military actions",
        WeightKey::HappyMargin => "happiness margin",
        WeightKey::Discontent => "discontent",
        WeightKey::Uprising => "uprising risk",
        WeightKey::PopCost => "cost of new population",
        WeightKey::Wonders => "completed wonders",
        WeightKey::WonderProgress => "wonder progress",
        WeightKey::WonderRemaining => "wonder cost left",
        WeightKey::TechLevels => "technology level",
        WeightKey::NumTechs => "technologies",
        WeightKey::SpecialTechs => "special technologies",
        WeightKey::GovLevel => "government level",
        WeightKey::Leader => "leader in play",
        WeightKey::HandCivil => "civil cards in hand",
        WeightKey::HandValue => "value of your hand",
        WeightKey::HandMilitary => "military cards in hand",
        WeightKey::HandMilValue => "value of your military hand",
        WeightKey::TacticLevel => "tactic level",
        WeightKey::Colonies => "colonies",
        WeightKey::Pacts => "pacts",
        WeightKey::BlueFree => "blue tokens in your bank",
        WeightKey::CorruptionHeadroom => "room left before corruption worsens",
        WeightKey::ConsumptionHeadroom => "population left before food costs more",
        WeightKey::BestFarm => "farm level",
        WeightKey::BestMine => "mine level",
        WeightKey::BestLab => "lab level",
        WeightKey::BestTemple => "temple level",
        WeightKey::BestLibrary => "library level",
        WeightKey::BestTheater => "theater level",
        WeightKey::BestArena => "arena level",
        WeightKey::BestUnit => "best unit level",
        WeightKey::RivalCulture => "rival culture",
        WeightKey::RivalCultureRate => "rival culture/turn",
        WeightKey::RivalScienceRate => "rival science/turn",
        WeightKey::RivalStrength => "rival strength",
        WeightKey::RivalMeanCulture => "average rival culture",
        other => return other.name().replace('_', " "),
    };
    label.to_string()
}

/// Turn a feature-vector delta into a short reason, best contributions
/// first -- Python's `explain`. `top` caps how many terms are named.
pub fn explain(before: &Features, after: &Features, w: &Weights, top: usize) -> String {
    let mut parts: Vec<(f64, WeightKey, f64)> = Vec::new();
    for &k in WeightKey::ALL {
        let d = after.get(k) - before.get(k);
        if d == 0.0 {
            continue;
        }
        let weight = w.get(k);
        if weight == 0.0 {
            continue;
        }
        parts.push(((weight * d).abs(), k, d));
    }
    parts.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    let words: Vec<String> = parts
        .into_iter()
        .take(top)
        .map(|(_, k, d)| {
            let sign = if d > 0.0 { "+" } else { "" };
            let val =
                if d.fract() == 0.0 { format!("{}", d as i64) } else { format!("{d:.1}") };
            format!("{sign}{val} {}", feature_word(k))
        })
        .collect();
    if words.is_empty() {
        "keeps your options open".to_string()
    } else {
        words.join(", ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::advisor::state_io;
    use crate::apply;
    use crate::bots::weighted::{features, rivals};
    use crate::cards::CardId;
    use crate::game;

    fn card(name: &str) -> CardId {
        CardId::by_name(name).unwrap_or_else(|| panic!("no such card: {name}"))
    }

    /// `develop`'s cost note is `tech_cost_net`, not the raw `tech_cost` --
    /// see this module's top doc comment. Winston Churchill's military
    /// science discount only reduces UNIT technologies' cost, so Swordsmen
    /// (an Infantry tech, printed science cost 4 -- the base units like
    /// Warriors/Bronze print 0 and never exercise this path at all) shows
    /// the gap, and Irrigation (a farm tech, unaffected by the pool) is the
    /// control that must NOT move.
    #[test]
    fn developing_a_unit_tech_under_churchills_discount_prices_the_net_cost_not_the_raw_one() {
        let mut st = game::new_game(2, 1);
        let idx = st.decider();
        st.players[idx as usize].leader = card("Winston Churchill");
        st.players[idx as usize].mil_sci_discount = 3;
        st.players[idx as usize].hand_civil.push(card("Swordsmen"));

        let note =
            cost_note(&st, &st.players[idx as usize], Move::Develop { card: card("Swordsmen"), full: None });
        // Swordsmen costs 4 science printed; a 3-point discount pool nets it
        // to 1 -- the raw price (what the bug priced) would have printed
        // "4 science" instead.
        assert_eq!(note, "1 science, 1 civil action", "{note}");

        // A non-unit tech is untouched by the same discount pool: raw and
        // net agree, so this both documents the boundary and would have
        // caught an over-broad fix that discounted every tech's cost.
        st.players[idx as usize].hand_civil.push(card("Irrigation"));
        let note2 =
            cost_note(&st, &st.players[idx as usize], Move::Develop { card: card("Irrigation"), full: None });
        assert!(note2.starts_with("3 science"), "{note2}");
    }

    /// `apply::do_build` skips `costs::pay_ca` for a non-unit build when
    /// Development of Civil Life's one-shot discount is banked (`p.
    /// one_time_discount.build_resources != 0`) -- see `costs::
    /// build_civil_life_free`'s doc comment and docs/REPLAY.md Finding 1.
    /// Before this test's fix, `cost_note` printed "1 civil action"
    /// regardless, so a human reading the advisor's line for a free build
    /// saw an action-budget number that did not match what actually got
    /// spent at the table. Irrigation (a farm tech, non-unit) is the same
    /// card `build_cost_for_production_card_gets_the_one_time_discount_but_
    /// not_the_per_age_pool` in `apply.rs` uses to exercise this discount
    /// field, so this test's cost math is already pinned down there.
    #[test]
    fn a_civil_life_exempt_build_is_described_as_costing_no_civil_action() {
        let mut st = game::new_game(2, 1);
        let idx = st.decider();
        st.players[idx as usize].one_time_discount.build_resources = 1;

        let note =
            cost_note(&st, &st.players[idx as usize], Move::Build { card: card("Irrigation") });
        assert_eq!(note, "3 resources, no civil action", "{note}");

        // The control: with no discount banked, the same build prints the
        // normal 1-civil-action note -- this is what would have masked the
        // bug if the test only checked the exempt case.
        st.players[idx as usize].one_time_discount.build_resources = 0;
        let note2 =
            cost_note(&st, &st.players[idx as usize], Move::Build { card: card("Irrigation") });
        assert_eq!(note2, "4 resources, 1 civil action", "{note2}");
    }

    #[test]
    fn take_prices_in_civil_actions() {
        let st = game::new_game(3, 2);
        let note = cost_note(&st, st.actor(), Move::Take { slot: 0 });
        assert!(note.ends_with("civil action(s)"), "{note}");
    }

    #[test]
    fn a_take_move_names_the_card_the_type_and_the_row_slot() {
        let st = game::new_game(3, 2);
        let name = st.card_row[0].name();
        let text = describe_move(&st, Move::Take { slot: 0 }, None);
        assert!(text.starts_with(&format!("TAKE '{name}'")), "{text}");
        assert!(text.contains("row slot 0"), "{text}");
    }

    #[test]
    fn end_turn_and_pol_pass_carry_no_cost_tail() {
        let st = game::new_game(2, 4);
        assert!(!describe_move(&st, Move::EndTurn, None).contains('['));
        assert!(!describe_move(&st, Move::PolPass, None).contains('['));
    }

    /// `who` names the human's own seat distinctly, matching `_who`.
    #[test]
    fn aggression_names_the_human_seat_specially() {
        let st = game::new_game(3, 9);
        let board = state_io::new_board(3, 1, 9);
        let text =
            describe_move(&st, Move::Aggression { card: card("Warriors"), target: 1 }, Some(&board));
        assert!(text.contains("p1 (you)"), "{text}");
        let text2 =
            describe_move(&st, Move::Aggression { card: card("Warriors"), target: 2 }, Some(&board));
        assert!(text2.contains("p2") && !text2.contains("p2 (you)"), "{text2}");
    }

    /// `explain` is deterministic and `top` actually caps the term count:
    /// the single best term `top=1` returns is always the first term
    /// `top=3` returns too, for the identical `before`/`after` pair.
    #[test]
    fn explain_top_1_agrees_with_the_first_term_of_top_3() {
        let st = game::new_game(3, 5);
        let idx = st.decider();
        let ctx = rivals::rival_context(&st, idx, None, None);
        let before = features::features(&st, idx, Some(&ctx), None, false);
        let mut trial = st.clone();
        apply::apply(&mut trial, Move::Take { slot: 0 });
        let after = features::features(&trial, idx, Some(&ctx), None, false);
        let w = Weights::defaults();

        let one = explain(&before, &after, &w, 1);
        let three = explain(&before, &after, &w, 3);
        assert_ne!(one, "keeps your options open", "a Take move changes the board");
        assert!(!one.contains(','), "top=1 must name exactly one term: {one}");
        assert!(three.starts_with(&one), "{three:?} should start with {one:?}");
    }

    #[test]
    fn explain_falls_back_to_keeps_your_options_open_with_no_deltas() {
        let f = Features::default();
        let w = Weights::defaults();
        assert_eq!(explain(&f, &f, &w, 3), "keeps your options open");
    }
}
