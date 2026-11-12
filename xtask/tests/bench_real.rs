//! `cargo xtask bench` end to end with the real tools: clang, LLVM's `llc`,
//! GNU `time` and the workspace's hod (`spec/harness.md` "bench").
//!
//! Tools-gated like every oracle test (`docs/VALIDATION.md` "Oracles"):
//! without the tools the test skips with a note; with `HOD_REQUIRE_TOOLS=1`
//! a missing tool is a failure.
#![cfg(target_os = "linux")]

mod common;

use std::path::{Path, PathBuf};

use common::Scratch;
use xtask::bench::measure::{GNU_TIME, KNOWN_ALLOCATION_KIB};
use xtask::bench::record::{Ratio, Results, Status, Tool};
use xtask::bench::{self, Config};
use xtask::irdiff::tools::{LLVM_MAJORS, find_llvm};
use xtask::irdiff::{self, Search};

const SOURCE: &str = "\
static int square(int x) { return x * x; }
int sum_of_squares(const int *values, int count) {
    int sum = 0;
    for (int i = 0; i < count; ++i) sum += square(values[i]);
    return sum;
}
int main(void) {
    int values[4] = {1, 2, 3, 4};
    return sum_of_squares(values, 4) == 30 ? 0 : 1;
}
";

/// This checkout's build directory: the parent of the directory cargo
/// gives integration tests for their files.
fn build_dir() -> PathBuf {
    Path::new(env!("CARGO_TARGET_TMPDIR")).parent().expect("it is <target>/tmp").to_path_buf()
}

// "Measurement", "The method's error", "Outcomes": one tiny input, produced
// by clang and measured with llc and with the workspace's hod. llc's
// record is a time, a peak memory, an object; hod's is whatever hod does
// with the module today, and with its fallback it costs what llc costs and
// a little more. The memory probe is within its stated error.
#[test]
fn one_input_end_to_end() {
    let scratch = Scratch::new("bench-real");
    let mut config = Config::new(scratch.join("ws"), scratch.join("build"));
    let llvm = find_llvm(None, &Search::from_env()).ok().flatten();
    let time = Path::new(GNU_TIME).is_file();
    if hod_testutil::tools_required() {
        assert!(
            llvm.as_ref().is_some_and(|llvm| llvm.major == LLVM_MAJORS[0]) && time,
            "required tools are missing (HOD_REQUIRE_TOOLS=1): LLVM {} with its clang, {GNU_TIME}",
            LLVM_MAJORS[0]
        );
        config.require_tools = true;
    }
    if llvm.is_none() {
        eprintln!("note: no llc found; skipping this tool-backed test");
        return;
    }
    std::fs::create_dir_all(scratch.join("ws/bench/src")).unwrap();
    std::fs::write(
        scratch.join("ws/bench/manifest"),
        "common = -O0 -S -emit-llvm\n[tiny]\nclass = small\nsource = tiny.c\nflags = -std=c17\n",
    )
    .unwrap();
    std::fs::write(scratch.join("ws/bench/src/tiny.c"), SOURCE).unwrap();
    // The development build of hod is enough to see what it does, and is
    // the one the other tests of this crate build.
    config.hod = Some(irdiff::build_workspace_hod(&build_dir()).expect("hod builds"));
    config.runs = 2;

    let run = bench::run(&config).expect("the run can be made");
    let summary = run.summary(true);
    if run.results.is_none() {
        assert!(!config.require_tools, "{summary}");
        eprintln!("note: skipping this tool-backed test:\n{summary}");
        return;
    }
    assert_eq!(run.exit_status(), 0, "{summary}");
    let results = run.results.as_ref().unwrap();
    assert_eq!(results.records.len(), 6, "{summary}");
    assert_eq!(&Results::read(run.results_path.as_ref().unwrap()).unwrap(), results);
    assert!(results.get("llc_version").unwrap().starts_with("LLVM version "), "{summary}");
    assert!(results.get("hod_version").unwrap().starts_with("hod version "), "{summary}");

    for input in ["tiny", "tiny-g"] {
        let llc = results.find(input, Tool::Llc).unwrap();
        assert_eq!((llc.status, llc.runs, llc.functions), (Status::Ok, 2, Some(3)), "{summary}");
        let wall = llc.wall_us_min.unwrap();
        assert!(wall > 500 && wall < 60_000_000, "{summary}");
        assert!(llc.object_bytes.unwrap() > 500 && llc.ir_bytes.unwrap() > 500, "{summary}");
        assert!(llc.ir_bytes_per_s().unwrap() > 0, "{summary}");

        // hod alone: a decline with its reason today, an object later.
        let hod = results.find(input, Tool::Hod).unwrap();
        match hod.status {
            Status::Declined => {
                assert!(!hod.detail.is_empty() && hod.object_bytes.is_none(), "{summary}");
                assert!(summary.contains(&format!("declined: {} (in ", hod.detail)), "{summary}");
            }
            Status::Ok => {
                assert!(hod.object_bytes.unwrap() > 0 && hod.wall_ratio.is_some(), "{summary}");
            }
            other => panic!("hod: {other:?}\n{summary}"),
        }
        // hod with its fallback always produces an object.
        let hybrid = results.find(input, Tool::HodFallback).unwrap();
        assert!(hybrid.produced() && hybrid.wall_ratio.is_some(), "{summary}");
        if hybrid.status == Status::Fallback {
            assert_eq!(hybrid.size_ratio, Ratio::of(1, 1), "llc's object, through hod\n{summary}");
            // spec/cli.md "Phase report": the front half's phases, as far
            // as the decline got, between `read` and `compile`.
            let phases: Vec<&str> = hybrid.phases.iter().map(|(name, _)| name.as_str()).collect();
            assert!(phases.len() >= 4 && phases[0] == "read", "{phases:?}");
            assert!(phases.ends_with(&["compile", "fallback", "total"]), "{phases:?}");
            let front = &phases[1..phases.len() - 3];
            assert!(["parse", "classify", "bodies"].starts_with(front), "{phases:?}");
        }

        if time {
            // llc is tens of MiB; hod's fallback run is llc's memory.
            let rss = llc.rss_kib.unwrap();
            assert!(rss > 8 << 10 && rss < 4 << 20, "{summary}");
            assert!(llc.cpu_us.is_some() && hybrid.rss_ratio.is_some(), "{summary}");
        }
    }
    if time {
        // "The method's error": within 1% of the known allocation.
        let error: i64 =
            results.get("rss_error_kib").expect("the probe was checked").parse().unwrap();
        assert!(error.abs() <= (KNOWN_ALLOCATION_KIB / 100) as i64, "{summary}");
        assert!(results.get("floor_us").unwrap().parse::<u64>().unwrap() > 0);
    }
}
