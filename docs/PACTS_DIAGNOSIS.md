# Why the champions never play pacts (and almost never colonize)

Status: COMPLETE
Date: 2026-07-26

**Summary: it is (2) a bot blind spot, not an engine bug.** Pact moves are
generated and legal in 16% of politics decisions; the champions never take
them because a 1-ply evaluator cannot see any move whose payoff is deferred
to another player's decision, and ties break to the do-nothing option.
Colonies are the same failure plus two aggravating causes. A third,
independent bot bug was found on the way (wrong evaluation perspective on
other players' decisions).

## Verdict (pacts): **BOT BLIND SPOT, not an engine bug.**

`offer_pact` moves *are* generated, and often. Instrumented 4 mirror
self-play games at 3 players with the 3p champion weights
(`experiments/champion_3p.json`):

| quantity | value |
|---|---|
| politics decisions | 218 |
| decisions where `offer_pact` was in `legal_moves` | **35 (16%)** |
| per game: politics decisions with a pact available | 8, 14, 11, 2 |
| `offer_pact` moves actually chosen | **0** |

So pact cards reach hands, the politics phase offers them, the 2p removal
logic is correctly *not* firing at 3p, and the offer/accept/refuse flow is
reachable. The engine is fine. The bot simply never scores a pact above
`pol_pass`.

## Why the bot can never choose a pact (mechanical, not a tuning accident)

Both bots are **1-ply**: `pick()` copies the state, applies the candidate
move, and evaluates the resulting state (`engine/bots/__init__.py:171-195`,
`engine/bots/weighted.py` `WeightedBot.pick`).

`offer_pact` does *not* put a pact into play. `engine/actions.py:979-992`
(`_h_offer_pact`) does exactly three things:

1. `p.hand_military.remove(name)` — the card **leaves your hand**,
2. sets `politics_done` / `phase = "actions"`,
3. `interact.push_choice(state, target, "pact_offer", ...)` — a *pending*
   choice on the **other** player.

The pact object is only created later, in the partner's choice handler
`engine/interact.py:217-228` (`_c_pact_offer`), on `accept`. So in the
trial state the deciding bot evaluates:

* `pacts` feature (`engine/bots/weighted.py:182`, weight `0.5` by default)
  is **unchanged at 0** — the pact does not exist yet;
* `hand_military` and `hand_mil_value` (`weighted.py:203-206`) have gone
  **down** by one card;
* nothing else moved at all.

With any positive hand weight, `offer_pact` is therefore **strictly worse
than `pol_pass` in every position, by a constant**. This is visible in the
probe: every `offer_pact` variant scores identically (same value for side
A, side B and each possible partner — the evaluation is completely blind to
who the partner is and what the pact does), and always below `pol_pass`:

```
round 20, chosen ('prepare_event', 'Impact of Happiness')  160.878
          ('pol_pass',)                                    157.913
          ('offer_pact','Loss of Sovereignty',0,'A')       156.809
          ('offer_pact','Loss of Sovereignty',0,'B')       156.809   <- identical
          ('offer_pact','Loss of Sovereignty',1,'A')       156.809   <- identical
```

Direct proof — diffing the feature vectors of the two successor states of
the *same* position, from the mover's own seat, with the 3p champion
weights:

```
move ('offer_pact', 'International Tourism', 0, '')
feature diff  pol_pass -> offer_pact:
    hand_military   6 -> 5
    hand_mil_value 21 -> 17
weighted delta: -1.10445        # every other feature identical
```

Two features move, both downward. There is no path by which any pact can
ever be chosen.

The champion's `pacts` weight is dead code: no reachable 1-ply successor
state ever has a nonzero `pacts` count for the *mover*, so the hill climb
has never been able to select on it. (It can be nonzero for a player who
*accepted* a pact — but accepting is a `choose` move, and the same 1-ply
horizon applies to whether accepting looks good.)

`GreedyBot`'s 19-feature vector (`engine/bots/__init__.py:80-110`) has no
`pacts` feature at all, so for greedy it is doubly hopeless.

### The same horizon problem, but worse: aggressions

Note `aggression` is 0.03/game at 3p and 0.11/game at 4p, and `war` is
**0.00** — for the same structural reason: `_h_aggression`
(`actions.py:972-976`) also just pushes a pending defence choice, so the
attacker's 1-ply lookahead sees the military card leave hand and no gain.
The whole politics phase has collapsed to `pol_pass` (9.98/game at 3p,
18.38/game at 4p) plus `prepare_event`, which is the *only* politics move
whose reward (`p.culture += level`, `actions.py:964`) lands immediately
inside the mover's own trial state. That is a strong confirmation of the
diagnosis: the bot plays exactly the politics moves that pay off within
one ply and none of the ones that don't.

## Recommended fixes, ranked by risk

**1. (lowest risk, highest value) Resolve deferred self-choices during the
1-ply trial, or add a "pending-offer credit" term.**
The cleanest minimal version: in `features()`, count pacts the player has
*offered and not yet had resolved* alongside pacts in play, i.e. read
`state.pending` for a `pact_offer` whose `ctx["owner"] == idx` and credit
it (discounted, e.g. 0.5x, for the refusal risk). ~10 lines in
`engine/bots/weighted.py`, no engine change, immediately makes the existing
`pacts` weight live and hill-climbable.

**2. Add pact-quality features, not just a count.**
`pacts` as a bare count cannot distinguish `Peace Treaty` from
`Loss of Sovereignty` (which costs Player B culture). Add features derived
from the pact's own effect block for the side the mover would take —
e.g. `pact_strength_gain`, `pact_culture_gain`, `pact_science_gain`,
`pact_food_gain`, `pact_blocks_attack` — computed by applying
`effects._pact_blocks` for the offered side. Medium effort, medium risk
(new feature keys need to be added to weight files / defaults).

**3. (same fix generalises) Make the accept/refuse choice informed.**
The partner's `choose` move is already evaluated at 1 ply *after* the pact
exists, so accepting is at least visible — but verify the accept branch
isn't being systematically refused for the same hand-value reason. Cheap to
check once fix 1 lands and offers start happening.

**4. (higher risk, engine change — do NOT do this lightly)**
Making `_h_offer_pact` optimistically place the pact and remove it on
refusal would make it 1-ply-visible, but it changes engine semantics and
would break the fingerprint/perf_check determinism. Not recommended;
fix it in the evaluator, not the rules.

**Do not "fix" this by tuning weights.** No weight value can make a move
that produces a strictly-dominated successor state get picked.

## Impact statement

The champions have been trained on a game in which the entire diplomacy
and aggression layer never fires. That is not an engine correctness bug —
the rules are implemented — but it *is* a training-distribution bug: the
78 weights were optimised in a world where politics is "pass or seed an
event", so any weight relating to pacts, colonies, aggression defence or
war is untrained noise, and the derived human-facing advice in
`docs/HEURISTICS.md` cannot say anything about the political game.

## Colonies

### Verdict: **SAME ROOT CAUSE (1-ply invisibility + tie-break), plus a
second, 4p-only cause upstream. Still not an engine bug.**

The colonization auction is implemented and reachable
(`engine/interact.py:508-572`). Probe of 5 mirror 3p games with the 3p
champion:

| quantity | value |
|---|---|
| auction decisions | 16 |
| ...with 3 bidders still active | 3 |
| ...with 2 bidders still active | 5 |
| ...with 1 bidder still active | 8 |
| `bid` chosen | **1** |
| `bid_pass` chosen | 15 |

#### Cause A — a bid is *literally invisible* while anyone else is still in

`pending_moves` for an auction returns `[("bid_pass",), ("bid", 1), ...]`
(`engine/interact.py:47-53`) — **`bid_pass` is index 0**. Applying `("bid",
n)` when other bidders remain only mutates the `pend` dict
(`_auction_move`, `interact.py:522-542`); no player state changes. The
feature vector is built purely from player state, so **every bid evaluates
to exactly the same number as passing**, and `pick()` breaks ties with
strict `>` (`engine/bots/__init__.py:191`, `weighted.py` likewise), so the
first move — `bid_pass` — always wins. Directly observed:

```
round 12  Inhabited Territory (I)  3 bidders active
  ('bid_pass',) 102.474   ('bid',1) 102.474   ('bid',2) 102.474  ...  ALL EQUAL
round 15  Historic Territory (II)  3 bidders active
  ('bid_pass',) 95.997    ('bid',1) 95.997    ('bid',2) 95.997   ...  ALL EQUAL
```

Because the *first* bidder can never see value in bidding, everyone passes
and the territory goes to the past-events pile unclaimed. This is the same
class of bug as the pact one: a multi-step move whose payoff lands outside
the 1-ply horizon.

#### Cause B — even the visible case is rejected

When only one bidder is left active, a bid resolves the auction immediately
(`interact.py:537-541` → `colonize()`), so the colony *is* inside the trial
state and the evaluation is real. It still loses:

```
round 12  Inhabited Territory (I)   1 bidder active
  ('bid_pass',) 36.474   ('bid',1) 34.586   ('bid',2) 28.984   ('bid',3) 23.382
round 15  Developed Territory (II)  1 bidder active
  ('bid_pass',) 97.374   ('bid',1) 94.685   ('bid',2) 82.282
```

The sacrifice costs real weighted features — `workers` (1.76 at 3p) and
`unit_workers` per unit returned to the yellow bank, plus `yellow_bank`
(-0.28) and the knock-on `pop_cost`/`consumption` — while the gain is a
single `colonies` count feature. That trade is *modelled*, but with an
**untrained coefficient** (see below), and it ignores the colony's
permanent yield entirely except through the count.

#### Cause C (4 players only) — auctions never even start

3 full 4p games with the 4p champion: **zero auction decisions**, and only
16 `prepare_event` moves total. Territory cards only reach the board by
being seeded into the events deck with `prepare_event`
(`engine/actions.py:255-256, 960-969`) and then revealed. The 4p champion
has `hand_military = 0.908` (vs 0.504 at 3p), i.e. it values *holding*
military cards more than the culture `prepare_event` pays, so it passes
politics ~94% of the time and almost never seeds an event. No seeded
events → no revealed territories → no auctions. This is why 4p colony bids
(0.02/game) are even rarer than 3p (0.08/game).

### The smoking gun: these weights were never under selection

```
              colonies   pacts     (BASE_WEIGHTS default: colonies 2.0, pacts 0.5)
champion 2p    3.311     0.625
champion 3p    2.000     0.644     <- colonies is EXACTLY the untouched default
champion 4p   -0.962     0.469     <- drifted NEGATIVE
```

The 3p champion's `colonies` weight is bit-for-bit the hand-written
default: thousands of hill-climb generations never once moved it, because
no game outcome ever depended on it. The 4p champion's went *negative*,
which is pure random drift on a feature that fires ~0.02 times per game.
Any advice in `docs/HEURISTICS.md` derived from these two coefficients is
noise, and should be marked as such.

## Recommended fixes for colonies, ranked by risk

**1. (lowest risk) Break auction ties toward action, or make bids visible.**
Two cheap options, in preference order:
   a. In `features()`, add an `auction_committed` term: if the top pending
      decision is an `auction` whose `high` is this player, credit the
      expected colony (e.g. `colonies + 1` discounted by the number of
      still-active rivals). ~8 lines in `engine/bots/weighted.py`, no
      engine change. This makes the *first* bid visible and therefore
      possible.
   b. Cheaper still but cruder: in `interact.pending_moves`, put
      `("bid_pass",)` **last** rather than first, so the tie-break falls to
      the smallest legal bid instead of passing. One-line change, but it
      changes the move ordering the fingerprint depends on
      (`tools/fingerprint.json`, `engine/perf_check.py`) — coordinate with
      whoever owns the engine.

**2. Replace the bare `colonies` count with yield-aware features.**
Territories differ hugely (`Historic Territory II` = +2 happy and 11
culture now; `Vast Territory II` = +4 yellow, -1 blue, 4 food). Derive
`colony_yellow`, `colony_blue`, `colony_happy`, `colony_strength` from
`permanentEffects` so the evaluator sees what it actually bought. The
immediate effects already land in the trial state, so those need nothing.

**3. Re-run the hill climb after 1 and 2, and reset `colonies`/`pacts` to
their defaults first** — the current 4p value of -0.96 is drift, and
carrying it into a run where the feature suddenly matters would start the
search in the wrong basin.

**4. Separately, check the 4p `hand_military` weight (0.908).** It is
plausibly a genuine optimum (military cards defend attacks), but combined
with cause C it means the 4p champion opts out of events, territories,
aggressions and pacts all at once. Worth an ablation: does forcing a lower
`hand_military` at 4p change the win rate?

## Rules check (no engine defects found)

Read `engine/actions.py:240-296`, `engine/actions.py:979-1003`,
`engine/interact.py:217-228` and `engine/interact.py:464-586` against
`docs/RULES_SPEC.md` §5.9, §5.10, §11.1-11.5. Everything matched:

* pacts are gated to 3+ players (`actions.py:258`), and 2p decks drop them
  at build time via the `2p` copy counts in
  `data/cards_military_actions.json` — the 2p zero is expected, correct,
  and not evidence of anything;
* offering costs a political action but no MA (§5.9 / FAQ p.16) — matches;
* refuse returns the card to hand (`interact.py:227`) — matches;
* accepting replaces any previous pact in the owner's own area
  (`interact.py:222`, single-element list) — matches;
* auction order starts from the politics-phase player and goes clockwise
  (`interact.py:511`), passing is permanent, the last bidder must colonize
  (`interact.py:537-541`) — matches §11.2.

**One cosmetic deviation worth a follow-up (not the cause of anything
here):** `actions.py:258` gates on `len(state.active_players()) < 3`, which
is *dynamic*. In a 3-player game where someone resigns (§5.11), pacts
silently become illegal mid-game for the two survivors. The real rule is a
**setup** rule (remove pacts from the deck in a 2-player game), so the
gate should be on the number of seats, not the number of survivors. Low
impact (resign is 0.07/game) but it is a genuine rules mismatch.

## Third finding (found while verifying): WeightedBot scores other
## players' decisions from the WRONG player's point of view

`WeightedBot.pick` uses **`idx = state.current`**
(`engine/bots/weighted.py:357`). `GreedyBot.pick` correctly uses
**`state.decider()`** (`engine/bots/__init__.py:181`). They differ whenever
`state.pending` is non-empty and the pending decision belongs to somebody
other than the player whose turn it is (`engine/state.py:140-144`).

Measured over 5 mirror 3p games with the 3p champion:

| pending decision | total | evaluated from the wrong seat |
|---|---|---|
| `choice` (accept/refuse a pact, defend, lose_colony, annex, …) | 47 | **15 (32%)** |
| `auction` (colony bidding) | 16 | **10 (63%)** |

So the champion resolves most colony bids and a third of all interactive
choices by maximising **a rival's** position. The pact accept/refuse
decision (`_c_pact_offer`) is *always* one of these — the partner is by
definition not the current player — so even if fix #1 makes bots start
offering pacts, the accept side is scored backwards until this is fixed.

**Fix (trivial, do this first):** change `engine/bots/weighted.py:357` to
`idx = state.decider()`. One line. It will change self-play results, so it
invalidates the current champions and any fingerprint that covers
`WeightedBot` — but it is unambiguously a bug, and it is cheap to re-run
the climb. Note `rival_context(state, idx)` on the next line must use the
same `idx`.

## Bottom line

Neither zero-pacts nor near-zero-colonies is an engine bug. Both are the
same architectural limitation: **a 1-ply evaluator cannot see any move
whose effect is deferred to another player's decision**, and the tie-break
sends every such move to the do-nothing option. The consequence is
serious anyway — the champions were tuned in a game with no diplomacy, no
colonization and effectively no aggression, so the political half of
Through the Ages is untrained, and the `colonies`/`pacts`/aggression
weights in `experiments/champion_*.json` are unselected noise that should
not be read as advice.
