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

### RECOMMENDATION (do not implement yet — this is the finding, not the work)

**The copy is ~17x more work than the mutation, so structural sharing beats
any constant-factor copy win by an order of magnitude.** The leaf fast path
above bought 1.55x; the ceiling for "copy faster" is maybe another 1.3x.
Copy-on-write or an undo stack has a theoretical ceiling near **17x** on the
copy component, i.e. roughly 64% -> ~5% of GreedyBot runtime, or about a
**2.5x whole-bot speed-up**, and it would help PyPy more than CPython because
it removes the short-lived-object churn that PyPy's GC handles worst (see
section 3).

Two designs, in order of preference:

1. **Undo stack (journalling `apply`).** `GreedyBot` needs the trial state only
   long enough to call `evaluate`, and it discards it immediately — so no
   persistence is needed at all, just `apply(state, mv)` / `undo(state)`.
   Record `(container, key, old_value)` for every write plus
   append/pop records for lists; 6.4 slots per move means a journal of ~7
   entries versus 395 slot copies. This is the cheapest possible scheme and
   needs no change to the state representation, only to the mutation sites.
   Risk: `engine/actions.py` + `effects.py` + `events.py` mutate state in many
   places; every one of them must go through the journal or the undo is wrong.
   Mitigation: a paranoid mode that `copy_state`s anyway and asserts the undone
   state is identical — the existing 135-game fingerprint then proves it.
2. **Copy-on-write with a version stamp.** Clone only the ~5.4 nodes on the
   mutated path, share the rest. Needs every mutation site to go through a
   `mutable(obj)` accessor, which is a larger and more invasive change than the
   journal, and it makes aliasing bugs possible (two logical states sharing one
   dict). Only worth it if a future bot needs to hold many trial states alive
   at once (i.e. real multi-ply search), which the journal cannot do.

Prerequisite for either: the mutation sites must be enumerated. `STRICT` legality
asserts and the 135-game fingerprint are the safety net that makes this
tractable; do it as its own branch, not inside a perf pass.

