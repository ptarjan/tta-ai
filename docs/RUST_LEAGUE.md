# The Rust league: three arms climbing against a fixed anchor (2026-08-06)

Replaces the now-deleted `docs/LEAGUE_TRAINING.md` (git history), which
documented the Python pool-based trainer —
`experiments/hillclimb_league.py`, its tiers, gate veto and ablation cursor.
None of that has a Rust equivalent: grepping `rust/src/` for a tier system, a
gate veto or an ablation cursor returns nothing, and `experiments/` no longer
has a `.py` file in it. This is a short reference for what actually runs
today, read from `rust/src/bin/climb.rs` and `experiments/rust_league.sh`
rather than from memory of the old design.

## What runs, and how

Three independent arms, one per player count (`climb --players 2`, `3`, `4`),
each its own champion file (`experiments/rust_champion_{2,3,4}p.json`).
`experiments/rust_league.sh` is the cron-kept-alive supervisor: it runs every
10 minutes, relaunches any arm that is not currently running, and launches
each with `--hours 6` so a long-lived process periodically re-execs — picking
up a rebuilt binary and re-reading its own checkpoint on the way in — without
anyone remembering to restart it by hand. Six physical cores split three ways
gives each arm `--threads 2`.

## The climb, per generation

A (1+λ) search, λ=2 mutants per generation by default. Each mutant plays the
**sitting champion directly** — not a pool or a field of past champions — at
the same table on the same deals, so the comparison is the game's own result:
one game per paired sample, null exactly `1/players`. A generation accepts a
mutant when the one-sided lower bound on its win share (`--accept-z`, default
1.2816 = 90%) clears the null; `challenge()` grows the batch (`--screen`,
default 24 games) up to `--max-games` (240), stopping early on a clear win
that has survived `--min-games` (default 2× screen) or abandoning early once
the mutant is clearly behind.

Step size (`sigma`) adapts by the 1/5th success rule over the trailing 12
generations — accept rate above 25% raises it ×1.25, below 12% shrinks it
×0.85, floored at `--sigma-floor` (0.08). After `--stall-kick` (default 15)
generations with no accept, one large forced mutation reopens sigma (up to
0.8) for a few generations instead of grinding the same neighbourhood. The
`culture` weight — the evaluation's numeraire — is frozen, and every weight is
clamped to ±60.

## The anchor and its drift veto

`ANCHOR` is a fixed weight vector — the built-in defaults unless `--anchor
PATH` overrides it — that never changes for the life of a run and is not
stored in the checkpoint at all (a compile-time constant needs no
re-supplying across restarts). Every accepted mutant is also measured against
the anchor (`--anchor-games`, default 120), and a promotion is **vetoed** when
the candidate's anchor standing is unambiguously worse than the champion's —
the two confidence intervals do not even touch. That is deliberately
conservative: the veto exists to catch a slide, not to referee noise.

This exists because the Python league's champions consistently measured worse
than the untrained starting vector even though every individual generation
had legitimately beaten its own parent — a self-play cycle that a
purely-relative accept rule cannot see. Asking the absolute question every
generation is the fix.

## Operator control

Touch `experiments/logs/stop_rust_league_{2,3,4}p` to stop one arm — the
supervisor checks the sentinel every cron pass and kills a running process
immediately, not just at its next natural exit; the same file also prevents
relaunch until removed. Every generation's full state (champion, generation
number, sigma, since-accept count, anchor standing) is checkpointed to
`--out` atomically, so a kill costs at most the generation in flight.

**Do not run git commands — not even `git status`— in the live checkout while
the arms are running; it kills them.** Work from a clone instead.

## Reading a run

`--log PATH` appends one JSON line per generation (op, share, lower bound,
anchor standing, accept/veto). `experiments/logs/rust_league.log` is the
supervisor's own relaunch log, and `experiments/logs/rust_league_{K}p.log` is
each arm's stdout/stderr.

## Which champion file is live?

**`experiments/rust_champion_{2,3,4}p.json` — these three, and only these
three.** They are what `climb` (via `experiments/rust_league.sh`) checkpoints
every accepted generation, `save_weights`-formatted (sorted keys, `gen`/
`sigma`/`since_accept`/`vs_anchor` alongside the vector). They are listed in
`.gitignore`, so they exist **only in a working checkout that has run the
league** — a fresh `git clone` will not have them, and neither will a
worktree that has never run `climb`.

Everything else with "champion" in its name is historical: `experiments/
champion_{2,3,4}p.json` was the last-ever snapshot from the retired Python
trainer (78 keys, frozen 2026-07-26) and now lives at `analysis/frozen/
python_champion_{2,3,4}p_..._2026-07-26.json`; the other files under
`analysis/frozen/` are earlier snapshots of the *current* Rust lineage, each
named for the generation and key count it was cut at (see that directory's
own `README.md`). None of them update themselves — only the three
`rust_champion_*.json` files do.

**A version note, not a bug:** the live champion files can carry fewer
weight keys than `WeightKey::ALL` in `rust/src/bots/weighted/weights.rs` —
130 vs. 133 as of 2026-08-06 — because the process writing them is running
whatever commit the working checkout was built from when it was last
(re)started, and `weights.rs` has grown three keys (`has_unit`,
`card_board_bonus`, `wonder_board_credit`) on `master` since. `save_weights`
itself always writes every key in `WeightKey::ALL` for the binary that calls
it (`rust/src/bots/weighted/eval.rs::a_champion_round_trips_through_text`
round-trips all of them); the three missing keys simply have not been
written yet by *this* run. They will appear, seeded at their defaults, the
next generation after the checkout is updated and the league is restarted.
