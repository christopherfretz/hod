#!/usr/bin/env python3
"""The summary of a `sweep` run (.github/workflows/sweep.yml, spec/harness.md
"CI"): one Markdown table of the libc++ suite's counts per runner and back
end, hod's share of the modules, the shards' wall times, and the tests
whose outcome differs between hod and the baseline (kiln with `llc`) on the
same runner.

    scripts/libcxx-sweep.py [--mode strict|hybrid] <results dir>

<results dir> holds the shards' artifacts merged: kiln's `cargo xtask
libcxx --results` files `libcxx-<arch>-<backend>-<K>.tsv` (backend `hod`
or `llc`), `wall-<arch>-<backend>-<K>.txt` ("<name> <seconds>") and, for
hod, `log-<arch>-hod-<K>.tar.gz` (scripts/libcxx-hod-kiln.sh's
HOD_LIBCXX_LOGDIR: `cov/*.tsv`, one coverage line per module, its first
field `hod`, `fallback` or `error`).

Exit status 1 when a test PASSes with `llc` and does not with hod on the
same runner (the sweep's one failure); other differences (XFAIL, XPASS,
UNSUPPORTED, a TIMEOUT with llc) are listed and do not fail. 2 on a usage
error or no results.
"""
import collections
import os
import re
import sys
import tarfile

OUTCOMES = ["PASS", "FAIL", "XFAIL", "XPASS", "UNSUPPORTED", "KILLED", "TIMEOUT", "ERROR"]
NAME = re.compile(r"^libcxx-(?P<arch>[a-z0-9_]+)-(?P<backend>hod|llc)-(?P<shard>\d+)\.tsv$")


def die(msg):
    print(f"libcxx-sweep: {msg}", file=sys.stderr)
    sys.exit(2)


def files(top):
    for root, _, names in os.walk(top):
        for name in sorted(names):
            yield root, name


def results(top):
    """(arch, backend) -> {test: (outcome, detail)}."""
    out = collections.defaultdict(dict)
    for root, name in files(top):
        m = NAME.match(name)
        if not m:
            continue
        with open(os.path.join(root, name), encoding="utf-8", errors="replace") as f:
            for line in f:
                if line.startswith("#") or not line.strip():
                    continue
                fields = line.rstrip("\n").split("\t")
                if len(fields) < 2:
                    continue
                # The phase (`compile`, `run`), then its message, which
                # names the phase again.
                detail = fields[2] if len(fields) > 2 else "-"
                if len(fields) > 3 and fields[3] not in ("", "-"):
                    detail = fields[3]
                out[(m["arch"], m["backend"])][fields[1]] = (fields[0], detail)
    return out


def walls(top):
    """(arch, backend) -> [seconds per shard]."""
    out = collections.defaultdict(list)
    for root, name in files(top):
        if name.startswith("wall-") and name.endswith(".txt"):
            with open(os.path.join(root, name), encoding="utf-8") as f:
                parts = f.read().split()
            m = re.match(r"^([a-z0-9_]+)-(hod|llc)-\d+$", parts[0]) if len(parts) == 2 else None
            if m:
                out[(m[1], m[2])].append(int(parts[1]))
    return out


def modules(top):
    """arch -> Counter of the first field of hod's coverage lines, and the
    refusals: arch -> [(reason, test key)]."""
    counts = collections.defaultdict(collections.Counter)
    refused = collections.defaultdict(list)
    mark = "/libcxx/test/"
    for root, name in files(top):
        m = re.match(r"^log-([a-z0-9_]+)-hod-\d+\.tar\.gz$", name)
        if not m:
            continue
        with tarfile.open(os.path.join(root, name)) as tar:
            tests = {}
            for member in tar.getmembers():
                if member.isfile() and member.name.endswith("/args.tsv"):
                    text = tar.extractfile(member).read().decode("utf-8", "replace")
                    for line in text.splitlines():
                        key, _, argv = line.partition("\t")
                        for a in argv.split(" "):
                            if mark in a and a.endswith(".cpp"):
                                tests[key] = a.split(mark, 1)[1]
            for member in tar.getmembers():
                if not (member.isfile() and "/cov/" in member.name):
                    continue
                text = tar.extractfile(member).read().decode("utf-8", "replace")
                for line in text.splitlines():
                    fields = line.split("\t")
                    counts[m[1]][fields[0]] += 1
                    if fields[0] != "hod":
                        key = os.path.basename(member.name)[: -len(".tsv")]
                        test = tests.get(key, key)
                        refused[m[1]].append((fields[1] if len(fields) > 1 else "?", test))
    return counts, refused


def cell(text):
    return text.replace("|", "\\|").replace("\n", " ")[:160]


def main(argv):
    mode = "strict"
    args = list(argv)
    if args[:1] == ["--mode"]:
        if len(args) < 2 or args[1] not in ("strict", "hybrid"):
            die("--mode is strict or hybrid")
        mode, args = args[1], args[2:]
    if len(args) != 1:
        die("usage: libcxx-sweep.py [--mode strict|hybrid] <results dir>")
    top = args[0]
    sets = results(top)
    if not sets:
        die(f"no libcxx-<arch>-<backend>-<K>.tsv under {top}")
    wall = walls(top)
    counts, refused = modules(top)
    keys = sorted(sets, key=lambda k: (k[0] != "x86_64", k[0], k[1]))

    print(f"## libc++ 22.1.8 suite: kiln + hod ({mode}) and kiln + llc, native runners\n")
    head = " | ".join(o.lower() for o in OUTCOMES)
    print(f"| runner | back end | total | {head} | shards (s) | hod's modules |")
    print("|---|---|---:|" + "---:|" * len(OUTCOMES) + "---|---|")
    for arch, backend in keys:
        tally = collections.Counter(o for o, _ in sets[(arch, backend)].values())
        row = " | ".join(str(tally.get(o, 0)) for o in OUTCOMES)
        shards = " + ".join(str(s) for s in sorted(wall.get((arch, backend), []))) or "-"
        mods = "-"
        if backend == "hod" and counts.get(arch):
            c = counts[arch]
            mods = f"{c['hod']} of {sum(c.values())}"
        total = len(sets[(arch, backend)])
        print(f"| {arch} | {backend} | {total} | {row} | {shards} | {mods} |")

    failures = 0
    for arch in sorted({a for a, _ in keys}, key=lambda a: (a != "x86_64", a)):
        hod, llc = sets.get((arch, "hod")), sets.get((arch, "llc"))
        print(f"\n### {arch}: tests whose outcome differs (hod against llc)\n")
        if hod is None or llc is None:
            print(f"Not compared: the {'hod' if hod is None else 'llc'} results are missing.")
            failures += 1
            continue
        rows = []
        for test in sorted(set(hod) | set(llc)):
            h, b = hod.get(test, ("MISSING", "-")), llc.get(test, ("MISSING", "-"))
            if h[0] != b[0]:
                gate = b[0] == "PASS"
                failures += gate
                rows.append((test, h, b, "hod's failure" if gate else "listed"))
        if not rows:
            print("None.")
        else:
            print("| test | hod | llc | | hod's detail |")
            print("|---|---|---|---|---|")
            for test, h, b, what in rows:
                print(f"| `{test}` | {h[0]} | {b[0]} | {what} | {cell(h[1])} |")
        for reason, key in sorted(refused.get(arch, [])):
            print(f"\nhod did not build a module of `{key}`: `{reason}`")
    print(
        f"\n{failures} test(s) pass with llc and not with hod."
        if failures
        else "\nNo test passes with llc and not with hod."
    )
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
