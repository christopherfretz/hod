//! Test-only support for the link-and-run tests (`spec/object.md`
//! "Link-and-run tests"): the compiler driver, the linkers that are
//! present, and how a linked program is run.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use hod_obj::{Object, elf};
use hod_testutil::{Outcome, Scratch, Target, find_tool, linkers, run, run_with, tool};

/// One way to link: the linker's name and the compiler-driver flags that
/// select it.
#[derive(Clone, Debug)]
pub struct Linker {
    pub name: &'static str,
    flags: Vec<OsString>,
}

impl Linker {
    /// The linker `program`, selected as `name` through a directory
    /// `<base>/use-<name>` ([`hod_testutil::linkers::driver_flags`]).
    fn through(base: &Path, name: &'static str, program: &Path) -> Self {
        Linker { name, flags: linkers::driver_flags(base, name, program) }
    }

    /// The compiler-driver flags that select this linker, for a test that
    /// runs its own `g++` link.
    #[allow(dead_code)] // Not every test binary that includes this uses it.
    pub fn driver_flags(&self) -> &[OsString] {
        &self.flags
    }

    /// Whether this linker honors `SHF_LINK_ORDER` and keeps the members of
    /// a group together when it discards unused sections. GNU ld and lld
    /// do; mold 2.37 treats such sections like any other; wild is excepted
    /// (`hod_testutil::linkers::EXCEPTIONS`, case `link-order`).
    pub fn keeps_associated_sections(&self) -> bool {
        self.name != "mold" && linkers::exception(self.name, "link-order").is_none()
    }
}

/// The compiler drivers for one target, the linkers that are present, and
/// the emulator, if the target is not the host.
///
/// A test for AArch64 and one for x86-64 differ in the [`Target`] given to
/// [`Toolchain::for_target`] alone: on a host of another architecture the
/// GNU cross drivers compile and link, the linkers are the same programs
/// (lld, mold and wild link for every target; GNU ld is the cross
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
        Self::new(scratch, target, "gcc", "g++", Vec::new())
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
        let (driver, cxx) = (target.c_driver(), target.cxx_driver());
        Self::new(scratch, target, &driver, &cxx, runner)
    }

    /// The linkers of the matrix that are present
    /// ([`hod_testutil::linkers::present`]), each selected through a
    /// directory passed with `-B` ([`hod_testutil::linkers::driver_flags`]):
    /// the linker `hod_testutil` found, whatever it is called and wherever
    /// it is installed (`ld.lld-22`, `x86_64-linux-gnu-ld.bfd`, wild's
    /// pinned release).
    fn new(
        scratch: &Scratch,
        target: Target,
        driver: &str,
        cxx: &str,
        runner: Vec<OsString>,
    ) -> Option<Self> {
        let driver = tool(driver)?;
        let linkers = linkers::present(target)
            .into_iter()
            .map(|(name, found)| Linker::through(scratch.path(), name, &found))
            .collect();
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
        let printed = outcome.stderr_text();
        let tolerated = linkers::tolerated_warning(linker.name, flags);
        if tolerated != Some(printed.as_str()) {
            assert_eq!(printed, "", "{}: the link printed diagnostics", linker.name);
        }
        // Which linker really ran is recorded in the output's `.comment`:
        // lld, mold and wild sign it; GNU ld leaves only what the C
        // runtime's start files carry.
        if let Some(readelf) = find_tool("readelf") {
            let flags_and_file =
                ["-p", ".comment"].map(OsStr::new).into_iter().chain([output.as_os_str()]);
            let comment = run(readelf, flags_and_file).stdout_text();
            let signer = linkers::signer(&comment);
            let ok = match linker.name {
                "bfd" => {
                    signer.is_none() && (comment.contains("GCC") || flags.contains(&"-nostdlib"))
                }
                name => signer == Some(name),
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

/// A program header as `readelf -lW` prints it.
#[derive(Clone, Copy, Debug)]
pub struct Segment {
    pub offset: u64,
    pub vaddr: u64,
    pub memsz: u64,
    pub align: u64,
}

/// The `PT_LOAD` and `PT_GNU_RELRO` headers of a linked program, with the
/// text `readelf -lW` printed; `None` when no `readelf` is installed.
pub fn load_and_relro(program: &Path) -> Option<(Vec<Segment>, Vec<Segment>, String)> {
    let readelf = find_tool("llvm-readelf").or_else(|| find_tool("readelf"))?;
    let ran = run(readelf, [OsStr::new("-lW"), program.as_os_str()]);
    assert!(ran.success(), "readelf -lW {}: {}", program.display(), ran.stderr_text());
    let text = ran.stdout_text();
    let (mut loads, mut relros) = (Vec::new(), Vec::new());
    for line in text.lines() {
        // `Type Offset VirtAddr PhysAddr FileSiz MemSiz Flg Align`; the
        // flags may hold a space (`R E`), so the alignment is the last.
        let fields: Vec<&str> = line.split_whitespace().collect();
        let into = match fields.first() {
            Some(&"LOAD") => &mut loads,
            Some(&"GNU_RELRO") => &mut relros,
            _ => continue,
        };
        let hex = |s: &str| u64::from_str_radix(s.trim_start_matches("0x"), 16).unwrap();
        into.push(Segment {
            offset: hex(fields[1]),
            vaddr: hex(fields[2]),
            memsz: hex(fields[5]),
            align: hex(fields[fields.len() - 1]),
        });
    }
    Some((loads, relros, text))
}

/// Asserts what the native dynamic loader requires of a program's program
/// headers and an emulator does not check: each `PT_LOAD`'s address and
/// offset agree modulo its alignment, the `PT_LOAD`s ascend, and each
/// `PT_GNU_RELRO` range, rounded to pages as `ld.so` rounds it (start and
/// end down), lies in pages that `PT_LOAD`s map. glibc's `ld.so` leaves
/// the address gaps between `PT_LOAD`s unmapped, so its `mprotect` of a
/// RELRO range across a gap fails ("cannot apply additional memory
/// protection after relocation: Cannot allocate memory"); `qemu-user`
/// applies it page by page and lets it through. Checked for every page
/// size the target's loader may run with that the file's alignment allows.
pub fn check_loadable(program: &Path, target: Target) {
    let Some((loads, relros, text)) = load_and_relro(program) else { return };
    let shown = program.display();
    assert!(!loads.is_empty(), "{shown}: no PT_LOAD\n{text}");
    for (i, load) in loads.iter().enumerate() {
        let align = load.align.max(1);
        assert_eq!(
            load.vaddr % align,
            load.offset % align,
            "{shown}: PT_LOAD {i}'s address and offset disagree modulo {align:#x}\n{text}"
        );
        if let Some(next) = loads.get(i + 1) {
            assert!(load.vaddr < next.vaddr, "{shown}: PT_LOAD {i} does not ascend\n{text}");
        }
    }
    let pages: &[u64] = match target {
        Target::X86_64 => &[0x1000],
        Target::Aarch64 => &[0x1000, 0x4000, 0x1_0000],
    };
    let min_align = loads.iter().map(|l| l.align).min().unwrap_or(0x1000);
    for &page in pages.iter().filter(|&&page| page <= min_align.max(0x1000)) {
        let down = |a: u64| a & !(page - 1);
        let up = |a: u64| (a + page - 1) & !(page - 1);
        let mapped: Vec<(u64, u64)> =
            loads.iter().map(|l| (down(l.vaddr), up(l.vaddr + l.memsz))).collect();
        for relro in &relros {
            let (start, end) = (down(relro.vaddr), down(relro.vaddr + relro.memsz));
            let mut at = start;
            while at < end {
                let Some(&(_, to)) = mapped.iter().find(|(from, to)| *from <= at && at < *to)
                else {
                    panic!(
                        "{shown}: with {page:#x}-byte pages the loader's mprotect of \
                         PT_GNU_RELRO [{start:#x}, {end:#x}) meets the unmapped page {at:#x} \
                         (ENOMEM natively)\n{text}"
                    );
                };
                at = to;
            }
        }
    }
}
