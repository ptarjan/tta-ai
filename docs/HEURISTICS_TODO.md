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
- **Unit 3 (priority lists): DONE.** New top-level section "Priority lists:
  which card do I take?" — states up front that the lists are **per age, not
  global** (the row only ever holds the current age and the next, so a global
  card ranking is unusable), gives one global *type*-level order, then ranks
  leaders / wonders / civil buildings / technologies within each age with a
  one-line reason each. Flags loudly that (a) the AI is nearly card-blind when
  *taking* a card so per-card take counts are weak, and (b) every military and
  political card is systematically underrated because the AI cannot use it.
- **Unit 4 (the four reader questions): DONE.** New section "Four questions a
  reader asked", plus the 4p build order upgraded from coarse to a real
  move-by-move table (20 games).
  1. *Wasted actions* — answered from `docs/WASTED_ACTIONS.md`, which another
     agent measured while this was being written. **The reader is right and the
     bot is wrong**; the doc says so plainly rather than defending it.
  2. *Round 0* — no. Stated at the top of the doc and again here.
  3. *Mine or farm* — mine at 2p and 4p, farm at 3p, all on round 2.
  4. *First temple* — three milestones separated: nothing to research (Religion
     is printed on the board), first temple BUILT round 3 / 7 / 4, happy-face
     deadline set by the population track around round 9.
- **Unit 5 (rule 8): DONE.** Rule 8 is no longer presented as a rule at all. It
  is now headed "NOT ADVICE - a known defect. The AI cannot fight, so ignore
  everything it does about fighting", explains the mechanism (payoff lands inside
  another player's pending decision, so the move is strictly dominated by passing
  regardless of tuning), cites the evidence (pact legal in 16% of political
  decisions across 240 games, chosen zero times; scored at -1.10445 against
  passing) and tells the reader explicitly to **use aggression normally** and not
  to copy the AI's army size. The intro now says "seven rules, plus one warning
  label".

## All five units are complete.

## Later: two coordinator messages, both landed

- **A hand-written book bot beats our trained AI** (62.9% +/- 4.7%, n=400, 2p;
  mean culture 155 vs 124). HEURISTICS.md is restructured around this. It now
  opens with "Read this first: our AI is not a strong player", followed by a new
  section "What the measurements actually confirm" holding the five measured
  corrections and the book bot's 13-step priority list. Everything else is
  explicitly demoted to "this describes what our AI does". Rules 5 and 7 carry
  warnings because they point the same way as the AI's biggest measured failing
  (stops investing, gets overtaken around round 15). Frozen-AI-predates-7d40f53
  caveat is stated. Source: docs/STRENGTH_CHECK.md, engine/bots/book.py.
- **"The training moved this weight, therefore it matters" is not valid.**
  Evidence grades are now numbered 1-3 in "How to read this document", with
  grade 3 (learned weights) marked as not evidence at all, and the
  wonder_remaining result (27.6% +/- 6.3% vs a 25% null, n=192) given as the
  caught-red-handed example. New [confirmed] tag for head-to-head results.
- **The 4p colony layer was dead in all our data.** New caveat 4, plus warnings
  on the 4p build order, the 4p per-count section, the wonder priority list and
  the "what this document does not know" colonies entry. NOTE: docs/
  AGGRESSION_FIX.md and the later 12-game check disagree on the mechanism
  (auctions opening with no eligible bidders vs no territory ever revealed); the
  doc states the agreed consequence and flags the disagreement rather than
  picking a side. Someone should reconcile those two measurements.
- **The 4p wonder-first opening** is now presented as a mis-set number worth
  nothing, not strategy. The sweep-speed / competition explanation was deleted,
  not softened - the Age A deck is count-invariant and the first sweep is in
  round 2, so those mechanisms are inert on round 1.
- Also corrected: the old "probably at 4 players" hedge on training strength (4p
  is actually the strongest relative to its null) and the understated seed noise.

Still open: re-run the book-bot benchmark against an AI trained after the
7d40f53 card-DB fix; reconcile the two colony measurements.

Remaining nice-to-haves, none blocking:
- Re-run the 4-player build order with 60 games instead of 20 (it was starved by
  the live training load): `python3 analysis/opening_order.py --players 4
  --games 60 --champion /tmp/ch4.json`.
- The per-card priority lists for buildings/technologies rest on take counts,
  which are weak evidence because the bot is nearly card-blind when taking. If
  the end_turn scoring bug in docs/WASTED_ACTIONS.md gets fixed, re-harvest and
  rebuild those lists - they should get much sharper.
- Nothing in this document is tested against a human opponent.

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
