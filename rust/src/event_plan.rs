//! Who prepared which event, solved from the journal instead of guessed.
//!
//! `Move::PrepareEvent` is the Politics-phase action that puts an Event or
//! Territory card from a player's military hand onto the FUTURE events pile
//! and, in the same breath, reveals and resolves the top card of the CURRENT
//! events pile (`apply::h_prepare_event` -> `events::reveal_current_event`,
//! §5.2). `replay_common.rs` used to treat this as unrecoverable hidden
//! information: it GUESSED forward, inferring a preparation at every
//! Politics decision the journal did not otherwise explain and popping the
//! next observed reveal off a FIFO to satisfy it. One wrong guess (a player
//! who simply passed) desynchronised every event after it.
//!
//! # BGO does log the preparation -- the old reading of the journal was wrong
//!
//! Every preparation is one journal line of exactly this shape:
//!
//! ```text
//! Orange plays event Orange scores 1 culture; Current event:; A / Development of Settlement; Each civilization gains 1 population.
//! ```
//!
//! and it carries three independent facts:
//!
//! - **who** prepared: the line's own actor colour (`Orange`), which is
//!   `state.current` at the moment `reveal_current_event` runs, i.e. the
//!   preparer;
//! - **which age** of card they prepared: `h_prepare_event` scores exactly
//!   `card.level()` culture, so `"scores N culture"` IS the prepared card's
//!   age (never 0 anywhere in the corpus -- Age A events are setup-only);
//! - **what the reveal turned up**: the `"Current event:; <Age> / <Name>"`
//!   clause.
//!
//! Over the 1,011-game corpus the correspondence is exact: 17,889 `"plays
//! event"` lines, 17,889 `"Current event:"` clauses, and not one of either
//! without the other. So the number, the order, the actor and the age of
//! every preparation in the game are all directly observed. What is NOT
//! observed is which specific card of that age was prepared -- and that is
//! what this module solves.
//!
//! # The pile model, and the constraint that pins the identities
//!
//! Preparation pushes onto `future_events`; reveal pops from the END of
//! `current_events`; when `current_events` empties, `recycle_future_events`
//! moves the whole future pile over, shuffles it, and stable-sorts it
//! descending by age so the LOWEST age pops first. BGO logs that moment as
//! `"Future events are now current events."`, always as a clause on the
//! reveal line whose pop emptied the pile (the clause is rendered BEFORE the
//! `"Current event:"` clause on that line, but it describes what happened
//! AFTER the pop -- exactly `reveal_current_event`'s own trailing recycle).
//!
//! That cuts the reveal sequence into [`Batch`]es: one per pile. Batch 0 is
//! the setup pile (`game::new_game`: `num_players + 2` Age A cards). Every
//! later batch is, by construction, exactly the cards prepared while the
//! PREVIOUS batch was being consumed. So:
//!
//! > the multiset of AGES revealed in batch `b+1` must equal the multiset of
//! > `"scores N culture"` values on the preparations made during batch `b`.
//!
//! That is a hard, falsifiable constraint, and over the whole corpus it
//! holds in **3,291 of 3,291** batch windows (2,283 complete batches exact,
//! 1,008 final truncated batches consistent), with batch 0 measuring
//! `num_players + 2` in every game. Nothing here is fitted or tolerant: a
//! game whose journal violates it is reported as [`EventPlanError`] rather
//! than papered over.
//!
//! # What stays underdetermined, and the tie-break
//!
//! Within one batch and one age, the recycle shuffle destroys the order, so
//! which of the same-age preparations became which of the same-age reveals
//! is genuinely unrecoverable -- and irrelevant: the pile is a set, and the
//! reveal ORDER is read straight from the journal, not derived. The
//! canonical tie-break is positional (the `k`-th same-age preparation in a
//! window becomes the `k`-th same-age reveal of the next batch). The only
//! thing it can affect is `GameState::seeded_by`.
//!
//! Cards prepared in the LAST window (the game ended before their pile was
//! ever revealed) and setup-pile cards never reached have no observation at
//! all; they are filled with unused cards of the right age and kind, and are
//! SIMULATED in `replay_common.rs`'s sense -- they exist so the piles have
//! the right size and age profile, and nothing ever validates them.

use std::collections::HashMap;

use crate::cards::{CardId, CardType};

/// One journal-observed preparation: one `"<Color> plays event ..."` line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Preparation {
    /// The journal line this was read off (`Line::lineno`).
    pub lineno: usize,
    /// Seat of the player whose political action this was.
    pub actor: u8,
    /// Age of the card put on the future-events pile, from `"scores N
    /// culture"` -- `h_prepare_event` scores exactly `card.level()`.
    pub level: u8,
    /// The card that goes onto the future-events pile: solved from the next
    /// batch's reveals where one exists, otherwise unused filler.
    pub card: CardId,
    /// The card this action's own reveal turned up.
    pub revealed: CardId,
    /// This reveal emptied the current-events pile, so the future pile
    /// became the current one right after it (`"Future events are now
    /// current events."`).
    pub ends_batch: bool,
}

/// Why a journal's event record could not be reconciled with the pile model.
/// Never used to loosen the solve -- `replay_common.rs` stops the game on it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EventPlanError {
    /// A `"plays event"` line whose `"scores N culture"` clause is missing
    /// or unparseable.
    NoCultureClause { lineno: usize },
    /// A `"plays event"` line with no `"Current event:; <Age> / <Name>"`
    /// clause, or one naming a card this build's table does not know.
    UnknownRevealedCard { lineno: usize, name: String },
    /// The batch constraint failed: a batch revealed more cards of some age
    /// than the preceding window prepared. One of the models is wrong (the
    /// pile mechanics, the culture-per-level rule, or the recycle marker's
    /// meaning) -- worth chasing, never worth relaxing.
    BatchAgeMismatch {
        lineno: usize,
        level: u8,
        prepared: usize,
        revealed: usize,
    },
}

impl std::fmt::Display for EventPlanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EventPlanError::NoCultureClause { lineno } => {
                write!(f, "line {lineno}: \"plays event\" with no parseable \"scores N culture\" clause")
            }
            EventPlanError::UnknownRevealedCard { lineno, name } => {
                write!(f, "line {lineno}: revealed event/territory {name:?} is not a known card")
            }
            EventPlanError::BatchAgeMismatch { lineno, level, prepared, revealed } => write!(
                f,
                "line {lineno}: the current-events pile turned up {revealed} Age-{level} card(s) but only \
                 {prepared} were prepared into it"
            ),
        }
    }
}

/// The whole game's event record, solved.
#[derive(Clone, Debug, Default)]
pub struct EventPlan {
    /// The starting current-events pile in REVEAL order (`game::new_game`
    /// deals `num_players + 2` Age A cards): the journal-observed ones
    /// first, then filler for any the game ended before reaching.
    pub setup_pile: Vec<CardId>,
    /// Every preparation in the game, in journal order.
    pub preparations: Vec<Preparation>,
}

impl EventPlan {
    /// The cards the pile that starts right after `preparations[i]` (which
    /// must have `ends_batch`) will turn up, in reveal order -- i.e. the
    /// journal's own order for the batch the engine is about to recycle
    /// into. Empty when the game ended before that pile was touched.
    pub fn next_batch_reveals(&self, i: usize) -> Vec<CardId> {
        self.preparations[i + 1..]
            .iter()
            .scan(false, |done, p| {
                if *done {
                    return None;
                }
                *done = p.ends_batch;
                Some(p.revealed)
            })
            .collect()
    }
}

/// A `"<Color> plays event ..."` line, before the solve.
struct RawPrep {
    lineno: usize,
    actor: u8,
    level: u8,
    revealed: CardId,
    ends_batch: bool,
}

/// `"...plays event <Color> scores N culture..."` -> `N`, the prepared
/// card's age (see the module doc).
fn prepared_level(text: &str) -> Option<u8> {
    let p = text.find("plays event")?;
    let rest = &text[p..];
    let s = rest.find(" scores ")?;
    let after = &rest[s + " scores ".len()..];
    let end = after.find(|c: char| !c.is_ascii_digit())?;
    if !after[end..].starts_with(" culture") {
        return None;
    }
    after[..end].parse().ok()
}

/// The age tag and event/territory name out of `"...Current event:; <Age> /
/// <Name>; ..."`. BOTH halves are needed: the territory families recur
/// across ages under one printed name (`"Vast Territory"` exists at Age I
/// AND Age II), and `card_table.rs` disambiguates them with a `" (I)"` /
/// `" (II)"` suffix that BGO's journal never prints -- but BGO DOES print
/// the age right there in the same clause.
pub fn current_event_age_and_name(text: &str) -> Option<(&str, &str)> {
    let p = text.find("Current event:; ")?;
    let rest = &text[p + "Current event:; ".len()..];
    let slash = rest.find(" / ")?;
    let age = rest[..slash].trim();
    let after_slash = &rest[slash + 3..];
    let end = after_slash.find(';').unwrap_or(after_slash.len());
    Some((age, after_slash[..end].trim()))
}

/// Resolve a revealed card by the age AND name the journal printed --
/// `"<Name> (<Age>)"` first (`card_table.rs`'s disambiguated spelling),
/// falling back to the bare name for the cards that occur only once.
fn revealed_card(
    card_index: &HashMap<&'static str, CardId>,
    age: &str,
    name: &str,
) -> Option<CardId> {
    card_index
        .get(format!("{name} ({age})").as_str())
        .or_else(|| card_index.get(name))
        .copied()
}

/// BGO's own marker for `events::recycle_future_events`.
const RECYCLE_MARKER: &str = "Future events are now current events.";

/// Solve one game's event record. `lines` is `(lineno, actor_seat, text)`
/// for every `"<Color> plays event ..."` journal line, in journal order --
/// `replay_common.rs` owns the parse that identifies them.
pub fn solve(
    plays_event_lines: &[(usize, u8, &str)],
    card_index: &HashMap<&'static str, CardId>,
    num_players: u8,
) -> Result<EventPlan, EventPlanError> {
    let mut raw = Vec::with_capacity(plays_event_lines.len());
    for &(lineno, actor, text) in plays_event_lines {
        let level = prepared_level(text).ok_or(EventPlanError::NoCultureClause { lineno })?;
        let (age, name) =
            current_event_age_and_name(text).ok_or_else(|| EventPlanError::UnknownRevealedCard {
                lineno,
                name: String::new(),
            })?;
        let revealed = revealed_card(card_index, age, name).ok_or_else(|| {
            EventPlanError::UnknownRevealedCard { lineno, name: format!("{name} ({age})") }
        })?;
        raw.push(RawPrep { lineno, actor, level, revealed, ends_batch: text.contains(RECYCLE_MARKER) });
    }

    // Batch `b` is `raw[starts[b]..starts[b + 1]]`; a batch ENDS at the
    // preparation carrying the recycle marker (see the module doc).
    let mut starts = vec![0usize];
    for (i, p) in raw.iter().enumerate() {
        if p.ends_batch {
            starts.push(i + 1);
        }
    }
    if *starts.last().expect("starts is never empty") != raw.len() {
        starts.push(raw.len());
    }

    let mut used: Vec<CardId> = raw.iter().map(|p| p.revealed).collect();
    let mut solved: Vec<Option<CardId>> = vec![None; raw.len()];

    // Batch `b + 1`'s reveals ARE the cards prepared during batch `b`.
    for b in 1..starts.len().saturating_sub(1) {
        let window = starts[b - 1]..starts[b];
        let batch = starts[b]..starts[b + 1];
        for level in 0..=4u8 {
            let mut slots = window.clone().filter(|&i| raw[i].level == level);
            for i in batch.clone().filter(|&i| raw[i].revealed.level() == level) {
                let slot = slots.next().ok_or(EventPlanError::BatchAgeMismatch {
                    lineno: raw[i].lineno,
                    level,
                    prepared: window.clone().filter(|&j| raw[j].level == level).count(),
                    revealed: batch.clone().filter(|&j| raw[j].revealed.level() == level).count(),
                })?;
                solved[slot] = Some(raw[i].revealed);
            }
        }
    }

    // Setup pile: the journal-observed batch-0 reveals, then filler for any
    // Age A card the game ended before turning up.
    let mut setup_pile: Vec<CardId> = raw[..starts.get(1).copied().unwrap_or(raw.len())]
        .iter()
        .map(|p| p.revealed)
        .collect();
    while setup_pile.len() < num_players as usize + 2 {
        let filler = unused_card_of_level(0, &used);
        used.push(filler);
        setup_pile.push(filler);
    }

    let preparations = raw
        .iter()
        .zip(solved)
        .map(|(p, card)| {
            let card = card.unwrap_or_else(|| {
                let filler = unused_card_of_level(p.level, &used);
                used.push(filler);
                filler
            });
            Preparation {
                lineno: p.lineno,
                actor: p.actor,
                level: p.level,
                card,
                revealed: p.revealed,
                ends_batch: p.ends_batch,
            }
        })
        .collect();

    Ok(EventPlan { setup_pile, preparations })
}

/// Some Event/Territory card of age `level` that this game's plan has not
/// already spoken for. Pure SIMULATED filler for a card the journal never
/// revealed (see the module doc); falls back to ANY card of that age, and
/// finally to any card at all, so a corpus with an unusually deep pile can
/// never panic here.
fn unused_card_of_level(level: u8, used: &[CardId]) -> CardId {
    let ids = (0..crate::CARDS.len() as u16).map(CardId);
    ids.clone()
        .find(|id| {
            id.level() == level
                && matches!(id.kind(), CardType::Event | CardType::Territory)
                && !used.contains(id)
        })
        .or_else(|| ids.clone().find(|id| id.level() == level && !used.contains(id)))
        .or_else(|| ids.clone().find(|id| !used.contains(id)))
        .expect("the card table is larger than any one game's event piles")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::replay_common::build_card_index;

    fn idx() -> HashMap<&'static str, CardId> {
        build_card_index()
    }

    #[test]
    fn the_culture_clause_on_a_plays_event_line_is_the_prepared_cards_age() {
        assert_eq!(
            prepared_level("Orange plays event Orange scores 1 culture; Current event:; A / Development of Settlement; x"),
            Some(1)
        );
        assert_eq!(
            prepared_level("Purple plays event Purple scores 3 culture; Current event:; I / Foray; x"),
            Some(3)
        );
        assert_eq!(prepared_level("Orange builds Warrior Orange spends 2 resources"), None);
    }

    #[test]
    fn a_prepared_card_is_solved_to_the_card_the_next_pile_turns_up_at_that_age() {
        // 2p: setup pile is 4 Age A cards. Two of them are revealed here,
        // the second ending the batch, so the next pile is exactly the two
        // cards prepared during it -- an Age I and an Age II card.
        let lines = [
            (10usize, 0u8, "Orange plays event Orange scores 1 culture; Current event:; A / Development of Settlement; x"),
            (20, 1, "Purple plays event Purple scores 2 culture; Future events are now current events.; Current event:; A / Development of Warfare; x"),
            (30, 0, "Orange plays event Orange scores 1 culture; Current event:; I / Foray; x"),
            (40, 1, "Purple plays event Purple scores 1 culture; Current event:; II / Prosperity; x"),
        ];
        let index = idx();
        let plan = solve(&lines, &index, 2).expect("this journal satisfies the batch constraint");
        assert_eq!(plan.preparations.len(), 4);
        // The Age I preparation (Orange, line 10) became the Age I reveal.
        assert_eq!(plan.preparations[0].card, index["Foray"]);
        // The Age II preparation (Purple, line 20) became the Age II reveal.
        assert_eq!(plan.preparations[1].card, index["Prosperity"]);
        assert_eq!(plan.preparations[0].actor, 0);
        assert_eq!(plan.preparations[1].actor, 1);
        assert!(plan.preparations[1].ends_batch);
        // Setup pile: two observed Age A reveals plus two never reached.
        assert_eq!(plan.setup_pile.len(), 4);
        assert_eq!(plan.setup_pile[0], index["Development of Settlement"]);
        assert_eq!(plan.setup_pile[1], index["Development of Warfare"]);
    }

    #[test]
    fn a_pile_that_turns_up_an_age_nobody_prepared_into_it_is_reported_not_papered_over() {
        // Same shape as above, but both preparations in the first window
        // are Age I while the next pile turns up an Age II card.
        let lines = [
            (10usize, 0u8, "Orange plays event Orange scores 1 culture; Current event:; A / Development of Settlement; x"),
            (20, 1, "Purple plays event Purple scores 1 culture; Future events are now current events.; Current event:; A / Development of Warfare; x"),
            (30, 0, "Orange plays event Orange scores 1 culture; Current event:; I / Foray; x"),
            (40, 1, "Purple plays event Purple scores 1 culture; Current event:; II / Prosperity; x"),
        ];
        let err = solve(&lines, &idx(), 2).expect_err("the batch constraint must fail here");
        assert_eq!(
            err,
            EventPlanError::BatchAgeMismatch { lineno: 40, level: 2, prepared: 0, revealed: 1 }
        );
    }

    #[test]
    fn a_territory_family_that_recurs_across_ages_resolves_to_the_age_the_journal_printed() {
        let index = idx();
        let one = revealed_card(&index, "I", "Vast Territory").expect("Age I copy exists");
        let two = revealed_card(&index, "II", "Vast Territory").expect("Age II copy exists");
        assert_ne!(one, two);
        assert_eq!(one.level(), 1);
        assert_eq!(two.level(), 2);
    }

    #[test]
    fn a_preparation_whose_pile_the_game_never_reached_gets_unused_filler_of_its_own_age() {
        let lines = [(
            10usize,
            0u8,
            "Orange plays event Orange scores 2 culture; Current event:; A / Development of Settlement; x",
        )];
        let plan = solve(&lines, &idx(), 2).expect("a one-preparation journal is consistent");
        assert_eq!(plan.preparations[0].level, 2);
        assert_eq!(plan.preparations[0].card.level(), 2);
        assert!(matches!(plan.preparations[0].card.kind(), CardType::Event | CardType::Territory));
        assert_ne!(plan.preparations[0].card, plan.preparations[0].revealed);
    }

    #[test]
    fn next_batch_reveals_stops_at_the_pile_boundary_the_journal_marks() {
        let lines = [
            (10usize, 0u8, "Orange plays event Orange scores 1 culture; Current event:; A / Development of Settlement; x"),
            (20, 1, "Purple plays event Purple scores 1 culture; Future events are now current events.; Current event:; A / Development of Warfare; x"),
            (30, 0, "Orange plays event Orange scores 1 culture; Current event:; I / Foray; x"),
            (40, 1, "Purple plays event Purple scores 1 culture; Future events are now current events.; Current event:; I / Raiders; x"),
            (50, 0, "Orange plays event Orange scores 1 culture; Current event:; I / Good Harvest; x"),
        ];
        let index = idx();
        let plan = solve(&lines, &index, 2).expect("consistent");
        assert_eq!(plan.next_batch_reveals(1), vec![index["Foray"], index["Raiders"]]);
        assert_eq!(plan.next_batch_reveals(3), vec![index["Good Harvest"]]);
    }
}
