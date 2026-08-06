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
rust/         the live implementation: engine, bots, self-play, the hill-climb
              league, the advisor and the app harness (rust/src/bin/)
engine/       the original Python rules engine and bot family -- superseded by
              rust/ for everything except one thing: it is still a live
              runtime dependency of the GPU neural-training loop
              (experiments/neural_search_loop.sh), which has not been ported
tools/        tools/gate.sh (the Python verification gate for engine/) and its
              two dependencies; everything else here was one-off and is gone
experiments/  the live Rust league (rust_league.sh) plus the still-Python,
              still-live neural self-play/training loop and its GPU-box
              deploy scripts (deploy/); the old Python hill-climb league is
              gone -- rust/src/bin/climb.rs replaced it
tests/        the Python unit suite -- kept because it covers engine/, which
              is still live (see above)
data/         card data, derived from the sources in docs/SOURCES.md
docs/         see docs/README.md
analysis/     analysis/frozen/ only: reference champion vectors the neural
              loop and several docs load by that path; everything else here
              was one-off and is gone
```

**Most of the Python in this repo is dead and has been deleted** (`advisor/`,
`harness/`, `exp_quiesce/`, most of `tools/`, `analysis/` and `experiments/`) —
`rust/` is the current implementation of the engine, every bot, self-play, the
hill-climb league, the advisor and the app harness. What survives under
`engine/`, `tests/`, `tools/gate.sh` and part of `experiments/` is not legacy:
it is a still-running Python/torch GPU training loop for the neural net
(scheduled on the desktop training box, see [`docs/DESKTOP_QUIET.md`](docs/DESKTOP_QUIET.md)
and [`docs/NEURAL_SEARCH_LOOP.md`](docs/NEURAL_SEARCH_LOOP.md)) that has not
been ported. Do not delete `engine/` without porting that loop first.

Build the Rust binaries with `cd rust && cargo build --release`; they land in
`rust/target/release/` (`arena`, `climb`, `selfplay`, `advisor`, `harness`).

The bots live in `engine/bots/`: `GreedyBot` (`__init__.py`), `WeightedBot`
(`weighted.py`, the linear evaluator), `QuiescentBot` (`quiescent.py`),
`PlanBot` (`plan.py`, the beam search), the book bots (`book.py`) and the
neural family (`neural_*.py`). [`docs/BOT_ROSTER.md`](docs/BOT_ROSTER.md) says what each is for.

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

Play a game through the app harness (Rust; see `rust/src/bin/harness.rs`):

```bash
rust/target/release/harness --players 3 --difficulty hard --app-version 2.4.1
```

Evaluate one bot against another, seat-balanced (either the Python wrapper,
still live, or the Rust binary directly):

```bash
python3 -m experiments.evaluate --a champion_4p.json --b greedy --games 120 --players 4
rust/target/release/arena --a challenger.json --b experiments/champion_4p.json --games 240 --threads 6
```

The self-play league is Rust now and is kept alive from cron by
`experiments/rust_league.sh`, which launches all three player counts (`climb`,
`rust/src/bin/climb.rs`). The separate, still-Python neural self-play/training
loop (`experiments/neural_search_loop.sh`) runs unattended on the desktop GPU
box; see [`docs/DESKTOP_QUIET.md`](docs/DESKTOP_QUIET.md).

---

## Two rules that have each already cost real work

**The fingerprint gate is not optional.** Eight arms — NARROW/WIDE
(GreedyBot), WNARROW/WWIDE (WeightedBot), QNARROW/QWIDE (QuiescentBot),
PNARROW/PWIDE (PlanBot) — hash a fixed batch of games per bot. A digest that
moves means behaviour changed; a digest that moves *unexpectedly* is a bug the
logs cannot show you after the fact. **Never re-derive a failing digest to make
the gate pass.** Explain why it moved first, on a clean clone, attributing each
moved arm to a specific cause. The derivation discipline is written down in
[`docs/PYPY.md`](docs/PYPY.md#90-a-trap-found-before-any-code-was-written-the-fingerprint-files-are-stale) §9.0.

**Do not run any git command in a working checkout while the league arms are
running** — not `pull`, not `checkout`, not `stash`, not even `status`. It
kills them. Stop an arm by touching
`experiments/logs/stop_rust_league_{2,3,4}p` (see `experiments/rust_league.sh`
— the sentinel stops a running arm immediately, not just the next relaunch),
or work from a clone under `/tmp`, as this deletion pass did.

---

## Housekeeping

`docs/README.md` exists so the tree does not grow back to sixty files. Before
adding a document, check whether the answer belongs in an existing one. An
investigation write-up whose question has been answered and whose fix has
landed should be folded into the relevant topic doc and deleted, not left
lying around.
