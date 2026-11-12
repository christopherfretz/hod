//! Developer tasks. Contract: `spec/harness.md`.
//!
//! `cargo xtask ci [--quick]` is the gate described in `docs/VALIDATION.md`.
//! `cargo xtask jobserver ...` counts and refills the machine's token pool.
//! `cargo xtask shim ...` builds the LLVM directory that plugs hod into kiln.
//! `cargo xtask irdiff ...` is the IR differential harness (layer L4).
//! `cargo xtask corpus ...` captures, verifies and counts the IR corpus.
//! `cargo xtask roundtrip ...` parses and prints the corpus (layer L2).

use std::process::ExitCode;

const USAGE: &str = "\
usage: cargo xtask ci [--quick]
       cargo xtask jobserver <status|refill> [--fifo <path>] [--jobs <n>]
       cargo xtask shim --llvm <dir> --hod <hod binary> --out <dir> [--kiln <kiln++>]
       cargo xtask irdiff [options] [<file.ll or directory>...]   (--help for the options)
       cargo xtask corpus capture|verify|census|fetch [options]   (--help for the options)
       cargo xtask bench [options]                                (--help for the options)
       cargo xtask roundtrip [options]                            (--help for the options)
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("jobserver") => xtask::jobs::main(args[1..].iter().cloned()),
        Some("ci") => ci(args.iter().any(|a| a == "--quick")),
        Some("shim") => shim(&args[1..]),
        Some("irdiff") => irdiff(&args[1..]),
        Some("corpus") => corpus(&args[1..]),
        #[cfg(unix)]
        Some("roundtrip") => ExitCode::from(xtask::roundtrip::command(&args[1..])),
        #[cfg(unix)]
        Some("bench") => xtask::bench::main(&args[1..]),
        _ => {
            eprint!("{USAGE}");
            ExitCode::from(2)
        }
    }
}

/// Runs each step in order and stops at the first failure.
fn ci(quick: bool) -> ExitCode {
    let steps: &[(&str, &[&str])] = &[
        ("fmt", &["fmt", "--all", "--check"]),
        ("clippy", &["clippy", "--workspace", "--all-targets", "--", "-D", "warnings"]),
        ("test", &["test", "--workspace", "--quiet"]),
    ];
    for (name, cargo_args) in steps {
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
    eprintln!("ci: irdiff");
    if !irdiff_gate(quick) {
        eprintln!("ci: irdiff FAILED");
        return ExitCode::FAILURE;
    }
    // The full gate adds the cross-linking layer and the later ones as they
    // land (docs/VALIDATION.md "Gates").
    eprintln!("ci: ok");
    ExitCode::SUCCESS
}

/// The L4 step of the gate (spec/harness.md "irdiff", "In the gate"): the
/// quick tier against the preferred oracle, or every test against each
/// oracle LLVM.
#[cfg(unix)]
fn irdiff_gate(quick: bool) -> bool {
    use xtask::irdiff::{Config, TierFilter, run, tools::LLVM_MAJORS};
    let runs: Vec<(Option<String>, TierFilter)> = if quick {
        vec![(None, TierFilter::Quick)]
    } else {
        LLVM_MAJORS.iter().map(|major| (Some(major.to_string()), TierFilter::All)).collect()
    };
    let mut ok = true;
    for (llvm, tier) in runs {
        let mut config = Config::for_workspace();
        (config.llvm, config.tier) = (llvm, tier);
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
