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

## The anchor number is not a strength ladder — read the gauntlet too

The anchor is only ever the built-in `DEFAULT_WEIGHTS`, and it never moves.
Every champion has long since run away from it: [`docs/HUMAN_PLAY.md`'s
"Diagnosing the 2p champion's passivity: anchor saturation, not a
measurement bug"](HUMAN_PLAY.md) measured the 2p champion at **89.6% ± 4.0**
against the anchor, but the *3p* champion's vector, played at a real 2p
table, beats the same anchor **93.8% ± 3.2** — statistically the same
margin — and the two champions play each other dead even, **54.4% ± 6.6**
and **50.8% ± 6.6** on two independent seeds. An "anchor=0.95" line in the
log says a champion is much better than a fixed point nearly everything
competent beats by now; it says nothing about whether *this generation* is
any better than the last one you actually looked at.

That is what the gauntlet is for: a second number, alongside the anchor, that
is comparable across time because its opponents never change.

**What it is.** `analysis/frozen/gauntlet/` holds dated, committed snapshots
of past champions — currently one per arm, cut 2026-08-06 at the generations
that were live then (`champion_2p_gen1454_140key_2026-08-06.json`, `_3p_
gen1384_`, `_4p_gen448_`). Unlike `experiments/rust_champion_{2,3,4}p.json`
(gitignored, live, rewritten every generation), these files never change
once committed. **Rule: gauntlet members are frozen forever.** A file already
in `analysis/frozen/gauntlet/` is never edited or replaced — new reference
points are only ever *added*, dated and named for their generation and key
count (`analysis/frozen/README.md`'s naming rule), exactly as `docs/HAZARDS.md`
warns for the older frozen vectors used in A/B harnesses (that directory's
own `README.md` — a stale reference answers a question with confident
silence, not an error, if the vocabulary or provenance is wrong). These are a
different collection for a different purpose (measuring the live league's
own progress, not feeding A/B harnesses) which is why they live in their own
`gauntlet/` subdirectory rather than mixed into the top-level frozen files.

**Why every arm plays all three, not just its own.** `experiments/
rust_league.sh` passes all three `--gauntlet` files to every arm, 2p included
— deliberately, because the finding above is that a same-lineage anchor
comparison alone cannot tell a genuinely stronger vector from a saturated
one, and cross-player-count matchups (a 3p-trained vector seated at a 2p
table) are exactly the check that surfaced it. Running that check
automatically, every cadence tick, going forward is the point.

**Cadence and cost.** `--gauntlet-every` (default 50 generations) and
`--gauntlet-games` (default 60 per member) — so, at 3 members, 180 games
every 50 generations, amortizing to **3.6 games/generation**. That is small
against a single generation's own spend: `--lambda` 2 mutants each buy
between `--screen` (24) and `--max-games` (240) games in `challenge()`, plus
`--anchor-games` (120) on any generation that has a promotion candidate — so
even the cheapest possible generation (48 games, an immediate two-mutant
reject) pays roughly 13× the gauntlet's amortized cost, and a typical one
pays far more. Every-generation gauntlet measurement was considered and
rejected for exactly that reason: it would roughly double the cheapest
generations' cost for a number that does not need per-generation resolution
— nothing about "am I still better than my ancestor from 50 generations ago"
changes fast enough to need finer sampling than that.

**The gate is untouched.** `measure_gauntlet` is called only for logging,
after this generation's accept/veto/reject decision has already been made
from `challenge()` and `Anchor::clearly_worse_than` — neither of which take a
gauntlet result as an argument (`gauntlet_measurement_does_not_influence_the_
accept_gate` in `rust/src/bin/climb.rs` pins this: the same challenge on the
same seed returns bit-identical results whether or not `Config::gauntlet` is
populated). The accept gate, the veto, the mutation operators and the
objective are exactly what they were before this section existed.

**Reading it.** `--log`'s JSONL gains a `"gauntlet"` array, always present
(empty except on cadence generations) so no line changes shape: `{"name":
"champion_2p_gen1454_140key_2026-08-06", "share": 0.83, "half": 0.06, "n":
60}` per member. `"anchor"`/`"anchor_half"` are untouched, so anything already
reading those two fields keeps working; `"gauntlet"` is new and additive.
`stdout` gets the same numbers appended to the generation line as
`gauntlet=[name=share,...]` when it ran that generation.

**What this does and does not prove.** A rising gauntlet score is real
evidence that the current champion beats a specific, dated, past version of
itself (or a sibling arm's past version) by more than that past version did
— a comparable number over time, which the anchor is not anymore. It is
**not** an external strength measurement. All three gauntlet members were
themselves produced by this same climb, against this same anchor, with this
same mutation operator and accept gate, so a bias shared by the whole
lineage — the same blind spot, the same class of move systematically
under- or over-valued — is invisible to it by construction: the gauntlet and
the current champion would drift together and the score would look fine
throughout. This is `docs/HAZARDS.md`'s "no external anchor" hazard, and the
gauntlet does not resolve it — it only narrows what "anchor saturation" can
hide by replacing a single fixed, long-outgrown point with several dated
ones spread across the lineage's own history. Treat a flat or falling
gauntlet score as a real signal something regressed; treat a rising one as
"better than these specific past selves," not as "strong" in any absolute
sense. `docs/HUMAN_PLAY.md`'s behavioural census against real games remains
the only check in this repo that is not trained by the same process it is
evaluating.

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
anchor standing, accept/veto, and — every `--gauntlet-every` generations —
gauntlet standing; see "The anchor number is not a strength ladder" above).
`experiments/logs/rust_league.log` is the supervisor's own relaunch log, and
`experiments/logs/rust_league_{K}p.log` is each arm's stdout/stderr.

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
