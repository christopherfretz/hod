//! Test-only support for the link-and-run tests (`spec/object.md`
//! "Link-and-run tests"): the compiler driver, the linkers that are
//! present, and how a linked program is run.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use hod_obj::{Object, elf};
use hod_testutil::{Outcome, Scratch, find_tool, run, run_with, tool, tools_required};

/// One way to link: the linker's name and the compiler-driver flags that
/// select it.
#[derive(Debug)]
pub struct Linker {
    pub name: &'static str,
    flags: Vec<OsString>,
}

impl Linker {
    /// Whether this linker honors `SHF_LINK_ORDER` and keeps the members of
    /// a group together when it discards unused sections. GNU ld and lld
    /// do; mold 2.37 treats such sections like any other.
    pub fn keeps_associated_sections(&self) -> bool {
        self.name != "mold"
    }
}

/// The compiler driver for one target, the linkers that are present, and
/// the emulator, if the target is not the host.
#[derive(Debug)]
pub struct Toolchain {
    driver: PathBuf,
    pub linkers: Vec<Linker>,
    /// The command a program's path is appended to; empty on the host.
    runner: Vec<OsString>,
}

impl Toolchain {
    /// The host's toolchain.
    pub fn host(scratch: &Scratch) -> Option<Self> {
        Self::new(scratch, "gcc", "ld.bfd", Vec::new())
    }

    /// A toolchain for x86-64 Linux: the host's on an x86-64 host, else
    /// the GNU cross compiler driver and `qemu-x86_64` with the cross
    /// system root. lld and mold link for any target as they are.
    pub fn x86_64(scratch: &Scratch) -> Option<Self> {
        if cfg!(target_arch = "x86_64") {
            return Self::host(scratch);
        }
        let sysroot = Path::new("/usr/x86_64-linux-gnu");
        if !sysroot.is_dir() {
            assert!(!tools_required(), "no x86-64 system root (HOD_REQUIRE_TOOLS=1)");
            eprintln!("note: {} not found; skipping", sysroot.display());
            return None;
        }
        let runner = vec![tool("qemu-x86_64")?.into(), "-L".into(), sysroot.into()];
        Self::new(scratch, "x86_64-linux-gnu-gcc", "x86_64-linux-gnu-ld.bfd", runner)
    }

    /// `gcc -fuse-ld=<name>` runs the program `ld.<name>` it finds on its
    /// program path, so each linker gets a directory holding exactly that
    /// name, passed with `-B`. This selects the linker `hod_testutil` found
    /// whatever it is called and wherever it is installed (`ld.lld-22`,
    /// `/usr/lib/llvm-22/bin/ld.lld`, `x86_64-linux-gnu-ld.bfd`).
    fn new(scratch: &Scratch, driver: &str, bfd: &str, runner: Vec<OsString>) -> Option<Self> {
        let driver = tool(driver)?;
        let mut linkers = Vec::new();
        for (name, program) in [("bfd", bfd), ("lld", "ld.lld"), ("mold", "ld.mold")] {
            let Some(found) = tool(program) else { continue };
            let dir = scratch.join(format!("use-{name}"));
            std::fs::create_dir(&dir).unwrap();
            std::os::unix::fs::symlink(&found, dir.join(format!("ld.{name}"))).unwrap();
            let mut prefix = OsString::from("-B");
            prefix.push(&dir);
            linkers.push(Linker { name, flags: vec![prefix, format!("-fuse-ld={name}").into()] });
        }
        Some(Self { driver, linkers, runner })
    }

    /// Compiles C source to an object with the target's compiler.
    pub fn compile_c(
        &self,
        scratch: &Scratch,
        name: &str,
        source: &str,
        flags: &[&str],
    ) -> PathBuf {
        let input = scratch.write(&format!("{name}.c"), source);
        let output = scratch.join(format!("{name}.o"));
        let mut command = Command::new(&self.driver);
        command.args(["-c", "-O1"]).args(flags).arg(&input).arg("-o").arg(&output);
        let outcome = run_with(&mut command, &[], Duration::from_mins(5));
        assert!(outcome.success(), "cannot compile {name}.c: {}", outcome.stderr_text());
        output
    }

    /// Links `inputs` into `output`. Linker warnings are errors.
    pub fn link(
        &self,
        linker: &Linker,
        flags: &[&str],
        inputs: &[&Path],
        output: &Path,
    ) -> Outcome {
        let mut command = Command::new(&self.driver);
        command.args(&linker.flags).arg("-Wl,--fatal-warnings").args(flags);
        command.arg("-o").arg(output).args(inputs);
        run_with(&mut command, &[], Duration::from_mins(3))
    }

    /// Links, requiring success and silence, and checks which linker ran.
    pub fn link_ok(&self, linker: &Linker, flags: &[&str], inputs: &[&Path], output: &Path) {
        let outcome = self.link(linker, flags, inputs, output);
        assert!(
            outcome.success(),
            "{}: link failed (timed out: {}):\n{}",
            linker.name,
            outcome.timed_out(),
            outcome.stderr_text()
        );
        assert_eq!(outcome.stderr_text(), "", "{}: the link printed diagnostics", linker.name);
        // Which linker really ran is recorded in the output's `.comment`:
        // lld and mold sign it; GNU ld leaves only what the C runtime's
        // start files carry.
        if let Some(readelf) = find_tool("readelf") {
            let flags_and_file =
                ["-p", ".comment"].map(OsStr::new).into_iter().chain([output.as_os_str()]);
            let comment = run(readelf, flags_and_file).stdout_text();
            let signed = |by: &str| comment.contains(by);
            let ok = match linker.name {
                "lld" => signed("LLD") && !signed("mold"),
                "mold" => signed("mold") && !signed("LLD"),
                _ => {
                    !signed("LLD")
                        && !signed("mold")
                        && (signed("GCC") || flags.contains(&"-nostdlib"))
                }
            };
            assert!(ok, "{} did not link this:\n{comment}", linker.name);
        }
    }

    /// Runs a linked program, under the emulator when the target needs it.
    pub fn run(&self, linker: &Linker, program: &Path) -> Outcome {
        let ran = match self.runner.split_first() {
            Some((emulator, flags)) => {
                run(emulator, flags.iter().map(OsString::as_os_str).chain([program.as_os_str()]))
            }
            None => run(program, [] as [&str; 0]),
        };
        assert!(!ran.timed_out(), "{}: the program did not finish", linker.name);
        eprintln!("link+run with {:<4}: exit {:?}", linker.name, exit_code(&ran));
        ran
    }

    /// Links, requiring success and silence, then runs the program.
    pub fn link_and_run(
        &self,
        linker: &Linker,
        flags: &[&str],
        inputs: &[&Path],
        output: &Path,
    ) -> Outcome {
        self.link_ok(linker, flags, inputs, output);
        self.run(linker, output)
    }
}

pub fn exit_code(outcome: &Outcome) -> Option<i32> {
    outcome.status.and_then(|status| status.code())
}

/// Writes an object into the scratch directory.
pub fn write(scratch: &Scratch, name: &str, obj: &Object) -> PathBuf {
    scratch.write(name, elf::to_vec(obj).unwrap())
}

/// The offset of the first occurrence of `marker` in a file.
pub fn find_marker(file: &Path, marker: &str) -> Option<usize> {
    let bytes = std::fs::read(file).unwrap();
    bytes.windows(marker.len()).position(|window| window == marker.as_bytes())
}
