; WP-103 seeds: inline assembly with instruction templates, as clang
; writes them for AArch64. `driver.c` checks every function.

declare i64 @asm_helper(i64)

; `svc #0` with the syscall number in x8: getpid.
define i64 @sys_getpid() {
  %r = call i64 asm sideeffect "svc #0", "={x0},{x8},~{memory}"(i64 172)
  ret i64 %r
}

; A three-argument syscall wrapper: write(fd, buf, 0) returns 0.
define i64 @sys_write0(i64 %fd, ptr %buf) {
  %r = call i64 asm sideeffect "svc #0", "={x0},{x8},0,{x1},{x2},~{memory}"(i64 64, i64 %fd, ptr %buf, i64 0)
  ret i64 %r
}

define ptr @thread_pointer() {
  %r = call ptr asm "mrs $0, tpidr_el0", "=r"()
  ret ptr %r
}

define i64 @counter() {
  %r = call i64 asm sideeffect "mrs $0, cntvct_el0", "=r"()
  ret i64 %r
}

define i64 @fpcr_roundtrip() {
  %v = call i64 asm sideeffect "mrs $0, fpcr", "=r"()
  call void asm sideeffect "msr fpcr, $0", "r"(i64 %v)
  ret i64 %v
}

; A hand-written atomic add: ldxr/stxr loop with a numeric label.
define i32 @atomic_add(ptr %p, i32 %v) {
  %r = call { i32, i32 } asm sideeffect "1: ldxr ${0:w}, [$2]\0A add ${0:w}, ${0:w}, ${3:w}\0A stxr ${1:w}, ${0:w}, [$2]\0A cbnz ${1:w}, 1b", "=&r,=&r,r,r,~{memory}"(ptr %p, i32 %v)
  %n = extractvalue { i32, i32 } %r, 0
  ret i32 %n
}

define i64 @explicit_regs(i64 %x) {
  %r = call i64 asm "add x10, x9, #1", "={x10},{x9}"(i64 %x)
  ret i64 %r
}

define i32 @tied_w(i32 %x) {
  %r = call i32 asm "add ${0:w}, ${0:w}, #1", "=r,0"(i32 %x)
  ret i32 %r
}

; An i1 input tied to an i8 output.
define i8 @tied_i1(i1 %b) {
  %r = call i8 asm "", "=r,0"(i1 %b)
  ret i8 %r
}

define i32 @load_m(ptr %p) {
  %r = call i32 asm "ldr ${0:w}, $1", "=r,*m"(ptr elementtype(i32) %p)
  ret i32 %r
}

define void @store_m(ptr %p, i32 %v) {
  call void asm "str ${1:w}, $0", "=*m,r"(ptr elementtype(i32) %p, i32 %v)
  ret void
}

; `rm` with a value that is no constant: memory.
define i64 @load_rm(i64 %v) {
  %r = call i64 asm "ldr $0, $1", "=r,rm"(i64 %v)
  ret i64 %r
}

define i64 @clobbers(i64 %a, i64 %b) {
  %s = add i64 %a, 1000
  %r = call i64 asm "mov x9, #5\0A mov x10, #6\0A mov x11, #7\0A add $0, $1, x9", "=r,r,~{x9},~{x10},~{x11},~{x0},~{x1}"(i64 %a)
  %t = add i64 %r, %s
  %u = add i64 %t, %b
  ret i64 %u
}

define i64 @callee_saved(i64 %a) {
  %r = call i64 asm "mov x19, #77\0A mov x20, $1\0A add $0, x19, x20", "=r,r,~{x19},~{x20}"(i64 %a)
  ret i64 %r
}

define i64 @callee_saved_fp(double %a) {
  %r = call i64 asm "fmov d8, #2.0\0A fmul d8, d8, ${1:d}\0A fcvtzs $0, d8", "=r,w,~{v8}"(double %a)
  ret i64 %r
}

define i32 @flag_lt(i64 %a, i64 %b) {
  %r = call i32 asm "cmp $1, $2", "={@cclt},r,r"(i64 %a, i64 %b)
  ret i32 %r
}

define i8 @flag_eq(i32 %a, i32 %b) {
  %r = call i8 asm "cmp ${1:w}, ${2:w}", "={@cceq},r,r"(i32 %a, i32 %b)
  ret i8 %r
}

define double @fadd_w(double %a, double %b) {
  %r = call double asm "fadd ${0:d}, ${1:d}, ${2:d}", "=w,w,w"(double %a, double %b)
  ret double %r
}

define float @fneg_s(float %a) {
  %r = call float asm "fneg ${0:s}, ${1:s}", "=w,w"(float %a)
  ret float %r
}

define i64 @imm_i(i64 %x) {
  %r = call i64 asm "add $0, $1, #$2", "=r,r,i"(i64 %x, i64 7)
  ret i64 %r
}

define i64 @imm_n_neg(i64 %x) {
  %r = call i64 asm "sub $0, $1, #${2:n}", "=r,r,n"(i64 %x, i64 -3)
  ret i64 %r
}

define i64 @imm_zero_x(i64 %x) {
  %r = call i64 asm "add $0, $1, ${2:x}", "=r,r,r"(i64 %x, i64 5)
  %s = call i64 asm "orr $0, $1, ${2:x}", "=r,r,i"(i64 %r, i64 0)
  ret i64 %s
}

define { i64, i64 } @two_outputs(i64 %a, i64 %b) {
  %r = call { i64, i64 } asm "mov $0, $3\0A mov $1, $2", "=&r,=&r,r,r"(i64 %a, i64 %b)
  ret { i64, i64 } %r
}

define i64 @two_outputs_sum(i64 %a, i64 %b) {
  %r = call { i64, i64 } asm "sub $0, $2, $3\0A add $1, $2, $3", "=&r,=&r,r,r"(i64 %a, i64 %b)
  %x = extractvalue { i64, i64 } %r, 0
  %y = extractvalue { i64, i64 } %r, 1
  %m = mul i64 %x, %y
  ret i64 %m
}

define void @indirect_r(ptr %p) {
  call void asm "mov ${0:w}, #42", "=*r"(ptr elementtype(i32) %p)
  ret void
}

define i64 @call_helper(i64 %x) {
  %r = call i64 asm sideeffect "bl asm_helper", "={x0},{x0},~{x1},~{x2},~{x3},~{x4},~{x5},~{x6},~{x7},~{x8},~{x9},~{x10},~{x11},~{x12},~{x13},~{x14},~{x15},~{x16},~{x17},~{lr},~{v0},~{v1},~{v2},~{v3},~{v4},~{v5},~{v6},~{v7},~{v16},~{v17},~{v18},~{v19},~{v20},~{v21},~{v22},~{v23},~{v24},~{v25},~{v26},~{v27},~{v28},~{v29},~{v30},~{v31},~{memory},~{cc}"(i64 %x)
  ret i64 %r
}

define i64 @uid_variant(i64 %x) {
  %r = call i64 asm "$(mov $0, $1$|brk #0$) ${:comment} uid ${:uid}", "=r,r"(i64 %x)
  ret i64 %r
}

define i64 @local_forward(i64 %x) {
  %r = call i64 asm "mov $0, $1\0A cbz $0, 2f\0A add $0, $0, #1\0A2:", "=&r,r"(i64 %x)
  ret i64 %r
}

; Values live across the template in registers hod would choose.
define i64 @live_across(i64 %a, i64 %b, i64 %c) {
  %x = mul i64 %a, 3
  %y = mul i64 %b, 5
  %z = mul i64 %c, 7
  call void asm sideeffect "mov x9, xzr\0A mov x10, xzr\0A mov x11, xzr\0A mov x12, xzr\0A mov x0, xzr\0A mov x1, xzr\0A mov x2, xzr", "~{x9},~{x10},~{x11},~{x12},~{x0},~{x1},~{x2}"()
  %s = add i64 %x, %y
  %t = add i64 %s, %z
  ret i64 %t
}

define i32 @goto_zero(i64 %x) {
entry:
  callbr void asm "cbz $0, ${1:l}", "r,!i"(i64 %x) to label %ft [label %ind]
ft:
  ret i32 1
ind:
  ret i32 2
}

define i32 @goto_phi(i64 %x) {
entry:
  %a = add i64 %x, 10
  callbr void asm "cmp $0, #5\0A b.lt ${1:l}\0A cmp $0, #50\0A b.gt ${2:l}", "r,!i,!i"(i64 %a) to label %ft [label %lo, label %hi]
ft:
  br label %done
lo:
  br label %done
hi:
  br label %done
done:
  %r = phi i32 [ 0, %ft ], [ -1, %lo ], [ 1, %hi ]
  ret i32 %r
}

; WP-112: `callbr` with outputs, the result used on every edge out.
define i32 @goto_out(i32 %x) {
entry:
  %r = callbr i32 asm "add ${0:w}, ${0:w}, #3\0A cmp ${0:w}, #10\0A b.lt ${2:l}\0A cmp ${0:w}, #20\0A b.gt ${3:l}", "=r,0,!i,!i"(i32 %x) to label %ft [label %lo, label %hi]
ft:
  %f = add i32 %r, 1000
  ret i32 %f
lo:
  %l = add i32 %r, 2000
  ret i32 %l
hi:
  br label %done
done:
  %p = phi i32 [ %r, %hi ]
  %h = add i32 %p, 3000
  ret i32 %h
}

define i64 @goto_out2(i64 %x) {
entry:
  %s = callbr { i64, i64 } asm "mov $0, $2\0A add $1, $2, #1\0A cbz $2, ${3:l}", "=&r,=&r,r,!i"(i64 %x) to label %ft [label %z]
ft:
  %a = extractvalue { i64, i64 } %s, 0
  %b = extractvalue { i64, i64 } %s, 1
  %m = mul i64 %b, 10
  %r1 = add i64 %a, %m
  ret i64 %r1
z:
  %c = extractvalue { i64, i64 } %s, 1
  %r2 = add i64 %c, 500
  ret i64 %r2
}

define i32 @invoke_asm(i32 %x) personality ptr @hod_test_personality {
entry:
  %r = invoke i32 asm "add ${0:w}, ${1:w}, #2", "=r,r"(i32 %x) to label %ok unwind label %lp
ok:
  ret i32 %r
lp:
  %l = landingpad { ptr, i32 } cleanup
  ret i32 -1
}
declare i32 @hod_test_personality(...)

; WP-216 (`spec/asm.md` §16): RocksDB's `PRFM PLDL1KEEP, [${0:x},$1]`
; with `r,I`, then the register-offset, extended-index, unscaled and
; literal forms, upper and lower case; the prefetches change nothing, the
; loads after them read the array. `xpaclri` strips nothing from a plain
; value in x30 (`HINT #7`); the template keeps the return address in x9
; itself (`llc` saves no `lr` for a clobber of x30 in a leaf).
define i64 @prefetch_sum(ptr %p, i64 %i, i32 %j) {
  call void asm sideeffect "PRFM PLDL1KEEP, [${0:x},$1]", "r,I"(ptr %p, i32 1024)
  call void asm sideeffect "prfm pstl2strm, [$0, $1, lsl #3]\0A prfm plil3keep, [$0, ${2:w}, sxtw #3]\0A prfm #5, [$0, ${2:w}, uxtw]", "r,r,r"(ptr %p, i64 %i, i32 %j)
  call void asm sideeffect "prfum pldl2keep, [$0, #-8]\0A prfm pldl3strm, [$0, #32760]\0A 1: prfm pldl1strm, 1b\0A prfm pstl1keep, 2f\0A 2:", "r"(ptr %p)
  %a = load i64, ptr %p
  %q = getelementptr i64, ptr %p, i64 %i
  %b = load i64, ptr %q
  %s = add i64 %a, %b
  ret i64 %s
}

define i64 @xpaclri_lr(i64 %x) {
  %r = call i64 asm sideeffect "mov x9, x30\0A mov x30, $1\0A xpaclri\0A mov $0, x30\0A mov x30, x9", "=r,r,~{x9}"(i64 %x)
  ret i64 %r
}

; spec/asm.md §17 (WP-217): alignment directives in templates. Each
; `adr` takes the address of the instruction after a directive; the low
; bits are zero when the boundary is counted from the section (and the
; function and section are aligned enough), after linking.
define i64 @align_probe() {
  %a = call i64 asm sideeffect "nop\0A.p2align 5\0A1: adr $0, 1b", "=r"()
  %b = call i64 asm sideeffect "nop\0Anop\0A.balign 64\0A1: adr $0, 1b", "=r"()
  %c = call i64 asm sideeffect ".align 4\0A1: adr $0, 1b", "=r"()
  %d = call i64 asm sideeffect "b 1f\0A.P2ALIGN 3, 0xff\0A1: adr $0, 1b\0A.p2align 1\0A.balign 2", "=r"()
  %am = and i64 %a, 31
  %bm = and i64 %b, 63
  %cm = and i64 %c, 15
  %dm = and i64 %d, 7
  %ab = or i64 %am, %bm
  %cd = or i64 %cm, %dm
  %r = or i64 %ab, %cd
  ret i64 %r
}

; A branch over fill words and a maximum the padding passes.
define i64 @align_fill(i64 %x) {
  %r = call i64 asm "nop\0Ab 1f\0A.balign 32, 0xff\0A1: add $0, $1, #1\0A.p2align 6,,8\0Aadd $0, $0, #1", "=r,r"(i64 %x)
  ret i64 %r
}

; zstd's loop alignment (`zstd_decompress_block.c`, `zstd_lazy.c`: one
; directive or `nop` per statement), in front of a loop and in it.
define i64 @zstd_loop(i64 %n) {
entry:
  call void asm sideeffect ".p2align 6", ""()
  call void asm sideeffect "nop", ""()
  call void asm sideeffect ".p2align 5", ""()
  call void asm sideeffect "nop", ""()
  call void asm sideeffect ".p2align 4", ""()
  br label %loop
loop:
  %i = phi i64 [ 0, %entry ], [ %i1, %loop ]
  %s = phi i64 [ 0, %entry ], [ %s1, %loop ]
  call void asm sideeffect ".p2align 3", ""()
  %s1 = add i64 %s, %i
  %i1 = add i64 %i, 1
  %more = icmp slt i64 %i1, %n
  br i1 %more, label %loop, label %exit
exit:
  ret i64 %s1
}

; upb's `encode_longvarint` (`upb/wire/encode.c`, without BTI): a
; computed branch into a table of 8-byte entries placed after
; `.p2align 3`.
define void @upb_longvarint(ptr %p, i64 %val, i64 %skip) {
  %r = call { i64, i64 } asm sideeffect "adr $0, 0f\0Aadd $0, $0, $4, lsl #3\0Amov ${1:w}, #0x80\0Abr $0\0A.p2align 3\0A0:\0Aorr $0, $1, $2, lsr #56\0Astrb ${0:w}, [$3, #8]\0Aorr $0, $1, $2, lsr #49\0Astrb ${0:w}, [$3, #7]\0Aorr $0, $1, $2, lsr #42\0Astrb ${0:w}, [$3, #6]\0Aorr $0, $1, $2, lsr #35\0Astrb ${0:w}, [$3, #5]\0Aorr $0, $1, $2, lsr #28\0Astrb ${0:w}, [$3, #4]\0Aorr ${0:w}, ${1:w}, ${2:w}, lsr #21\0Astrb ${0:w}, [$3, #3]\0Aorr ${0:w}, ${1:w}, ${2:w}, lsr #14\0Astrb ${0:w}, [$3, #2]\0Aorr ${0:w}, ${1:w}, ${2:w}, lsr #7\0Astrb ${0:w}, [$3, #1]\0Aorr ${0:w}, ${2:w}, #0x80\0Astrb ${0:w}, [$3]\0A", "=&r,=&r,r,r,r,~{memory}"(i64 %val, ptr %p, i64 %skip)
  ret void
}

; WP-226 (`spec/asm.md` §20): the NEON templates of the census.
; Eigen's `fmla` by element, the accumulator `+w` (`GeneralBlockPanelKernel.h`).
define <4 x float> @eigen_fmla0(<4 x float> %c, <4 x float> %a, <4 x float> %b) {
  %r = call <4 x float> asm "fmla $0.4s, $1.4s, $2.s[0]\0A", "=w,w,w,0"(<4 x float> %a, <4 x float> %b, <4 x float> %c)
  ret <4 x float> %r
}

define <4 x float> @eigen_fmla3(<4 x float> %c, <4 x float> %a, <4 x float> %b) {
  %r = call <4 x float> asm "fmla $0.4s, $1.4s, $2.s[3]\0A", "=w,w,w,0"(<4 x float> %a, <4 x float> %b, <4 x float> %c)
  ret <4 x float> %r
}

; highway's conversions and compares (`arm_neon-inl.h`).
define <4 x i32> @hwy_fcvtzs(<4 x float> %v) {
  %r = call <4 x i32> asm "fcvtzs $0.4s, $1.4s", "=w,w"(<4 x float> %v)
  ret <4 x i32> %r
}

define <4 x i32> @hwy_fcvtzu(<4 x float> %v) {
  %r = call <4 x i32> asm "fcvtzu $0.4s, $1.4s", "=w,w"(<4 x float> %v)
  ret <4 x i32> %r
}

define <2 x i64> @hwy_fcvtzs2d(<2 x double> %v) {
  %r = call <2 x i64> asm "fcvtzs $0.2d, $1.2d", "=w,w"(<2 x double> %v)
  ret <2 x i64> %r
}

define i64 @hwy_fcvtzs_d(double %v) {
  %r = call i64 asm "fcvtzs ${0:d}, ${1:d}", "=w,w"(double %v)
  ret i64 %r
}

define i64 @hwy_fcvtzu_d(double %v) {
  %r = call i64 asm "fcvtzu ${0:d}, ${1:d}", "=w,w"(double %v)
  ret i64 %r
}

define i64 @hwy_fcmeq_d(double %v) {
  %r = call i64 asm "fcmeq ${0:d}, ${1:d}, ${1:d}", "=w,w"(double %v)
  ret i64 %r
}

define <4 x i32> @hwy_fcmeq4s(<4 x float> %v) {
  %r = call <4 x i32> asm "fcmeq $0.4s, $1.4s, $1.4s", "=w,w"(<4 x float> %v)
  ret <4 x i32> %r
}

define <8 x i16> @hwy_fcmeq8h(<8 x half> %v) #1 {
  %r = call <8 x i16> asm "fcmeq $0.8h, $1.8h, $1.8h", "=w,w"(<8 x half> %v)
  ret <8 x i16> %r
}

define <8 x i16> @hwy_fcvtzs8h(<8 x half> %v) #1 {
  %r = call <8 x i16> asm "fcvtzs $0.8h, $1.8h", "=w,w"(<8 x half> %v)
  ret <8 x i16> %r
}

; OpenCV's rounding conversion (`intrin_neon.hpp`).
define <4 x i32> @cv_fcvtns(<4 x float> %v) {
  %r = call <4 x i32> asm "fcvtns $0.4s, $1.4s", "=w,w"(<4 x float> %v)
  ret <4 x i32> %r
}

; xxhash's `umlal` (`XXH3_scalarRound` NEON), the accumulator `+w`.
define <2 x i64> @xxh_umlal(<2 x i64> %acc, <2 x i32> %a, <2 x i32> %b) {
  %r = call <2 x i64> asm "umlal $0.2d, $1.2s, $2.2s", "=w,w,w,0"(<2 x i32> %a, <2 x i32> %b, <2 x i64> %acc)
  ret <2 x i64> %r
}

; abseil's carry-less multiplies (`crc32_x86_arm_combined_simd.h`): `aes`.
define <2 x i64> @absl_pmull(<2 x i64> %a, <2 x i64> %b) #0 {
  %r = call <2 x i64> asm "pmull $0.1q, $1.1d, $2.1d", "=w,w,w"(<2 x i64> %a, <2 x i64> %b)
  ret <2 x i64> %r
}

define <2 x i64> @absl_pmull2(<2 x i64> %a, <2 x i64> %b) #0 {
  %r = call <2 x i64> asm "pmull2 $0.1q, $1.2d, $2.2d", "=w,w,w"(<2 x i64> %a, <2 x i64> %b)
  ret <2 x i64> %r
}

; `w` and `x` print the view of the value's size; `x` takes v0..v15.
define float @mod_wx(float %a, double %b) {
  %r = call float asm "fcvt ${0:w}, ${2:x}\0A fadd ${0:x}, ${0:w}, ${1:w}", "=&w,w,w"(float %a, double %b)
  ret float %r
}

define <4 x float> @x_constraint(<4 x float> %a, <4 x float> %b) {
  %r = call <4 x float> asm "fadd $0.4s, $1.4s, $2.4s", "=x,x,x"(<4 x float> %a, <4 x float> %b)
  ret <4 x float> %r
}

; Structure loads and stores, post-index.
define ptr @ld1_st1(ptr %dst, ptr %src) {
  %r = call ptr asm sideeffect "ld1 {v16.4s, v17.4s}, [$0], #32\0A fadd v16.4s, v16.4s, v17.4s\0A st1 {v16.4s}, [$2]", "=r,0,r,~{v16},~{v17},~{memory}"(ptr %src, ptr %dst)
  ret ptr %r
}

; Every register of v8..v15 written whole, clobbered by its several
; names: the low halves the caller keeps survive.
define double @clobber_v8_v15(ptr %a, ptr %b) {
  %r = call double asm sideeffect "ld1 {v8.16b-v11.16b}, [$1]\0A ld1 {v12.16b-v15.16b}, [$2]\0A fadd ${0:d}, d8, d15", "=w,r,r,~{v8},~{q9},~{d10},~{s11},~{v12},~{v13},~{v14},~{v15},~{memory}"(ptr %a, ptr %b)
  ret double %r
}

attributes #0 = { "target-features"="+aes,+fp-armv8,+neon" }
attributes #1 = { "target-features"="+fp-armv8,+fullfp16,+neon" }
