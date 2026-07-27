# What strong humans actually do, and where our bot isn't human (2026-07-27)

Owner of this doc: the BGO-analysis pull. `docs/BGO_CORPUS.md` owns the scrape
itself and is not edited from here.

This is the **first external anchor this project has ever had.** Everything
before it was our bots playing our bots, with `docs/UNATTENDED.md`'s "Open,
ranked" item 1 (*"No external anchor"*) as the standing admission. The 1,011
BGO journals in `sources/bgo/journals.tar.gz` now parse, and our champion has
been measured on the same axes with the same code path.

Read the "What this cannot tell you" section before quoting any number.

## What runs

    tar xzf sources/bgo/journals.tar.gz -C /tmp/bgo
    python3 tools/bgo_parse.py --journals /tmp/bgo/journals \
        --index sources/bgo/index.tsv --out /tmp/human.tsv
    python3 tools/bgo_stats.py --tsv /tmp/human.tsv --players 2 --cat
    nice -n 19 python3 tools/bgo_botmatch.py --players 2 --games 40 \
        --spec quiesce:experiments/league_state/champion_2p.json,levels=1 \
        --out /tmp/bot_2p.tsv
    python3 tools/bgo_stats.py --tsv /tmp/human.tsv --vs /tmp/bot_2p.tsv --players 2

`tools/bgo_botmatch.py` emits the **same TSV schema** as `tools/bgo_parse.py`
on purpose: a field is only comparable if both sides derive it the same way,
and sharing `FIELDS` makes a schema drift a crash rather than a silent
mismatch. `tools/bgo_stats.py` prints medians, IQRs, and a **cluster
bootstrap** 95% CI that resamples *games*, not player-rows, because the two
seats of one game are not independent (one player's war is the other's
defence, and both see the same card row).

## Parse coverage: complete enough to trust

* **1,011 / 1,011 journals parsed, 0 failures, 2,526 player-rows.** Expected
  2,527 (692x2 + 133x3 + 186x4); the missing row is game `7521535`, the one
  game in the corpus with no `End of game` line.
* **0 unresolved card names** out of 81,646 takes, after mapping five BGO
  spellings (`Stockpile`→`Stock Pile`, `Leonardo Da Vinci`, `Charles Chaplin`,
  `Maximillien Robespierre`, `Johannes Sebastian Bach`, `Bread & Circuses`).
* **All 515 war-resolution lines matched** to one of the 529 declarations. The
  14 unmatched declarations are wars declared on the last turn that the game
  ended before resolving — a real fact about the games, not a parse miss.
* **6,786 take-backs cancelled.** BGO lets a player undo a take inside their
  own turn (`takes X ... / puts X back in the row ...`), and both lines are in
  the journal. Counting them would have inflated takes by 8% and, worse,
  biased the CA-cost histogram toward whatever people tried and reconsidered.
* Residual unknowns: 539 takes (0.66%) whose reconstructed row tier fell
  outside 1-3, and 483 takes (0.6%) BGO logged with no cost clause. Both are
  counted in `tier_unknown` / `takes_free` rather than being silently binned.

### Two traps the corpus sets, both of which bit this parse first

1. **A wonder take costs more than its row slot.** `engine/actions.py`
   `take_cost` adds `+1 per completed wonder`, which is why the corpus contains
   takes logged at 4, 5, 6 and 7 civil actions — all of them wonders. The
   parser tracks completed wonders per player from the stage lines and
   subtracts, so the reported *row tier* means the same thing on both sides.
   Reading the raw CA number as a slot cost would have invented a "humans
   sometimes pay 7 CA for a card" finding out of a rule we already model.
2. **Card names repeat across ages.** `Urban Growth` is in the A, I, II *and*
   III decks; `Reserves` in I, II, III; ~15 action-card names in total, and
   they are the most-taken cards in the game. The journal prints only the name.
   The first version of this analysis keyed "cards taken per age" off a
   name→age table and produced "humans take 8.3 age-A cards per 2p game",
   which is impossible (the whole 2p age-A deck is 20 cards). **Per-age counts
   are keyed off the journal's own age column** — the age the *game* was in
   when the card was taken — on both sides.

`tests/test_bgo_parse.py` (13 tests) pins both of those plus take-backs,
Hammurabi's military-action payment, defender-wins-the-war, and the
`elects <new leader> <old leader> dies;` template. Two of those tests failed
on first run and were catching real bugs: leader elections were being
**undercounted by 39%** (5,591 vs the correct 9,187) because the election
regex truncated `Leonardo Da Vinci Hammurabi dies` to `Leonardo`, and
`discovers X using Breakthrough` was missing government changes. Both numbers
looked entirely plausible while they were wrong. That is the failure mode this
corpus is most exposed to, and it is why the tests exist.

## The human baseline

All figures are per player per game unless the row says per-GAME. n_games is
the unit the CI is computed over.

| axis | 2p (n=692 games) | 3p (n=133) | 4p (n=186) |
|---|---|---|---|
| game length, rounds | 19 [19-20] | 19 [18-19] | 19 [19-20] |
| final score (culture) | **156** [121-190] | 180 [140-211] | 182 [145-237] |
| winner's margin over 2nd | **33** [14-61] | 24 [10-43] | 23 [12-43] |
| unspent science at end | 14 [10-19] | 12 [8-17] | 12 [8-16] |
| **wars declared** | **0** [0-0], mean 0.25 | 0, mean 0.16 | 0, mean 0.15 |
| **wars declared, per GAME** | **0.51** [0.44, 0.58] | 0.48 [0.34, 0.63] | 0.61 [0.49, 0.75] |
| aggressions played | 0 [0-1], mean 0.69 | 0, mean 0.54 | 0, mean 0.75 |
| aggressions, per GAME | 1.39 [1.28, 1.50] | 1.63 [1.35, 1.92] | 3.01 [2.68, 3.31] |
| colonies taken | 1 [0-2] | 1 [0-2] | 1 [0-2] |
| colony bids made | 3 [1-5] | 2 [1-3] | 3 [1-5] |
| wonders completed | 1 [1-2], mean 1.50 | 2 [1-2] | 2 [1-2], mean 1.72 |
| wonder stages built | 7 [5-9] | 7 [5-9] | 7 [5-9] |
| government changes | 1 [1-1] | 1 [1-1] | 1 [1-1] |
| **round of first government** | **12** [10-14] | 11 [9-14] | 12 [10-14] |
| civil cards taken | **34** [31-38] | 29 [27-32] | 30 [27-33] |
| takes at row tier 1 (1 CA) | 24 [21-27] | 20 [17-23] | 20 [17-23] |
| takes at row tier 2 (2 CA) | 8 [6-10] | 7 [5-9] | 8 [6-10] |
| **takes at row tier 3 (3 CA)** | **2** [1-3] | 2 [1-3] | 2 [1-3] |
| **% of takes paid at 3 CA** | **5.3%** | 6.3% | 7.7% |
| leaders elected | 4 [3-4] | 4 [3-4] | 4 [3-4] |

Shape of the distributions, which the medians hide:

* **War is rare and almost always a finishing move.** 67% of 2p games contain
  **zero** war declarations; 83% of individual 2p players never declare one all
  game. When a human does declare, they win 84-91% of the time (2p 295/351,
  3p 58/64, 4p 99/114). So "our champion never loses a war it declares" is
  *not* the off-distribution part — humans don't lose them either. Declaring
  one is what humans almost never do.
* **Aggressions are the normal military outlet, not wars**, and they scale with
  table size: 1.4 per 2p game, 3.0 per 4p game, i.e. ~0.7 per player-pair
  either way.
* **Governments come late and once.** Median first government is round 12 of
  19 — humans mostly skip the Age I governments entirely and go straight to
  Constitutional Monarchy (35% of players, the single most common first
  government) or Republic (22%). 6.7% never leave Despotism. Median number of
  government changes is exactly 1.
* **The 3-CA take is a deliberate, rare, and skill-invariant habit.** Median
  human pays 3 CA for 5% of their cards (2 cards a game); 21% of 2p players
  never pay 3 CA at all; the 90th percentile is 11%. Nobody grabs the fresh
  end of the row routinely.

### Skill split: mostly flat, so don't lean on it

`index.tsv` carries BGO's `level` (Emperor 1162 rows / Warlord 619 / King 416 /
Prince 329, and the ordering itself is inferred, see `docs/BGO_CORPUS.md`).
On the axes that matter the split is nearly flat:

| 2p | Emperor (331 g) | King (102 g) | Warlord (166 g) | Prince (93 g) |
|---|---|---|---|---|
| wars per GAME | 0.52 [0.43,0.60] | 0.56 [0.41,0.72] | 0.59 [0.42,0.75] | 0.27 [0.16,0.40] |
| % takes at 3 CA | 5.37 [5.05,5.67] | — | — | 5.42 [5.04,5.80] (P+W) |
| first gov, round | 11.5 [11.3,11.8] | — | — | 12.7 [12.4,13.0] (P+W) |
| final score | 149 [144,154] | — | — | 170 [164,178] (P+W) |

**The 3-CA rate is identical across skill.** So it is a stable convention of
human play at every level, not a marker of expertise — which makes our bot's
divergence from it more interesting, not less, but also means the corpus gives
no evidence that paying 3 CA *less* would make you stronger. Note also that
Emperor games have *lower* scores and *tighter* margins than Prince/Warlord
games: two strong players suppress each other's engines. Do not read "higher
score = better play" across skill tiers.

## Bot vs human

Champion: `experiments/league_state/champion_2p/3p/4p.json` at gen 224/·/· as
of 2026-07-27 10:08, run as `quiesce:...,levels=1` — the trap-5 setting from
`docs/UNATTENDED.md`; running it as a bare weight file would have measured a
1-ply bot that is not the champion. Mirror tables, every seat the same policy.
**n = 40 games at 2p, 30 at 3p, 30 at 4p.** These are small; every claim below
is stated with its CI and the ones that overlap are called overlapping.

| axis | human 2p | bot 2p (n=40) | verdict |
|---|---|---|---|
| final score | 159.5 [156.0,163.0] | **84.1** [73.7,95.2] | **bot scores ~half** |
| wonders completed / player | 1.50 [1.45,1.55] | **0.40** [0.30,0.50] | **3.7x fewer** |
| wonder stages built | 7.44 [7.28,7.60] | **1.91** [1.60,2.24] | **3.9x fewer** |
| civil cards taken | 34.3 [34.0,34.5] | **24.2** [23.0,25.3] | **10 fewer cards** |
| % of takes paid at 3 CA | 5.3% [5.0,5.5] | **22.4%** [20.3,24.5] | **4.3x more** |
| wars declared per GAME | 0.51 [0.44,0.58] | **1.48** [1.07,1.90] | **2.9x more** |
| aggressions per GAME | 1.39 [1.28,1.50] | 2.05 [1.68,2.48] | 1.5x more |
| round of first government | 11.8 [11.6,11.9] | **8.1** [7.3,8.9] | **~4 rounds early** |
| unspent science at end | 15.5 [15.0,15.9] | 6.3 [5.1,7.6] | lower |
| colony bids made | 3.22 [3.05,3.38] | 1.80 [1.50,2.14] | fewer |
| leaders elected | 3.69 [3.66,3.72] | 2.94 [2.75,3.12] | fewer |
| game length, rounds | 19.4 [19.4,19.5] | 18.9 [18.2,19.6] | overlap |
| winner's margin | 43.2 [40.3,46.3] | 40.5 [32.4,50.2] | **overlap** |
| colonies taken | 1.51 [1.44,1.57] | 1.51 [1.27,1.77] | **overlap** |

3p and 4p reproduce every one of those signs, and the war gap gets **worse**
with table size:

| wars declared per GAME | human | bot | ratio |
|---|---|---|---|
| 2p | 0.51 [0.44,0.58] | 1.48 [1.07,1.90] | 2.9x |
| 3p | 0.48 [0.34,0.63] | 2.60 [2.13,3.03] | **5.4x** |
| 4p | 0.61 [0.49,0.75] | 3.17 [2.43,3.87] | **5.2x** |

| aggressions per GAME | human | bot | ratio |
|---|---|---|---|
| 2p | 1.39 | 2.05 | 1.5x |
| 3p | 1.63 | 4.47 | 2.7x |
| 4p | 3.01 | 6.40 | 2.1x |

At 4p the bot also **stops colonizing** (0.68 colonies/player vs 1.39 human,
1.02 bids vs 3.36) — the opposite direction from its aggression, and the one
place the 4p bot diverges from the 2p bot's profile.

### The five axes our bot is off-distribution on

Ordered by how far outside the human distribution the bot sits, not by how
easy they'd be to fix:

1. **It doesn't build wonders.** 0.40 completed / 1.91 stages per player vs
   1.50 / 7.44. Humans put ~7 stages of resources into wonders a game; our bot
   puts under 2. This is the largest structural difference in the whole
   comparison and it is not a 40-game artifact — the CIs are nowhere near each
   other and the sign holds at 3p and 4p.
2. **It takes 10 fewer cards, and pays 3 CA for four times as many of the ones
   it does take.** 24.2 takes at 22.4% tier-3, vs 34.3 at 5.3%. Both halves of
   that point the same way: the bot has a **smaller civil-action budget** and
   spends it impatiently. This is exactly the axis the user named as the real
   skill of the game, and it is the cleanest single behavioural finding here.
3. **It revolutions ~4 rounds too early** (round 8.1 vs 11.8) and slightly
   more often. Combined with (2), a plausible single story: an early
   government purchase buys civil actions the bot then fails to convert into
   cards or wonders — but *this doc does not test that story*, it only
   measures the two facts separately.
4. **It goes to war 3-5x too often**, worsening with table size, and to
   aggression 1.5-2.7x too often. The user's prior ("1.98 wars/game") is the
   right shape; the measured number for this champion under `levels=1` is 1.48
   [1.07,1.90] per 2p game, against a human 0.51. The "never loses one" half of
   the prior is *not* anomalous — humans win 84% of theirs.
5. **It scores half what humans score** (84 vs 160). Since the *winner's
   margin* is statistically indistinguishable from the human one (40.5 vs
   43.2), our games are not less decisive — both civilizations are just much
   smaller. This is consistent with (1) and (2) and with `fullcheck_2p.jsonl`,
   where the champion beats the book bots by ~80 culture: **the whole
   ecosystem, champion included, plays a lower-economy game than humans do.**

## What this cannot tell you

* **n=40/30/30 on the bot side.** The per-GAME war and aggression means have
  CIs of roughly ±0.4 and ±0.7; the ratios above are safe only because the gaps
  are multiples, not points. Anything in the table marked *overlap* is a
  non-result, not a match — 40 games cannot distinguish a 10% difference.
* **The score comparison is not a clean skill measurement.** Absolute culture
  depends on engine scoring details as well as play, and nothing here
  independently verifies that our end-of-game scoring matches BGO's. The
  supporting evidence that the gap is real play and not a scoring bug is
  behavioural (7.4 vs 1.9 wonder stages, 34 vs 24 takes), not arithmetic. A
  direct check — score one reconstructed human final position through our
  scorer — has **not** been done and is the obvious next validation.
* **Human ≠ optimal.** BGO's population is Prince-to-Emperor club players, and
  the corpus shows Emperor games scoring *lower* than Prince games. "Our bot is
  off-distribution" is a statement about distribution, not about correctness.
  It is entirely possible that early revolutions and frequent war are correct
  and that humans under-fight. What the corpus establishes is that our bot's
  policy is not one any human plays, which for a policy trained purely against
  its own relatives is the thing worth knowing.
* **Some human state is unrecoverable**, as `docs/BGO_PILOT.md` §6 predicted:
  the card row is never printed (so "did they wait for it to slide?" can only
  be answered as "what did they pay", never as "what was on offer"), military
  hands are counts only, and **army strength appears nowhere except inside a
  war resolution**. `war_str_att_mean`/`war_str_def_mean` are the only military
  totals in the corpus and they are conditioned on a war having happened, which
  is 17% of players. There is no human baseline for final military strength.
* **The corpus is ~8 months of BGO play, not 16 years** (`docs/BGO_CORPUS.md`:
  journals only exist back to about page 45 of the index), and the skill filter
  was dropped mid-scrape, so the level mix is what supply gave, not a design.
* **Take-backs are a BGO UI affordance humans have and our bot does not.** They
  are removed from the human counts so the comparison is fair, but 6,786 of
  them means humans reconsidered ~8% of their takes with full information about
  what the take would cost. That asymmetry is not modelled anywhere.

## Proposed next steps (proposals only — nothing here was implemented)

In the order the evidence supports, not the order of effort:

1. **Verify the scoring gap before acting on it.** Reconstruct one finished
   human position (wonders, techs, government, happy, population) by hand from
   a journal, run it through `effects.end_of_game_bonus`, and check it lands on
   BGO's printed final score. If it doesn't, finding (5) is an engine bug and
   several of the others may be downstream of it. This is cheap and it gates
   everything else.
2. **Ask why wonders lose.** The evaluator has `best_*` building terms and a
   `var:wonder` opponent, and the champion beats `var:wonder` 90-94%. Either
   wonders really are weak in our engine (an engine/data question — check stage
   costs, the `+1 CA per completed wonder` take surcharge, and end-of-game
   wonder culture) or the evaluator cannot see a multi-turn wonder as an
   investment at `levels=1`. These are distinguishable: price a fixed
   wonder-first script against the champion in a scripted A/B.
3. **Make the 3-CA rate an observable, then a diagnostic.** `tier3_pct` is
   computed by `tools/bgo_stats.py` for both sides already. Log it per
   generation alongside win rate. If a generation's tier-3 rate moves toward
   5% without losing win rate, that is evidence the search got more patient; if
   win rate and tier-3 rate are locked together, that is evidence our bot's
   impatience is load-bearing given its (smaller) economy — which is itself the
   answer to a real open question.
4. **Do not "fix" the war rate directly.** It is downstream: a civilization
   with 24 cards and no wonders has little else to convert into culture, and
   the league's `var:military` opponent is held to 5.5% of turns
   (`docs/UNATTENDED.md` trap 3), so war has never been punished in training.
   Re-measure the war rate *after* (2), and only add a war prior if it survives.
5. **Use the corpus for what it was scraped for.** This analysis reads 1,011
   games as summary statistics; the journals also support per-turn culture and
   science trajectories with a known final outcome, which is the value-function
   training set `docs/BGO_CORPUS.md` set out to build and which nothing here
   touches.
