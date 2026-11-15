//! `asm goto` with a template on x86-64 (WP-254, `spec/asm.md` §20): a
//! `callbr` whose template branches to its label operands. The template
//! is assembled as any other (WP-187); each branch to a label goes to a
//! trampoline that writes the outputs and that edge's `phi`s, then to
//! the destination; the fallthrough goes on at the default destination.
//!
//! The oracle is `llc` 22 and 19, `-O0` and the trusted `-O0
//! -fast-isel=0 -global-isel=0`: hod's objects (plain, every stress
//! option, far branches only) and `llc`'s are linked with a C driver
//! under every linker and run (under the emulator on a host that is not
//! x86-64), and every path of every function (the fallthrough, each
//! label) prints what is computed here. The corpus module
//! `dcl.asm/asm-goto.cpp` (kiln's, LLVM 22 and 19 forms) prints what
//! `llc`'s build of it prints.

#[path = "../../hod_obj/tests/common/linking.rs"]
#[allow(dead_code, reason = "a shared helper: not every function is used here")]
mod linking;

#[path = "common/float_gen.rs"]
mod float_gen;

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use float_gen::{ll_header, parse, pic};
use hod_cg::Stress;
use hod_testutil::{Scratch, run_with};

const FUNCTIONS: &str = include_str!("data/asm_goto/functions.ll");
const DRIVER: &str = include_str!("data/asm_goto/driver.c");
const CORPUS_22: &str = include_str!("data/asm_goto/asm-goto.llvm22.ll");
const CORPUS_19: &str = include_str!("data/asm_goto/asm-goto.llvm19.ll");

/// hod's builds: plain, every stress option, far branches only.
const BUILDS: [(&str, Stress); 3] = [
    ("hod", Stress { min_pool: false, far: false, displace: false, coro_frame: false }),
    ("stress", Stress::ALL),
    ("far", Stress { min_pool: false, far: true, displace: false, coro_frame: false }),
];

/// `llc`'s instruction selections: `-O0` and the trusted one.
const SELECTIONS: [(&str, &[&str]); 2] =
    [("O0", &["-O0"]), ("sdag", &["-O0", "-fast-isel=0", "-global-isel=0"])];

/// `llc` of LLVM `major` with `flags` for x86-64 PIC, to an object in
/// `dir`; `None` without that `llc`.
fn llc(dir: &Scratch, major: u32, flags: &[&str], name: &str, text: &str) -> Option<PathBuf> {
    let llc = float_gen::llc_of(major)?;
    let input = dir.write(&format!("{name}.ll"), text);
    let output = dir.join(format!("{name}.o"));
    let mut command = Command::new(llc);
    command
        .args(flags)
        .args(["-mtriple=x86_64-unknown-linux-gnu", "-relocation-model=pic", "-filetype=obj"])
        .arg(&input)
        .arg("-o")
        .arg(&output);
    let outcome = run_with(&mut command, &[], Duration::from_mins(5));
    assert!(outcome.success(), "llc {major} {flags:?}: {}", outcome.stderr_text());
    Some(output)
}

/// Every `llc` object of `text`: both majors, both selections.
fn llc_objects(scratch: &Scratch, prefix: &str, text: &str) -> Vec<(String, PathBuf)> {
    let mut objects = Vec::new();
    for major in [22, 19] {
        for (selection, flags) in SELECTIONS {
            let name = format!("{prefix}llc{major}-{selection}");
            if let Some(object) = llc(scratch, major, flags, &name, text) {
                objects.push((name, object));
            }
        }
    }
    objects
}

/// What the driver prints, computed from each function's paths.
fn expected() -> String {
    let mut lines = String::new();
    let mut sum = 0i64;
    for x in -12i32..14 {
        let one = if x != 0 { x + 20 } else { 10 };
        let many = match x {
            0 => -7,
            1 => 100,
            2 => 3 * x + 200,
            _ => 3 * x,
        };
        let this = if x == 5 || x == 6 { 5000 } else { 2 * (x + 1) };
        let cfi = if x.cast_unsigned() >= 4 { 2 } else { 1 };
        let frame = if x == 0 { 4 } else { 4 * x + 7 };
        let out = if x + 5 > 10 { x - 95 } else { 2 * (x + 5) };
        let phi = (x + 6).abs();
        let out2 = if x == 0 { 2 } else { 10 * (x + 1) + (x + 2) };
        let m = i64::from(x).max(1);
        let looped = m * (m - 1) / 2;
        sum += i64::from(frame);
        writeln!(lines, "{one} {many} {this} {cfi} {frame} {out} {phi} {out2} {looped} {x} {sum}")
            .unwrap();
    }
    lines
}

/// Links each object (with `extra` inputs) under every linker, runs it
/// and hands what it prints to `check` with the object's name; returns
/// the runs.
fn run_all(
    toolchain: &linking::Toolchain,
    scratch: &Scratch,
    objects: &[(String, PathBuf)],
    extra: &[&Path],
    check: &dyn Fn(&str, &str) -> Result<(), String>,
) -> Vec<String> {
    let mut ran = Vec::new();
    for (leg, object) in objects {
        for linker in &toolchain.linkers {
            let program = scratch.join(format!("{leg}-{}", linker.name));
            let mut inputs: Vec<&Path> = extra.to_vec();
            inputs.push(object);
            toolchain.link_ok(linker, &[], &inputs, &program);
            let outcome =
                run_with(&mut toolchain.runner_command(&program), &[], Duration::from_mins(2));
            let run = format!("{leg}/{}", linker.name);
            assert!(outcome.success(), "{run}: {}", outcome.stderr_text());
            check(leg, &outcome.stdout_text()).unwrap_or_else(|e| panic!("{run}: {e}"));
            ran.push(run);
        }
    }
    ran
}

/// The columns of `goto_phi` and `goto_out2` in the driver's lines.
const PHI_COLUMN: usize = 6;
const OUT2_COLUMN: usize = 7;

/// `got` against `want`, with column `masked` of each line left out.
fn compare(got: &str, want: &str, masked: Option<usize>) -> Result<(), String> {
    let cut = |text: &str| -> Vec<Vec<String>> {
        text.lines()
            .map(|line| {
                line.split(' ')
                    .enumerate()
                    .map(|(i, w)| if Some(i) == masked { "_".to_owned() } else { w.to_owned() })
                    .collect()
            })
            .collect()
    };
    if cut(got) == cut(want) { Ok(()) } else { Err(format!("printed\n{got}expected\n{want}")) }
}

#[test]
fn asm_goto_runs_every_path_as_llc_builds_it() {
    let text = format!("{}{FUNCTIONS}", ll_header());
    let (module, bodies) = parse(&text);
    let classifier = hod_ir::Classifier::new(hod_ir::Support::shipped(hod_ir::Arch::X86_64));
    for body in &bodies {
        classifier.body(&module, body).unwrap_or_else(|r| panic!("classify: {}", r.construct));
    }
    let scratch = Scratch::new("x64-asm-goto");
    let mut objects = Vec::new();
    for (leg, stress) in BUILDS {
        let object = hod_x64::compile_to_object(&module, &bodies, &pic(stress))
            .unwrap_or_else(|e| panic!("{leg}: {e}"));
        objects.push((leg.to_owned(), scratch.write(&format!("{leg}.o"), object)));
    }
    let Some(toolchain) = linking::Toolchain::x86_64(&scratch) else { return };
    objects.extend(llc_objects(&scratch, "", &text));
    let driver = toolchain.compile_c(&scratch, "driver", DRIVER, &[]);
    let want = expected();
    // `llc` 19 reads an output on the indirect edge wrongly in one
    // function per selection; hod writes the outputs on that edge as
    // `llc` 22 does, and only that column of that build is left out:
    // - `-O0` (fast selection) reloads `goto_phi`'s output from its
    //   spill slot before it is spilled there (`movl -8(%rsp), %ecx`
    //   ahead of `movl %eax, -8(%rsp)`): a stale value;
    // - `-fast-isel=0` loses `goto_out2`'s first output on that edge
    //   (AArch64's seed `goto_out2` shows the same, `spec/codegen.md`
    //   §5.5.3).
    let check = |leg: &str, got: &str| {
        let masked = match leg {
            "llc19-O0" => Some(PHI_COLUMN),
            "llc19-sdag" => Some(OUT2_COLUMN),
            _ => None,
        };
        compare(got, &want, masked)
    };
    let ran = run_all(&toolchain, &scratch, &objects, &[&driver], &check);
    eprintln!("x64 asm goto: 9 functions x 26 arguments agree over {ran:?}");
}

/// The corpus module `dcl.asm/asm-goto.cpp` (both LLVM forms): hod's
/// build prints what `llc`'s prints, and that is the program's answer.
#[test]
fn corpus_asm_goto_module_prints_what_llc_builds_print() {
    let scratch = Scratch::new("x64-asm-goto-corpus");
    let mut objects = Vec::new();
    for (form, text) in [("llvm22", CORPUS_22), ("llvm19", CORPUS_19)] {
        let (module, bodies) = parse(text);
        for (leg, stress) in BUILDS {
            let object = hod_x64::compile_to_object(&module, &bodies, &pic(stress))
                .unwrap_or_else(|e| panic!("{form} {leg}: {e}"));
            let name = format!("{form}-{leg}");
            objects.push((name.clone(), scratch.write(&format!("{name}.o"), object)));
        }
        objects.extend(llc_objects(&scratch, &format!("{form}-"), text));
    }
    let Some(toolchain) = linking::Toolchain::x86_64(&scratch) else { return };
    // f: the empty template falls through; g: its `jmp` is taken; h and
    // k: the output on the second label's edge and on the fallthrough.
    let ran =
        run_all(&toolchain, &scratch, &objects, &[], &|_, got| compare(got, "0 1 7 8 9\n", None));
    eprintln!("x64 asm goto corpus module: agrees over {ran:?}");
}
