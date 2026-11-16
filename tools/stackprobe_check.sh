#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Builds and runs the stackprobe self-test on Linux (native or in the cross image's host gcc).
# Each test thread named d<KiB> must report used >= KiB*1024 and < KiB*1024 + 32 KiB.
set -euo pipefail
cd "$(dirname "$0")"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
gcc -O2 -Wall -Werror -shared -fPIC -o "$tmp/stackprobe.so" stackprobe.c -ldl -lpthread
gcc -O1 -Wall -Werror -o "$tmp/t" stackprobe_test.c -lpthread
STACKPROBE_OUT="$tmp/out.txt" LD_PRELOAD="$tmp/stackprobe.so" "$tmp/t"
cat "$tmp/out.txt"
awk '/name=d[0-9]+ /{
  match($0, /name=d[0-9]+/); k = substr($0, RSTART+6, RLENGTH-6) * 1024
  match($0, /used=[0-9]+/); u = substr($0, RSTART+5, RLENGTH-5)
  n++; if (u < k || u >= k + 32768) { print "FAIL depth " k " used " u; bad = 1 }
} END { if (n != 3) { print "FAIL: expected 3 reports, got " n; bad = 1 } if (bad) exit 1; print "stackprobe self-test OK" }' "$tmp/out.txt"
