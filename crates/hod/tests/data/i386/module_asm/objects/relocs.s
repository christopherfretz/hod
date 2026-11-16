# Every relocation kind llvm-mc writes for i686 from module assembly,
# against undefined, global and local symbols, with addends in place
# (the x86-64 case "references" in 32-bit form).
.text
.globl g
loc:
  movl $sym, %eax
  movl $sym+4, %eax
  pushl $sym-8
  addl $sym, %ecx
  movl sym, %eax
  movl sym+8, %ecx
  movl %eax, sym+12
  movl 4+sym(,%ecx,4), %edx
  movl $loc, %eax
  movl $dloc+4, %eax
  movl dloc, %eax
  movl dloc+8(%ebx), %eax
  movl sym@GOT(%ebx), %eax
  movl sym@GOT+4(%ebx), %eax
  movl %gs:sym@GOT(%ebx), %eax
  movl sym@GOT(%ebp), %esp
  movl sym@GOT, %eax
  leal sym@GOT(%ebx), %eax
  pushl sym@GOT(%ebx)
  addl sym@GOT(%ebx), %eax
  testl %eax, sym@GOT(%ebx)
  movl sym@GOT(%ebx,%ecx,1), %eax
  movl sym@GOT(%esp), %eax
  movl $sym@GOT, %eax
  movl loc@GOT(%ebx), %eax
  movl g@GOT(%ebx), %eax
  movl sym@GOTOFF(%ebx), %eax
  leal sym@gotoff+4(%ebx), %eax
  leal loc@GOTOFF(%ebx), %eax
  leal dloc@GOTOFF+2(%ebx), %eax
  movl $sym@GOTOFF, %eax
  addl $_GLOBAL_OFFSET_TABLE_, %ebx
  movl $_GLOBAL_OFFSET_TABLE_, %eax
  pushl $_GLOBAL_OFFSET_TABLE_+8
  leal _GLOBAL_OFFSET_TABLE_(%ebx), %eax
  movl %gs:0, %eax
  movl %gs:t@ntpoff, %eax
  movl %gs:t@ntpoff+8, %eax
  movl $t@ntpoff+4, %eax
  movl $t@tpoff, %eax
  movl %gs:t@tpoff, %eax
  movl t@gotntpoff(%ebx), %eax
  movl t@gotntpoff, %eax
  movl t@indntpoff, %eax
  movl t@indntpoff, %ecx
  movl t@gottpoff(%ebx), %eax
  leal t@tlsgd(,%ebx,1), %eax
  leal t@tlsldm(%ebx), %eax
  leal t@dtpoff(%eax), %edx
  leal t@dtpoff+4(%eax), %edx
  movl $t@dtpoff, %edx
  call ___tls_get_addr@PLT
  call g
  call g@PLT
  call ext+4
  jmp g
  jmp ext@PLT
  je ext
  call loc
  call dloc
  jmp .Lend
  fstcw sym
  lock
  addl $sym, (%ecx)
g:
.Lend:
  ret
.data
dloc:
  .long 0
  .short sym+2
  .short dloc
  .long loc
  .long sym
  .long sym+3
  .long g
  .long ext+8
  .long dloc
  .long loc-4
  .long _GLOBAL_OFFSET_TABLE_
.section .rodata.x,"a"
  .long dloc+4
  .long loc+16
