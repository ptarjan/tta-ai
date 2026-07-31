# Through the Ages AI Engine

A from-scratch rules engine, bot family and self-play training loop for
**Through the Ages: A New Story of Civilization (2015 edition, base game)**.

The scope is deliberately locked to the 2015 base game. **No expansion
content** — no Leaders of the Lost World, no new wonders, no new cards. A
feature that only exists in the expansion is out of scope, not a TODO.

The goal is a bot that beats the official app's hardest AI comfortably, and
from that, two human-facing artefacts: a playbook a person can follow, and a
protocol by which the engine can tell a human what to play next.

---

## Start here

Everything is documented under [`docs/`](docs/README.md), which has its own
index explaining which document answers which question. The three to read
first, in order:

| doc | answers |
|---|---|
| [`docs/OPEN_ITEMS.md`](docs/OPEN_ITEMS.md) | *What is still open?* The single register of unfinished work, deferred decisions and unanswered questions. |
| [`docs/HAZARDS.md`](docs/HAZARDS.md) | *What will bite me?* Standing traps, every one of which has already cost a real bug. Read before touching the training loop. |
| [`docs/SYSTEM_COVERAGE.md`](docs/SYSTEM_COVERAGE.md) | *How good is the bot right now, and what does it never do?* The current whole-system census against the 1,011-game human corpus. |

Then, by area:

- **The game** — [`RULES_SPEC.md`](docs/RULES_SPEC.md) (the only copy of the
  rules in this repo, every claim cited), [`SOURCES.md`](docs/SOURCES.md)
  (card-data provenance), [`EXPERT_STRATEGY.md`](docs/EXPERT_STRATEGY.md)
  (published human consensus, gathered independently of our bots).
- **The bot** — [`BOT_ARCHITECTURE.md`](docs/BOT_ARCHITECTURE.md),
  [`BOT_ROSTER.md`](docs/BOT_ROSTER.md),
  [`DEEPER_SEARCH.md`](docs/DEEPER_SEARCH.md),
  [`INFORMATION_AUDIT.md`](docs/INFORMATION_AUDIT.md).
- **Training** — [`LEAGUE_TRAINING.md`](docs/LEAGUE_TRAINING.md),
  [`LEAGUE_OBJECTIVE.md`](docs/LEAGUE_OBJECTIVE.md),
  [`NEURAL_SEARCH_LOOP.md`](docs/NEURAL_SEARCH_LOOP.md),
  [`MODEL_CONSTANTS.md`](docs/MODEL_CONSTANTS.md).
- **Human-facing output** — [`HEURISTICS.md`](docs/HEURISTICS.md) (carries a
  staleness caveat; read the evidence grades).

---

## Layout

```
engine/       rules engine and the bot family (engine/bots/)
harness/      play against the app, or drive a game interactively
experiments/  the self-play league, hill climb and neural search loop
tools/        one-off measurement scripts, censuses, A/B harnesses, the gate
tests/        the unit suite (~1130 tests)
data/         card data, derived from the sources in docs/SOURCES.md
docs/         see docs/README.md
analysis/     corpus analysis output
```

The bots live in `engine/bots/`: `GreedyBot` (`__init__.py`), `WeightedBot`
(`weighted.py`, the linear evaluator), `QuiescentBot` (`quiescent.py`),
`PlanBot` (`plan.py`, the beam search), the book bots (`book.py`) and the
neural family (`neural_*.py`). `docs/BOT_ROSTER.md` says what each is for.

---

## Running things

Verify a change before committing — static checks, the unit suite, and the
eight-arm fingerprint digests:

```bash
bash tools/gate.sh            # full: tests + both fingerprints, plain and paranoid
bash tools/gate.sh --fast     # tests + narrow fingerprint only (inner loop)
```

Just the tests:

```bash
python3 -m unittest discover -s tests
```

Play a game through the app harness:

```bash
python3 -m harness.play --players 3
```

Evaluate one bot against another, seat-balanced:

```bash
python3 -m experiments.evaluate --a champion_4p.json --b greedy --games 120 --players 4
```

Run the self-play league (normally kept alive from cron by the watchdog, which
launches all three player counts):

```bash
bash experiments/watchdog.sh
```

---

## Two rules that have each already cost real work

**The fingerprint gate is not optional.** Eight arms — NARROW/WIDE
(GreedyBot), WNARROW/WWIDE (WeightedBot), QNARROW/QWIDE (QuiescentBot),
PNARROW/PWIDE (PlanBot) — hash a fixed batch of games per bot. A digest that
moves means behaviour changed; a digest that moves *unexpectedly* is a bug the
logs cannot show you after the fact. **Never re-derive a failing digest to make
the gate pass.** Explain why it moved first, on a clean clone, attributing each
moved arm to a specific cause. The derivation discipline is written down in
`docs/PYPY.md` §9.0.

**Do not run any git command in a working checkout while league arms are
running** — not `pull`, not `checkout`, not `stash`, not even `status`. It
kills them. Stop the arms first by writing the sentinels
`experiments/logs/stop_league_{2,3,4}p.json` and running
`bash experiments/watchdog.sh` to reap, or work from a clone under `/tmp`.

---

## Housekeeping

`docs/README.md` exists so the tree does not grow back to sixty files. Before
adding a document, check whether the answer belongs in an existing one. An
investigation write-up whose question has been answered and whose fix has
landed should be folded into the relevant topic doc and deleted, not left
lying around.
