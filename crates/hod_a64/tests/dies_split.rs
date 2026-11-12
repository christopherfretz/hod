//! Split DWARF (`-gsplit-dwarf`; `spec/dwarf.md` §6.14, WP-062),
//! compared with `llc -split-dwarf-file= [-split-dwarf-output=]`:
//!
//! - the oracle: `data/dies/{vars.c,members.cpp}`,
//!   `data/dies_scope/templates.cpp` and `data/dies_inline/inline.cpp`
//!   by clang 19 (`llc` 19 and 22), clang 22 and kiln++ (`llc` 22) with
//!   `-gsplit-dwarf`: hod's object and `.dwo` (and the object of the
//!   single-file form, `.dwo` sections inside) verify, alone and as a
//!   pair, and hod's skeleton and split trees are `llc`'s, with what
//!   §6.14 lets differ masked; the two headers carry one DWO id;
//! - a `-gline-tables-only -gsplit-dwarf` unit: a skeleton without
//!   split unit, as `llc` writes it;
//! - the refusals, by name;
//! - the sizes of both files, printed against `llc`'s;
//! - the cost of the split DIEs against the full unit's (a measurement,
//!   ignored; the gate checks the DIE counts).

mod common;
#[path = "dies_support/mod.rs"]
mod support;

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use common::{ll_header, parse};
use hod_a64::{A64, ModuleError, compile_module};
use hod_cg::{CgError, Chunk, Options, Scratch as CgScratch, compile_function, emit_module};
use hod_dwarf::SplitDwarf;
use hod_testutil::{Scratch, run_with, tool};
use support::{Node, count, count_tag, first_difference, kiln, verify};

/// The test sources, by directory under `tests/data`.
const SOURCES: [(&str, &str); 4] = [
    ("dies", "vars.c"),
    ("dies", "members.cpp"),
    ("dies_scope", "templates.cpp"),
    ("dies_inline", "inline.cpp"),
];

fn data_dir(dir: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data").join(dir)
}

fn dwarfdump() -> PathBuf {
    tool("llvm-dwarfdump-22").or_else(|| tool("llvm-dwarfdump")).expect("no llvm-dwarfdump")
}

fn ok(command: &mut Command) -> String {
    let outcome = run_with(command, &[], Duration::from_mins(2));
    assert!(outcome.success(), "{command:?}: {}{}", outcome.stdout_text(), outcome.stderr_text());
    outcome.stdout_text()
}

/// `-g -O0 -gsplit-dwarf` IR of `source` by `compiler`, compiled in
/// `dir` from a copy there (a C source given to kiln++ as C++), so that
/// the unit's directory is `dir` and its `.dwo` name `<stem>.dwo`.
fn split_ir(compiler: &Path, dir: &Path, data: &str, source: &str, extra: &[&str]) -> String {
    std::fs::create_dir_all(dir).unwrap();
    let is_kiln = compiler.file_name().is_some_and(|n| n.to_string_lossy().starts_with("kiln"));
    let stem = source.split('.').next().unwrap();
    let c = support::has_ext(source, "c") && !is_kiln;
    let copy = if c { source.to_owned() } else { format!("{stem}.cpp") };
    std::fs::copy(data_dir(data).join(source), dir.join(&copy)).unwrap();
    let mut command = Command::new(compiler);
    command.args(["-g", "-O0", "-gsplit-dwarf", "-S", "-emit-llvm", "-w"]).args(extra);
    if !c {
        command.arg("-std=c++20");
    }
    command.arg(&copy).arg("-o").arg(format!("{stem}.ll")).current_dir(dir);
    ok(&mut command);
    std::fs::read_to_string(dir.join(format!("{stem}.ll"))).unwrap()
}

/// The `.dwo` name a module records.
fn dwo_name(text: &str) -> &str {
    let at = text.find("splitDebugFilename: \"").expect("no splitDebugFilename") + 21;
    let len = text[at..].find('"').unwrap();
    &text[at..at + len]
}

/// The options of `split`.
fn options(split: SplitDwarf) -> Options {
    Options { split_dwarf: split, ..Options::default() }
}

/// hod's object of `text` in `dir`, `<stem>.o`, and in the separate form
/// its `.dwo`, `<stem>.dwo`.
fn hod_split(dir: &Path, stem: &str, text: &str, split: SplitDwarf) -> (PathBuf, Option<PathBuf>) {
    let (module, bodies) = parse(text);
    let mut emitted =
        compile_module(&module, &bodies, &options(split)).unwrap_or_else(|e| panic!("{stem}: {e}"));
    let companion = emitted.object.take_dwo();
    assert_eq!(companion.is_some(), split == SplitDwarf::Separate, "{stem}: the companion");
    let object = dir.join(format!("{stem}.o"));
    std::fs::write(&object, hod_obj::elf::to_vec(&emitted.object).unwrap()).unwrap();
    let dwo = companion.map(|c| {
        let path = dir.join(format!("{stem}.dwo"));
        std::fs::write(&path, hod_obj::elf::to_vec(&c).unwrap()).unwrap();
        path
    });
    (object, dwo)
}

/// `llc`'s object of the module `<stem>.ll` in `dir` and, separate, its
/// `.dwo`.
fn llc_split(llc: &Path, dir: &Path, stem: &str, name: &str, separate: bool) -> (PathBuf, PathBuf) {
    let (object, dwo) = (dir.join(format!("{stem}.o")), dir.join(format!("{stem}.dwo")));
    let mut command = Command::new(llc);
    command.args(["-O0", "-global-isel=0", "-fast-isel=0", "-filetype=obj"]);
    command.arg(format!("-split-dwarf-file={name}"));
    if separate {
        command.arg(format!("-split-dwarf-output={}", dwo.display()));
    }
    command.arg(dir.join(format!("{stem}.ll"))).arg("-o").arg(&object);
    ok(&mut command);
    (object, dwo)
}

/// The DIE trees of `--debug-info` on `file`, the module's directory
/// `dir` as `DIR`, with what §6.14 lets differ normalized:
/// `DW_AT_GNU_pubnames` (hod writes no name index)
/// dropped, and the code of a DIE (`low_pc`/`high_pc`, or `ranges`:
/// `llc` names the code of a function or a block that does not start its
/// section by a range list in a split unit) one `DW_AT_code`. A block's
/// `DW_AT_abstract_origin`, a bare offset, is compared by its target's
/// tag, or dropped with `strip` (hod's against `llc` 19, which writes
/// none, §6.7; as `dies_inline.rs` does).
fn trees(file: &Path, dir: &Path, strip: bool) -> Vec<Node> {
    let text = ok(Command::new(dwarfdump()).arg("--debug-info").arg(file));
    let text = text.replace(&dir.display().to_string(), "DIR");
    let mut tag_at = std::collections::HashMap::new();
    for line in text.lines() {
        if let Some((off, rest)) = line.split_once(':')
            && let Some(hex) = off.strip_prefix("0x")
            && let Ok(o) = u64::from_str_radix(hex, 16)
        {
            tag_at.insert(o, rest.trim().to_owned());
        }
    }
    let mut out = String::with_capacity(text.len());
    let mut tag = "";
    for line in text.lines() {
        if let Some((off, rest)) = line.split_once(':')
            && off.starts_with("0x")
        {
            tag = rest.trim();
        }
        let t = line.trim_start();
        if t.starts_with("DW_AT_GNU_pubnames") {
            continue;
        }
        if tag == "DW_TAG_lexical_block"
            && let Some(v) = t.strip_prefix("DW_AT_abstract_origin")
            && let Some(hex) = v.trim().strip_prefix("(0x").and_then(|h| h.strip_suffix(')'))
            && let Ok(o) = u64::from_str_radix(hex, 16)
        {
            if !strip {
                let target = tag_at.get(&o).map_or("?", String::as_str);
                writeln!(out, "  DW_AT_abstract_origin (\"{target}\")").unwrap();
            }
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    let mut units = support::parse_tree(&out);
    units.iter_mut().for_each(code_as_one);
    units
}

fn code_as_one(n: &mut Node) {
    let code = ["DW_AT_low_pc", "DW_AT_high_pc", "DW_AT_ranges"];
    let before = n.attrs.len();
    let at = n.attrs.iter().position(|a| code.contains(&a.as_str()));
    n.attrs.retain(|a| !code.contains(&a.as_str()));
    if let Some(at) = at
        && n.attrs.len() < before
    {
        n.attrs.insert(at, "DW_AT_code".to_owned());
    }
    n.children.iter_mut().for_each(code_as_one);
}

/// The DWO id of every unit header of `--debug-info` on `file`.
fn dwo_ids(file: &Path) -> Vec<String> {
    let text = ok(Command::new(dwarfdump()).arg("--debug-info").arg(file));
    text.lines()
        .filter_map(|l| l.split_once("DWO_id = ").map(|(_, r)| r.split(' ').next().unwrap()))
        .map(str::to_owned)
        .collect()
}

fn same(name: &str, ours: &Node, theirs: &Node) {
    if let Some(d) = first_difference(ours, theirs) {
        panic!("{name}: hod's tree, then llc's, differ at {d}");
    }
}

fn size(path: &Path) -> u64 {
    std::fs::metadata(path).unwrap().len()
}

/// The front ends and the `llc`s that are their references.
fn frontends() -> Vec<(String, PathBuf, Vec<u32>)> {
    let mut out = Vec::new();
    for (major, llcs) in [(19, vec![19, 22]), (22, vec![22])] {
        for lang in ["clang", "clang++"] {
            if let Some(c) = tool(&format!("{lang}-{major}")) {
                out.push((format!("{lang}-{major}"), c, llcs.clone()));
            }
        }
    }
    if let Some(k) = kiln() {
        out.push(("kiln".to_owned(), k, vec![22]));
    }
    out
}

/// One source by one front end against one `llc`: both forms, verified
/// and compared. Returns hod's and `llc`'s sizes (object, `.dwo`) and the
/// split unit's DIE count.
fn one(
    scratch: &Scratch,
    tag: &str,
    compiler: &Path,
    major: u32,
    (data, source): (&str, &str),
) -> ([u64; 2], [u64; 2], usize) {
    let llc = tool(&format!("llc-{major}")).unwrap();
    let stem = source.split('.').next().unwrap();
    let (hd, ld) = (scratch.join(format!("{tag}-hod")), scratch.join(format!("{tag}-llc")));
    let text = split_ir(compiler, &hd, data, source, &[]);
    let llc_text = split_ir(compiler, &ld, data, source, &[]);
    let name = dwo_name(&text).to_owned();
    assert_eq!(name, format!("{stem}.dwo"), "{tag}");
    let dd = dwarfdump();

    // Separate: the object and its `.dwo`.
    let (ho, hdwo) = hod_split(&hd, stem, &text, SplitDwarf::Separate);
    let hdwo = hdwo.unwrap();
    let (lo, ldwo) = llc_split(&llc, &ld, stem, &name, true);
    for f in [&ho, &hdwo, &lo, &ldwo] {
        verify(&dd, f);
    }
    let strip = major < 21;
    let (hs, ls) = (trees(&ho, &hd, strip), trees(&lo, &ld, false));
    let (hu, lu) = (trees(&hdwo, &hd, strip), trees(&ldwo, &ld, false));
    assert_eq!((hs.len(), hu.len()), (1, 1), "{tag}: hod's units");
    assert_eq!((ls.len(), lu.len()), (1, 1), "{tag}: llc's units");
    assert_eq!(hs[0].tag, "DW_TAG_skeleton_unit", "{tag}");
    same(&format!("{tag}: skeleton"), &hs[0], &ls[0]);
    same(&format!("{tag}: split unit"), &hu[0], &lu[0]);
    let ids = [dwo_ids(&ho), dwo_ids(&hdwo)].concat();
    assert_eq!(ids.len(), 2, "{tag}: {ids:?}");
    assert_eq!(ids[0], ids[1], "{tag}: the skeleton's DWO id is the split unit's");
    assert_ne!(ids[0], "0x0000000000000000", "{tag}");
    // The pair, as a debugger reads it: the object with its `.dwo` beside
    // it at the name the skeleton records.
    let pair = ok(Command::new(&dd).arg("--verify").arg("--debug-info").arg(&ho).current_dir(&hd));
    assert!(pair.contains("No errors."), "{tag}: the pair:\n{pair}");
    let hod_sizes = [size(&ho), size(&hdwo)];
    let llc_sizes = [size(&lo), size(&ldwo)];

    // Single: the `.dwo` sections in the object.
    let single = hd.join("single");
    std::fs::create_dir_all(&single).unwrap();
    let (hso, none) = hod_split(&single, stem, &text, SplitDwarf::Single);
    assert!(none.is_none());
    std::fs::write(ld.join("single.ll"), &llc_text).unwrap();
    let (lso, _) = llc_split(&llc, &ld, "single", &name, false);
    verify(&dd, &hso);
    let (h, l) = (trees(&hso, &hd, strip), trees(&lso, &ld, false));
    assert_eq!((h.len(), l.len()), (2, 2), "{tag}: single units");
    same(&format!("{tag}: single skeleton"), &h[0], &l[0]);
    same(&format!("{tag}: single split unit"), &h[1], &l[1]);
    assert_eq!(h[1], hu[0], "{tag}: the split unit is the same in both forms");
    (hod_sizes, llc_sizes, count(&hu[0]))
}

#[test]
fn split_units_are_llcs() {
    let scratch = Scratch::new("dies-split");
    let (mut compared, mut dies) = (0, 0);
    let mut sizes = String::new();
    let (mut hod_total, mut llc_total) = ([0u64; 2], [0u64; 2]);
    for (name, compiler, llcs) in frontends() {
        let cxx = name != "clang-19" && name != "clang-22";
        for source in SOURCES {
            if support::has_ext(source.1, "c") == cxx && name != "kiln" {
                continue;
            }
            for major in llcs.iter().copied() {
                if tool(&format!("llc-{major}")).is_none() {
                    continue;
                }
                let stem = source.1.split('.').next().unwrap();
                let tag = format!("{stem}-{name}-llc{major}");
                let (h, l, n) = one(&scratch, &tag, &compiler, major, source);
                writeln!(
                    sizes,
                    "{tag}: object {} / {} bytes, .dwo {} / {} bytes (hod / llc)",
                    h[0], l[0], h[1], l[1]
                )
                .unwrap();
                for k in 0..2 {
                    hod_total[k] += h[k];
                    llc_total[k] += l[k];
                }
                compared += 1;
                dies += n;
            }
        }
    }
    eprint!("{sizes}");
    eprintln!(
        "split DWARF: {compared} comparisons, {dies} split DIEs; objects {} / {} bytes, .dwo {} / {} \
         bytes (hod / llc)",
        hod_total[0], llc_total[0], hod_total[1], llc_total[1]
    );
    if compared > 0 {
        assert!(dies > 20 * compared, "the sources exercise DIEs");
    }
}

/// `-gline-tables-only -gsplit-dwarf`: no split unit. `llc` writes a
/// skeleton with the unit's names and DWO id 0, and an empty `.dwo`.
#[test]
fn a_line_table_unit_has_no_split_unit() {
    let scratch = Scratch::new("dies-split-lines");
    let mut compared = 0;
    for major in [19, 22] {
        let (Some(clang), Some(llc)) =
            (tool(&format!("clang-{major}")), tool(&format!("llc-{major}")))
        else {
            continue;
        };
        let (hd, ld) = (scratch.join(format!("hod-{major}")), scratch.join(format!("llc-{major}")));
        let source = ("dies", "vars.c");
        let text = split_ir(&clang, &hd, source.0, source.1, &["-gline-tables-only"]);
        split_ir(&clang, &ld, source.0, source.1, &["-gline-tables-only"]);
        assert!(text.contains("emissionKind: LineTablesOnly"));
        let (ho, hdwo) = hod_split(&hd, "vars", &text, SplitDwarf::Separate);
        let (lo, _) = llc_split(&llc, &ld, "vars", "vars.dwo", true);
        verify(&dwarfdump(), &ho);
        let (h, l) = (trees(&ho, &hd, false), trees(&lo, &ld, false));
        assert_eq!(h.len(), 1);
        assert_eq!(h[0].tag, "DW_TAG_skeleton_unit");
        assert!(h[0].children.is_empty());
        assert!(h[0].attrs.iter().any(|a| a.starts_with("DW_AT_producer")));
        same(&format!("line tables, clang {major}"), &h[0], &l[0]);
        assert_eq!(dwo_ids(&ho), ["0x0000000000000000"]);
        // An empty `.dwo` (an object without sections of its own).
        let dwo = std::fs::read(hdwo.unwrap()).unwrap();
        assert!(!String::from_utf8_lossy(&dwo).contains(".debug_"));
        compared += 1;
    }
    eprintln!("line-table units: {compared} compared");
}

/// `data/dies/hand.ll`, with a `.dwo` name: a block with two ranges, so
/// the split unit names a `.debug_rnglists.dwo` list by
/// `DW_FORM_rnglistx`.
#[test]
fn a_block_list_goes_to_the_dwo() {
    let scratch = Scratch::new("dies-split-hand");
    let mut compared = 0;
    for major in [19, 22] {
        let Some(llc) = tool(&format!("llc-{major}")) else { continue };
        let (hd, ld) = (scratch.join(format!("hod-{major}")), scratch.join(format!("llc-{major}")));
        let text = format!("{}{}", ll_header(major), support::data("hand.ll")).replace(
            "emissionKind: FullDebug,",
            "splitDebugFilename: \"hand.dwo\", emissionKind: FullDebug,",
        );
        for dir in [&hd, &ld] {
            std::fs::create_dir_all(dir).unwrap();
            std::fs::write(dir.join("hand.ll"), &text).unwrap();
        }
        let (ho, hdwo) = hod_split(&hd, "hand", &text, SplitDwarf::Separate);
        let hdwo = hdwo.unwrap();
        let (lo, ldwo) = llc_split(&llc, &ld, "hand", "hand.dwo", true);
        for f in [&ho, &hdwo] {
            verify(&dwarfdump(), f);
        }
        let dump = ok(Command::new(dwarfdump()).arg("--debug-info").arg(&hdwo));
        assert!(dump.contains("DW_AT_ranges\t(indexed (0x0) rangelist"), "{dump}");
        let sections = ok(Command::new(dwarfdump()).arg("--debug-rnglists").arg(&hdwo));
        assert!(sections.contains("offset_entry_count = 0x00000001"), "{sections}");
        let strip = major < 21;
        same(
            &format!("hand, llc {major}: skeleton"),
            &trees(&ho, &hd, strip)[0],
            &trees(&lo, &ld, false)[0],
        );
        same(
            &format!("hand, llc {major}: split unit"),
            &trees(&hdwo, &hd, strip)[0],
            &trees(&ldwo, &ld, false)[0],
        );
        compared += 1;
    }
    eprintln!("hand.ll: {compared} compared");
}

// ---------------------------------------------------------- refusals ----

const UNIT_HEAD: &str = "!llvm.module.flags = !{!2, !3}\n\
!1 = !DIFile(filename: \"r.c\", directory: \"/w\")\n\
!2 = !{i32 7, !\"Dwarf Version\", i32 5}\n!3 = !{i32 2, !\"Debug Info Version\", i32 3}\n\
!4 = !DISubroutineType(types: !5)\n!5 = !{null}\n";

/// A module with one function `f` of unit `!0`, whose fields are
/// `fields`; `inlined`: `f`'s code is inlined from `g`.
fn unit_module(fields: &str, kind: &str, inlined: bool) -> String {
    let mut s = ll_header(22);
    s.push_str("define void @f() !dbg !10 {\n  ret void, !dbg !11\n}\n!llvm.dbg.cu = !{!0}\n");
    s.push_str(UNIT_HEAD);
    write!(
        s,
        "!0 = distinct !DICompileUnit(language: DW_LANG_C11, file: !1, producer: \"p\", \
         isOptimized: false, runtimeVersion: 0, emissionKind: {kind}{fields})\n\
         !10 = distinct !DISubprogram(name: \"f\", scope: !1, file: !1, line: 1, type: !4, \
         scopeLine: 1, spFlags: DISPFlagDefinition, unit: !0)\n"
    )
    .unwrap();
    if inlined {
        s.push_str(
            "!11 = !DILocation(line: 7, scope: !12, inlinedAt: !13)\n\
             !12 = distinct !DISubprogram(name: \"g\", scope: !1, file: !1, line: 6, type: !4, \
             scopeLine: 6, spFlags: DISPFlagDefinition, unit: !0)\n\
             !13 = !DILocation(line: 2, scope: !10)\n",
        );
    } else {
        s.push_str("!11 = !DILocation(line: 2, scope: !10)\n");
    }
    s
}

fn refusal(text: &str, split: SplitDwarf) -> Result<(), &'static str> {
    let (module, bodies) = parse(text);
    match compile_module(&module, &bodies, &options(split)) {
        Ok(_) => Ok(()),
        Err(ModuleError::Emit(CgError::Unsupported { construct })) => Err(construct),
        Err(e) => panic!("{e}"),
    }
}

#[test]
fn what_split_dwarf_refuses() {
    let named = ", splitDebugFilename: \"r.dwo\", splitDebugInlining: false";
    for split in [SplitDwarf::Single, SplitDwarf::Separate] {
        assert_eq!(refusal(&unit_module(named, "FullDebug", true), split), Ok(()));
        assert_eq!(
            refusal(&unit_module(", splitDebugFilename: \"r.dwo\"", "FullDebug", true), split),
            Err(hod_dwarf::SPLIT_INLINING)
        );
        assert_eq!(
            refusal(&unit_module(", splitDebugFilename: \"r.dwo\"", "FullDebug", false), split),
            Ok(()),
            "splitDebugInlining matters only for inlined code"
        );
        assert_eq!(
            refusal(&unit_module(", splitDebugInlining: false", "FullDebug", false), split),
            Err(hod_dwarf::SPLIT_NAME)
        );
        // Refused before split DWARF looks (§6.10); `SPLIT_REDUCED` keeps
        // it refused when `LINE_TABLES_INLINED` is lifted.
        assert_eq!(
            refusal(&unit_module(named, "LineTablesOnly", true), split),
            Err(hod_dwarf::LINE_TABLES_INLINED)
        );
        assert_eq!(refusal(&unit_module(named, "LineTablesOnly", false), split), Ok(()));
        assert_eq!(
            refusal(&unit_module(", splitDebugInlining: false", "LineTablesOnly", false), split),
            Ok(()),
            "a unit without split unit needs no name"
        );
        // Two units.
        let two = unit_module(named, "FullDebug", false).replace(
            "!llvm.dbg.cu = !{!0}",
            "define void @g() !dbg !21 {\n  ret void, !dbg !22\n}\n!llvm.dbg.cu = !{!0, !20}",
        ) + "!20 = distinct !DICompileUnit(language: DW_LANG_C11, file: !1, producer: \"p\", \
               isOptimized: false, runtimeVersion: 0, emissionKind: FullDebug, \
               splitDebugFilename: \"r.dwo\", splitDebugInlining: false)\n\
               !21 = distinct !DISubprogram(name: \"g\", scope: !1, file: !1, line: 1, type: !4, \
               scopeLine: 1, spFlags: DISPFlagDefinition, unit: !20)\n\
               !22 = !DILocation(line: 2, scope: !21)\n";
        assert_eq!(refusal(&two, split), Err(hod_dwarf::SPLIT_SEVERAL_UNITS));
    }
    // A clang module's reference, split or not.
    let module_ref = ", splitDebugFilename: \"m.pcm\", dwoId: 7";
    for split in [SplitDwarf::Off, SplitDwarf::Separate] {
        assert_eq!(
            refusal(&unit_module(module_ref, "FullDebug", false), split),
            Err(hod_dwarf::DWO_ID)
        );
    }
    // Off: what is split is ignored, as `llc` without the option does.
    assert_eq!(refusal(&unit_module(named, "FullDebug", true), SplitDwarf::Off), Ok(()));
}

// --------------------------------------------------------- the cost ----

const N: usize = 20_000;

/// `n / 10` functions of ten locals in three blocks each, in a unit
/// with `-gsplit-dwarf`'s fields.
fn generated(n: usize) -> String {
    let (mut meta, mut code) = (String::new(), String::new());
    let mut next = 10;
    for f in 0..n / 10 {
        let sp = next;
        next += 1;
        writeln!(
            meta,
            "!{sp} = distinct !DISubprogram(name: \"f{f}\", scope: !1, file: !1, line: 1, \
             type: !4, scopeLine: 1, spFlags: DISPFlagDefinition, unit: !0, retainedNodes: !7)"
        )
        .unwrap();
        writeln!(code, "define dso_local void @f{f}() !dbg !{sp} {{\nentry:").unwrap();
        for i in 0..10 {
            writeln!(code, "  %v{i} = alloca i32, align 4").unwrap();
        }
        let mut scopes = vec![sp];
        for i in 0..10 {
            while scopes.len() <= i / 4 {
                let parent = scopes[scopes.len() - 1];
                writeln!(
                    meta,
                    "!{next} = distinct !DILexicalBlock(scope: !{parent}, file: !1, line: {}, \
                     column: 3)",
                    scopes.len() + 1
                )
                .unwrap();
                scopes.push(next);
                next += 1;
            }
            let scope = scopes[i / 4];
            let (var, loc) = (next, next + 1);
            next += 2;
            writeln!(
                meta,
                "!{var} = !DILocalVariable(name: \"v{i}\", scope: !{scope}, file: !1, line: {}, \
                 type: !6)\n!{loc} = !DILocation(line: {}, column: 5, scope: !{scope})",
                i + 2,
                i + 2
            )
            .unwrap();
            writeln!(
                code,
                "    #dbg_declare(ptr %v{i}, !{var}, !DIExpression(), !{loc})\n  \
                 store i32 {i}, ptr %v{i}, align 4, !dbg !{loc}"
            )
            .unwrap();
        }
        writeln!(meta, "!{next} = !DILocation(line: 13, column: 1, scope: !{sp})").unwrap();
        writeln!(code, "  ret void, !dbg !{next}\n}}").unwrap();
        next += 1;
    }
    format!(
        "{}{code}\n!llvm.dbg.cu = !{{!0}}\n!llvm.module.flags = !{{!2, !3}}\n\
         !0 = distinct !DICompileUnit(language: DW_LANG_C11, file: !1, producer: \"gen\", \
         isOptimized: false, runtimeVersion: 0, splitDebugFilename: \"gen.dwo\", \
         emissionKind: FullDebug, splitDebugInlining: false, nameTableKind: None)\n\
         !1 = !DIFile(filename: \"gen.c\", directory: \"/hod/gen\")\n\
         !2 = !{{i32 7, !\"Dwarf Version\", i32 5}}\n\
         !3 = !{{i32 2, !\"Debug Info Version\", i32 3}}\n\
         !4 = !DISubroutineType(types: !5)\n!5 = !{{null}}\n\
         !6 = !DIBasicType(name: \"int\", size: 32, encoding: DW_ATE_signed)\n!7 = !{{}}\n{meta}",
        ll_header(22)
    )
}

/// The module emission of `text` with `split`, best of `runs`, and the
/// object and `.dwo`.
fn emit_timed(text: &str, split: SplitDwarf, runs: usize) -> (f64, Vec<u8>, Vec<u8>) {
    let (module, bodies) = parse(text);
    let options = options(split);
    let mut scratch = CgScratch::new();
    let mut chunk = Chunk::new();
    for body in &bodies {
        compile_function(&A64, &module, body, &options, &mut scratch, &mut chunk).unwrap();
    }
    let chunks = [chunk];
    let mut best = f64::MAX;
    let (mut object, mut dwo) = (Vec::new(), Vec::new());
    for _ in 0..runs {
        let start = Instant::now();
        let mut emitted = emit_module(&module, &options, &chunks).unwrap();
        best = best.min(start.elapsed().as_secs_f64());
        if let Some(c) = emitted.object.take_dwo() {
            dwo = hod_obj::elf::to_vec(&c).unwrap();
        }
        object = hod_obj::elf::to_vec(&emitted.object).unwrap();
    }
    (best, object, dwo)
}

/// At `N` the split unit has the full unit's DIEs.
#[test]
fn twenty_thousand_split() {
    let text = generated(N);
    let (_, full, _) = emit_timed(&text, SplitDwarf::Off, 1);
    let (_, skeleton, dwo) = emit_timed(&text, SplitDwarf::Separate, 1);
    assert!(!dwo.is_empty() && skeleton.len() < full.len());
    let Some(dd) = tool("llvm-dwarfdump") else { return };
    let scratch = Scratch::new("dies-split-scale");
    let (f, d) = (scratch.write("full.o", full), scratch.write("gen.dwo", dwo));
    let fulls = support::trees(&dd, &f);
    let splits = support::trees(&dd, &d);
    for tag in ["DW_TAG_subprogram", "DW_TAG_variable", "DW_TAG_lexical_block"] {
        assert_eq!(count_tag(&splits[0], tag), count_tag(&fulls[0], tag), "{tag}");
    }
    assert_eq!(count_tag(&splits[0], "DW_TAG_variable"), N);
}

#[test]
#[ignore = "a measurement; run in release with --ignored --nocapture"]
#[allow(clippy::cast_precision_loss, reason = "rates to print")]
fn split_die_cost() {
    for n in [N, 2 * N] {
        let text = generated(n);
        let bare = text.replace("emissionKind: FullDebug", "emissionKind: NoDebug");
        let (without, _, _) = emit_timed(&bare, SplitDwarf::Off, 5);
        let (full, _, _) = emit_timed(&text, SplitDwarf::Off, 5);
        let (split, _, _) = emit_timed(&text, SplitDwarf::Separate, 5);
        let dies = (n + 3 * n / 10) as f64;
        let per = |t: f64| (t - without).max(0.0) * 1e9 / dies;
        eprintln!(
            "n={n}: {:.0} ns per DIE full, {:.0} ns per DIE split ({:.2} ms, {:.2} ms, {:.2} ms \
             without)",
            per(full),
            per(split),
            full * 1e3,
            split * 1e3,
            without * 1e3
        );
    }
}
