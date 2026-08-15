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

## Everything is Rust

The project began in Python and was ported to Rust. On **2026-08-15** the
Python implementation was deleted — `engine/`, `harness/`, `advisor/`,
`tests/` and the ~85 scripts that imported them. It had been superseded for
weeks and was run by nothing, but it was still on disk, and an agent spent an
entire session fixing bugs in it without changing a single measured result.

**Docs written before that date describe the Python tree.** Where a document
names `engine/…`, a `python3 -m …` command or a `.py` module, read it as
history: the behaviour it describes has an equivalent in `rust/src`, but the
path does not exist. The surviving Python under `tools/`, `analysis/` and
`experiments/` is standalone scripts only.

---

## Layout

```
rust/src/          the engine: rules, economy, combat, events, scoring
rust/src/bots/     the bot family, incl. bots/weighted (the linear evaluator)
rust/src/advisor/  the human-facing advisor
rust/src/bin/      ~30 analysis and training binaries (see below)
rust/tests/        integration tests; unit tests live beside the code
experiments/       the live league scripts and their measurement output
data/              card data, derived from the sources in docs/SOURCES.md
sources/           BGA's card text and the BGO human-game corpus index
analysis/          corpus analysis output and the worker measurement record
docs/              see docs/README.md
```

Useful binaries: `replaystats` (the corpus sweep — the project's headline
metric), `climb` and `selfplay` (the league), `advisor`, `replay`, `arena`,
`cardblame`, and the census family.

---

## Running things

`cargo` must be run from `rust/`, and `$HOME/.cargo/bin` is not on the default
PATH.

```bash
cd rust
cargo test --profile fasttest --lib          # ~22s, 1452 tests
cargo clippy --all-targets -- -D warnings    # ~25s; CI runs both
cargo build --release                        # ~6m; only for the live league
```

The league runs from a compiled release binary and is kept alive by cron every
ten minutes:

```bash
/bin/bash experiments/rust_league.sh         # start/resume all three arms
touch experiments/logs/stop_rust_league_2p   # then run the script again to stop 2p
pgrep -f 'selfplay|climb' | wc -l            # how many arms are actually up
```

---

## The only metric that counts

The **1011 replayed human games** from Board Gaming Online, swept by
`replaystats`. Current: **748 replay to completion, 721 of those score exactly
right.** A green build and a green test suite say nothing about whether the
rules are correct — only the sweep does.

Judge every change by **ID sets**, never by a mean: freeze the completed-game
IDs before, sweep after, and `comm` both directions so regressions and wins are
counted separately. The full procedure, and the traps it exists to avoid, are
in [`analysis/GUARD_METHOD.txt`](analysis/GUARD_METHOD.txt).

---

## Start here

| doc | answers |
|---|---|
| [`CLAUDE.md`](CLAUDE.md) | *What will confuse me in the first ten minutes?* |
| [`docs/RULES_SPEC.md`](docs/RULES_SPEC.md) | *What are the rules?* The oracle. Every claim cited. |
| [`docs/OPEN_ITEMS.md`](docs/OPEN_ITEMS.md) | *What is still open?* |
| [`docs/HAZARDS.md`](docs/HAZARDS.md) | *What will bite me?* Traps that each already cost a real bug. |
| [`docs/SYSTEM_COVERAGE.md`](docs/SYSTEM_COVERAGE.md) | *How good is the bot, and what does it never do?* |

Then by area: the bot in [`BOT_ARCHITECTURE.md`](docs/BOT_ARCHITECTURE.md) and
[`BOT_ROSTER.md`](docs/BOT_ROSTER.md); training in
[`LEAGUE_TRAINING.md`](docs/LEAGUE_TRAINING.md) and
[`LEAGUE_OBJECTIVE.md`](docs/LEAGUE_OBJECTIVE.md); human-facing output in
[`HEURISTICS.md`](docs/HEURISTICS.md).

---

## Housekeeping

Keep the working tree tracked and push regularly. `docs/README.md` exists so
the tree does not grow back to sixty files — before adding a document, check
whether the answer belongs in an existing one, and fold a finished
investigation into its topic doc rather than leaving it lying around.
