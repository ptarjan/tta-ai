# Self-healing relaunch for ONE desktop climb arm, selected by -K.
#
# One Scheduled Task per arm, never one script looping over all three:
# looping Start-Process calls for 2p/3p/4p from a single parent PowerShell
# process reliably hung the 1st and 3rd launch while the 2nd succeeded.
#
# Aliveness is the arm's LOG MTIME, not process enumeration.  Get-CimInstance
# Win32_Process from a Scheduled Task context has returned zero matches for
# processes confirmed alive seconds earlier by a direct check, so a watchdog
# that enumerates would relaunch an arm that is running perfectly well.
#
# The climb is launched through `start /low` so climb.exe inherits Idle
# priority: this runs as SYSTEM and must always lose the CPU to whoever is
# actually sitting at the machine.  Priority cannot be set after the fact --
# the launcher cmd.exe exits before climb.exe is enumerable, and a child
# inherits its priority at creation.
#
# PAUSE is written by D:\llm\game-guard.ps1 -- the same guard that stops
# llama-swap -- within ~10s of a game starting, and removed 30s after it exits.
# The guard does the stopping (it kills the arms outright, so the owner gets the
# machine back immediately); this script only declines to bring them back.
# Touch PAUSE by hand to pin the arms off, but expect the guard to clear it.
param([Parameter(Mandatory = $true)][int]$K)

$root = "C:\Users\micro\tta-desk"
if (Test-Path (Join-Path $root "PAUSE")) { exit }

# 20 minutes: the slowest arm writes a line every generation, and the slowest
# generation measured is under two minutes.  The task itself fires every 5, so
# an arm killed for a gaming session is back within five minutes of the game
# closing rather than sitting idle for a third of an hour.
$log = Join-Path $root "experiments\logs\rust_climb_${K}p.jsonl"
if (Test-Path $log) {
    $idle = ((Get-Date) - (Get-Item $log).LastWriteTime).TotalMinutes
    if ($idle -lt 20) { exit }
}

$bat = Join-Path $root "climb_${K}p.bat"
Start-Process -FilePath "cmd.exe" -WindowStyle Hidden `
    -ArgumentList "/c start `"climb${K}p`" /low /wait `"$bat`""
Add-Content -Path "C:\Users\micro\tta_watchdog.log" `
    -Value "$(Get-Date -Format s) relaunched ${K}p"
