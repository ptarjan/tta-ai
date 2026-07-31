# Does the bot touch every system? A per-subsystem census (2026-07-30)

The question this answers, in the owner's words: *"do the bots now do all the
right stuff — building wonders, using governments, doing wars, taking colonies,
buying tech, doing Age III events?"*  Nothing here is a fix; it is a
measurement, and every claim carries the number it came from.

Two instruments, both new and both committed with this document:

* `tools/system_census.py` — wraps every seat and taps the five engine entry
  points that carry an **outcome** (`events.resolve_war`,
  `events.finish_aggression`, `events.resolve_event`, `interact.start_auction`,
  `interact.start_defense`).  Every tap checks `state is real` before
  recording, because the beam copies the state and calls the same functions on
  the copy; counting those would measure the search, not the game — the mistake
  [`docs/CARD_BLINDNESS.md`](CARD_BLINDNESS.md) had to fix in its own discard probe (`22e6dd3`).  It
  records card **identity**, which is what "which wonder is never built" needs
  and what `tools/bgo_botmatch.py` does not carry.
* `tools/system_report.py` — folds the blobs into the tables below.

**Failure-mode labels used throughout, and they are not interchangeable:**

* **(a) ENGINE** — the rule is not implemented, or is implemented in a way that
  removes the player's decision.  The most serious.
* **(b) UNPRICED** — the engine does it, but no feature carries the value, so
  the hill climb can never learn to choose it.
* **(c) DECLINED** — the bot prices it and picks something else.  This may be
  correct play; it is reported, not condemned.

## Method

| | 2p | 3p | 4p |
|---|---|---|---|
| games | 40 | 28 | 24 |
| seat-games | 80 | 84 | 96 |
| bot | `plan:width=2,det=1` mirror — every seat the same policy | | |
| vector | `experiments/league_state/champion_2p.json`, **gen 71, live** | `archive_prequiescent_20260730/ladder_3p/gen01314.json` | `archive_prequiescent_20260730/ladder_4p/gen00361.json` |
| engine errors | 0 | 0 | 0 |

**The 3p and 4p vectors are the ARCHIVED pre-restart ones, not what the league
is training now.**  Both arms were restarted clean today.
`experiments/league_state/champion_3p.json` is gen 0 and is **byte-for-byte
`DEFAULT_WEIGHTS`** — 0 of 118 keys differ.  `champion_4p.json` is also gen 0.
Censusing those would measure the default constants, not any policy.  The two
archived pre-restart champions are trained (101 and 100 of 118 keys away from
the defaults) and are the last vectors that played 3p/4p at strength, so they
are the subject here.  Read every 3p/4p number below as *"the policy the league
had before today's restart"*.  Only the 2p column describes a currently-live,
currently-training vector.

Human numbers are `sources/bgo/journals.tar.gz` via `tools/bgo_parse.py` /
`tools/bgo_stats.py` — 1,011 games, 2,526 player-rows (692 2p / 133 3p /
186 4p).  Card-identity distributions (which wonder, which leader, which
government, which colony, which war) are pulled from the journals against
`engine.cards.db()` names.  Where the corpus carries no such number this
document says **no human baseline** rather than inventing a target.

n is 24–40 games a cell.  A factor of two here is a finding; ten percent is
not.  Everything called out below is a factor, a zero, or a card identity.

## Headline

All numbers per player per game unless the row says `/game`.  `h` = human.

| system | 2p bot | 2p h | 3p bot | 3p h | 4p bot | 4p h | verdict |
|---|---|---|---|---|---|---|---|
| wonders completed | 1.53 | 2.74 | 0.24 | 2.45 | 0.16 | 2.48 | **1.8× / 10× / 16× under** |
| wonder stages | 5.50 | 8.77 | 1.23 | 8.01 | 0.43 | 8.01 | under, worsening with table size |
| government changes | 1.06 | 1.14 | 1.37 | 1.16 | 1.44 | 1.18 | **healthy** |
| wars declared /game | 1.10 | 0.51 | 3.18 | 0.48 | 4.79 | 0.61 | **2.2× / 6.6× / 7.9× over** |
| aggressions /game | 1.73 | 1.39 | 2.36 | 1.63 | 2.46 | 3.01 | healthy |
| colonies held | 0.54 | 1.51 | 1.41 | 1.15 | 1.19 | 1.39 | 2.8× under at 2p, healthy at 3p/4p |
| colony bids | 0.61 | 3.22 | 5.39 | 2.38 | 2.35 | 3.36 | 2p under, 3p over |
| tech: yellow (farm/mine) | **0.20** | 2.52 | **0.10** | 2.47 | 1.10 | 2.72 | **13× / 26× / 2.5× under** |
| tech: blue (urban) | 6.00 | 3.71 | 2.74 | 3.86 | 1.44 | 4.00 | 1.6× over at 2p |
| tech: red (units) | **0.15** | 3.84 | **0.06** | 2.79 | **0.45** | 3.43 | **26× / 47× / 8× under** |
| tech: green (special) | 1.78 | 3.08 | 3.04 | 2.45 | 3.90 | 2.58 | under at 2p, over at 3p/4p |
| civil cards taken | 23.5 | 34.3 | 22.8 | 29.6 | 24.3 | 30.2 | ~7–11 cards short |
| cards taken in Age IV | **0.00** | 1.59 | **0.00** | 1.57 | **0.00** | 1.78 | **exact zero** |
| leaders played | 2.30 | 3.69 | 3.12 | 3.61 | 3.13 | 3.56 | 2p under, 3p/4p healthy |
| pacts offered | 0.00 (rule) | — | 1.10 | — | 1.59 | — | alive |
| pacts held at end | 0.00 (rule) | — | 0.52 | 0.86 /game | 0.55 | 2.64 /game | alive, under at 4p |
| Age III events revealed /game | 3.35 | — | 2.57 | — | 4.33 | — | alive, no human baseline |
| final score | 200 | 160 | 124 | 176 | 121 | 195 | **2p now out-scores humans** |

## 1. Wonders — the largest remaining gap, and it is about WHICH wonder

Started / completed / lost, per seat:

| | 2p | 3p | 4p | human |
|---|---|---|---|---|
| started | 2.33 | 0.66 | 0.24 | 2.78 / 2.50 / 2.52 |
| completed | 1.53 | 0.24 | 0.16 | 2.74 / 2.45 / 2.48 |
| finish rate | **66%** | 36% | 65% | **98.5%** |
| still unfinished at game end | 0.26 | 0.25 | 0.10 | 0.11 (4% of players) |
| lost to antiquation mid-game | 0.54 | 0.27 | 0.06 | not in corpus |
| stages built | 5.50 | 1.23 | 0.43 | 8.77 / 8.01 / 8.01 |

Two engine facts that fix the vocabulary:

* **A wonder cannot be swapped.**  `actions.py:145` refuses a wonder take while
  `p.wonder is not None`.  So "abandoned" never means "started a better one";
  it means either *unfinished at game end* or *discarded by `game._antiquate`*
  when its age falls two ages behind.  The latter is 0.54/seat at 2p, i.e. a
  quarter of all wonders started at 2p are simply binned by the age advance.
* **The Age III wonders are unfinishable in practice.**  They cost 14–16
  resources across 3–5 stages and the game ends ~1.5 rounds into Age IV.

Which wonders, 2p (taken → completed, out of 40 games):

    Pyramids                  7 → 0      Transcontinental Railroad  13 → 0
    Hanging Gardens          18 → 17     Eiffel Tower               20 → 17
    Colossus                  0 → 0      Kremlin                    21 → 17
    Library of Alexandria    15 → 8      Ocean Liners                6 → 0
    Great Wall               12 → 11     First Space Flight          5 → 0
    St. Peter's Basilica     24 → 22     Fast Food Chains            4 → 0
    Universitas Carolina     11 → 11     Internet                    5 → 0
    Taj Mahal                20 → 19     Hollywood                   7 → 0

**8 of the 16 wonders are completed zero times at 2p, 10 of 16 at 3p, 9 of 16
at 4p.**  All four Age III wonders are 0 at every player count.  Pyramids —
the single most-completed wonder in the human corpus (499 completions in 692
2p games) — is taken 7 times and finished **zero**.  Colossus is never even
taken at 2p.  Humans complete **all 16** at every player count.

The pattern is not stage count (Great Wall is 4 stages and 11/12 finished); it
is total resource cost plus age.  Everything at ≤13 total resources and age ≤ II
gets finished; everything at ≥12 in Age II and everything in Age III does not.
The bot starts the expensive ones (Transcontinental Railroad taken 13 times at
2p, 14 at 4p) and never pays them off.

**Does this track the wonder weights?**  Partly, and only at the extreme.  On
the vectors actually run: `wonder_progress` = 2.430 (2p) / 2.643 (3p) / 0.0018
(4p), `wonder_potential` = 0.0 / −0.022 / +0.131.  Completions are 1.53 / 0.24
/ 0.16.  The 4p vector, whose `wonder_progress` is effectively zero, is the
worst — consistent.  But 3p carries a **higher** `wonder_progress` than 2p and
completes **6.4× fewer** wonders, so `wonder_progress` alone does not predict
the behaviour; `row_urgency` (−0.191 at 2p, **+0.160** at 3p — the wrong sign
for a post-move residual, as [`analysis/frozen/README.md`](../analysis/frozen/README.md) already flags) and
`card_board_credit` (0.361 at 2p, 0.0 at 3p and 4p) differ too, and player
count itself changes wonder competition.  **This census cannot attribute the
2p/3p difference to any one weight and does not.**

Correcting a figure in circulation: **the live champion does not complete
"~0.051 wonders per deal".**  That number came from a probe with
`wonder_potential` pinned to 0.0.  The measured live-2p rate is **1.53 completed
per seat per game**, 30× that.

Label: **(c) DECLINED for the cheap wonders — it builds them.  (b)/(c) for the
expensive ones**: `wonder_remaining` (a *cost* weight, −0.062/−0.075/−0.087)
is the identity channel that survives, so a dearer wonder is scored worse
almost by construction, and nothing prices the endgame culture it would pay.
Not **(a)**: the engine builds wonders correctly and the rules are right.

## 2. Governments — healthy, and it reaches Age III

| | 2p | 3p | 4p | human |
|---|---|---|---|---|
| changes per seat | 1.06 | 1.37 | 1.44 | 1.14 / 1.16 / 1.18 |
| of which revolutions | 0.79 | 1.04 | 0.79 | not split in corpus |

**The bot does not die in Despotism.**  Of 80 seats at 2p, 12 end on Despotism
(15%); at 3p it is 5 of 84 (6.0%) and at 4p 4 of 96 (4.2%), against a human
6.7%.  Age II and Age III governments are reached
routinely: final governments at 2p are Theocracy 29, Democracy 22, Despotism
12, Communism 6, Constitutional Monarchy 4, Fundamentalism 3, Monarchy 3,
Republic 1 — **all 8 governments occur**, including all three Age III ones.
Both change routes are implemented and both are used: `("develop", gov)` is the
peaceful full-price change and `("revolution", gov)` the cheap all-civil-actions
one (`actions.py:507-511`).

The distribution is off-human: the bot's most common first change is
**Theocracy** (35 of 85 at 2p) where 35% of humans go Constitutional Monarchy
and 22% Republic.  That is a taste difference, not a coverage hole.

Label: **(c)** on the choice of government, nothing broken.

## 3. Wars and aggressions — the bot fights far too much, and only over culture

| /game | 2p | 3p | 4p | human |
|---|---|---|---|---|
| wars declared | 1.10 | 3.18 | 4.79 | 0.51 / 0.48 / 0.61 |
| wars resolved | 1.10 | 3.18 | 4.79 | — |
| attacker won | 0.98 | 2.36 | 4.00 | humans win 84–91% |
| attacker lost or drew | 0.13 | 0.82 | 0.79 | — |
| aggressions played | 1.73 | 2.36 | 2.46 | 1.39 / 1.63 / 3.01 |
| aggressions succeeded | 0.53 | 1.54 | 2.17 | — |
| **aggressions held off by the defender** | **1.20** | 0.82 | 0.29 | — |
| defences faced | 1.73 | 2.36 | 2.46 | — |
| defence cards spent | 1.23 | 0.93 | 0.54 | — |

**Aggression is now healthy at all three counts** — within a factor of ~1.5 of
the human rate, and at 4p the bot is *below* it.  **War is the outlier**:
2.2× the human rate at 2p and **6.6–7.9×** at 3p/4p.  The bot wins 89% of the
wars it declares, which humans also do, so the anomaly is purely the frequency.

**Defence works now.**  [`docs/AGGRESSION_RATE.md`](AGGRESSION_RATE.md#5-under-search-defence-is-reached--and-still-never-won-this-one-is-real) §5 reported *"1,549 defences
faced, 1,104 winnable, and zero won"* and proposed `QUIET_PENDING` as the fix.
That fix has since been flipped on — `engine/bots/pending.py:71` reads
`QUIET_PENDING = True` — and the census measures the consequence: **48 of 69
aggressions at 2p (70%) are held off by the defender.**  That section of
[`AGGRESSION_RATE.md`](AGGRESSION_RATE.md) is stale; the zero is gone.

Which wars, by type:

| | bot 2p | h 2p | bot 3p | h 3p | bot 4p | h 4p |
|---|---|---|---|---|---|---|
| War over Culture | 43 | 222 | 68 | 42 | 96 | 83 |
| War over Territory | **0** | 57 | 20 | 13 | 19 | 17 |
| War over Technology | 1 | 72 | 1 | 9 | **0** | 14 |

**War over Technology is effectively never declared** — 2 in 248 declarations
across all three counts, against a human 20% share, despite the deck holding 2
copies of it and 2 of Territory against 6 of Culture.  War over Territory is
never declared at 2p specifically.  The engine implements all three
(`events.WAR_SPOILS`, and `a7a5ef1` gave the victor of a War over Technology
the choice the rules require), so this is not **(a)**.  The bot's objective is
culture and War over Culture pays culture directly, so **(c)**, shading into
**(b)**: nothing converts stolen science or yellow tokens into the score the
search maximises.

## 4. Colonies — alive everywhere, thin at 2p

| | 2p | 3p | 4p |
|---|---|---|---|
| auctions started /game | 2.93 | 4.50 | 8.13 |
| bids /seat | 0.61 | 5.39 | 2.35 |
| colonies held at end /seat | **0.54** | 1.41 | 1.19 |
| human colonies /seat | 1.51 | 1.15 | 1.39 |
| human bids /seat | 3.22 | 2.38 | 3.36 |

**All 12 territory cards are auctioned at every player count** — the auction
machinery reaches the whole deck, so nothing is structurally unreachable.  At
3p and 4p **all 12 are also won**.  At 2p, 4 of the 12 are never won: Vast
Territory (I) and (II), Inhabited Territory (I) and (II) — the two that pay
food and population rather than culture/science/resources.  Colonies at 2p are
2.8× under the human rate; at 3p/4p the bot is at or above it.

`docs/BEHAVIOUR_AFTER_FIXES.md` (deleted 2026-07-30)'s "colonies at 4p are still effectively zero"
(0.01 bids/game, 2026-07-26) is **stale**: 4p is now 2.35 bids/seat and 1.19
colonies/seat.

One rules gap, and it is **(a)**: *the bot never chooses what to sacrifice.*
`interact._build_force` picks the payment greedily ("bonus cards before units",
cheapest unit first) with no decision exposed to the policy, where §11.3 makes
the sacrifice the colonising player's choice.  It is a small edge — the greedy
choice is usually right — but it is a rule the engine takes away from the
player, so it belongs in category (a), not (b).

Label otherwise: **(c)** at 3p/4p, **(c)** at 2p with the food/population
territories being the ones declined.

## 5. Technology by colour — the biggest structural hole in the whole census

Colour buckets, stated here because the base game does not print the words:
**yellow** = farm + mine, **blue** = lab/temple/library/arena/theater,
**red** = infantry/cavalry/artillery/air, **green** = special technologies.
Human counts are derived the same way from the journals' `takes X in hand`
lines against the same card database, with take-backs subtracted.

| takes /seat | bot 2p | h 2p | bot 3p | h 3p | bot 4p | h 4p |
|---|---|---|---|---|---|---|
| yellow (farm/mine) | **0.20** | 2.52 | **0.10** | 2.47 | 1.10 | 2.72 |
| blue (urban) | 6.00 | 3.71 | 2.74 | 3.86 | 1.44 | 4.00 |
| red (units) | **0.15** | 3.84 | **0.06** | 2.79 | **0.45** | 3.43 |
| green (special) | 1.78 | 3.08 | 3.04 | 2.45 | 3.90 | 2.58 |
| action cards | 8.59 | 12.82 | 10.00 | 10.07 | 10.75 | 9.46 |
| governments | 1.54 | 1.37 | 2.63 | 1.41 | 2.64 | 1.43 |
| leaders | 2.86 | 3.00 | 3.44 | 2.88 | 3.58 | 2.94 |
| wonders | 2.35 | 2.74 | 0.81 | 2.45 | 0.48 | 2.53 |

**The bot plays essentially the entire game on its Age A production and Age A
army.**  It takes 0.15 unit cards per seat-game at 2p against a human 3.84 —
**26× under** — and 0.20 farm/mine cards against 2.52.  Both are near-zero, and
both are the *same* defect: [`docs/CARD_BLINDNESS.md`](CARD_BLINDNESS.md#1111-units-were-negative-not-zero) §11.1.1 shows every
unit card prices to a strictly **negative** `card_potential` (−0.57 Warriors to
−4.40 Air Forces) because `unit_strength_credit` is the gate and it is **0.0 on
every vector this census ran** (2p live, 3p archived, 4p archived — checked).
`row_pressure` additionally skips any card with `card_potential <= 0`, so a
unit in the row is invisible to the row terms as well.

Label for red: **(b) UNPRICED, with the wrong sign** — the most actionable
finding in this document.  Label for yellow: **(b)/(c)** — farms and mines are
priced absolute-not-delta ([`docs/UNCOVERED_TYPES.md`](UNCOVERED_TYPES.md#0-summary) §0) and the bot substitutes
blue urban buildings, which it takes 1.6× more than humans do.

By age, and one exact zero:

| takes /seat | bot 2p | h 2p | bot 3p | h 3p | bot 4p | h 4p |
|---|---|---|---|---|---|---|
| age A | 2.43 | 1.32 | 1.94 | 1.55 | 2.18 | 1.65 |
| age I | 7.25 | 10.16 | 6.73 | 8.42 | 6.96 | 8.62 |
| age II | 7.10 | 9.98 | 6.75 | 8.75 | 7.26 | 8.85 |
| age III | 6.69 | 11.22 | 7.39 | 9.27 | 7.94 | 9.28 |
| **age IV** | **0.00** | 1.59 | **0.00** | 1.57 | **0.00** | 1.78 |

**The bot takes zero cards in Age IV, in 260 seat-games, at every player
count.**  Humans take 1.6–1.8.  The engine does not forbid it — `_advance_age`
empties the *deck* when Age IV starts (`game.py:170`) but the card row still
holds Age III cards and a take is still legal.  Age IV is one or two rounds
long and the bot always spends its civil actions elsewhere.  Label **(c)**,
probably correct play, but it is an exact zero and is listed as one.

## 6. Events, including Age III — fully exercised

| | 2p | 3p | 4p |
|---|---|---|---|
| events prepared /seat | 8.90 | 8.24 | 10.02 |
| ..of which territory cards | 1.49 | 1.55 | 2.05 |
| events revealed /game | 14.88 | 20.21 | 31.96 |
| age A revealed /game | 4.00 | 5.00 | 6.00 |
| age I revealed /game | 3.35 | 4.89 | 10.29 |
| age II revealed /game | 4.18 | 7.75 | 11.33 |
| **age III revealed /game** | **3.35** | **2.57** | **4.33** |
| final age reached | IV in 40/40 | IV in 28/28 | IV in 24/24 |

**Age III events are not a gap.**  Every game reaches Age IV, 2.6–4.3 Age III
events are revealed and resolved *during play* per game, and the ones still in
the current/future piles at game end pay out through
`events.evaluate_final_events` → `final_event_awards`, the single place the
fifteen "Impact of …" formulas are stated and the same function the evaluator
forecasts with (`weighted.event_scoring_margin`).  Both halves are exercised.

What *is* weak is the **choice** of which event to prepare, not the rate:
[`docs/CARD_BLINDNESS.md`](CARD_BLINDNESS.md#1241-the-ranking) §12.4.1 tier A #3 measures `flat` = 0.775–0.897 — at most
decisions every event in the hand scores identically, because `_card_yields`
returns the empty tuple for all 55 event cards and `hand_mil_potential` is 0.0
(2p, 4p) or 0.011 (3p).  Label **(b)**, and it is about ordering, not coverage.
No human baseline exists for prepare rates: the BGO journal prints the event
that came up, not the hand it came from.

## 7. Leaders — all 24 reachable, 2p uses fewest

| | 2p | 3p | 4p | human |
|---|---|---|---|---|
| leaders played /seat | 2.30 | 3.12 | 3.13 | 3.69 / 3.61 / 3.56 |
| distinct leaders ever played | **18 / 24** | 24 / 24 | 24 / 24 | 24 / 24 |

At 3p and 4p **every leader in the game gets played**.  At 2p, six never do in
80 seat-games: Aristotle, Hammurabi, Frederick Barbarossa, Genghis Khan, Albert
Einstein, Bill Gates.  Four of those six are military or science leaders, which
is the same shadow the red/yellow hole in §5 casts.  With 80 seat-games and 24
leaders competing for ~2.3 slots each, some zeros are expected from supply
alone, so the 2p list is suggestive rather than conclusive.

Label **(c)**, with the 2p military-leader absences downstream of **(b)** in §5.

## 8. Pacts — alive at 3p and 4p, absent at 2p by rule

| /seat | 2p | 3p | 4p |
|---|---|---|---|
| pacts offered | 0.00 | 1.10 | 1.59 |
| pacts held at game end | 0.00 | 0.52 | 0.55 |
| pacts cancelled | 0.00 | 0.00 | 0.01 |

Zero at 2p is the **rulebook** (RULES_SPEC §13: pacts are removed from the
decks in a two-player game — `actions.py:295` enforces it), not a defect.

Per game: 1.57 pacts standing at the end of a 3p game and 2.21 at 4p, against a
human corpus that logs **0.86 accepted per 3p game and 2.64 per 4p game**.  So
3p is above the human rate and 4p slightly below it.  The corpus does not log
pact *offers*, only acceptances, so only the standing count is comparable.
`docs/BEHAVIOUR_AFTER_FIXES.md` (deleted 2026-07-30)'s pact fix holds up.  Cancellation is ~never
used (1 in 260 seat-games); no human baseline.

Label **(c)**.

## 9. The one-off systems

Decisions per seat-game, 2p / 3p / 4p:

| system | 2p | 3p | 4p | status |
|---|---|---|---|---|
| military hand-limit discard | 29.09 | 23.16 | 20.89 | exercised on ~every turn |
| defence decisions faced /game | 1.73 | 2.36 | 2.46 | reached, and now **won** 70% at 2p |
| defence cards spent /game | 1.23 | 0.93 | 0.54 | real spend |
| units disbanded (`destroy`) | 0.44 | 1.19 | 1.04 | exercised |
| unit sacrifice for a colony | **no decision exists** | | | **(a)** |
| Age A cards taken | 2.43 | 1.94 | 2.18 | above human (1.32/1.55/1.65) |
| `bonus` (Military Bonus) cards | **no move handler exists** | | | **(a)**-by-design |
| tactics played | 0.21 | 0.77 | 0.78 | near-zero, see below |
| tactics copied | 0.08 | 1.29 | 1.42 | near-zero at 2p |

* **Military discard is a live decision** and fires 21–29 times per seat-game.
  The FIFO bug [`docs/UNCOVERED_TYPES.md`](UNCOVERED_TYPES.md) D1 describes is fixed (`1c08790`); the
  decision is now made by the policy at every one of those points.
* **Defence** is covered in §3: it is reached, cards are spent, and the
  defender now wins.
* **Unit sacrifice** — see §4.  The engine chooses the payment for the player.
  This is the one place in this census where a rule takes a decision away, and
  it is therefore the only clean **(a)**.
* **The three Military Bonus cards have no move handler at all.**  They are
  spendable only by the defence and colonisation machinery
  ([`docs/CARD_BLINDNESS.md`](CARD_BLINDNESS.md#1241-the-ranking) §12.4.1 tier C #9).  That is a rules-coverage question,
  not an evaluator one; the rulebook does not let you "play" one either, so it
  is (a)-by-design rather than a bug.
* **Tactics are near-dead at 2p** (0.21 played, 0.08 copied per seat-game) and
  the cause is §5: a tactic's value is `tacticBonus` × armies formable, and a
  bot with 0.15 unit takes per game can form no armies.  Confounded with the
  unit hole, and should be re-measured after it, not fixed in parallel.
* **Age A cards** — the four Age A technologies (Agriculture, Bronze,
  Philosophy, Religion, Warriors, Despotism) carry `count: 0` by convention
  because they are printed on the player board.  The Age A *deck* cards
  (4 wonders, 6 leaders, 10 events, 8 action cards) are all in play and the bot
  takes 2.4/seat at 2p, **more** than humans.

## What the bot never does

Ordered by how confident the zero is.

1. ~~**Buys unit technology.**~~  **FIXED 2026-07-30, see
   [`docs/CARD_BLINDNESS.md`](CARD_BLINDNESS.md).**  0.15 / 0.06 / 0.45 takes per seat-game
   against a human 3.84 / 2.79 / 3.43.  **(b)**, wrong sign: every unit priced
   to a negative `card_potential` because `unit_strength_credit` = 0.0 on all
   three vectors.  The diagnosis above is confirmed and was also found to be
   *incomplete*: removing the negative alone leaves a unit worth zero, and a
   card worth zero is still not a card worth taking (that document §1c).  A
   unit technology is now priced by a board query — the engine's own upgrade
   cost against an `effects.compute` strength diff, valued at
   d(`evaluate`)/d(strength) rather than at `w["strength"]`.  Re-censused on
   the same instrument: **0.20 → 1.06 at 2p and 0.08 → 4.16 at 3p**.
2. **Buys farms or mines.**  0.20 / 0.10 / 1.10 against a human 2.52 / 2.47 /
   2.72.  **(b)/(c)**.
3. **Takes any card in Age IV.**  0.00 in 260 seat-games, all three counts,
   against a human 1.6–1.8.  **(c)**.
4. **Completes an Age III wonder.**  0 of 16 possible in 260 seat-games (First
   Space Flight, Fast Food Chains, Internet, Hollywood, all counts).  **(c)**,
   driven by a cost-only identity channel.
5. **Completes the Pyramids.**  Taken 7 times at 2p, never finished; humans
   complete it 499 times in 692 2p games — their single most-built wonder.
   Never even taken at 3p or 4p.
6. **Declares War over Technology.**  2 declarations in 248, against a human
   share of ~20%.  Never at 4p.  **(c)/(b)**.
7. **Declares War over Territory at 2p.**  0 of 44 declarations; humans, 57 of
   351.  Alive at 3p (20) and 4p (19).
8. **Wins a food/population colony at 2p.**  Vast Territory I/II and Inhabited
   Territory I/II are auctioned but never won at 2p; all 12 are won at 3p/4p.
9. **Cancels a pact.**  1 in 260 seat-games.  No human baseline.
10. **Chooses which units to sacrifice for a colony.**  The engine chooses.
    **(a)** — the only rules-level decision loss found.
11. **Plays a Military Bonus card as a move.**  No handler exists; (a)-by-design.
12. **Plays six of the 24 leaders at 2p** (Aristotle, Hammurabi, Frederick
    Barbarossa, Genghis Khan, Albert Einstein, Bill Gates).  All 24 are played
    at 3p and 4p, so this is a 2p-taste zero on small n, not a reachability
    zero.

And the opposite — **wildly above human**:

1. **War declarations**: 2.2× at 2p, **6.6× at 3p, 7.9× at 4p**.  The single
   largest over-shoot in the census.
2. **Blue urban buildings at 2p**: 6.00 vs 3.71, 1.6×, and it is where the
   yellow/red budget went.
3. **Governments taken**: 2.63 / 2.64 at 3p/4p vs a human 1.41 / 1.43, ~1.9×.
4. **Age A cards**: 2.43 vs 1.32 at 2p, 1.8×.

## Which existing documents this supersedes

* **[`docs/HUMAN_BASELINE.md`](HUMAN_BASELINE.md) (2026-07-27) is materially stale on four axes.**
  It reports the 2p bot at 0.40 wonders completed, 1.91 stages and **84.1**
  final score against a human 156.  This census, on the current live 2p
  champion under `plan:width=2`, measures **1.53 / 5.50 / 199.8**.  The wonder
  gap has closed from 6.9× to 1.8× and the score gap has **reversed** — the 2p
  bot now out-scores the human median.  Its war finding (2.9× over) survives
  and has got worse at 3p/4p; its "bot stops colonizing at 4p" finding does not
  survive (1.19 colonies/seat now).
* **`docs/BEHAVIOUR_AFTER_FIXES.md` (2026-07-26, deleted 2026-07-30) is stale on three rows.**
  Wars "0.00 at all counts" is now 1.10–4.79 per game; aggressions "0.00" are
  1.73–2.46 per game; colony bids at 4p "0.01, still ~zero" are 2.35 per seat.
  Its pact conclusion holds.
* **[`docs/AGGRESSION_RATE.md`](AGGRESSION_RATE.md#5-under-search-defence-is-reached--and-still-never-won-this-one-is-real) §5 (2026-07-30) is stale within the same day.**
  "1,549 defences faced … zero won" was measured with `QUIET_PENDING` off; the
  default is now `True` (`engine/bots/pending.py:71`) and 70% of 2p aggressions
  are held off.
* **[`docs/CARD_BLINDNESS.md`](CARD_BLINDNESS.md#1241-the-ranking) §12.4.1 tier A #1** ranked wonders as the archetypal
  severed pipe on the frozen 78-key champions.  On the live 2p vector the pipe
  conducts (1.53 completions/seat).  Its tier A #2 — units — is **confirmed
  intact and is now the top-ranked hole**: §5 above measures the behavioural
  consequence at 26–47× under human.
