//! The targets programs are linked and run for: the host's own, or another
//! architecture through its GNU cross toolchain and the `qemu-user`
//! emulator (`spec/harness.md` "Cross runs").
//!
//! This is the one place that knows how a target's tools are named, where
//! its system root is and which CPU the emulator models, for the
//! link-and-run tests of the crates (`crates/hod_obj/tests/common/linking.rs`)
//! and for the harness (`xtask irdiff`).
//!
//! On Debian, an x86-64 cross toolchain on an AArch64 host is the packages
//! `gcc-x86-64-linux-gnu`, `g++-x86-64-linux-gnu`,
//! `binutils-x86-64-linux-gnu`, `libc6-dev-amd64-cross`,
//! `libstdc++-14-dev-amd64-cross` (system root `/usr/x86_64-linux-gnu`) and
//! `qemu-user` (`qemu-x86_64`); the AArch64 one on an x86-64 host is the
//! same with `aarch64-linux-gnu` and `arm64`.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// The CPU `qemu-x86_64` models for programs under test: x86-64 as the
/// psABI's baseline defines it (`x86-64-v1`: SSE and SSE2, CMOV, CX8, FXSR,
/// MMX, SYSCALL), which is the baseline hod generates for. QEMU's `qemu64`
/// model has SSE3, `CMPXCHG16B` and `LAHF`/`SAHF` in 64-bit mode on top,
/// which are removed. The emulator faults (`SIGILL`) on an instruction of a
/// feature the model does not have, so a program that uses one fails here
/// as it would on such a machine. The default `max` model has everything up
/// to AVX2 and would hide such a use.
///
/// `cmpxchg16b` is not in the baseline: LLVM's default x86-64 CPU calls
/// `__atomic_compare_exchange_16` for a 16-byte `cmpxchg`, and hod uses
/// the instruction only in functions whose target features include
/// `+cx16`. A test of such a function asks for the feature with the `cpu`
/// directive (`spec/harness.md` "Test directives"), which
/// [`cpu_with_features`] adds to this model for that test alone.
pub const X86_64_EMULATED_CPU: &str = "qemu64,-sse3,-cx16,-lahf-lm";

/// The CPU `qemu-aarch64` models: ARMv8.0-A without the extensions (no
/// LSE atomics), hod's AArch64 baseline. Not exercised on an AArch64 host.
pub const AARCH64_EMULATED_CPU: &str = "cortex-a53";

/// The CPU model `cpu` (`<model>,<+feature|-feature>...`, QEMU's syntax)
/// with each of `features` (names without the sign) turned on: a
/// `-<feature>` of the model is dropped and `+<feature>` appended, once.
/// QEMU applies every `-` after every `+`, so removing the `-` is what
/// turns a feature back on; the `+` turns on one the model lacks.
pub fn cpu_with_features(cpu: &str, features: &[&str]) -> String {
    let mut parts: Vec<String> = cpu.split(',').map(str::to_owned).collect();
    for feature in features {
        parts.retain(|part| part.strip_prefix('-') != Some(feature));
        let on = format!("+{feature}");
        if !parts.contains(&on) {
            parts.push(on);
        }
    }
    parts.join(",")
}

/// A target architecture of a Linux program.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Target {
    Aarch64,
    X86_64,
}

impl Target {
    /// Every target.
    pub const ALL: [Self; 2] = [Self::Aarch64, Self::X86_64];

    /// `aarch64` or `x86_64`, as in a triple.
    pub fn name(self) -> &'static str {
        match self {
            Self::Aarch64 => "aarch64",
            Self::X86_64 => "x86_64",
        }
    }

    /// The target this machine is, if it is a Linux machine of one.
    pub fn host() -> Option<Self> {
        if !cfg!(target_os = "linux") {
            return None;
        }
        Self::ALL.into_iter().find(|target| target.name() == std::env::consts::ARCH)
    }

    /// Whether programs of this target run here without an emulator.
    pub fn is_host(self) -> bool {
        Self::host() == Some(self)
    }

    /// The prefix of the GNU cross tools and the name of the system root:
    /// `x86_64-linux-gnu`.
    pub fn gnu_prefix(self) -> &'static str {
        match self {
            Self::Aarch64 => "aarch64-linux-gnu",
            Self::X86_64 => "x86_64-linux-gnu",
        }
    }

    /// The system root of the cross packages: `/usr/<prefix>`, which holds
    /// the target's dynamic loader and shared libraries.
    pub fn sysroot(self) -> PathBuf {
        Path::new("/usr").join(self.gnu_prefix())
    }

    /// The name of the C compiler driver: `gcc` for the host, else the
    /// cross driver `<prefix>-gcc`.
    pub fn c_driver(self) -> String {
        self.tool_name("gcc")
    }

    /// The name of the C++ compiler driver: `g++` or `<prefix>-g++`.
    pub fn cxx_driver(self) -> String {
        self.tool_name("g++")
    }

    /// The name of GNU ld: `ld.bfd` or `<prefix>-ld.bfd`. lld and mold
    /// are one program for every target; they take the target from the
    /// first object (the driver also passes `-m <emulation>`).
    pub fn bfd(self) -> String {
        self.tool_name("ld.bfd")
    }

    /// The name of the user-mode emulator: `qemu-x86_64`.
    pub fn emulator(self) -> String {
        format!("qemu-{}", self.name())
    }

    /// The CPU model the emulator is given ([`X86_64_EMULATED_CPU`]).
    pub fn emulated_cpu(self) -> &'static str {
        match self {
            Self::Aarch64 => AARCH64_EMULATED_CPU,
            Self::X86_64 => X86_64_EMULATED_CPU,
        }
    }

    /// The emulator's arguments before the program: the CPU model and the
    /// system root its dynamic loader and libraries are found in (`-L`; the
    /// environment's `QEMU_LD_PREFIX` is the same thing, but an argument
    /// cannot leak into or out of the program's environment).
    pub fn emulator_args(self, sysroot: &Path) -> Vec<OsString> {
        self.emulator_args_with(sysroot, &[])
    }

    /// [`Target::emulator_args`] with CPU `features` added to the model
    /// ([`cpu_with_features`]): the runner of a test with a `cpu`
    /// directive.
    pub fn emulator_args_with(self, sysroot: &Path, features: &[&str]) -> Vec<OsString> {
        let cpu = cpu_with_features(self.emulated_cpu(), features);
        vec!["-cpu".into(), cpu.into(), "-L".into(), sysroot.into()]
    }

    /// The Debian package that provides `what` for this target as a cross
    /// target: `gcc`, `g++`, `ld.bfd`, `sysroot` (the C library and the
    /// dynamic loader), `libstdc++` or `emulator`.
    pub fn cross_package(self, what: &str) -> String {
        let (dashed, arch) = match self {
            Self::Aarch64 => ("aarch64-linux-gnu", "arm64"),
            Self::X86_64 => ("x86-64-linux-gnu", "amd64"),
        };
        match what {
            "gcc" | "g++" => format!("{what}-{dashed}"),
            "ld.bfd" => format!("binutils-{dashed}"),
            "sysroot" => format!("libc6-dev-{arch}-cross"),
            "libstdc++" => format!("libstdc++-14-dev-{arch}-cross"),
            _ => "qemu-user".to_owned(),
        }
    }

    /// Finds `what` (as [`Target::cross_package`] names it) for this
    /// target, like [`crate::tool`]: a missing tool is a note that names
    /// its Debian package, or a panic under `HOD_REQUIRE_TOOLS=1`. For
    /// `sysroot`, the directory.
    pub fn tool(self, what: &str) -> Option<PathBuf> {
        let name = match what {
            "sysroot" => self.sysroot().display().to_string(),
            "gcc" => self.c_driver(),
            "g++" => self.cxx_driver(),
            "ld.bfd" => self.bfd(),
            "emulator" => self.emulator(),
            other => other.to_owned(),
        };
        let found = if what == "sysroot" {
            let root = self.sysroot();
            root.is_dir().then_some(root)
        } else {
            crate::find_tool(&name)
        };
        if found.is_none() {
            let hint = if self.is_host() {
                String::new()
            } else {
                format!(" (Debian package {})", self.cross_package(what))
            };
            assert!(!crate::tools_required(), "`{name}` not found{hint} (HOD_REQUIRE_TOOLS=1)");
            eprintln!("note: `{name}` not found{hint}; skipping the tool-backed part of this test");
        }
        found
    }

    fn tool_name(self, tool: &str) -> String {
        if self.is_host() { tool.to_owned() } else { format!("{}-{tool}", self.gnu_prefix()) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // spec/harness.md "Cross runs": the names of a target's tools.
    #[test]
    fn names_of_a_targets_tools() {
        let host = Target::host();
        for target in Target::ALL {
            let cross = Some(target) != host;
            assert_eq!(target.c_driver() == "gcc", !cross, "{target:?}");
            assert_eq!(target.cxx_driver() == "g++", !cross, "{target:?}");
            assert_eq!(target.bfd() == "ld.bfd", !cross, "{target:?}");
            if cross {
                assert_eq!(target.c_driver(), format!("{}-gcc", target.gnu_prefix()));
                assert_eq!(target.bfd(), format!("{}-ld.bfd", target.gnu_prefix()));
            }
        }
        assert_eq!(Target::X86_64.sysroot(), Path::new("/usr/x86_64-linux-gnu"));
        assert_eq!(Target::X86_64.emulator(), "qemu-x86_64");
        let args = Target::X86_64.emulator_args(Path::new("/sys/root"));
        assert_eq!(args, ["-cpu", X86_64_EMULATED_CPU, "-L", "/sys/root"]);
        assert!(!X86_64_EMULATED_CPU.contains("max"));
        // The `cpu` directive's features: a feature the model removes is
        // turned back on, one it lacks is added, each once.
        let args = Target::X86_64.emulator_args_with(Path::new("/r"), &["cx16", "popcnt", "cx16"]);
        assert_eq!(args, ["-cpu", "qemu64,-sse3,-lahf-lm,+cx16,+popcnt", "-L", "/r"]);
        assert_eq!(cpu_with_features("qemu64,+cx16", &["cx16"]), "qemu64,+cx16");
        let package = |what| Target::X86_64.cross_package(what);
        assert_eq!(package("gcc"), "gcc-x86-64-linux-gnu");
        assert_eq!(package("ld.bfd"), "binutils-x86-64-linux-gnu");
        assert_eq!(package("sysroot"), "libc6-dev-amd64-cross");
        assert_eq!(package("libstdc++"), "libstdc++-14-dev-amd64-cross");
        assert_eq!(package("emulator"), "qemu-user");
    }
}
