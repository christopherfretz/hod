//! `spec/ll.md` §7.4 and §11.9: debug information read into `hod_ir`'s
//! tables and printed back.
//!
//! The oracle is LLVM: a module clang or kiln wrote is parsed, bodies
//! included, and printed in the dialect of each LLVM major; `llvm-as`
//! must accept the text, and the DWARF `llc -O0` makes of it must be the
//! DWARF it makes of the original (`llvm-dwarfdump --debug-info
//! --debug-line`, offsets aside). The negative cases are checked against
//! `llvm-as`'s verdict.

mod common;

use std::process::Command;

use hod_ir::{Arch, DbgRecordKind, DiKind, Dialect, LocId, Module, PrintError, Value};
use hod_ll::{BodyParser, LlErrorKind, ParseOptions, parse_module};
use hod_testutil::{DEFAULT_TIMEOUT, Scratch, run_with};

use common::{Llvm, data, header};

/// The module and its bodies, or the first error; the unsupported
/// constructs recorded.
fn read(text: &[u8]) -> Result<(Module, Vec<hod_ir::Body>, Vec<String>), hod_ll::LlError> {
    let mut parsed = parse_module(text, &ParseOptions::default())?;
    let mut parser = BodyParser::new(&parsed.text);
    let mut bodies = Vec::new();
    for function in parsed.text.functions() {
        if let Some(body) = parser.parse(&mut parsed.module, function)? {
            body.verify(&parsed.module).expect("a valid body");
            bodies.push(body);
        }
    }
    let unsupported = parsed.module.unsupported().map(|u| u.construct.to_string()).collect();
    Ok((parsed.module, bodies, unsupported))
}

/// `text` printed in the dialect of `major`.
fn print(module: &Module, bodies: &[hod_ir::Body], major: u32) -> Result<String, PrintError> {
    let mut out = String::new();
    module.print(&mut out, Dialect::new(major).expect("a dialect"), bodies)?;
    Ok(out)
}

/// `llvm-dwarfdump --debug-info --debug-line` of what `llc -O0` makes of
/// `ir`, without the offsets, which move with the producer string's
/// length and the order of the abbreviations.
fn dwarf(llvm: &Llvm, scratch: &Scratch, name: &str, ir: &[u8], is_stmt: bool) -> String {
    let object = scratch.join(format!("{name}.o"));
    let mut llc = Command::new(llvm.tool("llc"));
    llc.args(["-O0", "-global-isel=0", "-fast-isel=0", "-filetype=obj", "-o"]);
    llc.arg(&object).arg("-");
    let outcome = run_with(&mut llc, ir, DEFAULT_TIMEOUT);
    assert!(outcome.success(), "{name}: llc: {}", outcome.stderr_text());
    let mut dump = Command::new(llvm.tool("llvm-dwarfdump"));
    dump.args(["--debug-info", "--debug-line"]).arg(&object);
    let outcome = run_with(&mut dump, &[], DEFAULT_TIMEOUT);
    assert!(outcome.success(), "{name}: llvm-dwarfdump: {}", outcome.stderr_text());
    let text = outcome.stdout_text();
    let mut normal = String::with_capacity(text.len());
    let mut previous: Vec<&str> = Vec::new();
    for line in text.lines() {
        if line.contains("file format") {
            continue;
        }
        // Without key instructions, LLVM writes no second row for a
        // place whose row was not a statement.
        let row = line.len() > 18
            && line.starts_with("0x")
            && line.as_bytes()[2..18].iter().all(u8::is_ascii_hexdigit);
        if !is_stmt && row {
            let place: Vec<&str> = line.split_whitespace().skip(1).take(3).collect();
            if place == previous {
                continue;
            }
            previous = place;
        }
        let mut rest = line;
        while let Some(at) = rest.find("0x") {
            normal.push_str(&rest[..at]);
            let hex = rest[at + 2..].bytes().take_while(u8::is_ascii_hexdigit).count();
            rest = if hex == 8 { &rest[at + 10..] } else {
                normal.push_str("0x");
                &rest[at + 2..]
            };
        }
        normal.push_str(rest);
        if !is_stmt {
            normal = normal.replace(" is_stmt", "");
        }
        normal.push('\n');
    }
    normal
}

/// What happened to one input printed for one major.
fn round_trip(llvm: &Llvm, scratch: &Scratch, name: &str, text: &[u8]) {
    let (module, bodies, unsupported) =
        read(text).unwrap_or_else(|error| panic!("{name}: {error}"));
    assert_eq!(unsupported, Vec::<String>::new(), "{name}");
    assert!(!module.debug.is_empty(), "{name}: debug information");
    let printed = print(&module, &bodies, llvm.major).unwrap_or_else(|e| panic!("{name}: {e}"));
    llvm.accepts(printed.as_bytes()).unwrap_or_else(|e| panic!("{name}: llvm-as: {e}"));
    // Key instructions (`atomGroup`) are dropped (`spec/ir.md` §6): they
    // decide only which rows of the line table are statements.
    let is_stmt = !text.windows(9).any(|w| w == b"atomGroup");
    let want = dwarf(llvm, scratch, &format!("{name}.in"), text, is_stmt);
    let got = dwarf(llvm, scratch, &format!("{name}.out"), printed.as_bytes(), is_stmt);
    if want != got {
        let first = want.lines().zip(got.lines()).position(|(a, b)| a != b);
        panic!("{name}: the DWARF differs from line {first:?}:\n{want}\n----\n{got}");
    }
}

/// §7.4: clang's `-g` output of a C and a C++ program, at `-O0` and at
/// `-O2` (assignment tracking off: §11.9), and kiln's, round-trip with
/// the same DWARF in every installed LLVM.
#[test]
fn debug_information_round_trips_with_the_same_dwarf() {
    let scratch = Scratch::new("hod_ll_debug");
    let mut checked = 0;
    for llvm in Llvm::all() {
        if !llvm.tool("llvm-dwarfdump").is_file() {
            eprintln!("note: skipping LLVM {}: no llvm-dwarfdump", llvm.major);
            continue;
        }
        for source in ["debug_types.cpp", "debug_c.c"] {
            for options in [&["-g"][..], &["-g", "-O2", "-Xclang", "-fexperimental-assignment-tracking=disabled"]] {
                let text = llvm.clang(source, Arch::Aarch64, options);
                let name = format!("{source} {options:?} llvm{}", llvm.major);
                round_trip(&llvm, &scratch, &name, &text);
                checked += 1;
            }
        }
        if llvm.major >= 22 {
            let text = data("debug/kiln_types.ll");
            round_trip(&llvm, &scratch, &format!("kiln_types.ll llvm{}", llvm.major), &text);
            checked += 1;
        }
    }
    eprintln!("debug round trips: {checked}");
}

/// §11.9: an assign record is read, and the printer refuses it by name:
/// the `DIAssignID` that ties it to its stores is not kept.
#[test]
fn assign_records_are_read_and_refused_by_the_printer() {
    let Some(llvm) = Llvm::all().into_iter().next() else { return };
    let text = llvm.clang("debug_c.c", Arch::Aarch64, &["-g", "-O2"]);
    let (module, bodies, unsupported) = read(&text).expect("the module");
    assert_eq!(unsupported, Vec::<String>::new());
    let assigns = bodies
        .iter()
        .flat_map(|body| body.dbg_records())
        .filter(|record| record.kind == DbgRecordKind::Assign)
        .count();
    assert!(assigns > 0, "clang -O2 -g writes #dbg_assign");
    match print(&module, &bodies, llvm.major) {
        Err(PrintError::Unsupported { construct, .. }) => {
            assert_eq!(construct, "debug:#dbg_assign");
        }
        other => panic!("{other:?}"),
    }
}

const BODY: &str = r#"
define i32 @f(i32 %x, i32 %y) !dbg !5 {
  %a = alloca i32, align 4
    #dbg_declare(ptr %a, !8, !DIExpression(), !10)
  store i32 %x, ptr %a, align 4, !dbg !10
    #dbg_value(!DIArgList(i32 %x, i32 %y), !9, !DIExpression(DW_OP_LLVM_arg, 0, DW_OP_LLVM_arg, 1, DW_OP_plus, DW_OP_stack_value), !11)
    #dbg_value(i32 %z, !9, !DIExpression(), !11)
  %z = add i32 %x, %y, !dbg !11
  call void @llvm.dbg.value(metadata i32 %z, metadata !9, metadata !DIExpression()), !dbg !11
    #dbg_label(!12, !13)
  ret i32 %z, !dbg !DILocation(line: 4, column: 3, scope: !5)
}
declare void @llvm.dbg.value(metadata, metadata, metadata)

!llvm.dbg.cu = !{!0}
!llvm.module.flags = !{!3, !4}
!0 = distinct !DICompileUnit(language: DW_LANG_C11, file: !1, emissionKind: FullDebug)
!1 = !DIFile(filename: "a.c", directory: "/")
!3 = !{i32 2, !"Debug Info Version", i32 3}
!4 = !{i32 7, !"Dwarf Version", i32 5}
!5 = distinct !DISubprogram(name: "f", scope: !1, file: !1, line: 1, type: !6, unit: !0, spFlags: DISPFlagDefinition)
!6 = !DISubroutineType(types: !{!7, !7, !7})
!7 = !DIBasicType(name: "int", size: 32, encoding: DW_ATE_signed)
!8 = !DILocalVariable(name: "a", scope: !5, file: !1, line: 1, type: !7)
!9 = !DILocalVariable(name: "s", scope: !5, file: !1, line: 2, type: !7)
!10 = !DILocation(line: 1, column: 1, scope: !5)
!11 = !DILocation(line: 2, column: 7, scope: !5, inlinedAt: !14)
!12 = !DILabel(scope: !5, name: "out", file: !1, line: 3)
!13 = !DILocation(line: 3, scope: !5)
!14 = distinct !DILocation(line: 9, scope: !5)
"#;

/// §11.9: the form a body's debug information takes, which the bitcode
/// reader fills too: locations per instruction, the declared slots and
/// the positioned records, from `#dbg_*` and from the intrinsic calls.
#[test]
fn a_body_keeps_its_records_and_locations() {
    let text = format!("{}{BODY}", header(Arch::Aarch64, 22));
    let (module, bodies, unsupported) = read(text.as_bytes()).expect("the module");
    assert_eq!(unsupported, Vec::<String>::new());
    let body = &bodies[0];
    assert_eq!(body.slot_vars().len(), 1);
    let records: Vec<_> = body.dbg_records().collect();
    assert_eq!(records.len(), 4);
    assert!(records[0].arg_list);
    assert_eq!(records[0].values.len(), 2);
    assert_eq!(records[1].kind, DbgRecordKind::Value);
    // A record may name a value defined after it.
    assert_ne!(records[1].values[0], Value::NONE);
    assert_eq!(records[2].kind, DbgRecordKind::Value);
    assert_eq!(records[3].kind, DbgRecordKind::Label);
    assert_eq!(module.debug.kind(records[3].var), DiKind::Label);
    // The location of the `add`, inlined, and the one in place on `ret`.
    let locs: Vec<LocId> = body.blocks().flat_map(|b| b.insts()).map(|i| body.loc(i.id())).collect();
    assert!(locs.iter().all(|&loc| loc != LocId::NONE), "{locs:?}");
    let add = module.debug.location(locs[1]).expect("a location");
    assert_eq!((add.line, add.column), (2, 7));
    assert_ne!(add.inlined_at, LocId::NONE);
    assert!(module.debug.is_distinct_loc(add.inlined_at));
    for llvm in Llvm::all() {
        let printed = print(&module, &bodies, llvm.major).expect("printed");
        llvm.accepts(printed.as_bytes()).unwrap_or_else(|e| panic!("{e}\n{printed}"));
    }
}

/// What hod makes of a case: an error, debug information refused by
/// name, or a clean module.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Verdict {
    /// An error; `llvm-as` fails too.
    Error,
    /// Refused by name: `llvm-as` accepts, and drops the debug
    /// information with a warning or would not use it.
    Refused,
    /// Read; `llvm-as` accepts.
    Read,
}

/// The negative cases: the text after the target lines, and hod's
/// verdict. `llvm-as` errs exactly on the `Error` ones.
const NEGATIVE: &[(&str, Verdict)] = &[
    ("!0 = !DIBogus(line: 1)", Verdict::Error),
    ("!0 = !DIBasicType(name: \"i\", foo: 1)", Verdict::Error),
    ("!0 = !DIBasicType(name: \"i\", size: 1, size: 2)", Verdict::Error),
    ("!0 = !DIBasicType(name: \"i\", size: -1)", Verdict::Error),
    ("!0 = !DIBasicType(name: \"i\", size: \"x\")", Verdict::Error),
    ("!0 = !DIBasicType(name: 3)", Verdict::Error),
    ("!0 = !DIBasicType(tag: DW_TAG_bogus, name: \"i\")", Verdict::Error),
    ("!0 = !DIBasicType(name: \"i\", encoding: DW_ATE_bogus)", Verdict::Error),
    ("!0 = !DIBasicType(name: \"i\", flags: DIFlagBogus)", Verdict::Error),
    ("!0 = !DIBasicType(name: \"i\", size: 18446744073709551616)", Verdict::Error),
    ("!0 = !DIBasicType(name: \"i\", size: 32", Verdict::Error),
    ("!0 = !DIBasicType(name: \"i\", size: 32, size)", Verdict::Error),
    ("!0 = !DIDerivedType(tag: DW_TAG_pointer_type)", Verdict::Error),
    ("!0 = !DIFile(filename: \"a.c\")", Verdict::Error),
    ("!0 = !DIFile(filename: \"a.c\", directory: \"/\", checksumkind: CSK_Bogus, checksum: \"0\")", Verdict::Error),
    ("!0 = !DIEnumerator(name: \"a\")", Verdict::Error),
    ("!0 = !DIEnumerator(name: \"a\", value: 1, isUnsigned: 1)", Verdict::Error),
    ("!0 = !DINamespace(name: \"n\", scope: null, exportSymbols: yes)", Verdict::Error),
    ("!0 = !DISubprogram(name: \"f\", spFlags: DISPFlagDefinition)", Verdict::Error),
    ("!0 = !DISubprogram(name: \"f\")", Verdict::Error),
    ("!0 = distinct !DISubprogram(name: \"f\", spFlags: DISPFlagBogus)", Verdict::Error),
    ("!0 = !DICompileUnit(language: DW_LANG_C11, file: !1)\n!1 = !DIFile(filename: \"a.c\", directory: \"/\")", Verdict::Error),
    ("!0 = !DILocation(line: 1, column: 2)", Verdict::Error),
    ("!0 = !DILocation(line: 1, column: 70000, scope: !1)\n!1 = distinct !DISubprogram(name: \"f\")", Verdict::Error),
    ("!0 = !DILocation(line: 1, scope: !1, bogus: 2)\n!1 = distinct !DISubprogram(name: \"f\")", Verdict::Error),
    ("!0 = !DILocation(line: 1, scope: !9)", Verdict::Error),
    ("!0 = !DILocalVariable(name: \"x\", arg: 70000, scope: !1)\n!1 = distinct !DISubprogram(name: \"f\")", Verdict::Error),
    ("!0 = !DIGlobalVariableExpression(var: !1)\n!1 = distinct !DIGlobalVariable(name: \"x\")", Verdict::Error),
    ("!0 = !DILabel(scope: !1, name: \"l\", file: !2)\n!1 = distinct !DISubprogram(name: \"f\")\n!2 = !DIFile(filename: \"a.c\", directory: \"/\")", Verdict::Error),
    ("!0 = !DIImportedEntity(tag: DW_TAG_imported_module, scope: !1, line: \"x\")\n!1 = !DINamespace(scope: null)", Verdict::Error),
    ("!0 = !DISubrange(count: !DIExpression(DW_OP_bogus))", Verdict::Error),
    ("!0 = !DIExpression(DW_OP_bogus)", Verdict::Error),
    ("!0 = !DIExpression(DW_OP_plus_uconst, -4)", Verdict::Error),
    ("!0 = !DIArgList(i32 1)", Verdict::Error),
    ("!0 = !DISubroutineType(types: !{null, !1})\n!1 = !DIBasicType(name: \"i\"", Verdict::Error),
    ("!0 = !DIBasicType(name: \"i\")\n!0 = !DIBasicType(name: \"j\")", Verdict::Error),
    ("define void @f(i32 %x) {\n    #dbg_value(i32 %x, !0, !DIExpression())\n  ret void\n}\n!0 = !{}", Verdict::Error),
    ("define void @f(i32 %x) {\n    #dbg_bogus(i32 %x, !0, !DIExpression(), !0)\n  ret void\n}\n!0 = !{}", Verdict::Error),
    ("define void @f(i32 %x) {\n    #dbg_value(i32 %x, !0, !DIExpression(), !9)\n  ret void\n}\n!0 = !{}", Verdict::Error),
    ("define void @f(i32 %x) {\n    #dbg_value(i32 %y, !0, !DIExpression(), !0)\n  ret void\n}\n!0 = !{}", Verdict::Error),
    ("define void @f(i32 %x) {\n    #dbg_value(i32 %x, !0, !DIExpression(), !0\n  ret void\n}\n!0 = !{}", Verdict::Error),
    ("define void @f(i32 %x) {\n    #dbg_label(!0)\n  ret void\n}\n!0 = !{}", Verdict::Error),
    // Accepted, with the debug information dropped for want of a
    // `Debug Info Version`.
    ("!llvm.dbg.cu = !{!0}\n!0 = !DIFile(filename: \"a.c\", directory: \"/\")", Verdict::Refused),
    ("define void @f() !dbg !0 {\n  ret void\n}\n!0 = !DIFile(filename: \"a.c\", directory: \"/\")", Verdict::Refused),
    ("@x = global i32 0, !dbg !0\n!0 = !DIFile(filename: \"a.c\", directory: \"/\")", Verdict::Refused),
    ("define void @f(i32 %x) {\n  ret void, !dbg !1\n}\n!1 = !DILocation(line: 1, scope: !2)\n!2 = !DIFile(filename: \"a.c\", directory: \"/\")", Verdict::Refused),
    // Kinds hod_ir does not represent.
    ("!0 = !DIMacro(type: DW_MACINFO_define, line: 1, name: \"A\", value: \"1\")", Verdict::Refused),
    ("!0 = !DIStringType(name: \"s\")", Verdict::Refused),
    // Accepted by both; an unused node is not checked by LLVM's verifier.
    ("!0 = !DICompositeType(tag: DW_TAG_structure_type, elements: !{!1, !1})\n!1 = !DIBasicType(name: \"i\")", Verdict::Read),
    ("!0 = !DIExpression(DW_OP_LLVM_fragment, 0, 32)", Verdict::Read),
    ("!0 = !DIBasicType(name: \"\")", Verdict::Read),
    ("!0 = !DIBasicType(name: \"i\", flags: 3)", Verdict::Read),
    ("!0 = !DIFile(filename: \"\", directory: \"\")", Verdict::Read),
    ("!0 = distinct !DICompositeType(tag: DW_TAG_structure_type, elements: !{!0})", Verdict::Read),
    ("!0 = !DIDerivedType(tag: DW_TAG_pointer_type, baseType: !1)\n!1 = !DICompositeType(tag: DW_TAG_structure_type, elements: !{!0})", Verdict::Read),
];

/// §7.4: the negative cases give errors, never panics, where `llvm-as`
/// fails, and are read or refused by name where it does not.
#[test]
fn malformed_debug_information_is_an_error_where_llvm_as_says_so() {
    let llvms = Llvm::all();
    for &(case, verdict) in NEGATIVE {
        let text = format!("{}{case}\n", header(Arch::Aarch64, 22));
        let got = match read(text.as_bytes()) {
            Err(error) => {
                assert!(
                    !matches!(error.kind, LlErrorKind::Limit),
                    "{case}: {error}"
                );
                Verdict::Error
            }
            Ok((_, _, unsupported)) if unsupported.iter().any(|u| u.starts_with("debug:")) => {
                Verdict::Refused
            }
            Ok((_, _, unsupported)) => {
                assert_eq!(unsupported, Vec::<String>::new(), "{case}");
                Verdict::Read
            }
        };
        assert_eq!(got, verdict, "{case}");
        for llvm in &llvms {
            let fails = llvm.accepts(text.as_bytes()).is_err();
            assert_eq!(fails, verdict == Verdict::Error, "{case}: llvm-as {}", llvm.major);
        }
    }
}
