//! The float-cast pre-run of `cargo xtask fuzz` (`spec/harness.md`
//! "Random programs", "Float casts"): a program that converts a float to
//! an integer type that cannot hold it is skipped as `ub-float-cast`; one
//! that does not is kept, whatever else the sanitizer would say of it.
//!
//! gcc with libubsan is required under `HOD_REQUIRE_TOOLS=1`; without
//! that variable a missing one skips these tests with a note.
#![cfg(target_os = "linux")]

use std::path::{Path, PathBuf};
use std::time::Duration;

use hod_testutil::Scratch;
use xtask::fuzz::case::{self, PreRun, UB_FLOAT_CAST};
use xtask::irdiff::{Limits, Search};

fn limits() -> (Limits, Limits) {
    let build = Limits { time: Duration::from_mins(2), memory_kib: 4 << 20 };
    let run = Limits { time: Duration::from_secs(5), memory_kib: 4 << 20 };
    (build, run)
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fuzz").join(name)
}

/// gcc with libubsan, or `None` with a note (a failure under
/// `HOD_REQUIRE_TOOLS=1`).
fn ubsan_cc(scratch: &Scratch) -> Option<PathBuf> {
    let found = Search::from_env()
        .on_path("gcc")
        .ok_or_else(|| "gcc not found".to_owned())
        .and_then(|cc| case::ubsan_works(&cc, scratch.path(), limits().0).map(|()| cc));
    match found {
        Ok(cc) => Some(cc),
        Err(why) => {
            let required = std::env::var_os("HOD_REQUIRE_TOOLS").is_some_and(|v| v == "1");
            assert!(!required, "the float-cast pre-run's tools: {why} (HOD_REQUIRE_TOOLS=1)");
            eprintln!("note: {why}; skipping the float-cast test");
            None
        }
    }
}

// "Float casts": the out-of-range conversion fires, with UBSan's line.
#[test]
fn an_out_of_range_float_cast_is_skipped() {
    let scratch = Scratch::new("fuzz-ubsan-cast");
    let Some(cc) = ubsan_cc(&scratch) else { return };
    let source = scratch.join("prog.c");
    std::fs::copy(fixture("ubsan-cast.c"), &source).unwrap();
    let mut log = Vec::new();
    let found = case::float_cast_prerun(&mut log, &cc, None, &source, scratch.path(), limits());
    let PreRun::Fired(line) = &found else { panic!("fired: {found:?}; {log:?}") };
    assert!(line.starts_with("prog.c:8:"), "the line, without the directory: {line}");
    assert!(line.contains("-8.4375") && line.contains("outside the range"), "{line}");
    assert_eq!(UB_FLOAT_CAST, "ub-float-cast");
    assert!(
        log.len() == 2 && log[0].contains("-fno-sanitize-recover=float-cast-overflow"),
        "{log:?}"
    );
    assert!(!scratch.join("ubsan-prerun").exists(), "the pre-run's program is removed");
}

// "Float casts": only the float-cast check skips; a signed overflow does
// not, but a finding's ubsan.txt reports it.
#[test]
fn other_undefined_behaviour_is_kept_and_only_labelled() {
    let scratch = Scratch::new("fuzz-ubsan-clean");
    let Some(cc) = ubsan_cc(&scratch) else { return };
    let source = scratch.join("prog.c");
    std::fs::copy(fixture("ubsan-clean.c"), &source).unwrap();
    let mut log = Vec::new();
    let found = case::float_cast_prerun(&mut log, &cc, None, &source, scratch.path(), limits());
    assert_eq!(found, PreRun::Clean, "{log:?}");
    let text = case::ubsan_triage(&cc, None, &source, scratch.path(), limits());
    assert!(text.contains("1 report(s)") && text.contains("signed integer overflow"), "{text}");
    let cast = scratch.join("cast.c");
    std::fs::copy(fixture("ubsan-cast.c"), &cast).unwrap();
    let text = case::ubsan_triage(&cc, None, &cast, scratch.path(), limits());
    assert!(text.contains("cast.c:8:") && text.contains("outside the range"), "{text}");
}

// "Float casts": a pre-run that does not build is not a skip.
#[test]
fn a_prerun_that_does_not_build_is_inconclusive() {
    let scratch = Scratch::new("fuzz-ubsan-broken");
    let Some(cc) = ubsan_cc(&scratch) else { return };
    let source = scratch.join("prog.c");
    std::fs::write(&source, "int main(void) { return }\n").unwrap();
    let mut log = Vec::new();
    let found = case::float_cast_prerun(&mut log, &cc, None, &source, scratch.path(), limits());
    assert!(matches!(found, PreRun::Inconclusive(_)), "{found:?}");
}
