; Lexical and syntactic corners (spec/ll.md §2, §3): names in every
; spelling, types defined after their use, several entities on a line,
; labels that look like other tokens.
source_filename = "any \22lexical\22.ll"
module asm ".globl asm_symbol"
module asm "asm_symbol = 42"

%0 = type { i32, %1 }
%1 = type { ptr, [2 x i8] }
%pair = type { %later, %later }
%later = type <{ i8, i32 }>
%opaque = type opaque
%"quoted name" = type { i8 }
%"esc\22aped" = type { i16 }
%fn = type void (i32, ...)
%list = type { ptr, %0 }
%-odd.name$ = type { i8, i8 }
%empty = type {}
%vec = type { <4 x i32>, <2 x ptr>, [0 x i64] }

$group = comdat any
$"quoted comdat" = comdat nodeduplicate

@numbered_type = global %0 { i32 1, %1 { ptr null, [2 x i8] c"ab" } }
@pair = global %pair { %later <{ i8 1, i32 2 }>, %later <{ i8 3, i32 4 }> }
@"quoted global" = global %"quoted name" { i8 7 }
@"esc\22aped\5Cname" = global %"esc\22aped" { i16 300 }
@-odd.name$ = global %-odd.name$ { i8 1, i8 2 }
@0 = private constant i32 10
@1 = internal global ptr @0
@5 = private constant i8 5
@ref5 = global ptr @5
@in_group = global i32 1, comdat($group)
@group = weak_odr global i32 2, comdat
@"quoted comdat" = global i64 3, comdat
@empty = global %empty zeroinitializer
@vec = global %vec { <4 x i32> <i32 1, i32 2, i32 3, i32 4>, <2 x ptr> <ptr @0, ptr null>, [0 x i64] zeroinitializer }
@forward = global ptr @defined_later
@defined_later = global ptr @forward

; Several entities on one line, and one entity over several lines.
@a = global i32 1 @b = global i32 2 declare void @d() define void @e() { ret void } !0 = !{}
@c
  =
  global
  i32
  3

; A semicolon, a quote and braces in strings.
@str = constant [14 x i8] c"} { ; \22 /* */\0A"
@str_user = global ptr getelementptr inbounds ([14 x i8], ptr @str, i64 0, i64 3)

define void @fn_type_user(ptr %f) {
  call void (i32, ...) %f(i32 1)
  ret void
}

define i32 @labels(i32 %x) {
entry:
  %"quoted } local" = add i32 %x, 1
  switch i32 %x, label %"}" [
    i32 0, label %-1
    i32 1, label %"1x"
    i32 2, label %i32
    i32 3, label %"quoted { label"
    i32 4, label %a.b-c$d
    i32 5, label %define
    i32 6, label %"0x1F"
  ]

-1:
  br label %"}"

1x:                                               ; preds = %entry } {
  br label %"}"

i32:
  br label %"}"

"quoted { label":
  br label %"}"

a.b-c$d:
  br label %"}"

define:
  br label %"}"

0x1F:
  br label %"}"

"}":
  %s = phi ptr [ @str, %entry ], [ @str, %-1 ], [ @str, %"1x" ], [ @str, %i32 ], [ @str, %"quoted { label" ], [ @str, %a.b-c$d ], [ @str, %define ], [ @str, %"0x1F" ]
  ret i32 %"quoted } local"
}

define { i32, { ptr, [2 x i8] } } @braces_in_types({ i32, { ptr } } %x) {
  %y = extractvalue { i32, { ptr } } %x, 0
  %r = insertvalue { i32, { ptr, [2 x i8] } } zeroinitializer, i32 %y, 0
  ret { i32, { ptr, [2 x i8] } } %r
}

uselistorder ptr @str, { 1, 0, 2, 3, 4, 5, 6, 7, 8 }

!llvm.ident = !{!1}
!1 = !{!"hand-written"}
