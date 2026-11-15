//! WP-230 (`spec/dwarf.md` §6.33): a function placed out of line and
//! also inlined (a template, `f<int>`, with a typedef local to it, a
//! structure local to one of its blocks, and a parameter pack) was
//! refused as `debug:inlined-local-type` (or `debug:local-type-scope`
//! when the inlining function came first). `llc` 22 writes the template
//! parameters under the abstract instance only (the concrete DIE gets
//! `DW_AT_abstract_origin` alone, `finishSubprogramDefinition`), a local
//! type reached once the instance exists under the instance (or its
//! block, `getOrCreateContextDIE`), and a local type reached before (a
//! retained type of the unit) under the concrete DIE, which it then
//! keeps.
//!
//! - `compiles_on_both_targets`: the three shapes, both targets;
//! - `trees_equal_llc`: the DIE tree (code addresses and frame offsets
//!   masked) equals `llc` 22's (`-O0 -fast-isel=0`, `-global-isel=0` on
//!   AArch64), both objects `--verify` clean, and the placements are
//!   checked by name.

use std::path::Path;
use std::process::Command;
use std::time::Duration;

use hod::{CompileError, OutputKind, Report, Settings, compile_with};
use hod_testutil::{Scratch, run_with, tool, tools_required};

const F: &str = r"define i32 @f(i32 %x) !dbg !21 {
  %u = alloca i32, align 4
  %s = alloca [8 x i8], align 4
    #dbg_declare(ptr %u, !24, !DIExpression(), !35)
  store i32 %x, ptr %u, align 4, !dbg !36
  call void @use(ptr %u), !dbg !36
    #dbg_declare(ptr %s, !26, !DIExpression(), !41)
  call void @use(ptr %s), !dbg !41
  ret i32 1, !dbg !36
}
";

const H: &str = r"define i32 @h(i32 %a) !dbg !53 {
  %u = alloca i32, align 4
  %s = alloca [8 x i8], align 4
    #dbg_declare(ptr %u, !24, !DIExpression(), !57)
  store i32 %a, ptr %u, align 4, !dbg !59
  call void @use(ptr %u), !dbg !59
    #dbg_declare(ptr %s, !26, !DIExpression(), !64)
  call void @use(ptr %s), !dbg !64
  ret i32 2, !dbg !56
}
";

const META: &str = r#"!llvm.dbg.cu = !{!2}
!llvm.module.flags = !{!9, !10}
!2 = distinct !DICompileUnit(language: DW_LANG_C_plus_plus_14, file: !3, producer: "hand", isOptimized: true, runtimeVersion: 0, emissionKind: FullDebugRETAINED)
!3 = !DIFile(filename: "lt.cpp", directory: "/m")
!6 = !DISubroutineType(types: !7)
!7 = !{!8, !8}
!8 = !DIBasicType(name: "int", size: 32, encoding: DW_ATE_signed)
!9 = !{i32 7, !"Dwarf Version", i32 5}
!10 = !{i32 2, !"Debug Info Version", i32 3}
!21 = distinct !DISubprogram(name: "f<int>", linkageName: "f", scope: !3, file: !3, line: 2, type: !6, scopeLine: 2, spFlags: DISPFlagDefinition | DISPFlagOptimized, unit: !2, templateParams: !33, retainedNodes: !22)
!22 = !{!23, !24, !26}
!23 = !DILocalVariable(name: "x", arg: 1, scope: !21, file: !3, line: 2, type: !8)
!24 = !DILocalVariable(name: "u", scope: !21, file: !3, line: 4, type: !25)
!25 = !DIDerivedType(tag: DW_TAG_typedef, name: "U", scope: !21, file: !3, line: 3, baseType: !8)
!26 = !DILocalVariable(name: "s", scope: !27, file: !3, line: 7, type: !29)
!27 = distinct !DILexicalBlock(scope: !21, file: !3, line: 6, column: 14)
!29 = distinct !DICompositeType(tag: DW_TAG_structure_type, name: "S", scope: !27, file: !3, line: 7, size: 64, elements: !30)
!30 = !{!31, !32}
!31 = !DIDerivedType(tag: DW_TAG_member, name: "a", scope: !29, file: !3, line: 7, baseType: !25, size: 32)
!32 = !DIDerivedType(tag: DW_TAG_member, name: "b", scope: !29, file: !3, line: 7, baseType: !8, size: 32, offset: 32)
!33 = !{!34, !37}
!34 = !DITemplateTypeParameter(name: "T", type: !8)
!37 = !DITemplateValueParameter(tag: DW_TAG_GNU_template_parameter_pack, name: "Args", value: !38)
!38 = !{!39}
!39 = !DITemplateTypeParameter(type: !8)
!35 = !DILocation(line: 0, scope: !21)
!36 = !DILocation(line: 4, column: 3, scope: !21)
!41 = !DILocation(line: 7, column: 5, scope: !27)
!53 = distinct !DISubprogram(name: "h", linkageName: "h", scope: !3, file: !3, line: 13, type: !6, scopeLine: 13, spFlags: DISPFlagDefinition | DISPFlagOptimized, unit: !2, retainedNodes: !54)
!54 = !{!55}
!55 = !DILocalVariable(name: "a", arg: 1, scope: !53, file: !3, line: 13, type: !8)
!56 = !DILocation(line: 0, scope: !53)
!57 = !DILocation(line: 0, scope: !21, inlinedAt: !58)
!58 = distinct !DILocation(line: 13, column: 23, scope: !53)
!59 = !DILocation(line: 4, column: 3, scope: !21, inlinedAt: !58)
!64 = !DILocation(line: 7, column: 5, scope: !27, inlinedAt: !58)
!90 = !{!25}
"#;

const TRIPLES: [&str; 2] = ["aarch64-unknown-linux-gnu", "x86_64-unknown-linux-gnu"];

/// The shapes: `f` first, `h` (which inlines `f`) first, and `f` first
/// with `U` a retained type of the unit.
const SHAPES: [(&str, bool, bool); 3] = [
    ("out-of-line-first", false, false),
    ("inlined-first", true, false),
    ("retained", false, true),
];

fn module(triple: &str, h_first: bool, retained: bool) -> String {
    let (first, second) = if h_first { (H, F) } else { (F, H) };
    let meta = META.replace("RETAINED", if retained { ", retainedTypes: !90" } else { "" });
    format!("target triple = \"{triple}\"\ndeclare void @use(ptr)\n{first}{second}{meta}")
}

fn compile(text: &str) -> Result<Vec<u8>, CompileError> {
    let options = hod::Options { output_kind: OutputKind::Object, ..hod::Options::default() };
    compile_with(text.as_bytes(), &options, &Settings::default(), &mut Report::default())
}

#[test]
fn compiles_on_both_targets() {
    for triple in TRIPLES {
        for (name, h_first, retained) in SHAPES {
            let object = compile(&module(triple, h_first, retained))
                .unwrap_or_else(|e| panic!("{triple}, {name}: {e}"));
            assert!(object.windows(11).any(|w| w == b".debug_info"), "{triple}, {name}");
        }
    }
}

fn output(command: &mut Command) -> String {
    let outcome = run_with(command, &[], Duration::from_mins(2));
    assert!(outcome.success(), "{command:?}: {}", outcome.stderr_text());
    outcome.stdout_text()
}

/// The values `spec/dwarf.md` §6.7 masks: code addresses and sizes,
/// section bases, and locations.
const MASKED: [&str; 9] = [
    "DW_AT_low_pc",
    "DW_AT_high_pc",
    "DW_AT_location",
    "DW_AT_frame_base",
    "DW_AT_addr_base",
    "DW_AT_str_offsets_base",
    "DW_AT_stmt_list",
    "DW_AT_producer",
    "DW_AT_abstract_origin",
];

/// The DIE tree of `object`, one line per DIE or attribute, references
/// by their target's name.
fn tree(dwarfdump: &Path, object: &Path) -> Vec<String> {
    let text = output(Command::new(dwarfdump).arg("--debug-info").arg(object));
    let mut out = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim_start();
        if let Some((addr, rest)) = line.split_once(':')
            && addr.starts_with("0x")
            && !addr.contains(' ')
        {
            let depth = rest.len() - rest.trim_start().len();
            let tag = rest.trim();
            if tag.starts_with("DW_TAG_") || tag == "NULL" {
                out.push(format!("{depth:>3} {tag}"));
            }
            continue;
        }
        if !trimmed.starts_with("DW_AT_") {
            continue;
        }
        let (name, value) = trimmed.split_once(char::is_whitespace).unwrap_or((trimmed, ""));
        if MASKED.contains(&name) {
            let named = value.find(" \"").map(|q| value[q..].trim().to_owned());
            match (name, named) {
                ("DW_AT_abstract_origin", Some(n)) => out.push(format!("    {name} {n}")),
                _ => out.push(format!("    {name}")),
            }
            continue;
        }
        let value = value.trim();
        let mut v =
            match value.strip_prefix("(0x").and_then(|rest| rest.find(" \"").map(|q| (rest, q))) {
                Some((rest, q)) => format!("({}", &rest[q + 1..]),
                None => value.to_owned(),
            };
        if let Some(num) = value.strip_prefix('(').and_then(|r| r.strip_suffix(')')) {
            let parsed = num
                .strip_prefix("0x")
                .map_or_else(|| num.parse::<u64>().ok(), |h| u64::from_str_radix(h, 16).ok());
            if let Some(n) = parsed {
                v = format!("({n})");
            }
        }
        out.push(format!("    {name} {v}"));
    }
    out
}

fn verify(dwarfdump: &Path, object: &Path) {
    let mut command = Command::new(dwarfdump);
    command.arg("--verify").arg(object);
    let outcome = run_with(&mut command, &[], Duration::from_mins(2));
    assert!(outcome.success(), "{} --verify: {}", object.display(), outcome.stdout_text());
}

/// The subprogram DIE each line of `tree` is under: `(depth, kind)` of
/// the nearest enclosing `DW_TAG_subprogram`, "abstract" when it has
/// `DW_AT_inline`, "concrete" otherwise.
fn parent_of(tree: &[String], what: &str) -> Vec<&'static str> {
    let mut found = Vec::new();
    for (i, line) in tree.iter().enumerate() {
        if line != &format!("    DW_AT_name (\"{what}\")") {
            continue;
        }
        // The DIE's tag line, then its depth.
        let tag = (0..i).rev().find(|&j| !tree[j].starts_with("    ")).unwrap();
        let depth: usize = tree[tag][..3].trim().parse().unwrap();
        let sub = (0..tag)
            .rev()
            .find(|&j| {
                !tree[j].starts_with("    ")
                    && tree[j][..3].trim().parse::<usize>().unwrap() < depth
                    && tree[j].ends_with("DW_TAG_subprogram")
            })
            .unwrap();
        let attrs = tree[sub + 1..].iter().take_while(|l| l.starts_with("    "));
        found.push(if attrs.clone().any(|l| l.starts_with("    DW_AT_inline")) {
            "abstract"
        } else {
            "concrete"
        });
    }
    found
}

#[test]
fn trees_equal_llc() {
    let (Some(llc), Some(dwarfdump)) = (tool("llc-22"), tool("llvm-dwarfdump-22")) else {
        assert!(!tools_required(), "llc-22 and llvm-dwarfdump-22 are required");
        eprintln!("note: no llc-22 or llvm-dwarfdump-22; skipping");
        return;
    };
    let scratch = Scratch::new("inlined_local_type");
    for triple in TRIPLES {
        for (shape, h_first, retained) in SHAPES {
            let name = format!("{}-{shape}", &triple[..triple.find('-').unwrap()]);
            let text = module(triple, h_first, retained);
            let ours = scratch.write(&format!("{name}.hod.o"), compile(&text).unwrap());
            let input = scratch.write(&format!("{name}.ll"), &text);
            let theirs = scratch.join(format!("{name}.llc.o"));
            let mut command = Command::new(&llc);
            command.args(["-O0", "-fast-isel=0", "-relocation-model=pic", "-filetype=obj"]);
            if triple.starts_with("aarch64") {
                command.arg("-global-isel=0");
            }
            output(command.arg(&input).arg("-o").arg(&theirs));
            verify(&dwarfdump, &ours);
            verify(&dwarfdump, &theirs);
            let (h, l) = (tree(&dwarfdump, &ours), tree(&dwarfdump, &theirs));
            assert_eq!(h.join("\n"), l.join("\n"), "{name}: hod's tree against llc's");
            // The template parameters are the abstract instance's only;
            // `S` is under its block there; `U` is there unless it was
            // retained, then under the concrete DIE.
            assert_eq!(parent_of(&h, "T"), ["abstract"], "{name}: T");
            assert_eq!(parent_of(&h, "Args"), ["abstract"], "{name}: Args");
            assert_eq!(parent_of(&h, "S"), ["abstract"], "{name}: S");
            let u = if retained { "concrete" } else { "abstract" };
            assert_eq!(parent_of(&h, "U"), [u], "{name}: U");
        }
    }
}
