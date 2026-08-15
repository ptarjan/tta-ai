# Working in this repo

A Through the Ages AI — the 2015 base game "A New Story of Civilization", NOT the
expansion. (It is not Terraforming Mars.) The repo is private and stays private.

## Layout

Everything real is in `rust/`. `rust/src` is the engine, the bots, the advisor and
the analysis binaries; `cargo` must be run from `rust/`, and `$HOME/.cargo/bin` is
not on the default PATH.

The Python implementation that used to live in `engine/`, `advisor/`, `harness/` and
`tests/` was replaced by the Rust port and **deleted on 2026-08-15**. If you find a
reference to it, the reference is stale. Do not resurrect it: an agent once spent a
whole session fixing bugs in it and changed no measured result. The surviving Python
under `tools/` is standalone utilities only (e.g. `tools/scrape_bgo.py`, which needs
`--edition 2015`).

## Git

`/Users/pt/tta-ai` is the tree the training league runs from, and as of 2026-08-15 it
is a **normal checkout** — everything the league executes is tracked. Keep it that
way: commit and push regularly, and never leave live code untracked. Before this was
fixed, the engine sat untracked on top of a stale branch, and an agent that ran
`git reset --hard origin/master` to tidy up destroyed a day of work without anything
looking wrong afterwards.

`experiments/rust_champion_*.json` are deliberately untracked — the running climb
rewrites them every few minutes.

## The only metric that counts

The corpus of 1011 replayed human BGO games. Current: **748 complete / 721 exact**.
A clean build and green tests prove nothing about the engine — verify with the sweep
in `analysis/GUARD_METHOD.txt`, comparing ID SETS against the frozen guard list,
never a mean.
