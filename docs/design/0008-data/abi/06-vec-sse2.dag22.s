	.file	"06-vec-sse2.ll"
	.text
	.globl	v4f
	.p2align	4
	.type	v4f,@function
v4f:
	.cfi_startproc
	subl	$12, %esp
	.cfi_def_cfa_offset 16
	movaps	16(%esp), %xmm0
	addl	$12, %esp
	.cfi_def_cfa_offset 4
	retl
.Lfunc_end0:
	.size	v4f, .Lfunc_end0-v4f
	.cfi_endproc

	.globl	v2i
	.p2align	4
	.type	v2i,@function
v2i:
	.cfi_startproc
	retl
.Lfunc_end1:
	.size	v2i, .Lfunc_end1-v2i
	.cfi_endproc

	.globl	v2s
	.p2align	4
	.type	v2s,@function
v2s:
	.cfi_startproc
	retl
.Lfunc_end2:
	.size	v2s, .Lfunc_end2-v2s
	.cfi_endproc

	.globl	v8f
	.p2align	4
	.type	v8f,@function
v8f:
	.cfi_startproc
	retl
.Lfunc_end3:
	.size	v8f, .Lfunc_end3-v8f
	.cfi_endproc

	.globl	cv
	.p2align	4
	.type	cv,@function
cv:
	.cfi_startproc
	subl	$44, %esp
	.cfi_def_cfa_offset 48
	movaps	%xmm0, %xmm2
	movl	%esp, %eax
	movaps	%xmm2, 16(%eax)
	movl	$1, (%eax)
	movaps	%xmm2, %xmm0
	movaps	%xmm2, %xmm1
	calll	ev@PLT
	addl	$44, %esp
	.cfi_def_cfa_offset 4
	retl
.Lfunc_end4:
	.size	cv, .Lfunc_end4-cv
	.cfi_endproc

	.globl	fd
	.p2align	4
	.type	fd,@function
fd:
	.cfi_startproc
	subl	$12, %esp
	.cfi_def_cfa_offset 16
	movsd	24(%esp), %xmm1
	movsd	16(%esp), %xmm0
	mulsd	%xmm1, %xmm0
	movsd	%xmm0, (%esp)
	fldl	(%esp)
	addl	$12, %esp
	.cfi_def_cfa_offset 4
	retl
.Lfunc_end5:
	.size	fd, .Lfunc_end5-fd
	.cfi_endproc

	.globl	ff
	.p2align	4
	.type	ff,@function
ff:
	.cfi_startproc
	flds	4(%esp)
	retl
.Lfunc_end6:
	.size	ff, .Lfunc_end6-ff
	.cfi_endproc

	.globl	fh
	.p2align	4
	.type	fh,@function
fh:
	.cfi_startproc
	pinsrw	$0, 4(%esp), %xmm0
	retl
.Lfunc_end7:
	.size	fh, .Lfunc_end7-fh
	.cfi_endproc

	.section	".note.GNU-stack","",@progbits
