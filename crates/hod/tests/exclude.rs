//! `!exclude` on a global (`spec/codegen.md` §9.7.4, WP-273), through the
//! session (what `hod` the program does), from text and from the bitcode
//! of `llvm-as` 19 and 22, for both targets:
//!
//! - the section's header fields: `.mason.meta`, `SHT_PROGBITS`,
//!   `sh_flags` exactly `SHF_EXCLUDE`, `sh_addralign` 8, in no group,
//!   without a relocation section; its bytes are the initializer's
//!   verbatim (0x00, 0xff and sequences that are not UTF-8 among them);
//! - linked into a program by GNU ld, lld and mold, the section is not in
//!   the output, and the program runs.
//!
//! The comparison of such objects with `llc`'s is `hod_cg`'s `emit_llc`
//! (snippet `exclude`).

#![allow(dead_code, reason = "the linking support is shared with other tests")]

#[path = "../../hod_obj/tests/common/linking.rs"]
mod linking;

use std::path::Path;
use std::process::Command;
use std::time::Duration;

use hod::{Options, OutputKind, RelocModel, Report, Settings};
use hod_testutil::{Scratch, Target, run_with, tool, tools_required};
use linking::{Toolchain, exit_code};

/// The standalone example the record names.
const EXAMPLE: &str = include_str!("../../hod_cg/tests/data/exclude/mason_meta.ll");

/// The initializer of `@__mason_meta` in [`EXAMPLE`].
const BYTES: [u8; 16] =
    [b'M', b'M', b'v', b'1', 0, 0xff, 0xc3, b'(', 0x80, 0xfe, 0xff, 0, 0xe2, 0x82, 0, b'z'];

const SHT_PROGBITS: u32 = 1;
const SHT_RELA: u32 = 4;
const SHT_GROUP: u32 = 17;
const SHF_EXCLUDE: u64 = 0x8000_0000;

fn triple(target: Target) -> &'static str {
    match target {
        Target::Aarch64 => "aarch64-unknown-linux-gnu",
        Target::X86_64 => "x86_64-unknown-linux-gnu",
    }
}

fn hod_object(input: &[u8], target: Target) -> Vec<u8> {
    let options = Options {
        triple: Some(triple(target).to_owned()),
        output_kind: OutputKind::Object,
        relocation_model: Some(RelocModel::Pic),
        ..Options::default()
    };
    hod::compile_with(input, &options, &Settings::default(), &mut Report::default())
        .unwrap_or_else(|e| panic!("hod: {e}"))
}

/// One section header of an ELF64 little-endian file.
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
    assert_eq!(&elf[..6], b"\x7fELF\x02\x01", "an ELF64 little-endian file");
    let shoff = u64_at(0x28) as usize;
    let (count, strndx) = (u16_at(0x3c) as usize, u16_at(0x3e) as usize);
    let header = |index: usize| shoff + index * 64;
    let names = u64_at(header(strndx) + 0x18) as usize;
    (0..count)
        .map(|index| {
            let at = header(index);
            let name_at = names + u32_at(at) as usize;
            let end = name_at + elf[name_at..].iter().position(|&b| b == 0).unwrap();
            Section {
                name: elf[name_at..end].to_vec(),
                sh_type: u32_at(at + 4),
                flags: u64_at(at + 8),
                offset: u64_at(at + 0x18),
                size: u64_at(at + 0x20),
                info: u32_at(at + 0x2c),
                align: u64_at(at + 0x30),
            }
        })
        .collect()
}

/// H-2 and the header fields of `object`'s `.mason.meta`.
fn check_object(object: &[u8], who: &str) {
    let all = sections(object);
    let found: Vec<usize> =
        (0..all.len()).filter(|&index| all[index].name == b".mason.meta").collect();
    let [index] = found[..] else { panic!("{who}: one .mason.meta, found {}", found.len()) };
    let meta = &all[index];
    assert_eq!(meta.sh_type, SHT_PROGBITS, "{who}");
    assert_eq!(meta.flags, SHF_EXCLUDE, "{who}: flags exactly SHF_EXCLUDE");
    assert_eq!(meta.align, 8, "{who}");
    let start = meta.offset as usize;
    assert_eq!(&object[start..start + meta.size as usize], &BYTES, "{who}: the bytes verbatim");
    for section in &all {
        if section.sh_type == SHT_RELA {
            assert_ne!(section.info as usize, index, "{who}: a relocation section for it");
        }
        if section.sh_type == SHT_GROUP {
            let members = &object[section.offset as usize + 4..][..section.size as usize - 4];
            let index = u32::try_from(index).unwrap().to_le_bytes();
            assert!(!members.chunks(4).any(|m| m == index), "{who}: in a group");
        }
    }
}

/// `llvm-as` of `major`'s bitcode of `text`.
fn bitcode(major: u32, text: &str) -> Option<Vec<u8>> {
    let llvm_as = tool(&format!("llvm-as-{major}"))?;
    let mut command = Command::new(llvm_as);
    command.args(["-o", "-", "-"]);
    let ran = run_with(&mut command, text.as_bytes(), Duration::from_mins(1));
    assert!(ran.success(), "llvm-as-{major}: {}", ran.stderr_text());
    Some(ran.stdout)
}

/// Every input of the example: its text, and its bitcode of each major.
fn inputs() -> Vec<(String, Vec<u8>)> {
    let mut inputs = vec![("text".to_owned(), EXAMPLE.as_bytes().to_vec())];
    for major in [19, 22] {
        match bitcode(major, EXAMPLE) {
            Some(bits) => inputs.push((format!("bitcode-{major}"), bits)),
            None => assert!(!tools_required(), "no llvm-as-{major} (HOD_REQUIRE_TOOLS=1)"),
        }
    }
    inputs
}

#[test]
fn excluded_section_has_llcs_header_and_the_constants_bytes() {
    for target in Target::ALL {
        for (input, bytes) in inputs() {
            let who = format!("{} {input}", target.name());
            check_object(&hod_object(&bytes, target), &who);
        }
    }
}

/// The shape hod keeps refused: an `!exclude` node that is not a tuple
/// (`llvm-as` takes one), refused by the old name.
#[test]
fn exclude_of_a_node_that_is_not_a_tuple_is_refused_by_name() {
    let text = "@x = global i32 1, section \".x\", !exclude !0\n!0 = !DIExpression()\n";
    let options = Options {
        triple: Some(triple(Target::X86_64).to_owned()),
        output_kind: OutputKind::Object,
        ..Options::default()
    };
    let error =
        hod::compile_with(text.as_bytes(), &options, &Settings::default(), &mut Report::default())
            .expect_err("refused");
    assert!(error.to_string().contains("global-metadata:exclude"), "{error}");
}

#[test]
fn linkers_drop_the_excluded_section() {
    for target in Target::ALL {
        let dir = Scratch::new(&format!("hod-exclude-{}", target.name()));
        let Some(toolchain) = Toolchain::for_target(&dir, target) else {
            assert!(!tools_required(), "no {} toolchain (HOD_REQUIRE_TOOLS=1)", target.name());
            continue;
        };
        let names: Vec<&str> = toolchain.linkers.iter().map(|linker| linker.name).collect();
        for wanted in ["bfd", "lld", "mold"] {
            assert!(names.contains(&wanted) || !tools_required(), "no {wanted}: {names:?}");
        }
        let object = dir.write("meta.o", hod_object(EXAMPLE.as_bytes(), target));
        for linker in &toolchain.linkers {
            let who = format!("{} {}", target.name(), linker.name);
            let program = dir.join(format!("meta-{}", linker.name));
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
