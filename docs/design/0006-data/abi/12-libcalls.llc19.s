	.text
	.file	"12-libcalls.ll"
	.globl	q_sin
	.p2align	4, 0x90
	.type	q_sin,@function
q_sin:
	.cfi_startproc
	jmp	sinf128@PLT
.Lfunc_end0:
	.size	q_sin, .Lfunc_end0-q_sin
	.cfi_endproc

	.globl	q_rem
	.p2align	4, 0x90
	.type	q_rem,@function
q_rem:
	.cfi_startproc
	jmp	fmodf128@PLT
.Lfunc_end1:
	.size	q_rem, .Lfunc_end1-q_rem
	.cfi_endproc

	.globl	q_add
	.p2align	4, 0x90
	.type	q_add,@function
q_add:
	.cfi_startproc
	jmp	__addtf3@PLT
.Lfunc_end2:
	.size	q_add, .Lfunc_end2-q_add
	.cfi_endproc

	.globl	l_sin
	.p2align	4, 0x90
	.type	l_sin,@function
l_sin:
	.cfi_startproc
	subq	$24, %rsp
	.cfi_def_cfa_offset 32
	fldt	32(%rsp)
	movq	%rsp, %rax
	fstpt	(%rax)
	callq	sinl@PLT
	addq	$24, %rsp
	.cfi_def_cfa_offset 8
	retq
.Lfunc_end3:
	.size	l_sin, .Lfunc_end3-l_sin
	.cfi_endproc

	.globl	l_rem
	.p2align	4, 0x90
	.type	l_rem,@function
l_rem:
	.cfi_startproc
	subq	$40, %rsp
	.cfi_def_cfa_offset 48
	fldt	48(%rsp)
	fldt	64(%rsp)
	movq	%rsp, %rax
	fstpt	16(%rax)
	fstpt	(%rax)
	callq	fmodl@PLT
	addq	$40, %rsp
	.cfi_def_cfa_offset 8
	retq
.Lfunc_end4:
	.size	l_rem, .Lfunc_end4-l_rem
	.cfi_endproc

	.globl	l_sqrt
	.p2align	4, 0x90
	.type	l_sqrt,@function
l_sqrt:
	.cfi_startproc
	fldt	8(%rsp)
	fsqrt
	retq
.Lfunc_end5:
	.size	l_sqrt, .Lfunc_end5-l_sqrt
	.cfi_endproc

	.globl	l_from_q
	.p2align	4, 0x90
	.type	l_from_q,@function
l_from_q:
	.cfi_startproc
	pushq	%rax
	.cfi_def_cfa_offset 16
	callq	__trunctfxf2@PLT
	popq	%rax
	.cfi_def_cfa_offset 8
	retq
.Lfunc_end6:
	.size	l_from_q, .Lfunc_end6-l_from_q
	.cfi_endproc

	.section	.rodata.cst8,"aM",@progbits,8
	.p2align	3, 0x0
.LCPI7_0:
	.quad	6881500230622117888
	.text
	.globl	l_from_u64
	.p2align	4, 0x90
	.type	l_from_u64,@function
l_from_u64:
	.cfi_startproc
	movq	%rdi, -8(%rsp)
	testq	%rdi, %rdi
	sets	%al
	movzbl	%al, %eax
	movl	%eax, %ecx
	fildll	-8(%rsp)
	leaq	.LCPI7_0(%rip), %rax
	flds	(%rax,%rcx,4)
	faddp	%st, %st(1)
	retq
.Lfunc_end7:
	.size	l_from_u64, .Lfunc_end7-l_from_u64
	.cfi_endproc

	.globl	l_from_i128
	.p2align	4, 0x90
	.type	l_from_i128,@function
l_from_i128:
	.cfi_startproc
	pushq	%rax
	.cfi_def_cfa_offset 16
	callq	__floattixf@PLT
	popq	%rax
	.cfi_def_cfa_offset 8
	retq
.Lfunc_end8:
	.size	l_from_i128, .Lfunc_end8-l_from_i128
	.cfi_endproc

	.section	.rodata.cst16,"aM",@progbits,16
	.p2align	4, 0x0
.LCPI9_0:
	.long	1127219200
	.long	1160773632
	.long	0
	.long	0
.LCPI9_1:
	.quad	0x4330000000000000
	.quad	0x4530000000000000
	.text
	.globl	d_from_u64
	.p2align	4, 0x90
	.type	d_from_u64,@function
d_from_u64:
	.cfi_startproc
	movq	%rdi, %xmm0
	movaps	.LCPI9_0(%rip), %xmm1
	punpckldq	%xmm1, %xmm0
	movapd	.LCPI9_1(%rip), %xmm1
	subpd	%xmm1, %xmm0
	movaps	%xmm0, %xmm1
	unpckhpd	%xmm0, %xmm0
	addsd	%xmm1, %xmm0
	retq
.Lfunc_end9:
	.size	d_from_u64, .Lfunc_end9-d_from_u64
	.cfi_endproc

	.section	.rodata.cst8,"aM",@progbits,8
	.p2align	3, 0x0
.LCPI10_0:
	.quad	0x43e0000000000000
	.text
	.globl	d_to_u64
	.p2align	4, 0x90
	.type	d_to_u64,@function
d_to_u64:
	.cfi_startproc
	movsd	.LCPI10_0(%rip), %xmm2
	movaps	%xmm0, %xmm1
	subsd	%xmm2, %xmm1
	cvttsd2si	%xmm1, %rcx
	cvttsd2si	%xmm0, %rax
	movq	%rax, %rdx
	sarq	$63, %rdx
	andq	%rdx, %rcx
	orq	%rcx, %rax
	retq
.Lfunc_end10:
	.size	d_to_u64, .Lfunc_end10-d_to_u64
	.cfi_endproc

	.globl	d_rem
	.p2align	4, 0x90
	.type	d_rem,@function
d_rem:
	.cfi_startproc
	jmp	fmod@PLT
.Lfunc_end11:
	.size	d_rem, .Lfunc_end11-d_rem
	.cfi_endproc

	.globl	h_add
	.p2align	4, 0x90
	.type	h_add,@function
h_add:
	.cfi_startproc
	subq	$24, %rsp
	.cfi_def_cfa_offset 32
	movss	%xmm1, 12(%rsp)
	movaps	%xmm0, %xmm1
	movss	12(%rsp), %xmm0
	movss	%xmm1, 16(%rsp)
	callq	__extendhfsf2@PLT
	movaps	%xmm0, %xmm1
	movss	16(%rsp), %xmm0
	movss	%xmm1, 20(%rsp)
	callq	__extendhfsf2@PLT
	movss	20(%rsp), %xmm1
	addss	%xmm1, %xmm0
	callq	__truncsfhf2@PLT
	addq	$24, %rsp
	.cfi_def_cfa_offset 8
	retq
.Lfunc_end12:
	.size	h_add, .Lfunc_end12-h_add
	.cfi_endproc

	.globl	ctlz32
	.p2align	4, 0x90
	.type	ctlz32,@function
ctlz32:
	.cfi_startproc
	bsrl	%edi, %eax
	movl	$63, %ecx
	cmovel	%ecx, %eax
	xorl	$31, %eax
	retq
.Lfunc_end13:
	.size	ctlz32, .Lfunc_end13-ctlz32
	.cfi_endproc

	.globl	pop32
	.p2align	4, 0x90
	.type	pop32,@function
pop32:
	.cfi_startproc
	movl	%edi, %ecx
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
	imull	$16843009, %eax, %eax
	shrl	$24, %eax
	retq
.Lfunc_end14:
	.size	pop32, .Lfunc_end14-pop32
	.cfi_endproc

	.globl	d_min
	.p2align	4, 0x90
	.type	d_min,@function
d_min:
	.cfi_startproc
	movaps	%xmm1, %xmm2
	movaps	%xmm0, %xmm3
	movaps	%xmm2, %xmm4
	movaps	%xmm3, %xmm0
	cmpunordsd	%xmm0, %xmm0
	movaps	%xmm0, %xmm1
	pand	%xmm4, %xmm1
	minsd	%xmm3, %xmm2
	pandn	%xmm2, %xmm0
	por	%xmm1, %xmm0
	retq
.Lfunc_end15:
	.size	d_min, .Lfunc_end15-d_min
	.cfi_endproc

	.globl	d_floor
	.p2align	4, 0x90
	.type	d_floor,@function
d_floor:
	.cfi_startproc
	jmp	floor@PLT
.Lfunc_end16:
	.size	d_floor, .Lfunc_end16-d_floor
	.cfi_endproc

	.globl	a128
	.p2align	4, 0x90
	.type	a128,@function
a128:
	.cfi_startproc
	pushq	%rax
	.cfi_def_cfa_offset 16
	movl	$5, %ecx
	callq	__atomic_fetch_add_16@PLT
	popq	%rcx
	.cfi_def_cfa_offset 8
	retq
.Lfunc_end17:
	.size	a128, .Lfunc_end17-a128
	.cfi_endproc

	.globl	c128
	.p2align	4, 0x90
	.type	c128,@function
c128:
	.cfi_startproc
	subq	$40, %rsp
	.cfi_def_cfa_offset 48
	movq	%r8, (%rsp)
	movq	%rcx, %rax
	movq	(%rsp), %rcx
	movq	%rax, 8(%rsp)
	movq	%rdx, %rax
	movq	8(%rsp), %rdx
	movq	%rax, 24(%rsp)
	movq	%rsi, 16(%rsp)
	leaq	16(%rsp), %rsi
	movl	$5, %r9d
	movl	%r9d, %r8d
	callq	__atomic_compare_exchange_16@PLT
	movq	16(%rsp), %rax
	movq	24(%rsp), %rdx
	addq	$40, %rsp
	.cfi_def_cfa_offset 8
	retq
.Lfunc_end18:
	.size	c128, .Lfunc_end18-c128
	.cfi_endproc

	.globl	l128
	.p2align	4, 0x90
	.type	l128,@function
l128:
	.cfi_startproc
	pushq	%rax
	.cfi_def_cfa_offset 16
	movl	$5, %esi
	callq	__atomic_load_16@PLT
	popq	%rcx
	.cfi_def_cfa_offset 8
	retq
.Lfunc_end19:
	.size	l128, .Lfunc_end19-l128
	.cfi_endproc

	.globl	a64
	.p2align	4, 0x90
	.type	a64,@function
a64:
	.cfi_startproc
	movq	%rsi, -24(%rsp)
	movq	%rdi, -16(%rsp)
	movq	(%rdi), %rax
	movq	%rax, -8(%rsp)
	jmp	.LBB20_1
.LBB20_1:
	movq	-8(%rsp), %rax
	movq	-16(%rsp), %rcx
	movq	-24(%rsp), %rsi
	movq	%rax, %rdx
	andq	%rsi, %rdx
	notq	%rdx
	lock		cmpxchgq	%rdx, (%rcx)
	movq	%rax, -32(%rsp)
	movq	%rax, -8(%rsp)
	jne	.LBB20_1
	jmp	.LBB20_2
.LBB20_2:
	movq	-32(%rsp), %rax
	retq
.Lfunc_end20:
	.size	a64, .Lfunc_end20-a64
	.cfi_endproc

	.globl	s64
	.p2align	4, 0x90
	.type	s64,@function
s64:
	.cfi_startproc
	xchgq	%rsi, (%rdi)
	retq
.Lfunc_end21:
	.size	s64, .Lfunc_end21-s64
	.cfi_endproc

	.section	".note.GNU-stack","",@progbits
