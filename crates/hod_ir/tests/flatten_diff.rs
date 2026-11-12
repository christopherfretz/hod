//! Initializer flattening differential tests (`spec/ir.md` §4.3): the bytes
//! and fixups `ConstPool::flatten` produces against the `.data` section and
//! relocations `llc` emits for the same global variables, on both targets,
//! with LLVM 22 and LLVM 19.
//!
//! Each configuration compiles one module: two variables per type of the
//! shared type corpus (one of plain data, one whose pointers may be symbol
//! addresses), then the hand-written address forms of §4.2. That covers
//! padding, nested aggregates, wide integers, every float kind, vectors
//! (packed ones too), byte strings and pointers with addends. One `llc`, one
//! `llvm-objdump -s -j .data` and one `llvm-readelf -S -r -s` run per
//! configuration.
//!
//! Checked for every variable: its size (the alloc size of its type); that
//! it is in `.bss` exactly when `ConstPool::is_zero_or_undef` says so; and,
//! in `.data`, every byte of its image and every relocation in it.
//!
//! Not covered at object level: a `dso_local_equivalent` on its own
//! (x86-64 has no relocation for `.quad f@PLT`), one-byte fixups (AArch64
//! has no such relocation) and `blockaddress` (block labels are private to
//! `llc`). `fold_diff.rs` covers the first two as assembly.

mod common;

use std::fmt::Write as _;

use common::consts::{Ctx, PRELUDE, address_cases, padding_cases};
use common::{CONFIGS, Config, Object, Place, Reloc, type_corpus};
use hod_ir::{Arch, ConstId, Fixup, FixupTarget};
use hod_testutil::Scratch;

/// Types per configuration; each gets two variables.
const TYPE_COUNT: usize = 300;

/// A variable of the module: its name, its definition (for messages), and
/// its initializer in the constant pool.
struct Global {
    name: String,
    definition: String,
    value: ConstId,
    /// Whether its relocation addends must equal `llc`'s exactly, or only
    /// modulo the width of the field.
    exact_addend: bool,
}

/// `addend` reduced to a signed number of `size` bytes.
fn reduced(addend: i64, size: u8) -> i64 {
    let shift = 64 - 8 * u32::from(size);
    (addend << shift) >> shift
}

/// The width in bytes of the field a relocation of this type fills.
fn reloc_size(kind: &str) -> u8 {
    match kind.rsplit(|c: char| !c.is_ascii_digit()).next() {
        Some("64") => 8,
        Some("32") => 4,
        Some("16") => 2,
        Some("8") => 1,
        _ => panic!("unknown relocation type {kind}"),
    }
}

/// The relocation type of a field of `size` bytes.
fn reloc_kind(arch: Arch, size: u8, pc_relative: bool) -> String {
    let name = match (arch, pc_relative, size) {
        (Arch::X86_64, false, 8) => "R_X86_64_64",
        (Arch::X86_64, false, 4) => "R_X86_64_32",
        (Arch::X86_64, false, 2) => "R_X86_64_16",
        (Arch::X86_64, false, 1) => "R_X86_64_8",
        (Arch::X86_64, true, 8) => "R_X86_64_PC64",
        (Arch::X86_64, true, 4) => "R_X86_64_PC32",
        (Arch::X86_64, true, 2) => "R_X86_64_PC16",
        (Arch::X86_64, true, 1) => "R_X86_64_PC8",
        (Arch::Aarch64, false, 8) => "R_AARCH64_ABS64",
        (Arch::Aarch64, false, 4) => "R_AARCH64_ABS32",
        (Arch::Aarch64, false, 2) => "R_AARCH64_ABS16",
        (Arch::Aarch64, true, 8) => "R_AARCH64_PREL64",
        (Arch::Aarch64, true, 4) => "R_AARCH64_PREL32",
        (Arch::Aarch64, true, 2) => "R_AARCH64_PREL16",
        _ => panic!("no relocation for a field of {size} bytes"),
    };
    name.to_string()
}

/// What an ELF assembler makes of a fixup.
enum Resolved {
    /// A relocation.
    Reloc(Reloc),
    /// A number known at assembly time.
    Number(u64),
}

/// What the assembler makes of `fixup`, which is at offset `place` of the
/// section with index `data`.
fn resolve(
    config: &Config,
    ctx: &Ctx,
    object: &Object,
    data: u32,
    place: u64,
    fixup: &Fixup,
) -> Resolved {
    let reloc = |kind: String, symbol: hod_ir::SymId, addend: i64| {
        Resolved::Reloc(Reloc { offset: place, kind, symbol: ctx.name(symbol).to_string(), addend })
    };
    match fixup.target {
        FixupTarget::Sym(sym) => {
            reloc(reloc_kind(config.arch, fixup.size, false), sym, fixup.addend)
        }
        FixupTarget::Diff { a, b } | FixupTarget::PltDiff { a, b } => {
            let (plus, minus) = (object.symbol(ctx.name(a)), object.symbol(ctx.name(b)));
            let plt = matches!(fixup.target, FixupTarget::PltDiff { .. });
            // Two symbols of one section: their distance is known.
            if !plt && plus.place == minus.place && matches!(plus.place, Place::Section(_)) {
                let distance = plus.value.wrapping_sub(minus.value);
                return Resolved::Number(distance.wrapping_add(fixup.addend as u64));
            }
            // `a - b` where `b` is in the section of the field P is
            // `a - P + (P - b)`: PC-relative.
            assert_eq!(minus.place, Place::Section(data), "the subtracted symbol must be in .data");
            let addend = fixup.addend.wrapping_add(place.wrapping_sub(minus.value) as i64);
            let kind = if plt {
                assert_eq!(fixup.size, 4, "a PLT difference is 32 bits");
                match config.arch {
                    Arch::X86_64 => "R_X86_64_PLT32".to_string(),
                    Arch::Aarch64 => "R_AARCH64_PLT32".to_string(),
                }
            } else {
                reloc_kind(config.arch, fixup.size, true)
            };
            reloc(kind, a, addend)
        }
        FixupTarget::Plt(_) | FixupTarget::Block { .. } | FixupTarget::BlockDiff { .. } => {
            panic!("no object-level case has {:?}", fixup.target)
        }
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut text, byte| {
        let _ = write!(text, "{byte:02x}");
        text
    })
}

/// The module of one configuration, and its variables with their
/// initializers built in `ctx`.
fn module(config: &Config, ctx: &mut Ctx) -> (String, Vec<Global>, usize) {
    let mut ir = config.header();
    ir.push_str(PRELUDE);
    let mut globals = Vec::new();
    for (index, ty) in type_corpus(0x51ab_0002, TYPE_COUNT).iter().enumerate() {
        for relocs in [false, true] {
            let name = format!("{}{index}", if relocs { "r" } else { "g" });
            ctx.this = ctx.sym(&name);
            let (text, value) = ctx.random_value(ty, relocs);
            let definition = format!("@{name} = global {} {text}", ty.text());
            let _ = writeln!(ir, "{definition}");
            globals.push(Global { name, definition, value, exact_addend: true });
        }
    }
    let random_count = globals.len();
    let cases: Vec<_> = padding_cases().into_iter().chain(address_cases()).collect();
    for (index, case) in cases.iter().enumerate() {
        if !case.in_object {
            continue;
        }
        let name = format!("k{index}");
        ctx.this = ctx.sym(&name);
        let text = case.text.replace("{self}", &format!("@{name}"));
        let definition = format!("@{name} = global {} {text}", case.ty.text());
        let _ = writeln!(ir, "{definition}");
        let value = (case.build)(ctx);
        assert_eq!(ctx.consts.ty(value), case.ty.intern(&mut ctx.types), "{definition}");
        globals.push(Global { name, definition, value, exact_addend: case.exact_addend });
    }
    (ir, globals, random_count)
}

/// Compares the relocations expected from hod's fixups (each with the
/// definition it is in and whether its addend is exactly `llc`'s) with all
/// the relocations of `.data`, in offset order on both sides. The prelude's
/// own variables have none. Returns how many were compared modulo the
/// width of the field.
fn compare_relocs(
    expected_relocs: &mut [(Reloc, &str, bool)],
    object: &Object,
    failures: &mut Vec<String>,
) -> usize {
    expected_relocs.sort_by_key(|(reloc, ..)| reloc.offset);
    let mut actual_relocs = object.data_relocs.clone();
    actual_relocs.sort_by_key(|reloc| reloc.offset);
    let mut actual = actual_relocs.iter().peekable();
    let mut inexact = 0;
    for (expected, definition, exact_addend) in expected_relocs.iter() {
        while actual.peek().is_some_and(|reloc| reloc.offset < expected.offset) {
            failures.push(format!("a relocation hod has no fixup for: {:?}", actual.next()));
        }
        match actual.peek() {
            Some(reloc) if reloc.offset == expected.offset => {
                let mut llvm = (*reloc).clone();
                if !exact_addend {
                    // Equal as the field's value, and different as written.
                    assert_ne!(llvm.addend, expected.addend, "{definition}: the addends agree");
                    llvm.addend = reduced(llvm.addend, reloc_size(&llvm.kind));
                    inexact += 1;
                }
                if llvm != *expected {
                    failures.push(format!("{definition}\n  hod  {expected:?}\n  LLVM {reloc:?}"));
                }
                actual.next();
            }
            _ => failures.push(format!("{definition}\n  LLVM has no relocation for {expected:?}")),
        }
    }
    failures.extend(actual.map(|reloc| format!("a relocation hod has no fixup for: {reloc:?}")));
    inexact
}

fn check(config: &Config) {
    let Some(tools) = common::tools(config.llvm) else { return };
    let mut ctx = Ctx::new(config, 0x0f1a_77e4);
    let (ir, globals, random_count) = module(config, &mut ctx);

    let scratch = Scratch::new(&format!("flatten-{}", config.name()));
    let object_path = tools.compile(&scratch, "flatten", &ir, false).unwrap_or_else(|error| {
        panic!("{}: llc rejected the initializer module:\n{error}", config.name())
    });
    let data = tools.section_bytes(&object_path, ".data");
    let object = tools.read_object(&object_path);
    let data_index = object.section_index(".data").expect("a .data section");
    let bss_index = object.section_index(".bss");

    let mut failures = Vec::new();
    let mut expected_relocs = Vec::new();
    let (mut in_bss, mut bytes_compared) = (0, 0);
    for Global { name, definition, value, exact_addend } in &globals {
        let symbol = object.symbol(name);
        let size = ctx.types.alloc_size_of(ctx.consts.ty(*value));
        if symbol.size != size {
            failures.push(format!("{definition}\n  size: hod {size}, LLVM {}", symbol.size));
            continue;
        }
        let expect_bss = ctx.consts.is_zero_or_undef(*value);
        let section = match symbol.place {
            Place::Section(index) if Some(index) == bss_index => ".bss",
            Place::Section(index) if index == data_index => ".data",
            other => panic!("{definition}: unexpected placement {other:?}"),
        };
        if expect_bss != (section == ".bss") {
            failures.push(format!(
                "{definition}\n  is_zero_or_undef is {expect_bss} but LLVM chose {section}"
            ));
            continue;
        }
        let (mut bytes, mut fixups) = (Vec::new(), Vec::new());
        if let Err(error) = ctx.consts.flatten(&ctx.types, *value, &mut bytes, &mut fixups) {
            failures.push(format!("{definition}\n  flatten failed: {error}"));
            continue;
        }
        assert_eq!(bytes.len() as u64, size, "{definition}: flatten appends the alloc size");
        if expect_bss {
            in_bss += 1;
            if !fixups.is_empty() || bytes.iter().any(|&byte| byte != 0) {
                failures.push(format!("{definition}\n  a .bss variable with a non-zero image"));
            }
            continue;
        }
        for fixup in &fixups {
            let place = symbol.value + fixup.offset;
            match resolve(config, &ctx, &object, data_index, place, fixup) {
                Resolved::Reloc(reloc) => {
                    expected_relocs.push((reloc, definition.as_str(), *exact_addend));
                }
                Resolved::Number(number) => {
                    let at = fixup.offset as usize;
                    let width = usize::from(fixup.size);
                    bytes[at..at + width].copy_from_slice(&number.to_le_bytes()[..width]);
                }
            }
        }
        let actual = &data[symbol.value as usize..][..size as usize];
        bytes_compared += actual.len();
        if actual != bytes {
            failures.push(format!("{definition}\n  hod  {}\n  LLVM {}", hex(&bytes), hex(actual)));
        }
    }

    let inexact = compare_relocs(&mut expected_relocs, &object, &mut failures);

    eprintln!(
        "flatten_diff {}: {} variables ({} random over {TYPE_COUNT} types, {} written by hand), \
         {in_bss} in .bss, {bytes_compared} bytes of .data and {} relocations compared \
         ({inexact} of them modulo the field width)",
        config.name(),
        globals.len(),
        random_count,
        globals.len() - random_count,
        expected_relocs.len(),
    );
    assert!(
        failures.is_empty(),
        "{}: {} initializer mismatches, the first of them:\n{}",
        config.name(),
        failures.len(),
        failures.iter().take(25).cloned().collect::<Vec<_>>().join("\n")
    );
}

/// §4.3 initializer images on aarch64 against LLVM 22.
#[test]
fn initializers_match_llvm22_on_aarch64() {
    check(&CONFIGS[0]);
}

/// §4.3 initializer images on x86-64 against LLVM 22.
#[test]
fn initializers_match_llvm22_on_x86_64() {
    check(&CONFIGS[1]);
}

/// §4.3 initializer images on aarch64 against LLVM 19.
#[test]
fn initializers_match_llvm19_on_aarch64() {
    check(&CONFIGS[2]);
}

/// §4.3 initializer images on x86-64 against LLVM 19.
#[test]
fn initializers_match_llvm19_on_x86_64() {
    check(&CONFIGS[3]);
}
