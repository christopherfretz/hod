//! ELF32 for 32-bit x86 (`spec/object.md` "ELF32 and `SHT_REL`", WP-284):
//! the writer's refusals, the in-place addends, a hand-built program read
//! by both `llvm-readelf`s and compared with LLVM's assembler's object,
//! linked by GNU ld, lld and mold and run under `qemu-i386`.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use hod_obj::{
    Binding, Compression, Error, GroupKind, I386Reloc, Limit, Machine, Object, SectionId,
    SectionKind, Symbol, SymbolId, SymbolKind, SymbolPlace, X64Reloc, elf,
};
use hod_testutil::{Scratch, find_tool, run, tool, tools_required};

fn string(obj: &mut Object, name: &str) -> hod_obj::StrId {
    obj.add_string(name.as_bytes()).unwrap()
}

fn define(
    obj: &mut Object,
    name: &str,
    kind: SymbolKind,
    section: SectionId,
    at: u64,
    size: u64,
) -> SymbolId {
    let name = string(obj, name);
    obj.add_symbol(Symbol::defined(name, kind, section, at, size))
}

/// The program of `i386_program.s` (below), built by hand: `_start` loads
/// `answer` (40), calls `helper` (adds 2) and exits with that status
/// through `int $0x80`. `R_386_32` and `R_386_PC32` against another
/// section and symbol, with addends 0, -4, 4, -8 and 16 in the bytes.
fn program() -> Object {
    let mut obj = Object::for_machine(Machine::I386);
    let text = obj.add_section(b".text", SectionKind::Text, 4).unwrap();
    // movl answer, %ebx; calll helper; movl $1, %eax; int $0x80
    obj.append(text, &[0x8b, 0x1d, 0, 0, 0, 0, 0xe8, 0, 0, 0, 0]);
    obj.append(text, &[0xb8, 1, 0, 0, 0, 0xcd, 0x80]);
    let helper_text = obj.add_section(b".text.helper", SectionKind::Text, 1).unwrap();
    // addl $2, %ebx; retl
    obj.append(helper_text, &[0x83, 0xc3, 0x02, 0xc3]);
    let data = obj.add_section(b".data", SectionKind::Data, 4).unwrap();
    obj.append(data, &[40, 0, 0, 0]);
    obj.append_zeros(data, 8);
    let debug = obj.add_section(b".debug_info", SectionKind::Debug, 1).unwrap();
    obj.append_zeros(debug, 204);

    let helper = define(&mut obj, "helper", SymbolKind::Func, helper_text, 0, 4);
    obj.symbol_mut(helper).binding = Binding::Local;
    let helper_section = obj.section_symbol(helper_text);
    let start = define(&mut obj, "_start", SymbolKind::Func, text, 0, 18);
    let answer = define(&mut obj, "answer", SymbolKind::NoType, data, 0, 0);
    let _ = helper;

    obj.add_relocation(text, 2, I386Reloc::Abs32, answer, 0);
    obj.add_relocation(text, 7, I386Reloc::Pc32, helper_section, -4);
    obj.add_relocation(data, 4, I386Reloc::Abs32, answer, 4);
    obj.add_relocation(data, 8, I386Reloc::Abs32, start, -8);
    obj.add_relocation(debug, 0, I386Reloc::Abs32, answer, 0x10);
    obj
}

/// What LLVM's assembler is given for the same object.
const PROGRAM_S: &str = "\t.text
\t.globl _start
\t.type _start,@function
_start:
\tmovl answer, %ebx
\tcalll helper
\tmovl $1, %eax
\tint $0x80
\t.size _start, .-_start
\t.section .text.helper,\"ax\",@progbits
\t.type helper,@function
helper:
\taddl $2, %ebx
\tretl
\t.size helper, .-helper
\t.data
\t.globl answer
\t.p2align 2
answer:
\t.long 40
\t.long answer+4
\t.long _start-8
\t.section .debug_info,\"\",@progbits
\t.long answer+0x10
\t.zero 200
";

fn bytes(obj: &Object) -> Vec<u8> {
    elf::to_vec(obj).unwrap()
}

fn u32_at(file: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(file[at..at + 4].try_into().unwrap())
}

/// A section's file bytes, by name, read with the ELF32 layout alone.
fn section_bytes<'f>(file: &'f [u8], wanted: &str) -> &'f [u8] {
    let shoff = u32_at(file, 32) as usize;
    let shnum = usize::from(u16::from_le_bytes([file[48], file[49]]));
    let shstrndx = usize::from(u16::from_le_bytes([file[50], file[51]]));
    let header = |index: usize| &file[shoff + 40 * index..shoff + 40 * index + 40];
    let names = u32_at(header(shstrndx), 16) as usize;
    for index in 0..shnum {
        let h = header(index);
        let name = &file[names + u32_at(h, 0) as usize..];
        let name = &name[..name.iter().position(|&b| b == 0).unwrap()];
        if name == wanted.as_bytes() {
            let (offset, size) = (u32_at(h, 16) as usize, u32_at(h, 20) as usize);
            return &file[offset..offset + size];
        }
    }
    panic!("no section {wanted}");
}

#[test]
fn header_and_records_are_elf32() {
    let file = bytes(&program());
    assert_eq!(&file[..8], b"\x7fELF\x01\x01\x01\x00");
    assert_eq!(u16::from_le_bytes([file[16], file[17]]), 1, "ET_REL");
    assert_eq!(u16::from_le_bytes([file[18], file[19]]), 3, "EM_386");
    assert_eq!(&file[40..42], &[52, 0], "e_ehsize");
    assert_eq!(&file[46..48], &[40, 0], "e_shentsize");
    assert_eq!(u32_at(&file, 32) % 4, 0, "section headers aligned to 4");
    let rel = section_bytes(&file, ".rel.data");
    assert_eq!(rel.len(), 16, "two `Elf32_Rel`");
    assert_eq!(rel[4], 1, "R_386_32 in the low byte of r_info");
    assert_eq!(section_bytes(&file, ".symtab").len() % 16, 0);
}

/// The rule: `field := field + addend` (mod 2^32), the model unchanged.
#[test]
fn addends_are_added_into_the_section_bytes() {
    let obj = program();
    let file = bytes(&obj);
    assert_eq!(
        section_bytes(&file, ".text"),
        [0x8b, 0x1d, 0, 0, 0, 0, 0xe8, 0xfc, 0xff, 0xff, 0xff, 0xb8, 1, 0, 0, 0, 0xcd, 0x80]
    );
    assert_eq!(section_bytes(&file, ".data"), [40, 0, 0, 0, 4, 0, 0, 0, 0xf8, 0xff, 0xff, 0xff]);
    assert_eq!(&section_bytes(&file, ".debug_info")[..4], [0x10, 0, 0, 0]);
    // The model keeps its bytes and its addends.
    let data = SectionId::from_index(2);
    assert_eq!(obj.data(data), [40, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    let addends: Vec<i64> = obj.relocations(data).map(|r| r.addend).collect();
    assert_eq!(addends, [4, -8]);

    // A field that already holds a value keeps it, plus the addend; the
    // extremes of the range are stored, wrapping modulo 2^32.
    let mut obj = Object::for_machine(Machine::I386);
    let data = obj.add_section(b".data", SectionKind::Data, 4).unwrap();
    obj.append(data, &[4, 0, 0, 0, 0xff, 0xff, 0xff, 0xff]);
    obj.append_zeros(data, 8);
    let sym = obj.section_symbol(data);
    obj.add_relocation(data, 0, I386Reloc::Abs32, sym, 4);
    obj.add_relocation(data, 4, I386Reloc::Abs32, sym, 1);
    obj.add_relocation(data, 8, I386Reloc::Abs32, sym, (1 << 32) - 1);
    obj.add_relocation(data, 12, I386Reloc::GotOff, sym, -(1 << 31));
    let file = bytes(&obj);
    assert_eq!(
        section_bytes(&file, ".data"),
        [8, 0, 0, 0, 0, 0, 0, 0, 0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0x80]
    );
}

#[test]
fn addends_that_do_not_fit_are_refused() {
    let refused = |kind: I386Reloc, offset: u64, addend: i64| {
        let mut obj = Object::for_machine(Machine::I386);
        let text = obj.add_section(b".text", SectionKind::Text, 4).unwrap();
        obj.append_zeros(text, 8);
        let sym = obj.section_symbol(text);
        obj.add_relocation(text, offset, kind, sym, addend);
        elf::to_vec(&obj).unwrap_err()
    };
    let text = SectionId::from_index(0);
    let not = |offset, addend| Error::AddendNotRepresentable { section: text, offset, addend };
    assert_eq!(refused(I386Reloc::Abs32, 0, 1 << 32), not(0, 1 << 32));
    assert_eq!(refused(I386Reloc::Pc32, 0, -(1 << 31) - 1), not(0, -(1 << 31) - 1));
    assert_eq!(refused(I386Reloc::None, 0, 1), not(0, 1));
    assert_eq!(refused(I386Reloc::Other(3), 0, 1), not(0, 1));
    // A field that reaches past the end of its section.
    assert_eq!(
        refused(I386Reloc::Abs32, 6, 1),
        Error::RelocationOutOfBounds { section: text, offset: 6 }
    );
    // Addend zero writes nothing: any kind, `Other` too, is accepted.
    let mut obj = Object::for_machine(Machine::I386);
    let text = obj.add_section(b".text", SectionKind::Text, 4).unwrap();
    obj.append_zeros(text, 4);
    let sym = obj.section_symbol(text);
    obj.add_relocation(text, 0, I386Reloc::Other(3), sym, 0);
    assert!(elf::to_vec(&obj).is_ok());
    // `r_info` has eight bits for the type.
    obj.add_relocation(text, 0, I386Reloc::Other(256), sym, 0);
    assert_eq!(elf::to_vec(&obj).unwrap_err(), Error::TooLarge(Limit::Elf32Field));
}

#[test]
fn kinds_of_another_machine_are_refused() {
    let mut obj = Object::for_machine(Machine::I386);
    let text = obj.add_section(b".text", SectionKind::Text, 4).unwrap();
    obj.append_zeros(text, 8);
    let sym = obj.section_symbol(text);
    obj.add_relocation(text, 0, X64Reloc::Pc32, sym, 0);
    assert_eq!(
        elf::to_vec(&obj).unwrap_err(),
        Error::RelocationArchMismatch { section: text, kind: X64Reloc::Pc32.into() }
    );
    let mut obj = Object::new(hod_obj::Arch::X86_64);
    let text = obj.add_section(b".text", SectionKind::Text, 4).unwrap();
    obj.append_zeros(text, 8);
    let sym = obj.section_symbol(text);
    obj.add_relocation(text, 0, I386Reloc::Pc32, sym, 0);
    assert_eq!(
        elf::to_vec(&obj).unwrap_err(),
        Error::RelocationArchMismatch { section: text, kind: I386Reloc::Pc32.into() }
    );
}

#[test]
fn values_beyond_32_bits_are_refused() {
    let mut obj = Object::for_machine(Machine::I386);
    let text = obj.add_section(b".text", SectionKind::Text, 4).unwrap();
    let name = string(&mut obj, "far");
    obj.add_symbol(Symbol {
        place: SymbolPlace::Absolute,
        value: 1 << 32,
        ..Symbol::undefined(name)
    });
    assert_eq!(elf::to_vec(&obj).unwrap_err(), Error::TooLarge(Limit::Elf32Field));
    let _ = text;
    let mut obj = Object::for_machine(Machine::I386);
    let bss = obj.add_section(b".bss", SectionKind::Bss, 4).unwrap();
    obj.append_zeros(bss, 1 << 32);
    assert_eq!(elf::to_vec(&obj).unwrap_err(), Error::TooLarge(Limit::Elf32Field));
}

#[test]
fn property_notes_are_word_padded_and_arrays_are_word_aligned() {
    let mut obj = Object::for_machine(Machine::I386);
    assert!(matches!(elf::add_gnu_property_note(&mut obj, 3), Err(Error::NotInElf32 { .. })));
    // WP-324: the property list at ELF32's 4-byte padding and alignment,
    // `llc`'s 28 bytes for one x86 feature word (IBT | SHSTK).
    let word = 3u32.to_le_bytes();
    let note =
        elf::add_gnu_properties(&mut obj, &[(elf::GNU_PROPERTY_X86_FEATURE_1_AND, &word)]).unwrap();
    assert_eq!(obj.section_align(note), 4);
    let llc: [u8; 28] = [
        4, 0, 0, 0, 12, 0, 0, 0, 5, 0, 0, 0, b'G', b'N', b'U', 0, 2, 0, 0, 0xc0, 4, 0, 0, 0, 3, 0,
        0, 0,
    ];
    assert_eq!(elf::gnu_properties_padded(&[(elf::GNU_PROPERTY_X86_FEATURE_1_AND, &word)], 4), llc);
    let init = elf::add_init_array(&mut obj, 101).unwrap();
    assert_eq!(obj.section_align(init), 4);
    assert_eq!(obj.machine(), Machine::I386);
    assert_eq!(Machine::I386.arch(), None);
    assert_eq!(Machine::from(hod_obj::Arch::X86_64).address_size(), 8);
}

#[test]
#[should_panic(expected = "use `Object::machine`")]
fn an_i386_object_has_no_arch() {
    let _ = Object::for_machine(Machine::I386).arch();
}

/// `Elf32_Chdr` (12 bytes, alignment 4), and the addends in the bytes
/// before they are compressed; a later nonzero addend is refused.
#[test]
fn compressed_sections_carry_their_addends() {
    let mut obj = program();
    assert_eq!(obj.compress_debug_sections(Compression::Zlib).unwrap(), 1);
    let debug = SectionId::from_index(3);
    assert_eq!(obj.section_align(debug), 4);
    let data = obj.data(debug);
    assert_eq!(u32_at(data, 0), 1, "ELFCOMPRESS_ZLIB");
    assert_eq!(u32_at(data, 4), 204, "ch_size");
    assert_eq!(u32_at(data, 8), 1, "ch_addralign");
    assert_eq!(&data[12..14], [0x78, 0x01]);
    let file = bytes(&obj);
    let scratch = Scratch::new("obj-i386-compressed");
    let path = scratch.write("z.o", &file);
    if let Some(readelf) = tool("llvm-readelf") {
        let out = run(
            &readelf,
            [OsStr::new("-z"), OsStr::new("-x"), OsStr::new(".debug_info"), path.as_os_str()],
        );
        assert!(out.success(), "{}", out.stderr_text());
        assert!(
            out.stdout_text().contains("0x00000000 10000000 00000000"),
            "{}",
            out.stdout_text()
        );
    }
    let sym = obj.section_symbol(debug);
    obj.add_relocation(debug, 4, I386Reloc::Abs32, sym, 1);
    assert_eq!(
        elf::to_vec(&obj).unwrap_err(),
        Error::AddendNotRepresentable { section: debug, offset: 4, addend: 1 }
    );
}

// ---- external tools ----

/// Runs a tool that must succeed without a warning; its standard output.
fn read(tool: &Path, args: &[&str], file: &Path) -> String {
    let out = run(tool, args.iter().map(OsStr::new).chain([file.as_os_str()]));
    assert!(out.success(), "{} {args:?}: {}", tool.display(), out.stderr_text());
    assert_eq!(out.stderr_text(), "", "{} {args:?} warned", tool.display());
    out.stdout_text()
}

/// Both LLVM majors' `llvm-readelf`, as many as are present.
fn readelfs() -> Vec<PathBuf> {
    let found: Vec<PathBuf> =
        ["llvm-readelf-22", "llvm-readelf-19"].into_iter().filter_map(find_tool).collect();
    if found.is_empty() {
        let _ = tool("llvm-readelf");
    }
    found
}

/// The lines of `text` from the one starting with `from`, up to the next
/// empty line.
fn block(text: &str, from: &str) -> Vec<String> {
    text.lines()
        .skip_while(|l| !l.starts_with(from))
        .take_while(|l| !l.trim().is_empty())
        .map(|l| l.split_whitespace().collect::<Vec<_>>().join(" "))
        .collect()
}

const EXPECTED_HEADER: &[&str] = &[
    "Class: ELF32",
    "Data: 2's complement, little endian",
    "OS/ABI: UNIX - System V",
    "Type: REL (Relocatable file)",
    "Machine: Intel 80386",
    "Entry point address: 0x0",
    "Flags: 0x0",
    "Size of this header: 52 (bytes)",
    "Size of section headers: 40 (bytes)",
];

const EXPECTED_RELOCS: &[&str] = &[
    "Relocation section '.rel.text'",
    "00000002 00000401 R_386_32 00000000 answer",
    "00000007 00000202 R_386_PC32 00000000 .text.helper",
    "Relocation section '.rel.data'",
    "00000004 00000401 R_386_32 00000000 answer",
    "00000008 00000301 R_386_32 00000000 _start",
    "Relocation section '.rel.debug_info'",
    "00000000 00000401 R_386_32 00000000 answer",
];

const EXPECTED_SYMBOLS: &[&str] = &[
    "0: 00000000 0 NOTYPE LOCAL DEFAULT UND",
    "1: 00000000 4 FUNC LOCAL DEFAULT 2 helper",
    "2: 00000000 0 SECTION LOCAL DEFAULT 2 .text.helper",
    "3: 00000000 18 FUNC GLOBAL DEFAULT 1 _start",
    "4: 00000000 0 NOTYPE GLOBAL DEFAULT 3 answer",
];

/// `llvm-readelf -h -S -s -r` of LLVM 19 and 22 read the program as
/// expected, and as they read LLVM's own object of the same assembly
/// (which orders its sections differently).
#[test]
fn readelf_reads_the_program_as_llvm_writes_it() {
    let scratch = Scratch::new("obj-i386-readelf");
    let ours = scratch.write("hod.o", bytes(&program()));
    let theirs = find_tool("llvm-mc").map(|mc| {
        let source = scratch.write("program.s", PROGRAM_S);
        let object = scratch.join("llvm.o");
        let out = run(
            &mc,
            [
                OsStr::new("-triple=i686-linux-gnu"),
                OsStr::new("-filetype=obj"),
                source.as_os_str(),
                OsStr::new("-o"),
                object.as_os_str(),
            ],
        );
        assert!(out.success(), "{}", out.stderr_text());
        object
    });
    if theirs.is_none() {
        let _ = tool("llvm-mc");
    }
    for readelf in readelfs() {
        let text = read(&readelf, &["-h", "-S", "-s", "-r", "-W"], &ours);
        let lines: Vec<String> =
            text.lines().map(|l| l.split_whitespace().collect::<Vec<_>>().join(" ")).collect();
        for line in EXPECTED_HEADER {
            assert!(lines.iter().any(|l| l == line), "{}: no `{line}`:\n{text}", readelf.display());
        }
        let relocs: Vec<String> = lines
            .iter()
            .filter(|l| l.starts_with("Relocation section") || l.contains(" R_386_"))
            .map(|l| l.split(" at offset").next().unwrap().to_owned())
            .collect();
        assert_eq!(relocs, EXPECTED_RELOCS, "{}", readelf.display());
        let symbols = block(&text, "Symbol table '.symtab'");
        assert_eq!(&symbols[2..], EXPECTED_SYMBOLS, "{}:\n{text}", readelf.display());
        let sections = block(&text, "Section Headers:");
        let row = |name: &str| {
            sections.iter().find(|l| l.split(' ').nth(2) == Some(name)).cloned().unwrap_or_default()
        };
        for (name, kind, rest) in [
            (".text", "PROGBITS", "000012 00 AX 0 0 4"),
            (".data", "PROGBITS", "00000c 00 WA 0 0 4"),
            (".rel.text", "REL", "000010 08 I"),
            (".rel.debug_info", "REL", "000008 08 I"),
            (".symtab", "SYMTAB", "000050 10"),
        ] {
            let row = row(name);
            assert!(row.contains(kind) && row.contains(rest), "{name}: `{row}`");
        }
        let Some(theirs) = &theirs else { continue };
        // Field by field against LLVM's object: the header but its
        // offsets and counts, every relocation, every symbol, and the
        // bytes of each section (the addends in them).
        let llvm = read(&readelf, &["-h", "-s", "-r", "-W"], theirs);
        let keep = |text: &str| -> Vec<String> {
            text.lines()
                // hod orders its sections differently (`spec/object.md`
                // "Section header order").
                .filter(|l| {
                    !l.to_lowercase().contains("section header")
                        && !l.contains("Relocation section")
                })
                .filter(|l| !l.contains("bytes into file") && !l.trim().is_empty())
                .map(|l| l.split_whitespace().collect::<Vec<_>>().join(" "))
                .map(|l| {
                    // Symbols name their section by index, which differs.
                    let mut f: Vec<&str> = l.split(' ').collect();
                    if f.len() == 8 && f[0].ends_with(':') && f[6] != "UND" {
                        f[6] = "#";
                    }
                    f.join(" ")
                })
                .collect()
        };
        let ours_text = read(&readelf, &["-h", "-s", "-r", "-W"], &ours);
        assert_eq!(keep(&ours_text), keep(&llvm), "{}", readelf.display());
    }
    if let (Some(objdump), Some(theirs)) = (find_tool("llvm-objdump"), &theirs) {
        let dump = |file: &Path| {
            let text = read(
                &objdump,
                &["-s", "-j", ".text", "-j", ".text.helper", "-j", ".data", "-j", ".debug_info"],
                file,
            );
            text.lines().skip(2).map(str::to_owned).collect::<Vec<_>>()
        };
        assert_eq!(dump(&ours), dump(theirs));
    }
}

/// Every named kind is printed by its name by both `llvm-readelf`s (the
/// numbers of design 0008 section 3.2).
#[test]
fn readelf_names_every_i386_relocation_kind() {
    let scratch = Scratch::new("obj-i386-names");
    let mut obj = Object::for_machine(Machine::I386);
    let text = obj.add_section(b".text", SectionKind::Text, 4).unwrap();
    obj.append_zeros(text, 4 * I386Reloc::NAMED.len() as u64);
    let name = string(&mut obj, "symbol");
    let symbol = obj.add_symbol(Symbol::undefined(name));
    for (index, &kind) in I386Reloc::NAMED.iter().enumerate() {
        obj.add_relocation(text, 4 * index as u64, kind, symbol, 0);
    }
    let file = scratch.write("names.o", bytes(&obj));
    for readelf in readelfs() {
        let text = read(&readelf, &["-r", "-W"], &file);
        let printed: Vec<&str> = text
            .lines()
            .filter(|l| l.split_whitespace().nth(2).is_some_and(|k| k.starts_with("R_386_")))
            .map(|l| l.split_whitespace().nth(2).unwrap())
            .collect();
        let names: Vec<&str> = I386Reloc::NAMED.iter().map(|k| k.elf_name().unwrap()).collect();
        assert_eq!(printed, names, "{}", readelf.display());
    }
}

/// Groups, `SHF_LINK_ORDER`, TLS, common and weak symbols and more than
/// 65 279 sections (`.symtab_shndx`, the escaped header counts) in ELF32,
/// read by both `llvm-readelf`s without a complaint.
#[test]
fn readelf_reads_groups_and_70000_sections() {
    let scratch = Scratch::new("obj-i386-many");
    let mut obj = Object::for_machine(Machine::I386);
    for index in 0..70_000u32 {
        let name = format!(".text.f{index}");
        let section = obj.add_section(name.as_bytes(), SectionKind::Text, 1).unwrap();
        obj.append(section, &[0xc3]);
        define(&mut obj, &name[6..], SymbolKind::Func, section, 0, 1);
    }
    let comdat = obj.add_section(b".text.inline", SectionKind::Text, 1).unwrap();
    obj.append(comdat, &[0xe8, 0, 0, 0, 0, 0xc3]);
    let inline = define(&mut obj, "inline", SymbolKind::Func, comdat, 0, 6);
    obj.symbol_mut(inline).binding = Binding::Weak;
    let group = obj.add_group(inline, GroupKind::Comdat);
    obj.add_group_member(group, comdat).unwrap();
    let callee = string(&mut obj, "f69999");
    let _ = callee;
    let target = SymbolId::from_index(69_999);
    obj.add_relocation(comdat, 1, I386Reloc::Plt32, target, -4);
    let tls = obj.add_section(b".tbss", SectionKind::TlsBss, 4).unwrap();
    obj.append_zeros(tls, 4);
    define(&mut obj, "counter", SymbolKind::Tls, tls, 0, 4);
    let linked = obj.add_section(b".data.linked", SectionKind::Data, 4).unwrap();
    obj.append_zeros(linked, 4);
    obj.set_linked_section(linked, comdat);
    let common = string(&mut obj, "shared");
    obj.add_symbol(
        Symbol { place: SymbolPlace::Common, value: 4, size: 8, ..Symbol::undefined(common) }
            .with_binding(Binding::Global),
    );
    let file = scratch.write("many.o", bytes(&obj));
    for readelf in readelfs() {
        let text = read(&readelf, &["--all", "-g", "-W"], &file);
        let lines: Vec<String> =
            text.lines().map(|l| l.split_whitespace().collect::<Vec<_>>().join(" ")).collect();
        let has = |want: &str| lines.iter().any(|l| l.contains(want));
        assert!(has("Number of section headers: 0 ("), "{}", readelf.display());
        assert!(has("Section header string table index: 65535 ("), "{}", readelf.display());
        assert!(has(".symtab_shndx SYMTAB SECTION INDICES"), "{}", readelf.display());
        assert!(
            has("COMDAT group section [ 1] `.group' [inline] contains 2 sections:"),
            "{}",
            readelf.display()
        );
        assert!(has("R_386_PLT32 00000000 f69999"), "{}", readelf.display());
        // Section 69 999, after the null section and the group.
        assert!(has("FUNC GLOBAL DEFAULT 70001 f69999"), "{}", readelf.display());
        assert!(has("4 TLS GLOBAL DEFAULT"), "{}", readelf.display());
        assert!(has("8 NOTYPE GLOBAL DEFAULT COM shared"), "{}", readelf.display());
    }
}

/// The linkers of the i386 matrix (design 0008: GNU ld, lld, mold; wild
/// does not link i386): `(name, program, arguments before the object)`.
fn linkers() -> Vec<(&'static str, PathBuf, Vec<&'static str>)> {
    let mut found = Vec::new();
    let gnu = find_tool("i686-linux-gnu-ld.bfd").or_else(|| find_tool("i686-linux-gnu-ld"));
    let candidates = [
        ("GNU ld", gnu, vec![]),
        ("lld", find_tool("ld.lld"), vec!["-m", "elf_i386"]),
        ("mold", find_tool("mold"), vec!["-m", "elf_i386"]),
    ];
    for (name, program, args) in candidates {
        if let Some(program) = program {
            found.push((name, program, args));
        } else {
            assert!(!tools_required(), "{name} for i386 not found (HOD_REQUIRE_TOOLS=1)");
            eprintln!("note: {name} for i386 not found; skipping it");
        }
    }
    found
}

/// The program, linked statically by each linker, exits with 42 under
/// `qemu-i386` (or natively on an x86 host that runs 32-bit programs).
#[test]
fn program_links_and_runs() {
    let scratch = Scratch::new("obj-i386-run");
    let object = scratch.write("program.o", bytes(&program()));
    let emulator = tool("qemu-i386");
    for (name, linker, args) in linkers() {
        let output = scratch.join(format!("program-{}", name.replace(' ', "-")));
        let out = run(
            &linker,
            args.iter()
                .map(OsStr::new)
                .chain([OsStr::new("-static"), OsStr::new("-e"), OsStr::new("_start")])
                .chain([OsStr::new("-o"), output.as_os_str(), object.as_os_str()]),
        );
        assert!(out.success(), "{name}: {}", out.stderr_text());
        let Some(emulator) = &emulator else { continue };
        let ran = run(emulator, [output.as_os_str()]);
        assert_eq!(ran.status.and_then(|s| s.code()), Some(42), "{name}: {}", ran.stderr_text());
    }
}
