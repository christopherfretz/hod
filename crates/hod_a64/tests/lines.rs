//! Line tables written by module emission (`spec/dwarf.md` §5.7, §9;
//! `spec/codegen.md` §9.7): the bridge from the functions' line records
//! to `hod_dwarf`, against `llc -O0 -global-isel=0 -fast-isel=0`.
//!
//! - what is written for which compile units (nothing without one, or
//!   for `NoDebug` units; `LineTablesOnly` refused by name);
//! - the rows of the hand-written cases (`data/lines/hand.ll`): a block
//!   that starts without a location, an explicit line 0, two returns, a
//!   function without a subprogram; an inlined location
//!   (`data/lines/hand_inlined.ll`) is refused until its DIEs are
//!   written (`debug:inlined-subroutine`, `spec/dwarf.md` §6.4);
//! - the relocations of the sequences and of the unit's ranges, with and
//!   without `-function-sections`;
//! - the oracle: per function, hod's rows against `llc`'s (LLVM 19 and
//!   22), modulo addresses and the permitted differences of §5.7;
//! - determinism: the same bytes for 1, 2, 3 and 8 chunks, compiled on
//!   one thread or several, and across runs.

mod common;
#[path = "lines_support/mod.rs"]
mod support;

use std::path::Path;
use std::process::Command;
use std::time::Duration;

use common::{ll_header, parse};
use hod_a64::{A64, ModuleError, compile_to_object};
use hod_cg::{CgError, Chunk, Options, Scratch as CgScratch, compile_function, emit_module};
use hod_ir::Body;
use hod_testutil::{Scratch, run_with, tool};
use support::{DRow, branch_locations, compare, data, rows, rows_by_function, with_header};

/// `hand.ll` (no target lines of its own) for LLVM `major`.
fn hand(major: u32) -> String {
    format!("{}{}", ll_header(major), data("hand.ll"))
}

fn object(text: &str, options: Options) -> Vec<u8> {
    let (module, bodies) = parse(text);
    compile_to_object(&module, &bodies, &options).unwrap_or_else(|e| panic!("{e}"))
}

fn section_names(dir: &Scratch, name: &str, bytes: &[u8]) -> Vec<String> {
    let path = dir.write(name, bytes);
    let Some(readelf) = tool("llvm-readelf") else { return Vec::new() };
    let mut command = Command::new(readelf);
    command.args(["-S", "-W"]).arg(&path);
    let out = run_with(&mut command, &[], Duration::from_mins(1));
    assert!(out.success(), "{}", out.stderr_text());
    out.stdout_text()
        .lines()
        .filter_map(|l| l.split(']').nth(1)?.split_whitespace().next().map(str::to_owned))
        .collect()
}

fn debug_sections(names: &[String]) -> Vec<&String> {
    names.iter().filter(|n| n.contains(".debug_")).collect()
}

/// A module without `!llvm.dbg.cu` and one whose only unit is `NoDebug`
/// write no debug section, and the same bytes as the module with no debug
/// information at all; `LineTablesOnly` and `DebugDirectivesOnly` are
/// refused by name.
#[test]
fn which_units_are_written() {
    let dir = Scratch::new("lines-units");
    let full = hand(22);
    let options = Options::default();
    let with = object(&full, options);
    let with_names = section_names(&dir, "full.o", &with);
    if !with_names.is_empty() {
        for s in [".debug_line", ".debug_line_str", ".debug_info", ".debug_abbrev", ".debug_str"] {
            assert!(with_names.iter().any(|n| n == s), "{s} in {with_names:?}");
        }
        assert!(with_names.iter().any(|n| n == ".rela.debug_line"), "{with_names:?}");
    }

    // No debug information at all: no attachments, no metadata.
    let bare: String = full
        .lines()
        .filter(|l| !l.starts_with('!'))
        .map(|l| l.split(", !dbg !").next().unwrap())
        .map(|l| l.replace(" !dbg !10 {", " {").replace(" !dbg !20 {", " {"))
        .map(|l| l.replace(" !dbg !30 {", " {").replace(" !dbg !50 {", " {"))
        .flat_map(|l| [l, "\n".to_owned()])
        .collect();
    assert!(!bare.contains("!dbg"), "{bare}");
    let bare = object(&bare, options);
    assert_eq!(debug_sections(&section_names(&dir, "bare.o", &bare)), Vec::<&String>::new());

    let no_cu = full.replace("!llvm.dbg.cu = !{!0}\n", "");
    assert_eq!(object(&no_cu, options), bare, "without !llvm.dbg.cu");
    let no_debug = full.replace("emissionKind: FullDebug", "emissionKind: NoDebug");
    assert_eq!(object(&no_debug, options), bare, "a NoDebug unit");

    for kind in ["LineTablesOnly", "DebugDirectivesOnly"] {
        let text = full.replace("emissionKind: FullDebug", &format!("emissionKind: {kind}"));
        let (module, bodies) = parse(&text);
        match compile_to_object(&module, &bodies, &options) {
            Err(ModuleError::Emit(e)) => {
                assert_eq!(e, CgError::unsupported(hod_cg::emit::unsupported::DEBUG_EMISSION_KIND));
            }
            other => panic!("{kind}: {:?}", other.map(|b| b.len())),
        }
    }
}

fn dwarf_tools() -> Option<(std::path::PathBuf, std::path::PathBuf)> {
    Some((tool("llvm-dwarfdump")?, tool("llvm-nm")?))
}

/// The rows of the hand-written cases, as `llvm-dwarfdump` reads them.
#[test]
fn hand_written_rows() {
    let Some((dwarfdump, nm)) = dwarf_tools() else { return };
    let dir = Scratch::new("lines-hand");
    let path = dir.write("hand.o", object(&hand(22), Options::default()));
    let by = rows_by_function(&dwarfdump, &nm, &path);
    let show = |name: &str| format!("{name}: {:#?}", by.get(name));

    // The block that starts with an instruction without a location: a
    // line-0 row there, in the file and column of the location before
    // it, not a statement.
    let block = &by["block_start"];
    assert!(
        block
            .iter()
            .any(|r| r.line == 0 && r.column == 7 && !r.stmt && r.file.ends_with("/hand.c")),
        "{}",
        show("block_start")
    );
    // An explicit line 0 is a line-0 row.
    let zero = &by["line_zero"];
    assert!(zero.iter().any(|r| r.line == 0), "{}", show("line_zero"));
    assert!(zero.iter().any(|r| r.line == 17 && r.stmt), "{}", show("line_zero"));
    // Two returns: both return lines, one epilogue.
    let two = &by["two_rets"];
    for line in [21, 22, 23] {
        assert!(two.iter().any(|r| r.line == line && r.stmt), "{line}: {}", show("two_rets"));
    }
    assert_eq!(two.iter().filter(|r| r.epilogue_begin).count(), 1, "{}", show("two_rets"));
    // No subprogram: no rows, as `llc` writes it.
    assert!(!by.contains_key("no_subprogram"), "{}", show("no_subprogram"));
    // Every function with a subprogram starts with a row at its scope
    // line, and has its prologue's end.
    for (name, line) in [("block_start", 3), ("line_zero", 15), ("two_rets", 20)] {
        let r = &by[name];
        assert_eq!((r[0].line, r[0].column, r[0].stmt), (line, 0, true), "{}", show(name));
        assert_eq!(r.iter().filter(|r| r.prologue_end).count(), 1, "{}", show(name));
    }
}

/// An inlined location: its row would keep its own scope's file and
/// line, but the function's DIEs need a `DW_TAG_inlined_subroutine`,
/// which is not written yet; the module is refused by that name
/// (`spec/dwarf.md` §6.4).
#[test]
fn inlined_locations_are_refused() {
    let text = format!("{}{}", ll_header(22), data("hand_inlined.ll"));
    let (module, bodies) = parse(&text);
    match compile_to_object(&module, &bodies, &Options::default()) {
        Err(ModuleError::Emit(e)) => {
            assert_eq!(e, CgError::unsupported("debug:inlined-subroutine"));
        }
        other => panic!("{:?}", other.map(|b| b.len())),
    }
}

/// `llvm-readelf -r` of a section's relocations: (symbol, addend).
fn relocations(path: &Path, section: &str) -> Vec<(String, i64)> {
    let Some(readelf) = tool("llvm-readelf") else { return Vec::new() };
    let mut command = Command::new(readelf);
    command.args(["-r", "-W"]).arg(path);
    let out = run_with(&mut command, &[], Duration::from_mins(1));
    assert!(out.success(), "{}", out.stderr_text());
    let text = out.stdout_text();
    let mut inside = false;
    let mut found = Vec::new();
    for l in text.lines() {
        if l.starts_with("Relocation section ") {
            inside = l.contains(&format!("'{section}'"));
            continue;
        }
        let f: Vec<&str> = l.split_whitespace().collect();
        if !inside || f.len() < 5 || !f[2].starts_with("R_AARCH64") {
            continue;
        }
        let (sign, addend) = (f[f.len() - 2], f[f.len() - 1]);
        let addend = i64::from_str_radix(addend, 16).unwrap();
        let addend = if sign == "-" { -addend } else { addend };
        found.push((f[4].to_owned(), addend));
    }
    found
}

/// Each function's sequence and range is relocated against the section
/// symbol of its code plus its offset there, with or without
/// `-function-sections`; the object verifies.
#[test]
fn relocated_against_section_symbols() {
    let Some((dwarfdump, nm)) = dwarf_tools() else { return };
    let dir = Scratch::new("lines-relocs");
    let text = with_header(&data("prog.ll"), 22);
    for function_sections in [false, true] {
        let options = Options { function_sections, ..Options::default() };
        let path = dir.write(&format!("prog-{function_sections}.o"), object(&text, options));
        let mut line = relocations(&path, ".rela.debug_line");
        line.retain(|(s, _)| !s.starts_with(".debug"));
        let functions = support::functions(&nm, &path);
        assert_eq!(line.len(), functions.len(), "one sequence per function: {line:?}");
        let expected: Vec<(String, i64)> = functions
            .iter()
            .map(|(name, address, _)| {
                if function_sections {
                    (format!(".text.{name}"), 0)
                } else {
                    (".text".to_owned(), *address as i64)
                }
            })
            .collect();
        let mut sorted = line.clone();
        sorted.sort_by_key(|(s, a)| (s.clone(), *a));
        let mut expected_sorted = expected.clone();
        expected_sorted.sort_by_key(|(s, a)| (s.clone(), *a));
        assert_eq!(sorted, expected_sorted, "function_sections {function_sections}");
        // The unit's ranges name the same symbols.
        let info = relocations(&path, ".rela.debug_addr");
        for (symbol, _) in &expected {
            assert!(info.iter().any(|(s, _)| s == symbol), "{symbol} in {info:?}");
        }
        let mut command = Command::new(&dwarfdump);
        command.args(["--verify", "--debug-line", "--debug-info"]).arg(&path);
        let out = run_with(&mut command, &[], Duration::from_mins(1));
        assert!(out.success(), "verify: {}{}", out.stdout_text(), out.stderr_text());
        let mut command = Command::new(&dwarfdump);
        command.args(["--debug-info"]).arg(&path);
        let out = run_with(&mut command, &[], Duration::from_mins(1)).stdout_text();
        assert!(out.contains("DW_AT_stmt_list"), "{out}");
        assert!(
            out.contains(if function_sections { "DW_AT_ranges" } else { "DW_AT_low_pc" }),
            "{out}"
        );
    }
}

/// The `.ll` inputs of the oracle, by name: (name, text for LLVM major).
fn oracle_inputs(major: u32) -> Vec<(&'static str, String)> {
    vec![("hand", hand(major)), ("prog", with_header(&data("prog.ll"), major))]
}

fn llc_of(major: u32) -> Option<std::path::PathBuf> {
    tool(&format!("llc-{major}"))
}

fn oracle(major: u32) {
    let Some((dwarfdump, nm)) = dwarf_tools() else { return };
    let Some(llc) = llc_of(major) else { return };
    let dir = Scratch::new(&format!("lines-oracle{major}"));
    let mut functions = 0;
    let mut hod_rows = 0;
    let mut llc_rows = 0;
    for (name, text) in oracle_inputs(major) {
        let hod_path = dir.write(&format!("{name}.hod.o"), object(&text, Options::default()));
        let input = dir.write(&format!("{name}.ll"), &text);
        let llc_path = dir.join(format!("{name}.llc.o"));
        let mut command = Command::new(&llc);
        command
            .args(["-O0", "-global-isel=0", "-fast-isel=0", "-filetype=obj"])
            .arg(&input)
            .arg("-o")
            .arg(&llc_path);
        let out = run_with(&mut command, &[], Duration::from_mins(2));
        assert!(out.success(), "llc {major}: {}", out.stderr_text());
        let hod = rows_by_function(&dwarfdump, &nm, &hod_path);
        let llc = rows_by_function(&dwarfdump, &nm, &llc_path);
        let names: Vec<&String> = hod.keys().collect();
        assert_eq!(names, llc.keys().collect::<Vec<_>>(), "{name}: functions with rows");
        let branches = branch_locations(&text);
        for (function, h) in &hod {
            if let Some(why) = compare(h, &llc[function], &branches) {
                panic!(
                    "llc {major}: {name}: {function}: {why}\n hod {h:#?}\n llc {:#?}",
                    llc[function]
                );
            }
            // The comparison catches a table one line off.
            let mut off = h.clone();
            for r in off.iter_mut().filter(|r| r.line != 0) {
                r.line += 1;
            }
            assert!(compare(&off, &llc[function], &branches).is_some(), "{function}: off by one");
            functions += 1;
            hod_rows += h.len();
            llc_rows += llc[function].len();
        }
        // The file table: every file `llc` names, with its checksum and
        // source, hod names too.
        let (hf, lf) = (file_entries(&dwarfdump, &hod_path), file_entries(&dwarfdump, &llc_path));
        for f in &lf {
            assert!(hf.contains(f), "{name}: {f:?} not in hod's {hf:?}");
        }
    }
    assert!(functions >= 6, "{functions}");
    eprintln!(
        "lines oracle (llc {major}): {functions} functions, {hod_rows} rows (llc {llc_rows}), equal modulo §5.7's differences"
    );
}

/// The file entries of the line tables: (name, dir, md5 and source
/// lines), as `llvm-dwarfdump` prints them.
fn file_entries(dwarfdump: &Path, path: &Path) -> Vec<String> {
    let mut command = Command::new(dwarfdump);
    command.arg("--debug-line").arg(path);
    let text = run_with(&mut command, &[], Duration::from_mins(1)).stdout_text();
    let mut out = Vec::new();
    let mut current = String::new();
    for l in text.lines() {
        let t = l.trim();
        if t.starts_with("file_names[") {
            if !current.is_empty() {
                out.push(std::mem::take(&mut current));
            }
        } else if t.starts_with("name:")
            || t.starts_with("md5_checksum:")
            || t.starts_with("source:")
        {
            current.push_str(t);
            current.push('|');
        } else if (t.starts_with("0x") || t.starts_with("Address")) && !current.is_empty() {
            out.push(std::mem::take(&mut current));
        }
    }
    out
}

#[test]
fn llc19_rows_match() {
    oracle(19);
}

#[test]
fn llc22_rows_match() {
    oracle(22);
}

/// The bodies of a module divided into `parts` chunks of consecutive
/// bodies, each compiled on its own thread when `threads`.
fn chunks(text: &str, parts: usize, threads: bool) -> Vec<u8> {
    let (module, bodies) = parse(text);
    let options = Options::default();
    let per = bodies.len().div_ceil(parts.max(1)).max(1);
    let groups: Vec<&[Body]> = bodies.chunks(per).collect();
    let compile = |group: &[Body]| {
        let mut scratch = CgScratch::new();
        let mut chunk = Chunk::new();
        for body in group {
            compile_function(&A64, &module, body, &options, &mut scratch, &mut chunk).unwrap();
        }
        chunk
    };
    let mut done: Vec<Chunk> = if threads {
        std::thread::scope(|s| {
            let handles: Vec<_> = groups.iter().map(|g| s.spawn(|| compile(g))).collect();
            handles.into_iter().map(|h| h.join().unwrap()).collect()
        })
    } else {
        groups.iter().map(|g| compile(g)).collect()
    };
    // Empty chunks, for the parts there are no bodies for.
    while done.len() < parts {
        done.push(Chunk::new());
    }
    let emitted = emit_module(&module, &options, &done).unwrap();
    hod_obj::elf::to_vec(&emitted.object).unwrap()
}

/// The same bytes for 1, 2, 3 and 8 chunks, on one thread or several,
/// and across runs.
#[test]
fn bytes_are_independent_of_chunks_and_threads() {
    let mut inputs = oracle_inputs(22);
    inputs.push(("tu1", with_header(&data("tu1.ll"), 22)));
    for (name, text) in &inputs {
        let reference = chunks(text, 1, false);
        assert_eq!(chunks(text, 1, false), reference, "{name}: a second run");
        for parts in [2, 3, 8] {
            for threads in [false, true] {
                assert_eq!(
                    chunks(text, parts, threads),
                    reference,
                    "{name}: {parts} chunks, threads {threads}"
                );
            }
        }
    }
}

/// The reader the oracle uses reads hod's table of `prog.ll`: every row
/// in its one file.
#[test]
fn reader_reads_both() {
    let Some((dwarfdump, _)) = dwarf_tools() else { return };
    let dir = Scratch::new("lines-reader");
    let path = dir.write("prog.o", object(&with_header(&data("prog.ll"), 22), Options::default()));
    let all: Vec<DRow> = rows(&dwarfdump, &path);
    assert!(all.len() > 20);
    assert!(all.iter().all(|r| r.file == "/hod/tests/lines/prog.c"), "{all:#?}");
}
