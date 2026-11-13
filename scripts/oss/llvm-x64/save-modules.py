#!/usr/bin/env python3
"""Save the modules of the census (WP-178).

save-modules.py <census.json> <findings dir> [--per 3]

For each reason of census.py's --json output (every reason of field 11,
so a module counts under each name it was refused for), up to --per TUs
not saved yet: rerun the TU's recorded command (kiln's wrapper logged
argv and cwd) with `-S -emit-llvm` and `-o <dir>/<reason>/<object>.ll`,
which writes the module kiln hands its back end at -O0; then run hod on
it (HOD_CENSUS=1, strict, -filetype=obj, output discarded) and write its
standard error beside it (`.hod.txt`), which names the function of a
body's refusal, and the command line (`.cmd.txt`). A reason name is made
a directory name by replacing `/` with `_`.
"""

import argparse
import json
import os
import shlex
import subprocess
import sys


def strip_args(argv):
    """argv without -o <x>, -c, -MD/-MF/-MT dependency outputs."""
    out, skip = [], False
    for a in argv:
        if skip:
            skip = False
            continue
        if a in ("-o", "-MF", "-MT", "-MQ"):
            skip = True
            continue
        if a in ("-c", "-MD", "-MMD") or (a.startswith("-o") and len(a) > 2):
            continue
        out.append(a)
    return out


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("census")
    ap.add_argument("dir")
    ap.add_argument("--per", type=int, default=3)
    ap.add_argument("--hod", required=True)
    a = ap.parse_args()
    data = json.load(open(a.census))["census"]
    saved = 0
    for reason, ents in sorted(data.items()):
        d = os.path.join(a.dir, reason.replace("/", "_"))
        os.makedirs(d, exist_ok=True)
        have = [n for n in os.listdir(d) if n.endswith(".ll")]
        for e in ents:
            if len(have) >= a.per:
                break
            if not e.get("argv"):
                continue
            base = os.path.basename(e["output"]).removesuffix(".o") + ".ll"
            if base in have:
                continue
            ll = os.path.join(d, base)
            # argv[0] is kiln++/kiln (the wrapper logs the real command).
            cmd = strip_args(e["argv"]) + ["-S", "-emit-llvm", "-o", ll]
            env = dict(os.environ, KILN_BACKEND="llvm")
            r = subprocess.run(cmd, cwd=e["cwd"], env=env, capture_output=True, text=True)
            if r.returncode != 0 or not os.path.exists(ll):
                print(f"save-modules: {reason}: {e['source']}: rc {r.returncode}: {r.stderr[:300]}", file=sys.stderr)
                continue
            with open(ll.removesuffix(".ll") + ".cmd.txt", "w") as f:
                f.write(f"cd {shlex.quote(e['cwd'])}\n{shlex.join(e['argv'])}\n")
            h = subprocess.run([a.hod, "--strict", "-filetype=obj", ll, "-o", "/dev/null"],
                               env=dict(os.environ, HOD_CENSUS="1"), capture_output=True, text=True)
            with open(ll.removesuffix(".ll") + ".hod.txt", "w") as f:
                f.write(f"exit {h.returncode}\n{h.stderr}")
            have.append(base)
            saved += 1
    print(f"saved {saved} modules under {a.dir}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
