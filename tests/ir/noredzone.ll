; noredzone, as clang puts it on every function with -mno-red-zone (kernel
; code): no data below the stack pointer. llc without it keeps a leaf
; function's locals in the x86-64 red zone; with it, it moves the stack
; pointer first. hod never keeps data below the stack pointer, on either
; target, so the attribute changes no code: leaves, a function that
; calls, through an attribute group and written in the header.
;@ covers: spec/ir.md §5.7 attributes (noredzone)
;@ stdout: leaf 21
;@ stdout: sum 36
;@ stdout: outer 64

@.leaf = private unnamed_addr constant [9 x i8] c"leaf %d\0A\00"
@.sum = private unnamed_addr constant [8 x i8] c"sum %d\0A\00"
@.outer = private unnamed_addr constant [10 x i8] c"outer %d\0A\00"

declare i32 @printf(ptr, ...)

define i32 @leaf(i32 %x) noinline noredzone {
  %a = alloca [8 x i32]
  %p = getelementptr [8 x i32], ptr %a, i64 0, i64 3
  store volatile i32 %x, ptr %p
  %v = load volatile i32, ptr %p
  %r = mul i32 %v, 3
  ret i32 %r
}

define i32 @sum(i32 %n) noinline #0 {
entry:
  %a = alloca [8 x i32]
  br label %fill
fill:
  %i = phi i32 [ 0, %entry ], [ %i1, %fill ]
  %w = zext i32 %i to i64
  %p = getelementptr [8 x i32], ptr %a, i64 0, i64 %w
  %v = add i32 %i, %n
  store volatile i32 %v, ptr %p
  %i1 = add i32 %i, 1
  %more = icmp ult i32 %i1, 8
  br i1 %more, label %fill, label %add
add:
  %j = phi i32 [ 0, %fill ], [ %j1, %add ]
  %s = phi i32 [ 0, %fill ], [ %s1, %add ]
  %wj = zext i32 %j to i64
  %q = getelementptr [8 x i32], ptr %a, i64 0, i64 %wj
  %e = load volatile i32, ptr %q
  %s1 = add i32 %s, %e
  %j1 = add i32 %j, 1
  %done = icmp eq i32 %j1, 8
  br i1 %done, label %out, label %add
out:
  ret i32 %s1
}

define i32 @outer(i32 %x) noinline #0 {
  %slot = alloca i32
  store volatile i32 %x, ptr %slot
  %l = call i32 @leaf(i32 %x)
  %s = call i32 @sum(i32 %x)
  %v = load volatile i32, ptr %slot
  %t = add i32 %l, %s
  %r = add i32 %t, %v
  ret i32 %r
}

define i32 @main() #0 {
  %l = call i32 @leaf(i32 7)
  call i32 (ptr, ...) @printf(ptr @.leaf, i32 %l)
  %s = call i32 @sum(i32 1)
  call i32 (ptr, ...) @printf(ptr @.sum, i32 %s)
  %o = call i32 @outer(i32 3)
  call i32 (ptr, ...) @printf(ptr @.outer, i32 %o)
  ret i32 0
}

attributes #0 = { noredzone }
