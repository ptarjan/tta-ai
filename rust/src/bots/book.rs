//! `BookBot` -- Through the Ages played "from the book".
//!
//! Ports `engine/bots/book.py` (1,111 lines) -- read that file's own module
//! doc comment first; it is the design rationale for everything below and is
//! restated here only where the Rust shape earns its own note. Short
//! version: every other bot in this repo is self-play, which only ever
//! proves a bot is good *relative to itself*. `BookBot` is a hand-written,
//! rule-based, ordered priority list -- the strategy human players actually
//! recommend, evaluated top to bottom until one rule fires -- so it is an
//! EXTERNAL yardstick rather than another self-play product. It does no
//! search and no lookahead.
//!
//! ## Verdict: real game knowledge, not a CPython hack -- ported, not deleted
//!
//! Unlike `fastcopy.py`/`trial.py` (see `bots/mod.rs`'s module doc comment),
//! nothing in `book.py` is CPython performance machinery: every rule and
//! every rank table is hand-authored strategy opinion, cited to
//! `docs/EVALUATOR_HISTORY.md`/`docs/EXPERT_STRATEGY.md`. The whole file is
//! ported faithfully below, with three exceptions: the repo owner ruled
//! ("fix things as you port") on three things the port found along the way,
//! fixed identically in `book.py` so the two engines keep agreeing:
//!
//! * **`_r_play_leader` used to rank with the bare v1 `LEADER_RANK` table
//!   even under `version >= 2`.** Every OTHER leader-ranking call site
//!   (`card_value`, transitively `best_take`/`best_develop`) goes through
//!   [`leader_rank`], which switches to `V2_LEADER_RANK`/`v2_homer` under
//!   `ctx.version >= 2`. This was a plain inconsistent-lookup bug -- e.g. a
//!   `version=2` bot used to prefer Julius Caesar (v1 rank 8) over
//!   Alexander the Great (v1 rank 5) even though the v2 tournament table
//!   ranks them the other way round (3.0 vs 5.5, "in the new TTA he is
//!   terrible"). `r_play_leader` now calls [`leader_rank`] like every other
//!   site; see `tests::r_play_leader_routes_through_the_version_aware_table`.
//! * **`V2_TUNABLES["iron_over_bronze"]` and `_Ctx.ca`/`_Ctx.ma`/
//!   `_Ctx.rival_best_culture`/`_Ctx.age_row`** were computed every decision
//!   and read by ZERO rules (grepped the whole repo, not just this file --
//!   nothing in `plan.py`/`weighted.py`/`neural_*.py`/`tools/` touched them
//!   either). Deleted rather than carried as dead weight: [`V2Tunables`] no
//!   longer has an `iron_over_bronze` field, and [`Ctx`] no longer has
//!   `ca`/`ma`/`rival_best_culture`/`age_row`. (If a rule was meant to price
//!   the Bronze-vs-Iron upgrade decision using `iron_over_bronze`, that rule
//!   never existed -- a missing rule, not just an unread tunable; reported,
//!   not invented, in the fix-up report.)
//! * **Winston Churchill's once-per-turn choice (`("churchill", ...)` /
//!   [`crate::moves::Move::Churchill`]) used to be referenced by none of the
//!   12 rules in `_action_phase`.** `h_churchill`/`_h_churchill` never
//!   spends a civil or military action (only the once-per-turn flag itself,
//!   lost for the turn either way), so taking it is strictly better than
//!   not -- never taking a free, no-downside bonus was a plain bug. Fixed
//!   with a 13th rule, [`r_churchill`], appended after `r_take_card`: it
//!   always takes the bonus when offered. WHICH of the two flavours to take
//!   remains a genuine, unresolved strategy question -- culture is
//!   unconditional value, the military discount is wiped at end of turn if
//!   nothing military gets bought or upgraded afterwards -- so `r_churchill`
//!   always defaults to the risk-free flavour (culture) rather than
//!   guessing at a same-turn military purchase.
//!
//! One more thing found along the way is genuinely NOT a bug, unlike the
//! three above, and stays exactly as it was:
//!
//! * **`self.rng` is dead in Python.** `BookBot.__init__` stores
//!   `rng or random.Random(seed)`, and NOTHING in `choose`/`_action_phase`/
//!   `_politics`/`_pending` or any rule function ever reads `self.rng`
//!   (every tie-break is a deterministic `max`/`min`/`sorted`). This port
//!   carries no rng field at all: [`BookBot`] is a pure function of
//!   `(state, moves)`, which is the true behaviour, not a smaller one.
//!
//! ## This port is generic over the champion evaluator, deliberately
//!
//! `BookImprovedBot` (Python: `weighted.WeightedBot` as `self.inner`) needs
//! the trained champion's full move-choosing policy, and `weighted.rs` is
//! not ported (`~4,100` lines, `evaluate`/`features`/`rival_context`) --
//! exactly the situation `quiescent.rs`'s own module doc comment describes
//! for its scorer. [`BookImprovedBot::choose`] therefore takes the champion
//! as `champion: impl Fn(&GameState, &[Move]) -> Move`, the full-policy
//! analogue of `quiescent.rs`'s `eval: &impl Fn(&GameState, u8) -> f64`: the
//! POLICY (which move kinds the book overrules, see [`is_override_kind`])
//! lives here, once; the SCORING/SEARCH that produces the champion's own
//! pick stays wherever `weighted.rs` eventually lands.
//!
//! ## Rank tables: name-keyed consts, typo-checked by a test, not the compiler
//!
//! `LEADER_RANK`/`WONDER_RANK`/`SPECIAL_RANK`/`TACTIC_RANK`/`V2_LEADER_RANK`/
//! `V2_WONDER_RANK`/`V2_NEVER_TAKE` are `&'static [(&'static str, f64)]`
//! (or `&[&str]`) consts, one line per Python dict entry, looked up by
//! [`rank_of`] via [`CardId::name`] -- a linear scan over a handful of
//! entries, the same shape `bots::counting::lookup` already uses for its own
//! short `Vec<(CardId, _)>` pairs. This is deliberately NOT a `CardId`-keyed
//! table resolved at startup: `CardId::by_name` is a runtime `fn` (a linear
//! scan over `CARDS`, see `cards.rs`), not a `const fn`, and `CARDS` itself
//! is a `static` (generated by `gen_cards.py`), so there is no way to
//! resolve a card name to a `CardId` inside a `const` initializer without
//! new compile-time machinery this port has no license to add. The closest
//! available substitute to "a typo is a compile error" is `tests::
//! every_rank_table_name_resolves_to_a_real_card`, which walks every entry
//! of every table below through `CardId::by_name` and panics on the first
//! miss -- a typo becomes a `cargo test` failure that runs on every commit,
//! not a silently-defaulted rank discovered by a strength regression months
//! later.
//!
//! ## `pub(crate)` widenings for `bots::variants`
//!
//! `bots::variants` (the `engine/bots/variants/` port) is BookBot's v2 rule
//! list with a per-archetype knob profile bolted on -- literally a subclass
//! in Python. Rust has no inheritance, so it cannot subclass this module;
//! instead it reuses the pieces that are identical across every archetype
//! (the rank tables, [`leader_rank`], [`wonder_value`], [`gov_value`],
//! [`action_card_value`], [`best_level_in`], the small lookup helpers, and
//! [`BookBot::pending_pick`] for the pending-decision phase, which no Python
//! variant overrides either) and writes its own profile-aware replacements
//! for the handful of rules that a profile actually changes (`prod_value`,
//! `card_value`, `best_build`/`best_develop`/`best_take`, `r_wonder_step`,
//! `r_military_floor`, `r_population`, `r_upgrade`, `r_play_leader`,
//! `r_revolution`, `politics`). The items below were widened from
//! module-private to `pub(crate)` for exactly this reuse, no wider -- nothing
//! outside this crate's `bots` tree needs them.
use crate::card_table::{FreeCivilActionValue, Special};
use crate::cards::{CardId, CardType, Production};
use crate::costs;
use crate::economy;
use crate::effects::{self, Stats};
use crate::interact;
use crate::moves::{ChurchillChoice, Move, MoveList, PactSide};
use crate::state::{
    Auction, Choice, ChoiceKind, ChoiceOption, Colonize, Defense, GameState, Keyword, Pending, Phase,
    PlayerState,
};

// --------------------------------------------------------------- card ranks

/// Leaders, v1 (the opinion-derived table). `_r_play_leader` reads this
/// table DIRECTLY rather than through [`leader_rank`] -- see this module's
/// top doc comment's "Verdict" section for why that asymmetry is kept.
const LEADER_RANK: &[(&str, f64)] = &[
    ("Hammurabi", 9.0),
    ("Julius Caesar", 8.0),
    ("Aristotle", 8.0),
    ("Moses", 7.0),
    ("Homer", 6.0),
    ("Alexander the Great", 5.0),
    ("Michelangelo", 9.0),
    ("Leonardo da Vinci", 8.0),
    ("Joan of Arc", 7.0),
    ("Frederick Barbarossa", 7.0),
    ("Genghis Khan", 6.0),
    ("Christopher Columbus", 5.0),
    ("Isaac Newton", 9.0),
    ("J. S. Bach", 8.0),
    ("William Shakespeare", 7.0),
    ("Napoleon Bonaparte", 7.0),
    ("Maximilien Robespierre", 6.0),
    ("James Cook", 5.0),
];

/// Wonders, v1. Traded off against total resource cost by [`wonder_value`].
const WONDER_RANK: &[(&str, f64)] = &[
    ("Pyramids", 9.0),
    ("Library of Alexandria", 8.0),
    ("Hanging Gardens", 7.0),
    ("Colossus", 5.0),
    ("St. Peter's Basilica", 8.0),
    ("Taj Mahal", 7.0),
    ("Universitas Carolina", 6.0),
    ("Great Wall", 6.0),
    ("Eiffel Tower", 8.0),
    ("Kremlin", 7.0),
    ("Transcontinental Railroad", 6.0),
    ("Ocean Liners", 5.0),
    ("First Space Flight", 8.0),
    ("Internet", 7.0),
    ("Hollywood", 7.0),
    ("Fast Food Chains", 6.0),
];

/// Special technologies, by the size of the permanent bonus.
pub(crate) const SPECIAL_RANK: &[(&str, f64)] = &[
    ("Code of Laws", 9.0),
    ("Masonry", 8.0),
    ("Warfare", 7.0),
    ("Cartography", 5.0),
];

/// Tactics we would rather hold than not, in strength order.
pub(crate) const TACTIC_RANK: &[(&str, f64)] = &[
    ("Medieval Army", 6.0),
    ("Heavy Cavalry", 5.0),
    ("Legion", 4.0),
    ("Phalanx", 3.0),
    ("Fighting Band", 2.0),
];

/// Rank on the same ~0-10 scale v1 uses, derived from tournament CA-spend and
/// win-correlation annotations (`docs/EXPERT_STRATEGY.md`). Ages A/I/II.
pub(crate) const V2_LEADER_RANK: &[(&str, f64)] = &[
    ("Hammurabi", 9.0),
    ("Aristotle", 8.0),
    ("Alexander the Great", 5.5),
    ("Julius Caesar", 3.0),
    ("Moses", 3.0),
    ("Joan of Arc", 9.0),
    ("Frederick Barbarossa", 6.5),
    ("Genghis Khan", 5.5),
    ("Leonardo da Vinci", 5.0),
    ("Christopher Columbus", 3.0),
    ("Michelangelo", 2.0),
    ("Napoleon Bonaparte", 10.0),
    ("Isaac Newton", 9.0),
    ("James Cook", 6.0),
    ("William Shakespeare", 6.0),
    ("Maximilien Robespierre", 4.0),
    ("J. S. Bach", 4.5),
];

/// Homer is the clearest 2p-vs-multiplayer split in the source. Default
/// (an unlisted player count) is `6.0`, matching Python's
/// `V2_HOMER.get(ctx.nplayers, 6.0)`.
pub(crate) fn v2_homer(nplayers: u8) -> f64 {
    match nplayers {
        2 => 5.0,
        3 => 7.5,
        4 => 7.5,
        _ => 6.0,
    }
}

pub(crate) const V2_WONDER_RANK: &[(&str, f64)] = &[
    ("Library of Alexandria", 9.0),
    ("Pyramids", 8.5),
    ("Colossus", 3.0),
    ("Hanging Gardens", 6.5),
    ("St. Peter's Basilica", 10.0),
    ("Great Wall", 10.0),
    ("Universitas Carolina", 7.0),
    ("Taj Mahal", 7.0),
];

/// Cards strong players essentially never pay for (`docs/EXPERT_STRATEGY.md`).
pub(crate) const V2_NEVER_TAKE: &[&str] =
    &["Stock Pile", "Patriotism (A)", "Cultural Heritage (A)", "Frugality (A)", "Frugality (I)"];

/// The card row charges 1 CA for slots 1-5, 2 for 6-9, 3 for 10-13. 76% of
/// tournament Age I picks were made at 1 CA and only 2.5% at 3 CA, so the
/// price ladder is convex, not linear. Default (any other cost) is `12.0`.
pub(crate) fn v2_price_ladder(cost: i32) -> f64 {
    match cost {
        0 => 0.0,
        1 => 1.0,
        2 => 3.5,
        3 => 9.0,
        _ => 12.0,
    }
}

/// `rank_of(TABLE, id, default)` -- the read side of every name-keyed rank
/// table above. Linear scan over a handful of entries; see this module's top
/// doc comment for why this is name-keyed rather than `CardId`-keyed.
pub(crate) fn rank_of(table: &[(&str, f64)], id: CardId, default: f64) -> f64 {
    let name = id.name();
    table.iter().find(|&&(n, _)| n == name).map_or(default, |&(_, v)| v)
}

/// Resolve a card name used by this module's own logic (not a rank table --
/// see [`leader_rank`]'s Leonardo/Columbus branches). Mirrors the
/// `CardId::by_name(name).unwrap_or_else(|| panic!(...))` idiom used
/// throughout this codebase (`bots::counting::homer`, `combat.rs`,
/// `costs.rs`, `interact.rs`).
///
/// # Panics
/// If `card_table.rs` has no card named `name`.
pub(crate) fn card(name: &str) -> CardId {
    CardId::by_name(name).unwrap_or_else(|| panic!("bots::book: no card named {name:?}"))
}

// --------------------------------------------------------- V2 tunables

/// Contested calls, exposed as parameters instead of decided by fiat.
/// `docs/EXPERT_STRATEGY.md` keeps a deliberately unresolved-disagreements
/// table; these are its entries. Mirrors Python's `V2_TUNABLES`: "TUNABLE"
/// describes the PARAMETER, not this struct -- no caller in the Python repo
/// ever actually overrides one, and none does here either, so every value
/// below is a frozen literal / fitted prior, not a live knob anything tunes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct V2Tunables {
    /// "Library of Alexandria wins more than Pyramids" vs "Pyramids taken
    /// 20% more often". >1 favours the Pyramids' extra civil action.
    pub pyramids_vs_loa: f64,
    /// Highest Age I tournament spend (0.59) but "arguably the weakest Age I
    /// wonder" on culture-opportunity math. Scales its rank.
    pub great_wall: f64,
    /// Tournament 0.00 and "utterly useless" vs "+20 culture over Great
    /// Wall" and the 30k dataset saying strong players undervalue it.
    pub taj_mahal: f64,
    /// Tier 3 at 0.03, but happiness genuinely solves an early problem.
    pub hanging_gardens: f64,
    // ("iron_over_bronze" used to live here -- "upgrade to Iron only if it
    // lands early in Age I" -- but no rule anywhere consulted it; deleted
    // rather than carried as dead weight, see this module's top doc
    // comment.)
    /// Theology was selected 0 times in 39 tournament games; happiness
    /// cards let you skip it. Multiplies its value.
    pub theology: f64,
}

impl Default for V2Tunables {
    fn default() -> Self {
        V2Tunables {
            pyramids_vs_loa: 0.95,
            great_wall: 0.6,
            taj_mahal: 0.7,
            hanging_gardens: 0.6,
            theology: 0.15,
        }
    }
}

// ------------------------------------------------------------------ context

/// The dozen numbers every rule below reads. Computed once per decision.
/// Mirrors `_Ctx`. `ca`/`ma`/`rival_best_culture`/`age_row` used to live
/// here too -- computed every decision, read by zero rules -- and were
/// deleted rather than carried as dead weight; see this module's top doc
/// comment.
#[derive(Clone, Copy, Debug)]
pub struct Ctx {
    pub version: u8,
    pub tun: V2Tunables,
    pub nplayers: u8,
    /// `AGE_IDX`: A=0, I=1, II=2, III=3, IV=4 -- exactly `Age`'s own
    /// discriminants, so this is `state.age_civil as u8` directly.
    pub age: u8,
    pub rnd: u16,
    pub last: bool,
    pub workers_free: u8,
    pub food_need: i32,
    pub res_need: i32,
    pub happy_gap: i32,
    pub strength: i32,
    pub mil_target: i32,
    /// "Late" = stop investing in economy that will not pay back.
    pub late: bool,
    pub s: Stats,
}

impl Ctx {
    pub fn new(state: &GameState, p_idx: u8, version: u8, tun: V2Tunables) -> Ctx {
        let p = &state.players[p_idx as usize];
        let s = effects::state_stats(state, p);
        let age = state.age_civil as u8;
        let last = state.last_round;

        // Food: the book target is a couple of food a turn spare so
        // population can keep growing.
        let food_need = (economy::consumption(p.yellow_bank) as i32 + 2 - s.food).max(0);
        // Resources: corruption punishes a big pile, so the target is a
        // healthy rate rather than a stockpile.
        let res_need = (3 + age as i32 - s.resources).max(0);
        // Happiness: discontent costs a worker, worse a full uprising.
        let happy_gap = economy::happy_required(p.yellow_bank) as i32 - s.happy;

        // Military target: heads-up, match the only rival; multiplayer,
        // match the second-strongest and stay within 3 of the strongest --
        // "don't be the softest target".
        let mut rival_strength = [0i32; 3];
        let mut n_rivals = 0usize;
        for q in state.players[..state.num_players as usize].iter() {
            if q.idx == p.idx || q.resigned {
                continue;
            }
            if n_rivals < rival_strength.len() {
                rival_strength[n_rivals] = effects::state_stats(state, q).strength;
                n_rivals += 1;
            }
        }
        rival_strength[..n_rivals].sort_unstable();
        let mil_target = match n_rivals {
            0 => 0,
            1 => rival_strength[0],
            _ => rival_strength[n_rivals - 2].max(rival_strength[n_rivals - 1] - 3),
        };

        Ctx {
            version,
            tun,
            nplayers: state.num_players,
            age,
            rnd: state.round,
            last,
            workers_free: p.workers_free,
            food_need,
            res_need,
            happy_gap,
            strength: s.strength,
            mil_target,
            late: last || age >= 3,
            s,
        }
    }
}

// ------------------------------------------------------------- value tables

/// Book value of a per-turn production block, by game phase. Early, food and
/// resources are worth more than culture because they buy the engine that
/// makes culture later; in Age III the reverse is true.
fn prod_value(prod: Production, ctx: &Ctx) -> f64 {
    let early = ctx.age <= 1;
    let mut v = 0.0;
    v += prod.food as f64 * if early { 3.0 } else { 1.5 };
    v += prod.resources as f64 * if early { 3.0 } else { 2.0 };
    v += prod.science as f64 * if !ctx.late { 3.5 } else { 1.0 };
    v += prod.culture as f64 * if early { 2.0 } else { 4.5 };
    v += prod.happy as f64 * if ctx.happy_gap >= 0 { 2.5 } else { 1.0 };
    v += prod.strength as f64 * 1.5;
    v
}

/// Book rank of a leader. v1 uses the opinion-derived table; v2 uses the
/// empirical tournament CA-spend ordering, with the two conditional leaders
/// the sources insist on (Leonardo needs a lab/library, Columbus needs a
/// colony) and the one value the sources split by player count (Homer).
pub(crate) fn leader_rank(id: CardId, ctx: &Ctx, p: Option<&PlayerState>) -> f64 {
    if ctx.version < 2 {
        return rank_of(LEADER_RANK, id, 5.0);
    }
    if id.name() == "Homer" {
        return v2_homer(ctx.nplayers);
    }
    let mut rank = rank_of(V2_LEADER_RANK, id, 5.0);
    if let Some(p) = p {
        if id.name() == "Leonardo da Vinci" {
            // "Don't take him without either Alchemy or Printing Press."
            let alchemy = card("Alchemy");
            let printing_press = card("Printing Press");
            let have = p.techs.has(alchemy)
                || p.hand_civil.contains(alchemy)
                || p.techs.has(printing_press)
                || p.hand_civil.contains(printing_press);
            rank = if have { 8.5 } else { 2.0 };
        } else if id.name() == "Christopher Columbus" {
            // "Only take with a colony already in hand. Do not take
            // speculatively."
            let has_colony = p.hand_military.as_slice().iter().any(|&c| c.kind() == CardType::Territory);
            rank = if has_colony { 8.0 } else { 1.5 };
        }
    }
    rank
}

/// Rank, discounted by how many resources the wonder still costs.
pub(crate) fn wonder_value(id: CardId, ctx: &Ctx) -> f64 {
    let card = id.get();
    let stages = card.stages;
    let total: i32 = stages.iter().map(|&s| s as i32).sum();
    let rank = if ctx.version >= 2 {
        let mut r = rank_of(V2_WONDER_RANK, id, 5.0);
        match id.name() {
            "Pyramids" => r *= ctx.tun.pyramids_vs_loa,
            "Great Wall" => r *= ctx.tun.great_wall,
            "Taj Mahal" => r *= ctx.tun.taj_mahal,
            "Hanging Gardens" => r *= ctx.tun.hanging_gardens,
            _ => {}
        }
        r
    } else {
        rank_of(WONDER_RANK, id, 5.0)
    };
    // Age III wonders are one-shot culture bombs; only worth starting if
    // there is time left to finish them.
    if ctx.last && stages.len() > 1 {
        return 0.0;
    }
    rank * 2.0 - total as f64 * 0.5
}

/// How much the book wants this card, used for taking and developing.
fn card_value(_state: &GameState, p: &PlayerState, ctx: &Ctx, id: CardId) -> f64 {
    let c = id.get();
    match c.kind {
        CardType::Leader => leader_rank(id, ctx, Some(p)) * 1.6,
        CardType::Wonder => wonder_value(id, ctx),
        CardType::Government => gov_value(p, ctx, id),
        CardType::SpecialTech => {
            let mut base = rank_of(SPECIAL_RANK, id, 4.0) * 1.5;
            if c.name == "Masonry" && p.wonder.is_none() && p.completed_wonders.is_empty() {
                base *= 0.6; // no wonder to discount
            }
            base
        }
        CardType::Action => action_card_value(p, ctx, id),
        _ if c.kind.takes_workers() => {
            let mut v = prod_value(c.production, ctx);
            if ctx.version >= 2 && c.name == "Theology" {
                // Selected exactly 0 times in 39 tournament games.
                v *= ctx.tun.theology;
            }
            // A technology is only worth what the buildings on it produce,
            // so a tech we cannot afford to staff is worth much less.
            let lvl = id.level() as i32;
            let best = best_level_in(p, c.kind);
            if lvl <= best {
                return 0.0; // never a downgrade
            }
            if c.kind.is_unit() {
                v = c.effects.strength as f64 * 2.0;
                if ctx.strength >= ctx.mil_target {
                    v *= 0.5; // already safe: units are a tax
                }
            }
            // Late in the game a new tech will not pay for its own build cost.
            if ctx.late && !matches!(c.kind, CardType::Theater | CardType::Temple | CardType::Library) {
                v *= 0.5;
            }
            v
        }
        CardType::Farm | CardType::Mine | CardType::Lab | CardType::Temple | CardType::Library | CardType::Arena | CardType::Theater | CardType::Infantry | CardType::Cavalry | CardType::Artillery | CardType::Air | CardType::Tactic | CardType::Aggression | CardType::War | CardType::Pact | CardType::Bonus | CardType::Territory | CardType::Event => 1.0,
    }
    // `_state` is never read here, matching Python's `_card_value`, whose
    // own `state` argument is likewise unused -- kept as a parameter only to
    // match every other `_r_*`/`_best_*` call site's shape.
}

pub(crate) fn best_level_in(p: &PlayerState, typ: CardType) -> i32 {
    p.techs.iter().filter(|&(id, _)| id.kind() == typ).map(|(id, _)| id.level() as i32).max().unwrap_or(-1)
}

/// Governments are bought for actions, not for their culture.
pub(crate) fn gov_value(p: &PlayerState, ctx: &Ctx, id: CardId) -> f64 {
    let c = id.get();
    let cur = p.government.get();
    let gain_ca = c.effects.civil_actions as i32 - cur.effects.civil_actions as i32;
    let gain_ma = c.effects.military_actions as i32 - cur.effects.military_actions as i32;
    if gain_ca <= 0 && gain_ma <= 0 {
        return 0.0;
    }
    // An extra civil action every remaining turn is the single most valuable
    // thing in the game early, and worth almost nothing on the last turn.
    let horizon = if ctx.late { 1.0 } else { 3.0 };
    (gain_ca as f64 * 4.0 + gain_ma as f64 * 1.5) * horizon
}

/// The `freeCivilAction` string payload a card prints, if any. Mirrors
/// Python's `eff.get("freeCivilAction")`.
fn free_civil_action_of(id: CardId) -> Option<FreeCivilActionValue> {
    id.get().special.iter().find_map(|s| match s {
        Special::FreeCivilAction(v) => Some(*v),
        Special::A(_) | Special::AllPlayers(_) | Special::B(_) | Special::BestTheaterDoubleCulture | Special::BothPlayers(_) | Special::BuildDiscount(_) | Special::CancelledIfPartiesAttackEachOther | Special::CannotPlayAggressionOrWar | Special::CivilActionBackOnTechDevelop(_) | Special::CivilActionUpgradeUrbanBuildingToTheater | Special::ColonizeDiscardUpTo2MilitaryCardsForBonus(_) | Special::ColonyImmediateBonusApplies | Special::ColonyPermanentBonusTransfers | Special::ComboFoodDiscount(_) | Special::ComboResourceDiscount(_) | Special::Condition(_) | Special::CultureFirstColony(_) | Special::CultureIfTopTwoStrength(_) | Special::CultureOnLeaveEqualToLabResourceProduction | Special::CultureOnRevolution(_) | Special::CultureOnTechDevelop(_) | Special::CulturePerAdditionalColony(_) | Special::CulturePerCivilizationWithMoreCulture(_) | Special::CulturePerHappyFromTemplesTheatersWonders(_) | Special::CulturePerLabEqualToLevel | Special::CulturePerLibraryTheaterPair(_) | Special::CulturePerTheater(_) | Special::DecreasePopulation(_) | Special::DestroyUrbanBuildings(_) | Special::DoubleBestMine | Special::DoublesTacticBonusOfOneArmy | Special::ExtraHappyPerHappySource(_) | Special::FinalScoring(_) | Special::FreePopIncreasePerTurn | Special::Gain(_) | Special::GainCulturePerLevelOfRemovedCard(_) | Special::GainFoodOrResources(_) | Special::GainResources(_) | Special::InfantryCountsAsCavalryForTactics | Special::LastRoundSubstitute(_) | Special::LeaderTakeCivilActionDiscount(_) | Special::LibraryDiscountsIfTheater | Special::Lose(_) | Special::MilitaryActionAsCivilPerTurn(_) | Special::MilitaryActionCombinedPopIncreaseAndUnitBuild | Special::NoAttacksBetweenParties | Special::OnAttackBetweenParties(_) | Special::OnBuildCulture(_) | Special::OnBuildCulturePerTechLevelSum | Special::OnReplacePutUnderCompletedWonderHappy(_) | Special::OncePerGameTwoPoliticalActions | Special::OpponentDecreasesPopulation(_) | Special::OpponentsPayDoubleMilitaryActionsToAttackYou | Special::OrTakesSpecialTechnologiesOfSameTotalScienceCost | Special::PeekTopEventCardInPolitics | Special::PerTurnChoice | Special::PlayerWithLeastCulture(_) | Special::PlayerWithMostCulture(_) | Special::PlayersWithMostDiscontentWorkers(_) | Special::PlayersWithMostHappyFaces(_) | Special::PopIncreaseFoodDiscount(_) | Special::RemoveAsPoliticalActionForYellowToken(_) | Special::RemoveAsPoliticalActionFreeColonize | Special::RemoveFromGame | Special::ResourceOnMilitaryUnitBuildOrUpgrade(_) | Special::ResourceOnTechDevelop(_) | Special::ResourcesForMilitaryUnitsPerStrongerCivilization(_) | Special::ResourcesPerLabEqualToLevel | Special::RevolutionUsesMilitaryActionsInstead | Special::ScienceOnTechCardTake(_) | Special::SciencePerBestLabOrLibraryLevel | Special::SciencePerLab(_) | Special::StealColony(_) | Special::StrengthPerArtillery(_) | Special::StrengthPerInfantry(_) | Special::StrengthPerMilitaryUnit(_) | Special::StrengthPerTempleOrGovernmentHappy(_) | Special::StrengthPerUnitType(_) | Special::StrongestPlayer(_) | Special::StrongestPlayers(_) | Special::TakeCivilActionDiscountIfLeaderReplacedThisTurn(_) | Special::TakeFromOpponent(_) | Special::TheaterResourceDiscountIfLibrary(_) | Special::TheaterScienceDiscountIfLibrary(_) | Special::TheaterTechScienceDiscount(_) | Special::VictorTakesCulture | Special::VictorTakesScienceUpTo(_) | Special::VictorTakesYellowTokens | Special::WeakestPlayer(_) | Special::WeakestPlayers(_) | Special::WonderTakeNoExtraCivilActions => None,
    })
}

/// Action cards are tempo: worth taking only if the free action is one you
/// were going to spend a civil action on anyway.
pub(crate) fn action_card_value(p: &PlayerState, ctx: &Ctx, id: CardId) -> f64 {
    let eff = &id.get().effects;
    let mut v = 0.0;
    v += eff.gain_culture as f64 * if !ctx.late { 1.0 } else { 2.0 };
    v += eff.gain_science as f64 * 1.5;
    v += eff.gain_food as f64;
    v += eff.gain_resources as f64 * 1.2;
    if let Some(free) = free_civil_action_of(id) {
        use FreeCivilActionValue as V;
        match free {
            V::BuildOneWonderStage => v += if !p.wonder.is_none() { 6.0 } else { 1.0 },
            V::IncreasePopulation => v += 4.0,
            V::BuildOrUpgradeFarmOrMine => v += 5.0,
            V::BuildOrUpgradeUrbanBuilding => v += 5.0,
            V::DevelopTechnology | V::UpgradeFarmMineOrUrbanBuilding => {}
        }
    }
    if eff.military_actions != 0 {
        v += 2.0;
    }
    v
}

// ---------------------------------------------------------------- the bot

/// Rule-based "received wisdom" opponent. Pure function of `(state, moves)`
/// -- see this module's top doc comment for why this port carries no rng
/// field, unlike Python's `BookBot`.
#[derive(Clone, Copy, Debug)]
pub struct BookBot {
    /// 1 = the opinion-derived rules; 2 = the empirical tournament tier list
    /// (see [`V2_LEADER_RANK`]/[`V2_WONDER_RANK`]/[`v2_price_ladder`]).
    pub version: u8,
    pub tunables: V2Tunables,
}

impl Default for BookBot {
    fn default() -> Self {
        BookBot { version: 1, tunables: V2Tunables::default() }
    }
}

type Rule = fn(&GameState, &PlayerState, &Ctx, &[Move]) -> Option<Move>;

/// The priority list `_action_phase` walks, in order. Every function here
/// mirrors its `_r_*` twin exactly; see each one's own doc comment.
const RULES: [Rule; 13] = [
    r_round1,
    r_revolution,
    r_play_leader,
    r_happiness,
    r_military_floor,
    r_wonder_step,
    r_population,
    r_place_worker,
    r_upgrade,
    r_develop,
    r_action_card,
    r_take_card,
    r_churchill,
];

impl BookBot {
    /// Entry point. `moves` should be exactly what [`crate::legal::
    /// legal_moves`] returns for `state` -- callers must have generated the
    /// move list themselves, the same split `quiescent.rs` and `pending.rs`
    /// use throughout this port (move GENERATION lives in `legal.rs`/
    /// `interact.rs`; this module only SELECTS).
    ///
    /// # Panics
    /// If `moves` is empty. Matches every other bot in this port
    /// (`quiescent.rs::pick_one`'s `unwrap_or(moves[0])` panics identically):
    /// a live game's [`crate::legal::legal_moves`] never returns an empty
    /// list, so an empty slice here is a caller bug, not a state to degrade
    /// gracefully from.
    pub fn choose(&self, state: &GameState, moves: &[Move]) -> Move {
        let filtered = super::filter_resign(moves, false);
        let moves = filtered.as_slice();
        if moves.len() == 1 {
            return moves[0];
        }
        if !state.pending.is_empty() {
            return self.pending_pick(state, moves);
        }
        let p_idx = state.actor().idx;
        let p = &state.players[p_idx as usize];
        let ctx = Ctx::new(state, p_idx, self.version, self.tunables);
        if state.phase == Phase::Politics {
            politics(state, p, &ctx, moves)
        } else {
            action_phase(state, p, &ctx, moves)
        }
    }

    pub(crate) fn pending_pick(&self, state: &GameState, moves: &[Move]) -> Move {
        let p_idx = state.actor().idx;
        let p = &state.players[p_idx as usize];
        let ctx = Ctx::new(state, p_idx, self.version, self.tunables);
        match state.pending.top().expect("pending_pick called with an empty pending stack") {
            Pending::Choice(c) => choice(state, p, &ctx, c),
            Pending::Auction(a) => auction(a, moves),
            Pending::Defense(d) => defense(d, moves),
            Pending::Colonize(col) => colonize(state, p, col, moves),
        }
    }
}

/// `_action_phase`: walk [`RULES`] top to bottom, first hit wins; `end_turn`
/// if nothing fires.
fn action_phase(state: &GameState, p: &PlayerState, ctx: &Ctx, moves: &[Move]) -> Move {
    for rule in RULES {
        if let Some(mv) = rule(state, p, ctx, moves) {
            return mv;
        }
    }
    Move::EndTurn
}

// rule 0 -------------------------------------------------------------

/// Round 1: taking cards is the only legal action. The book opening is a
/// leader first, then a wonder worth finishing, then tempo cards -- and
/// never a card from the expensive end of the row on turn one.
fn r_round1(state: &GameState, p: &PlayerState, ctx: &Ctx, moves: &[Move]) -> Option<Move> {
    if state.round != 1 {
        return None;
    }
    if !moves.iter().any(|m| matches!(m, Move::Take { .. })) {
        return None;
    }
    best_take(state, p, ctx, moves, true)
}

// rule 1 -------------------------------------------------------------

/// Revolution costs the whole turn's civil actions, so it only pays while
/// there are many turns left to spend the extra action on.
fn r_revolution(_state: &GameState, p: &PlayerState, ctx: &Ctx, moves: &[Move]) -> Option<Move> {
    if ctx.late || ctx.age >= 2 {
        return None;
    }
    let mut best: Option<Move> = None;
    let mut best_v = 0.0;
    for &m in moves {
        if let Move::Revolution { card } = m {
            let v = gov_value(p, ctx, card);
            if v > best_v {
                best = Some(m);
                best_v = v;
            }
        }
    }
    // Only worth burning a turn of actions for a real gain, and only early
    // enough that the gain compounds.
    if best_v >= 10.0 && ctx.rnd <= 8 {
        best
    } else {
        None
    }
}

// rule 2 -------------------------------------------------------------

/// Play a leader the moment you have one: an unplayed leader is a card that
/// did nothing all game.
///
/// Ranked through [`leader_rank`], the same version-aware helper every
/// other leader-ranking call site uses (`card_value`, transitively
/// `best_take`/`best_develop`). This rule used to read the bare v1
/// `LEADER_RANK` table directly regardless of `ctx.version`, so a
/// `version=2` bot picked WHICH leader in hand to play from the v1 opinion
/// table while it decided everything else about leaders from the v2 one --
/// an inconsistent lookup, not a deliberate choice; fixed here (and in
/// `book.py::_r_play_leader` identically). See
/// `tests::r_play_leader_routes_through_the_version_aware_table`.
fn r_play_leader(_state: &GameState, p: &PlayerState, ctx: &Ctx, moves: &[Move]) -> Option<Move> {
    let mut best: Option<Move> = None;
    let mut best_key: (f64, &str) = (0.0, "");
    for &m in moves {
        if let Move::PlayLeader { card } = m {
            let key = (leader_rank(card, ctx, Some(p)), card.name());
            if best.is_none() || key > best_key {
                best = Some(m);
                best_key = key;
            }
        }
    }
    let (best_mv, best_card) = match best {
        Some(m @ Move::PlayLeader { card }) => (m, card),
        _ => return None,
    };
    if p.leader.is_none() {
        return Some(best_mv);
    }
    // Replacing a leader is only right for a clear upgrade.
    if leader_rank(best_card, ctx, Some(p)) >= leader_rank(p.leader, ctx, Some(p)) + 2.0 {
        return Some(best_mv);
    }
    None
}

// rule 3 -------------------------------------------------------------

/// Discontent costs a worker every turn and an uprising is a catastrophe;
/// fix it before anything else.
fn r_happiness(state: &GameState, p: &PlayerState, ctx: &Ctx, moves: &[Move]) -> Option<Move> {
    if ctx.happy_gap <= 0 {
        return None;
    }
    best_build(state, p, ctx, moves, true).or_else(|| best_develop(state, p, ctx, moves, true))
}

// rule 4 -------------------------------------------------------------

/// Never be the weakest civilisation at the table -- weak military is a
/// standing invitation for every aggression in the game.
fn r_military_floor(_state: &GameState, p: &PlayerState, ctx: &Ctx, moves: &[Move]) -> Option<Move> {
    // A tactic multiplies what the units you already have are worth, so it
    // comes before buying more units.
    if p.tactic.is_none() {
        let mut best: Option<Move> = None;
        let mut best_v = 0.0;
        for &m in moves {
            if let Move::PlayTactic { card } = m {
                let v = rank_of(TACTIC_RANK, card, 0.0);
                if best.is_none() || v > best_v {
                    best = Some(m);
                    best_v = v;
                }
            }
        }
        if let Some(m) = best {
            return Some(m);
        }
    }
    if ctx.strength >= ctx.mil_target {
        return None;
    }
    // Upgrade an existing unit first (no new worker needed).
    let mut best: Option<Move> = None;
    let mut best_v = 0.0;
    for &m in moves {
        if let Move::Upgrade { from, to } = m {
            if !from.kind().is_unit() {
                continue;
            }
            let gain = to.get().effects.strength as f64 - from.get().effects.strength as f64;
            if gain > best_v {
                best = Some(m);
                best_v = gain;
            }
        }
    }
    if let Some(m) = best {
        return Some(m);
    }
    if p.workers_free > 0 {
        let mut best: Option<Move> = None;
        let mut best_v = 0.0;
        for &m in moves {
            if let Move::Build { card } = m {
                if !card.kind().is_unit() {
                    continue;
                }
                let v = card.get().effects.strength as f64;
                if v > best_v {
                    best = Some(m);
                    best_v = v;
                }
            }
        }
        if let Some(m) = best {
            return Some(m);
        }
    }
    None
}

// rule 5 -------------------------------------------------------------

/// Finish what you start: a half-built wonder is resources sitting idle, and
/// the culture only arrives on the last stage.
fn r_wonder_step(_state: &GameState, p: &PlayerState, ctx: &Ctx, moves: &[Move]) -> Option<Move> {
    if !moves.iter().any(|m| matches!(m, Move::WonderStep { .. })) {
        return None;
    }
    if !p.wonder.is_none() && wonder_value(p.wonder, ctx) <= 0.0 {
        return None;
    }
    // Take the biggest legal step (Masonry allows two stages per action).
    let mut best: Option<Move> = None;
    let mut best_steps = 0u8;
    for &m in moves {
        if let Move::WonderStep { steps } = m {
            if best.is_none() || steps > best_steps {
                best = Some(m);
                best_steps = steps;
            }
        }
    }
    best
}

// rule 6 -------------------------------------------------------------

/// Grow whenever there is food and a job waiting -- population is the
/// compounding asset -- but never grow into unhappiness.
fn r_population(_state: &GameState, p: &PlayerState, ctx: &Ctx, moves: &[Move]) -> Option<Move> {
    if let Some(m) = moves.iter().find(|m| matches!(m, Move::PopFree)) {
        return Some(*m);
    }
    // Frederick Barbarossa's combined action buys the same worker for a
    // MILITARY action instead of a civil one, a food cheaper, and puts a
    // unit under it. Whenever the book wants to grow and he is in play,
    // that is the way to grow.
    let mut combo_best: Option<Move> = None;
    let mut combo_key = (0i16, "");
    for &m in moves {
        if let Move::Barbarossa { card } = m {
            let key = (card.get().effects.strength, card.name());
            if combo_best.is_none() || key > combo_key {
                combo_best = Some(m);
                combo_key = key;
            }
        }
    }
    let has_pop = moves.iter().any(|m| matches!(m, Move::Pop));
    if combo_best.is_none() && !has_pop {
        return None;
    }
    if ctx.workers_free >= 1 && ctx.age <= 1 {
        return None; // a worker already idle: place it first
    }
    if ctx.workers_free >= 2 {
        return None;
    }
    // Growing raises the happiness requirement one band; do not step into
    // discontent.
    if economy::happy_required(p.yellow_bank.saturating_sub(1)) as i32 > ctx.s.happy {
        return None;
    }
    if ctx.late {
        return None; // a worker bought on the last turns cannot pay back
    }
    if let Some(m) = combo_best {
        return Some(m);
    }
    moves.iter().find(|m| matches!(m, Move::Pop)).copied()
}

// rule 7 -------------------------------------------------------------

/// An idle worker produces nothing. Farms and mines first while the engine
/// is still being built, then the buildings that score.
fn r_place_worker(state: &GameState, p: &PlayerState, ctx: &Ctx, moves: &[Move]) -> Option<Move> {
    if p.workers_free == 0 {
        return None;
    }
    best_build(state, p, ctx, moves, false)
}

// rule 8 -------------------------------------------------------------

/// Upgrading is the efficient move: one civil action, no new worker, and it
/// frees the old card's production immediately.
fn r_upgrade(_state: &GameState, _p: &PlayerState, ctx: &Ctx, moves: &[Move]) -> Option<Move> {
    let mut best: Option<Move> = None;
    let mut best_v = 0.0;
    for &m in moves {
        if let Move::Upgrade { from, to } = m {
            let typ = to.kind();
            if typ.is_unit() {
                continue; // handled by the military rule
            }
            let gain = prod_value(to.get().production, ctx) - prod_value(from.get().production, ctx);
            let cost = (to.get().resource_cost as i32 - from.get().resource_cost as i32).max(0);
            let mut v = gain - cost as f64 * 0.4;
            if typ == CardType::Farm && ctx.food_need <= 0 {
                v -= 2.0;
            }
            if v > best_v {
                best = Some(m);
                best_v = v;
            }
        }
    }
    if best_v >= 1.5 {
        best
    } else {
        None
    }
}

// rule 9 -------------------------------------------------------------

/// Science is only worth what you develop with it. Priority: an extra
/// action, then the economy tech you are short of, then the science engine,
/// then culture buildings.
fn r_develop(state: &GameState, p: &PlayerState, ctx: &Ctx, moves: &[Move]) -> Option<Move> {
    best_develop(state, p, ctx, moves, false)
}

// rule 10 ------------------------------------------------------------

fn r_action_card(_state: &GameState, p: &PlayerState, ctx: &Ctx, moves: &[Move]) -> Option<Move> {
    let mut best: Option<Move> = None;
    let mut best_v = 0.0;
    for &m in moves {
        if let Move::PlayAction { card } = m {
            let v = action_card_value(p, ctx, card);
            if v > best_v {
                best = Some(m);
                best_v = v;
            }
        }
    }
    if best_v >= 3.0 {
        best
    } else {
        None
    }
}

// rule 11 ------------------------------------------------------------

fn r_take_card(state: &GameState, p: &PlayerState, ctx: &Ctx, moves: &[Move]) -> Option<Move> {
    if !moves.iter().any(|m| matches!(m, Move::Take { .. })) {
        return None;
    }
    best_take(state, p, ctx, moves, false)
}

// rule 12 -------------------------------------------------------------

/// Winston Churchill's once-per-turn choice draws down no civil or military
/// action -- `h_churchill`/`_h_churchill` never touch `p.civil_actions`/
/// `p.military_actions`, only the choice itself, which is gone for the turn
/// whether it is spent or not (`p.churchill_used`). Never spending a free,
/// no-downside bonus was a plain bug (no rule referenced `Move::Churchill`
/// at all); fixed here and in `book.py::_r_churchill` identically.
///
/// WHICH of the two flavours to take is a strategy call, and it is DECIDED,
/// not deferred: **this bot always takes culture.** Not a placeholder --
/// don't "improve" it into a conditional here.
///
/// The military flavour (`p.mil_discount`/`p.mil_sci_discount`) is wiped at
/// end of turn (`economy::end_of_turn`), so it pays only if something
/// military gets bought or upgraded afterwards. Whether that happens is not
/// knowable from inside a single rule: this rule fires 13th, meaning nothing
/// else wanted the turn *yet*, but taking the discount can itself unlock a
/// purchase rules 1-12 then make on the next call. Pricing that requires
/// looking ahead at what the discount would buy -- exactly the lookahead
/// this bot exists to do without. Three culture a turn from Age III to the
/// end is worth more than an occasionally-unlocked military purchase, and it
/// can never be worth zero, which the military flavour frequently is.
///
/// `bots::weighted` is where the conditional belongs, and it should be
/// STRUCTURAL rather than a tuned discount for "restricted-ness": price the
/// military flavour as only the part that can actually be spent before it
/// expires -- science short of a military unit tech this player would
/// research, resources short of a military unit it would build, each capped
/// at 3 -- through the existing science/resource weights, against 3 x the
/// culture weight. When nothing military is pending that comes out at
/// exactly 0 and culture wins with no constant to tune. See
/// `board_yields::churchill`, which already floors the card at +3 culture
/// for the same reason.
fn r_churchill(_state: &GameState, _p: &PlayerState, _ctx: &Ctx, moves: &[Move]) -> Option<Move> {
    moves
        .iter()
        .copied()
        .find(|m| matches!(m, Move::Churchill { choice: ChurchillChoice::Culture }))
        .or_else(|| moves.iter().copied().find(|m| matches!(m, Move::Churchill { .. })))
}

// ------------------------------------------------------------ helpers

/// Choose what to put the free worker on.
fn best_build(state: &GameState, _p: &PlayerState, ctx: &Ctx, moves: &[Move], happy_only: bool) -> Option<Move> {
    let _ = state;
    let mut best: Option<Move> = None;
    let mut best_v = 0.0;
    for &m in moves {
        if let Move::Build { card } = m {
            let typ = card.kind();
            if typ.is_unit() {
                continue;
            }
            let c = card.get();
            let prod = c.production;
            if happy_only && prod.happy == 0 {
                continue;
            }
            let mut v = prod_value(prod, ctx);
            // The shortage you actually have is worth more than the one you
            // do not.
            if typ == CardType::Farm {
                v += 3.0 * ctx.food_need.min(2) as f64;
            } else if typ == CardType::Mine {
                v += 3.0 * ctx.res_need.min(2) as f64;
            }
            if prod.happy != 0 && ctx.happy_gap > 0 {
                v += 5.0 * ctx.happy_gap.min(2) as f64;
            }
            v -= c.resource_cost as f64 * 0.5;
            if ctx.late && prod.culture == 0 {
                v -= 3.0;
            }
            if v > best_v {
                best = Some(m);
                best_v = v;
            }
        }
    }
    if best_v > 0.5 {
        best
    } else {
        None
    }
}

fn best_develop(state: &GameState, p: &PlayerState, ctx: &Ctx, moves: &[Move], happy_only: bool) -> Option<Move> {
    let mut best: Option<Move> = None;
    let mut best_v = 0.0;
    for &m in moves {
        if let Move::Develop { card } = m {
            let c = card.get();
            if happy_only && c.production.happy == 0 {
                continue;
            }
            let mut v = card_value(state, p, ctx, card);
            // Science is the scarcest currency early; do not blow it all on
            // one card you cannot staff.
            let cost = costs::tech_cost(state, p, card).unwrap_or(0);
            v -= cost as f64 * 0.6;
            if c.kind.takes_workers() && p.workers_free == 0 && (c.resource_cost as u16) > p.resources {
                v -= 2.0;
            }
            if v > best_v {
                best = Some(m);
                best_v = v;
            }
        }
    }
    if best_v >= 1.0 {
        best
    } else {
        None
    }
}

/// Take cards you will actually play, from the cheap end of the row. The row
/// charges 1/2/3 civil actions by position, and every card also costs an
/// action to play later, so the book warning is to stop taking cards you
/// will never get round to.
fn best_take(state: &GameState, p: &PlayerState, ctx: &Ctx, moves: &[Move], first_turn: bool) -> Option<Move> {
    let hand = p.hand_civil.len() as f64;
    let mut best: Option<Move> = None;
    let mut best_v = 0.0;
    for &m in moves {
        if let Move::Take { slot } = m {
            let name = state.card_row[slot as usize];
            if name.is_none() {
                continue;
            }
            let cost = costs::take_cost(state, p, slot as usize);
            let mut v = card_value(state, p, ctx, name);
            let typ = name.kind();
            if typ == CardType::Wonder && !p.wonder.is_none() {
                continue;
            }
            if ctx.version >= 2 {
                // Cards strong players simply never pay for: every one of
                // these costs a SECOND civil action to realise.
                if V2_NEVER_TAKE.contains(&name.name()) {
                    continue;
                }
                // "No leader warrants spending 3 white points (except Age 3)."
                if typ == CardType::Leader && cost >= 3 && ctx.age < 3 {
                    continue;
                }
                // The two wonders the sources say are only ever worth a
                // bargain price.
                if (name.name() == "Taj Mahal" || name.name() == "Great Wall") && cost > 1 {
                    continue;
                }
                v -= v2_price_ladder(cost) * 3.0;
            } else {
                // A card costs an action now and an action to play: the
                // deeper slots have to clear a much higher bar.
                v -= cost as f64 * 3.0;
            }
            // Cards rot in hand; a full hand is wasted civil actions.
            v -= hand;
            if first_turn {
                // Turn one has 1-4 actions total and nothing else to spend
                // them on, so the bar is lower, but depth still matters.
                v += 4.0;
            }
            if v > best_v {
                best = Some(m);
                best_v = v;
            }
        }
    }
    if best_v > 0.0 {
        best
    } else {
        None
    }
}

// ----------------------------------------------------- politics phase

/// Politics: hit the leader when it is free, avoid wars you might lose, and
/// keep the event deck turning.
fn politics(state: &GameState, p: &PlayerState, ctx: &Ctx, moves: &[Move]) -> Move {
    // Aggressions are pure profit: the engine only offers targets whose
    // strength is already beaten. Point them at whoever is winning.
    if ctx.strength >= ctx.mil_target {
        let mut best: Option<Move> = None;
        let mut best_key = (0u16, "");
        for &m in moves {
            if let Move::Aggression { card, target } = m {
                let key = (state.players[target as usize].culture, card.name());
                if best.is_none() || key > best_key {
                    best = Some(m);
                    best_key = key;
                }
            }
        }
        if let Some(m) = best {
            return m;
        }
    }

    // War is a bet; only take it with a clear strength lead over the target
    // and with time left to collect.
    if !ctx.last {
        let mut best: Option<(Move, u8)> = None;
        let mut best_key = (0u16, "");
        for &m in moves {
            if let Move::War { card, target } = m {
                let key = (state.players[target as usize].culture, card.name());
                if best.is_none() || key > best_key {
                    best = Some((m, target));
                    best_key = key;
                }
            }
        }
        if let Some((m, target)) = best {
            let tgt_strength = effects::state_stats(state, &state.players[target as usize]).strength;
            if ctx.strength >= tgt_strength + 5 {
                return m;
            }
        }
    }

    // A pact is worth having when it is not a gift to a runaway leader.
    let mut best_pact: Option<Move> = None;
    let mut best_pact_key = ("", 0u8, 3u8);
    for &m in moves {
        if let Move::OfferPact { card, target, side } = m {
            if state.players[target as usize].culture <= p.culture {
                let side_rank = match side {
                    PactSide::Unspecified => 0u8,
                    PactSide::A => 1,
                    PactSide::B => 2,
                };
                let key = (card.name(), target, side_rank);
                if best_pact.is_none() || key < best_pact_key {
                    best_pact = Some(m);
                    best_pact_key = key;
                }
            }
        }
    }
    if let Some(m) = best_pact {
        return m;
    }

    let mut best_ev: Option<Move> = None;
    let mut best_ev_name = "";
    for &m in moves {
        if let Move::PrepareEvent { card } = m {
            let name = card.name();
            if best_ev.is_none() || name < best_ev_name {
                best_ev = Some(m);
                best_ev_name = name;
            }
        }
    }
    if let Some(m) = best_ev {
        return m;
    }
    Move::PolPass
}

// -------------------------------------------------- pending decisions

/// A running `(best index, best score)` accumulator for one `choice`
/// decision -- the Rust spelling of the `pick(score)` local closure Python
/// builds fresh per tag. `consider` keeps the FIRST option on a tie, exactly
/// as Python's `if best_v is None or v > best_v` does (strict `>`).
struct Best {
    i: usize,
    v: f64,
    set: bool,
}

impl Best {
    fn new() -> Self {
        Best { i: 0, v: 0.0, set: false }
    }

    fn consider(&mut self, i: usize, v: f64) {
        if !self.set || v > self.v {
            self.i = i;
            self.v = v;
            self.set = true;
        }
    }
}

/// Answer one open `choice` decision. `state`/`p`/`ctx` are exactly what the
/// scoring in each branch below needs -- see `_choice`'s own per-tag
/// comments in `book.py` for the rationale behind each score, restated only
/// where the Rust shape earns its own note.
fn choice(state: &GameState, p: &PlayerState, ctx: &Ctx, c: &Choice) -> Move {
    let opts = c.options.as_slice();
    let mut best = Best::new();
    match c.kind {
        ChoiceKind::FoodOrRes { .. } => {
            for (i, o) in opts.iter().enumerate() {
                let v = match o {
                    ChoiceOption::Word(Keyword::Food) => ctx.food_need as f64 * 2.0,
                    ChoiceOption::Word(Keyword::Resources) => ctx.res_need as f64 * 2.0 + 1.0,
                    ChoiceOption::Card(_) | ChoiceOption::Slot(_) | ChoiceOption::Move(_) | ChoiceOption::Gain(_) | ChoiceOption::Word(_) => 0.0,
                };
                best.consider(i, v);
            }
        }
        ChoiceKind::FreeBuild => {
            for (i, o) in opts.iter().enumerate() {
                let v = match o {
                    ChoiceOption::Word(Keyword::Skip) => 0.1,
                    ChoiceOption::Card(id) => prod_value(id.get().production, ctx),
                    ChoiceOption::Slot(_) | ChoiceOption::Move(_) | ChoiceOption::Gain(_) | ChoiceOption::Word(_) => 0.0,
                };
                best.consider(i, v);
            }
        }
        ChoiceKind::TakeRow { .. } => {
            // A free take: grab the best card, then stop.
            for (i, o) in opts.iter().enumerate() {
                let v = match o {
                    ChoiceOption::Word(Keyword::Stop) => 0.5,
                    ChoiceOption::Slot(slot) => {
                        let id = state.card_row[*slot as usize];
                        if id.is_none() {
                            0.0
                        } else {
                            card_value(state, p, ctx, id)
                        }
                    }
                    ChoiceOption::Card(_) | ChoiceOption::Move(_) | ChoiceOption::Gain(_) | ChoiceOption::Word(_) => 0.0,
                };
                best.consider(i, v);
            }
        }
        ChoiceKind::GainBlock => {
            // Options are gain blocks; prefer the one the book wants now.
            for (i, o) in opts.iter().enumerate() {
                let v = match o {
                    ChoiceOption::Gain(g) => {
                        let prod =
                            Production { food: g.food, resources: g.resources, science: 0, culture: 0, happy: 0, strength: 0 };
                        prod_value(prod, ctx)
                    }
                    ChoiceOption::Card(_) | ChoiceOption::Slot(_) | ChoiceOption::Move(_) | ChoiceOption::Word(_) => 0.0,
                };
                best.consider(i, v);
            }
        }
        ChoiceKind::FreeCivil { .. } => {
            // Options are concrete moves: rank them with the action rules.
            let mut sub = MoveList::new();
            for o in opts {
                if let ChoiceOption::Move(mv) = o {
                    sub.push(*mv);
                }
            }
            let chosen = action_phase(state, p, ctx, sub.as_slice());
            for (i, o) in opts.iter().enumerate() {
                if let ChoiceOption::Move(mv) = o {
                    if *mv == chosen {
                        best.consider(i, 1.0);
                        break;
                    }
                }
            }
        }
        ChoiceKind::DestroyOwn | ChoiceKind::LosePop | ChoiceKind::FlipWonder => {
            // Give up the least valuable thing.
            for (i, o) in opts.iter().enumerate() {
                if let ChoiceOption::Card(id) = o {
                    best.consider(i, -prod_value(id.get().production, ctx));
                }
            }
        }
        ChoiceKind::DiscardMilitary => {
            for (i, o) in opts.iter().enumerate() {
                if let ChoiceOption::Card(id) = o {
                    // Keep defensive tactics and wars; pitch events first.
                    let v = match id.kind() {
                        CardType::Event => 3.0,
                        CardType::Territory => 1.0,
                        CardType::Pact => 1.5,
                        CardType::Aggression => 0.5,
                        CardType::War => 0.2,
                        CardType::Tactic => 0.0,
                        CardType::Bonus => 0.4,
                        CardType::Farm | CardType::Mine | CardType::Lab | CardType::Temple | CardType::Library | CardType::Arena | CardType::Theater | CardType::Infantry | CardType::Cavalry | CardType::Artillery | CardType::Air | CardType::Government | CardType::SpecialTech | CardType::Wonder | CardType::Leader | CardType::Action => 1.0,
                    };
                    best.consider(i, v);
                }
            }
        }
        ChoiceKind::Raid { .. } | ChoiceKind::Annex { .. } => {
            // Take the most valuable thing from the victim.
            for (i, o) in opts.iter().enumerate() {
                if let ChoiceOption::Card(id) = o {
                    best.consider(i, prod_value(id.get().production, ctx));
                }
            }
        }
        ChoiceKind::Infiltrate { .. } => {
            for (i, o) in opts.iter().enumerate() {
                let v = match o {
                    ChoiceOption::Word(Keyword::Leader) => 1.0,
                    ChoiceOption::Word(Keyword::Wonder) => 0.5,
                    ChoiceOption::Card(_) | ChoiceOption::Slot(_) | ChoiceOption::Move(_) | ChoiceOption::Gain(_) | ChoiceOption::Word(_) => 0.0,
                };
                best.consider(i, v);
            }
        }
        ChoiceKind::PactOffer { owner, .. } => {
            // Accept a pact unless it props up the culture leader.
            let leading = state.players[owner as usize].culture > p.culture + 5;
            let want = if leading { Keyword::Refuse } else { Keyword::Accept };
            for (i, o) in opts.iter().enumerate() {
                if let ChoiceOption::Word(w) = o {
                    if *w == want {
                        best.consider(i, 1.0);
                        break;
                    }
                }
            }
        }
        ChoiceKind::WarTech { .. } => {
            // War over Technology: spend the strength advantage on science
            // or on the loser's blue cards, compared PER SCIENCE POINT --
            // science is the numeraire at 1.0 a point. A steal that does NOT
            // upgrade an icon the book already fills is scored at denial
            // value only (the card is discarded, not kept).
            for (i, o) in opts.iter().enumerate() {
                let v = match o {
                    ChoiceOption::Word(Keyword::Science) => 1.0,
                    ChoiceOption::Card(id) => {
                        let cost = (id.get().science_cost as i32).max(1);
                        let icon = costs::special_icon(id.get());
                        let mine = p
                            .techs
                            .iter()
                            .filter(|&(cid, _)| cid.kind() == CardType::SpecialTech && costs::special_icon(cid.get()) == icon)
                            .map(|(cid, _)| cid.level() as i32)
                            .max()
                            .unwrap_or(-1);
                        if id.level() as i32 <= mine {
                            0.35 // denial only; it is discarded
                        } else {
                            card_value(state, p, ctx, *id) / cost as f64
                        }
                    }
                    ChoiceOption::Slot(_) | ChoiceOption::Move(_) | ChoiceOption::Gain(_) | ChoiceOption::Word(_) => 0.0,
                };
                best.consider(i, v);
            }
        }
        ChoiceKind::LoseColony => {
            // No policy here in Python either -- always option 0.
            best.consider(0, 0.0);
        }
        ChoiceKind::PlunderSplit { .. } => {
            // Same shape as `GainBlock` (both offer `ChoiceOption::Gain`),
            // so the same valuation applies: price each split by this
            // book's own food/resources marginal value instead of a fixed
            // "resources first" order (FAQ p.7 -- the split is the
            // attacker's choice).
            for (i, o) in opts.iter().enumerate() {
                if let ChoiceOption::Gain(g) = o {
                    let prod = Production {
                        food: g.food,
                        resources: g.resources,
                        science: 0,
                        culture: 0,
                        happy: 0,
                        strength: 0,
                    };
                    best.consider(i, prod_value(prod, ctx));
                }
            }
        }
        ChoiceKind::FoodOrResSplit { lose } => {
            // Same `ChoiceOption::Gain` shape as `PlunderSplit` just above,
            // self-directed rather than attacker-directed (Raiders/Foray's
            // §5.3 `foodAndOrResources` split): price each split by this
            // book's own marginal food/resources value, negated for a loss
            // (prefer losing whichever combination this book values LEAST).
            for (i, o) in opts.iter().enumerate() {
                if let ChoiceOption::Gain(g) = o {
                    let prod = Production {
                        food: g.food,
                        resources: g.resources,
                        science: 0,
                        culture: 0,
                        happy: 0,
                        strength: 0,
                    };
                    let v = prod_value(prod, ctx);
                    best.consider(i, if lose { -v } else { v });
                }
            }
        }
    }
    Move::Choose { n: best.i as u8 }
}

/// Colonies are permanent production plus culture, so they are worth bidding
/// for -- but not worth crippling the army for.
fn auction(_a: &Auction, moves: &[Move]) -> Move {
    let mut top = 0u8;
    let mut any = false;
    for &m in moves {
        if let Move::Bid { n } = m {
            any = true;
            top = top.max(n);
        }
    }
    if !any {
        return Move::BidPass;
    }
    // Spend at most half the force we could bring, and never so much that we
    // drop under the military floor we care about.
    let cap = (top / 2).max(1);
    let mut best: Option<u8> = None;
    for &m in moves {
        if let Move::Bid { n } = m {
            if n <= cap && (best.is_none() || n < best.unwrap()) {
                best = Some(n);
            }
        }
    }
    match best {
        Some(n) => Move::Bid { n },
        None => Move::BidPass,
    }
}

/// Defend with the cards that actually add defence; do not burn the hand on
/// an attack that is already lost or already survived.
fn defense(d: &Defense, moves: &[Move]) -> Move {
    if !moves.iter().any(|m| matches!(m, Move::Defend { .. })) {
        return Move::DefendDone;
    }
    if d.atk - d.dfn <= 0 {
        return Move::DefendDone;
    }
    let mut best: Option<Move> = None;
    let mut best_v = 0;
    for &m in moves {
        if let Move::Defend { card } = m {
            let v = interact::defense_points(card);
            if v > best_v {
                best = Some(m);
                best_v = v;
            }
        }
    }
    best.unwrap_or(Move::DefendDone)
}

/// James Cook may discard at most this many military cards per colonization
/// (`interact::COOK_DISCARDS`, private there). Duplicated here rather than
/// made `pub`, the same shape `interact.rs::leader_is` documents itself as
/// duplicated (not imported) across this codebase's modules.
const COOK_DISCARDS: i32 = 2;

/// `interact::cook_bonus_per_discard`, private there -- duplicated for the
/// same reason as [`COOK_DISCARDS`] above.
fn cook_bonus_per_discard(p: &PlayerState) -> i32 {
    if p.leader.is_none() {
        return 0;
    }
    p.leader
        .get()
        .special
        .iter()
        .find_map(|s| match s {
            Special::ColonizeDiscardUpTo2MilitaryCardsForBonus(v) => Some(*v as i32),
            Special::A(_) | Special::AllPlayers(_) | Special::B(_) | Special::BestTheaterDoubleCulture | Special::BothPlayers(_) | Special::BuildDiscount(_) | Special::CancelledIfPartiesAttackEachOther | Special::CannotPlayAggressionOrWar | Special::CivilActionBackOnTechDevelop(_) | Special::CivilActionUpgradeUrbanBuildingToTheater | Special::ColonyImmediateBonusApplies | Special::ColonyPermanentBonusTransfers | Special::ComboFoodDiscount(_) | Special::ComboResourceDiscount(_) | Special::Condition(_) | Special::CultureFirstColony(_) | Special::CultureIfTopTwoStrength(_) | Special::CultureOnLeaveEqualToLabResourceProduction | Special::CultureOnRevolution(_) | Special::CultureOnTechDevelop(_) | Special::CulturePerAdditionalColony(_) | Special::CulturePerCivilizationWithMoreCulture(_) | Special::CulturePerHappyFromTemplesTheatersWonders(_) | Special::CulturePerLabEqualToLevel | Special::CulturePerLibraryTheaterPair(_) | Special::CulturePerTheater(_) | Special::DecreasePopulation(_) | Special::DestroyUrbanBuildings(_) | Special::DoubleBestMine | Special::DoublesTacticBonusOfOneArmy | Special::ExtraHappyPerHappySource(_) | Special::FinalScoring(_) | Special::FreeCivilAction(_) | Special::FreePopIncreasePerTurn | Special::Gain(_) | Special::GainCulturePerLevelOfRemovedCard(_) | Special::GainFoodOrResources(_) | Special::GainResources(_) | Special::InfantryCountsAsCavalryForTactics | Special::LastRoundSubstitute(_) | Special::LeaderTakeCivilActionDiscount(_) | Special::LibraryDiscountsIfTheater | Special::Lose(_) | Special::MilitaryActionAsCivilPerTurn(_) | Special::MilitaryActionCombinedPopIncreaseAndUnitBuild | Special::NoAttacksBetweenParties | Special::OnAttackBetweenParties(_) | Special::OnBuildCulture(_) | Special::OnBuildCulturePerTechLevelSum | Special::OnReplacePutUnderCompletedWonderHappy(_) | Special::OncePerGameTwoPoliticalActions | Special::OpponentDecreasesPopulation(_) | Special::OpponentsPayDoubleMilitaryActionsToAttackYou | Special::OrTakesSpecialTechnologiesOfSameTotalScienceCost | Special::PeekTopEventCardInPolitics | Special::PerTurnChoice | Special::PlayerWithLeastCulture(_) | Special::PlayerWithMostCulture(_) | Special::PlayersWithMostDiscontentWorkers(_) | Special::PlayersWithMostHappyFaces(_) | Special::PopIncreaseFoodDiscount(_) | Special::RemoveAsPoliticalActionForYellowToken(_) | Special::RemoveAsPoliticalActionFreeColonize | Special::RemoveFromGame | Special::ResourceOnMilitaryUnitBuildOrUpgrade(_) | Special::ResourceOnTechDevelop(_) | Special::ResourcesForMilitaryUnitsPerStrongerCivilization(_) | Special::ResourcesPerLabEqualToLevel | Special::RevolutionUsesMilitaryActionsInstead | Special::ScienceOnTechCardTake(_) | Special::SciencePerBestLabOrLibraryLevel | Special::SciencePerLab(_) | Special::StealColony(_) | Special::StrengthPerArtillery(_) | Special::StrengthPerInfantry(_) | Special::StrengthPerMilitaryUnit(_) | Special::StrengthPerTempleOrGovernmentHappy(_) | Special::StrengthPerUnitType(_) | Special::StrongestPlayer(_) | Special::StrongestPlayers(_) | Special::TakeCivilActionDiscountIfLeaderReplacedThisTurn(_) | Special::TakeFromOpponent(_) | Special::TheaterResourceDiscountIfLibrary(_) | Special::TheaterScienceDiscountIfLibrary(_) | Special::TheaterTechScienceDiscount(_) | Special::VictorTakesCulture | Special::VictorTakesScienceUpTo(_) | Special::VictorTakesYellowTokens | Special::WeakestPlayer(_) | Special::WeakestPlayers(_) | Special::WonderTakeNoExtraCivilActions => None,
        })
        .unwrap_or(0)
}

/// Build the colonization force out of the cheapest things you own.
///
/// RULES_SPEC 11.3 lets the winner choose; [`interact::colonize_moves`]'s
/// generation order (weakest unit first, then bonus cards, then Cook
/// discards, then more units, per that function's own doc comment) is not
/// this bot's policy -- this bot's own priority, matching `_colonize`
/// exactly, is: stop the moment the bid is met; JAMES COOK's discards come
/// FIRST when they alone can close the gap (each is worth a flat bonus, and
/// burning junk to close a gap of one or two is the cheapest thing on the
/// table); otherwise bonus cards before units, because a card is a
/// consumable and a unit is not.
fn colonize(state: &GameState, p: &PlayerState, c: &Colonize, moves: &[Move]) -> Move {
    if let Some(m) = moves.iter().find(|m| matches!(m, Move::SendDone)) {
        return *m;
    }
    if moves.iter().any(|m| matches!(m, Move::SendDiscard { .. })) {
        let committed = interact::force_value(state, p, c.units.as_slice(), c.bonuses.as_slice(), c.discards.as_slice());
        let gap = c.bid as i32 - committed;
        let slots_left = COOK_DISCARDS - c.discards.len() as i32;
        if gap <= slots_left * cook_bonus_per_discard(p) {
            if let Some(m) = moves.iter().find(|m| matches!(m, Move::SendDiscard { .. })) {
                return *m;
            }
        }
    }
    if let Some(m) = moves.iter().find(|m| matches!(m, Move::SendBonus { .. })) {
        return *m;
    }
    if let Some(m) = moves.iter().find(|m| matches!(m, Move::SendUnit { .. })) {
        return *m;
    }
    if let Some(m) = moves.iter().find(|m| matches!(m, Move::SendDiscard { .. })) {
        return *m;
    }
    moves[0]
}

// --------------------------------------------------------------- the hybrid

/// Move kinds where the measurements in `docs/EVALUATOR_HISTORY.md` say the
/// book beats the trained evaluator:
///
/// * `develop`     -- the champion hoards science it never converts
/// * `pop`/`pop_free` -- it stops growing the population far too early
/// * `wonder_step` -- it starts wonders and leaves them unfinished
/// * `revolution`  -- it will not pay a turn of actions for a permanent one
pub fn is_override_kind(mv: Move) -> bool {
    matches!(mv, Move::Develop { .. } | Move::Pop | Move::PopFree | Move::WonderStep { .. } | Move::Revolution { .. })
}

/// The trained champion, overruled by the book in its known blind spots.
///
/// This is the ablation that separates "the book bot is better" from "the
/// book bot is better *because of these specific rules*". It plays the
/// champion's evaluator everywhere except the move kinds in
/// [`is_override_kind`]; there, if the book wants to make such a move and
/// the champion does not, the book gets its way. See this module's top doc
/// comment for why the champion is injected as a full-policy closure rather
/// than constructed here.
#[derive(Clone, Copy, Debug, Default)]
pub struct BookImprovedBot {
    pub book: BookBot,
}

impl BookImprovedBot {
    /// `champion` is the trained bot's own `choose(state, moves) -> Move`,
    /// exactly as `weighted.WeightedBot.choose` is in Python. Not called
    /// when `state.pending`/`state.phase == Politics` -- Python's
    /// `BookImprovedBot.choose` returns the champion's pick unconditionally
    /// there too.
    pub fn choose(&self, state: &GameState, moves: &[Move], champion: impl Fn(&GameState, &[Move]) -> Move) -> Move {
        let champ = champion(state, moves);
        if !state.pending.is_empty() || state.phase == Phase::Politics {
            return champ;
        }
        if is_override_kind(champ) {
            return champ; // champion already making the right kind of move
        }
        let book = self.book.choose(state, moves);
        if is_override_kind(book) && moves.contains(&book) {
            return book;
        }
        champ
    }
}

// ============================================================== tests ====

#[cfg(test)]
mod tests {
    use super::*;

    /// The typo guard described in this module's top doc comment: every
    /// name in every rank table below must resolve to a real card, or this
    /// test fails on every `cargo test` -- the nearest available substitute
    /// to "a typo is a compile error" without new compile-time machinery.
    #[test]
    fn every_rank_table_name_resolves_to_a_real_card() {
        for &(name, _) in LEADER_RANK
            .iter()
            .chain(WONDER_RANK)
            .chain(SPECIAL_RANK)
            .chain(TACTIC_RANK)
            .chain(V2_LEADER_RANK)
            .chain(V2_WONDER_RANK)
        {
            assert!(CardId::by_name(name).is_some(), "no such card: {name:?}");
        }
        for &name in V2_NEVER_TAKE {
            assert!(CardId::by_name(name).is_some(), "no such card: {name:?}");
        }
        for name in ["Alchemy", "Printing Press"] {
            assert!(CardId::by_name(name).is_some(), "no such card: {name:?}");
        }
    }

    #[test]
    fn v2_price_ladder_matches_the_printed_row_costs_with_a_convex_default() {
        assert_eq!(v2_price_ladder(0), 0.0);
        assert_eq!(v2_price_ladder(1), 1.0);
        assert_eq!(v2_price_ladder(2), 3.5);
        assert_eq!(v2_price_ladder(3), 9.0);
        assert_eq!(v2_price_ladder(4), 12.0);
        assert_eq!(v2_price_ladder(-1), 12.0);
    }

    #[test]
    fn v2_homer_splits_2p_from_multiplayer() {
        assert_eq!(v2_homer(2), 5.0);
        assert_eq!(v2_homer(3), 7.5);
        assert_eq!(v2_homer(4), 7.5);
        assert_eq!(v2_homer(1), 6.0, "an unlisted player count falls back to the default");
    }

    #[test]
    fn rank_of_falls_back_to_the_default_for_an_unlisted_card() {
        let despotism = CardId::by_name("Despotism").expect("Despotism must exist");
        assert_eq!(rank_of(LEADER_RANK, despotism, 5.0), 5.0);
        let hammurabi = CardId::by_name("Hammurabi").expect("Hammurabi must exist");
        assert_eq!(rank_of(LEADER_RANK, hammurabi, 5.0), 9.0);
    }

    /// v1's `LEADER_RANK` ranks Julius Caesar (8) well above Alexander the
    /// Great (5); v2's tournament-derived `V2_LEADER_RANK` says the
    /// opposite -- Caesar 3.0 ("in the new TTA he is terrible"), Alexander
    /// 5.5. Before the fix `r_play_leader` read `LEADER_RANK` (v1) directly
    /// regardless of `ctx.version`, so a version=2 bot picked Caesar too;
    /// it must now route through the version-aware `leader_rank` like every
    /// other leader decision.
    #[test]
    fn r_play_leader_routes_through_the_version_aware_table() {
        let state = crate::game::new_game(2, 1);
        let p = &state.players[0];
        let caesar = card("Julius Caesar");
        let alexander = card("Alexander the Great");
        let moves = [Move::PlayLeader { card: caesar }, Move::PlayLeader { card: alexander }];

        let ctx1 = Ctx::new(&state, 0, 1, V2Tunables::default());
        assert_eq!(
            r_play_leader(&state, p, &ctx1, &moves),
            Some(Move::PlayLeader { card: caesar }),
            "v1 ranks Caesar above Alexander"
        );

        let ctx2 = Ctx::new(&state, 0, 2, V2Tunables::default());
        assert_eq!(
            r_play_leader(&state, p, &ctx2, &moves),
            Some(Move::PlayLeader { card: alexander }),
            "v2 ranks Alexander above Caesar -- before the fix this read the bare v1 \
             table and picked Caesar too"
        );
    }

    /// Replacing an already-played leader requires a >= 2 point gain on
    /// whichever table is in force. Michelangelo is v1's best leader (9)
    /// but v2's worst (2.0, "last in every list found"); with Caesar (v2
    /// rank 3.0) already played, v2 must NOT treat Michelangelo as the
    /// upgrade v1 would.
    #[test]
    fn r_play_leader_upgrade_threshold_also_uses_the_v2_table() {
        let mut state = crate::game::new_game(2, 1);
        state.players[0].leader = card("Julius Caesar");
        let p = &state.players[0];
        let michelangelo = card("Michelangelo");
        let moves = [Move::PlayLeader { card: michelangelo }];
        let ctx2 = Ctx::new(&state, 0, 2, V2Tunables::default());
        assert_eq!(
            r_play_leader(&state, p, &ctx2, &moves),
            None,
            "v2 should not swap into a leader it ranks worse"
        );
    }

    /// `h_churchill` never spends a civil or military action -- only the
    /// once-per-turn flag itself, lost for the turn either way -- so taking
    /// it is strictly better than not. `r_churchill` always takes it when
    /// offered, defaulting to the risk-free flavour (culture): the military
    /// discount is wiped at end of turn and wasted if nothing military gets
    /// bought or upgraded this turn, so WHICH flavour to prefer in that
    /// case remains an open strategy question, not resolved here.
    #[test]
    fn r_churchill_always_takes_the_free_bonus_defaulting_to_culture() {
        let state = crate::game::new_game(2, 1);
        let p = &state.players[0];
        let ctx = Ctx::new(&state, 0, 1, V2Tunables::default());
        let moves = [
            Move::Churchill { choice: ChurchillChoice::Culture },
            Move::Churchill { choice: ChurchillChoice::Military },
        ];
        assert_eq!(
            r_churchill(&state, p, &ctx, &moves),
            Some(Move::Churchill { choice: ChurchillChoice::Culture })
        );
        assert_eq!(r_churchill(&state, p, &ctx, &[]), None);
    }

    /// End-to-end: with Churchill as leader and nothing else on offer,
    /// `choose` must pick the free bonus over `end_turn` -- pins that
    /// `r_churchill` is really reachable through `RULES`, not just correct
    /// in isolation.
    #[test]
    fn choose_takes_churchill_over_end_turn_when_nothing_else_fires() {
        let mut state = crate::game::new_game(2, 1);
        state.round = 3;
        state.phase = Phase::Actions;
        state.players[0].leader = card("Winston Churchill");
        let moves =
            [Move::Churchill { choice: ChurchillChoice::Culture }, Move::Churchill { choice: ChurchillChoice::Military }, Move::EndTurn];
        for version in [1u8, 2] {
            let bot = BookBot { version, tunables: V2Tunables::default() };
            assert_eq!(
                bot.choose(&state, &moves),
                Move::Churchill { choice: ChurchillChoice::Culture },
                "version={version}"
            );
        }
    }

    #[test]
    fn is_override_kind_matches_exactly_the_five_documented_kinds() {
        let card = CardId::by_name("Irrigation").expect("Irrigation must exist");
        assert!(is_override_kind(Move::Develop { card }));
        assert!(is_override_kind(Move::Pop));
        assert!(is_override_kind(Move::PopFree));
        assert!(is_override_kind(Move::WonderStep { steps: 1 }));
        assert!(is_override_kind(Move::Revolution { card }));
        assert!(!is_override_kind(Move::EndTurn));
        assert!(!is_override_kind(Move::Build { card }));
        assert!(!is_override_kind(Move::Take { slot: 0 }));
    }

    #[test]
    fn choose_with_a_single_move_never_touches_the_state() {
        let state = crate::game::new_game(2, 1);
        let moves = crate::legal::legal_moves(&state);
        let one = [moves.as_slice()[0]];
        let bot = BookBot::default();
        assert_eq!(bot.choose(&state, &one), one[0]);
    }

    #[test]
    fn choose_always_returns_one_of_the_offered_moves_at_the_start_of_the_game() {
        for version in [1u8, 2] {
            let state = crate::game::new_game(3, 42);
            let moves = crate::legal::legal_moves(&state);
            let bot = BookBot { version, tunables: V2Tunables::default() };
            let mv = bot.choose(&state, moves.as_slice());
            assert!(moves.as_slice().contains(&mv), "version={version}: {mv:?} was not offered");
        }
    }

    #[test]
    fn choose_never_mutates_the_real_state() {
        let state = crate::game::new_game(3, 7);
        let before = state.clone();
        let moves = crate::legal::legal_moves(&state);
        let bot = BookBot::default();
        let _ = bot.choose(&state, moves.as_slice());
        assert_eq!(state.card_row, before.card_row);
        assert_eq!(state.players[0].hand_civil, before.players[0].hand_civil);
        assert_eq!(state.turn, before.turn);
    }

    /// Round 1 offers only `take`/`end_turn` (no other rule can fire before
    /// `r_round1` returns), and `r_round1` never returns anything but a
    /// `take` when one is offered -- pins that the fast, cheap-slot-biased
    /// opening rule really is reachable end to end through `choose`.
    #[test]
    fn round_1_always_takes_a_card_when_one_is_legal() {
        let state = crate::game::new_game(2, 1);
        let moves = crate::legal::legal_moves(&state);
        assert!(moves.as_slice().iter().any(|m| matches!(m, Move::Take { .. })), "round 1 must offer a take");
        let bot = BookBot::default();
        let mv = bot.choose(&state, moves.as_slice());
        assert!(matches!(mv, Move::Take { .. }), "round 1 must take a card, got {mv:?}");
    }

    /// `BookImprovedBot` with a champion that always ends the turn: `end_turn`
    /// is not in `is_override_kind`, so if the book independently wants an
    /// override-kind move the book's pick wins; otherwise the champion's does.
    #[test]
    fn book_improved_defers_to_the_champion_outside_the_override_kinds() {
        let state = crate::game::new_game(2, 1);
        let moves = crate::legal::legal_moves(&state);
        let hybrid = BookImprovedBot::default();
        let champion = |_s: &GameState, ms: &[Move]| ms[0];
        let picked = hybrid.choose(&state, moves.as_slice(), champion);
        assert!(moves.as_slice().contains(&picked));
    }
}
