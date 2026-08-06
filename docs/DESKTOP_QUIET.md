# Training must be invisible on the owner's gaming box

The RTX 3090 desktop is somebody's gaming PC first and a training node second.
On 2026-07-29 the owner was mid-session and our training was popping a console
window every few minutes. This records what was actually wrong, what was
changed, and — because a previous report on this same subsystem was wrong —
exactly how each claim was verified.

> **The GPU guard is retired (2026-08-06); everything else here still
> stands.** `experiments/gpu_guard.py` freed VRAM by hard-killing our torch
> `python.exe` when a foreign process appeared on the card. The training
> pipeline has no GPU and no torch in it any more — it is CPU Rust — so the
> guard has nothing to detect and nothing to kill, and it has been deleted
> along with `guard_task.xml` and `run_guard.cmd`.
>
> **What survives, and it is most of it.** The `PAUSE` flag is still read by
> `experiments/neural_search_loop.sh` before every worker launch, so it is now
> an *operator* control with no automatic writer: `touch PAUSE` parks
> training, deleting the file resumes it. Automatic politeness is what it
> always actually was — the loop task's `<Priority>7</Priority>`
> (below-normal, **inherited by every child process**) plus the loop's own
> `--threads` budget, which leaves cores for the hill-climb league. Neither of
> those was ever the guard's doing.
>
> **The windowless machinery in §1.1 and §2 is unchanged and is still
> load-bearing**: `tools/hidden_launch.vbs`, `tools/wincheck.ps1`, the
> explicit `<Duration>` on every trigger, and the reap-by-PID rule. Sections
> 1.2 and 1.3 are the reasons those exist and are still worth reading; §3.1's
> guard-detection test is now a historical record of a subsystem that no
> longer runs.
>
> **One manual step on the desktop.** `register_tasks.ps1` now issues
> `schtasks /delete /tn tta_gpu_guard /f` instead of registering it, so the
> next redeploy cleans the box. Until that redeploy happens, the already-
> registered task will fire every five minutes against a script that no longer
> exists. Either redeploy or run that one command by hand.

## 1. What was broken

### 1.1 Every launch flashed a console (the popup storm)

Three `TTA_Arm_*_Interval` Scheduled Tasks fired **every 15 minutes** running
`powershell -NoProfile -ExecutionPolicy Bypass -File ...` under an interactive
token. Three tasks on a 15-minute cycle is a visible flash every ~5 minutes on
average, *even though the watchdog usually did nothing but exit*. The neural
loop's trigger and its ~12 generation workers added more.

Three fixes that do **not** work, and why:

| attempt | why it fails |
|---|---|
| `powershell -WindowStyle Hidden` | the host window is created, then hidden. The flash IS what the owner sees. |
| `Start-Process -WindowStyle Hidden cmd.exe` | hides CMD only. Git's `bash.exe` re-execs `usr\bin\bash.exe`, a console app, which **allocates its own visible console**. The arm watchdog was already doing this and still flashed. |
| Scheduled Task with no interactive token (S4U) | genuinely windowless, but runs in session 0, and CUDA from session 0 on a consumer WDDM box is not dependable. The neural loop needs the GPU. |

What works: `tools/hidden_launch.vbs`. `wscript.exe` is a GUI-subsystem host so
it has no console of its own, and `WshShell.Run(cmd, 0, True)` creates the
child's console **already hidden**. Every descendant then *inherits* that
hidden console instead of allocating a visible one — which is why this fixes
the whole tree, not just the first process. It stays in the interactive
session, so the GPU still works.

### 1.2 The guard was dead, and could not restart

`tta_gpu_guard` had one trigger: `LogonTrigger` with a `<Repetition>` that had
**no `<Duration>`**. Task Scheduler silently drops such a repetition —
`schtasks /query` reported `Repeat: Every: N/A`. So the task ran once at logon
and never again. When the guard process was killed on 2026-07-29 it stayed
dead: `Last Result -1`, `Status Ready`, last run **two days earlier**, stale
PID in `gpu_guard.lock`, zero `python.exe` on the box.

A guard that cannot restart is not a guard. **This, not the detection logic,
is why the owner's session went unnoticed.** The detector itself is fine and
has fired correctly for two days (see 3.1). `tta_neural_loop` had the identical
broken repetition.

### 1.3 We were leaking processes

The guard's own log shows the kill count climbing monotonically across
sessions: **16, 25, 29, 39, 48, 53, 60, 66**, and 46 orphan `bash.exe` were
still alive on an idle, paused machine. A `neural_loop.sh` driver started on
Jul 27 was still running two days later.

Mechanism, in `desktop_watchdog_arm.ps1`: aliveness is judged by **log mtime**,
not by process enumeration (enumeration is unreliable from a Scheduled Task
context here — see that script's header). The gaming guard kills `python.exe`
only, so an arm's **bash driver survives a game**. Ten minutes later the log is
stale, the watchdog concludes the arm is dead and starts a *second* driver.
Now two drivers are alive and both spawn python workers. Every game session
leaked one driver trio per arm, forever.

The neural loop had the same class of bug from the other direction: its driver
deliberately survives a guard kill (so it can resume without waiting for a
trigger), but a driver that outlives its task registration is invisible to
`MultipleInstancesPolicy=IgnoreNew`, so the next trigger starts a duplicate.

## 2. What changed

| file | change |
|---|---|
| `tools/hidden_launch.vbs` | new — the windowless launcher every task now goes through |
| `tools/wincheck.ps1` | new — the verification tool, with a blindness sentinel (see 3.2) |
| `experiments/deploy/register_tasks.ps1` | new — registers all five tasks reproducibly instead of by hand-typed `schtasks` |
| `experiments/deploy/guard_task.xml` | `CalendarTrigger` repeating every 5 min with an explicit `Duration`; action via the VBS; `Hidden`; priority 5 (NORMAL — the guard must be scheduled promptly even under full training load) |
| `experiments/deploy/loop_task.xml` | same, 15 min, priority 7 (BELOW_NORMAL, inherited by every child) |
| `experiments/deploy/desktop_watchdog_arm.ps1` | now version-controlled; **reaps the previous driver tree by stored PID** (`taskkill /F /T`) before relaunching; launches via the VBS |
| `experiments/gpu_guard.py` | `PAUSE_HOLD` operator hold; **re-syncs `paused` against the file on every poll** |
| `experiments/neural_search_loop.sh` | PID + heartbeat lock: a live driver wins and the newcomer exits; a stale-heartbeat driver is reaped |

Two notes on the guard changes:

* **`PAUSE_HOLD`** lets an operator pin training off without becoming a second
  writer of `PAUSE`. It was added because an operator wrote `PAUSE` by hand
  during the gaming session and the guard would have deleted it 30 seconds
  later. The guard still *writes* `PAUSE` while a hold is in place; it just
  never *clears* it. Delete the hold to hand control back.
* **The resync is load-bearing.** The old guard trusted its in-memory `paused`
  flag. If `PAUSE` vanished underneath it (operator, crash, disk), the guard
  would believe it was still paused forever and never re-arm — a silent
  failure with no log line. It now compares against disk every poll. This is
  not hypothetical: it is what let the detection test in 3.1 recover in one
  second.

## 3. How each claim was verified

### 3.1 The guard fires on a real game — VERIFIED

Not inferred. With the owner's actual game on screen, `PAUSE` was deleted and
the guard re-created it:

```
2026-07-29 08:19:30 resync: PAUSE absent on disk but guard thought paused
2026-07-29 08:19:31 PAUSE ON  game detected [Sintopia-Win64-Shipping.exe] -> wrote PAUSE, killed 0 training python
```

and the detector's raw input confirms the game is visible to it:

```
143928, D:\SteamLibrary\steamapps\common\Sintopia\Sintopia\Binaries\Win64\Sintopia-Win64-Shipping.exe
```

which matches no `BENIGN` pattern. Note this settles an open question: on this
consumer WDDM box `nvidia-smi --query-compute-apps` **does** list graphics
processes, not just CUDA ones, which is what makes the allowlist approach sound.

### 3.2 Nothing of ours shows a window — VERIFIED, and the first check was a lie

The obvious check, run over SSH:

```powershell
Get-Process | Where-Object { $_.MainWindowTitle -ne '' }
```

returned **PASS**. It was blind. OpenSSH gets its own Windows session and window
enumeration is session-local, so it reported *zero windowed processes on the
entire desktop* while the owner was looking at a game. A check that cannot see
anything passes everything.

`tools/wincheck.ps1` therefore runs **in session 1** via a Scheduled Task and
prints a **sentinel**: the total count of windowed processes it can see. If
that is zero the check declares itself blind and its verdict must be ignored.

Result, with the full launch chain live (`wscript` → `cmd` → `bash` → `bash` →
`python`) and the game on screen:

```
session check at 2026-07-29T08:20:30  sessionId=1
TOTAL windowed processes visible: 10
SENTINEL OK: the desktop is visible to this check
--- all windowed ---
  ... Sintopia-Win64-Shipping   Sintopia ...
--- ours with a window ---
  PASS: none of python,pythonw,bash,cmd,wscript,powershell,conhost,git,sh has a visible window
--- our process counts ---
  python 2   bash 11   cmd 8   wscript 4   powershell 3
```

### 3.3 The loop yields to a game — VERIFIED

The loop task was started while `PAUSE` was set. The driver launched, hit
`wait_if_paused`, and parked without running any compute:

```
[Wed, Jul 29, 2026  8:20:11 AM] PAUSED for gaming; holding
```

### 3.4 The single-driver lock rejects duplicates — VERIFIED

Second start, with a live driver already running:

```
[Wed, Jul 29, 2026  8:21:11 AM] driver 1222 alive (heartbeat 30s) -- exiting, not starting a second
```

### 3.5 Still unverified, stated plainly

* **The arm-watchdog reap** (`taskkill /F /T` on the stored PID) is deployed but
  has not yet been observed doing a reap, because the arms do not relaunch
  while `PAUSE` is set. Confirm on the next resume that
  `C:\Users\micro\tta_watchdog.log` shows a `reaped previous driver tree` line
  and that `bash.exe` does not exceed ~3 per arm.
* **The 12-worker generation path under real load** has not been window-checked,
  because running it would have meant running compute during the owner's game.
  It uses the identical inherited-console chain verified in 3.2, and the
  `python` process in that check was launched through exactly that chain — but
  re-run `tools/wincheck.ps1` on the next full iteration to close it.

## 4. Standing rules for this box

1. **Never launch anything on the desktop except through
   `tools/hidden_launch.vbs`.** Not `Start-Process`, not a bare `.cmd` in a
   task action.
2. **Every trigger gets an explicit `<Duration>`.** A `<Repetition>` without one
   is silently discarded and your "self-healing" task runs exactly once.
3. **The guard is the only writer of `PAUSE`.** To hold training off, create
   `PAUSE_HOLD`; to release, delete it.
4. **Anything that restarts a worker must first reap the old one by PID.**
   Log-mtime aliveness plus a survivor bash equals a permanent leak.
5. **Verify from session 1, and make the check prove it can see.** A verifier
   that cannot observe the thing it checks reports success.

## 5. Repeatable redeploy

```
scp tools/hidden_launch.vbs tools/wincheck.ps1 ... micro@desktop:C:/Users/micro/tta-ai/tools/
powershell -NoProfile -ExecutionPolicy Bypass -File C:\Users\micro\tta-ai\experiments\deploy\register_tasks.ps1
schtasks /run /tn tta_wincheck  &&  type C:\Users\micro\tta_wincheck.txt
```

`register_tasks.ps1` converts the UTF-8 XMLs to UTF-16 on the way in (and
rewrites the encoding declaration to match) because `schtasks /xml` rejects
UTF-8 with `(1,40)::ERROR: unable to switch the encoding`. XML comments in
those files must also avoid `--`, which is illegal inside an XML comment and
produces `incorrect comment syntax`.
