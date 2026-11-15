movl %eax, (%ecx)
movl 8(%esp), %eax
movl %ebx, -4(%ebp)
addl $1000, %eax
incl %eax
decl %ecx
movl 0x1234, %eax
movl %gs:0, %eax
movl %gs:(%eax), %eax
leal 4(%eax,%ecx,4), %edx
movw %ax, (%ecx)
movb %al, %ah
movzbl %al, %eax
movsbl %cl, %edx
pushl %ebx
popl %esi
pushl $5
calll *%eax
retl $4
cmpxchg8b (%esi)
movsd 8(%esp), %xmm0
fldl 4(%esp)
fistpll (%esp)
cltd
idivl %ecx
shldl %cl, %eax, %edx
jmp *(%eax)
movl $0x12345678, %eax
