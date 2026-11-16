//! The linker matrix (`docs/VALIDATION.md` "Linker matrix",
//! `spec/harness.md` "Linkers"): GNU ld, lld, mold, wild and mortar, found
//! by a stable name, and the compiler-driver flags that make `gcc` or `g++`
//! link with one of them.
//!
//! GNU ld, lld and mold are found like any tool ([`crate::tool`]); wild is
//! not packaged by Debian and is a pinned release ([`WILD_VERSION`]) found
//! by [`find_wild`]. It is optional even under `HOD_REQUIRE_TOOLS=1` until
//! every hosted lane is proven to install it ([`WILD_REQUIRED`]). mortar
//! is built from a pinned commit of its repository ([`MORTAR_COMMIT`]),
//! found by [`find_mortar`], and optional in the same way
//! ([`MORTAR_REQUIRED`]); the output kinds it refuses by name are
//! [`REFUSALS`].

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::Target;

/// The names of the matrix's linkers, in report order.
pub const MATRIX: [&str; 5] = ["bfd", "lld", "mold", "wild", "mortar"];

/// The pinned commit of mortar (`https://github.com/christopherfretz/mortar`,
/// `docs/status/linkers.md`): the provisioning script builds this commit
/// (`cargo build --release -p mortar`) into `.toolchain/mortar/<commit>/`.
pub const MORTAR_COMMIT: &str = "ea9b94f42743d6772d690a374b5bf976e415f812";

/// Whether a missing mortar fails a test under `HOD_REQUIRE_TOOLS=1`.
/// False (WP-281): the hosted lanes build it since WP-310
/// (`.github/actions/mortar`, `HOD_MORTAR`), but it stays optional until
/// a hosted run has shown that step works on both runners, as wild's.
pub const MORTAR_REQUIRED: bool = false;

/// The variable that names the mortar executable, over every other place.
pub const MORTAR_VARIABLE: &str = "HOD_MORTAR";

/// Finds mortar without a note, as [`find_wild`] finds wild:
///
/// 1. `HOD_MORTAR`, the path of the executable (set to a path that is not a
///    file, it panics);
/// 2. `.toolchain/mortar/<MORTAR_COMMIT>/mortar` in the checkout or in any
///    directory above it;
/// 3. `mortar`, then `ld.mortar`, on `PATH`.
///
/// `HOD_MORTAR` set to the empty string skips 1 and 2.
///
/// # Panics
/// When `HOD_MORTAR` is set and names no file.
pub fn find_mortar() -> Option<PathBuf> {
    let on_path = || crate::find_tool("mortar").or_else(|| crate::find_tool("ld.mortar"));
    match std::env::var_os(MORTAR_VARIABLE) {
        Some(named) if named.is_empty() => on_path(),
        Some(named) => {
            let path = PathBuf::from(named);
            assert!(path.is_file(), "{MORTAR_VARIABLE}={} is not a file", path.display());
            Some(path)
        }
        None => pinned_mortar(Path::new(env!("CARGO_MANIFEST_DIR"))).or_else(on_path),
    }
}

/// `<dir>/.toolchain/mortar/<MORTAR_COMMIT>/mortar` for the first of
/// `start` and its ancestors that has it.
pub fn pinned_mortar(start: &Path) -> Option<PathBuf> {
    start
        .ancestors()
        .map(|dir| dir.join(".toolchain/mortar").join(MORTAR_COMMIT).join("mortar"))
        .find(|candidate| candidate.is_file())
}

/// The short form of [`MORTAR_COMMIT`] that notes and reasons show.
pub const MORTAR_SHORT: &str = "ea9b94f";

/// What a linker of the matrix refuses by name: an output kind, an option
/// or an input feature. The link is expected to exit non-zero printing
/// `refusal`, and the cell is recorded as REFUSED, neither a failure nor
/// a silent skip.
#[derive(Clone, Copy, Debug)]
pub struct Refusal {
    /// The linker of [`MATRIX`].
    pub linker: &'static str,
    /// The version (for mortar, the commit) the refusal was observed with.
    pub version: &'static str,
    /// What is refused: a driver or linker flag (`-shared`, `-r`,
    /// `--section-start`), or with `input` the stable name of an input
    /// feature a test asks [`input_refusal`] for.
    pub flag: &'static str,
    /// Whether `flag` names an input feature rather than a flag.
    pub input: bool,
    /// The text the linker prints on standard error, which is checked.
    pub refusal: &'static str,
    /// Empty for a kind the linker documents as refused; otherwise the
    /// finding that reports it to the linker's maintainers
    /// (`docs/status/linkers.md` "Findings", WP-281).
    pub finding: &'static str,
}

/// Everything a linker refuses by name (`docs/status/linkers.md`
/// "Refused kinds" and "Findings").
pub const REFUSALS: [Refusal; 4] = [
    Refusal {
        linker: "mortar",
        version: MORTAR_SHORT,
        flag: "-shared",
        input: false,
        refusal: "unsupported option: -shared",
        finding: "",
    },
    Refusal {
        linker: "mortar",
        version: MORTAR_SHORT,
        flag: "-r",
        input: false,
        refusal: "unsupported option: -r",
        finding: "",
    },
    Refusal {
        linker: "mortar",
        version: MORTAR_SHORT,
        flag: "--section-start",
        input: false,
        refusal: "unsupported option: --section-start",
        finding: "section-start-option",
    },
    Refusal {
        linker: "mortar",
        version: MORTAR_SHORT,
        flag: "compressed-debug-input",
        input: true,
        refusal: "unsupported feature: SHF_COMPRESSED input sections",
        finding: "compressed-debug-input",
    },
];

/// The refusal of `linker` that a link with `flags` meets (a flag of
/// [`REFUSALS`], alone, as `-Wl,<flag>`, or either with `=<value>`), if
/// any.
pub fn refusal(linker: &str, flags: &[&str]) -> Option<&'static Refusal> {
    let names = |flag: &str, wanted: &str| {
        let flag = flag.strip_prefix("-Wl,").unwrap_or(flag);
        flag == wanted || flag.strip_prefix(wanted).is_some_and(|rest| rest.starts_with('='))
    };
    REFUSALS
        .iter()
        .find(|r| r.linker == linker && !r.input && flags.iter().any(|f| names(f, r.flag)))
}

/// The refusal of `linker` for the input feature `case`, if any.
pub fn input_refusal(linker: &str, case: &str) -> Option<&'static Refusal> {
    REFUSALS.iter().find(|r| r.linker == linker && r.input && r.flag == case)
}

/// Checks the link of a REFUSED cell and prints that it was recorded: it
/// must have failed and printed the refusal. A link that succeeds panics
/// (the kind is now supported: the [`REFUSALS`] row is to be deleted and
/// the cell enabled), and so does a failure with another diagnostic.
///
/// # Panics
/// As said.
pub fn check_refused(refusal: &Refusal, succeeded: bool, stderr: &str, cell: &str) {
    assert!(
        !succeeded,
        "{} {} now links `{}` ({cell}): delete its row of linkers::REFUSALS and enable the cell",
        refusal.linker, refusal.version, refusal.flag
    );
    assert!(
        stderr.contains(refusal.refusal),
        "{} {} failed `{}` ({cell}) without its refusal `{}`:\n{stderr}",
        refusal.linker,
        refusal.version,
        refusal.flag,
        refusal.refusal
    );
    let finding = if refusal.finding.is_empty() { "documented" } else { refusal.finding };
    eprintln!(
        "note: {} {}: REFUSED `{}` ({cell}): {} [{finding}]",
        refusal.linker, refusal.version, refusal.flag, refusal.refusal
    );
}

/// The pinned release of wild (`docs/status/linkers.md`): the
/// provisioning script and the hosted set-up install this version.
pub const WILD_VERSION: &str = "0.11.0";

/// Whether a missing wild fails a test under `HOD_REQUIRE_TOOLS=1`. False
/// until a hosted run has shown the set-up installs it on every lane.
pub const WILD_REQUIRED: bool = false;

/// The variable that names the wild executable, over every other place.
pub const WILD_VARIABLE: &str = "HOD_WILD";

/// Finds wild without a note:
///
/// 1. `HOD_WILD`, the path of the executable; set to a path that is not a
///    file, it is a misconfiguration and panics rather than skip;
/// 2. `.toolchain/wild/<WILD_VERSION>/wild` in the checkout or in any
///    directory above it (the workspace that holds hod as a submodule,
///    where the provisioning script installs it);
/// 3. `wild`, then `ld.wild`, on `PATH`.
///
/// `HOD_WILD` set to the empty string skips 1 and 2: `PATH` alone decides
/// (the harness's tests with fake tools on a `PATH` of their own).
///
/// # Panics
/// When `HOD_WILD` is set and names no file.
pub fn find_wild() -> Option<PathBuf> {
    let on_path = || crate::find_tool("wild").or_else(|| crate::find_tool("ld.wild"));
    match std::env::var_os(WILD_VARIABLE) {
        Some(named) if named.is_empty() => on_path(),
        Some(named) => {
            let path = PathBuf::from(named);
            assert!(path.is_file(), "{WILD_VARIABLE}={} is not a file", path.display());
            Some(path)
        }
        None => pinned_wild(Path::new(env!("CARGO_MANIFEST_DIR"))).or_else(on_path),
    }
}

/// `<dir>/.toolchain/wild/<WILD_VERSION>/wild` for the first of `start`
/// and its ancestors that has it.
pub fn pinned_wild(start: &Path) -> Option<PathBuf> {
    start
        .ancestors()
        .map(|dir| dir.join(".toolchain/wild").join(WILD_VERSION).join("wild"))
        .find(|candidate| candidate.is_file())
}

/// Finds the program of the matrix's linker `name` for `target`, like
/// [`crate::tool`]: GNU ld is the target's (`ld.bfd` or the cross
/// `<prefix>-ld.bfd`); lld, mold and wild are one program for every target
/// (each takes the target from the driver's `-m <emulation>`). A missing
/// wild is a note, never a failure, while [`WILD_REQUIRED`] is false.
///
/// # Panics
/// For a name outside [`MATRIX`].
pub fn find_linker(target: Target, name: &str) -> Option<PathBuf> {
    match name {
        "bfd" if target.is_host() => crate::tool("ld.bfd"),
        "bfd" => target.tool("ld.bfd"),
        "lld" => crate::tool("ld.lld"),
        "mold" => crate::tool("ld.mold"),
        "wild" => {
            let found = find_wild();
            if found.is_none() {
                assert!(
                    !(WILD_REQUIRED && crate::tools_required()),
                    "required linker wild {WILD_VERSION} not found (HOD_REQUIRE_TOOLS=1)"
                );
                eprintln!(
                    "note: wild {WILD_VERSION} not found ({WILD_VARIABLE}, .toolchain/wild, \
                     PATH); the matrix runs without it"
                );
            }
            found
        }
        "mortar" => {
            let found = find_mortar();
            if found.is_none() {
                assert!(
                    !(MORTAR_REQUIRED && crate::tools_required()),
                    "required linker mortar {MORTAR_SHORT} not found (HOD_REQUIRE_TOOLS=1)"
                );
                eprintln!(
                    "note: mortar {MORTAR_SHORT} not found ({MORTAR_VARIABLE}, .toolchain/mortar, \
                     PATH); the matrix runs without it"
                );
            }
            found
        }
        other => panic!("`{other}` is not a linker of the matrix ({MATRIX:?})"),
    }
}

/// The matrix's linkers that are present for `target`, each with its
/// program, in [`MATRIX`] order. A linker that does not link the target at
/// all ([`links`]) is left out, with a note, without being looked for.
pub fn present(target: Target) -> Vec<(&'static str, PathBuf)> {
    MATRIX
        .into_iter()
        .filter(|name| links(name, target))
        .filter_map(|name| Some((name, find_linker(target, name)?)))
        .collect()
}

/// Whether the matrix's linker `name` links programs of `target`: every
/// one does but wild and mortar for i686 ([`EXCEPTIONS`], case `elf-i386`,
/// after a note naming it).
pub fn links(name: &str, target: Target) -> bool {
    !(target == Target::I686 && exception(name, "elf-i386").is_some())
}

/// The compiler-driver flags that make `gcc` or `g++` link with `program`
/// as the linker `name`, whatever the program is called and wherever it
/// is installed. Each linker gets a directory `<base>/use-<name>` (created
/// if needed) passed with `-B`, which the driver searches first:
///
/// - GNU ld, lld and mold: it holds the program as `ld.<name>`, selected
///   by `-fuse-ld=<name>`;
/// - wild: GCC 14's driver refuses `-fuse-ld=wild`, so the directory holds
///   it as `ld`, the name the driver runs without `-fuse-ld`; and
///   [`WILD_FLAGS`] follow;
/// - mortar: likewise as `ld` (no driver knows `-fuse-ld=mortar`); it
///   keeps unused sections by default, so nothing follows.
///
/// # Panics
/// When the directory or the link cannot be created.
pub fn driver_flags(base: &Path, name: &str, program: &Path) -> Vec<OsString> {
    let dir = base.join(format!("use-{name}"));
    std::fs::create_dir_all(&dir).unwrap();
    let as_ld = matches!(name, "wild" | "mortar");
    let file = if as_ld { "ld".to_owned() } else { format!("ld.{name}") };
    let link = dir.join(file);
    if std::fs::symlink_metadata(&link).is_err() {
        std::os::unix::fs::symlink(program, &link).unwrap();
    }
    let mut prefix = OsString::from("-B");
    prefix.push(&dir);
    if name == "wild" {
        std::iter::once(prefix).chain(WILD_FLAGS.iter().map(OsString::from)).collect()
    } else if as_ld {
        vec![prefix]
    } else {
        vec![prefix, format!("-fuse-ld={name}").into()]
    }
}

/// The driver flags wild is always given: wild discards unused sections
/// unless told not to, where GNU ld, lld and mold keep them unless told
/// to (`--gc-sections`). With `--no-gc-sections` first, the four link the
/// same inputs into the same set of sections, and a test that asks for
/// `-Wl,--gc-sections` still gets it (the last of the two wins).
pub const WILD_FLAGS: [&str; 1] = ["-Wl,--no-gc-sections"];

/// Why a linker of the matrix is excepted from one case of a test
/// (`docs/status/linkers.md` "Exceptions"; the triage of WP-239).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Class {
    /// (b) The linker's bug, or a difference from the other linkers that
    /// the ABI allows; a reproduction from objects hod did not write is
    /// kept with the status page.
    Linker,
    /// (c) A feature the linker says it does not support.
    Unsupported,
}

/// One case one linker is excepted from.
#[derive(Clone, Copy, Debug)]
pub struct Exception {
    /// The linker of [`MATRIX`].
    pub linker: &'static str,
    /// The version the exception was observed with.
    pub version: &'static str,
    /// The case's stable name, which the test asks [`exception`] for.
    pub case: &'static str,
    pub class: Class,
    pub reason: &'static str,
}

/// Every exception of the matrix. A test skips or changes ONE assertion for
/// a case listed here, never the test, so a linker that regresses on
/// anything else still fails it.
pub const EXCEPTIONS: [Exception; 9] = [
    Exception {
        linker: "mortar",
        version: MORTAR_SHORT,
        case: "elf-i386",
        class: Class::Unsupported,
        reason: "for i386 only `-static` output is linked: a dynamically linked executable \
                 (PIE or not) is refused with \"unsupported feature: dynamically linked output \
                 on i386\" and `-static-pie` with \"position-independent executable output\", \
                 and the i686 matrix's tests link dynamic programs, so it is GNU ld, lld and mold",
    },
    Exception {
        linker: "wild",
        version: WILD_VERSION,
        case: "elf-i386",
        class: Class::Unsupported,
        reason: "32-bit x86 is not linked at all: `-m elf_i386` is refused with \"Emulation \
                 'elf_i386' is not yet supported\" (design 0008 section 4.1), so the i686 \
                 matrix is GNU ld, lld and mold",
    },
    Exception {
        linker: "wild",
        version: WILD_VERSION,
        case: "plt-names",
        class: Class::Linker,
        reason: "on x86-64 wild's PLT is `.plt.got` entries (endbr64, bnd jmp through a \
                 GLOB_DAT slot) that llvm-objdump 22 names five bytes off (a call to \
                 __tls_get_addr shows as `__cxa_finalize@plt+0xb`; GNU objdump names it), so \
                 calls through it cannot be counted by name",
    },
    Exception {
        linker: "wild",
        version: WILD_VERSION,
        case: "section-start",
        class: Class::Linker,
        reason: "--section-start is not honored for .rodata, .got, .init_array, the PLT or the \
                 frame index (.text, .data and .bss move as asked), and x86-64 .ltext is placed \
                 after the large data: AArch64's far layout cannot move .rodata, and no x86-64 \
                 far layout of the large code model links (frame_ptr overflows)",
    },
    Exception {
        linker: "wild",
        version: WILD_VERSION,
        case: "link-order",
        class: Class::Linker,
        reason: "SHF_LINK_ORDER sections are laid out in input order rather than in the order \
                 of the sections they are linked to, and --gc-sections discards them even when \
                 that section is kept (GNU ld and lld do both)",
    },
    Exception {
        linker: "wild",
        version: WILD_VERSION,
        case: "merge-constants",
        class: Class::Linker,
        reason: "SHF_MERGE sections without SHF_STRINGS (.rodata.cst8) are not merged; merging \
                 is allowed, not required, and GNU ld, lld and mold do it",
    },
    Exception {
        linker: "wild",
        version: WILD_VERSION,
        case: "z-text",
        class: Class::Unsupported,
        reason: "`-z text`, which GCC's driver passes for -static-pie, is warned about as \
                 \"not yet supported\" (the link succeeds and the program runs)",
    },
    Exception {
        linker: "wild",
        version: WILD_VERSION,
        case: "tls-dtpoff-debug",
        class: Class::Linker,
        reason: "on x86-64 an R_X86_64_DTPOFF64 in .debug_info (the operand of \
                 `DW_OP_const8u, DW_OP_GNU_push_tls_address`, a _Thread_local variable's \
                 location) is written as the offset from the thread pointer (-8, -4) rather \
                 than the offset in the module's TLS block (0, 4, as GNU ld, lld and mold \
                 write), so gdb prints the word 8 bytes before the variable (WP-248)",
    },
    Exception {
        linker: "wild",
        version: WILD_VERSION,
        case: "plt-got-gdb",
        class: Class::Linker,
        reason: "on AArch64 every PLT entry is a `.plt.got` stub through a GLOB_DAT slot \
                 (BIND_NOW, no .plt and no .rela.plt), which the ABI allows but BFD's \
                 synthetic symbols (gdb's `<name>@plt`) do not name on AArch64, so gdb's \
                 `next` and `nexti` over a call to a shared library stop in `?? ()` (WP-248)",
    },
];

/// The exception of `linker` for `case`, if there is one, after a note
/// naming it (a test's log shows every assertion it did not make).
pub fn exception(linker: &str, case: &str) -> Option<&'static Exception> {
    let found = EXCEPTIONS.iter().find(|e| e.linker == linker && e.case == case)?;
    eprintln!(
        "note: {linker} {}: case `{case}` excepted ({:?}): {}",
        found.version, found.class, found.reason
    );
    Some(found)
}

/// The warning `linker` prints for a case of [`EXCEPTIONS`] that a link
/// with `flags` meets, which a test that requires a silent link accepts:
/// wild's about `-z text` for `-static-pie`.
pub fn tolerated_warning(linker: &str, flags: &[&str]) -> Option<&'static str> {
    (linker == "wild" && flags.contains(&"-static-pie") && exception(linker, "z-text").is_some())
        .then_some("wild: warning: -z text is not yet supported\n")
}

/// Which linker of the matrix signed a linked program's `.comment` text
/// (as `readelf -p .comment` prints it): lld writes `LLD <version>`, mold
/// `mold <version>`, wild `Linker: Wild <version>`, mortar `mortar
/// <version>`; GNU ld signs nothing, so `None` is GNU ld's (or no linker's).
pub fn signer(comment: &str) -> Option<&'static str> {
    let mortar = comment
        .match_indices("mortar ")
        .any(|(at, m)| comment[at + m.len()..].starts_with(|c: char| c.is_ascii_digit()));
    if comment.contains("Linker: Wild ") {
        Some("wild")
    } else if mortar {
        Some("mortar")
    } else if comment.contains("mold ") {
        Some("mold")
    } else if comment.contains("LLD ") {
        Some("lld")
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signatures() {
        let wild = "  [     1]  GCC: (Debian 14.2.0-19) 14.2.0\n  [    20]  Linker: Wild 0.11.0 \
                    non-git-build (compatible with GNU linkers)";
        assert_eq!(signer(wild), Some("wild"));
        assert_eq!(signer("mold 2.37.1 (compatible with GNU ld)"), Some("mold"));
        assert_eq!(signer("Linker: LLD 22.1.8"), Some("lld"));
        assert_eq!(signer("GCC: (Debian 14.2.0-19) 14.2.0"), None);
        assert_eq!(signer("  [    20]  mortar 0.0.0"), Some("mortar"));
        assert_eq!(signer("mortar x"), None);
    }

    #[test]
    fn refusals_are_found_and_checked() {
        let shared = refusal("mortar", &["-shared"]).unwrap();
        assert_eq!(shared.flag, "-shared");
        assert_eq!(refusal("mortar", &["-nostdlib", "-Wl,-r"]).unwrap().flag, "-r");
        assert!(refusal("mortar", &["-static-pie"]).is_none());
        assert!(refusal("lld", &["-shared"]).is_none());
        let start = refusal("mortar", &["-no-pie", "-Wl,--section-start=.data=0x1000"]);
        assert_eq!(start.unwrap().finding, "section-start-option");
        assert!(refusal("mortar", &["-rdynamic"]).is_none(), "a prefix is not the flag");
        assert!(refusal("mortar", &["compressed-debug-input"]).is_none(), "an input, not a flag");
        assert!(input_refusal("mortar", "compressed-debug-input").is_some());
        assert!(input_refusal("mortar", "-shared").is_none());
        for (i, r) in REFUSALS.iter().enumerate() {
            assert!(MATRIX.contains(&r.linker), "{r:?}");
            let again = REFUSALS[i + 1..].iter().any(|o| o.linker == r.linker && o.flag == r.flag);
            assert!(!again, "{r:?} is listed twice");
        }
        check_refused(shared, false, "ld: error: unsupported option: -shared\n", "unit");
        let linked = std::panic::catch_unwind(|| check_refused(shared, true, "", "unit"));
        assert!(linked.is_err(), "a refused kind that links must fail the test");
        let other = std::panic::catch_unwind(|| check_refused(shared, false, "boom", "unit"));
        assert!(other.is_err(), "a failure without the refusal's name must fail the test");
    }

    #[test]
    fn the_pinned_mortar_is_found_above_the_start() {
        let scratch = crate::Scratch::new("mortar-pin");
        let dir = scratch.join(format!(".toolchain/mortar/{MORTAR_COMMIT}"));
        std::fs::create_dir_all(&dir).unwrap();
        let below = scratch.join("hod/crates/x");
        std::fs::create_dir_all(&below).unwrap();
        assert_ne!(pinned_mortar(&below), Some(dir.join("mortar")));
        std::fs::write(dir.join("mortar"), "").unwrap();
        assert_eq!(pinned_mortar(&below), Some(dir.join("mortar")));
        assert!(MORTAR_COMMIT.starts_with(MORTAR_SHORT));
    }

    #[test]
    fn mortar_is_selected_as_ld() {
        let scratch = crate::Scratch::new("mortar-flags");
        let program = scratch.write("prog", "");
        let flags = driver_flags(scratch.path(), "mortar", &program);
        let dir = scratch.join("use-mortar");
        assert_eq!(flags, [OsString::from(format!("-B{}", dir.display()))]);
        assert_eq!(std::fs::read_link(dir.join("ld")).unwrap(), program);
    }

    #[test]
    fn exceptions_name_linkers_of_the_matrix_once() {
        for (i, e) in EXCEPTIONS.iter().enumerate() {
            assert!(MATRIX.contains(&e.linker), "{e:?}");
            assert!(!e.reason.is_empty() && !e.version.is_empty(), "{e:?}");
            let again =
                EXCEPTIONS[i + 1..].iter().any(|o| o.linker == e.linker && o.case == e.case);
            assert!(!again, "{e:?} is listed twice");
        }
        assert!(exception("wild", "merge-constants").is_some());
        assert!(exception("lld", "merge-constants").is_none());
        assert!(tolerated_warning("wild", &["-static-pie"]).is_some());
        assert!(tolerated_warning("wild", &["-pie"]).is_none());
    }

    #[test]
    fn the_pinned_wild_is_found_above_the_start() {
        let scratch = crate::Scratch::new("wild-pin");
        let dir = scratch.join(format!(".toolchain/wild/{WILD_VERSION}"));
        std::fs::create_dir_all(&dir).unwrap();
        let below = scratch.join("hod/crates/x");
        std::fs::create_dir_all(&below).unwrap();
        // A directory above the scratch may hold the real one.
        assert_ne!(pinned_wild(&below), Some(dir.join("wild")));
        std::fs::write(dir.join("wild"), "").unwrap();
        assert_eq!(pinned_wild(&below), Some(dir.join("wild")));
    }

    #[test]
    fn wild_is_selected_as_ld_and_the_others_by_fuse_ld() {
        let scratch = crate::Scratch::new("wild-flags");
        let program = scratch.write("prog", "");
        let flags = driver_flags(scratch.path(), "wild", &program);
        let dir = scratch.join("use-wild");
        let selected = OsString::from(format!("-B{}", dir.display()));
        assert_eq!(flags, [selected, OsString::from("-Wl,--no-gc-sections")]);
        assert_eq!(std::fs::read_link(dir.join("ld")).unwrap(), program);
        let flags = driver_flags(scratch.path(), "mold", &program);
        assert_eq!(flags[1], "-fuse-ld=mold");
        assert!(scratch.join("use-mold/ld.mold").exists());
        // Asked again, the directory is reused.
        assert_eq!(driver_flags(scratch.path(), "mold", &program), flags);
    }
}
