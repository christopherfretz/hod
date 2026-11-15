	.text
	.file	"02-fp.ll"
	.globl	rf
	.p2align	4, 0x90
	.type	rf,@function
rf:
	.cfi_startproc
	pushl	%eax
	.cfi_def_cfa_offset 8
	flds	8(%esp)
	fldl	12(%esp)
	fstps	(%esp)
	flds	(%esp)
	faddp	%st, %st(1)
	popl	%eax
	.cfi_def_cfa_offset 4
	retl
.Lfunc_end0:
	.size	rf, .Lfunc_end0-rf
	.cfi_endproc

	.globl	rd
	.p2align	4, 0x90
	.type	rd,@function
rd:
	.cfi_startproc
	fldl	4(%esp)
	fadd	%st, %st(0)
	retl
.Lfunc_end1:
	.size	rd, .Lfunc_end1-rd
	.cfi_endproc

	.globl	rx
	.p2align	4, 0x90
	.type	rx,@function
rx:
	.cfi_startproc
	fldt	4(%esp)
	retl
.Lfunc_end2:
	.size	rx, .Lfunc_end2-rx
	.cfi_endproc

	.globl	calld
	.p2align	4, 0x90
	.type	calld,@function
calld:
	.cfi_startproc
	subl	$28, %esp
	.cfi_def_cfa_offset 32
	movl	%esp, %eax
	fld1
	fstpt	12(%eax)
	movl	$1073741824, 8(%eax)
	movl	$0, 4(%eax)
	movl	$1065353216, (%eax)
	calll	extd@PLT
	addl	$28, %esp
	.cfi_def_cfa_offset 4
	retl
.Lfunc_end3:
	.size	calld, .Lfunc_end3-calld
	.cfi_endproc

	.globl	calldrop
	.p2align	4, 0x90
	.type	calldrop,@function
calldrop:
	.cfi_startproc
	subl	$28, %esp
	.cfi_def_cfa_offset 32
	movl	%esp, %eax
	fld1
	fstpt	12(%eax)
	movl	$1073741824, 8(%eax)
	movl	$0, 4(%eax)
	movl	$1065353216, (%eax)
	calll	extd@PLT
	fstp	%st(0)
	addl	$28, %esp
	.cfi_def_cfa_offset 4
	retl
.Lfunc_end4:
	.size	calldrop, .Lfunc_end4-calldrop
	.cfi_endproc

	.globl	rq
	.p2align	4, 0x90
	.type	rq,@function
rq:
	.cfi_startproc
	pushl	%ebx
	.cfi_def_cfa_offset 8
	pushl	%edi
	.cfi_def_cfa_offset 12
	pushl	%esi
	.cfi_def_cfa_offset 16
	.cfi_offset %esi, -16
	.cfi_offset %edi, -12
	.cfi_offset %ebx, -8
	movl	16(%esp), %ecx
	movl	%ecx, %eax
	movl	20(%esp), %edx
	movl	24(%esp), %esi
	movl	28(%esp), %edi
	movl	32(%esp), %ebx
	movl	%ebx, 12(%ecx)
	movl	%edi, 8(%ecx)
	movl	%esi, 4(%ecx)
	movl	%edx, (%ecx)
	popl	%esi
	.cfi_def_cfa_offset 12
	popl	%edi
	.cfi_def_cfa_offset 8
	popl	%ebx
	.cfi_def_cfa_offset 4
	retl	$4
.Lfunc_end5:
	.size	rq, .Lfunc_end5-rq
	.cfi_endproc

	.globl	rh
	.p2align	4, 0x90
	.type	rh,@function
rh:
	.cfi_startproc
	movw	4(%esp), %ax
	retl
.Lfunc_end6:
	.size	rh, .Lfunc_end6-rh
	.cfi_endproc

	.section	".note.GNU-stack","",@progbits
