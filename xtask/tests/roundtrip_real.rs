//! `cargo xtask roundtrip` with the real tools (`spec/harness.md`
//! "roundtrip"): a corpus of two hand-made entries per installed LLVM
//! major, parsed, printed, assembled, verified, built by the reference
//! `llc`, linked by every linker found and run. Skips without the tools;
//! fails under `HOD_REQUIRE_TOOLS=1`.
#![cfg(target_os = "linux")]

mod common;

use std::fmt::Write as _;

use common::Scratch;
use xtask::corpus::manifest::{Manifest, Row};
use xtask::irdiff::tools::{self, LLVM_MAJORS, REQUIRED_LINKERS};
use xtask::irdiff::{Arch, Search};
use xtask::roundtrip::{Options, Outcome, run};

/// A program that prints through the C library, with an initialized
/// global, a loop and a call: the printer's normal form for each.
const MODULE: &str = r#"@greeting = private unnamed_addr constant [6 x i8] c"hello\00", align 1
@count = global i32 3, align 4

declare i32 @puts(ptr)

define internal i32 @twice(i32 %x) {
  %y = mul i32 %x, 2
  ret i32 %y
}

define i32 @main() {
entry:
  %n = load i32, ptr @count, align 4
  br label %loop
loop:
  %i = phi i32 [ 0, %entry ], [ %next, %loop ]
  %r = call i32 @puts(ptr @greeting)
  %next = add i32 %i, 1
  %done = icmp eq i32 %next, %n
  br i1 %done, label %exit, label %loop
exit:
  %status = call i32 @twice(i32 2)
  ret i32 %status
}
"#;

/// The two modules of an entry of several: `lib` defines what `test`'s
/// `main` calls.
const LIB: &str = r"@base = global i32 40, align 4

define i32 @plus(i32 %x) {
  %b = load i32, ptr @base, align 4
  %y = add i32 %x, %b
  ret i32 %y
}
";

const TEST: &str = r#"@text = private unnamed_addr constant [8 x i8] c"bye %d\0A\00", align 1

declare i32 @printf(ptr, ...)
declare i32 @plus(i32)

define i32 @main() {
  %n = call i32 @plus(i32 2)
  %r = call i32 (ptr, ...) @printf(ptr @text, i32 %n)
  ret i32 0
}
"#;

/// A `main` that writes `bye` without a newline, with `write`.
const NO_NEWLINE: &str = r#"@bye = private unnamed_addr constant [3 x i8] c"bye", align 1

declare i64 @write(i32, ptr, i64)

define i32 @main() {
  %w = call i64 @write(i32 1, ptr @bye, i64 3)
  ret i32 0
}
"#;

fn installed() -> Option<Vec<u32>> {
    let search = Search::from_env();
    let majors: Vec<u32> = LLVM_MAJORS
        .into_iter()
        .filter(|major| {
            matches!(tools::find_llvm(Some(&major.to_string()), &search), Ok(Some(_)))
                && search.versioned("llvm-as", *major).is_some()
                && search.versioned("opt", *major).is_some()
        })
        .collect();
    let driver = tools::find_driver(&search).is_some();
    let linkers: Vec<&str> = REQUIRED_LINKERS
        .into_iter()
        .filter(|name| tools::find_linker(name, &search).is_some())
        .collect();
    let found = format!("LLVM {majors:?}, gcc: {driver}, linkers {linkers:?}");
    if hod_testutil::tools_required() {
        assert!(
            majors == LLVM_MAJORS && driver && linkers == REQUIRED_LINKERS,
            "required tools are missing (HOD_REQUIRE_TOOLS=1); found {found}"
        );
    }
    if majors.is_empty() || !driver || linkers.is_empty() || Arch::host().is_none() {
        eprintln!("note: roundtrip's tools not found ({found}); skipping this tool-backed test");
        return None;
    }
    Some(majors)
}

// spec/harness.md "roundtrip", "What an entry goes through": the whole
// trip with LLVM's tools, a pass and a behaviour that differs.
#[test]
fn real_tools_round_trip() {
    let Some(majors) = installed() else { return };
    let arch = Arch::host().unwrap().name();
    let scratch = Scratch::new("roundtrip-real");
    let corpus = scratch.join("corpus");
    let mut manifest = Manifest::default();
    for major in &majors {
        let triple = format!("target triple = \"{arch}-unknown-linux-gnu\"\n");
        let head =
            |name: &str| format!(";@ covers: real {name}\n;@ target: {arch}\n;@ llvm: {major}\n");
        let mut entries = Vec::new();
        for (name, stdout) in [("hello", "hello"), ("differs", "goodbye")] {
            let mut text = head(name) + ";@ link: m\n";
            for _ in 0..3 {
                let _ = writeln!(text, ";@ stdout: {stdout}");
            }
            text.push_str(";@ exit: 4\n");
            let _ = write!(text, "{triple}{MODULE}");
            entries.push((name, text, "text"));
        }
        // Several modules, linked into one program.
        let text = head("modules")
            + &format!(
                ";@ stdout: bye 42\n;@ module: lib\n{triple}{LIB};@ module: test\n{triple}{TEST}"
            );
        entries.push(("modules", text, "text text"));
        // Directives the engine applies.
        let text = head("directives")
            + ";@ link: allow-warnings\n;@ llc-args: -function-sections -debugger-tune=gdb\n\
               ;@ stdout-no-newline: bye\n"
            + &triple
            + NO_NEWLINE;
        entries.push(("directives", text, "text"));
        for (name, text, delivered) in entries {
            let entry = format!("real/llvm{major}/{name}.ll");
            let path = corpus.join(&entry);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, text).unwrap();
            let row = Row {
                entry: entry.clone(),
                source: "real".to_owned(),
                llvm: *major,
                test: name.to_owned(),
                revision: "unknown".to_owned(),
                producer: "hand".to_owned(),
                producer_args: "-".to_owned(),
                llc_args: "-".to_owned(),
                delivered: delivered.to_owned(),
                sha256: "0".repeat(64),
                bytes: 0,
                key: "0".repeat(64),
            };
            manifest.rows.insert(entry, row);
        }
    }
    std::fs::create_dir_all(&corpus).unwrap();
    manifest.write(&corpus).unwrap();
    let mut options = Options::new(scratch.join("build"));
    options.corpus = corpus;
    options.expected = scratch.join("expected.tsv");
    options.jobs = 2;
    let report = run(&options).unwrap();
    assert_eq!(report.results.len(), 4 * majors.len());
    for result in &report.results {
        let want = if result.entry.ends_with("differs.ll") {
            Outcome::BehaviourDiffers
        } else {
            Outcome::Pass
        };
        assert_eq!(result.outcome, want, "{}: {}", result.entry, result.detail);
        if want == Outcome::BehaviourDiffers {
            assert!(result.detail.contains("differs from the directives"), "{}", result.detail);
        }
    }
    assert_eq!(report.exit_status(), 1, "{}", report.summary(true));
}
