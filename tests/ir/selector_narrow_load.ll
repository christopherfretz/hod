; A zero-extended load of an i16 whose result is later sign-extended from
; i32: the i16 0xEAE0 is 60128 as an i32, and stays 60128 at every width.
; `llc -O0` with its default selector for AArch64 (GlobalISel, LLVM 19 and
; 22) folds the extensions into a sign-extending load and prints -5408:
; this is the first reproducer of spec/interp.md §10.1, and one of the two
; reasons the oracle is `llc -O0` with SelectionDAG (spec/harness.md
; "irdiff", "The oracle").
;@ covers: spec/ir.md §5.4 cast, memory (ZExt and SExt of a narrow Load)
;@ stdout: 60128

@g = global i16 -5408
@.fmt = private unnamed_addr constant [6 x i8] c"%lld\0A\00"

declare i32 @printf(ptr, ...)

define i64 @f() noinline {
  %a = load i16, ptr @g
  %b = zext i16 %a to i32
  %c = zext i32 %b to i33
  %d = sext i32 %b to i64
  %e = zext i33 %c to i64
  %r = or i64 %d, %e
  ret i64 %r
}

define i32 @main() {
  %r = call i64 @f()
  call i32 (ptr, ...) @printf(ptr @.fmt, i64 %r)
  ret i32 0
}
