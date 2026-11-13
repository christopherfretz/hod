; The facade's fixture (crates/hod/tests/front.rs): one module with a
; reason in the module pass (the tiny code model), one the body reader names (a
; scalable vector, refused by design) and one in the body pass (`ghccc`,
; unplanned), none of which a code-generation package turns on.
; front.bc is this module as LLVM 19's llvm-as writes it:
;   llvm-as-19 -o front.bc front.ll
target datalayout = "e-m:e-i8:8:32-i16:16:32-i64:64-i128:128-n32:64-S128-Fn32"
target triple = "aarch64-unknown-linux-gnu"

!llvm.module.flags = !{!900}
!900 = !{i32 1, !"Code Model", i32 0}

define void @scalable() nounwind {
  %v = alloca <vscale x 4 x i32>
  ret void
}

define ghccc void @ghc() nounwind {
  ret void
}

define i32 @add(i32 %a, i32 %b) nounwind {
  %s = add i32 %a, %b
  ret i32 %s
}
