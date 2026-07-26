# The advisor

Play *Through the Ages* (base 2015) at a physical table and have the trained
bot tell you what to do.

```
python3 -m advisor.advisor --players 3 --seat 0
```

`--seat` is where you sit in turn order (0 = start player). The advisor keeps
a mirror of the board in the engine, recommends your moves, and asks you for
the bits it cannot see: which cards were dealt into the row and what your
opponents did.

It loads `experiments/champion_{N}p.json` (the hill-climbed weights for that
player count) and falls back to the built-in defaults if that file is not
there yet. The banner tells you which it used.

## What you type

At the **your move** prompt:

| input | meaning |
|---|---|
| *(Enter)* | play the top recommendation |
| `1` `2` `3` | play that numbered recommendation |
| `take 4`, `build bronze`, `dev philo`, `wonder`, `end`, `pass` | play your own move — verb plus fuzzy arguments |
| `more` | show the rest of the candidate list |
| `board` | reprint the board |
| `state` | print the snapshot (save it, resume with `--load`) |
| `undo` | roll back to the start of your turn |
| `p1 c=34` | correct the board without leaving the prompt |
| `quit` | leave (prints a snapshot you can resume from) |

At the **what happened** prompt (blank line ends it):

| input | meaning |
|---|---|
| `take p1 7` | p1 took the card in row slot 7 (slots are 0-based, printed on the board) |
| `p1 c=41 s=12 str=9` | p1's culture / science / military strength |
| `p1 tech+ irrigation:2` | p1 has Irrigation with 2 workers on it |
| `p1 tech- warriors` | p1 no longer has Warriors |
| `p1 wonder pyr 2` | p1 is building Pyramids, 2 steps done |
| `p1 built+ colossus` | p1 finished the Colossus |
| `p1 leader caesar` / `p1 tactic legion` / `p1 gov=monarchy` | as printed |
| `p1 hc=3 hm=2` | p1's hand sizes |
| `deal bronze, iron, alchemy` | the row was swept and these were dealt |
| `row a, b, ., d` | retype the whole row when the mirror has drifted |
| `event <card>` / `age II` | a new current event / the age advanced |
| `p1 c=?` | **you don't know** — the mirror keeps its old value and remembers it is unsure |

Card names are fuzzy: `pyr`, `loa`, `hang gard`, `st pet` all resolve. If a
name is ambiguous the advisor lists the candidates and asks again. Scalars
you can set: `c` culture, `s` science, `f` food, `r` resources, `ca`/`ma`
actions, `str` strength, `hap` happiness, `cr`/`sr` culture and science per
turn, `blue`/`yel` bank tokens, `fw` free workers.

Nothing you can type crashes it. Bad input is explained and re-prompted, and
update lines work at *any* prompt, so you never have to think about which
question you are answering.

## What it does and does not simulate

* **Your** turn is played move by move through the real engine, so all the
  rules — costs, action limits, corruption, consumption, uprisings, events —
  are enforced exactly.
* **Opponents'** turns are *not* replayed as moves; you report the result.
  The engine still does the book-keeping (turn order, rounds, age changes,
  their end-of-turn production), then your updates are applied on top.
* Hidden information stays hidden: rival hands are tracked as counts, and
  `?` means "unknown" rather than a guess.
* The internal deck keeps the right *composition and size* (which is what
  decides when an age ends) even though the shuffle differs from yours; every
  card you report is removed from it and every card it wrongly dealt goes
  back.

## A real advised turn

Captured from an actual session (3 players, you are p0, round 6 of age I).
Lines you type are after `>`.

```
bot: experiments/champion_3p.json
== TTA  round 6  age I  turn: p0  (you)
------------------------------------------------------------------------------
card row (cost 1 / 1 1 1 1 | 2 2 2 2 | 3 3 3 3):
   0 (1) Frugality (I)  [action I]
   1 (1) Rich Land (I)  [action I]
   2 (1) Monarchy  [government I]
   3 (1) Swordsmen  [infantry I]
   4 (1) St. Peter's Basilica  [wonder I]
   5 (2) Reserves (I)  [action I]
   6 (2) Knights  [cavalry I]
   7 (2) Iron  [mine I]
   8 (2) Cultural Heritage (I)  [action I]
   9 (3) Alchemy  [lab I]
  10 (3) Genghis Khan  [leader I]
  11 (3) Iron  [mine I]
  12 (3) Masonry  [special-tech I]
current events: Development of Warfare, Development of Politics, Development of
Trade Routes, Development of Markets   (future 1)
tactics available: Medieval Army, Heavy Cavalry
------------------------------------------------------------------------------
p0 (you)  Despotism  leader: Alexander the Great  tactic: Heavy Cavalry
   culture 0 (+0/t)   science 1 (+1/t)   strength 9   happy 0/1
   food 1 (+2)   res 2 (+2)   CA 4/4   MA 2/2   free workers 1   yellow bank 15
   techs: Agriculture:2, Bronze:2, Philosophy:1, Religion:0, Warriors:4
   hand civil: Rich Land (A), Christopher Columbus, Warfare, Rich Land (I)
   hand mil:   Aggression: Raid (I)
p1  Despotism  tactic: Medieval Army
   culture 1 (+0/t)   science 2 (+1/t)   strength 0   happy 0/1
   food 7 (+6)   res 2 (+2)   CA 4/4   MA 2/2   free workers 2   yellow bank 14
   techs: Agriculture:6, Bread and Circuses:0, Bronze:2, Philosophy:1, ...
   hand: 5 civil, 4 military
p2  Despotism  leader: Homer  tactic: Medieval Army
   culture 4 (+2/t)   science 2 (+1/t)   strength 0   happy 3/1
   food 5 (+2)   res 2 (+2)   CA 4/4   MA 2/2   free workers 2   yellow bank 16
   techs: Agriculture:2, Bronze:2, Irrigation:0, Philosophy:1, Religion:2, ...
   hand: 4 civil, 1 military

-- your turn (round 6, age I): CA 4, MA 2, food 1, res 2, sci 1
 *1. PASS on politics (play no military card this turn)
       why: keeps your options open
  2. AGGRESSION 'Aggression: Raid (I)' against p1  [1 military action(s)]  (-0.8)
       why: -1 military cards in hand, -2 value of your military hand, -1 unspent
            military actions
  3. AGGRESSION 'Aggression: Raid (I)' against p2  [1 military action(s)]  (-0.8)
       why: -1 military cards in hand, -2 value of your military hand, -1 unspent
            military actions
your move>
  -> PASS on politics (play no military card this turn)

-- your turn (round 6, age I): CA 4, MA 2, food 1, res 2, sci 1
 *1. COPY TACTIC 'Medieval Army' from the common area  [2 military actions]
       why: -2 unspent military actions
  2. PLAY ACTION CARD 'Rich Land (A)'  [1 civil action]  (-1.0)
       why: -1 civil cards in hand, -1 value of your hand, -1 unspent civil actions
  3. TAKE 'St. Peter's Basilica' (wonder, age I) from row slot 4  [1 civil action(s)]  (-1.0)
       why: +8 wonder cost left, -1 unspent civil actions
your move>
  -> COPY TACTIC 'Medieval Army' from the common area  [2 military actions]

-- your turn (round 6, age I): CA 4, MA 0, food 1, res 2, sci 1
 *1. PLAY ACTION CARD 'Rich Land (A)'  [1 civil action]
       why: -1 civil cards in hand, -1 value of your hand, -1 unspent civil actions
  2. TAKE 'St. Peter's Basilica' (wonder, age I) from row slot 4  [1 civil action(s)]  (-0.0)
       why: +8 wonder cost left, -1 unspent civil actions
  3. PLAY ACTION CARD 'Rich Land (I)'  [1 civil action]  (-0.7)
       why: -2 value of your hand, -1 civil cards in hand, -1 unspent civil actions
your move> 2
  -> TAKE 'St. Peter's Basilica' (wonder, age I) from row slot 4  [1 civil action(s)]

-- your turn (round 6, age I): CA 3, MA 0, food 1, res 2, sci 1
 *1. PLAY ACTION CARD 'Rich Land (A)'  [1 civil action]
       why: -1 civil cards in hand, -1 value of your hand, -1 unspent civil actions
  2. PLAY ACTION CARD 'Rich Land (I)'  [1 civil action]  (-0.7)
       why: -2 value of your hand, -1 civil cards in hand, -1 unspent civil actions
  3. DESTROY 'Bronze': the worker goes back to your unused pile  (-1.7)
       why: -1 workers on cards, -1 resources/turn, +1 unused workers
your move> more
    - DESTROY 'Agriculture'  (-2.7)  why: -1 food/turn, -1 workers on cards
    - DESTROY 'Philosophy'  (-6.2)  why: -1 science/turn, -1 workers on cards
    - END YOUR TURN  (-6.9)  why: +2 corruption, -3 blue tokens in your bank, +2 resources
    - PLAY LEADER 'Christopher Columbus'  (-7.6)  why: -4 strength vs the leader, ...
    - BUILD 'Agriculture'  [2 resources, 1 civil action]  (-15.6)  why: +1 uprising risk, ...
your move>
  -> PLAY ACTION CARD 'Rich Land (A)'  [1 civil action]

-- your turn (round 6, age I): CA 2, MA 0, food 1, res 2, sci 1
 *1. CHOOSE: build Agriculture
       why: +1 uprising risk, +1 food/turn, +1 workers on cards
  2. CHOOSE: build Bronze  (-1.0)
       why: +1 uprising risk, +1 workers on cards, +1 resources/turn
your move>
  -> CHOOSE: build Agriculture

-- your turn (round 6, age I): CA 2, MA 0, food 1, res 1, sci 1
 *1. DESTROY 'Bronze': the worker goes back to your unused pile
       why: -1 uprising risk, -1 workers on cards, -1 resources/turn
  ...
your move> take 12
  ! you cannot take 'Masonry' from slot 12: it costs 3 civil actions and you have 2
your move> end
  -> END YOUR TURN (production, then pass the board on)

your turn is over.  Anything to correct on YOUR board (military cards drawn,
event effects)?
  >

3 new card(s) in row slots 10, 11, 12.
  new cards (left to right, '?' if unseen)> ?

-- p1's turn.  Tell me what they did (blank line when done, 'help' for the syntax):
  > take p1 4
    ok: p1 took Iron from slot 4
  > p1 c=41 s=12 str=9
    ok: p1 culture = 41; p1 science = 12; p1 str = 9
  > p1 tech+ irrigation:2
    ok: p1 techs Irrigation:2
  > p1 hc=3
    ok: p1 civil hand = 3
  >

3 new card(s) in row slots 10, 11, 12.
  new cards (left to right, '?' if unseen)> bronze
    ok: Bronze

-- p2's turn.  Tell me what they did (blank line when done, 'help' for the syntax):
  > take p2 0
    ok: p2 took Reserves (I) from slot 0
  > p2 c=?
    ok: p2.c unknown
  >
```

Reading the recommendations: the starred line is what the bot would play.
The number in brackets is the price at the table. `(-0.8)` is how much worse
that option scores than the best one, in the bot's own evaluation units, so a
gap near zero means "these are equivalent, play whichever you prefer". The
`why:` line names the three feature changes that dominated the score.

`CHOOSE:` lines appear when a card asks a question mid-move (here Rich Land
orders a free build) — answer them the same way.

## Saving and resuming

`state` (or `quit`) prints a snapshot. Save it to a file and resume with:

```
python3 -m advisor.advisor --load mygame.tta
```

## Files

* `advisor/state_io.py` — the board text format: `dumps` / `loads` /
  `patch` / `render`, and the fuzzy card-name resolver.
* `advisor/advisor.py` — `load_bot`, `rank_moves`, `describe_move`,
  `parse_move`, the `Advisor` mirror and the `Console` REPL.
* `advisor/tests/` — `python3 -m unittest discover -s advisor/tests -t .`
