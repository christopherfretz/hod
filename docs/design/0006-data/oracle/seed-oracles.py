#!/usr/bin/env python3
"""Design 0006 section 6.1: is `llc -O0` trusted for x86-64?

Builds every seed of tests/ir/ for x86_64-unknown-linux-gnu with llc 19
and 22 in three configurations (plain -O0 = FastISel; -O0 -fast-isel=0 =
SelectionDAG; -O2 as the witness), links each with the cross gcc and runs
it under qemu-x86_64, then compares stdout + exit status with the seed's
directives and across configurations. Also builds the same seed natively
(aarch64, the reference configuration) to time native against emulated.

usage: python3 -I seed-oracles.py <tests/ir dir> <work dir> > out.tsv
Sequential on purpose (shared host). Every child has a 30 s limit.
"""
import os
import subprocess
import sys
import time

SEEDS, WORK = sys.argv[1], sys.argv[2]
X64 = "x86_64-unknown-linux-gnu"
SYSROOT = "/usr/x86_64-linux-gnu"
CONFIGS = {
    "fast": ["-O0"],
    "dag": ["-O0", "-fast-isel=0"],
    "o2": ["-O2"],
}


def run(cmd, stdin=b"", timeout=30):
    t = time.monotonic()
    try:
        p = subprocess.run(cmd, input=stdin, capture_output=True, timeout=timeout)
        return p.returncode, p.stdout, p.stderr, time.monotonic() - t
    except subprocess.TimeoutExpired:
        return "timeout", b"", b"", time.monotonic() - t


def directives(path):
    d = {"stdout": [], "stdin": "", "exit": "0", "args": [], "link": [], "modules": []}
    text = open(path).read()
    for line in text.splitlines():
        if not line.startswith(";@ "):
            continue
        key, _, val = line[3:].partition(":")
        val = val.strip() if key != "stdout" else val[1:] if val.startswith(" ") else val
        if key == "stdout":
            d["stdout"].append(val)
        elif key == "stdin":
            d["stdin"] = val + "\n"
        elif key == "exit":
            d["exit"] = val
        elif key == "args":
            d["args"] = val.split()
        elif key == "link":
            d["link"] += val.split()
        elif key == "module":
            d["modules"].append(val)
    return d, text


def split_modules(text, names):
    """`;@ module: <name>` starts a module; text before the first belongs to none."""
    if not names:
        return [("main", text)]
    out, cur, buf = [], None, []
    for line in text.splitlines(keepends=True):
        if line.startswith(";@ module:"):
            if cur is not None:
                out.append((cur, "".join(buf)))
            cur, buf = line.split(":", 1)[1].strip(), []
        elif cur is not None:
            buf.append(line)
    out.append((cur, "".join(buf)))
    return out


def build(seed, d, text, llc, triple, extra, cc, tag):
    objs = []
    for name, mod in split_modules(text, d["modules"]):
        src = os.path.join(WORK, f"{seed}.{name}.ll")
        with open(src, "w") as f:
            f.write(mod)
        obj = os.path.join(WORK, f"{seed}.{name}.{tag}.o")
        pic = [] if "no-pie" in d["link"] else ["-relocation-model=pic"]
        rc, _, err, _ = run([llc, f"-mtriple={triple}", *extra, *pic, "-filetype=obj", src, "-o", obj])
        if rc != 0:
            return None, "llc: " + err.decode(errors="replace").strip().splitlines()[0][:120]
        objs.append(obj)
    exe = os.path.join(WORK, f"{seed}.{tag}")
    drv = cc[:-3] + "g++" if "c++" in d["link"] else cc
    link = [drv, *objs, "-o", exe]
    link += ["-no-pie"] if "no-pie" in d["link"] else ["-pie"]
    link += ["-lm"] if "m" in d["link"] else []
    link += ["-lpthread"]
    rc, _, err, _ = run(link)
    if rc != 0:
        return None, "link: " + err.decode(errors="replace").strip().splitlines()[-1][:120]
    return exe, ""


def outcome(rc, out):
    if rc == "timeout":
        return "timeout", out
    status = f"signal {-rc}" if isinstance(rc, int) and rc < 0 else str(rc)
    return status, out


def main():
    os.makedirs(WORK, exist_ok=True)
    print("seed\tllvm\tconfig\tmatches_expected\tstatus\tseconds\tnote")
    for name in sorted(os.listdir(SEEDS)):
        if not name.endswith(".ll"):
            continue
        seed = name[:-3]
        d, text = directives(os.path.join(SEEDS, name))
        want = ("\n".join(d["stdout"]) + "\n").encode() if d["stdout"] else b""
        for v in ("19", "22"):
            llc = f"/usr/lib/llvm-{v}/bin/llc"
            results = {}
            for cfg, extra in CONFIGS.items():
                exe, note = build(seed, d, text, llc, X64, extra, "x86_64-linux-gnu-gcc", f"x{v}{cfg}")
                if exe is None:
                    print(f"{seed}\t{v}\t{cfg}\t-\tbuild-error\t-\t{note}")
                    continue
                rc, out, _, secs = run(["qemu-x86_64", "-L", SYSROOT, exe, *d["args"]],
                                        d["stdin"].encode())
                st, out = outcome(rc, out)
                ok = (out == want or not d["stdout"]) and st == d["exit"]
                results[cfg] = (st, out)
                print(f"{seed}\t{v}\t{cfg}\t{ok}\t{st}\t{secs:.3f}\t")
            vals = set(results.values())
            if len(vals) > 1:
                print(f"{seed}\t{v}\tDISAGREE\t-\t-\t-\t{sorted(results)}")
            # native reference (aarch64, SelectionDAG), for the time ratio
            exe, note = build(seed, d, text, llc, "aarch64-unknown-linux-gnu",
                              ["-O0", "-global-isel=0", "-fast-isel=0"], "gcc", f"a{v}")
            if exe is not None:
                rc, out, _, secs = run([exe, *d["args"]], d["stdin"].encode())
                st, out = outcome(rc, out)
                ok = (out == want or not d["stdout"]) and st == d["exit"]
                print(f"{seed}\t{v}\tnative-a64\t{ok}\t{st}\t{secs:.3f}\t")
            sys.stdout.flush()


main()
