"""A card class must not be priced at zero on every vector that plays.

The bug this exists to prevent, in one sentence: **the suite checked that a
card was priced and never that its price was read.**  `docs/CARD_BLINDNESS.md`
wrote that sentence about itself; `unit_strength_credit` is what it cost.

The sequence was:

1. `docs/CARD_BLINDNESS.md` found the ten military unit cards had
   "zero visible gain" and were priced as pure cost.
2. `_UNIT_TO_FEATURE` + `_Y_UNIT` + `unit_strength_credit` were added, and
   `tests/test_card_pricing.py` grew four tests that all pass:
   `test_every_unit_card_prices_its_printed_strength`,
   `test_unit_strength_matches_what_the_engine_does_with_it`,
   `test_an_age_ii_cavalry_and_artillery_are_no_longer_the_same_card`,
   `test_the_credit_recovers_the_pre_fix_pricing_exactly`.
3. `unit_strength_credit` shipped at **0.0** in `DEFAULT_WEIGHTS`, so
   `card_potential` multiplied the whole new channel by zero and the ten unit
   cards priced *exactly* as they had before the fix.
4. Nothing failed, for days, while `docs/SYSTEM_COVERAGE.md` measured the bot
   taking military unit technology **0.06-0.45 times per seat-game against a
   human rate of 2.79-3.84** -- it fought whole games with Age A Warriors.

Four card audits ran without catching it (`CARD_BLINDNESS.md`,
`CARD_BLINDNESS.md`, `CARD_BLINDNESS.md`, `UNCOVERED_TYPES.md`) because
all four asked "is this card priced".  This file asks the other question, and
asks it in two ways: a **cheap structural** one that always runs, and an
**expensive behavioural** one behind `PLAY_RATE_CENSUS=1`.

See `docs/CARD_BLINDNESS.md` for the measurement the thresholds come from and
`tools/play_rate.py` for the instrument.
"""
from __future__ import annotations

import glob
import json
import os
import unittest

from engine import cards as C
from engine.bots import weighted as W

HERE = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))


def _dealt():
    """Card names that are actually dealt at some table size.

    The six starting technologies printed on the player board (Agriculture,
    Bronze, Philosophy, Religion, Warriors, Despotism) have `count` 0
    everywhere; nobody can take them, so they cannot be evidence either way.
    """
    out = []
    for c in C.db().cards:
        n = c.get("count")
        if isinstance(n, dict):
            if any((n.get(k) or 0) > 0 for k in ("2p", "3p", "4p")):
                out.append(c["name"])
        elif n:
            out.append(c["name"])
    return out


def class_gates():
    """{weight key: sorted card types} for every weight that gates ONE class.

    Derived, not written down: for each weight, perturb it by +1.0 and see
    which cards' `card_potential` moves.  A weight whose influence set is
    confined to a single card type is the ONLY per-card channel that type has,
    so at 0.0 the whole type prices identically -- which is exactly the
    `unit_strength_credit` shape.  Deriving it means a new class credit is
    covered by this file the moment it is added, instead of the moment
    somebody remembers to list it.
    """
    dealt = _dealt()
    typ = {c["name"]: c["type"] for c in C.db().cards}
    base = dict(W.DEFAULT_WEIGHTS)
    ref = {n: W.card_potential(n, base) for n in dealt}
    out = {}
    for k in W.DEFAULT_WEIGHTS:
        w2 = dict(base)
        w2[k] = base[k] + 1.0
        moved = {n for n in dealt
                 if abs(W.card_potential(n, w2) - ref[n]) > 1e-12}
        if not moved:
            continue
        types = sorted({typ[n] for n in moved})
        if len(types) == 1:
            out[k] = types
    # The unit types are four printed colours of one class -- infantry,
    # cavalry, artillery and air are all "a military unit technology", the
    # thing `engine/cards.UNIT_TYPES` names -- so a weight confined to exactly
    # that set gates one class even though it spans four `type` strings.
    units = set(C.UNIT_TYPES)
    for k in W.DEFAULT_WEIGHTS:
        w2 = dict(base)
        w2[k] = base[k] + 1.0
        moved = {typ[n] for n in dealt
                 if abs(W.card_potential(n, w2) - ref[n]) > 1e-12}
        if moved and moved <= units and k not in out:
            out[k] = sorted(moved)
    return out


#: Class gates that ship at 0.0 in `DEFAULT_WEIGHTS`, each with the reason
#: that was given for it.  **Every new league arm starts from
#: `DEFAULT_WEIGHTS`** -- `experiments/league_state/champion_3p.json` is gen 0
#: and byte-for-byte the defaults as of 2026-07-30 -- so a gate at 0.0 here is
#: a whole card class that every fresh arm begins completely blind to, and the
#: only way out is for the hill climb to stumble onto it.  Measured, that
#: happens roughly never: across 1,757 generations of the 2p/3p/4p arms
#: (72 + 1,315 + 370), 216 of them accepted, `unit_strength_credit` was moved
#: by an accepted mutation exactly ONCE, and that move made it negative.
#: See docs/CARD_BLINDNESS.md section 17.3.
#:
#: The set is asserted EXACTLY, so both directions are visible events: adding
#: a new zero-by-default class gate fails here, and fixing one fails here too
#: until the entry is deleted.
DEFAULT_ZERO_GATES = {
    "unit_strength_credit":
        "the ten military unit cards' per-worker strength.  0.0 was chosen so "
        "`tests/test_card_pricing.py::test_the_credit_recovers_the_pre_fix_"
        "pricing_exactly` could A/B the fix against itself -- and then nobody "
        "turned it on.  THIS IS THE BUG THIS FILE EXISTS FOR.",
    "defense_bonus":
        "the three Military Bonus cards' defence increment (1/3/5 over the "
        "flat 1 every military card is worth face down).  `bonus_card_credit` "
        "is 1.0, but it MULTIPLIES this weight and `colonize_bonus`, both of "
        "which are 0.0, so the whole bonus class prices at exactly 0.0.",
    "build_discount":
        "Masonry / Architecture / Engineering's resources off a wonder stage.",
    "wonder_stages_per_action":
        "the same three cards' second stage per action.",
    "free_civil_action":
        "the 18 action cards that grant a free civil action.",
    "resource_discount":
        "the 13 action cards that cheapen a build.",
    "restricted_resources":
        "the 4 action cards whose resources are earmarked.",
    "card_board_credit":
        "the 3 action cards priced by what they would do to the board.",
    "hand_limit":
        "Kremlin's civil hand limit.",
}


#: Class gates that are **non-positive on every trained vector on disk**.
#: Non-positive and not merely zero: every gate in `class_gates()` scales a
#: printed GAIN -- more strength, more defence, more colonization, a cheaper
#: build -- so a negative weight is not a preference, it is a sign error that
#: makes the card look worse for carrying the good thing.
#:
#: Measured 2026-07-30 over the three vectors in docs/CARD_BLINDNESS.md
#: (2p champion gen 72 live; 3p ladder gen 1314 and 4p ladder gen 361, both
#: the archived pre-restart champions):
#:
#:   unit_strength_credit   0.0      0.0      -0.01713
#:   defense_bonus          0.0      0.0      (absent -> 0.0)
#:   free_civil_action      0.0     -0.16007  -0.08449
#:
#: EMPTY SINCE 2026-08-02, and this is the whole point of the assertion: the
#: league climbed all three off zero on its own once it was given generations
#: to do it in.  Re-measured on the live champions (2p gen 119, 3p gen 32,
#: 4p gen 12):
#:
#:   unit_strength_credit   0.15835  0.00000  0.00449
#:   defense_bonus          0.00000  0.00000  0.07136
#:   free_civil_action      0.12849  0.00078  0.04118
#:
#: The set only ever shrinks; a name goes back in only with a fresh
#: measurement showing every trained vector at 0.0 again.
#:
#: This assertion only runs where those files exist; a fresh clone has no
#: `experiments/league_state`, and the test says so rather than passing
#: vacuously.
#:
#: `wonder_stages_per_action` went in on 2026-08-04, and the reason is a GUARD
#: rather than a verdict -- read the entry before treating it as a write-off.
#: It gates Masonry / Architecture / Engineering (nothing else), and it was
#: NEGATIVE on all three live champions: -0.13614 / -0.03634 / -0.04145.  That
#: is not "unpriced", it is priced BACKWARDS -- a standing markdown on the
#: three cards that make wonders cheap in actions, sitting next to the
#: negative net `wonder_progress` that `weighted.NET_NONNEG_PHASE` repairs
#: (docs/THEFT_IS_PRICED_BACKWARDS.md).  `hillclimb_league.NONNEG` could not
#: see it because NONNEG is derived from `DEFAULT[k] > 0` and this default is
#: exactly 0.0, so the key is in neither NONNEG nor NONPOS.
#:
#: `weighted.BENEFIT_GATES` now pins all nine such gates at >= 0.  So from
#: here on 0.0 is the GUARD'S FLOOR, not a measurement, and the honest state
#: of this coordinate is "not yet priced" -- which is what this list means.
#: `test_the_dead_set_has_not_gone_stale` asks for the line back the moment
#: the league prices it above zero, and `TestBenefitGatesAreDerived` below
#: stops the guarded set itself from going stale.
#:
#: MEASURED 2026-08-04, and it says the backwards price cost real play.  Take
#: rate per 2p seat, human corpus (1,384 seats) vs the 2p champion over 16
#: games -- `tools/play_rate.py bot --players 2 --games 16` then `report`:
#:
#:     Masonry        human 0.123   bot 0.062    2.0x under
#:     Architecture   human 0.253   bot 0.125    2.0x under
#:     Engineering    human 0.353   bot 0.094    3.8x under
#:
#: n=32 seats on the bot side, so these are rates and not intervals; the
#: DIRECTION is consistent across all three and matches the sign of the weight
#: that produced them.  Re-measure after the league has priced the coordinate
#: and put the result in docs/CARD_BLINDNESS.md.
DEAD_ON_EVERY_TRAINED_VECTOR = {"wonder_stages_per_action"}


def trained_vectors():
    """[(label, weights)] for every trained vector on this disk.

    `experiments/league_state` is gitignored, so this is empty in a fresh
    clone and the vector-level assertions skip.  `analysis/frozen` IS
    committed but predates every gate here (the keys are simply absent), so
    including it would score "absent" as "dead" and turn the ratchet into
    noise; it is deliberately not read.
    """
    ls = os.path.join(HERE, "experiments", "league_state")
    paths = sorted(glob.glob(os.path.join(ls, "champion_*.json")))
    # a ladder directory holds one file per accepted generation; only its TIP
    # is a champion, and the filenames are zero-padded so the last name is it.
    for d in sorted(glob.glob(os.path.join(ls, "*", "ladder_*p"))):
        tips = sorted(glob.glob(os.path.join(d, "gen*.json")))
        if tips:
            paths.append(tips[-1])
    out = []
    for path in paths:
        try:
            with open(path) as fh:
                w = json.load(fh)
        except Exception:
            continue
        w = w.get("weights", w)
        if isinstance(w, dict) and "culture" in w:
            out.append((os.path.relpath(path, HERE), w))
    return out


class TestClassGatesAreDerivedNotDeclared(unittest.TestCase):
    """The gate list must come from the code, so a new one cannot hide."""

    def test_every_derived_gate_is_written_down(self):
        derived = class_gates()
        listed = set(DEFAULT_ZERO_GATES)
        live = {k for k in derived if W.DEFAULT_WEIGHTS[k] != 0.0}
        missing = set(derived) - listed - live
        self.assertEqual(
            missing, set(),
            "new zero-by-default class gate(s) %s -- a whole card class is "
            "priced at exactly 0.0 for every arm that starts from "
            "DEFAULT_WEIGHTS.  Add it to DEFAULT_ZERO_GATES with the reason, "
            "or give it a non-zero default." % sorted(missing))

    def test_no_stale_entries(self):
        derived = class_gates()
        stale = {k for k in DEFAULT_ZERO_GATES
                 if k not in derived or W.DEFAULT_WEIGHTS.get(k) != 0.0}
        self.assertEqual(
            stale, set(),
            "%s is listed as a zero-by-default class gate but is no longer "
            "one.  Delete the entry -- a stale write-off is how the previous "
            "audits stayed green." % sorted(stale))

    def test_the_unit_class_is_gated_by_exactly_one_weight(self):
        """The specific shape that caused this: nine dealt unit cards whose
        only per-card channel is one weight."""
        derived = class_gates()
        units = set(C.UNIT_TYPES)
        gates = {k for k, ts in derived.items() if set(ts) <= units}
        self.assertEqual(gates, {"unit_strength_credit"}, sorted(gates))


class TestNoClassIsDeadOnEveryTrainedVector(unittest.TestCase):
    """The `unit_strength_credit` invariant, at vector level.

    A gate can be zero in the defaults and still be fine if training moves it.
    This asserts what training actually did, against the vectors on disk.
    """

    def _dead(self):
        vecs = trained_vectors()
        if not vecs:
            self.skipTest("no trained vector on disk (experiments/league_state"
                          " is gitignored); run the league or copy a champion")
        dead = set()
        for k in class_gates():
            # The INTENDED sign is the default's, and a default of 0.0 means
            # "prices a gain" -- every zero-default gate here scales a printed
            # benefit.  `yellow_bank` is the counter-example that makes this
            # necessary: it gates the twelve territories and defaults to -0.1
            # because a drained yellow bank is a cost, so a plain `<= 0` rule
            # would call the one gate the league trained HARDEST (-0.747 /
            # -3.740) dead.
            want = 1.0 if W.DEFAULT_WEIGHTS[k] >= 0.0 else -1.0
            vals = [w.get(k, W.DEFAULT_WEIGHTS[k]) * want for _lbl, w in vecs]
            if all(v <= 0.0 for v in vals):
                dead.add(k)
        return dead, vecs

    def test_the_dead_set_has_not_grown(self):
        dead, vecs = self._dead()
        new = dead - DEAD_ON_EVERY_TRAINED_VECTOR
        self.assertEqual(
            new, set(),
            "%s now prices its whole card class at <= 0 on ALL %d trained "
            "vectors (%s).  That is the unit_strength_credit failure again: "
            "the class is priced, the price is never read.  Measure the play "
            "rate with `python3 tools/play_rate.py` before writing it off."
            % (sorted(new), len(vecs), ", ".join(l for l, _ in vecs)))

    def test_the_dead_set_has_not_gone_stale(self):
        dead, vecs = self._dead()
        revived = DEAD_ON_EVERY_TRAINED_VECTOR - dead
        self.assertEqual(
            revived, set(),
            "%s is now positive on at least one trained vector.  Delete it "
            "from DEAD_ON_EVERY_TRAINED_VECTOR and record the play rate that "
            "resulted in docs/CARD_BLINDNESS.md." % sorted(revived))


class TestBenefitGatesAreDerived(unittest.TestCase):
    """`weighted.BENEFIT_GATES` is a written-down list of a DERIVABLE fact, so
    it can go stale the moment somebody adds a class credit.  This re-derives
    it the way `class_gates` does and demands the two agree.

    The derivation, stated as a rule: a weight whose only per-card channel is
    one card class, whose default is exactly 0.0, and which raises
    `card_potential` for EVERY card in that class, scales a printed grant --
    and a grant is never a reason not to take the card.  `yellow_bank` is the
    counter-example the rule has to survive: it also gates one class, but its
    default is -0.1 because a drained bank is a cost, so the `== 0.0` clause is
    doing real work rather than decorating the sentence.
    """

    def test_the_guarded_set_is_exactly_the_zero_default_grant_gates(self):
        base = dict(W.DEFAULT_WEIGHTS)
        derived = set()
        for k, _types in class_gates().items():
            if W.DEFAULT_WEIGHTS[k] != 0.0:
                continue
            w2 = dict(base)
            w2[k] = base[k] + 1.0
            deltas = [W.card_potential(n, w2) - W.card_potential(n, base)
                      for n in _dealt()]
            moved = [d for d in deltas if abs(d) > 1e-12]
            if moved and all(d > 0.0 for d in moved):
                derived.add(k)
        self.assertEqual(
            derived, set(W.BENEFIT_GATES),
            "weighted.BENEFIT_GATES no longer matches the gates derived from "
            "the card database.  A gate MISSING from BENEFIT_GATES is "
            "unguarded and free to train negative -- which is how "
            "wonder_stages_per_action reached -0.136 on the 2p champion.  A "
            "gate listed but not derived is either renamed or no longer "
            "confined to one class; check before deleting it.")

    def test_the_guard_pins_a_negative_grant_at_zero(self):
        """Behavioural, not a restatement of the list: a vector that prices a
        printed grant negatively must not survive a load."""
        w = dict(W.DEFAULT_WEIGHTS)
        w["wonder_stages_per_action"] = -0.5
        w["unit_strength_credit"] = -2.0
        out, viol = W.dominance_repair(w)
        self.assertEqual(out["wonder_stages_per_action"], 0.0)
        self.assertEqual(out["unit_strength_credit"], 0.0)
        self.assertEqual({v["weight"] for v in viol},
                         {"wonder_stages_per_action", "unit_strength_credit"})

    def test_a_positive_grant_is_left_alone(self):
        """Negative control.  The guard pins a sign, it does not pin a value:
        the league is free to price a grant as high as it likes."""
        w = dict(W.DEFAULT_WEIGHTS)
        w["wonder_stages_per_action"] = 4.25
        out, viol = W.dominance_repair(w)
        self.assertEqual(out["wonder_stages_per_action"], 4.25)
        self.assertEqual([v for v in viol
                          if v["weight"] == "wonder_stages_per_action"], [])


#: Per-seat-game take rates the human corpus supports, by card type, at 2p.
#: Source: `python3 tools/play_rate.py human` over the 1,011-game BGO corpus
#: in `sources/bgo/journals.tar.gz` (692 2p games, 1,384 seat-games), summed
#: over the type's base names -- the same numbers as the "by card type" block
#: of docs/CARD_BLINDNESS.md.
#:
#: The floor is **one eighth of the human rate**, which is a deliberately
#: loose bar: the bot is not required to play like a human, only to be within
#: an order of magnitude of one on a whole card class.  It is set from what
#: the failure actually looked like -- military unit technology at 0.06-0.45
#: against 2.79-3.84 is a factor of 6 to 46 -- and from what the passing
#: classes measure, none of which is anywhere near a factor of 8 down.  A
#: class that falls below this is not making a stylistic choice; it cannot
#: see the cards.
HUMAN_TAKES_2P = {
    "action": 12.98,
    "unit": 3.84,
    "leader": 3.70,
    "special-tech": 3.08,
    "wonder": 2.87,
    "lab": 1.62,
    "government": 1.37,
    "farm": 1.34,
    "mine": 1.18,
    "library": 0.70,
    "theater": 0.65,
    "temple": 0.51,
    "arena": 0.32,
}
FACTOR = 8.0


class TestBotPlayRateAgainstHumans(unittest.TestCase):
    """The behavioural half.  Expensive: a real self-play census.

        PLAY_RATE_CENSUS=1 python3 -m pytest tests/test_play_rate.py -k Bot

    Off by default because a 12-game 2p census at `plan:width=2` is ~8 minutes
    -- far past what belongs in a 1,000-test suite that runs on every commit.
    The cheap structural half above is what always runs, and it is the half
    that would have caught `unit_strength_credit` on the day it shipped.
    """

    @classmethod
    def setUpClass(cls):
        if os.environ.get("PLAY_RATE_CENSUS") != "1":
            raise unittest.SkipTest("set PLAY_RATE_CENSUS=1 to run the census")
        champ = os.path.join(HERE, "experiments", "league_state",
                             "champion_2p.json")
        if not os.path.exists(champ):
            raise unittest.SkipTest("no 2p champion on disk")
        import sys
        sys.path.insert(0, os.path.join(HERE, "tools"))
        import play_rate as PR
        games = int(os.environ.get("PLAY_RATE_GAMES", "12"))
        out = os.path.join("/tmp", "play_rate_test_2p.json")
        PR.run_bot("plan:%s,width=2,det=1" % champ, 2, games, 0, out)
        cls.blob = json.load(open(out))
        cls.PR = PR

    def test_no_card_class_falls_an_order_of_magnitude_below_humans(self):
        PR = self.PR
        idx = PR.db_index()
        by_name = {n: b for b, e in idx.items() for n in e["names"]}
        typ = {c["name"]: c["type"] for c in C.db().cards}
        seats = self.blob["totals"]["seats"]
        takes = self.blob["names"].get("card_take", {})
        got = {}
        for name, n in takes.items():
            t = typ.get(name)
            t = "unit" if t in C.UNIT_TYPES else t
            got[t] = got.get(t, 0) + n
        bad = []
        for t, human in sorted(HUMAN_TAKES_2P.items()):
            rate = got.get(t, 0) / seats
            if rate < human / FACTOR:
                bad.append("%s: bot %.2f vs human %.2f (%.0fx down)"
                           % (t, rate, human, human / max(rate, 1e-6)))
        self.assertEqual(bad, [], "card classes the bot barely takes over %d "
                                  "seat-games: %s" % (seats, "; ".join(bad)))


if __name__ == "__main__":
    unittest.main()
