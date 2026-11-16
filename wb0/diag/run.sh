#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# In the WB0 guest: run.sh NAME DPR [neoweb-host args…] — a neoweb-host run under a 1 GiB cap with smaps.py
# snapshots every 30 s (runs/NAME/smaps-<t>.csv), for the RSS-growth diagnosis.
set -euo pipefail
name=$1 dpr=$2; shift 2
out=/wb0/runs/$name; rm -rf "$out"; mkdir -p "$out"
export GALLIUM_DRIVER=softpipe LIBGL_ALWAYS_SOFTWARE=1 DRAW_USE_LLVM=0 RUST_LOG=warn XDG_RUNTIME_DIR=/run
/wb0/measure.sh 1G "$out/mem.csv" -- /wb0/neoweb-host --dpr "$dpr" --out "$out" "$@" > "$out/host.log" 2>&1 &
t=0
while kill -0 $! 2>/dev/null; do
  p=$(pgrep -x neoweb-host || true)
  [ -n "$p" ] && /wb0/smaps.py "$p" 6 > "$out/smaps-$(printf %04d $t).csv" 2>/dev/null || true
  sleep 30; t=$((t + 30))
done
wait $! || echo "exit $?"
