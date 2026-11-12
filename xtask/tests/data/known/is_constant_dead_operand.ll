; llvm.is.constant of a value loaded through an invalid pointer, the shape
; clang and kiln emit for GCC's bcp-1.c (`__builtin_constant_p(p[123456])`,
; an operand C does not evaluate). llc's LowerConstantIntrinsics folds the
; call to false and deletes the operand's now-dead computation
; (RecursivelyDeleteTriviallyDeadInstructions), so the load never runs and
; main returns 0. Found by WP-086 (corpus `gcc-torture/llvm*/bcp-1.c.ll`).
;@ covers: spec/ir.md §5.5 intrinsics (is.constant)
;@ unsupported(x86_64): *

@null = global ptr null

declare i1 @llvm.is.constant.i8(i8) #0

define i32 @probe(ptr %p) {
  %q = getelementptr inbounds i8, ptr %p, i64 123456
  %v = load i8, ptr %q, align 1
  %c = call i1 @llvm.is.constant.i8(i8 %v)
  %r = zext i1 %c to i32
  ret i32 %r
}

define i32 @main() {
  %p = load volatile ptr, ptr @null
  %r = call i32 @probe(ptr %p)
  ret i32 %r
}

attributes #0 = { convergent nocallback nofree nosync nounwind willreturn memory(none) }
