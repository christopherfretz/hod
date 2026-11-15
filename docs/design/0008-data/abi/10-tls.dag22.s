	.file	"10-tls.ll"
	.text
	.globl	t
	.p2align	4
	.type	t,@function
t:
	.cfi_startproc
	pushl	%esi
	.cfi_def_cfa_offset 8
	.cfi_offset %esi, -8
	movl	gd@INDNTPOFF, %eax
	movl	%gs:(%eax), %eax
	movl	%gs:ld@NTPOFF, %esi
	movl	ie@INDNTPOFF, %ecx
	movl	%gs:(%ecx), %ecx
	movl	%gs:le@NTPOFF, %edx
	addl	%esi, %eax
	addl	%edx, %ecx
	addl	%ecx, %eax
	popl	%esi
	.cfi_def_cfa_offset 4
	retl
.Lfunc_end0:
	.size	t, .Lfunc_end0-t
	.cfi_endproc

	.type	ld,@object
	.section	.tbss,"awT",@nobits
	.p2align	2, 0x0
ld:
	.long	0
	.size	ld, 4

	.type	le,@object
	.globl	le
	.p2align	2, 0x0
le:
	.long	0
	.size	le, 4

	.section	".note.GNU-stack","",@progbits
