#!/usr/bin/env python3
"""Interleaved phase measurements of one or more `hod` binaries (WP-096).

    python3 -I scripts/phase-profile.py --hod base=path/to/hod --hod new=... \
        --input path.ll [--input ...] [--runs 9] [--stdin pipe|file] \
        [--out pipe|file] [--work DIR] [-- extra hod args]

Each run starts the binary without a shell, with HOD_THREADS=1 and
HOD_TIME_PHASES, feeding the module on standard input (`pipe`: written
from this process's memory, as kiln does; `file`: the file itself) and
taking the object from standard output (`pipe`, drained) or into a file
under --work. The binaries alternate run by run, so load drift hits all of
them alike. Per binary and input it prints, for every phase, the median and
the minimum in microseconds, then the deterministic counters of the
process: read and write system calls and bytes (/proc/<pid>/io, read while
the process is a zombie), minor and major page faults and peak resident
memory (wait4's rusage).
"""

import argparse
import os
import statistics
import subprocess
import sys
import threading


def run_once(binary, data, path, args, work, stdin_mode, out_mode):
    phases_file = os.path.join(work, "phases.%d.txt" % os.getpid())
    try:
        os.unlink(phases_file)
    except FileNotFoundError:
        pass
    env = {"HOD_THREADS": "1", "HOD_TIME_PHASES": phases_file, "PATH": "/usr/bin:/bin"}
    out_path = "-" if out_mode == "pipe" else os.path.join(work, "out.%d.o" % os.getpid())
    argv = [binary, "-filetype=obj", "-o", out_path] + args
    if stdin_mode == "file":
        stdin = open(path, "rb")
    else:
        stdin = subprocess.PIPE
    proc = subprocess.Popen(argv, stdin=stdin, stdout=subprocess.PIPE, env=env)
    out = []
    reader = threading.Thread(target=lambda: out.append(proc.stdout.read()))
    reader.start()
    if stdin_mode == "pipe":
        try:
            proc.stdin.write(data)
        except BrokenPipeError:
            pass
        proc.stdin.close()
    else:
        stdin.close()
    reader.join()
    os.waitid(os.P_PID, proc.pid, os.WEXITED | os.WNOWAIT)
    io = {}
    with open("/proc/%d/io" % proc.pid) as f:
        for line in f:
            key, value = line.split(":")
            io[key] = int(value)
    _, status, usage = os.wait4(proc.pid, 0)
    proc.returncode = status
    phases = {}
    with open(phases_file) as f:
        for line in f:
            name, value = line.split("\t")
            phases[name] = int(value)
    counters = {
        "syscr": io["syscr"],
        "syscw": io["syscw"],
        "rchar": io["rchar"],
        "wchar": io["wchar"],
        "minflt": usage.ru_minflt,
        "majflt": usage.ru_majflt,
        "maxrss_kib": usage.ru_maxrss,
    }
    return phases, counters, len(out[0]) if out and out[0] else 0


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--hod", action="append", required=True, help="label=path")
    parser.add_argument("--input", action="append", required=True)
    parser.add_argument("--runs", type=int, default=9)
    parser.add_argument("--stdin", choices=["pipe", "file"], default="pipe")
    parser.add_argument("--out", choices=["pipe", "file"], default="pipe")
    parser.add_argument("--work", default=".")
    parser.add_argument("rest", nargs="*")
    opts = parser.parse_args()
    hods = [h.split("=", 1) for h in opts.hod]
    load = os.getloadavg()
    print("load %.1f %.1f %.1f, %d runs, stdin %s, out %s" % (load + (opts.runs, opts.stdin, opts.out)))
    for path in opts.input:
        with open(path, "rb") as f:
            data = f.read()
        results = {label: [] for label, _ in hods}
        for _ in range(opts.runs):
            for label, binary in hods:
                results[label].append(run_once(binary, data, path, opts.rest, opts.work, opts.stdin, opts.out))
        print("\n%s (%d bytes)" % (path, len(data)))
        names = []
        for runs in results.values():
            for phases, _, _ in runs:
                for name in phases:
                    if name not in names:
                        names.append(name)
        print("%-10s" % "phase" + "".join("%22s" % ("%s med/min" % label) for label, _ in hods))
        for name in names:
            row = "%-10s" % name
            for label, _ in hods:
                values = [p[name] for p, _, _ in results[label] if name in p]
                row += "%22s" % ("%d / %d" % (statistics.median(values), min(values)) if values else "-")
            print(row)
        for key in ["syscr", "syscw", "rchar", "wchar", "minflt", "majflt", "maxrss_kib"]:
            row = "%-10s" % key
            for label, _ in hods:
                values = [c[key] for _, c, _ in results[label]]
                row += "%22s" % ("%d / %d" % (statistics.median(values), min(values)))
            print(row)
        sizes = {label: {s for _, _, s in results[label]} for label, _ in hods}
        print("object bytes on stdout:", sizes)
    load = os.getloadavg()
    print("\nload after %.1f %.1f %.1f" % load)


if __name__ == "__main__":
    sys.exit(main())
