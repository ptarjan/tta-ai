"""Shared machinery for the variant roster.

WHY THIS EXISTS
---------------
``docs/STRENGTH_CHECK.md`` measured that a hand-written rule list (BookBot)
beats our trained champion 62.9% at 2p.  The diagnosis was that self-play had
converged: the champion only ever had to beat a mirror of itself, so it
learned a plan that is only good *relative to that plan*.  The fix is a POOL.
A bot that beats one opponent type has found an exploit; a bot that beats six
structurally different ones has learned something about the game.

Where the roster comes from: ``docs/EXPERT_STRATEGY.md`` ends with a
deliberately unresolved table, "Biggest open disagreements".  Every row of it
is a place where strong humans genuinely split.  Rather than pick a side and
hard-code it, each side becomes a playable strategy:

===========================  ==========================================
disagreement (EXPERT_STRATEGY)  variant that plays that side
===========================  ==========================================
Iron vs 3-Bronze             ``tempo`` (3-Bronze) / ``infra`` (Iron)
Age I culture                ``culture`` (yes) / everyone else (no)
Michelangelo / noob trap     ``wonder`` (plays the trap deliberately)
Napoleon "most important"    ``military`` (yes) / others (situational)
Player count                 every knob may be keyed by player count
===========================  ==========================================

WHAT A VARIANT IS
-----------------
A variant is *not* a new evaluator.  It is :class:`VariantBot` -- BookBot's
priority list, in the v2 (empirical tournament tier list) configuration --
plus two things:

1. a ``PROFILE`` dict of knobs, each entry citable to a line of
   EXPERT_STRATEGY.md, and
2. optionally a different ``RULES`` order, because "what do you do with your
   civil action when several rules fire" *is* the strategy.

Every knob may be a plain value or a dict keyed by player count
(``{2: x, 3: y, 4: z}``), which is how the 2-player skew warning in
EXPERT_STRATEGY.md §"Biggest open disagreements" is respected: the
competitive online lobby is almost exclusively 2p, so a 2p-derived number is
never applied blindly at 3p/4p.

DETERMINISM
-----------
No variant draws from ``self.rng``.  Every choice is an argmax over the
engine's own deterministic ``legal_moves`` order with a strict ``>``
comparison, so the first move wins ties and a given (seed, state) always
produces the same move.  This is required by the seed-paired benchmark in
``experiments/roster_match.py``.
"""
from __future__ import annotations

import random

from ... import actions as A
from ... import cards as C
from ... import economy
from ... import effects
from ..book import (SPECIAL_RANK, TACTIC_RANK, V2_NEVER_TAKE,
                    V2_PRICE_LADDER, BookBot, _action_card_value,
                    _best_level_in, _gov_value, _leader_rank, _wonder_value)

__all__ = ["VariantBot", "DEFAULT_PROFILE", "pc"]


def pc(value, nplayers):
    """Resolve a possibly player-count-keyed knob.

    ``{2: 0.0, 3: 1.0, 4: 1.0}`` -> the entry for this table size.  Anything
    else (including the name-keyed dicts like ``tech_bonus``) is returned
    unchanged, which is why the int-key test has to be exhaustive.
    """
    if isinstance(value, dict) and value and all(isinstance(k, int)
                                                 for k in value):
        if nplayers in value:
            return value[nplayers]
        return value[max(value)]
    return value


#: Every knob, with the neutral value that reproduces BookBot v2 behaviour.
#: A variant overrides only the entries its strategy actually disagrees with,
#: so a diff of a variant's PROFILE against this dict *is* its strategy.
DEFAULT_PROFILE = {
    # --- valuation -----------------------------------------------------
    #: multipliers on the per-turn production value of a card, by resource.
    "prod_weights": {"food": 1.0, "resources": 1.0, "science": 1.0,
                     "culture": 1.0, "happy": 1.0, "strength": 1.0},
    #: name -> additive bonus, applied when developing AND when taking.
    #: This is the variant's cited tech priority list.
    "tech_bonus": {},
    #: names never developed and never taken.  Used for the genuine
    #: "I do not buy this line at all" calls (tempo skipping Iron).
    "tech_veto": frozenset(),
    #: name -> additive bonus applied only when taking from the row.
    "card_bonus": {},
    #: leader/wonder overrides layered on top of the v2 tournament tier list.
    "leader_bonus": {},
    "wonder_bonus": {},
    #: global multiplier on every wonder's value, and a cap on how many
    #: wonders the variant is willing to have finished before it stops
    #: starting new ones ("+1 CA per completed wonder to take a new one").
    "wonder_appetite": 1.0,
    "wonder_max": 99,
    # --- card row discipline -------------------------------------------
    #: multiplier on the convex CA price ladder.  >1 = even more 1-CA-biased
    #: than the tournament data; <1 = happier to reach into the deep slots.
    "price_scale": 1.0,
    #: hard cap on the slot price this variant will pay, by age index
    #: (0=A, 1=I, 2=II, 3=III).  76% of tournament Age I picks are at 1 CA
    #: and only 2.5% at 3 CA, so nothing may habitually pay 3.
    "max_take_cost": {0: 2, 1: 2, 2: 2, 3: 3, 4: 3},
    #: the short universal 3-CA list (BGG 2393942): Air Forces is the only
    #: card called worth 3 CA "in every circumstance".
    "must_buy_3ca": frozenset(("Air Forces",)),
    #: "I tend not to use 3 civil actions until I have 5 or 6 available."
    "three_ca_min_actions": 5,
    #: per-card-in-hand penalty; "don't hoard technology cards".
    "hand_penalty": 1.0,
    # --- military -------------------------------------------------------
    #: "floor"  = never be last (BookBot's rule)
    #: "top2"   = match the strongest rival ("top 2 military position")
    #: "aggro"  = beat the strongest rival by ``mil_margin``
    "mil_stance": "floor",
    "mil_margin": 0,
    #: extra strength lead demanded before firing an aggression, by age.
    #: "it takes a strength lead of 5 to guarantee a successful Age I
    #: aggression... aggressive players may attempt with only 3 or 4."
    "agg_lead": {0: 4, 1: 4, 2: 3, 3: 3, 4: 3},
    #: lead demanded before declaring war, and the earliest age to do it.
    "war_lead": 5,
    "war_from_age": 1,
    #: "certainly don't play [events] when you are the weakest player."
    "seed_events_when_weakest": True,
    #: how many military units this variant is willing to staff, by age.
    #: "Age I: 3-4 units + Knights or Swordsmen + a matching tactic (~10
    #: strength); Age II: 4-6 units, Age II tactic, Cannon (15-25)."
    #: Every unit also costs a yellow cube, so an uncapped army is the
    #: "Over-Investment in Military" mistake, hcy1's #2 beginner error.
    "unit_cap": {0: 2, 1: 4, 2: 6, 3: 8, 4: 8},
    # --- economy --------------------------------------------------------
    #: card TYPE -> additive bonus when placing a worker.
    "build_bonus": {},
    #: multiplier on how eager the variant is to grow.
    "pop_appetite": 1.0,
    #: minimum ``_gov_value`` before burning a whole turn on a revolution.
    "revolution_min": 10.0,
    #: upgrade types this variant refuses to spend a civil action upgrading
    #: in place (the Iron-vs-Bronze disagreement lives here).
    "upgrade_veto": frozenset(),
}


class VariantBot(BookBot):
    """BookBot v2 with a strategy profile bolted on.

    Subclasses set :attr:`NAME`, :attr:`PROFILE` and optionally :attr:`RULES`.
    Everything else -- the pending-decision handling, the auction and defence
    logic, the card database access -- is inherited unchanged from
    ``engine/bots/book.py`` so the roster shares one tested implementation.
    """

    #: identifier used by the benchmark harness and the round-robin table.
    NAME = "variant"
    #: knob overrides; merged over DEFAULT_PROFILE at construction.
    PROFILE: dict = {}
    #: the priority list, top to bottom.  Overriding this reorders the
    #: strategy itself, which is the point of the roster.
    RULES = ("_r_round1", "_r_revolution", "_r_play_leader", "_r_happiness",
             "_r_military_floor", "_r_wonder_step", "_r_population",
             "_r_place_worker", "_r_upgrade", "_r_develop", "_r_action_card",
             "_r_take_card")

    def __init__(self, rng=None, seed=None, tunables=None, profile=None):
        super().__init__(rng=rng, seed=seed, version=2, tunables=tunables)
        prof = dict(DEFAULT_PROFILE)
        prof.update(self.PROFILE)
        if profile:
            prof.update(profile)
        self.profile = prof
        self.name = self.NAME

    # ------------------------------------------------------------ knobs
    def k(self, key, ctx):
        """Read a knob, resolving any player-count keying."""
        return pc(self.profile[key], ctx.nplayers)

    def age_k(self, key, ctx, default):
        """Read an AGE-keyed knob, then resolve player-count keying inside.

        Age-keyed and player-count-keyed knobs are both int-keyed dicts, so
        they cannot be told apart by inspection -- ``{0: 2, 1: 2, 2: 3}`` is
        ambiguous.  The two are therefore separated by *accessor*: knobs read
        through here are always age-keyed at the top level, and may carry a
        player-count dict as their value.
        """
        return pc(self.profile[key].get(ctx.age, default), ctx.nplayers)

    # -------------------------------------------------------- valuation
    def prod_value(self, prod, ctx):
        """Book's phase-dependent production value, scaled by the profile.

        The shape (food/resources matter early, culture matters late) is
        BookBot's and is left alone; a variant only rescales the axes it
        actually disagrees about.
        """
        raw = self.k("prod_weights", ctx)
        n = ctx.nplayers

        def w(key):
            # an individual axis may itself be player-count keyed, e.g.
            # CultureBot damps early culture as the table grows
            return pc(raw.get(key, 1.0), n)

        early = ctx.age <= 1
        v = 0.0
        v += prod.get("food", 0) * (3.0 if early else 1.5) * w("food")
        v += prod.get("resources", 0) * (3.0 if early else 2.0) * w("resources")
        v += prod.get("science", 0) * (3.5 if not ctx.late else 1.0) * w("science")
        v += prod.get("culture", 0) * (2.0 if early else 4.5) * w("culture")
        v += prod.get("happy", 0) * ((2.5 if ctx.happy_gap >= 0 else 1.0)
                                     * w("happy"))
        v += prod.get("strength", 0) * 1.5 * w("strength")
        return v

    def wonder_value(self, name, ctx):
        v = _wonder_value(name, ctx)
        v *= self.k("wonder_appetite", ctx)
        v *= pc(self.k("wonder_bonus", ctx).get(name, 1.0), ctx.nplayers)
        return v

    def card_value(self, state, p, ctx, name):
        """Profile-aware rewrite of ``book._card_value``.

        Same structure, but every branch can be moved by the profile, which
        is what makes two variants disagree about the same card row.
        """
        db = ctx.db
        card = db.get(name)
        typ = card["type"]
        bonus = pc(self.k("tech_bonus", ctx).get(name, 0.0), ctx.nplayers)

        if typ == "leader":
            base = _leader_rank(name, ctx, p) * 1.6
            return base + pc(self.k("leader_bonus", ctx).get(name, 0.0),
                             ctx.nplayers)

        if typ == "wonder":
            return self.wonder_value(name, ctx)

        if typ == "government":
            return _gov_value(state, p, ctx, name) + bonus

        if typ == "special-tech":
            base = SPECIAL_RANK.get(name, 4) * 1.5
            if name == "Masonry" and p.wonder is None and not p.completed_wonders:
                base *= 0.6
            return base + bonus

        if typ == "action":
            return _action_card_value(state, p, ctx, name) + bonus

        if typ in C.WORKER_TYPES:
            prod = card.get("production") or {}
            v = self.prod_value(prod, ctx)
            if name == "Theology":
                # selected exactly 0 times in 39 tournament games
                v *= ctx.tun["theology"]
            lvl = db.level_of(name)
            best = _best_level_in(p, typ, db)
            if lvl <= best:
                return 0.0                     # never a downgrade
            if typ in C.UNIT_TYPES:
                v = (card.get("strength") or 0) * 2.0
                if ctx.strength >= self.mil_goal(state, p, ctx):
                    v *= 0.5                   # already safe: units are a tax
            if ctx.late and typ not in ("theater", "temple", "library"):
                v *= 0.5
            return v + bonus

        return 1.0

    # --------------------------------------------------------- military
    def mil_goal(self, state, p, ctx):
        """The strength number this variant is trying to reach.

        ``floor`` reproduces BookBot ("never be the weakest").  ``top2`` and
        ``aggro`` encode "Top 2 military position guarantees most military
        events benefit you" and the aggression lead thresholds from
        EXPERT_STRATEGY.md §5.
        """
        stance = self.k("mil_stance", ctx)
        margin = self.k("mil_margin", ctx)
        if stance == "floor":
            return ctx.mil_target + margin
        rivals = [q for q in state.players
                  if q.idx != p.idx and not q.resigned]
        if not rivals:
            return margin
        top = max(effects.state_stats(state, q).strength for q in rivals)
        if stance == "top2":
            return top + margin
        return top + margin                    # "aggro": same target, bigger margin

    # ----------------------------------------------------- action phase
    def _action_phase(self, state, p, ctx, moves):
        by_kind = {}
        for m in moves:
            by_kind.setdefault(m[0], []).append(m)
        for rule_name in self.RULES:
            mv = getattr(self, rule_name)(state, p, ctx, by_kind)
            if mv is not None:
                return mv
        return ("end_turn",)

    # rule: revolution ---------------------------------------------------
    def _r_revolution(self, state, p, ctx, by_kind):
        if ctx.late or ctx.age >= 2:
            return None
        best, best_v = None, 0.0
        for mv in by_kind.get("revolution", ()):
            v = _gov_value(state, p, ctx, mv[1])
            if v > best_v:
                best, best_v = mv, v
        floor = self.k("revolution_min", ctx)
        return best if best_v >= floor and ctx.rnd <= 8 else None

    # rule: leader -------------------------------------------------------
    def _r_play_leader(self, state, p, ctx, by_kind):
        """Play a leader immediately -- "each round without a leader is
        significant negative utility" (39-game tournament data).

        Unlike BookBot this ranks with the v2 tournament table plus the
        variant's own leader_bonus, so e.g. the military variant really does
        prefer Napoleon and the wonder variant really does play Michelangelo.
        """
        cands = by_kind.get("play_leader")
        if not cands:
            return None

        def rank(nm):
            return (_leader_rank(nm, ctx, p)
                    + pc(self.k("leader_bonus", ctx).get(nm, 0.0),
                         ctx.nplayers))

        best = max(cands, key=lambda m: (rank(m[1]), m[1]))
        if p.leader is None:
            return best
        # replacing a leader costs a civil action you do not get back
        if rank(best[1]) >= rank(p.leader) + 2:
            return best
        return None

    # rule: military floor ----------------------------------------------
    def _r_military_floor(self, state, p, ctx, by_kind):
        """Never be the weakest; some variants aim higher (see mil_stance).

        "You don't necessarily win with good military, but you will almost
        certainly lose if you ignore it."
        """
        db = ctx.db
        if p.tactic is None:
            best = None
            for mv in by_kind.get("play_tactic", ()):
                if best is None or TACTIC_RANK.get(mv[1], 0) > \
                        TACTIC_RANK.get(best[1], 0):
                    best = mv
            if best is not None:
                return best
        if ctx.strength >= self.mil_goal(state, p, ctx):
            return None
        best, best_v = None, 0.0
        for mv in by_kind.get("upgrade", ()):
            if db.type_of(mv[1]) not in C.UNIT_TYPES:
                continue
            gain = ((db.get(mv[2]).get("strength") or 0)
                    - (db.get(mv[1]).get("strength") or 0))
            if gain > best_v:
                best, best_v = mv, gain
        if best is not None:
            return best
        if p.workers_free > 0 and self.unit_workers(p) < \
                self.age_k("unit_cap", ctx, 8):
            best, best_v = None, 0.0
            for mv in by_kind.get("build", ()):
                if db.type_of(mv[1]) not in C.UNIT_TYPES:
                    continue
                v = db.get(mv[1]).get("strength") or 0
                if v > best_v:
                    best, best_v = mv, v
            if best is not None:
                return best
        return None

    @staticmethod
    def unit_workers(p):
        """Workers currently standing on military units."""
        db = C.db()
        return sum(t.workers for n, t in p.techs.items()
                   if db.type_of(n) in C.UNIT_TYPES)

    # rule: wonders ------------------------------------------------------
    def _r_wonder_step(self, state, p, ctx, by_kind):
        steps = by_kind.get("wonder_step")
        if not steps:
            return None
        if p.wonder is not None and self.wonder_value(p.wonder.name, ctx) <= 0:
            return None
        return max(steps, key=lambda m: m[1])

    # rule: population ---------------------------------------------------
    def _r_population(self, state, p, ctx, by_kind):
        if by_kind.get("pop_free"):
            return by_kind["pop_free"][0]
        if not by_kind.get("pop"):
            return None
        appetite = self.k("pop_appetite", ctx)
        if appetite <= 0:
            return None
        idle_cap = 1 if appetite >= 1.5 else 2
        if ctx.workers_free >= 1 and ctx.age <= 1 and appetite < 1.5:
            return None
        if ctx.workers_free >= idle_cap:
            return None
        if economy.happy_required(max(0, p.yellow_bank - 1)) > ctx.s.happy:
            return None
        if ctx.late:
            return None
        return by_kind["pop"][0]

    # rule: upgrade in place --------------------------------------------
    def _r_upgrade(self, state, p, ctx, by_kind):
        db = ctx.db
        veto = self.k("upgrade_veto", ctx)
        best, best_v = None, 0.0
        for mv in by_kind.get("upgrade", ()):
            lo, hi = mv[1], mv[2]
            typ = db.type_of(hi)
            if typ in C.UNIT_TYPES:
                continue                       # the military rule owns these
            if typ in veto or hi in veto:
                continue
            gain = (self.prod_value(db.get(hi).get("production") or {}, ctx)
                    - self.prod_value(db.get(lo).get("production") or {}, ctx))
            cost = max(0, (db.get(hi).get("buildCost") or 0)
                       - (db.get(lo).get("buildCost") or 0))
            v = gain - cost * 0.4
            if typ == "farm" and ctx.food_need <= 0:
                v -= 2.0
            if v > best_v:
                best, best_v = mv, v
        return best if best_v >= 1.5 else None

    # ------------------------------------------------------------ helpers
    def _best_build(self, state, p, ctx, by_kind, happy_only=False):
        db = ctx.db
        bonus_by_type = self.k("build_bonus", ctx)
        best, best_v = None, 0.0
        for mv in by_kind.get("build", ()):
            name = mv[1]
            typ = db.type_of(name)
            if typ in C.UNIT_TYPES:
                continue
            card = db.get(name)
            prod = card.get("production") or {}
            if happy_only and not prod.get("happy"):
                continue
            v = self.prod_value(prod, ctx)
            if typ == "farm":
                v += 3.0 * min(2, ctx.food_need)
            elif typ == "mine":
                v += 3.0 * min(2, ctx.res_need)
            if prod.get("happy") and ctx.happy_gap > 0:
                v += 5.0 * min(2, ctx.happy_gap)
            v += pc(bonus_by_type.get(typ, 0.0), ctx.nplayers)
            v -= (card.get("buildCost") or 0) * 0.5
            if ctx.late and not prod.get("culture"):
                v -= 3.0
            if v > best_v:
                best, best_v = mv, v
        return best if best_v > 0.5 else None

    def _best_develop(self, state, p, ctx, by_kind, happy_only=False):
        db = ctx.db
        veto = self.k("tech_veto", ctx)
        best, best_v = None, 0.0
        for mv in by_kind.get("develop", ()):
            name = mv[1]
            if name in veto:
                continue
            card = db.get(name)
            if happy_only and not (card.get("production") or {}).get("happy"):
                continue
            v = self.card_value(state, p, ctx, name)
            cost = effects.tech_cost(state, p, name) or 0
            v -= cost * 0.6
            if card["type"] in C.WORKER_TYPES and p.workers_free <= 0 \
                    and (card.get("buildCost") or 0) > p.resources:
                v -= 2.0
            if v > best_v:
                best, best_v = mv, v
        return best if best_v >= 1.0 else None

    def _best_take(self, state, p, ctx, takes, first_turn=False):
        """Take cards you will actually play, from the cheap end of the row.

        The CA-price discipline here is the single most important shared
        rule in the roster: across 39 tournament games 76% of Age I picks
        were made at 1 CA and only 2.5% at 3 CA, so the price of depth is
        convex, and a bot that reaches for 2-3 CA cards habitually is not
        playing the game strong humans play.
        """
        db = ctx.db
        veto = self.k("tech_veto", ctx)
        card_bonus = self.k("card_bonus", ctx)
        ladder_scale = self.k("price_scale", ctx)
        cap = self.age_k("max_take_cost", ctx, 2)
        must = self.k("must_buy_3ca", ctx)
        hand = len(p.hand_civil)
        best, best_v = None, 0.0
        for mv in takes:
            idx = mv[1]
            name = state.card_row[idx]
            if name is None or name in veto:
                continue
            cost = A.take_cost(state, p, idx)
            typ = db.type_of(name)
            if typ == "wonder" and p.wonder is not None:
                continue
            if typ == "wonder" and len(p.completed_wonders) >= \
                    self.k("wonder_max", ctx):
                continue
            if name in V2_NEVER_TAKE:
                continue
            if typ == "leader" and cost >= 3 and ctx.age < 3:
                continue                       # "no leader warrants 3 white"
            if name in ("Taj Mahal", "Great Wall") and cost > 1 \
                    and name not in card_bonus:
                continue
            if cost > cap and name not in must:
                continue
            if cost >= 3 and ctx.s.civil_actions < \
                    self.k("three_ca_min_actions", ctx) and name not in must:
                continue
            v = self.card_value(state, p, ctx, name)
            v += pc(card_bonus.get(name, 0.0), ctx.nplayers)
            v -= V2_PRICE_LADDER.get(cost, 12.0) * 3.0 * ladder_scale
            v -= hand * self.k("hand_penalty", ctx)
            if first_turn:
                v += 4.0
            if v > best_v:
                best, best_v = mv, v
        return best if best_v > 0.0 else None

    # ---------------------------------------------------- politics phase
    def _politics(self, state, p, ctx, moves):
        """Aggressions, wars, pacts and event seeding.

        The engine only generates an aggression whose target is already
        beaten, but a bare 1-point lead still loses to defence cards, so the
        variant demands ``agg_lead`` on top -- "it takes a strength lead of 5
        to guarantee a successful Age I aggression".
        """
        by_kind = {}
        for m in moves:
            by_kind.setdefault(m[0], []).append(m)

        lead_needed = self.age_k("agg_lead", ctx, 3)
        aggs = by_kind.get("aggression")
        if aggs:
            ok = [m for m in aggs
                  if self._lead_over(state, p, state.players[m[2]])
                  >= lead_needed]
            if ok:
                return max(ok, key=lambda m: (state.players[m[2]].culture,
                                              m[1]))

        wars = by_kind.get("war")
        if wars and not ctx.last and ctx.age >= self.k("war_from_age", ctx):
            need = self.k("war_lead", ctx)
            ok = [m for m in wars
                  if self._lead_over(state, p, state.players[m[2]]) >= need]
            if ok:
                return max(ok, key=lambda m: (state.players[m[2]].culture,
                                              m[1]))

        pacts = by_kind.get("offer_pact")
        if pacts:
            cand = [m for m in pacts
                    if state.players[m[2]].culture <= p.culture]
            if cand:
                return sorted(cand)[0]

        evs = by_kind.get("prepare_event")
        if evs:
            if not self.k("seed_events_when_weakest", ctx) and self._weakest(
                    state, p):
                return ("pol_pass",)
            return sorted(evs)[0]
        return ("pol_pass",)

    def _lead_over(self, state, p, q):
        return (effects.attack_strength(state, p, q)
                - effects.state_stats(state, q).strength)

    def _weakest(self, state, p):
        mine = effects.state_stats(state, p).strength
        for q in state.players:
            if q.idx == p.idx or q.resigned:
                continue
            if effects.state_stats(state, q).strength < mine:
                return False
        return True
