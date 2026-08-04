# The champions preferred being robbed (2026-08-04)

`docs/AGGRESSION_STATUS.md` §3 said the bot gives up defences it can win, and
blamed the search horizon: the first defence card leaves the aggression
pending, so the outcome is invisible.

**That was wrong, and the instrument said so.** With `QUIET_PENDING` on, the
bot plays the whole defence out in its head before deciding. It reaches the
position where the aggression has *failed* and scores it BELOW surrender:

    2p champion, defender holding 4 military cards, needs all 4 to survive
      ("defend_done",)                  53.365
      ("defend", <any card>)            53.261     <- the WINNING line

It was not blind. It preferred to be robbed.

## What was actually wrong

    champion_2p, fresh 2p board, defender with 12 food / 12 resources / 8 science / 8 culture
      lose 4 resources    54.485 -> 54.485    +0.000
      lose 3 culture      54.485 -> 55.033    +0.548   <== theft HELPS

Two inversions, each of which the existing `guard_weights` is *structurally
unable to see*:

| | | |
|---|---|---|
| `culture` +1.0, `culture_early` **-1.3113** | net **-0.31** early | the per-key sign guard passes it: `culture` itself is positive, and the phase multipliers are exempt from that guard by an explicit measured decision. Nothing ever looked at the sum, which is the number `evaluate` multiplies by. |
| `resource_stock` **0.0** vs `blue_free` +0.4220 | losing a resource frees the blue token it sat on | 0.0 is not a sign violation, so nothing fired. Being plundered of 4 resources was worth **+1.27**. |

This is the repo's recurring bug class in a new coordinate system: *the guard
and the thing it guards are two different quantities, and nothing failed when
they disagreed.*

## The fix

`weighted.dominance_repair` — two rule-level orderings, applied in
`load_weights` (so every bot, tool and arena reads a repaired vector) and
again in `hillclimb_league.guard_weights` (so the champion **file** carries
what the bots actually play).

1. **Net non-negative phase terms**, `NET_NONNEG_PHASE`: `culture` and
   `wonder_progress`. Culture is the score; you are never worse off holding
   more of it. `wonder_progress` is `sum(stages[:built])` — resources that
   have *already left* `resource_stock`, so paid stages are strictly closer to
   the payoff. The risk of a half-built wonder is priced by four other
   features (`wonder_remaining`, `wonder_stages_left`,
   `wonder_turns_to_finish`, `wonder_overrun`), which is where a negative
   belongs.
2. **Dominance pairs**, `DOMINATES`: `resource_stock >= blue_free`. Spending a
   resource returns the token to the bank *and* buys the thing, so a stocked
   resource is worth at least a free token whatever either is worth.

3. **Benefit gates**, `BENEFIT_GATES` (added later the same day): the nine
   weights that scale a printed grant on exactly one card class may not be
   negative. `hillclimb_league.NONNEG` is derived as `{k: DEFAULT[k] > 0}`, so
   a credit whose default is exactly **0.0** — which is how every class credit
   ships, switched off for the league to price — is in neither NONNEG nor
   NONPOS and is guarded by nothing. Same shape as `resource_stock` above.
   `tests/test_play_rate.py::TestBenefitGatesAreDerived` re-derives the set by
   perturbing each weight and checking it raises `card_potential` for every
   card in its class, so the written-down list cannot go stale.

   Measured: `wonder_stages_per_action` was negative on **all three live
   champions** (-0.13614 / -0.03634 / -0.04145). It gates Masonry,
   Architecture and Engineering and nothing else — the three cards that make
   wonders cheap in actions. So the wonders complaint had two coordinates
   pointing the same way, not one: a negative net `wonder_progress` on 4p and
   a negative markdown on the build techs everywhere.
   `unit_strength_credit` (the failure `tests/test_play_rate.py` was written
   for in the first place), `restricted_resources`, `free_civil_action`,
   `hand_limit` and `build_discount` were negative on at least one trained
   vector too.

Repairs go to the **boundary**, not to the default — the smallest change that
makes the vector expressible. The resource pair is repaired by *raising*
`resource_stock`: `blue_free` was climbed (0.15 → 0.4220) and `resource_stock`
sat at exactly 0.0, which is the signature of a coordinate the climb never
moved rather than one it measured.

Deliberately **not** guarded: the other phase-multiplied terms. More workers
costs consumption, resource production on the last turn really is close to
worthless — a net-negative there is a strategic claim the league is entitled
to make. Only the terms where a pure gain cannot hurt under the rules are
pinned.

## Blast radius: every trained vector had at least one

Net sign flips found across the six trained vectors (base + multiplier):

| vector | term | net early | net late |
|---|---|---|---|
| champion_2p | **culture** | **-0.31** | +1.19 |
| champion_2p | workers | -0.13 | +0.15 |
| champion_2p | science_rate | +5.29 | -0.48 |
| champion_3p | strength_rel | -0.27 | +0.10 |
| champion_4p | **wonder_progress** | **-0.17** | **-0.21** |
| champion_4p | resource_rate | +0.11 | -1.34 |
| gen00104 | **culture** | **-1.32** | +1.48 |
| gen00361 | **wonder_progress** | **-0.30** | **-0.18** |

The bolded rows are the ones this guard repairs.

## Effect, measured

Same synthetic defence, after the repair — the four give-ups that were real
damage now defend, and the ones that remain are positions where the aggression
steals nothing the defender owns (no colony, no science, no culture) and
surrender is correct:

| aggression | before | after |
|---|---|---|
| Plunder (I/II/III) | give up | **DEFEND** |
| Raid (I/II/III) | DEFEND | DEFEND |
| Annex / Spy / Infiltrate / Armed Intervention | give up | give up (steals nothing here) |
| Enslave | give up | give up (pop loss ≈ 4 cards, borderline) |

And all pure losses now cost what they should:

    champion_2p after repair
      lose 4 resources    64.973 -> 63.285    -1.688
      lose 3 culture      64.973 -> 64.667    -0.306

## What locks it

`tests/test_theft_never_helps.py` asserts the *behaviour*, not the weights: on
every trained vector, taking things away from a player and giving it nothing
back may not raise that player's own evaluation. That survives retraining;
an assertion about `culture_early` would not.

## Not measured here

* Strength. This lands as a correctness fix on a rule-level dominance
  argument, exactly as `QUIET_PENDING` did — see `validate-in-prod-not-offline`:
  it goes to master and the league runs tell us.
* Whether champion_4p's negative `wonder_progress` actually suppressed its
  wonder play rate. The weight says it should have; nobody has counted.
* 3p/4p defence rates. The census re-run here is 2p only.
