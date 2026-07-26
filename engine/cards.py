"""Card database loader.

Merges the data/*.json part-files into one indexed card database.
Cards are plain dicts (straight from JSON) with guaranteed keys:
name, age, type, deck, plus type-specific fields. Effects are dicts of
mechanical tags; anything the engine can't interpret mechanically lives
in "text" and is handled by name-dispatch in effects.py.
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

CIVIL_ROW_TYPES = {  # taken from the civil card row
    "farm", "mine", "lab", "temple", "arena", "library", "theater",
    "infantry", "cavalry", "artillery", "air", "government", "special-tech",
    "wonder", "leader", "action",
}


class CardDB:
    def __init__(self, cards):
        self.cards = cards
        self.by_name = {}
        for c in cards:
            if c["name"] in self.by_name:
                raise ValueError(f"duplicate card name {c['name']}")
            self.by_name[c["name"]] = c

    @classmethod
    def load(cls, data_dir=DATA_DIR):
        cards = []
        for fn in PART_FILES:
            path = os.path.join(data_dir, fn)
            with open(path) as fh:
                part = json.load(fh)
            if part.get("scope") != "base-2015":
                raise ValueError(f"{fn}: wrong scope {part.get('scope')}")
            if not part.get("complete"):
                raise ValueError(f"{fn}: not marked complete")
            for c in part["cards"]:
                c.setdefault("deck",
                             "civil" if c["type"] in CIVIL_ROW_TYPES
                             else "military")
                c.setdefault("effects", {})
                c.setdefault("count", {"2p": 1, "3p": 1, "4p": 1})
                cards.append(c)
        return cls(cards)

    def get(self, name):
        return self.by_name[name]

    def civil_deck(self, age, num_players):
        """Card names (with multiplicity) for the civil deck of an age.
        Wonders/leaders/starting techs (count 0) are excluded by count."""
        return self._deck("civil", age, num_players)

    def military_deck(self, age, num_players):
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
