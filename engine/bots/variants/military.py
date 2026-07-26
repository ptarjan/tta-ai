"""MilitaryBot -- the top-2 military position line that actually cashes in.

``docs/STRENGTH_CHECK.md`` found our champion ending 3-player games **+6.6
strength ahead of BookBot and losing by 19 culture**: "It bought an army it
never converted into anything."  This variant exists to be the opponent that
does convert, so that a bot trained against the pool cannot get away with
either ignoring military or hoarding it.

THE PRIORITY LIST (each item cited)
-----------------------------------
1. **Top 2, not merely "not last".**  "Top 2 military position guarantees
   most military events benefit you (4-player) or nearly all (3-player)"
   -- lightningshroud, corroborated at BGG 1866366.  BookBot's floor rule
   ("match the second strongest") is deliberately raised here to "match the
   strongest, plus a margin".
2. **The red techs, in the published order.**  Age I: **Knights** ("taking
   knights is always worth 2 civil actions, sometimes even 3"; 6 of 10 Age I
   tactics need cavalry), Swordsmen as substitute -- "Going into Age 2 with
   none of the military techs is very dangerous."  Age II: **Cannon** ("4 of
   6 Age II tactics require Artillery", "worth 3 civil actions").  Age III:
   **Air Forces**, "the only card I think is worth 3 CA in every
   circumstance... ALWAYS PICK THIS!  If you can't use it yourself, take it
   so the others can't."  Plus **Strategy**, one of only three cards the #1
   BGA player "usually autograb[s] for 2 or 3 CA".
3. **The 3rd military action is the key Age I breakpoint.**  "The difference
   between 2 and 3 military action tokens is greater than any other number
   difference!"  Tiering: 3 MA = draw 3 cards/turn; 4+ MA = targeted
   aggression; 5+ MA = full warfare.
4. **Aggression thresholds are numeric, not vibes.**  "Even with just your
   starting two MAs, it takes a strength lead of 5 to guarantee a successful
   Age I aggression.  Aggressive players may attempt aggressions with only a
   3 or 4 strength lead" -- BGG 2424523.  The engine only offers targets it
   already beats, but a bare 1-point lead still loses to defence cards, so
   this bot demands a real margin.
5. **Attack the leader, repeatedly, and save culture theft for last.**
   "Attack the player who is winning, otherwise attack the weakest one.
   Don't switch targets, attack the same player over and over... Save the
   attacks that steal culture for later, start with the ones that steal
   resources, science and yellow cubes." -- BGG 2801950
6. **The war window is the end of Age II / start of Age III.**  "If you
   wanna win a game by pure military strategy, you should take aggressions
   and declare wars at the end of Age II or the beginning of Age III.
   Building an Age III army is too late."  Age I aggression is "essentially
   never unless a specific combo lands"; "Getting an age I aggression off is
   super hard, I wouldn't recommend trying too hard for it."
7. **Tactics discipline.**  Age I tactics have 2 copies each and never
   become antiquated -- activate freely.  **Age II tactics have exactly one
   copy** -- "delay activating an Age II tactic until necessary... hang on to
   it until late into Age III and then drop it right as you're waging a
   game-closing War over Culture." -- BGG 2424523
8. **Do not over-invest.**  "Over-Investment in Military" is hcy1's #2
   beginner mistake; most Age I aggressions cost the victim only ~2-3
   resources, and mutual over-investment produces "Mutually Assured
   Destruction" where nobody can attack.  The margin knob is therefore
   small and bounded, not "build units forever".

PLAYER-COUNT PARAMETERIZATION
-----------------------------
Aggression is worth materially more with more targets: "the more people
there are, the greater the pressure" (hcy1), and top-2 covers "all (in a 4p)
or almost all (in a 3p) military events".  In 2p there is nobody else to be
attacked instead of you, so the margin is larger; in 3-4p the same margin
buys top-2 more cheaply.  Pacts are 3-4p only (§13 forbids them in 2p), so
the pact preference is inert at 2p by rule, not by choice.

WHAT THIS BOT IS *NOT*
----------------------
It is not the "military wins the game" claim -- the corpus is explicit that
"military strength might not win you the game but it will definitely lose
you the game if you neglect it."  It is the opponent that makes neglect
expensive.
"""
from __future__ import annotations



from .base import VariantBot

__all__ = ["MilitaryBot"]

#: "Save the attacks that steal culture for later, start with the ones that
#: steal resources, science and yellow cubes" (BGG 2801950).  Higher = fire
#: first.  Culture theft is worth the most but is deliberately held back
#: until the target has culture worth taking.
_AGG_ORDER = {
    "Aggression: Plunder (I)": 3.0,
    "Aggression: Plunder (II)": 3.0,
    "Aggression: Plunder (III)": 3.0,
    "Aggression: Spy": 3.0,            # up to 5 science
    "Aggression: Raid (I)": 2.5,
    "Aggression: Raid (II)": 2.5,
    "Aggression: Raid (III)": 2.5,
    "Aggression: Enslave": 2.5,        # yellow cubes
    "Aggression: Annex": 2.0,
    "Aggression: Infiltrate": 1.5,     # removes a leader/wonder, +3 culture/level
    "Aggression: Armed Intervention": 1.0,  # up to 7 culture: save for last
}


class MilitaryBot(VariantBot):
    NAME = "military"

    PROFILE = {
        # rule 1
        "mil_stance": "top2",
        #: While the food/resource targets are unmet up to and including this
        #: age, hold to BookBot's floor instead of the top-2 target (see
        #: mil_goal).  Set to Age A only, by MEASUREMENT: gating through Age I
        #: scored 33.8% against CultureBot and gating through every age scored
        #: 16.9%, while pressing from Age I onward scored 46.9% (n=80 each,
        #: 2p, paired seeds).  That matches the published Age I floor -- "3-4
        #: units + Knights or Swordsmen + a matching tactic, ~10 strength" --
        #: which is an Age I commitment, not an Age II one.
        "econ_first_until_age": 0,
        "mil_margin": {2: 3, 3: 2, 4: 2},
        # rule 4/6: real thresholds, and no Age I adventures
        "agg_lead": {0: 99, 1: 4, 2: 3, 3: 3, 4: 3},
        "war_lead": 5,
        "war_from_age": 2,
        # "certainly don't play [events] when you are the weakest player"
        "seed_events_when_weakest": False,
        "tech_bonus": {
            # rule 2
            "Knights": 8.0,
            "Swordsmen": 5.0,
            "Cannon": 8.0,
            "Riflemen": 4.0,
            "Cavalrymen": 4.0,
            "Air Forces": 15.0,
            "Rockets": 4.0,
            "Modern Infantry": 4.0,
            "Tanks": 4.0,
            # A military plan still has to pay for its red techs: Knights 5
            # science, Cannon 6, Air Forces 12 -- "make sure you always have
            # 12 science available for Air Forces" (BGG 2597183).  Measured
            # at 1.4 science/turn without this, which is MasN's "1 = a way to
            # throw the game".
            "Alchemy": 4.0,
            # rule 3: Warfare is +1 MA +1 strength; Strategy is +2 MA +3 str
            "Warfare": 5.0,
            "Strategy": 9.0,
            "Military Theory": 4.0,
            # the military exit government: +5 strength, -2 science
            "Fundamentalism": 3.0,
            # not this plan's money
            "Masonry": -3.0,
            "Oil": -6.0,
        },
        "tech_veto": frozenset(("Oil",)),
        # rule 2: Air Forces and Strategy are the two cards this bot will
        # reach into the 3-CA slots for, exactly as the source prescribes.
        "must_buy_3ca": frozenset(("Air Forces", "Strategy")),
        "card_bonus": {
            "Air Forces": 8.0,
            "Strategy": 5.0,
            "Knights": 4.0,
            "Cannon": 4.0,
        },
        "leader_bonus": {
            # "Most important card in the game" (#1 BGA player); "biggest
            # potential because he can increase your strength by 6 or even
            # more".  This variant plays Camp A of the Napoleon row.
            "Napoleon Bonaparte": 5.0,
            # Camp B of the Julius Caesar row: "3 red points is very
            # defensive... Caesar and Colossus are a very good match" (hcy1).
            # Worth more with more opponents, per the same source.
            "Julius Caesar": {2: 1.0, 3: 3.0, 4: 3.0},
            # "+1 strength per unit" -- cash-in value, and this bot has units
            "Alexander the Great": 2.0,
            # "the third MA... allows comfortably skipping an age 1 government"
            "Joan of Arc": 2.0,
            # tempo not strength, and "hard to transition out of"
            "Genghis Khan": 1.0,
            "Michelangelo": -2.0,
        },
        "wonder_bonus": {
            # Camp B of the Great Wall row: "most of the games that my
            # opponents conceded on Age II/early Age III involved Great Wall
            # with a lot of cannons or infantry and the right tactics"
            # (250-game BGA player).  This is the only variant that builds
            # the infantry/artillery army that claim depends on.
            "Great Wall": 1.7,
            # Camp B of the Colossus row (hcy1, multiplayer): "the more
            # people there are, the greater the pressure".  Still base-game
            # Colossus, i.e. +2 strength only -- no expansion card draws.
            "Colossus": {2: 1.0, 3: 1.4, 4: 1.4},
        },
        "prod_weights": {"food": 1.0, "resources": 1.2, "science": 1.0,
                         "culture": 0.8, "happy": 1.0, "strength": 1.6},
        # a lab or two, not a science plan
        "build_bonus": {"lab": 1.0, "mine": 1.0},
        "price_scale": 1.0,
        "max_take_cost": {0: 2, 1: 2, 2: 3, 3: 3, 4: 3},
        "wonder_appetite": 0.8,
        "wonder_max": 2,
        "pop_appetite": 1.0,
    }

    #: Military is promoted above the wonder and population rules: this is
    #: the variant that spends its civil actions on the army.  It stays
    #: *below* the happiness rule because an uprising costs the whole
    #: production phase, which no amount of strength repairs.
    RULES = ("_r_round1", "_r_revolution", "_r_play_leader", "_r_happiness",
             "_r_tactics", "_r_military_floor", "_r_population",
             "_r_place_worker", "_r_wonder_step", "_r_upgrade",
             "_r_develop", "_r_action_card", "_r_take_card")

    #: Rule 1 + rule 8, the absolute floors published for a 3p game:
    #: "Age I: 3-4 units + Knights or Swordsmen + a matching tactic ~= 10;
    #: Age II: 4-6 units, Age II tactic, Cannon = 15-25; Age III: ~30."
    #: These are what "top 2" means in absolute terms once every bot at the
    #: table is militarily anaemic, which measurement says ours are.
    AGE_STRENGTH_FLOOR = {0: 0, 1: 10, 2: 18, 3: 30, 4: 30}

    # rules 1 + 8 ----------------------------------------------------------
    def mil_goal(self, state, p, ctx):
        """Top-2 *and* the published absolute floors -- but economy first.

        Without the first gate this bot converts every new worker into a
        soldier from turn 2 and never staffs a mine, which measured out at
        2.5 resources/turn and an army it still could not afford.  That is
        exactly the failure the sources name: "Over-Investment in Military"
        is hcy1's #2 beginner mistake, and "My guess is that you might be
        over-investing in military strength... essentially gambling on Wars
        that may never come" (BGG 2425822).

        So in Age A/I, while the food or resource target is unmet, the bot
        holds to BookBot's floor ("never be the weakest") and spends on the
        economy that will pay for the Age II army.  After that it pursues
        top-2 plus the published absolute floor for the age.
        """
        if ctx.age <= self.profile.get("econ_first_until_age", 0) \
                and (ctx.food_need > 0 or ctx.res_need > 0):
            return ctx.mil_target
        goal = super().mil_goal(state, p, ctx)
        return max(goal, self.AGE_STRENGTH_FLOOR.get(ctx.age, 0))

    # rule 7 --------------------------------------------------------------
    def _r_tactics(self, state, p, ctx, by_kind):
        """Tactics discipline, which BookBot does not implement at all.

        BookBot plays a tactic only when it has none, so it never upgrades.
        Here: always take a tactic if you have none (an army with no tactic
        is worth its raw unit strength and nothing else), but **hold an Age
        II tactic** -- there is exactly one copy of each -- until Age III or
        the last round, "then drop it right as you're waging a game-closing
        War over Culture."
        """
        cands = by_kind.get("play_tactic")
        if not cands:
            return None
        db = ctx.db

        def strength(nm):
            return db.get(nm).get("strength") or 0

        best = max(cands, key=lambda m: (strength(m[1]), m[1]))
        if p.tactic is None:
            # An Age II tactic played early is a singleton spent early; if an
            # Age I tactic is also on offer, use that one instead.
            age1 = [m for m in cands if db.get(m[1]).get("age") == "I"]
            if age1 and ctx.age < 3:
                return max(age1, key=lambda m: (strength(m[1]), m[1]))
            return best
        if strength(best[1]) <= strength(p.tactic):
            return None
        if db.get(best[1]).get("age") in ("II", "III") and ctx.age < 3 \
                and not ctx.last:
            return None                       # hold the singleton
        return best

    # rule 5 --------------------------------------------------------------
    def _politics(self, state, p, ctx, moves):
        """Cash strength in: aggressions first, ordered by what they steal.

        Differs from the base only in the *choice among* legal aggressions:
        the target is the culture leader (so the bot naturally keeps hitting
        the same player, as the source prescribes) and, among cards, the
        resource/science/cube thefts fire before the culture thefts.
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
                # late game the culture thefts stop being "for later"
                late = ctx.age >= 3
                return max(ok, key=lambda m: (
                    state.players[m[2]].culture,
                    (-_AGG_ORDER.get(m[1], 2.0) if late
                     else _AGG_ORDER.get(m[1], 2.0)),
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
            # "Don't offer pacts to the winning player" (BGG 2801950)
            cand = [m for m in pacts
                    if state.players[m[2]].culture <= p.culture]
            if cand:
                return sorted(cand)[0]

        evs = by_kind.get("prepare_event")
        if evs and not self._weakest(state, p):
            return sorted(evs)[0]
        return ("pol_pass",)
