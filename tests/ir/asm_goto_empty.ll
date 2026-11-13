; `asm goto` with an empty template: `callbr` of inline assembly that
; assembles to nothing goes on at its fallthrough; its indirect
; destinations are entered only from elsewhere (here an `indirectbr`
; through a `blockaddress` of one). A tied output is its input.
;@ covers: spec/ir.md §5.4 callbr
;@ stdout: void 5 -1 8
;@ stdout: out 22 4 -5
;@ stdout: loop 45
;@ stdout: addr 8 1007

@.void = private unnamed_addr constant [15 x i8] c"void %d %d %d\0A\00"
@.out = private unnamed_addr constant [14 x i8] c"out %d %d %d\0A\00"
@.loop = private unnamed_addr constant [9 x i8] c"loop %d\0A\00"
@.addr = private unnamed_addr constant [12 x i8] c"addr %d %d\0A\00"

declare i32 @printf(ptr, ...)

define internal i32 @plus_one(i32 %x) {
entry:
  callbr void asm sideeffect "", "r,!i,~{memory}"(i32 %x)
          to label %ft [label %ind]
ft:
  %a = add i32 %x, 1
  ret i32 %a
ind:
  ret i32 -100
}

define internal i32 @tied(i32 %x) {
entry:
  %y = mul i32 %x, 3
  %r = callbr i32 asm "", "=r,0,!i"(i32 %y) to label %ft [label %ind]
ind:
  ret i32 -100
ft:
  %s = add i32 %r, 7
  ret i32 %s
}

define internal i32 @sum(i32 %n) {
entry:
  br label %head
head:
  %i = phi i32 [ 0, %entry ], [ %i1, %body ]
  %acc = phi i32 [ 0, %entry ], [ %acc1, %body ]
  %c = icmp slt i32 %i, %n
  br i1 %c, label %go, label %out
go:
  callbr void asm "", "r,r,!i,!i"(i32 %i, i32 %acc)
          to label %body [label %side, label %out]
side:
  ret i32 -100
out:
  %res = phi i32 [ %acc, %head ], [ 999, %go ]
  ret i32 %res
body:
  %acc1 = add i32 %acc, %i
  %i1 = add i32 %i, 1
  br label %head
}

define internal i32 @addr(i32 %x) {
entry:
  %odd = and i32 %x, 1
  %c = icmp ne i32 %odd, 0
  br i1 %c, label %jump, label %asm
asm:
  callbr void asm sideeffect "", "r,!i"(i32 %x) to label %ft [label %ind]
ft:
  %d = shl i32 %x, 1
  ret i32 %d
jump:
  indirectbr ptr blockaddress(@addr, %ind), [label %ind]
ind:
  %v = phi i32 [ 100, %asm ], [ %x, %jump ]
  %w = add i32 %v, 1000
  ret i32 %w
}

define i32 @main() {
entry:
  %a0 = call i32 @plus_one(i32 4)
  %a1 = call i32 @plus_one(i32 -2)
  %a2 = call i32 @plus_one(i32 7)
  call i32 (ptr, ...) @printf(ptr @.void, i32 %a0, i32 %a1, i32 %a2)
  %b0 = call i32 @tied(i32 5)
  %b1 = call i32 @tied(i32 -1)
  %b2 = call i32 @tied(i32 -4)
  call i32 (ptr, ...) @printf(ptr @.out, i32 %b0, i32 %b1, i32 %b2)
  %c0 = call i32 @sum(i32 10)
  call i32 (ptr, ...) @printf(ptr @.loop, i32 %c0)
  %d0 = call i32 @addr(i32 4)
  %d1 = call i32 @addr(i32 7)
  call i32 (ptr, ...) @printf(ptr @.addr, i32 %d0, i32 %d1)
  ret i32 0
}
