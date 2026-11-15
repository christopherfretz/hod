;@ covers: kiln tests/conformance/dcl.attr.cconv/cc-run.cpp
;@ target: aarch64
;@ llvm: 22
;@ link: c++ m
;@ stdout: pm 21 pa 3.5 ms 60 msr 44 v 7 l 9
;@ stdout: t 42
; WP-247: an unedited copy of the corpus entry (written by `cargo xtask
; corpus capture`), which uses `win64cc`, `aarch64_vector_pcs`,
; `preserve_mostcc` and `preserve_allcc`.
; source: kiln tests/conformance/dcl.attr.cconv/cc-run.cpp
; producer-args: -std=c++26 -fno-crash-diagnostics -fclang-compat=22 -O1 -fdebug-prefix-map=<cwd>=. tests/conformance/dcl.attr.cconv/cc-run.cpp
; delivered: bitcode
; llc-args: -O1 -relocation-model=pic -filetype=obj
; ModuleID = '<stdin>'
source_filename = "tests/conformance/dcl.attr.cconv/cc-run.cpp"
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i8:8:32-i16:16:32-i64:64-i128:128-n32:64-S128-Fn32"
target triple = "aarch64-unknown-linux-gnu"

%struct.Big = type { i64, i64, i64, i64, i64 }
%struct.W = type { %struct.V }
%struct.V = type { ptr }

$_ZN1VD2Ev = comdat any

$_ZN1W1vEi = comdat any

$_ZN1WD0Ev = comdat any

$_ZTV1W = comdat any

$_ZTI1W = comdat any

$_ZTS1W = comdat any

$_ZTI1V = comdat any

$_ZTS1V = comdat any

@.str = private unnamed_addr constant [40 x i8] c"pm %d pa %.1f ms %ld msr %ld v %d l %d\0A\00", align 1
@.str.1 = private unnamed_addr constant [6 x i8] c"t %d\0A\00", align 1
@_ZTV1W = linkonce_odr dso_local unnamed_addr constant { [5 x ptr] } { [5 x ptr] [ptr null, ptr @_ZTI1W, ptr @_ZN1W1vEi, ptr @_ZN1VD2Ev, ptr @_ZN1WD0Ev] }, comdat, align 8
@_ZTI1W = linkonce_odr dso_local constant { ptr, ptr, ptr } { ptr getelementptr inbounds (ptr, ptr @_ZTVN10__cxxabiv120__si_class_type_infoE, i64 2), ptr @_ZTS1W, ptr @_ZTI1V }, comdat, align 8
@_ZTVN10__cxxabiv120__si_class_type_infoE = external global [0 x ptr]
@_ZTS1W = linkonce_odr dso_local constant [3 x i8] c"1W\00", comdat, align 1
@_ZTI1V = linkonce_odr dso_local constant { ptr, ptr } { ptr getelementptr inbounds (ptr, ptr @_ZTVN10__cxxabiv117__class_type_infoE, i64 2), ptr @_ZTS1V }, comdat, align 8
@_ZTVN10__cxxabiv117__class_type_infoE = external global [0 x ptr]
@_ZTS1V = linkonce_odr dso_local constant [3 x i8] c"1V\00", comdat, align 1

; Function Attrs: mustprogress nofree noinline norecurse nosync nounwind willreturn memory(none) uwtable
define dso_local preserve_mostcc noundef i32 @_Z2pmiiiiii(i32 noundef %0, i32 noundef %1, i32 noundef %2, i32 noundef %3, i32 noundef %4, i32 noundef %5) local_unnamed_addr #0 {
  %7 = add nsw i32 %1, %0
  %8 = add nsw i32 %7, %2
  %9 = add nsw i32 %8, %3
  %10 = add nsw i32 %9, %4
  %11 = add nsw i32 %10, %5
  ret i32 %11
}

; Function Attrs: mustprogress nofree noinline norecurse nosync nounwind willreturn memory(none) uwtable
define dso_local preserve_allcc noundef double @_Z2padf1Q(double noundef %0, float noundef %1, [2 x i64] %2) local_unnamed_addr #0 {
  %.fca.0.extract = extractvalue [2 x i64] %2, 0
  %.sroa.0.0.extract.trunc = trunc i64 %.fca.0.extract to i32
  %.fca.1.extract = extractvalue [2 x i64] %2, 1
  %4 = bitcast i64 %.fca.1.extract to double
  %5 = fpext float %1 to double
  %6 = fadd double %0, %5
  %7 = fadd double %6, %4
  %8 = sitofp i32 %.sroa.0.0.extract.trunc to double
  %9 = fsub double %7, %8
  ret double %9
}

; Function Attrs: mustprogress nofree noinline norecurse nosync nounwind willreturn memory(argmem: read) uwtable
define dso_local win64cc noundef i64 @_Z2ms1Q3Bigd([2 x i64] %0, ptr dead_on_return noundef readonly captures(none) %1, double noundef %2) local_unnamed_addr #1 {
  %.fca.0.extract = extractvalue [2 x i64] %0, 0
  %.fca.1.extract = extractvalue [2 x i64] %0, 1
  %4 = bitcast i64 %.fca.1.extract to double
  %sext = shl i64 %.fca.0.extract, 32
  %5 = ashr exact i64 %sext, 32
  %6 = fptosi double %4 to i64
  %7 = add nsw i64 %5, %6
  %8 = load i64, ptr %1, align 8, !tbaa !10
  %9 = getelementptr inbounds nuw i8, ptr %1, i64 32
  %10 = load i64, ptr %9, align 8, !tbaa !13
  %11 = fptosi double %2 to i64
  %12 = add i64 %7, %11
  %13 = add i64 %12, %8
  %14 = add i64 %13, %10
  ret i64 %14
}

; Function Attrs: mustprogress nofree noinline norecurse nosync nounwind willreturn memory(argmem: write) uwtable
define dso_local win64cc void @_Z3msrl(ptr dead_on_unwind noalias writable writeonly sret(%struct.Big) align 8 captures(none) initializes((0, 40)) %0, i64 noundef %1) local_unnamed_addr #2 {
  store i64 %1, ptr %0, align 8, !tbaa !10
  %3 = getelementptr inbounds nuw i8, ptr %0, i64 8
  store i64 %1, ptr %3, align 8, !tbaa !14
  %4 = getelementptr inbounds nuw i8, ptr %0, i64 16
  store i64 %1, ptr %4, align 8, !tbaa !15
  %5 = getelementptr inbounds nuw i8, ptr %0, i64 24
  store i64 %1, ptr %5, align 8, !tbaa !16
  %6 = getelementptr inbounds nuw i8, ptr %0, i64 32
  %7 = mul nsw i64 %1, 40
  store i64 %7, ptr %6, align 8, !tbaa !13
  ret void
}

; Function Attrs: mustprogress nofree noinline norecurse nosync nounwind willreturn memory(argmem: read) uwtable
define dso_local aarch64_vector_pcs noundef double @_Z2t1dd1Q3Big(double noundef %0, double noundef %1, [2 x i64] %2, ptr dead_on_return noundef readonly captures(none) %3) local_unnamed_addr #1 {
  %.fca.0.extract = extractvalue [2 x i64] %2, 0
  %.sroa.0.0.extract.trunc = trunc i64 %.fca.0.extract to i32
  %.fca.1.extract = extractvalue [2 x i64] %2, 1
  %5 = bitcast i64 %.fca.1.extract to double
  %6 = sitofp i32 %.sroa.0.0.extract.trunc to double
  %7 = tail call double @llvm.fmuladd.f64(double %0, double %1, double %6)
  %8 = fadd double %7, %5
  %9 = getelementptr inbounds nuw i8, ptr %3, i64 16
  %10 = load i64, ptr %9, align 8, !tbaa !15
  %11 = sitofp i64 %10 to double
  %12 = fadd double %8, %11
  ret double %12
}

; Function Attrs: nocallback nocreateundeforpoison nofree nosync nounwind speculatable willreturn memory(none)
declare double @llvm.fmuladd.f64(double, double, double) #3

; Function Attrs: mustprogress nofree noinline norecurse nosync nounwind willreturn memory(none) uwtable
define dso_local noundef double @_Z2t2dd(double noundef %0, double noundef %1) local_unnamed_addr #0 {
  %3 = fsub double %0, %1
  ret double %3
}

; Function Attrs: mustprogress norecurse nounwind uwtable
define dso_local noundef i32 @main() local_unnamed_addr #4 personality ptr @__gxx_personality_v0 {
  %1 = alloca i32, align 4
  %2 = alloca %struct.Big, align 8
  %3 = alloca %struct.Big, align 8
  %4 = alloca %struct.W, align 8
  %5 = alloca %struct.Big, align 8
  call void @llvm.lifetime.start.p0(ptr nonnull %1)
  store volatile i32 1, ptr %1, align 4, !tbaa !6
  %.0..0..0..0.22 = load volatile i32, ptr %1, align 4, !tbaa !6
  %6 = tail call preserve_mostcc noundef i32 @_Z2pmiiiiii(i32 noundef %.0..0..0..0.22, i32 noundef 2, i32 noundef 3, i32 noundef 4, i32 noundef 5, i32 noundef 6)
  %7 = tail call preserve_allcc noundef double @_Z2padf1Q(double noundef 1.500000e+00, float noundef 2.000000e+00, [2 x i64] [i64 1, i64 4607182418800017408])
  %.sroa.6.0..sroa_idx = getelementptr inbounds nuw i8, ptr %2, i64 8
  call void @llvm.memset.p0.i64(ptr noundef nonnull align 8 dereferenceable(24) %.sroa.6.0..sroa_idx, i8 0, i64 24, i1 false)
  store i64 10, ptr %2, align 8, !tbaa !17
  %.sroa.614.0..sroa_idx = getelementptr inbounds nuw i8, ptr %2, i64 32
  store i64 20, ptr %.sroa.614.0..sroa_idx, align 8, !tbaa !17
  %8 = call win64cc noundef i64 @_Z2ms1Q3Bigd([2 x i64] [i64 3, i64 4616752568008179712], ptr dead_on_return noundef nonnull %2, double noundef 2.300000e+01)
  call void @llvm.lifetime.start.p0(ptr nonnull %3) #12
  call void @llvm.memset.p0.i64(ptr noundef nonnull align 8 dereferenceable(40) %3, i8 0, i64 40, i1 false), !annotation !18
  %.0..0..0..0.23 = load volatile i32, ptr %1, align 4, !tbaa !6
  %9 = sext i32 %.0..0..0..0.23 to i64
  call win64cc void @_Z3msrl(ptr dead_on_unwind nonnull writable sret(%struct.Big) align 8 %3, i64 noundef %9)
  call void @llvm.lifetime.start.p0(ptr nonnull %4) #12
  store i64 0, ptr %4, align 8, !annotation !18
  store ptr getelementptr inbounds nuw inrange(-16, 24) (i8, ptr @_ZTV1W, i64 16), ptr %4, align 8, !tbaa !19
  %10 = call preserve_mostcc noundef i32 @_ZN1W1vEi(ptr noundef nonnull align 8 dereferenceable(8) %4, i32 noundef 5)
  %11 = load i64, ptr %3, align 8, !tbaa !10
  %12 = getelementptr inbounds nuw i8, ptr %3, i64 32
  %13 = load i64, ptr %12, align 8, !tbaa !13
  %14 = add i64 %11, 3
  %15 = add i64 %14, %13
  %16 = call i32 (ptr, ...) @printf(ptr noundef nonnull dereferenceable(1) @.str, i32 noundef %6, double noundef %7, i64 noundef %8, i64 noundef %15, i32 noundef %10, i32 noundef 9)
  call void @llvm.memset.p0.i64(ptr noundef nonnull align 8 dereferenceable(40) %5, i8 0, i64 40, i1 false)
  %17 = getelementptr inbounds nuw i8, ptr %5, i64 16
  store i64 10, ptr %17, align 8, !tbaa !15
  %18 = call aarch64_vector_pcs noundef double @_Z2t1dd1Q3Big(double noundef 4.000000e+00, double noundef 5.000000e+00, [2 x i64] [i64 10, i64 4611686018427387904], ptr dead_on_return noundef nonnull %5)
  %19 = call noundef double @_Z2t2dd(double noundef 1.000000e+00, double noundef 1.000000e+00)
  %20 = fadd double %18, %19
  %21 = fptosi double %20 to i32
  %22 = call i32 (ptr, ...) @printf(ptr noundef nonnull dereferenceable(1) @.str.1, i32 noundef %21)
  call void @llvm.lifetime.end.p0(ptr nonnull %4) #12
  call void @llvm.lifetime.end.p0(ptr nonnull %3) #12
  call void @llvm.lifetime.end.p0(ptr nonnull %1)
  ret i32 0
}

; Function Attrs: nocallback nofree nosync nounwind willreturn memory(argmem: readwrite)
declare void @llvm.lifetime.start.p0(ptr captures(none)) #5

; Function Attrs: nocallback nofree nounwind willreturn memory(argmem: write)
declare void @llvm.memset.p0.i64(ptr writeonly captures(none), i8, i64, i1 immarg) #6

declare i32 @__gxx_personality_v0(...)

; Function Attrs: nofree nounwind
declare noundef i32 @printf(ptr noundef readonly captures(none), ...) local_unnamed_addr #7

; Function Attrs: nocallback nofree nosync nounwind willreturn memory(argmem: readwrite)
declare void @llvm.lifetime.end.p0(ptr captures(none)) #5

; Function Attrs: mustprogress nounwind uwtable
define linkonce_odr dso_local void @_ZN1VD2Ev(ptr noundef nonnull align 8 dereferenceable(8) %0) unnamed_addr #8 comdat {
  ret void
}

; Function Attrs: mustprogress noinline nounwind uwtable
define linkonce_odr dso_local preserve_mostcc noundef i32 @_ZN1W1vEi(ptr noundef nonnull align 8 dereferenceable(8) %0, i32 noundef %1) unnamed_addr #9 comdat {
  %3 = add nsw i32 %1, 2
  ret i32 %3
}

; Function Attrs: inlinehint mustprogress nounwind uwtable
define linkonce_odr dso_local void @_ZN1WD0Ev(ptr noundef nonnull align 8 dereferenceable(8) %0) unnamed_addr #10 comdat {
  tail call void @_ZdlPvm(ptr noundef %0, i64 noundef 8) #13
  ret void
}

; Function Attrs: nobuiltin nounwind
declare void @_ZdlPvm(ptr noundef, i64 noundef) local_unnamed_addr #11

attributes #0 = { mustprogress nofree noinline norecurse nosync nounwind willreturn memory(none) uwtable "frame-pointer"="non-leaf-no-reserve" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #1 = { mustprogress nofree noinline norecurse nosync nounwind willreturn memory(argmem: read) uwtable "frame-pointer"="non-leaf-no-reserve" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #2 = { mustprogress nofree noinline norecurse nosync nounwind willreturn memory(argmem: write) uwtable "frame-pointer"="non-leaf-no-reserve" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #3 = { nocallback nocreateundeforpoison nofree nosync nounwind speculatable willreturn memory(none) }
attributes #4 = { mustprogress norecurse nounwind uwtable "frame-pointer"="non-leaf-no-reserve" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #5 = { nocallback nofree nosync nounwind willreturn memory(argmem: readwrite) }
attributes #6 = { nocallback nofree nounwind willreturn memory(argmem: write) }
attributes #7 = { nofree nounwind "frame-pointer"="non-leaf-no-reserve" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #8 = { mustprogress nounwind uwtable "frame-pointer"="non-leaf-no-reserve" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #9 = { mustprogress noinline nounwind uwtable "frame-pointer"="non-leaf-no-reserve" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #10 = { inlinehint mustprogress nounwind uwtable "frame-pointer"="non-leaf-no-reserve" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #11 = { nobuiltin nounwind "frame-pointer"="non-leaf-no-reserve" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #12 = { nounwind }
attributes #13 = { builtin nounwind }

!llvm.module.flags = !{!0, !1, !2, !3, !4}
!llvm.ident = !{!5}
!llvm.errno.tbaa = !{!6}

!0 = !{i32 1, !"wchar_size", i32 4}
!1 = !{i32 8, !"PIC Level", i32 2}
!2 = !{i32 7, !"PIE Level", i32 2}
!3 = !{i32 7, !"uwtable", i32 2}
!4 = !{i32 7, !"frame-pointer", i32 4}
!5 = !{!"kiln version 1.2.0"}
!6 = !{!7, !7, i64 0}
!7 = !{!"int", !8, i64 0}
!8 = !{!"omnipotent char", !9, i64 0}
!9 = !{!"Simple C++ TBAA"}
!10 = !{!11, !12, i64 0}
!11 = !{!"_ZTS3Big", !12, i64 0, !12, i64 8, !12, i64 16, !12, i64 24, !12, i64 32}
!12 = !{!"long", !8, i64 0}
!13 = !{!11, !12, i64 32}
!14 = !{!11, !12, i64 8}
!15 = !{!11, !12, i64 16}
!16 = !{!11, !12, i64 24}
!17 = !{!12, !12, i64 0}
!18 = !{!"auto-init"}
!19 = !{!20, !20, i64 0}
!20 = !{!"vtable pointer", !9, i64 0}
