//! Developer tasks. Contract: `spec/harness.md`.
//!
//! `cargo xtask ci [--quick]` is the gate described in `docs/VALIDATION.md`.

use std::process::{Command, ExitCode};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("ci") {
        ci(args.iter().any(|a| a == "--quick"))
    } else {
        eprintln!("usage: cargo xtask ci [--quick]");
        ExitCode::from(2)
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
