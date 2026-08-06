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
This is the same interaction `TRANSFER_TEST.md` documents elsewhere in this
repo (a conclusion drawn under one search need not hold under another): do
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
