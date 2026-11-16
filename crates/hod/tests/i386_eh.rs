//! WP-314, design 0008's I10: exceptions on i686 (`spec/codegen-x86.md`
//! §12). C++ programs of `tests/data/i386/eh` compiled by clang 19 and
//! 22 (`--target=i686-linux-gnu`) and the hand-written `eh/live.ll`, built
//! by `llc`, by hod and by hod under `hod_cg::Stress::ALL`; static
//! (`-fno-pic`, a non-PIE program), PIE (`-fPIE`) and, at CPU `i586` (no
//! `cmov`), static again; a throw across a shared library boundary
//! (`eh/lib.cpp` as a library, `eh/main.cpp` as a PIE, with a dynamic
//! thread-local variable in the catching frame). Every link by GNU ld, lld
//! and mold, every run under `qemu-i386` equal to `llc`'s.
//!
//! hod's `.eh_frame` passes `llvm-dwarfdump --verify` (19 and 22), and is
//! compared with `llc`'s after decoding: the same CIEs, the same number of
//! FDEs and of FDEs with an LSDA, and per FDE the same rules once the
//! advances and the rows of `ebx`, `esi`, `edi` are left out (hod saves
//! all three in every frame, `llc` those it uses); the relocation kinds of
//! the tables (`R_386_PC32` to the `DW.ref` slot, to the type-info stubs,
//! to the LSDAs and the code) and the slot's shape are `llc`'s under PIC.
//!
//! A missing tool is skipped with a note, unless `HOD_REQUIRE_TOOLS=1`.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

use hod_testutil::target::Target;
use hod_testutil::{LLVM_MAJORS, Scratch, linkers, tools_required};

mod i386_support;
use i386_support::{back_end_object, checked, clang_module, llc_object, llvm_tool, run, tool};

fn data(path: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/i386/eh").join(path)
}

/// The tools every test here needs.
struct Tools {
    gcc: PathBuf,
    qemu: PathBuf,
    linkers: Vec<(&'static str, PathBuf)>,
}

fn tools() -> Option<Tools> {
    let target = Target::I686;
    let (Some(gcc), Some(qemu)) = (tool(&target.c_driver()), tool(&target.emulator())) else {
        assert!(!tools_required(), "i686 gcc or qemu-i386 missing (HOD_REQUIRE_TOOLS=1)");
        eprintln!("note: no i686 gcc or qemu-i386; skipping");
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
    /// Links the C++ `inputs` with `flags` by `linker` into `out`.
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
        command.args(driver).args(flags).args(inputs).args(["-lstdc++", "-lm"]);
        command.arg("-o").arg(out);
        checked(&format!("{} link of {}", linker.0, out.display()), &mut command);
    }
}

/// hod's object of `ll` under `model`, by the back end, stressed or not.
fn hod_object(ll: &Path, model: hod_cg::RelocModel, stress: bool) -> Vec<u8> {
    let stress = if stress { hod_cg::Stress::ALL } else { hod_cg::Stress::default() };
    let options = hod_cg::Options { reloc_model: model, stress, ..hod_cg::Options::default() };
    back_end_object(&std::fs::read(ll).unwrap(), &options)
}

/// The three objects of `ll`: `llc`'s, hod's, hod's stressed.
fn objects(
    scratch: &Scratch,
    llc: &Path,
    ll: &Path,
    pic: bool,
    label: &str,
) -> [(&'static str, PathBuf); 3] {
    let (flag, model) = if pic {
        ("-relocation-model=pic", hod_cg::RelocModel::Pic)
    } else {
        ("-relocation-model=static", hod_cg::RelocModel::Static)
    };
    let llc_o = llc_object(scratch, llc, ll, &[flag], &format!("{label}-llc"));
    let hod_o = scratch.write(&format!("{label}-hod.o"), hod_object(ll, model, false));
    let stress_o = scratch.write(&format!("{label}-stress.o"), hod_object(ll, model, true));
    [("llc", llc_o), ("hod", hod_o), ("stress", stress_o)]
}

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

/// The variants of a module: a label, clang's flags, `llc`/hod under PIC.
const VARIANTS: [(&str, &[&str], bool); 3] = [
    ("static", &["-fno-pic"], false),
    ("pie", &["-fPIE"], true),
    ("i586", &["-fno-pic", "-march=i586"], false),
];

/// The C++ programs, each with its clang flags.
const PROGRAMS: [(&str, &[&str]); 2] = [("basic.cpp", &["-std=c++14"]), ("terminate.cpp", &[])];

/// §12: the programs and `live.ll`, every variant, every linker.
#[test]
fn exceptions_run_as_llc_s() {
    let Some(tools) = tools() else { return };
    let scratch = Scratch::new("i386-eh");
    let mut runs = 0;
    for major in LLVM_MAJORS {
        let (Some(clang), Some(llc)) = (llvm_tool(major, "clang"), llvm_tool(major, "llc")) else {
            assert!(!tools_required(), "LLVM {major} missing (HOD_REQUIRE_TOOLS=1)");
            continue;
        };
        let mut modules: Vec<(String, PathBuf, bool)> = Vec::new();
        for (program, extra) in PROGRAMS {
            for (variant, flags, pic) in VARIANTS {
                let label = format!("{program}-{major}-{variant}");
                let flags: Vec<&str> = flags.iter().chain(extra).copied().collect();
                let ll = clang_module(&scratch, &clang, &data(program), &flags, &label);
                modules.push((label, ll, pic));
            }
        }
        modules.push((format!("live-{major}-static"), data("live.ll"), false));
        modules.push((format!("live-{major}-pie"), data("live.ll"), true));
        for (label, ll, pic) in &modules {
            let objects = objects(&scratch, &llc, ll, *pic, label);
            for linker in &tools.linkers {
                let mut results = Vec::new();
                for (who, object) in &objects {
                    let exe = scratch.join(format!("{label}-{who}-{}", linker.0));
                    let flags: &[&str] = if *pic { &["-pie"] } else { &["-no-pie"] };
                    tools.link(&scratch, linker, flags, &[object], &exe);
                    results.push((*who, run(&tools.qemu, &exe)));
                }
                all_equal(&results, &format!("{label} ({})", linker.0));
                runs += 1;
            }
        }
    }
    eprintln!("i386 EH: {runs} program runs (llc, hod, hod stressed) equal");
}

/// §12: a throw across a shared library boundary, both ways, with a
/// general-dynamic thread-local variable in the catching frame; each
/// compiler's library with each compiler's program.
#[test]
fn exceptions_cross_a_shared_library() {
    let Some(tools) = tools() else { return };
    let scratch = Scratch::new("i386-eh-so");
    let mut runs = 0;
    for major in LLVM_MAJORS {
        let (Some(clang), Some(llc)) = (llvm_tool(major, "clang"), llvm_tool(major, "llc")) else {
            continue;
        };
        let lib_ll = clang_module(&scratch, &clang, &data("lib.cpp"), &["-fPIC"], "lib");
        let main_ll = clang_module(&scratch, &clang, &data("main.cpp"), &["-fPIE"], "main");
        let libs = objects(&scratch, &llc, &lib_ll, true, &format!("lib-{major}"));
        let mains = objects(&scratch, &llc, &main_ll, true, &format!("main-{major}"));
        for linker in &tools.linkers {
            let mut results = Vec::new();
            for ((who, lib_o), (_, main_o)) in libs.iter().zip(&mains) {
                let dir = scratch.join(format!("{major}-{who}-{}", linker.0));
                std::fs::create_dir_all(&dir).unwrap();
                let lib = dir.join("libeh.so");
                tools.link(&scratch, linker, &["-shared"], &[lib_o], &lib);
                let exe = dir.join("main");
                let rpath = format!("-Wl,-rpath,{}", dir.display());
                tools.link(&scratch, linker, &["-pie", &rpath], &[main_o, &lib], &exe);
                results.push((*who, run(&tools.qemu, &exe)));
            }
            all_equal(&results, &format!("shared library {major} ({})", linker.0));
            runs += 1;
        }
    }
    eprintln!("i386 EH across a shared library: {runs} runs equal");
}

/// `llvm-dwarfdump --eh-frame` of `object`, its entries split: each CIE
/// or FDE as its header line's kind and its instruction lines.
fn eh_entries(dwarfdump: &Path, object: &Path) -> Vec<(String, Vec<String>)> {
    let outcome = checked("llvm-dwarfdump", Command::new(dwarfdump).arg("--eh-frame").arg(object));
    let mut entries: Vec<(String, Vec<String>)> = Vec::new();
    for line in String::from_utf8_lossy(&outcome.stdout).lines() {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() >= 4 && (fields[3] == "CIE" || fields[3] == "FDE") {
            entries.push((fields[3].to_owned(), Vec::new()));
        } else if let Some((_, lines)) = entries.last_mut() {
            let line = line.trim();
            if !line.is_empty() && !line.starts_with("0x") {
                lines.push(line.to_owned());
            }
        }
    }
    entries
}

/// An FDE's rules up to its first exit (`def_cfa: ESP +4`) without the
/// advances, the `nop`s and the rows of `ebx`, `esi` and `edi`; whether
/// it has an LSDA. The rows after the first exit differ by design: `llc`
/// writes an exit per `ret` and restores the `ebp` rule after each for
/// the code that follows (`def_cfa: EBP +8`), hod has one exit at the
/// end of the function (`spec/codegen-x86.md` §3), also in a function
/// that never returns.
fn fde_shape(lines: &[String]) -> (Vec<String>, bool) {
    let lsda = lines.iter().any(|l| l.starts_with("LSDA Address"));
    let rules = lines
        .iter()
        .filter(|l| l.starts_with("DW_CFA_"))
        .filter(|l| !l.starts_with("DW_CFA_advance_loc") && !l.starts_with("DW_CFA_nop"))
        .filter(|l| !["EBX", "ESI", "EDI"].iter().any(|r| l.contains(&format!(" {r} "))))
        .take_while(|l| !l.starts_with("DW_CFA_def_cfa: ESP +4"))
        .cloned()
        .collect();
    (rules, lsda)
}

/// A summary of `object`'s `.eh_frame`: the CIEs (without the
/// personality's unrelocated address) and the multiset of FDE shapes.
fn eh_summary(dwarfdump: &Path, object: &Path) -> (Vec<Vec<String>>, BTreeMap<String, usize>) {
    let mut cies = Vec::new();
    let mut fdes = BTreeMap::new();
    for (kind, lines) in eh_entries(dwarfdump, object) {
        if kind == "CIE" {
            cies.push(
                lines.into_iter().filter(|l| !l.starts_with("Personality Address")).collect(),
            );
        } else {
            let (rules, lsda) = fde_shape(&lines);
            *fdes.entry(format!("lsda={lsda} {}", rules.join("; "))).or_insert(0) += 1;
        }
    }
    (cies, fdes)
}

/// The relocation kinds and targets of `object`'s unwind sections and of
/// the `DW.ref` slot, local section symbols by kind only.
fn table_relocations(readelf: &Path, object: &Path) -> BTreeMap<String, Vec<String>> {
    let outcome = checked("readelf -rW", Command::new(readelf).arg("-rW").arg(object));
    let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut section = None;
    for line in String::from_utf8_lossy(&outcome.stdout).lines() {
        if let Some(rest) = line.strip_prefix("Relocation section '") {
            let name = rest.split('\'').next().unwrap_or("").to_owned();
            let keep = name.contains("eh_frame")
                || name.contains("gcc_except_table")
                || name.contains("DW.ref");
            section = keep.then_some(name);
            continue;
        }
        let fields: Vec<&str> = line.split_whitespace().collect();
        if let Some(name) = &section
            && fields.len() >= 5
            && fields[2].starts_with("R_386_")
        {
            let sym = fields[4];
            let sym = if sym.starts_with('.') { ".<section>" } else { sym };
            out.entry(name.clone()).or_default().push(format!("{} {sym}", fields[2]));
        }
    }
    for kinds in out.values_mut() {
        kinds.sort();
        kinds.dedup();
    }
    out
}

/// §12: hod's tables verify, and decode to `llc`'s shapes.
#[test]
fn unwind_tables_verify_and_decode_as_llc_s() {
    let readelf = tool("readelf");
    let mut compared = 0;
    for major in LLVM_MAJORS {
        let scratch = Scratch::new("i386-eh-tables");
        let (Some(clang), Some(llc), Some(dwarfdump)) = (
            llvm_tool(major, "clang"),
            llvm_tool(major, "llc"),
            llvm_tool(major, "llvm-dwarfdump"),
        ) else {
            assert!(!tools_required(), "LLVM {major} missing (HOD_REQUIRE_TOOLS=1)");
            continue;
        };
        let basic = clang_module(&scratch, &clang, &data("basic.cpp"), &["-std=c++14"], "basic");
        let lib = clang_module(&scratch, &clang, &data("lib.cpp"), &["-fPIC"], "lib");
        for (label, ll) in [("basic", basic), ("lib", lib), ("live", data("live.ll"))] {
            let [(_, theirs), (_, ours), (_, stressed)] =
                objects(&scratch, &llc, &ll, true, &format!("{label}-{major}"));
            for object in [&ours, &stressed] {
                let verify = Command::new(&dwarfdump).arg("--verify").arg(object).output().unwrap();
                let text = String::from_utf8_lossy(&verify.stdout);
                assert!(verify.status.success() && text.contains("No errors."), "{label}: {text}");
            }
            // `live.ll` has no `"frame-pointer"="all"`: `llc` keeps the
            // CFA on `esp` there, hod always on `ebp` (§3).
            if label != "live" {
                let (cies, fdes) = eh_summary(&dwarfdump, &theirs);
                assert_eq!(eh_summary(&dwarfdump, &ours), (cies.clone(), fdes.clone()), "{label}");
                assert_eq!(eh_summary(&dwarfdump, &stressed), (cies, fdes), "{label} (stress)");
            }
            if let Some(readelf) = &readelf {
                let theirs = table_relocations(readelf, &theirs);
                assert_eq!(table_relocations(readelf, &ours), theirs, "{label}: relocations");
            }
            compared += 1;
        }
    }
    eprintln!("i386 EH: {compared} objects' tables verify and decode as llc's");
}
