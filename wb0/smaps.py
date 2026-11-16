#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""In the WB0 guest: sum a process's memory by mapping kind, to see what grows.

    smaps.py PID [TOP]   → CSV on stdout: kind,count,rss_kib,pss_kib,private_dirty_kib,shared_kib

Kinds: [heap], [stack], [anon], memfd:<name>, /dev/shm, the file path for file mappings, [vvar]/[vdso].
"""

import re
import sys
from collections import defaultdict

HEADER = re.compile(r"^[0-9a-f]+-[0-9a-f]+ \S+ \S+ \S+ \S+\s*(.*)$")
FIELDS = {"Rss": 0, "Pss": 1, "Private_Dirty": 2, "Shared_Clean": 3, "Shared_Dirty": 3}


def kind(path: str) -> str:
    if not path:
        return "[anon]"
    if path.startswith("/memfd:"):
        return "memfd:" + path[7:].replace(" (deleted)", "")
    if path.startswith("/dev/shm/"):
        return "/dev/shm"
    return path.replace(" (deleted)", "")


def main() -> None:
    pid, top = sys.argv[1], int(sys.argv[2]) if len(sys.argv) > 2 else 20
    sums = defaultdict(
        lambda: [0, 0, 0, 0, 0]
    )  # count, rss, pss, private_dirty, shared
    current = None
    with open(f"/proc/{pid}/smaps") as f:
        for line in f:
            m = HEADER.match(line)
            if m:
                current = sums[kind(m.group(1).strip())]
                current[0] += 1
                continue
            name, _, value = line.partition(":")
            if current is not None and name in FIELDS:
                current[1 + FIELDS[name]] += int(value.split()[0])
    print("kind,count,rss_kib,pss_kib,private_dirty_kib,shared_kib")
    for k, v in sorted(sums.items(), key=lambda kv: -kv[1][1])[:top]:
        print(f"{k},{v[0]},{v[1]},{v[2]},{v[3]},{v[4]}")


if __name__ == "__main__":
    main()
