	.file	"11-libcalls-sse2.ll"
	.text
	.globl	sdiv
	.p2align	4
	.type	sdiv,@function
sdiv:
	.cfi_startproc
	pushl	%edi
	.cfi_def_cfa_offset 8
	pushl	%esi
	.cfi_def_cfa_offset 12
	subl	$20, %esp
	.cfi_def_cfa_offset 32
	.cfi_offset %esi, -12
	.cfi_offset %edi, -8
	movl	32(%esp), %ecx
	movl	36(%esp), %edx
	movl	40(%esp), %esi
	movl	44(%esp), %edi
	movl	%esp, %eax
	movl	%edi, 12(%eax)
	movl	%esi, 8(%eax)
	movl	%edx, 4(%eax)
	movl	%ecx, (%eax)
	calll	__divdi3
	addl	$20, %esp
	.cfi_def_cfa_offset 12
	popl	%esi
	.cfi_def_cfa_offset 8
	popl	%edi
	.cfi_def_cfa_offset 4
	retl
.Lfunc_end0:
	.size	sdiv, .Lfunc_end0-sdiv
	.cfi_endproc

	.globl	udiv
	.p2align	4
	.type	udiv,@function
udiv:
	.cfi_startproc
	pushl	%edi
	.cfi_def_cfa_offset 8
	pushl	%esi
	.cfi_def_cfa_offset 12
	subl	$20, %esp
	.cfi_def_cfa_offset 32
	.cfi_offset %esi, -12
	.cfi_offset %edi, -8
	movl	32(%esp), %ecx
	movl	36(%esp), %edx
	movl	40(%esp), %esi
	movl	44(%esp), %edi
	movl	%esp, %eax
	movl	%edi, 12(%eax)
	movl	%esi, 8(%eax)
	movl	%edx, 4(%eax)
	movl	%ecx, (%eax)
	calll	__udivdi3
	addl	$20, %esp
	.cfi_def_cfa_offset 12
	popl	%esi
	.cfi_def_cfa_offset 8
	popl	%edi
	.cfi_def_cfa_offset 4
	retl
.Lfunc_end1:
	.size	udiv, .Lfunc_end1-udiv
	.cfi_endproc

	.globl	srem
	.p2align	4
	.type	srem,@function
srem:
	.cfi_startproc
	pushl	%edi
	.cfi_def_cfa_offset 8
	pushl	%esi
	.cfi_def_cfa_offset 12
	subl	$20, %esp
	.cfi_def_cfa_offset 32
	.cfi_offset %esi, -12
	.cfi_offset %edi, -8
	movl	32(%esp), %ecx
	movl	36(%esp), %edx
	movl	40(%esp), %esi
	movl	44(%esp), %edi
	movl	%esp, %eax
	movl	%edi, 12(%eax)
	movl	%esi, 8(%eax)
	movl	%edx, 4(%eax)
	movl	%ecx, (%eax)
	calll	__moddi3
	addl	$20, %esp
	.cfi_def_cfa_offset 12
	popl	%esi
	.cfi_def_cfa_offset 8
	popl	%edi
	.cfi_def_cfa_offset 4
	retl
.Lfunc_end2:
	.size	srem, .Lfunc_end2-srem
	.cfi_endproc

	.globl	urem
	.p2align	4
	.type	urem,@function
urem:
	.cfi_startproc
	pushl	%edi
	.cfi_def_cfa_offset 8
	pushl	%esi
	.cfi_def_cfa_offset 12
	subl	$20, %esp
	.cfi_def_cfa_offset 32
	.cfi_offset %esi, -12
	.cfi_offset %edi, -8
	movl	32(%esp), %ecx
	movl	36(%esp), %edx
	movl	40(%esp), %esi
	movl	44(%esp), %edi
	movl	%esp, %eax
	movl	%edi, 12(%eax)
	movl	%esi, 8(%eax)
	movl	%edx, 4(%eax)
	movl	%ecx, (%eax)
	calll	__umoddi3
	addl	$20, %esp
	.cfi_def_cfa_offset 12
	popl	%esi
	.cfi_def_cfa_offset 8
	popl	%edi
	.cfi_def_cfa_offset 4
	retl
.Lfunc_end3:
	.size	urem, .Lfunc_end3-urem
	.cfi_endproc

	.globl	mul
	.p2align	4
	.type	mul,@function
mul:
	.cfi_startproc
	pushl	%edi
	.cfi_def_cfa_offset 8
	pushl	%esi
	.cfi_def_cfa_offset 12
	.cfi_offset %esi, -12
	.cfi_offset %edi, -8
	movl	20(%esp), %esi
	movl	24(%esp), %edx
	movl	12(%esp), %eax
	movl	16(%esp), %ecx
	movl	%eax, %edi
	imull	%edx, %edi
	mull	%esi
	addl	%edi, %edx
	imull	%esi, %ecx
	addl	%ecx, %edx
	popl	%esi
	.cfi_def_cfa_offset 8
	popl	%edi
	.cfi_def_cfa_offset 4
	retl
.Lfunc_end4:
	.size	mul, .Lfunc_end4-mul
	.cfi_endproc

	.globl	shl
	.p2align	4
	.type	shl,@function
shl:
	.cfi_startproc
	pushl	%ebx
	.cfi_def_cfa_offset 8
	pushl	%esi
	.cfi_def_cfa_offset 12
	.cfi_offset %esi, -12
	.cfi_offset %ebx, -8
	movb	20(%esp), %bl
	movl	12(%esp), %esi
	movl	16(%esp), %edx
	movb	%bl, %cl
	movl	%esi, %eax
	shll	%cl, %eax
	movb	%bl, %cl
	shldl	%cl, %esi, %edx
	xorl	%ecx, %ecx
	testb	$32, %bl
	cmovnel	%eax, %edx
	cmovnel	%ecx, %eax
	popl	%esi
	.cfi_def_cfa_offset 8
	popl	%ebx
	.cfi_def_cfa_offset 4
	retl
.Lfunc_end5:
	.size	shl, .Lfunc_end5-shl
	.cfi_endproc

	.globl	ashr
	.p2align	4
	.type	ashr,@function
ashr:
	.cfi_startproc
	pushl	%ebx
	.cfi_def_cfa_offset 8
	pushl	%esi
	.cfi_def_cfa_offset 12
	pushl	%eax
	.cfi_def_cfa_offset 16
	.cfi_offset %esi, -12
	.cfi_offset %ebx, -8
	movb	24(%esp), %bl
	movl	16(%esp), %eax
	movl	20(%esp), %esi
	movl	%esi, (%esp)
	movb	%bl, %cl
	movl	%esi, %edx
	sarl	%cl, %edx
	movb	%bl, %cl
	shrdl	%cl, %esi, %eax
	movl	(%esp), %ecx
	sarl	$31, %ecx
	testb	$32, %bl
	cmovnel	%edx, %eax
	cmovnel	%ecx, %edx
	addl	$4, %esp
	.cfi_def_cfa_offset 12
	popl	%esi
	.cfi_def_cfa_offset 8
	popl	%ebx
	.cfi_def_cfa_offset 4
	retl
.Lfunc_end6:
	.size	ashr, .Lfunc_end6-ashr
	.cfi_endproc

	.globl	f2s
	.p2align	4
	.type	f2s,@function
f2s:
	.cfi_startproc
	subl	$20, %esp
	.cfi_def_cfa_offset 24
	movsd	24(%esp), %xmm0
	movsd	%xmm0, 8(%esp)
	fldl	8(%esp)
	fnstcw	6(%esp)
	movzwl	6(%esp), %eax
	orl	$3072, %eax
	movw	%ax, 4(%esp)
	fldcw	4(%esp)
	fistpll	8(%esp)
	fldcw	6(%esp)
	movl	8(%esp), %eax
	movl	12(%esp), %edx
	addl	$20, %esp
	.cfi_def_cfa_offset 4
	retl
.Lfunc_end7:
	.size	f2s, .Lfunc_end7-f2s
	.cfi_endproc

	.section	.rodata.cst8,"aM",@progbits,8
	.p2align	3, 0x0
.LCPI8_0:
	.quad	0x43e0000000000000
	.text
	.globl	f2u
	.p2align	4
	.type	f2u,@function
f2u:
	.cfi_startproc
	subl	$44, %esp
	.cfi_def_cfa_offset 48
	movsd	48(%esp), %xmm1
	movsd	%xmm1, (%esp)
	movsd	.LCPI8_0, %xmm0
	ucomisd	%xmm1, %xmm0
	xorps	%xmm1, %xmm1
	movsd	%xmm1, 8(%esp)
	movsd	%xmm0, 16(%esp)
	jbe	.LBB8_2
	movsd	8(%esp), %xmm0
	movsd	%xmm0, 16(%esp)
.LBB8_2:
	movsd	(%esp), %xmm0
	movsd	16(%esp), %xmm1
	subsd	%xmm1, %xmm0
	movsd	%xmm0, 32(%esp)
	setbe	%al
	fldl	32(%esp)
	fnstcw	30(%esp)
	movzwl	30(%esp), %ecx
	orl	$3072, %ecx
	movw	%cx, 28(%esp)
	fldcw	28(%esp)
	fistpll	32(%esp)
	fldcw	30(%esp)
	movzbl	%al, %ecx
	shll	$31, %ecx
	movl	32(%esp), %eax
	movl	36(%esp), %edx
	xorl	%ecx, %edx
	addl	$44, %esp
	.cfi_def_cfa_offset 4
	retl
.Lfunc_end8:
	.size	f2u, .Lfunc_end8-f2u
	.cfi_endproc

	.globl	s2f
	.p2align	4
	.type	s2f,@function
s2f:
	.cfi_startproc
	subl	$12, %esp
	.cfi_def_cfa_offset 16
	fildll	16(%esp)
	fstpl	(%esp)
	fldl	(%esp)
	addl	$12, %esp
	.cfi_def_cfa_offset 4
	retl
.Lfunc_end9:
	.size	s2f, .Lfunc_end9-s2f
	.cfi_endproc

	.section	.rodata.cst16,"aM",@progbits,16
	.p2align	4, 0x0
.LCPI10_0:
	.long	1127219200
	.long	1160773632
	.long	0
	.long	0
.LCPI10_1:
	.quad	0x4330000000000000
	.quad	0x4530000000000000
	.text
	.globl	u2f
	.p2align	4
	.type	u2f,@function
u2f:
	.cfi_startproc
	subl	$12, %esp
	.cfi_def_cfa_offset 16
	movaps	.LCPI10_0, %xmm1
	movq	16(%esp), %xmm0
	punpckldq	%xmm1, %xmm0
	movapd	.LCPI10_1, %xmm1
	subpd	%xmm1, %xmm0
	movaps	%xmm0, %xmm1
	unpckhpd	%xmm0, %xmm0
	addsd	%xmm1, %xmm0
	movsd	%xmm0, (%esp)
	fldl	(%esp)
	addl	$12, %esp
	.cfi_def_cfa_offset 4
	retl
.Lfunc_end10:
	.size	u2f, .Lfunc_end10-u2f
	.cfi_endproc

	.globl	f2s32
	.p2align	4
	.type	f2s32,@function
f2s32:
	.cfi_startproc
	cvttsd2si	4(%esp), %eax
	retl
.Lfunc_end11:
	.size	f2s32, .Lfunc_end11-f2s32
	.cfi_endproc

	.globl	frem
	.p2align	4
	.type	frem,@function
frem:
	.cfi_startproc
	jmp	fmod
.Lfunc_end12:
	.size	frem, .Lfunc_end12-frem
	.cfi_endproc

	.globl	smulo
	.p2align	4
	.type	smulo,@function
smulo:
	.cfi_startproc
	pushl	%ebx
	.cfi_def_cfa_offset 8
	pushl	%edi
	.cfi_def_cfa_offset 12
	pushl	%esi
	.cfi_def_cfa_offset 16
	pushl	%eax
	.cfi_def_cfa_offset 20
	.cfi_offset %esi, -16
	.cfi_offset %edi, -12
	.cfi_offset %ebx, -8
	movl	20(%esp), %eax
	movl	%eax, (%esp)
	movl	24(%esp), %ecx
	movl	28(%esp), %ebx
	movl	32(%esp), %esi
	mull	%ebx
	movl	%edx, %edi
	movl	(%esp), %edx
	imull	%ebx, %ecx
	addl	%edi, %ecx
	imull	%esi, %edx
	addl	%ecx, %edx
	addl	$4, %esp
	.cfi_def_cfa_offset 16
	popl	%esi
	.cfi_def_cfa_offset 12
	popl	%edi
	.cfi_def_cfa_offset 8
	popl	%ebx
	.cfi_def_cfa_offset 4
	retl
.Lfunc_end13:
	.size	smulo, .Lfunc_end13-smulo
	.cfi_endproc

	.section	.rodata.cst16,"aM",@progbits,16
	.p2align	4, 0x0
.LCPI14_0:
	.zero	16,85
.LCPI14_1:
	.zero	16,51
.LCPI14_2:
	.zero	16,15
	.text
	.globl	ctpop
	.p2align	4
	.type	ctpop,@function
ctpop:
	.cfi_startproc
	movq	4(%esp), %xmm1
	movaps	%xmm1, %xmm0
	psrlw	$1, %xmm0
	movaps	.LCPI14_0, %xmm2
	pand	%xmm2, %xmm0
	psubb	%xmm0, %xmm1
	movaps	.LCPI14_1, %xmm2
	movaps	%xmm1, %xmm0
	pand	%xmm2, %xmm0
	psrlw	$2, %xmm1
	pand	%xmm2, %xmm1
	paddb	%xmm1, %xmm0
	movaps	%xmm0, %xmm1
	psrlw	$4, %xmm1
	paddb	%xmm1, %xmm0
	movaps	.LCPI14_2, %xmm1
	pand	%xmm1, %xmm0
	xorps	%xmm1, %xmm1
	psadbw	%xmm1, %xmm0
	movd	%xmm0, %eax
	xorl	%edx, %edx
	retl
.Lfunc_end14:
	.size	ctpop, .Lfunc_end14-ctpop
	.cfi_endproc

	.globl	mc
	.p2align	4
	.type	mc,@function
mc:
	.cfi_startproc
	pushl	%esi
	.cfi_def_cfa_offset 8
	subl	$24, %esp
	.cfi_def_cfa_offset 32
	.cfi_offset %esi, -8
	movl	32(%esp), %ecx
	movl	36(%esp), %edx
	movl	40(%esp), %esi
	movl	%esp, %eax
	movl	%esi, 8(%eax)
	movl	%edx, 4(%eax)
	movl	%ecx, (%eax)
	calll	memcpy
	addl	$24, %esp
	.cfi_def_cfa_offset 8
	popl	%esi
	.cfi_def_cfa_offset 4
	retl
.Lfunc_end15:
	.size	mc, .Lfunc_end15-mc
	.cfi_endproc

	.section	".note.GNU-stack","",@progbits
