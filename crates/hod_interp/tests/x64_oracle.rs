//! The LLVM oracle's random programs on the other architecture
//! (`docs/design/0006-x86-64.md` 6.1, `spec/harness.md` "Cross runs";
//! WP-139): batches of `hod_interp`'s generated programs, written for this
//! machine, are built once natively (`llc -O0 -global-isel=0
//! -fast-isel=0`, the reference) and once for the other architecture with
//! only the triple line changed, by the same selector of each LLVM major,
//! linked by that architecture's cross driver and run under its emulator
//! on the baseline CPU. Both programs must print exactly what the
//! interpreter says, so exactly the same.
//!
//! The programs print what they compute; they depend on the layout only as
//! far as both 64-bit little-endian Linux targets share it. On an AArch64
//! host this is x86-64 SelectionDAG under `qemu-x86_64` against native
//! AArch64 SelectionDAG.
//!
//! The quick test runs four batches (120 programs) with each major; the
//! ignored one runs the design's larger batches. Without the tools the
//! tests skip with a note, or fail under `HOD_REQUIRE_TOOLS=1`.

mod llvm_text;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use hod_interp::generate::{Generated, Options, generate};
use hod_ir::TargetSpec;
use hod_testutil::{LLVM_MAJORS, Scratch, Target, run_with, tool, tools_required};

/// This machine's target and the other one, as the generator and
/// `hod_testutil` name them.
fn targets() -> Option<(TargetSpec, Target)> {
    let found = match Target::host() {
        Some(Target::Aarch64) => Some((TargetSpec::aarch64_linux_gnu(), Target::X86_64)),
        Some(Target::X86_64) => Some((TargetSpec::x86_64_linux_gnu(), Target::Aarch64)),
        None => None,
    };
    if found.is_none() {
        assert!(!tools_required(), "no native target for this host (HOD_REQUIRE_TOOLS=1)");
        eprintln!("note: skipping: the cross oracle runs on Linux only");
    }
    found
}

fn triple(target: Target) -> String {
    format!("{}-unknown-linux-gnu", target.name())
}

/// What builds and runs both sides.
struct Tools {
    /// The `llc` of each major installed.
    llc: Vec<(u32, PathBuf)>,
    gcc: PathBuf,
    cross_gcc: PathBuf,
    emulator: PathBuf,
    sysroot: PathBuf,
}

fn tools(other: Target) -> Option<Tools> {
    let llc: Vec<(u32, PathBuf)> = LLVM_MAJORS
        .into_iter()
        .filter_map(|major| {
            let path = PathBuf::from(format!("/usr/lib/llvm-{major}/bin/llc"));
            path.is_file().then_some((major, path))
        })
        .collect();
    if llc.is_empty() {
        assert!(!tools_required(), "no LLVM major installed (HOD_REQUIRE_TOOLS=1)");
        eprintln!("note: no llc of LLVM {LLVM_MAJORS:?}; skipping");
        return None;
    }
    Some(Tools {
        llc,
        gcc: tool("gcc")?,
        cross_gcc: other.tool("gcc")?,
        emulator: other.tool("emulator")?,
        sysroot: other.tool("sysroot")?,
    })
}

fn must(command: &mut Command, what: &str) -> Vec<u8> {
    let outcome = run_with(command, b"", Duration::from_mins(5));
    assert!(outcome.success(), "{what} failed ({:?}):\n{}", outcome.status, outcome.stderr_text());
    outcome.stdout
}

/// Builds `source` with `llc` for `triple` by the reference's selector,
/// links it with `driver` and returns the program.
fn build(llc: &Path, driver: &Path, triple: &str, source: &Path, out: &Path) -> PathBuf {
    let object = out.with_extension("o");
    must(
        Command::new(llc)
            .args(["-O0", "-global-isel=0", "-fast-isel=0", "-relocation-model=pic"])
            .args(["-filetype=obj", &format!("-mtriple={triple}")])
            .arg("-o")
            .arg(&object)
            .arg(source),
        "llc",
    );
    // `frem` is a call of `fmod`.
    must(Command::new(driver).arg(&object).arg("-o").arg(out).arg("-lm"), "link");
    out.to_path_buf()
}

/// What a program printed, and how it ended if not with status 0.
fn printed(outcome: &hod_testutil::Outcome) -> String {
    let mut text = outcome.stdout_text();
    if !outcome.success() {
        text.push_str(&format!("exited with {:?}\n", outcome.status));
    }
    text
}

/// Checks one batch: the interpreter, the native program and the other
/// architecture's program under the emulator, with each `llc`. Returns
/// how many programs were checked.
fn check_batch(
    tools: &Tools,
    native: &TargetSpec,
    other: Target,
    options: &Options,
    seeds: std::ops::Range<u64>,
) -> usize {
    let labels: Vec<u64> = seeds.collect();
    let programs: Vec<Generated> = labels.iter().map(|&seed| generate(seed, options)).collect();
    let mut file = llvm_text::File::new();
    let mut expected = String::new();
    for (&seed, program) in labels.iter().zip(&programs) {
        let outcome = program.run();
        assert!(outcome.result.is_ok(), "seed {seed}: {:?}", outcome.result);
        file.add(seed, program);
        expected.push_str(&llvm_text::expected(seed, &outcome));
    }
    let name = format!("batch-{}-{}", labels[0], labels.len());
    let scratch = Scratch::new("x64-oracle");
    let text = file.finish(native.triple());
    let other_triple = triple(other);
    let retargeted = text.replacen(
        &format!("target triple = \"{}\"", native.triple()),
        &format!("target triple = \"{other_triple}\""),
        1,
    );
    assert_ne!(text, retargeted, "the batch has a triple line");
    let source = scratch.write(&format!("{name}.ll"), &text);
    let cross_source = scratch.write(&format!("{name}.x.ll"), &retargeted);
    for (major, llc) in &tools.llc {
        let program = build(
            llc,
            &tools.gcc,
            native.triple(),
            &source,
            &scratch.join(format!("{name}.{major}")),
        );
        let here = printed(&run_with(&mut Command::new(&program), b"", Duration::from_mins(2)));
        assert_eq!(
            here, expected,
            "{name}: LLVM {major}'s native program and the interpreter disagree"
        );
        let cross = build(
            llc,
            &tools.cross_gcc,
            &other_triple,
            &cross_source,
            &scratch.join(format!("{name}.{major}.x")),
        );
        let mut emulated = Command::new(&tools.emulator);
        emulated.args(other.emulator_args(&tools.sysroot)).arg(&cross);
        // Six times a native program's time ("Cross runs").
        let there = printed(&run_with(&mut emulated, b"", Duration::from_mins(12)));
        if there != here {
            let kept = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("{name}.x.ll"));
            std::fs::write(&kept, &retargeted).expect("write");
            let at = here.lines().zip(there.lines()).position(|(a, b)| a != b);
            panic!(
                "{name}: LLVM {major}'s {} program under the emulator prints what the native one \
                 does not (first differing line {at:?}); batch kept in {}",
                other.name(),
                kept.display()
            );
        }
    }
    labels.len()
}

/// Runs `batches` (options, seeds) if the tools are there.
fn check_all(batches: &[(Options, std::ops::Range<u64>)]) -> usize {
    let Some((native, other)) = targets() else { return 0 };
    let Some(tools) = tools(other) else { return 0 };
    batches
        .iter()
        .map(|(options, seeds)| {
            let options = Options { target: native.clone(), ..options.clone() };
            check_batch(&tools, &native, other, &options, seeds.clone())
        })
        .sum()
}

/// Design 0006 6.1: the kept batches of the default, larger and vector
/// programs print the same on the other architecture, with each LLVM.
#[test]
fn random_programs_agree_on_the_other_architecture() {
    let vectors = Options { vectors: true, ..Options::default() };
    let checked = check_all(&[
        (Options::default(), 0..48),
        (Options::default().sized(8, 300), 1000..1006),
        (vectors.clone(), 0..60),
        (vectors.sized(6, 300), 500..506),
    ]);
    println!("{checked} programs print the same natively and under the emulator");
}

/// The design's larger batches (`docs/design/0006-data/oracle`): several
/// hundred programs at several sizes. A minute or more.
#[test]
#[ignore = "a minute or more; run with --ignored"]
fn many_random_programs_agree_on_the_other_architecture() {
    let checked = check_all(&[
        (Options::default(), 10_000..10_100),
        (Options::default(), 10_100..10_200),
        (Options::default(), 10_200..10_300),
        (Options::default().sized(2, 12), 20_000..20_250),
        (Options::default().sized(2, 12), 20_250..20_300),
        (Options::default().sized(6, 400), 30_000..30_020),
        (Options::default().sized(6, 400), 30_020..30_030),
        (Options::default().sized(3, 3000), 40_000..40_004),
        (Options::default().sized(3, 3000), 40_004..40_006),
    ]);
    println!("{checked} programs print the same natively and under the emulator");
}
