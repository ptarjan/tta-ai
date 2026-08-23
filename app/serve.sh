#!/bin/bash
# Serve the advisor on the LAN so the iPad can reach it. Deliberately NOT
# GitHub Pages: this repo carries CGE's rulebook PDFs and BGA's source under
# sources/, so a public site built from it would republish them.
set -euo pipefail
cd "$(dirname "$0")"
[ -f tta.wasm ] || ./build.sh
PORT="${1:-8777}"
echo "http://$(ipconfig getifaddr en0 2>/dev/null || hostname):$PORT/"
exec python3 -m http.server "$PORT" --bind 0.0.0.0
