# Distilled audits: score, combat, coverage, and bot-blindness findings

Distilled 2026-08-06 from ten audit docs (SCORE_AUDIT, COMBAT_AUDIT,
COVERAGE_AUDIT, SYSTEM_COVERAGE, UNCOVERED_TYPES, WAR_RATE_CENSUS,
AGGRESSION_RATE, AGGRESSION_STATUS, WASTED_ACTIONS, EVENT_SEEDING; ~7,000
lines, all `git rm`'d). Full narrative, per-run numbers and champion-specific
weight tables are in git history — search commit messages for the doc names
above.

**Provenance note.** All ten source docs were written against the Python
engine (`engine/*.py`, `tests/*.py`), deleted 2026-08-06 in favour of the
Rust port under `rust/src/`. Every rules/engine finding below was re-checked
against current `rust/src/` and is kept only where the equivalent logic still
exists there (paths given). Behavioural numbers (take-rates, census tables,
champion weight vectors, A/B win rates) describe specific long-gone
Python-era bot generations and are **not** carried forward — the game rules
outlive a bot generation, a win rate does not.

---

## 1. Rules bugs found and fixed (still true of the game; verified live in `rust/src/`)

Each of these was a real divergence from the printed 2015 base-game rules,
confirmed against the rulebook/FAQ, fixed, and the fix is present in the
current Rust engine. Not re-derived here — just recorded so nobody
re-discovers them by hand.

- **Card scoring "produced by X" means the source, not the whole rating.**
  `Impact of Agriculture` and `Impact of Industry` score what farms/mines
  *produce*, not the player's food/resource rating (which can include
  pact bonuses etc.). Engine's `building_output`/`mine_resources`/
  `farm_food` (`rust/src/effects.rs`) implement the source-only reading.
  Lesson: any card phrased "the X produced by their Y" needs the
  building-output path, not the flat rating.
- **Unstaffed buildings produce nothing.** A tech card with no worker on it
  is a technology, not a building; per-building leader bonuses (best
  lab/library, best theater, etc.) must require a worker. Settled by three
  independent sources (card text, FAQ v1.5 p.9 on the Transcontinental
  Railroad, and a 150-game BGO corpus check: 7303/7600 vs 7275/7600 in
  favour of the staffed reading). `rust/src/effects.rs`'s `building_output`
  table is the single reader all such modifiers now go through.
- **A ruined wonder (Ravages of Time) stops producing entirely**, including
  for Michelangelo's happy-face culture and St. Peter's happy-source count.
  `rust/src/effects.rs` filters flipped/ruined wonders out of every reader
  (`happy_source_count` and the completed-wonder iteration both exclude
  ruins now).
- **St. Peter's Basilica counts a colony as a happy source**, on the same
  terms as the government and leader cards (both already counted; a colony
  card is a card too). `happy_source_count` (`rust/src/effects.rs`) walks
  `p.colonies`.
- **A second air force must double the *outdated* army's (smaller) bonus**
  when armies of mixed age exist, not always the fresh one's.
- **"The players with the most/least X" affects every tied player, not
  one.** `playersWithMostHappyFaces` (Immigration) and
  `playersWithMostDiscontentWorkers` (Civil Unrest) are printed plural,
  per Code of Laws p.7. `rust/src/events.rs`'s `apply_tied_targets`
  implements the tied-multi-target reading explicitly (as opposed to the
  six genuinely-singular `strongestPlayer`/`weakestPlayer` keys).
- **Winston Churchill's military leader option is restricted**: the 3
  science/3 resources may only fund military-unit technologies/builds, not
  spent freely. `rust/src/apply.rs::h_churchill` branches on
  `ChurchillChoice::{Culture,Military}`.
- **Bill Gates pays his end-of-lab culture when he leaves play**, not only
  at game end (card text: "removed from the game **or** the game ends").
- **War over Technology's victor may take blue special technologies instead
  of science**, up to the strength advantage, at their printed (not
  discounted) cost, choosing freely how to split — this is a genuine
  player *decision*, not a formula. Implemented via
  `rust/src/interact.rs::war_tech_spoils` on the existing pending-choice
  machinery (all bots that clone+apply+evaluate get the choice for free;
  BookBot needed an explicit preference). Confirmed against ~40 archived
  BGO journals whose resolved spoils sum exactly to the strength advantage
  across mixed cards+science.
- **A war declaration must cancel a pact that ends on attack**, exactly as
  an aggression already did (`rust/src/combat.rs::cancel_attack_pacts`,
  called from both `h_war` and `start_aggression`). Missing this let a
  Promise-of-Military-Protection pact's +4 strength survive into the very
  war it should have been cancelled by.
- **Aggression legality must exclude *both* sides' pact-conferred strength**
  when the pact ends on attack, not just the attacker's — otherwise
  attacking a Military Alliance partner (+3 both sides) needed a 6-point
  edge instead of 1. `rust/src/combat.rs`'s `attack_strength`/
  `defense_strength` (mirrors, sharing `_doomed_pact_strength`) fixed this.
- **A resignation to 2 players must not strip pacts from hand or the
  current-age deck** — that's a *setup* rule (no pacts in a 2p deck), not a
  dynamic one keyed on live player count. Only future-age decks re-trim.
- **Revolution must carry over spent/unspent civil and military actions as
  an update against the new government's totals, not a cap** against the
  old ones — a revolution from Despotism (2 MA) to Monarchy (3 MA) with
  nothing spent must leave 3 MA available, not 2. Same bug mirrored for
  Robespierre's civil-action side. `rust/src/apply.rs::h_revolution`.
- **The one-name-per-technology rule does not apply to yellow action
  cards** (which have no science cost and aren't technologies) — several
  action cards exist in 2-3 copies and holding one must not block taking
  another. Gated on `CardType::Action` being excluded, `rust/src/legal.rs`.
- **`buildDiscount` reduces cost by the best single applicable age entry,
  not the sum of all of them** — the ages on one card are mutually
  exclusive (a building has exactly one age). `rust/src/effects.rs`'s
  `build_discount` is a flat per-age array (`Stats::build_discount`), not
  an accumulator.
- **Colonization: which units to sacrifice for the bid is the colonizing
  player's choice**, not an engine-picked greedy default (cheapest unit /
  bonus cards first) — CoL §11.3. This was fixed after the original audits
  by turning it into a real pending decision
  (`send_unit`/`send_bonus`/`send_done`); the old greedy behaviour is kept
  only as an explicit BookBot policy.
- **The end-of-turn military hand-limit discard is the player's decision**,
  not FIFO. Rulebook: "Only step requiring a decision." A blind
  oldest-first discard was measured (Python era) to throw away the single
  best defence card on ~20-40% of over-limit turns. `rust/src/interact.rs`
  now suspends `end_of_turn` and resolves the choice through
  `discard_options`/a real pending decision (`rust/src/economy.rs`,
  `rust/src/combat.rs`).
- **A bot must evaluate a pending decision from the seat that actually owns
  it** (`state.decider()`), not from `state.current` — the two differ
  whenever a pending choice (pact accept/refuse, a colony bid, a defence)
  belongs to someone other than the player whose turn it is. Getting this
  wrong means the bot maximises a rival's position on that decision. Fixed
  and now the pervasive convention: `rust/src/state.rs::decider()` is used
  throughout `rust/src/bots/`.
- **Aggression: Plunder's food/resource split is the attacker's choice, not
  the engine's.** FAQ p.7: the split between the two banks belongs to the
  aggressor; the engine used to drain resources first, unconditionally.
  Totals and the victim's cap were always right — only the mix was wrong.
  Fixed by opening a real pending decision, the same shape as `WarTech`/
  `TakeRow`: `interact::offer_plunder_split`/`ChoiceKind::PlunderSplit`,
  called from `combat::finish_aggression`, valued by the book bot in
  `bots/book.rs` via the same `prod_value` `GainBlock` already uses.
- **Annex and Infiltrate must not be legal against a target that fails
  their printed target clause** (Annex: no colony to steal; Infiltrate: no
  leader and no unfinished wonder to remove) — playing either anyway cost
  the card and a military action and resolved to nothing. This was
  previously left alone as ambiguous; it settles once you read the card's
  own printed target field rather than only the rulebook prose:
  `data/cards_military_actions.json` gives Annex's target as "one opponent
  who owns at least one colony" and Infiltrate's as "one opponent with a
  leader in play or a wonder under construction" (digital-edition card
  text; Infiltrate's wonder-loss reading also confirmed by FAQ p.11). A
  target that fails the card's own printed target clause is not a legal
  target, the same way one that beats the attacker's strength isn't.
  `rust/src/legal.rs::aggression_target_qualifies` gates `politics_moves`
  on the same `Special::StealColony`/`Special::RemoveFromGame` data
  `combat::finish_aggression`'s resolution already reads, so the check and
  the resolution can't drift onto two different notions of "this card".

## 2. Validation methodology — durable, applies to any future audit

These are lessons about *how to validate this engine*, not about any one
bug. They cost real time to learn and are cheap to re-state.

- **A corpus validates a formula only over the inputs it can produce.** A
  100%-agreement row is a statement about the *inputs measured*, not proof
  the formula is right. Textbook case: `Impact of Agriculture` scored
  66/66 against 1,011 human games while implementing the wrong formula
  (rating instead of farm production) — because at 2 players every pact is
  removed from the game, and a pact's food symbol is the *only* thing in
  the base game that puts food on the board from outside a farm. The two
  readings were identically equal in every game in a 2p-only corpus, so no
  amount of that data could have separated them. Before trusting a
  percentage from a replay corpus, ask what inputs produced it and whether
  they could distinguish the hypothesis you actually care about from the
  alternative.
- **A swap-diff pricer (put a card on the board, diff `compute()`) is exact
  over whatever the stats struct tracks, and silently blind to anything
  that lives outside it** (e.g. token grants that bypass the stats struct
  entirely). It "can never drift" from the rules engine, but only for the
  fields it can see — every non-stats side-effect of a swappable card needs
  an explicit rider.
- **Reverting a fix and re-measuring, one at a time, is the only reliable
  way to attribute a behavioural or fingerprint change** — reasoning about
  which fix "should" move a given metric is repeatedly wrong in this
  project's own history (a fix predicted inert moved a metric because of
  which player count it plays at; a fix predicted common turned out rare).
  Attribute, don't reason.
- **Before quoting an instrument's number, verify the instrument can move**
  — i.e. that it would report differently if the thing it's supposedly
  measuring were absent. Two separate "spectacular" findings in this
  project's history turned out to be measuring something that couldn't
  vary (a peek that was common-mode across all candidates and so cancelled
  out of every argmax; a leak-rate counter that was identical whether or
  not the leak was fixed because it counted "did this draw a card" instead
  of "was the card it drew the true one"). A negative control — show the
  number moves when you deliberately break the thing — is cheap and is
  what separates a result from an artifact.
- **Every rate needs its denominator stated alongside it**, and specifically
  "never chosen" vs "never offered" must be distinguished — a bare
  take-rate of 0% is ambiguous between "the bot refuses this" and "this was
  never legal here," and conflating them has produced wrong conclusions
  more than once in this project.
- **Standing rule for changes to this evaluator/engine**: land on master and
  read the real training-league runs; do not validate with offline paired
  A/B batches or replay a fixed fingerprint set (both compete with training
  for the same cores, and a digest-gate approach was deliberately replaced
  by richer logging when the Python `tools/gate.sh` was retired). Ship a
  change at the weight your own reasoning best defends, not pinned inert
  pending an A/B; if the best-reasoned setting is "off," don't land it.

## 3. Evaluator/search architecture — structural blind spots

The Python bot-evaluator architecture (a linear feature-weight dot product,
`WeightedBot` = 1-ply, `PlanBot`/`QuiescentBot` = beam/quiescence on top of
the same feature set) is carried into the Rust port essentially unchanged —
`rust/src/bots/weighted/` has the same `hand_potential`/`card_potential`/
`row_pressure`/`deferred_credit`/`event_scoring_margin` names and shapes as
the Python original, and `rust/src/bots/{plan,quiescent}.rs` are the same
search structures. The following are therefore standing architectural risks
to check for, not closed Python-era findings — verify current behaviour
before assuming either the bug or its fix is in whatever state the
Python-era measurement found it.

- **A 1-ply evaluator cannot see any move whose payoff is deferred to
  another player's decision**, and every tie in this project breaks toward
  the lowest-indexed/do-nothing candidate. This is the single mechanism
  behind three independent-looking symptoms in the Python-era bots: never
  offering a pact (the pact object doesn't exist until the partner accepts,
  so the offering trial state shows only "one fewer hand card"), never
  bidding first in a colony auction while rivals remain (a bid only
  resolves the auction, hence becomes visible, once you're the sole
  remaining bidder), and never declaring war profitably when the payoff
  lands a full turn later at resolution. The general fix pattern used here
  — `deferred_credit`/`auction_committed`-style terms that credit the
  *expected* value of an outstanding self-initiated pending decision, and
  a "drain the pending stack before scoring" pass (quiescence) before a
  bot prices its own real decision the same way it prices nodes inside its
  own search — is the shape to reach for again if this class of bug
  resurfaces. A related, easy-to-reintroduce bug: draining/quiescing must
  determinize hidden piles first (deck order *and* the face-down future
  events queue, which is public in age-order but not in identity) — a
  drain that peeks the real next card is a latent information leak even
  though it's provably common-mode (identical across every candidate, so
  it doesn't change any single decision) *until* some future feature makes
  candidates draw differing amounts, at which point it becomes live and
  nothing will fail to flag it.
- **Card-identity blindness**: if the evaluator reduces "what's in my hand"
  to a bare count and a sum of age levels, two completely different cards
  produce an identical feature vector, so every take/develop/play of a
  civil card prices near zero regardless of the card. In the Python
  history this was the dominant cause of the bot "wasting" civil actions
  (not an `end_turn`-scored-too-early search artifact, which is real but
  fighting it directly measurably made play *worse* — the fix that worked
  was giving the evaluator a way to tell a good card from a bad one via a
  discounted preview of its own effects, `hand_potential`). If a future
  audit finds a bot inexplicably passing instead of playing/taking an
  obviously good card, check whether the relevant card category has an
  identity-bearing feature before assuming the search or the weight is
  the problem.
- **`end_turn` (and any other move that runs a full production/end-of-turn
  phase inside its own trial state) is structurally flattered** relative to
  every other candidate in a 1-ply comparison, because its child state has
  already banked income the other candidates haven't. A flat additive bias
  constant cannot correct this because the flattery scales with the
  player's economy and the game phase (small early, large in Age III/IV).
  Don't "fix" it by retuning that bias in isolation — in this project doing
  so (with the card-identity fix *not* also present) reliably made the bot
  weaker, because the bias was incidentally acting as a confidence filter
  on an evaluator that otherwise couldn't tell a good move from noise.
- **Adding a new 0.0-default weight for one side of an already-priced
  trade does not leave a card "neutral" or "inert" — it biases it, in the
  direction of whichever side you just made visible.** A card whose cost is
  priced through a trained weight and whose benefit sits behind a 0.0
  default reads as pure cost and gets systematically avoided (measured:
  all 12 base-game special technologies priced net-negative under a
  trained vector, six of them taken zero times in 40 games); the mirror
  case (cost at 0.0, benefit trained) makes a card read as too cheap and
  over-taken. "Inert" is a true claim about a *previously-trained weight
  vector being numerically unchanged*; it is not a true claim about the
  *card*. Before adding a one-sided 0.0-default feature, check whether the
  other side of that same card's trade is already priced.
- **The forecast and the actual payout should be one function, not two.**
  Wherever a search wants to *estimate* a future scoring event (e.g. "what
  will the pending Age III `Impact of ...` events pay out right now"), route
  it through the exact same code the engine uses to actually pay it
  (`pending_final_events`/`final_event_culture`-style split in
  `rust/src/events.rs`) rather than restating the formula in the
  evaluator. A second copy of a scoring formula living in the bot layer is
  this project's single most repeated bug shape (it recurred at least five
  times across the Python-era history: build discount, hand double-counting,
  population cost, a ranking tie-break block, and Hollywood/Internet's
  building-output logic).
- **A linear (weighted-sum) evaluator cannot express diminishing returns or
  "I have enough of X, I need Y instead."** Two options that trade off the
  same way in every position (e.g., a mine's resource_rate constant always
  beating a farm's food_rate constant) will be resolved the same way
  *always*, in every game state, by construction — no amount of retraining
  fixes this without either a nonlinear term or separate features that
  can flip sign with context (e.g., an explicit horizon/lateness
  multiplier). If an audit finds a mechanic taken 0% of the time despite
  being legal constantly and priced non-negatively, check whether it's
  losing a fixed linear comparison to a mechanic that's *never* worse
  in this feature basis, before concluding the mechanic itself is
  mispriced.

## 4. Verdict per source doc

- **SCORE_AUDIT.md** — real content: nine confirmed scoring-rule bugs (§1
  above), the corpus-validates-only-what-it-varies finding, and the
  swap-diff blind-spot finding. Kept. The 23-card-type-by-type table,
  fingerprint-digest hashes, and the BGO-corpus percentage tables are
  Python-era measurement detail; dropped.
- **COMBAT_AUDIT.md** — real content: three confirmed war/aggression/pact
  bugs, the War-over-Technology decision implementation, the military
  discard-as-decision fix, and the 1-ply-can't-see-deferred-payoff
  diagnosis (pacts/colonies/wars). Kept. Per-generation win-rate numbers,
  specific game counts, and the multi-page "why the champions never play
  pacts" investigation narrative are dropped; the structural lesson
  survives in §3 above.
- **COVERAGE_AUDIT.md** — real content: two more confirmed engine bugs
  (revolution's action-carryover cap, one-per-name wrongly gating action
  cards) and the colonization-sacrifice-is-a-choice finding (later fixed).
  Kept. The large per-mechanic take-rate census and the dead/live-feature
  variance tables describe one Python-era champion generation and are
  dropped entirely.
- **SYSTEM_COVERAGE.md** — nothing durable beyond what's captured above.
  It is a point-in-time behavioural census of Python-era champions
  (wonder/war/colony/tech rates vs. a human corpus) that the doc's own
  banner-style caveats already mark as superseding *and being superseded
  by* several even-earlier documents — i.e., a census of a census.
  Nothing here is a rules fact, a fix, or a methodology lesson not already
  captured elsewhere. Dropped in full.
- **UNCOVERED_TYPES.md** — real content: the military-discard-FIFO bug
  (already captured under COMBAT_AUDIT's fix) and the general "half-priced
  card" lesson (§3 above). Kept (folded into §3). The special-tech/
  production-building take-rate tables and the specific weight-scan numbers
  are dropped.
- **WAR_RATE_CENSUS.md** — an explicitly partial, ~20x-undersized,
  single-arm, single-player-count run that answers none of its own
  questions ("no monotonic trend... not strong evidence of anything," "one
  arm only... every number below is a small-n point estimate"). Nothing
  durable; the census-instrument design note (separate the bot's own
  decisions from its search's trial evaluations when counting) is already
  captured in §2. Dropped in full.
- **AGGRESSION_RATE.md** — real content: the general shape "a rate measured
  under a 1-ply bot is not a fact about a deeper-searching bot" and the
  quiesce-before-scoring-your-own-pending-decision fix pattern, both folded
  into §3 above. The specific before/after game counts, digest hashes, and
  the extensive determinization/leak investigation (interesting but fully
  Python/`tools/gate.sh`-specific machinery, since deleted) are dropped.
- **AGGRESSION_STATUS.md** — a short, explicitly-stale re-measurement note;
  its own banner says the tool and the champion path it used are gone.
  Nothing durable beyond "the qualitative shape (rules-declined aggressions
  are the majority and correct; multi-card defences are the ones given up)
  is a reasonable prior, unconfirmed against the current bot." Dropped.
- **WASTED_ACTIONS.md** — real content: the `end_turn`-flattery structural
  bias and the card-identity-blindness root cause, both folded into §3
  above, including the non-obvious result that fixing the visible
  `end_turn` bias in isolation made the bot measurably *worse*. All 4p
  numbers in the source doc were self-flagged as measured against a known
  degenerate weight vector; not carried forward regardless.
- **EVENT_SEEDING.md** — real content: the forecast-should-equal-payout
  architecture lesson and the observation that `_card_yields`-style
  per-card tables are the wrong hook for military-deck cards priced by
  resolution (aggressions/wars) or already gated out entirely (pacts at
  2p), both folded into §3. Also recorded two live-but-unfixed Python-era
  bugs (`_c_pact_offer` overwriting rather than appending to a pact list;
  a book-bot pact-response reading a context key the offer handler never
  wrote) — both are Python-specific paths in deleted code and were not
  re-checked against Rust; not carried forward as an open item since
  there's nothing to point at anymore. The A/B win-rate tables and weight
  scans are dropped.

## 5. Sibling-rule sweep (2026-08-07): "fixed in one place, silently absent in a sibling"

Prompted by `d9e52c6` (Barbarians'/Raiders'/Crime Wave's weakest-cutoff
tie-break, factored into `events::protect_current_from_bad_tie`): a targeted
search for every OTHER place the same shape could recur — a rule
re-implemented in more than one function, fixed in one, never propagated to
its siblings, with nothing to catch the divergence. Inventory below, one
group per candidate family, each with a verdict.

### 5.1 Player-selection predicates (strongest/weakest/most/least), `rust/src/events.rs`

Every function that ranks players by a stat and picks one or more, as of
this sweep:

| site | selection | tie-break used | verdict |
|---|---|---|---|
| `apply_single_target` (`events.rs:893`) | `strongestPlayer`/`weakestPlayer`/`playerWithMostCulture`/`playerWithLeastCulture`, singular | `protect_current_from_bad_tie` when `favor_current=false` (weakest) | correct (fixed pre-`d9e52c6`, the original fix this sweep generalizes from) |
| `apply_tied_targets` (`events.rs:928`) | `playersWithMostHappyFaces`/`playersWithMostDiscontentWorkers`, ALL tied players | none — RULES_SPEC 5.3: "most/least: all tied civs affected, no tie-break" | **clean**: no tie-break applies here by rule, so the absence of one is correct, not an oversight |
| `conditional_target`'s culture pick (`events.rs:947`) | Barbarians' "player with most culture" (stage 1 of 2) | unreversed (favor current), same convention as `PlayerWithMostCulture` above | **clean**: this stage's outcome is not itself the penalty; it only sets up stage 2 |
| `conditional_target`'s weakest cutoff (`events.rs:962`) | Barbarians' "among the two weakest" (stage 2 of 2) | `protect_current_from_bad_tie` | fixed in `d9e52c6` |
| `resolve_count_targets`'s `strongestPlayers` (`events.rs:996`) | top-N strongest, benefit | unreversed (favor current) | **clean**, correct convention |
| `resolve_count_targets`'s `weakestPlayers` (`events.rs:1022`) | bottom-N weakest, penalty (Raiders, Crime Wave) | `protect_current_from_bad_tie` | fixed in `d9e52c6` |
| `apply_player_block`'s `take_yellow_tokens_from_weakest` (`events.rs:1222`, Uncertain Borders: "the strongest civilization takes 1 yellow token from weakest civilization's yellow bank") | single weakest, penalty (victim loses a token) | was unreversed — **the exact `d9e52c6` shape, uncovered by that fix because this is a separate function reached from inside `apply_player_block`, not `resolve_event`'s own dispatch table** | **ENGINE BUG, fixed this pass.** Now routes through `protect_current_from_bad_tie` like every other weakest-penalty selection. Regression test `uncertain_borders_spares_the_current_player_from_a_tied_weakest_token_loss` (confirmed red pre-fix, green post-fix). Corpus: 29→30 completed games, zero games lost (exact ID diff — game `7522606` newly completes), full 1,011-game `replaystats` pass. |

Combat's own tie rule (§FAQ p.16: "ties favor the defender; only the
attacker can win an aggression") is a different, single deterministic
2-party comparison in `combat.rs::resolve_war_outcome` — not this
selection-with-ordering shape, not duplicated, not touched.

### 5.2 Cost and affordability, `rust/src/legal.rs` vs `rust/src/costs.rs`

`legal.rs`'s `action_moves` build/upgrade legality loop computed its own
inline copy of the unit discount formula (`(cost - p.mil_discount -
homer_unit_discount(p, id)).max(0)`) instead of calling `costs::
build_cost_net`/`costs::upgrade_cost_net` — the exact functions
`apply::on_build_unit` calls at charge time. The two formulas were
byte-for-byte identical today (verified: full test suite and the full
1,011-game corpus completed-game-ID set are unchanged before/after), so
this was a **clean negative, not a live bug** — but it is precisely the
structural shape that let Homer's once-per-turn discount go
once-per-*build* in an earlier pass (`costs::homer_unit_discount`'s own doc
comment: found because a build_cost_net-style formula existed in only one
of the two places that needed it). Refactored both loops to call `costs::
build_cost_net`/`costs::upgrade_cost_net` directly, so a future discount
added to the charge path's formula cannot silently miss the legality path
(or vice versa) — the two can no longer diverge because there is only one
formula left to read.

Everywhere else checked (`take_cost`/`can_take`, `tech_cost_net` used
identically by both `legal.rs`'s three call sites and `costs.rs`'s own
charge path, Barbarossa's/Bach's own dedicated discount functions which
apply to non-unit costs Homer/`mil_discount` never touch): single source of
truth already, no duplication found.

### 5.3 Per-turn/once-per-game limiters, `rust/src/state.rs` flags

Every `_used`/`_used_this_turn` flag (`tactic_action_used`,
`hammurabi_used`, `hammurabi_replaced_this_turn`,
`replaced_leader_this_turn`, `churchill_used`, `bach_upgrade_used`,
`ocean_liners_used`, `homer_used_this_turn`,
`caesar_double_politics_used`, `trade_food_as_resource_used_this_turn`,
`trade_resource_as_food_used_this_turn`) was grepped for every read and
write site. **Clean**: each flag has exactly one gate site (`legal.rs` or
`costs.rs`) and one set site (`apply.rs`/`costs.rs`/`economy.rs`), and
`economy::end_of_turn` resets all eleven in a single unbroken block
(`economy.rs:695-707`) — no flag is missing from that block, so none can
leak past its turn. Homer's own once-per-turn cap (`costs.rs:608-626`'s doc
comment) already documents a prior instance of this exact shape being found
and fixed; nothing further to fix here.

### 5.4 Duplicated `match` over the same enum in two files

Checked `Special`'s dispatch: `effects::apply_special` is a single,
deliberately-exhaustive match with NO wildcard arm (`effects.rs:911`, its
own comment: "Adding a 93rd variant to the generated enum breaks this match
at compile time, which is the entire point") that assigns every variant to
exactly one canonical handler location by comment convention (`effects.rs`
itself / `combat.rs` / `events.rs` / "handled elsewhere"). The many other
`match sp { ... }` sites across `events.rs`/`combat.rs`/`effects.rs` are
`find_map`/filter lookups for ONE specific variant each (`Special::Gain`,
`Special::StrongestPlayer`, etc.), not competing exhaustive enumerations —
this is already the `d9e52c6`-style "make divergence a compile error"
structure, not an instance of the bug shape. Not touched.

### 5.5 Age/era-dependent lookups

Follow-up pass closing the one family 5.1-5.4 left open. Every field that
carries an "age" meaning (`state.age_civil`, `state.age_military`,
`state.current_events_age`, a card's own printed `age`, `Line::age` in the
replayer) was enumerated, then every reader of each was grepped and read.
One group per candidate family, verdict per group. Full negative: every
group below is a single funnel or a verified-consistent sibling set, no
divergence found, nothing fixed.

| group | sites | verdict |
|---|---|---|
| Age transition funnel | `game::advance_age` (`game.rs:524-557`, sets `age_civil`+`age_military` together, rebuilds both decks, runs `antiquate`+the -2 `yellow_bank` deduction); `game::antiquate` (`game.rs:572-627`, cutoff = the age param `advance_age` passes it, single loop over hands/leader/wonder/pacts); `game::force_civil_age_at_least` (`game.rs:722-731`, the replayer's only hook, loops calling the SAME `advance_age`) | **clean**: one function performs every age-linked mutation (deck swap, antiquation, yellow-token loss); the replayer's catch-up path and the live-game path are provably the same code, not two formulas that could drift |
| `age_civil` vs `age_military` as two fields | `advance_age` (`game.rs:537-538`) is the ONLY non-test site that writes either; both are always set to the identical value in the same statement pair | **clean**: the two fields cannot diverge in a reachable game state, so a reader consulting the "wrong" one of the pair is a documentation nit, not a live bug |
| "No military draw in Age IV" gate | `economy::end_of_turn` step 4 (`economy.rs:678`, `state.age_military != Age::IV`); `interact::apply_immediate_effects` for territories (`interact.rs:1489`, same condition); `events::draw_military` (`events.rs:1168`, `== Age::IV` early-return, same polarity inverted) | **clean**, genuine sibling group, all three read `age_military` (never `age_civil`) with matching polarity |
| Age-keyed discard piles (`civil_discard`, `civil_removed`, `discarded_military`) | `economy::discard_civil`/`discard_military` (`economy.rs:461-473`, key = card's own age, falling back to the matching current-age field only for `CardId::NONE`); `game::replenish`'s row-sweep (`game.rs:472`, card's own age); `antiquate`'s calls into the two `economy::discard_*` functions; `economy::draw_military`'s reshuffle (`economy.rs:512`, keys the CURRENT age's own pile by `age_military`, correctly — only the age in progress can reshuffle) | **clean**, one semantic ("the card's own age when known, else the age in progress") applied identically everywhere |
| Leader one-per-age gate (`taken_leader_ages` bitmask) | Sole writer `apply.rs:545`; two readers `costs::can_take_gated` (`costs.rs:372`) and the diagnostic-only `costs::take_rejection` (`costs.rs:457`), both `gate.taken_leader_ages & (1 << (card.age as u8))` | **clean**, and already the target structural shape: `costs.rs`'s own `take_rejection_agrees_with_can_take_gated` test pins the two readers against each other on every fixture, so a future divergence between them would already be a test failure, not a silent bug |
| Per-age build-discount table (`Stats::build_discount: [i32; 5]`) | Sole producer `effects::compute`/`state_stats` (`effects.rs:1047`, indexed by `card.age as usize`); sole consumer `costs::build_cost_for` (`costs.rs:506`). Every build/upgrade cost call site (`apply.rs:645`, `legal.rs:456/497/686/734/740/981`) goes through `build_cost_for`/`build_cost_net`/`upgrade_cost_net` — confirmed no second inline formula remains after `a008990`'s `legal.rs` refactor | **clean**, single source of truth, no sibling to diverge |
| "Belongs to the current/older age" cutoffs that look similar but are different rules | Uprising's `discard_leader_unless_current_age` (`events.rs:1216`, `leader.get().age != state.age_civil` — discards a leader that does NOT match the current age exactly); `antiquate`'s hand/leader/wonder/pact culls (`game.rs:581/592/601/608/625`, `card.get().age as u8 < cutoff` — discards anything OLDER than the age that just ended) | **clean**: verified against RULES_SPEC.md and the card text that these are genuinely two different predicates (`!=` vs `<`), not the same rule reimplemented two ways with one side wrong |
| Raid / `DestroyUrbanBuildings` age cutoffs | Producers: `combat::resolve_aggression` (`combat.rs:478-487`, one `QueueItem::Raid` enqueued per age in the card's own printed list) and `events`'s "destroy one urban building of each opponent" (`events.rs:1305-1314`, `max_age: Age::IV` used as a "no cap" sentinel); sole consumer `interact.rs:994-999` (`id.level() <= max_lv`) | **clean**: RULES_SPEC 5.5 confirms Raid II/III destroy TWO buildings under two INDEPENDENT age caps (e.g. "one of Age I or older AND one of Age II or older") — enqueuing one `Raid` per printed age, each with its own `<=` cutoff, is the correct rule, not a duplicated/diverged cutoff |
| `FlipWonder`/`FreeBuild` age filters | Single producer (`events.rs`) / single consumer (`interact.rs:935-955` and `interact.rs:848-905` respectively) for each | **clean**, no second implementation exists to disagree with the first |
| Neural-encoder and advisor I/O age fields | `push_onehot_age` (`bots/neural/encode.rs:436`) is the ONE function encoding `age_civil`, `age_military` AND `current_events_age` (`encode.rs:446-448`); `advisor::state_io`'s `age_str`/`parse_age` (`state_io.rs:436/446`) round-trip all three, and the civil/military `"age=X/Y"` split-on-`/` order (`state_io.rs:966-968`) matches the dump order (`state_io.rs:652-653`) exactly | **clean** |
| `current_events_age` | Sole writer `events::sync_current_events_age` (`events.rs:613-616`); readers are only the encoder and the advisor dump/load pair above | **clean**, no independent recomputation anywhere |
| `best_age_sibling`'s coupling to `state.age_civil` (flagged by name in this pass's brief as a "documented coupling" to check) | `replay_common.rs:3971` is the ONLY call site in the whole tree | **clean**: there is no second age-blind card-name disambiguator for it to silently disagree with |

`game::force_civil_age_at_least`'s replayer-side deferral (the "run the age
catch-up only once `state.pending`/`state.queue` are both drained" fix) was
landed by a concurrent pass as commit `62befa4` while this inventory was
being built; re-checked here only for the specific question this section
asks (single funnel, no duplicate age-transition logic) and found to still
hold — not re-touched.
