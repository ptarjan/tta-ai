//! `corpuscensus` -- a play-rate census over the 1,011-game BGO human corpus
//! (`sources/bgo/`), which nothing in this repo reads (see `docs/HAZARDS.md`'s
//! "no external anchor" entry: the bot's only strength signal is beating its
//! own frozen ancestors). This does not fix that hazard -- it does not play
//! the corpus, score it, or compare it to the bot -- it answers a narrower,
//! cheaper question: **what do strong humans actually spend their turns on**,
//! by counting, not by reconstructing game state (which the corpus cannot
//! support anyway -- see the module doc below on the row-imputation gap).
//!
//! ```text
//! tar -xzf sources/bgo/journals.tar.gz -C /tmp/bgo-journals
//! cargo run --profile difftest --bin corpuscensus -- \
//!     sources/bgo/index.tsv /tmp/bgo-journals/journals
//! ```
//!
//! Prints a Markdown report to stdout; `docs/HUMAN_PLAY.md`'s census section
//! is that output, pasted in by hand (this binary does not write the doc
//! itself -- the doc also carries prose the binary has no business
//! generating).
//!
//! # Method: journal text is a small fixed set of BGO-generated shapes
//!
//! `sources/bgo/journals.tar.gz` has one `journals/<game_id>.tsv` per game:
//! `date, player_colour, age, round, text`. `text` is English prose with a
//! fixed small vocabulary (BGO template-generates it), so this is text
//! classification against a discovered shape set, not a parser for a
//! context-free grammar. The shape set was discovered empirically (normalise
//! player colours/digits/card names out of every line, histogram what's
//! left) before writing a single match arm here -- see [`classify`]'s doc for
//! the shapes that resulted and [`main`]'s coverage report for how much of
//! the corpus they explain.
//!
//! One assumption this file does NOT make, despite being tempting: that
//! column 2 (`player_colour`) is always the line's actor. It almost always
//! is, but not for territory-auction wins ("Grey wins Developed Territory
//! Winning bid is 2" logged on a different player's row than Grey's) or the
//! forced-discard-choice prompt -- found by diffing text against column 2
//! while chasing the last few points of parser coverage. [`classify`] reads
//! the actor from `text` itself for exactly this reason; see its body.
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
//! card: "Leonardo Da Vinci" (BGO capitalises "Da") vs. "Leonardo da Vinci"
//! (engine), "Maximillien Robespierre" (BGO, extra L) vs. "Maximilien
//! Robespierre" (engine), "Charles Chaplin" (BGO's formal first name) vs.
//! "Charlie Chaplin" (engine), "Johannes Sebastian Bach" (BGO's full name)
//! vs. "J. S. Bach" (engine's abbreviation), "Warrior" (BGO, the unit as
//! built) vs. "Warriors" (engine, the card's printed name), "Ocean Liner"
//! (BGO) vs. "Ocean Liners" (engine wonder name), "Stockpile" (BGO, no
//! space) vs. "Stock Pile" (engine), "Bread & Circuses" (BGO, an
//! ampersand) vs. "Bread and Circuses" (engine), and "Loss of Sovereignity"
//! (BGO's misspelling) vs. "Loss of Sovereignty" (engine, and the rulebook).
//! [`ALIASES`] is the fix: each BGO spelling is inserted into the same
//! dictionary, pointing at the same [`CardId`] its engine spelling resolves
//! to. This was found, not assumed -- by two rounds of the same method: an
//! offline prefix-matching scan over every "unmatched" card-shaped string
//! before this file was written (which found the first seven), and this
//! binary's own coverage report against the unclassified-shape residue
//! (which found the last two, both under 30 occurrences). With the aliases
//! in, zero games contain a card name outside the 2015 base game -- the
//! `edition_verified_by` column's promise holds, checked, not just trusted.
//!
//! # What this deliberately does not do
//!
//! No game-state reconstruction, no move legality, no hidden information
//! (military hand contents / discards are permanently counts-only in the
//! journal, never identities -- nothing to recover here). No "prepare event"
//! action class: it does not exist in the text. BGO logs only the
//! resolution (`"X plays event ..."`); Age cards headed for a future event
//! slot leave no journal line of their own, so [`ActionClass::PlayEvent`] is
//! the closest observable proxy, not a distinct preparation step, and this
//! file does not pretend otherwise. No take-back correction: `docs/
//! HUMAN_PLAY.md`'s existing analysis (Python, now deleted, findings kept)
//! found ~8% of raw "takes" are a human undoing their own take within the
//! same turn (`"X takes Y in hand" ... "X puts Y back in the row" X gets
//! back exactly what it spent`); detecting that reliably needs the
//! actions-spent/actions-refunded pair to match exactly, which is state
//! tracking, not counting, so it is out of scope here. [`ActionClass::
//! PutBack`] is counted as its own bucket instead -- an upper bound on
//! take-backs, reported next to the raw take count so nobody mistakes one
//! for the other.

use std::collections::HashMap;
use std::env;
use std::fs;
use std::process::ExitCode;

use tta::{CardId, CardType, CARDS};

// ---------------------------------------------------------------------
// Card name dictionary
// ---------------------------------------------------------------------

/// BGO spelling -> the engine's spelling for the same card. See the module
/// doc's "BGO's spelling is not the engine's spelling" section for how each
/// of these was found (not assumed); see the module doc's "BGO's spelling is
/// not the engine's spelling" section for how and when each was caught.
const ALIASES: &[(&str, &str)] = &[
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
];

/// The longest a real base-game card name spans once split on ASCII spaces
/// (`"Development of Trade Routes"`, `"Promise of Military Protection"` are
/// four; nothing in `tta::CARDS` or [`ALIASES`] is longer). Kept a little
/// generous rather than exactly tight -- one extra failed HashMap lookup per
/// miss costs nothing measurable at this corpus size.
const MAX_NAME_WORDS: usize = 6;

/// Every string BGO's journal text might print for a card, mapped to the
/// [`CardId`] the engine knows it by. Built once in [`main`] and threaded
/// through as a plain reference -- not a global, per this project's rule
/// against process-global mutable state (`DESIGN.md` rule).
///
/// Populated from three sources per card: the disambiguated `name` (e.g.
/// `"Aggression: Plunder (II)"`), the `base_name` BGO actually prints (e.g.
/// `"Plunder"` after stripping the `"Aggression: "` prefix and any `" (I)"`/
/// `" (II)"`/`" (III)"` age suffix -- BGO's Territory and Aggression cards
/// print only the family name, never the age), and [`ALIASES`] for the cards
/// where BGO's own spelling differs from the engine's.
fn build_card_index() -> HashMap<&'static str, CardId> {
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
fn strip_age_suffix(name: &str) -> Option<&str> {
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
fn nth_word_end(s: &str, n: usize) -> Option<usize> {
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
fn longest_known_card_prefix<'a>(
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

// ---------------------------------------------------------------------
// Journal domain types
// ---------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
enum Color {
    Orange,
    Purple,
    Green,
    Grey,
}

impl Color {
    fn parse(s: &str) -> Option<Color> {
        match s {
            "Orange" => Some(Color::Orange),
            "Purple" => Some(Color::Purple),
            "Green" => Some(Color::Green),
            "Grey" => Some(Color::Grey),
            _ => None,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Color::Orange => "Orange",
            Color::Purple => "Purple",
            Color::Green => "Green",
            Color::Grey => "Grey",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash, PartialOrd, Ord)]
enum Tier {
    Prince,
    King,
    Warlord,
    Emperor,
}

impl Tier {
    fn parse(s: &str) -> Result<Tier, String> {
        match s {
            "Prince" => Ok(Tier::Prince),
            "King" => Ok(Tier::King),
            "Warlord" => Ok(Tier::Warlord),
            "Emperor" => Ok(Tier::Emperor),
            other => Err(format!("unknown BGO level tier {other:?}")),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Tier::Prince => "Prince",
            Tier::King => "King",
            Tier::Warlord => "Warlord",
            Tier::Emperor => "Emperor",
        }
    }
}

/// One row of `sources/bgo/index.tsv`.
struct GameMeta {
    id: String,
    players: u8,
    tier: Tier,
    rounds: u32,
    reached_age_iv: bool,
    scores: Vec<i32>,
}

fn parse_index(path: &str) -> Result<Vec<GameMeta>, String> {
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
        for entry in fields[c_results].split('|') {
            let (_name, score_str) = entry
                .rsplit_once(':')
                .ok_or_else(|| format!("index.tsv row {}: bad results entry {entry:?}", lineno + 2))?;
            let score: i32 = score_str
                .parse()
                .map_err(|e| format!("index.tsv row {}: bad score {score_str:?}: {e}", lineno + 2))?;
            scores.push(score);
        }
        games.push(GameMeta {
            id: fields[c_id].to_string(),
            players,
            tier,
            rounds,
            reached_age_iv,
            scores,
        });
    }
    Ok(games)
}

// ---------------------------------------------------------------------
// Line classification
// ---------------------------------------------------------------------

/// The action classes this census tallies. Every variant is either one of
/// the classes the task asked for by name, or one that fell out of parsing
/// those for free (once a line's leading colour is found, recognising the
/// verb after it costs the same whether or not the result is tallied).
/// Deliberately NOT included: a `PrepareEvent` variant -- see the module
/// doc for why that class does not exist in this text.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash, PartialOrd, Ord)]
enum ActionClass {
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
}

impl ActionClass {
    /// All variants, for a stable table iteration order and for the
    /// exhaustiveness check in the test module.
    const ALL: &'static [ActionClass] = &[
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
    ];

    fn label(self) -> &'static str {
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
        }
    }
}

/// The result of classifying one journal line: which action class it is,
/// and the card involved if the class carries one and matching found it.
/// `card` is `None` both for classes that never carry a card (`Pass`,
/// `Discard`, ...) and for the rare case a card-carrying class matched its
/// verb but [`longest_known_card_prefix`] found nothing after it -- callers
/// that need to tell those apart use [`Classified::card_expected`].
struct Classified {
    class: ActionClass,
    card: Option<CardId>,
}

fn card_expected(class: ActionClass) -> bool {
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
    )
}

/// The outcome of classifying one journal line. `Bookkeeping` covers
/// phase/turn markers BGO logs on their own line (`"Action Phase begins"`)
/// and secondary consequence clauses logged as their own row rather than
/// appended to the triggering line (`"Orange produces 3 food"` following a
/// colonize two lines earlier) -- recognised, counted toward coverage, but
/// not toward any action rate. `Unclassified` is text this file could not
/// place at all; that count is what [`main`] reports as parser coverage.
enum LineOutcome {
    Action(Classified),
    Bookkeeping,
    Unclassified,
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
fn classify(index: &HashMap<&'static str, CardId>, text: &str) -> LineOutcome {
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
    if text.starts_with("Barbarossa enlists a ") {
        // Frederick Barbarossa's leader ability (free unit each round),
        // logged with the leader's surname as subject, not a colour.
        return LineOutcome::Bookkeeping;
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
    // Two one-off leader-death/leader-election flavour quotes BGO prints as
    // their own line with no actor colour and no card name either
    // (Alexander the Great's death effect, Winston Churchill's election
    // quote). Cheap enough to name literally rather than leave unclassified.
    if text.starts_with("Alexander dies after building his great Empire")
        || text.starts_with("I have nothing to offer but blood, toil, tears, and sweat.")
    {
        return LineOutcome::Bookkeeping;
    }
    // J. S. Bach's leader ability (a free tech upgrade each round) is the
    // one line BGO prints with NO space at all between the leader's name
    // and the verb -- "Johannes Sebastian Bachupgrades Religion to Opera
    // ...". Every other shape in this file assumes single-space-delimited
    // text; this is the one confirmed exception, so it gets its own literal
    // check rather than a general fix to word-splitting for one card.
    if text.starts_with("Johannes Sebastian Bachupgrades ") {
        return LineOutcome::Bookkeeping;
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
fn classify_after_actor(index: &HashMap<&'static str, CardId>, rest: &str) -> LineOutcome {
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
fn classify_builds(index: &HashMap<&'static str, CardId>, after: &str) -> LineOutcome {
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
/// counts as [`ActionClass::UpgradeProduction`] -- since the same verb
/// covers both in BGO's text and the task only asked for the unit half.
fn classify_upgrade(index: &HashMap<&'static str, CardId>, after: &str) -> LineOutcome {
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
fn classify_tactic(index: &HashMap<&'static str, CardId>, after: &str) -> LineOutcome {
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
// Aggregation
// ---------------------------------------------------------------------

#[derive(Default)]
struct Bucket {
    games: u32,
    turns: u64,
    rounds_sum: u64,
    age_iv_games: u32,
    score_sum: i64,
    score_n: u64,
    score_max: i32,
    action_counts: HashMap<ActionClass, u64>,
    games_with_war: u32,
    games_with_aggression: u32,
    games_with_pact: u32,
    games_with_tactic: u32,
}

impl Bucket {
    fn add_game(&mut self, meta: &GameMeta, game_actions: &HashMap<ActionClass, u64>, turns: u64) {
        self.games += 1;
        self.turns += turns;
        self.rounds_sum += meta.rounds as u64;
        if meta.reached_age_iv {
            self.age_iv_games += 1;
        }
        for &s in &meta.scores {
            self.score_sum += s as i64;
            self.score_n += 1;
            self.score_max = self.score_max.max(s);
        }
        for (&class, &n) in game_actions {
            *self.action_counts.entry(class).or_insert(0) += n;
        }
        let has = |c: ActionClass| game_actions.get(&c).copied().unwrap_or(0) > 0;
        if has(ActionClass::DeclareWar) {
            self.games_with_war += 1;
        }
        if has(ActionClass::PlayAggression) {
            self.games_with_aggression += 1;
        }
        if has(ActionClass::ProposePact) {
            self.games_with_pact += 1;
        }
        if has(ActionClass::PlayTactic) {
            self.games_with_tactic += 1;
        }
    }

    fn count(&self, c: ActionClass) -> u64 {
        self.action_counts.get(&c).copied().unwrap_or(0)
    }

    fn per_game(&self, c: ActionClass) -> f64 {
        if self.games == 0 {
            0.0
        } else {
            self.count(c) as f64 / self.games as f64
        }
    }

    fn per_turn(&self, c: ActionClass) -> f64 {
        if self.turns == 0 {
            0.0
        } else {
            self.count(c) as f64 / self.turns as f64
        }
    }
}

fn print_action_table(name: &str, buckets: &[(&str, &Bucket)]) {
    println!("\n### {name}\n");
    print!("| action class |");
    for (label, _) in buckets {
        print!(" {label} /game | {label} /turn |");
    }
    println!();
    print!("|---|");
    for _ in buckets {
        print!("---|---|");
    }
    println!();
    for &class in ActionClass::ALL {
        print!("| {} |", class.label());
        for (_, b) in buckets {
            print!(" {:.3} | {:.4} |", b.per_game(class), b.per_turn(class));
        }
        println!();
    }
}

fn print_top_n(title: &str, freq: &HashMap<&'static str, u64>, n: usize) {
    let mut entries: Vec<(&&str, &u64)> = freq.iter().collect();
    entries.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    let total: u64 = freq.values().sum();
    let top_sum: u64 = entries.iter().take(n).map(|(_, c)| **c).sum();
    println!("\n### {title}\n");
    println!("Total: {total}, distinct names: {}\n", entries.len());
    println!("| rank | name | count | % of total |");
    println!("|---|---|---|---|");
    for (i, (name, count)) in entries.iter().take(n).enumerate() {
        println!(
            "| {} | {name} | {count} | {:.2}% |",
            i + 1,
            **count as f64 * 100.0 / total.max(1) as f64
        );
    }
    let tail = total - top_sum;
    let tail_names = entries.len().saturating_sub(n);
    println!(
        "\nLong tail (rank {}+, {tail_names} names): {tail} ({:.2}%)",
        n + 1,
        tail as f64 * 100.0 / total.max(1) as f64
    );
}

// ---------------------------------------------------------------------
// main
// ---------------------------------------------------------------------

fn run(index_path: &str, journals_dir: &str) -> Result<(), String> {
    let card_index = build_card_index();
    let games = parse_index(index_path)?;
    println!("Parsed {} games from {index_path}.", games.len());

    let mut by_players: HashMap<u8, Bucket> = HashMap::new();
    let mut by_tier: HashMap<Tier, Bucket> = HashMap::new();
    let mut overall = Bucket::default();

    let mut takes_freq: HashMap<&'static str, u64> = HashMap::new();
    let mut plays_freq: HashMap<&'static str, u64> = HashMap::new();

    let mut total_lines: u64 = 0;
    let mut classified_lines: u64 = 0;
    let mut unclassified_shapes: HashMap<String, u64> = HashMap::new();

    let mut excluded_games: Vec<(String, String)> = Vec::new();

    for meta in &games {
        let path = format!("{journals_dir}/{}.tsv", meta.id);
        let text = match fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) => {
                excluded_games.push((meta.id.clone(), format!("no journal file: {e}")));
                continue;
            }
        };

        let mut game_actions: HashMap<ActionClass, u64> = HashMap::new();
        let mut turns: u64 = 0;
        let mut bad_card_in_game = false;

        for (lineno, line) in text.lines().enumerate() {
            if lineno == 0 {
                continue; // header
            }
            if line.is_empty() {
                continue;
            }
            let fields: Vec<&str> = line.splitn(5, '\t').collect();
            if fields.len() != 5 {
                continue;
            }
            if Color::parse(fields[1]).is_none() {
                continue; // malformed row: player_colour isn't one of the 4 known colours
            }
            let raw_text = fields[4];
            total_lines += 1;

            match classify(&card_index, raw_text) {
                LineOutcome::Action(c) => {
                    classified_lines += 1;
                    if c.class == ActionClass::EndTurn {
                        turns += 1;
                    }
                    *game_actions.entry(c.class).or_insert(0) += 1;
                    if card_expected(c.class) && c.card.is_none() {
                        bad_card_in_game = true;
                    }
                    if let Some(id) = c.card {
                        let name = id.get().base_name;
                        match c.class {
                            ActionClass::TakeCard => {
                                *takes_freq.entry(name).or_insert(0) += 1;
                            }
                            ActionClass::BuildBuilding
                            | ActionClass::BuildUnit
                            | ActionClass::BuildWonderStage
                            | ActionClass::DevelopTechnology
                            | ActionClass::ElectLeader
                            | ActionClass::ChangeGovernment
                            | ActionClass::PlayTactic
                            | ActionClass::DeclareWar
                            | ActionClass::PlayAggression
                            | ActionClass::ProposePact
                            | ActionClass::Colonize
                            | ActionClass::PlayActionCard => {
                                *plays_freq.entry(name).or_insert(0) += 1;
                            }
                            _ => {}
                        }
                    }
                }
                LineOutcome::Bookkeeping => {
                    classified_lines += 1;
                }
                LineOutcome::Unclassified => {
                    let shape = normalize_shape(raw_text);
                    *unclassified_shapes.entry(shape).or_insert(0) += 1;
                }
            }
        }

        if bad_card_in_game {
            excluded_games.push((
                meta.id.clone(),
                "card-carrying line matched a verb but no known base-game card name followed \
                 it (possible expansion card or unrecognised name)"
                    .to_string(),
            ));
            continue;
        }

        overall.add_game(meta, &game_actions, turns);
        by_players
            .entry(meta.players)
            .or_default()
            .add_game(meta, &game_actions, turns);
        by_tier
            .entry(meta.tier)
            .or_default()
            .add_game(meta, &game_actions, turns);
    }

    let included_games = games.len() - excluded_games.len();
    println!(
        "\nIncluded {included_games} of {} indexed games ({} excluded).",
        games.len(),
        excluded_games.len()
    );
    if !excluded_games.is_empty() {
        println!("\nExcluded games:");
        for (id, reason) in &excluded_games {
            println!("- {id}: {reason}");
        }
    }

    println!(
        "\nParser coverage: {classified_lines} / {total_lines} lines classified ({:.2}%).",
        classified_lines as f64 * 100.0 / total_lines.max(1) as f64
    );
    let mut shapes: Vec<(&String, &u64)> = unclassified_shapes.iter().collect();
    shapes.sort_by(|a, b| b.1.cmp(a.1));
    println!("\nTop unclassified shapes:");
    for (shape, count) in shapes.iter().take(20) {
        println!("- {count:>6}  {shape}");
    }

    println!("\n## Overall (all {included_games} games)");
    print_action_table(
        "Action rates, overall",
        &[("all", &overall)],
    );

    let p2 = by_players.get(&2).cloned_default();
    let p3 = by_players.get(&3).cloned_default();
    let p4 = by_players.get(&4).cloned_default();
    print_action_table(
        "Action rates by player count",
        &[("2p", &p2), ("3p", &p3), ("4p", &p4)],
    );

    let prince = by_tier.get(&Tier::Prince).cloned_default();
    let king = by_tier.get(&Tier::King).cloned_default();
    let warlord = by_tier.get(&Tier::Warlord).cloned_default();
    let emperor = by_tier.get(&Tier::Emperor).cloned_default();
    print_action_table(
        "Action rates by BGO level tier",
        &[
            (Tier::Prince.as_str(), &prince),
            (Tier::King.as_str(), &king),
            (Tier::Warlord.as_str(), &warlord),
            (Tier::Emperor.as_str(), &emperor),
        ],
    );

    println!("\n### Military summary, per game, by player count\n");
    println!("| | 2p (n={}) | 3p (n={}) | 4p (n={}) |", p2.games, p3.games, p4.games);
    println!("|---|---|---|---|");
    println!(
        "| games with >=1 war declared | {} ({:.1}%) | {} ({:.1}%) | {} ({:.1}%) |",
        p2.games_with_war,
        p2.games_with_war as f64 * 100.0 / p2.games.max(1) as f64,
        p3.games_with_war,
        p3.games_with_war as f64 * 100.0 / p3.games.max(1) as f64,
        p4.games_with_war,
        p4.games_with_war as f64 * 100.0 / p4.games.max(1) as f64
    );
    println!(
        "| games with >=1 aggression played | {} ({:.1}%) | {} ({:.1}%) | {} ({:.1}%) |",
        p2.games_with_aggression,
        p2.games_with_aggression as f64 * 100.0 / p2.games.max(1) as f64,
        p3.games_with_aggression,
        p3.games_with_aggression as f64 * 100.0 / p3.games.max(1) as f64,
        p4.games_with_aggression,
        p4.games_with_aggression as f64 * 100.0 / p4.games.max(1) as f64
    );
    println!(
        "| games with >=1 pact proposed | {} ({:.1}%) | {} ({:.1}%) | {} ({:.1}%) |",
        p2.games_with_pact,
        p2.games_with_pact as f64 * 100.0 / p2.games.max(1) as f64,
        p3.games_with_pact,
        p3.games_with_pact as f64 * 100.0 / p3.games.max(1) as f64,
        p4.games_with_pact,
        p4.games_with_pact as f64 * 100.0 / p4.games.max(1) as f64
    );
    println!(
        "| wars declared /game | {:.3} | {:.3} | {:.3} |",
        p2.per_game(ActionClass::DeclareWar),
        p3.per_game(ActionClass::DeclareWar),
        p4.per_game(ActionClass::DeclareWar)
    );
    println!(
        "| aggressions played /game | {:.3} | {:.3} | {:.3} |",
        p2.per_game(ActionClass::PlayAggression),
        p3.per_game(ActionClass::PlayAggression),
        p4.per_game(ActionClass::PlayAggression)
    );
    println!(
        "| tactics played /game | {:.3} | {:.3} | {:.3} |",
        p2.per_game(ActionClass::PlayTactic),
        p3.per_game(ActionClass::PlayTactic),
        p4.per_game(ActionClass::PlayTactic)
    );
    println!(
        "| pacts proposed /game | {:.3} | {:.3} | {:.3} |",
        p2.per_game(ActionClass::ProposePact),
        p3.per_game(ActionClass::ProposePact),
        p4.per_game(ActionClass::ProposePact)
    );
    println!(
        "| pacts accepted /game | {:.3} | {:.3} | {:.3} |",
        p2.per_game(ActionClass::AcceptPact),
        p3.per_game(ActionClass::AcceptPact),
        p4.per_game(ActionClass::AcceptPact)
    );

    println!("\n### Game length and scoring, by player count\n");
    println!("| | 2p | 3p | 4p |");
    println!("|---|---|---|---|");
    println!(
        "| mean rounds | {:.2} | {:.2} | {:.2} |",
        p2.rounds_sum as f64 / p2.games.max(1) as f64,
        p3.rounds_sum as f64 / p3.games.max(1) as f64,
        p4.rounds_sum as f64 / p4.games.max(1) as f64
    );
    println!(
        "| reached Age IV | {}/{} ({:.1}%) | {}/{} ({:.1}%) | {}/{} ({:.1}%) |",
        p2.age_iv_games, p2.games, p2.age_iv_games as f64 * 100.0 / p2.games.max(1) as f64,
        p3.age_iv_games, p3.games, p3.age_iv_games as f64 * 100.0 / p3.games.max(1) as f64,
        p4.age_iv_games, p4.games, p4.age_iv_games as f64 * 100.0 / p4.games.max(1) as f64
    );
    println!(
        "| mean final score | {:.1} | {:.1} | {:.1} |",
        p2.score_sum as f64 / p2.score_n.max(1) as f64,
        p3.score_sum as f64 / p3.score_n.max(1) as f64,
        p4.score_sum as f64 / p4.score_n.max(1) as f64
    );
    println!(
        "| max final score seen | {} | {} | {} |",
        p2.score_max, p3.score_max, p4.score_max
    );

    print_top_n("Cards taken from the row (frequency)", &takes_freq, 25);
    print_top_n("Cards played/built/discovered (frequency)", &plays_freq, 25);

    Ok(())
}

/// `HashMap::get(&K).cloned_default()`-style helper: an absent bucket (no
/// games at all in that split) should print as zeros, not panic or vanish
/// from the table.
trait ClonedDefault<T> {
    fn cloned_default(self) -> T;
}
impl ClonedDefault<Bucket> for Option<&Bucket> {
    fn cloned_default(self) -> Bucket {
        match self {
            Some(b) => Bucket {
                games: b.games,
                turns: b.turns,
                rounds_sum: b.rounds_sum,
                age_iv_games: b.age_iv_games,
                score_sum: b.score_sum,
                score_n: b.score_n,
                score_max: b.score_max,
                action_counts: b.action_counts.clone(),
                games_with_war: b.games_with_war,
                games_with_aggression: b.games_with_aggression,
                games_with_pact: b.games_with_pact,
                games_with_tactic: b.games_with_tactic,
            },
            None => Bucket::default(),
        }
    }
}

/// Normalises one unclassified line for shape histogramming: strips actor
/// colours and digit runs so that e.g. "Orange scores 4 culture" and
/// "Purple scores 11 culture" collapse to the same bucket in the coverage
/// report. Deliberately crude (no card-name stripping) since this only runs
/// on the residue this file failed to classify, and the report just needs
/// to name the shape, not fully parse it.
fn normalize_shape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c.is_ascii_digit() {
            while chars.peek().is_some_and(|c| c.is_ascii_digit()) {
                chars.next();
            }
            out.push('#');
        } else {
            out.push(c);
        }
    }
    for color in ["Orange", "Purple", "Green", "Grey"] {
        out = out.replace(color, "<C>");
    }
    out.chars().take(160).collect()
}

fn main() -> ExitCode {
    let argv: Vec<String> = env::args().skip(1).collect();
    if argv.len() != 2 {
        eprintln!("usage: corpuscensus <index.tsv> <journals_dir>");
        return ExitCode::FAILURE;
    }
    match run(&argv[0], &argv[1]) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
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
        assert!(matches!(
            classify(&index, line),
            LineOutcome::Bookkeeping
        ));
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
    fn taking_spoils_of_war_is_bookkeeping_not_a_card_take() {
        let index = idx();
        let line = "Orange takes spoils of war Orange gets 5 science; Purple loses 5 science";
        assert!(matches!(classify(&index, line), LineOutcome::Bookkeeping));
    }

    #[test]
    fn a_completely_novel_sentence_is_reported_unclassified_not_misclassified() {
        let index = idx();
        let line = "Orange does something this parser has never seen before";
        assert!(matches!(
            classify(&index, line),
            LineOutcome::Unclassified
        ));
    }

    #[test]
    fn every_action_class_has_a_distinct_human_readable_label() {
        let mut labels = std::collections::HashSet::new();
        for &c in ActionClass::ALL {
            assert!(labels.insert(c.label()), "duplicate label for {c:?}");
        }
    }
}
