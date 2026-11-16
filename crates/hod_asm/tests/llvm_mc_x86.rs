//! `spec/asm.md` §22: the 32-bit template assembler (`hod_asm::x86`)
//! encodes every accepted body to the bytes `llvm-mc -triple=i686`
//! (LLVM 22 and 19) writes for it, its symbol fields to the same
//! relocations, and refuses by name what 32-bit mode does not have.

use std::fmt::Write as _;

use hod_asm::Refusal;
use hod_asm::x64::{Ref, RefKind};
use hod_asm::x86::assemble;
use hod_testutil::{Scratch, run, tool};

/// Bodies the assembler accepts in 32-bit mode.
const ACCEPTED: &[&str] = &[
    "ret",
    "retl",
    "leave",
    "leavel",
    "nop",
    "pause",
    "cpuid",
    "rdtsc",
    "rdtscp",
    "mfence",
    "lfence",
    "sfence",
    "int3",
    "int $0x80",
    "ud2",
    "hlt",
    "cld",
    "std",
    "pushf",
    "popf",
    "pushfl",
    "popfl",
    "cltd",
    "cwtl",
    "cbtw",
    "cwtd",
    "lahf",
    "sahf",
    "clc",
    "stc",
    "cmc",
    "fninit",
    "fwait",
    "fldz",
    "fld1",
    // Registers, immediates, memory.
    "movl %eax, %ecx",
    "movl %esp, %ebp",
    "movl %esi, %edi",
    "movw %ax, %bx",
    "movb %al, %dl",
    "movb %bl, %cl",
    "movl $1, %eax",
    "movl $-1, %edi",
    "movl $0x12345678, %ebx",
    "movl (%eax), %ecx",
    "movl 4(%esp), %eax",
    "movl -8(%ebp), %edx",
    "movl (%eax,%ecx,4), %edx",
    "movl 12(%esi,%edi,8), %ebp",
    "movl (,%ecx,2), %eax",
    "movl (%esp,%eax), %ebx",
    "movl (%ebp), %eax",
    "movl %eax, 16(%ecx)",
    "movb $7, (%edi)",
    "movw $300, 2(%esi)",
    "movl $5, (%esp)",
    "movl 0x1000, %eax",
    "movl 0x1000, %ecx",
    "movl %eax, 0x2000",
    "movb 0x10, %al",
    "movl %gs:0, %eax",
    "movl %gs:0, %ecx",
    "movl %fs:4, %eax",
    "movl %gs:(%eax), %edx",
    "movl %gs:8(%ecx), %eax",
    "leal 4(%eax), %ecx",
    "leal (%eax,%ebx,2), %esi",
    "leal -1(%esp), %ebp",
    "movzbl %al, %eax",
    "movzwl (%ecx), %edx",
    "movsbl %bl, %esi",
    "movswl %cx, %edi",
    // Arithmetic.
    "addl %eax, %ecx",
    "addl $1, %eax",
    "addl $1000, %ebx",
    "subl (%esp), %eax",
    "andl $0xff, %edx",
    "orl %ecx, 4(%eax)",
    "xorl %eax, %eax",
    "cmpl $3, %esi",
    "testl %eax, %eax",
    "testb $1, %cl",
    "adcl %edx, %ecx",
    "sbbl %eax, %eax",
    "incl %eax",
    "incl %edi",
    "decl %ecx",
    "incw %ax",
    "decw %si",
    "incb %al",
    "incl (%eax)",
    "negl %eax",
    "notl %edx",
    "imull %ecx, %eax",
    "imull $12, %ecx, %eax",
    "mull %ecx",
    "divl %ebx",
    "idivl (%esi)",
    "shll $3, %eax",
    "shrl %cl, %edx",
    "sarl %eax",
    "roll $1, %ebx",
    "shldl $4, %eax, %edx",
    // Bits.
    "bswap %eax",
    "bswapl %esi",
    "bsfl %eax, %ecx",
    "bsrl (%edx), %eax",
    "btl $3, %eax",
    "btsl %ecx, %edx",
    "xchgl %eax, %ecx",
    "xchg %ebx, %esi",
    "xchgl %eax, (%edx)",
    "lock xaddl %eax, (%ecx)",
    "lock; incl (%eax)",
    "lock cmpxchgl %ecx, (%edx)",
    "cmpxchg8b (%esi)",
    "lock cmpxchg8b (%edi)",
    "sete %al",
    "setne %cl",
    "setb (%eax)",
    "seta %bl",
    "cmovel %ecx, %eax",
    "cmovnel (%esi), %edx",
    "popcntl %eax, %ecx",
    // Stack.
    "push %eax",
    "pushl %ebx",
    "push %ebp",
    "pop %eax",
    "popl %esi",
    "pushl (%eax)",
    "popl 4(%esp)",
    "pushl $1",
    "pushl $0x1000",
    "push $-1",
    // Strings.
    "rep movsb",
    "rep movsl",
    "rep stosl",
    "repne scasb",
    "movsl",
    "lodsb",
    // Ports, SSE2.
    "inb %dx, %al",
    "outl %eax, %dx",
    "movd %eax, %xmm0",
    "movd %xmm7, %ecx",
    "addps %xmm1, %xmm2",
    // Branches to numeric labels.
    "1: decl %ecx\njnz 1b",
    "jmp 1f\nnop\n1:",
    "testl %eax, %eax\nje 1f\nincl %eax\n1:",
    "call 1f\n1: popl %eax",
    ".byte 0x90\n.short 1\n.long 2",
];

/// Bodies with one symbol field; `S` is renamed per body.
const SYMBOLIC: &[&str] = &[
    "movl S, %eax",
    "movl S, %ecx",
    "movl %eax, S",
    "movl S+8, %edx",
    "movl S(%eax), %ecx",
    "leal S(,%ecx,4), %eax",
    "movl $1, S",
    "movl %gs:S, %eax",
    "incl S",
    "call S",
    "call S@PLT",
    "jmp S",
    "jne S",
    "call S+4",
    "nop\n.reloc ., BFD_RELOC_NONE, S",
];

/// Bodies refused in 32-bit mode, by name.
const REFUSED: &[(&str, &str)] = &[
    ("movq %rax, %rcx", "asm:operand/register"),
    ("movl %r8d, %eax", "asm:operand/register"),
    ("movb %spl, %al", "asm:operand/register"),
    ("movb %ah, %al", "asm:operand/register"),
    ("movl (%rax), %eax", "asm:operand/register"),
    ("movl (%eax,%r8d), %eax", "asm:operand/register"),
    ("movl 4(%rip), %eax", "asm:operand/register"),
    ("movd %eax, %xmm8", "asm:operand/register"),
    ("pushq %rax", "asm:operand/register"),
    ("pushq $1", "asm:mnemonic/pushq"),
    ("popq (%eax)", "asm:mnemonic/popq"),
    ("retq", "asm:mnemonic/retq"),
    ("pushfq", "asm:mnemonic/pushfq"),
    ("syscall", "asm:mnemonic/syscall"),
    ("cltq", "asm:mnemonic/cltq"),
    ("cmpxchg16b (%eax)", "asm:mnemonic/cmpxchg16b"),
    ("rdgsbase %eax", "x86-mode:x86-64-only"),
    ("movl S@ntpoff, %eax", "asm:operand/specifier"),
    ("movl %gs:S@tpoff, %eax", "asm:operand/specifier"),
    ("movl S@GOTOFF(%ebx), %eax", "asm:operand/specifier"),
    ("jmp *%eax", "asm:operand/form"),
];

fn hod_refs(body: &str) -> Result<(Vec<u8>, Vec<Ref>), Refusal> {
    let (mut code, mut refs) = (Vec::new(), Vec::new());
    assemble(body, &mut code, &mut refs, &mut String::new())?;
    Ok((code, refs))
}

/// The `.text` bytes of `source` as `llvm-mc-<major> -triple=i686`
/// assembles it, and the object.
fn llvm(major: u32, source: &str, scratch: &Scratch, tag: &str) -> Option<Vec<u8>> {
    let mc = tool(&format!("llvm-mc-{major}"))?;
    let objcopy = tool(&format!("llvm-objcopy-{major}"))?;
    let src = scratch.write(&format!("{tag}-{major}.s"), source);
    let obj = scratch.join(format!("{tag}-{major}.o"));
    let bin = scratch.join(format!("{tag}-{major}.bin"));
    let out = run(
        &mc,
        [
            "--triple=i686-unknown-linux-gnu".as_ref(),
            "-filetype=obj".as_ref(),
            "-o".as_ref(),
            obj.as_os_str(),
            src.as_os_str(),
        ],
    );
    assert!(out.success(), "llvm-mc-{major} refused the bodies:\n{}", out.stderr_text());
    let args = ["-O".as_ref(), "binary".as_ref(), "--only-section=.text".as_ref(), obj.as_os_str()];
    let out = run(&objcopy, args.into_iter().chain([bin.as_os_str()]));
    assert!(out.success(), "llvm-objcopy failed: {}", out.stderr_text());
    Some(std::fs::read(&bin).expect("objcopy output"))
}

#[test]
fn accepted_bodies_match_llvm_mc_i686() {
    let (mut ours, mut spans, mut source) = (Vec::new(), Vec::new(), String::new());
    for body in ACCEPTED {
        let (bytes, refs) = hod_refs(body).unwrap_or_else(|e| panic!("{body:?} refused: {e}"));
        assert!(refs.is_empty(), "{body:?}: {refs:?}");
        spans.push((body, ours.len(), bytes.len()));
        ours.extend(bytes);
        source.push_str(body);
        source.push('\n');
    }
    let scratch = Scratch::new("hod-asm-mc-x86");
    for major in [22, 19] {
        let Some(theirs) = llvm(major, &source, &scratch, "all") else { continue };
        let mut report = String::new();
        for &(body, at, len) in &spans {
            let (a, b) = (&ours[at..at + len], theirs.get(at..at + len));
            if Some(a) != b {
                writeln!(report, "  {body:?}: hod {a:02x?}, llvm {b:02x?}").unwrap();
            }
        }
        assert!(
            report.is_empty() && ours.len() == theirs.len(),
            "LLVM {major} disagrees ({} vs {} bytes):\n{report}",
            ours.len(),
            theirs.len()
        );
        println!(
            "hod_asm x86: {} bodies, {} bytes equal llvm-mc-{major}'s",
            spans.len(),
            ours.len()
        );
    }
}

const fn elf_name(kind: RefKind) -> &'static str {
    match kind {
        RefKind::Pc32 => "R_386_PC32",
        RefKind::Plt32 => "R_386_PLT32",
        RefKind::Abs32S => "R_386_32",
        RefKind::None => "R_386_NONE",
        _ => "unexpected",
    }
}

#[test]
fn symbol_fields_match_llvm_mc_i686() {
    let (mut ours, mut lines, mut source) = (Vec::new(), Vec::new(), String::new());
    for (i, body) in SYMBOLIC.iter().enumerate() {
        let body = body.replace('S', &format!("sym{i}"));
        let (mut bytes, refs) = hod_refs(&body).unwrap_or_else(|e| panic!("{body:?} refused: {e}"));
        assert_eq!(refs.len(), 1, "{body:?}");
        for r in &refs {
            // REL: the addend is in the field, as llvm-mc writes it.
            let at = r.offset as usize;
            if r.kind != RefKind::None {
                let a = i32::try_from(r.addend).unwrap().to_le_bytes();
                bytes[at..at + 4].copy_from_slice(&a);
            }
            lines.push(format!("{:08x} {} {}", ours.len() + at, elf_name(r.kind), r.name));
        }
        ours.extend(bytes);
        source.push_str(&body);
        source.push('\n');
    }
    let scratch = Scratch::new("hod-asm-mc-x86-sym");
    for major in [22, 19] {
        let Some(theirs) = llvm(major, &source, &scratch, "sym") else { continue };
        assert!(ours == theirs, "LLVM {major}: the bytes differ:\n{ours:02x?}\n{theirs:02x?}");
        let Some(objdump) = tool(&format!("llvm-objdump-{major}")) else { continue };
        let obj = scratch.join(format!("sym-{major}.o"));
        let out = run(&objdump, ["-r".as_ref(), obj.as_os_str()]);
        assert!(out.success(), "llvm-objdump: {}", out.stderr_text());
        let mut theirs: Vec<String> = out
            .stdout_text()
            .lines()
            .filter(|l| l.contains("R_386"))
            .map(|l| l.split_whitespace().collect::<Vec<_>>().join(" "))
            .collect();
        theirs.sort();
        let mut lines = lines.clone();
        lines.sort();
        assert_eq!(lines, theirs, "LLVM {major}: the relocations differ");
        println!("hod_asm x86: {} symbol fields equal llvm-mc-{major}'s", lines.len());
    }
}

#[test]
fn refused_bodies_are_named() {
    for &(body, name) in REFUSED {
        assert_eq!(hod_refs(body).err().map(|r| r.0), Some(name), "{body:?}");
    }
}

/// What hod refuses for its register names, llvm-mc refuses too.
#[test]
fn refused_registers_are_errors_to_llvm_mc_i686() {
    let scratch = Scratch::new("hod-asm-mc-x86-bad");
    for major in [22, 19] {
        let Some(mc) = tool(&format!("llvm-mc-{major}")) else { continue };
        for &(body, name) in REFUSED {
            // `%ah`..`%dh` exist; hod has no high-byte view.
            if body.contains("%ah")
                || (name != "asm:operand/register" && !name.starts_with("asm:mnemonic/"))
            {
                continue;
            }
            let src = scratch.write("bad.s", body);
            let obj = scratch.join("bad.o");
            let args = ["--triple=i686-unknown-linux-gnu".as_ref(), "-filetype=obj".as_ref()];
            let out =
                run(&mc, args.into_iter().chain(["-o".as_ref(), obj.as_os_str(), src.as_os_str()]));
            assert!(!out.success() || body == "syscall", "llvm-mc-{major} takes {body:?}");
        }
    }
}
