@echo off
cd /d C:\Users\micro\tta-ai
"C:\Program Files\Git\bin\bash.exe" -lc "cd ~/tta-ai && mkdir -p loop && bash experiments/neural_loop.sh >> loop/master.out 2>&1"
