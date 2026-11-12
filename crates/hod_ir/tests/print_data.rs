//! `spec/ir.md` §9.3: printed initializers mean to LLVM what they mean to
//! hod. A module of variables is built through `hod_ir`, printed, compiled
//! by `llc`, and the object's `.data` section and its relocations are
//! compared, byte for byte, with what `ConstPool::flatten` (§4.3) makes of
//! the same initializers.
//!
//! The variables are those of `flatten_diff.rs`: two per type of the shared
//! type corpus (one of plain data, one whose pointers may be symbol
//! addresses), then the hand-written address forms of §4.2. There, LLVM
//! reads text a test wrote; here it reads text the printer wrote, from the
//! folded constants. Both targets, LLVM 22 and LLVM 19; one `llc`, one
//! `llvm-objdump` and one `llvm-readelf` run per configuration.
//!
//! The type table is the module's own on every dialect: hod's layout rules
//! are LLVM 22's, and the printer makes LLVM 19 lay the variables out the
//! same way (§9.2). The one thing it cannot give LLVM 19 on AArch64 is the
//! address spaces 270 to 272, so the types that hold such pointers are left
//! out of that configuration.

mod common;

use std::fmt::Write as _;

use common::consts::{Ctx, address_cases, padding_cases};
use common::{CONFIGS, Config, Object, Place, Reloc, Ty, type_corpus};
use hod_ir::{
    Arch, ConstId, Dialect, Fixup, FixupTarget, FunctionBuilder, FunctionDesc, Module, SymId,
    TargetSpec, TypeId, TypeTable, VariableDesc,
};
use hod_testutil::Scratch;

/// Types per configuration; each gets two variables.
const TYPE_COUNT: usize = 300;

/// A variable of the module.
struct Global {
    name: String,
    /// For messages: its type and how it was made.
    what: String,
    ty: TypeId,
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
    names: &[String],
    object: &Object,
    data: u32,
    place: u64,
    fixup: &Fixup,
) -> Resolved {
    let name = |symbol: SymId| names[symbol.index()].as_str();
    let reloc = |kind: String, symbol: SymId, addend: i64| {
        Resolved::Reloc(Reloc { offset: place, kind, symbol: name(symbol).to_string(), addend })
    };
    match fixup.target {
        FixupTarget::Sym(sym) => {
            reloc(reloc_kind(config.arch, fixup.size, false), sym, fixup.addend)
        }
        FixupTarget::Diff { a, b } | FixupTarget::PltDiff { a, b } => {
            let (plus, minus) = (object.symbol(name(a)), object.symbol(name(b)));
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

/// Whether `ty` holds a pointer of an address space other than 0 and 1.
fn has_wide_addr_space(ty: &Ty) -> bool {
    match ty {
        Ty::Ptr(space) => *space > 1,
        Ty::Vector(elem, _) | Ty::Array(elem, _) => has_wide_addr_space(elem),
        Ty::Struct(fields, _) => fields.iter().any(has_wide_addr_space),
        _ => false,
    }
}

/// The variables of one configuration, with their initializers built in
/// `ctx`.
fn globals(config: &Config, ctx: &mut Ctx) -> (Vec<Global>, usize) {
    // LLVM 19's AArch64 layout has no entry for the address spaces 270 to
    // 272 (§9.2).
    let narrow_only = config.arch == Arch::Aarch64 && config.llvm < 20;
    let mut globals = Vec::new();
    for (index, ty) in type_corpus(0x51ab_0002, TYPE_COUNT).iter().enumerate() {
        if narrow_only && has_wide_addr_space(ty) {
            continue;
        }
        for relocs in [false, true] {
            let name = format!("{}{index}", if relocs { "r" } else { "g" });
            ctx.this = ctx.sym(&name);
            let (text, value) = ctx.random_value(ty, relocs);
            let what = format!("@{name} = global {} {text}", ty.text());
            let ty = ty.intern(&mut ctx.types);
            globals.push(Global { name, what, ty, value, exact_addend: true });
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
        let what = format!("@{name} = global {} {}", case.ty.text(), case.text);
        let value = (case.build)(ctx);
        let ty = case.ty.intern(&mut ctx.types);
        globals.push(Global { name, what, ty, value, exact_addend: case.exact_addend });
    }
    (globals, random_count)
}

/// Compares the relocations expected from hod's fixups (each with the
/// variable it is in and whether its addend is exactly `llc`'s) with all
/// the relocations of `.data`, in offset order on both sides. Returns how
/// many were compared modulo the width of the field.
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
    for (expected, what, exact_addend) in expected_relocs.iter() {
        while actual.peek().is_some_and(|reloc| reloc.offset < expected.offset) {
            failures.push(format!("a relocation hod has no fixup for: {:?}", actual.next()));
        }
        match actual.peek() {
            Some(reloc) if reloc.offset == expected.offset => {
                let mut llvm = (*reloc).clone();
                if !exact_addend {
                    llvm.addend = reduced(llvm.addend, reloc_size(&llvm.kind));
                    inexact += 1;
                }
                if llvm != *expected {
                    failures.push(format!("{what}\n  hod  {expected:?}\n  LLVM {reloc:?}"));
                }
                actual.next();
            }
            _ => failures.push(format!("{what}\n  LLVM has no relocation for {expected:?}")),
        }
    }
    failures.extend(actual.map(|reloc| format!("a relocation hod has no fixup for: {reloc:?}")));
    inexact
}

/// The module: the five symbols every initializer may name (`@a` and `@b`
/// are defined data, `@x` is external data, `@f` is a defined function and
/// `@h` an external one), then the variables. Returns it with the body of
/// `@f` and the name of every symbol.
fn module(config: &Config, ctx: Ctx, globals: &[Global]) -> (Module, hod_ir::Body, Vec<String>) {
    let mut ctx = ctx;
    // The context numbers symbols in the order it met them.
    let count = ctx.sym("\u{1}the end").index();
    let names: Vec<String> =
        (0..count).map(|id| ctx.name(SymId::new(id as u32)).to_string()).collect();
    let target = match config.arch {
        Arch::Aarch64 => TargetSpec::aarch64_linux_gnu(),
        Arch::X86_64 => TargetSpec::x86_64_linux_gnu(),
    };
    let mut m = Module::new(target);
    m.types = ctx.types;
    m.consts = ctx.consts;
    for (id, name) in names.iter().enumerate() {
        assert_eq!(m.symbol_named(name.as_bytes()).unwrap().index(), id);
    }
    let int = |m: &mut Module, value: i64| m.consts.int_signed(&m.types, TypeId::I32, value);
    let seven = int(&mut m, 7).unwrap();
    let a = VariableDesc { init: Some(seven), ..VariableDesc::new(TypeId::I32) };
    m.add_variable(b"a", &a).unwrap();
    let four = m.types.array(TypeId::I32, 4).unwrap();
    let elems: Vec<ConstId> = (1..=4).map(|value| int(&mut m, value).unwrap()).collect();
    let elems = m.consts.aggregate(&m.types, four, &elems).unwrap();
    m.add_variable(b"b", &VariableDesc { init: Some(elems), ..VariableDesc::new(four) }).unwrap();
    m.add_variable(b"x", &VariableDesc::new(TypeId::I32)).unwrap();
    let void_fn = m.types.function(TypeId::VOID, &[], false).unwrap();
    m.add_function(b"h", &FunctionDesc::new(void_fn)).unwrap();
    let f = m
        .add_function(b"f", &FunctionDesc { has_body: true, ..FunctionDesc::new(void_fn) })
        .unwrap();
    for global in globals {
        let desc = VariableDesc { init: Some(global.value), ..VariableDesc::new(global.ty) };
        m.add_variable(global.name.as_bytes(), &desc)
            .unwrap_or_else(|error| panic!("{}: {error}", global.what));
    }
    m.verify_symbols().unwrap();
    let mut builder = FunctionBuilder::new(&m, f, void_fn).unwrap();
    let entry = builder.create_block().unwrap();
    builder.begin_block(entry).unwrap();
    builder.ret(None).unwrap();
    let body = builder.finish().unwrap();
    body.verify(&m).unwrap();
    (m, body, names)
}

fn check(config: &Config) {
    let Some(tools) = common::tools(config.llvm) else { return };
    let mut ctx = Ctx::new(config, 0x0f1a_77e4);
    // hod's own layout, whatever the dialect.
    let layout = match config.arch {
        Arch::Aarch64 => TargetSpec::aarch64_linux_gnu(),
        Arch::X86_64 => TargetSpec::x86_64_linux_gnu(),
    };
    ctx.types = TypeTable::new(layout.data_layout());
    for name in ["a", "b", "x", "h", "f"] {
        ctx.sym(name);
    }
    let (globals, random_count) = globals(config, &mut ctx);
    let (m, body, names) = module(config, ctx, &globals);

    let mut ir = String::new();
    m.print(&mut ir, Dialect::new(config.llvm).unwrap(), [&body]).unwrap();
    let scratch = Scratch::new(&format!("print-data-{}", config.name()));
    let object_path = tools.compile(&scratch, "data", &ir, false).unwrap_or_else(|error| {
        panic!("{}: llc rejected the printed module:\n{error}", config.name())
    });
    let data = tools.section_bytes(&object_path, ".data");
    let object = tools.read_object(&object_path);
    let data_index = object.section_index(".data").expect("a .data section");
    let bss_index = object.section_index(".bss");

    let mut failures = Vec::new();
    let mut expected_relocs = Vec::new();
    let (mut in_bss, mut bytes_compared) = (0, 0);
    for Global { name, what, ty, value, exact_addend } in &globals {
        let symbol = object.symbol(name);
        let size = m.types.alloc_size_of(*ty);
        if symbol.size != size {
            failures.push(format!("{what}\n  size: hod {size}, LLVM {}", symbol.size));
            continue;
        }
        let expect_bss = m.consts.is_zero_or_undef(*value);
        let section = match symbol.place {
            Place::Section(index) if Some(index) == bss_index => ".bss",
            Place::Section(index) if index == data_index => ".data",
            other => panic!("{what}: unexpected placement {other:?}"),
        };
        if expect_bss != (section == ".bss") {
            failures.push(format!(
                "{what}\n  is_zero_or_undef is {expect_bss} but LLVM chose {section}"
            ));
            continue;
        }
        let (mut bytes, mut fixups) = (Vec::new(), Vec::new());
        m.consts.flatten(&m.types, *value, &mut bytes, &mut fixups).unwrap();
        assert_eq!(bytes.len() as u64, size, "{what}: flatten appends the alloc size");
        if expect_bss {
            in_bss += 1;
            continue;
        }
        for fixup in &fixups {
            let place = symbol.value + fixup.offset;
            match resolve(config, &names, &object, data_index, place, fixup) {
                Resolved::Reloc(reloc) => {
                    expected_relocs.push((reloc, what.as_str(), *exact_addend));
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
            failures.push(format!("{what}\n  hod  {}\n  LLVM {}", hex(&bytes), hex(actual)));
        }
    }
    let inexact = compare_relocs(&mut expected_relocs, &object, &mut failures);

    eprintln!(
        "print_data {}: {} bytes of text, {} variables ({random_count} random, {} written by \
         hand), {in_bss} in .bss, {bytes_compared} bytes of .data and {} relocations compared \
         ({inexact} of them modulo the field width)",
        config.name(),
        ir.len(),
        globals.len(),
        globals.len() - random_count,
        expected_relocs.len(),
    );
    assert!(bytes_compared > 10_000 && expected_relocs.len() > 100, "{}", config.name());
    assert!(
        failures.is_empty(),
        "{}: {} initializer mismatches, the first of them:\n{}",
        config.name(),
        failures.len(),
        failures.iter().take(25).cloned().collect::<Vec<_>>().join("\n")
    );
}

/// §9.3 printed initializers on aarch64 through LLVM 22.
#[test]
fn printed_initializers_match_flatten_llvm22_on_aarch64() {
    check(&CONFIGS[0]);
}

/// §9.3 printed initializers on x86-64 through LLVM 22.
#[test]
fn printed_initializers_match_flatten_llvm22_on_x86_64() {
    check(&CONFIGS[1]);
}

/// §9.3 printed initializers on aarch64 through LLVM 19.
#[test]
fn printed_initializers_match_flatten_llvm19_on_aarch64() {
    check(&CONFIGS[2]);
}

/// §9.3 printed initializers on x86-64 through LLVM 19.
#[test]
fn printed_initializers_match_flatten_llvm19_on_x86_64() {
    check(&CONFIGS[3]);
}
