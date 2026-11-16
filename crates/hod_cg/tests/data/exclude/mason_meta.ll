; `!exclude` on a global (spec/codegen.md §9.7.4, WP-273): a section
; `.mason.meta`, SHT_PROGBITS, sh_flags exactly SHF_EXCLUDE, sh_addralign 8,
; in no group, without relocations, kept although nothing refers to it,
; its bytes the initializer's (0x00, 0xff and sequences that are not UTF-8
; among them). The target comes from the command line (-mtriple).

@__mason_meta = private constant [16 x i8] c"MMv1\00\FF\C3(\80\FE\FF\00\E2\82\00z", section ".mason.meta", align 8, !exclude !0
@llvm.compiler.used = appending global [1 x ptr] [ptr @__mason_meta], section "llvm.metadata"

define i32 @main() {
  ret i32 0
}

!0 = !{}
