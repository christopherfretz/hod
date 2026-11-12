; Globals whose initializers need relocations: a pointer to another global,
; a pointer into the middle of an array, a pointer to itself, a struct with
; pointers and integers mixed, a table of function pointers, a pointer in
; constant data, and an internal counter that is written at run time.
;@ covers: spec/ir.md §4.2 addresses, §4.3 initializer images
;@ tier: quick
;@ unsupported: codegen-not-implemented
;@ stdout: number=7 third=30 bytes=world self=1
;@ stdout: first: tag=1 f(9)=18
;@ stdout: second: tag=2 f(9)=81
;@ stdout: functions[0](5) = 10
;@ stdout: functions[1](5) = 25
;@ stdout: functions[2](5) = -5
;@ stdout: counter=103 zeroed=0

%entry = type { ptr, i32, ptr }

@number = global i32 7
@numbers = global [4 x i32] [i32 10, i32 20, i32 30, i32 40]
@to_number = global ptr @number
@to_third = global ptr getelementptr inbounds ([4 x i32], ptr @numbers, i64 0, i64 2)
@to_bytes = global ptr getelementptr inbounds (i8, ptr @.text, i64 6)
@self = global ptr @self
@counter = internal global i64 0
@zeroed = global [3 x i64] zeroinitializer

@.text = private unnamed_addr constant [12 x i8] c"hello world\00"
@.first = private unnamed_addr constant [6 x i8] c"first\00"
@.second = private unnamed_addr constant [7 x i8] c"second\00"
@table = internal constant [2 x %entry] [
  %entry { ptr @.first, i32 1, ptr @double_it },
  %entry { ptr @.second, i32 2, ptr @square_it }
]
@functions = global [3 x ptr] [ptr @double_it, ptr @square_it, ptr @negate_it]
@to_table = constant ptr getelementptr inbounds ([2 x %entry], ptr @table, i64 0, i64 1)

@.ptrs = private unnamed_addr constant [37 x i8] c"number=%d third=%d bytes=%s self=%d\0A\00"
@.entry = private unnamed_addr constant [20 x i8] c"%s: tag=%d f(9)=%d\0A\00"
@.fn = private unnamed_addr constant [23 x i8] c"functions[%d](5) = %d\0A\00"
@.count = private unnamed_addr constant [26 x i8] c"counter=%lld zeroed=%lld\0A\00"

declare i32 @printf(ptr, ...)

define i32 @double_it(i32 %v) {
  %r = add i32 %v, %v
  ret i32 %r
}

define i32 @square_it(i32 %v) {
  %r = mul i32 %v, %v
  ret i32 %r
}

define internal i32 @negate_it(i32 %v) {
  %r = sub i32 0, %v
  ret i32 %r
}

define void @bump(i64 %by) {
  %old = load i64, ptr @counter
  %new = add i64 %old, %by
  store i64 %new, ptr @counter
  ret void
}

define void @show_entry(ptr %e) {
  %name.p = getelementptr inbounds %entry, ptr %e, i32 0, i32 0
  %tag.p = getelementptr inbounds %entry, ptr %e, i32 0, i32 1
  %fn.p = getelementptr inbounds %entry, ptr %e, i32 0, i32 2
  %name = load ptr, ptr %name.p
  %tag = load i32, ptr %tag.p
  %fn = load ptr, ptr %fn.p
  %r = call i32 %fn(i32 9)
  call i32 (ptr, ...) @printf(ptr @.entry, ptr %name, i32 %tag, i32 %r)
  ret void
}

define i32 @main() {
entry:
  %p1 = load ptr, ptr @to_number
  %v1 = load i32, ptr %p1
  %p2 = load ptr, ptr @to_third
  %v2 = load i32, ptr %p2
  %p3 = load ptr, ptr @to_bytes
  %p4 = load ptr, ptr @self
  %same = icmp eq ptr %p4, @self
  %v4 = zext i1 %same to i32
  call i32 (ptr, ...) @printf(ptr @.ptrs, i32 %v1, i32 %v2, ptr %p3, i32 %v4)
  call void @show_entry(ptr @table)
  %second = load ptr, ptr @to_table
  call void @show_entry(ptr %second)
  br label %loop
loop:
  %i = phi i64 [ 0, %entry ], [ %next, %loop ]
  %slot = getelementptr inbounds [3 x ptr], ptr @functions, i64 0, i64 %i
  %fn = load ptr, ptr %slot
  %r = call i32 %fn(i32 5)
  %i32 = trunc i64 %i to i32
  call i32 (ptr, ...) @printf(ptr @.fn, i32 %i32, i32 %r)
  call void @bump(i64 %i)
  %next = add i64 %i, 1
  %more = icmp ult i64 %next, 3
  br i1 %more, label %loop, label %done
done:
  call void @bump(i64 100)
  %count = load i64, ptr @counter
  %z.p = getelementptr inbounds [3 x i64], ptr @zeroed, i64 0, i64 2
  %z = load i64, ptr %z.p
  call i32 (ptr, ...) @printf(ptr @.count, i64 %count, i64 %z)
  ret i32 0
}
