# Registers the desktop's three climb arms and their three watchdogs.
# Re-runnable; /f overwrites.  Run it from an elevated PowerShell:
#   powershell -NoProfile -ExecutionPolicy Bypass -File C:\Users\micro\tta-desk\experiments\deploy\install_desktop_climb.ps1
#
# Every task runs as SYSTEM.  The arms were previously registered "Interactive
# only", which means a logoff killed them and nothing brought them back -- on
# 2026-08-18 that cost 21 hours of climbing before anyone noticed.  SYSTEM has
# no desktop, which is fine here and only here: climb.exe is a console program
# that writes to a file and never draws anything.  Do NOT add /it, and do not
# wrap these in a vbs hide-launcher.
$root = "C:\Users\micro\tta-desk"
$watchdog = Join-Path $root "experiments\deploy\desktop_climb_watchdog.ps1"

foreach ($k in 2, 3, 4) {
    # The arm itself: /sc once at a time that never arrives, so it only ever
    # runs when something asks for it -- `schtasks /run` or the watchdog.
    schtasks /create /f /tn "TTAClimb_${k}p" /ru SYSTEM /sc once /st 23:59 `
        /tr "cmd /c start `"climb${k}p`" /low /wait `"$root\climb_${k}p.bat`""

    schtasks /create /f /tn "TTAWatch_${k}p" /ru SYSTEM /sc minute /mo 5 `
        /tr "powershell -NoProfile -ExecutionPolicy Bypass -File `"$watchdog`" -K $k"
}

# Yielding to games is NOT ours to detect.  D:\llm\game-guard.ps1 (task
# \gameguard, SYSTEM, polls every 10s) already does it for llama-swap, and does
# it structurally: every game on this box lives under D:\ and the only thing
# under D:\ that is not a game is D:\llm itself, so "a process under D:\ but not
# D:\llm\" IS a game -- no allowlist to go stale.  It kills climb.exe and writes
# the PAUSE flag above; the watchdogs bring the arms back once it clears.
#
# This replaced experiments/gpu_guard.py, which asked the opposite question --
# allowlist the benign, treat the rest as a game -- and so failed silently every
# time a new overlay appeared, with no log line telling "no game" apart from
# "allowlist stale".

# The neural-loop era's arm tasks, long disabled, pointed at the abandoned
# C:\Users\micro\tta-ai checkout.
foreach ($k in 2, 3, 4) { schtasks /delete /f /tn "TTA_Arm_${k}p_Interval" 2>$null }
