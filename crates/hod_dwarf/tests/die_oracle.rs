//! `spec/dwarf.md` §8.6: `llc`'s DIEs against hod's, on globals-only
//! modules (`-O0 -global-isel=0 -fast-isel=0`, both targets, every
//! installed major). hod's debug sections are spliced into `llc`'s object
//! (so they name `llc`'s symbols); both are verified and dumped, the DIE
//! trees compared after the normalization §6.7 names, and the targets of
//! the relocations compared.

#![allow(clippy::cast_precision_loss)]

mod common;

use common::dies::{
    addr_targets, count, die_section_list, dump, globals_module, hod_sections, parse_tree, render,
    sorted, splice, target_name, verify_all,
};
use common::meta::load;
use common::{Elf, TRIPLES, compile, dwarfdumps, llc};
use hod_testutil::Scratch;

fn modules() -> Vec<(&'static str, String)> {
    let data = |n: &str| {
        let path = format!("{}/tests/data/dies/{n}", env!("CARGO_MANIFEST_DIR"));
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
    };
    vec![
        ("globals", data("globals.ll")),
        ("strings", data("strings.ll")),
        ("many", globals_module(2000)),
    ]
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

/// What each relocation of a debug section of `elf` names, sorted: the
/// target section and addend, or for `.debug_str_offsets` the string.
fn reloc_targets(elf: &Elf, section: &str) -> Vec<String> {
    let Some(index) = elf.section(section) else { return Vec::new() };
    let Some(rela) = elf.rela_of(index) else { return Vec::new() };
    let strs = elf.section(".debug_str").map(|s| elf.bytes(s));
    let mut out: Vec<String> = elf
        .relas(rela)
        .iter()
        .map(|r| {
            let target = target_name(elf, r.sym, r.addend);
            match (section, strs) {
                (".debug_str_offsets", Some(s)) => {
                    let at = r.addend as usize;
                    let end = s[at..].iter().position(|&b| b == 0).unwrap();
                    String::from_utf8_lossy(&s[at..at + end]).into_owned()
                }
                _ => format!("{target} type {}", r.ty),
            }
        })
        .collect();
    out.sort();
    out
}

#[derive(Default)]
struct Tally {
    objects: usize,
    dies: usize,
    same_order: usize,
    llc_bytes: u64,
    hod_bytes: u64,
    /// Per section of `COMPARED`: (llc, hod).
    sections: [(u64, u64); 5],
}

const COMPARED: [&str; 5] =
    [".debug_abbrev", ".debug_info", ".debug_str_offsets", ".debug_str", ".debug_addr"];

fn size(elf: &Elf) -> u64 {
    COMPARED.iter().filter_map(|n| elf.section(n)).map(|i| elf.sections[i].size).sum()
}

fn oracle(major: u32) {
    let Some(llc) = llc(major) else { return };
    let dumps = dwarfdumps();
    let Some((_, dwarfdump)) = dumps.iter().find(|(m, _)| *m == major) else { return };
    let scratch = Scratch::new(&format!("dies-llc{major}"));
    let mut tally = Tally::default();
    for (name, ir) in modules() {
        let loaded = load(&ir);
        let ir = retarget(&ir);
        for (arch, triple) in TRIPLES {
            let label = format!("{name}-{major}-{arch:?}");
            let object = compile(&llc, &scratch, &label, &ir, triple);
            let elf = Elf::parse(object.clone());
            let (obj, line, dies) = hod_sections(&elf, arch, &loaded, |_| {});
            let patched = splice(&elf, &obj, &die_section_list(line, &dies));
            let ours = Elf::parse(patched.clone());
            verify_all(dwarfdump, &scratch, &format!("{label}.orig.o"), &object);
            verify_all(dwarfdump, &scratch, &format!("{label}.hod.o"), &patched);

            let theirs_text =
                dump(dwarfdump, &scratch, &format!("{label}.orig.o"), &object, &["--debug-info"]);
            let ours_text =
                dump(dwarfdump, &scratch, &format!("{label}.hod.o"), &patched, &["--debug-info"]);
            let theirs = parse_tree(&theirs_text, &addr_targets(&elf));
            let ours_tree = parse_tree(&ours_text, &addr_targets(&ours));
            assert_eq!(theirs.len(), 1, "{label}: one unit");
            assert_eq!(ours_tree.len(), 1, "{label}: one unit");
            let (a, b) = (sorted(&theirs[0]), sorted(&ours_tree[0]));
            if a != b {
                let (mut x, mut y) = (String::new(), String::new());
                render(&a, 0, &mut x);
                render(&b, 0, &mut y);
                let first = x.lines().zip(y.lines()).position(|(p, q)| p != q).unwrap_or(0);
                panic!(
                    "{label}: DIE trees differ at line {first}\nllc:\n{}\nhod:\n{}",
                    x.lines().skip(first.saturating_sub(5)).take(20).collect::<Vec<_>>().join("\n"),
                    y.lines().skip(first.saturating_sub(5)).take(20).collect::<Vec<_>>().join("\n"),
                );
            }
            // The comparison sees a wrong address.
            if name == "globals" {
                let (obj, line, dies) = hod_sections(&elf, arch, &loaded, |g| g.addend += 4);
                let wrong = splice(&elf, &obj, &die_section_list(line, &dies));
                let text = dump(
                    dwarfdump,
                    &scratch,
                    &format!("{label}.wrong.o"),
                    &wrong,
                    &["--debug-info"],
                );
                let tree = parse_tree(&text, &addr_targets(&Elf::parse(wrong.clone())));
                assert_ne!(sorted(&tree[0]), a, "{label}: a wrong address is caught");
            }
            tally.objects += 1;
            tally.dies += count(&a);
            tally.same_order += usize::from(theirs[0] == ours_tree[0]);

            for section in [".debug_info", ".debug_str_offsets", ".debug_addr"] {
                assert_eq!(
                    reloc_targets(&elf, section),
                    reloc_targets(&ours, section),
                    "{label}: targets of the relocations of {section}"
                );
            }
            for (k, n) in COMPARED.iter().enumerate() {
                let of = |e: &Elf| e.section(n).map_or(0, |i| e.sections[i].size);
                tally.sections[k].0 += of(&elf);
                tally.sections[k].1 += of(&ours);
            }
            tally.llc_bytes += size(&elf);
            tally.hod_bytes += size(&ours);
        }
    }
    eprintln!(
        "llc {major}: {} objects, {} DIEs equal after normalization, in llc's order (siblings and attributes) in {}; \
         DIE section bytes hod/llc {}/{} = {:.3}",
        tally.objects,
        tally.dies,
        tally.same_order,
        tally.hod_bytes,
        tally.llc_bytes,
        tally.hod_bytes as f64 / tally.llc_bytes as f64
    );
    for (n, (l, h)) in COMPARED.iter().zip(tally.sections) {
        eprintln!("llc {major}: {n}: hod {h}, llc {l}");
    }
}

/// §8.6 with LLVM 19.
#[test]
fn llc19_dies_match() {
    oracle(19);
}

/// §8.6 with LLVM 22.
#[test]
fn llc22_dies_match() {
    oracle(22);
}
