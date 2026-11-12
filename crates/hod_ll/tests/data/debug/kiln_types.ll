; spec/ll.md §7.4: what kiln++ -g -O0 makes of debug_types.cpp (tests/debug.rs).
; ModuleID = '<stdin>'
source_filename = "debug_types.cpp"
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i8:8:32-i16:16:32-i64:64-i128:128-n32:64-S128-Fn32"
target triple = "aarch64-unknown-linux-gnu"

%struct.Bits = type { i64 }
%struct.Box = type { [3 x i32] }
%"struct.ns::S" = type { %"struct.ns::B.base", [3 x i32], i32, [4 x i8] }
%"struct.ns::B.base" = type <{ ptr, i32 }>
%class.anon = type { ptr }
%union.U = type { i32 }
%struct.Derived = type { ptr, %struct.Base.base }
%struct.Base.base = type <{ ptr, i32 }>

$_ZN2ns1SIiLi3EEC2Ev = comdat any

$_ZN7DerivedC1Ev = comdat any

$_ZNK7Derived1vEv = comdat any

$_ZN7DerivedD1Ev = comdat any

$_ZN2ns1SIiLi3EED2Ev = comdat any

$_ZN2ns1BC2Ev = comdat any

$_ZN2ns1SIiLi3EED0Ev = comdat any

$_ZN2ns1BD2Ev = comdat any

$_ZN2ns1BD0Ev = comdat any

$_ZN4BaseC2Ev = comdat any

$_ZN7DerivedD0Ev = comdat any

$_ZTv0_n24_N7DerivedD1Ev = comdat any

$_ZTv0_n24_N7DerivedD0Ev = comdat any

$_ZTv0_n32_NK7Derived1vEv = comdat any

$_ZN4BaseD2Ev = comdat any

$_ZN4BaseD0Ev = comdat any

$_ZNK4Base1vEv = comdat any

$_ZNK2ns1SIiLi3EE1fEi = comdat any

$_ZN7DerivedD2Ev = comdat any

$_ZTW2tl = comdat any

$_ZN2ns1SIiLi3EE2smE = comdat any

$_ZTVN2ns1SIiLi3EEE = comdat any

$_ZTIN2ns1SIiLi3EEE = comdat any

$_ZTSN2ns1SIiLi3EEE = comdat any

$_ZTIN2ns1BE = comdat any

$_ZTSN2ns1BE = comdat any

$_ZTVN2ns1BE = comdat any

$_ZTV7Derived = comdat any

$_ZTT7Derived = comdat any

$_ZTI7Derived = comdat any

$_ZTS7Derived = comdat any

$_ZTI4Base = comdat any

$_ZTS4Base = comdat any

$_ZTV4Base = comdat any

@g = dso_local global i32 4, align 4, !dbg !0
@tl = dso_local thread_local global i32 1, align 4, !dbg !18
@_ZZ7countervE5count = internal global i32 0, align 4, !dbg !20
@__const.main.bits = private unnamed_addr constant %struct.Bits { i64 785 }, align 8
@__const.main.box = private unnamed_addr constant %struct.Box { [3 x i32] [i32 1, i32 2, i32 3] }, align 4
@.str = private unnamed_addr constant [10 x i8] c"%d %d %f\0A\00", align 1
@_ZL1h = internal global double 2.500000e+00, align 8, !dbg !24
@_ZN2ns1SIiLi3EE2smE = linkonce_odr dso_local global i32 3, comdat, align 4, !dbg !27
@_ZTVN2ns1SIiLi3EEE = linkonce_odr dso_local unnamed_addr constant { [4 x ptr] } { [4 x ptr] [ptr null, ptr @_ZTIN2ns1SIiLi3EEE, ptr @_ZN2ns1SIiLi3EED2Ev, ptr @_ZN2ns1SIiLi3EED0Ev] }, comdat, align 8
@_ZTIN2ns1SIiLi3EEE = linkonce_odr dso_local constant { ptr, ptr, ptr } { ptr getelementptr inbounds (ptr, ptr @_ZTVN10__cxxabiv120__si_class_type_infoE, i64 2), ptr @_ZTSN2ns1SIiLi3EEE, ptr @_ZTIN2ns1BE }, comdat, align 8
@_ZTVN10__cxxabiv120__si_class_type_infoE = external global [0 x ptr]
@_ZTSN2ns1SIiLi3EEE = linkonce_odr dso_local constant [15 x i8] c"N2ns1SIiLi3EEE\00", comdat, align 1
@_ZTIN2ns1BE = linkonce_odr dso_local constant { ptr, ptr } { ptr getelementptr inbounds (ptr, ptr @_ZTVN10__cxxabiv117__class_type_infoE, i64 2), ptr @_ZTSN2ns1BE }, comdat, align 8
@_ZTVN10__cxxabiv117__class_type_infoE = external global [0 x ptr]
@_ZTSN2ns1BE = linkonce_odr dso_local constant [8 x i8] c"N2ns1BE\00", comdat, align 1
@_ZTVN2ns1BE = linkonce_odr dso_local unnamed_addr constant { [4 x ptr] } { [4 x ptr] [ptr null, ptr @_ZTIN2ns1BE, ptr @_ZN2ns1BD2Ev, ptr @_ZN2ns1BD0Ev] }, comdat, align 8
@_ZTV7Derived = linkonce_odr dso_local unnamed_addr constant { [6 x ptr], [7 x ptr] } { [6 x ptr] [ptr inttoptr (i64 8 to ptr), ptr null, ptr @_ZTI7Derived, ptr @_ZNK7Derived1vEv, ptr @_ZN7DerivedD1Ev, ptr @_ZN7DerivedD0Ev], [7 x ptr] [ptr inttoptr (i64 -8 to ptr), ptr inttoptr (i64 -8 to ptr), ptr inttoptr (i64 -8 to ptr), ptr @_ZTI7Derived, ptr @_ZTv0_n24_N7DerivedD1Ev, ptr @_ZTv0_n24_N7DerivedD0Ev, ptr @_ZTv0_n32_NK7Derived1vEv] }, comdat, align 8
@_ZTT7Derived = linkonce_odr dso_local unnamed_addr constant [2 x ptr] [ptr getelementptr inbounds inrange(-24, 24) ({ [6 x ptr], [7 x ptr] }, ptr @_ZTV7Derived, i32 0, i32 0, i32 3), ptr getelementptr inbounds inrange(-32, 24) ({ [6 x ptr], [7 x ptr] }, ptr @_ZTV7Derived, i32 0, i32 1, i32 4)], comdat, align 8
@_ZTI7Derived = linkonce_odr dso_local constant { ptr, ptr, i32, i32, ptr, i64 } { ptr getelementptr inbounds (ptr, ptr @_ZTVN10__cxxabiv121__vmi_class_type_infoE, i64 2), ptr @_ZTS7Derived, i32 0, i32 1, ptr @_ZTI4Base, i64 -6141 }, comdat, align 8
@_ZTVN10__cxxabiv121__vmi_class_type_infoE = external global [0 x ptr]
@_ZTS7Derived = linkonce_odr dso_local constant [9 x i8] c"7Derived\00", comdat, align 1
@_ZTI4Base = linkonce_odr dso_local constant { ptr, ptr } { ptr getelementptr inbounds (ptr, ptr @_ZTVN10__cxxabiv117__class_type_infoE, i64 2), ptr @_ZTS4Base }, comdat, align 8
@_ZTS4Base = linkonce_odr dso_local constant [6 x i8] c"4Base\00", comdat, align 1
@_ZTV4Base = linkonce_odr dso_local unnamed_addr constant { [5 x ptr] } { [5 x ptr] [ptr null, ptr @_ZTI4Base, ptr @_ZN4BaseD2Ev, ptr @_ZN4BaseD0Ev, ptr @_ZNK4Base1vEv] }, comdat, align 8

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define dso_local noundef i32 @_ZN5outer2v14deepEi(i32 noundef %0) #0 !dbg !40 {
  %2 = alloca i32, align 4
  store i32 %0, ptr %2, align 4
    #dbg_declare(ptr %2, !46, !DIExpression(), !47)
  %3 = load i32, ptr %2, align 4, !dbg !48
  %4 = mul nsw i32 %3, 3, !dbg !49
  ret i32 %4, !dbg !50
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define dso_local noundef i32 @_Z3vlai(i32 noundef %0) #0 !dbg !51 {
  %2 = alloca i32, align 4
  %3 = alloca ptr, align 8
  %4 = alloca i64, align 8
  %5 = alloca i32, align 4
  store i32 %0, ptr %2, align 4
    #dbg_declare(ptr %2, !52, !DIExpression(), !53)
  %6 = load i32, ptr %2, align 4, !dbg !54
  %7 = zext i32 %6 to i64, !dbg !55
  %8 = call ptr @llvm.stacksave.p0(), !dbg !55
  store ptr %8, ptr %3, align 8, !dbg !55
  %9 = alloca i32, i64 %7, align 4, !dbg !55
  store i64 %7, ptr %4, align 8, !dbg !55
    #dbg_declare(ptr %5, !56, !DIExpression(), !58)
  store i32 0, ptr %5, align 4, !dbg !58
  br label %10, !dbg !59

10:                                               ; preds = %19, %1
  %11 = load i32, ptr %5, align 4, !dbg !60
  %12 = load i32, ptr %2, align 4, !dbg !62
  %13 = icmp slt i32 %11, %12, !dbg !63
  br i1 %13, label %14, label %22, !dbg !64

14:                                               ; preds = %10
  %15 = load i32, ptr %5, align 4, !dbg !65
  %16 = load i32, ptr %5, align 4, !dbg !66
  %17 = sext i32 %16 to i64, !dbg !67
  %18 = getelementptr inbounds i32, ptr %9, i64 %17, !dbg !67
  store i32 %15, ptr %18, align 4, !dbg !68
  br label %19, !dbg !67

19:                                               ; preds = %14
  %20 = load i32, ptr %5, align 4, !dbg !69
  %21 = add nsw i32 %20, 1, !dbg !69
  store i32 %21, ptr %5, align 4, !dbg !69
  br label %10, !dbg !70, !llvm.loop !71

22:                                               ; preds = %10
  %23 = load i32, ptr %2, align 4, !dbg !74
  %24 = sub nsw i32 %23, 1, !dbg !75
  %25 = sext i32 %24 to i64, !dbg !76
  %26 = getelementptr inbounds i32, ptr %9, i64 %25, !dbg !76
  %27 = load i32, ptr %26, align 4, !dbg !76
  %28 = load ptr, ptr %3, align 8, !dbg !77
  call void @llvm.stackrestore.p0(ptr %28), !dbg !77
  ret i32 %27, !dbg !77
}

; Function Attrs: nocallback nofree nosync nounwind willreturn
declare ptr @llvm.stacksave.p0() #1

; Function Attrs: nocallback nofree nosync nounwind willreturn
declare void @llvm.stackrestore.p0(ptr) #1

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define dso_local noundef i32 @_Z3sumiz(i32 noundef %0, ...) #0 !dbg !78 {
  %2 = alloca i32, align 4
  %3 = alloca i32, align 4
  %4 = alloca i32, align 4
  %5 = alloca i32, align 4
  store i32 %0, ptr %2, align 4
    #dbg_declare(ptr %2, !81, !DIExpression(), !82)
    #dbg_declare(ptr %3, !83, !DIExpression(), !84)
  store i32 0, ptr %3, align 4, !dbg !84
    #dbg_declare(ptr %4, !85, !DIExpression(), !87)
  store i32 0, ptr %4, align 4, !dbg !87
  br label %6, !dbg !88

6:                                                ; preds = %16, %1
  %7 = load i32, ptr %4, align 4, !dbg !89
  %8 = load i32, ptr %2, align 4, !dbg !91
  %9 = icmp slt i32 %7, %8, !dbg !92
  br i1 %9, label %10, label %19, !dbg !93

10:                                               ; preds = %6
    #dbg_declare(ptr %5, !94, !DIExpression(), !96)
  %11 = load i32, ptr %4, align 4, !dbg !97
  %12 = mul nsw i32 %11, 2, !dbg !98
  store i32 %12, ptr %5, align 4, !dbg !96
  %13 = load i32, ptr %5, align 4, !dbg !99
  %14 = load i32, ptr %3, align 4, !dbg !100
  %15 = add nsw i32 %14, %13, !dbg !100
  store i32 %15, ptr %3, align 4, !dbg !100
  br label %16, !dbg !101

16:                                               ; preds = %10
  %17 = load i32, ptr %4, align 4, !dbg !102
  %18 = add nsw i32 %17, 1, !dbg !102
  store i32 %18, ptr %4, align 4, !dbg !102
  br label %6, !dbg !103, !llvm.loop !104

19:                                               ; preds = %6
  br label %20, !dbg !105

20:                                               ; preds = %23, %19
  %21 = load i32, ptr %3, align 4, !dbg !106
  %22 = icmp slt i32 %21, 0, !dbg !108
  br i1 %22, label %23, label %24, !dbg !109

23:                                               ; preds = %20
  br label %20, !dbg !110

24:                                               ; preds = %20
  %25 = load i32, ptr %3, align 4, !dbg !111
  ret i32 %25, !dbg !112
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define dso_local noundef i32 @_Z7counterv() #0 !dbg !22 {
  %1 = load i32, ptr @_ZZ7countervE5count, align 4, !dbg !113
  %2 = add nsw i32 %1, 1, !dbg !113
  store i32 %2, ptr @_ZZ7countervE5count, align 4, !dbg !113
  ret i32 %2, !dbg !114
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define dso_local noundef i128 @_Z4widen(i128 noundef %0) #0 !dbg !115 {
  %2 = alloca i128, align 16
  store i128 %0, ptr %2, align 16
    #dbg_declare(ptr %2, !119, !DIExpression(), !120)
  %3 = load i128, ptr %2, align 16, !dbg !121
  %4 = mul nsw i128 %3, 3, !dbg !122
  ret i128 %4, !dbg !123
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define dso_local noundef { double, double } @_Z6squareCd([2 x double] noundef alignstack(8) %0) #0 !dbg !124 {
  %2 = alloca { double, double }, align 8
  %3 = alloca { double, double }, align 8
  store [2 x double] %0, ptr %3, align 8
    #dbg_declare(ptr %3, !128, !DIExpression(), !129)
  %4 = getelementptr inbounds nuw { double, double }, ptr %3, i32 0, i32 0, !dbg !130
  %5 = load double, ptr %4, align 8, !dbg !130
  %6 = getelementptr inbounds nuw { double, double }, ptr %3, i32 0, i32 1, !dbg !130
  %7 = load double, ptr %6, align 8, !dbg !130
  %8 = getelementptr inbounds nuw { double, double }, ptr %3, i32 0, i32 0, !dbg !131
  %9 = load double, ptr %8, align 8, !dbg !131
  %10 = getelementptr inbounds nuw { double, double }, ptr %3, i32 0, i32 1, !dbg !131
  %11 = load double, ptr %10, align 8, !dbg !131
  %12 = fmul double %5, %9, !dbg !132
  %13 = fmul double %7, %11, !dbg !132
  %14 = fmul double %5, %11, !dbg !132
  %15 = fmul double %7, %9, !dbg !132
  %16 = fsub double %12, %13, !dbg !132
  %17 = fadd double %14, %15, !dbg !132
  %18 = fcmp uno double %16, %16, !dbg !132
  br i1 %18, label %19, label %25, !dbg !132, !prof !133

19:                                               ; preds = %1
  %20 = fcmp uno double %17, %17, !dbg !132
  br i1 %20, label %21, label %25, !dbg !132, !prof !133

21:                                               ; preds = %19
  %22 = call noundef { double, double } @__muldc3(double noundef %5, double noundef %7, double noundef %9, double noundef %11) #12, !dbg !132
  %23 = extractvalue { double, double } %22, 0, !dbg !132
  %24 = extractvalue { double, double } %22, 1, !dbg !132
  br label %25, !dbg !132

25:                                               ; preds = %21, %19, %1
  %26 = phi double [ %16, %1 ], [ %16, %19 ], [ %23, %21 ], !dbg !132
  %27 = phi double [ %17, %1 ], [ %17, %19 ], [ %24, %21 ], !dbg !132
  %28 = getelementptr inbounds nuw { double, double }, ptr %2, i32 0, i32 0, !dbg !134
  %29 = getelementptr inbounds nuw { double, double }, ptr %2, i32 0, i32 1, !dbg !134
  store double %26, ptr %28, align 8, !dbg !134
  store double %27, ptr %29, align 8, !dbg !134
  %30 = load { double, double }, ptr %2, align 8, !dbg !134
  ret { double, double } %30, !dbg !134
}

declare { double, double } @__muldc3(double, double, double, double)

; Function Attrs: mustprogress noinline norecurse optnone uwtable
define dso_local noundef i32 @main(i32 noundef %0, ptr noundef %1) #2 personality ptr @__gxx_personality_v0 !dbg !135 {
  %3 = alloca i32, align 4
  %4 = alloca i32, align 4
  %5 = alloca i32, align 4
  %6 = alloca i32, align 4
  %7 = alloca ptr, align 8
  %8 = alloca %"struct.ns::S", align 8
  %9 = alloca ptr, align 8
  %10 = alloca %class.anon, align 8
  %11 = alloca %struct.Bits, align 8
  %12 = alloca %union.U, align 4
  %13 = alloca %struct.Box, align 4
  %14 = alloca i64, align 8
  %15 = alloca i32, align 4
  %16 = alloca %struct.Derived, align 8
  %17 = alloca [16 x i8], align 1
  %18 = alloca ptr, align 8
  %19 = alloca i32, align 4
  %20 = alloca { double, double }, align 8
  store i32 0, ptr %5, align 4
  store i32 %0, ptr %6, align 4
    #dbg_declare(ptr %6, !141, !DIExpression(), !142)
  store ptr %1, ptr %7, align 8
    #dbg_declare(ptr %7, !143, !DIExpression(), !144)
    #dbg_declare(ptr %8, !145, !DIExpression(), !146)
  call void @_ZN2ns1SIiLi3EEC2Ev(ptr noundef nonnull align 8 dereferenceable(28) %8) #12, !dbg !146
  %21 = load i32, ptr %6, align 4, !dbg !147
  %22 = getelementptr inbounds nuw %"struct.ns::S", ptr %8, i32 0, i32 1, !dbg !148
  %23 = getelementptr inbounds [3 x i32], ptr %22, i64 0, i64 0, !dbg !149
  store i32 %21, ptr %23, align 4, !dbg !150
    #dbg_declare(ptr %9, !151, !DIExpression(), !153)
  store ptr @_Z3sumiz, ptr %9, align 8, !dbg !153
    #dbg_declare(ptr %10, !154, !DIExpression(), !159)
  %24 = getelementptr inbounds nuw %class.anon, ptr %10, i32 0, i32 0, !dbg !160
  store ptr %8, ptr %24, align 8, !dbg !160
    #dbg_declare(ptr %11, !161, !DIExpression(), !168)
  call void @llvm.memcpy.p0.p0.i64(ptr align 8 %11, ptr align 8 @__const.main.bits, i64 8, i1 false), !dbg !168
    #dbg_declare(ptr %12, !169, !DIExpression(), !179)
  store i32 3, ptr %12, align 4, !dbg !180
    #dbg_declare(ptr %13, !181, !DIExpression(), !196)
  call void @llvm.memcpy.p0.p0.i64(ptr align 4 %13, ptr align 4 @__const.main.box, i64 12, i1 false), !dbg !196
    #dbg_declare(ptr %14, !197, !DIExpression(), !198)
  store i64 -1, ptr %14, align 8, !dbg !198
    #dbg_declare(ptr %15, !199, !DIExpression(), !200)
  store i32 -5, ptr %15, align 4, !dbg !200
    #dbg_declare(ptr %16, !201, !DIExpression(), !203)
  call void @_ZN7DerivedC1Ev(ptr noundef nonnull align 8 dereferenceable(8) %16) #12, !dbg !203
    #dbg_declare(ptr %17, !204, !DIExpression(), !208)
  %25 = getelementptr inbounds [16 x i8], ptr %17, i64 0, i64 0, !dbg !209
  call void @llvm.memset.p0.i64(ptr align 1 %25, i8 0, i64 16, i1 false), !dbg !209
  %26 = invoke noundef i32 @"_ZZ4mainENK3$_0clEi"(ptr noundef nonnull align 8 dereferenceable(8) %10, i32 noundef 2)
          to label %27 unwind label %98, !dbg !210

27:                                               ; preds = %2
  %28 = load ptr, ptr %9, align 8, !dbg !211
  %29 = invoke noundef i32 (i32, ...) %28(i32 noundef 2, i32 noundef 3)
          to label %30 unwind label %98, !dbg !211

30:                                               ; preds = %27
  %31 = load double, ptr @_ZL1h, align 8, !dbg !212
  %32 = invoke i32 (ptr, ...) @printf(ptr noundef @.str, i32 noundef %26, i32 noundef %29, double noundef %31)
          to label %33 unwind label %98, !dbg !213

33:                                               ; preds = %30
  %34 = load i32, ptr @_ZN2ns1SIiLi3EE2smE, align 4, !dbg !214
  %35 = call align 4 ptr @llvm.threadlocal.address.p0(ptr align 4 @tl), !dbg !215
  %36 = load i32, ptr %35, align 4, !dbg !215
  %37 = add nsw i32 %34, %36, !dbg !216
  %38 = load i32, ptr @g, align 4, !dbg !217
  %39 = add nsw i32 %37, %38, !dbg !218
  %40 = load i32, ptr %6, align 4, !dbg !219
  store i32 %40, ptr %3, align 4
    #dbg_declare(ptr %3, !220, !DIExpression(), !222)
    #dbg_declare(ptr %4, !224, !DIExpression(), !225)
  %41 = load i32, ptr %3, align 4, !dbg !226
  %42 = add nsw i32 %41, 1, !dbg !227
  store i32 %42, ptr %4, align 4, !dbg !225
  %43 = load i32, ptr %4, align 4, !dbg !228
  %44 = mul nsw i32 %43, 2, !dbg !229
  br label %45, !dbg !230

45:                                               ; preds = %33
  %46 = add nsw i32 %39, %44, !dbg !231
  %47 = load i32, ptr %6, align 4, !dbg !232
  %48 = call noundef i32 @_ZN5outer2v14deepEi(i32 noundef %47), !dbg !233
  %49 = add nsw i32 %46, %48, !dbg !234
  %50 = load i32, ptr %6, align 4, !dbg !235
  %51 = add nsw i32 %50, 2, !dbg !236
  %52 = call noundef i32 @_Z3vlai(i32 noundef %51), !dbg !237
  %53 = add nsw i32 %49, %52, !dbg !238
  %54 = call noundef i32 @_Z7counterv(), !dbg !239
  %55 = add nsw i32 %53, %54, !dbg !240
  %56 = invoke noundef i32 @_ZNK7Derived1vEv(ptr noundef nonnull align 8 dereferenceable(8) %16)
          to label %57 unwind label %98, !dbg !241

57:                                               ; preds = %45
  %58 = add nsw i32 %55, %56, !dbg !242
  %59 = load i64, ptr %11, align 8, !dbg !243
  %60 = and i64 %59, 7, !dbg !243
  %61 = trunc i64 %60 to i32, !dbg !243
  %62 = add nsw i32 %58, %61, !dbg !244
  %63 = getelementptr inbounds [4 x i8], ptr %12, i64 0, i64 0, !dbg !245
  %64 = load i8, ptr %63, align 4, !dbg !245
  %65 = zext i8 %64 to i32, !dbg !245
  %66 = add nsw i32 %62, %65, !dbg !246
  %67 = getelementptr inbounds nuw %struct.Box, ptr %13, i32 0, i32 0, !dbg !247
  %68 = getelementptr inbounds [3 x i32], ptr %67, i64 0, i64 1, !dbg !248
  %69 = load i32, ptr %68, align 4, !dbg !248
  %70 = add nsw i32 %66, %69, !dbg !249
  %71 = load i64, ptr %14, align 8, !dbg !250
  %72 = trunc i64 %71 to i32, !dbg !251
  %73 = add nsw i32 %70, %72, !dbg !252
  %74 = load i32, ptr %15, align 4, !dbg !253
  %75 = add nsw i32 %73, %74, !dbg !254
  %76 = load i32, ptr %6, align 4, !dbg !255
  %77 = sext i32 %76 to i128, !dbg !255
  %78 = call noundef i128 @_Z4widen(i128 noundef %77), !dbg !256
  %79 = trunc i128 %78 to i32, !dbg !256
  %80 = add nsw i32 %75, %79, !dbg !257
  %81 = getelementptr inbounds nuw { double, double }, ptr %20, i32 0, i32 0, !dbg !258
  %82 = getelementptr inbounds nuw { double, double }, ptr %20, i32 0, i32 1, !dbg !258
  store double 1.000000e+00, ptr %81, align 8, !dbg !258
  store double 0.000000e+00, ptr %82, align 8, !dbg !258
  %83 = load [2 x double], ptr %20, align 8, !dbg !258
  %84 = call noundef { double, double } @_Z6squareCd([2 x double] noundef alignstack(8) %83), !dbg !258
  %85 = extractvalue { double, double } %84, 0, !dbg !258
  %86 = extractvalue { double, double } %84, 1, !dbg !258
  %87 = fptosi double %85 to i32, !dbg !259
  %88 = add nsw i32 %80, %87, !dbg !260
  %89 = getelementptr inbounds [16 x i8], ptr %17, i64 0, i64 1, !dbg !261
  %90 = load i8, ptr %89, align 1, !dbg !261
  %91 = zext i8 %90 to i32, !dbg !261
  %92 = add nsw i32 %88, %91, !dbg !262
  %93 = load ptr, ptr %7, align 8, !dbg !263
  %94 = icmp ne ptr %93, null, !dbg !264
  %95 = zext i1 %94 to i32, !dbg !265
  %96 = add nsw i32 %92, %95, !dbg !266
  store i32 %96, ptr %5, align 4, !dbg !267
  call void @_ZN7DerivedD1Ev(ptr noundef nonnull align 8 dereferenceable(8) %16) #12, !dbg !268
  call void @_ZN2ns1SIiLi3EED2Ev(ptr noundef nonnull align 8 dereferenceable(28) %8) #12, !dbg !268
  %97 = load i32, ptr %5, align 4, !dbg !268
  ret i32 %97, !dbg !268

98:                                               ; preds = %45, %30, %27, %2
  %99 = landingpad { ptr, i32 }
          cleanup, !dbg !268
  %100 = extractvalue { ptr, i32 } %99, 0, !dbg !268
  store ptr %100, ptr %18, align 8, !dbg !268
  %101 = extractvalue { ptr, i32 } %99, 1, !dbg !268
  store i32 %101, ptr %19, align 4, !dbg !268
  call void @_ZN7DerivedD1Ev(ptr noundef nonnull align 8 dereferenceable(8) %16) #12, !dbg !268
  call void @_ZN2ns1SIiLi3EED2Ev(ptr noundef nonnull align 8 dereferenceable(28) %8) #12, !dbg !268
  br label %102, !dbg !268

102:                                              ; preds = %98
  %103 = load ptr, ptr %18, align 8, !dbg !268
  %104 = load i32, ptr %19, align 4, !dbg !268
  %105 = insertvalue { ptr, i32 } poison, ptr %103, 0, !dbg !268
  %106 = insertvalue { ptr, i32 } %105, i32 %104, 1, !dbg !268
  resume { ptr, i32 } %106, !dbg !268
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZN2ns1SIiLi3EEC2Ev(ptr noundef nonnull align 8 dereferenceable(28) %0) unnamed_addr #0 comdat !dbg !269 {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
    #dbg_declare(ptr %2, !274, !DIExpression(), !276)
  %3 = load ptr, ptr %2, align 8
  call void @_ZN2ns1BC2Ev(ptr noundef nonnull align 8 dereferenceable(12) %3) #12, !dbg !277
  store ptr getelementptr inbounds inrange(-16, 16) ({ [4 x ptr] }, ptr @_ZTVN2ns1SIiLi3EEE, i32 0, i32 0, i32 2), ptr %3, align 8, !dbg !277
  ret void, !dbg !277
}

; Function Attrs: nocallback nofree nounwind willreturn memory(argmem: readwrite)
declare void @llvm.memcpy.p0.p0.i64(ptr noalias writeonly captures(none), ptr noalias readonly captures(none), i64, i1 immarg) #3

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZN7DerivedC1Ev(ptr noundef nonnull align 8 dereferenceable(8) %0) unnamed_addr #0 comdat !dbg !278 {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
    #dbg_declare(ptr %2, !283, !DIExpression(), !285)
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds i8, ptr %3, i64 8, !dbg !286
  call void @_ZN4BaseC2Ev(ptr noundef nonnull align 8 dereferenceable(12) %4) #12, !dbg !286
  store ptr getelementptr inbounds inrange(-24, 24) ({ [6 x ptr], [7 x ptr] }, ptr @_ZTV7Derived, i32 0, i32 0, i32 3), ptr %3, align 8, !dbg !286
  %5 = getelementptr inbounds i8, ptr %3, i64 8, !dbg !286
  store ptr getelementptr inbounds inrange(-32, 24) ({ [6 x ptr], [7 x ptr] }, ptr @_ZTV7Derived, i32 0, i32 1, i32 4), ptr %5, align 8, !dbg !286
  ret void, !dbg !286
}

; Function Attrs: nocallback nofree nounwind willreturn memory(argmem: write)
declare void @llvm.memset.p0.i64(ptr writeonly captures(none), i8, i64, i1 immarg) #4

declare i32 @printf(ptr noundef, ...) #5

; Function Attrs: mustprogress noinline optnone uwtable
define internal noundef i32 @"_ZZ4mainENK3$_0clEi"(ptr noundef nonnull align 8 dereferenceable(8) %0, i32 noundef %1) #6 !dbg !287 {
  %3 = alloca ptr, align 8
  %4 = alloca i32, align 4
  store ptr %0, ptr %3, align 8
    #dbg_declare(ptr %3, !293, !DIExpression(), !295)
  store i32 %1, ptr %4, align 4
    #dbg_declare(ptr %4, !296, !DIExpression(), !297)
  %5 = load ptr, ptr %3, align 8
  %6 = load i32, ptr %4, align 4, !dbg !298
  %7 = getelementptr inbounds nuw %class.anon, ptr %5, i32 0, i32 0, !dbg !299
  %8 = load ptr, ptr %7, align 8, !dbg !299, !nonnull !45, !align !300
  %9 = call noundef i32 @_ZNK2ns1SIiLi3EE1fEi(ptr noundef nonnull align 8 dereferenceable(28) %8, i32 noundef 1), !dbg !301
  %10 = add nsw i32 %6, %9, !dbg !302
  ret i32 %10, !dbg !303
}

declare i32 @__gxx_personality_v0(...)

; Function Attrs: nocallback nofree nosync nounwind speculatable willreturn memory(none)
declare nonnull ptr @llvm.threadlocal.address.p0(ptr nonnull) #7

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local noundef i32 @_ZNK7Derived1vEv(ptr noundef nonnull align 8 dereferenceable(8) %0) unnamed_addr #0 comdat !dbg !304 {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
    #dbg_declare(ptr %2, !310, !DIExpression(), !312)
  %3 = load ptr, ptr %2, align 8
  ret i32 2, !dbg !313
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZN7DerivedD1Ev(ptr noundef nonnull align 8 dereferenceable(8) %0) unnamed_addr #0 comdat !dbg !314 {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
    #dbg_declare(ptr %2, !316, !DIExpression(), !317)
  %3 = load ptr, ptr %2, align 8
  call void @_ZN7DerivedD2Ev(ptr noundef nonnull align 8 dereferenceable(8) %3, ptr noundef @_ZTT7Derived) #12, !dbg !318
  %4 = getelementptr inbounds i8, ptr %3, i64 8, !dbg !318
  call void @_ZN4BaseD2Ev(ptr noundef nonnull align 8 dereferenceable(12) %4) #12, !dbg !318
  ret void, !dbg !318
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZN2ns1SIiLi3EED2Ev(ptr noundef nonnull align 8 dereferenceable(28) %0) unnamed_addr #0 comdat !dbg !319 {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
    #dbg_declare(ptr %2, !321, !DIExpression(), !322)
  %3 = load ptr, ptr %2, align 8
  call void @_ZN2ns1BD2Ev(ptr noundef nonnull align 8 dereferenceable(12) %3) #12, !dbg !323
  ret void, !dbg !325
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZN2ns1BC2Ev(ptr noundef nonnull align 8 dereferenceable(12) %0) unnamed_addr #0 comdat !dbg !326 {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
    #dbg_declare(ptr %2, !332, !DIExpression(), !334)
  %3 = load ptr, ptr %2, align 8
  store ptr getelementptr inbounds inrange(-16, 16) ({ [4 x ptr] }, ptr @_ZTVN2ns1BE, i32 0, i32 0, i32 2), ptr %3, align 8, !dbg !335
  ret void, !dbg !335
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZN2ns1SIiLi3EED0Ev(ptr noundef nonnull align 8 dereferenceable(28) %0) unnamed_addr #0 comdat !dbg !336 {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
    #dbg_declare(ptr %2, !337, !DIExpression(), !338)
  %3 = load ptr, ptr %2, align 8
  call void @_ZN2ns1SIiLi3EED2Ev(ptr noundef nonnull align 8 dereferenceable(28) %3) #12, !dbg !339
  call void @_ZdlPvm(ptr noundef %3, i64 noundef 32) #13, !dbg !339
  ret void, !dbg !339
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZN2ns1BD2Ev(ptr noundef nonnull align 8 dereferenceable(12) %0) unnamed_addr #0 comdat !dbg !340 {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
    #dbg_declare(ptr %2, !342, !DIExpression(), !343)
  %3 = load ptr, ptr %2, align 8
  ret void, !dbg !344
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZN2ns1BD0Ev(ptr noundef nonnull align 8 dereferenceable(12) %0) unnamed_addr #0 comdat !dbg !345 {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
    #dbg_declare(ptr %2, !346, !DIExpression(), !347)
  %3 = load ptr, ptr %2, align 8
  call void @_ZN2ns1BD2Ev(ptr noundef nonnull align 8 dereferenceable(12) %3) #12, !dbg !348
  call void @_ZdlPvm(ptr noundef %3, i64 noundef 16) #13, !dbg !348
  ret void, !dbg !349
}

; Function Attrs: nobuiltin nounwind
declare void @_ZdlPvm(ptr noundef, i64 noundef) #8

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZN4BaseC2Ev(ptr noundef nonnull align 8 dereferenceable(12) %0) unnamed_addr #0 comdat !dbg !350 {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
    #dbg_declare(ptr %2, !356, !DIExpression(), !358)
  %3 = load ptr, ptr %2, align 8
  store ptr getelementptr inbounds inrange(-16, 24) ({ [5 x ptr] }, ptr @_ZTV4Base, i32 0, i32 0, i32 2), ptr %3, align 8, !dbg !359
  ret void, !dbg !359
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZN7DerivedD0Ev(ptr noundef nonnull align 8 dereferenceable(8) %0) unnamed_addr #0 comdat !dbg !360 {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
    #dbg_declare(ptr %2, !361, !DIExpression(), !362)
  %3 = load ptr, ptr %2, align 8
  call void @_ZN7DerivedD1Ev(ptr noundef nonnull align 8 dereferenceable(8) %3) #12, !dbg !363
  call void @_ZdlPvm(ptr noundef %3, i64 noundef 24) #13, !dbg !363
  ret void, !dbg !363
}

; Function Attrs: noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZTv0_n24_N7DerivedD1Ev(ptr noundef %0) unnamed_addr #9 comdat !dbg !364 {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = load ptr, ptr %3, align 8, !dbg !366
  %5 = getelementptr inbounds i8, ptr %4, i64 -24, !dbg !366
  %6 = load i64, ptr %5, align 8, !dbg !366
  %7 = getelementptr inbounds i8, ptr %3, i64 %6, !dbg !366
  tail call void @_ZN7DerivedD1Ev(ptr noundef nonnull align 8 dereferenceable(8) %7) #12, !dbg !366
  ret void, !dbg !367
}

; Function Attrs: noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZTv0_n24_N7DerivedD0Ev(ptr noundef %0) unnamed_addr #9 comdat !dbg !368 {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = load ptr, ptr %3, align 8, !dbg !369
  %5 = getelementptr inbounds i8, ptr %4, i64 -24, !dbg !369
  %6 = load i64, ptr %5, align 8, !dbg !369
  %7 = getelementptr inbounds i8, ptr %3, i64 %6, !dbg !369
  tail call void @_ZN7DerivedD0Ev(ptr noundef nonnull align 8 dereferenceable(8) %7) #12, !dbg !369
  ret void, !dbg !370
}

; Function Attrs: noinline optnone uwtable
define linkonce_odr dso_local noundef i32 @_ZTv0_n32_NK7Derived1vEv(ptr noundef %0) unnamed_addr #10 comdat !dbg !371 {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = load ptr, ptr %3, align 8, !dbg !372
  %5 = getelementptr inbounds i8, ptr %4, i64 -32, !dbg !372
  %6 = load i64, ptr %5, align 8, !dbg !372
  %7 = getelementptr inbounds i8, ptr %3, i64 %6, !dbg !372
  %8 = tail call noundef i32 @_ZNK7Derived1vEv(ptr noundef nonnull align 8 dereferenceable(8) %7), !dbg !372
  ret i32 %8, !dbg !372
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZN4BaseD2Ev(ptr noundef nonnull align 8 dereferenceable(12) %0) unnamed_addr #0 comdat !dbg !373 {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
    #dbg_declare(ptr %2, !375, !DIExpression(), !376)
  %3 = load ptr, ptr %2, align 8
  ret void, !dbg !377
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZN4BaseD0Ev(ptr noundef nonnull align 8 dereferenceable(12) %0) unnamed_addr #0 comdat !dbg !378 {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
    #dbg_declare(ptr %2, !379, !DIExpression(), !380)
  %3 = load ptr, ptr %2, align 8
  call void @_ZN4BaseD2Ev(ptr noundef nonnull align 8 dereferenceable(12) %3) #12, !dbg !381
  call void @_ZdlPvm(ptr noundef %3, i64 noundef 16) #13, !dbg !381
  ret void, !dbg !381
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local noundef i32 @_ZNK4Base1vEv(ptr noundef nonnull align 8 dereferenceable(12) %0) unnamed_addr #0 comdat !dbg !382 {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
    #dbg_declare(ptr %2, !388, !DIExpression(), !390)
  %3 = load ptr, ptr %2, align 8
  ret i32 1, !dbg !391
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local noundef i32 @_ZNK2ns1SIiLi3EE1fEi(ptr noundef nonnull align 8 dereferenceable(28) %0, i32 noundef %1) #0 comdat !dbg !392 {
  %3 = alloca ptr, align 8
  %4 = alloca i32, align 4
  %5 = alloca i32, align 4
  store ptr %0, ptr %3, align 8
    #dbg_declare(ptr %3, !398, !DIExpression(), !400)
  store i32 %1, ptr %4, align 4
    #dbg_declare(ptr %4, !401, !DIExpression(), !402)
  %6 = load ptr, ptr %3, align 8
    #dbg_declare(ptr %5, !403, !DIExpression(), !404)
  %7 = load i32, ptr %4, align 4, !dbg !405
  %8 = getelementptr inbounds nuw %"struct.ns::S", ptr %6, i32 0, i32 1, !dbg !406
  %9 = getelementptr inbounds [3 x i32], ptr %8, i64 0, i64 0, !dbg !406
  %10 = load i32, ptr %9, align 4, !dbg !406
  %11 = add nsw i32 %7, %10, !dbg !407
  store i32 %11, ptr %5, align 4, !dbg !404
  %12 = load i32, ptr %5, align 4, !dbg !408
  ret i32 %12, !dbg !409
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZN7DerivedD2Ev(ptr noundef nonnull align 8 dereferenceable(8) %0, ptr noundef %1) unnamed_addr #0 comdat !dbg !410 {
  %3 = alloca ptr, align 8
  %4 = alloca ptr, align 8
  store ptr %0, ptr %3, align 8
    #dbg_declare(ptr %3, !411, !DIExpression(), !412)
  store ptr %1, ptr %4, align 8
    #dbg_declare(ptr %4, !413, !DIExpression(), !412)
  %5 = load ptr, ptr %3, align 8
  %6 = load ptr, ptr %4, align 8
  ret void, !dbg !416
}

; Function Attrs: noinline uwtable
define weak_odr hidden noundef ptr @_ZTW2tl() #11 comdat {
  %1 = call align 4 ptr @llvm.threadlocal.address.p0(ptr align 4 @tl)
  ret ptr %1
}

attributes #0 = { mustprogress noinline nounwind optnone uwtable "frame-pointer"="non-leaf-no-reserve" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #1 = { nocallback nofree nosync nounwind willreturn }
attributes #2 = { mustprogress noinline norecurse optnone uwtable "frame-pointer"="non-leaf-no-reserve" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #3 = { nocallback nofree nounwind willreturn memory(argmem: readwrite) }
attributes #4 = { nocallback nofree nounwind willreturn memory(argmem: write) }
attributes #5 = { "frame-pointer"="non-leaf-no-reserve" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #6 = { mustprogress noinline optnone uwtable "frame-pointer"="non-leaf-no-reserve" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #7 = { nocallback nofree nosync nounwind speculatable willreturn memory(none) }
attributes #8 = { nobuiltin nounwind "frame-pointer"="non-leaf-no-reserve" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #9 = { noinline nounwind optnone uwtable "frame-pointer"="non-leaf-no-reserve" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #10 = { noinline optnone uwtable "frame-pointer"="non-leaf-no-reserve" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #11 = { noinline uwtable "frame-pointer"="non-leaf-no-reserve" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #12 = { nounwind }
attributes #13 = { builtin nounwind }

!llvm.dbg.cu = !{!2}
!llvm.module.flags = !{!32, !33, !34, !35, !36, !37, !38}
!llvm.ident = !{!39}

!0 = !DIGlobalVariableExpression(var: !1, expr: !DIExpression())
!1 = distinct !DIGlobalVariable(name: "g", scope: !2, file: !3, line: 30, type: !12, isLocal: false, isDefinition: true)
!2 = distinct !DICompileUnit(language: DW_LANG_C_plus_plus_14, file: !3, producer: "kiln version 1.0.1", isOptimized: false, runtimeVersion: 0, emissionKind: FullDebug, enums: !4, retainedTypes: !16, globals: !17, splitDebugInlining: false, nameTableKind: None)
!3 = !DIFile(filename: "debug_types.cpp", directory: "/src", checksumkind: CSK_MD5, checksum: "5ebbb75793b5cb00615def0390939301")
!4 = !{!5, !11}
!5 = distinct !DICompositeType(tag: DW_TAG_enumeration_type, name: "Wide", file: !3, line: 20, baseType: !6, size: 64, flags: DIFlagEnumClass, elements: !8, identifier: "_ZTS4Wide")
!6 = !DIDerivedType(tag: DW_TAG_typedef, name: "uint64_t", file: !3, line: 4, baseType: !7)
!7 = !DIBasicType(name: "unsigned long", size: 64, encoding: DW_ATE_unsigned)
!8 = !{!9, !10}
!9 = !DIEnumerator(name: "Zero", value: 0, isUnsigned: true)
!10 = !DIEnumerator(name: "Max", value: 18446744073709551615, isUnsigned: true)
!11 = distinct !DICompositeType(tag: DW_TAG_enumeration_type, name: "Signed", file: !3, line: 21, baseType: !12, size: 32, elements: !13, identifier: "_ZTS6Signed")
!12 = !DIBasicType(name: "int", size: 32, encoding: DW_ATE_signed)
!13 = !{!14, !15}
!14 = !DIEnumerator(name: "Neg", value: -5)
!15 = !DIEnumerator(name: "Pos", value: 7)
!16 = !{!12}
!17 = !{!0, !18, !20, !24, !27}
!18 = !DIGlobalVariableExpression(var: !19, expr: !DIExpression())
!19 = distinct !DIGlobalVariable(name: "tl", scope: !2, file: !3, line: 32, type: !12, isLocal: false, isDefinition: true)
!20 = !DIGlobalVariableExpression(var: !21, expr: !DIExpression())
!21 = distinct !DIGlobalVariable(name: "count", scope: !22, file: !3, line: 43, type: !12, isLocal: true, isDefinition: true)
!22 = distinct !DISubprogram(name: "counter", linkageName: "_Z7counterv", scope: !3, file: !3, line: 43, type: !23, scopeLine: 43, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !2)
!23 = !DISubroutineType(types: !16)
!24 = !DIGlobalVariableExpression(var: !25, expr: !DIExpression())
!25 = distinct !DIGlobalVariable(name: "h", linkageName: "_ZL1h", scope: !2, file: !3, line: 31, type: !26, isLocal: true, isDefinition: true)
!26 = !DIBasicType(name: "double", size: 64, encoding: DW_ATE_float)
!27 = !DIGlobalVariableExpression(var: !28, expr: !DIExpression())
!28 = distinct !DIGlobalVariable(name: "sm", linkageName: "_ZN2ns1SIiLi3EE2smE", scope: !29, file: !3, line: 16, type: !12, isLocal: false, isDefinition: true, declaration: !30)
!29 = !DINamespace(name: "ns", scope: null)
!30 = !DIDerivedType(tag: DW_TAG_variable, name: "sm", scope: !31, file: !3, line: 11, baseType: !12, flags: DIFlagStaticMember)
!31 = distinct !DICompositeType(tag: DW_TAG_structure_type, name: "S<int, 3>", scope: !29, file: !3, line: 9, size: 256, flags: DIFlagFwdDecl | DIFlagNonTrivial, identifier: "_ZTSN2ns1SIiLi3EEE")
!32 = !{i32 7, !"Dwarf Version", i32 5}
!33 = !{i32 2, !"Debug Info Version", i32 3}
!34 = !{i32 1, !"wchar_size", i32 4}
!35 = !{i32 8, !"PIC Level", i32 2}
!36 = !{i32 7, !"PIE Level", i32 2}
!37 = !{i32 7, !"uwtable", i32 2}
!38 = !{i32 7, !"frame-pointer", i32 4}
!39 = !{!"kiln version 1.0.1"}
!40 = distinct !DISubprogram(name: "deep", linkageName: "_ZN5outer2v14deepEi", scope: !41, file: !3, line: 25, type: !43, scopeLine: 25, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !2, retainedNodes: !45)
!41 = !DINamespace(name: "v1", scope: !42, exportSymbols: true)
!42 = !DINamespace(name: "outer", scope: null)
!43 = !DISubroutineType(types: !44)
!44 = !{!12, !12}
!45 = !{}
!46 = !DILocalVariable(name: "x", arg: 1, scope: !40, file: !3, line: 25, type: !12)
!47 = !DILocation(line: 25, column: 54, scope: !40)
!48 = !DILocation(line: 25, column: 66, scope: !40)
!49 = !DILocation(line: 25, column: 68, scope: !40)
!50 = !DILocation(line: 25, column: 59, scope: !40)
!51 = distinct !DISubprogram(name: "vla", linkageName: "_Z3vlai", scope: !3, file: !3, line: 35, type: !43, scopeLine: 35, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !2, retainedNodes: !45)
!52 = !DILocalVariable(name: "n", arg: 1, scope: !51, file: !3, line: 35, type: !12)
!53 = !DILocation(line: 35, column: 13, scope: !51)
!54 = !DILocation(line: 35, column: 26, scope: !51)
!55 = !DILocation(line: 35, column: 18, scope: !51)
!56 = !DILocalVariable(name: "i", scope: !57, file: !3, line: 35, type: !12)
!57 = distinct !DILexicalBlock(scope: !51, file: !3, line: 35, column: 30)
!58 = !DILocation(line: 35, column: 39, scope: !57)
!59 = !DILocation(line: 35, column: 35, scope: !57)
!60 = !DILocation(line: 35, column: 46, scope: !61)
!61 = distinct !DILexicalBlock(scope: !57, file: !3, line: 35, column: 30)
!62 = !DILocation(line: 35, column: 50, scope: !61)
!63 = !DILocation(line: 35, column: 48, scope: !61)
!64 = !DILocation(line: 35, column: 30, scope: !57)
!65 = !DILocation(line: 35, column: 67, scope: !61)
!66 = !DILocation(line: 35, column: 62, scope: !61)
!67 = !DILocation(line: 35, column: 58, scope: !61)
!68 = !DILocation(line: 35, column: 65, scope: !61)
!69 = !DILocation(line: 35, column: 53, scope: !61)
!70 = !DILocation(line: 35, column: 30, scope: !61)
!71 = distinct !{!71, !64, !72, !73}
!72 = !DILocation(line: 35, column: 67, scope: !57)
!73 = !{!"llvm.loop.mustprogress"}
!74 = !DILocation(line: 35, column: 81, scope: !51)
!75 = !DILocation(line: 35, column: 83, scope: !51)
!76 = !DILocation(line: 35, column: 77, scope: !51)
!77 = !DILocation(line: 35, column: 89, scope: !51)
!78 = distinct !DISubprogram(name: "sum", linkageName: "_Z3sumiz", scope: !3, file: !3, line: 36, type: !79, scopeLine: 36, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !2, retainedNodes: !45)
!79 = !DISubroutineType(types: !80)
!80 = !{!12, !12, null}
!81 = !DILocalVariable(name: "n", arg: 1, scope: !78, file: !3, line: 36, type: !12)
!82 = !DILocation(line: 36, column: 13, scope: !78)
!83 = !DILocalVariable(name: "s", scope: !78, file: !3, line: 37, type: !12)
!84 = !DILocation(line: 37, column: 7, scope: !78)
!85 = !DILocalVariable(name: "i", scope: !86, file: !3, line: 38, type: !12)
!86 = distinct !DILexicalBlock(scope: !78, file: !3, line: 38, column: 3)
!87 = !DILocation(line: 38, column: 12, scope: !86)
!88 = !DILocation(line: 38, column: 8, scope: !86)
!89 = !DILocation(line: 38, column: 19, scope: !90)
!90 = distinct !DILexicalBlock(scope: !86, file: !3, line: 38, column: 3)
!91 = !DILocation(line: 38, column: 23, scope: !90)
!92 = !DILocation(line: 38, column: 21, scope: !90)
!93 = !DILocation(line: 38, column: 3, scope: !86)
!94 = !DILocalVariable(name: "k", scope: !95, file: !3, line: 38, type: !12)
!95 = distinct !DILexicalBlock(scope: !90, file: !3, line: 38, column: 31)
!96 = !DILocation(line: 38, column: 37, scope: !95)
!97 = !DILocation(line: 38, column: 41, scope: !95)
!98 = !DILocation(line: 38, column: 43, scope: !95)
!99 = !DILocation(line: 38, column: 53, scope: !95)
!100 = !DILocation(line: 38, column: 50, scope: !95)
!101 = !DILocation(line: 38, column: 56, scope: !95)
!102 = !DILocation(line: 38, column: 26, scope: !90)
!103 = !DILocation(line: 38, column: 3, scope: !90)
!104 = distinct !{!104, !93, !105, !73}
!105 = !DILocation(line: 38, column: 56, scope: !86)
!106 = !DILocation(line: 40, column: 7, scope: !107)
!107 = distinct !DILexicalBlock(scope: !78, file: !3, line: 40, column: 7)
!108 = !DILocation(line: 40, column: 9, scope: !107)
!109 = !DILocation(line: 40, column: 7, scope: !78)
!110 = !DILocation(line: 40, column: 14, scope: !107)
!111 = !DILocation(line: 41, column: 10, scope: !78)
!112 = !DILocation(line: 41, column: 3, scope: !78)
!113 = !DILocation(line: 43, column: 46, scope: !22)
!114 = !DILocation(line: 43, column: 39, scope: !22)
!115 = distinct !DISubprogram(name: "wide", linkageName: "_Z4widen", scope: !3, file: !3, line: 44, type: !116, scopeLine: 44, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !2, retainedNodes: !45)
!116 = !DISubroutineType(types: !117)
!117 = !{!118, !118}
!118 = !DIBasicType(name: "__int128", size: 128, encoding: DW_ATE_signed)
!119 = !DILocalVariable(name: "a", arg: 1, scope: !115, file: !3, line: 44, type: !118)
!120 = !DILocation(line: 44, column: 24, scope: !115)
!121 = !DILocation(line: 44, column: 36, scope: !115)
!122 = !DILocation(line: 44, column: 38, scope: !115)
!123 = !DILocation(line: 44, column: 29, scope: !115)
!124 = distinct !DISubprogram(name: "square", linkageName: "_Z6squareCd", scope: !3, file: !3, line: 45, type: !125, scopeLine: 45, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !2, retainedNodes: !45)
!125 = !DISubroutineType(types: !126)
!126 = !{!127, !127}
!127 = !DIBasicType(name: "complex", size: 128, encoding: DW_ATE_complex_float)
!128 = !DILocalVariable(name: "z", arg: 1, scope: !124, file: !3, line: 45, type: !127)
!129 = !DILocation(line: 45, column: 40, scope: !124)
!130 = !DILocation(line: 45, column: 52, scope: !124)
!131 = !DILocation(line: 45, column: 56, scope: !124)
!132 = !DILocation(line: 45, column: 54, scope: !124)
!133 = !{!"branch_weights", i32 1, i32 1048575}
!134 = !DILocation(line: 45, column: 45, scope: !124)
!135 = distinct !DISubprogram(name: "main", scope: !3, file: !3, line: 47, type: !136, scopeLine: 47, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !2, retainedNodes: !45)
!136 = !DISubroutineType(types: !137)
!137 = !{!12, !12, !138}
!138 = !DIDerivedType(tag: DW_TAG_pointer_type, baseType: !139, size: 64)
!139 = !DIDerivedType(tag: DW_TAG_pointer_type, baseType: !140, size: 64)
!140 = !DIBasicType(name: "char", size: 8, encoding: DW_ATE_unsigned_char)
!141 = !DILocalVariable(name: "argc", arg: 1, scope: !135, file: !3, line: 47, type: !12)
!142 = !DILocation(line: 47, column: 14, scope: !135)
!143 = !DILocalVariable(name: "argv", arg: 2, scope: !135, file: !3, line: 47, type: !138)
!144 = !DILocation(line: 47, column: 27, scope: !135)
!145 = !DILocalVariable(name: "s", scope: !135, file: !3, line: 48, type: !31)
!146 = !DILocation(line: 48, column: 13, scope: !135)
!147 = !DILocation(line: 49, column: 12, scope: !135)
!148 = !DILocation(line: 49, column: 5, scope: !135)
!149 = !DILocation(line: 49, column: 3, scope: !135)
!150 = !DILocation(line: 49, column: 10, scope: !135)
!151 = !DILocalVariable(name: "fp", scope: !135, file: !3, line: 50, type: !152)
!152 = !DIDerivedType(tag: DW_TAG_pointer_type, baseType: !79, size: 64)
!153 = !DILocation(line: 50, column: 9, scope: !135)
!154 = !DILocalVariable(name: "lam", scope: !135, file: !3, line: 51, type: !155)
!155 = distinct !DICompositeType(tag: DW_TAG_class_type, scope: !135, file: !3, line: 51, size: 64, flags: DIFlagTypePassByValue | DIFlagNonTrivial, elements: !156)
!156 = !{!157}
!157 = !DIDerivedType(tag: DW_TAG_member, name: "s", scope: !155, file: !3, line: 51, baseType: !158, size: 64)
!158 = !DIDerivedType(tag: DW_TAG_reference_type, baseType: !31, size: 64)
!159 = !DILocation(line: 51, column: 8, scope: !135)
!160 = !DILocation(line: 51, column: 14, scope: !135)
!161 = !DILocalVariable(name: "bits", scope: !135, file: !3, line: 52, type: !162)
!162 = distinct !DICompositeType(tag: DW_TAG_structure_type, name: "Bits", file: !3, line: 22, size: 64, flags: DIFlagTypePassByValue, elements: !163, identifier: "_ZTS4Bits")
!163 = !{!164, !166, !167}
!164 = !DIDerivedType(tag: DW_TAG_member, name: "a", scope: !162, file: !3, line: 22, baseType: !165, size: 3, flags: DIFlagBitField, extraData: i64 0)
!165 = !DIBasicType(name: "unsigned int", size: 32, encoding: DW_ATE_unsigned)
!166 = !DIDerivedType(tag: DW_TAG_member, name: "b", scope: !162, file: !3, line: 22, baseType: !12, size: 5, offset: 3, flags: DIFlagBitField, extraData: i64 0)
!167 = !DIDerivedType(tag: DW_TAG_member, name: "c", scope: !162, file: !3, line: 22, baseType: !6, size: 40, offset: 8, flags: DIFlagBitField, extraData: i64 0)
!168 = !DILocation(line: 52, column: 8, scope: !135)
!169 = !DILocalVariable(name: "u", scope: !135, file: !3, line: 53, type: !170)
!170 = distinct !DICompositeType(tag: DW_TAG_union_type, name: "U", file: !3, line: 23, size: 32, flags: DIFlagTypePassByValue, elements: !171, identifier: "_ZTS1U")
!171 = !{!172, !173, !175}
!172 = !DIDerivedType(tag: DW_TAG_member, name: "i", scope: !170, file: !3, line: 23, baseType: !12, size: 32)
!173 = !DIDerivedType(tag: DW_TAG_member, name: "f", scope: !170, file: !3, line: 23, baseType: !174, size: 32)
!174 = !DIBasicType(name: "float", size: 32, encoding: DW_ATE_float)
!175 = !DIDerivedType(tag: DW_TAG_member, name: "c", scope: !170, file: !3, line: 23, baseType: !176, size: 32)
!176 = !DICompositeType(tag: DW_TAG_array_type, baseType: !140, size: 32, elements: !177)
!177 = !{!178}
!178 = !DISubrange(count: 4)
!179 = !DILocation(line: 53, column: 5, scope: !135)
!180 = !DILocation(line: 54, column: 7, scope: !135)
!181 = !DILocalVariable(name: "box", scope: !135, file: !3, line: 55, type: !182)
!182 = distinct !DICompositeType(tag: DW_TAG_structure_type, name: "Box<int, 3, char, long>", file: !3, line: 24, size: 96, flags: DIFlagTypePassByValue, elements: !183, templateParams: !188, identifier: "_ZTS3BoxIiLi3EJclEE")
!183 = !{!184}
!184 = !DIDerivedType(tag: DW_TAG_member, name: "items", scope: !182, file: !3, line: 24, baseType: !185, size: 96)
!185 = !DICompositeType(tag: DW_TAG_array_type, baseType: !12, size: 96, elements: !186)
!186 = !{!187}
!187 = !DISubrange(count: 3)
!188 = !{!189, !190, !191}
!189 = !DITemplateTypeParameter(name: "T", type: !12)
!190 = !DITemplateValueParameter(name: "N", type: !12, value: i32 3)
!191 = !DITemplateValueParameter(tag: DW_TAG_GNU_template_parameter_pack, name: "Ts", value: !192)
!192 = !{!193, !194}
!193 = !DITemplateTypeParameter(type: !140)
!194 = !DITemplateTypeParameter(type: !195)
!195 = !DIBasicType(name: "long", size: 64, encoding: DW_ATE_signed)
!196 = !DILocation(line: 55, column: 27, scope: !135)
!197 = !DILocalVariable(name: "w", scope: !135, file: !3, line: 56, type: !5)
!198 = !DILocation(line: 56, column: 8, scope: !135)
!199 = !DILocalVariable(name: "e", scope: !135, file: !3, line: 57, type: !11)
!200 = !DILocation(line: 57, column: 10, scope: !135)
!201 = !DILocalVariable(name: "d", scope: !135, file: !3, line: 58, type: !202)
!202 = distinct !DICompositeType(tag: DW_TAG_structure_type, name: "Derived", file: !3, line: 28, size: 192, flags: DIFlagFwdDecl | DIFlagNonTrivial, identifier: "_ZTS7Derived")
!203 = !DILocation(line: 58, column: 11, scope: !135)
!204 = !DILocalVariable(name: "buf", scope: !135, file: !3, line: 59, type: !205)
!205 = !DICompositeType(tag: DW_TAG_array_type, baseType: !140, size: 128, elements: !206)
!206 = !{!207}
!207 = !DISubrange(count: 16)
!208 = !DILocation(line: 59, column: 8, scope: !135)
!209 = !DILocation(line: 60, column: 3, scope: !135)
!210 = !DILocation(line: 61, column: 24, scope: !135)
!211 = !DILocation(line: 61, column: 32, scope: !135)
!212 = !DILocation(line: 61, column: 42, scope: !135)
!213 = !DILocation(line: 61, column: 3, scope: !135)
!214 = !DILocation(line: 62, column: 10, scope: !135)
!215 = !DILocation(line: 62, column: 26, scope: !135)
!216 = !DILocation(line: 62, column: 24, scope: !135)
!217 = !DILocation(line: 62, column: 31, scope: !135)
!218 = !DILocation(line: 62, column: 29, scope: !135)
!219 = !DILocation(line: 62, column: 39, scope: !135)
!220 = !DILocalVariable(name: "a", arg: 1, scope: !221, file: !3, line: 34, type: !12)
!221 = distinct !DISubprogram(name: "inl", linkageName: "_ZL3inli", scope: !3, file: !3, line: 34, type: !43, scopeLine: 34, flags: DIFlagPrototyped, spFlags: DISPFlagLocalToUnit | DISPFlagDefinition, unit: !2, retainedNodes: !45)
!222 = !DILocation(line: 34, column: 58, scope: !221, inlinedAt: !223)
!223 = distinct !DILocation(line: 62, column: 35, scope: !135)
!224 = !DILocalVariable(name: "t", scope: !221, file: !3, line: 34, type: !12)
!225 = !DILocation(line: 34, column: 67, scope: !221, inlinedAt: !223)
!226 = !DILocation(line: 34, column: 71, scope: !221, inlinedAt: !223)
!227 = !DILocation(line: 34, column: 73, scope: !221, inlinedAt: !223)
!228 = !DILocation(line: 34, column: 85, scope: !221, inlinedAt: !223)
!229 = !DILocation(line: 34, column: 87, scope: !221, inlinedAt: !223)
!230 = !DILocation(line: 34, column: 78, scope: !221, inlinedAt: !223)
!231 = !DILocation(line: 62, column: 33, scope: !135)
!232 = !DILocation(line: 62, column: 52, scope: !135)
!233 = !DILocation(line: 62, column: 47, scope: !135)
!234 = !DILocation(line: 62, column: 45, scope: !135)
!235 = !DILocation(line: 62, column: 64, scope: !135)
!236 = !DILocation(line: 62, column: 69, scope: !135)
!237 = !DILocation(line: 62, column: 60, scope: !135)
!238 = !DILocation(line: 62, column: 58, scope: !135)
!239 = !DILocation(line: 62, column: 76, scope: !135)
!240 = !DILocation(line: 62, column: 74, scope: !135)
!241 = !DILocation(line: 62, column: 90, scope: !135)
!242 = !DILocation(line: 62, column: 86, scope: !135)
!243 = !DILocation(line: 63, column: 15, scope: !135)
!244 = !DILocation(line: 62, column: 94, scope: !135)
!245 = !DILocation(line: 63, column: 19, scope: !135)
!246 = !DILocation(line: 63, column: 17, scope: !135)
!247 = !DILocation(line: 63, column: 32, scope: !135)
!248 = !DILocation(line: 63, column: 28, scope: !135)
!249 = !DILocation(line: 63, column: 26, scope: !135)
!250 = !DILocation(line: 63, column: 48, scope: !135)
!251 = !DILocation(line: 63, column: 43, scope: !135)
!252 = !DILocation(line: 63, column: 41, scope: !135)
!253 = !DILocation(line: 63, column: 52, scope: !135)
!254 = !DILocation(line: 63, column: 50, scope: !135)
!255 = !DILocation(line: 63, column: 66, scope: !135)
!256 = !DILocation(line: 63, column: 61, scope: !135)
!257 = !DILocation(line: 63, column: 54, scope: !135)
!258 = !DILocation(line: 63, column: 88, scope: !135)
!259 = !DILocation(line: 63, column: 79, scope: !135)
!260 = !DILocation(line: 63, column: 72, scope: !135)
!261 = !DILocation(line: 64, column: 10, scope: !135)
!262 = !DILocation(line: 63, column: 100, scope: !135)
!263 = !DILocation(line: 64, column: 20, scope: !135)
!264 = !DILocation(line: 64, column: 25, scope: !135)
!265 = !DILocation(line: 64, column: 19, scope: !135)
!266 = !DILocation(line: 64, column: 17, scope: !135)
!267 = !DILocation(line: 62, column: 3, scope: !135)
!268 = !DILocation(line: 65, column: 1, scope: !135)
!269 = distinct !DISubprogram(name: "S", linkageName: "_ZN2ns1SIiLi3EEC2Ev", scope: !31, file: !3, line: 9, type: !270, scopeLine: 9, flags: DIFlagArtificial | DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !2, declaration: !273, retainedNodes: !45)
!270 = !DISubroutineType(types: !271)
!271 = !{null, !272}
!272 = !DIDerivedType(tag: DW_TAG_pointer_type, baseType: !31, size: 64, flags: DIFlagArtificial | DIFlagObjectPointer)
!273 = !DISubprogram(name: "S", scope: !31, type: !270, flags: DIFlagArtificial | DIFlagPrototyped, spFlags: 0)
!274 = !DILocalVariable(name: "this", arg: 1, scope: !269, type: !275, flags: DIFlagArtificial | DIFlagObjectPointer)
!275 = !DIDerivedType(tag: DW_TAG_pointer_type, baseType: !31, size: 64)
!276 = !DILocation(line: 0, scope: !269)
!277 = !DILocation(line: 9, column: 34, scope: !269)
!278 = distinct !DISubprogram(name: "Derived", linkageName: "_ZN7DerivedC1Ev", scope: !202, file: !3, line: 28, type: !279, scopeLine: 28, flags: DIFlagArtificial | DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !2, declaration: !282, retainedNodes: !45)
!279 = !DISubroutineType(types: !280)
!280 = !{null, !281}
!281 = !DIDerivedType(tag: DW_TAG_pointer_type, baseType: !202, size: 64, flags: DIFlagArtificial | DIFlagObjectPointer)
!282 = !DISubprogram(name: "Derived", scope: !202, type: !279, flags: DIFlagArtificial | DIFlagPrototyped, spFlags: 0)
!283 = !DILocalVariable(name: "this", arg: 1, scope: !278, type: !284, flags: DIFlagArtificial | DIFlagObjectPointer)
!284 = !DIDerivedType(tag: DW_TAG_pointer_type, baseType: !202, size: 64)
!285 = !DILocation(line: 0, scope: !278)
!286 = !DILocation(line: 28, column: 8, scope: !278)
!287 = distinct !DISubprogram(name: "operator()", linkageName: "_ZZ4mainENK3$_0clEi", scope: !155, file: !3, line: 51, type: !288, scopeLine: 51, flags: DIFlagPrototyped, spFlags: DISPFlagLocalToUnit | DISPFlagDefinition, unit: !2, declaration: !292, retainedNodes: !45)
!288 = !DISubroutineType(types: !289)
!289 = !{!12, !290, !12}
!290 = !DIDerivedType(tag: DW_TAG_pointer_type, baseType: !291, size: 64, flags: DIFlagArtificial | DIFlagObjectPointer)
!291 = !DIDerivedType(tag: DW_TAG_const_type, baseType: !155)
!292 = !DISubprogram(name: "operator()", scope: !155, file: !3, line: 51, type: !288, scopeLine: 51, flags: DIFlagPublic | DIFlagPrototyped, spFlags: DISPFlagLocalToUnit)
!293 = !DILocalVariable(name: "this", arg: 1, scope: !287, type: !294, flags: DIFlagArtificial | DIFlagObjectPointer)
!294 = !DIDerivedType(tag: DW_TAG_pointer_type, baseType: !291, size: 64)
!295 = !DILocation(line: 0, scope: !287)
!296 = !DILocalVariable(name: "q", arg: 2, scope: !287, file: !3, line: 51, type: !12)
!297 = !DILocation(line: 51, column: 22, scope: !287)
!298 = !DILocation(line: 51, column: 34, scope: !287)
!299 = !DILocation(line: 51, column: 38, scope: !287)
!300 = !{i64 8}
!301 = !DILocation(line: 51, column: 40, scope: !287)
!302 = !DILocation(line: 51, column: 36, scope: !287)
!303 = !DILocation(line: 51, column: 27, scope: !287)
!304 = distinct !DISubprogram(name: "v", linkageName: "_ZNK7Derived1vEv", scope: !202, file: !3, line: 28, type: !305, scopeLine: 28, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !2, declaration: !309, retainedNodes: !45)
!305 = !DISubroutineType(types: !306)
!306 = !{!12, !307}
!307 = !DIDerivedType(tag: DW_TAG_pointer_type, baseType: !308, size: 64, flags: DIFlagArtificial | DIFlagObjectPointer)
!308 = !DIDerivedType(tag: DW_TAG_const_type, baseType: !202)
!309 = !DISubprogram(name: "v", linkageName: "_ZNK7Derived1vEv", scope: !202, file: !3, line: 28, type: !305, scopeLine: 28, containingType: !202, virtualIndex: 0, flags: DIFlagPrototyped, spFlags: DISPFlagVirtual)
!310 = !DILocalVariable(name: "this", arg: 1, scope: !304, type: !311, flags: DIFlagArtificial | DIFlagObjectPointer)
!311 = !DIDerivedType(tag: DW_TAG_pointer_type, baseType: !308, size: 64)
!312 = !DILocation(line: 0, scope: !304)
!313 = !DILocation(line: 28, column: 58, scope: !304)
!314 = distinct !DISubprogram(name: "~Derived", linkageName: "_ZN7DerivedD1Ev", scope: !202, file: !3, line: 28, type: !279, scopeLine: 28, flags: DIFlagArtificial | DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !2, declaration: !315, retainedNodes: !45)
!315 = !DISubprogram(name: "~Derived", scope: !202, type: !279, containingType: !202, virtualIndex: 0, flags: DIFlagArtificial | DIFlagPrototyped, spFlags: DISPFlagVirtual)
!316 = !DILocalVariable(name: "this", arg: 1, scope: !314, type: !284, flags: DIFlagArtificial | DIFlagObjectPointer)
!317 = !DILocation(line: 0, scope: !314)
!318 = !DILocation(line: 28, column: 8, scope: !314)
!319 = distinct !DISubprogram(name: "~S", linkageName: "_ZN2ns1SIiLi3EED2Ev", scope: !31, file: !3, line: 9, type: !270, scopeLine: 9, flags: DIFlagArtificial | DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !2, declaration: !320, retainedNodes: !45)
!320 = !DISubprogram(name: "~S", scope: !31, type: !270, containingType: !31, virtualIndex: 0, flags: DIFlagArtificial | DIFlagPrototyped, spFlags: DISPFlagVirtual)
!321 = !DILocalVariable(name: "this", arg: 1, scope: !319, type: !275, flags: DIFlagArtificial | DIFlagObjectPointer)
!322 = !DILocation(line: 0, scope: !319)
!323 = !DILocation(line: 9, column: 34, scope: !324)
!324 = distinct !DILexicalBlock(scope: !319, file: !3, line: 9, column: 34)
!325 = !DILocation(line: 9, column: 34, scope: !319)
!326 = distinct !DISubprogram(name: "B", linkageName: "_ZN2ns1BC2Ev", scope: !327, file: !3, line: 8, type: !328, scopeLine: 8, flags: DIFlagArtificial | DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !2, declaration: !331, retainedNodes: !45)
!327 = distinct !DICompositeType(tag: DW_TAG_structure_type, name: "B", scope: !29, file: !3, line: 8, size: 128, flags: DIFlagFwdDecl | DIFlagNonTrivial, identifier: "_ZTSN2ns1BE")
!328 = !DISubroutineType(types: !329)
!329 = !{null, !330}
!330 = !DIDerivedType(tag: DW_TAG_pointer_type, baseType: !327, size: 64, flags: DIFlagArtificial | DIFlagObjectPointer)
!331 = !DISubprogram(name: "B", scope: !327, type: !328, flags: DIFlagArtificial | DIFlagPrototyped, spFlags: 0)
!332 = !DILocalVariable(name: "this", arg: 1, scope: !326, type: !333, flags: DIFlagArtificial | DIFlagObjectPointer)
!333 = !DIDerivedType(tag: DW_TAG_pointer_type, baseType: !327, size: 64)
!334 = !DILocation(line: 0, scope: !326)
!335 = !DILocation(line: 8, column: 8, scope: !326)
!336 = distinct !DISubprogram(name: "~S", linkageName: "_ZN2ns1SIiLi3EED0Ev", scope: !31, file: !3, line: 9, type: !270, scopeLine: 9, flags: DIFlagArtificial | DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !2, declaration: !320, retainedNodes: !45)
!337 = !DILocalVariable(name: "this", arg: 1, scope: !336, type: !275, flags: DIFlagArtificial | DIFlagObjectPointer)
!338 = !DILocation(line: 0, scope: !336)
!339 = !DILocation(line: 9, column: 34, scope: !336)
!340 = distinct !DISubprogram(name: "~B", linkageName: "_ZN2ns1BD2Ev", scope: !327, file: !3, line: 8, type: !328, scopeLine: 8, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !2, declaration: !341, retainedNodes: !45)
!341 = !DISubprogram(name: "~B", scope: !327, file: !3, line: 8, type: !328, scopeLine: 8, containingType: !327, virtualIndex: 0, flags: DIFlagPrototyped, spFlags: DISPFlagVirtual)
!342 = !DILocalVariable(name: "this", arg: 1, scope: !340, type: !333, flags: DIFlagArtificial | DIFlagObjectPointer)
!343 = !DILocation(line: 0, scope: !340)
!344 = !DILocation(line: 8, column: 26, scope: !340)
!345 = distinct !DISubprogram(name: "~B", linkageName: "_ZN2ns1BD0Ev", scope: !327, file: !3, line: 8, type: !328, scopeLine: 8, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !2, declaration: !341, retainedNodes: !45)
!346 = !DILocalVariable(name: "this", arg: 1, scope: !345, type: !333, flags: DIFlagArtificial | DIFlagObjectPointer)
!347 = !DILocation(line: 0, scope: !345)
!348 = !DILocation(line: 8, column: 25, scope: !345)
!349 = !DILocation(line: 8, column: 26, scope: !345)
!350 = distinct !DISubprogram(name: "Base", linkageName: "_ZN4BaseC2Ev", scope: !351, file: !3, line: 27, type: !352, scopeLine: 27, flags: DIFlagArtificial | DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !2, declaration: !355, retainedNodes: !45)
!351 = distinct !DICompositeType(tag: DW_TAG_structure_type, name: "Base", file: !3, line: 27, size: 128, flags: DIFlagFwdDecl | DIFlagNonTrivial, identifier: "_ZTS4Base")
!352 = !DISubroutineType(types: !353)
!353 = !{null, !354}
!354 = !DIDerivedType(tag: DW_TAG_pointer_type, baseType: !351, size: 64, flags: DIFlagArtificial | DIFlagObjectPointer)
!355 = !DISubprogram(name: "Base", scope: !351, type: !352, flags: DIFlagArtificial | DIFlagPrototyped, spFlags: 0)
!356 = !DILocalVariable(name: "this", arg: 1, scope: !350, type: !357, flags: DIFlagArtificial | DIFlagObjectPointer)
!357 = !DIDerivedType(tag: DW_TAG_pointer_type, baseType: !351, size: 64)
!358 = !DILocation(line: 0, scope: !350)
!359 = !DILocation(line: 27, column: 8, scope: !350)
!360 = distinct !DISubprogram(name: "~Derived", linkageName: "_ZN7DerivedD0Ev", scope: !202, file: !3, line: 28, type: !279, scopeLine: 28, flags: DIFlagArtificial | DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !2, declaration: !315, retainedNodes: !45)
!361 = !DILocalVariable(name: "this", arg: 1, scope: !360, type: !284, flags: DIFlagArtificial | DIFlagObjectPointer)
!362 = !DILocation(line: 0, scope: !360)
!363 = !DILocation(line: 28, column: 8, scope: !360)
!364 = distinct !DISubprogram(linkageName: "_ZTv0_n24_N7DerivedD1Ev", scope: !3, file: !3, line: 28, type: !365, scopeLine: 28, flags: DIFlagArtificial | DIFlagThunk, spFlags: DISPFlagDefinition, unit: !2)
!365 = !DISubroutineType(types: !45)
!366 = !DILocation(line: 0, scope: !364)
!367 = !DILocation(line: 28, column: 8, scope: !364)
!368 = distinct !DISubprogram(linkageName: "_ZTv0_n24_N7DerivedD0Ev", scope: !3, file: !3, line: 28, type: !365, scopeLine: 28, flags: DIFlagArtificial | DIFlagThunk, spFlags: DISPFlagDefinition, unit: !2)
!369 = !DILocation(line: 0, scope: !368)
!370 = !DILocation(line: 28, column: 8, scope: !368)
!371 = distinct !DISubprogram(linkageName: "_ZTv0_n32_NK7Derived1vEv", scope: !3, file: !3, line: 28, type: !365, scopeLine: 28, flags: DIFlagArtificial | DIFlagThunk, spFlags: DISPFlagDefinition, unit: !2)
!372 = !DILocation(line: 0, scope: !371)
!373 = distinct !DISubprogram(name: "~Base", linkageName: "_ZN4BaseD2Ev", scope: !351, file: !3, line: 27, type: !352, scopeLine: 27, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !2, declaration: !374, retainedNodes: !45)
!374 = !DISubprogram(name: "~Base", scope: !351, file: !3, line: 27, type: !352, scopeLine: 27, containingType: !351, virtualIndex: 0, flags: DIFlagPrototyped, spFlags: DISPFlagVirtual)
!375 = !DILocalVariable(name: "this", arg: 1, scope: !373, type: !357, flags: DIFlagArtificial | DIFlagObjectPointer)
!376 = !DILocation(line: 0, scope: !373)
!377 = !DILocation(line: 27, column: 23, scope: !373)
!378 = distinct !DISubprogram(name: "~Base", linkageName: "_ZN4BaseD0Ev", scope: !351, file: !3, line: 27, type: !352, scopeLine: 27, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !2, declaration: !374, retainedNodes: !45)
!379 = !DILocalVariable(name: "this", arg: 1, scope: !378, type: !357, flags: DIFlagArtificial | DIFlagObjectPointer)
!380 = !DILocation(line: 0, scope: !378)
!381 = !DILocation(line: 27, column: 23, scope: !378)
!382 = distinct !DISubprogram(name: "v", linkageName: "_ZNK4Base1vEv", scope: !351, file: !3, line: 27, type: !383, scopeLine: 27, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !2, declaration: !387, retainedNodes: !45)
!383 = !DISubroutineType(types: !384)
!384 = !{!12, !385}
!385 = !DIDerivedType(tag: DW_TAG_pointer_type, baseType: !386, size: 64, flags: DIFlagArtificial | DIFlagObjectPointer)
!386 = !DIDerivedType(tag: DW_TAG_const_type, baseType: !351)
!387 = !DISubprogram(name: "v", linkageName: "_ZNK4Base1vEv", scope: !351, file: !3, line: 27, type: !383, scopeLine: 27, containingType: !351, virtualIndex: 2, flags: DIFlagPrototyped, spFlags: DISPFlagVirtual)
!388 = !DILocalVariable(name: "this", arg: 1, scope: !382, type: !389, flags: DIFlagArtificial | DIFlagObjectPointer)
!389 = !DIDerivedType(tag: DW_TAG_pointer_type, baseType: !386, size: 64)
!390 = !DILocation(line: 0, scope: !382)
!391 = !DILocation(line: 27, column: 66, scope: !382)
!392 = distinct !DISubprogram(name: "f", linkageName: "_ZNK2ns1SIiLi3EE1fEi", scope: !31, file: !3, line: 14, type: !393, scopeLine: 14, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !2, declaration: !397, retainedNodes: !45)
!393 = !DISubroutineType(types: !394)
!394 = !{!12, !395, !12}
!395 = !DIDerivedType(tag: DW_TAG_pointer_type, baseType: !396, size: 64, flags: DIFlagArtificial | DIFlagObjectPointer)
!396 = !DIDerivedType(tag: DW_TAG_const_type, baseType: !31)
!397 = !DISubprogram(name: "f", linkageName: "_ZNK2ns1SIiLi3EE1fEi", scope: !31, file: !3, line: 14, type: !393, scopeLine: 14, flags: DIFlagPrototyped, spFlags: 0)
!398 = !DILocalVariable(name: "this", arg: 1, scope: !392, type: !399, flags: DIFlagArtificial | DIFlagObjectPointer)
!399 = !DIDerivedType(tag: DW_TAG_pointer_type, baseType: !396, size: 64)
!400 = !DILocation(line: 0, scope: !392)
!401 = !DILocalVariable(name: "y", arg: 2, scope: !392, file: !3, line: 14, type: !12)
!402 = !DILocation(line: 14, column: 13, scope: !392)
!403 = !DILocalVariable(name: "z", scope: !392, file: !3, line: 14, type: !12)
!404 = !DILocation(line: 14, column: 28, scope: !392)
!405 = !DILocation(line: 14, column: 32, scope: !392)
!406 = !DILocation(line: 14, column: 36, scope: !392)
!407 = !DILocation(line: 14, column: 34, scope: !392)
!408 = !DILocation(line: 14, column: 49, scope: !392)
!409 = !DILocation(line: 14, column: 42, scope: !392)
!410 = distinct !DISubprogram(name: "~Derived", linkageName: "_ZN7DerivedD2Ev", scope: !202, file: !3, line: 28, type: !279, scopeLine: 28, flags: DIFlagArtificial | DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !2, declaration: !315, retainedNodes: !45)
!411 = !DILocalVariable(name: "this", arg: 1, scope: !410, type: !284, flags: DIFlagArtificial | DIFlagObjectPointer)
!412 = !DILocation(line: 0, scope: !410)
!413 = !DILocalVariable(name: "vtt", arg: 2, scope: !410, type: !414, flags: DIFlagArtificial)
!414 = !DIDerivedType(tag: DW_TAG_pointer_type, baseType: !415, size: 64)
!415 = !DIDerivedType(tag: DW_TAG_pointer_type, baseType: null, size: 64)
!416 = !DILocation(line: 28, column: 8, scope: !410)
