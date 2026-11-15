	.file	"11-libcalls.ll"
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
	pushl	%edi
	.cfi_def_cfa_offset 8
	pushl	%esi
	.cfi_def_cfa_offset 12
	subl	$16, %esp
	.cfi_def_cfa_offset 28
	.cfi_offset %esi, -12
	.cfi_offset %edi, -8
	movb	36(%esp), %dl
	movl	28(%esp), %esi
	movl	32(%esp), %eax
	movb	%dl, %cl
	movl	%esi, %edi
	shll	%cl, %edi
	movl	%edi, (%esp)
	movb	%dl, %cl
	shldl	%cl, %esi, %eax
	movl	(%esp), %ecx
	movl	%eax, 4(%esp)
	xorl	%eax, %eax
	testb	$32, %dl
	movl	%ecx, 8(%esp)
	movl	%eax, 12(%esp)
	jne	.LBB5_2
	movl	(%esp), %eax
	movl	4(%esp), %ecx
	movl	%ecx, 8(%esp)
	movl	%eax, 12(%esp)
.LBB5_2:
	movl	8(%esp), %edx
	movl	12(%esp), %eax
	addl	$16, %esp
	.cfi_def_cfa_offset 12
	popl	%esi
	.cfi_def_cfa_offset 8
	popl	%edi
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
	pushl	%edi
	.cfi_def_cfa_offset 8
	pushl	%esi
	.cfi_def_cfa_offset 12
	subl	$16, %esp
	.cfi_def_cfa_offset 28
	.cfi_offset %esi, -12
	.cfi_offset %edi, -8
	movb	36(%esp), %dl
	movl	28(%esp), %esi
	movl	32(%esp), %eax
	movb	%dl, %cl
	movl	%eax, %edi
	sarl	%cl, %edi
	movl	%edi, (%esp)
	movb	%dl, %cl
	shrdl	%cl, %eax, %esi
	movl	(%esp), %ecx
	movl	%esi, 4(%esp)
	sarl	$31, %eax
	testb	$32, %dl
	movl	%ecx, 8(%esp)
	movl	%eax, 12(%esp)
	jne	.LBB6_2
	movl	(%esp), %eax
	movl	4(%esp), %ecx
	movl	%ecx, 8(%esp)
	movl	%eax, 12(%esp)
.LBB6_2:
	movl	8(%esp), %eax
	movl	12(%esp), %edx
	addl	$16, %esp
	.cfi_def_cfa_offset 12
	popl	%esi
	.cfi_def_cfa_offset 8
	popl	%edi
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
	fldl	24(%esp)
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

	.section	.rodata.cst4,"aM",@progbits,4
	.p2align	2, 0x0
.LCPI8_0:
	.long	0x5f000000
	.text
	.globl	f2u
	.p2align	4
	.type	f2u,@function
f2u:
	.cfi_startproc
	subl	$44, %esp
	.cfi_def_cfa_offset 48
	fldl	48(%esp)
	fstl	(%esp)
	flds	.LCPI8_0
	fucom	%st(1)
	fstp	%st(1)
	fnstsw	%ax
	sahf
	setbe	%al
	movb	%al, 11(%esp)
	fldz
	fstpl	12(%esp)
	fstpl	20(%esp)
	jbe	.LBB8_2
	fldl	12(%esp)
	fstpl	20(%esp)
.LBB8_2:
	movb	11(%esp), %al
	fldl	(%esp)
	fldl	20(%esp)
	fsubrp	%st, %st(1)
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
	fildll	4(%esp)
	retl
.Lfunc_end9:
	.size	s2f, .Lfunc_end9-s2f
	.cfi_endproc

	.section	.rodata.cst8,"aM",@progbits,8
	.p2align	3, 0x0
.LCPI10_0:
	.quad	6881500230622117888
	.text
	.globl	u2f
	.p2align	4
	.type	u2f,@function
u2f:
	.cfi_startproc
	subl	$20, %esp
	.cfi_def_cfa_offset 24
	movl	24(%esp), %ecx
	movl	28(%esp), %eax
	movl	%eax, 12(%esp)
	movl	%ecx, 8(%esp)
	shrl	$31, %eax
	fildll	8(%esp)
	flds	.LCPI10_0(,%eax,4)
	faddp	%st, %st(1)
	fstpl	(%esp)
	fldl	(%esp)
	addl	$20, %esp
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
	subl	$8, %esp
	.cfi_def_cfa_offset 12
	fldl	12(%esp)
	fnstcw	2(%esp)
	movzwl	2(%esp), %eax
	orl	$3072, %eax
	movw	%ax, (%esp)
	fldcw	(%esp)
	fistpl	4(%esp)
	fldcw	2(%esp)
	movl	4(%esp), %eax
	addl	$8, %esp
	.cfi_def_cfa_offset 4
	retl
.Lfunc_end11:
	.size	f2s32, .Lfunc_end11-f2s32
	.cfi_endproc

	.globl	frem
	.p2align	4
	.type	frem,@function
frem:
	.cfi_startproc
	subl	$28, %esp
	.cfi_def_cfa_offset 32
	fldl	32(%esp)
	fldl	40(%esp)
	movl	%esp, %eax
	fstpl	8(%eax)
	fstpl	(%eax)
	calll	fmod
	addl	$28, %esp
	.cfi_def_cfa_offset 4
	retl
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

	.globl	ctpop
	.p2align	4
	.type	ctpop,@function
ctpop:
	.cfi_startproc
	movl	4(%esp), %edx
	movl	8(%esp), %ecx
	movl	%ecx, %eax
	shrl	%eax
	andl	$1431655765, %eax
	subl	%eax, %ecx
	movl	%ecx, %eax
	andl	$858993459, %eax
	shrl	$2, %ecx
	andl	$858993459, %ecx
	addl	%ecx, %eax
	movl	%eax, %ecx
	shrl	$4, %ecx
	addl	%ecx, %eax
	andl	$252645135, %eax
	imull	$16843009, %eax, %ecx
	shrl	$24, %ecx
	movl	%edx, %eax
	shrl	%eax
	andl	$1431655765, %eax
	subl	%eax, %edx
	movl	%edx, %eax
	andl	$858993459, %eax
	shrl	$2, %edx
	andl	$858993459, %edx
	addl	%edx, %eax
	movl	%eax, %edx
	shrl	$4, %edx
	addl	%edx, %eax
	andl	$252645135, %eax
	imull	$16843009, %eax, %eax
	shrl	$24, %eax
	addl	%ecx, %eax
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
