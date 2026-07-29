# Must run IN THE INTERACTIVE SESSION (session 1). Run from an SSH shell it is
# blind -- OpenSSH gets its own session and window enumeration is session-local,
# so it reports zero windowed processes even while the desktop is full of them.
# That blindness reads as a PASS, which is exactly the kind of false green that
# has already burned this project once. The sentinel line below proves the
# check actually saw the desktop.
$out = "C:\Users\micro\tta_wincheck.txt"
$mine = 'python','pythonw','bash','cmd','wscript','powershell','conhost','git','sh'
$all = @(Get-Process | Where-Object { $_.MainWindowTitle -ne '' })
$lines = @()
$lines += "session check at $(Get-Date -Format o)  sessionId=$((Get-Process -Id $PID).SessionId)"
$lines += "TOTAL windowed processes visible: $($all.Count)"
if ($all.Count -eq 0) {
  $lines += "SENTINEL FAIL: zero windows visible at all -- this check is blind, ignore its verdict"
} else {
  $lines += "SENTINEL OK: the desktop is visible to this check"
}
$lines += "--- all windowed ---"
$all | ForEach-Object { $lines += ("  {0,-22} {1}" -f $_.ProcessName, $_.MainWindowTitle) }
$ours = @($all | Where-Object { $mine -contains $_.ProcessName })
$lines += "--- ours with a window ---"
if ($ours.Count -gt 0) {
  $ours | ForEach-Object { $lines += ("  FAIL {0,-18} {1}" -f $_.ProcessName, $_.MainWindowTitle) }
} else {
  $lines += "  PASS: none of $($mine -join ',') has a visible window"
}
$lines += "--- our process counts ---"
foreach ($n in $mine) { $c = @(Get-Process $n -EA 0).Count; if ($c -gt 0) { $lines += ("  {0,-12} {1}" -f $n, $c) } }
$lines | Set-Content -Path $out -Encoding ASCII
