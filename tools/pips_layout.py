#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""PIPSPROB.EXE's serial output -> the Markdown tables of docs/browser/wb2-std.md: the checks, then the layout values.
Usage: tools/pips_layout.py serial.log > tables.md"""
import re
import sys

checks, layout = [], []
for line in open(sys.argv[1], errors="replace"):
    line = line.replace("\r", "")
    if m := re.search(r"pips (\w+) \.\.\. (ok|FAILED|NOTSUP): (.*)", line):
        checks.append(m.groups())
    elif m := re.search(r"layout (\S+(?: \S+)?) = (\S+)", line):
        layout.append(m.groups())
print("| Check | Result | Detail |\n|---|---|---|")
for name, verdict, detail in checks:
    print(f"| `{name}` | {verdict} | {detail.strip()} |")
print("\n| Name | Value |\n|---|---:|")
for name, value in layout:
    print(f"| `{name}` | {value} |")
