//! `cargo xtask corpus`: the IR hod has to compile, captured from the
//! compilers that write it. Contract: `spec/harness.md` "corpus".
//!
//! `capture` runs kiln over its executing tests and clang over the GCC
//! torture suite and keeps each module, with what its program must do, as
//! an `irdiff` test. `verify` proves the corpus against its manifest and
//! runs it through `irdiff --hybrid`. `census` counts the constructs of the
//! captured modules. `fetch` gets the pinned torture suite.

pub mod capture;
pub mod census;
pub mod entry;
pub mod kiln;
pub mod manifest;
pub mod reasons;
pub mod sha256;
pub mod torture;
pub mod verify;

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::irdiff::process::{self, End, Limits};
use crate::irdiff::{self, Config, MAX_JOBS, Mode, Search};

/// The command's help.
pub const USAGE: &str = "\
usage: cargo xtask corpus capture [options]
       cargo xtask corpus verify [options] [<corpus dir>]
       cargo xtask corpus census [--report <file>] [<corpus dir>]
       cargo xtask corpus reasons [options] [<corpus dir>]
       cargo xtask corpus fetch [--torture <dir>]

capture: runs kiln++ over kiln's executing tests and clang over the GCC
torture suite, and keeps every module whose program can be an irdiff test.

  --source <name>      kiln, gcc-torture or all; repeatable (default: all)
  --llvm <major>       22 or 19; repeatable (default: each one installed)
  --out <dir>          the corpus directory (default: <build root>/corpus)
  --kiln <kiln++>      default: $HOD_KILN, else
                       <build root>/target-kiln/release/kiln++
  --kiln-repo <dir>    kiln's repository (default: $HOD_KILN_REPO, else
                       <build root>/../kiln)
  --torture <dir>      a GCC checkout with the torture suite (default:
                       $HOD_GCC_TESTS, else <build root>/../.toolchain/gcc-tests)
  --limit <N>          only the first N tests of each source and LLVM
  -j <N>, --jobs <N>   tests captured at once (default 3, at most 16)
  --force              capture again what is already in the corpus
  --retry-skipped      try again the tests the last capture skipped

verify: checks the corpus against its manifest, then runs every entry
through irdiff in hybrid mode, with each LLVM the corpus was captured for.

  -j <N>, --jobs <N>   tests run at once (default 3, at most 16)
  --hod <binary>       the hod to test (default: build this workspace's)
  --strict             run irdiff in strict mode instead
  --manifest-only      check the manifest, run nothing
  --keep, -v           as for irdiff

census: counts the constructs of the corpus's modules and writes the report
(default: docs/status/corpus.md of this workspace).

reasons: why hod declines each module, by text, native bitcode and
llvm-dis, first refusal and census, and what a decline costs.

  --hod <binary>       the hod to ask (default: <build dir>/release/hod)
  --before <binary>    an older hod to time the same declines with
  --llc                also time llc on each module's bitcode
  --limit <N>          only the first N entries of each LLVM
  -j <N>, --jobs <N>   modules at once (default 3, at most 16)
  --report <file>      write the report there (default: print it)

fetch: clones the pinned GCC tag, sparsely, into the --torture directory.

exit status: 0; 1 a FAIL or XPASS in verify; 3 an ERROR in verify, a
corpus that differs from its manifest, or a missing required tool; 2 usage
";

/// The directory builds of this workspace share: the parent of the build
/// directory when that is one of several `target-<checkout>` directories
/// (`scripts/env.sh`), else the build directory itself.
pub fn build_root(build_dir: &Path) -> PathBuf {
    let per_checkout =
        build_dir.file_name().is_some_and(|name| name.to_string_lossy().starts_with("target-"));
    match build_dir.parent() {
        Some(parent) if per_checkout => parent.to_path_buf(),
        _ => build_dir.to_path_buf(),
    }
}

/// The corpus directory when none is given.
pub fn default_dir(build_dir: &Path) -> PathBuf {
    build_root(build_dir).join("corpus")
}

fn from_env(name: &str) -> Option<PathBuf> {
    std::env::var_os(name).filter(|value| !value.is_empty()).map(PathBuf::from)
}

fn absolute(path: &Path) -> PathBuf {
    std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf())
}

/// The defaults of `capture` for this workspace and this process's
/// environment.
pub fn capture_defaults(build_dir: &Path) -> capture::Options {
    let root = build_root(build_dir);
    let beside = root.parent().unwrap_or(&root).to_path_buf();
    let mut options = capture::Options::new(default_dir(build_dir));
    options.kiln = from_env("HOD_KILN")
        .unwrap_or_else(|| root.join("target-kiln").join("release").join("kiln++"));
    options.kiln_repo = from_env("HOD_KILN_REPO").unwrap_or_else(|| beside.join("kiln"));
    options.torture =
        from_env("HOD_GCC_TESTS").unwrap_or_else(|| beside.join(".toolchain").join("gcc-tests"));
    options.require_tools = std::env::var_os("HOD_REQUIRE_TOOLS").is_some_and(|v| v == "1");
    options
}

/// The value of the option `name` at `args[*index]`, which is advanced
/// past it.
fn value<'a>(args: &'a [String], index: &mut usize, name: &str) -> Result<&'a str, String> {
    *index += 1;
    args.get(*index).map(String::as_str).ok_or_else(|| format!("{name} needs a value"))
}

fn jobs(text: &str) -> Result<usize, String> {
    match text.parse::<usize>() {
        Ok(jobs) if (1..=MAX_JOBS).contains(&jobs) => Ok(jobs),
        _ => Err(format!("invalid -j '{text}': expected 1 to {MAX_JOBS}")),
    }
}

/// Reads `capture`'s arguments into `options`.
pub fn parse_capture(args: &[String], options: &mut capture::Options) -> Result<(), String> {
    let mut sources = Vec::new();
    let mut index = 0;
    while index < args.len() {
        let arg = args[index].as_str();
        match arg {
            "--source" => match value(args, &mut index, arg)? {
                "all" => sources.extend(capture::SOURCES),
                name => match capture::SOURCES.iter().find(|source| **source == name) {
                    Some(source) => sources.push(*source),
                    None => {
                        return Err(format!(
                            "unknown source '{name}': expected kiln, gcc-torture or all"
                        ));
                    }
                },
            },
            "--llvm" => {
                let text = value(args, &mut index, arg)?;
                match text.parse::<u32>() {
                    Ok(major) if irdiff::tools::LLVM_MAJORS.contains(&major) => {
                        if !options.llvm.contains(&major) {
                            options.llvm.push(major);
                        }
                    }
                    _ => return Err(format!("invalid --llvm '{text}': expected 22 or 19")),
                }
            }
            "--out" => options.out = absolute(Path::new(value(args, &mut index, arg)?)),
            "--kiln" => options.kiln = absolute(Path::new(value(args, &mut index, arg)?)),
            "--kiln-repo" => options.kiln_repo = absolute(Path::new(value(args, &mut index, arg)?)),
            "--torture" => options.torture = absolute(Path::new(value(args, &mut index, arg)?)),
            "--limit" => {
                let text = value(args, &mut index, arg)?;
                options.limit =
                    Some(text.parse().map_err(|_| format!("invalid --limit '{text}'"))?);
            }
            "-j" | "--jobs" => options.jobs = jobs(value(args, &mut index, arg)?)?,
            "--force" => options.force = true,
            "--retry-skipped" => options.retry_skipped = true,
            other => return Err(format!("unknown argument '{other}'")),
        }
        index += 1;
    }
    if !sources.is_empty() {
        options.sources = capture::SOURCES.into_iter().filter(|s| sources.contains(s)).collect();
    }
    Ok(())
}

/// Reads `verify`'s arguments into `options`.
pub fn parse_verify(args: &[String], options: &mut verify::Options) -> Result<(), String> {
    let mut dir = None;
    let mut index = 0;
    while index < args.len() {
        let arg = args[index].as_str();
        match arg {
            "-j" | "--jobs" => options.config.jobs = jobs(value(args, &mut index, arg)?)?,
            "--hod" => options.config.hod = Some(PathBuf::from(value(args, &mut index, arg)?)),
            "--strict" => options.config.mode = Mode::Strict,
            "--manifest-only" => options.manifest_only = true,
            "--keep" => options.config.keep = true,
            "-v" | "--verbose" => options.config.verbose = true,
            other if other.starts_with('-') => return Err(format!("unknown option '{other}'")),
            other if dir.is_none() => dir = Some(absolute(Path::new(other))),
            other => return Err(format!("more than one corpus directory: '{other}'")),
        }
        index += 1;
    }
    if let Some(dir) = dir {
        options.dir = dir;
    }
    Ok(())
}

/// Reads `reasons`'s arguments: the options and where the report goes.
pub fn parse_reasons(
    args: &[String],
    build_dir: &Path,
) -> Result<(reasons::Options, Option<PathBuf>), String> {
    let mut options = reasons::Options {
        dir: default_dir(build_dir),
        hod: build_dir.join("release").join("hod"),
        before: None,
        llc: false,
        jobs: capture::DEFAULT_JOBS,
        limit: None,
        work: build_dir.join("corpus-reasons"),
    };
    let (mut report, mut given, mut index) = (None, false, 0);
    while index < args.len() {
        let arg = args[index].as_str();
        match arg {
            "--hod" => options.hod = absolute(Path::new(value(args, &mut index, arg)?)),
            "--before" => options.before = Some(absolute(Path::new(value(args, &mut index, arg)?))),
            "--llc" => options.llc = true,
            "--limit" => {
                let text = value(args, &mut index, arg)?;
                options.limit =
                    Some(text.parse().map_err(|_| format!("invalid --limit '{text}'"))?);
            }
            "-j" | "--jobs" => options.jobs = jobs(value(args, &mut index, arg)?)?,
            "--report" => report = Some(absolute(Path::new(value(args, &mut index, arg)?))),
            other if other.starts_with('-') => return Err(format!("unknown option '{other}'")),
            other if !given => {
                options.dir = absolute(Path::new(other));
                given = true;
            }
            other => return Err(format!("more than one corpus directory: '{other}'")),
        }
        index += 1;
    }
    Ok((options, report))
}

/// The defaults of `verify` for this workspace and this process's
/// environment.
pub fn verify_defaults(build_dir: &Path) -> verify::Options {
    let mut config = Config::for_workspace();
    config.build_dir = build_dir.to_path_buf();
    config.mode = Mode::Hybrid;
    config.jobs = config.jobs.min(capture::DEFAULT_JOBS);
    verify::Options { dir: default_dir(build_dir), config, manifest_only: false }
}

/// The steps of `fetch`: git's arguments, in order, for a sparse clone of
/// the pinned tag into `dir` that checks out the suite alone.
pub fn fetch_steps(dir: &Path) -> Vec<Vec<String>> {
    let dir = dir.to_string_lossy().into_owned();
    let words = |words: &[&str]| words.iter().map(|word| (*word).to_owned()).collect::<Vec<_>>();
    vec![
        words(&[
            "clone",
            "--quiet",
            "--depth=1",
            "--filter=blob:none",
            "--no-checkout",
            "--branch",
            torture::TAG,
            torture::REPOSITORY,
            &dir,
        ]),
        words(&[
            "-C",
            &dir,
            "sparse-checkout",
            "set",
            "--no-cone",
            &format!("/{}/", torture::EXECUTE_DIR),
        ]),
        words(&["-C", &dir, "checkout", "--quiet", torture::TAG]),
    ]
}

/// `fetch`: gets the torture suite, unless it is there. Returns what to
/// tell the user.
pub fn fetch(dir: &Path, search: &Search) -> Result<String, String> {
    let execute = dir.join(torture::EXECUTE_DIR);
    if execute.is_dir() {
        return Ok(format!(
            "the suite is already at '{}' (revision {})\n",
            execute.display(),
            capture::revision(dir, search)
        ));
    }
    if dir.exists() {
        return Err(format!("'{}' exists and does not hold the suite", dir.display()));
    }
    let git = search.on_path("git").ok_or("git not found")?;
    let limits = Limits { time: Duration::from_mins(30), memory_kib: 8 << 20 };
    for step in fetch_steps(dir) {
        let output = process::run(&git, |command| _ = command.args(&step), b"", limits)?;
        if output.end != End::Exit(0) {
            return Err(format!("git {} failed: {}", step.join(" "), output.first_error_line()));
        }
    }
    if !execute.is_dir() {
        return Err(format!("the clone has no '{}'", torture::EXECUTE_DIR));
    }
    Ok(format!(
        "fetched {} at {} into '{}' (revision {})\n",
        torture::REPOSITORY,
        torture::TAG,
        dir.display(),
        capture::revision(dir, search)
    ))
}

/// `cargo xtask corpus reasons`.
fn reasons_command(args: &[String], build_dir: &Path) -> u8 {
    let (options, report) = match parse_reasons(args, build_dir) {
        Ok(parsed) => parsed,
        Err(message) => {
            eprint!("xtask corpus: {message}\n{USAGE}");
            return 2;
        }
    };
    let written = reasons::run(&options).and_then(|text| match report {
        Some(path) => std::fs::write(&path, text)
            .map(|()| format!("wrote {}\n", path.display()))
            .map_err(|e| format!("cannot write '{}': {e}", path.display())),
        None => Ok(text),
    });
    match written {
        Ok(text) => {
            print!("{text}");
            0
        }
        Err(message) => {
            eprintln!("xtask corpus: {message}");
            2
        }
    }
}

/// Runs `cargo xtask corpus <args>` and returns the exit status.
pub fn command(args: &[String]) -> u8 {
    let build_dir = irdiff::default_build_dir();
    let usage = |message: &str| {
        eprint!("xtask corpus: {message}\n{USAGE}");
        2
    };
    let failed = |message: &str| {
        eprintln!("xtask corpus: {message}");
        2
    };
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        print!("{USAGE}");
        return 0;
    }
    let rest = args.get(1..).unwrap_or_default();
    match args.first().map(String::as_str) {
        Some("capture") => {
            let mut options = capture_defaults(&build_dir);
            if let Err(message) = parse_capture(rest, &mut options) {
                return usage(&message);
            }
            match capture::capture(&options) {
                Ok(report) => {
                    print!("{}", report.summary(&options.out));
                    report.exit_status()
                }
                Err(message) => failed(&message),
            }
        }
        Some("verify") => {
            let mut options = verify_defaults(&build_dir);
            if let Err(message) = parse_verify(rest, &mut options) {
                return usage(&message);
            }
            match verify::verify(&options) {
                Ok(report) => {
                    print!("{}", report.summary(options.config.verbose));
                    report.exit_status()
                }
                Err(message) => failed(&message),
            }
        }
        Some("census") => {
            let mut dir = default_dir(&build_dir);
            let mut report = irdiff::workspace_root().join("docs").join("status").join("corpus.md");
            let mut index = 0;
            let mut given = false;
            while index < rest.len() {
                match rest[index].as_str() {
                    "--report" => match value(rest, &mut index, "--report") {
                        Ok(path) => report = absolute(Path::new(path)),
                        Err(message) => return usage(&message),
                    },
                    other if other.starts_with('-') => {
                        return usage(&format!("unknown option '{other}'"));
                    }
                    other if !given => {
                        dir = absolute(Path::new(other));
                        given = true;
                    }
                    other => return usage(&format!("more than one corpus directory: '{other}'")),
                }
                index += 1;
            }
            match census::write_report(&dir, &report) {
                Ok(summary) => {
                    print!("{summary}");
                    0
                }
                Err(message) => failed(&message),
            }
        }
        Some("reasons") => reasons_command(rest, &build_dir),
        Some("fetch") => {
            let mut options = capture_defaults(&build_dir);
            let mut index = 0;
            while index < rest.len() {
                match rest[index].as_str() {
                    "--torture" => match value(rest, &mut index, "--torture") {
                        Ok(path) => options.torture = absolute(Path::new(path)),
                        Err(message) => return usage(&message),
                    },
                    other => return usage(&format!("unknown argument '{other}'")),
                }
                index += 1;
            }
            match fetch(&options.torture, &options.search) {
                Ok(text) => {
                    print!("{text}");
                    0
                }
                Err(message) => failed(&message),
            }
        }
        Some(other) => usage(&format!("unknown subcommand '{other}'")),
        None => usage("a subcommand is needed"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(words: &[&str]) -> Vec<String> {
        words.iter().map(|word| (*word).to_owned()).collect()
    }

    // spec/harness.md "corpus", "Where things are".
    #[test]
    fn default_places() {
        assert_eq!(build_root(Path::new("/w/.build/target-hod")), Path::new("/w/.build"));
        assert_eq!(build_root(Path::new("/w/hod/target")), Path::new("/w/hod/target"));
        assert_eq!(
            default_dir(Path::new("/w/.build/target-wp-017")),
            Path::new("/w/.build/corpus")
        );
        assert_eq!(default_dir(Path::new("/w/hod/target")), Path::new("/w/hod/target/corpus"));
    }

    // "capture": the command line.
    #[test]
    fn capture_arguments() {
        let parse = |words: &[&str]| {
            let mut options = capture::Options::new(PathBuf::from("/corpus"));
            parse_capture(&strings(words), &mut options).map(|()| options)
        };
        let options = parse(&[]).unwrap();
        assert_eq!(options.sources, ["kiln", "gcc-torture"]);
        assert!(options.llvm.is_empty() && options.limit.is_none());
        assert!(options.jobs <= capture::DEFAULT_JOBS && !options.force && !options.retry_skipped);

        let options = parse(&[
            "--source",
            "gcc-torture",
            "--llvm",
            "19",
            "--llvm",
            "22",
            "--llvm",
            "19",
            "--out",
            "/o",
            "--kiln",
            "/k/kiln++",
            "--kiln-repo",
            "/k",
            "--torture",
            "/t",
            "--limit",
            "7",
            "-j",
            "2",
            "--force",
            "--retry-skipped",
        ])
        .unwrap();
        assert_eq!(options.sources, ["gcc-torture"]);
        assert_eq!(options.llvm, [19, 22]);
        assert_eq!(options.out, Path::new("/o"));
        assert_eq!(
            (options.kiln.as_path(), options.kiln_repo.as_path()),
            (Path::new("/k/kiln++"), Path::new("/k"))
        );
        assert_eq!(options.torture, Path::new("/t"));
        assert_eq!((options.limit, options.jobs), (Some(7), 2));
        assert!(options.force && options.retry_skipped);
        assert_eq!(
            parse(&["--source", "gcc-torture", "--source", "kiln"]).unwrap().sources,
            ["kiln", "gcc-torture"]
        );
        assert_eq!(parse(&["--source", "all"]).unwrap().sources, ["kiln", "gcc-torture"]);

        for (words, message) in [
            (&["--source", "llvm"][..], "unknown source 'llvm'"),
            (&["--llvm", "18"], "invalid --llvm '18'"),
            (&["--llvm"], "--llvm needs a value"),
            (&["--limit", "x"], "invalid --limit 'x'"),
            (&["-j", "0"], "invalid -j '0'"),
            (&["-j", "17"], "invalid -j '17'"),
            (&["extra"], "unknown argument 'extra'"),
        ] {
            let error = parse(words).unwrap_err();
            assert!(error.contains(message), "{words:?}: {error}");
        }
    }

    // "verify": the command line.
    #[test]
    fn verify_arguments() {
        let parse = |words: &[&str]| {
            let mut options = verify_defaults(Path::new("/w/.build/target-x"));
            parse_verify(&strings(words), &mut options).map(|()| options)
        };
        let options = parse(&[]).unwrap();
        assert_eq!(options.dir, Path::new("/w/.build/corpus"));
        assert_eq!(options.config.mode, Mode::Hybrid);
        assert!(options.config.jobs <= capture::DEFAULT_JOBS && !options.manifest_only);
        let options =
            parse(&["/c", "--strict", "-j", "5", "--hod", "/h", "--manifest-only", "--keep", "-v"])
                .unwrap();
        assert_eq!(options.dir, Path::new("/c"));
        assert_eq!((options.config.mode, options.config.jobs), (Mode::Strict, 5));
        assert_eq!(options.config.hod.as_deref(), Some(Path::new("/h")));
        assert!(options.manifest_only && options.config.keep && options.config.verbose);
        assert!(parse(&["/a", "/b"]).unwrap_err().contains("more than one"));
        assert!(parse(&["--nope"]).unwrap_err().contains("unknown option"));
    }

    // "fetch": the pinned clone.
    #[test]
    fn fetch_is_pinned_and_sparse() {
        let steps = fetch_steps(Path::new("/t/gcc-tests"));
        assert_eq!(steps.len(), 3);
        assert_eq!(steps[0][0], "clone");
        assert!(steps[0].contains(&"releases/gcc-16.1.0".to_owned()));
        assert!(steps[0].contains(&"https://github.com/gcc-mirror/gcc.git".to_owned()));
        assert!(steps[0].contains(&"--filter=blob:none".to_owned()));
        assert_eq!(steps[1].last().unwrap(), "/gcc/testsuite/gcc.c-torture/execute/");
        assert_eq!(steps[2][2..], ["checkout", "--quiet", "releases/gcc-16.1.0"]);
    }
}
