#!/usr/bin/env python3
"""The census of the hod tree of the LLVM build (WP-178).

census.py [--stage NAME] [--json OUT] <tree>

<tree> is $HOD_OSS_ROOT/llvm-hod. Reads logs/tu.jsonl (kiln's wrapper:
one JSON line per compiler invocation) and logs/coverage.tsv (hod's
coverage log, spec/cli.md "Coverage log", with HOD_CENSUS=1), and prints
a Markdown section: TUs compiled (by kind), modules hod built, modules
that fell back grouped by their reason (field 2) with counts and up to
three example TUs, and the census of every reason (field 11) over the
modules that fell back. With --json it also writes the groups (reason ->
list of {output, source, argv, cwd}) for save-modules.py.

Lines are joined by the object path: field 5 of a coverage line is the
`-o` kiln was given, relative to the wrapper's working directory, which
is recorded in the JSON line.
"""

import argparse
import collections
import json
import os
import sys


def load_tus(path):
    tus = {}
    kinds = collections.Counter()
    langs = collections.Counter()
    links = collections.Counter()
    for line in open(path, encoding="utf-8", errors="replace"):
        try:
            r = json.loads(line)
        except json.JSONDecodeError:
            continue
        if r.get("phase") != "build":
            continue
        if r.get("source") in ("link", "none"):
            links[r.get("kind")] += 1
            continue
        out = r.get("output")
        if not out:
            continue
        key = os.path.normpath(os.path.join(r["cwd"], out))
        if key in tus:
            # A later compile of the same object (a rebuild) replaces it.
            kinds[tus[key]["kind"]] -= 1
            langs[tus[key].get("lang", "?")] -= 1
        tus[key] = r
        kinds[r.get("kind")] += 1
        langs[r.get("lang", "?")] += 1
    return tus, +kinds, +langs, +links


def load_cov(path, tus):
    """The last coverage line of each object, joined to its TU record."""
    cov = {}
    if not os.path.exists(path):
        return cov
    by_rel = collections.defaultdict(list)
    for k, r in tus.items():
        by_rel[r["output"]].append(k)
    for line in open(path, encoding="utf-8", errors="replace"):
        f = line.rstrip("\n").split("\t")
        if len(f) < 11:
            continue
        out = f[4]
        if os.path.isabs(out):
            key = os.path.normpath(out)
        else:
            ks = by_rel.get(out)
            key = ks[-1] if ks else out
        cov[key] = {"outcome": f[0], "reason": f[1], "reasons": [] if f[10] == "-" else split_reasons(f[10]),
                    "hod_us": f[6], "fallback_status": f[9]}
    return cov


def load_stats(path):
    """kiln's compile statistics log (KILN_STATS_LOG): the back end of the
    last job of each input, and why a job with backend `llvm` was not
    offered to hod (its opt triggers other than `alwaysinline`, else the
    static cause: output kind, -O level, LTO)."""
    if not os.path.exists(path):
        return None
    last = {}
    for line in open(path, encoding="utf-8", errors="replace"):
        try:
            r = json.loads(line)
        except json.JSONDecodeError:
            continue
        if r.get("output_kind") == "link" or r.get("input") is None:
            continue
        last[r.get("input")] = r
    backends = collections.Counter()
    not_offered = collections.Counter()
    examples = {}
    for inp, r in last.items():
        b = r.get("backend") or "none"
        backends[b] += 1
        if b == "llvm":
            trig = [t for t in r.get("triggers") or [] if t != "alwaysinline"]
            if trig:
                cause = "triggers: " + ",".join(sorted(trig))
            else:
                cause = f"output {r.get('output_kind')}, -O{r.get('opt_level')}, lto {r.get('lto')}"
            not_offered[cause] += 1
            examples.setdefault(cause, inp)
    return backends, not_offered, examples


def split_reasons(s):
    out, cur, esc = [], [], False
    for ch in s:
        if esc:
            cur.append(ch)
            esc = False
        elif ch == "\\":
            esc = True
        elif ch == ",":
            out.append("".join(cur))
            cur = []
        else:
            cur.append(ch)
    out.append("".join(cur))
    return [x for x in out if x and x != "..."]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("tree")
    ap.add_argument("--stage", default="")
    ap.add_argument("--json")
    a = ap.parse_args()
    tus, kinds, langs, links = load_tus(os.path.join(a.tree, "logs", "tu.jsonl"))
    cov = load_cov(os.path.join(a.tree, "logs", "coverage.tsv"), tus)
    outcomes = collections.Counter(c["outcome"] for c in cov.values())
    groups = collections.defaultdict(list)
    census = collections.Counter()
    census_tus = collections.defaultdict(list)
    for key, c in sorted(cov.items()):
        if c["outcome"] == "hod":
            continue
        t = tus.get(key, {})
        ent = {"output": key, "source": t.get("source"), "cwd": t.get("cwd"), "argv": t.get("argv")}
        groups[c["reason"]].append(ent)
        for r in c["reasons"] or [c["reason"]]:
            census[r] += 1
            census_tus[r].append(ent)
    no_cov = [k for k, r in tus.items() if r.get("kind") == "ok" and k not in cov]
    stats = load_stats(os.path.join(a.tree, "logs", "stats.jsonl"))
    p = print
    p(f"### Census{(' after ' + a.stage) if a.stage else ''}\n")
    p(f"- TUs compiled: {len(tus)} ({', '.join(f'{k} {v}' for k, v in sorted(kinds.items()))}; "
      f"{', '.join(f'{k} {v}' for k, v in sorted(langs.items()))})")
    p(f"- links: {sum(links.values())} ({', '.join(f'{k} {v}' for k, v in sorted(links.items()))})")
    p(f"- modules hod built: {outcomes['hod']}")
    p(f"- modules hod did not build: {outcomes['error'] + outcomes['fallback']} (kiln runs hod with "
      f"--strict, so a decline is outcome `error` and kiln falls back to llc: {outcomes['error']}; "
      f"outcome `fallback`: {outcomes['fallback']})")
    p(f"- compiled TUs without a coverage line (kiln did not hand the module to hod): {len(no_cov)}")
    for k in no_cov[:5]:
        p(f"  - `{os.path.relpath(k, a.tree)}`")
    if stats:
        backends, not_offered, examples = stats
        p(f"- kiln's statistics log (front-end jobs, last per input): "
          f"{', '.join(f'{k} {v}' for k, v in sorted(backends.items()))}")
        for cause, n in sorted(not_offered.items(), key=lambda kv: -kv[1]):
            p(f"  - not offered to hod, `{cause}`: {n} (e.g. `{examples[cause]}`)")
    p("")
    if groups:
        p("| reason (field 2) | modules | example TUs |")
        p("|---|---|---|")
        for r, ents in sorted(groups.items(), key=lambda kv: (-len(kv[1]), kv[0])):
            ex = ", ".join(f"`{os.path.relpath(e['source'] or e['output'], a.tree)}`" for e in ents[:3])
            p(f"| `{r}` | {len(ents)} | {ex} |")
        p("")
        p("Every reason of the modules that fell back (field 11, HOD_CENSUS=1):\n")
        p("| reason | modules |")
        p("|---|---|")
        for r, n in sorted(census.items(), key=lambda kv: (-kv[1], kv[0])):
            p(f"| `{r}` | {n} |")
        p("")
    if a.json:
        with open(a.json, "w") as f:
            json.dump({"first": groups, "census": census_tus}, f, indent=1)
    return 0


if __name__ == "__main__":
    sys.exit(main())
