//! Sections of module-level assembly's own name, linked (`spec/asm.md`
//! §9.1, `spec/codegen.md` §9.7.3, WP-255). The module is a reduced
//! kiln-built translation unit of a library that embeds gdb pretty
//! printers: module assembly that fills `.debug_gdb_scripts` (`"MS"`,
//! entries of 1 byte) with an inlined Python script
//! (`tests/data/gdb-scripts-<arch>.ll`; the AArch64 form is the x86-64
//! one with its triple and data layout replaced, the text is the same).
//!
//! - In the object, the section's header and bytes are `llc`'s (LLVM 19
//!   and 22) on both targets.
//! - Linked with a C `main` by each linker of the matrix that is present,
//!   the program runs, and its `.debug_gdb_scripts` is the one of the
//!   program linked from `llc`'s object.
//! - On an AArch64 host, gdb loads a tiny inlined script from hod's
//!   program (`set auto-load safe-path /`) and runs it.

mod elfview;

#[path = "../../hod_obj/tests/common/linking.rs"]
#[allow(dead_code)]
mod linking;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use elfview::Elf;
use hod_cg::{Options, RelocModel, emit_module};
use hod_obj::Arch;
use hod_testutil::{LLVM_MAJORS, Scratch, run_with, tool, tools_required};
use linking::{Toolchain, exit_code};

const SECTION: &str = ".debug_gdb_scripts";

const MAIN: &str = "int main(void) { return 0; }\n";

fn data_file(arch: Arch) -> PathBuf {
    let name = match arch {
        Arch::AArch64 => "gdb-scripts-aarch64.ll",
        Arch::X86_64 => "gdb-scripts-x86_64.ll",
    };
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data").join(name)
}

fn majors() -> Vec<u32> {
    LLVM_MAJORS
        .into_iter()
        .filter(|m| Path::new(&format!("/usr/lib/llvm-{m}/bin/llc")).is_file())
        .collect()
}

fn llc(scratch: &Scratch, major: u32, name: &str, input: &Path) -> PathBuf {
    let output = scratch.join(format!("{name}.o"));
    let mut command = Command::new(format!("/usr/lib/llvm-{major}/bin/llc"));
    command
        .args(["-O0", "-global-isel=0", "-fast-isel=0", "-relocation-model=pic", "-filetype=obj"])
        .arg(input)
        .arg("-o")
        .arg(&output);
    let outcome = run_with(&mut command, &[], Duration::from_mins(1));
    assert!(outcome.success(), "{name}: llc failed:\n{}", outcome.stderr_text());
    assert!(outcome.stderr_text().is_empty(), "{name}: llc: {}", outcome.stderr_text());
    output
}

fn hod(scratch: &Scratch, name: &str, text: &[u8]) -> PathBuf {
    let parsed = hod_ll::parse_module(text, &hod_ll::ParseOptions::default())
        .unwrap_or_else(|e| panic!("{name}: hod_ll: {e}"));
    hod_ir::Classifier::new(hod_ir::Support::shipped(parsed.module.target().arch()))
        .module(&parsed.module)
        .unwrap_or_else(|e| panic!("{name}: classify: {}", e.construct));
    let options = Options { reloc_model: RelocModel::Pic, ..Options::default() };
    let emitted = emit_module(&parsed.module, &options, &[]).expect("emission");
    linking::write(scratch, &format!("{name}.o"), &emitted.object)
}

/// The section's header fields and bytes: `name type flags entsize
/// align` and the contents.
fn section(file: &Path) -> (String, Vec<u8>) {
    let elf = Elf::parse(std::fs::read(file).unwrap());
    let matching: Vec<usize> =
        (0..elf.sections.len()).filter(|&i| elf.sections[i].name == SECTION).collect();
    assert_eq!(matching.len(), 1, "{}: one {SECTION}", file.display());
    let s = &elf.sections[matching[0]];
    let header = format!("{} {:#x} {:#x} {} {}", s.name, s.sh_type, s.flags, s.entsize, s.align);
    (header, elf.data(matching[0]).to_vec())
}

fn links_like_llc(arch: Arch, toolchain: &Toolchain, scratch: &Scratch) {
    let majors = majors();
    if majors.is_empty() {
        assert!(!tools_required(), "no llc (HOD_REQUIRE_TOOLS=1)");
        return;
    }
    let input = data_file(arch);
    let text = std::fs::read(&input).unwrap();
    let ours_o = hod(scratch, "hod", &text);
    let (header, bytes) = section(&ours_o);
    assert_eq!(header, format!("{SECTION} 0x1 0x30 1 1"), "hod's object");
    assert!(bytes.starts_with(b"\x04gdb.inlined-script."), "hod's object");
    assert!(bytes.ends_with(b"\n\0"), "hod's object");
    let main_o = toolchain.compile_c(scratch, "main", MAIN, &["-O0"]);
    for major in majors {
        let theirs_o = llc(scratch, major, &format!("llc-{major}"), &input);
        assert_eq!(section(&theirs_o), (header.clone(), bytes.clone()), "LLVM {major}: object");
        for linker in &toolchain.linkers {
            let mut sections = Vec::new();
            for (who, object) in [("hod", &ours_o), ("llc", &theirs_o)] {
                let program = scratch.join(format!("{who}-{major}-{}", linker.name));
                toolchain.link_ok(linker, &[], &[&main_o, object], &program);
                let outcome = toolchain.run(linker, &program);
                assert_eq!(exit_code(&outcome), Some(0), "{who} with {}", linker.name);
                sections.push(section(&program));
            }
            assert_eq!(sections[0], sections[1], "LLVM {major}, {}: programs", linker.name);
            assert_eq!(sections[0].1, bytes, "LLVM {major}, {}: program's bytes", linker.name);
        }
    }
    eprintln!(
        "emit_module_asm_link: {arch:?}: {SECTION} as llc's under {}",
        toolchain.linkers.iter().map(|l| l.name).collect::<Vec<_>>().join(", ")
    );
}

// spec/asm.md §9.1, spec/codegen.md §9.7.3: the reduced module's
// `.debug_gdb_scripts`, in the object and linked, is llc's (AArch64).
#[test]
fn gdb_scripts_link_like_llc() {
    let scratch = Scratch::new("emit-module-asm-link");
    let Some(toolchain) = Toolchain::aarch64(&scratch) else { return };
    links_like_llc(Arch::AArch64, &toolchain, &scratch);
}

// The same on x86-64, linked by the cross toolchain and run under the
// emulator when the host is not x86-64.
#[test]
fn x86_64_gdb_scripts_link_like_llc() {
    let scratch = Scratch::new("emit-module-asm-link-x64");
    let Some(toolchain) = Toolchain::x86_64(&scratch) else {
        assert!(!tools_required(), "no x86-64 toolchain (HOD_REQUIRE_TOOLS=1)");
        return;
    };
    links_like_llc(Arch::X86_64, &toolchain, &scratch);
}

/// A tiny inlined script (kind 4, Python) that prints a marker when gdb
/// loads the program.
const MARKER_MODULE: &str = concat!(
    "target datalayout = \"e-m:e-p270:32:32-p271:32:32-p272:64:64-i8:8:32-i16:16:32-i64:64-i128:128-n32:64-S128-Fn32\"\n",
    "target triple = \"aarch64-unknown-linux-gnu\"\n",
    "module asm \".pushsection \\22.debug_gdb_scripts\\22, \\22MS\\22,%progbits,1\"\n",
    "module asm \".ascii \\22\\5C4gdb.inlined-script.hod_marker\\5Cn\\22\"\n",
    "module asm \".ascii \\22import gdb\\5Cn\\22\"\n",
    "module asm \".ascii \\22gdb.write(\\5C\\22hod-inlined-script-ran\\5C\\5Cn\\5C\\22)\\5Cn\\22\"\n",
    "module asm \".byte 0\"\n",
    "module asm \".popsection\"\n",
);

// spec/asm.md §9.1: gdb finds and runs the script hod's program carries
// (natively on an AArch64 host, where gdb runs the target's programs).
#[test]
fn gdb_runs_the_inlined_script() {
    if !cfg!(target_arch = "aarch64") {
        eprintln!("note: the program is built for AArch64; skipping on this host");
        return;
    }
    let Some(gdb) = tool("gdb") else { return };
    let scratch = Scratch::new("emit-module-asm-gdb");
    let Some(toolchain) = Toolchain::aarch64(&scratch) else { return };
    let ours_o = hod(&scratch, "marker", MARKER_MODULE.as_bytes());
    let (_, bytes) = section(&ours_o);
    assert!(bytes.windows(22).any(|w| w == b"hod-inlined-script-ran"), "{bytes:?}");
    let main_o = toolchain.compile_c(&scratch, "main", MAIN, &["-O0"]);
    for linker in &toolchain.linkers {
        let program = scratch.join(format!("marker-{}", linker.name));
        toolchain.link_ok(linker, &[], &[&main_o, &ours_o], &program);
        let mut command = Command::new(&gdb);
        command.args(["-nx", "-batch", "-iex", "set auto-load safe-path /"]).args([
            "-ex",
            "set debuginfod enabled off",
            "-ex",
            "info auto-load python-scripts",
        ]);
        command.arg(&program);
        let out = run_with(&mut command, &[], Duration::from_mins(3));
        assert!(!out.timed_out(), "gdb on {}", program.display());
        let printed = out.stdout_text();
        assert!(
            printed.contains("hod-inlined-script-ran") && printed.contains("hod_marker"),
            "{}: gdb did not run the script:\n{printed}\n{}",
            linker.name,
            out.stderr_text()
        );
    }
}
