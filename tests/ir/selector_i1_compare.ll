; An unsigned compare of two i1 values, one of them the low bit of an i7
; argument whose other bits are set: 26 is 0b0011010, its low bit is 0, and
; `0 <u 0` is false. FastISel for AArch64 (`llc -O0 -global-isel=0`, LLVM 19
; and 22) compares the whole register and takes the branch: this is the
; second reproducer of spec/interp.md §10.1, and one of the two reasons
; the oracle is `llc -O0` with SelectionDAG (spec/harness.md "irdiff",
; "The oracle").
;@ covers: spec/ir.md §5.4 compare, cast (ICmp at i1 on a Trunc)
;@ unsupported: codegen-not-implemented
;@ stdout: 0

@.fmt = private unnamed_addr constant [4 x i8] c"%d\0A\00"

declare i32 @printf(ptr, ...)

define i32 @f(i7 %a) noinline {
entry:
  %t = trunc i7 %a to i1
  %s = shl i1 %t, 0
  %c = icmp ult i1 0, %s
  br i1 %c, label %yes, label %no
yes:
  ret i32 1
no:
  ret i32 0
}

define i32 @main() {
  %r = call i32 @f(i7 26)
  call i32 (ptr, ...) @printf(ptr @.fmt, i32 %r)
  ret i32 0
}
