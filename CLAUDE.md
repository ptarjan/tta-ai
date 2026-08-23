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

**"Python did it this way" is never a reason to change behaviour.** `rust/src` still
carries ~1500 comments citing `engine/*.py`. They are provenance for how the port was
made, not authority for what is correct, and the file they name no longer exists to
check. The rules oracle is `docs/RULES_SPEC.md`, then `sources/faq_v15.pdf` and
`sources/cge_code_of_laws.pdf` (official errata), then `sources/` for card text. If a
comment's only support is Python, treat the comment as unverified — fix the comment,
not the code. On 2026-08-15 an agent "fixed" Impact of Population to match a Python
comment and cost 141 exact score matches; see
`analysis/worker_notes_2026-08-15/impact_of_population_pool_exclusion.txt`.

**Search the open web before you settle a rules reading.** The oracle order above
decides which source WINS; it does not tell you that you read the winner correctly,
and a misreading of `RULES_SPEC.md` looks exactly like a correct one from inside.
BoardGameGeek rules threads, CGE's own pages and the published summaries are cheap,
independent, and have caught a wrong reading here before. They are a CHECK, never an
authority: if the web disagrees with RULES_SPEC or the official errata, the errata
win and the disagreement is a finding worth writing down. BGG itself is
Cloudflare-protected and returns 403 to a direct fetch, so read it through search
results.

**A BGO journal is evidence of what BGO DID, never of what the rules ARE.** Reaching
for the corpus to settle a rules question inverts the oracle order. Argument from
silence over the journals is worth nothing in particular: on 2026-08-23 a government
develop was made CA-free because "the journal never prints a CA cost on a
`discovers <Gov>` line", when 269 of those 2185 lines do print one, the journal's
separate `revolutions Change government to <Gov>` phrasing already marks the other
path, and RULES_SPEC 8.2 and every outside summary say a peaceful change costs 1 CA.

## The lint gate

What makes a tree clean is `cargo clippy --all-targets -- -D warnings`, not `cargo
build`. `rust/Cargo.toml` denies the wildcard-match lints on purpose; CI runs the same
command and nothing else guards it.

**CI installs the latest stable toolchain, and this machine's may be older**, so a
locally green clippy does not prove the gate. On 2026-08-23 clippy 1.98 added
`chunks_exact_to_as_chunks` and turned three pushes red on code none of them touched,
while local clippy 1.97 exited 0 every time. Clippy also stops at the first error *per
target*, so fixing one site only reveals the next — grep for the pattern across
`rust/src` and fix every instance in one commit.

Before pushing a lint fix, run the gate under the newest toolchain rather than
guessing: `rustup toolchain install <ver> --profile minimal --component clippy`, then
`cargo +<ver> clippy --all-targets -- -D warnings` in a `cp -Rc` clone, so the main
tree's build cache and any live sweep are untouched. Prefer a fix that compiles on both
toolchains over `#[allow]`.

## Git

`/Users/pt/tta-ai` is the tree the training league runs from, and as of 2026-08-15 it
is a **normal checkout** — everything the league executes is tracked. Keep it that
way: commit and push regularly, and never leave live code untracked. Before this was
fixed, the engine sat untracked on top of a stale branch, and an agent that ran
`git reset --hard origin/master` to tidy up destroyed a day of work without anything
looking wrong afterwards.

`experiments/rust_champion_*.json` are deliberately untracked — the running climb
rewrites them every few minutes.

There is no rule anywhere that you need permission to commit. If you finished
something, commit it and push it. Do not hold a working tree full of finished work
waiting to be asked. Commit **by pathspec** — several agents share this index, and
`git add -A` swallows another agent's files. Landing a change and being right about
it are separate things: a rules-correct fix lands even when it costs corpus games,
but it lands with its regression list stated (§"The only metric that counts"), never
behind a total that happens to come out unchanged.

## The only metric that counts

The corpus of 1011 replayed human BGO games. Current: **878 of 1011 complete**, and
**836 of those 878 also score exactly**. (Exact is a subset of complete, so it is
always the smaller number; never write the pair as "878/836", which reads as a
ratio.) Guard lists `analysis/guard_ids_878.txt` and `analysis/guard_exact_836.txt`,
both frozen from the same sweep as this line — refreeze them together, from one
sweep, or a later `comm` compares two different engines.

Do not read `replaystats`' own printed total as the corpus figure without checking
what it counts. An agent reported 871 from a run that measured 866; the five extra
were the SET-ASIDE games, which are unresolved, not completions won.

A completion bought by *guessing* an unlogged choice is worth less than the error it
replaced. A `StuckPending` is the report that a mechanic is unmodelled; a fallback
that picks "the first option" completes the game with fabricated state and deletes
that report. If you add one, it needs a game ID showing what it buys — an unproven
guess is pure blindness.

**A correct fix that regresses the corpus is still a good fix.** The goal is a
perfectly correct engine, not a high completion count. "0 regressions" is an
acceptance convenience, not the standard: if the rulebook says one thing and the
corpus rewards another, the rulebook wins and the count goes down. Measure the
regression, name the games, and land it anyway.

The usual reason a rules-right change regresses is **two bugs that were cancelling
each other out**. Fixing one exposes the other, and the corpus — which only ever saw
their sum — reports the honest engine as worse. Keep the rules-right change. The
games it "lost" are not a cost, they are the bug report for the second defect, and
they are the only reason you can now see it. Record those IDs and hunt the partner
bug; do not revert the fix to put the mask back on.

A citation must say what the comment claims it says. `costs::can_take_bypass_hand_limit`
landed asserting International Agreement ignores the §2.5 hand limit, quoting CoL's
"may use this option even in the last round" — which is about the final round, not
hand size — while the FAQ's own International Agreement entry opens "The usual rules
for drafting cards apply." Quote the sentence, not the page. And a legality change in
`legal.rs` widens the BOT's move space, not just the replayer's tolerance: if BGO
allows something the rulebook does not, tolerate it in the replayer and leave the
engine correct.

`replaystats` also prints a count of non-matching games "SET ASIDE" by
`journal_arithmetic_error_suppression`. Those are UNRESOLVED, not proven corpus
errors — the gate fires when BGO's journal and BGO's index agree and the engine is
the lone dissenter, which is equally the shape of an engine bug. They are excluded
from the exact count, so that number can never inflate the headline.
A clean build and green tests prove nothing about the engine — verify with the sweep
in `analysis/GUARD_METHOD.txt`, comparing ID SETS against the frozen guard list,
never a mean.

**A failing test is evidence, not an obstacle.** If a test asserts the opposite of the
change you are making, it is the record of a fix someone already measured. Read its
doc comment and the doc comment of the state it touches before touching it; editing
the test to match your change deletes the only guard the repo had. `OneTimeDiscount`
is the standing example and is SETTLED: spending any one of its three candidate
discounts exhausts ALL THREE (`OneTimeDiscount::exhaust`). The three-independent-grants
reading of the JSON schema is wrong, was tried, and was disproven on real BGO games —
see the struct's doc comment in `state.rs`. Do not re-derive it.

**A scoring change is not gated by completions.** A wrong culture total still reaches
game over, so the completion set can be byte-identical while scores rot — the Impact
of Population regression moved completions 748 → 748 and exact matches 721 → 580.
Read the exact-match count on both sides, in one clone, or you have measured nothing.
