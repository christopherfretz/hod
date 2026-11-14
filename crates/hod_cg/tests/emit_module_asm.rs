//! Module-level assembly of code and data (`spec/asm.md` §9,
//! `spec/codegen.md` §9.7.3, WP-112) against `llc`: each module is
//! emitted by `hod_cg::emit_module` and compiled by `llc` of each LLVM
//! major present, and the two objects must have the same sections (with
//! their bytes), symbols and relocations, up to `elfview`'s
//! normalizations, and the same mapping symbols in code sections. One
//! difference is recorded rather than followed: LLVM 19 gives a code
//! section other than `.text` that the text creates alignment 1, LLVM 22
//! gives it 4 (`hod` follows 22). What `hod_asm` does not read is
//! refused by an `asm:*` name. Since WP-204 (`spec/asm.md` §14) the
//! same holds on x86-64, with `.symver`.

mod borrow;
mod elfview;

use std::fmt::Write as _;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use elfview::{Elf, diff, is_mapping};
use hod_cg::{CgError, Options, RelocModel, emit_module};
use hod_ir::{
    AARCH64_LINUX_DATA_LAYOUT, AARCH64_LINUX_DATA_LAYOUT_LLVM19, X86_64_LINUX_DATA_LAYOUT,
};
use hod_testutil::{LLVM_MAJORS, Scratch, run_with, tools_required};

/// `module asm` lines of `text`, one per line.
fn asm(text: &str) -> String {
    let mut out = String::new();
    for line in text.lines() {
        let escaped = line.replace('\\', "\\5C").replace('"', "\\22");
        let _ = writeln!(out, "module asm \"{escaped}\"");
    }
    out
}

/// `(name, assembly, IR after it)`: each directive, each section shape,
/// each kind of reference and each way an assembly symbol meets a
/// module symbol.
const CASES: &[(&str, &str, &str)] = &[
    // The corpus shapes (`ext.gnu.extension/extension-asm.cpp`,
    // `expr.call/run-aarch64-overaligned-aggregate.cpp`).
    (
        "rodata-value",
        "\n.pushsection .rodata\n.globl kiln_ext_asm_value\nkiln_ext_asm_value: .long 42\n.popsection\n",
        "@kiln_ext_asm_value = external global i32\n\
         define i32 @get() {\n  %v = load i32, ptr @kiln_ext_asm_value\n  ret i32 %v\n}\n",
    ),
    (
        "code-calls",
        "  .text\n  .p2align 2\n  .globl asm_lo\nasm_lo:\n  mov x0, x1\n  ret\n  .globl asm_call_dump\n\
         asm_call_dump:\n  stp x29, x30, [sp, #-16]!\n  mov x29, sp\n  mov w0, #3\n  bl dump\n\
         ldp x29, x30, [sp], #16\n  ret\n",
        "declare i64 @asm_lo(i64, i64)\ndeclare void @asm_call_dump()\n\
         define void @dump() {\n  ret void\n}\n\
         define i64 @use() {\n  call void @asm_call_dump()\n  %r = call i64 @asm_lo(i64 1, i64 2)\n  ret i64 %r\n}\n",
    ),
    // Data directives, each spelling.
    (
        "integers",
        ".data\n.byte 1, 2, 255, -1\n.hword 0x1234\n.short -2\n.2byte 7\n.word 0x12345678\n.long -3\n\
         .int 9\n.4byte 10\n.quad 0x123456789abcdef0\n.xword -4\n.8byte 11\n",
        "",
    ),
    (
        "strings",
        ".section .rodata\n.ascii \"ab\\n\\t\\\"\\\\\\101\"\n.ascii \"\"\n.asciz \"x\"\n.string \"yz\", \"w\"\n",
        "",
    ),
    ("space", ".data\n.byte 1\n.zero 3\n.space 5\n.space 3, 0xaa\n.skip 2\n.skip 2, 7\n", ""),
    (
        "data-align",
        ".data\n.byte 1\n.p2align 3\n.byte 2\n.balign 16\n.byte 3\n.align 2\n.byte 4\n.balign 8, 0xcc\n\
         .byte 5\n.p2align 2, 0x11\n",
        "",
    ),
    (
        "code-align",
        ".text\nnop\n.p2align 4\nret\n.balign 8\nnop\n.p2align 3, 0x11\nnop\n.byte 1\n.p2align 2\nnop\n\
         .byte 2\n.p2align 4\nret\n",
        "",
    ),
    ("bss", ".bss\n.zero 8\nb:\n.zero 4\n.p2align 4\n.space 16\n", ""),
    (
        "sections",
        ".section .text.foo,\"ax\",@progbits\nf:\nret\n.section .data.rel.ro,\"aw\",@progbits\n.quad 1\n\
         .section .rodata.foo,\"a\"\n.long 2\n.section .bss.foo,\"aw\",@nobits\n.zero 4\n\
         .section .data.bar\n.long 3\n.section .text\nnop\n",
        "",
    ),
    (
        "stack",
        ".text\nnop\n.pushsection .data\n.long 1\n.pushsection .rodata\n.long 5\n.popsection\n.long 6\n\
         .popsection\nnop\n.section .rodata\n.long 2\n.previous\nnop\n.data\n.long 3\n.previous\n.long 4\n",
        "",
    ),
    // Assembly symbols and their directives.
    (
        "symbols",
        ".text\n.globl f\n.type f, @function\nf:\nret\n.size f, .-f\n.weak w\n.hidden w\nw:\nnop\nret\n\
         .size w, .-w\nl:\nnop\n.data\n.globl o\n.type o, %object\n.protected o\no:\n.quad 1\n.size o, 8\n\
         lo:\n.long 3\n.type lo, @object\n",
        "",
    ),
    (
        "set",
        ".set k, 42\n.globl k\n.equ k2, 7\nk3 = -1\n.data\n.long k\n.quad k2 + 1\n.byte k3\n",
        "",
    ),
    (
        "temporaries",
        ".text\nb .Lx\n.Lx:\nret\n.data\n.Ly:\n.long 1\n.quad .Ly\n.quad .Lx + 4\n",
        "",
    ),
    (
        "references",
        ".text\n.globl g\nbl ext_fn\nb g\nb.ne l\ncbz x0, g\ng:\nl:\nret\n\
         .data\ndl:\n.quad g\n.quad ext_fn + 8\n.word dl\n.quad l - 4\n.hword dl\n.quad dl\n",
        "",
    ),
    ("numeric", ".text\n1:\nnop\nb 1b\nb 1f\n1:\nret\ncbz x0, 1b\n", ""),
    ("code-data", ".text\nnop\n.word 0x12345678\nnop\n.long 1, 2\nret\n", ""),
    // Assembly meets module symbols.
    ("before-function", ".text\nnop\nret\n", "define void @D() {\n  ret void\n}\n"),
    ("data-before-variable", ".data\n.byte 1\n", "@V = global i32 7\n@W = global i64 8\n"),
    (
        "declaration-defined",
        ".text\n.globl f\nf:\nret\n.weak g\ng:\nret\n.data\n.globl v\nv:\n.long 1\n",
        "declare void @f()\ndeclare void @g()\n@v = external global i32\n\
         @keep = global [3 x ptr] [ptr @f, ptr @g, ptr @v]\n",
    ),
    (
        "declaration-defined-unused",
        ".text\n.globl f\n.type f, %function\nf:\nret\n.size f, .-f\n",
        "declare void @f()\n",
    ),
    (
        "visibility-from-ir",
        ".text\n.globl f\nf:\nret\n.data\n.globl v\n.type v, @object\nv:\n.long 1\n",
        "declare hidden void @f()\n@v = external protected global i32\n\
         @keep = global [2 x ptr] [ptr @f, ptr @v]\n",
    ),
    (
        "calls-module-function",
        ".text\n.globl t\nt:\nb D\n.data\n.quad D\n.quad V\n",
        "define void @D() {\n  ret void\n}\n@V = global i32 3\n",
    ),
    ("directives-on-declaration", ".weak U\n.text\nbl U\n", "declare void @U()\n"),
];

/// `(module assembly, IR after it, the name hod refuses it by)`.
const REFUSED: &[(&str, &str, &str)] = &[
    (".quad .", "", "asm:operand/expression"),
    (".text\n.byte 1\nnop", "", "asm:unaligned"),
    (".data\nnop", "", "asm:section"),
    (".section .rodata.str1.1,\"aMS\",@progbits,1", "", "asm:section"),
    (".section mine,\"aw\"", "", "asm:section"),
    (".section .data,\"ax\"", "", "asm:section"),
    (".cfi_startproc", "", "asm:directive/cfi_startproc"),
    (".symver a, a@V1", "", "asm:directive/symver"),
    (".internal U", "", "asm:directive/internal"),
    (".popsection", "", "asm:section"),
    (".text\nfoo x0", "", "asm:mnemonic/foo"),
    // A symbol the module defines: llc reports it defined twice.
    (".text\nD:\nret", "define void @D() {\n  ret void\n}\n", "asm:symbol"),
    (".data\nP:\n.long 1", "@P = private global i32 1\n@keep = global ptr @P\n", "asm:symbol"),
    // Directives llc resolves by precedence.
    (".globl U\n.weak U", "", "asm:symbol"),
    (".type U, @gnu_indirect_function", "", "asm:symbol"),
    (".size U, 4", "", "asm:symbol"),
    (".text\nf:\n.set f, 3", "", "asm:label"),
    // `adrp` is not among `spec/asm.md` §5.2's mnemonics.
    (".text\nadrp x0, dl\n.data\ndl:\n.long 1", "", "asm:mnemonic/adrp"),
];

fn header(major: u32) -> String {
    let layout =
        if major >= 20 { AARCH64_LINUX_DATA_LAYOUT } else { AARCH64_LINUX_DATA_LAYOUT_LLVM19 };
    format!("target datalayout = \"{layout}\"\ntarget triple = \"aarch64-unknown-linux-gnu\"\n")
}

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

/// The mapping symbols of code sections: `hod` writes none in data
/// sections (`spec/codegen.md` §9.4), `llc` does.
fn mapping(elf: &Elf) -> Vec<String> {
    let mut out: Vec<String> = elf
        .symbols
        .iter()
        .filter(|s| is_mapping(&s.name) && s.shndx != 0 && (s.shndx as usize) < elf.sections.len())
        .filter(|s| elf.sections[s.shndx as usize].flags & 4 != 0)
        .map(|s| format!("{} {} {}", &s.name[..2], elf.sections[s.shndx as usize].name, s.value))
        .collect();
    out.sort();
    out
}

/// LLVM 19's alignment of a code section the text creates (other than
/// `.text`) is 1; LLVM 22's, and `hod`'s, is 4.
fn as_llvm22(major: u32, lines: Vec<String>) -> Vec<String> {
    if major >= 20 {
        return lines;
    }
    let mut lines: Vec<String> = lines
        .into_iter()
        .map(|line| {
            if line.starts_with("section .text.") && line.contains(" f6 ") {
                line.replacen(" align 1 ", " align 4 ", 1)
            } else {
                line
            }
        })
        .collect();
    lines.sort();
    lines
}

fn module_text(major: u32, assembly: &str, ir: &str) -> String {
    format!("{}{}{ir}", header(major), asm(assembly))
}

// spec/asm.md §9, spec/codegen.md §9.7.3: what hod writes for module
// assembly is llc's: sections, bytes, symbols, relocations, mapping
// symbols.
#[test]
fn module_assembly_agrees_with_llc() {
    let majors: Vec<u32> = LLVM_MAJORS.into_iter().filter(|&m| llc_path(m).is_some()).collect();
    if majors.is_empty() {
        assert!(!tools_required(), "no llc (HOD_REQUIRE_TOOLS=1)");
        eprintln!("note: no llc; skipping");
        return;
    }
    let scratch = Scratch::new("emit-module-asm");
    let mut failures = Vec::new();
    for major in &majors {
        for &(name, assembly, ir) in CASES {
            let text = module_text(*major, assembly, ir);
            let theirs = Elf::parse(llc(&scratch, *major, name, &text));
            let parsed = hod_ll::parse_module(text.as_bytes(), &hod_ll::ParseOptions::default())
                .unwrap_or_else(|e| panic!("{name}: hod_ll: {e}"));
            let functions: Vec<_> = parsed.functions().iter().map(|f| f.sym).collect();
            let chunk = borrow::chunk_from(&parsed.module, &functions, &theirs, &|_| 4);
            let emitted = match emit_module(&parsed.module, &options(), &[chunk]) {
                Ok(emitted) => emitted,
                Err(e) => {
                    failures.push(format!("{name} (LLVM {major}): emit_module: {e}"));
                    continue;
                }
            };
            let ours = Elf::parse(hod_obj::elf::to_vec(&emitted.object).unwrap());
            let mut a = ours.describe_without(&|_| false, &skip);
            let mut b = as_llvm22(*major, theirs.describe_without(&|_| false, &skip));
            a.extend(mapping(&ours));
            b.extend(mapping(&theirs));
            a.sort();
            b.sort();
            let (only_ours, only_theirs) = diff(&a, &b);
            if !only_ours.is_empty() || !only_theirs.is_empty() {
                let mut report = format!("{name} (LLVM {major}):\n");
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

// spec/asm.md §8, §9: what hod_asm does not read is refused by its name
// in emission; classify passes module assembly on AArch64.
#[test]
fn unread_assembly_is_refused_by_name() {
    let support = hod_ir::Support::shipped(hod_ir::Arch::Aarch64);
    let mut failures = Vec::new();
    for &(assembly, ir, expected) in REFUSED {
        let text = module_text(22, assembly, ir);
        let parsed = hod_ll::parse_module(text.as_bytes(), &hod_ll::ParseOptions::default())
            .unwrap_or_else(|e| panic!("{assembly}: hod_ll: {e}"));
        assert!(hod_ir::Classifier::new(support).module(&parsed.module).is_ok(), "{assembly}");
        match emit_module(&parsed.module, &options(), &[]) {
            Err(CgError::Unsupported { construct }) if construct == expected => {}
            other => failures.push(format!("{assembly:?}: expected {expected}, got {other:?}")),
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// x86-64 cases (`spec/asm.md` §14), as [`CASES`].
const X64_CASES: &[(&str, &str, &str)] = &[
    // The corpus shape (`ext.gnu.extension/extension-asm.cpp`).
    (
        "rodata-value",
        "\n.pushsection .rodata\n.globl kiln_ext_asm_value\nkiln_ext_asm_value: .long 42\n.popsection\n",
        "@kiln_ext_asm_value = external global i32\n\
         define i32 @get() {\n  %v = load i32, ptr @kiln_ext_asm_value\n  ret i32 %v\n}\n",
    ),
    (
        "code-calls",
        ".text\n.p2align 4\n.globl asm_add\n.type asm_add, @function\nasm_add:\n  leaq (%rdi,%rsi), %rax\n  ret\n\
         .size asm_add, .-asm_add\n.globl asm_call\nasm_call:\n  pushq %rbp\n  movq %rsp, %rbp\n\
         movl $3, %edi\n  call dump\n  call dump@PLT\n  call dump+0\n  popq %rbp\n  ret\n",
        "declare i64 @asm_add(i64, i64)\ndeclare void @asm_call()\n\
         define void @dump(i32 %x) {\n  ret void\n}\n\
         define i64 @use() {\n  call void @asm_call()\n  %r = call i64 @asm_add(i64 1, i64 2)\n  ret i64 %r\n}\n",
    ),
    (
        "integers",
        ".data\n.byte 1, 2, 255, -1\n.short -2\n.word 0x1234\n.value 9\n.2byte 7\n.long -3\n.int 9\n\
         .4byte 10\n.quad 0x123456789abcdef0\n.8byte 11\n",
        "",
    ),
    ("space", ".data\n.byte 1\n.zero 3\n.space 5\n.space 3, 0xaa\n.skip 2\n.skip 2, 7\n", ""),
    (
        "data-align",
        ".data\n.byte 1\n.p2align 3\n.byte 2\n.balign 16\n.byte 3\n.align 4\n.byte 4\n.balign 8, 0xcc\n\
         .byte 5\n.p2align 2, 0x11\n",
        "",
    ),
    (
        "code-align",
        ".text\nnop\n.p2align 4\nret\n.balign 8\nnop\n.p2align 5\nnop\n.p2align 3, 0x11\nnop\n.align 64\nret\n",
        "",
    ),
    (
        "sections",
        ".section .text.foo,\"ax\",@progbits\nf:\nret\n.section .data.rel.ro,\"aw\",@progbits\n.quad 1\n\
         .section .rodata.foo,\"a\"\n.long 2\n.section .bss.foo,\"aw\",@nobits\n.zero 4\n\
         .section .text\nnop\n",
        "",
    ),
    (
        "stack",
        ".text\nnop\n.pushsection .data\n.long 1\n.pushsection .rodata\n.long 5\n.popsection\n.long 6\n\
         .popsection\nnop\n.section .rodata\n.long 2\n.previous\nnop\n.data\n.long 3\n.previous\n.long 4\n",
        "",
    ),
    (
        "symbols",
        ".text\n.globl f\n.type f, @function\nf:\nret\n.size f, .-f\n.weak w\n.hidden w\nw:\nnop\nret\n\
         .size w, .-w\nl:\nnop\n.data\n.globl o\n.type o, @object\n.protected o\no:\n.quad 1\n.size o, 8\n\
         lo:\n.long 3\n.type lo, @object\n",
        "",
    ),
    (
        "set",
        ".set k, 42\n.globl k\n.equ k2, 7\nk3 = -1\n.data\n.long k\n.quad k2 + 1\n.byte k3\n",
        "",
    ),
    (
        "references",
        ".text\n.globl g\ncall ext_fn\njmp g\njne l\nleaq l(%rip), %rax\nmovq ext_v@GOTPCREL(%rip), %rax\n\
         movl dl, %eax\nleaq dl(%rip), %rax\ncall l\njmp .Lx\n.Lx:\ng:\nl:\nret\n\
         .data\ndl:\n.quad g\n.quad ext_fn + 8\n.long dl\n.quad l - 4\n.short dl\n.quad dl\n",
        "",
    ),
    (
        "relaxation",
        ".text\n1:\njmp 2f\njne 1b\n.zero 130\n2:\njmp 1b\nje 1b\njmp 3f\n3:\nret\n",
        "",
    ),
    ("numeric", ".text\n1:\nnop\njmp 1b\njmp 1f\n1:\nret\njae 1b\n", ""),
    ("before-function", ".text\nnop\nret\n", "define void @D() {\n  ret void\n}\n"),
    ("data-before-variable", ".data\n.byte 1\n", "@V = global i32 7\n@W = global i64 8\n"),
    (
        "declaration-defined",
        ".text\n.globl f\nf:\nret\n.weak g\ng:\nret\n.data\n.globl v\nv:\n.long 1\n",
        "declare void @f()\ndeclare void @g()\n@v = external global i32\n\
         @keep = global [3 x ptr] [ptr @f, ptr @g, ptr @v]\n",
    ),
    (
        "calls-module-function",
        ".text\n.globl t\nt:\njmp D\n.data\n.quad D\n.quad V\n",
        "define void @D() {\n  ret void\n}\n@V = global i32 3\n",
    ),
    ("directives-on-declaration", ".weak U\n.text\ncall U\n", "declare void @U()\n"),
    // `.symver`: an alias of a defined name, a rename with `@@@` or of
    // an undefined name, nothing for an unreferenced undefined one.
    // (`@@@` renames a text label here: the borrowed code of a module
    // function is found by its name, which a rename changes.)
    (
        "symver",
        ".symver foo, foo@@V1\n.symver foo, foo@V0\n.symver bar, bar@V1\n.symver wk, wk@V3\n\
         .symver hid, hid@V4\n.symver unused, unused@V5\n.text\n.globl tf\n.type tf,@function\n\
         tf: movl $7, %eax\nret\n.size tf, .-tf\n.symver tf, tf@@V2\n.symver loc, loc@V6\n\
         .symver ext2, ext2@@@V7\n.symver ext3, ext3@V8\ncall ext3\ncall bar\njmp ext2\n.weak tw\ntw:\nret\n\
         .symver tw, tw@@@V9\n",
        "define void @foo() { ret void }\ndefine weak void @wk() { ret void }\n\
         define hidden void @hid() { ret void }\ndefine internal void @loc() { ret void }\n\
         declare void @bar()\ndeclare void @ext2()\n\
         define void @user() {\n  call void @loc()\n  ret void\n}\n",
    ),
];

/// `(x86-64 module assembly, IR after it, the name hod refuses it by)`.
const X64_REFUSED: &[(&str, &str, &str)] = &[
    (".cfi_startproc", "", "asm:directive/cfi_startproc"),
    (".hword 1", "", "asm:directive/hword"),
    (".symver u, u@@V1\n.text\ncall u", "", "asm:symbol"),
    (".text\nfoo %rax", "", "asm:mnemonic/foo"),
    (".text\nD:\nret", "define void @D() {\n  ret void\n}\n", "asm:symbol"),
    (".text\njmp *%rax", "", "asm:operand/form"),
];

fn x64_text(assembly: &str, ir: &str) -> String {
    format!(
        "target datalayout = \"{X86_64_LINUX_DATA_LAYOUT}\"\ntarget triple = \"x86_64-unknown-linux-gnu\"\n{}{ir}",
        asm(assembly)
    )
}

// spec/asm.md §14: on x86-64 what hod writes for module assembly is
// llc's: sections, bytes, symbols, relocations; classify passes it.
#[test]
fn x86_64_module_assembly_agrees_with_llc() {
    let majors: Vec<u32> = LLVM_MAJORS.into_iter().filter(|&m| llc_path(m).is_some()).collect();
    if majors.is_empty() {
        assert!(!tools_required(), "no llc (HOD_REQUIRE_TOOLS=1)");
        eprintln!("note: no llc; skipping");
        return;
    }
    let support = hod_ir::Support::shipped(hod_ir::Arch::X86_64);
    let scratch = Scratch::new("emit-module-asm-x64");
    let mut failures = Vec::new();
    for major in &majors {
        for &(name, assembly, ir) in X64_CASES {
            let text = x64_text(assembly, ir);
            let theirs = Elf::parse(llc(&scratch, *major, &format!("x64-{name}"), &text));
            let parsed = hod_ll::parse_module(text.as_bytes(), &hod_ll::ParseOptions::default())
                .unwrap_or_else(|e| panic!("{name}: hod_ll: {e}"));
            if let Err(e) = hod_ir::Classifier::new(support).module(&parsed.module) {
                failures.push(format!("{name}: classify: {}", e.construct));
                continue;
            }
            let functions: Vec<_> = parsed.functions().iter().map(|f| f.sym).collect();
            let chunk = borrow::chunk_from(&parsed.module, &functions, &theirs, &|_| 16);
            let emitted = match emit_module(&parsed.module, &options(), &[chunk]) {
                Ok(emitted) => emitted,
                Err(e) => {
                    failures.push(format!("{name} (LLVM {major}): emit_module: {e}"));
                    continue;
                }
            };
            let ours = Elf::parse(hod_obj::elf::to_vec(&emitted.object).unwrap());
            let mut a = ours.describe_without(&|_| false, &skip);
            let mut b = theirs.describe_without(&|_| false, &skip);
            a.sort();
            b.sort();
            let (only_ours, only_theirs) = diff(&a, &b);
            if !only_ours.is_empty() || !only_theirs.is_empty() {
                let mut report = format!("{name} (LLVM {major}):\n");
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

// spec/asm.md §14: what the x86-64 assembler does not read is refused by
// its name in emission.
#[test]
fn x86_64_unread_assembly_is_refused_by_name() {
    let mut failures = Vec::new();
    for &(assembly, ir, expected) in X64_REFUSED {
        let text = x64_text(assembly, ir);
        let parsed = hod_ll::parse_module(text.as_bytes(), &hod_ll::ParseOptions::default())
            .unwrap_or_else(|e| panic!("{assembly}: hod_ll: {e}"));
        match emit_module(&parsed.module, &options(), &[]) {
            Err(CgError::Unsupported { construct }) if construct == expected => {}
            other => failures.push(format!("{assembly:?}: expected {expected}, got {other:?}")),
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
