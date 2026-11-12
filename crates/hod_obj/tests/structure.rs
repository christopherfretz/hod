//! `spec/object.md` "ELF writer", "Errors" and "Determinism": the writer's
//! output, parsed back by the independent reader in `common`.

mod common;

use common::{
    Elf, Rela, SHF_ALLOC, SHF_EXECINSTR, SHF_GROUP, SHF_INFO_LINK, SHF_MERGE, SHF_STRINGS, SHF_TLS,
    SHF_WRITE, SHN_ABS, SHN_COMMON, SHN_UNDEF, SHT_FINI_ARRAY, SHT_GROUP, SHT_INIT_ARRAY,
    SHT_NOBITS, SHT_NOTE, SHT_PROGBITS, SHT_RELA, SHT_STRTAB, SHT_SYMTAB, STB_GLOBAL, STB_LOCAL,
    STB_WEAK, function, undefined,
};
use hod_obj::{
    A64Reloc, Arch, Binding, Error, GroupKind, Limit, Object, SectionId, SectionKind, StackNote,
    StrId, Symbol, SymbolId, SymbolKind, SymbolPlace, SymbolProblem, Visibility, WriteError,
    X64Reloc, elf,
};

fn parse(obj: &Object) -> Elf {
    Elf::parse(&elf::to_vec(obj).unwrap())
}

fn named(obj: &mut Object, name: &str, symbol: Symbol) -> SymbolId {
    let name = obj.add_string(name.as_bytes()).unwrap();
    obj.add_symbol(Symbol { name, ..symbol })
}

fn local(kind: SymbolKind, section: SectionId, value: u64) -> Symbol {
    Symbol::defined(StrId::EMPTY, kind, section, value, 0).with_binding(Binding::Local)
}

/// "File header".
#[test]
fn file_header_per_architecture() {
    let elf = parse(&common::return_42());
    assert_eq!((elf.machine, elf.flags, elf.os_abi), (183, 0, 0));

    let mut obj = Object::new(Arch::X86_64);
    obj.set_header_flags(0x1234);
    let elf = parse(&obj);
    assert_eq!((elf.machine, elf.flags, elf.os_abi), (62, 0x1234, 0));
}

/// "Section header order": an empty object still has the null section, the
/// stack note and the three tables, in that order.
#[test]
fn empty_object() {
    let elf = parse(&Object::new(Arch::AArch64));
    let names: Vec<_> = elf.sections.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, ["", ".note.GNU-stack", ".symtab", ".strtab", ".shstrtab"]);
    assert_eq!(elf.symbols.len(), 1);
    assert_eq!(elf.sections[elf.symtab].info, 1);
}

/// "Section kinds": type, flags, entry size, alignment and size of each.
#[test]
fn section_kinds() {
    let mut obj = Object::new(Arch::AArch64);
    let kinds = [
        (".text", SectionKind::Text, SHT_PROGBITS, SHF_ALLOC | SHF_EXECINSTR, 0),
        (".data", SectionKind::Data, SHT_PROGBITS, SHF_ALLOC | SHF_WRITE, 0),
        (".rodata", SectionKind::ReadOnlyData, SHT_PROGBITS, SHF_ALLOC, 0),
        (".bss", SectionKind::Bss, SHT_NOBITS, SHF_ALLOC | SHF_WRITE, 0),
        (".tdata", SectionKind::Tls, SHT_PROGBITS, SHF_ALLOC | SHF_WRITE | SHF_TLS, 0),
        (".tbss", SectionKind::TlsBss, SHT_NOBITS, SHF_ALLOC | SHF_WRITE | SHF_TLS, 0),
        (".init_array", SectionKind::InitArray, SHT_INIT_ARRAY, SHF_ALLOC | SHF_WRITE, 0),
        (".fini_array", SectionKind::FiniArray, SHT_FINI_ARRAY, SHF_ALLOC | SHF_WRITE, 0),
        (".note.x", SectionKind::Note, SHT_NOTE, SHF_ALLOC, 0),
        (".debug_info", SectionKind::Debug, SHT_PROGBITS, 0, 0),
        (
            ".rodata.str2.2",
            SectionKind::MergeableStrings { char_size: 2 },
            SHT_PROGBITS,
            SHF_ALLOC | SHF_MERGE | SHF_STRINGS,
            2,
        ),
        (
            ".rodata.cst8",
            SectionKind::MergeableConstants { entry_size: 8 },
            SHT_PROGBITS,
            SHF_ALLOC | SHF_MERGE,
            8,
        ),
        (
            ".debug_str",
            SectionKind::Elf {
                sh_type: elf::SHT_PROGBITS,
                sh_flags: elf::SHF_MERGE | elf::SHF_STRINGS,
                sh_entsize: 1,
            },
            SHT_PROGBITS,
            SHF_MERGE | SHF_STRINGS,
            1,
        ),
        (
            ".custom",
            SectionKind::Elf { sh_type: elf::SHT_NOBITS, sh_flags: elf::SHF_ALLOC, sh_entsize: 0 },
            SHT_NOBITS,
            SHF_ALLOC,
            0,
        ),
    ];
    for (index, &(name, kind, ..)) in kinds.iter().enumerate() {
        let align = 1 << (index % 5);
        let section = obj.add_section(name.as_bytes(), kind, align).unwrap();
        assert_eq!(obj.section_kind(section), kind);
        assert_eq!(obj.section_name(section), name.as_bytes());
        // Sixteen bytes: a multiple of every entry size above.
        if kind.is_nobits() {
            obj.append_zeros(section, 16);
        } else {
            obj.append(section, &[index as u8 + 1; 16]);
        }
    }
    let elf = parse(&obj);
    for (index, &(name, _, kind, flags, entsize)) in kinds.iter().enumerate() {
        let at = elf.only(name);
        assert_eq!(at, index + 1, "sections keep their order");
        let sec = &elf.sections[at];
        assert_eq!((sec.kind, sec.flags, sec.entsize), (kind, flags, entsize), "{name}");
        assert_eq!((sec.size, sec.align), (16, 1 << (index % 5)), "{name}");
        if kind != SHT_NOBITS {
            assert_eq!(elf.data(at), [index as u8 + 1; 16], "{name}");
        }
    }
    // Sections that occupy no file space take none: the next section's
    // contents start where theirs would.
    let bss = &elf.sections[elf.only(".bss")];
    let tdata = &elf.sections[elf.only(".tdata")];
    assert!(tdata.offset - bss.offset < 16);
}

/// "Section names": equal names are stored once, and a relocation section's
/// name is its section's name with `.rela` in front, sharing its bytes.
#[test]
fn section_names_are_shared() {
    let mut obj = Object::new(Arch::AArch64);
    let callee = undefined(&mut obj, "callee");
    let first = obj.add_section(b".text", SectionKind::Text, 4).unwrap();
    let second = obj.add_section(b".text", SectionKind::Text, 4).unwrap();
    let third = obj.add_section(b".text", SectionKind::Text, 4).unwrap();
    assert_ne!(first, second);
    obj.append(first, &[1; 4]);
    obj.append(second, &[2; 4]);
    obj.append(third, &[3; 4]);
    obj.add_relocation(second, 0, A64Reloc::Call26, callee, 0);
    let elf = parse(&obj);
    let texts = elf.named(".text");
    assert_eq!(texts, [1, 2, 3]);
    assert_eq!(elf.data(2), [2; 4]);
    let relas = elf.named(".rela.text");
    assert_eq!(relas.len(), 1);
    let rela = &elf.sections[relas[0]];
    assert_eq!(rela.info, 2);
    for &text in &texts {
        assert_eq!(elf.sections[text].sh_name, rela.sh_name + 5);
    }
    let table = elf.data(elf.shstrndx as usize);
    let count = table.windows(5).filter(|w| w == b".text").count();
    assert_eq!(count, 1, "the name is stored once");
}

/// "Symbol table": file symbols, other locals, then non-locals, each in id
/// order; `sh_info` is the first non-local; relocations follow the symbols
/// to their new indices.
#[test]
fn symbols_are_reordered_and_relocations_follow() {
    let mut obj = Object::new(Arch::AArch64);
    let text = obj.add_section(b".text", SectionKind::Text, 4).unwrap();
    obj.append_zeros(text, 64);
    let global =
        named(&mut obj, "global", Symbol::defined(StrId::EMPTY, SymbolKind::Func, text, 0, 8));
    let local_a = named(&mut obj, "local_a", local(SymbolKind::Func, text, 8));
    let extern_used = undefined(&mut obj, "extern_used");
    let file = obj.add_file_symbol(b"unit.cc").unwrap();
    let weak = named(
        &mut obj,
        "weak",
        Symbol::defined(StrId::EMPTY, SymbolKind::Object, text, 16, 4).with_binding(Binding::Weak),
    );
    let local_b = named(&mut obj, "local_b", local(SymbolKind::NoType, text, 24));
    let all = [global, local_a, extern_used, file, weak, local_b];
    assert_eq!(all.map(SymbolId::index), [0, 1, 2, 3, 4, 5]);
    for (slot, &symbol) in all.iter().enumerate() {
        if symbol != file {
            obj.add_relocation(text, 4 * slot as u64, A64Reloc::Abs32, symbol, slot as i64 - 2);
        }
    }

    let elf = parse(&obj);
    let names: Vec<_> = elf.symbols.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, ["", "unit.cc", "local_a", "local_b", "global", "extern_used", "weak"]);
    assert_eq!(elf.sections[elf.symtab].info, 4);
    let binds: Vec<_> = elf.symbols.iter().map(|s| s.bind).collect();
    assert_eq!(binds, [0, STB_LOCAL, STB_LOCAL, STB_LOCAL, STB_GLOBAL, STB_GLOBAL, STB_WEAK]);

    let rela = |offset, name: &str, addend| Rela {
        offset,
        sym: elf.symbol(name) as u32,
        kind: 258,
        addend,
    };
    assert_eq!(
        elf.relas_of(elf.only(".text")),
        [
            rela(0, "global", -2),
            rela(4, "local_a", -1),
            rela(8, "extern_used", 0),
            rela(16, "weak", 2),
            rela(20, "local_b", 3),
        ]
    );
    let rela_section = &elf.sections[elf.only(".rela.text")];
    assert_eq!(rela_section.kind, SHT_RELA);
    assert_eq!(rela_section.flags, SHF_INFO_LINK);
    assert_eq!(rela_section.link as usize, elf.symtab);
    assert_eq!(rela_section.info as usize, elf.only(".text"));
}

/// "Symbol table": every field of a symbol entry.
#[test]
fn symbol_fields() {
    let mut obj = Object::new(Arch::AArch64);
    let text = obj.add_section(b".text", SectionKind::Text, 4).unwrap();
    let tdata = obj.add_section(b".tdata", SectionKind::Tls, 8).unwrap();
    obj.append_zeros(text, 32);
    obj.append_zeros(tdata, 8);
    let function = Symbol::defined(StrId::EMPTY, SymbolKind::Func, text, 4, 12);
    named(&mut obj, "hidden", function.with_visibility(Visibility::Hidden));
    named(&mut obj, "protected", function.with_visibility(Visibility::Protected));
    named(&mut obj, "vector", Symbol { other: elf::STO_AARCH64_VARIANT_PCS, ..function });
    named(&mut obj, "object", Symbol { kind: SymbolKind::Object, ..function });
    named(&mut obj, "notype", Symbol { kind: SymbolKind::NoType, ..function });
    named(&mut obj, "tls", Symbol::defined(StrId::EMPTY, SymbolKind::Tls, tdata, 0, 8));
    named(
        &mut obj,
        "absolute",
        Symbol { place: SymbolPlace::Absolute, value: 0xdead_beef, ..function },
    );
    named(
        &mut obj,
        "common",
        Symbol {
            place: SymbolPlace::Common,
            kind: SymbolKind::Object,
            value: 16,
            size: 40,
            ..function
        },
    );
    let elf = parse(&obj);
    assert_eq!(elf.os_abi, 0);
    let get = |name: &str| &elf.symbols[elf.symbol(name)];
    let text = elf.only(".text") as u32;
    let hidden = get("hidden");
    assert_eq!((hidden.kind, hidden.other, hidden.shndx), (2, 2, text));
    assert_eq!((hidden.value, hidden.size), (4, 12));
    assert_eq!(get("protected").other, 3);
    assert_eq!(get("vector").other, 0x80);
    assert_eq!(get("object").kind, 1);
    assert_eq!(get("notype").kind, 0);
    assert_eq!((get("tls").kind, get("tls").shndx), (6, elf.only(".tdata") as u32));
    assert_eq!((get("absolute").shndx, get("absolute").value), (SHN_ABS, 0xdead_beef));
    let common = get("common");
    assert_eq!((common.shndx, common.value, common.size), (SHN_COMMON, 16, 40));
}

/// "File header": an indirect function marks the object as using the GNU
/// ABI, as does a retained section.
#[test]
fn gnu_extensions_set_the_os_abi() {
    let mut obj = Object::new(Arch::AArch64);
    let text = obj.add_section(b".text", SectionKind::Text, 4).unwrap();
    obj.append_zeros(text, 4);
    named(&mut obj, "resolver", Symbol::defined(StrId::EMPTY, SymbolKind::GnuIfunc, text, 0, 4));
    let elf = parse(&obj);
    assert_eq!(elf.os_abi, 3);
    assert_eq!(elf.symbols[elf.symbol("resolver")].kind, 10);

    let mut obj = Object::new(Arch::AArch64);
    let retained = SectionKind::Elf {
        sh_type: elf::SHT_PROGBITS,
        sh_flags: elf::SHF_ALLOC | elf::SHF_GNU_RETAIN,
        sh_entsize: 0,
    };
    obj.add_section(b".keep", retained, 1).unwrap();
    assert_eq!(parse(&obj).os_abi, 3);
}

/// "Symbol table": an undefined symbol nothing refers to is left out,
/// unless it is retained.
#[test]
fn unreferenced_undefined_symbols_are_omitted() {
    let mut obj = Object::new(Arch::AArch64);
    let text = obj.add_section(b".text", SectionKind::Text, 4).unwrap();
    obj.append_zeros(text, 8);
    let unused = undefined(&mut obj, "unused");
    let used = undefined(&mut obj, "used");
    let retained = undefined(&mut obj, "retained");
    let defined_later = undefined(&mut obj, "defined_later");
    assert!(!obj.is_referenced(used));
    obj.add_relocation(text, 0, A64Reloc::Call26, used, 0);
    obj.retain_symbol(retained);
    assert!(obj.is_referenced(used) && obj.is_referenced(retained) && !obj.is_referenced(unused));
    *obj.symbol_mut(defined_later) =
        Symbol { place: SymbolPlace::Section(text), value: 4, ..*obj.symbol(defined_later) };

    let elf = parse(&obj);
    let names: Vec<_> = elf.symbols.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, ["", "used", "retained", "defined_later"]);
    assert_eq!(elf.symbols[1].shndx, SHN_UNDEF);
    assert_eq!(elf.symbols[3].shndx, elf.only(".text") as u32);
    // Its name stays in the string table; only the symbol is dropped.
    let strtab = elf.data(elf.sections[elf.symtab].link as usize);
    assert!(strtab.windows(7).any(|w| w == b"unused\0"));
}

/// "Symbol table": section symbols are created once per section, are local
/// and unnamed, and a file symbol comes before them.
#[test]
fn section_and_file_symbols() {
    let mut obj = Object::new(Arch::AArch64);
    let text = obj.add_section(b".text", SectionKind::Text, 4).unwrap();
    let data = obj.add_section(b".data", SectionKind::Data, 8).unwrap();
    obj.append_zeros(text, 4);
    obj.append_zeros(data, 8);
    let text_symbol = obj.section_symbol(text);
    let data_symbol = obj.section_symbol(data);
    assert_eq!(obj.section_symbol(text), text_symbol);
    assert_ne!(text_symbol, data_symbol);
    assert_eq!(obj.symbol_count(), 2);
    obj.add_relocation(data, 0, A64Reloc::Abs64, text_symbol, 0);
    obj.add_file_symbol(b"a.cc").unwrap();

    let elf = parse(&obj);
    assert_eq!(elf.symbols.len(), 4);
    let file = &elf.symbols[1];
    assert_eq!((file.name.as_str(), file.kind, file.bind, file.shndx), ("a.cc", 4, 0, SHN_ABS));
    let section = &elf.symbols[2];
    assert_eq!((section.name.as_str(), section.kind, section.bind), ("", 3, STB_LOCAL));
    assert_eq!(section.shndx, elf.only(".text") as u32);
    assert_eq!(elf.symbols[3].shndx, elf.only(".data") as u32);
    assert_eq!(elf.relas_of(elf.only(".data")), [Rela { offset: 0, sym: 2, kind: 257, addend: 0 }]);
}

/// "Relocation sections": entries are in increasing offset order whatever
/// order they were added in, equal offsets in the order added, and sections
/// keep their own relocations when additions interleave.
#[test]
fn relocations_are_sorted_per_section() {
    let mut obj = Object::new(Arch::X86_64);
    let text = obj.add_section(b".text", SectionKind::Text, 16).unwrap();
    let data = obj.add_section(b".data", SectionKind::Data, 8).unwrap();
    obj.append_zeros(text, 64);
    obj.append_zeros(data, 64);
    let symbol = undefined(&mut obj, "target");
    obj.add_relocation(text, 40, X64Reloc::Plt32, symbol, -4);
    obj.add_relocation(data, 0, X64Reloc::Abs64, symbol, 0);
    obj.add_relocation(text, 8, X64Reloc::Pc32, symbol, -4);
    obj.add_relocation(data, 8, X64Reloc::Abs64, symbol, i64::MIN);
    obj.add_relocation(text, 8, X64Reloc::Other(250), symbol, i64::MAX);
    obj.add_relocation(text, 0, X64Reloc::GotPcrelX, symbol, -4);
    assert_eq!(obj.relocation_count(text), 4);
    assert_eq!(obj.relocation_count(data), 2);
    let added: Vec<_> = obj.relocations(text).map(|r| (r.offset, r.kind.elf_type())).collect();
    assert_eq!(added, [(40, 4), (8, 2), (8, 250), (0, 41)]);

    let elf = parse(&obj);
    let rela = |offset, kind, addend| Rela { offset, sym: 1, kind, addend };
    assert_eq!(
        elf.relas_of(elf.only(".text")),
        [rela(0, 41, -4), rela(8, 2, -4), rela(8, 250, i64::MAX), rela(40, 4, -4)]
    );
    assert_eq!(elf.relas_of(elf.only(".data")), [rela(0, 1, 0), rela(8, 1, i64::MIN)]);
}

/// "Section groups": the group section comes before its members, lists
/// each member followed by its relocation section, and names its signature
/// symbol; members and their relocation sections carry `SHF_GROUP`.
#[test]
fn group_contents() {
    let mut obj = Object::new(Arch::AArch64);
    let plain_text = obj.add_section(b".text", SectionKind::Text, 4).unwrap();
    function(&mut obj, plain_text, "main", &[common::BL, common::RET]);
    let answer = common::add_answer_comdat(&mut obj, Binding::Weak, 42);
    obj.add_relocation(plain_text, 0, A64Reloc::Call26, answer, 0);
    // A second group, not COMDAT, with a member without relocations, and a
    // local signature.
    let extra = obj.add_section(b".data.extra", SectionKind::Data, 8).unwrap();
    obj.append_zeros(extra, 8);
    let signature = obj.section_symbol(extra);
    let plain = obj.add_group(signature, GroupKind::Plain);
    obj.add_group_member(plain, extra).unwrap();
    assert_eq!(obj.group_count(), 2);
    assert_eq!(obj.group_signature(plain), signature);
    assert_eq!(obj.group_members(plain).collect::<Vec<_>>(), [extra]);
    assert_eq!(obj.section_group(extra), Some(plain));
    assert_eq!(obj.section_group(plain_text), None);

    let elf = parse(&obj);
    let groups = elf.named(".group");
    assert_eq!(groups, [1, 2]);
    let text = elf.only(".text.answer");
    let rodata = elf.only(".rodata.answer");
    let rela = elf.only(".rela.text.answer");
    let (flags, members) = elf.group(1);
    assert_eq!(flags, 1, "GRP_COMDAT");
    assert_eq!(members, [text as u32, rela as u32, rodata as u32]);
    assert_eq!(elf.sections[1].kind, SHT_GROUP);
    assert_eq!(elf.sections[1].info as usize, elf.symbol("answer"));
    assert_eq!(elf.sections[text].flags, SHF_ALLOC | SHF_EXECINSTR | SHF_GROUP);
    assert_eq!(elf.sections[rela].flags, SHF_INFO_LINK | SHF_GROUP);
    assert_eq!(elf.sections[elf.only(".rela.text")].flags, SHF_INFO_LINK);
    assert_eq!(elf.sections[elf.only(".text")].flags, SHF_ALLOC | SHF_EXECINSTR);

    let (flags, members) = elf.group(2);
    assert_eq!(flags, 0);
    assert_eq!(members, [elf.only(".data.extra") as u32]);
    let signature = &elf.symbols[elf.sections[2].info as usize];
    assert_eq!((signature.kind, signature.bind), (3, STB_LOCAL));
}

/// "Stack marker".
#[test]
fn stack_note() {
    let mut obj = common::return_42();
    let elf = parse(&obj);
    let note = &elf.sections[elf.only(".note.GNU-stack")];
    assert_eq!((note.kind, note.flags, note.size, note.align), (SHT_PROGBITS, 0, 0, 1));
    assert_eq!(elf.only(".note.GNU-stack"), elf.only(".text") + 1, "after the caller's sections");

    obj.set_stack_note(StackNote::Executable);
    let elf = parse(&obj);
    assert_eq!(elf.sections[elf.only(".note.GNU-stack")].flags, SHF_EXECINSTR);

    obj.set_stack_note(StackNote::Omit);
    assert!(parse(&obj).named(".note.GNU-stack").is_empty());
}

/// "Tables": types, links and alignment of the three tables.
#[test]
fn tables() {
    let elf = parse(&common::hello("hi"));
    let symtab = &elf.sections[elf.symtab];
    assert_eq!(symtab.kind, SHT_SYMTAB);
    assert_eq!(symtab.link as usize, elf.only(".strtab"));
    let strtab = &elf.sections[elf.only(".strtab")];
    assert_eq!((strtab.kind, strtab.flags, strtab.align, strtab.entsize), (SHT_STRTAB, 0, 1, 0));
    assert_eq!(elf.data(elf.only(".strtab")), b"\0hello.c\0.L.str\0puts\0main\0");
    let names: Vec<_> = elf.sections.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "",
            ".text",
            ".rodata",
            ".note.GNU-stack",
            ".rela.text",
            ".symtab",
            ".strtab",
            ".shstrtab"
        ]
    );
    assert_eq!(elf.data(elf.only(".rodata")), b"hi\0");
}

/// A sink that accepts a few bytes per call and counts the calls.
struct Trickle {
    bytes: Vec<u8>,
    step: usize,
    calls: usize,
}

impl std::io::Write for Trickle {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let n = buf.len().min(self.step);
        self.bytes.extend_from_slice(&buf[..n]);
        self.calls += 1;
        Ok(n)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// "Writing": `write` produces the bytes of `to_vec` through any sink, in
/// few calls, and reports their number; `file_size` agrees.
#[test]
fn write_matches_to_vec() {
    let mut obj = common::many_sections(300);
    // One section larger than the writer's buffer.
    let big = obj.add_section(b".rodata.big", SectionKind::ReadOnlyData, 64).unwrap();
    let pattern: Vec<u8> = (0..200_000u32).map(|i| (i % 251) as u8).collect();
    obj.append(big, &pattern);
    let expected = elf::to_vec(&obj).unwrap();
    assert_eq!(elf::file_size(&obj), Ok(expected.len() as u64));
    Elf::parse(&expected);

    let mut whole = Vec::new();
    assert_eq!(elf::write(&obj, &mut whole).unwrap(), expected.len() as u64);
    assert_eq!(whole, expected);

    let mut trickle = Trickle { bytes: Vec::new(), step: 7, calls: 0 };
    elf::write(&obj, &mut trickle).unwrap();
    assert_eq!(trickle.bytes, expected);

    // Hundreds of sections, symbols and headers reach the sink in a
    // handful of writes.
    let mut counted = Trickle { bytes: Vec::new(), step: usize::MAX, calls: 0 };
    elf::write(&obj, &mut counted).unwrap();
    assert_eq!(counted.bytes, expected);
    assert!(counted.calls <= 8, "{} writes", counted.calls);
}

struct Full {
    room: usize,
}

impl std::io::Write for Full {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if self.room < buf.len() {
            return Err(std::io::Error::other("full"));
        }
        self.room -= buf.len();
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// "Errors": a failing sink is an I/O error; an invalid object writes
/// nothing.
#[test]
fn write_errors() {
    let obj = common::hello("hi");
    assert!(matches!(elf::write(&obj, &mut Full { room: 100 }), Err(WriteError::Io(_))));

    let mut bad = common::hello("hi");
    let bss = bad.add_section(b".bss", SectionKind::Bss, 1).unwrap();
    bad.append(bss, b"x");
    let mut sink = Vec::new();
    let error = elf::write(&bad, &mut sink).unwrap_err();
    assert!(
        matches!(error, WriteError::Invalid(Error::DataInNobits { section }) if section == bss)
    );
    assert!(sink.is_empty());
    assert!(error.to_string().contains("occupies no file space"));
}

/// "Determinism": the same calls give the same bytes.
#[test]
fn output_is_deterministic() {
    let build = || {
        let mut obj = common::many_sections(2000);
        common::add_answer_comdat(&mut obj, Binding::Weak, 7);
        let data = obj.add_section(b".data", SectionKind::Data, 8).unwrap();
        obj.append_zeros(data, 32);
        let symbol = obj.section_symbol(data);
        // Out of order, so that the writer sorts.
        obj.add_relocation(data, 16, A64Reloc::Abs64, symbol, 8);
        obj.add_relocation(data, 0, A64Reloc::Abs64, symbol, 16);
        elf::to_vec(&obj).unwrap()
    };
    let first = build();
    assert_eq!(first, build());
    Elf::parse(&first);
}

/// "Errors": what `add_section` and `add_string` reject at once.
#[test]
fn immediate_errors() {
    let mut obj = Object::new(Arch::AArch64);
    assert_eq!(obj.add_string(b"a\0b"), Err(Error::NulInName));
    assert_eq!(obj.add_section(b".te\0xt", SectionKind::Text, 4), Err(Error::NulInName));
    for align in [0, 3, 12] {
        let result = obj.add_section(b".text", SectionKind::Text, align);
        assert_eq!(result, Err(Error::BadAlignment { align }));
    }
    let strings = SectionKind::MergeableStrings { char_size: 0 };
    assert_eq!(obj.add_section(b".s", strings, 1), Err(Error::BadEntrySize { section: None }));
    let constants = SectionKind::MergeableConstants { entry_size: 0 };
    assert_eq!(obj.add_section(b".c", constants, 1), Err(Error::BadEntrySize { section: None }));
    for sh_type in [0, elf::SHT_SYMTAB, elf::SHT_RELA, elf::SHT_REL, elf::SHT_GROUP, 18] {
        let kind = SectionKind::Elf { sh_type, sh_flags: 0, sh_entsize: 0 };
        assert_eq!(obj.add_section(b".x", kind, 1), Err(Error::ReservedSectionType { sh_type }));
    }
    for sh_flags in [elf::SHF_GROUP, elf::SHF_INFO_LINK, elf::SHF_LINK_ORDER] {
        let kind =
            SectionKind::Elf { sh_type: 1, sh_flags: sh_flags | elf::SHF_ALLOC, sh_entsize: 0 };
        assert_eq!(obj.add_section(b".x", kind, 1), Err(Error::ReservedSectionFlags { sh_flags }));
    }
    assert_eq!(obj.section_count(), 0, "a rejected section is not added");
    assert!(elf::to_vec(&obj).is_ok(), "rejected calls leave the object writable");
}

/// "Errors": what the writer reports.
#[test]
fn write_time_errors() {
    let base = || {
        let mut obj = Object::new(Arch::AArch64);
        let text = obj.add_section(b".text", SectionKind::Text, 4).unwrap();
        obj.append_zeros(text, 8);
        let symbol = undefined(&mut obj, "symbol");
        (obj, text, symbol)
    };
    let error = |obj: &Object| elf::to_vec(obj).unwrap_err();

    let (mut obj, text, symbol) = base();
    obj.add_relocation(text, 0, X64Reloc::Pc32, symbol, 0);
    assert_eq!(
        error(&obj),
        Error::RelocationArchMismatch { section: text, kind: X64Reloc::Pc32.into() }
    );
    assert_eq!(obj.relocation_count(text), 0);

    let (mut obj, text, symbol) = base();
    obj.add_relocation(text, 9, A64Reloc::Abs32, symbol, 0);
    obj.add_relocation(text, 4, A64Reloc::Abs32, symbol, 0);
    assert_eq!(error(&obj), Error::RelocationOutOfBounds { section: text, offset: 9 });

    let (mut obj, _, symbol) = base();
    let bss = obj.add_section(b".bss", SectionKind::Bss, 8).unwrap();
    obj.append_zeros(bss, 16);
    obj.add_relocation(bss, 0, A64Reloc::Abs64, symbol, 0);
    assert_eq!(error(&obj), Error::RelocationInNobits { section: bss });

    let (mut obj, text, _) = base();
    obj.align_to(text, 24, 0);
    assert_eq!(error(&obj), Error::BadAlignment { align: 24 });
    let (mut obj, text, _) = base();
    obj.raise_section_align(text, 0);
    assert_eq!(error(&obj), Error::BadAlignment { align: 0 });

    let (mut obj, ..) = base();
    let wide = SectionKind::MergeableStrings { char_size: 4 };
    let strings = obj.add_section(b".rodata.str4.4", wide, 4).unwrap();
    obj.append(strings, &[0; 6]);
    assert_eq!(error(&obj), Error::BadEntrySize { section: Some(strings) });

    let (mut obj, text, _) = base();
    let other = obj.add_section(b".text.b", SectionKind::Text, 4).unwrap();
    let signature = obj.section_symbol(text);
    let first = obj.add_group(signature, GroupKind::Comdat);
    let second = obj.add_group(signature, GroupKind::Comdat);
    obj.add_group_member(first, text).unwrap();
    obj.add_group_member(first, other).unwrap();
    assert_eq!(
        obj.add_group_member(second, text),
        Err(Error::SectionAlreadyInGroup { section: text })
    );
    assert_eq!(obj.group_members(first).collect::<Vec<_>>(), [text, other]);
    assert!(elf::to_vec(&obj).is_ok());
}

/// "Errors": symbols the writer rejects.
#[test]
fn bad_symbols() {
    let check = |symbol: Symbol, problem: SymbolProblem| {
        let mut obj = Object::new(Arch::AArch64);
        let text = obj.add_section(b".text", SectionKind::Text, 4).unwrap();
        obj.append_zeros(text, 8);
        obj.add_string(b"name").unwrap();
        let id = obj.add_symbol(symbol);
        obj.retain_symbol(id);
        assert_eq!(elf::to_vec(&obj), Err(Error::BadSymbol { symbol: id, problem }), "{symbol:?}");
    };
    let text = SectionId::from_index(0);
    let good = Symbol::defined(StrId::EMPTY, SymbolKind::Func, text, 0, 0);

    check(
        Symbol { place: SymbolPlace::Section(SectionId::from_index(1)), ..good },
        SymbolProblem::UnknownSection,
    );
    check(
        Symbol::undefined(StrId::EMPTY).with_binding(Binding::Local),
        SymbolProblem::UndefinedLocal,
    );
    check(Symbol { kind: SymbolKind::Section, ..good }, SymbolProblem::SectionSymbolShape);
    check(
        Symbol { kind: SymbolKind::Section, place: SymbolPlace::Absolute, ..good }
            .with_binding(Binding::Local),
        SymbolProblem::SectionSymbolShape,
    );
    check(
        Symbol { kind: SymbolKind::File, ..good }.with_binding(Binding::Local),
        SymbolProblem::FileSymbolShape,
    );
    check(
        Symbol { kind: SymbolKind::File, place: SymbolPlace::Absolute, ..good },
        SymbolProblem::FileSymbolShape,
    );
    check(Symbol { other: 0x81, ..good }, SymbolProblem::OtherOverlapsVisibility);

    // A name from another object: past the end, or inside a string.
    let mut other = Object::new(Arch::AArch64);
    other.add_string(b"a-long-name-of-another-object").unwrap();
    let foreign = other.add_string(b"x").unwrap();
    check(Symbol { name: foreign, ..good }, SymbolProblem::UnknownName);
    let mut other = Object::new(Arch::AArch64);
    other.add_string(b"ab").unwrap();
    let inside = other.add_string(b"c").unwrap();
    check(Symbol { name: inside, ..good }, SymbolProblem::UnknownName);
}

/// "Large section counts": the three escapes switch on exactly at
/// `SHN_LORESERVE` (0xff00). The reader checks each escape is used when,
/// and only when, it is needed.
#[test]
fn section_count_escapes_at_the_boundary() {
    // With no stack note and no relocations the file has the caller's
    // sections plus four: the null section and the three tables.
    for (sections, shnum_escaped, shstrndx_escaped) in
        [(0xff00 - 5, false, false), (0xff00 - 4, true, false), (0xff00 - 3, true, true)]
    {
        let mut obj = Object::new(Arch::AArch64);
        obj.set_stack_note(StackNote::Omit);
        for _ in 0..sections {
            obj.add_section(b".data", SectionKind::Data, 1).unwrap();
        }
        let elf = parse(&obj);
        assert_eq!(elf.sections.len() as u32, sections + 4);
        assert_eq!(elf.raw_shnum == 0, shnum_escaped, "{sections} sections");
        assert_eq!(elf.raw_shstrndx == 0xffff, shstrndx_escaped, "{sections} sections");
    }

    // A symbol in the last section whose index fits `st_shndx` (0xfeff),
    // then one more in the first section whose index does not (0xff00).
    let mut obj = Object::new(Arch::AArch64);
    for _ in 0..0xff00 {
        obj.add_section(b".data", SectionKind::Data, 1).unwrap();
    }
    let object = |index| {
        Symbol::defined(StrId::EMPTY, SymbolKind::Object, SectionId::from_index(index), 0, 0)
    };
    named(&mut obj, "fits", object(0xff00 - 2));
    let elf = parse(&obj);
    assert!(elf.named(".symtab_shndx").is_empty());
    assert_eq!(elf.symbols[elf.symbol("fits")].shndx, 0xff00 - 1);

    named(&mut obj, "escapes", object(0xff00 - 1));
    let elf = parse(&obj);
    assert_eq!(elf.named(".symtab_shndx").len(), 1);
    assert_eq!(elf.symbols[elf.symbol("fits")].shndx, 0xff00 - 1);
    assert_eq!(elf.symbols[elf.symbol("escapes")].shndx, 0xff00);
}

/// "Large section counts": an object with 70 000 function sections.
#[test]
fn more_than_65k_sections() {
    let count = 70_000;
    let obj = common::many_sections(count);
    let elf = parse(&obj);
    // Null, the sections, `main`'s, the stack note, one relocation section,
    // and four tables.
    assert_eq!(elf.sections.len() as u32, count + 8);
    assert_eq!((elf.raw_shnum, elf.raw_shstrndx), (0, 0xffff));
    assert_eq!(elf.sections[elf.only(".symtab_shndx")].link as usize, elf.symtab);

    let last = &elf.symbols[elf.symbol("f69999")];
    assert_eq!(last.shndx, count);
    assert_eq!(elf.sections[last.shndx as usize].name, ".text.f69999");
    assert_eq!(elf.data(last.shndx as usize), common::words(&[common::MOV_W0_42, common::RET]));
    let low = &elf.symbols[elf.symbol("f100")];
    assert_eq!(elf.sections[low.shndx as usize].name, ".text.f100");

    let main = &elf.symbols[elf.symbol("main")];
    assert_eq!(elf.sections[main.shndx as usize].name, ".text.main");
    let relas = elf.relas_of(main.shndx as usize);
    assert_eq!(relas, [Rela { offset: 0, sym: elf.symbol("f69999") as u32, kind: 282, addend: 0 }]);
    let rela = &elf.sections[elf.only(".rela.text.main")];
    assert_eq!(rela.info, main.shndx);
}

/// "Limits": the limits are far beyond what a test can build; what can be
/// checked is that the error exists and says what overflowed.
#[test]
fn limit_errors_name_the_limit() {
    assert_eq!(Error::TooLarge(Limit::Sections).to_string(), "too many sections");
    assert_eq!(Error::TooLarge(Limit::StringBytes).to_string(), "symbol names exceed 4 GiB");
}
