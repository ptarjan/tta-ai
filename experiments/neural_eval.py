"""Head-to-head strength of NeuralBot vs an opponent spec (single process, GPU).

The value model lives on the GPU and is SHARED across all games (single
process), and every decision scores its ~11 candidate states in one batched
forward pass.  Games are seat-rotated so seat order is exactly balanced; the
null win rate is 1/players and error bars are normal-approx 95% CIs (same
`mean_ci` as experiments/arena.py).

Reports win share AND mean final culture with CIs -- culture is the dense
statistic and is what docs/HUMAN_BASELINE.md / docs/SCORE_VALIDATION.md quote
(human 159.5, quiescent champion 64.7, 1-ply lineage vector 139.8, all 2p).

Usage (desktop):
    python neural_eval.py --ckpt checkpoints/value2p.pt --games 200 \
        --players 2 --opponent experiments/league_state/champion_2p.json
    python neural_eval.py --ckpt ... --opponent book
    python neural_eval.py --ckpt ... --opponent neural   # self-play control
"""
from __future__ import annotations

import argparse
import os
import sys
import time

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

from engine import game
from engine.bots.neural_net import NeuralValue
from engine.bots.neural_bot import NeuralBot
from experiments import arena


def make_opponent(spec_str, seed):
    if spec_str == "book":
        from engine.bots.book import BookBot
        return BookBot(seed=seed)
    if spec_str == "book2":
        from engine.bots.book import BookBot
        return BookBot(seed=seed, version=2)
    return arena.make_bot(arena.load_spec(spec_str), seed)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--ckpt", required=True)
    ap.add_argument("--opponent", default="default")
    ap.add_argument("--games", type=int, default=200)
    ap.add_argument("--players", type=int, default=2)
    ap.add_argument("--seed0", type=int, default=0)
    ap.add_argument("--device", default="cuda")
    ap.add_argument("--end-turn-bias", type=float, default=0.0)
    ap.add_argument("--determinize", type=int, default=1)
    ap.add_argument("--move-cap", type=int, default=20000)
    ap.add_argument("--search", default="1ply", choices=("1ply", "plan"),
                    help="which search wraps --ckpt: the 1-ply argmax "
                         "(NeuralBot) or PlanBot's whole-turn beam with the "
                         "net as the leaf (NeuralPlanBot). `--search plan "
                         "--opponent nplan:SAME.pt` is a search-only A/B.")
    ap.add_argument("--width", type=int, default=None, help="beam width")
    ap.add_argument("--nodes", type=int, default=None, help="beam node cap")
    ap.add_argument("--war", type=int, default=None, help="beam war lookahead")
    ap.add_argument("--threads", type=int, default=1,
                    help="torch CPU threads. ONE by default: torch otherwise "
                         "grabs every core for a 200x1897 GEMM that takes 4ms, "
                         "and N of these running in parallel then thrash. "
                         "Measured: 8 unthrottled procs got 0.25 core each.")
    ap.add_argument("--report", type=int, default=20,
                    help="games between progress lines")
    args = ap.parse_args()

    import torch
    if args.threads:
        torch.set_num_threads(args.threads)
    device = args.device if torch.cuda.is_available() else "cpu"
    value = NeuralValue.from_checkpoint(args.ckpt, device)
    print(f"loaded {args.ckpt} on {device} "
          f"({torch.cuda.get_device_name(0) if device.startswith('cuda') else 'cpu'})",
          flush=True)
    # a second neural opponent (candidate-vs-best gating): `--opponent neural:PATH`
    opp_value = None
    opp_search = "1ply"
    for pfx, srch in (("neural:", "1ply"), ("nplan:", "plan")):
        if args.opponent.startswith(pfx):
            opp_value = NeuralValue.from_checkpoint(
                args.opponent[len(pfx):].split(",")[0], device)
            opp_search = srch

    def make_neural(val, seed, search):
        """Wrap a value net in the requested search. Both sides of a gate go
        through here so a candidate is never accidentally given a different
        search from the incumbent."""
        if search == "plan":
            from engine.bots.neural_plan import NeuralPlanBot
            return NeuralPlanBot(val, seed=seed, width=args.width,
                                 max_nodes=args.nodes,
                                 determinize=bool(args.determinize),
                                 war_lookahead=(None if args.war is None
                                                else bool(args.war)))
        return NeuralBot(val, seed=seed, determinize=bool(args.determinize),
                         end_turn_bias=args.end_turn_bias)

    n = args.players
    shares, ca, cb, moves, errs = [], [], [], [], 0
    t0 = time.time()
    for g in range(args.games):
        seat = g % n
        seed = args.seed0 + g // n
        gseed = seed * 7919 + 17
        neural = make_neural(value, gseed * 97 + seat * 13 + 1, args.search)
        bots = []
        for i in range(n):
            if i == seat:
                bots.append(neural)
            elif args.opponent == "neural" or opp_value is not None:
                # self control (`neural`, shares model) OR candidate-vs-best
                # gating (`neural:PATH` / `nplan:PATH`, a second loaded model).
                bots.append(make_neural(
                    opp_value or value, gseed * 97 + i * 13 + 1,
                    opp_search if opp_value is not None else args.search))
            else:
                bots.append(make_opponent(args.opponent, gseed * 97 + i * 13 + 1))
        try:
            st = game.play_game(bots, n, seed=gseed, move_cap=args.move_cap)
            sc = game.scores(st)
        except Exception as e:
            errs += 1
            if errs <= 3:
                print("  engine error:", repr(e), flush=True)
            continue
        best = max(sc)
        tied = [i for i, v in enumerate(sc) if v == best]
        share = (1.0 / len(tied)) if seat in tied else 0.0
        others = [sc[i] for i in range(n) if i != seat]
        shares.append(share)
        ca.append(sc[seat])
        cb.append(sum(others) / len(others))
        moves.append(getattr(st, "moves_played", 0))
        if (g + 1) % max(1, args.report) == 0:
            m, half = arena.mean_ci(shares)
            dt = time.time() - t0
            print(f"  {g + 1}/{args.games}  win {m:.3f}+/-{half:.3f}  "
                  f"cul {sum(ca)/len(ca):.1f} vs {sum(cb)/len(cb):.1f}  "
                  f"({dt:.0f}s)", flush=True)

    m, half = arena.mean_ci(shares)
    cam, cah = arena.mean_ci(ca)
    cbm, cbh = arena.mean_ci(cb)
    margins = [x - y for x, y in zip(ca, cb)]
    mm, mh = arena.mean_ci(margins)
    null = 1.0 / n
    p = arena.p_value(m, half, null)
    print("\n==== RESULT ====")
    print(f"neural[{args.search}] vs {args.opponent} @ {n}p  n={len(shares)} "
          f"(det={args.determinize}, etb={args.end_turn_bias}, "
          f"width={args.width}, nodes={args.nodes})")
    print(f"win rate     {m:.3f} +/- {half:.3f}  (null {null:.3f}, p={p:.4g})")
    print(f"neural cult  {cam:.1f} +/- {cah:.1f}")
    print(f"oppo cult    {cbm:.1f} +/- {cbh:.1f}")
    print(f"margin       {mm:+.1f} +/- {mh:.1f}")
    print(f"moves/game   {sum(moves)/len(moves):.0f}   engine errors {errs}")
    # machine-parseable line for the self-play loop orchestrator
    print(f"SUMMARY win={m:.4f} ci={half:.4f} neural={cam:.1f} opp={cbm:.1f} "
          f"margin={mm:.1f} n={len(shares)} errs={errs}", flush=True)


if __name__ == "__main__":
    main()
