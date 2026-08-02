"""The own-seed event terms, and the registry guard that keeps them honest.

`weighted._EVENT_YIELD` prices an event effect block, and `events.apply_gains`
applies one.  Those are two lists of the same effect keys maintained in two
files, which is the exact "present in one list, absent from the other, and
nothing fails when they disagree" shape this project keeps finding.  The first
test below makes them disagree loudly instead.
"""

import ast
import inspect
import random

from engine import events, game
from engine.bots.weighted import (DEFAULT_WEIGHTS, WeightedBot, _EVENT_YIELD,
                                  evaluate, features, my_event_threat,
                                  my_seeded_pending, my_seeds)


def _applied_effect_keys():
    """Every string `events.apply_gains` dispatches on, read from its AST.

    Read rather than hand-listed: a hand-listed copy is a third registry, and
    a third registry rots the same way the first two do.
    """
    tree = ast.parse(inspect.getsource(events.apply_gains))
    keys = set()
    for node in ast.walk(tree):
        if not isinstance(node, ast.Compare):
            continue
        for op, comp in zip(node.ops, node.comparators):
            if isinstance(op, ast.Eq) and isinstance(comp, ast.Constant) \
                    and isinstance(comp.value, str):
                keys.add(comp.value)
            elif isinstance(op, ast.In) and isinstance(comp, ast.Tuple):
                for elt in comp.elts:
                    if isinstance(elt, ast.Constant) \
                            and isinstance(elt.value, str):
                        keys.add(elt.value)
    assert len(keys) > 10, "AST scrape found nothing; apply_gains changed shape"
    return keys


def test_event_yield_covers_every_applied_key():
    applied = _applied_effect_keys()
    missing = sorted(applied - set(_EVENT_YIELD))
    assert not missing, (
        "engine/bots/weighted.py _EVENT_YIELD does not price these effect "
        f"keys that events.apply_gains applies: {missing}.  An event the "
        "engine resolves and the evaluator cannot see is a silent blind spot; "
        "add them with the right (feature, sign) rather than deleting this "
        "assertion.")


def test_event_yield_has_no_keys_the_engine_never_applies():
    applied = _applied_effect_keys()
    extra = sorted(set(_EVENT_YIELD) - applied)
    assert not extra, (
        f"_EVENT_YIELD prices keys apply_gains never applies: {extra}.  "
        "Either the engine dropped a branch or this map has drifted.")


def test_lose_keys_are_priced_negative():
    """`loseScience` must not be a gain -- the reason this map is separate."""
    for key, (_fk, sign) in _EVENT_YIELD.items():
        if key.startswith("lose") or key.startswith("decrease"):
            assert sign == -1, f"{key} is priced with sign {sign}"
        elif key.startswith("gain") or key.startswith("increase"):
            assert sign == 1, f"{key} is priced with sign {sign}"


def test_my_seeds_are_mine_only():
    """The legality property the whole design rests on: no rival's seed."""
    st = _play(3, seed=5, steps=700)
    for idx in range(3):
        for name in my_seeds(st, idx):
            assert st.seeded_by.get(name) == idx, (
                f"my_seeds({idx}) returned {name!r}, seeded by "
                f"{st.seeded_by.get(name)} -- that is a peek at a face-down "
                "card, not memory.")


def test_seeded_pending_counts_only_unresolved():
    st = _play(3, seed=5, steps=700)
    live = set(st.current_events) | set(st.future_events)
    for idx in range(3):
        assert set(my_seeds(st, idx)) <= live
        assert not (set(my_seeds(st, idx)) & set(st.past_events)), \
            "a resolved event is not still owed to me"
        assert my_seeded_pending(st, idx) >= 0.0


def test_new_terms_are_dark_by_default():
    """A champion trained before these keys must evaluate bit-identically.

    Every new key defaults to 0.0, so `evaluate`'s `if wk:` skips it and the
    eval-only threat term is never called.  This is what lets the change land
    under running league arms without resetting a single champion.
    """
    st = _play(3, seed=7, steps=600)
    for k in ("my_seeded_pending", "my_event_threat", "rival_science_stock",
              "rival_food_stock", "rival_resource_stock", "rival_free_workers",
              "rival_yellow_bank", "rival_colonies", "rival_mil_actions",
              "rival_building_wonder"):
        assert DEFAULT_WEIGHTS[k] == 0.0, f"{k} is no longer dark by default"
    stripped = {k: v for k, v in DEFAULT_WEIGHTS.items()
                if not k.startswith("my_") and k not in (
                    "rival_science_stock", "rival_food_stock",
                    "rival_resource_stock", "rival_free_workers",
                    "rival_yellow_bank", "rival_colonies",
                    "rival_mil_actions", "rival_building_wonder")}
    assert evaluate(st, 0, DEFAULT_WEIGHTS) == evaluate(st, 0, stripped)


def test_threat_is_not_inert():
    """A term that is always zero has two causes; rule one of them out.

    Measured over 12 seeds x {2p,3p,4p} while this shipped: 83.5% of sampled
    positions had at least one of my own seeds pending, the threat was
    non-zero at 25.5% of them, and it ranged -11.8 to +14.8.  This test only
    pins that it fires at all, cheaply.

    Sampled MID-GAME on purpose.  The first cut of this test looked only at
    the finished position and read zero everywhere -- correctly, because by
    then every seed has resolved.  That is the "a rate of zero has two causes"
    trap in miniature: the term was fine and the measurement was standing in
    the wrong place.
    """
    w = dict(DEFAULT_WEIGHTS)
    w["my_event_threat"] = 1.0
    seen = 0
    for seed in (1, 4, 9):
        for st in _snapshots(3, seed=seed, steps=900, every=40):
            for idx in range(3):
                if my_event_threat(st, idx, w):
                    seen += 1
    assert seen, ("my_event_threat is zero everywhere -- either the pricing "
                  "broke or no ranked event was seeded in three whole games; "
                  "check which before relaxing this.")


def test_rival_board_reads_public_rival_fields():
    st = _play(3, seed=2, steps=600)
    f = features(st, 0, None, DEFAULT_WEIGHTS)
    rivals = [q for q in st.players if q.idx != 0 and not q.resigned]
    assert f["rival_science_stock"] == max(q.science for q in rivals)
    assert f["rival_food_stock"] == max(q.food for q in rivals)
    assert f["rival_resource_stock"] == max(q.resources for q in rivals)
    assert f["rival_colonies"] == max(len(q.colonies) for q in rivals)
    assert f["rival_building_wonder"] == sum(1 for q in rivals
                                             if q.wonder is not None)


def test_rival_board_is_zero_with_no_rivals():
    st = game.new_game(2, seed=0)
    for q in st.players:
        if q.idx != 0:
            q.resigned = True
    f = features(st, 0, None, DEFAULT_WEIGHTS)
    assert f["rival_science_stock"] == 0.0
    assert f["rival_building_wonder"] == 0.0


def test_targeting_terms_separate_two_targets():
    """The defect `tools/target_blindness.py` measured, pinned as a test.

    Hold the CARD fixed, vary only the TARGET, and the one-ply evaluator used
    to score both bit-identically: 74.3% of `war` groups, 76.6% of
    `aggression`, 50.2% of `offer_pact`, against `cancel_pact` at 0.0% as the
    control.  With the targeting weights lit that is 0.0% / 0.6% / 0.4% over
    8 seeds x {3p, 4p}; the survivors are rivals with genuinely equal culture
    and strength, which is a real tie rather than blindness.

    This test is the cheap version: find ONE such group and require the
    evaluator to separate it.
    """
    from engine import actions
    from engine.bots.fastcopy import copy_state

    w = dict(DEFAULT_WEIGHTS)
    w["attack_target_lead"] = 1.0
    w["attack_target_weakness"] = 0.5
    w["pact_partner_lead"] = 1.0

    checked = 0
    for seed in (0, 1, 2):
        for st in _snapshots(3, seed=seed, steps=900, every=1):
            idx = st.decider()
            groups = {}
            for mv in actions.legal_moves(st):
                pos = {"aggression": 2, "war": 2}.get(mv[0])
                if pos is not None and len(mv) > pos:
                    groups.setdefault((mv[0],) + tuple(mv[1:pos]), []).append(mv)
            for group in groups.values():
                if len(group) < 2:
                    continue
                vals = []
                for mv in group:
                    trial = copy_state(st)
                    try:
                        game.apply(trial, mv, random.Random(1))
                    except Exception:                          # noqa: BLE001
                        continue
                    vals.append(evaluate(trial, idx, w))
                if len(vals) < 2:
                    continue
                tgts = [st.players[mv[2]] for mv in group]
                if len({(q.culture) for q in tgts}) < 2:
                    continue          # a genuine tie, not evidence either way
                checked += 1
                assert max(vals) - min(vals) != 0.0, (
                    f"{group} scored identically at {vals} despite the "
                    "targets having different culture -- the evaluator is "
                    "blind to WHICH opponent it is attacking again.")
                if checked >= 3:
                    return
    assert checked, ("no attack with two distinguishable targets occurred in "
                     "three games -- this test proved nothing; widen it "
                     "rather than deleting it.")


def _snapshots(n, seed, steps, every):
    """Yield the live state every `every` plies -- the same object each time,
    so callers must read it before advancing (they all do)."""
    st = game.new_game(n, seed=seed)
    bots = [WeightedBot(DEFAULT_WEIGHTS, seed=seed * 7 + i) for i in range(n)]
    rng = random.Random(seed)
    for k in range(steps):
        if st.game_over:
            break
        try:
            game.apply(st, bots[st.decider()](st), rng)
        except Exception:                                      # noqa: BLE001
            break
        if k % every == every - 1:
            yield st


def _play(n, seed, steps):
    st = game.new_game(n, seed=seed)
    bots = [WeightedBot(DEFAULT_WEIGHTS, seed=seed * 7 + i) for i in range(n)]
    rng = random.Random(seed)
    for _ in range(steps):
        if st.game_over:
            break
        try:
            game.apply(st, bots[st.decider()](st), rng)
        except Exception:                                      # noqa: BLE001
            break
    return st
