; Inline assembly with templates on x86-64 (WP-187). Functions named
; `forced_*` place every operand in a register the constraints name, so
; the bytes of their templates are `llc`'s too; the others are checked
; by `driver.c` by what they compute.

; Host.cpp's `getX86CpuIDAndInfo`: `rbx` saved in `rsi` around `cpuid`.
define void @forced_cpuid_si(i32 %leaf, ptr %out) {
  %r = call { i32, i32, i32, i32 } asm "movq\09%rbx, %rsi\0A\09cpuid\0A\09xchgq\09%rbx, %rsi\0A", "={ax},={si},={cx},={dx},{ax},{cx},~{dirflag},~{fpsr},~{flags}"(i32 %leaf, i32 0)
  %a = extractvalue { i32, i32, i32, i32 } %r, 0
  %b = extractvalue { i32, i32, i32, i32 } %r, 1
  %c = extractvalue { i32, i32, i32, i32 } %r, 2
  %d = extractvalue { i32, i32, i32, i32 } %r, 3
  store i32 %a, ptr %out
  %p1 = getelementptr i32, ptr %out, i64 1
  store i32 %b, ptr %p1
  %p2 = getelementptr i32, ptr %out, i64 2
  store i32 %c, ptr %p2
  %p3 = getelementptr i32, ptr %out, i64 3
  store i32 %d, ptr %p3
  ret void
}

; `cpuid` writing `rbx` itself: the prologue saves it.
define void @forced_cpuid_bx(i32 %leaf, i32 %sub, ptr %out) {
  %r = call { i32, i32, i32, i32 } asm "cpuid", "={ax},={bx},={cx},={dx},{ax},{cx},~{dirflag},~{fpsr},~{flags}"(i32 %leaf, i32 %sub)
  %a = extractvalue { i32, i32, i32, i32 } %r, 0
  %b = extractvalue { i32, i32, i32, i32 } %r, 1
  %c = extractvalue { i32, i32, i32, i32 } %r, 2
  %d = extractvalue { i32, i32, i32, i32 } %r, 3
  store i32 %a, ptr %out
  %p1 = getelementptr i32, ptr %out, i64 1
  store i32 %b, ptr %p1
  %p2 = getelementptr i32, ptr %out, i64 2
  store i32 %c, ptr %p2
  %p3 = getelementptr i32, ptr %out, i64 3
  store i32 %d, ptr %p3
  ret void
}

; `<cpuid.h>`: `xchg` of `rbx` with an `r` operand, the dialect groups.
define void @cpuid_h(i32 %leaf, ptr %out) {
  %r = call { i32, i32, i32, i32 } asm "  xchg$(q$|$)  $(%$|$)rbx,${1:q}\0A cpuid\0A xchg$(q$|$)  $(%$|$)rbx,${1:q}", "={ax},=r,={cx},={dx},0,2,~{dirflag},~{fpsr},~{flags}"(i32 %leaf, i32 0)
  %a = extractvalue { i32, i32, i32, i32 } %r, 0
  %b = extractvalue { i32, i32, i32, i32 } %r, 1
  %c = extractvalue { i32, i32, i32, i32 } %r, 2
  %d = extractvalue { i32, i32, i32, i32 } %r, 3
  store i32 %a, ptr %out
  %p1 = getelementptr i32, ptr %out, i64 1
  store i32 %b, ptr %p1
  %p2 = getelementptr i32, ptr %out, i64 2
  store i32 %c, ptr %p2
  %p3 = getelementptr i32, ptr %out, i64 3
  store i32 %d, ptr %p3
  ret void
}

; `xgetbv` as bytes (blake3_dispatch.c, Host.cpp).
define i64 @forced_xgetbv_bytes() {
  %r = call { i32, i32 } asm ".byte 0x0f, 0x01, 0xd0", "={ax},={dx},{cx},~{dirflag},~{fpsr},~{flags}"(i32 0)
  %lo = extractvalue { i32, i32 } %r, 0
  %hi = extractvalue { i32, i32 } %r, 1
  %l = zext i32 %lo to i64
  %h = zext i32 %hi to i64
  %s = shl i64 %h, 32
  %v = or i64 %s, %l
  ret i64 %v
}

define i64 @forced_xgetbv() {
  %r = call { i32, i32 } asm "xgetbv\0A", "={ax},={dx},{cx},~{dirflag},~{fpsr},~{flags}"(i32 0)
  %lo = extractvalue { i32, i32 } %r, 0
  %hi = extractvalue { i32, i32 } %r, 1
  %l = zext i32 %lo to i64
  %h = zext i32 %hi to i64
  %s = shl i64 %h, 32
  %v = or i64 %s, %l
  ret i64 %v
}

; Valgrind's client request: `;` statements, `$$`, a tied operand.
define i64 @forced_valgrind(ptr %args, i64 %default) {
  %r = call i64 asm sideeffect "rolq $$3,  %rdi ; rolq $$13, %rdi\0A\09rolq $$61, %rdi ; rolq $$51, %rdi\0A\09xchgq %rbx,%rbx", "={dx},{ax},0,~{cc},~{memory},~{dirflag},~{fpsr},~{flags}"(ptr %args, i64 %default)
  ret i64 %r
}

define i64 @forced_rdtsc() {
  %r = call { i32, i32 } asm sideeffect "rdtsc", "={ax},={dx},~{dirflag},~{fpsr},~{flags}"()
  %lo = extractvalue { i32, i32 } %r, 0
  %hi = extractvalue { i32, i32 } %r, 1
  %l = zext i32 %lo to i64
  %h = zext i32 %hi to i64
  %s = shl i64 %h, 32
  %v = or i64 %s, %l
  ret i64 %v
}

define void @forced_pause() {
  call void asm sideeffect "pause", "~{dirflag},~{fpsr},~{flags}"()
  call void asm sideeffect "rep; nop", "~{memory},~{dirflag},~{fpsr},~{flags}"()
  call void asm sideeffect "mfence\0A\09lfence\0A\09sfence", "~{memory},~{dirflag},~{fpsr},~{flags}"()
  ret void
}

; `getpid` by `syscall`.
define i64 @forced_syscall_getpid() {
  %r = call i64 asm sideeffect "syscall", "={ax},{ax},~{rcx},~{r11},~{memory},~{dirflag},~{fpsr},~{flags}"(i64 39)
  ret i64 %r
}

; `rep movsb` with the three registers tied.
define ptr @forced_rep_movsb(ptr %dst, ptr %src, i64 %n) {
  %r = call { ptr, ptr, i64 } asm sideeffect "rep movsb", "={di},={si},={cx},0,1,2,~{memory},~{dirflag},~{fpsr},~{flags}"(ptr %dst, ptr %src, i64 %n)
  %d = extractvalue { ptr, ptr, i64 } %r, 0
  ret ptr %d
}

define void @forced_rep_stosb(ptr %dst, i8 %c, i64 %n) {
  %r = call { ptr, i64 } asm sideeffect "rep stosb", "={di},={cx},{ax},0,1,~{memory},~{dirflag},~{fpsr},~{flags}"(i8 %c, ptr %dst, i64 %n)
  ret void
}

; `mulq` with its fixed registers and an `rm` operand.
define i64 @mulhi(i64 %a, i64 %b) {
  %r = call { i64, i64 } asm "mulq $3", "={ax},={dx},{ax},rm,~{cc},~{dirflag},~{fpsr},~{flags}"(i64 %a, i64 %b)
  %h = extractvalue { i64, i64 } %r, 1
  ret i64 %h
}

; The atomics of the headers.
define i32 @lock_xadd(ptr %p, i32 %v) {
  %r = call i32 asm sideeffect "lock; xaddl $0, $1", "=r,=*m,0,*m,~{memory},~{dirflag},~{fpsr},~{flags}"(ptr elementtype(i32) %p, i32 %v, ptr elementtype(i32) %p)
  ret i32 %r
}

define i32 @lock_cmpxchg(ptr %p, i32 %old, i32 %new) {
  %r = call i32 asm sideeffect "lock; cmpxchgl $2, $1", "={ax},=*m,r,0,*m,~{memory},~{dirflag},~{fpsr},~{flags}"(ptr elementtype(i32) %p, i32 %new, i32 %old, ptr elementtype(i32) %p)
  ret i32 %r
}

define void @lock_inc(ptr %p) {
  call void asm sideeffect "lock; incl $0", "=*m,*m,~{memory},~{dirflag},~{fpsr},~{flags}"(ptr elementtype(i32) %p, ptr elementtype(i32) %p)
  ret void
}

define void @lock_or(ptr %p, i32 %v) {
  call void asm sideeffect "lock; orl $1, $0", "=*m,ir,*m,~{memory},~{dirflag},~{fpsr},~{flags}"(ptr elementtype(i32) %p, i32 %v, ptr elementtype(i32) %p)
  ret void
}

define void @lock_or_imm(ptr %p) {
  call void asm sideeffect "lock; orl $1, $0", "=*m,ir,*m,~{memory},~{dirflag},~{fpsr},~{flags}"(ptr elementtype(i32) %p, i32 256, ptr elementtype(i32) %p)
  ret void
}

define i64 @xchg(ptr %p, i64 %v) {
  %r = call i64 asm sideeffect "xchgq $0, $1", "=r,=*m,0,*m,~{memory},~{dirflag},~{fpsr},~{flags}"(ptr elementtype(i64) %p, i64 %v, ptr elementtype(i64) %p)
  ret i64 %r
}

define i32 @bswap32(i32 %x) {
  %r = call i32 asm "bswap $0", "=r,0,~{dirflag},~{fpsr},~{flags}"(i32 %x)
  ret i32 %r
}

define i64 @bswap64(i64 %x) {
  %r = call i64 asm "bswapq $0", "=r,0,~{dirflag},~{fpsr},~{flags}"(i64 %x)
  ret i64 %r
}

define i32 @bsf(i32 %x) {
  %r = call i32 asm "bsfl $1, $0", "=r,rm,~{dirflag},~{fpsr},~{flags}"(i32 %x)
  ret i32 %r
}

define i64 @bsr64(i64 %x) {
  %r = call i64 asm "bsrq $1, $0", "=r,r,~{dirflag},~{fpsr},~{flags}"(i64 %x)
  ret i64 %r
}

; Tied and early-clobber operands.
define i32 @tied_add(i32 %a, i32 %b) {
  %r = call i32 asm "addl $2, $0", "=r,0,r,~{dirflag},~{fpsr},~{flags}"(i32 %a, i32 %b)
  ret i32 %r
}

define i32 @plus_add(i32 %a, i32 %b) {
  %r = call i32 asm "addl $$1, $0\0A\09addl $2, $0", "=r,0,r,~{dirflag},~{fpsr},~{flags}"(i32 %a, i32 %b)
  ret i32 %r
}

define i32 @early(i32 %a, i32 %b) {
  %r = call i32 asm "movl $1, $0\0A\09subl $2, $0", "=&r,r,r,~{dirflag},~{fpsr},~{flags}"(i32 %a, i32 %b)
  ret i32 %r
}

define i64 @two_out(i64 %a, i64 %b) {
  %r = call { i64, i64 } asm "movq $2, $0\0A\09movq $3, $1\0A\09addq $1, $0\0A\09subq $2, $1", "=&r,=&r,r,r,~{dirflag},~{fpsr},~{flags}"(i64 %a, i64 %b)
  %x = extractvalue { i64, i64 } %r, 0
  %y = extractvalue { i64, i64 } %r, 1
  %m = mul i64 %x, 1000
  %s = add i64 %m, %y
  ret i64 %s
}

define i64 @swap_tied(i64 %a, i64 %b) {
  %r = call { i64, i64 } asm "xchgq $0, $1", "=r,=r,0,1,~{dirflag},~{fpsr},~{flags}"(i64 %a, i64 %b)
  %x = extractvalue { i64, i64 } %r, 0
  %y = extractvalue { i64, i64 } %r, 1
  %m = mul i64 %x, 1000
  %s = add i64 %m, %y
  ret i64 %s
}

define i32 @lea3(i32 %a, i32 %b) {
  %r = call i32 asm "leal ($1,$2,4), $0", "=r,r,r,~{dirflag},~{fpsr},~{flags}"(i64 0, i64 0)
  %a64 = zext i32 %a to i64
  %b64 = zext i32 %b to i64
  %q = call i32 asm "leal ($1,$2,4), $0", "=r,r,r,~{dirflag},~{fpsr},~{flags}"(i64 %a64, i64 %b64)
  %s = add i32 %q, %r
  ret i32 %s
}

define i8 @commutative(i8 %a, i8 %b) {
  %r = call i8 asm "addb $2, $0", "=q,%0,q,~{dirflag},~{fpsr},~{flags}"(i8 %a, i8 %b)
  ret i8 %r
}

define i16 @narrow(i16 %a, i16 %b) {
  %r = call i16 asm "imulw $2, $0", "=r,0,r,~{dirflag},~{fpsr},~{flags}"(i16 %a, i16 %b)
  ret i16 %r
}

; Modifiers.
define i32 @low_byte(i32 %x) {
  %r = call i32 asm "movzbl ${1:b}, ${0:k}", "=q,q,~{dirflag},~{fpsr},~{flags}"(i32 %x)
  ret i32 %r
}

define i64 @widen(i32 %x) {
  %r = call i64 asm "movslq ${1:k}, ${0:q}", "=r,r,~{dirflag},~{fpsr},~{flags}"(i32 %x)
  ret i64 %r
}

define i32 @word(i32 %x) {
  %r = call i32 asm "movzwl ${1:w}, $0", "=r,r,~{dirflag},~{fpsr},~{flags}"(i32 %x)
  ret i32 %r
}

define i32 @imm_mods(i32 %x) {
  %r = call i32 asm "addl $$${2:c}, $0\0A\09subl $$${3:n}, $0\0A\09addl $4, $0", "=r,0,i,i,n,~{dirflag},~{fpsr},~{flags}"(i32 %x, i32 100, i32 5, i32 1000)
  ret i32 %r
}

define i32 @ranged(i32 %x) {
  %r = call i32 asm "roll $2, $0\0A\09addl $3, $0", "=r,0,I,e,~{dirflag},~{fpsr},~{flags}"(i32 %x, i32 4, i32 -16)
  ret i32 %r
}

; Flag outputs and `setcc` with a `q` operand.
define i8 @less(i32 %a, i32 %b) {
  %r = call i8 asm "cmpl $2, $1", "={@ccl},r,r,~{dirflag},~{fpsr},~{flags}"(i32 %a, i32 %b)
  ret i8 %r
}

define i32 @equal(i32 %a, i32 %b) {
  %r = call { i32, i8 } asm "cmpl $3, $2\0A\09sete ${1:b}", "=r,=q,r,r,0,~{dirflag},~{fpsr},~{flags}"(i32 %a, i32 %b, i32 7)
  %e = extractvalue { i32, i8 } %r, 1
  %z = zext i8 %e to i32
  ret i32 %z
}

define i32 @bit_test(i32 %x, i32 %n) {
  %r = call i8 asm "btl $2, $1", "={@ccc},r,Ir,~{dirflag},~{fpsr},~{flags}"(i32 %x, i32 %n)
  %z = zext i8 %r to i32
  ret i32 %z
}

; Local labels: a loop and a forward skip.
define i32 @loop_sum(i32 %n) {
  %r = call { i32, i32 } asm "xorl $0, $0\0A1:\0A\09addl $1, $0\0A\09decl $1\0A\09jnz 1b", "=&r,=r,1,~{cc},~{dirflag},~{fpsr},~{flags}"(i32 %n)
  %s = extractvalue { i32, i32 } %r, 0
  ret i32 %s
}

define i32 @skip(i32 %x) {
  %r = call i32 asm "testl $0, $0\0A\09jz 1f\0A\09addl $$10, $0\0A1:", "=r,0,~{cc},~{dirflag},~{fpsr},~{flags}"(i32 %x)
  ret i32 %r
}

; SSE operands.
define double @sqrt_sd(double %x) {
  %r = call double asm "sqrtsd $1, $0", "=x,x,~{dirflag},~{fpsr},~{flags}"(double %x)
  ret double %r
}

define float @addss(float %a, float %b) {
  %r = call float asm "addss $2, $0", "=x,0,x,~{dirflag},~{fpsr},~{flags}"(float %a, float %b)
  ret float %r
}

; Memory operands: an input in memory, an indirect input, an `%fs:` read.
define i64 @load_m(ptr %p) {
  %r = call i64 asm "movq $1, $0", "=r,*m,~{dirflag},~{fpsr},~{flags}"(ptr elementtype(i64) %p)
  ret i64 %r
}

define void @store_m(ptr %p, i64 %v) {
  call void asm "movq $1, $0", "=*m,r,~{memory},~{dirflag},~{fpsr},~{flags}"(ptr elementtype(i64) %p, i64 %v)
  ret void
}

define i64 @value_m(i64 %v) {
  %r = call i64 asm "movq $1, $0\0A\09addq $1, $0", "=r,m,~{dirflag},~{fpsr},~{flags}"(i64 %v)
  ret i64 %r
}

define ptr @thread_pointer() {
  %r = call ptr asm "movq %fs:0, $0", "=r,~{dirflag},~{fpsr},~{flags}"()
  ret ptr %r
}

; `pushfq; popq`: the flags word; push and pop inside a template.
define i64 @flags_word() {
  %r = call i64 asm sideeffect "pushfq\0A\09popq $0", "=r,~{memory},~{dirflag},~{fpsr},~{flags}"()
  ret i64 %r
}

; Every register clobbered with values live across.
define i64 @clobber_live(i64 %a, i64 %b, i64 %c, i64 %d) {
  %x = mul i64 %a, 3
  %y = add i64 %b, 7
  %z = sub i64 %c, %d
  call void asm sideeffect "movq $$-1, %rax\0A\09movq $$-1, %rcx\0A\09movq $$-1, %rdx\0A\09movq $$-1, %rsi\0A\09movq $$-1, %rdi\0A\09movq $$-1, %r8\0A\09movq $$-1, %r9\0A\09movq $$-1, %r10\0A\09movq $$-1, %r11\0A\09movq $$-1, %rbx\0A\09movq $$-1, %r12\0A\09movq $$-1, %r13\0A\09movq $$-1, %r14\0A\09movq $$-1, %r15", "~{rax},~{rcx},~{rdx},~{rsi},~{rdi},~{r8},~{r9},~{r10},~{r11},~{rbx},~{r12},~{r13},~{r14},~{r15},~{dirflag},~{fpsr},~{flags}"()
  %s1 = add i64 %x, %y
  %s2 = add i64 %s1, %z
  %s3 = add i64 %s2, %a
  ret i64 %s3
}

define double @clobber_xmm(double %a, double %b) {
  %x = fmul double %a, %b
  call void asm sideeffect "xorps %xmm0, %xmm0\0A\09xorps %xmm1, %xmm1\0A\09xorpd %xmm8, %xmm8\0A\09xorps %xmm15, %xmm15", "~{xmm0},~{xmm1},~{xmm8},~{xmm15},~{dirflag},~{fpsr},~{flags}"()
  %s = fadd double %x, %a
  ret double %s
}

; A tied operand whose register is checked: `0` must be in `$0`'s place.
define i32 @tied_order(i32 %a, i32 %b, i32 %c) {
  %r = call i32 asm "subl $2, $0\0A\09imull $3, $0", "=r,0,r,r,~{dirflag},~{fpsr},~{flags}"(i32 %a, i32 %b, i32 %c)
  ret i32 %r
}

; Symbol fields (the census: llvm-rtdyld's `leaq sym@tpoff, $0`, and
; llvm-exegesis's `fwait`): relocations written from the template.
@asm_tls = thread_local(initialexec) global i64 42, align 8
@asm_cell = global i64 1234, align 8

define i64 @asm_fn() {
  ret i64 77
}

define i64 @forced_sym_tpoff() {
  %r = call i64 asm "leaq asm_tls@tpoff, $0", "={rax},~{dirflag},~{fpsr},~{flags}"()
  ret i64 %r
}

define i64 @forced_sym_fs_load() {
  %r = call i64 asm "movq %fs:asm_tls@tpoff, $0", "={rcx},~{dirflag},~{fpsr},~{flags}"()
  ret i64 %r
}

define i64 @forced_sym_gottpoff() {
  %r = call i64 asm "movq asm_tls@gottpoff(%rip), $0", "={rdx},~{dirflag},~{fpsr},~{flags}"()
  ret i64 %r
}

define i64 @sym_tpoff_r() {
  %r = call i64 asm "leaq asm_tls@tpoff, $0", "=r,~{dirflag},~{fpsr},~{flags}"()
  ret i64 %r
}

define ptr @tls_address() {
  %r = call ptr asm "movq %fs:0, $0\0A\09leaq asm_tls@tpoff($0), $0", "=r,~{dirflag},~{fpsr},~{flags}"()
  ret ptr %r
}

define ptr @forced_sym_pc32() {
  %r = call ptr asm "leaq asm_cell(%rip), $0", "={rax},~{dirflag},~{fpsr},~{flags}"()
  ret ptr %r
}

define i64 @forced_sym_pc32_load() {
  %r = call i64 asm "movq asm_cell(%rip), $0", "={rdi},~{dirflag},~{fpsr},~{flags}"()
  ret i64 %r
}

define ptr @forced_sym_gotpcrel() {
  %r = call ptr asm "movq asm_cell@GOTPCREL(%rip), $0", "={rsi},~{dirflag},~{fpsr},~{flags}"()
  ret ptr %r
}

define ptr @forced_sym_plt() {
  %r = call ptr asm "leaq asm_fn@PLT(%rip), $0", "={r8},~{dirflag},~{fpsr},~{flags}"()
  ret ptr %r
}

define void @forced_fwait() {
  call void asm sideeffect "fwait", "~{dirflag},~{fpsr},~{flags}"()
  call void asm sideeffect "wait", "~{dirflag},~{fpsr},~{flags}"()
  ret void
}
