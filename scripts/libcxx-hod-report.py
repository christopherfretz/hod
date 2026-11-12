#!/usr/bin/env python3
"""Reads a libc++ suite run made through scripts/libcxx-hod-kiln.sh
(docs/status/libcxx-hod.md) and prints its tables in Markdown.

    scripts/libcxx-hod-report.py <logdir> <results.tsv> <baseline.tsv> [--top N]
    scripts/libcxx-hod-report.py <logdir> --hod-only

<logdir> is the wrapper's HOD_LIBCXX_LOGDIR (args.tsv, cov/, phases/);
the .tsv files are kiln's `cargo xtask libcxx --results` output. Prints:
the pass counts against the baseline, the tests whose outcome changed,
per-test coverage (all objects by hod, some by the fallback), the census
of first refusals (reason -> modules) and of every reason (with
HOD_CENSUS=1: reason -> modules it occurs in, and tests that would be
hod-only if that reason alone went away), and back-end time.
With --hod-only: the tests all of whose objects hod built, one per line
(the input of the strict run).
"""
import collections
import os
import sys

MARK = "/libcxx/test/"


def tests_of_keys(logdir):
    """key -> test path relative to libcxx/test (from the compile command)."""
    out = {}
    with open(os.path.join(logdir, "args.tsv"), encoding="utf-8", errors="replace") as f:
        for line in f:
            key, _, argv = line.rstrip("\n").partition("\t")
            for a in argv.split(" "):
                if MARK in a and a.endswith(".cpp"):
                    out[key] = a.split(MARK, 1)[1]
    return out


def coverage(logdir):
    """key -> list of coverage lines (fields)."""
    cov = {}
    d = os.path.join(logdir, "cov")
    for name in os.listdir(d):
        with open(os.path.join(d, name), encoding="utf-8", errors="replace") as f:
            cov[name[:-4]] = [l.rstrip("\n").split("\t") for l in f if l.strip()]
    return cov


def phases(logdir):
    tot = collections.Counter()
    d = os.path.join(logdir, "phases")
    if not os.path.isdir(d):
        return tot
    for name in os.listdir(d):
        with open(os.path.join(d, name), encoding="utf-8") as f:
            for l in f:
                p = l.split("\t")
                if len(p) == 2:
                    tot[p[0]] += int(p[1])
    return tot


def results(path):
    r = {}
    with open(path, encoding="utf-8") as f:
        for l in f:
            if l.startswith("#"):
                continue
            p = l.rstrip("\n").split("\t")
            r[p[1]] = (p[0], p[2], float(p[4]) if len(p) > 4 else 0.0)
    return r


def reasons_of(line):
    rs = line[10] if len(line) > 10 else "-"
    if rs == "-":
        return {line[1]} if line[1] != "-" else set()
    return set(rs.split(","))


def family(r):
    """The construct family a reason belongs to (for the census by family)."""
    if r.startswith(("mem:atomic", "inst:atomicrmw", "atomicrmw:", "inst:cmpxchg", "inst:fence")):
        return "atomics"
    if (r == "type:vector" or r.endswith(("element", "shufflevector", "/vector"))
            or r.startswith("intrinsic:llvm.vector.")):
        return "vectors"
    if r.endswith("/i128"):
        return "i128 operations"
    if r.endswith("/fp128"):
        return "fp128 operations"
    return f"`{r}`"


def main():
    a = sys.argv[1:]
    if len(a) == 2 and a[1] == "--hod-only":
        keys, cov = tests_of_keys(a[0]), coverage(a[0])
        for k in sorted(cov):
            if k in keys and cov[k] and all(l[0] == "hod" for l in cov[k]):
                print(keys[k])
        return
    top = 25
    if "--top" in a:
        i = a.index("--top")
        top = int(a[i + 1])
        del a[i : i + 2]
    logdir, res_path, base_path = a
    keys, cov = tests_of_keys(logdir), coverage(logdir)
    res, base = results(res_path), results(base_path)

    print("## Outcomes\n")
    for name, r in (("baseline", base), ("hybrid", res)):
        c = collections.Counter(v[0] for v in r.values())
        print(f"- {name}: " + ", ".join(f"{k} {c[k]}" for k in sorted(c)))
    print("\n## Changed outcomes\n")
    print("| test | baseline | now | detail |\n|---|---|---|---|")
    changed = 0
    for t in sorted(set(res) | set(base)):
        b, n = base.get(t, ("-", "", 0))[0], res.get(t, ("-", "", 0))[0]
        if b != n:
            changed += 1
            print(f"| `{t}` | {b} | {n} | {res.get(t, ('', '-', 0))[1][:120]} |")
    print(f"\n{changed} changed\n")

    # Per test (the key of its compile step) and per module.
    test_cov = {}
    for k, lines in cov.items():
        t = keys.get(k, k)
        test_cov.setdefault(t, []).extend(lines)
    mods = [l for ls in cov.values() for l in ls]
    out = collections.Counter(l[0] for l in mods)
    allhod = [t for t, ls in test_cov.items() if ls and all(l[0] == "hod" for l in ls)]
    print("## Coverage\n")
    print(f"- modules: {len(mods)}; " + ", ".join(f"{k} {out[k]}" for k in sorted(out)))
    print(f"- tests with objects: {len(test_cov)}; all objects by hod: {len(allhod)}; "
          f"some by the fallback: {len(test_cov) - len(allhod)}")
    passed_allhod = [t for t in allhod if res.get(t, ("",))[0] == "PASS"]
    print(f"- of those all-hod tests, PASS now: {len(passed_allhod)}")
    kinds = collections.Counter(l[2] for l in mods)
    print("- input kinds: " + ", ".join(f"{k} {kinds[k]}" for k in sorted(kinds)))

    first = collections.Counter(l[1] for l in mods if l[0] != "hod")
    any_ = collections.Counter()
    alone = collections.Counter()  # reason -> tests hod-only if it alone went away
    for t, ls in test_cov.items():
        rs = set()
        for l in ls:
            if l[0] != "hod":
                rs |= reasons_of(l)
        if len(rs) == 1:
            alone[next(iter(rs))] += 1
    for l in mods:
        if l[0] != "hod":
            for r in reasons_of(l):
                any_[r] += 1
    print(f"\n## First refusals (top {top} of {len(first)})\n")
    print("| reason | modules (first) | modules (any) | tests freed alone |\n|---|---:|---:|---:|")
    for r, n in first.most_common(top):
        print(f"| `{r}` | {n} | {any_[r]} | {alone[r]} |")
    print(f"\n## Every reason, by tests it alone keeps from hod-only (top {top})\n")
    print("| reason | tests freed alone | modules (any) | modules (first) |\n|---|---:|---:|---:|")
    for r, n in sorted(any_.items(), key=lambda kv: (-alone[kv[0]], -kv[1]))[:top]:
        print(f"| `{r}` | {alone[r]} | {n} | {first[r]} |")

    # One construct can carry several names (`inst:atomicrmw` and
    # `atomicrmw:add`): the same count by family.
    fam_alone, fam_mods = collections.Counter(), collections.Counter()
    for t, ls in test_cov.items():
        fs = {family(r) for l in ls if l[0] != "hod" for r in reasons_of(l)}
        if len(fs) == 1:
            fam_alone[next(iter(fs))] += 1
    for l in mods:
        if l[0] != "hod":
            for f in {family(r) for r in reasons_of(l)}:
                fam_mods[f] += 1
    print("\n## By family (tests that only this family keeps from hod-only)\n")
    print("| family | tests freed alone | modules (any) |\n|---|---:|---:|")
    for f, n in sorted(fam_mods.items(), key=lambda kv: (-fam_alone[kv[0]], -kv[1]))[:top]:
        print(f"| {f} | {fam_alone[f]} | {n} |")

    def us(x):
        return int(x) if x not in ("-", "") else 0

    print("\n## Back-end time (sum over modules, seconds)\n")
    hod_ok = [l for l in mods if l[0] == "hod"]
    fb = [l for l in mods if l[0] == "fallback"]
    print(f"- hod-built modules: {len(hod_ok)}, hod {sum(us(l[6]) for l in hod_ok) / 1e6:.1f} s")
    print(f"- fallback modules: {len(fb)}, hod before declining "
          f"{sum(us(l[6]) for l in fb) / 1e6:.1f} s (conversion "
          f"{sum(us(l[8]) for l in fb) / 1e6:.1f} s), llc {sum(us(l[7]) for l in fb) / 1e6:.1f} s")
    ph = phases(logdir)
    if ph:
        print("- phases: " + ", ".join(f"{k} {v / 1e6:.1f}" for k, v in ph.most_common()))
    errs = [l for l in mods if l[0] == "error"]
    bad = [l for l in fb if l[9] != "0"]
    print(f"- outcome error: {len(errs)}; fallback exit != 0: {len(bad)}")
    for l in (errs + bad)[:20]:
        print(f"  - `{l[4]}` {l[1]} status {l[9]}")


if __name__ == "__main__":
    main()
