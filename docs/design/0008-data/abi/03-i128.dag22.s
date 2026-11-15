	.file	"03-i128.ll"
	.text
	.globl	r128
	.p2align	4
	.type	r128,@function
r128:
	.cfi_startproc
	pushl	%ebp
	.cfi_def_cfa_offset 8
	pushl	%ebx
	.cfi_def_cfa_offset 12
	pushl	%edi
	.cfi_def_cfa_offset 16
	pushl	%esi
	.cfi_def_cfa_offset 20
	subl	$12, %esp
	.cfi_def_cfa_offset 32
	.cfi_offset %esi, -20
	.cfi_offset %edi, -16
	.cfi_offset %ebx, -12
	.cfi_offset %ebp, -8
	movl	32(%esp), %ecx
	movl	%ecx, %eax
	movl	%eax, 8(%esp)
	movl	72(%esp), %eax
	movl	%eax, 4(%esp)
	movl	76(%esp), %ebp
	movl	56(%esp), %esi
	movl	60(%esp), %edx
	movl	64(%esp), %eax
	movl	68(%esp), %edi
	movl	%edi, (%esp)
	movl	48(%esp), %edi
	movl	52(%esp), %ebx
	addl	%eax, %edi
	movl	(%esp), %eax
	adcl	%eax, %ebx
	movl	4(%esp), %eax
	adcl	%eax, %esi
	movl	8(%esp), %eax
	adcl	%ebp, %edx
	movl	%ebx, 4(%ecx)
	movl	%edi, (%ecx)
	movl	%esi, 8(%ecx)
	movl	%edx, 12(%ecx)
	addl	$12, %esp
	.cfi_def_cfa_offset 20
	popl	%esi
	.cfi_def_cfa_offset 16
	popl	%edi
	.cfi_def_cfa_offset 12
	popl	%ebx
	.cfi_def_cfa_offset 8
	popl	%ebp
	.cfi_def_cfa_offset 4
	retl	$4
.Lfunc_end0:
	.size	r128, .Lfunc_end0-r128
	.cfi_endproc

	.globl	c128
	.p2align	4
	.type	c128,@function
c128:
	.cfi_startproc
	pushl	%ebx
	.cfi_def_cfa_offset 8
	pushl	%edi
	.cfi_def_cfa_offset 12
	pushl	%esi
	.cfi_def_cfa_offset 16
	subl	$64, %esp
	.cfi_def_cfa_offset 80
	.cfi_offset %esi, -16
	.cfi_offset %edi, -12
	.cfi_offset %ebx, -8
	movl	80(%esp), %eax
	movl	%eax, 40(%esp)
	movl	%eax, 44(%esp)
	movl	%esp, %eax
	leal	48(%esp), %ecx
	movl	%ecx, (%eax)
	movl	$0, 28(%eax)
	movl	$0, 24(%eax)
	movl	$0, 20(%eax)
	movl	$9, 16(%eax)
	movl	$7, 4(%eax)
	calll	e128@PLT
	subl	$4, %esp
	movl	40(%esp), %ecx
	movl	44(%esp), %eax
	movl	48(%esp), %edx
	movl	52(%esp), %esi
	movl	56(%esp), %edi
	movl	60(%esp), %ebx
	movl	%ebx, 12(%ecx)
	movl	%edi, 8(%ecx)
	movl	%esi, 4(%ecx)
	movl	%edx, (%ecx)
	addl	$64, %esp
	.cfi_def_cfa_offset 16
	popl	%esi
	.cfi_def_cfa_offset 12
	popl	%edi
	.cfi_def_cfa_offset 8
	popl	%ebx
	.cfi_def_cfa_offset 4
	retl	$4
.Lfunc_end1:
	.size	c128, .Lfunc_end1-c128
	.cfi_endproc

	.globl	mul128
	.p2align	4
	.type	mul128,@function
mul128:
	.cfi_startproc
	pushl	%ebp
	.cfi_def_cfa_offset 8
	pushl	%ebx
	.cfi_def_cfa_offset 12
	pushl	%edi
	.cfi_def_cfa_offset 16
	pushl	%esi
	.cfi_def_cfa_offset 20
	subl	$60, %esp
	.cfi_def_cfa_offset 80
	.cfi_offset %esi, -20
	.cfi_offset %edi, -16
	.cfi_offset %ebx, -12
	.cfi_offset %ebp, -8
	movl	80(%esp), %ecx
	movl	%ecx, %eax
	movl	%eax, 56(%esp)
	movl	96(%esp), %eax
	movl	%eax, 20(%esp)
	movl	100(%esp), %ebp
	movl	%ebp, 40(%esp)
	movl	120(%esp), %eax
	movl	%eax, (%esp)
	movl	124(%esp), %eax
	movl	%eax, 8(%esp)
	movl	104(%esp), %edx
	movl	%edx, 4(%esp)
	movl	108(%esp), %esi
	movl	112(%esp), %eax
	movl	%eax, 16(%esp)
	movl	116(%esp), %edi
	movl	%edi, 32(%esp)
	movl	%eax, %edi
	imull	%esi, %edi
	mull	%edx
	movl	%eax, %ebx
	movl	(%esp), %eax
	movl	%edx, %esi
	movl	4(%esp), %edx
	addl	%edi, %esi
	movl	32(%esp), %edi
	imull	%edx, %edi
	movl	20(%esp), %edx
	addl	%edi, %esi
	movl	%eax, %edi
	imull	%ebp, %edi
	movl	%edi, 12(%esp)
	mull	%edx
	movl	%eax, %edi
	movl	8(%esp), %eax
	movl	%edx, %ebp
	movl	12(%esp), %edx
	addl	%edx, %ebp
	movl	20(%esp), %edx
	imull	%edx, %eax
	movl	16(%esp), %edx
	addl	%eax, %ebp
	movl	20(%esp), %eax
	addl	%ebx, %edi
	movl	%edi, 52(%esp)
	adcl	%esi, %ebp
	mull	%edx
	movl	%eax, %edi
	movl	40(%esp), %eax
	movl	%edx, %esi
	movl	16(%esp), %edx
	movl	%esi, 24(%esp)
	mull	%edx
	movl	%eax, %ebx
	movl	20(%esp), %eax
	movl	%edx, %esi
	movl	24(%esp), %edx
	addl	%edx, %ebx
	movl	32(%esp), %edx
	movl	%ebx, 28(%esp)
	adcl	$0, %esi
	movl	%esi, 36(%esp)
	mull	%edx
	movl	%eax, %ebx
	movl	28(%esp), %eax
	movl	%edx, %esi
	movl	32(%esp), %edx
	addl	%eax, %ebx
	movl	36(%esp), %eax
	adcl	%esi, %eax
	movl	%eax, 44(%esp)
	setb	%al
	movzbl	%al, %esi
	movl	40(%esp), %eax
	movl	%esi, 48(%esp)
	mull	%edx
	movl	%eax, %esi
	movl	44(%esp), %eax
	addl	%eax, %esi
	movl	48(%esp), %eax
	adcl	%eax, %edx
	movl	52(%esp), %eax
	addl	%eax, %esi
	movl	56(%esp), %eax
	adcl	%ebp, %edx
	movl	%ebx, 4(%ecx)
	movl	%edi, (%ecx)
	movl	%esi, 8(%ecx)
	movl	%edx, 12(%ecx)
	addl	$60, %esp
	.cfi_def_cfa_offset 20
	popl	%esi
	.cfi_def_cfa_offset 16
	popl	%edi
	.cfi_def_cfa_offset 12
	popl	%ebx
	.cfi_def_cfa_offset 8
	popl	%ebp
	.cfi_def_cfa_offset 4
	retl	$4
.Lfunc_end2:
	.size	mul128, .Lfunc_end2-mul128
	.cfi_endproc

	.globl	div128
	.p2align	4
	.type	div128,@function
div128:
	.cfi_startproc
	pushl	%ebp
	.cfi_def_cfa_offset 8
	pushl	%ebx
	.cfi_def_cfa_offset 12
	pushl	%edi
	.cfi_def_cfa_offset 16
	pushl	%esi
	.cfi_def_cfa_offset 20
	subl	$556, %esp
	.cfi_def_cfa_offset 576
	.cfi_offset %esi, -20
	.cfi_offset %edi, -16
	.cfi_offset %ebx, -12
	.cfi_offset %ebp, -8
	movl	608(%esp), %edx
	movl	612(%esp), %eax
	movl	%eax, 404(%esp)
	movl	616(%esp), %esi
	movl	620(%esp), %eax
	movl	%eax, 408(%esp)
	movl	592(%esp), %eax
	movl	596(%esp), %edi
	movl	600(%esp), %ebx
	movl	604(%esp), %ebp
	movl	576(%esp), %ecx
	movl	%ecx, 396(%esp)
	movl	%ecx, 400(%esp)
	movl	%ebp, %ecx
	sarl	$31, %ecx
	movl	%ecx, 432(%esp)
	xorl	%ecx, %ebp
	xorl	%ecx, %ebx
	xorl	%ecx, %edi
	xorl	%ecx, %eax
	subl	%ecx, %eax
	movl	404(%esp), %ecx
	movl	%eax, 424(%esp)
	movl	432(%esp), %eax
	sbbl	%eax, %edi
	movl	408(%esp), %eax
	movl	%edi, 420(%esp)
	movl	432(%esp), %edi
	sbbl	%edi, %ebx
	movl	%ebx, 412(%esp)
	sbbl	%edi, %ebp
	movl	%ebp, 416(%esp)
	movl	%eax, %edi
	sarl	$31, %edi
	movl	%edi, 436(%esp)
	xorl	%edi, %eax
	xorl	%edi, %esi
	xorl	%edi, %ecx
	xorl	%edi, %edx
	subl	%edi, %edx
	movl	420(%esp), %edi
	movl	%edx, 460(%esp)
	movl	436(%esp), %edx
	sbbl	%edx, %ecx
	movl	424(%esp), %edx
	movl	%ecx, 440(%esp)
	movl	436(%esp), %ecx
	sbbl	%ecx, %esi
	movl	%esi, 428(%esp)
	sbbl	%ecx, %eax
	movl	432(%esp), %ecx
	movl	%eax, 464(%esp)
	movl	436(%esp), %eax
	xorl	%ecx, %eax
	movl	440(%esp), %ecx
	movl	%eax, 452(%esp)
	movl	%eax, 444(%esp)
	movl	452(%esp), %eax
	movl	%eax, 448(%esp)
	movl	452(%esp), %eax
	movl	%eax, 456(%esp)
	movl	464(%esp), %eax
	orl	%eax, %ecx
	movl	460(%esp), %eax
	orl	%esi, %eax
	orl	%ecx, %eax
	movl	464(%esp), %ecx
	sete	%al
	orl	%ebp, %edi
	orl	%ebx, %edx
	orl	%edi, %edx
	sete	%dl
	orb	%dl, %al
	movb	%al, 471(%esp)
	bsrl	%ecx, %eax
	xorl	$31, %eax
	bsrl	%esi, %edx
	xorl	$31, %edx
	orl	$32, %edx
	movl	%edx, 472(%esp)
	testl	%ecx, %ecx
	movl	%eax, 476(%esp)
	jne	.LBB3_8
	movl	472(%esp), %eax
	movl	%eax, 476(%esp)
.LBB3_8:
	movl	440(%esp), %ecx
	movl	460(%esp), %esi
	movl	476(%esp), %eax
	movl	%eax, 384(%esp)
	bsrl	%ecx, %eax
	xorl	$31, %eax
	bsrl	%esi, %edx
	xorl	$31, %edx
	orl	$32, %edx
	movl	%edx, 388(%esp)
	testl	%ecx, %ecx
	movl	%eax, 392(%esp)
	jne	.LBB3_10
	movl	388(%esp), %eax
	movl	%eax, 392(%esp)
.LBB3_10:
	movl	384(%esp), %eax
	movl	464(%esp), %edx
	movl	428(%esp), %ecx
	movl	392(%esp), %esi
	orl	$64, %esi
	movl	%esi, 376(%esp)
	orl	%edx, %ecx
	movl	%eax, 380(%esp)
	jne	.LBB3_12
	movl	376(%esp), %eax
	movl	%eax, 380(%esp)
.LBB3_12:
	movl	416(%esp), %ecx
	movl	412(%esp), %esi
	movl	380(%esp), %eax
	movl	%eax, 364(%esp)
	bsrl	%ecx, %eax
	xorl	$31, %eax
	bsrl	%esi, %edx
	xorl	$31, %edx
	orl	$32, %edx
	movl	%edx, 368(%esp)
	testl	%ecx, %ecx
	movl	%eax, 372(%esp)
	jne	.LBB3_14
	movl	368(%esp), %eax
	movl	%eax, 372(%esp)
.LBB3_14:
	movl	420(%esp), %ecx
	movl	424(%esp), %esi
	movl	372(%esp), %eax
	movl	%eax, 352(%esp)
	bsrl	%ecx, %eax
	xorl	$31, %eax
	bsrl	%esi, %edx
	xorl	$31, %edx
	orl	$32, %edx
	movl	%edx, 356(%esp)
	testl	%ecx, %ecx
	movl	%eax, 360(%esp)
	jne	.LBB3_16
	movl	356(%esp), %eax
	movl	%eax, 360(%esp)
.LBB3_16:
	movl	352(%esp), %eax
	movl	416(%esp), %edx
	movl	412(%esp), %ecx
	movl	360(%esp), %esi
	orl	$64, %esi
	movl	%esi, 344(%esp)
	orl	%edx, %ecx
	movl	%eax, 348(%esp)
	jne	.LBB3_18
	movl	344(%esp), %eax
	movl	%eax, 348(%esp)
.LBB3_18:
	movb	471(%esp), %cl
	movl	364(%esp), %ebp
	movl	348(%esp), %edx
	xorl	%eax, %eax
	movl	%eax, 308(%esp)
	subl	%edx, %ebp
	movl	%eax, %ebx
	sbbl	%ebx, %ebx
	movl	%eax, %edi
	sbbl	%edi, %edi
	movl	%eax, %esi
	sbbl	%esi, %esi
	movl	%esi, 312(%esp)
	movl	%edi, 316(%esp)
	movl	%ebx, 320(%esp)
	movl	%ebp, 324(%esp)
	movl	$127, %edx
	subl	%ebp, %edx
	movl	%eax, %edx
	sbbl	%ebx, %edx
	movl	%eax, %edx
	sbbl	%edi, %edx
	movl	%eax, %edx
	sbbl	%esi, %edx
	setb	%dl
	orb	%dl, %cl
	testb	%cl, %cl
	movl	%eax, %ecx
	movl	%ecx, 328(%esp)
	movl	%eax, %ecx
	movl	%ecx, 332(%esp)
	movl	%eax, %ecx
	movl	%ecx, 336(%esp)
	movl	%eax, 340(%esp)
	jne	.LBB3_20
	movl	424(%esp), %eax
	movl	420(%esp), %ecx
	movl	412(%esp), %edx
	movl	416(%esp), %esi
	movl	%esi, 328(%esp)
	movl	%edx, 332(%esp)
	movl	%ecx, 336(%esp)
	movl	%eax, 340(%esp)
.LBB3_20:
	movl	328(%esp), %eax
	movl	332(%esp), %ecx
	movl	336(%esp), %edx
	movl	340(%esp), %esi
	movl	%esi, 276(%esp)
	movl	%edx, 280(%esp)
	movl	%ecx, 284(%esp)
	movl	%eax, 288(%esp)
	movl	%esi, 292(%esp)
	movl	%edx, 296(%esp)
	movl	%ecx, 300(%esp)
	movl	%eax, 304(%esp)
	jne	.LBB3_5
	jmp	.LBB3_6
.LBB3_6:
	movl	284(%esp), %ecx
	movl	280(%esp), %edx
	movl	276(%esp), %esi
	movl	312(%esp), %ebp
	movl	320(%esp), %ebx
	movl	316(%esp), %eax
	movl	324(%esp), %edi
	xorl	$127, %edi
	orl	%eax, %edi
	movl	288(%esp), %eax
	orl	%ebp, %ebx
	orl	%ebx, %edi
	movl	%esi, 292(%esp)
	movl	%edx, 296(%esp)
	movl	%ecx, 300(%esp)
	movl	%eax, 304(%esp)
	je	.LBB3_5
	jmp	.LBB3_4
.LBB3_1:
	movl	264(%esp), %esi
	movl	260(%esp), %edx
	movl	256(%esp), %ecx
	movl	252(%esp), %eax
	movl	248(%esp), %ebp
	movl	244(%esp), %ebx
	movl	240(%esp), %edi
	movl	%ecx, 272(%esp)
	movl	268(%esp), %ecx
	shldl	$1, %ebx, %edi
	shldl	$1, %ebp, %ebx
	shldl	$1, %eax, %ebp
	addl	%eax, %eax
	orl	%eax, %esi
	movl	272(%esp), %eax
	orl	%ebp, %edx
	orl	%ebx, %ecx
	orl	%edi, %eax
	movl	%esi, 292(%esp)
	movl	%edx, 296(%esp)
	movl	%ecx, 300(%esp)
	movl	%eax, 304(%esp)
	jmp	.LBB3_5
.LBB3_2:
	movl	236(%esp), %eax
	movl	220(%esp), %edi
	movl	216(%esp), %ebx
	movl	212(%esp), %ebp
	movl	196(%esp), %ecx
	movl	148(%esp), %edx
	movl	140(%esp), %esi
	movl	%eax, 44(%esp)
	movl	232(%esp), %eax
	movl	%eax, 40(%esp)
	movl	228(%esp), %eax
	movl	%eax, 48(%esp)
	movl	224(%esp), %eax
	movl	%eax, 56(%esp)
	movl	204(%esp), %eax
	movl	%ecx, 100(%esp)
	movl	188(%esp), %ecx
	movl	%ecx, 124(%esp)
	movl	180(%esp), %ecx
	movl	%ecx, 120(%esp)
	movl	172(%esp), %ecx
	movl	%ecx, 116(%esp)
	movl	164(%esp), %ecx
	movl	%ecx, 72(%esp)
	movl	156(%esp), %ecx
	movl	%ecx, 64(%esp)
	movl	44(%esp), %ecx
	shldl	$1, %ebx, %edi
	shldl	$1, %ebp, %ebx
	shldl	$1, %eax, %ebp
	shldl	$1, %ecx, %eax
	movl	40(%esp), %ecx
	movl	%eax, 96(%esp)
	movl	44(%esp), %eax
	shldl	$1, %ecx, %eax
	movl	%eax, 68(%esp)
	movl	48(%esp), %eax
	shldl	$1, %eax, %ecx
	movl	%ecx, 52(%esp)
	movl	56(%esp), %ecx
	shldl	$1, %ecx, %eax
	movl	52(%esp), %ecx
	movl	%eax, 60(%esp)
	movl	56(%esp), %eax
	addl	%eax, %eax
	orl	%eax, %esi
	movl	60(%esp), %eax
	orl	%eax, %edx
	movl	64(%esp), %eax
	orl	%ecx, %eax
	movl	68(%esp), %ecx
	movl	%eax, 132(%esp)
	movl	72(%esp), %eax
	orl	%ecx, %eax
	movl	96(%esp), %ecx
	movl	%eax, 208(%esp)
	movl	76(%esp), %eax
	subl	%ecx, %eax
	movl	464(%esp), %ecx
	movl	80(%esp), %eax
	sbbl	%ebp, %eax
	movl	84(%esp), %eax
	sbbl	%ebx, %eax
	movl	88(%esp), %eax
	sbbl	%edi, %eax
	sarl	$31, %eax
	movl	%eax, 92(%esp)
	andl	$1, %eax
	movl	%eax, 136(%esp)
	xorl	%eax, %eax
	movl	%eax, 144(%esp)
	movl	%eax, 160(%esp)
	movl	144(%esp), %eax
	movl	%eax, 152(%esp)
	movl	92(%esp), %eax
	andl	%ecx, %eax
	movl	428(%esp), %ecx
	movl	%eax, 112(%esp)
	movl	92(%esp), %eax
	andl	%ecx, %eax
	movl	440(%esp), %ecx
	movl	%eax, 108(%esp)
	movl	92(%esp), %eax
	andl	%ecx, %eax
	movl	92(%esp), %ecx
	movl	%eax, 104(%esp)
	movl	460(%esp), %eax
	andl	%eax, %ecx
	movl	96(%esp), %eax
	subl	%ecx, %eax
	movl	100(%esp), %ecx
	movl	%eax, 200(%esp)
	movl	104(%esp), %eax
	sbbl	%eax, %ebp
	movl	108(%esp), %eax
	sbbl	%eax, %ebx
	movl	112(%esp), %eax
	sbbl	%eax, %edi
	movl	116(%esp), %eax
	addl	$-1, %eax
	movl	%eax, 168(%esp)
	movl	120(%esp), %eax
	adcl	$-1, %eax
	movl	%eax, 176(%esp)
	movl	124(%esp), %eax
	adcl	$-1, %eax
	movl	%eax, 184(%esp)
	movl	176(%esp), %eax
	adcl	$-1, %ecx
	movl	%ecx, 192(%esp)
	orl	%ecx, %eax
	movl	184(%esp), %ecx
	movl	%eax, 128(%esp)
	movl	168(%esp), %eax
	orl	%ecx, %eax
	movl	128(%esp), %ecx
	orl	%ecx, %eax
	movl	132(%esp), %ecx
	movl	136(%esp), %eax
	movl	%eax, 264(%esp)
	movl	144(%esp), %eax
	movl	%eax, 260(%esp)
	movl	152(%esp), %eax
	movl	%eax, 268(%esp)
	movl	160(%esp), %eax
	movl	%eax, 256(%esp)
	movl	%esi, %eax
	movl	%eax, 252(%esp)
	movl	%edx, %eax
	movl	%eax, 248(%esp)
	movl	%ecx, %eax
	movl	%eax, 244(%esp)
	movl	208(%esp), %eax
	movl	%eax, 240(%esp)
	movl	136(%esp), %eax
	movl	%eax, 140(%esp)
	movl	144(%esp), %eax
	movl	%eax, 148(%esp)
	movl	152(%esp), %eax
	movl	%eax, 156(%esp)
	movl	160(%esp), %eax
	movl	%eax, 164(%esp)
	movl	168(%esp), %eax
	movl	%eax, 172(%esp)
	movl	176(%esp), %eax
	movl	%eax, 180(%esp)
	movl	184(%esp), %eax
	movl	%eax, 188(%esp)
	movl	192(%esp), %eax
	movl	%eax, 196(%esp)
	movl	200(%esp), %eax
	movl	%eax, 204(%esp)
	movl	208(%esp), %eax
	movl	%ebp, 212(%esp)
	movl	%ebx, 216(%esp)
	movl	%edi, 220(%esp)
	movl	%esi, 224(%esp)
	movl	%edx, 228(%esp)
	movl	%ecx, 232(%esp)
	movl	%eax, 236(%esp)
	je	.LBB3_1
	jmp	.LBB3_2
.LBB3_3:
	movl	16(%esp), %eax
	movl	416(%esp), %ecx
	movl	412(%esp), %edx
	movl	420(%esp), %esi
	movl	424(%esp), %edi
	movl	%edi, 480(%esp)
	movl	%esi, 484(%esp)
	movl	%edx, 488(%esp)
	movl	%ecx, 492(%esp)
	movl	$0, 508(%esp)
	movl	$0, 504(%esp)
	movl	$0, 500(%esp)
	movl	$0, 496(%esp)
	movb	%al, %cl
	movb	%cl, 3(%esp)
	movb	%cl, %al
	shrb	$3, %al
	andb	$12, %al
	movzbl	%al, %edx
	movl	492(%esp,%edx), %edi
	movl	488(%esp,%edx), %esi
	movl	%esi, %ebx
	shrdl	%cl, %edi, %ebx
	movb	3(%esp), %cl
	movl	480(%esp,%edx), %eax
	movl	484(%esp,%edx), %edx
	movl	%edx, %ebp
	shrdl	%cl, %esi, %ebp
	movl	460(%esp), %esi
	movb	3(%esp), %cl
	shrl	%cl, %edi
	movb	3(%esp), %cl
	shrdl	%cl, %edx, %eax
	movl	440(%esp), %edx
	movl	428(%esp), %ecx
	movl	%eax, 4(%esp)
	movl	464(%esp), %eax
	addl	$-1, %esi
	movl	%esi, 76(%esp)
	adcl	$-1, %edx
	movl	%edx, 80(%esp)
	adcl	$-1, %ecx
	movl	%ecx, 84(%esp)
	adcl	$-1, %eax
	movl	%eax, 88(%esp)
	xorl	%ecx, %ecx
	movl	%ecx, %edx
	movl	%ecx, %esi
	movl	%ecx, %eax
	movl	%eax, 140(%esp)
	movl	4(%esp), %eax
	movl	%esi, 148(%esp)
	movl	8(%esp), %esi
	movl	%edx, 156(%esp)
	movl	12(%esp), %edx
	movl	%ecx, 164(%esp)
	movl	16(%esp), %ecx
	movl	%ecx, 172(%esp)
	movl	20(%esp), %ecx
	movl	%ecx, 180(%esp)
	movl	24(%esp), %ecx
	movl	%ecx, 188(%esp)
	movl	28(%esp), %ecx
	movl	%ecx, 196(%esp)
	movl	32(%esp), %ecx
	movl	%eax, 204(%esp)
	movl	36(%esp), %eax
	movl	%ebp, 212(%esp)
	movl	%ebx, 216(%esp)
	movl	%edi, 220(%esp)
	movl	%esi, 224(%esp)
	movl	%edx, 228(%esp)
	movl	%ecx, 232(%esp)
	movl	%eax, 236(%esp)
	jmp	.LBB3_2
.LBB3_4:
	movl	324(%esp), %ecx
	movl	416(%esp), %esi
	movl	412(%esp), %ebx
	movl	420(%esp), %ebp
	movl	312(%esp), %edi
	movl	320(%esp), %edx
	movl	%ecx, %eax
	addl	$1, %eax
	movl	%eax, 16(%esp)
	movl	316(%esp), %eax
	adcl	$0, %edx
	movl	%edx, 20(%esp)
	adcl	$0, %eax
	movl	%eax, 24(%esp)
	movl	424(%esp), %eax
	adcl	$0, %edi
	movl	%edi, 28(%esp)
	movl	%eax, 528(%esp)
	movl	24(%esp), %eax
	movl	%ebp, 532(%esp)
	movl	%ebx, 536(%esp)
	movl	%esi, 540(%esp)
	movl	$0, 524(%esp)
	movl	$0, 520(%esp)
	movl	$0, 516(%esp)
	movl	$0, 512(%esp)
	xorb	$127, %cl
	movb	%cl, 2(%esp)
	movb	%cl, %ch
	shrb	$3, %ch
	andb	$12, %ch
	negb	%ch
	movsbl	%ch, %ebx
	movl	536(%esp,%ebx), %ebp
	movl	540(%esp,%ebx), %esi
	shldl	%cl, %ebp, %esi
	movb	2(%esp), %cl
	movl	%esi, 36(%esp)
	movl	528(%esp,%ebx), %esi
	movl	532(%esp,%ebx), %ebx
	shldl	%cl, %ebx, %ebp
	movb	2(%esp), %cl
	movl	%ebp, 32(%esp)
	shldl	%cl, %esi, %ebx
	movb	2(%esp), %cl
	movl	%ebx, 12(%esp)
	shll	%cl, %esi
	movl	16(%esp), %ecx
	movl	%esi, 8(%esp)
	orl	%edi, %edx
	orl	%eax, %ecx
	xorl	%edi, %edi
	movl	%edi, %ebx
	movl	%edi, %ebp
	movl	%edi, %eax
	orl	%edx, %ecx
	movl	12(%esp), %edx
	movl	32(%esp), %ecx
	movl	%eax, 264(%esp)
	movl	36(%esp), %eax
	movl	%ebp, 260(%esp)
	movl	%ebx, 268(%esp)
	movl	%edi, 256(%esp)
	movl	%esi, 252(%esp)
	movl	%edx, 248(%esp)
	movl	%ecx, 244(%esp)
	movl	%eax, 240(%esp)
	je	.LBB3_1
	jmp	.LBB3_3
.LBB3_5:
	movl	396(%esp), %ecx
	movl	452(%esp), %ebp
	movl	444(%esp), %eax
	movl	292(%esp), %ebx
	movl	296(%esp), %edi
	movl	300(%esp), %esi
	movl	304(%esp), %edx
	xorl	%ebp, %edx
	xorl	%eax, %esi
	movl	448(%esp), %eax
	xorl	%eax, %edi
	movl	456(%esp), %eax
	xorl	%eax, %ebx
	subl	%eax, %ebx
	movl	448(%esp), %eax
	sbbl	%eax, %edi
	movl	444(%esp), %eax
	sbbl	%eax, %esi
	movl	400(%esp), %eax
	sbbl	%ebp, %edx
	movl	%ebx, (%ecx)
	movl	%edi, 4(%ecx)
	movl	%esi, 8(%ecx)
	movl	%edx, 12(%ecx)
	addl	$556, %esp
	.cfi_def_cfa_offset 20
	popl	%esi
	.cfi_def_cfa_offset 16
	popl	%edi
	.cfi_def_cfa_offset 12
	popl	%ebx
	.cfi_def_cfa_offset 8
	popl	%ebp
	.cfi_def_cfa_offset 4
	retl	$4
.Lfunc_end3:
	.size	div128, .Lfunc_end3-div128
	.cfi_endproc

	.section	".note.GNU-stack","",@progbits
