//! The x86-64 skeleton (WP-116, `spec/codegen.md` §5.6): position-
//! independent programs made of what the skeleton lowers (constants,
//! `phi`s, `br`, `ret`, `unreachable`, direct calls, symbol addresses),
//! linked by GNU ld, lld and mold and run; the object read back by
//! `llvm-readelf` and `llvm-dwarfdump`, the CFI compared with design 0006
//! §4 row by row; the x86-64 data layout; the refusals.
//!
//! The cross runner (`x86_64-linux-gnu-gcc` and `qemu-x86_64 -L
//! /usr/x86_64-linux-gnu` on a non-x86 host) is `hod_obj`'s test helper,
//! included by path; WP-025 (the shared link-and-run harness) replaces
//! this `#[path]` include and [`programs`] with its own.

#[path = "../../hod_obj/tests/common/linking.rs"]
#[allow(dead_code, reason = "a shared helper: not every function is used here")]
mod linking;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use hod_cg::{CgError, Options, RelocModel};
use hod_ir::{Body, Module, TypeId};
use hod_testutil::{Scratch, run_with, tool};
use linking::{Toolchain, exit_code};

/// The module header: the x86-64 data layout and triple.
fn header() -> String {
    format!(
        "target datalayout = \"{}\"\ntarget triple = \"x86_64-unknown-linux-gnu\"\n",
        hod_ir::X86_64_LINUX_DATA_LAYOUT
    )
}

/// What clang writes for `-fPIE`: `PIC Level` 2, `PIE Level` 2.
const PIE_FLAGS: &str = "\n!llvm.module.flags = !{!0, !1}\n\
    !0 = !{i32 8, !\"PIC Level\", i32 2}\n!1 = !{i32 7, !\"PIE Level\", i32 2}\n";

/// A module and its bodies from `.ll` text (the header is added).
fn parse(body: &str) -> (Module, Vec<Body>) {
    let text = format!("{}{body}{PIE_FLAGS}", header());
    let parsed = hod_ll::parse_module(text.as_bytes(), &hod_ll::ParseOptions::default())
        .unwrap_or_else(|e| panic!("hod_ll: {e}\n{text}"));
    let text_side = parsed.text;
    let mut module = parsed.module;
    let mut bodies = Vec::new();
    hod_ll::parse_bodies(&mut module, &text_side, |_, parsed| {
        let body = parsed
            .body
            .unwrap_or_else(|| panic!("hod_ll: unsupported {:?} in\n{text}", parsed.unsupported));
        bodies.push(body.clone());
        Ok::<(), hod_ll::LlError>(())
    })
    .unwrap_or_else(|e| panic!("hod_ll: {e}\n{text}"));
    (module, bodies)
}

fn pic() -> Options {
    Options { reloc_model: RelocModel::Pic, ..Options::default() }
}

/// The object of a module, compiled position-independent.
fn object(body: &str) -> Result<Vec<u8>, hod_x64::ModuleError> {
    let (module, bodies) = parse(body);
    hod_x64::compile_to_object(&module, &bodies, &pic())
}

/// The refusal of a module of one construct.
fn refusal(body: &str) -> &'static str {
    match object(body) {
        Err(hod_x64::ModuleError::Function {
            error: CgError::Unsupported { construct }, ..
        }) => construct,
        other => panic!("not a refusal: {:?}", other.map(|bytes| bytes.len())),
    }
}

/// `main` returns 42.
const RET_42: &str = "define i32 @main() {\n  ret i32 42\n}\n";

/// A chain of blocks, one backward branch, and `phi`s that swap: the
/// `phi` copies must be parallel. Exit status 42.
const PHI_CHAIN: &str = "\
define i32 @main() {
entry:
  br label %c
b:
  %r = phi i32 [ %q2, %d ]
  ret i32 %r
c:
  %p = phi i32 [ 42, %entry ]
  %q = phi i32 [ 7, %entry ]
  br label %d
d:
  %p2 = phi i32 [ %q, %c ]
  %q2 = phi i32 [ %p, %c ]
  br label %b
}
";

/// A direct call with arguments and a result, then `exit` through the
/// PLT, then `unreachable`. Exit status 42.
const CALLS: &str = "\
declare void @exit(i32) noreturn

define internal i32 @second(i32 %a, i32 %b, i64 %c, ptr %d, i8 %e, i32 %f) {
  ret i32 %f
}

define i32 @main() {
  %v = call i32 @second(i32 1, i32 2, i64 3, ptr null, i8 5, i32 42)
  call void @exit(i32 %v) noreturn
  unreachable
}
";

/// Symbol addresses: a local global (`lea` with `R_X86_64_PC32`), a
/// preemptible one defined in C (`mov` from the GOT,
/// `R_X86_64_REX_GOTPCRELX`), with offsets. Checked by a C `main`.
const ADDRESSES: &str = "\
@local = dso_local global [4 x i32] zeroinitializer
@shared = external global [4 x i32]

define ptr @local_at_8() {
  ret ptr getelementptr (i8, ptr @local, i64 8)
}

define ptr @shared_at_4() {
  ret ptr getelementptr (i8, ptr @shared, i64 4)
}

define ptr @local_base() {
  ret ptr @local
}
";

const ADDRESSES_MAIN: &str = "\
int shared[4];
extern int local[4];
int *local_at_8(void);
int *shared_at_4(void);
int *local_base(void);
int main(void) {
    if (shared_at_4() != &shared[1]) return 1;
    if (local_at_8() != local_base() + 2) return 2;
    return 42;
}
";

/// The linked programs of the gate, with the exit status each must have.
fn programs() -> [(&'static str, &'static str, Option<&'static str>); 4] {
    [
        ("ret42", RET_42, None),
        ("phis", PHI_CHAIN, None),
        ("calls", CALLS, None),
        ("addresses", ADDRESSES, Some(ADDRESSES_MAIN)),
    ]
}

/// §5.6, the gate: every program links as a PIE with GNU ld, lld and
/// mold and exits with 42 under the target.
#[test]
fn pie_programs_link_and_run_with_every_linker() {
    let dir = Scratch::new("x64-skeleton");
    let Some(toolchain) = Toolchain::x86_64(&dir) else { return };
    assert!(
        !hod_testutil::tools_required() || toolchain.linkers.len() == 3,
        "bfd, lld and mold are all required"
    );
    for (name, body, c_main) in programs() {
        let bytes = object(body).unwrap_or_else(|e| panic!("{name}: {e}"));
        let object = dir.write(&format!("{name}.o"), bytes);
        let mut inputs = vec![object];
        if let Some(source) = c_main {
            inputs.push(toolchain.compile_c(&dir, &format!("{name}-main"), source, &["-fPIE"]));
        }
        let inputs: Vec<&Path> = inputs.iter().map(PathBuf::as_path).collect();
        for linker in &toolchain.linkers {
            let program = dir.join(format!("{name}-{}", linker.name));
            let ran = toolchain.link_and_run(linker, &["-pie"], &inputs, &program);
            assert_eq!(exit_code(&ran), Some(42), "{name}, {}: {}", linker.name, ran.stderr_text());
        }
    }
}

/// Runs an LLVM tool on `object`, or `None` with the reason printed when
/// it is missing.
fn llvm(tool_name: &str, args: &[&str], object: &Path) -> Option<String> {
    let Some(path) = tool(tool_name) else {
        eprintln!("note: {tool_name} not found; skipping");
        return None;
    };
    let mut command = Command::new(path);
    command.args(args).arg(object);
    let outcome = run_with(&mut command, &[], Duration::from_mins(1));
    assert!(outcome.success(), "{tool_name}: {}", outcome.stderr_text());
    assert_eq!(outcome.stderr_text(), "", "{tool_name} warned");
    Some(outcome.stdout_text())
}

/// §5.6: `llvm-readelf` reads every object without a warning, and the
/// relocations are the PIC ones.
#[test]
fn objects_read_back_clean() {
    let dir = Scratch::new("x64-readelf");
    for (name, body, _) in programs() {
        let object = dir.write(&format!("{name}.o"), object(body).unwrap());
        let Some(out) = llvm("llvm-readelf", &["-h", "-S", "-r", "-s"], &object) else { return };
        assert!(out.contains("Advanced Micro Devices X86-64"), "{out}");
        assert!(out.contains("REL (Relocatable file)"), "{out}");
        match name {
            "calls" => assert!(out.contains("R_X86_64_PLT32") && out.contains("exit"), "{out}"),
            "addresses" => {
                assert!(out.contains("R_X86_64_PC32"), "{out}");
                assert!(out.contains("R_X86_64_REX_GOTPCRELX"), "{out}");
            }
            _ => {}
        }
        assert!(!out.contains("R_X86_64_32 ") && !out.contains("R_X86_64_64"), "{out}");
    }
}

/// The CFI program of `main` in `ret42`, as `llvm-dwarfdump --eh-frame`
/// prints it: design 0006 §4, the prologue's rows then the epilogue's.
#[test]
fn cfi_is_design_section_4() {
    let dir = Scratch::new("x64-cfi");
    let object = dir.write("ret42.o", object(RET_42).unwrap());
    let Some(out) = llvm("llvm-dwarfdump", &["--eh-frame"], &object) else { return };
    let fde = out.split(" FDE ").nth(1).unwrap_or_else(|| panic!("no FDE:\n{out}"));
    let ops: Vec<&str> = fde
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with("DW_CFA_") && !line.starts_with("DW_CFA_nop"))
        .collect();
    let expect = [
        "DW_CFA_advance_loc: 1 to 0x1",
        "DW_CFA_def_cfa_offset: +16",
        "DW_CFA_offset: RBP -16",
        "DW_CFA_advance_loc: 3 to 0x4",
        "DW_CFA_def_cfa_register: RBP",
        "DW_CFA_advance_loc: 1 to 0x5",
        "DW_CFA_offset: RBX -24",
        "DW_CFA_advance_loc: 2 to 0x7",
        "DW_CFA_offset: R12 -32",
        "DW_CFA_advance_loc: 2 to 0x9",
        "DW_CFA_offset: R13 -40",
        "DW_CFA_advance_loc: 2 to 0xb",
        "DW_CFA_offset: R14 -48",
        "DW_CFA_advance_loc: 2 to 0xd",
        "DW_CFA_offset: R15 -56",
    ];
    assert_eq!(&ops[..expect.len()], expect, "{fde}");
    let rest = &ops[expect.len()..];
    assert_eq!(rest.len(), 2, "one row after the prologue's: {fde}");
    assert!(rest[0].starts_with("DW_CFA_advance_loc"), "{fde}");
    assert_eq!(rest[1], "DW_CFA_def_cfa: RSP +8", "{fde}");
}

/// The x86-64 data layout as hod's layout computes it: `i128` and
/// `x86_fp80` are 16 bytes with 16-byte alignment, pointers 8.
#[test]
fn data_layout_is_honoured() {
    let (mut module, _) = parse("define void @f(i128 %a, x86_fp80 %b, ptr %c) {\n  ret void\n}\n");
    let i128 = module.types.int(128).unwrap();
    let types = &module.types;
    for (ty, size, align) in [(i128, 16, 16), (TypeId::X86_FP80, 16, 16), (TypeId::PTR, 8, 8)] {
        assert_eq!(types.alloc_size_of(ty), size, "{ty:?}");
        assert_eq!(types.align_of(ty), align, "{ty:?}");
    }
}

/// Each group that has not landed refuses by its own name.
#[test]
fn groups_that_have_not_landed_refuse_by_name() {
    for (body, name) in [
        ("define i32 @f(i32 %a) {\n  %b = add i32 %a, 1\n  ret i32 %b\n}\n", "x64:todo:int"),
        ("define void @f(ptr %p) {\n  store i32 1, ptr %p\n  ret void\n}\n", "x64:todo:mem"),
        (
            "define void @f(i1 %c) {\n  br i1 %c, label %a, label %a\na:\n  ret void\n}\n",
            "x64:todo:control",
        ),
        ("define void @f() {\n  fence seq_cst\n  ret void\n}\n", "x64:todo:atomic"),
        (
            "define void @f(i32 %a, i32 %b, i32 %c, i32 %d, i32 %e, i32 %f, i32 %g) {\n  ret void\n}\n",
            "x64:todo:abi",
        ),
    ] {
        assert_eq!(refusal(body), name, "{body}");
    }
}
