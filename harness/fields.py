"""What must the human actually type?  Ask the evaluator, do not guess.

The expensive part of a human-in-the-loop game is transcription, and
`docs/INFORMATION_AUDIT.md` measured that today's evaluator is blind to almost
everything a human would dutifully type in: the card row, opponents' hands,
opponents' food/resources/actions/techs/wonders.  Typing those is pure waste.

But the feature set is *growing* -- the card row, row costs and opponents'
civil hands are being wired into `features()` right now.  A hardcoded "minimal
field list" would be wrong the day that lands, and wrong in the dangerous
direction: the harness would stop collecting something the bot has started
reading, and the measurement would quietly become a measurement of a bot
playing blindfolded.

So the list is not written down anywhere.  It is *derived*, here, by
perturbation: take the live position, change one thing a human could observe,
re-score every legal move exactly the way `advisor.rank_moves` does, and see
whether the bot's decision moved.  If it did, the human has to type it.  If
nothing moved -- not the chosen move, not the ranking, not even the raw scores
-- the human must not be asked.

Adding a *new* observable to the harness means adding a `Probe` below.
Wiring a new *feature* into the evaluator means doing nothing at all here: the
probe that already covers that observable will start reporting `MOVE` on its
own, and `harness.play` will start prompting for it.
"""

from __future__ import annotations

import random
from dataclasses import dataclass, field
from typing import Callable

from engine import actions as A
from engine import cards as C
from engine import effects
from engine import game as G
from engine.bots import weighted as W
from engine.bots.fastcopy import copy_state

# ---------------------------------------------------------------- verdicts
#
# Ordered from "must be typed" to "must not be asked for".

LEGAL = "legal"    # the set of legal moves itself changed.  Mandatory for a
                   # reason that has nothing to do with the evaluator: the bot
                   # cannot take a card the mirror does not have.  Kept apart
                   # from MOVE so the report never implies the evaluator reads
                   # something it is blind to.
MOVE = "move"      # same moves available, different one chosen.  Mandatory.
RANK = "rank"      # same top move, different ordering below it.  Mandatory:
                   # a different weight vector or a deeper search diverges here,
                   # and the whole point of logging is to re-score later.
SCORE = "score"    # scores moved but the ranking is identical.  Advisory --
                   # decision-irrelevant for a 1-ply argmax, but it is a real
                   # dependency and worth recording once per game.
INERT = "inert"    # byte-identical scores.  The evaluator cannot see this.
                   # Asking a human for it is unpaid data entry.

RANKS = {LEGAL: 4, MOVE: 3, RANK: 2, SCORE: 1, INERT: 0}
MANDATORY = (LEGAL, MOVE, RANK)
# verdicts that mean "a feature reads it", as opposed to "the rules need it"
EVALUATED = (MOVE, RANK, SCORE)


def worse(a, b):
    """The stronger (more demanding) of two verdicts."""
    return a if RANKS[a] >= RANKS[b] else b


# ------------------------------------------------------------------ probes


@dataclass(frozen=True)
class Probe:
    """One observable a human could read off the app screen.

    `mutate(state, seat)` perturbs the mirror the way a *mis-transcription*
    of this observable would.  It must produce a legal-ish state: the point is
    to measure evaluator sensitivity, not to test the rules engine.
    """

    id: str
    label: str                    # what the operator reads off the screen
    ask: str                      # the advisor patch syntax, "" if not askable
    scope: str                    # rival | shared | self
    mutate: Callable
    # rough seconds of human time per round if this turns out to be mandatory
    seconds: float = 2.0
    # some observables are needed for reasons other than evaluation
    always: str = ""              # non-empty => mandatory regardless of verdict


def _rivals(state, seat):
    return [p for p in state.players if p.idx != seat and not p.resigned]


def _bump(attr, delta):
    def go(state, seat):
        for p in _rivals(state, seat):
            setattr(p, attr, getattr(p, attr) + delta)
            effects.invalidate(state, p)
    return go


_POOL_CACHE = {}


def _civil_pool(num_players=3):
    """Every civil card that can be in a game, by age.

    Built from `CardDB.civil_deck`, which is the same source the engine deals
    from, so a substituted card is always a card that could really have been
    there.
    """
    key = num_players
    if key not in _POOL_CACHE:
        db = C.db()
        by_age = {}
        for age in ("A", "I", "II", "III"):
            try:
                names = db.civil_deck(age, num_players)
            except Exception:
                names = []
            by_age[age] = sorted(set(names))
        _POOL_CACHE[key] = by_age
    return _POOL_CACHE[key]


def _other_card(name, pool):
    """A different card of the same age, so costs and legality stay comparable."""
    info = C.db().get(name) or {}
    for cand in pool.get(info.get("age"), ()):
        if cand != name:
            return cand
    return None


def _any_card(pool):
    for age in ("I", "II", "A", "III"):
        if pool.get(age):
            return pool[age][0]
    return None


def _swap_row(state, seat):
    pool = _civil_pool(state.num_players)
    row = list(state.card_row)
    changed = False
    for i, name in enumerate(row):
        if name:
            alt = _other_card(name, pool)
            if alt and alt != name:
                row[i] = alt
                changed = True
    if not changed:
        raise ValueError("could not substitute any row card")
    state.card_row = row


def _reverse_row(state, seat):
    """Same cards, reversed -- so every card's civil-action cost changes.

    This is the probe that matters most for the row work in flight: slot 0
    costs 1 CA and slot 9 costs 3 (`engine/actions.py:36-45`), so a bot that
    prices row depth correctly must notice.
    """
    cards = [n for n in state.card_row if n]
    cards.reverse()
    state.card_row = cards + [None] * (len(state.card_row) - len(cards))


def _clear_row(state, seat):
    state.card_row = [None] * len(state.card_row)


def _rival_hand_ids(state, seat):
    pool = _civil_pool(state.num_players)
    changed = False
    for p in _rivals(state, seat):
        new = [_other_card(n, pool) or n for n in p.hand_civil]
        changed = changed or new != list(p.hand_civil)
        p.hand_civil = new
    if not changed:
        raise ValueError("no rival civil card could be substituted")


def _rival_hand_size(state, seat):
    """One more card in each rival hand, WITHOUT an identity.

    Deliberately `hidden_civil` rather than appending a named card: that keeps
    this probe a pure test of the *size* channel (`hc=`, which is all the
    operator types) and leaves the identity channel entirely to
    `_rival_hand_ids`.  Appending a real card moved both at once and made the
    two verdicts uninterpretable.
    """
    for p in _rivals(state, seat):
        p.hidden_civil += 1


def _rival_mil_hand_size(state, seat):
    for p in _rivals(state, seat):
        p.hand_military = list(p.hand_military)[:-1] if p.hand_military else []


def _rival_techs(state, seat):
    """Move a worker off every rival production card.

    Their *board*, as opposed to the three rate scalars the app prints on top
    of it.  If this is INERT and the rate probes are not, the human should be
    reading the app's own summary numbers instead of mirroring boards -- which
    is the single biggest saving in the whole design.
    """
    for p in _rivals(state, seat):
        for t in p.techs.values():
            if t.workers:
                t.workers -= 1
                break
        effects.invalidate(state, p)


def _rival_wonders(state, seat):
    for p in _rivals(state, seat):
        p.completed_wonders = list(p.completed_wonders)[:-1]
        effects.invalidate(state, p)


def _rival_colonies(state, seat):
    for p in _rivals(state, seat):
        p.colonies = list(p.colonies)[:-1]
        effects.invalidate(state, p)


def _rival_wonder_progress(state, seat):
    for p in _rivals(state, seat):
        p.wonder = None
        effects.invalidate(state, p)


def _rival_gov(state, seat):
    for p in _rivals(state, seat):
        p.government = "Despotism"
        effects.invalidate(state, p)


def _clear_future_events(state, seat):
    state.future_events = []
    state.seeded_by = {}


def _clear_current_events(state, seat):
    state.current_events = []


def _mask_civil_deck(state, seat):
    """Keep the deck's *size* (which decides when the age ends) but destroy
    every card identity in it."""
    if state.civil_deck:
        state.civil_deck = [state.civil_deck[0]] * len(state.civil_deck)


def _clear_mil_discard(state, seat):
    state.discarded_military = {}


PROBES = [
    # ---- rival scalars the app prints directly on each player panel
    Probe("rival.culture", "rival culture (the score number)", "p{i} c=",
          "rival", _bump("culture", 25), 2.0,
          always="the final score, so it is in the result record regardless"),
    Probe("rival.culture_rate", "rival culture per turn", "p{i} cr=",
          "rival", _bump("culture_rate_extra", 4), 2.0),
    Probe("rival.science_rate", "rival science per turn", "p{i} sr=",
          "rival", _bump("science_rate_extra", 4), 2.0),
    Probe("rival.strength", "rival military strength", "p{i} str=",
          "rival", _bump("strength_extra", 8), 2.0),
    Probe("rival.science", "rival science stock", "p{i} s=",
          "rival", _bump("science", 20), 2.0),
    Probe("rival.food", "rival food stock", "p{i} f=",
          "rival", _bump("food", 15), 2.0),
    Probe("rival.resources", "rival resource stock", "p{i} r=",
          "rival", _bump("resources", 15), 2.0),
    Probe("rival.civil_actions", "rival civil actions left", "p{i} ca=",
          "rival", _bump("civil_actions", 2), 1.5),
    Probe("rival.military_actions", "rival military actions left", "p{i} ma=",
          "rival", _bump("military_actions", 2), 1.5),
    Probe("rival.workers_free", "rival unused workers", "p{i} fw=",
          "rival", _bump("workers_free", 3), 1.5),
    Probe("rival.yellow_bank", "rival yellow bank", "p{i} yel=",
          "rival", _bump("yellow_bank", 5), 1.5),
    Probe("rival.happy", "rival happiness", "p{i} hap=",
          "rival", _bump("happy_extra", 3), 1.5),
    # ---- rival board and hands (the expensive stuff)
    Probe("rival.techs", "rival tableau: every tech and its workers",
          "p{i} tech+ <card>:<n>", "rival", _rival_techs, 25.0),
    # a name when one is completed (a handful of times a game), plus one digit
    # a round for `mirror.RIVAL_CHECKS` to verify none was missed
    Probe("rival.wonders", "rival completed wonders", "p{i} built+ <wonder>",
          "rival", _rival_wonders, 2.5),
    # colonies and a wonder-in-progress sit FACE UP in the player's area, so
    # both are transcription of public information.  By name, with a count
    # checked every round, for the same reason completed wonders are: the name
    # carries printed effects the count cannot.
    Probe("rival.colonies", "rival colonies", "p{i} colony+ <territory>",
          "rival", _rival_colonies, 2.5),
    Probe("rival.wonder_progress", "rival wonder under construction",
          "p{i} wonder <wonder> <steps>", "rival", _rival_wonder_progress, 2.5),
    Probe("rival.government", "rival government", "p{i} gov=",
          "rival", _rival_gov, 2.0),
    Probe("rival.hand_civil_size", "rival civil hand size", "p{i} hc=",
          "rival", _rival_hand_size, 2.0),
    Probe("rival.hand_civil_ids",
          "rival civil hand CONTENTS (public by the rules)",
          "p{i} hand <card>, <card>", "rival", _rival_hand_ids, 8.0),
    Probe("rival.hand_military_size", "rival military hand size", "p{i} hm=",
          "rival", _rival_mil_hand_size, 2.0),
    # ---- shared board
    Probe("row.contents", "which cards are in the card row",
          "deal <card> <card>", "shared", _swap_row, 13.0,
          always="the bot cannot take a card the mirror does not have"),
    Probe("row.order", "the LEFT-TO-RIGHT ORDER of the row (= CA cost)",
          "row <card>, ...", "shared", _reverse_row, 4.0),
    Probe("row.occupancy", "which row slots are empty",
          "row ...", "shared", _clear_row, 2.0,
          always="legality of every take"),
    Probe("events.future", "the face-down politics deck",
          "(not askable: hidden)", "shared", _clear_future_events, 0.0),
    Probe("events.current", "the current events deck",
          "event <card>", "shared", _clear_current_events, 3.0),
    Probe("deck.civil_identity", "which civil cards are left in the deck",
          "(not askable: hidden)", "shared", _mask_civil_deck, 0.0),
    Probe("deck.military_discard", "the military discard pile",
          "(not askable in the app)", "shared", _clear_mil_discard, 0.0),
]

PROBES_BY_ID = {p.id: p for p in PROBES}


# --------------------------------------------------------------- the probe


def _score_moves(state, weights):
    """Exactly `advisor.rank_moves`' scoring path, as a {move: score} map.

    Kept in lockstep with `advisor.advisor.rank_moves` deliberately: if the two
    diverge, the harness would be deriving its field list from a bot other than
    the one in the room.
    """
    moves = G.legal_moves(state)
    if not moves:
        return {}
    idx = state.decider()
    moves = [m for m in moves if m[0] != "resign"] or moves
    try:
        ctx = W.rival_context(state, idx)
    except Exception:
        ctx = None
    end_bias = weights.get("end_turn_bias", 0.0)
    out = {}
    for mv in moves:
        trial = copy_state(state)
        try:
            A.apply(trial, mv, random.Random(0))
            val = W.evaluate(trial, idx, weights, ctx)
        except Exception:
            continue
        if mv[0] == "end_turn":
            val += end_bias
        out[tuple(mv)] = val
    return out


def _verdict(base, after):
    if not base or not after:
        return INERT
    if set(base) != set(after):
        # the perturbation changed which moves exist at all: a rules-level
        # dependency, not an evaluation one
        return LEGAL
    if all(abs(base[m] - after[m]) < 1e-12 for m in base):
        return INERT
    order_b = [m for m, _ in sorted(base.items(), key=lambda kv: -kv[1])]
    order_a = [m for m, _ in sorted(after.items(), key=lambda kv: -kv[1])]
    if order_b[0] != order_a[0]:
        return MOVE
    if order_b != order_a:
        return RANK
    return SCORE


def probe_position(state, seat, weights, probes=PROBES):
    """Classify every probe against one position.  Returns {probe_id: verdict}."""
    base = _score_moves(state, weights)
    out = {}
    for pr in probes:
        trial = copy_state(state)
        try:
            pr.mutate(trial, seat)
            after = _score_moves(trial, weights)
        except Exception:
            # A probe that cannot be applied to this position tells us nothing.
            # Erring toward INERT would silently stop the harness asking for
            # something the bot reads, so err the other way: the operator keeps
            # typing it and the bug is visible instead of invisible.
            out[pr.id] = RANK
            continue
        out[pr.id] = _verdict(base, after)
    return out


def probe_positions(positions, seat, weights, probes=PROBES):
    """Union over several positions: the strongest verdict each probe earns."""
    out = {pr.id: INERT for pr in probes}
    for st in positions:
        for pid, v in probe_position(st, seat, weights, probes).items():
            out[pid] = worse(out[pid], v)
    return out


@dataclass
class Requirement:
    probe: Probe
    verdict: str
    reason: str

    @property
    def mandatory(self):
        return self.verdict in MANDATORY or bool(self.probe.always)

    def to_json(self):
        return {"id": self.probe.id, "verdict": self.verdict,
                "mandatory": self.mandatory, "label": self.probe.label,
                "ask": self.probe.ask, "scope": self.probe.scope,
                "reason": self.reason}


def requirements(verdicts, probes=PROBES):
    """Turn raw verdicts into the operator-facing list, mandatory first."""
    out = []
    for pr in probes:
        v = verdicts.get(pr.id, INERT)
        if pr.always and v not in MANDATORY:
            reason = f"not read by the evaluator, but required: {pr.always}"
        elif v == LEGAL:
            reason = "changes which moves are legal at all (rules, not eval)"
        elif v == MOVE:
            reason = "changes the move the bot plays"
        elif v == RANK:
            reason = "changes the ranking below the top move"
        elif v == SCORE:
            reason = "shifts scores but never the ranking (advisory)"
        else:
            reason = "the evaluator is blind to it -- do not transcribe"
        out.append(Requirement(pr, v, reason))
    out.sort(key=lambda r: (not r.mandatory, -RANKS[r.verdict], r.probe.id))
    return out


def transcription_cost(reqs, num_rivals):
    """Seconds of human time per round implied by a requirement list."""
    total = 0.0
    for r in reqs:
        if not r.mandatory:
            continue
        n = num_rivals if r.probe.scope == "rival" else 1
        total += r.probe.seconds * n
    return total


# -------------------------------------------------- positions to probe with


def sample_positions(num_players=3, seat=0, count=4, seed=7, weights=None):
    """A handful of real mid-game positions for `seat`, from cheap self-play.

    Used for the pre-flight report (`python3 -m harness.fields`).  In a live
    session `harness.play` probes the actual board instead, which is both free
    and more honest.
    """
    bot = W.WeightedBot(weights, seed=seed)
    st = G.new_game(num_players, seed)
    rng = random.Random(seed)
    out, guard = [], 0
    # spread the samples across the game: early Age I through late Age III
    targets = [3, 8, 14, 20, 26][:count] or [8]
    want = set(targets)
    while not st.game_over and guard < 8000 and want:
        guard += 1
        if st.round in want and st.decider() == seat and st.phase == "actions":
            out.append(copy_state(st))
            want.discard(st.round)
        moves = G.legal_moves(st)
        if not moves:
            break
        G.apply(st, bot.choose(st, moves, rng), rng)
    return out


def report(num_players=3, seat=0, weights=None, positions=None):
    """The pre-flight table: what a human must type, and what they must not."""
    from advisor.advisor import load_bot
    bot = load_bot(num_players) if weights is None else W.WeightedBot(weights)
    if positions is None:
        positions = sample_positions(num_players, seat, weights=bot.weights)
    verdicts = probe_positions(positions, seat, bot.weights)
    reqs = requirements(verdicts)
    return reqs, transcription_cost(reqs, num_players - 1)


def _main(argv=None):
    import argparse
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--players", type=int, default=3)
    ap.add_argument("--seat", type=int, default=0)
    args = ap.parse_args(argv)
    reqs, cost = report(args.players, args.seat)
    print(f"derived field set, {args.players}p, seat {args.seat}\n")
    print(f"{'observable':<28} {'verdict':<7} {'ask':<26} why")
    print("-" * 100)
    for r in reqs:
        mark = "*" if r.mandatory else " "
        print(f"{mark}{r.probe.id:<27} {r.verdict:<7} {r.probe.ask:<26} "
              f"{r.reason}")
    print(f"\n* = the human must transcribe it.  "
          f"{sum(1 for r in reqs if r.mandatory)} of {len(reqs)} observables.")
    print(f"estimated transcription: {cost:.0f} s/round "
          f"(~{cost * 20 / 60:.0f} min over a 20-round game)")


if __name__ == "__main__":
    _main()
