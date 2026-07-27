"""``HumanBot`` -- a BookBot-family policy shaped to a HUMAN behavioural profile.

WHY THIS EXISTS
---------------
Every opponent in the training pool was ``BookBot`` or a ``BookBot`` subclass
driven by hand-written numeric triggers, and ``docs/TWOP_PROFILE.md`` measured
what that costs: the champion does not beat ``var:military`` by out-playing it,
it beats it by holding its ``war_lead >= 5`` trigger shut -- that trigger fires
on 5.5% of turns against the champion and on 41-44% of turns against everyone
else.  A pool of threshold bots teaches a climber to find thresholds.

``docs/HUMAN_BASELINE.md`` then measured that the whole ecosystem -- champion
*and* book *and* all six variants -- plays a game no human plays: ~20-24 cards
taken against a human 34, 1-2 wonders against 2.74, and (for the champion) 22%
of takes at 3 CA against a human 4.5%.

So these bots are built to two rules the variant roster does not follow:

1. **Their statistics are fitted to the corpus, not asserted.**  Each subclass
   carries a ``TARGET`` -- the mean of a real, identified segment of the 1,011
   BGO games -- and ``FIT_KNOBS``, the knobs ``tools/human_fit.py`` was allowed
   to move while minimising the distance to it.  The numbers in ``PROFILE`` are
   the *output* of that fit.  Nobody chose them by taste.

2. **Nothing they do is gated on a cliff.**  Every behavioural decision that
   an opponent could suppress -- aggression, war, the 3-CA reach -- is a
   LOGISTIC probability in the lead, not a threshold on it.  Holding a
   HumanBot one point below some number does not turn it off; it moves it a
   few percent along a smooth curve.  That is the anti-exploit property the
   pool was missing, and ``docs/HUMAN_BOTS.md`` measures whether it holds.

The stochasticity is real (these bots draw from ``self.rng``, which every
variant deliberately does not) and it is the point: a deterministic opponent
has one line to be memorised.  It is still *reproducible* -- the seed comes
from ``experiments/arena.py``'s ``seed * 97 + i * 13 + 1``, which depends only
on the game seed and the seat, so a candidate duel and the paired champion
duel meet the same opponent draw.
"""
from __future__ import annotations

import math

from ... import actions as A
from ... import cards as C
from ..book import V2_NEVER_TAKE, V2_PRICE_LADDER
from ..variants.base import VariantBot, pc

__all__ = ["HumanBot", "HUMAN_DEFAULTS", "logistic"]


def logistic(x):
    if x < -60.0:
        return 0.0
    if x > 60.0:
        return 1.0
    return 1.0 / (1.0 + math.exp(-x))


#: Knobs HumanBot adds on top of ``variants.base.DEFAULT_PROFILE``.  The
#: values here are the NEUTRAL ones -- a HumanBot with an empty PROFILE is
#: BookBot v2 with smooth military gates and no noise.  Each archetype's
#: PROFILE is the fitted diff against this.
HUMAN_DEFAULTS = {
    # --- card throughput -------------------------------------------------
    #: additive bonus on every candidate take.  The single most load-bearing
    #: knob: the whole bot family takes ~20 cards a game where humans take 34,
    #: because ``_best_take``'s convex price ladder leaves most rows scoring
    #: below its accept floor and the bot simply ends its turn with civil
    #: actions unspent.  Raising this buys cards; that is what humans do with
    #: the actions.
    "take_bias": 0.0,
    #: softmax temperature over the take candidates.  0 = argmax (the variant
    #: behaviour).  >0 samples proportional to exp(value / T), so the bot does
    #: not always pick the same card from the same row -- there is no single
    #: deterministic line to learn.
    "take_temp": 0.0,
    #: Gaussian noise (in value units) added to every scored take, develop and
    #: build option.  Models the spread of human judgement; also means an
    #: opponent cannot rely on the exact ordering of two near-equal options.
    "noise": 0.0,
    #: hand size above which the per-card hand penalty applies at all.  Humans
    #: hold cards; BookBot's flat penalty starts biting at one card in hand.
    "hand_free": 0,
    #: probability, per action, that taking a card is tried BEFORE the
    #: build/upgrade/develop rules instead of after them.
    #:
    #: This is the knob that actually moves card throughput, and finding that
    #: out was the main surprise of the fit.  ``take_bias`` saturates: past
    #: about +11 the bot already wants every card in the row and still only
    #: reaches ~27 takes, because ``_r_take_card`` is LAST in BookBot's
    #: priority list and by the time it is reached the civil actions are gone.
    #: The bot family's 20-24 takes against a human 34 is therefore not a
    #: valuation difference at all -- it is a priority-order difference.
    #: Sampling the order per action (rather than flipping it outright) keeps
    #: the bot from degenerating into "take every action", and is one more
    #: place where there is no fixed line to memorise.
    "take_first_p": 0.0,
    #: cap the take on the ROW TIER (1/2/3) rather than on the total civil
    #: action cost.
    #:
    #: `engine/actions.py:take_cost` is ``row_cost(idx) + completed wonders``.
    #: The variant roster's ``max_take_cost`` is compared against that total,
    #: so a bot that finishes three wonders finds *every* slot in the row
    #: priced at 4+ and its take rule silently stops producing candidates --
    #: it then ends its turn with civil actions unspent.  Measured: BookBot
    #: leaves 1.68 civil actions unused on 56% of its turns, ~32 wasted
    #: actions a game, which is the whole of the 20-vs-34 card gap.
    #: `docs/HUMAN_BASELINE.md` names this exact confusion as trap 2 on the
    #: analysis side; this is the same trap on the policy side.  Humans pay
    #: the surcharge and keep buying, and it is still priced in via
    #: V2_PRICE_LADDER below, which reads the true cost.
    "cap_on_tier": True,
    # --- wonders ---------------------------------------------------------
    #: bonus on continuing a wonder already started.  Humans finish 96% of the
    #: wonders they start (2.78 started vs 2.74 completed per 2p player).
    "wonder_finish_bias": 0.0,
    # --- government timing ----------------------------------------------
    #: earliest / latest round this bot will spend a turn on a revolution.
    #: Median human first government is round 12 of 19; the variant roster
    #: hard-codes ``ctx.rnd <= 8``.
    "revolution_round_min": 1,
    "revolution_round_max": 8,
    #: minimum AGE INDEX (A=0, I=1, II=2) of a government this bot will adopt.
    #:
    #: The second surprise of the fit.  BookBot revolts to the first Age I
    #: government it can afford -- Monarchy, 5 civil actions -- around round 9,
    #: and then never revolts again, so it plays the back half of the game on 5
    #: civil actions.  `docs/HUMAN_BASELINE.md`: "humans mostly skip the Age I
    #: governments entirely and go straight to Constitutional Monarchy (35% of
    #: players, the single most common first government) or Republic (22%)",
    #: which are **6 and 7** civil actions, at a median round of 12.  Over the
    #: back eight rounds that is 12-16 extra civil actions, and civil actions
    #: are what cards are bought with: it is most of the 20-vs-34 card gap.
    #: Setting this to 2 makes the bot wait, which is both the human behaviour
    #: and the reason its card throughput can move at all.
    "gov_min_age": 0,
    # --- smooth military gates ------------------------------------------
    #: p(fire) = rate * logistic((lead - centre) / width).  `rate` is the
    #: ceiling at infinite lead, `centre` the lead at which the bot is at half
    #: its ceiling, `width` how gradual the ramp is.  Width > 0 is what makes
    #: this NOT a threshold: there is no lead at which the behaviour switches
    #: off, so an opponent cannot hold it shut.
    "agg_rate": 1.0,
    "agg_centre": 4.0,
    "agg_width": 1.5,
    "war_rate": 1.0,
    "war_centre": 5.0,
    "war_width": 1.5,
    #: multiplier applied to both rates in the last two rounds of the game --
    #: 67% of 2p games contain zero wars, and when a human does declare one it
    #: is almost always a finishing move.
    "endgame_mil_boost": 1.0,
}


class HumanBot(VariantBot):
    """BookBot v2, re-shaped toward a measured human segment.

    Subclasses set :attr:`NAME`, :attr:`PROFILE` (the fitted knobs),
    :attr:`TARGET` (the corpus statistics being matched) and :attr:`FIT_KNOBS`
    (what the fitter was allowed to move).
    """

    NAME = "human"
    PROFILE: dict = {}
    #: corpus axis means for this archetype -- the fit objective.
    TARGET: dict = {}
    #: knob -> candidate values, read by ``tools/human_fit.py``.
    FIT_KNOBS: dict = {}
    FIT_WEIGHTS: dict = {}
    #: how many corpus player-rows this archetype was fitted to.
    N_ROWS = 0

    def __init__(self, rng=None, seed=None, tunables=None, profile=None):
        super().__init__(rng=rng, seed=seed, tunables=tunables, profile=None)
        prof = dict(HUMAN_DEFAULTS)
        prof.update(self.profile)          # DEFAULT_PROFILE + subclass PROFILE
        for k, v in (self.PROFILE or {}).items():
            prof[k] = v
        if profile:
            prof.update(profile)
        self.profile = prof
        self.name = self.NAME

    # ------------------------------------------------------ action phase
    def _action_phase(self, state, p, ctx, moves):
        """The variant's priority list, with the take rule sometimes promoted.

        See ``take_first_p`` in :data:`HUMAN_DEFAULTS` for why this exists.
        """
        by_kind = {}
        for m in moves:
            by_kind.setdefault(m[0], []).append(m)
        rules = list(self.RULES)
        pr = self.k("take_first_p", ctx)
        if pr > 0 and "_r_take_card" in rules and self.rng.random() < pr:
            rules.remove("_r_take_card")
            anchor = "_r_population" if "_r_population" in rules else rules[-1]
            rules.insert(rules.index(anchor), "_r_take_card")
        for rule_name in rules:
            mv = getattr(self, rule_name)(state, p, ctx, by_kind)
            if mv is not None:
                return mv
        return ("end_turn",)

    # ------------------------------------------------------------- noise
    def _jitter(self, ctx):
        s = self.k("noise", ctx)
        return self.rng.gauss(0.0, s) if s else 0.0

    # --------------------------------------------------------- revolution
    def _r_revolution(self, state, p, ctx, by_kind):
        """Same rule as the variant, but with a fitted round WINDOW.

        ``variants.base`` hard-codes ``ctx.rnd <= 8``, which is why every bot
        in the roster revolts around round 8-10 and the corpus median is 12.
        """
        if ctx.late or ctx.age >= 3:
            return None
        lo = self.k("revolution_round_min", ctx)
        hi = self.k("revolution_round_max", ctx)
        if ctx.rnd < lo or ctx.rnd > hi:
            return None
        from ..book import _gov_value
        min_age = self.k("gov_min_age", ctx)
        best, best_v = None, 0.0
        for mv in by_kind.get("revolution", ()):
            if C.level(ctx.db.age_of(mv[1])) < min_age:
                continue
            v = _gov_value(state, p, ctx, mv[1])
            if v > best_v:
                best, best_v = mv, v
        return best if best_v >= self.k("revolution_min", ctx) else None

    # ------------------------------------------------------------ wonders
    def _r_wonder_step(self, state, p, ctx, by_kind):
        """Finish what you start.

        The corpus's most extreme bot/human gap is wonders (8.77 stages per
        human player against 1.91 for the champion), and the second half of
        that finding is that humans essentially never abandon one: 2.78
        started, 2.74 completed.  ``wonder_finish_bias`` re-prices a
        part-built wonder so that a mid-build wonder keeps its worker.
        """
        steps = by_kind.get("wonder_step")
        if not steps:
            return None
        if p.wonder is not None:
            v = self.wonder_value(p.wonder.name, ctx)
            v += self.k("wonder_finish_bias", ctx)
            if v <= 0:
                return None
        return max(steps, key=lambda m: m[1])

    # ---------------------------------------------------------- valuation
    def card_value(self, state, p, ctx, name):
        """The variant valuation, with early governments zeroed by knob.

        ``gov_min_age`` has to bite here as well as in the revolution rule:
        otherwise the bot happily spends a civil action *taking* Monarchy from
        the row and then never plays it.
        """
        if ctx.db.get(name)["type"] == "government" and \
                C.level(ctx.db.age_of(name)) < self.k("gov_min_age", ctx):
            return 0.0
        return super().card_value(state, p, ctx, name)

    # -------------------------------------------------------------- takes
    def _best_take(self, state, p, ctx, takes, first_turn=False):
        """Human card throughput: a softer price ladder, sampled not argmaxed.

        Structurally the variant's rule, with three changes, each of which is
        a measured human/bot gap rather than a preference:

        * ``take_bias`` shifts the accept floor -- the bot family's 20 takes
          against a human 34;
        * ``hand_free`` lets it hold cards, since the flat per-card penalty is
          what stops a bot buying ahead;
        * the choice among acceptable cards is a softmax draw, so two games
          with the same row do not produce the same pick.
        """
        db = ctx.db
        veto = self.k("tech_veto", ctx)
        card_bonus = self.k("card_bonus", ctx)
        ladder_scale = self.k("price_scale", ctx)
        cap = self.age_k("max_take_cost", ctx, 2)
        must = self.k("must_buy_3ca", ctx)
        hand = max(0, len(p.hand_civil) - self.k("hand_free", ctx))
        bias = self.k("take_bias", ctx)
        temp = self.k("take_temp", ctx)
        cands = []
        for mv in takes:
            idx = mv[1]
            name = state.card_row[idx]
            if name is None or name in veto:
                continue
            cost = A.take_cost(state, p, idx)
            gate = A.row_cost(idx) if self.k("cap_on_tier", ctx) else cost
            typ = db.type_of(name)
            if typ == "wonder" and p.wonder is not None:
                continue
            if typ == "wonder" and len(p.completed_wonders) >= \
                    self.k("wonder_max", ctx):
                continue
            if name in V2_NEVER_TAKE:
                continue
            if typ == "leader" and gate >= 3 and ctx.age < 3:
                continue
            if gate > cap and name not in must:
                continue
            if gate >= 3 and ctx.s.civil_actions < \
                    self.k("three_ca_min_actions", ctx) and name not in must:
                continue
            v = self.card_value(state, p, ctx, name)
            v += pc(card_bonus.get(name, 0.0), ctx.nplayers)
            v -= V2_PRICE_LADDER.get(cost, 12.0) * 3.0 * ladder_scale
            v -= hand * self.k("hand_penalty", ctx)
            v += bias
            if first_turn:
                v += 4.0
            v += self._jitter(ctx)
            if v > 0.0:
                cands.append((v, mv))
        if not cands:
            return None
        if temp <= 0:
            return max(cands, key=lambda t: t[0])[1]
        top = max(v for v, _m in cands)
        ws = [math.exp((v - top) / temp) for v, _m in cands]
        tot = sum(ws)
        r = self.rng.random() * tot
        acc = 0.0
        for w, (_v, mv) in zip(ws, cands):
            acc += w
            if acc >= r:
                return mv
        return cands[-1][1]

    # ---------------------------------------------------- politics phase
    def _politics(self, state, p, ctx, moves):
        """Aggression / war / pacts, on LOGISTIC gates instead of thresholds.

        ``docs/TWOP_PROFILE.md``'s exploit is a step function: ``var:military``
        needs a +3 strength lead and fires on 41-44% of turns against most of
        the roster and 5.5% against the champion, because the champion learned
        to sit one point under the step.  Here the same lead produces a
        PROBABILITY, so suppressing the bot's lead reduces its aggression
        smoothly and can never zero it.
        """
        by_kind = {}
        for m in moves:
            by_kind.setdefault(m[0], []).append(m)
        boost = self.k("endgame_mil_boost", ctx) if (ctx.last or ctx.late) \
            else 1.0

        aggs = by_kind.get("aggression")
        if aggs:
            best = max(aggs, key=lambda m: (state.players[m[2]].culture, m[1]))
            lead = self._lead_over(state, p, state.players[best[2]])
            pr = self.k("agg_rate", ctx) * boost * logistic(
                (lead - self.k("agg_centre", ctx))
                / max(0.25, self.k("agg_width", ctx)))
            if self.rng.random() < pr:
                return best

        wars = by_kind.get("war")
        if wars and not ctx.last and ctx.age >= self.k("war_from_age", ctx):
            best = max(wars, key=lambda m: (state.players[m[2]].culture, m[1]))
            lead = self._lead_over(state, p, state.players[best[2]])
            pr = self.k("war_rate", ctx) * boost * logistic(
                (lead - self.k("war_centre", ctx))
                / max(0.25, self.k("war_width", ctx)))
            if self.rng.random() < pr:
                return best

        pacts = by_kind.get("offer_pact")
        if pacts:
            cand = [m for m in pacts
                    if state.players[m[2]].culture <= p.culture]
            if cand:
                return sorted(cand)[0]

        evs = by_kind.get("prepare_event")
        if evs:
            if not self.k("seed_events_when_weakest", ctx) and \
                    self._weakest(state, p):
                return ("pol_pass",)
            return sorted(evs)[0]
        return ("pol_pass",)
