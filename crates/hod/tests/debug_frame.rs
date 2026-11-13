//! `.debug_frame` end to end (WP-150, `spec/eh.md` §10): hod against
//! `llc` (19 and 22) for the IR of `tests/data/debug_frame/frames.c` by
//! clang 19, clang 22 and kiln++, on AArch64 and x86-64.
//!
//! - `-g` with `-fno-asynchronous-unwind-tables -fno-unwind-tables`
//!   (every function `nounwind` without `uwtable`): `.debug_frame` and no
//!   `.eh_frame` from both; the same CIE bytes, the same number of FDEs,
//!   each FDE's entry rule the same, and its bytes the same where its rows
//!   are, offsets included (the code is the same there); hod's rows are
//!   those of its own `.eh_frame` for the module with tables (which the
//!   unwinding tests check);
//! - the same module with one function that may throw: every FDE in
//!   `.eh_frame` (`llc` chooses the section once per module), none in
//!   `.debug_frame`;
//! - without `-g`: no CFI at all for such functions.
//!
//! hod's frames are not `llc`'s (its prologue and epilogue are its own: a
//! frame record in a leaf, rows for the epilogue), so for this source no
//! FDE has `llc`'s rows; the bytes of FDEs written from `llc`'s own rows
//! are `crates/hod_eh/tests/debug_frame.rs`'s. kiln++'s module has
//! functions that may throw: every FDE is in `.eh_frame`, from both.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use hod_testutil::{Scratch, run_with, tool, tools_required};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Arch {
    A64,
    X64,
}

const ARCHES: [Arch; 2] = [Arch::A64, Arch::X64];

impl Arch {
    fn triple(self) -> &'static str {
        match self {
            Self::A64 => "aarch64-unknown-linux-gnu",
            Self::X64 => "x86_64-unknown-linux-gnu",
        }
    }
}

fn output(command: &mut Command) -> String {
    let outcome = run_with(command, &[], Duration::from_mins(2));
    assert!(outcome.success(), "{command:?}: {}", outcome.stderr_text());
    outcome.stdout_text()
}

fn kiln() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("HOD_KILN").filter(|v| !v.is_empty()) {
        return Some(PathBuf::from(path));
    }
    let beside = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .ancestors()
        .nth(2)
        .map(|root| root.join("target-kiln").join("release").join("kiln++"));
    match beside {
        Some(path) if path.is_file() => Some(path),
        _ => {
            assert!(!tools_required(), "no kiln++ found; set HOD_KILN (HOD_REQUIRE_TOOLS=1)");
            None
        }
    }
}

/// A front end (its name, compiler, whether it reads C++) and its `llc`.
fn frontends() -> Vec<(&'static str, PathBuf, bool, PathBuf)> {
    let mut out = Vec::new();
    for (name, major) in [("c19", 19), ("c22", 22)] {
        if let (Some(cc), Some(llc)) =
            (tool(&format!("clang-{major}")), tool(&format!("llc-{major}")))
        {
            out.push((name, cc, false, llc));
        }
    }
    if let (Some(k), Some(llc)) = (kiln(), tool("llc-22")) {
        out.push(("kiln", k, true, llc));
    }
    out
}

fn source() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data/debug_frame/frames.c")
}

/// The `-O0` IR of the source.
fn ir(
    scratch: &Scratch,
    fe: &(&str, PathBuf, bool, PathBuf),
    arch: Arch,
    flags: &[&str],
) -> String {
    let tag = format!("{}-{arch:?}-{}", fe.0, flags.join("").replace(['-', '='], ""));
    let mut input = source();
    if fe.2 {
        input = scratch.write(&format!("{tag}.cpp"), std::fs::read_to_string(&input).unwrap());
    }
    let out = scratch.join(format!("{tag}.ll"));
    let mut command = Command::new(&fe.1);
    command.arg(format!("--target={}", arch.triple())).args(["-O0", "-S", "-emit-llvm", "-w"]);
    if fe.2 {
        command.arg("-std=c++20");
    }
    output(command.args(flags).arg(&input).arg("-o").arg(&out));
    std::fs::read_to_string(out).unwrap()
}

fn hod_object(scratch: &Scratch, arch: Arch, name: &str, text: &str) -> PathBuf {
    let parsed = hod_ll::parse_module(text.as_bytes(), &hod_ll::ParseOptions::default())
        .unwrap_or_else(|e| panic!("hod_ll: {e}"));
    let text_side = parsed.text;
    let mut module = parsed.module;
    let mut bodies = Vec::new();
    hod_ll::parse_bodies(&mut module, &text_side, |_, parsed| {
        bodies.push(parsed.body.expect("a body").clone());
        Ok::<(), hod_ll::LlError>(())
    })
    .unwrap_or_else(|e| panic!("hod_ll: {e}"));
    let bytes = match arch {
        Arch::A64 => hod_a64::compile_to_object(&module, &bodies, &hod_cg::Options::default())
            .unwrap_or_else(|e| panic!("{name}: {e:?}")),
        Arch::X64 => {
            let pic =
                hod_cg::Options { reloc_model: hod_cg::RelocModel::Pic, ..Default::default() };
            hod_x64::compile_to_object(&module, &bodies, &pic)
                .unwrap_or_else(|e| panic!("{name}: {e:?}"))
        }
    };
    scratch.write(&format!("{name}.o"), bytes)
}

/// `llc` at the oracle's flags.
fn llc_object(scratch: &Scratch, llc: &Path, arch: Arch, name: &str, text: &str) -> PathBuf {
    let input = scratch.write(&format!("{name}.ll"), text);
    let out = scratch.join(format!("{name}.o"));
    let flags: &[&str] = match arch {
        Arch::A64 => &["-O0", "-global-isel=0", "-fast-isel=0"],
        Arch::X64 => &["-O0", "-fast-isel=0", "-relocation-model=pic"],
    };
    output(Command::new(llc).args(flags).arg("-filetype=obj").arg(&input).arg("-o").arg(&out));
    out
}

/// One entry of a dump: whether a CIE, its header line's range, the
/// instructions and the rows.
#[derive(Debug, Default, Clone)]
struct Entry {
    cie: bool,
    offset: usize,
    length: usize,
    pc: (u64, u64),
    rows: Vec<String>,
}

/// The entries of `section` (`.debug_frame` or `.eh_frame`).
fn entries(dwarfdump: &Path, object: &Path, section: &str) -> Vec<Entry> {
    let text = output(Command::new(dwarfdump).args(["--debug-frame", "--eh-frame"]).arg(object));
    let mut out: Vec<Entry> = Vec::new();
    let mut inside = false;
    for line in text.lines() {
        if line.ends_with(" contents:") {
            inside = line.starts_with(section);
            continue;
        }
        if !inside || line.trim().is_empty() {
            continue;
        }
        if !line.starts_with(' ') {
            let f: Vec<&str> = line.split_whitespace().collect();
            if f.len() < 4 || !(f[3] == "CIE" || f[3] == "FDE") {
                continue;
            }
            let hex = |s: &str| usize::from_str_radix(s, 16).unwrap();
            let mut e = Entry {
                cie: f[3] == "CIE",
                offset: hex(f[0]),
                length: hex(f[1]),
                ..Entry::default()
            };
            if let Some(pc) = f.iter().find_map(|w| w.strip_prefix("pc=")) {
                let (a, b) = pc.split_once("...").unwrap();
                e.pc = (u64::from_str_radix(a, 16).unwrap(), u64::from_str_radix(b, 16).unwrap());
            }
            out.push(e);
            continue;
        }
        let t = line.trim();
        let row = (t.starts_with("0x") && t.contains(": CFA=")) || t.starts_with("CFA=");
        if row && let Some(e) = out.last_mut() {
            e.rows.push(t.to_owned());
        }
    }
    out
}

/// A row's rules without its code offset.
fn rules(row: &str) -> &str {
    row.split_once(": ").filter(|(a, _)| a.starts_with("0x")).map_or(row, |(_, r)| r)
}

/// The FDE's rows relative to its function's start.
fn relative(e: &Entry) -> Vec<String> {
    e.rows
        .iter()
        .map(|r| match r.split_once(": ") {
            Some((a, rest)) if a.starts_with("0x") => {
                let at = u64::from_str_radix(&a[2..], 16).unwrap();
                format!("{:#x}: {rest}", at - e.pc.0)
            }
            _ => r.clone(),
        })
        .collect()
}

/// The bytes of `section` (`llvm-readelf -x`).
fn section_bytes(readelf: &Path, object: &Path, section: &str) -> Vec<u8> {
    let text = output(Command::new(readelf).args(["-x", section]).arg(object));
    let mut out = Vec::new();
    for line in text.lines() {
        let t = line.trim_start();
        let Some(rest) = t.strip_prefix("0x") else { continue };
        let words: Vec<&str> = rest.split_whitespace().skip(1).take(4).collect();
        for w in words {
            if w.len() % 2 != 0 || !w.bytes().all(|b| b.is_ascii_hexdigit()) {
                break;
            }
            for k in (0..w.len()).step_by(2) {
                out.push(u8::from_str_radix(&w[k..k + 2], 16).unwrap());
            }
        }
    }
    out
}

fn has_section(readelf: &Path, object: &Path, name: &str) -> bool {
    output(Command::new(readelf).arg("-S").arg(object))
        .lines()
        .any(|l| l.split_whitespace().any(|w| w == name))
}

#[test]
fn debug_frame_matches_llc() {
    let (Some(dwarfdump), Some(readelf)) = (tool("llvm-dwarfdump"), tool("llvm-readelf")) else {
        return;
    };
    let scratch = Scratch::new("debug-frame-e2e");
    let no_tables = ["-g", "-fno-asynchronous-unwind-tables", "-fno-unwind-tables"];
    let (mut compared, mut fdes, mut same_rules, mut identical) = (0, 0, 0, 0);
    for fe in frontends() {
        for arch in ARCHES {
            let tag = format!("{}-{arch:?}", fe.0);
            let text = ir(&scratch, &fe, arch, &no_tables);
            assert!(!text.contains(" uwtable"), "{tag}");
            let hod = hod_object(&scratch, arch, &format!("{tag}-hod"), &text);
            let llc = llc_object(&scratch, &fe.3, arch, &format!("{tag}-llc"), &text);
            if fe.2 {
                // kiln++'s C++ functions that call may throw (no
                // `nounwind`): the module's FDEs are all in `.eh_frame`,
                // the `nounwind` ones included, as `llc` writes them.
                let defines = text.lines().filter(|l| l.starts_with("define ")).count();
                for object in [&hod, &llc] {
                    assert!(!has_section(&readelf, object, ".debug_frame"), "{}", object.display());
                    let n =
                        entries(&dwarfdump, object, ".eh_frame").iter().filter(|e| !e.cie).count();
                    assert_eq!(n, defines, "{}: every function's FDE", object.display());
                }
                compared += 1;
                continue;
            }
            for object in [&hod, &llc] {
                assert!(has_section(&readelf, object, ".debug_frame"), "{}", object.display());
                assert!(!has_section(&readelf, object, ".eh_frame"), "{}", object.display());
                let verify = run_with(
                    Command::new(&dwarfdump).arg("--verify").arg(object),
                    &[],
                    Duration::from_mins(2),
                );
                assert!(verify.success(), "{}: {}", object.display(), verify.stdout_text());
            }
            let (h, l) = (
                entries(&dwarfdump, &hod, ".debug_frame"),
                entries(&dwarfdump, &llc, ".debug_frame"),
            );
            assert_eq!(h.len(), l.len(), "{tag}: entries");
            assert!(h[0].cie && l[0].cie && h.iter().skip(1).all(|e| !e.cie), "{tag}: one CIE");
            let (hb, lb) = (
                section_bytes(&readelf, &hod, ".debug_frame"),
                section_bytes(&readelf, &llc, ".debug_frame"),
            );
            let cie = h[0].length + 4;
            assert_eq!(hb[..cie], lb[..cie], "{tag}: the CIE");
            for (a, b) in h.iter().zip(&l).skip(1) {
                // The rule at the function's entry (the CIE's) is the
                // same; the frame after it is each code's own.
                assert_eq!(
                    a.rows.first().map(|r| rules(r)),
                    b.rows.first().map(|r| rules(r)),
                    "{tag}: FDE {:#x}'s entry rule",
                    a.offset
                );
                fdes += 1;
                let (ra, rb): (Vec<&str>, Vec<&str>) = (
                    a.rows.iter().map(|r| rules(r)).collect(),
                    b.rows.iter().map(|r| rules(r)).collect(),
                );
                if ra == rb {
                    same_rules += 1;
                }
                if relative(a) == relative(b) && a.pc.1 - a.pc.0 == b.pc.1 - b.pc.0 {
                    let (x, y) = (
                        &hb[a.offset..a.offset + a.length + 4],
                        &lb[b.offset..b.offset + b.length + 4],
                    );
                    assert_eq!(x, y, "{tag}: FDE {:#x} with the same rows", a.offset);
                    identical += 1;
                }
            }

            // hod's rows are its `.eh_frame`'s for the module with tables.
            let tables = ir(&scratch, &fe, arch, &["-g"]);
            let eh = hod_object(&scratch, arch, &format!("{tag}-hod-uw"), &tables);
            let e = entries(&dwarfdump, &eh, ".eh_frame");
            let rows = |es: &[Entry]| {
                es.iter().filter(|e| !e.cie).map(|e| e.rows.clone()).collect::<Vec<_>>()
            };
            assert_eq!(rows(&h), rows(&e), "{tag}: hod's .debug_frame rows are its .eh_frame rows");

            // One function that may throw: every FDE in `.eh_frame`.
            let mixed = format!("{text}\ndefine void @may_throw() {{\n  ret void\n}}\n");
            let hm = hod_object(&scratch, arch, &format!("{tag}-hod-mixed"), &mixed);
            let lm = llc_object(&scratch, &fe.3, arch, &format!("{tag}-llc-mixed"), &mixed);
            for object in [&hm, &lm] {
                assert!(!has_section(&readelf, object, ".debug_frame"), "{}", object.display());
                let n = entries(&dwarfdump, object, ".eh_frame").iter().filter(|e| !e.cie).count();
                assert_eq!(n, h.len(), "{}: every function's FDE", object.display());
            }

            // Without debug information: no CFI.
            let plain = ir(&scratch, &fe, arch, &no_tables[1..]);
            let hp = hod_object(&scratch, arch, &format!("{tag}-hod-plain"), &plain);
            let lp = llc_object(&scratch, &fe.3, arch, &format!("{tag}-llc-plain"), &plain);
            for object in [&hp, &lp] {
                assert!(!has_section(&readelf, object, ".debug_frame"), "{}", object.display());
                assert!(!has_section(&readelf, object, ".eh_frame"), "{}", object.display());
            }
            compared += 1;
        }
    }
    eprintln!(
        "{compared} modules: {fdes} FDEs, {same_rules} with llc's rules, {identical} byte-identical"
    );
}
