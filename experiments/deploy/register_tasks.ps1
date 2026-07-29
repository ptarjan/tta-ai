# Register every TtA Scheduled Task on the desktop, windowless and durable.
#
# Run elevated on the box:
#   powershell -NoProfile -ExecutionPolicy Bypass -File C:\Users\micro\tta-ai\experiments\deploy\register_tasks.ps1
#
# Two invariants this file exists to keep, both of which were violated in
# production and cost the box owner a gaming session:
#
#  1. NOTHING WE START MAY SHOW A WINDOW.  Every action goes through
#     tools/hidden_launch.vbs.  `powershell -WindowStyle Hidden` and
#     `Start-Process -WindowStyle Hidden` are both insufficient: the first
#     flashes, and the second only hides the process it starts -- a console app
#     launched underneath it (git's bash.exe re-execs usr\bin\bash.exe)
#     allocates its OWN visible console.  wscript.exe is a GUI-subsystem host,
#     so the console it creates is hidden from birth and is INHERITED all the
#     way down the tree.
#
#  2. EVERY TASK MUST BE ABLE TO RESTART ITSELF.  A <Repetition> with no
#     <Duration> is silently dropped by Task Scheduler; `schtasks /query`
#     reports "Repeat: Every: N/A".  Both the guard and the loop had exactly
#     that, so when the guard process died it stayed dead for two days and the
#     box was unguarded.  Every trigger below is a CalendarTrigger with an
#     explicit Duration.
#
# Duplicate starts are harmless by design: the guard holds a PID lock, the
# neural loop holds a PID+heartbeat lock, and each arm watchdog reaps the
# previous driver tree by stored PID before launching.

# native tools write to stderr for benign things (deleting a task that does
# not exist yet); with "Stop" PowerShell turns that into a terminating error.
# Exit codes are checked explicitly instead.
$ErrorActionPreference = "Continue"
$repo = "C:\Users\micro\tta-ai"
$vbs  = "$repo\tools\hidden_launch.vbs"

if (-not (Test-Path $vbs)) { throw "missing $vbs -- deploy tools/hidden_launch.vbs first" }

function Register-Xml($name, $xmlPath) {
  if (-not (Test-Path $xmlPath)) { throw "missing $xmlPath" }
  # `schtasks /xml` insists on UTF-16 and fails a UTF-8 file with the
  # unhelpful "(1,40)::ERROR: unable to switch the encoding" -- column 40 is
  # the encoding attribute, and it must AGREE with the bytes.  So the
  # declaration is rewritten alongside the encoding.  The repo keeps these as
  # readable UTF-8 rather than committing UTF-16 blobs nobody can diff.
  $u16 = Join-Path $env:TEMP ("tta_reg_" + $name + ".xml")
  $text = [System.IO.File]::ReadAllText($xmlPath) -replace 'encoding="UTF-8"', 'encoding="UTF-16"'
  [System.IO.File]::WriteAllText($u16, $text, [System.Text.Encoding]::Unicode)
  cmd.exe /c "schtasks.exe /delete /tn ""$name"" /f >nul 2>&1"
  & schtasks.exe /create /tn $name /xml $u16 /f
  $rc = $LASTEXITCODE
  Remove-Item $u16 -Force -EA 0
  if ($rc -ne 0) { throw "failed to register $name" }
  "registered $name"
}

Register-Xml "tta_gpu_guard"   "$repo\experiments\deploy\guard_task.xml"
Register-Xml "tta_neural_loop" "$repo\experiments\deploy\loop_task.xml"

# --- the three CPU league arms ---------------------------------------------
# Previously: `powershell -NoProfile -ExecutionPolicy Bypass -File ... -K n`
# run directly under an interactive token, every 15 minutes, three of them --
# a visible console flash every ~5 minutes on average even when the watchdog
# did nothing but exit.  That was the single biggest source of the popup storm.
foreach ($k in 2, 3, 4) {
  $inner = "powershell -NoProfile -ExecutionPolicy Bypass -File C:\Users\micro\desktop_watchdog_arm.ps1 -K $k"
  $xml = @"
<?xml version="1.0" encoding="UTF-8"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo><Description>TtA ${k}p league arm watchdog (windowless; reaps the previous driver tree before relaunching)</Description></RegistrationInfo>
  <Triggers>
    <LogonTrigger><Enabled>true</Enabled></LogonTrigger>
    <CalendarTrigger>
      <StartBoundary>2026-01-01T00:00:00</StartBoundary>
      <Enabled>true</Enabled>
      <ScheduleByDay><DaysInterval>1</DaysInterval></ScheduleByDay>
      <Repetition><Interval>PT15M</Interval><Duration>P1D</Duration><StopAtDurationEnd>false</StopAtDurationEnd></Repetition>
    </CalendarTrigger>
  </Triggers>
  <Principals>
    <Principal id="Author">
      <UserId>micro</UserId>
      <LogonType>InteractiveToken</LogonType>
      <RunLevel>LeastPrivilege</RunLevel>
    </Principal>
  </Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <AllowHardTerminate>true</AllowHardTerminate>
    <StartWhenAvailable>true</StartWhenAvailable>
    <RunOnlyIfIdle>false</RunOnlyIfIdle>
    <Hidden>true</Hidden>
    <ExecutionTimeLimit>PT1H</ExecutionTimeLimit>
    <Priority>7</Priority>
  </Settings>
  <Actions Context="Author">
    <Exec>
      <Command>wscript.exe</Command>
      <Arguments>//B //Nologo "$vbs" "$inner"</Arguments>
      <WorkingDirectory>$repo</WorkingDirectory>
    </Exec>
  </Actions>
</Task>
"@
  $tmp = Join-Path $env:TEMP "tta_arm_${k}.xml"
  # Task Scheduler requires UTF-16 for /xml
  [System.IO.File]::WriteAllText($tmp, $xml, [System.Text.Encoding]::Unicode)
  Register-Xml "TTA_Arm_${k}p_Interval" $tmp
  cmd.exe /c "schtasks.exe /delete /tn ""TTA_Arm_${k}p_Logon"" /f >nul 2>&1"
  Remove-Item $tmp -Force -EA 0
}

""
"--- registered tasks ---"
& schtasks.exe /query /fo table | Select-String -Pattern "tta_|TTA_"
