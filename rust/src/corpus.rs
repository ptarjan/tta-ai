//! Shared parsing for the BGO human-game corpus (`sources/bgo/`): turning
//! `index.tsv` rows and `journals/<game_id>.tsv` lines into typed values two
//! different binaries need. `rust/src/bin/corpuscensus.rs` (a play-rate
//! census -- counts action classes, never touches `GameState`) and
//! `rust/src/bin/replay.rs` (a game-state reconstruction, drives the real
//! engine) both start from the same journal text and the same closed
//! vocabulary of BGO-generated line shapes, so the parsing lives here once
//! rather than twice. See `docs/HUMAN_PLAY.md` and `docs/REPLAY.md` for what
//! each binary does with it.
//!
//! # Method: journal text is a small fixed set of BGO-generated shapes
//!
//! `text` is English prose with a fixed small vocabulary (BGO
//! template-generates it), so [`classify`] is text classification against a
//! discovered shape set, not a parser for a context-free grammar. The shape
//! set was discovered empirically (normalise player colours/digits/card
//! names out of every line, histogram what's left) before a single match arm
//! was written -- see [`classify`]'s doc for the shapes that resulted.
//!
//! One assumption this file does NOT make, despite being tempting: that
//! column 2 (`player_colour`) is always the line's actor. It almost always
//! is, but not for territory-auction wins ("Grey wins Developed Territory
//! Winning bid is 2" logged on a different player's row than Grey's) or the
//! forced-discard/destroy-choice prompt -- found by diffing text against
//! column 2 while chasing the last few points of parser coverage. [`classify`]
//! reads the actor from `text` itself for exactly this reason; see its body.
//!
//! # Card-name matching: longest known prefix, not per-verb regexes
//!
//! BGO's text never delimits a card name from what follows it (no quotes, no
//! fixed-width field) -- `"Orange elects Isaac Newton Leonardo Da Vinci
//! dies; ..."` is one card name (`Isaac Newton`, the winner) immediately
//! butted against another (`Leonardo Da Vinci`, the leader who just died) with
//! no separator at all. A regex with a fixed stop-word runs into exactly this
//! ambiguity. The fix used everywhere in this file is
//! [`longest_known_card_prefix`]: build the full base-game name dictionary
//! from `tta::CARDS` once, then at any position where a card name is
//! expected, try the longest whitespace-delimited prefix that is a known
//! name and take it -- greedy-longest-match against a closed dictionary has
//! no ambiguity to resolve, because the dictionary is checked, not guessed.
//!
//! # BGO's spelling is not the engine's spelling
//!
//! `tta::CARDS` names come from the official card data (`gen_cards.py`);
//! BGO's journal text comes from BGO's own UI strings, and the two disagree
//! on nine cards -- not expansion cards, mostly not even typos in the
//! corpus, just two independently-authored English strings (or, for "Loss
//! of Sovereignity", one of them actually is a BGO typo) for the same real
//! card. [`ALIASES`] is the fix: each BGO spelling is inserted into the same
//! dictionary, pointing at the same [`CardId`] its engine spelling resolves
//! to. This was found, not assumed -- by two rounds of the same method: an
//! offline prefix-matching scan over every "unmatched" card-shaped string
//! before this file was written (which found the first seven), and
//! `corpuscensus`'s own coverage report against the unclassified-shape
//! residue (which found the last two, both under 30 occurrences). With the
//! aliases in, zero games contain a card name outside the 2015 base game --
//! the `edition_verified_by` column's promise holds, checked, not just
//! trusted.

use std::collections::HashMap;
use std::fs;

use crate::{Age, CardId, CardType, CARDS};

// ---------------------------------------------------------------------
// Card name dictionary
// ---------------------------------------------------------------------

/// BGO spelling -> the engine's spelling for the same card. See the module
/// doc's "BGO's spelling is not the engine's spelling" section for how each
/// of these was found (not assumed).
pub const ALIASES: &[(&str, &str)] = &[
    ("Leonardo Da Vinci", "Leonardo da Vinci"),
    ("Maximillien Robespierre", "Maximilien Robespierre"),
    ("Charles Chaplin", "Charlie Chaplin"),
    ("Johannes Sebastian Bach", "J. S. Bach"),
    ("Warrior", "Warriors"),
    ("Ocean Liner", "Ocean Liners"),
    ("Stockpile", "Stock Pile"),
    ("Bread & Circuses", "Bread and Circuses"),
    // A BGO typo, not a spelling choice: the card is "Loss of Sovereignty".
    ("Loss of Sovereignity", "Loss of Sovereignty"),
    // BGO's own display name for this Age A event differs from the engine's
    // (which follows the underlying `data/*.json` spelling): the printed
    // flavour text ("Immediately, each civilization may either: increase
    // its population; or build a farm, mine or urban building; or develop
    // a technology. It costs 1 [resource] less than usual.") is verbatim
    // "Development of Civil Life"'s card text (`state.rs`'s own doc comment
    // on `OneTimeDiscount` quotes it) -- confirmed by text match, not
    // guessed. Missing this alias meant `current_event_name`'s lookup
    // silently failed every time this event (471/1011 games in the corpus)
    // fired, dropping it from `replay.rs`'s `event_reveals` prescan FIFO
    // entirely and shifting every LATER event in that game by one slot --
    // the single largest confirmed cause of the "event-timing collapse"
    // mismatch category in `docs/REPLAY.md` (found by testing against a
    // real 2p game whose FIRST Age I event was this one).
    ("Development of Civilization", "Development of Civil Life"),
];

/// The longest a real base-game card name spans once split on ASCII spaces
/// (`"Development of Trade Routes"`, `"Promise of Military Protection"` are
/// four; nothing in `tta::CARDS` or [`ALIASES`] is longer). Kept a little
/// generous rather than exactly tight -- one extra failed HashMap lookup per
/// miss costs nothing measurable at this corpus size.
pub const MAX_NAME_WORDS: usize = 6;

/// Every string BGO's journal text might print for a card, mapped to the
/// [`CardId`] the engine knows it by. Built once by the caller and threaded
/// through as a plain reference -- not a global, per this project's rule
/// against process-global mutable state (`DESIGN.md` rule).
///
/// Populated from three sources per card: the disambiguated `name` (e.g.
/// `"Aggression: Plunder (II)"`), the `base_name` BGO actually prints (e.g.
/// `"Plunder"` after stripping the `"Aggression: "` prefix and any `" (I)"`/
/// `" (II)"`/`" (III)"` age suffix -- BGO's Territory and Aggression cards
/// print only the family name, never the age), and [`ALIASES`] for the cards
/// where BGO's own spelling differs from the engine's.
pub fn build_card_index() -> HashMap<&'static str, CardId> {
    let mut index: HashMap<&'static str, CardId> = HashMap::new();
    for i in 0..CARDS.len() {
        let id = CardId(i as u16);
        let card = id.get();
        index.entry(card.name).or_insert(id);
        index.entry(card.base_name).or_insert(id);
        if let Some(short) = card.base_name.strip_prefix("Aggression: ") {
            index.entry(short).or_insert(id);
        }
        if let Some(unsuffixed) = strip_age_suffix(card.base_name) {
            index.entry(unsuffixed).or_insert(id);
        }
        if let Some(unsuffixed) = strip_age_suffix(card.name) {
            index.entry(unsuffixed).or_insert(id);
        }
    }
    for (bgo_spelling, engine_spelling) in ALIASES {
        let id = *index
            .get(engine_spelling)
            .unwrap_or_else(|| panic!("alias target {engine_spelling:?} not found in CARDS"));
        index.entry(bgo_spelling).or_insert(id);
    }
    index
}

/// Strips a trailing `" (A)"`/`" (I)"`/`" (II)"`/`" (III)"`/`" (IV)"` age tag,
/// the way `card_table.rs` disambiguates same-named cards that recur across
/// ages. BGO's journal text never prints the tag (it prints the bare family
/// name, e.g. `"Vast Territory"` regardless of which age's copy was drawn),
/// so the index needs the untagged form to match against it.
pub fn strip_age_suffix(name: &str) -> Option<&str> {
    for tag in [" (A)", " (I)", " (II)", " (III)", " (IV)"] {
        if let Some(stripped) = name.strip_suffix(tag) {
            return Some(stripped);
        }
    }
    None
}

/// Byte offset just past the `n`-th (1-indexed) space-delimited word of `s`,
/// or `None` if `s` has fewer than `n` words. Assumes single-space-delimited
/// text, true of every BGO journal line sampled.
pub fn nth_word_end(s: &str, n: usize) -> Option<usize> {
    let mut pos = 0usize;
    for (i, word) in s.split(' ').enumerate() {
        pos += word.len();
        if i + 1 == n {
            return Some(pos);
        }
        pos += 1; // the space
    }
    None
}

/// Finds the longest whitespace-delimited prefix of `s` that is a known card
/// name, trying [`MAX_NAME_WORDS`] words down to one. Returns the card and
/// the remainder of `s` starting right after the matched span (so a
/// glued-on `;` like `"...Pyramids; Orange spends..."` lands at the front of
/// the remainder, not swallowed into the match). See the module doc's "Card
/// name matching" section for why longest-prefix-against-a-closed-dictionary
/// replaces per-verb regexes here.
pub fn longest_known_card_prefix<'a>(
    index: &HashMap<&'static str, CardId>,
    s: &'a str,
) -> Option<(CardId, &'a str)> {
    for words in (1..=MAX_NAME_WORDS).rev() {
        let Some(end) = nth_word_end(s, words) else {
            continue;
        };
        let raw = &s[..end];
        let trimmed = raw.trim_end_matches([';', ',', '.']);
        if let Some(&id) = index.get(trimmed) {
            return Some((id, &s[end..]));
        }
    }
    None
}

/// Nine card families -- the six free-civil-action cards this bore, plus
/// Territories, Aggressions and Military Bonuses -- print the SAME name once
/// per age with a stronger effect each time (`Rich Land`'s printed
/// `resourceDiscount` is 1/2/3/4 across its four copies). BGO's journal text
/// never carries an age tag (`"takes Rich Land in hand"` reads identically
/// for all four), so [`build_card_index`]'s bare-name key necessarily picks
/// ONE of them -- whichever age happens to iterate first when the map is
/// built (`HashMap::or_insert`), whether or not that is the copy actually in
/// play.
///
/// The journal DOES imply a bound, though: nothing newer than the civil
/// deck's current age can be in anyone's hand or row. Given any card in the
/// same name family as `named` (typically [`build_card_index`]'s arbitrary
/// pick), returns the sibling at the highest age not exceeding
/// `at_or_below` -- the closest available approximation of "the copy BGO is
/// actually dealing right now" -- or `named` itself if every sibling is
/// somehow newer (shouldn't happen for a real journal line, but this is a
/// total function, not a partial one).
///
/// Found by replaying a real 2p game (`7523044`): this binary priced
/// "Orange builds Alchemy using Urban Growth" one resource too high because
/// it had resolved "Urban Growth" to its Age A copy (`resourceDiscount: 1`)
/// while the copy actually in Orange's hand -- taken from the Age I civil
/// row, `docs/REPLAY.md`'s "SIMULATED" row content notwithstanding, a fact
/// this function did not yet exist to use -- was the Age I copy
/// (`resourceDiscount: 2`). The same misattribution then hid the Age I
/// copy's WIDER free-civil-action option set from `legal::free_action_moves`,
/// which was being asked about the wrong card's discount entirely.
pub fn best_age_sibling(named: CardId, at_or_below: Age) -> CardId {
    let base = named.get().base_name;
    let mut best = named;
    for i in 0..CARDS.len() {
        let id = CardId(i as u16);
        let c = id.get();
        if c.base_name != base || c.age as u8 > at_or_below as u8 {
            continue;
        }
        if c.age as u8 > best.get().age as u8 || best.get().age as u8 > at_or_below as u8 {
            best = id;
        }
    }
    best
}

/// Every card sharing `named`'s family (same `base_name`, `named` itself
/// included), in no particular order. The full candidate set
/// [`best_age_sibling`] picks its single best-guess answer from -- for a
/// caller that has a STRONGER signal than "not newer than the current age"
/// (e.g. a `"using <Card>"` line's own observed payment, which pins the
/// discount -- and so the age -- exactly, `replay_common.rs`'s
/// `resolve_named_card_by_effect`), searching every sibling for one whose
/// printed numbers match the observed line beats guessing.
pub fn family_siblings(named: CardId) -> Vec<CardId> {
    let base = named.get().base_name;
    (0..CARDS.len()).map(|i| CardId(i as u16)).filter(|id| id.get().base_name == base).collect()
}

// ---------------------------------------------------------------------
// Journal domain types
// ---------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Color {
    Orange,
    Purple,
    Green,
    Grey,
}

impl Color {
    pub fn parse(s: &str) -> Option<Color> {
        match s {
            "Orange" => Some(Color::Orange),
            "Purple" => Some(Color::Purple),
            "Green" => Some(Color::Green),
            "Grey" => Some(Color::Grey),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Color::Orange => "Orange",
            Color::Purple => "Purple",
            Color::Green => "Green",
            Color::Grey => "Grey",
        }
    }

    /// This colour's seat index, assuming BGO's fixed turn-order convention
    /// Orange, Purple, Green, Grey -- confirmed against real journals for 2p,
    /// 3p and 4p games (`docs/REPLAY.md`'s "seating" section): whichever
    /// colours are in a game, they always occupy this prefix of the fixed
    /// order, Orange always seat 0.
    pub fn seat(self) -> u8 {
        match self {
            Color::Orange => 0,
            Color::Purple => 1,
            Color::Green => 2,
            Color::Grey => 3,
        }
    }

    /// The inverse of [`Color::seat`]: the colour BGO's fixed turn-order
    /// convention assigns to seat `s`. `None` for any `s` outside the four
    /// real seats (mirrors [`Color::parse`]'s own `None`-on-unrecognised
    /// convention).
    pub fn from_seat(s: u8) -> Option<Color> {
        match s {
            0 => Some(Color::Orange),
            1 => Some(Color::Purple),
            2 => Some(Color::Green),
            3 => Some(Color::Grey),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash, PartialOrd, Ord)]
pub enum Tier {
    Prince,
    King,
    Warlord,
    Emperor,
}

impl Tier {
    pub fn parse(s: &str) -> Result<Tier, String> {
        match s {
            "Prince" => Ok(Tier::Prince),
            "King" => Ok(Tier::King),
            "Warlord" => Ok(Tier::Warlord),
            "Emperor" => Ok(Tier::Emperor),
            other => Err(format!("unknown BGO level tier {other:?}")),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Tier::Prince => "Prince",
            Tier::King => "King",
            Tier::Warlord => "Warlord",
            Tier::Emperor => "Emperor",
        }
    }
}

/// One row of `sources/bgo/index.tsv`.
pub struct GameMeta {
    pub id: String,
    pub players: u8,
    pub tier: Tier,
    pub rounds: u32,
    pub reached_age_iv: bool,
    pub scores: Vec<i32>,
    /// The `results` column's player names, in the SAME order as [`scores`]
    /// -- `index.tsv`'s own order, which is not necessarily seating order.
    /// `replay.rs` uses this only for display; nothing here maps a name to a
    /// [`Color`] (the journal never prints player names, only colours).
    pub names: Vec<String>,
}

pub fn parse_index(path: &str) -> Result<Vec<GameMeta>, String> {
    let text = fs::read_to_string(path).map_err(|e| format!("reading {path}: {e}"))?;
    let mut lines = text.lines();
    let header = lines.next().ok_or_else(|| "empty index.tsv".to_string())?;
    let cols: Vec<&str> = header.split('\t').collect();
    let col_index = |name: &str| -> Result<usize, String> {
        cols.iter()
            .position(|c| *c == name)
            .ok_or_else(|| format!("index.tsv missing column {name:?}"))
    };
    let c_id = col_index("game_id")?;
    let c_players = col_index("players")?;
    let c_level = col_index("level")?;
    let c_final_age = col_index("final_age")?;
    let c_rounds = col_index("rounds")?;
    let c_results = col_index("results")?;

    let mut games = Vec::new();
    for (lineno, line) in lines.enumerate() {
        if line.is_empty() {
            continue;
        }
        let fields: Vec<&str> = line.split('\t').collect();
        let want = cols.len();
        if fields.len() != want {
            return Err(format!(
                "index.tsv row {} has {} fields, want {want}",
                lineno + 2,
                fields.len()
            ));
        }
        let players: u8 = fields[c_players]
            .parse()
            .map_err(|e| format!("index.tsv row {}: bad players field: {e}", lineno + 2))?;
        let rounds: u32 = fields[c_rounds]
            .parse()
            .map_err(|e| format!("index.tsv row {}: bad rounds field: {e}", lineno + 2))?;
        let tier = Tier::parse(fields[c_level])?;
        let reached_age_iv = fields[c_final_age] == "Age IV";
        let mut scores = Vec::new();
        let mut names = Vec::new();
        for entry in fields[c_results].split('|') {
            let (name, score_str) = entry
                .rsplit_once(':')
                .ok_or_else(|| format!("index.tsv row {}: bad results entry {entry:?}", lineno + 2))?;
            let score: i32 = score_str
                .parse()
                .map_err(|e| format!("index.tsv row {}: bad score {score_str:?}: {e}", lineno + 2))?;
            scores.push(score);
            names.push(name.to_string());
        }
        games.push(GameMeta {
            id: fields[c_id].to_string(),
            players,
            tier,
            rounds,
            reached_age_iv,
            scores,
            names,
        });
    }
    Ok(games)
}

// ---------------------------------------------------------------------
// Line classification
// ---------------------------------------------------------------------

/// The action classes a journal line can classify to. Every variant is
/// either one of the classes `corpuscensus` was asked for by name, or one
/// that fell out of parsing those for free (once a line's leading colour is
/// found, recognising the verb after it costs the same whether or not the
/// result is tallied). Deliberately NOT included: a `PrepareEvent` variant --
/// see the module doc for why that class does not exist in this text.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash, PartialOrd, Ord)]
pub enum ActionClass {
    TakeCard,
    BuildBuilding,
    BuildUnit,
    BuildWonderStage,
    IncreasePopulation,
    UpgradeUnit,
    UpgradeProduction,
    DevelopTechnology,
    ElectLeader,
    ChangeGovernment,
    PlayTactic,
    DeclareWar,
    WinWar,
    PlayAggression,
    ProposePact,
    AcceptPact,
    Colonize,
    Discard,
    Bid,
    WinAuction,
    Destroy,
    Disband,
    Pass,
    PlayEvent,
    PlayActionCard,
    PutBack,
    EndTurn,
    /// Alexander the Great's leader ability exercised as a political action
    /// (`Move::RemoveLeaderYellow`) -- BGO logs it as flavour text with no
    /// leading actor colour ("Alexander dies after building his great
    /// Empire <Color> gets 1 yellow token"), the actor named only in the
    /// trailing consequence clause. See `classify`'s own comment on this
    /// line and `replay_common::replay_game`'s special-cased dispatch
    /// (mirrors how `EndTurn` lines, the other no-leading-colour shape, are
    /// handled).
    RemoveLeaderYellow,
    /// Christopher Columbus's leader ability exercised as a political action
    /// (`Move::ColumbusColonize`): "As a political action, you may remove
    /// Columbus from play to colonize a territory in your hand without
    /// sacrificing any units" (`bga_throughtheages_material.inc.php`). BGO
    /// logs it as `"Christopher Columbus discovers <Age> / <Territory>"` --
    /// no leading actor colour (previously silently swallowed as
    /// `Bookkeeping` by `classify`'s "known card name leads the line" catch-
    /// all, since "Christopher Columbus" is itself a known card name) AND,
    /// unlike `RemoveLeaderYellow`'s Alexander line, no trailing consequence
    /// clause naming the actor either -- the ONLY line in the whole corpus
    /// that needs column 2 (`Line::color`) read at all. Found chasing the
    /// `IllegalMove: Pop` bucket: dropping this move silently skipped the
    /// colonized territory's `effects.yellow_tokens`/`immediate_effects`
    /// grants, drifting `pop_cost` for the rest of the game (`docs/
    /// REPLAY.md`).
    ColumbusColonize,
    /// Frederick Barbarossa's leader ability (`Move::Barbarossa`): a free
    /// population increase immediately followed by building the named unit
    /// with it, for 1 military action total. BGO logs it as flavour text
    /// with no leading actor colour ("Barbarossa enlists a <Unit>; <Color>
    /// spends N food[; <Color> loses N military resource][; <Color> spends
    /// M resource(s)]"), the actor named only in the trailing clauses --
    /// same no-leading-colour shape as `RemoveLeaderYellow`, handled the
    /// same way in `replay_common::replay_game`'s special-cased dispatch.
    /// Previously treated as pure `Bookkeeping` and silently dropped:
    /// discarding both the yellow-bank spend and the unit build, which then
    /// drifts every one of `yellow_bank`/`workers_free`/`resources`/`food`
    /// for the rest of the game -- found chasing the Build/Upgrade/
    /// WonderStep cost-mismatch cluster (`docs/REPLAY.md`).
    Barbarossa,
    /// J. S. Bach's leader ability (`Move::BachTheater`) -- see `classify`'s
    /// own comment on the `"Johannes Sebastian Bachupgrades "` line for why
    /// this used to be (wrongly) `Bookkeeping`.
    BachTheater,
}

impl ActionClass {
    /// All variants, for a stable table iteration order and for the
    /// exhaustiveness check in the test module.
    pub const ALL: &'static [ActionClass] = &[
        ActionClass::TakeCard,
        ActionClass::BuildBuilding,
        ActionClass::BuildUnit,
        ActionClass::BuildWonderStage,
        ActionClass::IncreasePopulation,
        ActionClass::UpgradeUnit,
        ActionClass::UpgradeProduction,
        ActionClass::DevelopTechnology,
        ActionClass::ElectLeader,
        ActionClass::ChangeGovernment,
        ActionClass::PlayTactic,
        ActionClass::DeclareWar,
        ActionClass::WinWar,
        ActionClass::PlayAggression,
        ActionClass::ProposePact,
        ActionClass::AcceptPact,
        ActionClass::Colonize,
        ActionClass::Discard,
        ActionClass::Bid,
        ActionClass::WinAuction,
        ActionClass::Destroy,
        ActionClass::Disband,
        ActionClass::Pass,
        ActionClass::PlayEvent,
        ActionClass::PlayActionCard,
        ActionClass::PutBack,
        ActionClass::EndTurn,
        ActionClass::RemoveLeaderYellow,
        ActionClass::ColumbusColonize,
        ActionClass::Barbarossa,
        ActionClass::BachTheater,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ActionClass::TakeCard => "take card from row",
            ActionClass::BuildBuilding => "build building",
            ActionClass::BuildUnit => "build unit",
            ActionClass::BuildWonderStage => "build wonder stage",
            ActionClass::IncreasePopulation => "increase population",
            ActionClass::UpgradeUnit => "upgrade unit",
            ActionClass::UpgradeProduction => "upgrade production (farm/mine)",
            ActionClass::DevelopTechnology => "develop technology",
            ActionClass::ElectLeader => "elect leader",
            ActionClass::ChangeGovernment => "change government",
            ActionClass::PlayTactic => "play tactic",
            ActionClass::DeclareWar => "declare war",
            ActionClass::WinWar => "win war",
            ActionClass::PlayAggression => "play aggression",
            ActionClass::ProposePact => "propose pact",
            ActionClass::AcceptPact => "accept pact",
            ActionClass::Colonize => "colonize",
            ActionClass::Discard => "discard",
            ActionClass::Bid => "bid",
            ActionClass::WinAuction => "win auction",
            ActionClass::Destroy => "destroy",
            ActionClass::Disband => "disband",
            ActionClass::Pass => "pass",
            ActionClass::PlayEvent => "play event",
            ActionClass::PlayActionCard => "play action card",
            ActionClass::PutBack => "put card back (take-back upper bound)",
            ActionClass::EndTurn => "end turn (player-turn denominator)",
            ActionClass::RemoveLeaderYellow => "Alexander the Great: remove for a yellow token",
            ActionClass::ColumbusColonize => "Christopher Columbus: remove to colonize a territory",
            ActionClass::Barbarossa => "Frederick Barbarossa: free population increase + build",
            ActionClass::BachTheater => "J. S. Bach: Temple/Library to Theater",
        }
    }
}

/// The result of classifying one journal line: which action class it is,
/// and the card involved if the class carries one and matching found it.
/// `card` is `None` both for classes that never carry a card (`Pass`,
/// `Discard`, ...) and for the rare case a card-carrying class matched its
/// verb but [`longest_known_card_prefix`] found nothing after it -- callers
/// that need to tell those apart use [`card_expected`].
pub struct Classified {
    pub class: ActionClass,
    pub card: Option<CardId>,
}

pub fn card_expected(class: ActionClass) -> bool {
    matches!(
        class,
        ActionClass::TakeCard
            | ActionClass::BuildBuilding
            | ActionClass::BuildUnit
            | ActionClass::BuildWonderStage
            | ActionClass::UpgradeUnit
            | ActionClass::UpgradeProduction
            | ActionClass::DevelopTechnology
            | ActionClass::ElectLeader
            | ActionClass::ChangeGovernment
            | ActionClass::PlayTactic
            | ActionClass::DeclareWar
            | ActionClass::WinWar
            | ActionClass::PlayAggression
            | ActionClass::ProposePact
            | ActionClass::Colonize
            | ActionClass::WinAuction
            | ActionClass::Destroy
            | ActionClass::Disband
            | ActionClass::PlayActionCard
            | ActionClass::PutBack
            | ActionClass::ColumbusColonize
            | ActionClass::Barbarossa
            | ActionClass::BachTheater
    )
}

/// The outcome of classifying one journal line. `Bookkeeping` covers
/// phase/turn markers BGO logs on their own line (`"Action Phase begins"`)
/// and secondary consequence clauses logged as their own row rather than
/// appended to the triggering line (`"Orange produces 3 food"` following a
/// colonize two lines earlier) -- recognised, counted toward coverage, but
/// not toward any action rate. `Unclassified` is text this file could not
/// place at all.
pub enum LineOutcome {
    Action(Classified),
    Bookkeeping,
    Unclassified,
}

/// Splits `text` into `(actor, rest)` if a known colour leads it, the same
/// prefix scan [`classify`] itself does (see that function's body) --
/// exposed separately because callers that need MORE than [`classify`]
/// extracts from a line (a war/aggression/pact target colour, an action-point
/// cost, a bid amount -- `replay.rs`'s job, not `corpuscensus.rs`'s) need the
/// actor AND the remainder of the line to re-parse it themselves, without
/// duplicating this scan.
pub fn actor_and_rest(text: &str) -> Option<(Color, &str)> {
    for color in [Color::Orange, Color::Purple, Color::Green, Color::Grey] {
        if let Some(rest) = text.strip_prefix(color.as_str()).and_then(|r| r.strip_prefix(' ')) {
            return Some((color, rest));
        }
    }
    None
}

/// Classifies `text` (the journal's `text` column) into an [`LineOutcome`].
/// Column 2 (`player_colour`) is not consulted here at all -- see this
/// function's body for why the leading colour is read from `text` itself.
///
/// Shape coverage (measured empirically over all 1,011 games before this
/// function was written, by normalising colours/digits/card names out of
/// every line and histogramming what's left -- see the module doc): the top
/// ~90 shapes by frequency account for ~90% of all 451k lines, and every
/// arm below corresponds to one or more of those shapes. The match order is
/// significant: `"plays event"` is checked before generic `"plays "` so the
/// literal word `"event"` is never looked up in the card dictionary, and
/// longest-prefix card matching (not a fixed stop word) is what resolves
/// `"elects Isaac Newton Leonardo Da Vinci dies"` to elector `Isaac Newton`
/// with no ambiguity against the following `Leonardo Da Vinci`.
pub fn classify(index: &HashMap<&'static str, CardId>, text: &str) -> LineOutcome {
    // Lines BGO logs with no leading actor name at all.
    if text.starts_with("Game ") && text.ends_with("created.") {
        return LineOutcome::Bookkeeping;
    }
    if text == "Action Phase begins" || text == "No Discard Phase" {
        return LineOutcome::Bookkeeping;
    }
    if text.starts_with("Discard Phase ") {
        return LineOutcome::Bookkeeping;
    }
    if text.starts_with("Last turn") {
        return LineOutcome::Bookkeeping;
    }
    if text.starts_with("concedes defeat") {
        // Sometimes bare, sometimes with a trailing "<Color> scores/gets/
        // loses ..." consequence clause glued on with no separator.
        return LineOutcome::Bookkeeping;
    }
    if text.starts_with("End of game") {
        return LineOutcome::Bookkeeping;
    }
    if text == "All players have joined." {
        return LineOutcome::Bookkeeping;
    }
    // Frederick Barbarossa's leader ability (a free population increase
    // immediately spent building the named unit, `Move::Barbarossa`): BGO
    // logs it with the leader's surname as subject, not a colour, the actor
    // named only in the trailing "<Color> spends ..." clause(s) -- see
    // `ActionClass::Barbarossa`'s own doc comment for why this used to be
    // (wrongly) `Bookkeeping`.
    if let Some(after) = text.strip_prefix("Barbarossa enlists a ") {
        return match longest_known_card_prefix(index, after) {
            Some((id, _)) => LineOutcome::Action(Classified { class: ActionClass::Barbarossa, card: Some(id) }),
            None => LineOutcome::Unclassified,
        };
    }
    if text.starts_with("Terrorists destroy a ") {
        // The Terrorism event's flavour-text destruction line
        // ("Terrorists destroy a <Color> <Building>").
        return LineOutcome::Bookkeeping;
    }
    if text.starts_with("End turn") {
        // "End turn <Color> scores: ..." and the rarer "End turn <Card>
        // scores <N> culture.; <Color> scores: ..." (an Age III scoringEvent
        // card firing first). Either way this line is BGO's own per-turn
        // marker, which is what makes it the player-turn denominator.
        return LineOutcome::Action(Classified {
            class: ActionClass::EndTurn,
            card: None,
        });
    }
    // Winston Churchill's election quote: pure flavour text with no state
    // of its own -- the actual leader change is already applied by the
    // preceding "<Color> elects Winston Churchill" line.
    if text.starts_with("I have nothing to offer but blood, toil, tears, and sweat.") {
        return LineOutcome::Bookkeeping;
    }
    // Alexander the Great's leader ability, exercised as a political action
    // ("As a political action, you may remove Alexander from play and add 1
    // yellow token from the box to your yellow bank" --
    // `bga_throughtheages_material.inc.php`): BGO logs the WHOLE thing as
    // flavour text with no leading actor colour, the actor named only in
    // its own trailing consequence clause ("Alexander dies after building
    // his great Empire <Color> gets 1 yellow token"). Previously treated as
    // pure `Bookkeeping` and silently dropped -- discarding the yellow
    // token gain it always carries, which then drifts the reconstructed
    // yellow bank (and therefore `pop_cost`/`consumption`) for the rest of
    // the game. This is a real `Move::RemoveLeaderYellow`, not flavour;
    // `replay_common::replay_game` special-cases its dispatch the same way
    // it already does for the other no-leading-colour shape, `EndTurn`.
    if text.starts_with("Alexander dies after building his great Empire") {
        return LineOutcome::Action(Classified { class: ActionClass::RemoveLeaderYellow, card: None });
    }
    // Christopher Columbus's leader ability ("Christopher Columbus discovers
    // <Age> / <Territory>") -- see `ActionClass::ColumbusColonize`'s own doc
    // comment. Must be checked before this function's own generic "a known
    // card name leads the line" `Bookkeeping` catch-all below, since
    // "Christopher Columbus" is itself a card name and would otherwise match
    // there first and silently swallow the whole line, exactly as it did
    // before this was found.
    if let Some(rest) = text.strip_prefix("Christopher Columbus discovers ") {
        let card = rest.find(" / ").and_then(|slash| {
            let age = rest[..slash].trim();
            let name = rest[slash + " / ".len()..].trim();
            index
                .get(format!("{name} ({age})").as_str())
                .or_else(|| index.get(name))
                .copied()
        });
        return LineOutcome::Action(Classified { class: ActionClass::ColumbusColonize, card });
    }
    // J. S. Bach's leader ability (a free tech upgrade each round) is the
    // one line BGO prints with NO space at all between the leader's name
    // and the verb -- "Johannes Sebastian Bachupgrades Religion to Opera
    // ...". Every other shape in this file assumes single-space-delimited
    // text; this is the one confirmed exception, so it gets its own literal
    // check rather than a general fix to word-splitting for one card.
    // J. S. Bach's leader ability (`Move::BachTheater`): once per turn, as
    // an action-phase action, convert a Temple/Library-family building into
    // a Theater. BGO logs it with NO space between the leader's surname and
    // the verb ("Johannes Sebastian Bachupgrades <From> to <To> ..."), the
    // one confirmed exception to this file's "single-space-delimited"
    // assumption (see the old comment this replaced). Always the CURRENT
    // actor's own move (an action-phase action can only ever be theirs),
    // so unlike `RemoveLeaderYellow`/`Barbarossa` above it needs no actor
    // colour at all, trailing or otherwise -- `replay_common::replay_game`
    // resolves the actor as `state.current`, the same way it already does
    // for `EndTurn`. Previously `Bookkeeping` and silently dropped: losing
    // both the resource spend and the tableau change, which then drifts
    // `resources`/`workers_free` for the rest of the game -- found chasing
    // the Build/Upgrade/WonderStep cost-mismatch cluster (`docs/REPLAY.md`),
    // the same shape as `Barbarossa` just above.
    if let Some(after) = text.strip_prefix("Johannes Sebastian Bach").and_then(|s| s.strip_prefix("upgrades ")) {
        let Some((_from, remainder)) = longest_known_card_prefix(index, after) else {
            return LineOutcome::Unclassified;
        };
        let Some(to_part) = remainder.strip_prefix(" to ") else {
            return LineOutcome::Unclassified;
        };
        return match longest_known_card_prefix(index, to_part) {
            Some((id, _)) => LineOutcome::Action(Classified { class: ActionClass::BachTheater, card: Some(id) }),
            None => LineOutcome::Unclassified,
        };
    }
    // A handful more no-actor-colour system/consequence lines, each cheap
    // enough to name literally: a war/aggression that rolled to no effect,
    // an admin correction BGO occasionally injects, and an aggression's
    // "target had nothing to lose" outcome.
    if text.starts_with("Attacker's strength:")
        || text.starts_with("GAME DATA UPDATED")
        || text.starts_with("Operation successful")
    {
        return LineOutcome::Bookkeeping;
    }

    // The common case: the line is led by the acting player's own colour.
    // Tried against all four rather than trusting column 2, because BGO
    // does NOT always log a line under the colour named in its own text --
    // observed for territory-auction resolutions ("Grey wins Developed
    // Territory Winning bid is 2", logged on a DIFFERENT player's row) and
    // for the "must destroy/disband" forced-choice prompt. Column 2 still
    // decides which player's tally a line counts toward whenever the two
    // agree, which is the overwhelming majority of lines; this loop is what
    // makes classification correct on the minority where they don't.
    for color in [Color::Orange, Color::Purple, Color::Green, Color::Grey] {
        if let Some(rest) = text.strip_prefix(color.as_str()).and_then(|r| r.strip_prefix(' ')) {
            return classify_after_actor(index, rest);
        }
    }

    // No actor colour led the line at all. The remaining recognised shape
    // here is a secondary consequence clause whose subject is a card name
    // instead of a colour: the 15 Age III "Impact of ..." scoring-event
    // cards reprint their own name as the line's subject
    // ("Impact of Strength Each civilization scores culture..."), and a
    // handful of leaders have a passive per-round bonus logged the same way
    // ("Bill Gates scoring Orange scores 4 culture"). Either way this line
    // is a trailing detail of an action already tallied elsewhere (the
    // PlayEvent or ElectLeader that triggered it), not a new one -- so it
    // is bookkeeping, not Unclassified, whenever a known card name leads it.
    if longest_known_card_prefix(index, text).is_some() {
        return LineOutcome::Bookkeeping;
    }

    LineOutcome::Unclassified
}

/// Verb dispatch for a line already known to be led by a player colour --
/// `rest` is the text right after `"<Color> "`. Split out of [`classify`]
/// because subject detection (which colour, if any, led the line) and verb
/// dispatch (what that colour then did) are two different jobs; see
/// [`classify`] for why the former can't just trust column 2.
pub fn classify_after_actor(index: &HashMap<&'static str, CardId>, rest: &str) -> LineOutcome {
    if rest.starts_with("takes spoils of war") {
        // The war winner's post-victory consequence clause, not a card
        // take -- checked before the generic "takes " handler below, which
        // requires " in hand" and would otherwise call this Unclassified.
        return LineOutcome::Bookkeeping;
    }
    if let Some(after) = rest.strip_prefix("takes ") {
        return match after.find(" in hand") {
            Some(card_end) => match longest_known_card_prefix(index, &after[..card_end]) {
                Some((id, _)) => LineOutcome::Action(Classified {
                    class: ActionClass::TakeCard,
                    card: Some(id),
                }),
                None => LineOutcome::Unclassified,
            },
            None => LineOutcome::Unclassified,
        };
    }
    if rest.starts_with("plays event") {
        return LineOutcome::Action(Classified {
            class: ActionClass::PlayEvent,
            card: None,
        });
    }
    if let Some(after) = rest.strip_prefix("plays ") {
        return match longest_known_card_prefix(index, after) {
            Some((id, remainder)) if remainder.starts_with(" against ") => {
                LineOutcome::Action(Classified {
                    class: ActionClass::PlayAggression,
                    card: Some(id),
                })
            }
            Some((id, _)) => LineOutcome::Action(Classified {
                class: ActionClass::PlayActionCard,
                card: Some(id),
            }),
            None => LineOutcome::Unclassified,
        };
    }
    if rest.starts_with("increases population") {
        return LineOutcome::Action(Classified {
            class: ActionClass::IncreasePopulation,
            card: None,
        });
    }
    if let Some(after) = rest.strip_prefix("builds ") {
        return classify_builds(index, after);
    }
    if let Some(after) = rest.strip_prefix("upgrades ") {
        return classify_upgrade(index, after);
    }
    if let Some(after) = rest.strip_prefix("elects ") {
        return match longest_known_card_prefix(index, after) {
            Some((id, _)) => LineOutcome::Action(Classified {
                class: ActionClass::ElectLeader,
                card: Some(id),
            }),
            None => LineOutcome::Unclassified,
        };
    }
    if let Some(after) = rest.strip_prefix("discovers ") {
        return match longest_known_card_prefix(index, after) {
            Some((id, _)) => LineOutcome::Action(Classified {
                class: ActionClass::DevelopTechnology,
                card: Some(id),
            }),
            None => LineOutcome::Unclassified,
        };
    }
    if let Some(after) = rest.strip_prefix("colonizes a ") {
        return match longest_known_card_prefix(index, after) {
            Some((id, _)) => LineOutcome::Action(Classified {
                class: ActionClass::Colonize,
                card: Some(id),
            }),
            None => LineOutcome::Unclassified,
        };
    }
    if rest.starts_with("revolutions") {
        return match rest.find("Change government to ") {
            Some(p) => {
                let after = &rest[p + "Change government to ".len()..];
                match longest_known_card_prefix(index, after) {
                    Some((id, _)) => LineOutcome::Action(Classified {
                        class: ActionClass::ChangeGovernment,
                        card: Some(id),
                    }),
                    None => LineOutcome::Unclassified,
                }
            }
            None => LineOutcome::Unclassified,
        };
    }
    if let Some(after) = rest.strip_prefix("sets up new tactics ") {
        return classify_tactic(index, after);
    }
    if let Some(after) = rest.strip_prefix("adopts existing tactics ") {
        return classify_tactic(index, after);
    }
    if let Some(after) = rest.strip_prefix("proposes ") {
        return match longest_known_card_prefix(index, after) {
            Some((id, remainder)) if remainder.contains(" to ") => LineOutcome::Action(Classified {
                class: ActionClass::ProposePact,
                card: Some(id),
            }),
            _ => LineOutcome::Unclassified,
        };
    }
    if rest.starts_with("accepts pact offer") {
        // Sometimes bare, sometimes with a trailing "<PactName> is
        // cancelled" clause (accepting one pact auto-cancels an
        // incompatible standing one) -- still an accept either way.
        return LineOutcome::Action(Classified {
            class: ActionClass::AcceptPact,
            card: None,
        });
    }
    if let Some(after) = rest.strip_prefix("declares ") {
        return match longest_known_card_prefix(index, after) {
            Some((id, remainder)) if remainder.starts_with(" on ") => LineOutcome::Action(Classified {
                class: ActionClass::DeclareWar,
                card: Some(id),
            }),
            _ => LineOutcome::Unclassified,
        };
    }
    if let Some(after) = rest.strip_prefix("wins ") {
        return match longest_known_card_prefix(index, after) {
            Some((id, _)) if id.get().kind == CardType::War => LineOutcome::Action(Classified {
                class: ActionClass::WinWar,
                card: Some(id),
            }),
            Some((id, remainder)) if remainder.contains("Winning bid is") => {
                LineOutcome::Action(Classified {
                    class: ActionClass::WinAuction,
                    card: Some(id),
                })
            }
            _ => LineOutcome::Unclassified,
        };
    }
    if let Some(after) = rest.strip_prefix("destroys ") {
        return match longest_known_card_prefix(index, after) {
            Some((id, _)) => LineOutcome::Action(Classified {
                class: ActionClass::Destroy,
                card: Some(id),
            }),
            None => LineOutcome::Unclassified,
        };
    }
    if let Some(after) = rest.strip_prefix("disbands ") {
        return match longest_known_card_prefix(index, after) {
            Some((id, _)) => LineOutcome::Action(Classified {
                class: ActionClass::Disband,
                card: Some(id),
            }),
            None => LineOutcome::Unclassified,
        };
    }
    if rest.starts_with("discards ") {
        return LineOutcome::Action(Classified {
            class: ActionClass::Discard,
            card: None,
        });
    }
    if rest.starts_with("bids ") {
        return LineOutcome::Action(Classified {
            class: ActionClass::Bid,
            card: None,
        });
    }
    if let Some(after) = rest.strip_prefix("puts ") {
        return match longest_known_card_prefix(index, after) {
            Some((id, remainder)) if remainder.starts_with(" back in the row") => {
                LineOutcome::Action(Classified {
                    class: ActionClass::PutBack,
                    card: Some(id),
                })
            }
            _ => LineOutcome::Unclassified,
        };
    }
    if rest.starts_with("passes Political Phase") || rest == "passes" {
        return LineOutcome::Action(Classified {
            class: ActionClass::Pass,
            card: None,
        });
    }
    // Secondary consequence clauses BGO sometimes logs as their own line
    // (colonize/event/wonder/war/pact follow-on effects), a forced-choice
    // prompt, a pact-offer decline, and a rejected-move log entry (the
    // client tried an illegal action, e.g. not enough food to play a card).
    // Recognised so coverage reflects them, not tallied as an action since
    // they are not one BGO ever attributes as a distinct player choice.
    if rest.starts_with("produces ")
        || rest.starts_with("scores ")
        || rest.starts_with("gets ")
        || rest.starts_with("loses ")
        || rest.starts_with("spends ")
        || rest.starts_with("receives a new immigrant")
        || rest.starts_with("must destroy/disband")
        || rest.starts_with("defends ")
        || rest.starts_with("tries to defend")
        || rest.starts_with("declines")
        || rest.starts_with("thought he could play the card")
    {
        return LineOutcome::Bookkeeping;
    }

    LineOutcome::Unclassified
}

/// `"builds "` continuation: either `"<N> stage(s) of <Wonder>"`, a unit
/// (`"Warrior"` / `"Warriors"`, no dictionary lookup needed beyond the
/// alias), or a production/urban building.
pub fn classify_builds(index: &HashMap<&'static str, CardId>, after: &str) -> LineOutcome {
    if let Some(digits_end) = after.find(|c: char| !c.is_ascii_digit()) {
        if digits_end > 0 {
            let stage_tail = &after[digits_end..];
            if let Some(wonder_part) = stage_tail
                .strip_prefix(" stages of ")
                .or_else(|| stage_tail.strip_prefix(" stage of "))
            {
                return match longest_known_card_prefix(index, wonder_part) {
                    Some((id, _)) => LineOutcome::Action(Classified {
                        class: ActionClass::BuildWonderStage,
                        card: Some(id),
                    }),
                    None => LineOutcome::Unclassified,
                };
            }
        }
    }
    match longest_known_card_prefix(index, after) {
        Some((id, _)) if id.get().kind.is_unit() => LineOutcome::Action(Classified {
            class: ActionClass::BuildUnit,
            card: Some(id),
        }),
        Some((id, _)) => LineOutcome::Action(Classified {
            class: ActionClass::BuildBuilding,
            card: Some(id),
        }),
        None => LineOutcome::Unclassified,
    }
}

/// `"upgrades "` continuation: `"<From> to <To> ..."`. Bucketed by the
/// target's kind -- a unit promotion (`Infantry`/`Cavalry`/`Artillery`/
/// `Air`) counts as [`ActionClass::UpgradeUnit`], a farm/mine tech swap
/// counts as [`ActionClass::UpgradeProduction`].
pub fn classify_upgrade(index: &HashMap<&'static str, CardId>, after: &str) -> LineOutcome {
    let Some((_from_id, remainder)) = longest_known_card_prefix(index, after) else {
        return LineOutcome::Unclassified;
    };
    let Some(to_part) = remainder.strip_prefix(" to ") else {
        return LineOutcome::Unclassified;
    };
    match longest_known_card_prefix(index, to_part) {
        Some((id, _)) if id.get().kind.is_unit() => LineOutcome::Action(Classified {
            class: ActionClass::UpgradeUnit,
            card: Some(id),
        }),
        Some((id, _)) => LineOutcome::Action(Classified {
            class: ActionClass::UpgradeProduction,
            card: Some(id),
        }),
        None => LineOutcome::Unclassified,
    }
}

/// `"sets up new tactics "` / `"adopts existing tactics "` continuation:
/// `"<Age> / <Tactic>"`.
pub fn classify_tactic(index: &HashMap<&'static str, CardId>, after: &str) -> LineOutcome {
    match after.find(" / ") {
        Some(p) => {
            let tactic_part = &after[p + " / ".len()..];
            match longest_known_card_prefix(index, tactic_part) {
                Some((id, _)) => LineOutcome::Action(Classified {
                    class: ActionClass::PlayTactic,
                    card: Some(id),
                }),
                None => LineOutcome::Unclassified,
            }
        }
        None => LineOutcome::Unclassified,
    }
}

// ---------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn idx() -> HashMap<&'static str, CardId> {
        build_card_index()
    }

    #[test]
    fn every_card_in_the_base_game_table_is_reachable_by_its_own_printed_name() {
        let index = idx();
        for i in 0..CARDS.len() {
            let id = CardId(i as u16);
            assert!(
                index.contains_key(id.get().name) || index.contains_key(id.get().base_name),
                "card {} not reachable in the dictionary",
                id.get().name
            );
        }
    }

    #[test]
    fn takes_engineering_genius_in_hand_and_spends_a_civil_action() {
        let index = idx();
        let line = "Orange takes Engineering Genius in hand Orange uses 1 civil action";
        let LineOutcome::Action(c) = classify(&index, line) else {
            panic!("expected an action");
        };
        assert_eq!(c.class, ActionClass::TakeCard);
        assert_eq!(c.card.unwrap().get().base_name, "Engineering Genius");
    }

    #[test]
    fn plays_engineering_genius_to_build_a_wonder_stage_is_the_action_card_play_not_the_stage() {
        // The task's example line: the primary logged action is playing the
        // action card; the wonder-stage build is a consequence clause on
        // the same line, not tallied separately by this classifier.
        let index = idx();
        let line = "Orange plays Engineering Genius Orange builds 1 stage of Pyramids; Orange spends 1 resource";
        let LineOutcome::Action(c) = classify(&index, line) else {
            panic!("expected an action");
        };
        assert_eq!(c.class, ActionClass::PlayActionCard);
        assert_eq!(c.card.unwrap().get().base_name, "Engineering Genius");
    }

    #[test]
    fn purple_increases_population_and_spends_food() {
        let index = idx();
        let line = "Purple increases population Purple spends 2 food";
        let LineOutcome::Action(c) = classify(&index, line) else {
            panic!("expected an action");
        };
        assert_eq!(c.class, ActionClass::IncreasePopulation);
        assert!(c.card.is_none());
    }

    #[test]
    fn purple_builds_bronze_as_a_building_not_a_unit() {
        let index = idx();
        let line = "Purple builds Bronze Purple spends 2 resources";
        let LineOutcome::Action(c) = classify(&index, line) else {
            panic!("expected an action");
        };
        assert_eq!(c.class, ActionClass::BuildBuilding);
    }

    #[test]
    fn orange_builds_a_warrior_as_a_unit_via_the_bgo_singular_alias() {
        let index = idx();
        let line = "Orange builds Warrior Orange spends 2 resources";
        let LineOutcome::Action(c) = classify(&index, line) else {
            panic!("expected an action");
        };
        assert_eq!(c.class, ActionClass::BuildUnit);
        assert_eq!(c.card.unwrap().get().base_name, "Warriors");
    }

    #[test]
    fn purple_elects_aristotle_with_no_trailing_clause() {
        let index = idx();
        let line = "Purple elects Aristotle";
        let LineOutcome::Action(c) = classify(&index, line) else {
            panic!("expected an action");
        };
        assert_eq!(c.class, ActionClass::ElectLeader);
        assert_eq!(c.card.unwrap().get().base_name, "Aristotle");
    }

    #[test]
    fn electing_isaac_newton_resolves_the_electee_not_the_glued_on_dying_leader() {
        // The line under test butts two leader names together with no
        // separator: "Isaac Newton Leonardo Da Vinci dies; ...". Longest-
        // prefix matching must stop at "Isaac Newton" (the elected leader),
        // not run on into "Leonardo Da Vinci" (the leader who just died).
        let index = idx();
        let line = "Orange elects Isaac Newton Leonardo Da Vinci dies; Orange gets 1 civil action";
        let LineOutcome::Action(c) = classify(&index, line) else {
            panic!("expected an action");
        };
        assert_eq!(c.class, ActionClass::ElectLeader);
        assert_eq!(c.card.unwrap().get().base_name, "Isaac Newton");
    }

    #[test]
    fn leonardo_da_vinci_in_bgo_capitalisation_resolves_via_the_alias_table() {
        let index = idx();
        let line = "Orange elects Leonardo Da Vinci";
        let LineOutcome::Action(c) = classify(&index, line) else {
            panic!("expected an action");
        };
        assert_eq!(c.card.unwrap().get().base_name, "Leonardo da Vinci");
    }

    #[test]
    fn orange_takes_stockpile_in_hand_resolves_to_the_two_word_engine_name() {
        let index = idx();
        let line = "Orange takes Stockpile in hand Orange uses 1 civil action";
        let LineOutcome::Action(c) = classify(&index, line) else {
            panic!("expected an action");
        };
        assert_eq!(c.card.unwrap().get().base_name, "Stock Pile");
    }

    #[test]
    fn purple_takes_pyramids_in_hand_and_uses_two_civil_actions() {
        let index = idx();
        let line = "Purple takes Pyramids in hand Purple uses 2 civil action";
        let LineOutcome::Action(c) = classify(&index, line) else {
            panic!("expected an action");
        };
        assert_eq!(c.class, ActionClass::TakeCard);
        assert_eq!(c.card.unwrap().get().base_name, "Pyramids");
    }

    #[test]
    fn orange_builds_one_stage_of_pyramids_and_wonder_completed() {
        let index = idx();
        let line = "Orange builds 1 stage of Pyramids; ; Wonder completed";
        let LineOutcome::Action(c) = classify(&index, line) else {
            panic!("expected an action");
        };
        assert_eq!(c.class, ActionClass::BuildWonderStage);
        assert_eq!(c.card.unwrap().get().base_name, "Pyramids");
    }

    #[test]
    fn purple_upgrades_bronze_to_coal_is_a_production_upgrade() {
        let index = idx();
        let line = "Purple upgrades Bronze to Coal Purple spends 6 resources";
        let LineOutcome::Action(c) = classify(&index, line) else {
            panic!("expected an action");
        };
        assert_eq!(c.class, ActionClass::UpgradeProduction);
    }

    #[test]
    fn orange_upgrades_warrior_to_cavalrymen_is_a_unit_upgrade() {
        let index = idx();
        let line = "Orange upgrades Warrior to Cavalrymen Orange spends 3 resources";
        let LineOutcome::Action(c) = classify(&index, line) else {
            panic!("expected an action");
        };
        assert_eq!(c.class, ActionClass::UpgradeUnit);
        assert_eq!(c.card.unwrap().get().base_name, "Cavalrymen");
    }

    #[test]
    fn purple_sets_up_new_tactics_from_the_row_one_fighting_band() {
        let index = idx();
        let line = "Purple sets up new tactics I / Fighting Band";
        let LineOutcome::Action(c) = classify(&index, line) else {
            panic!("expected an action");
        };
        assert_eq!(c.class, ActionClass::PlayTactic);
        assert_eq!(c.card.unwrap().get().base_name, "Fighting Band");
    }

    #[test]
    fn orange_declares_war_over_culture_on_green() {
        let index = idx();
        let line = "Orange declares War over Culture on Green The victor takes 5 culture + 1 culture for each point of strength advantage from the defeated civilization. ; Orange uses 3 military action";
        let LineOutcome::Action(c) = classify(&index, line) else {
            panic!("expected an action");
        };
        assert_eq!(c.class, ActionClass::DeclareWar);
        assert_eq!(c.card.unwrap().get().base_name, "War over Culture");
    }

    #[test]
    fn purple_wins_war_over_territory_by_strength() {
        let index = idx();
        let line = "Purple wins War over Territory Attacker's strength: 30; Defender's strength: 22";
        let LineOutcome::Action(c) = classify(&index, line) else {
            panic!("expected an action");
        };
        assert_eq!(c.class, ActionClass::WinWar);
    }

    #[test]
    fn purple_plays_plunder_against_orange_is_an_aggression_not_a_generic_card_play() {
        let index = idx();
        let line = "Purple plays Plunder against Orange Your rival loses a total of up to 5 resource and/or food (your choice). You gain that many resource and food.; ; Purple uses 1 military action";
        let LineOutcome::Action(c) = classify(&index, line) else {
            panic!("expected an action");
        };
        assert_eq!(c.class, ActionClass::PlayAggression);
        assert_eq!(c.card.unwrap().get().base_name, "Aggression: Plunder");
    }

    #[test]
    fn grey_proposes_military_alliance_to_orange() {
        let index = idx();
        let line = "Grey proposes Military Alliance to Orange Grey is A; Orange is B; Both civilizations add +3 to their strength. This pact ends if one attacks the other.";
        let LineOutcome::Action(c) = classify(&index, line) else {
            panic!("expected an action");
        };
        assert_eq!(c.class, ActionClass::ProposePact);
        assert_eq!(c.card.unwrap().get().base_name, "Military Alliance");
    }

    #[test]
    fn green_accepts_a_pact_offer() {
        let index = idx();
        let line = "Green accepts pact offer";
        let LineOutcome::Action(c) = classify(&index, line) else {
            panic!("expected an action");
        };
        assert_eq!(c.class, ActionClass::AcceptPact);
    }

    #[test]
    fn purple_colonizes_a_vast_territory_with_no_age_suffix_in_the_text() {
        let index = idx();
        let line = "Purple colonizes a Vast Territory Sacrificed Units:; 1 Warrior; 1 Warrior; 1 Colonization card +1; Total force: 4; Purple produces 3 food";
        let LineOutcome::Action(c) = classify(&index, line) else {
            panic!("expected an action");
        };
        assert_eq!(c.class, ActionClass::Colonize);
        assert_eq!(c.card.unwrap().get().base_name, "Vast Territory");
    }

    #[test]
    fn orange_revolutions_into_hammurabi_style_government_change() {
        let index = idx();
        let line = "Orange revolutions Change government to Monarchy; 5 science points spent; Orange loses 5 science; Orange scores 2 culture";
        let LineOutcome::Action(c) = classify(&index, line) else {
            panic!("expected an action");
        };
        assert_eq!(c.class, ActionClass::ChangeGovernment);
        assert_eq!(c.card.unwrap().get().base_name, "Monarchy");
    }

    #[test]
    fn orange_discards_two_cards() {
        let index = idx();
        let line = "Orange discards 2 cards";
        let LineOutcome::Action(c) = classify(&index, line) else {
            panic!("expected an action");
        };
        assert_eq!(c.class, ActionClass::Discard);
    }

    #[test]
    fn end_turn_line_counts_as_the_player_turn_marker() {
        let index = idx();
        let line = "End turn Orange scores:; ; 0 culture (now 0); 1 science (now 1); 2 food - consumption: 0 (now 2); 2 resources (now 2)";
        let LineOutcome::Action(c) = classify(&index, line) else {
            panic!("expected an action");
        };
        assert_eq!(c.class, ActionClass::EndTurn);
    }

    #[test]
    fn action_phase_begins_is_bookkeeping_not_unclassified() {
        let index = idx();
        let line = "Action Phase begins";
        assert!(matches!(classify(&index, line), LineOutcome::Bookkeeping));
    }

    #[test]
    fn territory_auction_win_is_read_from_text_even_when_it_is_not_the_electee() {
        // Real corpus line: BGO logs this under a DIFFERENT player's
        // player_colour column than the "Grey" named in the text -- the
        // reason classify() derives the actor from text, not from column 2.
        let index = idx();
        let line = "Grey wins Developed Territory Winning bid is 2";
        let LineOutcome::Action(c) = classify(&index, line) else {
            panic!("expected an action");
        };
        assert_eq!(c.class, ActionClass::WinAuction);
        assert_eq!(c.card.unwrap().get().base_name, "Developed Territory");
    }

    #[test]
    fn an_impact_of_strength_scoring_line_with_no_actor_colour_is_bookkeeping() {
        // Age III scoringEvent cards reprint their own name as the line's
        // subject instead of any player's colour.
        let index = idx();
        let line = "Impact of Strength Each civilizations score culture according to their relative strength:; 4/2/1/0 for 4-player; Orange scores 4 culture";
        assert!(matches!(classify(&index, line), LineOutcome::Bookkeeping));
    }

    #[test]
    fn bread_and_circuses_resolves_via_the_ampersand_alias() {
        let index = idx();
        let line = "Purple builds Bread & Circuses Purple spends 4 resources";
        let LineOutcome::Action(c) = classify(&index, line) else {
            panic!("expected an action");
        };
        assert_eq!(c.card.unwrap().get().base_name, "Bread and Circuses");
    }

    #[test]
    fn concedes_defeat_with_a_glued_on_consequence_clause_is_still_bookkeeping() {
        let index = idx();
        let line = "concedes defeat Orange scores 4 culture; Purple loses 4 culture";
        assert!(matches!(classify(&index, line), LineOutcome::Bookkeeping));
    }

    #[test]
    fn a_war_defense_resolution_line_is_bookkeeping_not_a_new_action_class() {
        let index = idx();
        let line = "Purple defends 2 Defense card +1 played; 1 military card played; Purple strength: 12; Orange strength: 9";
        assert!(matches!(classify(&index, line), LineOutcome::Bookkeeping));
    }

    #[test]
    fn declining_a_pact_offer_is_bookkeeping() {
        let index = idx();
        let line = "Green declines offer";
        assert!(matches!(classify(&index, line), LineOutcome::Bookkeeping));
    }

    #[test]
    fn a_rejected_move_attempt_is_bookkeeping_not_a_real_frugality_play() {
        let index = idx();
        let line = "Orange thought he could play the card Frugality Not enough food";
        assert!(matches!(classify(&index, line), LineOutcome::Bookkeeping));
    }

    #[test]
    fn alexander_death_line_is_a_remove_leader_yellow_action_not_bookkeeping() {
        // Regression: this line used to be treated as flavour text and
        // silently dropped, discarding the yellow token it always carries
        // and drifting the reconstructed yellow bank for the rest of the
        // game (found chasing the `IllegalMove: Pop` bucket).
        let index = idx();
        let line = "Alexander dies after building his great Empire Orange gets 1 yellow token";
        let LineOutcome::Action(c) = classify(&index, line) else {
            panic!("expected an action, not bookkeeping");
        };
        assert_eq!(c.class, ActionClass::RemoveLeaderYellow);
        assert!(c.card.is_none());
    }

    #[test]
    fn columbus_discovery_line_is_a_columbus_colonize_action_not_bookkeeping() {
        // Regression: "Christopher Columbus" is itself a known card name, so
        // this line used to match the generic "known card name leads the
        // line" `Bookkeeping` catch-all and get silently dropped, discarding
        // the colonized territory's yellow_tokens/immediate_effects grants
        // and drifting pop_cost for the rest of the game -- the SAME shape
        // as the Alexander regression above, found chasing the same
        // `IllegalMove: Pop` bucket.
        let index = idx();
        let line = "Christopher Columbus discovers I / Vast Territory";
        let LineOutcome::Action(c) = classify(&index, line) else {
            panic!("expected an action, not bookkeeping");
        };
        assert_eq!(c.class, ActionClass::ColumbusColonize);
        assert_eq!(c.card, index.get("Vast Territory (I)").copied());
    }

    #[test]
    fn columbus_discovery_line_resolves_the_age_tagged_card_not_the_bare_name() {
        // The same territory family recurs across ages under one printed
        // name (`Vast Territory` at both Age I and Age II) -- BGO's "<Age> /
        // <Name>" clause disambiguates exactly the way `event_plan::
        // current_event_age_and_name`'s own doc explains for the identical
        // shape on event-reveal lines.
        let index = idx();
        let line = "Christopher Columbus discovers II / Vast Territory";
        let LineOutcome::Action(c) = classify(&index, line) else {
            panic!("expected an action, not bookkeeping");
        };
        assert_eq!(c.card, index.get("Vast Territory (II)").copied());
    }

    #[test]
    fn barbarossa_enlist_line_is_a_barbarossa_action_not_bookkeeping() {
        // Regression: this line used to be treated as flavour text and
        // silently dropped, discarding both the free population increase
        // and the unit build -- found chasing the Build/Upgrade/WonderStep
        // cost-mismatch cluster (135 games / 425 lines corpus-wide).
        let index = idx();
        let line = "Barbarossa enlists a Warrior; Orange spends 1 food; Orange spends 1 resource";
        let LineOutcome::Action(c) = classify(&index, line) else {
            panic!("expected an action, not bookkeeping");
        };
        assert_eq!(c.class, ActionClass::Barbarossa);
        assert_eq!(c.card.unwrap().get().base_name, "Warriors");
    }

    #[test]
    fn barbarossa_enlist_resolves_a_non_warrior_unit_too() {
        let index = idx();
        let line = "Barbarossa enlists a Knights; Green spends 2 food; Green spends 2 resources";
        let LineOutcome::Action(c) = classify(&index, line) else {
            panic!("expected an action, not bookkeeping");
        };
        assert_eq!(c.class, ActionClass::Barbarossa);
        assert_eq!(c.card.unwrap().get().base_name, "Knights");
    }

    #[test]
    fn bach_upgrade_line_is_a_bach_theater_action_not_bookkeeping() {
        // Regression: this glued-together, no-space line ("Bachupgrades",
        // no leading colour either) used to be treated as flavour text and
        // silently dropped, discarding the resource spend and the tableau
        // change -- found chasing the same cost-mismatch cluster (79 games
        // / 111 lines corpus-wide).
        let index = idx();
        let line = "Johannes Sebastian Bachupgrades Religion to Opera Purple spends 3 resources";
        let LineOutcome::Action(c) = classify(&index, line) else {
            panic!("expected an action, not bookkeeping");
        };
        assert_eq!(c.class, ActionClass::BachTheater);
        assert_eq!(c.card.unwrap().get().base_name, "Opera");
    }

    #[test]
    fn bach_upgrade_line_with_no_trailing_cost_clause_still_classifies() {
        // Free (fully-discounted) Bach upgrades print no "spends" clause at
        // all -- the classification must not depend on one being present.
        let index = idx();
        let line = "Johannes Sebastian Bachupgrades Philosophy to Drama";
        let LineOutcome::Action(c) = classify(&index, line) else {
            panic!("expected an action, not bookkeeping");
        };
        assert_eq!(c.class, ActionClass::BachTheater);
        assert_eq!(c.card.unwrap().get().base_name, "Drama");
    }

    #[test]
    fn taking_spoils_of_war_is_bookkeeping_not_a_card_take() {
        let index = idx();
        let line = "Orange takes spoils of war Orange gets 5 science; Purple loses 5 science";
        assert!(matches!(classify(&index, line), LineOutcome::Bookkeeping));
    }

    #[test]
    fn a_completely_novel_sentence_is_reported_unclassified_not_misclassified() {
        let index = idx();
        let line = "Orange does something this parser has never seen before";
        assert!(matches!(classify(&index, line), LineOutcome::Unclassified));
    }

    #[test]
    fn every_action_class_has_a_distinct_human_readable_label() {
        let mut labels = std::collections::HashSet::new();
        for &c in ActionClass::ALL {
            assert!(labels.insert(c.label()), "duplicate label for {c:?}");
        }
    }

    #[test]
    fn actor_and_rest_splits_off_the_leading_colour_and_keeps_the_remainder_verbatim() {
        let (actor, rest) = actor_and_rest("Grey declares War over Culture on Green blah").unwrap();
        assert_eq!(actor, Color::Grey);
        assert_eq!(rest, "declares War over Culture on Green blah");
    }

    #[test]
    fn actor_and_rest_is_none_when_no_known_colour_leads_the_line() {
        assert!(actor_and_rest("Action Phase begins").is_none());
    }

    #[test]
    fn colour_seats_follow_bgos_fixed_turn_order() {
        assert_eq!(Color::Orange.seat(), 0);
        assert_eq!(Color::Purple.seat(), 1);
        assert_eq!(Color::Green.seat(), 2);
        assert_eq!(Color::Grey.seat(), 3);
    }

    fn card(name: &str) -> CardId {
        CardId::by_name(name).unwrap_or_else(|| panic!("no such card: {name}"))
    }

    #[test]
    fn build_card_index_resolves_a_recurring_family_name_to_its_earliest_age() {
        // Not the fix -- the BUG this file's age-resolution helpers exist to
        // work around. `HashMap::or_insert` keeps whichever age iterates
        // first in `CARDS` (construction order, Age A before Age I before
        // ...), so a bare journal name like `"Urban Growth"` (no age tag --
        // BGO never prints one) always lands on Age A here. Pinned so a
        // future change to `CARDS`'s construction order fails loudly instead
        // of silently flipping which age every OTHER caller has to correct
        // for.
        let index = idx();
        assert_eq!(*index.get("Urban Growth").unwrap(), card("Urban Growth (A)"));
    }

    #[test]
    fn best_age_sibling_picks_the_highest_age_not_newer_than_the_bound() {
        let a = card("Urban Growth (A)");
        assert_eq!(best_age_sibling(a, Age::A), card("Urban Growth (A)"));
        assert_eq!(best_age_sibling(a, Age::I), card("Urban Growth (I)"));
        assert_eq!(best_age_sibling(a, Age::II), card("Urban Growth (II)"));
        assert_eq!(best_age_sibling(a, Age::III), card("Urban Growth (III)"));
    }

    #[test]
    fn best_age_sibling_is_a_no_op_for_a_card_with_no_same_name_siblings() {
        // Bronze (Age A only) has no same-named sibling at any other age --
        // every bound must return Bronze itself, not wander onto an
        // unrelated card.
        let bronze = card("Bronze");
        assert_eq!(best_age_sibling(bronze, Age::III), bronze);
    }

    #[test]
    fn family_siblings_finds_every_age_of_a_recurring_action_card_and_nothing_else() {
        let mut names: Vec<&str> = family_siblings(card("Rich Land (A)")).iter().map(|id| id.get().name).collect();
        names.sort_unstable();
        assert_eq!(names, ["Rich Land (A)", "Rich Land (I)", "Rich Land (II)"]);
    }
}
