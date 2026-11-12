; llvm.global_ctors and llvm.global_dtors: constructors run before main in
; priority order (lower numbers first, then the order of the list), and
; destructors run after main returns in the reverse order. A constructor
; also fills in a table that main reads.
;@ covers: spec/ir.md §2.2 symbols (Module::ctors, Module::dtors)
;@ unsupported: codegen-not-implemented
;@ stdout: ctor early (101), step 1
;@ stdout: ctor middle (200), step 2
;@ stdout: ctor first (default), step 3
;@ stdout: ctor second (default), step 4
;@ stdout: main: step 5, table 11 22 33 44
;@ stdout: dtor first (default), step 6
;@ stdout: dtor middle (200), step 7
;@ stdout: dtor early (101), step 8

@.ctor = private unnamed_addr constant [18 x i8] c"ctor %s, step %d\0A\00"
@.dtor = private unnamed_addr constant [18 x i8] c"dtor %s, step %d\0A\00"
@.main = private unnamed_addr constant [34 x i8] c"main: step %d, table %d %d %d %d\0A\00"
@.early = private unnamed_addr constant [12 x i8] c"early (101)\00"
@.middle = private unnamed_addr constant [13 x i8] c"middle (200)\00"
@.first = private unnamed_addr constant [16 x i8] c"first (default)\00"
@.second = private unnamed_addr constant [17 x i8] c"second (default)\00"
@step = global i32 0
@table = global [4 x i32] zeroinitializer

@llvm.global_ctors = appending global [4 x { i32, ptr, ptr }] [
  { i32, ptr, ptr } { i32 65535, ptr @ctor_first, ptr null },
  { i32, ptr, ptr } { i32 200, ptr @ctor_middle, ptr null },
  { i32, ptr, ptr } { i32 65535, ptr @ctor_second, ptr null },
  { i32, ptr, ptr } { i32 101, ptr @ctor_early, ptr null }
]
@llvm.global_dtors = appending global [3 x { i32, ptr, ptr }] [
  { i32, ptr, ptr } { i32 65535, ptr @dtor_first, ptr null },
  { i32, ptr, ptr } { i32 101, ptr @dtor_early, ptr null },
  { i32, ptr, ptr } { i32 200, ptr @dtor_middle, ptr null }
]

declare i32 @printf(ptr, ...)

define internal i32 @next() {
  %old = load i32, ptr @step
  %new = add i32 %old, 1
  store i32 %new, ptr @step
  ret i32 %new
}

define internal void @ctor_early() {
  %s = call i32 @next()
  call i32 (ptr, ...) @printf(ptr @.ctor, ptr @.early, i32 %s)
  ret void
}

define internal void @ctor_middle() {
entry:
  %s = call i32 @next()
  call i32 (ptr, ...) @printf(ptr @.ctor, ptr @.middle, i32 %s)
  br label %fill
fill:
  %i = phi i32 [ 0, %entry ], [ %i.next, %fill ]
  %slot = getelementptr inbounds [4 x i32], ptr @table, i32 0, i32 %i
  %v0 = add i32 %i, 1
  %v = mul i32 %v0, 11
  store i32 %v, ptr %slot
  %i.next = add i32 %i, 1
  %done = icmp eq i32 %i.next, 4
  br i1 %done, label %exit, label %fill
exit:
  ret void
}

define internal void @ctor_first() {
  %s = call i32 @next()
  call i32 (ptr, ...) @printf(ptr @.ctor, ptr @.first, i32 %s)
  ret void
}

define internal void @ctor_second() {
  %s = call i32 @next()
  call i32 (ptr, ...) @printf(ptr @.ctor, ptr @.second, i32 %s)
  ret void
}

define internal void @dtor_early() {
  %s = call i32 @next()
  call i32 (ptr, ...) @printf(ptr @.dtor, ptr @.early, i32 %s)
  ret void
}

define internal void @dtor_middle() {
  %s = call i32 @next()
  call i32 (ptr, ...) @printf(ptr @.dtor, ptr @.middle, i32 %s)
  ret void
}

define internal void @dtor_first() {
  %s = call i32 @next()
  call i32 (ptr, ...) @printf(ptr @.dtor, ptr @.first, i32 %s)
  ret void
}

define i32 @main() {
  %s = call i32 @next()
  %t0 = load i32, ptr @table
  %t1 = load i32, ptr getelementptr inbounds ([4 x i32], ptr @table, i32 0, i32 1)
  %t2 = load i32, ptr getelementptr inbounds ([4 x i32], ptr @table, i32 0, i32 2)
  %t3 = load i32, ptr getelementptr inbounds ([4 x i32], ptr @table, i32 0, i32 3)
  call i32 (ptr, ...) @printf(ptr @.main, i32 %s, i32 %t0, i32 %t1, i32 %t2, i32 %t3)
  ret i32 0
}
