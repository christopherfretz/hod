	.file	"12-atomic-sse2.ll"
	.text
	.globl	ld64
	.p2align	4
	.type	ld64,@function
ld64:
	.cfi_startproc
	movl	4(%esp), %eax
	movq	(%eax), %xmm0
	movd	%xmm0, %eax
	pshufd	$85, %xmm0, %xmm0
	movd	%xmm0, %edx
	retl
.Lfunc_end0:
	.size	ld64, .Lfunc_end0-ld64
	.cfi_endproc

	.globl	st64
	.p2align	4
	.type	st64,@function
st64:
	.cfi_startproc
	movl	4(%esp), %eax
	movq	8(%esp), %xmm0
	movq	%xmm0, (%eax)
	lock		orl	$0, (%esp)
	retl
.Lfunc_end1:
	.size	st64, .Lfunc_end1-st64
	.cfi_endproc

	.globl	add64
	.p2align	4
	.type	add64,@function
add64:
	.cfi_startproc
	pushl	%ebx
	.cfi_def_cfa_offset 8
	pushl	%edi
	.cfi_def_cfa_offset 12
	pushl	%esi
	.cfi_def_cfa_offset 16
	subl	$28, %esp
	.cfi_def_cfa_offset 44
	.cfi_offset %esi, -16
	.cfi_offset %edi, -12
	.cfi_offset %ebx, -8
	movl	52(%esp), %eax
	movl	%eax, 8(%esp)
	movl	48(%esp), %eax
	movl	%eax, 12(%esp)
	movl	44(%esp), %ecx
	movl	%ecx, 16(%esp)
	movl	(%ecx), %eax
	movl	4(%ecx), %edx
	movl	%eax, 20(%esp)
	movl	%edx, 24(%esp)
	jmp	.LBB2_1
.LBB2_1:
	movl	24(%esp), %edx
	movl	20(%esp), %eax
	movl	16(%esp), %esi
	movl	8(%esp), %edi
	movl	12(%esp), %ecx
	movl	%eax, %ebx
	addl	%ecx, %ebx
	movl	%edx, %ecx
	adcl	%edi, %ecx
	lock		cmpxchg8b	(%esi)
	movl	%edx, (%esp)
	movl	%eax, 4(%esp)
	movl	%eax, 20(%esp)
	movl	%edx, 24(%esp)
	jne	.LBB2_1
	jmp	.LBB2_2
.LBB2_2:
	movl	(%esp), %edx
	movl	4(%esp), %eax
	addl	$28, %esp
	.cfi_def_cfa_offset 16
	popl	%esi
	.cfi_def_cfa_offset 12
	popl	%edi
	.cfi_def_cfa_offset 8
	popl	%ebx
	.cfi_def_cfa_offset 4
	retl
.Lfunc_end2:
	.size	add64, .Lfunc_end2-add64
	.cfi_endproc

	.globl	cx64
	.p2align	4
	.type	cx64,@function
cx64:
	.cfi_startproc
	pushl	%ebx
	.cfi_def_cfa_offset 8
	pushl	%esi
	.cfi_def_cfa_offset 12
	.cfi_offset %esi, -12
	.cfi_offset %ebx, -8
	movl	16(%esp), %eax
	movl	20(%esp), %edx
	movl	24(%esp), %ebx
	movl	28(%esp), %ecx
	movl	12(%esp), %esi
	lock		cmpxchg8b	(%esi)
	popl	%esi
	.cfi_def_cfa_offset 8
	popl	%ebx
	.cfi_def_cfa_offset 4
	retl
.Lfunc_end3:
	.size	cx64, .Lfunc_end3-cx64
	.cfi_endproc

	.globl	add32
	.p2align	4
	.type	add32,@function
add32:
	.cfi_startproc
	movl	4(%esp), %ecx
	movl	8(%esp), %eax
	lock		xaddl	%eax, (%ecx)
	retl
.Lfunc_end4:
	.size	add32, .Lfunc_end4-add32
	.cfi_endproc

	.globl	ld128
	.p2align	4
	.type	ld128,@function
ld128:
	.cfi_startproc
	pushl	%ebx
	.cfi_def_cfa_offset 8
	pushl	%edi
	.cfi_def_cfa_offset 12
	pushl	%esi
	.cfi_def_cfa_offset 16
	subl	$48, %esp
	.cfi_def_cfa_offset 64
	.cfi_offset %esi, -16
	.cfi_offset %edi, -12
	.cfi_offset %ebx, -8
	movl	64(%esp), %eax
	movl	%eax, 24(%esp)
	movl	%eax, 28(%esp)
	movl	68(%esp), %ecx
	movl	%esp, %eax
	leal	32(%esp), %edx
	movl	%edx, 8(%eax)
	movl	%ecx, 4(%eax)
	movl	$5, 12(%eax)
	movl	$16, (%eax)
	calll	__atomic_load@PLT
	movl	24(%esp), %ecx
	movl	28(%esp), %eax
	movl	32(%esp), %edx
	movl	36(%esp), %esi
	movl	40(%esp), %edi
	movl	44(%esp), %ebx
	movl	%ebx, 12(%ecx)
	movl	%edi, 8(%ecx)
	movl	%esi, 4(%ecx)
	movl	%edx, (%ecx)
	addl	$48, %esp
	.cfi_def_cfa_offset 16
	popl	%esi
	.cfi_def_cfa_offset 12
	popl	%edi
	.cfi_def_cfa_offset 8
	popl	%ebx
	.cfi_def_cfa_offset 4
	retl	$4
.Lfunc_end5:
	.size	ld128, .Lfunc_end5-ld128
	.cfi_endproc

	.globl	ldd
	.p2align	4
	.type	ldd,@function
ldd:
	.cfi_startproc
	subl	$12, %esp
	.cfi_def_cfa_offset 16
	movl	16(%esp), %eax
	movq	(%eax), %xmm0
	movlpd	%xmm0, (%esp)
	fldl	(%esp)
	addl	$12, %esp
	.cfi_def_cfa_offset 4
	retl
.Lfunc_end6:
	.size	ldd, .Lfunc_end6-ldd
	.cfi_endproc

	.globl	fence
	.p2align	4
	.type	fence,@function
fence:
	.cfi_startproc
	mfence
	retl
.Lfunc_end7:
	.size	fence, .Lfunc_end7-fence
	.cfi_endproc

	.section	".note.GNU-stack","",@progbits
