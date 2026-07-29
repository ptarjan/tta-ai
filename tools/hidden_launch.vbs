' Launch a command with NO console window, ever.
'
' The box owner games on this machine and asked that training be invisible.
' It was not: three arm-watchdog Scheduled Tasks fire every 15 minutes and the
' neural loop's hourly trigger plus its ~12 generation workers each flashed a
' console, so the owner saw a window pop every few minutes.
'
' Why a .vbs rather than any of the obvious alternatives:
'   * `powershell -WindowStyle Hidden` still flashes -- the host window is
'     created and then hidden, and the flash is what the owner sees.
'   * `Start-Process -WindowStyle Hidden cmd.exe` hides CMD, but a console app
'     that CMD launches (git's bash.exe re-execs usr\bin\bash.exe) allocates a
'     NEW console of its own, which is visible.
'   * A Scheduled Task with no interactive token (S4U / "run whether user is
'     logged on or not") never shows a window, but it runs in session 0, and on
'     a consumer WDDM box CUDA from session 0 is not dependable.  The neural
'     loop needs the GPU, so it has to stay in the interactive session.
'
' wscript.exe is a GUI-subsystem host, so IT has no console.  WshShell.Run with
' window style 0 creates the child's console already hidden, and every
' descendant INHERITS that hidden console instead of allocating a visible one.
' That is what makes the whole process tree silent, not just the first process.
'
' Waits for the child (third argument True) on purpose: the Scheduled Task then
' stays in the Running state for as long as the work does, which is what makes
' MultipleInstancesPolicy=IgnoreNew actually prevent a second copy.  Returning
' immediately would mark the task Ready and let the next trigger start a
' duplicate -- one of the two process-leak paths this repo just fixed.
'
' Usage:  wscript.exe //B //Nologo hidden_launch.vbs "<command line>"
' Exit code is the child's, so the task's Last Result stays meaningful.

Option Explicit
Dim sh, cmd, i, rc

If WScript.Arguments.Count = 0 Then
    WScript.Quit 2
End If

cmd = WScript.Arguments(0)
For i = 1 To WScript.Arguments.Count - 1
    cmd = cmd & " " & WScript.Arguments(i)
Next

Set sh = CreateObject("WScript.Shell")
' 0 = SW_HIDE, True = wait for the child to finish
rc = sh.Run(cmd, 0, True)
WScript.Quit rc
