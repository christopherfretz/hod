; i386 join E (4): `atomicrmw fadd` / `fsub` on `float` and `double`,
; atomic loads and stores of `float` and `double` (the 8-byte ones are
; one access: x87 `fild`/`fistp` in hod, `movq` in `llc`), `atomicrmw
; xchg` of both, in SSE2 functions (`"target-cpu"="pentium4"`); results
; used and unused, edge values (signed zero, subnormal, overflow to
; infinity, a NaN).
target triple = "i686-unknown-linux-gnu"

@af = internal global float 0.0, align 4
@ad = internal global double 0.0, align 8
@fs = internal constant [8 x float] [float 1.5, float -0.0, float 0x3800000000000000, float 0x47EFFFFFE0000000, float 0x47EFFFFFE0000000, float 0xC6293E5940000000, float 0x3FB99999A0000000, float 0x7FF8000000000000]
@ds = internal constant [8 x double] [double 0.1, double -0.0, double 4.9406564584124654e-324, double 0x7FEFFFFFFFFFFFFF, double 0x7FEFFFFFFFFFFFFF, double -2.5, double 1.0e-300, double 0x7FF8000000000000]
@fmt = private constant [31 x i8] c"%d %.9g %.9g %.17g %.17g %08x\0A\00"
@fmt2 = private constant [26 x i8] c"%.9g %d %.17g %.9g %.17g\0A\00"

declare i32 @printf(ptr, ...)

define i32 @main() #0 {
entry:
  br label %loop
loop:
  %i = phi i32 [ 0, %entry ], [ %i1, %loop ]
  %pf = getelementptr [8 x float], ptr @fs, i32 0, i32 %i
  %pd = getelementptr [8 x double], ptr @ds, i32 0, i32 %i
  %f = load float, ptr %pf
  %d = load double, ptr %pd
  %of = atomicrmw fadd ptr @af, float %f seq_cst, align 4
  %od = atomicrmw fsub ptr @ad, double %d seq_cst, align 8
  %nf = load atomic float, ptr @af acquire, align 4
  %nd = load atomic double, ptr @ad seq_cst, align 8
  %bits = load atomic i32, ptr @af seq_cst, align 4
  %ofd = fpext float %of to double
  %nfd = fpext float %nf to double
  call i32 (ptr, ...) @printf(ptr @fmt, i32 %i, double %ofd, double %nfd, double %od, double %nd, i32 %bits)
  %hf = fmul float %f, 5.000000e-01
  %qd = fmul double %d, 2.500000e-01
  %u1 = atomicrmw fsub ptr @af, float %hf monotonic, align 4
  %u2 = atomicrmw fadd ptr @ad, double %qd release, align 8
  %i1 = add i32 %i, 1
  %done = icmp eq i32 %i1, 8
  br i1 %done, label %out, label %loop
out:
  store atomic float -0.0, ptr @af seq_cst, align 4
  store atomic double 4.9406564584124654e-324, ptr @ad release, align 8
  %lf = load atomic float, ptr @af acquire, align 4
  %ld = load atomic double, ptr @ad seq_cst, align 8
  %lb = load atomic i32, ptr @af seq_cst, align 4
  %sign = lshr i32 %lb, 31
  %xf = atomicrmw xchg ptr @af, float 2.750000e+00 seq_cst, align 4
  %xd = atomicrmw xchg ptr @ad, double -5.000000e-01 seq_cst, align 8
  %lfd = fpext float %lf to double
  %xfd = fpext float %xf to double
  call i32 (ptr, ...) @printf(ptr @fmt2, double %lfd, i32 %sign, double %ld, double %xfd, double %xd)
  ret i32 0
}

attributes #0 = { "target-cpu"="pentium4" }
