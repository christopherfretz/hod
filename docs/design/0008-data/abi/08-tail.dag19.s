	.text
	.file	"08-tail.ll"
	.globl	tail
	.p2align	4, 0x90
	.type	tail,@function
tail:
	.cfi_startproc
	jmp	t1@PLT
.Lfunc_end0:
	.size	tail, .Lfunc_end0-tail
	.cfi_endproc

	.globl	must
	.p2align	4, 0x90
	.type	must,@function
must:
	.cfi_startproc
	movl	4(%esp), %eax
	movl	%eax, 4(%esp)
	jmp	t1@PLT
.Lfunc_end1:
	.size	must, .Lfunc_end1-must
	.cfi_endproc

	.globl	mustfwd
	.p2align	4, 0x90
	.type	mustfwd,@function
mustfwd:
	.cfi_startproc
	movl	8(%esp), %eax
	movl	4(%esp), %ecx
	movl	%ecx, 8(%esp)
	movl	%eax, 4(%esp)
	jmp	t2@PLT
.Lfunc_end2:
	.size	mustfwd, .Lfunc_end2-mustfwd
	.cfi_endproc

	.globl	cn
	.p2align	4, 0x90
	.type	cn,@function
cn:
	.cfi_startproc
	subl	$12, %esp
	.cfi_def_cfa_offset 16
	movl	16(%esp), %ecx
	calll	nested@PLT
	addl	$12, %esp
	.cfi_def_cfa_offset 4
	retl
.Lfunc_end3:
	.size	cn, .Lfunc_end3-cn
	.cfi_endproc

	.globl	isnest
	.p2align	4, 0x90
	.type	isnest,@function
isnest:
	.cfi_startproc
	movl	4(%esp), %eax
	retl
.Lfunc_end4:
	.size	isnest, .Lfunc_end4-isnest
	.cfi_endproc

	.section	".note.GNU-stack","",@progbits
