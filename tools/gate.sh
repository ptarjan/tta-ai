#!/bin/bash
# The verification gate for the journal/undo work (docs/PYPY.md section 6).
#
#   ruff clean               (static: F821 undefined-name and friends)
#   bug audit clean          (dynamic: no swallowed NameError/TypeError/...)
#   546 unit tests green
#   narrow fingerprint == 0a6ed6ad...   (33 games)
#   wide   fingerprint == 4a8c6ca6...   (102 games)
#   all of the above unchanged under FASTCOPY_PARANOID=1
#   plus the weighted / quiescent / plan arms -- one per bot that searches,
#   because a fingerprint can only catch a change to a bot it plays
#
# tools/fingerprint.json / tools/fingerprint_wide.json are now IN SYNC with
# the values below (re-saved via `perf_check save`) -- `python3 -m
# engine.perf_check check tools/fingerprint.json` is safe to use again. The
# digests this script gates on are still the ones written down here, not
# re-read from those files, so this comment (and PYPY.md, where this table is
# mirrored) remains the thing to update whenever a digest legitimately moves.
#
#   bash tools/gate.sh            # tests + both fingerprints, plain and paranoid
#   bash tools/gate.sh --fast     # tests + narrow only (quick inner loop)
set -u
cd "$(dirname "$0")/.."

# Re-derived on master 3439b0e (2026-07-26), per docs/PYPY.md 9.0's rule:
# compute from scratch on a clean detached worktree of master AND
# independently on a second worktree, and require the two to agree --
# agreement is the proof, not either number alone. Both narrow and wide moved
# this time (previously it was WIDE only, and NARROW's 3-seed greedy set was
# assumed too small to be touched -- this round it WAS touched, so nothing
# was assumed and all four were re-derived from scratch).
#
# Cause: commit 7315494 ("Coverage audit: census + variance instruments, 3
# rulebook fixes", see docs/COVERAGE_AUDIT.md Sec 2) changed engine/actions.py
# with two real rules fixes both bots' evaluation goes through --
# (1) `_h_revolution` no longer discards the actions the new government
# grants (Sec 2.1: a revolt from Despotism to Monarchy now correctly yields
# 3 military actions, not 2 -- Revolution has a 30-65% take-rate, so this
# moves many games), and (2) the one-per-name rule is no longer applied to
# yellow action cards, which exist in 2-3 copies per deck (Sec 2.2). Verified
# as the only behaviour-affecting change in range: `git diff 4886b65..3439b0e
# --stat` touches engine/actions.py only inside engine/; everything else in
# range (experiments/arena.py's degenerate-champion guard, experiments/
# summarize.py's feature grouping, the new standalone tools/coverage_census.py
# and tools/feature_variance.py) is additive/reporting-only and not on the
# perf_check hash path.
#
# Reconfirmed unchanged on master 52a4cb6 (2026-07-26, PYPY.md 9.19): the only
# engine/ change since 3439b0e is engine/bots/weighted.py (the resign guard,
# see WNARROW/WWIDE below), which GreedyBot's search never touches, so NARROW
# and WIDE were re-derived from scratch (not assumed) and landed on the same
# two values.
#
# Re-derived on `score-bugfix`, rebased onto master 9c8b6f5 (2026-07-27) and
# re-confirmed there.  Same two-sided discipline: computed from scratch in the
# working worktree AND independently in a second detached worktree of the
# commit, required to agree, and diffed PER CASE (33/102, key-by-key, not just
# the 8-char prefix) rather than by digest alone.  Negative control run too --
# perturb `Impact of Industry` by +1 on a scratch worktree, confirm the gate
# FAILs, restore, confirm it passes again.
#
# Cause: the four scoring fixes in docs/SCORE_BUGFIX.md.  ATTRIBUTED, not
# assumed -- each of the four was reverted on its own and all four arms
# re-hashed (docs/SCORE_BUGFIX.md 4):
#
#   * NARROW and WIDE moved for exactly ONE of them, `Impact of Population`
#     now counting unused workers (engine/events.py).  Reverting only that one
#     puts both back on 2fd656b3 / 1169007d to the byte, which is the proof
#     that the other three are inert for GreedyBot.
#   * WNARROW and WWIDE moved for that one AND for `Impact of Industry` now
#     scoring mine production instead of the resource rating.  Reverting both
#     `engine/events.py` hunks puts them back on 7fc72fca / 9dc0a5a6.
#   * The other two fixes -- Hollywood/Internet's one-time culture, and
#     Charlie Chaplin doubling one theater instead of a whole card -- move
#     NO arm.  Measured, not assumed: the fingerprint's bots essentially never
#     complete an Age III wonder (1 in 80 seat-games for the trained
#     production vector, 0 here), and neither GreedyBot nor DEFAULT_WEIGHTS
#     reaches Chaplin with two workers on its best theater.  Worth writing
#     down as a COVERAGE hole: these 135 games cannot catch a bug in either.
#
# UNCHANGED by docs/CARD_BLINDNESS.md (2026-07-29), and that is the load-
# bearing half of that change's attribution rather than a lucky no-op.  The
# only behaviour-affecting hunk there is two entries added to
# `_EFF_TO_FEATURE` in engine/bots/weighted.py, which is read by exactly one
# function, `_card_yields`, reached from exactly one place, `card_potential`.
# GreedyBot does not evaluate through weighted.py at all, so these two arms
# CANNOT move for it -- and they did not, in both independent derivations,
# while all SIX arms below moved.  If a future change to `_card_yields` ever
# moves NARROW or WIDE, that is a bug in the change, not a digest to update.
# (Superseded: NARROW and WIDE DID move on `military-discard`, and correctly --
# see the block below.  That change is not a `_card_yields` change.)

# ---------------------------------------------------------------------------
# Re-derived on `military-discard` (2026-07-30, commit 1c08790,
# docs/MILITARY_DISCARD.md).  ALL EIGHT arms moved -- the first entry in this
# file's history for which even the two GreedyBot arms move, and that is the
# tell that this is a change to the RULES rather than to an evaluator.
#
#     arm       old         new
#     NARROW    0a6ed6ad    bd0e9a62
#     WIDE      4a8c6ca6    cf4f0a22
#     WNARROW   5eff41eb    549e4a90
#     WWIDE     d03e0964    0e03e3b7
#     QNARROW   eff1bef5    b15d7b18
#     QWIDE     9e9695d4    bf221746
#     PNARROW   c534ac3d    d307c480
#     PWIDE     ee627d64    4d71894c
#
# Cause, and it is one rule: RULES_SPEC 6.6 step 1, the end-of-turn military
# discard, was `hand_military.pop(0)` -- FIFO, taken with no decision at all.
# The rulebook makes it the player's choice and says it is the ONLY decision in
# the end-of-turn sequence (sources/ubg_subsequent-rounds.txt:182, "Once you
# have decided which military cards to discard, the rest of your turn is
# automatic").  It is now a real `push_choice`.
#
# WHY ALL EIGHT, INCLUDING GREEDY.  Every previous entry in this file moved
# only the arms whose bot searches under `weighted.evaluate`, because the cause
# was always a change to what the evaluator could see.  This one changes the
# MOVE STREAM: a decision that did not exist now appears in it, ~31 times per
# 2p game (tools/discard_census.py).  `perf_check` hashes the full game log,
# the final scores, the winners and the move count, so a bot that does not
# evaluate at all -- RandomBot, GreedyBot -- still hashes differently, because
# it is being asked a question it was never asked before and its answer is in
# the log.  An arm that did NOT move here would be the surprising result.
#
# ATTRIBUTED, not assumed, three ways:
#
#   * BASELINE.  All ten arms re-derived from scratch on the PARENT commit
#     (7b183fe) came back byte-identical to the eight old values above-left.
#     Necessary rather than ceremonial: master moved 19 commits while this was
#     being written, including an end-of-game scoring audit that touched
#     engine/events.py, so "the constants in this file are still master's" was
#     checked rather than believed.  (It also means that scoring audit moved no
#     digest, which is worth knowing next door.)
#   * TWO-SIDED.  Derived from scratch in the working clone AND independently
#     in a second clone at the same commit, per docs/PYPY.md 9.0.  The two
#     agreed on all eight.
#   * REVERT CONTROL, by construction rather than by a third run.  The only
#     files in 1c08790 on `perf_check`'s import path are engine/economy.py,
#     engine/game.py and engine/interact.py; advisor/, analysis/, tests/,
#     tools/ and docs/ are never imported by it.  Reverting those three files
#     IS the parent tree, whose arms are the old values above -- the baseline
#     run above therefore is the revert control.
#
# The two FASTCOPY_PARANOID arms agree with their plain counterparts
# (bd0e9a62, cf4f0a22), so the new decision does not disturb fastcopy.
#
# Nothing here was re-derived to make a gate pass: the gate FAILED on 1c08790
# by design, and these eight values are the result of computing the new
# behaviour, not of reading it back off a failure message.  Two arms (QWIDE,
# PWIDE) first came back with a BLANK digest -- that is a killed subprocess,
# not a moved hash, and they were re-run rather than recorded.
# ---------------------------------------------------------------------------
# Re-derived on `scoring-fixes`, rebased onto master efa37b5 (2026-07-30,
# docs/SCORE_AUDIT.md).  ALL EIGHT arms moved.  Cause: nine rules fixes to
# end-of-game scoring, every one a rule violation rather than a tuning
# choice, so they ship regardless of measured strength.
#
#     arm       old         new
#     NARROW    bd0e9a62    cd0971ed
#     WIDE      cf4f0a22    77c81e82
#     WNARROW   549e4a90    f0b240da
#     WWIDE     0e03e3b7    9010ec80
#     QNARROW   b15d7b18    ad62a4e5
#     QWIDE     bf221746    caf7cdd7
#     PNARROW   d307c480    85c06781
#     PWIDE     4d71894c    12b1dce0
#
# Two-sided as 9.0 requires, and then some.  Derivation 1 in the working
# worktree and derivation 2 in an independent clone of efa37b5 with the same
# patch applied AGREED ON ALL EIGHT ARMS, byte for byte.  A clean checkout of
# efa37b5 was hashed first as a control and reproduced the OLD column above
# exactly (NARROW bd0e9a62, WNARROW 549e4a90), so the base was known-good
# before anything of mine was measured against it.  The attribution run below
# independently reproduced BOTH endpoints a third time.
#
# ATTRIBUTED, not assumed.  Each fix was reverted on its own from the
# all-fixed tree and `narrow` + `weighted narrow` re-hashed
# (docs/SCORE_AUDIT.md 9.1):
#
#   LIVE for GreedyBot AND WeightedBot:
#     * `Impact of Happiness`/`Immigration`'s "the PLAYERS with the most X"
#       now affecting every tied player (RULES_SPEC 5.3 [CoL p.7]), and
#     * "your best lab or library" no longer counting an UNSTAFFED card,
#       which is Leonardo/Newton/Einstein and is the single widest-reaching
#       fix in the set.
#   LIVE for GreedyBot only:
#     * a wonder ruined by Ravages of Time no longer feeding Michelangelo,
#     * Winston Churchill's military option being ring-fenced.
#   LIVE for WeightedBot only:
#     * `Impact of Agriculture` scoring farms instead of the food rating.
#       Note this one is live only because the fingerprint plays 4p: at 2
#       players every pact is removed from the game and a pact's food symbol
#       is the only thing that can separate farm food from the food rating.
#       I predicted this fix could not move any arm and the measurement
#       corrected me, which is the argument for attributing rather than
#       reasoning.
#   INERT for both (measured, not assumed):
#     * Bill Gates paying culture when he LEAVES play,
#     * St. Peter's ignoring a ruined wonder, and ignoring a colony,
#     * the air force doubling an outdated army at the fresh rate,
#     * Taj Mahal's blue token in board_yields (it cannot move an arm today:
#       `card_board_credit` defaults to 0.0 and GreedyBot does not evaluate
#       through weighted.py at all).
#
# A note for the next person to read a `check_fp` FAIL here: during the first
# attempt at this derivation, two other lanes were running an unscoped
# `pkill -f "engine.perf_check"`, which kills EVERY lane's hasher.  Three runs
# died that way and each appeared as a FAIL with a *blank* "got" field.  That
# is a killed subprocess, not a moved hash.  Those runs were discarded and the
# whole derivation was redone in a quiet window rather than written down.
# ---------------------------------------------------------------------------
# Re-derived on `a7a5ef1` ("The victor of a War over Technology chooses",
# 2026-07-30, docs/WAR_OVER_TECHNOLOGY.md).  SIX of the eight arms moved --
# NARROW, WIDE, QNARROW, QWIDE, PNARROW, PWIDE -- and WNARROW/WWIDE did not.
#
#     arm       old         new
#     NARROW    cd0971ed    ca255af3
#     WIDE      77c81e82    f223cea1
#     WNARROW   f0b240da    f0b240da   (unchanged)
#     WWIDE     9010ec80    9010ec80   (unchanged)
#     QNARROW   ad62a4e5    9ad67497
#     QWIDE     caf7cdd7    e83054f7
#     PNARROW   85c06781    32a99881
#     PWIDE     12b1dce0    f7a092a2
#
# Two-sided as 9.0 requires: derived independently in /tmp/wardigest-a and
# /tmp/wardigest-b, two separate clones of the same commit, and the two agreed
# byte-for-byte on all eight arms including the two that did not move.
#
# CLEAN-BASE CONTROL FIRST, and it passed: a full gate on the parent 75f780f
# reproduced all eight of the previously committed constants exactly (GATE
# PASS).  So the base was known-good and these six moves are attributable to
# this commit rather than to drift that a re-derivation would have buried.
#
# Cause: `events.resolve_war` is no longer total.  A won War over Technology
# now leaves the victor a pending `war_tech` choice, and the spoils are settled
# through `interact.settle_war_spoils` instead of being taken as science
# unconditionally.  That changes the move sequence on every seed that resolves
# such a war, which is why the two GreedyBot arms (NARROW/WIDE) moved for once:
# unlike an evaluator change, this is a RULES change and GreedyBot plays the
# rules.  WNARROW/WWIDE holding still is the informative part -- WeightedBot
# under DEFAULT_WEIGHTS never declares a war in the 33/102-game fingerprint, so
# its arms cannot see the change at all.  That is consistent with the war
# lane's own census, which found zero war declarations in 52 default-weight
# games, and it is a measurement of how rare the card is, not of its value.
NARROW=ca255af3
WIDE=f223cea1

# The greedy fingerprint above plays GreedyBot ONLY, which is exactly why four
# master rebases left it untouched (9.0/9.6) -- and exactly why it can never
# catch a change to WeightedBot, the bot the league actually trains (9.14).
# These two are the same 33/102 split played by WeightedBot instead.
#
# Re-derived on master 52a4cb6 (2026-07-26, PYPY.md 9.19), same two-sided
# discipline as always -- fresh detached checkout vs. a second worktree,
# full per-case JSON diffed key-by-key, not just the two hex prefixes below.
#
# Cause: commit fb9c12a ("WeightedBot: guard against resign, as RandomBot
# always has") added an `allow_resign=False` guard to WeightedBot.pick() that
# filters `("resign",)` out of the legal moves whenever a non-resign move
# exists. `git diff 3439b0e..52a4cb6 --stat -- engine/` touches exactly two
# files: engine/bots/plan.py (new, additive -- PlanBot, not reachable from
# perf_check) and engine/bots/weighted.py, whose full diff across that whole
# range is byte-for-byte fb9c12a's 18-line resign guard and nothing else --
# confirmed by reading the diff, not by assuming the commit message. Under
# DEFAULT_WEIGHTS (what the fingerprint plays), a resign move is apparently
# live and gets chosen on some seeds; under the trained champions' weights it
# is not (fb9c12a's message: "byte-identical for the trained champions"),
# which is exactly why only the two WEIGHTED arms moved and NARROW/WIDE
# (GreedyBot, which never resigns either way) did not.
#
# Re-derived on `score-bugfix`, rebased onto master 9c8b6f5 (2026-07-27), alongside
# NARROW/WIDE above; see the attribution note there.  These two moved for two
# of the four fixes rather than one.
#
# ---------------------------------------------------------------------------
# Re-derived on `wonder-identity` (2026-07-29, docs/CARD_BLINDNESS.md).  All
# SIX of the arms below -- WNARROW, WWIDE, QNARROW, QWIDE, PNARROW, PWIDE --
# moved, and NARROW/WIDE above did not.  Two-sided as 9.0 requires: computed
# from scratch in the working worktree AND independently in a second detached
# worktree of the parent commit with the same one-file patch applied, the two
# required to agree.  They agreed on all eight arms.
#
#     arm       old         new
#     NARROW    0a6ed6ad    0a6ed6ad   (unchanged -- GreedyBot)
#     WIDE      4a8c6ca6    4a8c6ca6   (unchanged -- GreedyBot)
#     WNARROW   302c546c    5eff41eb
#     WWIDE     4e40a58c    d03e0964
#     QNARROW   0e90a7e6    eff1bef5
#     QWIDE     41f078e5    9e9695d4
#     PNARROW   ad64a55b    c534ac3d
#     PWIDE     441cd256    ee627d64
#
# Cause, and it is a ONE-LINE-PAIR cause: `_EFF_TO_FEATURE` in
# engine/bots/weighted.py gained `"culture": "culture_rate"` and
# `"science": "science_rate"`.  Those two keys are how ten cards spell their
# per-turn culture (Eiffel Tower 4, Taj Mahal 3, St. Peter's 2, Kremlin 2,
# Library of Alexandria, Universitas Carolina, Great Wall, Hanging Gardens,
# Joan of Arc, Mahatma Gandhi) and two spell science, and `_card_yields` was
# dropping every one of them on the floor.  `engine/effects.py:FLAT_KEYS`
# already treats the short spelling as production, so this is the evaluator
# catching up with the rules engine, not a new opinion.
#
# ATTRIBUTED, not assumed.  The rest of the change -- nine new weights, the
# `_EFF_SPECIAL` table, the `_Y_GAIN/_Y_COST/_Y_RATE` kind flag, the finish-
# discipline features -- landed in the PARENT commit and all eight arms were
# derived on it and came back byte-identical to the eight values above-left.
# So the six moves here are the two map entries and nothing else.  The whole
# change is additionally switchable at runtime on the `card_rate_credit`
# weight: setting it to 0.0 restores master's pricing exactly, which is what
# docs/CARD_BLINDNESS.md section 5 A/Bs against.
# Both moved again on `military-discard`; see the block above NARROW.
#
# ---- unit-technology board pricing (docs/UNIT_TECH_PRICING.md) -------------
#
# SIX of the eight arms moved -- every arm whose bot evaluates through
# `weighted.card_potential`, and no other.  The two GreedyBot arms
# (NARROW/WIDE) held still, which is the informative half: GreedyBot does not
# call `card_potential` at all, so an arm of it moving would have meant the
# change had leaked into the rules.
#
#     arm       old         new
#     NARROW    ca255af3    ca255af3   (unchanged -- GreedyBot)
#     WIDE      f223cea1    f223cea1   (unchanged -- GreedyBot)
#     WNARROW   f0b240da    dc1e3bbe
#     WWIDE     9010ec80    f401b342
#     QNARROW   9ad67497    02f63fe7
#     QWIDE     e83054f7    2f1f774e
#     PNARROW   3e428ad2    b17d2aa1
#     PWIDE     fc990004    d2240d3c
#
# Cause: `DEFAULT_WEIGHTS` gained `unit_tech_credit` at 1.0, which routes a
# unit technology's price through `weighted.unit_tech_value` -- an
# `effects.compute` upgrade diff valued at `strength_marginal` -- instead of
# through the static `_card_yields` table that charged a fresh build and
# credited a strength nobody believed.  Every unit card in the row and in the
# civil hand therefore prices differently under DEFAULT_WEIGHTS, and all
# three searching bots see it.
#
# ATTRIBUTED, not assumed, and to ONE constant: a third clone of this exact
# tree with `"unit_tech_credit": 1.0` changed to `0.0` and nothing else
# touched reproduces all EIGHT pre-change digests byte for byte.  So the six
# moves are the credit and nothing else in the change -- the refactor itself
# (the new module functions, `rival_strength`, `_is_unit`) is provably inert.
#
# Two-sided as docs/PYPY.md 9.0 requires: derived from scratch in /tmp/dig-a
# and independently in /tmp/dig-b, two separate clones of the same commit, and
# the two agreed on all eight arms -- including the two that did not move.  A
# clean-base control on the parent commit (25740b1, /tmp/gate-base) reproduced
# every pre-change constant first.  Nothing here was re-derived to make the
# gate pass; the gate FAILED by design in both clones and these are the
# computed values.
#
# ---- ALL technology board pricing (docs/YELLOW_TECH_PRICING.md) ------------
#
# The same SIX arms moved again, for the same structural reason and with the
# same two that held: NARROW/WIDE are GreedyBot, which never calls
# `weighted.card_potential`.
#
#     arm       old         new
#     NARROW    ca255af3    ca255af3   (unchanged -- GreedyBot)
#     WIDE      f223cea1    f223cea1   (unchanged -- GreedyBot)
#     WNARROW   dc1e3bbe    16dc9a1a
#     WWIDE     f401b342    a1b74078
#     QNARROW   02f63fe7    2f59c5c0
#     QWIDE     2f1f774e    23b8d66e
#     PNARROW   b17d2aa1    15bd49fc
#     PWIDE     d2240d3c    c8fe5d3a
#
# Cause: `DEFAULT_WEIGHTS` gained `tech_board_credit` at 1.0, which routes
# EVERY technology's price -- the eleven farm/mine/lab/urban/special types,
# and the `tech_levels` half of the four red ones -- through
# `weighted.tech_value`.  That prices a card by an `effects.compute` upgrade
# diff plus the technology levels it buys, each valued at
# `weighted.feature_marginal` (d(evaluate)/d(feature), phase multipliers
# included) instead of at the bare `w[k]` the static `_card_yields` table
# used.  Every technology in the row and in the civil hand therefore prices
# differently under DEFAULT_WEIGHTS, and all three searching bots see it.
#
# ATTRIBUTED, not assumed, and to ONE constant: a third clone of this exact
# tree with `"tech_board_credit": 1.0` changed to `0.0` and nothing else
# touched reproduces all EIGHT pre-change digests byte for byte.  So the six
# moves are that credit and nothing else in the change -- `feature_marginal`,
# `tech_upgrade`, `_delta_triples`, `_upgradable_onto` and `_is_levelled_tech`
# are provably inert on their own.
#
# Two-sided as docs/PYPY.md 9.0 requires: derived from scratch in /tmp/gateA
# and independently in /tmp/gateB, two separate copies of the same tree, and
# the two agreed on all eight arms -- including the two that did not move.  A
# clean-base control on the parent commit (c0525c4, /tmp/base) reproduced all
# eight pre-change constants first.  Nothing here was re-derived to make the
# gate pass.
# ---- the horizon: a measured deal rate and an exact gauge -----------------
#     (docs/MODEL_CONSTANTS.md)
#
# The same SIX arms moved, and the same two held.  NARROW/WIDE are GreedyBot,
# which calls neither `rounds_left` nor `lateness` -- a GreedyBot arm moving
# here would have meant the change had leaked out of the evaluator.
#
#     arm       old         new
#     NARROW    ca255af3    ca255af3   (unchanged -- GreedyBot)
#     WIDE      f223cea1    f223cea1   (unchanged -- GreedyBot)
#     WNARROW   16dc9a1a    6d888d7c
#     WWIDE     a1b74078    c52302c2
#     QNARROW   2f59c5c0    bbbb203a
#     QWIDE     23b8d66e    3df0155f
#     PNARROW   15bd49fc    1b883d6f
#     PWIDE     c8fe5d3a    3922ebc4
#
# Cause, and it is TWO causes in one commit, attributed separately below:
#
#   (1) `rounds_left` no longer divides by the fitted `CARDS_PER_ROUND`
#       {2: 6.29, 3: 6.73, 4: 5.71}.  The sweep half of the deal rate is
#       `n * SWEEP[n]` and is exact (RULES_SPEC 2.1); the take half is now
#       MEASURED in the game being played, off two public counts.  The fitted
#       constant assumed 0.29 takes/round at 2p where the current defaults
#       take 1.88, which left the horizon 1.80 rounds LONG.
#   (2) `lateness` is no longer the fitted affine map
#       `(z - rounds_left)/(z - 5)` with `z = _L_ZERO[n]`.  It is
#       `1 - cards_unseen/supply` -- the exact fraction of the civil card
#       supply already dealt, with both endpoints rule-derived.
#
# The third constant in that commit, `RIVAL_TAKE_P = 0.25` -> a per-rival
# estimate off the rival's open board, CANNOT move an arm and did not:
# `row_bargain_forgone` defaults to 0.0 and `evaluate` skips `row_pressure`
# entirely when both row weights are zero.  That is measured below, not
# assumed.
#
# CLEAN-BASE CONTROL FIRST, and it passed: a full gate on the parent 8b972ef
# in /tmp/constbase reproduced all eight committed constants exactly (GATE
# PASS, 1070 tests).  So the base was known-good before anything of mine was
# measured against it.
#
# TWO-SIDED as docs/PYPY.md 9.0 requires: derived from scratch in
# /tmp/constfix and independently in /tmp/constgateB -- a second fresh clone
# of 8b972ef with the same file set copied onto it -- and the two agreed
# BYTE-FOR-BYTE on all eight arms, including the two that did not move.
#
# ATTRIBUTED, not assumed, and by ENVIRONMENT rather than by editing a third
# clone.  `engine/bots/weighted.py` reads three A/B hatches from the
# environment at import (`TTA_LEGACY_DEAL_RATE`, `TTA_LEGACY_LATENESS`,
# `TTA_LEGACY_ROW_TAKE`), each restoring exactly one retired constant.  That
# is a STRONGER control than a patched clone: the tree being hashed is
# byte-identical to the tree being shipped, so nothing but the named cause can
# differ.  From /tmp/constattr, on this exact tree:
#
#   tree / hatches set                NARROW    WNARROW   QNARROW   PNARROW
#   parent 8b972ef                    ca255af3  16dc9a1a  2f59c5c0  15bd49fc
#   all three hatches ON              ca255af3  16dc9a1a  2f59c5c0  15bd49fc
#   ROW-TAKE hatch only               --        6d888d7c  bbbb203a  1b883d6f
#   new rate + LEGACY gauge           ca255af3  7ed600d1  487b2aa5  c1d0caea
#   new gauge + LEGACY rate           ca255af3  6d888d7c  bbbb203a  1b883d6f
#   shipped (both new)                ca255af3  6d888d7c  bbbb203a  1b883d6f
#
# (WWIDE/QWIDE/PWIDE follow the narrow arms; the all-hatches-on row was run on
# all eight and reproduced c8fe5d3a / a1b74078 / 23b8d66e too.)
#
# Four things fall straight out of that table, and the third is the one worth
# reading twice.
#
#   * ALL THREE HATCHES ON REPRODUCES THE PARENT'S EIGHT.  That is the strong
#     form of "everything else in this commit is inert": the renames, the
#     comments, the new `rival_take_share` key in DEFAULT_WEIGHTS and the `w`
#     threaded through `features()` are provably behaviour-free.
#   * THE ROW-TAKE HATCH ALONE REPRODUCES THE SHIPPED DIGESTS.  So replacing
#     `RIVAL_TAKE_P` moves nothing here, exactly as predicted from
#     `row_bargain_forgone` defaulting to 0.0.  Measured, not assumed.
#   * CAUSE (2), THE GAUGE, IS THE WHOLE MOVE.  New gauge + LEGACY deal rate
#     already lands on the shipped digests, byte for byte, on all three arms.
#   * CAUSE (1), THE DEAL RATE, IS INERT ON THESE ARMS -- AND NOT BECAUSE IT
#     DOES NOTHING.  With the LEGACY gauge on it moves WNARROW to a third
#     value (7ed600d1) that is neither the parent's nor the shipped one, so
#     the plumbing is live.  It is inert in the SHIPPED combination for a
#     structural reason that was checked in the source rather than inferred
#     from the hash: the new `lateness` is `1 - cards_unseen/supply` and does
#     not call `rounds_left` at all, and the only other consumer of
#     `rounds_left` inside `evaluate` is the `wonder_overrun` feature, whose
#     weight is 0.0 in DEFAULT_WEIGHTS.  The fingerprint plays DEFAULT_WEIGHTS.
#     So under these weights `rounds_left` has NO path to the evaluation, and
#     an arm moving for cause (1) would have meant one of those two statements
#     was wrong.  A trained vector with a non-zero `wonder_overrun` WOULD see
#     it, and so does `neural_encode`.
#
# Nothing here was re-derived to make the gate pass.  The gate FAILED on this
# tree by design, in both clones, and these are the computed values.
#
# Test count goes 1070 -> 1087, accounted for exactly: +10 from the new
# tests/test_model_constants.py, +4 net in tests/test_horizon.py (five new,
# and `test_calibration_against_the_old_schedule` removed -- it asserted the
# new gauge stayed within 0.10 of the OLD age bucket, which was the
# champion-compatibility constraint the honest gauge deliberately gives up),
# and +3 in tests/test_row_features.py.
# UNCHANGED by the Ocean Liners repricing (docs/MODEL_CONSTANTS.md 9), and
# that no-op is a PREDICTION THAT WAS TESTED rather than a lucky result.
# `board_yields._free_pop_increase` is a `WONDER_RIDERS` entry, reached only
# from `card_potential`'s wonder branch, which is gated on
# `card_board_credit + card_board_wonder` -- both 0.0 in DEFAULT_WEIGHTS.  The
# fingerprint plays DEFAULT_WEIGHTS, so the handler is UNREACHABLE on all eight
# arms and every one of them had to hold.  All eight did, in two independent
# clones (/tmp/constfix and a fresh clone of 7bf483a with the same file set).
# If a future change to `_free_pop_increase` ever moves an arm, that is the
# change escaping its gate, not a digest to update.
# Test count 1087 -> 1093: +6 from
# tests/test_board_yields.py:TestOceanLinersIsPricedByTheBoardNotByARate.
WNARROW=ba77b499
WWIDE=f4d6a545

# ...and the same argument one bot further on (docs/PYPY.md section 10).
# `experiments/run_league.sh` trains `--candidate-bot plan:width=2` at 2p and
# `--candidate-bot quiescent:levels=1` at 3p/4p.  NEITHER of those bots was
# played by any arm above, so before these four, no digest in this project
# could catch a change to PlanBot or QuiescentBot -- the 9.14 hole exactly,
# one league re-target later.  Section 10 converts both to the undo stack, so
# it could not be merged without them.
#
# Sized by cost rather than by symmetry with the 33/102 greedy split: a 2p
# PlanBot game is ~4 cpu-s and a 4p one ~16, against ~0.15 for a greedy game.
# plan narrow is 3 games, plan wide 6; quiescent narrow 9, quiescent wide 24.
#
# Derived at master 419012e on the pre-conversion tree (the tools/perf_check
# commit, engine behaviour untouched) and independently in a second detached
# checkout of the same commit, per 9.0's rule -- the two agreeing is the
# proof, not either number alone.
#
# All four moved on `wonder-identity` (2026-07-29); see the table and the
# attribution in the WNARROW/WWIDE block above.  PlanBot and QuiescentBot both
# search under the same `evaluate`, so `card_potential` is on their hash path
# exactly as it is on WeightedBot's.
# All four moved again on `military-discard`; see the block above NARROW.
# All four moved again on `war-over-technology`; see the block above NARROW.
#
# ---------------------------------------------------------------------------
# Re-derived on the drain flip (2026-07-30, docs/DRAIN_AB.md).  EXACTLY TWO of
# the eight arms moved -- PNARROW and PWIDE -- and the other six did not.
#
#     arm       old         new
#     NARROW    ca255af3    ca255af3   (unchanged -- GreedyBot)
#     WIDE      f223cea1    f223cea1   (unchanged -- GreedyBot)
#     WNARROW   f0b240da    f0b240da   (unchanged -- WeightedBot does not search)
#     WWIDE     9010ec80    9010ec80   (unchanged)
#     QNARROW   9ad67497    9ad67497   (unchanged -- see the note below)
#     QWIDE     e83054f7    e83054f7   (unchanged)
#     PNARROW   32a99881    089219c6
#     PWIDE     f7a092a2    d911788e
#
# Two-sided as 9.0 requires: derived independently in /tmp/flip-a and
# /tmp/flip-b, two separate clones of the same commit carrying the same
# one-constant patch, and the two agreed byte-for-byte on all eight arms --
# including the six that did not move, which is the half of the agreement that
# is easy to forget to check.
#
# CLEAN-BASE CONTROL FIRST, and it passed: a full gate on the unflipped tree
# of the parent 0335348 reproduced all eight committed constants exactly (GATE
# PASS, 1027 tests).  So these two moves are attributable to the flip and not
# to drift a re-derivation would have quietly absorbed.
#
# Cause, and it is ONE CONSTANT: `engine/bots/pending.py`'s `QUIET_PENDING`
# went False -> True.  A pending decision that is MINE is now priced after
# draining the pending stack, which is how each bot's own `_beam` already
# prices every node it searches (`apply -> _quiesce -> score`).  The real
# decision and the searched decision were being priced by different rules.
#
# WHY ONLY THE PLAN ARMS.  `wants_quiet` returns False unless `state.pending`
# is non-empty, so the flag is unreachable except at a pending decision, and
# only the two bots that route through `pending.fallback_pick` -- PlanBot and
# NeuralPlanBot -- can reach it at all.  QuiescentBot searches under the same
# `evaluate` but does not go through the shared short-circuit, which is why
# QNARROW/QWIDE hold still here even though they moved on the last three
# derivations.  GreedyBot and DEFAULT_WEIGHTS WeightedBot never reach it.
#
# NOT a strength claim.  This is a consistency fix and lands on that basis
# (docs/DRAIN_AB.md 1): nothing in `weighted.features` reads `pend["atk"]` or
# `pend["dfn"]`, so an undrained position cannot express whether a defence
# succeeds, and 588 of 589 winnable defences need 2+ cards.  The A/B is
# corroboration, and it is uneven -- decisive at 3p (4/4 blocks, z ~ 9.3 over
# 600 games) and NOT independently established at 4p (one pure block, z =
# 1.54).  Read docs/DRAIN_AB.md 2 before quoting a number from it; there are
# two pools and they say different things.
#
# `DETERMINIZE` was deliberately left False in this commit so that these two
# digest moves attribute to one constant.  It is the other half of the same
# inconsistency and gets its own derivation.
#
# ---------------------------------------------------------------------------
# Re-derived on the determinization fix (2026-07-30, docs/AGGRESSION_RATE.md
# 9a).  EXACTLY TWO of the eight arms moved -- PNARROW and PWIDE -- and the
# other six did not.
#
#     arm       old         new
#     NARROW    ca255af3    ca255af3   (unchanged -- GreedyBot)
#     WIDE      f223cea1    f223cea1   (unchanged -- GreedyBot)
#     WNARROW   f0b240da    f0b240da   (unchanged -- WeightedBot)
#     WWIDE     9010ec80    9010ec80   (unchanged)
#     QNARROW   9ad67497    9ad67497   (unchanged -- QuiescentBot)
#     QWIDE     e83054f7    e83054f7   (unchanged)
#     PNARROW   089219c6    3e428ad2
#     PWIDE     d911788e    fc990004
#
# CAUSE, AND IT IS ONE OF THE TWO THINGS IN THAT COMMIT, NOT BOTH.  The commit
# does two things: (1) `plan.determinize` now shuffles `current_events` as
# well as the two draw decks, and (2) `pending.DETERMINIZE` goes False -> True
# with both bots' `PENDING_DETERMINIZE` set to None.  Only (1) moves a digest.
#
# ATTRIBUTED, not assumed -- each cause was applied ALONE to a clean clone of
# the parent and both plan arms re-hashed:
#
#     tree                        PNARROW     PWIDE
#     parent (master 1fbf128)     089219c6    d911788e
#     (2) pending flip ONLY       089219c6    d911788e   <- INERT, byte-exact
#     (1) event shuffle ONLY      3e428ad2    fc990004   <- the whole move
#     both (what shipped)         3e428ad2    fc990004
#
# The pending flip reproducing the parent BYTE-FOR-BYTE on both arms is the
# strong form of "measured zero", and it agrees with the independent
# behavioural census: `tools/pending_divergence.py --lever det` at 3p changed
# the pick on 0 of 1328 of the bot's own pending decisions (a re-run of the
# 0/1346 in docs/AGGRESSION_RATE.md 9).  Two instruments, same answer.
#
# WHY ONLY THE PLAN ARMS, for cause (1).  `plan.determinize` is called from
# `PlanBot.pick`, `NeuralPlanBot.pick`, `NeuralBot.pick` and
# `pending.prepare_root`.  GreedyBot does not call it; WeightedBot does not
# call it; QuiescentBot does not call it.  So NARROW/WIDE, WNARROW/WWIDE and
# QNARROW/QWIDE CANNOT move for this change, and they did not, in both
# independent derivations.  If a future change to `determinize` ever moves one
# of those six, that is a bug in the change, not a digest to update.
#
# (Read that list again for what it costs elsewhere: WeightedBot and
# QuiescentBot do not determinize AT ALL, so they still draw the true next
# card on 100% of trial draws.  That is a live defect, it is measured at 0 of
# 2138 move changes today, and it is written down in
# docs/AGGRESSION_RATE.md 9a.1 rather than fixed here -- fixing it would move
# four of the six arms above and belongs in its own derivation.)
#
# WHAT (1) ACTUALLY CHANGES.  `events.reveal_current_event` pops
# `current_events` at the top of every turn, and nothing was ever shuffling
# that pile, so every `end_turn` the beam expanded revealed the REAL next
# event inside a search that believed it had determinized.
# `tools/infoleak.py --true-card` (the mode added in the same commit, because
# the old mode counts candidates that DRAW and therefore returns the same
# number leaking or not) puts it at 100.0% of event draws before and 38.3%
# after, against a ~33% chance floor for a 3-card pile.  Behaviourally it
# moves 78 of 3448 beam picks at 3p (2.3%) -- unlike cause (2), this one is
# NOT inert, which is why it and it alone moves these two digests.
#
# CLEAN-BASE CONTROL FIRST, and it passed: a full gate on unmodified master
# 1fbf128 reproduced all eight committed constants exactly (GATE PASS, 1027
# tests).  So the base was known-good and these two moves are attributable to
# this commit rather than to drift a re-derivation would have absorbed.
#
# TWO-SIDED as 9.0 requires: derived independently in /tmp/det-work and
# /tmp/det-work-b, two separate clones carrying the same commit, and the two
# agreed byte-for-byte on ALL EIGHT arms -- including the six that did not
# move, which is the half of the agreement that is easy to forget to check.
#
# Test count goes 1027 -> 1035, and the +8 is accounted for exactly:
# +7 from the new tests/test_search_root_is_determinized.py, and +1 from
# splitting `test_the_determinize_difference_is_pinned_not_accidental` (which
# pinned the two bots' values as DIFFERENT, and had to go when they stopped
# being different) into a "neither class carries its own default" test and a
# "the bot-wide det=0 switch reaches this path too" test.
#
# Nothing here was re-derived to make the gate pass.  The gate FAILED on this
# tree by design, twice, in two clones, and these two values are the output of
# computing the new behaviour plus a per-cause attribution that predicted them
# before the full-tree run reported them.
#
# All four moved again on the unit-technology board pricing; the table, the
# cause and the one-constant attribution are in the block above WNARROW.
# Test count goes 1040 -> 1053: +12 from the new tests/test_unit_pricing.py
# and +1 from splitting `test_zero_credit_is_the_static_answer_for_every_card`
# in tests/test_board_yields.py, which had to grow a sibling once units
# stopped being gated on `card_board_credit` and started being gated on
# `unit_tech_credit`.
#
# All four moved once more on the whole-technology board pricing; the table,
# the cause and the one-constant attribution are in the block above WNARROW.
# Test count goes 1053 -> 1070: +16 from the new tests/test_yellow_pricing.py
# and +1 from splitting `test_zero_credit_is_the_static_answer_for_every_card`
# in tests/test_board_yields.py a second time, which needed a third sibling
# once the non-red technologies started being gated on `tech_board_credit`.
# All four moved again on the horizon rework; the table, the two causes and
# the environment-hatch attribution are in the block above WNARROW.
# SIX MOVED AGAIN and the two GreedyBot arms held still again, on the yellow
# ACTION-card board pricing (docs/ACTION_CARD_PRICING.md).
#
#     arm       old         new
#     NARROW    ca255af3    ca255af3   (unchanged -- GreedyBot)
#     WIDE      f223cea1    f223cea1   (unchanged -- GreedyBot)
#     WNARROW   6d888d7c    ba77b499
#     WWIDE     c52302c2    f4d6a545
#     QNARROW   bbbb203a    4ab439b2
#     QWIDE     3df0155f    5d05f578
#     PNARROW   1b883d6f    0a637b40
#     PWIDE     3922ebc4    ccc96764
#
# NOTE ON THE BASE.  This lane was derived once against 8b972ef, and the
# horizon lane (docs/MODEL_CONSTANTS.md) landed underneath it and moved all
# six evaluator arms.  Everything below was therefore RE-DERIVED from scratch
# on the new base rather than carried over -- a digest re-used across a base
# change is exactly the laundering docs/PYPY.md 9.0 forbids.  The first
# derivation's numbers (e9cdc2d4 / 0c5a4337 / ce0d22bf / 49b898e1 / 65d9a884 /
# b952c68e, against a 8b972ef base) are recorded here only so the reader can
# see that they were discarded and not reconciled.
#
# Cause: `DEFAULT_WEIGHTS` gained `action_board_credit` at 1.0, which routes
# all 33 yellow action cards' price through `weighted.action_value` instead of
# the static table.  Sixteen of the 33 priced at EXACTLY 0.000 before it --
# thirteen because `free_civil_action` and `resource_discount` are weights
# `features()` never emits (so `evaluate` never pays for them and no game can
# put a gradient on them; they are 0.0 on every champion in the pool), three
# because `_card_choices` was multiplied by `card_board_credit`, also 0.0
# everywhere.  Every searching bot's civil hand and card row therefore price
# differently, so all six evaluator arms were EXPECTED to move and did.
#
# ATTRIBUTED TO ONE CONSTANT, on a third clean clone: `action_board_credit`
# changed from 1.0 to 0.0 and nothing else touched reproduces **all eight**
# pre-change digests byte for byte -- ca255af3 / f223cea1 / 6d888d7c /
# c52302c2 / bbbb203a / 3df0155f / 1b883d6f / 3922ebc4.  So `action_value`,
# `_yield_marginal`, `_RESTRICTED_TO_FEATURE`, `_is_action`,
# `restricted_resource_credit` and `free_action_credit` are provably inert on
# their own and the six moves are that one default.
#
# GREEDYBOT HOLDING STILL IS THE INFORMATIVE HALF, as it was for the two
# technology lanes: GreedyBot never calls `card_potential`, so a NARROW/WIDE
# move would have meant a card-pricing change had leaked into the rules.  It
# did not.
#
# Two-sided per 9.0: derived from scratch in /tmp/actionfix and independently
# in /tmp/actionfix2, two separate clones of the same tree, which agreed byte
# for byte on all eight arms INCLUDING the two that did not move.  A clean-base
# control on the parent commit (7bf483a, /tmp/actionctl2) reproduced all eight
# of ITS committed constants first.  Nothing was re-derived to make the gate
# pass; when the base moved, every arm was recomputed rather than rebased.
#
# Test count goes 1107 -> 1128: +20 from the new tests/test_action_pricing.py
# and +1 from splitting `test_zero_credit_is_the_static_answer_for_every_card`
# in tests/test_board_yields.py a third time, which needed a fourth sibling
# once the action cards started being gated on `action_board_credit`.
PNARROW=0a637b40
PWIDE=ccc96764
QNARROW=4ab439b2
QWIDE=5d05f578

fail=0
# The interpreter under test.  `PY=pypy3 bash tools/gate.sh --journal` runs
# every arm under PyPy and requires the SAME digests, which is the
# cross-interpreter determinism check docs/PYPY.md section 2 did by hand and
# section 11 needed for the journal arms as well.  Default unchanged.
PY="${PY:-python3}"
note() { printf '%-32s %s\n' "$1" "$2"; }

# NOTE: /bin/bash on macOS is 3.2.  An earlier version of these two helpers
# collected the env assignments into an array (`envs+=(...)`, `"${envs[@]}"`);
# under 3.2 that silently produced garbled output and a spurious GATE FAIL on
# a tree whose digests were provably correct when the same command was run by
# hand.  A gate that cries wolf is worse than no gate -- see 9.0 for how much
# damage a misleading gate reading does on this project -- so both helpers now
# take the environment as ONE plain string and there are no arrays anywhere.

check_fp() {   # name  want-prefix  "ENV=1 ENV2=2"  [perf_check args...]
  local name="$1" want="$2" envstr="$3"; shift 3
  local got
  got=$(env $envstr nice -n 10 "$PY" -m engine.perf_check hash "$@" 2>&1 \
        | awk '/^FINGERPRINT/{print $2}')
  case "$got" in
    "$want"*) note "$name" "OK   ${got:0:16}" ;;
    *)        note "$name" "FAIL ${got:0:16} != ${want}..."; fail=1 ;;
  esac
}

run_tests() {   # name  "ENV=1"
  local name="$1" envstr="$2"
  local out
  out=$(env $envstr nice -n 10 "$PY" -m unittest discover -s tests 2>&1 | tail -4)
  if echo "$out" | grep -q '^OK'; then
    note "$name" "OK   $(echo "$out" | grep -o 'Ran [0-9]* tests')"
  else
    note "$name" "FAIL"; echo "$out"; fail=1
  fi
}

# -- static analysis, first because it is the cheapest arm in the file ------
#
# ~200ms, no game played.  It exists because of the `_quiesce` bug
# (docs/INFORMATION_AUDIT.md 6.3): `ctx.get("root_row")` in a method with no
# `ctx` in scope, wrapped in `except Exception:`.  Every call raised NameError,
# the except ate it, and all 550 tests passed -- only `plan wide` below caught
# it, and only because the damage happened to move the final scores.  `ruff
# check --select F821` flags that line instantly.  Rule set is correctness-only
# and deliberately narrow; see ruff.toml for what is excluded and why.
#
# Skipped with a loud note rather than a FAIL if ruff is absent, so the gate
# still works on a machine without it.
run_lint() {
  if ! command -v ruff >/dev/null 2>&1; then
    note "ruff" "SKIP (not installed: pip install ruff)"; return
  fi
  local out
  out=$(ruff check --no-cache . 2>&1)
  if [ $? = 0 ]; then note "ruff" "OK   $(echo "$out" | tail -1)"
  else note "ruff" "FAIL"; echo "$out" | tail -20; fail=1; fi
}

# -- swallowed-programmer-error audit --------------------------------------
#
# The dynamic half of the same guard, for the cases static analysis cannot see
# (a name that exists but is None, a signature that drifted, an unguarded
# denominator).  Watches sys.monitoring's RAISE event, which fires BEFORE any
# `except` runs, so it sees a swallowed NameError/TypeError/... without touching
# the ~56 deliberate `except Exception:` blocks that keep a 40-hour league run
# alive.  4p, all four bots -- the widest search, ~20s.
#
# tests/test_no_swallowed_bugs.py is the 4-second 2p version inside the suite,
# and carries the negative control proving the instrument can still fail.
run_audit() {
  local out
  out=$(nice -n 10 "$PY" -m tools.bug_audit --games 1 --players 4 2>&1)
  if [ $? = 0 ]; then note "bug audit" "OK   $(echo "$out" | tail -1 | cut -c1-40)"
  else note "bug audit" "FAIL"; echo "$out" | head -20; fail=1; fi
}

run_lint
run_audit
run_tests "unittest" ""
# The suite again with the journal checking itself against a copy_state oracle
# on every rollback.  This is nearly free (the tests are seconds) and it is a
# real arm: a test that performs an unjournalled container mutation passes
# here-but-not-there, which is precisely the bug class this branch exists to
# prevent.  Keeping the suite paranoid-clean is what makes it usable as a
# check rather than merely as a test.
run_tests "unittest JOURNAL_PARANOID" "JOURNAL_PARANOID=1"

check_fp "narrow fingerprint"        "$NARROW" ""
check_fp "narrow FASTCOPY_PARANOID"  "$NARROW" "FASTCOPY_PARANOID=1"
check_fp "weighted narrow"           "$WNARROW" "" --weighted
check_fp "quiescent narrow"          "$QNARROW" "" --quiescent
check_fp "plan narrow"               "$PNARROW" "" --plan
if [ "${1:-}" != "--fast" ]; then
  check_fp "wide fingerprint"        "$WIDE" "" --wide
  check_fp "wide FASTCOPY_PARANOID"  "$WIDE" "FASTCOPY_PARANOID=1" --wide
  check_fp "weighted wide"           "$WWIDE" "" --weighted --wide
  check_fp "quiescent wide"          "$QWIDE" "" --quiescent --wide
  check_fp "plan wide"               "$PWIDE" "" --plan --wide
fi

# The journal arms.  These only mean anything once step 5 has converted every
# module, so they are opt-in until then -- but when they do run they are the
# strongest check in the file: TTA_JOURNAL=1 makes GreedyBot search by undo
# instead of by copy, and JOURNAL_PARANOID=1 additionally copies the state,
# rolls back, and structurally diffs the two on EVERY candidate move.  A
# missed mutation site raises there, naming the attribute path, rather than
# showing up as a digest mismatch with no clue attached.
if [ "${1:-}" = "--journal" ]; then
  check_fp "narrow JOURNAL"            "$NARROW" "TTA_JOURNAL=1"
  check_fp "narrow JOURNAL+PARANOID"   "$NARROW" "TTA_JOURNAL=1 JOURNAL_PARANOID=1"
  check_fp "wide JOURNAL"              "$WIDE" "TTA_JOURNAL=1" --wide
  check_fp "wide JOURNAL+PARANOID"     "$WIDE" "TTA_JOURNAL=1 JOURNAL_PARANOID=1" --wide
  # The same four arms with WEIGHTED searching.  9.14 showed the two bots do
  # not cover the same mutation sites -- WeightedBot reaches the refused-pact
  # site (interact.py:228) that 60 games of GreedyBot never execute -- so these
  # are not a duplicate of the arms above, they are the other half of the
  # coverage claim.
  check_fp "weighted narrow JOURNAL"          "$WNARROW" "TTA_JOURNAL=1" --weighted
  check_fp "weighted narrow JOURNAL+PARANOID" "$WNARROW" "TTA_JOURNAL=1 JOURNAL_PARANOID=1" --weighted
  check_fp "weighted wide JOURNAL"            "$WWIDE" "TTA_JOURNAL=1" --weighted --wide
  check_fp "weighted wide JOURNAL+PARANOID"   "$WWIDE" "TTA_JOURNAL=1 JOURNAL_PARANOID=1" --weighted --wide
  # Section 10's arms.  These are the ones that matter for the league today:
  # QuiescentBot and PlanBot are what `run_league.sh` trains, and both now
  # search by undo, NESTED (QuiescentBot reaches depth 3: pick -> _resolve/
  # _pick -> war_value).  The PARANOID variants copy the state, apply the
  # candidate by undo, roll back and structurally diff -- including dict key
  # order -- at EVERY nesting level, on every node of every beam.
  #
  # `plan wide JOURNAL+PARANOID` is by far the slowest arm in this file
  # (~20 min under load: 6 games x ~15k beam nodes, each copied and diffed).
  check_fp "quiescent narrow JOURNAL"          "$QNARROW" "TTA_JOURNAL=1" --quiescent
  check_fp "quiescent narrow JOURNAL+PARANOID" "$QNARROW" "TTA_JOURNAL=1 JOURNAL_PARANOID=1" --quiescent
  check_fp "plan narrow JOURNAL"               "$PNARROW" "TTA_JOURNAL=1" --plan
  check_fp "plan narrow JOURNAL+PARANOID"      "$PNARROW" "TTA_JOURNAL=1 JOURNAL_PARANOID=1" --plan
  check_fp "quiescent wide JOURNAL"            "$QWIDE" "TTA_JOURNAL=1" --quiescent --wide
  check_fp "quiescent wide JOURNAL+PARANOID"   "$QWIDE" "TTA_JOURNAL=1 JOURNAL_PARANOID=1" --quiescent --wide
  check_fp "plan wide JOURNAL"                 "$PWIDE" "TTA_JOURNAL=1" --plan --wide
  check_fp "plan wide JOURNAL+PARANOID"        "$PWIDE" "TTA_JOURNAL=1 JOURNAL_PARANOID=1" --plan --wide
fi

if [ "$fail" = 0 ]; then echo "GATE PASS"; else echo "GATE FAIL"; fi
exit $fail
