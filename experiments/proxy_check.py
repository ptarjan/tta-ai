"""THE PROXY GUARDRAIL: is the number the league climbs the number we ship?

    python3 -m experiments.proxy_check --players 2
    python3 -m experiments.proxy_check --report          # read the series back

Why this exists
---------------
The league accepts a champion when it beats its parent on a paired score
measured under the TRAINING architecture (`--candidate-bot`).  That is a proxy
for the thing we care about, which is how the vector plays under the policy we
would actually ship (`plan:width=8`).  The two have already come apart once,
badly, and nothing in the loop noticed:

* `docs/TRANSFER_TEST.md` -- the quiescent-trained vector Q is +36.3 +/- 4.8
  margin better than the 1-ply-trained vector P under the training proxy, and
  **-32.5 +/- 6.9 worse under `plan:width=8`**.  The proxy did not merely
  mis-state the size of the improvement; it got the SIGN wrong.
* `docs/PLAN_WAR_LOOKAHEAD.md` -- giving PlanBot a war lookahead removed the
  inversion.  It did not make the proxy predictive: it now says +36.3 +/- 4.8
  where the ship policy says **+1.4 +/- 5.3 at 52.2% +/- 3.7%**, i.e. a null.
  "Actively wrong" became "uninformative about magnitude".

Both of those were one-off measurements by a human-driven agent on frozen
vectors.  Neither is a monitor.  So the arms can climb their proxy for two
days, accept a hundred champions, and there is no artefact anywhere that says
whether any of it reached the ship policy.  This module is that artefact.

What it measures
----------------
Every `--every-accepts` accepted champions (or `--max-hours` since the last
reading, whichever comes first -- an arm that accepts slowly still produces a
time series), for one arm:

1. **head to head under the ship policy**: the newly accepted champion against
   the PREVIOUSLY VALIDATED champion, both played by `--policy` (default
   `plan:width=8`), seat-rotated on the same deals.  Null is 1/players.
2. **an absolute anchor**: the same new champion against `book` under the same
   policy, reported as OWN FINAL CULTURE.  The head-to-head chain is relative
   and a chain can drift; own culture against a fixed external opponent is
   comparable across the whole series and against the numbers written down in
   `docs/HUMAN_BASELINE.md` (human 2p median 159.5) and
   `docs/PLAN_WAR_LOOKAHEAD.md` 4a (P 213.4, Q 127.8 under `plan:width=8`).

and appends one record to `proxy_history_{K}p.jsonl` plus one legible block to
`experiments/logs/proxy_check.log`.

How to read the output
----------------------
Each reading gets a VERDICT from the head-to-head CULTURE MARGIN and its 95%
CI (see `verdict_of` for why margin and not win share):

    confirms      lower bound above +MARGIN_MIN.  A real gain, resolved.
    INVERTED      upper bound below -MARGIN_MIN.  A real LOSS: the champion
                  the proxy chose is worse under the ship policy than the one
                  it replaced.  The `docs/TRANSFER_TEST.md` failure, live.
    flat          the CI is tighter than MARGIN_RESOLUTION and covers the
                  no-effect band.  Measured, and there is nothing there.
    inconclusive  the CI is wider than MARGIN_RESOLUTION.  **NOT MEASURED.**
                  Not reassurance, not a divergence -- an instrument problem.

The fourth verdict is the one that keeps this file honest.  Without it the
first real reading printed `confirms` off a win-share lower bound of 50.03%
against a 50% null: a coin flip reported as reassurance, which is
`docs/HAZARDS.md` trap 1 committed by the very thing meant to catch it.

Two separate alarms, because they need different responses:

    !! PROXY DIVERGENCE      an INVERTED reading, or `--divergence-run`
                             consecutive RESOLVED readings with no gain while
                             accepts pile up.  The training loop is the
                             problem.
    !! GUARDRAIL NOT RESOLVING   `--divergence-run` consecutive inconclusive
                             readings.  The guardrail is the problem: raise
                             --deals and --every-accepts together.
    !! PROXY GUARDRAIL STARVED   --stale-hours have passed with accepted
                             champions and no successful reading at all.

Cost, and why it cannot hurt the arms
-------------------------------------
* It runs from `experiments/proxy_watch.sh` (cron), never from inside the
  trainer, so it cannot slow, block or crash an arm.
* It reads only LADDER files, which are written once when a champion is
  accepted and never rewritten -- so there is no torn-read race with a live
  arm, unlike `champion_{K}p.json` which is rewritten every generation.
* `nice -n 19`, `--workers 1`, and n bounded by `--deals` / `--anchor-deals`.
  The defaults are sized per player count so one reading costs a few percent
  of the arm's throughput between readings; `--dry-run` prints the estimate.
* A lock file means two arms never measure at once.  It WAITS for the lock
  rather than skipping, because a neighbouring job that holds it whenever
  cron looks would otherwise starve the guardrail in silence.
"""
from __future__ import annotations

import argparse
import errno
import json
import os
import sys
import time

os.environ.setdefault("TTA_JOURNAL", "1")

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

from engine.bots.weighted import load_weights            # noqa: E402
from experiments import arena                            # noqa: E402
from experiments import hillclimb_league as L            # noqa: E402
from experiments import hillclimb_pool as P              # noqa: E402 (make_bot)

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)
DEFAULT_STATE = os.path.join(HERE, "league_state")
LOG = os.path.join(HERE, "logs", "proxy_check.log")
LOCK = os.path.join(HERE, "logs", "proxy_check.lock")

#: Ship policy.  `plan:width=8` is what `docs/BOT_ARCHITECTURE.md`,
#: `docs/TRANSFER_TEST.md` and `docs/PLAN_WAR_LOOKAHEAD.md` all measure, so a
#: reading here is directly comparable to every number written down there.
SHIP_POLICY = "plan:width=8"

#: Per-player-count budget.  A deal is one seed played from every seat, so a
#: reading is `deals * players` head-to-head games plus `anchor_deals *
#: players` anchor games.  These fall with player count because the cost of a
#: PlanBot game rises steeply with it (docs/TRAINING_RUN.md: 9.1 / ~12 / 17.4
#: cpu-s per game against `book`, 15.8 / ~30 / 51.3 with every seat searching),
#: while the arms' generations get *slower*, so the ratio of guardrail cost to
#: arm throughput stays in the same few percent.
#:
#: THE FIRST SIZING WAS TOO SMALL AND THE VERDICT RULE HID IT.  20 deals at 2p
#: gave a win-share half-width of 15.0 points, and the first reading came back
#: 65.0% +/- 15.0% -- a lower bound sitting exactly on the null, reported as
#: `confirms`.  On a project that has been burned repeatedly by small-n
#: over-reading, a guardrail whose reassuring verdict is a coin flip is worse
#: than no guardrail.  Two things changed together: `n` roughly doubled AND the
#: cadence went sparse to pay for it, so the cost per unit time is unchanged
#: and each reading is worth acting on.  See `verdict_of` for the other half.
BUDGET = {
    2: {"deals": 40, "anchor_deals": 15, "every_accepts": 12, "max_hours": 12.0},
    3: {"deals": 20, "anchor_deals": 8, "every_accepts": 20, "max_hours": 16.0},
    4: {"deals": 12, "anchor_deals": 5, "every_accepts": 15, "max_hours": 20.0},
}

#: A reading must clear the null by this much in CULTURE MARGIN before it may
#: say `confirms` (or fall this far below it to say `INVERTED`).  Culture, not
#: win share: win share is a 0/1 step with ~10x the paired variance, it
#: saturates against `book` at 0.94-0.97 under PlanBot (docs/TRANSFER_TEST.md
#: 3), and every finding in TRANSFER_TEST / PLAN_WAR_LOOKAHEAD is quoted in
#: margin for exactly that reason.  5 culture points is small against the
#: differences those documents measure (+98.8, -32.5, +11.4) and large against
#: nothing.
MARGIN_MIN = 5.0

#: A reading may only say `flat` -- "measured, and there is no effect" -- if it
#: could have SEEN an effect of this size.  A wider CI than this is
#: `inconclusive`, which is a different statement and must not read as
#: reassurance.
MARGIN_RESOLUTION = 15.0

#: Seeds are FIXED across readings on purpose: every reading plays the same
#: deals, so two readings differ because the champions differ and not because
#: the shuffle did.  They are also far from the trainer's own seed base
#: (`gen * 1_000_003 + seed * 7717`), so the guardrail is not scored on games
#: the arm has already fitted to.
H2H_SEED = 5150
ANCHOR_SEED = 90210


# ------------------------------------------------------------------- files

def ladder_dir(state_dir, players):
    return os.path.join(state_dir, f"ladder_{players}p")


def history_path(state_dir, players):
    return os.path.join(state_dir, f"proxy_history_{players}p.jsonl")


def ladder_files(state_dir, players):
    """Accepted champions, oldest first.  Immutable once written."""
    d = ladder_dir(state_dir, players)
    if not os.path.isdir(d):
        return []
    return [os.path.join(d, fn) for fn in sorted(os.listdir(d))
            if fn.startswith("gen") and fn.endswith(".json")]


def gen_of(path):
    try:
        return int(os.path.basename(path)[3:-5])
    except ValueError:
        return -1


def read_history(state_dir, players):
    p = history_path(state_dir, players)
    out = []
    if os.path.exists(p):
        with open(p) as fh:
            for line in fh:
                line = line.strip()
                if not line:
                    continue
                try:
                    out.append(json.loads(line))
                except ValueError:
                    continue          # a torn last line is not worth dying for
    return out


def accepted_edges(state_dir, players, lo_gen, hi_gen):
    """The proxy's own claim: accepted edges in (lo_gen, hi_gen].

    Read with a substring filter before `json.loads` because the generation
    log carries the full per-opponent table on every accepted row and is a few
    MB; this keeps the guardrail's own cost in the milliseconds.
    """
    path = os.path.join(state_dir, f"generations_{players}p.jsonl")
    edges = []
    if not os.path.exists(path):
        return edges
    with open(path) as fh:
        for line in fh:
            if '"accepted": true' not in line:
                continue
            try:
                r = json.loads(line)
            except ValueError:
                continue
            if lo_gen < r.get("gen", -1) <= hi_gen and r.get("edge") is not None:
                edges.append(float(r["edge"]))
    return edges


# -------------------------------------------------------------------- lock

class Lock:
    """One guardrail measurement at a time, box-wide.

    A stale lock (holder killed mid-run) is stolen after `stale_h` hours
    rather than wedging the guardrail forever -- the failure mode of a monitor
    that silently stops monitoring is the one thing this module cannot have.
    """

    def __init__(self, path=LOCK, stale_h=6.0, wait_s=900.0, poll_s=20.0):
        self.path, self.stale_h, self.fd = path, stale_h, None
        self.wait_s, self.poll_s, self.waited = wait_s, poll_s, 0.0

    def _try(self):
        try:
            self.fd = os.open(self.path, os.O_CREAT | os.O_EXCL | os.O_WRONLY)
            return True
        except OSError as exc:
            if exc.errno != errno.EEXIST:
                raise
            return False

    def __enter__(self):
        os.makedirs(os.path.dirname(self.path), exist_ok=True)
        # WAIT, do not skip.  The first version returned immediately when the
        # lock was held, and a neighbouring measurement job held it every time
        # cron looked -- so proxy_watch.log filled with "another measurement
        # holds the lock, skipping" and the guardrail never ran at all.
        # Skip-and-forget turns a busy box into a silent monitor.
        t0 = time.time()
        while not self._try():
            age = time.time() - os.path.getmtime(self.path)
            if age >= self.stale_h * 3600:      # holder died mid-run
                os.unlink(self.path)
                continue
            if time.time() - t0 >= self.wait_s:
                self.waited = time.time() - t0
                return None
            time.sleep(self.poll_s)
        self.waited = time.time() - t0
        os.write(self.fd, f"{os.getpid()} {time.strftime('%F %T')}\n".encode())
        return self

    def __exit__(self, *exc):
        if self.fd is not None:
            os.close(self.fd)
            try:
                os.unlink(self.path)
            except OSError:
                pass
        return False


# ------------------------------------------------------------------ duels

def _lock_holder():
    """Who holds the lock, for the message -- never raises."""
    try:
        with open(LOCK) as fh:
            return fh.read().strip() or "unknown"
    except OSError:
        return "released just now"


def spec_for(path, policy):
    """A ladder file -> the arena spec that plays it under the ship policy."""
    L.CANDIDATE_ARCH = L.parse_candidate_bot(policy)
    return L.as_spec(load_weights(path))


def measure(new_path, base_path, players, policy, deals, anchor_deals,
            workers=1):
    out = {}
    new_spec = spec_for(new_path, policy)
    base_spec = spec_for(base_path, policy)
    t0 = time.time()
    h = arena.duel(new_spec, base_spec, players, deals * players,
                   seed0=H2H_SEED, workers=workers)
    # The margin's own CI, which `arena.duel` does not return -- it reports the
    # mean margin but a CI only for the win share.  The verdict is a threshold
    # on this, so it has to be computed rather than eyeballed.
    mm, mci = arena.mean_ci([x for x in (h.get("per_game_margin") or [])
                             if x is not None])
    out["h2h"] = {"win_rate": h["win_rate"], "ci": h["ci"], "null": h["null"],
                  "margin": h.get("margin"), "margin_ci": mci,
                  "culture": h.get("culture_a"),
                  "opp_culture": h.get("culture_b"), "games": h["games"],
                  "deals": deals, "secs": round(time.time() - t0, 1)}
    del mm
    if anchor_deals > 0:
        t1 = time.time()
        a = arena.duel(new_spec, "book", players, anchor_deals * players,
                       seed0=ANCHOR_SEED, workers=workers)
        out["anchor"] = {"opponent": "book", "win_rate": a["win_rate"],
                         "ci": a["ci"], "margin": a.get("margin"),
                         "culture": a.get("culture_a"),
                         "opp_culture": a.get("culture_b"),
                         "games": a["games"], "deals": anchor_deals,
                         "secs": round(time.time() - t1, 1)}
    return out


def verdict_of(h2h, margin_min=MARGIN_MIN, resolution=MARGIN_RESOLUTION):
    """FOUR verdicts, on the paired CULTURE MARGIN, not on win share.

        confirms      lower bound above +margin_min: a real gain, resolved
        INVERTED      upper bound below -margin_min: a real LOSS, resolved
        flat          the CI is tighter than `resolution` and covers the
                      no-effect band: measured, and there is nothing there
        inconclusive  the CI is wider than `resolution`: NOT MEASURED

    The fourth one is the important one and the first version of this file did
    not have it.  Its first reading came back at a win-share lower bound of
    50.03% against a 50% null and printed `confirms` -- a coin flip that landed
    right, reported as reassurance.  `docs/HAZARDS.md` trap 1 is the same
    mistake in the full check (an n=48 row read 50.0% where n=400 said 27.6%),
    and a guardrail that repeats the error it exists to catch is worthless.

    `inconclusive` is deliberately NOT a divergence -- it is a statement about
    the instrument, and it drives its own separate loud line (see
    `divergence`), because "we are not measuring anything" and "the proxy has
    decoupled" need different responses.
    """
    m = h2h.get("margin")
    ci = h2h.get("margin_ci")
    if m is None or ci is None:
        return "inconclusive"
    if m - ci > margin_min:
        return "confirms"
    if m + ci < -margin_min:
        return "INVERTED"
    if ci <= resolution:
        return "flat"
    return "inconclusive"


# ------------------------------------------------------------------ report

def format_history(hist, players):
    lines = [f"    {'at':<17}{'champ':>7}{'base':>7}{'acc':>5}"
             f"{'margin':>9}{'+/-':>7}{'win%':>7}{'own cult':>10}"
             f"{'vs book':>9}  verdict"]
    for r in hist:
        h = r.get("h2h") or {}
        a = r.get("anchor") or {}
        lines.append(
            f"    {r.get('at', '?')[:16]:<17}{r.get('champion_gen', -1):>7}"
            f"{r.get('baseline_gen', -1):>7}{r.get('accepts_between', 0):>5}"
            f"{(h.get('margin') or 0):>+9.1f}"
            f"{(h.get('margin_ci') or 0):>7.1f}"
            f"{(h.get('win_rate') or 0):>7.1%}"
            f"{(h.get('culture') or 0):>10.1f}"
            f"{(a.get('culture') or 0):>9.1f}"
            f"  {r.get('verdict', '?')}")
    return "\n".join(lines)


def divergence(hist, run=3):
    """(is_diverging, message).  The loud half of the guardrail.

    The proxy only ever claims progress -- an accept requires a positive lower
    bound on its own metric, so `accepts_between > 0` IS a claim.  So the
    question is never "did the proxy claim something", it is "did the ship
    policy ever agree".

    `inconclusive` readings do NOT count toward a divergence.  They are the
    instrument failing, not the proxy failing, and conflating the two is how a
    guardrail ends up crying wolf about the training loop when the real fault
    is its own sample size.  They get their own alarm in `starvation`.
    """
    if not hist:
        return False, ""
    last = hist[-1]
    if last.get("verdict") == "INVERTED":
        return True, ("the champion the proxy just accepted is WORSE under "
                      "the ship policy than the one it replaced -- this is "
                      "docs/TRANSFER_TEST.md's failure mode, live")
    tail = [r for r in hist if r.get("verdict") in ("confirms", "flat")][-run:]
    if len(tail) >= run and all(r.get("verdict") == "flat" for r in tail):
        acc = sum(r.get("accepts_between", 0) for r in tail)
        return True, (f"{run} consecutive RESOLVED readings with no gain "
                      f"under the ship policy, across {acc} accepted "
                      f"champions -- the proxy is claiming progress the "
                      f"policy we would ship cannot see")
    return False, ""


def unresolved(hist, run=3):
    """(is_unresolved, message).  The guardrail complaining about ITSELF.

    A run of `inconclusive` readings means the instrument cannot see an effect
    of the size that matters at the n it is being given.  That is not
    reassurance and it is not a divergence: it is a request to raise --deals
    (and, to pay for it, --every-accepts).
    """
    tail = hist[-run:]
    if len(tail) >= run and all(r.get("verdict") == "inconclusive"
                                for r in tail):
        ci = [((r.get("h2h") or {}).get("margin_ci") or 0.0) for r in tail]
        return True, (f"{run} consecutive readings too wide to resolve "
                      f"+/-{MARGIN_RESOLUTION:g} culture (half-widths "
                      + ", ".join(f"{c:.1f}" for c in ci)
                      + ") -- the guardrail is not measuring anything at this "
                        "sample size; raise --deals and --every-accepts "
                        "together so the cost per hour is unchanged")
    return False, ""


def starvation(hist, newest_gen, accepts_pending, hours_since, stale_hours):
    """(is_starved, message).  A monitor that stops monitoring, loudly.

    The failure this exists for is mundane and real: the lock was held by
    another job every time cron looked, so `proxy_watch.log` filled up with
    "another measurement holds the lock, skipping" and nothing was ever
    measured.  Silence from a monitor is indistinguishable from good news,
    which is the one thing a monitor may never be.
    """
    if accepts_pending < 1 or hours_since < stale_hours:
        return False, ""
    last = f"gen {hist[-1]['champion_gen']}" if hist else "NEVER"
    return True, (f"no successful reading for {hours_since:.1f}h (last: "
                  f"{last}) while {accepts_pending} champions have been "
                  f"accepted, newest gen {newest_gen}.  The arm is training "
                  f"unvalidated.  Check experiments/logs/proxy_watch.log for "
                  f"lock contention and the box for a competing job")


def log_block(fh, players, rec, hist, policy, run=3):
    def w(line=""):
        fh.write(line + "\n")

    h, a = rec.get("h2h") or {}, rec.get("anchor") or {}
    w("")
    w("=" * 78)
    w(f"[{players}p] PROXY CHECK {rec['at']}  policy={policy}  "
      f"gen {rec['baseline_gen']} -> {rec['champion_gen']} "
      f"({rec['accepts_between']} accepts, {rec['gens_between']} generations)")
    w(f"  proxy claim   : {rec['accepts_between']} accepted champions, "
      f"summed accepted edge {rec['proxy_edge_sum']:+.4f} "
      f"(training metric, {rec['objective_note']})")
    # The bounds are printed explicitly because the verdict is a threshold on
    # them.  A verdict that cannot be SEEN to be marginal is a verdict that
    # gets over-quoted, and this project has been burned by exactly that.
    # `ci` is arena's 95% two-sided half-width (z=1.96).
    mg, mci = h.get("margin") or 0.0, h.get("margin_ci") or 0.0
    w(f"  ship policy   : culture margin {mg:+.1f} +/- {mci:.1f} "
      f"(95% CI [{mg - mci:+.1f}, {mg + mci:+.1f}], "
      f"needs {MARGIN_MIN:+g} to confirm, "
      f"+/-{MARGIN_RESOLUTION:g} to resolve)")
    w(f"                  own culture {h.get('culture', 0):.1f} vs "
      f"{h.get('opp_culture', 0):.1f}; win share "
      f"{h.get('win_rate', 0):.1%} +/- {h.get('ci', 0):.1%} vs null "
      f"{h.get('null', 0):.1%} (secondary: a 0/1 step with ~10x the variance)")
    w(f"                  over {h.get('games', 0)} games "
      f"({h.get('deals', 0)} deals, {h.get('secs', 0):.0f}s)")
    if a:
        w(f"  anchor vs book: own culture {a.get('culture', 0):.1f} "
          f"(book {a.get('opp_culture', 0):.1f}), win share "
          f"{a.get('win_rate', 0):.1%} +/- {a.get('ci', 0):.1%}, "
          f"n={a.get('games', 0)}")
    w(f"  VERDICT       : {rec['verdict']}"
      + ("   (NOT MEASURED -- the CI is too wide to call, do not read this "
         "as reassurance)" if rec["verdict"] == "inconclusive" else ""))
    diverging, why = divergence(hist, run)
    if diverging:
        w("")
        w("  !! PROXY DIVERGENCE !!")
        w(f"  !! {why}")
        w("  !! see docs/PROXY_GUARDRAIL.md 'what to do about a divergence'")
    wide, why_wide = unresolved(hist, run)
    if wide:
        w("")
        w("  !! GUARDRAIL NOT RESOLVING !!")
        w(f"  !! {why_wide}")
    w("")
    w("  history (every reading for this arm):")
    w(format_history(hist, players))
    if len(hist) >= 2:
        first, last = hist[0], hist[-1]
        c0 = ((first.get("anchor") or {}).get("culture") or 0.0)
        c1 = ((last.get("anchor") or {}).get("culture") or 0.0)
        n = sum(r.get("accepts_between", 0) for r in hist)
        w(f"  absolute trend: own culture vs book under {policy} "
          f"{c0:.1f} -> {c1:.1f} ({c1 - c0:+.1f}) over {n} accepted "
          f"champions.  This is the series that answers 'is proxy progress "
          f"producing real progress'; the human 2p median is 159.5 "
          f"(docs/HUMAN_BASELINE.md).")
    w("=" * 78)


# --------------------------------------------------------------------- run

def check_arm(players, state_dir=DEFAULT_STATE, policy=SHIP_POLICY,
              deals=None, anchor_deals=None, every_accepts=None,
              max_hours=None, workers=1, force=False, dry_run=False,
              run=3, stale_hours=24.0, log=print):
    b = BUDGET.get(players, BUDGET[2])
    deals = b["deals"] if deals is None else deals
    anchor_deals = b["anchor_deals"] if anchor_deals is None else anchor_deals
    every_accepts = b["every_accepts"] if every_accepts is None else every_accepts
    max_hours = b["max_hours"] if max_hours is None else max_hours

    files = ladder_files(state_dir, players)
    if len(files) < 2:
        log(f"[{players}p] proxy check: ladder has {len(files)} champions, "
            f"nothing to compare yet")
        return None
    hist = read_history(state_dir, players)
    by_gen = {gen_of(p): p for p in files}
    new_path = files[-1]
    new_gen = gen_of(new_path)

    if hist:
        base_gen = hist[-1]["champion_gen"]
        base_path = by_gen.get(base_gen)
        if base_path is None:               # aged out of the ladder dir
            base_path = files[max(0, len(files) - 1 - every_accepts)]
            base_gen = gen_of(base_path)
        accepts = sum(1 for p in files if base_gen < gen_of(p) <= new_gen)
        hours = (time.time() - hist[-1].get("ts", 0)) / 3600.0
    else:
        # FIRST reading: look back `every_accepts` accepts so the series starts
        # with a real measurement instead of an empty seed record, and take it
        # AS SOON AS there are two champions to compare (`hours` is infinite,
        # so `--max-hours` fires).  A monitor that says nothing for its first N
        # accepts has a blind spot exactly where a retarget lands, which is the
        # moment you most want a reading.
        base_path = files[max(0, len(files) - 1 - every_accepts)]
        base_gen = gen_of(base_path)
        accepts = sum(1 for p in files if base_gen < gen_of(p) <= new_gen)
        hours = float("inf")

    # STARVATION: shout before deciding whether a reading is due, because the
    # whole failure mode is "never due, never run, never noticed".
    starved, why_starved = starvation(hist, new_gen, accepts,
                                      0.0 if hours == float("inf")
                                      else hours, stale_hours)
    if starved:
        msg = (f"[{players}p] !! PROXY GUARDRAIL STARVED: {why_starved}")
        log(msg)
        os.makedirs(os.path.dirname(LOG), exist_ok=True)
        with open(LOG, "a") as fh:
            fh.write(f"\n!! {time.strftime('%F %T')} {msg}\n")

    due = force or accepts >= every_accepts or (accepts >= 1 and
                                                hours >= max_hours)
    if not due:
        log(f"[{players}p] proxy check: not due -- {accepts}/{every_accepts} "
            f"accepts and {hours:.1f}/{max_hours:.0f}h since gen {base_gen}")
        return None
    if accepts < 1:
        log(f"[{players}p] proxy check: no new accepted champion since "
            f"gen {base_gen}")
        return None
    if dry_run:
        log(f"[{players}p] proxy check DUE: gen {base_gen} -> {new_gen}, "
            f"{accepts} accepts; would play {deals * players} h2h + "
            f"{anchor_deals * players} anchor games under {policy}")
        return None

    log(f"[{players}p] proxy check: gen {base_gen} -> {new_gen} "
        f"({accepts} accepts), {deals * players} + {anchor_deals * players} "
        f"games under {policy}")
    t0 = time.time()
    res = measure(new_path, base_path, players, policy, deals, anchor_deals,
                  workers=workers)
    edges = accepted_edges(state_dir, players, base_gen, new_gen)
    rec = {
        "at": time.strftime("%F %T"), "ts": time.time(),
        "players": players, "policy": policy,
        "champion": new_path, "champion_gen": new_gen,
        "baseline": base_path, "baseline_gen": base_gen,
        "accepts_between": accepts, "gens_between": new_gen - base_gen,
        "proxy_edge_sum": round(sum(edges), 4),
        "proxy_edges": [round(e, 4) for e in edges],
        "objective_note": "positive by construction: an accept requires a "
                          "positive one-sided lower bound",
        "secs": round(time.time() - t0, 1),
    }
    rec.update(res)
    rec["verdict"] = verdict_of(rec["h2h"])
    hist.append(rec)

    path = history_path(state_dir, players)
    os.makedirs(os.path.dirname(os.path.abspath(path)), exist_ok=True)
    with open(path, "a") as fh:
        fh.write(json.dumps(rec) + "\n")
        fh.flush()
        os.fsync(fh.fileno())
    os.makedirs(os.path.dirname(LOG), exist_ok=True)
    with open(LOG, "a") as fh:
        log_block(fh, players, rec, hist, policy, run)
        fh.flush()
    log(f"[{players}p] proxy check: {rec['verdict']} -- ship win share "
        f"{rec['h2h']['win_rate']:.1%} +/- {rec['h2h']['ci']:.1%} "
        f"(null {rec['h2h']['null']:.1%}), own culture vs book "
        f"{(rec.get('anchor') or {}).get('culture', 0):.1f}, "
        f"{rec['secs']:.0f}s")
    diverging, why = divergence(hist, run)
    if diverging:
        log(f"[{players}p] !! PROXY DIVERGENCE: {why}")
    return rec


def report(state_dir=DEFAULT_STATE, players=(2, 3, 4), log=print):
    for k in players:
        hist = read_history(state_dir, k)
        log(f"\n[{k}p] proxy history: {len(hist)} readings")
        if hist:
            log(format_history(hist, k))
            diverging, why = divergence(hist)
            log(f"  status: {'!! DIVERGING -- ' + why if diverging else 'ok'}")


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--players", type=int, default=2, choices=(2, 3, 4))
    ap.add_argument("--state-dir", default=DEFAULT_STATE)
    ap.add_argument("--policy", default=SHIP_POLICY,
                    help="the SHIP policy to validate under "
                         "(default %(default)s)")
    ap.add_argument("--deals", type=int, default=None)
    ap.add_argument("--anchor-deals", type=int, default=None)
    ap.add_argument("--every-accepts", type=int, default=None)
    ap.add_argument("--max-hours", type=float, default=None,
                    help="validate anyway after this long, if at least one "
                         "champion was accepted -- a slow arm still gets a "
                         "time series")
    ap.add_argument("--workers", type=int, default=1)
    ap.add_argument("--stale-hours", type=float, default=24.0,
                    help="shout if this long has passed with accepted "
                         "champions and no successful reading")
    ap.add_argument("--divergence-run", type=int, default=3,
                    help="consecutive non-confirming readings that count as a "
                         "divergence")
    ap.add_argument("--force", action="store_true")
    ap.add_argument("--dry-run", action="store_true")
    ap.add_argument("--lock-wait", type=float, default=15.0,
                    metavar="MIN",
                    help="minutes to WAIT for the measurement lock before "
                         "giving up (default %(default)g).  Waiting rather "
                         "than skipping is deliberate: a neighbour job that "
                         "holds the lock whenever cron looks would otherwise "
                         "starve the guardrail silently")
    ap.add_argument("--no-lock", action="store_true")
    ap.add_argument("--report", action="store_true")
    a = ap.parse_args(argv)
    if a.report:
        return report(a.state_dir)
    kw = dict(players=a.players, state_dir=a.state_dir, policy=a.policy,
              deals=a.deals, anchor_deals=a.anchor_deals,
              every_accepts=a.every_accepts, max_hours=a.max_hours,
              workers=a.workers, force=a.force, dry_run=a.dry_run,
              run=a.divergence_run, stale_hours=a.stale_hours)
    if a.no_lock or a.dry_run:
        check_arm(**kw)
        return
    lock = Lock(wait_s=a.lock_wait * 60.0)
    with lock as lk:
        if lk is None:
            # Not silent, and not final: the next cron invocation retries, and
            # if the lock never clears the --stale-hours alarm fires from
            # inside check_arm.  A monitor that quietly gives up is the bug
            # this message exists to make visible.
            print(f"proxy check [{a.players}p]: gave up after waiting "
                  f"{lock.waited / 60:.0f} min for {LOCK} (held by "
                  f"{_lock_holder()}); the next invocation retries")
            return
        check_arm(**kw)


if __name__ == "__main__":
    main()
