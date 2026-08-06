# Human play and outside sources: distilled

Source docs for this file (`HUMAN_BASELINE.md`, `HUMAN_BOTS.md`,
`BEHAVIOUR_CLONE.md`, `EXTERNAL_AIS.md`, `BGO_CORPUS.md`, `SOURCES.md`) were
deleted 2026-08-06 after this distillation; recover them from git history
for the full narrative, per-axis tables, and error bars. All bot/pipeline
code these docs describe was Python and is gone with `engine/` — no
`human` pool tier, no behaviour-cloning tooling, no Rust equivalent of
either. What survives is the data and the findings about it, which are
independent of any implementation.

## The BGO corpus: what it is and where it lives

1,011 finished 2015-edition ("A New Story of Civilization") games scraped
from Boardgaming-Online (692×2p, 133×3p, 186×4p), committed at
`sources/bgo/index.tsv` (metadata) and `sources/bgo/journals.tar.gz` (raw
per-game journals, ~7 MB). This is real, present data, not a promise — the
scrape's own writeup document ended with an unfilled "Results" placeholder,
but every other doc in this project cites and uses the completed 1,011-game
figure, and `index.tsv` confirms it (1,011 rows).

Reachability, if this ever needs re-running or extending: BGO's finished-games
index (`idJeu=10` for the 2015 edition, vs. `idJeu=4` for the 2006 original —
easy to grab the wrong one) has ~178k games total, but **journals only exist
back to roughly the last 8 months of play** (verified by direct sampling —
pages past ~45 of the pager return "No entries found" for real, completed
games). The corpus is drawn from that reachable window, not from BGO's full
16-year history. Login is a plain form POST, no CSRF, no Cloudflare;
`robots.txt` permits `index.php`. Board Game Arena has a much larger 2015
corpus (1.19M games) but is dead as a data source — explicit ToS prohibition
on automated extraction, no bot to spar against, everything login-gated. Its
published PHP source (`srussking/throughtheages` on GitHub) is still useful
as a free rules cross-check, unrelated to scraping.

**The one thing the journal never records: the civil card row.** You can see
which card a human took and what tier they paid, never what else was on
offer. This kills reconstructing "choice sets" for imitation learning (fixed
below by uniform-in-tier imputation, at a measured cost — see Behaviour
cloning) but does not affect value-learning (state → eventual outcome),
which only needs the position and the final score, both of which the
journal has. Military hand contents and discards are permanently
counts-only, never identities.

Three parsing traps that silently produced wrong numbers before being
caught (kept because the failure mode — "the number looked plausible while
wrong" — is the reusable lesson, not the specific bug): stage-completion
lines nested inside another card's effect line were undercounted 40% by a
regex anchored at line-start; leader elections were undercounted 39% by a
regex that truncated on a second-name collision; card names repeat across
ages and the journal prints only the base name, so age-keyed stats must use
the journal's own age column, never a name→age lookup table.

## Measured human baseline (2p unless noted; n = games, cluster-bootstrap CIs)

From `tools/bgo_stats.py` over the full parsed corpus:

| axis | 2p (n=692) | 3p (n=133) | 4p (n=186) |
|---|---|---|---|
| final score (culture) | **156** median, mean 159.5 [156.0,163.0] | 180 [140,211] | 182 [145,237] |
| wonders completed/player | 2.74 | 2.45 | 2.48 |
| wonder stages built | 8.77 | ~8 | ~8 |
| civil cards taken | 34.3 [34.0,34.5] | 29 | 30 |
| % of takes paid at 3 CA | 4.5% (median 3.3%) | 5.7% | 7.3% |
| wars declared, per GAME | 0.51 [0.44,0.58] | 0.48 | 0.61 |
| round of first government | 11.8 [11.6,11.9] | 11 | 12 |

Notable shapes, not just medians: **war is rare and almost always decisive**
— 67% of 2p games have zero declarations, 83% of individual players never
declare one, and when a human does declare they win 84–91% of the time.
**The 3-CA take is a deliberate, skill-invariant habit**, not a tell of
weak play — flat at 4.0–4.9% across every BGO skill tier (Prince through
Emperor), so a bot's divergence from it is not explained by "our reference
humans are only intermediate." **Governments come once, late** — median
round 12 of ~19, and 6.7% of players never leave Despotism.

**Skill tier is mostly a wash and score falls as skill rises**: Emperor
games score *lower* on average than Prince games (149 vs 175) because two
strong players suppress each other's engines — do not read "higher score =
better play" across tiers, and do not assume the top tier is the thing
worth imitating (checked directly in the archetype work below and rejected
for that reason).

Two structural facts about the corpus worth knowing before quoting any
per-axis number: 6,786 take-backs (human undoes a take with full cost
information) are excluded from all counts, since including them would
inflate takes ~8% and bias the tier histogram; and BGO applies a `+1 CA per
completed wonder` surcharge to every take, so raw logged costs of 4–7 CA are
wonder-inflated tier-1/2/3 takes, not evidence of a "pay 7 CA" tier — the
parser subtracts completed-wonder count before comparing tiers.

## Play-rate census (Rust, regenerable) -- what the corpus was still missing

The Python tooling above did rich per-axis stats but the corpus was never
read from anywhere in `rust/` -- the bot's only strength signal is beating
its own frozen ancestors (`docs/HAZARDS.md`'s "no external anchor" hazard).
This section does not close that hazard (it does not play the corpus,
score it, or compare a bot's move rate to a human's) -- it is the narrower
thing the Rust side had zero of: a **play-rate census**, text extraction
and counting only, no game-state reconstruction. Source:
`rust/src/bin/corpuscensus.rs` (`#[cfg(test)] mod tests`, 35 cases, real
journal lines as fixtures, `cargo test --profile difftest corpuscensus`).
No dependencies (`rust/Cargo.toml`'s `[dependencies]` stays empty) --
card-name matching is a hand-rolled longest-known-prefix scan against
`tta::CARDS`, described in the binary's own module doc.

Regenerate:

```
tar -xzf sources/bgo/journals.tar.gz -C /tmp/bgo-journals
cargo run --profile difftest --bin corpuscensus -- \
    sources/bgo/index.tsv /tmp/bgo-journals/journals
```

**Coverage: 451,403 / 451,453 journal lines classified (99.99%)**, 0 of the
1,011 games excluded (no expansion cards found once BGO's own spelling
quirks are aliased to the engine's -- nine cards, all documented in the
binary's `ALIASES` table, e.g. "Leonardo Da Vinci" vs. engine "Leonardo da
Vinci", "Stockpile" vs. engine "Stock Pile", "Loss of Sovereignity"
(BGO typo) vs. "Loss of Sovereignty"). The 50 unclassified lines left are
one-offs below 4 occurrences each: failed colonization attempts
("Insufficient task force ... Colonists were unable to settle"), pact
cancellations, and free-text game renames -- named, not hidden, per the
binary's own coverage report.

**Sanity check against the Python-era numbers above**: wars declared/game
came out 0.507 (2p) / 0.481 (3p) / 0.613 (4p) here, against 0.51 / 0.48 /
0.61 from `tools/bgo_stats.py` -- independent implementations, same
corpus, agreement to the second decimal. One scope difference to flag:
this census's "take card from row" (87.5/game overall) counts BOTH civil-
and military-row takes undifferentiated (BGO logs both through the same
`"X takes Y in hand"` shape, split only by whether the trailing clause says
`"uses N civil action"` or `"uses N military action"` -- not parsed apart
here), where the Python tool's 34.3/game was civil-row only; the two are
not the same measurement. This census's own "put card back" tally (6.86/
game, 7.8% of takes) reproduces the Python tool's ~8% take-back estimate
independently, for what that is worth as a second cross-check.

### Action rates, per game and per player-turn, by player count

| action class | 2p /game | 2p /turn | 3p /game | 3p /turn | 4p /game | 4p /turn |
|---|---|---|---|---|---|---|
| take card from row | 74.51 | 1.971 | 95.41 | 1.775 | 130.01 | 1.764 |
| build building | 12.97 | 0.343 | 21.59 | 0.402 | 28.93 | 0.393 |
| build unit | 12.21 | 0.323 | 15.14 | 0.282 | 24.57 | 0.333 |
| build wonder stage | 12.37 | 0.327 | 17.67 | 0.329 | 24.66 | 0.335 |
| increase population | 17.78 | 0.470 | 26.25 | 0.488 | 38.54 | 0.523 |
| upgrade unit | 3.06 | 0.081 | 3.90 | 0.073 | 7.23 | 0.098 |
| upgrade production (farm/mine) | 15.46 | 0.409 | 21.26 | 0.396 | 30.30 | 0.411 |
| develop technology | 23.67 | 0.626 | 31.08 | 0.578 | 44.24 | 0.600 |
| elect leader | 7.37 | 0.195 | 10.82 | 0.201 | 14.25 | 0.193 |
| change government | 0.51 | 0.014 | 0.91 | 0.017 | 1.41 | 0.019 |
| play tactic | 4.24 | 0.112 | 5.86 | 0.109 | 9.29 | 0.126 |
| declare war | 0.51 | 0.013 | 0.48 | 0.009 | 0.61 | 0.008 |
| win war | 0.50 | 0.013 | 0.47 | 0.009 | 0.58 | 0.008 |
| play aggression | 1.39 | 0.037 | 1.63 | 0.030 | 3.01 | 0.041 |
| propose pact | 0.00 | 0.000 | 0.94 | 0.018 | 2.84 | 0.039 |
| accept pact | 0.00 | 0.000 | 0.87 | 0.016 | 2.64 | 0.036 |
| colonize | 3.01 | 0.080 | 3.44 | 0.064 | 5.58 | 0.076 |
| discard | 18.26 | 0.483 | 28.55 | 0.531 | 35.92 | 0.488 |
| bid | 6.43 | 0.170 | 7.15 | 0.133 | 13.45 | 0.183 |
| win territory auction | 3.01 | 0.080 | 3.44 | 0.064 | 5.58 | 0.076 |
| destroy | 2.15 | 0.057 | 3.11 | 0.058 | 4.34 | 0.059 |
| disband | 0.99 | 0.026 | 1.57 | 0.029 | 2.13 | 0.029 |
| pass | 19.16 | 0.507 | 31.10 | 0.579 | 46.56 | 0.632 |
| play event | 14.76 | 0.390 | 19.29 | 0.359 | 27.47 | 0.373 |
| play action card | 13.42 | 0.355 | 15.40 | 0.286 | 19.67 | 0.267 |
| put card back (take-back upper bound) | 6.20 | 0.164 | 6.80 | 0.126 | 9.39 | 0.127 |
| **player-turns (denominator)** | 37.81 | -- | 53.76 | -- | 73.69 | -- |

`win territory auction` and `colonize` are identical **by construction, not
coincidence** -- see "most surprising number" below.

### Action rates per game, by BGO level tier

| action class | Prince | King | Warlord | Emperor |
|---|---|---|---|---|
| take card from row | 88.68 | 88.89 | 84.73 | 88.12 |
| build building | 18.12 | 17.28 | 18.01 | 16.13 |
| build wonder stage | 15.76 | 15.56 | 15.69 | 14.94 |
| develop technology | 28.89 | 28.95 | 27.42 | 28.66 |
| elect leader | 9.15 | 9.17 | 8.90 | 9.14 |
| change government | 0.69 | 0.71 | 0.70 | 0.76 |
| play tactic | 5.14 | 5.51 | 4.71 | 5.77 |
| declare war | 0.33 | 0.52 | 0.50 | 0.59 |
| play aggression | 1.76 | 1.74 | 1.44 | 1.85 |
| propose pact | 0.70 | 0.59 | 0.60 | 0.68 |
| colonize | 3.99 | 3.47 | 3.77 | 3.32 |
| discard | 22.36 | 23.05 | 23.37 | 22.67 |
| player-turns (denominator) | 47.02 | 46.88 | 45.87 | 46.58 |

Skill tier moves almost nothing here -- consistent with the Python-era
finding above ("skill tier is mostly a wash"): pact rate is flat
(0.59-0.70 across all four tiers), war rate rises gently with tier
(0.33 Prince -> 0.59 Emperor) but stays a minority behaviour throughout.
Full table (all 26 action classes x 4 tiers x both normalisations) is in
the binary's own stdout, not reproduced here for space.

### Military summary, per game, by player count

| | 2p (n=692) | 3p (n=133) | 4p (n=186) |
|---|---|---|---|
| games with >=1 war declared | 32.8% | 30.8% | 36.6% |
| games with >=1 aggression played | 63.7% | 66.9% | 83.3% |
| games with >=1 pact proposed | 0.0% | 54.9% | 95.7% |
| wars declared /game | 0.507 | 0.481 | 0.613 |
| aggressions played /game | 1.389 | 1.632 | 3.005 |
| tactics played /game | 4.240 | 5.857 | 9.290 |
| pacts proposed /game | 0.000 | 0.940 | 2.839 |
| pacts accepted /game | 0.000 | 0.865 | 2.640 |

Pacts are structurally impossible at 2p (need a third party) and near-
universal at 4p (95.7% of games) -- diplomacy scales with player count far
more sharply than war does (36.6% of 4p games) or even aggression (83.3%).

### Game length and scoring, by player count

| | 2p | 3p | 4p |
|---|---|---|---|
| mean rounds | 19.43 | 18.59 | 19.17 |
| reached Age IV | 100.0% (692/692) | 100.0% (133/133) | 100.0% (186/186) |
| mean final score | 159.5 | 176.5 | 195.1 |
| max final score seen | 409 | 372 | 428 |

100% Age IV is a property of the corpus filter (`sources/bgo/index.tsv`
only keeps finished games), not a claim about play -- do not read it as
"humans always reach Age IV."

### Cards taken from the row / played, top 15 (of 1,011 games, all counts combined)

| rank | taken | count | | rank | played/built/discovered | count |
|---|---|---|---|---|---|---|
| 1 | Reserves | 5,109 | | 1 | Knights | 4,787 |
| 2 | Urban Growth | 4,565 | | 2 | Warriors | 4,626 |
| 3 | Engineering Genius | 3,400 | | 3 | Reserves | 4,158 |
| 4 | Breakthrough | 3,216 | | 4 | Cannon | 3,542 |
| 5 | Rich Land | 2,943 | | 5 | Religion | 3,096 |
| 6 | Efficient Upgrade | 2,860 | | 6 | Bronze | 2,945 |
| 7 | Revolutionary Idea | 2,634 | | 7 | Engineering Genius | 2,809 |
| 8 | Frugality | 2,255 | | 8 | Swordsmen | 2,647 |
| 9 | Iron | 1,946 | | 9 | Philosophy | 2,226 |
| 10 | Patriotism | 1,822 | | 10 | Revolutionary Idea | 2,123 |
| 11 | Irrigation | 1,744 | | 11 | Iron | 2,042 |
| 12 | Cannon | 1,632 | | 12 | Pyramids | 1,994 |
| 13 | Alchemy | 1,590 | | 13 | Frugality | 1,969 |
| 14 | Knights | 1,581 | | 14 | Alchemy | 1,944 |
| 15 | Air Forces | 1,551 | | 15 | Air Forces | 1,782 |

Long tail: 77 more distinct names taken (43.9% of all 88,432 takes), 119
more distinct names played (49.1% of all 113,351 plays) -- card choice has
a heavy tail even after the top 15, nobody is playing a fixed 15-card
subset.

### Most surprising number

`win territory auction` (the `"<Color> wins <Territory> Winning bid is
<N>"` line) and `colonize` (`"<Color> colonizes a <Territory> ..."`) are
**exactly equal, per territory type, in every one of the 1,011 games**:
664 Developed / 576 Historic / 577 Inhabited / 577 Strategic / 584 Vast /
602 Wealthy, both sides, 3,580 = 3,580 total. Winning a territory's bid
auction and colonizing it are not two independent decisions with some
success rate in between -- in this corpus every won auction produced a
colonize attempt BGO logged as successful. (A handful of failed attempts
exist in the raw text -- "Insufficient task force ... Colonists were
unable to settle", 12 lines total across all 1,011 games -- rare enough
to not move the per-territory-type counts at this resolution.) This was
caught by suspicion, not intent: the two numbers came out identical to
three decimal places across every player-count and tier split, which is
what a bug looks like, so it was checked against the raw `grep` counts
before being written down as a real finding rather than a copy-paste
error.

## Bot vs. human play-rate census: the first external anchor (2026-08-06)

`docs/HAZARDS.md`'s "no external anchor" hazard says the bot's only strength
signal is beating its own frozen ancestors -- it never plays against, or gets
measured against, anything outside this repo. This section still does not
close that hazard (the bot does not play the human corpus, and nothing here
scores one against the other), but it is the first time the two have been
put side by side at all: `rust/src/bin/botcensus.rs` instruments BOT
self-play to count the exact same 26 [`ActionClass`]es `corpuscensus.rs`
counts over the human corpus, with the same two normalisations (per game,
per player-turn), so the columns below are a genuine apples-to-apples
comparison, not two differently-defined numbers that happen to have similar
names.

### Method and caveats

- **Structural counting, not text.** `botcensus.rs` reads real engine state
  (the `Move` chosen, the `Pending` decision stack, `PlayerState` fields) --
  it never goes through a journal. Most classes fall straight out of the
  `Move` the bot picked; four (`PlayEvent`, `WinAuction`, `Colonize`,
  `WinWar`) don't correspond to a single `Move` at all and are detected from
  state transitions instead -- see `botcensus.rs`'s own module doc for
  exactly how, and for a bug that method caught in itself (below).
- **Definitions were matched to the human side's, not re-derived.** Where
  `corpuscensus.rs` had to make a judgement call (Barbarossa-as-BuildUnit's
  human-side analogue doesn't exist so there is nothing to match; Bach's
  upgrade is bookkeeping on the human side because BGO's text glues the
  leader's name to the verb with no space, so `botcensus.rs` excludes
  `Move::BachTheater` too, for consistency rather than completeness), the
  bot side copies the same call. Full list of exclusions and the reasoning
  for each is in `botcensus.rs`'s module doc.
- **Self-play mirror match, production weights.** Every seat plays
  `BotKind::Weighted` (the kind `climb.rs`'s arena actually plays) loaded
  from `experiments/rust_champion_{2,3,4}p.json` -- the current champion
  vectors the league is climbing, gitignored, not committed here. This is
  **three separately-trained vectors**, one per player count, not one
  policy that generalises across table sizes -- a pattern that holds at one
  player count and inverts at another (several below) is as likely to be
  "this vector found a different local optimum" as "player count changes
  what's correct." Every bot seat also plays against an IDENTICAL copy of
  itself, where the human's opponents are a real, mixed BGO population (any
  skill, human variance, actual negotiation) -- a difference could be the
  bot's behaviour, the mirror-match setting, or the corpus's population;
  this section flags divergences, it does not adjudicate which side caused
  each one.
- **Sample sizes**: bot n=300 (2p) / 300 (3p) / 150 (4p) games; human n=692
  (2p) / 133 (3p) / 186 (4p) games (same corpus as above).
- **Comparing against "strong" humans**: the census's own tier table above
  (BGO's own Prince/King/Warlord/Emperor skill ladder) already found skill
  moves almost nothing in these rates (war 0.33->0.59, tactic 5.14->5.77,
  pact flat) -- but that table isn't crossed with player count, so there is
  no "strong 2p human" row to read off directly. Given the tier table's own
  finding that skill barely matters here, the all-tier per-player-count
  numbers used below are a reasonable stand-in for "strong human," flagged
  as an approximation rather than silently assumed.
- **`put card back`** has no bot-side number at all (not 0.000) -- there is
  no "undo a take" `Move` in this engine, because it is not a rules action,
  only a BGO client misclick correction. The bot cannot misclick.
- **A bug this method caught in itself**: the first version of `Colonize`
  detection diffed `PlayerState::colonies` length, which also grows when a
  war/aggression's `Annex` spoil steals an EXISTING colony from a rival --
  not a fresh colonization. That inflated 2p's bot colonize rate 6x
  (`colonize` >> `win territory auction`, which the human corpus's own
  "most surprising number" section established should be near-equal).
  Caught by the same suspicious-equality check that section used, fixed by
  crediting `Colonize` in lockstep with `WinAuction` instead (a won auction
  always completes into exactly one colonize -- `interact::colonize`'s own
  `assert!` guarantees it) -- see `botcensus.rs`'s module doc for the full
  account. Left in this doc as a demonstration that the "most surprising
  number" cross-check is worth re-running on any new instrumentation, not
  just trusted once.

### 2-player: bot vs. human (n=300 bot games, n=692 human games)

| action class | bot /game | human /game | ratio (bot/human) |
|---|---|---|---|
| take card from row | 47.667 | 74.51 | 0.64x |
| build building | 20.680 | 12.97 | 1.59x |
| build unit | 5.893 | 12.21 | 0.48x |
| **build wonder stage** | 0.183 | 12.37 | **0.01x** |
| increase population | 13.000 | 17.78 | 0.73x |
| **upgrade unit** | 0.000 | 3.06 | **0.00x** |
| upgrade production (farm/mine) | 8.970 | 15.46 | 0.58x |
| develop technology | 9.067 | 23.67 | 0.38x |
| elect leader | 6.720 | 7.37 | 0.91x |
| **change government** | 2.157 | 0.51 | **4.23x** |
| **play tactic** | 25.390 | 4.24 | **5.99x** |
| **declare war** | 0.000 | 0.51 | **0.00x** |
| **win war** | 0.000 | 0.50 | **0.00x** |
| **play aggression** | 0.033 | 1.39 | **0.02x** |
| propose pact | 0.000 | 0.00 | -- (both structurally 0 at 2p) |
| accept pact | 0.000 | 0.00 | -- (both structurally 0 at 2p) |
| **colonize** | 0.107 | 3.01 | **0.04x** |
| discard | 24.957 | 18.26 | 1.37x |
| **bid** | 0.640 | 6.43 | **0.10x** |
| **win territory auction** | 0.097 | 3.01 | **0.03x** |
| destroy | 5.290 | 2.15 | 2.46x |
| **disband** | 6.433 | 0.99 | **6.50x** |
| pass | 24.307 | 19.16 | 1.27x |
| play event | 11.353 | 14.76 | 0.77x |
| play action card | 16.760 | 13.42 | 1.25x |
| put card back | N/A (no such `Move`) | 6.20 | -- |
| player-turns (denominator) | 38.82 | 37.81 | 1.03x |

### 3-player: bot vs. human (n=300 bot games, n=133 human games)

| action class | bot /game | human /game | ratio (bot/human) |
|---|---|---|---|
| take card from row | 55.840 | 95.41 | 0.59x |
| build building | 8.220 | 21.59 | 0.38x |
| build unit | 13.457 | 15.14 | 0.89x |
| build wonder stage | 6.057 | 17.67 | 0.34x |
| increase population | 18.213 | 26.25 | 0.69x |
| **upgrade unit** | 13.083 | 3.90 | **3.36x** |
| **upgrade production (farm/mine)** | 1.153 | 21.26 | **0.05x** |
| develop technology | 8.970 | 31.08 | 0.29x |
| elect leader | 8.270 | 10.82 | 0.76x |
| change government | 1.213 | 0.91 | 1.33x |
| **play tactic** | 19.240 | 5.86 | **3.28x** |
| **declare war** | 1.763 | 0.48 | **3.67x** |
| **win war** | 1.623 | 0.47 | **3.45x** |
| **play aggression** | 4.877 | 1.63 | **2.99x** |
| **propose pact** | 7.813 | 0.94 | **8.31x** |
| **accept pact** | 3.910 | 0.87 | **4.49x** |
| colonize | 1.100 | 3.44 | 0.32x |
| discard | 30.063 | 28.55 | 1.05x |
| bid | 8.827 | 7.15 | 1.24x |
| win territory auction | 1.000 | 3.44 | 0.29x |
| destroy | 2.153 | 3.11 | 0.69x |
| disband | 0.523 | 1.57 | 0.33x |
| pass | 31.577 | 31.10 | 1.02x |
| play event | 15.743 | 19.29 | 0.82x |
| play action card | 19.197 | 15.40 | 1.25x |
| put card back | N/A (no such `Move`) | 6.80 | -- |
| player-turns (denominator) | 59.72 | 53.76 | 1.11x |

### 4-player: bot vs. human (n=150 bot games, n=186 human games)

| action class | bot /game | human /game | ratio (bot/human) |
|---|---|---|---|
| take card from row | 86.133 | 130.01 | 0.66x |
| build building | 25.773 | 28.93 | 0.89x |
| build unit | 21.960 | 24.57 | 0.89x |
| build wonder stage | 27.973 | 24.66 | 1.13x |
| increase population | 33.293 | 38.54 | 0.86x |
| **upgrade unit** | 0.773 | 7.23 | **0.11x** |
| upgrade production (farm/mine) | 23.567 | 30.30 | 0.78x |
| develop technology | 33.667 | 44.24 | 0.76x |
| elect leader | 12.987 | 14.25 | 0.91x |
| **change government** | 0.013 | 1.41 | **0.01x** |
| **play tactic** | 41.180 | 9.29 | **4.43x** |
| **declare war** | 4.193 | 0.61 | **6.87x** |
| **win war** | 3.960 | 0.58 | **6.83x** |
| **play aggression** | 10.393 | 3.01 | **3.45x** |
| **propose pact** | 19.600 | 2.84 | **6.90x** |
| accept pact | 4.900 | 2.64 | 1.86x |
| colonize | 5.013 | 5.58 | 0.90x |
| discard | 63.773 | 35.92 | 1.78x |
| bid | 8.813 | 13.45 | 0.66x |
| win territory auction | 4.973 | 5.58 | 0.89x |
| destroy | 1.873 | 4.34 | 0.43x |
| disband | 1.207 | 2.13 | 0.57x |
| pass | 44.880 | 46.56 | 0.96x |
| play event | 23.427 | 27.47 | 0.85x |
| play action card | 12.393 | 19.67 | 0.63x |
| put card back | N/A (no such `Move`) | 9.39 | -- |
| player-turns (denominator) | 98.27 | 73.69 | 1.33x |

### Where the bot is far off -- the candidate blind spots

Bolded rows above are >=3x or <=0.15x; grouped here by theme since several
are the same underlying pattern showing up across the tables, not 78
independent facts.

**1. The 2p champion barely fights or expands -- likely a real gap.** At 2p
the bot declares war in **0 of 300 games**, plays aggression at 2% of the
human rate, bids in auctions at 10%, wins territories at 3%, and builds a
wonder stage at 1.5% of the human rate. Humans at 2p aren't passive either
(0.51 wars/game, 3.01 colonizations/game) -- this reads as the 2p vector
having converged on an unusually insular, build-only equilibrium (its
`build building` rate is 1.6x human, consistent with redirecting effort
inward) rather than a principled strategy, since it skips essentially every
other avenue of expansion at once rather than substituting one for another.
Worth investigating as a genuine 2p champion weakness, not written off as
"humans play more aggressively than necessary."

**2. 3p/4p do the opposite: 3-7x human war/aggression rates.** `declare war`
is 3.67x human at 3p and 6.87x at 4p; `win war` tracks it almost exactly
(the near-1:1 declare/win ratio the human corpus itself noted holds for the
bot too); `play aggression` is ~3x human at both. Paired with finding 1,
this is not "the bot is more/less aggressive than humans" as a single fact
-- it is three separately-trained vectors that landed on three different
military postures, which argues these are per-vector local optima the
training process found, not a stable "bot playstyle." The 3p/4p rates could
be a genuine edge self-play discovered against a population that behaves
identically to itself (no bluffing, no real betrayal cost) and might not
transfer as cleanly against the more cautious mixed-skill humans in the
corpus -- plausible but unverified; flagged, not claimed.

**3. Pact proposals succeed far less often than humans' do.** Computing
propose:accept as a hit rate: humans land **~93%** of proposed pacts at both
3p (0.87/0.94) and 4p (2.64/2.84) -- consistent with people only proposing
deals they're fairly sure will be taken. The bot's hit rate is **~50%** at 3p
(3.91/7.81) and **~25%** at 4p (4.90/19.60) -- it proposes 6-8x the human
rate but a large majority go nowhere at 4p. This is the most interpretable
finding in the whole census: `OfferPact` is a 1-ply linear-evaluator move
(no search, no opponent modelling), so it can look attractive in isolation
without the evaluator checking whether the identical-policy opponent would
actually accept it -- a plausible, checkable hypothesis for a real
inefficiency (spending actions on proposals that don't land), not
necessarily "the bot is more diplomatic."

**4. `play tactic` is the one divergence that's consistent across ALL THREE
player counts** (5.99x / 3.28x / 4.43x) -- every other large divergence
flips sign or vanishes somewhere. Because it's stable across three
independently-trained vectors, this reads as a genuine, robust bot
preference rather than training noise: tactics are a strong, cheap,
low-downside defensive investment, and the bot may simply be correct to lean
on them harder than a human corpus that skews toward casual play. Tentative
read: **defensible**, not a blind spot -- but the most testable one, since
"does higher tactic rate correlate with the champion's actual win rate"
is answerable from existing league data without a new experiment.

**5. `upgrade unit` and `upgrade production` swing wildly and inconsistently
by player count** (upgrade unit: 0.00x / 3.36x / 0.11x; upgrade production:
0.58x / 0.05x / 0.78x) with no direction that holds across player counts.
Read as per-vector idiosyncrasy (see the method caveat above) rather than a
real finding -- training likely never rewarded upgrade RATE directly, only
final score, so different vectors found different, roughly-equally-good
paths to spending resources.

**6. `change government` also swings hard (4.23x at 2p, 0.01x at 4p)** for
the same reason as (5): no consistent direction across player counts, so
treated as per-vector idiosyncrasy rather than a signal.

**7. `take card from row` is consistently ~35-40% below human at every
player count** (0.64x / 0.59x / 0.66x) -- the one broadly-below-human rate
that IS stable across all three vectors, alongside (4)'s stable
above-human one. Player-turn counts are close to human's (`player-turns`
ratio 1.03x-1.33x, i.e. bot games are the same length or a bit longer), so
this is not "shorter games, naturally fewer takes" -- the bot is spending a
consistently larger share of its actions on other things. Plausible and
not obviously wrong (taking cards you don't act on is exactly BGO humans'
~8% self-corrected "put back" behaviour, which the bot cannot exhibit
at all, and hand-limit pressure differs from a human's), but consistent
enough across three vectors to be worth a closer look rather than dismissing
as noise.

**Not flagged as concerning**: `discard` running higher at 4p (1.78x)
tracks directly from finding 2 (far more war/aggression means far more
military cards drawn and discarded) -- an expected consequence, not an
independent gap. `destroy`/`disband` move in different directions at
different counts with no consistent story, same treatment as (5)/(6).

## Human play does not cluster into archetypes (a corpus fact)

k-means over twelve behavioural axes (wonders, stages, takes, tier-3 rate,
wars, aggressions, bids, science, first-gov round, leaders, age-I/III
takes), tested against a **permutation null** (same data, columns
independently shuffled — destroys co-occurrence, keeps every marginal).
Real archetypes would beat that null clearly; they don't: silhouette ratio
over the null is 1.0–1.9x across k=2..6 and player counts, split-half ARI
(cluster two random halves, compare) is 0.24–0.69 and doesn't improve with
k. **Human play in this corpus is one blob with directions in it, not
discrete styles** — except war, which is genuinely bimodal (83% zero, the
rest 1–2). The same three continuous directions recover at every k:
economy size, cards-vs-wonders, and a militarist minority. This is a fact
about the population, independent of any bot or fitting method built on top
of it, which is why it's the one finding from `HUMAN_BOTS.md` still worth
keeping now that the Python archetype bots it also described are gone.

## Behaviour cloning: human-likeness and playing strength are anti-correlated — but it's search-dependent

Fitting a linear evaluator's weights to predict 152k reconstructed human
decisions by conditional logit (`weighted.evaluate`, 64 features)
reproduces human move choice far better than any hand-tuned vector (36.1%
top-1 held-out agreement vs. 17–19% for the trained champion / default
weights) — but at 1-ply the resulting vector is very weak (7.4 final
culture vs. a human 159.5, vs. the champion's 110.5). Sweeping a
regularisation-toward-default penalty traces a **monotone trade-off**:
every step toward human-like move choice costs playing strength, over a
36-to-27-point move-agreement range and a 105-to-7-point culture range.
Mechanism, and this is the transferable part: **move choice cannot identify
a weight on a feature that doesn't vary between the candidates of a single
decision.** Culture stock is such a feature (it changes only when a wonder
completes, never as a direct consequence of "which move now"), so a
move-choice fit drives its weight toward zero regardless of how important
culture actually is — the clone learns *how* humans play, not *what the
game is for*. Grafting only the six culture-related weights from the
default vector back onto an otherwise-cloned vector recovered ~88 culture
points by itself.

**But the ordering flips under a deeper search.** Under `plan:width=8` (the
policy this project would actually ship), the same heavily-regularised
clone scores 108.3 [98.6,118.2] against the champion's 69.0 [59.2,77.8] —
a reversal of the 1-ply result — and lands within the human corpus's own
CI on five behavioural axes simultaneously (wonders started, government
changes, round of first government, civil cards taken, age-II/III takes).
This is the same interaction `docs/EVALUATOR_HISTORY.md`'s transfer-test
entry documents elsewhere in this repo (a conclusion drawn under one search
need not hold under another): do
not quote a strength comparison between two weight vectors without naming
the search it was measured under, and remeasure under the ship policy
before trusting a 1-ply verdict either way.

The clone still misses badly on takes specifically (35% of human decisions,
clone at chance) because the evaluator has no feature that distinguishes
one row card from another beyond a linearised `hand_potential` term — a
feature gap, not a fitting gap, and the single highest-value fix named for
any future human-likeness work.

## What other AIs do (survey conclusion, still true)

No drop-in strong external TTA bot exists to spar against or clone from.
Checked and ruled out: the official CGE app's Hard AI (community consensus:
a hand-tuned weighting heuristic, not search-based — architecturally the
same class as this project's own linear bot, reachable only by a human
playing it by hand, no API/export/log access of any kind); Board Game Arena
(no bot at all, ToS forbids scraping); published research (**none exists on
Through the Ages** — checked arXiv, Semantic Scholar, Czech university
thesis repositories, the community boardgame-AI-research index; this
project would be first). The closest useful adjacent work is the Tabletop
Games framework (QMUL) literature on structurally similar heavy euros
(Terraforming Mars, Puerto Rico, Race for the Galaxy, 7 Wonders, Dominion):
its most actionable transferable finding is Keldon Jones' Race for the
Galaxy AI — a small TD-learned net over hand-designed features, ~30k
self-play games, reached near-world-class play and is the template this
project's own value-net line already follows. The literature's other
recurring lesson: action-space size (TTA's huge legal-move sets), not
hidden information (TTA's is thin — the row is public), is the central
algorithmic problem for this genre, and determinized/PIMC search is
typically adequate rather than needing full ISMCTS.

## Outside sources for card data, and what they corrected

Card stats (costs, effects, per-player-count copy counts) were cross-checked
against multiple independent sources: the official rulebook/FAQ, the Board
Game Arena Studio implementation source (`material.inc.php`, authoritative
for copy counts), a Tabletop Simulator workshop mod (independent
confirmation), and — added later as a fourth, mostly-corroborating opinion —
two files from a BoardGameGeek file section (a card-reference PDF and a
`.xls`). Where sources disagreed, both values were written down and the
data was changed only when three-plus independent sources agreed against
this project's own numbers; a disagreement with only one weak source
(`BGG 409053`, no provenance statement, wrong totals on multiple checks) was
correctly left unapplied.

**One real bug this process found and fixed**: 13 military card counts in
`data/cards_military_actions.json` had Age I/III tactics and aggressions
swapped in the wrong proportion (Age I tactics at half their correct count:
5 instead of 10 of 45 Age I military cards), unanimous 3–0 or 4–0 against
this project across every independent source once actually checked. This
was live-data-affecting: the hill climb had been training against a deck
with the wrong military-card mix, and the fix invalidated cross-comparison
of any generation on either side of the commit that applied it. Confirmed
fixed and current in `data/cards_military_actions.json` (`Fighting Band`
etc. now read count 2 at 2p/3p/4p, matching the correction). If a future
card-data change is proposed, `docs/SOURCES.md`'s replacement — this
section — is the reminder to check three-source agreement before touching
`data/`, not just the loudest single source.

## Diagnosing the 2p champion's passivity: anchor saturation, not a measurement bug (2026-08-06)

The census above found the 2p champion declaring **0 wars in 300 games**,
bidding in territory auctions at 10% of the human rate, and building wonder
stages at ~1.5–2% of it — yet the same champion reports ~88–95% against its
own anchor in `experiments/rust_champion_2p.json`. This section works out
why those two facts coexist, testing (not arguing from the armchair) the
four candidate explanations `docs/HAZARDS.md`'s "no external anchor" hazard
implies: a measurement bug, a too-weak anchor, an objective that rewards
this, and 2p-specific wonder mispricing. Weights compared here come from
`/Users/pt/tta-ai/experiments/` (gitignored, not committed) at whatever
generation the live league had reached when each measurement was taken —
the champion is a moving target under continuous training throughout this
investigation, which turns out to matter (see the last section below).

### 1. Ruled out: the war/wonder numbers are not a `botcensus` artifact

`botcensus.rs` already caught one bug in itself (miscounting colonization
by diffing a field that also grows on a war's `Annex` spoil — see above), so
its other numbers needed independent confirmation before being trusted
further. Two checks, neither reusing `classify_move`/`BotClass`:

- **Wars, by state-edge detection.** A standalone loop (not committed —
  scratch code, deleted after use) played the same weights through
  `game::step` directly and counted every transition of
  `PlayerState::war_declared_by_me` from `CardId::NONE` to set, across
  `state.rs`'s own field rather than `Move::War` pattern-matching. Result
  over 300 games, seed 1: **0 wars, exact agreement** with `botcensus`'s
  count via a completely different code path.
- **Wonders, by final-state accounting.** The same loop summed
  `PlayerState::completed_wonders.len()` (an append-only field, never
  reused for anything else) plus the in-progress `wonder_steps` at game
  end — no move classification at all. Result: **0.053 wonders
  completed/player/game**, about 2% of the human corpus's 2.74/player
  baseline from the measured-human-baseline table above — the same
  order-of-magnitude gap `botcensus` reported, via numbers that share no
  code with it.

Both independent measurements land in the same place `botcensus` did. The
passivity is real; **explanation 1 (bad measurement) is ruled out.**

One wrinkle this check surfaced: re-running `botcensus` itself today against
the *current* checkpoint (`gen` ~1262, same command as the doc's original
table) gives `build wonder stage` = **0.527/game**, not the 0.183/game
tabulated above — a ~2.9x increase within the same day. The league kept
training after this morning's crash-loop fix (`95728dc`, seed-overflow
panics that had it repeatedly resuming from a gen-1164 checkpoint) landed,
and `experiments/logs/rust_climb_2p.jsonl` shows real, continued generation
acceptance since (gen 1164's own logged standing was 88.3% vs. anchor;
by gen ~1298 it's over 95%). So this doc's original table was already
measuring a stale, mid-recovery checkpoint by the time it was read — worth
knowing before treating any single census run as a fixed fact about "the"
2p champion, since it is not a fixed thing. War rate did not move (still
exactly 0 in today's rerun); wonder rate did. That asymmetry is itself a
clue, developed below.

### 2. Directly demonstrated: the anchor's signal saturates and cannot tell these two vectors apart

`climb.rs`'s own module doc is explicit that the anchor exists only to
catch a **regression** (every champion the old Python league produced was
secretly worse than the untuned default vector — see that file's point 2),
not to serve as an absolute strength ladder. Whether it can still do the
narrower job is testable: does a large margin over the anchor mean anything
about strength *relative to another real, differently-trained vector*?

Measured with `arena`/`kindmatch` (240 games = 120 deals × 2 seats,
`--threads 3`, `rust_champion_2p.json` at its current live checkpoint):

| opponent | 2p champion's win rate |
|---|---|
| default weights (the anchor) | **89.6% ± 4.0** (p < 0.0001) |
| `GreedyBot` (`kindmatch --a weighted --b greedy`) | **97.5% ± 2.0** |
| `RandomBot` | **98.75% ± 1.4** |
| `rust_champion_3p.json`'s vector, played at a real 2p table | **54.4% ± 6.6** (p = 0.19, seed 700) / **50.8% ± 6.6** (p = 0.80, seed 12345) — statistically even on both seeds |
| `rust_champion_4p.json`'s vector, played at a real 2p table | **68.8% ± 6.3** (p < 0.0001) |

And the control that makes this decisive: the anchor itself is not simply
"weak at everything" — `weighted` playing the *default* vector also beats
`GreedyBot` 94.4% and `RandomBot` 99.6%, and `rust_champion_3p.json`'s
vector beats the *same* anchor 93.8% ± 3.2, a margin statistically
indistinguishable from the 2p champion's own 89.6%.

Put together: two vectors (2p champion, 3p vector) that both crush the
anchor by ~90% margins are themselves an even match at a real 2p table
(54%/51% across two independent seeds). The anchor comparison cannot tell
a genuinely strong strategy from this one — **its signal has saturated**.
An 88–95% score against the anchor is consistent with "this vector is much
stronger than a fixed, never-updated default" and tells you nothing further
about whether it is close to the best strategy reachable, or merely better
than a weak fixed point that a wide range of competent vectors all beat by
similar margins. **Explanation 2 (the anchor is too weak, in the specific
sense of no longer discriminating) is directly supported by evidence, not
just plausible by design.**

Caveat on the 3p/4p-vector comparisons: those vectors were never trained
for a 2p table, so any pact-related weight they carry is simply inert
there (no legal pact target exists at 2p) rather than actively wrong — a
fair, if imperfect, "different strong opponent" rather than a clean
strength benchmark.

### 3. A plausible, code-grounded mechanism for why passive play specifically survives the climb

`climb.rs::challenge` accepts a mutant only when the **one-sided lower
confidence bound** on its win share against the current champion clears the
null (`lo > null`, `accept_z` = 90% one-sided by default) — not merely a
higher point estimate. This is a deliberately conservative gate (the file's
own doc: "a veto that fires on noise would stall a healthy climb," applied
here to acceptance too), and it has a side effect the design doc does not
name: **it is variance-averse, not just mean-seeking.** A mutation that
raises the mean win rate but also raises its variance can fail to clear a
lower-bound gate that an equal- or lower-mean, lower-variance mutation
clears easily.

War is exactly the kind of high-variance move this would select against.
The human corpus's own finding (this doc, above) is that war is rare and
almost always decisive — 84–91% win rate for the side that declares it, a
near-coin-flip-stakes gamble on the roll of relative strength at
resolution time. In a **mirror self-play match** (`climb`'s own arena
seats the mutant against the champion, both `BotKind::Weighted`), an
aggressive mutation's expected value against a near-identical opponent is
close to neutral by symmetry, while its variance is not — exactly the
profile a `lo > null` gate structurally disfavors relative to a safe,
low-variance economic line whose edge is small but reliable. This is
consistent with, though not proven by, what was actually observed: 0 wars
across 300 games in two independent measurements taken hours apart, while
the 3p and 4p champions (separately trained, different local optima per
this doc's earlier "Where the bot is far off" section) each landed on
*much* more warlike postures — three different vectors, three different
resolutions of the same mean/variance tradeoff, which is itself consistent
with "the gate's variance-aversion interacts with the opponent's
population statistics" rather than "war is bad at 2p and good at 3p/4p"
being a fact about the game.

This is offered as **a plausible mechanism grounded in the actual accept
rule**, not a proven causal account — confirming it would mean mapping the
local fitness landscape around the champion (does a genuinely-tested
aggressive 2p mutant in fact show higher variance and a similar-or-better
mean, and does it fail the gate for exactly that reason), which is a
retraining/instrumentation exercise out of scope for a diagnosis that was
asked not to retrain or touch weights. **Explanation 3 (the objective
rewards this) is plausible and mechanistically grounded, not confirmed.**

### 4. Correlational, not causal: 2p's wonder-timing weight is a striking outlier

Comparing the wonder-related weights across the three live champions:

| key | 2p | 3p | 4p |
|---|---|---|---|
| `wonder_turns_to_finish` | **-6.819** | -0.440 | +0.022 |
| `wonder_stages_left` | 2.596 | -0.060 | -0.303 |
| `wonders` | -0.533 | -0.790 | +1.800 |
| `wonder_remaining` | -1.145 | -0.707 | +0.525 |
| `wonder_progress` | 1.192 | -0.592 | 0.940 |
| `card_board_wonder` | 0.378 | -0.477 | 0.520 |
| `rival_building_wonder` | -0.188 | 0.117 | 2.016 |

`wonder_turns_to_finish` stands out: 2p's coefficient is roughly 15x more
negative than 3p's and has the *opposite sign* from 4p's — a wonder that
will take many more turns to complete is penalized far harder by the 2p
vector than by either other champion. That is exactly the kind of term
that would suppress starting long wonders specifically at 2p, and it lines
up directionally with the near-zero wonder-building rate. But this is a
**correlation observed in one linear evaluator's weights**, not a
causally-isolated effect — the 64-odd weights interact, feature scales
differ by player count (fewer civil actions per turn at 2p means "turns to
finish" is a bigger number for the same wonder to begin with, which by
itself would justify *some* asymmetry in this coefficient without any
brokenness), and testing causality would mean perturbing this one weight
and re-measuring, i.e. exactly the retraining this diagnosis was told not
to do. **Explanation 4 (wonders are mispriced at 2p) is consistent with the
weight evidence and worth flagging, but not established as causal.**

### Verdict: evidence separates some of these, not all

- **Explanation 1 (bad measurement): ruled out.** Two independent
  measurement routes reproduce `botcensus`'s war and wonder numbers.
- **Explanation 2 (anchor too weak): supported directly.** The anchor
  comparison provably cannot distinguish the 2p champion from an
  independently-trained vector that is, in fact, its equal at a real 2p
  table. An 88–95% anchor score is not evidence of absolute strength once
  the anchor is this saturated.
- **Explanation 3 (objective rewards this): plausible, mechanistically
  grounded in `climb.rs`'s conservative one-sided accept gate, not proven.**
  Confirming it needs a landscape-mapping experiment this diagnosis did not
  run.
- **Explanation 4 (2p-specific wonder mispricing): a real, quantified
  correlation** (`wonder_turns_to_finish` is a 15x outlier), **not shown to
  be causal.**
- **The crash-loop connection is partial, not the whole story.** Training
  was genuinely stalled at gen 1164 by the seed-overflow bug and has
  resumed since the same-day fix — the wonder-building rate nearly tripled
  in a same-day rerun, so some of the gap was a stale-checkpoint artifact of
  training being interrupted mid-climb. But the war rate has not moved at
  all (0/300 in both measurements), which argues against "just let it keep
  training and the passivity will fix itself" as a complete account — the
  war-avoidance specifically looks like it could be the more structural
  piece (explanation 3), while the wonder gap looks like a mix of the
  structural wonder-timing weight (explanation 4) and genuine recovery
  headroom left by the crash loop.

**Recommended next action**: do not touch weights or the league. Two things
worth doing precede any weight change: (a) let the post-crash-fix training
run substantially longer (the league is running continuously; this was a
same-day snapshot) and re-run this same census in a week or two to see how
much of the wonder gap is recovery vs. a stable local optimum — war rate is
the more informative number to watch, since it hasn't moved yet; (b) if the
gap persists, the actual test for explanation 3 is a landscape-mapping
experiment outside `climb`'s own loop — measure the variance, not just the
mean, of a deliberately-aggressive 2p mutant's win share against the
champion, to see whether it is being rejected by the accept gate for
exactly the mean/variance reason argued above. Both are measurement
proposals, not fixes — this diagnosis was scoped to find out which
explanation the evidence supports, and the honest answer is "anchor
saturation, demonstrated directly, plus a plausible-but-unconfirmed
variance-averse selection pressure — not a bug."
