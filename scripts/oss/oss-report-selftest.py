#!/usr/bin/env python3
"""oss-report-selftest.py: the self-check of oss-report.py's list of
expected results (WP-263; spec/harness.md "Real projects", "Expected
results"). It writes synthetic lane roots (report.json and LastTest.log
per build) into a temporary directory, runs `oss-report.py check` and
`census` over each, and asserts the outcome:

  expected difference    the control passes, hod fails with the entry's
                         text: exit 0, TEST-EXPECTED.txt, no regression
  unexpected difference  a test no entry names, or the entry's text not in
                         the output: exit 1, TEST-REGRESSIONS.txt
  stale entry            the test passes under hod, or no build has it:
                         exit 1, TEST-STALE.txt
  control fails too      (the x86-64 lane under qemu) no difference: exit
                         0, nothing in the three files, the entry moot
  a malformed list       exit 2
  leftovers (WP-268)     projects.py run-phase kills what a phase leaves
                         running and records it, with a core limit of 1
                         byte; report-json carries it
                         as `leftovers`; the census shows it beside the
                         test counts and it changes no verdict

Needs only python3; no lane, no build. Run it as
`python3 -I scripts/oss/oss-report-selftest.py` (CI could run it in the
hosted lane's quick job or the oss workflow's summary job, before
`oss-report.py check`). Exit status 0 when every case holds.
"""

import json
import os
import shutil
import subprocess
import sys
import tempfile
import time

HERE = os.path.dirname(os.path.abspath(__file__))
REPORT = os.path.join(HERE, "oss-report.py")
PROJECTS = os.path.join(HERE, "projects.py")
TEST = "absl_symbolize_test"
TEXT = 'Which is: "inline_func"'
ENTRY = ["abseil", "x86_64", "*", "*", TEST, "fails", TEXT, "out-of-line always_inline call", 'spec/cli.md "Always-inline"']


def build(root, mode, status, output="", host="x86_64", target="aarch64", flavour="O0", leftovers=None):
    d = os.path.join(root, "build", "abseil", f"{flavour}-{mode}")
    os.makedirs(os.path.join(d, "Testing", "Temporary"))
    passed = sum(1 for s in status.values() if s == "Passed")
    r = {"schema": 1, "project": "abseil", "mode": mode, "flavour": flavour, "host": host, "target": target,
         "configure_rc": 0, "build_rc": 0, "test_rc": 0 if passed == len(status) else 8,
         "tests": {"passed": passed, "failed": len(status) - passed, "total": len(status)},
         "test_status": status, "tus": {"total": 1, "ok": 1, "failed": 0}, "leftovers": leftovers,
         "hod": None if mode == "llc" else {"objects": 1, "backends": {"hod": 1}, "not_offered": {}, "refusals": {}}}
    with open(os.path.join(d, "report.json"), "w", encoding="utf-8") as f:
        json.dump(r, f)
    with open(os.path.join(d, "Testing", "Temporary", "LastTest.log"), "w", encoding="utf-8") as f:
        for t, s in status.items():
            f.write(f"1/2 Testing: {t}\n1/2 Test: {t}\nOutput:\n{output if t == TEST else ''}\n"
                    f"Test {s}.\n\"{t}\" end time: Nov 15 12:44 UTC\n")


def run(root, entries, *cmd):
    lst = os.path.join(root, "expected.tsv")
    with open(lst, "w", encoding="utf-8") as f:
        f.write("# synthetic\n" + "".join("\t".join(e) + "\n" for e in entries))
    env = dict(os.environ, HOD_OSS_ROOT=root, HOD_OSS_FINDINGS=os.path.join(root, "findings"))
    p = subprocess.run([sys.executable, "-I", REPORT, "--expected", lst, *cmd], env=env, capture_output=True, text=True)
    return p.returncode, p.stdout + p.stderr


def files(root):
    out = {}
    for n in ("TEST-REGRESSIONS.txt", "TEST-EXPECTED.txt", "TEST-STALE.txt"):
        p = os.path.join(root, "findings", n)
        out[n] = open(p, encoding="utf-8").read() if os.path.exists(p) else None
    return out


FAILS = []


def check(name, cond, detail):
    print(f"{'ok  ' if cond else 'FAIL'} {name}")
    if not cond:
        FAILS.append(name)
        print("     " + detail.replace("\n", "\n     "))


def case(tmp, name, ctl, hod, entries, rc, regs, exp, stale, output=TEXT, **kw):
    root = os.path.join(tmp, name.replace(" ", "-"))
    build(root, "llc", ctl, **kw)
    build(root, "hod", hod, output, **kw)
    got, log = run(root, entries, "check")
    f = files(root)
    check(f"{name}: exit {rc}", got == rc, log)
    for key, want in (("TEST-REGRESSIONS.txt", regs), ("TEST-EXPECTED.txt", exp), ("TEST-STALE.txt", stale)):
        check(f"{name}: {key} {'names ' + want if want else 'empty'}",
              (want in (f[key] or "")) if want else f[key] == "", repr(f[key]))
    got, log = run(root, entries, "census")
    check(f"{name}: census", got == 0 and "## Expected results" in log, log[-2000:])
    return log


def gone(pid):
    """The process has exited (a zombie not yet reaped by its new parent counts)."""
    try:
        with open(f"/proc/{pid}/stat", encoding="utf-8") as f:
            return f.read().rsplit(")", 1)[1].split()[0] in ("Z", "X")
    except OSError:
        return True


def leftovers(tmp, ok):
    # A phase that leaves a process running: killed, recorded, the phase's
    # status kept (spec/harness.md "Real projects", "A phase's leftovers").
    d = os.path.join(tmp, "phase")
    os.makedirs(d)
    rec = os.path.join(d, "leftovers.jsonl")
    p = subprocess.run([sys.executable, "-I", PROJECTS, "run-phase", "--phase", "test", "--timeout", "0",
                        "--record", rec, "--", "sh", "-c", "sleep 600 & echo $! > pid; exit 3"],
                       cwd=d, capture_output=True, text=True)
    with open(os.path.join(d, "pid"), encoding="utf-8") as f:
        pid = int(f.read())
    for _ in range(40):  # the orphan's new parent reaps it
        if gone(pid):
            break
        time.sleep(0.05)
    recs = [json.loads(line) for line in open(rec, encoding="utf-8")]
    check("leftovers: run-phase keeps the phase's status", p.returncode == 3, p.stdout + p.stderr)
    check("leftovers: run-phase kills the leftover", gone(pid), f"pid {pid} still running")
    check("leftovers: run-phase records and logs it",
          len(recs) == 1 and recs[0]["count"] == 1 and recs[0]["names"] == ["sleep"]
          and "left 1 processes running: sleep" in p.stdout, repr(recs) + p.stdout)
    p = subprocess.run([sys.executable, "-I", PROJECTS, "run-phase", "--phase", "test", "--timeout", "1",
                        "--", "sh", "-c", "sleep 600"], cwd=d, capture_output=True, text=True)
    check("leftovers: the phase's limit gives 124", p.returncode == 124 and "reached its limit" in p.stdout,
          f"rc {p.returncode}: {p.stdout}")
    p = subprocess.run([sys.executable, "-I", PROJECTS, "run-phase", "--phase", "test", "--", sys.executable, "-I", "-c",
                        "import resource; print(resource.getrlimit(resource.RLIMIT_CORE))"],
                       cwd=d, capture_output=True, text=True)
    check("a phase's core limit is 1 byte, soft and hard", p.returncode == 0 and "(1, 1)" in p.stdout,
          p.stdout + p.stderr)
    p = subprocess.run([sys.executable, "-I", PROJECTS, "report-json", "--project", "abseil", "--mode", "llc",
                        "--flavour", "O0", "--compiler-path", "kiln++", "--build-dir", d, "--configure-rc", "0",
                        "--test-rc", "3", "--wall", "1"], capture_output=True, text=True)
    with open(os.path.join(d, "report.json"), encoding="utf-8") as f:
        left = json.load(f).get("leftovers")
    check("leftovers: report.json carries them", p.returncode == 0 and left is not None and left["count"] == 1
          and left["names"] == ["sleep"] and left["phases"]["test"]["count"] == 1, repr(left) + p.stderr)
    # The census shows a control's leftovers; they change no verdict.
    root = os.path.join(tmp, "leftovers")
    build(root, "llc", ok, leftovers={"count": 2, "names": ["absl_x_test"], "phases": {}})
    build(root, "hod", ok)
    got, log = run(root, [], "check")
    check("leftovers: no verdict (exit 0)", got == 0, log)
    got, log = run(root, [], "census")
    check("leftovers: the census shows them", "2 left running: absl_x_test" in log, log[-2000:])


def main():
    tmp = tempfile.mkdtemp(prefix="oss-report-selftest-")
    try:
        ok = {TEST: "Passed", "other": "Passed"}
        bad = {TEST: "Failed", "other": "Passed"}
        log = case(tmp, "expected difference", ok, bad, [ENTRY], 0, "", f"abseil O0 hod: {TEST}: expected", "")
        check("expected difference: the census says why", "expected: fails, the control passes" in log, log[-2000:])
        case(tmp, "unexpected difference", ok, {TEST: "Failed", "other": "Failed"}, [ENTRY], 1,
             "abseil O0 hod: other", f"abseil O0 hod: {TEST}: expected", "")
        case(tmp, "another failure", ok, bad, [ENTRY], 1, f"abseil O0 hod: {TEST} (an expected entry's text", "", "",
             output="Expected: (symbol) != (nullptr)")
        case(tmp, "no entry", ok, bad, [], 1, f"abseil O0 hod: {TEST}", "", "")
        case(tmp, "stale passes", ok, ok, [ENTRY], 1, "", "", f"abseil O0 hod: {TEST}: stale entry: passes under hod")
        case(tmp, "stale no such test", {"other": "Passed"}, {"other": "Passed"}, [ENTRY], 1, "", "",
             f"abseil O0 hod: {TEST}: stale entry: no such test")
        log = case(tmp, "control fails too", bad, bad, [ENTRY], 0, "", "", "", host="aarch64", target="x86_64")
        check("control fails too: moot in the census", "the control fails too (under emulation)" in log, log[-2000:])
        case(tmp, "other target", ok, bad, [ENTRY], 1, f"abseil O0 hod: {TEST}", "", "", host="aarch64")
        root = os.path.join(tmp, "malformed")
        build(root, "llc", ok)
        got, log = run(root, [ENTRY[:5] + ["passes"] + ENTRY[6:]], "check")
        check("a malformed list: exit 2", got == 2 and "outcome" in log, log)
        leftovers(tmp, ok)
    finally:
        shutil.rmtree(tmp)
    print(f"oss-report-selftest: {'FAILED: ' + ', '.join(FAILS) if FAILS else 'every case holds'}")
    return 1 if FAILS else 0


if __name__ == "__main__":
    sys.exit(main())
