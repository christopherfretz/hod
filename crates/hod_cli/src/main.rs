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
    with_process_io(|io| ExitCode::from(driver::run(&args, &env, &driver::compile_hod, io)))
}

/// Runs `f` with the process's standard streams, unlocked: each read or
/// write locks for itself (`spec/cli.md` "Threads", WP-334). A stream
/// locked for the whole run blocks a write from any other thread until
/// the run ends, and the run waits for its threads.
fn with_process_io<R>(f: impl FnOnce(&mut driver::Io<'_>) -> R) -> R {
    f(&mut driver::Io {
        stdin: &mut std::io::stdin(),
        stdout: &mut std::io::stdout(),
        stderr: &mut std::io::stderr(),
    })
}

#[cfg(test)]
mod tests {
    use std::io::Write;
    use std::sync::mpsc;
    use std::time::Duration;

    /// WP-334: while the driver runs, another thread (a code-generation
    /// worker of `--threads`) can still write to stdout and stderr. A
    /// stream locked for the whole run made such a write wait for the
    /// end of the run, and the run wait for the thread: a deadlock.
    #[test]
    fn other_threads_can_write_while_the_driver_runs() {
        super::with_process_io(|_io| {
            let (done, wrote) = mpsc::channel();
            let writer = std::thread::spawn(move || {
                // Not `eprint!`: the test harness captures it, without
                // touching the stream's lock.
                std::io::stderr().write_all(b"").unwrap();
                std::io::stdout().write_all(b"").unwrap();
                done.send(()).unwrap();
            });
            // Received at once when nothing holds the streams; the bound
            // only turns a deadlock into a failure.
            let result = wrote.recv_timeout(Duration::from_secs(60));
            assert!(result.is_ok(), "a write from another thread waited for the driver");
            writer.join().unwrap();
        });
    }
}
