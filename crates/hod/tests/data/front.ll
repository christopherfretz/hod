; The facade's fixture (crates/hod/tests/front.rs): one module with a
; reason in the module pass, one the body reader names, and one in the
; body pass. front.bc is this module as LLVM 19's llvm-as writes it:
;   llvm-as-19 -o front.bc front.ll
target datalayout = "e-m:e-i8:8:32-i16:16:32-i64:64-i128:128-n32:64-S128-Fn32"
target triple = "aarch64-unknown-linux-gnu"

declare void @g(ptr, ...)

define void @forward(ptr %p, ...) {
  musttail call void (ptr, ...) @g(ptr %p, ...)
  ret void
}

define i32 @add(i32 %a, i32 %b) {
  %s = add i32 %a, %b
  ret i32 %s
}
