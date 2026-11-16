# The text meets the module's symbols (the x86-64 cases
# "declaration-defined", "directives-on-declaration" and
# "calls-module-function" in 32-bit form): it defines declarations,
# gives directives to declarations and definitions, refers to module
# variables and thread-locals.
.text
.globl f
.type f, @function
f:
  movl V, %eax
  movl $W, %ecx
  movl %gs:T@ntpoff, %edx
  call U
  ret
.weak g
g:
  ret
.data
.globl v
v:
  .long 1
  .long V
  .long H
.weak U
.hidden W
.protected P
