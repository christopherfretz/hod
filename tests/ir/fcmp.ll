; All sixteen fcmp predicates on double and on float, including unordered
; operands (a quiet NaN, which is compared but never printed) and the two
; zeros, plus conditional branches on floating-point comparisons.
;
; Each line shows the seven ordered predicates (oeq ogt oge olt ole one
; ord), the seven unordered ones (ueq ugt uge ult ule une uno), then true
; and false.
;@ covers: spec/ir.md §5.4 compare (fcmp)
;@ stdout: 1 < 2      0001111 0001110 10
;@ stdout: 2 > 1      0110011 0110010 10
;@ stdout: 2 == 2     1010101 1010100 10
;@ stdout: nan ? 1    0000000 1111111 10
;@ stdout: 1 ? nan    0000000 1111111 10
;@ stdout: -0 == 0    1010101 1010100 10
;@ stdout: inf > -inf 0110011 0110010 10
;@ stdout: float <    0001111 0001110 10
;@ stdout: float nan  0000000 1111111 10
;@ stdout: -2.5 is negative
;@ stdout: -0.0 is zero
;@ stdout: 1e300 is positive
;@ stdout: nan is unordered

@.row = private unnamed_addr constant [42 x i8] c"%-10s %d%d%d%d%d%d%d %d%d%d%d%d%d%d %d%d\0A\00"
@.class = private unnamed_addr constant [10 x i8] c"%s is %s\0A\00"
@.lt = private unnamed_addr constant [6 x i8] c"1 < 2\00"
@.gt = private unnamed_addr constant [6 x i8] c"2 > 1\00"
@.eq = private unnamed_addr constant [7 x i8] c"2 == 2\00"
@.nanl = private unnamed_addr constant [8 x i8] c"nan ? 1\00"
@.nanr = private unnamed_addr constant [8 x i8] c"1 ? nan\00"
@.zeros = private unnamed_addr constant [8 x i8] c"-0 == 0\00"
@.infs = private unnamed_addr constant [11 x i8] c"inf > -inf\00"
@.flt = private unnamed_addr constant [8 x i8] c"float <\00"
@.fnan = private unnamed_addr constant [10 x i8] c"float nan\00"
@.negative = private unnamed_addr constant [9 x i8] c"negative\00"
@.zero = private unnamed_addr constant [5 x i8] c"zero\00"
@.positive = private unnamed_addr constant [9 x i8] c"positive\00"
@.unordered = private unnamed_addr constant [10 x i8] c"unordered\00"
@.c1 = private unnamed_addr constant [5 x i8] c"-2.5\00"
@.c2 = private unnamed_addr constant [5 x i8] c"-0.0\00"
@.c3 = private unnamed_addr constant [6 x i8] c"1e300\00"
@.c4 = private unnamed_addr constant [4 x i8] c"nan\00"

declare i32 @printf(ptr, ...)

define void @row(ptr %label, double %a, double %b) {
  %oeq = fcmp oeq double %a, %b
  %ogt = fcmp ogt double %a, %b
  %oge = fcmp oge double %a, %b
  %olt = fcmp olt double %a, %b
  %ole = fcmp ole double %a, %b
  %one = fcmp one double %a, %b
  %ord = fcmp ord double %a, %b
  %ueq = fcmp ueq double %a, %b
  %ugt = fcmp ugt double %a, %b
  %uge = fcmp uge double %a, %b
  %ult = fcmp ult double %a, %b
  %ule = fcmp ule double %a, %b
  %une = fcmp une double %a, %b
  %uno = fcmp uno double %a, %b
  %tru = fcmp true double %a, %b
  %fal = fcmp false double %a, %b
  %z0 = zext i1 %oeq to i32
  %z1 = zext i1 %ogt to i32
  %z2 = zext i1 %oge to i32
  %z3 = zext i1 %olt to i32
  %z4 = zext i1 %ole to i32
  %z5 = zext i1 %one to i32
  %z6 = zext i1 %ord to i32
  %z7 = zext i1 %ueq to i32
  %z8 = zext i1 %ugt to i32
  %z9 = zext i1 %uge to i32
  %z10 = zext i1 %ult to i32
  %z11 = zext i1 %ule to i32
  %z12 = zext i1 %une to i32
  %z13 = zext i1 %uno to i32
  %z14 = zext i1 %tru to i32
  %z15 = zext i1 %fal to i32
  call i32 (ptr, ...) @printf(ptr @.row, ptr %label, i32 %z0, i32 %z1, i32 %z2, i32 %z3, i32 %z4, i32 %z5, i32 %z6, i32 %z7, i32 %z8, i32 %z9, i32 %z10, i32 %z11, i32 %z12, i32 %z13, i32 %z14, i32 %z15)
  ret void
}

define void @rowf(ptr %label, float %a, float %b) {
  %oeq = fcmp oeq float %a, %b
  %ogt = fcmp ogt float %a, %b
  %oge = fcmp oge float %a, %b
  %olt = fcmp olt float %a, %b
  %ole = fcmp ole float %a, %b
  %one = fcmp one float %a, %b
  %ord = fcmp ord float %a, %b
  %ueq = fcmp ueq float %a, %b
  %ugt = fcmp ugt float %a, %b
  %uge = fcmp uge float %a, %b
  %ult = fcmp ult float %a, %b
  %ule = fcmp ule float %a, %b
  %une = fcmp une float %a, %b
  %uno = fcmp uno float %a, %b
  %tru = fcmp true float %a, %b
  %fal = fcmp false float %a, %b
  %z0 = zext i1 %oeq to i32
  %z1 = zext i1 %ogt to i32
  %z2 = zext i1 %oge to i32
  %z3 = zext i1 %olt to i32
  %z4 = zext i1 %ole to i32
  %z5 = zext i1 %one to i32
  %z6 = zext i1 %ord to i32
  %z7 = zext i1 %ueq to i32
  %z8 = zext i1 %ugt to i32
  %z9 = zext i1 %uge to i32
  %z10 = zext i1 %ult to i32
  %z11 = zext i1 %ule to i32
  %z12 = zext i1 %une to i32
  %z13 = zext i1 %uno to i32
  %z14 = zext i1 %tru to i32
  %z15 = zext i1 %fal to i32
  call i32 (ptr, ...) @printf(ptr @.row, ptr %label, i32 %z0, i32 %z1, i32 %z2, i32 %z3, i32 %z4, i32 %z5, i32 %z6, i32 %z7, i32 %z8, i32 %z9, i32 %z10, i32 %z11, i32 %z12, i32 %z13, i32 %z14, i32 %z15)
  ret void
}

; Branches on comparisons; a NaN fails all three ordered tests.
define ptr @classify(double %x) {
entry:
  %neg = fcmp olt double %x, 0.0
  br i1 %neg, label %is.negative, label %not.negative
not.negative:
  %zero = fcmp oeq double %x, 0.0
  br i1 %zero, label %is.zero, label %not.zero
not.zero:
  %pos = fcmp ogt double %x, 0.0
  br i1 %pos, label %is.positive, label %is.unordered
is.negative:
  ret ptr @.negative
is.zero:
  ret ptr @.zero
is.positive:
  ret ptr @.positive
is.unordered:
  ret ptr @.unordered
}

define i32 @main() {
  call void @row(ptr @.lt, double 1.0, double 2.0)
  call void @row(ptr @.gt, double 2.0, double 1.0)
  call void @row(ptr @.eq, double 2.0, double 2.0)
  call void @row(ptr @.nanl, double 0x7FF8000000000000, double 1.0)
  call void @row(ptr @.nanr, double 1.0, double 0x7FF8000000000000)
  call void @row(ptr @.zeros, double -0.0, double 0.0)
  call void @row(ptr @.infs, double 0x7FF0000000000000, double 0xFFF0000000000000)
  call void @rowf(ptr @.flt, float -1.5, float 0.5)
  call void @rowf(ptr @.fnan, float 0x7FF8000000000000, float 0x7FF8000000000000)
  %c1 = call ptr @classify(double -2.5)
  call i32 (ptr, ...) @printf(ptr @.class, ptr @.c1, ptr %c1)
  %c2 = call ptr @classify(double -0.0)
  call i32 (ptr, ...) @printf(ptr @.class, ptr @.c2, ptr %c2)
  %c3 = call ptr @classify(double 1.0e300)
  call i32 (ptr, ...) @printf(ptr @.class, ptr @.c3, ptr %c3)
  %c4 = call ptr @classify(double 0x7FF8000000000000)
  call i32 (ptr, ...) @printf(ptr @.class, ptr @.c4, ptr %c4)
  ret i32 0
}
