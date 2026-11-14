//! WP-189 (`spec/dwarf.md` §6.24): the debug-information refusals a
//! real project met at `-O0 -g`, on both targets, DWARF 5 and 4:
//!
//! - a template value parameter whose value is the address of a symbol
//!   this object does not define (`template<void (*F)()>` of an extern
//!   function, `template<int &R>` of an extern variable) is
//!   `DW_AT_location (DW_OP_addrx N, DW_OP_stack_value)` (`DW_OP_addr` in
//!   version 4) relocated against the undefined symbol, as `llc` writes
//!   it; it was `debug:template-address`;
//! - an explicit member function with a ref-qualifier (`explicit operator
//!   T() const &`) is the family `subprogram (member declaration,
//!   explicit, qualified)`; it was `debug:die-unsupported`;
//! - a `debug:die-unsupported` refusal's detail names the node's kind and
//!   tag.
//!
//! Each module's DIE tree is compared with `llc` 22's (`-O0
//! -global-isel=0 -fast-isel=0`) line by line, offsets and indices masked,
//! when `llc` is installed; both objects verify.

use std::path::Path;
use std::time::Duration;

use hod::{CompileError, Construct, OutputKind, Report, Settings, compile_with};
use hod_testutil::{Scratch, run_with, tool};

const TRIPLES: [&str; 2] = ["aarch64-unknown-linux-gnu", "x86_64-unknown-linux-gnu"];

/// Template parameters taking addresses: of an extern function and
/// variable (undefined here), of a defined function and variable, a
/// reference to the extern variable, and the same extern symbol twice.
const TEMPLATES: &str = r#"target triple = "TRIPLE"

@def_var = dso_local global i32 0, align 4, !dbg !40
@ext_var = external global i32, align 4

define dso_local void @_Z6def_fnv() #0 !dbg !10 {
entry:
  ret void, !dbg !16
}

declare void @_Z6ext_fnv()

attributes #0 = { noinline nounwind optnone "frame-pointer"="all" }

!llvm.dbg.cu = !{!0}
!llvm.module.flags = !{!2, !3}

!0 = distinct !DICompileUnit(language: DW_LANG_C_plus_plus_14, file: !1, producer: "kiln", isOptimized: false, runtimeVersion: 0, emissionKind: FullDebug, retainedTypes: !20, globals: !43, splitDebugInlining: false, nameTableKind: None)
!1 = !DIFile(filename: "templates.cpp", directory: "/hod/tests")
!2 = !{i32 7, !"Dwarf Version", i32 VERSION}
!3 = !{i32 2, !"Debug Info Version", i32 3}
!10 = distinct !DISubprogram(name: "def_fn", linkageName: "_Z6def_fnv", scope: !1, file: !1, line: 1, type: !11, scopeLine: 1, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !0)
!11 = !DISubroutineType(types: !12)
!12 = !{null}
!13 = !DIBasicType(name: "int", size: 32, encoding: DW_ATE_signed)
!14 = !DIDerivedType(tag: DW_TAG_pointer_type, baseType: !11, size: 64)
!15 = !DIDerivedType(tag: DW_TAG_pointer_type, baseType: !13, size: 64)
!16 = !DILocation(line: 1, column: 15, scope: !10)
!17 = !DIDerivedType(tag: DW_TAG_reference_type, baseType: !13, size: 64)
!20 = !{!21, !24, !27, !30, !33, !36}
!21 = distinct !DICompositeType(tag: DW_TAG_structure_type, name: "SF<&ext_fn>", file: !1, line: 3, size: 8, flags: DIFlagTypePassByValue, elements: !4, templateParams: !22, identifier: "_ZTS2SFIXadL_Z6ext_fnvEEE")
!22 = !{!23}
!23 = !DITemplateValueParameter(name: "F", type: !14, value: ptr @_Z6ext_fnv)
!24 = distinct !DICompositeType(tag: DW_TAG_structure_type, name: "SF<&def_fn>", file: !1, line: 3, size: 8, flags: DIFlagTypePassByValue, elements: !4, templateParams: !25, identifier: "_ZTS2SFIXadL_Z6def_fnvEEE")
!25 = !{!26}
!26 = !DITemplateValueParameter(name: "F", type: !14, value: ptr @_Z6def_fnv)
!27 = distinct !DICompositeType(tag: DW_TAG_structure_type, name: "SA<&ext_var>", file: !1, line: 4, size: 8, flags: DIFlagTypePassByValue, elements: !4, templateParams: !28, identifier: "_ZTS2SAIXadL_Z7ext_varEEE")
!28 = !{!29}
!29 = !DITemplateValueParameter(name: "P", type: !15, value: ptr @ext_var)
!30 = distinct !DICompositeType(tag: DW_TAG_structure_type, name: "SA<&def_var>", file: !1, line: 4, size: 8, flags: DIFlagTypePassByValue, elements: !4, templateParams: !31, identifier: "_ZTS2SAIXadL_Z7def_varEEE")
!31 = !{!32}
!32 = !DITemplateValueParameter(name: "P", type: !15, value: ptr @def_var)
!33 = distinct !DICompositeType(tag: DW_TAG_structure_type, name: "SR<ext_var>", file: !1, line: 5, size: 8, flags: DIFlagTypePassByValue, elements: !4, templateParams: !34, identifier: "_ZTS2SRIL_Z7ext_varEE")
!34 = !{!35}
!35 = !DITemplateValueParameter(name: "R", type: !17, value: ptr @ext_var)
!36 = distinct !DICompositeType(tag: DW_TAG_structure_type, name: "SB<&ext_fn, &ext_var>", file: !1, line: 6, size: 8, flags: DIFlagTypePassByValue, elements: !4, templateParams: !37, identifier: "_ZTS2SBIXadL_Z6ext_fnvEEXadL_Z7ext_varEEE")
!37 = !{!38, !39}
!38 = !DITemplateValueParameter(name: "G", type: !14, value: ptr @_Z6ext_fnv)
!39 = !DITemplateValueParameter(name: "Q", type: !15, value: ptr @ext_var)
!4 = !{}
!40 = !DIGlobalVariableExpression(var: !41, expr: !DIExpression())
!41 = distinct !DIGlobalVariable(name: "def_var", scope: !0, file: !1, line: 2, type: !13, isLocal: false, isDefinition: true)
!43 = !{!40}
"#;

/// A class whose member functions are explicit and ref-qualified
/// (`explicit operator int() const &`, `explicit operator long() &&`, a
/// virtual one, a protected deleted one, one with no linkage name).
const EXPLICIT: &str = r#"target triple = "TRIPLE"

define dso_local void @f() #0 !dbg !10 {
entry:
  ret void, !dbg !16
}

attributes #0 = { noinline nounwind optnone "frame-pointer"="all" }

!llvm.dbg.cu = !{!0}
!llvm.module.flags = !{!2, !3}

!0 = distinct !DICompileUnit(language: DW_LANG_C_plus_plus_14, file: !1, producer: "kiln", isOptimized: false, runtimeVersion: 0, emissionKind: FullDebug, retainedTypes: !19, splitDebugInlining: false, nameTableKind: None)
!1 = !DIFile(filename: "explicit.cpp", directory: "/hod/tests")
!2 = !{i32 7, !"Dwarf Version", i32 VERSION}
!3 = !{i32 2, !"Debug Info Version", i32 3}
!4 = !{}
!10 = distinct !DISubprogram(name: "f", linkageName: "f", scope: !1, file: !1, line: 9, type: !11, scopeLine: 9, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !0)
!11 = !DISubroutineType(types: !12)
!12 = !{null}
!13 = !DIBasicType(name: "int", size: 32, encoding: DW_ATE_signed)
!14 = !DIBasicType(name: "long", size: 64, encoding: DW_ATE_signed)
!15 = !DIBasicType(name: "bool", size: 8, encoding: DW_ATE_boolean)
!16 = !DILocation(line: 9, column: 1, scope: !10)
!19 = !{!20}
!20 = distinct !DICompositeType(tag: DW_TAG_structure_type, name: "S", file: !1, line: 1, size: 64, flags: DIFlagTypePassByReference | DIFlagNonTrivial, elements: !21, vtableHolder: !20, identifier: "_ZTS1S")
!21 = !{!22, !24, !28, !32, !36, !40}
!22 = !DIDerivedType(tag: DW_TAG_member, name: "_vptr$S", scope: !1, file: !1, baseType: !23, size: 64, flags: DIFlagArtificial)
!23 = !DIDerivedType(tag: DW_TAG_pointer_type, baseType: null, size: 64)
!24 = !DISubprogram(name: "operator int", linkageName: "_ZNKR1ScviEv", scope: !20, file: !1, line: 2, type: !25, scopeLine: 2, flags: DIFlagExplicit | DIFlagPrototyped | DIFlagLValueReference, spFlags: 0)
!25 = !DISubroutineType(types: !26)
!26 = !{!13, !27}
!27 = !DIDerivedType(tag: DW_TAG_pointer_type, baseType: !44, size: 64, flags: DIFlagArtificial | DIFlagObjectPointer)
!28 = !DISubprogram(name: "operator long", linkageName: "_ZNO1ScvlEv", scope: !20, file: !1, line: 3, type: !29, scopeLine: 3, flags: DIFlagExplicit | DIFlagPrototyped | DIFlagRValueReference, spFlags: 0)
!29 = !DISubroutineType(types: !30)
!30 = !{!14, !31}
!31 = !DIDerivedType(tag: DW_TAG_pointer_type, baseType: !20, size: 64, flags: DIFlagArtificial | DIFlagObjectPointer)
!32 = !DISubprogram(name: "operator bool", linkageName: "_ZNKR1ScvbEv", scope: !20, file: !1, line: 4, type: !33, scopeLine: 4, containingType: !20, virtualIndex: 0, flags: DIFlagExplicit | DIFlagPrototyped | DIFlagLValueReference, spFlags: DISPFlagVirtual)
!33 = !DISubroutineType(types: !34)
!34 = !{!15, !27}
!36 = !DISubprogram(name: "operator char", linkageName: "_ZNR1ScvcEv", scope: !20, file: !1, line: 6, type: !37, scopeLine: 6, flags: DIFlagProtected | DIFlagExplicit | DIFlagPrototyped | DIFlagLValueReference, spFlags: DISPFlagDeleted)
!37 = !DISubroutineType(types: !38)
!38 = !{!39, !31}
!39 = !DIBasicType(name: "char", size: 8, encoding: DW_ATE_signed_char)
!40 = !DISubprogram(name: "operator short", scope: !20, file: !1, line: 7, type: !41, scopeLine: 7, flags: DIFlagExplicit | DIFlagPrototyped | DIFlagRValueReference, spFlags: DISPFlagLocalToUnit)
!41 = !DISubroutineType(types: !42)
!42 = !{!43, !31}
!43 = !DIBasicType(name: "short", size: 16, encoding: DW_ATE_signed)
!44 = !DIDerivedType(tag: DW_TAG_const_type, baseType: !20)
"#;

fn module(template: &str, triple: &str, version: u32) -> String {
    template.replace("TRIPLE", triple).replace("VERSION", &version.to_string())
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
/// sizes and indices masked; without the unit's `producer` and the
/// function's code range.
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

/// A line with a constant `(0x08)` read as `(8)` (the dump's radix is not
/// the writer's, `spec/dwarf.md` §6.7), every other hexadecimal number
/// (an offset, an index, an address) masked.
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

/// `(symbol, addend)` of each `.debug_addr` relocation of `object`.
fn addr_relocations(readelf: &Path, object: &Path) -> Vec<(String, String)> {
    let text = run_tool(readelf, &["-rW".as_ref(), object.as_os_str()]);
    let mut out = Vec::new();
    let mut inside = false;
    for line in text.lines() {
        if line.starts_with("Relocation section") {
            inside = line.contains("'.rela.debug_addr'") || line.contains("'.rela.debug_info'");
            continue;
        }
        let fields: Vec<&str> = line.split_whitespace().collect();
        if inside && fields.len() >= 7 && fields[0].chars().all(|c| c.is_ascii_hexdigit()) {
            out.push((fields[4].to_owned(), fields[6].to_owned()));
        }
    }
    out
}

/// hod's object of `text`, compared with `llc` 22's when it is installed:
/// the same DIE tree, both verified. Returns hod's tree.
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
    let a: Vec<&String> = ours.iter().filter(producer).collect();
    let b: Vec<&String> = theirs.iter().filter(producer).collect();
    if let Some(i) = (0..a.len().max(b.len())).find(|&i| a.get(i) != b.get(i)) {
        panic!(
            "{name}: line {i}: hod {:?}, llc {:?}\nhod:\n{}",
            a.get(i),
            b.get(i),
            ours.join("\n")
        );
    }
    if let Some(readelf) = tool("llvm-readelf") {
        let symbols = |o: &Path| {
            let mut r = addr_relocations(&readelf, o);
            r.retain(|(s, _)| s.contains("ext_"));
            r.sort();
            r.dedup();
            r
        };
        assert_eq!(
            symbols(&hod),
            symbols(&reference),
            "{name}: relocations against extern symbols"
        );
    }
    ours
}

#[test]
fn template_addresses_of_undefined_symbols_are_written_as_llc_writes_them() {
    let scratch = Scratch::new("wp189-templates");
    for triple in TRIPLES {
        for version in [5, 4] {
            let name = format!("templates-{}-{version}", &triple[..6]);
            let text = module(TEMPLATES, triple, version);
            let ours = compare(&scratch, &name, &text);
            if ours.is_empty() {
                continue;
            }
            let op = if version == 5 { "DW_OP_addrx" } else { "DW_OP_addr " };
            let located = ours
                .iter()
                .filter(|l| l.starts_with("DW_AT_location") && l.contains(op))
                .filter(|l| l.contains("DW_OP_stack_value"))
                .count();
            assert_eq!(located, 7, "{name}: every address parameter has a location\n{ours:?}");
            // The relocations name the undefined symbols themselves.
            let object = compile(&text).unwrap();
            let hod = scratch.write(&format!("{name}.again.o"), &object);
            if let Some(readelf) = tool("llvm-readelf") {
                let relocs = addr_relocations(&readelf, &hod);
                for symbol in ["ext_var", "_Z6ext_fnv"] {
                    assert!(
                        relocs.iter().any(|(s, a)| s == symbol && a == "0"),
                        "{name}: {symbol} + 0 in {relocs:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn explicit_ref_qualified_members_are_written_as_llc_writes_them() {
    let scratch = Scratch::new("wp189-explicit");
    for triple in TRIPLES {
        for version in [5, 4] {
            let name = format!("explicit-{}-{version}", &triple[..6]);
            let ours = compare(&scratch, &name, &module(EXPLICIT, triple, version));
            if ours.is_empty() {
                continue;
            }
            let count = |at: &str| ours.iter().filter(|l| l.starts_with(at)).count();
            assert_eq!(count("DW_AT_explicit"), 5, "{name}");
            assert_eq!(count("DW_AT_reference"), 3, "{name}");
            assert_eq!(count("DW_AT_rvalue_reference"), 2, "{name}");
            // `llc` writes `DW_AT_deleted` only from version 5.
            assert_eq!(count("DW_AT_deleted"), usize::from(version == 5), "{name}");
        }
    }
}

/// A node the DIE writer does not write is refused as
/// `debug:die-unsupported` with a detail naming its kind and tag: here a
/// pointer type with an address space.
#[test]
fn a_die_refusal_names_the_node_kind_and_tag() {
    let text = module(TEMPLATES, TRIPLES[0], 5).replace(
        "!15 = !DIDerivedType(tag: DW_TAG_pointer_type, baseType: !13, size: 64)",
        "!15 = !DIDerivedType(tag: DW_TAG_pointer_type, baseType: !13, size: 64, dwarfAddressSpace: 1)",
    );
    match compile(&text) {
        Err(CompileError::Unsupported(u)) => {
            assert_eq!(u.construct, Construct::Named("debug:die-unsupported".into()));
            let detail = u.detail.unwrap_or_default();
            assert!(detail.starts_with("DIDerivedType DW_TAG_pointer_type (node "), "{detail}");
            assert!(detail.ends_with("a pointer with an address space"), "{detail}");
        }
        other => panic!("expected a decline, got {:?}", other.map(|o| o.len())),
    }
}

/// Thread-local variables on x86-64: a global, an internal one and a
/// function's static.
const TLS: &str = r#"target triple = "x86_64-unknown-linux-gnu"

@t = dso_local thread_local global i32 4, align 4, !dbg !20
@_ZL1u = internal thread_local global i32 0, align 4, !dbg !23

define dso_local i32 @main() #0 !dbg !10 {
entry:
  %a = load i32, ptr @t, align 4, !dbg !16
  %b = load i32, ptr @_ZL1u, align 4, !dbg !16
  %c = add i32 %a, %b, !dbg !16
  ret i32 %c, !dbg !16
}

attributes #0 = { noinline nounwind optnone "frame-pointer"="all" }

!llvm.dbg.cu = !{!0}
!llvm.module.flags = !{!2, !3}

!0 = distinct !DICompileUnit(language: DW_LANG_C_plus_plus_14, file: !1, producer: "kiln", isOptimized: false, runtimeVersion: 0, splitDebugFilename: "tls.dwo", emissionKind: FullDebug, globals: !19, splitDebugInlining: false, nameTableKind: None)
!1 = !DIFile(filename: "tls.cpp", directory: "/hod/tests")
!2 = !{i32 7, !"Dwarf Version", i32 5}
!3 = !{i32 2, !"Debug Info Version", i32 3}
!4 = !{}
!10 = distinct !DISubprogram(name: "main", scope: !1, file: !1, line: 2, type: !11, scopeLine: 2, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !0)
!11 = !DISubroutineType(types: !12)
!12 = !{!13}
!13 = !DIBasicType(name: "int", size: 32, encoding: DW_ATE_signed)
!16 = !DILocation(line: 2, column: 14, scope: !10)
!19 = !{!20, !23}
!20 = !DIGlobalVariableExpression(var: !21, expr: !DIExpression())
!21 = distinct !DIGlobalVariable(name: "t", scope: !0, file: !1, line: 1, type: !13, isLocal: false, isDefinition: true)
!23 = !DIGlobalVariableExpression(var: !24, expr: !DIExpression())
!24 = distinct !DIGlobalVariable(name: "u", linkageName: "_ZL1u", scope: !0, file: !1, line: 1, type: !13, isLocal: true, isDefinition: true)
"#;

/// A split unit's thread-local variables (WP-189): `DW_OP_GNU_const_index`
/// of a `.debug_addr` entry relocated by `R_X86_64_DTPOFF64`, as `llc`
/// writes them; the object and its `.dwo` verify.
#[test]
fn split_thread_local_variables_are_written_as_llc_writes_them() {
    let scratch = Scratch::new("wp189-tls");
    let dwo = scratch.join("tls.out.dwo");
    let options = hod::Options {
        output_kind: OutputKind::Object,
        split_dwarf_file: Some("tls.dwo".into()),
        split_dwarf_output: Some(dwo.clone()),
        ..hod::Options::default()
    };
    let object =
        compile_with(TLS.as_bytes(), &options, &Settings::default(), &mut Report::default())
            .unwrap_or_else(|e| panic!("{e}"));
    let hod = scratch.write("tls.hod.o", &object);
    let (Some(dwarfdump), Some(readelf)) = (tool("llvm-dwarfdump"), tool("llvm-readelf")) else {
        return;
    };
    for file in [&hod, &dwo] {
        let verified = run_tool(&dwarfdump, &["--verify".as_ref(), file.as_os_str()]);
        assert!(verified.contains("No errors."), "{}: {verified}", file.display());
    }
    let locations = |dwo: &Path| -> Vec<String> {
        let text = run_tool(&dwarfdump, &["--debug-info".as_ref(), dwo.as_os_str()]);
        text.lines().filter(|l| l.contains("DW_AT_location")).map(|l| l.trim().to_owned()).collect()
    };
    let tls_relocations = |object: &Path| -> Vec<String> {
        let text = run_tool(&readelf, &["-rW".as_ref(), object.as_os_str()]);
        let mut out: Vec<String> = text
            .lines()
            .filter(|l| l.contains("R_X86_64_DTPOFF64"))
            .map(|l| l.split_whitespace().nth(4).unwrap_or_default().to_owned())
            .collect();
        out.sort();
        out
    };
    let ours = locations(&dwo);
    assert_eq!(ours.len(), 2, "{ours:?}");
    for l in &ours {
        assert!(l.contains("DW_OP_GNU_const_index") && l.ends_with("DW_OP_GNU_push_tls_address)"));
    }
    assert_eq!(tls_relocations(&hod), ["_ZL1u", "t"]);
    let Some(llc) = tool("llc-22") else { return };
    let ll = scratch.write("tls.ll", TLS);
    let reference = scratch.join("tls.llc.o");
    let llc_dwo = scratch.join("tls.llc.dwo");
    let mut command = std::process::Command::new(&llc);
    command
        .args(["-O0", "-filetype=obj", "-split-dwarf-file=tls.dwo"])
        .arg(format!("-split-dwarf-output={}", llc_dwo.display()))
        .arg("-o")
        .arg(&reference)
        .arg(&ll);
    let out = run_with(&mut command, &[], Duration::from_mins(2));
    assert!(out.success(), "llc: {}", out.stderr_text());
    assert_eq!(locations(&llc_dwo), ours);
    assert_eq!(tls_relocations(&reference), tls_relocations(&hod));
}
