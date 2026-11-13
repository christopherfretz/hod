//! `spec/eh.md` §10 (WP-150): `.debug_frame` against `llc`. Each case is
//! a module with debug information whose functions are all `nounwind`
//! without `uwtable`; `llc` (19 and 22, `-O0`, PIC, both targets) writes
//! their CFI to `.debug_frame` (`CFISection::Debug`). Its entries are
//! read back with `llvm-dwarfdump --debug-frame`, turned into
//! `hod_eh`'s input and written again by `write_debug_frame`: the section
//! (bytes, type, flags, alignment) and its relocations must be the ones
//! `llc` wrote.

mod common;

use common::{Elf, Rela, compile, hex, llc, parse_dump, rows_of};
use hod_eh::{Fde, Row, write_debug_frame};
use hod_obj::{Arch, Object, SectionKind, StrId, Symbol, SymbolId};
use hod_testutil::{Scratch, run, tool};

const MODULE: &str = r#"
declare void @ext(ptr)
define i32 @leaf(i32 %a) nounwind { %b = add i32 %a, 1
  ret i32 %b }
define void @non_leaf() nounwind { call void @ext(ptr null)
  ret void }
define void @saves() nounwind {
  call void asm sideeffect "", "{CLOBBER}"()
  call void @ext(ptr null)
  ret void
}
define void @big() nounwind { %a = alloca [70000 x i8]
  call void @ext(ptr %a)
  ret void }
define void @frame_pointer() nounwind "frame-pointer"="all" { call void @ext(ptr null)
  ret void }
!llvm.dbg.cu = !{!0}
!llvm.module.flags = !{!2}
!0 = distinct !DICompileUnit(language: DW_LANG_C11, file: !1, emissionKind: FullDebug)
!1 = !DIFile(filename: "a.c", directory: "/w")
!2 = !{i32 2, !"Debug Info Version", i32 3}
"#;

fn targets() -> [(Arch, &'static str, &'static str); 2] {
    [
        (Arch::AArch64, "aarch64-unknown-linux-gnu", "~{x19},~{x20},~{x28},~{d8},~{d15}"),
        (Arch::X86_64, "x86_64-unknown-linux-gnu", "~{rbx},~{r12},~{r15}"),
    ]
}

/// `llc`'s `.debug_frame` entries as the `.eh_frame` dump's parser reads
/// them.
fn debug_frame_entries(scratch: &Scratch, name: &str, object: &[u8]) -> Option<Vec<common::Entry>> {
    let dump = tool("llvm-dwarfdump")?;
    let path = scratch.write(name, object);
    let out = run(&dump, [std::ffi::OsStr::new("--debug-frame"), path.as_os_str()]);
    assert!(out.success(), "llvm-dwarfdump {name}: {}", out.stderr_text());
    let text = out.stdout_text();
    let text = text.split(".eh_frame contents:").next().unwrap_or_default();
    Some(parse_dump(&text.replace(".debug_frame contents:", ".eh_frame contents:")))
}

fn oracle(major: u32) {
    let Some(llc) = llc(major) else { return };
    let scratch = Scratch::new(&format!("debug-frame-llc{major}"));
    for (arch, triple, clobber) in targets() {
        let label = format!("{triple}-{major}");
        let ir = MODULE.replace("{CLOBBER}", clobber);
        let object = compile(&llc, &scratch, &label, &ir, triple);
        let Some(entries) = debug_frame_entries(&scratch, &format!("{label}.o"), &object) else {
            return;
        };
        let elf = Elf::parse(object.clone());
        let df = elf.section(".debug_frame").expect(".debug_frame");
        let rela = elf.rela_of(df).expect(".rela.debug_frame");
        let relas = elf.relas(rela);
        let fdes: Vec<_> = entries.iter().filter(|e| !e.is_cie).collect();
        assert_eq!(fdes.len(), 5, "{label}: one FDE per function");
        assert_eq!(entries.iter().filter(|e| e.is_cie).count(), 1, "{label}");
        // No function needs an unwind entry: no `.eh_frame` (a module
        // with one has every FDE there, `hod_cg`'s choice).
        assert!(elf.section(".eh_frame").is_none(), "{label}");

        // Placeholders for llc's symbols 1..n, so that indices match.
        let mut obj = Object::new(arch);
        for _ in 1..elf.symbols.len() {
            obj.add_symbol(Symbol::undefined(StrId::EMPTY));
        }
        let rows: Vec<(Vec<Row>, bool)> = fdes.iter().map(|e| rows_of(arch, e)).collect();
        let inputs: Vec<Fde<'_>> = fdes
            .iter()
            .zip(&rows)
            .enumerate()
            .map(|(i, (e, (rows, state)))| {
                assert!(!state, "{label}: llc remembered state");
                let pc = relas.iter().find(|r| r.offset == e.offset + 8).expect("pc relocation");
                Fde {
                    order: i as u32,
                    start: SymbolId::from_index(pc.sym - 1),
                    start_addend: pc.addend,
                    size: (e.pc_end - e.pc_begin) as u32,
                    rows,
                    personality: None,
                    lsda: None,
                }
            })
            .collect();
        let section = write_debug_frame(&mut obj, &inputs).expect("hod_eh").expect("a section");
        let llc_header = &elf.sections[df];
        assert_eq!(obj.section_kind(section), SectionKind::Debug, "{label}");
        assert_eq!((llc_header.ty, llc_header.flags), (1, 0), "{label}: PROGBITS, no flags");
        assert_eq!(obj.section_align(section), llc_header.align, "{label}: alignment");
        assert_eq!(hex(obj.data(section)), hex(elf.bytes(df)), "{label}: section bytes");
        // The CIE pointers name the section's own symbol: llc's is its
        // section symbol in its table, hod's a new one.
        let own = obj.section_symbol(section).index() + 1;
        let llc_own = elf
            .symbols
            .iter()
            .position(|s| s.info & 0xf == 3 && usize::from(s.shndx) == df)
            .expect("llc's section symbol of .debug_frame") as u32;
        let new: Vec<Rela> = obj
            .relocations(section)
            .map(|r| {
                let sym = r.symbol.index() + 1;
                Rela {
                    offset: r.offset,
                    sym: if sym == own { llc_own } else { sym },
                    ty: r.kind.elf_type(),
                    addend: r.addend,
                }
            })
            .collect();
        assert_eq!(new, relas, "{label}: relocations");
        eprintln!("{label}: {} FDEs byte-identical", inputs.len());
    }
}

/// §10 with LLVM 19.
#[test]
fn llc19_debug_frame_is_written_as_llc_writes_it() {
    oracle(19);
}

/// §10 with LLVM 22.
#[test]
fn llc22_debug_frame_is_written_as_llc_writes_it() {
    oracle(22);
}

/// No FDE, no section; a personality or an LSDA is refused.
#[test]
fn empty_and_refused() {
    let mut obj = Object::new(Arch::X86_64);
    assert_eq!(write_debug_frame(&mut obj, &[]), Ok(None));
    let text = obj.add_section(b".text", SectionKind::Text, 16).unwrap();
    obj.append(text, &[0xc3]);
    let start = obj.section_symbol(text);
    let fde = Fde {
        order: 3,
        start,
        start_addend: 0,
        size: 1,
        rows: &[],
        personality: Some(start),
        lsda: None,
    };
    assert_eq!(
        write_debug_frame(&mut obj, &[fde]),
        Err(hod_eh::Error::PersonalityInDebugFrame { order: 3 })
    );
    // An FDE without rows: the CIE (24 bytes) and a 24-byte FDE.
    let plain = Fde { personality: None, ..fde };
    let section = write_debug_frame(&mut obj, &[plain]).unwrap().unwrap();
    assert_eq!(obj.data(section).len(), 24 + 24);
    assert_eq!(obj.relocations(section).count(), 2);
}
