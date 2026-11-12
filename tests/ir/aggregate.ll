; First-class aggregates: insertvalue and extractvalue on structs, nested
; structs and arrays; structs passed and returned by value; whole-aggregate
; load and store; and aggregate phi and select.
;@ covers: spec/ir.md §5.4 aggregate (ExtractValue, InsertValue)
;@ unsupported: *
;@ stdout: made: { 7, -9000000000 }
;@ stdout: swapped: { -410065403, 2 }
;@ stdout: picked: { -410065403, 2 }
;@ stdout: merged: { 3, 0 }
;@ stdout: merged: { 6, 1 }
;@ stdout: mixed: { 3.75, 42 }
;@ stdout: nested: -3 { 11, -22 } [100 -200 300] 0.125
;@ stdout: nested: 9 { -410065403, 2 } [100 -1 300] 2.500
;@ stdout: sum=401

%pair = type { i32, i64 }
%mixed = type { double, i32 }
%nested = type { i8, %pair, [3 x i16], double }

@.pair = private unnamed_addr constant [18 x i8] c"%s: { %d, %lld }\0A\00"
@.mixed = private unnamed_addr constant [21 x i8] c"mixed: { %.2f, %d }\0A\00"
@.nested = private unnamed_addr constant [41 x i8] c"nested: %d { %d, %lld } [%d %d %d] %.3f\0A\00"
@.sum = private unnamed_addr constant [10 x i8] c"sum=%lld\0A\00"
@.made = private unnamed_addr constant [5 x i8] c"made\00"
@.swapped = private unnamed_addr constant [8 x i8] c"swapped\00"
@.picked = private unnamed_addr constant [7 x i8] c"picked\00"
@.merged = private unnamed_addr constant [7 x i8] c"merged\00"
@seed = global %nested { i8 -3, %pair { i32 11, i64 -22 }, [3 x i16] [i16 100, i16 -200, i16 300], double 0.125 }

declare i32 @printf(ptr, ...)

define %pair @make(i32 %a, i64 %b) {
  %p0 = insertvalue %pair poison, i32 %a, 0
  %p1 = insertvalue %pair %p0, i64 %b, 1
  ret %pair %p1
}

; { a, b } becomes { trunc(b) + k, sext(a) - k }.
define %pair @swap(%pair %p, i32 %k) {
  %a = extractvalue %pair %p, 0
  %b = extractvalue %pair %p, 1
  %b32 = trunc i64 %b to i32
  %first = add i32 %b32, %k
  %a64 = sext i32 %a to i64
  %k64 = sext i32 %k to i64
  %second = sub i64 %a64, %k64
  %r0 = insertvalue %pair %p, i32 %first, 0
  %r1 = insertvalue %pair %r0, i64 %second, 1
  ret %pair %r1
}

define %mixed @scale(%mixed %m, double %by) {
  %d = extractvalue %mixed %m, 0
  %n = extractvalue %mixed %m, 1
  %d2 = fmul double %d, %by
  %n2 = add i32 %n, 1
  %r0 = insertvalue %mixed poison, double %d2, 0
  %r1 = insertvalue %mixed %r0, i32 %n2, 1
  ret %mixed %r1
}

define void @show_pair(ptr %label, %pair %p) {
  %a = extractvalue %pair %p, 0
  %b = extractvalue %pair %p, 1
  call i32 (ptr, ...) @printf(ptr @.pair, ptr %label, i32 %a, i64 %b)
  ret void
}

define void @show_nested(ptr %at) {
  %n = load %nested, ptr %at
  %tag = extractvalue %nested %n, 0
  %a = extractvalue %nested %n, 1, 0
  %b = extractvalue %nested %n, 1, 1
  %e0 = extractvalue %nested %n, 2, 0
  %e1 = extractvalue %nested %n, 2, 1
  %e2 = extractvalue %nested %n, 2, 2
  %d = extractvalue %nested %n, 3
  %tag.i = sext i8 %tag to i32
  %e0.i = sext i16 %e0 to i32
  %e1.i = sext i16 %e1 to i32
  %e2.i = sext i16 %e2 to i32
  call i32 (ptr, ...) @printf(ptr @.nested, i32 %tag.i, i32 %a, i64 %b, i32 %e0.i, i32 %e1.i, i32 %e2.i, double %d)
  ret void
}

; A phi of aggregates: the pair that reaches the join depends on the flag.
define %pair @merge(i1 %flag, %pair %x, %pair %y) {
entry:
  br i1 %flag, label %left, label %right
left:
  %l = call %pair @swap(%pair %x, i32 1)
  br label %join
right:
  %r = call %pair @swap(%pair %y, i32 2)
  br label %join
join:
  %p = phi %pair [ %l, %left ], [ %r, %right ]
  ret %pair %p
}

define i32 @main() {
  %local = alloca %nested
  %p = call %pair @make(i32 7, i64 -9000000000)
  call void @show_pair(ptr @.made, %pair %p)
  %q = call %pair @swap(%pair %p, i32 5)
  call void @show_pair(ptr @.swapped, %pair %q)
  %a = extractvalue %pair %p, 0
  %big = icmp sgt i32 %a, 100
  %s = select i1 %big, %pair %p, %pair %q
  call void @show_pair(ptr @.picked, %pair %s)
  %m1 = call %pair @merge(i1 true, %pair { i32 1, i64 2 }, %pair { i32 3, i64 4 })
  call void @show_pair(ptr @.merged, %pair %m1)
  %m2 = call %pair @merge(i1 false, %pair { i32 1, i64 2 }, %pair { i32 3, i64 4 })
  call void @show_pair(ptr @.merged, %pair %m2)

  %m = call %mixed @scale(%mixed { double 1.25, i32 41 }, double 3.0)
  %md = extractvalue %mixed %m, 0
  %mn = extractvalue %mixed %m, 1
  call i32 (ptr, ...) @printf(ptr @.mixed, double %md, i32 %mn)

  call void @show_nested(ptr @seed)
  %n0 = load %nested, ptr @seed
  %n1 = insertvalue %nested %n0, %pair %q, 1
  %n2 = insertvalue %nested %n1, i16 -1, 2, 1
  %n3 = insertvalue %nested %n2, i8 9, 0
  %n4 = insertvalue %nested %n3, double 2.5, 3
  store %nested %n4, ptr %local
  call void @show_nested(ptr %local)

  %arr = extractvalue %nested %n4, 2
  %x0 = extractvalue [3 x i16] %arr, 0
  %x1 = extractvalue [3 x i16] %arr, 1
  %x2 = extractvalue [3 x i16] %arr, 2
  %x0.w = sext i16 %x0 to i64
  %x1.w = sext i16 %x1 to i64
  %x2.w = sext i16 %x2 to i64
  %s0 = add i64 %x0.w, %x1.w
  %s1 = add i64 %s0, %x2.w
  %inner = extractvalue %nested %n4, 1, 1
  %s2 = add i64 %s1, %inner
  call i32 (ptr, ...) @printf(ptr @.sum, i64 %s2)
  ret i32 0
}
