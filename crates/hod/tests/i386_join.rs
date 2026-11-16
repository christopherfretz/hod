//! The i386 packages together (design 0008 I7, I8, I13; the join of
//! WP-298, WP-299 and WP-301): 64-bit integers in globals of every kind
//! and in thread-locals of every model, their divisions through libgcc
//! (`-lgcc_s`, so that the calls go through a real PLT under PIC), and
//! inline assembly with `A` (`edx:eax`) operands and memory operands that
//! are globals and thread-locals. Programs under `tests/data/i386/join/`:
//! `wide.c`, `asm.c` (C through clang, no `target-cpu` and
//! `-march=pentium4`) and `join.ll` (by hand, symbols as operands).
//!
//! Each is built three ways: a position-dependent program (`-fno-pic`,
//! static model), a PIE (`-fPIE`, `-relocation-model=pic`) and a shared
//! library (`-fPIC -Dmain=prog_main`) behind `pic/driver.c`; every one
//! against `ext.c` as a shared library (built by gcc). `llc`'s object
//! (19 and 22), hod's and hod's under `hod_cg::Stress::ALL`, linked by
//! GNU ld, lld and mold, run under `qemu-i386`: every run equals `llc`'s.
//!
//! A missing tool is skipped with a note, unless `HOD_REQUIRE_TOOLS=1`.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

use hod::{Options, OutputKind, RelocModel};
use hod_testutil::target::Target;
use hod_testutil::{LLVM_MAJORS, Scratch, linkers, tools_required};

mod i386_support;
use i386_support::{
    TRIPLE, back_end_object, checked, clang_module, cpu_features, llc_object, llvm_tool,
    run_with_features, tool,
};

/// The programs (C, or IR written by hand).
const PROGRAMS: [&str; 3] = ["wide.c", "join.ll", "asm.c"];

/// The CPU variants: none (`generic`) and `pentium4`.
const CPUS: [(&str, &[&str]); 2] = [("generic", &[]), ("p4", &["-march=pentium4"])];

/// How a program is built.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// A position-dependent program.
    Exe,
    /// A PIE.
    Pie,
    /// A shared library behind `pic/driver.c`.
    Shared,
}

const KINDS: [(Kind, &str); 3] = [(Kind::Exe, "exe"), (Kind::Pie, "pie"), (Kind::Shared, "so")];

impl Kind {
    fn pic(self) -> bool {
        self != Kind::Exe
    }

    fn clang_flags(self) -> &'static [&'static str] {
        match self {
            Kind::Exe => &["-fno-pic", "-fno-pie"],
            Kind::Pie => &["-fPIE"],
            Kind::Shared => &["-fPIC", "-Dmain=prog_main"],
        }
    }
}

fn data(path: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/i386").join(path)
}

/// hod's object of `text` through the facade, and `hod_x86`'s under
/// `Stress::ALL`.
fn hod_objects(text: &[u8], pic: bool, label: &str) -> (Vec<u8>, Vec<u8>) {
    let options = Options {
        output_kind: OutputKind::Object,
        triple: Some(TRIPLE.into()),
        relocation_model: Some(if pic { RelocModel::Pic } else { RelocModel::Static }),
        ..Options::default()
    };
    let plain = hod::compile(text, &options).unwrap_or_else(|e| panic!("hod {label}: {e}"));
    let stress = hod_cg::Options {
        reloc_model: if pic { hod_cg::RelocModel::Pic } else { hod_cg::RelocModel::Static },
        stress: hod_cg::Stress::ALL,
        ..hod_cg::Options::default()
    };
    (plain, back_end_object(text, &stress))
}

/// Whether `program` is IR written by hand (`.ll`).
fn is_ir(program: &str) -> bool {
    Path::new(program).extension().is_some_and(|e| e == "ll")
}

/// The module of `program` for `kind` and `cpu_flags`, as `<label>.ll`.
fn module(
    scratch: &Scratch,
    clang: &Path,
    program: &str,
    kind: Kind,
    cpu_flags: &[&str],
    label: &str,
) -> PathBuf {
    if is_ir(program) {
        let mut text = std::fs::read_to_string(data(&format!("join/{program}"))).unwrap();
        if kind == Kind::Shared {
            text = text.replace("@main(", "@prog_main(");
        }
        if !cpu_flags.is_empty() {
            text =
                text.replace("attributes #0 = {", "attributes #0 = { \"target-cpu\"=\"pentium4\"");
        }
        return scratch.write(&format!("{label}.ll"), text);
    }
    let mut flags = kind.clang_flags().to_vec();
    flags.extend_from_slice(cpu_flags);
    clang_module(scratch, clang, &data(&format!("join/{program}")), &flags, label)
}

struct Tools {
    gcc: PathBuf,
    qemu: PathBuf,
    linkers: Vec<(&'static str, PathBuf)>,
}

fn tools() -> Option<Tools> {
    let target = Target::I686;
    let (Some(gcc), Some(qemu)) = (tool(&target.c_driver()), tool(&target.emulator())) else {
        return None;
    };
    let linkers = linkers::present(target);
    for name in ["bfd", "lld", "mold"] {
        assert!(
            !tools_required() || linkers.iter().any(|(n, _)| *n == name),
            "linker {name} missing (HOD_REQUIRE_TOOLS=1)"
        );
    }
    Some(Tools { gcc, qemu, linkers })
}

impl Tools {
    /// Links `inputs` with `flags` by `linker` into `out`; libgcc's shared
    /// library first, so that `__divdi3` and its kin are called through
    /// the PLT.
    fn link(
        &self,
        scratch: &Scratch,
        linker: &(&str, PathBuf),
        flags: &[&str],
        inputs: &[&Path],
        out: &Path,
    ) {
        let mut command = Command::new(&self.gcc);
        let driver: Vec<OsString> = linkers::driver_flags(scratch.path(), linker.0, &linker.1);
        command.args(driver).args(flags).args(inputs).args(["-lgcc_s", "-pthread"]);
        command.arg("-o").arg(out);
        checked(&format!("{} link of {}", linker.0, out.display()), &mut command);
    }
}

/// Module-level assembly on i386: text of symbol directives only compiles
/// (`hod_cg`'s WP-075 path, which writes no code); text with code or data
/// is refused by name (`module:asm`: `hod_asm` has no 32-bit module
/// assembler yet), never assembled as another architecture's.
#[test]
fn module_asm_is_directives_only_on_i386() {
    let options = Options {
        output_kind: OutputKind::Object,
        triple: Some(TRIPLE.into()),
        ..Options::default()
    };
    let head = "target triple = \"i686-unknown-linux-gnu\"\n";
    let body = "define i32 @f() {\n  ret i32 1\n}\n";
    let symbols = format!("{head}module asm \".globl f\"\nmodule asm \".hidden f\"\n{body}");
    hod::compile(symbols.as_bytes(), &options).unwrap_or_else(|e| panic!("directives: {e}"));
    for text in ["g: movl $5, %eax", ".section .rodata", ".long 7"] {
        let module = format!("{head}module asm \"{text}\"\n{body}");
        let error = hod::compile(module.as_bytes(), &options).unwrap_err().to_string();
        assert!(error.contains("module:asm"), "{text}: {error}");
    }
}

/// Every program, kind and CPU, by each compiler and linker.
#[test]
fn the_packages_run_together_as_llc_s() {
    let Some(tools) = tools() else { return };
    let scratch = Scratch::new("i386-join");
    let driver = scratch.join("driver.o");
    let mut command = Command::new(&tools.gcc);
    command.args(["-c", "-fPIE"]).arg(data("pic/driver.c")).arg("-o").arg(&driver);
    checked("driver", &mut command);
    let mut exts = Vec::new();
    for linker in &tools.linkers {
        let ext = scratch.join(format!("libext-{}.so", linker.0));
        let mut command = Command::new(&tools.gcc);
        let flags: Vec<OsString> = linkers::driver_flags(scratch.path(), linker.0, &linker.1);
        command.args(flags).args(["-shared", "-fPIC"]).arg(data("join/ext.c")).arg("-o").arg(&ext);
        checked(&format!("libext ({})", linker.0), &mut command);
        exts.push(ext);
    }
    let (mut programs, mut runs) = (0, 0);
    for major in LLVM_MAJORS {
        let (Some(clang), Some(llc)) = (llvm_tool(major, "clang"), llvm_tool(major, "llc")) else {
            continue;
        };
        for program in PROGRAMS {
            for (cpu, cpu_flags) in CPUS {
                for (kind, kind_name) in KINDS {
                    let label = format!("{program}-{major}-{cpu}-{kind_name}");
                    let ll = module(&scratch, &clang, program, kind, cpu_flags, &label);
                    let text = std::fs::read(&ll).unwrap();
                    let (plain, stress) = hod_objects(&text, kind.pic(), &label);
                    let reloc: &[&str] = if kind.pic() { &["-relocation-model=pic"] } else { &[] };
                    let objects = [
                        ("llc", llc_object(&scratch, &llc, &ll, reloc, &format!("{label}-llc"))),
                        ("hod", scratch.write(&format!("{label}-hod.o"), plain)),
                        ("stress", scratch.write(&format!("{label}-stress.o"), stress)),
                    ];
                    programs += 1;
                    for (linker, ext) in tools.linkers.iter().zip(&exts) {
                        let mut results = Vec::new();
                        for (who, object) in &objects {
                            let tag = format!("{label}-{who}-{}", linker.0);
                            let exe = scratch.join(&tag);
                            match kind {
                                Kind::Exe | Kind::Pie => {
                                    let flag = if kind == Kind::Exe { "-no-pie" } else { "-pie" };
                                    tools.link(&scratch, linker, &[flag], &[object, ext], &exe);
                                }
                                Kind::Shared => {
                                    let lib = scratch.join(format!("lib{tag}.so"));
                                    tools.link(
                                        &scratch,
                                        linker,
                                        &["-shared"],
                                        &[object, ext],
                                        &lib,
                                    );
                                    let inputs = [driver.as_path(), &lib, ext];
                                    tools.link(&scratch, linker, &["-pie"], &inputs, &exe);
                                }
                            }
                            let result = run_with_features(&tools.qemu, &exe, cpu_features(cpu));
                            results.push((*who, result));
                        }
                        let (_, reference) = &results[0];
                        assert!(
                            reference.1.ends_with(b"done\n") || is_ir(program),
                            "{label} ({}): llc's run did not finish: {}",
                            linker.0,
                            String::from_utf8_lossy(&reference.1)
                        );
                        assert_eq!(reference.0, Some(0), "{label} ({}): llc's status", linker.0);
                        for (who, result) in &results[1..] {
                            assert_eq!(
                                reference,
                                result,
                                "{label} ({}): {who}'s run differs from llc's\nllc: {}\n{who}: {}",
                                linker.0,
                                String::from_utf8_lossy(&reference.1),
                                String::from_utf8_lossy(&result.1)
                            );
                        }
                        runs += 1;
                    }
                }
            }
        }
    }
    eprintln!("i386 join: {programs} programs, {runs} runs (llc, hod, stress each) equal llc's");
}
