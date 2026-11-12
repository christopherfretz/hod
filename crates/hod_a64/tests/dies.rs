//! Function DIEs (`spec/dwarf.md` §6.4, §8.7): subprograms, parameters,
//! locals, lexical blocks and labels, compared with `llc`'s.
//!
//! - the oracle: `data/dies/vars.c` and `members.cpp` by clang 19
//!   (`llc` 19 and 22), clang 22 and kiln++ (`llc` 22), and the
//!   hand-written `data/dies/hand.ll`: hod's DIE trees are `llc`'s, in
//!   `llc`'s order, modulo what §8.7 masks (addresses, locations, the
//!   frame base); both objects verify;
//! - the hand-written cases one by one: a block with two ranges, a
//!   variable whose scope has no code, a label in a block without
//!   variables, an unused parameter;
//! - each refusal by name, in a module with that one construct;
//! - a global of function-pointer type;
//! - determinism over chunks and threads;
//! - the corpus survey (with `HOD_DIES_CORPUS`, a note otherwise).

mod common;
#[path = "dies_support/mod.rs"]
mod support;

use std::process::Command;
use std::time::Duration;

use common::{ll_header, parse};
use hod_a64::{A64, ModuleError, compile_to_object};
use hod_cg::{CgError, Chunk, Options, Scratch as CgScratch, compile_function, emit_module};
use hod_ir::Body;
use hod_testutil::{Scratch, run_with, tool};
use support::{
    Node, count, count_tag, data, first_difference, hod_object, ir, kiln, llc_object, trees, verify,
};

/// Exploration aid: compiles `HOD_DIES_IN` to `HOD_DIES_OUT`.
#[test]
#[ignore = "an aid for exploring by hand"]
fn compile_by_hand() {
    let (Ok(input), Ok(output)) = (std::env::var("HOD_DIES_IN"), std::env::var("HOD_DIES_OUT"))
    else {
        return;
    };
    let text = std::fs::read_to_string(&input).unwrap();
    let (module, bodies) = parse(&text);
    let bytes = compile_to_object(&module, &bodies, &Options::default())
        .unwrap_or_else(|e| panic!("{input}: {e}"));
    std::fs::write(&output, bytes).unwrap();
}

/// `hand.ll` for LLVM `major`.
fn hand(major: u32) -> String {
    format!("{}{}", ll_header(major), data("hand.ll"))
}

#[derive(Default)]
struct Tally {
    objects: usize,
    dies: usize,
    subprograms: usize,
    variables: usize,
}

/// hod's tree of `text` against `llc` `major`'s, both verified.
fn compare(scratch: &Scratch, major: u32, name: &str, text: &str, tally: &mut Tally) -> Node {
    let Some(llc) = tool(&format!("llc-{major}")) else { panic!("no llc-{major}") };
    let Some(dwarfdump) = tool("llvm-dwarfdump-22").or_else(|| tool("llvm-dwarfdump")) else {
        panic!("no llvm-dwarfdump")
    };
    let h = hod_object(scratch, &format!("{name}-hod"), text);
    let l = llc_object(scratch, &llc, &format!("{name}-llc{major}"), text);
    verify(&dwarfdump, &h);
    verify(&dwarfdump, &l);
    let (th, tl) = (trees(&dwarfdump, &h), trees(&dwarfdump, &l));
    assert_eq!(th.len(), 1, "{name}: one unit");
    assert_eq!(tl.len(), 1, "{name}: llc's units");
    if let Some(d) = first_difference(&th[0], &tl[0]) {
        panic!("{name} (llc {major}): hod's tree, then llc's, differ at {d}");
    }
    tally.objects += 2;
    tally.dies += count(&th[0]);
    tally.subprograms += count_tag(&th[0], "DW_TAG_subprogram");
    tally.variables +=
        count_tag(&th[0], "DW_TAG_variable") + count_tag(&th[0], "DW_TAG_formal_parameter");
    th.into_iter().next().unwrap()
}

/// §8.7: the C and C++ sources by every compiler, and `hand.ll`.
#[test]
fn trees_match_llc() {
    let scratch = Scratch::new("dies-oracle");
    let mut tally = Tally::default();
    let Some(clang19) = tool("clang-19") else { return };
    let Some(clang22) = tool("clang-22") else { return };
    let Some(clangxx19) = tool("clang++-19") else { return };
    let Some(clangxx22) = tool("clang++-22") else { return };
    for (source, c19, c22) in
        [("vars.c", &clang19, &clang22), ("members.cpp", &clangxx19, &clangxx22)]
    {
        let stem = source.split('.').next().unwrap();
        let text = ir(c19, &scratch, &format!("{stem}-c19"), source);
        for major in [19, 22] {
            compare(&scratch, major, &format!("{stem}-c19"), &text, &mut tally);
        }
        let text = ir(c22, &scratch, &format!("{stem}-c22"), source);
        compare(&scratch, 22, &format!("{stem}-c22"), &text, &mut tally);
        if let Some(k) = kiln() {
            let text = ir(&k, &scratch, &format!("{stem}-kiln"), source);
            compare(&scratch, 22, &format!("{stem}-kiln"), &text, &mut tally);
        }
    }
    for major in [19, 22] {
        compare(&scratch, major, &format!("hand{major}"), &hand(major), &mut tally);
    }
    assert!(tally.subprograms >= 60, "{} subprograms", tally.subprograms);
    eprintln!(
        "trees equal to llc's: {} objects, {} DIEs of hod's ({} subprograms, {} variables)",
        tally.objects, tally.dies, tally.subprograms, tally.variables
    );
}

fn find<'n>(n: &'n Node, tag: &str, name: &str) -> Option<&'n Node> {
    let named = format!("DW_AT_name (\"{name}\")");
    if n.tag == tag && n.attrs.iter().any(|a| a == &named) {
        return Some(n);
    }
    n.children.iter().find_map(|c| find(c, tag, name))
}

fn tags(n: &Node) -> Vec<&str> {
    n.children.iter().map(|c| c.tag.as_str()).collect()
}

/// The hand-written cases, one by one (§6.4).
#[test]
fn hand_written_cases() {
    let Some(dwarfdump) = tool("llvm-dwarfdump-22").or_else(|| tool("llvm-dwarfdump")) else {
        return;
    };
    let scratch = Scratch::new("dies-hand");
    let object = hod_object(&scratch, "hand", &hand(22));
    verify(&dwarfdump, &object);
    let unit = trees(&dwarfdump, &object).remove(0);
    let sp = |name| find(&unit, "DW_TAG_subprogram", name).unwrap_or_else(|| panic!("{name}"));

    // A block interrupted by its parent's code: two ranges.
    let split = sp("split");
    assert_eq!(tags(split), ["DW_TAG_formal_parameter", "DW_TAG_variable", "DW_TAG_lexical_block"]);
    let block = &split.children[2];
    assert!(block.attrs.iter().any(|a| a == "DW_AT_ranges"), "{block:?}");
    let mut command = Command::new(&dwarfdump);
    command.args(["--debug-info", "--name=split", "-c"]).arg(&object);
    let out = run_with(&mut command, &[], Duration::from_mins(1)).stdout_text();
    let ranges = out.split("DW_AT_ranges").nth(1).unwrap_or("");
    let listed = ranges.lines().take(4).filter(|l| l.trim_start().starts_with("[0x")).count();
    assert_eq!(listed, 2, "two ranges:\n{out}");

    // The variable of a block with no code is dropped, and so is the
    // block.
    assert_eq!(tags(sp("declared_only")), ["DW_TAG_formal_parameter"]);
    // A label in a block without variables goes to its parent; an
    // unused parameter named by a declare is written.
    assert_eq!(
        tags(sp("with_label")),
        ["DW_TAG_formal_parameter", "DW_TAG_formal_parameter", "DW_TAG_label"]
    );
    assert!(find(sp("with_label"), "DW_TAG_formal_parameter", "unused").is_some());
    assert!(sp("no_vars").children.is_empty());
}

/// A module with a function `f` whose body and metadata are `body` and
/// `meta`; `!14` is a local `a` of `f`, `!15` a location in it.
fn one_function(body: &str, meta: &str) -> String {
    format!(
        "{}\
define dso_local void @f(i32 %v) !dbg !10 {{
entry:
  %a = alloca i32, align 4
{body}
  ret void, !dbg !15
}}

!llvm.dbg.cu = !{{!0}}
!llvm.module.flags = !{{!2, !3}}

!0 = distinct !DICompileUnit(language: DW_LANG_C_plus_plus_14, file: !1, producer: \"hand\", isOptimized: false, runtimeVersion: 0, emissionKind: FullDebug, splitDebugInlining: false, nameTableKind: None)
!1 = !DIFile(filename: \"r.cpp\", directory: \"/hod/tests/dies\")
!2 = !{{i32 7, !\"Dwarf Version\", i32 5}}
!3 = !{{i32 2, !\"Debug Info Version\", i32 3}}
!4 = !{{}}
!6 = !DIBasicType(name: \"int\", size: 32, encoding: DW_ATE_signed)
!11 = !DISubroutineType(types: !12)
!12 = !{{null, !6}}
!14 = !DILocalVariable(name: \"a\", scope: !10, file: !1, line: 2, type: !6)
!15 = !DILocation(line: 2, column: 3, scope: !10)
{meta}",
        ll_header(22)
    )
}

const PLAIN_SP: &str = "!10 = distinct !DISubprogram(name: \"f\", linkageName: \"_Z1fi\", scope: !1, file: !1, line: 1, type: !11, scopeLine: 1, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !0, retainedNodes: !4)\n";

fn refusal(text: &str) -> CgError {
    let (module, bodies) = parse(text);
    match compile_to_object(&module, &bodies, &Options::default()) {
        Err(ModuleError::Emit(e) | ModuleError::Function { error: e, .. }) => e,
        other => panic!("{:?}\n{text}", other.map(|b| b.len())),
    }
}

/// Each refusal of §6.4, in a module with that one construct.
#[test]
fn refusals_are_named() {
    let declare = "    #dbg_declare(ptr %a, !14, !DIExpression(), !15)";
    // The base module compiles: every case below differs from it by one
    // construct.
    let base = one_function(declare, PLAIN_SP);
    let (module, bodies) = parse(&base);
    compile_to_object(&module, &bodies, &Options::default()).expect("the base case");

    let cases: [(&str, String, &str); 7] = [
        (
            "debug:dbg-value",
            one_function("    #dbg_value(i32 %v, !14, !DIExpression(), !15)", PLAIN_SP),
            "#dbg_value",
        ),
        (
            "debug:dbg-assign",
            one_function(
                "  store i32 %v, ptr %a, align 4, !DIAssignID !20\n    #dbg_assign(i32 %v, !14, !DIExpression(), !20, ptr %a, !DIExpression(), !15)",
                &format!("{PLAIN_SP}!20 = distinct !DIAssignID()\n"),
            ),
            "#dbg_assign",
        ),
        (
            "debug:fragment",
            one_function(
                "    #dbg_declare(ptr %a, !14, !DIExpression(DW_OP_LLVM_fragment, 0, 16), !15)",
                PLAIN_SP,
            ),
            "a fragment",
        ),
        (
            "debug:declare-address",
            one_function(
                "  %p = getelementptr i8, ptr %a, i64 0\n    #dbg_declare(ptr %p, !14, !DIExpression(), !15)",
                PLAIN_SP,
            ),
            "a declare of an address that is not a frame slot",
        ),
        (
            "debug:out-of-line-definition",
            one_function(
                declare,
                "!10 = distinct !DISubprogram(name: \"f\", linkageName: \"_ZN1n1fEi\", scope: !30, file: !1, line: 1, type: !11, scopeLine: 1, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !0, declaration: !31, retainedNodes: !4)\n\
                 !30 = !DINamespace(name: \"n\", scope: null)\n\
                 !31 = !DISubprogram(name: \"f\", linkageName: \"_ZN1n1fEi\", scope: !30, file: !1, line: 1, type: !11, scopeLine: 1, flags: DIFlagPrototyped, spFlags: 0)\n",
            ),
            "a namespace function defined out of line",
        ),
        (
            "debug:inlined-subroutine",
            one_function(
                "  store i32 %v, ptr %a, align 4, !dbg !33",
                &format!(
                    "{PLAIN_SP}!32 = distinct !DISubprogram(name: \"g\", scope: !1, file: !1, line: 5, type: !11, scopeLine: 5, spFlags: DISPFlagDefinition, unit: !0)\n\
                     !33 = !DILocation(line: 6, column: 1, scope: !32, inlinedAt: !15)\n"
                ),
            ),
            "an inlined location",
        ),
        (
            "debug:local-static",
            format!(
                "@s = internal global i32 0, align 4, !dbg !40\n{}",
                one_function(
                    declare,
                    &format!(
                        "{PLAIN_SP}!40 = !DIGlobalVariableExpression(var: !41, expr: !DIExpression())\n\
                         !41 = distinct !DIGlobalVariable(name: \"s\", scope: !10, file: !1, line: 3, type: !6, isLocal: true, isDefinition: true)\n\
                         !42 = !{{!40}}\n"
                    ),
                )
                .replace("nameTableKind: None)", "globals: !42, nameTableKind: None)")
            ),
            "a function's static",
        ),
    ];
    for (name, text, what) in &cases {
        assert_eq!(refusal(text), CgError::unsupported(name), "{what}");
    }
}

/// A global of function-pointer type, beside the function it points to,
/// is written through module emission (`UnitPlan::for_functions`): the
/// tree is `llc`'s.
#[test]
fn function_pointer_global_is_written() {
    let text = format!(
        "{}\
@fp = dso_local global ptr @f, align 8, !dbg !20

define dso_local void @f(i32 %v) !dbg !10 {{
entry:
  ret void, !dbg !15
}}

!llvm.dbg.cu = !{{!0}}
!llvm.module.flags = !{{!2, !3}}

!0 = distinct !DICompileUnit(language: DW_LANG_C11, file: !1, producer: \"hand\", isOptimized: false, runtimeVersion: 0, emissionKind: FullDebug, globals: !5, splitDebugInlining: false, nameTableKind: None)
!1 = !DIFile(filename: \"fp.c\", directory: \"/hod/tests/dies\")
!2 = !{{i32 7, !\"Dwarf Version\", i32 5}}
!3 = !{{i32 2, !\"Debug Info Version\", i32 3}}
!4 = !{{}}
!5 = !{{!20}}
!6 = !DIBasicType(name: \"int\", size: 32, encoding: DW_ATE_signed)
!10 = distinct !DISubprogram(name: \"f\", scope: !1, file: !1, line: 1, type: !11, scopeLine: 1, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !0, retainedNodes: !4)
!11 = !DISubroutineType(types: !12)
!12 = !{{null, !6}}
!15 = !DILocation(line: 1, column: 20, scope: !10)
!20 = !DIGlobalVariableExpression(var: !21, expr: !DIExpression())
!21 = distinct !DIGlobalVariable(name: \"fp\", scope: !0, file: !1, line: 2, type: !22, isLocal: false, isDefinition: true)
!22 = !DIDerivedType(tag: DW_TAG_pointer_type, baseType: !11, size: 64)
",
        ll_header(22)
    );
    if tool("llc-22").is_none() || tool("llvm-dwarfdump-22").is_none() {
        return;
    }
    let scratch = Scratch::new("dies-fp");
    let unit = compare(&scratch, 22, "fp", &text, &mut Tally::default());
    let fp = find(&unit, "DW_TAG_variable", "fp").expect("the global");
    assert!(fp.attrs.iter().any(|a| a.starts_with("DW_AT_type")), "{fp:?}");
    assert!(count_tag(&unit, "DW_TAG_subroutine_type") == 1, "{unit:?}");
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
    let scratch = Scratch::new("dies-chunks");
    let mut inputs = vec![("hand".to_owned(), hand(22))];
    if let (Some(c), Some(cxx)) = (tool("clang-22"), tool("clang++-22")) {
        inputs.push(("vars".into(), ir(&c, &scratch, "vars", "vars.c")));
        inputs.push(("members".into(), ir(&cxx, &scratch, "members", "members.cpp")));
    }
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

/// §8.7, the corpus: every module of `HOD_DIES_CORPUS` (a directory,
/// searched for `.ll` files) with `!llvm.dbg.cu` is compiled; a module
/// hod compiles verifies, and its tree is `llc`'s (template parameters
/// stripped from both inputs). Prints what was refused, by name.
#[test]
fn corpus_survey() {
    let Some(root) = std::env::var_os("HOD_DIES_CORPUS") else {
        eprintln!("note: set HOD_DIES_CORPUS to a corpus directory to survey it");
        return;
    };
    let scratch = Scratch::new("dies-corpus");
    let mut files = Vec::new();
    let mut stack = vec![std::path::PathBuf::from(root)];
    while let Some(dir) = stack.pop() {
        for e in std::fs::read_dir(&dir).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x == "ll") {
                files.push(p);
            }
        }
    }
    files.sort();
    let mut refused = std::collections::BTreeMap::<String, usize>::new();
    let (mut seen, mut equal, mut dwarf4) = (0, 0, 0);
    let mut tally = Tally::default();
    for (i, path) in files.iter().enumerate() {
        let text = std::fs::read_to_string(path).unwrap();
        if !text.contains("!llvm.dbg.cu") {
            continue;
        }
        seen += 1;
        if text.contains("!\"Dwarf Version\", i32 4") {
            dwarf4 += 1;
            continue;
        }
        let text = support::strip_template_params(&text);
        let (module, bodies) = parse(&text);
        match compile_to_object(&module, &bodies, &Options::default()) {
            Ok(_) => {
                let major = if path.to_string_lossy().contains("llvm19") { 19 } else { 22 };
                compare(&scratch, major, &format!("c{i}"), &text, &mut tally);
                equal += 1;
            }
            Err(ModuleError::Emit(e) | ModuleError::Function { error: e, .. }) => {
                *refused.entry(e.to_string()).or_default() += 1;
            }
            Err(e) => panic!("{}: {e}", path.display()),
        }
    }
    eprintln!(
        "corpus: {seen} modules with debug information; {dwarf4} ask for DWARF 4 (not compared); \
         {equal} trees equal to llc's ({} DIEs, {} subprograms); refused: {refused:?}",
        tally.dies, tally.subprograms
    );
}
