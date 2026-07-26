# HEURISTICS.md — work log for whoever picks this up next

The reader (a smart board gamer, not a programmer) gave this feedback on the
first draft:

> "Good start on the doc. Can you please write it without so much ML jargon?
> Just tell me what to do. Also the build order would be useful as would a sort
> order of leaders/wonders/civic buildings/technologies per age (or globally?)"

Plus four questions:
1. "I'm really surprised they ever waste an action. Isn't taking or playing a
   yellow card almost always worth it?"
2. "Round 3 leader seems later. Is the draft round counted as round 0?"
3. "Do you do mine or farm?"
4. "when do you build the first one?" (temples / happiness)

## Plan (five units, commit + push after each)

1. **De-jargon the whole document.** — DONE
2. **Add a BUILD ORDER section** (turn-by-turn, rounds 1–6, per player count).
3. **Add PRIORITY / SORT ORDER lists** (leaders, wonders, civil buildings,
   technologies) with a stated global-vs-per-age policy.
4. **Answer the reader's four questions in the doc**, in plain language.
5. **Restate rule 8 as a known defect of the AI**, not as advice.

## Status

- **Unit 1 (de-jargon): DONE.** Removed "weight vector", "hill climbing",
  "generation", "champion", "1-ply", "evaluation function", "mutant", "feature
  diff", "weighted delta", "local optimum", "under selection", "gradient",
  "trial state", "harvest". Everything that came from tuning is now phrased as
  "the AI taught itself to value X more/less than we told it to"; everything from
  self-play is "across N self-play games". Also added an explicit **round
  numbering** note at the top (round 1 = the Age A turn; there is no round 0 —
  `engine/state.py:110`, `engine/game.py:75`), which answers reader question 2.
- **Unit 2 (build order): DONE.** New section "The build order, turn by turn" in
  the Opening chapter, with move-by-move tables for 2p and 3p (60 self-play games
  each, from the now-working `analysis/opening_order.py`; raw output committed as
  `analysis/out_opening_{2,3}p.txt`). 4p is the coarse version only — the
  fine-grained run was still starved by the live training load. **If you pick
  this up: re-run `python3 analysis/opening_order.py --players 4 --games 60
  --champion /tmp/ch4.json` and replace the 4p subsection with a real table.**
  Answers reader question 3 (mine or farm): **2p mines on round 2 in 100% of
  games, 3p farms on round 2 in 97%.**
- **Units 3–5: TODO.**

## Notes for the next person

- `analysis/opening_order.py` was **broken** — it wrapped bots with a `.choose()`
  method but `engine/game.play_game` calls bots as plain callables, so every game
  raised `TypeError: 'NoneType' object is not callable` and it had produced zero
  data. Fixed: `__call__(self, state)` delegating to `self.inner(state)`, plus
  `card_type()` now reads the card dicts correctly (cards are dicts, not
  objects), plus whole-game `first_develop` / `first_build` / `first_take`
  milestone tracking so we can separate "research it" from "build the first one".
- **Never `git add -A` in this repo.** A live training run is constantly
  rewriting `experiments/champion_*.json`, `experiments/generations_*.jsonl` and
  `experiments/league_*/`. Add only the paths you touched.
- Always copy `experiments/champion_*.json` to `/tmp` before analysing; they move
  under you.
- Forward links currently written into the doc that must exist by the end of unit
  4: `#is-wasting-a-civil-action-ever-right` and
  `#when-exactly-do-you-build-the-first-temple`.
