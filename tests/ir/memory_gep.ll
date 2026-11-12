; load, store and getelementptr: an array filled through a variable index,
; struct fields at their natural offsets, a two-dimensional array, byte
; arithmetic with a negative index, and loads and stores at i8, i16 and
; i64 with explicit alignments.
;@ covers: spec/ir.md §5.4 memory (Load, Store), address (PtrAdd)
;@ tier: quick
;@ unsupported(aarch64): global:function-definition
;@ unsupported(x86_64): global:variable-definition
;@ stdout: squares: 0 9 49 sum=140
;@ stdout: record: -3 70000 -5000000000 77 -1234 size=24
;@ stdout: grid[2][3]=23 grid[1][0]=10 total=138
;@ stdout: bytes: abcdefg back=e
;@ stdout: widths: -2 4660 -81985529216486896

%record = type { i8, i32, i64, [4 x i16] }

@.squares = private unnamed_addr constant [26 x i8] c"squares: %d %d %d sum=%d\0A\00"
@.record = private unnamed_addr constant [36 x i8] c"record: %d %d %lld %d %d size=%lld\0A\00"
@.grid = private unnamed_addr constant [38 x i8] c"grid[2][3]=%d grid[1][0]=%d total=%d\0A\00"
@.bytes = private unnamed_addr constant [19 x i8] c"bytes: %s back=%c\0A\00"
@.widths = private unnamed_addr constant [20 x i8] c"widths: %d %d %lld\0A\00"

declare i32 @printf(ptr, ...)

define void @fill(ptr %array, i64 %n) {
entry:
  br label %loop
loop:
  %i = phi i64 [ 0, %entry ], [ %i.next, %loop ]
  %slot = getelementptr inbounds i32, ptr %array, i64 %i
  %i32 = trunc i64 %i to i32
  %sq = mul i32 %i32, %i32
  store i32 %sq, ptr %slot, align 4
  %i.next = add i64 %i, 1
  %more = icmp ult i64 %i.next, %n
  br i1 %more, label %loop, label %done
done:
  ret void
}

define i32 @total(ptr %array, i64 %n) {
entry:
  br label %loop
loop:
  %i = phi i64 [ 0, %entry ], [ %i.next, %loop ]
  %acc = phi i32 [ 0, %entry ], [ %acc.next, %loop ]
  %slot = getelementptr inbounds i32, ptr %array, i64 %i
  %v = load i32, ptr %slot, align 4
  %acc.next = add i32 %acc, %v
  %i.next = add i64 %i, 1
  %more = icmp ult i64 %i.next, %n
  br i1 %more, label %loop, label %done
done:
  ret i32 %acc.next
}

define void @squares() {
  %array = alloca [8 x i32], align 4
  call void @fill(ptr %array, i64 8)
  %p0 = getelementptr inbounds [8 x i32], ptr %array, i64 0, i64 0
  %p3 = getelementptr inbounds [8 x i32], ptr %array, i64 0, i64 3
  %p7 = getelementptr inbounds [8 x i32], ptr %array, i64 0, i64 7
  %v0 = load i32, ptr %p0, align 4
  %v3 = load i32, ptr %p3, align 4
  %v7 = load i32, ptr %p7, align 4
  %sum = call i32 @total(ptr %array, i64 8)
  call i32 (ptr, ...) @printf(ptr @.squares, i32 %v0, i32 %v3, i32 %v7, i32 %sum)
  ret void
}

define void @set_record(ptr %r, i64 %which) {
  %f0 = getelementptr inbounds %record, ptr %r, i32 0, i32 0
  %f1 = getelementptr inbounds %record, ptr %r, i32 0, i32 1
  %f2 = getelementptr inbounds %record, ptr %r, i32 0, i32 2
  %f3 = getelementptr inbounds %record, ptr %r, i32 0, i32 3, i64 %which
  store i8 -3, ptr %f0, align 1
  store i32 70000, ptr %f1, align 4
  store i64 -5000000000, ptr %f2, align 8
  store i16 -1234, ptr %f3, align 2
  ret void
}

define void @records() {
  %r = alloca %record, align 8
  %f3.0 = getelementptr inbounds %record, ptr %r, i32 0, i32 3, i64 0
  store i16 77, ptr %f3.0, align 2
  call void @set_record(ptr %r, i64 2)
  %f0 = getelementptr inbounds %record, ptr %r, i32 0, i32 0
  %f1 = getelementptr inbounds %record, ptr %r, i32 0, i32 1
  %f2 = getelementptr inbounds %record, ptr %r, i32 0, i32 2
  %f3.2 = getelementptr inbounds %record, ptr %r, i32 0, i32 3, i64 2
  %v0 = load i8, ptr %f0, align 1
  %v1 = load i32, ptr %f1, align 4
  %v2 = load i64, ptr %f2, align 8
  %v3.0 = load i16, ptr %f3.0, align 2
  %v3.2 = load i16, ptr %f3.2, align 2
  %x0 = sext i8 %v0 to i32
  %x3.0 = sext i16 %v3.0 to i32
  %x3.2 = sext i16 %v3.2 to i32
  ; The size of the struct, as the distance to the next element.
  %end = getelementptr %record, ptr %r, i32 1
  %end.int = ptrtoint ptr %end to i64
  %r.int = ptrtoint ptr %r to i64
  %size = sub i64 %end.int, %r.int
  call i32 (ptr, ...) @printf(ptr @.record, i32 %x0, i32 %v1, i64 %v2, i32 %x3.0, i32 %x3.2, i64 %size)
  ret void
}

define void @grid() {
entry:
  %g = alloca [3 x [4 x i32]], align 4
  br label %rows
rows:
  %i = phi i64 [ 0, %entry ], [ %i.next, %rows.latch ]
  br label %cols
cols:
  %j = phi i64 [ 0, %rows ], [ %j.next, %cols ]
  %cell = getelementptr inbounds [3 x [4 x i32]], ptr %g, i64 0, i64 %i, i64 %j
  %ten = mul i64 %i, 10
  %v = add i64 %ten, %j
  %v32 = trunc i64 %v to i32
  store i32 %v32, ptr %cell, align 4
  %j.next = add i64 %j, 1
  %cols.more = icmp ult i64 %j.next, 4
  br i1 %cols.more, label %cols, label %rows.latch
rows.latch:
  %i.next = add i64 %i, 1
  %rows.more = icmp ult i64 %i.next, 3
  br i1 %rows.more, label %rows, label %done
done:
  %c23 = getelementptr inbounds [3 x [4 x i32]], ptr %g, i64 0, i64 2, i64 3
  %c10 = getelementptr inbounds [3 x [4 x i32]], ptr %g, i64 0, i64 1, i64 0
  %v23 = load i32, ptr %c23, align 4
  %v10 = load i32, ptr %c10, align 4
  %sum = call i32 @total(ptr %g, i64 12)
  call i32 (ptr, ...) @printf(ptr @.grid, i32 %v23, i32 %v10, i32 %sum)
  ret void
}

define void @bytes(i64 %back) {
  %buf = alloca [8 x i8], align 1
  %end = getelementptr inbounds i8, ptr %buf, i64 7
  store i8 0, ptr %end, align 1
  br label %loop
loop:
  %p = phi ptr [ %end, %0 ], [ %prev, %loop ]
  %c = phi i8 [ 103, %0 ], [ %c.next, %loop ]
  %prev = getelementptr inbounds i8, ptr %p, i64 -1
  store i8 %c, ptr %prev, align 1
  %c.next = sub i8 %c, 1
  %more = icmp ne ptr %prev, %buf
  br i1 %more, label %loop, label %done
done:
  %neg = sub i64 0, %back
  %at = getelementptr inbounds i8, ptr %end, i64 %neg
  %ch = load i8, ptr %at, align 1
  %ch32 = zext i8 %ch to i32
  call i32 (ptr, ...) @printf(ptr @.bytes, ptr %buf, i32 %ch32)
  ret void
}

; Unaligned loads and stores inside a byte buffer.
define void @widths() {
  %buf = alloca [16 x i8], align 1
  %p1 = getelementptr inbounds i8, ptr %buf, i64 1
  %p3 = getelementptr inbounds i8, ptr %buf, i64 3
  %p5 = getelementptr inbounds i8, ptr %buf, i64 5
  store i16 -2, ptr %p1, align 1
  store i16 4660, ptr %p3, align 1
  store i64 -81985529216486896, ptr %p5, align 1
  %a = load i16, ptr %p1, align 1
  %b = load i16, ptr %p3, align 1
  %c = load i64, ptr %p5, align 1
  %a32 = sext i16 %a to i32
  %b32 = zext i16 %b to i32
  call i32 (ptr, ...) @printf(ptr @.widths, i32 %a32, i32 %b32, i64 %c)
  ret void
}

define i32 @main() {
  call void @squares()
  call void @records()
  call void @grid()
  call void @bytes(i64 3)
  call void @widths()
  ret i32 0
}
