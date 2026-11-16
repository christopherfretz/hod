# The x86-64 cases "integers", "space", "data-align", "set" and
# "rodata-value" (hod_cg/tests/emit_module_asm.rs) in one text: data
# directives, fills, alignment, constants and strings.
.data
.byte 1, 2, 255, -1
.short -2
.word 0x1234
.value 9
.2byte 7
.long -3
.int 9
.4byte 10
.quad 0x123456789abcdef0
.8byte 11
.byte 1
.zero 3
.space 5
.space 3, 0xaa
.skip 2
.skip 2, 7
.p2align 3
.byte 2
.balign 16
.byte 3
.align 4
.byte 4
.balign 8, 0xcc
.byte 5
.p2align 2, 0x11
.ascii "ab\tc\n", "\x41\101"
.asciz "zero", "terminated"
.string "s"
.set k, 42
.globl k
.equ k2, 7
k3 = -1
.long k
.quad k2 + 1
.byte k3
.pushsection .rodata
.globl kiln_ext_asm_value
kiln_ext_asm_value: .long 42
.popsection
.bss
.zero 12
.p2align 4
.space 4
.ident "first tool 1.0"
.ident "second\twith \x41 kept"
