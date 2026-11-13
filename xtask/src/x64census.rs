//! `cargo xtask x64census`: x86-64 readiness of the target-neutral half
//! (WP-138; design 0006 §1.1, §6.5). Contract: `spec/harness.md`
//! "x64census".
//!
//! Every `.ll` file under the directories given goes through:
//!
//! 1. `ll`: `hod_ll` reads the module and every body; each body and the
//!    module's symbols verify (`roundtrip`'s in-process half);
//! 2. `print`: `hod_ir`'s printer writes it in the dialect of the file's
//!    LLVM major (the `llvmNN` component of its path; 22 without one),
//!    `llvm-as` of that major accepts the text, and reading and printing
//!    it again gives the same text;
//! 3. `facade`: the text and its `llvm-as` bitcode go through
//!    `hod::compile_with` with every row of the classify matrix that is
//!    not refused by design taken as supported, and every reason
//!    collected. Both routes must give the same reasons. The only reason
//!    the back half of a ready front half gives is the code generator's
//!    (`codegen-not-implemented`, or an `x64:` name of `hod_x64`); every
//!    other reason is the front half's, by name;
//! 4. with `--shipped`: the reasons of the shipped matrix, counted (what
//!    the x86-64 column refuses);
//! 5. with `--interp`: for a module with `main` whose run in
//!    `hod_interp` ends (`abort` and `exit` are host functions), the
//!    exit status equals that of the module built by `llc -O0
//!    -fast-isel=0 -relocation-model=pic`, linked with
//!    `x86_64-linux-gnu-gcc` and run under `qemu-x86_64`.
//!
//! The output is one line per module that is not ready, and the census:
//! modules per stage outcome and refusals by name.

use std::cell::Cell;
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Mutex;
use std::time::Duration;

use crate::irdiff::directives;
use crate::irdiff::process::{self, End, Limits};
use crate::irdiff::tools::Search;
use crate::roundtrip::ingest::ingest;

pub const USAGE: &str = "\
usage: cargo xtask x64census [--interp] [--shipped] [-j <N>] [--keep <dir>] <dir or file.ll>...
  --interp   run `main` in hod_interp and compare with llc + qemu-x86_64
  --shipped  also count the reasons of the shipped x86-64 matrix
  -j <N>     modules at a time (default 4)
  --keep <dir>  where the oracle's objects and programs are built (default: a temporary directory)
";

/// The reasons that belong to the code generator, not the front half.
fn is_codegen_reason(reason: &str) -> bool {
    reason == "codegen-not-implemented" || reason.starts_with("x64:")
}

#[derive(Default)]
struct Census {
    modules: usize,
    by_route: BTreeMap<String, usize>,
    stage: BTreeMap<String, usize>,
    front_reasons: BTreeMap<String, usize>,
    shipped_first: BTreeMap<String, usize>,
    shipped_all: BTreeMap<String, usize>,
    interp: BTreeMap<String, usize>,
    lines: Vec<String>,
}

impl Census {
    fn count(map: &mut BTreeMap<String, usize>, key: impl Into<String>) {
        *map.entry(key.into()).or_default() += 1;
    }
}

struct Config {
    interp: bool,
    shipped: bool,
    jobs: usize,
    keep: Option<PathBuf>,
    inputs: Vec<PathBuf>,
}

fn parse_args(args: &[String]) -> Result<Option<Config>, String> {
    let mut config = Config { interp: false, shipped: false, jobs: 4, keep: None, inputs: vec![] };
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--help" | "-h" => return Ok(None),
            "--interp" => config.interp = true,
            "--shipped" => config.shipped = true,
            "-j" => {
                let n = args.next().ok_or("-j needs a number")?;
                config.jobs = n.parse().map_err(|_| format!("-j {n}: not a number"))?;
                config.jobs = config.jobs.clamp(1, 4);
            }
            "--keep" => config.keep = Some(PathBuf::from(args.next().ok_or("--keep needs a dir")?)),
            other if other.starts_with('-') => return Err(format!("unknown option {other}")),
            path => config.inputs.push(PathBuf::from(path)),
        }
    }
    if config.inputs.is_empty() {
        return Err("no input".into());
    }
    Ok(Some(config))
}

/// The command; returns the exit status (0: every module is ready).
pub fn command(args: &[String]) -> u8 {
    let config = match parse_args(args) {
        Ok(Some(config)) => config,
        Ok(None) => {
            print!("{USAGE}");
            return 0;
        }
        Err(message) => {
            eprint!("xtask x64census: {message}\n{USAGE}");
            return 2;
        }
    };
    let mut files = Vec::new();
    for input in &config.inputs {
        collect(input, &mut files);
    }
    files.sort();
    let files = units(files);
    let scratch = match &config.keep {
        Some(dir) => dir.clone(),
        None => std::env::temp_dir().join(format!("hod-x64census-{}", std::process::id())),
    };
    if config.interp
        && let Err(e) = std::fs::create_dir_all(&scratch)
    {
        eprintln!("xtask x64census: {}: {e}", scratch.display());
        return 2;
    }
    let census = Mutex::new(Census::default());
    let next = Mutex::new(0usize);
    std::thread::scope(|scope| {
        for _ in 0..config.jobs {
            scope.spawn(|| {
                loop {
                    let index = {
                        let mut next =
                            next.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                        let index = *next;
                        *next += 1;
                        index
                    };
                    let Some((path, part)) = files.get(index) else { break };
                    let result = check(path, *part, &config, &scratch, index);
                    let label = match part {
                        Some(part) => format!("{}#{}", path.display(), part + 1),
                        None => path.display().to_string(),
                    };
                    let mut census =
                        census.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                    merge(&mut census, &label, &result);
                }
            });
        }
    });
    if config.keep.is_none() && config.interp {
        let _ = std::fs::remove_dir_all(&scratch);
    }
    let census = census.into_inner().unwrap_or_else(std::sync::PoisonError::into_inner);
    print!("{}", report(&census));
    let ready = census.stage.get("ready").copied().unwrap_or(0);
    u8::from(ready != census.modules)
}

fn collect(path: &Path, out: &mut Vec<PathBuf>) {
    if path.is_dir() {
        let Ok(entries) = std::fs::read_dir(path) else { return };
        for entry in entries.flatten() {
            collect(&entry.path(), out);
        }
    } else if path.extension().is_some_and(|e| e == "ll") {
        out.push(path.to_path_buf());
    }
}

/// The modules to check: each file, and each module of a file of several
/// (`;@ module:` directives, a corpus entry of several translation units)
/// by its index.
fn units(files: Vec<PathBuf>) -> Vec<(PathBuf, Option<usize>)> {
    let mut units = Vec::new();
    for path in files {
        let count =
            std::fs::read_to_string(&path).map_or(0, |text| directives::modules(&text).len());
        if count == 0 {
            units.push((path, None));
        } else {
            units.extend((0..count).map(|part| (path.clone(), Some(part))));
        }
    }
    units
}

/// The LLVM major a path is under (`…/llvm19/…`); 22 without one.
fn major_of(path: &Path) -> u32 {
    path.components()
        .filter_map(|part| part.as_os_str().to_str())
        .find_map(|part| part.strip_prefix("llvm").and_then(|n| n.parse().ok()))
        .unwrap_or(22)
}

/// What became of one module.
#[derive(Default)]
struct Result1 {
    major: u32,
    /// `ready`, or the first stage that failed (`ll:...`, `print:...`,
    /// `facade:...`).
    stage: String,
    detail: String,
    front_reasons: Vec<String>,
    shipped: Vec<String>,
    interp: Option<String>,
}

const TOOL_LIMITS: Limits = Limits { time: Duration::from_mins(2), memory_kib: 8 << 20 };

#[allow(clippy::too_many_lines, reason = "the stages in order, one after another")]
fn check(
    path: &Path,
    part: Option<usize>,
    config: &Config,
    scratch: &Path,
    index: usize,
) -> Result1 {
    let major = major_of(path);
    let mut r = Result1 { major, ..Result1::default() };
    let source = match std::fs::read(path) {
        Ok(source) => match part {
            Some(part) => directives::modules(&String::from_utf8_lossy(&source))
                .into_iter()
                .nth(part)
                .map(|module| module.text.into_bytes())
                .unwrap_or_default(),
            None => source,
        },
        Err(e) => {
            r.stage = "harness:read".into();
            r.detail = e.to_string();
            return r;
        }
    };
    let search = Search::from_env();
    // 1 and 2: read, verify, print, llvm-as, read and print again.
    let printed = match ingest(&source, major) {
        Ok(printed) => printed,
        Err((outcome, detail)) => {
            r.stage = format!("ll:{}", outcome_name(outcome));
            r.detail = detail;
            return r;
        }
    };
    let Some(llvm_as) = search.versioned("llvm-as", major) else {
        r.stage = "harness:no-llvm-as".into();
        return r;
    };
    match process::run(
        &llvm_as,
        |c| {
            c.args(["-", "-o", "/dev/null"]);
        },
        printed.as_bytes(),
        TOOL_LIMITS,
    ) {
        Ok(out) if out.end == End::Exit(0) => {}
        Ok(out) => {
            r.stage = "print:llvm-as-refused".into();
            r.detail = out.first_error_line();
            return r;
        }
        Err(e) => {
            r.stage = "harness:llvm-as".into();
            r.detail = e;
            return r;
        }
    }
    match ingest(printed.as_bytes(), major) {
        Ok(again) if again == printed || renamed(&again) == renamed(&printed) => {}
        Ok(again) => {
            r.stage = "print:not-a-fixed-point".into();
            r.detail = first_difference(&renamed(&printed), &renamed(&again));
            return r;
        }
        Err((outcome, detail)) => {
            r.stage = format!("print:reread-{}", outcome_name(outcome));
            r.detail = detail;
            return r;
        }
    }
    // 3: the facade on both routes.
    let bitcode = match process::run(
        &llvm_as,
        |c| {
            c.args(["-", "-o", "-"]);
        },
        &source,
        TOOL_LIMITS,
    ) {
        Ok(out) if out.end == End::Exit(0) => out.stdout,
        Ok(out) => {
            r.stage = "harness:llvm-as-source".into();
            r.detail = out.first_error_line();
            return r;
        }
        Err(e) => {
            r.stage = "harness:llvm-as".into();
            r.detail = e;
            return r;
        }
    };
    let everything = hod_ir::Support::everything_implemented(hod_ir::Arch::X86_64);
    let text = reasons(&source, Some(everything.clone()));
    let bc = reasons(&bitcode, Some(everything));
    if text != bc {
        r.stage = "facade:routes-differ".into();
        r.detail = format!("text {text:?} bitcode {bc:?}");
        return r;
    }
    let front: Vec<String> = match text {
        Ok(reasons) => reasons.into_iter().filter(|reason| !is_codegen_reason(reason)).collect(),
        Err(message) => {
            r.stage = "facade:error".into();
            r.detail = message;
            return r;
        }
    };
    if config.shipped {
        r.shipped = reasons(&source, None).unwrap_or_else(|e| vec![format!("error: {e}")]);
    }
    if front.is_empty() {
        r.stage = "ready".into();
    } else {
        r.stage = "facade:front-refuses".into();
        r.detail = front.join(", ");
        r.front_reasons = front;
    }
    // 5: the interpreter against llc + qemu; a module of several is a
    // part of a program.
    if config.interp && part.is_none() {
        r.interp =
            Some(interp_vs_oracle(&source, major, &search, &scratch.join(index.to_string())));
    }
    r
}

/// Every reason the facade gives, in order (`Ok(empty)` cannot happen
/// for x86-64: the code generator's reason is always there).
fn reasons(input: &[u8], support: Option<hod_ir::Support>) -> Result<Vec<String>, String> {
    let options = hod::Options {
        output_kind: hod::OutputKind::Object,
        opt_level: hod::OptLevel::O0,
        relocation_model: Some(hod::RelocModel::Pic),
        ..hod::Options::default()
    };
    let settings = hod::Settings { census: true, support, ..hod::Settings::default() };
    let mut report = hod::Report::default();
    let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        hod::compile_with(input, &options, &settings, &mut report)
    }));
    match caught {
        Err(_) => Err("panicked".into()),
        Ok(Ok(_)) => Ok(vec![]),
        Ok(Err(hod::CompileError::Unsupported(u))) => {
            if report.reasons.is_empty() {
                Ok(vec![u.construct.name().to_owned()])
            } else {
                Ok(report.reasons)
            }
        }
        Ok(Err(e)) => Err(e.to_string()),
    }
}

fn outcome_name(outcome: crate::roundtrip::Outcome) -> String {
    format!("{outcome:?}")
}

/// `text` with every `%name` replaced by its rank of first appearance:
/// the printer names the values it makes (`%v6.m0`), and a second
/// reading keeps names the first did not have, so two prints of one
/// module are compared up to the names of local values and types,
/// numbered afresh in each function.
fn renamed(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut names: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    let mut i = 0;
    let mut copied = 0;
    while i < bytes.len() {
        // Local names are per function.
        if bytes[i..].starts_with(b"\ndefine ") {
            names.clear();
        }
        if bytes[i] != b'%' {
            i += 1;
            continue;
        }
        let start = i;
        i += 1;
        if bytes.get(i) == Some(&b'"') {
            i += 1;
            while i < bytes.len() && bytes[i] != b'"' {
                i += 1;
            }
            i = (i + 1).min(bytes.len());
        } else {
            while i < bytes.len()
                && (bytes[i].is_ascii_alphanumeric()
                    || matches!(bytes[i], b'.' | b'_' | b'$' | b'-'))
            {
                i += 1;
            }
        }
        if i == start + 1 {
            continue;
        }
        let name = &text[start..i];
        let next = names.len();
        let rank = *names.entry(name).or_insert(next);
        out.push_str(&text[copied..start]);
        let _ = write!(out, "%#{rank}");
        copied = i;
    }
    out.push_str(&text[copied..]);
    out
}

fn first_difference(a: &str, b: &str) -> String {
    for (n, (x, y)) in a.lines().zip(b.lines()).enumerate() {
        if x != y {
            return format!("line {}: `{x}` then `{y}`", n + 1);
        }
    }
    format!("lengths {} and {}", a.lines().count(), b.lines().count())
}

/// How a program ended: an exit status, or `abort`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Ending {
    Exit(u8),
    Abort,
}

/// Runs `main` in the interpreter; `Err` is why no ending is known.
fn interpret(source: &[u8]) -> Result<Ending, String> {
    use hod_interp::{Interp, Program, Stop, Trap, Val};
    use hod_ir::TypeKind;
    let mut parsed = hod_ll::parse_module(source, &hod_ll::ParseOptions::default())
        .map_err(|e| format!("parse: {e}"))?;
    let mut parser = hod_ll::BodyParser::new(&parsed.text);
    let mut bodies = Vec::new();
    for function in parsed.text.functions() {
        if let Ok(Some(body)) = parser.parse(&mut parsed.module, function) {
            bodies.push(body);
        }
    }
    let module = &parsed.module;
    let main = module.find_symbol(b"main").ok_or("no-main")?;
    let program = Program::with_bodies(module, &bodies).map_err(|e| format!("program: {e:?}"))?;
    let TypeKind::Function { params, .. } = module.types.kind(module.symbol(main).ty()) else {
        return Err("no-main".into());
    };
    let args: Vec<Val> = match params.len() {
        0 => vec![],
        2 => vec![Val::int(32, 1), Val::Ptr(0)],
        _ => return Err("main-signature".into()),
    };
    let ending: Rc<Cell<Option<Ending>>> = Rc::new(Cell::new(None));
    let mut interp = Interp::new(&program);
    let at_abort = Rc::clone(&ending);
    interp.host("abort", move |_, _| {
        at_abort.set(Some(Ending::Abort));
        Err(Trap::Host)
    });
    for name in ["exit", "_exit", "_Exit"] {
        let at_exit = Rc::clone(&ending);
        interp.host(name, move |_, args| {
            let status = args.first().and_then(Val::as_int).unwrap_or(0);
            at_exit.set(Some(Ending::Exit(status as u8)));
            Err(Trap::Host)
        });
    }
    let outcome = interp.run(main, &args);
    match outcome.result {
        Ok(Some(value)) => Ok(Ending::Exit(value.as_int().unwrap_or(0) as u8)),
        Ok(None) => Ok(Ending::Exit(0)),
        Err(Stop::Trap(Trap::Host)) => ending.get().ok_or_else(|| "trap:host".into()),
        Err(Stop::Trap(trap)) => {
            let at = outcome.stopped_at.map_or_else(String::new, |(sym, inst)| {
                format!(" in @{} at {inst:?}", String::from_utf8_lossy(module.symbol(sym).name()))
            });
            Err(format!("trap:{}{at}", trap.name()))
        }
        Err(Stop::Unsupported(reason)) => Err(format!("unsupported:{reason}")),
        Err(Stop::Invalid(text)) => Err(format!("INVALID:{text}")),
    }
}

/// `agree`, `DIFFERS: ...`, or why there is nothing to compare.
fn interp_vs_oracle(source: &[u8], major: u32, search: &Search, dir: &Path) -> String {
    let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| interpret(source)));
    let want = match caught {
        Err(_) => return "INTERP-PANICKED".into(),
        Ok(Err(why)) => return why,
        Ok(Ok(ending)) => ending,
    };
    let (Some(llc), Some(cc), Some(qemu)) = (
        search.versioned("llc", major),
        search.on_path("x86_64-linux-gnu-gcc"),
        search.on_path("qemu-x86_64"),
    ) else {
        return "oracle:no-tools".into();
    };
    if std::fs::create_dir_all(dir).is_err() {
        return "oracle:scratch".into();
    }
    let (obj, exe) = (dir.join("m.o"), dir.join("m"));
    let built = process::run(
        &llc,
        |c| {
            c.args([
                "-O0",
                "-fast-isel=0",
                "-mtriple=x86_64-unknown-linux-gnu",
                "-relocation-model=pic",
                "-filetype=obj",
                "-",
                "-o",
            ])
            .arg(&obj);
        },
        source,
        TOOL_LIMITS,
    );
    if !matches!(built, Ok(ref out) if out.end == End::Exit(0)) {
        return "oracle:llc-failed".into();
    }
    let linked = process::run(
        &cc,
        |c| {
            c.arg(&obj).arg("-o").arg(&exe).arg("-lm");
        },
        b"",
        TOOL_LIMITS,
    );
    if !matches!(linked, Ok(ref out) if out.end == End::Exit(0)) {
        return "oracle:link-failed".into();
    }
    let ran = process::run(
        &qemu,
        |c| {
            c.args(["-L", "/usr/x86_64-linux-gnu"]).arg(&exe);
        },
        b"",
        TOOL_LIMITS,
    );
    let got = match ran {
        Ok(out) => match out.end {
            End::Exit(134) | End::Signal(6) => Ending::Abort,
            End::Exit(status) => Ending::Exit(status as u8),
            other => return format!("oracle:{other:?}"),
        },
        Err(e) => return format!("oracle:{e}"),
    };
    let _ = std::fs::remove_dir_all(dir);
    if got == want { "agree".into() } else { format!("DIFFERS: interp {want:?} llc {got:?}") }
}

fn merge(census: &mut Census, label: &str, r: &Result1) {
    census.modules += 1;
    Census::count(&mut census.by_route, format!("llvm{}", r.major));
    let stage_key = r.stage.clone();
    Census::count(&mut census.stage, stage_key);
    for reason in &r.front_reasons {
        Census::count(&mut census.front_reasons, reason.clone());
    }
    if let Some(first) = r.shipped.first() {
        Census::count(&mut census.shipped_first, first.clone());
    }
    for reason in &r.shipped {
        Census::count(&mut census.shipped_all, reason.clone());
    }
    if let Some(interp) = &r.interp {
        if interp.starts_with("trap:") || interp.starts_with("oracle:") {
            census.lines.push(format!("{label}: interp {interp}"));
        }
        let key = if interp.starts_with("trap:") {
            interp.split(' ').next().unwrap_or(interp).to_owned()
        } else if interp.starts_with("DIFFERS")
            || interp.starts_with("INTERP")
            || interp.starts_with("INVALID")
        {
            census.lines.push(format!("{label}: interp {interp}"));
            interp.split(':').next().unwrap_or(interp).to_owned()
        } else {
            interp.clone()
        };
        Census::count(&mut census.interp, key);
    }
    if r.stage != "ready" {
        census.lines.push(format!("{label}: {}: {}", r.stage, r.detail));
    }
}

fn report(census: &Census) -> String {
    let mut out = String::new();
    for line in &census.lines {
        let _ = writeln!(out, "{line}");
    }
    let _ = writeln!(out, "x64census: {} modules", census.modules);
    let section = |out: &mut String, title: &str, map: &BTreeMap<String, usize>| {
        if map.is_empty() {
            return;
        }
        let _ = writeln!(out, "{title}:");
        let mut rows: Vec<_> = map.iter().collect();
        rows.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
        for (key, n) in rows {
            let _ = writeln!(out, "  {n:5}  {key}");
        }
    };
    section(&mut out, "by LLVM major", &census.by_route);
    section(&mut out, "by stage", &census.stage);
    section(&mut out, "front-half refusals (modules)", &census.front_reasons);
    section(&mut out, "shipped matrix, first reason", &census.shipped_first);
    section(&mut out, "shipped matrix, every reason (modules)", &census.shipped_all);
    section(&mut out, "interpreter vs llc + qemu", &census.interp);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arguments() {
        let args =
            |list: &[&str]| parse_args(&list.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>());
        assert!(args(&[]).is_err());
        assert!(args(&["--help"]).unwrap().is_none());
        let c = args(&["--interp", "-j", "9", "d"]).unwrap().unwrap();
        assert!(c.interp && !c.shipped && c.jobs == 4 && c.inputs == [PathBuf::from("d")]);
        assert!(args(&["--bogus", "d"]).is_err());
    }

    #[test]
    fn majors_and_reasons() {
        assert_eq!(major_of(Path::new("x/llvm19/kiln/a.ll")), 19);
        assert_eq!(major_of(Path::new("x/kiln/a.ll")), 22);
        assert!(is_codegen_reason("codegen-not-implemented"));
        assert!(is_codegen_reason("x64:todo:inst:add"));
        assert!(!is_codegen_reason("type:x86_mmx"));
        assert_eq!(renamed("%a = add i32 %b, %a ; %\"x y\" 5%"), "%#0 = add i32 %#1, %#0 ; %#2 5%");
    }

    // spec/harness.md "x64census": a corpus entry of several modules is
    // checked module by module, not read as one module.
    #[test]
    fn several_modules_are_units() {
        let scratch = hod_testutil::Scratch::new("x64census-units");
        let one = scratch.path().join("one.ll");
        let two = scratch.path().join("two.ll");
        std::fs::write(&one, "define void @f() {\n  ret void\n}\n").unwrap();
        std::fs::write(
            &two,
            concat!(
                ";@ module: a.ll\ntarget triple = \"x86_64-unknown-linux-gnu\"\n@x = global i32 0\n",
                ";@ module: b.ll\ntarget triple = \"x86_64-unknown-linux-gnu\"\n@x = global i32 1\n",
            ),
        )
        .unwrap();
        let units = units(vec![one.clone(), two.clone()]);
        assert_eq!(units, [(one, None), (two.clone(), Some(0)), (two.clone(), Some(1))]);
        let config = parse_args(&["d".to_owned()]).unwrap().unwrap();
        for part in [0, 1] {
            let r = check(&two, Some(part), &config, scratch.path(), part);
            assert!(!r.stage.starts_with("ll:"), "module {part}: {} {}", r.stage, r.detail);
        }
    }

    // spec/harness.md "x64census": an x86-64 module the front half takes
    // whole is refused by the code generator alone, on both routes.
    #[test]
    fn a_ready_module() {
        let m = b"target datalayout = \"e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128\"\n\
                  target triple = \"x86_64-unknown-linux-gnu\"\n\
                  define i32 @main() {\n  ret i32 3\n}\n";
        let everything = hod_ir::Support::everything_implemented(hod_ir::Arch::X86_64);
        let reasons = reasons(m, Some(everything)).unwrap();
        assert!(reasons.iter().all(|r| is_codegen_reason(r)), "{reasons:?}");
        assert_eq!(interpret(m), Ok(Ending::Exit(3)));
    }
}
