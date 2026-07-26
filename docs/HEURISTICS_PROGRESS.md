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
