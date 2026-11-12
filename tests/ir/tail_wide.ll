; WP-084: conversions between floating point and i100/i128 (the calls
; llc makes: __fixdfti, __floattidf and their kind), a complex long
; double's fp128 parts through extractvalue/insertvalue, unsigned i128
; multiplication with overflow, and a switch on i128. The signed
; multiplication is not here: llc 19 calls compiler-rt's __muloti4 for
; it, which libgcc does not have (LLVM 22 and hod expand it in line); it
; is compared with llc 22 and the interpreter in
; crates/hod_a64/tests/tail.rs.
;@ covers: spec/ir.md §5.4 cast, aggregate (ExtractValue, InsertValue of fp128), terminator (Switch at i128), §5.5 intrinsics (umul.with.overflow wide); spec/codegen.md §5.5.1 (WP-084)
;@ tier: quick
;@ unsupported(x86_64): *
;@ stdout: conv 2426047410323587072 8 12345 12345
;@ stdout: conv -7250000000000000000 0 -2 1.2676506e+30
;@ stdout: conv 3 0 7.5557863725914323e+22 7.55578637e+22
;@ stdout: cplx -42
;@ stdout: cplx 3000000
;@ stdout: mulo 0 35 0
;@ stdout: mulo 18446744073709551615 0 1
;@ stdout: mulo 18446744073709551615 18446744073709551614 0
;@ stdout: mulo 0 0 1
;@ stdout: sw 10
;@ stdout: sw 20
;@ stdout: sw 30
;@ stdout: sw 40

@.conv = private unnamed_addr constant [27 x i8] c"conv %lld %llu %.17g %.9g\0A\00"
@.cplx = private unnamed_addr constant [11 x i8] c"cplx %lld\0A\00"
@.mulo = private unnamed_addr constant [19 x i8] c"mulo %llu %llu %d\0A\00"
@.sw = private unnamed_addr constant [9 x i8] c"sw %lld\0A\00"

declare i32 @printf(ptr, ...)
declare double @llvm.fabs.f64(double)
declare { i128, i1 } @llvm.umul.with.overflow.i128(i128, i128)

define void @conv(double %d, i64 %hi, i64 %lo) {
  %s = fptosi double %d to i128
  %s64 = trunc i128 %s to i64
  %da = call double @llvm.fabs.f64(double %d)
  %u = fptoui double %da to i100
  %u1 = lshr i100 %u, 64
  %u64 = trunc i100 %u1 to i64
  %h = zext i64 %hi to i128
  %h2 = shl i128 %h, 64
  %l = zext i64 %lo to i128
  %x = or i128 %h2, %l
  %f = sitofp i128 %x to double
  %y = trunc i128 %x to i100
  %g = uitofp i100 %y to float
  %gd = fpext float %g to double
  call i32 (ptr, ...) @printf(ptr @.conv, i64 %s64, i64 %u64, double %f, double %gd)
  ret void
}

define { fp128, fp128 } @mk(fp128 %re, fp128 %im) {
  %a = insertvalue { fp128, fp128 } undef, fp128 %re, 0
  %b = insertvalue { fp128, fp128 } %a, fp128 %im, 1
  ret { fp128, fp128 } %b
}

define void @cplx(i64 %a, i64 %b) {
  %x = sitofp i64 %a to fp128
  %y = sitofp i64 %b to fp128
  %c = call { fp128, fp128 } @mk(fp128 %x, fp128 %y)
  %re = extractvalue { fp128, fp128 } %c, 0
  %im = extractvalue { fp128, fp128 } %c, 1
  %m = fmul fp128 %re, %im
  %r = fptosi fp128 %m to i64
  call i32 (ptr, ...) @printf(ptr @.cplx, i64 %r)
  ret void
}

define void @mulo(i64 %ah, i64 %al, i64 %b) {
  %h = zext i64 %ah to i128
  %h2 = shl i128 %h, 64
  %l = zext i64 %al to i128
  %a = or i128 %h2, %l
  %bb = sext i64 %b to i128
  %u = call { i128, i1 } @llvm.umul.with.overflow.i128(i128 %a, i128 %bb)
  %uv = extractvalue { i128, i1 } %u, 0
  %uo = extractvalue { i128, i1 } %u, 1
  %uvh = lshr i128 %uv, 64
  %p1 = trunc i128 %uvh to i64
  %p0 = trunc i128 %uv to i64
  %f0 = zext i1 %uo to i32
  call i32 (ptr, ...) @printf(ptr @.mulo, i64 %p1, i64 %p0, i32 %f0)
  ret void
}

define void @sw(i64 %hi, i64 %lo) {
entry:
  %h = zext i64 %hi to i128
  %h2 = shl i128 %h, 64
  %l = zext i64 %lo to i128
  %x = or i128 %h2, %l
  switch i128 %x, label %d [
    i128 0, label %a
    i128 18446744073709551616, label %b
    i128 -1, label %c
  ]
a:
  br label %out
b:
  br label %out
c:
  br label %out
d:
  br label %out
out:
  %r = phi i64 [ 10, %a ], [ 20, %b ], [ 30, %c ], [ 40, %d ]
  call i32 (ptr, ...) @printf(ptr @.sw, i64 %r)
  ret void
}

define i32 @main() {
  call void @conv(double 1.5e20, i64 0, i64 12345)
  call void @conv(double -7.25e18, i64 -1, i64 -2)
  call void @conv(double 3.0, i64 4096, i64 0)
  call void @cplx(i64 6, i64 -7)
  call void @cplx(i64 1000000, i64 3)
  call void @mulo(i64 0, i64 5, i64 7)
  call void @mulo(i64 1, i64 0, i64 -1)
  call void @mulo(i64 9223372036854775807, i64 -1, i64 2)
  call void @mulo(i64 4294967296, i64 0, i64 4294967296)
  call void @sw(i64 0, i64 0)
  call void @sw(i64 1, i64 0)
  call void @sw(i64 -1, i64 -1)
  call void @sw(i64 0, i64 -1)
  ret i32 0
}
