	.file	"05-cc.ll"
	.text
	.globl	inr
	.p2align	4
	.type	inr,@function
inr:
	.cfi_startproc
	movl	4(%esp), %ecx
	addl	%ecx, %eax
	retl
.Lfunc_end0:
	.size	inr, .Lfunc_end0-inr
	.cfi_endproc

	.globl	inr64
	.p2align	4
	.type	inr64,@function
inr64:
	.cfi_startproc
	addl	%ecx, %eax
	adcl	$0, %edx
	retl
.Lfunc_end1:
	.size	inr64, .Lfunc_end1-inr64
	.cfi_endproc

	.globl	inrp
	.p2align	4
	.type	inrp,@function
inrp:
	.cfi_startproc
	movl	%eax, %ecx
	movl	%ecx, %eax
	movl	%edx, (%ecx)
	retl
.Lfunc_end2:
	.size	inrp, .Lfunc_end2-inrp
	.cfi_endproc

	.globl	fcc
	.p2align	4
	.type	fcc,@function
fcc:
	.cfi_startproc
	movl	%ecx, %eax
	movl	8(%esp), %ecx
	addl	%ecx, %eax
	retl
.Lfunc_end3:
	.size	fcc, .Lfunc_end3-fcc
	.cfi_endproc

	.globl	std
	.p2align	4
	.type	std,@function
std:
	.cfi_startproc
	movl	4(%esp), %eax
	retl	$8
.Lfunc_end4:
	.size	std, .Lfunc_end4-std
	.cfi_endproc

	.globl	fast
	.p2align	4
	.type	fast,@function
fast:
	.cfi_startproc
	movl	4(%esp), %eax
	retl	$4
.Lfunc_end5:
	.size	fast, .Lfunc_end5-fast
	.cfi_endproc

	.globl	this
	.p2align	4
	.type	this,@function
this:
	.cfi_startproc
	movl	4(%esp), %eax
	retl	$4
.Lfunc_end6:
	.size	this, .Lfunc_end6-this
	.cfi_endproc

	.globl	regc
	.p2align	4
	.type	regc,@function
regc:
	.cfi_startproc
	movl	%ecx, %eax
	retl
.Lfunc_end7:
	.size	regc, .Lfunc_end7-regc
	.cfi_endproc

	.globl	pmost
	.p2align	4
	.type	pmost,@function
pmost:
	.cfi_startproc
	movl	4(%esp), %eax
	retl
.Lfunc_end8:
	.size	pmost, .Lfunc_end8-pmost
	.cfi_endproc

	.globl	pall
	.p2align	4
	.type	pall,@function
pall:
	.cfi_startproc
	movl	4(%esp), %eax
	retl
.Lfunc_end9:
	.size	pall, .Lfunc_end9-pall
	.cfi_endproc

	.globl	cold
	.p2align	4
	.type	cold,@function
cold:
	.cfi_startproc
	movl	4(%esp), %eax
	retl
.Lfunc_end10:
	.size	cold, .Lfunc_end10-cold
	.cfi_endproc

	.globl	tcc
	.p2align	4
	.type	tcc,@function
tcc:
	.cfi_startproc
	movl	%ecx, %eax
	retl	$12
.Lfunc_end11:
	.size	tcc, .Lfunc_end11-tcc
	.cfi_endproc

	.globl	clobber
	.p2align	4
	.type	clobber,@function
clobber:
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
	movl	%esp, %eax
	movl	%ecx, (%eax)
	calll	extc@PLT
	#APP
	#NO_APP
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
	retl
.Lfunc_end12:
	.size	clobber, .Lfunc_end12-clobber
	.cfi_endproc

	.section	".note.GNU-stack","",@progbits
