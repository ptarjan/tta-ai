# HEURISTICS.md progress log

## 2026-07-26 (session 2)
- Champion generations at start of this session: **2p gen 149, 3p gen 116, 4p gen 101**.
- Launched fresh behaviour harvest (120 games/count, mirror self-play, nice 15,
  frozen champion copies in /tmp/beh_champ_*.json) writing to
  `experiments/behaviour_{2,3,4}p.new.json`. Logs: `experiments/logs/beh_*p.log`.
  When they land: move them over `behaviour_{2,3,4}p.json` and re-check every number.
- DONE so far: header, "How to read", rules 1-8 (pre-existing).
- NEXT: sections 1-6 (Opening, Midgame, Endgame, Per-player-count, Traps, Quick reference).

### 11:15
- Header snapshot + strength table refreshed (gen 149/116/101; 82.3/70.3/66.2% vs start point).
- Quick reference section written (all [rules], from RULES_SPEC).
- Common traps section written (rules + FRESH analyze_weights drift). Behaviour
  numbers not yet folded in there.
- `experiments/behaviour.py` is BROKEN: `_summarize_group` calls an undefined
  `all_snaps_iter`, so every run dies after playing all its games. Not my file;
  worked around with `analysis/behaviour_run.py` (injects the helper, same CLI).
- Behaviour harvest re-launched via that wrapper, 120 games/count -> *.new.json.
- NEXT: Opening, Midgame, Endgame, Per-player-count (all need the fresh JSON);
  then re-verify the numbers in "If you remember nothing else" against it.

### 11:05
- Fresh 2p + 3p behaviour (120 games, gen 149 / 116) landed and MOVED over
  `experiments/behaviour_{2,3}p.json`. 4p harvest still running (pid 33784,
  ~6 min in) -> `experiments/behaviour_4p.new.json`; `behaviour_4p.json` is
  still the OLD 60-game gen-101 file. Do not cite 4p numbers until it lands.
- Fresh data moved a lot vs the old file. Known stale numbers in rules 1-8:
  2p ca_left_per_turn is now 1.735 (doc says 2.75), turns_with_unspent_ca 0.428
  (doc says 0.68); 3p is now 1.925 / 0.486 so the "3p wastes fewest" claim is
  DEAD. 2p temples 3.65/game (doc says 2.83). 2p sci_per_culture is no longer
  monotone (0.79 / 0.78 / 0.92 / 0.87). All to be fixed in the re-verify pass.
- The 3p champion has become a MILITARY build: 7.14 infantry/game, strength 7.28
  at end of Age III vs 2p's 3.79, never upgrades production in 39% of games.
  The 2p champion DISBANDS its starting Warriors on round 2 in ~all games.
- WRITTEN: "Opening: Age A and the first four rounds" (2p/3p only, flagged).
- NEXT: Midgame (Age I-II), then Endgame, then per-count, then re-verify rules 1-8.

### 11:12
- 4p harvest LANDED (120 games, 0 errors) and moved over `experiments/behaviour_4p.json`.
  All three files are now fresh 120-game harvests at gen 149 / 116 / 101.
- The fresh 4p champion is a completely different animal from the old 60-game one:
  takes a wonder on ROUND 1 in 100% of games, 1.96 wonders started / 0.79 completed,
  16.35 final techs, wastes only 0.38 CA/turn, but scores only 56.4 culture and
  passes in the Politics Phase on 87% of turns (1.4 event preps/game vs 11.3 at 2p).
- WRITTEN: "Midgame: late Age I through Age II". Findings worth keeping:
  * all three counts park the yellow bank just above 11 from round 9 (avoiding the
    10-token happiness step) -- cleanest 3-count consensus in the data
  * midgame is worker reallocation via `destroy` (5.9/5.5/10.9 per game), not growth
  * temples first and most-built at all 3 counts; theaters/arenas last
  * sci/culture crossover is Age I -> II at all 3; the old "monotone fall" claim is DEAD
  * MA economy: unused MA -> military cards -> event preps -> free culture
- NEXT: Endgame (Age III-IV), Per-player-count, then re-verify rules 1-8.

### 11:20
- WRITTEN: "Endgame: Age III and Age IV". Key findings:
  * Age IV is ONE turn: 143/155/163 Age-IV turns over 120 games = 1.19/1.29/1.36
    per game. The real "stop buying rate" deadline is ~4 turns out, round 19-20.
  * Last two rounds are worth +19.6 / +18.3 / +11.2 culture (final Age III event
    scoring). Preparing an Age III event guarantees it is evaluated.
  * Banked science at end 25.7 / 12.9 / 6.2 vs final techs 12.85 / 9.98 / 16.35 --
    least banked science = most techs. 2p's 25.7 is a flaw, called out as one.
  * Zero wars in 360 games at all 3 counts; aggression only at 4p and only late
    (median round 18.5).
  * Rule 7 ("stop buying rate in Age III") is weight-supported but NOT
    behaviour-supported: no champion actually stops. Downgraded to [mixed] in situ.
- TODO carried forward: the Opening section was written before 4p landed and says
  "at both counts" in a few places -- needs 4p numbers folded in.
- NEXT: fold 4p into Opening, then Per-player-count, then re-verify rules 1-8.

### 11:28
- Folded the fresh 4p numbers into the Opening section (round-1 wonder, disband,
  card-row depth, government, end-of-Age-I table, 3-way round-2 comparison).
- Notable revision: with 4p in, the 3p military opening now looks like a LOCAL
  OPTIMUM rather than a player-count effect -- 4p faces three opponents and still
  opens economically, and ends with the most techs. Said so in the doc.
- 4p takes a wonder on round 1 in 120/120 games; the rules argument is good
  (wonders bypass the hand, round 1 is take-only, no completed-wonder surcharge
  yet) but it starts 1.96 and finishes 0.79, so the doc takes the first half only.
- NEXT: Per-player-count section, then re-verify rules 1-8.

### 11:40
- NEW TOOL `analysis/leak_check.py`: wraps `economy.end_of_turn` at runtime and
  compares the culture the rating said you should score against what you actually
  scored. The gap is the 4-culture-per-missing-food starvation penalty (§6.6).
  (Careful: the bot's 1-ply search also calls end_of_turn on CLONED states, so the
  tally is keyed on id(state) and only the id play_game returned is counted.
  Without that the numbers come out ~5x too big.)
- BIGGEST FINDING OF THE SESSION (20 games/count, mirror, frozen champions):
    2p: 19.7 culture burned to starvation per player-game (14.1% of turns short)
    3p:  4.8 (4.9% of turns)
    4p: 59.6 culture burned vs 52.7 actually scored (47.3% of turns short!)
  In Age III the 4p champion burns 5.25 culture/turn to starvation against a
  culture RATE of 6.63 -- it nets about 1.4. This entirely explains why 4p final
  culture is 56.4 against 2p's 123.7 despite 4p having more techs.
  Uprisings by contrast cost almost nothing (0.38 / 0.03 / 1.80 per game).
- 60-game confirmation run launched -> experiments/logs/leak_check.log
- Also found: `military_by_age.ratio_to_strongest` (vs the STRONGEST rival, not the
  mean) is 1.02-1.07 at 2p but 0.75-0.84 at 3p and 0.46-0.60 at 4p, and the 4p
  champion spends ~50% of turns BELOW HALF the strongest rival's strength.
  Rule 8's "parity with the table" claim is only true at 2p.
- NEXT: Per-player-count section, then rewrite traps #2, then re-verify rules 1-8.

### 11:52
- 60-game leak_check CONFIRMATION RUN LANDED (`experiments/logs/leak_check.log`).
  Confirms the 20-game finding with slightly softer 4p numbers:
    2p: 21.36 culture burned to starvation / player-game, 16.5% of turns
    3p:  6.02, 6.3% of turns
    4p: 56.05 against ~60 banked, 46.1% of turns
  By age at 4p: I=2.2, II=17.8, III=27.6 (4.71/turn), IV=8.5 (6.25/turn).
  Uprisings: 0.27 / 0.03 / 0.64 culture per player-game -- near-free. USE THESE
  60-game numbers, not the 20-game ones.
- Champion generations now 169 / 130 / 111 (15 / 10 / 6 accepted mutants).
- WRITTEN: "What changes with the player count" (per-count section). Contents:
  rules-diff table (corrected against RULES_SPEC §13: sweep 3/2/1, deck trimming,
  NOT a refill-size difference), divergence table, one subsection per count,
  opening cheat sheet, "where the counts agree".
  * 2p = cheap-card conveyor (88.4% band-1 cards, 1.15 CA/card) but worst action
    discipline (57.6% of Age III CAs wasted, 25.7 banked science).
  * 3p = expensive cards (56.9% band-3, 2.33 CA/card) + military build; called a
    LOCAL OPTIMUM, with 4p as the counter-evidence.
  * 4p = best action discipline (0.38 CA/turn wasted) + most techs (16.35) but
    starving engine (food rate 1.20/1.18/1.03/0.89, culture 56.4).
- NEXT: rewrite trap #2 (starvation) with the 60-game numbers, then rewrite
  headline rule 8 (ratio_to_strongest), then re-verify rules 1-8 + header.

### 12:00
- REWROTE trap #2 (starvation) with the 60-game numbers. It is now the longest
  trap and framed as "the biggest leak in the game".
- IMPORTANT correctness note discovered while writing: `s.food` / behaviour
  `food_rate` is GROSS food production per turn (engine/effects.py:45,
  experiments/behaviour.py:157), NOT net of consumption. So the right comparison
  is production vs the consumption number (2 at 12-9 tokens left, 3 at 8-5,
  RULES_SPEC 6.1). All champions sit at 9.2-9.8 pop_bank at end of Age III and
  7.2-7.8 in Age IV -> consumption steps 2->3 in the last age at EVERY count.
  That arithmetic reproduces the measured burn exactly (4p produces ~1.0 vs
  consumption 2 = 1 food short = 4 culture/turn; measured 2.5-4.7).
  Any future edit that treats food_rate as net is WRONG.
- Also added the honest exception to trap #4: a farm bought in Age III that
  closes a 1-food gap is worth ~24 culture, so "stop buying rate" does not apply
  to rate that PREVENTS A PENALTY.
- NEXT: rewrite headline rule 8 (military ratio_to_strongest), then re-verify
  rules 1-7 + header snapshot against the fresh 120-game data.

### 11:40 (session resumed after bridge restart)
- Checked state: Midgame, Endgame, Per-count, rule 8 honest rewrite, and the
  60-game leak_check fold-in are ALL DONE (commits 2ab8188 back to df1e479).
  The only remaining item is the human-facing FINAL READ-THROUGH.
- Header snapshot refreshed to the live generations 176 / 132 / 113, and made
  the plateau explicit: last accepted mutant was gen 147 / 120 / 103, i.e. the
  climbs have gone 30 / 12 / 10 generations without an improvement.
- Fixed a real contradiction found in the read-through: rule 8 said "mean
  strength at the END of Age III 3.1/6.8/2.3" while the opening and per-count
  sections say 3.79/7.28/2.99. Both are right — military_by_age is a mean over
  every Age III turn, end_of_age is the last turn only. Rule 8 now says which
  is which and cross-references the other.
- NEXT: read-through of Midgame -> Endgame -> Per-count -> Traps -> Quick ref,
  hunting vague advice, missing numbers and cross-section contradictions.

### 11:50
- READ-THROUGH, Endgame section. Found and fixed a REAL ERROR: the "Military in
  the endgame" table labelled `by_age.strength_vs_opponents` as "ratio to
  strongest rival" (2p 1.07 / 3p 1.03 / 4p 1.13). It is not — behaviour.py:377
  divides by `opp_strength`, the MEAN over opponents (behaviour.py:175), while
  `military_by_age.ratio_to_strongest` uses `opp_strength_max` (:176, :505).
  So the endgame section was asserting Age IV parity at all three counts while
  headline rule 8 asserted 0.75 / 0.60. Table now carries BOTH columns, says the
  mean-rival column is meaningless in a mirror, and the "parity is the target"
  conclusion is re-derived from the strength_deficit weight instead of from the
  fake parity. (4p Age IV vs mean is 1.06, not 1.13 -- also corrected.)
- Added the starvation exception to the endgame "stop buying rate" paragraph
  with the actual arithmetic (4 culture/turn/missing food, ~16 culture for a
  round-19 farm), so it no longer contradicts trap #2.
- NEXT: per-count section, traps, quick reference.

### 12:02
- READ-THROUGH, per-count + traps. Fixes:
  * 2p section claimed "two cards leave the row every turn and eight come in" --
    wrong, the 2p sweep is THREE per turn (RULES_SPEC 1.5 / 2.1, six a round),
    which is also what the rules-diff table two paragraphs above says. Fixed.
  * 2p "food rate a comfortable 2.3 all game" contradicted trap #2 (2p burns
    21.4 culture to starvation). Now says 2.3 is only comfortable while
    consumption is 2, and points at the Age III-IV bank crossing.
  * trap #2 food table had 4p Age I = 1.40; behaviour_4p by_age says 1.60. Fixed.
  * per-count 4p food numbers (1.20/1.18/1.03/0.89) are END-OF-AGE snapshots, not
    the per-turn means in trap #2 (1.60/1.12/1.05/1.04). Both are now labelled.
  * "keep a real food surplus" was exactly the kind of numberless advice the
    brief calls out -- replaced with "consumption + 1 = 3/turn at 12-9 tokens,
    4/turn at 8-5".
  * trap #4 said the 4p climb accepted 5 mutants; it is 6.
  * added the `food_rate_late` sign flip at 4p (-0.6 -> +0.17) to trap #2's
    weight evidence.
  * refreshed the stale gen 169/130/111 in the per-count section.
- NEXT: quick reference read-through, then the "remember nothing else" / opening
  consistency sweep, then declare structural completeness.

### 12:15
- Added the single most actionable new fact of the read-through, to the midgame
  Wonders subsection: at 4p (235 wonder builds over 120 games) builds STARTED by
  round 12 complete 59% of the time (82/140); builds started round 13+ complete
  14% (13/95); and the three 12-resource Age II wonders (Ocean Liners, Kremlin,
  Transcontinental Railroad) went 0 for 58. Rule: start a wonder by round 12 or
  do not start it. Cost figures from data/cards_wonders_leaders.json (Age A = 6
  resources, Age I = 8-9, Age II = 12-13).
- Footnoted the started/completed metric quirk (2p shows 0.18 completed vs 0.17
  started; a wonder finished on the turn it is started can register no start).
- Cheat-sheet first-aggression row corrected to 18.5 (8.3%) / 4 (3.3%).
- Quick reference re-checked line by line against RULES_SPEC 1.5/2.1/6.1/6.2/6.3/
  6.6/12.x/13 -- all tables correct, no changes needed.
- NEXT: final top-to-bottom consistency pass (numbers that appear twice), then
  declare structural completeness.

### 12:28
- Cross-section number sweep. Fixed final_techs quoted as 12.85 / 9.98 in two
  places (endgame "stop banking science" table and the midgame waste section);
  behaviour `overall.final_techs` is 12.88 / 9.81 / 16.35. Verified against the
  JSON: rates by age, unspent-action shares (42.8% / 48.6% / 10.8% of turns with
  something left), MA left per turn, end-of-age strengths -- all correct.
- Noted in trap #2 that its final-culture column (129.9 / 107.5 / 60.1) comes
  from leak_check's own 60 games, not the 120-game harvest (123.7 / 113.2 /
  56.4), so the two are not a contradiction.
- NEW CLOSING SECTION "What this document does not know": pacts (ZERO played in
  240 games at 3p/4p -- the move type never appears in moves_per_game),
  colonies (0.18/0.08/0.02 bids per game -- effectively never), fighting, wonders
  at 2p/3p, per-card strength, and human opponents. This is the honest boundary
  of the study and it was missing.
- STRUCTURALLY COMPLETE. All planned sections exist: header + how to read,
  If you remember nothing else (8 rules), Opening, Midgame, Endgame, Player
  count, Common traps (6), Quick reference (rules-only), What this document does
  not know. Every quantitative claim is cited to behaviour_{2,3,4}p.json,
  leak_check.log, analyze_weights.py, PROGRESS.md or RULES_SPEC.md, and the
  [rules]/[strong]/[mixed]/[provisional]/[thin] tags are in place throughout.
- Remaining work is refresh, not structure: re-harvest behaviour when the climbs
  accept new mutants (last acceptances were gen 147 / 120 / 103) and re-run the
  header strength table.

### 12:36
- Last contradiction found and fixed: "Where the counts actually agree" claimed
  "temples are the most-built card at every count". False -- `builds_by_type`
  says the biggest spender is mines at 2p (4.28) and 4p (5.38) and infantry at 3p
  (7.14); temples are the most-built URBAN building at 2p/3p only (labs 4.71 beat
  them at 4p). Reworded, with the all-card numbers included so the claim can be
  checked at a glance. The two other places that make this claim (rule 4 and the
  midgame build-order subsection) were already correctly scoped.
- Added a table of contents and a "five minutes before you play" reading path
  (rules -> trap #2 -> your count's opening cheat sheet).

### 12:42 — SESSION CLOSE
- HEURISTICS.md is STRUCTURALLY COMPLETE (1448 lines, 9 sections + TOC). Nothing
  planned is missing. Future work is data refresh, not writing.
- PROCESS NOTE FOR THE NEXT AGENT: do NOT use `git add -A` in this repo. The
  hillclimb is live and rewrites experiments/champion_*.json, generations_*.jsonl
  and league_*/ constantly, and other agents have files in flight (a
  sources/bgg_*.xls appeared and vanished mid-session and got swept into two of
  my commits). Stage explicitly:
      git add docs/HEURISTICS.md docs/HEURISTICS_PROGRESS.md analysis
- Refresh checklist when the climbs next accept mutants (currently gen
  176 / 132 / 113, last acceptance 147 / 120 / 103):
  1. re-run experiments/run_behaviour.sh at 120 games/count
  2. re-run analysis/leak_check.py (60 games) for trap #2
  3. re-run experiments/analyze_weights.py for every weight percentage
  4. update the header snapshot (generations, accepted counts, anchor table)
