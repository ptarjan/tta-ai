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
