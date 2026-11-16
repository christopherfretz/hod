# Instructions in 32-bit mode, branches and their relaxation, numeric
# labels, padding with one-byte nops (the x86-64 cases "code-calls",
# "code-align", "relaxation", "numeric" in 32-bit form).
.text
.p2align 4
.globl asm_add
.type asm_add, @function
asm_add:
  movl 4(%esp), %eax
  addl 8(%esp), %eax
  ret
.size asm_add, .-asm_add
.globl asm_call
asm_call:
  pushl %ebp
  movl %esp, %ebp
  pushl %ebx
  pushl %esi
  pushl $3
  call dump
  call dump@PLT
  call dump+0
  addl $4, %esp
  leal -8(%ebp,%ecx,4), %edx
  movzbl (%edx), %eax
  movsbl %al, %ecx
  imull $12, %ecx, %ecx
  shll $3, %eax
  sarl %cl, %eax
  xorl %edx, %edx
  cltd
  idivl %ecx
  testl %eax, %eax
  sete %al
  cmovnel %ecx, %eax
  lock xaddl %eax, (%ecx)
  rep movsb
  incl %eax
  decl %ecx
  negl %eax
  notl %eax
  popl %esi
  popl %ebx
  leave
  ret
nop
.p2align 4
ret
.balign 8
nop
.p2align 5
nop
.p2align 3, 0x11
nop
.align 64
ret
1:
jmp 2f
jne 1b
.zero 130
2:
jmp 1b
je 1b
jmp 3f
3:
ret
1:
nop
jmp 1b
jmp 1f
1:
ret
jae 1b
