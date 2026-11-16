; WP-307: module-level assembly of position-independent i686 code, built
; into a PIE and into a shared library. The text finds the GOT itself
; (a thunk, then `$_GLOBAL_OFFSET_TABLE_`: R_386_GOTPC), reads IR
; variables through the GOT (R_386_GOT32X and R_386_GOT32), its own
; data through @GOTOFF, calls an IR function through the PLT, and
; reaches thread-locals in the dynamic models and initial exec (@tlsgd,
; @tlsldm with @dtpoff, @gotntpoff; @gottpoff is `tls32.ll`'s).
target triple = "i686-unknown-linux-gnu"

module asm ".text"
module asm "asm_thunk:"
module asm "  movl (%esp), %ebx"
module asm "  ret"
module asm ".globl asm_pic"
module asm ".type asm_pic, @function"
module asm "asm_pic:"
module asm "asm_pic_body:"
module asm "  pushl %ebx"
module asm "  pushl %esi"
module asm "  call asm_thunk"
module asm "  addl $_GLOBAL_OFFSET_TABLE_, %ebx"
module asm "  movl ir_value@GOT(%ebx), %eax"
module asm "  movl (%eax), %esi"
module asm "  movl $0, %eax"
module asm "  addl ir_value@GOT(%ebx), %eax"
module asm "  addl (%eax), %esi"
module asm "  leal pic_local@GOTOFF(%ebx), %eax"
module asm "  addl 4(%eax), %esi"
module asm "  addl pic_hidden@GOTOFF(%ebx), %esi"
module asm "  pushl $5"
module asm "  call ir_twice@PLT"
module asm "  addl $4, %esp"
module asm "  addl %eax, %esi"
module asm "  movl %esi, %eax"
module asm "  popl %esi"
module asm "  popl %ebx"
module asm "  ret"
module asm ".size asm_pic, .-asm_pic"
module asm ".globl asm_pic_tls"
module asm ".type asm_pic_tls, @function"
module asm "asm_pic_tls:"
module asm "  pushl %ebx"
module asm "  pushl %esi"
module asm "  call asm_thunk"
module asm "  addl $_GLOBAL_OFFSET_TABLE_, %ebx"
module asm "  leal tls_v@tlsgd(,%ebx,1), %eax"
module asm "  call ___tls_get_addr@PLT"
module asm "  movl (%eax), %esi"
module asm "  leal tls_h@tlsldm(%ebx), %eax"
module asm "  call ___tls_get_addr@PLT"
module asm "  addl tls_h@dtpoff(%eax), %esi"
module asm "  movl tls_v@gotntpoff(%ebx), %eax"
module asm "  addl %gs:(%eax), %esi"
module asm "  movl %esi, %eax"
module asm "  popl %esi"
module asm "  popl %ebx"
module asm "  ret"
module asm ".globl asm_pic_call"
module asm ".type asm_pic_call, @function"
module asm "asm_pic_call:"
module asm "  jmp asm_pic_body"
module asm ".data"
module asm "pic_local: .long 1000, 3000"
module asm ".globl pic_hidden"
module asm ".hidden pic_hidden"
module asm "pic_hidden: .long 20000"
module asm ".globl asm_pic_ptr"
module asm "asm_pic_ptr: .long ir_value"

@ir_value = global i32 100
@tls_v = thread_local global i32 7
@tls_h = hidden thread_local global i32 11
@asm_pic_ptr = external global ptr
@fmt = private constant [16 x i8] c"%d %d %d %d %d\0A\00"

declare i32 @asm_pic()
declare i32 @asm_pic_tls()
declare i32 @asm_pic_call()
declare i32 @printf(ptr, ...)

define i32 @ir_twice(i32 %x) #0 {
  %r = mul i32 %x, 2
  ret i32 %r
}

define i32 @prog_main() #0 {
  %a = call i32 @asm_pic()
  %b = call i32 @asm_pic_tls()
  %p = load ptr, ptr @asm_pic_ptr
  %c = load i32, ptr %p
  store i32 9, ptr @tls_v
  %h = load i32, ptr @tls_h
  %h2 = add i32 %h, 1
  store i32 %h2, ptr @tls_h
  %d = call i32 @asm_pic_tls()
  %e = call i32 @asm_pic_call()
  %r = call i32 (ptr, ...) @printf(ptr @fmt, i32 %a, i32 %b, i32 %c, i32 %d, i32 %e)
  ret i32 0
}

attributes #0 = { nounwind }
