//! Shared by the DIE tests (`spec/dwarf.md` §8.6): `llvm-dwarfdump
//! --verify` of whole objects, a reader of `--debug-info` output into DIE
//! trees with the normalization the design permits, and the splice of
//! `hod_dwarf`'s DIE sections into `llc`'s object.

use std::path::Path;

use hod_dwarf::{
    DieSections, GlobalAddr, LineSections, UnitInput, UnitPlan, write_debug_info, write_debug_line,
};
use hod_obj::{Arch, Object, SectionId, StrId, Symbol, SymbolId, SymbolKind, SymbolPlace};
use hod_testutil::{Scratch, run};

use super::meta::Loaded;
use super::{Elf, Rela, rela_bytes};

/// `llvm-dwarfdump --verify` of every section must be clean.
pub fn verify_all(dwarfdump: &Path, scratch: &Scratch, name: &str, object: &[u8]) {
    let path = scratch.write(name, object);
    let out = run(dwarfdump, ["--verify".into(), path.into_os_string()]);
    assert!(
        out.success() && out.stdout_text().contains("No errors."),
        "llvm-dwarfdump --verify {name}:\n{}{}",
        out.stdout_text(),
        out.stderr_text()
    );
}

/// `llvm-dwarfdump` with `args` on an object.
pub fn dump(
    dwarfdump: &Path,
    scratch: &Scratch,
    name: &str,
    object: &[u8],
    args: &[&str],
) -> String {
    let path = scratch.write(name, object);
    let mut all: Vec<std::ffi::OsString> = args.iter().map(Into::into).collect();
    all.push(path.into());
    let out = run(dwarfdump, &all);
    assert!(out.success(), "llvm-dwarfdump {name}: {}", out.stderr_text());
    out.stdout_text()
}

// ------------------------------------------------------------ trees ----

/// A DIE as `llvm-dwarfdump --debug-info` prints it, normalized.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Node {
    pub tag: String,
    pub attrs: Vec<String>,
    pub children: Vec<Node>,
}

/// What a relocated address names: the section of the symbol and the
/// offset in it.
pub fn target_name(elf: &Elf, sym: u32, addend: i64) -> String {
    let s = &elf.symbols[sym as usize];
    let section = elf.sections.get(usize::from(s.shndx)).map_or("?", |h| h.name.as_str());
    format!("{section}+{:#x}", s.value as i64 + addend)
}

/// The target of each `.debug_addr` entry of a one-unit object, by
/// index.
pub fn addr_targets(elf: &Elf) -> Vec<String> {
    let Some(addr) = elf.section(".debug_addr") else { return Vec::new() };
    let rela = elf.rela_of(addr).expect(".rela.debug_addr");
    let mut out = vec![String::new(); (elf.sections[addr].size as usize - 8) / 8];
    for r in elf.relas(rela) {
        out[(r.offset as usize - 8) / 8] = target_name(elf, r.sym, r.addend);
    }
    out
}

/// An attribute line with what the design lets differ removed: a
/// reference's offset (its target's name stays), an address index (the
/// target it names replaces it), and the radix of a constant.
fn normalize(line: &str, addrs: &[String]) -> String {
    let line = line.trim();
    let (name, value) = line.split_once(char::is_whitespace).unwrap_or((line, ""));
    let value = value.trim();
    let mut v = value.to_string();
    if let Some(rest) = value.strip_prefix("(0x")
        && let Some(q) = rest.find(" \"")
    {
        v = format!("({}", &rest[q + 1..]);
    }
    if let Some(i) = value.strip_prefix("(DW_OP_addrx 0x").and_then(|r| r.strip_suffix(')')) {
        let i = usize::from_str_radix(i, 16).unwrap();
        v = format!("(DW_OP_addrx {})", addrs.get(i).map_or("?", String::as_str));
    }
    if let Some(num) = value.strip_prefix('(').and_then(|r| r.strip_suffix(')')) {
        let parsed = num
            .strip_prefix("0x")
            .map_or_else(|| num.parse::<u64>().ok(), |h| u64::from_str_radix(h, 16).ok());
        if let Some(n) = parsed {
            v = format!("({n})");
        }
    }
    format!("{name} {v}")
}

/// The DIE trees of the units of `--debug-info` output.
pub fn parse_tree(text: &str, addrs: &[String]) -> Vec<Node> {
    let mut units: Vec<Node> = Vec::new();
    // The path of open DIEs, by depth.
    let mut open: Vec<Node> = Vec::new();
    let close_to = |open: &mut Vec<Node>, units: &mut Vec<Node>, depth: usize| {
        while open.len() > depth {
            let done = open.pop().unwrap();
            match open.last_mut() {
                Some(parent) => parent.children.push(done),
                None => units.push(done),
            }
        }
    };
    for line in text.lines() {
        let t = line.trim_start();
        if line.starts_with("0x") && line.contains("Compile Unit:") {
            close_to(&mut open, &mut units, 0);
            continue;
        }
        if line.starts_with("0x") {
            let (_, rest) = line.split_once(':').unwrap();
            let spaces = rest.len() - rest.trim_start().len();
            let depth = (spaces - 1) / 2;
            let what = rest.trim();
            if what == "NULL" {
                close_to(&mut open, &mut units, depth);
                continue;
            }
            close_to(&mut open, &mut units, depth);
            open.push(Node { tag: what.to_string(), attrs: Vec::new(), children: Vec::new() });
        } else if t.starts_with("DW_AT_") {
            open.last_mut().unwrap().attrs.push(normalize(t, addrs));
        } else if !t.is_empty()
            && !open.is_empty()
            && !t.ends_with("file format elf64-littleaarch64")
        {
            // A continuation of the last attribute (a range list).
            if let Some(a) = open.last_mut().unwrap().attrs.last_mut() {
                a.push(' ');
                a.push_str(t);
            }
        }
    }
    close_to(&mut open, &mut units, 0);
    units
}

/// A tree with its attributes and children sorted: the order of siblings
/// and of attributes is what the design lets differ.
pub fn sorted(n: &Node) -> Node {
    let mut attrs = n.attrs.clone();
    attrs.sort();
    let mut children: Vec<Node> = n.children.iter().map(sorted).collect();
    children.sort();
    Node { tag: n.tag.clone(), attrs, children }
}

pub fn count(n: &Node) -> usize {
    1 + n.children.iter().map(count).sum::<usize>()
}

pub fn render(n: &Node, depth: usize, out: &mut String) {
    out.push_str(&format!("{}{}\n", "  ".repeat(depth), n.tag));
    for a in &n.attrs {
        out.push_str(&format!("{}  {a}\n", "  ".repeat(depth)));
    }
    for c in &n.children {
        render(c, depth + 1, out);
    }
}

// ----------------------------------------------------------- splice ----

/// An object whose symbols `0..n-1` stand for `llc`'s `1..n` (index 0 of
/// an ELF symbol table is the null symbol).
pub fn stand_in(elf: &Elf, arch: Arch) -> Object {
    let mut obj = Object::new(arch);
    for _ in 1..elf.symbols.len() {
        obj.add_symbol(Symbol::undefined(StrId::EMPTY));
    }
    obj
}

/// `llc`'s number for symbol `s` of a [`stand_in`] object: a stand-in's,
/// or the section symbol of the `llc` section with the name of the
/// section `s` is the symbol of.
pub fn llc_symbol(elf: &Elf, obj: &Object, s: SymbolId) -> u32 {
    let n = elf.symbols.len() as u32;
    if s.index() + 1 < n {
        return s.index() + 1;
    }
    let sym = obj.symbol(s);
    assert_eq!(sym.kind, SymbolKind::Section);
    let SymbolPlace::Section(sec) = sym.place else { panic!("a section symbol") };
    let name = String::from_utf8_lossy(obj.section_name(sec)).into_owned();
    let index = elf.section(&name).unwrap_or_else(|| panic!("llc has no {name}"));
    elf.section_symbol(index)
}

/// The relocations of `sec` in `llc`'s numbering.
pub fn relas_of(elf: &Elf, obj: &Object, sec: SectionId) -> Vec<Rela> {
    obj.relocations(sec)
        .map(|r| Rela {
            offset: r.offset,
            sym: llc_symbol(elf, obj, r.symbol),
            ty: r.kind.elf_type(),
            addend: r.addend,
        })
        .collect()
}

/// `llc`'s object with the contents (and relocations) of `secs` replaced
/// by those of the same-named sections of `obj`. Every debug section of
/// `llc`'s must be among them, and the reverse.
pub fn splice(elf: &Elf, obj: &Object, secs: &[SectionId]) -> Vec<u8> {
    let names: Vec<String> =
        secs.iter().map(|&s| String::from_utf8_lossy(obj.section_name(s)).into_owned()).collect();
    for s in &elf.sections {
        if s.name.starts_with(".debug_") {
            assert!(names.contains(&s.name), "llc writes {} and hod does not", s.name);
        }
    }
    let mut bytes: Vec<(usize, Vec<u8>)> = Vec::new();
    for (&sec, name) in secs.iter().zip(&names) {
        let index =
            elf.section(name).unwrap_or_else(|| panic!("hod writes {name} and llc does not"));
        bytes.push((index, obj.data(sec).to_vec()));
        let relas = relas_of(elf, obj, sec);
        match elf.rela_of(index) {
            Some(rela) => bytes.push((rela, rela_bytes(&relas))),
            None => assert!(relas.is_empty(), "{name}: hod relocates and llc does not"),
        }
    }
    let parts: Vec<(usize, &[u8])> = bytes.iter().map(|(i, b)| (*i, b.as_slice())).collect();
    elf.replace(&parts)
}

/// hod's debug sections for `llc`'s object of a globals-only module: the
/// line table from the plan's files, the DIEs from `loaded`'s tables,
/// each global relocated against `llc`'s symbol of the same name (with
/// `adjust` applied).
pub fn hod_sections(
    elf: &Elf,
    arch: Arch,
    loaded: &Loaded,
    adjust: fn(&mut GlobalAddr),
) -> (Object, LineSections, DieSections) {
    let plan = UnitPlan::new(&loaded.debug, loaded.units[0]).expect("plan");
    let mut obj = stand_in(elf, arch);
    let line = write_debug_line(&mut obj, &plan.files().header(), &[]).expect("line table");
    let globals: Vec<GlobalAddr> = loaded
        .globals
        .iter()
        .map(|(name, expr)| {
            let at = elf
                .symbols
                .iter()
                .position(|s| &s.name == name)
                .unwrap_or_else(|| panic!("llc has no symbol {name}"));
            let mut g =
                GlobalAddr { expr: *expr, symbol: SymbolId::from_index(at as u32 - 1), addend: 0 };
            adjust(&mut g);
            g
        })
        .collect();
    let input = UnitInput { plan: &plan, line_offset: 0, globals: &globals, code: &[] };
    let dies = write_debug_info(&mut obj, line.line, &[input]).expect("DIEs");
    (obj, line, dies)
}

pub fn die_section_list(line: LineSections, d: &DieSections) -> Vec<SectionId> {
    let mut v = vec![line.line, line.line_str, d.abbrev, d.info, d.str_offsets, d.str];
    v.extend(d.addr);
    v.extend(d.rnglists);
    v
}

// -------------------------------------------------------- generated ----

/// A generated globals-only `-g` module: `n` globals of base and pointer
/// types, at file scope and in namespaces up to three deep (inner names
/// repeat under different parents), some internal, some over-aligned.
pub fn globals_module(n: usize) -> String {
    use std::collections::BTreeMap;
    use std::fmt::Write;
    // (IR type, align, DIBasicType name, size, encoding)
    const BASE: [(&str, u32, &str, u32, &str, &str); 6] = [
        ("i32", 4, "int", 32, "DW_ATE_signed", "0"),
        ("i8", 1, "char", 8, "DW_ATE_signed_char", "0"),
        ("i64", 8, "long", 64, "DW_ATE_signed", "0"),
        ("double", 8, "double", 64, "DW_ATE_float", "0.000000e+00"),
        ("i8", 1, "bool", 8, "DW_ATE_boolean", "0"),
        ("i32", 4, "unsigned int", 32, "DW_ATE_unsigned", "0"),
    ];
    let mut meta = String::new();
    let mut globals = String::new();
    // !0 unit, !1 file, !2 the globals, !3 !4 flags, types from !10.
    for (k, b) in BASE.iter().enumerate() {
        writeln!(
            meta,
            "!{} = !DIBasicType(name: \"{}\", size: {}, encoding: {})",
            10 + k,
            b.2,
            b.3,
            b.4
        )
        .unwrap();
        writeln!(
            meta,
            "!{} = !DIDerivedType(tag: DW_TAG_pointer_type, baseType: !{}, size: 64)",
            16 + k,
            10 + k
        )
        .unwrap();
    }
    writeln!(meta, "!22 = !DIDerivedType(tag: DW_TAG_pointer_type, baseType: !16, size: 64)")
        .unwrap();
    writeln!(meta, "!23 = !DIDerivedType(tag: DW_TAG_pointer_type, baseType: null, size: 64)")
        .unwrap();
    let mut next = 30;
    let mut namespaces: BTreeMap<Vec<usize>, u32> = BTreeMap::new();
    let mut list = Vec::new();
    for i in 0..n {
        let depth = i % 4;
        let path: Vec<usize> = (0..depth).map(|d| (i / (1 + d * 3)) % (8 >> d)).collect();
        let mut scope = 0;
        let mut mangled = String::new();
        for d in 0..depth {
            let key = path[..=d].to_vec();
            let name = format!("{}{}", ["top", "mid", "low"][d], path[d]);
            mangled.push_str(&format!("{}{name}", name.len()));
            scope = *namespaces.entry(key).or_insert_with(|| {
                let id = next;
                next += 1;
                let parent = if scope == 0 { "null".to_string() } else { format!("!{scope}") };
                writeln!(meta, "!{id} = !DINamespace(name: \"{name}\", scope: {parent})").unwrap();
                id
            });
        }
        let name = if depth > 0 && i % 10 == 1 { "shared".to_string() } else { format!("v{i}") };
        let symbol =
            if depth == 0 { name.clone() } else { format!("_ZN{mangled}{}{name}E", name.len()) };
        let kind = i % 14;
        let ty = match kind {
            0..6 => 10 + kind,
            6..12 => 16 + kind - 6,
            12 => 22,
            _ => 23,
        };
        let (ir, mut align, init) = match kind {
            0..6 => (BASE[kind].0, BASE[kind].1, BASE[kind].5),
            _ => ("ptr", 8, "null"),
        };
        let local = i % 7 == 3;
        let mut extra = String::new();
        if i % 11 == 5 {
            align = 32;
            extra = ", align: 256".to_string();
        }
        let (expr, var) = (next, next + 1);
        next += 2;
        let linkage =
            if depth == 0 { String::new() } else { format!(", linkageName: \"{symbol}\"") };
        let scope = if depth == 0 { "!0".to_string() } else { format!("!{scope}") };
        writeln!(
            globals,
            "@{symbol} = {}global {ir} {init}, align {align}, !dbg !{expr}",
            if local { "internal " } else { "dso_local " }
        )
        .unwrap();
        writeln!(meta, "!{expr} = !DIGlobalVariableExpression(var: !{var}, expr: !DIExpression())")
            .unwrap();
        writeln!(
            meta,
            "!{var} = distinct !DIGlobalVariable(name: \"{name}\"{linkage}, scope: {scope}, file: !1, line: {}, type: !{ty}, isLocal: {local}, isDefinition: true{extra})",
            10 + i
        )
        .unwrap();
        list.push(format!("!{expr}"));
    }
    format!(
        "; generated\nsource_filename = \"gen.cpp\"\n\n{globals}\n!llvm.dbg.cu = !{{!0}}\n!llvm.module.flags = !{{!3, !4}}\n\n\
         !0 = distinct !DICompileUnit(language: DW_LANG_C_plus_plus_14, file: !1, producer: \"hod generated\", isOptimized: false, runtimeVersion: 0, emissionKind: FullDebug, globals: !2, splitDebugInlining: false, nameTableKind: None)\n\
         !1 = !DIFile(filename: \"gen.cpp\", directory: \"/hod/gen\")\n\
         !2 = !{{{}}}\n\
         !3 = !{{i32 7, !\"Dwarf Version\", i32 5}}\n\
         !4 = !{{i32 2, !\"Debug Info Version\", i32 3}}\n{meta}",
        list.join(", ")
    )
}
