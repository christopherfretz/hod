//! Module-level assembly made only of symbol directives (`spec/ir.md`
//! §2.3, `spec/codegen.md` §9.7.1, WP-075) against `llc`: each module is
//! emitted by `hod_cg::emit_module` and compiled by `llc` of each LLVM
//! major present, and the two objects must have the same sections,
//! symbols (name, binding, type, visibility, section) and relocations,
//! up to `elfview`'s normalizations. What `llc` warns about, fails on, or
//! resolves by a precedence rule of its assembler is refused by
//! `module:asm`.

mod borrow;
mod elfview;

use std::fmt::Write as _;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use elfview::{Elf, diff};
use hod_cg::{CgError, Options, RelocModel, emit_module};
use hod_ir::{
    AARCH64_LINUX_DATA_LAYOUT, AARCH64_LINUX_DATA_LAYOUT_LLVM19, X86_64_LINUX_DATA_LAYOUT,
};
use hod_obj::Arch;
use hod_testutil::{LLVM_MAJORS, Scratch, run_with, tools_required};

/// `(name, module text after the header)`: every directive and every
/// kind of symbol it may name.
const CASES: &[(&str, &str)] = &[
    // libstdc++'s `<iostream>`: the reference that links the library's
    // initializer.
    ("iostream", "module asm \".globl _ZSt21ios_base_library_initv\"\n"),
    ("empty-lines", "module asm \"\"\nmodule asm \"\"\nmodule asm \".globl U\"\n"),
    ("global", "module asm \".global U\"\n"),
    ("weak-new", "module asm \".weak U\"\n"),
    ("hidden-new", "module asm \".hidden U\"\n"),
    ("protected-new", "module asm \".protected U\"\n"),
    ("globl-hidden", "module asm \".globl U\"\nmodule asm \".hidden U\"\n"),
    ("hidden-then-protected", "module asm \".hidden U\"\nmodule asm \".protected U\"\n"),
    ("list", "module asm \".globl U, U2 ,U3\"\n"),
    (
        "comments",
        "module asm \"# hello\"\nmodule asm \"  .globl U // c\"\nmodule asm \"/* x */ .globl U3 ; .globl U4\"\n",
    ),
    ("type-new", "module asm \".type U, @function\"\n"),
    ("type-globl-new", "module asm \".globl U\"\nmodule asm \".type U, %object\"\n"),
    ("type-notype", "module asm \".type U, STT_NOTYPE\"\n"),
    ("weak-decl-used", "module asm \".weak U\"\ndeclare void @U()\n@keep = global ptr @U\n"),
    ("weak-decl-unused", "module asm \".weak U\"\ndeclare void @U()\n"),
    ("globl-decl-unused", "module asm \".globl U\"\ndeclare void @U()\n"),
    ("globl-decl-used", "module asm \".globl U\"\ndeclare void @U()\n@keep = global ptr @U\n"),
    ("weak-extern-weak", "module asm \".weak U\"\ndeclare extern_weak void @U()\n"),
    ("hidden-decl-used", "module asm \".hidden U\"\ndeclare void @U()\n@keep = global ptr @U\n"),
    (
        "protected-hidden-decl",
        "module asm \".protected U\"\ndeclare hidden void @U()\n@keep = global ptr @U\n",
    ),
    ("type-decl", "module asm \".type U, @function\"\ndeclare void @U()\n@keep = global ptr @U\n"),
    ("type-object-on-fn-decl", "module asm \".type U, @object\"\ndeclare void @U()\n"),
    (
        "type-var-decl",
        "module asm \".type V, @object\"\n@V = external global i32\n@keep = global ptr @V\n",
    ),
    ("globl-def", "module asm \".globl D\"\ndefine void @D() {\n  ret void\n}\n"),
    ("weak-weak-def", "module asm \".weak D\"\ndefine weak void @D() {\n  ret void\n}\n"),
    ("hidden-def", "module asm \".hidden D\"\ndefine void @D() {\n  ret void\n}\n"),
    ("protected-def", "module asm \".protected D\"\ndefine void @D() {\n  ret void\n}\n"),
    (
        "hidden-linkonce",
        "module asm \".hidden D\"\ndefine linkonce_odr void @D() {\n  ret void\n}\n@keep = global ptr @D\n",
    ),
    (
        "protected-hidden-def",
        "module asm \".protected D\"\ndefine hidden void @D() {\n  ret void\n}\n",
    ),
    ("type-fn-def", "module asm \".type D, %function\"\ndefine void @D() {\n  ret void\n}\n"),
    ("type-var-def", "module asm \".type V, @object\"\n@V = global i32 1\n"),
    ("hidden-var", "module asm \".hidden V\"\n@V = global i32 1\n"),
    ("hidden-alias", "module asm \".hidden A\"\n@v = global i32 0\n@A = alias i32, ptr @v\n"),
    ("globl-common", "module asm \".globl C\"\n@C = common global i32 0\n"),
    ("hidden-common", "module asm \".hidden C\"\n@C = common global i32 0\n"),
    ("weak-weak-var", "module asm \".weak V\"\n@V = weak global i32 1\n"),
    // A function that refers to a name only the assembly mentions.
    (
        "weak-call",
        "module asm \".weak hook\"\ndeclare void @hook()\ndefine void @D() {\n  call void @hook()\n  ret void\n}\n",
    ),
];

/// Module text `llc` warns about or refuses, or whose effect depends on
/// its assembler's precedence rules: `hod` refuses each as `module:asm`.
const REFUSED: &[&str] = &[
    "module asm \"  nop\"\n",
    "module asm \"f:\"\n",
    "module asm \".text\"\n",
    "module asm \".set x, 42\"\n",
    "module asm \"x = 42\"\n",
    "module asm \".symver a, a@V1\"\n",
    "module asm \".size U, 4\"\n",
    "module asm \".internal U\"\n",
    "module asm \".globl \\22a b\\22\"\n",
    "module asm \".globl U\"\nmodule asm \".weak U\"\n",
    "module asm \".type U, @gnu_indirect_function\"\n",
    "module asm \".pushsection .rodata\"\nmodule asm \".popsection\"\n",
    // `llc`: "changed binding to STB_GLOBAL" (an error).
    "module asm \".weak D\"\ndefine void @D() {\n  ret void\n}\n",
    // `llc`: "changed binding to STB_WEAK" (a warning).
    "module asm \".globl D\"\ndefine weak void @D() {\n  ret void\n}\n",
    "module asm \".globl U\"\ndeclare extern_weak void @U()\n@keep = global ptr @U\n",
    // A local symbol: `llc` makes it global.
    "module asm \".globl D\"\ndefine internal void @D() {\n  ret void\n}\n@keep = global ptr @D\n",
    "module asm \".hidden P\"\n@P = private global i32 1\n@keep = global ptr @P\n",
    // Types `llc` merges by precedence.
    "module asm \".type V, @function\"\n@V = global i32 1\n",
    "module asm \".type T, @object\"\n@T = external thread_local global i32\n",
    "module asm \".weak C\"\n@C = common global i32 0\n",
];

/// The module header of `arch` for `llc` of LLVM `major`.
fn header(arch: Arch, major: u32) -> String {
    let (layout, triple) = match arch {
        Arch::AArch64 if major >= 20 => (AARCH64_LINUX_DATA_LAYOUT, "aarch64-unknown-linux-gnu"),
        Arch::AArch64 => (AARCH64_LINUX_DATA_LAYOUT_LLVM19, "aarch64-unknown-linux-gnu"),
        Arch::X86_64 => (X86_64_LINUX_DATA_LAYOUT, "x86_64-unknown-linux-gnu"),
    };
    format!("target datalayout = \"{layout}\"\ntarget triple = \"{triple}\"\n")
}

/// Cases for x86-64 only (WP-143): its comment character.
const X64_CASES: &[(&str, &str)] =
    &[("hash-comment", "module asm \"# a comment\"\nmodule asm \".globl U # and another\"\n")];

fn llc_path(major: u32) -> Option<PathBuf> {
    let installed = PathBuf::from(format!("/usr/lib/llvm-{major}/bin/llc"));
    installed.is_file().then_some(installed)
}

/// `llc`'s object for `text`, which it must compile without a word.
fn llc(scratch: &Scratch, major: u32, name: &str, text: &str) -> Vec<u8> {
    let input = scratch.write(&format!("{name}-{major}.ll"), text);
    let output = scratch.join(format!("{name}-{major}.o"));
    let mut command = Command::new(llc_path(major).unwrap());
    command
        .args(["-O0", "-global-isel=0", "-fast-isel=0", "-filetype=obj", "-relocation-model=pic"])
        .arg(&input)
        .arg("-o")
        .arg(&output);
    let outcome = run_with(&mut command, &[], Duration::from_mins(1));
    assert!(outcome.success(), "{name}: llc-{major} failed:\n{}", outcome.stderr_text());
    assert!(outcome.stderr_text().is_empty(), "{name}: llc-{major}: {}", outcome.stderr_text());
    std::fs::read(&output).unwrap()
}

fn options() -> Options {
    Options { reloc_model: RelocModel::Pic, ..Options::default() }
}

fn skip(name: &str) -> bool {
    name == ".eh_frame"
}

// spec/ir.md §2.3, spec/codegen.md §9.7.1: the symbol table hod writes
// for directive-only module assembly is llc's.
#[test]
fn directives_agree_with_llc() {
    let majors: Vec<u32> = LLVM_MAJORS.into_iter().filter(|&m| llc_path(m).is_some()).collect();
    if majors.is_empty() {
        assert!(!tools_required(), "no llc (HOD_REQUIRE_TOOLS=1)");
        eprintln!("note: no llc; skipping");
        return;
    }
    let scratch = Scratch::new("emit-asm");
    let mut failures = Vec::new();
    let runs =
        [Arch::AArch64, Arch::X86_64].into_iter().flat_map(|a| majors.iter().map(move |m| (a, *m)));
    for (arch, major) in runs {
        let extra = if arch == Arch::X86_64 { X64_CASES } else { &[] };
        // `hod_ir` reads `//` as a comment on AArch64 only; on x86-64 it
        // refuses the text as `module:asm` (`llc` reads it as a comment
        // there too: a missed acceptance, not a difference).
        let cases = CASES.iter().filter(|c| arch == Arch::AArch64 || c.0 != "comments");
        for &(name, ir) in cases.chain(extra) {
            let text = format!("{}{ir}", header(arch, major));
            let tag = format!("{name}-{arch:?}");
            let theirs = Elf::parse(llc(&scratch, major, &tag, &text));
            let parsed = hod_ll::parse_module(text.as_bytes(), &hod_ll::ParseOptions::default())
                .unwrap_or_else(|e| panic!("{name}: hod_ll: {e}"));
            let functions: Vec<_> = parsed.functions().iter().map(|f| f.sym).collect();
            let align = if arch == Arch::X86_64 { 16 } else { 4 };
            let chunk = borrow::chunk_from(&parsed.module, &functions, &theirs, &|_| align);
            let emitted = emit_module(&parsed.module, &options(), &[chunk])
                .unwrap_or_else(|e| panic!("{name}: emit_module: {e}"));
            let ours = Elf::parse(hod_obj::elf::to_vec(&emitted.object).unwrap());
            let (a, b) = (
                ours.describe_without(&|_| false, &skip),
                theirs.describe_without(&|_| false, &skip),
            );
            let (only_ours, only_theirs) = diff(&a, &b);
            if !only_ours.is_empty() || !only_theirs.is_empty() {
                let mut report = format!("{tag} (LLVM {major}):\n");
                for line in &only_ours {
                    let _ = writeln!(report, "  hod: {line}");
                }
                for line in &only_theirs {
                    let _ = writeln!(report, "  llc: {line}");
                }
                failures.push(report);
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

// spec/ir.md §2.3: everything else stays `module:asm`, in `classify` and
// in emission.
#[test]
fn other_assembly_is_refused_by_name() {
    let support = hod_ir::Support::shipped(hod_ir::Arch::Aarch64);
    for ir in REFUSED {
        let text = format!("{}{ir}", header(Arch::AArch64, 22));
        let parsed = hod_ll::parse_module(text.as_bytes(), &hod_ll::ParseOptions::default())
            .unwrap_or_else(|e| panic!("{ir}: hod_ll: {e}"));
        match emit_module(&parsed.module, &options(), &[]) {
            Err(CgError::Unsupported { construct }) => assert_eq!(construct, "module:asm", "{ir}"),
            other => panic!("{ir}: {other:?}"),
        }
        let rejection = hod_ir::Classifier::new(support).module(&parsed.module).expect_err(ir);
        assert_eq!(rejection.construct, "module:asm", "{ir}");
    }
    for &(name, ir) in CASES {
        let text = format!("{}{ir}", header(Arch::AArch64, 22));
        let parsed =
            hod_ll::parse_module(text.as_bytes(), &hod_ll::ParseOptions::default()).unwrap();
        let mut effects = Vec::new();
        assert!(hod_ir::module_asm_effects(&parsed.module, &mut effects).is_ok(), "{name}");
    }
}
