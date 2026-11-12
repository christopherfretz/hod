//! `spec/eh.md` §9.9: real unwinding through hod's LSDAs.
//! `tests/data/lsda.ll` (C++ compiled by clang 19 at `-O0`; the source is
//! `tests/data/lsda.cpp`) catches by type after a handler that does not
//! match, falls through non-matching inner handlers to an outer one with a
//! cleanup on the way, rethrows out of a handler, catches all, passes an
//! exception through a satisfied exception specification, and, on request,
//! violates an exception specification or `noexcept`. `llc` compiles it;
//! its `.gcc_except_table` sections are replaced by the ones `write_lsdas`
//! writes from the decoded rows, and its `.eh_frame` by the one
//! `write_eh_frame` writes with each FDE pointing at hod's LSDA (every
//! other byte of the object unchanged). Linked with GNU ld, lld and mold
//! with `--eh-frame-hdr`, the program must print what the original prints
//! and exit as it does; with a deliberately wrong table it must not.

mod common;

use std::ffi::OsString;
use std::path::Path;

use common::lsda::{Rows, lsda_trip};
use common::{compile, linkers, llc, round_trip};
use hod_obj::Arch;
use hod_testutil::{Outcome, Scratch, run, tool};

const IR: &str = include_str!("data/lsda.ll");
const EXPECTED: &str = "caught int 1\ncleanup inner\ncaught Base 2\nrethrowing 3\n\
cleanup handler\ncaught all\ncleanup allowed\ncaught Error 4 through a filter\n\
caught int 5 in two_pads\ncaught Error 5 in two_pads\ndone\n";

/// The scenarios: the program's argument and what the original prints.
const RUNS: [(&str, &str); 3] = [
    ("", EXPECTED),
    ("filter", "violating a filter\n"),
    ("noexcept", "throwing out of noexcept\n"),
];

fn link(driver: &Path, flags: &[OsString], object: &Path, program: &Path) {
    let mut args: Vec<OsString> = flags.to_vec();
    args.push(object.into());
    args.push("-Wl,--eh-frame-hdr".into());
    args.push("-o".into());
    args.push(program.into());
    let linked = run(driver, &args);
    assert!(linked.success(), "link {}: {}", program.display(), linked.stderr_text());
}

fn run_program(program: &Path, arg: &str) -> Outcome {
    let args: Vec<&str> = if arg.is_empty() { Vec::new() } else { vec![arg] };
    run(program, args)
}

/// The exit as a comparable value: a code, or the signal that ended it.
fn exit(o: &Outcome) -> String {
    use std::os::unix::process::ExitStatusExt;
    match o.status {
        Some(s) => s.code().map_or_else(|| format!("signal {:?}", s.signal()), |c| c.to_string()),
        None => "timed out".to_string(),
    }
}

/// llc's object with hod's LSDAs and `.eh_frame` in it; `mutate` changes
/// the rows first.
fn hod_object(
    scratch: &Scratch,
    label: &str,
    object: &[u8],
    mutate: &dyn Fn(&str, &mut Rows),
    check: bool,
) -> Option<Vec<u8>> {
    let t = lsda_trip(label, Arch::AArch64, IR, object, false, false, mutate, check);
    if check {
        assert!(t.identical, "{label}: hod's LSDAs are llc's");
        eprintln!("{label}: {} LSDAs, {} call sites, byte-identical", t.functions, t.sites);
    }
    let lsda_of = |name: &str| t.lsdas[name];
    let rt = round_trip(scratch, label, Arch::AArch64, IR, &t.patched, false, Some(&lsda_of))?;
    Some(rt.patched)
}

fn unwind(major: u32) {
    if !cfg!(target_arch = "aarch64") {
        eprintln!("note: the unwinding programs are AArch64; skipping on this host");
        return;
    }
    let Some(llc) = llc(major) else { return };
    let Some(driver) = tool("g++") else { return };
    let scratch = Scratch::new(&format!("lsda-unwind{major}"));
    let label = format!("lsda-{major}");
    let object = compile(&llc, &scratch, &label, IR, "aarch64-unknown-linux-gnu");
    let Some(ours) = hod_object(&scratch, &label, &object, &|_, _| {}, true) else { return };
    // The wrong table: `two_pads`' first call lands on the second pad,
    // whose handler catches `Error`, not the `int` thrown.
    let wrong_pad = |name: &str, rows: &mut Rows| {
        if name == "_Z8two_padsi" {
            let first = rows.call_sites.iter().position(|c| c.pad != hod_eh::NO_PAD).unwrap();
            let other = (0..rows.pads.len() as u32)
                .find(|&p| p != rows.call_sites[first].pad)
                .expect("two pads");
            rows.call_sites[first].pad = other;
            rows.call_sites[first].landing = 0;
        }
    };
    let Some(wrong) = hod_object(&scratch, &format!("{label}-wrong"), &object, &wrong_pad, false)
    else {
        return;
    };
    let original = scratch.write(&format!("{label}.orig.o"), &object);
    let patched = scratch.write(&format!("{label}.hod.o"), &ours);
    let wrong = scratch.write(&format!("{label}.wrong.o"), &wrong);
    let mut report = Vec::new();
    for (name, flags) in linkers(&scratch) {
        let tag = format!("{label}-{name}");
        let programs =
            [("orig", &original), ("hod", &patched), ("wrong", &wrong)].map(|(kind, object)| {
                let program = scratch.join(format!("{tag}.{kind}"));
                link(&driver, &flags, object, &program);
                program
            });
        for (arg, expected) in RUNS {
            let [want, got, bad] = programs.each_ref().map(|p| run_program(p, arg));
            assert_eq!(want.stdout_text(), expected, "{tag} {arg}: the original program");
            assert_eq!(got.stdout_text(), want.stdout_text(), "{tag} {arg}: {}", got.stderr_text());
            assert_eq!(exit(&got), exit(&want), "{tag} {arg}: exit");
            assert_eq!(got.stderr_text(), want.stderr_text(), "{tag} {arg}: stderr");
            if arg.is_empty() {
                assert!(want.success(), "{tag}: the original program exits 0");
                assert_ne!(
                    (bad.stdout_text(), exit(&bad)),
                    (want.stdout_text(), exit(&want)),
                    "{tag}: the wrong table must change what the program does"
                );
                report.push(format!("{tag}: as the original; wrong table: exit {}", exit(&bad)));
            } else {
                assert!(!want.success(), "{tag} {arg}: the original terminates");
                report.push(format!("{tag} {arg}: terminates as the original ({})", exit(&got)));
            }
        }
    }
    eprintln!("{}", report.join("\n"));
}

/// §9.9 with LLVM 19.
#[test]
fn llc19_objects_unwind_through_hod_lsdas() {
    unwind(19);
}

/// §9.9 with LLVM 22.
#[test]
fn llc22_objects_unwind_through_hod_lsdas() {
    unwind(22);
}
