; spec/interp.md §10.1, FastISel's AArch64 miscompile, run on x86-64.
; Correct answer: 0 (exit status 0).
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
  ret i32 %r
}
