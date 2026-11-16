#!/usr/bin/env python3
"""check-workflows.py: the `workflows` step of `cargo xtask ci` (WP-265;
spec/harness.md "ci"). It parses every hosted workflow file and every
local action with a YAML parser and checks the structural facts a typo
breaks silently, which GitHub reports only when the workflow is next run:

  a workflow      a mapping with the keys `on` and `jobs`, `jobs` a
                  non-empty mapping; every job a mapping with `runs-on`
                  or `uses` (a reusable workflow); a job with `runs-on`
                  has a non-empty list of `steps`; every name `needs`
                  gives is a job of the same file; every step a mapping
                  with `run` or `uses`
  an action       a mapping with `runs`, which has `using`; a composite
                  action's every step has `run` or `uses`
  any file        parses; no mapping has a key twice

The YAML 1.1 trap: PyYAML's default loader reads the key `on` (and `off`,
`yes`, `no`) as a boolean. GitHub reads YAML 1.2, where only `true` and
`false` are booleans; the loader here does the same.

Usage: python3 -I scripts/check-workflows.py [FILE]...
  With no FILE: .github/workflows/*.yml and *.yaml and
  .github/actions/*/action.yml of the workspace. A FILE under a directory
  named `actions`, or named action.yml/action.yaml, is an action.
Exit status: 0 every file holds; 1 a file does not (one line per finding,
`<file>: <what>`); 3 the parser is missing (python3-yaml).
"""

import glob
import os
import re
import sys

try:
    import yaml
except ImportError:
    print("check-workflows: no YAML parser: python3 cannot import yaml "
          "(Debian: apt-get install python3-yaml)", file=sys.stderr)
    sys.exit(3)

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
BOOL = "tag:yaml.org,2002:bool"


class Loader(yaml.SafeLoader):
    """SafeLoader with YAML 1.2 booleans and duplicate keys refused."""

    def construct_mapping(self, node, deep=False):
        seen = set()
        for key_node, _ in node.value:
            key = self.construct_object(key_node, deep=True)
            if isinstance(key, (str, int, float, bool)) and key in seen:
                raise yaml.constructor.ConstructorError(
                    None, None, f"duplicate key {key!r}", key_node.start_mark)
            if isinstance(key, (str, int, float, bool)):
                seen.add(key)
        return super().construct_mapping(node, deep=deep)


Loader.yaml_implicit_resolvers = {
    first: [(tag, rx) for tag, rx in resolvers if tag != BOOL]
    for first, resolvers in yaml.SafeLoader.yaml_implicit_resolvers.items()
}
Loader.add_implicit_resolver(
    BOOL, re.compile(r"^(?:true|True|TRUE|false|False|FALSE)$"), list("tTfF"))


def check_steps(steps, where, problems):
    if not isinstance(steps, list) or not steps:
        problems.append(f"{where}: `steps` is not a non-empty list")
        return
    for i, step in enumerate(steps, 1):
        if not isinstance(step, dict):
            problems.append(f"{where}: step {i} is not a mapping")
        elif "run" not in step and "uses" not in step:
            name = step.get("name")
            label = f"step {i}" + (f" ({name})" if name else "")
            problems.append(f"{where}: {label} has neither `run` nor `uses`")


def check_workflow(doc, problems):
    if not isinstance(doc, dict):
        problems.append("not a mapping")
        return
    if True in doc:
        problems.append("a key read as the boolean true (`on` under YAML 1.1?)")
    if "on" not in doc:
        problems.append("no `on`")
    jobs = doc.get("jobs")
    if not isinstance(jobs, dict) or not jobs:
        problems.append("`jobs` is missing or not a non-empty mapping")
        return
    for name, job in jobs.items():
        where = f"job {name}"
        if not isinstance(job, dict):
            problems.append(f"{where}: not a mapping")
            continue
        if "runs-on" not in job and "uses" not in job:
            problems.append(f"{where}: neither `runs-on` nor `uses`")
        needs = job.get("needs", [])
        if isinstance(needs, str):
            needs = [needs]
        if not isinstance(needs, list):
            problems.append(f"{where}: `needs` is not a name or a list")
            needs = []
        for need in needs:
            if need not in jobs:
                problems.append(f"{where}: needs `{need}`, which is not a job")
            elif need == name:
                problems.append(f"{where}: needs itself")
        if "runs-on" in job and "uses" not in job:
            check_steps(job.get("steps"), where, problems)


def check_action(doc, problems):
    if not isinstance(doc, dict) or not isinstance(doc.get("runs"), dict):
        problems.append("no `runs` mapping")
        return
    runs = doc["runs"]
    if "using" not in runs:
        problems.append("`runs` has no `using`")
    if runs.get("using") == "composite":
        check_steps(runs.get("steps"), "runs", problems)


def is_action(path):
    parts = os.path.normpath(path).split(os.sep)
    return "actions" in parts[:-1] or parts[-1] in ("action.yml", "action.yaml")


def check_file(path):
    """The findings for one file, each `<path>: <what>`."""
    try:
        with open(path, encoding="utf-8") as f:
            doc = yaml.load(f, Loader=Loader)
    except (OSError, UnicodeDecodeError) as e:
        return [f"{path}: cannot read: {e}"]
    except yaml.YAMLError as e:
        text = " ".join(str(e).split())
        return [f"{path}: does not parse: {text}"]
    problems = []
    (check_action if is_action(path) else check_workflow)(doc, problems)
    return [f"{path}: {p}" for p in problems]


def default_files():
    files = []
    for pattern in (".github/workflows/*.yml", ".github/workflows/*.yaml",
                    ".github/actions/*/action.yml", ".github/actions/*/action.yaml"):
        files.extend(sorted(glob.glob(os.path.join(ROOT, pattern))))
    return files


def main(args):
    files = args or default_files()
    if not files:
        print("check-workflows: no workflow file found", file=sys.stderr)
        return 1
    problems = [p for path in files for p in check_file(path)]
    for p in problems:
        print(p)
    if problems:
        print(f"check-workflows: {len(problems)} finding(s) in {len(files)} file(s)",
              file=sys.stderr)
        return 1
    print(f"check-workflows: {len(files)} file(s) hold", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
