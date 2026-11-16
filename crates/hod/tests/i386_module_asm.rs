//! WP-307: module-level assembly (`module asm`) for i686 (`spec/asm.md`
//! §23).
//!
//! - Objects: each text of `tests/data/i386/module_asm/objects` (`.s`,
//!   with the IR of the `.ll` of the same name after it, if any) as a
//!   module compiled by `llc` 19 and 22 and by hod, static and PIC: the
//!   two objects have the same sections (type, flags, alignment, bytes),
//!   the same relocations (offset, kind, symbol or section, the addend
//!   in place) and the same symbols.
//! - Runs: `static.ll` (non-PIC) as a program, `pic.ll` as a PIE and as a
//!   shared library, each behind `driver.c`, by llc 19 and 22, hod and
//!   hod under `Stress::ALL`, linked by every linker present (GNU ld, lld,
//!   mold), run under `qemu-i386`: every output is llc's.
//! - Refusals: what the 32-bit module assembler does not read is refused
//!   by its `asm:*` name.
//!
//! A missing tool is skipped with a note, unless `HOD_REQUIRE_TOOLS=1`.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;

use hod::{Options, OutputKind, RelocModel};
use hod_testutil::target::Target;
use hod_testutil::{LLVM_MAJORS, Scratch, linkers, tools_required};

mod i386_support;
use i386_support::{TRIPLE, back_end_object, checked, llc_object, llvm_tool, run, tool};

fn data(path: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/i386/module_asm").join(path)
}

/// `module asm` lines of `text`, one per line.
fn asm(text: &str) -> String {
    let mut out = String::new();
    for line in text.lines() {
        let escaped = line.replace('\\', "\\5C").replace('"', "\\22");
        let _ = writeln!(out, "module asm \"{escaped}\"");
    }
    out
}

fn module(assembly: &str, ir: &str) -> String {
    format!("target triple = \"{TRIPLE}\"\n{}{ir}", asm(assembly))
}

/// hod's object of `text` under `model`, by the facade.
fn hod_object(text: &[u8], model: RelocModel) -> Result<Vec<u8>, String> {
    let options = Options {
        output_kind: OutputKind::Object,
        triple: Some(TRIPLE.into()),
        relocation_model: Some(model),
        ..Options::default()
    };
    hod::compile(text, &options).map_err(|e| e.to_string())
}

fn u16_at(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([b[o], b[o + 1]])
}

fn u32_at(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}

/// One section of an ELF32 object.
struct Section {
    name: String,
    ty: u32,
    flags: u32,
    offset: usize,
    size: usize,
    link: u32,
    info: u32,
    align: u32,
    entsize: u32,
}

/// The sections, symbols and relocations of an ELF32 relocatable
/// object, described as sorted lines (section indices replaced by names,
/// a relocation's addend read from its field).
struct Described {
    sections: Vec<String>,
    relocations: Vec<String>,
    symbols: Vec<String>,
}

/// Sections both compilers may write differently and that the text does
/// not write.
fn ignored(name: &str) -> bool {
    matches!(name, "" | ".note.GNU-stack" | ".llvm_addrsig" | ".symtab" | ".strtab" | ".shstrtab")
        || name.starts_with(".rel")
}

fn describe(bytes: &[u8]) -> Described {
    assert_eq!(&bytes[..5], b"\x7fELF\x01", "an ELF32 object");
    let shoff = u32_at(bytes, 0x20) as usize;
    let shnum = usize::from(u16_at(bytes, 0x30));
    let shstrndx = usize::from(u16_at(bytes, 0x32));
    let header = |i: usize| shoff + 40 * i;
    let raw: Vec<Section> = (0..shnum)
        .map(|i| {
            let h = header(i);
            Section {
                name: String::new(),
                ty: u32_at(bytes, h + 4),
                flags: u32_at(bytes, h + 8),
                offset: u32_at(bytes, h + 16) as usize,
                size: u32_at(bytes, h + 20) as usize,
                link: u32_at(bytes, h + 24),
                info: u32_at(bytes, h + 28),
                align: u32_at(bytes, h + 32),
                entsize: u32_at(bytes, h + 36),
            }
        })
        .collect();
    let cstr = |at: usize| {
        let end = bytes[at..].iter().position(|&b| b == 0).unwrap() + at;
        String::from_utf8_lossy(&bytes[at..end]).into_owned()
    };
    let strtab = raw[shstrndx].offset;
    let mut sections = raw;
    for (i, s) in sections.iter_mut().enumerate() {
        s.name = cstr(strtab + u32_at(bytes, header(i)) as usize);
    }
    let data = |s: &Section| -> &[u8] {
        if s.ty == 8 { &[] } else { &bytes[s.offset..s.offset + s.size] }
    };
    let mut out = Described { sections: Vec::new(), relocations: Vec::new(), symbols: Vec::new() };
    for s in &sections {
        if ignored(&s.name) {
            continue;
        }
        let hex = data(s).iter().fold(String::new(), |mut hex, b| {
            let _ = write!(hex, "{b:02x}");
            hex
        });
        out.sections.push(format!(
            "section {} type {} flags {:#x} align {} entsize {} size {} bytes {hex}",
            s.name, s.ty, s.flags, s.align, s.entsize, s.size
        ));
    }
    // Symbols: (name, the name a relocation shows).
    let symtab = sections.iter().find(|s| s.ty == 2).expect("a symbol table");
    let names = sections[symtab.link as usize].offset;
    let mut shown = Vec::new();
    for i in 0..symtab.size / 16 {
        let o = symtab.offset + 16 * i;
        let name = cstr(names + u32_at(bytes, o) as usize);
        let (value, size) = (u32_at(bytes, o + 4), u32_at(bytes, o + 8));
        let (info, other, shndx) = (bytes[o + 12], bytes[o + 13], u16_at(bytes, o + 14));
        let place = match shndx {
            0 => "UND".to_owned(),
            0xfff1 => "ABS".to_owned(),
            n => sections[usize::from(n)].name.clone(),
        };
        let ty = info & 0xf;
        shown.push(if ty == 3 { place.clone() } else { name.clone() });
        if name.is_empty() || ty == 3 || ty == 4 {
            continue;
        }
        out.symbols.push(format!(
            "symbol {name} bind {} type {ty} vis {} in {place} value {value:#x} size {size}",
            info >> 4,
            other & 3
        ));
    }
    for s in sections.iter().filter(|s| s.ty == 9) {
        let target = &sections[s.info as usize];
        let bytes_of = data(target);
        for i in 0..s.size / 8 {
            let o = s.offset + 8 * i;
            let (offset, info) = (u32_at(bytes, o) as usize, u32_at(bytes, o + 4));
            let kind = info & 0xff;
            let addend = match kind {
                0 => 0,
                20 => i64::from(u16_at(bytes_of, offset) as i16),
                _ => i64::from(u32_at(bytes_of, offset) as i32),
            };
            out.relocations.push(format!(
                "reloc {} {offset:#x} type {kind} against {} addend {addend}",
                target.name,
                shown[(info >> 8) as usize]
            ));
        }
    }
    out.sections.sort();
    out.relocations.sort();
    out.symbols.sort();
    out
}

/// The lines of `ours` not in `theirs` and the reverse.
fn differences(what: &str, ours: &[String], theirs: &[String], report: &mut String) {
    for line in ours.iter().filter(|l| !theirs.contains(l)) {
        let _ = writeln!(report, "  {what} hod: {line}");
    }
    for line in theirs.iter().filter(|l| !ours.contains(l)) {
        let _ = writeln!(report, "  {what} llc: {line}");
    }
}

/// The object texts: `(name, assembly, IR)`.
fn object_texts() -> Vec<(String, String, String)> {
    let mut texts = Vec::new();
    let mut entries: Vec<PathBuf> = std::fs::read_dir(data("objects"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "s"))
        .collect();
    entries.sort();
    for path in entries {
        let name = path.file_stem().unwrap().to_string_lossy().into_owned();
        let assembly = std::fs::read_to_string(&path).unwrap();
        let ir = std::fs::read_to_string(path.with_extension("ll")).unwrap_or_default();
        texts.push((name, assembly, ir));
    }
    texts
}

// spec/asm.md §23: what hod writes for i686 module assembly is llc's:
// section bytes, relocations with their addends in place, symbols.
#[test]
fn module_assembly_objects_are_llc_s() {
    let scratch = Scratch::new("i386-module-asm-objects");
    let texts = object_texts();
    assert!(texts.len() >= 6, "the object texts");
    let (mut compared, mut sections, mut relocations) = (0, 0, 0);
    let mut failures = Vec::new();
    for (name, assembly, ir) in &texts {
        let text = module(assembly, ir);
        let ll = scratch.write(&format!("{name}.ll"), &text);
        for (model, flag) in [(RelocModel::Static, "static"), (RelocModel::Pic, "pic")] {
            let ours = match hod_object(text.as_bytes(), model) {
                Ok(o) => describe(&o),
                Err(e) => {
                    failures.push(format!("{name} ({flag}): hod: {e}"));
                    continue;
                }
            };
            for major in LLVM_MAJORS {
                let Some(llc) = llvm_tool(major, "llc") else { continue };
                let flags = [format!("-relocation-model={flag}")];
                let flags: Vec<&str> = flags.iter().map(String::as_str).collect();
                let o = llc_object(&scratch, &llc, &ll, &flags, &format!("{name}-{flag}-{major}"));
                let theirs = describe(&std::fs::read(o).unwrap());
                let mut report = String::new();
                differences("section", &ours.sections, &theirs.sections, &mut report);
                differences("reloc", &ours.relocations, &theirs.relocations, &mut report);
                differences("symbol", &ours.symbols, &theirs.symbols, &mut report);
                if !report.is_empty() {
                    failures.push(format!("{name} ({flag}, LLVM {major}):\n{report}"));
                }
                compared += 1;
                sections += theirs.sections.len();
                relocations += theirs.relocations.len();
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    eprintln!(
        "i386 module asm: {} texts, {compared} objects, {sections} sections and \
         {relocations} relocations equal llc's",
        texts.len()
    );
}

/// `(module assembly, IR after it, the name hod refuses it by)`.
const REFUSED: &[(&str, &str, &str)] = &[
    // As on x86-64 (hod_cg/tests/emit_module_asm.rs `X64_REFUSED`).
    (".cfi_startproc", "", "asm:directive/cfi_startproc"),
    (".hword 1", "", "asm:directive/hword"),
    (".symver u, u@@V1\n.text\ncall u", "", "asm:symbol"),
    (".text\nfoo %eax", "", "asm:mnemonic/foo"),
    (".text\nD:\nret", "define void @D() nounwind {\n  ret void\n}\n", "asm:symbol"),
    (".text\njmp *%eax", "", "asm:operand/form"),
    (".section mine,\"ax\"\nret", "", "asm:section-flags/exec"),
    (".section mine,\"awT\"", "", "asm:section-flags/tls"),
    (".section .note.x,\"a\"", "", "asm:section-name"),
    (".data\n.byte sym", "", "asm:operand/symbol"),
    (".data\n.long a - b\na:\nb:", "", "asm:operand/expression"),
    (".text\nmovl .Lx@GOT(%ebx), %eax\n.Lx:", "", "asm:operand/symbol"),
    (".text\nl:\nmovl %gs:l@ntpoff, %eax", "", "asm:symbol"),
    (".text\nmovl x(%rip), %eax", "", "asm:operand/register"),
    // 32-bit mode.
    (".text\npushq $1", "", "asm:mnemonic/pushq"),
    (".text\npushq %rax", "", "asm:operand/register"),
    (".text\nmovq %rax, %rbx", "", "asm:operand/register"),
    (".text\nsyscall", "", "asm:mnemonic/syscall"),
    (".text\nmovl %r8d, %eax", "", "asm:operand/register"),
    // A `.quad` of a symbol: an error to llvm-mc for i686.
    (".data\n.quad sym", "", "asm:operand/symbol"),
    // A symbol immediate of a 16- or 8-bit field, two symbol fields.
    (".text\nmovw $sym, %ax", "", "asm:operand/symbol"),
    (".text\nmovb $sym, %al", "", "asm:operand/symbol"),
    (".text\nmovl $sym, sym2", "", "asm:operand/symbol"),
    // Specifiers that are not i686's or not of the field.
    (".text\nmovl sym@PLT, %eax", "", "asm:operand/specifier"),
    (".text\nmovl sym@GOTPCREL, %eax", "", "asm:operand/specifier"),
    (".text\nmovl $sym@PLT, %eax", "", "asm:operand/specifier"),
    (".text\nmovl $_GLOBAL_OFFSET_TABLE_@GOTOFF, %eax", "", "asm:operand/specifier"),
    (".text\ncall sym@GOTOFF", "", "asm:operand/specifier"),
    (".data\n.long sym@GOTOFF", "", "asm:operand/syntax"),
    // A difference of labels in an immediate (`.-1b`).
    (".text\n1:\naddl $_GLOBAL_OFFSET_TABLE_+(.-1b), %ebx", "", "asm:operand/expression"),
];

// spec/asm.md §23: what the 32-bit module assembler does not read is
// refused by name.
#[test]
fn unread_module_assembly_is_refused_by_name() {
    let mut failures = Vec::new();
    for &(assembly, ir, expected) in REFUSED {
        match hod_object(module(assembly, ir).as_bytes(), RelocModel::Pic) {
            Err(e) if e.contains(&format!("'{expected}'")) => {}
            other => failures.push(format!("{assembly:?}: expected {expected}, got {other:?}")),
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The tools the run test needs.
struct Tools {
    gcc: PathBuf,
    qemu: PathBuf,
    linkers: Vec<(&'static str, PathBuf)>,
}

fn tools() -> Option<Tools> {
    let target = Target::I686;
    let (Some(gcc), Some(qemu)) = (tool(&target.c_driver()), tool(&target.emulator())) else {
        return None;
    };
    let linkers = linkers::present(target);
    for name in ["bfd", "lld", "mold"] {
        assert!(
            !tools_required() || linkers.iter().any(|(n, _)| *n == name),
            "linker {name} missing (HOD_REQUIRE_TOOLS=1)"
        );
    }
    Some(Tools { gcc, qemu, linkers })
}

impl Tools {
    fn link(
        &self,
        scratch: &Scratch,
        linker: &(&str, PathBuf),
        flags: &[&str],
        inputs: &[&Path],
        out: &Path,
    ) {
        let mut command = Command::new(&self.gcc);
        let driver: Vec<OsString> = linkers::driver_flags(scratch.path(), linker.0, &linker.1);
        command.args(driver).args(flags).args(inputs).arg("-o").arg(out);
        checked(&format!("{} link of {}", linker.0, out.display()), &mut command);
    }

    /// Links `object` with the driver as `shape` says (a non-PIE
    /// executable, a PIE, or a shared library that a PIE uses) and
    /// returns the program, named `tag`.
    fn link_shape(
        &self,
        scratch: &Scratch,
        linker: &(&str, PathBuf),
        shape: &str,
        driver: &Path,
        object: &Path,
        tag: &str,
    ) -> PathBuf {
        let exe = scratch.join(tag);
        match shape {
            "exe" => self.link(scratch, linker, &["-no-pie"], &[driver, object], &exe),
            "pie" => self.link(scratch, linker, &["-pie"], &[driver, object], &exe),
            _ => {
                let lib = scratch.join(format!("lib{tag}.so"));
                self.link(scratch, linker, &["-shared"], &[object], &lib);
                self.link(scratch, linker, &["-pie"], &[driver, &lib], &exe);
            }
        }
        exe
    }
}

// spec/asm.md §23: programs whose module assembly and IR use each
// other's functions, data and thread-locals run as llc's, non-PIC and
// PIC, as programs and from a shared library, by every linker.
#[test]
fn module_assembly_programs_run_as_llc_s() {
    let programs = [
        ("static.ll", RelocModel::Static),
        ("pic.ll", RelocModel::Pic),
        ("tls32.ll", RelocModel::Pic),
    ];
    // hod's objects, plain and stressed: compiling is the claim even
    // without the tools to run them.
    let mut objects: BTreeMap<&str, [Vec<u8>; 2]> = BTreeMap::new();
    for (program, model) in programs {
        let text = std::fs::read(data(program)).unwrap();
        let plain = hod_object(&text, model).unwrap_or_else(|e| panic!("{program}: {e}"));
        let options = hod_cg::Options {
            reloc_model: if model == RelocModel::Pic {
                hod_cg::RelocModel::Pic
            } else {
                hod_cg::RelocModel::Static
            },
            stress: hod_cg::Stress::ALL,
            ..hod_cg::Options::default()
        };
        objects.insert(program, [plain, back_end_object(&text, &options)]);
    }
    let Some(tools) = tools() else { return };
    let scratch = Scratch::new("i386-module-asm-run");
    let driver = scratch.join("driver.o");
    let mut command = Command::new(&tools.gcc);
    command.args(["-c", "-fPIE"]).arg(data("driver.c")).arg("-o").arg(&driver);
    checked("driver", &mut command);
    let (mut programs_built, mut runs) = (0, 0);
    for major in LLVM_MAJORS {
        let Some(llc) = llvm_tool(major, "llc") else { continue };
        for (program, model) in programs {
            let stem = program.trim_end_matches(".ll");
            let flag = if model == RelocModel::Pic { "pic" } else { "static" };
            let label = format!("{stem}-{major}");
            let llc_o = llc_object(
                &scratch,
                &llc,
                &data(program),
                &[&format!("-relocation-model={flag}")],
                &format!("{label}-llc"),
            );
            let [plain, stressed] = &objects[program];
            let hod_o = scratch.write(&format!("{label}-hod.o"), plain);
            let stress_o = scratch.write(&format!("{label}-stress.o"), stressed);
            let built = [("llc", llc_o), ("hod", hod_o), ("stress", stress_o)];
            let shapes: &[&str] = match program {
                "static.ll" => &["exe"],
                // The thread-pointer offsets of an executable.
                "tls32.ll" => &["pie"],
                _ => &["pie", "shared"],
            };
            for linker in &tools.linkers {
                // mold links neither R_386_TLS_LE_32 nor R_386_TLS_IE_32,
                // lld not the second ("unknown relocation"), llc's
                // objects as hod's.
                if program == "tls32.ll" && linker.0 != "bfd" {
                    continue;
                }
                for shape in shapes {
                    let mut results = Vec::new();
                    for (who, object) in &built {
                        let tag = format!("{label}-{shape}-{who}-{}", linker.0);
                        let exe = tools.link_shape(&scratch, linker, shape, &driver, object, &tag);
                        results.push((*who, run(&tools.qemu, &exe)));
                        programs_built += 1;
                    }
                    let (_, reference) = &results[0];
                    assert_eq!(
                        reference.0,
                        Some(0),
                        "{label} {shape} ({}): llc's status",
                        linker.0
                    );
                    assert!(!reference.1.is_empty(), "{label} {shape}: llc's output");
                    for (who, result) in &results[1..] {
                        assert_eq!(
                            reference,
                            result,
                            "{label} {shape} ({}): {who}'s run differs from llc's\nllc: {}\n{who}: {}",
                            linker.0,
                            String::from_utf8_lossy(&reference.1),
                            String::from_utf8_lossy(&result.1)
                        );
                        runs += 1;
                    }
                }
            }
        }
    }
    eprintln!(
        "i386 module asm: {programs_built} programs linked and run, {runs} hod runs equal llc's"
    );
}
