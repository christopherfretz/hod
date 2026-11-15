//! Variable-length arrays in `alwaysinline` bodies (WP-237, `spec/cli.md`
//! "Always-inline"), by execution on both targets (x86-64 under
//! `qemu-x86_64` on another host).
//!
//! A C front end writes a variable-length array at `-O0` as
//! `%s = call ptr @llvm.stacksave.p0()`, `store ptr %s, ptr %saved_stack`
//! (an entry-block slot), the dynamic `alloca`, and at the end of the
//! scope a load of the slot fed to `llvm.stackrestore`. hod keeps an
//! `alwaysinline` body out of line; `opt -passes=always-inline` (19 and
//! 22) inlines it and wraps the inlined body in a pair of its own in the
//! caller (`%savedstack`). Either way the stack pointer after the body is
//! the one before it and the array is valid until the restore.
//!
//! `data/alwaysinline_vla/vla-<target>.ll` is clang 22's pre-opt module
//! (`-O0 -Xclang -disable-llvm-passes -fno-omit-frame-pointer
//! -mno-omit-leaf-frame-pointer`, so that LLVM 19 reads it) of: arrays of
//! 1-, 4-, 8- and 24-byte elements and of a 64-byte-aligned element, two
//! arrays in one body, a body inlined twice into one caller, in a loop of
//! 100,000 iterations (the array's address must not move), with an early
//! return out of the array's scope, an array in a loop's scope, nested
//! bodies each with an array, a caller with its own array, and the shape
//! of jemalloc's `tcache_bin_flush_impl_small` (a union array of
//! `nflush + 1` in a body another body calls). `driver.c` calls them with
//! 0, 1, 2, 17, 1,000 and 100,000 elements. hod (plain, and every
//! function under `Stress::ALL`) and `opt -passes=always-inline` + `llc
//! -O0` 19 and 22, linked by bfd, lld and mold, print the same.

#![allow(dead_code, reason = "the linking support is shared with other tests")]

#[path = "../../hod_obj/tests/common/linking.rs"]
mod linking;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use hod::{Options, OutputKind, RelocModel, Report, Settings};
use hod_testutil::{LLVM_MAJORS, Scratch, Target, run_with, tool};
use linking::{Toolchain, exit_code};

const DRIVER: &str = include_str!("data/alwaysinline_vla/driver.c");

fn module(target: Target) -> &'static str {
    match target {
        Target::Aarch64 => include_str!("data/alwaysinline_vla/vla-aarch64.ll"),
        Target::X86_64 => include_str!("data/alwaysinline_vla/vla-x86_64.ll"),
    }
}

/// hod's object through the session (what `hod` the program does).
fn hod_object(text: &str) -> Vec<u8> {
    let options = Options {
        output_kind: OutputKind::Object,
        relocation_model: Some(RelocModel::Pic),
        ..Options::default()
    };
    hod::compile_with(text.as_bytes(), &options, &Settings::default(), &mut Report::default())
        .unwrap_or_else(|e| panic!("hod: {e}"))
}

/// The backend's object of every function under `Stress::ALL`. The
/// session has nothing to change in this module (its `alwaysinline`
/// bodies are `internal`, compiled as they are).
fn stressed_object(text: &str, target: Target) -> Vec<u8> {
    let parsed = hod_ll::parse_module(text.as_bytes(), &hod_ll::ParseOptions::default())
        .unwrap_or_else(|e| panic!("hod_ll: {e}"));
    let side = parsed.text;
    let mut module = parsed.module;
    let mut bodies = Vec::new();
    hod_ll::parse_bodies(&mut module, &side, |_, read| {
        bodies.push(read.body.expect("every body is read").clone());
        Ok::<(), hod_ll::LlError>(())
    })
    .unwrap_or_else(|e| panic!("hod_ll: {e}"));
    let options = hod_cg::Options {
        reloc_model: hod_cg::RelocModel::Pic,
        stress: hod_cg::Stress::ALL,
        ..hod_cg::Options::default()
    };
    match target {
        Target::X86_64 => hod_x64::compile_to_object(&module, &bodies, &options)
            .unwrap_or_else(|e| panic!("stress: {e}")),
        Target::Aarch64 => hod_a64::compile_to_object(&module, &bodies, &options)
            .unwrap_or_else(|e| panic!("stress: {e}")),
    }
}

/// `opt -passes=always-inline` then `llc -O0` of `major`: the reference.
fn llvm_object(dir: &Scratch, major: u32, text: &str, name: &str) -> Option<PathBuf> {
    let opt = tool(&format!("opt-{major}"))?;
    let llc = tool(&format!("llc-{major}"))?;
    let mut inline = Command::new(opt);
    inline.args(["-passes=always-inline", "-o", "-", "-"]);
    let inlined = run_with(&mut inline, text.as_bytes(), Duration::from_mins(2));
    assert!(inlined.success(), "opt {major}: {}", inlined.stderr_text());
    let object = dir.join(name);
    let mut command = Command::new(llc);
    command.args(["-O0", "-global-isel=0", "-fast-isel=0", "-relocation-model=pic"]);
    command.args(["-filetype=obj", "-o"]).arg(&object);
    let ran = run_with(&mut command, &inlined.stdout, Duration::from_mins(2));
    assert!(ran.success(), "llc {major}: {}", ran.stderr_text());
    Some(object)
}

// Every build of every target, under every linker, prints the same; the
// array never moves in the loop, the over-aligned one is aligned, and the
// stack pointer after the calls is the one before.
#[test]
fn variable_length_arrays_in_always_inline_bodies_run_as_opt_and_llc() {
    for target in Target::ALL {
        let dir = Scratch::new(&format!("hod-alwaysinline-vla-{}", target.name()));
        let Some(toolchain) = Toolchain::for_target(&dir, target) else { continue };
        let text = module(target);
        let mut objects = vec![
            ("hod".to_owned(), dir.write("hod.o", hod_object(text))),
            ("stress".to_owned(), dir.write("stress.o", stressed_object(text, target))),
        ];
        for major in LLVM_MAJORS {
            let name = format!("llvm{major}.o");
            if let Some(object) = llvm_object(&dir, major, text, &name) {
                objects.push((format!("llvm{major}"), object));
            }
        }
        let main = toolchain.compile_c(&dir, "driver", DRIVER, &["-O0", "-fPIE"]);
        let mut expected: Option<(String, String)> = None;
        let mut runs = 0;
        for (build, object) in &objects {
            for linker in &toolchain.linkers {
                let who = format!("{} {build} {}", target.name(), linker.name);
                let program = dir.join(format!("{build}-{}", linker.name));
                let inputs: [&Path; 2] = [&main, object];
                let ran = toolchain.link_and_run(linker, &["-pie"], &inputs, &program);
                assert_eq!(exit_code(&ran), Some(0), "{who}: {}", ran.stderr_text());
                let out = ran.stdout_text();
                match &expected {
                    None => expected = Some((who, out)),
                    Some((first, want)) => assert_eq!(&out, want, "{who} against {first}"),
                }
                runs += 1;
            }
        }
        let Some((_, out)) = expected else { continue };
        assert!(out.contains("loop 0 0\n"), "{}: the array moved:\n{out}", target.name());
        assert!(out.ends_with("stack same\n"), "{}:\n{out}", target.name());
        // `call_aligned(n)` answers the sum of `i % 256` below `n`, or
        // 1,000,000 + the misalignment.
        let aligned = out.lines().filter_map(|line| line.strip_prefix("aligned "));
        for line in aligned {
            let mut words = line.split(' ').map(|v| v.parse::<u64>().expect(line));
            let (n, sum) = (words.next().expect(line), words.next().expect(line));
            assert_eq!(sum, (0..n).map(|i| i % 256).sum::<u64>(), "{}: {line}", target.name());
        }
        assert_eq!(out.lines().count(), 6 * 8 + 3, "{}", target.name());
        eprintln!("{}: {runs} runs print the same", target.name());
    }
}
