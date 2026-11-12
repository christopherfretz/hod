; llvm.is.constant of literal aggregates: llc's LowerConstantIntrinsics
; calls a struct or array literal of manifest constants a constant (true),
; so main returns 1 | 2 = 3. clang and kiln never emit this (they fold
; __builtin_constant_p of a non-scalar in the front end); found by WP-086.
;@ covers: spec/ir.md §5.5 intrinsics (is.constant)
;@ unsupported(x86_64): *
;@ exit: 3

declare i1 @llvm.is.constant.sl_i32i32s({ i32, i32 }) #0
declare i1 @llvm.is.constant.a2i32([2 x i32]) #0

define i32 @main() {
  %a = call i1 @llvm.is.constant.sl_i32i32s({ i32, i32 } { i32 1, i32 2 })
  %b = call i1 @llvm.is.constant.a2i32([2 x i32] [i32 1, i32 2])
  %x = zext i1 %a to i32
  %y = zext i1 %b to i32
  %s = shl i32 %y, 1
  %r = or i32 %x, %s
  ret i32 %r
}

attributes #0 = { convergent nounwind memory(none) }
