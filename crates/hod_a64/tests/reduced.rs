//! Units whose `emissionKind` is `LineTablesOnly` or `DebugDirectivesOnly`
//! (`spec/dwarf.md` §6.10, WP-061), compared with `llc`'s.
//!
//! - `data/reduced/lto.cpp` with `-gline-tables-only` by clang 19 (`llc`
//!   19 and 22), clang 22 and kiln++ (`llc` 22): hod's DIE trees are
//!   `llc`'s (the unit alone: no function has inlined code), both
//!   objects verify, and the line table is byte for byte the one hod
//!   writes for the same source with `-g` (whose rows `lines.rs`
//!   compares with `llc`'s);
//! - the same with `-gline-directives-only`: the debug sections are
//!   `llc`'s (the line table alone) and the table is the same again;
//! - modules linked from a full, a line-tables-only, a directives-only
//!   and a no-debug unit, in several orders: one tree per unit with DIEs,
//!   equal to `llc`'s, and `llc`'s sections;
//! - each refusal by name, in a module with that one construct, and what
//!   is not refused around it (inlined code in a directives-only unit).

mod common;
#[path = "dies_support/mod.rs"]
mod support;

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use common::parse;
use hod_a64::{ModuleError, compile_to_object};
use hod_cg::{CgError, Options};
use hod_testutil::{Scratch, run_with, tool};
use support::{Node, count, first_difference, hod_object, kiln, llc_object, trees, verify};

/// The directory of this file's sources.
fn data_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data/reduced")
}

fn output(command: &mut Command) -> String {
    let outcome = run_with(command, &[], Duration::from_mins(2));
    assert!(outcome.success(), "{command:?}: {}", outcome.stderr_text());
    outcome.stdout_text()
}

/// `-O0` IR of `source` by `compiler` with the debug flag `flag`
/// (`-gline-tables-only`, `-gline-directives-only`, `-g`, `-g0`).
pub fn ir_with(compiler: &Path, scratch: &Scratch, name: &str, source: &str, flag: &str) -> String {
    let out = scratch.join(format!("{name}.ll"));
    let mut command = Command::new(compiler);
    command.args([flag, "-O0", "-S", "-emit-llvm", "-w", "-std=c++20"]);
    command.arg(data_dir().join(source)).arg("-o").arg(&out);
    output(&mut command);
    std::fs::read_to_string(out).unwrap()
}

/// The IR of `parts` linked by `llvm-link` `major`, in that order.
fn linked(scratch: &Scratch, major: u32, name: &str, parts: &[(&str, &str)]) -> Option<String> {
    let llvm_link = tool(&format!("llvm-link-{major}"))?;
    let clang = tool(&format!("clang++-{major}"))?;
    let mut command = Command::new(llvm_link);
    command.arg("-S");
    for (source, flag) in parts {
        let stem = source.split('.').next().unwrap();
        ir_with(&clang, scratch, &format!("{name}-{stem}"), source, flag);
        command.arg(scratch.join(format!("{name}-{stem}.ll")));
    }
    let out = scratch.join(format!("{name}.ll"));
    command.arg("-o").arg(&out);
    output(&mut command);
    Some(std::fs::read_to_string(out).unwrap())
}

fn dwarfdump() -> PathBuf {
    tool("llvm-dwarfdump-22").or_else(|| tool("llvm-dwarfdump")).expect("no llvm-dwarfdump")
}

/// The names of an object's sections that start `.debug_` or
/// `.rela.debug_`, sorted.
fn debug_sections(object: &Path) -> Vec<String> {
    let readelf =
        tool("llvm-readelf-22").or_else(|| tool("llvm-readelf")).expect("no llvm-readelf");
    let mut command = Command::new(readelf);
    command.args(["-S", "-W"]).arg(object);
    let mut names: Vec<String> = output(&mut command)
        .lines()
        .filter_map(|l| l.split(']').nth(1)?.split_whitespace().next().map(str::to_owned))
        .filter(|n| n.starts_with(".debug_") || n.starts_with(".rela.debug_"))
        .collect();
    names.sort();
    names
}

/// The bytes of the first section named `name` of the ELF file `object`.
fn section_bytes(object: &Path, name: &str) -> Vec<u8> {
    let file = std::fs::read(object).unwrap();
    let u16_at = |o: usize| u16::from_le_bytes(file[o..o + 2].try_into().unwrap()) as usize;
    let u32_at = |o: usize| u32::from_le_bytes(file[o..o + 4].try_into().unwrap()) as usize;
    let u64_at = |o: usize| u64::from_le_bytes(file[o..o + 8].try_into().unwrap()) as usize;
    let shoff = u64_at(0x28);
    let shnum = u16_at(0x3c);
    let shstrndx = u16_at(0x3e);
    let header = |i: usize| shoff + 64 * i;
    let strtab = u64_at(header(shstrndx) + 0x18);
    for i in 0..shnum {
        let h = header(i);
        let at = strtab + u32_at(h);
        let end = at + file[at..].iter().position(|&b| b == 0).unwrap();
        if &file[at..end] == name.as_bytes() {
            let (offset, size) = (u64_at(h + 0x18), u64_at(h + 0x20));
            return file[offset..offset + size].to_vec();
        }
    }
    panic!("{}: no section {name}", object.display());
}

#[derive(Default)]
struct Tally {
    objects: usize,
    units: usize,
    dies: usize,
}

/// hod's trees of `text` against `llc` `major`'s, both verified, and the
/// debug sections of both; returns hod's trees and object.
fn compare(
    scratch: &Scratch,
    major: u32,
    name: &str,
    text: &str,
    tally: &mut Tally,
) -> (Vec<Node>, PathBuf) {
    let llc = tool(&format!("llc-{major}")).unwrap_or_else(|| panic!("no llc-{major}"));
    let dwarfdump = dwarfdump();
    let h = hod_object(scratch, &format!("{name}-hod"), text);
    let l = llc_object(scratch, &llc, &format!("{name}-llc{major}"), text);
    verify(&dwarfdump, &h);
    verify(&dwarfdump, &l);
    assert_eq!(debug_sections(&h), debug_sections(&l), "{name} (llc {major}): debug sections");
    let (th, tl) = (trees(&dwarfdump, &h), trees(&dwarfdump, &l));
    assert_eq!(th.len(), tl.len(), "{name} (llc {major}): units");
    for (a, b) in th.iter().zip(&tl) {
        if let Some(d) = first_difference(a, b) {
            panic!("{name} (llc {major}): hod's tree, then llc's, differ at {d}");
        }
        tally.dies += count(a);
    }
    tally.objects += 2;
    tally.units += th.len();
    (th, h)
}

/// The compilers of the oracle: clang 19 (with the `llc` majors its IR
/// is read by), clang 22 and kiln++.
fn compilers() -> Vec<(String, PathBuf, &'static [u32])> {
    let mut out: Vec<(String, PathBuf, &'static [u32])> = Vec::new();
    if let Some(c) = tool("clang++-19") {
        out.push(("c19".into(), c, &[19, 22]));
    }
    if let Some(c) = tool("clang++-22") {
        out.push(("c22".into(), c, &[22]));
    }
    if let Some(k) = kiln() {
        out.push(("kiln".into(), k, &[22]));
    }
    out
}

/// §6.10: `-gline-tables-only`: the unit alone, `llc`'s; the line table
/// is the full unit's.
#[test]
fn line_tables_only_matches_llc() {
    let scratch = Scratch::new("reduced-lto");
    let mut tally = Tally::default();
    for (who, compiler, majors) in compilers() {
        let lto =
            ir_with(&compiler, &scratch, &format!("lto-{who}"), "lto.cpp", "-gline-tables-only");
        assert!(lto.contains("emissionKind: LineTablesOnly"), "{who}");
        let mut hod = None;
        for &major in majors {
            let (units, object) = compare(&scratch, major, &format!("lto-{who}"), &lto, &mut tally);
            assert_eq!(units.len(), 1);
            assert_eq!(units[0].tag, "DW_TAG_compile_unit");
            assert!(units[0].children.is_empty(), "{who}: {:?}", units[0].children);
            hod = Some(object);
        }
        let hod = hod.unwrap();
        // The same line table as the full unit's: the code's locations
        // are the same, only the DIEs differ.
        let full = ir_with(&compiler, &scratch, &format!("full-{who}"), "lto.cpp", "-g");
        let full = hod_object(&scratch, &format!("full-{who}-hod"), &full);
        assert_eq!(
            section_bytes(&hod, ".debug_line"),
            section_bytes(&full, ".debug_line"),
            "{who}: the line table of -gline-tables-only and of -g"
        );
        assert_eq!(section_bytes(&hod, ".debug_line_str"), section_bytes(&full, ".debug_line_str"));
    }
    assert!(tally.objects >= 2 || !hod_testutil::tools_required(), "no compiler ran");
    eprintln!(
        "line-tables-only trees equal to llc's: {} objects, {} units, {} DIEs of hod's",
        tally.objects, tally.units, tally.dies
    );
}

/// §6.10: `-gline-directives-only`: the line table alone, as `llc`
/// writes it, the same table as `-gline-tables-only`'s; inlined code is
/// no refusal there.
#[test]
fn directives_only_is_the_line_table_alone() {
    let scratch = Scratch::new("reduced-ddo");
    let mut tally = Tally::default();
    for (who, compiler, majors) in compilers() {
        let name = format!("ddo-{who}");
        let ddo = ir_with(&compiler, &scratch, &name, "lto.cpp", "-gline-directives-only");
        assert!(ddo.contains("emissionKind: DebugDirectivesOnly"), "{who}");
        for &major in majors {
            let (units, object) = compare(&scratch, major, &name, &ddo, &mut tally);
            assert!(units.is_empty(), "{who}: {units:?}");
            assert_eq!(
                debug_sections(&object),
                [".debug_line", ".debug_line_str", ".rela.debug_line"],
                "{who}"
            );
        }
        let lto = ir_with(
            &compiler,
            &scratch,
            &format!("ddo-lto-{who}"),
            "lto.cpp",
            "-gline-tables-only",
        );
        let lto = hod_object(&scratch, &format!("ddo-lto-{who}-hod"), &lto);
        let ddo = scratch.join(format!("{name}-hod.o"));
        assert_eq!(section_bytes(&ddo, ".debug_line"), section_bytes(&lto, ".debug_line"), "{who}");

        let inlined = format!("ddo-inlined-{who}");
        let text = ir_with(&compiler, &scratch, &inlined, "inlined.cpp", "-gline-directives-only");
        assert!(text.contains("inlinedAt"), "{who}: the call is inlined");
        for &major in majors {
            compare(&scratch, major, &inlined, &text, &mut tally);
        }
    }
    eprintln!("directives-only objects equal to llc's: {}", tally.objects);
}

/// §6.10: units of every kind in one module (`llvm-link`), in several
/// orders: the trees of the units with DIEs and the sections are `llc`'s.
#[test]
fn mixed_units_match_llc() {
    let scratch = Scratch::new("reduced-mixed");
    let mut tally = Tally::default();
    let full = ("mixed_full.cpp", "-g");
    let lto = ("mixed_lto.cpp", "-gline-tables-only");
    let ddo = ("mixed_ddo.cpp", "-gline-directives-only");
    let none = ("mixed_none.cpp", "-g0");
    let orders: [(&str, &[(&str, &str)]); 5] = [
        ("lfdn", &[lto, full, ddo, none]),
        ("dfnl", &[ddo, full, none, lto]),
        ("ndl", &[none, ddo, lto]),
        ("fl", &[full, lto]),
        ("dn", &[ddo, none]),
    ];
    for major in [19, 22] {
        for (name, parts) in orders {
            let name = format!("{name}-{major}");
            let Some(text) = linked(&scratch, major, &name, parts) else { continue };
            let (units, _) = compare(&scratch, major, &name, &text, &mut tally);
            let with_dies = parts.iter().filter(|p| p.1 == "-g" || p.1.contains("tables")).count();
            assert_eq!(units.len(), with_dies, "{name}");
        }
    }
    assert!(tally.units >= 12 || !hod_testutil::tools_required(), "{} units", tally.units);
    eprintln!(
        "mixed modules equal to llc's: {} objects, {} units, {} DIEs of hod's",
        tally.objects, tally.units, tally.dies
    );
}

fn refused(text: &str) -> Option<CgError> {
    let (module, bodies) = parse(text);
    match compile_to_object(&module, &bodies, &Options::default()) {
        Ok(_) => None,
        Err(ModuleError::Emit(e)) => Some(e),
        Err(e) => panic!("{e}"),
    }
}

/// §6.10: each refusal by its name, in a module with that one construct.
#[test]
fn refusals_by_name() {
    let Some(clang) = tool("clang++-22") else { return };
    let scratch = Scratch::new("reduced-refused");
    // Code inlined into a function of a line-tables-only unit is written
    // (WP-094, §6.13); with template parameters, which no producer writes
    // there, it is refused.
    let text = ir_with(&clang, &scratch, "inlined", "inlined.cpp", "-gline-tables-only");
    assert_eq!(refused(&text), None);
    let at = text.find("!DISubprogram(name: \"caller\"").unwrap();
    let at = at + text[at..].find("unit: !").unwrap();
    let text = format!(
        "{}templateParams: !900, {}\n!900 = !{{!901}}\n\
         !901 = !DITemplateTypeParameter(name: \"T\", type: null)\n",
        &text[..at],
        &text[at..]
    );
    assert_eq!(refused(&text), Some(CgError::unsupported(hod_dwarf_names::LINE_TABLES_INLINED)));

    // A function of a line-tables-only unit with variable records: the
    // full unit's IR relabelled, which `llc` 22 crashes on.
    let full = ir_with(&clang, &scratch, "vars", "mixed_full.cpp", "-g");
    assert!(full.contains("#dbg_declare") || full.contains("llvm.dbg.declare"));
    let text = full.replace("emissionKind: FullDebug", "emissionKind: LineTablesOnly");
    assert_eq!(refused(&text), Some(CgError::unsupported(hod_dwarf_names::LINE_TABLES_VARIABLES)));
    // The same records in a directives-only unit: nothing is written for
    // them, as `llc` writes nothing.
    let text = full.replace("emissionKind: FullDebug", "emissionKind: DebugDirectivesOnly");
    assert_eq!(refused(&text), None);

    // A reduced unit that lists a global, for each reduced kind.
    let lto = ir_with(&clang, &scratch, "globals", "mixed_lto.cpp", "-gline-tables-only");
    let with_global = lto.replace("nameTableKind: None)", "globals: !{}, nameTableKind: None)");
    assert_eq!(refused(&with_global), None, "an empty list is no global");
    let g = full_global(&lto);
    assert_eq!(refused(&g), Some(CgError::unsupported(hod_dwarf_names::REDUCED_UNIT_ENTITIES)));
    let g = g.replace("emissionKind: LineTablesOnly", "emissionKind: DebugDirectivesOnly");
    assert_eq!(refused(&g), Some(CgError::unsupported(hod_dwarf_names::REDUCED_UNIT_ENTITIES)));
}

/// The stable names of `hod_dwarf::reduced` (this crate does not depend
/// on `hod_dwarf`).
mod hod_dwarf_names {
    pub const REDUCED_UNIT_ENTITIES: &str = "debug:reduced-unit-entities";
    pub const LINE_TABLES_VARIABLES: &str = "debug:line-tables-variables";
    pub const LINE_TABLES_INLINED: &str = "debug:line-tables-inlined";
}

/// `lto` (clang 22's `-gline-tables-only` IR of `mixed_lto.cpp`) with a
/// global variable `g` that the unit lists.
fn full_global(lto: &str) -> String {
    let mut text = lto.replace("nameTableKind: None)", "globals: !{!900}, nameTableKind: None)");
    let cu = text.find("distinct !DICompileUnit").map(|at| {
        let line_start = text[..at].rfind('\n').map_or(0, |n| n + 1);
        text[line_start..].split(' ').next().unwrap().to_owned()
    });
    let cu = cu.expect("a unit");
    let file =
        text.split("file: ").nth(1).and_then(|r| r.split(',').next()).expect("a file").to_owned();
    text = text.replacen("\n!", "\n@g = global i32 7, align 4, !dbg !900\n!", 1);
    write!(
        text,
        "!900 = !DIGlobalVariableExpression(var: !901, expr: !DIExpression())\n\
         !901 = distinct !DIGlobalVariable(name: \"g\", scope: {cu}, file: {file}, line: 1, \
         type: !902, isLocal: false, isDefinition: true)\n\
         !902 = !DIBasicType(name: \"int\", size: 32, encoding: DW_ATE_signed)\n"
    )
    .unwrap();
    text
}
