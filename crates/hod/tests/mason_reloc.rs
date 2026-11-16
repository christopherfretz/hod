//! hod's own `.mason.meta` unit (`spec/codegen.md` §9.7.5, WP-332),
//! through the session, for every target (x86-64, AArch64, i386) and
//! relocation model (`static`, `pie`, `pic`):
//!
//! - the object holds exactly one `.mason.meta` section, `SHT_PROGBITS`,
//!   `sh_flags` exactly `SHF_EXCLUDE`, `sh_addralign` 8, in no group, with
//!   no relocation section; its contents read as a sequence of units
//!   (mortar's `spec/meta.md` MT-10) and exactly one of them is hod's
//!   (producer 2) with the one kind-1 row `reloc-model` = the model;
//! - with kiln's container in the module, kiln's unit comes first with
//!   its bytes unchanged (H-2) and hod's follows it in the one section;
//! - linked into a program by every linker present (GNU ld, lld, mold,
//!   mortar; wild when present), the program runs and has no
//!   `.mason.meta`.

#![allow(dead_code, reason = "the linking support is shared with other tests")]

#[path = "../../hod_obj/tests/common/linking.rs"]
mod linking;

use std::fmt::Write as _;
use std::path::Path;

use hod::{Options, OutputKind, RelocModel, Report, Settings};
use hod_testutil::{Scratch, Target, tools_required};
use linking::{Toolchain, exit_code};

const TARGETS: [Target; 3] = [Target::X86_64, Target::Aarch64, Target::I686];

const SHT_PROGBITS: u32 = 1;
const SHT_RELA: u32 = 4;
const SHT_REL: u32 = 9;
const SHT_GROUP: u32 = 17;
const SHF_EXCLUDE: u64 = 0x8000_0000;

/// The relocation models of the row, with the options and module flag
/// that select them.
#[derive(Clone, Copy, Debug)]
enum Model {
    Static,
    Pie,
    Pic,
}

const MODELS: [Model; 3] = [Model::Static, Model::Pie, Model::Pic];

impl Model {
    fn value(self) -> &'static [u8] {
        match self {
            Model::Static => b"static",
            Model::Pie => b"pie",
            Model::Pic => b"pic",
        }
    }
}

fn triple(target: Target) -> &'static str {
    match target {
        Target::Aarch64 => "aarch64-unknown-linux-gnu",
        Target::X86_64 => "x86_64-unknown-linux-gnu",
        Target::I686 => "i686-unknown-linux-gnu",
    }
}

/// A program that returns 0; with `kiln`, kiln's container too.
fn module(model: Model, kiln: bool) -> String {
    let mut text = String::from(
        "@counter = global i32 41\n\
         define i32 @main() {\n  %v = load i32, ptr @counter\n  %r = sub i32 %v, 41\n  ret i32 %r\n}\n",
    );
    if kiln {
        let _ = write!(text, "@__mason_meta = private constant [{} x i8] c\"", KILN_UNIT.len());
        for byte in KILN_UNIT {
            let _ = write!(text, "\\{byte:02X}");
        }
        text.push_str(
            "\", section \".mason.meta\", align 8, !exclude !{}\n\
             @llvm.compiler.used = appending global [1 x ptr] [ptr @__mason_meta], section \"llvm.metadata\"\n",
        );
    }
    if matches!(model, Model::Pie) {
        text.push_str(
            "!llvm.module.flags = !{!0, !1}\n!0 = !{i32 8, !\"PIC Level\", i32 2}\n\
             !1 = !{i32 7, !\"PIE Level\", i32 2}\n",
        );
    }
    text
}

/// A unit as kiln writes one (producer 1), with one kind-1 row
/// `lang` = `c++`: 32 bytes of header, a 32-byte block, a 10-byte string
/// table, padding to 80.
const KILN_UNIT: [u8; 80] = [
    b'M', b'A', b'S', b'N', 1, 0, 0, 0, 80, 0, 0, 0, 32, 0, 0, 0, // magic, 1.0, size, header
    1, 0, 0, 0, 64, 0, 0, 0, 10, 0, 0, 0, 1, 0, 0, 0, // blocks, strtab, producer 1, flags
    1, 0, 1, 0, 32, 0, 0, 0, 16, 0, 0, 0, 1, 0, 0, 0, // kind 1, v1, block 32, row 16, 1 row
    1, 0, 0, 0, 6, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, // name 1, value 6
    0, b'l', b'a', b'n', b'g', 0, b'c', b'+', b'+', 0, 0, 0, 0, 0, 0, 0,
];

fn hod_object(model: Model, kiln: bool, target: Target) -> Vec<u8> {
    let options = Options {
        triple: Some(triple(target).to_owned()),
        output_kind: OutputKind::Object,
        relocation_model: match model {
            Model::Static => None,
            Model::Pie | Model::Pic => Some(RelocModel::Pic),
        },
        ..Options::default()
    };
    let text = module(model, kiln);
    hod::compile_with(text.as_bytes(), &options, &Settings::default(), &mut Report::default())
        .unwrap_or_else(|e| panic!("hod: {e}"))
}

/// One section header of an ELF32 or ELF64 little-endian file.
struct Section {
    name: Vec<u8>,
    sh_type: u32,
    flags: u64,
    offset: u64,
    size: u64,
    info: u32,
    align: u64,
}

fn sections(elf: &[u8]) -> Vec<Section> {
    let u16_at = |at: usize| u64::from(u16::from_le_bytes(elf[at..at + 2].try_into().unwrap()));
    let u32_at = |at: usize| u32::from_le_bytes(elf[at..at + 4].try_into().unwrap());
    let u64_at = |at: usize| u64::from_le_bytes(elf[at..at + 8].try_into().unwrap());
    assert_eq!(&elf[..4], b"\x7fELF");
    assert_eq!(elf[5], 1, "little-endian");
    let wide = match elf[4] {
        1 => false,
        2 => true,
        class => panic!("ELF class {class}"),
    };
    // The word-sized fields: `(ELF32 offset, ELF64 offset)`.
    let word = |at: (usize, usize)| if wide { u64_at(at.1) } else { u64::from(u32_at(at.0)) };
    let shoff = word((0x20, 0x28)) as usize;
    let (count, strndx) = if wide {
        (u16_at(0x3c) as usize, u16_at(0x3e) as usize)
    } else {
        (u16_at(0x30) as usize, u16_at(0x32) as usize)
    };
    let size = if wide { 64 } else { 40 };
    let header = |index: usize| shoff + index * size;
    let names = word((header(strndx) + 0x10, header(strndx) + 0x18)) as usize;
    (0..count)
        .map(|index| {
            let at = header(index);
            let name_at = names + u32_at(at) as usize;
            let end = name_at + elf[name_at..].iter().position(|&b| b == 0).unwrap();
            Section {
                name: elf[name_at..end].to_vec(),
                sh_type: u32_at(at + 4),
                flags: word((at + 8, at + 8)),
                offset: word((at + 0x10, at + 0x18)),
                size: word((at + 0x14, at + 0x20)),
                info: u32_at(if wide { at + 0x2c } else { at + 0x1c }),
                align: word((at + 0x20, at + 0x30)),
            }
        })
        .collect()
}

/// A unit read by MT-10's framing: its producer and its kind-1 rows as
/// (name, value, flags, reserved).
struct Unit {
    at: usize,
    size: usize,
    producer: u16,
    rows: Vec<(Vec<u8>, Vec<u8>, u32, u32)>,
}

/// The units of a container, as mortar's MT-10 reader frames them: at an
/// 8-aligned offset a unit (`MASN`) or eight zero bytes of padding.
fn units(data: &[u8]) -> Vec<Unit> {
    let u16_at = |b: &[u8], at: usize| u16::from_le_bytes(b[at..at + 2].try_into().unwrap());
    let u32_at = |b: &[u8], at: usize| u32::from_le_bytes(b[at..at + 4].try_into().unwrap());
    let mut out = Vec::new();
    let mut at = 0;
    while at < data.len() {
        if data[at..at + 8] == [0; 8] {
            at += 8;
            continue;
        }
        let unit = &data[at..];
        assert_eq!(&unit[..4], b"MASN", "a unit at {at}");
        assert_eq!((u16_at(unit, 4), u16_at(unit, 6)), (1, 0), "version 1.0");
        let size = u32_at(unit, 8) as usize;
        assert_eq!(size % 8, 0, "unit_size a multiple of 8");
        let unit = &unit[..size];
        let header_size = u32_at(unit, 12) as usize;
        assert_eq!(header_size, 32);
        let (strtab_at, strtab_size) = (u32_at(unit, 20) as usize, u32_at(unit, 24) as usize);
        let strtab = &unit[strtab_at..strtab_at + strtab_size];
        assert_eq!(strtab[0], 0, "MT-12: offset 0 is the empty string");
        let string = |offset: u32| {
            let s = &strtab[offset as usize..];
            s[..s.iter().position(|&b| b == 0).unwrap()].to_vec()
        };
        assert_eq!(u16_at(unit, 30), 0, "unit flags");
        let mut rows = Vec::new();
        let mut block = header_size;
        for _ in 0..u32_at(unit, 16) {
            let (kind, version) = (u16_at(unit, block), u16_at(unit, block + 2));
            let block_size = u32_at(unit, block + 4) as usize;
            let (row_size, row_count) = (u32_at(unit, block + 8), u32_at(unit, block + 12));
            assert_eq!(block_size % 8, 0);
            if kind == 1 {
                assert_eq!((version, row_size), (1, 16), "MT-26 v1 rows");
                for r in 0..row_count as usize {
                    let row = block + 16 + r * 16;
                    rows.push((
                        string(u32_at(unit, row)),
                        string(u32_at(unit, row + 4)),
                        u32_at(unit, row + 8),
                        u32_at(unit, row + 12),
                    ));
                }
            }
            block += block_size;
        }
        out.push(Unit { at, size, producer: u16_at(unit, 28), rows });
        at += size;
    }
    out
}

/// The header fields and contents of `object`'s one `.mason.meta`: the
/// units in it, in order.
fn container(object: &[u8], who: &str) -> (Vec<u8>, Vec<Unit>) {
    let all = sections(object);
    let found: Vec<usize> =
        (0..all.len()).filter(|&index| all[index].name == b".mason.meta").collect();
    let [index] = found[..] else { panic!("{who}: one .mason.meta, found {}", found.len()) };
    let meta = &all[index];
    assert_eq!(meta.sh_type, SHT_PROGBITS, "{who}");
    assert_eq!(meta.flags, SHF_EXCLUDE, "{who}: flags exactly SHF_EXCLUDE");
    assert_eq!(meta.align, 8, "{who}");
    for section in &all {
        if matches!(section.sh_type, SHT_RELA | SHT_REL) {
            assert_ne!(section.info as usize, index, "{who}: a relocation section for it");
        }
        if section.sh_type == SHT_GROUP {
            let members = &object[section.offset as usize + 4..][..section.size as usize - 4];
            let index = u32::try_from(index).unwrap().to_le_bytes();
            assert!(!members.chunks(4).any(|m| m == index), "{who}: in a group");
        }
    }
    let data = object[meta.offset as usize..][..meta.size as usize].to_vec();
    assert_eq!(data.len() % 8, 0, "{who}");
    let units = units(&data);
    (data, units)
}

fn check_hod_unit(unit: &Unit, model: Model, who: &str) {
    assert_eq!(unit.producer, 2, "{who}: producer 2 (hod)");
    assert_eq!(unit.size, 88, "{who}");
    let [(name, value, flags, reserved)] = &unit.rows[..] else {
        panic!("{who}: one row, found {}", unit.rows.len())
    };
    assert_eq!(name.as_slice(), b"reloc-model", "{who}");
    assert_eq!(value.as_slice(), model.value(), "{who}");
    assert_eq!((*flags, *reserved), (0, 0), "{who}");
}

#[test]
fn every_object_holds_one_hod_unit_with_the_model() {
    for target in TARGETS {
        for model in MODELS {
            let who = format!("{} {model:?}", target.name());
            let (_, units) = container(&hod_object(model, false, target), &who);
            let [unit] = &units[..] else { panic!("{who}: one unit, found {}", units.len()) };
            assert_eq!(unit.at, 0, "{who}");
            check_hod_unit(unit, model, &who);
        }
    }
}

#[test]
fn kilns_unit_comes_first_unchanged_and_hods_follows() {
    for target in TARGETS {
        for model in MODELS {
            let who = format!("{} {model:?} with kiln's container", target.name());
            let (data, units) = container(&hod_object(model, true, target), &who);
            assert_eq!(&data[..KILN_UNIT.len()], &KILN_UNIT, "{who}: kiln's bytes (H-2)");
            let [kiln, hod] = &units[..] else { panic!("{who}: two units, found {}", units.len()) };
            assert_eq!((kiln.at, kiln.producer), (0, 1), "{who}: kiln's unit first");
            assert_eq!(hod.at, KILN_UNIT.len(), "{who}: hod's right after it");
            check_hod_unit(hod, model, &who);
        }
    }
}

#[test]
fn the_unit_is_deterministic() {
    for target in TARGETS {
        let a = hod_object(Model::Pic, true, target);
        assert_eq!(a, hod_object(Model::Pic, true, target), "{}", target.name());
    }
}

#[test]
fn linkers_drop_hods_unit() {
    for target in [Target::X86_64, Target::Aarch64] {
        let dir = Scratch::new(&format!("hod-mason-reloc-{}", target.name()));
        let Some(toolchain) = Toolchain::for_target(&dir, target) else {
            assert!(!tools_required(), "no {} toolchain (HOD_REQUIRE_TOOLS=1)", target.name());
            continue;
        };
        let names: Vec<&str> = toolchain.linkers.iter().map(|linker| linker.name).collect();
        for wanted in ["bfd", "lld", "mold"] {
            assert!(names.contains(&wanted) || !tools_required(), "no {wanted}: {names:?}");
        }
        let object = dir.write("reloc.o", hod_object(Model::Pie, false, target));
        for linker in &toolchain.linkers {
            let who = format!("{} {}", target.name(), linker.name);
            let program = dir.join(format!("reloc-{}", linker.name));
            let inputs: [&Path; 1] = [&object];
            let ran = toolchain.link_and_run(linker, &["-pie"], &inputs, &program);
            assert_eq!(exit_code(&ran), Some(0), "{who}: {}", ran.stderr_text());
            let linked = std::fs::read(&program).unwrap();
            assert!(
                !sections(&linked).iter().any(|section| section.name == b".mason.meta"),
                "{who}: the section is in the output"
            );
            eprintln!("{who}: linked and ran without .mason.meta");
        }
    }
}
