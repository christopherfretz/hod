	.text
	.file	"01-int.ll"
	.globl	call_ints
	.p2align	4, 0x90
	.type	call_ints,@function
call_ints:
	.cfi_startproc
	subq	$24, %rsp
	.cfi_def_cfa_offset 32
	movq	%rsp, %rax
	movq	$-1, 8(%rax)
	movl	$255, (%rax)
	movl	$255, %esi
	movl	$65535, %ecx
	movl	$1, %r8d
	movl	$-1, %r9d
	movl	%r9d, %edi
	movl	%r9d, %edx
	callq	sink8@PLT
	addq	$24, %rsp
	.cfi_def_cfa_offset 8
	retq
.Lfunc_end0:
	.size	call_ints, .Lfunc_end0-call_ints
	.cfi_endproc

	.globl	recv
	.p2align	4, 0x90
	.type	recv,@function
recv:
	.cfi_startproc
	movslq	16(%rsp), %rcx
	movsbq	8(%rsp), %rax
	addq	%rcx, %rax
	retq
.Lfunc_end1:
	.size	recv, .Lfunc_end1-recv
	.cfi_endproc

	.globl	ret_s8
	.p2align	4, 0x90
	.type	ret_s8,@function
ret_s8:
	.cfi_startproc
	movb	%dil, %al
	retq
.Lfunc_end2:
	.size	ret_s8, .Lfunc_end2-ret_s8
	.cfi_endproc

	.globl	ret_bool
	.p2align	4, 0x90
	.type	ret_bool,@function
ret_bool:
	.cfi_startproc
	movb	%dil, %al
	andb	$1, %al
	retq
.Lfunc_end3:
	.size	ret_bool, .Lfunc_end3-ret_bool
	.cfi_endproc

	.globl	ret_i1_plain
	.p2align	4, 0x90
	.type	ret_i1_plain,@function
ret_i1_plain:
	.cfi_startproc
	movb	%dil, %al
	retq
.Lfunc_end4:
	.size	ret_i1_plain, .Lfunc_end4-ret_i1_plain
	.cfi_endproc

	.globl	ret_i16_plain
	.p2align	4, 0x90
	.type	ret_i16_plain,@function
ret_i16_plain:
	.cfi_startproc
	movw	%di, %ax
	retq
.Lfunc_end5:
	.size	ret_i16_plain, .Lfunc_end5-ret_i16_plain
	.cfi_endproc

	.globl	use_ret8
	.p2align	4, 0x90
	.type	use_ret8,@function
use_ret8:
	.cfi_startproc
	pushq	%rax
	.cfi_def_cfa_offset 16
	callq	get8@PLT
	movzbl	%al, %eax
	popq	%rcx
	.cfi_def_cfa_offset 8
	retq
.Lfunc_end6:
	.size	use_ret8, .Lfunc_end6-use_ret8
	.cfi_endproc

	.section	".note.GNU-stack","",@progbits
