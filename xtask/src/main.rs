//! Developer tasks. Contract: `spec/harness.md`.
//!
//! `cargo xtask ci [--quick]` is the gate described in `docs/VALIDATION.md`.
//! `cargo xtask shim ...` builds the LLVM directory that plugs hod into kiln.

use std::process::{Command, ExitCode};

const USAGE: &str = "\
usage: cargo xtask ci [--quick]
       cargo xtask shim --llvm <dir> --hod <hod binary> --out <dir> [--kiln <kiln++>]
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("ci") => ci(args.iter().any(|a| a == "--quick")),
        Some("shim") => shim(&args[1..]),
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
        let ok = Command::new(env!("CARGO"))
            .args(*cargo_args)
            .status()
            .is_ok_and(|status| status.success());
        if !ok {
            eprintln!("ci: {name} FAILED");
            return ExitCode::FAILURE;
        }
    }
    // The full gate adds the differential and cross-linking layers as they
    // land (docs/VALIDATION.md "Gates"); today it equals the quick gate.
    let _ = quick;
    eprintln!("ci: ok");
    ExitCode::SUCCESS
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
