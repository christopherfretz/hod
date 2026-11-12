//! `spec/object.md` "Testing", link-and-run: objects the writer produces
//! are linked by GNU ld, lld and mold, and the programs run.
//!
//! The code in the objects is hand-encoded AArch64, so these tests exist
//! only on an AArch64 Linux host. A missing linker skips its part with a
//! note, or fails under `HOD_REQUIRE_TOOLS=1`.

#![cfg(all(target_arch = "aarch64", target_os = "linux"))]

mod common;

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use common::{
    ADD_X0_LO12, ADRP_X0, B, BL, LDR_W0_LO12, LDR_X0_LO12, POP_FRAME, PUSH_FRAME, RET, SET_FP,
    function, undefined,
};
use hod_obj::{A64Reloc, Arch, Binding, Object, SectionKind, Symbol, SymbolKind, elf};
use hod_testutil::{Outcome, Scratch, find_tool, run, run_with, tool};

/// One way to link: the linker's name and the compiler-driver flags that
/// select it.
struct Linker {
    name: &'static str,
    flags: Vec<OsString>,
}

/// The compiler driver, and the linkers that are present.
///
/// `gcc -fuse-ld=<name>` runs the program `ld.<name>` it finds on its
/// program path, so each linker gets a directory holding exactly that
/// name, passed with `-B`. This selects the linker `hod_testutil` found
/// whatever it is called and wherever it is installed (`ld.lld-22`,
/// `/usr/lib/llvm-22/bin/ld.lld`).
fn toolchain(scratch: &Scratch) -> Option<(PathBuf, Vec<Linker>)> {
    let gcc = tool("gcc")?;
    let mut linkers = Vec::new();
    for name in ["bfd", "lld", "mold"] {
        let program = format!("ld.{name}");
        let Some(found) = tool(&program) else { continue };
        let dir = scratch.join(format!("use-{name}"));
        std::fs::create_dir(&dir).unwrap();
        std::os::unix::fs::symlink(&found, dir.join(&program)).unwrap();
        let mut prefix = OsString::from("-B");
        prefix.push(&dir);
        linkers.push(Linker { name, flags: vec![prefix, format!("-fuse-ld={name}").into()] });
    }
    Some((gcc, linkers))
}

/// Links objects into a program. Linker warnings are errors.
fn link(gcc: &Path, linker: &Linker, objects: &[&Path], output: &Path) -> Outcome {
    let mut command = Command::new(gcc);
    command.args(&linker.flags).arg("-Wl,--fatal-warnings").arg("-o").arg(output).args(objects);
    run_with(&mut command, &[], Duration::from_mins(3))
}

/// Links, requiring success and silence, then runs the program.
fn link_and_run(gcc: &Path, linker: &Linker, objects: &[&Path], output: &Path) -> Outcome {
    let outcome = link(gcc, linker, objects, output);
    assert!(
        outcome.success(),
        "{}: link failed (timed out: {}):\n{}",
        linker.name,
        outcome.timed_out(),
        outcome.stderr_text()
    );
    assert_eq!(outcome.stderr_text(), "", "{}: the link printed diagnostics", linker.name);
    // Which linker really ran is recorded in the program's `.comment`.
    if let Some(readelf) = find_tool("readelf") {
        let flags = ["-p", ".comment"].map(OsStr::new);
        let comment = run(readelf, flags.into_iter().chain([output.as_os_str()])).stdout_text();
        let (signature, others) = match linker.name {
            "lld" => ("LLD", ["mold", "mold"]),
            "mold" => ("mold", ["LLD", "LLD"]),
            _ => ("GCC", ["LLD", "mold"]),
        };
        assert!(comment.contains(signature), "{} did not link this:\n{comment}", linker.name);
        assert!(!others.iter().any(|other| comment.contains(other)), "{}: {comment}", linker.name);
    }
    let ran = run(output, [] as [&str; 0]);
    assert!(!ran.timed_out(), "{}: the program did not finish", linker.name);
    eprintln!("link+run with {:<4}: exit {:?}", linker.name, ran.status.and_then(|s| s.code()));
    ran
}

fn exit_code(outcome: &Outcome) -> Option<i32> {
    outcome.status.and_then(|status| status.code())
}

fn write(scratch: &Scratch, name: &str, obj: &Object) -> PathBuf {
    scratch.write(name, elf::to_vec(obj).unwrap())
}

/// (a) `main` returns 42.
#[test]
fn main_returns_42() {
    let scratch = Scratch::new("obj-link-ret42");
    let Some((gcc, linkers)) = toolchain(&scratch) else { return };
    let object = write(&scratch, "ret42.o", &common::return_42());
    for linker in &linkers {
        let program = scratch.join(format!("ret42-{}", linker.name));
        let ran = link_and_run(&gcc, linker, &[&object], &program);
        assert_eq!(exit_code(&ran), Some(42), "{}", linker.name);
    }
}

/// (b) `main` passes a string in `.rodata` to `puts`: page-relative
/// addressing of a local symbol, and a call through the PLT.
#[test]
fn main_calls_puts() {
    let scratch = Scratch::new("obj-link-hello");
    let Some((gcc, linkers)) = toolchain(&scratch) else { return };
    let object = write(&scratch, "hello.o", &common::hello("hello from hod"));
    for linker in &linkers {
        let program = scratch.join(format!("hello-{}", linker.name));
        let ran = link_and_run(&gcc, linker, &[&object], &program);
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
    let Some((gcc, linkers)) = toolchain(&scratch) else { return };
    for (binding, label) in [(Binding::Weak, "weak"), (Binding::Global, "global")] {
        let main = write(&scratch, &format!("main-{label}.o"), &comdat_main(binding, 42));
        let helper = write(&scratch, &format!("helper-{label}.o"), &comdat_helper(binding, 7));
        for linker in &linkers {
            let program = scratch.join(format!("comdat-{label}-{}", linker.name));
            let ran = link_and_run(&gcc, linker, &[&main, &helper], &program);
            assert_eq!(exit_code(&ran), Some(42), "{} {label}: main's group", linker.name);
            let ran = link_and_run(&gcc, linker, &[&helper, &main], &program);
            assert_eq!(exit_code(&ran), Some(7), "{} {label}: helper's group", linker.name);
        }
    }
}

/// Without the group, the two global definitions of (c) do collide: the
/// control for (c).
#[test]
fn duplicate_global_definitions_without_comdat_fail() {
    let scratch = Scratch::new("obj-link-duplicate");
    let Some((gcc, linkers)) = toolchain(&scratch) else { return };
    let build = |name: &str| {
        let mut obj = Object::new(Arch::AArch64);
        let text = obj.add_section(b".text", SectionKind::Text, 4).unwrap();
        function(&mut obj, text, "answer", &[common::MOV_W0_42, RET]);
        function(&mut obj, text, name, &[common::MOV_W0_42, RET]);
        obj
    };
    let first = write(&scratch, "first.o", &build("main"));
    let second = write(&scratch, "second.o", &build("other"));
    for linker in &linkers {
        let linked = link(&gcc, linker, &[&first, &second], &scratch.join("duplicate"));
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
    let Some((gcc, linkers)) = toolchain(&scratch) else { return };
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
        for linker in &linkers {
            let program = scratch.join(format!("data-{label}-{}", linker.name));
            let ran = link_and_run(&gcc, linker, &[&object], &program);
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
    let Some((gcc, linkers)) = toolchain(&scratch) else { return };
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
    for linker in &linkers {
        let program = scratch.join(format!("bss-{}", linker.name));
        let ran = link_and_run(&gcc, linker, &[&object], &program);
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
    let Some((gcc, linkers)) = toolchain(&scratch) else { return };
    let object = write(&scratch, "many.o", &common::many_sections(70_000));
    for linker in &linkers {
        let program = scratch.join(format!("many-{}", linker.name));
        let ran = link_and_run(&gcc, linker, &[&object], &program);
        assert_eq!(exit_code(&ran), Some(42), "{}", linker.name);
    }
}
