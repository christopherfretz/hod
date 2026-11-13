; llvm.memcpy, llvm.memmove and llvm.memset: small constant lengths (which
; a backend may expand inline), larger constant lengths, lengths known
; only at run time, a zero length, and overlapping moves in both
; directions.
;@ covers: spec/ir.md §5.5 intrinsics (memcpy, memmove, memset)
;@ stdout: copy 27: abcdefghijklmnopqrstuvwxyz
;@ stdout: copy 5: abcuvwxyijklmnopqrstuvwxyz
;@ stdout: copy n: abcuvwxyijabcdefghijklmxyz
;@ stdout: move down: efghijklmnopqrstuvwxuvwxyz
;@ stdout: move up: abcdabcdefghijklmnopqrstyz
;@ stdout: set 7: abc-------ghijklmnopqrstyz
;@ stdout: set n: abc-------***********rstyz
;@ stdout: zero length: abc-------***********rstyz
;@ stdout: big: checksum 991222188
;@ stdout: big: checksum 3634205541
;@ stdout: big: checksum 2845203503
;@ stdout: struct: checksum 3675954214
;@ stdout: struct: contents

@.text = private unnamed_addr constant [8 x i8] c"%s: %s\0A\00"
@.sum = private unnamed_addr constant [17 x i8] c"%s: checksum %u\0A\00"
@.alphabet = private unnamed_addr constant [27 x i8] c"abcdefghijklmnopqrstuvwxyz\00"
@.l.copy = private unnamed_addr constant [8 x i8] c"copy 27\00"
@.l.small = private unnamed_addr constant [7 x i8] c"copy 5\00"
@.l.var = private unnamed_addr constant [7 x i8] c"copy n\00"
@.l.fwd = private unnamed_addr constant [10 x i8] c"move down\00"
@.l.back = private unnamed_addr constant [8 x i8] c"move up\00"
@.l.set = private unnamed_addr constant [6 x i8] c"set 7\00"
@.l.setvar = private unnamed_addr constant [6 x i8] c"set n\00"
@.l.zero = private unnamed_addr constant [12 x i8] c"zero length\00"
@.l.big = private unnamed_addr constant [4 x i8] c"big\00"
@.l.struct = private unnamed_addr constant [7 x i8] c"struct\00"
@big.src = global [1000 x i8] zeroinitializer
@big.dst = global [1000 x i8] zeroinitializer
@record = global { i64, i32, i16, i8, [9 x i8] } { i64 81985529216486895, i32 -559038737, i16 4660, i8 86, [9 x i8] c"contents\00" }

declare i32 @printf(ptr, ...)
declare void @llvm.memcpy.p0.p0.i64(ptr, ptr, i64, i1)
declare void @llvm.memcpy.p0.p0.i32(ptr, ptr, i32, i1)
declare void @llvm.memmove.p0.p0.i64(ptr, ptr, i64, i1)
declare void @llvm.memset.p0.i64(ptr, i8, i64, i1)

define i32 @checksum(ptr %p, i64 %n) {
entry:
  br label %loop
loop:
  %i = phi i64 [ 0, %entry ], [ %i.next, %loop ]
  %acc = phi i32 [ 0, %entry ], [ %acc.next, %loop ]
  %at = getelementptr inbounds i8, ptr %p, i64 %i
  %b = load i8, ptr %at
  %b.w = zext i8 %b to i32
  %rot = mul i32 %acc, 31
  %acc.next = add i32 %rot, %b.w
  %i.next = add i64 %i, 1
  %done = icmp eq i64 %i.next, %n
  br i1 %done, label %exit, label %loop
exit:
  ret i32 %acc.next
}

define void @copy_n(ptr %dst, ptr %src, i64 %n) {
  call void @llvm.memcpy.p0.p0.i64(ptr %dst, ptr %src, i64 %n, i1 false)
  ret void
}

define void @move_n(ptr %dst, ptr %src, i64 %n) {
  call void @llvm.memmove.p0.p0.i64(ptr %dst, ptr %src, i64 %n, i1 false)
  ret void
}

define void @set_n(ptr %dst, i8 %value, i64 %n) {
  call void @llvm.memset.p0.i64(ptr %dst, i8 %value, i64 %n, i1 false)
  ret void
}

define i32 @main() {
entry:
  %buf = alloca [64 x i8]
  %copy = alloca { i64, i32, i16, i8, [9 x i8] }
  call void @llvm.memset.p0.i64(ptr %buf, i8 0, i64 64, i1 false)
  call void @llvm.memcpy.p0.p0.i64(ptr %buf, ptr @.alphabet, i64 27, i1 false)
  call i32 (ptr, ...) @printf(ptr @.text, ptr @.l.copy, ptr %buf)

  %at3 = getelementptr inbounds i8, ptr %buf, i64 3
  %from20 = getelementptr inbounds i8, ptr @.alphabet, i64 20
  call void @llvm.memcpy.p0.p0.i32(ptr align 1 %at3, ptr align 1 %from20, i32 5, i1 false)
  call i32 (ptr, ...) @printf(ptr @.text, ptr @.l.small, ptr %buf)

  %at10 = getelementptr inbounds i8, ptr %buf, i64 10
  call void @copy_n(ptr %at10, ptr @.alphabet, i64 13)
  call i32 (ptr, ...) @printf(ptr @.text, ptr @.l.var, ptr %buf)

  call void @copy_n(ptr %buf, ptr @.alphabet, i64 27)
  %at4 = getelementptr inbounds i8, ptr %buf, i64 4
  call void @move_n(ptr %buf, ptr %at4, i64 20)
  call i32 (ptr, ...) @printf(ptr @.text, ptr @.l.fwd, ptr %buf)

  call void @copy_n(ptr %buf, ptr @.alphabet, i64 27)
  call void @llvm.memmove.p0.p0.i64(ptr %at4, ptr %buf, i64 20, i1 false)
  call i32 (ptr, ...) @printf(ptr @.text, ptr @.l.back, ptr %buf)

  call void @llvm.memset.p0.i64(ptr %at3, i8 45, i64 7, i1 false)
  call i32 (ptr, ...) @printf(ptr @.text, ptr @.l.set, ptr %buf)

  call void @set_n(ptr %at10, i8 42, i64 11)
  call i32 (ptr, ...) @printf(ptr @.text, ptr @.l.setvar, ptr %buf)

  call void @set_n(ptr %buf, i8 33, i64 0)
  call void @copy_n(ptr %buf, ptr %at10, i64 0)
  call void @move_n(ptr %at10, ptr %buf, i64 0)
  call i32 (ptr, ...) @printf(ptr @.text, ptr @.l.zero, ptr %buf)
  br label %fill

fill:
  %i = phi i64 [ 0, %entry ], [ %i.next, %fill ]
  %slot = getelementptr inbounds [1000 x i8], ptr @big.src, i64 0, i64 %i
  %sq = mul i64 %i, %i
  %b = trunc i64 %sq to i8
  store i8 %b, ptr %slot
  %i.next = add i64 %i, 1
  %done = icmp eq i64 %i.next, 1000
  br i1 %done, label %big, label %fill

big:
  call void @llvm.memcpy.p0.p0.i64(ptr align 16 @big.dst, ptr align 16 @big.src, i64 1000, i1 false)
  %c1 = call i32 @checksum(ptr @big.dst, i64 1000)
  call i32 (ptr, ...) @printf(ptr @.sum, ptr @.l.big, i32 %c1)
  %dst1 = getelementptr inbounds i8, ptr @big.dst, i64 1
  call void @llvm.memmove.p0.p0.i64(ptr %dst1, ptr @big.dst, i64 999, i1 false)
  %c2 = call i32 @checksum(ptr @big.dst, i64 1000)
  call i32 (ptr, ...) @printf(ptr @.sum, ptr @.l.big, i32 %c2)
  %dst500 = getelementptr inbounds i8, ptr @big.dst, i64 500
  call void @llvm.memset.p0.i64(ptr %dst500, i8 -1, i64 300, i1 false)
  %c3 = call i32 @checksum(ptr @big.dst, i64 1000)
  call i32 (ptr, ...) @printf(ptr @.sum, ptr @.l.big, i32 %c3)

  call void @llvm.memcpy.p0.p0.i64(ptr align 8 %copy, ptr align 8 @record, i64 24, i1 false)
  %c4 = call i32 @checksum(ptr %copy, i64 24)
  call i32 (ptr, ...) @printf(ptr @.sum, ptr @.l.struct, i32 %c4)
  %name = getelementptr inbounds { i64, i32, i16, i8, [9 x i8] }, ptr %copy, i32 0, i32 4
  call i32 (ptr, ...) @printf(ptr @.text, ptr @.l.struct, ptr %name)
  ret i32 0
}
