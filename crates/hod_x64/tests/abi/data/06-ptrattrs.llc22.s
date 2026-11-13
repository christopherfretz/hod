	.file	"06-ptrattrs.ll"
	.text
	.globl	call_sret
	.p2align	4
	.type	call_sret,@function
call_sret:
	.cfi_startproc
	pushq	%rax
	.cfi_def_cfa_offset 16
	movl	$7, %esi
	callq	f_sret@PLT
	popq	%rax
	.cfi_def_cfa_offset 8
	retq
.Lfunc_end0:
	.size	call_sret, .Lfunc_end0-call_sret
	.cfi_endproc

	.globl	def_sret
	.p2align	4
	.type	def_sret,@function
def_sret:
	.cfi_startproc
	movq	%rdi, %rax
	movq	%rsi, (%rdi)
	retq
.Lfunc_end1:
	.size	def_sret, .Lfunc_end1-def_sret
	.cfi_endproc

	.globl	call_byval
	.p2align	4
	.type	call_byval,@function
call_byval:
	.cfi_startproc
	subq	$72, %rsp
	.cfi_def_cfa_offset 80
	movq	16(%rdi), %rcx
	movq	%rsp, %rax
	movq	%rcx, 48(%rax)
	movaps	(%rdi), %xmm0
	movaps	%xmm0, 32(%rax)
	movq	%rcx, 16(%rax)
	movq	(%rdi), %rcx
	movq	8(%rdi), %rdx
	movq	%rdx, 8(%rax)
	movq	%rcx, (%rax)
	movl	$1, %edi
	callq	f_byval@PLT
	addq	$72, %rsp
	.cfi_def_cfa_offset 8
	retq
.Lfunc_end2:
	.size	call_byval, .Lfunc_end2-call_byval
	.cfi_endproc

	.globl	def_byval
	.p2align	4
	.type	def_byval,@function
def_byval:
	.cfi_startproc
	movq	8(%rsp), %rax
	retq
.Lfunc_end3:
	.size	def_byval, .Lfunc_end3-def_byval
	.cfi_endproc

	.globl	call_nest
	.p2align	4
	.type	call_nest,@function
call_nest:
	.cfi_startproc
	pushq	%rax
	.cfi_def_cfa_offset 16
	movq	%rdi, %r10
	movl	$3, %edi
	callq	f_nest@PLT
	popq	%rcx
	.cfi_def_cfa_offset 8
	retq
.Lfunc_end4:
	.size	call_nest, .Lfunc_end4-call_nest
	.cfi_endproc

	.globl	call_inreg
	.p2align	4
	.type	call_inreg,@function
call_inreg:
	.cfi_startproc
	pushq	%rax
	.cfi_def_cfa_offset 16
	movl	$1, %edi
	movl	$2, %esi
	callq	f_inreg@PLT
	popq	%rcx
	.cfi_def_cfa_offset 8
	retq
.Lfunc_end5:
	.size	call_inreg, .Lfunc_end5-call_inreg
	.cfi_endproc

	.globl	call_swift
	.p2align	4
	.type	call_swift,@function
call_swift:
	.cfi_startproc
	pushq	%rax
	.cfi_def_cfa_offset 16
	callq	f_swift@PLT
	popq	%rcx
	.cfi_def_cfa_offset 8
	retq
.Lfunc_end6:
	.size	call_swift, .Lfunc_end6-call_swift
	.cfi_endproc

	.section	".note.GNU-stack","",@progbits
