; i386 join E stage 3: `bitcast` between `float` / `double` and `i32` /
; `i64` on the x87 path (no `target-cpu`: `generic`, no SSE). The bits of
; the ROUNDED value are cast (a product kept at 80 bits is rounded first),
; signalling NaN patterns pass unchanged (no `fld` of an integer's bits),
; the `i64` side is two halves; values from arguments, call results, a
; `phi`, memory; results used by x87 arithmetic and by integer code.
target triple = "i686-unknown-linux-gnu"

@fbits = internal global [8 x i32] [i32 0, i32 -2147483648, i32 1, i32 2139095040, i32 2143289344, i32 2141192193, i32 -1, i32 1065353217], align 4
@dbits = internal global [8 x i64] [i64 0, i64 -9223372036854775808, i64 1, i64 9218868437227405312, i64 9221120237041090560, i64 9218868437227405313, i64 -1, i64 4607182418800017409], align 8
@fvals = internal global [4 x float] [float 0x3FF19999A0000000, float 0x3FF3333340000000, float 0x47EFFFFFE0000000, float 0x36A0000000000000], align 4
@dvals = internal global [4 x double] [double 1.1, double 0x3FF0000000000001, double 0x7FEFFFFFFFFFFFFF, double 4.9406564584124654e-324], align 8
@slot = internal global float 0.0, align 4
@fmt1 = private constant [24 x i8] c"%d f %08x %08x %08x %d\0A\00"
@fmt2 = private constant [30 x i8] c"%d d %016llx %016llx %016llx\0A\00"
@fmt3 = private constant [27 x i8] c"%d v %08x %.9g %016llx %d\0A\00"
@fmt4 = private constant [24 x i8] c"%d c %08x %016llx %08x\0A\00"

declare i32 @printf(ptr, ...)

define internal float @fcall(float %a, float %b) noinline {
  %p = fmul float %a, %b
  ret float %p
}

define internal double @dcall(double %a, double %b) noinline {
  %p = fmul double %a, %b
  ret double %p
}

; Integer bits to `float` / `double` and back, with no arithmetic between:
; every pattern (signalling NaNs among them) comes back unchanged.
define internal void @roundtrip(i32 %i) noinline {
  %pf = getelementptr [8 x i32], ptr @fbits, i32 0, i32 %i
  %pd = getelementptr [8 x i64], ptr @dbits, i32 0, i32 %i
  %wf = load i32, ptr %pf
  %wd = load i64, ptr %pd
  %f = bitcast i32 %wf to float
  %d = bitcast i64 %wd to double
  store float %f, ptr @slot
  %back = bitcast float %f to i32
  %mem = load i32, ptr @slot
  %db = bitcast double %d to i64
  %neg = fneg float %f
  %nb = bitcast float %neg to i32
  %isnan = fcmp uno double %d, %d
  %nz = zext i1 %isnan to i32
  call i32 (ptr, ...) @printf(ptr @fmt1, i32 %i, i32 %back, i32 %mem, i32 %nb, i32 %nz)
  %dn = fneg double %d
  %dnb = bitcast double %dn to i64
  %x = xor i64 %wd, %db
  call i32 (ptr, ...) @printf(ptr @fmt2, i32 %i, i64 %db, i64 %dnb, i64 %x)
  ret void
}

; Computed values cast: the product's bits are those of the product
; rounded to its type; integer arithmetic on the bits, then back.
define internal void @computed(i32 %i, float %arg) noinline {
  %pf = getelementptr [4 x float], ptr @fvals, i32 0, i32 %i
  %pd = getelementptr [4 x double], ptr @dvals, i32 0, i32 %i
  %a = load float, ptr %pf
  %d = load double, ptr %pd
  %p = fmul float %a, %a
  %pb = bitcast float %p to i32
  %q = fdiv float %p, 3.0
  %qb = bitcast float %q to i32
  %ab = bitcast float %arg to i32
  %up = add i32 %pb, 1
  %uf = bitcast i32 %up to float
  %s = fadd float %uf, %p
  %sd = fpext float %s to double
  %dp = fmul double %d, %d
  %dpb = bitcast double %dp to i64
  %cmp = icmp slt i32 %pb, %qb
  %cz = zext i1 %cmp to i32
  call i32 (ptr, ...) @printf(ptr @fmt3, i32 %i, i32 %qb, double %sd, i64 %dpb, i32 %cz)
  %cf = call float @fcall(float %a, float %a)
  %cfb = bitcast float %cf to i32
  %cd = call double @dcall(double %d, double %d)
  %cdb = bitcast double %cd to i64
  %abx = xor i32 %ab, %cfb
  call i32 (ptr, ...) @printf(ptr @fmt4, i32 %i, i32 %cfb, i64 %cdb, i32 %abx)
  ret void
}

; A cast in a block of its own, of a value of a `phi`, and the `phi` of a
; cast.
define internal i64 @phis(i1 %c, double %x, double %y) noinline {
entry:
  %m = fmul double %x, %y
  br i1 %c, label %a, label %b
a:
  %ab = bitcast double %m to i64
  br label %join
b:
  %s = fadd double %x, %y
  %sb = bitcast double %s to i64
  br label %join
join:
  %r = phi i64 [ %ab, %a ], [ %sb, %b ]
  %back = bitcast i64 %r to double
  %h = fmul double %back, 5.000000e-01
  %hb = bitcast double %h to i64
  %o = xor i64 %hb, %r
  ret i64 %o
}

define i32 @main() {
entry:
  br label %rt
rt:
  %i = phi i32 [ 0, %entry ], [ %i1, %rt ]
  call void @roundtrip(i32 %i)
  %i1 = add i32 %i, 1
  %d1 = icmp eq i32 %i1, 8
  br i1 %d1, label %cp, label %rt
cp:
  %j = phi i32 [ 0, %rt ], [ %j1, %cp ]
  %jf = sitofp i32 %j to float
  %arg = fmul float %jf, 0x3FB99999A0000000
  call void @computed(i32 %j, float %arg)
  %j1 = add i32 %j, 1
  %d2 = icmp eq i32 %j1, 4
  br i1 %d2, label %ph, label %cp
ph:
  %p1 = call i64 @phis(i1 true, double 1.1, double 2.2)
  %p2 = call i64 @phis(i1 false, double 1.1, double 2.2)
  call i32 (ptr, ...) @printf(ptr @fmt2, i32 9, i64 %p1, i64 %p2, i64 0)
  ret i32 0
}
