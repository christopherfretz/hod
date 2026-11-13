	.file	"14-frame.ll"
	.text
	.globl	f
	.p2align	4
	.type	f,@function
f:
	.cfi_startproc
	pushq	%rbp
	.cfi_def_cfa_offset 16
	.cfi_offset %rbp, -16
	movq	%rsp, %rbp
	.cfi_def_cfa_register %rbp
	pushq	%r12
	pushq	%rbx
	subq	$112, %rsp
	.cfi_offset %rbx, -32
	.cfi_offset %r12, -24
	movq	%rdi, -128(%rbp)
	#APP
	#NO_APP
	leaq	-116(%rbp), %rdi
	callq	g@PLT
	movq	-128(%rbp), %rax
	addq	$112, %rsp
	popq	%rbx
	popq	%r12
	popq	%rbp
	.cfi_def_cfa %rsp, 8
	retq
.Lfunc_end0:
	.size	f, .Lfunc_end0-f
	.cfi_endproc

	.section	".note.GNU-stack","",@progbits
