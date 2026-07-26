"""Where does GreedyBot's time actually go?

Two modes, because they lie in opposite directions:

  * ``--mode sample`` (default) -- a background thread snapshots the main
    thread's stack every ``--interval`` seconds via ``sys._current_frames``.
    Sampling costs ~nothing per call, so functions that are called millions of
    times with tiny bodies (``fastcopy._cv``) are *not* penalised.  Reports
    self-time (the leaf frame) and inclusive time (anywhere on the stack).
  * ``--mode cprofile`` -- deterministic, exact call counts, but adds ~1 us per
    call, which inflates exactly the hot tiny functions we care about.  Use it
    for "how many times", not for "what fraction".

    nice -n 10 python3 tools/profile_bot.py --players 4 --games 2
    nice -n 10 python3 tools/profile_bot.py --mode cprofile --games 1
"""
from __future__ import annotations

import argparse
import collections
import random
import sys
import threading
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from engine import game                     # noqa: E402
from engine.bots import GreedyBot, RandomBot  # noqa: E402


def _bots(kind, n, seed):
    cls = RandomBot if kind == "random" else GreedyBot
    return [cls(random.Random(seed * 131 + i)) for i in range(n)]


class Sampler(threading.Thread):
    """Snapshot the target thread's stack at a fixed interval."""

    daemon = True

    def __init__(self, target_id, interval):
        super().__init__()
        self.target_id, self.interval = target_id, interval
        self.self_time = collections.Counter()
        self.incl = collections.Counter()
        self.samples = 0
        self._stop = threading.Event()

    def run(self):
        frames = sys._current_frames
        while not self._stop.is_set():
            f = frames().get(self.target_id)
            if f is not None:
                code = f.f_code
                self.self_time[(code.co_filename, code.co_name)] += 1
                seen = set()
                while f is not None:
                    c = f.f_code
                    seen.add((c.co_filename, c.co_name))
                    f = f.f_back
                self.incl.update(seen)
                self.samples += 1
            time.sleep(self.interval)

    def stop(self):
        self._stop.set()
        self.join(timeout=2.0)


def _short(key):
    fn, name = key
    parts = Path(fn).parts
    tail = "/".join(parts[-2:]) if len(parts) > 1 else fn
    return f"{tail}:{name}"


def run_sample(kind, n, games, interval, top):
    s = Sampler(threading.get_ident(), interval)
    t0 = time.process_time()
    s.start()
    for seed in range(games):
        game.play_game(_bots(kind, n, seed), n, seed=seed)
    s.stop()
    cpu = time.process_time() - t0
    tot = s.samples or 1
    print(f"{kind} {n}p, {games} games, {cpu:.1f} cpu-s, {s.samples} samples "
          f"@{interval * 1000:.0f} ms\n")
    print(f"{'SELF %':>7}  {'INCL %':>7}  function")
    for key, c in s.self_time.most_common(top):
        print(f"{100 * c / tot:7.2f}  {100 * s.incl[key] / tot:7.2f}  {_short(key)}")
    print("\ntop inclusive:")
    for key, c in s.incl.most_common(top):
        print(f"{100 * s.self_time[key] / tot:7.2f}  {100 * c / tot:7.2f}  {_short(key)}")


def run_cprofile(kind, n, games, top):
    import cProfile
    import pstats
    pr = cProfile.Profile()
    pr.enable()
    for seed in range(games):
        game.play_game(_bots(kind, n, seed), n, seed=seed)
    pr.disable()
    pstats.Stats(pr).sort_stats("tottime").print_stats(top)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--kind", default="greedy")
    ap.add_argument("--players", type=int, default=4)
    ap.add_argument("--games", type=int, default=2)
    ap.add_argument("--interval", type=float, default=0.002)
    ap.add_argument("--top", type=int, default=25)
    ap.add_argument("--mode", default="sample", choices=("sample", "cprofile"))
    a = ap.parse_args()
    if a.mode == "sample":
        run_sample(a.kind, a.players, a.games, a.interval, a.top)
    else:
        run_cprofile(a.kind, a.players, a.games, a.top)
    return 0


if __name__ == "__main__":
    sys.exit(main())
