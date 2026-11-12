//! The `hod` command-line tool: `llc`'s command line in front of hod's code
//! generator, with a fallback to the real `llc` and a coverage log.
//! Contract: `spec/cli.md`.

mod args;
mod convert;
mod coverage;
mod driver;
mod jobs;
mod phases;

use std::ffi::OsString;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let env = driver::Environment::from_vars(|name| std::env::var_os(name));
    let mut io = driver::Io {
        stdin: &mut std::io::stdin().lock(),
        stdout: &mut std::io::stdout().lock(),
        stderr: &mut std::io::stderr().lock(),
    };
    ExitCode::from(driver::run(&args, &env, &hod::compile_with, &mut io))
}
