	.text
	.file	"09-musttail.ll"
	.globl	thunk
	.p2align	4, 0x90
	.type	thunk,@function
thunk:
	.cfi_startproc
	jmp	target@PLT
.Lfunc_end0:
	.size	thunk, .Lfunc_end0-thunk
	.cfi_endproc

	.section	".note.GNU-stack","",@progbits
