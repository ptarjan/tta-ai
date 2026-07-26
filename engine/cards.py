"""Card database loader.

Merges the data/*.json part-files into one indexed card database.
Cards are plain dicts (straight from JSON) with guaranteed keys:
name, age, type, deck, plus type-specific fields. Effects are dicts of
mechanical tags; anything the engine can't interpret mechanically lives
in "text" and is handled by name-dispatch in effects.py.

Part-files may still be under construction (``"complete": false``).  Those
are loaded anyway; their names are recorded in ``db.incomplete_parts`` and
``db.has_military`` tells the engine whether the military side of the game
(politics phase, events, aggressions, wars, military card draws) can be
played at all.  When the data is incomplete the engine runs civil-only and
switches the military rules on automatically once the file is finished.
"""
from __future__ import annotations

import json
import os

DATA_DIR = os.path.join(os.path.dirname(__file__), "..", "data")

PART_FILES = [
    "cards_civil.json",
    "cards_wonders_leaders.json",
    "cards_military_actions.json",
]

AGES = ["A", "I", "II", "III", "IV"]
AGE_LEVEL = {a: i for i, a in enumerate(AGES)}

CIVIL_ROW_TYPES = {  # taken from the civil card row
    "farm", "mine", "lab", "temple", "arena", "library", "theater",
    "infantry", "cavalry", "artillery", "air", "government", "special-tech",
    "wonder", "leader", "action",
}

URBAN_TYPES = {"lab", "temple", "library", "theater", "arena"}
UNIT_TYPES = {"infantry", "cavalry", "artillery", "air"}
PRODUCTION_TYPES = {"farm", "mine"}
# technology types that can hold workers
WORKER_TYPES = URBAN_TYPES | UNIT_TYPES | PRODUCTION_TYPES

# military card types the engine needs before it can run the politics phase
REQUIRED_MILITARY_TYPES = {"event", "territory", "aggression", "war",
                           "bonus", "tactic"}


def level(age):
    """Age as a number: A=0, I=1, II=2, III=3, IV=4."""
    return AGE_LEVEL[age]


def _disambiguate(cards):
    """Make card names unique keys.

    A few military cards share a name across ages (Aggression: Plunder I/II/
    III, the six territories in Ages I and II).  Names are the engine's card
    identity (decks, hands, the card row all store names), so duplicated
    names get an age suffix; ``card["baseName"]`` keeps the printed name for
    rules that key on it (war spoils, one-per-name).
    """
    seen = {}
    for c in cards:
        seen[c["name"]] = seen.get(c["name"], 0) + 1
    for c in cards:
        if seen[c["name"]] > 1:
            c["name"] = f"{c['baseName']} ({c['age']})"


class CardDB:
    def __init__(self, cards, incomplete_parts=()):
        self.cards = cards
        self.incomplete_parts = list(incomplete_parts)
        self.by_name = {}
        for c in cards:
            if c["name"] in self.by_name:
                raise ValueError(f"duplicate card name {c['name']}")
            self.by_name[c["name"]] = c
        self.types = {c["type"] for c in cards}
        # Military rules are playable as soon as every required military card
        # type is present; a part-file still marked "complete": false only
        # means more cards may arrive later, not that the data is unusable.
        self.has_military = REQUIRED_MILITARY_TYPES <= self.types
        # civil action (yellow) cards live in the same pending part-file
        self.has_action_cards = "action" in self.types

    @classmethod
    def load(cls, data_dir=DATA_DIR):
        cards = []
        incomplete = []
        for fn in PART_FILES:
            path = os.path.join(data_dir, fn)
            with open(path) as fh:
                part = json.load(fh)
            if part.get("scope") != "base-2015":
                raise ValueError(f"{fn}: wrong scope {part.get('scope')}")
            if not part.get("complete"):
                incomplete.append(fn)
            for c in part["cards"]:
                c.setdefault("deck",
                             "civil" if c["type"] in CIVIL_ROW_TYPES
                             else "military")
                c.setdefault("effects", {})
                c.setdefault("count", {"2p": 1, "3p": 1, "4p": 1})
                c["baseName"] = c["name"]
                cards.append(c)
        _disambiguate(cards)
        return cls(cards, incomplete)

    def get(self, name):
        return self.by_name[name]

    def type_of(self, name):
        return self.by_name[name]["type"]

    def age_of(self, name):
        return self.by_name[name]["age"]

    def level_of(self, name):
        return AGE_LEVEL[self.by_name[name]["age"]]

    def civil_deck(self, age, num_players):
        """Card names (with multiplicity) for the civil deck of an age.
        Wonders/leaders/starting techs (count 0) are excluded by count."""
        return self._deck("civil", age, num_players)

    def military_deck(self, age, num_players):
        if not self.has_military:
            return []
        return self._deck("military", age, num_players)

    def _deck(self, deck, age, num_players):
        key = f"{num_players}p"
        out = []
        for c in self.cards:
            if c["deck"] == deck and c["age"] == age:
                out.extend([c["name"]] * c["count"].get(key, 0))
        return out

    def wonders(self, age):
        return [c for c in self.cards
                if c["type"] == "wonder" and c["age"] == age]

    def leaders(self, age):
        return [c for c in self.cards
                if c["type"] == "leader" and c["age"] == age]

    def of_type(self, type_):
        return [c for c in self.cards if c["type"] == type_]


_DB = None


def db():
    """Process-wide singleton card database."""
    global _DB
    if _DB is None:
        _DB = CardDB.load()
    return _DB
