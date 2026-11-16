#!/usr/bin/env python3
"""Helpers of hod's real-project lane (spec/harness.md "Real projects").

A port of kiln's scripts/oss/projects.py (kiln:spec/landed/harness/
WP-666.md): the same projects.toml format, the same `shell` and
`report-json` commands, reduced to the cmake and autotools rows the lane drives, and a
report that adds the lane's mode and flavour, hod's census of the build
and the project's per-test results.

  projects.py list [--json]              project names, one per line
  projects.py shell <project>            the project's table as bash assignments
  projects.py report-json <args...>      write report.json for one build tree
  projects.py emulate-tests <build dir> <emulator>...
                                         run ctest's x86-64 test programs under the emulator
  projects.py run-phase --phase P --timeout S --record F -- <command>...
                                         run one phase in its own session, kill what it leaves
"""

import argparse
import json
import os
import platform
import re
import shlex
import signal
import subprocess
import sys
import time
import tomllib

HERE = os.path.dirname(os.path.abspath(__file__))
TOML = os.path.join(HERE, "projects.toml")

REQUIRED = ("url", "tag", "build", "cxx_std", "test")
BUILDS = ("cmake", "autotools")
MODES = ("llc", "hod", "strict")
FLAVOURS = {"O0": "-O0 -g0", "O0g": "-O0 -g"}
# What a build is for (oss-build.sh --target): aarch64 native, x86_64
# cross-built and its tests run under qemu (WP-191).
TARGETS = ("aarch64", "x86_64")
VMEM_MAX_KB = 6000000
# The test step's cap may go higher, for a test that reserves address
# space it never touches (mold reserves 8 GiB per arena, up to ten).
TEST_VMEM_MAX_KB = 100000000
LINK_VMEM_MAX_KB = 16000000
CTEST_RE = re.compile(r"(\d+)% tests passed, (\d+) tests? failed out of (\d+)")
# One ctest result line: `  3/12 Test  #3: name ....   Passed    0.01 sec`,
# `***Failed`, `***Timeout`, `***Exception: SegFault`, `***Not Run`.
CTEST_LINE_RE = re.compile(r"^\s*\d+/\d+ Test\s+#\d+: (\S+) \.+\s*(\S.*?)\s+[\d.]+ sec\s*$")
# Catch2 v2's totals (duckdb's unittest, kiln's CATCH_RE), its all-passed
# line, and the rules around a failed test case's header; the list of the
# cases a row's test command writes (`unittest --list-test-names-only`).
CATCH_RE = re.compile(r"^test cases:\s*(\d+)\s*\|\s*(\d+) passed\s*\|\s*(\d+) failed")
CATCH_ALL_RE = re.compile(r"^All tests passed \(\d+ assertions? in (\d+) test cases?\)")
CATCH_RULE_RE = re.compile(r"^-{79}\s*$")
CATCH_DOTS_RE = re.compile(r"^\.{79}\s*$")
CATCH_LIST = "catch-tests.log"
CATCH_PROGRESS_RE = re.compile(r"^\[\d+/\d+\] \(\d+%\): (.+)$")
# The reason name of a hod refusal as a directory name (hod-shim's rule).
SAFE_RE = re.compile(r"[^A-Za-z0-9:_.+-]")


def load(path=TOML):
    with open(path, "rb") as f:
        data = tomllib.load(f)
    for name, p in data.items():
        missing = [k for k in REQUIRED if k not in p]
        if missing:
            raise SystemExit(f"{path}: [{name}] lacks {', '.join(missing)}")
        if p["build"] not in BUILDS:
            raise SystemExit(f"{path}: [{name}] build must be one of {BUILDS} (the lane drives cmake and autotools rows)")
        p.setdefault("configure", [])
        p.setdefault("build_targets", [])
        p.setdefault("c_compiler", "clang-19")
        p.setdefault("c", "clang")
        if p["c"] not in ("kiln", "clang"):
            raise SystemExit(f"{path}: [{name}] c must be \"kiln\" or \"clang\"")
        p.setdefault("submodules", False)
        p.setdefault("source_subdir", "")
        p.setdefault("notes", "")
        p.setdefault("deps", [])
        p.setdefault("install_configure", [])
        p.setdefault("drop_flags", [])
        p.setdefault("checkout", name)
        p.setdefault("test_timeout", 1800)
        p.setdefault("vmem_kb", 3000000)
        if not (isinstance(p["vmem_kb"], int) and 0 < p["vmem_kb"] <= VMEM_MAX_KB):
            raise SystemExit(f"{path}: [{name}] vmem_kb must be an integer in 1..{VMEM_MAX_KB}")
        p.setdefault("test_vmem_kb", p["vmem_kb"])
        if not (isinstance(p["test_vmem_kb"], int) and 0 < p["test_vmem_kb"] <= TEST_VMEM_MAX_KB):
            raise SystemExit(f"{path}: [{name}] test_vmem_kb must be an integer in 1..{TEST_VMEM_MAX_KB}")
        # A link's cap (kiln-cxx's KILN_OSS_LINK_VMEM_KB): a big program's
        # link maps every archive (duckdb's -O0 library is about 1 GB).
        p.setdefault("link_vmem_kb", p["vmem_kb"])
        if not (isinstance(p["link_vmem_kb"], int) and 0 < p["link_vmem_kb"] <= LINK_VMEM_MAX_KB):
            raise SystemExit(f"{path}: [{name}] link_vmem_kb must be an integer in 1..{LINK_VMEM_MAX_KB}")
        if not (isinstance(p["test_timeout"], int) and p["test_timeout"] > 0):
            raise SystemExit(f"{path}: [{name}] test_timeout must be a positive integer")
    for name, p in data.items():
        bad = [d for d in p["deps"] if d not in data]
        if bad:
            raise SystemExit(f"{path}: [{name}] deps must name rows of this file: {', '.join(bad)}")
    return data


def read_jsonl(path):
    """One JSON object per line; a torn line is skipped."""
    out = []
    try:
        with open(path, encoding="utf-8", errors="replace") as f:
            for line in f:
                line = line.strip()
                if not line:
                    continue
                try:
                    out.append(json.loads(line))
                except json.JSONDecodeError:
                    pass
    except FileNotFoundError:
        pass
    return out


def is_tu(rec):
    return rec.get("source") not in (None, "link", "none")


def summarize(records):
    """TU and link counts of the build phase (kiln's report.json `tus`, `links`)."""
    build = [r for r in records if r.get("phase") == "build"]
    tus = [r for r in build if is_tu(r)]
    links = [r for r in build if r.get("source") == "link"]
    by_kind = {}
    for r in tus:
        by_kind[r.get("kind", "other")] = by_kind.get(r.get("kind", "other"), 0) + 1
    by_lang = {}
    for r in tus:
        b = by_lang.setdefault(r.get("lang") or "c++", {"total": 0, "ok": 0, "failed": 0})
        b["total"] += 1
        b["ok" if r.get("kind") == "ok" else "failed"] += 1
    failed = [{"source": r.get("source"), "kind": r.get("kind"), "diag": r.get("diag", "")} for r in tus if r.get("kind") != "ok"]
    return {
        "tus": {
            "total": len(tus),
            "ok": by_kind.get("ok", 0),
            "failed": len(tus) - by_kind.get("ok", 0),
            "by_kind": dict(sorted(by_kind.items())),
        },
        "tus_by_lang": dict(sorted(by_lang.items())),
        "failed_tus": failed[:50],
        "links": {
            "total": len(links),
            "ok": sum(1 for r in links if r.get("kind") == "ok"),
            "failed": sum(1 for r in links if r.get("kind") != "ok"),
        },
        "tu_cpu_s": round(sum(r.get("cpu_s", 0) for r in tus), 1),
        "peak_tu_rss_kb": max((r.get("max_rss_kb", 0) for r in tus), default=0),
    }


def read_coverage(path):
    """hod's coverage log (spec/cli.md "Coverage log"): (outcome, reason,
    census reasons) per line, fields by position."""
    out = []
    try:
        with open(path, encoding="utf-8", errors="replace") as f:
            for line in f:
                fs = line.rstrip("\n").split("\t")
                if len(fs) < 8:
                    continue
                census = []
                if len(fs) >= 11 and fs[10] != "-":
                    census = [c.replace("\\,", ",") for c in re.split(r"(?<!\\),", fs[10])]
                out.append({"outcome": fs[0], "reason": fs[1], "output": fs[4], "bytes": int(fs[3]) if fs[3].isdigit() else -1, "us": int(fs[6]) if fs[6].isdigit() else 0, "census": census})
    except FileNotFoundError:
        pass
    return out


def census(build_dir):
    """hod's part of one build: what kiln offered it (kiln's statistics
    log), what it built, and every refusal by name with its functions
    (hod's coverage log and hod-shim's log)."""
    stats = [r for r in read_jsonl(os.path.join(build_dir, "kiln-stats.jsonl")) if r.get("output_kind") == "obj"]
    backends = {}
    not_offered = {}
    for r in stats:
        b = r.get("backend") or "none"
        backends[b] = backends.get(b, 0) + 1
        if b == "llvm":
            key = ",".join(r.get("triggers") or []) or "-"
            if r.get("opt_level") not in (None, "0"):
                key = f"-O{r.get('opt_level')}"
            not_offered[key] = not_offered.get(key, 0) + 1
    cov = read_coverage(os.path.join(build_dir, "coverage.log"))
    shim = read_jsonl(os.path.join(build_dir, "hod-shim.jsonl"))
    refusals = {}
    for r in shim:
        if r.get("rc") == 0:
            continue
        name = r.get("reason") or "unknown"
        e = refusals.setdefault(name, {"modules": 0, "first": 0, "functions": {}, "examples": []})
        e["modules"] += 1
        e["first"] += 1
        fn = r.get("function")
        if fn:
            e["functions"][fn] = e["functions"].get(fn, 0) + 1
        if len(e["examples"]) < 3:
            src = next((a for a in r.get("kiln_argv", []) if a.endswith((".cc", ".cpp", ".cxx", ".c", ".C"))), None)
            e["examples"].append({"source": src, "function": fn, "cwd": r.get("kiln_cwd")})
    # The census (HOD_CENSUS=1): every reason of each refused module.
    census_counts = {}
    for c in cov:
        if c["outcome"] == "hod":
            continue
        for name in set(c["census"]) | {c["reason"]}:
            if name and name != "-":
                census_counts[name] = census_counts.get(name, 0) + 1
    for name, n in census_counts.items():
        # A reason found only by the census (never a module's first) has
        # no entry yet: make it before reading it.
        e = refusals.setdefault(name, {"modules": 0, "first": 0, "functions": {}, "examples": []})
        e["modules"] = max(n, e["modules"])
    for e in refusals.values():
        e["functions"] = dict(sorted(e["functions"].items(), key=lambda kv: -kv[1])[:10])
    return {
        "objects": len(stats),
        "backends": dict(sorted(backends.items())),
        "not_offered": dict(sorted(not_offered.items())),
        "hod_runs": len(cov),
        "hod_built": sum(1 for c in cov if c["outcome"] == "hod"),
        "hod_refused": sum(1 for c in cov if c["outcome"] != "hod"),
        "hod_s": round(sum(c["us"] for c in cov) / 1e6, 1),
        "refusals": dict(sorted(refusals.items(), key=lambda kv: (-kv[1]["first"], -kv[1]["modules"], kv[0]))),
    }


def test_results(log_path):
    """(summary line, counts or None, {test: status}) of a ctest log."""
    try:
        with open(log_path, encoding="utf-8", errors="replace") as f:
            lines = [l.rstrip("\n") for l in f]
    except FileNotFoundError:
        return None, None, {}
    per = {}
    for line in lines:
        m = CTEST_LINE_RE.match(line)
        if m:
            per[m.group(1)] = m.group(2).lstrip("*").strip()
    for line in reversed(lines):
        m = CTEST_RE.search(line)
        if m:
            total, failed = int(m.group(3)), int(m.group(2))
            return line.strip(), {"passed": total - failed, "failed": failed, "total": total}, per
    catch = catch_results(lines, os.path.join(os.path.dirname(log_path), CATCH_LIST))
    if catch:
        return catch
    icu = icu_results(lines)
    if icu:
        return icu
    last = next((l.strip() for l in reversed(lines) if l.strip()), "")
    return last, None, per


def icu_results(lines):
    """ICU4C's `make check` (test/Makefile's check-recursive): the last
    `ALL TESTS SUMMARY:` block, `All tests OK: <suites>` or `ok: <suites>`
    then `===== ERRS: <suites>`, one status per test suite (intltest,
    cintltst, iotest, ...). None for another log."""
    for i in range(len(lines) - 1, -1, -1):
        if lines[i].strip() != "ALL TESTS SUMMARY:":
            continue
        per = {}
        for l in lines[i + 1:i + 3]:
            for prefix, status in (("All tests OK:", "Passed"), ("ok:", "Passed"), ("===== ERRS:", "Failed")):
                if l.startswith(prefix):
                    per.update({t: status for t in l[len(prefix):].split()})
        if not per:
            return None
        failed = sum(1 for s in per.values() if s != "Passed")
        return lines[i + 1].strip() + (f"; {lines[i + 2].strip()}" if failed else ""), \
            {"passed": len(per) - failed, "failed": failed, "total": len(per)}, per
    return None


def catch_results(lines, list_path):
    """A Catch2 (v2) console log (duckdb's unittest): (totals line, counts,
    {test case: status}) or None. A failed case's header is the dashed
    rule, its name, another rule, its location and a dotted rule; every
    case of `list_path` (the row's `--list-test-names-only` output, when
    the test command wrote one) not failed is `Passed`. A progress line
    ending in a carriage return is split there."""
    parts = []
    for p in (p for l in lines for p in l.split("\r")):
        # A failed case's rule printed after a progress line without its newline.
        if len(p) > 79 and p.endswith("-" * 79) and not CATCH_RULE_RE.match(p):
            parts += [p[:-79], "-" * 79]
        else:
            parts.append(p)
    totals = None
    for i in range(len(parts) - 1, -1, -1):
        m = CATCH_RE.match(parts[i].strip())
        if m:
            total, passed, failed = (int(x) for x in m.groups())
            totals = (parts[i].strip(), {"passed": passed, "failed": failed, "total": total})
            break
        m = CATCH_ALL_RE.match(parts[i].strip())
        if m:
            total = int(m.group(1))
            totals = (parts[i].strip(), {"passed": total, "failed": 0, "total": total})
            break
    if totals is None:
        return None
    failed = set()
    for i in range(len(parts) - 3):
        if CATCH_RULE_RE.match(parts[i]) and parts[i + 1].strip() and not CATCH_RULE_RE.match(parts[i + 1]):
            j = i + 1
            while j < len(parts) and not CATCH_RULE_RE.match(parts[j]):
                j += 1
            # The rule after the name (and its sections), then file:line and dots.
            if j + 2 < len(parts) and CATCH_DOTS_RE.match(parts[j + 2]):
                failed.add(parts[i + 1].strip())
    # The cases that ran: duckdb's progress lines (`[n/N] (p%): <case>`);
    # a case's own output (or the abort of the case running when the
    # program died) can be glued on after its name.
    ran = [m.group(1) for m in (CATCH_PROGRESS_RE.match(p) for p in parts) if m]
    ran_set = set(ran)
    per = {}
    try:
        with open(list_path, encoding="utf-8", errors="replace") as f:
            names = [l.strip() for l in f if l.strip()]
    except FileNotFoundError:
        names = []
    for t in names:
        if not ran or t in ran_set or any(r.startswith(t) for r in ran):
            per[t] = "Passed"
        else:
            per[t] = "Not Run"
    for t in failed:
        per[t] = "Failed"
    return totals[0], totals[1], per


# One ctest test whose command is a path (CTestTestfile.cmake, as CMake
# writes it): `add_test([=[name]=] "/path/prog" ...)`.
ADD_TEST_RE = re.compile(r'^(\s*add_test\((?:\[(=*)\[.*?\]\2\]|"[^"]*"|\S+)\s+)"([^"]+)"')


def is_x86_64_elf(path):
    """An ELF file for x86-64 (e_machine 62)."""
    try:
        with open(path, "rb") as f:
            h = f.read(20)
    except OSError:
        return False
    return len(h) == 20 and h[:4] == b"\x7fELF" and h[5] == 1 and int.from_bytes(h[18:20], "little") == 62


def cmd_emulate_tests(a):
    """A cross build's tests (WP-191): CMake puts the crosscompiling
    emulator before a test whose COMMAND names a target, but not before a
    path (`$<TARGET_FILE:t>`, as googletest's own tests are declared).
    Every CTestTestfile.cmake under the build dir gets the emulator before
    each test command that is an x86-64 program; a second run changes
    nothing."""
    emu = " ".join('"' + x.replace('"', '\\"') + '"' for x in a.emulator)
    n = 0
    for root, _dirs, files in os.walk(a.build_dir):
        if "CTestTestfile.cmake" not in files:
            continue
        path = os.path.join(root, "CTestTestfile.cmake")
        with open(path, encoding="utf-8", errors="replace") as f:
            lines = f.readlines()
        out = []
        for line in lines:
            m = ADD_TEST_RE.match(line)
            # A test of add_test's old signature (`add_test(name prog)`,
            # simdjson's) names its program relative to its directory.
            prog = m and os.path.join(root, m.group(3))
            if m and is_x86_64_elf(prog):
                line = f'{m.group(1)}{emu} "{prog}"{line[m.end():]}'
                n += 1
            elif m:
                # A script test running the program itself (doctest's
                # `cmake -DCOMMAND=<prog> <args> -P exec_test.cmake`,
                # glog's `-DLOGCLEANUP=<prog>`): the emulator goes first,
                # space-separated where the script splits a command line
                # itself (arguments follow the program, or doctest's
                # COMMAND), else as list items for
                # `execute_process(COMMAND ${VAR})`.
                def under(c):
                    nonlocal n
                    if not is_x86_64_elf(c.group(3)):
                        return c.group(0)
                    n += 1
                    sep = " " if c.group(2) == "COMMAND" or line[c.end():c.end() + 1] == " " else ";"
                    return f'{c.group(1)}{sep.join(a.emulator)}{sep}{c.group(3)}'
                line = SCRIPT_COMMAND_RE.sub(under, line)
            out.append(line)
        if out != lines:
            with open(path, "w", encoding="utf-8") as f:
                f.writelines(out)
    # ctest's pre- and post-test commands (CTestCustom.cmake, CLI11's
    # `informational`): one command line in a string, run as it stands.
    custom = os.path.join(a.build_dir, "CTestCustom.cmake")
    if os.path.isfile(custom):
        with open(custom, encoding="utf-8", errors="replace") as f:
            text = f.read()
        def under(m):
            nonlocal n
            if not is_x86_64_elf(m.group(2)):
                return m.group(0)
            n += 1
            return f'{m.group(1)}"{" ".join(a.emulator)} {m.group(2)}"'
        new = CUSTOM_TEST_RE.sub(under, text)
        if new != text:
            with open(custom, "w", encoding="utf-8") as f:
                f.write(new)
    print(f"projects.py: {n} test commands put under {a.emulator[0]}")


# `"-D<VAR>=/path/prog[ args]"`, an argument of a `cmake -P` test.
SCRIPT_COMMAND_RE = re.compile(r'("-D(\w+)=)([^" ;]+)')

# `set(CTEST_CUSTOM_PRE_TEST "/path/prog")` (or POST_TEST) in CTestCustom.cmake.
CUSTOM_TEST_RE = re.compile(r'(set\(\s*CTEST_CUSTOM_(?:PRE|POST)_TEST\s+)"([^" ]+)"')


# A phase's leftovers (WP-268; spec/harness.md "Real projects", "A phase's
# leftovers"). A phase (configure, build, test) runs as the leader of a
# session of its own; when the leader has exited (or was stopped at the
# phase's time limit), every process still in that session, or carrying the
# phase's tag in its environment (a program that left the session with
# setsid but kept its environment), is a leftover: it is logged by name,
# sent SIGTERM, and SIGKILL after a grace period, and recorded in the build
# tree's leftovers.jsonl. The leader stays unreaped (waitid WNOWAIT) until
# the sweep is over, so the session's number cannot be given to another
# process meanwhile: the sweep can never reach a process outside the phase.
PHASE_TAG = "HOD_OSS_PHASE_TAG"
GRACE_S = 10


def proc_info(pid):
    """(state, session, name, args, cpu seconds) of a live process, or None."""
    try:
        with open(f"/proc/{pid}/stat", "rb") as f:
            st = f.read().decode("utf-8", "replace")
        with open(f"/proc/{pid}/cmdline", "rb") as f:
            args = f.read().replace(b"\0", b" ").decode("utf-8", "replace").strip()
    except OSError:
        return None
    r = st.rfind(")")
    comm, fields = st[st.find("(") + 1:r], st[r + 2:].split()
    try:
        exe = os.path.basename(os.readlink(f"/proc/{pid}/exe")).removesuffix(" (deleted)")
    except OSError:
        exe = ""
    # The program's own name: the executable's, but for an interpreter or a
    # wrapper (bash, python, qemu, nice, timeout) the first argument that
    # is not an option names it better.
    name = exe or comm
    if args:
        argv = args.split()
        if "-c" in argv[1:2]:
            pass  # a shell's script: the shell's own name
        elif name in ("bash", "sh", "dash") or name.startswith(("python", "qemu-", "nice", "timeout", "env")):
            rest = [x for x in argv[1:] if not x.startswith("-")]
            if rest:
                name = f"{name} {os.path.basename(rest[0])}"
    cpu = (int(fields[11]) + int(fields[12])) / os.sysconf("SC_CLK_TCK")
    return fields[0], int(fields[3]), name, args, cpu


def tagged(pid, needle):
    try:
        with open(f"/proc/{pid}/environ", "rb") as f:
            return needle in b"\0" + f.read()
    except OSError:
        return False


def phase_members(sid, tag):
    """{pid: info} of the live (not zombie) processes of the phase: in its
    session, or carrying its tag."""
    needle = b"\0" + f"{PHASE_TAG}={tag}".encode() + b"\0"
    me, out = os.getpid(), {}
    for d in os.listdir("/proc"):
        if not d.isdigit() or int(d) in (me, sid):
            continue
        pid = int(d)
        info = proc_info(pid)
        if info is None or info[0] in ("Z", "X"):
            continue
        if info[1] == sid or tagged(pid, needle):
            out[pid] = info
    return out


def signal_member(pid, sid, tag, sig):
    """Signals pid if it is still a member of the phase. The pidfd pins the
    process: a pid reused between the scan and the signal fails the check."""
    try:
        fd = os.pidfd_open(pid)
    except OSError:
        return False
    try:
        info = proc_info(pid)
        needle = b"\0" + f"{PHASE_TAG}={tag}".encode() + b"\0"
        if info is None or not (info[1] == sid or tagged(pid, needle)):
            return False
        signal.pidfd_send_signal(fd, sig)
        return True
    except OSError:
        return False
    finally:
        os.close(fd)


def sweep(sid, tag):
    """Kills what the phase left running; [(pid, name, args, cpu, signal)]."""
    left = {}
    for _ in range(3):  # a leftover may start another before it dies
        found = {p: i for p, i in phase_members(sid, tag).items() if p not in left}
        if not found:
            break
        for pid, info in found.items():
            if signal_member(pid, sid, tag, signal.SIGTERM):
                left[pid] = [info, "TERM"]
        end = time.monotonic() + GRACE_S
        while time.monotonic() < end and any(p in left for p in phase_members(sid, tag)):
            time.sleep(0.25)
        for pid in phase_members(sid, tag):
            if pid in left and signal_member(pid, sid, tag, signal.SIGKILL):
                left[pid][1] = "KILL"
    return [(pid, i[2], i[3], i[4], s) for pid, (i, s) in sorted(left.items())]


def no_core_dumps():
    """A core size limit of exactly 1 byte, soft and hard, for the phase
    (WP-268). A death test's child crashes on purpose. When core_pattern
    names a file, 1 is below an ELF dump's minimum size and no file is
    written, as with 0. When core_pattern is a pipe the kernel ignores
    the limit except for 1: fs/coredump.c, `if (cprm.limit == 1) { ...
    coredump_report_failure("RLIMIT_CORE is set to 1, aborting core");
    goto fail_unlock; }`. So the host's handler is never started; with 0
    it would still be started for every crash."""
    import resource
    resource.setrlimit(resource.RLIMIT_CORE, (1, 1))


def cmd_run_phase(a):
    cmd = a.command[1:] if a.command[:1] == ["--"] else a.command
    if not cmd:
        sys.exit("projects.py run-phase: no command")
    tag = f"{a.phase}-{os.getpid()}-{time.time_ns()}-{os.urandom(4).hex()}"
    sys.stdout.flush()
    child = subprocess.Popen(cmd, env=dict(os.environ, **{PHASE_TAG: tag}), start_new_session=True,
                             preexec_fn=no_core_dumps)
    sid = child.pid  # the leader of its own session and process group
    stopped, reaped = [], []

    def killpg(sig):
        if reaped or sid <= 1:  # once reaped, the number may belong to another process
            return
        try:
            os.killpg(sid, sig)
        except ProcessLookupError:
            pass

    def forward(signum, _frame):  # an interrupt of the lane stops the phase too
        stopped.append(signum)
        killpg(signum)

    for s in (signal.SIGINT, signal.SIGTERM, signal.SIGHUP):
        signal.signal(s, forward)
    deadline = time.monotonic() + a.timeout if a.timeout > 0 else None
    timed_out, kill_at = False, None
    while os.waitid(os.P_PID, sid, os.WEXITED | os.WNOWAIT | os.WNOHANG) is None:
        now = time.monotonic()
        if deadline is not None and not timed_out and now >= deadline:
            timed_out, kill_at = True, now + GRACE_S
            print(f"oss-build: the {a.phase} phase reached its limit of {a.timeout} s: stopped", flush=True)
            killpg(signal.SIGTERM)
        elif kill_at is not None and now >= kill_at:
            killpg(signal.SIGKILL)
            kill_at = None
        time.sleep(0.25)
    left = sweep(sid, tag)  # the leader is a zombie still: its session number is held
    reaped.append(True)
    rc = child.wait()
    rc = 124 if timed_out else (128 - rc if rc < 0 else rc)  # as timeout(1) and bash report it
    for pid, name, args, cpu, sig in left:
        print(f"oss-build: left running after the {a.phase} phase: {name} (pid {pid}, {cpu:.1f} s CPU, killed with SIG{sig}): {args[:300]}")
    if left:
        print(f"oss-build: the {a.phase} phase left {len(left)} processes running: "
              + ", ".join(sorted({n for _, n, _, _, _ in left})))
    sys.stdout.flush()
    if a.record:
        with open(a.record, "a", encoding="utf-8") as f:
            f.write(json.dumps({"phase": a.phase, "rc": rc, "timed_out": timed_out, "count": len(left),
                                "names": sorted({n for _, n, _, _, _ in left}),
                                "processes": [{"pid": p, "name": n, "args": g[:300], "cpu_s": round(c, 1), "signal": s}
                                              for p, n, g, c, s in left]}) + "\n")
    if stopped:
        rc = 128 + stopped[0]
    sys.exit(rc)


def read_leftovers(build_dir):
    """The leftovers of a build's phases (leftovers.jsonl): None when no
    phase recorded any (an older tree), else {count, names, phases}."""
    recs = read_jsonl(os.path.join(build_dir, "leftovers.jsonl"))
    if not recs:
        return None
    phases = {r["phase"]: {"count": r["count"], "names": r["names"]} for r in recs if "phase" in r}
    return {"count": sum(p["count"] for p in phases.values()),
            "names": sorted({n for p in phases.values() for n in p["names"]}), "phases": phases}


def cmd_leftovers(a):
    """One line for the lane's log: `N (name, ...)` for a phase, or nothing."""
    for r in read_jsonl(os.path.join(a.build_dir, "leftovers.jsonl")):
        if r.get("phase") == a.phase and r.get("count"):
            print(f"{r['count']} left running, killed: {', '.join(r['names'])}")


def cmd_report_json(a):
    recs = read_jsonl(os.path.join(a.build_dir, "compile.jsonl"))
    s = summarize(recs)
    summary, counts, per = (None, None, {})
    if a.test_rc is not None:
        summary, counts, per = test_results(os.path.join(a.build_dir, "test.log"))
    p = load()[a.project]
    rep = {
        "schema": 1,
        "project": a.project,
        "mode": a.mode,
        "flavour": a.flavour,
        "flags": FLAVOURS[a.flavour],
        "host": platform.machine(),
        "target": a.target,
        "compiler_path": a.compiler_path,
        "hod_path": a.hod_path,
        "kiln": a.kiln,
        "url": p["url"],
        "tag": p["tag"],
        "cxx_std": p["cxx_std"],
        "c": p["c"],
        "date": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "configure_rc": a.configure_rc,
        "build_rc": a.build_rc,
        "test_rc": a.test_rc,
        "test_summary": summary,
        "tests": counts,
        "test_status": per,
        # What the phases left running and the lane killed (WP-268): a
        # project's own behaviour, the control's as much as hod's; never
        # a verdict.
        "leftovers": read_leftovers(a.build_dir),
        "wall_s": round(a.wall, 1),
        **s,
        "hod": census(a.build_dir) if a.mode != "llc" else None,
    }
    out = os.path.join(a.build_dir, "report.json")
    with open(out, "w", encoding="utf-8") as f:
        json.dump(rep, f, indent=1)
        f.write("\n")
    print(out)


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)
    l = sub.add_parser("list")
    l.add_argument("--json", action="store_true")
    s = sub.add_parser("shell")
    s.add_argument("project")
    e = sub.add_parser("emulate-tests")
    e.add_argument("build_dir")
    e.add_argument("emulator", nargs=argparse.REMAINDER)
    r = sub.add_parser("report-json")
    r.add_argument("--project", required=True)
    r.add_argument("--mode", choices=MODES, required=True)
    r.add_argument("--flavour", choices=sorted(FLAVOURS), required=True)
    r.add_argument("--target", choices=TARGETS, default="aarch64")
    r.add_argument("--compiler-path", required=True)
    r.add_argument("--hod-path", default="")
    r.add_argument("--kiln", default="")
    r.add_argument("--build-dir", required=True)
    r.add_argument("--configure-rc", type=int, required=True)
    r.add_argument("--build-rc", type=int)
    r.add_argument("--test-rc", type=int)
    r.add_argument("--wall", type=float, required=True)
    ph = sub.add_parser("run-phase")
    ph.add_argument("--phase", required=True)
    ph.add_argument("--timeout", type=int, default=0, help="seconds, 0 for none")
    ph.add_argument("--record", default="", help="append the phase's leftovers to this JSONL file")
    ph.add_argument("command", nargs=argparse.REMAINDER)
    lo = sub.add_parser("leftovers")
    lo.add_argument("build_dir")
    lo.add_argument("phase")
    a = ap.parse_args()

    if a.cmd == "list":
        names = list(load())
        print(json.dumps(names) if a.json else "\n".join(names))
    elif a.cmd == "shell":
        data = load()
        if a.project not in data:
            sys.exit(f"projects.py: unknown project '{a.project}' (see {TOML})")
        p = data[a.project]
        q = shlex.quote
        for k in ("url", "tag", "build", "cxx_std", "test", "c", "c_compiler", "source_subdir", "checkout", "test_timeout", "vmem_kb", "test_vmem_kb", "link_vmem_kb"):
            print(f"P_{k.upper()}={q(str(p[k]))}")
        print(f"P_SUBMODULES={'1' if p['submodules'] else ''}")
        print("P_CONFIGURE=(" + " ".join(q(x) for x in p["configure"]) + ")")
        print("P_BUILD_TARGETS=(" + " ".join(q(x) for x in p["build_targets"]) + ")")
        print("P_DEPS=(" + " ".join(q(x) for x in p["deps"]) + ")")
        print("P_INSTALL_CONFIGURE=(" + " ".join(q(x) for x in p["install_configure"]) + ")")
        print("P_DROP_FLAGS=(" + " ".join(q(x) for x in p["drop_flags"]) + ")")
    elif a.cmd == "emulate-tests":
        cmd_emulate_tests(a)
    elif a.cmd == "report-json":
        cmd_report_json(a)
    elif a.cmd == "run-phase":
        cmd_run_phase(a)
    elif a.cmd == "leftovers":
        cmd_leftovers(a)


if __name__ == "__main__":
    main()
