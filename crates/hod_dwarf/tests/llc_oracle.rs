//! `spec/dwarf.md` §8.2: `llc`'s line tables, decoded into `hod_dwarf`'s
//! input, written again, and compared byte for byte; the spliced object
//! decoded and verified; the line policy against `llc`'s rows.
//!
//! `HOD_DWARF_CORPUS=<dir>` adds every `-g` module (`DICompileUnit`) under
//! `dir` (the captured corpus, `.build/corpus`) to the modules.

mod common;

use std::path::{Path, PathBuf};

use common::{
    LlcTable, TRIPLES, Written, compile, decode, dump_lines, dwarfdumps, functions, generate, hex,
    llc, policy_rows, splice, verify, with_named_differences, write,
};
use hod_dwarf::Row;
use hod_obj::SectionKind;
use hod_obj::elf::{SHF_MERGE, SHF_STRINGS, SHT_PROGBITS};
use hod_testutil::Scratch;

const MAIN: &str = include_str!("data/lines/main.ll");
const SECOND: &str = include_str!("data/lines/second.ll");

fn corpus(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let mut entries: Vec<PathBuf> = entries.filter_map(|e| e.ok().map(|e| e.path())).collect();
    entries.sort();
    for p in entries {
        if p.is_dir() {
            corpus(&p, out);
        } else if p.extension().is_some_and(|e| e == "ll") {
            out.push(p);
        }
    }
}

/// The modules: name, text, extra `llc` flags.
fn modules(major: u32) -> Vec<(String, String, Vec<&'static str>)> {
    let generated = generate::module(40, 60, 40);
    let mut out = vec![
        ("main".to_string(), MAIN.to_string(), vec![]),
        ("second".to_string(), SECOND.to_string(), vec![]),
        ("generated".to_string(), generated.clone(), vec![]),
        ("generated-fs".to_string(), generated, vec!["-function-sections"]),
    ];
    if let Some(dir) = std::env::var_os("HOD_DWARF_CORPUS") {
        let mut paths = Vec::new();
        corpus(Path::new(&dir), &mut paths);
        for p in paths {
            let wanted = format!("llvm{major}");
            if !p.components().any(|c| c.as_os_str() == wanted.as_str()) {
                continue;
            }
            let text = std::fs::read_to_string(&p).unwrap();
            // `llc` writes the version the module asks for; hod writes
            // DWARF 5 only (`spec/dwarf.md` §11), so a DWARF 4 module has
            // no table to compare.
            if text.contains("\"Dwarf Version\", i32 4") {
                eprintln!("note: {} asks for DWARF 4; not compared", p.display());
            } else if text.contains("DICompileUnit") {
                out.push((p.display().to_string(), text, vec![]));
            }
        }
    }
    out
}

#[derive(Default)]
struct Tally {
    objects: usize,
    sequences: usize,
    rows: usize,
    functions: usize,
    exact: usize,
    named: usize,
    file_stmts: usize,
    merged: usize,
    llc_bytes: usize,
    hod_bytes: usize,
}

fn llc_grouping(t: &LlcTable) -> Vec<(u32, i64, u32, Vec<Row>)> {
    t.seqs.iter().map(|s| (s.symbol, s.start, s.size, s.rows.clone())).collect()
}

fn check_written(label: &str, t: &LlcTable, w: &Written) {
    let s = &t.elf.sections;
    assert_eq!(w.line_kind, SectionKind::Debug, "{label}: .debug_line kind");
    assert_eq!((s[t.line].ty, s[t.line].flags), (SHT_PROGBITS, 0), "{label}: llc's .debug_line");
    assert_eq!(
        w.str_kind,
        SectionKind::Elf {
            sh_type: s[t.line_str].ty,
            sh_flags: s[t.line_str].flags,
            sh_entsize: s[t.line_str].entsize
        },
        "{label}: .debug_line_str type and flags"
    );
    assert_eq!(s[t.line_str].flags, SHF_MERGE | SHF_STRINGS);
    assert_eq!(w.aligns, (s[t.line].align, s[t.line_str].align), "{label}: alignments");
    assert_eq!(hex(&w.line), hex(t.elf.bytes(t.line)), "{label}: .debug_line bytes");
    assert_eq!(w.relas, t.elf.relas(t.line_rela), "{label}: .rela.debug_line");
    assert_eq!(w.line_str, t.elf.bytes(t.line_str), "{label}: .debug_line_str bytes");
}

fn oracle(major: u32) {
    let Some(llc) = llc(major) else { return };
    let dumps = dwarfdumps();
    let Some((_, dwarfdump)) = dumps.iter().find(|(m, _)| *m == major) else { return };
    let scratch = Scratch::new(&format!("dwarf-llc{major}"));
    let mut tally = Tally::default();
    for (name, ir, extra) in modules(major) {
        for (arch, triple) in TRIPLES {
            let short = name.rsplit('/').next().unwrap();
            let label = format!("{short}-{major}-{arch:?}{}", extra.join(""));
            let object = compile_with(&llc, &scratch, &label, &ir, triple, &extra);
            let t = decode(dwarfdump, &scratch, &label, &object);
            tally.objects += 1;
            tally.sequences += t.seqs.len();
            tally.rows += t.seqs.iter().map(|s| s.rows.len()).sum::<usize>();

            // Byte identity with llc's grouping, in both input orders.
            let grouping = llc_grouping(&t);
            for reverse in [false, true] {
                let w = write(&t, arch, &grouping, reverse);
                check_written(&label, &t, &w);
            }
            let w = write(&t, arch, &grouping, false);
            let patched = splice(&t, &w);
            let decoded = dump_lines(dwarfdump, &scratch, &format!("{label}.hod.o"), &patched);
            assert_eq!(decoded.rows, t.dump.rows, "{label}: decoded rows");
            for (_, d) in &dumps {
                verify(d, &scratch, &format!("{label}.hod.o"), &patched);
            }

            // The policy against llc's rows (§8.2), and hod's grouping:
            // one sequence per function.
            let mut per_function = Vec::new();
            for f in functions(&t) {
                tally.functions += 1;
                let rows = policy_rows(&f);
                if rows == f.rows {
                    tally.exact += 1;
                } else {
                    let (expected, stmts, merged) = with_named_differences(&f.rows);
                    assert_eq!(
                        rows, expected,
                        "{label}: the policy's rows of {} against llc's",
                        f.name
                    );
                    tally.named += 1;
                    tally.file_stmts += stmts;
                    tally.merged += merged;
                    if !name.starts_with("generated") {
                        eprintln!("{label}: {}: {stmts} file statements, {merged} merged", f.name);
                    }
                }
                per_function.push((f.symbol, f.start, f.size, rows));
            }
            let hod = write(&t, arch, &per_function, false);
            let patched = splice(&t, &hod);
            for (_, d) in &dumps {
                verify(d, &scratch, &format!("{label}.fn.o"), &patched);
            }
            tally.llc_bytes += t.elf.bytes(t.line).len();
            tally.hod_bytes += hod.line.len();
        }
    }
    eprintln!(
        "llc {major}: {} objects, {} sequences, {} rows byte-identical; policy: {} functions, {} as llc, {} with named differences ({} rows made statements at a file change, {} duplicate rows merged); one sequence per function: {} bytes against llc's {} ({:.3}x)",
        tally.objects,
        tally.sequences,
        tally.rows,
        tally.functions,
        tally.exact,
        tally.named,
        tally.file_stmts,
        tally.merged,
        tally.hod_bytes,
        tally.llc_bytes,
        f64::from(u32::try_from(tally.hod_bytes).unwrap())
            / f64::from(u32::try_from(tally.llc_bytes).unwrap())
    );
    assert!(tally.exact > 0);
}

fn compile_with(
    llc: &Path,
    scratch: &Scratch,
    label: &str,
    ir: &str,
    triple: &str,
    extra: &[&str],
) -> Vec<u8> {
    if extra.is_empty() {
        return compile(llc, scratch, label, ir, triple);
    }
    let input = scratch.write(&format!("{label}.ll"), ir);
    let output = scratch.join(format!("{label}.o"));
    let mut args: Vec<std::ffi::OsString> = common::LLC_FLAGS.iter().map(Into::into).collect();
    args.extend(extra.iter().map(Into::into));
    args.push(format!("-mtriple={triple}").into());
    args.push(input.into());
    args.push("-o".into());
    args.push(output.clone().into());
    let out = hod_testutil::run(llc, &args);
    assert!(out.success(), "llc {label}: {}", out.stderr_text());
    std::fs::read(output).unwrap()
}

/// §8.2 with LLVM 19.
#[test]
fn llc19_line_tables_round_trip() {
    oracle(19);
}

/// §8.2 with LLVM 22.
#[test]
fn llc22_line_tables_round_trip() {
    oracle(22);
}

#[test]
fn generated_module_is_deterministic() {
    assert_eq!(generate::module(3, 5, 2), generate::module(3, 5, 2));
}
