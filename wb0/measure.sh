#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Run in the WB0 guest: measure.sh <cap: 512M|1G|max> <out.csv> -- <command...>
# Runs the command in its own cgroup v2 group (memory.max=cap, no swap) and samples once a second:
# RSS/PSS (smaps_rollup), VmPeak/VmHWM, threads of the main process, and the cgroup's current/peak memory.
# Optional: VA=2097152 applies `ulimit -v` in KiB (the phone's 2 GiB private VA).
set -euo pipefail
cap=$1 out=$2; shift 3   # skip "--"
cg=/sys/fs/cgroup/wb0.$$
echo +memory > /sys/fs/cgroup/cgroup.subtree_control 2>/dev/null || true
mkdir "$cg"
echo "$cap" > "$cg/memory.max"
echo 0 > "$cg/memory.swap.max"
trap 'rmdir "$cg" 2>/dev/null || true' EXIT

( echo $BASHPID > "$cg/cgroup.procs"; [ -n "${VA:-}" ] && ulimit -v "$VA"; exec "$@" ) &
pid=$!
kib() { awk -v k="$1:" '$1==k {print $2}' "$2" 2>/dev/null || true; }
echo "t_s,rss_kib,pss_kib,vmpeak_kib,vmhwm_kib,threads,cg_current_b,cg_peak_b" > "$out"
t0=$(date +%s)
while kill -0 "$pid" 2>/dev/null; do
  s=/proc/$pid/status r=/proc/$pid/smaps_rollup
  line="$(( $(date +%s) - t0 )),$(kib Rss $r),$(kib Pss $r),$(kib VmPeak $s),$(kib VmHWM $s),$(kib Threads $s),$(cat "$cg/memory.current"),$(cat "$cg/memory.peak")"
  echo "$line" >> "$out"
  echo "wb0 $(basename "$out") $line" > /dev/tty1 2>/dev/null || true   # visible on the VNC screen
  sleep 1
done
rc=0; wait "$pid" || rc=$?
oom=$(awk '$1=="oom_kill" {print $2}' "$cg/memory.events")
echo "# exit=$rc oom_kill=$oom peak_b=$(cat "$cg/memory.peak")" >> "$out"
exit "$rc"
