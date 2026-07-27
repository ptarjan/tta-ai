"""Desync detection: make a drifting mirror fail loudly, not silently.

`docs/EXTERNAL_AIS.md` section 6d names this as the failure mode that makes the
whole exercise worthless: "a 90-minute game can be entirely worthless and
nobody notices".  A mirror that has quietly diverged from the app is not a
degraded measurement, it is a *fabricated* one -- the bot was asked to move in
a position that never existed.

The asymmetry that makes this tractable:

* **Our own board is SIMULATED.**  Every one of our moves goes through the real
  engine, so the mirror *predicts* our culture, science, strength, food,
  resources, actions, hand sizes and banks.  A prediction can be checked, and
  the app prints all of it on one panel.  These are the real checksums.
* **Rival boards are FORCED.**  We never replay a rival's turn; the human types
  a handful of numbers off the app's player panel and `state_io._force`
  back-solves the mirror to match.  A forced value can never disagree with
  itself, so it is *not* a checksum.  The only cross-check available there is
  arithmetic consistency over time, which is a warning, not a proof.

There is now one exception, and it is worth its keystroke: **completed wonders
are DERIVED, not forced.**  Their names arrive as they happen (`p1 built+
Colossus`) because a name carries effects a count cannot, so the mirror holds
an opinion about how many each rival has -- and an opinion can be checked
against the number on the panel.  That makes `w` the first and only hard rival
check in this file: it catches the one rival-side failure that is otherwise
permanent, an operator who missed a `built+` and never finds out.

So: everything in `SELF_CHECKS` and `RIVAL_CHECKS` is a hard failure when it
disagrees, and `rival_consistency` is a soft warning.  Nothing here ever "fixes
up" the mirror on its own -- an automatic repair is exactly how a silent
corruption survives.
"""

from __future__ import annotations

from dataclasses import dataclass, field

from engine import economy, effects

FAIL = "fail"      # the mirror is provably wrong.  Stop.
WARN = "warn"      # suspicious, not proof.  Show it, let the operator judge.


@dataclass(frozen=True)
class Check:
    key: str            # what the operator types: "c=41"
    label: str          # where to look in the app
    get: callable       # (state, player) -> int
    spine: bool = False  # part of the fast positional form


def _stats(state, p):
    effects.invalidate(state, p)
    return effects.compute(state, p)


SELF_CHECKS = [
    Check("c", "culture (your score)", lambda st, p: p.culture, True),
    Check("s", "science", lambda st, p: p.science, True),
    Check("str", "military strength", lambda st, p: _stats(st, p).strength, True),
    Check("f", "food in store", lambda st, p: p.food, True),
    Check("r", "resources in store", lambda st, p: p.resources, True),
    Check("cr", "culture per turn", lambda st, p: _stats(st, p).culture),
    Check("sr", "science per turn", lambda st, p: _stats(st, p).science),
    Check("hap", "happy faces", lambda st, p: _stats(st, p).happy),
    Check("ca", "civil actions available (the total, not what is left)",
          lambda st, p: _stats(st, p).civil_actions),
    Check("ma", "military actions available",
          lambda st, p: _stats(st, p).military_actions),
    Check("fw", "unused (yellow) workers", lambda st, p: p.workers_free),
    Check("yel", "yellow bank", lambda st, p: p.yellow_bank),
    Check("blue", "blue bank tokens", lambda st, p: effects.blue_available(p)),
    Check("hc", "civil cards in your hand", lambda st, p: len(p.hand_civil)),
    Check("hm", "military cards in your hand",
          lambda st, p: len(p.hand_military)),
]
SELF_BY_KEY = {c.key: c for c in SELF_CHECKS}
SPINE = [c.key for c in SELF_CHECKS if c.spine]

# board-wide checks: cheap, and they catch the worst class of drift (the mirror
# sitting in a different round or age from the app, which invalidates every
# horizon-scaled weight at once)
BOARD_CHECKS = ["round", "age", "row"]


@dataclass(frozen=True)
class Discrepancy:
    key: str
    expected: int | str
    reported: int | str
    severity: str
    where: str = ""

    def __str__(self):
        who = f"{self.where} " if self.where else ""
        return (f"{who}{self.key}: mirror says {self.expected}, "
                f"you read {self.reported}")


# ------------------------------------------------------------- self checks


def self_snapshot(board, keys=None):
    """What the mirror predicts the app is showing on our own panel."""
    st, p = board.state, board.state.players[board.me]
    keys = keys or [c.key for c in SELF_CHECKS]
    return {k: SELF_BY_KEY[k].get(st, p) for k in keys if k in SELF_BY_KEY}


def board_snapshot(board):
    st = board.state
    return {"round": st.round, "age": st.age_civil,
            "row": sum(1 for c in st.card_row if c)}


def compare(expected, reported, severity=FAIL, where=""):
    """Every key present in BOTH dicts that disagrees.

    Absent keys are not silently passed as OK -- they are simply not checked,
    and `harness.play` refuses to advance a round until the spine is present.
    """
    out = []
    for k, want in expected.items():
        if k not in reported or reported[k] is None:
            continue
        got = reported[k]
        if isinstance(want, str) or isinstance(got, str):
            same = str(want).strip().upper() == str(got).strip().upper()
        else:
            same = int(want) == int(got)
        if not same:
            out.append(Discrepancy(k, want, got, severity, where))
    return out


def check_self(board, reported):
    return compare(self_snapshot(board), reported, FAIL, f"p{board.me}")


def check_board(board, reported):
    return compare(board_snapshot(board), reported, FAIL, "board")


# ------------------------------------------------------------ rival checks


@dataclass
class RivalHistory:
    """Arithmetic consistency for values we can only ever be *told*.

    We force a rival's culture, so the mirror cannot contradict it.  But we
    were also told their culture *rate* last round, and culture only moves by
    production plus card/event effects.  A report that violates
    ``c_now >= c_prev + rate_prev - slack`` is usually a misread panel or a
    transposed digit, caught for free.
    """

    slack: int = 8
    seen: dict = field(default_factory=dict)   # idx -> (round, culture, rate)

    def check(self, idx, rnd, culture, rate):
        out = []
        prev = self.seen.get(idx)
        if prev and culture is not None:
            p_rnd, p_c, p_rate = prev
            turns = max(0, rnd - p_rnd)
            if p_rate is not None and turns:
                lo = p_c + p_rate * turns - self.slack
                hi = p_c + p_rate * turns + 6 * turns + self.slack
                if not lo <= culture <= hi:
                    out.append(Discrepancy(
                        "culture", f"{lo}..{hi} (from +{p_rate}/turn at "
                                   f"round {p_rnd})", culture, WARN, f"p{idx}"))
        if culture is not None:
            self.seen[idx] = (rnd, culture, rate)
        return out


# ------------------------------------------------------------- the parser


def parse_line(text, spine=None, allowed=None):
    """Parse a check line.  Two forms, both accepted on one line.

    Fast positional spine, in `SPINE` order::

        41/12/9/3/5

    Explicit keys, any order, any subset::

        c=41 s=12 str=9 age=II row=13

    Returns (values, errors).  A key we do not know is an error, never a
    silently dropped field.
    """
    spine = spine or SPINE
    allowed = allowed or (set(SELF_BY_KEY) | set(BOARD_CHECKS))
    vals, errs = {}, []
    for tok in text.replace(",", " ").split():
        if "=" in tok:
            k, _, v = tok.partition("=")
            k = k.strip().lower()
            v = v.strip().split("/")[0]      # accept "ca=3/4"
            if k not in allowed:
                errs.append(f"unknown check field {k!r}")
                continue
            if k == "age":
                vals[k] = v.upper()
                continue
            try:
                vals[k] = int(v)
            except ValueError:
                errs.append(f"{k}: {v!r} is not a number")
            continue
        if "/" in tok:
            parts = tok.split("/")
            if len(parts) > len(spine):
                errs.append(f"the spine is {'/'.join(spine)} "
                            f"({len(spine)} numbers), you gave {len(parts)}")
                continue
            for key, part in zip(spine, parts):
                part = part.strip()
                if part in ("", "?"):
                    continue
                try:
                    vals[key] = int(part)
                except ValueError:
                    errs.append(f"{key}: {part!r} is not a number")
            continue
        errs.append(f"cannot read {tok!r} -- use 41/12/9/3/5 or c=41")
    return vals, errs


def parse_rival_line(text):
    """``p1 c=41 cr=5 sr=3 str=12`` -> (idx, {key: value}, errors).

    Accepts the bare positional form too: ``p1 41/5/3/12/4/3/1``, and any
    prefix of it -- a trailing field left off is simply not reported, which
    for a forced key means "unchanged" and for a checked key means "not
    checked this round".
    """
    toks = text.split()
    if not toks:
        return None, {}, ["empty"]
    who = toks[0].lower().lstrip("p")
    try:
        idx = int(who)
    except ValueError:
        return None, {}, [f"{toks[0]!r} is not a player like 'p1'"]
    vals, errs = parse_line(" ".join(toks[1:]),
                            spine=list(RIVAL_ASK_KEYS),
                            allowed=set(RIVAL_ASK_KEYS))
    return idx, vals, errs


# ------------------------------------------------------------ the verdict


@dataclass
class CheckResult:
    round: int
    reported: dict
    discrepancies: list

    @property
    def failed(self):
        return any(d.severity == FAIL for d in self.discrepancies)

    @property
    def warned(self):
        return any(d.severity == WARN for d in self.discrepancies)

    def to_json(self):
        return {"round": self.round, "reported": self.reported,
                "discrepancies": [
                    {"key": d.key, "expected": d.expected,
                     "reported": d.reported, "severity": d.severity,
                     "where": d.where} for d in self.discrepancies]}


def missing_spine(reported, spine=None):
    """Spine keys the operator did not supply.  A round is not verified
    without them; `harness.play` refuses to move on."""
    spine = spine or SPINE
    return [k for k in spine if reported.get(k) is None]


def round_check(board, reported, history=None, rivals=None):
    """Full per-round verification.  `rivals` is {idx: {key: value}}.

    Called BEFORE the forced rival values are applied, which is the only order
    that makes `check_rivals` mean anything: after the patches the mirror
    agrees with the operator by construction.
    """
    ds = (check_self(board, reported) + check_board(board, reported)
          + check_rivals(board, rivals))
    if history is not None:
        for idx, vals in (rivals or {}).items():
            ds += history.check(idx, board.state.round,
                                vals.get("c"), vals.get("cr"))
    return CheckResult(board.state.round, dict(reported), ds)


# ------------------------------- the claim the cost estimate rests on ------
#
# Every rival-derived feature `weighted.features()` produces, and the inputs
# the app prints that pin each of them down.  `tests/test_harness_mirror.py`
# asserts the two still line up, by wrecking every rival board and rebuilding
# it from nothing but what the operator types.  That test is the tripwire on a
# moving target, and it has already fired once: the GAP-3 work added
# `rival_free_ca` / `rival_hand_civil` / `rival_wonders`, and the four numbers
# we asked for could not reconstruct any of the three.
#
# The rule when it fires again is NOT "append the new name here".  It is:
#   1. is the quantity public at the table?  (If it is not, the feature is
#      cheating and the evaluator is the thing that has to change.)
#   2. can `state_io` apply it to a rival we never mirror?
#   3. does the wreck-and-restore test below still pass through that path?
# Only then does the name belong in this tuple.

RIVAL_FEATURE_KEYS = ("rival_culture", "rival_mean_culture",
                      "rival_culture_rate", "rival_science_rate",
                      "rival_strength", "rival_free_ca",
                      "rival_hand_civil", "rival_wonders")

# What the operator reads off one rival's panel, in prompt order.  All seven
# are public information in Through the Ages: culture and the two rates are
# printed on the panel, strength is printed on the panel, action tokens and
# completed wonders sit face up in the player's area, and civil cards are only
# ever taken from an open row in full view (RULES_SPEC.md 2.6).  Nothing here
# asks the human to look at a hidden zone.
RIVAL_ASK_KEYS = ("c", "cr", "sr", "str", "ca", "hc", "w")

# ...of which these are pushed into the mirror as `state_io` patch lines.
# `c/cr/sr/str` back-solve through `_force`; `ca` sets the civil-action
# counter; `hc` sets the count of cards in hand we cannot name (which is what
# `rival_hand_civil` reads -- see PlayerState.hand_size).
RIVAL_FORCE_KEYS = ("c", "cr", "sr", "str", "ca", "hc")

# ...and this one is CHECKED instead.  A wonder's name carries its effects, so
# the mirror learns wonders by name (`p1 built+ Colossus`) as they are built;
# the count on the panel then verifies we did not miss one.
RIVAL_CHECKS = [
    Check("w", "completed wonders (face up in their area)",
          lambda st, q: len(q.completed_wonders)),
]
RIVAL_CHECK_BY_KEY = {c.key: c for c in RIVAL_CHECKS}

# Each asked field's probe in `harness.fields`, so the static ask above and the
# derived field list can be held against each other -- and so the operator can
# be told, at startup, which of the seven the bot does not actually read today.
# `tests/test_harness_mirror.py` asserts every key here names a real probe: a
# field we ask for with no probe behind it is a field that can quietly stop
# mattering (or quietly start) and nobody would ever find out.
RIVAL_PROBE_IDS = {
    "c": "rival.culture",
    "cr": "rival.culture_rate",
    "sr": "rival.science_rate",
    "str": "rival.strength",
    "ca": "rival.civil_actions",
    "hc": "rival.hand_civil_size",
    "w": "rival.wonders",
}

# One line of operator-facing help per asked field, so `harness.play` and
# `docs/APP_HARNESS.md` cannot drift apart from this file.
RIVAL_LABELS = {
    "c": "culture (their score)",
    "cr": "culture per turn",
    "sr": "science per turn",
    "str": "military strength",
    "ca": "civil actions (their total -- between turns it is all of them)",
    "hc": "civil cards in hand",
    "w": "completed wonders",
}


def rival_snapshot(board, idx):
    """What the mirror believes about the CHECKED rival quantities."""
    q = board.state.players[idx]
    return {c.key: c.get(board.state, q) for c in RIVAL_CHECKS}


def check_rivals(board, rivals):
    """Hard checks on rival values the mirror DERIVES rather than forces.

    `rivals` is {idx: {key: value}}.  A key the operator left blank is not
    checked -- same rule as everywhere else here, absence is never agreement.
    """
    out = []
    for idx, vals in (rivals or {}).items():
        if not 0 <= idx < len(board.state.players):
            continue
        out += compare(rival_snapshot(board, idx), vals, FAIL, f"p{idx}")
    return out
