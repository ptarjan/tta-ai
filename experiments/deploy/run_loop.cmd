@echo off
REM Durable entry point for the SEARCH-BACKED neural self-play loop.
REM Registered as the tta_neural_loop Scheduled Task: logon trigger + hourly
REM repetition + RestartOnFailure, MultipleInstancesPolicy=IgnoreNew, and
REM Priority 7 (below normal) which every child python inherits -- so a game
REM always outranks this. The loop itself reads the guard's PAUSE flag and
REM yields; experiments/gpu_guard.py is the only writer of that flag.
REM See docs/NEURAL_SEARCH_LOOP.md section 8.
REM
REM THIS FILE IS THE ONE THE SCHEDULED TASK RUNS, and loop_task.xml points at
REM this path directly so there is no second copy to drift. It drifted once:
REM an untracked hand-edited C:\Users\micro\tta-ai\run_loop.cmd launched the
REM search loop while this committed copy still launched neural_loop.sh -- the
REM 41-hour null written up in docs/NEURAL_LOOP_NULL.md. Anyone who read the
REM repo to find out what the desktop was running got the wrong answer.
cd /d C:\Users\micro\tta-ai
set GENW=6
set GATEW=6
set WIDTH=8
set NODES=1200
set TEACHER_GAMES=480
"C:\Program Files\Git\bin\bash.exe" -lc "cd ~/tta-ai && mkdir -p loop2 && bash experiments/neural_search_loop.sh 500 180 160 >> loop2/master.out 2>&1"
