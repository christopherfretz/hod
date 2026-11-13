	.file	"02-fp.ll"
	.section	.rodata.cst4,"aM",@progbits,4
	.p2align	2, 0x0
.LCPI0_0:
	.long	0x3f800000
.LCPI0_2:
	.long	0x40400000
.LCPI0_4:
	.long	0x40a00000
.LCPI0_6:
	.long	0x40e00000
	.section	.rodata.cst8,"aM",@progbits,8
	.p2align	3, 0x0
.LCPI0_1:
	.quad	0x4000000000000000
.LCPI0_3:
	.quad	0x4010000000000000
.LCPI0_5:
	.quad	0x4018000000000000
.LCPI0_7:
	.quad	0x4020000000000000
	.text
	.globl	call_fp
	.p2align	4
	.type	call_fp,@function
call_fp:
	.cfi_startproc
	subq	$24, %rsp
	.cfi_def_cfa_offset 32
	movq	%rsp, %rax
	movabsq	$4621819117588971520, %rcx
	movq	%rcx, 8(%rax)
	movl	$1091567616, (%rax)
	movss	.LCPI0_0(%rip), %xmm0
	movsd	.LCPI0_1(%rip), %xmm1
	movss	.LCPI0_2(%rip), %xmm2
	movsd	.LCPI0_3(%rip), %xmm3
	movss	.LCPI0_4(%rip), %xmm4
	movsd	.LCPI0_5(%rip), %xmm5
	movss	.LCPI0_6(%rip), %xmm6
	movsd	.LCPI0_7(%rip), %xmm7
	callq	sinkf@PLT
	addq	$24, %rsp
	.cfi_def_cfa_offset 8
	retq
.Lfunc_end0:
	.size	call_fp, .Lfunc_end0-call_fp
	.cfi_endproc

	.globl	call_h
	.p2align	4
	.type	call_h,@function
call_h:
	.cfi_startproc
	pushq	%rax
	.cfi_def_cfa_offset 16
	callq	h@PLT
	popq	%rax
	.cfi_def_cfa_offset 8
	retq
.Lfunc_end1:
	.size	call_h, .Lfunc_end1-call_h
	.cfi_endproc

	.globl	call_q
	.p2align	4
	.type	call_q,@function
call_q:
	.cfi_startproc
	pushq	%rax
	.cfi_def_cfa_offset 16
	movaps	%xmm0, %xmm1
	movaps	%xmm1, %xmm0
	callq	q@PLT
	popq	%rax
	.cfi_def_cfa_offset 8
	retq
.Lfunc_end2:
	.size	call_q, .Lfunc_end2-call_q
	.cfi_endproc

	.globl	ret_dd
	.p2align	4
	.type	ret_dd,@function
ret_dd:
	.cfi_startproc
	movaps	%xmm0, %xmm1
	movaps	%xmm1, %xmm0
	retq
.Lfunc_end3:
	.size	ret_dd, .Lfunc_end3-ret_dd
	.cfi_endproc

	.globl	ret_id
	.p2align	4
	.type	ret_id,@function
ret_id:
	.cfi_startproc
	movq	%rdi, %rax
	retq
.Lfunc_end4:
	.size	ret_id, .Lfunc_end4-ret_id
	.cfi_endproc

	.globl	ret_4f
	.p2align	4
	.type	ret_4f,@function
ret_4f:
	.cfi_startproc
	movss	%xmm0, -8(%rsp)
	movss	%xmm1, -4(%rsp)
	flds	-8(%rsp)
	flds	-4(%rsp)
	retq
.Lfunc_end5:
	.size	ret_4f, .Lfunc_end5-ret_4f
	.cfi_endproc

	.globl	ret_5f
	.p2align	4
	.type	ret_5f,@function
ret_5f:
	.cfi_startproc
	movq	%rdi, %rax
	movss	%xmm0, 16(%rdi)
	retq
.Lfunc_end6:
	.size	ret_5f, .Lfunc_end6-ret_5f
	.cfi_endproc

	.section	".note.GNU-stack","",@progbits
