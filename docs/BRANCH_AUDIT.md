# Branch audit: `master` vs `deeper-search`

Date: 2026-07-26. Audit performed read-only against `origin/master` (33d0ff1) and
`deeper-search` (42d86a1).

## How the divergence happened

An agent ran `git checkout -b deeper-search` in the *shared* working tree. Every
agent that subsequently committed in that tree landed its work on `deeper-search`
without noticing, while `git push origin master` kept pushing the untouched
`master` ref. Later agents worked around the problem by cherry-picking their work
onto `master` through throwaway worktrees. The result is two refs with heavily
overlapping content under different hashes.

At audit time:

- 23 commits on `deeper-search` not on `origin/master`
- 10 commits on `origin/master` not on `deeper-search`

**Neither ref is a superset of the other.** `deeper-search` is *not* simply
"ahead". This is the single most important fact in this document: a naive
diff-based apply of `deeper-search` onto `master` would have *deleted* real work,
including all 785 lines of `docs/EXPERT_STRATEGY.md`.

## Method

1. `git log --cherry-mark --left-right origin/master...deeper-search` to bucket
   commits into equivalent (`=`) vs unique (`<` / `>`) by patch-id.
2. Patch-id alone over-reports: `master`'s 10eb612 squashes seven separate
   `OPENING_AUDIT` commits from the branch, so those show as `>` (unique) even
   though the resulting file is byte-identical. Every `>` commit was therefore
   re-checked by comparing blob hashes of the files it touched.
3. `git diff --stat origin/master deeper-search` for the authoritative
   content-level accounting, which is what actually matters.

## Per-commit accounting: the 23 commits on `deeper-search`

### Already on `master` under a different hash (17 commits) — no action needed

Ten commits match by patch-id (`=`), landed by earlier agents' cherry-picks:

| Branch commit | Subject |
|---|---|
| cb2ce2c / a70f8fd | Add BookBot: a hand-written 'received wisdom' opponent |
| 9f965a1 / 42d86a1 | OPENING_AUDIT: match the headline A/B wording |
| 1116c75 / aecb6b7 | WASTED_ACTIONS: the fix makes the bot WORSE |
| 56d3f12 / 7c11e6c | Wasted actions: HorizonBot + waste-rate reporting |
| f6318cc / c59f106 | Wasted actions: PassFixBot + frozen champion snapshot |
| bb49f64 / f7d5ade | HEURISTICS: rule 8 is a defect report, not advice |
| 38554ca / 6cae854 | HEURISTICS: answer the reader's four questions |
| d667cb7 / 4ee59a1 | HEURISTICS: add per-age priority lists |
| e2f7cd4 / 98069a3 | HEURISTICS: add the turn-by-turn build order |
| 65c7629 / b07eb9f | Re-baseline the determinism digests |

Seven more are flagged unique by patch-id but are **content-identical** on
`master`, squashed into 10eb612 "Opening audit: final verdict, the wonder-weight
A/B, and the seed sweep":

5c0f369, 81e721c, f204b5c, 98297c5, 5fbe6ec, 22e6932, 840b567 — all touch only
`docs/OPENING_AUDIT.md`.

Proof: `docs/OPENING_AUDIT.md` is blob `6fc8c90` on **both** refs. Identical.
No content is lost by discarding these seven commits.

### Genuinely only on `deeper-search` (6 commits) — must be landed

| Branch commit | Subject | Files |
|---|---|---|
| db81c9b | QuiescentBot: resolve the pending stack before evaluating (step 1) | `engine/bots/quiescent.py`, `experiments/arena.py`, `tools/quiesce_bench.py` |
| cbaf76b | DEEPER_SEARCH: the design, and why quiescence rather than 2-ply | `docs/DEEPER_SEARCH.md` |
| 1e7b30b | QuiescentBot: nested quiescence (LEVELS) | `engine/bots/quiescent.py`, `experiments/arena.py` |
| 212eb0c | DEEPER_SEARCH step 3: the cost measurement, and why it is 1.2x not 20x | `docs/DEEPER_SEARCH.md` |
| 233b577 | tools/no_credit_check.py: deferred_credit stubbed to zero | `tools/no_credit_check.py` |
| 66435d6 | tools/behaviour_counts.py: per-game counts of 1-ply-blind move classes | `tools/behaviour_counts.py` |

This is the entire quiescence cluster and nothing else. Five new files plus one
additive hunk in `experiments/arena.py` (the `quiesce:` spec prefix).

## Content only on `master` (10 commits) — would be destroyed by a naive merge

`deeper-search` is missing all of this. It is real, wanted work:

- 33d0ff1 Strength check: 4p result, the hybrid null, two tournament diagnostics
- eb3b086 `docs/EXPERT_STRATEGY.md` (785 lines) — expert human strategy consensus
- 1627175 `STRENGTH_CHECK` large-n numbers (2p n=400, 3p n=300)
- 44fb2ee cardvalue: pin weight dicts so a recycled `id()` cannot serve stale potentials
- 9ac8614 WASTED_ACTIONS root-cause fix VALIDATED (67.2%, +22 culture)
- 38d2bc1 HEURISTICS: lead with what beat our AI
- 9083283 BookImprovedBot
- 9b666b5 WASTED_ACTIONS actionable verdict
- 233a00d Strength check vs the hand-written book bot
- 10eb612 Opening audit final verdict (the squash described above)

Files exclusive to `master`: `docs/EXPERT_STRATEGY.md`, `docs/STRENGTH_CHECK.md`,
`docs/HEURISTICS_TODO.md`, `analysis/cardvalue_duel.py`, `experiments/book_diag.py`,
`experiments/frozen/champion_{2,3,4}p_strengthcheck.json`. `master` is also strictly
ahead on `docs/HEURISTICS.md`, `docs/WASTED_ACTIONS.md`, `engine/bots/book.py` and
`experiments/bookmatch.py` — no branch-only commit touches any of those four.

## Training-restart checklist

Verified against `origin/master` before remediation:

| Item | Status before |
|---|---|
| `engine/bots/quiescent.py` (QuiescentBot) | MISSING |
| `quiesce:` prefix in `experiments/arena.py` | MISSING |
| military card-count fix (7d40f53) | present |
| rating clamp (5898006) | present |
| `engine/bots/book.py` | present |
| `docs/EXPERT_STRATEGY.md` | present |
| `docs/STRENGTH_CHECK.md` | present |
| `docs/OPENING_AUDIT.md` | present |
| `docs/AGGRESSION_FIX.md` | present |
| `docs/WASTED_ACTIONS.md` | present |
| `docs/HEURISTICS.md` | present |

## Remediation plan

Cherry-pick the six quiescence commits from `deeper-search` onto `master`, in
order, in a throwaway worktree. Nothing else needs to move. After that `master`
contains a strict superset of both refs and `deeper-search` can be retired.

## Note for other agents

A branch `land-quiescent` (worktree `/private/tmp/deeper_wt`) contains the same
six commits cherry-picked onto an *older* `master` (5 commits behind). It is
unpushed. Once the six commits are on `origin/master` that branch is redundant.
Do **not** apply it as a diff against current `master` — because it is based on a
stale `master`, its diff reverts `docs/EXPERT_STRATEGY.md`, `docs/HEURISTICS_TODO.md`
and parts of `STRENGTH_CHECK.md`/`WASTED_ACTIONS.md`/`HEURISTICS.md`.
