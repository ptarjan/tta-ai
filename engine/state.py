"""Game state for Through the Ages (base 2015).

Plain dataclasses, JSON-serializable via to_dict/from_dict. Mutated in
place by engine.actions; snapshots via copy() where needed.

Resource model (see docs/RULES_SPEC.md §6.4): food/resources are kept as
scalars; the number of BLUE TOKENS they occupy is derived (minimal-token
greedy over the denominations of the farm/mine cards in play) so that the
blue bank level -- and therefore corruption -- stays faithful without
tracking individual tokens.
"""
from __future__ import annotations

import copy as _copy
from dataclasses import dataclass, field, asdict, fields

AGES = ["A", "I", "II", "III", "IV"]

#: Set to True by `engine.journal` for the duration of a trial (journalled)
#: `apply`, and back to False by `rollback`.  See `GameState.emit`.
#:
#: It lives here rather than being read out of `engine.journal` only to avoid
#: an import cycle (`journal` imports `state`); `journal.begin`/`rollback` are
#: the only writers.
SUPPRESS_LOG = False


@dataclass
class TechCard:
    """A technology in a player's tableau (or being built: wonder)."""
    name: str
    workers: int = 0          # yellow tokens on the card
    stored: int = 0           # blue tokens on the card (food/resources)


@dataclass
class WonderInProgress:
    name: str
    steps_built: int = 0


@dataclass
class PlayerState:
    idx: int
    # tableau: name -> TechCard, includes farms/mines/urban/units/special
    techs: dict = field(default_factory=dict)
    government: str = "Despotism"
    leader: str | None = None
    used_leader_ability: bool = False   # once-per-game/turn abilities
    wonder: WonderInProgress | None = None
    completed_wonders: list = field(default_factory=list)
    destroyed_wonders: int = 0          # still count for the take surcharge
    homer_wonder: str | None = None     # wonder Homer was tucked under
    tactic: str | None = None
    tactic_exclusive: bool = False      # still in my play area (not public)
    colonies: list = field(default_factory=list)
    # pacts in MY play area: {name, owner, partner, a, b} (§5.9)
    pacts: list = field(default_factory=list)
    flipped_wonders: list = field(default_factory=list)  # Ravages of Time
    hand_civil: list = field(default_factory=list)
    hand_military: list = field(default_factory=list)
    # Cards KNOWN to be in this hand whose identity we do not know.  Always 0
    # in a real game and in every self-play line: the engine deals every card
    # by name, so `hidden_* == 0` and `hand_size()` is `len(hand_*)` exactly.
    #
    # They exist for the app harness (docs/APP_HARNESS.md), which mirrors a
    # rival whose hand SIZE is public (§2.6: cards are taken from an open row
    # in full view) but whose contents it did not transcribe.  Without them a
    # rival with three untranscribed cards reads as an EMPTY hand, which is
    # not "unknown", it is *wrong*: the hand-limit gate (§2.5) says they can
    # still take, and `features()` scores them as holding nothing.
    hidden_civil: int = 0
    hidden_military: int = 0
    taken_leader_ages: list = field(default_factory=list)
    # pools
    yellow_bank: int = 18               # unborn population
    # yellow tokens that entered this supply from OUTSIDE it (card grants and
    # transfers).  Only bookkeeping: lets tests assert that nothing else ever
    # creates a token (§12.2.4).
    yellow_granted: int = 0
    workers_free: int = 1               # available (unused) workers
    blue_total: int = 16                # total blue tokens owned (bank+cards)
    food: int = 0
    resources: int = 0
    science: int = 0
    culture: int = 0
    culture_rate_extra: int = 0         # event-granted per-turn culture
    science_rate_extra: int = 0
    strength_extra: int = 0
    happy_extra: int = 0
    # actions remaining this turn
    civil_actions: int = 4
    military_actions: int = 2
    # per-turn flags
    politics_done: bool = False
    tactic_action_used: bool = False    # max 1 play/copy tactic per phase
    taken_this_turn: list = field(default_factory=list)  # action cards
    # civil actions spent THIS TURN reaching into the card row (§2.3 slot
    # cost + the wonder surcharge / Hammurabi discount).  Reset in
    # game.start_turn next to `taken_this_turn`.  Pure bookkeeping: nothing in
    # the rules reads it, but `features()` cannot see how a civil action was
    # spent, and "a CA spent grabbing from the row" and "a CA spent upgrading a
    # worker" move in OPPOSITE directions as the game goes on
    # (docs/EXPERT_STRATEGY.md:550), so the evaluator needs them as two
    # channels rather than one `ca_left`.  See docs/INFORMATION_AUDIT.md GAP 1.
    ca_spent_taking: int = 0
    hammurabi_used: bool = False        # 1 MA used as CA per turn
    churchill_used: bool = False
    bach_upgrade_used: bool = False
    ocean_liners_used: bool = False
    caesar_double_politics_used: bool = False
    resigned: bool = False
    skip_next_politics: bool = False    # International Agreement (§ CoL p.12)
    ca_penalty_next_turn: int = 0       # Rebellion
    # resource discount pool for military unit builds/upgrades this turn
    # (Patriotism / Wave of Nationalism / Military Build-Up, §3.11)
    mil_discount: int = 0
    # one-time event discount, e.g. {"build": {"resources": 1}} (§ Civil Life)
    one_time_discount: dict = field(default_factory=dict)
    # war bookkeeping: (war_name, attacker_idx, defender_idx)
    wars_declared_on_me: list = field(default_factory=list)
    war_declared_by_me: tuple | None = None

    def has(self, name):
        return name in self.techs

    def hand_size(self, which="civil"):
        """Cards in hand, named or not.  The quantity §2.5 actually counts."""
        if which == "civil":
            return len(self.hand_civil) + self.hidden_civil
        return len(self.hand_military) + self.hidden_military

    def worker_count(self, name):
        t = self.techs.get(name)
        return t.workers if t else 0


@dataclass
class GameState:
    num_players: int
    seed: int
    players: list = field(default_factory=list)   # PlayerState
    current: int = 0
    turn: int = 0                        # 1-based player-turn counter
    round: int = 1                       # 1-based round counter
    start_player: int = 0
    age_civil: str = "A"                 # age of the civil deck being drawn
    age_military: str = "A"
    civil_deck: list = field(default_factory=list)
    military_deck: list = field(default_factory=list)
    card_row: list = field(default_factory=list)  # names or None, 13 slots
    future_events: list = field(default_factory=list)
    current_events: list = field(default_factory=list)
    past_events: list = field(default_factory=list)
    current_events_age: str = "A"
    seeded_by: dict = field(default_factory=dict)  # event -> player idx
    scoring_events: list = field(default_factory=list)  # Age III events
    available_tactics: list = field(default_factory=list)  # common area
    discarded_military: dict = field(default_factory=dict)  # age -> [names]
    # Civil cards swept off the left of the row and destroyed (§2.1), age ->
    # [names], exactly mirroring `discarded_military` above.  A human at the
    # table sees every one of these go; the engine used to write `None` over
    # the slot and destroy the record with it, which made the legal card count
    #     unseen(age) = civil_deck(age, n) - row - hands - tableaux - discard
    # uncomputable.  Nothing in the rules or the turn loop reads this list --
    # it is a record, not state -- so it cannot change play.  See
    # docs/INFORMATION_AUDIT.md GAP 5.
    civil_discard: dict = field(default_factory=dict)       # age -> [names]
    has_military: bool = False           # military data complete?
    last_round: bool = False
    final_round_end: int | None = None   # turn index after which game ends
    game_over: bool = False
    final_scores: list | None = None
    phase: str = "politics"              # politics | actions | done
    # decisions owned by somebody other than the current player (engine.interact)
    pending: list = field(default_factory=list)
    queue: list = field(default_factory=list)
    forced_winner: int | None = None     # last player standing (§5.11)
    log: list = field(default_factory=list)

    # NOT a dataclass field (deliberately un-annotated, so `fields()`,
    # `asdict()` and engine.bots.fastcopy's generated copiers never see it):
    # a class-level default for the private stats cache `engine.effects` hangs
    # off a live state.  `effects.state_stats` used to create the instance
    # attribute off a caught AttributeError, which cost 23,305 raised-and-
    # caught exceptions per 4p 4-game batch because every `copy_state` and
    # every `journal.begin`/`rollback` drops the cache again (it is `_`-prefixed
    # and so is never copied) and the next read re-raises.  With this default,
    # a cold state reads `None` through the class instead of raising, and the
    # cache-absent semantics are unchanged: the instance attribute is still
    # what `state_stats` writes, still absent from `__dict__` until then, and
    # still `__dict__.pop`ed by the journal.
    _stats_cache = None

    def me(self):
        return self.players[self.current]

    def decider(self):
        """Index of the player who must choose the next move."""
        if self.pending:
            return self.pending[-1]["player"]
        return self.current

    def actor(self):
        return self.players[self.decider()]

    def active_players(self):
        return [p for p in self.players if not p.resigned]

    def copy(self):
        return _copy.deepcopy(self)

    # ---------------- serialization ----------------
    def to_dict(self):
        return asdict(self)

    @classmethod
    def from_dict(cls, d):
        d = dict(d)
        players = []
        for pd in d.pop("players", []):
            pd = dict(pd)
            techs = {k: TechCard(**v) for k, v in pd.pop("techs", {}).items()}
            w = pd.pop("wonder", None)
            known = {f.name for f in fields(PlayerState)}
            pd = {k: v for k, v in pd.items() if k in known}
            p = PlayerState(**pd)
            p.techs = techs
            p.wonder = WonderInProgress(**w) if w else None
            players.append(p)
        known = {f.name for f in fields(cls)}
        gs = cls(**{k: v for k, v in d.items() if k in known})
        gs.players = players
        return gs

    def emit(self, msg):
        # A trial move must not write to the real log.  Today it cannot:
        # `copy_state` hands the search a state whose `log` is a fresh `[]`,
        # so the trial's log entries are created and thrown away.  The undo
        # stack has no copy to absorb them, and appending to the real log is
        # not merely wasteful, it is *destructive*: past 400 entries this
        # method does `del self.log[:100]`, and the log is in the fingerprint
        # digest.  Journalling the log instead would mean snapshotting a
        # 400-element list per candidate move -- as much copying as the undo
        # stack exists to remove.  So: suppress, which reproduces the copy
        # path's observable behaviour exactly (nothing reads `state.log`
        # during play; see docs/PYPY.md 9.5).
        if SUPPRESS_LOG:
            return
        self.log.append(f"T{self.turn} P{self.current}: {msg}")
        if len(self.log) > 400:
            del self.log[:100]
