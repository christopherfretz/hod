//! The `hod` command-line tool. Contract: `spec/cli.md` (WP-015).

use std::process::ExitCode;

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    if args.next().as_deref() == Some("--version") {
        println!("hod version {}", env!("CARGO_PKG_VERSION"));
        ExitCode::SUCCESS
    } else {
        eprintln!("hod: code generation is not implemented yet");
        ExitCode::from(2)
    }
}
