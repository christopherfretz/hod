	.file	"09-globals.ll"
	.text
	.p2align	4
	.type	loc,@function
loc:
	.cfi_startproc
	retl
.Lfunc_end0:
	.size	loc, .Lfunc_end0-loc
	.cfi_endproc

	.globl	ld
	.p2align	4
	.type	ld,@function
ld:
	.cfi_startproc
	movl	g, %eax
	movl	l, %edx
	movl	h, %ecx
	addl	%edx, %eax
	addl	%ecx, %eax
	retl
.Lfunc_end1:
	.size	ld, .Lfunc_end1-ld
	.cfi_endproc

	.globl	calls
	.p2align	4
	.type	calls,@function
calls:
	.cfi_startproc
	subl	$12, %esp
	.cfi_def_cfa_offset 16
	calll	ext@PLT
	calll	exth
	calll	loc
	addl	$12, %esp
	.cfi_def_cfa_offset 4
	retl
.Lfunc_end2:
	.size	calls, .Lfunc_end2-calls
	.cfi_endproc

	.globl	addr
	.p2align	4
	.type	addr,@function
addr:
	.cfi_startproc
	movl	$ext, %eax
	retl
.Lfunc_end3:
	.size	addr, .Lfunc_end3-addr
	.cfi_endproc

	.globl	addrl
	.p2align	4
	.type	addrl,@function
addrl:
	.cfi_startproc
	movl	$loc, %eax
	retl
.Lfunc_end4:
	.size	addrl, .Lfunc_end4-addrl
	.cfi_endproc

	.section	.rodata.cst4,"aM",@progbits,4
	.p2align	2, 0x0
.LCPI5_0:
	.long	0x40600000
	.text
	.globl	cst
	.p2align	4
	.type	cst,@function
cst:
	.cfi_startproc
	fldl	4(%esp)
	flds	.LCPI5_0
	faddp	%st, %st(1)
	retl
.Lfunc_end5:
	.size	cst, .Lfunc_end5-cst
	.cfi_endproc

	.globl	sw
	.p2align	4
	.type	sw,@function
sw:
	.cfi_startproc
	subl	$12, %esp
	.cfi_def_cfa_offset 16
	movl	16(%esp), %eax
	movl	%eax, 8(%esp)
	subl	$4, %eax
	ja	.LBB6_5
	movl	8(%esp), %eax
	movl	.LJTI6_0(,%eax,4), %eax
	jmpl	*%eax
.LBB6_1:
	calll	ext@PLT
	jmp	.LBB6_5
.LBB6_2:
	calll	exth
	jmp	.LBB6_5
.LBB6_3:
	calll	loc
	jmp	.LBB6_5
.LBB6_4:
	calll	ext@PLT
	jmp	.LBB6_5
.LBB6_5:
	addl	$12, %esp
	.cfi_def_cfa_offset 4
	retl
.Lfunc_end6:
	.size	sw, .Lfunc_end6-sw
	.cfi_endproc
	.section	.rodata,"a",@progbits
	.p2align	2, 0x0
.LJTI6_0:
	.long	.LBB6_1
	.long	.LBB6_2
	.long	.LBB6_3
	.long	.LBB6_4
	.long	.LBB6_1

	.type	l,@object
	.local	l
	.comm	l,4,4
	.hidden	h
	.type	h,@object
	.bss
	.globl	h
	.p2align	2, 0x0
h:
	.long	0
	.size	h, 4

	.type	d,@object
	.globl	d
	.p2align	2, 0x0
d:
	.long	0
	.size	d, 4

	.type	fp,@object
	.data
	.globl	fp
	.p2align	2, 0x0
fp:
	.long	ext
	.size	fp, 4

	.hidden	exth
	.section	".note.GNU-stack","",@progbits
