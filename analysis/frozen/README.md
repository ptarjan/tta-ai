# The frozen champion vectors: what they are, and what they are not

These files are the fixed reference bots the A/B harnesses load. They are
**not** the bot the league trains. This file exists because that difference
silently invalidated a night of measurements, and nothing on disk said so.

## Provenance

All three were written by `f6318cc` (2026-07-26 12:45) — "frozen champion
snapshot for a reproducible A/B". Each was copied off the *running* league's
on-disk champion at that moment, which is why every one of them is a few
generations AHEAD of the `experiments/champion_*.json` committed in the same
commit:

| file | gen at snapshot | committed sibling |
|---|---|---|
| `champion_2p.json` | 220 | `experiments/champion_2p.json` gen 209 |
| `champion_3p.json` | 160 | `experiments/champion_3p.json` gen 152 |
| `champion_4p.DEGENERATE.json` | 139 | `experiments/champion_4p.json` gen 133 |

They are legitimate trained champions **of their era**. The problem is the
era.

## The vocabulary moved underneath them

The weight vocabulary has grown twice since the snapshot, and always by
addition — the frozen keys are a strict subset of the live keys, which are a
strict subset of `DEFAULT_WEIGHTS`:

    frozen (78)  ⊂  live league champions (99)  ⊂  DEFAULT_WEIGHTS (112)

A weight absent from a loaded vector is filled in from `DEFAULT_WEIGHTS`
(`load_weights`), and the 21 keys the frozen vectors lack include the entire
card-row block: `row_urgency`, `row_bargain_forgone`, `card_rate_credit`,
`take_cost_paid`, `hand_potential`, `rival_hand_potential`. Most of those
default to **0.0**.

### Why that is not a harmless staleness

`engine/bots/weighted.py` documents that a wonder never enters `hand_civil`,
and that `features()` reads `p.wonder` only for resource arithmetic. **The
only path by which a wonder's identity can reach the policy at all is
`row_pressure`, and `evaluate()` gates that call on `row_urgency` or
`row_bargain_forgone` being non-zero.** Both are absent from the frozen
vectors, so both fill in as 0.0, so `row_pressure` is never called.

Measured directly (`evaluate()` on 480 states with a repriced wonder in the
row, `card_rate_credit` 1.0 vs 0.0):

| vector | states where the reprice changes `evaluate()` |
|---|---|
| frozen 2p | **0 / 480** (max delta 0.0000) |
| live 2p | **480 / 480** (max delta 21.1403) |

Against a frozen vector, "repricing wonders moved wonder behaviour by zero" is
an *arithmetic identity*, not an experimental result. No sample size can
change it. Any A/B whose lever runs through `card_potential` is untestable
here and must use a vector that carries the card-row weights.

## `champion_4p.DEGENERATE.json` — retired, deliberately kept

This file is the vector `docs/TRAINING_RUN.md` says never to warm-start from
(`science = -6.08883`; `docs/CULTURE_GAP.md` Sec 8f measured it at **20.1%
against a 25% null** — worse than random seating), six generations later. It
reproduces **all 62** of that vector's informative weights bit-for-bit and
differs on exactly two keys, `colonies` and `pacts` — and only because
`15b9764` reset those two to defaults in the committed copy while the on-disk
climb kept going.

Two keys were enough to defeat `refuse_if_degenerate_champion`, which tested
exact content (`all(mine.get(k) == v ...)`). The guard now scores provenance
over the *informative* keys instead; see `_degenerate_match` in
`experiments/arena.py`. The separation is total: both descendants score
1.000, and every other champion in the repo — including all three live league
champions and `DEFAULT_WEIGHTS` itself — scores 0.000.

The file is renamed rather than deleted so the numbers already published
against it can still be reproduced and audited. It is refused by the guard
under any name. **Do not use it for anything but forensics.**

## The rule: freeze, but freeze something current

Both instincts are right and they are not actually in conflict.

**Keep freezing.** Measuring against the live champion directly is not an
option. The league writes `experiments/league_state/champion_*.json` whenever a
mutant is accepted, so two runs an hour apart are not comparable, an A/B whose
arms are collected at different times is confounded by training, and no result
can ever be reproduced. A moving baseline is not a baseline.

**But a frozen vector is only a valid instrument for levers it can carry.**
That is the failure here. Nobody checked that the frozen reference still had
the weight the experiment was about, and the harness cannot tell the
difference between "this lever does nothing" and "this vector has no socket for
this lever" — both come back as a clean, well-powered null. Staleness in a
weight vector does not degrade gracefully. It answers zero, with confidence
intervals.

So:

1. **Re-cut the snapshot whenever the weight vocabulary grows**, not on a
   schedule. Growth is the event that invalidates old snapshots; drift within
   a fixed vocabulary is exactly the thing freezing is supposed to protect
   against, and is fine.
2. **Name snapshots for their generation and vocabulary size** —
   `champion_2p_gen54_99key.json`, not `champion_2p.json`. A name that cannot
   go stale cannot silently go stale.
3. **State the vocabulary size in the doc that reports the result.** "Measured
   against the frozen 2p champion" is not a provenance statement. "gen 220,
   78 keys, code at 112" is.
4. **Before running any A/B, assert the lever is non-zero in the base
   vector.** One line. It would have caught this before the first of the
   12,800 games. If the lever is gated (`evaluate()` has several
   `if <weight>:` guards), assert the *gate* is non-zero too — that is the
   specific trap here: `card_rate_credit` was set on both arms and still did
   nothing, because `row_urgency` upstream of it was 0.0.
5. **Report the live champion alongside**, on a smaller sample, whenever a
   result is going to be used to justify changing the bot. The frozen vector
   answers "is this lever real?"; only the live one answers "is this lever
   real for the bot we are actually training?" They are different questions
   and tonight they had different answers.
