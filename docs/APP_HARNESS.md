# The app harness: measuring our bot against the CGE app's Hard AI

Everything else we measure is our own bots playing our own bots. This is the
only path to an **external** number, and there is no automated version of it:
`docs/EXTERNAL_AIS.md` §1 established that the official app has no log export,
no API, no mod hooks and no readable saves. A human at the keyboard is the
entire interface.

This document is the operator's manual. The design rationale — and the
measured field-by-field justification for how little you type — is in §2.

```
python3 -m harness.fields                 # what you will have to type, and why
python3 -m harness.play --players 3 --difficulty hard --app-version 2.4.1
```

---

## 1. Before you start a game

The harness refuses to open a log for a game we cannot honestly label.

| Check | Why it is fatal |
|---|---|
| **New Leaders & Wonders DLC is OFF** | Our engine does not implement it. A DLC game is not a weak measurement, it is a *mislabelled* one, and it will pollute the aggregate forever. |
| Difficulty recorded (`--difficulty hard`) | "The app AI" is four different agents. |
| No "world leader" personalities | Those are a different experiment. Pass `--personality X` to label them as such; the harness will not let you mix them into a `hard` run. |
| Player count matches `--players` | Weights are per player count. |
| You are seat 0 unless you say otherwise | `--seat`. |

Then: **strict mode is the measurement.** Press Enter, play what the bot
starred, in the app, exactly. Every time you "fix" a recommendation, the score
stops being a measurement of the bot — the harness records the override and
says so at the end, but only strict games belong in the headline number. Use
`--free` when you want a cheap bug-hunt instead: you play, the bot only watches,
and the override rate is the product.

---

## 2. What you type, and what you must not

The expensive part of a human-in-the-loop game is transcription.
`docs/INFORMATION_AUDIT.md` measured that the evaluator is blind to most of
what a conscientious operator would type in. So the harness does not have a
hardcoded input list — it **derives** one by perturbing the live position and
watching whether the bot's decision moves (`harness/fields.py`).

`python3 -m harness.fields` on the 3p champion today:

| you must type | why |
|---|---|
| the cards dealt into the row | you cannot take a card the mirror does not have (*legality*, not evaluation) |
| the row's left-to-right order | slot position is the civil-action cost |
| each rival's **strength** | `strength_lead` / `strength_deficit` are clamped, so it moves the argmax |
| each rival's **culture** | it is the score; it goes in the result record regardless |

| you must NOT type | measured verdict |
|---|---|
| rival techs, workers, wonders, government | reachable entirely through the three rate numbers the app already prints |
| rival food, resources, science stock | **inert** — zero effect on the evaluation |
| rival civil/military hand size *and contents* | **inert**, even though civil hands are public by the rules |
| rival civil actions, military actions, free workers, yellow bank, happiness | **inert** |
| current events, future events, deck contents, military discards | **inert** |

That table is the entire saving. §6d of `EXTERNAL_AIS.md` priced opponent turns
at "4–8 patch fields × 2 opponents × ~18 rounds, 20–30 s per opponent turn,
**12–18 min/game**". Most of those fields are in the second table.

**This list will grow, and the harness handles that by itself.** The card-row
and opponent-hand features are being written right now. When they land,
`row.contents` and `rival.hand_civil_ids` will start coming back as decision-
relevant, and the harness — which re-runs the derivation against the live board
every round — will interrupt the game with:

```
  ** THE BOT'S EYES CHANGED. It now reads: rival.hand_civil_ids, row.contents
```

Nobody has to remember to update anything. If you want to know before you sit
down, run `python3 -m harness.fields`.

### The four numbers per rival

We never mirror an opponent's board. You read four numbers off their player
panel and `advisor/state_io.py` back-solves the mirror to match:

```
  p1    c/cr/sr/str > 41/5/3/12
```

culture / culture-per-turn / science-per-turn / strength. That is sound because
those four pin down **every** rival-derived feature the evaluator has
(`rival_culture`, `rival_mean_culture`, `rival_culture_rate`,
`rival_science_rate`, `rival_strength`).
`tests/test_harness_mirror.py::ForcedRivalsAreExact` asserts exactly that, by
wrecking every rival board and restoring only those four numbers. **If that
test ever fails, this shortcut is dead and the per-game cost roughly doubles.**
It is the tripwire on the moving target; do not delete it.

Two of the four (`cr`, `sr`) are advisory today — they do not change the move.
They are kept because they cost four keystrokes, they power the arithmetic
consistency check below, and a logged game is meant to be re-scorable by future
bots that will read them.

---

## 3. The per-round loop

```
-- p1's turn.
  new cards (left to right, '?' if unseen)> bro irr alc
  did p1 hit YOU or change the shared board? (Enter = no) >

-- p2's turn.
  new cards (left to right, '?' if unseen)> mas, hang
  did p2 hit YOU or change the shared board? (Enter = no) >

== round 7 check (age II) -- read the app, do not guess
  you   c/s/str/f/r > 41/12/9/3/5
  p1    c/cr/sr/str > 22/4/3/6
  p2    c/cr/sr/str > 30/5/2/0

-- your turn (round 7, age II): CA 4, MA 2, food 3, res 5, sci 12
 *1. TAKE 'Iron' (mine, age I) from row slot 2  [1 civil action(s)]
your move>
```

Card names are fuzzy — `pyr`, `hang gard`, `st pet` all resolve, and a whole
deal goes on one line separated by spaces or commas. `?` means "I did not see
them", which is legal but makes the row untrustworthy; use it sparingly.

Everything the advisor already supported still works at any prompt: `board`,
`state`, `more`, `undo`, `p1 c=34`, `quit`.

---

## 4. Desync: the failure this harness exists to prevent

A mirror that has quietly diverged from the app does not produce a degraded
measurement, it produces a **fabricated** one — the bot was asked to move in a
position that never existed, and afterwards the log is indistinguishable from a
real game. §6d called this out: "a 90-minute game can be entirely worthless and
nobody notices".

The asymmetry that makes it catchable:

* **Your own board is simulated.** Every one of your moves goes through the real
  engine, so the mirror *predicts* your culture, science, strength, food and
  resources. A prediction can be checked, and the app prints all of it on one
  panel. That is the `you   c/s/str/f/r` line, and it is not skippable — the
  harness will not advance a round without all five.
* **Rival boards are forced.** A number you typed cannot disagree with itself,
  so rival values are *not* checksums. The only cross-check available there is
  arithmetic: if p1 was on 40 culture at +6/turn and you now type 4, you get a
  `?` warning. A warning, never a stop.

When the self-check fails:

```
!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!
DESYNC. The mirror and the app disagree:
   p0 c: mirror says 38, you read 41
The bot has been choosing moves in a position that is not the
one on your screen, for an unknown number of plies.
  [a] abort  -- log it and stop. Right answer for early games.
  [r] resync -- type corrections, then say what caused it.
!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!
```

**There is deliberately no "continue anyway".** A resync demands a free-text
cause (`unknown` is an acceptable answer) and permanently marks the game
`trusted: false`, because nobody knows how many plies the bot spent in the wrong
position before you caught it. `harness.record.summarize` counts untrusted games
but never pools them. Expect to abort the first game or two; that is the design
working, not you failing.

---

## 5. The record

One JSONL file per game, flushed after every write, so a game abandoned in round
9 still leaves rounds 1–9 on disk.

| record | contains |
|---|---|
| `game` | setup (players / seat / difficulty / **dlc** / edition / app version / weights), the `limitations` register, and the derived `observables` list |
| `decision` | the full `state_io.dumps()` snapshot as a string, the **full** ranked candidate list with scores, the move played, `source` ∈ `bot`/`human`, latency |
| `observed` | what you reported about the rivals and the row, plus the literal lines you typed |
| `check` | every per-round checksum, pass or fail, with both numbers |
| `resync` | discrepancies, corrections, and your stated cause |
| `result` | app scores, margin vs the best opponent, rounds, `trusted`, measured `effort`, and the `limitations` register **repeated** |

Because `state` is the verbatim snapshot and `loads(dumps(b))` round-trips,
every position you log can be re-scored by every bot we ever train. That is what
makes an hour of your time reusable rather than spent.

### The limitations register, and the pact bias in particular

`limitations` appears on the header **and** the result, because whoever reads
the result later may never see the header. The load-bearing entry:

> CGE's AI never offers a pact and refuses every pact offered, so the entire
> pact branch of our policy is never exercised, rewarded or punished in these
> games. This result measures the bot on a **strictly smaller game** than
> self-play. Any pact-related weight must be validated by self-play only.

Our engine implements the full pact subsystem (`offer_pact` / `cancel_pact`,
§5.9–5.10). None of it runs here. Do not report a win rate from this harness as
if it measured the game we train on. At 2 players the register downgrades the
note to `low`, because the rules disable pacts there anyway.

### Reading the logs back

```python
from harness.record import summarize
summarize(["games/2026-07-27-a.jsonl", "games/2026-07-27-b.jsonl"])
```

Returns mean margin, its standard error, win rate, measured effort, the
`limitations`, and `poolable: false` if the games were not all the same
experiment.

---

## 6. What it costs, honestly

Measured inputs, 3 players, base game:

* **~6.5 new row cards per round** (measured over three full self-play games:
  `SWEEP[3] = 2` per player turn, refilled to 13). Over ~20 rounds that is about
  **150 card names per game** — the single largest transcription cost, and it is
  irreducible: without it the bot cannot see what it is allowed to take.
* **13 numbers per round**: 5 for your panel, 4 per rival.
* **~60–70 keystrokes per round** of transcription in total. The harness counts
  them for real and writes the count into the `result` record, so after game 1
  this stops being an estimate.

| item | per game |
|---|---|
| playing a 3p game against Hard AIs at all — irreducible | 30–45 min |
| your turns: reading the recommendation and mirroring it into the app | 5–10 min |
| the row: ~150 card names, ~2 s each including reading them off screen | 4–8 min |
| the per-round rival snapshot: 8 numbers off two panels, ~15 s | 4–6 min |
| the per-round self checksum: 5 numbers, ~8 s | 2–3 min |
| setup wizard, result record, occasional resync | 3–6 min |
| **total** | **50–80 min** |

Assumptions, stated so you can disagree with them: ~20 rounds; a fluent operator
by game 3; card names abbreviated to 3–4 characters; the app's player panels
readable without navigating away from the board; no aborts. Add roughly 15–20%
across a programme for games discarded to desync, which puts a *usable* game at
**60–95 minutes**.

Against the previous estimate of 75–110 min this is a real reduction of roughly
25–30%, and it is worth being clear about where it does and does not come from:

* **Real:** opponent-board mirroring (§6d's 12–18 min) collapses to reading 8
  numbers a round, ~5 min, because the audit proves the rest is invisible.
* **Bookkeeping:** the row transcription was folded into §6d's "opponent turns"
  line and is now priced separately. It did not get cheaper; it got honest.
* **Not reducible at all:** the 30–45 minutes of actually playing the game
  dominates the total and no harness design touches it.

So: ten usable games is **10–16 hours**, fifteen is **15–24**. That is better
than the 12–18 hours previously quoted for ten, but not by enough to change the
recommendation. It is still 10–15 games for one coarse verdict, then stop. If
someone wants the next big saving it is in the row (~150 names/game) — screen
OCR, or naming only the cards the bot might actually take — not in anything else
on this list.

---

## 7. What can still go wrong silently

The checks are real but they are not complete. In descending order of how much
it would worry me:

1. **A "helpful" operator in strict mode.** If you play something the bot did
   not star, the harness records `source: human` — but if you *misread* the
   recommendation and play the wrong card in the app while pressing Enter here,
   nothing catches it until the next round's checksum, and only if it moved one
   of the five checked numbers. Slow, careful mirroring of your own moves is the
   one thing the harness cannot do for you.
2. **A drift that is invisible to the five spine numbers.** Culture, science,
   strength, food and resources do not cover everything: a wrong *tech level*,
   a wrong worker distribution, a missing wonder step, or a mis-entered card in
   your hand can all be consistent with correct totals for several rounds. The
   fuller `cr / sr / hap / ca / ma / fw / yel / blue / hc / hm` set is checked if
   you type it (`c=41 s=12 hc=4 ...`) but is not required. If you want a real
   belt-and-braces game, type the full set on the first round of each age.
3. **The row.** Answering `?` to a deal, or transcribing a card as a plausible
   wrong neighbour (`Iron` vs `Bronze`), is checked only by the *count* of cards
   in the row, not their identity. This is the largest uncovered surface, and it
   gets worse the moment the row features land.
4. **Rival numbers.** They are forced, so they are only as good as your reading.
   The arithmetic warning catches transposed digits in *culture*; nothing checks
   a misread strength, which is the one rival number that provably changes the
   bot's move.
5. **Events that hit you between your turns.** The `did pN hit YOU?` prompt is
   the only place they are captured. Forgetting one usually shows up in the next
   checksum — usually.
6. **App version drift.** CGE has re-tuned the AI in patches. `--app-version` is
   recorded but not verified; games months apart may not be the same opponent.

None of these produce a *plausible-looking* game silently for long — items 1, 2
and 5 all tend to surface within a round or two — except item 3, where a wrong
card sits in the row indefinitely and the bot simply evaluates the wrong option
set. That is the honest weak point of this design.
