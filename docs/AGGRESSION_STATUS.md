# Aggression is live; defence is where it still loses (2026-08-04)

Re-measurement of the question [`docs/AGGRESSION_RATE.md`](AGGRESSION_RATE.md)
(2026-07-30) and [`docs/WAR_RATE_CENSUS.md`](WAR_RATE_CENSUS.md) (2026-07-31)
left open, against **today's** 2p champion rather than the ones those
documents ran on.

    nice -n 19 python3.13 tools/aggression_census.py \
      --spec plan:experiments/league_state/champion_2p.json,width=2,det=1 \
      --players 2 --games 12 --workers 2

n=12 games, 471 politics decisions. Small — every rate below is a rate, not a
confidence interval, and the defence counts especially are single digits.

## 1. The "the bot never attacks" claim is wrong, and here is where it came from

| | value |
|---|---|
| aggressions resolved | **0.917 / game** |
| aggression held / offered / chosen | 259 / 104 / **11** |
| war held / offered / chosen | 184 / 148 / **23** |

Wars were a *structural zero* on 2026-07-30 — `_h_war` wrote two fields no
feature read, so declaring war was a pure cost. They are not zero now.

The "never plays an aggression or a war" sentence still in
`tests/test_coordinate_registry.py::_probe_attack` is **true of what it
describes and does not describe the league**: it is a `WeightedBot` on
`DEFAULT_WEIGHTS` at 1 ply, which is the corpus sampler. `docs/AGGRESSION_RATE.md`
§1 already established that 1 ply is the wrong regime to read a war rate off
(0.040/game there, 7-9x higher under search). Anyone quoting that docstring
as a fact about the champion — as this repo's own coordinator did on
2026-08-04 — is quoting it out of its stated scope.

## 2. What actually filters an aggression, in order

Of 259 decisions holding an aggression card:

* **155 (60%) the RULES declined** — `actions._politics_moves` refuses the
  move when `defense_strength(target) >= attack_strength(me)` (§5.4.2 pact
  block and the strength test, `engine/actions.py:316-330`). This is correct
  play, not a bug, and in a champion *mirror* it is the expected majority: two
  identical civilisations are usually not stronger than one another. It does
  say the bot is not building a strength lead it could cash.
* 104 offered, **11 chosen (10.6%)**. Declined for: `prepare_event` 57,
  `war` 15, `pol_pass` 21. So when it passes on an aggression it is usually
  doing something else military, not sitting still.

## 3. The remaining hole: it gives up defences it could win

11 defences faced:

| | n |
|---|---|
| impossible (no arithmetic line) | 7 |
| **reachable** | **4** |
| ..attempted | 2 |
| ..**GAVE UP while reachable** | **2** |
| winnable on ONE card | 0 |
| needs 2+ cards | 4 (gave up 2) |

Every reachable defence in this sample needed **two or more cards**, and it
abandoned half of them. Zero needed only one card — so the observed give-ups
are exactly the multi-card case and nothing else.

That shape is the same horizon failure that made wars a structural zero: the
first defence card leaves the defence *pending*, so the state the bot scores
after playing it shows the cost of the card and none of the benefit, and the
second card is never reached. `docs/COMBAT_AUDIT.md` §2.6.3's "1,104 winnable,
zero won" is the same number before search; search has moved it off zero
(2 of 4 attempted here) without closing it.

**This is the open item, not the attack rate.** A bot that attacks at ~0.9/game
and cannot defend a two-card defence is paying the aggression tax in both
directions.

## 4. Not measured here

* 3p/4p — this ran 2p only, and `docs/SYSTEM_COVERAGE.md` §3 records the war
  rate is *worse* at higher player counts, so 2p is the friendly case.
* Whether the attack rate is now above or below the human rate.
  `docs/WAR_RATE_CENSUS.md` had it at 2.9x human on the 2026-07-31 champion;
  nothing here re-derives that, and the champion has moved 30+ generations
  since.
