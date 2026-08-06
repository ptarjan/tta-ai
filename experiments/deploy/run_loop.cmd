@echo off
REM Durable entry point for the SEARCH-BACKED neural self-play loop.
REM Registered as the tta_neural_loop Scheduled Task: logon trigger + hourly
REM repetition + RestartOnFailure, MultipleInstancesPolicy=IgnoreNew, and
REM Priority 7 (below normal) which EVERY CHILD PROCESS INHERITS -- so a game
REM always outranks this. That inheritance, plus the GENW/GATEW thread budget
REM set below, is the whole of the box's CPU politeness now.
REM
REM The loop still reads the PAUSE flag and yields, but nothing writes it
REM automatically any more: experiments/gpu_guard.py freed VRAM by killing
REM torch, and the pipeline is CPU Rust with neither. PAUSE is an operator
REM control -- touch it to park training, delete it to resume.
REM See docs/NEURAL_SEARCH_LOOP.md sections 8 and 9.
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
