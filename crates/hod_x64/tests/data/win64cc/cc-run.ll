;@ covers: kiln tests/conformance/dcl.attr.cconv/cc-run.cpp
;@ target: x86_64
;@ llvm: 22
;@ link: c++ m
;@ stdout: pm 21 pa 3.5 ms 60 msr 44 v 7 l 9
;@ stdout: t 42
; WP-246: a copy of the corpus entry (written by `cargo xtask corpus
; capture`) with one edit: `x86_regcallcc` and `x86_vectorcallcc`, which
; hod refuses by design, are removed from the two definitions and the two
; calls, so the module is all conventions hod builds; the output is the
; same.
; source: kiln tests/conformance/dcl.attr.cconv/cc-run.cpp
; producer-args: -std=c++26 -fno-crash-diagnostics -fclang-compat=22 --target=x86_64-unknown-linux-gnu -O1 -fdebug-prefix-map=<cwd>=. tests/conformance/dcl.attr.cconv/cc-run.cpp
; delivered: bitcode
; llc-args: -O1 -relocation-model=pic -filetype=obj
; ModuleID = '<stdin>'
source_filename = "tests/conformance/dcl.attr.cconv/cc-run.cpp"
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128"
target triple = "x86_64-unknown-linux-gnu"

%struct.Big = type { i64, i64, i64, i64, i64 }
%struct.Q = type { i32, double }
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
define dso_local preserve_allcc noundef double @_Z2padf1Q(double noundef %0, float noundef %1, i32 %2, double %3) local_unnamed_addr #0 {
  %5 = fpext float %1 to double
  %6 = fadd double %0, %5
  %7 = fadd double %6, %3
  %8 = sitofp i32 %2 to double
  %9 = fsub double %7, %8
  ret double %9
}

; Function Attrs: mustprogress nofree noinline norecurse nosync nounwind willreturn memory(argmem: read) uwtable
define dso_local win64cc noundef i64 @_Z2ms1Q3Bigd(ptr dead_on_return noundef readonly captures(none) %0, ptr dead_on_return noundef readonly captures(none) %1, double noundef %2) local_unnamed_addr #1 {
  %4 = load i32, ptr %0, align 8, !tbaa !9
  %5 = sext i32 %4 to i64
  %6 = getelementptr inbounds nuw i8, ptr %0, i64 8
  %7 = load double, ptr %6, align 8, !tbaa !12
  %8 = fptosi double %7 to i64
  %9 = load i64, ptr %1, align 8, !tbaa !13
  %10 = getelementptr inbounds nuw i8, ptr %1, i64 32
  %11 = load i64, ptr %10, align 8, !tbaa !16
  %12 = fptosi double %2 to i64
  %13 = add i64 %5, %12
  %14 = add i64 %13, %8
  %15 = add i64 %14, %9
  %16 = add i64 %15, %11
  ret i64 %16
}

; Function Attrs: mustprogress nofree noinline norecurse nosync nounwind willreturn memory(argmem: write) uwtable
define dso_local win64cc void @_Z3msrl(ptr dead_on_unwind noalias writable writeonly sret(%struct.Big) align 8 captures(none) initializes((0, 40)) %0, i64 noundef %1) local_unnamed_addr #2 {
  store i64 %1, ptr %0, align 8, !tbaa !13
  %3 = getelementptr inbounds nuw i8, ptr %0, i64 8
  store i64 %1, ptr %3, align 8, !tbaa !17
  %4 = getelementptr inbounds nuw i8, ptr %0, i64 16
  store i64 %1, ptr %4, align 8, !tbaa !18
  %5 = getelementptr inbounds nuw i8, ptr %0, i64 24
  store i64 %1, ptr %5, align 8, !tbaa !19
  %6 = getelementptr inbounds nuw i8, ptr %0, i64 32
  %7 = mul nsw i64 %1, 40
  store i64 %7, ptr %6, align 8, !tbaa !16
  ret void
}

; Function Attrs: mustprogress nofree noinline norecurse nosync nounwind willreturn memory(none) uwtable
define dso_local noundef double @_Z14__regcall3__t1dd1Q3Big(double noundef %0, double noundef %1, i32 %2, double %3, i64 %4, i64 %5, i64 %6, i64 %7, i64 %8) local_unnamed_addr #0 {
  %10 = sitofp i32 %2 to double
  %11 = tail call double @llvm.fmuladd.f64(double %0, double %1, double %10)
  %12 = fadd double %3, %11
  %13 = sitofp i64 %6 to double
  %14 = fadd double %12, %13
  ret double %14
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
  %2 = alloca %struct.Q, align 8
  %3 = alloca %struct.Big, align 8
  %4 = alloca %struct.Big, align 8
  %5 = alloca %struct.W, align 8
  call void @llvm.lifetime.start.p0(ptr nonnull %1)
  store volatile i32 1, ptr %1, align 4, !tbaa !5
  %.0..0..0..0.16 = load volatile i32, ptr %1, align 4, !tbaa !5
  %6 = tail call preserve_mostcc noundef i32 @_Z2pmiiiiii(i32 noundef %.0..0..0..0.16, i32 noundef 2, i32 noundef 3, i32 noundef 4, i32 noundef 5, i32 noundef 6)
  %7 = tail call preserve_allcc noundef double @_Z2padf1Q(double noundef 1.500000e+00, float noundef 2.000000e+00, i32 1, double 1.000000e+00)
  %.sroa.610.0..sroa_idx = getelementptr inbounds nuw i8, ptr %3, i64 8
  call void @llvm.memset.p0.i64(ptr noundef nonnull align 8 dereferenceable(24) %.sroa.610.0..sroa_idx, i8 0, i64 24, i1 false)
  store i32 3, ptr %2, align 8, !tbaa !9
  %8 = getelementptr inbounds nuw i8, ptr %2, i64 8
  store double 4.500000e+00, ptr %8, align 8, !tbaa !12
  store i64 10, ptr %3, align 8, !tbaa !20
  %.sroa.611.0..sroa_idx = getelementptr inbounds nuw i8, ptr %3, i64 32
  store i64 20, ptr %.sroa.611.0..sroa_idx, align 8, !tbaa !20
  %9 = call win64cc noundef i64 @_Z2ms1Q3Bigd(ptr dead_on_return noundef nonnull %2, ptr dead_on_return noundef nonnull %3, double noundef 2.300000e+01)
  call void @llvm.lifetime.start.p0(ptr nonnull %4) #12
  call void @llvm.memset.p0.i64(ptr noundef nonnull align 8 dereferenceable(40) %4, i8 0, i64 40, i1 false), !annotation !21
  %.0..0..0..0.17 = load volatile i32, ptr %1, align 4, !tbaa !5
  %10 = sext i32 %.0..0..0..0.17 to i64
  call win64cc void @_Z3msrl(ptr dead_on_unwind nonnull writable sret(%struct.Big) align 8 %4, i64 noundef %10)
  call void @llvm.lifetime.start.p0(ptr nonnull %5) #12
  store i64 0, ptr %5, align 8, !annotation !21
  store ptr getelementptr inbounds nuw inrange(-16, 24) (i8, ptr @_ZTV1W, i64 16), ptr %5, align 8, !tbaa !22
  %11 = call preserve_mostcc noundef i32 @_ZN1W1vEi(ptr noundef nonnull align 8 dereferenceable(8) %5, i32 noundef 5)
  %12 = load i64, ptr %4, align 8, !tbaa !13
  %13 = getelementptr inbounds nuw i8, ptr %4, i64 32
  %14 = load i64, ptr %13, align 8, !tbaa !16
  %15 = add i64 %12, 3
  %16 = add i64 %15, %14
  %17 = call i32 (ptr, ...) @printf(ptr noundef nonnull dereferenceable(1) @.str, i32 noundef %6, double noundef %7, i64 noundef %9, i64 noundef %16, i32 noundef %11, i32 noundef 9)
  %18 = call noundef double @_Z14__regcall3__t1dd1Q3Big(double noundef 4.000000e+00, double noundef 5.000000e+00, i32 10, double 2.000000e+00, i64 poison, i64 poison, i64 10, i64 poison, i64 poison)
  %19 = call noundef double @_Z2t2dd(double noundef 1.000000e+00, double noundef 1.000000e+00)
  %20 = fadd double %18, %19
  %21 = fptosi double %20 to i32
  %22 = call i32 (ptr, ...) @printf(ptr noundef nonnull dereferenceable(1) @.str.1, i32 noundef %21)
  call void @llvm.lifetime.end.p0(ptr nonnull %5) #12
  call void @llvm.lifetime.end.p0(ptr nonnull %4) #12
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
define linkonce_odr dso_local void @_ZN1VD2Ev(ptr noundef nonnull align 8 dereferenceable(8) %0) unnamed_addr #8 comdat align 2 {
  ret void
}

; Function Attrs: mustprogress noinline nounwind uwtable
define linkonce_odr dso_local preserve_mostcc noundef i32 @_ZN1W1vEi(ptr noundef nonnull align 8 dereferenceable(8) %0, i32 noundef %1) unnamed_addr #9 comdat align 2 {
  %3 = add nsw i32 %1, 2
  ret i32 %3
}

; Function Attrs: inlinehint mustprogress nounwind uwtable
define linkonce_odr dso_local void @_ZN1WD0Ev(ptr noundef nonnull align 8 dereferenceable(8) %0) unnamed_addr #10 comdat align 2 {
  tail call void @_ZdlPvm(ptr noundef %0, i64 noundef 8) #13
  ret void
}

; Function Attrs: nobuiltin nounwind
declare void @_ZdlPvm(ptr noundef, i64 noundef) local_unnamed_addr #11

attributes #0 = { mustprogress nofree noinline norecurse nosync nounwind willreturn memory(none) uwtable "min-legal-vector-width"="0" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="x86-64" "target-features"="+cmov,+cx8,+fxsr,+mmx,+sse,+sse2,+x87" "tune-cpu"="generic" }
attributes #1 = { mustprogress nofree noinline norecurse nosync nounwind willreturn memory(argmem: read) uwtable "min-legal-vector-width"="0" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="x86-64" "target-features"="+cmov,+cx8,+fxsr,+mmx,+sse,+sse2,+x87" "tune-cpu"="generic" }
attributes #2 = { mustprogress nofree noinline norecurse nosync nounwind willreturn memory(argmem: write) uwtable "min-legal-vector-width"="0" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="x86-64" "target-features"="+cmov,+cx8,+fxsr,+mmx,+sse,+sse2,+x87" "tune-cpu"="generic" }
attributes #3 = { nocallback nocreateundeforpoison nofree nosync nounwind speculatable willreturn memory(none) }
attributes #4 = { mustprogress norecurse nounwind uwtable "min-legal-vector-width"="0" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="x86-64" "target-features"="+cmov,+cx8,+fxsr,+mmx,+sse,+sse2,+x87" "tune-cpu"="generic" }
attributes #5 = { nocallback nofree nosync nounwind willreturn memory(argmem: readwrite) }
attributes #6 = { nocallback nofree nounwind willreturn memory(argmem: write) }
attributes #7 = { nofree nounwind "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="x86-64" "target-features"="+cmov,+cx8,+fxsr,+mmx,+sse,+sse2,+x87" "tune-cpu"="generic" }
attributes #8 = { mustprogress nounwind uwtable "min-legal-vector-width"="0" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="x86-64" "target-features"="+cmov,+cx8,+fxsr,+mmx,+sse,+sse2,+x87" "tune-cpu"="generic" }
attributes #9 = { mustprogress noinline nounwind uwtable "min-legal-vector-width"="0" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="x86-64" "target-features"="+cmov,+cx8,+fxsr,+mmx,+sse,+sse2,+x87" "tune-cpu"="generic" }
attributes #10 = { inlinehint mustprogress nounwind uwtable "min-legal-vector-width"="0" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="x86-64" "target-features"="+cmov,+cx8,+fxsr,+mmx,+sse,+sse2,+x87" "tune-cpu"="generic" }
attributes #11 = { nobuiltin nounwind "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="x86-64" "target-features"="+cmov,+cx8,+fxsr,+mmx,+sse,+sse2,+x87" "tune-cpu"="generic" }
attributes #12 = { nounwind }
attributes #13 = { builtin nounwind }

!llvm.module.flags = !{!0, !1, !2, !3}
!llvm.ident = !{!4}
!llvm.errno.tbaa = !{!5}

!0 = !{i32 1, !"wchar_size", i32 4}
!1 = !{i32 8, !"PIC Level", i32 2}
!2 = !{i32 7, !"PIE Level", i32 2}
!3 = !{i32 7, !"uwtable", i32 2}
!4 = !{!"kiln version 1.2.0"}
!5 = !{!6, !6, i64 0}
!6 = !{!"int", !7, i64 0}
!7 = !{!"omnipotent char", !8, i64 0}
!8 = !{!"Simple C++ TBAA"}
!9 = !{!10, !6, i64 0}
!10 = !{!"_ZTS1Q", !6, i64 0, !11, i64 8}
!11 = !{!"double", !7, i64 0}
!12 = !{!10, !11, i64 8}
!13 = !{!14, !15, i64 0}
!14 = !{!"_ZTS3Big", !15, i64 0, !15, i64 8, !15, i64 16, !15, i64 24, !15, i64 32}
!15 = !{!"long", !7, i64 0}
!16 = !{!14, !15, i64 32}
!17 = !{!14, !15, i64 8}
!18 = !{!14, !15, i64 16}
!19 = !{!14, !15, i64 24}
!20 = !{!15, !15, i64 0}
!21 = !{!"auto-init"}
!22 = !{!23, !23, i64 0}
!23 = !{!"vtable pointer", !8, i64 0}
