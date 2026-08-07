# docs/VARIANTS.md — the archetype roster (`rust/src/bots/variants/`)

Six hand-written, rule-based opponents, ported from `engine/bots/variants/`
(Python) to `rust/src/bots/variants/` (Rust) 2026-08-06. Read
`rust/src/bots/variants/mod.rs`'s own module doc comment first — this file is
the short index, not a restatement.

## Why this exists

Self-play converges: a champion that only ever has to beat a mirror of
itself learns a plan that is good only *relative to that plan*. Each
archetype below is a real, human-cited strategy (`docs/EXPERT_STRATEGY.md`'s
"biggest open disagreements" table), so a bot trained against the pool has to
be good against six structurally different plans, not one exploit.

## The roster

| name | selectable as | plays | identity, one line |
|---|---|---|---|
| Culture | `culture` | Camp B of "Age I culture" | Overbuys a theater/temple culture rate in Age I; deliberately punishable by war. |
| Military | `military` | Napoleon "most important" | Top-2 military position, and actually cashes strength into aggressions/wars. |
| Science | `science` | Alchemy-first | Hits a published science-per-turn scale, stops staffing new labs at the ceiling. |
| Wonder | `wonder` | Michelangelo / wonder spam | The published "noob trap," played straight: wonder step outranks the military floor. |
| Infra | `infra` | Iron + Irrigation | The orthodox 3-4p book line; closest roster member to plain `BookBot`. |
| Tempo | `tempo` | 3-Bronze, buy the 5th action | The 2p line: never upgrades a mine, spends every action on the card row instead. |

Every name is a `BotKind` (`rust/src/bots/greedy.rs`), so `selfplay --bots
weighted,culture`, `--list-bots`, etc. all work today. `book` (plain
`BookBot`, v2 rules, no profile) was wired in alongside them for the same
reason — it had no name anywhere before this port.

## Shape

One shared engine (`Profile` + `RuleId` + the `r_*`/`best_*`/`politics`
functions in `mod.rs`), reused by all six. Each archetype file
(`culture.rs`, `military.rs`, ...) is just a `const PROFILE: Profile` and a
`const RULES: &[RuleId]` — a diff against `DEFAULT_PROFILE`/the shared rule
order, restated as data, plus a doc comment carrying the original Python
docstring's citations (do not lose those; they cite real expert sources).
`MilitaryBot`'s two real method overrides in Python (`mil_goal`'s
economy-first gate, `_r_tactics`) and `ScienceBot`'s (`_best_build`'s lab
ceiling) became `Profile` fields (`econ_first_until_age`,
`age_strength_floor`, `science_ceiling`) rather than special-cased code —
see `mod.rs`'s top doc comment for why that is a faithful translation, not a
simplification.

Pending decisions (auctions, aggression defence, colonization) are not
reimplemented: no Python variant overrides them either, so
`VariantBot::choose` delegates straight to `BookBot::pending_pick`.

## Bugs fixed rather than mirrored, and one legitimate quirk kept

- None found beyond what `book.rs`'s own port already fixed (see that file's
  top doc comment: the `_r_play_leader` version-table bug, the dead
  `iron_over_bronze`/`ca`/`ma` fields, the missing Churchill rule). The
  variant profiles inherit those fixes by construction, since they reuse
  `book::leader_rank`/`Ctx` directly rather than re-deriving them.
- One quirk is KEPT, not fixed, because it is not a bug: `_r_action_card`
  (every archetype) scores action cards through the unweighted,
  module-level `book::action_card_value`, never through a profile's
  `tech_bonus`/`card_bonus` — this is what the Python source actually does
  (`_r_action_card` is inherited unchanged from `BookBot` and calls the
  free function, not `self.card_value`), so no archetype's action-card
  play is profile-flavoured. Restated explicitly in `mod.rs`'s
  `r_action_card` doc comment so a future reader doesn't "fix" it.

## Legality (private-information audit)

Every function in `rust/src/bots/variants/` that reads a rival reads only
public state: `effects::state_stats` (derived from a player's own played
tableau/army — visible to everyone at a physical table), `combat::
attack_strength` (same, plus public pact state), and
`PlayerState::culture`/`government`/`wonder`/`completed_wonders`/`techs`/
`state.card_row`. `hand_civil`/`hand_military` are read only for the
deciding player's own hand (legal self-knowledge — e.g. the Leonardo/
Columbus leader conditions, the hand-size penalty in `best_take`). Nothing
in this port reads a rival's hand, and nothing iterates
`state.civil_deck`/`state.military_deck` beyond what `costs`/`effects`
already do internally for cost math — no code here reads deck order or
contents directly. Audited by hand, function by function, while porting;
see `mod.rs`'s own "Legality" doc section for the per-function detail.

## Smoke measurement (2026-08-06)

`/Users/pt/tta-ai/experiments/rust_champion_2p.json` (the live 2p weighted
champion, copied into the port's working clone — it is gitignored, never
committed) vs. `culture` at 2p, 60 games: `selfplay --bots weighted,culture
--players 2 --games 60 --weights rust_champion_2p.json --seed 1` (seats
rotate every game, so this is seat-balanced, not the arena binary's deal-
paired design — a smoke test that the port plays legal, sane games, NOT a
benchmark; a real round-robin measurement is a separate, later piece of
work).

```
bot           games  win rate  mean cult  resigned
culture          60     18.3%      139.7         0
weighted         60     81.7%      198.8         0
```

60/60 games completed, mean 272 moves/game, no resignations, no panics. The
champion beats CultureBot decisively (81.7%/18.3%) — a reversal of the
stale Python-era `docs/BOT_ROSTER.md` number (champion losing 15%/85% to
CultureBot, 2026-07-30), consistent with that measurement predating the
Rust rewrite, ~2000 generations of climbing, and six engine bug fixes. This
one number is a smoke test, not a claim about the roster's current
strength ordering.

## What was not done

- No round-robin across all 7 archetypes x all player counts — out of
  scope for this port; the user will run the real measurement separately.
- `docs/BOT_ROSTER.md` (the Python-era roster measurement, 2026-07-30) is
  NOT updated and should not be read as current: it predates the Rust
  rewrite, ~2000 generations of climbing, and six engine bug fixes.
