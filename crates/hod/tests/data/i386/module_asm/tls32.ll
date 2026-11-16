; WP-307: the thread-pointer offsets of the other sign, `@tpoff`
; (R_386_TLS_LE_32) and `@gottpoff` (R_386_TLS_IE_32), in module
; assembly of a PIE. mold links neither, lld not the second ("unknown
; relocation", for llc's object as for hod's): the run test links this
; program by GNU ld only.
target triple = "i686-unknown-linux-gnu"

module asm ".text"
module asm ".globl asm_tls32"
module asm ".type asm_tls32, @function"
module asm "asm_tls32:"
module asm "  pushl %ebx"
module asm "  call 1f"
module asm "1:"
module asm "  popl %ebx"
module asm "  addl $_GLOBAL_OFFSET_TABLE_+1, %ebx"
module asm "  movl %gs:0, %ecx"
module asm "  subl $tls_v@tpoff, %ecx"
module asm "  movl (%ecx), %eax"
module asm "  movl tls_v@gottpoff(%ebx), %edx"
module asm "  movl %gs:0, %ecx"
module asm "  subl %edx, %ecx"
module asm "  addl (%ecx), %eax"
module asm "  movl %gs:0, %ecx"
module asm "  subl $tls_v@tpoff+4, %ecx"
module asm "  addl (%ecx), %eax"
module asm "  popl %ebx"
module asm "  ret"

@tls_v = thread_local global [2 x i32] [i32 7, i32 30]
@fmt = private constant [7 x i8] c"%d %d\0A\00"

declare i32 @asm_tls32()
declare i32 @printf(ptr, ...)

define i32 @prog_main() #0 {
  %a = call i32 @asm_tls32()
  store i32 9, ptr @tls_v
  %b = call i32 @asm_tls32()
  %r = call i32 (ptr, ...) @printf(ptr @fmt, i32 %a, i32 %b)
  ret i32 0
}

attributes #0 = { nounwind }
