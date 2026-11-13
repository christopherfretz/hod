//! Unwind tables of `emit_module` against `llc` (`spec/codegen.md` §9.7,
//! `spec/eh.md` §1.1, §9.1): modules whose functions carry `uwtable`,
//! `nounwind`, personalities, landing pads and COMDATs are emitted by
//! `hod` and compiled by `llc` of each LLVM major present, and the two
//! objects must be the same, `.eh_frame` and `.gcc_except_table*`
//! included: sections, groups, symbols, relocations and bytes.
//!
//! The function bodies are not the point, so `hod`'s chunk carries
//! `llc`'s code (`borrow`) and its rows are read back from `llc`'s own
//! tables: the CFI rows from its FDEs, the call-site rows, pads, clauses
//! and type ids from its LSDAs, every padded entry split at each call of
//! a `nounwind` function inside it, and a row marked `nounwind` for each
//! such call, which `emit_module` must leave out as `llc` does. What is
//! tested is which functions get an FDE, a personality and an LSDA, the
//! sections, groups and symbols of the tables, and the wiring of each
//! function's rows to its place after the merge.
//!
//! Then the COMDAT case is linked: a COMDAT function with an FDE and an
//! LSDA defined in two objects, with each linker, and run: an exception
//! thrown through it is caught by its own handler.

mod borrow;
mod common;
mod elfview;

#[path = "../../hod_obj/tests/common/linking.rs"]
#[allow(dead_code)]
mod linking;

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use common::ehview;
use elfview::{Elf, diff};
use hod_cg::{
    CallSiteRow, CgError, Chunk, ClauseRow, NO_PAD, Options, PadRow, RelocModel, emit_module,
};
use hod_ir::{AARCH64_LINUX_DATA_LAYOUT, AARCH64_LINUX_DATA_LAYOUT_LLVM19, Module};
use hod_testutil::{LLVM_MAJORS, Scratch, run_with, tools_required};

const DECLS: &str = r"
@_ZTIi = external constant ptr
@_ZTIl = external constant ptr
declare i32 @__gxx_personality_v0(...)
declare i32 @__gcc_personality_v0(...)
declare ptr @__cxa_begin_catch(ptr)
declare void @__cxa_end_catch()
declare ptr @__cxa_allocate_exception(i64)
declare void @__cxa_throw(ptr, ptr, ptr)
declare void @quiet() nounwind
declare void @may_throw()
";

/// One case: a name, extra `llc` flags, the module text after the
/// declarations.
struct Case {
    name: &'static str,
    flags: &'static [&'static str],
    ir: &'static str,
}

/// Which functions get an FDE (`needsUnwindTableEntry`) and a
/// personality.
const ATTRS: &str = r"
define void @plain_nounwind() nounwind {
  ret void
}
define void @nounwind_uwtable() nounwind uwtable {
  ret void
}
define void @nounwind_sync() nounwind uwtable(sync) {
  ret void
}
define void @may_unwind() {
  ret void
}
define internal void @local_may_unwind() {
  ret void
}
define void @personality_no_pads() nounwind personality ptr @__gxx_personality_v0 {
  ret void
}
define void @calls_local() nounwind {
  call void @local_may_unwind()
  ret void
}
";

/// Landing pads: catches, a catch-all, a cleanup with a filter, calls of
/// a `nounwind` function between `invoke`s of one pad, a plain call that
/// can throw between `invoke`s, two pads. Each landing pad comes before
/// the `ret`, so `llc`'s epilogue is the last code. One personality: with
/// two, `llc` orders their CIEs by a pointer (`spec/eh.md` §5).
const PADS: &str = r"
define i32 @catches(i32 %x) uwtable personality ptr @__gxx_personality_v0 {
entry:
  call void @quiet()
  invoke void @may_throw() to label %next unwind label %lpad
next:
  call void @quiet()
  invoke void @may_throw() to label %next2 unwind label %lpad
next2:
  call void @quiet()
  call void @may_throw()
  invoke void @may_throw() to label %done unwind label %lpad2
lpad:
  %lp = landingpad { ptr, i32 } catch ptr @_ZTIi catch ptr null
  %e = extractvalue { ptr, i32 } %lp, 0
  %p = call ptr @__cxa_begin_catch(ptr %e)
  call void @__cxa_end_catch()
  br label %done
lpad2:
  %lp2 = landingpad { ptr, i32 } cleanup filter [1 x ptr] [ptr @_ZTIl]
  resume { ptr, i32 } %lp2
done:
  ret i32 %x
}
define void @cleanup_only() personality ptr @__gxx_personality_v0 {
entry:
  invoke void @may_throw() to label %done unwind label %lpad
lpad:
  %lp = landingpad { ptr, i32 } cleanup
  call void @quiet()
  resume { ptr, i32 } %lp
done:
  ret void
}
define void @after() {
  ret void
}
";

/// A COMDAT function with pads next to plain functions.
const COMDAT: &str = r"
$inl = comdat any
define linkonce_odr i32 @inl() comdat personality ptr @__gxx_personality_v0 {
entry:
  invoke void @may_throw() to label %done unwind label %lpad
lpad:
  %lp = landingpad { ptr, i32 } catch ptr @_ZTIi
  %e = extractvalue { ptr, i32 } %lp, 0
  %p = call ptr @__cxa_begin_catch(ptr %e)
  call void @__cxa_end_catch()
  ret i32 42
done:
  ret i32 7
}
define i32 @use_inl() {
  %r = call i32 @inl()
  ret i32 %r
}
define void @also() personality ptr @__gxx_personality_v0 {
entry:
  invoke void @may_throw() to label %done unwind label %lpad
lpad:
  %lp = landingpad { ptr, i32 } catch ptr @_ZTIl
  %e = extractvalue { ptr, i32 } %lp, 0
  %p = call ptr @__cxa_begin_catch(ptr %e)
  call void @__cxa_end_catch()
  br label %done
done:
  ret void
}
";

const CASES: &[Case] = &[
    Case { name: "attrs", flags: &[], ir: ATTRS },
    Case { name: "attrs-fs", flags: &["-function-sections"], ir: ATTRS },
    Case { name: "pads", flags: &[], ir: PADS },
    Case { name: "pads-fs", flags: &["-function-sections"], ir: PADS },
    Case { name: "comdat", flags: &[], ir: COMDAT },
    Case { name: "comdat-fs", flags: &["-function-sections"], ir: COMDAT },
];

fn header(major: u32) -> String {
    let layout =
        if major >= 20 { AARCH64_LINUX_DATA_LAYOUT } else { AARCH64_LINUX_DATA_LAYOUT_LLVM19 };
    format!("target datalayout = \"{layout}\"\ntarget triple = \"aarch64-unknown-linux-gnu\"\n")
}

fn llc_path(major: u32) -> Option<PathBuf> {
    let installed = PathBuf::from(format!("/usr/lib/llvm-{major}/bin/llc"));
    installed.is_file().then_some(installed)
}

fn llc(scratch: &Scratch, major: u32, name: &str, text: &str, flags: &[&str]) -> PathBuf {
    let input = scratch.write(&format!("{name}-{major}.ll"), text);
    let output = scratch.join(format!("{name}-{major}.o"));
    let mut command = Command::new(llc_path(major).unwrap());
    command.args(["-O0", "-global-isel=0", "-fast-isel=0", "-relocation-model=pic"]);
    command.arg("-filetype=obj").args(flags).arg(&input).arg("-o").arg(&output);
    let outcome = run_with(&mut command, &[], Duration::from_mins(1));
    assert!(outcome.success(), "{name}: llc-{major} failed:\n{}", outcome.stderr_text());
    output
}

fn section_index(elf: &Elf, name: &str) -> Option<usize> {
    elf.sections.iter().position(|s| s.name == name)
}

/// A relocation target in `llc`'s file: a symbol name, or a section.
type Target = Result<String, usize>;

fn reloc_lookup(elf: &Elf, section: usize) -> impl Fn(u64) -> Option<(Target, i64)> + '_ {
    let relocs = elf.relocs_in(section);
    move |offset| {
        relocs
            .iter()
            .find(|r| r.0 == offset)
            .map(|(_, _, target, addend)| (target.clone(), *addend))
    }
}

/// The function `llc`'s FDE starts at: the function symbol at that
/// section and offset.
fn function_at(elf: &Elf, target: &(Target, i64)) -> String {
    let (Err(section), addend) = target else { panic!("an FDE against a symbol: {target:?}") };
    elf.symbols
        .iter()
        .find(|s| s.info & 0xf == 2 && s.shndx as usize == *section && s.value as i64 == *addend)
        .unwrap_or_else(|| panic!("no function at {section}+{addend}"))
        .name
        .clone()
}

/// `chunk` with every function's rows rebuilt from `llc`'s tables.
#[allow(clippy::too_many_lines)]
fn rows_from_llc(module: &Module, chunk: &Chunk, theirs: &Elf) -> Chunk {
    let eh = section_index(theirs, ".eh_frame").expect("llc wrote .eh_frame");
    let lookup = reloc_lookup(theirs, eh);
    let (_, fdes) = ehview::eh_frame(theirs.data(eh), &lookup).expect("llc's .eh_frame decodes");
    let mut out = chunk.clone();
    out.funcs.clear();
    let mut rebuilt = Chunk::new();
    for func in &chunk.funcs {
        let name = String::from_utf8_lossy(module.symbol(func.sym).name()).into_owned();
        let mut func = func.clone();
        let fde = fdes.iter().find(|f| function_at(theirs, &f.start) == name);
        let start = rebuilt.cfi.len() as u32;
        if let Some(fde) = fde {
            rebuilt.cfi.extend_from_slice(&fde.rows);
        }
        func.cfi = start..rebuilt.cfi.len() as u32;
        let marks = (
            rebuilt.call_sites.len() as u32,
            rebuilt.pads.len() as u32,
            rebuilt.clauses.len() as u32,
            rebuilt.type_ids.len() as u32,
            rebuilt.filter_ids.len() as u32,
        );
        if let Some((Err(table), at)) = fde.and_then(|f| f.lsda.clone()) {
            func.has_pads = true;
            let lookup = reloc_lookup(theirs, table);
            let view = ehview::lsda(theirs.data(table), at as usize, 8, &lookup).expect("LSDA");
            // The type table: through each stub word to its type-info.
            for entry in &view.types {
                let sym = entry.as_ref().map(|(stub, addend)| {
                    let Err(data) = stub else { panic!("a type entry against a symbol") };
                    let found = reloc_lookup(theirs, *data)(*addend as u64).expect("a stub");
                    let Ok(name) = found.0 else { panic!("a stub against a section") };
                    module.find_symbol(name.as_bytes()).expect("a module type-info")
                });
                rebuilt.type_ids.push(sym);
            }
            // One pad per landing address and action.
            let mut keys: Vec<(u64, u64)> = Vec::new();
            let code = chunk.code_of(chunk.funcs.iter().find(|f| f.sym == func.sym).unwrap());
            let size = code.len() as u64;
            // The calls of `nounwind` functions, from the code's relocations.
            let relocs = &chunk.relocs[func.relocs.start as usize..func.relocs.end as usize];
            let quiet: Vec<u32> = relocs
                .iter()
                .filter(|r| match r.sym {
                    hod_cg::SymRef::Module(sym) => match module.symbol(sym).kind() {
                        hod_ir::SymbolKind::Function { attrs, .. } => {
                            module.fn_attrs(attrs).has(hod_ir::FnAttrs::NOUNWIND)
                        }
                        _ => false,
                    },
                    hod_cg::SymRef::Runtime(_) | hod_cg::SymRef::NoopCoroFrame => false,
                })
                .map(|r| r.offset)
                .collect();
            let mut rows: Vec<CallSiteRow> = Vec::new();
            for &(start, len, landing, action) in &view.sites {
                assert!(start + len <= size);
                let pad = if landing == 0 {
                    NO_PAD
                } else if let Some(i) = keys.iter().position(|&k| k == (landing, action)) {
                    i as u32
                } else {
                    keys.push((landing, action));
                    let first = rebuilt.clauses.len() as u32 - marks.2;
                    // Action 0: a cleanup and nothing else.
                    let mut cleanup = action == 0;
                    let chain =
                        if action == 0 { Vec::new() } else { view.chain(action).expect("a chain") };
                    for filter in chain {
                        match filter {
                            0 => cleanup = true,
                            f if f > 0 => rebuilt.clauses.push(ClauseRow::Catch(f as u32)),
                            f => {
                                let ids = view.filter(f).expect("a filter");
                                let from = rebuilt.filter_ids.len() as u32 - marks.4;
                                rebuilt.filter_ids.extend(ids.iter().map(|&id| id as u32));
                                let to = rebuilt.filter_ids.len() as u32 - marks.4;
                                rebuilt.clauses.push(ClauseRow::Filter(from..to));
                            }
                        }
                    }
                    let last = rebuilt.clauses.len() as u32 - marks.2;
                    rebuilt.pads.push(PadRow {
                        block: hod_ir::BlockId::new(keys.len() as u32),
                        offset: landing as u32,
                        cleanup,
                        clauses: first..last,
                    });
                    keys.len() as u32 - 1
                };
                // Split the entry at each quiet call inside it.
                let mut from = start as u32;
                let end = (start + len) as u32;
                let inside: Vec<u32> =
                    quiet.iter().copied().filter(|&c| c >= from && c + 4 <= end).collect();
                for call in inside {
                    if call > from {
                        rows.push(CallSiteRow {
                            start: from,
                            len: call - from,
                            pad,
                            landing: 0,
                            nounwind: false,
                        });
                    }
                    rows.push(CallSiteRow {
                        start: call,
                        len: 4,
                        pad: NO_PAD,
                        landing: 0,
                        nounwind: true,
                    });
                    from = call + 4;
                }
                if end > from {
                    rows.push(CallSiteRow {
                        start: from,
                        len: end - from,
                        pad,
                        landing: 0,
                        nounwind: false,
                    });
                }
            }
            // The quiet calls no entry covers.
            for &call in &quiet {
                if !rows.iter().any(|r| r.start <= call && call < r.start + r.len) {
                    rows.push(CallSiteRow {
                        start: call,
                        len: 4,
                        pad: NO_PAD,
                        landing: 0,
                        nounwind: true,
                    });
                }
            }
            rows.sort_by_key(|r| r.start);
            rebuilt.call_sites.extend(rows);
        }
        func.call_sites = marks.0..rebuilt.call_sites.len() as u32;
        func.pads = marks.1..rebuilt.pads.len() as u32;
        func.clauses = marks.2..rebuilt.clauses.len() as u32;
        func.type_ids = marks.3..rebuilt.type_ids.len() as u32;
        func.filter_ids = marks.4..rebuilt.filter_ids.len() as u32;
        out.funcs.push(func);
    }
    out.cfi = rebuilt.cfi;
    out.call_sites = rebuilt.call_sites;
    out.pads = rebuilt.pads;
    out.clauses = rebuilt.clauses;
    out.type_ids = rebuilt.type_ids;
    out.filter_ids = rebuilt.filter_ids;
    out
}

fn options(flags: &[&str]) -> Options {
    let function_sections = flags.contains(&"-function-sections");
    Options { reloc_model: RelocModel::Pic, function_sections, ..Options::default() }
}

/// The parsed module, `llc`'s object, and `hod`'s object from `llc`'s code
/// and tables, for `text` under `flags`.
fn both(
    scratch: &Scratch,
    major: u32,
    name: &str,
    text: &str,
    flags: &[&str],
    mutate: &dyn Fn(&mut Chunk),
) -> (PathBuf, Elf, Result<Vec<u8>, CgError>) {
    let theirs_o = llc(scratch, major, name, text, flags);
    let theirs = Elf::parse(std::fs::read(&theirs_o).unwrap());
    let parsed = hod_ll::parse_module(text.as_bytes(), &hod_ll::ParseOptions::default())
        .unwrap_or_else(|e| panic!("{name}: hod_ll: {e}"));
    let functions: Vec<_> = parsed.functions().iter().map(|f| f.sym).collect();
    let chunk = borrow::chunk_from(&parsed.module, &functions, &theirs, &|_| 4);
    let mut chunk = rows_from_llc(&parsed.module, &chunk, &theirs);
    mutate(&mut chunk);
    let ours = emit_module(&parsed.module, &options(flags), &[chunk])
        .map(|emitted| hod_obj::elf::to_vec(&emitted.object).unwrap());
    (theirs_o, theirs, ours)
}

/// `llc`'s description, without the `GCC_except_table<n>` labels it
/// defines at each LSDA and nothing refers to (`spec/eh.md` §9.6).
fn describe(elf: &Elf) -> Vec<String> {
    let mut lines = elf.describe(&|_| false);
    lines.retain(|l| !l.starts_with("symbol GCC_except_table"));
    lines
}

/// The differences between `hod`'s and `llc`'s objects.
fn compare(ours: &Elf, theirs: &Elf) -> Vec<String> {
    let (only_ours, only_theirs) = diff(&describe(ours), &describe(theirs));
    let mut out: Vec<String> = only_ours.into_iter().map(|l| format!("hod: {l}")).collect();
    out.extend(only_theirs.into_iter().map(|l| format!("llc: {l}")));
    out
}

fn majors() -> Vec<u32> {
    let majors: Vec<u32> = LLVM_MAJORS.into_iter().filter(|&m| llc_path(m).is_some()).collect();
    if majors.is_empty() {
        assert!(!tools_required(), "no llc (HOD_REQUIRE_TOOLS=1)");
        eprintln!("note: no llc; skipping");
    }
    majors
}

/// §1.1, §9.1: the objects are the same, tables included.
#[test]
fn unwind_tables_agree_with_llc() {
    let scratch = Scratch::new("emit-eh");
    let mut failures = Vec::new();
    let (mut fdes, mut lsdas) = (0, 0);
    for major in majors() {
        for case in CASES {
            let text = format!("{}{DECLS}{}", header(major), case.ir);
            let (_, theirs, ours) = both(&scratch, major, case.name, &text, case.flags, &|_| {});
            let ours = Elf::parse(ours.unwrap_or_else(|e| panic!("{}: {e}", case.name)));
            let differences = compare(&ours, &theirs);
            let eh = section_index(&theirs, ".eh_frame").unwrap();
            let (_, decoded) =
                ehview::eh_frame(theirs.data(eh), &reloc_lookup(&theirs, eh)).unwrap();
            fdes += decoded.len();
            lsdas += decoded.iter().filter(|f| f.lsda.is_some()).count();
            let verdict = if differences.is_empty() { "agree" } else { "DIFFER" };
            eprintln!(
                "emit_eh: LLVM {major} {:<10} {verdict} ({} FDEs, {} LSDAs)",
                case.name,
                decoded.len(),
                decoded.iter().filter(|f| f.lsda.is_some()).count()
            );
            if !differences.is_empty() {
                failures.push(format!(
                    "{} (LLVM {major}):\n  {}",
                    case.name,
                    differences.join("\n  ")
                ));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    eprintln!("emit_eh: {fdes} FDEs and {lsdas} LSDAs byte-identical to llc's");
}

/// The wiring bugs this test exists to catch, each planted in the rows
/// `emit_module` reads or in what it is given: the comparison must fail.
/// (The same bugs planted in `src/emit/unwind.rs` were caught by the
/// comparison too: `spec/landed/codegen/WP-049.md`.)
#[test]
fn wrong_wiring_fails_the_comparison() {
    let Some(&major) = majors().last() else { return };
    let scratch = Scratch::new("emit-eh-wrong");
    let text = format!("{}{DECLS}{COMDAT}", header(major));
    // A function's code one word later than its rows say.
    let shifted = |chunk: &mut Chunk| {
        let f = &mut chunk.funcs[0];
        for row in &mut chunk.cfi[f.cfi.start as usize..f.cfi.end as usize] {
            row.offset += 4;
        }
    };
    // A COMDAT function that gets no FDE.
    let dropped = |chunk: &mut Chunk| {
        let f = &mut chunk.funcs[0];
        (f.nounwind, f.uwtable, f.personality, f.has_pads) = (true, false, None, false);
    };
    // Two functions' LSDA rows exchanged.
    let swapped = |chunk: &mut Chunk| {
        let (a, b) = (chunk.funcs[0].type_ids.clone(), chunk.funcs[2].type_ids.clone());
        chunk.funcs[0].type_ids = b;
        chunk.funcs[2].type_ids = a;
    };
    for (what, mutate) in [
        ("shifted rows", &shifted as &dyn Fn(&mut Chunk)),
        ("dropped COMDAT FDE", &dropped),
        ("swapped LSDA rows", &swapped),
    ] {
        let (_, theirs, ours) = both(&scratch, major, "wrong", &text, &[], mutate);
        let ours = Elf::parse(ours.unwrap());
        assert!(!compare(&ours, &theirs).is_empty(), "{what}: the comparison did not fail");
    }
}

/// `spec/object.md` "References into a group from outside it": a COMDAT
/// function with an FDE and an LSDA defined in two objects, linked with
/// each linker without a diagnostic; the program throws through the kept
/// copy and catches in it, and its tables decode.
#[test]
fn comdat_tables_link_with_every_linker() {
    let Some(&major) = majors().last() else { return };
    let scratch = Scratch::new("emit-eh-link");
    // Natively on AArch64, under qemu-aarch64 elsewhere.
    let Some(toolchain) = linking::Toolchain::for_target(&scratch, hod_testutil::Target::Aarch64)
    else {
        return;
    };
    let thrower = r"
define void @may_throw() {
  %e = call ptr @__cxa_allocate_exception(i64 4)
  store i32 5, ptr %e
  call void @__cxa_throw(ptr %e, ptr @_ZTIi, ptr null)
  unreachable
}
define void @quiet() nounwind {
  ret void
}
";
    let a = format!("{}{DECLS}{COMDAT}", header(major));
    let b = format!(
        "{}{DECLS}{}define i32 @use_inl_b() {{\n  %r = call i32 @inl()\n  ret i32 %r\n}}\n",
        header(major),
        &COMDAT[..COMDAT.find("define i32 @use_inl()").unwrap()]
    );
    let t = format!("{}{DECLS}", header(major))
        .replace("declare void @quiet() nounwind\ndeclare void @may_throw()\n", "")
        + thrower;
    let mut objects = Vec::new();
    for (name, text) in [("a", &a), ("b", &b), ("t", &t)] {
        let (_, _, ours) = both(&scratch, major, name, text, &[], &|_| {});
        objects.push(scratch.write(&format!("{name}-hod.o"), ours.unwrap()));
    }
    let main = toolchain.compile_c(
        &scratch,
        "main",
        "#include <stdio.h>\nint use_inl(void); int use_inl_b(void);\n\
         int main(void) { printf(\"%d %d\\n\", use_inl(), use_inl_b()); return 0; }\n",
        &[],
    );
    // The library after the objects that need it.
    let inputs: Vec<&Path> = std::iter::once(main.as_path())
        .chain(objects.iter().map(PathBuf::as_path))
        .chain(std::iter::once(Path::new("-lstdc++")))
        .collect();
    let mut linked = 0;
    for linker in &toolchain.linkers {
        let program = scratch.join(format!("comdat-{}", linker.name));
        let ran = toolchain.link_and_run(linker, &[], &inputs, &program);
        assert_eq!(linking::exit_code(&ran), Some(0), "{}: {}", linker.name, ran.stderr_text());
        assert_eq!(ran.stdout_text(), "42 42\n", "{}", linker.name);
        for tool in
            [("readelf", &["--debug-dump=frames"][..]), ("llvm-dwarfdump", &["--eh-frame"][..])]
        {
            let Some(path) = hod_testutil::tool(tool.0) else { continue };
            let mut command = Command::new(path);
            command.args(tool.1).arg(&program);
            let out = run_with(&mut command, &[], Duration::from_mins(1));
            let text = format!("{}{}", out.stdout_text(), out.stderr_text());
            assert!(out.success(), "{}: {} failed", linker.name, tool.0);
            for bad in ["warning", "error", "Warning", "Error", "invalid"] {
                assert!(!text.contains(bad), "{}: {} says {bad}:\n{text}", linker.name, tool.0);
            }
            assert!(text.contains("FDE"), "{}: {} found no FDE", linker.name, tool.0);
        }
        linked += 1;
    }
    let mut note = String::new();
    let _ = write!(note, "emit_eh: the COMDAT program ran under {linked} linkers");
    eprintln!("{note}");
}

/// The module and bodies of `text`.
fn parse(text: &str) -> (Module, Vec<hod_ir::Body>) {
    let parsed = hod_ll::parse_module(text.as_bytes(), &hod_ll::ParseOptions::default())
        .unwrap_or_else(|e| panic!("hod_ll: {e}"));
    let (side, mut module) = (parsed.text, parsed.module);
    let mut bodies = Vec::new();
    hod_ll::parse_bodies(&mut module, &side, |_, parsed| {
        bodies.push(parsed.body.expect("a body").clone());
        Ok::<(), hod_ll::LlError>(())
    })
    .unwrap_or_else(|e| panic!("hod_ll: {e}"));
    (module, bodies)
}

/// `spec/eh.md` §9.3: a direct call of a function declared `nounwind`
/// gets a row marked `nounwind`, and no LSDA entry of its own; any other
/// call keeps its entry. A call through a pointer is not one. (A gap
/// entry may run on across a `nounwind` call, as `llc`'s does, so only
/// the call before every other has nothing covering it.)
#[test]
fn calls_of_nounwind_functions_get_no_entry() {
    let text = format!(
        "{}{DECLS}{}",
        header(22),
        r"
define void @f(ptr %fp) personality ptr @__gxx_personality_v0 {
entry:
  call void @quiet()
  invoke void @may_throw() to label %next unwind label %lpad
next:
  call void @may_throw()
  call void %fp()
  call void @quiet()
  ret void
lpad:
  %lp = landingpad { ptr, i32 } cleanup
  resume { ptr, i32 } %lp
}
"
    );
    let (module, bodies) = parse(&text);
    let mut chunk = Chunk::new();
    let tm = common::target::Tm::default();
    for body in &bodies {
        hod_cg::compile_function(
            &tm,
            &module,
            body,
            &Options::default(),
            &mut hod_cg::Scratch::new(),
            &mut chunk,
        )
        .unwrap();
    }
    let flags: Vec<bool> = chunk.call_sites.iter().map(|row| row.nounwind).collect();
    // `quiet`, the `invoke`, `may_throw`, the pointer, `quiet`, and the
    // call of `_Unwind_Resume` (a runtime function).
    assert_eq!(flags, [true, false, false, false, true, false], "{:?}", chunk.call_sites);
    let emitted = emit_module(&module, &Options::default(), &[chunk.clone()]).unwrap();
    let tables = common::exec::Tables::decode(&emitted).unwrap();
    assert_eq!((tables.fdes, tables.lsdas), (1, 1));
    let obj = &emitted.object;
    let section = ehview::find_section(obj, b".gcc_except_table").unwrap();
    let reloc = |offset| ehview::object_reloc(obj, section, offset);
    let view = ehview::lsda(obj.data(section), 0, 8, &reloc).unwrap();
    let covered = |row: &CallSiteRow| {
        view.sites.iter().any(|&(start, len, _, _)| {
            start <= u64::from(row.start) && u64::from(row.start + row.len) <= start + len
        })
    };
    for row in &chunk.call_sites {
        assert!(covered(row) || row.nounwind, "{row:?} in {:?}", view.sites);
    }
    assert!(!covered(&chunk.call_sites[0]), "{:?}", view.sites);
    assert_eq!(view.sites.len(), 2, "the invoke, and one gap entry for the rest");
}

/// `spec/codegen.md` §9.7: a linker option that is not a pair is refused
/// by name (`llc` writes `.linker-options` as pairs).
#[test]
fn a_linker_option_that_is_not_a_pair_is_refused() {
    for list in [&[&b"lib"[..]][..], &[b"a", b"b", b"c"]] {
        let mut module = Module::new(hod_ir::TargetSpec::aarch64_linux_gnu());
        module.add_linker_option(list).unwrap();
        let result = emit_module(&module, &Options::default(), &[Chunk::new()]);
        assert_eq!(result.err(), Some(CgError::unsupported("module:linker-option-not-a-pair")));
    }
    let mut module = Module::new(hod_ir::TargetSpec::aarch64_linux_gnu());
    module.add_linker_option(&[b"key", b"value"]).unwrap();
    assert!(emit_module(&module, &Options::default(), &[Chunk::new()]).is_ok());
}

/// `spec/eh.md` §1.1: a function with a personality LLVM does not know
/// and no landing pads is refused by name: `llc` writes its personality
/// and an LSDA of call sites hod does not record. A known one gets
/// neither.
#[test]
fn an_unknown_personality_without_pads_is_refused() {
    let Some(&major) = majors().last() else { return };
    let scratch = Scratch::new("emit-eh-personality");
    let text = format!(
        "{}{DECLS}declare i32 @my_personality(...)\n\
         define void @f() personality ptr @my_personality {{\n  ret void\n}}\n",
        header(major)
    );
    // `llc` wrote an LSDA; the rows are those of a function without pads.
    let no_pads = |chunk: &mut Chunk| {
        let f = &mut chunk.funcs[0];
        (f.has_pads, f.call_sites) = (false, 0..0);
    };
    let (_, _, ours) = both(&scratch, major, "unknown", &text, &[], &no_pads);
    assert_eq!(ours.err(), Some(CgError::unsupported("eh:unknown-personality-without-pads")));
}

/// `spec/codegen.md` §6.1, §9.3: the shared trap after the exit
/// sequences, where a block whose address is taken and that nothing
/// reaches is bound, is reached from the body, so its rows restate the
/// body's: the CFA rule the epilogue's rows had changed.
#[test]
fn the_shared_trap_restates_the_body_rows() {
    let text = format!(
        "{}@addr = global ptr blockaddress(@g, %dead)\n\
         define i64 @g(ptr %p) {{\nentry:\n  indirectbr ptr %p, [label %r]\nr:\n  ret i64 1\n\
         dead:\n  ret i64 2\n}}\n",
        header(22)
    );
    let (module, bodies) = parse(&text);
    let mut chunk = Chunk::new();
    let tm = common::target::Tm::default();
    hod_cg::compile_function(
        &tm,
        &module,
        &bodies[0],
        &Options::default(),
        &mut hod_cg::Scratch::new(),
        &mut chunk,
    )
    .unwrap();
    let fp = u16::from(common::isa::FP);
    let last = chunk.cfi.last().unwrap().offset;
    let trap: Vec<_> =
        chunk.cfi.iter().filter(|row| row.offset == last).map(|row| row.op).collect();
    assert!(
        trap.contains(&hod_cg::CfiOp::DefCfa { reg: fp, offset: 16 }),
        "the rows at the trap: {:?}",
        chunk.cfi
    );
    assert!(last < chunk.code.len() as u32, "{:?}", chunk.cfi);
}
