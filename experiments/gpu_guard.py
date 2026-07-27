r"""Gaming guard for the shared RTX 3090 desktop.

The owner games on this box and the GPU CANNOT be shared with a game without
stutter, so when a game appears our GPU/CPU training must yield the machine
COMPLETELY. This guard is the single arbiter for the whole box: it writes/removes
the PAUSE flag that every training loop (this project's neural self-play loop AND
the CPU league-arm worker) reads, and it hard-kills our training python to free
VRAM cleanly the instant a game is detected.

Detection (and why it is what it is)
-------------------------------------
This is a consumer WDDM box, so `nvidia-smi` reports **per-process VRAM as N/A**
(the coordinator's ">500MB VRAM" signal is a TCC/datacenter feature and is
unavailable here -- verified on the box). What IS available:
  * `nvidia-smi --query-compute-apps=pid,process_name` lists every GPU process
    WITH ITS FULL PATH -- and on this box that includes graphics (C+G) apps, not
    just CUDA compute. A launched game's .exe appears here.
So we detect by PATH: a process is FOREIGN (a game) if its GPU-process path is
not our python and not one of the many benign desktop/launcher apps that always
hold a graphics context (dwm, chrome, discord, steam helper, powertoys, ...).
Games live in game-specific directories (World of Warcraft\Wow.exe, steamapps\
common\<game>\...), so they never match the benign patterns. The cost asymmetry
is deliberate: a false pause merely stops training for a bit (it relaunches),
whereas a missed game causes the stutter the owner explicitly forbids -- so
anything unrecognized on the GPU is treated as a game.

Debounce: a state change requires the condition to hold for CONFIRM polls (~30s)
so a transient GPU app does not thrash training.

This guard NEVER kills itself or the game, only our python. It is intended to be
run as a Windows Scheduled Task (durability across reboots and SSH sessions).
"""
from __future__ import annotations

import os
import subprocess
import sys
import time

HOME = r"C:\Users\micro\tta-ai"
PAUSE = os.path.join(HOME, "PAUSE")
LOGDIR = os.path.join(HOME, "experiments", "logs")
LOG = os.path.join(LOGDIR, "gpu_guard.log")
LOCK = os.path.join(HOME, "gpu_guard.lock")

POLL = 15          # seconds between checks
CONFIRM = 2        # consecutive polls required to change state (~30s)

# Benign GPU-process path substrings (case-insensitive). Our own training is
# python.exe (matched by "python"). Everything on the GPU that is NOT matched
# here is treated as a game. Games live in their own dirs and never match.
# NOTE: plain lowercase substrings matched against the full GPU-process path.
# Use directory/name fragments that a GAME's path never contains -- games live
# in their own dirs (World of Warcraft\Wow.exe, steamapps\common\<game>\...),
# NOT under these launcher/desktop paths. Keep it generous: a false pause is
# cheap (training relaunches), a missed game is not.
BENIGN = [
    "python",                        # OUR training (and any python)
    "\\windows\\", "dwm.exe", "explorer.exe",
    "logonui.exe", "lockapp.exe", "textinputhost", "shellexperiencehost",
    "searchhost", "startmenuexperiencehost", "applicationframehost",
    "systemsettings", "video.ui", "widgets", "sihost", "ctfmon",
    "nvidia",                        # nvidia app / overlay / share
    "powertoys",
    "razer", "chroma", "alienware", "alienfx",
    "dropbox",
    "discord",
    "\\google\\chrome", "chrome.exe",
    "msedge", "edgewebview",
    "steamwebhelper", "\\steam\\bin", "steamui", "\\steam\\steam.exe",
    "streamdeck", "elgato",
    "wowhead",
    "\\battle.net\\", "battle.net.exe", "\\agent.exe",  # launcher/updater dir
    "windowsterminal", "openconsole", "conhost",
    "common files",
    "obs64", "nvcontainer", "nvsphelper",
    "cursor.exe", "code.exe", "\\git\\",
]


def log(msg):
    os.makedirs(LOGDIR, exist_ok=True)
    line = f"{time.strftime('%Y-%m-%d %H:%M:%S')} {msg}"
    with open(LOG, "a") as f:
        f.write(line + "\n")
    print(line, flush=True)


def _run(cmd):
    try:
        return subprocess.run(cmd, capture_output=True, text=True,
                              timeout=30).stdout
    except Exception as e:
        log(f"WARN cmd failed {cmd!r}: {e!r}")
        return ""


def gpu_processes():
    """[(pid, fullpath)] of every process holding a GPU context."""
    out = _run(["nvidia-smi", "--query-compute-apps=pid,process_name",
                "--format=csv,noheader"])
    procs = []
    for line in out.splitlines():
        line = line.strip()
        if not line:
            continue
        parts = line.split(",", 1)
        if len(parts) != 2:
            continue
        pid = parts[0].strip()
        path = parts[1].strip()
        if pid.isdigit():
            procs.append((int(pid), path))
    return procs


def foreign_gpu_procs():
    """GPU processes that are neither ours nor a known-benign desktop app."""
    out = []
    for pid, path in gpu_processes():
        low = path.lower()
        # nvidia-smi returns these when it cannot read a process's path (an
        # elevated/system GPU process seen from a lower-integrity token). These
        # are persistent system processes present at idle too, NOT games, so
        # ignoring them avoids a constant false-pause. The guard also runs
        # elevated (task RunLevel HighestAvailable) so in practice paths ARE
        # readable and real games (which run non-elevated) are always resolved.
        if (not path or "insufficient permissions" in low
                or "[n/a]" in low or "not supported" in low):
            continue
        if any(b.lower() in low for b in BENIGN):
            continue
        out.append((pid, path))
    return out


def our_python_pids():
    """PIDs of our training python (all python.exe except this guard)."""
    out = _run(["tasklist", "/FI", "IMAGENAME eq python.exe", "/FO", "CSV",
                "/NH"])
    me = os.getpid()
    pids = []
    for line in out.splitlines():
        line = line.strip().strip('"')
        if not line or line.lower().startswith("info:"):
            continue
        cols = line.split('","')
        if len(cols) < 2:
            continue
        try:
            pid = int(cols[1])
        except ValueError:
            continue
        if pid != me:
            pids.append(pid)
    return pids


def kill_training():
    pids = our_python_pids()
    if not pids:
        return 0
    for pid in pids:
        _run(["taskkill", "/F", "/PID", str(pid)])
    return len(pids)


def main():
    # single instance
    if os.path.exists(LOCK):
        try:
            with open(LOCK) as f:
                old = int(f.read().strip() or "0")
            # if the pid in the lock is still alive, exit
            alive = str(old) in _run(["tasklist", "/FI", f"PID eq {old}",
                                      "/FO", "CSV", "/NH"])
            if alive and old != os.getpid():
                print(f"guard already running (pid {old}); exiting")
                return
        except Exception:
            pass
    with open(LOCK, "w") as f:
        f.write(str(os.getpid()))

    log(f"GUARD START pid={os.getpid()} poll={POLL}s confirm={CONFIRM}")
    paused = os.path.exists(PAUSE)
    foreign_streak = 0
    clear_streak = 0
    # test hook: touch <HOME>\GUARD_TEST_FOREIGN to simulate a game
    test_flag = os.path.join(HOME, "GUARD_TEST_FOREIGN")

    while True:
        foreign = foreign_gpu_procs()
        if os.path.exists(test_flag):
            foreign = foreign + [(-1, "TEST_FOREIGN_GAME.exe")]
        if foreign:
            foreign_streak += 1
            clear_streak = 0
        else:
            clear_streak += 1
            foreign_streak = 0

        if not paused and foreign_streak >= CONFIRM:
            names = ", ".join(os.path.basename(p) for _, p in foreign)
            open(PAUSE, "w").write(f"game: {names}\n")
            n = kill_training()
            log(f"PAUSE ON  game detected [{names}] -> wrote PAUSE, "
                f"killed {n} training python")
            paused = True
        elif paused and clear_streak >= CONFIRM:
            if os.path.exists(PAUSE):
                os.remove(PAUSE)
            log("PAUSE OFF  no foreign GPU process for "
                f"{CONFIRM*POLL}s -> removed PAUSE, training may resume")
            paused = False

        time.sleep(POLL)


if __name__ == "__main__":
    main()
