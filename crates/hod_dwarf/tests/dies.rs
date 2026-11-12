//! `spec/dwarf.md` §8.6: the DIE writer on objects built with `hod_obj`.
//! With `llvm-dwarfdump --verify` of every installed major over a spread
//! of inputs (no globals, many globals, nested namespaces, long and
//! duplicate strings, code in several sections and in a COMDAT group);
//! the relocations of both targets; identical bytes whatever the order
//! and chunking of the inputs; errors before the object is touched.

mod common;

use common::dies::{dump, globals_module, verify_all};
use common::meta::{Loaded, load};
use common::{Elf, dwarfdumps};
use hod_dwarf::{
    CodeSpan, Error, GlobalAddr, Row, Sequence, UnitInput, UnitPlan, flags, write_debug_info,
    write_debug_line,
};
use hod_ir::{DebugInfo, DiFieldValue, DiId, DiKind, DiNode, dw_ate, dw_lang, dw_tag};
use hod_obj::{
    Arch, Binding, GroupKind, Object, SectionKind, Symbol, SymbolId, SymbolKind, SymbolPlace,
};
use hod_testutil::Scratch;

const ARCHES: [Arch; 2] = [Arch::AArch64, Arch::X86_64];

fn data(name: &str) -> String {
    let path = format!("{}/tests/data/dies/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

/// Code for a test object: sections, each with an optional COMDAT
/// signature and its functions' sizes.
struct Code<'c> {
    sections: &'c [(&'c str, bool, &'c [u64])],
}

const NO_CODE: Code<'static> = Code { sections: &[] };

/// An object with the globals of `loaded` in `.data` and the functions
/// of `code`, and what the writer needs to describe them.
/// (`Object` is not `Clone`: [`Built::obj`] builds it again, the same.)
struct Built<'c> {
    arch: Arch,
    names: Vec<(String, DiId)>,
    code: &'c Code<'c>,
    globals: Vec<GlobalAddr>,
    spans: Vec<CodeSpan>,
}

impl Built<'_> {
    fn obj(&self) -> Object {
        make(self.arch, &self.names, self.code).0
    }
}

fn build<'c>(arch: Arch, loaded: &Loaded, code: &'c Code<'c>) -> Built<'c> {
    let (_, globals, spans) = make(arch, &loaded.globals, code);
    Built { arch, names: loaded.globals.clone(), code, globals, spans }
}

fn make(
    arch: Arch,
    names: &[(String, DiId)],
    code: &Code<'_>,
) -> (Object, Vec<GlobalAddr>, Vec<CodeSpan>) {
    let mut obj = Object::new(arch);
    let mut globals = Vec::new();
    if !names.is_empty() {
        let sec = obj.add_section(b".data", SectionKind::Data, 16).unwrap();
        for (name, expr) in names {
            let value = obj.append_zeros(sec, 16);
            let name = obj.add_string(name.as_bytes()).unwrap();
            let symbol = obj.add_symbol(Symbol {
                value,
                size: 16,
                place: SymbolPlace::Section(sec),
                kind: SymbolKind::Object,
                ..Symbol::undefined(name)
            });
            globals.push(GlobalAddr { expr: *expr, symbol, addend: 0 });
        }
    }
    let mut spans = Vec::new();
    for (k, &(name, comdat, sizes)) in code.sections.iter().enumerate() {
        let sec = obj.add_section(name.as_bytes(), SectionKind::Text, 4).unwrap();
        let section_symbol = obj.section_symbol(sec);
        for (f, &size) in sizes.iter().enumerate() {
            let at = obj.append_zeros(sec, size);
            let fname = obj.add_string(format!("f{k}_{f}").as_bytes()).unwrap();
            let fsym = obj.add_symbol(Symbol {
                value: at,
                size,
                place: SymbolPlace::Section(sec),
                kind: SymbolKind::Func,
                binding: if comdat { Binding::Weak } else { Binding::Global },
                ..Symbol::undefined(fname)
            });
            if comdat && f == 0 {
                let g = obj.add_group(fsym, GroupKind::Comdat);
                obj.add_group_member(g, sec).unwrap();
            }
            spans.push(CodeSpan { symbol: section_symbol, addend: at as i64, size });
        }
    }
    (obj, globals, spans)
}

/// Writes the line table and the DIEs of `loaded`'s first unit into
/// `b.obj()` (with `globals` and `code` as given) and returns the object's
/// bytes.
fn write(b: &Built, plan: &UnitPlan<'_>, globals: &[GlobalAddr], code: &[CodeSpan]) -> Vec<u8> {
    let mut obj = b.obj();
    let rows: Vec<[Row; 1]> = b
        .spans
        .iter()
        .map(|_| [Row { offset: 0, file: 0, line: 1, column: 0, flags: flags::IS_STMT }])
        .collect();
    let seqs: Vec<Sequence<'_>> = b
        .spans
        .iter()
        .zip(&rows)
        .enumerate()
        .map(|(i, (s, r))| Sequence {
            order: i as u32,
            symbol: s.symbol,
            addend: s.addend,
            size: s.size as u32,
            rows: r,
        })
        .collect();
    let line = write_debug_line(&mut obj, &plan.files().header(), &seqs).unwrap();
    let input = UnitInput { plan, line_offset: 0, globals, code };
    write_debug_info(&mut obj, line.line, &[input]).unwrap();
    hod_obj::elf::to_vec(&obj).unwrap()
}

fn object(arch: Arch, loaded: &Loaded, code: &Code<'_>) -> Vec<u8> {
    let plan = UnitPlan::new(&loaded.debug, loaded.units[0]).unwrap();
    let b = build(arch, loaded, code);
    write(&b, &plan, &b.globals, &b.spans)
}

/// A module with a unit and no globals.
fn empty_module() -> String {
    "!llvm.dbg.cu = !{!0}\n\
     !0 = distinct !DICompileUnit(language: DW_LANG_C_plus_plus_14, file: !1, producer: \"p\", isOptimized: false, runtimeVersion: 0, emissionKind: FullDebug, nameTableKind: None)\n\
     !1 = !DIFile(filename: \"empty.cpp\", directory: \"/hod\")\n"
        .to_string()
}

const SEVERAL: Code<'static> = Code {
    sections: &[
        (".text", false, &[24, 40, 8]),
        (".text._Z6inlinev", true, &[16]),
        (".text.startup", false, &[12]),
        (".text._Z4tmplIiEvv", true, &[32]),
    ],
};
const ONE: Code<'static> = Code { sections: &[(".text", false, &[24, 40])] };

fn cases() -> Vec<(&'static str, String, Code<'static>)> {
    vec![
        ("empty", empty_module(), NO_CODE),
        ("no-globals", empty_module(), ONE),
        ("no-globals-several", empty_module(), SEVERAL),
        ("globals", data("globals.ll"), NO_CODE),
        ("globals-one", data("globals.ll"), ONE),
        ("strings", data("strings.ll"), SEVERAL),
        ("main", data("main.ll"), ONE),
        ("many", globals_module(3000), SEVERAL),
    ]
}

// ------------------------------------------------------------ verify ----

/// §8.6: every case verifies on every installed major, and the dump
/// names what was written.
#[test]
fn verify_spread() {
    let tools = dwarfdumps();
    if tools.is_empty() {
        assert!(!hod_testutil::tools_required(), "llvm-dwarfdump is required");
        eprintln!("skipped: no llvm-dwarfdump");
        return;
    }
    let scratch = Scratch::new("dies-verify");
    for (label, text, code) in cases() {
        let loaded = load(&text);
        for arch in ARCHES {
            let bytes = object(arch, &loaded, &code);
            for (major, tool) in &tools {
                let name = format!("{label}-{arch:?}-{major}.o");
                verify_all(tool, &scratch, &name, &bytes);
                let info = dump(tool, &scratch, &name, &bytes, &["--debug-info"]);
                assert!(info.contains("DW_TAG_compile_unit"), "{name}");
                let ranges = code.sections.len() > 1;
                assert_eq!(info.contains("DW_AT_ranges"), ranges, "{name}");
                assert_eq!(info.contains("DW_AT_high_pc"), code.sections.len() == 1, "{name}");
                if label == "globals" {
                    for n in ["\"deep_ptr\"", "\"bottom\"", "\"(anonymous namespace)\"", "\"v1\""] {
                        let n = if n.contains("anonymous") { "DW_TAG_namespace" } else { n };
                        assert!(info.contains(n), "{name}: {n}");
                    }
                    assert!(info.contains("DW_AT_export_symbols"), "{name}");
                    assert!(info.contains("DW_AT_alignment\t(32)"), "{name}");
                }
                if label == "many" {
                    let vars = info.matches("DW_TAG_variable").count();
                    assert_eq!(vars, 3000, "{name}");
                }
                if label == "strings" {
                    let long = format!("\"a_very_long_name_{}\"", "x".repeat(290));
                    assert!(info.contains(&long[..300]), "{name}");
                    assert!(info.contains(r#"("caf\303\251")"#), "{name}");
                }
            }
        }
    }
}

/// §6.3: one copy of each string in `.debug_str`, however often named.
#[test]
fn strings_are_pooled() {
    let loaded = load(&globals_module(400));
    let bytes = object(Arch::AArch64, &loaded, &ONE);
    let elf = Elf::parse(bytes);
    let s = elf.bytes(elf.section(".debug_str").unwrap());
    let mut seen = std::collections::HashSet::new();
    for part in s.split(|&b| b == 0).filter(|p| !p.is_empty()) {
        assert!(seen.insert(part), "{} twice", String::from_utf8_lossy(part));
    }
    assert!(seen.contains(&b"shared"[..]) && seen.contains(&b"mid1"[..]));
    // `.debug_str_offsets`: one entry per distinct string of the unit.
    let offs = elf.section(".debug_str_offsets").unwrap();
    assert_eq!((elf.sections[offs].size as usize - 8) / 4, seen.len());
}

// ------------------------------------------------------- relocations ----

/// §1.1 and §1.3: kinds and targets of every relocation, on both
/// targets; debug sections stay out of groups; the unit's `low_pc` with
/// ranges is not relocated.
#[test]
fn relocations() {
    let loaded = load(&data("globals.ll"));
    for arch in ARCHES {
        let (abs32, abs64) = match arch {
            Arch::AArch64 => (258, 257),
            Arch::X86_64 => (10, 1),
        };
        let elf = Elf::parse(object(arch, &loaded, &SEVERAL));
        let sec_sym = |name: &str| elf.section_symbol(elf.section(name).unwrap());
        let info = elf.section(".debug_info").unwrap();
        let mut relas = elf.relas(elf.rela_of(info).unwrap());
        relas.sort_by_key(|r| r.offset);
        let targets: Vec<(u64, u32, u32, i64)> =
            relas.iter().map(|r| (r.offset, r.sym, r.ty, r.addend)).collect();
        assert_eq!(
            targets,
            [
                (8, sec_sym(".debug_abbrev"), abs32, 0),
                // code (2), producer, language (2), name; then the bases
                // around comp_dir and `low_pc` (8 bytes, no relocation)
                // and `ranges`.
                (18, sec_sym(".debug_str_offsets"), abs32, 8),
                (22, sec_sym(".debug_line"), abs32, 0),
                (36, sec_sym(".debug_addr"), abs32, 8),
                (40, sec_sym(".debug_rnglists"), abs32, 12),
            ]
            .into_iter()
            .collect::<Vec<_>>(),
            "{arch:?}"
        );
        let so = elf.section(".debug_str_offsets").unwrap();
        for r in elf.relas(elf.rela_of(so).unwrap()) {
            assert_eq!((r.sym, r.ty), (sec_sym(".debug_str"), abs32));
        }
        let addr = elf.section(".debug_addr").unwrap();
        let mut entries = elf.relas(elf.rela_of(addr).unwrap());
        entries.sort_by_key(|r| r.offset);
        // `.text`'s three functions are contiguous: one entry.
        assert_eq!(entries.len(), loaded.globals.len() + SEVERAL.sections.len(), "{arch:?}");
        for r in &entries {
            assert_eq!(r.ty, abs64);
        }
        // The code entries: against the section symbol of each code
        // section, a COMDAT one included; one per merged run.
        let code: Vec<(u32, i64)> =
            entries[loaded.globals.len()..].iter().map(|r| (r.sym, r.addend)).collect();
        let want: Vec<(u32, i64)> =
            SEVERAL.sections.iter().map(|(name, _, _)| (sec_sym(name), 0)).collect();
        assert_eq!(code, want, "{arch:?}");
        for (i, s) in elf.sections.iter().enumerate() {
            assert!(elf.rela_of(i).is_none() || !s.name.starts_with(".debug_rnglists"));
            if s.ty == 17 {
                let members = elf.bytes(i);
                for m in members[4..].chunks(4) {
                    let m = u32::from_le_bytes(m.try_into().unwrap()) as usize;
                    assert!(!elf.sections[m].name.starts_with(".debug"), "{arch:?}: in a group");
                }
            }
        }
        let str_sec = &elf.sections[elf.section(".debug_str").unwrap()];
        assert_eq!((str_sec.flags & 0x30, str_sec.entsize), (0x30, 1));
    }
}

// ------------------------------------------------------- determinism ----

struct Lcg(u64);

impl Lcg {
    fn below(&mut self, n: usize) -> usize {
        self.0 =
            self.0.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        ((self.0 >> 33) % n as u64) as usize
    }
    fn shuffle<T>(&mut self, v: &mut [T]) {
        for i in (1..v.len()).rev() {
            v.swap(i, self.below(i + 1));
        }
    }
}

/// §7.1: the bytes depend on the inputs as sets: shuffled globals and
/// spans, spans split into pieces (and overlapping), chunks concatenated
/// in any order, plans built and written on other threads.
#[test]
fn order_and_chunking_do_not_matter() {
    let loaded = load(&globals_module(500));
    for arch in ARCHES {
        let plan = UnitPlan::new(&loaded.debug, loaded.units[0]).unwrap();
        let b = build(arch, &loaded, &SEVERAL);
        let reference = write(&b, &plan, &b.globals, &b.spans);
        let mut r = Lcg(7);
        for round in 0..8 {
            let mut globals = b.globals.clone();
            r.shuffle(&mut globals);
            // Split every span into pieces at random points; repeat some.
            let mut code = Vec::new();
            for s in &b.spans {
                let cut = 1 + r.below(s.size as usize - 1) as u64;
                code.push(CodeSpan { size: cut, ..*s });
                code.push(CodeSpan { addend: s.addend + cut as i64, size: s.size - cut, ..*s });
                if round % 2 == 1 {
                    code.push(CodeSpan { size: cut.max(1), ..*s });
                }
            }
            r.shuffle(&mut code);
            // Chunks: the inputs cut into pieces and joined in any order.
            let chunks = 1 + r.below(5);
            let mut parts: Vec<Vec<GlobalAddr>> = vec![Vec::new(); chunks];
            for g in globals {
                parts[r.below(chunks)].push(g);
            }
            r.shuffle(&mut parts);
            let globals: Vec<GlobalAddr> = parts.concat();
            assert_eq!(write(&b, &plan, &globals, &code), reference, "{arch:?} round {round}");
        }
        let same: Vec<Vec<u8>> = std::thread::scope(|s| {
            let handles: Vec<_> = (0..3)
                .map(|_| {
                    s.spawn(|| {
                        let plan = UnitPlan::new(&loaded.debug, loaded.units[0]).unwrap();
                        write(&b, &plan, &b.globals, &b.spans)
                    })
                })
                .collect();
            handles.into_iter().map(|h| h.join().unwrap()).collect()
        });
        for bytes in same {
            assert_eq!(bytes, reference, "{arch:?}: another thread");
        }
    }
}

// ------------------------------------------------------------ errors ----

/// §6.6: every error is reported before the object is touched.
#[test]
fn errors_leave_the_object_unchanged() {
    let loaded = load(&data("globals.ll"));
    let plan = UnitPlan::new(&loaded.debug, loaded.units[0]).unwrap();
    let b = build(Arch::AArch64, &loaded, &ONE);
    let base = || {
        let mut obj = b.obj();
        let line = write_debug_line(&mut obj, &plan.files().header(), &[]).unwrap();
        (obj, line)
    };
    let (base_obj, line) = base();
    let before = hod_obj::elf::to_vec(&base_obj).unwrap();
    let g0 = b.globals[0];
    let s0 = b.spans[0];
    let bad_symbol = SymbolId::from_index(base_obj.symbol_count() + 10);
    let cases: Vec<(Vec<GlobalAddr>, Vec<CodeSpan>, Error)> = vec![
        (
            vec![GlobalAddr { expr: loaded.units[0], ..g0 }],
            vec![],
            Error::GlobalNotInUnit { expr: loaded.units[0].raw() },
        ),
        (vec![g0, g0], vec![], Error::DuplicateGlobal { expr: g0.expr.raw() }),
        (
            vec![GlobalAddr { symbol: bad_symbol, ..g0 }],
            vec![],
            Error::BadSymbol { symbol: bad_symbol.index() },
        ),
        (
            vec![],
            vec![s0, CodeSpan { symbol: bad_symbol, ..s0 }],
            Error::BadSymbol { symbol: bad_symbol.index() },
        ),
        (vec![], vec![CodeSpan { addend: -4, ..s0 }], Error::BadSpan { index: 0 }),
        (vec![], vec![s0, CodeSpan { size: u64::MAX, ..s0 }], Error::BadSpan { index: 1 }),
    ];
    for (globals, code, want) in cases {
        let mut obj = base().0;
        let input = UnitInput { plan: &plan, line_offset: 0, globals: &globals, code: &code };
        assert_eq!(write_debug_info(&mut obj, line.line, &[input]), Err(want));
        assert_eq!(hod_obj::elf::to_vec(&obj).unwrap(), before);
    }
    let mut obj = base().0;
    assert_eq!(write_debug_info(&mut obj, line.line, &[]), Err(Error::NoUnit));
    assert_eq!(hod_obj::elf::to_vec(&obj).unwrap(), before);
}

/// §6.5: what the core does not write yet is an error, not a wrong DIE.
#[test]
fn unsupported_shapes_are_errors() {
    let unit = "!llvm.dbg.cu = !{!0}\n\
        !0 = distinct !DICompileUnit(language: DW_LANG_C_plus_plus_14, file: !1, producer: \"p\", isOptimized: false, runtimeVersion: 0, emissionKind: FullDebug, globals: !2, nameTableKind: None)\n\
        !1 = !DIFile(filename: \"u.cpp\", directory: \"/hod\")\n\
        !2 = !{!3}\n\
        !3 = !DIGlobalVariableExpression(var: !4, expr: !DIExpression())\n";
    let variants = [
        (
            "!4 = distinct !DIGlobalVariable(name: \"c\", scope: !0, file: !1, line: 1, type: !5, isLocal: false, isDefinition: true)\n\
             !5 = !DIDerivedType(tag: DW_TAG_const_type, baseType: !6)\n\
             !6 = !DIBasicType(name: \"int\", size: 32, encoding: DW_ATE_signed)\n",
            "a type",
        ),
        (
            "!4 = distinct !DIGlobalVariable(name: \"s\", scope: !5, file: !1, line: 1, type: !6, isLocal: false, isDefinition: true)\n\
             !5 = distinct !DICompositeType(tag: DW_TAG_structure_type, name: \"S\", file: !1, line: 1, size: 8, identifier: \"_ZTS1S\")\n\
             !6 = !DIBasicType(name: \"int\", size: 32, encoding: DW_ATE_signed)\n",
            "a scope",
        ),
    ];
    for (body, what) in variants {
        let loaded = load(&format!("{unit}{body}"));
        match UnitPlan::new(&loaded.debug, loaded.units[0]) {
            Err(Error::Unsupported { .. }) => {}
            other => panic!("{what}: {other:?}"),
        }
    }
    let mut d = DebugInfo::default();
    let file = d.reserve().unwrap();
    let mut f = DiNode::llvm_default(DiKind::File);
    f.set(b"filename", DiFieldValue::Str(b"x.cpp")).unwrap();
    d.define(file, &f).unwrap();
    assert_eq!(UnitPlan::new(&d, file).err(), Some(Error::NotACompileUnit { node: file.raw() }));
}

// ----------------------------------------------------------- builder ----

/// The tables built with `hod_ir`'s builder directly, without text: two
/// namespaces, an `int`, a pointer to it and two variables.
#[test]
#[allow(clippy::too_many_lines)]
fn tables_from_the_builder() {
    use DiFieldValue::{Bool, Node, Str, UInt};
    let mut d = DebugInfo::default();
    let [file, unit, ns, inner, int, ptr, v1, e1, v2, e2] =
        std::array::from_fn(|_| d.reserve().unwrap());
    let define = |d: &mut DebugInfo, id, kind, fields: &[(&str, DiFieldValue<'_>)], distinct| {
        let mut n = DiNode::llvm_default(kind);
        for (f, v) in fields {
            n.set(f.as_bytes(), *v).unwrap();
        }
        if distinct { d.define_distinct(id, &n).unwrap() } else { d.define(id, &n).unwrap() }
    };
    define(
        &mut d,
        file,
        DiKind::File,
        &[("filename", Str(b"b.cpp")), ("directory", Str(b"/b"))],
        false,
    );
    define(&mut d, ns, DiKind::Namespace, &[("name", Str(b"outer")), ("scope", Node(None))], false);
    define(
        &mut d,
        inner,
        DiKind::Namespace,
        &[("name", Str(b"inner")), ("scope", Node(Some(ns)))],
        false,
    );
    define(
        &mut d,
        int,
        DiKind::BasicType,
        &[
            ("name", Str(b"int")),
            ("size", UInt(32)),
            ("encoding", UInt(u64::from(dw_ate(b"DW_ATE_signed").unwrap()))),
        ],
        false,
    );
    define(
        &mut d,
        ptr,
        DiKind::DerivedType,
        &[
            ("tag", UInt(u64::from(dw_tag(b"DW_TAG_pointer_type").unwrap()))),
            ("baseType", Node(Some(int))),
            ("size", UInt(64)),
        ],
        false,
    );
    for (v, name, scope, ty) in [(v1, &b"a"[..], ns, int), (v2, &b"p"[..], inner, ptr)] {
        define(
            &mut d,
            v,
            DiKind::GlobalVariable,
            &[
                ("name", Str(name)),
                ("scope", Node(Some(scope))),
                ("file", Node(Some(file))),
                ("line", UInt(3)),
                ("type", Node(Some(ty))),
                ("isLocal", Bool(false)),
                ("isDefinition", Bool(true)),
            ],
            true,
        );
    }
    let empty = d.expr_from_elements(&[]).unwrap();
    for (e, v) in [(e1, v1), (e2, v2)] {
        define(
            &mut d,
            e,
            DiKind::GlobalVariableExpression,
            &[("var", Node(Some(v))), ("expr", DiFieldValue::Expr(empty))],
            false,
        );
    }
    let globals = [e1, e2];
    define(
        &mut d,
        unit,
        DiKind::CompileUnit,
        &[
            ("language", UInt(u64::from(dw_lang(b"DW_LANG_C_plus_plus_14").unwrap()))),
            ("file", Node(Some(file))),
            ("producer", Str(b"builder")),
            ("emissionKind", UInt(1)),
            ("globals", DiFieldValue::List(&globals)),
        ],
        true,
    );
    d.add_compile_unit(unit).unwrap();
    d.verify().unwrap();
    let plan = UnitPlan::new(&d, unit).unwrap();
    // The unit, two namespaces, two variables, two types.
    assert_eq!(plan.die_count(), 7);
    assert!(plan.die_of(int).is_some() && plan.die_of(file).is_none());
    let loaded = Loaded {
        debug: d.clone(),
        units: vec![unit],
        globals: vec![("p".into(), e2), ("a".into(), e1)],
    };
    let b = build(Arch::X86_64, &loaded, &ONE);
    let bytes = write(&b, &plan, &b.globals, &b.spans);
    let tools = dwarfdumps();
    if tools.is_empty() {
        assert!(!hod_testutil::tools_required(), "llvm-dwarfdump is required");
        eprintln!("skipped: no llvm-dwarfdump");
        return;
    }
    let scratch = Scratch::new("dies-builder");
    for (major, tool) in &tools {
        let name = format!("builder-{major}.o");
        verify_all(tool, &scratch, &name, &bytes);
        let info = dump(tool, &scratch, &name, &bytes, &["--debug-info"]);
        for n in ["\"outer\"", "\"inner\"", "\"builder\"", "\"b.cpp\"", "\"int *\""] {
            assert!(info.contains(n), "{major}: {n}\n{info}");
        }
    }
}
