//! `spec/object.md` "Testing", link-and-run: objects the writer produces
//! are linked by GNU ld, lld and mold, and the programs run.
//!
//! The code in the objects is hand-encoded AArch64, so these tests exist
//! only on an AArch64 Linux host. A missing linker skips its part with a
//! note, or fails under `HOD_REQUIRE_TOOLS=1`.

#![cfg(all(target_arch = "aarch64", target_os = "linux"))]

mod common;

use std::path::PathBuf;

use common::linking::{Toolchain, exit_code, find_marker, write};
use common::{
    ADD_X0_LO12, ADRP_X0, B, BL, LDR_W0_LO12, LDR_X0_LO12, MOV_W0_42, POP_FRAME, PUSH_FRAME, RET,
    SET_FP, define, function, undefined,
};
use hod_obj::{
    A64Reloc, Arch, Binding, GroupKind, Object, SectionKind, Symbol, SymbolKind, SymbolPlace,
};
use hod_testutil::Scratch;

/// (a) `main` returns 42.
#[test]
fn main_returns_42() {
    let scratch = Scratch::new("obj-link-ret42");
    let Some(tools) = Toolchain::host(&scratch) else { return };
    let object = write(&scratch, "ret42.o", &common::return_42());
    for linker in &tools.linkers {
        let program = scratch.join(format!("ret42-{}", linker.name));
        let ran = tools.link_and_run(linker, &[], &[&object], &program);
        assert_eq!(exit_code(&ran), Some(42), "{}", linker.name);
    }
}

/// (b) `main` passes a string in `.rodata` to `puts`: page-relative
/// addressing of a local symbol, and a call through the PLT.
#[test]
fn main_calls_puts() {
    let scratch = Scratch::new("obj-link-hello");
    let Some(tools) = Toolchain::host(&scratch) else { return };
    let object = write(&scratch, "hello.o", &common::hello("hello from hod"));
    for linker in &tools.linkers {
        let program = scratch.join(format!("hello-{}", linker.name));
        let ran = tools.link_and_run(linker, &[], &[&object], &program);
        assert_eq!(ran.stdout_text(), "hello from hod\n", "{}", linker.name);
        assert_eq!(exit_code(&ran), Some(0), "{}", linker.name);
    }
}

/// `int main(void) { return helper(); }`, with its own copy of the COMDAT
/// group `answer`, whose function returns `value`.
fn comdat_main(binding: Binding, value: u32) -> Object {
    let mut obj = Object::new(Arch::AArch64);
    let text = obj.add_section(b".text", SectionKind::Text, 4).unwrap();
    let helper = undefined(&mut obj, "helper");
    function(&mut obj, text, "main", &[PUSH_FRAME, SET_FP, BL, POP_FRAME, RET]);
    obj.add_relocation(text, 8, A64Reloc::Call26, helper, 0);
    common::add_answer_comdat(&mut obj, binding, value);
    obj
}

/// `int helper(void) { return answer(); }`, with its own copy of the
/// COMDAT group `answer`, whose function returns `value`.
fn comdat_helper(binding: Binding, value: u32) -> Object {
    let mut obj = Object::new(Arch::AArch64);
    let answer = common::add_answer_comdat(&mut obj, binding, value);
    let text = obj.add_section(b".text", SectionKind::Text, 4).unwrap();
    function(&mut obj, text, "helper", &[B]);
    obj.add_relocation(text, 0, A64Reloc::Jump26, answer, 0);
    obj
}

/// (c) Two objects define the same COMDAT group. The linker keeps one
/// whole group, the first on the command line: the function and the data
/// it loads through the group's relocations come from the same object.
/// With a global rather than weak function, only the group keeps the link
/// from failing on a duplicate definition.
#[test]
fn comdat_groups_are_deduplicated() {
    let scratch = Scratch::new("obj-link-comdat");
    let Some(tools) = Toolchain::host(&scratch) else { return };
    for (binding, label) in [(Binding::Weak, "weak"), (Binding::Global, "global")] {
        let main = write(&scratch, &format!("main-{label}.o"), &comdat_main(binding, 42));
        let helper = write(&scratch, &format!("helper-{label}.o"), &comdat_helper(binding, 7));
        for linker in &tools.linkers {
            let program = scratch.join(format!("comdat-{label}-{}", linker.name));
            let ran = tools.link_and_run(linker, &[], &[&main, &helper], &program);
            assert_eq!(exit_code(&ran), Some(42), "{} {label}: main's group", linker.name);
            let ran = tools.link_and_run(linker, &[], &[&helper, &main], &program);
            assert_eq!(exit_code(&ran), Some(7), "{} {label}: helper's group", linker.name);
        }
    }
}

/// Without the group, the two global definitions of (c) do collide: the
/// control for (c).
#[test]
fn duplicate_global_definitions_without_comdat_fail() {
    let scratch = Scratch::new("obj-link-duplicate");
    let Some(tools) = Toolchain::host(&scratch) else { return };
    let build = |name: &str| {
        let mut obj = Object::new(Arch::AArch64);
        let text = obj.add_section(b".text", SectionKind::Text, 4).unwrap();
        function(&mut obj, text, "answer", &[MOV_W0_42, RET]);
        function(&mut obj, text, name, &[MOV_W0_42, RET]);
        obj
    };
    let first = write(&scratch, "first.o", &build("main"));
    let second = write(&scratch, "second.o", &build("other"));
    for linker in &tools.linkers {
        let linked = tools.link(linker, &[], &[&first, &second], &scratch.join("duplicate"));
        assert!(!linked.success(), "{}", linker.name);
        let message = linked.stderr_text();
        assert!(
            message.contains("multiple definition") || message.contains("duplicate symbol"),
            "{}: {message}",
            linker.name
        );
    }
}

/// (d) A pointer in `.data` initialized with an address: `ABS64` against a
/// global symbol, and against a section symbol with an addend.
#[test]
fn data_relocations() {
    let scratch = Scratch::new("obj-link-data");
    let Some(tools) = Toolchain::host(&scratch) else { return };
    let build = |through_section_symbol: bool| {
        let mut obj = Object::new(Arch::AArch64);
        let text = obj.add_section(b".text", SectionKind::Text, 4).unwrap();
        let rodata = obj.add_section(b".rodata", SectionKind::ReadOnlyData, 4).unwrap();
        let data = obj.add_section(b".data", SectionKind::Data, 8).unwrap();
        // const int values[2] = {13, 42}; const int *pointer = &values[1];
        obj.append(rodata, &13u32.to_le_bytes());
        let at = obj.append(rodata, &42u32.to_le_bytes());
        let slot = obj.append_zeros(data, 8);
        let name = obj.add_string(b"pointer").unwrap();
        let pointer = obj.add_symbol(Symbol::defined(name, SymbolKind::Object, data, slot, 8));
        if through_section_symbol {
            let section = obj.section_symbol(rodata);
            obj.add_relocation(data, slot, A64Reloc::Abs64, section, at as i64);
        } else {
            let name = obj.add_string(b"answer").unwrap();
            let answer = obj.add_symbol(Symbol::defined(name, SymbolKind::Object, rodata, at, 4));
            obj.add_relocation(data, slot, A64Reloc::Abs64, answer, 0);
        }
        // return *pointer;
        function(&mut obj, text, "main", &[ADRP_X0, LDR_X0_LO12, LDR_W0_LO12, RET]);
        obj.add_relocation(text, 0, A64Reloc::AdrPrelPgHi21, pointer, 0);
        obj.add_relocation(text, 4, A64Reloc::Ldst64AbsLo12Nc, pointer, 0);
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

/// A zero-initialized variable with a large alignment, in a section that
/// occupies no file space and sits between two that do: `main` returns 42
/// only if the variable is zero and its address is 4096-aligned.
#[test]
fn bss_is_zero_and_aligned() {
    let scratch = Scratch::new("obj-link-bss");
    let Some(tools) = Toolchain::host(&scratch) else { return };
    let mut obj = Object::new(Arch::AArch64);
    let text = obj.add_section(b".text", SectionKind::Text, 4).unwrap();
    let bss = obj.add_section(b".bss", SectionKind::Bss, 1).unwrap();
    let data = obj.add_section(b".data", SectionKind::Data, 8).unwrap();
    obj.append(data, &[0xff; 8]);
    obj.append_zeros(bss, 3);
    let at = obj.align_to(bss, 4096, 0);
    obj.append_zeros(bss, 8);
    let name = obj.add_string(b"zeroed").unwrap();
    let zeroed = obj.add_symbol(Symbol::defined(name, SymbolKind::Object, bss, at, 8));
    let code = [
        ADRP_X0,
        ADD_X0_LO12,
        0x1200_2c01, // and w1, w0, #0xfff
        LDR_W0_LO12, // ldr w0, [x0]
        0x0b01_0000, // add w0, w0, w1
        0x1100_a800, // add w0, w0, #42
        RET,
    ];
    function(&mut obj, text, "main", &code);
    obj.add_relocation(text, 0, A64Reloc::AdrPrelPgHi21, zeroed, 0);
    obj.add_relocation(text, 4, A64Reloc::AddAbsLo12Nc, zeroed, 0);
    let object = write(&scratch, "bss.o", &obj);
    for linker in &tools.linkers {
        let program = scratch.join(format!("bss-{}", linker.name));
        let ran = tools.link_and_run(linker, &[], &[&object], &program);
        assert_eq!(exit_code(&ran), Some(42), "{}", linker.name);
    }
}

/// (e) 70 000 function sections: `main`, in section 70 001, jumps to the
/// function in section 70 000, which returns 42. Both sections' indices,
/// the section count and the section name table index need the ELF
/// escapes for values at or above 0xff00.
#[test]
fn more_than_65k_sections_link() {
    let scratch = Scratch::new("obj-link-many");
    let Some(tools) = Toolchain::host(&scratch) else { return };
    let object = write(&scratch, "many.o", &common::many_sections(70_000));
    for linker in &tools.linkers {
        let program = scratch.join(format!("many-{}", linker.name));
        let ran = tools.link_and_run(linker, &[], &[&object], &program);
        assert_eq!(exit_code(&ran), Some(42), "{}", linker.name);
    }
}

/// `mov w0, #value`
fn mov_w0(value: u32) -> u32 {
    0x5280_0000 | value << 5
}

/// "Section groups": a COMDAT group with a member that is not loaded, as
/// `.debug_types` units are, and references into the group from outside it
/// by sections that describe code: debug sections (`ABS64` against the
/// member's section symbol and against a local symbol in it) and
/// `.eh_frame` (`PREL32` against the section symbol). Both objects carry
/// the group; the references of the copy that is discarded are tolerated
/// by all linkers, and that copy's members are gone from the program.
#[test]
fn discarded_comdat_copies_may_be_referenced_by_debug_sections() {
    let scratch = Scratch::new("obj-link-group-debug");
    let Some(tools) = Toolchain::host(&scratch) else { return };
    let describe = |mut obj: Object, tag: &str| {
        let answer = obj.symbol_count() - 1;
        let answer = (0..=answer).map(hod_obj::SymbolId::from_index);
        let answer = answer.rev().find(|&id| obj.string(obj.symbol(id).name) == b"answer").unwrap();
        let SymbolPlace::Section(text) = obj.symbol(answer).place else { unreachable!() };
        let group = obj.section_group(text).unwrap();
        let member = obj.add_section(b".debug_hod.types", SectionKind::Debug, 1).unwrap();
        obj.append(member, format!("IN-GROUP-{tag}").as_bytes());
        obj.add_group_member(group, member).unwrap();
        let section = obj.section_symbol(text);
        let local = define(&mut obj, ".Lanswer", SymbolKind::NoType, Binding::Local, text, 4, 0);
        let outside = obj.add_section(b".debug_hod.addr", SectionKind::Debug, 1).unwrap();
        obj.append_zeros(outside, 16);
        obj.append(outside, format!("OUTSIDE-{tag}").as_bytes());
        obj.add_relocation(outside, 0, A64Reloc::Abs64, section, 8);
        obj.add_relocation(outside, 8, A64Reloc::Abs64, local, 0);
        // One CIE (CFA = sp) and one FDE for the twelve bytes of `answer`.
        #[rustfmt::skip]
        let frames = [
            0x10, 0, 0, 0,  0, 0, 0, 0,  1, b'z', b'R', 0,  1, 0x7c, 30, 1,  0x1b, 0x0c, 31, 0,
            0x10, 0, 0, 0,  0x18, 0, 0, 0,  0, 0, 0, 0,  12, 0, 0, 0,  0, 0, 0, 0,
        ];
        let eh_frame = obj.add_section(b".eh_frame", SectionKind::ReadOnlyData, 8).unwrap();
        obj.append(eh_frame, &frames);
        obj.add_relocation(eh_frame, 0x1c, A64Reloc::Prel32, section, 0);
        obj
    };
    let main = write(&scratch, "main.o", &describe(comdat_main(Binding::Weak, 42), "MAIN"));
    let helper = write(&scratch, "helper.o", &describe(comdat_helper(Binding::Weak, 7), "HELPER"));
    for linker in &tools.linkers {
        let program = scratch.join(format!("group-debug-{}", linker.name));
        for (objects, kept, discarded, value) in
            [([&main, &helper], "MAIN", "HELPER", 42), ([&helper, &main], "HELPER", "MAIN", 7)]
        {
            let ran = tools.link_and_run(linker, &[], &objects.map(PathBuf::as_path), &program);
            assert_eq!(exit_code(&ran), Some(value), "{}", linker.name);
            let has = |marker: String| find_marker(&program, &marker).is_some();
            assert!(has(format!("IN-GROUP-{kept}")), "{}", linker.name);
            assert!(!has(format!("IN-GROUP-{discarded}")), "{}", linker.name);
            assert!(has(format!("OUTSIDE-{kept}")), "{}", linker.name);
            assert!(has(format!("OUTSIDE-{discarded}")), "{}", linker.name);
        }
    }
}

/// An object with a group of the given kind whose signature is the weak
/// function `shared` (which returns 42); the group also holds a read-only
/// section with the text `GROUP-<tag>`. With `with_main`, `main` jumps to
/// `shared`.
fn tagged_group(kind: GroupKind, tag: &str, with_main: bool) -> Object {
    let mut obj = Object::new(Arch::AArch64);
    let text = obj.add_section(b".text.shared", SectionKind::Text, 4).unwrap();
    let rodata = obj.add_section(b".rodata.shared", SectionKind::ReadOnlyData, 1).unwrap();
    obj.append(rodata, format!("GROUP-{tag}").as_bytes());
    let shared = function(&mut obj, text, "shared", &[MOV_W0_42, RET]);
    obj.symbol_mut(shared).binding = Binding::Weak;
    let group = obj.add_group(shared, kind);
    obj.add_group_member(group, text).unwrap();
    obj.add_group_member(group, rodata).unwrap();
    if with_main {
        let text = obj.add_section(b".text", SectionKind::Text, 4).unwrap();
        function(&mut obj, text, "main", &[B]);
        obj.add_relocation(text, 0, A64Reloc::Jump26, shared, 0);
    }
    obj
}

/// "Section groups": `GroupKind::Plain` groups with one signature in two
/// objects are both kept, where the linker keeps one of two COMDAT groups;
/// and when unused sections are discarded, a group's member that nothing
/// refers to stays with the member that is used (GNU ld and lld; mold
/// decides per section).
#[test]
fn plain_groups_are_not_deduplicated() {
    let scratch = Scratch::new("obj-link-group-plain");
    let Some(tools) = Toolchain::host(&scratch) else { return };
    for (kind, label) in [(GroupKind::Plain, "plain"), (GroupKind::Comdat, "comdat")] {
        let first = write(&scratch, &format!("{label}-a.o"), &tagged_group(kind, "FIRST", true));
        let second = write(&scratch, &format!("{label}-b.o"), &tagged_group(kind, "SECOND", false));
        for linker in &tools.linkers {
            let program = scratch.join(format!("{label}-{}", linker.name));
            let ran = tools.link_and_run(linker, &[], &[&first, &second], &program);
            assert_eq!(exit_code(&ran), Some(42), "{} {label}", linker.name);
            assert!(find_marker(&program, "GROUP-FIRST").is_some(), "{} {label}", linker.name);
            let second_kept = find_marker(&program, "GROUP-SECOND").is_some();
            assert_eq!(second_kept, kind == GroupKind::Plain, "{} {label}", linker.name);

            let flags = ["-Wl,--gc-sections"];
            let ran = tools.link_and_run(linker, &flags, &[&first, &second], &program);
            assert_eq!(exit_code(&ran), Some(42), "{} {label} gc", linker.name);
            if linker.keeps_associated_sections() {
                assert!(find_marker(&program, "GROUP-FIRST").is_some(), "{} gc", linker.name);
            }
        }
    }
}

/// "Sections", linked sections: three functions in sections of their own,
/// each with a record in a `.hod.meta` section linked to it, the records
/// added in the opposite order. `main` calls the first and the third.
/// The linker lays the records out in the order of their functions and,
/// discarding unused sections, drops the second function's record with it
/// and keeps the others, which nothing else refers to. Without the link
/// (the control) no record survives. mold 2.37 does not implement
/// `SHF_LINK_ORDER`: it must still link the object, and drops the records.
#[test]
fn linked_sections_follow_their_targets() {
    let scratch = Scratch::new("obj-link-order");
    let Some(tools) = Toolchain::host(&scratch) else { return };
    let build = |linked: bool| {
        let mut obj = Object::new(Arch::AArch64);
        let mut functions = Vec::new();
        for (index, name) in ["one", "two", "three"].into_iter().enumerate() {
            let section = format!(".text.{name}");
            let text = obj.add_section(section.as_bytes(), SectionKind::Text, 4).unwrap();
            let symbol = function(&mut obj, text, name, &[mov_w0(index as u32 + 1), RET]);
            functions.push((text, symbol, name));
        }
        for &(text, _, name) in functions.iter().rev() {
            let meta = obj.add_section(b".hod.meta", SectionKind::ReadOnlyData, 1).unwrap();
            obj.append(meta, format!("META-{name};").as_bytes());
            if linked {
                obj.set_linked_section(meta, text);
            }
        }
        let text = obj.add_section(b".text.main", SectionKind::Text, 4).unwrap();
        function(&mut obj, text, "main", &[PUSH_FRAME, SET_FP, BL, BL, MOV_W0_42, POP_FRAME, RET]);
        obj.add_relocation(text, 8, A64Reloc::Call26, functions[0].1, 0);
        obj.add_relocation(text, 12, A64Reloc::Call26, functions[2].1, 0);
        obj
    };
    let linked = write(&scratch, "linked.o", &build(true));
    let unlinked = write(&scratch, "unlinked.o", &build(false));
    for linker in &tools.linkers {
        let program = scratch.join(format!("order-{}", linker.name));
        let records = |object, flags: &[&str]| {
            let ran = tools.link_and_run(linker, flags, &[object], &program);
            assert_eq!(exit_code(&ran), Some(42), "{}", linker.name);
            ["META-one;", "META-two;", "META-three;"].map(|marker| find_marker(&program, marker))
        };
        let gc = ["-Wl,--gc-sections"];
        assert_eq!(records(&unlinked, &gc), [None; 3], "{}: the control", linker.name);
        let [one, two, three] = records(&linked, &[]);
        assert!(one.is_some() && two.is_some() && three.is_some(), "{}", linker.name);
        if linker.keeps_associated_sections() {
            assert!(one < two && two < three, "{}: in the functions' order", linker.name);
        }
        let [one, two, three] = records(&linked, &gc);
        assert_eq!(two, None, "{}: discarded with its function", linker.name);
        if linker.keeps_associated_sections() {
            assert!(one.is_some() && one < three, "{}: kept with their functions", linker.name);
        }
    }
}

/// "Section kinds", mergeable sections: two objects each hold the string
/// `shared` in `.rodata.str1.1` and the constant 42.0 in `.rodata.cst8`,
/// at different offsets and among other entries. `main` returns 1 if the
/// two strings have one address, plus 2 if the two constants do: 3 when
/// the linker merged both sections, which it does only for the mergeable
/// kinds with the right entry size, and 0 for plain read-only sections
/// (the control).
#[test]
fn mergeable_sections_are_merged() {
    let scratch = Scratch::new("obj-link-merge");
    let Some(tools) = Toolchain::host(&scratch) else { return };
    let build = |mergeable: bool, is_main: bool| {
        let mut obj = Object::new(Arch::AArch64);
        let (strings, constants) = if mergeable {
            let strings = SectionKind::MergeableStrings { char_size: 1 };
            (strings, SectionKind::MergeableConstants { entry_size: 8 })
        } else {
            (SectionKind::ReadOnlyData, SectionKind::ReadOnlyData)
        };
        let strings = obj.add_section(b".rodata.str1.1", strings, 1).unwrap();
        let constants = obj.add_section(b".rodata.cst8", constants, 8).unwrap();
        let (prefix, binding, before, after, other): (_, _, &[u8], &[u8], f64) = if is_main {
            ("a", Binding::Local, b"only in main\0", b"", 1.0)
        } else {
            ("b", Binding::Global, b"", b"only in other\0", 7.0)
        };
        obj.append(strings, before);
        let string = obj.append(strings, b"shared\0");
        obj.append(strings, after);
        if is_main {
            obj.append(constants, &other.to_le_bytes());
        }
        let constant = obj.append(constants, &42f64.to_le_bytes());
        if !is_main {
            obj.append(constants, &other.to_le_bytes());
        }
        let object = SymbolKind::Object;
        define(&mut obj, &format!("{prefix}_string"), object, binding, strings, string, 7);
        define(&mut obj, &format!("{prefix}_constant"), object, binding, constants, constant, 8);
        if is_main {
            let text = obj.add_section(b".text", SectionKind::Text, 4).unwrap();
            let code = [
                ADRP_X0,
                ADD_X0_LO12,
                0x9000_0001, // adrp x1, b_string
                0x9100_0021, // add x1, x1, #lo12
                0x9000_0002, // adrp x2, a_constant
                0x9100_0042, // add x2, x2, #lo12
                0x9000_0003, // adrp x3, b_constant
                0x9100_0063, // add x3, x3, #lo12
                0xeb01_001f, // cmp x0, x1
                0x1a9f_17e4, // cset w4, eq
                0xeb03_005f, // cmp x2, x3
                0x1a9f_17e5, // cset w5, eq
                0x0b05_0480, // add w0, w4, w5, lsl #1
                RET,
            ];
            function(&mut obj, text, "main", &code);
            let theirs = ["b_string", "b_constant"].map(|name| undefined(&mut obj, name));
            let ours = [0, 1].map(hod_obj::SymbolId::from_index);
            for (index, symbol) in [ours[0], theirs[0], ours[1], theirs[1]].into_iter().enumerate()
            {
                let at = 8 * index as u64;
                obj.add_relocation(text, at, A64Reloc::AdrPrelPgHi21, symbol, 0);
                obj.add_relocation(text, at + 4, A64Reloc::AddAbsLo12Nc, symbol, 0);
            }
        }
        obj
    };
    for (mergeable, label, expected) in [(true, "merge", 3), (false, "plain", 0)] {
        let main = write(&scratch, &format!("{label}-main.o"), &build(mergeable, true));
        let other = write(&scratch, &format!("{label}-other.o"), &build(mergeable, false));
        for linker in &tools.linkers {
            let program = scratch.join(format!("{label}-{}", linker.name));
            let ran = tools.link_and_run(linker, &[], &[&main, &other], &program);
            assert_eq!(exit_code(&ran), Some(expected), "{} {label}", linker.name);
        }
    }
}
