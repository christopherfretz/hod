//! `spec/asm.md` §10: every x86-64 instruction form the assembler
//! accepts encodes to the bytes LLVM 22's and LLVM 19's `llvm-mc`
//! produce for the same text, a pinned core set encodes to bytes fixed
//! here (so the check holds where no LLVM is installed), and the forms
//! it refuses are refused by name.

use std::fmt::Write as _;

use hod_asm::Refusal;
use hod_asm::x64::{Ref, RefKind, assemble};
use hod_testutil::{Scratch, run, tool};

/// Bodies the assembler accepts. Each is assembled alone by hod; LLVM
/// assembles them all in one file. A body's numeric labels are its own.
const ACCEPTED: &[&str] = &[
    // §14.
    "ret",
    "retq",
    "leave",
    "leaveq",
    "fucompp",
    "fchs",
    "fabs",
    "fldz",
    "fld1",
    "fprem",
    "fprem1",
    "fsqrt",
    "frndint",
    "fnop",
    "ftst",
    "fxam",
    "fldl2t",
    "fldl2e",
    "fldpi",
    "fldlg2",
    "fldln2",
    "f2xm1",
    "fyl2x",
    "fptan",
    "fpatan",
    "fxtract",
    "fdecstp",
    "fincstp",
    "fyl2xp1",
    "fsincos",
    "fscale",
    "fsin",
    "fcos",
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
    "fwait",
    "wait",
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
    // §13 (WP-199): prefetches, flushes, state.
    "prefetchw (%rax)",
    "prefetch 8(%rdi)",
    "prefetchwt1 (%r9)",
    "prefetchnta (%rax)",
    "prefetcht0 64(%rax)",
    "prefetcht1 (%r9)",
    "prefetcht2 -8(%rsp,%rcx,4)",
    "prefetchw %fs:(%rax)",
    "clflush (%rax)",
    "clflushopt 8(%rax)",
    "clwb (%r12)",
    "xsave (%rax)",
    "xrstor64 (%rax)",
    "fxsave (%rsp)",
    "xsavec64 16(%rdi)",
    // §13: the multi-byte no-op.
    "nopl 64(%rax)",
    "nopw (%rax,%rax)",
    "nopq (%rax)",
    "nopl 0x0(%rax,%rax,1)",
    "nopl %eax",
    "nopw %r9w",
    "nop %eax",
    "nopq %r15",
    // §13: system, enclave, flags.
    "rdmsr",
    "wrmsr",
    "rdpmc",
    "xsetbv",
    "xend",
    "xtest",
    "rdpkru",
    "wrpkru",
    "serialize",
    "endbr64",
    "enclu",
    "encls",
    "enclv",
    "pconfig",
    "hreset $0",
    "hreset $255",
    "clc",
    "stc",
    "cmc",
    "lahf",
    "sahf",
    "cli",
    "sti",
    "rdrand %eax",
    "rdrandw %ax",
    "rdseedq %r10",
    "rdpid %rax",
    "rdfsbase %rax",
    "rdgsbasel %ecx",
    "wrfsbase %r8",
    "wrgsbaseq %rdi",
    // §13: ports.
    "inb %dx, %al",
    "inw %dx, %ax",
    "inl %dx, %eax",
    "in %dx, %al",
    "inb $0x60, %al",
    "inw $1, %ax",
    "inl $255, %eax",
    "outb %al, %dx",
    "outw %ax, %dx",
    "outl %eax, %dx",
    "out %al, %dx",
    "outb %al, $0x80",
    "outw %ax, $0",
    "outl %eax, $3",
    // §13: x87 and MXCSR state.
    "fnstsw (%rax)",
    "fnstsw %ax",
    "fnstsw",
    "fnclex",
    "fninit",
    "fnstenv (%rax)",
    "fldenv (%rax)",
    "fnsave (%rsp)",
    "frstor 8(%rsp)",
    "stmxcsr (%rax)",
    "ldmxcsr (%r10)",
    "stmxcsr -4(%rsp)",
    "fstcw (%rax)",
    "fstsw (%rax)",
    "fstsw %ax",
    "fstsw",
    "fstenv (%rax)",
    "fsave (%rax)",
    "fclex",
    "finit",
    "fstenv (%rax)\n\tandw $0x7f00, 4(%rax)\n\tfldenv (%rax)",
    // §13: crc32.
    "crc32b %cl, %eax",
    "crc32b %r9b, %eax",
    "crc32b %sil, %eax",
    "crc32b %cl, %rax",
    "crc32b %r10b, %r11",
    "crc32w %cx, %eax",
    "crc32l %ecx, %eax",
    "crc32l %r8d, %r9d",
    "crc32q %rcx, %rax",
    "crc32q %r15, %r8",
    "crc32 %cl, %eax",
    "crc32 %rcx, %rax",
    // §13: packed floating point.
    "divps %xmm1, %xmm0",
    "divpd (%rdi), %xmm9",
    "addps %xmm2, %xmm3",
    "subpd %xmm8, %xmm1",
    "mulps 16(%rsp), %xmm0",
    "sqrtpd %xmm1, %xmm1",
    "minps %xmm4, %xmm5",
    "maxpd %xmm15, %xmm0",
    "unpcklps %xmm1, %xmm0",
    "unpckhpd (%rax), %xmm2",
    // §13: string instructions, alone and repeated.
    "movsb",
    "movsw",
    "movsl",
    "movsq",
    "movsd",
    "stosb",
    "stosw",
    "stosl",
    "stosq",
    "lodsb",
    "lodsw",
    "lodsl",
    "lodsq",
    "scasb",
    "scasw",
    "scasl",
    "scasq",
    "cmpsb",
    "cmpsw",
    "cmpsl",
    "cmpsq",
    "rep movsl",
    "rep movsw",
    "rep movsq",
    "rep movsd",
    "rep stosl",
    "rep stosw",
    "rep stosq",
    "rep lodsb",
    "rep; lodsq",
    "repe cmpsb",
    "repz cmpsq",
    "rep cmpsl",
    "repne scasb",
    "repnz scasw",
    "repne; scasq",
    "repne cmpsb",
    "repne nop",
    // §13: `lock` on every lockable memory destination.
    "lock btsl %eax, (%rdi)",
    "lock btrq $3, (%rdi)",
    "lock btcw %ax, 2(%rdi)",
    "lock negl (%rdi)",
    "lock notq (%rdi)",
    "lock andl %eax, (%rdi)",
    "lock orb $1, (%rdi)",
    "lock xorq %rax, (%rdi)",
    "lock subl $1, (%rdi)",
    "lock adcl %eax, (%rdi)",
    "lock sbbq $5, (%rdi)",
    "lock incq (%rdi)",
    "lock decb (%rdi)",
    "lock xchgl %eax, (%rdi)",
    "lock cmpxchgq %rcx, (%rdx)",
    "lock cmpxchg16b (%rdi)",
    "lock; addl $0, 0(%rsp)",
    "lock; btsq $63, (%rax)",
    ".byte 0xf2 ; lock ; xchg %eax, (%rcx)",
    // §13: the census's sequences.
    "rdtscp\nshl $32, %rdx\nor %rdx, %rax\nlfence",
    "bswapl %eax ; bswapl %ecx ; xchgl %eax,%ecx",
    "inb %dx,%al\noutb %al,$0x80",
    "int3\nnopl 64(%rax)",
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
    ("jmp $foo", "asm:operand/form"),
    ("call foo@gotpcrel", "asm:operand/form"),
    ("callw foo", "asm:mnemonic/callw"),
    ("lock call foo", "asm:operand/form"),
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
    // §13: what stays refused, and the forms LLVM's assembler refuses.
    ("rep stosd", "asm:mnemonic/stosd"),
    ("lodsd", "asm:mnemonic/lodsd"),
    ("rep movsb (%rsi), (%rdi)", "asm:operand/form"),
    ("movsb (%rsi), (%rdi)", "asm:operand/form"),
    ("nop (%rax)", "asm:operand/form"),
    ("nopb %al", "asm:operand/range"),
    ("inb %cx, %al", "asm:operand/register"),
    ("inb %dx, %bl", "asm:operand/register"),
    ("inq %dx, %rax", "asm:operand/range"),
    ("inb $256, %al", "asm:operand/range"),
    ("crc32q %cl, %eax", "asm:operand/register"),
    ("crc32q (%rdi), %rax", "asm:operand/form"),
    ("fnstsw %bx", "asm:operand/register"),
    ("lock fstsw", "asm:operand/form"),
    ("lock prefetchw (%rax)", "asm:operand/form"),
    ("prefetchw %rax", "asm:operand/form"),
    ("rdpid %eax", "asm:operand/register"),
    ("rdrandb %al", "asm:operand/register"),
    ("mov %cr3, %rax", "asm:operand/register"),
    ("vdivpd %xmm2, %xmm1, %xmm0", "asm:mnemonic/vdivpd"),
    ("call *%rax", "asm:operand/form"),
    ("movabsq $sym, %rcx", "asm:operand/symbol"),
    (".cfi_remember_state", "asm:directive/cfi_remember_state"),
    ("fadd %st(1), %st", "asm:operand/register"),
    ("fld %st(1)", "asm:operand/register"),
    (".reloc .+4, BFD_RELOC_NONE, sym", "asm:operand/expression"),
    (".reloc ., R_X86_64_PC32, sym", "asm:directive/reloc"),
    (".reloc ., BFD_RELOC_NONE, 1f\n1:", "asm:operand/symbol"),
    (".reloc ., BFD_RELOC_NONE", "asm:operand/syntax"),
    (".byte 256", "asm:operand/range"),
    (".short 0x10000", "asm:operand/range"),
    ("int $256", "asm:operand/range"),
    ("rolq $256, %rax", "asm:operand/range"),
    ("movq (%rdi", "asm:operand/syntax"),
    ("leaq s@GOTPCREL(%rip), %rax", "asm:operand/specifier"),
    ("movq s@tpoff(%rip), %rax", "asm:operand/specifier"),
    ("leaq s@gottpoff, %rax", "asm:operand/specifier"),
    ("movq $s@tpoff, %rax", "asm:operand/specifier"),
    ("leaq s@gotoff(%rip), %rax", "asm:operand/specifier"),
    ("movl $s, %eax", "asm:operand/symbol"),
    ("movq 1f@tpoff, %rax\n1:", "asm:operand/symbol"),
    ("\"unterminated", "asm:lexical"),
    ("/* unterminated", "asm:lexical"),
];

fn hod(body: &str) -> Result<Vec<u8>, Refusal> {
    let (code, refs) = hod_refs(body)?;
    assert!(refs.is_empty(), "{body:?}: {refs:?}");
    Ok(code)
}

fn hod_refs(body: &str) -> Result<(Vec<u8>, Vec<Ref>), Refusal> {
    let (mut code, mut refs) = (Vec::new(), Vec::new());
    assemble(body, &mut code, &mut refs, &mut String::new())?;
    Ok((code, refs))
}

/// Bodies with a symbol field (`spec/asm.md` §10.2 "Symbols"); `S` is
/// the symbol, renamed per body (llvm-mc does not take one symbol both
/// thread-local and not).
const SYMBOLIC: &[&str] = &[
    "leaq S@tpoff, %rax",
    "leaq S@TPOFF+16, %r9",
    "movq %fs:S@tpoff, %rax",
    "movq %fs:S@tpoff+8, %rax",
    "movl %fs:S@tpoff, %r10d",
    "movl S@tpoff(%rax), %ecx",
    "leaq S@dtpoff(%rax), %rcx",
    "movq S@gottpoff(%rip), %rax",
    "addq S@gottpoff(%rip), %r11",
    "leaq S(%rip), %rax",
    "leaq S+4(%rip), %rdx",
    "leaq 8+S(%rip), %rdx",
    "movl $1, S(%rip)",
    "movb $1, S-1(%rip)",
    "cmpq $1, S@GOTPCREL(%rip)",
    "leaq S@PLT(%rip), %rax",
    "movq S@GOTPCREL(%rip), %rax",
    "movq S@gotpcrel(%rip), %r8",
    "movl S@GOTPCREL(%rip), %eax",
    "movl S@GOTPCREL(%rip), %r8d",
    "addq S@GOTPCREL(%rip), %rax",
    "adcl S@GOTPCREL(%rip), %eax",
    "andq S@GOTPCREL(%rip), %rax",
    "cmpl S@GOTPCREL(%rip), %eax",
    "orl S@GOTPCREL(%rip), %eax",
    "sbbq S@GOTPCREL(%rip), %rax",
    "subl S@GOTPCREL(%rip), %eax",
    "xorq S@GOTPCREL(%rip), %rax",
    "testq %rax, S@GOTPCREL(%rip)",
    "testl %eax, S@GOTPCREL(%rip)",
    "movq %rax, S@GOTPCREL(%rip)",
    "movw S@GOTPCREL(%rip), %ax",
    "addl %eax, S@GOTPCREL(%rip)",
    "imulq S@GOTPCREL(%rip), %rax",
    "movq S+8@GOTPCREL(%rip), %rax",
    "movq S@GOTPCREL+8(%rip), %rax",
    "movq %fs:S@GOTPCREL(%rip), %rax",
    "movl S, %eax",
    "movl %fs:S, %eax",
    "leaq S(%rbx), %rax",
    "movq S(,%rcx,8), %rax",
    "lock xaddl %eax, S(%rip)",
    "lock; incl S(%rip)",
    "jmp 1f\nmovq S@gottpoff(%rip), %rax\n1:",
    // §13: `.reloc` (protobuf's `StrongReferenceToType`, a `Ws` operand).
    "nop\n.reloc ., BFD_RELOC_NONE, S",
    "nop\n.reloc ., BFD_RELOC_NONE, S+8",
    "nop\n.reloc ., R_X86_64_NONE, S-4\nnop",
    // §14: a branch or call to a symbol, near, `R_X86_64_PLT32`.
    "call S",
    "callq S",
    "call S@PLT",
    "call S+16",
    "jmp S",
    "jmp S@plt",
    "jne S",
    "jae S-8",
    "call S+0",
    "call S@PLT+4",
    "jmp 1f\ncall S\n1:",
    "nop\n1: call S\njmp 1b",
    "jmp 1f\n.quad 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0\n.reloc ., BFD_RELOC_NONE, S\n1:",
];

const fn elf_name(kind: RefKind) -> &'static str {
    match kind {
        RefKind::Pc32 => "R_X86_64_PC32",
        RefKind::Plt32 => "R_X86_64_PLT32",
        RefKind::GotPcRel => "R_X86_64_GOTPCREL",
        RefKind::GotPcRelX => "R_X86_64_GOTPCRELX",
        RefKind::RexGotPcRelX => "R_X86_64_REX_GOTPCRELX",
        RefKind::GotTpOff => "R_X86_64_GOTTPOFF",
        RefKind::TpOff32 => "R_X86_64_TPOFF32",
        RefKind::DtpOff32 => "R_X86_64_DTPOFF32",
        RefKind::Abs32S => "R_X86_64_32S",
        RefKind::None => "R_X86_64_NONE",
    }
}

/// The relocation lines of `llvm-objdump -r`: `offset type value`.
fn reloc_line(offset: usize, kind: RefKind, name: &str, addend: i64) -> String {
    let value = match addend {
        0 => name.to_owned(),
        a if a < 0 => format!("{name}-0x{:x}", a.unsigned_abs()),
        a => format!("{name}+0x{a:x}"),
    };
    format!("{offset:016x} {} {value}", elf_name(kind))
}

#[test]
fn symbol_fields_match_llvm_mc() {
    let (mut ours, mut lines, mut source) = (Vec::new(), Vec::new(), String::new());
    for (i, body) in SYMBOLIC.iter().enumerate() {
        let body = body.replace('S', &format!("sym{i}"));
        let (bytes, refs) = hod_refs(&body).unwrap_or_else(|e| panic!("{body:?} refused: {e}"));
        assert_eq!(refs.len(), 1, "{body:?}");
        for r in &refs {
            lines.push(reloc_line(ours.len() + r.offset as usize, r.kind, &r.name, r.addend));
        }
        ours.extend(bytes);
        source.push_str(&body);
        source.push('\n');
    }
    let scratch = Scratch::new("hod-asm-mc-x64-sym");
    for major in [22, 19] {
        let Some(theirs) = llvm(major, &source, &scratch) else { continue };
        assert!(ours == theirs, "LLVM {major}: the bytes differ");
        let Some(objdump) = tool(&format!("llvm-objdump-{major}")) else { continue };
        let obj = scratch.join(format!("all-{major}.o"));
        let out = run(&objdump, ["-r".as_ref(), obj.as_os_str()]);
        assert!(out.success(), "llvm-objdump: {}", out.stderr_text());
        let text = out.stdout_text();
        let mut theirs: Vec<String> = text
            .lines()
            .filter(|l| l.contains("R_X86_64"))
            .map(|l| l.split_whitespace().collect::<Vec<_>>().join(" "))
            .collect();
        // llvm-mc writes a `.reloc` field when its fragment closes, out of
        // offset order; the set of fields is what is compared.
        theirs.sort();
        let mut lines = lines.clone();
        lines.sort();
        assert_eq!(lines, theirs, "LLVM {major}: the relocations differ");
        println!("hod_asm x64: {} symbol fields equal llvm-mc-{major}'s", lines.len());
    }
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
        assert_eq!(hod_refs(body).err().map(|r| r.0), Some(name), "{body:?}");
    }
}
