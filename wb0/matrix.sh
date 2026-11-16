#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Run WB0 measurements in the guest, one per line of a matrix file, and collect results on the host.
#   wb0/matrix.sh wb0/matrix.txt [DATE]
# Matrix line:  id site gl dpr cap va probe soak      (# comments; "-" = off)
#   site  = wb0/sites/<site>.json      gl  = mesa|swgl      dpr = device pixel ratio (1 = desktop layout)
#   cap   = cgroup memory.max (512M|1G|max)                 va  = ulimit -v KiB (2097152 = phone's 2 GiB) or -
#   probe = stack | -   (stackprobe.so; never combined with memory numbers)    soak = seconds or -
# Results: wb0/results/<DATE>/<id>/ (steps.csv, mem.csv, PNGs, *.mem.json, logs) and wb0/results/<DATE>/index.csv.
# Env: WATCH=1 shows runs on the guest screen (VNC); BIN=out/neoweb-host; WB0_G_USER/WB0_G_PASS/WB0_G_DOC are passed
# over ssh stdin (never on a command line or in logs).
set -euo pipefail
cd "$(dirname "$0")/.."
matrix=$1 date=${2:-$(date +%F)}
res=wb0/results/$date
BIN=${BIN:-out/neoweb-host}
ANDROID_UA='Mozilla/5.0 (Android 10; Mobile; rv:153.0) Servo/0.8.0 Firefox/153.0'
ssh_opts=(-q -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o ServerAliveInterval=30 -i out/guest/guest_key)
g() { ssh "${ssh_opts[@]}" -p 2222 root@127.0.0.1 "$@"; }

mkdir -p "$res"
[ -f "$res/index.csv" ] || echo "id,site,gl,dpr,cap,va,probe,soak,exit,oom_kill,steps_ok,steps_total,peak_cg_mib,max_rss_mib,max_pss_mib,end_rss_mib,vmpeak_mib,max_threads,wall_s" > "$res/index.csv"

# Ship the harness.
g 'mkdir -p /wb0/sites /wb0/runs'
scp "${ssh_opts[@]}" -P 2222 "$BIN" wb0/measure.sh wb0/smaps.py root@127.0.0.1:/wb0/ >/dev/null
scp "${ssh_opts[@]}" -P 2222 wb0/sites/*.json root@127.0.0.1:/wb0/sites/ >/dev/null
[ -f out/stackprobe.so ] && scp "${ssh_opts[@]}" -P 2222 out/stackprobe.so root@127.0.0.1:/wb0/ >/dev/null
bin=/wb0/$(basename "$BIN")

grep -v '^\s*#' "$matrix" | while read -r id site gl dpr cap va probe soak; do
  [ -n "${id:-}" ] || continue
  echo "== $id: $site $gl dpr=$dpr cap=$cap va=$va probe=$probe soak=$soak"
  args=(--workflow "/wb0/sites/$site.json" --gl "$gl" --dpr "$dpr" --out "/wb0/runs/$id")
  [ "$dpr" = 1 ] || args+=(--pref "user_agent=$ANDROID_UA")
  [ "$soak" = - ] || args+=(--soak "$soak" --timeout $((soak + 900)))
  [ "${WATCH:-0}" = 1 ] && args+=(--fb /dev/fb0)
  env=(GALLIUM_DRIVER=softpipe LIBGL_ALWAYS_SOFTWARE=1 DRAW_USE_LLVM=0 RUST_LOG=warn XDG_RUNTIME_DIR=/run)
  [ "$gl" = llvmpipe ] && { env=(GALLIUM_DRIVER=llvmpipe LIBGL_ALWAYS_SOFTWARE=1 RUST_LOG=warn XDG_RUNTIME_DIR=/run); args[3]=mesa; }
  [ "$va" = - ] || env+=(VA="$va")
  [ "$probe" = stack ] && env+=(LD_PRELOAD=/wb0/stackprobe.so STACKPROBE_OUT="/wb0/runs/$id/stacks.txt")
  quoted=$(printf ' %q' "${args[@]}")
  t0=$(date +%s)
  rc=0
  printf '%s\n%s\n%s\n' "${WB0_G_USER:-}" "${WB0_G_PASS:-}" "${WB0_G_DOC:-}" | g "
    pkill -x $(basename "$bin") 2>/dev/null; sleep 1; sync; echo 3 > /proc/sys/vm/drop_caches
    rm -rf /wb0/runs/$id && mkdir -p /wb0/runs/$id && cd /wb0
    read -r u; read -r p; read -r d
    [ -n \"\$u\" ] && export WB0_G_USER=\"\$u\" WB0_G_PASS=\"\$p\"; [ -n \"\$d\" ] && export WB0_G_DOC=\"\$d\"
    env ${env[*]} ./measure.sh $cap /wb0/runs/$id/mem.csv -- $bin$quoted > /wb0/runs/$id/host.log 2>&1" || rc=$?
  wall=$(( $(date +%s) - t0 ))
  rm -rf "${res:?}/$id" && scp -r "${ssh_opts[@]}" -P 2222 "root@127.0.0.1:/wb0/runs/$id" "$res/" >/dev/null </dev/null
  # Summarise: memory columns from mem.csv (KiB), step results from steps.csv.
  python3 - "$res/$id" "$id,$site,$gl,$dpr,$cap,$va,$probe,$soak,$rc" "$wall" >> "$res/index.csv" <<'EOF'
import csv, sys, pathlib
d, head, wall = pathlib.Path(sys.argv[1]), sys.argv[2], sys.argv[3]
rows, oom, peak = [], "", ""
for line in (d / "mem.csv").read_text().splitlines()[1:]:
    if line.startswith("#"):
        kv = dict(p.split("=") for p in line[1:].split())
        oom, peak = kv.get("oom_kill", ""), kv.get("peak_b", "")
    elif line:
        rows.append(line.split(","))
col = lambda i: [int(r[i]) for r in rows if len(r) > i and r[i]]
mib = lambda v: f"{v / 1024:.0f}" if v else ""
steps = list(csv.DictReader((d / "steps.csv").open())) if (d / "steps.csv").exists() else []
ok = sum(s["ok"] == "1" for s in steps)
rss, pss, vmpeak, thr = col(1), col(2), col(3), col(5)
print(",".join([head, oom, str(ok), str(len(steps)), f"{int(peak) / 2**20:.0f}" if peak else "",
                mib(max(rss, default=0)), mib(max(pss, default=0)), mib(rss[-1] if rss else 0),
                mib(max(vmpeak, default=0)), str(max(thr, default="")), wall]))
EOF
  tail -1 "$res/index.csv"
done
