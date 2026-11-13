//! `cargo xtask debug columns`: the line tables of hod's objects against
//! `llc`'s, with columns, over the `-g` modules of the corpus.
//! Contract: `spec/harness.md` "debug", "Columns"; `spec/dwarf.md` §5.7.
//!
//! Each module is compiled by hod (strict) and by `llc` (the reference,
//! of the module's LLVM major when its path names one), both objects are
//! linked into a position-independent executable by lld (unresolved symbols
//! allowed, so that every function has an
//! address and a symbol), and the two are compared by
//! [`lines::compare`]: the `(function, line)` pairs and the rows with
//! columns. With `--dump <dir>`, hod's object's disassembly with raw bytes
//! and relocations is written to `<dir>/<index>.dis` (the evidence that a
//! change of rows leaves the code alone: two dumps are compared with
//! `diff -r`).

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, PoisonError};

use super::build::{self, REFERENCE};
use super::lines::{self, LineTools, LinesReport};
use super::permitted::IrFacts;
use super::verify;
use crate::irdiff::process::{self, End};
use crate::irdiff::tools::Search;

/// What `debug columns` is given.
#[derive(Clone, Debug)]
pub struct Options {
    pub paths: Vec<PathBuf>,
    pub jobs: usize,
    pub hod: Option<PathBuf>,
    pub build_dir: PathBuf,
    pub dump: Option<PathBuf>,
    pub verbose: bool,
}

/// One module's outcome.
#[derive(Clone, Debug)]
pub enum Outcome {
    Compared(LinesReport),
    /// hod declined, with its reason.
    Declined(String),
    Failed(String),
}

/// The LLVM major of a module: `llvm19` or `llvm22` in its path, else 22.
fn module_major(module: &Path) -> u32 {
    let named = |m: u32| module.components().any(|c| c.as_os_str() == format!("llvm{m}").as_str());
    if named(19) { 19 } else { 22 }
}

fn run_tool(tool: &Path, configure: impl Fn(&mut Command), what: &str) -> Result<String, String> {
    let output = process::run(tool, configure, b"", verify::LIMITS)?;
    if output.end != End::Exit(0) {
        return Err(build::failure(what, &output, verify::LIMITS));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

struct Tools {
    hod: PathBuf,
    llc: Vec<(u32, PathBuf)>,
    lld: PathBuf,
    lines: LineTools,
}

fn one(module: &Path, tools: &Tools, dir: &Path, dump: Option<&Path>) -> Outcome {
    let extra = fs::read_to_string(module)
        .ok()
        .and_then(|t| {
            t.lines().take_while(|l| l.starts_with(';') || l.is_empty()).find_map(|l| {
                l.strip_prefix(";@")?.trim().strip_prefix("llc-args:").map(str::to_owned)
            })
        })
        .unwrap_or_default();
    let extra: Vec<&str> = extra.split_whitespace().collect();
    let (hod_o, llc_o) = (dir.join("hod.o"), dir.join("llc.o"));
    match build::hod_object(&tools.hod, module, &hod_o, verify::LIMITS) {
        build::HodBuild::Built => {}
        build::HodBuild::Declined(reason) => return Outcome::Declined(reason),
        build::HodBuild::Failed(e) => return Outcome::Failed(e),
    }
    let major = module_major(module);
    let Some((_, llc)) = tools.llc.iter().find(|(m, _)| *m == major).or(tools.llc.first()) else {
        return Outcome::Failed("no llc".to_owned());
    };
    let reference = run_tool(
        llc,
        |c| {
            c.args(REFERENCE).args(["-filetype=obj", "-relocation-model=pic"]).args(&extra);
            c.arg("-o").arg(&llc_o).arg(module);
        },
        "llc",
    );
    if let Err(e) = reference {
        return Outcome::Failed(e);
    }
    if let Some(dump) = dump {
        let shown = run_tool(
            &tools.lines.objdump,
            |c| {
                c.args(["-d", "-r"]).arg(&hod_o).env("LC_ALL", "C");
            },
            "llvm-objdump",
        );
        let name = dir.file_name().unwrap_or_default().to_string_lossy().into_owned();
        // The object's own path is in the first lines; the code follows.
        let text = shown.map(|t| t.replace(&hod_o.display().to_string(), "hod.o"));
        if let Err(e) = text
            .and_then(|t| fs::write(dump.join(format!("{name}.dis")), t).map_err(|e| e.to_string()))
        {
            return Outcome::Failed(e);
        }
    }
    let mut shared = Vec::new();
    for (object, name) in [(&hod_o, "hod.so"), (&llc_o, "llc.so")] {
        let out = dir.join(name);
        let linked = run_tool(
            &tools.lld,
            |c| {
                c.args([
                    "-pie",
                    "--unresolved-symbols=ignore-all",
                    "-z",
                    "notext",
                    "-e",
                    "0",
                    "-o",
                ])
                .arg(&out)
                .arg(object);
            },
            "ld.lld",
        );
        if let Err(e) = linked {
            return Outcome::Failed(e);
        }
        shared.push(out);
    }
    let facts = IrFacts::of_modules([fs::read_to_string(module).unwrap_or_default().as_str()]);
    match lines::compare(&shared[0], &shared[1], &tools.lines, &facts) {
        Ok(report) => Outcome::Compared(report),
        Err(e) => Outcome::Failed(e),
    }
}

/// Runs the comparison; the modules and their outcomes, in name order.
pub fn run(options: &Options) -> Result<Vec<(PathBuf, Outcome)>, String> {
    let search = Search::from_env();
    let lines_tools =
        super::sessions::find_line_tools().ok_or("no llvm-dwarfdump, -nm, -objdump")?;
    let llc: Vec<(u32, PathBuf)> =
        [22, 19].into_iter().filter_map(|m| Some((m, search.versioned("llc", m)?))).collect();
    let lld = search
        .versioned("ld.lld", 22)
        .or_else(|| search.versioned("ld.lld", 19))
        .or_else(|| search.on_path("ld.lld"))
        .ok_or("no ld.lld")?;
    let paths = if options.paths.is_empty() {
        vec![crate::corpus::default_dir(&options.build_dir)]
    } else {
        options.paths.clone()
    };
    let modules = verify::debug_modules(&paths)?;
    let hod = match &options.hod {
        Some(hod) => hod.clone(),
        None => crate::irdiff::build_workspace_hod(&options.build_dir)?,
    };
    let tools = Tools { hod, llc, lld, lines: lines_tools };
    if let Some(dump) = &options.dump {
        fs::create_dir_all(dump).map_err(|e| format!("cannot create '{}': {e}", dump.display()))?;
    }
    let work = options.build_dir.join("l7").join(format!("columns-{}", std::process::id()));
    fs::create_dir_all(&work).map_err(|e| format!("cannot create '{}': {e}", work.display()))?;
    let next = AtomicUsize::new(0);
    let slots = Mutex::new(vec![None; modules.len()]);
    std::thread::scope(|scope| {
        for _ in 0..options.jobs.clamp(1, modules.len().max(1)) {
            scope.spawn(|| {
                loop {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    let Some(module) = modules.get(index) else { break };
                    let dir = work.join(format!("{index:05}"));
                    let outcome = match fs::create_dir_all(&dir) {
                        Ok(()) => one(module, &tools, &dir, options.dump.as_deref()),
                        Err(e) => {
                            Outcome::Failed(format!("cannot create '{}': {e}", dir.display()))
                        }
                    };
                    let _ = fs::remove_dir_all(&dir);
                    slots.lock().unwrap_or_else(PoisonError::into_inner)[index] = Some(outcome);
                }
            });
        }
    });
    let _ = fs::remove_dir_all(&work);
    let outcomes = slots.into_inner().unwrap_or_else(PoisonError::into_inner);
    Ok(modules
        .into_iter()
        .zip(outcomes)
        .map(|(m, o)| (m, o.unwrap_or_else(|| Outcome::Failed("not run".to_owned()))))
        .collect())
}

/// The report: per module, then the totals; with `verbose`, every
/// position on one side only.
pub fn summary(outcomes: &[(PathBuf, Outcome)], verbose: bool) -> String {
    let mut out = String::new();
    let mut total = LinesReport::default();
    let (mut compared, mut declined, mut failed) = (0, 0, 0);
    for (module, outcome) in outcomes {
        let name = module.display();
        match outcome {
            Outcome::Compared(report) => {
                compared += 1;
                total.add(report);
                let c = &report.columns;
                let _ = writeln!(
                    out,
                    "columns {name}: {} of {} functions same set, {} same but permitted",
                    c.same_set, c.functions, c.same_permitted
                );
                if verbose {
                    for (side, list) in [("hod only", &c.hod_only), ("llc only", &c.llc_only)] {
                        for (function, (file, line, column), class) in list {
                            let class = class.name();
                            let _ = writeln!(
                                out,
                                "  {class} {side} {file}:{line}:{column} in {function}"
                            );
                        }
                    }
                }
            }
            Outcome::Declined(reason) => {
                declined += 1;
                let _ = writeln!(out, "columns {name}: declined: {reason}");
            }
            Outcome::Failed(e) => {
                failed += 1;
                let _ = writeln!(out, "columns {name}: FAILED: {e}");
            }
        }
    }
    let _ = writeln!(
        out,
        "columns: {} -g modules: {compared} compared, {declined} declined, {failed} failed",
        outcomes.len()
    );
    let _ = writeln!(out, "columns: lines: {}", total.summary());
    let _ = writeln!(out, "columns: {}", total.columns.summary());
    out
}

/// `cargo xtask debug columns ...`.
pub fn command(args: &[String], build_dir: PathBuf) -> u8 {
    let mut options =
        Options { paths: Vec::new(), jobs: 4, hod: None, build_dir, dump: None, verbose: false };
    let mut i = 0;
    while i < args.len() {
        let arg = args[i].as_str();
        let mut take = || {
            i += 1;
            args.get(i).cloned().ok_or_else(|| format!("{arg} needs a value"))
        };
        let got = match arg {
            "-v" | "--verbose" => {
                options.verbose = true;
                Ok(())
            }
            "-j" | "--jobs" => match take().map(|n| n.parse::<usize>()) {
                Ok(Ok(n)) if n > 0 => {
                    options.jobs = n.min(4);
                    Ok(())
                }
                _ => Err("-j needs a count".to_owned()),
            },
            "--hod" => take().map(|v| options.hod = Some(PathBuf::from(v))),
            "--dump" => take().map(|v| options.dump = Some(PathBuf::from(v))),
            _ if !arg.starts_with('-') => {
                options.paths.push(PathBuf::from(arg));
                Ok(())
            }
            _ => Err(format!("unknown option '{arg}'")),
        };
        if let Err(e) = got {
            eprint!("xtask debug columns: {e}\n{}", super::USAGE);
            return 2;
        }
        i += 1;
    }
    match run(&options) {
        Ok(outcomes) => {
            print!("{}", summary(&outcomes, options.verbose));
            u8::from(outcomes.iter().any(|(_, o)| matches!(o, Outcome::Failed(_))))
        }
        Err(e) => {
            eprintln!("xtask debug columns: {e}");
            3
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn major_of_a_module() {
        assert_eq!(module_major(Path::new("/c/kiln/llvm19/a/b.ll")), 19);
        assert_eq!(module_major(Path::new("/c/kiln/llvm22/a/b.ll")), 22);
        assert_eq!(module_major(Path::new("/c/x.ll")), 22);
    }
}
