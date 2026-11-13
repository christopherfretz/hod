	.text
	.file	"11-i1.ll"
	.globl	pass_trunc
	.p2align	4, 0x90
	.type	pass_trunc,@function
pass_trunc:
	.cfi_startproc
	pushq	%rax
	.cfi_def_cfa_offset 16
	callq	take_i1@PLT
	popq	%rax
	.cfi_def_cfa_offset 8
	retq
.Lfunc_end0:
	.size	pass_trunc, .Lfunc_end0-pass_trunc
	.cfi_endproc

	.globl	pass_trunc8
	.p2align	4, 0x90
	.type	pass_trunc8,@function
pass_trunc8:
	.cfi_startproc
	pushq	%rax
	.cfi_def_cfa_offset 16
	callq	take_i8@PLT
	popq	%rax
	.cfi_def_cfa_offset 8
	retq
.Lfunc_end1:
	.size	pass_trunc8, .Lfunc_end1-pass_trunc8
	.cfi_endproc

	.globl	recv_i1
	.p2align	4, 0x90
	.type	recv_i1,@function
recv_i1:
	.cfi_startproc
	movl	%edi, %eax
	andl	$1, %eax
	retq
.Lfunc_end2:
	.size	recv_i1, .Lfunc_end2-recv_i1
	.cfi_endproc

	.globl	use_give
	.p2align	4, 0x90
	.type	use_give,@function
use_give:
	.cfi_startproc
	pushq	%rax
	.cfi_def_cfa_offset 16
	callq	give_i1@PLT
	movzbl	%al, %eax
	andl	$1, %eax
	popq	%rcx
	.cfi_def_cfa_offset 8
	retq
.Lfunc_end3:
	.size	use_give, .Lfunc_end3-use_give
	.cfi_endproc

	.globl	ret_trunc
	.p2align	4, 0x90
	.type	ret_trunc,@function
ret_trunc:
	.cfi_startproc
	movb	%dil, %al
	retq
.Lfunc_end4:
	.size	ret_trunc, .Lfunc_end4-ret_trunc
	.cfi_endproc

	.section	".note.GNU-stack","",@progbits
