#!/usr/bin/env python3
"""oss-report.py: the reports of hod's real-project lane (spec/harness.md
"Real projects").

  oss-report.py [--target T] collect <project>   copy the modules hod refused in the
                                    project's hod and strict trees into
                                    <findings>/<project>/<reason>/ (at most
                                    $HOD_OSS_KEEP, default one, per reason
                                    per project), each with its command
                                    line and a one-line description
  oss-report.py rerun <project> <flavour> <mode> <test> [--times N]
                                    run one ctest test of a kept tree N
                                    times (default 2) under the row's
                                    memory cap, appending the count of
                                    passes to the tree's reruns.jsonl
  oss-report.py status [--out F] [--root DIR]... [--provenance F] [--candidate C]
                                    docs/status/oss.md from every report
                                    (stdout without --out); with --root,
                                    the reports of each lane root named
                                    (the newest report of a build wins);
                                    --provenance names the hand-kept file
                                    of hod commits, kiln commits and notes
                                    (scripts/oss/provenance-<target>.tsv);
                                    --candidate marks the rows measured at
                                    another hod than the candidate's
  oss-report.py census [--out F]    the census page (the status rows and
                                    the refusals with their findings)

Reads <root>/build/<project>/<flavour>-<mode>/report.json (written by
oss-build.sh through projects.py). The root is $HOD_OSS_ROOT, else
<build root>/oss beside $CARGO_TARGET_DIR; the findings are
$HOD_OSS_FINDINGS, else <build root>/findings/oss. `--target x86_64`
(WP-191, before the command) reads the cross trees instead,
<root>/x64-build/, and keeps their findings under <findings>/x64/.
"""

import argparse
import glob
import json
import os
import shutil
import stat
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.dont_write_bytecode = True  # no __pycache__ in the checkout
sys.path.insert(0, HERE)
import projects  # noqa: E402

FLAVOURS = ("O0", "O0g")
MODES = ("llc", "hod", "strict")
KEEP = int(os.environ.get("HOD_OSS_KEEP") or 1)


def roots():
    oss, findings = os.environ.get("HOD_OSS_ROOT"), os.environ.get("HOD_OSS_FINDINGS")
    if oss and findings:
        return oss, findings
    broot = os.environ.get("HOD_BUILD_ROOT")
    if not broot:
        t = os.environ.get("CARGO_TARGET_DIR")
        if not t:
            sys.exit("oss-report: source scripts/env.sh first (or set HOD_OSS_ROOT and HOD_OSS_FINDINGS)")
        broot = os.path.dirname(t)
    return oss or os.path.join(broot, "oss"), findings or os.path.join(broot, "findings", "oss")


# A target's trees and findings (oss-build.sh's TP prefix).
PREFIX = {"aarch64": "", "x86_64": "x64-"}
TARGET = "aarch64"


def trees(oss):
    return os.path.join(oss, PREFIX[TARGET] + "build")


def target_findings(findings):
    return findings if TARGET == "aarch64" else os.path.join(findings, "x64")


def load_reports(oss):
    reps = {}
    for path in sorted(glob.glob(os.path.join(trees(oss), "*", "*", "report.json"))):
        tree = os.path.basename(os.path.dirname(path))
        if tree.endswith("-dep"):
            continue
        try:
            with open(path, encoding="utf-8") as f:
                r = json.load(f)
        except (OSError, json.JSONDecodeError):
            continue
        # The per-test results from the log, as this projects.py reads it
        # (a report written by an older one is read alike); a log this
        # projects.py cannot read (a project's own test runner, not ctest)
        # leaves the report's own results.
        log = os.path.join(os.path.dirname(path), "test.log")
        if r.get("test_rc") is not None and os.path.exists(log):
            r["test_status"] = projects.test_results(log)[2] or r.get("test_status") or {}
        # Reruns of single tests (`rerun`): {test: {"runs": n, "passed": k}}.
        r["reruns"] = {}
        rr = os.path.join(os.path.dirname(path), "reruns.jsonl")
        if os.path.exists(rr):
            with open(rr, encoding="utf-8") as f:
                for line in f:
                    e = json.loads(line)
                    a = r["reruns"].setdefault(e["test"], {"runs": 0, "passed": 0, "env": set()})
                    a["env"].add(e.get("arena") or "unset")
                    a["runs"] += 1
                    a["passed"] += 1 if e["passed"] else 0
        reps[(r["project"], r["flavour"], r["mode"])] = r
    return reps


def load_roots(oss_roots):
    """The reports of several lane roots (`status --root`): for a (project,
    flavour, mode) found under more than one root, the newest report (its
    `date`) is the row's."""
    reps = {}
    for oss in oss_roots:
        for k, r in load_reports(oss).items():
            if k not in reps or (r.get("date") or "") > (reps[k].get("date") or ""):
                reps[k] = r
    return reps


# The provenance file (`status --provenance`), hand-kept from the lanes'
# state notes, one tab-separated row per line ('#' starts a comment):
#   hod<TAB><binary name><TAB><hod commit>        a frozen copy named by its sha256
#   kiln<TAB><project><TAB><flavour><TAB><mode><TAB><kiln>
#                                                 for a report without a `kiln` field
#                                                 (`*` matches any project, flavour or mode)
#   note<TAB><Markdown>                           a numbered note under the table
PROV = {"hod": {}, "kiln": [], "note": []}


def load_provenance(path):
    with open(path, encoding="utf-8") as f:
        for n, line in enumerate(f, 1):
            line = line.rstrip("\n")
            if not line.strip() or line.startswith("#"):
                continue
            c = line.split("\t")
            if c[0] == "hod" and len(c) == 3:
                PROV["hod"][c[1]] = c[2]
            elif c[0] == "kiln" and len(c) == 5:
                PROV["kiln"].append(c[1:])
            elif c[0] == "note" and len(c) == 2:
                PROV["note"].append(c[1])
            else:
                sys.exit(f"oss-report: {path}:{n}: not a provenance row")


def hod_commit(r):
    """The hod commit of a report: the provenance file's for its binary,
    else the binary's name without `hod-` and `main-` (a frozen copy's name
    carries its commit)."""
    name = os.path.basename(r.get("hod_path") or "")
    if not name:
        return ""
    return PROV["hod"].get(name) or name.removeprefix("hod-").removeprefix("main-")


def kiln_of(r):
    if r.get("kiln"):
        return r["kiln"]
    for p, fl, m, k in PROV["kiln"]:
        if p in ("*", r["project"]) and fl in ("*", r["flavour"]) and m in ("*", r["mode"]):
            return k
    return ""


def order(reps):
    """Projects in projects.toml's order, then any others."""
    names = list(projects.load())
    seen = {p for p, _, _ in reps}
    return [n for n in names if n in seen] + sorted(seen - set(names))


def build_cell(r):
    if r is None:
        return "-"
    if r["configure_rc"] != 0:
        return "configure failed"
    if r["build_rc"] == 0:
        return "ok"
    t = r["tus"]
    return f"failed ({t['failed']} TUs)"


def test_cell(r):
    if r is None or r.get("tests") is None:
        return "-" if r is None or r.get("test_rc") is None else f"rc {r['test_rc']}"
    t = r["tests"]
    return f"{t['passed']}/{t['total']}"


def diff_tests(ctl, r):
    """Tests that pass in the control and do not in r, and the reverse."""
    if ctl is None or r is None:
        return None, None
    a, b = ctl.get("test_status") or {}, r.get("test_status") or {}
    worse = sorted(t for t, s in a.items() if s == "Passed" and b.get(t, "missing") != "Passed")
    better = sorted(t for t, s in b.items() if s == "Passed" and a.get(t) != "Passed")
    return worse, better


def hod_commits(*rs):
    return sorted({hod_commit(r) for r in rs if r} - {""})


def hod_cell(*rs):
    """The hod commits of the hod and strict builds (`hod_commit`)."""
    return ", ".join(f"`{n}`" for n in hod_commits(*rs)) or "-"


def kiln_cell(*rs):
    """The kiln commits of the row's builds (the control's included)."""
    names = sorted({kiln_of(r) for r in rs if r} - {""})
    return ", ".join(f"`{n}`" for n in names) or "-"


# `status --candidate <commit>`: the release candidate's hod commit; a row
# whose hod or strict build used another hod is marked as measured before it.
CANDIDATE = None


def candidate_cell(*rs):
    names = hod_commits(*rs)
    if not names:
        return "-"
    if CANDIDATE == "-":  # not chosen yet: every row is measured before it
        return "no: measured before the candidate"
    c = CANDIDATE.lower()
    at = all(n.lower().startswith(c) or c.startswith(n.lower()) for n in names)
    return "yes" if at else "no: measured before the candidate"


def status_rows(reps, notes=None):
    """The table; a test worse than the control's that passed every
    recorded rerun (and at least two) is not counted worse: the cell says
    `same (rerun)` with a numbered note, appended to `notes`."""
    notes = [] if notes is None else notes
    cand = " at the candidate |" if CANDIDATE else ""
    out = [
        "| project | revision | flavour | TUs | objects | built by hod | fell back | not offered | hybrid build | strict build | tests llc | tests hod | tests strict | against the control | hod | kiln |" + cand,
        "|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|" + ("---|" if CANDIDATE else ""),
    ]
    for p in order(reps):
        for fl in FLAVOURS:
            l, h, s = (reps.get((p, fl, m)) for m in MODES)
            if not (l or h or s):
                continue
            ref = h or s or l
            c = (h or s or {}).get("hod") or {}
            be = c.get("backends", {})
            fell = be.get("hod-declined", 0) + be.get("hod-failed", 0)
            no = be.get("llvm", 0)
            diffs = []
            reran = []
            for name, r in (("hod", h), ("strict", s)):
                if l and l.get("tests") and r and not r.get("tests"):
                    diffs.append(f"{name}: not tested")  # its build failed or its tests did not run
                    continue
                worse, better = diff_tests(l, r)
                worse, better = worse or [], better or []
                rr = (r or {}).get("reruns") or {}
                for t in [t for t in worse if t in rr]:
                    e = rr[t]
                    if e["runs"] >= 2 and e["passed"] == e["runs"]:
                        worse.remove(t)
                        notes.append(f"{p} {fl} {name}: `{t}` failed in the build's test run and passed {e['passed']} of {e['runs']} reruns under the same memory cap"
                                     f" (`MALLOC_ARENA_MAX` {', '.join(sorted(e['env']))}).")
                        reran.append(len(notes))
                    else:
                        notes.append(f"{p} {fl} {name}: `{t}` passed {e['passed']} of {e['runs']} reruns.")
                        worse[worse.index(t)] = f"{t} (note {len(notes)})"
                if worse:
                    diffs.append(f"{name}: {len(worse)} worse ({', '.join(worse[:3])}{', ...' if len(worse) > 3 else ''})")
                if better:
                    diffs.append(f"{name}: {len(better)} better")
            if l and (h or s) and not diffs:
                diffs.append("same (rerun, " + ", ".join(f"note {n}" for n in reran) + ")" if reran else "same")
            elif reran:
                diffs.append(", ".join(f"note {n}" for n in reran))
            out.append(
                f"| {p} | {ref.get('tag') or '-'} | {fl} | {ref['tus']['total']} | {c.get('objects', '-')} | {be.get('hod', '-') if c else '-'} | "
                f"{fell if c else '-'} | {no if c else '-'} | {build_cell(h)} | {build_cell(s)} | "
                f"{test_cell(l)} | {test_cell(h)} | {test_cell(s)} | {'; '.join(diffs) or '-'} | {hod_cell(h, s)} | {kiln_cell(l, h, s)} |"
                + (f" {candidate_cell(h, s)} |" if CANDIDATE else "")
            )
    return out


def census_rows(reps, findings=None):
    agg = {}
    for (p, fl, m), r in reps.items():
        if m != "hod" or not r.get("hod"):
            continue
        for name, e in r["hod"]["refusals"].items():
            a = agg.setdefault(name, {"first": 0, "modules": 0, "where": set(), "functions": {}})
            a["first"] += e["first"]
            a["modules"] += e["modules"]
            a["where"].add(f"{p} {fl}")
            for fn, n in e["functions"].items():
                a["functions"][fn] = a["functions"].get(fn, 0) + n
    if not agg:
        return ["No refusal: every module kiln offered hod in the hybrid builds was built by hod."]
    out = [
        "`first`: modules for which it was the reason hod gave (the module fell back for it); "
        "`census`: modules in which hod found it at all (`HOD_CENSUS=1`).",
        "",
        "| refusal | first | census | builds | functions (most frequent) |" + (" findings |" if findings else ""),
        "|---|---|---|---|---|" + ("---|" if findings else ""),
    ]
    for name, a in sorted(agg.items(), key=lambda kv: (-kv[1]["first"], -kv[1]["modules"], kv[0])):
        fns = sorted(a["functions"].items(), key=lambda kv: -kv[1])[:3]
        fcell = ", ".join(f"`{f[:60]}`" for f, _ in fns) or "-"
        row = f"| `{name}` | {a['first']} | {a['modules']} | {', '.join(sorted(a['where']))} | {fcell} |"
        if findings:
            mine = {p for p, _, _ in reps}
            d = sorted(x for x in glob.glob(os.path.join(findings, "*", safe(name))) if os.path.basename(os.path.dirname(x)) in mine)
            row += f" {', '.join(os.path.relpath(x, findings) for x in d) or '-'} |"
        out.append(row)
    return out


def strict_rows(reps):
    """The strict builds by what kiln's statistics log says of each
    object (`backend`), not by the build's exit status (WP-191): under
    KILN_HOD_STRICT=1 an object hod declined is still built by llc."""
    rows = []
    for p in order(reps):
        for fl in FLAVOURS:
            r = reps.get((p, fl, "strict"))
            if not r or not r.get("hod"):
                continue
            be = r["hod"].get("backends", {})
            refused = ", ".join(f"`{k}` {v['modules']}" for k, v in r["hod"].get("refusals", {}).items()) or "-"
            rows.append(f"| {p} | {fl} | {r['hod'].get('objects', 0)} | {be.get('hod', 0)} | {be.get('hod-declined', 0)} | "
                        f"{be.get('hod-failed', 0)} | {be.get('llvm', 0)} | {build_cell(r)} | {test_cell(r)} | {refused} |")
    if not rows:
        return []
    return ["", "## Strict builds (kiln's statistics log)", "",
            "| project | flavour | objects | hod | declined | failed | llvm | build | tests | refusals (modules) |",
            "|---|---|---|---|---|---|---|---|---|---|", *rows]


def not_offered_rows(reps):
    agg = {}
    for (p, fl, m), r in reps.items():
        if m != "hod" or not r.get("hod"):
            continue
        for k, n in r["hod"]["not_offered"].items():
            agg.setdefault(k, {}).setdefault(f"{p} {fl}", n)
    if not agg:
        return []
    out = ["", "Objects kiln did not offer hod (kiln's statistics log: the `opt` triggers, `-` for a static reason):", "",
           "| why | objects | builds |", "|---|---|---|"]
    for k, w in sorted(agg.items(), key=lambda kv: -sum(kv[1].values())):
        out.append(f"| `{k}` | {sum(w.values())} | {', '.join(f'{b} ({n})' for b, n in sorted(w.items()))} |")
    return out


def safe(name):
    return projects.SAFE_RE.sub("_", name or "unknown")[:100]


def kept_source(d, src):
    """Whether a module of `src` is already kept in `d`."""
    for c in glob.glob(os.path.join(d, "*.cmd")):
        with open(c, encoding="utf-8", errors="replace") as f:
            if f" {src} " in f.read():
                return True
    return False


def functions_with(ll, reason):
    """For a refusal `attr:<a>` that names no function: the functions
    defined with an attribute group holding `<a>` (or `"<a>"=...`)."""
    if not reason.startswith("attr:"):
        return []
    attr = reason[len("attr:") :]
    groups = set()
    defs = []
    try:
        with open(ll, encoding="utf-8", errors="replace") as f:
            for line in f:
                if line.startswith("attributes #"):
                    if f'"{attr}"' in line or f" {attr} " in line:
                        groups.add(line.split()[1])
                elif line.startswith("define "):
                    defs.append(line)
    except OSError:
        return []
    out = []
    for line in defs:
        toks = line.rstrip().rstrip("{").split()
        if any(t in groups for t in toks):
            name = next((t for t in toks if t.startswith("@")), "?").split("(")[0]
            out.append(name.lstrip("@"))
    return out


def census_of(tree):
    """{(output, bytes): census reasons} from a tree's coverage log: every
    reason hod found in each refused module (HOD_CENSUS=1)."""
    out = {}
    for c in projects.read_coverage(os.path.join(tree, "coverage.log")):
        if c["outcome"] != "hod":
            out.setdefault((c["output"], c["bytes"]), set()).update(x for x in c["census"] + [c["reason"]] if x and x != "-")
    return out


def cmd_collect(a):
    oss, findings = roots()
    findings = target_findings(findings)
    kept = 0
    for fl in FLAVOURS:
        for m in ("hod", "strict"):
            tree = os.path.join(trees(oss), a.project, f"{fl}-{m}")
            cen = census_of(tree)
            for meta in sorted(glob.glob(os.path.join(tree, "declined", "*", "*.json"))):
                with open(meta, encoding="utf-8") as f:
                    rec = json.load(f)
                ll = meta[: -len(".json")] + ".ll"
                if not os.path.exists(ll):
                    continue
                first = rec.get("reason")
                # The module is kept under its first reason and under every
                # other reason of its census (WP-191: a reason that is never
                # a module's first still gets its modules).
                others = sorted(cen.get((rec.get("output"), rec.get("bytes")), set()) - {first})
                src = next((x for x in rec.get("kiln_argv", []) if x.endswith((".cc", ".cpp", ".cxx", ".c", ".C"))), "?")
                for reason in [first] + others:
                    d = os.path.join(findings, a.project, safe(reason))
                    os.makedirs(d, exist_ok=True)
                    have = sorted(glob.glob(os.path.join(d, "*.ll")))
                    have = [h for h in have if os.path.basename(h) != "min.ll"]
                    # A reduced reason (min.ll, its 1.ll dropped) is kept already.
                    if len(have) >= KEEP or os.path.exists(os.path.join(d, "min.ll")) or kept_source(d, src):
                        continue
                    n = len(have) + 1
                    # A hard link, not a copy: one module is kept under each of
                    # its census reasons, and a vector-heavy project's modules
                    # (100 MB, hundreds of reasons) would fill the disk (WP-200).
                    try:
                        os.link(ll, os.path.join(d, f"{n}.ll"))
                    except OSError:
                        shutil.copyfile(ll, os.path.join(d, f"{n}.ll"))
                    fn = rec.get("function") if reason == first else None
                    if not fn:
                        fns = functions_with(ll, reason or "")
                        if fns:
                            fn = ", ".join(fns[:3]) + (" ..." if len(fns) > 3 else "")
                    with open(os.path.join(d, f"{n}.cmd"), "w", encoding="utf-8") as f:
                        f.write(f"# flavour {fl}, mode {m}; cwd {rec.get('kiln_cwd')}\n")
                        f.write(" ".join(rec.get("kiln_argv", [])) + " \n")
                        f.write("# hod " + " ".join(rec.get("hod_args", [])) + "\n")
                        if reason == first:
                            f.write("# " + (rec.get("diag") or "") + "\n")
                        else:
                            f.write(f"# census: {reason} (the module's first refusal: {first})\n")
                    how = "" if reason == first else f", census; first `{first}`"
                    desc = f"{n}: `{reason}`" + (f" in `{fn}`" if fn else "") + f" ({os.path.basename(src)}, {fl}, {m}{how})"
                    with open(os.path.join(d, "README.md"), "a", encoding="utf-8") as f:
                        f.write(desc + "\n")
                    kept += 1
    print(f"oss-report: {kept} modules kept under {os.path.join(findings, a.project)}")


def page(reps, title, intro, findings=None):
    lines = [f"# {title}", "", *intro, ""]
    notes = []
    lines += status_rows(reps, notes)
    notes += PROV["note"]
    if notes:
        lines += ["", "Notes:", ""] + [f"{i}. {n}" for i, n in enumerate(notes, 1)]
    lines += ["", "## Refusal census (hybrid builds)", ""]
    lines += census_rows(reps, findings)
    lines += not_offered_rows(reps)
    lines += strict_rows(reps)
    return "\n".join(lines) + "\n"


def write(text, out):
    if out:
        with open(out, "w", encoding="utf-8") as f:
            f.write(text)
        print(f"oss-report: wrote {out}")
    else:
        sys.stdout.write(text)


def cmd_rerun(a):
    """Run one test of a kept tree `a.times` times under the row's cap."""
    import resource
    import subprocess
    import time

    oss, _ = roots()
    tree = os.path.join(trees(oss), a.project, f"{a.flavour}-{a.mode}")
    if not os.path.exists(os.path.join(tree, "CTestTestfile.cmake")):
        sys.exit(f"oss-report: {tree} is not a kept build tree")
    row = projects.load()[a.project]
    cap = int(os.environ.get("KILN_OSS_VMEM_KB") or row["vmem_kb"]) * 1024
    tmp = os.path.join(tree, "test-tmp")
    os.makedirs(tmp, exist_ok=True)
    env = dict(os.environ, TMPDIR=tmp, TEST_TMPDIR=tmp)
    env.setdefault("MALLOC_ARENA_MAX", "4")  # as oss-build.sh's test run
    fifo = os.environ.get("HOD_OSS_JOBSERVER_FIFO") or "/run/hod/lanes.fifo"
    if stat.S_ISFIFO(os.stat(fifo).st_mode if os.path.exists(fifo) else 0):  # the real-project pool
        env["MAKEFLAGS"] = f"--jobserver-auth=fifo:{fifo}"

    def limit():
        resource.setrlimit(resource.RLIMIT_AS, (cap, cap))

    for i in range(a.times):
        p = subprocess.run(["ctest", "-R", f"^{a.test}$", "--output-on-failure"], cwd=tree, env=env,
                           preexec_fn=limit, capture_output=True, text=True)
        log = os.path.join(tree, f"rerun-{a.test}-{int(time.time())}-{i}.log")
        with open(log, "w", encoding="utf-8") as f:
            f.write(p.stdout + p.stderr)
        per = projects.test_results(log)[2]
        ok = per.get(a.test) == "Passed"
        with open(os.path.join(tree, "reruns.jsonl"), "a", encoding="utf-8") as f:
            f.write(json.dumps({"test": a.test, "passed": ok, "rc": p.returncode, "log": os.path.basename(log),
                                "arena": env.get("MALLOC_ARENA_MAX"),
                                "ts": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())}) + "\n")
        print(f"oss-report: {a.project} {a.flavour} {a.mode} {a.test}: run {i + 1}: {'passed' if ok else 'failed'}")


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--target", choices=sorted(PREFIX), default="aarch64")
    sub = ap.add_subparsers(dest="cmd", required=True)
    c = sub.add_parser("collect")
    c.add_argument("project")
    r = sub.add_parser("rerun")
    for n in ("project", "flavour", "mode", "test"):
        r.add_argument(n)
    r.add_argument("--times", type=int, default=2)
    for n in ("status", "census"):
        s = sub.add_parser(n)
        s.add_argument("--out")
        s.add_argument("--root", action="append", default=[],
                       help="a lane root to read (repeatable); default $HOD_OSS_ROOT or <build root>/oss")
        s.add_argument("--provenance", help="the provenance file: hod commits of frozen copies, kiln, notes")
        s.add_argument("--candidate", help="the release candidate's hod commit: rows at another hod are marked"
                       " (`-`: not chosen yet, every row is marked)")
    a = ap.parse_args()
    global TARGET, CANDIDATE
    TARGET = a.target
    if a.cmd == "collect":
        return cmd_collect(a)
    if a.cmd == "rerun":
        return cmd_rerun(a)
    CANDIDATE = a.candidate
    if a.provenance:
        load_provenance(a.provenance)
    if a.root:
        findings = os.environ.get("HOD_OSS_FINDINGS")
        reps = load_roots(a.root)
    else:
        oss, findings = roots()
        reps = load_reports(oss)
    nfind = target_findings(findings) if findings else None
    kiln = next((r.get("compiler_path") for r in reps.values()), "?")
    intro = [
        "Generated by `scripts/oss/oss-report.py status` from the lane's reports "
        "(docs/VALIDATION.md \"Real projects (L9)\"): each project built through its own "
        "build system with kiln at `-O0` (flavour `O0`) and `-O0 -g` (`O0g`), three ways: "
        "`llc` (kiln's LLVM back end, the control), `hod` (kiln with hod, falling back to llc "
        "where hod declines), `strict` (no fallback). `objects`: the objects kiln made in the "
        "hybrid build; `fell back`: offered to hod and declined or failed; `not offered`: "
        "kiln took the LLVM route itself (an `opt` trigger). Tests: passed/total (ctest). "
        "`revision`: the project's tag or commit; `hod`: the hod commit of the hod and strict builds; "
        "`kiln`: the kiln commit of the row's builds (the control's included)"
        + (f"; `at the candidate`: whether the hod and strict builds used the candidate's hod, "
           + ("which is not chosen yet" if CANDIDATE == "-" else f"`{CANDIDATE}`")
           + " (a control does not depend on hod)." if CANDIDATE else "."),
    ]
    hosts = sorted({r.get("host", "") for r in reps.values()} - {""})
    if hosts:
        intro += ["", f"Host: {', '.join(hosts)}; target: {TARGET}" + (
            " (cross-built, tests under qemu-x86_64)." if TARGET != "aarch64" else ".")]
    if a.cmd == "status":
        title = "Real projects through kiln and hod" + ("" if TARGET == "aarch64" else f", {TARGET}")
        write(page(reps, title, intro), a.out)
    else:
        title = "Real-project census (live)" if TARGET == "aarch64" else f"Real-project census, {TARGET} (live)"
        text = page(reps, title, intro + ["", f"kiln: {kiln}"], nfind)
        notes = os.path.join(nfind, "NOTES.md") if nfind else ""  # the lane's own remarks, by hand
        if notes and os.path.exists(notes):
            with open(notes, encoding="utf-8") as f:
                text += "\n" + f.read()
        write(text, a.out)


if __name__ == "__main__":
    main()
