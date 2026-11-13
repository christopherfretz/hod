//! Building a debugger test's programs: the front end, `llc` and hod, the
//! linker matrix. Contract: `spec/harness.md` "debug", "What a test
//! does".

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::directives::{DebugTest, Lang};
use crate::irdiff::process::{self, End, Limits, Output};
use crate::irdiff::tools::{self, Linker, Search};

/// The reference: `llc -O0` with the `SelectionDAG` selector.
pub const REFERENCE: [&str; 3] = ["-O0", "-global-isel=0", "-fast-isel=0"];

/// What the front ends are given besides the test's `cflags`.
pub const FRONTEND_FLAGS: [&str; 4] = ["-g", "-O0", "-S", "-emit-llvm"];

/// What a `pre-opt: checks` test's front end is given besides (WP-108).
pub const PRE_OPT_FLAGS: [&str; 2] = ["-Xclang", "-disable-llvm-passes"];

/// The tools of a debugger run.
#[derive(Clone, Debug)]
pub struct Tools {
    /// `clang-<N>` / `clang++-<N>` by front-end name, when found.
    pub frontends: Vec<(String, PathBuf)>,
    /// `llc` by LLVM major, when found.
    pub llc: Vec<(u32, PathBuf)>,
    pub hod: PathBuf,
    /// The linker driver (`gcc`).
    pub driver: Option<PathBuf>,
    pub linkers: Vec<Linker>,
    /// The LLVM 22 installation kiln is pointed at (`--llvm-dir=`).
    pub kiln_llvm: Option<PathBuf>,
}

impl Tools {
    /// The front end named `name` (`clang-19`, `clang++-22`, `kiln`).
    pub fn frontend(&self, name: &str) -> Option<&Path> {
        self.frontends.iter().find(|(n, _)| n == name).map(|(_, p)| p.as_path())
    }

    pub fn llc(&self, major: u32) -> Option<&Path> {
        self.llc.iter().find(|(m, _)| *m == major).map(|(_, p)| p.as_path())
    }
}

/// `clang-<N>` (`clang++-<N>`) on `PATH`, else in `/usr/lib/llvm-<N>/bin`.
pub fn find_clang(search: &Search, cxx: bool, major: u32) -> Option<PathBuf> {
    search.versioned(if cxx { "clang++" } else { "clang" }, major)
}

/// The LLVM major of the IR a front end writes, which is the `llc` that
/// is its reference: clang's own; kiln writes LLVM 22.
pub fn frontend_major(frontend: &str) -> u32 {
    match frontend {
        "clang-19" | "ir-19" => 19,
        _ => 22,
    }
}

/// The front end's tool name for a language: `clang-N` for C,
/// `clang++-N` for C++, `kiln` for kiln.
pub fn frontend_tool(frontend: &str, lang: Lang) -> String {
    match (frontend, lang) {
        ("kiln", _) => "kiln".to_owned(),
        (clang, Lang::Cxx) => clang.replace("clang", "clang++"),
        (clang, _) => clang.to_owned(),
    }
}

/// A step that failed: what it was and what it said.
pub fn failure(step: &str, output: &Output, limits: Limits) -> String {
    let how = match output.end {
        End::Exit(code) => format!("exit status {code}"),
        End::Signal(signal) => format!("signal {signal}"),
        End::TimedOut => format!("timed out after {} s", limits.time.as_secs()),
    };
    let said = output.first_error_line();
    if said.is_empty() { format!("{step}: {how}") } else { format!("{step}: {how}: {said}") }
}

/// The front end's IR of one module into `out`, run in the module's
/// directory with its file name, so that the unit's name is the file's.
pub fn frontend_ir(
    tools: &Tools,
    test: &DebugTest,
    frontend: &str,
    module: &Path,
    out: &Path,
    limits: Limits,
) -> Result<(), String> {
    if test.lang == Lang::Ir {
        return fs::copy(module, out)
            .map(|_| ())
            .map_err(|e| format!("cannot copy '{}': {e}", module.display()));
    }
    let name = frontend_tool(frontend, test.lang);
    let compiler = tools.frontend(&name).ok_or_else(|| format!("no {name}"))?;
    let dir = module.parent().unwrap_or(Path::new("."));
    let file = module.file_name().unwrap_or_default().to_owned();
    let tmp = out.parent().unwrap_or(Path::new("."));
    let configure = |c: &mut Command| {
        if frontend == "kiln" {
            if let Some(llvm) = &tools.kiln_llvm {
                c.arg(tools::prefixed("--llvm-dir=", llvm));
            }
            c.arg("-fno-crash-diagnostics");
        }
        c.args(FRONTEND_FLAGS).args(&test.cflags);
        if test.pre_opt {
            c.args(PRE_OPT_FLAGS);
        }
        c.arg(&file).arg("-o").arg(out);
        c.current_dir(dir).env("TMPDIR", tmp).env("LC_ALL", "C");
    };
    let output = process::run(compiler, configure, b"", limits)?;
    match output.end {
        End::Exit(0) if out.is_file() => Ok(()),
        _ => Err(failure(&name, &output, limits)),
    }
}

/// The `.dwo` name a `-gsplit-dwarf` module records (its unit's
/// `splitDebugFilename`), if any (WP-062).
pub fn split_dwarf_name(ir: &Path) -> Option<String> {
    let text = fs::read_to_string(ir).ok()?;
    let at = text.find("splitDebugFilename: \"")? + "splitDebugFilename: \"".len();
    let len = text[at..].find('"')?;
    Some(text[at..at + len].to_owned()).filter(|name| !name.is_empty())
}

/// What `llc` and hod are given for such a module: the name, and the
/// `.dwo` file beside the object.
fn split_dwarf_args(ir: &Path, object: &Path) -> Vec<OsString> {
    split_dwarf_name(ir).map_or_else(Vec::new, |name| {
        vec![
            OsString::from(format!("-split-dwarf-file={name}")),
            tools::prefixed("-split-dwarf-output=", &object.with_extension("dwo")),
        ]
    })
}

/// Puts the `.dwo` of each of `objects` that has one beside `program`,
/// under the last component of its recorded name (`dwos`: object, name),
/// where the debuggers look for it.
pub fn place_dwos(
    objects: &[PathBuf],
    dwos: &[(PathBuf, String)],
    program: &Path,
) -> Result<(), String> {
    let dir = program.parent().unwrap_or(Path::new("."));
    for (object, name) in dwos.iter().filter(|(o, _)| objects.contains(o)) {
        let from = object.with_extension("dwo");
        let to = dir.join(Path::new(name).file_name().unwrap_or_default());
        fs::copy(&from, &to).map_err(|e| format!("cannot copy '{}': {e}", from.display()))?;
    }
    Ok(())
}

/// `llc` (the reference) on `ir` into `object`.
pub fn llc_object(llc: &Path, ir: &Path, object: &Path, limits: Limits) -> Result<(), String> {
    let split = split_dwarf_args(ir, object);
    let configure = |c: &mut Command| {
        c.args(REFERENCE).args(&split).args(["-filetype=obj", "-relocation-model=pic", "-o"]);
        c.arg(object).arg(ir);
    };
    let output = process::run(llc, configure, b"", limits)?;
    match output.end {
        End::Exit(0) if object.is_file() => Ok(()),
        _ => Err(failure("llc", &output, limits)),
    }
}

/// `opt -passes='default<O0>'` (the `opt` beside `llc`) on `ir` into
/// `out`: what `llc` is given in a `pre-opt: checks` test.
pub fn opt_ir(llc: &Path, ir: &Path, out: &Path, limits: Limits) -> Result<(), String> {
    let opt = tools::opt_beside(llc);
    if !opt.is_file() {
        return Err(format!("no opt beside '{}'", llc.display()));
    }
    let configure = |c: &mut Command| {
        c.args([crate::irdiff::engine::PRE_OPT_PASSES, "-S", "-o"]).arg(out).arg(ir);
    };
    let output = process::run(&opt, configure, b"", limits)?;
    match output.end {
        End::Exit(0) if out.is_file() => Ok(()),
        _ => Err(failure("opt", &output, limits)),
    }
}

/// What hod did with a module.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HodBuild {
    Built,
    /// It declined, with the coverage log's reason.
    Declined(String),
    /// It failed: a crash, another status, no object.
    Failed(String),
}

/// hod (strict) on `ir` into `object`, with its coverage log beside it.
pub fn hod_object(hod: &Path, ir: &Path, object: &Path, limits: Limits) -> HodBuild {
    let log = object.with_extension("coverage.tsv");
    let split = split_dwarf_args(ir, object);
    let configure = |c: &mut Command| {
        c.arg(tools::prefixed("--coverage-log=", &log)).arg("--strict").args(&split);
        c.args(["-O0", "-filetype=obj", "-relocation-model=pic", "-o"]).arg(object).arg(ir);
        for variable in ["HOD_FALLBACK_LLC", "HOD_STRICT", "HOD_COVERAGE_LOG", "HOD_IN_FALLBACK"] {
            c.env_remove(variable);
        }
    };
    let output = match process::run(hod, configure, b"", limits) {
        Ok(output) => output,
        Err(e) => return HodBuild::Failed(e),
    };
    let line = fs::read_to_string(&log).unwrap_or_default();
    // The coverage log's one line: the outcome, then the reason
    // (`spec/cli.md`).
    let mut fields = line.lines().next().unwrap_or("").split('\t');
    let (outcome, reason) = (fields.next().unwrap_or(""), fields.next().unwrap_or(""));
    match (output.end, outcome) {
        (End::Exit(0), "hod") if object.is_file() => HodBuild::Built,
        (End::Exit(2), "error") => HodBuild::Declined(reason.to_owned()),
        _ => HodBuild::Failed(failure("hod", &output, limits)),
    }
}

/// Links `objects` with `linker` into `program` (PIE, as the objects are
/// built), `extra` after the objects; C++ tests get the C++ library.
#[expect(clippy::too_many_arguments, reason = "the link of one program, all of it")]
pub fn link(
    tools: &Tools,
    linker_dir: &Path,
    linker: &Linker,
    objects: &[PathBuf],
    extra: &[String],
    cxx: bool,
    program: &Path,
    limits: Limits,
) -> Result<(), String> {
    let driver = tools.driver.as_deref().ok_or("no linker driver (gcc)")?;
    let mut args: Vec<OsString> = vec![tools::prefixed("-B", linker_dir), "-pie".into()];
    args.extend(tools::linker_flags(&linker.name).iter().map(OsString::from));
    args.push("-o".into());
    args.push(program.into());
    args.extend(objects.iter().map(OsString::from));
    args.extend(extra.iter().map(OsString::from));
    if cxx {
        args.push("-lstdc++".into());
    }
    args.push("-lm".into());
    let configure = |c: &mut Command| {
        c.args(&args).env("LC_ALL", "C");
    };
    let output = process::run(driver, configure, b"", limits)?;
    if !(output.end == End::Exit(0) && program.is_file()) {
        return Err(failure(&format!("link with {}", linker.name), &output, limits));
    }
    let bytes = fs::read(program).map_err(|e| format!("cannot read program: {e}"))?;
    tools::check_signature(&linker.name, &bytes)
}

/// One directory per linker, holding it as `ld`, for the driver's `-B`.
pub fn prepare_linkers(work: &Path, linkers: &[Linker]) -> Result<(), String> {
    for linker in linkers {
        let dir = work.join("linkers").join(&linker.name);
        fs::create_dir_all(&dir).map_err(|e| format!("cannot create '{}': {e}", dir.display()))?;
        let link = dir.join("ld");
        let _ = fs::remove_file(&link);
        std::os::unix::fs::symlink(&linker.path, &link)
            .map_err(|e| format!("cannot create '{}': {e}", link.display()))?;
    }
    Ok(())
}
