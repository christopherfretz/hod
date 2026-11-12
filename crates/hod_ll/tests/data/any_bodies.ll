; Function bodies that the index scan must step over exactly
; (spec/ll.md §4.2): braces in every place a brace can stand.

%pair = type { i32, { i8, [2 x { i16 }] } }

@"}" = global i32 1
@"{" = global i32 2
@text = private constant [5 x i8] c"}{}\22{"
@table = global [2 x ptr] [ptr blockaddress(@braces, %"}"), ptr blockaddress(@braces, %"{")]

declare void @sink(ptr, ...)
declare i32 @personality(...)

define void @empty() {
  ret void
}

define { i32, { i8 } } @braces({ i32, { i8 } } %in, <{ i8, i32 }> %packed) {
entry:
  %a = alloca { i32, { i8, [2 x { i16 }] } }, align 4 ; } a brace in a comment {
  %b = insertvalue { i32, { i8 } } %in, i32 7, 0
  call void (ptr, ...) @sink(ptr @"}", { i32, i32 } { i32 1, i32 2 }, <{ i8 }> <{ i8 3 }>)
  call void (ptr, ...) @sink(ptr @text, [3 x i8] c"}}}", ptr @"{")
  br label %"}"

"}":                                              ; preds = %entry
  %c = phi i32 [ 0, %entry ], [ 1, %"}" ]
  indirectbr ptr blockaddress(@braces, %"{"), [label %"{", label %"}"]

"{":
  store { i32, { i8 } } { i32 1, { i8 } { i8 2 } }, ptr %a, align 4, !annotation !{!"}"}
  ret { i32, { i8 } } %b
}

@between = global ptr @braces

define void @strings() section "a } section" {
  call void (ptr, ...) @sink(ptr null, [2 x i8] c"\22}", [1 x i8] c"{") ; "
  ret void
}

define void @"quoted } name"(ptr %"arg }") personality ptr @personality {
  invoke void (ptr, ...) @sink(ptr %"arg }")
          to label %done unwind label %"pad }"

done:
  ret void

"pad }":
  %lp = landingpad { ptr, i32 }
          cleanup
          catch ptr @"}"
          filter [2 x ptr] [ptr @"{", ptr @text]
  resume { ptr, i32 } %lp
}

define void @switch(i32 %v) {
  switch i32 %v, label %out [
    i32 0, label %out
    i32 1, label %one
  ]

one:
  br label %out

out:
  ret void
}

define void @0() {
  ret void
}

define internal void @1(i32 %0, i32 %1) {
  %3 = add i32 %0, %1
  ret void
}

define void @same_line() { ret void }
define void @two() { ret void } define void @three() {
  ret void }

define void @metadata() !annotation !0 {
  call void (ptr, ...) @sink(ptr null), !annotation !{!"{", !{}}
  ret void, !annotation !0
}

define i32 @prefixed() prefix { i32, i8 } { i32 1, i8 2 } prologue i8 144 {
  ret i32 0
}

@after = global ptr @same_line

!0 = !{!"} in metadata {"}
