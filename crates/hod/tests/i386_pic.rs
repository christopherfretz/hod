//! WP-299, design 0008's I8: position-independent i686 code. The five I5
//! programs (`tests/data/i386/*.c`) compiled by clang `-fPIE` and `-fPIC`
//! and by `llc`/hod with `-relocation-model=pic`, linked as PIE programs
//! and, built into a shared library with `-Dmain=prog_main` behind a
//! driver, as a program plus a library; the pair `pic/main.c` (PIE) and
//! `pic/lib.c` (shared library) both built by each compiler. Every link
//! by GNU ld, lld and mold, every run under `qemu-i386` equal to `llc`'s
//! (19 and 22), hod's objects also under `hod_cg::Stress::ALL`; no output
//! has `DT_TEXTREL`; the relocation kinds per symbol equal `llc`'s.
//!
//! A missing tool is skipped with a note, unless `HOD_REQUIRE_TOOLS=1`.

use std::collections::BTreeSet;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

use hod::{Options, OutputKind};
use hod_testutil::target::Target;
use hod_testutil::{LLVM_MAJORS, Scratch, linkers, tools_required};

mod i386_support;
use i386_support::{
    TRIPLE, back_end_object, checked, clang_module, llc_object, llvm_tool, run, tool,
};

/// The I5 programs of `tests/data/i386`.
const PROGRAMS: [&str; 5] = ["loops.c", "switch.c", "calls.c", "memory.c", "bytes.c"];

fn data(path: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/i386").join(path)
}

/// hod's object of `ll` under `-relocation-model=pic`, by the facade.
fn hod_pic(ll: &Path) -> Vec<u8> {
    let options = Options {
        output_kind: OutputKind::Object,
        triple: Some(TRIPLE.into()),
        relocation_model: Some(hod::RelocModel::Pic),
        ..Options::default()
    };
    hod::compile(&std::fs::read(ll).unwrap(), &options)
        .unwrap_or_else(|e| panic!("hod {}: {e}", ll.display()))
}

/// `hod_x86`'s PIC object of `ll` under `Stress::ALL`.
fn stressed_pic(ll: &Path) -> Vec<u8> {
    let options = hod_cg::Options {
        reloc_model: hod_cg::RelocModel::Pic,
        stress: hod_cg::Stress::ALL,
        ..hod_cg::Options::default()
    };
    back_end_object(&std::fs::read(ll).unwrap(), &options)
}

/// The three objects of one module: `llc`'s, hod's, hod's stressed.
fn objects(scratch: &Scratch, llc: &Path, ll: &Path, label: &str) -> [(&'static str, PathBuf); 3] {
    let llc_o = llc_object(scratch, llc, ll, &["-relocation-model=pic"], &format!("{label}-llc"));
    let hod_o = scratch.write(&format!("{label}-hod.o"), hod_pic(ll));
    let stress_o = scratch.write(&format!("{label}-stress.o"), stressed_pic(ll));
    [("llc", llc_o), ("hod", hod_o), ("stress", stress_o)]
}

/// The tools every test here needs.
struct Tools {
    gcc: PathBuf,
    qemu: PathBuf,
    readelf: Option<PathBuf>,
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
    let readelf = LLVM_MAJORS.iter().rev().find_map(|&m| llvm_tool(m, "llvm-readelf"));
    Some(Tools { gcc, qemu, readelf, linkers })
}

impl Tools {
    /// Links `inputs` with `flags` by `linker` into `out`.
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
        command.args(driver).args(flags).args(inputs).arg("-o").arg(out);
        checked(&format!("{} link of {}", linker.0, out.display()), &mut command);
        self.no_textrel(out);
    }

    /// `out` has no `DT_TEXTREL` (and no `DF_TEXTREL`).
    fn no_textrel(&self, out: &Path) {
        let Some(readelf) = &self.readelf else { return };
        let outcome = checked("llvm-readelf -d", Command::new(readelf).arg("-d").arg(out));
        let text = String::from_utf8_lossy(&outcome.stdout);
        assert!(!text.contains("TEXTREL"), "{}: text relocations\n{text}", out.display());
    }

    /// The (kind, symbol) pairs of `object`'s relocations outside
    /// `.eh_frame` (hod writes no unwind tables for i686 yet: I10), every
    /// local symbol as `<local>`: `llc` names a local symbol by its
    /// section where hod keeps the symbol, the same relocation.
    fn reloc_kinds(&self, object: &Path) -> BTreeSet<(String, String)> {
        let Some(readelf) = &self.readelf else { return BTreeSet::new() };
        let symbols = checked("llvm-readelf -s", Command::new(readelf).arg("-s").arg(object));
        let globals: BTreeSet<String> = String::from_utf8_lossy(&symbols.stdout)
            .lines()
            .filter_map(|line| {
                let f: Vec<&str> = line.split_whitespace().collect();
                (f.len() >= 8 && matches!(f[4], "GLOBAL" | "WEAK")).then(|| f[7].to_owned())
            })
            .collect();
        let outcome = checked("llvm-readelf -r", Command::new(readelf).arg("-r").arg(object));
        let mut kinds = BTreeSet::new();
        let mut skip = false;
        for line in String::from_utf8_lossy(&outcome.stdout).lines() {
            if line.starts_with("Relocation section") {
                skip = line.contains(".eh_frame");
                continue;
            }
            let fields: Vec<&str> = line.split_whitespace().collect();
            if !skip && fields.len() >= 3 && fields[2].starts_with("R_386_") {
                let sym = fields.get(4).copied().unwrap_or("");
                let sym = if globals.contains(sym) { sym } else { "<local>" };
                // A call of a local function in the same section: `llc`'s
                // assembler resolves it, hod leaves `R_386_PC32` (WP-293).
                if fields[2] == "R_386_PC32" && sym == "<local>" {
                    continue;
                }
                // `llc` calls these for `llvm.mem*`, hod copies inline.
                if matches!(sym, "memcpy" | "memmove" | "memset") {
                    continue;
                }
                kinds.insert((fields[2].to_owned(), sym.to_owned()));
            }
        }
        kinds
    }
}

/// Asserts that every run of `results` (llc's first) is llc's.
/// One run: who built it, and its status and output.
type Run<'a> = (&'a str, (Option<i32>, Vec<u8>));

fn all_equal(results: &[Run<'_>], what: &str) {
    let (_, reference) = &results[0];
    for (who, result) in &results[1..] {
        assert_eq!(
            reference,
            result,
            "{what}: {who}'s run differs from llc's\nllc: {}\n{who}: {}",
            String::from_utf8_lossy(&reference.1),
            String::from_utf8_lossy(&result.1)
        );
    }
}

/// The I5 programs as PIE programs (`-fPIE` and `-fPIC` modules) and as
/// shared libraries behind a driver; the relocation kinds equal llc's.
#[test]
fn i5_programs_run_as_pie_and_from_shared_libraries() {
    let Some(tools) = tools() else { return };
    let scratch = Scratch::new("i386-pic");
    let driver = scratch.join("driver.o");
    let mut command = Command::new(&tools.gcc);
    command.args(["-c", "-fPIE"]).arg(data("pic/driver.c")).arg("-o").arg(&driver);
    checked("driver", &mut command);
    let (mut runs, mut tables) = (0, 0);
    for major in LLVM_MAJORS {
        let (Some(clang), Some(llc)) = (llvm_tool(major, "clang"), llvm_tool(major, "llc")) else {
            continue;
        };
        for program in PROGRAMS {
            for flavour in ["-fPIE", "-fPIC"] {
                let label = format!("{program}-{major}{flavour}");
                let mut flags = vec![flavour];
                if flavour == "-fPIC" {
                    flags.push("-Dmain=prog_main");
                }
                let ll = clang_module(&scratch, &clang, &data(program), &flags, &label);
                let objects = objects(&scratch, &llc, &ll, &label);
                let llc_kinds = tools.reloc_kinds(&objects[0].1);
                for (who, object) in &objects[1..] {
                    let kinds = tools.reloc_kinds(object);
                    assert_eq!(llc_kinds, kinds, "{label}: {who}'s relocation kinds");
                    tables += 1;
                }
                for linker in &tools.linkers {
                    let mut results = Vec::new();
                    for (who, object) in &objects {
                        let exe = scratch.join(format!("{label}-{who}-{}", linker.0));
                        if flavour == "-fPIE" {
                            tools.link(&scratch, linker, &["-pie"], &[object], &exe);
                        } else {
                            let lib = scratch.join(format!("lib{label}-{who}-{}.so", linker.0));
                            tools.link(&scratch, linker, &["-shared"], &[object], &lib);
                            tools.link(&scratch, linker, &["-pie"], &[&driver, &lib], &exe);
                        }
                        results.push((*who, run(&tools.qemu, &exe)));
                    }
                    all_equal(&results, &format!("{label} ({})", linker.0));
                    runs += 1;
                }
            }
        }
    }
    eprintln!("i386 PIC: {runs} program runs equal llc's; {tables} relocation tables equal llc's");
}

/// `pic/main.c` as a PIE against `pic/lib.c` as a shared library, both
/// built by each compiler (and hod's program against llc's library and
/// the reverse).
#[test]
fn a_program_and_its_shared_library_run_as_llc_s() {
    let Some(tools) = tools() else { return };
    let scratch = Scratch::new("i386-pic-pair");
    let mut runs = 0;
    for major in LLVM_MAJORS {
        let (Some(clang), Some(llc)) = (llvm_tool(major, "clang"), llvm_tool(major, "llc")) else {
            continue;
        };
        let main_ll = clang_module(
            &scratch,
            &clang,
            &data("pic/main.c"),
            &["-fPIE"],
            &format!("main-{major}"),
        );
        let lib_ll =
            clang_module(&scratch, &clang, &data("pic/lib.c"), &["-fPIC"], &format!("lib-{major}"));
        let mains = objects(&scratch, &llc, &main_ll, &format!("main-{major}"));
        let libs = objects(&scratch, &llc, &lib_ll, &format!("lib-{major}"));
        for (objects, what) in [(&mains, "main"), (&libs, "lib")] {
            let llc_kinds = tools.reloc_kinds(&objects[0].1);
            for (who, object) in &objects[1..] {
                assert_eq!(llc_kinds, tools.reloc_kinds(object), "{what} {major}: {who}'s kinds");
            }
        }
        for linker in &tools.linkers {
            let mut results = Vec::new();
            for (m, (who, main_o)) in mains.iter().enumerate() {
                for (l, (lib_who, lib_o)) in libs.iter().enumerate() {
                    // Each compiler with itself, and hod's with llc's.
                    if m != l && m * l != 0 {
                        continue;
                    }
                    let tag = format!("{major}-{who}-{lib_who}-{}", linker.0);
                    let lib = scratch.join(format!("libpair-{tag}.so"));
                    tools.link(&scratch, linker, &["-shared"], &[lib_o], &lib);
                    let exe = scratch.join(format!("pair-{tag}"));
                    tools.link(&scratch, linker, &["-pie"], &[main_o, &lib], &exe);
                    results.push((*who, run(&tools.qemu, &exe)));
                }
            }
            assert_eq!(results[0].1.0, Some(100), "{}", String::from_utf8_lossy(&results[0].1.1));
            all_equal(&results, &format!("pair {major} ({})", linker.0));
            runs += results.len();
        }
    }
    eprintln!("i386 PIC pair: {runs} runs equal llc's");
}
