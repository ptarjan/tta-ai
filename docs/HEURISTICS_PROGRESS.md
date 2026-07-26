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
