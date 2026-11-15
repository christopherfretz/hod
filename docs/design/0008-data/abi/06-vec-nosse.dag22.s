	.file	"06-vec-nosse.ll"
	.text
	.globl	v4f
	.p2align	4
	.type	v4f,@function
v4f:
	.cfi_startproc
	movl	4(%esp), %ecx
	movl	%ecx, %eax
	flds	56(%esp)
	flds	60(%esp)
	flds	64(%esp)
	flds	68(%esp)
	fstps	12(%ecx)
	fstps	8(%ecx)
	fstps	4(%ecx)
	fstps	(%ecx)
	retl	$4
.Lfunc_end0:
	.size	v4f, .Lfunc_end0-v4f
	.cfi_endproc

	.globl	v2i
	.p2align	4
	.type	v2i,@function
v2i:
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
.Lfunc_end1:
	.size	v2i, .Lfunc_end1-v2i
	.cfi_endproc

	.globl	v2s
	.p2align	4
	.type	v2s,@function
v2s:
	.cfi_startproc
	movl	4(%esp), %eax
	movl	8(%esp), %edx
	retl
.Lfunc_end2:
	.size	v2s, .Lfunc_end2-v2s
	.cfi_endproc

	.globl	v8f
	.p2align	4
	.type	v8f,@function
v8f:
	.cfi_startproc
	pushl	%eax
	.cfi_def_cfa_offset 8
	movl	8(%esp), %ecx
	movl	%ecx, %eax
	flds	12(%esp)
	fstps	(%esp)
	flds	16(%esp)
	flds	20(%esp)
	flds	24(%esp)
	flds	28(%esp)
	flds	32(%esp)
	flds	36(%esp)
	flds	40(%esp)
	fstps	28(%ecx)
	flds	(%esp)
	fxch	%st(1)
	fstps	24(%ecx)
	fxch	%st(1)
	fstps	20(%ecx)
	fxch	%st(1)
	fstps	16(%ecx)
	fxch	%st(1)
	fstps	12(%ecx)
	fxch	%st(1)
	fstps	8(%ecx)
	fxch	%st(1)
	fstps	4(%ecx)
	fstps	(%ecx)
	popl	%ecx
	.cfi_def_cfa_offset 4
	retl	$4
.Lfunc_end3:
	.size	v8f, .Lfunc_end3-v8f
	.cfi_endproc

	.globl	cv
	.p2align	4
	.type	cv,@function
cv:
	.cfi_startproc
	subl	$108, %esp
	.cfi_def_cfa_offset 112
	movl	112(%esp), %eax
	movl	%eax, 72(%esp)
	movl	%eax, 76(%esp)
	flds	116(%esp)
	flds	120(%esp)
	flds	124(%esp)
	flds	128(%esp)
	movl	%esp, %eax
	fsts	68(%eax)
	fxch	%st(1)
	fsts	64(%eax)
	fxch	%st(2)
	fsts	60(%eax)
	fxch	%st(3)
	fsts	56(%eax)
	fxch	%st(1)
	fsts	52(%eax)
	fxch	%st(2)
	fsts	48(%eax)
	fxch	%st(3)
	fsts	44(%eax)
	fxch	%st(1)
	fsts	40(%eax)
	fxch	%st(2)
	fsts	36(%eax)
	fxch	%st(3)
	fsts	32(%eax)
	fxch	%st(1)
	fsts	28(%eax)
	fxch	%st(2)
	fsts	24(%eax)
	fxch	%st(3)
	fstps	20(%eax)
	fstps	16(%eax)
	fstps	12(%eax)
	fstps	8(%eax)
	leal	80(%esp), %ecx
	movl	%ecx, (%eax)
	movl	$1, 4(%eax)
	calll	ev@PLT
	subl	$4, %esp
	movl	72(%esp), %ecx
	movl	76(%esp), %eax
	flds	80(%esp)
	flds	84(%esp)
	flds	88(%esp)
	flds	92(%esp)
	fstps	12(%ecx)
	fstps	8(%ecx)
	fstps	4(%ecx)
	fstps	(%ecx)
	addl	$108, %esp
	.cfi_def_cfa_offset 4
	retl	$4
.Lfunc_end4:
	.size	cv, .Lfunc_end4-cv
	.cfi_endproc

	.globl	fd
	.p2align	4
	.type	fd,@function
fd:
	.cfi_startproc
	fldl	12(%esp)
	fldl	4(%esp)
	fmulp	%st, %st(1)
	retl
.Lfunc_end5:
	.size	fd, .Lfunc_end5-fd
	.cfi_endproc

	.globl	ff
	.p2align	4
	.type	ff,@function
ff:
	.cfi_startproc
	flds	4(%esp)
	retl
.Lfunc_end6:
	.size	ff, .Lfunc_end6-ff
	.cfi_endproc

	.globl	fh
	.p2align	4
	.type	fh,@function
fh:
	.cfi_startproc
	movw	4(%esp), %ax
	retl
.Lfunc_end7:
	.size	fh, .Lfunc_end7-fh
	.cfi_endproc

	.section	".note.GNU-stack","",@progbits
