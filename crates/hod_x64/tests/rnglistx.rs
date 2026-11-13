//! Range lists as `DW_FORM_rnglistx` in a DWARF 5 unit (WP-163,
//! `spec/dwarf.md` §6.4): every `DW_AT_ranges` is an index into the
//! unit's offset table, the scopes' lists in DIE order and the unit's own
//! last, with `DW_AT_rnglists_base` on the unit, as `llc` writes it. The
//! only relocation against `.debug_rnglists` is the unit's base.
//!
//! The module is `hod_a64`'s inlined-code test (`dies_inline/hand.ll`:
//! an inlined subroutine and a block each interrupted by their parent's
//! code), compiled for x86-64 with and without `-function-sections` (the
//! unit's own list). The indices and the table's entry count must be
//! `llc`'s (LLVM 19 and 22 when installed), and the objects verify.
#![cfg(target_os = "linux")]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use hod_cg::{Options, RelocModel};
use hod_testutil::{Scratch, run_with, tool};

fn source() -> String {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../hod_a64/tests/data/dies_inline/hand.ll");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    format!(
        "target datalayout = \"{}\"\ntarget triple = \"x86_64-unknown-linux-gnu\"\n{text}",
        hod_ir::X86_64_LINUX_DATA_LAYOUT
    )
}

fn hod(text: &str, function_sections: bool) -> Vec<u8> {
    let parsed = hod_ll::parse_module(text.as_bytes(), &hod_ll::ParseOptions::default())
        .unwrap_or_else(|e| panic!("hod_ll: {e}"));
    let mut module = parsed.module;
    let mut bodies = Vec::new();
    hod_ll::parse_bodies(&mut module, &parsed.text, |_, parsed| {
        bodies.push(
            parsed.body.unwrap_or_else(|| panic!("unsupported {:?}", parsed.unsupported)).clone(),
        );
        Ok::<(), hod_ll::LlError>(())
    })
    .unwrap_or_else(|e| panic!("hod_ll: {e}"));
    let options = Options { reloc_model: RelocModel::Pic, function_sections, ..Options::default() };
    hod_x64::compile_to_object(&module, &bodies, &options).unwrap_or_else(|e| panic!("hod: {e}"))
}

fn llc(dir: &Scratch, major: u32, text: &str, function_sections: bool) -> Option<PathBuf> {
    let llc = hod_testutil::find_tool(&format!("llc-{major}"))?;
    let input = dir.write(&format!("llc{major}.ll"), text);
    let output = dir.join(format!("llc{major}-{function_sections}.o"));
    let mut command = Command::new(llc);
    command
        .args(["-O0", "-fast-isel=0", "-mtriple=x86_64-unknown-linux-gnu"])
        .args(["-relocation-model=pic", "-filetype=obj"]);
    if function_sections {
        command.arg("-function-sections");
    }
    command.arg(&input).arg("-o").arg(&output);
    let outcome = run_with(&mut command, &[], Duration::from_mins(5));
    assert!(outcome.success(), "llc {major} failed:\n{}", outcome.stderr_text());
    Some(output)
}

fn stdout(tool_name: &str, args: &[&str], object: &Path) -> Option<String> {
    let path = tool(tool_name)?;
    let mut command = Command::new(path);
    command.args(args).arg(object);
    let outcome = run_with(&mut command, &[], Duration::from_mins(2));
    assert!(outcome.success(), "{tool_name}: {}", outcome.stderr_text());
    Some(outcome.stdout_text())
}

/// What is compared: each `DW_AT_ranges` as (the tag of its DIE, its
/// form and index), and the table's `offset_entry_count`.
#[derive(Debug, PartialEq, Eq)]
struct Lists {
    ranges: Vec<(String, String)>,
    entries: Vec<String>,
}

fn lists(object: &Path) -> Option<Lists> {
    let info = stdout("llvm-dwarfdump", &["-v", "--debug-info"], object)?;
    let mut tag = String::new();
    let mut ranges = Vec::new();
    for line in info.lines() {
        let t = line.trim_start();
        if let Some(at) = t.find("DW_TAG_") {
            t[at..].split_whitespace().next().unwrap_or("").clone_into(&mut tag);
        } else if t.starts_with("DW_AT_ranges") {
            // `DW_AT_ranges [DW_FORM_rnglistx]	(indexed (0x1) rangelist = ...`
            let form = t.split(['[', ']']).nth(1).unwrap_or("").to_owned();
            let index = t.split("indexed (").nth(1).and_then(|r| r.split(')').next());
            ranges.push((tag.clone(), format!("{form} {}", index.unwrap_or("-"))));
        }
    }
    let rnglists = stdout("llvm-dwarfdump", &["--debug-rnglists"], object)?;
    let entries = rnglists
        .lines()
        .filter_map(|l| l.split("offset_entry_count = ").nth(1))
        .map(str::to_owned)
        .collect();
    Some(Lists { ranges, entries })
}

/// The relocations of `.rela.debug_info` against `.debug_rnglists`.
fn list_relocations(object: &Path) -> Option<usize> {
    let relocs = stdout("llvm-readelf", &["-r"], object)?;
    let mut in_info = false;
    let mut n = 0;
    for line in relocs.lines() {
        if line.starts_with("Relocation section") {
            in_info = line.contains("'.rela.debug_info'");
        } else if in_info && line.contains(".debug_rnglists") {
            n += 1;
        }
    }
    Some(n)
}

fn verify(object: &Path) {
    if let Some(out) = stdout("llvm-dwarfdump", &["--verify"], object) {
        assert!(out.contains("No errors."), "{}: {out}", object.display());
    }
}

#[test]
fn ranges_are_rnglistx_as_llc_writes_them() {
    let dir = Scratch::new("rnglistx");
    let text = source();
    for function_sections in [false, true] {
        let object =
            dir.write(&format!("hod-{function_sections}.o"), hod(&text, function_sections));
        verify(&object);
        let Some(ours) = lists(&object) else { return };
        assert!(
            ours.ranges.iter().all(|(_, f)| f.starts_with("DW_FORM_rnglistx")),
            "every DW_AT_ranges is rnglistx: {ours:?}"
        );
        let scopes = ours.ranges.iter().filter(|(t, _)| t != "DW_TAG_compile_unit").count();
        assert!(scopes >= 2, "the module has scope lists: {ours:?}");
        assert_eq!(
            ours.ranges.iter().any(|(t, _)| t == "DW_TAG_compile_unit"),
            function_sections,
            "the unit's own list: {ours:?}"
        );
        assert_eq!(list_relocations(&object), Some(1), "only the unit's rnglists_base");
        // Each index reaches its own list: the unit's is its functions'.
        if let (Some(lists), Some(mut funcs)) = (resolved(&object), functions(&object)) {
            for (tag, mut ranges) in lists {
                if tag == "DW_TAG_compile_unit" {
                    ranges.sort();
                    funcs.sort();
                    assert_eq!(ranges, funcs, "the unit's list is its functions' code");
                }
            }
        }
        for major in [19, 22] {
            let Some(reference) = llc(&dir, major, &text, function_sections) else { continue };
            let Some(theirs) = lists(&reference) else { continue };
            assert_eq!(ours, theirs, "llc {major}, function sections {function_sections}");
        }
    }
}

/// Each `DW_AT_ranges`' tag and the ranges it resolves to.
fn resolved(object: &Path) -> Option<Vec<(String, Vec<String>)>> {
    let info = stdout("llvm-dwarfdump", &["--debug-info"], object)?;
    let mut tag = String::new();
    let mut out: Vec<(String, Vec<String>)> = Vec::new();
    let mut open = false;
    for line in info.lines() {
        let t = line.trim();
        if let Some(at) = t.find("DW_TAG_") {
            t[at..].split_whitespace().next().unwrap_or("").clone_into(&mut tag);
            open = false;
        } else if t.starts_with("DW_AT_ranges") {
            out.push((tag.clone(), Vec::new()));
            open = true;
        } else if open && t.starts_with('[') {
            if let Some(last) = out.last_mut() {
                last.1.push(t.trim_end_matches(')').to_owned());
            }
            open = !t.ends_with("))");
        } else {
            open = false;
        }
    }
    Some(out)
}

/// The `[low, high)` of each `DW_TAG_subprogram` with code, sorted, as
/// `llvm-dwarfdump` prints a range list's entries.
fn functions(object: &Path) -> Option<Vec<String>> {
    let info = stdout("llvm-dwarfdump", &["--debug-info"], object)?;
    let (mut sub, mut low) = (false, None);
    let mut out = Vec::new();
    for line in info.lines() {
        let t = line.trim();
        let value = || t.split('(').nth(1).map(|v| v.trim_end_matches(')').to_owned());
        if t.contains("DW_TAG_") {
            sub = t.contains("DW_TAG_subprogram");
            low = None;
        } else if sub && t.starts_with("DW_AT_low_pc") {
            low = value();
        } else if sub
            && t.starts_with("DW_AT_high_pc")
            && let (Some(l), Some(h)) = (low.take(), value())
        {
            out.push(format!("[{l}, {h}"));
        }
    }
    out.sort();
    Some(out)
}

/// §6.20: DWARF 4 reads each `rnglistx` list of the version 5 unit by its
/// index (WP-163), so every DIE's `.debug_ranges` list is the version 5
/// list of the same code.
#[test]
fn dwarf_4_resolves_each_index_to_its_list() {
    let dir = Scratch::new("rnglistx4");
    let five = source();
    let four = five.replace("!\"Dwarf Version\", i32 5", "!\"Dwarf Version\", i32 4");
    assert_ne!(five, four);
    for function_sections in [false, true] {
        let o5 = dir.write(&format!("v5-{function_sections}.o"), hod(&five, function_sections));
        let o4 = dir.write(&format!("v4-{function_sections}.o"), hod(&four, function_sections));
        verify(&o4);
        let (Some(r5), Some(r4)) = (resolved(&o5), resolved(&o4)) else { return };
        assert!(r5.iter().filter(|(t, _)| t != "DW_TAG_compile_unit").count() >= 2, "{r5:?}");
        assert_eq!(r4, r5, "function sections {function_sections}");
    }
}
