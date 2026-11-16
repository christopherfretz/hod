//! `cargo xtask compare-binaries` (`spec/harness.md` "compare-binaries")
//! against two fake hods: shell scripts that write their arguments (but
//! the object's path) and their standard input to the object, so that
//! every class can be provoked without hod or a corpus.
//!
//! A global of the module named `@<what>` makes the fakes differ:
//! `@differ` (head writes another object), `@differ_pic` (head writes
//! another object under `-relocation-model=pic`), `@head_refuses` (head
//! exits 2), `@both_refuse` (both exit 2 with the same diagnostic),
//! `@refuse_name` (both exit 2, each naming another construct).
#![cfg(target_os = "linux")]

mod common;

use std::path::{Path, PathBuf};
use std::time::Duration;

use common::Scratch;
use hod_testutil::write_script;
use xtask::compare_binaries::{Class, Options, Report, run};
use xtask::irdiff::Limits;

fn fake(role: &str) -> String {
    format!(
        r#"PATH=/usr/bin:/bin
role={role}
if [ "$1" = --version ]; then echo "fake hod $role"; exit 0; fi
out= args=
while [ $# -gt 0 ]; do
  case "$1" in -o) out=$2; shift ;; *) args="$args $1" ;; esac
  shift
done
case " $args " in *" - "*) ;; *) echo "fake: no module on standard input" >&2; exit 3 ;; esac
in=$(cat)
case "$in" in *@both_refuse*) echo "hod: error: unsupported construct 'same'" >&2; exit 2 ;; esac
case "$in" in *@refuse_name*) echo "hod: error: unsupported construct '$role' in $out" >&2; exit 2 ;; esac
case "$role$in" in head*@head_refuses*) echo "hod: error: no" >&2; exit 2 ;; esac
{{
  echo "$args"
  printf '%s' "$in"
  case "$role$in" in head*@differ_pic*) case "$args" in *pic*) echo Y ;; esac ;; esac
  case "$role$in" in head*@differ\ *) echo X ;; esac
}} > "$out""#
    )
}

struct Setup {
    scratch: Scratch,
    base: PathBuf,
    head: PathBuf,
}

fn setup(label: &str) -> Setup {
    let scratch = Scratch::new(label);
    let base = scratch.join("hod-base");
    let head = scratch.join("hod-head");
    write_script(&base, &fake("base"));
    write_script(&head, &fake("head"));
    std::fs::create_dir_all(scratch.join("corpus/kiln/llvm22")).unwrap();
    Setup { scratch, base, head }
}

fn entry(dir: &Path, name: &str, global: &str) {
    let text = format!(
        ";@ target: aarch64\n;@ llvm: 22\ntarget triple = \"aarch64-unknown-linux-gnu\"\n\
         @{global} = global i32 0\ndefine i32 @main() {{\n  ret i32 0\n}}\n"
    );
    std::fs::write(dir.join(format!("{name}.ll")), text).unwrap();
}

fn options(s: &Setup) -> Options {
    let mut options = Options::new(&s.scratch.join("build"));
    options.base = Some(s.base.clone());
    options.head = Some(s.head.clone());
    options.corpora.push(s.scratch.join("corpus"));
    options.out = s.scratch.join("out");
    options.jobs = 2;
    options.limits = Limits { time: Duration::from_mins(1), memory_kib: 1 << 20 };
    options
}

fn classes(report: &Report, name: &str) -> Vec<(String, Class)> {
    report
        .outcomes
        .iter()
        .filter(|o| o.name.contains(name))
        .map(|o| (o.mode.clone(), o.class))
        .collect()
}

#[test]
fn every_class() {
    let s = setup("cmpbin-classes");
    let dir = s.scratch.join("corpus/kiln/llvm22");
    entry(&dir, "equal", "plain");
    entry(&dir, "differ", "differ");
    entry(&dir, "differ_pic", "differ_pic");
    entry(&dir, "head_refuses", "head_refuses");
    entry(&dir, "both_refuse", "both_refuse");
    entry(&dir, "refuse_name", "refuse_name");
    let report = run(&options(&s)).unwrap();
    let both = |class| vec![("plain".to_owned(), class), ("pic".to_owned(), class)];
    assert_eq!(classes(&report, "equal.ll"), both(Class::Identical));
    assert_eq!(classes(&report, "differ.ll"), both(Class::Different));
    assert_eq!(
        classes(&report, "differ_pic.ll"),
        vec![("plain".to_owned(), Class::Identical), ("pic".to_owned(), Class::Different)]
    );
    assert_eq!(classes(&report, "head_refuses.ll"), both(Class::ChangedOutcome));
    assert_eq!(classes(&report, "both_refuse.ll"), both(Class::IdenticalFailure));
    assert_eq!(classes(&report, "refuse_name.ll"), both(Class::ChangedOutcome));
    assert!(report.outcomes.iter().all(|o| o.target == "aarch64"));
    assert_eq!((report.modules(), report.outcomes.len()), (6, 12));
    assert_eq!(report.exit_status(), 1);
    // The object's path is not part of a diagnostic: the refusal that
    // names it is a changed outcome only for the construct's name.
    let renamed = report.outcomes.iter().find(|o| o.name.contains("refuse_name")).unwrap();
    assert!(renamed.detail.contains("'base' in <object>"), "{}", renamed.detail);
    let out = s.scratch.join("out");
    let differences = std::fs::read_to_string(out.join("differences.tsv")).unwrap();
    assert_eq!(differences.lines().count(), 1 + 2 + 1 + 2 + 2, "{differences}");
    let summary = std::fs::read_to_string(out.join("summary.md")).unwrap();
    assert!(summary.contains("fake hod base") && summary.contains("fake hod head"), "{summary}");
    assert!(summary.contains("| aarch64 | pic | 6 | 1 | 2 | 2 | 1 | 0 |"), "{summary}");
    assert!(summary.contains("6 modules, 12 comparisons"), "{summary}");
    assert!(!out.join("work").exists());
}

#[test]
fn equal_binaries_pass() {
    let s = setup("cmpbin-equal");
    let dir = s.scratch.join("corpus/kiln/llvm22");
    entry(&dir, "a", "a");
    entry(&dir, "b", "both_refuse");
    // An entry of two modules: each module is compiled on its own.
    let two = ";@ target: aarch64\n;@ module: one.ll\n@x = global i32 0\n\
               ;@ module: two.ll\n@y = global i32 0\n";
    std::fs::write(dir.join("two.ll"), two).unwrap();
    let mut options = options(&s);
    options.head = Some(s.base.clone());
    let report = run(&options).unwrap();
    assert_eq!(report.exit_status(), 0, "{}", report.summary());
    assert_eq!((report.modules(), report.outcomes.len()), (4, 8));
    assert_eq!(report.count(Class::IdenticalFailure), 2);
    assert!(report.outcomes.iter().any(|o| o.name.ends_with("two.ll#two.ll")));
}

// WP-292: a seed that names no target (no `;@ target:`, no triple) is
// compiled once per target hod compiles for, each with its `-mtriple`;
// one that names its target only for that target.
#[test]
fn a_seed_without_a_target_is_compared_per_target() {
    let s = setup("cmpbin-notarget");
    let dir = s.scratch.join("corpus/kiln/llvm22");
    std::fs::write(dir.join("any.ll"), "@any = global i32 0\n").unwrap();
    entry(&dir, "named", "named");
    let report = run(&options(&s)).unwrap();
    assert_eq!(report.exit_status(), 0, "{}", report.summary());
    let targets: Vec<&str> = xtask::irdiff::Arch::compiled().iter().map(|a| a.name()).collect();
    assert!(targets.len() >= 2, "{targets:?}");
    let mut any: Vec<(String, String)> = report
        .outcomes
        .iter()
        .filter(|o| o.name.ends_with("any.ll"))
        .map(|o| (o.target.clone(), o.mode.clone()))
        .collect();
    any.sort();
    let mut expected: Vec<(String, String)> = targets
        .iter()
        .flat_map(|t| ["pic", "plain"].map(|m| ((*t).to_owned(), m.to_owned())))
        .collect();
    expected.sort();
    assert_eq!(any, expected);
    assert!(report.outcomes.iter().all(|o| o.class == Class::Identical));
    let named: Vec<&str> = report
        .outcomes
        .iter()
        .filter(|o| o.name.ends_with("named.ll"))
        .map(|o| o.target.as_str())
        .collect();
    assert_eq!(named, ["aarch64", "aarch64"]);
    // Each object carries the arguments its fake was given: the triple.
    let units = xtask::compare_binaries::corpus_units(&dir.join("any.ll")).unwrap();
    for unit in &units {
        let arch = xtask::irdiff::Arch::from_name(&unit.target).unwrap();
        assert!(unit.args.contains(&format!("-mtriple={}", arch.triple())), "{:?}", unit.args);
    }
    assert_eq!(report.modules(), 2);
}

#[test]
fn an_empty_corpus_fails() {
    let s = setup("cmpbin-empty");
    let report = run(&options(&s)).unwrap();
    assert_eq!(report.exit_status(), 1);
    assert!(report.totals_line().contains("nothing was compared"), "{}", report.totals_line());
}

#[test]
fn a_binary_that_cannot_run_fails_loudly() {
    let s = setup("cmpbin-noexec");
    entry(&s.scratch.join("corpus/kiln/llvm22"), "a", "a");
    let broken = s.scratch.join("not-executable");
    std::fs::write(&broken, "#!/bin/sh\necho hod\n").unwrap();
    let mut options = options(&s);
    options.head = Some(broken);
    let error = run(&options).unwrap_err();
    assert!(error.contains("--version"), "{error}");
    options.head = Some(s.scratch.join("missing"));
    assert!(run(&options).is_err());
}

#[test]
fn kept_modules_replay_their_arguments() {
    let s = setup("cmpbin-kept");
    let kept = s.scratch.join("kept/aarch64/proj/O0");
    std::fs::create_dir_all(&kept).unwrap();
    let module = "target triple = \"x86_64-unknown-linux-gnu\"\n@differ_pic = global i32 0\n";
    std::fs::write(kept.join("ab12.ll"), module).unwrap();
    std::fs::write(kept.join("cd34.ll"), "@both_refuse = global i32 0\n").unwrap();
    std::fs::write(
        kept.join("MODULES.tsv"),
        "hash\tbytes\toutput\targs\n\
         ab12\t70\t/x/a.o\t--strict\t-O0\t-relocation-model=pic\t-o\t/x/a.o\t-\n\
         cd34\t28\t/x/b.o\t--strict\t-O0\t-o/x/b.o\t-\n",
    )
    .unwrap();
    let mut options = options(&s);
    options.corpora.clear();
    options.modules.push(s.scratch.join("kept"));
    let report = run(&options).unwrap();
    let got: Vec<(&str, &str, Class)> =
        report.outcomes.iter().map(|o| (o.target.as_str(), o.mode.as_str(), o.class)).collect();
    assert_eq!(
        got,
        vec![
            ("x86_64", "kept:pic", Class::Different),
            ("unknown", "kept:default", Class::IdenticalFailure)
        ]
    );
    // A manifest that does not parse is an error, not a silent skip.
    std::fs::write(kept.join("MODULES.tsv"), "zz\t1\n").unwrap();
    assert!(run(&options).is_err());
}
