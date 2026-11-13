	.file	"07-vec.ll"
	.text
	.globl	call_vec
	.p2align	4
	.type	call_vec,@function
call_vec:
	.cfi_startproc
	pushq	%rax
	.cfi_def_cfa_offset 16
	callq	sv@PLT
	popq	%rax
	.cfi_def_cfa_offset 8
	retq
.Lfunc_end0:
	.size	call_vec, .Lfunc_end0-call_vec
	.cfi_endproc

	.globl	ret_v8f
	.p2align	4
	.type	ret_v8f,@function
ret_v8f:
	.cfi_startproc
	retq
.Lfunc_end1:
	.size	ret_v8f, .Lfunc_end1-ret_v8f
	.cfi_endproc

	.globl	ret_v2i
	.p2align	4
	.type	ret_v2i,@function
ret_v2i:
	.cfi_startproc
	retq
.Lfunc_end2:
	.size	ret_v2i, .Lfunc_end2-ret_v2i
	.cfi_endproc

	.globl	ret_v16f
	.p2align	4
	.type	ret_v16f,@function
ret_v16f:
	.cfi_startproc
	retq
.Lfunc_end3:
	.size	ret_v16f, .Lfunc_end3-ret_v16f
	.cfi_endproc

	.section	".note.GNU-stack","",@progbits
