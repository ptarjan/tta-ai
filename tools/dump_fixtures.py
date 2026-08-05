"""Dump seeded self-play games as differential-testing fixtures for the Rust
port (`rust/DESIGN.md`, "How correctness is established").

There is no in-process bridge between the two engines (no `pyo3`; the Rust
engine is a standalone native program), so the oracle is offline: this script
plays a game with the real Python engine and records, per ply, exactly what a
Rust replay needs to check itself against --

    ply      -- 0-based index of this decision (one `actions.apply` call)
    decider  -- player index who chose `chosen`, from `state.decider()`
                BEFORE `chosen` was applied (matters when a pending decision
                hands the move to someone other than `state.current`)
    phase    -- `state.phase` at decision time ("politics" | "actions" |
                "done"), also captured BEFORE `chosen` was applied
    legal    -- the full `actions.legal_moves(state)` list, BEFORE apply, in
                the exact order the engine produced it (move ordering is part
                of the contract: bots break ties by index, so a reordered
                list silently changes play -- rust/DESIGN.md)
    chosen   -- the move actually applied, one element of `legal`
    digest   -- `state_digest` of the state AFTER `chosen` was applied
    state    -- `GameState.to_dict()` AFTER `chosen` was applied, included
                only every `--state-every` plies (always on the last ply)

Usage:

    python3.13 tools/dump_fixtures.py --players 3 --seed 1001 --games 3 \\
        --out rust/tests/fixtures --max-plies 600 --state-every 25

    python3.13 tools/dump_fixtures.py --verify --players 3 --seed 1001

Output is one JSON-Lines file per game: a "header" record, then one "ply"
record per move, then a "footer" record. See `_move_json` and `state_digest`
for the exact, documented encoding of moves and of the digest.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
import random
import sys

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

from engine import actions, game                     # noqa: E402
from engine.bots import GreedyBot, RandomBot          # noqa: E402

REPO_ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))

_BOTS = {"greedy": GreedyBot, "random": RandomBot}

_DUMP_JSON_KW = dict(sort_keys=True, separators=(",", ":"))

#: Used ONLY for the on-disk fixture file (`dump_game`), never for
#: `state_digest`. `sort_keys=True` is right for the digest (order-
#: insensitivity where Python's own dict order carries no information the
#: Rust side can reproduce -- see `state_digest`'s docstring), but it is
#: WRONG for the raw "state" snapshot embedded in a `ply` record: `p.techs`
#: is a `dict[name, TechCard]` whose insertion order IS the tableau's BUILD
#: order, and the Rust port's `Tableau` deliberately preserves that order
#: (`rust/DESIGN.md`, `rust/src/state.rs::Tableau::remove`'s doc comment --
#: `economy.lose_population` weakens the FIRST worker-holding card in build
#: order). `json.dumps(sort_keys=True)` would alphabetize `techs`' keys on
#: the way to disk and silently discard exactly the order
#: `GameState::from_json` needs to reconstruct a `Tableau` Python would
#: agree with. Found 2026-08-05 while building that loader: every fixture
#: previously on disk had this bug (verified against `2p_seed1.jsonl`'s
#: `techs` keys, which came out alphabetically sorted); fixtures were
#: regenerated once this constant existed.
_FILE_JSON_KW = dict(sort_keys=False, separators=(",", ":"))


# --------------------------------------------------------------- git rev
#
# NEVER shells out to `git`. This tree can be the working tree of a live
# league arm; a `git` subprocess anywhere near it is out of bounds even for
# something as innocuous as `git rev-parse HEAD`. Reading `.git/HEAD` and the
# ref file it points at is plain file I/O and carries none of that risk.

def _engine_git_rev(repo_root=REPO_ROOT):
    """Best-effort current commit hash, or `None` if anything is unexpected
    (detached-HEAD edge cases, a missing `.git`, a packed-refs layout this
    does not anticipate). Never raises and never runs `git`."""
    try:
        git_dir = os.path.join(repo_root, ".git")
        with open(os.path.join(git_dir, "HEAD")) as f:
            head = f.read().strip()
        if not head.startswith("ref:"):
            return head or None                 # detached HEAD: a bare sha
        ref = head.split(" ", 1)[1].strip()
        loose = os.path.join(git_dir, ref)
        if os.path.exists(loose):
            with open(loose) as f:
                return f.read().strip()
        packed = os.path.join(git_dir, "packed-refs")
        if os.path.exists(packed):
            with open(packed) as f:
                for line in f:
                    line = line.strip()
                    if line.endswith(" " + ref):
                        return line.split()[0]
        return None
    except OSError:
        return None


# ------------------------------------------------------------- move I/O

def _move_json(mv):
    """Canonical, lossless move serialization.

    A move is a small Python tuple, e.g. `("upgrade", "Bronze", "Iron")`.
    This becomes the JSON array `["upgrade", "Bronze", "Iron"]` -- element
    order preserved exactly, nothing reordered, nothing deduplicated. This is
    exactly what `json.dumps` would already do to a tuple; it is spelled out
    as its own function so the "canonical" contract has one place to change.
    """
    return list(mv)


# ------------------------------------------------------------ the digest

#: `log` is prose (`GameState.emit`'s free-text messages) and it self-
#: truncates past 400 entries (see `GameState.emit`), so it is not part of
#: the game state and would make the digest depend on log-trimming history
#: rather than on the game. Nothing else is excluded: `GameState._stats_cache`
#: is not a dataclass field, so `to_dict()` (`dataclasses.asdict`) never
#: produces it in the first place.
_DIGEST_EXCLUDE = ("log",)


def state_digest(state):
    """A stable digest of `state`, order-insensitive exactly where the Python
    state itself is, and order-sensitive exactly where it is not.

    `GameState.to_dict()` (`dataclasses.asdict`) recursively turns every
    nested dataclass (`PlayerState`, `TechCard`, `WonderInProgress`) into a
    plain dict/list/scalar tree already, so nothing here needs to walk
    dataclasses itself.

    The hash is `blake2b` over `json.dumps(..., sort_keys=True)`:

    * `sort_keys=True` sorts every dict's keys alphabetically, RECURSIVELY.
      That is exactly the order-insensitivity a dict needs: `p.techs` (tech
      name -> TechCard), `state.seeded_by`, `civil_discard` /
      `civil_removed` / `discarded_military` (age -> [names]), and
      `p.one_time_discount` are all Python dicts whose iteration order the
      real engine visibly depends on (see `engine/statediff.py`'s docstring
      on why dict order matters for the *undo-stack* oracle) -- but the Rust
      side represents none of them as an insertion-ordered dict (DESIGN.md
      rule 1: "any `HashMap<String, _>` in the engine is a bug"), so there is
      no insertion order for it to reproduce, and comparing against one would
      make the fixture fail for a difference that carries no information
      about correctness. Sorting by key removes exactly that non-signal.
    * Lists are NOT reordered: `json.dumps` preserves list order, and list
      order there is real game state -- `card_row` is 13 positional slots,
      `civil_deck`/`military_deck` order is draw order (next card off the
      top), `players` is seating order, `hand_civil`/`hand_military` order
      is the order cards were taken. A divergence in any of those IS a
      correctness bug and must not be hashed away.
    * A `tuple` (`PlayerState.war_declared_by_me`) serializes identically to
      a `list` -- a deliberate loss, since Rust has no tuple/list distinction
      to check it against either.
    * `separators=(",", ":")` is not required for correctness (the hash
      covers the string either way); it makes the canonical string cheap to
      reproduce by hand for a spot check.
    """
    d = state.to_dict()
    for k in _DIGEST_EXCLUDE:
        d.pop(k, None)
    canon = json.dumps(d, **_DUMP_JSON_KW)
    return hashlib.blake2b(canon.encode("utf-8")).hexdigest()


# ------------------------------------------------------------- self-play

def _pick(bot, state, moves):
    """Adapter over the two bot APIs in `engine.bots`: `GreedyBot.pick` takes
    the already-computed move list; `RandomBot` only has `.choose`, which
    accepts the same `(state, moves)` shape. Using either avoids recomputing
    `legal_moves` a second time -- it is a pure function of `state`, so a
    second call would return an equal list anyway, but calling the bot with
    the SAME list object this script already recorded is what guarantees
    `chosen` is one of `legal`'s own elements, not merely equal to one.
    """
    if hasattr(bot, "pick"):
        return bot.pick(state, moves)
    return bot.choose(state, moves)


def play_fixture(num_players, seed, max_plies, state_every, bot_name="greedy"):
    """Play one seeded self-play game. Returns `(header, plies, footer)`,
    each a JSON-serializable dict (`plies` a list of them).

    Mirrors `engine.game.play_game`'s bot construction (same `seed * 131 + i`
    per-player bot seed). Unlike an earlier version of this script, `apply`'s
    rng is NOT one persistent stream threaded through the whole game: that
    made a fixture's mid-game shuffles depend on every prior shuffle call
    ever made against that stream, a position no `GameState` snapshot can
    recover (see `rust/src/game.rs`'s KNOWN GAPS block, gap 2, and commit
    `b258b9a`). Instead each `apply` call gets its own rng freshly derived
    from the state via `game._rng_for`, exactly as `game.end_turn` /
    `game.start_turn` do and exactly as the documented default entry point
    `actions.apply(state, mv)` now does (see `actions._h_prepare_event`'s own
    `_rng_for` backfill) -- so a fixture recorded this way is reproducible
    from a state snapshot alone, which is the whole point of a differential
    fixture. `play_game` itself is still not used because it does not expose
    the legal-move list or the decider for each ply.
    """
    bot_cls = _BOTS[bot_name]
    bots = [bot_cls(random.Random(seed * 131 + i)) for i in range(num_players)]
    state = game.new_game(num_players, seed=seed)

    header = {
        "kind": "header",
        "players": num_players,
        "seed": seed,
        "bot": bot_name,
        "state_every": state_every,
        "max_plies": max_plies,
        "engine_rev": _engine_git_rev(),
    }

    plies = []
    ply = 0
    while not state.game_over and ply < max_plies:
        decider = state.decider()
        phase = state.phase
        legal = actions.legal_moves(state)
        chosen = _pick(bots[decider], state, legal)
        actions.apply(state, chosen, game._rng_for(state))

        rec = {
            "kind": "ply",
            "ply": ply,
            "decider": decider,
            "phase": phase,
            "legal": [_move_json(m) for m in legal],
            "chosen": _move_json(chosen),
            "digest": state_digest(state),
        }
        if state_every > 0 and ply % state_every == 0:
            rec["state"] = state.to_dict()
        plies.append(rec)
        ply += 1

    # "Always include the final state": whatever ended the loop -- a normal
    # finish or hitting --max-plies -- the LAST ply record carries the full
    # state even if it fell on a `state_every` off-beat.
    if plies and "state" not in plies[-1]:
        plies[-1]["state"] = state.to_dict()

    footer = {
        "kind": "footer",
        "plies": ply,
        "game_over": state.game_over,
        "truncated": (not state.game_over) and ply >= max_plies,
        "scores": game.scores(state),
        "winners": game.winners(state),
    }
    return header, plies, footer


def dump_game(path, num_players, seed, max_plies, state_every, bot_name="greedy"):
    """Play one game and write its fixture file. Returns the ply count."""
    header, plies, footer = play_fixture(num_players, seed, max_plies,
                                          state_every, bot_name)
    with open(path, "w") as f:
        for rec in (header, *plies, footer):
            f.write(json.dumps(rec, **_FILE_JSON_KW) + "\n")
    return len(plies)


# ------------------------------------------------------------- verify

def verify_determinism(num_players, seed, max_plies=2000, bot_name="greedy"):
    """Play the same seeded game twice, in-process, and assert the two
    digest streams (and the two chosen-move streams) match exactly.

    This is the whole determinism argument: a fixture is only useful as an
    oracle if generating it twice produces the same answer, and this is
    cheaper and more direct than diffing two on-disk files (`--verify` never
    touches disk; `state_every=0` skips building `state` dicts it does not
    need). Raises `AssertionError` naming the first divergence; returns the
    ply count on success.
    """
    def digests(run_id):
        _, plies, _ = play_fixture(num_players, seed, max_plies, 0, bot_name)
        return [(p["ply"], p["digest"], p["chosen"]) for p in plies]

    a = digests(1)
    b = digests(2)
    if len(a) != len(b):
        raise AssertionError(
            f"determinism check: run 1 has {len(a)} plies, run 2 has {len(b)}")
    for (pa, da, ca), (pb, db, cb) in zip(a, b):
        assert pa == pb
        if ca != cb:
            raise AssertionError(f"ply {pa}: chosen move differs: {ca!r} != {cb!r}")
        if da != db:
            raise AssertionError(f"ply {pa}: digest differs: {da} != {db}")
    return len(a)


# ------------------------------------------------------------------- CLI

def _build_argparser():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--players", type=int, choices=(2, 3, 4), default=3)
    ap.add_argument("--seed", type=int, default=0,
                     help="seed of the first game; game i uses seed + i")
    ap.add_argument("--games", type=int, default=1)
    ap.add_argument("--out", default=None, help="output directory")
    ap.add_argument("--max-plies", type=int, default=2000)
    ap.add_argument("--state-every", type=int, default=1,
                     help="include full state every K plies (always on the "
                          "last ply); 1 means every ply")
    ap.add_argument("--bot", choices=tuple(_BOTS), default="greedy")
    ap.add_argument("--verify", action="store_true",
                     help="run one game twice in-process and check the "
                          "digest streams match, instead of dumping fixtures")
    return ap


def main(argv=None):
    args = _build_argparser().parse_args(argv)

    if args.verify:
        n = verify_determinism(args.players, args.seed, args.max_plies, args.bot)
        print(f"OK: {n} plies, digests identical across two in-process runs "
              f"(players={args.players} seed={args.seed} bot={args.bot})")
        return 0

    if not args.out:
        print("error: --out DIR is required unless --verify", file=sys.stderr)
        return 2
    os.makedirs(args.out, exist_ok=True)
    for i in range(args.games):
        seed = args.seed + i
        path = os.path.join(args.out, f"{args.players}p_seed{seed}.jsonl")
        n = dump_game(path, args.players, seed, args.max_plies,
                       args.state_every, args.bot)
        size = os.path.getsize(path)
        print(f"wrote {path}: {n} plies, {size} bytes")
    return 0


if __name__ == "__main__":
    sys.exit(main())
