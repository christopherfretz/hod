; The instructions of the x86-64 census (WP-199, `spec/asm.md` §13), each
; template with every register forced, so its bytes are `llc`'s too;
; `census.c` checks each by what it computes, under an emulated CPU with
; the extensions (`crc32`, `rdtscp`, `lahf` in 64-bit mode). The
; privileged and enclave instructions are in functions nothing calls.

; The prefetch family: no architectural effect.
define void @forced_prefetch(ptr %p) {
  call void asm sideeffect "prefetchw (%rdi)\0A\09prefetch (%rdi)\0A\09prefetchwt1 8(%rdi)\0A\09prefetchnta (%rdi)\0A\09prefetcht0 64(%rdi)\0A\09prefetcht1 (%rdi,%rdi)\0A\09prefetcht2 -8(%rdi)", "{rdi},~{memory},~{dirflag},~{fpsr},~{flags}"(ptr %p)
  ret void
}

; Cache-line flushes and fences.
define void @forced_flush(ptr %p) {
  call void asm sideeffect "clflush (%rdi)\0A\09clflushopt 64(%rdi)\0A\09sfence\0A\09lock; addl $$0, 0(%rsp)", "{rdi},~{memory},~{dirflag},~{fpsr},~{flags}"(ptr %p)
  ret void
}

; The multi-byte no-ops.
define void @forced_nops() {
  call void asm sideeffect "nopl 64(%rax)\0A\09nopw (%rax,%rax)\0A\09nopq (%rax)\0A\09nopl %eax\0A\09nopw %r9w", "~{dirflag},~{fpsr},~{flags}"()
  ret void
}

; MXCSR stored, loaded, stored again.
define void @forced_mxcsr(ptr %p) {
  call void asm sideeffect "stmxcsr (%rdi)\0A\09ldmxcsr (%rdi)\0A\09stmxcsr 4(%rdi)", "{rdi},~{memory},~{dirflag},~{fpsr},~{flags}"(ptr %p)
  ret void
}

; The x87 environment and state: stored, loaded, the waiting forms.
define i16 @forced_x87(ptr %env, ptr %cw, ptr %sw) {
  %r = call i16 asm sideeffect "fnstenv (%rdi)\0A\09fldenv (%rdi)\0A\09fstcw (%rsi)\0A\09fnclex\0A\09fclex\0A\09fnstsw (%rdx)\0A\09fstsw %ax", "={ax},{rdi},{rsi},{rdx},~{memory},~{dirflag},~{fpsr},~{flags}"(ptr %env, ptr %cw, ptr %sw)
  ret i16 %r
}

define void @forced_x87_save(ptr %state, ptr %cw) {
  call void asm sideeffect "fsave (%rdi)\0A\09frstor (%rdi)\0A\09fnsave (%rdi)\0A\09frstor (%rdi)\0A\09fninit\0A\09finit\0A\09fnstcw (%rsi)", "{rdi},{rsi},~{memory},~{dirflag},~{fpsr},~{flags}"(ptr %state, ptr %cw)
  ret void
}

; CRC-32C, the widths of the census.
define i64 @forced_crc32q(i64 %crc, i64 %x) {
  %r = call i64 asm "crc32q %rcx, %rax", "={ax},{ax},{rcx},~{dirflag},~{fpsr},~{flags}"(i64 %crc, i64 %x)
  ret i64 %r
}

define i64 @forced_crc32b_q(i64 %crc, i8 %x) {
  %r = call i64 asm "crc32b %cl, %rax", "={ax},{ax},{cx},~{dirflag},~{fpsr},~{flags}"(i64 %crc, i8 %x)
  ret i64 %r
}

define i32 @forced_crc32(i32 %crc, i32 %x) {
  %r = call i32 asm "crc32b %cl, %eax\0A\09crc32w %cx, %eax\0A\09crc32l %ecx, %eax", "={ax},{ax},{cx},~{dirflag},~{fpsr},~{flags}"(i32 %crc, i32 %x)
  ret i32 %r
}

; abseil nanobenchmark.cc's `rdtscp` stamp.
define i64 @forced_rdtscp() {
  %r = call i64 asm sideeffect "rdtscp\0A\09shl $$32, %rdx\0A\09or %rdx, %rax\0A\09lfence", "={ax},~{rcx},~{rdx},~{memory},~{dirflag},~{fpsr},~{flags}"()
  ret i64 %r
}

; The carry flag set, complemented, cleared (`0x101`); `lahf` and `sahf`
; (`0x203`).
define i32 @forced_carry() {
  %r = call i32 asm sideeffect "xorl %eax, %eax\0A\09xorl %ecx, %ecx\0A\09stc\0A\09cmc\0A\09cmc\0A\09setc %al\0A\09stc\0A\09clc\0A\09cmc\0A\09setc %cl\0A\09shll $$8, %ecx\0A\09orl %ecx, %eax", "={ax},~{rcx},~{dirflag},~{fpsr},~{flags}"()
  ret i32 %r
}

define i32 @forced_lahf() {
  %r = call i32 asm sideeffect "xorl %eax, %eax\0A\09stc\0A\09lahf\0A\09shrl $$8, %eax\0A\09andl $$1, %eax\0A\09movl $$0x100, %ecx\0A\09xchgl %eax, %ecx\0A\09clc\0A\09sahf\0A\09setc %al\0A\09shll $$1, %eax\0A\09orl %ecx, %eax", "={ax},~{rcx},~{dirflag},~{fpsr},~{flags}"()
  ret i32 %r
}

; `lock` on the bit and logic instructions.
define void @forced_lock_bits(ptr %p, i64 %n) {
  call void asm sideeffect "lock btsq $$3, (%rdi)\0A\09lock btrq $$0, (%rdi)\0A\09lock btcq %rsi, (%rdi)\0A\09lock notq 8(%rdi)\0A\09lock negq 16(%rdi)\0A\09lock andq %rsi, 24(%rdi)\0A\09lock xorq $$5, 32(%rdi)\0A\09lock; orl $$0x100, 40(%rdi)", "{rdi},{rsi},~{memory},~{dirflag},~{fpsr},~{flags}"(ptr %p, i64 %n)
  ret void
}

; clang immintrin.h's `_InterlockedExchange_HLEAcquire`.
define i32 @forced_hle_xchg(ptr %p, i32 %v) {
  %r = call i32 asm sideeffect ".byte 0xf2 ; lock ; xchg %eax, (%rcx)", "={ax},{ax},{rcx},~{memory},~{dirflag},~{fpsr},~{flags}"(i32 %v, ptr %p)
  ret i32 %r
}

; `strlen` by `repne scasb`: the count left in `rcx`.
define i64 @forced_repne_scasb(ptr %s) {
  %r = call { ptr, i64 } asm sideeffect "repne scasb", "={rdi},={rcx},{ax},0,1,~{memory},~{dirflag},~{fpsr},~{flags}"(i8 0, ptr %s, i64 -1)
  %c = extractvalue { ptr, i64 } %r, 1
  ret i64 %c
}

; `repe cmpsb`: the count left at the first difference.
define i64 @forced_repe_cmpsb(ptr %a, ptr %b, i64 %n) {
  %r = call { ptr, ptr, i64 } asm sideeffect "repe cmpsb", "={rsi},={rdi},={rcx},0,1,2,~{memory},~{dirflag},~{fpsr},~{flags}"(ptr %a, ptr %b, i64 %n)
  %c = extractvalue { ptr, ptr, i64 } %r, 2
  ret i64 %c
}

; The string instructions without `rep`: 15 bytes moved, a quadword
; loaded, a quadword and a byte stored; the end of the destination.
define ptr @forced_strings(ptr %dst, ptr %src) {
  %r = call { ptr, ptr } asm sideeffect "movsb\0A\09movsw\0A\09movsl\0A\09movsq\0A\09lodsq\0A\09stosq\0A\09movb $$0x2a, %al\0A\09stosb", "={rdi},={rsi},0,1,~{rax},~{memory},~{dirflag},~{fpsr},~{flags}"(ptr %dst, ptr %src)
  %d = extractvalue { ptr, ptr } %r, 0
  ret ptr %d
}

; libstdc++ simd_x86.h's division, on the low lane.
define double @forced_divpd(double %a, double %b) {
  %r = call double asm "divpd %xmm1, %xmm0", "={xmm0},{xmm0},{xmm1},~{dirflag},~{fpsr},~{flags}"(double %a, double %b)
  ret double %r
}

define float @forced_divps(float %a, float %b) {
  %r = call float asm "divps %xmm1, %xmm0", "={xmm0},{xmm0},{xmm1},~{dirflag},~{fpsr},~{flags}"(float %a, float %b)
  ret float %r
}

; glibc sys/io.h and the system headers: privileged, not called.
define i32 @forced_ports(i16 %port, i32 %v) {
  %r = call i32 asm sideeffect "inb %dx, %al\0A\09inw %dx, %ax\0A\09inl %dx, %eax\0A\09outb %al, %dx\0A\09outw %ax, %dx\0A\09outl %eax, %dx\0A\09outb %al, $$0x80\0A\09inb $$0x60, %al", "={ax},{dx},0,~{dirflag},~{fpsr},~{flags}"(i16 %port, i32 %v)
  ret i32 %r
}

define void @forced_system() {
  call void asm sideeffect "rdmsr\0A\09wrmsr\0A\09enclu\0A\09encls\0A\09enclv\0A\09pconfig\0A\09hreset $$0\0A\09cli\0A\09sti\0A\09int3\0A\09nopl 64(%rax)", "~{rax},~{rcx},~{rdx},~{memory},~{dirflag},~{fpsr},~{flags}"()
  ret void
}
