"""Game state for Through the Ages (base 2015).

Plain dataclasses, JSON-serializable via to_dict/from_dict. Mutated in
place by engine.actions; snapshots via copy() where needed.
"""
from __future__ import annotations

import copy as _copy
from dataclasses import dataclass, field, asdict

AGES = ["A", "I", "II", "III", "IV"]


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
    homer_wonder: str | None = None     # wonder Homer was tucked under
    tactic: str | None = None
    colonies: list = field(default_factory=list)
    pacts: list = field(default_factory=list)
    hand_civil: list = field(default_factory=list)
    hand_military: list = field(default_factory=list)
    # pools
    yellow_bank: int = 18               # unborn population, set at setup
    workers_free: int = 1               # available workers
    blue_bank: int = 16
    food: int = 0
    resources: int = 0
    science: int = 0
    culture: int = 0
    culture_rate_extra: int = 0         # event-granted per-turn culture
    # actions remaining this turn
    civil_actions: int = 4
    military_actions: int = 2
    # per-turn flags
    ocean_liners_used: bool = False
    caesar_double_politics_used: bool = False
    # war bookkeeping: (war_name, attacker_idx, defender_idx)
    wars_declared_on_me: list = field(default_factory=list)
    war_declared_by_me: tuple | None = None


@dataclass
class GameState:
    num_players: int
    seed: int
    players: list = field(default_factory=list)   # PlayerState
    current: int = 0
    turn: int = 0
    age_civil: str = "A"                 # age of the civil deck being drawn
    civil_deck: list = field(default_factory=list)
    military_deck: list = field(default_factory=list)
    card_row: list = field(default_factory=list)  # (name, age) or None
    future_events: list = field(default_factory=list)
    current_events: list = field(default_factory=list)
    current_events_age: str = "A"
    seeded_by: dict = field(default_factory=dict)  # event -> player idx
    scoring_events: list = field(default_factory=list)  # 4 Age III events
    available_tactics: list = field(default_factory=list)  # shared board
    discarded_military: list = field(default_factory=list)
    game_over: bool = False
    final_scores: list | None = None
    phase: str = "politics"              # politics | actions | done
    log: list = field(default_factory=list)

    def me(self):
        return self.players[self.current]

    def copy(self):
        return _copy.deepcopy(self)

    def to_dict(self):
        d = asdict(self)
        return d

    def emit(self, msg):
        self.log.append(f"T{self.turn} P{self.current}: {msg}")
        if len(self.log) > 400:
            del self.log[:100]
