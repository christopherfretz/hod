#!/usr/bin/env python3
"""The i386 real-project lane's summaries (scripts/oss/i686/i686-oss.sh).

  i686-report.py tree <root>/<project>/<mode>   one tree's summary
  i686-report.py table <root>                   the lane's markdown table,
      the census, the tests that pass in control and fail in hybrid or
      strict, and the back-end time of llc (control) against hod over the
      objects both built (hod time of a hybrid object includes its fallback)

A tree's logs/: time.tsv (step, us, status, source, object), cov.tsv (the
source, then hod's coverage line: back end, first reason, ..., field 11
every reason), tests.tsv (name, PASS|FAIL), build.ok (make's status).
"""
import collections
import os
import sys

MODES = ("control", "hybrid", "strict")


def rows(path):
    if not os.path.exists(path):
        return []
    with open(path, errors="replace") as f:
        return [l.rstrip("\n").split("\t") for l in f if l.strip()]


def reasons(c):  # c: a cov.tsv row (source first)
    line = c[1:]
    rs = line[10] if len(line) > 10 else "-"
    if rs in ("-", ""):
        return {line[1]} if len(line) > 1 and line[1] != "-" else set()
    return set(rs.split(","))


def tree(t):
    logs = os.path.join(t, "logs")
    times = rows(os.path.join(logs, "time.tsv"))
    cov = rows(os.path.join(logs, "cov.tsv"))
    tests = rows(os.path.join(logs, "tests.tsv"))
    be = {}  # object -> (step, us, status)
    for step, us, st, _src, obj in (r[:5] for r in times if len(r) >= 5):
        if step in ("llc", "hod"):
            be[obj] = (step, int(us), st)
    units = [r for r in times if r[0] == "ir" and r[2] == "0"]
    refused = [c for c in cov if c[1] != "hod"]
    names = collections.Counter(r for c in refused for r in reasons(c))
    status = open(os.path.join(t, "build.ok")).read().strip() if os.path.exists(os.path.join(t, "build.ok")) else "?"
    return {
        "units": len(units),
        "by_hod": sum(1 for c in cov if c[1] == "hod"),
        "refused": len(refused),
        "refused_srcs": sorted({c[0] for c in refused}),
        "names": names,
        "failed_backend": sum(1 for v in be.values() if v[2] != "0"),
        "be": be,
        "build": status,
        "tests": {r[0]: r[1] for r in tests if len(r) >= 2},
    }


def show_tree(t):
    s = tree(t)
    p = s["tests"]
    npass = sum(1 for v in p.values() if v == "PASS")
    print(f"{t}: build status {s['build']}; units {s['units']}; by hod {s['by_hod']}; "
          f"refused {s['refused']}; back-end failures {s['failed_backend']}; "
          f"tests {npass} pass / {len(p) - npass} fail")
    for n, c in s["names"].most_common():
        print(f"  refused: {n} x{c}")
    for n, v in sorted(p.items()):
        if v != "PASS":
            print(f"  FAIL {n}")


def table(root):
    print("| project | mode | units | by hod | refused | back-end failures | build | tests pass / fail |")
    print("|---|---|---:|---:|---:|---:|---|---|")
    census = collections.defaultdict(collections.Counter)
    suspects, timing = [], []
    for p in sorted(os.listdir(root)):
        trees = {m: tree(os.path.join(root, p, m)) for m in MODES if os.path.isdir(os.path.join(root, p, m))}
        for m, s in trees.items():
            npass = sum(1 for v in s["tests"].values() if v == "PASS")
            hod = s["by_hod"] if m != "control" else "-"
            ref = s["refused"] if m != "control" else "-"
            print(f"| {p} | {m} | {s['units']} | {hod} | {ref} | {s['failed_backend']} | {s['build']} | "
                  f"{npass} / {len(s['tests']) - npass} |")
            if m != "control":
                for n, c in s["names"].items():
                    census[(n, p)][m] = c
        c = trees.get("control")
        if not c:
            continue
        for m in ("hybrid", "strict"):
            s = trees.get(m)
            if not s:
                continue
            for n, v in sorted(s["tests"].items()):
                if c["tests"].get(n) == "PASS" and v != "PASS":
                    suspects.append((p, m, n))
            # The same objects (by path relative to the tree), built by both.
            cb = {os.path.relpath(o, os.path.join(root, p, "control")): v for o, v in c["be"].items()}
            hb = {os.path.relpath(o, os.path.join(root, p, m)): v for o, v in s["be"].items()}
            common = [o for o in cb if o in hb and cb[o][2] == "0" and hb[o][2] == "0" and hb[o][0] == "hod"]
            tl, th = sum(cb[o][1] for o in common), sum(hb[o][1] for o in common)
            timing.append((p, m, len(common), tl, th))
    print("\n## Census (reasons hod refused, modules)\n")
    print("| reason | project | hybrid | strict |\n|---|---|---:|---:|")
    for (n, p), c in sorted(census.items()):
        print(f"| `{n}` | {p} | {c.get('hybrid', 0)} | {c.get('strict', 0)} |")
    if not census:
        print("| (none) | | | |")
    print("\n## Tests passing in control, failing with hod\n")
    for p, m, n in suspects:
        print(f"- {p} {m}: {n}")
    if not suspects:
        print("- none")
    print("\n## Back-end time over the same objects (ms; hod time of a hybrid object includes its fallback)\n")
    print("| project | mode | objects | llc | hod | hod / llc |\n|---|---|---:|---:|---:|---:|")
    for p, m, n, tl, th in timing:
        r = f"{th / tl:.2f}" if tl else "-"
        print(f"| {p} | {m} | {n} | {tl / 1000:.0f} | {th / 1000:.0f} | {r} |")


if __name__ == "__main__":
    if len(sys.argv) != 3 or sys.argv[1] not in ("tree", "table"):
        sys.exit(__doc__)
    (show_tree if sys.argv[1] == "tree" else table)(sys.argv[2])
