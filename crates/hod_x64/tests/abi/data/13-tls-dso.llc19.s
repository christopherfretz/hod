	.text
	.file	"13-tls-dso.ll"
	.globl	get
	.p2align	4, 0x90
	.type	get,@function
get:
	.cfi_startproc
	subq	$24, %rsp
	.cfi_def_cfa_offset 32
	data16
	leaq	ext@TLSGD(%rip), %rdi
	data16
	data16
	rex64
	callq	__tls_get_addr@PLT
	movq	%rax, 16(%rsp)
	leaq	.Lloc$local@TLSLD(%rip), %rdi
	callq	__tls_get_addr@PLT
	movq	%rax, (%rsp)
	movq	ie@GOTTPOFF(%rip), %rax
	movq	%rax, 8(%rsp)
	leaq	ld@TLSLD(%rip), %rdi
	callq	__tls_get_addr@PLT
	movq	(%rsp), %rsi
	movq	8(%rsp), %rcx
	movq	%rax, %rdx
	movq	16(%rsp), %rax
	movl	(%rax), %eax
	movl	.Lloc$local@DTPOFF(%rsi), %esi
	movl	%fs:(%rcx), %ecx
	movl	ld@DTPOFF(%rdx), %edx
	addl	%esi, %eax
	addl	%edx, %ecx
	addl	%ecx, %eax
	addq	$24, %rsp
	.cfi_def_cfa_offset 8
	retq
.Lfunc_end0:
	.size	get, .Lfunc_end0-get
	.cfi_endproc

	.type	loc,@object
	.section	.tbss,"awT",@nobits
	.globl	loc
	.p2align	2, 0x0
loc:
.Lloc$local:
	.long	0
	.size	loc, 4

	.type	ld,@object
	.p2align	2, 0x0
ld:
	.long	0
	.size	ld, 4

	.section	".note.GNU-stack","",@progbits
