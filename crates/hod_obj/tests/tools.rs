//! `spec/object.md` "ELF writer", checked by external readers:
//! `llvm-readelf`, binutils `readelf` and `llvm-objdump` must read the
//! writer's objects without a warning and report what was put in.
//!
//! A missing tool skips its part with a note, or fails under
//! `HOD_REQUIRE_TOOLS=1` (`docs/VALIDATION.md` "Oracles").

mod common;

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use common::{function, undefined};
use hod_obj::{
    A64Reloc, Arch, Binding, Object, SectionKind, StrId, Symbol, SymbolKind, SymbolPlace,
    Visibility, X64Reloc, elf,
};
use hod_testutil::{Scratch, run, tool, tools_required};

/// The `readelf` implementations present: `(label, path)`.
fn readelfs() -> Vec<(&'static str, PathBuf)> {
    let mut found = Vec::new();
    if let Some(path) = tool("llvm-readelf") {
        found.push(("llvm-readelf", path));
    }
    // `readelf` may be a link to LLVM's; only the GNU one counts here.
    match tool("readelf") {
        Some(path) if run(&path, ["--version"]).stdout_text().contains("GNU readelf") => {
            found.push(("readelf", path));
        }
        Some(_) => {
            assert!(!tools_required(), "`readelf` is not GNU readelf (HOD_REQUIRE_TOOLS=1)");
            eprintln!("note: `readelf` is not GNU readelf; skipping it");
        }
        None => {}
    }
    found
}

/// Runs a tool on a file. It must succeed and print nothing on standard
/// error, which is where both `readelf`s put their warnings.
fn read(tool: &Path, flags: &[&str], file: &Path) -> String {
    let outcome = run(tool, flags.iter().map(OsStr::new).chain([file.as_os_str()]));
    assert!(outcome.success(), "{} {flags:?} failed: {}", tool.display(), outcome.stderr_text());
    assert_eq!(outcome.stderr_text(), "", "{} {flags:?} warned", tool.display());
    let text = outcome.stdout_text();
    for bad in ["warning", "Warning", "<corrupt", "out of range", "bad section index"] {
        assert!(!text.contains(bad), "{} {flags:?} printed `{bad}`", tool.display());
    }
    text
}

fn write(scratch: &Scratch, name: &str, obj: &Object) -> PathBuf {
    scratch.write(name, elf::to_vec(obj).unwrap())
}

/// One row of `readelf -S`.
#[derive(Debug, PartialEq, Eq)]
struct SectionRow {
    index: u32,
    name: String,
    kind: String,
    size: u64,
    entsize: u64,
    flags: String,
    link: u32,
    info: u32,
    align: u64,
}

/// One row of `readelf -s`: `(name, type, bind, visibility, index)`.
type SymbolRow = (String, String, String, String, String);

/// One row of `readelf -r`: `(offset, type, symbol, addend)`.
type RelocRow = (u64, String, String, i64);

/// What the GNU-style output of `readelf -S -s -r` says. Both
/// implementations print the same tables.
#[derive(Debug, Default)]
struct Report {
    sections: Vec<SectionRow>,
    symbols: Vec<SymbolRow>,
    relocs: Vec<RelocRow>,
}

impl Report {
    fn parse(text: &str) -> Self {
        let mut report = Self::default();
        for line in text.lines() {
            let line = line.trim();
            if let Some(row) = section_row(line) {
                report.sections.push(row);
            } else if let Some(row) = symbol_row(line) {
                report.symbols.push(row);
            } else if let Some(row) = reloc_row(line) {
                report.relocs.push(row);
            }
        }
        report
    }

    fn section(&self, name: &str) -> &SectionRow {
        let found: Vec<_> = self.sections.iter().filter(|s| s.name == name).collect();
        assert_eq!(found.len(), 1, "sections named {name}");
        found[0]
    }

    /// `(type, bind, visibility, index)` of the only symbol with this name.
    fn symbol(&self, name: &str) -> (&str, &str, &str, &str) {
        let found: Vec<_> = self.symbols.iter().filter(|s| s.0 == name).collect();
        assert_eq!(found.len(), 1, "symbols named {name}");
        let (_, kind, bind, visibility, index) = found[0];
        (kind, bind, visibility, index)
    }
}

/// `[Nr] Name Type Address Off Size ES Flg Lk Inf Al`, where the flags may
/// be absent and the type may be several words.
fn section_row(line: &str) -> Option<SectionRow> {
    let (index, rest) = line.strip_prefix('[')?.split_once(']')?;
    let index = index.trim().parse().ok()?;
    let mut tokens: Vec<&str> = rest.split_whitespace().collect();
    let align = tokens.pop()?.parse().ok()?;
    let info = tokens.pop()?.parse().ok()?;
    let sh_link = tokens.pop()?.parse().ok()?;
    let flags = if tokens.last()?.chars().all(|c| c.is_ascii_alphabetic()) {
        tokens.pop()?.to_owned()
    } else {
        String::new()
    };
    let entsize = u64::from_str_radix(tokens.pop()?, 16).ok()?;
    let size = u64::from_str_radix(tokens.pop()?, 16).ok()?;
    let _offset = tokens.pop()?;
    let _address = tokens.pop()?;
    // The null section has no name, and its type is one word.
    let (name, kind) = match tokens.as_slice() {
        [kind] => (String::new(), (*kind).to_owned()),
        [name, kind @ ..] => ((*name).to_owned(), kind.join(" ")),
        [] => return None,
    };
    Some(SectionRow { index, name, kind, size, entsize, flags, link: sh_link, info, align })
}

/// `Num: Value Size Type Bind Vis Ndx Name`, where the name may be absent
/// and the visibility may be followed by `[...]` annotations.
fn symbol_row(line: &str) -> Option<SymbolRow> {
    let tokens: Vec<&str> = line.split_whitespace().filter(|t| !t.starts_with('[')).collect();
    let [number, value, _size, kind, bind, visibility, index, name @ ..] = tokens.as_slice() else {
        return None;
    };
    number.strip_suffix(':')?.parse::<u32>().ok()?;
    (value.len() == 16).then_some(())?;
    let own = |s: &&str| (*s).to_owned();
    Some((
        name.first().map(own).unwrap_or_default(),
        own(kind),
        own(bind),
        own(visibility),
        own(index),
    ))
}

/// `Offset Info Type Value Name + Addend`.
fn reloc_row(line: &str) -> Option<RelocRow> {
    let tokens: Vec<&str> = line.split_whitespace().collect();
    let [offset, info, kind, _value, name, sign, addend] = tokens.as_slice() else {
        return None;
    };
    (offset.len() == 16 && info.len() == 16).then_some(())?;
    let offset = u64::from_str_radix(offset, 16).ok()?;
    let addend = i64::from_str_radix(addend, 16).ok()?;
    let addend = if *sign == "-" { -addend } else { addend };
    Some((offset, (*kind).to_owned(), (*name).to_owned(), addend))
}

fn reloc(offset: u64, kind: &str, symbol: &str, addend: i64) -> RelocRow {
    (offset, kind.to_owned(), symbol.to_owned(), addend)
}

/// "ELF writer": the smallest objects, read back in full.
#[test]
fn readelf_reads_hello() {
    let scratch = Scratch::new("obj-readelf-hello");
    let file = write(&scratch, "hello.o", &common::hello("hello from hod"));
    for (label, tool) in readelfs() {
        let text = read(&tool, &["--all", "-g", "-W"], &file);
        assert!(text.contains("REL (Relocatable file)") && text.contains("AArch64"), "{label}");
        assert!(text.contains("There are no section groups in this file"), "{label}");
        let report = Report::parse(&text);
        let rows: Vec<_> = report
            .sections
            .iter()
            .map(|s| (s.name.as_str(), s.kind.as_str(), s.size, s.flags.as_str(), s.align))
            .collect();
        assert_eq!(
            rows,
            [
                ("", "NULL", 0, "", 0),
                (".text", "PROGBITS", 32, "AX", 4),
                (".rodata", "PROGBITS", 15, "A", 1),
                (".note.GNU-stack", "PROGBITS", 0, "", 1),
                (".rela.text", "RELA", 72, "I", 8),
                (".symtab", "SYMTAB", 120, "", 8),
                (".strtab", "STRTAB", 26, "", 1),
                (".shstrtab", "STRTAB", 62, "", 1),
            ],
            "{label}"
        );
        let rela = report.section(".rela.text");
        assert_eq!((rela.link, rela.info, rela.entsize), (5, 1, 24), "{label}");
        let symtab = report.section(".symtab");
        assert_eq!((symtab.link, symtab.info, symtab.entsize), (6, 3, 24), "{label}");
        let own = |row: (&str, &str, &str, &str, &str)| {
            (
                row.0.to_owned(),
                row.1.to_owned(),
                row.2.to_owned(),
                row.3.to_owned(),
                row.4.to_owned(),
            )
        };
        assert_eq!(
            report.symbols,
            [
                ("", "NOTYPE", "LOCAL", "DEFAULT", "UND"),
                ("hello.c", "FILE", "LOCAL", "DEFAULT", "ABS"),
                (".L.str", "OBJECT", "LOCAL", "DEFAULT", "2"),
                ("puts", "NOTYPE", "GLOBAL", "DEFAULT", "UND"),
                ("main", "FUNC", "GLOBAL", "DEFAULT", "1"),
            ]
            .map(own),
            "{label}"
        );
        assert_eq!(
            report.relocs,
            [
                reloc(8, "R_AARCH64_ADR_PREL_PG_HI21", ".L.str", 0),
                reloc(12, "R_AARCH64_ADD_ABS_LO12_NC", ".L.str", 0),
                reloc(16, "R_AARCH64_CALL26", "puts", 0),
            ],
            "{label}"
        );
    }
}

/// An object with every section kind and every kind of symbol.
fn everything() -> Object {
    let mut obj = Object::new(Arch::AArch64);
    obj.add_file_symbol(b"everything.cc").unwrap();
    let text = obj.add_section(b".text", SectionKind::Text, 4).unwrap();
    let data = obj.add_section(b".data", SectionKind::Data, 8).unwrap();
    let rodata = obj.add_section(b".rodata", SectionKind::ReadOnlyData, 16).unwrap();
    let bss = obj.add_section(b".bss", SectionKind::Bss, 32).unwrap();
    let tdata = obj.add_section(b".tdata", SectionKind::Tls, 8).unwrap();
    let tbss = obj.add_section(b".tbss", SectionKind::TlsBss, 8).unwrap();
    let init = obj.add_section(b".init_array", SectionKind::InitArray, 8).unwrap();
    let fini = obj.add_section(b".fini_array", SectionKind::FiniArray, 8).unwrap();
    let note = obj.add_section(b".note.hod", SectionKind::Note, 4).unwrap();
    let debug = obj.add_section(b".debug_abbrev", SectionKind::Debug, 1).unwrap();
    let strings = SectionKind::MergeableStrings { char_size: 1 };
    let strings = obj.add_section(b".rodata.str1.1", strings, 1).unwrap();
    let constants = SectionKind::MergeableConstants { entry_size: 8 };
    let constants = obj.add_section(b".rodata.cst8", constants, 8).unwrap();
    let debug_str = SectionKind::Elf {
        sh_type: elf::SHT_PROGBITS,
        sh_flags: elf::SHF_MERGE | elf::SHF_STRINGS,
        sh_entsize: 1,
    };
    let debug_str = obj.add_section(b".debug_str", debug_str, 1).unwrap();
    obj.append(strings, b"one\0two\0");
    obj.append(constants, &[1; 16]);
    obj.append(debug_str, b"producer\0");
    obj.append(debug, &[0]);
    // One note: owner "hod", four bytes of description, type 1.
    let mut record = Vec::new();
    for word in [4u32, 4, 1] {
        record.extend_from_slice(&word.to_le_bytes());
    }
    record.extend_from_slice(b"hod\0\x01\x02\x03\x04");
    obj.append_owned(note, record);
    obj.append_zeros(data, 24);
    obj.append_zeros(rodata, 16);
    obj.append_zeros(bss, 4096);
    obj.append_zeros(tdata, 8);
    obj.append_zeros(tbss, 16);
    obj.append_zeros(init, 8);
    obj.append_zeros(fini, 8);

    let ctor = function(&mut obj, text, "ctor", &[common::RET]);
    let dtor = function(&mut obj, text, "dtor", &[common::RET]);
    obj.symbol_mut(dtor).binding = Binding::Local;
    obj.add_relocation(init, 0, A64Reloc::Abs64, ctor, 0);
    obj.add_relocation(fini, 0, A64Reloc::Abs64, dtor, 0);
    let text_section = obj.section_symbol(text);
    obj.add_relocation(data, 8, A64Reloc::Abs64, text_section, 4);
    obj.add_relocation(data, 16, A64Reloc::Prel32, text_section, -8);

    let mut add = |name: &str, symbol: Symbol| {
        let name = obj.add_string(name.as_bytes()).unwrap();
        obj.add_symbol(Symbol { name, ..symbol })
    };
    let object =
        |section, size| Symbol::defined(StrId::EMPTY, SymbolKind::Object, section, 0, size);
    add("variable", object(data, 8));
    add("hidden", object(rodata, 16).with_visibility(Visibility::Hidden));
    add("protected", object(rodata, 16).with_visibility(Visibility::Protected));
    add("weak", object(bss, 4096).with_binding(Binding::Weak));
    add("tls_init", Symbol { kind: SymbolKind::Tls, ..object(tdata, 8) });
    add("tls_zero", Symbol { kind: SymbolKind::Tls, ..object(tbss, 16) });
    add("absolute", Symbol { place: SymbolPlace::Absolute, value: 0x1234, ..object(data, 0) });
    add("common", Symbol { place: SymbolPlace::Common, value: 8, ..object(data, 64) });
    add("resolver", Symbol::defined(StrId::EMPTY, SymbolKind::GnuIfunc, text, 0, 4));
    add(
        "vector",
        Symbol {
            other: elf::STO_AARCH64_VARIANT_PCS,
            ..Symbol::defined(StrId::EMPTY, SymbolKind::Func, text, 4, 4)
        },
    );
    let external = undefined(&mut obj, "external");
    obj.add_relocation(data, 0, A64Reloc::Abs64, external, 0);
    obj
}

/// "Section kinds" and "Symbol table", as both `readelf`s see them.
#[test]
fn readelf_reads_every_section_and_symbol_kind() {
    let scratch = Scratch::new("obj-readelf-everything");
    let file = write(&scratch, "everything.o", &everything());
    for (label, tool) in readelfs() {
        let text = read(&tool, &["--all", "-g", "-W"], &file);
        assert!(text.contains("UNIX - GNU"), "{label}: an IFUNC symbol sets the OS ABI");
        assert!(text.contains("VARIANT_PCS"), "{label}");
        let report = Report::parse(&text);
        let expected = [
            (".text", "PROGBITS", 8, 0, "AX", 4),
            (".data", "PROGBITS", 24, 0, "WA", 8),
            (".rodata", "PROGBITS", 16, 0, "A", 16),
            (".bss", "NOBITS", 4096, 0, "WA", 32),
            (".tdata", "PROGBITS", 8, 0, "WAT", 8),
            (".tbss", "NOBITS", 16, 0, "WAT", 8),
            (".init_array", "INIT_ARRAY", 8, 0, "WA", 8),
            (".fini_array", "FINI_ARRAY", 8, 0, "WA", 8),
            (".note.hod", "NOTE", 20, 0, "A", 4),
            (".debug_abbrev", "PROGBITS", 1, 0, "", 1),
            (".rodata.str1.1", "PROGBITS", 8, 1, "AMS", 1),
            (".rodata.cst8", "PROGBITS", 16, 8, "AM", 8),
            (".debug_str", "PROGBITS", 9, 1, "MS", 1),
            (".note.GNU-stack", "PROGBITS", 0, 0, "", 1),
            (".rela.data", "RELA", 72, 24, "I", 8),
            (".rela.init_array", "RELA", 24, 24, "I", 8),
            (".rela.fini_array", "RELA", 24, 24, "I", 8),
        ];
        for (index, (name, kind, size, entsize, flags, align)) in expected.into_iter().enumerate() {
            let row = report.section(name);
            assert_eq!(row.index as usize, index + 1, "{label}: {name}");
            assert_eq!(
                (row.kind.as_str(), row.size, row.entsize, row.flags.as_str(), row.align),
                (kind, size, entsize, flags, align),
                "{label}: {name}"
            );
        }
        assert_eq!(report.section(".rela.init_array").info, report.section(".init_array").index);

        let expected = [
            ("everything.cc", ("FILE", "LOCAL", "DEFAULT", "ABS")),
            ("dtor", ("FUNC", "LOCAL", "DEFAULT", "1")),
            ("ctor", ("FUNC", "GLOBAL", "DEFAULT", "1")),
            ("variable", ("OBJECT", "GLOBAL", "DEFAULT", "2")),
            ("hidden", ("OBJECT", "GLOBAL", "HIDDEN", "3")),
            ("protected", ("OBJECT", "GLOBAL", "PROTECTED", "3")),
            ("weak", ("OBJECT", "WEAK", "DEFAULT", "4")),
            ("tls_init", ("TLS", "GLOBAL", "DEFAULT", "5")),
            ("tls_zero", ("TLS", "GLOBAL", "DEFAULT", "6")),
            ("absolute", ("OBJECT", "GLOBAL", "DEFAULT", "ABS")),
            ("common", ("OBJECT", "GLOBAL", "DEFAULT", "COM")),
            ("resolver", ("IFUNC", "GLOBAL", "DEFAULT", "1")),
            ("vector", ("FUNC", "GLOBAL", "DEFAULT", "1")),
            ("external", ("NOTYPE", "GLOBAL", "DEFAULT", "UND")),
        ];
        for (name, row) in expected {
            assert_eq!(report.symbol(name), row, "{label}: {name}");
        }
        // The section symbol: both tools show the section's name for it.
        assert_eq!(report.symbol(".text"), ("SECTION", "LOCAL", "DEFAULT", "1"), "{label}");
        assert_eq!(report.symbols.len(), 16, "{label}");
        assert_eq!(
            report.relocs,
            [
                reloc(0, "R_AARCH64_ABS64", "external", 0),
                reloc(8, "R_AARCH64_ABS64", ".text", 4),
                reloc(16, "R_AARCH64_PREL32", ".text", -8),
                reloc(0, "R_AARCH64_ABS64", "ctor", 0),
                reloc(0, "R_AARCH64_ABS64", "dtor", 0),
            ],
            "{label}"
        );
    }
}

/// "Relocations": every named kind has the number the psABI gives the
/// name, according to both `readelf`s.
#[test]
fn readelf_names_every_relocation_kind() {
    // Kinds this binutils does not know by name yet; it prints
    // `unrecognized` for them and LLVM's is the only oracle.
    const NEWER_THAN_BINUTILS: [&str; 2] = ["R_AARCH64_PLT32", "R_AARCH64_GOTPCREL32"];

    let scratch = Scratch::new("obj-readelf-relocs");
    let build = |arch, kinds: Vec<hod_obj::RelocKind>| {
        let mut obj = Object::new(arch);
        let text = obj.add_section(b".text", SectionKind::Text, 4).unwrap();
        obj.append_zeros(text, 8 * kinds.len() as u64);
        let symbol = undefined(&mut obj, "symbol");
        let mut names = Vec::new();
        for (index, kind) in kinds.into_iter().enumerate() {
            obj.add_relocation(text, 8 * index as u64, kind, symbol, 0);
            names.push(kind.elf_name().unwrap());
        }
        (obj, names)
    };
    let a64 = build(Arch::AArch64, A64Reloc::NAMED.iter().map(|&k| k.into()).collect());
    let x64 = build(Arch::X86_64, X64Reloc::NAMED.iter().map(|&k| k.into()).collect());
    for (file, (obj, names)) in [("a64.o", a64), ("x64.o", x64)] {
        let file = write(&scratch, file, &obj);
        for (label, tool) in readelfs() {
            let text = read(&tool, &["-r", "-W"], &file);
            let printed: Vec<&str> = text
                .lines()
                .filter(|line| {
                    line.len() > 34 && line.as_bytes()[..16].iter().all(u8::is_ascii_hexdigit)
                })
                .map(|line| line.split_whitespace().nth(2).unwrap())
                .collect();
            assert_eq!(printed.len(), names.len(), "{label}");
            for (printed, expected) in printed.into_iter().zip(&names) {
                if label == "readelf" && NEWER_THAN_BINUTILS.contains(expected) {
                    assert!(printed == *expected || printed.starts_with("unrecognized"), "{label}");
                } else {
                    assert_eq!(printed, *expected, "{label}");
                }
            }
        }
    }
}

/// "Section groups", as both `readelf`s see them.
#[test]
fn readelf_reads_comdat_groups() {
    let mut obj = common::return_42();
    common::add_answer_comdat(&mut obj, Binding::Weak, 42);
    let scratch = Scratch::new("obj-readelf-groups");
    let file = write(&scratch, "comdat.o", &obj);
    for (label, tool) in readelfs() {
        let text = read(&tool, &["--all", "-g", "-W"], &file);
        let lines: Vec<String> =
            text.lines().map(|l| l.split_whitespace().collect::<Vec<_>>().join(" ")).collect();
        let start = lines
            .iter()
            .position(|l| l == "COMDAT group section [ 1] `.group' [answer] contains 3 sections:")
            .unwrap_or_else(|| panic!("{label}: no group header in:\n{text}"));
        assert_eq!(
            lines[start + 2..start + 5],
            ["[ 3] .text.answer", "[ 6] .rela.text.answer", "[ 4] .rodata.answer"],
            "{label}"
        );
        let report = Report::parse(&text);
        let group = report.section(".group");
        assert_eq!((group.kind.as_str(), group.index, group.entsize), ("GROUP", 1, 4), "{label}");
        assert_eq!(report.section(".text.answer").flags, "AXG", "{label}");
        assert_eq!(report.section(".rodata.answer").flags, "AG", "{label}");
        assert_eq!(report.section(".rela.text.answer").flags, "IG", "{label}");
        assert_eq!(report.symbol("answer"), ("FUNC", "WEAK", "DEFAULT", "3"), "{label}");
    }
}

/// "Large section counts", as both `readelf`s see them.
#[test]
fn readelf_reads_70000_sections() {
    let scratch = Scratch::new("obj-readelf-many");
    let file = write(&scratch, "many.o", &common::many_sections(70_000));
    for (label, tool) in readelfs() {
        let text = read(&tool, &["--all", "-g", "-W"], &file);
        let lines: Vec<String> =
            text.lines().map(|l| l.split_whitespace().collect::<Vec<_>>().join(" ")).collect();
        let has = |line: &str| lines.iter().any(|l| l == line);
        assert!(has("Number of section headers: 0 (70008)"), "{label}");
        assert!(has("Section header string table index: 65535 (70007)"), "{label}");
        let report = Report::parse(&text);
        assert_eq!(report.sections.len(), 70_008, "{label}");
        assert_eq!(report.section(".text.f69999").index, 70_000, "{label}");
        assert_eq!(report.section(".rela.text.main").info, 70_001, "{label}");
        assert_eq!(report.section(".symtab_shndx").link, report.section(".symtab").index);
        assert_eq!(report.symbol("f65279"), ("FUNC", "GLOBAL", "DEFAULT", "65280"), "{label}");
        assert_eq!(report.symbol("f69999"), ("FUNC", "GLOBAL", "DEFAULT", "70000"), "{label}");
        assert_eq!(report.symbol("main"), ("FUNC", "GLOBAL", "DEFAULT", "70001"), "{label}");
        assert_eq!(report.relocs, [reloc(0, "R_AARCH64_JUMP26", "f69999", 0)], "{label}");
    }
}

/// `int main(void) { puts("x86"); return 42; }` for x86-64, and a pointer
/// to `main` in `.data`.
fn x86_64_program() -> Object {
    let mut obj = Object::new(Arch::X86_64);
    let text = obj.add_section(b".text", SectionKind::Text, 16).unwrap();
    let rodata = obj.add_section(b".rodata", SectionKind::ReadOnlyData, 1).unwrap();
    let data = obj.add_section(b".data", SectionKind::Data, 8).unwrap();
    obj.append(rodata, b"x86\0");
    let name = obj.add_string(b".L.str").unwrap();
    let string = obj.add_symbol(
        Symbol::defined(name, SymbolKind::Object, rodata, 0, 4).with_binding(Binding::Local),
    );
    let puts = undefined(&mut obj, "puts");
    #[rustfmt::skip]
    let code = [
        0x55,                                     // push %rbp
        0x48, 0x89, 0xe5,                         // mov %rsp, %rbp
        0x48, 0x8d, 0x3d, 0, 0, 0, 0,             // lea .L.str(%rip), %rdi
        0xe8, 0, 0, 0, 0,                         // call puts
        0xb8, 42, 0, 0, 0,                        // mov $42, %eax
        0x5d,                                     // pop %rbp
        0xc3,                                     // ret
    ];
    obj.append(text, &code);
    let name = obj.add_string(b"main").unwrap();
    let main = obj.add_symbol(Symbol::defined(name, SymbolKind::Func, text, 0, code.len() as u64));
    obj.add_relocation(text, 7, X64Reloc::Pc32, string, -4);
    obj.add_relocation(text, 12, X64Reloc::Plt32, puts, -4);
    obj.append_zeros(data, 8);
    obj.add_relocation(data, 0, X64Reloc::Abs64, main, 0);
    obj
}

fn squeeze(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// "File header" and "Relocations" for x86-64, where the host cannot run
/// the program: `llvm-readelf` and `llvm-objdump` read the object.
#[test]
fn x86_64_object_is_read_by_llvm_tools() {
    let scratch = Scratch::new("obj-x86");
    let file = write(&scratch, "x86.o", &x86_64_program());
    for (label, tool) in readelfs() {
        let text = read(&tool, &["--all", "-g", "-W"], &file);
        assert!(text.contains("Advanced Micro Devices X86-64"), "{label}");
        let report = Report::parse(&text);
        assert_eq!(
            report.relocs,
            [
                reloc(7, "R_X86_64_PC32", ".L.str", -4),
                reloc(12, "R_X86_64_PLT32", "puts", -4),
                reloc(0, "R_X86_64_64", "main", 0),
            ],
            "{label}"
        );
        assert_eq!(report.section(".text").align, 16, "{label}");
        assert_eq!(report.symbol("main"), ("FUNC", "GLOBAL", "DEFAULT", "1"), "{label}");
    }
    let Some(objdump) = tool("llvm-objdump") else { return };
    let text = squeeze(&read(&objdump, &["-d", "-r"], &file));
    for expected in [
        "file format elf64-x86-64",
        "<main>:",
        "pushq %rbp",
        "leaq (%rip), %rdi",
        "R_X86_64_PC32 .L.str-0x4",
        "R_X86_64_PLT32 puts-0x4",
        "movl $0x2a, %eax",
        "retq",
    ] {
        assert!(text.contains(expected), "llvm-objdump: no `{expected}` in:\n{text}");
    }
}

/// The AArch64 program of `tests/link.rs`, disassembled: the code and its
/// relocations are where the linker will look for them.
#[test]
fn aarch64_object_is_disassembled_by_llvm_objdump() {
    let Some(objdump) = tool("llvm-objdump") else { return };
    let scratch = Scratch::new("obj-objdump");
    let file = write(&scratch, "hello.o", &common::hello("hi"));
    let text = squeeze(&read(&objdump, &["-d", "-r"], &file));
    for expected in [
        "file format elf64-littleaarch64",
        "<main>:",
        "stp x29, x30, [sp, #-0x10]!",
        "adrp x0, 0x0 <main>",
        "R_AARCH64_ADR_PREL_PG_HI21 .L.str",
        "R_AARCH64_ADD_ABS_LO12_NC .L.str",
        "R_AARCH64_CALL26 puts",
        "ldp x29, x30, [sp], #0x10",
        "ret",
    ] {
        assert!(text.contains(expected), "llvm-objdump: no `{expected}` in:\n{text}");
    }
}
