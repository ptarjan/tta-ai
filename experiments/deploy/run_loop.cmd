@echo off
cd /d C:\Users\micro\tta-ai
"C:\Program Files\Git\bin\bash.exe" -lc "cd ~/tta-ai && bash experiments/neural_loop.sh 120 >> loop/master.out 2>&1"
