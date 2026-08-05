//! Card-coverage ratchet (2026-08-05): every card in the base game must be
//! CHOSEN by a real decision in at least one `rust/tests/fixtures/*.jsonl`
//! recording, or be named on [`STRUCTURAL_EXCLUSIONS`] with a reason.
//!
//! ## The hole this closes
//!
//! `differential.rs` replays nine recorded games and agrees with Python on
//! every ply -- but that is only agreement about the cards those nine games
//! happen to draw. A card whose Rust behaviour is simply wrong, but that no
//! recorded game ever exercises, passes every assertion in this crate today.
//! That is exactly the shape of gap that hid the Barbarossa/Bach/Cook leader
//! abilities until they were found by hand. This test turns "no fixture
//! exercises card X" from silence into a build failure, and -- because
//! [`STRUCTURAL_EXCLUSIONS`] is checked both ways (missing-but-required AND
//! present-but-no-longer-needed) -- it is a ratchet: total required coverage
//! can only go up over time, never quietly regress by growing the allowlist.
//!
//! ## What "exercised" means here, and why not the weaker reading
//!
//! A card counts as covered when it is the card named by some `ply.chosen`
//! move (`Move::card()`) in some fixture -- i.e. a real game actually PLAYED
//! it, not merely had the option to. `legal_moves_match_python_order`
//! already separately guards that Rust's LEGALITY check for a card matches
//! Python's; what nothing guards today is Rust's EFFECT of actually applying
//! that card, because `apply_matches_python_state_stream`'s state-diff only
//! ever runs on plies real games reached. Across the current nine fixtures,
//! 13 cards are offered-but-never-chosen (see `differential.rs`'s replay
//! counts) -- under an "offered counts" reading those would already pass,
//! despite `apply()` for them never having been checked against Python once.
//! "Chosen" is the stricter, more useful bar: it is the one the
//! Barbarossa/Bach/Cook bugs actually needed to be caught by, since all
//! three were `apply()`-side effect bugs, not legality bugs.
//!
//! ## Granularity: `CardId`, not the printed/base name
//!
//! A handful of base names are printed on more than one physical card across
//! ages (e.g. `Aggression: Plunder` at I/II/III, each a DIFFERENT `CardId`
//! with a different magnitude -- `cards.rs`'s `base_name` field, `engine/
//! cards.py::_disambiguate`). Rolling those up to the base name for this
//! test would let "Plunder (I) got chosen once" excuse "Plunder (III)",
//! which prices completely differently, from ever being checked. Every
//! `CardId` -- the table's actual unit of identity, and the granularity a
//! real per-card port bug lives at -- is checked separately.
//!
//! ## Building [`STRUCTURAL_EXCLUSIONS`] honestly
//!
//! Every entry here was traced through `engine/*.py` (not guessed from a
//! raw "never appears" measurement, which conflates real gaps with
//! structural impossibilities) to confirm the card genuinely cannot be named
//! by any `Move`, not merely that nine games didn't happen to reach it:
//!
//! * **All 16 wonders.** A wonder is drafted with `Move::Take{slot}` -- a
//!   ROW SLOT INDEX, not a card name -- and `engine/actions.py::take_card`
//!   auto-starts it (`p.wonder = WonderInProgress(name)`) the instant it is
//!   taken, without ever entering `hand_civil`. No `Move` variant carries a
//!   wonder's `CardId` at all: `Move::card()`'s match arms name every
//!   civil-hand card type EXCEPT `Wonder`. Wonder identity is real game
//!   state and IS checked -- by the state-diff half of
//!   `apply_matches_python_state_stream`, against `GameState::from_json`'s
//!   `p.wonder` -- just never at the move layer this test measures.
//! * **All 10 Age A events** (`Development of Agriculture` .. `Development
//!   of Warfare`). `engine/game.py::new_game` seeds them directly into
//!   `state.current_events` (bypassing `state.military_deck`, which Age A
//!   explicitly sets to `[]`: "military: no deck in Age A"). Every round,
//!   `engine/events.py::reveal_current_event` pops and resolves the top one
//!   automatically, with no `Move` in between. The ONLY other way a card
//!   becomes nameable is `Move::PrepareEvent`, which requires the card to be
//!   in `p.hand_military` -- reachable only via `economy.draw_military`
//!   drawing from `state.military_deck`, which never contains an Age A card.
//!   Age A also has no politics phase at all (round 1 is `state.phase =
//!   "actions"` unconditionally, and Age A always ends at the first
//!   replenish, still round 1), so `PrepareEvent` could not fire for one
//!   even if it were somehow drawn.
//! * **Despotism.** The one card in the base game with `count == [0, 0, 0]`
//!   printed as a `Government` (`engine/cards.py::CardDB.civil_deck`:
//!   "Wonders/leaders/starting techs (count 0) are excluded by count") --
//!   it is assigned directly (`p.government = "Despotism"`,
//!   `engine/game.py::new_game`) and never enters a deck, a hand, or the
//!   card row, so it can never be the target of `Move::Develop` or
//!   `Move::Revolution`.
//!
//! Explicitly NOT here, because tracing them found no structural block --
//! only bad luck across nine games -- and they are real coverage gaps, not
//! allowlist material: every leader (Hammurabi, Robespierre, Napoleon,
//! Newton, Einstein, Churchill are all ordinary count-1 civil-row cards),
//! every special-tech, every pact (needs 3+ players, which most of the nine
//! fixtures are), Modern Infantry, Iron, Coal, and the `freeCivilAction`
//! action cards (`legal.rs`'s "gap 1" is CLOSED as of today -- see
//! `card_table.rs`'s `FreeCivilActionValue` -- so there is no longer any
//! reason for these to be structurally special; a game just needs to play
//! them). Territories are likewise not here: `Move::PrepareEvent`/
//! `Move::ColumbusColonize`/`Move::SendUnit`/`Move::SendBonus` all name a
//! territory `CardId` directly, so a territory reaching hand and being
//! prepared, or being colonized, is exactly this test's normal "chosen"
//! path -- nothing about being a territory blocks it.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use tta::card_table::CARDS;
use tta::cards::{Age, CardId, CardType};
use tta::fixtures::{self, Record};

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

/// `(printed card name, one-line reason, a cheap structural check of that
/// reason)`. The check is re-verified every run so a `gen_cards.py` change
/// that quietly makes a reason false (e.g. Despotism someday printing a
/// nonzero count) fails loudly here instead of the allowlist just going
/// stale.
struct Exclusion {
    name: &'static str,
    reason: &'static str,
    still_true: fn(&'static tta::cards::Card) -> bool,
}

fn is_wonder(c: &'static tta::cards::Card) -> bool {
    c.kind == CardType::Wonder
}

fn is_age_a_event(c: &'static tta::cards::Card) -> bool {
    c.kind == CardType::Event && c.age == Age::A
}

fn is_uncounted_government(c: &'static tta::cards::Card) -> bool {
    c.kind == CardType::Government && c.count == [0, 0, 0]
}

const WONDER_REASON: &str =
    "wonder: drafted by Move::Take{slot} (a row index, no card name) and \
     auto-started on take (engine/actions.py::take_card); no Move variant \
     ever carries a wonder's CardId -- see this file's top doc comment";

const AGE_A_EVENT_REASON: &str =
    "Age A event: seeded straight into state.current_events and resolved \
     automatically every round (engine/events.py::reveal_current_event); \
     Age A's military_deck is always [], so it can never reach a hand for \
     Move::PrepareEvent either, and Age A has no politics phase at all";

const STRUCTURAL_EXCLUSIONS: &[Exclusion] = &[
    Exclusion { name: "Pyramids", reason: WONDER_REASON, still_true: is_wonder },
    Exclusion { name: "Hanging Gardens", reason: WONDER_REASON, still_true: is_wonder },
    Exclusion { name: "Library of Alexandria", reason: WONDER_REASON, still_true: is_wonder },
    Exclusion { name: "Colossus", reason: WONDER_REASON, still_true: is_wonder },
    Exclusion { name: "Great Wall", reason: WONDER_REASON, still_true: is_wonder },
    Exclusion { name: "St. Peter's Basilica", reason: WONDER_REASON, still_true: is_wonder },
    Exclusion { name: "Taj Mahal", reason: WONDER_REASON, still_true: is_wonder },
    Exclusion { name: "Universitas Carolina", reason: WONDER_REASON, still_true: is_wonder },
    Exclusion { name: "Eiffel Tower", reason: WONDER_REASON, still_true: is_wonder },
    Exclusion { name: "Kremlin", reason: WONDER_REASON, still_true: is_wonder },
    Exclusion { name: "Ocean Liners", reason: WONDER_REASON, still_true: is_wonder },
    Exclusion { name: "Transcontinental Railroad", reason: WONDER_REASON, still_true: is_wonder },
    Exclusion { name: "Fast Food Chains", reason: WONDER_REASON, still_true: is_wonder },
    Exclusion { name: "First Space Flight", reason: WONDER_REASON, still_true: is_wonder },
    Exclusion { name: "Hollywood", reason: WONDER_REASON, still_true: is_wonder },
    Exclusion { name: "Internet", reason: WONDER_REASON, still_true: is_wonder },
    Exclusion {
        name: "Development of Agriculture",
        reason: AGE_A_EVENT_REASON,
        still_true: is_age_a_event,
    },
    Exclusion {
        name: "Development of Civil Life",
        reason: AGE_A_EVENT_REASON,
        still_true: is_age_a_event,
    },
    Exclusion { name: "Development of Crafts", reason: AGE_A_EVENT_REASON, still_true: is_age_a_event },
    Exclusion { name: "Development of Markets", reason: AGE_A_EVENT_REASON, still_true: is_age_a_event },
    Exclusion {
        name: "Development of Politics",
        reason: AGE_A_EVENT_REASON,
        still_true: is_age_a_event,
    },
    Exclusion {
        name: "Development of Religion",
        reason: AGE_A_EVENT_REASON,
        still_true: is_age_a_event,
    },
    Exclusion { name: "Development of Science", reason: AGE_A_EVENT_REASON, still_true: is_age_a_event },
    Exclusion {
        name: "Development of Settlement",
        reason: AGE_A_EVENT_REASON,
        still_true: is_age_a_event,
    },
    Exclusion {
        name: "Development of Trade Routes",
        reason: AGE_A_EVENT_REASON,
        still_true: is_age_a_event,
    },
    Exclusion { name: "Development of Warfare", reason: AGE_A_EVENT_REASON, still_true: is_age_a_event },
    Exclusion {
        name: "Despotism",
        reason: "starting government: count == [0, 0, 0] in every player count \
                 (engine/cards.py::CardDB.civil_deck), assigned directly by \
                 engine/game.py::new_game and never dealt into any deck, hand \
                 or row slot, so Move::Develop/Move::Revolution can never name it",
        still_true: is_uncounted_government,
    },
];

/// Every card `ply.chosen.card()` ever names, across every fixture.
fn chosen_cards() -> BTreeSet<CardId> {
    let dir = fixtures_dir();
    let files = fixtures::fixture_files(&dir)
        .unwrap_or_else(|e| panic!("reading fixtures dir {}: {e}", dir.display()));
    assert!(!files.is_empty(), "no *.jsonl fixtures in {} to measure coverage from", dir.display());

    let mut chosen = BTreeSet::new();
    for path in files {
        let records = fixtures::read_fixture_file(&path).unwrap_or_else(|e| panic!("{e}"));
        for rec in records {
            if let Record::Ply(p) = rec {
                if let Some(card) = p.chosen.card() {
                    chosen.insert(card);
                }
            }
        }
    }
    chosen
}

#[test]
fn every_card_is_chosen_by_a_fixture_or_named_on_the_allowlist() {
    let chosen = chosen_cards();

    // Every exclusion must resolve to a real card, must still satisfy the
    // structural fact its reason claims, and -- ratchet direction 1 -- must
    // still actually be uncovered. An allowlist entry for a card a fixture
    // now covers is exactly the rot this test exists to prevent: it would
    // hide a REAL regression (that card silently losing its only exercise)
    // behind "it was never covered anyway".
    let mut stale_but_now_covered = Vec::new();
    let mut reason_no_longer_true = Vec::new();
    let mut excluded_ids: BTreeSet<CardId> = BTreeSet::new();
    for ex in STRUCTURAL_EXCLUSIONS {
        let id = CardId::by_name(ex.name)
            .unwrap_or_else(|| panic!("STRUCTURAL_EXCLUSIONS: no such card {:?}", ex.name));
        if !(ex.still_true)(id.get()) {
            reason_no_longer_true.push(ex.name);
        }
        if chosen.contains(&id) {
            stale_but_now_covered.push(ex.name);
        }
        excluded_ids.insert(id);
    }
    assert!(
        reason_no_longer_true.is_empty(),
        "STRUCTURAL_EXCLUSIONS entries whose stated structural reason no longer holds \
         (card_table.rs changed under them -- re-verify and fix the entry, don't just \
         delete it): {:#?}",
        reason_no_longer_true
            .iter()
            .map(|name| {
                let reason = STRUCTURAL_EXCLUSIONS.iter().find(|e| &e.name == name).unwrap().reason;
                format!("{name}: {reason}")
            })
            .collect::<Vec<_>>()
    );
    assert!(
        stale_but_now_covered.is_empty(),
        "STRUCTURAL_EXCLUSIONS entries for cards a fixture now actually chooses -- \
         remove these from the allowlist, coverage only ratchets up: {stale_but_now_covered:?}"
    );

    // Ratchet direction 2: every OTHER card must be covered.
    let mut uncovered: Vec<&'static str> = CARDS
        .iter()
        .enumerate()
        .filter_map(|(i, c)| {
            let id = CardId(i as u16);
            if excluded_ids.contains(&id) || chosen.contains(&id) {
                None
            } else {
                Some(c.name)
            }
        })
        .collect();
    uncovered.sort_unstable();

    eprintln!(
        "card coverage: {}/{} cards chosen by a fixture, {} structurally excluded, {} uncovered",
        chosen.len(),
        CARDS.len(),
        STRUCTURAL_EXCLUSIONS.len(),
        uncovered.len(),
    );
    assert!(
        uncovered.is_empty(),
        "{} card(s) are neither chosen by any tests/fixtures/*.jsonl recording nor named on \
         STRUCTURAL_EXCLUSIONS above -- either record a fixture that plays one (tools/\
         dump_fixtures.py; try other seeds/bots/player counts) or, if tracing engine/*.py \
         shows it is genuinely unreachable as a Move, add it to STRUCTURAL_EXCLUSIONS with \
         that trace as the reason:\n    {}",
        uncovered.len(),
        uncovered.join("\n    ")
    );
}
