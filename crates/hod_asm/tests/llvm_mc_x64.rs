//! `spec/asm.md` §10: every x86-64 instruction form the assembler
//! accepts encodes to the bytes LLVM 22's and LLVM 19's `llvm-mc`
//! produce for the same text, a pinned core set encodes to bytes fixed
//! here (so the check holds where no LLVM is installed), and the forms
//! it refuses are refused by name.

use std::fmt::Write as _;

use hod_asm::Refusal;
use hod_asm::x64::assemble;
use hod_testutil::{Scratch, run, tool};

/// Bodies the assembler accepts. Each is assembled alone by hod; LLVM
/// assembles them all in one file. A body's numeric labels are its own.
const ACCEPTED: &[&str] = &[
    // Without operands.
    "nop",
    "pause",
    "rep; nop",
    "cpuid",
    "rdtsc",
    "rdtscp",
    "xgetbv",
    "mfence",
    "lfence",
    "sfence",
    "int3",
    "int $3",
    "int $0x80",
    "ud2",
    "hlt",
    "cld",
    "std",
    "syscall",
    "pushfq",
    "popfq",
    "pushf",
    "popf",
    "cltq",
    "cqto",
    "cltd",
    "cwtl",
    "cbtw",
    "cwtd",
    // Moves.
    "movq %rbx, %rsi",
    "movq %rsi, %rbx",
    "mov %rax, %r12",
    "movl %eax, %r9d",
    "movw %ax, %r10w",
    "movb %al, %sil",
    "movb %r8b, %dil",
    "movq $1, %rax",
    "movq $-1, %rax",
    "movl $0xffffffff, %eax",
    "movq $0xffffffff, %rax",
    "movabsq $0x123456789, %rcx",
    "movq (%rdi), %rax",
    "movq %rax, (%rdi)",
    "movl 8(%rsp), %eax",
    "movl -4(%rbp), %eax",
    "movq 16(%rdi,%rsi,8), %rax",
    "movq (%r12,%r13), %r14",
    "movb $1, (%r13)",
    "movw $2, 2(%rax)",
    "movl $3, 4(%rax,%rcx,4)",
    "movq $-8, (%rdx)",
    "movq %fs:0, %rax",
    "movq %fs:(%rdi), %rax",
    "mov %fs:0x28, %rcx",
    "movq %gs:8, %rdx",
    "movq 0x10, %rax",
    "movzbl %al, %eax",
    "movzbq (%rdi), %rax",
    "movzwl %cx, %edx",
    "movsbl %al, %eax",
    "movswq %r9w, %r10",
    "movslq %eax, %rax",
    "movzx %al, %ecx",
    "lea 1(%r9), %r10",
    "leaq 8(%rax,%rbx,2), %rcx",
    "leal (%rdi,%rsi), %eax",
    "lea (%rip), %rax",
    "xchgq %rbx, %rsi",
    "xchg %rax, %rbx",
    "xchgl %eax, (%rdi)",
    "xchgb %al, (%rsi)",
    // Arithmetic and logic.
    "addl $1, %eax",
    "addq $1000, %rbx",
    "addq $-128, %rcx",
    "addb $1, %al",
    "addw $1, %ax",
    "add %rax, %rbx",
    "addq (%rdi), %rax",
    "addq %rax, (%rdi)",
    "addl $1, (%rdi)",
    "orl $1, (%rsi)",
    "adcq %rcx, %rdx",
    "sbbl %eax, %eax",
    "andq $-16, %rsp",
    "subq $8, %rsp",
    "xorl %eax, %eax",
    "xorq %r11, %r11",
    "cmpl %esi, %edi",
    "cmp $2, %r8d",
    "cmpq $0, (%rax)",
    "testl %eax, %eax",
    "testq $1, %rdi",
    "testb $0x80, %al",
    "test (%rdi), %eax",
    "test %eax, (%rdi)",
    "incl %eax",
    "decq (%rdi)",
    "notq %rax",
    "negl %ecx",
    "mulq %rbx",
    "mull (%rdi)",
    "divq %rcx",
    "idivl %esi",
    "imulq %rbx",
    "imul %rcx, %rax",
    "imulq (%rdi), %rax",
    "imul $10, %rax, %rcx",
    "imull $100000, %edi, %eax",
    // Shifts and bits.
    "rolq $13, %rdi",
    "roll $3, %eax",
    "rorq %cl, %rax",
    "shll %cl, %eax",
    "shlq $1, %rax",
    "shrl %eax",
    "sarl $31, %edx",
    "salq $2, (%rdi)",
    "rclq %rax",
    "rcrb $1, %al",
    "shldq $4, %rbx, %rax",
    "shrdl %cl, %edx, %eax",
    "bswap %eax",
    "bswapq %r12",
    "bsfq %rax, %rcx",
    "bsrl %edx, %eax",
    "tzcntq %rsi, %rdi",
    "lzcntl %eax, %eax",
    "popcntq (%rdi), %rax",
    "btl $2, %eax",
    "btq %rsi, %rdi",
    "btsq $63, (%rdi)",
    "btrl %eax, %ecx",
    "btcq $1, %rdx",
    // Condition codes.
    "sete %al",
    "setc %r9b",
    "setne (%rdi)",
    "setb %sil",
    "setge %dl",
    "cmovneq %rcx, %rax",
    "cmovel %edx, %eax",
    "cmovbq (%rdi), %rax",
    "cmova %rsi, %rdi",
    // Atomics.
    "lock xaddl %eax, (%rdi)",
    "lock; xaddl %eax, (%rdi)",
    "lock xaddq %rax, 8(%rdi)",
    "lock xaddw %ax, (%rdi)",
    "lock; xaddw %ax, (%rdi)",
    "lock cmpxchgl %ecx, (%rdi)",
    "lock; cmpxchgq %rsi, (%rdx)",
    "lock incl (%rdi)",
    "lock; decq (%rax)",
    "lock addl $1, (%rdi)",
    "lock orl $1, (%rsi)",
    "lock addw $1, %fs:(%rdi)",
    "lock cmpxchg16b (%rdi)",
    "lock cmpxchg8b (%rsi)",
    "xaddq %rax, (%rdi)",
    "cmpxchgl %ecx, (%rdi)",
    // Stack.
    "push %rbx",
    "pushq %r12",
    "pop %rbx",
    "popq %r15",
    "pushq $1",
    "pushq $1000",
    "pushq (%rdi)",
    "popq (%rdi)",
    "pushfq; popq %rax",
    "pushq %rax; popfq",
    // String instructions.
    "rep movsb",
    "rep movsq",
    "rep stosb",
    "rep stosl",
    "rep; movsb",
    // SSE.
    "sqrtsd %xmm1, %xmm0",
    "sqrtss (%rdi), %xmm9",
    "addss %xmm2, %xmm3",
    "addsd 8(%rax), %xmm15",
    "subsd %xmm8, %xmm0",
    "mulss %xmm0, %xmm1",
    "divsd %xmm4, %xmm5",
    "minsd %xmm1, %xmm2",
    "maxss %xmm1, %xmm2",
    "xorps %xmm0, %xmm0",
    "xorpd %xmm8, %xmm8",
    "andps %xmm1, %xmm2",
    "andnpd (%rdi), %xmm2",
    "orps %xmm3, %xmm4",
    "ucomisd %xmm1, %xmm0",
    "comiss (%rsi), %xmm3",
    "movss %xmm1, %xmm2",
    "movss (%rdi), %xmm0",
    "movsd %xmm0, (%rdi)",
    "movsd (%rax), %xmm12",
    "movaps %xmm1, %xmm2",
    "movapd (%rdi), %xmm0",
    "movups %xmm0, (%rdi)",
    "movupd %xmm9, %xmm10",
    "movdqa %xmm1, %xmm2",
    "movdqu (%rdi), %xmm3",
    "movdqu %xmm3, (%rsi)",
    "movd %eax, %xmm0",
    "movd %xmm1, %ecx",
    "movq %rax, %xmm0",
    "movq %xmm8, %r9",
    "movq %xmm1, %xmm2",
    "movq (%rdi), %xmm0",
    "movq %xmm0, (%rdi)",
    "movd (%rdi), %xmm5",
    "cvtss2sd %xmm1, %xmm2",
    "cvtsd2ss (%rdi), %xmm2",
    // x87 control.
    "fnstcw (%rdi)",
    "fldcw (%rdi)",
    // Branches to local labels.
    "1: jmp 1b",
    "jz 1f\nnop\n1:",
    "1: decl %ecx\njnz 1b",
    "jmp 1f\n.quad 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0\n1:",
    "1:\n.quad 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0\njne 1b",
    "1: 2: jmp 2b",
    // Data and statements.
    ".byte 0x0f, 0x01, 0xd0",
    ".byte 0xf3, 0x90",
    ".word 0x1234",
    ".short -1",
    ".long 0xdeadbeef",
    ".quad -2",
    "movq %rbx, %rsi\ncpuid\nxchgq %rbx, %rsi",
    "xchgq %rbx, %rsi\n\tcpuid\n\txchgq %rbx, %rsi",
    "rolq $3, %rdi ; rolq $13, %rdi\nrolq $61, %rdi ; rolq $51, %rdi\nxchgq %rbx,%rbx",
    "nop # comment\nnop // comment\nnop /* comment */",
    "movl $1, %eax\n\t# a comment line\n\tcpuid",
];

/// The pinned core: bodies and the bytes LLVM 22 and 19 give them.
const PINNED: &[(&str, &[u8])] = &[
    ("cpuid", &[0x0f, 0xa2]),
    ("rdtsc", &[0x0f, 0x31]),
    ("xgetbv", &[0x0f, 0x01, 0xd0]),
    ("pause", &[0xf3, 0x90]),
    ("rep; nop", &[0xf3, 0x90]),
    (".byte 0x0f, 0x01, 0xd0", &[0x0f, 0x01, 0xd0]),
    ("rolq $13, %rdi", &[0x48, 0xc1, 0xc7, 0x0d]),
    ("xchgq %rbx, %rsi", &[0x48, 0x87, 0xf3]),
    ("xchg %rax, %rbx", &[0x48, 0x93]),
    ("movq %rbx, %rsi", &[0x48, 0x89, 0xde]),
    ("lock xaddl %eax, (%rdi)", &[0xf0, 0x0f, 0xc1, 0x07]),
    ("lock xaddw %ax, (%rdi)", &[0x66, 0xf0, 0x0f, 0xc1, 0x07]),
    ("lock; xaddw %ax, (%rdi)", &[0xf0, 0x66, 0x0f, 0xc1, 0x07]),
    ("lock addw $1, %fs:(%rdi)", &[0x64, 0x66, 0xf0, 0x83, 0x07, 0x01]),
    ("bswapq %r12", &[0x49, 0x0f, 0xcc]),
    ("rep movsb", &[0xf3, 0xa4]),
    ("movq %fs:0, %rax", &[0x64, 0x48, 0x8b, 0x04, 0x25, 0, 0, 0, 0]),
    ("int $3", &[0xcc]),
    ("int $0x80", &[0xcd, 0x80]),
    ("pushq $1", &[0x6a, 0x01]),
    ("test (%rdi), %eax", &[0x85, 0x07]),
    ("1: jmp 1b", &[0xeb, 0xfe]),
];

/// Bodies refused, with the refusal.
const REFUSED: &[(&str, &str)] = &[
    ("frob %eax", "asm:mnemonic/frob"),
    ("vaddps %xmm0, %xmm1, %xmm2", "asm:mnemonic/vaddps"),
    ("movd %rax, %xmm0", "asm:operand/register"),
    ("addsd %xmm0, (%rdi)", "asm:operand/form"),
    ("movsxd %ecx, %rdx", "asm:mnemonic/movsxd"),
    ("x: nop", "asm:label"),
    (".p2align 4", "asm:directive/p2align"),
    (".text", "asm:directive/text"),
    ("jmp foo", "asm:operand/symbol"),
    ("call foo", "asm:mnemonic/call"),
    ("movq foo(%rip), %rax", "asm:operand/symbol"),
    ("jmp 3f", "asm:operand/label"),
    ("jmp *%rax", "asm:operand/form"),
    ("inc (%rax)", "asm:operand/form"),
    ("movl %eax, %rbx", "asm:operand/register"),
    ("movl (%eax), %ebx", "asm:operand/register"),
    ("movl (,%rsp,2), %ebx", "asm:operand/register"),
    ("movl (%rax,%rbx,3), %ecx", "asm:operand/range"),
    ("lock", "asm:operand/form"),
    ("lock movl %eax, (%rdi)", "asm:operand/form"),
    ("lock; jmp 1f\n1:", "asm:operand/form"),
    ("rep cmpsb", "asm:mnemonic/cmpsb"),
    (".byte 256", "asm:operand/range"),
    (".short 0x10000", "asm:operand/range"),
    ("int $256", "asm:operand/range"),
    ("rolq $256, %rax", "asm:operand/range"),
    ("movq (%rdi", "asm:operand/syntax"),
    ("\"unterminated", "asm:lexical"),
    ("/* unterminated", "asm:lexical"),
];

fn hod(body: &str) -> Result<Vec<u8>, Refusal> {
    let mut code = Vec::new();
    assemble(body, &mut code, &mut String::new())?;
    Ok(code)
}

/// The bytes of `.text` of `source` as `llvm-mc-<major>` assembles it.
fn llvm(major: u32, source: &str, scratch: &Scratch) -> Option<Vec<u8>> {
    let mc = tool(&format!("llvm-mc-{major}"))?;
    let objcopy = tool(&format!("llvm-objcopy-{major}"))?;
    let src = scratch.write(&format!("all-{major}.s"), source);
    let obj = scratch.join(format!("all-{major}.o"));
    let bin = scratch.join(format!("all-{major}.bin"));
    let out = run(
        &mc,
        [
            "--triple=x86_64-unknown-linux-gnu".as_ref(),
            "-filetype=obj".as_ref(),
            "-o".as_ref(),
            obj.as_os_str(),
            src.as_os_str(),
        ],
    );
    assert!(out.success(), "llvm-mc-{major} refused the accepted bodies:\n{}", out.stderr_text());
    let out = run(
        &objcopy,
        [
            "-O".as_ref(),
            "binary".as_ref(),
            "--only-section=.text".as_ref(),
            obj.as_os_str(),
            bin.as_os_str(),
        ],
    );
    assert!(out.success(), "llvm-objcopy failed: {}", out.stderr_text());
    Some(std::fs::read(&bin).expect("objcopy output"))
}

#[test]
fn accepted_bodies_match_llvm_mc() {
    let mut ours = Vec::new();
    let mut spans = Vec::new();
    let mut source = String::new();
    for body in ACCEPTED {
        let bytes = hod(body).unwrap_or_else(|e| panic!("{body:?} refused: {e}"));
        spans.push((body, ours.len(), bytes.len()));
        ours.extend(bytes);
        source.push_str(body);
        source.push('\n');
    }
    let scratch = Scratch::new("hod-asm-mc-x64");
    for major in [22, 19] {
        let Some(theirs) = llvm(major, &source, &scratch) else { continue };
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
            "hod_asm x64: {} bodies, {} bytes equal llvm-mc-{major}'s",
            spans.len(),
            ours.len()
        );
    }
}

#[test]
fn pinned_bodies() {
    for &(body, bytes) in PINNED {
        assert_eq!(hod(body).as_deref(), Ok(bytes), "{body:?}");
    }
}

#[test]
fn refused_bodies_are_named() {
    for &(body, name) in REFUSED {
        assert_eq!(hod(body).err().map(|r| r.0), Some(name), "{body:?}");
    }
}
