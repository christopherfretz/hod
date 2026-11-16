//! The run harness of WP-316's tests (`i386_va.rs`, `i386_atomic.rs`,
//! `i386_dynstack.rs`): a program (C by clang, or IR text) built by `llc`
//! 19 and 22 and by hod (through the facade and under `Stress::ALL`),
//! static or as a PIE, linked by GNU ld (and lld and mold where asked),
//! run under `qemu-i386`; every run must print what `llc`'s prints and
//! exit as it does, and `llc`'s must exit 0.

#![allow(dead_code, reason = "each test file uses part of the harness")]

use std::path::{Path, PathBuf};
use std::process::Command;

use hod::{Options, OutputKind, RelocModel};
use hod_testutil::target::Target;
use hod_testutil::{LLVM_MAJORS, Scratch, linkers};

use crate::i386_support::{
    TRIPLE, back_end_object, checked, clang_module, llc_object, llvm_tool, run_with_features, tool,
};

/// A program.
pub enum Source<'a> {
    /// A C or C++ file and clang's flags (`-march=...`, `-fPIE`).
    C(&'a Path, &'a [&'a str]),
    /// A module's text.
    Ll(&'a str),
}

/// How a program is built and run.
#[derive(Clone, Copy)]
pub struct Build<'a> {
    /// A PIE (`-relocation-model=pic`, `-pie`), else static (`-no-pie`).
    pub pic: bool,
    /// `llc`'s and hod's `-mcpu`, if any.
    pub mcpu: Option<&'a str>,
    /// The QEMU CPU features the program's code may use.
    pub features: &'a [&'a str],
    /// The link driver's extra flags (`-pthread`).
    pub link: &'a [&'a str],
    /// Every linker present (GNU ld, lld, mold), else GNU ld only.
    pub all_linkers: bool,
    /// hod's plain object from `hod_x86` directly instead of the facade
    /// (whose classifier refuses the IR `va_arg` instruction for every
    /// target: `inst:va_arg`, `hod_ir::classify`).
    pub back_end: bool,
}

/// The static build at the default CPU, linked by GNU ld.
pub const STATIC: Build<'static> =
    Build { pic: false, mcpu: None, features: &[], link: &[], all_linkers: false, back_end: false };

/// The tools.
pub struct Lane {
    gcc: PathBuf,
    qemu: PathBuf,
    linkers: Vec<(&'static str, PathBuf)>,
}

/// The lane, `None` when the cross tools are missing.
pub fn lane() -> Option<Lane> {
    let target = Target::I686;
    let (Some(gcc), Some(qemu)) = (tool(&target.c_driver()), tool(&target.emulator())) else {
        return None;
    };
    Some(Lane { gcc, qemu, linkers: linkers::present(target) })
}

/// hod's objects of `text`: the facade's and `hod_x86`'s under
/// `Stress::ALL`.
pub fn hod_objects(text: &[u8], build: Build<'_>, label: &str) -> (Vec<u8>, Vec<u8>) {
    let options = Options {
        output_kind: OutputKind::Object,
        triple: Some(TRIPLE.into()),
        relocation_model: Some(if build.pic { RelocModel::Pic } else { RelocModel::Static }),
        cpu: build.mcpu.map(str::to_owned),
        ..Options::default()
    };
    let reloc_model = if build.pic { hod_cg::RelocModel::Pic } else { hod_cg::RelocModel::Static };
    let plain = if build.back_end {
        back_end_object(text, &hod_cg::Options { reloc_model, ..hod_cg::Options::default() })
    } else {
        hod::compile(text, &options).unwrap_or_else(|e| panic!("hod {label}: {e}"))
    };
    let stress =
        hod_cg::Options { reloc_model, stress: hod_cg::Stress::ALL, ..hod_cg::Options::default() };
    let stressed = if build.mcpu.is_some() {
        // The back end's own entry has no `-mcpu`: the facade's object
        // stands for both.
        plain.clone()
    } else {
        back_end_object(text, &stress)
    };
    (plain, stressed)
}

/// The first line where `got` differs from `want`, for the message.
fn first_difference(want: &[u8], got: &[u8]) -> String {
    let (want, got) = (String::from_utf8_lossy(want), String::from_utf8_lossy(got));
    let mut lines = want.lines().zip(got.lines()).enumerate();
    match lines.find(|(_, (a, b))| a != b) {
        Some((n, (a, b))) => format!("line {}: llc {a:?}, hod {b:?}", n + 1),
        None => format!(
            "lengths: llc {} lines, hod {} lines",
            want.lines().count(),
            got.lines().count()
        ),
    }
}

impl Lane {
    /// Builds and runs `source` as `build` says with every LLVM version
    /// present; the number of runs compared.
    pub fn check(
        &self,
        scratch: &Scratch,
        name: &str,
        source: &Source<'_>,
        build: Build<'_>,
    ) -> usize {
        let mut runs = 0;
        for major in LLVM_MAJORS {
            let Some(llc) = llvm_tool(major, "llc") else { continue };
            let label = format!("{name}-{major}-{}", if build.pic { "pie" } else { "exe" });
            let ll = match source {
                Source::C(path, flags) => {
                    let Some(clang) = llvm_tool(major, "clang") else { continue };
                    let mut all: Vec<&str> = flags.to_vec();
                    all.push(if build.pic { "-fPIE" } else { "-fno-pic" });
                    clang_module(scratch, &clang, path, &all, &label)
                }
                Source::Ll(text) => scratch.write(&format!("{label}.ll"), text),
            };
            let text = std::fs::read(&ll).unwrap();
            let (plain, stress) = hod_objects(&text, build, &label);
            let mcpu = build.mcpu.map(|c| format!("-mcpu={c}"));
            let mut flags: Vec<&str> = Vec::new();
            if build.pic {
                flags.push("-relocation-model=pic");
            }
            if let Some(m) = &mcpu {
                flags.push(m);
            }
            let objects = [
                ("llc", llc_object(scratch, &llc, &ll, &flags, &format!("{label}-llc"))),
                ("hod", scratch.write(&format!("{label}-hod.o"), plain)),
                ("stress", scratch.write(&format!("{label}-stress.o"), stress)),
            ];
            let linkers: Vec<_> =
                self.linkers.iter().filter(|(n, _)| build.all_linkers || *n == "bfd").collect();
            for linker in linkers {
                let mut results = Vec::new();
                for (who, object) in &objects {
                    let exe = scratch.join(format!("{label}-{who}-{}", linker.0));
                    let mut command = Command::new(&self.gcc);
                    command.args(linkers::driver_flags(scratch.path(), linker.0, &linker.1));
                    command.arg(if build.pic { "-pie" } else { "-no-pie" });
                    command.arg(object).args(build.link).arg("-o").arg(&exe);
                    checked(&format!("{} link of {label} ({who})", linker.0), &mut command);
                    results.push(run_with_features(&self.qemu, &exe, build.features));
                    runs += 1;
                }
                let what = format!("{label} ({})", linker.0);
                assert_eq!(
                    results[0].0,
                    Some(0),
                    "{what}: llc's run failed: {:?}",
                    String::from_utf8_lossy(&results[0].1)
                );
                for (who, result) in ["hod", "stress"].iter().zip(&results[1..]) {
                    assert!(
                        results[0].1 == result.1,
                        "{what}: {who}'s output differs from llc's at {}",
                        first_difference(&results[0].1, &result.1)
                    );
                    assert_eq!(results[0].0, result.0, "{what}: {who}'s status");
                }
            }
        }
        runs
    }
}

/// The error of hod on the module `body` for i686 (`-mcpu` `cpu`), or
/// `None` when it compiles.
pub fn refusal(body: &str, cpu: Option<&str>) -> Option<String> {
    let text = format!("target triple = \"{TRIPLE}\"\n{body}\n");
    let options = Options {
        output_kind: OutputKind::Object,
        triple: Some(TRIPLE.into()),
        cpu: cpu.map(str::to_owned),
        ..Options::default()
    };
    hod::compile(text.as_bytes(), &options).err().map(|e| e.to_string())
}

/// The error of `hod_x86` itself (no classifier) on the module `body`,
/// or `None` when it compiles.
pub fn back_end_refusal(body: &str) -> Option<String> {
    let text = format!("target triple = \"{TRIPLE}\"\n{body}\n");
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
    hod_x86::compile_to_object(&module, &bodies, &hod_cg::Options::default())
        .err()
        .map(|e| e.to_string())
}

/// The data file `name` of directory `dir` under `tests/data/i386`.
pub fn data(dir: &str, name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/i386").join(dir).join(name)
}
