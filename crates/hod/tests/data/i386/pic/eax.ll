; WP-299: the programs that would make pic::mem_inst build an access over a
; symbol that reads eax (which holds the GOT pointer or the symbol's address
; under PIC): a call's result (returned in eax) stored straight to a global,
; an argument and a loaded value stored to globals of every width (1, 2, 3, 4
; bytes), a symbol's address stored to a symbol, a quotient (computed in eax)
; stored to a global, each to a preemptible global (R_386_GOT32X) and a
; hidden one (R_386_GOTOFF). main's exit status is a checksum of all of them.
target datalayout = "e-m:e-p:32:32-p270:32:32-p271:32:32-p272:64:64-i128:128-f64:32:64-f80:32-n8:16:32-S128"
target triple = "i686-unknown-linux-gnu"

@g8 = global i8 0
@g16 = global i16 0
@g24 = global [4 x i8] zeroinitializer
@g32 = global i32 0
@gp = global ptr null
@h8 = hidden global i8 0
@h16 = hidden global i16 0
@h32 = hidden global i32 0
@hp = hidden global ptr null

define i32 @ret32(i32 %x) noinline nounwind {
  %r = mul i32 %x, 3
  ret i32 %r
}

define i8 @ret8(i8 %x) noinline nounwind {
  %r = add i8 %x, 1
  ret i8 %r
}

define i16 @ret16(i16 %x) noinline nounwind {
  %r = add i16 %x, 2
  ret i16 %r
}

define void @stores(i32 %x, i8 %b, i16 %w, ptr %p) noinline nounwind {
  %r = call i32 @ret32(i32 %x)
  store i32 %r, ptr @g32
  %r2 = call i32 @ret32(i32 %r)
  store i32 %r2, ptr @h32
  %c = call i8 @ret8(i8 %b)
  store i8 %c, ptr @g8
  %c2 = call i8 @ret8(i8 %c)
  store i8 %c2, ptr @h8
  %s = call i16 @ret16(i16 %w)
  store i16 %s, ptr @g16
  store i16 %w, ptr @h16
  %t = trunc i32 %r to i24
  store i24 %t, ptr @g24
  store ptr %p, ptr @gp
  store ptr @g32, ptr @hp
  ret void
}

define void @quotients(i32 %x, i32 %y) noinline nounwind {
  %q = sdiv i32 %x, %y
  store i32 %q, ptr @g32
  %m = urem i32 %x, %y
  store i32 %m, ptr @h32
  %l = load i32, ptr @g32
  %n = add i32 %l, %m
  store i32 %n, ptr @g32
  ret void
}

define i32 @main() nounwind {
  call void @stores(i32 5, i8 7, i16 300, ptr @h8)
  %a = load i32, ptr @g32
  %b = load i32, ptr @h32
  %c = load i8, ptr @g8
  %d = load i8, ptr @h8
  %e = load i16, ptr @g16
  %f = load i16, ptr @h16
  %g = load i24, ptr @g24
  %p = load ptr, ptr @gp
  %pq = load ptr, ptr @hp
  %ok1 = icmp eq ptr %p, @h8
  %ok2 = icmp eq ptr %pq, @g32
  %ok = and i1 %ok1, %ok2
  %c32 = zext i8 %c to i32
  %d32 = zext i8 %d to i32
  %e32 = zext i16 %e to i32
  %f32 = zext i16 %f to i32
  %g32v = zext i24 %g to i32
  %s1 = add i32 %a, %b
  %s2 = add i32 %s1, %c32
  %s3 = add i32 %s2, %d32
  %s4 = add i32 %s3, %e32
  %s5 = add i32 %s4, %f32
  %s6 = add i32 %s5, %g32v
  call void @quotients(i32 47, i32 5)
  %q = load i32, ptr @g32
  %m = load i32, ptr @h32
  %s7 = add i32 %s6, %q
  %s8 = add i32 %s7, %m
  %bad = select i1 %ok, i32 0, i32 1000
  %s9 = add i32 %s8, %bad
  %r = urem i32 %s9, 251
  ret i32 %r
}
