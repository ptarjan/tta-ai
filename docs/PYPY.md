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

**SUPERSEDED IN PART — see section 11.** The second condition happened (the
undo stack, section 9) and the re-test was run. "PyPy loses every single cell"
is **no longer true**: PyPy now wins GreedyBot by 1.45-1.65x and PlanBot by
1.12-1.24x. The *decision* is unchanged, but for a different reason — the
league stopped running GreedyBot and now runs WeightedBot pools with
`plan`/`quiescent` candidates, and PyPy loses those.

## Status / next steps (keep current)

- [x] Task 1 — determinism re-verified, 33/33 + 102/102 identical. PASS.
- [x] Task 2 — steady-state games/s table (8 s warm-up). **PyPy loses every cell.**
- [x] Task 2b — greedy cells re-run with a 45 s warm-up. PyPy still 17–23% behind.
- [x] Task 3 — core scaling / worker count: 6 physical cores, no SMT -> 4 workers.
- [x] Task 4 — **NO SWITCH.** Climbs stay on CPython 3.14.6, untouched.
- [ ] Task 5 — further engine optimisation (favouring both runtimes).
- [x] **Re-test PyPy after the undo stack lands — DONE, section 11.**
      Still no switch for the league, but the per-cell picture inverted for
      GreedyBot and PlanBot. Read section 11 before quoting anything from
      section 3.

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
- [x] The real prize remains section 6: the undo stack, ~1.8x, on its own
      branch. Re-test PyPy *after* that lands, not before. **DONE — the undo
      stack landed (`17c03ea`, `47c0e5b`, `ae20f2b`) and the re-test is
      section 11.** The answer is now bot-dependent: PyPy wins GreedyBot
      (1.45-1.65x) and PlanBot (1.12-1.24x) and loses WeightedBot (0.82-0.97x),
      so the league still stays on CPython. Section 3's "PyPy loses every cell"
      is retired; its *conclusion* survives, for a different reason.

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

**Stale as of 9.18**: "insensitive to every kind of change" stopped being true
twice more after this was written — an actual `engine/actions.py` rules change
moved all four digests. See 9.18 for the current values; this section stays
as the historical record of what was true up to `15b9764`.

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

### 9.4 Steps 3 and 4 — `emit()` and `_stats_cache` (DONE)

**Step 3, `emit()` (hazard 2).** `engine/state.py` grows a module-global
`SUPPRESS_LOG`, set by `journal.begin` and cleared by `journal.rollback`;
`GameState.emit` returns immediately while it is set. This *reproduces the
copy path's behaviour exactly* rather than inventing new behaviour: today
`copy_state` hands the search a state whose `log` is a fresh `[]`, so a trial
move's log lines are created and thrown away. Two facts make suppression the
right answer rather than journalling the log:

* `emit` is **destructive**, not just append-only — past 400 entries it does
  `del self.log[:100]`. Under the undo stack that deletion would hit the real
  game's log, and the log is inside the fingerprint digest.
* Journalling the log would mean snapshotting a 400-element list per candidate
  move, which is as much copying as the whole undo stack exists to remove.

Nothing reads `state.log` during play — `grep -rn '\.log\b' engine/ analysis/
experiments/ tools/` finds only `perf_check.py` (the digest) and
`tools/dump_game.py` — so suppression cannot change control flow.

The paranoid oracle was tightened at the same time: it now copies with
`keep_log=True` and diffs with `include_log=True` (verified that
`copy_state(st, keep_log=True)` really does build a *new* list, so the check
is not vacuous). A regression in suppression therefore surfaces as an
immediate `AssertionError` naming `log`, rather than as a fingerprint
mismatch half an hour later. There is a test that injects exactly that
regression and asserts it is caught.

**Step 4, `_stats_cache` (hazard 4).** Already landed with step 2: `rollback`
does `st.__dict__.pop("_stats_cache", None)`. `_`-prefixed fields are not
copied today, so each trial currently gets a clean cache; under the undo stack
the *real* state's cache would be polluted by trial computes. `invalidate` is
1.4% of runtime, so dropping is far cheaper than the risk of restoring it.

Tests: 5 new (`LogSuppression`), 120 total, gate green on all four arms.

### 9.5 Next steps — resume here

- [x] Step 1: `engine/statediff.py` + 31 detection tests + `tools/gate.sh`.
- [x] Step 2: `engine/journal.py` — `begin`/`rollback`, journalling
      `__setattr__` for the 4 state dataclasses, `touch()` for containers,
      `JOURNAL_PARANOID=1` (copy_state oracle + statediff on every rollback).
      Landed with 26 tests, **no call site converted.**
- [x] Step 3: `emit()` suppression (above).
- [x] Step 4: `_stats_cache` cleared on rollback (above).
- [x] Step 5: **DONE** (9.9, commits 5a-5f). Convert containers module by
      module — `actions.py`, then
      `effects.py`, `interact.py`, `game.py`, `events.py`, `economy.py` — with
      `bash tools/gate.sh` after **each** module, plus a run under
      `JOURNAL_PARANOID=1`.
- [x] Step 6: **DONE** (9.10) — 61/61 converted sites proven executed, via
      `tools/mutation_coverage.py`; the 7 that self-play never reaches got
      targeted rollback tests. (Original wording: the paranoid diff only
      proves the sites the 135 games actually *execute*; any unreached
      mutating line is an unverified site and must be audited by hand. **This
      is the residual risk 6.5 did not name** and it must not be skipped.
      `coverage.py` turned out not to be installable — PEP 668 — so the tool
      uses `sys.monitoring` instead.)
- [x] Step 7: **DONE** (9.12) — 1.75x measured on 4p greedy; gate 10/10 (9.11).

### 9.6 Rebase onto master `6d0247c` — the baseline MOVED, legitimately

The fourth rebase of this branch, and the first one that is **not** inert.
9.0's rule ("re-derive on the merge-base before debugging anything") earned
its keep here.

`git diff 15b9764 6d0247c -- engine/` is mostly additive (`bots/book.py`,
`bots/quiescent.py`, `bots/variants/*`, `weighted.py`) — none of which
GreedyBot goes through — **plus 11 lines in `effects.py`**:

```python
s.science   = max(0, s.science)
s.culture   = max(0, s.culture)
s.food      = max(0, s.food)
s.resources = max(0, s.resources)
s.strength  = max(0, s.strength)
```

the rulebook's "Limits on Ratings" applied to every rating rather than only
happiness. `compute` *is* on GreedyBot's evaluation path, so the fingerprint
had to move. Re-derived from scratch on a clean detached worktree of master
`6d0247c` and, separately, on the rebased branch:

| | master `6d0247c` | journal-undo (rebased) |
|---|---|---|
| narrow (33 games) | `6f5c72ef7c011cf7` | `6f5c72ef7c011cf7` |
| wide (102 games) | `7814c5c9c276b0a2` | `7814c5c9c276b0a2` |

```
narrow 6f5c72ef7c011cf747d9a8870391fb4c8f4503de42860316bb6c1b59ce379bcf
wide   7814c5c9c276b0a2229b6b58143351c2ad1a1058f283db70d1d9a50d5448e8ce
```

**The two sides agreeing is the entire proof; the value itself proves
nothing.** `tools/gate.sh` now gates on these. The old pair
(`c2befef1…` / `47e06a41…`) was correct up to master `15b9764` and is dead —
anyone still quoting it, including the task description that sent me here, is
quoting a stale number. That is the 9.0 trap, third occurrence.

**Stale as of 9.18, fourth occurrence of the same trap**: `6f5c72ef…` /
`7814c5c9…` were themselves superseded twice more — first (never recorded
here, only in `tools/gate.sh`'s own comments) by the combat-audit rules fixes
at master `4886b65` (WIDE only: `7814c5c9…` → `a966d158…`), then by the
coverage-audit rules fixes at master `3439b0e` (both NARROW and WIDE moved
this time). See 9.18 for the current values and the two-sided derivation.

### 9.7 GreedyBot's journal path was wired BEFORE step 5, on purpose

6.6 condition 3 says convert module by module; 9.5 step 6 says the residual
risk is *coverage* — sites the 135 games never execute. But there is a second
problem with converting first and testing last: with nothing calling the
journal, a per-module gate only proves the `touch()` calls are inert, which
they trivially are (`_J is None` returns immediately). It cannot prove a site
was *found*.

So `GreedyBot._pick_journalled` landed first, behind `TTA_JOURNAL=1`
(off by default — `experiments/`, `analysis/`, `WeightedBot` and
`QuiescentBot` are untouched). With it, `JOURNAL_PARANOID=1` copies the state,
applies the candidate by undo, rolls back, and structurally diffs on **every
candidate move of every game**. A missed site then announces itself by path:

```
AssertionError: journal rollback did not restore the state:
  state.card_row[0]: type str != NoneType
  state.card_row[1]: type str != NoneType
  state.card_row[2]: type str != NoneType
```

That is `game.py:117` — `_replenish` does `row[i] = None` on the *current*
list before rebinding `state.card_row` to a new one, so the in-place clear is
invisible to the (journalled) attribute write that follows. It was found on
the first probe game, in seconds, by path. Hand-auditing 470 sites would not
reliably have found it, because it is the kind of site that *looks* covered:
there is a journalled attribute write two lines below it.

Conversion order is therefore: convert a module → `bash tools/gate.sh` (proves
inertness, journal OFF) → probe with `TTA_JOURNAL=1 JOURNAL_PARANOID=1`
(proves discovery, journal ON) → next module. `tools/gate.sh --journal` runs
the four journal arms once every module is in.

`journal.begin` also drops `_stats_cache` on **entry**, not just on exit.
The cache is content-keyed (`stats_key`) so a warm cache is usually safe — but
only usually: a mutation site that forgets `effects.invalidate` is invisible on
the copy path, because a copy never carries a cache and always recomputes.
Starting cold makes the journal path faithful to the copy path by
construction, and costs nothing the copy path was not already paying.

### 9.8 The one hole `grep` could not close: is `JOURNALLED_CLASSES` complete?

`tools/find_mutations.py` finds container sites because subscripts and
`append`/`pop`/… are syntax. The 300 attribute writes are covered by the
`__setattr__` hook — **but only for the four classes in
`journal.JOURNALLED_CLASSES`.** An attribute write to a state-reachable object
of any *fifth* class would be silently unjournalled and would not appear in any
grep, because `x.y = z` looks identical whatever `x` is. That is the one
failure mode neither 6.5's list nor the site census names.

Closed mechanically instead of by argument (`tests/test_journal.py`,
`SetattrCoverageIsComplete`): walk a real 120-move 4p mid-game state and assert
the set of reachable types is exactly what the journal assumes — the four
dataclasses, and containers only of type `list`/`dict`/`set`/`tuple`/
`frozenset` (the three mutable ones being exactly what `touch()` accepts;
`touch` raises `JournalError` on anything else). Both pass. If anyone later
adds a fifth state dataclass, the test fails and names it.

### 9.9 Step 5 — all six modules converted (commits 5a–5f)

One commit per module per 6.6 condition 3, `bash tools/gate.sh` after each.
Final census from `tools/find_mutations.py`: **166 container sites, 100
converted, 66 locals with a written argument.**

| module | sites | converted | locals | note |
|---|---|---|---|---|
| actions.py | 56 | 19 | 37 | locals are all `moves`/`out`/`by_type`/`costs`/… built inside move generation |
| effects.py | 31 | 0 | 31 | 19 locals, 5 module memo dicts, 4 `_stats_cache` entries, 0 pact writes |
| interact.py | 43 | 29 | 14 | the nested case: dicts *inside* `state.pending` |
| game.py | 7 | 5 | 2 | includes the `_replenish` site paranoid mode caught |
| events.py | 5 | 4 | 1 | |
| economy.py | 6 | 6 | 0 | the only doubly-nested site |

Three shapes were new relative to 6.2's model and are the ones to remember:

1. **In-place mutation immediately before a journalled rebind** (`game.py`
   `_replenish`). The attribute write two lines down makes the site *look*
   covered. Only paranoid mode found it.
2. **A dict inside a state container** (`interact.py` auctions/defense).
   `touch(state.pending)` restores *which* dicts are in the list, not their
   contents; `pend` needs its own record and `pend["active"]` a third.
3. **Two containers reached in one expression** (`economy.py`
   `discarded_military.setdefault(age, []).append(name)`). Both records are
   required and neither implies the other; snapshotting a freshly-created `[]`
   is harmless, so the same code is correct whichever branch `setdefault`
   takes.

`touch` is written out at every site rather than hoisted out of loops, even
where a hoist would be equivalent, because `find_mutations.py` checks the
mutated expression *textually*. A hoisted touch leaves the site reading as
unconverted, and a checklist that lies is worse than no checklist. The cost of
a redundant touch is one `id()` set probe.

### 9.10 Step 6 — coverage, and the seven sites self-play never reaches

`tools/mutation_coverage.py` (new): the site census × `sys.monitoring` LINE
events, restricted to those exact lines, over real journalled games. Returning
`DISABLE` from the callback retires each location on first hit, so tracing is
nearly free. `coverage.py` is not installable on this box (PEP 668) and is the
wrong shape regardless — the question is a verdict on 166 specific lines, not
a percentage.

**First pass, 60 games (20 seeds × 2p/3p/4p): 7 converted sites had never
executed.** The aggression-defense exchange (4), Annex, a refused pact offer,
and the multi-step `lose_pop` re-queue. The 135-game paranoid suite had
therefore proven *nothing whatever* about them — which is exactly the failure
mode 9.5 predicted, and it would have shipped looking green.

They are now driven directly by `RareSitesRollBackExactly`
(`tests/test_journal.py`): construct the state, run the path inside a journal,
diff the rollback against a `copy_state` oracle. Same standard as paranoid
mode, with the state constructed instead of stumbled into. The tool traces the
test suite as well as the games, because a site verified by a targeted test is
*better* verified than one that happened to come up in self-play.

**Result: 0 of 61 converted sites unexecuted.** Non-zero is exit status 1.

12 unconverted sites remain unexecuted and all 12 are accounted for: 9 are
`cards.py` DB construction (import time, static card data, unreachable from a
trial `apply`), 1 is `actions.py:367`'s `moves.append(("pop_free",))` (the
same local as the other 36), and 1 is `state.py:202`'s `del self.log[:100]` —
hazard 2 of 6.5 by name, which cannot run inside a trial because `emit`
returns early while `SUPPRESS_LOG` is set.

### 9.11 The gate, all TEN arms

`bash tools/gate.sh --journal`, on the final tree:

```
unittest                         OK   Ran 128 tests
unittest JOURNAL_PARANOID        OK   Ran 128 tests
narrow fingerprint               OK   6f5c72ef7c011cf7
narrow FASTCOPY_PARANOID         OK   6f5c72ef7c011cf7
wide fingerprint                 OK   7814c5c9c276b0a2
wide FASTCOPY_PARANOID           OK   7814c5c9c276b0a2
narrow JOURNAL                   OK   6f5c72ef7c011cf7
narrow JOURNAL+PARANOID          OK   6f5c72ef7c011cf7
wide JOURNAL                     OK   7814c5c9c276b0a2
wide JOURNAL+PARANOID            OK   7814c5c9c276b0a2
GATE PASS
```

(The last arm is ~15 minutes on this box under the hill climbs' load -- 102
games, each candidate move copied, applied, rolled back and diffed. It was
run separately and returned the full
`7814c5c9c276b0a2229b6b58143351c2ad1a1058f283db70d1d9a50d5448e8ce`.)

Two gate bugs surfaced while assembling this table, and both are worth
recording because both would have produced a *misleading reading* rather than
an honest failure — the exact category of problem 9.0 exists to warn about.

**A test that asserted nothing.** The `unittest JOURNAL_PARANOID` arm was
added late and immediately earned itself: `RareSitesRollBackExactly`'s negative control had been written to rely
on `journal.begin`'s built-in oracle, which only exists when
`JOURNAL_PARANOID=1` is set. It passed when run by hand (I had the variable
set) and asserted nothing at all under `tools/gate.sh`, which runs unittest
with a clean environment. The gate caught it. It also surfaced a pre-existing
test that made a deliberately unjournalled container write; that is now
journalled, so the suite is paranoid-clean and can be *used* as a check rather
than merely run as a test.

**A gate that cried wolf.** `tools/gate.sh` reported `GATE FAIL` with three
arms missing from its output and a long run of whitespace where they should
have been — on a tree whose digests were provably correct when the same
commands were run by hand. Cause: `/bin/bash` on macOS is **3.2**, and the
helper I had refactored collected environment assignments into an array
(`envs+=(...)`, `"${envs[@]}"`). Under 3.2 that garbles rather than errors.
Both helpers now take the environment as one plain string and the script
contains no arrays. This is the failure mode that gets a good change reverted
and a real regression blessed, so: **when this gate fails, reproduce the
failing arm by hand before believing it** — in both directions.

The last four arms are the claim. `JOURNAL` says 135 games played by undo
instead of by copy produce byte-identical logs and scores.
`JOURNAL+PARANOID` says that on **every candidate move of those 135 games**
the state was additionally copied, the candidate applied by undo, rolled back,
and the two structurally diffed including dict key order — with no difference
found anywhere.

### 9.12 Step 7 — MEASURED end-to-end throughput: 1.6x at 3p, 1.75x at 4p

`engine.perf_check bench`, `time.process_time` (this process's own CPU, the
only stable measure while three hill climbs keep the box busy — see the
docstring). Three independent measurement pairs, `nice -n 15`, one worker at a
time, baseline and journal alternating so any drift hits both:

| | 3p baseline | 3p journal | 4p baseline | 4p journal |
|---|---|---|---|---|
| run 1 (8 games)  | 2.74 | 4.33 | 1.43 | 2.38 |
| run 2 (10 games) | 2.64 | 4.75 | 1.32 | 2.41 |
| run 3 (10 games) | 2.60 | 4.10 | 1.34 | 2.35 |
| **mean games/cpu-s** | **2.66** | **4.39** | **1.36** | **2.38** |
| **speed-up** | | **1.65x** | | **1.75x** |

In moves/cpu-s, which is insensitive to game-length variation between seeds:
3p 655 → 1038 (**1.58x**), 4p 528 → 920 (**1.74x**).

So: **1.75x on 4p greedy, the cell the hill climbs actually run**, against
6.4's projection of ~1.8x. The projection was close and slightly optimistic,
which is the expected direction — it modelled the copy going to zero, and the
journal is not free: the `__setattr__` hook taxes every attribute write in the
process (not just trial ones) at 6.4x, `touch` costs an id-set probe per
container per candidate, and `begin` drops the stats cache.

Worth noting what the variance says. The three *baselines* are tight
(4p: 1.43 / 1.32 / 1.34) and so are the three *journal* numbers
(2.38 / 2.41 / 2.35); the per-run ratios (1.66 / 1.83 / 1.75) move more than
either column does, which is contention noise on a loaded box rather than
anything about the change. Anyone re-measuring should take the mean of several
alternating pairs, not a single ratio — a single pair here would have
supported any claim between 1.66x and 1.83x.

This is on top of the 1.55x fastcopy win (4a) and the 1.23-1.33x it delivered
end-to-end (4c); those are not additive, because the journal removes the very
copy that fastcopy made cheap. 1.75x is measured against current master, which
already has all of that.

### 9.13 Status — steps 1-7 DONE, branch is green and NOT merged

Every 6.6 condition is met:

1. Design A (undo stack), not B. ✓
2. `engine/journal.py` + the paranoid differ landed and passed before any call
   site was converted. ✓
3. Six modules, one commit each, gate after each. ✓
4. Hard gate at every step, digests unchanged. ✓ (10/10 arms, 9.11)
5. Done on a worktree the hill climbs never read. ✓

Plus the two things 6.6 did not ask for and should have: the coverage audit
(9.10) and the proof that the class list is complete (9.8).

**Still not merged, deliberately.** `TTA_JOURNAL` is off by default, so
merging this branch changes nothing until someone sets it — but the trainer
supervisor relaunches `experiments.hillclimb_league` from the master checkout
every hour, so the merge and the trainer restart should be one deliberate act
by a human, not a side effect. Remaining work for whoever picks this up:

- [ ] Decide whether `USE_JOURNAL` should default ON, or whether the hill
      climbs should pass it explicitly. Defaulting on makes `WeightedBot` and
      `QuiescentBot` pay the `__setattr__` hook for nothing (9.8's test would
      still pass, but the cost stops being zero), so "explicit in
      `run_league.sh`" is probably right.
- [ ] **Do not assume this change extends to `QuiescentBot`.**
      `bots/quiescent.py` has three more `copy_state` calls and searches
      multiple ply: `_best_move` recurses, and `_war_value` copies a state
      that is *itself* already a trial (`scratch = copy_state(state)`, line
      209). So it holds several live trial states at once, which is precisely
      the capability 6.6 said design A does not have and design B exists to
      provide — `journal.begin` raises `JournalError` on nesting by design.
      Converting it needs either a nested/stacked journal or leaving it on the
      copy path. This is the trigger 6.6 named for revisiting design B.
- [ ] Re-run `tools/mutation_coverage.py` after any engine change that adds a
      container mutation. It is cheap and it is the only thing standing
      between a new site and a silently corrupted training run.

### 9.14 The gap 9.13 left: the league does not run GreedyBot

9.12 measured 1.65x/1.75x **on GreedyBot**, and 9.13's remaining-work list
treated `WeightedBot` as a bot that would *pay* for the journal rather than one
that would be *paid by* it ("defaulting on makes `WeightedBot` and
`QuiescentBot` pay the `__setattr__` hook for nothing"). That reads the
situation backwards. `WeightedBot.pick` had exactly the same
copy-per-candidate-move shape as `GreedyBot.pick` — `trial = copy_state(state)`
inside the candidate loop, weighted.py:669 — and the branch had not touched
`engine/bots/weighted.py` at all. Merged as it stood, this branch would have
delivered close to nothing to the actual training workload.

#### Which bot the league actually instantiates — measured, not assumed

Seats are built in one place, `experiments/arena.py:111`, via a `make_bot` that
`experiments/hillclimb_pool.py:128` monkey-patches over `arena.make_bot`.
Replaying `Pool.acceptance_subset(gen, 4)` for 200 generations against the real
pool gives the opponent-seat census; adding the candidate seat (every 2p game is
one candidate + one opponent) gives:

| seat class | share of all seats | searches by copying? |
|---|---|---|
| `WeightedBot` (candidate, mirror/champion, past:*, floor default) | **~69%** | **yes, 1-ply, one copy per candidate** |
| `BookBot` / `VariantBot` (book, book2, var:*6) | ~27% | **no** — rule-based, no `copy_state` anywhere |
| `GreedyBot` | ~2% | yes |
| `RandomBot` | ~2% | no |
| `QuiescentBot` | **0%** | (yes, but not present) |

So WeightedBot is ~69% of seats and a considerably larger share of league *CPU*
than that, because the 27% BookBot-family seats do no search at all. GreedyBot,
the bot 9.12 measured, is a ~2% floor opponent.

**`QuiescentBot` is 0% of league seats.** `quiesce:` specs are parsed only in
`arena.py:44-53` (`load_spec`), and neither `hillclimb_league.py` nor
`hillclimb_pool.py` ever calls `load_spec`. The only way into the pool is
`hillclimb_pool.py:483-485`, guarded by `with_quiescent`, which defaults `False`
at `hillclimb_league.py:563` and is a `store_true` flag at `:895`;
`experiments/run_league.sh` does not pass it. Every `quiesce:` string in the
repo comes from a human CLI (`tools/quiesce_bench.py`, `experiments/evaluate.py`,
`analysis/*`). This matters for the payoff both ways: converting WeightedBot
targets ~69% of seats, and leaving QuiescentBot on the copy path costs the
league exactly nothing today.

#### The digests could not have caught any of this

9.0/9.6 lean on the fingerprint playing **GreedyBot only** — that blindness is
the whole reason four master rebases left `6f5c72ef…`/`7814c5c9…` untouched.
The corollary nobody wrote down: **no digest in this project can catch a change
to `WeightedBot`.** Gating a WeightedBot change on the greedy fingerprint is the
9.0 trap wearing a different hat — a green gate that proves nothing about the
line that changed.

So `engine/perf_check.py` grows a `weighted` bot kind and `weighted_cases()`,
sized 11 seeds x 3 player counts = 33 games narrow and 34 x 3 = 102 wide, the
same 33/102 split as the greedy sets so "the 135-game suite" means the same
amount of play whichever bot searches. Baselines were derived per 9.0's rule —
from scratch on a clean detached worktree of master `6d0247c` **and** on this
branch, requiring agreement:

```
weighted narrow (33 games)  dff85378482c9fbd8f04319dbe1fdd2975cb49870e71efd3f29bd77af536fb91
```

That agreement is also the first evidence that step 5's 100 converted container
sites are inert under a *different move distribution* — everything before this
point was checked with GreedyBot's moves only.

#### The conversion (commit 47c0e5b)

`WeightedBot._pick_journalled`, behind the same `USE_JOURNAL` flag, following
`GreedyBot._pick_journalled` line for line — the copy path is left exactly as it
was so it stays available as the paranoid oracle. Three of WeightedBot's own
semantics are **not** GreedyBot's and are preserved deliberately:

* `ctx` is computed once at the root on the unmutated state and passed to every
  candidate. Under the journal the root state *is* the object the candidates
  mutate, so capturing before the loop is load-bearing rather than incidental.
  `rival_context` returns a plain dict of numbers, so it is not aliased to
  anything a rollback restores.
* `end_bias` is **added**; GreedyBot subtracts a fixed 0.01.
* `evaluate` sits **inside** the `except Exception: continue`. GreedyBot's
  journalled loop evaluates outside its `try`; copying that shape here would
  convert WeightedBot's "an unscorable candidate is skipped, never fatal" into a
  mid-game crash. This is the one place where following the existing pattern
  exactly would have been the bug.

`USE_JOURNAL` and the trial rng moved to a new leaf module `engine/bots/trial.py`,
because `bots/__init__.py` imports `bots.weighted` at module scope and a
package-level import from `weighted` would depend on the order of lines in
`__init__.py`. Both names are re-exported, so `bots.USE_JOURNAL` still resolves
for `tools/mutation_coverage.py` and `tests/test_journal.py`.

#### Coverage — WeightedBot reaches a site GreedyBot's pass never did

This is the whole risk argument, and it pays out. `tools/mutation_coverage.py`
grows `--bot`; 60 games (20 seeds x 2p/3p/4p) per bot, **games only, no tests**,
so the two move distributions can be compared honestly:

| | converted sites unreached | unconverted (local) unreached |
|---|---|---|
| GreedyBot games only | 7 / 61 | 27 / 101 |
| WeightedBot games only | 7 / 61 | 21 / 101 |
| **union of both** | **6 / 61** | 19 / 101 |
| either bot + the test suite | **0 / 61** | 12 / 101 |

The sets are not nested:

* WeightedBot reaches **`engine/interact.py:228`** — `journal.touch(owner.hand_military).append(name)`, the **refused pact offer**, which 9.10 listed by name as one of the seven converted sites 60 games of GreedyBot self-play never executed. WeightedBot plays pacts; GreedyBot does not. Under the journal that site is now proven in real play, not only by its targeted test.
* GreedyBot reaches **`engine/actions.py:799`** — `del journal.touch(p.techs)[old]`, the tech-replacement `del`, which WeightedBot's 60 games never hit.
* WeightedBot additionally executes 8 unconverted sites GreedyBot never does (6 in `effects.py`), i.e. more of the evaluation surface.

So the previous pass's coverage claim was *bot-specific*, exactly as suspected,
and re-running it under WeightedBot was not a formality. With the test suite
traced as well (9.10's standard: a site driven by a targeted rollback test is
better verified than one stumbled into), **both bots report 0 of 61 converted
sites unexecuted**, exit status 0. The 12 unconverted misses are the same 12
9.10 accounted for — 9 `cards.py` import-time DB construction, `actions.py:367`,
and `state.py:202`'s `del self.log[:100]`, which cannot run inside a trial
because `emit` returns early while `SUPPRESS_LOG` is set.

#### The gate — 16 arms, and the 135-game paranoid suite with WeightedBot searching

The four weighted arms are the coverage argument cashed in. `TTA_JOURNAL=1
JOURNAL_PARANOID=1` with WeightedBot searching means: on **every candidate move
of 135 WeightedBot games**, the state is copied, the candidate applied by undo,
rolled back, and the two structurally diffed including dict key order.

```
weighted narrow(33)   master 6d0247c   dff85378482c9fbd   (25s)
weighted narrow(33)   branch           dff85378482c9fbd   (22s)
weighted narrow(33)   branch JOURNAL   dff85378482c9fbd   (19s)
weighted narrow(33)   branch J+PARA    dff85378482c9fbd   (137s)
weighted wide(102)    master 6d0247c   477d1c1fe6d2e770   (92s)
weighted wide(102)    branch           477d1c1fe6d2e770   (69s)
weighted wide(102)    branch JOURNAL   477d1c1fe6d2e770   (50s)
weighted wide(102)    branch J+PARA    477d1c1fe6d2e770   (341s)
```

```
weighted narrow dff85378482c9fbd8f04319dbe1fdd2975cb49870e71efd3f29bd77af536fb91
weighted wide   477d1c1fe6d2e770e34f338d9425da3d8d7e7235a8b904a2c179fdbb27debcdc
```

No missed site, first try. 9.11's lesson says a first-try pass is exactly when
to distrust the instrument, so the check was proved live in both directions
before being believed:

* **positive**: `journal.begin` counted at **4169 calls** in one 4p WeightedBot
  game, with `weighted.USE_JOURNAL`, `bots.USE_JOURNAL` and `journal.PARANOID`
  all confirmed `True` in the same process;
* **negative**: `journal.touch` replaced by `lambda o: o` — record nothing,
  return the object — in all five converted modules. The weighted paranoid run
  then failed on the first game, by path:
  `journal rollback did not restore the state: state.players[0].hand_civil: keys gained: ['0']`.

So the arm can fail, and does, and the pass means something.

`tools/gate.sh` grows the four weighted arms (`WNARROW=dff85378`,
`WWIDE=477d1c1f`) alongside the six it had; the greedy digests `6f5c72ef` /
`7814c5c9` are unchanged throughout.

`tests/test_journal_weighted.py`: 7 tests, 135 total.  `bash tools/gate.sh
--journal` on the final tree:

```
unittest                         OK   Ran 135 tests
unittest JOURNAL_PARANOID        OK   Ran 135 tests
narrow fingerprint               OK   6f5c72ef7c011cf7
narrow FASTCOPY_PARANOID         OK   6f5c72ef7c011cf7
weighted narrow                  OK   dff85378482c9fbd
wide fingerprint                 OK   7814c5c9c276b0a2
wide FASTCOPY_PARANOID           OK   7814c5c9c276b0a2
weighted wide                    OK   477d1c1fe6d2e770
narrow JOURNAL                   OK   6f5c72ef7c011cf7
narrow JOURNAL+PARANOID          OK   6f5c72ef7c011cf7
wide JOURNAL                     OK   7814c5c9c276b0a2
wide JOURNAL+PARANOID            OK   7814c5c9c276b0a2
weighted narrow JOURNAL          OK   dff85378482c9fbd
weighted narrow JOURNAL+PARANOID OK   dff85378482c9fbd
weighted wide JOURNAL            OK   477d1c1fe6d2e770
weighted wide JOURNAL+PARANOID   OK   477d1c1fe6d2e770
GATE PASS
```

Both greedy digests unchanged from 9.6, both weighted digests equal to master's.
270 games played by undo instead of by copy, 270 of them additionally
copy-diffed on every candidate move.

### 9.15 MEASURED end-to-end throughput on WeightedBot: 1.40x at 3p, 1.44x at 4p

9.12's protocol exactly: `engine.perf_check bench`, `time.process_time`,
`nice -n 15`, ONE worker, and the arms interleaved **within** each round so any
drift in machine load hits all three. Three arms, on three separate worktrees,
so the journal win and the rng win are attributed independently:

* `BASE` — master `6d0247c`: copy path, `random.Random(0)` per candidate
* `RNG` — `3bcae9c`: copy path, shared trial rng
* `JRNL` — branch with `TTA_JOURNAL=1`: undo stack + shared trial rng

| games/cpu-s | 3p BASE | 3p RNG | 3p JRNL | 4p BASE | 4p RNG | 4p JRNL |
|---|---|---|---|---|---|---|
| round 1 | 1.86 | 1.97 | 2.61 | 0.92 | 0.97 | 1.41 |
| round 2 | 1.86 | 1.96 | 2.63 | 1.06 | 1.00 | 1.47 |
| round 3 | 1.88 | 2.00 | 2.62 | 1.00 | 0.97 | 1.42 |
| **mean** | **1.87** | **1.98** | **2.62** | **0.99** | **0.98** | **1.43** |

**Speed-up over master: 1.40x at 3p, 1.44x at 4p.** In moves/cpu-s, which is
insensitive to game-length variation between seeds: 3p 406 → 569 (**1.40x**),
4p 360 → 519 (**1.44x**) — the two metrics agreeing to three digits is the
sanity check that the game-length noise 9.12 warned about is not driving this.

Attributed separately, `JRNL / RNG` is the journal alone: **1.33x at 3p, 1.46x
at 4p**.

**Why this is less than GreedyBot's 1.75x, and why that is the expected
direction.** The journal removes the copy; it does not touch evaluation.
GreedyBot's evaluation is 19 features and one `fsum`. WeightedBot's is ~78
weights over ~57 features plus `hand_potential`, which re-prices every card in
hand. So the copy is a *smaller fraction* of a WeightedBot candidate than of a
GreedyBot candidate, and Amdahl takes the rest. 1.44x on the bot that fills
~69% of league seats is worth considerably more wall-clock than 1.75x on the
one that fills ~2%.

#### The rng fix: real, small, and NOT resolvable end-to-end — 8.1 for the third time

5a profiled `random.Random(0)` per candidate at 10.8%. 8.1 A/B'd it on
GreedyBot and found the profiler had overstated it. On WeightedBot the same
thing happens again, and this time the honest answer is that the end-to-end
measurement **cannot see it at all**:

| | 3p | 4p |
|---|---|---|
| direct accounting: `Random(0)` cost x apply count / game cpu | **6.2%** | **4.4%** |
| A/B, 3 rounds x 10 games (games/cpu-s) | +5.9% | −1.3% |
| A/B, 5 rounds x 16 games (moves/cpu-s) | +0.9% | +2.6% |
| paired, same-process, 8 reps (moves/cpu-s) | +6.7% (sd 0.17) | +0.9% (sd 0.07) |

The direct accounting is solid — 9645 ns per `random.Random(0)` measured by
`timeit`, times 1979 (3p) / 4169 (4p) `apply` calls per game, over 0.31 / 0.91
cpu-s per game. The end-to-end arms are not: the round-to-round spread of the
*baseline alone* is 380–471 moves/cpu-s at 3p, a ~20% noise floor with three
hill climbs on the box, and a 5% effect does not survive that. Even the paired
same-process design, which removes cross-process drift, has sd 0.17 at 3p.

So: **keep the fix, claim ~4-6% from the accounting, and do not claim the A/B
supports a number** — it is consistent with anything from 0% to 7%. The fix is
worth keeping regardless because it is free: no behaviour change (all four
weighted digests identical), no new failure mode, and it removes a real 9.6 us
of work from every one of ~4000 candidates a game.

The standing lesson of 8.3 now has three data points and should be read as a
rule: **on this box, a profiler line under ~10% cannot be confirmed by an
end-to-end A/B.** Either measure it directly (cost x count) or accept it
unmeasured; do not run more rounds hoping the noise averages out, because the
noise is contention, not sampling.

### 9.16 QuiescentBot: the nesting verdict, and where USE_JOURNAL should live

#### QuiescentBot cannot reach the journal — verified structurally and by execution

9.13 was right that `QuiescentBot` must stay on the copy path: `_war_value`
does `scratch = copy_state(state)` on a state that is *itself* already a trial
(`quiescent.py:210`), `_pick` copies inside `_resolve` which is itself called
from inside a candidate (`:176`), and `journal.begin` raises on nesting by
design (`journal.py:166-169`). `copy_state` inside an open journal raises too
(`journal.py:80-85`), so there is no quiet fallback to hide behind — which is
the right shape: the failure is loud or it does not exist.

The question 9.13 did not answer is whether journalling `WeightedBot` could
reach a nested `begin` *through* QuiescentBot. It cannot:

* `QuiescentBot` is a bare `class QuiescentBot:` (`quiescent.py:219`), not a
  subclass of anything. Nothing in the tree subclasses `WeightedBot`.
* It re-implements the 1-ply pick itself (`_pick`, `:164-196`) instead of
  delegating. Its only imports from `weighted` are `DEFAULT_WEIGHTS`,
  `evaluate` and `rival_context` (`:90`) — and an AST scan of the whole of
  `weighted.py` finds **no writes to state at all**: every assignment and
  container mutation in the file targets a local (`gains`, `out`, `best`), a
  `self` field, or a module-level weight dict at import time. So the shared
  surface is read-only and journal-free.
* `engine/bots/book.py:905` is the one other caller of `WeightedBot.choose`
  (`BookImprovedBot`). It calls it once, at its own root, on the real state —
  no nesting — and it is not in the league pool regardless.

`tests/test_journal_weighted.py` pins all of it, each assertion with a positive
control because 9.11 caught a test on this branch that asserted nothing:

* QuiescentBot opens **0** journals over a 60-move 3p game with
  `USE_JOURNAL` forced on — while WeightedBot under the *same* counter opens
  many, which is what makes the zero meaningful;
* QuiescentBot enters `WeightedBot.pick` **0** times — again against a live
  spy proven to fire for WeightedBot;
* a nested `_pick_journalled` inside an open journal raises `JournalError`,
  so if anyone ever does wire it up, they get a stack trace and not a
  corrupted training run;
* QuiescentBot's copy-inside-a-trial search still works with the journalling
  `__setattr__` installed process-wide, which it will be as soon as any
  WeightedBot search has run in that process.

#### Standing constraint: the journal is process-global, so the harness must stay process-parallel

`_J` is a module global. `experiments/arena.py:192` uses
`multiprocessing.Pool`, and there is no `threading` anywhere in
`hillclimb_league.py` / `hillclimb_pool.py` / `arena.py` / `harness.py`, so
every worker has its own journal and the design is sound. **A thread-parallel
harness would corrupt states across threads silently.** Anyone who moves the
league to threads must put `_J` in a `threading.local` first.

#### Recommendation: set it explicitly in `run_league.sh`; do NOT default it on

9.13 reached "explicit in `run_league.sh`" for a reason that turns out to be
wrong ("defaulting on makes `WeightedBot` and `QuiescentBot` pay the hook for
nothing"). Both halves are wrong: `WeightedBot` is the *beneficiary*, at 1.44x
and ~69% of seats; and `QuiescentBot` pays **zero**, because `journal.install()`
is lazy — the hook goes on the dataclasses at the first `begin()`, and a process
running only QuiescentBot never calls one.

The conclusion survives its reasoning being replaced, for a better reason:

**Keep the default OFF so the copy path stays the oracle.** The journal's whole
safety story is that two independent implementations agree. If `USE_JOURNAL`
defaults on, every caller in `analysis/`, `tools/` and `experiments/` switches
to the undo path at once and there is nothing left to disagree with. The next
engine change that adds a container mutation — and there will be one; that is
what `tools/mutation_coverage.py` exists for — would then corrupt everything
uniformly instead of showing up as a divergence between two paths.

So:

1. `export TTA_JOURNAL=1` in `experiments/run_league.sh`, so the trainer takes
   the 1.44x and nothing else changes behaviour.
2. Leave `USE_JOURNAL` defaulting off everywhere else.
3. `bash tools/gate.sh --journal` (16 arms then; **28 as of section 10**)
   before any merge that touches
   `engine/`, not the 6-arm default.
4. Re-run `tools/mutation_coverage.py --bot weighted` **and** `--bot greedy`
   after any engine change that adds a container mutation. 9.14 showed the two
   bots do not cover the same sites, so one bot is not an audit.

**`experiments/run_league.sh` is deliberately NOT edited on this branch.** With
the flag off, merging `journal-undo` still changes nothing until a human sets
it — which is the property 9.13 wanted and the reason the supervisor's hourly
relaunch is safe. The whole change is one line, to be added at the same moment
the merge is made, not before:

```diff
 set -u
 cd "$(dirname "$0")/.."
+export TTA_JOURNAL=1        # docs/PYPY.md 9.14-9.16: 1.44x on WeightedBot
 K=${1:-2}; H=${2:-8}; W=${3:-6}; L=${4:-2}; B=${5:-12}; S=${6:-4}; Z=${7:-1.2816}
```

The three running climbs pick it up on their next hourly restart with no other
action, and reverting is the same one line.

### 9.17 Status — the branch now covers the bot the league actually runs

| | GreedyBot (9.12) | WeightedBot (9.15) |
|---|---|---|
| share of league seats | ~2% | **~69%** |
| speed-up at 3p | 1.65x | **1.40x** |
| speed-up at 4p | 1.75x | **1.44x** |
| converted sites reached, games only | 54/61 | 54/61 (union 55, +tests 61) |
| 135-game paranoid suite | `6f5c72ef` / `7814c5c9` | `dff85378` / `477d1c1f` |

(Stale as of 9.18 — all four digests in this row moved, twice, after two
legitimate rules fixes landed on master. See 9.18 for the current values.)

Remaining work is unchanged from 9.13 except that the WeightedBot item is done:

- [x] Convert `WeightedBot` (9.14) and measure it (9.15).
- [x] Decide the `USE_JOURNAL` default (9.16: explicit in `run_league.sh`).
- [x] Verify `QuiescentBot` cannot reach a nested `begin` (9.16).
- [ ] `QuiescentBot` itself still needs either a stacked journal or design B
      before it can leave the copy path. It is 0% of league seats today, so
      this is not urgent — but `--with-quiescent` exists, and the day someone
      passes it, the pool gains a bot that must not see `TTA_JOURNAL=1`. The
      guard for that day is the test above, not a comment.
- [ ] Re-run `tools/mutation_coverage.py` with **both** bots after any engine
      change that adds a container mutation.

### 9.18 Two more rebases while parked — combat audit (`4886b65`) then coverage
### audit (`3439b0e`) — all four digests moved, and this time NARROW did too

Master moved twice more since 9.17 while this work sat merged-but-undocumented.
Both hops are real rules fixes, not gate noise, and per 9.0's rule neither was
trusted without deriving it twice, independently, and requiring agreement.

**Hop 1 — combat audit, master `4886b65` (done by a different session; recorded
here for the first time because it never made it into this file, only into
`tools/gate.sh`'s own comments).** `33bd156` ("Fix three combat rules bugs
found by the audit") changed `engine/actions.py` and `engine/effects.py` —
real war/pact/aggression fixes that GreedyBot's evaluation goes through — and
`e990920` (well before `33bd156`, but not yet reflected in the WNARROW/WWIDE
pair at the time) replaced WeightedBot's default `lateness()` schedule, which
every WeightedBot feature reads. WIDE moved because the wider 10-seed greedy
set happened to contain a game touching one of the three combat bugs; NARROW's
3 greedy seeds did not. Both weighted digests moved because *every*
WeightedBot feature prices against `lateness()`.

| | 9.6/9.17 (`6d0247c`) | hop 1 (`4886b65`) |
|---|---|---|
| narrow (33 games) | `6f5c72ef7c011cf7` | `6f5c72ef7c011cf7` (unchanged) |
| wide (102 games) | `7814c5c9c276b0a2` | `a966d158f0486366` |
| weighted narrow (33 games) | `dff85378482c9fbd` | `b943e1a6…` |
| weighted wide (102 games) | `477d1c1fe6d2e770` | `540c3f97…` |

**Hop 2 — coverage audit, master `3439b0e` (this session).** `git diff
4886b65..3439b0e --stat` touches exactly one file inside `engine/`:
`engine/actions.py`. Everything else in range —
`experiments/arena.py`'s degenerate-champion guard, `experiments/summarize.py`'s
feature-grouping fix, and the new standalone `tools/coverage_census.py` /
`tools/feature_variance.py` — is additive or reporting-only and not on the
`perf_check` hash path (confirmed by reading each diff, not by assumption).
`engine/actions.py`'s two changes (docs/COVERAGE_AUDIT.md Secs 2.1-2.2), both
of which **either bot's evaluation goes through** since they are in shared
action-generation/resolution code, not a bot file:

* `_h_revolution` no longer discards the actions the new government grants —
  only the pool that *paid* for the revolution is emptied; the other pool is
  capped at the new government's total instead of zeroed, so a revolt from
  Despotism to Monarchy now correctly yields 3 military actions, not 2.
  Revolution has a 30-65% take-rate across the fingerprint's games, so this
  moves both GreedyBot's and WeightedBot's play — which is exactly why NARROW
  moved this time, unlike hop 1: this is not a combat-specific interaction confined
  to the wider seed set, it is a core turn-structure change common enough that
  even 3 greedy seeds hit it.
* the one-per-name rule (`_can_take_gated`) is no longer applied to yellow
  ACTION cards, which exist in 2-3 copies per deck and are not technologies —
  holding one copy no longer blocks taking another.

Derived per 9.0's rule: computed from scratch on a fresh detached checkout of
master `3439b0e` at `/tmp/tta-gate-verify-A`, and independently in a second
worktree (`gate-rebaseline`, branched off the same `3439b0e`), each run
`nice -n 15`, at most 2 concurrent `perf_check` processes. Not just the 8-char
summary compared — the full per-case digest list in each side's `perf_check
save` output was diffed key-by-key (33/102/33/102 cases respectively) and
found identical in every case, both sides, all four arms:

| | side A (fresh checkout) | side B (worktree) |
|---|---|---|
| narrow (33 games) | `2fd656b38729de71` | `2fd656b38729de71` |
| wide (102 games) | `1169007df1517e33` | `1169007df1517e33` |
| weighted narrow (33 games) | `a7691eaac8b59fac` | `a7691eaac8b59fac` |
| weighted wide (102 games) | `c7045ab13862e4fb` | `c7045ab13862e4fb` |

Full digests:

```
narrow          2fd656b38729de718361749330edf220d8a908c07000829b86708b456faf8f44
wide            1169007df1517e33681f9c567839a1ae3dc9e7c88fac6288f5549bce3328d9ba
weighted narrow a7691eaac8b59fac996786f2d90db852b122bf2230386244b2ab9c0c208dec69
weighted wide   c7045ab13862e4fb8542b09aa51392fbfa84da1b3b7137be433e362424baa510
```

**The two sides agreeing, per-case, is the entire proof — not either number
alone**, same as 9.6 and every prior re-derivation. `tools/gate.sh` now gates
on these four; `tools/fingerprint.json` / `tools/fingerprint_wide.json`
re-saved via `perf_check save` to match (narrow/wide only — there is no
committed weighted fingerprint file, same as before). Negative control run
alongside this (nudge a default weight on a scratch copy, confirm the gate
FAILS, then discard the scratch copy): the gate can fail, so the pass above
means something.

```
unittest                         OK   Ran 248 tests
unittest JOURNAL_PARANOID        OK   Ran 248 tests
narrow fingerprint                OK   2fd656b38729de71
narrow FASTCOPY_PARANOID          OK   2fd656b38729de71
weighted narrow                   OK   a7691eaac8b59fac
wide fingerprint                  OK   1169007df1517e33
wide FASTCOPY_PARANOID            OK   1169007df1517e33
weighted wide                     OK   c7045ab13862e4fb
GATE PASS
```

Updated table for 9.17's row, current as of `3439b0e`:

| | GreedyBot | WeightedBot |
|---|---|---|
| 135-game paranoid suite | `2fd656b3` / `1169007d` | `a7691eaa` / `c7045ab1` |

Lesson worth keeping: hop 1's writeup reasoned that NARROW's 3-seed greedy set
"never happens to hit" a combat-rules interaction, and that held again through
one more rebase — until hop 2, where it didn't. **"NARROW has historically
been insensitive" is an empirical observation about specific past diffs, not a
property of the narrow set.** Every rebase re-derives all four from scratch;
none is ever assumed unchanged going in.

### 9.19 Hop 3 — WeightedBot's own resign guard (`fb9c12a`), only the two
### weighted arms moved this time

`bash tools/gate.sh` FAILed on clean master with the greedy pair (NARROW,
WIDE) still `OK` and only WNARROW/WWIDE off — the mirror image of hop 1
(where WIDE alone moved and NARROW didn't). That localises the change to
WeightedBot before looking at a single diff.

**Master moved twice under this hop, which is itself worth recording.** The
FAIL was first observed at `e8c9062`. `git diff 3439b0e..e8c9062 --stat --
engine/` showed exactly two files: `engine/bots/plan.py` (new, additive —
PlanBot, not on the `perf_check` path) and `engine/bots/weighted.py`, whose
*entire* diff across that whole multi-commit range was byte-for-byte commit
`fb9c12a`'s 18 lines and nothing else. Before committing anything, master was
re-checked and had already advanced to `52a4cb6` (ten more `CULTURE_GAP`
commits). Per 9.0/9.18's rule this was **not** waved through as "probably
fine" — both sides were reset to `52a4cb6` and every arm was re-derived from
scratch there too. `git diff 3439b0e..52a4cb6 --stat -- engine/` confirmed the
same two files, same byte-for-byte `weighted.py` diff; the ten intervening
commits touch only `docs/CULTURE_GAP.md` and standalone `tools/*.py` scripts
never imported by `engine/perf_check.py`. Master was re-checked a third time
immediately before the gate.sh/PYPY.md commit below and had not moved again.

**The cause.** `fb9c12a` ("WeightedBot: guard against resign, as RandomBot
always has") adds `allow_resign=False` to `WeightedBot.__init__` and, in
`pick()`, filters `("resign",)` out of the legal moves whenever a non-resign
move exists (`engine/bots/weighted.py` ~lines 780-805). The commit message
claims it is "byte-identical for the trained champions" — true, but beside
the point for a *fingerprint* gate: `perf_check`'s weighted cases play
`DEFAULT_WEIGHTS` (`WeightedBot(rng=rng)`, no weight vector), not any trained
champion, and under `DEFAULT_WEIGHTS` a resign move is apparently scored
competitively often enough to get picked on some of the 33/102 seeds. That is
exactly why WNARROW and WWIDE moved and NARROW/WIDE (GreedyBot, which has no
`allow_resign` concept and was untouched by this commit) did not.

This is judged a legitimate, intended behaviour change, not a regression:
`docs/COVERAGE_AUDIT.md` independently established that resign is a
guaranteed loss no evaluation feature can see (nothing reads the resigned
flag; 9/12 games resigned in one 4p probe, 0 wins) — a bot that stops
resigning is strictly better, so the digests moving is the gate doing its
job, not the gate breaking.

**Derived per 9.0's rule**, against the actual final head `52a4cb6`: computed
from scratch on a fresh detached checkout (`/tmp/tta-gate-verify-A2`) and
independently in a second worktree (`gate-rebaseline-2`, branched off the
same `52a4cb6`), `nice -n 15`, at most 2 concurrent `perf_check` processes.
Both narrow arms were re-derived too, not assumed unchanged, even though the
diff audit above already predicted they would be — the full per-case digest
list (33/102/33/102 cases) was diffed key-by-key between the two sides for
all four arms and found identical in every case:

| | side A (fresh checkout) | side B (worktree) |
|---|---|---|
| narrow (33 games) | `2fd656b38729de71` | `2fd656b38729de71` (unchanged) |
| wide (102 games) | `1169007df1517e33` | `1169007df1517e33` (unchanged) |
| weighted narrow (33 games) | `7fc72fcab0726803` | `7fc72fcab0726803` |
| weighted wide (102 games) | `9dc0a5a66e2edf62` | `9dc0a5a66e2edf62` |

Full digests:

```
narrow          2fd656b38729de718361749330edf220d8a908c07000829b86708b456faf8f44
wide            1169007df1517e33681f9c567839a1ae3dc9e7c88fac6288f5549bce3328d9ba
weighted narrow 7fc72fcab07268031e113c06e49a2dea969fa5a77d25c0cd571f3713ec3039e3
weighted wide   9dc0a5a66e2edf62f38c672e14090d31c4864f3f23b35130c1953f18fe66fb71
```

`tools/gate.sh` now gates on these four (narrow/wide unchanged from 9.18;
WNARROW/WWIDE updated with the cause above written inline). `tools/
fingerprint.json` / `tools/fingerprint_wide.json` needed no re-save this time
— narrow and wide didn't move, so the files `perf_check save` had already
written at hop 2 were still correct; diffed to confirm rather than assumed.
`bash tools/gate.sh` (full, both narrow and wide, both plain and
FASTCOPY_PARANOID):

```
unittest                         OK   Ran 254 tests
unittest JOURNAL_PARANOID        OK   Ran 254 tests
narrow fingerprint                OK   2fd656b38729de71
narrow FASTCOPY_PARANOID          OK   2fd656b38729de71
weighted narrow                   OK   7fc72fcab0726803
wide fingerprint                  OK   1169007df1517e33
wide FASTCOPY_PARANOID            OK   1169007df1517e33
weighted wide                     OK   9dc0a5a66e2edf62
GATE PASS
```

(254, not 248 or 135's-worth of a stray count — `tests/test_quiescent.py`
landed since 9.18 and the unit-test count in `gate.sh`'s header comment was
stale; corrected in passing.)

Negative control, run in the same worktree after the PASS above: bumped
`BASE_WEIGHTS["culture"]` from `1.0` to `1.5` in `engine/bots/weighted.py`,
re-ran `bash tools/gate.sh --fast`. Result:

```
weighted narrow                  FAIL 922f9aae8f2721c1 != 7fc72fca...
GATE FAIL
```

— narrow (GreedyBot) still `OK`, exactly as expected for a WeightedBot-only
perturbation. Reverted with `git checkout -- engine/bots/weighted.py`,
diffed byte-for-byte against a pre-change backup to confirm the revert was
exact, then re-ran `--fast` to confirm `GATE PASS` again before leaving the
worktree. No artifact left behind.

Updated table for 9.18's row, current as of `52a4cb6`:

| | GreedyBot | WeightedBot |
|---|---|---|
| 135-game paranoid suite | `2fd656b3` / `1169007d` | `7fc72fca` / `9dc0a5a6` |

Lesson worth keeping, on top of 9.18's: **a re-baseline is only as good as
the head it was derived against, and that head can move while you are still
deriving it.** This hop's FAIL was first read on `e8c9062`; by the time both
sides had finished the first full pass, master was `52a4cb6`. The fix is not
to derive faster, it is to re-check immediately before trusting the numbers
and redo the whole two-sided derivation against whatever the actual head
turns out to be — cheap here because the intervening commits were docs/tools
only, but the check has to happen regardless of how the diff turns out,
because the alternative is exactly the failure mode this section exists to
name: a baseline that was already stale the moment it landed.

### 9.20 Hop 4 — the scoring bugfix (`score-bugfix`), all four arms moved and
### the attribution is exact

`docs/SCORE_BUGFIX.md` changed four things in `engine/`, deliberately, so this
is not a rebase hop (though master did move under it, to `9c8b6f5`, and all
four were re-confirmed after rebasing -- the new `engine/bots/human/` package
is additive and off the `perf_check` path, and moved nothing): the gate was *expected* to fail and the job was to say
precisely which change moved which arm.

Old: `NARROW 2fd656b3`, `WIDE 1169007d`, `WNARROW 7fc72fca`, `WWIDE 9dc0a5a6`.
New: `NARROW 0a6ed6ad`, `WIDE 4a8c6ca6`, `WNARROW 302c546c`, `WWIDE 4e40a58c`.

**Attributed by reverting each of the four fixes on its own and re-hashing all
four arms**, which is stronger than the usual "read the diff and reason about
it" and was cheap (narrow 6.5s, wide 39s, weighted wide 2m28s per arm):

| revert | NARROW | WIDE | WNARROW | WWIDE |
|---|---|---|---|---|
| `Impact of Industry` (mine production, not the rating) | SAME | SAME | `142b3371` | `d7328f3a` |
| `Impact of Population` (count unused workers) | **`2fd656b3`** | **`1169007d`** | `4ce2cf6e` | `ecbfc9dd` |
| Hollywood/Internet effective output | SAME | SAME | SAME | SAME |
| Chaplin doubles one theater, not a card | SAME | SAME | SAME | SAME |
| the two `Impact of ...` fixes **together** | — | — | **`7fc72fca`** | **`9dc0a5a6`** |

The bold cells are the proof, and they are better than a narrative cause:
reverting one fix puts the two GreedyBot arms back on their old digests to the
byte, and reverting two puts the two WeightedBot arms back on theirs. There is
no residue to explain.

**Two of the four engine changes move no digest at all, and that is a coverage
statement about this gate, not a statement about those changes.** The 135
games essentially never complete an Age III wonder (measured: one Hollywood in
80 seat-games for the *trained* production vector, zero for GreedyBot and
DEFAULT_WEIGHTS), and never reach Charlie Chaplin holding two workers on his
best theater. `tools/gate.sh` therefore cannot catch a regression in either —
`tests/test_scoring_bugfix.py` and `tools/bgo_rescore.py` are the only guards
that can. 9.14's lesson generalises: the fingerprint covers the *code paths
its bots execute*, and a rules fix in a card almost nobody plays is invisible
to it no matter how many arms it has.

Lesson worth keeping, and it cost an hour here: **a script that patches the
tree in place must restore it in a `finally`, and nothing else may be
measured while it runs.** A first attribution pass was killed by a timeout
mid-hash and left `engine/effects.py` in a reverted state — `git status`
still said "modified" (the new helper was there), so it looked normal, and a
concurrently-launched measurement process imported the reverted module at
start-up and produced numbers that were wrong by exactly one game. Both were
caught by re-deriving on a verified-clean tree and diffing against the
recorded run; the fix is a `try/finally` and a rule not to overlap.

## 10. The undo stack reaches the bots the league actually trains

Section 9 delivered 1.40–1.75x and 9.16 closed with "`QuiescentBot` itself
still needs either a stacked journal or design B before it can leave the copy
path. It is 0% of league seats today, so this is not urgent." **That stopped
being true.** `experiments/run_league.sh` now trains

| arm | `--candidate-bot` |
|---|---|
| 2p | `plan:width=2` (`engine/bots/plan.py`) |
| 3p | `quiescent:levels=1` (`engine/bots/quiescent.py`) |
| 4p | `quiescent:levels=1` |

and **both of those bots were pinned to `copy_state`**, so section 9's win
reached none of the CPU the box was actually spending. This section converts
them. Everything below was measured on 2026-07-29.

### 10.0 First correction: the profile that motivated this was wrong by 2x

The brief that sent me here quoted a sampling profile of `plan:width=2` at 2p
putting `copy_state` at "~50% inclusive". That profile had **16 samples**.
Re-taken properly (`tools/profile_bot.py`, 2 ms sampling, 4266–4958 samples per
cell, `nice -n 15`, league running):

| cell | samples | cpu-s | `copy_state` INCL | `weighted.evaluate` INCL | `actions.apply` INCL |
|---|---|---|---|---|---|
| `plan:width=2` 2p | 4525 | 35.5 | **24.2%** | 45.8% | 15.6% |
| `quiescent:levels=1` 2p | 4590 | 33.2 | **23.1%** | 48.8% | 15.9% |
| `plan:width=2` 4p | 4958 | 42.4 | **28.2%** | 47.6% | 11.9% |
| `quiescent:levels=1` 4p | 4266 | 29.6 | **24.0%** | 53.4% | 7.4% |

Profiled with the league's own champion vectors, not `DEFAULT_WEIGHTS`, via a
new `tools/profile_bot.py --weights` — `tools/arch_cost.py` records that the
default vector systematically understates what a search bot costs.
`tools/profile_bot.py --kind` now also accepts the league's own spec strings,
so a profile quotes what the trainer was actually launched with.

Self-time, top of each 4p cell, for the record:

| `plan:width=2` 4p | SELF % | `quiescent:levels=1` 4p | SELF % |
|---|---|---|---|
| `_copy_PlayerState` | 15.6 | `_copy_PlayerState` | 13.2 |
| `weighted.features` | 8.0 | `weighted.features` | 8.7 |
| `weighted.evaluate` | 7.1 | `weighted.evaluate` | 7.7 |
| `effects.compute` | 6.6 | `effects.compute` | 7.5 |
| `_copy_gs_nolog` | 5.5 | `weighted.card_potential` | 5.1 |

**So the ceiling for removing 100% of the copying is 1.30–1.39x, not 2x.**
These two bots spend about half their time in `evaluate` — ~78 weights over ~57
features plus `hand_potential` — which is the same reason 9.15 measured
WeightedBot at 1.44x against GreedyBot's 1.75x, one step further along. Any
claim above ~1.3x for this work was arithmetic-impossible, and it is worth
knowing that *before* building rather than after.

### 10.1 The census: what fraction of these copies is discard-shaped

`tools/copy_census.py` (new) wraps `copy_state` in each bot module, counts by
call site, and — for the beam — detects **survivors**: a copy is a survivor if
it is later passed to `copy_state` again as the source, i.e. it survived the
prune and got expanded at the next ply. Every copy is kept alive for the
duration of one root decision so `id()` cannot be recycled underneath the
measurement, and survivors are counted once per surviving *state*, not once per
child it spawns. (The first version of the tool counted the latter and reported
83% survival — the same number read backwards. Worth recording because it is a
plausible-looking answer that would have killed the design.)

| cell | copies | by site | discard-shaped |
|---|---|---|---|
| `quiescent:levels=1` 2p, 20 games | 49255 | `pick:289` 93.4%, `_pick:174` 6.3%, `war_value:217` 0.3% | **100.0%** |
| `quiescent:levels=1` 4p, 4 games | 48970 | `_pick:174` 50.3%, `pick:289` 49.2%, `war_value:217` 0.5% | **100.0%** |
| `plan:width=2` 2p, 3 games | 52438 | `_beam:223` 91.1%, `war_value` 4.7%, `_one_ply:195` 3.1%, `pick:180` 1.2% | **90.3%** |
| `plan:width=2` 4p, 1 game | 53676 | `_beam:223` 80.3%, `_one_ply:195` 9.8%, `war_value` 9.1%, `pick:180` 0.8% | **92.0%** |

Inside `_beam` alone: **10.6% of copies survive the prune at 2p, 10.0% at 4p.**
The other ~90% are made, scored and thrown away.

Two things fall out that the design has to respect:

1. **QuiescentBot needs nothing but nesting.** 100.0% of its copies are
   copy-apply-score-discard and its call graph is a strict stack: `pick` →
   `_resolve` → `_pick` → `war_value` is depth 3.
2. **`_pick`'s share of QuiescentBot's copies is 6% at 2p and 50% at 4p.** A
   coverage claim made at one player count is not a claim at another; the
   fingerprint cases below cover all three for that reason.

### 10.2 The gate could not see either bot — four new arms, derived two-sided

9.14's lesson ("no digest in this project can catch a change to WeightedBot")
recurs verbatim one league re-target later: none of the four existing arms
plays PlanBot or QuiescentBot, so **no digest could catch a change to either**,
and converting them behind the greedy/weighted gate would have been a green
gate proving nothing about the lines that changed.

`engine/perf_check.py` grows a `plan` bot kind (width 2, as the league runs it —
and the interesting case for the prune: at width 8 nearly every node survives,
at width 2 nearly none does) plus `plan_cases()` / `quiescent_cases()` behind
`--plan` / `--quiescent`. Sized by cost rather than by symmetry with the 33/102
greedy split, because a 2p PlanBot game is ~4 cpu-s and a 4p one ~16 against
~0.15 for a greedy game: plan narrow 3 games / wide 6, quiescent narrow 9 /
wide 24.

Derived on master `419012e` per 9.0's rule — from scratch on the working
worktree at the pre-conversion commit, and independently in a second detached
checkout of the same commit, requiring agreement:

```
plan narrow      ad64a55b57cae4c73d6d144ea8280dc8816599d72583f81835d98784aa7ccd7d
plan wide        441cd256ec5c989ec8ba2d19cc1cf1e3b7d6f7e7d8b1742427513575c30a8501
quiescent narrow 0e90a7e631c3d25f35cf8a6945c497792172a47bd3ade33d4eaee4eaa94bbecd
quiescent wide   41f078e5a6c2cf5d7b9a0c79cf13f659cb2d5bee3b4397bc37e2d6fd86ab61bb
```

### 10.3 The journal change: a strictly-LIFO stack, and a copy that detaches

Two edits to `engine/journal.py`, one to `engine/bots/fastcopy.py`.

**Nesting.** `begin` pushes onto `_STACK`; `rollback` refuses anything but the
innermost journal. The invariant that makes the nested case correct is one
line: *each journal records the pre-state of everything mutated while IT is
innermost*, so rolling it back restores exactly its own `begin` state, by
induction on depth. Both mechanisms already had that property and neither
needed changing — the `__setattr__` hook appends to `_J`, and `touch`'s `seen`
set is per-journal, so an object touched at depth 2 is snapshotted again there
even though depth 1 already snapshotted it. `SUPPRESS_LOG` is now lifted only
when the stack empties.

LIFO is *enforced*, not assumed. An out-of-order rollback restores container
contents in the wrong order, which is hazard 3 of 6.5 and the one corruption
`==` cannot see (9.1: `statediff` compares dict key order deliberately).

**`copy_state` inside an open journal.** This used to raise, and that raise is
precisely what pinned both bots to the copy path — 9.16 called it "the right
shape: the failure is loud or it does not exist". It is now a `detach`/`attach`
bracket instead, on one argument: **a copy allocates fresh objects and writes
only to them, and it never mutates its source.** So detaching cannot lose a
record. Journalling those writes, on the other hand, would have rollback empty
a copy the caller is still holding — that is the corruption the raise was
protecting against, and the reason the fix is "detach" rather than "ignore
`__dict__` writes". The `__dict__` branch of `_journalling_setattr` still
raises; it is now simply unreachable from `copy_state`. The detach is in a
`finally`, with a test that injects a raising copier — without it, one failed
copy would leave the rest of a trial unjournalled, which is the silent-real-
state-corruption failure mode the whole design exists to prevent.

The paranoid oracle needs this at every level (it copies the state at each
`begin`), and PlanBot needs it to materialise survivors.

**9.16's standing constraint is unchanged and now has a second name in it.**
`_STACK` is a module global exactly as `_J` is, so the harness must stay
process-parallel (`multiprocessing`, as `experiments/arena.py` is) — a
thread-parallel harness would corrupt states across threads silently. Anyone
moving the league to threads must put **both** `_J` and `_STACK` in a
`threading.local` first.

### 10.4 QuiescentBot — a pure nest; three sites, nothing else to arrange

`pick`, `_pick` and `war_value` each grow a `_journalled` twin behind
`USE_JOURNAL`, following `WeightedBot._pick_journalled` line for line. The copy
paths stay exactly as they were, because they are the paranoid oracle.

Two things are deliberately *outside* the journal and it matters:

* `box` (the node budget) and `self.stats` are plain Python objects owned by
  the bot and not reachable from the state, so nothing journals them and budget
  consumption is bit-identical to the copy path. A different budget is a
  different search.
* `root_ctx` is computed once by `pick` on the unmutated state. Under the
  journal the root state *is* the object the candidates mutate, so capturing it
  before the loop is load-bearing rather than incidental — the same point 9.14
  records for WeightedBot's `ctx`.

`war_value`'s docstring promises "``state`` is never mutated". On the copy path
that is free; on the journal path it is the rollback, so it gets its own test.

### 10.5 PlanBot's beam — measure first, then re-apply the survivors

The beam is the one place in this repo where copy-apply-score-discard does not
hold: it holds `width` states alive at once. That is exactly what 6.3 meant by
"design B's only advantage is holding many trial states alive simultaneously …
today no bot needs that". A bot needs it now — but the census says it needs it
for **10% of the nodes**, not for all of them.

`_beam_journalled` therefore

1. expands every child under a journal, with no copy at all;
2. keeps only its score and the `(parent, move)` pair that produced it;
3. sorts and prunes exactly as before;
4. **re-applies** the ~`width` winners onto fresh copies of their parents.

That trades `width` extra `apply`+`_quiesce` per ply for `nodes − width`
copies, which the census prices at roughly 9:1 in our favour.

Re-applying is exact, not approximate, and rests on a property that was already
there for a different reason: `_rng()` hands out a Mersenne Twister that is
always at the *start* of the `Random(0)` stream (it re-seeds lazily, iff the
previous consumer drew — 8.2). So a child is a deterministic function of
`(parent, move)` and applying it twice cannot diverge, however many rng calls
happen in between. `self.nodes`, `self.searches`, `self.wars_priced` and the
`MAX_NODES` budget count expansions only; the re-apply is deliberately not
counted, and there is a test that both paths report the same `nodes`.

`_one_ply` converts too (it is called from `_quiesce`, i.e. from inside a beam
node's journal — nested by construction), and `_score`'s war lookahead inherits
`quiescent.war_value`'s conversion.

**Rejected: materialise-on-survive.** Now that `copy_state` detaches, a
survivor could be copied from *inside* its journal just before the rollback,
saving the re-apply. Not taken: the prune is global over the children of every
frontier node, so you do not know which children survive until all of them are
scored — meaning either you copy every child (the status quo) or you keep every
child's journal open until the prune, which is not LIFO. A two-pass version of
it is exactly the re-apply above with a sharper edge on it.

**Not revisited: design B (copy-on-write).** 6.6's trigger for revisiting it was
"a bot needs simultaneous live trial states", and PlanBot does — but only
`width` of them, and a beam that holds `width` real copies plus a journal for
the expansion has the same asymptotics as COW with none of COW's aliasing risk
(6.3: a missed `mutable()` corrupts the *real* game, where a missed `touch()`
only corrupts a trial and is caught by the oracle). **6.3's "no bot needs that"
is now stale in its premise and still right in its conclusion.**

### 10.6 The gate — 28 arms, and what the paranoid ones actually assert

`tools/gate.sh` grows four plain arms and eight journal arms. The journal ones
are the claim: `TTA_JOURNAL=1 JOURNAL_PARANOID=1` with PlanBot searching means
that on **every node of every beam** the state was copied, the candidate applied
by undo, rolled back, and the two structurally diffed *including dict key
order*, at every nesting level. `plan wide JOURNAL+PARANOID` is by a wide
margin the slowest arm in the file.

`bash tools/gate.sh --journal` on the final tree, one uninterrupted run:

```
unittest                          OK   Ran 546 tests
unittest JOURNAL_PARANOID         OK   Ran 546 tests
narrow fingerprint                OK   0a6ed6ad9f22e914
narrow FASTCOPY_PARANOID          OK   0a6ed6ad9f22e914
weighted narrow                   OK   302c546c8a0eb181
quiescent narrow                  OK   0e90a7e631c3d25f
plan narrow                       OK   ad64a55b57cae4c7
wide fingerprint                  OK   4a8c6ca6f31afc9c
wide FASTCOPY_PARANOID            OK   4a8c6ca6f31afc9c
weighted wide                     OK   4e40a58c196f5b3a
quiescent wide                    OK   41f078e5a6c2cf5d
plan wide                         OK   441cd256ec5c989e
narrow JOURNAL                    OK   0a6ed6ad9f22e914
narrow JOURNAL+PARANOID           OK   0a6ed6ad9f22e914
wide JOURNAL                      OK   4a8c6ca6f31afc9c
wide JOURNAL+PARANOID             OK   4a8c6ca6f31afc9c
weighted narrow JOURNAL           OK   302c546c8a0eb181
weighted narrow JOURNAL+PARANOID  OK   302c546c8a0eb181
weighted wide JOURNAL             OK   4e40a58c196f5b3a
weighted wide JOURNAL+PARANOID    OK   4e40a58c196f5b3a
quiescent narrow JOURNAL          OK   0e90a7e631c3d25f
quiescent narrow JOURNAL+PARANOID OK   0e90a7e631c3d25f
plan narrow JOURNAL               OK   ad64a55b57cae4c7
plan narrow JOURNAL+PARANOID      OK   ad64a55b57cae4c7
quiescent wide JOURNAL            OK   41f078e5a6c2cf5d
quiescent wide JOURNAL+PARANOID   OK   41f078e5a6c2cf5d
plan wide JOURNAL                 OK   441cd256ec5c989e
plan wide JOURNAL+PARANOID        OK   441cd256ec5c989e
GATE PASS
```

The four pre-existing digests (`0a6ed6ad` / `4a8c6ca6` / `302c546c` / `4e40a58c`)
are unchanged from 9.20 throughout, which is the check that nothing in
section 10 leaked into GreedyBot's or WeightedBot's paths — the journal is
shared machinery and the `copy_state` detach in particular is on every bot's
copy path, so those four arms are not a formality here.

`tools/mutation_coverage.py` re-run for **all four** searching bots (9.14: the
bots do not cover the same sites, so one bot is not an audit), 2 games per
player count plus the traced test suite (1 game for plan, which is 40x the cost
per game):

| searching bot | converted sites never executed | unconverted (local) |
|---|---|---|
| greedy | **0 / 62** | 11 / 106 |
| weighted | **0 / 62** | 11 / 106 |
| quiescent | **0 / 62** | 11 / 106 |
| plan | **0 / 62** | 11 / 106 |

Negative controls, because 9.11's lesson is that a first-try pass is exactly
when to distrust the instrument. Both are permanent tests, not one-off probes:

* `journal.touch` replaced by the identity → the QuiescentBot search comes back
  with a corrupted state, and the check fires. It fires *differently* depending
  on the environment — with `JOURNAL_PARANOID=1` the oracle inside `rollback`
  raises first and names the path, without it the corruption survives to the
  structural diff — so the test accepts either signal. Accepting only one would
  have made the control silently vacuous under the gate's paranoid unittest
  arm, which is the "test that asserted nothing" of 9.11 in a new costume.
* `PlanBot._replay` made to rebuild the *parent* instead of the child → the two
  beams' score dicts diverge. That is the test that the re-apply is doing real
  work; without it, `test_beam_returns_identical_scores` could pass on a beam
  that never re-applied anything.

### 10.7 The digests only play `DEFAULT_WEIGHTS` — checked against the real champions too

9.20's lesson is that the fingerprint covers *the code paths its bots execute*,
and `perf_check`'s bots play `DEFAULT_WEIGHTS`. A trained champion attacks far
more (docs/DEEPER_SEARCH.md 3.1), so it reaches wars, pacts and auctions at
quite different rates — exactly the move classes whose journalling is hardest.
So the same games were played under `experiments/league_state/champion_{2,3,4}p
.json`, journal off and on, hashing the **full game log, the final scores and
the move count**:

```
plan       2p champion_2p   OFF ac765ada6c653dc4   ON ac765ada6c653dc4   SAME
quiescent  3p champion_3p   OFF 4a74693b31acad5e   ON 4a74693b31acad5e   SAME
quiescent  4p champion_4p   OFF 23147cb2c6e148e6   ON 23147cb2c6e148e6   SAME
```

Those are the three cells `run_league.sh` is running right now, at the exact
specs it is running them with.

### 10.8 MEASURED throughput — 1.25x to 1.56x, and it grows with state size

Method: one subprocess per arm (`TTA_JOURNAL` is read at import), the two arms
**interleaved within each round**, three rounds, `nice -n 15`,
`time.process_time`. Both arms play the **same seeds** — and, because the
change is byte-identical, literally the same games — so cpu-seconds per game is
like-for-like and none of the game-length noise 9.12 warned about can leak in.

**All of these are post-renice.** The box's league arms were moved to `nice 19`
part-way through this session; every number in this table was taken after that,
and no arm of any pair straddles it. Load average during the run was 12–15 on 6
cores, so absolute cpu-s/game are not comparable with any earlier section.

| cell | journal OFF (cpu-s/game) | journal ON | speed-up | per-round range |
|---|---|---|---|---|
| `quiescent` 2p | 0.425 ± 0.004 | 0.322 ± 0.011 | **1.32x** | 1.29–1.37 |
| `quiescent` 3p | 0.919 ± 0.018 | 0.670 ± 0.012 | **1.37x** | 1.35–1.40 |
| `quiescent` 4p | 2.130 ± 0.131 | 1.461 ± 0.086 | **1.46x** | 1.44–1.47 |
| `plan:width=2` 2p | 3.729 ± 0.145 | 2.983 ± 0.132 | **1.25x** | 1.19–1.35 |
| `plan:width=2` 2p, re-run | 4.203 ± 0.062 | 3.319 ± 0.236 | **1.27x** | 1.20–1.41 |
| `plan:width=2` 3p | 8.450 ± 0.298 | 6.054 ± 0.443 | **1.40x** | 1.34–1.47 |
| `plan:width=2` 4p | 15.895 ± 0.438 | 10.179 ± 0.402 | **1.56x** | 1.51–1.60 |

(± is the sample standard deviation over the rounds. 12 games/round at
quiescent 2p down to 1 game/round at plan 4p, warm-up game excluded. The
`plan` 2p re-run is 5 games/round over 4 rounds rather than 3 over 3.)

**The honest reading, cell by cell.** Five of the six cells have a per-round
range narrower than the effect and can be quoted as measured. **`plan` 2p
cannot**, and it is annoyingly the league's actual 2p cell, so it was measured
twice — seven post-renice rounds in total, whose ratios are

```
1.354  1.187  1.217        (3 games/round)
1.227  1.245  1.204  1.409 (5 games/round)
```

Five of the seven sit in 1.20–1.25, with one low and one high outlier, and the
two runs' means agree (1.250x and 1.266x) despite their *absolute* cpu-s/game
differing by 13% between runs — which is exactly the contention drift 9.15
describes, and exactly why the arms are interleaved. **The honest claim for
this cell is ~1.25x, not a third digit**, and the second run did not tighten the
range enough to say more. Per 9.15's rule ("the noise is contention, not
sampling") more rounds will not fix it; a quiet box would.

**The win exceeds what the profile predicts, and the excess grows with player
count.** 10.0 measured the copy at 24–28% inclusive, which caps the achievable
speed-up at 1.30–1.39x even if the journal were free — and it is not free. Yet
4p measures 1.46x and 1.56x. Two candidate explanations were checked:

* *`DEFAULT_WEIGHTS` vs champion weights.* Real, and part of it: re-profiling
  `quiescent` 4p with `DEFAULT_WEIGHTS` (what the A/B plays) puts `copy_state`
  at **27.7%** rather than the 24.0% the champion-vector profile showed. That
  moves the ceiling to 1.38x. It does not close the gap.
* *Garbage collection.* Ruled out. Running the copy-path arm with `gc.disable()`
  changes `quiescent` 4p from 2.379 to 2.427 cpu-s/game — i.e. nothing, within
  noise. The ~2 million container allocations a 4p game's copies make are
  acyclic and die by refcount, so the cyclic collector was never the cost.

The remaining candidate, offered as a **hypothesis and not as a measurement**,
is cache footprint: a 4p `copy_state` moves ~40 KB, and a game makes ~50000 of
them, so the copy evicts the working set that `evaluate` is about to read. That
cost is real CPU time but a sampling profiler charges it to whichever frame
stalls, so it is invisible in `copy_state`'s inclusive share. The supporting
pattern is that the *excess over the Amdahl prediction grows monotonically with
player count* — 2p 1.25–1.32x against a ~1.35x ceiling (at or below), 4p
1.46–1.56x against a ~1.38x ceiling (well above) — which is what a
state-size-driven effect looks like and is not what a fixed overhead looks
like. Anyone who wants to settle it should count L2 misses, not re-run the A/B.

**What this is worth to the league.** The 3p and 4p arms train
`quiescent:levels=1` (1.37x, 1.46x) and the 2p arm trains `plan:width=2`
(~1.25x). `experiments/run_league.sh` already exports `TTA_JOURNAL=1` for
section 9's sake, so **all three arms pick this up on their next hourly
restart with no further action** — and, per 10.7, with byte-identical play.



### 10.9 Two things that did NOT work, and one profiler line that is a lie

**The census tool's first survivor definition was wrong and looked right.**
Counting "copies whose *source* was a survivor" instead of "distinct states
that survived" reported 83.4% survival at `plan:width=2` 2p — i.e. it said the
beam's copies were nearly all load-bearing and the whole idea was dead. The
number is not absurd on its face (a beam does reuse its frontier), which is
what makes it dangerous. The tell was arithmetic: at width 2 with ~10 legal
moves, 79 copies per root decision cannot contain 66 surviving states. Anyone
writing a survivorship counter should sanity-check it against `width × plies`
before believing it.

**`bots/quiescent.py:_fresh` is 5.0% of the sampling profile and 0.01% of the
runtime.** It appears at 4.99% SELF at 4p and 5.03% at 2p, which reads like a
free win sitting in a six-line function. Bounded directly instead, per 8.3:
over two 4p games, `_fresh` is called 17546 times and actually re-seeds **65 of
them (0.4%)**, exactly matching 8.2's measurement of how often a trial `apply`
draws; `timeit` puts `setstate` at 10.26 us, so the whole re-seeding cost is
0.001 cpu-s of 4.83 — **0.01%**. Even charging every call a full microsecond
only reaches 0.36%. So the sampler over-attributes this frame by more than two
orders of magnitude.

That is the **fourth** time the 2 ms sampler has inflated a small,
frequently-entered frame on this project (5a→8.1 for `random.__init__`, 9.15
for the same item on WeightedBot, and now this). 8.3's rule should be read as
binding, not advisory: **a sampling-profile line under ~10% on this box is not
evidence.** Bound it with cost × count or with a probe that deletes the work,
before writing any code.

**The 16-sample profile in 10.0 is the third failure of the same kind**, one
level up: it is not that the sampler was biased, it is that 16 samples has no
resolution at all. `tools/profile_bot.py` prints its sample count in the header
for exactly this reason; if the number is not in the hundreds, the table under
it is decoration.

### 10.10 Status and what is left

- [x] `journal.begin` nests, strictly LIFO; `copy_state` detaches instead of
      raising (10.3).
- [x] `QuiescentBot` converted — all three copy sites (10.4).
- [x] `PlanBot` converted — `_beam` by re-apply-for-survivors, plus `_one_ply`
      and the war lookahead (10.5).
- [x] `plan` / `quiescent` fingerprint arms, derived two-sided, in
      `tools/gate.sh` (10.2). **This is the durable part**: those two bots now
      have a determinism gate at all, which they did not before, whatever
      happens to the undo stack.
- [x] Coverage re-audited for all four searching bots (10.6).
- [ ] `engine/bots/neural_plan.py`'s `NeuralPlanBot` has its own `_beam` and its
      own `war_value` and reads no `USE_JOURNAL`, so it is untouched and still
      copies. It is the same shape as `PlanBot._beam` and would take the same
      treatment; nobody is training it on this box today, so it was left alone
      rather than converted blind.
- [ ] `evaluate` is now unambiguously the largest line item for both bots
      (45.8–53.4% inclusive), and `effects.compute` inside it is 17–26%. That
      is where the next 1.2x lives, and it is a different kind of work from
      everything in sections 4–10: not "copy less" but "price fewer features
      per candidate".
- [ ] Re-run `tools/mutation_coverage.py` with all four bots after any engine
      change that adds a container mutation. The `--bot plan` arm is the
      expensive one; 1 game per player count is enough.

## 11. The re-test section 3 asked for — run at last, and the answer is now
## BOT-DEPENDENT: PyPy wins the bots the league does not run

Section 3 measured GreedyBot/RandomBot on CPython 3.14.6 vs PyPy 7.3.23, found
PyPy losing every cell by 11–44%, and both status checklists then said, twice:
**"Re-test PyPy *after* [the undo stack] lands, not before."** The undo stack
landed (`17c03ea`, `47c0e5b`, `ae20f2b`; section 9). Nobody had re-tested. This
is that re-test, at master `9794bd7`.

> **Read this before quoting the `plan` / `quiescent` rows.** Everything below
> was measured at `9794bd7`, i.e. **before** section 10's `7ef6ac8` put
> PlanBot and QuiescentBot on a nested undo stack. At `9794bd7` those two bots
> searched by `copy_state` and opened zero journals (11.3 measures exactly
> that), so their `TTA_JOURNAL=0` rows are their only rows and section 10 has
> since changed the code underneath them. What that does to *this* comparison
> is not knowable from either section alone: section 10 measures the journal's
> win on CPython (1.25x at `plan:width=2` 2p, 1.37x/1.46x at
> `quiescent:levels=1` 3p/4p) and this section measures PyPy against CPython
> on the copy path. The two effects are not independent — 11.4 shows the
> journal changes PyPy's *ratio* on GreedyBot (up) and on WeightedBot (down),
> in opposite directions — so **the 3p/4p league rows in 11.6 must be re-taken
> on top of `7ef6ac8` before anyone acts on them.** The verdict in 11.10 does
> not turn on that (it is a "do not switch", and section 10 makes the CPython
> side faster, which can only make switching *less* attractive), but the
> numbers do.

Two things make it a different measurement rather than a re-run of section 3's
command, and both turned out to matter:

1. **The workload changed shape.** Section 3 benchmarked `greedy` and `random`.
   The league today runs neither. The live `--candidate-bot` flags are
   `plan:width=2` on the 2p arm and `quiescent:levels=1` on the 3p and 4p arms,
   against a pool of `WeightedBot` seats — deep search, long games, a tight
   `copy_state` / `actions.apply` / `evaluate` loop.
2. **The journal changed what is hot.** Under `TTA_JOURNAL=1` (which
   `experiments/run_league.sh:22` sets) WeightedBot's inner loop is an undo
   stack driven by a `__setattr__` hook installed on the state dataclasses.

### 11.1 Method, and what is different from section 3

Same tool (`tools/bench_interp.py`), same metric (`time.process_time` — CPU
seconds burnt by the benchmark process, never wall clock), same `nice -n 10`,
same discipline of a CPU-seconds warm-up. Four changes:

* **`--kinds` now takes the league's own bot specs.** `plan:width=2` and
  `quiescent:levels=1` mean here exactly what they mean on the
  `hillclimb_league` command line. Before this, `bench_interp` could not
  express the bots the project actually trains.
* **`--games N` measures a FIXED SET OF SEEDS**, not a fixed number of
  CPU-seconds. This matters at the top of the table: a 4p `plan:width=2` game
  is ~16 CPU-s, so a 30 s window compares CPython on seeds 0–2 against PyPy on
  seeds 0–1 — two different workloads. With a fixed seed set the two
  interpreters play *byte-identical* games (11.8 is what licenses that) and
  every game is a **paired** observation.
* **CPython and PyPy run back to back inside each cell**, and each cell is
  repeated 3 times (5 for the two noisiest). The ratio of a back-to-back pair
  is the result; the absolute is not (11.2).
* **`--opponent` and `--hook`** were added for the two cells that model a real
  league worker rather than a bot playing itself.

### 11.2 The contention caveat, stated with numbers rather than adjectives

6 physical cores, no hyperthreading, no E-cores. Five `hillclimb_league`
workers were CPU-busy throughout at `nice 19`; load average 7.4–10.1. The
benchmark process got 85–90% of a core.

CPU-time-per-game is far more stable than wall clock here but it is **not**
load-independent — cache and memory-bandwidth contention inflate it. Measured,
not assumed: the same cell measured 1.4576 and then 1.0798–1.1383 games/cpu-s
on CPython across five repeats, a 35% spread, with no code change. **Only
back-to-back A/B pairs are trustworthy on this box.** Every ratio below comes
from a pair measured within ~60 s of itself; no absolute number here should be
compared with an absolute number from any other section.

A free replication fell out of a regime change mid-run: the league arms were
reniced to 19 partway through, so the first pass (archived, not used in the
tables) is a second measurement of the same cells under *heavier* contention.

| cell | PyPy/CPython, league at nice 0 | PyPy/CPython, league at nice 19 |
|---|---|---|
| greedy 2p j0 | 1.463 | 1.445 |
| greedy 2p j1 | 1.566 | 1.597 |
| greedy 4p j0 | 1.432 | 1.465 |
| greedy 4p j1 | 1.816 | 1.648 |
| quiescent 2p | 0.941 | 1.009 |
| quiescent 4p | 0.873 | 0.919 |
| weighted 2p j0 | 1.008 | 0.972 |
| weighted 2p j1 | 0.874 | 0.938 |
| weighted 4p j0 | 0.873 | 0.861 |
| weighted 4p j1 | 0.813 | 0.809 |

Ten cells, two scheduling regimes, same sign and same rough magnitude in all
ten. The contention level is not what produces the result.

### 11.3 The cells that were skipped, and the measurement that justifies it

`TTA_JOURNAL=1` was **not** run for `plan:width=2` or `quiescent:levels=1`,
because at this commit it is structurally a no-op for them. That is measured,
not read off the source: counting `journal.begin` calls over one full 2p game,

```
USE_JOURNAL = True
greedy     journal.begin calls in one 2p game: 1200
weighted   journal.begin calls in one 2p game: 1386
plan       journal.begin calls in one 2p game:    0
quiescent  journal.begin calls in one 2p game:    0
```

Both bots search by `copy_state` (9.16: `journal.begin` raises on nesting, and
QuiescentBot holds several live trial states at once), and `journal.install()`
is lazy, so a process running only those bots never even gets the hook. The
journal-on and journal-off cells would be the same code.

They would *not* be the same code in a mixed league worker, where a WeightedBot
seat installs the hook process-wide. That case is measured instead, twice: with
`--hook` (11.5) and in the league-shaped cells (11.6).

### 11.4 The main table — {CPython 3.14.6, PyPy 7.3.23} x bot x players x journal

games/cpu-s, mean over repeats, +/- sample SD. The ratio column is the mean of
the per-repeat back-to-back ratios, with the full [min, max] over repeats,
because that range is the honest error bar and the mean alone is not.

| bot | np | j | CPython | PyPy | PyPy/CPython | [min, max] | n |
|---|---|---|---|---|---|---|---|
| greedy | 2p | 0 | 5.301 +/- 0.297 | **7.659 +/- 0.939** | **1.45** | [1.23, 1.62] | 5 |
| greedy | 2p | 1 | 7.663 +/- 0.409 | **12.241 +/- 0.694** | **1.60** | [1.51, 1.71] | 3 |
| greedy | 4p | 0 | 1.282 +/- 0.081 | **1.879 +/- 0.151** | **1.46** | [1.42, 1.49] | 3 |
| greedy | 4p | 1 | 2.445 +/- 0.128 | **4.030 +/- 0.159** | **1.65** | [1.60, 1.72] | 3 |
| plan:width=2 | 2p | 0 | 0.2428 +/- 0.0016 | **0.3007 +/- 0.0064** | **1.24** | [1.20, 1.26] | 3 |
| plan:width=2 | 4p | 0 | 0.0638 +/- 0.0023 | **0.0714 +/- 0.0008** | **1.12** | [1.08, 1.14] | 3 |
| quiescent:levels=1 | 2p | 0 | 2.307 +/- 0.020 | 2.327 +/- 0.035 | 1.01 | [1.00, 1.02] | 3 |
| quiescent:levels=1 | 4p | 0 | **0.4379 +/- 0.0116** | 0.4025 +/- 0.0054 | 0.92 | [0.91, 0.93] | 3 |
| weighted | 2p | 0 | **3.146 +/- 0.060** | 3.059 +/- 0.076 | 0.97 | [0.92, 1.00] | 3 |
| weighted | 2p | 1 | **3.972 +/- 0.031** | 3.728 +/- 0.175 | 0.94 | [0.90, 0.98] | 3 |
| weighted | 4p | 0 | **0.788 +/- 0.037** | 0.678 +/- 0.020 | 0.86 | [0.80, 0.94] | 5 |
| weighted | 4p | 1 | **1.184 +/- 0.155** | 0.958 +/- 0.036 | 0.82 | [0.63, 0.90] | 5 |

Bold is the winner. Two notes on the last row: its CPython spread is one
outlier (1.4576 on repeat 1, then 1.0798 / 1.1290 / 1.1383 on the rest); with
that repeat dropped the ratio is 0.86 [0.85, 0.90]. Either way PyPy loses it,
and the pessimistic-for-the-claim reading (0.90) still loses.

RandomBot, section 3's other bot, re-taken in the current tree for continuity
(3 repeats, 100 games per measure window): **2p 0.92x [0.88, 0.98], 4p 0.81x
[0.78, 0.82]**. Section 3 measured 0.56x and 0.89x. So the pure engine loop
with no search in it still favours CPython, exactly as it did in July.

**The verdict is no longer uniform.** PyPy is 1.45–1.65x faster on GreedyBot,
1.12–1.24x faster on PlanBot, a wash on QuiescentBot at 2p, and 0.81–0.94x —
i.e. slower — on RandomBot, QuiescentBot 4p and WeightedBot everywhere. The
undo stack did move the needle: on GreedyBot, turning `TTA_JOURNAL` on improves
PyPy's ratio (1.45 -> 1.60 at 2p, 1.46 -> 1.65 at 4p). On WeightedBot it makes
it slightly worse (0.97 -> 0.94, 0.86 -> 0.82).

### 11.5 The write-barrier tax on the copy path is under 1%, on both
### interpreters

The worry that a Python-level `__setattr__` on four dataclasses would be a
megamorphic write barrier punishing the bots that still search by `copy_state`
turns out to be small. `--hook` installs the hook and opens no journal — what
QuiescentBot sees in a worker where some WeightedBot seat has already searched:

| cell | hook off | hook on | tax |
|---|---|---|---|
| quiescent 2p, CPython | 2.307 | 2.289 | 0.8% |
| quiescent 2p, PyPy | 2.327 | 2.314 | 0.6% |
| quiescent 4p, CPython | 0.4379 | 0.4803 | none measurable (noise) |
| quiescent 4p, PyPy | 0.4025 | 0.4054 | none measurable (noise) |

No cell is outside its own repeat spread (the 4p rows differ by more in
the direction that would mean the hook makes things *faster*, which is how you
know you are reading noise). This retires the concern; it does not
need to be modelled in any capacity planning.

### 11.6 The cells that actually decide it — league-shaped games

A league game is not a bot playing itself. It is one candidate seat against
`n-1` pool seats, with `TTA_JOURNAL=1`, so the WeightedBot pool takes the undo
path and installs the hook while the candidate searches by copy. `--opponent
weighted` reproduces exactly that, and the candidate spec is the one the
corresponding live arm is running right now:

| league arm | candidate | CPython | PyPy | PyPy/CPython | [min, max] |
|---|---|---|---|---|---|
| 2p | `plan:width=2` | 0.4917 +/- 0.0260 | **0.5699 +/- 0.0135** | **1.16** | [1.08, 1.23] |
| 3p | `quiescent:levels=1` | **1.682 +/- 0.074** | 1.373 +/- 0.151 | 0.82 | [0.77, 0.87] |
| 4p | `quiescent:levels=1` | **0.8072 +/- 0.0191** | 0.6923 +/- 0.0525 | 0.86 | [0.80, 0.90] |

3 repeats each, back-to-back pairs. **The 2p arm would gain ~16% from PyPy; the
3p and 4p arms would lose 14–18%.**

### 11.7 Why — NOT ANSWERED, and three failed attempts are the reason

This is the part to be honest about. Three independent attempts to decompose
the whole-game result disagreed with each other by up to 2.5x *on PyPy* while
agreeing on CPython, so no mechanism is claimed here.

1. **Per-operation microbenchmark** (`tools/bench_hotspots.py`, new). PyPy wins
   nearly every primitive: `copy_state` 1.97x, `copy_state+apply` 1.61x,
   `weighted.features` 3.47x, `weighted.evaluate` 3.59x, `legal_moves` 2.02x,
   `math.fsum` 2.78x, an `lru_cache` hit 2.44x, a plain attribute write 2.29x.
   The single exception is `journal begin+apply+rollback` at 0.73x. Suspecting
   PyPy escape analysis was deleting work whose result was discarded, every
   benchmarked call was changed to store its result in a module-level `SINK`;
   the ratios moved by less than 0.1 (e.g. `copy_state` 2.24 -> 1.97). So that
   was not the artefact — but this decomposition predicts PyPy should win
   WeightedBot outright, and it does not.
2. **Real move distribution.** The micro above applies one fixed cheap move. A
   sweep that copies and applies *every legal move* over 6 mid-game states —
   the shape of a 1-ply search — gives `copy+apply` at **0.95x** rather than
   1.61x, and `journal+apply` at 1.23x. So the move distribution alone is worth
   1.7x on that row, which is a real caution about all fixed-move micros.
3. **`pick()`-level timing.** Timing each bot's own `pick` on fixed mid-game
   states says PyPy is 1.58x (Greedy) and 2.18x (Weighted) faster — 2.39x and
   2.69x on late-game states. Wrapping `pick` in a `time.process_time()` pair
   inside whole games says the opposite: PyPy 0.96x on Greedy. Instrumentation
   perturbs PyPy far more than it perturbs CPython.

Three decompositions, three different answers, one of them contradicting the
uninstrumented whole-game measurement that all five repeats agree on. **The
uninstrumented, paired, whole-game numbers in 11.4 and 11.6 are the result; the
decompositions are not evidence for anything and are recorded so that the next
person does not repeat them expecting a mechanism.** If someone wants the
mechanism, the tool to reach for is a PyPy JIT log (`PYPYLOG=jit-summary:-`),
not another Python-level timer.

### 11.8 Correctness — the determinism gate re-run under PyPy, journal included

PyPy wins somewhere, so this had to be established before any recommendation.
`tools/gate.sh`'s hard-coded digests were derived before master's last
`engine/effects.py` and `engine/events.py` changes, so they are not the
reference here: **CPython at this commit is the reference.** CPython saves,
PyPy checks — section 2's protocol, self-baselining, immune to a stale
constant. `tools/gate.sh` also grew a `PY=` override so the whole gate can be
run under either interpreter (`PY=pypy3 bash tools/gate.sh --journal`).

All eight arms, all green, at `9794bd7`:

```
                            digest (CPython, this commit)     PyPy check
narrow          (33 greedy) 0a6ed6ad9f22e914...               OK  identical
wide           (102 greedy) 4a8c6ca6f31afc9c...               OK  identical
weighted narrow (33 wtd)    302c546c8a0eb181...               OK  identical
weighted wide  (102 wtd)    4e40a58c196f5b3a...               OK  identical
narrow          TTA_JOURNAL=1                                 OK  identical
wide            TTA_JOURNAL=1                                 OK  identical
weighted narrow TTA_JOURNAL=1                                 OK  identical
weighted wide   TTA_JOURNAL=1                                 OK  identical
```

**270 games x 2 search paths x 2 interpreters, every digest identical.** Three
independent claims fall out of that one table, and it is worth naming them
separately because they are not the same claim:

* **Cross-interpreter.** PyPy reproduces CPython byte for byte on all 270
  games (33 + 102 GreedyBot, 33 + 102 WeightedBot). Section 2's `math.fsum`
  fix is still doing its job.
* **Cross-path.** The journal-on digests equal the journal-off digests on both
  interpreters — the undo stack and `copy_state` agree, under PyPy too. That
  is the property section 9's whole safety story rests on, and it had never
  been checked on PyPy.
* **Structural.** The two narrow arms re-run under
  `TTA_JOURNAL=1 JOURNAL_PARANOID=1` on PyPy — every rollback checked against a
  `copy_state` oracle and structurally diffed — also pass. A missed mutation
  site would raise there naming the attribute, not merely change a digest.

Unit tests, 536 of them:

```
python3 -m unittest discover -s tests                    Ran 536 tests in 36.9s  OK
pypy3   -m unittest discover -s tests                    Ran 536 tests in 64.4s  OK
JOURNAL_PARANOID=1 pypy3 -m unittest discover -s tests   Ran 536 tests in 63.6s  OK
```

(PyPy is 1.7x *slower* on the suite, as in section 2 and for the same reason:
536 short tests never reach JIT warm-up. It says nothing about self-play.)

Incidental but worth recording: all four CPython digests came out **exactly
equal to the constants hard-coded in `tools/gate.sh`**, so despite the
`engine/effects.py` and `engine/events.py` changes on master since those were
derived, the gate's baseline is current at `9794bd7` and needs no re-derivation.


### 11.9 What of the tree is PyPy-eligible at all

The neural code imports torch, so it was worth checking what that rules out.
On this box it rules out nothing, because **neither interpreter has torch**:

```
python3 -c "import torch"  ->  ModuleNotFoundError
pypy3   -c "import torch"  ->  ModuleNotFoundError
python3 -c "import numpy"  ->  ModuleNotFoundError
pypy3   -c "import numpy"  ->  ModuleNotFoundError
```

Torch training lives on the desktop compute node, not here. `engine/` is
torch-free apart from `engine/bots/neural_net.py`, which defers the import
behind `HAVE_TORCH`; `experiments/arena.py` keeps `load_spec` torch-free for
`neural:` and `nplan:` on purpose (`tests/test_neural_plan.py::
test_load_spec_is_torch_free` pins it). Verified directly: `pypy3` imports
`experiments.arena`, `experiments.hillclimb_league` and
`engine.bots.neural_encode` fine, parses `plan:`, `quiesce:` and `nplan:`
specs, and fails on `make_bot("neural:...")` with the *same* error CPython
gives on this machine. So the entire Mac-side league stack — engine, arena,
hillclimb, gate, perf_check — is pure stdlib and PyPy-eligible; only actually
building a neural bot is not, and that cannot run here on CPython either.

### 11.10 VERDICT: **DO NOT SWITCH** the league. Optionally switch the 2p arm.

The arithmetic, over the five live workers (`run_league.sh 2 …` x1,
`run_league.sh 3 …` x2, `run_league.sh 4 …` x2), using 11.6:

```
switch everything:  (1 x 1.16 + 2 x 0.82 + 2 x 0.86) / 5  =  0.90   -> 10% LOSS
switch the 2p arm:   1 x 1.16 on 1 of 5 workers            =  +3.2% aggregate
switch 3p/4p only:  (2 x 0.82 + 2 x 0.86) / 4              =  0.84   -> 16% LOSS
```

So:

* **The 3p and 4p arms stay on CPython 3.14.6.** They are 4 of the 5 workers
  and PyPy costs them 14–18%. This is not close and no amount of warm-up
  changes it (the warm-up is 25–40 CPU-seconds per cell and both interpreters
  play 8–23 warm-up games before the window opens).
* **The 2p arm *could* move to PyPy for a real 1.16x [1.08, 1.23].** It is one
  worker, so the aggregate gain is ~3%, against the cost of running one live
  training arm on a second interpreter. Not recommended on those grounds alone
  — but it is now a real option rather than a closed question, and if the 2p
  arm ever gets more workers, or if `plan:width=N` spreads to 3p/4p, the
  arithmetic changes and this should be re-run.
* **Section 3's "PyPy loses every cell" is retired.** It is no longer true:
  PyPy wins GreedyBot by 1.45–1.65x and PlanBot by 1.12–1.24x. What survives is
  its *conclusion for the workload the league runs*, which is unchanged for a
  completely different reason than in July.
* **The `math.fsum` determinism work (section 2) remains the thing worth
  keeping** and it is what made this re-test cheap: 135 games byte-identical
  across interpreters means the two can be handed the same seeds and compared
  as paired samples, which is the only reason a 2-game measure window is
  defensible at the top of the table.

Re-test again if: the league's `--candidate-bot` changes (PlanBot at 3p/4p
would flip at least part of this), or CPython's specialising interpreter
regresses. **One of those triggers has already fired**: section 10's `7ef6ac8`
gave `QuiescentBot` and `PlanBot` a nested undo stack, which retires 11.3's
"the journal is a no-op for these two bots" and makes the 3p/4p rows of 11.4
and 11.6 pre-conversion measurements. Re-taking them is one command per cell
(`--kinds quiescent:levels=1 --opponent weighted` with `TTA_JOURNAL` both
ways) and it should be done before anyone quotes 0.82x/0.86x as current.
