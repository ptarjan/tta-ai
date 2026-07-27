# Fixing the scoring bugs `docs/SCORE_VALIDATION.md` found (2026-07-27)

Branch: `score-bugfix`, merged to master. Acts on §3 of
`docs/SCORE_VALIDATION.md`, which located three scoring bugs against the
1,011-game BGO human corpus and deliberately left them unfixed so the gate
digests would stay put for that measurement.

All three are confirmed and fixed. **A fourth fell out of fixing the third**,
and it is the only one of the four that was visible outside the scoring code.
All four gate digests moved and were re-derived deliberately (§4). The suite
adds 25 tests (23 in `tests/test_scoring_bugfix.py`, 2 in
`tests/test_bgo_rescore.py` for the new oracle) — 401 -> 426 on the branch,
**461 green** after rebasing onto master `9c8b6f5`, whose own 35 tests and all
four digests are unaffected by these changes.

## One-paragraph answer

The three reported bugs were real, the rules and BGO agree on all three, and
fixing them moves each one's own oracle: `Impact of Industry` **452 → 542 of
542**, `Impact of Population` **322 → 341 of 584**, `Hollywood` **85 → 168 of
186**, `Internet` **174 → 247 of 293** (all-rows counts, whose denominator
does not move). Fixing Hollywood exposed a fourth: **Charlie Chaplin was
doubling every worker on the best theater card instead of one building**, and
because that is a culture *rating* bug rather than a scoring bug it has an
independent oracle — BGO's printed per-turn culture, on 43,847 lines, which
goes **91.9% → 92.9%** and drags all-five-rates agreement from 79.2% to 80.0%
and turn-16+ agreement from 58.1% to **62.1%**. **The wonder A/B did not
move.** `docs/SCORE_VALIDATION.md` §6.2 hoped that Bug 3 was suppressing
wonder payoffs; re-run on both vectors after the fix, forcing wonders still
costs the production vector **−33.4 ± 6.9** margin (was −34.3 ± 7.0) and is
still worth **+20.8 own culture and zero margin** to the quiescent champion
(+4.3 ± 7.0, unchanged to the decimal). The reason is measured in §3: the
unforced production bot completes **one** Age III wonder in 80 seat-games,
so the two cards Bug 3 touched were essentially never scored at all.

---

## 1. The four bugs

### 1.1 `Impact of Industry` scored the resource rating (over-scored)

Card, digital edition (`sources/bga_throughtheages_material.inc.php:3835`,
and the same wording in `data/cards_military_actions.json`): *"Each
civilization scores culture equal to the amount of resources its mines
produce. **(Ignore any production from other sources.)**"* Rules and BGO
agree; nothing to adjudicate.

`engine/events.py` read `v * s.resources`. Two things put resources on the
rating without being mine production, and the card data already says which
way each goes:

* **Bill Gates** — his own card text: *"stored as on a mine; **not affected by
  Transcontinental Railroad or Event: Industry**"*. Excluded.
* **Transcontinental Railroad** — *"one of your best mines produces twice as
  many resources"*; the card note cites FAQ v1.5 p.9, *"benefit counts toward
  Impact of Industry"*. Included.

Now `effects.mine_resources(p)`. Residuals before the fix were `+6×9, +9×6,
+4×2, +12×2, +7×1` — every one positive and every one a Bill Gates lab level,
which is what a rating-vs-mines confusion looks like.

### 1.2 `Impact of Population` ignored unused workers (under-scored)

Card: *"2 culture per content worker above 10."* A yellow token in the worker
pool is a worker. The rulebook makes this concrete
(`sources/ubg_subsequent-rounds.txt`, "A Discontent Worker"): a discontent
worker is physically **an unused worker moved onto the happiness track**, and
"this worker still counts as an unused worker". So the population this card
counts is on-card workers **plus** the pool, minus discontent.

`engine/events.py` summed `t.workers` only. Residuals before the fix were
`−2×21, −4×8, −6×5, −8×2, −10×1` — all negative, all exact multiples of 2,
i.e. a whole number of uncounted workers.

**This one is not fully closed, and the remainder is the known-open happy-face
question, not the population formula.** Split by whether our engine says the
seat has discontent workers (clean rows only, after the fix):

| | n | exact | residuals |
|---|---|---|---|
| our discontent == 0 | 72 | **68** | +2×2, −2×1, −4×1 (mixed sign = replay noise) |
| our discontent > 0 | 16 | 5 | −2×5, −4×5, −12×1 (all negative) |

The obvious alternative — BGO does not subtract discontent at all — was
tested and **does not fit either**: 75/88 overall against the fix's 73/88,
7/16 on the discontent rows against 5/16, and its residuals go positive
(+2×9, +4×2). So neither reading is right on those 16 rows, our discontent
estimate is the suspect, and happy faces are exactly the input
`docs/SCORE_VALIDATION.md` §8 says the journal never prints. Same open
question as `Impact of Happiness` (70.8%). The card says "content worker", so
discontent stays subtracted.

### 1.3 Hollywood and Internet used printed production (under-scored)

Both cards score what buildings *give*, not what is printed on them:

* Hollywood: *"culture equal to twice the total culture production of your
  theaters and libraries"*
* Internet: *"culture equal to the combined culture, science and strength your
  urban buildings give"* — and `data/cards_wonders_leaders.json` already
  recorded the answer, *"CONFIRMED via fandom wiki + FAQ v1.5: leader effects
  on urban-building output count — Sid Meier, Shakespeare, Bach, Chaplin,
  Newton, Einstein"*. The engine implemented one of those six.

Those are exactly the six leaders who can still be alive when an Age III
wonder completes (§9.1: an Age I leader is dead before Age III), which is a
useful cross-check that the list is the whole list.

Rather than add five more special cases, `engine/effects.py` now has
`_BUILDING_OUTPUT`: a table mapping each modifier key to *(the building types
it modifies, the rating it modifies)*, and `building_output(p, types, attrs)`
which sums printed per-worker production over those buildings plus every
modifier whose types are a subset of the ones asked about. Hollywood, the
Internet and `Impact of Industry` are all three lines against it, so they can
no longer disagree with each other, and `_building_modifier` is deliberately
the same arithmetic as the matching branch of `_apply_modifier` so a card that
scores a building's output and the rating that building feeds cannot diverge.

The subset rule is what makes Shakespeare correct without a special case: his
`culturePerLibraryTheaterPair` reads a library *and* a theater, so it counts
for Hollywood (which asks about both) and would not for a theaters-only
question. Michelangelo is deliberately **not** in the table — he pays for
happy faces, not for output — which the corpus agrees with.

### 1.4 (new) Charlie Chaplin doubled a whole card, not one building

Fixing 1.3 flipped Hollywood's residual sign for Chaplin: from `−8×9` to
`+8×6, +6×2, +16×1`. Those are twice `4×{1,2}` and `3×1` — a whole number of
*extra workers* on a 4-culture (Movies) or 3-culture (Opera) theater.

Card: *"Your best theater produces twice as much culture."* One theater = one
building = one worker, the same reading the engine already gave the
Transcontinental Railroad's *"one of your best mines produces twice as many
resources"* (which it implemented as one worker). `_apply_modifier` was
multiplying by `p.worker_count(b)`.

**This is the one bug of the four with an oracle outside the scoring code**,
because it changes the culture *rating* that BGO prints on every `End turn`
line, so it is testable on 43,847 rows rather than on ~100 scoring events:

| | before | after |
|---|---|---|
| culture production == BGO | 40,280 / 43,847 (91.9%) | **40,718 (92.9%)** |
| all five rates at once | 34,733 (79.2%) | **35,077 (80.0%)** |
| turns 16+, all five | 4,107 / 7,069 (58.1%) | **4,388 (62.1%)** |
| final positions passing the cleanliness gate | 405 / 2,525 (16.0%) | **454 (18.0%)** |

For scale, the largest replayer fix in `docs/SCORE_VALIDATION.md` (Ravages of
Time) was worth 89.4% → 91.9% on the same row. This one is a third of that,
and it is in the engine.

---

## 2. Before / after, whole corpus

`python3 tools/bgo_rescore.py --journals /tmp/bgo/journals`, 1,011 games,
0 crashes. Three columns because fixing 1.4 also lets *more replays pass the
cleanliness gate*, so the clean denominators move and are not comparable
across columns; the **all-rows** counts are, because their denominator is
fixed.

Clean rows (denominator moves in the last column — read the % not the count):

| oracle | before | after 1.1-1.3 | after 1.4 too |
|---|---|---|---|
| Impact of Industry | 61 / 81 (75.3%) | 81 / 81 | **95 / 95 (100%)** |
| Impact of Population | 43 / 81 (53.1%) | 68 / 81 (84.0%) | **73 / 88 (83.0%)** |
| Hollywood (at completion) | 20 / 35 (57.1%) | 26 / 35 (74.3%) | **44 / 44 (100%)** |
| Internet (at completion) | 46 / 65 (70.8%) | 60 / 65 (92.3%) | **63 / 68 (92.6%)** |

All rows, no cleanliness filter at all (fixed denominator):

| oracle | before | after |
|---|---|---|
| Impact of Industry | 452 / 542 | **542 / 542** |
| Impact of Population | 322 / 584 | **341 / 584** |
| Hollywood | 85 / 186 | **168 / 186** |
| Internet | 174 / 293 | **247 / 293** |
| Fast Food Chains (control, untouched) | 376 / 435 | 376 / 435 |
| First Space Flight (control, untouched) | 456 / 505 | 456 / 505 |

Nothing else in the fifteen-row `Impact of ...` table moved except by gaining
rows: Agriculture, Balance, Government, Progress, Science and Wonders are
still 100%, Technology 98.9%, and Happiness (70.8%) and Strength (64.3%)
are still the two `docs/SCORE_VALIDATION.md` §8 lists as untestable.

### 2.1 A fourth oracle, added to `tools/bgo_rescore.py`

`docs/SCORE_VALIDATION.md` §3.3's Hollywood/Internet table was computed ad
hoc and not committed, so there was nothing to re-run. It is now part of the
tool: every `"...; Wonder completed; <Colour> scores N culture"` line is BGO's
own Age III one-time bonus on a tableau we can rebuild, so the seat is frozen
at that instant and `effects.on_wonder_complete` is asked the same question.
The line is only used when **exactly one** wonder finished and **exactly one**
culture figure is attributed to its owner, so the number cannot be a sum of
two effects. A row is clean when the seat has no unmodelled events, its
tokens are conserved at that instant, and the last `End turn` before it had
all five production numbers exact.

This gate is stricter than §3.3's (35 clean Hollywoods against its 72), which
is why the counts differ from that document. The *signature* is identical:
before the fix, Hollywood was wrong on Chaplin 10/10 and Shakespeare 5/5 and
right on everything else; Internet on Einstein 11/11, Shakespeare 3/3, Newton
1/1, Chaplin 2/2, and right on Sid Meier 28/30.

---

## 3. The wonder A/B: re-run, and it did not move

This was the point of the exercise. `docs/SCORE_VALIDATION.md` §3.3 noted
that Bug 3's sign made wonders *worse* in our engine than in the real game,
and §6.2 measured forcing wonders at −34.3 ± 7.0 margin for the production
vector P and +20.8 own culture for the quiescent champion Q. Same command,
same frozen champion files, same seeds, after the fix:

**P, 1-ply-lineage production vector, 1-ply search, 40 deals = 80 games/row:**

| force | own culture | rival | margin | was (§6.2) | wonders |
|---|---|---|---|---|---|
| 0.00 | 155.1 ± 5.1 | 155.1 ± 5.1 | 0.0 ± 6.0 | 0.0 ± 6.1 | 0.73 |
| 0.10 | 145.1 ± 4.4 | 155.0 ± 5.1 | −9.9 ± 5.6 | −10.8 ± 5.6 | 1.45 |
| 0.20 | 147.3 ± 4.4 | 154.9 ± 4.8 | −7.6 ± 6.0 | −8.4 ± 6.0 | 1.8 |
| 0.40 | 148.3 ± 5.0 | 153.6 ± 4.8 | −5.3 ± 6.4 | −6.1 ± 6.4 | 2.4 |
| 0.70 | 139.8 ± 5.1 | 162.6 ± 4.9 | −22.8 ± 6.0 | −23.4 ± 6.1 | 3.1 |
| 1.00 | 125.5 ± 5.6 | 158.9 ± 4.6 | **−33.4 ± 6.9** | −34.3 ± 7.0 | 3.8 |

**Q, quiescent champion `levels=1`, 25 deals = 50 games/row:**

| force | own culture | rival | margin | was (§6.2) | wonders |
|---|---|---|---|---|---|
| 0.00 | 64.9 ± 6.4 | 64.9 ± 6.4 | 0.0 ± 5.5 | 0.0 ± 5.5 | 0.40 |
| 0.20 | 71.6 ± 6.9 | 72.3 ± 5.8 | −0.7 ± 6.1 | −0.7 ± 6.1 | 0.88 |
| 0.50 | 80.8 ± 5.9 | 81.1 ± 5.4 | −0.3 ± 5.8 | −0.3 ± 5.8 | 1.46 |
| 1.00 | 85.7 ± 6.7 | 81.4 ± 5.8 | **+4.3 ± 7.0** | +4.3 ± 7.0 | 1.9 |

**Every P row moved by less than a fifth of its own standard error and every Q
row is identical to one decimal place.** `docs/SCORE_VALIDATION.md` §6.3's
conclusions stand unaltered: wonders are still bad value for the strongest
vector we have, and still invisible-but-margin-neutral for the champion.

### 3.1 Why it did not move — measured, not assumed

Because the two cards Bug 3 touched are almost never scored. Counting which
wonders actually complete, P at 2p, 40 deals × 2 seats = 80 seat-games:

| | force 0.0 (the real bot) | force 1.0 |
|---|---|---|
| Fast Food Chains / game | 0.000 | 0.188 |
| First Space Flight | 0.000 | 0.025 |
| **Hollywood** | **0.013** (1 in 80) | **0.100** |
| **Internet** | **0.000** | **0.100** |
| all wonders | 0.725 | 3.8 |

**The unforced bot completes one Age III wonder in 80 seat-games**, total,
across all four of them. Even at full forcing Hollywood and the Internet land
0.1 times each per game, and their average under-score was ~3.5 and ~1.0
culture — so Bug 3 was worth about **0.45 culture per game at maximum
forcing** and ~0.05 in normal play, against a −33 margin. It was never a
candidate explanation, and this is the number that says so.

The mechanism is mundane: Age III wonders cost 14-18 resources across 3-5
stages and become available in the last few rounds of a 20-round game. Any
future wonder work should price *when* a wonder is reachable, not whether its
payoff is implemented — §6.3's untested civil-action-budget story is still the
live hypothesis and is still untested.

### 3.2 What Bug 4 does to bot play (it is not free either)

Chaplin is the final leader in **22-33%** of P's games, so 1.4 is the one fix
that changes ordinary play — and it changes it *downward*: we were
over-crediting Chaplin's culture rating. It is small (P's unforced culture
155.2 → 155.1, inside noise) but it is not nothing, and it is why all six gate
digests moved.

---

## 4. Gate digests

`bash tools/gate.sh` on master before any change: **GATE PASS, 401 tests**,
`NARROW 2fd656b3`, `WNARROW 7fc72fca`, `WIDE 1169007d`, `WWIDE 9dc0a5a6`.

All four moved. Re-derived per `docs/PYPY.md` 9.0's rule — computed from
scratch in the working worktree and independently in a second detached one,
with the two required to agree — and **attributed rather than assumed**: each
of the four fixes was reverted on its own and all four arms re-hashed.

| | old | new |
|---|---|---|
| NARROW | `2fd656b3` | `0a6ed6ad` |
| WIDE | `1169007d` | `4a8c6ca6` |
| WNARROW | `7fc72fca` | `302c546c` |
| WWIDE | `9dc0a5a6` | `4e40a58c` |

The attribution (`SAME` = that fix alone does not move that arm):

| revert | NARROW | WIDE | WNARROW | WWIDE |
|---|---|---|---|---|
| 1.1 Industry | SAME | SAME | `142b3371` | `d7328f3a` |
| 1.2 Population | **`2fd656b3`** | **`1169007d`** | `4ce2cf6e` | `ecbfc9dd` |
| 1.3 Hollywood/Internet | SAME | SAME | SAME | SAME |
| 1.4 Chaplin | SAME | SAME | SAME | SAME |
| 1.1 **and** 1.2 together | — | — | **`7fc72fca`** | **`9dc0a5a6`** |

Read the bold cells: reverting only 1.2 puts both GreedyBot arms back on
their old master digests exactly, and reverting both `engine/events.py` hunks
puts both WeightedBot arms back on theirs. So the whole movement of all four
digests is the two `Impact of ...` fixes and nothing else.

**Two of the four fixes move no digest at all, and that is a coverage
finding.** The fingerprint's 135 games essentially never complete an Age III
wonder (§3.1: one Hollywood in 80 seat-games for the *trained* production
vector; zero for GreedyBot and DEFAULT_WEIGHTS), and never reach Chaplin with
two workers on his best theater. `tools/gate.sh` cannot catch a regression in
either 1.3 or 1.4 — only `tests/test_scoring_bugfix.py` and
`tools/bgo_rescore.py` can. That is written into `tools/gate.sh` next to the
constants, in the same place as every other cause note.

---

## 5. Negatives, nulls and what is still open

* **The wonder A/B is a null.** That is the headline result of §3 and it is
  reported as a null, not buried: the fix that was hoped to change it changed
  it by less than a fifth of a standard error, and the reason is that the
  affected cards are never played.
* **`Impact of Population` is 83%, not 100%**, and the residual is entirely on
  rows where our engine computes discontent > 0. Two readings were tested and
  neither fits. Unresolved; same root as `Impact of Happiness`.
* **`Impact of Happiness` (70.8%) and `Impact of Strength` (64.3%) are
  untouched and still open.** Nothing here looked at either. If a fifth
  scoring bug exists it is behind happy faces or behind tactics, exactly where
  `docs/SCORE_VALIDATION.md` §8 said it would be.
* **`Impact of Colonies` is 86.2%** with symmetric ±3 residuals and
  Architecture / Variety / Competition sit at 92-93% with small mixed-sign
  ones. §2 of the previous document attributes these to the replayer (stolen
  colonies, unseen military workers). Not re-examined here; nothing in this
  branch moved them.
* **The three controls did not move**, which is the check that the refactor
  did not smear: Fast Food Chains and First Space Flight are 376/435 and
  456/505 before and after, and Sid Meier's Internet rows stayed at 28/30.
* **2p only**, same as everything before it. Nothing was run at 3p or 4p.
* **`n` is 80 games per A/B row for P and 50 for Q**, unchanged from §6.2, so
  the "did not move" claim is a claim about a shift of ≲ 1 point, not about
  exact equality.

## 6. Reproducing

```
tar xzf sources/bgo/journals.tar.gz -C /tmp/bgo
python3 tools/bgo_rescore.py --journals /tmp/bgo/journals   # all four oracles

cp experiments/league_state/champion_2p.json /tmp/Q2p.json
cp experiments/archive_preplan/league_state_1ply_20260726/champion_2p.json /tmp/P2p.json
nice -n 19 python3 tools/wonder_ab.py --spec /tmp/P2p.json --deals 40 \
    --force 0 --force 0.1 --force 0.2 --force 0.4 --force 0.7 --force 1.0
nice -n 19 python3 tools/wonder_ab.py --spec quiesce:/tmp/Q2p.json,levels=1 \
    --deals 25 --force 0 --force 0.2 --force 0.5 --force 1.0
```

Everything ran `nice -n 19` alongside three live league arms.
