# Self-healing relaunch for ONE desktop TTA replicate arm, selected by -K.
# Split into one task per arm (rather than one script looping over all
# three) because looping Start-Process calls for 2p/3p/4p from a single
# parent PowerShell process was observed to reliably hang the 1st and 3rd
# launch while the 2nd succeeded, regardless of which player-count was in
# which position -- looks like contention in process creation from a single
# parent hitting something (COM/shell init?) that's slow once, fast right
# after, contended on a third close call. Three independent Scheduled Tasks
# each doing exactly one Start-Process call sidesteps it entirely.
#
# See desktop_watchdog.ps1's header (superseded by this) for the rest of
# the design rationale: log-mtime aliveness (not process enumeration --
# Get-CimInstance Win32_Process from a Scheduled Task context returned zero
# matches for processes confirmed alive seconds earlier by a direct check),
# PAUSE-file awareness, deadline-based remaining-budget relaunch, Idle
# priority set explicitly after launch (owner's games must always win the
# CPU over this).

param([Parameter(Mandatory=$true)][int]$K)

$repoRoot = "C:\Users\micro\tta-ai"
$pauseFlag = Join-Path $repoRoot "PAUSE"
if (Test-Path $pauseFlag) { exit }

$deadlineFile = "C:\Users\micro\tta_arms_deadline.txt"
$logDir = Join-Path $repoRoot "experiments\logs"
$watchdogLog = "C:\Users\micro\tta_watchdog.log"
if (-not (Test-Path $deadlineFile)) { exit }
$deadline = [double](Get-Content $deadlineFile -Raw)
$now = [double]([DateTimeOffset]::UtcNow.ToUnixTimeSeconds())
if ($now -ge $deadline) { exit }
$remainHours = [Math]::Max(1, [Math]::Ceiling(($deadline - $now) / 3600))

$common = "--weight-guard clamp --past-k 2 --hall-dir experiments/hall_of_fame --candidate-bot quiescent:levels=1 --human-bots all --objective blend --objective-alpha 0.15 --pool-weights book=0.6,variant=0.6,human=0.6,mirror=1.0,past=1.2,hall=1.6,floor=0"

$armTable = @{
  2 = [pscustomobject]@{ W=4; B=24; Seed=1001; State="C:/Users/micro/rep_2p" };
  3 = [pscustomobject]@{ W=6; B=24; Seed=1002; State="C:/Users/micro/rep_3p" };
  4 = [pscustomobject]@{ W=6; B=48; Seed=1003; State="C:/Users/micro/rep_4p" };
}
$arm = $armTable[$K]
if (-not $arm) { exit }

$staleMinutes = 10
$logFile = Join-Path $logDir "desktop_${K}p.log"
$isAlive = $false
if (Test-Path $logFile) {
  $age = (Get-Date) - (Get-Item $logFile).LastWriteTime
  if ($age.TotalMinutes -lt $staleMinutes) { $isAlive = $true }
}
if ($isAlive) { exit }

# --- reap the previous driver before starting a new one --------------------
# THE PROCESS LEAK.  Aliveness above is judged by LOG MTIME, not by process
# enumeration (see the header for why enumeration is unreliable here).  When
# the gaming guard kills training it kills python.exe only -- the arm's bash
# driver is bash, so it SURVIVES.  Ten minutes later the log is stale, this
# watchdog concludes the arm is dead and starts a second one, and now two
# drivers are alive, both spawning python workers.  Every game session leaked
# one driver trio per arm, which is why the guard's kill count climbed
# monotonically across sessions: 16, 25, 29, 39, 48, 53, 60, 66, and 46 orphan
# bash.exe were still alive on an idle machine.
#
# Killing by STORED PID needs no enumeration, and /T takes the whole tree
# (cmd -> bash -> bash -> python workers), so the old driver cannot outlive its
# replacement.  Absent/dead PID is a silent no-op, which is the common case.
$pidFile = "C:\Users\micro\tta_arm_${K}.pid"
if (Test-Path $pidFile) {
  $oldPid = (Get-Content $pidFile -Raw).Trim()
  if ($oldPid -match '^\d+$') {
    & taskkill.exe /F /T /PID $oldPid 2>&1 | Out-Null
    "$(Get-Date -Format o) watchdog(${K}p): reaped previous driver tree pid=$oldPid" | Out-File -Append $watchdogLog
  }
}

$batPath = "C:\Users\micro\launch_${K}p_wd.bat"
$bashCmd = "cd /c/Users/micro/tta-ai && ./experiments/desktop_arm.sh $K $remainHours $($arm.W) $($arm.B) $common --state-dir $($arm.State) --seed $($arm.Seed)"
$batContent = "@echo off`r`ncd /d C:\Users\micro\tta-ai`r`n`"C:\Program Files\Git\bin\bash.exe`" -lc `"$bashCmd`"`r`n"
Set-Content -Path $batPath -Value $batContent -Encoding ASCII -NoNewline
# Launched through tools/hidden_launch.vbs, not Start-Process -WindowStyle
# Hidden: that flag hides CMD, but git's bash.exe re-execs usr\bin\bash.exe,
# which is a console app and ALLOCATES ITS OWN visible console.  wscript is a
# GUI host, so the console it creates is hidden from birth and every descendant
# inherits it.  The owner games on this box and asked that training be
# invisible; this is the difference between "mostly hidden" and silent.
$vbs = "C:\Users\micro\tta-ai\tools\hidden_launch.vbs"
$p = Start-Process -FilePath "wscript.exe" `
       -ArgumentList "//B", "//Nologo", "`"$vbs`"", "`"cmd.exe /c `"`"$batPath`"`"`"" `
       -WindowStyle Hidden -PassThru
Start-Sleep -Milliseconds 500
try { $p.PriorityClass = [System.Diagnostics.ProcessPriorityClass]::Idle } catch {}
Set-Content -Path $pidFile -Value $p.Id -Encoding ASCII
"$(Get-Date -Format o) watchdog(${K}p): relaunched (${remainHours}h left, workers=$($arm.W) block=$($arm.B), pid=$($p.Id))" | Out-File -Append $watchdogLog
