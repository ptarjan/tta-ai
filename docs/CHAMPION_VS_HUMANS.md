# Champion vs. strong humans: where they disagree, and why

Corpus-wide run of `agreement.rs` (never run corpus-wide before), on the
strong-tier BGO corpus (Warlord + Emperor, 716 games, all with journals),
against the canonical gauntlet champions (`analysis/frozen/gauntlet/
champion_{2,3,4}p_gen{1454,1384,448}_140key_2026-08-06.json`), on
master `8513ae0` — i.e. tonight's engine fixes and replay-depth work.
170,626 decision points, run time ~4 minutes total (no sampling needed).

Supersedes `docs/AGREEMENT.md`'s numbers for anything by age past Age I:
that run (150 games, tier-mixed, pre-fix) essentially never reached Age
II+ because replays died early back then (n=20 total at Age II, n=0 at
III/IV). This run reaches Age III/IV in the majority of games (see
Completion below), so this is the first time this question has actually
been asked past the opening.

**Legality check (done first, as instructed): no violation found.**
`WeightedBot::evaluate` has exactly one term that reads a rival's hand by
card identity (`RivalHandPotential` → `cards::rival_hand_potential`,
`bots/weighted/cards.rs:1846`) — and its own comment states why that's
legal: taken civil cards sit face-up in a player's display (RULES_SPEC
2.6, "open civil cards convention"), so a rival's `hand_civil` is public
information, unlike military hand or deck order. Confirmed further: the
weight is 0.0 (absent/default) in all three champion files actually used
here, so the term doesn't even fire in this run. No code path anywhere in
`bots/weighted/` reads a *rival's* `hand_military` (every `hand_military`
read is on the acting player's own hand). Clean.

## Headline

**Top-1 agreement: 21.4% (36,489/170,626).** Where the human's move
wasn't the bot's favorite, it ranked 2nd–3rd 24.0% of the time, 4th–10th
35.4%, worse than 10th 19.1%. Excluding `end_turn` (the easiest, most
forced category), agreement is 17.8%.

Completion: 153/716 games (21%) replayed to the literal end with no
mismatch; but 73% of games now get through at least Age III before any
stop (523/716 reach III or IV, only 51/716 — 7% — still die in Age I).
That 7% is the number to compare against "used to die in Age I" — it's
now the small minority, not the rule.

Discard-tainted decisions (reached after an arbitrary BGO discard choice,
`docs/REPLAY.md`) agree less: 19.6% vs. 28.6% untainted — real but not
the main story.

### By age — reproduces the expected opening-humanlike / late-game-less-so pattern

| age | n | agree | rank1 | 2-3 | 4-10 | worse |
|---|---|---|---|---|---|---|
| A | 4,351 | **56.4%** | 56.4% | 22.2% | 21.1% | 0.3% |
| I | 65,282 | 23.4% | 23.4% | 28.1% | 34.9% | 13.6% |
| II | 57,528 | 19.0% | 19.0% | 23.3% | 36.8% | 20.6% |
| III | 37,011 | 17.3% | 17.3% | 18.3% | 36.0% | 28.2% |
| IV | 6,454 | 21.6% | 21.6% | 23.1% | 33.7% | 21.7% |

Confirms the known pattern: the champion's opening is unusually
human-like (56% top-1 in Age A, essentially never badly wrong — "worse"
is 0.3%), and it drifts away from human play as the game goes on, worst
at Age III. (Age IV's small uptick is probably just fewer, more forced
late-game legal-move lists, not a real return to human-likeness — n
there is 9x smaller than II/III.)

### By category (n≥50, worst first)

| category | n | agree | bot's alternative when it disagrees |
|---|---|---|---|
| take_card | 43,086 | **4.0%** | Build 30%, Take-a-different-card 25%, Develop 19%, PlayAction 10% |
| aggression_or_war | 836 | **8.5%** | PrepareEvent 56%, PolPass 25%, a different War/Aggression 13% |
| increase_population | 11,700 | **11.5%** | Build 33%, Develop 23%, Take 16%, PlayAction 13% |
| leader_or_wonder_step | 13,425 | 17.4% | Build 45%, Develop 21%, Take 15%, PlayAction 10% |
| build | 33,657 | 22.2% | (mostly a different Build/Develop) |
| bid | 5,992 | 22.5% | — |
| tactics | 2,918 | 22.7% | — |
| other | 22,608 | 28.6% | — |
| political_action | 11,387 | 38.9% | — |
| end_turn | 24,509 | 42.5% | — |
| pact | 508 | 45.7% | — |

`take_card`/`aggression_or_war`/`increase_population`/`leader_or_wonder_step`
are the four worst, and three of them (all but `aggression_or_war`) share
one signature: when the bot disagrees with a human's Take/Pop/Leader/Wonder
move, its own preferred alternative is overwhelmingly **Build or Develop**
(49–65% combined) — well above Build+Develop's 40% baseline share of the
bot's #1 pick across *all* decisions. That's a real, specific lean, not
just "Build is common."

## The why: four disagreements traced to real positions

### 1. take_card — the dominant sub-story is "should you even take a card"

`take_card`'s legal-move list isn't "which of ~6 row cards" — it's the
*entire* turn menu (the row averages ~17–20 affordable slots across civil
+ military rows), so this category answers "did the human choose to draw
at all, vs. build/develop/play what's already in hand." 60% of the time
the bot disagrees, its alternative isn't a different card — it's *don't
take, build instead*.

- **Game 7522053, Age II, line 234**: Purple takes Eiffel Tower into hand
  (2 civil actions), then next turn builds a stage of it. Bot's #1 was
  `Build { Religion }` — a card already in hand — instead.
- **Game 7521703, Age II, line 199**: Green takes Reserves, then
  immediately takes Alchemy too (2 civil actions on 2 separate takes, no
  building at all that turn). Bot wanted `Build { Religion }` again.
- **Game 7523104, Age II, lines 155–160**: the clearest case — Purple
  takes Cannon, *puts it back* (refunding the civil action), takes
  Scientific Method, *puts that back too*, then finally re-takes Cannon
  and takes Patriotism. The human visibly deliberated between options in
  real time before settling on Cannon over the bot's preferred
  alternative slot — this one reads as considered, not a misclick.

Verdict: mixed, genuinely can't fully resolve from static eval scores.
The clean pattern (build now vs. stock up hand for later) matches a
known, general TTA principle — humans hold cards in hand as banked
options and often take before they strictly need to, since row cards
disappear and refill costs escalate with position. A 1-ply evaluator that
prices "card in hand, unbuilt" mostly by its face value rather than its
option value would systematically undervalue taking. Can't prove it here
without a multi-turn counterfactual (out of scope for this pass), but the
direction and volume (43k decisions, 25% of the whole corpus) make this
the single most consequential category to look at next.

### 2. aggression_or_war — the champion looks passive

When a human declared War or Aggression and the bot disagreed, 81% of the
time the bot's own preferred move was to *not* fight — `PrepareEvent`
(56%) or pass the political phase (25%) — options that are direct
alternatives in the same political-action decision, not different phases.

- **Game 7520718, Age II, line 217**: Purple declares War over Territory
  on Orange (yellow-token grab off a strength edge), then spends the rest
  of the turn teching/building normally — no visible backlash. Bot wanted
  `PrepareEvent`.
- **Game 7523030, Age II, line 174**: same shape — Purple declares War
  over Territory, then plays Napoleon and builds two Cavalrymen. Again no
  visible cost to the war itself.
- **Game 7523016, Age I, line 96**: Orange raids Purple (destroys an urban
  building) — and Purple **concedes defeat** four lines later. Caveat:
  this could be an unrelated early resignation (round 7, Age I is very
  early to quit), not necessarily caused by the raid — can't tell from
  the journal alone, and early concessions like this are a real
  contamination risk for this category's sample generally.

Verdict: leans toward a real champion weakness — the wars sampled here
mostly look like clean, low-downside points grabs that a strong human
took and the engine's own scoring afterward doesn't show punishment for.
But this is the least certain finding here: `aggression_or_war` is a
comparatively small category (836 decisions, 0.5% of the corpus) and BGO
early resignations can bias it. Worth a dedicated look (does the champion
under-produce war/aggression in self-play too, independent of human
comparison) before trusting this as a fix target.

### 3 & 4. increase_population and leader_or_wonder_step — same "invest vs. cash now" shape

- **Game 7523818, Age III, lines 241/242**: Purple increases population
  twice in a row (3 food, then 4 food) before building two Cannons and
  Religion. Bot's alternative to the first Pop was `WonderStep`, to the
  second was `Build { Religion }` — i.e. convert resources into a
  building right now rather than grow the population base.
- **Game 7523818, Age I→II, line 122**: Orange takes and immediately
  elects **Leonardo Da Vinci** as leader. Bot's #1 was
  `PlayAction { Urban Growth (I) }` instead.
- **Game 7523818, Age II, lines 164/165**: Orange takes Kremlin, then
  builds two stages of it back-to-back. Bot wanted
  `PlayAction { Breakthrough (II) }` both times — cash an action card for
  an immediate effect instead of advancing a wonder that pays off over
  many future turns.

Verdict: population growth and leader/wonder investment are exactly the
kind of compounding, long-horizon moves that strong TTA players are
taught to prioritize (more workers/wonders now means more of everything
for the rest of the game) and that a 1-ply, largely-static evaluator
structurally underprices relative to an immediate, visible building
payoff. This is the same underlying shape as finding #1, just showing up
in three different move types — call it one finding, not four: **the
champion is short-horizon relative to strong humans, and it costs it most
on population/leader/wonder/take decisions specifically**, matching the
"human-like opening, less human-like later" pattern in the age table
(these compounding investments matter more as the game gets longer).

## What I did not chase

- Whether the champion's own *self-play* win rate would improve by
  reweighting toward take/pop/leader/wonder investment — that requires
  training runs, out of scope for this analysis pass.
- `events::food_or_resources`'s hardcoded resources-first order (flagged
  in the brief): checked, and it **cannot appear in this data at all** —
  it's an automatic effect resolution for multi-player event gain/lose
  blocks (no attacker to ask), applied without ever opening a
  `Pending::Choice`, so `agreement.rs` — which only records decisions off
  the `legal_moves` list — structurally never sees it. The separate,
  genuinely-player-facing `ChoiceKind::FoodOrRes` (single-card "gain N
  food or resources" effects) *does* go through a real ranked decision
  and is included above (bucketed `other`, per `agreement.rs`'s own
  category doc) — no visible anomaly there.
- Did not re-run `--planbot` (the beam-search secondary comparison) —
  budget went to the corpus-wide pass and the trace-down instead, per the
  brief's own priority ("the report matters more than the last decimal").

## Reproduce

```
cargo build --profile difftest --bin agreement   # from rust/
./target/difftest/agreement ../sources/bgo/index.tsv <journals_dir> <weights_dir> $(strong-tier ids)
```
Weights dir needs `rust_champion_{2,3,4}p.json` filenames — copy the three
`analysis/frozen/gauntlet/champion_*_2026-08-06.json` files under those
names into a scratch directory (not committed; the gauntlet files are the
citable, committed reference).
