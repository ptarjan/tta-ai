//! The coordinate registry: docs/OPEN_ITEMS.md item 5.
//!
//! Python's `tests/test_coordinate_registry.py` asserted, in BOTH
//! directions, that every weight had a live reader and every reader had a
//! declared weight, with a `KNOWN_DEAD` allow-list ratchet for the
//! exceptions. That test died with the Python engine; this file mirrors its
//! SHAPE rather than inventing a new design, adapted to what a fieldless
//! Rust enum already buys back for free (see "Direction 2" below) and to
//! what it cannot check by construction (see "Direction 1").
//!
//! ## Direction 2: a reader with no declared key
//!
//! This is a **compile error** in this port, not a runtime check --
//! `weights.rs`'s own top doc comment already establishes why:
//! [`WeightKey`] is a fieldless enum, so `WeightKey::Typo` anywhere in this
//! crate fails `cargo build` before a test could ever run, which is strictly
//! stronger than a test that only fails `cargo test`. The one place a bare
//! STRING names a weight without that protection is I/O -- a champion JSON
//! read off disk -- and that boundary is guarded elsewhere, not by this
//! file: [`super::eval::load_weights`] makes an unknown name a loud `Err`
//! rather than a silently-ignored dict entry (see that function's own doc
//! comment), and [`super::weights::RETIRED_KEYS`] is the named exception for
//! a name that is expected to be unknown. `tests::
//! a_weight_name_outside_the_table_has_nowhere_to_land` below pins the one
//! corner of this direction this file DOES own: [`WeightKey::by_name`]
//! itself must say "no such key" for a typo and for a retired name alike,
//! since it is the single gate both are caught at.
//!
//! ## Direction 1: a declared key with no live reader
//!
//! Two independent checks, because a Rust enum buys nothing here the way it
//! did for direction 2 -- `WeightKey::ALL` happily lists a variant nothing
//! ever reads, and [`super::eval::evaluate`]'s own generic `for &k in
//! WeightKey::ALL { ... w.get(k) ... }` loop reads EVERY key uniformly, so
//! "was `Weights::get` ever called with this key" is true of all 133 by
//! construction and would catch nothing. Both checks below deliberately look
//! for something narrower and more specific than that:
//!
//! * [`tests::every_weight_key_is_named_by_production_source_outside_its_own_declaration`]
//!   -- a source-text scan (docs/OPEN_ITEMS.md item 5's option (a)): every
//!   [`WeightKey`] variant must appear as a literal `WeightKey::<Variant>`
//!   somewhere in this crate's production source OUTSIDE `weights.rs` (the
//!   declaration table itself, where every variant trivially appears once).
//!   Strong for exactly the case a generic loop is blind to: a coordinate
//!   named nowhere a human wrote `WeightKey::ThatSpecificKey`, which is what
//!   "declared and never priced by name" looks like in source.
//! * [`tests::every_weight_key_features_writes_actually_goes_nonzero_on_some_sampled_position`]
//!   -- runtime instrumentation (option (b)): drives real self-play games
//!   forward and checks that every [`WeightKey`] [`super::features::
//!   features`] is coded to `Features::set` actually reads back nonzero on
//!   at least one sampled position. This is the direction that would have
//!   caught a `features()`-side version of docs/OPEN_ITEMS.md item 1's
//!   `gov_action_cost` bug automatically: a coordinate can have a literal
//!   `f.set(WeightKey::X, ...)` call site (satisfying the source scan above)
//!   and still be dead in every real game if the formula feeding it never
//!   evaluates non-zero -- exactly `docs/OPEN_ITEMS.md`'s own `wonder_
//!   stages_per_action` precedent, cited in `features.rs`'s top doc comment.
//!
//! Both checks carry a `KNOWN_DEAD`-shaped ratchet for their real, honest
//! exceptions ([`PHASE_SUFFIXED_NO_LITERAL_READER`], [`NOT_SET_BY_FEATURES`]):
//! every entry names WHY, and both tests assert the exception is still
//! genuinely needed (not merely absent from the corpus) every run -- if a
//! future edit makes an excused key start showing up where its exemption
//! says it cannot, the assertion flips and the entry must come out, exactly
//! the "ratchet, not a one-way escape hatch" property Python's `KNOWN_DEAD`
//! had.

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::path::{Path, PathBuf};

    use crate::bots::weighted::features::features;
    use crate::bots::weighted::weights::{WeightKey, RETIRED_KEYS};
    use crate::{game, legal, moves::Move};

    // ---------------------------------------------------- source scanning
    //
    // Shared by both direction-1 checks below: walk every `.rs` file this
    // crate ships, keeping only the PRODUCTION half of each one. This
    // repo's own convention -- checked nowhere else, so stated here plainly
    // -- is that a file's `#[cfg(test)] mod ...` block(s) sit at the TAIL,
    // never interleaved with more production code after; every file this
    // scan actually touches (`weights.rs`, `features.rs`, `eval.rs`,
    // `cards.rs`, `rivals.rs`, `row.rs`, `events.rs`, `horizon.rs`,
    // `lib.rs`, ...) follows it. That makes "drop everything from the
    // first `#[cfg(test)]` onward" a safe textual proxy for "production
    // code only" without parsing Rust -- a test-only literal mention (like
    // a `WeightKey::WorkersEarly` inside an `eval.rs` test, which exists
    // today) must not count as a reader, or this scan would rate a key
    // "live" because a TEST happened to name it, not because anything
    // reads it while actually evaluating a board.

    fn crate_src_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
    }

    fn collect_rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
        let entries = std::fs::read_dir(dir).unwrap_or_else(|e| panic!("reading {}: {e}", dir.display()));
        for entry in entries {
            let path = entry.unwrap_or_else(|e| panic!("reading {}: {e}", dir.display())).path();
            if path.is_dir() {
                collect_rs_files(&path, out);
            } else if path.extension().is_some_and(|e| e == "rs") {
                out.push(path);
            }
        }
    }

    /// Every `.rs` file's PRODUCTION text (see this module's doc comment),
    /// concatenated, skipping `exclude` entirely -- direction 1's first
    /// check passes `weights.rs` (the declaration table, where every
    /// variant trivially appears once) so it never marks its own home a
    /// "reader".
    fn production_source_excluding(exclude: &Path) -> String {
        let mut files = Vec::new();
        collect_rs_files(&crate_src_root(), &mut files);
        let mut buf = String::new();
        for f in files {
            if f == exclude {
                continue;
            }
            let text = std::fs::read_to_string(&f).unwrap_or_else(|e| panic!("reading {}: {e}", f.display()));
            let cut = text.find("#[cfg(test)]").unwrap_or(text.len());
            buf.push_str(&text[..cut]);
            buf.push('\n');
        }
        buf
    }

    /// All whitespace stripped -- used only by the multi-line-call-site
    /// check below (`f.set(\n    WeightKey::X,` must match the same as
    /// `f.set(WeightKey::X,` written on one line); the plain
    /// `WeightKey::<Variant>` scan needs no such normalisation since that
    /// token is never itself broken across a line.
    fn strip_whitespace(s: &str) -> String {
        s.chars().filter(|c| !c.is_whitespace()).collect()
    }

    // ------------------------------------------------- direction 1, check A

    /// Read via [`WeightKey::early`]/[`WeightKey::late`] off a
    /// [`super::weights::PHASE_KEYS`] BASE key (a runtime value, not a
    /// literal identifier) rather than by their own name -- see `weights.
    /// rs`'s top doc comment, "The phase-suffixed keys". `eval::evaluate`'s
    /// `PHASE_KEYS` loop and `rivals::feature_marginal` are the real
    /// readers; `weights::tests::phase_keys_and_the_flat_table_agree`
    /// already keeps this eight-key set itself honest (every name here
    /// really is `<base>_early`/`<base>_late` for a live `PHASE_KEYS`
    /// member), so this list only has to say WHY no literal reader is
    /// expected, not re-derive WHICH eight keys that is.
    const PHASE_SUFFIXED_NO_LITERAL_READER: &[WeightKey] = &[
        WeightKey::WorkersEarly,
        WeightKey::WorkersLate,
        WeightKey::StrengthRelEarly,
        WeightKey::StrengthRelLate,
        WeightKey::TechLevelsEarly,
        WeightKey::TechLevelsLate,
        WeightKey::HandValueEarly,
        WeightKey::HandValueLate,
    ];

    /// Direction 1, source-scan half (docs/OPEN_ITEMS.md item 5, option
    /// (a)): every [`WeightKey`] must be named by a literal
    /// `WeightKey::<Variant>` somewhere in production source outside its
    /// own declaration table, OR be a documented, ratcheted exception.
    #[test]
    fn every_weight_key_is_named_by_production_source_outside_its_own_declaration() {
        let weights_rs = crate_src_root().join("bots/weighted/weights.rs");
        let prod = production_source_excluding(&weights_rs);
        let excused: HashSet<WeightKey> = PHASE_SUFFIXED_NO_LITERAL_READER.iter().copied().collect();

        for &k in WeightKey::ALL {
            let needle = format!("WeightKey::{k:?}");
            let count = prod.matches(&needle).count();
            if excused.contains(&k) {
                // RATCHET: this key is excused because NOTHING names it
                // directly today. If a future edit adds a real
                // `WeightKey::<Variant>` call site for it, this assertion
                // flips and the entry must come out of the exception list
                // above -- an allow-list that only ever grows is not a
                // ratchet, it is the same silent drift this file exists to
                // stop.
                assert_eq!(
                    count, 0,
                    "{}: listed in PHASE_SUFFIXED_NO_LITERAL_READER as reachable only via \
                     .early()/.late() indirection, but production source now names it directly \
                     ({count} occurrence(s)) -- remove it from the exception list",
                    k.name()
                );
            } else {
                assert!(
                    count >= 1,
                    "{}: no `WeightKey::{k:?}` call site anywhere in production source outside \
                     weights.rs's own declaration table -- this is docs/OPEN_ITEMS.md item 5's \
                     bug class, a coordinate declared and never read by name. If this is \
                     deliberate, add it to PHASE_SUFFIXED_NO_LITERAL_READER (or a new exception \
                     list) with a comment saying why, the same way the phase-suffixed keys are \
                     documented above",
                    k.name()
                );
            }
        }
    }

    // ------------------------------------------------- direction 1, check B

    /// A minimal self-play driver, deliberately NOT a third copy of
    /// `tests/suite/common/mod.rs::play_random` (that file's own doc
    /// comment names the exact bug -- two copies of a move-legality filter
    /// drifting apart -- this would reintroduce for the harness's dodge of
    /// still-unported move kinds). `src/*.rs` unit tests cannot reach that
    /// integration-test-only module at all (each side of that boundary
    /// compiles separately), and by 2026-08-05 `apply.rs` has no
    /// `unimplemented!` left to dodge in the first place (see that module's
    /// doc comment) -- the one thing still worth filtering here is
    /// `Move::Resign` itself, which is a fully legal move that would end a
    /// game (and this test's sampling of it) early on purpose, not a
    /// missing module. One rule, stated once, not a second registry of
    /// exclusions to keep in sync with anything.
    struct Rng(u64);
    impl Rng {
        fn below(&mut self, n: usize) -> usize {
            self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut z = self.0;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            ((z ^ (z >> 31)) % n as u64) as usize
        }
    }

    /// Every [`WeightKey`] value [`features::features`] set on some sampled
    /// position, across every player and every sampled turn of every driven
    /// game -- the raw material both direction-1-check-B and the exemption
    /// cross-check below read off.
    fn sample_nonzero_feature_keys() -> HashSet<WeightKey> {
        let mut nonzero = HashSet::new();
        for n in [2u8, 3, 4] {
            for seed in 0u64..60 {
                let mut state = game::new_game(n, seed.wrapping_mul(7919).wrapping_add(u64::from(n)));
                let mut rng = Rng(seed ^ 0x5EED);
                let mut moves = 0usize;
                loop {
                    if state.game_over || moves > game::MOVE_CAP {
                        break;
                    }
                    // Sample every few moves rather than every one: cheap
                    // enough to still cover thousands of real positions
                    // across 180 driven games, without the cost of a full
                    // `features()` call (which itself recomputes `effects::
                    // compute`) after every single move.
                    if moves % 7 == 0 {
                        for idx in 0..n {
                            let f = features(&state, idx, None, None, false);
                            for &k in WeightKey::ALL {
                                if f.get(k) != 0.0 {
                                    nonzero.insert(k);
                                }
                            }
                        }
                    }
                    let legal = legal::legal_moves(&state);
                    let playable: Vec<Move> =
                        legal.as_slice().iter().copied().filter(|&m| !matches!(m, Move::Resign)).collect();
                    if playable.is_empty() {
                        break;
                    }
                    let mv = playable[rng.below(playable.len())];
                    game::step(&mut state, mv);
                    moves += 1;
                }
            }
        }
        nonzero
    }

    /// [`WeightKey`]s [`features::features`] is NOT coded to
    /// `Features::set` at all -- see that module's own top doc comment
    /// ("A `WeightKey` that `features()` never writes ... is simply left at
    /// its zero default here"). Each entry names WHERE the key is actually
    /// priced instead, so the claim is checkable against a real file/
    /// function rather than taken on faith. `tests::
    /// the_features_exemption_table_matches_what_features_rs_actually_writes`
    /// below is the ratchet that keeps this list itself from drifting stale
    /// against `features.rs`'s own `f.set` call sites.
    const NOT_SET_BY_FEATURES: &[(WeightKey, &str)] = &[
        // Multipliers the card-in-hand valuation layer (cards.rs) applies
        // to a static/board yield-table ROW -- "how much to trust this
        // kind of estimate", not a board fact, so there is nothing for
        // `features()` to write.
        (WeightKey::CardBoardCredit, "cards.rs board-credit multiplier"),
        (WeightKey::CardBoardLeader, "cards.rs board-credit multiplier"),
        (WeightKey::CardBoardGovernment, "cards.rs board-credit multiplier"),
        (WeightKey::CardBoardAction, "cards.rs board-credit multiplier"),
        (WeightKey::CardBoardWonder, "cards.rs board-credit multiplier"),
        (WeightKey::CardBoardBonus, "cards.rs board-credit multiplier"),
        (WeightKey::CardRateCredit, "cards.rs static-table rate multiplier"),
        (WeightKey::UnitStrengthCredit, "cards.rs static-table unit multiplier"),
        (WeightKey::UnitTechCredit, "cards.rs unit-tech multiplier"),
        (WeightKey::TechBoardCredit, "cards.rs tech board-credit multiplier"),
        (WeightKey::ActionBoardCredit, "cards.rs action board-credit multiplier"),
        (WeightKey::GovBoardCredit, "cards.rs gov board-credit multiplier"),
        (WeightKey::WonderBoardCredit, "cards.rs wonder board-credit multiplier"),
        (WeightKey::BuildFreshCredit, "cards.rs build-fresh multiplier"),
        (WeightKey::RestrictedResourceCredit, "cards.rs restricted-resource multiplier"),
        (WeightKey::FreeActionCredit, "cards.rs free-action multiplier"),
        (WeightKey::TerritoryCredit, "cards.rs territory multiplier"),
        (WeightKey::BonusCardCredit, "cards.rs bonus-card multiplier"),
        // Identity-aware terms priced through `w` directly inside
        // `eval::evaluate`'s own gated blocks (each skipped outright at
        // its default 0.0 weight) -- not linear in the board, so not
        // folded into the `Features` vector at all. See `eval.rs`'s own
        // doc comment on `evaluate`'s structure.
        (WeightKey::HandPotential, "eval::evaluate gated block (cards::hand_potential)"),
        (WeightKey::WonderPotential, "eval::evaluate gated block (cards::wonder_potential)"),
        (WeightKey::HandMilPotential, "eval::evaluate gated block (cards::hand_mil_potential)"),
        (WeightKey::TacticGain, "eval::evaluate gated block (cards::tactic_terms)"),
        (WeightKey::TacticShort, "eval::evaluate gated block (cards::tactic_terms)"),
        (WeightKey::RivalHandPotential, "eval::evaluate gated block (cards::rival_hand_potential)"),
        (WeightKey::RowUrgency, "eval::evaluate gated block (row::row_pressure)"),
        (WeightKey::RowBargainForgone, "eval::evaluate gated block (row::row_pressure)"),
        (WeightKey::RowLastCopy, "eval::evaluate gated block (row::row_last_copy)"),
        (WeightKey::MyEventThreat, "eval::evaluate gated block (events::my_event_threat)"),
        // Static/board action-card coordinates read via `cards::
        // feature_key` off a `board_yields::Feature`/static `CardYield`
        // triple, never via `Features::set`. `GovActionCost` is
        // docs/OPEN_ITEMS.md item 1: a real reader exists (`cards.rs`'s
        // `government_cost`/`feature_key`), it is just multiplied by a
        // weight nobody has climbed off its 0.0 default yet -- a VALUE bug,
        // not a missing-reader one, which is exactly why this probe does
        // not (and should not) flag it.
        (WeightKey::FreeCivilAction, "cards.rs static/board card valuation"),
        (WeightKey::ResourceDiscount, "cards.rs static/board card valuation"),
        (WeightKey::DefenseBonus, "cards.rs static/board card valuation"),
        (WeightKey::GovActionCost, "cards.rs government valuation (feature_key); docs/OPEN_ITEMS.md item 1"),
        (WeightKey::RestrictedResources, "cards.rs static/board card valuation"),
        (WeightKey::HandSwapExtra, "cards.rs hand-swap valuation"),
        // Rival-row pricing, read directly in row.rs/rivals.rs.
        (WeightKey::RivalDesire, "row.rs/rivals.rs rival-row pricing"),
        (WeightKey::RivalTakeShare, "row.rs/rivals.rs rival-row pricing"),
        // Not board features at all.
        (WeightKey::EndTurnBias, "eval::WeightedBot's search bias, not a board feature"),
        (WeightKey::RateHorizon, "horizon::rate_multiplier reads this directly"),
        // The four PHASE_KEYS' *_early/*_late partners -- blended
        // dynamically off the BASE key's raw feature value (which
        // `features()` DOES set), never written under their own suffixed
        // name. Same eight keys as `PHASE_SUFFIXED_NO_LITERAL_READER`
        // above, for the same underlying reason.
        (WeightKey::WorkersEarly, "phase blend off Workers"),
        (WeightKey::WorkersLate, "phase blend off Workers"),
        (WeightKey::StrengthRelEarly, "phase blend off StrengthRel"),
        (WeightKey::StrengthRelLate, "phase blend off StrengthRel"),
        (WeightKey::TechLevelsEarly, "phase blend off TechLevels"),
        (WeightKey::TechLevelsLate, "phase blend off TechLevels"),
        (WeightKey::HandValueEarly, "phase blend off HandValue"),
        (WeightKey::HandValueLate, "phase blend off HandValue"),
    ];

    /// Direction 1, runtime-instrumentation half (docs/OPEN_ITEMS.md item
    /// 5, option (b)): every [`WeightKey`] `features()` is coded to write
    /// must actually go nonzero on at least one of several thousand sampled
    /// positions from real, varied 2p/3p/4p self-play -- not merely have a
    /// call site (check A above already covers that), but observably
    /// reach a live value when the engine actually plays games.
    #[test]
    fn every_weight_key_features_writes_actually_goes_nonzero_on_some_sampled_position() {
        let nonzero = sample_nonzero_feature_keys();
        let excused: HashSet<WeightKey> = NOT_SET_BY_FEATURES.iter().map(|&(k, _)| k).collect();

        for &k in WeightKey::ALL {
            if excused.contains(&k) {
                continue; // documented in NOT_SET_BY_FEATURES above
            }
            assert!(
                nonzero.contains(&k),
                "{}: features() is coded to `Features::set` this key, but it never went \
                 nonzero on any sampled position across 180 driven 2p/3p/4p self-play games \
                 -- this is docs/OPEN_ITEMS.md item 5's bug class (a coordinate with a writer \
                 that structurally never fires), the same shape as the `wonder_stages_per_\
                 action` gap features.rs's own doc comment cites. If this is a genuinely rare \
                 feature rather than a bug, add it to NOT_SET_BY_FEATURES with a comment \
                 explaining why a random-play corpus this size should not be expected to \
                 reach it, and a pointer to whatever OTHER test proves it live (the way \
                 `wonder_overrun_fires_for_a_constructed_near_completion_shortfall_state` \
                 already does for `wonder_overrun`)",
                k.name()
            );
        }
    }

    /// The ratchet closing the loop on [`NOT_SET_BY_FEATURES`] itself: it
    /// must name EXACTLY the keys `features.rs` has no `f.set(WeightKey::
    /// X, ...)` call site for, in both directions -- checked the same
    /// source-scan way check A checks the whole crate, restricted to one
    /// file. Without this, the exemption table above could silently go
    /// stale in either direction: a key added to `features()` later would
    /// stay wrongly excused (this test's sibling would never flag it
    /// missing, because it is skipped), or a key removed from `features()`
    /// would start failing the sibling test for a reason that has nothing
    /// to do with a real bug.
    #[test]
    fn the_not_set_by_features_exemption_table_matches_what_features_rs_actually_writes() {
        let features_rs = crate_src_root().join("bots/weighted/features.rs");
        let text = std::fs::read_to_string(&features_rs).unwrap_or_else(|e| panic!("reading {}: {e}", features_rs.display()));
        let prod = &text[..text.find("#[cfg(test)]").unwrap_or(text.len())];
        let normalized = strip_whitespace(prod);

        let excused: HashSet<WeightKey> = NOT_SET_BY_FEATURES.iter().map(|&(k, _)| k).collect();
        for &k in WeightKey::ALL {
            let needle = format!("f.set(WeightKey::{k:?},");
            let is_written = normalized.contains(&needle);
            assert_eq!(
                is_written,
                !excused.contains(&k),
                "{}: features.rs {} an `f.set(WeightKey::{k:?}, ...)` call site, which \
                 disagrees with NOT_SET_BY_FEATURES's claim -- update the exemption table \
                 to match reality",
                k.name(),
                if is_written { "now has" } else { "still has no" }
            );
        }
    }

    /// `NOT_SET_BY_FEATURES` has no duplicate entries -- a repeated key
    /// would silently do nothing (the second copy is redundant), which is
    /// exactly the kind of drift a ratchet list must not tolerate quietly.
    #[test]
    fn not_set_by_features_has_no_duplicate_keys() {
        let mut seen = HashSet::new();
        for &(k, _) in NOT_SET_BY_FEATURES {
            assert!(seen.insert(k), "{} listed twice in NOT_SET_BY_FEATURES", k.name());
        }
    }

    // ------------------------------------------------------- direction 2

    /// Direction 2 ("a reader with no declared key") is a compile error for
    /// every REAL call site -- see this module's top doc comment. The one
    /// place a bare string names a weight without that protection is I/O, a
    /// champion JSON on disk, and [`WeightKey::by_name`] is the boundary:
    /// it must say "no such key" for both a plain typo and a deliberately
    /// [`RETIRED_KEYS`] name, since a future `eval::load_weights` reads
    /// through this exact gate to decide which of the two it is looking at.
    #[test]
    fn a_weight_name_outside_the_table_has_nowhere_to_land() {
        assert_eq!(WeightKey::by_name("this_is_not_a_real_weight"), None);
        assert_eq!(WeightKey::by_name(""), None);
        for &name in RETIRED_KEYS {
            assert_eq!(WeightKey::by_name(name), None, "{name}: retired but resolvable again");
        }
    }
}
