# Heuristics for human players

**Through the Ages: A New Story of Civilization — base game, 2015 edition, no expansion.**

Written for someone sitting at a table with the physical game. Everything here
comes from a rules-complete engine plus a self-play AI that is still training.

---

## How to read this document

**Snapshot warning.** The AI is mid-training. Every number below is a snapshot
taken on **2026-07-26**, with the champions at roughly generation 43 (2 players),
32 (3 players) and 25 (4 players). The behaviour samples were harvested a few
generations earlier, at 60 games per player count. Numbers will move. Structural
advice ("spend your actions", "science early, culture late") is much more stable
than any single figure.

**Where the numbers come from.**

| Source | What it is |
|---|---|
| `experiments/behaviour_{2,3,4}p.json` | 60 self-play games per player count. What the champion actually *did*: milestone rounds, worker splits, rates by age, cards bought. Board snapshots taken at the end of each of its turns. |
| `experiments/analyze_weights.py` | Which of the 78 evaluation weights the search moved away from the hand-set defaults, and in which direction. |
| `experiments/PROGRESS.md` | Strength measurements against fixed baselines, and the search's history. |
| `docs/RULES_SPEC.md` | The rules themselves — every table in the Quick reference section is rulebook-accurate, not learned. |

**Confidence tags.** Each claim is tagged:

- **[rules]** — a fact from the rulebook. Not an opinion.
- **[strong]** — the behaviour data and the weight drift agree, and it holds at
  more than one player count.
- **[mixed]** — the player counts disagree, or two sources point different ways.
  Read the caveat before acting on it.
- **[provisional]** — one player count, one climb, small sample, or a plausible
  artefact of how the AI searches. Interesting, not proven.

**Two honest caveats you should carry through the whole document.**

1. *The 2-player champion has barely learned anything.* It has accepted **one**
   mutant in its whole run; its weights sit an order of magnitude closer to the
   hand-set starting point than 3p or 4p (mean drift 0.05 vs 0.81 and 0.50), and
   it is statistically indistinguishable from its own start point (44.8% ± 9.9
   against it, null 50%). **2p advice below is mostly hand-set intuition that
   simply hasn't been refuted**, not something the search discovered. Treat it
   as a decent default, not as evidence.
2. *All behaviour numbers come from mirror self-play.* The champion plays copies
   of itself, so any "relative to opponents" figure is close to 1.0 by
   construction and tells you very little. Absolute figures (my strength, my
   science rate, my worker split) are the useful ones.

---

## If you remember nothing else

Eight rules. In rough order of how much they are worth.

1. **Spend all your civil actions, every single turn.** Actions do not carry
   over [rules]. The strongest-improving climb (3p) is also the one that wastes
   the fewest — it ends its turns with 1.77 unused civil actions against 2.75
   and 2.98 for the other two, and it leaves *something* unspent on 46% of turns
   against 68% and 71%. The search independently drove the "leftover actions are
   nice" weights to zero or below at every count that moved. **[strong]**

2. **Take a leader on round 1 or 2 and put it in play by round 3.** Median round
   to take one: 2 at all three player counts. Median round to play one: 3 (2p),
   4 (3p), 3 (4p). The champion has a leader out in 75% of Age I turns at 2p and
   70% at 4p. The "having any leader in play" weight more than doubled at 3p
   (+1.5 → +3.4). **[strong]**

3. **Upgrade your production on round 2.** At 2p and 4p the champion's first
   farm/mine upgrade lands on round 2 in **100% of games** (median and both
   quartiles are round 2). First urban building upgrade follows on round 3-4.
   3p is the exception and delays it — see the per-count section. **[strong]**

4. **Build about three temples, and never let an uprising happen.** Temples are
   the single most-built card type at every player count: 2.83 / 2.95 / 2.67
   builds per game, first one around round 5-8. An uprising cancels your entire
   production phase [rules] and carries the largest penalty in the whole
   78-weight evaluation (−12 by hand; the search pushed it to −13.2 / −17.6 /
   −13.1). **[strong]**

5. **Science first, culture later — the crossover is in Age II.** Champion
   science-to-culture ratio by age (2p / 3p / 4p): Age I **0.96 / 1.94 / 0.85**,
   Age II 0.79 / 0.63 / 0.46, Age III 0.82 / 0.56 / 0.32, Age IV 0.88 / 0.49 /
   0.28. It falls monotonically at all three counts. Both learning climbs raised
   "early culture rate" (+54% / +42%) and cut "late science rate" (−40% / −30%).
   **[strong]**

6. **Do not hoard. Not science points, not cards.** The 3p search flipped the
   sign on banked science points (+0.5 → −0.19): unspent science is dead weight.
   Both learning climbs raised the value of cards in hand *early* and cut it
   *late* (−78% / −50%). Hold cards in Ages A-I; cash them out from Age II on.
   **[strong]**

7. **Stop buying rate in Age III.** Both climbs that moved cut the value of
   late-game resource rate (−54% / −31%) and late-game science rate (−40% /
   −30%). A farm or lab bought in Age III does not pay for itself before
   scoring. Buy culture instead. **[strong]**

8. **Keep military at parity with the strongest player — that is all you need,
   until 3 players.** The champions never attack: **zero wars** in 180 games,
   and 4 aggressions total (all at 3p). They keep strength at 1.02-1.06× the
   table in every age. But the size of army needed to hold that parity is wildly
   different by count — see the per-count section. **[mixed]**

---
