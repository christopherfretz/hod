//! The `hod` command-line tool: `llc`'s command line in front of hod's code
//! generator, with a fallback to the real `llc` and a coverage log.
//! Contract: `spec/cli.md`.

mod convert;
mod driver;
mod phases;

use std::ffi::OsString;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let env = hod::Environment::from_process();
    let mut io = driver::Io {
        stdin: &mut std::io::stdin().lock(),
        stdout: &mut std::io::stdout().lock(),
        stderr: &mut std::io::stderr().lock(),
    };
    ExitCode::from(driver::run(&args, &env, &hod::compile_releasing, &mut io))
}
