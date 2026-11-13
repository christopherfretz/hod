//! `spec/object.md` "ELF conventions", link-and-run: initializer and
//! finalizer arrays and the GNU property note, for AArch64 (on an AArch64
//! Linux host) and for x86-64 (anywhere the x86-64 toolchain and emulator
//! are, `Toolchain::x86_64`), with GNU ld, lld and mold. A missing tool
//! skips its part with a note, or fails under `HOD_REQUIRE_TOOLS=1`.

#![cfg(target_os = "linux")]

mod common;

use std::path::Path;

use common::linking::{Toolchain, exit_code, write};
use common::{BTI_C, Say, function_bytes, say, words};
use hod_obj::{A64Reloc, Arch, Object, RelocKind, SectionKind, X64Reloc, elf};
use hod_testutil::{Scratch, Target, run, tool};

fn abs64(arch: Arch) -> RelocKind {
    match arch {
        Arch::AArch64 => A64Reloc::Abs64.into(),
        Arch::X86_64 => X64Reloc::Abs64.into(),
    }
}

/// `add_init_array` and `add_fini_array`: initializers run before `main`
/// in increasing priority order and finalizers after it in decreasing
/// order, the default priority being the largest. The sections are added
/// in another order, and 1000 sorts before 200 as text: the linker orders
/// them by the number in the name.
fn init_and_fini_arrays(tools: &Toolchain, scratch: &Scratch, arch: Arch) {
    let mut obj = Object::new(arch);
    let at = Say::new(&mut obj);
    say(&mut obj, at, "main", "main");
    for priority in [elf::DEFAULT_INIT_PRIORITY, 30000, 200, 1000] {
        for kind in ["init", "fini"] {
            let function =
                say(&mut obj, at, &format!("{kind}_{priority}"), &format!("{kind} {priority}"));
            let array = match kind {
                "init" => elf::add_init_array(&mut obj, priority),
                _ => elf::add_fini_array(&mut obj, priority),
            };
            let array = array.unwrap();
            let slot = obj.append_zeros(array, 8);
            obj.add_relocation(array, slot, abs64(arch), function, 0);
        }
    }
    let object = write(scratch, "arrays.o", &obj);
    for linker in &tools.linkers {
        let program = scratch.join(format!("arrays-{}", linker.name));
        let ran = tools.link_and_run(linker, &[], &[&object], &program);
        assert_eq!(
            ran.stdout_text(),
            "init 200\ninit 1000\ninit 30000\ninit 65535\nmain\n\
             fini 65535\nfini 30000\nfini 1000\nfini 200\n",
            "{}",
            linker.name
        );
        assert_eq!(exit_code(&ran), Some(0), "{}", linker.name);
    }
}

#[test]
fn aarch64_init_and_fini_arrays_run_in_priority_order() {
    let scratch = Scratch::new("obj-arrays-a64");
    let Some(tools) = Toolchain::for_target(&scratch, Target::Aarch64) else { return };
    init_and_fini_arrays(&tools, &scratch, Arch::AArch64);
}

#[test]
fn x86_64_init_and_fini_arrays_run_in_priority_order() {
    let scratch = Scratch::new("obj-arrays-x64");
    let Some(tools) = Toolchain::x86_64(&scratch) else { return };
    init_and_fini_arrays(&tools, &scratch, Arch::X86_64);
}

/// An object with one function, and the property note when `features` is
/// given. `_start` is a whole program: it exits with status 42 through the
/// system call, so that no other object is linked in.
fn with_note(arch: Arch, name: &str, features: Option<u32>) -> Object {
    let mut obj = Object::new(arch);
    let text = obj.add_section(b".text", SectionKind::Text, 16).unwrap();
    let code = match arch {
        // bti c; mov x0, #42; mov x8, #93 (exit); svc #0
        Arch::AArch64 => words(&[BTI_C, 0xd280_0540, 0xd280_0ba8, 0xd400_0001]),
        // endbr64; mov $42, %edi; mov $60, %eax (exit); syscall
        Arch::X86_64 => {
            vec![0xf3, 0x0f, 0x1e, 0xfa, 0xbf, 42, 0, 0, 0, 0xb8, 60, 0, 0, 0, 0x0f, 0x05]
        }
    };
    function_bytes(&mut obj, text, name, &code);
    if let Some(features) = features {
        elf::add_gnu_property_note(&mut obj, features).unwrap();
    }
    obj
}

/// `add_gnu_property_note`: the linker gives the program a feature exactly
/// when every object declares it. mold 2.37 does that for x86-64 only: it
/// leaves the AArch64 property out of the program whatever compiler made
/// the objects, so there only its absence without the notes is checked.
fn property_note(
    tools: &Toolchain,
    scratch: &Scratch,
    arch: Arch,
    features: u32,
    names: [&str; 2],
) {
    let Some(readelf) = tool("llvm-readelf") else { return };
    let start = write(scratch, "start.o", &with_note(arch, "_start", Some(features)));
    let marked = write(scratch, "marked.o", &with_note(arch, "other", Some(features)));
    let unmarked = write(scratch, "unmarked.o", &with_note(arch, "other", None));
    for linker in &tools.linkers {
        for (other, expected) in [(&marked, true), (&unmarked, false)] {
            let program = scratch.join(format!("note-{expected}-{}", linker.name));
            let flags = ["-nostdlib", "-static"];
            let ran = tools.link_and_run(linker, &flags, &[&start, other], &program);
            assert_eq!(exit_code(&ran), Some(42), "{}", linker.name);
            let notes = run(&readelf, [Path::new("--notes"), &program]).stdout_text();
            let propagates = arch == Arch::X86_64 || linker.name != "mold";
            for name in names {
                let found = notes.contains(name);
                assert!(!found || expected, "{}: {name} in:\n{notes}", linker.name);
                assert!(
                    found || !expected || !propagates,
                    "{}: no {name} in:\n{notes}",
                    linker.name
                );
            }
        }
    }
}

#[test]
fn aarch64_property_note_reaches_the_program() {
    let scratch = Scratch::new("obj-note-a64");
    let Some(tools) = Toolchain::for_target(&scratch, Target::Aarch64) else { return };
    let features =
        elf::GNU_PROPERTY_AARCH64_FEATURE_1_BTI | elf::GNU_PROPERTY_AARCH64_FEATURE_1_PAC;
    property_note(&tools, &scratch, Arch::AArch64, features, ["BTI", "PAC"]);
}

#[test]
fn x86_64_property_note_reaches_the_program() {
    let scratch = Scratch::new("obj-note-x64");
    let Some(tools) = Toolchain::x86_64(&scratch) else { return };
    let features = elf::GNU_PROPERTY_X86_FEATURE_1_IBT | elf::GNU_PROPERTY_X86_FEATURE_1_SHSTK;
    property_note(&tools, &scratch, Arch::X86_64, features, ["IBT", "SHSTK"]);
}
