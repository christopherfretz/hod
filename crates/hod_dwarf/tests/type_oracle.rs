//! `spec/dwarf.md` §8.6: the type graph against `llc`. Hand-written IR
//! per feature, and `-g` IR of feature-dense C++ (`tests/data/types/`)
//! by clang 19, clang 22 and kiln++, made into the oracle input
//! (`common/types.rs`), are compiled by `llc` (`-O0 -global-isel=0
//! -fast-isel=0`, both targets); hod's DIE sections, from `hod_ll`'s
//! reading of the same text, are spliced into `llc`'s object. Both must
//! pass `llvm-dwarfdump --verify`, their DIE trees must be equal after
//! the normalization of §6.7, and their relocations must name the same
//! targets.

#![allow(clippy::cast_precision_loss)]

mod common;

use std::path::{Path, PathBuf};

use common::dies::{
    addr_targets, count, die_section_list, dump, parse_tree, render, sorted, splice, target_name,
    verify_all,
};
use common::types::{cxx, extract, module_sections, oracle_input, read, strip_template_params};
use common::{Elf, TRIPLES, compile, dwarfdumps, llc};
use hod_testutil::{Scratch, tool, tools_required};

fn data(name: &str) -> PathBuf {
    PathBuf::from(format!("{}/tests/data/types/{name}", env!("CARGO_MANIFEST_DIR")))
}

/// The module without its target lines, so `-mtriple` decides.
fn retarget(ir: &str) -> String {
    ir.lines()
        .filter(|l| !l.starts_with("target datalayout") && !l.starts_with("target triple"))
        .fold(String::new(), |mut out, l| {
            out.push_str(l);
            out.push('\n');
            out
        })
}

fn reloc_targets(elf: &Elf, section: &str) -> Vec<String> {
    let Some(index) = elf.section(section) else { return Vec::new() };
    let Some(rela) = elf.rela_of(index) else { return Vec::new() };
    let strs = elf.section(".debug_str").map(|s| elf.bytes(s));
    let mut out: Vec<String> = elf
        .relas(rela)
        .iter()
        .map(|r| match (section, strs) {
            (".debug_str_offsets", Some(s)) => {
                let at = r.addend as usize;
                let end = s[at..].iter().position(|&b| b == 0).unwrap();
                String::from_utf8_lossy(&s[at..at + end]).into_owned()
            }
            _ => format!("{} type {}", target_name(elf, r.sym, r.addend), r.ty),
        })
        .collect();
    out.sort();
    out
}

/// `text` with each `DW_AT_count` or `DW_AT_friend` that is a reference
/// (an offset at which a DIE starts) named by that DIE's `DW_AT_name`:
/// `llvm-dwarfdump` prints those two as bare offsets, which differ
/// between the two layouts; the DIEs they name must not.
fn name_refs(text: &str) -> String {
    let mut names = std::collections::HashMap::new();
    let mut at = None;
    for line in text.lines() {
        if let Some((off, _)) = line.split_once(": ")
            && let Some(hex) = off.strip_prefix("0x")
            && let Ok(o) = u64::from_str_radix(hex, 16)
        {
            at = Some(o);
        } else if let Some(o) = at
            && let Some(name) = line.trim_start().strip_prefix("DW_AT_name\t(")
        {
            names.insert(o, name.trim_end_matches(')').to_string());
            at = None;
        }
    }
    let mut out = String::with_capacity(text.len());
    for line in text.lines() {
        let named = ["DW_AT_count", "DW_AT_friend"].into_iter().find_map(|a| {
            let v = line.trim_start().strip_prefix(a)?.strip_prefix("\t(0x")?.strip_suffix(')')?;
            let o = u64::from_str_radix(v, 16).ok().filter(|_| v.len() == 8)?;
            Some((a, names.get(&o)?))
        });
        match named {
            Some((a, n)) => out.push_str(&format!("{a}\t(the DIE of {n})")),
            None => out.push_str(line),
        }
        out.push('\n');
    }
    out
}

/// What one comparison found.
#[derive(Default)]
struct Tally {
    objects: usize,
    dies: usize,
    same_order: usize,
    llc_info: u64,
    hod_info: u64,
}

/// One module through `llc` `major` and hod, on both targets.
fn compare(major: u32, label: &str, ir: &str, tally: &mut Tally) {
    let Some(llc) = llc(major) else { return };
    let dumps = dwarfdumps();
    let Some((_, dwarfdump)) = dumps.iter().find(|(m, _)| *m == major) else { return };
    let scratch = Scratch::new(&format!("types-{label}-{major}"));
    let module = read(ir);
    let for_llc = retarget(&strip_template_params(ir));
    for (arch, triple) in TRIPLES {
        let label = format!("{label}-{major}-{arch:?}");
        let object = compile(&llc, &scratch, &label, &for_llc, triple);
        let elf = Elf::parse(object.clone());
        let (obj, line, dies, _) = module_sections(&elf, arch, &module)
            .unwrap_or_else(|e| panic!("{label}: {e}: {}", node_text(&module, &e)));
        let patched = splice(&elf, &obj, &die_section_list(line, &dies));
        let ours = Elf::parse(patched.clone());
        verify_all(dwarfdump, &scratch, &format!("{label}.orig.o"), &object);
        verify_all(dwarfdump, &scratch, &format!("{label}.hod.o"), &patched);
        let theirs_text =
            dump(dwarfdump, &scratch, &format!("{label}.orig.o"), &object, &["--debug-info"]);
        let ours_text =
            dump(dwarfdump, &scratch, &format!("{label}.hod.o"), &patched, &["--debug-info"]);
        let theirs = parse_tree(&name_refs(&theirs_text), &addr_targets(&elf));
        let ours_tree = parse_tree(&name_refs(&ours_text), &addr_targets(&ours));
        assert_eq!(theirs.len(), 1, "{label}: one unit");
        assert_eq!(ours_tree.len(), 1, "{label}: one unit");
        let (a, b) = (sorted(&theirs[0]), sorted(&ours_tree[0]));
        if a != b {
            let (mut x, mut y) = (String::new(), String::new());
            render(&a, 0, &mut x);
            render(&b, 0, &mut y);
            let first = x.lines().zip(y.lines()).position(|(p, q)| p != q).unwrap_or(0);
            let around = |s: &str| {
                s.lines().skip(first.saturating_sub(12)).take(30).collect::<Vec<_>>().join("\n")
            };
            panic!(
                "{label}: DIE trees differ at line {first}\nllc:\n{}\nhod:\n{}",
                around(&x),
                around(&y)
            );
        }
        for section in [".debug_info", ".debug_str_offsets", ".debug_addr"] {
            assert_eq!(
                reloc_targets(&elf, section),
                reloc_targets(&ours, section),
                "{label}: targets of the relocations of {section}"
            );
        }
        if theirs[0] != ours_tree[0] && std::env::var_os("HOD_SHOW_ORDER").is_some() {
            let (mut x, mut y) = (String::new(), String::new());
            render(&theirs[0], 0, &mut x);
            render(&ours_tree[0], 0, &mut y);
            let first = x.lines().zip(y.lines()).position(|(p, q)| p != q).unwrap_or(0);
            let around = |s: &str| {
                s.lines().skip(first.saturating_sub(8)).take(16).collect::<Vec<_>>().join("\n")
            };
            eprintln!(
                "{label}: order differs at line {first}\nllc:\n{}\nhod:\n{}",
                around(&x),
                around(&y)
            );
        }
        tally.objects += 1;
        tally.dies += count(&a);
        tally.same_order += usize::from(theirs[0] == ours_tree[0]);
        let info = |e: &Elf| e.section(".debug_info").map_or(0, |i| e.sections[i].size);
        tally.llc_info += info(&elf);
        tally.hod_info += info(&ours);
    }
}

/// The node an error names, for the message.
fn node_text(module: &hod_ir::Module, e: &hod_dwarf::Error) -> String {
    use hod_dwarf::Error as E;
    let node = match *e {
        E::BadReference { node, .. }
        | E::Unsupported { node, .. }
        | E::MemberOutsideType { node }
        | E::TypeCycle { node }
        | E::MissingSize { node } => node,
        _ => return String::new(),
    };
    format!("{:?}", module.debug.node(hod_ir::DiId::new(node)))
}

fn report(what: &str, t: &Tally) {
    eprintln!(
        "{what}: {} objects, {} DIEs equal as trees, {} in llc's order; .debug_info hod/llc {}/{} = {:.3}",
        t.objects,
        t.dies,
        t.same_order,
        t.hod_info,
        t.llc_info,
        t.hod_info as f64 / t.llc_info.max(1) as f64
    );
}

/// `kiln++`, beside the build directory or from `HOD_KILN`.
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
            eprintln!("note: no kiln++ (set HOD_KILN); skipping its part");
            None
        }
    }
}

/// The oracle input of `source` by `compiler`.
fn from_cxx(compiler: &Path, label: &str, source: &str) -> Option<String> {
    let extract_tool = tool("llvm-extract")?;
    let scratch = Scratch::new(&format!("types-cxx-{label}"));
    let ir = cxx(compiler, &scratch, label, &data(source), None);
    Some(extract(&extract_tool, &scratch, label, &oracle_input(&ir)))
}

/// §8.6: hand-written IR, one module per feature class.
#[test]
fn hand_written_ir_matches_llc() {
    let mut tally = Tally::default();
    for (name, majors) in
        [("hand.ll", &[19, 22][..]), ("hand_c.ll", &[19, 22][..]), ("hand22.ll", &[22][..])]
    {
        let ir = std::fs::read_to_string(data(name)).unwrap();
        for &m in majors {
            compare(m, name.trim_end_matches(".ll"), &ir, &mut tally);
        }
    }
    report("hand-written IR", &tally);
}

/// §8.6: `types.cpp` by clang 19 (`llc` 19 and 22), clang 22 and kiln++
/// (`llc` 22; clang 22's IR does not parse in `llc` 19).
#[test]
fn types_cpp_matches_llc() {
    let mut tally = Tally::default();
    if let Some(c) = tool("clang++-19")
        && let Some(ir) = from_cxx(&c, "types-clang19", "types.cpp")
    {
        compare(19, "types-clang19", &ir, &mut tally);
        compare(22, "types-clang19", &ir, &mut tally);
    }
    if let Some(c) = tool("clang++-22")
        && let Some(ir) = from_cxx(&c, "types-clang22", "types.cpp")
    {
        compare(22, "types-clang22", &ir, &mut tally);
    }
    if let Some(k) = kiln()
        && let Some(ir) = from_cxx(&k, "types-kiln", "types.cpp")
    {
        compare(22, "types-kiln", &ir, &mut tally);
    }
    report("types.cpp", &tally);
}

/// §8.6, §7.2: `stl.cpp`, heavy in the standard library, by clang 19
/// and 22.
#[test]
fn stl_cpp_matches_llc() {
    let mut tally = Tally::default();
    if let Some(c) = tool("clang++-19")
        && let Some(ir) = from_cxx(&c, "stl-clang19", "stl.cpp")
    {
        compare(19, "stl-clang19", &ir, &mut tally);
    }
    if let Some(c) = tool("clang++-22")
        && let Some(ir) = from_cxx(&c, "stl-clang22", "stl.cpp")
    {
        compare(22, "stl-clang22", &ir, &mut tally);
    }
    report("stl.cpp", &tally);
}
