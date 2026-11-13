//! hod's environment: every `HOD_*` variable and `MAKEFLAGS`, read in this
//! one place (`spec/cli.md` "Environment", "hod's own options", "Threads").
//!
//! The `hod` binary reads it once per invocation ([`Environment::from_process`])
//! and puts its command line over it; a program that compiles in process
//! reads it once per process ([`Settings::from_env`], or
//! [`Environment::from_process`] for the rest: strict mode, the coverage
//! log), so that both routes agree on the settings by construction
//! (`docs/design/0004-in-process-seam.md` §2.5).

use std::ffi::OsString;
use std::fmt;
use std::path::PathBuf;

use crate::args::compression;
use crate::{Compression, Settings, Threads, jobserver};

/// The real `llc` (`--fallback=`).
pub const ENV_FALLBACK: &str = "HOD_FALLBACK_LLC";
/// `1` is `--strict`.
pub const ENV_STRICT: &str = "HOD_STRICT";
/// The coverage log (`--coverage-log=`).
pub const ENV_COVERAGE_LOG: &str = "HOD_COVERAGE_LOG";
/// The `llvm-dis` for bitcode (`--bitcode-via=`).
pub const ENV_BITCODE_VIA: &str = "HOD_BITCODE_VIA";
/// `--compress-debug-sections=` (WP-061).
pub const ENV_COMPRESS_DEBUG_SECTIONS: &str = "HOD_COMPRESS_DEBUG_SECTIONS";
/// Set by hod for the processes it starts, the fallback and the bitcode
/// converter. A hod that starts with it set stands where one of them
/// should be, and stops instead of recursing.
pub const ENV_IN_FALLBACK: &str = "HOD_IN_FALLBACK";
/// The variables that are settings. The bitcode converter is started
/// without them.
pub const SETTINGS: [&str; 4] = [ENV_FALLBACK, ENV_STRICT, ENV_COVERAGE_LOG, ENV_BITCODE_VIA];
/// The file the phase report is appended to.
pub const ENV_TIME_PHASES: &str = "HOD_TIME_PHASES";
/// `1` asks for every reason of a declined module (coverage field 11), not
/// only the first.
pub const ENV_CENSUS: &str = "HOD_CENSUS";
/// The most threads code generation uses (`spec/cli.md` "Threads"), a
/// positive integer; `--threads=` wins over it; without either, extra
/// threads come only from a jobserver.
pub const ENV_THREADS: &str = "HOD_THREADS";
/// The jobserver's variable (`spec/cli.md` "Threads").
pub const ENV_MAKEFLAGS: &str = "MAKEFLAGS";
/// The value of `--bitcode-via` that names hod's own bitcode reader.
pub const BITCODE_NATIVE: &str = "native";

/// hod's settings from the environment. A variable set but empty is unset.
///
/// `#[non_exhaustive]`: a variable hod comes to read is a field more, and
/// no caller's code breaks for it (`spec/cli.md` "Embedding"). A caller
/// reads the fields and may set them; it makes one with
/// [`Environment::from_process`], [`Environment::from_vars`] or
/// [`Environment::default`] (no variable set), not with a literal.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct Environment {
    /// `HOD_FALLBACK_LLC`.
    pub fallback: Option<PathBuf>,
    /// `HOD_STRICT=1`.
    pub strict: bool,
    /// `HOD_COVERAGE_LOG`.
    pub coverage_log: Option<PathBuf>,
    /// `HOD_BITCODE_VIA`.
    pub bitcode_via: Option<PathBuf>,
    /// `HOD_COMPRESS_DEBUG_SECTIONS`: what the shim puts in
    /// [`crate::Options::debug_compression`] when the command line does
    /// not say.
    pub compress_debug_sections: Option<Compression>,
    /// `HOD_IN_FALLBACK` is set: this process was started by another hod,
    /// as its fallback or its bitcode converter.
    pub in_fallback: bool,
    /// `HOD_TIME_PHASES`.
    pub time_phases: Option<PathBuf>,
    /// `HOD_CENSUS=1`.
    pub census: bool,
    /// `HOD_THREADS`.
    pub threads: Option<usize>,
    /// `MAKEFLAGS`, for a jobserver (`spec/cli.md` "Threads"), if UTF-8.
    pub makeflags: Option<String>,
    /// The first variable with a value hod does not accept, as the
    /// diagnostic says it (reason `usage`). The other fields hold what
    /// could be read.
    pub error: Option<String>,
}

impl Environment {
    /// Reads the variables of this process.
    pub fn from_process() -> Self {
        Self::from_vars(|name| std::env::var_os(name))
    }

    /// Reads the variables through `var`, each once.
    pub fn from_vars(var: impl Fn(&str) -> Option<OsString>) -> Self {
        let non_empty = |name: &str| var(name).filter(|value| !value.is_empty());
        let mut env = Environment {
            fallback: non_empty(ENV_FALLBACK).map(PathBuf::from),
            coverage_log: non_empty(ENV_COVERAGE_LOG).map(PathBuf::from),
            bitcode_via: non_empty(ENV_BITCODE_VIA).map(PathBuf::from),
            in_fallback: non_empty(ENV_IN_FALLBACK).is_some(),
            time_phases: non_empty(ENV_TIME_PHASES).map(PathBuf::from),
            makeflags: non_empty(ENV_MAKEFLAGS).and_then(|flags| flags.into_string().ok()),
            ..Environment::default()
        };
        if let Some(value) = non_empty(ENV_COMPRESS_DEBUG_SECTIONS) {
            match compression(&value) {
                Ok(c) => env.compress_debug_sections = Some(c),
                Err(message) => {
                    env.error = Some(format!("{ENV_COMPRESS_DEBUG_SECTIONS}: {message}"));
                }
            }
        }
        if let Some(value) = non_empty(ENV_THREADS) {
            match value.to_str().and_then(|v| v.parse::<usize>().ok()).filter(|&n| n > 0) {
                Some(threads) => env.threads = Some(threads),
                None => {
                    env.error = Some(format!(
                        "invalid value '{}' for {ENV_THREADS}: expected a positive integer",
                        value.to_string_lossy()
                    ));
                }
            }
        }
        for (name, flag) in [(ENV_STRICT, &mut env.strict), (ENV_CENSUS, &mut env.census)] {
            match non_empty(name) {
                None => {}
                Some(value) if value == "1" => *flag = true,
                Some(value) if value == "0" => {}
                Some(value) => {
                    env.error.get_or_insert(format!(
                        "invalid value '{}' for {name}: expected 1 or 0",
                        value.to_string_lossy()
                    ));
                }
            }
        }
        env
    }

    /// The threads of one compile (`spec/cli.md` "Threads"): `explicit`
    /// (the caller's own `--threads=`) over `HOD_THREADS`, granted
    /// outright; else a jobserver of `MAKEFLAGS`, opened at its first use;
    /// else the calling thread alone. Touches no file.
    pub fn threads(&self, explicit: Option<usize>) -> Threads {
        jobserver::threads(explicit.or(self.threads), self.makeflags.as_deref())
    }

    /// The [`Settings`] the environment asks for: `census`, `threads`
    /// (without an explicit count), bitcode read natively
    /// (`HOD_BITCODE_VIA` is the binary's: in process nothing converts).
    ///
    /// # Errors
    /// [`Environment::error`].
    pub fn settings(&self) -> Result<Settings, EnvError> {
        if let Some(message) = &self.error {
            return Err(EnvError(message.clone()));
        }
        Ok(Settings { census: self.census, threads: self.threads(None), ..Settings::default() })
    }
}

impl Settings {
    /// [`Environment::settings`] of this process's environment. Read it once
    /// per process, not per module.
    ///
    /// # Errors
    /// A variable with a value hod does not accept.
    pub fn from_env() -> Result<Settings, EnvError> {
        Environment::from_process().settings()
    }
}

/// A variable with a value hod does not accept; the message names it
/// (reason `usage`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnvError(pub String);

impl fmt::Display for EnvError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for EnvError {}

#[cfg(test)]
mod tests {
    use super::*;
    use hod_testutil::jobs::PrivatePool;

    // spec/cli.md "Environment".
    #[test]
    fn environment() {
        let vars = |pairs: &'static [(&'static str, &'static str)]| {
            Environment::from_vars(move |name| {
                pairs.iter().find(|(k, _)| *k == name).map(|(_, v)| OsString::from(v))
            })
        };
        assert_eq!(vars(&[]), Environment::default());
        let all = vars(&[
            ("HOD_FALLBACK_LLC", "/usr/lib/llvm-22/bin/llc"),
            ("HOD_STRICT", "1"),
            ("HOD_COVERAGE_LOG", "cov.tsv"),
            ("HOD_BITCODE_VIA", "/usr/lib/llvm-22/bin/llvm-dis"),
            ("HOD_IN_FALLBACK", "1"),
        ]);
        assert_eq!(all.fallback, Some("/usr/lib/llvm-22/bin/llc".into()));
        assert_eq!(all.coverage_log, Some("cov.tsv".into()));
        assert_eq!(all.bitcode_via, Some("/usr/lib/llvm-22/bin/llvm-dis".into()));
        assert!(all.strict && all.in_fallback && all.error.is_none());
        // Empty is unset; HOD_STRICT=0 is off.
        let empty = vars(&[
            ("HOD_FALLBACK_LLC", ""),
            ("HOD_STRICT", ""),
            ("HOD_COVERAGE_LOG", ""),
            ("HOD_BITCODE_VIA", ""),
            ("HOD_IN_FALLBACK", ""),
        ]);
        assert_eq!(empty, Environment::default());
        assert_eq!(vars(&[("HOD_STRICT", "0")]), Environment::default());
        let bad = vars(&[("HOD_STRICT", "yes")]);
        assert!(!bad.strict);
        assert!(bad.error.unwrap().contains("invalid value 'yes' for HOD_STRICT"));
        // WP-061: HOD_COMPRESS_DEBUG_SECTIONS takes the option's values.
        let zlib = vars(&[("HOD_COMPRESS_DEBUG_SECTIONS", "zlib")]);
        assert_eq!(zlib.compress_debug_sections, Some(Compression::Zlib));
        let zstd = vars(&[("HOD_COMPRESS_DEBUG_SECTIONS", "zstd")]);
        assert!(zstd.error.unwrap().contains("compression 'zstd' is not supported"));
        // HOD_CENSUS is read like HOD_STRICT.
        assert!(vars(&[("HOD_CENSUS", "1")]).census);
        assert_eq!(vars(&[("HOD_CENSUS", "0")]), Environment::default());
        assert_eq!(vars(&[("HOD_CENSUS", "")]), Environment::default());
        let bad = vars(&[("HOD_CENSUS", "all")]);
        assert!(!bad.census);
        assert!(bad.error.unwrap().contains("invalid value 'all' for HOD_CENSUS: expected 1 or 0"));
        // HOD_THREADS is a positive integer; MAKEFLAGS is kept for a
        // jobserver ("Threads").
        assert_eq!(vars(&[("HOD_THREADS", "8")]).threads, Some(8));
        assert_eq!(vars(&[("HOD_THREADS", "")]), Environment::default());
        for value in ["0", "-1", "many", "1.5"] {
            let pairs: &'static [(&str, &str)] = Box::leak(Box::new([("HOD_THREADS", value)]));
            let bad = vars(pairs);
            assert_eq!(bad.threads, None);
            let expected = format!("invalid value '{value}' for HOD_THREADS: expected a positive");
            assert!(bad.error.unwrap().contains(&expected), "{value}");
        }
        let make = vars(&[("MAKEFLAGS", "-j4 --jobserver-auth=fifo:/tmp/fifo")]);
        assert_eq!(make.makeflags.as_deref(), Some("-j4 --jobserver-auth=fifo:/tmp/fifo"));
    }

    // spec/cli.md "The command line in the facade": the settings of the
    // environment, the threads by the shim's rule.
    #[test]
    fn settings_of_the_environment() {
        let env = Environment { census: true, threads: Some(3), ..Environment::default() };
        let settings = env.settings().unwrap();
        assert!(settings.census);
        assert_eq!(settings.threads, Threads { max: 3, ..Threads::default() });
        assert_eq!(settings.bitcode, crate::BitcodeReading::Native);
        assert_eq!(settings.support, None);
        // The caller's own count wins over HOD_THREADS.
        assert_eq!(env.threads(Some(5)).max, 5);
        assert_eq!(Environment::default().settings(), Ok(Settings::default()));
        // A bitcode converter is the binary's business.
        let via = Environment { bitcode_via: Some("llvm-dis".into()), ..Environment::default() };
        assert_eq!(via.settings(), Ok(Settings::default()));
        let bad = Environment::from_vars(|name| (name == ENV_STRICT).then(|| "yes".into()));
        let error = bad.settings().unwrap_err();
        assert_eq!(error.to_string(), "invalid value 'yes' for HOD_STRICT: expected 1 or 0");
        // A jobserver: tokens from the pool, at most the client's cap.
        let pool = PrivatePool::new("hod-env-threads", 2);
        let env = Environment { makeflags: Some(pool.makeflags()), ..Environment::default() };
        let threads = env.settings().unwrap().threads;
        assert_eq!(threads.max, jobserver::JOBSERVER_MAX_THREADS);
        let tokens = threads.tokens.expect("a jobserver");
        assert_eq!(tokens.take(3), 2);
        tokens.give_back(2);
        // HOD_THREADS wins over the jobserver: no token is asked for.
        let both = Environment { threads: Some(2), ..env };
        assert!(both.settings().unwrap().threads.tokens.is_none());
    }

    // `Settings::from_env` reads this process: the test runner's
    // environment, which sets none of hod's variables but may have a
    // jobserver; it must not fail.
    #[test]
    fn from_env_reads_the_process() {
        let env = Environment::from_process();
        if env.error.is_none() {
            assert_eq!(Settings::from_env().map(|s| s.census), Ok(env.census));
        }
    }
}
