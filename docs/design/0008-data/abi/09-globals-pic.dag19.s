	.text
	.file	"09-globals-pic.ll"
	.p2align	4, 0x90
	.type	loc,@function
loc:
	.cfi_startproc
	retl
.Lfunc_end0:
	.size	loc, .Lfunc_end0-loc
	.cfi_endproc

	.globl	ld
	.p2align	4, 0x90
	.type	ld,@function
ld:
	.cfi_startproc
	calll	.L1$pb
	.cfi_adjust_cfa_offset 4
.L1$pb:
	popl	%ecx
	.cfi_adjust_cfa_offset -4
.Ltmp0:
	addl	$_GLOBAL_OFFSET_TABLE_+(.Ltmp0-.L1$pb), %ecx
	movl	g@GOT(%ecx), %eax
	movl	(%eax), %eax
	movl	l@GOTOFF(%ecx), %edx
	movl	h@GOTOFF(%ecx), %ecx
	addl	%edx, %eax
	addl	%ecx, %eax
	retl
.Lfunc_end1:
	.size	ld, .Lfunc_end1-ld
	.cfi_endproc

	.globl	calls
	.p2align	4, 0x90
	.type	calls,@function
calls:
	.cfi_startproc
	pushl	%ebx
	.cfi_def_cfa_offset 8
	subl	$8, %esp
	.cfi_def_cfa_offset 16
	.cfi_offset %ebx, -8
	calll	.L2$pb
	.cfi_adjust_cfa_offset 4
.L2$pb:
	popl	%ebx
	.cfi_adjust_cfa_offset -4
.Ltmp1:
	addl	$_GLOBAL_OFFSET_TABLE_+(.Ltmp1-.L2$pb), %ebx
	movl	%ebx, 4(%esp)
	calll	ext@PLT
	movl	4(%esp), %ebx
	calll	exth
	movl	4(%esp), %ebx
	calll	loc
	addl	$8, %esp
	.cfi_def_cfa_offset 8
	popl	%ebx
	.cfi_def_cfa_offset 4
	retl
.Lfunc_end2:
	.size	calls, .Lfunc_end2-calls
	.cfi_endproc

	.globl	addr
	.p2align	4, 0x90
	.type	addr,@function
addr:
	.cfi_startproc
	calll	.L3$pb
	.cfi_adjust_cfa_offset 4
.L3$pb:
	popl	%eax
	.cfi_adjust_cfa_offset -4
.Ltmp2:
	addl	$_GLOBAL_OFFSET_TABLE_+(.Ltmp2-.L3$pb), %eax
	movl	ext@GOT(%eax), %eax
	retl
.Lfunc_end3:
	.size	addr, .Lfunc_end3-addr
	.cfi_endproc

	.globl	addrl
	.p2align	4, 0x90
	.type	addrl,@function
addrl:
	.cfi_startproc
	calll	.L4$pb
	.cfi_adjust_cfa_offset 4
.L4$pb:
	popl	%eax
	.cfi_adjust_cfa_offset -4
.Ltmp3:
	addl	$_GLOBAL_OFFSET_TABLE_+(.Ltmp3-.L4$pb), %eax
	leal	loc@GOTOFF(%eax), %eax
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
	.p2align	4, 0x90
	.type	cst,@function
cst:
	.cfi_startproc
	calll	.L5$pb
	.cfi_adjust_cfa_offset 4
.L5$pb:
	popl	%eax
	.cfi_adjust_cfa_offset -4
.Ltmp4:
	addl	$_GLOBAL_OFFSET_TABLE_+(.Ltmp4-.L5$pb), %eax
	fldl	4(%esp)
	flds	.LCPI5_0@GOTOFF(%eax)
	faddp	%st, %st(1)
	retl
.Lfunc_end5:
	.size	cst, .Lfunc_end5-cst
	.cfi_endproc

	.globl	sw
	.p2align	4, 0x90
	.type	sw,@function
sw:
	.cfi_startproc
	pushl	%ebx
	.cfi_def_cfa_offset 8
	subl	$8, %esp
	.cfi_def_cfa_offset 16
	.cfi_offset %ebx, -8
	calll	.L6$pb
	.cfi_adjust_cfa_offset 4
.L6$pb:
	popl	%eax
	.cfi_adjust_cfa_offset -4
.Ltmp5:
	addl	$_GLOBAL_OFFSET_TABLE_+(.Ltmp5-.L6$pb), %eax
	movl	%eax, (%esp)
	movl	16(%esp), %eax
	movl	%eax, 4(%esp)
	subl	$4, %eax
	ja	.LBB6_5
	movl	(%esp), %ecx
	movl	4(%esp), %eax
	movl	.LJTI6_0@GOTOFF(%ecx,%eax,4), %eax
	addl	%ecx, %eax
	jmpl	*%eax
.LBB6_1:
	movl	(%esp), %ebx
	calll	ext@PLT
	jmp	.LBB6_5
.LBB6_2:
	movl	(%esp), %ebx
	calll	exth
	jmp	.LBB6_5
.LBB6_3:
	movl	(%esp), %ebx
	calll	loc
	jmp	.LBB6_5
.LBB6_4:
	movl	(%esp), %ebx
	calll	ext@PLT
	jmp	.LBB6_5
.LBB6_5:
	addl	$8, %esp
	.cfi_def_cfa_offset 8
	popl	%ebx
	.cfi_def_cfa_offset 4
	retl
.Lfunc_end6:
	.size	sw, .Lfunc_end6-sw
	.cfi_endproc
	.section	.rodata,"a",@progbits
	.p2align	2, 0x0
.LJTI6_0:
	.long	.LBB6_1@GOTOFF
	.long	.LBB6_2@GOTOFF
	.long	.LBB6_3@GOTOFF
	.long	.LBB6_4@GOTOFF
	.long	.LBB6_1@GOTOFF

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
.Ld$local:
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
