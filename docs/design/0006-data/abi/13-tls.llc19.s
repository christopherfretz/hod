	.text
	.file	"13-tls.ll"
	.globl	get
	.p2align	4, 0x90
	.type	get,@function
get:
	.cfi_startproc
	movq	ext@GOTTPOFF(%rip), %rax
	movq	ie@GOTTPOFF(%rip), %rcx
	movl	%fs:(%rax), %eax
	movl	%fs:loc@TPOFF, %esi
	movl	%fs:(%rcx), %ecx
	movl	%fs:ld@TPOFF, %edx
	addl	%esi, %eax
	addl	%edx, %ecx
	addl	%ecx, %eax
	retq
.Lfunc_end0:
	.size	get, .Lfunc_end0-get
	.cfi_endproc

	.type	loc,@object
	.section	.tbss,"awT",@nobits
	.globl	loc
	.p2align	2, 0x0
loc:
	.long	0
	.size	loc, 4

	.type	ld,@object
	.p2align	2, 0x0
ld:
	.long	0
	.size	ld, 4

	.section	".note.GNU-stack","",@progbits
