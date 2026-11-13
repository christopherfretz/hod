	.text
	.file	"05-aggr.ll"
	.globl	call_aggr
	.p2align	4, 0x90
	.type	call_aggr,@function
call_aggr:
	.cfi_startproc
	pushq	%rax
	.cfi_def_cfa_offset 16
	callq	sa@PLT
	popq	%rax
	.cfi_def_cfa_offset 8
	retq
.Lfunc_end0:
	.size	call_aggr, .Lfunc_end0-call_aggr
	.cfi_endproc

	.globl	ret3
	.p2align	4, 0x90
	.type	ret3,@function
ret3:
	.cfi_startproc
	movq	%rdi, %rcx
	retq
.Lfunc_end1:
	.size	ret3, .Lfunc_end1-ret3
	.cfi_endproc

	.globl	ret4
	.p2align	4, 0x90
	.type	ret4,@function
ret4:
	.cfi_startproc
	movq	%rdi, %rax
	movq	%rsi, 24(%rdi)
	retq
.Lfunc_end2:
	.size	ret4, .Lfunc_end2-ret4
	.cfi_endproc

	.globl	ret3x32
	.p2align	4, 0x90
	.type	ret3x32,@function
ret3x32:
	.cfi_startproc
	movl	%edi, %ecx
	retq
.Lfunc_end3:
	.size	ret3x32, .Lfunc_end3-ret3x32
	.cfi_endproc

	.globl	ret_lp
	.p2align	4, 0x90
	.type	ret_lp,@function
ret_lp:
	.cfi_startproc
	movl	%esi, %edx
	movq	%rdi, %rax
	retq
.Lfunc_end4:
	.size	ret_lp, .Lfunc_end4-ret_lp
	.cfi_endproc

	.globl	call_spill
	.p2align	4, 0x90
	.type	call_spill,@function
call_spill:
	.cfi_startproc
	subq	$24, %rsp
	.cfi_def_cfa_offset 32
	movq	%rsi, %r9
	movq	%rdi, %r8
	movq	%rsp, %rax
	movq	%r9, 8(%rax)
	movq	%r8, (%rax)
	movq	%r8, %rdi
	movq	%r9, %rsi
	movq	%r8, %rdx
	movq	%r9, %rcx
	callq	s7@PLT
	addq	$24, %rsp
	.cfi_def_cfa_offset 8
	retq
.Lfunc_end5:
	.size	call_spill, .Lfunc_end5-call_spill
	.cfi_endproc

	.section	".note.GNU-stack","",@progbits
