	.file	"08-varargs.ll"
	.text
	.globl	call_va
	.p2align	4
	.type	call_va,@function
call_va:
	.cfi_startproc
	subq	$24, %rsp
	.cfi_def_cfa_offset 32
	movq	%rsi, %rcx
	movq	%rdi, %rdx
	fldt	32(%rsp)
	movq	%rsp, %rax
	fstpt	(%rax)
	leaq	.Lfmt(%rip), %rdi
	movl	$1, %esi
	movb	$1, %al
	callq	printf@PLT
	addq	$24, %rsp
	.cfi_def_cfa_offset 8
	retq
.Lfunc_end0:
	.size	call_va, .Lfunc_end0-call_va
	.cfi_endproc

	.globl	def_va
	.p2align	4
	.type	def_va,@function
def_va:
	.cfi_startproc
	subq	$104, %rsp
	.cfi_def_cfa_offset 112
	testb	%al, %al
	je	.LBB1_8
	movaps	%xmm0, -48(%rsp)
	movaps	%xmm1, -32(%rsp)
	movaps	%xmm2, -16(%rsp)
	movaps	%xmm3, (%rsp)
	movaps	%xmm4, 16(%rsp)
	movaps	%xmm5, 32(%rsp)
	movaps	%xmm6, 48(%rsp)
	movaps	%xmm7, 64(%rsp)
.LBB1_8:
	movq	%r9, -56(%rsp)
	movq	%r8, -64(%rsp)
	movq	%rcx, -72(%rsp)
	movq	%rdx, -80(%rsp)
	movq	%rsi, -88(%rsp)
	leaq	-96(%rsp), %rax
	movq	%rax, 96(%rsp)
	leaq	112(%rsp), %rax
	movq	%rax, 88(%rsp)
	movl	$48, 84(%rsp)
	movl	$8, 80(%rsp)
	movl	80(%rsp), %eax
	movl	%eax, -100(%rsp)
	cmpl	$48, %eax
	jae	.LBB1_2
	movl	-100(%rsp), %ecx
	movq	96(%rsp), %rdx
	movl	%ecx, %eax
	addq	%rdx, %rax
	addl	$8, %ecx
	movl	%ecx, 80(%rsp)
	movq	%rax, -112(%rsp)
	jmp	.LBB1_3
.LBB1_2:
	movq	88(%rsp), %rax
	movq	%rax, %rcx
	addq	$8, %rcx
	movq	%rcx, 88(%rsp)
	movq	%rax, -112(%rsp)
.LBB1_3:
	movq	-112(%rsp), %rax
	movl	(%rax), %eax
	movl	%eax, -120(%rsp)
	movl	84(%rsp), %eax
	movl	%eax, -116(%rsp)
	cmpl	$176, %eax
	jae	.LBB1_5
	movl	-116(%rsp), %ecx
	movq	96(%rsp), %rdx
	movl	%ecx, %eax
	addq	%rdx, %rax
	addl	$16, %ecx
	movl	%ecx, 84(%rsp)
	movq	%rax, -128(%rsp)
	jmp	.LBB1_6
.LBB1_5:
	movq	88(%rsp), %rax
	movq	%rax, %rcx
	addq	$8, %rcx
	movq	%rcx, 88(%rsp)
	movq	%rax, -128(%rsp)
.LBB1_6:
	movl	-120(%rsp), %eax
	movq	-128(%rsp), %rcx
	cvttsd2si	(%rcx), %ecx
	addl	%ecx, %eax
	addq	$104, %rsp
	.cfi_def_cfa_offset 8
	retq
.Lfunc_end1:
	.size	def_va, .Lfunc_end1-def_va
	.cfi_endproc

	.type	.Lfmt,@object
	.section	.rodata,"a",@progbits
.Lfmt:
	.asciz	"%f\n"
	.size	.Lfmt, 4

	.section	".note.GNU-stack","",@progbits
