"""The operator loop: play the CGE app, log the bot.

Built on `advisor.Console`, which already does the hard part -- mirroring the
board, ranking moves, fuzzy card names, never crashing on bad input.  What is
added here is only the three things `docs/EXTERNAL_AIS.md` section 6 says are
missing:

1. **A minimal, derived prompt.**  Opponent turns ask for the freshly dealt row
   cards and nothing else.  Rival state is collected once per round as four
   numbers per rival, read straight off the app's player panel, because
   `harness.fields` shows those four numbers reconstruct every rival-derived
   feature the evaluator has.  The derivation is re-run every round against the
   live board, so when the card-row / opponent-hand features land the harness
   starts demanding them **by itself** and says so out loud.
2. **A mandatory per-round checksum** on our own board, which the mirror
   *predicts* and can therefore be caught getting wrong.
3. **A structured record** (`harness.record`) with the setup, the pact bias and
   the derived observable set attached.

It also counts keystrokes, so the cost estimate in `docs/APP_HARNESS.md` stops
being a guess after the first real game.
"""

from __future__ import annotations

import argparse
import time

import advisor.state_io as S
from advisor.advisor import Advisor, Console, _Quit, load_bot
from engine import actions as A
from engine import game as G

from harness import fields as F
from harness import mirror as M
from harness.record import DIFFICULTIES, GameLog, Setup, SetupError

HEADER = """\
Through the Ages -- CGE app harness.

Per round you type this, and nothing else:
  * the cards dealt into the row, as you see them ('bro irr alc' is enough)
  * one line for your own panel     c/s/str/f/r
  * one line per rival panel        c/cr/sr/str

Everything else on their boards -- techs, wonders, government, food,
resources, actions, hand sizes -- is provably invisible to the evaluator
(`python3 -m harness.fields` prints the derivation).  Do not type it.  If the
evaluator grows an eye for something new, this harness will start asking for
it on its own, mid-game, in capital letters.

STRICT MODE IS THE MEASUREMENT.  Press Enter and play what the bot starred.
The moment you "help" it, the score stops measuring the bot -- and only the
note you write at the end will ever say so.
"""


class HarnessConsole(Console):
    """`advisor.Console` plus checksums, logging and a keystroke counter."""

    def __init__(self, adv, log, inp=input, out=print, strict=True,
                 verdicts=None):
        super().__init__(adv, inp=inp, out=out)
        self.log = log
        self.strict = strict
        self.keystrokes = 0
        self.prompts = 0
        self.rounds_checked = 0
        self.overrides = 0
        self.history = M.RivalHistory()
        self.typed = []                 # literal lines, the forensic trail
        self.checked_round = None
        self.aborted = False
        self.abort_reason = ""
        self.verdicts = dict(verdicts or {})
        self.mandatory = self._mandatory_ids()
        self.t0 = time.time()
        # record the move the advisor actually applied, without duplicating
        # `Console.handle_move_input`'s parsing
        self.last_move = None
        inner = adv.play

        def play(move):
            ok, msg = inner(move)
            if ok:
                self.last_move = list(move)
            return ok, msg
        adv.play = play

    def _mandatory_ids(self):
        return {r.probe.id for r in F.requirements(self.verdicts)
                if r.mandatory}

    # ---- io, counted
    def ask(self, prompt):
        self.prompts += 1
        line = super().ask(prompt)
        self.keystrokes += len(line or "") + 1        # + the Enter
        if line and line.strip():
            self.typed.append(line.strip())
        return line

    # ------------------------------------------------------- our own turn

    def my_turn(self):
        self.check_dealt()
        if not self.round_check():
            return False
        self._snapshot = S.dumps(self.adv.board)
        adv = self.adv
        while adv.my_turn() and not adv.state.game_over:
            cands = adv.recommend(40)
            if not cands:
                return True
            self.show_candidates(cands[:3])
            state_text = S.dumps(adv.board)
            st_round, st_age = adv.state.round, adv.state.age_civil
            t = time.time()
            line = self.ask("your move> ").strip()
            latency = time.time() - t
            self.last_move = None
            if not self.handle_move_input(line, cands[:3]):
                return False
            if self.last_move is not None:
                self._log_decision(state_text, cands, line, st_round, st_age,
                                   latency)
        return True

    def _log_decision(self, state_text, cands, raw, round_, age, latency):
        ranked = [{"move": list(c.move), "score": round(c.score, 5),
                   "text": c.text, "reason": c.reason} for c in cands]
        top = list(cands[0].move) if cands else None
        source = "bot" if self.last_move == top else "human"
        if source == "human":
            self.overrides += 1
        self.log.decision(state_text, ranked, self.last_move, source,
                          round_, age, latency_s=round(latency, 1),
                          note="" if source == "bot" else f"typed {raw!r}")
        if self.strict and source == "human":
            self.say("  ** OVERRIDE recorded. In strict mode this game no "
                     "longer measures the bot; say so in the result notes.")

    # ------------------------------------------------------ opponent turns

    def opponent_turn(self):
        adv = self.adv
        who = adv.state.decider()
        self.check_dealt()
        self.say(f"\n-- p{who}'s turn.")
        line = self.ask(f"  did p{who} hit YOU or change the shared board? "
                        f"(Enter = no) > ").strip()
        while line:
            if line.lower() in ("quit", "q", "exit"):
                return False
            if line.lower() in ("help", "?"):
                self.say(S.PATCH_HELP)
            else:
                self.report(line)
            line = self.ask("  > ").strip()
        adv.skip_opponent_turn()
        return True

    # ------------------------------------------------------- the checksum

    def round_check(self):
        """Once per round, right before our turn: does the mirror match the app?

        This is the only thing standing between an hour of work and a silently
        fabricated game, so it is not skippable and there is no "ignore".
        """
        st = self.adv.state
        if self.checked_round == st.round:
            return True
        self.checked_round = st.round
        self.rounds_checked += 1
        self._rederive()
        self.say(f"\n== round {st.round} check (age {st.age_civil}) "
                 f"-- read the app, do not guess")
        reported = self._ask_self()
        rivals = self._ask_rivals(st)
        result = M.round_check(self.adv.board, reported, self.history, rivals)
        self.log.check(result, state_text=S.dumps(self.adv.board))
        for d in result.discrepancies:
            if d.severity == M.WARN:
                self.say(f"  ? {d}  (arithmetic, not proof -- re-read it)")
        if result.failed and not self.handle_desync(result):
            return False
        for idx, vals in rivals.items():
            for key in M.RIVAL_ASK_KEYS:
                if vals.get(key) is not None:
                    self.report(f"p{idx} {key}={vals[key]}")
        self.log.observed(st.round, getattr(self.adv, "dealt_slots", []),
                          {str(k): v for k, v in rivals.items()}, self.typed)
        self.typed = []
        return True

    def _rederive(self):
        """Re-run the field derivation against the live board.

        Free (one extra scoring pass per probe) and it is what keeps this
        harness correct across the feature work in flight: the day a card-row
        feature lands, `row.contents` stops being advisory and the operator is
        told, in the middle of the game, that the rules of engagement changed.
        """
        try:
            fresh = F.probe_position(self.adv.state, self.adv.board.me,
                                     self.adv.bot.weights)
        except Exception:
            return
        for k, v in fresh.items():
            self.verdicts[k] = F.worse(self.verdicts.get(k, F.INERT), v)
        now = self._mandatory_ids()
        new = now - self.mandatory
        if new:
            self.say("\n  ** THE BOT'S EYES CHANGED. It now reads: "
                     + ", ".join(sorted(new)))
            self.say("  ** These must be transcribed from here on, and games "
                     "logged before this point saw less. See "
                     "`python3 -m harness.fields`.")
        self.mandatory = now

    def _ask_self(self):
        while True:
            line = self.ask(f"  you   {'/'.join(M.SPINE)} > ").strip()
            if line.lower() in ("quit", "q", "exit"):
                raise _Quit()
            if line.lower() == "board":
                self.say(S.render(self.adv.board))
                continue
            reported, errs = M.parse_line(line)
            missing = M.missing_spine(reported)
            if errs:
                for e in errs:
                    self.say(f"    ! {e}")
                continue
            if missing:
                self.say(f"    ! still need {', '.join(missing)} -- the check "
                         f"is the point of this exercise, not paperwork")
                continue
            return reported

    def _ask_rivals(self, st):
        rivals = {}
        for q in st.players:
            if q.idx == self.adv.board.me or q.resigned:
                continue
            while True:
                line = self.ask(f"  p{q.idx}    "
                                f"{'/'.join(M.RIVAL_ASK_KEYS)} > ").strip()
                if line.lower() in ("quit", "q", "exit"):
                    raise _Quit()
                vals, errs = M.parse_line(
                    line, spine=list(M.RIVAL_ASK_KEYS),
                    allowed=set(M.RIVAL_ASK_KEYS))
                if errs:
                    for e in errs:
                        self.say(f"    ! {e}")
                    continue
                rivals[q.idx] = vals
                break
        return rivals

    def handle_desync(self, result):
        """A hard mismatch.  There is no 'continue anyway' option, by design."""
        self.say("\n" + "!" * 70)
        self.say("DESYNC. The mirror and the app disagree:")
        for d in result.discrepancies:
            if d.severity == M.FAIL:
                self.say(f"   {d}")
        self.say("The bot has been choosing moves in a position that is not the")
        self.say("one on your screen, for an unknown number of plies.")
        self.say("  [a] abort  -- log it and stop. Right answer for early games.")
        self.say("  [r] resync -- type corrections, then say what caused it.")
        self.say("!" * 70)
        while True:
            choice = self.ask("  a/r> ").strip().lower()[:1]
            if choice == "a":
                self.aborted = True
                self.abort_reason = "; ".join(
                    str(d) for d in result.discrepancies if d.severity == M.FAIL)
                self.log.resync(result.round, result.discrepancies, [],
                                "aborted, not resynced")
                return False
            if choice == "r":
                self.say("  type corrections (blank line to finish):")
                patches = []
                while True:
                    line = self.ask("   > ").strip()
                    if not line:
                        break
                    patches.append(line)
                    self.report(line)
                cause = self.ask("  what caused it? (required; 'unknown' is a "
                                 "valid answer) > ")
                while not cause.strip():
                    cause = self.ask("  a cause is required > ")
                self.log.resync(result.round, result.discrepancies, patches,
                                cause.strip())
                after = M.round_check(self.adv.board, result.reported)
                if after.failed:
                    self.say("  ! still out of sync:")
                    for d in after.discrepancies:
                        self.say(f"     {d}")
                    continue
                self.say("  resynced. This game is now UNTRUSTED "
                         "(record.trusted=false): keep it, do not pool it.")
                return True

    # --------------------------------------------------------- the ending

    def run(self):
        self.say(HEADER)
        self.say(f"bot: {getattr(self.adv.bot, 'source', '?')}")
        self.say(S.render(self.adv.board))
        try:
            while not self.adv.state.game_over:
                if self.adv.my_turn():
                    if not self.my_turn():
                        break
                elif not self.opponent_turn():
                    break
        except _Quit:
            self.aborted = True
            self.abort_reason = self.abort_reason or "operator quit"
        if not self.adv.state.game_over and not self.aborted:
            # stopping early is not a result. Recording it as one would put a
            # half-played game into the aggregate with a real-looking margin.
            self.aborted = True
            self.abort_reason = "stopped before the game ended"
        return self.finish()

    def finish(self):
        st = self.adv.state
        scores = {}
        if st.game_over:
            scores = {f"p{i}": s for i, s in enumerate(G.scores(st))}
        notes = []
        if not self.aborted:
            self.say("\nFinal scores AS THE APP SHOWS THEM "
                     "(these, not the mirror's, are the measurement).")
            real = self._ask_scores()
            if real:
                if scores and real != scores:
                    notes.append(f"mirror scores {scores} != app scores {real}")
                scores = real
        minutes = self.ask("  wall-clock minutes (Enter = measured) > ").strip()
        try:
            human_minutes = int(minutes)
        except ValueError:
            human_minutes = max(1, round((time.time() - self.t0) / 60))
        free = self.ask("  notes (misreads, overrides, anything odd) > ")
        if free.strip():
            notes.append(free.strip())
        if self.overrides:
            notes.append(f"{self.overrides} override(s) of the bot's top pick")
        rec = self.log.result(
            scores, st.round, human_minutes=human_minutes,
            notes="; ".join(notes), aborted=self.aborted,
            abort_reason=self.abort_reason,
            effort=self.effort(human_minutes),
            observables=[r.to_json() for r in F.requirements(self.verdicts)])
        self.say(f"\nwrote {self.log.path}")
        flag = "" if rec["trusted"] else f" ({rec['untrusted_reason']})"
        self.say(f"  trusted={rec['trusted']}{flag}")
        e = rec["effort"]
        self.say(f"  {e['keystrokes']} keystrokes / {e['prompts']} prompts "
                 f"over {e['rounds']} rounds "
                 f"= {e['keystrokes_per_round']:.0f} keystrokes per round")
        self.say("  REMINDER: no pacts were possible in this game -- the CGE AI "
                 "refuses them. This is not a full-game measurement.")
        return rec

    def _ask_scores(self):
        n = self.adv.state.num_players
        line = self.ask(f"  final scores  "
                        f"{'/'.join('p%d' % i for i in range(n))} > ").strip()
        toks = line.replace(",", " ").replace("/", " ").split()
        out = {}
        for i, tok in enumerate(toks):
            try:
                out[f"p{i}"] = int(tok)
            except ValueError:
                pass
        return out

    def effort(self, human_minutes):
        r = max(1, self.rounds_checked)
        return {"keystrokes": self.keystrokes, "prompts": self.prompts,
                "rounds": self.rounds_checked, "overrides": self.overrides,
                "keystrokes_per_round": self.keystrokes / r,
                "minutes_per_round": (human_minutes or 0) / r}


# ------------------------------------------------------------------- setup


def ask_setup(ask, say, args):
    """The pre-flight.  Refuses to start a game we cannot honestly log."""
    dlc = args.dlc
    if dlc is None:
        say("\nIs 'New Leaders & Wonders' switched OFF in the app's game setup? "
            "Our engine does not implement it, so a DLC game is a mislabelled "
            "log, not a weak one.")
        dlc = not ask("  expansion is OFF? [y/N] > ").strip().lower().startswith("y")
    return Setup(
        game_id=args.id or time.strftime("%Y-%m-%d-%H%M%S"),
        players=args.players, seat=args.seat, difficulty=args.difficulty,
        mode="free" if args.free else "strict", dlc=dlc,
        app_version=args.app_version, weights=args.weights or "",
        operator=args.operator, personalities=list(args.personality or []),
    ).validate()


def build_parser():
    ap = argparse.ArgumentParser(description="CGE app harness")
    ap.add_argument("--players", type=int, default=3)
    ap.add_argument("--seat", type=int, default=0)
    ap.add_argument("--seed", type=int, default=0)
    ap.add_argument("--weights", default=None)
    ap.add_argument("--difficulty", default="hard", choices=list(DIFFICULTIES))
    ap.add_argument("--free", action="store_true",
                    help="free mode: you play, the bot only watches")
    ap.add_argument("--dlc", type=lambda s: s.lower() in ("1", "true", "yes"),
                    default=None, help="New Leaders & Wonders on? MUST be off")
    ap.add_argument("--app-version", default="")
    ap.add_argument("--operator", default="")
    ap.add_argument("--personality", action="append",
                    help="a 'world leader' AI personality (labels the game as "
                         "a different experiment)")
    ap.add_argument("--id", default=None)
    ap.add_argument("--log", default=None)
    ap.add_argument("--load", default=None)
    return ap


def main(argv=None, inp=input, out=print):
    args = build_parser().parse_args(argv)
    try:
        setup = ask_setup(inp, out, args)
    except SetupError as exc:
        out(f"\nrefusing to start: {exc}")
        return 2

    if args.load:
        with open(args.load) as fh:
            board = S.loads(fh.read())
    else:
        board = S.new_board(args.players, me=args.seat, seed=args.seed)
    bot = load_bot(board.state.num_players, args.weights, seed=args.seed)
    setup.weights = setup.weights or getattr(bot, "source", "")
    adv = Advisor(board, bot, seed=args.seed)
    if not args.load:
        adv.dealt_slots = list(range(A.ROW_SIZE))

    verdicts = F.probe_position(board.state, args.seat, bot.weights)
    path = args.log or f"games/{setup.game_id}.jsonl"
    log = GameLog(setup, path, requirements=F.requirements(verdicts))
    out(f"\nlogging to {path}")
    HarnessConsole(adv, log, inp=inp, out=out, strict=not args.free,
                   verdicts=verdicts).run()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
