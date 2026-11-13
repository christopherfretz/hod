; Drift seed (design 0006 §8 risk 7, WP-130): dead instructions and
; freeze (spec/codegen.md §4.7). An instruction with no use and no
; effect is not executed: a dead division by a zero computed at run time
; and a dead signed division of the smallest value by -1 do not trap
; (x86-64's div raises #DE on both, AArch64's sdiv/udiv never trap), and
; dead loads through a null pointer and through a pointer far outside
; any object do not fault, as at -O0 llc's selectors drop them. A frozen
; poison value is one value for all its uses: x - x is 0 and x == x;
; freeze of a defined value is that value. Both targets must print the
; same. Operands come from memory, so nothing is folded. Expected values
; derived by hand, not by a compiler.
;@ covers: spec/ir.md §5.4 integer (udiv, sdiv, urem, srem with no use), memory (load with no use), freeze; spec/codegen.md §4.7 (dead results, freeze)
;@ stdout: dead: survived
;@ stdout: freeze: sub 0 eq 1 xor 0 defined 1234 -7

@in = global [4 x i64] [i64 0, i64 -9223372036854775808, i64 -1, i64 1234]
@far = global i64 -4096

@.f1 = private unnamed_addr constant [16 x i8] c"dead: survived\0A\00"
@.f2 = private unnamed_addr constant [47 x i8] c"freeze: sub %lld eq %d xor %d defined %lld %d\0A\00"

declare i32 @printf(ptr, ...)

define internal i64 @in_at(i32 %i) {
  %p = getelementptr [4 x i64], ptr @in, i32 0, i32 %i
  %v = load i64, ptr %p
  ret i64 %v
}

define internal void @dead() {
  %zero = call i64 @in_at(i32 0)
  %min = call i64 @in_at(i32 1)
  %m1 = call i64 @in_at(i32 2)
  %x = call i64 @in_at(i32 3)
  %d1 = udiv i64 %x, %zero
  %d2 = sdiv i64 %min, %m1
  %d3 = urem i64 %x, %zero
  %d4 = srem i64 %min, %m1
  %z32 = trunc i64 %zero to i32
  %x32 = trunc i64 %x to i32
  %d5 = sdiv i32 %x32, %z32
  %null = inttoptr i64 %zero to ptr
  %l1 = load i64, ptr %null
  %faraddr = load i64, ptr @far
  %farp = inttoptr i64 %faraddr to ptr
  %l2 = load i32, ptr %farp
  call i32 (ptr, ...) @printf(ptr @.f1)
  ret void
}

define internal void @freezes() {
  %x = call i64 @in_at(i32 3)
  %big = call i64 @in_at(i32 1)
  ; poison: the smallest value minus one under nsw
  %p = sub nsw i64 %big, 1
  %f = freeze i64 %p
  %s = sub i64 %f, %f
  %e = icmp eq i64 %f, %f
  %e32 = zext i1 %e to i32
  %p32 = trunc i64 %p to i32
  %f32 = freeze i32 %p32
  %x32 = xor i32 %f32, %f32
  %fx = freeze i64 %x
  %m7 = sub i32 0, 7
  %fm7 = freeze i32 %m7
  call i32 (ptr, ...) @printf(ptr @.f2, i64 %s, i32 %e32, i32 %x32, i64 %fx, i32 %fm7)
  ret void
}

define i32 @main() {
  call void @dead()
  call void @freezes()
  ret i32 0
}
