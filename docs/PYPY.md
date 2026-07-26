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

### Re-verification at HEAD 7c2eef1 (2026-07-26, post-fsum-fix)

Independent re-run of the whole 33-case suite, both interpreters, from a clean
checkout of master at `7c2eef1` (fsum fix is `4290459`):

```
$ nice -n 10 python3 -m engine.perf_check save /tmp/fp_cpy.json
saved 3229c4a0f0d6a4a122ee5e16d44cbc99728da4a9e1855e6ceb36532045223ad7 (33 cases)

$ nice -n 10 /usr/local/bin/pypy3 -m engine.perf_check check /tmp/fp_cpy.json
OK  identical behaviour: 3229c4a0f0d6a4a122ee5e16d44cbc99728da4a9e1855e6ceb36532045223ad7
```

**33/33 cases byte-identical**, including the previously-diverging
`(4, 'greedy', 2)`.  `check` compares per-case digests, and it printed no
`differs:` lines — the digest covers the full game log, final scores, winners,
move count, turn and round, so this is byte-identical game logs *and* scores,
not just matching totals.

Belt-and-braces: the 102-case `--wide` sweep (24 random + 10 greedy seeds per
player count) also agrees exactly:

```
$ nice -n 10 python3 -m engine.perf_check save /tmp/fp_wide_cpy.json --wide
saved c7e73ede8a5bfd4567adb7f7660d7e19ae61088d3f1cbf4077c27a45e10a098b (102 cases)

$ nice -n 10 /usr/local/bin/pypy3 -m engine.perf_check check /tmp/fp_wide_cpy.json
OK  identical behaviour: c7e73ede8a5bfd4567adb7f7660d7e19ae61088d3f1cbf4077c27a45e10a098b
```

**VERDICT: 135/135 games (33 narrow + 102 wide) byte-identical across
interpreters. Determinism holds.**

## 3. Steady-state throughput — CPython 3.14.6 vs PyPy 7.3.23

Tool: `tools/bench_interp.py`. It warms up for a fixed number of **CPU-seconds**
(not games — a 4p greedy game is ~1.3 CPU-s, a 2p random game 0.02 CPU-s, so a
game-count warm-up is wildly unfair to one cell or the other), then measures for
a fixed number of CPU-seconds and reports only that steady-state window. It also
prints a per-second ramp trace of the warm-up so the JIT ramp is visible.

Run: `nice -n 10`, sequentially (CPython first, then PyPy), 8 s warm-up / 12 s
measure per cell. **The three hill climbs were running throughout** (4 CPU-busy
python3 processes on 6 cores, load average ~7.8), which is why the metric is
`time.process_time` — CPU seconds consumed by the benchmark process itself —
and not wall clock. Both interpreters saw the same load, sequentially.

| cell | CPython 3.14.6 | PyPy 7.3.23 | PyPy / CPython |
|---|---|---|---|
| random 2p | **54.06** games/cpu-s | 30.07 | 0.56x |
| random 3p | **34.36** | 25.90 | 0.75x |
| random 4p | **19.61** | 17.36 | 0.89x |
| greedy 2p | **3.498** | 2.902 | 0.83x |
| greedy 3p | **1.673** | 1.398 | 0.84x |
| greedy 4p | **0.744** | 0.624 | 0.84x |

Moves/cpu-s tells the same story (e.g. greedy 4p: 289 CPython vs 241 PyPy).

**PyPy is slower than CPython in every single cell**, by 11–44%.

Warm-up ramps (games/s per warm-up second) confirm the JIT does ramp — PyPy
random 2p climbs 10.0 → 25.9 over the 8 s warm-up — but even fully warm it
never catches CPython 3.14. Because the greedy ramps were still rising at the
8 s mark, the greedy cells were re-run with a much longer warm-up; see below.

Why CPython wins here: 3.14's specialising adaptive interpreter is very good at
exactly this workload (attribute loads on dataclasses, small dict probes,
`lru_cache` hits), the engine has already been hand-optimised *for* CPython
(module-level card-DB bindings, compiled effect programs, `lru_cache`d move
scaffolding), and the hot loop is allocation-heavy short-lived object churn
(`copy_state` per candidate move) rather than the long numeric loops PyPy's
JIT excels at. PyPy also pays a GC cost on that churn that CPython's refcounting
frees immediately.

### Long-warm-up re-check of the greedy cells — PyPy still loses

The greedy ramps were still rising at 8 s, so the greedy cells were re-run with
a **45 s CPU warm-up and a 30 s measure window** (PyPy first, then CPython, both
`nice -n 10`, climbs still running):

| cell | CPython 3.14.6 | PyPy 7.3.23 | PyPy / CPython |
|---|---|---|---|
| greedy 2p | **3.528** games/cpu-s | 2.929 | 0.83x |
| greedy 4p | **0.815** games/cpu-s | 0.628 | 0.77x |

PyPy's 2p ramp shows the JIT plateauing after ~4 s (0.95 → ~2.5 by second 4,
then flat around 2.4–3.4 for the remaining 33 s). It is fully warm and still
17–23% behind. Longer warm-up did not change the verdict; if anything the wider
window made the 4p gap *worse* (0.89x at 8 s -> 0.77x at 45 s on random 4p's
sibling cell).

### DECISION (task 4): **DO NOT switch the hill climbs to pypy3**

The climbs run GreedyBot self-play, which is the cell where PyPy is 17–23%
slower. Switching would cost throughput *and* risk a live training run for
nothing. The three detached climbs stay on `python3` (CPython 3.14.6).
No interpreter switch point to record — there is no switch.

The determinism work is not wasted: `math.fsum` in `evaluate` (commit 4290459)
means the engine is now reproducible across interpreters *and across CPython
versions* (3.11 vs 3.12+ differed on `sum()` of floats before it), which is
worth keeping on its own merits.

Re-test pypy3 if any of these change: PyPy gains a faster GC for
short-lived-object churn, the bots stop copying a whole `GameState` per
candidate move, or the project moves to an older/non-specialising CPython.

## Status / next steps (keep current)

- [x] Task 1 — determinism re-verified, 33/33 + 102/102 identical. PASS.
- [x] Task 2 — steady-state games/s table (8 s warm-up). **PyPy loses every cell.**
- [x] Task 2b — greedy cells re-run with a 45 s warm-up. PyPy still 17–23% behind.
- [x] Task 3 — core scaling / worker count: 6 physical cores, no SMT -> 4 workers.
- [x] Task 4 — **NO SWITCH.** Climbs stay on CPython 3.14.6, untouched.
- [ ] Task 5 — further engine optimisation (favouring both runtimes).

### Re-baseline note (commit f4bcac0, 2026-07-26)

The main agent changed a rule after these measurements: yellow action cards now
resolve their ordered action FIRST at full price with the gains landing after.
That changes play in any game involving Breakthrough / Frugality, so **the
fingerprint digests quoted above are stale from f4bcac0 onward**. The
throughput numbers are unaffected in any meaningful way (the change moves work
around inside a move, it does not add or remove any).

Determinism re-verified at f4bcac0, both interpreters, `nice -n 10`:

```
$ python3 -m unittest discover -s tests            ->  Ran 58 tests  OK
$ python3 -m engine.perf_check save /tmp/fp2_cpy.json
saved c2befef1bb640a05b5862627d7a1fb76134adff562fec748b044d89dc056755a (33 cases)
$ pypy3   -m engine.perf_check check /tmp/fp2_cpy.json
OK  identical behaviour: c2befef1bb640a05b5862627d7a1fb76134adff562fec748b044d89dc056755a

$ python3 -m engine.perf_check save /tmp/fp2_wide_cpy.json --wide
saved 47e06a41c8a888891a90090272374a0e9b87c237d8be103cb4db29627f4ec46d (102 cases)
$ pypy3   -m engine.perf_check check /tmp/fp2_wide_cpy.json
OK  identical behaviour: 47e06a41c8a888891a90090272374a0e9b87c237d8be103cb4db29627f4ec46d
```

**135/135 games still byte-identical across interpreters after the rules
change.** (The digests moved — `3229c4a0` -> `c2befef1`, `c7e73ede` ->
`47e06a41` — exactly as expected for a real rules change.)

**Current cross-interpreter baseline digests: narrow `c2befef1…`, wide
`47e06a41…`.**

## 4. Task 5 — copy_state optimisation

`copy_state` is ~64% of GreedyBot runtime: the bot copies the entire
`GameState` once per candidate move. Microbenchmark: `tools/bench_copy.py`
(12 mid-game 4p states, `time.process_time`, `nice -n 10`, climbs running).

Absolute copies/cpu-s drift with machine load (the same code measured 5054,
5817 and 6846 on three different days/loads), so every number below is an
**A/B pair measured back-to-back in the same minute**, and the ratio is the
result — not the absolute.

### 4a. Leaf-class fast path for `TechCard` / `WonderInProgress` — **1.55x**

A mid-game 4p state holds ~31 `TechCard`s out of ~35 dataclasses copied, so
almost all of the dataclass work is these two tiny all-scalar classes.
`_cdc`'s generic path built an intermediate dict, tested every field's type
and then `.update()`d it onto the `__dict__` that `__new__` had already
allocated. The new `_LEAF` path is `cls.__new__(cls)` plus one C-level
`dict(v.__dict__)` — no Python-level loop, no per-field type test, no
intermediate dict. The generic `_cdc` also lost its intermediate dict (dict
comprehension assigned straight onto `__dict__`), and empty list/dict get a
literal instead of a comprehension.

An import-time guard (`_check_leaf`) raises if either class ever grows a
non-scalar field, so the fast path cannot silently start sharing mutable
state with the real game.

| A/B pair (back to back) | before | after | ratio |
|---|---|---|---|
| 3 s warm / 8 s measure | 6846 copies/cpu-s (146.1 us) | **10498** (95.3 us) | **1.53x** |
| 2 s warm / 6 s measure | 5817 copies/cpu-s (171.9 us) | **9330** (107.2 us) | **1.60x** |

**Verification gate: PASSED** — 58/58 tests OK, narrow `c2befef1…` and wide
`47e06a41…` both unchanged (135/135 games byte-identical).

### 4b. How much of the copy does GreedyBot actually MUTATE? **1.6% / 5.7%**

Tool: `tools/measure_mutation.py`. At every branching GreedyBot decision it
copies the state, applies each candidate move to the copy, then structurally
diffs copy vs original (`log` and `_`-prefixed attrs excluded, exactly as
`copy_state` excludes them). Two ratios:

* **slots** — scalar leaves (dataclass fields, dict values, list items) that
  differ, over all scalar leaves copied. "How much data changed."
* **nodes** — container objects (dataclass / dict / list / set) that lie on a
  path to some change, over all containers copied. This is exactly what a
  copy-on-write state would have to clone: COW clones the spine from the root
  down to each mutation and shares everything else.

4p GreedyBot, 2 full games, 771 branching decisions, **9235 candidate moves**:

| | per candidate move | fraction |
|---|---|---|
| scalar slots copied | 395.4 | — |
| scalar slots **mutated** | **6.43** | **1.63%** |
| container nodes copied | 93.7 | — |
| container nodes **on a mutated path** | **5.37** | **5.74%** |

Per move kind (nodes on a mutated path), all 9235 candidates:

| move kind | slots changed | nodes on mutated path |
|---|---|---|
| `pol_pass` | 0.51% | 3.25% |
| `copy_tactic` | 0.67% | 2.76% |
| `destroy` | 0.76% | 5.32% |
| `take` | 0.88% | 5.83% |
| `pop` | 1.04% | 3.25% |
| `develop` | 1.17% | 5.33% |
| `play_action` | 1.36% | 4.11% |
| `offer_pact` | 1.46% | 5.37% |
| `play_tactic` | 1.67% | 4.27% |
| `choose` | 1.86% | 8.77% |
| `war` | 1.90% | 6.16% |
| `prepare_event` | 3.93% | 10.46% |
| `resign` | 7.91% | 9.23% |
| `end_turn` | 8.24% | 9.04% |

Even the worst move kind (`end_turn`, which runs the whole §6.6 end-of-turn
sequence) touches under 10% of the nodes. The common cases are 3–6%.

**The mutation is a constant, the copy is not.** Same tool at 2 players
(1 game, 123 branching decisions, 1392 candidates):

| | 2p | 4p |
|---|---|---|
| scalar slots copied | 245.6 | 395.4 |
| scalar slots mutated | **6.14** | **6.43** |
| container nodes copied | 50.1 | 93.7 |
| nodes on a mutated path | **5.38** | **5.37** |
| mutated fraction (nodes) | 10.7% | 5.7% |

The absolute mutation size is flat (~6 slots, ~5.4 nodes) while the copy grows
with the player count and with game length. So the waste ratio gets *worse* as
states get bigger — 4p late-game, the exact cell the hill climbs spend their
time in, is where a share-don't-copy scheme pays most.

### 4c. END-TO-END training throughput from the fastcopy work — **1.23x / 1.32x / 1.33x**

The 1.55x microbenchmark is the copy in isolation; what the climbs actually
gain is the *whole-game* rate. Measured with `tools/bench_interp.py`
(`time.process_time`, 2 s warm-up, 6 s measure, `nice -n 10`, climbs running),
old vs new **interleaved in the same run** — a `git worktree` at HEAD with only
`engine/bots/fastcopy.py` reverted to the pre-fastcopy version, so the A/B
isolates exactly that one file — and repeated twice:

| GreedyBot | pre-fastcopy (games/cpu-s) | leaf fast path | speed-up |
|---|---|---|---|
| 2p | 4.621 / 4.645 | 5.709 / 5.683 | **1.23x** |
| 3p | 2.091 / 2.118 | 2.797 / 2.776 | **1.32x** |
| 4p | 0.992 / 0.991 | 1.305 / 1.327 | **1.33x** |

Rep-to-rep spread is under 2%, so these are real. The 4p number is the one
that matters: the hill climbs are 4p-heavy, and **greedy 4p went 0.99 ->
1.32 games/cpu-s, a 33% throughput gain — one third more games per CPU-second
for free.** In `engine/PROGRESS.md` terms the greedy 4p cell moved 1.01
(c8a70a4) -> 1.32.

Why 1.33x end-to-end and not 1.55x: Amdahl. If copy were 64% of runtime, a
1.55x copy would give 1.29x overall — the measurement is right in line, which
independently confirms the 64% figure.

### RECOMMENDATION (short form; the full design writeup is section 6)

**The copy is ~17x more work than the mutation, so structural sharing beats
any constant-factor copy win by an order of magnitude.** The leaf fast path
above bought 1.55x; the ceiling for "copy faster" is maybe another 1.3x.
Copy-on-write or an undo stack has a theoretical ceiling near **17x** on the
copy component. Section 6 works the design, the arithmetic, the risk and the
go/no-go out in full.

## 5. Re-profile after the 1.55x fastcopy win (2026-07-26)

`nice -n 10 python3 tools/profile_bot.py --players 4 --games 10`, sampling
mode (2 ms, 806 samples over 7.7 cpu-s, GreedyBot 4p, climbs running).
Sampling — not cProfile — because cProfile's ~1 us per call would inflate
exactly the tiny hot functions (`_cv`) this is measuring. SELF % is the leaf
frame, INCL % is anywhere-on-the-stack.

| SELF % | INCL % | function | what it is |
|---|---|---|---|
| 30.0 | 47.0 | `bots/fastcopy.py:_cv` | recursive value copy |
| 17.0 | 35.4 | `bots/fastcopy.py:_cdc` | generic dataclass copy |
| 5.8 | 12.0 | `engine/effects.py:compute` | per-player stats |
| **5.7** | **10.8** | **`random.py:__init__`** | **`random.Random(0)` per candidate move** |
| 5.1 | 5.1 | `random.py:seed` | (called by the above) |
| 4.6 | 18.9 | `bots/__init__.py:evaluate` | linear eval |
| 3.6 | 50.6 | `bots/fastcopy.py:copy_state` | the copy, total |
| 2.4 | 2.4 | `engine/cards.py:level_of` | |
| 2.1 | 13.7 | `bots/__init__.py:features` | feature extraction |
| 1.7 | 1.7 | `<string>:__init__` | dataclass-generated `__init__` |
| 1.6 | 1.6 | `importlib._bootstrap:_handle_fromlist` | a function-level `import` in a hot path |
| 1.4 | 1.4 | `engine/effects.py:invalidate` | stats-cache clear |

Rolled up by area:

| area | share of GreedyBot 4p runtime |
|---|---|
| **`copy_state` (the whole copy)** | **50.6%** (was ~64% pre-fastcopy) |
| `actions.apply` of the trial move | 16.1% |
| `evaluate` (features + weights) | 18.9% |
| of which `effects.compute` + `state_stats` | 12.0% / 7.0% |
| **`random.Random(0)` construction** | **10.8%** |

Two readings:

1. **The copy is still the single biggest line item at 50.6%**, even after the
   1.55x leaf fast path. Amdahl now says a further 1.3x on the copy is worth
   only ~1.2x overall, while eliminating the copy (section 6) is worth ~2.0x.
   This is the same conclusion as 4b, now measured on the post-fastcopy code.
2. **A new #2 appeared that was hidden before: 10.8% of GreedyBot's runtime is
   spent constructing `random.Random` objects.** See 5a.

### 5a. `random.Random(0)` per candidate move — 10.8%, one-line fix, NOT MINE TO MAKE

`engine/bots/__init__.py:157`:

```python
actions.apply(trial, mv, random.Random(0))
```

That constructs a fresh Mersenne Twister **for every candidate move** — and
seeding an MT is not cheap (it initialises a 624-word state array), which is
why `random.__init__` + `random.seed` together are 10.8% inclusive / 10.8%
self of the whole bot. GreedyBot evaluates ~12 candidates per decision, so
this is ~12 MT seedings per decision that all produce the identical stream.

The fresh-object-per-candidate behaviour is **load-bearing**: each candidate
must see the same random stream from the same starting point, so a single
shared `Random` instance advanced across candidates would change the digests.
The safe fix keeps the stream exactly and only skips the seeding work:

```python
_TRIAL_RNG = random.Random(0)
_TRIAL_RNG_STATE = _TRIAL_RNG.getstate()     # module level, computed once
...
_TRIAL_RNG.setstate(_TRIAL_RNG_STATE)        # per candidate, replaces Random(0)
actions.apply(trial, mv, _TRIAL_RNG)
```

`setstate` restores byte-identically the state a freshly-constructed
`Random(0)` has, so the stream every candidate sees is unchanged — this is a
provably digest-preserving rewrite, not an approximation. `setstate` is a
C-level copy of the state tuple; `seed()` is `init_by_array`. Expect to
recover most of the 10.8%.

**Not applied here**: `engine/bots/__init__.py` is off limits to this pass
(the `math.fsum` in `evaluate` is load-bearing for determinism and the file is
another agent's). This is written up as a one-line change for its owner, with
the measurement above as justification. It is the best
effort-to-payoff ratio left on the table.


## 6. Copy-on-write / undo stack — full design writeup and go/no-go

*This section is the deliverable asked for: the design, the expected gain, the
risk, and a recommendation. It is deliberately NOT implemented. Judge it
first.*

### 6.1 The case, in one paragraph

GreedyBot copies the entire `GameState` once per candidate move, then throws
the copy away microseconds later. Section 4b measured what that copy is for:
**6.43 scalar slots and 5.37 container nodes change per candidate move, out of
395.4 slots and 93.7 nodes copied.** That is **17x more container nodes copied
than touched, 61x more scalar slots**, and section 4b showed the mutation size
is *flat* (~6 slots at 2p and at 4p) while the copy grows with player count and
game length. The copy is 50.6% of runtime (section 5) and the work it does is
98.4% dead. No constant-factor improvement to the copier addresses that; only
not copying does.

### 6.2 Design A — undo stack (journalling `apply`). PREFERRED.

GreedyBot's use is `copy -> apply(mv) -> evaluate -> discard`. It never holds
two trial states at once and never needs the trial to outlive the `evaluate`
call. So it does not need persistence at all — it needs `apply` to be
*reversible*:

```python
j = journal.begin(state)
try:
    actions.apply(state, mv, rng)
    val = evaluate(state, ...)
finally:
    journal.rollback(j)        # state is bit-identical to before
```

The journal is a plain list of undo records, appended by every write:

| write | record | undo |
|---|---|---|
| `obj.attr = v` | `(0, obj.__dict__, 'attr', old)` or `_MISSING` | restore or `del` |
| `d[k] = v` | `(0, d, k, old)` or `_MISSING` | restore or `del` |
| `lst.append(x)` | `(1, lst)` | `lst.pop()` |
| `lst.pop()/insert/remove/del` | `(2, lst, index, old)` | re-insert / restore |
| `lst.sort()/reverse()/slice-assign` | `(3, lst, list(lst))` | `lst[:] = old` |
| `set.add/discard` | `(4, s, x, was_present)` | inverse |

At 6.43 mutated slots per candidate the journal is **~7 records per move**
versus 395 slot copies and ~35 object allocations. Rollback is a reversed walk
of ~7 records. Both are O(mutation), not O(state) — which is exactly the shape
section 4b says the problem has.

Mechanically the cheapest form is a tiny helper module `engine/journal.py`
exporting `setattr_`, `setitem`, `append`, `pop`, ... that are no-ops
(direct writes) when journalling is off — a module-global `_J = None` test,
one branch. That keeps the non-search path (the real game, `play_game`,
`experiments/`) at essentially current speed; only GreedyBot turns journalling
on.

### 6.3 Design B — copy-on-write with a version stamp

Give every container a version stamp; `mutable(obj)` clones it into the current
generation if its stamp is stale and rewires the parent pointer. Only the
~5.4 nodes on the mutated path get cloned; the other ~88 are shared.

Cost versus A: every *read* path stays as-is but every *write* path needs a
`mutable()` call **and** the parent chain must be reachable (a `GameState`
today has no parent pointers, so nodes need back-references or every write
needs a root-relative path). It also makes true aliasing bugs possible — two
logical states sharing one dict, where a missed `mutable()` silently corrupts
the *real* game rather than just the trial.

Design B's only advantage over A is that it supports holding **many** trial
states alive simultaneously, i.e. real multi-ply search (minimax/MCTS), which
an undo stack cannot do. Today no bot needs that.

### 6.4 Expected speed-up — the arithmetic

From section 5, `copy_state` is **50.6%** of GreedyBot 4p runtime.

| scenario | copy component | whole-bot speed-up | greedy 4p games/cpu-s |
|---|---|---|---|
| today (post-fastcopy) | 50.6% | 1.00x | 1.32 |
| another 1.3x on the copier (the realistic ceiling for "copy faster") | 38.9% | 1.19x | ~1.57 |
| journal, optimistic (copy -> 0%) | 0% | **2.02x** | ~2.67 |
| journal, realistic (journal+rollback costs ~1/10 of the copy, plus a branch on every write) | ~5% | **1.83x** | ~2.4 |

So: **expect ~1.8x end-to-end on the cell the hill climbs actually run**, and
more than that at 4p late game, because section 4b showed the wasted fraction
*grows* with state size while the mutation stays flat. Stack it with the
`random.Random(0)` fix (5a, another ~1.1x) and greedy 4p plausibly reaches
~2.6-2.9 games/cpu-s versus 0.99 before this perf pass started — roughly 3x.

It also helps PyPy disproportionately (section 3 blamed PyPy's loss partly on
short-lived-object churn its GC handles worse than CPython's refcounting), so
the PyPy verdict is explicitly worth re-testing after this change and not
before.

### 6.5 The risk, stated honestly

**The binding constraint is that the digests must not move.** narrow
`c2befef1…` / wide `47e06a41…`, 135 games, byte-identical logs and scores. A
change that alters them is a bug in the change, not a new baseline. Specific
hazards, in descending order of how likely they are to bite:

1. **A missed mutation site.** ~385 candidate mutation sites exist across
   `actions.py` (247 attribute writes), `effects.py`, `events.py`,
   `economy.py`, `game.py`, `interact.py`, plus 107 list/dict mutator calls,
   29 subscript assignments and 2 `del`s. Every single one that touches state
   during a trial `apply` must be journalled. One miss = the *real* game state
   is silently corrupted by a bot's hypothetical move. This is the whole risk.
2. **`state.log`.** `copy_state` deliberately *drops* the log, so today trial
   moves cannot touch it. Under journalling the trial `apply` calls `emit()`
   on the real log — and `emit` truncates (`del self.log[:100]` past 400
   entries), which is destructive. The log is *in the fingerprint digest*, so
   this must be handled explicitly (suppress `emit` during trials, which is
   also what the copy path effectively does today).
3. **Dict/list ordering.** LIFO rollback restores insertion order exactly
   (delete-then-reinsert only happens in the reverse of the order it occurred),
   so ordering is safe *if and only if* rollback is strictly LIFO. Any
   out-of-order rollback silently reorders `p.techs`, which the engine iterates.
4. **The stats cache.** `_stats_cache` is `_`-prefixed and therefore not
   copied today; each trial gets a clean cache. Under undo the *real* state's
   cache is polluted by trial computes and must be invalidated (or restored)
   on rollback. `invalidate` is only 1.4% of runtime, so clearing on rollback
   is an acceptable and much safer choice than trying to restore it.
5. **Exceptions mid-`apply`.** `STRICT` legality asserts and illegal-move
   paths raise from the middle of a mutation sequence. Rollback must be in a
   `finally`, and it must be correct from a *partial* journal.
6. **Non-search callers.** `experiments/`, `analysis/` and `WeightedBot` also
   call `copy_state`; the journal must be opt-in so they are unaffected.

Mitigation that makes this tractable, and it is a strong one: a **paranoid
mode** that does both — `copy_state` the state as today, run the journalled
apply, roll back, and structurally diff the rolled-back state against the copy,
raising on any difference. `tools/measure_mutation.py` already contains the
structural differ needed. Run the 135-game fingerprint suite under paranoid
mode and every mutation site that matters in real play is exercised and
checked; then turn paranoid mode off for production. That converts "did I find
all 385 sites?" from an audit question into a test question.

### 6.6 GO / NO-GO

**GO — but as its own branch, with the paranoid differ written FIRST, and not
inside a perf pass.**

Reasoning:
* The prize is real and large: ~1.8x on the exact cell the hill climbs run,
  and it is the only remaining change of that size. Everything else left is
  1.05-1.2x.
* The measurement supporting it is not a guess: 9235 candidate moves measured,
  flat ~6-slot mutation against a 395-slot copy, confirmed at two player
  counts, and the 50.6% copy share re-confirmed post-fastcopy.
* The risk is concentrated in one failure mode (a missed mutation site) that
  has a mechanical, complete detector (paranoid diff + the 135-game
  fingerprint). That is an unusually good risk profile for a change this size.
* It is reversible: the journal helpers are additive, and `copy_state` stays
  in the tree as the fallback and as the paranoid-mode oracle.

Conditions on the GO:
1. Design **A (undo stack)**, not B. B's extra capability (many live trial
   states) has no consumer today and it carries real-corruption risk instead of
   trial-only risk.
2. `engine/journal.py` + the paranoid differ land and pass on 135 games
   **before** any call site is converted.
3. Convert mutation sites module by module, running the fingerprint after each
   module. Do not convert all six in one commit.
4. Hard gate at every step: 58 tests green **and** narrow `c2befef1…` / wide
   `47e06a41…` unchanged. Digest movement = revert.
5. Do it while the hill climbs are quiescent, or at minimum never on the
   checkout they are reading.

**NO-GO on Design B** unless and until a bot needs simultaneous live trial
states (multi-ply search). Revisit then, and reuse the journal for the
single-ply case regardless.

## 7. Task 5 continued — exec-generated per-class copiers (commit c54f36b)

Guided by section 5 (the copy is still 50.6% of GreedyBot 4p), the next
constant-factor win goes after what the copier *decides* rather than what it
copies. Per state copy the old code made ~209 per-field and ~115 per-element
`type()` + frozenset probes, all of which are a pure function of the class.
They are now decided once at import and baked into a straight-line
`exec`-generated copy function per dataclass — the trick `dataclasses` uses
for `__init__`. Field plans:

| plan | applies to | generated code |
|---|---|---|
| scalar | annotation is `int`/`str`/`bool`/`float` or `X \| None` | `d['x']` (shared) |
| atomic container | registry: decks, hands, event lists, `seeded_by`, … | `list(d['x'])` / `dict(d['x'])` — one C call |
| dataclass container | `GameState.players`, `PlayerState.techs` | comprehension calling that class's generated copier, one `type() is` guard per element |
| generic | everything else (`pacts`, `pending`, `one_time_discount`, …) | `_cv(d['x'])`, unchanged |

The atomic-container registry is the only claim annotations cannot verify (the
fields are annotated bare `list`/`dict`), so it gets three guards: a
`len(__dict__)` check that falls back to the fully generic copier if the
instance does not match its class schema (the one tolerated deviation is
`effects`' private `_stats_cache`), per-element `type()` guards on the
dataclass containers, and a new **paranoid mode** — `FASTCOPY_PARANOID=1`
verifies every atomic-container element is immutable and raises otherwise.

### Measurement

`tools/bench_copy.py`, `nice -n 10`, A/B back to back against a `git worktree`
at the pre-change commit, twice:

| copy microbenchmark | leaf fast path (4a) | generated | ratio |
|---|---|---|---|
| rep 1 | 11306 copies/cpu-s (88.45 us) | **14209** (70.38 us) | **1.26x** |
| rep 2 | 11494 copies/cpu-s (87.01 us) | **13985** (71.51 us) | **1.22x** |

Cumulative on `copy_state` since the perf pass began: 6601 -> 14100
copies/cpu-s = **2.14x**.

End-to-end, `tools/bench_interp.py` (2 s warm / 6 s measure, `nice -n 10`,
climbs running), same worktree A/B, twice:

| GreedyBot | leaf fast path | generated | speed-up |
|---|---|---|---|
| 2p | 5.739 / 5.790 games/cpu-s | 6.458 / 6.531 | **1.13x** |
| 3p | 2.864 / 2.872 | 3.218 / 3.286 | **1.13x** |
| 4p | 1.317 / 1.332 | **1.494 / 1.500** | **1.13x** |

Rep-to-rep spread under 1.5%. 1.24x on a 50.6% component predicts 1.11x
overall; 1.13x measured, so Amdahl is again consistent and the 50.6% figure is
independently confirmed.

**Greedy 4p is now 1.50 games/cpu-s, from 0.99 before the fastcopy work —
1.51x cumulative on the cell the hill climbs actually run.**

Gate: 58/58 tests OK; narrow `c2befef1…` and wide `47e06a41…` unchanged, and
**also unchanged under `FASTCOPY_PARANOID=1`** — 135 games of real play with
every atomic container element-checked, no aliasing found.

## Status / next steps (keep current) — updated

- [x] Task 1-4 — see the checklist above; **NO interpreter switch.**
- [x] Task 5a — re-profile after the 1.55x fastcopy win (section 5). Copy is
      still #1 at 50.6%; `random.Random(0)` per candidate is a new #2 at 10.8%.
- [x] Task 5b — copy-on-write / undo design writeup + go/no-go (section 6).
      **GO on design A (undo stack), as its own branch, paranoid differ first.**
- [x] Task 5c — exec-generated per-class copiers, 1.24x copy / 1.13x
      end-to-end (section 7, commit c54f36b).
- [x] **DONE (section 8)** — the `random.Random(0)` per candidate move.
      Owner-authorised and applied, but as a *lazy* reseed, not 5a's plain
      `setstate`: measured 1.07x, and the item was ~6% of runtime, not the
      13.6% the sampling profiler claimed. See 8.1 for the correction.
- [ ] **Owner action, same file** — `features()` does `from .. import cards as C`
      / `from .. import economy` *inside the function*, i.e. once per candidate
      move; `importlib._bootstrap._handle_fromlist` is 1.6% of runtime. Hoist to
      module level if the import cycle allows, else bind once lazily.
- [ ] Next constant-factor targets, in profile order after this change:
      `effects.compute` (12.0%), `evaluate`/`features` (18.9%), and the
      remaining generic `_cv` paths (`pacts`, `pending`, `queue`,
      `one_time_discount`, `discarded_military`).
- [ ] The real prize remains section 6: the undo stack, ~1.8x, on its own
      branch. Re-test PyPy *after* that lands, not before.

### 7a. Re-profile after the generated copiers, and one more copy pass (11bb52c)

Same tool/method as section 5 (`--players 4 --games 10`, 735 samples, 6.9 cpu-s):

| SELF % | INCL % | function |
|---|---|---|
| 17.0 | 32.7 | `<fastcopy:PlayerState>:_copy_PlayerState` |
| 14.2 | 14.4 | `bots/fastcopy.py:_cv` (the remaining generic paths) |
| 7.9 | **43.8** | `<fastcopy:GameState>:_copy_gs_nolog` — **the whole copy** |
| 7.8 | **13.6** | `random.py:__init__` + `seed` — `random.Random(0)` per candidate |
| 5.0 | 24.0 | `bots/__init__.py:evaluate` |
| 4.8 | 13.2 | `engine/effects.py:compute` |
| 4.5 | 4.5 | `<fastcopy:TechCard>:_copy_TechCard` |
| 2.5 | 18.5 | `bots/__init__.py:features` |
| 2.2 + 1.4 | — | `importlib._bootstrap:_handle_fromlist` / `:parent` — function-level imports |

The copy fell 50.6% -> **43.8%**, and `random.Random(0)` construction rose to
**13.6%**: it is now unambiguously the largest single fixable item, and it
lives in the one file this pass may not touch (see 5a).

`_cv`'s remaining 14.4% was the generic recursive path for the handful of
fields with no plan. Two more plans close most of it: `discarded_military`
(dict of name-lists) and `one_time_discount` (dict of scalar dicts) become
`{k: list/dict(x) ...}`, and every remaining generic field short-circuits its
empty/`None` case with a walrus test instead of paying for a `_cv` call —
`pacts`, `pending`, `queue`, `wonder` and `final_scores` are empty or `None`
in nearly every copy.

| A/B, back to back, twice | before | after | ratio |
|---|---|---|---|
| `bench_copy` rep 1 | 13986 copies/cpu-s | **15861** | 1.13x |
| `bench_copy` rep 2 | 14535 | **15913** | 1.09x |
| `bench_interp` greedy 4p rep 1 | 1.506 games/cpu-s | 1.522 | 1.01x |
| `bench_interp` greedy 4p rep 2 | 1.492 | 1.625 | 1.09x |

**1.11x on the copy; end-to-end ~1.05x** — the two end-to-end reps straddle
the Amdahl prediction (1.11x on a 43.8% component predicts 1.05x) and the
spread between them is larger than the effect, so 1.05x is the honest number
and the microbenchmark is what actually resolves this change. Diminishing
returns on the copier are now obvious: 1.55x, then 1.24x, then 1.11x.

Gate: 58/58 tests, narrow `c2befef1…` / wide `47e06a41…` unchanged, unchanged
under `FASTCOPY_PARANOID=1`.

**Cumulative for the whole perf pass: greedy 4p 0.99 -> ~1.55 games/cpu-s,
`copy_state` 6601 -> 15900 copies/cpu-s (2.4x).** The copier is done; the next
real step is section 6's undo stack.

## 8. The `random.Random(0)` fix — APPLIED, and the profiler was wrong about it

Owner-authorised change to `engine/bots/__init__.py` (5a). Applied, but **not
in the form 5a proposed**, because an in-process A/B showed the proposed form
gains essentially nothing. Both the correction and the measurements are here so
the profile in 5/7a is not trusted uncritically again.

### 8.1 What the A/B actually measured

Method: one process, `GreedyBot.pick` monkeypatched between arms, 4 games of 4p
greedy (seeds 0-3), `process_time`, arms alternated, 3 reps, `nice -n 10` with
the climbs running. Reported per *candidate move* because all arms produce the
identical 18003 candidates — the rng change does not alter the games.

| arm | us/candidate (best of 3 reps) | vs `Random(0)` |
|---|---|---|
| `random.Random(0)` per candidate (before) | 114.8 | 1.00x |
| 5a's `setstate(frozen)` per candidate | 118.1 (rep-best 114.8-150) | **~1.00x** |
| shared rng, never reset (perf probe, not legal) | 110.6 | 1.04x |
| **lazy reset — reseed only if actually drawn from (SHIPPED)** | **107.4** | **1.07x** |

Two corrections to the earlier profile fall out of this:

1. **The item was ~6%, not 13.6%.** The upper bound on the whole thing is the
   "shared rng" probe — remove the per-candidate rng entirely and you get 4-6%,
   not 13.6%. The 2 ms sampling profiler over-attributed `random.__init__` /
   `random.seed`: they are short C-heavy frames that the sampler catches
   disproportionately. Cross-check by arithmetic: `timeit` puts `Random(0)` at
   **9.37 us** and 18003 candidates over 4 games is 0.169 cpu-s of 2.43, i.e.
   **6.9%** — consistent with the probe, not with 13.6%.
2. **`setstate` is not much cheaper than `seed`**: `timeit` says 6.48 us versus
   9.37 us, only 1.4x, because restoring also walks the 625-element state
   tuple. 5a assumed it was a cheap C memcpy. Saving 2.9 us of a 115 us
   candidate is 2.5% in theory and unmeasurable in practice — which is exactly
   what the A/B found.

### 8.2 What shipped instead

The engine's *only* use of the rng anywhere is `rng.shuffle` (5 sites). Counted
directly: **a trial `apply` draws from the rng in 69 of 18003 candidates —
0.4%.** So the reseed is nearly always reseeding a Twister that nothing touched.

`_TrialRandom(random.Random)` sets a `used` flag in `random()` and
`getrandbits()` — the two C-level entry points every other method
(`shuffle`, `choice`, `randrange`, `sample`, `randbytes`, the variates) is built
on, so no draw can escape the flag — and `pick` reseeds from the frozen
`getstate()` snapshot **only when `used` is set**. An untouched Twister is
byte-identical to a fresh `Random(0)`, so every candidate still sees the
`Random(0)` stream from its start: the equivalence is exact, not statistical.

Cost in the common case is one class-attribute load. Thread-safety is not lost
in practice — the harnesses are `multiprocessing`, never threads.

**Result: ~1.07x end-to-end on greedy 4p (115 -> 107 us/candidate), and the
remaining rng cost is now ~3%, of which the irreducible part is the 0.4% of
candidates that genuinely draw.**

Gate: 58/58 tests, narrow `c2befef1…` / wide `47e06a41…` unchanged, unchanged
under `FASTCOPY_PARANOID=1`.

### 8.3 Standing lesson

The sampling profiler's attribution for *small, frequently-entered C frames* is
inflated. Before spending effort on a profile line item, bound it with a probe
that deletes the work entirely, or with `timeit` x call-count arithmetic. Had
5a been shipped as written it would have been a 0% change sold as 13.6%.

## 9. The undo stack — branch `journal-undo` (IN PROGRESS)

Section 6's design A, implemented on its own branch per the 6.6 conditions.
**Nothing here is on master and nothing here should be merged until the whole
sequence is green.** Work in the worktree `/Users/pt/tta-ai-journal`.

### 9.0 A trap found before any code was written: the fingerprint files are STALE

`python3 -m engine.perf_check check tools/fingerprint.json` reports
**MISMATCH on a completely untouched HEAD**. This is not a regression:

* `tools/fingerprint.json` / `tools/fingerprint_wide.json` were last saved at
  commit `7c2eef1`, with digests `3229c4a0…` / `c7e73ede…`;
* legitimate behaviour changes landed afterwards without a re-save;
* `check` prints `MISMATCH <computed> != <wanted>`, and the **computed** value
  is exactly the documented `c2befef1…` / `47e06a41…`.

So the files are the stale side, not the code. Anyone gating on them reads a
false failure and is one step away from "fixing" a non-bug, or from re-saving
the files and thereby blessing whatever regression they were carrying.

**Use `bash tools/gate.sh`** (added on this branch). It gates on the digests
written down here, and runs all four arms — 58 tests, narrow, wide, and both
fingerprints again under `FASTCOPY_PARANOID=1` — in one command.

Verified baseline of this branch (= master `6376981`, so commit `6376981`'s
WeightedBot `state.decider()` fix does **not** move the greedy fingerprints,
as predicted):

```
unittest                  OK   Ran 58 tests
narrow fingerprint        OK   c2befef1bb640a05
narrow FASTCOPY_PARANOID  OK   c2befef1bb640a05
wide fingerprint          OK   47e06a41c8a88889
wide FASTCOPY_PARANOID    OK   47e06a41c8a88889
```

Full wide digest, previously only recorded to 8 chars: `47e06a41c8a88889…`.

#### Baseline RE-DERIVED after rebasing onto master `afb1b6c` (2026-07-26)

Master moved under this branch while it was parked: `0808b64` and `166867d`
added deferred-payoff / yield-aware features, and `6376981` changed which
player `WeightedBot` scores. Those are real behaviour changes, so the
baseline had to be re-derived rather than assumed — a digest that moved
because of a *rebase* is not a bug in the undo stack and must not be chased.

Rebased `journal-undo` onto `afb1b6c`, then computed the digests from scratch
on **both** worktrees (master at `afb1b6c`, and the rebased branch):

| | master `afb1b6c` | journal-undo (rebased) |
|---|---|---|
| narrow (33 games) | `c2befef1bb640a05` | `c2befef1bb640a05` |
| wide (102 games) | `47e06a41c8a88889` | `47e06a41c8a88889` |
| unittest | OK, 58 tests | OK, 115 tests |

**The baseline is unchanged**, and the reason is worth writing down so nobody
re-derives it again in a panic: the fingerprint plays **GreedyBot only**.
`0808b64` / `166867d` / `6376981` all changed the *feature vector* and
`WeightedBot`, neither of which GreedyBot's evaluation goes through. So
`c2befef1…` / `47e06a41…` remain the correct gate, and `tools/gate.sh` needs
no edit.

Full digests for the record:

```
narrow c2befef1bb640a05b5862627d7a1fb76134adff562fec748b044d89dc056755a
wide   47e06a41c8a888891a90090272374a0e9b87c237d8be103cb4db29627f4ec46d
```

Corollary for whoever gates next: a fingerprint that moves after a rebase
should first be re-derived **on the merge-base of master**, not debugged. If
master's digest and the branch's digest agree, the branch is clean whatever
the number is.

#### Re-derived AGAIN after rebasing onto master `15b9764` (2026-07-26)

Master moved twice more while the branch was parked: `af114aa` (docs only) and
`15b9764` (resets the `colonies`/`pacts` weights in the three
`experiments/champion_*.json`). Per the rule above the digests were re-derived
from scratch **on the master worktree at `15b9764`** before the branch was
trusted:

```
narrow c2befef1bb640a05b5862627d7a1fb76134adff562fec748b044d89dc056755a
wide   47e06a41c8a888891a90090272374a0e9b87c237d8be103cb4db29627f4ec46d
```

**Still unchanged**, and again for a structural reason rather than luck:
`grep champion engine/perf_check.py engine/bots/*.py` is empty — the
fingerprint constructs its bots directly and never loads a champion file, so
no amount of hill-climb weight movement can touch it. Combined with 9.0's
finding (GreedyBot-only, so `WeightedBot`/feature-vector commits are inert),
the fingerprint is insensitive to *every* kind of change master has made so
far during this branch's life. `tools/gate.sh` still needs no edit.

### 9.1 Step 1 — the paranoid structural differ (commit 5f168fb, DONE)

6.6 condition 2: differ first, no call site converted. `engine/statediff.py`
returns the **path** to every structural difference between two states.

It is deliberately stronger than `==` in exactly one place: **it compares dict
key order.** `{'a':1,'b':2} == {'b':2,'a':1}` is `True`, but the engine
*iterates* `p.techs`, `state.seeded_by` and `p.one_time_discount`. A non-LIFO
rollback that restores the right values in the wrong insertion order changes
real play while comparing equal — hazard 3 of 6.5, invisible to `==`, and the
single most likely way this project ships a silent corruption. There is a test
for the concrete form (pop a key, put it back, it lands at the end).

`tests/test_statediff.py`: 31 tests, one per row of the 6.2 undo-record table,
all asserting **detection** rather than agreement. Plus a test that
`copy_state` is a faithful oracle at every decision of a 120-move game — if
that ever fails, the paranoid check is comparing against a broken oracle and
proves nothing. Test count is now 89; the original 58 are untouched.

### 9.2 Correction to 6.5's site count: 470, not ~385

AST count of writes that could touch state, over the eight engine modules:

| module | attr writes | subscript writes | mutator calls | `del` | total |
|---|---|---|---|---|---|
| actions.py | 81 | 12 | 47 | 1 | 141 |
| effects.py | 54 | 18 | 13 | – | 85 |
| interact.py | 22 | 7 | 35 | 1 | 65 |
| game.py | 56 | 2 | 5 | – | 63 |
| events.py | 45 | – | 5 | – | 50 |
| economy.py | 26 | 1 | 5 | – | 32 |
| cards.py | 13 | 6 | 6 | – | 25 |
| state.py | 3 | – | 5 | 1 | 9 |
| **total** | **300** | **46** | **121** | **3** | **470** |

(Upper bound — some targets are locals, e.g. the `Stats` accumulator in
`effects.py`, not reachable state.) The shape that matters: **attribute writes
are 300 of 470, 64% of the risk.**

### 9.3 The measurement that changes the design: journal attrs via `__setattr__`

6.2 assumed every one of those 470 sites gets hand-converted to
`journal.setattr_(obj, 'attr', v)`. That is 470 chances to miss one, and 470
lines of the engine made unreadable. There is a much better option for the
300 attribute writes — a journalling `__setattr__` on the four state
dataclasses — **if** it is affordable. Two probes say it is:

| probe | result |
|---|---|
| cost of a Python-level `__setattr__` on a dataclass | 93.3 ns → 600.3 ns, **6.4x per write** |
| attribute writes performed by one *trial* `apply` (4p greedy, 3179 candidates) | **3.8** |

6.4x sounds fatal and is not: 3.8 writes × ~0.5 us = **~2 us per candidate
against a ~107 us candidate, i.e. ~2%**, versus the ~44% the copy costs. The
per-write cost is irrelevant because `apply` barely writes; it *reads* and
*computes*. (Consistent with 4b's 6.43 mutated slots per candidate — the rest
of the 6.43 are container slots.)

So **300 of 470 sites (64%) need no call-site change at all and carry zero
miss risk** — a `__setattr__` cannot be forgotten. The hand-converted surface
drops to the 170 container mutations (subscripts, `append`/`pop`/…, `del`),
which are also the ones that `grep` finds reliably.

One wrinkle, already checked: the generated copiers assign `n.__dict__ = {…}`
wholesale rather than field by field, so a journalling `__setattr__` fires
**once per copied object, not once per field** — ~35 calls per `copy_state`,
not ~400. And while the journal is on, `copy_state` is not running at all.

### 9.4 Next steps — resume here

- [x] Step 1: `engine/statediff.py` + 31 detection tests + `tools/gate.sh`
      (commit 5f168fb). Gate green.
- [ ] Step 2: `engine/journal.py` — `begin`/`rollback`, journalling
      `__setattr__` for the 4 state dataclasses, container helpers for the
      170 hand sites, and `JOURNAL_PARANOID=1` (copy_state oracle + statediff
      on every rollback). Land with tests, **still converting no call site.**
- [ ] Step 3: `emit()`. `copy_state` drops the log, so a trial apply must not
      touch `state.log` — suppress `emit` while journalling, which is what the
      copy path already does in effect. The log is in the digest.
- [ ] Step 4: `_stats_cache` — clear on rollback (`invalidate` is 1.4%; much
      safer than trying to restore it).
- [ ] Step 5: convert containers module by module — `actions.py`, then
      `effects.py`, `interact.py`, `game.py`, `events.py`, `economy.py` — with
      `bash tools/gate.sh` after **each** module, plus a run under
      `JOURNAL_PARANOID=1`.
- [ ] Step 6: coverage check. The paranoid diff only proves the sites that the
      135 games actually *execute*. Run the paranoid suite under `coverage.py`
      and confirm every mutating line in the engine was reached; any unreached
      mutating line is an unverified site and must be audited by hand. **This
      is the residual risk 6.5 did not name** and it must not be skipped.
- [ ] Step 7: flip GreedyBot from `copy_state` to journal, measure, gate.
