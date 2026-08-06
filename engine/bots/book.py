"""BookBot -- Through the Ages played "from the book".

Everything else in this repo is self-play.  Self-play only ever proves a bot
is good *relative to itself*: a population that has agreed on a bad plan will
happily report 50% win rates forever.  BookBot exists to be an EXTERNAL
yardstick.  It is a hand-written, rule-based policy that follows the strategy
human players actually recommend, so beating it (or losing to it) means
something about absolute strength.

It is deliberately NOT another weighted evaluator and does no lookahead.  It
is an ordered priority list -- the kind of thing a human can hold in their
head at the table -- evaluated top to bottom until one rule fires.

THE PRIORITY LIST (action phase)
--------------------------------
  0. Round 1: take a leader, then a wonder, then tempo cards.
  1. Revolution to a better government (must be the turn's first action).
  2. Play a leader from hand -- a leader unplayed is a leader wasted.
  3. Emergency happiness: never let discontent cost you a worker or an uprising.
  4. Military floor: never be the weakest civilisation at the table.
  5. Continue an in-progress wonder while the resources are there.
  6. Grow the population when there is food and a job for the worker.
  7. Put idle workers to work: farm/mine deficits first, then buildings.
  8. Upgrade in place (one action, no new worker) when it beats a new build.
  9. Spend science on the tech priority list.
 10. Play action cards whose free action is one you wanted anyway.
 11. Take cards from the row -- cheap slots, cards you will actually play.
 12. Winston Churchill's once-per-turn choice, if he is your leader and it
     is unused: it draws down no civil or military action, so taking it is
     strictly better than not (the choice is lost for the turn either way).
     WHICH of the two flavours to take is a genuine strategy question --
     culture is unconditional, the military discount is wasted if nothing
     military gets bought before end of turn -- so this always takes the
     risk-free flavour (culture) rather than guessing at a same-turn
     military purchase; a smarter choice is an open question, not answered
     here.
 13. End the turn.

SOURCES
-------
The rules below are the repeated consensus of published TtA strategy advice.
The ones with a bracketed tag are cited in docs/STRENGTH_CHECK.md, which lists
the URLs and local source files each came from.  Card data (costs, stages,
production) is read from the engine card DB, not hard-coded, so the rules stay
true if the DB is corrected.
"""
from __future__ import annotations

import random

from .. import actions as A
from .. import cards as C
from .. import economy
from .. import effects

__all__ = ["BookBot", "BookImprovedBot"]

AGE_IDX = {"A": 0, "I": 1, "II": 2, "III": 3, "IV": 4}


# --------------------------------------------------------------- card ranks
#
# Leaders.  The consensus ordering values, in rough order: permanent extra
# actions > a compounding science/culture engine > a one-off bonus.
LEADER_RANK = {
    # Age A
    "Hammurabi": 9,           # a military action as a civil action, every turn
    "Julius Caesar": 8,       # +1 strength, +1 MA: the military tempo leader
    "Aristotle": 8,           # science on every tech card taken: compounds
    "Moses": 7,               # cheaper population = a bigger civilisation
    "Homer": 6,               # +1 happy plugs the Age A happiness hole
    "Alexander the Great": 5,  # strong only if you commit to many units
    # Age I
    "Michelangelo": 9,        # culture per happy face, and free wonder taking
    "Leonardo da Vinci": 8,   # science per best lab level: the science engine
    "Joan of Arc": 7,         # culture + MA + strength off temples
    "Frederick Barbarossa": 7,  # combined pop+unit action, discounts
    "Genghis Khan": 6,        # needs a military lead to pay off
    "Christopher Columbus": 5,  # colony-dependent
    # Age II
    "Isaac Newton": 9,        # science per lab level AND an action back
    "J. S. Bach": 8,          # culture per theater, cheap theater tech
    "William Shakespeare": 7,
    "Napoleon Bonaparte": 7,  # +2 MA, big strength: the war leader
    "Maximilien Robespierre": 6,
    "James Cook": 5,
}

# Wonders.  Score is "how much the finished wonder is worth", traded off
# against total resource cost by _wonder_value().  Permanent extra actions and
# permanent culture rate rank above one-shot effects.
WONDER_RANK = {
    "Pyramids": 9,              # +1 civil action forever, 6 resources
    "Library of Alexandria": 8,  # +1 culture +1 science + hand limits
    "Hanging Gardens": 7,       # +1 culture +2 happy
    "Colossus": 5,              # military only
    "St. Peter's Basilica": 8,  # +2 culture +1 happy, extra happy per source
    "Taj Mahal": 7,             # +3 culture for 8 resources
    "Universitas Carolina": 6,  # +1 culture +2 science
    "Great Wall": 6,            # strong defence, weak economy
    "Eiffel Tower": 8,          # +4 culture
    "Kremlin": 7,               # +1 CA +1 MA +2 culture, -1 happy
    "Transcontinental Railroad": 6,
    "Ocean Liners": 5,
    "First Space Flight": 8,
    "Internet": 7,
    "Hollywood": 7,
    "Fast Food Chains": 6,
}

# Special technologies, by the size of the permanent bonus.
SPECIAL_RANK = {
    "Code of Laws": 9,      # +1 civil action
    "Masonry": 8,           # build discount AND two wonder stages per action
    "Warfare": 7,           # +1 MA +1 strength
    "Cartography": 5,
}

# Tactics we would rather hold than not (all of them, in strength order).
TACTIC_RANK = {
    "Medieval Army": 6, "Heavy Cavalry": 5, "Legion": 4, "Phalanx": 3,
    "Fighting Band": 2,
}


def _db():
    return C.db()


# ------------------------------------------------------------------ context

class _Ctx:
    """The dozen numbers every rule below reads.  Computed once per decision."""

    # `ca`, `ma`, `rival_best_culture` and `age_row` used to live here too:
    # computed every decision, read by zero rules (grepped the whole repo,
    # not just this file -- nothing in `plan.py`/`weighted.py`/`neural_*.py`/
    # `tools/` touched them either).  Deleted rather than carried as dead
    # weight; see `rust/src/bots/book.rs`'s matching deletion.
    __slots__ = ("p", "s", "age", "rnd", "last", "food_need",
                 "res_need", "happy_gap", "mil_target", "strength",
                 "workers_free", "late", "db",
                 "version", "tun", "nplayers")

    def __init__(self, state, p, version=1, tun=None):
        db = self.db = _db()
        self.version = version
        self.tun = tun or V2_TUNABLES
        self.nplayers = len(state.players)
        self.p = p
        s = self.s = effects.state_stats(state, p)
        self.age = AGE_IDX.get(state.age_civil, 0)
        self.rnd = state.round
        self.last = bool(getattr(state, "last_round", False))
        self.workers_free = p.workers_free

        # Food: you must out-produce consumption or the civilisation stalls.
        # The book target is a couple of food a turn spare so population can
        # keep growing; growth is what everything else is built on.
        self.food_need = max(0, economy.consumption(p.yellow_bank) + 2 - s.food)
        # Resources: buildings, units and wonder stages all come out of here.
        # Corruption punishes a big pile, so the target is a healthy rate.
        self.res_need = max(0, (3 + self.age) - s.resources)

        # Happiness: discontent costs a worker, worse a full uprising.
        self.happy_gap = economy.happy_required(p.yellow_bank) - s.happy
        self.strength = s.strength
        rivals = [q for q in state.players if q.idx != p.idx and not q.resigned]
        rs = sorted(effects.state_stats(state, q).strength for q in rivals)
        if not rs:
            self.mil_target = 0
        elif len(rs) == 1:
            # Heads-up: there is nobody else to be attacked instead of you.
            self.mil_target = rs[-1]
        else:
            # Multiplayer: the book rule is "don't be the softest target".
            # Match the second-strongest, and stay within 3 of the strongest.
            self.mil_target = max(rs[-2], rs[-1] - 3)
        # "Late" = stop investing in economy that will not pay back.
        self.late = self.last or self.age >= 3


# ------------------------------------------------------------- value tables

def _prod_value(prod, ctx):
    """Book value of a per-turn production block, by game phase.

    Early, food and resources are worth more than culture because they buy
    the engine that makes culture later; in Age III the reverse is true --
    culture you cannot spend is the only thing that scores.
    """
    early = ctx.age <= 1
    v = 0.0
    v += prod.get("food", 0) * (3.0 if early else 1.5)
    v += prod.get("resources", 0) * (3.0 if early else 2.0)
    v += prod.get("science", 0) * (3.5 if not ctx.late else 1.0)
    v += prod.get("culture", 0) * (2.0 if early else 4.5)
    v += prod.get("happy", 0) * (2.5 if ctx.happy_gap >= 0 else 1.0)
    v += prod.get("strength", 0) * 1.5
    return v


def _leader_rank(name, ctx, p=None):
    """Book rank of a leader.

    v1 uses the opinion-derived table; v2 uses the empirical tournament
    CA-spend ordering, with the two conditional leaders the sources insist on
    (Leonardo needs a lab/library, Columbus needs a colony) and the one value
    the sources split by player count (Homer).
    """
    if ctx.version < 2:
        return LEADER_RANK.get(name, 5)
    if name == "Homer":
        return V2_HOMER.get(ctx.nplayers, 6.0)
    rank = V2_LEADER_RANK.get(name, 5.0)
    if p is not None:
        if name == "Leonardo da Vinci":
            # "Don't take him without either Alchemy or Printing Press."
            have = any(n in p.techs or n in p.hand_civil
                       for n in ("Alchemy", "Printing Press"))
            rank = 8.5 if have else 2.0
        elif name == "Christopher Columbus":
            # "Only take with a colony already in hand.  Do not take
            # speculatively."
            has_colony = any(ctx.db.type_of(n) == "territory"
                             for n in p.hand_military)
            rank = 8.0 if has_colony else 1.5
    return rank


def _wonder_value(name, ctx):
    """Rank, discounted by how many resources the wonder still costs."""
    card = ctx.db.get(name)
    stages = card.get("stages") or []
    total = sum(stages)
    if ctx.version >= 2:
        rank = V2_WONDER_RANK.get(name, 5.0)
        t = ctx.tun
        if name == "Pyramids":
            rank *= t["pyramids_vs_loa"]
        elif name == "Great Wall":
            rank *= t["great_wall"]
        elif name == "Taj Mahal":
            rank *= t["taj_mahal"]
        elif name == "Hanging Gardens":
            rank *= t["hanging_gardens"]
    else:
        rank = WONDER_RANK.get(name, 5)
    # Age III wonders are one-shot culture bombs; they are only worth starting
    # if there is time left to finish them.
    if ctx.last and len(stages) > 1:
        return 0.0
    return rank * 2.0 - total * 0.5


def _card_value(state, p, ctx, name):
    """How much the book wants this card, used for taking and developing."""
    db = ctx.db
    card = db.get(name)
    typ = card["type"]

    if typ == "leader":
        # Only one leader per age can be taken, and an unplayed leader is
        # dead weight; a leader already in hand or in play blocks this.
        return _leader_rank(name, ctx, p) * 1.6

    if typ == "wonder":
        return _wonder_value(name, ctx)

    if typ == "government":
        return _gov_value(state, p, ctx, name)

    if typ == "special-tech":
        base = SPECIAL_RANK.get(name, 4) * 1.5
        if name == "Masonry" and p.wonder is None and not p.completed_wonders:
            base *= 0.6                       # no wonder to discount
        return base

    if typ == "action":
        return _action_card_value(state, p, ctx, name)

    if typ in C.WORKER_TYPES or typ == "special-tech":
        prod = card.get("production") or {}
        v = _prod_value(prod, ctx)
        if ctx.version >= 2 and name == "Theology":
            # Selected exactly 0 times in 39 tournament games: happiness cards
            # and Bread and Circuses cover the same problem more cheaply.
            v *= ctx.tun["theology"]
        # A technology is only worth what the buildings on it produce, so a
        # tech we cannot afford to staff is worth much less.
        lvl = db.level_of(name)
        best = _best_level_in(p, typ, db)
        if lvl <= best:
            return 0.0                        # never a downgrade
        if typ in C.UNIT_TYPES:
            v = (card.get("strength") or 0) * 2.0
            if ctx.strength >= ctx.mil_target:
                v *= 0.5                      # already safe: units are a tax
        # Late in the game a new tech will not pay for its own build cost.
        if ctx.late and typ not in ("theater", "temple", "library"):
            v *= 0.5
        return v

    return 1.0


def _best_level_in(p, typ, db):
    return max((db.level_of(n) for n in p.techs if db.type_of(n) == typ),
               default=-1)


def _gov_value(state, p, ctx, name):
    """Governments are bought for actions, not for their culture."""
    db = ctx.db
    card = db.get(name)
    cur = db.get(p.government)
    gain_ca = (card.get("civilActions") or 0) - (cur.get("civilActions") or 0)
    gain_ma = (card.get("militaryActions") or 0) - (cur.get("militaryActions") or 0)
    if gain_ca <= 0 and gain_ma <= 0:
        return 0.0
    # An extra civil action every remaining turn is the single most valuable
    # thing in the game early, and worth almost nothing on the last turn.
    horizon = 1.0 if ctx.late else 3.0
    return (gain_ca * 4.0 + gain_ma * 1.5) * horizon


def _action_card_value(state, p, ctx, name):
    """Action cards are tempo: worth taking only if the free action is one
    you were going to spend a civil action on anyway."""
    eff = ctx.db.get(name).get("effects") or {}
    v = 0.0
    v += (eff.get("gainCulture") or 0) * (1.0 if not ctx.late else 2.0)
    v += (eff.get("gainScience") or 0) * 1.5
    v += (eff.get("gainFood") or 0) * 1.0
    v += (eff.get("gainResources") or 0) * 1.2
    free = eff.get("freeCivilAction")
    if free == "build_one_wonder_stage":
        v += 6.0 if p.wonder is not None else 1.0
    elif free == "increase_population":
        v += 4.0
    elif free == "build_or_upgrade_farm_or_mine":
        v += 5.0
    elif free == "build_or_upgrade_urban_building":
        v += 5.0
    if eff.get("militaryActions"):
        v += 2.0
    return v


# ---------------------------------------------------------------- the bot

class BookBot:
    """Rule-based "received wisdom" opponent.  Deterministic given its rng."""

    name = "book"
    ALLOW_RESIGN = False

    def __init__(self, rng=None, seed=None, version=1, tunables=None):
        self.rng = rng or random.Random(seed)
        #: 1 = the opinion-derived rules; 2 = the empirical tournament tier
        #: list (see V2_LEADER_RANK / V2_WONDER_RANK / V2_PRICE_LADDER).
        self.version = version
        self.tunables = dict(V2_TUNABLES, **(tunables or {}))
        if version >= 2:
            self.name = "book2"

    def __call__(self, state):
        return self.choose(state, A.legal_moves(state))

    # -------------------------------------------------------- entry point
    def choose(self, state, moves, rng=None):
        if not moves:
            return None
        moves = [m for m in moves if m[0] != "resign"] or moves
        if len(moves) == 1:
            return moves[0]
        if state.pending:
            return self._pending(state, moves)
        p = state.actor()
        ctx = _Ctx(state, p, self.version, self.tunables)
        if state.phase == "politics":
            return self._politics(state, p, ctx, moves)
        return self._action_phase(state, p, ctx, moves)

    # ------------------------------------------------------- action phase
    def _action_phase(self, state, p, ctx, moves):
        by_kind = {}
        for m in moves:
            by_kind.setdefault(m[0], []).append(m)

        for rule in (self._r_round1,
                     self._r_revolution,
                     self._r_play_leader,
                     self._r_happiness,
                     self._r_military_floor,
                     self._r_wonder_step,
                     self._r_population,
                     self._r_place_worker,
                     self._r_upgrade,
                     self._r_develop,
                     self._r_action_card,
                     self._r_take_card,
                     self._r_churchill):
            mv = rule(state, p, ctx, by_kind)
            if mv is not None:
                return mv
        return ("end_turn",)

    # rule 0 -------------------------------------------------------------
    def _r_round1(self, state, p, ctx, by_kind):
        """Round 1: taking cards is the only legal action.

        The book opening is a leader first (the whole game is played with it),
        then a wonder worth finishing, then tempo cards -- and never a card
        from the expensive end of the row on turn one, when actions are the
        scarcest they will ever be.
        """
        if state.round != 1:
            return None
        takes = by_kind.get("take")
        if not takes:
            return None
        return self._best_take(state, p, ctx, takes, first_turn=True)

    # rule 1 -------------------------------------------------------------
    def _r_revolution(self, state, p, ctx, by_kind):
        """Revolution costs the whole turn's civil actions, so it only pays
        while there are many turns left to spend the extra action on."""
        if ctx.late or ctx.age >= 2:
            return None
        best, best_v = None, 0.0
        for mv in by_kind.get("revolution", ()):
            v = _gov_value(state, p, ctx, mv[1])
            if v > best_v:
                best, best_v = mv, v
        # only worth burning a turn of actions for a real gain, and only
        # early enough that the gain compounds
        return best if best_v >= 10.0 and ctx.rnd <= 8 else None

    # rule 2 -------------------------------------------------------------
    def _r_play_leader(self, state, p, ctx, by_kind):
        """Play a leader the moment you have one: an unplayed leader is a
        card that did nothing all game.

        Ranked through `_leader_rank`, the same version-aware helper every
        other leader-ranking call site uses (`_card_value`, transitively
        `_best_take`/`_best_develop`).  This rule used to read the bare v1
        `LEADER_RANK` table directly regardless of `ctx.version`, so a
        version=2 bot picked WHICH leader in hand to play from the v1
        opinion table while deciding everything else about leaders from the
        v2 tournament table -- an inconsistent lookup, not a deliberate
        choice; fixed here."""
        cands = by_kind.get("play_leader")
        if not cands:
            return None
        best = max(cands, key=lambda m: (_leader_rank(m[1], ctx, p), m[1]))
        if p.leader is None:
            return best
        # Replacing a leader is only right for a clear upgrade.
        if _leader_rank(best[1], ctx, p) >= _leader_rank(p.leader, ctx, p) + 2:
            return best
        return None

    # rule 3 -------------------------------------------------------------
    def _r_happiness(self, state, p, ctx, by_kind):
        """Discontent costs a worker every turn and an uprising is a
        catastrophe; fix it before anything else."""
        if ctx.happy_gap <= 0:
            return None
        mv = self._best_build(state, p, ctx, by_kind, happy_only=True)
        if mv is not None:
            return mv
        mv = self._best_develop(state, p, ctx, by_kind, happy_only=True)
        if mv is not None:
            return mv
        return None

    # rule 4 -------------------------------------------------------------
    def _r_military_floor(self, state, p, ctx, by_kind):
        """Never be the weakest civilisation at the table.

        Weak military is not just lost wars -- it is a standing invitation for
        every aggression in the game to be pointed at you, which is how the
        strategy writers describe most beginner losses.
        """
        # A tactic multiplies what the units you already have are worth, so
        # it comes before buying more units.
        if p.tactic is None:
            best = None
            for mv in by_kind.get("play_tactic", ()):
                if best is None or TACTIC_RANK.get(mv[1], 0) > TACTIC_RANK.get(best[1], 0):
                    best = mv
            if best is not None:
                return best
        if ctx.strength >= ctx.mil_target:
            return None
        # upgrade an existing unit first (no new worker needed)
        best, best_v = None, 0.0
        db = ctx.db
        for mv in by_kind.get("upgrade", ()):
            if db.type_of(mv[1]) not in C.UNIT_TYPES:
                continue
            gain = (db.get(mv[2]).get("strength") or 0) - (db.get(mv[1]).get("strength") or 0)
            if gain > best_v:
                best, best_v = mv, gain
        if best is not None:
            return best
        if p.workers_free > 0:
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

    # rule 5 -------------------------------------------------------------
    def _r_wonder_step(self, state, p, ctx, by_kind):
        """Finish what you start: a half-built wonder is resources sitting
        idle, and the culture only arrives on the last stage."""
        steps = by_kind.get("wonder_step")
        if not steps:
            return None
        if p.wonder is not None and _wonder_value(p.wonder.name, ctx) <= 0:
            return None
        # take the biggest legal step (Masonry allows two stages per action)
        return max(steps, key=lambda m: m[1])

    # rule 6 -------------------------------------------------------------
    def _r_population(self, state, p, ctx, by_kind):
        """Grow whenever there is food and a job waiting -- population is the
        compounding asset -- but never grow into unhappiness."""
        if by_kind.get("pop_free"):
            return by_kind["pop_free"][0]
        # Frederick Barbarossa's combined action buys the same worker for a
        # MILITARY action instead of a civil one, a food cheaper, and puts a
        # unit under it.  Whenever the book wants to grow and he is in play,
        # that is the way to grow -- and a book that ranks him a 7 in
        # LEADER_RANK and then never uses him is the same "declared but never
        # played" defect one level up.
        combo = by_kind.get("barbarossa")
        if not by_kind.get("pop") and not combo:
            return None
        if ctx.workers_free >= 1 and ctx.age <= 1:
            return None            # a worker already idle: place it first
        if ctx.workers_free >= 2:
            return None
        # growing raises the happiness requirement one band; do not step into
        # discontent
        if economy.happy_required(max(0, p.yellow_bank - 1)) > ctx.s.happy:
            return None
        if ctx.late:
            return None            # a worker bought on the last turns cannot pay back
        if combo:
            db = ctx.db
            return max(combo, key=lambda m: (db.get(m[1]).get("strength") or 0,
                                             m[1]))
        return by_kind["pop"][0]

    # rule 7 -------------------------------------------------------------
    def _r_place_worker(self, state, p, ctx, by_kind):
        """An idle worker produces nothing.  Farms and mines first while the
        engine is still being built, then the buildings that score."""
        if p.workers_free <= 0:
            return None
        return self._best_build(state, p, ctx, by_kind)

    # rule 8 -------------------------------------------------------------
    def _r_upgrade(self, state, p, ctx, by_kind):
        """Upgrading is the efficient move: one civil action, no new worker,
        and it frees the old card's production immediately."""
        db = ctx.db
        best, best_v = None, 0.0
        for mv in by_kind.get("upgrade", ()):
            lo, hi = mv[1], mv[2]
            typ = db.type_of(hi)
            if typ in C.UNIT_TYPES:
                continue                       # handled by the military rule
            gain = _prod_value(db.get(hi).get("production") or {}, ctx) - \
                _prod_value(db.get(lo).get("production") or {}, ctx)
            cost = max(0, (db.get(hi).get("buildCost") or 0) -
                       (db.get(lo).get("buildCost") or 0))
            v = gain - cost * 0.4
            if typ in ("farm",) and ctx.food_need <= 0:
                v -= 2.0
            if v > best_v:
                best, best_v = mv, v
        return best if best_v >= 1.5 else None

    # rule 9 -------------------------------------------------------------
    def _r_develop(self, state, p, ctx, by_kind):
        """Science is only worth what you develop with it.  Priority:
        an extra action, then the economy tech you are short of, then the
        science engine, then culture buildings."""
        return self._best_develop(state, p, ctx, by_kind)

    # rule 10 ------------------------------------------------------------
    def _r_action_card(self, state, p, ctx, by_kind):
        best, best_v = None, 0.0
        for mv in by_kind.get("play_action", ()):
            v = _action_card_value(state, p, ctx, mv[1])
            if v > best_v:
                best, best_v = mv, v
        return best if best_v >= 3.0 else None

    # rule 11 ------------------------------------------------------------
    def _r_take_card(self, state, p, ctx, by_kind):
        takes = by_kind.get("take")
        if not takes:
            return None
        return self._best_take(state, p, ctx, takes)

    # rule 12 ------------------------------------------------------------
    def _r_churchill(self, state, p, ctx, by_kind):
        """Winston Churchill's once-per-turn choice draws down no civil or
        military action -- `_h_churchill` never touches `p.civil_actions`/
        `p.military_actions` -- only the choice itself, which is gone for
        the turn whether it is spent or not (`p.churchill_used`).  Never
        spending a free, no-downside bonus was a plain bug (no rule in the
        12-rule list ever referenced `("churchill", ...)`), not a strategy
        call, so it is fixed here.

        WHICH of the two flavours to take *is* a strategy call: culture is
        unconditional value, but the military discount
        (`p.mil_discount`/`p.mil_sci_discount`) is wiped at end of turn
        (`economy.end_of_turn`) and so is entirely wasted if this rule fires
        on a turn nothing military gets bought or upgraded afterwards.
        Predicting that would need to peek at rules this bot has not run
        yet this turn -- a real optimisation, not a bug fix -- so this
        always takes the risk-free flavour instead."""
        cands = by_kind.get("churchill")
        if not cands:
            return None
        for mv in cands:
            if mv[1] == "culture":
                return mv
        return cands[0]

    # ------------------------------------------------------------ helpers
    def _best_build(self, state, p, ctx, by_kind, happy_only=False):
        """Choose what to put the free worker on."""
        db = ctx.db
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
            v = _prod_value(prod, ctx)
            # the shortage you actually have is worth more than the one you
            # do not
            if typ == "farm":
                v += 3.0 * min(2, ctx.food_need)
            elif typ == "mine":
                v += 3.0 * min(2, ctx.res_need)
            if prod.get("happy") and ctx.happy_gap > 0:
                v += 5.0 * min(2, ctx.happy_gap)
            cost = card.get("buildCost") or 0
            v -= cost * 0.5
            if ctx.late and not prod.get("culture"):
                v -= 3.0
            if v > best_v:
                best, best_v = mv, v
        return best if best_v > 0.5 else None

    def _best_develop(self, state, p, ctx, by_kind, happy_only=False):
        db = ctx.db
        best, best_v = None, 0.0
        for mv in by_kind.get("develop", ()):
            name = mv[1]
            card = db.get(name)
            if happy_only and not (card.get("production") or {}).get("happy"):
                continue
            v = _card_value(state, p, ctx, name)
            cost = effects.tech_cost(state, p, name) or 0
            # science is the scarcest currency early; do not blow it all on
            # one card you cannot staff
            v -= cost * 0.6
            if card["type"] in C.WORKER_TYPES and p.workers_free <= 0 \
                    and (card.get("buildCost") or 0) > p.resources:
                v -= 2.0
            if v > best_v:
                best, best_v = mv, v
        return best if best_v >= 1.0 else None

    def _best_take(self, state, p, ctx, takes, first_turn=False):
        """Take cards you will actually play, from the cheap end of the row.

        The row charges 1/2/3 civil actions by position, and every card also
        costs an action to play later, so the book warning is to stop taking
        cards you will never get round to.
        """
        db = ctx.db
        hand = len(p.hand_civil)
        best, best_v = None, 0.0
        for mv in takes:
            idx = mv[1]
            name = state.card_row[idx]
            if name is None:
                continue
            cost = A.take_cost(state, p, idx)
            v = _card_value(state, p, ctx, name)
            typ = db.type_of(name)
            if typ == "wonder" and p.wonder is not None:
                continue
            if ctx.version >= 2:
                # Cards strong players simply never pay for: every one of
                # these costs a SECOND civil action to realise.
                if name in V2_NEVER_TAKE:
                    continue
                # "No leader warrants spending 3 white points (except Age 3)."
                if typ == "leader" and cost >= 3 and ctx.age < 3:
                    continue
                # The two wonders the sources say are only ever worth a
                # bargain price.
                if name in ("Taj Mahal", "Great Wall") and cost > 1:
                    continue
                # 76% of tournament Age I picks are made at 1 CA and only
                # 2.5% at 3 CA, so the price of depth is convex, not linear.
                v -= V2_PRICE_LADDER.get(cost, 12.0) * 3.0
            else:
                # a card costs an action now and an action to play: the deeper
                # slots have to clear a much higher bar
                v -= cost * 3.0
            # cards rot in hand; a full hand is wasted civil actions
            v -= hand * 1.0
            if first_turn:
                # turn one has 1-4 actions total and nothing else to spend
                # them on, so the bar is lower, but depth still matters
                v += 4.0
            if v > best_v:
                best, best_v = mv, v
        return best if best_v > 0.0 else None

    # ----------------------------------------------------- politics phase
    def _politics(self, state, p, ctx, moves):
        """Politics: hit the leader when it is free, avoid wars you might
        lose, and keep the event deck turning."""
        by_kind = {}
        for m in moves:
            by_kind.setdefault(m[0], []).append(m)

        # aggressions are pure profit: the engine only offers targets whose
        # strength is already beaten.  Point them at whoever is winning.
        aggs = by_kind.get("aggression")
        if aggs and ctx.strength >= ctx.mil_target:
            return max(aggs, key=lambda m: (state.players[m[2]].culture, m[1]))

        # war is a bet; only take it with a clear strength lead over the
        # target and with time left to collect
        wars = by_kind.get("war")
        if wars and not ctx.last:
            best = max(wars, key=lambda m: (state.players[m[2]].culture, m[1]))
            tgt = effects.state_stats(state, state.players[best[2]]).strength
            if ctx.strength >= tgt + 5:
                return best

        # a pact is worth having when it is not a gift to a runaway leader
        pacts = by_kind.get("offer_pact")
        if pacts:
            cand = [m for m in pacts
                    if state.players[m[2]].culture <= p.culture]
            if cand:
                return sorted(cand)[0]

        evs = by_kind.get("prepare_event")
        if evs:
            return sorted(evs)[0]
        return ("pol_pass",)

    # -------------------------------------------------- pending decisions
    def _pending(self, state, moves):
        pend = state.pending[-1]
        kind = pend["kind"]
        p = state.actor()
        ctx = _Ctx(state, p)
        ctx.version, ctx.tun = self.version, self.tunables
        if kind == "choice":
            return self._choice(state, p, ctx, pend, moves)
        if kind == "auction":
            return self._auction(state, p, ctx, pend, moves)
        if kind == "defense":
            return self._defense(state, p, ctx, pend, moves)
        if kind == "colonize":
            return self._colonize(state, p, ctx, pend, moves)
        return moves[0]

    def _choice(self, state, p, ctx, pend, moves):
        tag = pend["tag"]
        opts = pend["options"]
        db = ctx.db

        def pick(score):
            best_i, best_v = 0, None
            for i, o in enumerate(opts):
                v = score(o)
                if best_v is None or v > best_v:
                    best_i, best_v = i, v
            return ("choose", best_i)

        if tag == "food_or_res":
            return pick(lambda o: (ctx.food_need * 2 if o == "food"
                                   else ctx.res_need * 2 + 1))

        if tag in ("free_build",):
            def build_score(o):
                if o == "skip":
                    return 0.1
                return _prod_value(db.get(o).get("production") or {}, ctx)
            return pick(build_score)

        if tag == "take_row":
            # a free take: grab the best card, then stop
            def row_score(o):
                if o == "stop":
                    return 0.5
                name = state.card_row[o]
                return _card_value(state, p, ctx, name) if name else 0.0
            return pick(row_score)

        if tag == "gain_block":
            # options are gain blocks; prefer the one the book wants now
            def gain_score(o):
                if isinstance(o, dict):
                    return _prod_value(o, ctx) + (o.get("culture", 0) or 0)
                return 0.0
            return pick(gain_score)

        if tag == "free_civil":
            # options are concrete moves: rank them with the action rules
            sub = [tuple(o) for o in opts]
            chosen = self._action_phase(state, p, ctx, sub)
            for i, o in enumerate(sub):
                if o == chosen:
                    return ("choose", i)
            return ("choose", 0)

        if tag in ("destroy_own", "lose_pop", "flip_wonder"):
            # give up the least valuable thing
            def loss_score(o):
                card = db.get(o) if o in db.by_name else None
                if card is None:
                    return 0.0
                return -_prod_value(card.get("production") or {}, ctx)
            return pick(loss_score)

        if tag == "discard_military":
            def mil_score(o):
                card = db.get(o) if o in db.by_name else None
                if card is None:
                    return 0.0
                t = card["type"]
                # keep defensive tactics and wars; pitch events first
                return {"event": 3.0, "territory": 1.0, "pact": 1.5,
                        "aggression": 0.5, "war": 0.2, "tactic": 0.0,
                        "bonus": 0.4}.get(t, 1.0)
            return pick(mil_score)

        if tag in ("raid", "annex"):
            # take the most valuable thing from the victim
            def loot_score(o):
                card = db.get(o) if o in db.by_name else None
                if card is None:
                    return 0.0
                return _prod_value(card.get("production") or {}, ctx)
            return pick(loot_score)

        if tag == "infiltrate":
            return pick(lambda o: 1.0 if o == "leader" else 0.5)

        if tag == "war_tech":
            # War over Technology: spend the strength advantage on science or
            # on the loser's blue cards.  Both sides of the choice are paid
            # for out of ONE budget, so they are compared PER SCIENCE POINT
            # -- science is the numeraire at 1.0 a point, and a card costing
            # `techCost` has to beat `techCost` science.  The card's own
            # value is the book's existing `SPECIAL_RANK` table via
            # `_card_value`, so this preference cannot drift away from what
            # the book pays for the same card at the row.
            #
            # A steal that does NOT upgrade an icon the book already fills is
            # scored at denial value only: Code of Laws p.3 keeps the higher
            # level card in play and DISCARDS the other, so taking a card
            # sideways or downwards puts nothing in our play area -- it only
            # takes it out of theirs.
            def spoil_score(o):
                if o == "science":
                    return 1.0
                card = db.get(o) if o in db.by_name else None
                if card is None:
                    return 0.0
                cost = max(1, card.get("techCost") or 1)
                icon = A.special_icon(card)
                mine = max((db.level_of(n) for n in p.techs
                            if db.type_of(n) == "special-tech"
                            and A.special_icon(db.get(n)) == icon),
                           default=-1)
                if db.level_of(o) <= mine:
                    return 0.35          # denial only; it is discarded
                return _card_value(state, p, ctx, o) / cost
            return pick(spoil_score)

        if tag == "pact_offer":
            # accept a pact unless it props up the culture leader.
            # The key is "owner", not "from": `actions._h_offer_pact` builds
            # the ctx as {"owner", "name", "a", "b"} and has never written a
            # "from".  `.get("from")` was therefore always None, `leading` was
            # always False, and this bot ACCEPTED EVERY PACT EVER OFFERED --
            # the refusal branch below had never executed once.  From the
            # deciding player's seat the counterparty is the offerer, which is
            # exactly `ctx["owner"]` (the choice is pushed to the target).
            partner = pend.get("ctx", {}).get("owner")
            leading = (partner is not None
                       and state.players[partner].culture > p.culture + 5)
            want = "refuse" if leading else "accept"
            for i, o in enumerate(opts):
                if o == want:
                    return ("choose", i)
            return ("choose", 0)

        if tag == "lose_colony":
            return ("choose", 0)
        return ("choose", 0)

    def _auction(self, state, p, ctx, pend, moves):
        """Colonies are permanent production plus culture, so they are worth
        bidding for -- but not worth crippling the army for."""
        bids = [m for m in moves if m[0] == "bid"]
        if not bids:
            return ("bid_pass",)
        top = max(m[1] for m in bids)
        # spend at most half the force we could bring, and never so much that
        # we drop under the military floor we care about
        cap = max(1, top // 2)
        legal = [m for m in bids if m[1] <= cap]
        if not legal:
            return ("bid_pass",)
        return min(legal, key=lambda m: m[1])

    def _defense(self, state, p, ctx, pend, moves):
        """Defend with the cards that actually add defence; do not burn the
        hand on an attack that is already lost or already survived."""
        defends = [m for m in moves if m[0] == "defend"]
        if not defends:
            return ("defend_done",)
        db = ctx.db
        need = pend.get("atk", 0) - pend.get("dfn", 0)
        if need <= 0:
            return ("defend_done",)
        best, best_v = None, 0.0
        for mv in defends:
            card = db.get(mv[1]) if mv[1] in db.by_name else None
            eff = (card.get("effects") or {}) if card else {}
            bonus = eff.get("defenseBonus")
            v = bonus if isinstance(bonus, int) else 1
            if v > best_v:
                best, best_v = mv, v
        return best if best is not None else ("defend_done",)

    def _colonize(self, state, p, ctx, pend, moves):
        """Build the colonization force out of the cheapest things you own.

        RULES_SPEC 11.3 lets the winner choose; the ranking here is the one
        the engine used to hard-code, made explicit as a POLICY so it can be
        argued with: stop the moment the bid is met (every extra unit is
        permanent strength thrown away); pay the mandatory unit with the
        weakest unit; and close the remaining gap with bonus CARDS before
        further units, because a card is a consumable and a unit is not.
        `interact._colonize_moves` already emits the moves in that order --
        weakest unit first, then cheapest bonus card -- so the choice is
        `moves[0]`, but it is spelled out rather than inherited so a change
        to that ordering cannot silently change this bot's policy.

        JAMES COOK's discards come FIRST, but only when they can finish the
        job on their own.  Each is worth a flat +1 and a Military Bonus card
        is worth 2, 4 or 6 defence if it is kept, so burning junk to close a
        gap of one or two is the cheapest thing on the table -- while spending
        a slot on a gap Cook cannot close (his cap is two) would throw the
        card away and still cost a bonus card afterwards.
        """
        from .. import interact as I
        if ("send_done",) in moves:
            return ("send_done",)
        discards = [m for m in moves if m[0] == "send_discard"]
        if discards:
            gap = pend["bid"] - I.pend_force(state, p, pend)
            if gap <= I.cook_slots_left(pend) * I.COOK_BONUS_PER_DISCARD:
                return discards[0]
        bonuses = [m for m in moves if m[0] == "send_bonus"]
        if bonuses:
            return bonuses[0]
        units = [m for m in moves if m[0] == "send_unit"]
        if units:
            return units[0]
        return discards[0] if discards else moves[0]


# --------------------------------------------------------------- the hybrid

#: Move kinds where the measurements in docs/STRENGTH_CHECK.md say the book
#: beats the trained evaluator.  Everything else is left to the champion.
#:
#: * ``develop``     -- the champion hoards science it never converts
#: * ``pop``         -- it stops growing the population far too early
#: * ``wonder_step`` -- it starts wonders and leaves them unfinished
#: * ``revolution``  -- it will not pay a turn of actions for a permanent one
BOOK_OVERRIDE_KINDS = frozenset(("develop", "pop", "pop_free", "wonder_step",
                                 "revolution"))


class BookImprovedBot:
    """The trained champion, overruled by the book in its known blind spots.

    This is the ablation that separates "the book bot is better" from "the
    book bot is better *because of these specific rules*".  It plays the
    champion's evaluator everywhere except the move kinds in
    :data:`BOOK_OVERRIDE_KINDS`; there, if the book wants to make such a move
    and the champion does not, the book gets its way.
    """

    name = "book-improved"

    def __init__(self, weights=None, rng=None, seed=None):
        from .weighted import WeightedBot
        self.inner = WeightedBot(weights=weights, seed=seed)
        self.book = BookBot(seed=seed)
        self.rng = rng or random.Random(seed)

    def __call__(self, state):
        return self.choose(state, A.legal_moves(state))

    def choose(self, state, moves, rng=None):
        champ = self.inner.choose(state, moves, rng)
        if state.pending or state.phase == "politics":
            return champ
        if champ is not None and champ[0] in BOOK_OVERRIDE_KINDS:
            return champ            # champion already making the right kind of move
        book = self.book.choose(state, moves, rng)
        if book is not None and book[0] in BOOK_OVERRIDE_KINDS and book in moves:
            return book
        return champ


# ======================================================================
# v2: the empirical tournament tier list
# ======================================================================
#
# docs/EXPERT_STRATEGY.md summarises a BGG study of 39 games across 3
# International Championships and 3 Intermezzo seasons, scoring every card by
# the average number of civil actions strong players spent on it per game
# (https://boardgamegeek.com/thread/2494200).  That is *revealed preference*
# from strong humans -- the closest thing to ground truth available.
#
# Two things stop it from being used raw:
#
#   * CA-spend measures what players PAY, not what WINS.  The study annotates
#     some cards as over- or under-performing their price (Colossus is
#     "anti-correlated with winning" at 0.13; Taj Mahal is "undervalued" at
#     0.00).  The ranks below blend spend with those annotations.
#   * The competitive lobby the tier list is drawn from is almost exclusively
#     2-player, so a 2p-derived value must not be applied blindly at 3p/4p.
#     Where the source explicitly splits (Homer: Tier 1 in 3p/4p data but
#     D-tier in 2p), the rank is player-count aware.
#
# Where the experts genuinely disagree, the value is a TUNABLE rather than a
# hard-coded side -- see V2_TUNABLES.

#: Contested calls, exposed as parameters instead of being decided by fiat.
#: docs/EXPERT_STRATEGY.md keeps a deliberately unresolved disagreements
#: table; these are its entries.
#:
#: "TUNABLE" IS ABOUT THE PARAMETER, NOT ABOUT THIS TABLE.  The `tunables=`
#: argument really is plumbed -- `BookBotV2`, `variants/base.py` and
#: `human/base.py` all merge an override dict over these defaults -- but NO
#: caller in the repo supplies one, so in practice every number below is a
#: frozen literal and should be classified as a fitted prior (from the expert
#: sources, cited per entry), not as a live knob somebody is fitting.  That is
#: the intended state: BookBot is a deliberately hand-written external
#: yardstick (docs/STRENGTH_CHECK.md), and hard-coded human opinion is the
#: point of it.  If a lane ever wants to fit these, it is one dict away.
V2_TUNABLES = {
    # "Library of Alexandria wins more than Pyramids" vs "Pyramids taken 20%
    # more often".  >1 favours the Pyramids' extra civil action.
    "pyramids_vs_loa": 0.95,
    # Highest Age I tournament spend (0.59) but "arguably the weakest Age I
    # wonder" on culture-opportunity math.  Scales its rank.
    "great_wall": 0.6,
    # Tournament 0.00 and "utterly useless" vs "+20 culture over Great Wall"
    # and the 30k dataset saying strong players undervalue it.
    "taj_mahal": 0.7,
    # Tier 3 at 0.03, but happiness genuinely solves an early problem.
    "hanging_gardens": 0.6,
    # ("iron_over_bronze" used to live here -- "upgrade to Iron only if it
    # lands early in Age I" -- but no rule below ever consulted it; deleted
    # rather than carried as dead weight.  If a rule is meant to price the
    # Iron-over-Bronze upgrade decision, that rule does not exist yet; this
    # was a missing rule, not just an unread tunable, so it is not invented
    # here -- see this repo's book-bot fix report.)
    # Theology was selected 0 times in 39 tournament games; happiness cards
    # let you skip it.  Multiplies its value.
    "theology": 0.15,
}

#: Rank on the same ~0-10 scale v1 uses, derived from tournament CA-spend and
#: the win-correlation annotations.  Ages A/I/II of the base game.
V2_LEADER_RANK = {
    # Age A -- "a bridge, not a win condition"
    "Hammurabi": 9.0,           # highest of the age at 0.36 CA-spend
    "Aristotle": 8.0,           # "wins if 2 or more are available at the same cost"
    "Alexander the Great": 5.5,  # 0.23
    "Julius Caesar": 3.0,       # 0.03; "in the new TTA he is terrible"
    "Moses": 3.0,               # 0.03; weakest by consensus, food is a trap
    # Age I
    "Joan of Arc": 9.0,         # 0.33, top of the age
    "Frederick Barbarossa": 6.5,  # 0.15
    "Genghis Khan": 5.5,        # 0.08, tempo not strength
    "Leonardo da Vinci": 5.0,   # 0.05 bare; conditional bonus applied below
    "Christopher Columbus": 3.0,  # 0.03; conditional on a colony
    "Michelangelo": 2.0,        # 0.00, "last in every list found"
    # Age II
    "Napoleon Bonaparte": 10.0,  # "most important card in the game"
    "Isaac Newton": 9.0,
    "James Cook": 6.0,
    "William Shakespeare": 6.0,
    "Maximilien Robespierre": 4.0,
    "J. S. Bach": 4.5,          # "consensus weakest Age II", a noob trap
}

#: Homer is the clearest 2p-vs-multiplayer split in the source.
V2_HOMER = {2: 5.0, 3: 7.5, 4: 7.5}

V2_WONDER_RANK = {
    "Library of Alexandria": 9.0,   # 0.31 and wins more than Pyramids
    "Pyramids": 8.5,                # 0.23, taken more often, wins slightly less
    "Colossus": 3.0,                # 0.13 but ANTI-correlated with winning
    "Hanging Gardens": 6.5,         # Tier 3 (0.03); scaled by tunable
    "St. Peter's Basilica": 10.0,   # "the best wonder in the game"
    "Great Wall": 10.0,             # 0.59 highest spend; scaled by tunable
    "Universitas Carolina": 7.0,    # 0.28, "undervalued"
    "Taj Mahal": 7.0,               # 0.00 but "undervalued (!)"; scaled
}

#: Cards strong players essentially never pay for.  The action cards here all
#: cost a second civil action to realise, which is the stated reason.
V2_NEVER_TAKE = frozenset((
    "Stock Pile", "Patriotism (A)", "Cultural Heritage (A)",
    "Frugality (A)", "Frugality (I)",
))

#: The card row charges 1 CA for slots 1-5, 2 for 6-9, 3 for 10-13.  Across 39
#: tournament games 76% of Age I picks were made at 1 CA and only 2.5% at 3 CA,
#: so the price ladder is convex, not linear.
V2_PRICE_LADDER = {0: 0.0, 1: 1.0, 2: 3.5, 3: 9.0}
