; Linkage kinds a C++ translation unit produces: linkonce_odr functions and
; data in COMDAT groups (one group with two members), weak and weak_odr
; definitions, extern_weak declarations that nothing defines (their
; address is null and they are not called), hidden and internal symbols,
; aliases, and a table of pointers to all of them.
;@ covers: spec/ir.md §2.2 symbols (linkage, visibility, COMDAT, alias)
;@ unsupported: codegen-not-implemented
;@ stdout: square=144 bump=11,12 weak=101 weak_odr=42 hidden=9 internal=240
;@ stdout: table=16 weak_value=5 hidden_value=21 internal_value=4
;@ stdout: missing function: absent, missing value: absent
;@ stdout: alias=8 weak alias=9 same=1 data alias=21
;@ stdout: through table: 81 109 16 8

$inline_square = comdat any
$shared_table = comdat any
$counter_group = comdat any

@.calls = private unnamed_addr constant [64 x i8] c"square=%d bump=%d,%d weak=%d weak_odr=%d hidden=%d internal=%d\0A\00"
@.data = private unnamed_addr constant [58 x i8] c"table=%d weak_value=%d hidden_value=%d internal_value=%d\0A\00"
@.missing = private unnamed_addr constant [41 x i8] c"missing function: %s, missing value: %s\0A\00"
@.alias = private unnamed_addr constant [46 x i8] c"alias=%d weak alias=%d same=%d data alias=%d\0A\00"
@.table = private unnamed_addr constant [28 x i8] c"through table: %d %d %d %d\0A\00"
@.absent = private unnamed_addr constant [7 x i8] c"absent\00"
@.present = private unnamed_addr constant [8 x i8] c"present\00"

@shared_table = linkonce_odr constant [3 x i32] [i32 3, i32 5, i32 8], comdat
@inline_counter = linkonce_odr global i32 10, comdat($counter_group)
@weak_value = weak global i32 5
@missing_value = extern_weak global i32
@hidden_value = hidden global i32 21
@internal_value = internal global i32 4
@functions = global [4 x ptr] [ptr @inline_square, ptr @weak_default, ptr @plain_alias, ptr @hidden_fn]

@plain_alias = alias i32 (i32), ptr @plain
@weak_alias = weak alias i32 (i32), ptr @plain
@value_alias = alias i32, ptr @hidden_value

declare i32 @printf(ptr, ...)
declare extern_weak i32 @missing_fn(i32)

define linkonce_odr i32 @inline_square(i32 %x) comdat {
  %r = mul i32 %x, %x
  ret i32 %r
}

define linkonce_odr i32 @bump() comdat($counter_group) {
  %old = load i32, ptr @inline_counter
  %new = add i32 %old, 1
  store i32 %new, ptr @inline_counter
  ret i32 %new
}

define weak i32 @weak_default(i32 %x) {
  %r = add i32 %x, 100
  ret i32 %r
}

define weak_odr i32 @weak_odr_fn(i32 %x) {
  %r = shl i32 %x, 1
  ret i32 %r
}

define hidden i32 @hidden_fn(i32 %x) {
  %r = sub i32 %x, 1
  ret i32 %r
}

define internal i32 @internal_fn(i32 %x) {
  %r = xor i32 %x, 255
  ret i32 %r
}

define i32 @plain(i32 %x) {
  %r = add i32 %x, 7
  ret i32 %r
}

define i32 @main() {
entry:
  %sq = call i32 @inline_square(i32 12)
  %b1 = call i32 @bump()
  %b2 = call i32 @bump()
  %w = call i32 @weak_default(i32 1)
  %wo = call i32 @weak_odr_fn(i32 21)
  %h = call i32 @hidden_fn(i32 10)
  %in = call i32 @internal_fn(i32 15)
  call i32 (ptr, ...) @printf(ptr @.calls, i32 %sq, i32 %b1, i32 %b2, i32 %w, i32 %wo, i32 %h, i32 %in)

  %t0 = load i32, ptr @shared_table
  %t1 = load i32, ptr getelementptr inbounds ([3 x i32], ptr @shared_table, i32 0, i32 1)
  %t2 = load i32, ptr getelementptr inbounds ([3 x i32], ptr @shared_table, i32 0, i32 2)
  %t01 = add i32 %t0, %t1
  %t = add i32 %t01, %t2
  %wv = load i32, ptr @weak_value
  %hv = load i32, ptr @hidden_value
  %iv = load i32, ptr @internal_value
  call i32 (ptr, ...) @printf(ptr @.data, i32 %t, i32 %wv, i32 %hv, i32 %iv)

  %no.fn = icmp eq ptr @missing_fn, null
  br i1 %no.fn, label %checked, label %call.missing
call.missing:
  call i32 @missing_fn(i32 1)
  br label %checked
checked:
  %fn.text = select i1 %no.fn, ptr @.absent, ptr @.present
  %no.value = icmp eq ptr @missing_value, null
  %value.text = select i1 %no.value, ptr @.absent, ptr @.present
  call i32 (ptr, ...) @printf(ptr @.missing, ptr %fn.text, ptr %value.text)

  %a = call i32 @plain_alias(i32 1)
  %wa = call i32 @weak_alias(i32 2)
  %same = icmp eq ptr @plain_alias, @plain
  %same.i = zext i1 %same to i32
  %va = load i32, ptr @value_alias
  call i32 (ptr, ...) @printf(ptr @.alias, i32 %a, i32 %wa, i32 %same.i, i32 %va)

  %f0 = load ptr, ptr @functions
  %f1 = load ptr, ptr getelementptr inbounds ([4 x ptr], ptr @functions, i32 0, i32 1)
  %f2 = load ptr, ptr getelementptr inbounds ([4 x ptr], ptr @functions, i32 0, i32 2)
  %f3 = load ptr, ptr getelementptr inbounds ([4 x ptr], ptr @functions, i32 0, i32 3)
  %r0 = call i32 %f0(i32 9)
  %r1 = call i32 %f1(i32 9)
  %r2 = call i32 %f2(i32 9)
  %r3 = call i32 %f3(i32 9)
  call i32 (ptr, ...) @printf(ptr @.table, i32 %r0, i32 %r1, i32 %r2, i32 %r3)
  ret i32 0
}
