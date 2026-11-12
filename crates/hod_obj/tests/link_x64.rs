//! `spec/object.md` "Link-and-run tests" for x86-64: objects with
//! hand-encoded x86-64 code are linked by GNU ld, lld and mold and the
//! programs run, under `qemu-x86_64` when the host is not x86-64
//! (`Toolchain::x86_64`). These mirror the first programs of
//! `tests/link.rs`. A missing tool skips the test with a note, or fails
//! under `HOD_REQUIRE_TOOLS=1`.

#![cfg(target_os = "linux")]

mod common;

use common::linking::{Toolchain, exit_code, write};
use common::{define, function_bytes, undefined};
use hod_obj::{Arch, Binding, GroupKind, Object, SectionKind, SymbolId, SymbolKind, X64Reloc};
use hod_testutil::Scratch;

fn object() -> (Object, hod_obj::SectionId) {
    let mut obj = Object::new(Arch::X86_64);
    let text = obj.add_section(b".text", SectionKind::Text, 16).unwrap();
    (obj, text)
}

/// `main` returns 42.
#[test]
fn main_returns_42() {
    let scratch = Scratch::new("obj-x64-ret42");
    let Some(tools) = Toolchain::x86_64(&scratch) else { return };
    let (mut obj, text) = object();
    // mov $42, %eax; ret
    function_bytes(&mut obj, text, "main", &[0xb8, 42, 0, 0, 0, 0xc3]);
    let object = write(&scratch, "ret42.o", &obj);
    for linker in &tools.linkers {
        let program = scratch.join(format!("ret42-{}", linker.name));
        let ran = tools.link_and_run(linker, &[], &[&object], &program);
        assert_eq!(exit_code(&ran), Some(42), "{}", linker.name);
    }
}

/// `main` passes a string in `.rodata` to `puts`: `PC32` against a local
/// symbol and a call through the PLT (`PLT32`).
#[test]
fn main_calls_puts() {
    let scratch = Scratch::new("obj-x64-hello");
    let Some(tools) = Toolchain::x86_64(&scratch) else { return };
    let object = write(&scratch, "hello.o", &common::x86_64_program());
    for linker in &tools.linkers {
        let program = scratch.join(format!("hello-{}", linker.name));
        let ran = tools.link_and_run(linker, &[], &[&object], &program);
        assert_eq!(ran.stdout_text(), "x86\n", "{}", linker.name);
        assert_eq!(exit_code(&ran), Some(42), "{}", linker.name);
    }
}

/// A pointer in `.data` initialized with an address: `R_X86_64_64` against
/// a global symbol, and against a section symbol with an addend.
#[test]
fn data_relocations() {
    let scratch = Scratch::new("obj-x64-data");
    let Some(tools) = Toolchain::x86_64(&scratch) else { return };
    let build = |through_section_symbol: bool| {
        let (mut obj, text) = object();
        let rodata = obj.add_section(b".rodata", SectionKind::ReadOnlyData, 4).unwrap();
        let data = obj.add_section(b".data", SectionKind::Data, 8).unwrap();
        // const int values[2] = {13, 42}; const int *pointer = &values[1];
        obj.append(rodata, &13u32.to_le_bytes());
        let at = obj.append(rodata, &42u32.to_le_bytes());
        let slot = obj.append_zeros(data, 8);
        let object = SymbolKind::Object;
        let pointer = define(&mut obj, "pointer", object, Binding::Global, data, slot, 8);
        if through_section_symbol {
            let section = obj.section_symbol(rodata);
            obj.add_relocation(data, slot, X64Reloc::Abs64, section, at as i64);
        } else {
            let answer = define(&mut obj, "answer", object, Binding::Global, rodata, at, 4);
            obj.add_relocation(data, slot, X64Reloc::Abs64, answer, 0);
        }
        // mov pointer(%rip), %rax; mov (%rax), %eax; ret
        function_bytes(&mut obj, text, "main", &[0x48, 0x8b, 0x05, 0, 0, 0, 0, 0x8b, 0x00, 0xc3]);
        obj.add_relocation(text, 3, X64Reloc::Pc32, pointer, -4);
        obj
    };
    for (through_section_symbol, label) in [(false, "symbol"), (true, "section")] {
        let object = write(&scratch, &format!("data-{label}.o"), &build(through_section_symbol));
        for linker in &tools.linkers {
            let program = scratch.join(format!("data-{label}-{}", linker.name));
            let ran = tools.link_and_run(linker, &[], &[&object], &program);
            assert_eq!(exit_code(&ran), Some(42), "{} {label}", linker.name);
        }
    }
}

/// An object with the COMDAT group `answer` (a function that returns the
/// 32-bit `value` it loads, through `PC32`, from a section of the same
/// group) and the function `name`, which jumps to `callee`.
fn comdat_object(binding: Binding, value: u32, name: &str, callee: &str) -> Object {
    let (mut obj, text) = object();
    let group_text = obj.add_section(b".text.answer", SectionKind::Text, 16).unwrap();
    let rodata = obj.add_section(b".rodata.answer", SectionKind::ReadOnlyData, 4).unwrap();
    obj.append(rodata, &value.to_le_bytes());
    let data = define(&mut obj, "answer_value", SymbolKind::Object, Binding::Weak, rodata, 0, 4);
    // mov answer_value(%rip), %eax; ret
    let answer = function_bytes(&mut obj, group_text, "answer", &[0x8b, 0x05, 0, 0, 0, 0, 0xc3]);
    obj.symbol_mut(answer).binding = binding;
    obj.add_relocation(group_text, 2, X64Reloc::Pc32, data, -4);
    let group = obj.add_group(answer, GroupKind::Comdat);
    obj.add_group_member(group, group_text).unwrap();
    obj.add_group_member(group, rodata).unwrap();
    let callee: SymbolId = if callee == "answer" { answer } else { undefined(&mut obj, callee) };
    // jmp callee
    function_bytes(&mut obj, text, name, &[0xe9, 0, 0, 0, 0]);
    obj.add_relocation(text, 1, X64Reloc::Plt32, callee, -4);
    obj
}

/// Two objects define the same COMDAT group; the linker keeps the first
/// one whole, with the function weak and with it global.
#[test]
fn comdat_groups_are_deduplicated() {
    let scratch = Scratch::new("obj-x64-comdat");
    let Some(tools) = Toolchain::x86_64(&scratch) else { return };
    for (binding, label) in [(Binding::Weak, "weak"), (Binding::Global, "global")] {
        let main = comdat_object(binding, 42, "main", "helper");
        let main = write(&scratch, &format!("main-{label}.o"), &main);
        let helper = comdat_object(binding, 7, "helper", "answer");
        let helper = write(&scratch, &format!("helper-{label}.o"), &helper);
        for linker in &tools.linkers {
            let program = scratch.join(format!("comdat-{label}-{}", linker.name));
            let ran = tools.link_and_run(linker, &[], &[&main, &helper], &program);
            assert_eq!(exit_code(&ran), Some(42), "{} {label}: main's group", linker.name);
            let ran = tools.link_and_run(linker, &[], &[&helper, &main], &program);
            assert_eq!(exit_code(&ran), Some(7), "{} {label}: helper's group", linker.name);
        }
    }
}
