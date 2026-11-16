; WP-307: module-level assembly of a non-PIC i686 program. The text
; defines functions and data the IR uses (declared here, defined there),
; calls an IR function, reads IR variables and a thread-local (LE through
; @ntpoff, IE through @indntpoff; @tpoff is `tls32.ll`'s), and gives
; directives to symbols of both sides; function-level assembly in
; `prog_main` calls a function of the module text.
target triple = "i686-unknown-linux-gnu"

module asm ".text"
module asm ".p2align 4"
module asm ".globl asm_add"
module asm ".type asm_add, @function"
module asm "asm_add:"
module asm "  movl 4(%esp), %eax"
module asm "  addl 8(%esp), %eax"
module asm "  ret"
module asm ".size asm_add, .-asm_add"
module asm ".globl asm_sum"
module asm ".type asm_sum, @function"
module asm "asm_sum:"
module asm "  xorl %eax, %eax"
module asm "  movl $asm_table, %ecx"
module asm "  xorl %edx, %edx"
module asm "1:"
module asm "  addl (%ecx,%edx,4), %eax"
module asm "  incl %edx"
module asm "  cmpl asm_count, %edx"
module asm "  jl 1b"
module asm "  addl asm_table+12, %eax"
module asm "  movsbl asm_bytes+1, %ecx"
module asm "  addl %ecx, %eax"
module asm "  ret"
module asm ".globl asm_call_ir"
module asm ".type asm_call_ir, @function"
module asm "asm_call_ir:"
module asm "  pushl 4(%esp)"
module asm "  call ir_twice"
module asm "  addl $4, %esp"
module asm "  addl ir_value, %eax"
module asm "  movl ir_hidden_ptr, %ecx"
module asm "  addl (%ecx), %eax"
module asm "  pushl %eax"
module asm "  call ir_hidden_fn"
module asm "  addl $4, %esp"
module asm "  jmp .Ldone"
module asm "  ud2"
module asm ".Ldone:"
module asm "  ret"
module asm ".globl asm_tls"
module asm ".type asm_tls, @function"
module asm "asm_tls:"
module asm "  movl %gs:tls_v@ntpoff, %eax"
module asm "  movl tls_v@indntpoff, %ecx"
module asm "  addl %gs:(%ecx), %eax"
module asm "  movl $tls_v@ntpoff, %ecx"
module asm "  addl %gs:(%ecx), %eax"
module asm "  ret"
module asm ".weak asm_weak"
module asm ".type asm_weak, @function"
module asm "asm_weak:"
module asm "  movl $7, %eax"
module asm "  ret"
module asm ".globl asm_hidden"
module asm ".hidden asm_hidden"
module asm "asm_hidden:"
module asm "  movl $asm_msg, %eax"
module asm "  ret"
module asm ".data"
module asm ".globl asm_table"
module asm ".type asm_table, @object"
module asm "asm_table: .long 1, 2, 3, 4"
module asm ".size asm_table, 16"
module asm ".globl asm_count"
module asm "asm_count: .long 4"
module asm ".globl asm_ptr"
module asm "asm_ptr: .long ir_value"
module asm "asm_bytes: .byte 9, -3, 0"
module asm ".pushsection .rodata"
module asm "asm_msg: .asciz \22from asm\22"
module asm ".popsection"
module asm ".hidden ir_hidden_fn"

@ir_value = global i32 100
@ir_hidden = hidden global i32 1000
@ir_hidden_ptr = global ptr @ir_hidden
@tls_v = thread_local global i32 7
@asm_table = external global [4 x i32]
@asm_count = external global i32
@asm_ptr = external global ptr
@fmt = private constant [34 x i8] c"%d %d %d %d %d %d %d %d %d %d %s\0A\00"

declare i32 @asm_add(i32, i32)
declare i32 @asm_sum()
declare i32 @asm_call_ir(i32)
declare i32 @asm_tls()
declare i32 @asm_weak()
declare ptr @asm_hidden()
declare i32 @printf(ptr, ...)

define i32 @ir_twice(i32 %x) #0 {
  %r = mul i32 %x, 2
  ret i32 %r
}

define hidden i32 @ir_hidden_fn(i32 %x) #0 {
  %r = add i32 %x, 10000
  ret i32 %r
}

define i32 @prog_main() #0 {
  %a = call i32 @asm_add(i32 40, i32 2)
  %b = call i32 @asm_sum()
  %c = call i32 @asm_call_ir(i32 21)
  %d = call i32 @asm_tls()
  %p = load ptr, ptr @asm_ptr
  %e = load i32, ptr %p
  %f = call i32 @asm_weak()
  %t = getelementptr [4 x i32], ptr @asm_table, i32 0, i32 2
  %g = load i32, ptr %t
  %h = load i32, ptr @asm_count
  store i32 8, ptr @tls_v
  %i = call i32 @asm_tls()
  %m = call ptr @asm_hidden()
  ; Function-level assembly calls into the module's text.
  %j = call i32 asm sideeffect "pushl $$5\0A\09pushl $$6\0A\09call asm_add\0A\09addl $$8, %esp", "={eax},~{ecx},~{edx},~{memory},~{dirflag},~{fpsr},~{flags}"()
  %r = call i32 (ptr, ...) @printf(ptr @fmt, i32 %a, i32 %b, i32 %c, i32 %d, i32 %e, i32 %f, i32 %g, i32 %h, i32 %i, i32 %j, ptr %m)
  ret i32 0
}

attributes #0 = { nounwind }
