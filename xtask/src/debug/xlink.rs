//! Results a linker exception explains (`spec/harness.md` "debug",
//! "Linker exceptions"; WP-248).
//!
//! A case of [`linkers::EXCEPTIONS`] can break the ORACLE's session: the
//! program `llc`'s object makes, linked by that linker, does not meet a
//! check of the test, while the same object linked by every other linker
//! does. Without this module such a result is an ERROR (the test looks
//! broken) and fails the run. With it, a result is an XLINK when ALL of
//! these hold:
//!
//! 1. [`EXPECTED`] lists the (linker, case, target, test, debugger);
//! 2. the linker has that case in [`linkers::EXCEPTIONS`];
//! 3. `llc`'s session lacks a check;
//! 4. the case's witness ([`witness`]: a mechanical reading of the linked
//!    program) holds for `llc`'s program AND for hod's.
//!
//! Anything else stays what it was: the same test under the other
//! linkers, another test under this linker, a broken oracle session the
//! witness does not explain (ERROR), and hod's session whenever the
//! oracle's is usable (compared as usual). A listed case whose oracle
//! session meets every check is stale: hod's PASS becomes an XPASS that
//! names the entry, so it is removed when the linker is fixed.

use std::path::Path;

use hod_testutil::linkers::{self, Exception};

use super::lines::{self, LineTools};
use super::sessions::Outcome;

/// One test that a case of [`linkers::EXCEPTIONS`] breaks the oracle's
/// session of, on one target.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Expected {
    pub linker: &'static str,
    /// The case's name in [`linkers::EXCEPTIONS`].
    pub case: &'static str,
    /// The target's name (`aarch64`, `x86_64`).
    pub target: &'static str,
    /// The test's file name in `tests/debug`.
    pub test: &'static str,
    /// The debugger family (`gdb`, `lldb`: `lldb-19` and `lldb-22` too).
    pub debugger: &'static str,
}

/// Every (linker, case, target, test, debugger) measured to break the
/// oracle's session (`spec/landed/harness/WP-248.md`).
pub const EXPECTED: [Expected; 3] = [
    Expected {
        linker: "wild",
        case: "tls-dtpoff-debug",
        target: "x86_64",
        test: "tls_static.c",
        debugger: "gdb",
    },
    Expected {
        linker: "wild",
        case: "plt-got-gdb",
        target: "aarch64",
        test: "stack_protector.c",
        debugger: "gdb",
    },
    Expected {
        linker: "wild",
        case: "plt-got-gdb",
        target: "aarch64",
        test: "hardening_a64.cpp",
        debugger: "gdb",
    },
];

/// Where a session ran: the linker, the debugger (`gdb`, `lldb-19`), the
/// target's name and the test (a path or a file name).
#[derive(Clone, Copy, Debug)]
pub struct Session<'a> {
    pub linker: &'a str,
    pub debugger: &'a str,
    pub target: &'a str,
    pub test: &'a str,
}

/// The entry of [`EXPECTED`] for `session`, when the linker's exception
/// lists its case.
pub fn expected(session: Session<'_>) -> Option<(Expected, &'static Exception)> {
    let file = Path::new(session.test).file_name()?.to_str()?;
    let family = session.debugger.split('-').next().unwrap_or(session.debugger);
    let entry = EXPECTED.iter().find(|e| {
        e.linker == session.linker
            && e.target == session.target
            && e.test == file
            && e.debugger == family
    })?;
    Some((*entry, linkers::exception(session.linker, entry.case)?))
}

/// Whether `program` shows the case: what the linker wrote that the
/// exception's reason states, read with LLVM's tools.
///
/// # Errors
/// When a tool fails, or for a case this module has no witness of.
pub fn witness(case: &str, program: &Path, tools: &LineTools) -> Result<bool, String> {
    match case {
        "tls-dtpoff-debug" => {
            let info = lines::tool_output(&tools.dwarfdump, &["--debug-info"], program)?;
            Ok(tls_offset_from_thread_pointer(&info))
        }
        "plt-got-gdb" => {
            let headers = lines::tool_output(&tools.objdump, &["--section-headers"], program)?;
            Ok(plt_got_only(&headers))
        }
        other => Err(format!("no witness of the linker exception `{other}`")),
    }
}

/// `tls-dtpoff-debug`: a TLS location (`DW_OP_const8u <offset>,
/// DW_OP_GNU_push_tls_address` or `DW_OP_form_tls_address`, as
/// `llvm-dwarfdump --debug-info` prints it) whose offset is negative. An
/// offset in a module's TLS block (DTPOFF) never is; an x86-64 offset from
/// the thread pointer (TPOFF) always is.
pub fn tls_offset_from_thread_pointer(debug_info: &str) -> bool {
    debug_info.lines().any(|line| {
        let Some(at) = line.find("DW_OP_const8u 0x") else { return false };
        let rest = &line[at + "DW_OP_const8u 0x".len()..];
        let Some((digits, after)) = rest.split_once(',') else { return false };
        let tls = after.trim_start().starts_with("DW_OP_GNU_push_tls_address")
            || after.trim_start().starts_with("DW_OP_form_tls_address");
        tls && u64::from_str_radix(digits, 16).is_ok_and(|v| v >> 63 == 1)
    })
}

/// `plt-got-gdb`: the program has `.plt.got` and neither `.plt` nor
/// `.rela.plt` (`llvm-objdump --section-headers`): no PLT entry BFD's
/// synthetic symbols name on AArch64.
pub fn plt_got_only(section_headers: &str) -> bool {
    let has = |name: &str| {
        section_headers.lines().any(|l| l.split_whitespace().nth(1).is_some_and(|n| n == name))
    };
    has(".plt.got") && !has(".plt") && !has(".rela.plt")
}

/// What an expected case does to one result of hod's program.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// Not a case of [`EXPECTED`]: the result stands.
    Stands,
    /// The oracle's session is broken by the case: an XLINK.
    Excepted(String),
    /// The case is listed, `llc`'s session meets every check: stale.
    Stale(String),
}

/// The verdict for an [`EXPECTED`] case (`None`: none), given why `llc`'s
/// session is broken (`None`: it met every check) and whether the witness
/// holds for `llc`'s program and for hod's (`None`: not read, or a tool
/// failed).
pub fn verdict(
    case: Option<(Expected, &Exception)>,
    broken: Option<&str>,
    witnessed: Option<(bool, bool)>,
) -> Verdict {
    let Some((entry, exception)) = case else { return Verdict::Stands };
    let named = format!("{} {} exception `{}`", entry.linker, exception.version, entry.case);
    match (broken, witnessed) {
        (Some(why), Some((true, true))) => Verdict::Excepted(format!("{named}: {why}")),
        (Some(_), _) => Verdict::Stands,
        (None, _) => Verdict::Stale(format!(
            "stale {named}: llc's session meets every check (remove the entry from \
             xtask/src/debug/xlink.rs EXPECTED)"
        )),
    }
}

/// Applies `verdict` to hod's result `(outcome, detail)` when `llc`'s
/// session is broken because `why` (`None`: it is not).
pub fn apply(
    verdict: &Verdict,
    why: Option<&str>,
    outcome: Outcome,
    detail: String,
) -> (Outcome, String) {
    match (verdict, why) {
        (Verdict::Excepted(named), _) => {
            (Outcome::Xlink, format!("{named} (hod's: {})", outcome.name()))
        }
        (_, Some(why)) => (Outcome::Error, format!("{why} (hod's: {})", outcome.name())),
        (Verdict::Stale(stale), None) if outcome == Outcome::Pass => {
            (Outcome::Xpass, stale.clone())
        }
        (Verdict::Stale(stale), None) => (outcome, format!("{detail}; {stale}")),
        (Verdict::Stands, None) => (outcome, detail),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WILD_TLS: &str = "\
0x00000023:   DW_TAG_variable
                DW_AT_name\t(\"global_ticks\")
                DW_AT_location\t(DW_OP_const8u 0xfffffffffffffff8, DW_OP_GNU_push_tls_address)
";
    const BFD_TLS: &str = "\
                DW_AT_location\t(DW_OP_const8u 0x0, DW_OP_GNU_push_tls_address)
                DW_AT_location\t(DW_OP_const8u 0x4, DW_OP_GNU_push_tls_address)
                DW_AT_location\t(DW_OP_const8u 0xfffffffffffffff8)
";

    #[test]
    fn the_tls_witness_is_a_negative_tls_offset() {
        assert!(tls_offset_from_thread_pointer(WILD_TLS));
        // DTPOFF offsets, and a negative constant that is not a TLS address.
        assert!(!tls_offset_from_thread_pointer(BFD_TLS));
    }

    #[test]
    fn the_plt_witness_is_a_plt_got_alone() {
        let header = "Sections:\nIdx Name          Size     VMA              Type\n";
        let wild = format!("{header} 13 .plt.got      00000080 0000000000010960 TEXT\n");
        let mold = format!(
            "{header}  9 .rela.plt     00000090 0000000000000640\n 11 .plt          000000a0 \
             0000000000000720 TEXT\n 12 .plt.got      00000010 0000000000000800 TEXT\n"
        );
        assert!(plt_got_only(&wild));
        assert!(!plt_got_only(&mold));
        assert!(!plt_got_only(header));
    }

    fn at(
        linker: &str,
        debugger: &str,
        target: &str,
        test: &str,
    ) -> Option<(Expected, &'static Exception)> {
        expected(Session { linker, debugger, target, test })
    }

    #[test]
    fn only_listed_cases_of_the_exceptions_are_expected() {
        let tls = "tests/debug/tls_static.c";
        let (entry, exception) = at("wild", "gdb", "x86_64", tls).unwrap();
        assert_eq!((entry.case, exception.case), ("tls-dtpoff-debug", "tls-dtpoff-debug"));
        assert!(at("wild", "gdb", "aarch64", tls).is_none());
        assert!(at("bfd", "gdb", "x86_64", tls).is_none());
        assert!(at("wild", "lldb-22", "x86_64", tls).is_none());
        assert!(at("wild", "gdb", "x86_64", "tests/debug/stepping.c").is_none());
        assert!(at("wild", "gdb", "aarch64", "hardening_a64.cpp").is_some());
        // lldb steps over wild's PLT stubs: the case is gdb's only.
        assert!(at("wild", "lldb-19", "aarch64", "hardening_a64.cpp").is_none());
        for entry in EXPECTED {
            assert!(linkers::exception(entry.linker, entry.case).is_some(), "{entry:?}");
            let none = Path::new("/nonexistent");
            let tools = LineTools { dwarfdump: none.into(), nm: none.into(), objdump: none.into() };
            // Every listed case has a witness (the tool fails, the case is known).
            let err = witness(entry.case, none, &tools).unwrap_err();
            assert!(!err.contains("no witness"), "{err}");
        }
    }

    #[test]
    fn an_excepted_error_is_an_xlink_and_nothing_else_is() {
        let case = at("wild", "gdb", "x86_64", "tls_static.c");
        let why = "llc's session lacks the check '$1 = 6'";
        // Broken by the case, witnessed on both programs: XLINK.
        let v = verdict(case, Some(why), Some((true, true)));
        let (outcome, detail) = apply(&v, Some(why), Outcome::Pass, String::new());
        assert_eq!(outcome, Outcome::Xlink);
        assert!(detail.contains("`tls-dtpoff-debug`") && detail.contains(why), "{detail}");
        assert!(detail.ends_with("(hod's: PASS)"), "{detail}");
        // The witness absent on either program: still an ERROR.
        for witnessed in [Some((true, false)), Some((false, true)), None] {
            let v = verdict(case, Some(why), witnessed);
            assert_eq!(v, Verdict::Stands);
            assert_eq!(apply(&v, Some(why), Outcome::Pass, String::new()).0, Outcome::Error);
        }
        // Not listed (another linker, target or test): an ERROR.
        let v = verdict(at("lld", "gdb", "x86_64", "tls_static.c"), Some(why), Some((true, true)));
        assert_eq!(apply(&v, Some(why), Outcome::Fail, String::new()).0, Outcome::Error);
        // Not listed and the oracle usable: hod's result stands.
        let (outcome, detail) = apply(&Verdict::Stands, None, Outcome::Fail, "line 3".into());
        assert_eq!((outcome, detail.as_str()), (Outcome::Fail, "line 3"));
    }

    #[test]
    fn a_listed_case_that_passes_is_reported_stale() {
        let case = at("wild", "gdb", "aarch64", "stack_protector.c");
        let v = verdict(case, None, None);
        let Verdict::Stale(ref text) = v else { panic!("{v:?}") };
        assert!(text.contains("stale wild") && text.contains("`plt-got-gdb`"), "{text}");
        let (outcome, detail) = apply(&v, None, Outcome::Pass, String::new());
        assert_eq!((outcome, detail.as_str()), (Outcome::Xpass, text.as_str()));
        // hod's own difference is not hidden by the stale entry.
        let (outcome, detail) = apply(&v, None, Outcome::Fail, "line 3".into());
        assert_eq!(outcome, Outcome::Fail);
        assert!(detail.starts_with("line 3; stale"), "{detail}");
    }
}
