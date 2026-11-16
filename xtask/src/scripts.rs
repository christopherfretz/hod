//! The gate's checks of the workspace's own scripts and hosted workflow
//! files (WP-265; `spec/harness.md` "ci"): the `scripts` step runs the
//! self-test of the real-project report and parses every shell script
//! under `scripts/` without running it; the `workflows` step parses every
//! file of `.github/workflows/` (and the local actions) with a YAML
//! parser and checks their structure (`scripts/check-workflows.py`).

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::ci::StepResult;
use crate::jobs::children::run_step;

/// The self-test of `scripts/oss/oss-report.py` (WP-263).
pub const SELFTEST: &str = "scripts/oss/oss-report-selftest.py";

/// The workflow checker the `workflows` step runs.
pub const CHECK_WORKFLOWS: &str = "scripts/check-workflows.py";

/// The checker's exit status when python3 cannot import `yaml`.
pub const NO_PARSER: i32 = 3;

/// The shell that parses a script, from its name and first line: `sh` for
/// a `#!/bin/sh` script (dash on Debian, which accepts what bash does not
/// always), `bash` for a `bash` shebang or a `.sh` file without one (a
/// file to `source`), `None` for any other file.
pub fn shell_of(path: &Path, first_line: &str) -> Option<&'static str> {
    if let Some(shebang) = first_line.strip_prefix("#!") {
        let mut words = shebang.split_whitespace();
        let program = words.next().unwrap_or("");
        let program = if program.ends_with("/env") { words.next().unwrap_or("") } else { program };
        return match program.rsplit('/').next() {
            Some("sh") => Some("sh"),
            Some("bash") => Some("bash"),
            _ => None,
        };
    }
    path.extension().is_some_and(|ext| ext == "sh").then_some("bash")
}

/// Every shell script under `dir`, sorted, with the shell that parses it.
pub fn shell_scripts(dir: &Path) -> Vec<(PathBuf, &'static str)> {
    let mut found = Vec::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(dir) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(kind) = entry.file_type() else { continue };
            if kind.is_dir() {
                pending.push(path);
            } else if kind.is_file() {
                let text = std::fs::read(&path).unwrap_or_default();
                let first = text.split(|b| *b == b'\n').next().unwrap_or(&[]);
                if let Some(shell) = shell_of(&path, &String::from_utf8_lossy(first)) {
                    found.push((path, shell));
                }
            }
        }
    }
    found.sort();
    found
}

/// A tool the step needs is absent: skipped with the reason, or a failure
/// under `HOD_REQUIRE_TOOLS=1`.
fn missing(step: &str, why: String, require_tools: bool) -> StepResult {
    if require_tools {
        eprintln!("{step}: error: {why} (HOD_REQUIRE_TOOLS=1)");
        StepResult::Failed
    } else {
        eprintln!("{step}: skipped: {why}; under HOD_REQUIRE_TOOLS=1 this fails");
        StepResult::Skipped(why)
    }
}

/// The `scripts` step: `python3 -I` [`SELFTEST`], then `sh -n`/`bash -n`
/// over every shell script under `<root>/scripts`.
pub fn scripts_gate(root: &Path, require_tools: bool) -> StepResult {
    eprintln!("ci: python3 -I {SELFTEST}");
    let mut selftest = Command::new("python3");
    selftest.arg("-I").arg(root.join(SELFTEST)).current_dir(root);
    match run_step(&mut selftest, true) {
        Ok((status, _)) if status.success() => {}
        Ok((status, _)) => {
            eprintln!("scripts: {SELFTEST}: {status}");
            return StepResult::Failed;
        }
        Err(e) => return missing("scripts", format!("cannot run python3: {e}"), require_tools),
    }
    let scripts = shell_scripts(&root.join("scripts"));
    let mut bad = 0;
    for (path, shell) in &scripts {
        let shown = path.strip_prefix(root).unwrap_or(path).display();
        match run_step(Command::new(shell).arg("-n").arg(path), false) {
            Ok((status, _)) if status.success() => {}
            Ok((status, _)) => {
                bad += 1;
                eprintln!("scripts: {shell} -n {shown}: {status}");
            }
            Err(e) => return missing("scripts", format!("cannot run {shell}: {e}"), require_tools),
        }
    }
    eprintln!("scripts: {} shell script(s) parsed, {bad} with an error", scripts.len());
    if bad == 0 { StepResult::Passed } else { StepResult::Failed }
}

/// The `workflows` step: [`CHECK_WORKFLOWS`] over `files`, or over the
/// workspace's workflow and action files when `files` is empty. Without
/// the YAML parser (exit status [`NO_PARSER`]) the step is skipped, or
/// fails under `HOD_REQUIRE_TOOLS=1`.
pub fn workflows_gate(root: &Path, files: &[PathBuf], require_tools: bool) -> StepResult {
    eprintln!("ci: python3 -I {CHECK_WORKFLOWS}");
    let mut check = Command::new("python3");
    check.arg("-I").arg(root.join(CHECK_WORKFLOWS)).args(files).current_dir(root);
    match run_step(&mut check, true) {
        Ok((status, _)) if status.success() => StepResult::Passed,
        Ok((status, _)) if status.code() == Some(NO_PARSER) => missing(
            "workflows",
            "no PyYAML: apt-get install python3-yaml".to_owned(),
            require_tools,
        ),
        Ok(_) => StepResult::Failed,
        Err(e) => missing("workflows", format!("cannot run python3: {e}"), require_tools),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shell_of_a_script() {
        let p = Path::new;
        assert_eq!(shell_of(p("a.sh"), "#!/bin/sh"), Some("sh"));
        assert_eq!(shell_of(p("a"), "#!/usr/bin/env bash"), Some("bash"));
        assert_eq!(shell_of(p("a.sh"), "#!/bin/bash -eu"), Some("bash"));
        assert_eq!(shell_of(p("env.sh"), "# source this"), Some("bash"));
        assert_eq!(shell_of(p("kiln-cc"), "#!/usr/bin/env python3"), None);
        assert_eq!(shell_of(p("a.sh"), "#!/usr/bin/env python3"), None);
        assert_eq!(shell_of(p("list.txt"), "a"), None);
    }

    // The workspace's scripts are found, each with its shebang's shell.
    #[test]
    fn the_workspaces_shell_scripts() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let scripts = shell_scripts(&root.join("scripts"));
        let named = |name: &str| scripts.iter().find(|(path, _)| path.ends_with(name)).map(|s| s.1);
        assert_eq!(named("scripts/env.sh"), Some("bash"));
        assert_eq!(named("scripts/jobserver.sh"), Some("bash"));
        assert_eq!(named("scripts/libcxx-hod-kiln.sh"), Some("sh"));
        assert_eq!(named("scripts/oss/llvm/build.sh"), Some("bash"));
        assert_eq!(named("scripts/oss/kiln-cc"), None);
        assert_eq!(named("scripts/mortar-commit.sh"), Some("bash"));
    }

    // WP-310: the hosted lanes' mortar action builds the commit this
    // script prints, which must be `MORTAR_COMMIT` (the one place it is
    // written); a checkout without the constant is an error, not a guess.
    #[cfg(unix)]
    #[test]
    fn the_mortar_commit_script_prints_the_pin() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let script = root.join("scripts/mortar-commit.sh");
        let run = |arg: &Path| {
            let mut command = Command::new("bash");
            command.arg(&script).arg(arg);
            hod_testutil::run_with(&mut command, b"", hod_testutil::DEFAULT_TIMEOUT)
        };
        let outcome = run(&root);
        assert!(outcome.success(), "{}", String::from_utf8_lossy(&outcome.stderr));
        let printed = String::from_utf8(outcome.stdout).unwrap();
        assert_eq!(printed, format!("{}\n", hod_testutil::linkers::MORTAR_COMMIT));
        let empty = root.join("xtask/src");
        let outcome = run(&empty);
        assert!(!outcome.success() && outcome.stdout.is_empty());
        assert!(String::from_utf8_lossy(&outcome.stderr).contains("no MORTAR_COMMIT"));
    }
}
