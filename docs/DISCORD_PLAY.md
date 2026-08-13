# Playing Through the Ages over Discord (2p, CGE app, Hard AI, no expansion)

Paul is at the iPad; the operator is on Discord. `advisor`'s normal mode is
an interactive terminal REPL (`docs/APP_HARNESS.md`'s `harness` binary is
the same shape) — neither can be driven from a chat message. `advisor
--save` is the one-shot alternative: one invocation reads a handful of
update lines off stdin, prints the recommended move(s) for the resulting
turn, and writes the position back out for the next exchange. No tty, no
prompts, exits non-zero (naming the bad line) on anything it cannot parse.

This doc is the operator's script: the commands to run, the five things
Paul has to report each turn, and a worked example. It does not replace
`docs/APP_HARNESS.md` — that binary's 7-numbers-per-rival protocol is a
richer, still-interactive measurement tool for a logged benchmark session.
This is the cheap path for an actual casual game, built around what
was already measured to matter (below).

## 0. One-time setup, per session

```
cd rust
cargo build --profile difftest --bin advisor
```

`experiments/rust_champion_2p.json` is rewritten by the running hill-climb
every few minutes — advising off it live would mean the position and the
weights that scored it don't agree by the time you read the reply. Freeze
one copy at the start of the session and reuse it for the whole game:

```
cp /Users/pt/tta-ai/experiments/rust_champion_2p.json ~/tta_2p_frozen.json
```

(That `cp` is the only thing that touches `/Users/pt/tta-ai`; everything
below runs against the frozen copy, never the live file.)

## 1. What Paul has to report, and nothing more

1. **The card row** — all 13 slots, left to right, `.` for an empty slot.
   Contents, occupancy and slot order all matter (slot position is the
   civil-action cost); nothing else about the row does.
2. Rival's **military strength**.
3. Rival's **culture**.
4. **Every military card Paul draws**, by name, as he draws it —
   `p0 hand <civil cards> | <military cards>`, retyping the whole hand.

(2) and (3) repeat once per rival — at 2p that's exactly one rival, `p1`.

Items 1–3 are the *evaluation* inputs: they were measured
(`docs/APP_HARNESS.md` §2 / §6) and everything else about the position is
inert against the current evaluator. **Item 4 is a legality input and was
missed by that measurement**, because a census of self-play never
discovers it. The advisor plays Paul's civil cards itself, so it knows his
civil hand exactly — but military cards are DRAWN at end of turn, and the
mirror draws them at random from its own deck. Within two rounds of the
first live game the bot recommended preparing an event card
(`Rebellion`) that Paul did not hold. A recommendation naming a card the
player cannot play is not a bad recommendation, it is not a
recommendation at all, so this is not optional the way (2) and (3)
degrade gracefully.

What is still knowingly approximate, and is accepted: the rival's *hand*.
The engine simulates the rival's turn with its own bot, so it takes
different cards than the app's AI did. The `row` line repairs the table
every exchange and `p1 leader` / `p1 wonder` / `p1 built+` repair what is
public about their board, but nobody can see the rival's hand and the
protocol does not pretend to.

## 2. The exact commands

**Turn 1 of a fresh game** (no `--load` yet — the freshly-dealt row in the
engine's own mirror is a guess, so the first update line always corrects
it):

```
printf 'row <13 cards, left to right, . for empty>\n' | \
  ./target/difftest/advisor --players 2 --seat 0 \
    --weights ~/tta_2p_frozen.json \
    --save ~/tta_state.json
```

**Every exchange after that** — report the row plus the rival line, load
the previous state, save over it:

```
printf 'row <13 cards>\np1 str=<N> c=<N>\n' | \
  ./target/difftest/advisor --players 2 --seat 0 \
    --weights ~/tta_2p_frozen.json \
    --load ~/tta_state.json --save ~/tta_state.json
```

**The row you report is the row as it stands in the app at the moment
Paul is about to act** — after the rival's turn and after the
start-of-turn replenish, i.e. exactly what he is looking at. `run_batch`
hands the turn back first and applies the report second, so the position
the bot scores is the reported one. (It used to patch first and then
advance, which let the engine's own sweep-and-refill run on top: at 2p
that discards the three leftmost cards, slides every survivor three slots
left and deals three cards that are not on the table. Every slot number
printed was off by three. Fixed in `1e83812`, with
`the_reported_row_is_the_one_the_bot_scores_and_is_never_replenished_on_top_of`
pinning it.)

If Paul reports only the newly-dealt cards and what the rival took, the
full row can be reconstructed rather than retyped: at 2p the replenish
sweeps the **3 leftmost slots** (`docs/RULES_SPEC.md` §2, sweep 2p:3 /
3p:2 / 4p:1), slides the survivors left in order, then deals into the
right-hand gaps. Check the arithmetic every time — the number of new
cards Paul reports must exactly equal the number of gaps the sweep and
the takes opened. If it doesn't, the reconstruction is wrong; ask for the
whole row instead of guessing.

Read the printed move sequence to Paul; he plays exactly that, in order,
in the app. Nothing else needs typing back — the state file already has
it. If a line does not parse, `advisor` exits non-zero and stderr names
the exact line, e.g.:

```
advisor: bad update line(s):
  "row Urban Growth, ...": "Urban" is ambiguous: Urban Growth (A), Urban Growth (I), Urban Growth (II), Urban Growth (III)
```

— disambiguate with the age suffix shown on the card in the app (`Urban
Growth (A)`) and resend. Card names are fuzzy otherwise (a short prefix,
initials, or a subsequence all resolve, same as the interactive REPL).

## 3. The update-line grammar (only what's used here)

```
row <13 cards>       retype the whole row, comma- or space-separated,
                      '.' for an empty slot -- always all 13, always the
                      full row as it stands right now
p1 str=<N>            rival's military strength
p1 c=<N>               rival's culture
```

Both rival fields can go on one line (`p1 str=3 c=4`) or two. Blank lines
and `#`-prefixed lines are ignored. (The underlying grammar in
`rust/src/advisor/state_io.rs`'s `PATCH_HELP` supports much more — tech,
wonders, colonies, government, hand sizes — but none of it is needed here;
do not ask Paul for it.)

## 4. What gets printed

One block per action of the recommended turn: the position (round, age,
CA/MA/food/res/sci), the top pick starred plus up to two runners-up with
their score gap and a one-line reason, e.g.:

```
-- move 3 (round 3, age I): CA 3, MA 2, food 2, res 0, sci 2
 *1. TAKE 'Iron' (Mine, age I) from row slot 6  [2 civil action(s)]
       why: +2 value of your hand, +1 civil cards in hand, -2 unspent civil actions
  2. TAKE 'Patriotism (I)' (Action, age I) from row slot 7  [2 civil action(s)]  (-0.5)
       why: +2 value of your hand, +1 civil cards in hand, -2 unspent civil actions
```

The starred move is always the one actually applied to the saved state —
there is no one at a keyboard to confirm it, so the bot's own top pick and
"what Paul should play" are the same thing. The turn ends when `advisor`
prints `END YOUR TURN`.

## 5. Worked example (real output, 3 rounds)

**Exchange 1** — fresh game, Paul reads the row off the app:

```
$ printf 'row Urban Growth (A), Frugality (A), Hanging Gardens, Homer, Stock Pile, Aristotle, Hammurabi, Julius Caesar, Moses, Alexander the Great, Colossus, Library of Alexandria, .\n' | \
  ./target/difftest/advisor --players 2 --seat 0 --save ~/tta_state.json

ok: row set

-- move 1 (round 1, age A): CA 1, MA 0, food 0, res 0, sci 0
 *1. TAKE 'Homer' (Leader, age A) from row slot 3  [1 civil action(s)]
       why: +1 civil cards in hand, +1 value of your hand, -1 unspent civil actions
  2. TAKE 'Stock Pile' (Action, age A) from row slot 4  [1 civil action(s)]  (-0.1)
  3. TAKE 'Urban Growth (A)' (Action, age A) from row slot 0  [1 civil action(s)]  (-0.1)

-- move 2 (round 1, age A): CA 0, MA 0, food 0, res 0, sci 0
 *1. END YOUR TURN (production, then pass the board on)
       why: -3 room left before corruption worsens, +2 resources, -4 blue tokens in your bank
```

Tell Paul: take Homer, then end turn. The Hard AI now plays its turn on
the iPad.

**Exchange 2** — Paul reads the new row and p1's strength/culture off the
app:

```
$ printf 'row Frugality (A), Hanging Gardens, Stock Pile, Aristotle, Hammurabi, Julius Caesar, Moses, Alexander the Great, Colossus, Library of Alexandria, ., Urban Growth (A), Homer\np1 str=1 c=0\n' | \
  ./target/difftest/advisor --players 2 --seat 0 --load ~/tta_state.json --save ~/tta_state.json

ok: row set
ok: p1 str = 1; p1 c = 0

-- move 1 (round 2, age I): CA 4, MA 2, food 2, res 2, sci 1
 *1. PASS on politics (play no military card this turn)
...
-- move 6 (round 2, age I): CA 0, MA 2, food 0, res 0, sci 1
 *1. END YOUR TURN (production, then pass the board on)
```

(Six actions this turn — `advisor` prints every one, in order; Paul
mirrors the starred pick at each step, then ends turn.)

**Exchange 3** — same shape again:

```
$ printf 'row Hanging Gardens, Stock Pile, Aristotle, Hammurabi, Moses, Alexander the Great, Colossus, Library of Alexandria, ., Urban Growth (A), ., Iron, Patriotism (I)\np1 str=3 c=4\n' | \
  ./target/difftest/advisor --players 2 --seat 0 --load ~/tta_state.json --save ~/tta_state.json

ok: row set
ok: p1 str = 3; p1 c = 4

-- move 1 (round 3, age I): CA 4, MA 2, food 2, res 3, sci 2
 *1. PASS on politics (play no military card this turn)
...
-- move 6 (round 3, age I): CA 0, MA 1, food 2, res 0, sci 2
 *1. END YOUR TURN (production, then pass the board on)
```

`~/tta_state.json` after each exchange is the full snapshot (`state_io`'s
own format); it round-trips exactly through `--save`/`--load` (see
`saving_and_loading_a_board_produces_the_identical_recommended_move` and
the other tests in `rust/src/bin/advisor.rs`'s `#[cfg(test)] mod tests`),
so an interrupted Discord session picks back up losslessly from that one
file plus whatever Paul reports next.

## 6. If the game ends

`advisor` detects it and prints `game over.  final culture: p0=<N>,
p1=<N>` instead of a move block; the state file still gets written, exit
code is still `0`.
