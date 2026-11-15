	.file	"04-aggr.ll"
	.text
	.globl	rs
	.p2align	4
	.type	rs,@function
rs:
	.cfi_startproc
	movl	4(%esp), %eax
	movl	8(%esp), %ecx
	movl	%ecx, (%eax)
	retl	$4
.Lfunc_end0:
	.size	rs, .Lfunc_end0-rs
	.cfi_endproc

	.globl	crs
	.p2align	4
	.type	crs,@function
crs:
	.cfi_startproc
	subl	$28, %esp
	.cfi_def_cfa_offset 32
	movl	%esp, %eax
	leal	16(%esp), %ecx
	movl	%ecx, (%eax)
	movl	$5, 4(%eax)
	calll	ers@PLT
	subl	$4, %esp
	movl	16(%esp), %eax
	addl	$28, %esp
	.cfi_def_cfa_offset 4
	retl
.Lfunc_end1:
	.size	crs, .Lfunc_end1-crs
	.cfi_endproc

	.globl	bv
	.p2align	4
	.type	bv,@function
bv:
	.cfi_startproc
	movl	4(%esp), %eax
	retl
.Lfunc_end2:
	.size	bv, .Lfunc_end2-bv
	.cfi_endproc

	.globl	cbv
	.p2align	4
	.type	cbv,@function
cbv:
	.cfi_startproc
	subl	$28, %esp
	.cfi_def_cfa_offset 32
	movl	32(%esp), %edx
	movl	8(%edx), %ecx
	movl	%esp, %eax
	movl	%ecx, 12(%eax)
	movl	(%edx), %ecx
	movl	4(%edx), %edx
	movl	%edx, 8(%eax)
	movl	%ecx, 4(%eax)
	movl	$2, 16(%eax)
	movl	$1, (%eax)
	calll	ebv@PLT
	addl	$28, %esp
	.cfi_def_cfa_offset 4
	retl
.Lfunc_end3:
	.size	cbv, .Lfunc_end3-cbv
	.cfi_endproc

	.globl	cbv16
	.p2align	4
	.type	cbv16,@function
cbv16:
	.cfi_startproc
	subl	$44, %esp
	.cfi_def_cfa_offset 48
	movl	48(%esp), %edx
	movl	16(%edx), %ecx
	movl	%esp, %eax
	movl	%ecx, 16(%eax)
	movl	12(%edx), %ecx
	movl	%ecx, 12(%eax)
	movl	8(%edx), %ecx
	movl	%ecx, 8(%eax)
	movl	(%edx), %ecx
	movl	4(%edx), %edx
	movl	%edx, 4(%eax)
	movl	%ecx, (%eax)
	calll	ebv16@PLT
	addl	$44, %esp
	.cfi_def_cfa_offset 4
	retl
.Lfunc_end4:
	.size	cbv16, .Lfunc_end4-cbv16
	.cfi_endproc

	.globl	rpair
	.p2align	4
	.type	rpair,@function
rpair:
	.cfi_startproc
	movl	$1, %eax
	movl	$2, %edx
	retl
.Lfunc_end5:
	.size	rpair, .Lfunc_end5-rpair
	.cfi_endproc

	.globl	rtriple
	.p2align	4
	.type	rtriple,@function
rtriple:
	.cfi_startproc
	movl	$1, %eax
	movl	$2, %edx
	movl	$3, %ecx
	retl
.Lfunc_end6:
	.size	rtriple, .Lfunc_end6-rtriple
	.cfi_endproc

	.globl	rquint
	.p2align	4
	.type	rquint,@function
rquint:
	.cfi_startproc
	movl	4(%esp), %ecx
	movl	%ecx, %eax
	movl	$5, 16(%ecx)
	movl	$4, 12(%ecx)
	movl	$3, 8(%ecx)
	movl	$2, 4(%ecx)
	movl	$1, (%ecx)
	retl	$4
.Lfunc_end7:
	.size	rquint, .Lfunc_end7-rquint
	.cfi_endproc

	.section	.rodata.cst4,"aM",@progbits,4
	.p2align	2, 0x0
.LCPI8_0:
	.long	0x40000000
	.text
	.globl	rdd
	.p2align	4
	.type	rdd,@function
rdd:
	.cfi_startproc
	flds	.LCPI8_0
	fld1
	retl
.Lfunc_end8:
	.size	rdd, .Lfunc_end8-rdd
	.cfi_endproc

	.globl	rfi
	.p2align	4
	.type	rfi,@function
rfi:
	.cfi_startproc
	movl	$2, %eax
	fld1
	retl
.Lfunc_end9:
	.size	rfi, .Lfunc_end9-rfi
	.cfi_endproc

	.globl	rarr
	.p2align	4
	.type	rarr,@function
rarr:
	.cfi_startproc
	movl	$1, %eax
	movl	$2, %edx
	retl
.Lfunc_end10:
	.size	rarr, .Lfunc_end10-rarr
	.cfi_endproc

	.section	".note.GNU-stack","",@progbits
