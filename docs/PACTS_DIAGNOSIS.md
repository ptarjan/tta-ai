# Why the champions never play pacts (and almost never colonize)

Status: IN PROGRESS (written incrementally; see git history of this file)
Date: 2026-07-26

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
move, and evaluates the resulting state (`engine/bots/__init__.py:141-165`,
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

The champion's `pacts` weight is dead code: no reachable 1-ply successor
state ever has a nonzero `pacts` count for the *mover*, so the hill climb
has never been able to select on it. (It can be nonzero for a player who
*accepted* a pact — but accepting is a `choose` move, and the same 1-ply
horizon applies to whether accepting looks good.)

`GreedyBot`'s 19-feature vector (`engine/bots/__init__.py:75-105`) has no
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

(section below, written next)
