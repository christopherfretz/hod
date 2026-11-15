; `asm goto` (a `callbr` whose template is not empty) on x86-64, WP-254.
; Every template uses baseline instructions only (the emulator's cpu
; has nothing above SSE2). `tests/asm_goto.rs` computes what each
; function returns.

; One label: taken when %x is not zero.
define i32 @goto_one(i32 %x) {
entry:
  callbr void asm sideeffect "testl $0, $0; jne ${1:l}", "r,!i,~{dirflag},~{fpsr},~{flags}"(i32 %x)
          to label %ft [label %ind]
ft:
  ret i32 10
ind:
  %r = add i32 %x, 20
  ret i32 %r
}

; Three labels, one of them a block whose `phi` also takes the
; fallthrough's edge.
define i32 @goto_many(i32 %x) {
entry:
  %y = mul i32 %x, 3
  br label %asm
asm:
  callbr void asm sideeffect "cmpl $$1, $0; je ${1:l}; cmpl $$2, $0; je ${2:l}; cmpl $$3, $0; jb ${3:l}", "r,!i,!i,!i,~{dirflag},~{fpsr},~{flags}"(i32 %x)
          to label %ft [label %one, label %two, label %join]
ft:
  br label %join
one:
  ret i32 100
two:
  %t = add i32 %y, 200
  ret i32 %t
join:
  %j = phi i32 [ %y, %ft ], [ -7, %asm ]
  ret i32 %j
}

; A label that is also the fallthrough (with a `phi` of both edges), and
; a second label written with another modifier and with none.
define i32 @goto_self(i32 %x) {
entry:
  %y = add i32 %x, 1
  callbr void asm "testl $0, $0; js ${1:l}; cmpl $$5, $0; je ${2:P}; cmpl $$6, $0; je $2", "r,!i,!i,~{dirflag},~{fpsr},~{flags}"(i32 %x)
          to label %ft [label %ft, label %five]
ft:
  %p = phi i32 [ %y, %entry ], [ %y, %entry ]
  %q = mul i32 %p, 2
  ret i32 %q
five:
  ret i32 5000
}

; Call-frame directives (WP-212 part 1) and a label.
define i32 @goto_cfi(i32 %x) {
entry:
  callbr void asm sideeffect ".cfi_remember_state; .cfi_def_cfa_offset 64; nop; .cfi_restore_state; cmpl $$4, $0; jae ${1:l}", "r,!i,~{dirflag},~{fpsr},~{flags}"(i32 %x)
          to label %ft [label %ind]
ft:
  ret i32 1
ind:
  ret i32 2
}

declare i32 @bump(i32)

; A frame (calls, values live across the template) and every
; callee-saved register written by the template.
define i32 @goto_frame(i32 %x) {
entry:
  %a = call i32 @bump(i32 %x)
  %b = call i32 @bump(i32 %a)
  callbr void asm sideeffect "movl $$7, %ebx; movl $$8, %r12d; movl $$9, %r13d; movl $$10, %r14d; movl $$11, %r15d; testl $0, $0; jne ${1:l}", "r,!i,~{rbx},~{r12},~{r13},~{r14},~{r15},~{dirflag},~{fpsr},~{flags}"(i32 %x)
          to label %ft [label %ind]
ft:
  %c = call i32 @bump(i32 %b)
  %s = add i32 %c, %a
  ret i32 %s
ind:
  %d = mul i32 %b, 3
  %e = add i32 %d, %a
  ret i32 %e
}

; An output, read on both edges.
define i32 @goto_out(i32 %x) {
entry:
  %r = callbr i32 asm "addl $$5, $0; cmpl $$10, $0; jg ${2:l}", "=r,0,!i,~{dirflag},~{fpsr},~{flags}"(i32 %x)
          to label %ft [label %ind]
ft:
  %s = mul i32 %r, 2
  ret i32 %s
ind:
  %t = sub i32 %r, 100
  ret i32 %t
}

; An output whose only use on the fallthrough is a `phi`. (A `phi` of
; the output in an indirect destination stops `llc` 22's verifier after
; its `callbr` preparation: "PHI nodes not grouped at top of basic
; block".)
define i32 @goto_phi(i32 %x) {
entry:
  %y = add i32 %x, 9
  %r = callbr i32 asm "subl $$3, $0; testl $0, $0; js ${2:l}", "=r,0,!i,~{dirflag},~{fpsr},~{flags}"(i32 %y)
          to label %ft [label %ind]
ft:
  %p = phi i32 [ %r, %entry ]
  ret i32 %p
ind:
  %n = sub i32 0, %r
  ret i32 %n
}

; Two outputs, read on both edges.
define i32 @goto_out2(i32 %x) {
entry:
  %r = callbr { i32, i32 } asm "movl $2, $0; movl $2, $1; addl $$1, $0; addl $$2, $1; testl $2, $2; jne ${3:l}", "=&r,=&r,r,!i,~{dirflag},~{fpsr},~{flags}"(i32 %x)
          to label %ft [label %ind]
ft:
  %a = extractvalue { i32, i32 } %r, 0
  %b = extractvalue { i32, i32 } %r, 1
  %s = mul i32 %a, %b
  ret i32 %s
ind:
  %c = extractvalue { i32, i32 } %r, 0
  %d = extractvalue { i32, i32 } %r, 1
  %t = mul i32 %c, 10
  %u = add i32 %t, %d
  ret i32 %u
}

; A label back to the loop's head, whose `phi`s take the edge.
define i64 @goto_loop(i64 %n) {
entry:
  br label %head
head:
  %i = phi i64 [ 0, %entry ], [ %i1, %go ]
  %acc = phi i64 [ 0, %entry ], [ %acc1, %go ]
  %acc1 = add i64 %acc, %i
  %i1 = add i64 %i, 1
  br label %go
go:
  callbr void asm "cmpq $1, $0; jl ${2:l}", "r,r,!i,~{dirflag},~{fpsr},~{flags}"(i64 %i1, i64 %n)
          to label %out [label %head]
out:
  ret i64 %acc1
}
