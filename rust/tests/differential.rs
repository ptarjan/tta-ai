//! Differential test against `tools/dump_fixtures.py`'s output
//! (DESIGN.md "How correctness is established").
//!
//! Two layers of assertion, in the order the port grew into them:
//!
//!   * **Type-layer completeness** (`every_real_move_and_card_maps_onto_the_type_layer`,
//!     `every_digest_is_well_formed`, `every_fixture_ends_with_a_state_snapshot`):
//!     every move tag and card name a real game produced is representable by
//!     `Move`/`CardId`, with no game logic involved at all. These ran before
//!     `legal.rs`/`apply.rs` existed and still run first -- a fixture whose
//!     shape the type layer cannot represent should fail here, loudly and
//!     specifically, before the replay below has a chance to produce a
//!     confusing downstream symptom instead.
//!   * **State replay** (`legal_moves_match_python_order`,
//!     `apply_matches_python_state_stream`): actually plays each fixture
//!     through `legal_moves`/`apply` (`GameState::from_json` in
//!     `src/fixtures.rs` loads a dumped state to replay from) and compares
//!     against the recording. See the section doc comment further down for
//!     how, and for why a raw digest comparison was not the right tool.
//!
//! A failure in the first layer is not "a test broke" -- it is "the type
//! layer is missing something a real game does". A failure in the second is
//! "state diverges at ply N, field X" (DESIGN.md) -- see [`ReplayReport`].

use std::path::{Path, PathBuf};

use tta::apply::apply;
use tta::cards::CardId;
use tta::fixtures::{self, Ply, Record};
use tta::legal::legal_moves;
use tta::moves::Move;
use tta::state::GameState;

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

/// Every fixture file, parsed. Panics with a precise `file:line: message`
/// (via `FixtureError`'s `Display`) on the first record this reader or the
/// type layer cannot make sense of -- that message IS the finding.
fn load_all() -> Vec<(PathBuf, Vec<Record>)> {
    let dir = fixtures_dir();
    let files = fixtures::fixture_files(&dir)
        .unwrap_or_else(|e| panic!("reading fixtures dir {}: {e}", dir.display()));
    assert!(
        !files.is_empty(),
        "no *.jsonl fixtures in {} -- generate some with tools/dump_fixtures.py \
         (see rust/tests/fixtures/README or the worker's report for the exact command)",
        dir.display()
    );
    files
        .into_iter()
        .map(|path| {
            let records = fixtures::read_fixture_file(&path)
                .unwrap_or_else(|e| panic!("{e}"));
            (path, records)
        })
        .collect()
}

/// The type layer is complete: every move tag and every card name that
/// `tools/dump_fixtures.py` recorded from real self-play games (across 2p,
/// 3p and 4p, both a greedy bot and a legality-fuzzing random bot, including
/// war, aggression, pacts, colonization auctions and defense -- the
/// rarer/harder-to-reach corners of the move space) is representable by
/// `Move` and `CardId`.
///
/// `fixtures::parse_move` and `fixtures::read_fixture_file` already fail
/// loudly (an `Err`, turned into a panic by `load_all`) on the first tag or
/// card name they cannot map, so simply parsing everything without a panic
/// IS the assertion. This test additionally counts what it saw so a
/// coverage regression (a fixture directory that quietly lost its war/pact/
/// colonize game) is visible in the test output rather than only in a diff.
#[test]
fn every_real_move_and_card_maps_onto_the_type_layer() {
    let all = load_all();
    let mut games = 0usize;
    let mut plies = 0usize;
    let mut tags_seen: std::collections::BTreeSet<&'static str> = Default::default();

    for (path, records) in &all {
        games += 1;
        let mut saw_header = false;
        let mut saw_footer = false;
        for rec in records {
            match rec {
                Record::Header(h) => {
                    saw_header = true;
                    assert!(
                        (2..=4).contains(&h.players),
                        "{}: header claims {}p, outside 2..=4",
                        path.display(),
                        h.players
                    );
                }
                Record::Ply(p) => {
                    plies += 1;
                    // Every legal move parsed (parse_move already ran in
                    // read_fixture_file); re-derive the tag purely for the
                    // coverage tally below.
                    assert!(
                        p.legal.iter().any(|m| moves_eq(m, &p.chosen)),
                        "{}: ply {}: chosen move is not a member of its own \
                         legal list -- python and this reader disagree about \
                         move identity",
                        path.display(),
                        p.ply
                    );
                    if let Some(card) = p.chosen.card() {
                        assert_ne!(
                            card,
                            CardId::NONE,
                            "{}: ply {}: chosen move names CardId::NONE",
                            path.display(),
                            p.ply
                        );
                    }
                    tags_seen.insert(tag_of(&p.chosen));
                }
                Record::Footer(_) => saw_footer = true,
            }
        }
        assert!(saw_header, "{}: no header record", path.display());
        assert!(saw_footer, "{}: no footer record", path.display());
    }

    eprintln!(
        "differential: {games} fixture files, {plies} plies, {} distinct move tags: {tags_seen:?}",
        tags_seen.len()
    );
    assert!(games >= 3, "expected fixtures for at least 2p/3p/4p, found {games} files");
}

/// Structural move equality good enough for "is `chosen` one of `legal`"
/// (`Move` derives `PartialEq`, but that is exactly what this calls --
/// spelled out as its own function only so the assertion message above can
/// name what it's really asking).
fn moves_eq(a: &tta::moves::Move, b: &tta::moves::Move) -> bool {
    a == b
}

/// A short move-shape name for the coverage tally in the test above.
/// Deliberately NOT the reverse of `fixtures::parse_move`'s tag table --
/// that table maps STRINGS to variants; this is Rust matching its own enum,
/// and the two are checked against each other by both existing and by
/// construction (a variant added to `Move` without a matching arm here is a
/// non-exhaustive-match compile error, same as in `moves.rs` itself).
fn tag_of(m: &tta::moves::Move) -> &'static str {
    use tta::moves::Move::*;
    match m {
        Take { .. } => "take",
        Build { .. } => "build",
        Develop { .. } => "develop",
        Upgrade { .. } => "upgrade",
        WonderStep { .. } => "wonder_step",
        Pop => "pop",
        PopFree => "pop_free",
        Revolution { .. } => "revolution",
        PlayLeader { .. } => "play_leader",
        PlayAction { .. } => "play_action",
        Destroy { .. } => "destroy",
        PlayTactic { .. } => "play_tactic",
        CopyTactic { .. } => "copy_tactic",
        Aggression { .. } => "aggression",
        War { .. } => "war",
        OfferPact { .. } => "offer_pact",
        CancelPact { .. } => "cancel_pact",
        PrepareEvent { .. } => "prepare_event",
        Bid { .. } => "bid",
        BidPass => "bid_pass",
        Defend { .. } => "defend",
        DefendDone => "defend_done",
        SendUnit { .. } => "send_unit",
        SendBonus { .. } => "send_bonus",
        SendDiscard { .. } => "send_discard",
        SendDone => "send_done",
        Choose { .. } => "choose",
        Churchill { .. } => "churchill",
        EndTurn => "end_turn",
        PolPass => "pol_pass",
        Resign => "resign",
        RemoveLeaderYellow => "remove_leader_yellow",
        ColumbusColonize { .. } => "columbus_colonize",
        Barbarossa { .. } => "barbarossa",
        BachTheater { .. } => "bach_theater",
    }
}

/// Every ply's `digest` field is present and looks like a blake2b hex digest
/// (`tools/dump_fixtures.py`'s `state_digest`: 64 bytes -> 128 hex chars).
/// This cannot check the digest is CORRECT yet -- that needs `apply` -- but
/// a malformed digest would mean the fixture format itself has drifted from
/// what this reader expects, which is worth catching on its own.
#[test]
fn every_digest_is_well_formed() {
    for (path, records) in load_all() {
        for rec in records {
            if let Record::Ply(p) = rec {
                assert_eq!(
                    p.digest.len(),
                    128,
                    "{}: ply {}: digest {:?} is not 128 hex chars",
                    path.display(),
                    p.ply,
                    p.digest
                );
                assert!(
                    p.digest.chars().all(|c| c.is_ascii_hexdigit()),
                    "{}: ply {}: digest {:?} has non-hex characters",
                    path.display(),
                    p.ply,
                    p.digest
                );
            }
        }
    }
}

/// The last `Ply` record in every fixture carries a `state` snapshot ("always
/// include the final state" -- `tools/dump_fixtures.py`'s header docstring).
#[test]
fn every_fixture_ends_with_a_state_snapshot() {
    for (path, records) in load_all() {
        let last_ply = records.iter().rev().find_map(|r| match r {
            Record::Ply(p) => Some(p),
            _ => None,
        });
        let last_ply = last_ply.unwrap_or_else(|| panic!("{}: no ply records", path.display()));
        assert!(
            last_ply.state.is_some(),
            "{}: last ply ({}) has no state snapshot",
            path.display(),
            last_ply.ply
        );
    }
}

// ===================================================================
// State-replay assertions. `GameState::from_json` (`src/fixtures.rs`) and
// `GameState::diff` (its structural-compare twin) now exist, so these two
// tests replay real games instead of being `#[ignore]`d stubs.
//
// ## Why this is not "digest equality" as originally sketched
//
// The stub this replaced said a divergence "should name one ply and, once
// there is a Rust-side structural differ ..., one field" -- that differ is
// `GameState::diff`, and it is used directly instead of a digest. A digest
// that actually MATCHES `tools/dump_fixtures.py::state_digest` is not
// attainable: that digest hashes Python's full `to_dict()`, including
// `pending`/`queue`/`seeded_by` (`interact.rs`/`events.rs` bookkeeping,
// nothing Rust models -- see `fixtures.rs`'s `GameState::from_json` doc
// comment), so a Rust digest could only agree with Python's by luck. A
// structural diff between two ALREADY-LOADED `GameState`s sidesteps this
// entirely -- it only ever looks at fields Rust represents -- and is more
// useful anyway: it names the field, not just "different".
//
// ## Why this is not "replay the whole file start to finish"
//
// `apply()` does not implement every `Move` yet (`EndTurn`, `OfferPact`,
// the `interact.rs` response moves, `PrepareEvent`, `Aggression` all
// `unimplemented!()` -- see `apply.rs`'s top doc comment), and `legal_moves`
// does not generate every move Python does (pact/aggression/war generation
// and 18 `freeCivilAction` cards' `PlayAction` -- see `legal.rs`'s top doc
// comment, gaps 1 and 3). Both are real, already-documented, in-progress
// gaps, not something to paper over here. So the replay:
//
//   * resyncs from a fresh `GameState::from_json` at every ply that carries
//     a full state snapshot (fixtures are dumped with `--state-every 10`
//     specifically so a chain broken by one of the gaps above is never more
//     than ~10 plies from a fresh anchor -- see `tools/dump_fixtures.py`'s
//     `_FILE_JSON_KW` comment for the OTHER fix that made anchors usable at
//     all: the tableau build order bug),
//   * classifies every `apply()` panic and every `legal_moves` mismatch as
//     either "a documented gap" (named, counted, skipped) or "unexplained"
//     (reported as a real divergence, with file/ply/field detail) -- see
//     `classify_panic` and `classify_legal_mismatch`,
//   * and never silently drops a divergence into a blanket ignore: every
//     ply is accounted for in exactly one bucket of [`ReplayReport`].

/// Substrings that appear ONLY in panic messages `apply.rs` writes for a
/// documented, already-reported gap (its top doc comment, and the doc
/// comments on `h_play_action`/`one_time_culture`/`apply_free_civil_move`).
/// Every one of `apply.rs`'s `unimplemented!()` call sites was read while
/// writing this list (2026-08-05) and each contains at least one of these
/// phrases; a NEW `unimplemented!()`/`panic!()` added later without one of
/// these phrases is exactly the case this is meant NOT to swallow -- it
/// falls through to "unexplained" and fails the test loudly instead.
const KNOWN_APPLY_GAP_MARKERS: &[&str] = &[
    "not ported",
    "blocked on",
    "interact.rs",
    "events.rs",
    "game.rs",
    "named gap",
    "decision queue",
    // "caller must ensure enough food" -- `h_pop`'s `debug_assert!` -- was
    // listed here until 2026-08-05. It fired because `h_pop` passed a
    // hardcoded `0` for `one_time_food_discount` while Python passed the
    // real `p.one_time_discount`, so Rust priced a population increase 1
    // food above Python's and tripped its own affordability assert
    // (confirmed then at `3p_seed100.jsonl` ply 206 and `4p_seed133.jsonl`
    // ply 408). `state::OneTimeDiscount` exists now and `h_pop` reads it;
    // the marker fires on zero plies across all 9 fixtures, so it is gone.
    // A marker that never fires is an excuse nobody is checking.
    //
    // "per-player-count" / "not captured by the type layer" -- `apply.rs::
    // h_play_action`'s `unimplemented!()` for `Wave of Nationalism`/
    // `Military Build-Up`/`Endowment for the Arts` -- were listed here until
    // 2026-08-05. `gen_cards.py` now gives `Special::
    // CulturePerCivilizationWithMoreCulture`/`Special::
    // ResourcesForMilitaryUnitsPerStrongerCivilization` a real `[i16; 3]`
    // payload (the `count_table` shape `strongestPlayers`/`weakestPlayers`/
    // `condition` already used) and `h_play_action` applies both, so the
    // panic these two markers matched no longer happens: `apply_known_gap`
    // dropped from 7 to 0 across all 13 fixtures. Both markers fire on zero
    // plies now -- gone, same as the food-discount marker above.
];

fn classify_panic(msg: &str) -> bool {
    KNOWN_APPLY_GAP_MARKERS.iter().any(|m| msg.contains(m))
}

// A `BUILD_DISCOUNT_GAP_PLIES` allowlist lived here until 2026-08-05: 12
// verified `(file, ply)` pairs in `4p_seed123.jsonl` where Rust priced
// `Bread and Circuses` (an arena, printed `buildCost` 3) at 3 while Python
// charged 2, because an Age I `buildDiscount` of 1 was active and
// `effects::Stats` had no `build_discount` field to hold it. Both symptoms
// it covered -- `Build{Bread and Circuses}` missing from `legal_moves` at 11
// plies where `res=2` was affordable only at Python's real price, and the
// one overcharge state-diff at ply 439 -- are gone now that `costs.rs`
// applies the pool. Deleted rather than kept "just in case": an allowlist
// that no longer fires is a claim about the engine nobody is checking.

fn panic_message(e: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = e.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = e.downcast_ref::<String>() {
        s.clone()
    } else {
        "<non-string panic payload>".to_string()
    }
}

/// Whether `m` is a move `legal_moves` is DOCUMENTED not to generate yet.
/// Currently nothing -- see the two retired arms below -- but the function
/// (and the `legal_known_gap` counting path in [`classify_legal_mismatch`])
/// stays in place for the next real one, rather than deleting the whole
/// mechanism: `KNOWN_APPLY_GAP_MARKERS` right above this uses the same
/// "keep the plumbing, retire the excuse" shape.
fn is_known_legal_gap_move(m: Move) -> bool {
    match m {
        // Two arms lived here until 2026-08-05, both citing `legal.rs`'s
        // top doc comment "gaps 1 and 3":
        //   * `OfferPact`/`Aggression`/`War` unconditionally, for
        //     "`politics_moves` does not generate pact/aggression/war moves
        //     at all (combat.rs / pact resolution not built)".
        //   * `PlayAction { card }` where `card` carries
        //     `Special::FreeCivilAction`, for "`action_card_playable`
        //     returns `false` unconditionally for the 18 `freeCivilAction`
        //     cards".
        // Both gaps are closed as of today (`legal.rs`'s own top doc
        // comment now marks its gap 2 -- combat/pact resolution --
        // "CLOSED 2026-08-05", and `card_table.rs`'s `FreeCivilActionValue`
        // -- gap 1 -- "fixed 2026-08-05"): `politics_moves` generates
        // `OfferPact`/`Aggression`/`War`, and `action_card_playable` reads
        // `Special::FreeCivilAction` for real, for all six ordered-action
        // kinds. Confirmed empirically too, not just by doc comment:
        // `legal_known_gap=0` across all 9 fixtures (3388 legal-move
        // comparisons) before this edit, meaning neither arm had fired even
        // once -- so removing them changes nothing about the 9 existing
        // fixtures, but stops a REAL divergence in pact/aggression/war
        // legality, or in a freeCivilAction card's playability, for a card
        // no fixture exercised yet from being silently absorbed into
        // "known gap" instead of failing loudly. That risk was concrete,
        // not theoretical: `card_coverage.rs`'s allowlist work the same day
        // found pacts, aggressions and freeCivilAction cards among the real
        // (fillable) coverage gaps this exemption would have sat directly
        // in front of.
        //
        // A third arm lived here until 2026-08-05, for a different reason
        // (excusing `Build`/`Develop`/`Upgrade`/`Pop` whenever
        // `discount_active`, because `costs.rs`/`economy.rs` hardcoded the
        // `one_time_discount` term at 0): `state::OneTimeDiscount` exists
        // now and all three cost paths read it, so that arm is gone too.
        //
        // "An allowlist entry that no longer fires is a claim about the
        // engine nobody is checking" applied three times running now.
        _ => false,
    }
}

/// Fixture's `legal` list (`theirs`) vs Rust's (`ours`): `Some(detail)` if
/// EVERY move in `theirs` but not `ours` is a documented gap (checked via
/// [`is_known_legal_gap_move`]) and removing exactly those moves from
/// `theirs`, IN PLACE, reproduces `ours` exactly (order-preserving --
/// DESIGN.md: move order is part of the contract) -- i.e. `ours` is `theirs`
/// with only known-missing moves filtered out. `None` if the two lists
/// disagree in any other way (extra moves Rust generates that Python
/// doesn't, a reordering, or a missing move that is not one of the
/// documented gaps): that is an unexplained divergence.
fn classify_legal_mismatch(ours: &[Move], theirs: &[Move]) -> Option<String> {
    let mut removed = 0usize;
    let mut ti = theirs.iter();
    for &om in ours {
        loop {
            match ti.next() {
                None => return None,
                Some(&tm) if tm == om => break,
                Some(&tm) if is_known_legal_gap_move(tm) => removed += 1,
                Some(_) => return None,
            }
        }
    }
    for &tm in ti {
        if !is_known_legal_gap_move(tm) {
            return None;
        }
        removed += 1;
    }
    // `is_known_legal_gap_move` currently has no exemptions (2026-08-05 --
    // see its doc comment), so in practice this arm is unreachable: the
    // caller only invokes this function when `ours != theirs`, and with
    // nothing exempt, any real difference returns `None` above before
    // reaching here. Left in place, message kept generic rather than
    // naming specific gaps, for the day `is_known_legal_gap_move` grows a
    // real one again.
    Some(format!(
        "{removed} move(s) present in python's legal list but not rust's, all matching a \
         documented gap (see is_known_legal_gap_move's doc comment for what's currently exempt)"
    ))
}

/// Everything the replay found, bucketed so the two tests below can each
/// assert on their own slice of it without re-walking the fixtures.
#[derive(Default)]
struct ReplayReport {
    /// `legal_moves(state)` matched the fixture's `legal` list exactly.
    legal_ok: usize,
    /// Matched only after removing moves `classify_legal_mismatch` could
    /// attribute to a documented `legal.rs` gap.
    legal_known_gap: usize,
    /// Did not match, and the difference is NOT fully explained by a
    /// documented gap -- one string per occurrence, `file:ply` prefixed.
    legal_divergences: Vec<String>,

    /// `apply()` ran without panicking.
    applied_ok: usize,
    /// `apply()` panicked with a message matching a documented gap
    /// (`classify_panic`) -- `(file, ply, chosen move tag, message)`.
    apply_known_gap: Vec<(String, u32, &'static str, String)>,
    /// `apply()` panicked with a message NOT matching any documented gap --
    /// a real finding.
    apply_divergences: Vec<String>,

    /// A full state snapshot existed after `apply()` and `GameState::diff`
    /// found no differences.
    state_ok: usize,
    /// A full state snapshot existed and `diff` found at least one --
    /// one string per ply (fields joined), `file:ply` prefixed.
    state_divergences: Vec<String>,
    /// A full state snapshot existed but `GameState::from_json` itself
    /// could not load it (e.g. a non-empty `pending`/`queue`/`seeded_by` --
    /// `fixtures.rs`'s `GameState::from_json` doc comment); the replay chain
    /// had to jump past it to the next loadable anchor without checking it.
    state_unloadable: Vec<String>,
}

/// Replay one fixture file: bootstrap from its first state snapshot (always
/// ply 0 -- `tools/dump_fixtures.py`'s `ply % state_every == 0` with
/// `ply == 0`), then walk forward ply by ply, checking `legal_moves` and
/// `apply()` against the recording, resyncing at the next loadable state
/// snapshot whenever the chain breaks (an `apply()` panic, or a snapshot
/// this loader cannot represent).
fn replay_file<'a>(path: &Path, records: &'a [Record], report: &mut ReplayReport) {
    let plies: Vec<&'a Ply> = records
        .iter()
        .filter_map(|r| match r {
            Record::Ply(p) => Some(p),
            _ => None,
        })
        .collect();

    let Some((anchor, mut cur)) = next_anchor(&plies, 0, path, report) else {
        return; // no loadable anchor anywhere in the file -- nothing to replay
    };
    // `plies[anchor].state` is the state AFTER `plies[anchor]`'s own move was
    // applied -- i.e. the state BEFORE the NEXT ply's move -- so replay
    // checking starts one past the anchor, not on it (checking `legal_moves`
    // of "after ply N" against ply N's OWN `legal` list, which was recorded
    // BEFORE ply N's move, is a guaranteed, meaningless mismatch).
    let mut k = anchor + 1;

    while k < plies.len() {
        let ply = plies[k];
        // ---- 1. legal_moves(state) vs the fixture's `legal` list ----
        let ours = legal_moves(&cur);
        if ours.as_slice() == ply.legal.as_slice() {
            report.legal_ok += 1;
        } else if classify_legal_mismatch(ours.as_slice(), &ply.legal).is_some() {
            report.legal_known_gap += 1;
        } else {
            report.legal_divergences.push(format!(
                "{}: ply {} (phase {:?}, decider {}): legal_moves mismatch\n    rust:   {:?}\n    python: {:?}",
                path.display(),
                ply.ply,
                ply.phase,
                ply.decider,
                ours.as_slice(),
                ply.legal
            ));
        }

        // ---- 2. apply(state, chosen) ----
        let chosen = ply.chosen;
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| apply(&mut cur, chosen)));
        match outcome {
            Err(payload) => {
                let msg = panic_message(&*payload);
                if classify_panic(&msg) {
                    report.apply_known_gap.push((path.display().to_string(), ply.ply, tag_of(&chosen), msg));
                } else {
                    report.apply_divergences.push(format!(
                        "{}: ply {} (phase {:?}, decider {}): apply({:?}) panicked: {msg}",
                        path.display(),
                        ply.ply,
                        ply.phase,
                        ply.decider,
                        chosen
                    ));
                }
                // `cur` is unreliable after a caught panic (partial mutation);
                // jump to the next state we can load fresh from the fixture.
                match next_anchor(&plies, k + 1, path, report) {
                    Some((j, s)) => {
                        cur = s;
                        k = j + 1;
                    }
                    None => return,
                }
                continue;
            }
            Ok(()) => report.applied_ok += 1,
        }

        // ---- 3. structural compare against the next full snapshot, if any ----
        if let Some(json) = &ply.state {
            match GameState::from_json(json) {
                Ok(expected) => {
                    let diffs = cur.diff(&expected);
                    if diffs.is_empty() {
                        report.state_ok += 1;
                    } else {
                        report.state_divergences.push(format!(
                            "{}: ply {}: state diverges after apply({:?}):\n    {}",
                            path.display(),
                            ply.ply,
                            chosen,
                            diffs.join("\n    ")
                        ));
                    }
                    // Resync to Python's ground truth regardless of outcome so
                    // one divergence does not cascade into every later ply.
                    cur = expected;
                }
                Err(e) => {
                    report.state_unloadable.push(format!("{}: ply {}: {e}", path.display(), ply.ply));
                    match next_anchor(&plies, k + 1, path, report) {
                        Some((j, s)) => {
                            cur = s;
                            k = j + 1;
                        }
                        None => return,
                    }
                    continue;
                }
            }
        }
        k += 1;
    }
}

/// The next ply at or after `from` whose `state` snapshot loads cleanly,
/// as `(index into plies, loaded GameState)`.
/// Plies skipped along the way that DID carry a snapshot but failed to load
/// are recorded in `report.state_unloadable`; plies with no snapshot at all
/// are silently skipped (nothing to report -- they simply were not a
/// candidate anchor).
fn next_anchor<'a>(
    plies: &[&'a Ply],
    from: usize,
    path: &Path,
    report: &mut ReplayReport,
) -> Option<(usize, GameState)> {
    for (j, p) in plies.iter().enumerate().skip(from) {
        if let Some(json) = &p.state {
            match GameState::from_json(json) {
                Ok(s) => return Some((j, s)),
                Err(e) => {
                    report.state_unloadable.push(format!("{}: ply {}: {e}", path.display(), p.ply));
                }
            }
        }
    }
    None
}

fn run_replay() -> ReplayReport {
    let mut report = ReplayReport::default();
    for (path, records) in load_all() {
        replay_file(&path, &records, &mut report);
    }
    eprintln!(
        "differential replay: legal_ok={} legal_known_gap={} legal_divergences={} | \
         applied_ok={} apply_known_gap={} apply_divergences={} | \
         state_ok={} state_divergences={} state_unloadable={}",
        report.legal_ok,
        report.legal_known_gap,
        report.legal_divergences.len(),
        report.applied_ok,
        report.apply_known_gap.len(),
        report.apply_divergences.len(),
        report.state_ok,
        report.state_divergences.len(),
        report.state_unloadable.len(),
    );
    if !report.state_unloadable.is_empty() {
        let pending = report.state_unloadable.iter().filter(|s| s.contains("`pending`")).count();
        let queue = report.state_unloadable.iter().filter(|s| s.contains("`queue`")).count();
        let other: Vec<&String> = report
            .state_unloadable
            .iter()
            .filter(|s| !s.contains("`pending`") && !s.contains("`queue`"))
            .collect();
        eprintln!(
            "  state_unloadable breakdown: pending={pending} queue={queue} other={}",
            other.len()
        );
        for s in other.iter().take(5) {
            eprintln!("    other: {s}");
        }
    }
    report
}

/// Rust's `legal_moves(state)` must produce the SAME moves in the SAME order
/// as the Python fixture's `legal` list, for every ply this replay reaches --
/// except plies whose ENTIRE difference is explained by a documented
/// `legal.rs` gap (`classify_legal_mismatch`), which are counted, not
/// asserted on. Move ordering is part of the contract (DESIGN.md): the bots
/// break ties by index, so a reordered list silently changes play.
#[test]
fn legal_moves_match_python_order() {
    let report = run_replay();
    assert!(
        !report.legal_divergences.is_empty() || report.legal_ok > 0,
        "replay reached no plies at all -- something upstream is broken, not just gapped"
    );
    assert!(
        report.legal_divergences.is_empty(),
        "{} unexplained legal_moves divergence(s):\n{}",
        report.legal_divergences.len(),
        report.legal_divergences.join("\n\n")
    );
}

/// Rust's `apply(state, chosen)` must produce a state structurally identical
/// (`GameState::diff`) to Python's, at every ply that carries a full state
/// snapshot and that `apply()` did not panic on for a documented reason.
/// Also asserts no `apply()` call panicked for an UNDOCUMENTED reason -- see
/// `classify_panic`.
#[test]
fn apply_matches_python_state_stream() {
    let report = run_replay();
    assert!(
        report.state_ok > 0,
        "replay never reached a state snapshot after a successful apply() -- \
         something upstream is broken, not just gapped"
    );
    assert!(
        report.apply_divergences.is_empty(),
        "{} unexplained apply() panic(s):\n{}",
        report.apply_divergences.len(),
        report.apply_divergences.join("\n\n")
    );
    assert!(
        report.state_divergences.is_empty(),
        "{} state divergence(s) after apply():\n{}",
        report.state_divergences.len(),
        report.state_divergences.join("\n\n")
    );
}

/// Acceptance case for the `buildDiscount` gap that `BUILD_DISCOUNT_GAP_PLIES`
/// used to excuse (`costs.rs` charged printed cost, ignoring the discount
/// pool entirely, until it read it): `Bread and Circuses` (arena, Age I,
/// printed build cost 3) at a ply where Masonry's Age I `buildDiscount` of 1
/// is in play.
///
/// Re-pinned 2026-08-05 against `4p_seed1.jsonl` ply 520 player 2 -- the
/// fixtures were regenerated (RNG-derivation fix, `game.rs`'s KNOWN GAP 2),
/// which replays every game differently from ply 1 on, so `4p_seed123.jsonl`
/// ply 414 decider 0 (the previous pin) no longer has Masonry in play at all.
/// The live Python engine (`engine.effects.build_cost` on the same dumped
/// state, checked directly) says 1, not the naive `3 - buildDiscount = 2`:
/// this player's `one_time_discount` stacks an extra resource off arenas on
/// top of Masonry's, which is real game state this test does not need to
/// name to pin -- it just needs Rust to agree with Python on the SAME dumped
/// state, which is the whole point of the acceptance case.
#[test]
fn bread_and_circuses_is_discounted_by_the_build_discount_pool() {
    let path = fixtures_dir().join("4p_seed1.jsonl");
    let records = fixtures::read_fixture_file(&path).unwrap_or_else(|e| panic!("{e}"));
    let json = records
        .iter()
        .find_map(|r| match r {
            Record::Ply(p) if p.ply == 520 => p.state.as_ref(),
            _ => None,
        })
        .expect("ply 520 carries a state snapshot");
    let state = GameState::from_json(json).unwrap_or_else(|e| panic!("{e}"));
    let card = CardId::by_name("Bread and Circuses").unwrap();
    let p = &state.players[2];
    assert_eq!(
        tta::effects::state_stats(&state, p).build_discount[1],
        1,
        "Masonry's Age I entry"
    );
    assert_eq!(tta::costs::build_cost_for(&state, p, card), Some(1));
}
