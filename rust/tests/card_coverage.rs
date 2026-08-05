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
//! it, not merely had the option to -- OR, for the handful of cards no `Move`
//! variant can ever name at all (below), when the fixture's own recorded
//! STATE proves a real game actually reached the equivalent effect. Both
//! readings share the same bar: a real, played game, not an offered option.
//! `legal_moves_match_python_order` already separately guards that Rust's
//! LEGALITY check for a card matches Python's; what nothing guards today is
//! Rust's EFFECT of actually applying that card, because
//! `apply_matches_python_state_stream`'s state-diff only ever runs on plies
//! real games reached. Across the fixtures, several cards are
//! offered-but-never-chosen (see `differential.rs`'s replay counts) -- under
//! an "offered counts" reading those would already pass, despite `apply()`
//! for them never having been checked against Python once. "Chosen" is the
//! stricter, more useful bar: it is the one the Barbarossa/Bach/Cook bugs
//! actually needed to be caught by, since all three were `apply()`-side
//! effect bugs, not legality bugs. Counting a card as covered merely because
//! it sat in a deck or a hand -- rather than being applied -- would be
//! exactly that weaker reading, and this file does not do it anywhere below.
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
//! real per-card port bug lives at -- is checked separately, in every
//! coverage source below (move-chosen and state-observed alike).
//!
//! ## Cards no `Move` can ever name, and the honest state-based bar for each
//!
//! Three families of card are never named by any `Move` variant, no matter
//! how many games are recorded -- traced through `engine/*.py` (not guessed
//! from a raw "never appears" measurement, which conflates real gaps with
//! structural impossibilities) to confirm each genuinely cannot be a
//! `Move::card()`, not merely that some games didn't happen to reach it. For
//! each, the fixtures' recorded state (every fixture is `state_every: 1`, so
//! this is available at every ply) contains the SAME real-game fact the move
//! layer would have named if it could -- so this file reads it from there
//! instead of leaving the card permanently un-checkable:
//!
//! * **All 16 wonders** ([`taken_wonders`]). A wonder is drafted with
//!   `Move::Take{slot}` -- a ROW SLOT INDEX, not a card name -- and
//!   `engine/actions.py::take_card` auto-starts it (`p.wonder =
//!   WonderInProgress(name)`) the instant it is taken, without ever entering
//!   `hand_civil`. No `Move` variant carries a wonder's `CardId` at all:
//!   `Move::card()`'s match arms name every civil-hand card type EXCEPT
//!   `Wonder`. But `_can_take_gated` only offers a wonder-typed slot at all
//!   when `p.wonder is None`, so a player is NEVER already mid-wonder at the
//!   moment they take one -- which means the OWN ply's post-apply state,
//!   `players[decider].wonder.name`, unambiguously names the wonder that
//!   `Move::Take{slot}` just took. This is not a weaker bar than "chosen": it
//!   is the same move (`Move::Take`) that was actually chosen, with its
//!   identity read from the one place the move's own syntax does not carry
//!   it. (Resolving the slot against the row instead, `card_row[slot]` on
//!   the PRECEDING ply, would name the same card by the same reasoning --
//!   `players[decider].wonder.name` on the take's own ply is preferred here
//!   because it needs no cross-ply bookkeeping and is exactly as direct.)
//! * **All 10 Age A events** ([`resolved_events`]), `Development of
//!   Agriculture` .. `Development of Warfare`. `engine/game.py::new_game`
//!   seeds them directly into `state.current_events` (bypassing
//!   `state.military_deck`, which Age A explicitly sets to `[]`: "military:
//!   no deck in Age A"). Every round, `engine/events.py::
//!   reveal_current_event` pops and resolves the top one automatically, with
//!   no `Move` in between. The ONLY other way a card becomes nameable is
//!   `Move::PrepareEvent`, which requires the card to be in `p.hand_military`
//!   -- reachable only via `economy.draw_military` drawing from
//!   `state.military_deck`, which never contains an Age A card. Age A also
//!   has no politics phase at all (round 1 is `state.phase = "actions"`
//!   unconditionally, and Age A always ends at the first replenish, still
//!   round 1), so `PrepareEvent` could not fire for one even if it were
//!   somehow drawn. But `reveal_current_event` (`engine/events.py:178-179`)
//!   appends the name to `state.past_events` -- APPEND-ONLY, per that same
//!   function, and read by nothing that ever removes an entry -- in the same
//!   branch that calls `resolve_event` to apply its effect. So
//!   `state.past_events` containing a name is exactly "a real game applied
//!   this event's effect", the auto-resolved equivalent of "chosen": there is
//!   no decision to point at, but there is still a real, played game that
//!   really reached and really applied it, and the state stream is where
//!   that fact lives.
//! * **Despotism** ([`despotism_observed`]). The one card in the base game
//!   with `count == [0, 0, 0]` printed as a `Government`
//!   (`engine/cards.py::CardDB.civil_deck`: "Wonders/leaders/starting techs
//!   (count 0) are excluded by count") -- it is assigned directly
//!   (`p.government = "Despotism"`, `engine/game.py::new_game`) and never
//!   enters a deck, a hand, or the card row, so it can never be the target of
//!   `Move::Develop` or `Move::Revolution`. Every player's `government` field
//!   is recorded in every fixture's state from ply 0, so `"Despotism"`
//!   appearing there is a real game that was really assigned it and really
//!   played at least the opening of a turn under it -- the direct analogue of
//!   "a real game chose it", for a card that was never anybody's choice.
//!
//! As of 2026-08-05, all 27 cards in these three families are covered this
//! way by the existing fixtures with no new recordings needed (verified by
//! scanning every fixture for each), so [`STRUCTURAL_EXCLUSIONS`] is empty.
//! The allowlist mechanism -- and both ratchet-direction checks below -- stay
//! in place rather than being deleted: if some future card is found to be
//! GENUINELY unnameable by any `Move` AND unobservable in the recorded
//! state (nothing like that is known to exist today), it belongs here with
//! that trace as the reason, not silently uncovered and not hidden behind an
//! invented state signal that doesn't actually prove the effect ran.
//!
//! Explicitly not candidates for the allowlist, because tracing them found
//! no structural block -- only bad luck across however many games happen to
//! be recorded -- and they are real coverage gaps, not allowlist material:
//! every leader (Hammurabi, Robespierre, Napoleon, Newton, Einstein,
//! Churchill are all ordinary count-1 civil-row cards), every special-tech,
//! every pact (needs 3+ players), Modern Infantry, Iron, Coal, and the
//! `freeCivilAction` action cards (`legal.rs`'s "gap 1" is CLOSED as of
//! today -- see `card_table.rs`'s `FreeCivilActionValue` -- so there is no
//! longer any reason for these to be structurally special; a game just needs
//! to play them). Territories are likewise not here: `Move::PrepareEvent`/
//! `Move::ColumbusColonize`/`Move::SendUnit`/`Move::SendBonus` all name a
//! territory `CardId` directly, so a territory reaching hand and being
//! prepared, or being colonized, is exactly this test's normal "chosen"
//! path -- nothing about being a territory blocks it. (Note that a
//! territory's auto-resolution as an auction, `reveal_current_event`'s
//! `type_of(name) == "territory"` branch, deliberately does NOT append to
//! `past_events` -- only the non-territory branch does -- so
//! [`resolved_events`] cannot and does not accidentally paper over a
//! territory that was only ever drawn, never actually colonized/prepared.)

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use tta::card_table::CARDS;
use tta::cards::CardId;
use tta::fixtures::{self, Json, Record};
use tta::moves::Move;

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

/// `(printed card name, one-line reason, a cheap structural check of that
/// reason)`. The check is re-verified every run so a `gen_cards.py` change
/// that quietly makes a reason false would fail loudly here instead of the
/// allowlist just going stale. Empty today (see the top doc comment) -- kept
/// as live infrastructure, not deleted, because the ratchet this file
/// implements only works if a future genuinely-unnameable-and-unobservable
/// card has somewhere honest to go.
struct Exclusion {
    name: &'static str,
    reason: &'static str,
    still_true: fn(&'static tta::cards::Card) -> bool,
}

const STRUCTURAL_EXCLUSIONS: &[Exclusion] = &[];

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

/// Wonders a fixture actually TOOK -- see the top doc comment's wonder
/// bullet for why `players[decider].wonder.name`, on the very ply
/// `Move::Take{slot}` was chosen, unambiguously names the wonder.
fn taken_wonders() -> BTreeSet<CardId> {
    let dir = fixtures_dir();
    let files = fixtures::fixture_files(&dir)
        .unwrap_or_else(|e| panic!("reading fixtures dir {}: {e}", dir.display()));

    let mut taken = BTreeSet::new();
    for path in files {
        let records = fixtures::read_fixture_file(&path).unwrap_or_else(|e| panic!("{e}"));
        for rec in records {
            let Record::Ply(p) = rec else { continue };
            if !matches!(p.chosen, Move::Take { .. }) {
                continue;
            }
            let Some(state) = &p.state else { continue };
            let Some(players) = state.get("players").and_then(Json::as_arr) else { continue };
            let Some(player) = players.get(p.decider as usize) else { continue };
            let Some(wonder) = player.get("wonder") else { continue };
            let Some(name) = wonder.get("name").and_then(Json::as_str) else { continue };
            if let Some(id) = CardId::by_name(name) {
                taken.insert(id);
            }
        }
    }
    taken
}

/// Every card name `reveal_current_event` actually resolved (applied the
/// effect of) in some fixture -- `state.past_events`, append-only, across
/// every recorded ply. See the top doc comment's Age A bullet: this is the
/// bar relied on for the 10 Age A events specifically, since no `Move` can
/// ever name one, but it is not restricted to Age A -- any event's real
/// resolution shows up here the same way, which only ever ADDS coverage on
/// top of [`chosen_cards`] (e.g. via `Move::PrepareEvent` for later ages),
/// never substitutes a weaker signal for it.
fn resolved_events() -> BTreeSet<CardId> {
    let dir = fixtures_dir();
    let files = fixtures::fixture_files(&dir)
        .unwrap_or_else(|e| panic!("reading fixtures dir {}: {e}", dir.display()));

    let mut resolved = BTreeSet::new();
    for path in files {
        let records = fixtures::read_fixture_file(&path).unwrap_or_else(|e| panic!("{e}"));
        for rec in records {
            let Record::Ply(p) = rec else { continue };
            let Some(state) = &p.state else { continue };
            let Some(past) = state.get("past_events").and_then(Json::as_arr) else { continue };
            for name in past.iter().filter_map(Json::as_str) {
                if let Some(id) = CardId::by_name(name) {
                    resolved.insert(id);
                }
            }
        }
    }
    resolved
}

/// Whether any fixture ever recorded a player whose `government` was
/// `"Despotism"` -- see the top doc comment's Despotism bullet.
fn despotism_observed() -> bool {
    let dir = fixtures_dir();
    let files = fixtures::fixture_files(&dir)
        .unwrap_or_else(|e| panic!("reading fixtures dir {}: {e}", dir.display()));

    for path in files {
        let records = fixtures::read_fixture_file(&path).unwrap_or_else(|e| panic!("{e}"));
        for rec in records {
            let Record::Ply(p) = rec else { continue };
            let Some(state) = &p.state else { continue };
            let Some(players) = state.get("players").and_then(Json::as_arr) else { continue };
            for player in players {
                if player.get("government").and_then(Json::as_str) == Some("Despotism") {
                    return true;
                }
            }
        }
    }
    false
}

#[test]
fn every_card_is_chosen_by_a_fixture_or_named_on_the_allowlist() {
    let mut covered = chosen_cards();
    covered.extend(taken_wonders());
    covered.extend(resolved_events());
    if despotism_observed() {
        let id = CardId::by_name("Despotism")
            .unwrap_or_else(|| panic!("no such card \"Despotism\""));
        covered.insert(id);
    }

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
        if covered.contains(&id) {
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
        "STRUCTURAL_EXCLUSIONS entries for cards a fixture now actually covers -- \
         remove these from the allowlist, coverage only ratchets up: {stale_but_now_covered:?}"
    );

    // Ratchet direction 2: every OTHER card must be covered.
    let mut uncovered: Vec<&'static str> = CARDS
        .iter()
        .enumerate()
        .filter_map(|(i, c)| {
            let id = CardId(i as u16);
            if excluded_ids.contains(&id) || covered.contains(&id) {
                None
            } else {
                Some(c.name)
            }
        })
        .collect();
    uncovered.sort_unstable();

    eprintln!(
        "card coverage: {}/{} cards covered by a fixture (move-chosen or state-observed), \
         {} structurally excluded, {} uncovered",
        covered.len(),
        CARDS.len(),
        STRUCTURAL_EXCLUSIONS.len(),
        uncovered.len(),
    );
    assert!(
        uncovered.is_empty(),
        "{} card(s) are neither chosen/state-observed by any tests/fixtures/*.jsonl recording \
         nor named on STRUCTURAL_EXCLUSIONS above -- either record a fixture that plays one \
         (tools/dump_fixtures.py; try other seeds/bots/player counts) or, if tracing \
         engine/*.py shows it is genuinely unreachable as a Move AND unobservable in the \
         recorded state, add it to STRUCTURAL_EXCLUSIONS with that trace as the reason:\n    {}",
        uncovered.len(),
        uncovered.join("\n    ")
    );
}
