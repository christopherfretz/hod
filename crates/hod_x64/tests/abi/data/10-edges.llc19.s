	.text
	.file	"10-edges.ll"
	.globl	ret_4f
	.p2align	4, 0x90
	.type	ret_4f,@function
ret_4f:
	.cfi_startproc
	movss	%xmm3, -8(%rsp)
	movss	%xmm2, -4(%rsp)
	flds	-8(%rsp)
	flds	-4(%rsp)
	retq
.Lfunc_end0:
	.size	ret_4f, .Lfunc_end0-ret_4f
	.cfi_endproc

	.globl	ret_3d
	.p2align	4, 0x90
	.type	ret_3d,@function
ret_3d:
	.cfi_startproc
	movsd	%xmm2, -8(%rsp)
	fldl	-8(%rsp)
	retq
.Lfunc_end1:
	.size	ret_3d, .Lfunc_end1-ret_3d
	.cfi_endproc

	.globl	split_pair
	.p2align	4, 0x90
	.type	split_pair,@function
split_pair:
	.cfi_startproc
	movq	%r9, %rax
	movq	8(%rsp), %rcx
	subq	%rcx, %rax
	retq
.Lfunc_end2:
	.size	split_pair, .Lfunc_end2-split_pair
	.cfi_endproc

	.globl	in_struct
	.p2align	4, 0x90
	.type	in_struct,@function
in_struct:
	.cfi_startproc
	movq	%rsi, %rax
	retq
.Lfunc_end3:
	.size	in_struct, .Lfunc_end3-in_struct
	.cfi_endproc

	.globl	v_second
	.p2align	4, 0x90
	.type	v_second,@function
v_second:
	.cfi_startproc
	movaps	%xmm1, %xmm0
	retq
.Lfunc_end4:
	.size	v_second, .Lfunc_end4-v_second
	.cfi_endproc

	.globl	v8_hi
	.p2align	4, 0x90
	.type	v8_hi,@function
v8_hi:
	.cfi_startproc
	movaps	%xmm1, %xmm0
	retq
.Lfunc_end5:
	.size	v8_hi, .Lfunc_end5-v8_hi
	.cfi_endproc

	.section	.rodata.cst16,"aM",@progbits,16
	.p2align	4, 0x0
.LCPI6_0:
	.long	1
	.long	1
	.long	1
	.long	1
	.text
	.globl	v_i1
	.p2align	4, 0x90
	.type	v_i1,@function
v_i1:
	.cfi_startproc
	movaps	.LCPI6_0(%rip), %xmm1
	pxor	%xmm1, %xmm0
	retq
.Lfunc_end6:
	.size	v_i1, .Lfunc_end6-v_i1
	.cfi_endproc

	.globl	v3
	.p2align	4, 0x90
	.type	v3,@function
v3:
	.cfi_startproc
	retq
.Lfunc_end7:
	.size	v3, .Lfunc_end7-v3
	.cfi_endproc

	.globl	v_ninth
	.p2align	4, 0x90
	.type	v_ninth,@function
v_ninth:
	.cfi_startproc
	movaps	8(%rsp), %xmm0
	retq
.Lfunc_end8:
	.size	v_ninth, .Lfunc_end8-v_ninth
	.cfi_endproc

	.globl	ret_v16f
	.p2align	4, 0x90
	.type	ret_v16f,@function
ret_v16f:
	.cfi_startproc
	retq
.Lfunc_end9:
	.size	ret_v16f, .Lfunc_end9-ret_v16f
	.cfi_endproc

	.globl	ret_v32f
	.p2align	4, 0x90
	.type	ret_v32f,@function
ret_v32f:
	.cfi_startproc
	movq	%rdi, %rax
	movaps	%xmm7, 112(%rdi)
	movaps	%xmm6, 96(%rdi)
	movaps	%xmm5, 80(%rdi)
	movaps	%xmm4, 64(%rdi)
	movaps	%xmm3, 48(%rdi)
	movaps	%xmm2, 32(%rdi)
	movaps	%xmm1, 16(%rdi)
	movaps	%xmm0, (%rdi)
	retq
.Lfunc_end10:
	.size	ret_v32f, .Lfunc_end10-ret_v32f
	.cfi_endproc

	.globl	v2i64
	.p2align	4, 0x90
	.type	v2i64,@function
v2i64:
	.cfi_startproc
	retq
.Lfunc_end11:
	.size	v2i64, .Lfunc_end11-v2i64
	.cfi_endproc

	.globl	v1i64
	.p2align	4, 0x90
	.type	v1i64,@function
v1i64:
	.cfi_startproc
	movq	%rdi, %rax
	retq
.Lfunc_end12:
	.size	v1i64, .Lfunc_end12-v1i64
	.cfi_endproc

	.globl	v2f
	.p2align	4, 0x90
	.type	v2f,@function
v2f:
	.cfi_startproc
	retq
.Lfunc_end13:
	.size	v2f, .Lfunc_end13-v2f
	.cfi_endproc

	.globl	pass_i1
	.p2align	4, 0x90
	.type	pass_i1,@function
pass_i1:
	.cfi_startproc
	pushq	%rax
	.cfi_def_cfa_offset 16
	subl	$3, %edi
	sete	%al
	movzbl	%al, %edi
	callq	take_i1@PLT
	popq	%rax
	.cfi_def_cfa_offset 8
	retq
.Lfunc_end14:
	.size	pass_i1, .Lfunc_end14-pass_i1
	.cfi_endproc

	.globl	q_stack
	.p2align	4, 0x90
	.type	q_stack,@function
q_stack:
	.cfi_startproc
	movaps	24(%rsp), %xmm0
	retq
.Lfunc_end15:
	.size	q_stack, .Lfunc_end15-q_stack
	.cfi_endproc

	.section	".note.GNU-stack","",@progbits
