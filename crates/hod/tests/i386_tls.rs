//! WP-299, design 0008's I8: i686 thread-local storage. `tls/tls.c` and
//! `tls/other.c` built with each `-ftls-model=` into a position-dependent
//! program (static relocation model: local exec and `@INDNTPOFF` initial
//! exec) and a PIE (`@GOTNTPOFF` initial exec), and `other.c` as a shared
//! library in the initial-exec model; four threads kept alive together by
//! a barrier. Every link by GNU ld, lld and mold (which relax initial exec
//! to local exec where the variable is the program's), every run under
//! `qemu-i386` equal to `llc`'s (19 and 22), hod's objects also under
//! `hod_cg::Stress::ALL`. The dynamic models (a shared library's default)
//! are refused as `x86:tls:dynamic` (`spec/codegen-x86.md`).
//!
//! A missing tool is skipped with a note, unless `HOD_REQUIRE_TOOLS=1`.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

use hod::{CompileError, Options, OutputKind, Unsupported};
use hod_testutil::target::Target;
use hod_testutil::{LLVM_MAJORS, Scratch, linkers, tools_required};

mod i386_support;
use i386_support::{
    TRIPLE, back_end_object, checked, clang_module, llc_object, llvm_tool, run, tool,
};

fn data(path: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/i386/tls").join(path)
}

fn reloc_model(pic: bool) -> hod::RelocModel {
    if pic { hod::RelocModel::Pic } else { hod::RelocModel::Static }
}

/// hod's object of `text`.
fn hod_object(text: &[u8], pic: bool) -> Result<Vec<u8>, CompileError> {
    let options = Options {
        output_kind: OutputKind::Object,
        triple: Some(TRIPLE.into()),
        relocation_model: Some(reloc_model(pic)),
        ..Options::default()
    };
    hod::compile(text, &options)
}

/// `llc`'s, hod's and hod's stressed objects of `ll`.
fn objects(scratch: &Scratch, llc: &Path, ll: &Path, pic: bool, label: &str) -> [PathBuf; 3] {
    let flags: &[&str] = if pic { &["-relocation-model=pic"] } else { &[] };
    let llc_o = llc_object(scratch, llc, ll, flags, &format!("{label}-llc"));
    let text = std::fs::read(ll).unwrap();
    let hod = hod_object(&text, pic).unwrap_or_else(|e| panic!("hod {label}: {e}"));
    let options = hod_cg::Options {
        reloc_model: if pic { hod_cg::RelocModel::Pic } else { hod_cg::RelocModel::Static },
        stress: hod_cg::Stress::ALL,
        ..hod_cg::Options::default()
    };
    let stressed = back_end_object(&text, &options);
    [
        llc_o,
        scratch.write(&format!("{label}-hod.o"), hod),
        scratch.write(&format!("{label}-stress.o"), stressed),
    ]
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
        command.args(driver).args(flags).args(inputs).arg("-pthread").arg("-o").arg(out);
        checked(&format!("{} link of {}", linker.0, out.display()), &mut command);
    }
}

const WHO: [&str; 3] = ["llc", "hod", "stress"];

fn all_equal(results: &[(Option<i32>, Vec<u8>)], what: &str) {
    for (who, result) in WHO.iter().zip(results).skip(1) {
        assert_eq!(
            &results[0],
            result,
            "{what}: {who}'s run differs from llc's\nllc: {}\n{who}: {}",
            String::from_utf8_lossy(&results[0].1),
            String::from_utf8_lossy(&result.1)
        );
    }
}

/// Each model in a position-dependent program and in a PIE.
#[test]
fn every_model_runs_in_programs_as_llc_s() {
    let Some(tools) = tools() else { return };
    let scratch = Scratch::new("i386-tls");
    let mut runs = 0;
    for major in LLVM_MAJORS {
        let (Some(clang), Some(llc)) = (llvm_tool(major, "clang"), llvm_tool(major, "llc")) else {
            continue;
        };
        for model in ["local-exec", "initial-exec", "global-dynamic", "local-dynamic"] {
            for pie in [false, true] {
                let label = format!("{major}-{model}-{}", if pie { "pie" } else { "exe" });
                let tls_model = format!("-ftls-model={model}");
                let pic_flags: [&str; 2] =
                    if pie { ["-fPIE", "-pie"] } else { ["-fno-pic", "-fno-pie"] };
                let flags = [pic_flags[0], tls_model.as_str()];
                let main =
                    clang_module(&scratch, &clang, &data("tls.c"), &flags, &format!("tls-{label}"));
                let other = clang_module(
                    &scratch,
                    &clang,
                    &data("other.c"),
                    &flags,
                    &format!("other-{label}"),
                );
                let mains = objects(&scratch, &llc, &main, pie, &format!("tls-{label}"));
                let others = objects(&scratch, &llc, &other, pie, &format!("other-{label}"));
                let link_flag = if pie { "-pie" } else { "-no-pie" };
                for linker in &tools.linkers {
                    let mut results = Vec::new();
                    for (who, (m, o)) in WHO.iter().zip(mains.iter().zip(&others)) {
                        let exe = scratch.join(format!("{label}-{who}-{}", linker.0));
                        tools.link(&scratch, linker, &[link_flag], &[m, o], &exe);
                        results.push(run(&tools.qemu, &exe));
                    }
                    assert_eq!(
                        results[0].0,
                        Some(16),
                        "{}",
                        String::from_utf8_lossy(&results[0].1)
                    );
                    all_equal(&results, &format!("{label} ({})", linker.0));
                    runs += 1;
                }
            }
        }
    }
    eprintln!("i386 TLS: {runs} program runs equal llc's");
}

/// `other.c` as a shared library in the initial-exec model, `tls.c` as
/// the PIE that loads it; the library's default model (general dynamic)
/// is refused by name.
#[test]
fn a_shared_library_in_initial_exec_runs_and_dynamic_models_are_refused() {
    let Some(tools) = tools() else { return };
    let scratch = Scratch::new("i386-tls-so");
    let mut runs = 0;
    for major in LLVM_MAJORS {
        let (Some(clang), Some(llc)) = (llvm_tool(major, "clang"), llvm_tool(major, "llc")) else {
            continue;
        };
        let main =
            clang_module(&scratch, &clang, &data("tls.c"), &["-fPIE"], &format!("tls-{major}"));
        let flags = ["-fPIC", "-ftls-model=initial-exec"];
        let other =
            clang_module(&scratch, &clang, &data("other.c"), &flags, &format!("other-{major}"));
        let mains = objects(&scratch, &llc, &main, true, &format!("tls-{major}"));
        let others = objects(&scratch, &llc, &other, true, &format!("other-{major}"));
        for linker in &tools.linkers {
            let mut results = Vec::new();
            for (who, (m, o)) in WHO.iter().zip(mains.iter().zip(&others)) {
                let tag = format!("{major}-{who}-{}", linker.0);
                let lib = scratch.join(format!("libother-{tag}.so"));
                tools.link(&scratch, linker, &["-shared"], &[o], &lib);
                let exe = scratch.join(format!("tls-so-{tag}"));
                tools.link(&scratch, linker, &["-pie"], &[m, &lib], &exe);
                results.push(run(&tools.qemu, &exe));
            }
            assert_eq!(results[0].0, Some(16), "{}", String::from_utf8_lossy(&results[0].1));
            all_equal(&results, &format!("shared {major} ({})", linker.0));
            runs += 1;
        }
        for model in ["global-dynamic", "local-dynamic"] {
            let flags = ["-fPIC", &format!("-ftls-model={model}")];
            let ll = clang_module(
                &scratch,
                &clang,
                &data("other.c"),
                &flags,
                &format!("dyn-{major}-{model}"),
            );
            let result = hod_object(&std::fs::read(&ll).unwrap(), true);
            let Err(CompileError::Unsupported(Unsupported { construct, .. })) = &result else {
                panic!("{model}: {result:?}");
            };
            assert_eq!(construct.name(), "x86:tls:dynamic", "{model}");
        }
    }
    eprintln!("i386 TLS shared library: {runs} runs equal llc's");
}

/// `llvm.thread.pointer` is `mov rd, gs:[0]`; `threadlocal.address` of a
/// local-exec variable is the thread pointer plus `R_386_TLS_LE`.
#[test]
fn the_thread_pointer_is_gs_zero() {
    let text = format!(
        "target triple = \"{TRIPLE}\"\n@v = thread_local(localexec) global i32 1\n\
         define ptr @tp() {{\n  %p = call ptr @llvm.thread.pointer()\n  ret ptr %p\n}}\n\
         define ptr @addr() {{\n  %p = call ptr @llvm.threadlocal.address.p0(ptr @v)\n  ret ptr %p\n}}\n\
         declare ptr @llvm.thread.pointer()\ndeclare ptr @llvm.threadlocal.address.p0(ptr)\n"
    );
    for pic in [false, true] {
        let object = hod_object(text.as_bytes(), pic).unwrap_or_else(|e| panic!("{e}"));
        let tp = [0x65, 0xa1, 0, 0, 0, 0]; // mov eax, gs:[0]
        let gs_load = |o: &[u8]| o.windows(3).any(|w| w[0] == 0x65 && w[1] == 0x8b);
        assert!(object.windows(6).any(|w| w == tp) || gs_load(&object), "pic {pic}");
    }
}
