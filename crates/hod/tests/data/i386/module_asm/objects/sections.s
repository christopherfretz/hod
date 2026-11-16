# The x86-64 cases "sections", "stack" and "symbols" in 32-bit form:
# standard and custom sections, the section stack, `.previous`, symbol
# attributes, `.size`, `.type`.
.section .text.foo,"ax",@progbits
f:
ret
.section .data.rel.ro,"aw",@progbits
.quad 1
.section .rodata.foo,"a"
.long 2
.section .bss.foo,"aw",@nobits
.zero 4
.section .text
nop
.pushsection .data
.long 1
.pushsection .rodata
.long 5
.popsection
.long 6
.popsection
nop
.section .rodata
.long 2
.previous
nop
.data
.long 3
.previous
.long 4
.pushsection ".debug_gdb_scripts", "MS",%progbits,1
.ascii "\4gdb.inlined-script\n"
.byte 0
.popsection
.section mine,"a",@progbits
.long 7
.section notes,"",@note
.long 0
.text
.globl gf
.type gf, @function
gf:
ret
.size gf, .-gf
.weak w
.hidden w
w:
nop
ret
.size w, .-w
l:
nop
.data
.globl o
.type o, @object
.protected o
o:
.long 1
.size o, 4
lo:
.long 3
.type lo, @object
