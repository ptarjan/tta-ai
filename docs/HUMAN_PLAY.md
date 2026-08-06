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
