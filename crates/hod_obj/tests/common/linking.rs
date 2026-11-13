//! Test-only support for the link-and-run tests (`spec/object.md`
//! "Link-and-run tests"): the compiler driver, the linkers that are
//! present, and how a linked program is run.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use hod_obj::{Object, elf};
use hod_testutil::{Outcome, Scratch, Target, find_tool, run, run_with, tool};

/// One way to link: the linker's name and the compiler-driver flags that
/// select it.
#[derive(Clone, Debug)]
pub struct Linker {
    pub name: &'static str,
    flags: Vec<OsString>,
}

impl Linker {
    /// The linker `program`, selected as `name` through a directory
    /// `<base>/use-<name>` that holds it as `ld.<name>` (see
    /// [`Toolchain`]'s constructor).
    fn through(base: &Path, name: &'static str, program: &Path) -> Self {
        let dir = base.join(format!("use-{name}"));
        std::fs::create_dir(&dir).unwrap();
        std::os::unix::fs::symlink(program, dir.join(format!("ld.{name}"))).unwrap();
        let mut prefix = OsString::from("-B");
        prefix.push(&dir);
        Linker { name, flags: vec![prefix, format!("-fuse-ld={name}").into()] }
    }

    /// Whether this linker honors `SHF_LINK_ORDER` and keeps the members of
    /// a group together when it discards unused sections. GNU ld and lld
    /// do; mold 2.37 treats such sections like any other.
    pub fn keeps_associated_sections(&self) -> bool {
        self.name != "mold"
    }
}

/// The compiler drivers for one target, the linkers that are present, and
/// the emulator, if the target is not the host.
///
/// A test for AArch64 and one for x86-64 differ in the [`Target`] given to
/// [`Toolchain::for_target`] alone: on a host of another architecture the
/// GNU cross drivers compile and link, the three linkers are the same
/// programs (lld and mold link for every target; GNU ld is the cross
/// `<prefix>-ld.bfd`), and programs run under `qemu-<arch>` with the cross
/// system root and the baseline CPU model ([`hod_testutil::target`]).
#[derive(Debug)]
pub struct Toolchain {
    /// The target programs are built for.
    pub target: Target,
    driver: PathBuf,
    /// The C++ driver, if it is installed ([`Toolchain::compile_cxx`]).
    cxx_driver: Option<PathBuf>,
    pub linkers: Vec<Linker>,
    /// The command a program's path is appended to; empty on the host.
    runner: Vec<OsString>,
}

impl Toolchain {
    /// The host's toolchain.
    pub fn host(scratch: &Scratch) -> Option<Self> {
        let target = Target::host().unwrap_or(Target::Aarch64);
        Self::new(scratch, target, "gcc", "g++", "ld.bfd", Vec::new())
    }

    /// A toolchain for AArch64 Linux: [`Toolchain::for_target`] (the
    /// host's on an AArch64 host, the cross one and `qemu-aarch64` on any
    /// other).
    pub fn aarch64(scratch: &Scratch) -> Option<Self> {
        Self::for_target(scratch, Target::Aarch64)
    }

    /// A toolchain for x86-64 Linux: [`Toolchain::for_target`].
    pub fn x86_64(scratch: &Scratch) -> Option<Self> {
        Self::for_target(scratch, Target::X86_64)
    }

    /// A toolchain for `target`: the host's when it is the host, else the
    /// GNU cross compiler drivers and `qemu-<arch> -cpu <baseline> -L
    /// <system root>`. A missing cross tool is a note naming its Debian
    /// package, and `None` (a failure under `HOD_REQUIRE_TOOLS=1`).
    pub fn for_target(scratch: &Scratch, target: Target) -> Option<Self> {
        if target.is_host() {
            return Self::host(scratch);
        }
        let sysroot = target.tool("sysroot")?;
        let mut runner = vec![target.tool("emulator")?.into_os_string()];
        runner.extend(target.emulator_args(&sysroot));
        let (driver, cxx, bfd) = (target.c_driver(), target.cxx_driver(), target.bfd());
        Self::new(scratch, target, &driver, &cxx, &bfd, runner)
    }

    /// `gcc -fuse-ld=<name>` runs the program `ld.<name>` it finds on its
    /// program path, so each linker gets a directory holding exactly that
    /// name, passed with `-B`. This selects the linker `hod_testutil` found
    /// whatever it is called and wherever it is installed (`ld.lld-22`,
    /// `/usr/lib/llvm-22/bin/ld.lld`, `x86_64-linux-gnu-ld.bfd`).
    fn new(
        scratch: &Scratch,
        target: Target,
        driver: &str,
        cxx: &str,
        bfd: &str,
        runner: Vec<OsString>,
    ) -> Option<Self> {
        let driver = tool(driver)?;
        let mut linkers = Vec::new();
        for (name, program) in [("bfd", bfd), ("lld", "ld.lld"), ("mold", "ld.mold")] {
            let Some(found) = tool(program) else { continue };
            linkers.push(Linker::through(scratch.path(), name, &found));
        }
        Some(Self { target, driver, cxx_driver: find_tool(cxx), linkers, runner })
    }

    /// lld of one LLVM major (`ld.lld-<major>`), besides the matrix's
    /// (the newest found), if it is installed.
    pub fn lld(scratch: &Scratch, major: u32) -> Option<Linker> {
        let found = find_tool(&format!("ld.lld-{major}"))?;
        let dir = scratch.join(format!("lld-{major}"));
        std::fs::create_dir_all(&dir).unwrap();
        Some(Linker::through(&dir, "lld", &found))
    }

    /// The same toolchain whose emulator models the CPU `cpu` (QEMU's
    /// name, `max`): for a test of instructions an extension adds, which
    /// the baseline model ([`Target::emulated_cpu`]) lacks. A native
    /// runner is unchanged.
    #[allow(dead_code)] // Not every test binary that includes this uses it.
    pub fn with_emulated_cpu(mut self, cpu: &str) -> Self {
        if let Some(at) = self.runner.iter().position(|arg| arg == "-cpu") {
            self.runner[at + 1] = cpu.into();
        }
        self
    }

    /// Whether programs run under an emulator.
    pub fn emulated(&self) -> bool {
        !self.runner.is_empty()
    }

    /// The command that runs `program` (under the emulator, if any), for
    /// a test to add arguments to.
    pub fn runner_command(&self, program: &Path) -> Command {
        match self.runner.split_first() {
            Some((emulator, flags)) => {
                let mut command = Command::new(emulator);
                command.args(flags).arg(program);
                command
            }
            None => Command::new(program),
        }
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
        Self::compile_with(&self.driver, &input, name, flags)
    }

    /// Compiles C++ source to an object with the target's C++ compiler;
    /// `None` (after a note naming its package, or a panic under
    /// `HOD_REQUIRE_TOOLS=1`) when it is not installed. The program is
    /// linked with `-lstdc++` among the inputs.
    pub fn compile_cxx(
        &self,
        scratch: &Scratch,
        name: &str,
        source: &str,
        flags: &[&str],
    ) -> Option<PathBuf> {
        let Some(cxx) = &self.cxx_driver else {
            self.target.tool("g++");
            return None;
        };
        let input = scratch.write(&format!("{name}.cpp"), source);
        Some(Self::compile_with(cxx, &input, name, flags))
    }

    fn compile_with(driver: &Path, input: &Path, name: &str, flags: &[&str]) -> PathBuf {
        let output = input.with_file_name(format!("{name}.o"));
        let mut command = Command::new(driver);
        command.args(["-c", "-O1"]).args(flags).arg(input).arg("-o").arg(&output);
        let outcome = run_with(&mut command, &[], Duration::from_mins(5));
        let shown = input.file_name().unwrap_or_default().display();
        assert!(outcome.success(), "cannot compile {shown}: {}", outcome.stderr_text());
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

    /// Runs a linked program, under the emulator when the target needs it
    /// (with the five-minute hang guard of compiles: an emulated program
    /// at load 30 has taken longer than the host's one minute).
    pub fn run(&self, linker: &Linker, program: &Path) -> Outcome {
        let ran = if self.emulated() {
            run_with(&mut self.runner_command(program), &[], Duration::from_mins(5))
        } else {
            run(program, [] as [&str; 0])
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
