# PyPy switchover

Working log, appended to continuously.

## 1. Install audit (2026-07-26) — VERDICT: native, no Rosetta, nothing to fix

```
$ which pypy3
/usr/local/bin/pypy3
$ file $(which pypy3)
/usr/local/bin/pypy3: Mach-O 64-bit executable x86_64
$ pypy3 --version
Python 3.11.15 (194f9f44b505, Jul 15 2026, 12:12:21)
[PyPy 7.3.23 with GCC Apple LLVM 16.0.0 (clang-1600.0.26.6)]
$ pypy3 -c "import platform; print(platform.machine())"
x86_64
```

`x86_64` looked alarming at first (Rosetta?), but **this machine is not Apple
silicon at all**:

```
$ sysctl -n hw.model machdep.cpu.brand_string
Macmini8,1
Intel(R) Core(TM) i5-8500B CPU @ 3.00GHz
$ sysctl -n sysctl.proc_translated   # -> unknown oid (Intel host, no Rosetta)
$ sysctl -n hw.optional.arm64        # -> unknown oid
$ arch -arm64 uname -m               # -> arch: Unknown architecture: arm64
```

So `/usr/local/bin/pypy3` is a **native x86_64 binary on a native x86_64 host**.
No emulation penalty, no arm64 build to fetch. PyPy 7.3.23 / Python 3.11.15 is
current. Install is good as-is.

CPython for comparison: see below.

## Cores

```
$ sysctl -n hw.ncpu               -> 6
$ sysctl -n hw.perflevel0.logicalcpu -> 6
$ sysctl -n hw.perflevel1.logicalcpu -> (absent)
```

Coffee Lake i5-8500B: 6 physical cores, **no hyperthreading**, no E-cores.
"Leave 2 free" therefore means **4 worker processes**.

## 2. Correctness

### Test suite: PASS on both

```
python3 -m unittest discover -s tests   ->  Ran 57 tests in  4.315s  OK
pypy3   -m unittest discover -s tests   ->  Ran 57 tests in 14.358s  OK
```

(PyPy is *slower* on the test suite — 57 short tests never reach JIT warmup and
pay ~3x interpretation + compile overhead. Expected; irrelevant to self-play.)

### Determinism fingerprint: ONE case out of 33 diverges

`engine/perf_check.py hash` covers 33 fixed (players, bot, seed) games.

```
CPython  3229c4a0f0d6a4a122ee5e16d44cbc99728da4a9e1855e6ceb36532045223ad7
PyPy     63d62a709a24eb834e899605971300327266d2c9d74136cc3fa05f65e003583f
```

Per-case diff (`tools/dump_game.py`): **32/33 identical, 1 differs**, namely
`(4, 'greedy', 2)`:

```
CPython scores [112, 92, 113, 228]
PyPy    scores [112, 94, 113, 226]
first divergence at log index 50 of 83/83
  ctx 49: T93 P0: event Popularization of Science resolved
  A  50: T104 P3: event National Pride resolved      <- CPython
  B  50: T94  P1: event National Pride resolved      <- PyPy
```

Not a hash-randomisation problem — each interpreter is *self*-consistent and
reproducible, and `PYTHONHASHSEED` in {0,1,2,12345} changes nothing on either:

```
CPython, every seed: [112, 92, 113, 228]
PyPy,    every seed: [112, 94, 113, 226]
```

So it is a structural container-ordering dependency (a `set` iterated without
sorting, or similar) that happens to be stable within one interpreter but
differs between the two. Hunt below.

### Root cause: `sum()` of floats — CPython 3.12+ uses compensated summation, PyPy does not

Bisected with `tools/trace_game.py`. The 365 applied moves are identical up to
move 215; move 216 differs:

```
   A 216: ('pol_pass',)                                  <- CPython
   B 216: ('prepare_event', 'Strategic Territory (II)')  <- PyPy
```

`tools/trace_game.py --probe 4 greedy 2 216` replays to that decision and dumps
GreedyBot's 1-ply evaluation of all 11 legal moves:

```
move                                              CPython      PyPy
('pol_pass',)                                     56.25        56.25
('offer_pact', 'Acceptance of Supremacy', 0, 'A') 56.25        56.25
... (8 more, all 56.25 / 56.25)
('prepare_event', 'Strategic Territory (II)')     56.25        56.250000000000014   <<<
```

Every move evaluates to the *same* position value. `GreedyBot.pick` keeps the
best strictly (`val > best_val`), so on CPython the whole list ties and the
first move (`pol_pass`) wins; on PyPy the last move is larger by 1.4e-14 and
wins instead. One ULP flips the move, and the game diverges from there.

The 1-ULP difference comes from `engine/bots/__init__.py::evaluate`:

```python
own = sum(w.get(k, 0.0) * v for k, v in f.items())
```

**CPython 3.12 added Neumaier compensated summation to builtin `sum()` for
floats; PyPy 3.11's `sum()` is a naive left-to-right accumulation.** This box
runs CPython 3.14.6, so it gets the compensated result and PyPy gets the naive
one. Nothing to do with hash order, set order, or `float` repr.

Corollary worth knowing independently of PyPy: **this engine is already not
reproducible across CPython versions** — CPython 3.11 would produce PyPy's
answer here, not 3.14's.

### FIX: `math.fsum` in `evaluate` — determinism achieved, zero behaviour change

`math.fsum` is *exactly rounded* on every Python implementation, so both
interpreters agree. Verified by monkeypatch first (`tools/fsum_patch.py`) before
touching the engine:

```
CPython, sum()  (baseline)  3229c4a0f0d6a4a122ee5e16d44cbc99728da4a9e1855e6ceb36532045223ad7
PyPy,    sum()              63d62a709a24eb834e899605971300327266d2c9d74136cc3fa05f65e003583f
CPython, fsum()             3229c4a0f0d6a4a122ee5e16d44cbc99728da4a9e1855e6ceb36532045223ad7  <- same as baseline
PyPy,    fsum()             3229c4a0f0d6a4a122ee5e16d44cbc99728da4a9e1855e6ceb36532045223ad7  <- same as baseline
```

That is the ideal outcome: `fsum` reproduces bit-for-bit what CPython 3.14 was
already doing, so **the running CPython hill climbs are not perturbed at all**,
and PyPy now agrees with them. The one-line change is in
`engine/bots/__init__.py::evaluate` (a file otherwise owned by another agent;
the edit is 1 line plus `import math` plus a comment).

Post-fix state, both interpreters:

```
python3 -m engine.perf_check check <fp>   ->  OK  identical behaviour: 3229c4a0...
pypy3   -m engine.perf_check check <fp>   ->  OK  identical behaviour: 3229c4a0...
python3 -m unittest discover -s tests     ->  57 tests OK
pypy3   -m unittest discover -s tests     ->  57 tests OK
```

**Determinism gate: PASSED.** PyPy is cleared for training use.
