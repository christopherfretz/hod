	.text
	.file	"04-i128.ll"
	.globl	call_i128
	.p2align	4, 0x90
	.type	call_i128,@function
call_i128:
	.cfi_startproc
	subq	$72, %rsp
	.cfi_def_cfa_offset 80
	movq	%rsi, 64(%rsp)
	movq	%rdi, 56(%rsp)
	movq	%rsp, %rax
	movq	%rsi, 8(%rax)
	movq	%rdi, (%rax)
	movl	$1, %edi
	movq	%rdi, 24(%rsp)
	movl	$2, %esi
	movq	%rsi, 32(%rsp)
	movl	$3, %edx
	movq	%rdx, 40(%rsp)
	movl	$4, %ecx
	movq	%rcx, 48(%rsp)
	movl	$5, %r8d
	callq	s5@PLT
	movq	24(%rsp), %rdi
	movq	32(%rsp), %rsi
	movq	40(%rsp), %rdx
	movq	48(%rsp), %rcx
	movq	56(%rsp), %r8
	movq	64(%rsp), %r9
	movq	%rsp, %rax
	movq	$6, (%rax)
	callq	s4@PLT
	addq	$72, %rsp
	.cfi_def_cfa_offset 8
	retq
.Lfunc_end0:
	.size	call_i128, .Lfunc_end0-call_i128
	.cfi_endproc

	.globl	ret_i128
	.p2align	4, 0x90
	.type	ret_i128,@function
ret_i128:
	.cfi_startproc
	movq	%rcx, -8(%rsp)
	movq	%rdx, %rcx
	movq	-8(%rsp), %rdx
	movq	%rdi, %rax
	movq	%rax, %rdi
	imulq	%rdx, %rdi
	mulq	%rcx
	addq	%rdi, %rdx
	imulq	%rcx, %rsi
	addq	%rsi, %rdx
	retq
.Lfunc_end1:
	.size	ret_i128, .Lfunc_end1-ret_i128
	.cfi_endproc

	.globl	div_i128
	.p2align	4, 0x90
	.type	div_i128,@function
div_i128:
	.cfi_startproc
	pushq	%rax
	.cfi_def_cfa_offset 16
	callq	__divti3@PLT
	popq	%rcx
	.cfi_def_cfa_offset 8
	retq
.Lfunc_end2:
	.size	div_i128, .Lfunc_end2-div_i128
	.cfi_endproc

	.section	".note.GNU-stack","",@progbits
