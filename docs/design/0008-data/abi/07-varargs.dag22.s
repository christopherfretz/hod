	.file	"07-varargs.ll"
	.text
	.globl	va
	.p2align	4
	.type	va,@function
va:
	.cfi_startproc
	pushl	%eax
	.cfi_def_cfa_offset 8
	leal	12(%esp), %eax
	movl	%eax, (%esp)
	movl	(%esp), %eax
	addl	$3, %eax
	andl	$-4, %eax
	movl	%eax, %ecx
	addl	$4, %ecx
	movl	%ecx, (%esp)
	movl	(%eax), %eax
	movl	(%esp), %ecx
	addl	$3, %ecx
	andl	$-4, %ecx
	addl	$8, %ecx
	movl	%ecx, (%esp)
	popl	%ecx
	.cfi_def_cfa_offset 4
	retl
.Lfunc_end0:
	.size	va, .Lfunc_end0-va
	.cfi_endproc

	.globl	cva
	.p2align	4
	.type	cva,@function
cva:
	.cfi_startproc
	subl	$44, %esp
	.cfi_def_cfa_offset 48
	movl	%esp, %eax
	fld1
	fstpt	24(%eax)
	movl	$1073741824, 12(%eax)
	movl	$0, 8(%eax)
	movl	$0, 20(%eax)
	movl	$3, 16(%eax)
	movl	$1, 4(%eax)
	movl	$.Lfmt, (%eax)
	calll	printf@PLT
	addl	$44, %esp
	.cfi_def_cfa_offset 4
	retl
.Lfunc_end1:
	.size	cva, .Lfunc_end1-cva
	.cfi_endproc

	.type	.Lfmt,@object
	.section	.rodata,"a",@progbits
.Lfmt:
	.asciz	"%d\n"
	.size	.Lfmt, 4

	.section	".note.GNU-stack","",@progbits
