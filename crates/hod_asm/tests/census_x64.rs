//! `spec/asm.md` §13: the census of the x86-64 inline-assembly templates
//! real code writes (WP-199), each body as the template reads once its
//! operands are substituted the way `llc` prints them. Every body the
//! assembler accepts encodes to the bytes `llvm-mc` 22 and 19 write for
//! the same text; every body it refuses is refused by the name the
//! table gives. The table is `spec/landed/codegen/WP-199.md`'s.

use std::fmt::Write as _;

use hod_asm::x64::assemble;
use hod_testutil::{Scratch, run, tool};

/// What hod does with a census body.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Want {
    /// Assembled, with this many symbol fields.
    Ok(usize),
    /// Refused by this name.
    No(&'static str),
}

use Want::{No, Ok};

/// One census row: the substituted body, where it is written, the number
/// of templates of that shape the census found, and what hod does.
struct Row {
    body: &'static str,
    from: &'static str,
    count: u32,
    want: Want,
}

const fn row(body: &'static str, from: &'static str, count: u32, want: Want) -> Row {
    Row { body, from, count, want }
}

/// The census, most frequent shape first within each group.
const CENSUS: &[Row] = &[
    // Breakpoints, counters, identification.
    row("int3", "gtest.cc (4 copies), hwasan, kiln-built modules (10)", 14, Ok(0)),
    row("int $3", "doctest.h, debugger.h", 4, Ok(0)),
    row("rdtsc", "benchmark cycleclock.h (2+2), abseil unscaledcycleclock", 6, Ok(0)),
    row("cpuid", "cpuid.h, Host.cpp, abseil, msan_test, xray, kmp.h", 12, Ok(0)),
    row(
        "xchgq %rbx, %rsi\ncpuid\nxchgq %rbx, %rsi",
        "cpuid.h __cpuid, __cpuid_count (64-bit)",
        2,
        Ok(0),
    ),
    row("movq %rbx, %rsi\ncpuid\nxchgq %rbx, %rsi", "Host.cpp, cpu_model/x86.c", 4, Ok(0)),
    row(".byte 0x0f, 0x01, 0xd0", "Host.cpp, cpu_model/x86.c (xgetbv)", 2, Ok(0)),
    row("xgetbv", "blake3_dispatch.c", 1, Ok(0)),
    row("rdtscp", "xray_x86_64.inc", 1, Ok(0)),
    row("rdtscp\nshl $32, %rdx\nor %rdx, %rax\nlfence", "abseil nanobenchmark.cc", 1, Ok(0)),
    row("rdtscp\nlfence", "abseil nanobenchmark.cc", 1, Ok(0)),
    row("lfence\nrdtsc\nshl $32, %rdx\nor %rdx, %rax\nlfence", "abseil nanobenchmark.cc", 1, Ok(0)),
    row("lfence\nrdtsc\nlfence", "abseil nanobenchmark.cc", 1, Ok(0)),
    row("pause", "rpmalloc.c, sanitizer_atomic_clang.h", 2, Ok(0)),
    row("rdmsr", "clang intrin.h __readmsr", 1, Ok(0)),
    row("hlt", "clang intrin.h __halt", 1, Ok(0)),
    row("nop", "clang intrin.h __nop, kmp_os.h, ompt callback.h", 4, Ok(0)),
    row("enclu", "clang sgxintrin.h", 1, Ok(0)),
    row("encls", "clang sgxintrin.h", 1, Ok(0)),
    row("enclv", "clang sgxintrin.h", 1, Ok(0)),
    row("pconfig", "clang pconfigintrin.h", 1, Ok(0)),
    row("hreset $0", "clang hresetintrin.h", 1, Ok(0)),
    row("int3\nnopl 64(%rax)", "hwasan_checks.h (2)", 2, Ok(0)),
    // Prefetches, flushes, fences.
    row("prefetchw (%rax)", "abseil prefetch.h (2), prefetch_test.cc module", 3, Ok(0)),
    row("prefetchnta (%rax)", "scudo internal_defs.h, sanitizer_internal_defs.h", 2, Ok(0)),
    row("prefetcht0 64(%rax)", "protobuf port_def.inc", 1, Ok(0)),
    row("lock; addl $0, 0(%rsp)", "kmp_barrier.cpp", 1, Ok(0)),
    // Byte swaps, bit operations, division.
    row("bswapl %eax", "asm/swab.h __arch_swab32", 1, Ok(0)),
    row("bswapq %rax", "asm/swab.h __arch_swab64", 1, Ok(0)),
    row("bswapl %eax ; bswapl %ecx ; xchgl %eax,%ecx", "asm/swab.h (32-bit swab64)", 1, Ok(0)),
    row("btr %rcx,%rax", "libstdc++ experimental/bits/simd.h", 1, Ok(0)),
    row("btc %rcx, %rax", "protobuf varint_shuffle.h", 1, Ok(0)),
    row("ror $7,%eax\n\tmovb %cl,%al", "protobuf parse_context.h", 1, Ok(0)),
    row("divq %rcx", "compiler-rt udivmodti4.c", 1, Ok(0)),
    row("crc32q %rcx, %rax", "bolt CallGraph.cpp (2)", 2, Ok(0)),
    // Atomics.
    row("lock\nxadd %rax,(%rcx)", "ittnotify_config.h (2)", 2, Ok(0)),
    row(
        ".byte 0xf2 ; lock ; xchg %eax, (%rcx)",
        "clang immintrin.h _InterlockedExchange_HLE*",
        4,
        Ok(0),
    ),
    row(
        ".byte 0xf2 ; lock ; cmpxchg %ecx, (%rdx)",
        "clang immintrin.h _InterlockedCompareExchange_HLE*",
        4,
        Ok(0),
    ),
    row(".byte 0xf2;xchg %eax,(%rcx)", "kmp_lock.cpp HLE acquire", 1, Ok(0)),
    row(".byte 0xf3;movl %eax,(%rcx)", "kmp_lock.cpp HLE release", 1, Ok(0)),
    row(".byte 0x0f; .byte 0x01; .byte 0xd5", "kmp_lock.cpp (xend)", 1, Ok(0)),
    row(".byte 0xC6; .byte 0xF8; .byte 0xff", "kmp_lock.cpp (xabort)", 1, Ok(0)),
    row("movl %eax, %ebx; .byte 0x64, 0x67, 0x90", "kmp_itt.h", 1, Ok(0)),
    // String instructions.
    row("rep movsb", "protobuf cardinality.h, libc op_x86.h, clang intrin.h", 3, Ok(0)),
    row("rep movsl", "clang intrin.h __movsd", 1, Ok(0)),
    row("rep movsw", "clang intrin.h __movsw", 1, Ok(0)),
    row("rep movsq", "clang intrin.h __movsq", 1, Ok(0)),
    row("rep stosl", "clang intrin.h __stosd", 1, Ok(0)),
    row("rep stosw", "clang intrin.h __stosw", 1, Ok(0)),
    row("rep stosq", "clang intrin.h __stosq", 1, Ok(0)),
    // System calls.
    row("syscall", "sanitizer_syscall_linux_x86_64.inc (4)", 4, Ok(0)),
    row("mov %r8, %r10;syscall", "sanitizer_syscall_linux_x86_64.inc", 1, Ok(0)),
    row("mov %r8, %r10;mov %r9, %r8;syscall", "sanitizer_syscall_linux_x86_64.inc", 1, Ok(0)),
    row(
        "mov %r8, %r10;mov %r9, %r8;mov %r11, %r9;syscall",
        "sanitizer_syscall_linux_x86_64.inc",
        1,
        Ok(0),
    ),
    row("movq $2, %rax\nsyscall", "bolt sys_x86_64.h (24)", 24, Ok(0)),
    // Thread pointer and frame reads.
    row("mov %fs:40, %rax", "glibc bits/platform/features.h", 1, Ok(0)),
    row("mov %fs:72, %rax", "glibc bits/platform/features.h", 1, Ok(0)),
    row("mov %fs:0, %rax", "sanitizer_linux.h, rpmalloc.c", 2, Ok(0)),
    row("mov %fs:8,%rax", "sanitizer_linux_libcdep.cpp", 1, Ok(0)),
    row("mov %fs:16,%rax", "sanitizer_linux_libcdep.cpp", 1, Ok(0)),
    row("ror  $0x11,     %rax \nxor  %fs:0x30, %rax ", "tsan_platform_linux.cpp", 1, Ok(0)),
    row("mov %rbp, %rax", "glog stacktrace_x86-inl.h", 1, Ok(0)),
    row("leaq 0(%rip),%rax;", "abseil symbolize_test.cc (2)", 2, Ok(0)),
    row("lea 0(%rip), %rax", "sanitizer_stacktrace.h", 1, Ok(0)),
    row("leaq LLVMRTDyldTLSSpace@tpoff, %rax", "llvm-rtdyld.cpp", 1, Ok(1)),
    // x87 and SSE control.
    row("fwait", "libc FEnvImpl.h, llvm-exegesis, kiln-built modules (5)", 7, Ok(0)),
    row("fnstcw (%rax)", "glibc fpu_control.h, libc FEnvImpl.h, fp_mode.c", 3, Ok(0)),
    row("fldcw (%rax)", "glibc fpu_control.h, libc FEnvImpl.h, kmp.h", 3, Ok(0)),
    row("fnclex", "libc FEnvImpl.h, kmp.h", 2, Ok(0)),
    row("fstcw (%rax)", "kmp.h", 1, Ok(0)),
    row("fnstsw (%rax)", "libc FEnvImpl.h", 1, Ok(0)),
    row("stmxcsr (%rax)", "libc FEnvImpl.h", 1, Ok(0)),
    row("ldmxcsr (%rax)", "libc FEnvImpl.h", 1, Ok(0)),
    row("fnstenv (%rax)", "libc FEnvImpl.h", 1, Ok(0)),
    row("fldenv (%rax)", "libc FEnvImpl.h", 1, Ok(0)),
    row("fstenv (%rax)\n\tandw $0x7f00, 4(%rax)\n\tfldenv (%rax)", "kmp.h", 1, Ok(0)),
    row("sqrtss %xmm0, %xmm0", "libc x86_64/sqrt.h", 1, Ok(0)),
    row("sqrtsd %xmm0, %xmm0", "libc x86_64/sqrt.h (2)", 2, Ok(0)),
    row("divpd\t%xmm1, %xmm0", "libstdc++ simd_x86.h", 1, Ok(0)),
    row("divps\t%xmm1, %xmm0", "libstdc++ simd_x86.h", 1, Ok(0)),
    // Port input and output.
    row("inb %dx,%al", "glibc sys/io.h, clang intrin.h", 2, Ok(0)),
    row("inw %dx,%ax", "glibc sys/io.h, clang intrin.h", 2, Ok(0)),
    row("inl %dx,%eax", "glibc sys/io.h, clang intrin.h", 2, Ok(0)),
    row("outb %al,%dx", "glibc sys/io.h, clang intrin.h", 2, Ok(0)),
    row("outw %ax,%dx", "glibc sys/io.h, clang intrin.h", 2, Ok(0)),
    row("outl %eax,%dx", "glibc sys/io.h, clang intrin.h", 2, Ok(0)),
    row("inb %dx,%al\noutb %al,$0x80", "glibc sys/io.h inb_p", 1, Ok(0)),
    row("inw %dx,%ax\noutb %al,$0x80", "glibc sys/io.h inw_p", 1, Ok(0)),
    row("inl %dx,%eax\noutb %al,$0x80", "glibc sys/io.h inl_p", 1, Ok(0)),
    row("outb %al,%dx\noutb %al,$0x80", "glibc sys/io.h outb_p", 1, Ok(0)),
    row("outw %ax,%dx\noutb %al,$0x80", "glibc sys/io.h outw_p", 1, Ok(0)),
    row("outl %eax,%dx\noutb %al,$0x80", "glibc sys/io.h outl_p", 1, Ok(0)),
    // What stays refused.
    row("mov %cr3, %rax", "clang intrin.h __readcr3", 1, No("asm:operand/register")),
    row("mov %rax, %cr3", "clang intrin.h __writecr3", 1, No("asm:operand/register")),
    row("vdivpd\t%xmm2, %xmm1, %xmm0", "libstdc++ simd_x86.h", 1, No("asm:mnemonic/vdivpd")),
    row("vdivps\t%xmm2, %xmm1, %xmm0", "libstdc++ simd_x86.h", 1, No("asm:mnemonic/vdivps")),
    row("jmpq *%rdx", "libunwind UnwindLevel1.c (CET)", 1, No("asm:operand/form")),
    row(
        "syscall\ntestq %rax,%rax\njnz 1f\nxorq %rbp,%rbp\npopq %rax\npopq %rdi\ncall *%rax\n\
         movq %rax,%rdi\nmovq $60,%rax\nsyscall\n1:",
        "sanitizer_linux.cpp internal_clone",
        1,
        No("asm:operand/form"),
    ),
    row(
        "leaq __hot_end(%rip), %rax\nmovabsq $__hot_end, %rcx",
        "bolt sys_x86_64.h",
        1,
        No("asm:operand/symbol"),
    ),
    row(
        "call __bolt_fini_trampoline",
        "bolt instr.cpp, hugify.cpp (3)",
        3,
        No("asm:mnemonic/call"),
    ),
    row(
        "1: .byte  0xC7; .byte 0xF8;\n.long  1f-1b-6\njmp   2f\n1:  movl  %eax,%ecx\n2:",
        "kmp_lock.cpp (xbegin as bytes)",
        1,
        No("asm:operand/expression"),
    ),
    row(
        "movq %rbp, %r15\nmovq %rax, %rbp\n.cfi_remember_state\n.cfi_def_cfa rbp, 0",
        "valgrind.h CALL_FN_* on amd64 (15 macros)",
        15,
        No("asm:directive/cfi_remember_state"),
    ),
];

fn hod(body: &str) -> Result<(Vec<u8>, usize), hod_asm::Refusal> {
    let (mut code, mut refs) = (Vec::new(), Vec::new());
    assemble(body, &mut code, &mut refs, &mut String::new())?;
    Result::Ok((code, refs.len()))
}

/// Every census body is accepted or refused as the table says.
#[test]
fn census_bodies_are_as_the_table_says() {
    let mut report = String::new();
    let (mut accepted, mut templates) = (0, 0);
    for r in CENSUS {
        let got = match hod(r.body) {
            Result::Ok((_, refs)) => Ok(refs),
            Err(e) => No(e.0),
        };
        if got != r.want {
            writeln!(report, "  {:?} ({}): {got:?}, the table says {:?}", r.body, r.from, r.want)
                .unwrap();
        }
        templates += r.count;
        if matches!(got, Ok(_)) {
            accepted += r.count;
        }
    }
    assert!(report.is_empty(), "census rows differ:\n{report}");
    println!("x86-64 census: {accepted} of {templates} templates assembled");
}

/// The `.text` bytes of `source` as `llvm-mc-<major>` assembles it.
fn llvm(major: u32, source: &str, scratch: &Scratch) -> Option<Vec<u8>> {
    let mc = tool(&format!("llvm-mc-{major}"))?;
    let objcopy = tool(&format!("llvm-objcopy-{major}"))?;
    let src = scratch.write(&format!("census-{major}.s"), source);
    let obj = scratch.join(format!("census-{major}.o"));
    let bin = scratch.join(format!("census-{major}.bin"));
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
    assert!(out.success(), "llvm-mc-{major} refused the census bodies:\n{}", out.stderr_text());
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

/// Every accepted census body without a symbol field is `llvm-mc`'s
/// bytes (the one with a field is `llvm_mc_x64.rs`'s `SYMBOLIC`).
#[test]
fn accepted_census_bodies_are_llvm_mc_bytes() {
    let (mut ours, mut spans, mut source) = (Vec::new(), Vec::new(), String::new());
    for r in CENSUS.iter().filter(|r| r.want == Ok(0)) {
        let Result::Ok((bytes, _)) = hod(r.body) else { continue };
        spans.push((r.body, ours.len(), bytes.len()));
        ours.extend(bytes);
        source.push_str(r.body);
        source.push('\n');
    }
    let scratch = Scratch::new("hod-asm-census-x64");
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
        println!("x86-64 census: {} bodies equal llvm-mc-{major}'s", spans.len());
    }
}
