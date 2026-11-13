; WP-084: llvm.sshl.sat/ushl.sat, readcyclecounter,
; is.constant of literal aggregates, and the operand of a folded
; is.constant that is never evaluated (GCC's bcp-1.c: a load through a
; null pointer that llc deletes with the call).
; llvm.aarch64.crc32* is not here: every seed test is also built for the
; other target, and llc for x86_64 aborts on an AArch64 intrinsic. It is
; compared with llc in crates/hod_a64/tests/tail.rs.
;@ covers: spec/ir.md §5.5 intrinsics (sshl.sat, ushl.sat, readcyclecounter, is.constant); spec/codegen.md §5.5.1 (WP-084)
;@ tier: quick
;@ stdout: sat 48 48 48 0
;@ stdout: sat 12800 127 12800 0
;@ stdout: sat -192 -128 65535 4294967295
;@ stdout: sat -200 -128 65535 4294967295
;@ stdout: sat 0 0 0 0
;@ stdout: misc 1 0

@.sat = private unnamed_addr constant [17 x i8] c"sat %d %d %u %u\0A\00"
@.misc = private unnamed_addr constant [12 x i8] c"misc %d %d\0A\00"
@null = global ptr null

declare i32 @printf(ptr, ...)
declare i32 @llvm.sshl.sat.i32(i32, i32)
declare i8 @llvm.sshl.sat.i8(i8, i8)
declare i16 @llvm.ushl.sat.i16(i16, i16)
declare i64 @llvm.ushl.sat.i64(i64, i64)
declare i64 @llvm.readcyclecounter()
declare i1 @llvm.is.constant.sl_i32i32s({ i32, i32 })
declare i1 @llvm.is.constant.i8(i8)

define void @sat(i32 %a, i32 %k) {
  %s = call i32 @llvm.sshl.sat.i32(i32 %a, i32 %k)
  %a8 = trunc i32 %a to i8
  %k8 = trunc i32 %k to i8
  %s8 = call i8 @llvm.sshl.sat.i8(i8 %a8, i8 %k8)
  %s8x = sext i8 %s8 to i32
  %a16 = trunc i32 %a to i16
  %k16 = trunc i32 %k to i16
  %u16 = call i16 @llvm.ushl.sat.i16(i16 %a16, i16 %k16)
  %u16x = zext i16 %u16 to i32
  %a64 = sext i32 %a to i64
  %k64 = zext i32 %k to i64
  %u64 = call i64 @llvm.ushl.sat.i64(i64 %a64, i64 %k64)
  %u64h = lshr i64 %u64, 32
  %u64t = trunc i64 %u64h to i32
  call i32 (ptr, ...) @printf(ptr @.sat, i32 %s, i32 %s8x, i32 %u16x, i32 %u64t)
  ret void
}

define i32 @probe(ptr %p) {
  %q = getelementptr inbounds i8, ptr %p, i64 123456
  %v = load i8, ptr %q, align 1
  %c = call i1 @llvm.is.constant.i8(i8 %v)
  %r = zext i1 %c to i32
  ret i32 %r
}

define i32 @main() {
  call void @sat(i32 3, i32 4)
  call void @sat(i32 100, i32 7)
  call void @sat(i32 -3, i32 6)
  call void @sat(i32 -100, i32 1)
  call void @sat(i32 0, i32 5)
  %t0 = call i64 @llvm.readcyclecounter()
  %t1 = call i64 @llvm.readcyclecounter()
  %up = icmp uge i64 %t1, %t0
  %agg = call i1 @llvm.is.constant.sl_i32i32s({ i32, i32 } { i32 1, i32 2 })
  %both = and i1 %up, %agg
  %b = zext i1 %both to i32
  %p = load volatile ptr, ptr @null
  %r = call i32 @probe(ptr %p)
  call i32 (ptr, ...) @printf(ptr @.misc, i32 %b, i32 %r)
  ret i32 0
}
