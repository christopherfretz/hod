//! WP-195 (`spec/dwarf.md` §6.25): the debug-information refusals real
//! projects met at `-O0 -g`, on both targets:
//!
//! - a variable two `#dbg_declare`s of slots name in one instance of its
//!   scope (a macro's `while (T r = ...)` expanded twice on one line) is
//!   one DIE located by the first slot, as `llc` writes it; it was
//!   `debug:variable-twice`, and any other repetition still is;
//! - a function's static whose class declaration reaches a type local to
//!   the function (an `auto` member returning a local class) is one
//!   subprogram DIE; it was `debug:local-static`;
//! - a function inlined everywhere whose retained nodes are imports
//!   scoped in it (a function-local `using`) has them under its abstract
//!   instance; it was `debug:retained-nodes`, and a retained variable, or
//!   a function also placed out of line, still is;
//! - a `debug:` refusal's detail names the node and its function.
//!
//! Each module's DIE tree is compared with `llc` 22's (`-O0
//! -global-isel=0 -fast-isel=0`) line by line, offsets and indices masked,
//! when `llc` is installed; hod's object verifies.

use std::path::Path;
use std::time::Duration;

use hod::{CompileError, Construct, OutputKind, Report, Settings, compile_with};
use hod_testutil::{Scratch, run_with, tool};

const TRIPLES: [&str; 2] = ["aarch64-unknown-linux-gnu", "x86_64-unknown-linux-gnu"];

/// `r` declared twice by two slots on one line: `llc` keeps the first.
const TWICE: &str = r#"target triple = "TRIPLE"

define dso_local i32 @_Z1fi(i32 %x) #0 !dbg !10 {
entry:
  %x.addr = alloca i32, align 4
  %r1 = alloca i32, align 4
  %r2 = alloca i32, align 4
  store i32 %x, ptr %x.addr, align 4
    #dbg_declare(ptr %x.addr, !14, !DIExpression(), !15)
    #dbg_declare(ptr %r1, !16, !DIExpression(), !17)
  %a = load i32, ptr %x.addr, align 4, !dbg !17
  %b = add i32 %a, 1, !dbg !17
  store i32 %b, ptr %r1, align 4, !dbg !17
    #dbg_declare(ptr %r2, !16, !DIExpression(), !18)
  %c = add i32 %a, 2, !dbg !18
  store i32 %c, ptr %r2, align 4, !dbg !18
  %d = load i32, ptr %r1, align 4, !dbg !19
  ret i32 %d, !dbg !19
}

attributes #0 = { noinline nounwind optnone "frame-pointer"="all" }

!llvm.dbg.cu = !{!0}
!llvm.module.flags = !{!2, !3}

!0 = distinct !DICompileUnit(language: DW_LANG_C_plus_plus_14, file: !1, producer: "kiln", isOptimized: false, runtimeVersion: 0, emissionKind: FullDebug, splitDebugInlining: false, nameTableKind: None)
!1 = !DIFile(filename: "twice.cpp", directory: "/hod/tests")
!2 = !{i32 7, !"Dwarf Version", i32 5}
!3 = !{i32 2, !"Debug Info Version", i32 3}
!4 = !{}
!10 = distinct !DISubprogram(name: "f", linkageName: "_Z1fi", scope: !1, file: !1, line: 1, type: !11, scopeLine: 1, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !0, retainedNodes: !4)
!11 = !DISubroutineType(types: !12)
!12 = !{!13, !13}
!13 = !DIBasicType(name: "int", size: 32, encoding: DW_ATE_signed)
!14 = !DILocalVariable(name: "x", arg: 1, scope: !10, file: !1, line: 1, type: !13)
!15 = !DILocation(line: 1, column: 11, scope: !10)
!16 = !DILocalVariable(name: "r", scope: !10, file: !1, line: 2, type: !13)
!17 = !DILocation(line: 2, column: 3, scope: !10)
!18 = !DILocation(line: 2, column: 19, scope: !10)
!19 = !DILocation(line: 3, column: 3, scope: !10)
"#;

/// `static auto w() { struct L { int v; }; static int calls; return
/// L{++calls}; }` of `struct P`: the static's function's declaration
/// returns a type local to the function.
const LOCAL_STATIC: &str = r#"target triple = "TRIPLE"

@_ZZN1P1wEvE5calls = linkonce_odr dso_local global i32 0, align 4, !dbg !20

define linkonce_odr dso_local i32 @_ZN1P1wEv() #0 !dbg !30 {
entry:
  %n = load i32, ptr @_ZZN1P1wEvE5calls, align 4, !dbg !33
  %m = add i32 %n, 1, !dbg !33
  store i32 %m, ptr @_ZZN1P1wEvE5calls, align 4, !dbg !33
  ret i32 %m, !dbg !33
}

define dso_local i32 @main() #0 !dbg !40 {
entry:
  %v = call i32 @_ZN1P1wEv(), !dbg !41
  ret i32 %v, !dbg !41
}

attributes #0 = { noinline nounwind optnone "frame-pointer"="all" }

!llvm.dbg.cu = !{!0}
!llvm.module.flags = !{!2, !3}

!0 = distinct !DICompileUnit(language: DW_LANG_C_plus_plus_14, file: !1, producer: "kiln", isOptimized: false, runtimeVersion: 0, emissionKind: FullDebug, globals: !19, splitDebugInlining: false, nameTableKind: None)
!1 = !DIFile(filename: "static.cpp", directory: "/hod/tests")
!2 = !{i32 7, !"Dwarf Version", i32 5}
!3 = !{i32 2, !"Debug Info Version", i32 3}
!4 = !{}
!13 = !DIBasicType(name: "int", size: 32, encoding: DW_ATE_signed)
!19 = !{!20}
!20 = !DIGlobalVariableExpression(var: !21, expr: !DIExpression())
!21 = distinct !DIGlobalVariable(name: "calls", scope: !30, file: !1, line: 4, type: !13, isLocal: false, isDefinition: true)
!22 = distinct !DICompositeType(tag: DW_TAG_structure_type, name: "P", file: !1, line: 1, size: 8, flags: DIFlagTypePassByValue, elements: !23, identifier: "_ZTS1P")
!23 = !{!24}
!24 = !DISubprogram(name: "w", linkageName: "_ZN1P1wEv", scope: !22, file: !1, line: 2, type: !25, scopeLine: 2, flags: DIFlagPrototyped | DIFlagStaticMember, spFlags: 0)
!25 = !DISubroutineType(types: !26)
!26 = !{!27}
!27 = distinct !DICompositeType(tag: DW_TAG_structure_type, name: "L", scope: !30, file: !1, line: 3, size: 32, flags: DIFlagTypePassByValue, elements: !28, identifier: "_ZTSZN1P1wEvE1L")
!28 = !{!29}
!29 = !DIDerivedType(tag: DW_TAG_member, name: "v", scope: !27, file: !1, line: 3, baseType: !13, size: 32)
!30 = distinct !DISubprogram(name: "w", linkageName: "_ZN1P1wEv", scope: !22, file: !1, line: 2, type: !25, scopeLine: 2, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !0, declaration: !24, retainedNodes: !4)
!33 = !DILocation(line: 4, column: 3, scope: !30)
!40 = distinct !DISubprogram(name: "main", scope: !1, file: !1, line: 6, type: !42, scopeLine: 6, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !0)
!41 = !DILocation(line: 6, column: 14, scope: !40)
!42 = !DISubroutineType(types: !43)
!43 = !{!13}
"#;

/// `namespace n { int map(int v) { return v; } }` and an always-inline
/// `int f(int a) { using n::map; return map(a); }` inlined into `g`.
const IMPORT: &str = r#"target triple = "TRIPLE"

define dso_local i32 @_Z1gi(i32 %0) #0 !dbg !10 {
  %2 = alloca i32, align 4
  %3 = alloca i32, align 4
  store i32 %0, ptr %3, align 4
    #dbg_declare(ptr %3, !15, !DIExpression(), !16)
  %4 = load i32, ptr %3, align 4, !dbg !17
  store i32 %4, ptr %2, align 4
    #dbg_declare(ptr %2, !18, !DIExpression(), !24)
  %5 = load i32, ptr %2, align 4, !dbg !26
  %6 = call i32 @_ZN1n3mapEi(i32 %5), !dbg !27
  ret i32 %6, !dbg !28
}

define linkonce_odr dso_local i32 @_ZN1n3mapEi(i32 %0) #0 !dbg !22 {
  %2 = alloca i32, align 4
  store i32 %0, ptr %2, align 4
    #dbg_declare(ptr %2, !29, !DIExpression(), !30)
  %3 = load i32, ptr %2, align 4, !dbg !31
  ret i32 %3, !dbg !32
}

attributes #0 = { noinline nounwind optnone "frame-pointer"="all" }

!llvm.dbg.cu = !{!0}
!llvm.module.flags = !{!2, !3}

!0 = distinct !DICompileUnit(language: DW_LANG_C_plus_plus_14, file: !1, producer: "kiln", isOptimized: false, runtimeVersion: 0, emissionKind: FullDebug, splitDebugInlining: false, nameTableKind: None)
!1 = !DIFile(filename: "import.cpp", directory: "/hod/tests")
!2 = !{i32 7, !"Dwarf Version", i32 5}
!3 = !{i32 2, !"Debug Info Version", i32 3}
!10 = distinct !DISubprogram(name: "g", linkageName: "_Z1gi", scope: !1, file: !1, line: 3, type: !11, scopeLine: 3, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !0, retainedNodes: !14)
!11 = !DISubroutineType(types: !12)
!12 = !{!13, !13}
!13 = !DIBasicType(name: "int", size: 32, encoding: DW_ATE_signed)
!14 = !{}
!15 = !DILocalVariable(name: "a", arg: 1, scope: !10, file: !1, line: 3, type: !13)
!16 = !DILocation(line: 3, column: 11, scope: !10)
!17 = !DILocation(line: 3, column: 25, scope: !10)
!18 = !DILocalVariable(name: "a", arg: 1, scope: !19, file: !1, line: 2, type: !13)
!19 = distinct !DISubprogram(name: "f", linkageName: "_Z1fi", scope: !1, file: !1, line: 2, type: !11, scopeLine: 2, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !0, retainedNodes: !20)
!20 = !{!21}
!21 = !DIImportedEntity(tag: DW_TAG_imported_declaration, scope: !19, entity: !22, file: !1, line: 2)
!22 = distinct !DISubprogram(name: "map", linkageName: "_ZN1n3mapEi", scope: !23, file: !1, line: 1, type: !11, scopeLine: 1, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !0, retainedNodes: !14)
!23 = !DINamespace(name: "n", scope: null)
!24 = !DILocation(line: 2, column: 49, scope: !19, inlinedAt: !25)
!25 = distinct !DILocation(line: 3, column: 23, scope: !10)
!26 = !DILocation(line: 2, column: 79, scope: !19, inlinedAt: !25)
!27 = !DILocation(line: 2, column: 75, scope: !19, inlinedAt: !25)
!28 = !DILocation(line: 3, column: 16, scope: !10)
!29 = !DILocalVariable(name: "v", arg: 1, scope: !22, file: !1, line: 1, type: !13)
!30 = !DILocation(line: 1, column: 34, scope: !22)
!31 = !DILocation(line: 1, column: 46, scope: !22)
!32 = !DILocation(line: 1, column: 39, scope: !22)
"#;

fn module(template: &str, triple: &str) -> String {
    template.replace("TRIPLE", triple)
}

fn compile(text: &str) -> Result<Vec<u8>, CompileError> {
    let options = hod::Options { output_kind: OutputKind::Object, ..hod::Options::default() };
    compile_with(text.as_bytes(), &options, &Settings::default(), &mut Report::default())
}

fn run_tool(tool: &Path, args: &[&std::ffi::OsStr]) -> String {
    let mut command = std::process::Command::new(tool);
    command.args(args);
    let out = run_with(&mut command, &[], Duration::from_mins(2));
    assert!(out.success(), "{command:?}: {}", out.stderr_text());
    out.stdout_text()
}

/// The DIE tree of `object`, one attribute per line, offsets, addresses,
/// sizes and indices masked; without the code ranges.
fn tree(dwarfdump: &Path, object: &Path) -> Vec<String> {
    let text = run_tool(dwarfdump, &["--debug-info".as_ref(), object.as_os_str()]);
    let mut lines = Vec::new();
    for line in text.lines().skip_while(|l| !l.contains("DW_TAG_compile_unit")) {
        let line = line.trim();
        if line.is_empty() || line.starts_with("DW_AT_high_pc") || line.starts_with("DW_AT_low_pc")
        {
            continue;
        }
        let line = line
            .split_once(": ")
            .map_or(line, |(at, rest)| if at.starts_with("0x") { rest } else { line });
        lines.push(mask_hex(line));
    }
    lines
}

/// A line with every hexadecimal number (an offset, an index, an
/// address) masked, a constant `(0x08)` read as `(8)`.
fn mask_hex(line: &str) -> String {
    if let Some((at, value)) = line.split_once("\t(0x")
        && let Some(digits) = value.strip_suffix(')')
        && let Ok(n) = u64::from_str_radix(digits, 16)
    {
        return format!("{at}\t({n})");
    }
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    while let Some(at) = rest.find("0x") {
        out.push_str(&rest[..at]);
        out.push_str("0x_");
        rest = rest[at + 2..].trim_start_matches(|c: char| c.is_ascii_hexdigit());
    }
    out.push_str(rest);
    out
}

/// hod's object of `text`, compared with `llc` 22's when it is installed:
/// the same DIE tree, hod's verified. Returns hod's tree.
fn compare(scratch: &Scratch, name: &str, text: &str) -> Vec<String> {
    let object = compile(text).unwrap_or_else(|e| panic!("{name}: {e}"));
    let hod = scratch.write(&format!("{name}.hod.o"), &object);
    let Some(dwarfdump) = tool("llvm-dwarfdump") else { return Vec::new() };
    let verified = run_tool(&dwarfdump, &["--verify".as_ref(), hod.as_os_str()]);
    assert!(verified.contains("No errors."), "{name}: {verified}");
    let ours = tree(&dwarfdump, &hod);
    let Some(llc) = tool("llc-22") else { return ours };
    let ll = scratch.write(&format!("{name}.ll"), text);
    let reference = scratch.join(format!("{name}.llc.o"));
    let mut command = std::process::Command::new(&llc);
    command
        .args(["-O0", "-global-isel=0", "-fast-isel=0", "-filetype=obj", "-o"])
        .arg(&reference)
        .arg(&ll);
    let out = run_with(&mut command, &[], Duration::from_mins(2));
    assert!(out.success(), "{name}: llc: {}", out.stderr_text());
    let theirs = tree(&dwarfdump, &reference);
    let producer = |l: &&String| !l.starts_with("DW_AT_producer");
    // hod's frames are laid out otherwise than `llc`'s: a slot's offset
    // differs (`cargo xtask debug trees` and the debugger cases compare
    // the places).
    let place = |l: &String| -> String {
        if l.starts_with("DW_AT_location") { "DW_AT_location".into() } else { l.clone() }
    };
    let a: Vec<String> = ours.iter().filter(producer).map(place).collect();
    let b: Vec<String> = theirs.iter().filter(producer).map(place).collect();
    if let Some(i) = (0..a.len().max(b.len())).find(|&i| a.get(i) != b.get(i)) {
        panic!(
            "{name}: line {i}: hod {:?}, llc {:?}\nhod:\n{}\nllc:\n{}",
            a.get(i),
            b.get(i),
            ours.join("\n"),
            theirs.join("\n")
        );
    }
    ours
}

/// The `debug:` construct and detail `text` is refused with.
fn refusal(text: &str) -> (String, String) {
    match compile(text) {
        Err(CompileError::Unsupported(u)) => {
            let Construct::Named(name) = u.construct else { panic!("{:?}", u.construct) };
            (name.to_string(), u.detail.unwrap_or_default())
        }
        other => panic!("expected a decline, got {:?}", other.map(|o| o.len())),
    }
}

fn count(tree: &[String], line: &str) -> usize {
    tree.iter().filter(|l| l.trim() == line).count()
}

#[test]
fn a_variable_declared_by_two_slots_is_located_by_the_first() {
    let scratch = Scratch::new("wp195-twice");
    for triple in TRIPLES {
        let name = format!("twice-{}", &triple[..6]);
        let ours = compare(&scratch, &name, &module(TWICE, triple));
        if ours.is_empty() {
            continue;
        }
        assert_eq!(count(&ours, "DW_AT_name\t(\"r\")"), 1, "{name}: one DIE\n{ours:?}");
        // The first slot's place: the tree of the module without the
        // second record.
        let once = module(TWICE, triple)
            .replace("    #dbg_declare(ptr %r2, !16, !DIExpression(), !18)\n", "");
        let object = compile(&once).unwrap_or_else(|e| panic!("{name}: {e}"));
        let path = scratch.write(&format!("{name}.once.o"), &object);
        let dwarfdump = tool("llvm-dwarfdump").expect("dwarfdump");
        assert_eq!(ours, tree(&dwarfdump, &path), "{name}: located by the first slot");
    }
}

/// A second record of the variable that is not a slot is refused, by
/// name, with the variable and its function.
#[test]
fn a_variable_declared_by_a_slot_and_a_value_is_refused() {
    for triple in TRIPLES {
        let text = module(TWICE, triple).replace(
            "    #dbg_declare(ptr %r2, !16, !DIExpression(), !18)\n  %c = add i32 %a, 2, !dbg !18\n",
            "  %c = add i32 %a, 2, !dbg !18\n    #dbg_value(i32 %c, !16, !DIExpression(), !18)\n",
        );
        let (construct, detail) = refusal(&text);
        assert_eq!(construct, "debug:variable-twice", "{triple}");
        assert!(detail.starts_with("DILocalVariable (node "), "{triple}: {detail}");
        assert!(detail.ends_with(") 'r' in function '_Z1fi'"), "{triple}: {detail}");
    }
}

#[test]
fn a_static_of_a_function_returning_its_local_type_has_one_subprogram() {
    let scratch = Scratch::new("wp195-static");
    for triple in TRIPLES {
        let name = format!("static-{}", &triple[..6]);
        let ours = compare(&scratch, &name, &module(LOCAL_STATIC, triple));
        if ours.is_empty() {
            continue;
        }
        assert_eq!(count(&ours, "DW_AT_name\t(\"calls\")"), 1, "{name}\n{ours:?}");
        assert_eq!(count(&ours, "DW_AT_name\t(\"L\")"), 1, "{name}\n{ours:?}");
    }
}

#[test]
fn an_inlined_functions_import_is_under_its_abstract_instance() {
    let scratch = Scratch::new("wp195-import");
    for triple in TRIPLES {
        let name = format!("import-{}", &triple[..6]);
        let ours = compare(&scratch, &name, &module(IMPORT, triple));
        if ours.is_empty() {
            continue;
        }
        assert_eq!(count(&ours, "DW_TAG_imported_declaration"), 1, "{name}\n{ours:?}");
    }
}

/// A retained variable of an inlined function, or retained nodes of a
/// function also placed out of line, stay `debug:retained-nodes`.
#[test]
fn other_retained_nodes_of_inlined_functions_are_refused() {
    for triple in TRIPLES {
        let text = module(IMPORT, triple).replace(
            "!20 = !{!21}",
            "!20 = !{!21, !33}\n!33 = !DILocalVariable(name: \"u\", scope: !19, file: !1, line: 2, type: !13)",
        );
        assert_eq!(refusal(&text).0, "debug:retained-nodes", "{triple}: a variable");
        let text = module(IMPORT, triple).replace(
            "attributes #0",
            "define linkonce_odr dso_local i32 @_Z1fi(i32 %0) #0 !dbg !19 {\n  ret i32 %0, !dbg !34\n}\n\nattributes #0",
        ) + "!34 = !DILocation(line: 2, column: 1, scope: !19)\n";
        assert_eq!(refusal(&text).0, "debug:retained-nodes", "{triple}: out of line too");
    }
}
