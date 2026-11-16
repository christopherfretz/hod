; WP-315: first-class aggregates on i386. extractvalue / insertvalue,
; load / store, phi, select (variable and constant arms, `{}`), members
; i1 / i24 / i40 / i64 / ptr / float / double / x86_fp80, nested and
; zero-sized members, arguments and returns in registers and in memory,
; each printed and checked against llc's run.
target datalayout = "e-m:e-p:32:32-p270:32:32-p271:32:32-p272:64:64-i128:128-f64:32:64-f80:32-n8:16:32-S128"
target triple = "i686-unknown-linux-gnu"

%mix = type { i1, i24, i40, i64, ptr, float, double, x86_fp80 }
%nest = type { i8, {}, { i16, [0 x i32], [3 x i8] }, [2 x { i32, i1 }] }
%pair = type { i32, i32 }
%big = type { [5 x i32], double }

@fmt.int = private constant [9 x i8] c"%s %llx\0A\00"
@fmt.fp = private constant [10 x i8] c"%s %.17g\0A\00"
@fmt.ld = private constant [11 x i8] c"%s %.21Lg\0A\00"
@done = private constant [5 x i8] c"done\00"
@n.a = private constant [2 x i8] c"a\00"
@n.b = private constant [2 x i8] c"b\00"
@n.c = private constant [2 x i8] c"c\00"
@n.d = private constant [2 x i8] c"d\00"
@n.e = private constant [2 x i8] c"e\00"
@n.f = private constant [2 x i8] c"f\00"
@n.g = private constant [2 x i8] c"g\00"
@n.h = private constant [2 x i8] c"h\00"
@marker = global i32 77
@gmix = global %mix { i1 true, i24 -3, i40 549755813887, i64 -81985529216486896, ptr @marker, float 1.5, double -2.25, x86_fp80 0xK4000C000000000000000 }
@gnest = global %nest zeroinitializer
@gbig = global %big { [5 x i32] [i32 1, i32 2, i32 3, i32 4, i32 5], double 6.5 }

declare i32 @printf(ptr, ...)
declare i32 @puts(ptr)

define internal void @pint(ptr %n, i64 %v) nounwind {
  %r = call i32 (ptr, ...) @printf(ptr @fmt.int, ptr %n, i64 %v)
  ret void
}
define internal void @pfp(ptr %n, double %v) nounwind {
  %r = call i32 (ptr, ...) @printf(ptr @fmt.fp, ptr %n, double %v)
  ret void
}
define internal void @pld(ptr %n, x86_fp80 %v) nounwind {
  %r = call i32 (ptr, ...) @printf(ptr @fmt.ld, ptr %n, x86_fp80 %v)
  ret void
}

define internal void @show(%mix %m) nounwind {
  %a = extractvalue %mix %m, 0
  %a64 = zext i1 %a to i64
  call void @pint(ptr @n.a, i64 %a64)
  %b = extractvalue %mix %m, 1
  %b64 = sext i24 %b to i64
  call void @pint(ptr @n.b, i64 %b64)
  %c = extractvalue %mix %m, 2
  %c64 = zext i40 %c to i64
  call void @pint(ptr @n.c, i64 %c64)
  %d = extractvalue %mix %m, 3
  call void @pint(ptr @n.d, i64 %d)
  %e = extractvalue %mix %m, 4
  %e1 = load i32, ptr %e
  %e64 = sext i32 %e1 to i64
  call void @pint(ptr @n.e, i64 %e64)
  %f = extractvalue %mix %m, 5
  %f64 = fpext float %f to double
  call void @pfp(ptr @n.f, double %f64)
  %g = extractvalue %mix %m, 6
  call void @pfp(ptr @n.g, double %g)
  %h = extractvalue %mix %m, 7
  call void @pld(ptr @n.h, x86_fp80 %h)
  ret void
}

; Built member by member from arguments; returned in memory.
define internal %mix @build(i1 %a, i24 %b, i40 %c, i64 %d, ptr %e, float %f, double %g, x86_fp80 %h) nounwind {
  %m0 = insertvalue %mix undef, i1 %a, 0
  %m1 = insertvalue %mix %m0, i24 %b, 1
  %m2 = insertvalue %mix %m1, i40 %c, 2
  %m3 = insertvalue %mix %m2, i64 %d, 3
  %m4 = insertvalue %mix %m3, ptr %e, 4
  %m5 = insertvalue %mix %m4, float %f, 5
  %m6 = insertvalue %mix %m5, double %g, 6
  %m7 = insertvalue %mix %m6, x86_fp80 %h, 7
  ret %mix %m7
}

; Small aggregates in registers, both ways.
define internal %pair @swap(%pair %p) nounwind {
  %x = extractvalue %pair %p, 0
  %y = extractvalue %pair %p, 1
  %q0 = insertvalue %pair poison, i32 %y, 0
  %q1 = insertvalue %pair %q0, i32 %x, 1
  ret %pair %q1
}
define internal { i64 } @wide1(i64 %v) nounwind {
  %r = insertvalue { i64 } undef, i64 %v, 0
  ret { i64 } %r
}
define internal { i40, i1 } @flag(i40 %v, i1 %f) nounwind {
  %r0 = insertvalue { i40, i1 } undef, i40 %v, 0
  %r1 = insertvalue { i40, i1 } %r0, i1 %f, 1
  ret { i40, i1 } %r1
}
define internal {} @empty({} %e) nounwind {
  ret {} %e
}
define internal %big @bigid(%big %b, i32 %k) nounwind {
  %w = extractvalue %big %b, 0, 2
  %w1 = add i32 %w, %k
  %r = insertvalue %big %b, i32 %w1, 0, 2
  ret %big %r
}

; phi and select of aggregates, a double live across them.
define internal %mix @choose(i32 %k, %mix %x, %mix %y, double %z) nounwind {
entry:
  %c = icmp sgt i32 %k, 0
  %s = select i1 %c, %mix %x, %mix %y
  %zz = fmul double %z, 3.0
  br i1 %c, label %then, label %join
then:
  %t = insertvalue %mix %s, double %zz, 6
  br label %join
join:
  %p = phi %mix [ %s, %entry ], [ %t, %then ]
  ret %mix %p
}

define internal %pair @consts(i1 %c) nounwind {
  %s = select i1 %c, %pair { i32 1, i32 2 }, %pair { i32 3, i32 4 }
  ret %pair %s
}

define i32 @main() nounwind {
entry:
  %m = load %mix, ptr @gmix
  call void @show(%mix %m)
  %b = call %mix @build(i1 false, i24 8388607, i40 -1, i64 1311768467463790320, ptr @marker, float -0.5, double 1.0e300, x86_fp80 0xK3FFF8000000000000001)
  call void @show(%mix %b)
  %slot = alloca %mix
  store %mix %b, ptr %slot
  %b2 = load %mix, ptr %slot
  %b3 = insertvalue %mix %b2, i40 4660, 2
  store %mix %b3, ptr @gmix
  %m2 = load %mix, ptr @gmix
  call void @show(%mix %m2)
  %c1 = call %mix @choose(i32 1, %mix %m, %mix %b, double 2.5)
  call void @show(%mix %c1)
  %c2 = call %mix @choose(i32 -1, %mix %m, %mix %b, double 2.5)
  call void @show(%mix %c2)
  %p = call %pair @swap(%pair { i32 10, i32 20 })
  %px = extractvalue %pair %p, 0
  %px64 = zext i32 %px to i64
  call void @pint(ptr @n.a, i64 %px64)
  %k1 = call %pair @consts(i1 true)
  %k2 = call %pair @consts(i1 false)
  %k1y = extractvalue %pair %k1, 1
  %k2y = extractvalue %pair %k2, 1
  %ks = mul i32 %k1y, 10
  %ks1 = add i32 %ks, %k2y
  %ks64 = zext i32 %ks1 to i64
  call void @pint(ptr @n.b, i64 %ks64)
  %w = call { i64 } @wide1(i64 -2)
  %wv = extractvalue { i64 } %w, 0
  call void @pint(ptr @n.c, i64 %wv)
  %fl = call { i40, i1 } @flag(i40 1099511627775, i1 true)
  %flv = extractvalue { i40, i1 } %fl, 0
  %flf = extractvalue { i40, i1 } %fl, 1
  %flv64 = zext i40 %flv to i64
  %flf64 = zext i1 %flf to i64
  %fls = add i64 %flv64, %flf64
  call void @pint(ptr @n.d, i64 %fls)
  %e = call {} @empty({} zeroinitializer)
  ; nested and zero-sized members
  %n0 = load %nest, ptr @gnest
  %n1 = insertvalue %nest %n0, i8 -7, 0
  %n2 = insertvalue %nest %n1, i16 300, 2, 0
  %n3 = insertvalue %nest %n2, i8 9, 2, 2, 1
  %n4 = insertvalue %nest %n3, i32 -5, 3, 1, 0
  %n5 = insertvalue %nest %n4, i1 true, 3, 1, 1
  %n6 = insertvalue %nest %n5, {} {}, 1
  store %nest %n6, ptr @gnest
  %n7 = load %nest, ptr @gnest
  %na = extractvalue %nest %n7, 0
  %nb = extractvalue %nest %n7, 2, 0
  %nc = extractvalue %nest %n7, 2, 2, 1
  %nd = extractvalue %nest %n7, 3, 1
  %nd0 = extractvalue { i32, i1 } %nd, 0
  %nd1 = extractvalue { i32, i1 } %nd, 1
  %na64 = sext i8 %na to i64
  %nb64 = zext i16 %nb to i64
  %nc64 = zext i8 %nc to i64
  %nd064 = sext i32 %nd0 to i64
  %nd164 = zext i1 %nd1 to i64
  %s1 = shl i64 %na64, 40
  %s2 = shl i64 %nb64, 24
  %s3 = shl i64 %nc64, 16
  %s4 = shl i64 %nd164, 8
  %t1 = xor i64 %s1, %s2
  %t2 = xor i64 %t1, %s3
  %t3 = xor i64 %t2, %s4
  %t4 = xor i64 %t3, %nd064
  call void @pint(ptr @n.e, i64 %t4)
  ; an aggregate argument and return in memory
  %g = load %big, ptr @gbig
  %g2 = call %big @bigid(%big %g, i32 1000)
  %gw = extractvalue %big %g2, 0, 2
  %gw64 = zext i32 %gw to i64
  call void @pint(ptr @n.f, i64 %gw64)
  %gd = extractvalue %big %g2, 1
  call void @pfp(ptr @n.g, double %gd)
  %d = call i32 @puts(ptr @done)
  ret i32 0
}
