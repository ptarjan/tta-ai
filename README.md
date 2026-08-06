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
- **Training** — [`LEAGUE_TRAINING.md`](docs/LEAGUE_TRAINING.md) (Python-era
  mechanism reference; the live league is now `rust/src/bin/climb.rs`, see
  Layout below),
  [`NEURAL_SEARCH_LOOP.md`](docs/NEURAL_SEARCH_LOOP.md),
  [`MODEL_CONSTANTS.md`](docs/MODEL_CONSTANTS.md).
- **Human-facing output** — [`HEURISTICS.md`](docs/HEURISTICS.md) (carries a
  staleness caveat; read the evidence grades).

---

## Layout

```
rust/         the implementation. All of it: engine, bots, self-play, the
              hill-climb league, the neural net's forward pass AND its
              backprop trainer, the advisor and the app harness
              (rust/src/bin/)
experiments/  the two live drivers -- rust_league.sh (cron, the hill-climb
              league) and neural_search_loop.sh (Scheduled Task on the
              desktop, the neural loop) -- plus the champion vectors those
              read and the Windows deploy scripts (deploy/)
tools/        hidden_launch.vbs and wincheck.ps1, the Windows launcher that
              keeps Scheduled Tasks windowless and the check that proves it;
              plus the two fingerprint JSONs, kept as the recorded behaviour
              of the retired Python engine
data/         card data, derived from the sources in docs/SOURCES.md
docs/         see docs/README.md
sources/      the human game-log corpus
analysis/     analysis/frozen/ only: reference champion vectors the neural
              loop and several docs load by that path
```

**There is no Python left in this repo.** `engine/` (20,100 lines), `tests/`
(25,240), the Python half of `experiments/` (2,681) and `tools/gate.sh` +
`bug_audit.py` (1,334) are gone, along with `pytest.ini` and `ruff.toml`. The
last thing holding them was the neural self-play loop, which was a live
unattended pipeline calling six Python stages; every stage of it is now a Rust
binary. The one `.py` file that remains is
`sources/github_chellmuth_tta_cards.py`, which is archived third-party corpus,
not our code.

Build the Rust binaries with `cd rust && cargo build --release`; they land in
`rust/target/release/`:

| binary | what it does |
|---|---|
| `arena` | duel two weight vectors, seat-paired, deal-clustered interval |
| `neuraleval` | duel two arbitrary bot specs, either side a net checkpoint |
| `climb` | the hill-climbing league |
| `selfplay` | play N games and tally by bot and by seat |
| `rankdata` | generate value-net training shards from a teacher's self-play |
| `neuraltrain` | train the value net (backprop, AdamW, ranking + value loss) |
| `advisor` | interactive "what should I play next" |
| `harness` | interactive app-mirroring session |

The bots live in `rust/src/bots/`: `GreedyBot` (`greedy.rs`), `WeightedBot`
(`weighted/`, the linear evaluator), `QuiescentBot` (`quiescent.rs`),
`PlanBot` (`plan.rs`, the beam search), the book bots (`book.rs`) and the
neural family (`neural/`). [`docs/BOT_ROSTER.md`](docs/BOT_ROSTER.md) says
what each is for.

---

## Running things

Verify a change before committing:

```bash
cd rust && cargo test --profile difftest
```

Play a game through the app harness:

```bash
rust/target/release/harness --players 3 --difficulty hard --app-version 2.4.1
```

Evaluate one weight vector against another, seat-balanced:

```bash
rust/target/release/arena --a challenger.json --b experiments/champion_4p.json --games 240 --threads 6
```

Evaluate any two bot specs, including value-net checkpoints, seat-balanced.
A spec is `KIND[:PATH][,KEY=VALUE]...`; `neural:` is the net's 1-ply argmax
and `nplan:` is the whole-turn beam with the net as its leaf:

```bash
rust/target/release/neuraleval --a nplan:checkpoints/cand.ckpt,width=8 \
    --b plan:analysis/frozen/champion_2p.json,width=8 --games 240 --threads 6
```

A `KEY` the `KIND` does not read is an error rather than a no-op — a silently
ignored `width=` measures a different bot than the one you asked for.

The self-play league is kept alive from cron by `experiments/rust_league.sh`,
which launches all three player counts (`climb`). The neural self-play and
training loop (`experiments/neural_search_loop.sh`) runs unattended on the
desktop box under a Scheduled Task; see
[`docs/DESKTOP_QUIET.md`](docs/DESKTOP_QUIET.md) and
[`docs/NEURAL_SEARCH_LOOP.md`](docs/NEURAL_SEARCH_LOOP.md).

---

## Two rules that have each already cost real work

**A behaviour change must be explained, not absorbed.** The eight-arm
fingerprint gate that used to enforce this hashed a fixed batch of games per
bot under the Python engine, and retired with it; `cargo test --profile
difftest` is the gate now. The discipline it existed to impose has not
retired: when a test that pins measured behaviour starts failing, **never
re-derive the expected value to make it pass.** Explain what moved and why
first, attributing it to a specific cause. That rule was written down after it
was broken, in
`docs/PYPY.md` (deleted)
§9.0, and it is about people, not about Python.

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
