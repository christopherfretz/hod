//! Developer tasks. Contract: `spec/harness.md`.
//!
//! `cargo xtask ci [--quick] [--step <name>]...` is the gate described in `docs/VALIDATION.md`.
//! `cargo xtask jobserver ...` counts and refills the machine's token pool.
//! `cargo xtask shim ...` builds the LLVM directory that plugs hod into kiln.
//! `cargo xtask irdiff ...` is the IR differential harness (layer L4).
//! `cargo xtask corpus ...` captures, verifies and counts the IR corpus.
//! `cargo xtask roundtrip ...` parses and prints the corpus (layer L2).
//! `cargo xtask debug ...` runs the debuggers on hod's programs (layer L7).
//! `cargo xtask x64census ...` checks the front half on x86-64 modules.

use std::process::ExitCode;

const USAGE: &str = "\
usage: cargo xtask ci [--quick] [--step fmt|clippy|test|irdiff|roundtrip|debug]...
       cargo xtask jobserver <status|refill> [--fifo <path>] [--jobs <n>]
       cargo xtask shim --llvm <dir> --hod <hod binary> --out <dir> [--kiln <kiln++>]
       cargo xtask irdiff [options] [<file.ll or directory>...]   (--help for the options)
       cargo xtask corpus capture|verify|census|fetch [options]   (--help for the options)
       cargo xtask bench [options]                                (--help for the options)
       cargo xtask roundtrip [options]                            (--help for the options)
       cargo xtask debug [run|kiln|verify] [options]              (--help for the options)
       cargo xtask x64census [options] <dir or file.ll>...        (--help for the options)
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("jobserver") => xtask::jobs::main(args[1..].iter().cloned()),
        Some("ci") => match xtask::ci::Args::parse(&args[1..]) {
            Ok(ci_args) => ci(&ci_args),
            Err(message) => {
                eprint!("xtask ci: {message}\n{USAGE}");
                ExitCode::from(2)
            }
        },
        Some("shim") => shim(&args[1..]),
        Some("irdiff") => irdiff(&args[1..]),
        Some("corpus") => corpus(&args[1..]),
        #[cfg(unix)]
        Some("roundtrip") => ExitCode::from(xtask::roundtrip::command(&args[1..])),
        #[cfg(unix)]
        Some("bench") => xtask::bench::main(&args[1..]),
        #[cfg(unix)]
        Some("debug") => ExitCode::from(xtask::debug::command(&args[1..])),
        #[cfg(unix)]
        Some("x64census") => ExitCode::from(xtask::x64census::command(&args[1..])),
        _ => {
            eprint!("{USAGE}");
            ExitCode::from(2)
        }
    }
}

/// Runs each selected step in order and stops at the first failure
/// (`spec/harness.md` "ci").
fn ci(args: &xtask::ci::Args) -> ExitCode {
    let quick = args.quick;
    let steps: &[(&str, &[&str])] = &[
        ("fmt", &["fmt", "--all", "--check"]),
        ("clippy", &["clippy", "--workspace", "--all-targets", "--", "-D", "warnings"]),
        ("test", &["test", "--workspace", "--quiet"]),
    ];
    for (name, cargo_args) in steps {
        if !args.runs(name) {
            continue;
        }
        eprintln!("ci: {name}");
        // A cargo step is a jobserver client of its own (spec/harness.md
        // "Jobserver"), and is run once more if an artifact vanished under
        // it ("ci").
        let ok = xtask::ci::cargo_step(cargo_args);
        if !ok {
            eprintln!("ci: {name} FAILED");
            return ExitCode::FAILURE;
        }
    }
    if args.runs("irdiff") {
        eprintln!("ci: irdiff");
        if !irdiff_gate(quick) {
            eprintln!("ci: irdiff FAILED");
            return ExitCode::FAILURE;
        }
    }
    if args.runs("roundtrip") {
        eprintln!("ci: roundtrip");
        if !roundtrip_gate(quick) {
            eprintln!("ci: roundtrip FAILED");
            return ExitCode::FAILURE;
        }
    }
    // L7: the full gate runs every debugger test (the quick tests ran in
    // `cargo test`).
    #[cfg(unix)]
    if !quick && args.runs("debug") && !xtask::debug::gate() {
        eprintln!("ci: debug FAILED");
        return ExitCode::FAILURE;
    }
    // The full gate adds the cross-linking layer and the later ones as they
    // land (docs/VALIDATION.md "Gates").
    eprintln!("ci: ok");
    ExitCode::SUCCESS
}

/// The L4 step of the gate (spec/harness.md "irdiff", "In the gate"): the
/// runs `xtask::ci::irdiff_runs` plans for the tier, the other
/// architecture's under its emulator among them.
#[cfg(unix)]
fn irdiff_gate(quick: bool) -> bool {
    use xtask::irdiff::{Arch, Config, run, workspace_root};
    let mut ok = true;
    for planned in xtask::ci::irdiff_runs(quick, Arch::host()) {
        eprintln!("ci: {}", xtask::ci::irdiff_command(&planned));
        let mut config = Config::for_workspace();
        config.llvm = planned.llvm.map(|major| major.to_string());
        (config.target, config.tier, config.mode) = (planned.target, planned.tier, planned.mode);
        config.witness = planned.witness;
        config.paths = planned.paths.iter().map(|path| workspace_root().join(path)).collect();
        match run(&config) {
            Ok(report) => {
                eprint!("{}", report.summary(false));
                ok &= report.exit_status() == 0;
            }
            Err(message) => {
                eprintln!("xtask irdiff: {message}");
                ok = false;
            }
        }
    }
    ok
}

#[cfg(not(unix))]
fn irdiff_gate(_: bool) -> bool {
    eprintln!("ci: irdiff is only available on Unix hosts; skipped");
    true
}

/// The L2 step of the gate (spec/harness.md "roundtrip", "In the gate"):
/// the quick tier of the corpus, or all of it.
#[cfg(unix)]
fn roundtrip_gate(quick: bool) -> bool {
    let mut options = xtask::roundtrip::Options::for_workspace();
    options.quick = quick;
    match xtask::roundtrip::gate(&options) {
        Ok(text) => {
            eprint!("{text}");
            true
        }
        Err(text) => {
            eprint!("{text}");
            false
        }
    }
}

#[cfg(not(unix))]
fn roundtrip_gate(_: bool) -> bool {
    eprintln!("ci: roundtrip is only available on Unix hosts; skipped");
    true
}

/// `cargo xtask irdiff` (spec/harness.md "irdiff").
#[cfg(unix)]
fn irdiff(args: &[String]) -> ExitCode {
    use xtask::irdiff::{Config, USAGE, parse_args, run};
    let mut config = Config::for_workspace();
    match parse_args(args, &mut config) {
        Ok(true) => {}
        Ok(false) => {
            print!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Err(message) => {
            eprint!("xtask irdiff: {message}\n{USAGE}");
            return ExitCode::from(2);
        }
    }
    match run(&config) {
        Ok(report) => {
            print!("{}", report.summary(config.verbose));
            ExitCode::from(report.exit_status())
        }
        Err(message) => {
            eprintln!("xtask irdiff: {message}");
            ExitCode::from(2)
        }
    }
}

#[cfg(not(unix))]
fn irdiff(_: &[String]) -> ExitCode {
    eprintln!("xtask irdiff: only available on Unix hosts");
    ExitCode::FAILURE
}

/// `cargo xtask corpus` (spec/harness.md "corpus").
#[cfg(unix)]
fn corpus(args: &[String]) -> ExitCode {
    ExitCode::from(xtask::corpus::command(args))
}

#[cfg(not(unix))]
fn corpus(_: &[String]) -> ExitCode {
    eprintln!("xtask corpus: only available on Unix hosts");
    ExitCode::FAILURE
}

/// `cargo xtask shim` (spec/harness.md "shim").
#[cfg(unix)]
fn shim(args: &[String]) -> ExitCode {
    use xtask::shim::{USAGE, build, parse_args, summary};
    if args.iter().any(|a| a == "--help" || a == "-h") {
        print!("{USAGE}");
        return ExitCode::SUCCESS;
    }
    let request = match parse_args(args) {
        Ok(request) => request,
        Err(message) => {
            eprint!("xtask shim: {message}\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    match build(&request) {
        Ok(shim) => {
            print!("{}", summary(&shim));
            ExitCode::SUCCESS
        }
        Err(message) => {
            eprintln!("xtask shim: {message}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(not(unix))]
fn shim(_: &[String]) -> ExitCode {
    eprintln!("xtask shim: only available on Unix hosts");
    ExitCode::FAILURE
}
