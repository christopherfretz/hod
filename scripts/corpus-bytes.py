#!/usr/bin/env python3
"""Byte identity of two `hod` binaries over the corpus' text modules (WP-096).

    python3 -I scripts/corpus-bytes.py --base path/to/hod --head path/to/hod \
        [--corpus DIR] [-j 4] [--limit N]

Every text module of every manifest entry (an entry of several modules is
split at its `;@ module:` lines) is compiled by both binaries with the
entry's llc arguments, from standard input to standard output. A module
agrees when both exit alike and give the same object bytes, or the same
standard error (a decline: its reason and message). Prints the counts and
every module that disagrees; exits 1 if any does.
"""

import argparse
import concurrent.futures
import hashlib
import os
import subprocess
import sys


def modules(path):
    with open(path, "rb") as f:
        text = f.read()
    parts = []
    current = []
    for line in text.splitlines(keepends=True):
        if line.startswith(b";@ module:") and current:
            parts.append(b"".join(current))
            current = []
        current.append(line)
    parts.append(b"".join(current))
    # The entry's header lines before the first module are comments; a
    # part without a module in it is that header alone.
    return [p for p in parts if b"target triple" in p or b"define " in p or b"declare " in p] or [text]


def run(binary, data, args):
    env = {"HOD_THREADS": "1", "PATH": "/usr/bin:/bin"}
    proc = subprocess.run([binary, "-o", "-"] + args, input=data, capture_output=True, env=env, timeout=600)
    return proc.returncode, hashlib.sha256(proc.stdout).hexdigest(), len(proc.stdout), proc.stderr


def check(job):
    entry, index, data, args, base, head = job
    a = run(base, data, args)
    b = run(head, data, args)
    return entry, index, a, b


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--base", required=True)
    parser.add_argument("--head", required=True)
    parser.add_argument("--corpus", default="/Volumes/AgentWork/hod-helper/.build/corpus")
    parser.add_argument("-j", type=int, default=4)
    parser.add_argument("--limit", type=int, default=0)
    opts = parser.parse_args()
    jobs = []
    with open(os.path.join(opts.corpus, "MANIFEST.tsv")) as f:
        header = f.readline().rstrip("\n").split("\t")
        for line in f:
            row = dict(zip(header, line.rstrip("\n").split("\t")))
            if "text" not in row["delivered"].split():
                continue
            args = row["llc_args"].split()
            for index, data in enumerate(modules(os.path.join(opts.corpus, row["entry"]))):
                jobs.append((row["entry"], index, data, args, opts.base, opts.head))
            if opts.limit and len(jobs) >= opts.limit:
                break
    objects = declines = differ = 0
    with concurrent.futures.ThreadPoolExecutor(max_workers=min(opts.j, 4)) as pool:
        for entry, index, a, b in pool.map(check, jobs):
            if a[0] != b[0] or a[1] != b[1] or (a[0] != 0 and a[3] != b[3]):
                differ += 1
                print("DIFFER %s#%d: base %s, head %s" % (entry, index, a[:3], b[:3]))
                if a[3] != b[3]:
                    print("  base: %s\n  head: %s" % (a[3][:300], b[3][:300]))
            elif a[0] == 0:
                objects += 1
            else:
                declines += 1
    print("%d modules: %d objects identical, %d declines identical, %d differ" % (len(jobs), objects, declines, differ))
    return 1 if differ else 0


if __name__ == "__main__":
    sys.exit(main())
