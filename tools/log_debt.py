#!/usr/bin/env python3
"""What a replay still takes from the log, as one number to drive to zero.

Reads `replay`'s summary on stdin and adds up the counts of every line that says the log
supplied something the engine should have made itself: state set back to the log's, primitives
answered from the log, places with no native walk, runs not modelled. When the sum is zero the
log is only compared with, never taken from, and the replay is a free run. The player's
commands, the dialogs' answers and the random numbers are inputs and are not counted.

usage: replay LOG ISO [...] | tools/log_debt.py [--top N]
"""
import re
import sys

TAKEN = re.compile(
    r"set to the log's|the log's taken|^primitives: from the log$|not modelled|"
    r"^places: the log's \(no native walk there\)$|from the log's list|\(the log's tree\)|"
    r"tree not found on the disc$"
)
INPUT = re.compile(r"player's command|Dialog")


def main() -> None:
    top = int(sys.argv[2]) if len(sys.argv) > 2 and sys.argv[1] == "--top" else 8
    rows = []
    for line in sys.stdin:
        m = re.match(r"\s+(.*?)\s+(\d+)$", line.rstrip("\n"))
        if not m:
            continue
        label, count = m.group(1), int(m.group(2))
        if count and TAKEN.search(label) and not INPUT.search(label):
            rows.append((count, label))
    rows.sort(reverse=True)
    print(f"log debt: {sum(c for c, _ in rows)} in {len(rows)} kinds")
    for count, label in rows[:top]:
        print(f"  {count:8d}  {label}")


if __name__ == "__main__":
    main()
