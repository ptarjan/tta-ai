# Self-healing relaunch for ONE desktop climb arm, selected by -K.
#
# One Scheduled Task per arm, never one script looping over all three:
# looping Start-Process calls for 2p/3p/4p from a single parent PowerShell
# process reliably hung the 1st and 3rd launch while the 2nd succeeded.
#
# Aliveness is a RUN FLAG, with log mtime only as a backstop.  Process
# enumeration is not an option: Get-CimInstance Win32_Process from a Scheduled
# Task context has returned zero matches for processes confirmed alive seconds
# earlier by a direct check, so a watchdog that enumerates would relaunch an arm
# that is running perfectly well.
#
# climb_Kp.bat creates run_Kp.flag before exec'ing climb.exe and deletes it
# afterwards, so the flag is gone the moment the arm exits for any reason the
# launcher survives -- a crash, or the guard's taskkill.  That is the common
# case and it heals within one watchdog tick.
#
# Log mtime cannot carry that job alone.  It answers "has this arm written
# recently", and a threshold long enough not to relaunch a live arm mid-gauntlet
# is necessarily long enough that a freshly-killed arm still looks alive.  It
# stays as the backstop for the one case the flag cannot see: the whole cmd tree
# dying at once (logoff, reboot, hard power loss) leaves a stale flag on disk.
#
# The climb is launched through `start /low` so climb.exe inherits Idle
# priority: this runs as SYSTEM and must always lose the CPU to whoever is
# actually sitting at the machine.  Priority cannot be set after the fact --
# the launcher cmd.exe exits before climb.exe is enumerable, and a child
# inherits its priority at creation.
#
# Two pause files, because they have two owners and only one of them is a
# person.  PAUSE belongs to D:\llm\game-guard.ps1 -- the same guard that stops
# llama-swap -- which writes it within ~10s of a game starting and deletes it on
# every poll once the game has been gone for 30s.  The guard does the stopping
# (it kills the arms outright, so whoever sat down gets the machine back at
# once); this script only declines to bring them back.
#
# That makes PAUSE useless as a hand-operated switch: touch it with no game
# running and the guard eats it within ten seconds, measured.  PAUSE_MANUAL is
# never written or deleted by anything automatic, so it stays until someone
# removes it.  Do not merge these into one file -- a single flag cannot be both
# "cleared as soon as the machine is free" and "stays until a human says so".
param([Parameter(Mandatory = $true)][int]$K)

$root = "C:\Users\micro\tta-desk"
foreach ($p in "PAUSE", "PAUSE_MANUAL") {
    if (Test-Path (Join-Path $root $p)) { exit }
}

$flag = Join-Path $root "run_${K}p.flag"
if (Test-Path $flag) {
    # 20 minutes, and it has to be generous: an arm is silent for a whole
    # gauntlet block, and relaunching a live arm would put two climbers on one
    # champion file and one log.  Erring long only costs a slower recovery from
    # the rare stale-flag case; erring short corrupts the run.
    $log = Join-Path $root "experiments\logs\rust_climb_${K}p.jsonl"
    if (Test-Path $log) {
        $idle = ((Get-Date) - (Get-Item $log).LastWriteTime).TotalMinutes
        if ($idle -lt 20) { exit }
    } else { exit }
}

$runner = Join-Path $root "experiments\deploy\run_climb_arm.cmd"
Start-Process -FilePath "cmd.exe" -WindowStyle Hidden `
    -ArgumentList "/c start `"climb${K}p`" /low /wait `"$runner`" $K"
Add-Content -Path "C:\Users\micro\tta_watchdog.log" `
    -Value "$(Get-Date -Format s) relaunched ${K}p"
