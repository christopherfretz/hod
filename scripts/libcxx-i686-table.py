#!/usr/bin/env python3
"""The per-test table and the refusal census of an i386 libc++ run made by
scripts/libcxx-hod-i686.sh in all three modes (WP-336;
docs/status/libcxx-hod-i686.md).

    scripts/libcxx-i686-table.py <run dir> <out dir>

<run dir> holds control.tsv, hybrid.tsv, strict.tsv and the merged log
directories hybrid/ and strict/ (`libcxx-hod-i686.sh merge`); hybrid may
be missing (its column is then "-" and the census is strict's). Which back
end built a module is read from the coverage log's first field (`hod`,
`fallback`, `error`), never from an exit status. Writes to <out dir>:

- table.tsv: test, control, hybrid, strict outcome, hybrid modules by hod
  / with objects, strict modules by hod / with objects, the strict
  refusals (first reason of each refused module), the hybrid census
  reasons (every reason of every declined module, HOD_CENSUS=1);
- totals.md: the three columns' outcome counts, and per column the tests
  hod built completely;
- census.md: every reason by the tests it blocks (any) and the tests it
  alone blocks (only), from the hybrid census;
- suspects.tsv: tests that PASS in control and do not pass in hybrid or
  strict although hod built at least one of their modules in that mode;
- trouble.tsv: hod steps that crashed, panicked or ran 60 s or more.
"""
import collections
import os
import sys

MARK = "/libcxx/test/"


def results(path):
    r = {}
    with open(path, encoding="utf-8") as f:
        for line in f:
            if line.startswith("#") or not line.strip():
                continue
            p = line.rstrip("\n").split("\t")
            r[p[1]] = (p[0], p[2] if len(p) > 2 else "-")
    return r


def key_tests(logdir):
    """key -> test, from the compile steps' sources (time.tsv) and argv."""
    out = {}
    tpath = os.path.join(logdir, "time.tsv")
    if os.path.exists(tpath):
        with open(tpath, encoding="utf-8", errors="replace") as f:
            for line in f:
                p = line.rstrip("\n").split("\t")
                if len(p) >= 5 and MARK in p[4]:
                    out.setdefault(p[0], p[4].split(MARK, 1)[1])
    with open(os.path.join(logdir, "args.tsv"), encoding="utf-8", errors="replace") as f:
        for line in f:
            key, _, argv = line.rstrip("\n").partition("\t")
            if key in out:
                continue
            for a in argv.split(" "):
                if MARK in a and a.endswith(".cpp"):
                    out[key] = a.split(MARK, 1)[1]
    return out


def per_test_cov(logdir):
    """test -> list of coverage lines (fields) of its modules."""
    keys = key_tests(logdir)
    d = os.path.join(logdir, "cov")
    cov = collections.defaultdict(list)
    for name in os.listdir(d):
        t = keys.get(name[:-4], name[:-4])
        with open(os.path.join(d, name), encoding="utf-8", errors="replace") as f:
            cov[t].extend(line.rstrip("\n").split("\t") for line in f if line.strip())
    return cov


def all_reasons(line):
    rs = line[10] if len(line) > 10 else "-"
    if rs == "-":
        return [line[1]] if line[1] != "-" else []
    return rs.split(",")


def trouble(run, out, modes):
    """trouble.tsv: hod steps that ended other than 0 (built) or 2 (declined:
    a refusal in strict) -- a panic (101), a signal (128 + n), another
    error -- or that ran 60 s or more, from <mode>/time.tsv (WP-339)."""
    with open(os.path.join(out, "trouble.tsv"), "w", encoding="utf-8") as f:
        f.write("# mode\ttest\tkey\tstatus\tseconds\tsource\n")
        for m in modes:
            logdir = os.path.join(run, m)
            keys = key_tests(logdir)
            with open(os.path.join(logdir, "time.tsv"), encoding="utf-8", errors="replace") as tf:
                for line in tf:
                    p = line.rstrip("\n").split("\t")
                    if len(p) < 5 or p[1] != "hod":
                        continue
                    secs = int(p[2]) / 1e6
                    if p[3] not in ("0", "2") or secs >= 60:
                        f.write(f"{m}\t{keys.get(p[0], '-')}\t{p[0]}\t{p[3]}\t{secs:.1f}\t{p[4]}\n")


def main():
    run, out = sys.argv[1], sys.argv[2]
    os.makedirs(out, exist_ok=True)
    # A run may lack hybrid (WP-339 ran control and strict only): its column
    # is then "-", and the census comes from strict's coverage logs, which
    # list every reason too (the runner gives strict HOD_CENSUS=1).
    have = {m: os.path.exists(os.path.join(run, f"{m}.tsv")) for m in ("hybrid", "strict")}
    res = {m: results(os.path.join(run, f"{m}.tsv")) if have.get(m, True) else {}
           for m in ("control", "hybrid", "strict")}
    cov = {m: per_test_cov(os.path.join(run, m)) if have[m] else {} for m in ("hybrid", "strict")}
    census_from = "hybrid" if have["hybrid"] else "strict"
    trouble(run, out, [m for m in ("hybrid", "strict") if have[m]])
    tests = sorted(set().union(*res.values()))

    def built(m, t):
        ls = cov[m].get(t, [])
        return sum(1 for l in ls if l[0] == "hod"), len(ls)

    rows, suspects = [], []
    for t in tests:
        c, h, s = (res[m].get(t, ("-", "-"))[0] for m in ("control", "hybrid", "strict"))
        hb, hn = built("hybrid", t)
        sb, sn = built("strict", t)
        refused = sorted({l[1] for l in cov["strict"].get(t, []) if l[0] != "hod"})
        census = sorted({r for l in cov[census_from].get(t, []) if l[0] != "hod"
                         for r in all_reasons(l)})
        rows.append((t, c, h, s, f"{hb}/{hn}", f"{sb}/{sn}", ",".join(refused) or "-",
                     ",".join(census) or "-"))
        for m, o, b in (("hybrid", h, hb), ("strict", s, sb)):
            if c == "PASS" and o != "PASS" and b > 0:
                suspects.append((t, m, o, f"{b}/{hn if m == 'hybrid' else sn}",
                                 res[m].get(t, ("-", "-"))[1][:200]))
    with open(os.path.join(out, "table.tsv"), "w", encoding="utf-8") as f:
        f.write("# test\tcontrol\thybrid\tstrict\thybrid-hod/modules\tstrict-hod/modules"
                f"\tstrict-refusals\t{census_from}-census\n")
        for r in rows:
            f.write("\t".join(r) + "\n")
    with open(os.path.join(out, "suspects.tsv"), "w", encoding="utf-8") as f:
        f.write("# test\tmode\toutcome\thod/modules\tdetail\n")
        for r in suspects:
            f.write("\t".join(r) + "\n")

    with open(os.path.join(out, "totals.md"), "w", encoding="utf-8") as f:
        outcomes = sorted({r[i] for r in rows for i in (1, 2, 3)})
        f.write("| outcome | control | hybrid | strict |\n|---|---:|---:|---:|\n")
        for o in outcomes:
            f.write(f"| {o} | " + " | ".join(str(sum(1 for r in rows if r[i] == o))
                                             for i in (1, 2, 3)) + " |\n")
        f.write(f"| total | {len(rows)} | {len(rows)} | {len(rows)} |\n\n")
        for m, i in (("hybrid", 4), ("strict", 5)):
            if not have[m]:
                continue
            with_obj = [r for r in rows if not r[i].endswith("/0")]
            full = [r for r in with_obj if r[i].split("/")[0] == r[i].split("/")[1]]
            mods = sum(int(r[i].split("/")[1]) for r in rows)
            hodm = sum(int(r[i].split("/")[0]) for r in rows)
            f.write(f"- {m}: tests with objects {len(with_obj)}, all objects by hod {len(full)}, "
                    f"modules {mods}, by hod {hodm}\n")
        refused = [r for r in rows if r[6] != "-"]
        f.write(f"- strict: tests with a refused module {len(refused)}; of those, outcome "
                + ", ".join(f"{k} {v}" for k, v in sorted(collections.Counter(r[3] for r in refused).items()))
                + "\n")
        f.write(f"- pass in control, not in hybrid or strict, a module by hod: {len(suspects)} rows, "
                f"{len({s[0] for s in suspects})} tests\n")

    anyc, only = collections.Counter(), collections.Counter()
    first = collections.Counter()
    for r in rows:
        rs = [] if r[7] == "-" else r[7].split(",")
        for x in rs:
            anyc[x] += 1
        if len(rs) == 1:
            only[rs[0]] += 1
        for x in ([] if r[6] == "-" else r[6].split(",")):
            first[x] += 1
    with open(os.path.join(out, "census.md"), "w", encoding="utf-8") as f:
        f.write(f"{sum(1 for r in rows if r[7] != '-')} tests with a declined module "
                f"({census_from} census)\n\n")
        f.write("| reason | tests blocked | blocked only by it | strict: tests, as first refusal |\n"
                "|---|---:|---:|---:|\n")
        for x, n in sorted(anyc.items(), key=lambda kv: (-kv[1], -only[kv[0]], kv[0])):
            f.write(f"| `{x}` | {n} | {only[x]} | {first[x]} |\n")
        extra = sorted(set(first) - set(anyc))
        for x in extra:
            f.write(f"| `{x}` (strict only) | 0 | 0 | {first[x]} |\n")


if __name__ == "__main__":
    main()
