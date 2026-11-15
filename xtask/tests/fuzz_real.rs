//! `cargo xtask fuzz` end to end with the real tools: csmith, clang,
//! `llc`, `llvm-extract`, the linker driver (`spec/harness.md` "Random
//! programs").
//!
//! csmith is not among the tools `HOD_REQUIRE_TOOLS=1` requires (hosted
//! CI does not install it): without it, or without clang, `llc` and gcc
//! of LLVM 22, these tests skip with a note whatever that variable says.
//! The tests of the ABI generator (`--profile abi`) need no csmith.
#![cfg(target_os = "linux")]

use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Duration, SystemTime};

use hod_testutil::Scratch;
use xtask::fuzz::case::{Kind, Mode};
use xtask::fuzz::profile::Profile;
use xtask::fuzz::{Options, run};
use xtask::irdiff::{self, Arch, Search, directives};

/// Whether the tools are here; a note when they are not.
fn tools_found() -> bool {
    found(&["csmith", "clang-22", "llc-22", "gcc"])
}

/// Whether `tools` (a name, or `<name>-<major>`) are here.
fn found(tools: &[&str]) -> bool {
    let search = Search::from_env();
    let missing: Vec<&str> = tools
        .iter()
        .map(|&tool| {
            let here = match tool.rsplit_once('-').and_then(|(n, m)| Some((n, m.parse().ok()?))) {
                Some((name, major)) => search.versioned(name, major).is_some(),
                None => search.on_path(tool).is_some(),
            };
            (tool, here)
        })
        .filter_map(|(name, found)| (!found).then_some(name))
        .collect();
    if !missing.is_empty() || Arch::host().is_none() {
        eprintln!("note: {missing:?} not found (or no supported host); skipping the fuzz test");
        return false;
    }
    true
}

/// csmith writes `platform.info` into its current directory: every csmith
/// child runs in its seed's directory under the run's output directory,
/// so the run writes none in the crate's directory (a test's) or the
/// workspace's. A file written there before `since` (by csmith run by
/// hand) is not the run's: a note, not a failure.
/// When a test started, less two seconds (a file system's coarse times).
fn started() -> SystemTime {
    SystemTime::now() - Duration::from_secs(2)
}

fn no_platform_info(since: SystemTime) {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    for dir in [crate_dir, crate_dir.parent().expect("the workspace")] {
        let file = dir.join("platform.info");
        let Ok(modified) = file.metadata().and_then(|m| m.modified()) else { continue };
        assert!(modified < since, "csmith wrote {} during the run", file.display());
        eprintln!("note: a stray {} older than the run (not the run's)", file.display());
    }
}

fn build_dir() -> PathBuf {
    Path::new(env!("CARGO_TARGET_TMPDIR")).parent().expect("it is <target>/tmp").to_path_buf()
}

fn workspace_hod() -> PathBuf {
    static HOD: OnceLock<PathBuf> = OnceLock::new();
    HOD.get_or_init(|| irdiff::build_workspace_hod(&build_dir()).expect("hod builds")).clone()
}

fn options(scratch: &Scratch, seed: u64, count: u64) -> Options {
    let mut options = Options::new(scratch.join("build"));
    options.out = scratch.join("fuzz");
    (options.seed, options.count, options.jobs) = (seed, count, 2);
    options.hod = Some(workspace_hod());
    options
}

// "What a seed does": a few seeds on the host; hod agrees with the oracle or declines
// them, and nothing is wrong on the oracle's side.
#[test]
fn seeds_on_the_host() {
    if !tools_found() {
        return;
    }
    let since = started();
    let scratch = Scratch::new("fuzz-host");
    let summary = run(&options(&scratch, 1, 3)).expect("the run");
    let text = summary.text();
    assert_eq!(summary.seeds, 3, "{text}");
    assert!(summary.findings.is_empty() && summary.errors.is_empty(), "{text}");
    assert!(summary.agree + summary.refused.values().sum::<usize>() >= 1, "{text}");
    let results = std::fs::read_to_string(summary.results_file.as_ref().unwrap()).unwrap();
    assert_eq!(results.lines().count(), 4, "{results}");
    // The work directory is gone; no finding was saved.
    let left: Vec<_> = std::fs::read_dir(scratch.join("fuzz")).unwrap().flatten().collect();
    assert_eq!(left.len(), 1, "{left:?}");
    no_platform_info(since);
}

// "Float casts": csmith 2.3.0's seed 101105 (c-float) converts -8.4375 to
// a uint16_t; the budget at df415453 saved it as a finding (LLVM's
// selectors differ on it: GlobalISel as hod, SelectionDAG as the oracle).
// The pre-run skips it with the counted reason and keeps UBSan's line.
#[test]
fn a_float_cast_seed_is_skipped() {
    if !tools_found() {
        return;
    }
    let scratch = Scratch::new("fuzz-float-cast");
    let summary = run(&options(&scratch, 101_105, 1)).expect("the run");
    let text = summary.text();
    assert_eq!(summary.skipped.get("ub-float-cast"), Some(&1), "{text}");
    assert!(summary.findings.is_empty(), "{text}");
    let results = std::fs::read_to_string(summary.results_file.as_ref().unwrap()).unwrap();
    let line = results.lines().find(|l| l.starts_with("101105\t")).expect("the seed's line");
    assert!(line.contains("skipped\tub-float-cast: prog.c:"), "{line}");
    assert!(line.contains("-8.4375 is outside the range"), "{line}");
}

// "Findings", "Reduction": a hod that miscompiles (every `xor` is built as
// an `or`) gives a finding, saved with its commands and outputs, which
// the witness blames on hod and which is reduced to a seed test.
#[test]
fn a_wrong_hod_is_a_finding() {
    if !tools_found() {
        return;
    }
    let scratch = Scratch::new("fuzz-wrong");
    let wrong = scratch.write_script(
        "wrong-hod",
        &format!(
            "for last; do :; done\n\
             n=$# i=0\n\
             for a do i=$((i+1)); [ $i -eq 1 ] && set --; \
             if [ $i -lt $n ]; then set -- \"$@\" \"$a\"; fi; done\n\
             sed 's/ = xor / = or /' \"$last\" > \"$last.wrong.ll\"\n\
             exec '{}' \"$@\" \"$last.wrong.ll\"",
            workspace_hod().display()
        ),
    );
    let mut options = options(&scratch, 3, 1);
    options.hod = Some(wrong);
    options.reduce_budget = 40;
    let summary = run(&options).expect("the run");
    let text = summary.text();
    assert_eq!(summary.exit_status(), 1, "{text}");
    let [found] = &summary.findings[..] else { panic!("one finding: {text}") };
    assert_eq!(found.kind, Kind::Disagree, "{text}");
    assert!(found.witness.contains("hod is suspect"), "{text}");
    for file in ["prog.c", "prog.ll", "commands.sh", "oracle.stdout", "hod.stdout", "seed.ll"] {
        assert!(found.dir.join(file).is_file(), "{file} in {}", found.dir.display());
    }
    let (before, after, probes) = found.reduced.expect("reduced");
    assert!(after * 2 < before && probes <= 40, "{text}");
    // The seed test's directives parse, expect the failure on this
    // target, and name no path of the run.
    let seed = std::fs::read_to_string(found.dir.join("seed.ll")).unwrap();
    let expect = directives::parse(&seed).expect("the seed's directives");
    let host = Arch::host().unwrap();
    assert!(expect.xfail_reason(host, 22, None).is_some(), "{seed}");
    assert!(!expect.stdout.is_empty(), "{seed}");
    assert!(!seed.contains(&*scratch.join("").to_string_lossy()), "{seed}");
}

// "Mix": a few csmith seeds split in two, each half built by hod with the
// other built by llc; and the same seeds with -g ("Debug"): no finding,
// nothing wrong on the oracle's side, and the line comparison ran.
#[test]
fn mixed_and_debug_seeds_on_the_host() {
    if !tools_found() || !found(&["llvm-extract-22", "llvm-dwarfdump-22"]) {
        return;
    }
    let since = started();
    let scratch = Scratch::new("fuzz-modes");
    for mode in [Mode::Mix, Mode::Debug] {
        let mut options = options(&scratch, 1, 3);
        options.mode = mode;
        let summary = run(&options).expect("the run");
        let text = summary.text();
        assert_eq!(summary.seeds, 3, "{text}");
        assert!(summary.findings.is_empty() && summary.errors.is_empty(), "{text}");
        if mode == Mode::Debug && summary.agree > 0 {
            assert!(summary.lines.as_ref().is_some_and(|l| l.same > 0), "{text}");
        }
    }
    no_platform_info(since);
}

// Harness gap (debug comparison, not hod): `llc -O0` deletes the store of
// csmith's `(*p) = (*p);`, so hod's object alone has a row at the line,
// and `debug::permitted` has no class for it
// (xtask/src/debug/permitted.rs:165 `pair_class`; spec/dwarf.md §5.7,
// "Permitted differences"). `--debug` takes such pairs apart
// (`case::self_assignment`); this passes once the comparison classifies
// them itself and none is left to take apart.
#[test]
#[ignore = "harness gap: no permitted class for a self-assignment's store, xtask/src/debug/permitted.rs:165"]
fn self_assignments_are_a_permitted_class() {
    if !tools_found() || !found(&["llvm-dwarfdump-22"]) {
        return;
    }
    let scratch = Scratch::new("fuzz-self-assign");
    let mut options = options(&scratch, 2, 1);
    options.mode = Mode::Debug;
    let summary = run(&options).expect("the run");
    assert_eq!(summary.self_assigned, 0, "{}", summary.text());
}

// Harness gap (debug comparison, not hod): at csmith's `if ((x, -6L))`,
// a branch on a constant to the next block, hod emits nothing (it folds
// the branch, spec/codegen.md §6.2) and `llc -O0` a test and a jump, so
// llc's object alone has a row at the line; `debug::permitted` has no
// class for it (xtask/src/debug/permitted.rs:165 `pair_class`). `--debug`
// takes such pairs apart (`case::constant_branch`); this passes once the
// comparison classifies them itself. Seed 231 on x86-64 has one.
#[test]
#[ignore = "harness gap: no permitted class for a folded constant branch, xtask/src/debug/permitted.rs:165"]
fn constant_branches_are_a_permitted_class() {
    if !found(&["csmith", "clang-22", "llc-22", "x86_64-linux-gnu-gcc", "llvm-dwarfdump-22"]) {
        return;
    }
    let scratch = Scratch::new("fuzz-constant-branch");
    let mut options = options(&scratch, 231, 1);
    options.mode = Mode::Debug;
    options.target = Some(Arch::X86_64);
    let summary = run(&options).expect("the run");
    assert_eq!(summary.constant_branches, 0, "{}", summary.text());
}

// "Set apart" (`selector-split`): seeds 110043 (`l_419 |= ((&l_31 == &l_31)
// & (-4L));`, prog.c:258) and 110164 (`g_31 |= (4294967294UL == (p_22 !=
// g_29));`, prog.c:425) of the 1.0 budget (aarch64, LLVM 22, debug):
// SelectionDAG folds the statement away and has no row at the line;
// FastISel and GlobalISel keep rows, as hod does. The pair is set apart
// with its evidence, and the seed is no finding.
#[test]
fn selector_splits_are_set_apart() {
    let cross = Arch::host() == Some(Arch::Aarch64) || found(&["aarch64-linux-gnu-gcc"]);
    if !cross || !tools_found() || !found(&["llvm-dwarfdump-22", "llvm-nm-22", "llvm-objdump-22"]) {
        return;
    }
    let scratch = Scratch::new("fuzz-selector-split");
    for (seed, function, line, counts) in
        [(110_043, "func_1", 258, (1, 1, 1)), (110_164, "func_19", 425, (1, 1, 1))]
    {
        let mut options = options(&scratch, seed, 1);
        options.mode = Mode::Debug;
        options.target = Some(Arch::Aarch64);
        let summary = run(&options).expect("the run");
        let text = summary.text();
        assert!(summary.findings.is_empty() && summary.errors.is_empty(), "{text}");
        let split = summary
            .selector_splits
            .iter()
            .find(|(s, split)| *s == seed && split.pair.0 == function && split.pair.2 == line)
            .map(|(_, split)| (split.hod, split.fast_isel, split.global_isel));
        assert_eq!(split, Some(counts), "{text}");
        let evidence = format!(
            "selector-split: seed {seed} {function} at prog.c:{line}: rows hod 1 selection-dag 0 \
             fast-isel 1 global-isel 1"
        );
        assert!(text.contains(&evidence), "{text}");
    }
}

/// A hod that swaps the first two parameters of every `callee_*` of the
/// ABI generator's programs in its definitions, declarations and calls
/// alike (`fuzz/swap-params.awk`): a module agrees with itself, an object
/// of llc's does not.
fn swapping_hod(scratch: &Scratch) -> PathBuf {
    let awk = scratch.write("swap-params.awk", include_str!("fuzz/swap-params.awk"));
    scratch.write_script(
        "swapping-hod",
        &format!(
            "for last; do :; done\n\
             n=$# i=0\n\
             for a do i=$((i+1)); [ $i -eq 1 ] && set --; \
             if [ $i -lt $n ]; then set -- \"$@\" \"$a\"; fi; done\n\
             awk -f '{}' \"$last\" > \"$last.wrong.ll\" || exit 1\n\
             exec '{}' \"$@\" \"$last.wrong.ll\"",
            awk.display(),
            workspace_hod().display()
        ),
    )
}

// "Mix", "Reduction": the planted ABI bug is invisible to the whole-module
// comparison and a finding of --mix, blamed on hod by both witnesses,
// reduced with the split kept, and saved as a seed of two modules.
#[test]
fn a_wrong_calling_convention_is_a_mix_finding() {
    if !found(&["clang-22", "llc-22", "llvm-extract-22", "gcc", "awk"]) || Arch::host().is_none() {
        return;
    }
    let scratch = Scratch::new("fuzz-mix-wrong");
    let mut options = options(&scratch, 2, 1);
    options.profile = Profile::by_name("abi").unwrap();
    options.hod = Some(swapping_hod(&scratch));
    options.reduce_budget = 40;
    let whole = run(&options).expect("the whole run");
    assert!(whole.findings.is_empty() && whole.agree == 1, "{}", whole.text());
    options.mode = Mode::Mix;
    let summary = run(&options).expect("the mixed run");
    let text = summary.text();
    assert_eq!(summary.exit_status(), 1, "{text}");
    let [found] = &summary.findings[..] else { panic!("one finding: {text}") };
    assert_eq!(found.kind, Kind::Disagree, "{text}");
    assert!(
        found.detail.contains("hod A + llc B") || found.detail.contains("llc A + hod B"),
        "{text}"
    );
    assert!(found.witness.contains("hod is suspect"), "{text}");
    assert!(found.witness.contains("disagree between objects"), "{text}");
    let (before, after, probes) = found.reduced.expect("reduced");
    assert!(after * 4 < before && probes <= 40, "{text}");
    let seed = std::fs::read_to_string(found.dir.join("seed.ll")).unwrap();
    let expect = directives::parse(&seed).expect("the seed's directives");
    assert!(expect.xfail_reason(Arch::host().unwrap(), 22, None).is_some(), "{seed}");
    assert_eq!(seed.matches("\n;@ module: ").count(), 2, "{seed}");
    assert!(!seed.contains(&*scratch.join("").to_string_lossy()), "{seed}");
}

/// The tools of the `llc19-int128-last-register` tests: the abi profile
/// at LLVM 19 on x86-64, `--mix`, and `llc` 22.
fn int128_tools_found() -> bool {
    let cross = Arch::host() == Some(Arch::X86_64) || found(&["x86_64-linux-gnu-gcc"]);
    cross && found(&["clang-19", "llc-19", "llvm-extract-19", "llc-22", "awk"])
}

fn int128_options(scratch: &Scratch, seed: u64, llvm: u32, mode: Mode) -> Options {
    let mut options = options(scratch, seed, 1);
    options.profile = Profile::by_name("abi").unwrap();
    (options.target, options.llvm, options.mode) = (Some(Arch::X86_64), llvm, mode);
    options.reduce_budget = 0;
    options
}

// "Set apart (seeds)" (`llc19-int128-last-register`): seeds 125392 (whole)
// and 126005 (--mix) of the 1.0 budget (x86-64, LLVM 19, abi): an i128
// argument with only r9 left, which llc 19 also takes r9 away from. gcc
// on prog.c and llc 22 on the module print hod's output (in --mix: hod's
// half with llc 22's other half), so the seed is set apart, no finding.
#[test]
fn llc19_int128_findings_are_set_apart() {
    if !int128_tools_found() {
        return;
    }
    let scratch = Scratch::new("fuzz-int128");
    for (seed, mode, site) in [
        (125_392, Mode::Whole, "i128 at call @callee_2 argument 10"),
        (126_005, Mode::Mix, "i128 at define @callee_2 parameter 7"),
    ] {
        let summary = run(&int128_options(&scratch, seed, 19, mode)).expect("the run");
        let text = summary.text();
        assert!(summary.findings.is_empty() && summary.errors.is_empty(), "{text}");
        let [(s, class, line)] = &summary.set_apart[..] else { panic!("one set apart: {text}") };
        assert_eq!((*s, *class), (seed, "llc19-int128-last-register"), "{text}");
        assert!(line.contains(&format!("seed {seed}: {site} with one integer")), "{text}");
        assert!(line.contains("gcc -O0 (prog.c) and llc-22 -O0 (the module) print"), "{text}");
        let results = std::fs::read_to_string(summary.results_file.as_ref().unwrap()).unwrap();
        assert!(results.contains(&format!("{seed}\tabi\tset-apart\t")), "{results}");
    }
}

// "Set apart (seeds)", at `--mix`'s self-check (WP-266): seed 226098 of
// the 1.0 candidate's budget (x86-64, LLVM 19, abi): llc 19's halves and
// its whole module print different garbage (`va_arg` reads the r9 its
// caller left unset); gcc and llc 22 print the same and llc 19's whole
// module does not, so the seed is set apart, not an error of the harness.
#[test]
fn llc19_int128_self_check_is_set_apart() {
    if !int128_tools_found() {
        return;
    }
    let scratch = Scratch::new("fuzz-int128-self-check");
    let seed = 226_098;
    let summary = run(&int128_options(&scratch, seed, 19, Mode::Mix)).expect("the run");
    let text = summary.text();
    assert!(summary.findings.is_empty() && summary.errors.is_empty(), "{text}");
    let [(s, class, line)] = &summary.set_apart[..] else { panic!("one set apart: {text}") };
    assert_eq!((*s, *class), (seed, "llc19-int128-last-register"), "{text}");
    assert!(line.contains("seed 226098: i128 at call @callee_0 argument 7 with one"), "{text}");
    assert!(line.contains("print the same (1377 bytes, exited with status 0)"), "{text}");
    assert!(line.contains("llc-19: llc's two halves disagree with the whole module"), "{text}");
}

// "Set apart" (`dead-store`, WP-266): seeds 220612 (func_1, prog.c:713,
// `l_2459 |= (&g_1494[0] == &g_1494[2]);`) and 220916 (func_23,
// prog.c:718, `(*l_161) = (g_168 = 0x8.564011p+55);`, stores of poison)
// of the 1.0 candidate's budget (x86-64, LLVM 22, debug): SelectionDAG
// deletes the statement, FastISel keeps code and a row as hod does, and
// GlobalISel falls back on the function (for other instructions of it).
// The pair is set apart with its evidence, and the seed is no finding.
#[test]
fn dead_stores_are_set_apart() {
    if !found(&["csmith", "clang-22", "llc-22", "x86_64-linux-gnu-gcc", "llvm-dwarfdump-22"])
        || !found(&["llvm-nm-22", "llvm-objdump-22"])
    {
        return;
    }
    let scratch = Scratch::new("fuzz-dead-store");
    for (seed, function, line, store) in [
        (220_612, "func_1", 713, "a store to the pointer a load of the line reads"),
        (220_916, "func_23", 718, "a store of poison"),
    ] {
        let mut options = options(&scratch, seed, 1);
        options.mode = Mode::Debug;
        options.target = Some(Arch::X86_64);
        let summary = run(&options).expect("the run");
        let text = summary.text();
        assert!(summary.findings.is_empty() && summary.errors.is_empty(), "{text}");
        let evidence = format!(
            "dead-store: seed {seed} {function} at prog.c:{line}: rows hod 1 selection-dag 0 \
             fast-isel 1 global-isel fell back (\"warning: Instruction selection used fallback \
             path for {function}\"); the IR: loads, stores, compares, casts, {store}"
        );
        assert!(text.contains(&evidence), "{text}");
    }
}

/// The two statements of [`dead_stores_are_set_apart`] alone.
const DEAD_STORES_C: &str = "\
int g; float *p; unsigned char x; int *a[4];
void f(void) {
  x |= (&a[0] == &a[2]);
  *p = (g = 0x8.564011p+55);
  g = 1;
}
int main(void) { float y; p = &y; f(); return 0; }
";

// "Set apart", `dead-store` (WP-266): what the fallback hides. The two
// statements of seeds 220612 and 220916 in a function GlobalISel selects
// itself (clang 22 -g, x86-64): SelectionDAG has no row at either line,
// FastISel and GlobalISel have one, as hod does. So it is a selector
// split (rows 1/0/1/1), and `dead-store`'s IR condition stands in for
// GlobalISel's row only where GlobalISel gave no answer of its own.
#[test]
fn dead_stores_without_fallback_are_a_selector_split() {
    use std::process::Command;
    use xtask::debug::lines::{LineTools, stmt_row_counts};
    use xtask::fuzz::case::{SELECTOR_SPLIT, SelectorRows, dead_store, fallbacks, selector_split};
    let gcc = if Arch::host() == Some(Arch::X86_64) { "gcc" } else { "x86_64-linux-gnu-gcc" };
    if !found(&["clang-22", "llc-22", "llvm-dwarfdump-22", "llvm-nm-22", "llvm-objdump-22", gcc]) {
        return;
    }
    let search = Search::from_env();
    let tool = |name: &str| search.versioned(name, 22).expect("found above");
    let line_tools = LineTools {
        dwarfdump: tool("llvm-dwarfdump"),
        nm: tool("llvm-nm"),
        objdump: tool("llvm-objdump"),
    };
    let scratch = Scratch::new("fuzz-dead-store-alone");
    let source = scratch.write("t.c", DEAD_STORES_C);
    let ir = scratch.join("t.ll");
    let step = |command: &mut Command| {
        let out = hod_testutil::run_with(command, b"", Duration::from_mins(2));
        assert!(out.success(), "{command:?}: {}", String::from_utf8_lossy(&out.stderr));
        out.stderr
    };
    step(
        Command::new(tool("clang"))
            .args(["-O0", "-g", "-w", "-S", "-emit-llvm"])
            .args(["--target=x86_64-unknown-linux-gnu", "-o"])
            .arg(&ir)
            .arg(&source),
    );
    let tail = ["-filetype=obj", "-mtriple=x86_64-unknown-linux-gnu", "-relocation-model=pic"];
    let mut built = Vec::new();
    for (name, args) in [
        ("hod", &[][..]),
        ("selection-dag", &["-global-isel=0", "-fast-isel=0"][..]),
        ("fast-isel", &["-global-isel=0", "-fast-isel=1"][..]),
        ("global-isel", &["-global-isel=1", "-global-isel-abort=2"][..]),
    ] {
        let object = scratch.join(format!("{name}.o"));
        let compiler = if name == "hod" { workspace_hod() } else { tool("llc") };
        let stderr = step(
            Command::new(compiler).arg("-O0").args(args).args(tail).arg("-o").arg(&object).arg(&ir),
        );
        let program = scratch.join(name);
        step(Command::new(gcc).arg("-pie").arg("-o").arg(&program).arg(&object));
        let rows = stmt_row_counts(&program, &line_tools).expect("the rows");
        built.push(SelectorRows { rows, no_evidence: fallbacks(&stderr), failed: false });
    }
    let [hod, dag, fast, global] = &built[..] else { unreachable!() };
    assert!(global.no_evidence.is_empty(), "GlobalISel selects f itself");
    for line in [3, 4] {
        let pair = hod.rows.keys().find(|(f, _, l)| f == "f" && *l == line).expect("hod's row");
        assert_eq!(dag.rows.get(pair), None, "SelectionDAG deletes line {line}");
        let rows = hod.rows[pair];
        let split = selector_split(pair, rows, fast, global).expect("a selector split");
        assert_eq!(split.class(), SELECTOR_SPLIT);
        assert!(split.fast_isel >= 1 && split.global_isel >= 1, "{split:?}");
        let facts = xtask::debug::permitted::IrFacts::of_modules([std::fs::read_to_string(&ir)
            .unwrap()
            .as_str()]);
        assert!(facts.dead_store("f", &pair.1, line).is_some(), "line {line}");
        assert_eq!(dead_store(pair, rows, fast, global, &facts), None);
    }
}

// "Set apart (seeds)": the planted ABI bug (hod swaps two parameters) in
// a program with the shape (abi seed 2 at x86-64 LLVM 19 has one):
// llc 22's other half disagrees with hod's half as llc 19's does, so the
// finding stays a finding and says why; at LLVM 22 the class is not asked
// at all. (On seed 126005 the swap crosses no object boundary that
// matters: hod A + llc 22 B prints gcc's output, and the seed is set
// apart, rightly: llc 19 alone explains its finding.)
#[test]
fn a_wrong_convention_with_the_shape_stays_a_finding() {
    if !int128_tools_found() || !found(&["clang-22", "llvm-extract-22"]) {
        return;
    }
    let scratch = Scratch::new("fuzz-int128-wrong");
    for llvm in [19, 22] {
        let mut options = int128_options(&scratch, 2, llvm, Mode::Mix);
        options.hod = Some(swapping_hod(&scratch));
        let summary = run(&options).expect("the run");
        let text = summary.text();
        assert!(summary.set_apart.is_empty(), "{text}");
        let [found] = &summary.findings[..] else { panic!("one finding: {text}") };
        assert_eq!(found.kind, Kind::Disagree, "{text}");
        let asked = found.detail.contains("not llc19-int128-last-register: ");
        assert_eq!(asked, llvm == 19, "{text}");
    }
}

// "Profiles", "Census": the ABI generator's programs reach what csmith's
// do not, and the census counts it.
#[test]
fn the_census_of_the_abi_profile() {
    if !found(&["clang-22"]) || Arch::host().is_none() {
        return;
    }
    let scratch = Scratch::new("fuzz-census");
    let mut options = options(&scratch, 1, 8);
    options.profile = Profile::by_name("abi").unwrap();
    options.mode = Mode::Census;
    let summary = run(&options).expect("the census");
    let census = summary.census.as_ref().expect("a census");
    assert_eq!(census.programs, 8, "{}", summary.text());
    for feature in ["define:varargs", "type:i128", "intrinsic:llvm.va_start", "attr:sret"] {
        assert!(census.features.contains_key(feature), "{feature}: {}", summary.text());
    }
}
