//! One measured child process, and the statistics over several. Contract:
//! `spec/harness.md` "bench", "Measurement".
//!
//! The differential harness's bounded child (`irdiff::process::run`) polls
//! for the end of its child, which is right for a test and wrong for a
//! stopwatch. This is the variant for measurement: the same limits, set
//! the same way, with the clock read by a thread blocked in `wait`, and
//! the child's peak resident memory and CPU time taken from GNU `time`.
//! No `unsafe` and no dependency: the standard library cannot call
//! `wait4`, so the kernel's resource usage record of a child is only
//! reachable through a program that can.

use std::io::Read;
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::channel;
use std::time::{Duration, Instant};

use crate::irdiff::process::{End, Limits};

/// Where GNU time is installed.
pub const GNU_TIME: &str = "/usr/bin/time";

/// What GNU time is asked to write: the child's peak resident set size in
/// KiB, and its user and system CPU seconds (two decimals).
const TIME_FORMAT: &str = "%M %U %S";

/// The most that is kept of a measured child's standard error.
const STDERR_CAP: usize = 64 << 10;

/// How long a child's standard error is waited for after it ended.
const DRAIN_GRACE: Duration = Duration::from_secs(2);

/// The exit status of the wrapper when it cannot set a limit.
const SETUP_FAILED: i32 = 125;
const SETUP_MESSAGE: &str = "bench: cannot set the resource limits";

/// `$1`: the address-space limit in KiB. `$2`: the CPU limit in seconds.
/// The rest: the program and its arguments. The same wrapper as the
/// differential harness's ("irdiff", "Child processes").
const WRAPPER: &str = "ulimit -c 0 && ulimit -v \"$1\" && ulimit -t \"$2\" || \
                       { echo 'bench: cannot set the resource limits' >&2; exit 125; }\n\
                       shift 2\n\
                       exec \"$@\"";

/// How a child's resource usage is obtained.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Probe {
    /// GNU time, if it is usable; without it only wall time is measured.
    pub time: Option<PathBuf>,
}

impl Probe {
    /// Uses `time` if it accepts `-f <format> -o <file>` and writes three
    /// numbers for a child that does nothing. `scratch` is a writable
    /// directory.
    pub fn detect(time: &Path, scratch: &Path) -> Self {
        let candidate = Self { time: Some(time.to_path_buf()) };
        let limits = Limits { time: Duration::from_secs(20), memory_kib: 1 << 20 };
        let usable = time.is_file()
            && run_measured(
                Path::new("/bin/sh"),
                |c| {
                    c.args(["-c", ":"]);
                },
                &candidate,
                &scratch.join("probe.usage"),
                limits,
            )
            .is_ok_and(|sample| sample.end == End::Exit(0) && sample.rss_kib.is_some());
        if usable { candidate } else { Self { time: None } }
    }

    /// The `usage` header value of a results file.
    pub fn describe(&self) -> String {
        match &self.time {
            Some(time) => format!("gnu-time ({} -f '{TIME_FORMAT}')", time.display()),
            None => "none".to_owned(),
        }
    }
}

/// One run of a measured child.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sample {
    pub end: End,
    /// From just before the process was started to the return of `wait`.
    pub wall: Duration,
    /// User plus system CPU time, microseconds (GNU time reports
    /// hundredths of a second).
    pub cpu_us: Option<u64>,
    /// Peak resident set size, KiB.
    pub rss_kib: Option<u64>,
    /// The start of the child's standard error.
    pub stderr: Vec<u8>,
}

impl Sample {
    /// The first non-empty line of standard error, for a diagnostic.
    pub fn first_error_line(&self) -> String {
        let text = String::from_utf8_lossy(&self.stderr);
        text.lines().map(str::trim).find(|line| !line.is_empty()).unwrap_or("").to_owned()
    }
}

/// Runs `program` once for measurement, under `limits`. `configure` adds
/// the arguments, environment and working directory. Standard input and
/// output are null, standard error is captured. `usage_file` is a scratch
/// path for GNU time's report; it is removed.
///
/// An error is a failure of the harness or the machine, not of the
/// program: it is missing, cannot be started, or a limit cannot be set.
pub fn run_measured(
    program: &Path,
    configure: impl FnOnce(&mut Command),
    probe: &Probe,
    usage_file: &Path,
    limits: Limits,
) -> Result<Sample, String> {
    if !program.is_file() {
        return Err(format!("cannot run '{}': no such file", program.display()));
    }
    let mut command = Command::new("/bin/sh");
    command.arg("-c").arg(WRAPPER).arg("sh");
    command.arg(limits.memory_kib.to_string()).arg(limits.cpu_seconds().to_string());
    // The limits go outside GNU time, which then starts the program with
    // them inherited; its report is the program's alone.
    if let Some(time) = &probe.time {
        command.arg(time).args(["-f", TIME_FORMAT, "-o"]).arg(usage_file).arg("--");
    }
    command.arg(program);
    configure(&mut command);
    // A measured child is one process: it must not find a jobserver and
    // decide to use more cores than its share.
    command.env_remove("MAKEFLAGS").env_remove("CARGO_MAKEFLAGS");
    command.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped());
    // Its own process group, so that the time limit reaches the wrapper,
    // GNU time and the program together.
    command.process_group(0);
    let _ = std::fs::remove_file(usage_file);

    // The jobserver client of WP-024 takes this child's token here, before
    // the clock starts: `let _token = crate::jobs::global().acquire();`.
    let start = Instant::now();
    let mut child =
        command.spawn().map_err(|e| format!("cannot run '{}': {e}", program.display()))?;
    let group = child.id();
    let (errors_done, errors) = channel();
    if let Some(mut pipe) = child.stderr.take() {
        std::thread::spawn(move || {
            let mut kept = Vec::new();
            let mut chunk = vec![0; 1 << 14];
            while let Ok(n) = pipe.read(&mut chunk) {
                if n == 0 {
                    break;
                }
                let room = STDERR_CAP - kept.len();
                kept.extend_from_slice(&chunk[..n.min(room)]);
            }
            let _ = errors_done.send(kept);
        });
    }
    // A thread blocked in `wait` reads the clock the moment the child is
    // gone; polling would add its interval to every measurement.
    let (ended, end) = channel();
    std::thread::spawn(move || {
        let status = child.wait();
        let _ = ended.send((status, Instant::now()));
    });
    let (status, finished) = if let Ok((status, at)) = end.recv_timeout(limits.time) {
        (Some(status), at)
    } else {
        // The leader is not reaped until the waiting thread returns, so
        // its group id is still its own.
        kill_group(group);
        let killed = end.recv_timeout(Duration::from_secs(30));
        (None, killed.map_or_else(|_| Instant::now(), |(_, at)| at))
    };
    let stderr = errors.recv_timeout(DRAIN_GRACE).unwrap_or_else(|_| {
        // Something the child started still holds the pipe.
        kill_group(group);
        Vec::new()
    });
    let usage = std::fs::read_to_string(usage_file).unwrap_or_default();
    let _ = std::fs::remove_file(usage_file);
    let wall = finished.duration_since(start);

    let Some(status) = status else {
        return Ok(Sample { end: End::TimedOut, wall, cpu_us: None, rss_kib: None, stderr });
    };
    let status = status.map_err(|e| format!("cannot wait for '{}': {e}", program.display()))?;
    let mut end = match (status.code(), status.signal()) {
        (Some(code), _) => End::Exit(code),
        (None, Some(signal)) => End::Signal(signal),
        (None, None) => return Err(format!("'{}' ended without a status", program.display())),
    };
    if end == End::Exit(SETUP_FAILED) && stderr.starts_with(SETUP_MESSAGE.as_bytes()) {
        return Err(format!(
            "cannot limit '{}' to {} KiB of address space and {} s of CPU time",
            program.display(),
            limits.memory_kib,
            limits.cpu_seconds()
        ));
    }
    let report = parse_usage(&usage);
    // GNU time exits with the signal's number when the program was
    // terminated by one, and says so in its report.
    if let Some(signal) = report.signal {
        end = End::Signal(signal);
    }
    Ok(Sample { end, wall, cpu_us: report.cpu_us, rss_kib: report.rss_kib, stderr })
}

/// What GNU time's report says.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Usage {
    rss_kib: Option<u64>,
    cpu_us: Option<u64>,
    signal: Option<i32>,
}

/// Reads GNU time's report: an optional line `Command exited with
/// non-zero status N` or `Command terminated by signal N`, then the
/// formatted line, `<KiB> <user seconds> <system seconds>`.
fn parse_usage(text: &str) -> Usage {
    let mut usage = Usage::default();
    for line in text.lines() {
        if let Some(signal) = line.strip_prefix("Command terminated by signal ") {
            usage.signal = signal.trim().parse().ok();
        }
    }
    let last = text.lines().rev().find(|line| !line.trim().is_empty()).unwrap_or("");
    let fields: Vec<&str> = last.split_whitespace().collect();
    if let [rss, user, system] = fields[..] {
        usage.rss_kib = rss.parse().ok();
        usage.cpu_us = seconds_to_us(user).zip(seconds_to_us(system)).map(|(u, s)| u + s);
    }
    usage
}

/// `12.34` seconds as microseconds; at most six decimals.
fn seconds_to_us(text: &str) -> Option<u64> {
    let (whole, fraction) = text.split_once('.').unwrap_or((text, ""));
    let digits = |s: &str| s.bytes().all(|b| b.is_ascii_digit());
    if whole.is_empty() || !digits(whole) || !digits(fraction) || fraction.len() > 6 {
        return None;
    }
    let scale = 10u64.pow(6 - fraction.len() as u32);
    let fraction = if fraction.is_empty() { 0 } else { fraction.parse::<u64>().ok()? * scale };
    whole.parse::<u64>().ok()?.checked_mul(1_000_000)?.checked_add(fraction)
}

/// Sends `SIGKILL` to every process of the group `pgid`, through the
/// shell's `kill` (the standard library signals one process only).
fn kill_group(pgid: u32) {
    let _ = Command::new("/bin/sh")
        .args(["-c", "kill -KILL \"-$1\"", "sh"])
        .arg(pgid.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

/// The numbers of several runs of one tool on one input.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Samples {
    /// Wall-clock microseconds of every run, in run order.
    pub wall_us: Vec<u64>,
    /// CPU microseconds of the runs that reported them.
    pub cpu_us: Vec<u64>,
    /// Peak resident KiB of the runs that reported it.
    pub rss_kib: Vec<u64>,
}

impl Samples {
    pub fn push(&mut self, sample: &Sample) {
        self.wall_us.push(u64::try_from(sample.wall.as_micros()).unwrap_or(u64::MAX));
        self.cpu_us.extend(sample.cpu_us);
        self.rss_kib.extend(sample.rss_kib);
    }

    pub fn runs(&self) -> usize {
        self.wall_us.len()
    }

    /// The index of the fastest run; the first of equals.
    pub fn fastest(&self) -> Option<usize> {
        let min = self.wall_us.iter().min()?;
        self.wall_us.iter().position(|us| us == min)
    }
}

/// The smallest, the middle and the largest of `values`. The middle of an
/// even count is the mean of the two middle values, rounded down.
pub fn min_median_max(values: &[u64]) -> Option<(u64, u64, u64)> {
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    let (first, last) = (*sorted.first()?, *sorted.last()?);
    let middle = sorted.len() / 2;
    let median = if sorted.len() % 2 == 1 {
        sorted[middle]
    } else {
        sorted[middle - 1] + (sorted[middle] - sorted[middle - 1]) / 2
    };
    Some((first, median, last))
}

/// What the method costs and how far it can be trusted, measured at the
/// start of a run.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Calibration {
    /// The fastest wall time, microseconds, of a child that does nothing:
    /// what every measured time includes.
    pub floor_us: Option<u64>,
    /// The peak memory measured for a child that allocates and touches
    /// [`KNOWN_ALLOCATION_KIB`] more than another, minus that amount, KiB.
    pub rss_error_kib: Option<i64>,
}

/// The allocation of the memory probe's self-check: 64 MiB.
pub const KNOWN_ALLOCATION_KIB: u64 = 64 << 10;

/// How many times the do-nothing child is run for the floor.
const FLOOR_RUNS: usize = 5;

/// Measures the method itself: the floor, with `/bin/true`, and the error
/// of the memory figure, with `dd` reading one block of
/// [`KNOWN_ALLOCATION_KIB`] against `dd` reading one block of 4 KiB.
pub fn calibrate(probe: &Probe, scratch: &Path) -> Calibration {
    let limits = Limits { time: Duration::from_secs(20), memory_kib: 1 << 20 };
    let usage = scratch.join("calibrate.usage");
    let quiet = |program: &str, args: &[&str]| {
        let args: Vec<String> = args.iter().map(|&arg| arg.to_owned()).collect();
        run_measured(
            Path::new(program),
            |c| {
                c.args(&args);
            },
            probe,
            &usage,
            limits,
        )
        .ok()
        .filter(|sample| sample.end == End::Exit(0))
    };
    let floor_us = (0..FLOOR_RUNS)
        .filter_map(|_| quiet("/bin/true", &[]))
        .map(|sample| u64::try_from(sample.wall.as_micros()).unwrap_or(u64::MAX))
        .min();
    let dd = |block_kib: u64| {
        let block = format!("bs={}", block_kib * 1024);
        ["/usr/bin/dd", "/bin/dd"]
            .into_iter()
            .find(|dd| Path::new(dd).is_file())
            .and_then(|dd| quiet(dd, &["if=/dev/zero", "of=/dev/null", "count=1", &block]))
            .and_then(|sample| sample.rss_kib)
    };
    let rss_error_kib = dd(4).zip(dd(4 + KNOWN_ALLOCATION_KIB)).map(|(small, large)| {
        let grown = i64::try_from(large).unwrap_or(i64::MAX) - i64::try_from(small).unwrap_or(0);
        grown - i64::try_from(KNOWN_ALLOCATION_KIB).unwrap_or(0)
    });
    Calibration { floor_us, rss_error_kib }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hod_testutil::Scratch;

    const ROOMY: Limits = Limits { time: Duration::from_secs(20), memory_kib: 1 << 20 };

    /// A stand-in for GNU time: runs the command, then writes what the
    /// environment says to the `-o` file, as GNU time would.
    pub(crate) const FAKE_TIME: &str = r#"PATH=/usr/bin:/bin
out=
while [ $# -gt 0 ]; do
  case "$1" in
    -f) shift ;;
    -o) out=$2; shift ;;
    --) shift; break ;;
  esac
  shift
done
"$@"
status=$?
if [ $status -gt 128 ]; then
  echo "Command terminated by signal $((status - 128))" > "$out"; status=$((status - 128))
elif [ $status -ne 0 ]; then
  echo "Command exited with non-zero status $status" > "$out"
else
  : > "$out"
fi
echo "${FAKE_RSS:-1234} ${FAKE_USER:-0.25} ${FAKE_SYS:-0.05}" >> "$out"
exit $status"#;

    fn sh(script: &str, probe: &Probe, scratch: &Scratch, limits: Limits) -> Sample {
        run_measured(
            Path::new("/bin/sh"),
            |c| {
                c.arg("-c").arg(script);
            },
            probe,
            &scratch.join("usage"),
            limits,
        )
        .unwrap()
    }

    // spec/harness.md "bench", "Measurement": GNU time's report is the
    // peak memory and the CPU seconds, after an optional status line.
    #[test]
    fn usage_report_parsing() {
        let usage = parse_usage("67128 0.41 0.07\n");
        assert_eq!(usage, Usage { rss_kib: Some(67128), cpu_us: Some(480_000), signal: None });
        let failed = parse_usage("Command exited with non-zero status 3\n700 0.00 0.00\n");
        assert_eq!(failed, Usage { rss_kib: Some(700), cpu_us: Some(0), signal: None });
        let killed = parse_usage("Command terminated by signal 11\n700 1.50 2\n\n");
        assert_eq!(killed, Usage { rss_kib: Some(700), cpu_us: Some(3_500_000), signal: Some(11) });
        assert_eq!(parse_usage(""), Usage::default());
        assert_eq!(parse_usage("cannot run x\n"), Usage::default());
        assert_eq!(parse_usage("12 0.1\n"), Usage::default());
        assert_eq!(parse_usage("12 a 0.1\n").cpu_us, None);
        assert_eq!(seconds_to_us("0.01"), Some(10_000));
        assert_eq!(seconds_to_us("12"), Some(12_000_000));
        assert_eq!(seconds_to_us("1.000001"), Some(1_000_001));
        for bad in ["", ".5", "1.0000001", "-1", "1e3"] {
            assert_eq!(seconds_to_us(bad), None, "{bad}");
        }
    }

    // "Measurement": fastest of N; the middle run; an even count.
    #[test]
    fn fastest_of_n_selection() {
        assert_eq!(min_median_max(&[]), None);
        assert_eq!(min_median_max(&[7]), Some((7, 7, 7)));
        assert_eq!(min_median_max(&[30, 10, 20]), Some((10, 20, 30)));
        assert_eq!(min_median_max(&[40, 10, 21, 30]), Some((10, 25, 40)));
        assert_eq!(
            min_median_max(&[u64::MAX, u64::MAX - 1]),
            Some((u64::MAX - 1, u64::MAX - 1, u64::MAX))
        );

        let mut samples = Samples::default();
        for (wall, cpu, rss) in
            [(900, Some(10), Some(5)), (700, None, Some(7)), (700, Some(30), None)]
        {
            samples.push(&Sample {
                end: End::Exit(0),
                wall: Duration::from_micros(wall),
                cpu_us: cpu,
                rss_kib: rss,
                stderr: Vec::new(),
            });
        }
        assert_eq!(samples.runs(), 3);
        assert_eq!(samples.wall_us, [900, 700, 700]);
        assert_eq!(
            (samples.cpu_us.as_slice(), samples.rss_kib.as_slice()),
            (&[10, 30][..], &[5, 7][..])
        );
        // The fastest run is the first of the equally fast ones.
        assert_eq!(samples.fastest(), Some(1));
        assert_eq!(Samples::default().fastest(), None);
    }

    // "Measurement": wall time, the exit status, standard error, and the
    // usage figures of the probe; the child's environment and directory.
    #[test]
    fn a_measured_run() {
        let scratch = Scratch::new("bench-measure");
        let probe = Probe { time: Some(scratch.write_script("time", FAKE_TIME)) };
        let sample = sh("sleep 0.2; echo diagnostic >&2; exit 3", &probe, &scratch, ROOMY);
        assert_eq!(sample.end, End::Exit(3));
        assert!(
            sample.wall >= Duration::from_millis(200) && sample.wall < Duration::from_secs(10),
            "{sample:?}"
        );
        assert_eq!((sample.rss_kib, sample.cpu_us), (Some(1234), Some(300_000)));
        assert_eq!(sample.first_error_line(), "diagnostic");
        assert!(!scratch.join("usage").exists(), "the usage file is removed");

        let sample = run_measured(
            Path::new("/bin/sh"),
            |c| {
                c.args(["-c", "echo \"$V ${MAKEFLAGS:-none} ${CARGO_MAKEFLAGS:-none}\" >&2; pwd >&2; ulimit -v >&2"]);
                c.env("V", "value").env("MAKEFLAGS", "--jobserver-auth=fifo:/x").env("CARGO_MAKEFLAGS", "-j");
                c.env("FAKE_RSS", "99").current_dir(scratch.path());
            },
            &probe,
            &scratch.join("usage"),
            Limits { memory_kib: 654_321, ..ROOMY },
        )
        .unwrap();
        let dir = scratch.path().canonicalize().unwrap();
        // The jobserver is not handed to a measured child; the limits are.
        assert_eq!(
            String::from_utf8_lossy(&sample.stderr),
            format!("value none none\n{}\n654321\n", dir.display())
        );
        assert_eq!(sample.rss_kib, Some(99));

        // Without a probe: wall time only.
        let bare = sh("exit 0", &Probe { time: None }, &scratch, ROOMY);
        assert_eq!((bare.end, bare.rss_kib, bare.cpu_us), (End::Exit(0), None, None));
    }

    // "Measurement": a signal is told from an exit status, also through
    // the probe; a child at the time limit is killed with its group and
    // has no numbers.
    #[test]
    fn signals_and_the_time_limit() {
        let scratch = Scratch::new("bench-measure-end");
        let probe = Probe { time: Some(scratch.write_script("time", FAKE_TIME)) };
        assert_eq!(sh("kill -SEGV $$", &probe, &scratch, ROOMY).end, End::Signal(11));
        assert_eq!(sh("exit 11", &probe, &scratch, ROOMY).end, End::Exit(11));
        assert_eq!(
            sh("kill -SEGV $$", &Probe { time: None }, &scratch, ROOMY).end,
            End::Signal(11)
        );

        let marker = scratch.join("survived");
        let script = format!("(sleep 2; touch '{}') & sleep 60", marker.display());
        let started = Instant::now();
        let sample =
            sh(&script, &probe, &scratch, Limits { time: Duration::from_millis(300), ..ROOMY });
        assert_eq!((sample.end, sample.rss_kib, sample.cpu_us), (End::TimedOut, None, None));
        assert!(started.elapsed() < Duration::from_secs(10), "{:?}", started.elapsed());
        std::thread::sleep(Duration::from_millis(2200).saturating_sub(started.elapsed()));
        assert!(!marker.exists(), "a grandchild outlived the time limit");
    }

    // "Measurement": what is the harness's failure, not the child's; and
    // the probe is used only if it works.
    #[test]
    fn harness_errors_and_probe_detection() {
        let scratch = Scratch::new("bench-measure-errors");
        let none = Probe { time: None };
        let missing =
            run_measured(Path::new("/nonexistent/tool"), |_| {}, &none, &scratch.join("u"), ROOMY);
        assert!(missing.unwrap_err().contains("cannot run '/nonexistent/tool'"));

        let good = scratch.write_script("time", FAKE_TIME);
        assert_eq!(Probe::detect(&good, scratch.path()), Probe { time: Some(good.clone()) });
        assert!(Probe::detect(&good, scratch.path()).describe().starts_with("gnu-time ("));
        let silent = scratch.write_script(
            "silent-time",
            "while [ \"$1\" != -- ]; do shift; done; shift; exec \"$@\"",
        );
        assert_eq!(Probe::detect(&silent, scratch.path()), none);
        assert_eq!(Probe::detect(&scratch.join("absent"), scratch.path()), none);
        assert_eq!(none.describe(), "none");
    }

    // "Measurement", "The method's error": with the real GNU time, a child
    // that allocates and touches 64 MiB more than another is measured
    // within 1% of that; and the floor is measured.
    #[test]
    fn real_probe_measures_a_known_allocation() {
        let scratch = Scratch::new("bench-measure-real");
        let probe = Probe::detect(Path::new(GNU_TIME), scratch.path());
        if probe.time.is_none() {
            assert!(
                !hod_testutil::tools_required(),
                "GNU time is required ({GNU_TIME}; HOD_REQUIRE_TOOLS=1)"
            );
            eprintln!("note: {GNU_TIME} is not GNU time; skipping the memory probe's self-check");
            return;
        }
        let calibration = calibrate(&probe, scratch.path());
        let error = calibration.rss_error_kib.expect("dd is part of every system");
        let bound = i64::try_from(KNOWN_ALLOCATION_KIB / 100).unwrap();
        assert!(
            error.abs() <= bound,
            "measured {error} KiB off a 64 MiB allocation; stated error {bound} KiB"
        );
        let floor = calibration.floor_us.expect("/bin/true runs");
        assert!(floor > 0 && floor < 5_000_000, "{floor}");

        // CPU time is the child's: a busy loop of a quarter second or so.
        let busy = sh("i=0; while [ $i -lt 200000 ]; do i=$((i+1)); done", &probe, &scratch, ROOMY);
        assert_eq!(busy.end, End::Exit(0));
        let (cpu, wall) = (busy.cpu_us.unwrap(), u64::try_from(busy.wall.as_micros()).unwrap());
        assert!(cpu >= 10_000 && cpu <= wall + 20_000, "cpu {cpu} us, wall {wall} us");
    }
}
