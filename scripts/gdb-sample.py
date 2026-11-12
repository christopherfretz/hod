#!/usr/bin/env python3
"""A sampling profiler for a machine without `perf` (WP-096).

    python3 -I scripts/gdb-sample.py <pid> [samples] [filter]

Attaches gdb to the running process `samples` times, takes a backtrace
each time (inlined frames included, as the release profile keeps line
tables), and prints, over the samples whose stack mentions `filter` (a
substring of a function name; all samples without one), the share of
samples each function is on top of the stack (self) and anywhere in it
(inclusive), and the hottest source lines on top. Each attach stops the
process for about a third of a second, so the samples are spread evenly
over its run, not timed precisely.
"""

import collections
import re
import subprocess
import sys

FRAME = re.compile(r"^#\d+\s+(?:0x[0-9a-f]+ in )?(.+?) \(.*?\)(?: at (\S+))?")


def sample(pid):
    out = subprocess.run(
        ["gdb", "-p", str(pid), "-batch", "-ex", "bt 40"],
        capture_output=True,
        text=True,
        timeout=60,
    ).stdout
    frames = []
    for line in out.splitlines():
        m = FRAME.match(line)
        if m:
            frames.append((m.group(1), m.group(2) or ""))
    return frames


def main():
    pid = int(sys.argv[1])
    count = int(sys.argv[2]) if len(sys.argv) > 2 else 100
    needle = sys.argv[3] if len(sys.argv) > 3 else None
    self_counts = collections.Counter()
    incl_counts = collections.Counter()
    lines = collections.Counter()
    # Self time charged to the first frame of the program's own code:
    # memcmp, malloc and the inlined helpers of the standard library are
    # charged to the function that called them.
    own = collections.Counter()
    kept = 0
    for _ in range(count):
        try:
            frames = sample(pid)
        except subprocess.TimeoutExpired:
            continue
        if not frames:
            break
        if needle and not any(needle in name for name, _ in frames):
            continue
        kept += 1
        self_counts[frames[0][0]] += 1
        lines[frames[0][1]] += 1
        for name, path in frames:
            if path.startswith("crates/"):
                own["%s  %s" % (name, path)] += 1
                break
        for name in {name for name, _ in frames}:
            incl_counts[name] += 1
    print("%d samples kept" % kept)
    print("\nself")
    for name, n in self_counts.most_common(30):
        print("%5.1f%%  %s" % (100.0 * n / kept, name))
    print("\ninclusive")
    for name, n in incl_counts.most_common(45):
        print("%5.1f%%  %s" % (100.0 * n / kept, name))
    print("\nself, charged to the first frame in crates/")
    for name, n in own.most_common(30):
        print("%5.1f%%  %s" % (100.0 * n / kept, name))
    print("\nlines on top")
    for name, n in lines.most_common(30):
        print("%5.1f%%  %s" % (100.0 * n / kept, name))


if __name__ == "__main__":
    main()
