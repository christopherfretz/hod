	.file	"13-frame.ll"
	.text
	.globl	fp
	.p2align	4
	.type	fp,@function
fp:
	.cfi_startproc
	pushl	%ebp
	.cfi_def_cfa_offset 8
	.cfi_offset %ebp, -8
	movl	%esp, %ebp
	.cfi_def_cfa_register %ebp
	subl	$8, %esp
	movl	%esp, %eax
	leal	-4(%ebp), %ecx
	movl	%ecx, (%eax)
	calll	use@PLT
	addl	$8, %esp
	popl	%ebp
	.cfi_def_cfa %esp, 4
	retl
.Lfunc_end0:
	.size	fp, .Lfunc_end0-fp
	.cfi_endproc

	.globl	dyn
	.p2align	4
	.type	dyn,@function
dyn:
	.cfi_startproc
	pushl	%ebp
	.cfi_def_cfa_offset 8
	.cfi_offset %ebp, -8
	movl	%esp, %ebp
	.cfi_def_cfa_register %ebp
	subl	$8, %esp
	movl	8(%ebp), %eax
	addl	$15, %eax
	andl	$-16, %eax
	movl	%esp, %ecx
	subl	%eax, %ecx
	movl	%ecx, %esp
	subl	$16, %esp
	movl	%esp, %eax
	movl	%ecx, (%eax)
	calll	use@PLT
	movl	%ebp, %esp
	popl	%ebp
	.cfi_def_cfa %esp, 4
	retl
.Lfunc_end1:
	.size	dyn, .Lfunc_end1-dyn
	.cfi_endproc

	.globl	over
	.p2align	4
	.type	over,@function
over:
	.cfi_startproc
	pushl	%ebp
	.cfi_def_cfa_offset 8
	.cfi_offset %ebp, -8
	movl	%esp, %ebp
	.cfi_def_cfa_register %ebp
	andl	$-32, %esp
	subl	$64, %esp
	movl	%esp, %eax
	leal	32(%esp), %ecx
	movl	%ecx, (%eax)
	calll	use@PLT
	movl	%ebp, %esp
	popl	%ebp
	.cfi_def_cfa %esp, 4
	retl
.Lfunc_end2:
	.size	over, .Lfunc_end2-over
	.cfi_endproc

	.globl	big
	.p2align	4
	.type	big,@function
big:
	.cfi_startproc
	pushl	%ebp
	.cfi_def_cfa_offset 8
	.cfi_offset %ebp, -8
	movl	%esp, %ebp
	.cfi_def_cfa_register %ebp
	subl	$8200, %esp
	movl	%esp, %eax
	leal	-8192(%ebp), %ecx
	movl	%ecx, (%eax)
	calll	use@PLT
	addl	$8200, %esp
	popl	%ebp
	.cfi_def_cfa %esp, 4
	retl
.Lfunc_end3:
	.size	big, .Lfunc_end3-big
	.cfi_endproc

	.globl	realign
	.p2align	4
	.type	realign,@function
realign:
	.cfi_startproc
	pushl	%ebp
	.cfi_def_cfa_offset 8
	.cfi_offset %ebp, -8
	movl	%esp, %ebp
	.cfi_def_cfa_register %ebp
	andl	$-16, %esp
	subl	$16, %esp
	movl	%esp, %eax
	leal	4(%esp), %ecx
	movl	%ecx, (%eax)
	calll	use@PLT
	movl	%ebp, %esp
	popl	%ebp
	.cfi_def_cfa %esp, 4
	retl
.Lfunc_end4:
	.size	realign, .Lfunc_end4-realign
	.cfi_endproc

	.section	".note.GNU-stack","",@progbits
