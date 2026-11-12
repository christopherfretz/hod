//! `cargo xtask debug`: layer L7 of `docs/VALIDATION.md`, debug
//! information under the debuggers. Contract: `spec/harness.md` "debug";
//! `spec/dwarf.md` §8.4.
//!
//! - `run` (the default): the debugger tests of `tests/debug/`, each built
//!   by hod and by `llc`, linked by every linker, with the same gdb and
//!   lldb sessions on both programs ([`sessions`]); and the
//!   `(function, line)` comparison of the two programs ([`lines`]).
//! - `kiln`: kiln's own debugger tests with hod as kiln's backend
//!   ([`kiln`]).
//! - `verify`: every `-g` module of the corpus hod builds is
//!   `llvm-dwarfdump --verify` clean ([`verify`]).
//! - `columns`: the line tables, with columns, of hod's and `llc`'s
//!   objects of every `-g` module of the corpus ([`columns`]).

pub mod build;
pub mod columns;
pub mod directives;
pub mod kiln;
pub mod lines;
pub mod permitted;
pub mod sessions;
pub mod transcript;
pub mod verify;

use std::path::PathBuf;

/// The command's help.
pub const USAGE: &str = "\
usage: cargo xtask debug [run] [options] [<test or directory>...]
       cargo xtask debug kiln [--kiln <kiln++>] [--kiln-repo <dir>] [--hod <binary>]
       cargo xtask debug verify [-j <N>] [--hod <binary>] [-v] [<corpus dir or .ll>...]
       cargo xtask debug columns [-j <N>] [--hod <binary>] [--dump <dir>] [-v] [<dir or .ll>...]

run: builds each debugger test (default: tests/debug/) with clang 19, clang 22
and kiln++ as front ends, compiles the IR with hod and with llc -O0
(SelectionDAG), links both with every linker, runs the test's gdb and lldb
scripts on both programs and compares the masked transcripts; also compares
the (function, line) pairs of the two programs.

  -j <N>, --jobs <N>     tests run at once (default 4)
  --hod <binary>         the hod to test (default: build this workspace's)
  --linker <name>        bfd, lld, mold or wild; repeatable (default: every one)
  --debugger <name>      gdb, lldb, lldb-19 or lldb-22; repeatable (default: all)
  --frontend <name>      clang-19, clang-22, kiln, ir-19, ir-22; repeatable
  --tier <quick|all>     the tests to run (default: all)
  --no-lines             no (function, line) comparison
  --kiln <kiln++>        the kiln front end (default: the build root's)
  --keep                 keep the work directories of failing tests
  --results <file>       the results file (default: <build>/l7/results.tsv)
  -v, --verbose          list every result and every lines difference

kiln: kiln's tests/debuginfo, compiled by kiln++ with llc and with hod (strict).
verify: llvm-dwarfdump --verify on every -g module hod builds (default: the
corpus).
columns: hod's and llc's line rows (function, line, column) of every -g module
(default: the corpus); --dump writes hod's disassembly per module.

exit status: 0 no failure; 1 a FAIL, XPASS or debugger crash on hod's program
(verify: a module that does not verify; kiln: a test that passes with llc and
not with hod); 3 an ERROR or a missing required tool; 2 usage
";

fn value(args: &[String], i: &mut usize, name: &str) -> Result<String, String> {
    *i += 1;
    args.get(*i).cloned().ok_or_else(|| format!("{name} needs a value"))
}

/// Splits `--name=value` into its two parts.
fn split(arg: &str) -> (&str, Option<&str>) {
    match arg.split_once('=') {
        Some((name, value)) if name.starts_with("--") => (name, Some(value)),
        _ => (arg, None),
    }
}

/// Parses `run`'s options into `options`. `Ok(false)` is `--help`.
pub fn parse_run(args: &[String], options: &mut sessions::Options) -> Result<bool, String> {
    let mut i = 0;
    let mut paths_only = false;
    while i < args.len() {
        let (name, inline) = split(&args[i]);
        let take = |i: &mut usize| match inline {
            Some(v) => Ok(v.to_owned()),
            None => value(args, i, name),
        };
        if paths_only || !name.starts_with('-') {
            options.paths.push(PathBuf::from(&args[i]));
        } else {
            match name {
                "--" => paths_only = true,
                "-h" | "--help" => return Ok(false),
                "-j" | "--jobs" => {
                    let n = take(&mut i)?;
                    options.jobs = n.parse().ok().filter(|&n| n > 0).ok_or("-j needs a count")?;
                    options.jobs = options.jobs.min(16);
                }
                "--hod" => options.hod = Some(PathBuf::from(take(&mut i)?)),
                "--kiln" => options.kiln = Some(PathBuf::from(take(&mut i)?)),
                "--results" => options.results = Some(PathBuf::from(take(&mut i)?)),
                "--linker" => {
                    let l = take(&mut i)?;
                    if !directives::LINKER_WORDS.contains(&l.as_str()) {
                        return Err(format!("unknown linker '{l}'"));
                    }
                    options.linkers.push(l);
                }
                "--debugger" => {
                    let d = take(&mut i)?;
                    if !directives::DEBUGGER_WORDS.contains(&d.as_str()) {
                        return Err(format!("unknown debugger '{d}'"));
                    }
                    options.debuggers.push(d);
                }
                "--frontend" => {
                    let f = take(&mut i)?;
                    if !directives::FRONTENDS.contains(&f.as_str()) {
                        return Err(format!("unknown front end '{f}'"));
                    }
                    options.frontends.push(f);
                }
                "--tier" => {
                    options.quick = match take(&mut i)?.as_str() {
                        "quick" => true,
                        "all" => false,
                        other => return Err(format!("unknown tier '{other}'")),
                    };
                }
                "--no-lines" => options.lines = false,
                "--keep" => options.keep = true,
                "-v" | "--verbose" => options.verbose = true,
                _ if name.starts_with("-j") && name.len() > 2 => {
                    options.jobs = name[2..].parse().ok().filter(|&n| n > 0).ok_or("bad -j")?;
                    options.jobs = options.jobs.min(16);
                }
                _ => return Err(format!("unknown option '{}'", args[i])),
            }
        }
        i += 1;
    }
    Ok(true)
}

fn require_tools() -> bool {
    std::env::var_os("HOD_REQUIRE_TOOLS").is_some_and(|v| v == "1")
}

/// The L7 step of the full gate (`spec/harness.md` "debug", "In the
/// gate"): every debugger test, then every `-g` module of the corpus
/// verified. The quick gate has no step of its own: `cargo test` runs the
/// quick tests (`xtask/tests/debug_real.rs`).
pub fn gate() -> bool {
    let build_dir = crate::irdiff::default_build_dir();
    let mut options = sessions::Options::new(build_dir.clone());
    options.require_tools = require_tools();
    let sessions_ok = match sessions::run(&options) {
        Ok(report) => {
            eprint!("{}", report.summary(false));
            report.exit_status() == 0
        }
        Err(e) => {
            eprintln!("xtask debug: {e}");
            false
        }
    };
    let options = verify::Options {
        paths: Vec::new(),
        jobs: 4,
        hod: None,
        build_dir,
        require_tools: require_tools(),
        limits: verify::LIMITS,
    };
    let verify_ok = match verify::run(&options) {
        Ok(report) => {
            eprint!("{}", report.summary(false));
            report.exit_status() == 0
        }
        Err(e) => {
            eprintln!("xtask debug verify: {e}");
            false
        }
    };
    sessions_ok && verify_ok
}

/// `cargo xtask debug ...`; the exit status.
#[expect(clippy::too_many_lines, reason = "the three subcommands' options")]
pub fn command(args: &[String]) -> u8 {
    let build_dir = crate::irdiff::default_build_dir();
    let usage = |message: String| {
        eprint!("xtask debug: {message}\n{USAGE}");
        2
    };
    match args.first().map(String::as_str) {
        Some("kiln") => {
            let mut options = kiln::Options {
                kiln: None,
                kiln_repo: None,
                hod: None,
                build_dir,
                require_tools: require_tools(),
            };
            let mut i = 1;
            while i < args.len() {
                let (name, inline) = split(&args[i]);
                let got = match inline {
                    Some(v) => Ok(v.to_owned()),
                    None => value(args, &mut i, name),
                };
                let slot = match name {
                    "--kiln" => &mut options.kiln,
                    "--kiln-repo" => &mut options.kiln_repo,
                    "--hod" => &mut options.hod,
                    _ => return usage(format!("unknown option '{}'", args[i])),
                };
                match got {
                    Ok(v) => *slot = Some(PathBuf::from(v)),
                    Err(e) => return usage(e),
                }
                i += 1;
            }
            match kiln::run(&options) {
                Ok(report) => {
                    print!("{}", report.summary());
                    report.exit_status()
                }
                Err(e) => {
                    eprintln!("xtask debug kiln: {e}");
                    3
                }
            }
        }
        Some("columns") => columns::command(&args[1..], build_dir),
        Some("verify") => {
            let mut options = verify::Options {
                paths: Vec::new(),
                jobs: 4,
                hod: None,
                build_dir,
                require_tools: require_tools(),
                limits: verify::LIMITS,
            };
            let mut verbose = false;
            let mut i = 1;
            while i < args.len() {
                let (name, inline) = split(&args[i]);
                let take = |i: &mut usize| match inline {
                    Some(v) => Ok(v.to_owned()),
                    None => value(args, i, name),
                };
                match name {
                    "-v" | "--verbose" => verbose = true,
                    "-j" | "--jobs" => match take(&mut i).map(|n| n.parse::<usize>()) {
                        Ok(Ok(n)) if n > 0 => options.jobs = n.min(16),
                        _ => return usage("-j needs a count".to_owned()),
                    },
                    "--hod" => match take(&mut i) {
                        Ok(v) => options.hod = Some(PathBuf::from(v)),
                        Err(e) => return usage(e),
                    },
                    _ if !name.starts_with('-') => options.paths.push(PathBuf::from(&args[i])),
                    _ => return usage(format!("unknown option '{}'", args[i])),
                }
                i += 1;
            }
            match verify::run(&options) {
                Ok(report) => {
                    print!("{}", report.summary(verbose));
                    report.exit_status()
                }
                Err(e) => {
                    eprintln!("xtask debug verify: {e}");
                    3
                }
            }
        }
        _ => {
            let rest = if args.first().is_some_and(|a| a == "run") { &args[1..] } else { args };
            let mut options = sessions::Options::new(build_dir);
            options.require_tools = require_tools();
            match parse_run(rest, &mut options) {
                Ok(true) => {}
                Ok(false) => {
                    print!("{USAGE}");
                    return 0;
                }
                Err(e) => return usage(e),
            }
            match sessions::run(&options) {
                Ok(report) => {
                    print!("{}", report.summary(options.verbose));
                    report.exit_status()
                }
                Err(e) => {
                    eprintln!("xtask debug: {e}");
                    3
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    // spec/harness.md "debug", "Options".
    #[test]
    fn run_options() {
        let mut options = sessions::Options::new(PathBuf::from("/b"));
        let args = strings(&[
            "-j2",
            "--linker",
            "mold",
            "--linker=bfd",
            "--debugger",
            "lldb",
            "--frontend",
            "kiln",
            "--tier",
            "quick",
            "--no-lines",
            "--keep",
            "-v",
            "--hod",
            "/h",
            "a.c",
            "--",
            "-odd.c",
        ]);
        assert!(parse_run(&args, &mut options).unwrap());
        assert_eq!(options.jobs, 2);
        assert_eq!(options.linkers, vec!["mold", "bfd"]);
        assert_eq!(options.debuggers, vec!["lldb"]);
        assert_eq!(options.frontends, vec!["kiln"]);
        assert!(options.quick && !options.lines && options.keep && options.verbose);
        assert_eq!(options.hod, Some(PathBuf::from("/h")));
        assert_eq!(options.paths, vec![PathBuf::from("a.c"), PathBuf::from("-odd.c")]);
        for bad in [&["--linker", "gold"][..], &["--tier", "x"], &["-j", "0"], &["--frob"]] {
            let mut options = sessions::Options::new(PathBuf::from("/b"));
            assert!(parse_run(&strings(bad), &mut options).is_err(), "{bad:?}");
        }
        let mut options = sessions::Options::new(PathBuf::from("/b"));
        assert!(!parse_run(&strings(&["--help"]), &mut options).unwrap());
    }
}
