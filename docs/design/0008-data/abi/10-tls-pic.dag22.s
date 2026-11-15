	.file	"10-tls-pic.ll"
	.text
	.globl	t
	.p2align	4
	.type	t,@function
t:
	.cfi_startproc
	pushl	%ebx
	.cfi_def_cfa_offset 8
	pushl	%esi
	.cfi_def_cfa_offset 12
	subl	$20, %esp
	.cfi_def_cfa_offset 32
	.cfi_offset %esi, -12
	.cfi_offset %ebx, -8
	calll	.L0$pb
	.cfi_adjust_cfa_offset 4
.L0$pb:
	popl	%ebx
	.cfi_adjust_cfa_offset -4
.Ltmp0:
	addl	$_GLOBAL_OFFSET_TABLE_+(.Ltmp0-.L0$pb), %ebx
	movl	%ebx, 12(%esp)
	leal	gd@TLSGD(,%ebx), %eax
	calll	___tls_get_addr@PLT
	movl	12(%esp), %ebx
	movl	(%eax), %eax
	movl	%eax, 16(%esp)
	leal	ld@TLSLDM(%ebx), %eax
	calll	___tls_get_addr@PLT
	movl	12(%esp), %ecx
	movl	%eax, %edx
	movl	16(%esp), %eax
	movl	ld@DTPOFF(%edx), %esi
	movl	ie@GOTNTPOFF(%ecx), %ecx
	movl	%gs:(%ecx), %ecx
	movl	%gs:le@NTPOFF, %edx
	addl	%esi, %eax
	addl	%edx, %ecx
	addl	%ecx, %eax
	addl	$20, %esp
	.cfi_def_cfa_offset 12
	popl	%esi
	.cfi_def_cfa_offset 8
	popl	%ebx
	.cfi_def_cfa_offset 4
	retl
.Lfunc_end0:
	.size	t, .Lfunc_end0-t
	.cfi_endproc

	.type	ld,@object
	.section	.tbss,"awT",@nobits
	.p2align	2, 0x0
ld:
	.long	0
	.size	ld, 4

	.type	le,@object
	.globl	le
	.p2align	2, 0x0
le:
	.long	0
	.size	le, 4

	.section	".note.GNU-stack","",@progbits
