; Floating-point intrinsics on double and float: sqrt (of a negative too),
; fabs, floor, ceil, trunc, rint, nearbyint, round, roundeven at halves and
; signed zeros; minimum/maximum (NaN and signed zeros), minnum/maxnum
; (NaN, not signed zeros, whose order they leave open), copysign; fma
; against a separate multiply and add. Operands come from memory. A NaN
; prints as "nan" whatever its sign and payload (the targets' default NaNs
; differ in sign). Expected values computed by a script, not a compiler.
;@ covers: spec/ir.md §5.5 intrinsics (sqrt, fabs, floor, ceil, trunc, rint, nearbyint, round, roundeven, minimum, maximum, minnum, maxnum, copysign, fma)
;@ link: m
;@ stdout: double 2.5: 1.5811388300841898 2.5 2 3 2 2 2 3 2
;@ stdout: double -2.5: nan 2.5 -3 -2 -2 -2 -2 -3 -2
;@ stdout: double 0.5: 0.70710678118654757 0.5 0 1 0 0 0 1 0
;@ stdout: double -1.5: nan 1.5 -2 -1 -1 -2 -2 -2 -2
;@ stdout: double 3.75: 1.9364916731037085 3.75 3 4 3 4 4 4 4
;@ stdout: double -0: -0 0 -0 -0 -0 -0 -0 -0 -0
;@ stdout: double -0.25: nan 0.25 -1 -0 -0 -0 -0 -0 -0
;@ stdout: double 1e+30: 1000000000000000 1e+30 1e+30 1e+30 1e+30 1e+30 1e+30 1e+30 1e+30
;@ stdout: double -7: nan 7 -7 -7 -7 -7 -7 -7 -7
;@ stdout: double 9: 3 9 9 9 9 9 9 9 9
;@ stdout: double 1.5, -2: minimum -2 maximum 1.5 copysign -1.5 minnum -2 maxnum 1.5
;@ stdout: double -0, 0: minimum -0 maximum 0 copysign 0
;@ stdout: double 0, -0: minimum -0 maximum 0 copysign -0
;@ stdout: double 3, nan: minimum nan maximum nan minnum 3 maxnum 3
;@ stdout: double nan, -4: minimum nan maximum nan copysign nan minnum -4 maxnum -4
;@ stdout: double fma(0.1,10,-1) 5.5511151231257827e-17 mul-add 0
;@ stdout: float 2.5: 1.5811388492584229 2.5 2 3 2 2 2 3 2
;@ stdout: float -2.5: nan 2.5 -3 -2 -2 -2 -2 -3 -2
;@ stdout: float 0.5: 0.70710676908493042 0.5 0 1 0 0 0 1 0
;@ stdout: float -1.5: nan 1.5 -2 -1 -1 -2 -2 -2 -2
;@ stdout: float 3.75: 1.9364917278289795 3.75 3 4 3 4 4 4 4
;@ stdout: float -0: -0 0 -0 -0 -0 -0 -0 -0 -0
;@ stdout: float -0.25: nan 0.25 -1 -0 -0 -0 -0 -0 -0
;@ stdout: float 1.0000000150474662e+30: 999999986991104 1.0000000150474662e+30 1.0000000150474662e+30 1.0000000150474662e+30 1.0000000150474662e+30 1.0000000150474662e+30 1.0000000150474662e+30 1.0000000150474662e+30 1.0000000150474662e+30
;@ stdout: float -7: nan 7 -7 -7 -7 -7 -7 -7 -7
;@ stdout: float 9: 3 9 9 9 9 9 9 9 9
;@ stdout: float 1.5, -2: minimum -2 maximum 1.5 copysign -1.5 minnum -2 maxnum 1.5
;@ stdout: float -0, 0: minimum -0 maximum 0 copysign 0
;@ stdout: float 0, -0: minimum -0 maximum 0 copysign -0
;@ stdout: float 3, nan: minimum nan maximum nan minnum 3 maxnum 3
;@ stdout: float nan, -4: minimum nan maximum nan copysign nan minnum -4 maxnum -4
;@ stdout: float fma(0.1,10,-1) 1.4901161193847656e-08 mul-add 0

@.s0 = private unnamed_addr constant [7 x i8] c"double\00"
@.s1 = private unnamed_addr constant [2 x i8] c":\00"
@.s2 = private unnamed_addr constant [2 x i8] c",\00"
@.s3 = private unnamed_addr constant [10 x i8] c": minimum\00"
@.s4 = private unnamed_addr constant [9 x i8] c" maximum\00"
@.s5 = private unnamed_addr constant [10 x i8] c" copysign\00"
@.s6 = private unnamed_addr constant [8 x i8] c" minnum\00"
@.s7 = private unnamed_addr constant [8 x i8] c" maxnum\00"
@.s8 = private unnamed_addr constant [22 x i8] c"double fma(0.1,10,-1)\00"
@.s9 = private unnamed_addr constant [9 x i8] c" mul-add\00"
@.s10 = private unnamed_addr constant [6 x i8] c"float\00"
@.s11 = private unnamed_addr constant [21 x i8] c"float fma(0.1,10,-1)\00"
@.g = private unnamed_addr constant [7 x i8] c" %.17g\00"
@.nan = private unnamed_addr constant [5 x i8] c" nan\00"
@.str = private unnamed_addr constant [3 x i8] c"%s\00"
@dv = global [23 x double] [double 0x4004000000000000, double 0xC004000000000000, double 0x3FE0000000000000, double 0xBFF8000000000000, double 0x400E000000000000, double 0x8000000000000000, double 0xBFD0000000000000, double 0x46293E5939A08CEA, double 0xC01C000000000000, double 0x4022000000000000, double 0x3FF8000000000000, double 0xC000000000000000, double 0x8000000000000000, double 0x0000000000000000, double 0x0000000000000000, double 0x8000000000000000, double 0x4008000000000000, double 0x7FF8000000000000, double 0x7FF8000000000000, double 0xC010000000000000, double 0x3FB999999999999A, double 0x4024000000000000, double 0xBFF0000000000000]
@fv = global [23 x float] [float 0x4004000000000000, float 0xC004000000000000, float 0x3FE0000000000000, float 0xBFF8000000000000, float 0x400E000000000000, float 0x8000000000000000, float 0xBFD0000000000000, float 0x46293E5940000000, float 0xC01C000000000000, float 0x4022000000000000, float 0x3FF8000000000000, float 0xC000000000000000, float 0x8000000000000000, float 0x0000000000000000, float 0x0000000000000000, float 0x8000000000000000, float 0x4008000000000000, float 0x7FF8000000000000, float 0x7FF8000000000000, float 0xC010000000000000, float 0x3FB99999A0000000, float 0x4024000000000000, float 0xBFF0000000000000]

declare i32 @printf(ptr, ...)
declare i32 @putchar(i32)
declare double @llvm.sqrt.f64(double)
declare double @llvm.fabs.f64(double)
declare double @llvm.floor.f64(double)
declare double @llvm.ceil.f64(double)
declare double @llvm.trunc.f64(double)
declare double @llvm.rint.f64(double)
declare double @llvm.nearbyint.f64(double)
declare double @llvm.round.f64(double)
declare double @llvm.roundeven.f64(double)
declare double @llvm.minimum.f64(double, double)
declare double @llvm.maximum.f64(double, double)
declare double @llvm.minnum.f64(double, double)
declare double @llvm.maxnum.f64(double, double)
declare double @llvm.copysign.f64(double, double)
declare double @llvm.fma.f64(double, double, double)
declare float @llvm.sqrt.f32(float)
declare float @llvm.fabs.f32(float)
declare float @llvm.floor.f32(float)
declare float @llvm.ceil.f32(float)
declare float @llvm.trunc.f32(float)
declare float @llvm.rint.f32(float)
declare float @llvm.nearbyint.f32(float)
declare float @llvm.round.f32(float)
declare float @llvm.roundeven.f32(float)
declare float @llvm.minimum.f32(float, float)
declare float @llvm.maximum.f32(float, float)
declare float @llvm.minnum.f32(float, float)
declare float @llvm.maxnum.f32(float, float)
declare float @llvm.copysign.f32(float, float)
declare float @llvm.fma.f32(float, float, float)

; Prints " <v>", or " nan".
define void @show(double %v) {
  %nan = fcmp uno double %v, 0.0
  br i1 %nan, label %isnan, label %number
isnan:
  call i32 (ptr, ...) @printf(ptr @.nan)
  ret void
number:
  call i32 (ptr, ...) @printf(ptr @.g, double %v)
  ret void
}

define void @text(ptr %s) {
  call i32 (ptr, ...) @printf(ptr @.str, ptr %s)
  ret void
}

define i32 @main() {
  call void @text(ptr @.s0)
  %t1 = getelementptr [23 x double], ptr @dv, i64 0, i64 0
  %t2 = load double, ptr %t1
  call void @show(double %t2)
  call void @text(ptr @.s1)
  %t3 = getelementptr [23 x double], ptr @dv, i64 0, i64 0
  %t4 = load double, ptr %t3
  %t5 = call double @llvm.sqrt.f64(double %t4)
  call void @show(double %t5)
  %t6 = getelementptr [23 x double], ptr @dv, i64 0, i64 0
  %t7 = load double, ptr %t6
  %t8 = call double @llvm.fabs.f64(double %t7)
  call void @show(double %t8)
  %t9 = getelementptr [23 x double], ptr @dv, i64 0, i64 0
  %t10 = load double, ptr %t9
  %t11 = call double @llvm.floor.f64(double %t10)
  call void @show(double %t11)
  %t12 = getelementptr [23 x double], ptr @dv, i64 0, i64 0
  %t13 = load double, ptr %t12
  %t14 = call double @llvm.ceil.f64(double %t13)
  call void @show(double %t14)
  %t15 = getelementptr [23 x double], ptr @dv, i64 0, i64 0
  %t16 = load double, ptr %t15
  %t17 = call double @llvm.trunc.f64(double %t16)
  call void @show(double %t17)
  %t18 = getelementptr [23 x double], ptr @dv, i64 0, i64 0
  %t19 = load double, ptr %t18
  %t20 = call double @llvm.rint.f64(double %t19)
  call void @show(double %t20)
  %t21 = getelementptr [23 x double], ptr @dv, i64 0, i64 0
  %t22 = load double, ptr %t21
  %t23 = call double @llvm.nearbyint.f64(double %t22)
  call void @show(double %t23)
  %t24 = getelementptr [23 x double], ptr @dv, i64 0, i64 0
  %t25 = load double, ptr %t24
  %t26 = call double @llvm.round.f64(double %t25)
  call void @show(double %t26)
  %t27 = getelementptr [23 x double], ptr @dv, i64 0, i64 0
  %t28 = load double, ptr %t27
  %t29 = call double @llvm.roundeven.f64(double %t28)
  call void @show(double %t29)
  call i32 @putchar(i32 10)
  call void @text(ptr @.s0)
  %t30 = getelementptr [23 x double], ptr @dv, i64 0, i64 1
  %t31 = load double, ptr %t30
  call void @show(double %t31)
  call void @text(ptr @.s1)
  %t32 = getelementptr [23 x double], ptr @dv, i64 0, i64 1
  %t33 = load double, ptr %t32
  %t34 = call double @llvm.sqrt.f64(double %t33)
  call void @show(double %t34)
  %t35 = getelementptr [23 x double], ptr @dv, i64 0, i64 1
  %t36 = load double, ptr %t35
  %t37 = call double @llvm.fabs.f64(double %t36)
  call void @show(double %t37)
  %t38 = getelementptr [23 x double], ptr @dv, i64 0, i64 1
  %t39 = load double, ptr %t38
  %t40 = call double @llvm.floor.f64(double %t39)
  call void @show(double %t40)
  %t41 = getelementptr [23 x double], ptr @dv, i64 0, i64 1
  %t42 = load double, ptr %t41
  %t43 = call double @llvm.ceil.f64(double %t42)
  call void @show(double %t43)
  %t44 = getelementptr [23 x double], ptr @dv, i64 0, i64 1
  %t45 = load double, ptr %t44
  %t46 = call double @llvm.trunc.f64(double %t45)
  call void @show(double %t46)
  %t47 = getelementptr [23 x double], ptr @dv, i64 0, i64 1
  %t48 = load double, ptr %t47
  %t49 = call double @llvm.rint.f64(double %t48)
  call void @show(double %t49)
  %t50 = getelementptr [23 x double], ptr @dv, i64 0, i64 1
  %t51 = load double, ptr %t50
  %t52 = call double @llvm.nearbyint.f64(double %t51)
  call void @show(double %t52)
  %t53 = getelementptr [23 x double], ptr @dv, i64 0, i64 1
  %t54 = load double, ptr %t53
  %t55 = call double @llvm.round.f64(double %t54)
  call void @show(double %t55)
  %t56 = getelementptr [23 x double], ptr @dv, i64 0, i64 1
  %t57 = load double, ptr %t56
  %t58 = call double @llvm.roundeven.f64(double %t57)
  call void @show(double %t58)
  call i32 @putchar(i32 10)
  call void @text(ptr @.s0)
  %t59 = getelementptr [23 x double], ptr @dv, i64 0, i64 2
  %t60 = load double, ptr %t59
  call void @show(double %t60)
  call void @text(ptr @.s1)
  %t61 = getelementptr [23 x double], ptr @dv, i64 0, i64 2
  %t62 = load double, ptr %t61
  %t63 = call double @llvm.sqrt.f64(double %t62)
  call void @show(double %t63)
  %t64 = getelementptr [23 x double], ptr @dv, i64 0, i64 2
  %t65 = load double, ptr %t64
  %t66 = call double @llvm.fabs.f64(double %t65)
  call void @show(double %t66)
  %t67 = getelementptr [23 x double], ptr @dv, i64 0, i64 2
  %t68 = load double, ptr %t67
  %t69 = call double @llvm.floor.f64(double %t68)
  call void @show(double %t69)
  %t70 = getelementptr [23 x double], ptr @dv, i64 0, i64 2
  %t71 = load double, ptr %t70
  %t72 = call double @llvm.ceil.f64(double %t71)
  call void @show(double %t72)
  %t73 = getelementptr [23 x double], ptr @dv, i64 0, i64 2
  %t74 = load double, ptr %t73
  %t75 = call double @llvm.trunc.f64(double %t74)
  call void @show(double %t75)
  %t76 = getelementptr [23 x double], ptr @dv, i64 0, i64 2
  %t77 = load double, ptr %t76
  %t78 = call double @llvm.rint.f64(double %t77)
  call void @show(double %t78)
  %t79 = getelementptr [23 x double], ptr @dv, i64 0, i64 2
  %t80 = load double, ptr %t79
  %t81 = call double @llvm.nearbyint.f64(double %t80)
  call void @show(double %t81)
  %t82 = getelementptr [23 x double], ptr @dv, i64 0, i64 2
  %t83 = load double, ptr %t82
  %t84 = call double @llvm.round.f64(double %t83)
  call void @show(double %t84)
  %t85 = getelementptr [23 x double], ptr @dv, i64 0, i64 2
  %t86 = load double, ptr %t85
  %t87 = call double @llvm.roundeven.f64(double %t86)
  call void @show(double %t87)
  call i32 @putchar(i32 10)
  call void @text(ptr @.s0)
  %t88 = getelementptr [23 x double], ptr @dv, i64 0, i64 3
  %t89 = load double, ptr %t88
  call void @show(double %t89)
  call void @text(ptr @.s1)
  %t90 = getelementptr [23 x double], ptr @dv, i64 0, i64 3
  %t91 = load double, ptr %t90
  %t92 = call double @llvm.sqrt.f64(double %t91)
  call void @show(double %t92)
  %t93 = getelementptr [23 x double], ptr @dv, i64 0, i64 3
  %t94 = load double, ptr %t93
  %t95 = call double @llvm.fabs.f64(double %t94)
  call void @show(double %t95)
  %t96 = getelementptr [23 x double], ptr @dv, i64 0, i64 3
  %t97 = load double, ptr %t96
  %t98 = call double @llvm.floor.f64(double %t97)
  call void @show(double %t98)
  %t99 = getelementptr [23 x double], ptr @dv, i64 0, i64 3
  %t100 = load double, ptr %t99
  %t101 = call double @llvm.ceil.f64(double %t100)
  call void @show(double %t101)
  %t102 = getelementptr [23 x double], ptr @dv, i64 0, i64 3
  %t103 = load double, ptr %t102
  %t104 = call double @llvm.trunc.f64(double %t103)
  call void @show(double %t104)
  %t105 = getelementptr [23 x double], ptr @dv, i64 0, i64 3
  %t106 = load double, ptr %t105
  %t107 = call double @llvm.rint.f64(double %t106)
  call void @show(double %t107)
  %t108 = getelementptr [23 x double], ptr @dv, i64 0, i64 3
  %t109 = load double, ptr %t108
  %t110 = call double @llvm.nearbyint.f64(double %t109)
  call void @show(double %t110)
  %t111 = getelementptr [23 x double], ptr @dv, i64 0, i64 3
  %t112 = load double, ptr %t111
  %t113 = call double @llvm.round.f64(double %t112)
  call void @show(double %t113)
  %t114 = getelementptr [23 x double], ptr @dv, i64 0, i64 3
  %t115 = load double, ptr %t114
  %t116 = call double @llvm.roundeven.f64(double %t115)
  call void @show(double %t116)
  call i32 @putchar(i32 10)
  call void @text(ptr @.s0)
  %t117 = getelementptr [23 x double], ptr @dv, i64 0, i64 4
  %t118 = load double, ptr %t117
  call void @show(double %t118)
  call void @text(ptr @.s1)
  %t119 = getelementptr [23 x double], ptr @dv, i64 0, i64 4
  %t120 = load double, ptr %t119
  %t121 = call double @llvm.sqrt.f64(double %t120)
  call void @show(double %t121)
  %t122 = getelementptr [23 x double], ptr @dv, i64 0, i64 4
  %t123 = load double, ptr %t122
  %t124 = call double @llvm.fabs.f64(double %t123)
  call void @show(double %t124)
  %t125 = getelementptr [23 x double], ptr @dv, i64 0, i64 4
  %t126 = load double, ptr %t125
  %t127 = call double @llvm.floor.f64(double %t126)
  call void @show(double %t127)
  %t128 = getelementptr [23 x double], ptr @dv, i64 0, i64 4
  %t129 = load double, ptr %t128
  %t130 = call double @llvm.ceil.f64(double %t129)
  call void @show(double %t130)
  %t131 = getelementptr [23 x double], ptr @dv, i64 0, i64 4
  %t132 = load double, ptr %t131
  %t133 = call double @llvm.trunc.f64(double %t132)
  call void @show(double %t133)
  %t134 = getelementptr [23 x double], ptr @dv, i64 0, i64 4
  %t135 = load double, ptr %t134
  %t136 = call double @llvm.rint.f64(double %t135)
  call void @show(double %t136)
  %t137 = getelementptr [23 x double], ptr @dv, i64 0, i64 4
  %t138 = load double, ptr %t137
  %t139 = call double @llvm.nearbyint.f64(double %t138)
  call void @show(double %t139)
  %t140 = getelementptr [23 x double], ptr @dv, i64 0, i64 4
  %t141 = load double, ptr %t140
  %t142 = call double @llvm.round.f64(double %t141)
  call void @show(double %t142)
  %t143 = getelementptr [23 x double], ptr @dv, i64 0, i64 4
  %t144 = load double, ptr %t143
  %t145 = call double @llvm.roundeven.f64(double %t144)
  call void @show(double %t145)
  call i32 @putchar(i32 10)
  call void @text(ptr @.s0)
  %t146 = getelementptr [23 x double], ptr @dv, i64 0, i64 5
  %t147 = load double, ptr %t146
  call void @show(double %t147)
  call void @text(ptr @.s1)
  %t148 = getelementptr [23 x double], ptr @dv, i64 0, i64 5
  %t149 = load double, ptr %t148
  %t150 = call double @llvm.sqrt.f64(double %t149)
  call void @show(double %t150)
  %t151 = getelementptr [23 x double], ptr @dv, i64 0, i64 5
  %t152 = load double, ptr %t151
  %t153 = call double @llvm.fabs.f64(double %t152)
  call void @show(double %t153)
  %t154 = getelementptr [23 x double], ptr @dv, i64 0, i64 5
  %t155 = load double, ptr %t154
  %t156 = call double @llvm.floor.f64(double %t155)
  call void @show(double %t156)
  %t157 = getelementptr [23 x double], ptr @dv, i64 0, i64 5
  %t158 = load double, ptr %t157
  %t159 = call double @llvm.ceil.f64(double %t158)
  call void @show(double %t159)
  %t160 = getelementptr [23 x double], ptr @dv, i64 0, i64 5
  %t161 = load double, ptr %t160
  %t162 = call double @llvm.trunc.f64(double %t161)
  call void @show(double %t162)
  %t163 = getelementptr [23 x double], ptr @dv, i64 0, i64 5
  %t164 = load double, ptr %t163
  %t165 = call double @llvm.rint.f64(double %t164)
  call void @show(double %t165)
  %t166 = getelementptr [23 x double], ptr @dv, i64 0, i64 5
  %t167 = load double, ptr %t166
  %t168 = call double @llvm.nearbyint.f64(double %t167)
  call void @show(double %t168)
  %t169 = getelementptr [23 x double], ptr @dv, i64 0, i64 5
  %t170 = load double, ptr %t169
  %t171 = call double @llvm.round.f64(double %t170)
  call void @show(double %t171)
  %t172 = getelementptr [23 x double], ptr @dv, i64 0, i64 5
  %t173 = load double, ptr %t172
  %t174 = call double @llvm.roundeven.f64(double %t173)
  call void @show(double %t174)
  call i32 @putchar(i32 10)
  call void @text(ptr @.s0)
  %t175 = getelementptr [23 x double], ptr @dv, i64 0, i64 6
  %t176 = load double, ptr %t175
  call void @show(double %t176)
  call void @text(ptr @.s1)
  %t177 = getelementptr [23 x double], ptr @dv, i64 0, i64 6
  %t178 = load double, ptr %t177
  %t179 = call double @llvm.sqrt.f64(double %t178)
  call void @show(double %t179)
  %t180 = getelementptr [23 x double], ptr @dv, i64 0, i64 6
  %t181 = load double, ptr %t180
  %t182 = call double @llvm.fabs.f64(double %t181)
  call void @show(double %t182)
  %t183 = getelementptr [23 x double], ptr @dv, i64 0, i64 6
  %t184 = load double, ptr %t183
  %t185 = call double @llvm.floor.f64(double %t184)
  call void @show(double %t185)
  %t186 = getelementptr [23 x double], ptr @dv, i64 0, i64 6
  %t187 = load double, ptr %t186
  %t188 = call double @llvm.ceil.f64(double %t187)
  call void @show(double %t188)
  %t189 = getelementptr [23 x double], ptr @dv, i64 0, i64 6
  %t190 = load double, ptr %t189
  %t191 = call double @llvm.trunc.f64(double %t190)
  call void @show(double %t191)
  %t192 = getelementptr [23 x double], ptr @dv, i64 0, i64 6
  %t193 = load double, ptr %t192
  %t194 = call double @llvm.rint.f64(double %t193)
  call void @show(double %t194)
  %t195 = getelementptr [23 x double], ptr @dv, i64 0, i64 6
  %t196 = load double, ptr %t195
  %t197 = call double @llvm.nearbyint.f64(double %t196)
  call void @show(double %t197)
  %t198 = getelementptr [23 x double], ptr @dv, i64 0, i64 6
  %t199 = load double, ptr %t198
  %t200 = call double @llvm.round.f64(double %t199)
  call void @show(double %t200)
  %t201 = getelementptr [23 x double], ptr @dv, i64 0, i64 6
  %t202 = load double, ptr %t201
  %t203 = call double @llvm.roundeven.f64(double %t202)
  call void @show(double %t203)
  call i32 @putchar(i32 10)
  call void @text(ptr @.s0)
  %t204 = getelementptr [23 x double], ptr @dv, i64 0, i64 7
  %t205 = load double, ptr %t204
  call void @show(double %t205)
  call void @text(ptr @.s1)
  %t206 = getelementptr [23 x double], ptr @dv, i64 0, i64 7
  %t207 = load double, ptr %t206
  %t208 = call double @llvm.sqrt.f64(double %t207)
  call void @show(double %t208)
  %t209 = getelementptr [23 x double], ptr @dv, i64 0, i64 7
  %t210 = load double, ptr %t209
  %t211 = call double @llvm.fabs.f64(double %t210)
  call void @show(double %t211)
  %t212 = getelementptr [23 x double], ptr @dv, i64 0, i64 7
  %t213 = load double, ptr %t212
  %t214 = call double @llvm.floor.f64(double %t213)
  call void @show(double %t214)
  %t215 = getelementptr [23 x double], ptr @dv, i64 0, i64 7
  %t216 = load double, ptr %t215
  %t217 = call double @llvm.ceil.f64(double %t216)
  call void @show(double %t217)
  %t218 = getelementptr [23 x double], ptr @dv, i64 0, i64 7
  %t219 = load double, ptr %t218
  %t220 = call double @llvm.trunc.f64(double %t219)
  call void @show(double %t220)
  %t221 = getelementptr [23 x double], ptr @dv, i64 0, i64 7
  %t222 = load double, ptr %t221
  %t223 = call double @llvm.rint.f64(double %t222)
  call void @show(double %t223)
  %t224 = getelementptr [23 x double], ptr @dv, i64 0, i64 7
  %t225 = load double, ptr %t224
  %t226 = call double @llvm.nearbyint.f64(double %t225)
  call void @show(double %t226)
  %t227 = getelementptr [23 x double], ptr @dv, i64 0, i64 7
  %t228 = load double, ptr %t227
  %t229 = call double @llvm.round.f64(double %t228)
  call void @show(double %t229)
  %t230 = getelementptr [23 x double], ptr @dv, i64 0, i64 7
  %t231 = load double, ptr %t230
  %t232 = call double @llvm.roundeven.f64(double %t231)
  call void @show(double %t232)
  call i32 @putchar(i32 10)
  call void @text(ptr @.s0)
  %t233 = getelementptr [23 x double], ptr @dv, i64 0, i64 8
  %t234 = load double, ptr %t233
  call void @show(double %t234)
  call void @text(ptr @.s1)
  %t235 = getelementptr [23 x double], ptr @dv, i64 0, i64 8
  %t236 = load double, ptr %t235
  %t237 = call double @llvm.sqrt.f64(double %t236)
  call void @show(double %t237)
  %t238 = getelementptr [23 x double], ptr @dv, i64 0, i64 8
  %t239 = load double, ptr %t238
  %t240 = call double @llvm.fabs.f64(double %t239)
  call void @show(double %t240)
  %t241 = getelementptr [23 x double], ptr @dv, i64 0, i64 8
  %t242 = load double, ptr %t241
  %t243 = call double @llvm.floor.f64(double %t242)
  call void @show(double %t243)
  %t244 = getelementptr [23 x double], ptr @dv, i64 0, i64 8
  %t245 = load double, ptr %t244
  %t246 = call double @llvm.ceil.f64(double %t245)
  call void @show(double %t246)
  %t247 = getelementptr [23 x double], ptr @dv, i64 0, i64 8
  %t248 = load double, ptr %t247
  %t249 = call double @llvm.trunc.f64(double %t248)
  call void @show(double %t249)
  %t250 = getelementptr [23 x double], ptr @dv, i64 0, i64 8
  %t251 = load double, ptr %t250
  %t252 = call double @llvm.rint.f64(double %t251)
  call void @show(double %t252)
  %t253 = getelementptr [23 x double], ptr @dv, i64 0, i64 8
  %t254 = load double, ptr %t253
  %t255 = call double @llvm.nearbyint.f64(double %t254)
  call void @show(double %t255)
  %t256 = getelementptr [23 x double], ptr @dv, i64 0, i64 8
  %t257 = load double, ptr %t256
  %t258 = call double @llvm.round.f64(double %t257)
  call void @show(double %t258)
  %t259 = getelementptr [23 x double], ptr @dv, i64 0, i64 8
  %t260 = load double, ptr %t259
  %t261 = call double @llvm.roundeven.f64(double %t260)
  call void @show(double %t261)
  call i32 @putchar(i32 10)
  call void @text(ptr @.s0)
  %t262 = getelementptr [23 x double], ptr @dv, i64 0, i64 9
  %t263 = load double, ptr %t262
  call void @show(double %t263)
  call void @text(ptr @.s1)
  %t264 = getelementptr [23 x double], ptr @dv, i64 0, i64 9
  %t265 = load double, ptr %t264
  %t266 = call double @llvm.sqrt.f64(double %t265)
  call void @show(double %t266)
  %t267 = getelementptr [23 x double], ptr @dv, i64 0, i64 9
  %t268 = load double, ptr %t267
  %t269 = call double @llvm.fabs.f64(double %t268)
  call void @show(double %t269)
  %t270 = getelementptr [23 x double], ptr @dv, i64 0, i64 9
  %t271 = load double, ptr %t270
  %t272 = call double @llvm.floor.f64(double %t271)
  call void @show(double %t272)
  %t273 = getelementptr [23 x double], ptr @dv, i64 0, i64 9
  %t274 = load double, ptr %t273
  %t275 = call double @llvm.ceil.f64(double %t274)
  call void @show(double %t275)
  %t276 = getelementptr [23 x double], ptr @dv, i64 0, i64 9
  %t277 = load double, ptr %t276
  %t278 = call double @llvm.trunc.f64(double %t277)
  call void @show(double %t278)
  %t279 = getelementptr [23 x double], ptr @dv, i64 0, i64 9
  %t280 = load double, ptr %t279
  %t281 = call double @llvm.rint.f64(double %t280)
  call void @show(double %t281)
  %t282 = getelementptr [23 x double], ptr @dv, i64 0, i64 9
  %t283 = load double, ptr %t282
  %t284 = call double @llvm.nearbyint.f64(double %t283)
  call void @show(double %t284)
  %t285 = getelementptr [23 x double], ptr @dv, i64 0, i64 9
  %t286 = load double, ptr %t285
  %t287 = call double @llvm.round.f64(double %t286)
  call void @show(double %t287)
  %t288 = getelementptr [23 x double], ptr @dv, i64 0, i64 9
  %t289 = load double, ptr %t288
  %t290 = call double @llvm.roundeven.f64(double %t289)
  call void @show(double %t290)
  call i32 @putchar(i32 10)
  call void @text(ptr @.s0)
  %t291 = getelementptr [23 x double], ptr @dv, i64 0, i64 10
  %t292 = load double, ptr %t291
  call void @show(double %t292)
  call void @text(ptr @.s2)
  %t293 = getelementptr [23 x double], ptr @dv, i64 0, i64 11
  %t294 = load double, ptr %t293
  call void @show(double %t294)
  call void @text(ptr @.s3)
  %t295 = getelementptr [23 x double], ptr @dv, i64 0, i64 10
  %t296 = load double, ptr %t295
  %t297 = getelementptr [23 x double], ptr @dv, i64 0, i64 11
  %t298 = load double, ptr %t297
  %t299 = call double @llvm.minimum.f64(double %t296, double %t298)
  call void @show(double %t299)
  call void @text(ptr @.s4)
  %t300 = getelementptr [23 x double], ptr @dv, i64 0, i64 10
  %t301 = load double, ptr %t300
  %t302 = getelementptr [23 x double], ptr @dv, i64 0, i64 11
  %t303 = load double, ptr %t302
  %t304 = call double @llvm.maximum.f64(double %t301, double %t303)
  call void @show(double %t304)
  call void @text(ptr @.s5)
  %t305 = getelementptr [23 x double], ptr @dv, i64 0, i64 10
  %t306 = load double, ptr %t305
  %t307 = getelementptr [23 x double], ptr @dv, i64 0, i64 11
  %t308 = load double, ptr %t307
  %t309 = call double @llvm.copysign.f64(double %t306, double %t308)
  call void @show(double %t309)
  call void @text(ptr @.s6)
  %t310 = getelementptr [23 x double], ptr @dv, i64 0, i64 10
  %t311 = load double, ptr %t310
  %t312 = getelementptr [23 x double], ptr @dv, i64 0, i64 11
  %t313 = load double, ptr %t312
  %t314 = call double @llvm.minnum.f64(double %t311, double %t313)
  call void @show(double %t314)
  call void @text(ptr @.s7)
  %t315 = getelementptr [23 x double], ptr @dv, i64 0, i64 10
  %t316 = load double, ptr %t315
  %t317 = getelementptr [23 x double], ptr @dv, i64 0, i64 11
  %t318 = load double, ptr %t317
  %t319 = call double @llvm.maxnum.f64(double %t316, double %t318)
  call void @show(double %t319)
  call i32 @putchar(i32 10)
  call void @text(ptr @.s0)
  %t320 = getelementptr [23 x double], ptr @dv, i64 0, i64 12
  %t321 = load double, ptr %t320
  call void @show(double %t321)
  call void @text(ptr @.s2)
  %t322 = getelementptr [23 x double], ptr @dv, i64 0, i64 13
  %t323 = load double, ptr %t322
  call void @show(double %t323)
  call void @text(ptr @.s3)
  %t324 = getelementptr [23 x double], ptr @dv, i64 0, i64 12
  %t325 = load double, ptr %t324
  %t326 = getelementptr [23 x double], ptr @dv, i64 0, i64 13
  %t327 = load double, ptr %t326
  %t328 = call double @llvm.minimum.f64(double %t325, double %t327)
  call void @show(double %t328)
  call void @text(ptr @.s4)
  %t329 = getelementptr [23 x double], ptr @dv, i64 0, i64 12
  %t330 = load double, ptr %t329
  %t331 = getelementptr [23 x double], ptr @dv, i64 0, i64 13
  %t332 = load double, ptr %t331
  %t333 = call double @llvm.maximum.f64(double %t330, double %t332)
  call void @show(double %t333)
  call void @text(ptr @.s5)
  %t334 = getelementptr [23 x double], ptr @dv, i64 0, i64 12
  %t335 = load double, ptr %t334
  %t336 = getelementptr [23 x double], ptr @dv, i64 0, i64 13
  %t337 = load double, ptr %t336
  %t338 = call double @llvm.copysign.f64(double %t335, double %t337)
  call void @show(double %t338)
  call i32 @putchar(i32 10)
  call void @text(ptr @.s0)
  %t339 = getelementptr [23 x double], ptr @dv, i64 0, i64 14
  %t340 = load double, ptr %t339
  call void @show(double %t340)
  call void @text(ptr @.s2)
  %t341 = getelementptr [23 x double], ptr @dv, i64 0, i64 15
  %t342 = load double, ptr %t341
  call void @show(double %t342)
  call void @text(ptr @.s3)
  %t343 = getelementptr [23 x double], ptr @dv, i64 0, i64 14
  %t344 = load double, ptr %t343
  %t345 = getelementptr [23 x double], ptr @dv, i64 0, i64 15
  %t346 = load double, ptr %t345
  %t347 = call double @llvm.minimum.f64(double %t344, double %t346)
  call void @show(double %t347)
  call void @text(ptr @.s4)
  %t348 = getelementptr [23 x double], ptr @dv, i64 0, i64 14
  %t349 = load double, ptr %t348
  %t350 = getelementptr [23 x double], ptr @dv, i64 0, i64 15
  %t351 = load double, ptr %t350
  %t352 = call double @llvm.maximum.f64(double %t349, double %t351)
  call void @show(double %t352)
  call void @text(ptr @.s5)
  %t353 = getelementptr [23 x double], ptr @dv, i64 0, i64 14
  %t354 = load double, ptr %t353
  %t355 = getelementptr [23 x double], ptr @dv, i64 0, i64 15
  %t356 = load double, ptr %t355
  %t357 = call double @llvm.copysign.f64(double %t354, double %t356)
  call void @show(double %t357)
  call i32 @putchar(i32 10)
  call void @text(ptr @.s0)
  %t358 = getelementptr [23 x double], ptr @dv, i64 0, i64 16
  %t359 = load double, ptr %t358
  call void @show(double %t359)
  call void @text(ptr @.s2)
  %t360 = getelementptr [23 x double], ptr @dv, i64 0, i64 17
  %t361 = load double, ptr %t360
  call void @show(double %t361)
  call void @text(ptr @.s3)
  %t362 = getelementptr [23 x double], ptr @dv, i64 0, i64 16
  %t363 = load double, ptr %t362
  %t364 = getelementptr [23 x double], ptr @dv, i64 0, i64 17
  %t365 = load double, ptr %t364
  %t366 = call double @llvm.minimum.f64(double %t363, double %t365)
  call void @show(double %t366)
  call void @text(ptr @.s4)
  %t367 = getelementptr [23 x double], ptr @dv, i64 0, i64 16
  %t368 = load double, ptr %t367
  %t369 = getelementptr [23 x double], ptr @dv, i64 0, i64 17
  %t370 = load double, ptr %t369
  %t371 = call double @llvm.maximum.f64(double %t368, double %t370)
  call void @show(double %t371)
  call void @text(ptr @.s6)
  %t372 = getelementptr [23 x double], ptr @dv, i64 0, i64 16
  %t373 = load double, ptr %t372
  %t374 = getelementptr [23 x double], ptr @dv, i64 0, i64 17
  %t375 = load double, ptr %t374
  %t376 = call double @llvm.minnum.f64(double %t373, double %t375)
  call void @show(double %t376)
  call void @text(ptr @.s7)
  %t377 = getelementptr [23 x double], ptr @dv, i64 0, i64 16
  %t378 = load double, ptr %t377
  %t379 = getelementptr [23 x double], ptr @dv, i64 0, i64 17
  %t380 = load double, ptr %t379
  %t381 = call double @llvm.maxnum.f64(double %t378, double %t380)
  call void @show(double %t381)
  call i32 @putchar(i32 10)
  call void @text(ptr @.s0)
  %t382 = getelementptr [23 x double], ptr @dv, i64 0, i64 18
  %t383 = load double, ptr %t382
  call void @show(double %t383)
  call void @text(ptr @.s2)
  %t384 = getelementptr [23 x double], ptr @dv, i64 0, i64 19
  %t385 = load double, ptr %t384
  call void @show(double %t385)
  call void @text(ptr @.s3)
  %t386 = getelementptr [23 x double], ptr @dv, i64 0, i64 18
  %t387 = load double, ptr %t386
  %t388 = getelementptr [23 x double], ptr @dv, i64 0, i64 19
  %t389 = load double, ptr %t388
  %t390 = call double @llvm.minimum.f64(double %t387, double %t389)
  call void @show(double %t390)
  call void @text(ptr @.s4)
  %t391 = getelementptr [23 x double], ptr @dv, i64 0, i64 18
  %t392 = load double, ptr %t391
  %t393 = getelementptr [23 x double], ptr @dv, i64 0, i64 19
  %t394 = load double, ptr %t393
  %t395 = call double @llvm.maximum.f64(double %t392, double %t394)
  call void @show(double %t395)
  call void @text(ptr @.s5)
  %t396 = getelementptr [23 x double], ptr @dv, i64 0, i64 18
  %t397 = load double, ptr %t396
  %t398 = getelementptr [23 x double], ptr @dv, i64 0, i64 19
  %t399 = load double, ptr %t398
  %t400 = call double @llvm.copysign.f64(double %t397, double %t399)
  call void @show(double %t400)
  call void @text(ptr @.s6)
  %t401 = getelementptr [23 x double], ptr @dv, i64 0, i64 18
  %t402 = load double, ptr %t401
  %t403 = getelementptr [23 x double], ptr @dv, i64 0, i64 19
  %t404 = load double, ptr %t403
  %t405 = call double @llvm.minnum.f64(double %t402, double %t404)
  call void @show(double %t405)
  call void @text(ptr @.s7)
  %t406 = getelementptr [23 x double], ptr @dv, i64 0, i64 18
  %t407 = load double, ptr %t406
  %t408 = getelementptr [23 x double], ptr @dv, i64 0, i64 19
  %t409 = load double, ptr %t408
  %t410 = call double @llvm.maxnum.f64(double %t407, double %t409)
  call void @show(double %t410)
  call i32 @putchar(i32 10)
  call void @text(ptr @.s8)
  %t411 = getelementptr [23 x double], ptr @dv, i64 0, i64 20
  %t412 = load double, ptr %t411
  %t413 = getelementptr [23 x double], ptr @dv, i64 0, i64 21
  %t414 = load double, ptr %t413
  %t415 = getelementptr [23 x double], ptr @dv, i64 0, i64 22
  %t416 = load double, ptr %t415
  %t417 = call double @llvm.fma.f64(double %t412, double %t414, double %t416)
  call void @show(double %t417)
  call void @text(ptr @.s9)
  %t418 = getelementptr [23 x double], ptr @dv, i64 0, i64 20
  %t419 = load double, ptr %t418
  %t420 = getelementptr [23 x double], ptr @dv, i64 0, i64 21
  %t421 = load double, ptr %t420
  %t422 = getelementptr [23 x double], ptr @dv, i64 0, i64 22
  %t423 = load double, ptr %t422
  %t424 = fmul double %t419, %t421
  %t425 = fadd double %t424, %t423
  call void @show(double %t425)
  call i32 @putchar(i32 10)
  call void @text(ptr @.s10)
  %t426 = getelementptr [23 x float], ptr @fv, i64 0, i64 0
  %t427 = load float, ptr %t426
  %t428 = fpext float %t427 to double
  call void @show(double %t428)
  call void @text(ptr @.s1)
  %t429 = getelementptr [23 x float], ptr @fv, i64 0, i64 0
  %t430 = load float, ptr %t429
  %t431 = call float @llvm.sqrt.f32(float %t430)
  %t432 = fpext float %t431 to double
  call void @show(double %t432)
  %t433 = getelementptr [23 x float], ptr @fv, i64 0, i64 0
  %t434 = load float, ptr %t433
  %t435 = call float @llvm.fabs.f32(float %t434)
  %t436 = fpext float %t435 to double
  call void @show(double %t436)
  %t437 = getelementptr [23 x float], ptr @fv, i64 0, i64 0
  %t438 = load float, ptr %t437
  %t439 = call float @llvm.floor.f32(float %t438)
  %t440 = fpext float %t439 to double
  call void @show(double %t440)
  %t441 = getelementptr [23 x float], ptr @fv, i64 0, i64 0
  %t442 = load float, ptr %t441
  %t443 = call float @llvm.ceil.f32(float %t442)
  %t444 = fpext float %t443 to double
  call void @show(double %t444)
  %t445 = getelementptr [23 x float], ptr @fv, i64 0, i64 0
  %t446 = load float, ptr %t445
  %t447 = call float @llvm.trunc.f32(float %t446)
  %t448 = fpext float %t447 to double
  call void @show(double %t448)
  %t449 = getelementptr [23 x float], ptr @fv, i64 0, i64 0
  %t450 = load float, ptr %t449
  %t451 = call float @llvm.rint.f32(float %t450)
  %t452 = fpext float %t451 to double
  call void @show(double %t452)
  %t453 = getelementptr [23 x float], ptr @fv, i64 0, i64 0
  %t454 = load float, ptr %t453
  %t455 = call float @llvm.nearbyint.f32(float %t454)
  %t456 = fpext float %t455 to double
  call void @show(double %t456)
  %t457 = getelementptr [23 x float], ptr @fv, i64 0, i64 0
  %t458 = load float, ptr %t457
  %t459 = call float @llvm.round.f32(float %t458)
  %t460 = fpext float %t459 to double
  call void @show(double %t460)
  %t461 = getelementptr [23 x float], ptr @fv, i64 0, i64 0
  %t462 = load float, ptr %t461
  %t463 = call float @llvm.roundeven.f32(float %t462)
  %t464 = fpext float %t463 to double
  call void @show(double %t464)
  call i32 @putchar(i32 10)
  call void @text(ptr @.s10)
  %t465 = getelementptr [23 x float], ptr @fv, i64 0, i64 1
  %t466 = load float, ptr %t465
  %t467 = fpext float %t466 to double
  call void @show(double %t467)
  call void @text(ptr @.s1)
  %t468 = getelementptr [23 x float], ptr @fv, i64 0, i64 1
  %t469 = load float, ptr %t468
  %t470 = call float @llvm.sqrt.f32(float %t469)
  %t471 = fpext float %t470 to double
  call void @show(double %t471)
  %t472 = getelementptr [23 x float], ptr @fv, i64 0, i64 1
  %t473 = load float, ptr %t472
  %t474 = call float @llvm.fabs.f32(float %t473)
  %t475 = fpext float %t474 to double
  call void @show(double %t475)
  %t476 = getelementptr [23 x float], ptr @fv, i64 0, i64 1
  %t477 = load float, ptr %t476
  %t478 = call float @llvm.floor.f32(float %t477)
  %t479 = fpext float %t478 to double
  call void @show(double %t479)
  %t480 = getelementptr [23 x float], ptr @fv, i64 0, i64 1
  %t481 = load float, ptr %t480
  %t482 = call float @llvm.ceil.f32(float %t481)
  %t483 = fpext float %t482 to double
  call void @show(double %t483)
  %t484 = getelementptr [23 x float], ptr @fv, i64 0, i64 1
  %t485 = load float, ptr %t484
  %t486 = call float @llvm.trunc.f32(float %t485)
  %t487 = fpext float %t486 to double
  call void @show(double %t487)
  %t488 = getelementptr [23 x float], ptr @fv, i64 0, i64 1
  %t489 = load float, ptr %t488
  %t490 = call float @llvm.rint.f32(float %t489)
  %t491 = fpext float %t490 to double
  call void @show(double %t491)
  %t492 = getelementptr [23 x float], ptr @fv, i64 0, i64 1
  %t493 = load float, ptr %t492
  %t494 = call float @llvm.nearbyint.f32(float %t493)
  %t495 = fpext float %t494 to double
  call void @show(double %t495)
  %t496 = getelementptr [23 x float], ptr @fv, i64 0, i64 1
  %t497 = load float, ptr %t496
  %t498 = call float @llvm.round.f32(float %t497)
  %t499 = fpext float %t498 to double
  call void @show(double %t499)
  %t500 = getelementptr [23 x float], ptr @fv, i64 0, i64 1
  %t501 = load float, ptr %t500
  %t502 = call float @llvm.roundeven.f32(float %t501)
  %t503 = fpext float %t502 to double
  call void @show(double %t503)
  call i32 @putchar(i32 10)
  call void @text(ptr @.s10)
  %t504 = getelementptr [23 x float], ptr @fv, i64 0, i64 2
  %t505 = load float, ptr %t504
  %t506 = fpext float %t505 to double
  call void @show(double %t506)
  call void @text(ptr @.s1)
  %t507 = getelementptr [23 x float], ptr @fv, i64 0, i64 2
  %t508 = load float, ptr %t507
  %t509 = call float @llvm.sqrt.f32(float %t508)
  %t510 = fpext float %t509 to double
  call void @show(double %t510)
  %t511 = getelementptr [23 x float], ptr @fv, i64 0, i64 2
  %t512 = load float, ptr %t511
  %t513 = call float @llvm.fabs.f32(float %t512)
  %t514 = fpext float %t513 to double
  call void @show(double %t514)
  %t515 = getelementptr [23 x float], ptr @fv, i64 0, i64 2
  %t516 = load float, ptr %t515
  %t517 = call float @llvm.floor.f32(float %t516)
  %t518 = fpext float %t517 to double
  call void @show(double %t518)
  %t519 = getelementptr [23 x float], ptr @fv, i64 0, i64 2
  %t520 = load float, ptr %t519
  %t521 = call float @llvm.ceil.f32(float %t520)
  %t522 = fpext float %t521 to double
  call void @show(double %t522)
  %t523 = getelementptr [23 x float], ptr @fv, i64 0, i64 2
  %t524 = load float, ptr %t523
  %t525 = call float @llvm.trunc.f32(float %t524)
  %t526 = fpext float %t525 to double
  call void @show(double %t526)
  %t527 = getelementptr [23 x float], ptr @fv, i64 0, i64 2
  %t528 = load float, ptr %t527
  %t529 = call float @llvm.rint.f32(float %t528)
  %t530 = fpext float %t529 to double
  call void @show(double %t530)
  %t531 = getelementptr [23 x float], ptr @fv, i64 0, i64 2
  %t532 = load float, ptr %t531
  %t533 = call float @llvm.nearbyint.f32(float %t532)
  %t534 = fpext float %t533 to double
  call void @show(double %t534)
  %t535 = getelementptr [23 x float], ptr @fv, i64 0, i64 2
  %t536 = load float, ptr %t535
  %t537 = call float @llvm.round.f32(float %t536)
  %t538 = fpext float %t537 to double
  call void @show(double %t538)
  %t539 = getelementptr [23 x float], ptr @fv, i64 0, i64 2
  %t540 = load float, ptr %t539
  %t541 = call float @llvm.roundeven.f32(float %t540)
  %t542 = fpext float %t541 to double
  call void @show(double %t542)
  call i32 @putchar(i32 10)
  call void @text(ptr @.s10)
  %t543 = getelementptr [23 x float], ptr @fv, i64 0, i64 3
  %t544 = load float, ptr %t543
  %t545 = fpext float %t544 to double
  call void @show(double %t545)
  call void @text(ptr @.s1)
  %t546 = getelementptr [23 x float], ptr @fv, i64 0, i64 3
  %t547 = load float, ptr %t546
  %t548 = call float @llvm.sqrt.f32(float %t547)
  %t549 = fpext float %t548 to double
  call void @show(double %t549)
  %t550 = getelementptr [23 x float], ptr @fv, i64 0, i64 3
  %t551 = load float, ptr %t550
  %t552 = call float @llvm.fabs.f32(float %t551)
  %t553 = fpext float %t552 to double
  call void @show(double %t553)
  %t554 = getelementptr [23 x float], ptr @fv, i64 0, i64 3
  %t555 = load float, ptr %t554
  %t556 = call float @llvm.floor.f32(float %t555)
  %t557 = fpext float %t556 to double
  call void @show(double %t557)
  %t558 = getelementptr [23 x float], ptr @fv, i64 0, i64 3
  %t559 = load float, ptr %t558
  %t560 = call float @llvm.ceil.f32(float %t559)
  %t561 = fpext float %t560 to double
  call void @show(double %t561)
  %t562 = getelementptr [23 x float], ptr @fv, i64 0, i64 3
  %t563 = load float, ptr %t562
  %t564 = call float @llvm.trunc.f32(float %t563)
  %t565 = fpext float %t564 to double
  call void @show(double %t565)
  %t566 = getelementptr [23 x float], ptr @fv, i64 0, i64 3
  %t567 = load float, ptr %t566
  %t568 = call float @llvm.rint.f32(float %t567)
  %t569 = fpext float %t568 to double
  call void @show(double %t569)
  %t570 = getelementptr [23 x float], ptr @fv, i64 0, i64 3
  %t571 = load float, ptr %t570
  %t572 = call float @llvm.nearbyint.f32(float %t571)
  %t573 = fpext float %t572 to double
  call void @show(double %t573)
  %t574 = getelementptr [23 x float], ptr @fv, i64 0, i64 3
  %t575 = load float, ptr %t574
  %t576 = call float @llvm.round.f32(float %t575)
  %t577 = fpext float %t576 to double
  call void @show(double %t577)
  %t578 = getelementptr [23 x float], ptr @fv, i64 0, i64 3
  %t579 = load float, ptr %t578
  %t580 = call float @llvm.roundeven.f32(float %t579)
  %t581 = fpext float %t580 to double
  call void @show(double %t581)
  call i32 @putchar(i32 10)
  call void @text(ptr @.s10)
  %t582 = getelementptr [23 x float], ptr @fv, i64 0, i64 4
  %t583 = load float, ptr %t582
  %t584 = fpext float %t583 to double
  call void @show(double %t584)
  call void @text(ptr @.s1)
  %t585 = getelementptr [23 x float], ptr @fv, i64 0, i64 4
  %t586 = load float, ptr %t585
  %t587 = call float @llvm.sqrt.f32(float %t586)
  %t588 = fpext float %t587 to double
  call void @show(double %t588)
  %t589 = getelementptr [23 x float], ptr @fv, i64 0, i64 4
  %t590 = load float, ptr %t589
  %t591 = call float @llvm.fabs.f32(float %t590)
  %t592 = fpext float %t591 to double
  call void @show(double %t592)
  %t593 = getelementptr [23 x float], ptr @fv, i64 0, i64 4
  %t594 = load float, ptr %t593
  %t595 = call float @llvm.floor.f32(float %t594)
  %t596 = fpext float %t595 to double
  call void @show(double %t596)
  %t597 = getelementptr [23 x float], ptr @fv, i64 0, i64 4
  %t598 = load float, ptr %t597
  %t599 = call float @llvm.ceil.f32(float %t598)
  %t600 = fpext float %t599 to double
  call void @show(double %t600)
  %t601 = getelementptr [23 x float], ptr @fv, i64 0, i64 4
  %t602 = load float, ptr %t601
  %t603 = call float @llvm.trunc.f32(float %t602)
  %t604 = fpext float %t603 to double
  call void @show(double %t604)
  %t605 = getelementptr [23 x float], ptr @fv, i64 0, i64 4
  %t606 = load float, ptr %t605
  %t607 = call float @llvm.rint.f32(float %t606)
  %t608 = fpext float %t607 to double
  call void @show(double %t608)
  %t609 = getelementptr [23 x float], ptr @fv, i64 0, i64 4
  %t610 = load float, ptr %t609
  %t611 = call float @llvm.nearbyint.f32(float %t610)
  %t612 = fpext float %t611 to double
  call void @show(double %t612)
  %t613 = getelementptr [23 x float], ptr @fv, i64 0, i64 4
  %t614 = load float, ptr %t613
  %t615 = call float @llvm.round.f32(float %t614)
  %t616 = fpext float %t615 to double
  call void @show(double %t616)
  %t617 = getelementptr [23 x float], ptr @fv, i64 0, i64 4
  %t618 = load float, ptr %t617
  %t619 = call float @llvm.roundeven.f32(float %t618)
  %t620 = fpext float %t619 to double
  call void @show(double %t620)
  call i32 @putchar(i32 10)
  call void @text(ptr @.s10)
  %t621 = getelementptr [23 x float], ptr @fv, i64 0, i64 5
  %t622 = load float, ptr %t621
  %t623 = fpext float %t622 to double
  call void @show(double %t623)
  call void @text(ptr @.s1)
  %t624 = getelementptr [23 x float], ptr @fv, i64 0, i64 5
  %t625 = load float, ptr %t624
  %t626 = call float @llvm.sqrt.f32(float %t625)
  %t627 = fpext float %t626 to double
  call void @show(double %t627)
  %t628 = getelementptr [23 x float], ptr @fv, i64 0, i64 5
  %t629 = load float, ptr %t628
  %t630 = call float @llvm.fabs.f32(float %t629)
  %t631 = fpext float %t630 to double
  call void @show(double %t631)
  %t632 = getelementptr [23 x float], ptr @fv, i64 0, i64 5
  %t633 = load float, ptr %t632
  %t634 = call float @llvm.floor.f32(float %t633)
  %t635 = fpext float %t634 to double
  call void @show(double %t635)
  %t636 = getelementptr [23 x float], ptr @fv, i64 0, i64 5
  %t637 = load float, ptr %t636
  %t638 = call float @llvm.ceil.f32(float %t637)
  %t639 = fpext float %t638 to double
  call void @show(double %t639)
  %t640 = getelementptr [23 x float], ptr @fv, i64 0, i64 5
  %t641 = load float, ptr %t640
  %t642 = call float @llvm.trunc.f32(float %t641)
  %t643 = fpext float %t642 to double
  call void @show(double %t643)
  %t644 = getelementptr [23 x float], ptr @fv, i64 0, i64 5
  %t645 = load float, ptr %t644
  %t646 = call float @llvm.rint.f32(float %t645)
  %t647 = fpext float %t646 to double
  call void @show(double %t647)
  %t648 = getelementptr [23 x float], ptr @fv, i64 0, i64 5
  %t649 = load float, ptr %t648
  %t650 = call float @llvm.nearbyint.f32(float %t649)
  %t651 = fpext float %t650 to double
  call void @show(double %t651)
  %t652 = getelementptr [23 x float], ptr @fv, i64 0, i64 5
  %t653 = load float, ptr %t652
  %t654 = call float @llvm.round.f32(float %t653)
  %t655 = fpext float %t654 to double
  call void @show(double %t655)
  %t656 = getelementptr [23 x float], ptr @fv, i64 0, i64 5
  %t657 = load float, ptr %t656
  %t658 = call float @llvm.roundeven.f32(float %t657)
  %t659 = fpext float %t658 to double
  call void @show(double %t659)
  call i32 @putchar(i32 10)
  call void @text(ptr @.s10)
  %t660 = getelementptr [23 x float], ptr @fv, i64 0, i64 6
  %t661 = load float, ptr %t660
  %t662 = fpext float %t661 to double
  call void @show(double %t662)
  call void @text(ptr @.s1)
  %t663 = getelementptr [23 x float], ptr @fv, i64 0, i64 6
  %t664 = load float, ptr %t663
  %t665 = call float @llvm.sqrt.f32(float %t664)
  %t666 = fpext float %t665 to double
  call void @show(double %t666)
  %t667 = getelementptr [23 x float], ptr @fv, i64 0, i64 6
  %t668 = load float, ptr %t667
  %t669 = call float @llvm.fabs.f32(float %t668)
  %t670 = fpext float %t669 to double
  call void @show(double %t670)
  %t671 = getelementptr [23 x float], ptr @fv, i64 0, i64 6
  %t672 = load float, ptr %t671
  %t673 = call float @llvm.floor.f32(float %t672)
  %t674 = fpext float %t673 to double
  call void @show(double %t674)
  %t675 = getelementptr [23 x float], ptr @fv, i64 0, i64 6
  %t676 = load float, ptr %t675
  %t677 = call float @llvm.ceil.f32(float %t676)
  %t678 = fpext float %t677 to double
  call void @show(double %t678)
  %t679 = getelementptr [23 x float], ptr @fv, i64 0, i64 6
  %t680 = load float, ptr %t679
  %t681 = call float @llvm.trunc.f32(float %t680)
  %t682 = fpext float %t681 to double
  call void @show(double %t682)
  %t683 = getelementptr [23 x float], ptr @fv, i64 0, i64 6
  %t684 = load float, ptr %t683
  %t685 = call float @llvm.rint.f32(float %t684)
  %t686 = fpext float %t685 to double
  call void @show(double %t686)
  %t687 = getelementptr [23 x float], ptr @fv, i64 0, i64 6
  %t688 = load float, ptr %t687
  %t689 = call float @llvm.nearbyint.f32(float %t688)
  %t690 = fpext float %t689 to double
  call void @show(double %t690)
  %t691 = getelementptr [23 x float], ptr @fv, i64 0, i64 6
  %t692 = load float, ptr %t691
  %t693 = call float @llvm.round.f32(float %t692)
  %t694 = fpext float %t693 to double
  call void @show(double %t694)
  %t695 = getelementptr [23 x float], ptr @fv, i64 0, i64 6
  %t696 = load float, ptr %t695
  %t697 = call float @llvm.roundeven.f32(float %t696)
  %t698 = fpext float %t697 to double
  call void @show(double %t698)
  call i32 @putchar(i32 10)
  call void @text(ptr @.s10)
  %t699 = getelementptr [23 x float], ptr @fv, i64 0, i64 7
  %t700 = load float, ptr %t699
  %t701 = fpext float %t700 to double
  call void @show(double %t701)
  call void @text(ptr @.s1)
  %t702 = getelementptr [23 x float], ptr @fv, i64 0, i64 7
  %t703 = load float, ptr %t702
  %t704 = call float @llvm.sqrt.f32(float %t703)
  %t705 = fpext float %t704 to double
  call void @show(double %t705)
  %t706 = getelementptr [23 x float], ptr @fv, i64 0, i64 7
  %t707 = load float, ptr %t706
  %t708 = call float @llvm.fabs.f32(float %t707)
  %t709 = fpext float %t708 to double
  call void @show(double %t709)
  %t710 = getelementptr [23 x float], ptr @fv, i64 0, i64 7
  %t711 = load float, ptr %t710
  %t712 = call float @llvm.floor.f32(float %t711)
  %t713 = fpext float %t712 to double
  call void @show(double %t713)
  %t714 = getelementptr [23 x float], ptr @fv, i64 0, i64 7
  %t715 = load float, ptr %t714
  %t716 = call float @llvm.ceil.f32(float %t715)
  %t717 = fpext float %t716 to double
  call void @show(double %t717)
  %t718 = getelementptr [23 x float], ptr @fv, i64 0, i64 7
  %t719 = load float, ptr %t718
  %t720 = call float @llvm.trunc.f32(float %t719)
  %t721 = fpext float %t720 to double
  call void @show(double %t721)
  %t722 = getelementptr [23 x float], ptr @fv, i64 0, i64 7
  %t723 = load float, ptr %t722
  %t724 = call float @llvm.rint.f32(float %t723)
  %t725 = fpext float %t724 to double
  call void @show(double %t725)
  %t726 = getelementptr [23 x float], ptr @fv, i64 0, i64 7
  %t727 = load float, ptr %t726
  %t728 = call float @llvm.nearbyint.f32(float %t727)
  %t729 = fpext float %t728 to double
  call void @show(double %t729)
  %t730 = getelementptr [23 x float], ptr @fv, i64 0, i64 7
  %t731 = load float, ptr %t730
  %t732 = call float @llvm.round.f32(float %t731)
  %t733 = fpext float %t732 to double
  call void @show(double %t733)
  %t734 = getelementptr [23 x float], ptr @fv, i64 0, i64 7
  %t735 = load float, ptr %t734
  %t736 = call float @llvm.roundeven.f32(float %t735)
  %t737 = fpext float %t736 to double
  call void @show(double %t737)
  call i32 @putchar(i32 10)
  call void @text(ptr @.s10)
  %t738 = getelementptr [23 x float], ptr @fv, i64 0, i64 8
  %t739 = load float, ptr %t738
  %t740 = fpext float %t739 to double
  call void @show(double %t740)
  call void @text(ptr @.s1)
  %t741 = getelementptr [23 x float], ptr @fv, i64 0, i64 8
  %t742 = load float, ptr %t741
  %t743 = call float @llvm.sqrt.f32(float %t742)
  %t744 = fpext float %t743 to double
  call void @show(double %t744)
  %t745 = getelementptr [23 x float], ptr @fv, i64 0, i64 8
  %t746 = load float, ptr %t745
  %t747 = call float @llvm.fabs.f32(float %t746)
  %t748 = fpext float %t747 to double
  call void @show(double %t748)
  %t749 = getelementptr [23 x float], ptr @fv, i64 0, i64 8
  %t750 = load float, ptr %t749
  %t751 = call float @llvm.floor.f32(float %t750)
  %t752 = fpext float %t751 to double
  call void @show(double %t752)
  %t753 = getelementptr [23 x float], ptr @fv, i64 0, i64 8
  %t754 = load float, ptr %t753
  %t755 = call float @llvm.ceil.f32(float %t754)
  %t756 = fpext float %t755 to double
  call void @show(double %t756)
  %t757 = getelementptr [23 x float], ptr @fv, i64 0, i64 8
  %t758 = load float, ptr %t757
  %t759 = call float @llvm.trunc.f32(float %t758)
  %t760 = fpext float %t759 to double
  call void @show(double %t760)
  %t761 = getelementptr [23 x float], ptr @fv, i64 0, i64 8
  %t762 = load float, ptr %t761
  %t763 = call float @llvm.rint.f32(float %t762)
  %t764 = fpext float %t763 to double
  call void @show(double %t764)
  %t765 = getelementptr [23 x float], ptr @fv, i64 0, i64 8
  %t766 = load float, ptr %t765
  %t767 = call float @llvm.nearbyint.f32(float %t766)
  %t768 = fpext float %t767 to double
  call void @show(double %t768)
  %t769 = getelementptr [23 x float], ptr @fv, i64 0, i64 8
  %t770 = load float, ptr %t769
  %t771 = call float @llvm.round.f32(float %t770)
  %t772 = fpext float %t771 to double
  call void @show(double %t772)
  %t773 = getelementptr [23 x float], ptr @fv, i64 0, i64 8
  %t774 = load float, ptr %t773
  %t775 = call float @llvm.roundeven.f32(float %t774)
  %t776 = fpext float %t775 to double
  call void @show(double %t776)
  call i32 @putchar(i32 10)
  call void @text(ptr @.s10)
  %t777 = getelementptr [23 x float], ptr @fv, i64 0, i64 9
  %t778 = load float, ptr %t777
  %t779 = fpext float %t778 to double
  call void @show(double %t779)
  call void @text(ptr @.s1)
  %t780 = getelementptr [23 x float], ptr @fv, i64 0, i64 9
  %t781 = load float, ptr %t780
  %t782 = call float @llvm.sqrt.f32(float %t781)
  %t783 = fpext float %t782 to double
  call void @show(double %t783)
  %t784 = getelementptr [23 x float], ptr @fv, i64 0, i64 9
  %t785 = load float, ptr %t784
  %t786 = call float @llvm.fabs.f32(float %t785)
  %t787 = fpext float %t786 to double
  call void @show(double %t787)
  %t788 = getelementptr [23 x float], ptr @fv, i64 0, i64 9
  %t789 = load float, ptr %t788
  %t790 = call float @llvm.floor.f32(float %t789)
  %t791 = fpext float %t790 to double
  call void @show(double %t791)
  %t792 = getelementptr [23 x float], ptr @fv, i64 0, i64 9
  %t793 = load float, ptr %t792
  %t794 = call float @llvm.ceil.f32(float %t793)
  %t795 = fpext float %t794 to double
  call void @show(double %t795)
  %t796 = getelementptr [23 x float], ptr @fv, i64 0, i64 9
  %t797 = load float, ptr %t796
  %t798 = call float @llvm.trunc.f32(float %t797)
  %t799 = fpext float %t798 to double
  call void @show(double %t799)
  %t800 = getelementptr [23 x float], ptr @fv, i64 0, i64 9
  %t801 = load float, ptr %t800
  %t802 = call float @llvm.rint.f32(float %t801)
  %t803 = fpext float %t802 to double
  call void @show(double %t803)
  %t804 = getelementptr [23 x float], ptr @fv, i64 0, i64 9
  %t805 = load float, ptr %t804
  %t806 = call float @llvm.nearbyint.f32(float %t805)
  %t807 = fpext float %t806 to double
  call void @show(double %t807)
  %t808 = getelementptr [23 x float], ptr @fv, i64 0, i64 9
  %t809 = load float, ptr %t808
  %t810 = call float @llvm.round.f32(float %t809)
  %t811 = fpext float %t810 to double
  call void @show(double %t811)
  %t812 = getelementptr [23 x float], ptr @fv, i64 0, i64 9
  %t813 = load float, ptr %t812
  %t814 = call float @llvm.roundeven.f32(float %t813)
  %t815 = fpext float %t814 to double
  call void @show(double %t815)
  call i32 @putchar(i32 10)
  call void @text(ptr @.s10)
  %t816 = getelementptr [23 x float], ptr @fv, i64 0, i64 10
  %t817 = load float, ptr %t816
  %t818 = fpext float %t817 to double
  call void @show(double %t818)
  call void @text(ptr @.s2)
  %t819 = getelementptr [23 x float], ptr @fv, i64 0, i64 11
  %t820 = load float, ptr %t819
  %t821 = fpext float %t820 to double
  call void @show(double %t821)
  call void @text(ptr @.s3)
  %t822 = getelementptr [23 x float], ptr @fv, i64 0, i64 10
  %t823 = load float, ptr %t822
  %t824 = getelementptr [23 x float], ptr @fv, i64 0, i64 11
  %t825 = load float, ptr %t824
  %t826 = call float @llvm.minimum.f32(float %t823, float %t825)
  %t827 = fpext float %t826 to double
  call void @show(double %t827)
  call void @text(ptr @.s4)
  %t828 = getelementptr [23 x float], ptr @fv, i64 0, i64 10
  %t829 = load float, ptr %t828
  %t830 = getelementptr [23 x float], ptr @fv, i64 0, i64 11
  %t831 = load float, ptr %t830
  %t832 = call float @llvm.maximum.f32(float %t829, float %t831)
  %t833 = fpext float %t832 to double
  call void @show(double %t833)
  call void @text(ptr @.s5)
  %t834 = getelementptr [23 x float], ptr @fv, i64 0, i64 10
  %t835 = load float, ptr %t834
  %t836 = getelementptr [23 x float], ptr @fv, i64 0, i64 11
  %t837 = load float, ptr %t836
  %t838 = call float @llvm.copysign.f32(float %t835, float %t837)
  %t839 = fpext float %t838 to double
  call void @show(double %t839)
  call void @text(ptr @.s6)
  %t840 = getelementptr [23 x float], ptr @fv, i64 0, i64 10
  %t841 = load float, ptr %t840
  %t842 = getelementptr [23 x float], ptr @fv, i64 0, i64 11
  %t843 = load float, ptr %t842
  %t844 = call float @llvm.minnum.f32(float %t841, float %t843)
  %t845 = fpext float %t844 to double
  call void @show(double %t845)
  call void @text(ptr @.s7)
  %t846 = getelementptr [23 x float], ptr @fv, i64 0, i64 10
  %t847 = load float, ptr %t846
  %t848 = getelementptr [23 x float], ptr @fv, i64 0, i64 11
  %t849 = load float, ptr %t848
  %t850 = call float @llvm.maxnum.f32(float %t847, float %t849)
  %t851 = fpext float %t850 to double
  call void @show(double %t851)
  call i32 @putchar(i32 10)
  call void @text(ptr @.s10)
  %t852 = getelementptr [23 x float], ptr @fv, i64 0, i64 12
  %t853 = load float, ptr %t852
  %t854 = fpext float %t853 to double
  call void @show(double %t854)
  call void @text(ptr @.s2)
  %t855 = getelementptr [23 x float], ptr @fv, i64 0, i64 13
  %t856 = load float, ptr %t855
  %t857 = fpext float %t856 to double
  call void @show(double %t857)
  call void @text(ptr @.s3)
  %t858 = getelementptr [23 x float], ptr @fv, i64 0, i64 12
  %t859 = load float, ptr %t858
  %t860 = getelementptr [23 x float], ptr @fv, i64 0, i64 13
  %t861 = load float, ptr %t860
  %t862 = call float @llvm.minimum.f32(float %t859, float %t861)
  %t863 = fpext float %t862 to double
  call void @show(double %t863)
  call void @text(ptr @.s4)
  %t864 = getelementptr [23 x float], ptr @fv, i64 0, i64 12
  %t865 = load float, ptr %t864
  %t866 = getelementptr [23 x float], ptr @fv, i64 0, i64 13
  %t867 = load float, ptr %t866
  %t868 = call float @llvm.maximum.f32(float %t865, float %t867)
  %t869 = fpext float %t868 to double
  call void @show(double %t869)
  call void @text(ptr @.s5)
  %t870 = getelementptr [23 x float], ptr @fv, i64 0, i64 12
  %t871 = load float, ptr %t870
  %t872 = getelementptr [23 x float], ptr @fv, i64 0, i64 13
  %t873 = load float, ptr %t872
  %t874 = call float @llvm.copysign.f32(float %t871, float %t873)
  %t875 = fpext float %t874 to double
  call void @show(double %t875)
  call i32 @putchar(i32 10)
  call void @text(ptr @.s10)
  %t876 = getelementptr [23 x float], ptr @fv, i64 0, i64 14
  %t877 = load float, ptr %t876
  %t878 = fpext float %t877 to double
  call void @show(double %t878)
  call void @text(ptr @.s2)
  %t879 = getelementptr [23 x float], ptr @fv, i64 0, i64 15
  %t880 = load float, ptr %t879
  %t881 = fpext float %t880 to double
  call void @show(double %t881)
  call void @text(ptr @.s3)
  %t882 = getelementptr [23 x float], ptr @fv, i64 0, i64 14
  %t883 = load float, ptr %t882
  %t884 = getelementptr [23 x float], ptr @fv, i64 0, i64 15
  %t885 = load float, ptr %t884
  %t886 = call float @llvm.minimum.f32(float %t883, float %t885)
  %t887 = fpext float %t886 to double
  call void @show(double %t887)
  call void @text(ptr @.s4)
  %t888 = getelementptr [23 x float], ptr @fv, i64 0, i64 14
  %t889 = load float, ptr %t888
  %t890 = getelementptr [23 x float], ptr @fv, i64 0, i64 15
  %t891 = load float, ptr %t890
  %t892 = call float @llvm.maximum.f32(float %t889, float %t891)
  %t893 = fpext float %t892 to double
  call void @show(double %t893)
  call void @text(ptr @.s5)
  %t894 = getelementptr [23 x float], ptr @fv, i64 0, i64 14
  %t895 = load float, ptr %t894
  %t896 = getelementptr [23 x float], ptr @fv, i64 0, i64 15
  %t897 = load float, ptr %t896
  %t898 = call float @llvm.copysign.f32(float %t895, float %t897)
  %t899 = fpext float %t898 to double
  call void @show(double %t899)
  call i32 @putchar(i32 10)
  call void @text(ptr @.s10)
  %t900 = getelementptr [23 x float], ptr @fv, i64 0, i64 16
  %t901 = load float, ptr %t900
  %t902 = fpext float %t901 to double
  call void @show(double %t902)
  call void @text(ptr @.s2)
  %t903 = getelementptr [23 x float], ptr @fv, i64 0, i64 17
  %t904 = load float, ptr %t903
  %t905 = fpext float %t904 to double
  call void @show(double %t905)
  call void @text(ptr @.s3)
  %t906 = getelementptr [23 x float], ptr @fv, i64 0, i64 16
  %t907 = load float, ptr %t906
  %t908 = getelementptr [23 x float], ptr @fv, i64 0, i64 17
  %t909 = load float, ptr %t908
  %t910 = call float @llvm.minimum.f32(float %t907, float %t909)
  %t911 = fpext float %t910 to double
  call void @show(double %t911)
  call void @text(ptr @.s4)
  %t912 = getelementptr [23 x float], ptr @fv, i64 0, i64 16
  %t913 = load float, ptr %t912
  %t914 = getelementptr [23 x float], ptr @fv, i64 0, i64 17
  %t915 = load float, ptr %t914
  %t916 = call float @llvm.maximum.f32(float %t913, float %t915)
  %t917 = fpext float %t916 to double
  call void @show(double %t917)
  call void @text(ptr @.s6)
  %t918 = getelementptr [23 x float], ptr @fv, i64 0, i64 16
  %t919 = load float, ptr %t918
  %t920 = getelementptr [23 x float], ptr @fv, i64 0, i64 17
  %t921 = load float, ptr %t920
  %t922 = call float @llvm.minnum.f32(float %t919, float %t921)
  %t923 = fpext float %t922 to double
  call void @show(double %t923)
  call void @text(ptr @.s7)
  %t924 = getelementptr [23 x float], ptr @fv, i64 0, i64 16
  %t925 = load float, ptr %t924
  %t926 = getelementptr [23 x float], ptr @fv, i64 0, i64 17
  %t927 = load float, ptr %t926
  %t928 = call float @llvm.maxnum.f32(float %t925, float %t927)
  %t929 = fpext float %t928 to double
  call void @show(double %t929)
  call i32 @putchar(i32 10)
  call void @text(ptr @.s10)
  %t930 = getelementptr [23 x float], ptr @fv, i64 0, i64 18
  %t931 = load float, ptr %t930
  %t932 = fpext float %t931 to double
  call void @show(double %t932)
  call void @text(ptr @.s2)
  %t933 = getelementptr [23 x float], ptr @fv, i64 0, i64 19
  %t934 = load float, ptr %t933
  %t935 = fpext float %t934 to double
  call void @show(double %t935)
  call void @text(ptr @.s3)
  %t936 = getelementptr [23 x float], ptr @fv, i64 0, i64 18
  %t937 = load float, ptr %t936
  %t938 = getelementptr [23 x float], ptr @fv, i64 0, i64 19
  %t939 = load float, ptr %t938
  %t940 = call float @llvm.minimum.f32(float %t937, float %t939)
  %t941 = fpext float %t940 to double
  call void @show(double %t941)
  call void @text(ptr @.s4)
  %t942 = getelementptr [23 x float], ptr @fv, i64 0, i64 18
  %t943 = load float, ptr %t942
  %t944 = getelementptr [23 x float], ptr @fv, i64 0, i64 19
  %t945 = load float, ptr %t944
  %t946 = call float @llvm.maximum.f32(float %t943, float %t945)
  %t947 = fpext float %t946 to double
  call void @show(double %t947)
  call void @text(ptr @.s5)
  %t948 = getelementptr [23 x float], ptr @fv, i64 0, i64 18
  %t949 = load float, ptr %t948
  %t950 = getelementptr [23 x float], ptr @fv, i64 0, i64 19
  %t951 = load float, ptr %t950
  %t952 = call float @llvm.copysign.f32(float %t949, float %t951)
  %t953 = fpext float %t952 to double
  call void @show(double %t953)
  call void @text(ptr @.s6)
  %t954 = getelementptr [23 x float], ptr @fv, i64 0, i64 18
  %t955 = load float, ptr %t954
  %t956 = getelementptr [23 x float], ptr @fv, i64 0, i64 19
  %t957 = load float, ptr %t956
  %t958 = call float @llvm.minnum.f32(float %t955, float %t957)
  %t959 = fpext float %t958 to double
  call void @show(double %t959)
  call void @text(ptr @.s7)
  %t960 = getelementptr [23 x float], ptr @fv, i64 0, i64 18
  %t961 = load float, ptr %t960
  %t962 = getelementptr [23 x float], ptr @fv, i64 0, i64 19
  %t963 = load float, ptr %t962
  %t964 = call float @llvm.maxnum.f32(float %t961, float %t963)
  %t965 = fpext float %t964 to double
  call void @show(double %t965)
  call i32 @putchar(i32 10)
  call void @text(ptr @.s11)
  %t966 = getelementptr [23 x float], ptr @fv, i64 0, i64 20
  %t967 = load float, ptr %t966
  %t968 = getelementptr [23 x float], ptr @fv, i64 0, i64 21
  %t969 = load float, ptr %t968
  %t970 = getelementptr [23 x float], ptr @fv, i64 0, i64 22
  %t971 = load float, ptr %t970
  %t972 = call float @llvm.fma.f32(float %t967, float %t969, float %t971)
  %t973 = fpext float %t972 to double
  call void @show(double %t973)
  call void @text(ptr @.s9)
  %t974 = getelementptr [23 x float], ptr @fv, i64 0, i64 20
  %t975 = load float, ptr %t974
  %t976 = getelementptr [23 x float], ptr @fv, i64 0, i64 21
  %t977 = load float, ptr %t976
  %t978 = getelementptr [23 x float], ptr @fv, i64 0, i64 22
  %t979 = load float, ptr %t978
  %t980 = fmul float %t975, %t977
  %t981 = fadd float %t980, %t979
  %t982 = fpext float %t981 to double
  call void @show(double %t982)
  call i32 @putchar(i32 10)
  ret i32 0
}
