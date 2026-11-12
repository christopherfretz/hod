; main with argc and argv, input from stdin, and a non-zero exit status:
; prints the argument count, the arguments after the program name, an
; empty line, and then copies stdin to stdout.
;@ covers: spec/ir.md §5.4 call, memory (the process interface every other test relies on)
;@ tier: quick
;@ args: alpha beta
;@ stdin: one line of input
;@ exit: 3
;@ unsupported(x86_64): *
;@ stdout: argc=3
;@ stdout: alpha beta
;@ stdout:
;@ stdout: one line of input

@.argc = private unnamed_addr constant [9 x i8] c"argc=%d\0A\00"
@.arg = private unnamed_addr constant [5 x i8] c"%s%s\00"

@.space = private unnamed_addr constant [2 x i8] c" \00"
@.empty = private unnamed_addr constant [1 x i8] c"\00"

declare i32 @printf(ptr, ...)
declare i32 @getchar()
declare i32 @putchar(i32)

define i32 @main(i32 %argc, ptr %argv) {
entry:
  call i32 (ptr, ...) @printf(ptr @.argc, i32 %argc)
  %none = icmp sle i32 %argc, 1
  br i1 %none, label %args.done, label %args
args:
  %i = phi i32 [ 1, %entry ], [ %i.next, %args ]
  %i.w = sext i32 %i to i64
  %slot = getelementptr inbounds ptr, ptr %argv, i64 %i.w
  %arg = load ptr, ptr %slot
  %first = icmp eq i32 %i, 1
  %sep = select i1 %first, ptr @.empty, ptr @.space
  call i32 (ptr, ...) @printf(ptr @.arg, ptr %sep, ptr %arg)
  %i.next = add i32 %i, 1
  %more = icmp slt i32 %i.next, %argc
  br i1 %more, label %args, label %args.done
args.done:
  call i32 @putchar(i32 10)
  call i32 @putchar(i32 10)
  br label %copy
copy:
  %c = call i32 @getchar()
  %eof = icmp slt i32 %c, 0
  br i1 %eof, label %exit, label %echo
echo:
  call i32 @putchar(i32 %c)
  br label %copy
exit:
  ret i32 %argc
}
