; Global values with every field (spec/ll.md §5.6 to §5.9).

%big = type { [8 x i64] }

$c_any = comdat any
$c_fn = comdat any
$c_nodup = comdat nodeduplicate
$shared = comdat any

; Linkage.
@l_external = global i32 1
@l_private = private global i32 2
@l_internal = internal global i32 3
@l_weak = weak global i32 4
@l_weak_odr = weak_odr global i32 5
@l_linkonce = linkonce global i32 6
@l_linkonce_odr = linkonce_odr global i32 7
@l_common = common global i32 0
@l_common_big = common global [100 x i64] zeroinitializer, align 16
@l_available = available_externally global i32 8
@l_extern = external global i32
@l_extern_weak = extern_weak global i32
@l_users = global [4 x ptr] [ptr @l_extern, ptr @l_extern_weak, ptr @l_private, ptr @l_available]

; Visibility, preemption, addresses that do not matter.
@v_hidden = hidden global i32 1
@v_protected = protected global i32 2
@v_default = default global i32 3
@v_hidden_weak = weak hidden global i32 4
@v_hidden_extern = external hidden global i32
@v_dso_local = dso_local global i32 5
@v_dso_preemptable = dso_preemptable global i32 6
@v_dso_local_extern = external dso_local global i32
@v_unnamed = unnamed_addr constant i32 7
@v_local_unnamed = local_unnamed_addr constant i32 8
@v_private_unnamed = private unnamed_addr constant [4 x i8] c"abc\00", align 1
@v_users = global [2 x ptr] [ptr @v_hidden_extern, ptr @v_dso_local_extern]

; Constant or not, sections, alignment, COMDATs.
@s_constant = constant i64 9
@s_zero = global [32 x i8] zeroinitializer
@s_section = global i32 1, section "custom_data"
@s_section_const = constant i32 2, section ".rodata.named", align 16
@s_section_quoted = global i32 3, section "odd \22name\22"
@s_align = global i8 4, align 128
@s_align_under = global i64 5, align 1
@s_big_array = global [40 x i8] c"0123456789012345678901234567890123456789"
@s_comdat = global i32 6, comdat($c_any)
@c_any = weak_odr constant i32 7, comdat
@c_nodup = global i32 8, comdat, align 8
@s_shared_a = linkonce_odr global i32 9, comdat($shared)
@s_shared_b = linkonce_odr global i32 10, comdat($shared), section "custom_group", align 32
@s_externally = externally_initialized global i32 11
@s_no_sanitize = global i32 12, no_sanitize_address, no_sanitize_hwaddress
@s_dyninit = global i32 13, sanitize_address_dyninit
@s_all = weak_odr hidden unnamed_addr constant { i8, i32 } { i8 1, i32 2 }, section "everything", comdat($shared), align 64

; Thread-local storage.
@t_general = thread_local global i32 1
@t_local_dynamic = thread_local(localdynamic) global i32 2
@t_initial_exec = thread_local(initialexec) global i32 3
@t_local_exec = thread_local(localexec) global i32 4
@t_zero = thread_local global [64 x i8] zeroinitializer
@t_internal = internal thread_local global i64 5, align 16
@t_extern = external thread_local global i32
@t_const = thread_local constant i32 6
@t_weak = weak_odr hidden thread_local(initialexec) global i32 7, comdat($shared)

; Aliases and IFUNCs.
@aliasee = global { i32, i32, [4 x i8] } { i32 1, i32 2, [4 x i8] c"wxyz" }
@a_plain = alias { i32, i32, [4 x i8] }, ptr @aliasee
@a_weak = weak alias { i32, i32, [4 x i8] }, ptr @aliasee
@a_hidden = hidden alias i32, ptr @aliasee
@a_internal = internal alias i32, ptr @aliasee
@a_private = private alias i32, ptr @aliasee
@a_member = alias i32, getelementptr inbounds ({ i32, i32, [4 x i8] }, ptr @aliasee, i32 0, i32 1)
@a_byte = protected unnamed_addr alias i8, getelementptr (i8, ptr @aliasee, i64 9)
@a_of_alias = alias i32, ptr @a_member
@a_fn = alias void (), ptr @f_plain
@a_fn_weak = weak_odr dso_local alias void (), ptr @f_plain
@a_tls = thread_local alias i32, ptr @t_general
@a_linkonce = linkonce_odr alias i32, ptr @aliasee
@a_users = global [3 x ptr] [ptr @a_internal, ptr @a_private, ptr @a_of_alias]
@i_plain = ifunc void (), ptr @resolver
@i_weak = weak ifunc i32 (i32), ptr @resolver
@i_hidden = hidden ifunc void (), ptr @resolver
@i_internal = internal ifunc void (), ptr @resolver
@i_through_alias = ifunc void (), ptr @resolver_alias
@resolver_alias = internal alias ptr (), ptr @resolver
@i_users = global [2 x ptr] [ptr @i_internal, ptr @i_plain]

define internal ptr @resolver() {
  ret ptr @f_plain
}

; Function linkage, visibility, calling conventions.
define void @f_plain() {
  ret void
}
define internal void @f_internal() {
  ret void
}
define private void @f_private() {
  ret void
}
define weak void @f_weak() {
  ret void
}
define weak_odr hidden void @f_weak_odr() {
  ret void
}
define linkonce_odr protected void @f_linkonce_odr() comdat($c_fn) {
  ret void
}
define linkonce void @f_linkonce() {
  ret void
}
define available_externally void @f_available() {
  ret void
}
define dso_local void @f_dso_local() unnamed_addr {
  ret void
}
define void @f_local_unnamed() local_unnamed_addr {
  ret void
}
declare void @f_decl()
declare extern_weak void @f_decl_weak()
declare hidden void @f_decl_hidden()
declare dso_local void @f_decl_dso_local()
declare protected i32 @f_decl_protected(i32)
define ccc void @f_cc_c() {
  ret void
}
define fastcc void @f_cc_fast() {
  ret void
}
declare coldcc void @f_cc_cold()
declare cc 10 void @f_cc_10()
declare cc17 void @f_cc_17()
define preserve_mostcc void @f_cc_preserve_most() {
  ret void
}
define preserve_allcc void @f_cc_preserve_all() {
  ret void
}
define tailcc void @f_cc_tail() {
  ret void
}
define swiftcc void @f_cc_swift(ptr swiftself %self, ptr swifterror %err) {
  ret void
}
define swifttailcc void @f_cc_swifttail(ptr swiftasync %ctx) {
  ret void
}
declare cc 1023 void @f_cc_last()

; Return and parameter attributes.
define signext i8 @f_ret_signext() {
  ret i8 -1
}
define zeroext i16 @f_ret_zeroext() {
  ret i16 1
}
define noundef nonnull align 8 dereferenceable(8) ptr @f_ret_many(ptr noundef nonnull returned align 8 dereferenceable(8) %p) {
  ret ptr %p
}
define void @f_params(i8 signext %a, i16 zeroext %b, i32 inreg %c, ptr nest %d, ptr noalias nocapture readonly %e, i32 noundef %0, ptr align 16 %g, ptr dereferenceable_or_null(4) %h) {
  ret void
}
define void @f_sret(ptr sret(%big) align 8 %out, ptr byval(%big) align 8 %in, ptr byref({ i32, i32 }) %ref) {
  ret void
}
define void @f_inalloca(ptr inalloca(i32) %a) {
  ret void
}
define void @f_preallocated(ptr preallocated(i64) %a) {
  ret void
}
define void @f_stack_align(ptr byval(%big) align 32 alignstack(32) %a, i64 alignstack(16) %b) {
  ret void
}
define i32 @f_variadic(i32 %n, ...) {
  ret i32 %n
}
declare i32 @f_variadic_only(...)
declare void @f_unnamed_params(i32, ptr nocapture, double, { i32, ptr }, [2 x i64], <4 x float>, i128, half, fp128)
declare void @f_range(i32 range(i32 0, 10), double nofpclass(nan inf), float nofpclass(nzero))
declare range(i8 -1, 2) i8 @f_ret_range()
declare void @f_writable(ptr writable dereferenceable(4), ptr dead_on_unwind, ptr readnone, ptr writeonly)
declare void @f_allocptr(ptr allocptr, i64 allocalign)
declare void @f_string_param(ptr "param-string"="x" "bare-param-string", i32 "another")

; Function attributes, written in place and through groups.
define void @f_inline_attrs() nounwind noreturn uwtable "frame-pointer"="all" "target-cpu"="generic" {
  unreachable
}
define void @f_group0() #0 {
  ret void
}
define void @f_group_and_inline() #1 nounwind "target-features"="" {
  ret void
}
define void @f_two_groups() #0 #1 {
  ret void
}
define void @f_override() "frame-pointer"="none" #0 {
  ret void
}
define void @f_undefined_group() #99 {
  ret void
}
define void @f_uwtable_sync() uwtable(sync) {
  ret void
}
define void @f_uwtable_async() uwtable(async) {
  ret void
}
define void @f_alignstack() alignstack(16) {
  ret void
}
define void @f_group_align() #2 {
  ret void
}
define void @f_group_align_override() #2 align 4 {
  ret void
}
define ptr @f_dropped(i64 %n) cold minsize optsize noinline nofree nosync willreturn mustprogress memory(none) allocsize(0) allockind("alloc,zeroed") vscale_range(1,16) speculatable nobuiltin norecurse nomerge inlinehint nocallback "no-trapping-math"="true" "unsafe-fp-math"="false" {
  ret ptr null
}
define void @f_kept() naked returns_twice alwaysinline ssp sspreq sspstrong presplitcoroutine sanitize_address sanitize_hwaddress sanitize_memory sanitize_thread sanitize_memtag "tune-cpu"="generic" "patchable-function-entry"="2" "patchable-function-prefix"="1" "function-instrument"="xray-always" "stack-probe-size"="4096" "probe-stack"="inline-asm" "split-stack" "zero-call-used-regs"="used" "stack-protector-buffer-size"="8" "min-legal-vector-width"="0" "no-jump-tables"="true" "no-builtins" "no-builtin-memcpy" "no-builtin-memset" {
  ret void
}
define void @f_rejected() unnamed_addr convergent jumptable noredzone noimplicitfloat nonlazybind strictfp safestack shadowcallstack speculative_load_hardening nocf_check noduplicate optforfuzzing "denormal-fp-math"="ieee,ieee" "use-sample-profile" "made-up-attribute"="1" {
  ret void
}
define void @f_branch_protection() "branch-target-enforcement"="true" "sign-return-address"="non-leaf" "sign-return-address-key"="b_key" "guarded-control-stack"="false" {
  ret void
}
define void @f_sign_none() "sign-return-address"="none" "sign-return-address-key"="a_key" "branch-protection-pauth-lr"="true" {
  ret void
}
declare void @f_decl_attrs() #0
declare void @f_decl_inline_attrs() nounwind "frame-pointer"="non-leaf"

; Placement, personality.
define void @f_section() section "fn_section" {
  ret void
}
define void @f_align() align 64 {
  ret void
}
define void @f_comdat() comdat {
  ret void
}
$f_comdat = comdat any
define void @f_everything() unnamed_addr #0 section "fn_all" comdat($c_fn) align 32 personality ptr @personality {
  ret void
}
declare i32 @personality(...)
define void @f_personality() personality ptr @personality {
  invoke void @f_decl() to label %ok unwind label %pad

ok:
  ret void

pad:
  %lp = landingpad { ptr, i32 } cleanup catch ptr null filter [1 x ptr] [ptr @l_external]
  resume { ptr, i32 } %lp
}
define void @f_align_attr_order() align 16 nounwind {
  ret void
}

; The special arrays.
@llvm.used = appending global [3 x ptr] [ptr @l_internal, ptr @f_internal, ptr @t_internal], section "llvm.metadata"
@llvm.compiler.used = appending global [2 x ptr] [ptr @l_private, ptr @f_private], section "llvm.metadata"
@llvm.global_ctors = appending global [5 x { i32, ptr, ptr }] [
  { i32, ptr, ptr } { i32 65535, ptr @f_internal, ptr null },
  { i32, ptr, ptr } { i32 101, ptr @f_plain, ptr @s_comdat },
  { i32, ptr, ptr } { i32 0, ptr @f_private, ptr null },
  { i32, ptr, ptr } { i32 70000, ptr @f_weak, ptr null },
  { i32, ptr, ptr } { i32 5, ptr null, ptr null }
]
@llvm.global_dtors = appending global [2 x { i32, ptr, ptr }] [{ i32, ptr, ptr } { i32 65535, ptr @f_plain, ptr null }, { i32, ptr, ptr } { i32 3, ptr @f_weak, ptr @l_weak }]

attributes #0 = { nounwind uwtable(sync) "frame-pointer"="non-leaf" "target-cpu"="generic" "no-trapping-math"="true" }
attributes #1 = { noreturn "frame-pointer"="all" "stack-protector-buffer-size"="4" cold }
attributes #2 = { align=64 alignstack=32 nounwind }

!llvm.module.flags = !{!0, !1, !2, !3, !4, !5}
!llvm.linker.options = !{!6, !7}
!llvm.dependent-libraries = !{!8, !9}
!llvm.ident = !{!10}
!custom.named = !{!10}

!0 = !{i32 8, !"PIC Level", i32 2}
!1 = !{i32 7, !"PIE Level", i32 2}
!2 = !{i32 7, !"uwtable", i32 1}
!3 = !{i32 7, !"frame-pointer", i32 2}
!4 = !{i32 1, !"wchar_size", i32 4}
!5 = !{i32 1, !"Code Model", i32 3}
!6 = !{!"lib", !"first"}
!7 = !{!"-u", !"needed symbol"}
!8 = !{!"m"}
!9 = !{!"quoted \22lib\22"}
!10 = !{!"hand-written"}
