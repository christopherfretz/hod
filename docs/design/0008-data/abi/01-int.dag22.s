	.file	"01-int.ll"
	.text
	.globl	r8
	.p2align	4
	.type	r8,@function
r8:
	.cfi_startproc
	movb	12(%esp), %cl
	movb	8(%esp), %dl
	movb	4(%esp), %al
	addb	%dl, %al
	addb	%cl, %al
	retl
.Lfunc_end0:
	.size	r8, .Lfunc_end0-r8
	.cfi_endproc

	.globl	r16s
	.p2align	4
	.type	r16s,@function
r16s:
	.cfi_startproc
	movw	4(%esp), %ax
	retl
.Lfunc_end1:
	.size	r16s, .Lfunc_end1-r16s
	.cfi_endproc

	.globl	r64
	.p2align	4
	.type	r64,@function
r64:
	.cfi_startproc
	movl	12(%esp), %ecx
	movl	4(%esp), %eax
	movl	8(%esp), %edx
	addl	%ecx, %eax
	adcl	$0, %edx
	retl
.Lfunc_end2:
	.size	r64, .Lfunc_end2-r64
	.cfi_endproc

	.globl	r1
	.p2align	4
	.type	r1,@function
r1:
	.cfi_startproc
	movb	4(%esp), %al
	retl
.Lfunc_end3:
	.size	r1, .Lfunc_end3-r1
	.cfi_endproc

	.globl	call64
	.p2align	4
	.type	call64,@function
call64:
	.cfi_startproc
	subl	$28, %esp
	.cfi_def_cfa_offset 32
	movl	%esp, %eax
	movl	$3, 12(%eax)
	movl	$0, 8(%eax)
	movl	$2, 4(%eax)
	movl	$1, (%eax)
	calll	ext64@PLT
	addl	$28, %esp
	.cfi_def_cfa_offset 4
	retl
.Lfunc_end4:
	.size	call64, .Lfunc_end4-call64
	.cfi_endproc

	.globl	rp
	.p2align	4
	.type	rp,@function
rp:
	.cfi_startproc
	movl	4(%esp), %eax
	retl
.Lfunc_end5:
	.size	rp, .Lfunc_end5-rp
	.cfi_endproc

	.section	".note.GNU-stack","",@progbits
