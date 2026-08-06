# The frozen champion vectors: what they are, and what they are not

These files are the fixed reference bots the A/B harnesses load. They are
**not** the bot the league trains. This file exists because that difference
silently invalidated a night of measurements, and nothing on disk said so.

## Provenance

All three were written by `f6318cc` (2026-07-26 12:45) — "frozen champion
snapshot for a reproducible A/B". Each was copied off the *running* league's
on-disk champion at that moment, which is why every one of them is a few
generations AHEAD of that commit's own committed sibling, `experiments/
champion_*.json` — itself moved into this directory on 2026-08-06 and
renamed for its era and generation so it cannot be mistaken for current (see
[`docs/RUST_LEAGUE.md`](../../docs/RUST_LEAGUE.md#which-champion-file-is-live)):

| file | gen at snapshot | committed sibling |
|---|---|---|
| `champion_2p.json` | 220 | `python_champion_2p_gen209_2026-07-26.json` |
| `champion_3p.json` | 160 | `python_champion_3p_gen152_2026-07-26.json` |
| `champion_4p.DEGENERATE.json` | 139 | `python_champion_4p_gen133_2026-07-26.json` |

The three `python_champion_{2,3,4}p_..._2026-07-26.json` files are the
**final vectors the Python-era trainer ever wrote** — 78 weight keys, last
meaningfully changed 2026-07-26, superseded since by the Rust league's own
`experiments/rust_champion_{2,3,4}p.json` (130+ keys and climbing; gitignored,
so only present in a working checkout — see
[`docs/RUST_LEAGUE.md`](../../docs/RUST_LEAGUE.md#which-champion-file-is-live)).
They are kept here, not deleted, purely so numbers already published against
them in old docs can still be checked; do not read them as describing the
current bot.

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
(`science = -6.08883`; measured (Python-era, in the now-deleted
`docs/CULTURE_GAP.md` — see `docs/EVALUATOR_HISTORY.md`'s "Superseded without
independent content" note, or git history for the specific number) at
**20.1% against a 25% null** — worse than random seating), six generations
later. It
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

## The current reference vectors (use these)

Cut 2026-07-30 from the live league champions, named for the generation and
key count so the name cannot go stale silently. Every one carries
`row_pressure` **open**, which is the thing the 78-key vectors did not and the
reason they could not answer a wonder-pricing question.

| file | gen | keys | `row_urgency` | `card_rate_credit` | wonder path | wonders visible |
|---|---|---|---|---|---|---|
| `champion_2p_gen54_99key.json` | 54 | 99 | −0.19109 | 0.12812 | `row_pressure` | 4/16 |
| `champion_3p_gen1255_99key.json` | 1255 | 99 | **+0.16269** | 0.89906 | `row_pressure` | 2/16 |
| `champion_4p_gen350_99key.json` | 350 | 99 | +0.00237 | 1.0 | `row_pressure` | 2/16 |
| *(retired)* `champion_{2,3}p.json` | 220/160 | 78 | absent → 0.0 | 1.0 | **NOTHING** | — |
| *(quarantined)* `champion_4p.DEGENERATE.json` | 139 | 78 | absent → 0.0 | 1.0 | **NOTHING** | — |

Print the full table for any vector, and do it *before* you measure:

```bash
python3 tools/conduction_table.py analysis/frozen/champion_3p_gen1255_99key.json
```

### Two gates, and passing the first tells you nothing about the second

**Gate 1 — is the consumer open?** `evaluate()` skips whole feature functions
when their scale weight is 0.0. A wonder never enters `hand_civil`, so its
`card_potential` reaches the policy only through `row_pressure`. On the 78-key
vectors that gate is shut and the tool prints `for a WONDER specifically:
NOTHING` — the one sentence that would have saved the 12,800-game null.

**Gate 2 — does the card clear `card_potential > 0`?** `row_pressure` skips
any card whose potential is `<= 0` ("the sweep destroying a card I do not want
is not a loss"). This is a **threshold, not a slope**, and it is where the
+88% actually came from. At the live 2p champion's *trained*
`card_rate_credit = 0.12812`, **4** of 16 wonders clear it; at 0.0, **0** do;
at 1.0, **8** do — and those 8 are exactly the 8 repriced wonders that moved.
**The reprice did not make wonders better, it made them visible.**

That threshold is a property of the *vector*, not of the code, and it is easy
to get backwards: under `DEFAULT_WEIGHTS` 11 of 16 wonders already price above
zero and the same knob changes **nothing**. A probe run against defaults would
have concluded there was no gate at all.
`tests/test_conduction_table.py` pins both directions.

### Caveat on the 3p reference: its `row_urgency` sign is arbitrary

`row_urgency` is read off the **post-move** state, so it measures urgency
*left behind*; taking the doomed card lowers it, and preferring that take
therefore requires a **negative** weight (which is why
`tests/test_row_features.py`'s `row_on()` helper uses −0.1). The 2p champion's
−0.19109 has the right sign. **The 3p champion's +0.16269 does not**, and it
was checked rather than assumed before this file was blessed:

* **Not a per-player-count subtlety.** `_SWEEP = {2: 3, 3: 2, 4: 1}`, so the
  slide is `n * SWEEP[n]` = 6 at 2p and 6 at 3p over the same 13-slot row. The
  arithmetic at 2p and 3p is identical.
* **Not an inert weight.** Measured across real decisions, the urgency term
  varies between candidate moves on **35.0%** of 3p decisions with a mean
  `|w| × range` of 0.2276 and a max of 5.99. It is doing something.
* **But not decisive either.** `tools/guard_ab.py 3 300` with the sign flipped
  (+0.16269 → −0.16269), paired on seeds against two opponents: pooled win
  edge **+0.0025 ± 0.0305**, margin edge **+0.48 ± 2.73**, n=600. A tight null.

So the climb has no usable gradient on this weight at the *strength* level and
drifted to a semantically wrong sign without ever paying for it. The vector is
still the right reference — it is the bot the league trains — but:

> **Any behavioural measurement at 3p that reads card ordering (concordance,
> take-rate ordering, row-timing) is reading a sign that is arbitrary.**
> Win-rate and margin measurements are unaffected. Do not interpret 3p
> row-pressure behaviour as a learned preference.

The 4p champion's +0.00237 is a different case and *not* a defect: that weight
varies on only 9.8% of decisions with a mean `|w| × range` of **0.0007**, so it
is simply unidentifiable — there is no gradient for the climb to follow, and
the value is noise around zero. Read no meaning into its sign either.

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
4. **Before running any A/B, print the conduction table.**
   `python3 tools/conduction_table.py <vector>` — one second, and it names
   both gates. `experiments.arena.assert_lever_conducts()` enforces gate 1
   automatically for anything routed through it. Gate 2 has no automatic
   enforcement because a zero visible set is sometimes the honest answer; read
   the line.
5. **Report the live champion alongside**, on a smaller sample, whenever a
   result is going to be used to justify changing the bot. The frozen vector
   answers "is this lever real?"; only the live one answers "is this lever
   real for the bot we are actually training?" They are different questions
   and tonight they had different answers.
