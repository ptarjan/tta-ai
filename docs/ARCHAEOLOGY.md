# Archaeology: a ledger of lost and unfinished work

Date: 2026-07-26. Branch `archaeology`, worktree off `master` @ `8e751cb`.
Read-only dig: no engine code was changed, no experiment was run, nothing was
retried. Every "verified" line below is a read or a static check against the
tree at `8e751cb`.

Scope covered: all 22 files in `docs/` (~11,200 lines), plus `analysis/`,
`engine/`, `experiments/`, `tools/`, the three stray `PROGRESS.md` working logs
outside `docs/`, git history (deleted files, unmerged branches, dangling
objects, stashes) and in-code TODO/FIXME markers.

**Reading conventions.** Every finding carries a status:

* **PROVEN** — finished, and the evidence would survive a re-read (n >= 200,
  interval quoted, control arm present).
* **UNPROVEN** — finished, but the evidence is thin (n < 200, no interval, no
  control, never replicated). *This project's recurring failure mode is a
  confident result that later reverses*, so n < 200 is treated as unproven on
  principle.
* **ABANDONED** — started, never finished.
* **CONTRADICTED** — a later document or commit silently says otherwise.
* **STALE** — may have been true; the code it describes has since changed.

MEASURED vs INFERRED is marked inline. Where a claim cannot be settled either
way from the tree, it says so rather than guessing.

**Re-verification.** Parts of this dig ran in parallel. The three
highest-ranked findings were independently re-checked afterwards rather than
taken on report, and all three held exactly:

* `git diff master...has-unit -- engine/bots/weighted.py` is a **9-line diff**
  adding `"has_unit": 1.0 if unit_workers else 0.0` to `features()` and
  `"has_unit": 1.0` to `BASE_WEIGHTS`, with a comment citing §11.3 and
  `docs/AGGRESSION_FIX.md`; `grep -rn "has_unit"` over `engine/ experiments/
  tools/ tests/` on master returns **0 hits**.
* `tools/gate.sh` carries `NARROW=6f5c72ef` / `WIDE=7814c5c9` at lines 26-27
  and `WNARROW=dff85378` / `WWIDE=477d1c1f` at lines 35-36, as described.
* `len(BASE_WEIGHTS) == 62`, `len(DEFAULT_WEIGHTS) == 82`, and calling
  `experiments.summarize.group_of` over `BASE_WEIGHTS` returns `"?"` for
  exactly `['pact_blocks_attack', 'auction_committed', 'auction_bid',
  'hand_potential']`.

---

# PART I — THE RANKED SHORTLIST

Ranked by recoverable value: how much it would change what we do next, divided
by what it costs to act on. The table is the ranking; the numbered sections
below are the detail and are **not** in rank order (they were written in the
order the evidence arrived — the `#` column maps them).

| rank | finding | # | cost | why it ranks here |
|---|---|---|---|---|
| 1 | `has-unit` branch: 9 lines, applies clean, never A/B'd — the only genuinely lost work in git | 13 | ~1h compute | a rules-justified feature parked on an explicit "do not merge until the A/B is run", and the A/B harness now exists |
| 2 | `tools/gate.sh`'s two WeightedBot digests are stale — the gate **fails on a clean tree today** | 6 | ~2 min | a guardrail that cries wolf is a guardrail that gets ignored, in a repo with 11 live worktrees |
| 3 | Three tools silently default to pre-horizon champions and print invalid numbers without crashing | 14 | 3 one-line edits | highest risk of the *next* wrong conclusion, because nothing fails |
| 4 | The per-weight ablation ledger has been running all night and nobody has read it | 3 | free (already paid) | answers the project's oldest open question — "does this weight matter?" |
| 5 | The war/aggression fix was promised, never written, still missing | 1 | medium | two independent diagnoses, exact mechanism, half-built in `QuiescentBot` |
| 6 | `QuiescentBot` is built, costed, integrated — and has never been scored | 2 | in flight | the direct test of "is it the ply count?"; also gates deleting the hand-priced patches |
| 7 | `HEURISTICS.md` describes three champions that no longer exist, and the live 2p vector contradicts its headline advice | 15 | re-label | the most reader-facing document in the repo is describing deleted bots |
| 8 | The "compensating weights" conclusion is right but the repo already measured that removing the flaw makes it worse | 4 | read before acting | stops a plausible next step that is a measured 10-20 culture regression |
| 9 | The weight guard forbids a region the repo's own measurement says is better | 5 | one ablation | a constraint on a live multi-hour run, contradicted and never revisited |
| 10 | PyPy's own stated re-test trigger has been met and nobody re-tested | 16 | minutes | the interpreter decision was made against a copy-bound bot that no longer exists |
| 11 | `experiments/summarize.py` bins the four newest features as `"?"` in every published weight table | 17 | 4 strings | silently corrupts the tables three documents are generated from |
| 12 | `experiments/behaviour.py` has been broken across three sessions; a 3-line fix gates every behaviour re-harvest | 18 | 3 lines | cheapest unblock in the repo |
| 13 | Finished programmes with an explicit "re-run this" that was never run | 10 | hours | three of them, all cheap relative to what was already spent |
| 14 | `experiments/PROGRESS.md` is a weight-interpretation document a later audit invalidated | 7 | delete/stamp | most advice-shaped, least supported |
| 15 | The pact gate is a live-count check where the rule is setup-time | 11 | delete 2 lines | verified wrong, zero test coverage on master, a branch test already fails on it |
| 16 | `baselines.jsonl` still has no timestamp/generation/seed | 8 | a few lines | the known mechanism by which this project publishes stale numbers |
| 17 | The project still has **no external anchor of any kind** | 19 | 12-18h | every strength number in the repo is self-referential |
| 18 | `analysis/opening_order.py` half-fixed: now runs and reports `"?"` for every card | 9 | 2 lines | worse than before — it no longer crashes |
| 19 | Assorted unactioned follow-ups from finished audits | 12 | small each | one of them (the pact accept branch) has teeth |
| 20 | Stale counts (82 vs 78 weights) rooted in one docstring, propagated to five docs | 20 | 6 edits | makes `CULTURE_GAP.md` self-contradictory |
| 21 | Three benchmark scripts point at three deleted checkouts | 21 | delete | dead weight with no inbound references |
| 22 | Branch and dangling-object cleanup | 22 | prune | nothing recoverable is in there — verified, so it can be pruned safely |

## 1. The war/aggression fix was promised, never written, and is still missing — and the live 2p arm is training without it

`docs/AGGRESSION_FIX.md:135-137` ends its section B with

> ### Fix (same shape as the pact/colony fix, `166867d`)
> See the next section for the implementation and the A/B result.

There is no next section. The file ends at line 137, mid-thought.

**VERIFIED, MEASURED (static):** the fix does not exist in `WeightedBot`.
`deferred_credit()` (`engine/bots/weighted.py:121-173`) handles exactly two
pending kinds — `pact_offer` and `auction`. A pending `defense` (the payoff of
an aggression) hits the `continue` at `weighted.py:146-147` and is credited
nothing. `features()` (`weighted.py:412-481`) contains **no key reading
`war_declared_by_me` or `wars_declared_on_me`**; grep over `engine/` finds
those fields only in `state.py:100-101`, `actions.py:1016-1046`,
`events.py:560-567` and `fastcopy.py:93` — never in an evaluator.

So a war declaration remains a **pure cost with no representable benefit**, and
an aggression is priced at exactly the value of the card leaving hand. This is
not a tuning problem: no weight vector can select a move whose successor state
is strictly dominated.

*Not a new discovery — `docs/CULTURE_GAP.md:100-119` (committed tonight) found
the same truncation independently.* What archaeology adds is that it is still
true at `8e751cb`, and that **`QuiescentBot` already solves half of it**:
`engine/bots/quiescent.py:204-231,298` implements `WAR_LOOKAHEAD`, scoring a
`war` candidate by calling the engine's own `events.resolve_war` on a scratch
copy, and quiescence resolves the `defense` pending for aggressions. That work
is finished, sitting on master, and **its strength A/B has never been run**
(see item 2). The war hole and the unmeasured search bot are the same gap seen
from two ends.

**Worth having: yes, high.** Two independent diagnoses, exact mechanism, and a
mostly-built fix. Caveat that must travel with it: `CULTURE_GAP.md:290-333`
measured a forced-attack oracle and it gained *nothing* (4p −46.4 vs −45.1
culture margin, 2p 0.354 vs 0.271 win, both n=48 — **UNPROVEN at that n**), so
the expected return is low-to-medium, not high. Close the hole because it is a
representation defect, not because it is expected to win games.

## 2. `QuiescentBot` is fully built, costed, and has never been scored

`docs/DEEPER_SEARCH.md` §4 (strength A/B), §5 (behaviour counts) and §6
(GO/NO-GO) all read exactly `RESULTS PENDING` (lines 154-164). Sections 1-3 are
complete and the cost measurement is a genuinely good result: quiescence costs
**1.16x / 1.29x / 1.18x** at 2p/3p/4p, because it fires on only 2.7-4.1% of
candidates (`DEEPER_SEARCH.md:130-134`, MEASURED, 8 games per count — n is
small but it is a timing measurement, not a win rate, so the small n matters
much less).

Status: **ABANDONED mid-experiment**, and another agent is running the A/B
right now, so this is recorded rather than actioned.

The load-bearing observation nobody has cashed in
(`DEEPER_SEARCH.md:83-87`): at 0% truncation, `state.pending` is empty when
quiescence completes, so `deferred_credit` contributes **exactly zero** — the
quiescent bot runs with the hand-priced pact/colony patches as dead code. If it
matches or beats `WeightedBot`, the hand-priced `PACT_OFFER_CREDIT` and the
`auction_committed`/`auction_bid` weights can be deleted rather than
maintained. That is a concrete cleanup gated on one experiment.

## 3. The league's own per-weight ablation ledger has been running all night and nobody has read it

**This is the best piece of buried, already-paid-for evidence in the repo.**

`docs/OPENING_AUDIT.md:46-65` makes the sharpest epistemic claim in the
project: mutations move **19 weights at once** and are accepted on a single
48-game test, so *"the AI moved this weight, therefore it matters" is not a
valid inference anywhere in `HEURISTICS.md`*. It asked (§6 follow-up 1) for
per-weight ablation.

That ablation was subsequently **built** (`docs/LEAGUE_TRAINING.md:248-273`,
`--ablate-every`) and has been **quietly running**. Read at `8e751cb`
(MEASURED, from `experiments/league_state/weight_credit_2p.json` and
`experiments/archive_prehorizon/weight_credit_{3,4}p.json`, n=72 per weight):

| arm | weights covered | no-measurable-effect | load-bearing | harmful |
|---|---|---|---|---|
| 2p (live) | 35 | **24** | 10 | 1 (`blue_free`, +0.023) |
| 3p (archived, pre-horizon) | 21 | 16 | 4 | 1 (`best_library`, +0.022) |
| 4p (archived, pre-horizon) | 10 | 8 | 1 | 1 (`best_theater`, +0.021) |

2p load-bearing order (mean edge from zeroing the weight):
`hand_potential −0.194`, `end_turn_bias −0.189`, `culture_rate −0.110`,
`happy_margin −0.049`, `hand_value_late −0.040`, `food_rate −0.035`,
`auction_committed −0.016`, `gov_level −0.011`, `culture_early −0.010`,
`consumption −0.005`.

Three things fall straight out of this and none of them is written down
anywhere:

1. **Roughly two thirds of the weights measure as doing nothing** at n=72.
   That is `OPENING_AUDIT`'s hypothesis confirmed by direct measurement rather
   than by argument.
2. **`hand_potential` and `end_turn_bias` are the top two load-bearing weights
   at 2p** — the exact two that `docs/WASTED_ACTIONS.md` identifies as (a) the
   fix for card-identity blindness and (b) a compensation for the `end_turn`
   search artifact. See item 4.
3. **`auction_committed` measures load-bearing at 2p here (−0.016)** but the
   smoke run in `LEAGUE_TRAINING.md:480-483` measured it as *harmful*
   (+0.0833) at n=24. Directly conflicting readings of the same weight; the
   n=72 one is the better of two weak measurements. **Both UNPROVEN** — n=72 is
   below the 200 bar.

**Cheapest recoverable value in the whole dig:** the data exists, the reader
(`python3 -m experiments.hillclimb_league --report --players 2`) exists, and
nothing has ever consumed the output. Raising `--ablate-games` would make it
citable. `CULTURE_GAP.md:800-804` independently asks for exactly this on
`culture_rate_early/_late` — those two weights are **not yet in the 2p ledger**
(the cursor has covered 35 of 82).

## 4. The "trained weights compensate for a structural flaw" conclusion is well supported — but the repo has already measured that *removing the flaw makes the bot worse*

This is the item most likely to change tonight's plan, so it is stated at
length.

**Supporting (all MEASURED, and this part is PROVEN):**

* `docs/WASTED_ACTIONS.md:50-77` — `end_turn` is scored one production phase
  ahead of every sibling; the flattery is +12.57 (2p) and +26.28 in Age IV,
  while the best declined real move is worth +0.48. The hill climb pushed
  `end_turn_bias` from −3.0 to −8.28 fighting it. That weight is explicitly a
  compensation.
* `WASTED_ACTIONS.md:283-287` — `hand_value_late = −0.78` is "the hill climb
  correctly learning *cards this bot holds never become anything*", i.e. a true
  statement about the old code that the `hand_potential` fix falsifies.
* `docs/CULTURE_GAP.md:704-724` — the horizon fix invalidates the 3p and 4p
  champions (13.9% vs a 33.3% null; 20.1% vs 25.0%; n=400 each, **PROVEN**),
  with matched controls at §8e showing a perturbation that moves *more* of the
  champion's decisions is a flat null. The champions are fitted to the shape of
  the defect.

**Contradicting, or at least sharply qualifying (MEASURED, PROVEN):**

`WASTED_ACTIONS.md:184-201` removed the structural flaw five ways and the bot
got **significantly weaker every time**:

| bot | win rate vs champion @2p | n |
|---|---|---|
| `passfix`, eps 0.0 | 38.4% ± 4.8% | 400 |
| `passfix`, eps −0.05 | 39.8% ± 4.8% | 400 |
| `horizon`, eps −0.01 | 29.8% ± 4.4% | 400 |
| `horizon`, eps +4.0 | 11.0% ± 4.3% | 200 |
| null | 50.0% | |

and `WASTED_ACTIONS.md:277-281`: **card valuation plus same-horizon scoring
scores 39.8% ± 6.7% (n=200), against 69.6% for the card fix alone.** The
`end_turn` artifact must stay.

The explanation given (`WASTED_ACTIONS.md:218-226`) is the useful part: the
+12 phantom was acting as a **move-quality filter**, admitting only moves the
evaluation is confident about (`develop` +10.7, `wonder_step` +8.9) and
screening out the ones it cannot rank at all (`take` −0.16, identical for every
card in the row). Lower the bar and the bot acts on evaluation noise.

**So the correct form of tonight's conclusion is narrower than "the weights are
compensating for a structural flaw, therefore fix the flaw".** The compensating
weight is load-bearing (item 3 measures `end_turn_bias` as the #2 most
load-bearing weight at 2p), and removing the flaw without first repairing the
*representation* is a measured 10-20 culture regression. `LEAGUE_TRAINING.md`
already carries a standing warning about this
(`LEAGUE_TRAINING.md:320-326`: "**`end_turn_bias` is not a bug** … nothing in
this document should be read as licence to fix it"), and
`CULTURE_GAP.md:493-497` deliberately locks it in the two-sided guard for the
same reason. Anyone acting on the compensation framing should read those three
places first.

## 5. A load-bearing claim in the live trainer that was measured to be false and shipped anyway

`docs/LEAGUE_TRAINING.md:736-746` — **"Two probes that were meant to be
sabotage and are not."** Negating `science` to −2.0 scored **+0.0875 margin /
+0.0599 win share at 3p** and **+0.1227 margin at 4p**, taking BookBot from
0.0% to 12.5% at 3p. Negating `culture` scored +0.1148 margin / +0.0755 win
share at 3p. Win share and margin agree independently at 3p, so these are
findings about `DEFAULT_WEIGHTS`, not metric artifacts.

The weight guard's whole premise is the opposite: "a term whose
`DEFAULT_WEIGHTS` value is strictly positive means *more of this is better*, so
a trained value below zero is a sign inversion rather than a strategy"
(`LEAGUE_TRAINING.md:311-315`). The doc notes the collision itself and then:
*"Left clamping for the launch, since it prevents the known degeneracy; worth
revisiting."* It was never revisited. **VERIFIED**: `guard_weights`
(`experiments/hillclimb_league.py:150-166`) still clamps every member of
`NONNEG | NONPOS` to 0.0, and `docs/TRAINING_RUN.md:11-17` shows the live run
launched with `--weight-guard clamp`.

Status: **CONTRADICTED but still shipped, and running right now.** The guard is
forbidding a region of weight space that the repo's own measurement says
contains better vectors than the default. The counter-evidence is real too
(`science = −6.089` really did collapse 4p play to 9.7% ± 2.7%), so the honest
statement is that the relationship is **non-monotonic** and the guard is a
blunt instrument, not that it is simply wrong.

**Worth having: yes.** It is a constraint on a multi-hour training run, it is
cheap to test (the ablation machinery in item 3 is the right instrument), and
tonight's `8543933` made the guard *stricter* (two-sided) without anyone
re-opening this.

## 6. The determinism guardrail is switched off and has been for a long time

`engine/PROGRESS.md:200-208` records, as a note to whoever owns `tools/`:

> **`tools/fingerprint.json` (3229c4a0…) and `tools/fingerprint_wide.json`
> (c7e73ede…) are still the pre-f4bcac0 files and will report MISMATCH on every
> run** until they are re-saved.

**VERIFIED, MEASURED (static read of the files at `8e751cb`):**
`tools/fingerprint.json` still holds `3229c4a0f0d6a4a1…` and
`tools/fingerprint_wide.json` still holds `c7e73ede8a5bfd45…`. Neither has been
touched since `7c2eef1`. The known-good digests are recorded only in prose
(`engine/PROGRESS.md:158-161`: narrow `6f5c72ef…`, wide `7814c5c9…`, measured
at `7d40f53`), and several rules/data changes have landed since.

This is a repo where **several agents edit the engine in parallel worktrees**.
The one automated check that would catch an accidental behaviour change is
returning MISMATCH unconditionally, which means it is being ignored, which
means it is not a check. Status: **STALE / rotted guardrail.** Cost to fix: one
`perf_check save` from a clean tree — but note the reason it was left alone
(a save fingerprints the *working tree*, so it must be done from a clean
checkout, not from a worktree with uncommitted engine edits).

Related and unresolved: `engine/PROGRESS.md:194-198` flags that PYPY.md's
CPython/PyPy byte-identity claim rests on the same stale digests and **has not
been re-verified** since. Nothing suggests it broke; nobody has checked.

**And it is worse than the stale files: the maintained gate is failing too.**
`tools/gate.sh` carries four digests — `:26-27` `NARROW=6f5c72ef`
`WIDE=7814c5c9` (GreedyBot/RandomBot) and `:35-36` `WNARROW=dff85378`
`WWIDE=477d1c1f` (WeightedBot), all derived on master `6d0247c`.

* The **greedy** pair is still valid (INFERRED, strongly): `git log
  6d0247c..HEAD -- engine/ ':!engine/bots'` shows only journal conversions, and
  `engine/bots/__init__.py` changed only for the rng/journal work, which was
  verified digest-preserving at the time.
* The **weighted** pair is **STALE, and this was measured, not inferred.**
  `e990920` replaced the default `lateness()` schedule
  (`weighted.py:300-321` new; the old function survives only as
  `lateness_by_age` at `:324-329`, reachable via the `horizon_age` escape
  hatch at `:749`). One 2p seed-0 WeightedBot game run both ways gives digest
  `43f8dc56…` (HEAD default) against `51258877…` (`horizon_age=1`), with
  scores `[96,141]` vs `[88,137]` — **different**. So the default WeightedBot
  evaluation changed and `dff85378`/`477d1c1f` cannot still hold.

**Consequence: `bash tools/gate.sh` fails today, on a clean tree, with no
regression present.** That is precisely the "gate that cried wolf" failure mode
`PYPY.md:1319-1328` warns about — the mode in which a good change gets reverted
and a real regression gets blessed. Fix: re-derive the two weighted digests and
update `tools/gate.sh:35-36` with a note pointing at `e990920`. Two minutes.

There is a second-order lesson here worth keeping, from `PYPY.md:1413-1463`: a
seat census over 200 replayed generations found the live league pool is ~69%
WeightedBot, ~27% Book/Variant, **~2% GreedyBot and 0% QuiescentBot**. So the
*greedy* digests — the ones that still work — cover almost nothing the trainer
actually runs, and the *weighted* digests — the ones that cover 69% of it — are
the broken pair.

## 7. `experiments/PROGRESS.md` is a stale weight-interpretation document that a later audit explicitly invalidated

`experiments/PROGRESS.md:336-392` publishes "Top movers, 3p", "Top movers, 4p"
and "What 3p and 4p agree on" — a page of readings like *"hoarding science is a
negative"*, *"a big unfinished wonder is an asset at 4p"*, *"libraries beat
labs at 3p"*, each derived purely from a weight having drifted from its
default.

`docs/OPENING_AUDIT.md:46-65` demolishes exactly this inference, and does so
using **one of the very entries in that table**: `wonder_remaining`'s 4p sign
flip, which `experiments/PROGRESS.md:357` reads as a strategy, is measured at
**0.276 ± 0.063 against a 0.25 null — indistinguishable** (n=192,
`OPENING_AUDIT.md:280-292`), and was a hitchhiker in a gen-5 mutation that
moved 19 weights at once.

Status: **CONTRADICTED, silently.** `OPENING_AUDIT` flagged the corrections it
needed made to `HEURISTICS.md` (it did not own that file) and nobody looked at
`experiments/PROGRESS.md`, which makes the same class of claim more densely.
The whole file is also **STALE** in its mechanics: it documents **78 weights**
(now 82), `L = min(1, age_level/3)` (replaced by the turns-remaining horizon in
`e990920`), the old mirror hillclimb as the live system (replaced by the
league), and champion strength numbers from gen 21-39.

**Worth having: yes, as a deletion or a stamped-stale header.** It is the most
advice-shaped document in the repo and the least supported.

## 8. `experiments/baselines.jsonl` still has no timestamp, generation or seed — the exact defect that turned a stale number into a published claim

`docs/OPENING_AUDIT.md:336-407` traced a wrong claim in `HEURISTICS.md` ("4p is
probably above its starting point") to a stale block in `baselines.jsonl`, and
made follow-up #2: give the file a timestamp and a champion generation, because
"every row is currently unattributable, which is what let a stale number become
a published claim".

**VERIFIED, MEASURED:** rows still carry only
`{players, games, requested, win_rate, ci, null, p, culture_a, culture_b,
moves, errors, error_sample, secs, a, b}`. No timestamp, no generation, no
seed. The file is also **gitignored** (`.gitignore:8`), so it is not in the
repo at all — yet it was quoted as evidence in a committed document.

Status: **ABANDONED follow-up.** Cost: a few lines in `experiments/evaluate.py`
where the row is written. Value: it is the known mechanism by which this
project publishes stale numbers.

## 9. `analysis/opening_order.py` is half-fixed and still cannot report a card type

`docs/OPENING_AUDIT.md:95-120` diagnosed two bugs in a script owned by another
agent: (1) a `Logger` wrapper exposing `__call__ = None`, so every game raised
`TypeError` and zero games were logged; (2) `card_type()` using
`getattr(c, "type", None) or getattr(c, "kind", "?")` on cards that are **plain
dicts**, so every card type would report `"?"` even once the games ran.

**VERIFIED:** bug 1 is fixed — `analysis/opening_order.py:69` now defines a real
`def __call__(self, state)`. Bug 2 is **not** — line 41 still reads
`getattr(c, "type", None) or getattr(c, "kind", "?")`, and a direct check
confirms `engine.cards.db().get("Pyramids")` is a `dict` whose `getattr(...,
"type")` is `None`. The same pattern is at line 50 for `age`.

So the script now runs and silently reports `"?"` for every card, and its
farm-vs-mine "first production build" detector (`typ in ("farm","mine")`) can
never fire. Status: **half-repaired, still broken, and now *worse* than before**
— it no longer crashes, so it will produce a plausible-looking table of
nothing. Two-line fix.

## 10. Three finished measurement programmes with an explicit "re-run this" that was never run

All three are cheap relative to what has already been spent on them.

**(a) `docs/STRENGTH_CHECK.md:62-85`** — the BookBot-beats-champion result
(62.9% / 42.2% / 64.3% ⚠, n=400/300/300, **PROVEN** at 2p/3p; the **4p arm is
quarantined** — it is the known-degenerate vector, see `analysis/frozen/README.md`) was measured on champions
trained almost entirely *before* `7d40f53` corrected the military card counts.
The doc says plainly: *"this benchmark should be re-run once a post-`7d40f53`
champion exists"*, and `experiments/frozen/` holds the exact weights so the
comparison repeats. A post-`7d40f53` champion now exists (the league arms).
Never re-run.

**(b) `docs/WASTED_ACTIONS.md:453-498` §11** — three things to measure *before*
the retraining run, in order: (a) re-run the §2 wasted-action probe against the
fixed bot, with an explicit warning that the waste rate may **not** drop even
though the bot got much stronger; (b) re-run the book-bot benchmark, described
as "the cleanest test of the hypothesis"; (c) re-seed 4p and tune
`hand_potential` per player count. Only (c) happened —
`docs/TRAINING_RUN.md:47-49` confirms the 4p `hand_potential: 0.125 -> 0.0`
override. (a) and (b) were skipped and the long run started anyway. Both are
"minutes of compute against a run measured in hours" by the doc's own estimate.

**(c) `docs/BOT_ROSTER.md:36-39`** — the 47,520-game round robin (n=240 per
cell, zero engine errors, **PROVEN**) explicitly lists two things not run: the
reverse direction at 3p/4p (~2h, would fill the blank lower triangles), and
`experiments/roster_behaviour.py`, *"written and committed but never
executed"*. The polished tier-list write-up is deliberately deferred until a
bot finishes hill climbing, which is a reasonable call, but the roster's own
headline — **CultureBot is the real gate, MilitaryBot scales hard with table
size (1.71x par at 4p)** — is already load-bearing in the league pool design
and rests on this data.

## 11. A real rules bug, known and unfixed: the pact gate is dynamic where the rule is setup-time

`docs/PACTS_DIAGNOSIS.md:301-307` flagged it and nothing changed.

**VERIFIED, quoted from `engine/actions.py:257-259`:**

```python
        elif typ == "pact":
            if len(state.active_players()) < 3:          # §13: no pacts in 2p
                continue
```

`active_players()` is a *live* count. In a 3-player game where someone resigns
(§5.11), pacts silently become illegal mid-game for the two survivors. The real
2015 rule is a **setup** rule: remove pact cards from the military decks in a
2-player game (`docs/RULES_SPEC.md` §1.3, and the 2p copy counts in
`data/cards_military_actions.json` already implement it correctly at deck-build
time). The gate should be on seats, not survivors — or should not exist at all,
since the deck trimming already guarantees it.

Impact is genuinely low (resign is 0.07/game per `PACTS_DIAGNOSIS.md:307`) and
it is stated as such. It is on this list because it is a *known, verified,
one-line* rules mismatch that has survived a full rules audit, a diagnosis
document, and a combat audit in flight. No test covers it — `tests/` contains
no case exercising resignation-then-pact.

## 12. Unactioned follow-ups from finished audits, in one place

Cheap, individually small, all verified as still outstanding:

| # | Follow-up | Source | Status |
|---|---|---|---|
| a | Re-test `wonder_remaining` with a single-weight mutation | `OPENING_AUDIT.md:429-434` | superseded in principle by the ablation ledger (item 3), but `wonder_remaining` is not in the 35 weights covered at 2p |
| b | Re-check "4p starts 1.96 wonders, finishes 0.79" — ~1.2 abandoned wonders/game, likely pure waste | `OPENING_AUDIT.md:442-444` | never done |
| c | Ablate the 4p `hand_military` weight (0.908), which makes the 4p champion opt out of events, territories, aggressions and pacts at once | `PACTS_DIAGNOSIS.md:277-281` | never done |
| d | Expose the colonization sacrifice as a decision (currently a greedy weakest-first pick) | `engine/PROGRESS.md:126` | never done — `interact.py:562` still picks the cheapest sacrifice for the player |
| e | Features for the `defend` and `choose` move kinds | `engine/PROGRESS.md:130` | partially done: `bid` got `auction_committed`/`auction_bid`; `defend` and `choose` still have none |
| f | `--check-games 48` is too small for the regression tripwire; "use 100+" | `LEAGUE_TRAINING.md:846-856` | never raised; the live run uses the default |
| g | Drop the one-sided clamps on the ten *positive*-default phase multipliers, whose sign is equally non-gauge-invariant | `CULTURE_GAP.md:499-505` | explicitly "flagged, not done" |

Item (e) is the one with teeth: the `choose` move kind covers accepting or
refusing a pact, and `PACTS_DIAGNOSIS.md:325-329` warns that the accept side is
scored from a partner's perspective and has never been checked for a
systematic-refusal bug. The `state.decider()` fix landed
(`weighted.py:799`), so the perspective bug is gone, but "verify the accept
branch isn't being systematically refused" (`PACTS_DIAGNOSIS.md:134-138`) was
never done.

## 13. `has-unit` — the only genuinely lost work in git history

**RANK 1.** Branch `c96b653`, authored 15:21, present on origin *and* locally,
with **no worktree**. Every other branch in the repo moved during this dig;
this one has not been touched in six hours.

**MEASURED:** `git diff master...has-unit` is **9 lines in one file**
(`engine/bots/weighted.py`): a `has_unit` feature (`1.0 if unit_workers else
0.0`) plus `BASE_WEIGHTS["has_unit"] = 1.0`. The feature is **absent from
master under any name** — `grep -rn "has_unit"` over `engine/ experiments/
tools/ tests/` returns zero hits. Master has only the *linear* `unit_workers`
(`weighted.py:431`, weight 0.1 at `:625`), which is exactly what the commit
message argues is inadequate. **The patch still applies cleanly to current
master** (`git apply --check`), because the horizon rewrite did not touch its
insertion context.

The commit message states its own blocker: *"PENDING: this has NOT had its
3p/4p no-harm A/B yet, which is why it lives on a branch instead of master. Do
not merge until that A/B is run."*

The rules argument behind it is independently verifiable and is the same cliff
`AGGRESSION_FIX.md` §A measured: §11.3 requires sacrificing at least one
military unit, so `interact.start_auction` (`interact.py:508-519,565`) drops
zero-unit players from the auction *before they get a decision*. A player with
0 units and a player with 1 unit are categorically different, and a linear
`unit_workers` term cannot express a step. `AGGRESSION_FIX.md:56-60` measured
the 2p and 4p champions at **0.00 and 0.07 units per player** — i.e. sitting
exactly on the wrong side of that cliff.

Status: **ABANDONED, and recoverable.** The A/B harness that did not exist when
it was parked now does: `tools/guard_ab.py` (added `8543933`) runs the
trainer's own seed-paired `hillclimb_league._series` against `var:culture` and
`book`. It takes `key=value` overrides on an existing champion, so adding a
*new* key needs a one-line change.

**One caveat that must travel with it (INFERRED):** the horizon fix `e990920`
landed ~4.5h *after* this branch was parked, and `CULTURE_GAP.md` §8f measured
that the pre-horizon 3p/4p champions are decisively invalidated. The A/B must
run against a **post-horizon** champion from `experiments/league_state/`, not
against the 15:24 `experiments/champion_*.json` files.

A second caveat from the commit log: `b6112312` records that `has_unit` was
originally measured only in a **contaminated shared checkout**, never cleanly.
`WASTED_ACTIONS.md:390-394` is the other end of that same story — it notes
another agent adding this very feature mid-duel and argues why the mirror-match
design made it harmless there.

## 14. Three tools silently default to champions the horizon fix invalidated

**RANK 3, and the highest risk of the *next* wrong conclusion, precisely
because nothing crashes.**

**MEASURED:**

| tool | default |
|---|---|
| `tools/quiesce_bench.py:9` | `--weights experiments/champion_4p.json` |
| `tools/no_credit_check.py:51` | `quiesce:experiments/champion_4p.json,levels=2` |
| `tools/behaviour_counts.py:15` | `--spec quiesce:experiments/champion_4p.json,levels=2` |

`experiments/champion_{2,3,4}p.json` are dated 15:24. `e990920` landed at
19:42. `CULTURE_GAP.md` §8f measured (n=400) that the new `lateness()` drops
the 4p champion to **20.1% against a 25% null** and the 3p champion to **13.9%
against 33.3%**. Any number these three tools print today is measuring a
crippled vector, and they run without error.

`tools/culture_probe.py:196` already does it right — it defaults to
`experiments/league_state/champion_{k}p.json`, the live path. One-line fix
each.

Compounding this, `docs/TRAINING_RUN.md:41-44` warns that
`experiments/champion_4p.json` holds the degenerate `science = −6.089` vector
and *"never warm-start from it"* — and three tools default to it.

## 15. `docs/HEURISTICS.md` describes three champions that no longer exist, and the live 2p vector contradicts its headline advice

**RANK 7.** `HEURISTICS.md` describes champions at gens 176/132/113. Three
things happened since (**MEASURED**):

1. `experiments/champion_{2,3,4}p.json` moved on to gen 209/152/133;
2. training relocated entirely to `experiments/league_state/`, so those files
   are no longer the live champions at all;
3. **the 3p and 4p arms were wiped and restarted from `DEFAULT_WEIGHTS` at
   21:25**, following `CULTURE_GAP.md` §10 —
   `experiments/league_state/generations_{3,4}p.jsonl` both begin at
   `{"gen": 1, ..., "at": "2026-07-26 21:25:21"}`, with the pre-restart state
   preserved in `experiments/archive_prehorizon/`.

So every 3p and 4p number in `HEURISTICS.md` — the build orders, the per-count
sections, the priority lists, the wonder tables — describes **bots that were
deleted**. Status: **STALE**, comprehensively, not merely "some numbers moved".

The sharper finding is at 2p, where the arm survived. Read from
`experiments/league_state/champion_2p.json` against `DEFAULT_WEIGHTS`
(**MEASURED**), thirteen weights are now **exactly 0.0**, all clamped there by
the two-sided guard: `ca_left, civil_actions, colonies, corruption_loss,
culture_rate_early, hand_military, leader, pact_blocks_attack, pacts,
rival_culture, rival_science_rate, strength_rel, uprising`.
`experiments/league_state/guard_2p.jsonl` records the guard firing repeatedly
on most of them (`hand_mil_value` 61x, `hand_military` 57x, `ca_left` 54x,
`uprising` 18x).

Three of those zeros directly contradict `HEURISTICS.md`'s own headline claims:

| `HEURISTICS.md` claim | live 2p value |
|---|---|
| "buy civil actions — the second-largest measured failing; actions compound and almost nothing else does" (`:114-118`, `:357-383`) | `civil_actions = 0.0`, `ca_left = 0.0` |
| "`uprising` is the largest single term at every count: −14.0 / −15.5 / −21.2" `[strong]` (`:2003-2005`) | `uprising = 0.0` — and the search kept driving it *positive* |
| "both AIs with most practice doubled the value of a leader out" (`:390-394`) | `leader = 0.0` |
| "fear of being weakest — all three roughly doubled it, one of only four agreements" (`:549-554`) | `strength_deficit = −0.046` against a default of −0.6, i.e. abolished |

And the largest-magnitude term in the live 2p vector is **`end_turn_bias =
−14.44`** (default −3.0) — a pure search-artifact correction.

**This is the single strongest piece of evidence for tonight's working
conclusion (d), and it is measured rather than argued:** a vector in which an
uprising, a civil action and a leader are all worth exactly nothing, while the
biggest term is a correction for a horizon artifact, is a set of correction
terms fitted around a structural defect — not a model of the game. It also
means the guard is doing two things at once: preventing inversions *and*
lobotomising the vector. **Nobody has measured the cumulative cost of thirteen
clamped-to-zero terms.** The one weight where the clamp *was* measured
(`rival_culture`, n=400) came back a tight null, so this is a real open
question and not a foregone conclusion in either direction.

One apparent inconsistency this dig can resolve: `weighted.py:499-501` reports
**69.6% ± 4.5%** and `hillclimb_league.py:144-145` reports **72.5% ± 4.4%** for
what looks like the same measurement. They are different runs and
`WASTED_ACTIONS.md:261-269` distinguishes them: 69.6% is
`analysis/cardvalue_duel.py`'s reference implementation at disc 0.125, 72.5% is
the shipped `hand_potential` in `weighted.py` (which additionally clamps costs
through `max(0, w)`). Both n=400. Neither is wrong; neither says which it is.

## 16. PyPy's own re-test trigger has been met, and nobody re-tested

**RANK 10.** `docs/PYPY.md:211-259` concluded **DO NOT SWITCH** — PyPy lost
every cell (0.77-0.83x). Evidence quality: **6 cells, one rep each, 8s warm /
12s measure, no confidence intervals, on a box under load from three
concurrent climbs.** Directionally believable; by this project's own standard,
**UNPROVEN as a number**, adequate as a verdict at the time.

The doc names its own re-test trigger twice — `PYPY.md:267-270` (*"Re-test
pypy3 if … the bots stop copying a whole `GameState` per candidate move"*) and
again at `:776-777` (*"Re-test PyPy after that lands, not before"*).

**That trigger has been met.** The journal/undo work merged and is live in
training: `experiments/run_league.sh:22` exports `TTA_JOURNAL=1`, and
`grep -c 'journal\.' engine/*.py` finds 72 converted call sites. The whole
premise of the PyPy verdict — a copy-bound bot — no longer describes the
system. **Nobody re-tested.** Cost: minutes
(`python3 -m engine.perf_check bench --kinds weighted` under both interpreters,
with and without `TTA_JOURNAL=1`).

Two related staleness notes in the same file: `PYPY.md:1389` ("Still not
merged, deliberately") and `:1751-1765` ("`run_league.sh` is deliberately NOT
edited on this branch") are both **false at HEAD** — `17c03ea` merged it and
edited that file. A reader of §9.13/§9.16 today gets the wrong picture of what
is running.

## 17. `experiments/summarize.py` bins the four newest features as `"?"`

**RANK 11, and it is a live bug in the reporting path.**

**MEASURED:** `summarize.GROUPS` (`experiments/summarize.py:29-47`) enumerates
feature names **by hand**. Four `BASE_WEIGHTS` keys are missing:
`pact_blocks_attack`, `auction_committed`, `auction_bid`, `hand_potential`.
`group_of()` (`:51-60`) falls through to `return "?"` for exactly these four.

`experiments/analyze_weights.py` imports `GROUPS` and `group_of`, and is the
tool that generates the weight-vocabulary tables for `docs/HEURISTICS.md`,
`docs/HEURISTICS_PROGRESS.md` and `experiments/PROGRESS.md`. **Every published
weight breakdown has therefore been silently binning four features into
`"?"`** — and they are the four most recently added and most interesting ones:
three are the pact/colony features from the `PACTS_DIAGNOSIS` work, and the
fourth is the card-value fix. Cost to fix: four strings.

## 18. `experiments/behaviour.py` has been broken across at least three sessions

**RANK 12, cheapest unblock in the repo.**

`experiments/behaviour.py:541,555` call `all_snaps_iter(recs)`; **there is no
definition of `all_snaps_iter` in the file** (MEASURED).
`analysis/behaviour_run.py:25-30` monkey-patches it in, which is why the
wrapper works and the module does not. `HEURISTICS_PROGRESS.md:17-19` flagged
it; `BEHAVIOUR_AFTER_FIXES.md:16-18` worked around it; nobody has fixed it.
Three lines, copied from the wrapper. It gates every behaviour re-harvest,
including `experiments/roster_behaviour.py` (which has never been run at all).

## 19. The project has no external anchor of any kind, and the one designed remedy has zero lines of implementation

**RANK 17.** `docs/EXTERNAL_AIS.md` opens by naming this exact weakness
(`:8-11`): every strength number in the repo is self-referential.

* §3, "open-source TTA AI projects" — **NEVER STARTED.** Still literally two
  lines reading `TODO — under investigation` (`:242-244`). §7 (`:853-856`) says
  this is *the one finding that could change the recommendation* and that it is
  *cheap to check*; the Next-steps list ranks it **#1** (`:876`). Untouched.
* §6, the human-in-the-loop harness — **DESIGNED IN FULL (186 lines: JSONL
  schema, tiers, sample sizes, a 12-18h costing), NEVER BUILT.** `advisor/` has
  no `--log` flag and no JSONL writer (**MEASURED**, grep over `advisor/*.py`).
  Zero logged games exist. The doc's own §7 calls this the *only externally
  calibrated anchor available* (`:829-833`).
* §5a, BGO — **STARTED AND ABANDONED.** `tools/scrape_bgo.py` exists and
  imports; the journal product was exercised **exactly once**
  (`sources/bgo_journal_7523809.tsv`, n=**1 game**), and the *recommended*
  product — outcome metadata for score-scale calibration, ranked #3 "do it" —
  was never run at all. No BGO metadata exists anywhere in `data/`, `sources/`
  or `analysis/`.

What *was* actioned paid for itself and should be kept as record: §2e (the BGA
source as a rules oracle, which settled the military-count bug), §5c (the two
BGG files, whose cross-check **found a real data bug**), and the negative
results in §1/§2/§4 — no API, no papers, no bot, ToS blocks — which are worth
keeping precisely so the work is not re-done.

`sources/gamefaqs_75690.txt` is still a **58-byte Cloudflare challenge page**,
not content (**MEASURED**), and both `EXTERNAL_AIS.md:889-891` and
`EXPERT_STRATEGY.md:785` already say so. Anything sourced to it in
`EXPERT_STRATEGY.md` rests on search-result snippets, and the document is
honest about that.

## 20. Stale feature counts, rooted in one docstring, propagated to five documents

**RANK 20.** Ground truth (**MEASURED** from the live module):
`len(DEFAULT_WEIGHTS) == 82`, `len(BASE_WEIGHTS) == 62`, `len(PHASE_KEYS) ==
10` (62 + 2x10 = 82).

Six places say otherwise: `engine/bots/weighted.py:3` ("~57 features") and `:6`
("78 weights total") — the root, never updated as `pact_blocks_attack`,
`auction_committed`, `auction_bid` and `hand_potential` were added — plus
`experiments/PROGRESS.md:14,24`, `docs/OPENING_AUDIT.md:57`, and
`docs/PYPY.md:1629-1630`.

`docs/CULTURE_GAP.md` **contradicts itself on this**: line 218 says "25 of the
78 weights", line 765 says "25 of 82 weights" — same fact, two numbers, same
document. `experiments/hillclimb_league.py:98` has it right. Fix the docstring
first, then the five copies.

## 21. Three benchmark scripts point at three deleted checkouts

**RANK 21.** **MEASURED:** `tools/bench_rng_ab.sh:12-13`,
`tools/bench_weighted_ab.sh:25-27` and `tools/weighted_digests.sh:12-19`
hardcode `/Users/pt/tta-ai-mbase`, `-rngonly` and `-journal`. None of the three
exists, and none is a registered worktree. All three parse fine and run to
nothing (`FAILED` rows or empty output). They are the A/B rigs for the
journal/RNG perf work, which has landed; they are also **the only three scripts
in the repo with no inbound reference from any doc, script or test.**
`docs/PYPY.md:898` still instructs the reader to work in
`/Users/pt/tta-ai-journal`. Delete, or parameterise with a `$BASE_DIR`.

Two adjacent tool defects found in the same sweep:

* `tools/bench_interp.py:38` — `(RandomBot if kind == "random" else GreedyBot)`,
  so **`--kinds weighted` silently benchmarks GreedyBot.** A live trap now that
  WeightedBot is the bot that matters. `engine/perf_check.py:99` supports
  `--kinds weighted` properly; prefer it.
* `tools/fsum_patch.py` — obsolete. The `math.fsum` fix it prototyped landed at
  `engine/bots/__init__.py:129-135`.

## 22. Git history is clean — nothing recoverable is buried in it

Recorded so nobody digs here again. All **MEASURED**.

* **Deleted files:** 6 deletions, **zero renames**. Every one is benign.
  `docs/BOT_ROSTER.md` was deleted by `1a6a43c` and **re-added** by `8c94318`
  (which matters: three code comments cite it and would otherwise dangle). The
  rest are superseded intermediates.
* **Dangling objects:** 26 dangling commits from `git fsck`. 23 are
  rebase/cherry-pick duplicates of reachable commits. Of the three unique ones,
  two are identical dropped stashes of the horizon work mid-flight (master is
  **strictly ahead** by direct blob diff) and one is 194k lines of GPG-signing
  probe junk. One dangling 21KB blob is a pre-horizon `weighted.py`. **Nothing
  of value.** `git stash list` is empty.
* **Content-dead branches, safe to prune:** `fix/rate-horizon` (two-dot diff
  against master is **byte-empty**), `diag/culture-gap` (strictly behind
  master), and `coverage-audit` / `quiesce-ab` / `bgo-pilot` (identical to
  master).
* **Actively moving, do not disturb:** `combat-audit`, `probe/horizon-4p`,
  `arch/bot-shape` — all three advanced *during* this dig.
* **One item to check before `combat-audit` merges:** its `tests/test_combat.py`
  (761 lines, 55 tests, runs in 0.051s because positions are built by hand
  rather than simulated) **fails 4 tests against master**, each citing printed
  rulebook text — but the fix commit `33bd156` says it fixed *three*. The likely
  odd one out is `test_a_resignation_does_not_make_a_pact_in_hand_unplayable`,
  because it lives in an `actions.py` legality gate rather than an `effects.py`
  rule — i.e. it is item 11 of this shortlist, found independently. Worth
  confirming it is not dropped.

---

# PART II — PER-FILE LEDGER

One entry per document, largest first. Each records what was claimed, its real
status, the evidence quality, and whether it is still worth having.

## `docs/AGGRESSION_FIX.md` (137 lines)

**Truncated mid-sentence. The headline fix was never written.**

* §A, "4-player colony auctions: hypothesis REFUTED" — **PROVEN as a
  refutation, UNPROVEN as a measurement.** 8 mirror 4p games (lines 25-43),
  n=8. The refutation itself does not need n: the mechanism is exact and was
  read out of the code (`interact.py:508-519` builds the bidder list from
  `max_force > 0`, which is 0 when the unit pool is empty; §11.3 requires
  sacrificing at least one unit). The 2p and 4p champions own **0.00 / 0.07**
  military units per player, so they are excluded from every auction at the
  door. Verdict "not an engine bug, the 4p champion's army is the blocker" is
  correct and still true. Still worth having: **yes** — it kills a plausible
  wrong theory (bad event seeding) permanently.
* §B, "Aggressions and wars: confirmed" — **PROVEN as a diagnosis.** 6 mirror
  games per count instrumenting every politics decision, plus a direct probe
  over **357 sampled positions** in which the best attack scored below
  `pol_pass` every single time. That is a structural claim with a large sample
  behind it.
* §B's fix — **ABANDONED.** See shortlist item 1. Line 137 is the last line of
  the file.
* One correction the doc makes that is worth preserving
  (`AGGRESSION_FIX.md:105-108`): the behaviour harness's 0.00/0.11 aggression
  numbers count **one champion seat**, not the table, so the 2p/3p zeroes are
  the baseline rate seen through a smaller window, **not a regression**. This
  is exactly the kind of measurement artifact that this project has been bitten
  by repeatedly.

## `docs/DEEPER_SEARCH.md` (164 lines) — *owned by another agent, read-only*

* §1-2, the design argument — **finished, and it is the clearest statement in
  the repo of the 1-ply defect.** The table at lines 21-27 (move / what `apply`
  leaves in the trial state / where the payoff actually is) is the single most
  reusable artifact in this document.
* §3, cost — **MEASURED, finished.** 1.16x/1.29x/1.18x, with the mechanism
  (only 2.7-4.1% of candidates leave a decision hanging). Truncation 0% / 1.9%
  / 0%.
* §4/§5/§6 — **`RESULTS PENDING`, ABANDONED.** Being run now by another agent.
* One claim that is **STALE**: `DEEPER_SEARCH.md:104-117` says the
  `journal-undo` branch has the machinery but *"no call site converted"* and
  that converting ~470 sites is the whole risk. That was overtaken — the
  journal work landed (`journal step 5a`…`5f`, `a332d0c` "61/61 converted sites
  proven executed", `ae20f2b` "final gate is 10/10 on the finished tree",
  `17c03ea` "run_league: take the journal path (1.44x on WeightedBot)").
  **VERIFIED:** `engine/journal.py` exists on master and `grep -c 'journal\.'
  engine/*.py` returns 72 call sites. The paragraph should be read as history,
  not as current state.

## `docs/PACTS_DIAGNOSIS.md` (348 lines)

**Status: COMPLETE and largely PROVEN. The strongest diagnostic document in the
repo, and the one whose recommendations were most followed.**

* Verdict "bot blind spot, not an engine bug" — **PROVEN**, by three
  independent routes: move-legality instrumentation (pacts legal in 16% of
  politics decisions, chosen 0 times), a feature-vector diff showing exactly two
  features move and both downward (`hand_military 6→5`, `hand_mil_value 21→17`,
  weighted delta −1.10), and a rules read against `RULES_SPEC` that found no
  defect. The feature diff is *exact arithmetic*, not a sample, so n does not
  apply.
* Colony causes A/B/C — **PROVEN for A and B** (bids evaluate byte-identically
  to passing while rivals remain, directly observed at lines 188-193; the
  visible single-bidder case still loses on `workers`/`unit_workers`).
  **Superseded for C**: `AGGRESSION_FIX.md` §A later refuted the "4p auctions
  never start because events aren't seeded" story and replaced it with "the 4p
  champion owns no units". Both documents are on master; only `AGGRESSION_FIX`
  says which one won. Worth a cross-reference.
* "The smoking gun: these weights were never under selection" — the 3p
  `colonies` weight was **bit-for-bit the hand-written default** after thousands
  of generations. **PROVEN and important**; it is the cleanest demonstration in
  the repo that an unselected weight can look trained.
* Third finding, `WeightedBot` scoring the wrong seat — **PROVEN and FIXED**
  (`6376981`). **VERIFIED:** `engine/bots/weighted.py:799` and
  `quiescent.py:262` both now use `state.decider()`.
* Fix #4, "ablate the 4p `hand_military` weight (0.908)" — **never done.**
* Fix #3, "verify the accept branch isn't being systematically refused" —
  **never done.**

## `docs/BEHAVIOUR_AFTER_FIXES.md` (65 lines)

**Status: finished, honest, PROVEN.** 240 games (80 per count), mirror
self-play, 0 engine errors, seed recorded, commands recorded. Pacts 0.00 →
1.80/3.21 at 3p/4p; colony bids 0.08 → 2.17 at 3p; **wars 0.00 → 0.00,
aggressions unmoved**. The verdict "partly worked" is correctly hedged and the
baseline's aggression range (0.03-0.11) is explicitly called out as a range
rather than a per-count number.

One operational fact worth keeping: `experiments/behaviour.py` "is still broken
standalone, see its module docstring" and the working wrapper is
`analysis/behaviour_run.py`. **VERIFIED** both files exist at `8e751cb`.

## `docs/WASTED_ACTIONS.md` (498 lines)

**Status: the best-evidenced document in the repo. PROVEN, replicated, with a
control arm and a negative result reported in full.**

* The core finding (98.4% of wasted-action turns had an affordable legal move;
  60.1% declined a move the bot's *own* evaluation scored positive) was
  **independently re-measured after the `7d40f53` deck change** and reproduced
  to within a percent (3553 vs 3557 turns, 59.9% vs 60.1%, +12.41 vs +12.57
  flattery). Replication is exactly what the rest of this repo lacks.
* The `hand_potential` fix — **PROVEN at 2p** (72.5% ± 4.4%, n=400, p<1e-5),
  with a byte-identical `disc = 0.0` control returning exactly 50.0%. That
  control is the reason this result should be trusted.
* **Explicitly NOT proven elsewhere**, and the document says so: 3p is not
  significant (35.8% ± 4.7% against a 33.3% null, n=399) and 4p regressed to
  9.7% ± 2.7% before the cost clamp. The 4p `hand_potential: 0.0` override in
  the live run descends directly from this.
* The five-way negative result on removing the `end_turn` artifact — see
  shortlist item 4. **PROVEN**, and the single most important thing in the file.
* §11's three pre-retraining measurements — (a) and (b) **ABANDONED**, see
  shortlist item 10b.
* Measurement-environment note at lines 382-394 is a model of its kind: it
  records that another agent added a `has_unit` feature *during* the duels and
  argues why the mirror-match design makes it harmless. Cross-reference: the
  `has-unit` branch still exists unmerged on origin.

## `docs/STRENGTH_CHECK.md` (308 lines)

**Status: substantively COMPLETE; its own status header is STALE.**

Line 3 reads *"Status: 2p and 3p final. 4p and the hybrid ablation still
running"*, but the 4p row (64.3%, ±5.4%, n=300 — now **quarantined**, see
`analysis/frozen/README.md`) and the `BookImprovedBot`
hybrid null (50.8% ± 5.7%, p=0.77, n=300) are both in the file, and commit
`33d0ff1` is titled "Strength check: 4p result, the hybrid null, two tournament
diagnostics". The header was never updated. Harmless, but it is the kind of
thing that makes a reader distrust the body.

* BookBot beats the champion at every count — **PROVEN** (n=400/300/300, all
  well outside their intervals), with GreedyBot rows as the control that makes
  the result interpretable rather than alarming.
* `BookImprovedBot` p=0.77 — **the most informative negative result in the
  repo.** Patching the champion's four most-implicated move kinds with book
  logic changes nothing, which is what licenses "the whole plan is wrong, not
  four local habits".
* BookBot v2 vs v1: +2.1% at 2p (p=0.098, n=1600) and *negative* at 3p
  (p=0.31, n=600). **Correctly reported as a non-result** — swapping
  opinion-derived card ranks for 39 games of championship revealed preference
  moved almost nothing. This is a real, deflating finding about how much card
  valuation matters relative to the priority list, and it deserves to be
  remembered before anyone invests in card tier work again.
* The two tournament cross-checks (`pickstats.py`, 30 games, 1176 picks) —
  **UNPROVEN, n=30 games.** The Theology result (0.47/game against a tournament
  0.00 in 39 games) and Frugality (29 of 30 games) are large enough effects to
  survive the small n, but the "20.2 picks per player per game is suspicious"
  claim is explicitly flagged by the doc as suspicious-not-proven, and should
  stay that way.
* "Re-run once a post-`7d40f53` champion exists" — **ABANDONED**, shortlist 10a.

## `docs/OPENING_AUDIT.md` (464 lines)

**Status: COMPLETE, PROVEN, and the most epistemically valuable document in the
repo. Its follow-ups were not done.**

* Verdict "undertrained noise — a single weight, flipped by accident at
  generation 5 of 138, never revisited" — **PROVEN** by four converging routes:
  seat-for-seat re-measurement (n=400/seat), a cross-play matrix showing the 4p
  vector opens wonder-first at 2p and 3p too, an untrained control identical to
  the decimal at all three counts, and a direct single-weight revert that
  removes the behaviour entirely (74% → 0%).
* The A/B: 0.276 ± 0.063 against a 0.25 null, n=192. **The doc reports a caveat
  against its own conclusion** (against the untrained bot, 0.792 vs 0.641, ~2.5
  standard errors the other way) and refuses to hide it. That is the standard
  the rest of the repo should be held to.
* §"What this implies about every other weight we quote" — see shortlist item
  3. This generalisation is the highest-value paragraph in `docs/`.
* §5's correction of `baselines.jsonl` — **PROVEN** (the 4p gap is 44 points
  and the champion's mean culture nearly doubled while the opponent's did not
  move; seed noise moves both together). Seed noise is *also* real and larger
  than `HEURISTICS.md` claimed: a 16-point swing from the seed alone at 2p.
* Follow-ups 2 (`baselines.jsonl` metadata) and 3 (`opening_order.py`) —
  shortlist items 8 and 9. Follow-up 4 (4p abandoned wonders) never done.
* One loose thread nobody picked up (`OPENING_AUDIT.md:312-315`): in the mirror
  head-to-head both bots score ~55 mean culture against 200-260 when either
  plays the default bot. *"Two strong, nearly identical bots at the same table
  appear to strangle each other's scoring."* Flagged as "not investigated
  here" and still not investigated. It matters, because mirror scores are an
  input to the league's `mirror` tier.

## `docs/LEAGUE_TRAINING.md` (868 lines)

**Status: built and running; its own status header is STALE and one of its
measured findings was shipped against.**

* Line 3: *"Status: built, smoke-tested end to end at 2p, 3p and 4p, NOT yet
  launched."* **CONTRADICTED by `docs/TRAINING_RUN.md`**, which records the
  launch at 16:29 and the relaunch at 18:41. Never updated.
* The design rationale (candidate must beat a pool it did not produce), the
  tier weighting, the paired-edge statistic with a null of exactly 0, the
  rotating acceptance subset with a guaranteed gate opponent, and the
  full-pool re-check with an `untested_regression` alarm — **all finished, and
  the reasoning is sound and explicit.**
* `MARGIN_SCALE = 120` — **MEASURED, not guessed**, and the justification (2.5x
  the per-game margin sd, keeping the operating band inside `|m/scale| <~ 1.8`)
  is exactly right. The gate-gradient proof at lines 704-734 is a good piece of
  work: at 4p, **27 of 32 gate rows are dead on win share and 0 of 32 on
  margin**, and margin is also the lower-variance estimator.
* "The variant tier was one opponent, not seven" — **the single finding that
  would have wasted the whole run.** Every roster class inherited `BookBot`'s
  `name = "book"`, so `acceptance_subset`'s de-duplication by label meant at
  most one variant could ever enter a generation's decision. Fixed. Worth
  keeping as a lesson about labels being identities.
* The sabotage-that-isn't finding — shortlist item 5. **CONTRADICTED but
  shipped.**
* `--check-games` too small — shortlist item 12f.
* Smoke-run ablation numbers (n=24) are **UNPROVEN** and the doc says so
  ("At n=24 these are weak claims"). Good.

## `docs/CULTURE_GAP.md` (824 lines) — *owned by another agent, read-only*

Tonight's work. Recorded here only for cross-reference and for one
contradiction it resolves.

* §0 opens by noting the brief it was given was **one fullcheck stale** — a
  0/48 shutout that was real when measured and no longer the state of the arm.
  That is the third instance in this repo of a conclusion built on a stale
  baseline, and it is exactly the failure mode this dig was asked to hunt.
* §2b independently re-derives the `AGGRESSION_FIX.md` truncation. Two
  independent readers, same conclusion, still unfixed.
* §4's counterfactuals are all **UNPROVEN at n=48 and are labelled as such** —
  the doc's own error-bar paragraph at lines 14-16 is unusually good practice.
  §9 later replaces one of them with an n=400 answer (a tight null, ±0.024 win
  rate), which is the right way to close an n=48 result.
* §8f is the finding that matters most for planning: the horizon fix
  **invalidates the 3p and 4p champions** (13.9% vs 33.3%; 20.1% vs 25.0%;
  n=400), with matched controls at §8e proving it is the fix and not generic
  perturbation. That is why `experiments/archive_prehorizon/` exists.
* §8b(i) records a hypothesis that failed with exact arithmetic behind it (the
  unclamped-`L` sign flip explains none of the regression, to three significant
  figures). Recorded specifically so nobody reaches for it again. Keep.

## `docs/EXPERT_STRATEGY.md` (785 lines)

**Status: FINISHED as research; PARTIALLY APPLIED. Nothing here is measured
against our engine, and the document says so in its first line.**

This is the only external check in the repo and it is genuinely well sourced —
provenance caveats are stated up front (BGG needs a text proxy; GameFAQs is
behind Cloudflare so its content is search-snippets only; `sources/gamefaqs_75690.txt`
is **just a Cloudflare interstitial, not content**; Reddit material is confined
to Appendix B and marked).

* The base-game filter (6 leaders, 4 wonders per age) with an explicit
  do-not-code expansion exclusion list — **applied and load-bearing.** This is
  the guard that keeps a 2015-base-game project from ingesting expansion tier
  lists.
* **Applied:** the tournament tier ordering, the never-take list (Theology,
  Stock Pile, Patriotism, Cultural Heritage, Frugality), the conditional leader
  rules and `V2_TUNABLES` for genuine expert splits are all in
  `engine/bots/book.py` (**VERIFIED**: `V2_NEVER_TAKE` at line 1004,
  `V2_TUNABLES` at 943, `mil_target`/`res_need`/`food_need` in the context
  object at 126-167).
* **NOT applied — the quantitative benchmarks.** The "Consolidated priority
  list (codable)" (lines 590-632) and §10's mistake list contain a large number
  of hard, checkable thresholds that appear nowhere in the bots: the corruption
  rule (**blue bank >= 11 tokens, i.e. committed food+resources+wonder stages
  <= 5**), the absolute strength floors by age (~10 / 15-25 / ~30), the science
  targets (4/turn end of Age I; 10+/turn in Age III), the MA tiering
  (3 MA = draw 3 + deter; 4+ = targeted aggression; 5+ = full warfare), the
  aggression threshold (a 4-5 strength lead to be worth playing), and the
  CA targets by age (4 / 5 / 6.5 / 8). `book.py` encodes softer versions
  (`res_need = (3 + age) - resources`, `food_need = consumption + 2 - food`,
  `mil_target` = second-strongest rival or leader−3) but not these.
* **Worth having: yes, high, and specifically the thresholds.** They are the
  only source of *absolute* targets in a project whose every other number is
  relative to itself. A `var:expert` pool bot built from the consolidated
  priority list would be the cheapest new gate opponent available, and the
  roster already shows rule-based bots cost ~1-6s against the champion's 25-50s
  (`BOT_ROSTER.md:105-117`).
* One deliberate non-finding to preserve: §"Biggest open disagreements" refuses
  to resolve 17 genuine expert splits and says *parameterize these, do not
  hard-code them*. `V2_TUNABLES` honours that for six of them. The rest are
  unparameterised.

## `docs/BOT_ROSTER.md` (182 lines)

**Status: WORKING NOTES by design, PROVEN, deliberately not polished.**

47,520 games, n=240 per cell, seed-paired, seat-rotated, zero engine errors,
with an accidental 12-pairing re-measurement that returned **bit-identical**
win rates. The caveats section is careful in the two places it needs to be
(Wilson intervals substituted for shutout cells; the 3p/4p lower triangle
deliberately blank because the reciprocal of "one Culture vs three Infra" is
not "one Infra vs three Culture").

Headline results still load-bearing today: **CultureBot is the real gate**
(1.58/1.62/1.67x par), **MilitaryBot scales hard with table size** (1.14x at 2p
→ 1.71x at 4p), **the trained champion is 10th of 12 at 2p** and is not a gate.
The cost table is the reason the league pool is cheap.

Unfinished, both stated: the reverse 3p/4p direction, and
`experiments/roster_behaviour.py` — **written, committed, never executed.**

## `docs/BRANCH_AUDIT.md` (131 lines)

**Status: COMPLETE, and the remediation it prescribed was carried out.**

The audit's core fact — that `deeper-search` and `origin/master` were *neither*
a superset of the other, and a naive diff-based apply would have deleted all
785 lines of `docs/EXPERT_STRATEGY.md` — is the kind of finding that saves a
day. Method is sound (patch-id bucketing, then blob-hash verification because
patch-id over-reports on squashes).

**VERIFIED at `8e751cb`:** the six quiescence commits landed
(`engine/bots/quiescent.py`, `tools/quiesce_bench.py`,
`tools/no_credit_check.py`, `tools/behaviour_counts.py` all present; the
`quiesce:` prefix is in `experiments/arena.py`). Neither `deeper-search` nor
`land-quiescent` appears in `git branch -a`, and `/private/tmp/deeper_wt` is
not in `git worktree list`. The document is now **history, correctly closed.**

Keep it for the process lesson in "How the divergence happened": an agent ran
`git checkout -b` in the *shared* working tree and every subsequent agent
committed to the wrong ref without noticing. That failure mode is one shared
checkout away from recurring; the current practice of one worktree per agent is
the fix, and this document is why.

## `docs/TRAINING_RUN.md` (102 lines)

**Status: terse operational note, accurate when written, now PARTIALLY STALE.**

It documents three live supervisors (2p/3p/4p, PIDs 26277/26278/26279, relaunched
at 18:41 on the journal engine). What actually happened since, and is recorded
nowhere: the 3p and 4p arms were **wiped and restarted from `DEFAULT_WEIGHTS`
at 21:25**, per `CULTURE_GAP.md` §10's recommendation, with the pre-restart
state preserved in `experiments/archive_prehorizon/`.
**MEASURED:** `experiments/league_state/generations_{3,4}p.jsonl` both begin at
`{"gen": 1, ..., "at": "2026-07-26 21:25:21"}`, and
`experiments/league_state/` holds `weight_credit_2p.json` and
`ablation_2p.jsonl` only, while `experiments/archive_prehorizon/` holds the 3p
and 4p equivalents. So the 2p arm is mature (gen ~340) and the 3p/4p arms are
a few mutations from default.

`experiments/archive_prehorizon/` itself is a **correct, deliberate,
self-consistent snapshot** — 16 files plus `ladder_{3,4}p/`, taken after the
horizon commit, containing the complete 3p/4p training state at the moment the
horizon fix invalidated it. Not lost work. Leave it alone.

Three things in it are worth keeping regardless and should not be lost when it
is updated:

1. **`--init` is ignored once the state dir already has a champion.** So
  "restart with `--init default`" does *not* reset a run, it resumes. To start
  clean you must move `experiments/league_state/` aside first. This is
  documented as a gotcha that once worked in our favour.
2. `experiments/champion_4p.json` (the old top-level file, **not** the one in
  `league_state/`) holds the degenerate `science = −6.089` vector. Never
  warm-start from it.
3. "Do not commit": the trainer constantly rewrites `experiments/champion_*.json`,
  `experiments/league_state/**` and `experiments/league_*p/`. Never `git add -A`
  while a run is live.

## `docs/ARCHITECTURE.md` (43 lines)

**Status: still broadly accurate, one claim overstated, one stage understated.**

* "Speed target: >=50 full 4p games/minute for RandomBot" — **met with room**;
  `engine/PROGRESS.md:96` records ~330 4p games/minute.
* "`tests/`: rules unit tests keyed to RULES_SPEC sections" — **partially
  true.** `tests/test_engine.py` has 58 tests and cites 16 distinct RULES_SPEC
  section numbers (§1.2, §1.9, §2.3, §2.5, §3.11, §5.8, §5.9, §6, §6.1, §6.4,
  §11, §11.3, §11.4, §11.5, §12.2.4, §12.3) out of 13 top-level sections — so
  the keying is real but sparse. Whole areas (§5.4 aggression resolution, §5.6
  war legality, §13 two-player) have no keyed test; a combat audit adding
  exactly those is in flight on another branch.
* Stage 6, "Advisor" — **finished and understated.** `advisor/` is complete and
  working with 49 green tests and its own README containing a captured
  transcript of a real advised turn. It is arguably the only user-facing
  deliverable in the repo and no document in `docs/` mentions its status.
  `advisor/PROGRESS.md:27-33` lists three follow-ups, none done: deeper search
  than 1 ply for the recommendation (which it notes "matters most in the last
  round" — the same horizon defect the rest of the repo is chasing), tracking
  rival hand contents when a take is public, and a `--log` flag.

## `docs/OPEN_QUESTIONS.md` (50 lines)

**Status: genuinely current and genuinely closed — not abandoned.** Checked
because a 50-line file marked "WORKING LIST" is a natural suspect.

All 20 items are struck through with a resolution, a date and a source. The
two items the user personally ruled on (19: `resourcesForMilitaryUnits` is a
**total pool**, not a per-unit discount; 20: an action card's **ordered action
resolves first at full price**, gains land after) are both recorded with the
engine change and the pinning tests (`test_gains_land_after_the_ordered_action`,
`test_frugality_food_lands_after_the_population_increase`).

**One inconsistency worth flagging:** `engine/PROGRESS.md:118-120` still lists
as a "Known gap / deliberate simplification" the **assumption** that an action
card's gains resolve *before* its ordered action — the opposite of what
OPEN_QUESTIONS item 20 records as resolved and implemented. One of the two is
stale; the tests say `OPEN_QUESTIONS` is right and `engine/PROGRESS.md` is the
stale one. Same for the `resourcesForMilitaryUnits` entry directly above it,
which says "not confirmed by the rulebook" when item 19 records the user's
ruling.

## `docs/RULES_SPEC.md` (332 lines)

**Status: COMPLETE.** Every section ends with `<!-- SECTION COMPLETE -->`, every
claim carries a page citation to one of the Handbook, the Code of Laws, or FAQ
v15. This is the foundation the rest of the repo cites and it holds up. No lost
work found in it.

The one thing it makes visible by contrast: the engine has two acknowledged
deviations from it that are not tracked here — the greedy colonization
sacrifice (§11.3 gives the winner the choice) and the aggression legality
filter that refuses to emit an attack the attacker would lose (the rulebook
permits a doomed aggression; `CULTURE_GAP.md:268-272` notes this is a
conservative deviation that cannot explain anything, which is right, but it does
mean every "legal aggression" count in this repo is a count of *winnable* ones).

## `docs/HEURISTICS.md` / `HEURISTICS_PROGRESS.md` / `HEURISTICS_TODO.md`

See Part III.

## `docs/PYPY.md` / `EXTERNAL_AIS.md` / `SOURCES.md`

See Part III.

---

# PART III — THE LARGE DOCS, AND THE TOOLING

## `docs/HEURISTICS.md` (2430 lines)

**Status: STALE, comprehensively — see shortlist item 15.** It describes three
champions that no longer exist, and its headline advice is contradicted by the
one surviving champion's own weight vector.

Beyond that, two things about its *evidence* need recording.

**The one section it tells you to trust is n=12.** `HEURISTICS.md:78-139`
("What the measurements actually confirm") is presented as the sole grade-1
section, to be believed over everything else in the file. It has two distinct
evidence bases and merges them:

| claim | real n | verdict |
|---|---|---|
| BookBot beats champion_2p 62.9% ± 4.7% | **n=400**, seat-rotated, paired seeds | **PROVEN** — for the win rate only |
| The five corrections (workers −4.5, food −2.6, CA −1.6, wonders −0.8, science +14.6, culture crossover ~r15) | **n=12 paired 2p games** (`STRENGTH_CHECK.md:88`) | **UNPROVEN.** No CIs. Never replicated. |
| "+6.6 strength at 3p and still loses" | n=12 at 3p | **UNPROVEN** |

The *direction* (BookBot wins) is solid at n=400. The *diagnosis* — that it
wins because of workers, food, civil actions and banked science — is a
twelve-game story with no error bars, and the rest of the document was
restructured around it. `experiments/book_diag.py` exists and is cheap;
re-running it at n>=200 is one of the highest value-per-minute items available.

**It is also one tier behind the best known opponent.** `HEURISTICS.md`'s whole
"believe the book bot" framing (`:141-179`) predates `docs/BOT_ROSTER.md`, which
measured (n=240/cell, 47,520 games) that **CultureBot sits at 1.58/1.62/1.67x
par against BookBot's ~1.2x** and beats the champion 85/15 at 2p.
`HEURISTICS.md` never mentions CultureBot, MilitaryBot or the roster at all.

**Two of its four structural examples are now factually wrong.** Caveat 3
(`:305-324`), rule 8 (`:478-500`) and "The one misreading this document must
not cause" (`:2339-2399`) all state as present-tense fact that the bot cannot
offer a pact, bid on a colony, attack, or declare war. Pacts and colonies were
**fixed** (`166867d`; pacts 0.00 → 1.80/3.21 per game, colony bids 0.08 → 2.17
at 3p, n=240). War and aggression are **still true**. The worked `−1.10445`
pact example at `:2358` is a description of code that no longer exists. This
needs a surgical edit, not a rewrite: the argument is right, two of its four
instances are stale.

**The `end_turn` section is backwards.** `HEURISTICS.md:1444-1485` frames the
horizon artifact as a bug and tells the reader *"You are right and the AI is
wrong."* `engine/bots/weighted.py:687-707` now carries a 15-line **"DO NOT
'FIX' THIS"** comment with four n=400 measurements behind it. See shortlist
item 4. The code's own explanation — that the flattery acts as a move-quality
filter — is **INFERRED, not measured**, and is itself a load-bearing but
untested rationalisation.

**Card-blindness was fixed and the document still asserts the defect.**
`:1472-1480` and `:1598-1605` say two different cards produce identical inputs,
and downgrade the per-card priority lists to "weak hint" *because of it*.
`weighted.py:484-601` (`_card_yields`, `card_potential`, `hand_potential`)
fixed it, measured at 2p with a byte-identical null control. So
`HEURISTICS_TODO.md:113-116` ("re-harvest the priority lists once
card-blindness is fixed") is **unblocked and untouched**.

## `docs/HEURISTICS_PROGRESS.md` (319) and `docs/HEURISTICS_TODO.md` (135)

**Status: accurate as a record; most items ABANDONED.**

| item | status |
|---|---|
| Units 1-5 (de-jargon, build order, priority lists, four questions, rule 8) | **DONE**, verified present |
| Re-run the 4p build order at 60 games | **UNTOUCHED.** `analysis/` holds only `out_opening_2p.txt` and `out_opening_3p.txt`; the 4p build order at `HEURISTICS.md:657-701` still rests on **n=20**, and now describes a deleted champion |
| Re-run the book-bot benchmark against a post-`7d40f53` champion | **PARTIALLY DONE by another route** — `BOT_ROSTER.md` re-measured on current code at n=240/cell and reproduced the verdict. Nobody closed the TODO or updated the caveat at `HEURISTICS.md:43-49` |
| Reconcile the two 4p colony measurements | **UNTOUCHED** — blocked on `AGGRESSION_FIX.md`, which trails off mid-fix. Both underlying measurements are small-n, so it is **UNPROVEN either way** and cheap to settle |
| Re-harvest the priority lists once card-blindness is fixed | **UNBLOCKED AND UNTOUCHED** |
| "Nothing tested against a human opponent" | still true — see shortlist item 19 |
| `analysis/opening_order.py` is broken | recorded as fixed; **half-fixed in fact**, see shortlist item 9 |
| `experiments/behaviour.py` is broken (`all_snaps_iter`) | **UNTOUCHED, STILL BROKEN** — shortlist item 18 |
| Refresh checklist (behaviour / leak_check / analyze_weights / header) | **UNTOUCHED**, and unrunnable as written: `experiments/analyze_weights.py:51` defaults to `experiments/champion_%dp.json`, no longer the live champion |
| "Never `git add -A`" | **still true and more so** — `experiments/league_state/` is rewritten every few seconds |

One item worth preserving verbatim from `HEURISTICS_PROGRESS.md:131-138`: the
note that **`food_rate` is GROSS, not net**, and that starvation costs 4 culture
per missing food. It ends *"Any future edit that treats `food_rate` as net is
WRONG."* That is exactly the kind of trap-marker that should outlive the
document it is in. (Not re-derived here — verified by citation only, and the
cited files were edited at 18:41, so a two-minute re-check is warranted before
anyone re-uses the starvation arithmetic.)

## `docs/PYPY.md` (1788 lines)

**Status: a genuinely good working log. Most of it is PROVEN and landed; its
headline verdict is STALE; three of its artifacts have rotted.**

**Landed and still true:**

* The `sum()` vs `math.fsum` cross-interpreter divergence
  (`PYPY.md:93-162`) — **PROVEN, and unusually well done for this repo**: root
  cause bisected to a single move with `tools/trace_game.py`, fix verified by
  135 games byte-identical on both interpreters, then **re-verified after a
  rules change**. Landed at `engine/bots/__init__.py:129-135`.
  * Undocumented corollary, still true: `WeightedBot.evaluate`
    (`weighted.py:735-740`) uses a hand-rolled `total += wk*v` loop, not
    `fsum`. Still deterministic across interpreters (naive left-to-right is the
    same everywhere), so no bug — but only GreedyBot's evaluation is
    exactly-rounded. Worth one sentence somewhere.
* The mutation measurement (`PYPY.md:360-405`) — **PROVEN**, n=9235 candidate
  moves, per-move-kind breakdown, and it made a correct *large* prediction
  (~1.8x projected, 1.75x measured). **This is the template for how this
  project should justify work before doing it.**
* The journal design, GO/NO-GO, and throughput measurement (§6, §9) —
  **FINISHED AND EXECUTED.** All five GO conditions met plus two the design did
  not ask for. 1.65x/1.75x greedy, 1.40x/1.44x weighted, measured with three
  alternating rounds and two metrics agreeing to three digits, with the
  per-run spread reported honestly. **The only correct measurement protocol for
  this box in the whole repo.**
* The seat census (§9.14) — **PROVEN, and the most valuable self-correction in
  the file.** Replaying `Pool.acceptance_subset` over 200 generations against
  the real pool found the league is ~69% WeightedBot / ~27% Book+Variant /
  ~2% Greedy / **0% Quiescent**, with the explicit consequence that *no digest
  in this project can catch a change to `WeightedBot`* — the trap that has
  since re-sprung (shortlist item 6).

**Superseded or refuted, and worth knowing so nobody re-cites it:**

* The leaf-class fast path (1.55x, `:329-344`) — **GONE**, replaced by
  exec-generated per-class copiers (`fastcopy.py:268,289`). The number
  describes no code in the tree.
* `random.Random(0)` at 10.8-13.6% of runtime (`:455,:788`) — **refuted** by
  §8.1, over-attributed by ~2x. Do not cite.
* The `setstate` fix (§5a) — **A/B'd at ~1.00x and abandoned.** What shipped is
  the lazy reseed (`engine/bots/trial.py:45-75`), ~1.07x on greedy and below
  the noise floor on WeightedBot — claimed only from direct cost×count
  accounting, which the doc states honestly.
* End-to-end fastcopy numbers (`:415-427`) — **STALE**, measured against a
  worktree that no longer exists, on a code path that is no longer the training
  path.

**The one thing that should be promoted out of this file.**
`PYPY.md:888-892`, restated at `:1663-1667`, established across three data
points:

> *On this box, a profiler line under ~10% cannot be confirmed by an end-to-end
> A/B. Either measure it directly (cost × count) or accept it unmeasured.*

That is the only rule in the repo that would have prevented two of its own
reversals, and it is buried at line 888 of a 1789-line working log. It belongs
in a methods note.

**Open, never started:** function-level imports in `features()`
(`engine/bots/__init__.py:87-88`, claimed 1.6-2.2% — note the rule above says
this cannot be settled by A/B); `effects.compute` (12-13%) and
`evaluate`/`features` (18.9-24%) as next targets; and the structural one —
**`QuiescentBot` cannot use the journal** (`quiescent.py:174,209,277` still
call `copy_state`; `trial.py:20-25` says it must never honour `USE_JOURNAL`
because `journal.begin` raises on nesting). It needs a stacked journal or
design B. Currently free, since it is 0% of league seats — but that is
circular: the bot without the structural flaw is untrained *and* now ~1.4x
slower than the bot with it.

## `docs/EXTERNAL_AIS.md` (894 lines)

**Status: ~90% careful research that was never actioned; the ~10% that was
actioned paid for itself.** Detail in shortlist item 19.

Worth keeping as record, do not re-do: §1 (the CGE app has no API, no log
export, no mod hooks), §2a-d (BGA has 1.19M games, no bot, and a ToS that
forbids scraping — correctly not actioned), §4 (there are no TTA papers; the
adjacent work is TAG/RFTG/Dominion; TD-learning is the recommended upgrade
path). Negative results that stop work being re-done are worth their storage.

The §4 recommendation is the one strategic thread nobody has pulled:
`WeightedBot` is still a **linear** evaluator (`weighted.py:774`), with no
nonlinear head and no TD code anywhere in the repo. Given that four separate
findings in this ledger are variants of "a linear evaluator cannot express this
step function" — the colonization unit cliff (item 13), `strength_lead` capped
at 6 with a weight of 6.392 that no single step can earn (`CULTURE_GAP.md:368-374`),
the age-bucket horizon (`e990920`), and card identity (`hand_potential`) — that
recommendation deserves more than a TODO.

## `docs/SOURCES.md` (359 lines)

**Status: the most fully-actioned document in the set. PROVEN and APPLIED.**

* Card-data provenance and edition filtering — applied across
  `data/cards_*.json` and gated by `data/validate_cards.py` (179 civil / 150
  military anchors).
* The BGG third-opinion conflicts — **PROVEN, and correctly resulted in NO
  change.** Two BGG files split; the 179-card component count and the
  rulebook's 6-and-3 removal pattern settle it. Good adversarial reasoning: the
  right outcome of a cross-check is often "we were already right".
* **The military-count bug** — **PROVEN 4-0 and APPLIED** at `7d40f53`.
  Verified in-tree: `data/cards_military_actions.json:48-54` now has Fighting
  Band at 2/2. Age I tactics 5 → 10, aggressions 11 → 6, totals unchanged.
* **Its consequence was not actioned.** `SOURCES.md:308-311`: *"This
  invalidates comparability of hill-climb generations run before this commit …
  champions and league tables either side of `7d40f53` cannot be compared …
  restarting them is the user's call."* Whether the pre-`7d40f53` generations
  were ever discarded **cannot be determined from the repo** — the generation
  JSONLs are untracked working artifacts with no commit provenance and no
  boundary marker. Stated plainly rather than guessed. The same is true of
  whether `experiments/champion_*.json` were retrained after the `6376981`
  decider fix. Both are live "a result that will later reverse" risks.
* One booby trap left in place: `sources/bga_card_counts.tsv` — **our own
  derived extract** — is still uncorrected (`SOURCES.md:264-270`) and still
  contradicts the `.php` it claims to be extracted from. It is cited by
  `docs/EXPERT_STRATEGY.md` as an authoritative source. Ten-minute fix, never
  done.

## Tooling census

Verified by parse (`ast.parse` on 34 Python scripts, `bash -n` on 6 shell
scripts — **all 40 clean**) and by import (**all 32 importable modules import
clean against current master** — so nothing is broken at the *symbol* level by
`e990920`). Nothing was run that simulates games, so throughput and correctness
claims below are marked UNVERIFIED where they depend on execution.

**NEVER RUN.** `experiments/roster_behaviour.py` — the precedent, confirmed. It
declares `--out experiments/roster_behaviour.jsonl` (line 117) and **that file
has never existed anywhere in the repo or in git history**. Single commit
`b68e313`, referenced only from `docs/BOT_ROSTER.md`. It imports clean, so it
is runnable — just never run. It is the missing behavioural half of a
deliberately deferred deliverable (`1a6a43c`: *"prose write-up deferred until a
bot has finished hill climbing"*), and running it would settle the 4p colony
disagreement as a side effect.

**BROKEN — dead checkout references.** `tools/bench_rng_ab.sh`,
`tools/bench_weighted_ab.sh`, `tools/weighted_digests.sh` — shortlist item 21.

**BROKEN SEMANTICALLY — pre-horizon champion defaults.**
`tools/quiesce_bench.py`, `tools/no_credit_check.py`,
`tools/behaviour_counts.py` — shortlist item 14.

**RUN AND USED** (named in a doc *and* has surviving output artifacts):
`experiments/gate_gradient_proof.py` and `margin_calib.py` (all six
`experiments/measurements/gate_margin/*.json` present); `hillclimb_league.py`,
`hillclimb_pool.py`, `arena.py`, `run_league.sh` (live `league_state/`, 596KB
of logs); `behaviour.py` + `run_behaviour.sh` (via the wrapper — the module
itself is broken); `roster_match.py` (63KB of `roster_match.jsonl`);
`roster_report.py` (it generates the tables in `BOT_ROSTER.md`);
`measure_champions.sh`; `tools/scrape_bgo.py`; `tools/scrape_bgg_files.mjs`;
`tools/bench_copy.py`, `bench_interp.py`, `mutation_coverage.py`,
`find_mutations.py`; `tools/horizon_ab.py`, `guard_ab.py`, `culture_probe.py`.

**RUN ONCE THEN ABANDONED.** `experiments/bookmatch.py` (declares
`--out experiments/bookmatch.jsonl`; the file is absent but five
`experiments/logs/bookmatch_*.log` exist, so it ran and the JSONL was discarded
or redirected); `book_diag.py`; `pickstats.py`; `harness.py` (superseded by
`arena.py`); `summarize.py` / `analyze_weights.py` (and see shortlist item 17);
`tools/fsum_patch.py`, `dump_game.py`, `trace_game.py`, `measure_mutation.py`,
`profile_bot.py` — all one-shot diagnostics from the PyPy era, all still
importable.

## In-code TODO/FIXME sweep: remarkably clean

Grepping `TODO|FIXME|XXX|HACK|for now|temporary|should be|not implemented|
approximation` across `engine/ tools/ experiments/ tests/ analysis/ advisor/`
yields **7 hits and none is a real debt marker**: one is a *string literal* in
report output (`tools/find_mutations.py:125`), two use "approximation" as
correct statistical or design vocabulary, two are prose asserting the opposite
of debt, and two are quoted expert advice. A wider sweep
(`placeholder|stub|WIP|hard-cod|we assume|good enough|revisit`) adds nine more,
almost all of which are *claims of rigour* — "measured, not assumed",
"MEASURED, not assumed", "read from the engine card DB, not hard-coded". The
single genuine marker is a documented sentinel
(`experiments/hillclimb_pool.py:75`).

**Conclusion: there is essentially no comment-level technical debt in this
repo. The debt lives in stale counts, stale defaults and unmerged branches, not
in TODOs** — which is precisely why an archaeology pass finds things a grep for
`TODO` would not.

Doc citations from code all resolve: all 9 cited `docs/*.md` files exist on
master (`BOT_ROSTER.md` only because `8c94318` re-added it after `1a6a43c`
deleted it — three code comments would otherwise dangle), and both cited
*sections* resolve. The one dangling section reference lives on a branch
(`tools/shape_ab.py` cites `CULTURE_GAP.md section 11`, which exists only in
that branch's own 634-line extension) and would break the moment the tool is
merged without the doc.

## One structural observation about the evidence standard

Not a lost finding, but it explains why so many entries above are marked
UNPROVEN. From `experiments/generations_*.jsonl` (**MEASURED**): candidate
acceptance runs at **n=48 games at 2p and 4p, n=144 at 3p**, with
`--accept-z 1.2816` (one-sided 90%). By the n>=200 bar this ledger applies,
**every accepted generation is an unproven step**, and a 90% one-sided test
repeated over hundreds of generations is, mechanically, a false-acceptance
machine. `LEAGUE_TRAINING.md` mitigates this well — paired scoring against a
byte-identical champion game removes the dominant variance term, the gate veto
stops a loss being averaged away, and the periodic full-pool re-check is a
regression tripwire — but the tripwire's own sample (`--check-games 48`) is
flagged in the same document as too small to trust, and was never raised.

This is the mechanism behind the failure mode this dig was asked to hunt. It is
not a bug, and the design compensates for it thoughtfully; it is simply the
reason that "confident results that later reversed" keeps happening, and it is
worth naming.

