#!/usr/bin/env python3
"""List the container mutation sites the undo stack has to convert by hand.

docs/PYPY.md section 9.2: attribute writes (300 of 470 sites) are handled
without call-site edits by the journalling `__setattr__`, so what is left is
the container surface -- subscript assignment, `del`, and the list/dict/set
mutator methods.  Those *do* need a `journal.touch(...)` at every site, and a
missed one is a silent corruption of the real game.

This is the working list for that conversion and, later, the checklist for the
coverage audit (step 6): every site here must either be converted or argued
away in writing.

    python3 tools/find_mutations.py                 # summary table
    python3 tools/find_mutations.py engine/actions.py   # sites, with source
    python3 tools/find_mutations.py --todo          # unconverted sites only

A site counts as CONVERTED when the mutated expression is literally
`journal.touch(...)`, which is the only form the conversion is allowed to use
-- so this check is textual on the AST, not a guess.
"""
from __future__ import annotations

import ast
import os
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))

#: methods that mutate the receiver in place
MUTATORS = {
    "append", "extend", "insert", "remove", "pop", "clear", "sort",
    "reverse", "update", "add", "discard", "setdefault", "popitem",
    "intersection_update", "difference_update", "symmetric_difference_update",
}

DEFAULT_FILES = [
    "engine/actions.py", "engine/effects.py", "engine/interact.py",
    "engine/game.py", "engine/events.py", "engine/economy.py",
    "engine/cards.py", "engine/state.py",
]


def _is_touch(node):
    """True if `node` is a `journal.touch(...)` / `touch(...)` call."""
    if not isinstance(node, ast.Call):
        return False
    f = node.func
    if isinstance(f, ast.Attribute):
        return f.attr == "touch"
    return isinstance(f, ast.Name) and f.id == "touch"


def _root_name(node):
    """The leftmost name of an attribute/subscript chain, for triage."""
    while True:
        if isinstance(node, ast.Attribute):
            node = node.value
        elif isinstance(node, ast.Subscript):
            node = node.value
        elif isinstance(node, ast.Call):
            node = node.func
        else:
            break
    return node.id if isinstance(node, ast.Name) else "?"


class Scan(ast.NodeVisitor):
    def __init__(self):
        self.sites = []          # (line, kind, root, converted)

    def _add(self, node, kind, target):
        self.sites.append((node.lineno, kind, _root_name(target),
                           _is_touch(target)))

    def visit_Assign(self, node):
        for t in node.targets:
            if isinstance(t, ast.Subscript):
                self._add(node, "setitem", t.value)
        self.generic_visit(node)

    def visit_AugAssign(self, node):
        if isinstance(node.target, ast.Subscript):
            self._add(node, "augitem", node.target.value)
        self.generic_visit(node)

    def visit_Delete(self, node):
        for t in node.targets:
            if isinstance(t, ast.Subscript):
                self._add(node, "delitem", t.value)
        self.generic_visit(node)

    def visit_Call(self, node):
        f = node.func
        if isinstance(f, ast.Attribute) and f.attr in MUTATORS:
            self._add(node, f.attr, f.value)
        self.generic_visit(node)


def scan(path):
    src = open(os.path.join(ROOT, path), encoding="utf-8").read()
    s = Scan()
    s.visit(ast.parse(src))
    return s.sites, src.splitlines()


def main(argv):
    todo_only = "--todo" in argv
    files = [a for a in argv if not a.startswith("-")] or DEFAULT_FILES
    detail = len(files) == 1 or todo_only

    total = done = 0
    print(f"{'file':<22} {'sites':>6} {'converted':>10} {'todo':>6}")
    for path in files:
        sites, lines = scan(path)
        n = len(sites)
        d = sum(1 for s in sites if s[3])
        total += n
        done += d
        print(f"{path:<22} {n:>6} {d:>10} {n - d:>6}")
        if detail:
            for line, kind, root, conv in sites:
                if todo_only and conv:
                    continue
                mark = "ok " if conv else "TODO"
                print(f"    {mark} {path}:{line:<5} {kind:<10} {root:<12}"
                      f" | {lines[line - 1].strip()[:70]}")
    print(f"{'TOTAL':<22} {total:>6} {done:>10} {total - done:>6}")


if __name__ == "__main__":
    main(sys.argv[1:])
