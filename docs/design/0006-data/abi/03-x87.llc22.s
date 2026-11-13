	.file	"03-x87.ll"
	.text
	.globl	call_ld
	.p2align	4
	.type	call_ld,@function
call_ld:
	.cfi_startproc
	subq	$56, %rsp
	.cfi_def_cfa_offset 64
	fldt	64(%rsp)
	fld	%st(0)
	fstpt	44(%rsp)
	movq	%rsp, %rax
	fld	%st(0)
	fstpt	16(%rax)
	fstpt	(%rax)
	movl	$1, %edi
	callq	ld@PLT
	fldt	44(%rsp)
	faddp	%st, %st(1)
	addq	$56, %rsp
	.cfi_def_cfa_offset 8
	retq
.Lfunc_end0:
	.size	call_ld, .Lfunc_end0-call_ld
	.cfi_endproc

	.globl	ld_to_d
	.p2align	4
	.type	ld_to_d,@function
ld_to_d:
	.cfi_startproc
	fldt	8(%rsp)
	fstpl	-8(%rsp)
	movsd	-8(%rsp), %xmm0
	retq
.Lfunc_end1:
	.size	ld_to_d, .Lfunc_end1-ld_to_d
	.cfi_endproc

	.globl	ld_to_i
	.p2align	4
	.type	ld_to_i,@function
ld_to_i:
	.cfi_startproc
	fldt	8(%rsp)
	fnstcw	-10(%rsp)
	movzwl	-10(%rsp), %eax
	orl	$3072, %eax
	movw	%ax, -12(%rsp)
	fldcw	-12(%rsp)
	fistpll	-8(%rsp)
	fldcw	-10(%rsp)
	movq	-8(%rsp), %rax
	retq
.Lfunc_end2:
	.size	ld_to_i, .Lfunc_end2-ld_to_i
	.cfi_endproc

	.section	.rodata.cst4,"aM",@progbits,4
	.p2align	2, 0x0
.LCPI3_0:
	.long	0x5f000000
	.text
	.globl	ld_to_u
	.p2align	4
	.type	ld_to_u,@function
ld_to_u:
	.cfi_startproc
	fldt	8(%rsp)
	flds	.LCPI3_0(%rip)
	fucomi	%st(1), %st
	setbe	%al
	fldz
	fcmovbe	%st(1), %st
	fstp	%st(1)
	fsubrp	%st, %st(1)
	fnstcw	-10(%rsp)
	movzwl	-10(%rsp), %ecx
	orl	$3072, %ecx
	movw	%cx, -12(%rsp)
	fldcw	-12(%rsp)
	fistpll	-8(%rsp)
	fldcw	-10(%rsp)
	movzbl	%al, %eax
	movl	%eax, %ecx
	shlq	$63, %rcx
	movq	-8(%rsp), %rax
	xorq	%rcx, %rax
	retq
.Lfunc_end3:
	.size	ld_to_u, .Lfunc_end3-ld_to_u
	.cfi_endproc

	.globl	ld_cmp
	.p2align	4
	.type	ld_cmp,@function
ld_cmp:
	.cfi_startproc
	fldt	8(%rsp)
	fldt	24(%rsp)
	fucompi	%st(1), %st
	fstp	%st(0)
	seta	%al
	retq
.Lfunc_end4:
	.size	ld_cmp, .Lfunc_end4-ld_cmp
	.cfi_endproc

	.globl	ld_complex
	.p2align	4
	.type	ld_complex,@function
ld_complex:
	.cfi_startproc
	fldt	8(%rsp)
	fld	%st(0)
	retq
.Lfunc_end5:
	.size	ld_complex, .Lfunc_end5-ld_complex
	.cfi_endproc

	.section	".note.GNU-stack","",@progbits
