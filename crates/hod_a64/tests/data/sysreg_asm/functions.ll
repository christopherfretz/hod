; WP-188 seeds: `mrs`/`msr` of named system registers and of the generic
; `S<op0>_<op1>_C<n>_C<m>_<op2>` form, as clang writes them for AArch64.
; `driver.c` checks every function against its own inline assembly.

define i64 @cntfrq() {
  %r = call i64 asm sideeffect "mrs $0, cntfrq_el0", "=r"()
  ret i64 %r
}

define i64 @cntfrq_generic() {
  %r = call i64 asm sideeffect "mrs $0, S3_3_C14_C0_0", "=r"()
  ret i64 %r
}

define i64 @cntvct() {
  %r = call i64 asm sideeffect "mrs $0, CNTVCT_EL0", "=r"()
  ret i64 %r
}

define ptr @tpidr() {
  %r = call ptr asm "mrs $0, tpidr_el0", "=r"()
  ret ptr %r
}

define i64 @midr() {
  %r = call i64 asm "mrs $0, MIDR_EL1", "=r"()
  ret i64 %r
}

define i64 @isar0() {
  %r = call i64 asm "mrs $0, id_aa64isar0_el1", "=r"()
  ret i64 %r
}

define i64 @pfr0() {
  %r = call i64 asm "mrs $0, id_aa64pfr0_el1", "=r"()
  ret i64 %r
}

define i64 @ctr() {
  %r = call i64 asm "mrs $0, ctr_el0", "=r"()
  ret i64 %r
}

define i64 @dczid() {
  %r = call i64 asm "mrs $0, dczid_el0", "=r"()
  ret i64 %r
}

; Writes `v` to FPCR, reads it back, restores the old value: returns what
; was read.
define i64 @fpcr_set(i64 %v) {
  %old = call i64 asm sideeffect "mrs $0, fpcr", "=r"()
  call void asm sideeffect "msr fpcr, $0", "r"(i64 %v)
  %r = call i64 asm sideeffect "mrs $0, FPCR", "=r"()
  call void asm sideeffect "msr S3_3_C4_C4_0, $0", "r"(i64 %old)
  ret i64 %r
}

; Sets NZCV to `v` and reads it back.
define i64 @nzcv_set(i64 %v) {
  %r = call i64 asm sideeffect "msr nzcv, $1\0Amrs $0, nzcv", "=r,r,~{cc}"(i64 %v)
  ret i64 %r
}

; Clears FPSR and reads it back.
define i64 @fpsr_clear() {
  %r = call i64 asm sideeffect "msr fpsr, xzr\0Amrs $0, fpsr", "=r"()
  ret i64 %r
}
