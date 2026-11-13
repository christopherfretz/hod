//! Finding the tools of a differential run: the oracle `llc`, the linker
//! driver and the linker matrix. Contract: `spec/harness.md` "irdiff",
//! "Tools".

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::time::Duration;

use hod_testutil::Target;

use super::directives::Arch;
use super::engine::REFERENCE_SELECTOR;
use super::process::{self, End, Limits};

/// The LLVM majors searched when none is asked for, preferred first.
pub const LLVM_MAJORS: [u32; 2] = [22, 19];

/// The linkers that must be present under `HOD_REQUIRE_TOOLS=1`. `wild`
/// is in the matrix when it is found, and optional until it is provisioned.
pub const REQUIRED_LINKERS: [&str; 3] = ["bfd", "lld", "mold"];

/// Where tools are looked for.
#[derive(Clone, Debug)]
pub struct Search {
    /// The value of `PATH`.
    pub path: OsString,
    /// The directory that holds `llvm-<N>/bin` (`/usr/lib`).
    pub llvm_root: PathBuf,
}

impl Search {
    /// This process's `PATH` and `/usr/lib`.
    pub fn from_env() -> Self {
        Self {
            path: std::env::var_os("PATH").unwrap_or_default(),
            llvm_root: PathBuf::from("/usr/lib"),
        }
    }

    /// The first file named `file` in the directories of `path`.
    pub fn on_path(&self, file: &str) -> Option<PathBuf> {
        std::env::split_paths(&self.path).map(|dir| dir.join(file)).find(|p| p.is_file())
    }

    /// `<tool>-<major>` on `PATH`, else `<llvm_root>/llvm-<major>/bin/<tool>`.
    pub fn versioned(&self, tool: &str, major: u32) -> Option<PathBuf> {
        self.on_path(&format!("{tool}-{major}")).or_else(|| {
            let installed = self.llvm_root.join(format!("llvm-{major}/bin/{tool}"));
            installed.is_file().then_some(installed)
        })
    }
}

/// An oracle.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Llvm {
    pub llc: PathBuf,
    pub major: u32,
}

/// A linker of the matrix.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Linker {
    /// `bfd`, `lld`, `mold` or `wild`.
    pub name: String,
    pub path: PathBuf,
}

/// Everything one differential run executes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Toolchain {
    /// The oracle.
    pub llc: PathBuf,
    /// Its LLVM major.
    pub llvm_major: u32,
    /// The `hod` under test.
    pub hod: PathBuf,
    /// The compiler driver that links (`gcc`); without one nothing is
    /// linked or run.
    pub driver: Option<PathBuf>,
    /// The linkers to link with, in report order.
    pub linkers: Vec<Linker>,
    /// What runs the programs of a cross target ("Cross runs"); `None`
    /// runs them directly, which only the host's own programs can be.
    pub emulator: Option<Emulator>,
}

/// The user-mode emulator that runs a cross target's programs: `program
/// <args> <test program> <test args>`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Emulator {
    /// `qemu-<arch>`.
    pub program: PathBuf,
    /// `-cpu <baseline> -L <system root>` ([`Target::emulator_args`]).
    pub args: Vec<OsString>,
}

/// The `hod_testutil` name of an architecture, which knows its cross
/// tools, system root and emulated CPU.
pub fn target_of(arch: Arch) -> Target {
    match arch {
        Arch::Aarch64 => Target::Aarch64,
        Arch::X86_64 => Target::X86_64,
    }
}

/// The arguments the oracle's `llc` gets after `-O0` for `arch`: the
/// instruction selector (`spec/harness.md` "The oracle"). The one place
/// the reference is configured per target; the hybrid fallback and every
/// harness command that builds a reference use it.
pub fn reference_selector(arch: Arch) -> &'static [&'static str] {
    match arch {
        Arch::Aarch64 | Arch::X86_64 => &REFERENCE_SELECTOR,
    }
}

/// Finds the compiler driver that links programs of `arch`: `gcc` for the
/// host's, else the GNU cross driver `<prefix>-gcc`.
pub fn find_driver_for(arch: Arch, search: &Search) -> Option<PathBuf> {
    search.on_path(&target_of(arch).c_driver())
}

/// [`find_linker`] for programs of `arch`: GNU ld of another architecture
/// is its cross `<prefix>-ld.bfd`; lld, mold and wild link every target.
pub fn find_linker_for(name: &str, arch: Arch, search: &Search) -> Option<PathBuf> {
    match name {
        "bfd" => search.on_path(&target_of(arch).bfd()),
        _ => find_linker(name, search),
    }
}

/// Finds the emulator of `arch`'s programs: `qemu-<arch>`, then
/// `qemu-<arch>-static`.
pub fn find_emulator(arch: Arch, search: &Search) -> Option<PathBuf> {
    let name = target_of(arch).emulator();
    search.on_path(&name).or_else(|| search.on_path(&format!("{name}-static")))
}

/// The limits of a tool the harness only asks for its version.
const PROBE: Limits = Limits { time: Duration::from_secs(20), memory_kib: 4 << 20 };

/// The LLVM major `llc --version` reports ("LLVM version 22.1.8").
pub fn llc_major(llc: &Path) -> Result<u32, String> {
    let output = process::run(
        llc,
        |c| {
            c.arg("--version");
        },
        b"",
        PROBE,
    )?;
    let text = String::from_utf8_lossy(&output.stdout);
    let major = text.split("LLVM version ").nth(1).and_then(|rest| {
        let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
        digits.parse::<u32>().ok()
    });
    match (output.end, major) {
        (End::Exit(0), Some(major)) => Ok(major),
        _ => Err(format!("'{} --version' did not report an LLVM version", llc.display())),
    }
}

/// Finds the oracle.
///
/// - `None`: the first of LLVM 22 and 19 that is installed (`llc-<N>` on
///   `PATH`, else `/usr/lib/llvm-<N>/bin/llc`), else a plain `llc` on
///   `PATH` if it is one of those majors.
/// - a number: that major, found the same way.
/// - anything else: an installation directory; its `bin/llc` must exist.
///
/// `Ok(None)` means no such LLVM is installed. An error is a request that
/// cannot be met: a directory without `bin/llc`, or an `llc` that does not
/// say what it is.
pub fn find_llvm(selector: Option<&str>, search: &Search) -> Result<Option<Llvm>, String> {
    let wanted: Vec<u32> = match selector {
        None => LLVM_MAJORS.to_vec(),
        Some(text) if !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit()) => {
            vec![text.parse().map_err(|_| format!("invalid LLVM major '{text}'"))?]
        }
        Some(dir) => {
            let llc = Path::new(dir).join("bin").join("llc");
            if !llc.is_file() {
                return Err(format!("'{dir}' is not an LLVM installation: no bin/llc"));
            }
            let llc = std::path::absolute(&llc).unwrap_or(llc);
            let major = llc_major(&llc)?;
            return Ok(Some(Llvm { llc, major }));
        }
    };
    for &major in &wanted {
        if let Some(llc) = search.versioned("llc", major) {
            let reported = llc_major(&llc)?;
            if reported != major {
                return Err(format!("'{}' is LLVM {reported}, not {major}", llc.display()));
            }
            return Ok(Some(Llvm { llc, major }));
        }
    }
    if let Some(llc) = search.on_path("llc")
        && let Ok(major) = llc_major(&llc)
        && wanted.contains(&major)
    {
        return Ok(Some(Llvm { llc, major }));
    }
    Ok(None)
}

/// The `opt` of the oracle `llc`: `opt-<N>` beside `llc-<N>`, else
/// `opt` beside it. It may not exist; whoever runs it says so.
pub fn opt_beside(llc: &Path) -> PathBuf {
    let name = llc.file_name().map(OsStr::to_string_lossy).unwrap_or_default();
    let opt = match name.strip_prefix("llc") {
        Some(suffix) if suffix.starts_with('-') => format!("opt{suffix}"),
        _ => "opt".to_owned(),
    };
    llc.with_file_name(opt)
}

/// Finds a linker of the matrix by its name.
///
/// - `bfd`: `ld.bfd`.
/// - `lld`: `ld.lld-<N>` or `/usr/lib/llvm-<N>/bin/ld.lld` for LLVM 22,
///   then 19, then `ld.lld`.
/// - `mold`: `mold`, then `ld.mold`.
/// - `wild`: `wild`, then `ld.wild`.
pub fn find_linker(name: &str, search: &Search) -> Option<PathBuf> {
    match name {
        "bfd" => search.on_path("ld.bfd"),
        "lld" => LLVM_MAJORS
            .into_iter()
            .find_map(|major| search.versioned("ld.lld", major))
            .or_else(|| search.on_path("ld.lld")),
        "mold" => search.on_path("mold").or_else(|| search.on_path("ld.mold")),
        "wild" => search.on_path("wild").or_else(|| search.on_path("ld.wild")),
        _ => None,
    }
}

/// Finds the compiler driver that links: `gcc`.
pub fn find_driver(search: &Search) -> Option<PathBuf> {
    search.on_path("gcc")
}

/// What the driver passes to a linker besides the test's own needs: one
/// thread, because the harness is what runs things in parallel.
pub fn linker_flags(name: &str) -> &'static [&'static str] {
    match name {
        "lld" => &["-Wl,--threads=1"],
        "mold" => &["-Wl,--thread-count=1"],
        _ => &[],
    }
}

/// Which linker's signature a linked program carries in its `.comment`
/// section: `LLD <digit>` is lld's, `mold <digit>` is mold's. GNU ld leaves
/// none.
pub fn signature(program: &[u8]) -> Option<&'static str> {
    let has = |marker: &[u8]| {
        program
            .windows(marker.len() + 1)
            .any(|w| w.starts_with(marker) && w[marker.len()].is_ascii_digit())
    };
    if has(b"mold ") {
        Some("mold")
    } else if has(b"LLD ") {
        Some("lld")
    } else {
        None
    }
}

/// Checks that `program` was linked by the linker named `name`. `wild` is
/// not checked: its signature is not specified yet.
pub fn check_signature(name: &str, program: &[u8]) -> Result<(), String> {
    let expected = match name {
        "bfd" => None,
        "lld" => Some("lld"),
        "mold" => Some("mold"),
        _ => return Ok(()),
    };
    let found = signature(program);
    if found == expected {
        Ok(())
    } else {
        Err(format!(
            "the driver did not link with {name}: the program carries {} signature",
            found.map_or_else(|| "no linker's".to_owned(), |other| format!("{other}'s"))
        ))
    }
}

/// `path` as an `OsString` with `prefix` in front (`-B<dir>`).
pub fn prefixed(prefix: &str, path: &Path) -> OsString {
    let mut joined = OsString::from(prefix);
    joined.push(path);
    joined
}

/// The file name of a path, for messages.
pub fn base_name(path: &Path) -> String {
    path.file_name().unwrap_or(OsStr::new("?")).to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use hod_testutil::Scratch;

    fn fake_llc(scratch: &Scratch, at: &str, version: &str) -> PathBuf {
        let path = scratch.join(at);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        hod_testutil::write_script(
            &path,
            &format!("echo 'Debian LLVM version {version}'; echo '  Optimized build.'"),
        );
        path
    }

    fn search(scratch: &Scratch, dirs: &[&str]) -> Search {
        let path = std::env::join_paths(dirs.iter().map(|d| scratch.join(d))).unwrap();
        Search { path, llvm_root: scratch.join("usr-lib") }
    }

    // spec/harness.md "irdiff", "Tools": the oracle, by default.
    #[test]
    fn llvm_default_search() {
        let scratch = Scratch::new("irdiff-llvm-default");
        let empty = search(&scratch, &["bin"]);
        assert_eq!(find_llvm(None, &empty), Ok(None));

        // An installation directory is found; 22 is preferred to 19.
        let llc19 = fake_llc(&scratch, "usr-lib/llvm-19/bin/llc", "19.1.7");
        assert_eq!(find_llvm(None, &empty), Ok(Some(Llvm { llc: llc19.clone(), major: 19 })));
        let llc22 = fake_llc(&scratch, "bin/llc-22", "22.1.8");
        assert_eq!(find_llvm(None, &empty), Ok(Some(Llvm { llc: llc22.clone(), major: 22 })));
        // A versioned name on PATH wins over the installation directory.
        let named19 = fake_llc(&scratch, "bin/llc-19", "19.1.0");
        assert_eq!(find_llvm(Some("19"), &empty), Ok(Some(Llvm { llc: named19, major: 19 })));
        assert_eq!(find_llvm(Some("22"), &empty), Ok(Some(Llvm { llc: llc22, major: 22 })));
        assert_eq!(find_llvm(Some("21"), &empty), Ok(None));
    }

    // "Tools": a plain `llc` counts only if it is a wanted major.
    #[test]
    fn plain_llc_must_be_a_wanted_major() {
        let scratch = Scratch::new("irdiff-llvm-plain");
        let plain = fake_llc(&scratch, "bin/llc", "18.1.3");
        let found = search(&scratch, &["bin"]);
        assert_eq!(find_llvm(None, &found), Ok(None));
        assert_eq!(find_llvm(Some("19"), &found), Ok(None));
        assert_eq!(find_llvm(Some("18"), &found), Ok(Some(Llvm { llc: plain, major: 18 })));

        let plain = fake_llc(&scratch, "other/llc", "19.1.7");
        let found = search(&scratch, &["other"]);
        assert_eq!(find_llvm(None, &found), Ok(Some(Llvm { llc: plain, major: 19 })));
    }

    // "Tools": `--llvm <dir>`, and tools that are not what they claim.
    #[test]
    fn llvm_directory_and_errors() {
        let scratch = Scratch::new("irdiff-llvm-dir");
        let nowhere = search(&scratch, &["bin"]);
        let llc = fake_llc(&scratch, "opt/llvm/bin/llc", "20.1.0");
        let dir = scratch.join("opt/llvm");
        let found = find_llvm(Some(dir.to_str().unwrap()), &nowhere);
        assert_eq!(found, Ok(Some(Llvm { llc, major: 20 })));

        let missing = find_llvm(Some(scratch.join("nope").to_str().unwrap()), &nowhere);
        assert!(missing.unwrap_err().contains("is not an LLVM installation: no bin/llc"));

        // `llc-22` that reports another major is refused, not used.
        fake_llc(&scratch, "bin/llc-22", "19.1.7");
        let wrong = find_llvm(Some("22"), &nowhere).unwrap_err();
        assert!(wrong.contains("is LLVM 19, not 22"), "{wrong}");

        // A tool that does not report a version at all.
        let mute = scratch.join("mute/bin/llc");
        std::fs::create_dir_all(mute.parent().unwrap()).unwrap();
        hod_testutil::write_script(&mute, "echo hello");
        let silent = find_llvm(Some(scratch.join("mute").to_str().unwrap()), &nowhere);
        assert!(silent.unwrap_err().contains("did not report an LLVM version"));
    }

    // "Tools": the linker matrix and the driver.
    #[test]
    fn linkers_and_driver() {
        let scratch = Scratch::new("irdiff-linkers");
        let found = search(&scratch, &["bin"]);
        for name in ["bfd", "lld", "mold", "wild", "gold"] {
            assert_eq!(find_linker(name, &found), None, "{name}");
        }
        assert_eq!(find_driver(&found), None);

        let touch = |at: &str| {
            let path = scratch.join(at);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, "").unwrap();
            path
        };
        let gcc = touch("bin/gcc");
        assert_eq!(find_driver(&found), Some(gcc));
        let bfd = touch("bin/ld.bfd");
        touch("bin/ld");
        assert_eq!(find_linker("bfd", &found), Some(bfd));

        let plain_lld = touch("bin/ld.lld");
        assert_eq!(find_linker("lld", &found), Some(plain_lld));
        let lld19 = touch("usr-lib/llvm-19/bin/ld.lld");
        assert_eq!(find_linker("lld", &found), Some(lld19));
        let lld22 = touch("bin/ld.lld-22");
        assert_eq!(find_linker("lld", &found), Some(lld22));

        let ld_mold = touch("bin/ld.mold");
        assert_eq!(find_linker("mold", &found), Some(ld_mold));
        let mold = touch("bin/mold");
        assert_eq!(find_linker("mold", &found), Some(mold));
        let wild = touch("bin/wild");
        assert_eq!(find_linker("wild", &found), Some(wild));
    }

    // "Tools": which linker linked a program.
    #[test]
    fn linker_signatures() {
        let gnu = b"\0GCC: (Debian 14.2.0-19) 14.2.0\0";
        let lld = b"\0GCC: (Debian 14.2.0-19) 14.2.0\0Linker: Debian LLD 22.1.8\0";
        let mold = b"mold 2.37.1 (compatible with GNU ld)\0GCC: (Debian 14.2.0-19) 14.2.0\0";
        assert_eq!(signature(gnu), None);
        assert_eq!(signature(lld), Some("lld"));
        assert_eq!(signature(mold), Some("mold"));
        // The words alone, as a program's own data, are not a signature.
        assert_eq!(signature(b"the LLD linker; bread mold grows"), None);
        assert_eq!(signature(b"mold "), None);

        assert_eq!(check_signature("bfd", gnu), Ok(()));
        assert_eq!(check_signature("lld", lld), Ok(()));
        assert_eq!(check_signature("mold", mold), Ok(()));
        assert_eq!(check_signature("wild", gnu), Ok(()));
        let wrong = check_signature("lld", gnu).unwrap_err();
        assert!(wrong.contains("did not link with lld") && wrong.contains("no linker's"));
        let wrong = check_signature("bfd", mold).unwrap_err();
        assert!(wrong.contains("did not link with bfd") && wrong.contains("mold's"), "{wrong}");
    }

    // "Linking": each linker runs on one thread.
    #[test]
    fn opt_beside_the_oracle() {
        let opt = |llc: &str| opt_beside(Path::new(llc));
        assert_eq!(opt("/usr/bin/llc-22"), Path::new("/usr/bin/opt-22"));
        assert_eq!(opt("/usr/lib/llvm-19/bin/llc"), Path::new("/usr/lib/llvm-19/bin/opt"));
        assert_eq!(opt("/x/bin/llc-19.1"), Path::new("/x/bin/opt-19.1"));
        assert_eq!(opt("/x/bin/my-llc"), Path::new("/x/bin/opt"));
    }

    // "Cross runs": the driver, GNU ld and the emulator of the target that
    // is not the host; lld and mold are the same for every target.
    #[test]
    fn cross_tools() {
        let scratch = Scratch::new("irdiff-cross-tools");
        let found = search(&scratch, &["bin"]);
        let host = Arch::host().expect("a Linux host");
        let other = if host == Arch::Aarch64 { Arch::X86_64 } else { Arch::Aarch64 };
        let prefix = target_of(other).gnu_prefix();
        let touch = |name: &str| {
            let path = scratch.join("bin").join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, "").unwrap();
            path
        };
        let (gcc, bfd, lld) = (touch("gcc"), touch("ld.bfd"), touch("ld.lld"));
        assert_eq!(find_driver_for(host, &found), Some(gcc));
        assert_eq!(find_driver_for(other, &found), None, "the host's gcc is not a cross driver");
        assert_eq!(find_linker_for("bfd", host, &found), Some(bfd));
        assert_eq!(find_linker_for("bfd", other, &found), None);
        let cross_gcc = touch(&format!("{prefix}-gcc"));
        let cross_bfd = touch(&format!("{prefix}-ld.bfd"));
        assert_eq!(find_driver_for(other, &found), Some(cross_gcc));
        assert_eq!(find_linker_for("bfd", other, &found), Some(cross_bfd));
        assert_eq!(find_linker_for("lld", other, &found), Some(lld));

        assert_eq!(find_emulator(other, &found), None);
        let qemu = format!("qemu-{}", other.name());
        let fallback = touch(&format!("{qemu}-static"));
        assert_eq!(find_emulator(other, &found), Some(fallback));
        let preferred = touch(&qemu);
        assert_eq!(find_emulator(other, &found), Some(preferred));
        // The reference is configured per target in one place.
        for arch in Arch::ALL {
            assert_eq!(reference_selector(arch), ["-global-isel=0", "-fast-isel=0"]);
        }
    }

    #[test]
    fn single_threaded_linkers() {
        assert_eq!(linker_flags("bfd"), &[] as &[&str]);
        assert_eq!(linker_flags("lld"), &["-Wl,--threads=1"]);
        assert_eq!(linker_flags("mold"), &["-Wl,--thread-count=1"]);
        assert_eq!(linker_flags("wild"), &[] as &[&str]);
    }
}
