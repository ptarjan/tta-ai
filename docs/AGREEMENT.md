# Move-agreement analysis: does the bot's own preference match the human's, move by move?

Phase 2 of the project `REPLAY.md` calls its prerequisite ("prerequisite for
a future move-agreement analysis; not that analysis itself"). Phase 1 built
`rust/src/bin/agreement.rs` (reusing `replay_common.rs`'s reconstruction —
see `REPLAY.md`) and `WeightedBot::rank_moves` (`bots/weighted/eval.rs`).
This doc is that analysis: at every point `replay_common.rs`'s
reconstruction reaches a real, journal-observed human decision, what would
the champion `WeightedBot` have played instead, and where they differ, why.

**Headline**: overall top-1 agreement is **26.4% (2,493/9,428, 95% CI
25.6–27.3%)** — the bot's single best move matches the human's roughly one
time in four. The single most concrete, well-evidenced finding in this
analysis is not a small tweak: at the exact decision points where a human
elected a leader, completed a wonder stage, or grew their population, the
bot's own top-ranked alternative is a plain building action **55–85% of the
time**, consistently across all three player counts (see "The dominant
pattern" below) — a 1-ply evaluator systematically pulling toward immediate,
visible building value over longer-payoff investments.

**No rules bug found.** Nothing in this pass's output looked like a legal
human move producing a decision point that shouldn't exist; every mismatch
traces to a judgement difference or a reconstruction artifact (see
"Discard/military taint" below), not an engine defect.

## Regeneration

```text
cd rust
tar -xzf ../sources/bgo/journals.tar.gz -C /tmp/bgo-journals   # once

# game selection: first 50 per player count in index.tsv order, no
# cherry-picking (692x2p/133x3p/186x4p exist in the corpus, so 50/50/50 was
# available at every count)
IDS=$(for n in 2 3 4; do awk -F'\t' -v n=$n \
    'NR>1 && $3==n{print $1}' ../sources/bgo/index.tsv | head -50; done)

cargo run --profile difftest --bin agreement -- \
    ../sources/bgo/index.tsv /tmp/bgo-journals/journals ../experiments \
    $IDS > ../agreement.tsv 2> ../agreement.stderr

cd ..
python3 tools/aggregate_agreement.py agreement.tsv
```

Sample: 150 games (50×2p, 50×3p, 50×4p), all completed cleanly (no crash, no
unhandled `IllegalMove`/`StuckPending` visible in `agreement.stderr` beyond
`replay`'s own already-documented early stop — see `REPLAY.md`). 9,428
decision points recorded (62.9/game average). Wall time ≈35–40ms/game.

`tools/aggregate_agreement.py` is the aggregation script this doc's numbers
come from (stdlib-only Python, reads the TSV schema `agreement.rs`'s own
module doc specifies).

## BGO skill tier

`sources/bgo/index.tsv`'s `level` column (Prince/King/Warlord/Emperor) IS a
real skill/rating field — already parsed by `corpus::GameMeta::tier` and
now surfaced as `agreement.rs`'s `tier` column (added in phase 2). No proxy
was needed or used.

## Overall agreement, with n

| slice | k/n | rate | 95% CI |
|---|---|---|---|
| all decisions | 2,493/9,428 | 26.4% | 25.6–27.3% |
| excluding `discard_tainted` | 1,928/6,644 | 29.0% | 27.9–30.1% |
| `discard_tainted` only | 565/2,784 | 20.3% | 18.8–21.8% |

Excluding taint moves the rate ~2.6 points — real, but not the dominant
driver of the low overall number (see "Discard/military taint" below for
why the excluded-vs-included gap is smaller than the taint SHARE would
suggest).

### By player count

| players | k/n | rate | 95% CI |
|---|---|---|---|
| 2p | 688/2,350 | 29.3% | 27.5–31.1% |
| 3p | 783/3,295 | 23.8% | 22.3–25.2% |
| 4p | 1,022/3,783 | 27.0% | 25.6–28.5% |

### By game age

| age | k/n | rate | 95% CI |
|---|---|---|---|
| A | 571/1,145 | 49.9% | 47.0–52.8% |
| I | 1,920/8,263 | 23.2% | 22.3–24.2% |
| II | 2/20 | 10.0% | 2.8–30.1% |
| III / IV | — | n=0 | — |

Handled gracefully, not a bug: `replay.rs`'s reconstruction stops early on
every sampled game (mean ~63 actions before a stop — `REPLAY.md`), so this
150-game sample essentially never reaches Age III/IV; `age` here is
`GameState::age_civil`, read structurally, not the journal's own age
column. Age A's much higher agreement (49.9%) is expected and not that
interesting on its own — round 1's legal-move list is short and dominated by
`Take`/`EndTurn`, and there is very little of substance a linear evaluator
can get wrong yet.

### By move category

Phase-2 revision of phase 1's mapping (documented in `agreement.rs`'s own
`Category` doc comment): `Develop`/`Upgrade` now fold into `build` (both are
building actions under TTA's rules, not a separate decision shape); `Bid`/
`BidPass` now get their own `bid` bucket, split out of `other`, specifically
so the colonization-auction census finding is testable at the move level.

| category | k/n | rate | 95% CI |
|---|---|---|---|
| take_card | 216/2,862 | 7.5% | 6.6–8.6% |
| increase_population | 105/901 | 11.7% | 9.7–13.9% |
| leader_or_wonder_step | 167/1,192 | 14.0% | 12.2–16.1% |
| build | 218/973 | 22.4% | 19.9–25.1% |
| political_action | 172/566 | 30.4% | 26.7–34.3% |
| other | 357/968 | 36.9% | 33.9–40.0% |
| tactics | 40/104 | 38.5% | 29.7–48.1% |
| pact | 15/38 | 39.5% | 25.6–55.3% |
| aggression_or_war | 3/8 | 37.5% | 13.7–69.4% |
| bid | 13/26 | 50.0% | 32.1–67.9% |
| end_turn | 1,187/1,790 | 66.3% | 64.1–68.5% |

The last four rows (`tactics`, `pact`, `aggression_or_war`, `bid`) have real
but thin n — read their point estimates as directional, not precise; see
"Military/diplomacy categories" below for per-row detail and the
discard-taint caveat that hits them hardest.

### By BGO skill tier

| tier | k/n | rate | 95% CI |
|---|---|---|---|
| Prince | 305/1,089 | 28.0% | 25.4–30.7% |
| King | 470/1,740 | 27.0% | 25.0–29.1% |
| Warlord | 647/2,665 | 24.3% | 22.7–25.9% |
| Emperor | 1,071/3,934 | 27.2% | 25.9–28.6% |

Flat across tiers (24–28%, overlapping CIs) — consistent with
`HUMAN_PLAY.md`'s own standing finding that BGO skill tier barely moves
play-rate statistics; it doesn't move move-level agreement either. Do not
read tier as a confound for any breakdown above.

## Discard/military taint

29.5% of all 9,428 decisions (2,784) occur downstream of at least one
already-resolved `Pending::Choice(DiscardMilitary)` this game that
`DiscardSolver` could not deduce with certainty (`chosen`/`forced_collision`,
never `solved` — see `REPLAY.md`'s discard-solver section). Every card
identity in that player's simulated military hand from that point on is a
guess, not a fact, so the position the bot is asked to evaluate is slightly
fictional.

The taint is NOT evenly spread across categories — it concentrates exactly
where a real game's timeline concentrates military-hand decisions:

| category | tainted / n | share |
|---|---|---|
| bid | 26/26 | 100% |
| aggression_or_war | 8/8 | 100% |
| tactics | 77/104 | 74% |
| pact | 16/38 | 42% |
| take_card / build / increase_population / leader_or_wonder_step / political_action / end_turn | — | 21–31% (near the 29.5% overall average) |

**Every single `bid` and `aggression_or_war` decision point in this sample
is discard-tainted.** This is expected, not a bug: bidding and
war/aggression both happen late enough in a real game (auctions after
several rounds of play; a declared war/aggression after several rounds of
military draws) that at least one discard has essentially always already
fired by the time either category is reached in a game this reconstruction
can still follow. It means the "Census cross-check" verdicts for these two
categories below are graded on a much shorter leash than `take_card`/
`build`/`leader_or_wonder_step` — read them as suggestive, not conclusive.

## Human-rank distribution among disagreements (n=6,935)

| rank | share | | rank | share |
|---|---|---|---|---|
| 2 | 22.6% | | 9 | 5.0% |
| 3 | 15.1% | | 10 | 3.3% |
| 4 | 9.6% | | 11–15 | 10.1% (combined) |
| 5 | 7.2% | | 16–20 | 4.3% (combined) |
| 6 | 8.4% | | 21+ | 1.4% (combined) |
| 7 | 7.4% | | | |
| 8 | 6.0% | | | |

`uncounted`: 0 — as expected, since `human_move` is always drawn from the
same `legal_moves` list the bot ranked (`agreement.rs`'s own doc comment).
Nearly a quarter of all disagreements (22.6%) are the bot's own SECOND
choice — a near-miss, not a blind spot — and the top three ranks alone
(2–4) cover 47.3% of disagreements. The tail is real but small: only 1.4%
of disagreements have the human's move ranked 21st or worse out of what is
often a large candidate list.

Per-category shape (full distributions in the aggregation script's own
output; summarised here):

- **`build`** disagreements cluster HARD at rank 2–3 (30.1% + 27.8% = 57.9%
  of its 755 disagreements) — when the bot disagrees on *which* build, it is
  almost always a close second choice, not a rejection of building itself.
- **`take_card`** disagreements are comparatively FLAT across ranks 2–9
  (roughly 6–14% each) — a long, even tail, not a single dominant
  alternative (see "take_card" deep-dive below for why).
- **`leader_or_wonder_step`** and **`increase_population`** cluster at
  ranks 2–7 with no single dominant rank — consistent with "the bot usually
  has ONE clearly preferred alternative (a build), and the human's move
  lands somewhere in the upper-middle of the list," not "barely considered
  at all."

## The dominant pattern: a `build`-now bias against wonder/leader/population investment

This is the headline qualitative finding. At `leader_or_wonder_step`
decisions (a human just played `PlayLeader` or paid for a `WonderStep` —
1,192 decision points), the bot's own #1-ranked alternative is a plain
`Build` **65% of the time (775/1,192)** — not spread across many
alternatives, concentrated on one. Split further:

- `WonderStep` (718 decisions): bot's top choice is `Build` **66.1%**
  (475/718); only 94 times (13.1%) does the bot's own top choice agree the
  wonder stage was right, and only 46 times does it prefer a DIFFERENT
  wonder stage.
- `PlayLeader` (474 decisions): bot's top choice is `Build` **63.3%**
  (300/474); only 89 times (18.8%) does it agree electing a leader was
  right.

**This holds at every player count, not just 2p** — 55.4% at 2p (164/296),
65.3% at 3p (279/427), 70.8% at 4p (332/469). That cross-player-count
consistency is a genuine refinement of `HUMAN_PLAY.md`'s own prior
diagnosis: that doc found `wonder_turns_to_finish` (a weight that penalises
a slow-to-complete wonder) is a striking 2p-specific outlier
(-6.819 vs -0.440 at 3p, +0.022 at 4p) and flagged it as a PLAUSIBLE,
NOT-CONFIRMED explanation for 2p's near-zero wonder-building rate. Since the
"prefer build over wonder step" pattern holds just as strongly (or more so)
at 3p/4p, where that specific weight is near zero or even positive, **it
cannot be the whole story** — this looks like a broader evaluator property
(building's IMMEDIATE, visible payoff outcompetes a wonder/leader
investment's deferred one under a 1-ply lookahead), not a single
misconfigured coordinate.

The same shape appears at `increase_population` decisions (901 of them,
human played `Pop`): the bot's own top choice is `Build` **45.8%** of the
time (413/901), a further 24.0% (216/901) is a `Take`, and only 11.7%
(105/901) agrees `Pop` was right.

**Concrete example** (clean, `discard_tainted=false`): game `7520707`
(Warlord, 3p), journal line 54 — `"Green builds 1 stage of Hanging Gardens
Green spends 2 resources"`, reached after Green spent the same turn taking
`Reserves` and `Alchemy`. The bot's own top choice at that exact position
was `Build { card: Religion }`, valued 112 points higher (580.9 vs 468.9,
+24%) than the wonder stage the human actually paid for; the human's real
move ranked 6th of 19 legal candidates. Three more `WonderStep`-vs-`Build`
instances in the SAME game (lines 64/72/73/74, all `Build { card: Religion
}` as the bot's preferred alternative, gaps of 14–24%) show this was not a
one-off read of that position but a stable preference across the whole
game.

**Concrete example** (leader election, clean): game `7522510` (Emperor,
3p), line 51 — `"Green elects Michelangelo"`. Bot's own top choice: `Build
{ card: Religion }` (554.3 vs 461.7, +20%); human's move ranked 2nd of 17 —
a closer call than the wonder-stage examples, but the same direction.

**Verdict: BOT IS WRONG** (a genuine strategic weakness, not a corpus
artifact or a case where the human's play looks arguably worse) — a 1-ply
linear evaluator undervalues investments (wonder progress, leader
elections, population growth) whose payoff is not realised on the very next
turn, relative to an ordinary building action whose value is booked
immediately. This is the single most actionable finding in this analysis;
`HUMAN_PLAY.md`'s own census (bot builds 0.01–1.13x the human wonder-stage
rate depending on player count) is the outcome-level symptom this document
supplies the move-level mechanism for.

**Methodology caveat, stated plainly**: this analysis does not control for
how many `Build`-shaped options exist in the legal-move list at each point
(a real TTA position often has multiple buildable cards in hand plus row
technologies, i.e. more DISTINCT `Build` candidates than there are
`WonderStep`/`PlayLeader` candidates, which are usually one or two). Some of
`Build`'s share of "bot's #1 pick" could be option-count base rate rather
than pure per-option overvaluation. Two things argue the effect is real
regardless: the size of the individual score gaps (14–24% relative, not
marginal near-ties), and that a genuinely flat per-option evaluator would
still need to rank the SPECIFIC wonder/leader move ABOVE every individual
build candidate to agree — which it does at only 13–19% of these decisions,
well below what a large but evenly-priced candidate pool would predict.

## `take_card`: mostly a "which card," not "whether to take," disagreement

`take_card` is the highest-volume category (2,862 decisions, 30.4% of the
whole sample) and has the LOWEST agreement rate (7.5%). Breaking down the
bot's own top-choice variant when a human took a card:

| bot's top choice | share |
|---|---|
| `Take` (a different slot) | 43.2% (1,235/2,862) |
| `Build` | 36.4% (1,043/2,862) |
| everything else (`EndTurn`/`Pop`/`Develop`/`PlayAction`/`PlayLeader`/`PlayTactic`/`WonderStep`/...) | 20.4% |

So the bot is NOT broadly averse to the take-card action type at these
decisions — 43.2% of the time its own #1 choice is also a take, just of a
different card — but the specific card/slot rarely matches (agreement drops
to 7.5% once slot has to match exactly), and a substantial 36.4% slice is
the SAME `build`-now pull documented above. Rank distribution for
disagreements is comparatively flat (6–14% at every rank from 2–9, no sharp
peak) — consistent with "many candidate row cards score close together,"
unlike `build`'s sharp rank-2/3 cluster.

**Concrete examples**: game `7523809` (Emperor, 2p), line 23 —
`Take{slot:0}` vs bot's `Take{slot:4}`, essentially tied (217.6 vs 217.8,
rank 2). Game `7522525` (Emperor, 3p), line 40 — `"Orange takes Genghis
Khan in hand Orange uses 2 civil action"` vs the bot's `Build { card:
Religion }` (540.8 vs 582.2, +7.6%, rank 2) — a leader-card TAKE
specifically, one step upstream of the leader-election pattern above.

**Verdict: mostly BOT IS WRONG on card/slot identity (a real but harder-to-
characterise weakness — this project's own standing finding, `HUMAN_PLAY.md`'s
behaviour-cloning section, is that "the clone still misses badly on takes
specifically... because the evaluator has no feature that distinguishes one
row card from another beyond a linearised `hand_potential` term — a feature
gap"), PARTIALLY the same build-now bias identified above.** Not an
artifact: `take_card`'s taint share (see the taint table) sits at the
sample average, not elevated.

## Military/diplomacy categories: thin, heavily tainted, but still informative

### `aggression_or_war` (n=8, 100% discard-tainted)

Too thin for a confident verdict on its own, but the SHAPE contradicts a
naive "the bot never considers fighting" reading. Two of eight are
outright agreements (rank 1); of the six disagreements, the bot's own
top-ranked alternative is ANOTHER aggression/war move (a different target
or card) in at least one case (`7522668`: human declared `Raid` on seat 3,
bot preferred `Raid` on seat 2, a 0.4% score gap — barely a disagreement at
all) — i.e. the bot isn't rejecting military action as a category, it
sometimes just prefers a different target. Concrete example: game
`7523355` (King, 2p), line 87 — `"Purple plays Enslave against Orange"`
(after several turns of building, a leader-completed wonder, and a
military draw/discard cycle) — bot's alternative was `PrepareEvent`,
human's move ranked 3rd of 4, an 8.3% gap. **Verdict: sample too small and
100% tainted for a standalone verdict — see the census cross-check below
for how this interacts with the outcome-level finding.**

### `pact` (n=38, 42% tainted) — proposing vs. accepting split cleanly

This category mixes two structurally different decisions and they behave
oppositely:

- **Accepting** (`Move::Choose` resolving an open `PactOffer`, 13
  decisions): agreement 77% (10/13) — when a human accepts a proposed
  pact, the bot's own valuation usually agrees.
- **Proposing** (`Move::OfferPact`, 12 decisions where the human is the one
  proposing): agreement 17% (2/12) — and tellingly, in ZERO of the 10
  disagreements is the bot's own top alternative ALSO a pact proposal (just
  a different partner/card); it is most often `PrepareEvent` (5/10),
  `Aggression` (2/10), or `PolPass` (2/10). At the SPECIFIC positions where
  a human chose to propose, the bot's own top-ranked move essentially never
  chooses to propose anything.

**Verdict: COMPLICATES the census finding** (see below) rather than
confirming it outright — see the cross-check table for the reconciliation.

### `bid` (n=26, 100% discard-tainted, thinnest per-player-count: 2p n=2, 3p n=10, 4p n=14)

Once a colonization auction is already open (i.e. the decision point
exists at all), the bot mostly still wants to bid: of 13 disagreements, only
1 has the bot preferring `BidPass` over the human's actual bid — the other
12 are the bot ALSO choosing to bid, just a different amount, often close in
score (e.g. game `7523818`, line 106: human `Bid{n:2}`, bot's top `Bid{n:5}`,
753.2 vs 756.6, a 0.45% gap). **Verdict: the 100% taint rate and n=26 mean
this cannot be treated as conclusive, but what data exists points AWAY from
"the bot avoids bidding once asked" and toward "the bot doesn't often end
up at this decision point in its own games in the first place"** — see the
cross-check below.

### `tactics` (n=104, 74% tainted)

Bot's own top choice is also `PlayTactic` 43.3% of the time (45/104), `Build`
27.9% (29/104), `EndTurn` 17.3% (18/104). Similar shape to `take_card`: not
a wholesale rejection of the category, but a real pull toward `build`
alongside genuine card-choice disagreement. Given the 74% taint rate
(tactics are drawn from the same military deck a discard removes cards
from), this category's numbers should be read as a lower-confidence
signal specifically because of that overlap, not despite it.

## Census cross-check

`HUMAN_PLAY.md`'s bot-vs-human play-RATE census (self-play games, aggregate
counts) and this document's move-level analysis (real human games, one
decision at a time) are different instruments; here is what each of the
five named findings looks like from this side, and whether it corroborates,
contradicts, or complicates the rate-level story.

| census finding | this analysis | verdict |
|---|---|---|
| Bot declares ~0 wars/game (2p) vs human 0.5–0.6/game | n=8, 100% discard-tainted; 2/8 outright agree, and disagreements sometimes still prefer a DIFFERENT war/aggression, not avoidance | **Complicates.** Move-level evidence doesn't show the bot rejecting war/aggression as a category at the (rare, tainted) points this sample reaches it — but the sample is too thin and too tainted to confirm or refute the census's near-zero rate. |
| Bot plays far fewer aggressions/tactics | `tactics`: bot's own top choice is ALSO a tactic 43% of the time; real but partial `build`-pull (28%) alongside genuine card-choice mismatch | **Partially contradicts.** The self-play rate gap looks bigger than a pure "doesn't want to" story — at real positions the bot's own preference is still often *a* tactic, just not the *same* tactic. |
| Bot takes cards from the row 35–40% below human rate, stable across player counts | `take_card` agreement is low (7.5%) but bot's own top choice is STILL a take 43% of the time; the rest splits mostly to `build` (36%) | **Contradicts the "avoids taking" framing, corroborates the underlying mechanism.** The census's take-rate gap is more consistent with the SAME `build`-now bias documented above (bot spends more of its OWN turns building) than with a standalone dislike of the take action. |
| Bot over-proposes pacts (6–8x human rate) but gets fewer accepted | Accepting: 77% agreement (bot usually agrees a good pact should be taken). Proposing: 17% agreement, and the bot's own alternative is NEVER another pact proposal | **Complicates sharply.** At the specific board states where a HUMAN chose to propose, the bot's own valuation essentially never independently arrives at "propose a pact" as the best move — which sits awkwardly next to a self-play rate 6–8x human's. The two are not directly comparable (self-play games visit structurally different states than this human corpus), but this is a real tension worth flagging for anyone extending this work, not a confirmation. |
| 4p bot barely contests colonization auctions | Once a bid decision is already open, the bot mostly still wants to bid (12/13 disagreements are STILL a bid, different amount); 4p n=14 only, 100% tainted | **Contradicts the "declines to bid" framing, on thin/tainted evidence.** What little data exists points toward the census gap coming from the bot not REACHING bid decisions as often (a difference in earlier play, e.g. fewer territory auctions won or contested to begin with) rather than declining once asked. Not confirmed — n=14 at 4p specifically is too small to lean on hard. |

## PlanBot subsample: skipped

`bots::plan::pick`/`pick_collecting` (the deeper beam-search bot) does not
expose a `(&self, state, moves) -> Vec<(Move, f64)>`-shaped API the way
`WeightedBot::rank_moves` does — it needs a `PlanConfig`, a mutable `Stats`
accumulator, a mutable `pending::Counters`, and a seeded `PyRandom` threaded
through a multi-sample beam search, and its own per-candidate scoring
(`totals: Vec<(Move, f64, u32)>`, averaged over however many of `cfg.samples`
determinized rounds actually sampled each candidate) is closer to what
`rank_moves` needs than `choose` is, but still requires writing a new
ranking-shaped wrapper plus non-trivial config/rng plumbing in `agreement.rs`
to use it — not a drop-in bot substitution. Per this task's own "skip if
nontrivial, don't spend a lot of time" instruction, this was not attempted
this pass. Left as the natural next step for anyone continuing this work
(`bots/plan.rs`'s `pick_collecting` is the concrete starting point).

## What to reconsider before scaling this further

1. **Option-count control.** No breakdown here controls for how many
   candidates of each "shape" exist in a given `legal_moves` list — see the
   caveat under "The dominant pattern."
2. **Taint is concentrated, not diffuse** — treat `bid`/`aggression_or_war`
   (100% tainted in this sample) as directional only until either the
   sample grows enough to find untainted instances, or discard identity
   becomes recoverable by some other means.
3. **This sample never reaches Age III/IV** (`replay.rs`'s own early-stop
   ceiling, `REPLAY.md`) — none of this document's findings can speak to
   late-game play at all.
4. **A 1-ply evaluator, not the ship policy.** `WeightedBot::choose`/
   `rank_moves` is exactly what real games are scored with in this repo's
   `arena`/`climb`, but `HUMAN_PLAY.md`'s own behaviour-cloning section
   found that a conclusion drawn under one search depth need not hold under
   another (`plan:width=8` inverted a 1-ply strength comparison). This
   document's findings are about the 1-ply evaluator specifically, not a
   claim about what a deeper search would prefer at the same positions.
