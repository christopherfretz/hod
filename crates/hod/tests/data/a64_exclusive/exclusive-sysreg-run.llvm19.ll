;@ covers: kiln tests/conformance/target.acle/exclusive-sysreg-run.cpp
;@ target: aarch64
;@ llvm: 19
;@ link: c++ m
;@ stdout: ok 9
; corpus entry written by `cargo xtask corpus capture`; do not edit
; source: kiln tests/conformance/target.acle/exclusive-sysreg-run.cpp
; producer-args: -std=c++26 -fno-crash-diagnostics -fclang-compat=19 -O2 -fdebug-prefix-map=<cwd>=. tests/conformance/target.acle/exclusive-sysreg-run.cpp
; delivered: bitcode
; llc-args: -O2 -relocation-model=pic -filetype=obj
; ModuleID = '<stdin>'
source_filename = "tests/conformance/target.acle/exclusive-sysreg-run.cpp"
target datalayout = "e-m:e-i8:8:32-i16:16:32-i64:64-i128:128-n32:64-S128-Fn32"
target triple = "aarch64-unknown-linux-gnu"

@.str = private unnamed_addr constant [12 x i8] c"8-bit wraps\00", align 1
@.str.1 = private unnamed_addr constant [15 x i8] c"32-bit acq/rel\00", align 1
@.str.2 = private unnamed_addr constant [7 x i8] c"64-bit\00", align 1
@.str.3 = private unnamed_addr constant [8 x i8] c"128-bit\00", align 1
@.str.4 = private unnamed_addr constant [7 x i8] c"double\00", align 1
@.str.5 = private unnamed_addr constant [8 x i8] c"pointer\00", align 1
@.str.6 = private unnamed_addr constant [6 x i8] c"clrex\00", align 1
@.str.7 = private unnamed_addr constant [21 x i8] c"cntvct_el0 monotonic\00", align 1
@.str.8 = private unnamed_addr constant [20 x i8] c"upper-case spelling\00", align 1
@_ZL5fails = internal unnamed_addr global i32 0, align 4
@.str.9 = private unnamed_addr constant [7 x i8] c"ok %d\0A\00", align 1
@_ZL6checks = internal unnamed_addr global i32 0, align 4
@.str.10 = private unnamed_addr constant [9 x i8] c"FAIL %s\0A\00", align 1

; Function Attrs: mustprogress norecurse nounwind uwtable
define dso_local noundef range(i32 0, 2) i32 @main() local_unnamed_addr #0 {
.preheader31:
  %0 = alloca i8, align 4
  %1 = alloca i32, align 4
  %2 = alloca i64, align 8
  %3 = alloca i128, align 16
  %4 = alloca double, align 8
  %5 = alloca [4 x i32], align 4
  %6 = alloca ptr, align 8
  call void @llvm.lifetime.start.p0(i64 1, ptr nonnull %0) #4
  store i8 -6, ptr %0, align 4, !tbaa !6
  br label %11

7:                                                ; preds = %_ZL4bumpIhiET_PS0_T0_.exit.9
  %8 = load i32, ptr @_ZL5fails, align 4, !tbaa !9
  %9 = add nsw i32 %8, 1
  store i32 %9, ptr @_ZL5fails, align 4, !tbaa !9
  %10 = call i32 (ptr, ...) @printf(ptr noundef nonnull dereferenceable(1) @.str.10, ptr noundef nonnull @.str)
  br label %_ZL5checkbPKc.exit

_ZL5checkbPKc.exit:                               ; preds = %_ZL4bumpIhiET_PS0_T0_.exit.9, %7
  call void @llvm.lifetime.start.p0(i64 4, ptr nonnull %1) #4
  store i32 0, ptr %1, align 4, !tbaa !9
  br label %.preheader

11:                                               ; preds = %.preheader31, %11
  %12 = call i64 @llvm.aarch64.ldxr.p0(ptr nonnull elementtype(i8) %0)
  %13 = add i64 %12, 1
  %14 = and i64 %13, 255
  %15 = call i32 @llvm.aarch64.stxr.p0(i64 %14, ptr nonnull elementtype(i8) %0)
  %.not.i = icmp eq i32 %15, 0
  br i1 %.not.i, label %_ZL4bumpIhiET_PS0_T0_.exit, label %11, !llvm.loop !11

_ZL4bumpIhiET_PS0_T0_.exit:                       ; preds = %11, %_ZL4bumpIhiET_PS0_T0_.exit
  %16 = call i64 @llvm.aarch64.ldxr.p0(ptr nonnull elementtype(i8) %0)
  %17 = add i64 %16, 1
  %18 = and i64 %17, 255
  %19 = call i32 @llvm.aarch64.stxr.p0(i64 %18, ptr nonnull elementtype(i8) %0)
  %.not.i.1 = icmp eq i32 %19, 0
  br i1 %.not.i.1, label %_ZL4bumpIhiET_PS0_T0_.exit.1, label %_ZL4bumpIhiET_PS0_T0_.exit, !llvm.loop !11

_ZL4bumpIhiET_PS0_T0_.exit.1:                     ; preds = %_ZL4bumpIhiET_PS0_T0_.exit, %_ZL4bumpIhiET_PS0_T0_.exit.1
  %20 = call i64 @llvm.aarch64.ldxr.p0(ptr nonnull elementtype(i8) %0)
  %21 = add i64 %20, 1
  %22 = and i64 %21, 255
  %23 = call i32 @llvm.aarch64.stxr.p0(i64 %22, ptr nonnull elementtype(i8) %0)
  %.not.i.2 = icmp eq i32 %23, 0
  br i1 %.not.i.2, label %_ZL4bumpIhiET_PS0_T0_.exit.2, label %_ZL4bumpIhiET_PS0_T0_.exit.1, !llvm.loop !11

_ZL4bumpIhiET_PS0_T0_.exit.2:                     ; preds = %_ZL4bumpIhiET_PS0_T0_.exit.1, %_ZL4bumpIhiET_PS0_T0_.exit.2
  %24 = call i64 @llvm.aarch64.ldxr.p0(ptr nonnull elementtype(i8) %0)
  %25 = add i64 %24, 1
  %26 = and i64 %25, 255
  %27 = call i32 @llvm.aarch64.stxr.p0(i64 %26, ptr nonnull elementtype(i8) %0)
  %.not.i.3 = icmp eq i32 %27, 0
  br i1 %.not.i.3, label %_ZL4bumpIhiET_PS0_T0_.exit.3, label %_ZL4bumpIhiET_PS0_T0_.exit.2, !llvm.loop !11

_ZL4bumpIhiET_PS0_T0_.exit.3:                     ; preds = %_ZL4bumpIhiET_PS0_T0_.exit.2, %_ZL4bumpIhiET_PS0_T0_.exit.3
  %28 = call i64 @llvm.aarch64.ldxr.p0(ptr nonnull elementtype(i8) %0)
  %29 = add i64 %28, 1
  %30 = and i64 %29, 255
  %31 = call i32 @llvm.aarch64.stxr.p0(i64 %30, ptr nonnull elementtype(i8) %0)
  %.not.i.4 = icmp eq i32 %31, 0
  br i1 %.not.i.4, label %_ZL4bumpIhiET_PS0_T0_.exit.4, label %_ZL4bumpIhiET_PS0_T0_.exit.3, !llvm.loop !11

_ZL4bumpIhiET_PS0_T0_.exit.4:                     ; preds = %_ZL4bumpIhiET_PS0_T0_.exit.3, %_ZL4bumpIhiET_PS0_T0_.exit.4
  %32 = call i64 @llvm.aarch64.ldxr.p0(ptr nonnull elementtype(i8) %0)
  %33 = add i64 %32, 1
  %34 = and i64 %33, 255
  %35 = call i32 @llvm.aarch64.stxr.p0(i64 %34, ptr nonnull elementtype(i8) %0)
  %.not.i.5 = icmp eq i32 %35, 0
  br i1 %.not.i.5, label %_ZL4bumpIhiET_PS0_T0_.exit.5, label %_ZL4bumpIhiET_PS0_T0_.exit.4, !llvm.loop !11

_ZL4bumpIhiET_PS0_T0_.exit.5:                     ; preds = %_ZL4bumpIhiET_PS0_T0_.exit.4, %_ZL4bumpIhiET_PS0_T0_.exit.5
  %36 = call i64 @llvm.aarch64.ldxr.p0(ptr nonnull elementtype(i8) %0)
  %37 = add i64 %36, 1
  %38 = and i64 %37, 255
  %39 = call i32 @llvm.aarch64.stxr.p0(i64 %38, ptr nonnull elementtype(i8) %0)
  %.not.i.6 = icmp eq i32 %39, 0
  br i1 %.not.i.6, label %_ZL4bumpIhiET_PS0_T0_.exit.6, label %_ZL4bumpIhiET_PS0_T0_.exit.5, !llvm.loop !11

_ZL4bumpIhiET_PS0_T0_.exit.6:                     ; preds = %_ZL4bumpIhiET_PS0_T0_.exit.5, %_ZL4bumpIhiET_PS0_T0_.exit.6
  %40 = call i64 @llvm.aarch64.ldxr.p0(ptr nonnull elementtype(i8) %0)
  %41 = add i64 %40, 1
  %42 = and i64 %41, 255
  %43 = call i32 @llvm.aarch64.stxr.p0(i64 %42, ptr nonnull elementtype(i8) %0)
  %.not.i.7 = icmp eq i32 %43, 0
  br i1 %.not.i.7, label %_ZL4bumpIhiET_PS0_T0_.exit.7, label %_ZL4bumpIhiET_PS0_T0_.exit.6, !llvm.loop !11

_ZL4bumpIhiET_PS0_T0_.exit.7:                     ; preds = %_ZL4bumpIhiET_PS0_T0_.exit.6, %_ZL4bumpIhiET_PS0_T0_.exit.7
  %44 = call i64 @llvm.aarch64.ldxr.p0(ptr nonnull elementtype(i8) %0)
  %45 = add i64 %44, 1
  %46 = and i64 %45, 255
  %47 = call i32 @llvm.aarch64.stxr.p0(i64 %46, ptr nonnull elementtype(i8) %0)
  %.not.i.8 = icmp eq i32 %47, 0
  br i1 %.not.i.8, label %_ZL4bumpIhiET_PS0_T0_.exit.8, label %_ZL4bumpIhiET_PS0_T0_.exit.7, !llvm.loop !11

_ZL4bumpIhiET_PS0_T0_.exit.8:                     ; preds = %_ZL4bumpIhiET_PS0_T0_.exit.7, %_ZL4bumpIhiET_PS0_T0_.exit.8
  %48 = call i64 @llvm.aarch64.ldxr.p0(ptr nonnull elementtype(i8) %0)
  %49 = add i64 %48, 1
  %50 = and i64 %49, 255
  %51 = call i32 @llvm.aarch64.stxr.p0(i64 %50, ptr nonnull elementtype(i8) %0)
  %.not.i.9 = icmp eq i32 %51, 0
  br i1 %.not.i.9, label %_ZL4bumpIhiET_PS0_T0_.exit.9, label %_ZL4bumpIhiET_PS0_T0_.exit.8, !llvm.loop !11

_ZL4bumpIhiET_PS0_T0_.exit.9:                     ; preds = %_ZL4bumpIhiET_PS0_T0_.exit.8
  %52 = load i8, ptr %0, align 4, !tbaa !6
  %53 = icmp eq i8 %52, 4
  %54 = load i32, ptr @_ZL6checks, align 4, !tbaa !9
  %55 = add nsw i32 %54, 1
  store i32 %55, ptr @_ZL6checks, align 4, !tbaa !9
  br i1 %53, label %_ZL5checkbPKc.exit, label %7

.preheader:                                       ; preds = %_ZL5checkbPKc.exit, %_ZL12bump_acq_relIiiET_PS0_T0_.exit
  %.01433 = phi i32 [ 0, %_ZL5checkbPKc.exit ], [ %137, %_ZL12bump_acq_relIiiET_PS0_T0_.exit ]
  br label %132

56:                                               ; preds = %_ZL12bump_acq_relIiiET_PS0_T0_.exit
  %57 = load i32, ptr %1, align 4, !tbaa !9
  %58 = icmp eq i32 %57, 300
  %59 = load i32, ptr @_ZL6checks, align 4, !tbaa !9
  %60 = add nsw i32 %59, 1
  store i32 %60, ptr @_ZL6checks, align 4, !tbaa !9
  br i1 %58, label %_ZL5checkbPKc.exit18, label %61

61:                                               ; preds = %56
  %62 = load i32, ptr @_ZL5fails, align 4, !tbaa !9
  %63 = add nsw i32 %62, 1
  store i32 %63, ptr @_ZL5fails, align 4, !tbaa !9
  %64 = call i32 (ptr, ...) @printf(ptr noundef nonnull dereferenceable(1) @.str.10, ptr noundef nonnull @.str.1)
  br label %_ZL5checkbPKc.exit18

_ZL5checkbPKc.exit18:                             ; preds = %56, %61
  call void @llvm.lifetime.start.p0(i64 8, ptr nonnull %2) #4
  store i64 1099511627776, ptr %2, align 8, !tbaa !13
  br label %65

65:                                               ; preds = %65, %_ZL5checkbPKc.exit18
  %66 = call i64 @llvm.aarch64.ldxr.p0(ptr nonnull elementtype(i64) %2)
  %67 = add nsw i64 %66, -1
  %68 = call i32 @llvm.aarch64.stxr.p0(i64 %67, ptr nonnull elementtype(i64) %2)
  %.not.i19 = icmp eq i32 %68, 0
  br i1 %.not.i19, label %_ZL4bumpIliET_PS0_T0_.exit, label %65, !llvm.loop !15

_ZL4bumpIliET_PS0_T0_.exit:                       ; preds = %65
  %69 = icmp eq i64 %66, 1099511627776
  %70 = load i64, ptr %2, align 8
  %71 = icmp eq i64 %70, 1099511627775
  %72 = select i1 %69, i1 %71, i1 false
  %73 = load i32, ptr @_ZL6checks, align 4, !tbaa !9
  %74 = add nsw i32 %73, 1
  store i32 %74, ptr @_ZL6checks, align 4, !tbaa !9
  br i1 %72, label %_ZL5checkbPKc.exit20, label %75

75:                                               ; preds = %_ZL4bumpIliET_PS0_T0_.exit
  %76 = load i32, ptr @_ZL5fails, align 4, !tbaa !9
  %77 = add nsw i32 %76, 1
  store i32 %77, ptr @_ZL5fails, align 4, !tbaa !9
  %78 = call i32 (ptr, ...) @printf(ptr noundef nonnull dereferenceable(1) @.str.10, ptr noundef nonnull @.str.2)
  br label %_ZL5checkbPKc.exit20

_ZL5checkbPKc.exit20:                             ; preds = %_ZL4bumpIliET_PS0_T0_.exit, %75
  call void @llvm.lifetime.start.p0(i64 16, ptr nonnull %3) #4
  store i128 1267650600228229401496703205376, ptr %3, align 16, !tbaa !16
  br label %79

79:                                               ; preds = %79, %_ZL5checkbPKc.exit20
  %80 = call { i64, i64 } @llvm.aarch64.ldaxp(ptr nonnull %3)
  %81 = extractvalue { i64, i64 } %80, 1
  %82 = extractvalue { i64, i64 } %80, 0
  %83 = zext i64 %82 to i128
  %84 = add nuw nsw i128 %83, 7
  %.sroa.0.0.extract.trunc.i = trunc i128 %84 to i64
  %85 = lshr i128 %84, 64
  %.tr = trunc nuw nsw i128 %85 to i64
  %.narrow = add i64 %81, %.tr
  %86 = call i32 @llvm.aarch64.stlxp(i64 %.sroa.0.0.extract.trunc.i, i64 %.narrow, ptr nonnull %3)
  %.not.i21 = icmp eq i32 %86, 0
  br i1 %.not.i21, label %_ZL12bump_acq_relIniET_PS0_T0_.exit, label %79, !llvm.loop !18

_ZL12bump_acq_relIniET_PS0_T0_.exit:              ; preds = %79
  %87 = load i128, ptr %3, align 16, !tbaa !16
  %88 = icmp eq i128 %87, 1267650600228229401496703205383
  %89 = load i32, ptr @_ZL6checks, align 4, !tbaa !9
  %90 = add nsw i32 %89, 1
  store i32 %90, ptr @_ZL6checks, align 4, !tbaa !9
  br i1 %88, label %_ZL5checkbPKc.exit22, label %91

91:                                               ; preds = %_ZL12bump_acq_relIniET_PS0_T0_.exit
  %92 = load i32, ptr @_ZL5fails, align 4, !tbaa !9
  %93 = add nsw i32 %92, 1
  store i32 %93, ptr @_ZL5fails, align 4, !tbaa !9
  %94 = call i32 (ptr, ...) @printf(ptr noundef nonnull dereferenceable(1) @.str.10, ptr noundef nonnull @.str.3)
  br label %_ZL5checkbPKc.exit22

_ZL5checkbPKc.exit22:                             ; preds = %_ZL12bump_acq_relIniET_PS0_T0_.exit, %91
  call void @llvm.lifetime.start.p0(i64 8, ptr nonnull %4) #4
  store double 1.500000e+00, ptr %4, align 8, !tbaa !19
  br label %95

95:                                               ; preds = %95, %_ZL5checkbPKc.exit22
  %96 = call i64 @llvm.aarch64.ldxr.p0(ptr nonnull elementtype(i64) %4)
  %97 = bitcast i64 %96 to double
  %98 = fadd double %97, 2.500000e-01
  %99 = bitcast double %98 to i64
  %100 = call i32 @llvm.aarch64.stxr.p0(i64 %99, ptr nonnull elementtype(i64) %4)
  %.not.i23 = icmp eq i32 %100, 0
  br i1 %.not.i23, label %_ZL4bumpIddET_PS0_T0_.exit, label %95, !llvm.loop !21

_ZL4bumpIddET_PS0_T0_.exit:                       ; preds = %95
  %101 = load double, ptr %4, align 8, !tbaa !19
  %102 = fcmp oeq double %101, 1.750000e+00
  %103 = load i32, ptr @_ZL6checks, align 4, !tbaa !9
  %104 = add nsw i32 %103, 1
  store i32 %104, ptr @_ZL6checks, align 4, !tbaa !9
  br i1 %102, label %_ZL5checkbPKc.exit24, label %105

105:                                              ; preds = %_ZL4bumpIddET_PS0_T0_.exit
  %106 = load i32, ptr @_ZL5fails, align 4, !tbaa !9
  %107 = add nsw i32 %106, 1
  store i32 %107, ptr @_ZL5fails, align 4, !tbaa !9
  %108 = call i32 (ptr, ...) @printf(ptr noundef nonnull dereferenceable(1) @.str.10, ptr noundef nonnull @.str.4)
  br label %_ZL5checkbPKc.exit24

_ZL5checkbPKc.exit24:                             ; preds = %_ZL4bumpIddET_PS0_T0_.exit, %105
  call void @llvm.lifetime.start.p0(i64 16, ptr nonnull %5) #4
  call void @llvm.memset.p0.i64(ptr noundef nonnull align 4 dereferenceable(16) %5, i8 0, i64 16, i1 false)
  call void @llvm.lifetime.start.p0(i64 8, ptr nonnull %6) #4
  store ptr %5, ptr %6, align 8, !tbaa !22
  br label %109

109:                                              ; preds = %109, %_ZL5checkbPKc.exit24
  %110 = call i64 @llvm.aarch64.ldxr.p0(ptr nonnull elementtype(i64) %6)
  %111 = add nuw i64 %110, 12
  %112 = call i32 @llvm.aarch64.stxr.p0(i64 %111, ptr nonnull elementtype(i64) %6)
  %.not.i25 = icmp eq i32 %112, 0
  br i1 %.not.i25, label %_ZL4bumpIPiiET_PS1_T0_.exit, label %109, !llvm.loop !24

_ZL4bumpIPiiET_PS1_T0_.exit:                      ; preds = %109
  %113 = load ptr, ptr %6, align 8, !tbaa !22
  %114 = getelementptr inbounds i8, ptr %5, i64 12
  %115 = icmp eq ptr %113, %114
  %116 = load i32, ptr @_ZL6checks, align 4, !tbaa !9
  %117 = add nsw i32 %116, 1
  store i32 %117, ptr @_ZL6checks, align 4, !tbaa !9
  br i1 %115, label %_ZL5checkbPKc.exit26, label %118

118:                                              ; preds = %_ZL4bumpIPiiET_PS1_T0_.exit
  %119 = load i32, ptr @_ZL5fails, align 4, !tbaa !9
  %120 = add nsw i32 %119, 1
  store i32 %120, ptr @_ZL5fails, align 4, !tbaa !9
  %121 = call i32 (ptr, ...) @printf(ptr noundef nonnull dereferenceable(1) @.str.10, ptr noundef nonnull @.str.5)
  br label %_ZL5checkbPKc.exit26

_ZL5checkbPKc.exit26:                             ; preds = %_ZL4bumpIPiiET_PS1_T0_.exit, %118
  %122 = call i64 @llvm.aarch64.ldxr.p0(ptr nonnull elementtype(i32) %1)
  call void @llvm.aarch64.clrex()
  %123 = load i32, ptr %1, align 4, !tbaa !9
  %124 = icmp eq i32 %123, 300
  %125 = load i32, ptr @_ZL6checks, align 4, !tbaa !9
  %126 = add nsw i32 %125, 1
  store i32 %126, ptr @_ZL6checks, align 4, !tbaa !9
  br i1 %124, label %_ZL5checkbPKc.exit27, label %127

127:                                              ; preds = %_ZL5checkbPKc.exit26
  %128 = load i32, ptr @_ZL5fails, align 4, !tbaa !9
  %129 = add nsw i32 %128, 1
  store i32 %129, ptr @_ZL5fails, align 4, !tbaa !9
  %130 = call i32 (ptr, ...) @printf(ptr noundef nonnull dereferenceable(1) @.str.10, ptr noundef nonnull @.str.6)
  br label %_ZL5checkbPKc.exit27

_ZL5checkbPKc.exit27:                             ; preds = %_ZL5checkbPKc.exit26, %127
  %131 = call i64 @llvm.read_volatile_register.i64(metadata !25)
  br label %155

132:                                              ; preds = %.preheader, %132
  %133 = call i64 @llvm.aarch64.ldaxr.p0(ptr nonnull elementtype(i32) %1)
  %134 = add i64 %133, 3
  %135 = and i64 %134, 4294967295
  %136 = call i32 @llvm.aarch64.stlxr.p0(i64 %135, ptr nonnull elementtype(i32) %1)
  %.not.i28 = icmp eq i32 %136, 0
  br i1 %.not.i28, label %_ZL12bump_acq_relIiiET_PS0_T0_.exit, label %132, !llvm.loop !26

_ZL12bump_acq_relIiiET_PS0_T0_.exit:              ; preds = %132
  %137 = add nuw nsw i32 %.01433, 1
  %exitcond.not = icmp eq i32 %137, 100
  br i1 %exitcond.not, label %56, label %.preheader, !llvm.loop !27

138:                                              ; preds = %155
  %139 = load i32, ptr @_ZL6checks, align 4, !tbaa !9
  %140 = add nsw i32 %139, 1
  store i32 %140, ptr @_ZL6checks, align 4, !tbaa !9
  br i1 %158, label %_ZL5checkbPKc.exit29, label %141

141:                                              ; preds = %138
  %142 = load i32, ptr @_ZL5fails, align 4, !tbaa !9
  %143 = add nsw i32 %142, 1
  store i32 %143, ptr @_ZL5fails, align 4, !tbaa !9
  %144 = call i32 (ptr, ...) @printf(ptr noundef nonnull dereferenceable(1) @.str.10, ptr noundef nonnull @.str.7)
  br label %_ZL5checkbPKc.exit29

_ZL5checkbPKc.exit29:                             ; preds = %138, %141
  %145 = call i64 @llvm.read_volatile_register.i64(metadata !25)
  %146 = call i64 @llvm.read_volatile_register.i64(metadata !28)
  %.not = icmp ult i64 %146, %145
  %147 = load i32, ptr @_ZL6checks, align 4, !tbaa !9
  %148 = add nsw i32 %147, 1
  store i32 %148, ptr @_ZL6checks, align 4, !tbaa !9
  br i1 %.not, label %149, label %_ZL5checkbPKc.exit30

149:                                              ; preds = %_ZL5checkbPKc.exit29
  %150 = load i32, ptr @_ZL5fails, align 4, !tbaa !9
  %151 = add nsw i32 %150, 1
  store i32 %151, ptr @_ZL5fails, align 4, !tbaa !9
  %152 = call i32 (ptr, ...) @printf(ptr noundef nonnull dereferenceable(1) @.str.10, ptr noundef nonnull @.str.8)
  br label %_ZL5checkbPKc.exit30

_ZL5checkbPKc.exit30:                             ; preds = %_ZL5checkbPKc.exit29, %149
  %153 = load i32, ptr @_ZL5fails, align 4, !tbaa !9
  %154 = icmp eq i32 %153, 0
  br i1 %154, label %160, label %165

155:                                              ; preds = %_ZL5checkbPKc.exit27, %155
  %.01536 = phi i32 [ 0, %_ZL5checkbPKc.exit27 ], [ %159, %155 ]
  %.016.off035 = phi i1 [ true, %_ZL5checkbPKc.exit27 ], [ %158, %155 ]
  %.01734 = phi i64 [ %131, %_ZL5checkbPKc.exit27 ], [ %156, %155 ]
  %156 = call i64 @llvm.read_volatile_register.i64(metadata !25)
  %157 = icmp uge i64 %156, %.01734
  %158 = select i1 %.016.off035, i1 %157, i1 false
  %159 = add nuw nsw i32 %.01536, 1
  %exitcond38.not = icmp eq i32 %159, 1000
  br i1 %exitcond38.not, label %138, label %155, !llvm.loop !29

160:                                              ; preds = %_ZL5checkbPKc.exit30
  %161 = load i32, ptr @_ZL6checks, align 4, !tbaa !9
  %162 = call i32 (ptr, ...) @printf(ptr noundef nonnull dereferenceable(1) @.str.9, i32 noundef %161)
  %.pre = load i32, ptr @_ZL5fails, align 4, !tbaa !9
  %163 = icmp ne i32 %.pre, 0
  %164 = zext i1 %163 to i32
  br label %165

165:                                              ; preds = %160, %_ZL5checkbPKc.exit30
  %166 = phi i32 [ %164, %160 ], [ 1, %_ZL5checkbPKc.exit30 ]
  call void @llvm.lifetime.end.p0(i64 8, ptr nonnull %6) #4
  call void @llvm.lifetime.end.p0(i64 16, ptr nonnull %5) #4
  call void @llvm.lifetime.end.p0(i64 8, ptr nonnull %4) #4
  call void @llvm.lifetime.end.p0(i64 16, ptr nonnull %3) #4
  call void @llvm.lifetime.end.p0(i64 8, ptr nonnull %2) #4
  call void @llvm.lifetime.end.p0(i64 4, ptr nonnull %1) #4
  call void @llvm.lifetime.end.p0(i64 1, ptr nonnull %0) #4
  ret i32 %166
}

; Function Attrs: nocallback nofree nosync nounwind willreturn memory(argmem: readwrite)
declare void @llvm.lifetime.start.p0(i64 immarg, ptr nocapture) #1

; Function Attrs: nocallback nofree nosync nounwind willreturn memory(argmem: readwrite)
declare void @llvm.lifetime.end.p0(i64 immarg, ptr nocapture) #1

; Function Attrs: nocallback nofree nounwind willreturn memory(argmem: write)
declare void @llvm.memset.p0.i64(ptr nocapture writeonly, i8, i64, i1 immarg) #2

; Function Attrs: nofree nounwind willreturn
declare i64 @llvm.aarch64.ldxr.p0(ptr) #3

; Function Attrs: nounwind
declare void @llvm.aarch64.clrex() #4

; Function Attrs: nounwind
declare i64 @llvm.read_volatile_register.i64(metadata) #4

; Function Attrs: nofree nounwind
declare noundef i32 @printf(ptr nocapture noundef readonly, ...) local_unnamed_addr #5

; Function Attrs: nofree nounwind willreturn
declare i32 @llvm.aarch64.stxr.p0(i64, ptr) #3

; Function Attrs: nofree nounwind willreturn
declare i64 @llvm.aarch64.ldaxr.p0(ptr) #3

; Function Attrs: nofree nounwind willreturn
declare i32 @llvm.aarch64.stlxr.p0(i64, ptr) #3

; Function Attrs: nofree nounwind willreturn
declare { i64, i64 } @llvm.aarch64.ldaxp(ptr) #3

; Function Attrs: nofree nounwind willreturn
declare i32 @llvm.aarch64.stlxp(i64, i64, ptr) #3

attributes #0 = { mustprogress norecurse nounwind uwtable "frame-pointer"="non-leaf" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #1 = { nocallback nofree nosync nounwind willreturn memory(argmem: readwrite) }
attributes #2 = { nocallback nofree nounwind willreturn memory(argmem: write) }
attributes #3 = { nofree nounwind willreturn }
attributes #4 = { nounwind }
attributes #5 = { nofree nounwind "frame-pointer"="non-leaf" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }

!llvm.linker.options = !{}
!llvm.module.flags = !{!0, !1, !2, !3, !4}
!llvm.ident = !{!5}

!0 = !{i32 1, !"wchar_size", i32 4}
!1 = !{i32 8, !"PIC Level", i32 2}
!2 = !{i32 7, !"PIE Level", i32 2}
!3 = !{i32 7, !"uwtable", i32 2}
!4 = !{i32 7, !"frame-pointer", i32 1}
!5 = !{!"kiln version 1.2.0"}
!6 = !{!7, !7, i64 0}
!7 = !{!"omnipotent char", !8, i64 0}
!8 = !{!"Simple C++ TBAA"}
!9 = !{!10, !10, i64 0}
!10 = !{!"int", !7, i64 0}
!11 = distinct !{!11, !12}
!12 = !{!"llvm.loop.mustprogress"}
!13 = !{!14, !14, i64 0}
!14 = !{!"long", !7, i64 0}
!15 = distinct !{!15, !12}
!16 = !{!17, !17, i64 0}
!17 = !{!"__int128", !7, i64 0}
!18 = distinct !{!18, !12}
!19 = !{!20, !20, i64 0}
!20 = !{!"double", !7, i64 0}
!21 = distinct !{!21, !12}
!22 = !{!23, !23, i64 0}
!23 = !{!"any pointer", !7, i64 0}
!24 = distinct !{!24, !12}
!25 = !{!"cntvct_el0"}
!26 = distinct !{!26, !12}
!27 = distinct !{!27, !12}
!28 = !{!"CNTVCT_EL0"}
!29 = distinct !{!29, !12}
