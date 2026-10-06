@echo off
rem Launch ONE desktop climb arm and maintain its run flag.  Usage: run_climb_arm.cmd 2
rem
rem Both launch paths -- the TTAClimb_Kp scheduled task and the watchdog -- go
rem through here, so there is exactly one place that knows the flag exists.  The
rem arm's own climb_Kp.bat is left alone: it is a long hand-maintained command
rem line of gauntlet paths, and threading bookkeeping through it would mean
rem three copies of this logic that can drift apart.
rem
rem The flag is what desktop_climb_watchdog.ps1 reads to decide the arm is gone.
rem `call` blocks until climb.exe exits for any reason -- a crash, or the game
rem guard's taskkill -- and the delete on the next line is what the watchdog
rem sees.  A logoff or reboot kills this cmd.exe too and strands the flag; the
rem watchdog's log-mtime backstop covers that case and only that case.
setlocal
set "K=%~1"
set "ROOT=C:\Users\micro\tta-desk"
echo %DATE% %TIME% %K%p> "%ROOT%\run_%K%p.flag"
call "%ROOT%\climb_%K%p.bat"
del /f /q "%ROOT%\run_%K%p.flag" 2>nul
rem Both callers launch this through `start`, which runs a batch file under
rem cmd /K, so without this exit every launch leaves an idle cmd.exe plus a
rem conhost behind in SYSTEM's session-0 window station.  ~120 of them filled
rem its desktop heap and every new SYSTEM console process (llamaswap-boot)
rem died at start with 0xC0000142.  Only append below this line: cmd re-reads
rem a running batch file by byte offset.
exit
