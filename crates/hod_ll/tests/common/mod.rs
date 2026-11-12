//! Support shared by `hod_ll`'s tests: the LLVM tools of one version, the
//! corpus they generate, and a reader for the objects `llc` writes.
//!
//! Each test file uses part of this module.
#![allow(dead_code)]

pub mod elf;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicUsize, Ordering};

use hod_ir::{AARCH64_LINUX_DATA_LAYOUT, AARCH64_LINUX_DATA_LAYOUT_LLVM19, Arch};
use hod_ir::{TargetSpec, X86_64_LINUX_DATA_LAYOUT};
use hod_ll::{ParseOptions, ParsedModule, parse_module};
use hod_testutil::{DEFAULT_TIMEOUT, LLVM_MAJORS, Scratch, run_with, tools_required};

use elf::Elf;

/// Child processes run at once while the corpus is generated: the
/// machine is shared.
const WORKERS: usize = 8;

/// The directory of hand-written inputs.
pub fn data_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data")
}

/// Reads a file of the data directory.
pub fn data(name: &str) -> Vec<u8> {
    let path = data_dir().join(name);
    std::fs::read(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

/// The hand-written `.ll` files, by name: `any_*.ll` are in the dialect
/// of every supported LLVM, `v22_*.ll` in that of LLVM 22 only.
pub fn handwritten() -> Vec<(String, Vec<u8>)> {
    let mut files: Vec<String> = std::fs::read_dir(data_dir())
        .expect("tests/data")
        .map(|entry| entry.expect("tests/data entry").file_name().to_string_lossy().into_owned())
        .filter(|name| Path::new(name).extension().is_some_and(|extension| extension == "ll"))
        .collect();
    files.sort();
    files.into_iter().map(|name| (name.clone(), data(&name))).collect()
}

/// The `target` lines of a module for `arch` in the dialect of LLVM
/// `major`.
pub fn header(arch: Arch, major: u32) -> String {
    let (triple, layout) = match (arch, major) {
        (Arch::Aarch64, 19) => ("aarch64-unknown-linux-gnu", AARCH64_LINUX_DATA_LAYOUT_LLVM19),
        (Arch::Aarch64, _) => ("aarch64-unknown-linux-gnu", AARCH64_LINUX_DATA_LAYOUT),
        (Arch::X86_64, _) => ("x86_64-unknown-linux-gnu", X86_64_LINUX_DATA_LAYOUT),
    };
    format!("target datalayout = \"{layout}\"\ntarget triple = \"{triple}\"\n")
}

/// The clang name of `arch`'s target.
pub fn clang_target(arch: Arch) -> &'static str {
    match arch {
        Arch::Aarch64 => "aarch64-linux-gnu",
        Arch::X86_64 => "x86_64-linux-gnu",
    }
}

pub fn arch_name(arch: Arch) -> &'static str {
    match arch {
        Arch::Aarch64 => "aarch64",
        Arch::X86_64 => "x86_64",
    }
}

pub const ARCHES: [Arch; 2] = [Arch::Aarch64, Arch::X86_64];

/// The tools of one LLVM version.
#[derive(Clone, Debug)]
pub struct Llvm {
    pub major: u32,
    bin: PathBuf,
}

impl Llvm {
    /// LLVM `major`'s tools, in `/usr/lib/llvm-<major>/bin`.
    ///
    /// `None`, after a note, when they are missing and tools are optional.
    ///
    /// # Panics
    /// When they are missing and `HOD_REQUIRE_TOOLS=1`.
    pub fn find(major: u32) -> Option<Llvm> {
        let bin = PathBuf::from(format!("/usr/lib/llvm-{major}/bin"));
        let all = ["clang", "llvm-as", "llvm-dis", "llc", "llvm-readelf"];
        if all.iter().all(|tool| bin.join(tool).is_file()) {
            return Some(Llvm { major, bin });
        }
        assert!(
            !tools_required(),
            "LLVM {major} tools not found in {} and HOD_REQUIRE_TOOLS=1",
            bin.display()
        );
        eprintln!("note: skipping: LLVM {major} tools not found in {}", bin.display());
        None
    }

    /// Every supported LLVM that is installed.
    pub fn all() -> Vec<Llvm> {
        LLVM_MAJORS.iter().filter_map(|&major| Llvm::find(major)).collect()
    }

    pub fn tool(&self, name: &str) -> PathBuf {
        self.bin.join(name)
    }

    /// What clang makes of the data file `source` for `arch` with
    /// `options`, as LLVM IR text at `-O0`.
    ///
    /// # Panics
    /// If clang fails.
    pub fn clang(&self, source: &str, arch: Arch, options: &[&str]) -> Vec<u8> {
        let mut command = Command::new(self.tool("clang"));
        command.arg(format!("--target={}", clang_target(arch)));
        command.args(["-S", "-emit-llvm", "-O0", "-w", "-o", "-"]);
        if Path::new(source).extension().is_some_and(|extension| extension == "cpp") {
            command.arg("-std=c++20");
        }
        command.args(options).arg(data_dir().join(source));
        let outcome = run_with(&mut command, &[], DEFAULT_TIMEOUT);
        assert!(
            outcome.success(),
            "clang-{} {source} {options:?}: {}",
            self.major,
            outcome.stderr_text()
        );
        outcome.stdout
    }

    /// Whether `llvm-as` accepts `ir`; its diagnostics if not.
    pub fn accepts(&self, ir: &[u8]) -> Result<(), String> {
        let mut command = Command::new(self.tool("llvm-as"));
        command.args(["-o", "/dev/null", "-"]);
        let outcome = run_with(&mut command, ir, DEFAULT_TIMEOUT);
        if outcome.success() { Ok(()) } else { Err(outcome.stderr_text()) }
    }

    /// `ir` as `llvm-as | llvm-dis` prints it: LLVM's own reading of the
    /// module, upgraded and in canonical form.
    pub fn canonical(&self, ir: &[u8]) -> Result<Vec<u8>, String> {
        let mut assemble = Command::new(self.tool("llvm-as"));
        assemble.args(["-o", "-", "-"]);
        let bitcode = run_with(&mut assemble, ir, DEFAULT_TIMEOUT);
        if !bitcode.success() {
            return Err(bitcode.stderr_text());
        }
        let mut disassemble = Command::new(self.tool("llvm-dis"));
        disassemble.args(["-o", "-", "-"]);
        let text = run_with(&mut disassemble, &bitcode.stdout, DEFAULT_TIMEOUT);
        if text.success() { Ok(text.stdout) } else { Err(text.stderr_text()) }
    }

    /// The object `llc -O0` makes of `ir`, with every variable in a
    /// section of its own and private symbols kept in the symbol table.
    pub fn object(&self, scratch: &Scratch, name: &str, ir: &[u8]) -> Result<Vec<u8>, String> {
        let output = scratch.join(format!("{name}.o"));
        let mut command = Command::new(self.tool("llc"));
        command.args(["-O0", "-filetype=obj", "--data-sections", "--save-temp-labels", "-o"]);
        command.arg(&output).arg("-");
        let outcome = run_with(&mut command, ir, DEFAULT_TIMEOUT);
        if !outcome.success() {
            return Err(outcome.stderr_text());
        }
        std::fs::read(&output).map_err(|error| error.to_string())
    }
}

/// One module of the corpus.
#[derive(Debug)]
pub struct Case {
    /// What it is, for messages: source, options, LLVM version, target.
    pub name: String,
    pub llvm: u32,
    pub arch: Arch,
    /// The module as written (by clang or by hand).
    pub text: Vec<u8>,
    /// The module as `llvm-as | llvm-dis` prints it.
    pub canonical: Vec<u8>,
    /// The module as the newest installed LLVM prints it: `canonical`
    /// with every upgrade that LLVM has since given its reader.
    pub upgraded: Vec<u8>,
    /// What `llc` made of it, or why it made nothing.
    pub object: Result<Elf, String>,
    /// The object file itself.
    pub object_bytes: Vec<u8>,
    /// Whether clang wrote it.
    pub from_clang: bool,
    /// The clang options it was compiled with.
    pub options: &'static [&'static str],
}

/// The clang option sets of the C sources.
pub const C_OPTIONS: [&[&str]; 2] = [
    &[],
    &[
        "-g",
        "-fPIC",
        "-fstack-protector-strong",
        "-funwind-tables",
        "-fsanitize=signed-integer-overflow,shift",
        "-fcommon",
    ],
];

/// The clang option sets of `cxx.cpp`.
pub const CXX_OPTIONS: [&[&str]; 3] =
    [&[], &["-fexperimental-relative-c++-abi-vtables"], &["-g", "-fPIE"]];

struct Job {
    name: String,
    llvm: Llvm,
    arch: Arch,
    /// A data file for clang, or the text of a module.
    source: Result<&'static str, Vec<u8>>,
    options: &'static [&'static str],
}

fn jobs() -> Vec<Job> {
    let mut jobs = Vec::new();
    let handwritten = handwritten();
    for llvm in Llvm::all() {
        for arch in ARCHES {
            let tag = format!("[llvm{} {}]", llvm.major, arch_name(arch));
            let mut clang = |source: &'static str, options: &'static [&'static str]| {
                jobs.push(Job {
                    name: format!("{source} {} {tag}", options.join(" ")),
                    llvm: llvm.clone(),
                    arch,
                    source: Ok(source),
                    options,
                });
            };
            for options in C_OPTIONS {
                clang("globals.c", options);
                clang("attrs.c", options);
            }
            for options in CXX_OPTIONS {
                clang("cxx.cpp", options);
            }
            // Branch protection is in module flags and in function
            // attributes, and LLVM upgrades the one to the other.
            clang(
                "attrs.c",
                match arch {
                    Arch::Aarch64 => &["-mbranch-protection=standard"],
                    Arch::X86_64 => &["-fcf-protection=full"],
                },
            );
            // The large one once per LLVM version, on alternating
            // targets, without and with debug information.
            if (llvm.major == 19) == (arch == Arch::X86_64) {
                clang("stl.cpp", &[]);
                clang("stl.cpp", &["-g"]);
            }
            for (file, text) in &handwritten {
                if file.starts_with("v22_") && llvm.major < 22 {
                    continue;
                }
                let mut module = header(arch, llvm.major).into_bytes();
                module.extend_from_slice(text);
                jobs.push(Job {
                    name: format!("{file} {tag}"),
                    llvm: llvm.clone(),
                    arch,
                    source: Err(module),
                    options: &[],
                });
            }
        }
    }
    jobs
}

fn build(job: Job, scratch: &Scratch, index: usize) -> Case {
    let (text, from_clang) = match job.source {
        Ok(source) => (job.llvm.clang(source, job.arch, job.options), true),
        Err(text) => (text, false),
    };
    let canonical = job
        .llvm
        .canonical(&text)
        .unwrap_or_else(|error| panic!("{}: llvm-as rejects a corpus module: {error}", job.name));
    let newest = Llvm::all().into_iter().max_by_key(|llvm| llvm.major).expect("an LLVM");
    let upgraded = if newest.major == job.llvm.major {
        canonical.clone()
    } else {
        newest.canonical(&text).unwrap_or_else(|error| {
            panic!("{}: llvm-as {} rejects a corpus module: {error}", job.name, newest.major)
        })
    };
    let object_bytes = job.llvm.object(scratch, &format!("case{index}"), &text);
    let object = object_bytes.as_ref().map(|bytes| Elf::parse(bytes)).map_err(Clone::clone);
    Case {
        name: job.name,
        llvm: job.llvm.major,
        arch: job.arch,
        text,
        canonical,
        upgraded,
        object,
        object_bytes: object_bytes.unwrap_or_default(),
        from_clang,
        options: job.options,
    }
}

/// The corpus (`spec/ll.md` §10): what clang 19 and 22 make of the
/// sources in `tests/data` for both targets under several option sets,
/// and the hand-written modules, each with LLVM's canonical printing of
/// it and `llc`'s object. Generated once per test process. Empty, after
/// a note, when no LLVM is installed.
pub fn corpus() -> &'static [Case] {
    static CORPUS: OnceLock<Vec<Case>> = OnceLock::new();
    CORPUS.get_or_init(|| {
        let scratch = Scratch::new("hod_ll_corpus");
        let jobs: Vec<std::sync::Mutex<Option<Job>>> =
            jobs().into_iter().map(|job| std::sync::Mutex::new(Some(job))).collect();
        let next = AtomicUsize::new(0);
        let mut cases: Vec<(usize, Case)> = std::thread::scope(|scope| {
            let workers: Vec<_> = (0..WORKERS)
                .map(|_| {
                    scope.spawn(|| {
                        let mut done = Vec::new();
                        loop {
                            let index = next.fetch_add(1, Ordering::Relaxed);
                            let Some(slot) = jobs.get(index) else {
                                return done;
                            };
                            let job = slot.lock().expect("job").take().expect("job taken once");
                            done.push((index, build(job, &scratch, index)));
                        }
                    })
                })
                .collect();
            workers
                .into_iter()
                .flat_map(|worker| worker.join().expect("a corpus worker panicked"))
                .collect()
        });
        cases.sort_by_key(|&(index, _)| index);
        cases.into_iter().map(|(_, case)| case).collect()
    })
}

/// Parses `text` for `arch` when it names no target.
pub fn parse(text: &[u8], arch: Arch) -> Result<ParsedModule<'_>, hod_ll::LlError> {
    let target = match arch {
        Arch::Aarch64 => TargetSpec::aarch64_linux_gnu(),
        Arch::X86_64 => TargetSpec::x86_64_linux_gnu(),
    };
    parse_module(text, &ParseOptions { default_target: Some(target) })
}

/// Parses a module of the corpus.
///
/// # Panics
/// If the parser refuses it.
pub fn parsed(case: &Case) -> ParsedModule<'_> {
    parse(&case.text, case.arch).unwrap_or_else(|error| panic!("{}: {error}", case.name))
}

/// A deterministic generator (a 64-bit LCG, high bits).
pub struct Lcg(pub u64);

impl Lcg {
    pub fn next(&mut self) -> u64 {
        self.0 =
            self.0.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }

    pub fn below(&mut self, bound: usize) -> usize {
        (self.next() % bound as u64) as usize
    }
}
