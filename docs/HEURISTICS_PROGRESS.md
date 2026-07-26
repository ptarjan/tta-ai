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
