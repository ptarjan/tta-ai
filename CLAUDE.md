# Working in this repo

This is a Through the Ages AI (the 2015 base game, NOT the expansion). It is not
Terraforming Mars.

## This directory is NOT a normal checkout. Do not run git against origin here.

`/Users/pt/tta-ai` is the tree the training league runs from. Its local master is
deliberately unrelated to `origin/master`, and most of the live code is UNTRACKED.

    origin/master tracks 112 files under rust/.  This line tracks 19.

**NEVER `git reset --hard`, `git pull`, `git rebase`, `git clean`, or rsync
`--delete` from origin in this directory.** Doing so reverts those 19 files to an
older parallel port and clobbers ~93 untracked live source files. This happened on
2026-08-15 and permanently destroyed `rust/tools/`, `rust/logs/`, `rust/pgo/` and
`experiments/champ_backup_2026-08-14/`. Nothing looked wrong afterwards: the tree
compiled and all tests passed. Fixes land here BY HAND, file by file.

The `.gitignore` is whitelist-mode, so new files need `git add -f`. Commit by
pathspec (`git commit -- <paths>`), never `git add -A`.

## The dead Python tree

`engine/*.py`, `tools/*.py` and the rest of the Python implementation were replaced
by the Rust port in `rust/` and are run by nothing. Fixing them changes no measured
result. All engine work happens in `rust/src`.

## The only metric that counts

The corpus of 1011 replayed human BGO games. Current: **748 complete / 721 exact**.
A clean build and green tests prove nothing — verify with the sweep in
`analysis/GUARD_METHOD.txt`, comparing ID SETS against the frozen guard list, never
a mean. `cargo` must be run from `rust/`, and `$HOME/.cargo/bin` is not on PATH.
