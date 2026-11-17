; clang++-19 -std=c++20 -O0 -g -S -emit-llvm -Xclang -disable-llvm-passes
; (aarch64-unknown-linux-gnu; the target triple, datalayout, target-cpu and
; target-features removed, so that the seed runs on every target) of:
;
; // A generator whose class parameter is passed by address (non-trivial copy
; // and destructor), compiled with -g: the parameter's debug location.
; #include <coroutine>
; #include <cstdio>
; struct big {
;   int a[4];
;   explicit big(int s) { for (int i = 0; i < 4; ++i) a[i] = s + i; }
;   big(const big& o) { for (int i = 0; i < 4; ++i) a[i] = o.a[i]; }
;   ~big() { std::printf("gone %d\n", a[0]); }
; };
; struct gen {
;   struct promise_type {
;     int v = 0;
;     gen get_return_object() { return gen{std::coroutine_handle<promise_type>::from_promise(*this)}; }
;     std::suspend_always initial_suspend() noexcept { return {}; }
;     std::suspend_always final_suspend() noexcept { return {}; }
;     std::suspend_always yield_value(int x) noexcept { v = x; return {}; }
;     void return_void() noexcept {}
;     void unhandled_exception() noexcept {}
;   };
;   std::coroutine_handle<promise_type> h;
; };
; gen count(big b, int n) {
;   for (int i = 0; i < n; ++i) co_yield b.a[i] * 10 + n;
; }
; int main() {
;   gen g = count(big(3), 3);
;   for (g.h.resume(); !g.h.done(); g.h.resume()) std::printf("%d\n", g.h.promise().v);
;   g.h.destroy();
;   return 0;
; }
;
; WP-350: debug information of a by-address coroutine parameter.
;@ covers: spec/ir.md §5.5 intrinsics (coroutine bodies; spec/codegen.md §7.6)
;@ kept-off(i686): lp64-abi
;@ input: pre-opt
;@ dialect: 19
;@ link: c++
;@ stdout: gone 3
;@ stdout: 33
;@ stdout: 43
;@ stdout: 53
;@ stdout: gone 3
; ModuleID = 'byaddr.cpp'
source_filename = "byaddr.cpp"

%struct.gen = type { %"struct.std::__n4861::coroutine_handle" }
%"struct.std::__n4861::coroutine_handle" = type { ptr }
%struct.big = type { [4 x i32] }
%"struct.gen::promise_type" = type { i32 }
%"struct.std::__n4861::suspend_always" = type { i8 }
%"struct.std::__n4861::coroutine_handle.0" = type { ptr }

$_ZN3bigC2ERKS_ = comdat any

$_ZN3gen12promise_typeC2Ev = comdat any

$_ZN3gen12promise_type17get_return_objectEv = comdat any

$_ZN3gen12promise_type15initial_suspendEv = comdat any

$_ZNKSt7__n486114suspend_always11await_readyEv = comdat any

$_ZNKSt7__n486114suspend_always13await_suspendENS_16coroutine_handleIvEE = comdat any

$_ZNSt7__n486116coroutine_handleIN3gen12promise_typeEE12from_addressEPv = comdat any

$_ZNKSt7__n486116coroutine_handleIN3gen12promise_typeEEcvNS0_IvEEEv = comdat any

$_ZNKSt7__n486114suspend_always12await_resumeEv = comdat any

$_ZN3gen12promise_type11yield_valueEi = comdat any

$_ZN3gen12promise_type11return_voidEv = comdat any

$_ZN3gen12promise_type13final_suspendEv = comdat any

$_ZN3bigD2Ev = comdat any

$_ZN3bigC2Ei = comdat any

$_ZNKSt7__n486116coroutine_handleIN3gen12promise_typeEE6resumeEv = comdat any

$_ZNKSt7__n486116coroutine_handleIN3gen12promise_typeEE4doneEv = comdat any

$_ZNKSt7__n486116coroutine_handleIN3gen12promise_typeEE7promiseEv = comdat any

$_ZNKSt7__n486116coroutine_handleIN3gen12promise_typeEE7destroyEv = comdat any

$_ZNSt7__n486116coroutine_handleIN3gen12promise_typeEE12from_promiseERS2_ = comdat any

$_ZNSt7__n486116coroutine_handleIN3gen12promise_typeEEC2Ev = comdat any

$_ZNSt7__n486116coroutine_handleIvE12from_addressEPv = comdat any

$_ZNKSt7__n486116coroutine_handleIN3gen12promise_typeEE7addressEv = comdat any

$_ZNSt7__n486116coroutine_handleIvEC2Ev = comdat any

$__clang_call_terminate = comdat any

@.str = private unnamed_addr constant [4 x i8] c"%d\0A\00", align 1, !dbg !0
@.str.1 = private unnamed_addr constant [9 x i8] c"gone %d\0A\00", align 1, !dbg !8

; Function Attrs: mustprogress noinline optnone presplitcoroutine uwtable
define dso_local i64 @_Z5count3bigi(ptr noundef %0, i32 noundef %1) #0 personality ptr @__gxx_personality_v0 !dbg !355 {
  %3 = alloca %struct.gen, align 8
  %4 = alloca ptr, align 8
  %5 = alloca i32, align 4
  %6 = alloca %struct.big, align 4
  %7 = alloca ptr, align 8
  %8 = alloca i32, align 4
  %9 = alloca i32, align 4
  %10 = alloca %"struct.gen::promise_type", align 4
  %11 = alloca %"struct.std::__n4861::suspend_always", align 1
  %12 = alloca %"struct.std::__n4861::suspend_always", align 1
  %13 = alloca i32, align 4
  %14 = alloca %"struct.std::__n4861::suspend_always", align 1
  %15 = alloca %"struct.std::__n4861::suspend_always", align 1
  %16 = alloca %"struct.std::__n4861::suspend_always", align 1
  %17 = alloca %"struct.std::__n4861::suspend_always", align 1
  store ptr %0, ptr %4, align 8
    #dbg_declare(ptr %4, !359, !DIExpression(DW_OP_deref), !360)
  store i32 %1, ptr %5, align 4
    #dbg_declare(ptr %5, !361, !DIExpression(), !362)
  %18 = bitcast ptr %10 to ptr
  %19 = call token @llvm.coro.id(i32 16, ptr %18, ptr null, ptr null), !dbg !363
  %20 = call i1 @llvm.coro.alloc(token %19), !dbg !363
  br i1 %20, label %21, label %24, !dbg !363

21:                                               ; preds = %2
  %22 = call i64 @llvm.coro.size.i64(), !dbg !364
  %23 = call noalias noundef nonnull ptr @_Znwm(i64 noundef %22) #16, !dbg !364
  br label %24, !dbg !363

24:                                               ; preds = %21, %2
  %25 = phi ptr [ null, %2 ], [ %23, %21 ], !dbg !363
  %26 = call ptr @llvm.coro.begin(token %19, ptr %25), !dbg !363
  call void @llvm.lifetime.start.p0(i64 16, ptr %6) #2, !dbg !365
    #dbg_declare(ptr %6, !359, !DIExpression(), !366)
  invoke void @_ZN3bigC2ERKS_(ptr noundef nonnull align 4 dereferenceable(16) %6, ptr noundef nonnull align 4 dereferenceable(16) %0)
          to label %27 unwind label %39, !dbg !365

27:                                               ; preds = %24
  call void @llvm.lifetime.start.p0(i64 4, ptr %9) #2, !dbg !365
    #dbg_declare(ptr %9, !361, !DIExpression(), !366)
  %28 = load i32, ptr %5, align 4, !dbg !365
  store i32 %28, ptr %9, align 4, !dbg !365
  call void @llvm.lifetime.start.p0(i64 4, ptr %10) #2, !dbg !364
    #dbg_declare(ptr %10, !367, !DIExpression(), !366)
  call void @_ZN3gen12promise_typeC2Ev(ptr noundef nonnull align 4 dereferenceable(4) %10) #2, !dbg !364
  %29 = invoke i64 @_ZN3gen12promise_type17get_return_objectEv(ptr noundef nonnull align 4 dereferenceable(4) %10)
          to label %30 unwind label %43, !dbg !364

30:                                               ; preds = %27
  %31 = getelementptr inbounds %struct.gen, ptr %3, i32 0, i32 0, !dbg !364
  %32 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %31, i32 0, i32 0, !dbg !364
  %33 = inttoptr i64 %29 to ptr, !dbg !364
  store ptr %33, ptr %32, align 8, !dbg !364
  call void @llvm.lifetime.start.p0(i64 1, ptr %11) #2, !dbg !364
  call void @_ZN3gen12promise_type15initial_suspendEv(ptr noundef nonnull align 4 dereferenceable(4) %10) #2, !dbg !364
  %34 = call noundef i1 @_ZNKSt7__n486114suspend_always11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %11) #2, !dbg !364
  br i1 %34, label %47, label %35, !dbg !364

35:                                               ; preds = %30
  %36 = call token @llvm.coro.save(ptr null), !dbg !364
  call void @llvm.coro.await.suspend.void(ptr %11, ptr %26, ptr @_Z5count3bigi.__await_suspend_wrapper__init) #2, !dbg !364
  %37 = call i8 @llvm.coro.suspend(token %36, i1 false), !dbg !364
  switch i8 %37, label %98 [
    i8 0, label %47
    i8 1, label %38
  ], !dbg !364

38:                                               ; preds = %35
  br label %48, !dbg !364

39:                                               ; preds = %24
  %40 = landingpad { ptr, i32 }
          cleanup, !dbg !373
  %41 = extractvalue { ptr, i32 } %40, 0, !dbg !373
  store ptr %41, ptr %7, align 8, !dbg !373
  %42 = extractvalue { ptr, i32 } %40, 1, !dbg !373
  store i32 %42, ptr %8, align 4, !dbg !373
  br label %104, !dbg !373

43:                                               ; preds = %27
  %44 = landingpad { ptr, i32 }
          cleanup, !dbg !373
  %45 = extractvalue { ptr, i32 } %44, 0, !dbg !373
  store ptr %45, ptr %7, align 8, !dbg !373
  %46 = extractvalue { ptr, i32 } %44, 1, !dbg !373
  store i32 %46, ptr %8, align 4, !dbg !373
  call void @llvm.lifetime.end.p0(i64 4, ptr %10) #2, !dbg !364
  call void @llvm.lifetime.end.p0(i64 4, ptr %9) #2, !dbg !364
  call void @_ZN3bigD2Ev(ptr noundef nonnull align 4 dereferenceable(16) %6) #2, !dbg !364
  br label %104, !dbg !364

47:                                               ; preds = %35, %30
  call void @_ZNKSt7__n486114suspend_always12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %11) #2, !dbg !364
  br label %48, !dbg !364

48:                                               ; preds = %47, %38
  %49 = phi i32 [ 0, %47 ], [ 2, %38 ]
  call void @llvm.lifetime.end.p0(i64 1, ptr %11) #2, !dbg !364
  switch i32 %49, label %90 [
    i32 0, label %50
  ]

50:                                               ; preds = %48
  call void @llvm.lifetime.start.p0(i64 4, ptr %13) #2, !dbg !374
    #dbg_declare(ptr %13, !377, !DIExpression(), !378)
  store i32 0, ptr %13, align 4, !dbg !378
  br label %51, !dbg !374

51:                                               ; preds = %74, %50
  %52 = load i32, ptr %13, align 4, !dbg !379
  %53 = load i32, ptr %9, align 4, !dbg !381
  %54 = icmp slt i32 %52, %53, !dbg !382
  br i1 %54, label %56, label %55, !dbg !383

55:                                               ; preds = %51
  br label %77, !dbg !383

56:                                               ; preds = %51
  call void @llvm.lifetime.start.p0(i64 1, ptr %14) #2, !dbg !384
  %57 = getelementptr inbounds %struct.big, ptr %6, i32 0, i32 0, !dbg !385
  %58 = load i32, ptr %13, align 4, !dbg !386
  %59 = sext i32 %58 to i64, !dbg !387
  %60 = getelementptr inbounds [4 x i32], ptr %57, i64 0, i64 %59, !dbg !387
  %61 = load i32, ptr %60, align 4, !dbg !387
  %62 = mul nsw i32 %61, 10, !dbg !388
  %63 = load i32, ptr %9, align 4, !dbg !389
  %64 = add nsw i32 %62, %63, !dbg !390
  call void @_ZN3gen12promise_type11yield_valueEi(ptr noundef nonnull align 4 dereferenceable(4) %10, i32 noundef %64) #2, !dbg !384
  %65 = call noundef i1 @_ZNKSt7__n486114suspend_always11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %14) #2, !dbg !384
  br i1 %65, label %70, label %66, !dbg !384

66:                                               ; preds = %56
  %67 = call token @llvm.coro.save(ptr null), !dbg !384
  call void @llvm.coro.await.suspend.void(ptr %14, ptr %26, ptr @_Z5count3bigi.__await_suspend_wrapper__yield) #2, !dbg !384
  %68 = call i8 @llvm.coro.suspend(token %67, i1 false), !dbg !384
  switch i8 %68, label %98 [
    i8 0, label %70
    i8 1, label %69
  ], !dbg !384

69:                                               ; preds = %66
  br label %71, !dbg !384

70:                                               ; preds = %66, %56
  call void @_ZNKSt7__n486114suspend_always12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %14) #2, !dbg !384
  br label %71, !dbg !384

71:                                               ; preds = %70, %69
  %72 = phi i32 [ 0, %70 ], [ 2, %69 ]
  call void @llvm.lifetime.end.p0(i64 1, ptr %14) #2, !dbg !384
  switch i32 %72, label %77 [
    i32 0, label %73
  ]

73:                                               ; preds = %71
  br label %74, !dbg !384

74:                                               ; preds = %73
  %75 = load i32, ptr %13, align 4, !dbg !391
  %76 = add nsw i32 %75, 1, !dbg !391
  store i32 %76, ptr %13, align 4, !dbg !391
  br label %51, !dbg !392, !llvm.loop !393

77:                                               ; preds = %71, %55
  %78 = phi i32 [ %72, %71 ], [ 4, %55 ]
  call void @llvm.lifetime.end.p0(i64 4, ptr %13) #2, !dbg !392
  switch i32 %78, label %90 [
    i32 4, label %79
  ]

79:                                               ; preds = %77
  call void @_ZN3gen12promise_type11return_voidEv(ptr noundef nonnull align 4 dereferenceable(4) %10) #2, !dbg !364
  br label %80, !dbg !364

80:                                               ; preds = %79
  call void @llvm.lifetime.start.p0(i64 1, ptr %16) #2, !dbg !364
  call void @_ZN3gen12promise_type13final_suspendEv(ptr noundef nonnull align 4 dereferenceable(4) %10) #2, !dbg !364
  %81 = call noundef i1 @_ZNKSt7__n486114suspend_always11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %16) #2, !dbg !364
  br i1 %81, label %86, label %82, !dbg !364

82:                                               ; preds = %80
  %83 = call token @llvm.coro.save(ptr null), !dbg !364
  call void @llvm.coro.await.suspend.void(ptr %16, ptr %26, ptr @_Z5count3bigi.__await_suspend_wrapper__final) #2, !dbg !364
  %84 = call i8 @llvm.coro.suspend(token %83, i1 true), !dbg !364
  switch i8 %84, label %98 [
    i8 0, label %86
    i8 1, label %85
  ], !dbg !364

85:                                               ; preds = %82
  br label %87, !dbg !364

86:                                               ; preds = %82, %80
  call void @_ZNKSt7__n486114suspend_always12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %16) #2, !dbg !364
  br label %87, !dbg !364

87:                                               ; preds = %86, %85
  %88 = phi i32 [ 0, %86 ], [ 2, %85 ]
  call void @llvm.lifetime.end.p0(i64 1, ptr %16) #2, !dbg !364
  switch i32 %88, label %90 [
    i32 0, label %89
  ]

89:                                               ; preds = %87
  br label %90, !dbg !364

90:                                               ; preds = %89, %87, %77, %48
  %91 = phi i32 [ %49, %48 ], [ %78, %77 ], [ %88, %87 ], [ 0, %89 ], !dbg !366
  call void @llvm.lifetime.end.p0(i64 4, ptr %10) #2, !dbg !364
  call void @llvm.lifetime.end.p0(i64 4, ptr %9) #2, !dbg !364
  call void @_ZN3bigD2Ev(ptr noundef nonnull align 4 dereferenceable(16) %6) #2, !dbg !364
  call void @llvm.lifetime.end.p0(i64 16, ptr %6) #2, !dbg !364
  %92 = call ptr @llvm.coro.free(token %19, ptr %26), !dbg !364
  %93 = icmp ne ptr %92, null, !dbg !364
  br i1 %93, label %94, label %96, !dbg !364

94:                                               ; preds = %90
  %95 = call i64 @llvm.coro.size.i64(), !dbg !364
  call void @_ZdlPvm(ptr noundef %92, i64 noundef %95) #2, !dbg !364
  br label %96, !dbg !364

96:                                               ; preds = %90, %94
  switch i32 %91, label %115 [
    i32 0, label %97
    i32 2, label %98
  ]

97:                                               ; preds = %96
  br label %98, !dbg !364

98:                                               ; preds = %97, %96, %82, %66, %35
  %99 = call i1 @llvm.coro.end(ptr null, i1 false, token none), !dbg !364
  %100 = getelementptr inbounds %struct.gen, ptr %3, i32 0, i32 0, !dbg !364
  %101 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %100, i32 0, i32 0, !dbg !364
  %102 = load ptr, ptr %101, align 8, !dbg !364
  %103 = ptrtoint ptr %102 to i64, !dbg !364
  ret i64 %103, !dbg !364

104:                                              ; preds = %43, %39
  call void @llvm.lifetime.end.p0(i64 16, ptr %6) #2, !dbg !364
  %105 = call ptr @llvm.coro.free(token %19, ptr %26), !dbg !364
  %106 = icmp ne ptr %105, null, !dbg !364
  br i1 %106, label %107, label %109, !dbg !364

107:                                              ; preds = %104
  %108 = call i64 @llvm.coro.size.i64(), !dbg !364
  call void @_ZdlPvm(ptr noundef %105, i64 noundef %108) #2, !dbg !364
  br label %109, !dbg !364

109:                                              ; preds = %104, %107
  br label %110, !dbg !364

110:                                              ; preds = %109
  %111 = load ptr, ptr %7, align 8, !dbg !364
  %112 = load i32, ptr %8, align 4, !dbg !364
  %113 = insertvalue { ptr, i32 } poison, ptr %111, 0, !dbg !364
  %114 = insertvalue { ptr, i32 } %113, i32 %112, 1, !dbg !364
  resume { ptr, i32 } %114, !dbg !364

115:                                              ; preds = %96
  unreachable
}

; Function Attrs: nocallback nofree nosync nounwind willreturn memory(argmem: read)
declare token @llvm.coro.id(i32, ptr readnone, ptr nocapture readonly, ptr) #1

; Function Attrs: nounwind
declare i1 @llvm.coro.alloc(token) #2

; Function Attrs: nobuiltin allocsize(0)
declare noundef nonnull ptr @_Znwm(i64 noundef) #3

; Function Attrs: nounwind memory(none)
declare i64 @llvm.coro.size.i64() #4

; Function Attrs: nounwind
declare ptr @llvm.coro.begin(token, ptr writeonly) #2

; Function Attrs: nocallback nofree nosync nounwind willreturn memory(argmem: readwrite)
declare void @llvm.lifetime.start.p0(i64 immarg, ptr nocapture) #5

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZN3bigC2ERKS_(ptr noundef nonnull align 4 dereferenceable(16) %0, ptr noundef nonnull align 4 dereferenceable(16) %1) unnamed_addr #6 comdat !dbg !396 {
  %3 = alloca ptr, align 8
  %4 = alloca ptr, align 8
  %5 = alloca i32, align 4
  store ptr %0, ptr %3, align 8
    #dbg_declare(ptr %3, !397, !DIExpression(), !399)
  store ptr %1, ptr %4, align 8
    #dbg_declare(ptr %4, !400, !DIExpression(), !401)
  %6 = load ptr, ptr %3, align 8
    #dbg_declare(ptr %5, !402, !DIExpression(), !405)
  store i32 0, ptr %5, align 4, !dbg !405
  br label %7, !dbg !406

7:                                                ; preds = %21, %2
  %8 = load i32, ptr %5, align 4, !dbg !407
  %9 = icmp slt i32 %8, 4, !dbg !409
  br i1 %9, label %10, label %24, !dbg !410

10:                                               ; preds = %7
  %11 = load ptr, ptr %4, align 8, !dbg !411
  %12 = getelementptr inbounds %struct.big, ptr %11, i32 0, i32 0, !dbg !412
  %13 = load i32, ptr %5, align 4, !dbg !413
  %14 = sext i32 %13 to i64, !dbg !411
  %15 = getelementptr inbounds [4 x i32], ptr %12, i64 0, i64 %14, !dbg !411
  %16 = load i32, ptr %15, align 4, !dbg !411
  %17 = getelementptr inbounds %struct.big, ptr %6, i32 0, i32 0, !dbg !414
  %18 = load i32, ptr %5, align 4, !dbg !415
  %19 = sext i32 %18 to i64, !dbg !414
  %20 = getelementptr inbounds [4 x i32], ptr %17, i64 0, i64 %19, !dbg !414
  store i32 %16, ptr %20, align 4, !dbg !416
  br label %21, !dbg !414

21:                                               ; preds = %10
  %22 = load i32, ptr %5, align 4, !dbg !417
  %23 = add nsw i32 %22, 1, !dbg !417
  store i32 %23, ptr %5, align 4, !dbg !417
  br label %7, !dbg !418, !llvm.loop !419

24:                                               ; preds = %7
  ret void, !dbg !421
}

declare i32 @__gxx_personality_v0(...)

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZN3gen12promise_typeC2Ev(ptr noundef nonnull align 4 dereferenceable(4) %0) unnamed_addr #6 comdat !dbg !422 {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
    #dbg_declare(ptr %2, !424, !DIExpression(), !425)
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds %"struct.gen::promise_type", ptr %3, i32 0, i32 0, !dbg !426
  store i32 0, ptr %4, align 4, !dbg !426
  ret void, !dbg !427
}

; Function Attrs: mustprogress noinline optnone uwtable
define linkonce_odr dso_local i64 @_ZN3gen12promise_type17get_return_objectEv(ptr noundef nonnull align 4 dereferenceable(4) %0) #7 comdat !dbg !428 {
  %2 = alloca %struct.gen, align 8
  %3 = alloca ptr, align 8
  store ptr %0, ptr %3, align 8
    #dbg_declare(ptr %3, !429, !DIExpression(), !430)
  %4 = load ptr, ptr %3, align 8
  %5 = getelementptr inbounds %struct.gen, ptr %2, i32 0, i32 0, !dbg !431
  %6 = call i64 @_ZNSt7__n486116coroutine_handleIN3gen12promise_typeEE12from_promiseERS2_(ptr noundef nonnull align 4 dereferenceable(4) %4), !dbg !432
  %7 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %5, i32 0, i32 0, !dbg !432
  %8 = inttoptr i64 %6 to ptr, !dbg !432
  store ptr %8, ptr %7, align 8, !dbg !432
  %9 = getelementptr inbounds %struct.gen, ptr %2, i32 0, i32 0, !dbg !433
  %10 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %9, i32 0, i32 0, !dbg !433
  %11 = load ptr, ptr %10, align 8, !dbg !433
  %12 = ptrtoint ptr %11 to i64, !dbg !433
  ret i64 %12, !dbg !433
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZN3gen12promise_type15initial_suspendEv(ptr noundef nonnull align 4 dereferenceable(4) %0) #6 comdat !dbg !434 {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
    #dbg_declare(ptr %2, !435, !DIExpression(), !436)
  %3 = load ptr, ptr %2, align 8
  ret void, !dbg !437
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local noundef i1 @_ZNKSt7__n486114suspend_always11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %0) #6 comdat !dbg !438 {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
    #dbg_declare(ptr %2, !439, !DIExpression(), !441)
  %3 = load ptr, ptr %2, align 8
  ret i1 false, !dbg !442
}

; Function Attrs: nomerge nounwind
declare token @llvm.coro.save(ptr) #8

; Function Attrs: alwaysinline mustprogress
define private void @_Z5count3bigi.__await_suspend_wrapper__init(ptr noundef nonnull %0, ptr noundef %1) #9 !dbg !443 {
  %3 = alloca ptr, align 8
  %4 = alloca ptr, align 8
  %5 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %6 = alloca %"struct.std::__n4861::coroutine_handle", align 8
  store ptr %0, ptr %3, align 8
    #dbg_declare(ptr %3, !445, !DIExpression(), !446)
  store ptr %1, ptr %4, align 8
    #dbg_declare(ptr %4, !447, !DIExpression(), !446)
  %7 = load ptr, ptr %3, align 8
  %8 = load ptr, ptr %4, align 8
  %9 = call i64 @_ZNSt7__n486116coroutine_handleIN3gen12promise_typeEE12from_addressEPv(ptr noundef %8) #2, !dbg !448
  %10 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %6, i32 0, i32 0, !dbg !448
  %11 = inttoptr i64 %9 to ptr, !dbg !448
  store ptr %11, ptr %10, align 8, !dbg !448
  %12 = call i64 @_ZNKSt7__n486116coroutine_handleIN3gen12promise_typeEEcvNS0_IvEEEv(ptr noundef nonnull align 8 dereferenceable(8) %6) #2, !dbg !448
  %13 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %5, i32 0, i32 0, !dbg !448
  %14 = inttoptr i64 %12 to ptr, !dbg !448
  store ptr %14, ptr %13, align 8, !dbg !448
  %15 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %5, i32 0, i32 0, !dbg !448
  %16 = load ptr, ptr %15, align 8, !dbg !448
  %17 = ptrtoint ptr %16 to i64, !dbg !448
  call void @_ZNKSt7__n486114suspend_always13await_suspendENS_16coroutine_handleIvEE(ptr noundef nonnull align 1 dereferenceable(1) %7, i64 %17) #2, !dbg !448
  ret void, !dbg !448
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZNKSt7__n486114suspend_always13await_suspendENS_16coroutine_handleIvEE(ptr noundef nonnull align 1 dereferenceable(1) %0, i64 %1) #6 comdat !dbg !449 {
  %3 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %4 = alloca ptr, align 8
  %5 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %3, i32 0, i32 0
  %6 = inttoptr i64 %1 to ptr
  store ptr %6, ptr %5, align 8
  store ptr %0, ptr %4, align 8
    #dbg_declare(ptr %4, !450, !DIExpression(), !451)
    #dbg_declare(ptr %3, !452, !DIExpression(), !453)
  %7 = load ptr, ptr %4, align 8
  ret void, !dbg !454
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local i64 @_ZNSt7__n486116coroutine_handleIN3gen12promise_typeEE12from_addressEPv(ptr noundef %0) #6 comdat !dbg !455 {
  %2 = alloca %"struct.std::__n4861::coroutine_handle", align 8
  %3 = alloca ptr, align 8
  store ptr %0, ptr %3, align 8
    #dbg_declare(ptr %3, !456, !DIExpression(), !457)
    #dbg_declare(ptr %2, !458, !DIExpression(), !459)
  call void @_ZNSt7__n486116coroutine_handleIN3gen12promise_typeEEC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %2) #2, !dbg !459
  %4 = load ptr, ptr %3, align 8, !dbg !460
  %5 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %2, i32 0, i32 0, !dbg !461
  store ptr %4, ptr %5, align 8, !dbg !462
  %6 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %2, i32 0, i32 0, !dbg !463
  %7 = load ptr, ptr %6, align 8, !dbg !463
  %8 = ptrtoint ptr %7 to i64, !dbg !463
  ret i64 %8, !dbg !463
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local i64 @_ZNKSt7__n486116coroutine_handleIN3gen12promise_typeEEcvNS0_IvEEEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #6 comdat !dbg !464 {
  %2 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %3 = alloca ptr, align 8
  store ptr %0, ptr %3, align 8
    #dbg_declare(ptr %3, !465, !DIExpression(), !467)
  %4 = load ptr, ptr %3, align 8
  %5 = call noundef ptr @_ZNKSt7__n486116coroutine_handleIN3gen12promise_typeEE7addressEv(ptr noundef nonnull align 8 dereferenceable(8) %4) #2, !dbg !468
  %6 = call i64 @_ZNSt7__n486116coroutine_handleIvE12from_addressEPv(ptr noundef %5) #2, !dbg !469
  %7 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %2, i32 0, i32 0, !dbg !469
  %8 = inttoptr i64 %6 to ptr, !dbg !469
  store ptr %8, ptr %7, align 8, !dbg !469
  %9 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %2, i32 0, i32 0, !dbg !470
  %10 = load ptr, ptr %9, align 8, !dbg !470
  %11 = ptrtoint ptr %10 to i64, !dbg !470
  ret i64 %11, !dbg !470
}

declare void @llvm.coro.await.suspend.void(ptr, ptr, ptr)

; Function Attrs: nounwind
declare i8 @llvm.coro.suspend(token, i1) #2

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZNKSt7__n486114suspend_always12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %0) #6 comdat !dbg !471 {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
    #dbg_declare(ptr %2, !472, !DIExpression(), !473)
  %3 = load ptr, ptr %2, align 8
  ret void, !dbg !474
}

; Function Attrs: nocallback nofree nosync nounwind willreturn memory(argmem: readwrite)
declare void @llvm.lifetime.end.p0(i64 immarg, ptr nocapture) #5

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZN3gen12promise_type11yield_valueEi(ptr noundef nonnull align 4 dereferenceable(4) %0, i32 noundef %1) #6 comdat !dbg !475 {
  %3 = alloca ptr, align 8
  %4 = alloca i32, align 4
  store ptr %0, ptr %3, align 8
    #dbg_declare(ptr %3, !476, !DIExpression(), !477)
  store i32 %1, ptr %4, align 4
    #dbg_declare(ptr %4, !478, !DIExpression(), !479)
  %5 = load ptr, ptr %3, align 8
  %6 = load i32, ptr %4, align 4, !dbg !480
  %7 = getelementptr inbounds %"struct.gen::promise_type", ptr %5, i32 0, i32 0, !dbg !481
  store i32 %6, ptr %7, align 4, !dbg !482
  ret void, !dbg !483
}

; Function Attrs: alwaysinline mustprogress
define private void @_Z5count3bigi.__await_suspend_wrapper__yield(ptr noundef nonnull %0, ptr noundef %1) #9 !dbg !484 {
  %3 = alloca ptr, align 8
  %4 = alloca ptr, align 8
  %5 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %6 = alloca %"struct.std::__n4861::coroutine_handle", align 8
  store ptr %0, ptr %3, align 8
    #dbg_declare(ptr %3, !485, !DIExpression(), !486)
  store ptr %1, ptr %4, align 8
    #dbg_declare(ptr %4, !487, !DIExpression(), !486)
  %7 = load ptr, ptr %3, align 8
  %8 = load ptr, ptr %4, align 8
  %9 = call i64 @_ZNSt7__n486116coroutine_handleIN3gen12promise_typeEE12from_addressEPv(ptr noundef %8) #2, !dbg !488
  %10 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %6, i32 0, i32 0, !dbg !488
  %11 = inttoptr i64 %9 to ptr, !dbg !488
  store ptr %11, ptr %10, align 8, !dbg !488
  %12 = call i64 @_ZNKSt7__n486116coroutine_handleIN3gen12promise_typeEEcvNS0_IvEEEv(ptr noundef nonnull align 8 dereferenceable(8) %6) #2, !dbg !488
  %13 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %5, i32 0, i32 0, !dbg !488
  %14 = inttoptr i64 %12 to ptr, !dbg !488
  store ptr %14, ptr %13, align 8, !dbg !488
  %15 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %5, i32 0, i32 0, !dbg !488
  %16 = load ptr, ptr %15, align 8, !dbg !488
  %17 = ptrtoint ptr %16 to i64, !dbg !488
  call void @_ZNKSt7__n486114suspend_always13await_suspendENS_16coroutine_handleIvEE(ptr noundef nonnull align 1 dereferenceable(1) %7, i64 %17) #2, !dbg !488
  ret void, !dbg !488
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZN3gen12promise_type11return_voidEv(ptr noundef nonnull align 4 dereferenceable(4) %0) #6 comdat !dbg !489 {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
    #dbg_declare(ptr %2, !490, !DIExpression(), !491)
  %3 = load ptr, ptr %2, align 8
  ret void, !dbg !492
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZN3gen12promise_type13final_suspendEv(ptr noundef nonnull align 4 dereferenceable(4) %0) #6 comdat !dbg !493 {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
    #dbg_declare(ptr %2, !494, !DIExpression(), !495)
  %3 = load ptr, ptr %2, align 8
  ret void, !dbg !496
}

; Function Attrs: alwaysinline mustprogress
define private void @_Z5count3bigi.__await_suspend_wrapper__final(ptr noundef nonnull %0, ptr noundef %1) #9 !dbg !497 {
  %3 = alloca ptr, align 8
  %4 = alloca ptr, align 8
  %5 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %6 = alloca %"struct.std::__n4861::coroutine_handle", align 8
  store ptr %0, ptr %3, align 8
    #dbg_declare(ptr %3, !498, !DIExpression(), !499)
  store ptr %1, ptr %4, align 8
    #dbg_declare(ptr %4, !500, !DIExpression(), !499)
  %7 = load ptr, ptr %3, align 8
  %8 = load ptr, ptr %4, align 8
  %9 = call i64 @_ZNSt7__n486116coroutine_handleIN3gen12promise_typeEE12from_addressEPv(ptr noundef %8) #2, !dbg !501
  %10 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %6, i32 0, i32 0, !dbg !501
  %11 = inttoptr i64 %9 to ptr, !dbg !501
  store ptr %11, ptr %10, align 8, !dbg !501
  %12 = call i64 @_ZNKSt7__n486116coroutine_handleIN3gen12promise_typeEEcvNS0_IvEEEv(ptr noundef nonnull align 8 dereferenceable(8) %6) #2, !dbg !501
  %13 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %5, i32 0, i32 0, !dbg !501
  %14 = inttoptr i64 %12 to ptr, !dbg !501
  store ptr %14, ptr %13, align 8, !dbg !501
  %15 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %5, i32 0, i32 0, !dbg !501
  %16 = load ptr, ptr %15, align 8, !dbg !501
  %17 = ptrtoint ptr %16 to i64, !dbg !501
  call void @_ZNKSt7__n486114suspend_always13await_suspendENS_16coroutine_handleIvEE(ptr noundef nonnull align 1 dereferenceable(1) %7, i64 %17) #2, !dbg !501
  ret void, !dbg !501
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZN3bigD2Ev(ptr noundef nonnull align 4 dereferenceable(16) %0) unnamed_addr #6 comdat personality ptr @__gxx_personality_v0 !dbg !502 {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
    #dbg_declare(ptr %2, !503, !DIExpression(), !504)
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds %struct.big, ptr %3, i32 0, i32 0, !dbg !505
  %5 = getelementptr inbounds [4 x i32], ptr %4, i64 0, i64 0, !dbg !505
  %6 = load i32, ptr %5, align 4, !dbg !505
  %7 = invoke i32 (ptr, ...) @printf(ptr noundef @.str.1, i32 noundef %6)
          to label %8 unwind label %9, !dbg !507

8:                                                ; preds = %1
  ret void, !dbg !508

9:                                                ; preds = %1
  %10 = landingpad { ptr, i32 }
          catch ptr null, !dbg !507
  %11 = extractvalue { ptr, i32 } %10, 0, !dbg !507
  call void @__clang_call_terminate(ptr %11) #17, !dbg !507
  unreachable, !dbg !507
}

; Function Attrs: nobuiltin nounwind
declare void @_ZdlPvm(ptr noundef, i64 noundef) #10

; Function Attrs: nounwind memory(argmem: read)
declare ptr @llvm.coro.free(token, ptr nocapture readonly) #11

; Function Attrs: nounwind
declare i1 @llvm.coro.end(ptr, i1, token) #2

; Function Attrs: mustprogress noinline norecurse optnone uwtable
define dso_local noundef i32 @main() #12 personality ptr @__gxx_personality_v0 !dbg !509 {
  %1 = alloca i32, align 4
  %2 = alloca %struct.gen, align 8
  %3 = alloca %struct.big, align 4
  %4 = alloca ptr, align 8
  %5 = alloca i32, align 4
  store i32 0, ptr %1, align 4
    #dbg_declare(ptr %2, !510, !DIExpression(), !511)
  call void @_ZN3bigC2Ei(ptr noundef nonnull align 4 dereferenceable(16) %3, i32 noundef 3), !dbg !512
  %6 = invoke i64 @_Z5count3bigi(ptr noundef %3, i32 noundef 3)
          to label %7 unwind label %24, !dbg !513

7:                                                ; preds = %0
  %8 = getelementptr inbounds %struct.gen, ptr %2, i32 0, i32 0, !dbg !513
  %9 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %8, i32 0, i32 0, !dbg !513
  %10 = inttoptr i64 %6 to ptr, !dbg !513
  store ptr %10, ptr %9, align 8, !dbg !513
  call void @_ZN3bigD2Ev(ptr noundef nonnull align 4 dereferenceable(16) %3) #2, !dbg !513
  %11 = getelementptr inbounds %struct.gen, ptr %2, i32 0, i32 0, !dbg !514
  call void @_ZNKSt7__n486116coroutine_handleIN3gen12promise_typeEE6resumeEv(ptr noundef nonnull align 8 dereferenceable(8) %11), !dbg !516
  br label %12, !dbg !517

12:                                               ; preds = %22, %7
  %13 = getelementptr inbounds %struct.gen, ptr %2, i32 0, i32 0, !dbg !518
  %14 = call noundef i1 @_ZNKSt7__n486116coroutine_handleIN3gen12promise_typeEE4doneEv(ptr noundef nonnull align 8 dereferenceable(8) %13) #2, !dbg !520
  %15 = xor i1 %14, true, !dbg !521
  br i1 %15, label %16, label %28, !dbg !522

16:                                               ; preds = %12
  %17 = getelementptr inbounds %struct.gen, ptr %2, i32 0, i32 0, !dbg !523
  %18 = call noundef nonnull align 4 dereferenceable(4) ptr @_ZNKSt7__n486116coroutine_handleIN3gen12promise_typeEE7promiseEv(ptr noundef nonnull align 8 dereferenceable(8) %17), !dbg !524
  %19 = getelementptr inbounds %"struct.gen::promise_type", ptr %18, i32 0, i32 0, !dbg !525
  %20 = load i32, ptr %19, align 4, !dbg !525
  %21 = call i32 (ptr, ...) @printf(ptr noundef @.str, i32 noundef %20), !dbg !526
  br label %22, !dbg !526

22:                                               ; preds = %16
  %23 = getelementptr inbounds %struct.gen, ptr %2, i32 0, i32 0, !dbg !527
  call void @_ZNKSt7__n486116coroutine_handleIN3gen12promise_typeEE6resumeEv(ptr noundef nonnull align 8 dereferenceable(8) %23), !dbg !528
  br label %12, !dbg !529, !llvm.loop !530

24:                                               ; preds = %0
  %25 = landingpad { ptr, i32 }
          cleanup, !dbg !532
  %26 = extractvalue { ptr, i32 } %25, 0, !dbg !532
  store ptr %26, ptr %4, align 8, !dbg !532
  %27 = extractvalue { ptr, i32 } %25, 1, !dbg !532
  store i32 %27, ptr %5, align 4, !dbg !532
  call void @_ZN3bigD2Ev(ptr noundef nonnull align 4 dereferenceable(16) %3) #2, !dbg !513
  br label %30, !dbg !513

28:                                               ; preds = %12
  %29 = getelementptr inbounds %struct.gen, ptr %2, i32 0, i32 0, !dbg !533
  call void @_ZNKSt7__n486116coroutine_handleIN3gen12promise_typeEE7destroyEv(ptr noundef nonnull align 8 dereferenceable(8) %29), !dbg !534
  ret i32 0, !dbg !535

30:                                               ; preds = %24
  %31 = load ptr, ptr %4, align 8, !dbg !513
  %32 = load i32, ptr %5, align 4, !dbg !513
  %33 = insertvalue { ptr, i32 } poison, ptr %31, 0, !dbg !513
  %34 = insertvalue { ptr, i32 } %33, i32 %32, 1, !dbg !513
  resume { ptr, i32 } %34, !dbg !513
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZN3bigC2Ei(ptr noundef nonnull align 4 dereferenceable(16) %0, i32 noundef %1) unnamed_addr #6 comdat !dbg !536 {
  %3 = alloca ptr, align 8
  %4 = alloca i32, align 4
  %5 = alloca i32, align 4
  store ptr %0, ptr %3, align 8
    #dbg_declare(ptr %3, !537, !DIExpression(), !538)
  store i32 %1, ptr %4, align 4
    #dbg_declare(ptr %4, !539, !DIExpression(), !540)
  %6 = load ptr, ptr %3, align 8
    #dbg_declare(ptr %5, !541, !DIExpression(), !544)
  store i32 0, ptr %5, align 4, !dbg !544
  br label %7, !dbg !545

7:                                                ; preds = %18, %2
  %8 = load i32, ptr %5, align 4, !dbg !546
  %9 = icmp slt i32 %8, 4, !dbg !548
  br i1 %9, label %10, label %21, !dbg !549

10:                                               ; preds = %7
  %11 = load i32, ptr %4, align 4, !dbg !550
  %12 = load i32, ptr %5, align 4, !dbg !551
  %13 = add nsw i32 %11, %12, !dbg !552
  %14 = getelementptr inbounds %struct.big, ptr %6, i32 0, i32 0, !dbg !553
  %15 = load i32, ptr %5, align 4, !dbg !554
  %16 = sext i32 %15 to i64, !dbg !553
  %17 = getelementptr inbounds [4 x i32], ptr %14, i64 0, i64 %16, !dbg !553
  store i32 %13, ptr %17, align 4, !dbg !555
  br label %18, !dbg !553

18:                                               ; preds = %10
  %19 = load i32, ptr %5, align 4, !dbg !556
  %20 = add nsw i32 %19, 1, !dbg !556
  store i32 %20, ptr %5, align 4, !dbg !556
  br label %7, !dbg !557, !llvm.loop !558

21:                                               ; preds = %7
  ret void, !dbg !560
}

; Function Attrs: mustprogress noinline optnone uwtable
define linkonce_odr dso_local void @_ZNKSt7__n486116coroutine_handleIN3gen12promise_typeEE6resumeEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #7 comdat !dbg !561 {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
    #dbg_declare(ptr %2, !562, !DIExpression(), !563)
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %3, i32 0, i32 0, !dbg !564
  %5 = load ptr, ptr %4, align 8, !dbg !564
  call void @llvm.coro.resume(ptr %5), !dbg !565
  ret void, !dbg !566
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local noundef i1 @_ZNKSt7__n486116coroutine_handleIN3gen12promise_typeEE4doneEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #6 comdat !dbg !567 {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
    #dbg_declare(ptr %2, !568, !DIExpression(), !569)
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %3, i32 0, i32 0, !dbg !570
  %5 = load ptr, ptr %4, align 8, !dbg !570
  %6 = call i1 @llvm.coro.done(ptr %5), !dbg !571
  ret i1 %6, !dbg !572
}

declare i32 @printf(ptr noundef, ...) #13

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local noundef nonnull align 4 dereferenceable(4) ptr @_ZNKSt7__n486116coroutine_handleIN3gen12promise_typeEE7promiseEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #6 comdat !dbg !573 {
  %2 = alloca ptr, align 8
  %3 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
    #dbg_declare(ptr %2, !574, !DIExpression(), !575)
  %4 = load ptr, ptr %2, align 8
    #dbg_declare(ptr %3, !576, !DIExpression(), !577)
  %5 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %4, i32 0, i32 0, !dbg !578
  %6 = load ptr, ptr %5, align 8, !dbg !578
  %7 = call ptr @llvm.coro.promise(ptr %6, i32 4, i1 false), !dbg !579
  store ptr %7, ptr %3, align 8, !dbg !577
  %8 = load ptr, ptr %3, align 8, !dbg !580
  ret ptr %8, !dbg !581
}

; Function Attrs: mustprogress noinline optnone uwtable
define linkonce_odr dso_local void @_ZNKSt7__n486116coroutine_handleIN3gen12promise_typeEE7destroyEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #7 comdat !dbg !582 {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
    #dbg_declare(ptr %2, !583, !DIExpression(), !584)
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %3, i32 0, i32 0, !dbg !585
  %5 = load ptr, ptr %4, align 8, !dbg !585
  call void @llvm.coro.destroy(ptr %5), !dbg !586
  ret void, !dbg !587
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local i64 @_ZNSt7__n486116coroutine_handleIN3gen12promise_typeEE12from_promiseERS2_(ptr noundef nonnull align 4 dereferenceable(4) %0) #6 comdat !dbg !588 {
  %2 = alloca %"struct.std::__n4861::coroutine_handle", align 8
  %3 = alloca ptr, align 8
  store ptr %0, ptr %3, align 8
    #dbg_declare(ptr %3, !589, !DIExpression(), !590)
    #dbg_declare(ptr %2, !591, !DIExpression(), !592)
  call void @_ZNSt7__n486116coroutine_handleIN3gen12promise_typeEEC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %2) #2, !dbg !592
  %4 = load ptr, ptr %3, align 8, !dbg !593
  %5 = call ptr @llvm.coro.promise(ptr %4, i32 4, i1 true), !dbg !594
  %6 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %2, i32 0, i32 0, !dbg !595
  store ptr %5, ptr %6, align 8, !dbg !596
  %7 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %2, i32 0, i32 0, !dbg !597
  %8 = load ptr, ptr %7, align 8, !dbg !597
  %9 = ptrtoint ptr %8 to i64, !dbg !597
  ret i64 %9, !dbg !597
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZNSt7__n486116coroutine_handleIN3gen12promise_typeEEC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %0) unnamed_addr #6 comdat !dbg !598 {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
    #dbg_declare(ptr %2, !599, !DIExpression(), !601)
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %3, i32 0, i32 0, !dbg !602
  store ptr null, ptr %4, align 8, !dbg !602
  ret void, !dbg !603
}

; Function Attrs: nounwind memory(none)
declare ptr @llvm.coro.promise(ptr nocapture, i32, i1) #4

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local i64 @_ZNSt7__n486116coroutine_handleIvE12from_addressEPv(ptr noundef %0) #6 comdat !dbg !604 {
  %2 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %3 = alloca ptr, align 8
  store ptr %0, ptr %3, align 8
    #dbg_declare(ptr %3, !605, !DIExpression(), !606)
    #dbg_declare(ptr %2, !607, !DIExpression(), !608)
  call void @_ZNSt7__n486116coroutine_handleIvEC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %2) #2, !dbg !608
  %4 = load ptr, ptr %3, align 8, !dbg !609
  %5 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %2, i32 0, i32 0, !dbg !610
  store ptr %4, ptr %5, align 8, !dbg !611
  %6 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %2, i32 0, i32 0, !dbg !612
  %7 = load ptr, ptr %6, align 8, !dbg !612
  %8 = ptrtoint ptr %7 to i64, !dbg !612
  ret i64 %8, !dbg !612
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local noundef ptr @_ZNKSt7__n486116coroutine_handleIN3gen12promise_typeEE7addressEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #6 comdat !dbg !613 {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
    #dbg_declare(ptr %2, !614, !DIExpression(), !615)
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %3, i32 0, i32 0, !dbg !616
  %5 = load ptr, ptr %4, align 8, !dbg !616
  ret ptr %5, !dbg !617
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZNSt7__n486116coroutine_handleIvEC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %0) unnamed_addr #6 comdat !dbg !618 {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
    #dbg_declare(ptr %2, !619, !DIExpression(), !621)
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %3, i32 0, i32 0, !dbg !622
  store ptr null, ptr %4, align 8, !dbg !622
  ret void, !dbg !623
}

; Function Attrs: noinline noreturn nounwind uwtable
define linkonce_odr hidden void @__clang_call_terminate(ptr noundef %0) #14 comdat {
  %2 = call ptr @__cxa_begin_catch(ptr %0) #2
  call void @_ZSt9terminatev() #17
  unreachable
}

declare ptr @__cxa_begin_catch(ptr)

declare void @_ZSt9terminatev()

declare void @llvm.coro.resume(ptr)

; Function Attrs: nounwind memory(argmem: readwrite)
declare i1 @llvm.coro.done(ptr nocapture readonly) #15

declare void @llvm.coro.destroy(ptr)

attributes #0 = { mustprogress noinline optnone presplitcoroutine uwtable "frame-pointer"="non-leaf" "no-trapping-math"="true" "stack-protector-buffer-size"="8" }
attributes #1 = { nocallback nofree nosync nounwind willreturn memory(argmem: read) }
attributes #2 = { nounwind }
attributes #3 = { nobuiltin allocsize(0) "frame-pointer"="non-leaf" "no-trapping-math"="true" "stack-protector-buffer-size"="8" }
attributes #4 = { nounwind memory(none) }
attributes #5 = { nocallback nofree nosync nounwind willreturn memory(argmem: readwrite) }
attributes #6 = { mustprogress noinline nounwind optnone uwtable "frame-pointer"="non-leaf" "no-trapping-math"="true" "stack-protector-buffer-size"="8" }
attributes #7 = { mustprogress noinline optnone uwtable "frame-pointer"="non-leaf" "no-trapping-math"="true" "stack-protector-buffer-size"="8" }
attributes #8 = { nomerge nounwind }
attributes #9 = { alwaysinline mustprogress }
attributes #10 = { nobuiltin nounwind "frame-pointer"="non-leaf" "no-trapping-math"="true" "stack-protector-buffer-size"="8" }
attributes #11 = { nounwind memory(argmem: read) }
attributes #12 = { mustprogress noinline norecurse optnone uwtable "frame-pointer"="non-leaf" "no-trapping-math"="true" "stack-protector-buffer-size"="8" }
attributes #13 = { "frame-pointer"="non-leaf" "no-trapping-math"="true" "stack-protector-buffer-size"="8" }
attributes #14 = { noinline noreturn nounwind uwtable "frame-pointer"="non-leaf" "no-trapping-math"="true" "stack-protector-buffer-size"="8" }
attributes #15 = { nounwind memory(argmem: readwrite) }
attributes #16 = { allocsize(0) }
attributes #17 = { noreturn nounwind }

!llvm.dbg.cu = !{!13}
!llvm.linker.options = !{}
!llvm.module.flags = !{!347, !348, !349, !350, !351, !352, !353}
!llvm.ident = !{!354}

!0 = !DIGlobalVariableExpression(var: !1, expr: !DIExpression())
!1 = distinct !DIGlobalVariable(scope: null, file: !2, line: 28, type: !3, isLocal: true, isDefinition: true)
!2 = !DIFile(filename: "byaddr.cpp", directory: ".", checksumkind: CSK_MD5, checksum: "ea9597ebd6676725685f9efbd23745ff")
!3 = !DICompositeType(tag: DW_TAG_array_type, baseType: !4, size: 32, elements: !6)
!4 = !DIDerivedType(tag: DW_TAG_const_type, baseType: !5)
!5 = !DIBasicType(name: "char", size: 8, encoding: DW_ATE_unsigned_char)
!6 = !{!7}
!7 = !DISubrange(count: 4)
!8 = !DIGlobalVariableExpression(var: !9, expr: !DIExpression())
!9 = distinct !DIGlobalVariable(scope: null, file: !2, line: 9, type: !10, isLocal: true, isDefinition: true)
!10 = !DICompositeType(tag: DW_TAG_array_type, baseType: !4, size: 72, elements: !11)
!11 = !{!12}
!12 = !DISubrange(count: 9)
!13 = distinct !DICompileUnit(language: DW_LANG_C_plus_plus_14, file: !2, producer: "Debian clang version 19.1.7 (3+b1)", isOptimized: false, runtimeVersion: 0, emissionKind: FullDebug, retainedTypes: !14, globals: !152, imports: !153, splitDebugInlining: false, nameTableKind: None)
!14 = !{!15, !32, !150, !151, !77, !35, !56}
!15 = distinct !DICompositeType(tag: DW_TAG_structure_type, name: "big", file: !2, line: 5, size: 128, flags: DIFlagTypePassByReference | DIFlagNonTrivial, elements: !16, identifier: "_ZTS3big")
!16 = !{!17, !20, !24, !29}
!17 = !DIDerivedType(tag: DW_TAG_member, name: "a", scope: !15, file: !2, line: 6, baseType: !18, size: 128)
!18 = !DICompositeType(tag: DW_TAG_array_type, baseType: !19, size: 128, elements: !6)
!19 = !DIBasicType(name: "int", size: 32, encoding: DW_ATE_signed)
!20 = !DISubprogram(name: "big", scope: !15, file: !2, line: 7, type: !21, scopeLine: 7, flags: DIFlagExplicit | DIFlagPrototyped, spFlags: 0)
!21 = !DISubroutineType(types: !22)
!22 = !{null, !23, !19}
!23 = !DIDerivedType(tag: DW_TAG_pointer_type, baseType: !15, size: 64, flags: DIFlagArtificial | DIFlagObjectPointer)
!24 = !DISubprogram(name: "big", scope: !15, file: !2, line: 8, type: !25, scopeLine: 8, flags: DIFlagPrototyped, spFlags: 0)
!25 = !DISubroutineType(types: !26)
!26 = !{null, !23, !27}
!27 = !DIDerivedType(tag: DW_TAG_reference_type, baseType: !28, size: 64)
!28 = !DIDerivedType(tag: DW_TAG_const_type, baseType: !15)
!29 = !DISubprogram(name: "~big", scope: !15, file: !2, line: 9, type: !30, scopeLine: 9, flags: DIFlagPrototyped, spFlags: 0)
!30 = !DISubroutineType(types: !31)
!31 = !{null, !23}
!32 = distinct !DICompositeType(tag: DW_TAG_structure_type, name: "gen", file: !2, line: 11, size: 64, flags: DIFlagTypePassByValue | DIFlagNonTrivial, elements: !33, identifier: "_ZTS3gen")
!33 = !{!34}
!34 = !DIDerivedType(tag: DW_TAG_member, name: "h", scope: !32, file: !2, line: 21, baseType: !35, size: 64)
!35 = distinct !DICompositeType(tag: DW_TAG_structure_type, name: "coroutine_handle<gen::promise_type>", scope: !37, file: !36, line: 193, size: 64, flags: DIFlagTypePassByValue | DIFlagNonTrivial, elements: !39, templateParams: !148, identifier: "_ZTSNSt7__n486116coroutine_handleIN3gen12promise_typeEEE")
!36 = !DIFile(filename: "/usr/lib/gcc/aarch64-linux-gnu/14/../../../../include/c++/14/coroutine", directory: "")
!37 = !DINamespace(name: "__n4861", scope: !38, exportSymbols: true)
!38 = !DINamespace(name: "std", scope: null)
!39 = !{!40, !42, !46, !52, !121, !125, !130, !133, !136, !139, !140, !143, !144, !145}
!40 = !DIDerivedType(tag: DW_TAG_member, name: "_M_fr_ptr", scope: !35, file: !36, line: 255, baseType: !41, size: 64, flags: DIFlagPrivate)
!41 = !DIDerivedType(tag: DW_TAG_pointer_type, baseType: null, size: 64)
!42 = !DISubprogram(name: "coroutine_handle", scope: !35, file: !36, line: 197, type: !43, scopeLine: 197, flags: DIFlagPrototyped, spFlags: 0)
!43 = !DISubroutineType(types: !44)
!44 = !{null, !45}
!45 = !DIDerivedType(tag: DW_TAG_pointer_type, baseType: !35, size: 64, flags: DIFlagArtificial | DIFlagObjectPointer)
!46 = !DISubprogram(name: "coroutine_handle", scope: !35, file: !36, line: 199, type: !47, scopeLine: 199, flags: DIFlagPrototyped, spFlags: 0)
!47 = !DISubroutineType(types: !48)
!48 = !{null, !45, !49}
!49 = !DIDerivedType(tag: DW_TAG_typedef, name: "nullptr_t", scope: !38, file: !50, line: 314, baseType: !51)
!50 = !DIFile(filename: "/usr/lib/gcc/aarch64-linux-gnu/14/../../../../include/aarch64-linux-gnu/c++/14/bits/c++config.h", directory: "")
!51 = !DIBasicType(tag: DW_TAG_unspecified_type, name: "decltype(nullptr)")
!52 = !DISubprogram(name: "from_promise", linkageName: "_ZNSt7__n486116coroutine_handleIN3gen12promise_typeEE12from_promiseERS2_", scope: !35, file: !36, line: 202, type: !53, scopeLine: 202, flags: DIFlagPrototyped | DIFlagStaticMember, spFlags: 0)
!53 = !DISubroutineType(types: !54)
!54 = !{!35, !55}
!55 = !DIDerivedType(tag: DW_TAG_reference_type, baseType: !56, size: 64)
!56 = distinct !DICompositeType(tag: DW_TAG_structure_type, name: "promise_type", scope: !32, file: !2, line: 12, size: 32, flags: DIFlagTypePassByValue | DIFlagNonTrivial, elements: !57, identifier: "_ZTSN3gen12promise_typeE")
!57 = !{!58, !59, !63, !113, !114, !117, !120}
!58 = !DIDerivedType(tag: DW_TAG_member, name: "v", scope: !56, file: !2, line: 13, baseType: !19, size: 32)
!59 = !DISubprogram(name: "get_return_object", linkageName: "_ZN3gen12promise_type17get_return_objectEv", scope: !56, file: !2, line: 14, type: !60, scopeLine: 14, flags: DIFlagPrototyped, spFlags: 0)
!60 = !DISubroutineType(types: !61)
!61 = !{!32, !62}
!62 = !DIDerivedType(tag: DW_TAG_pointer_type, baseType: !56, size: 64, flags: DIFlagArtificial | DIFlagObjectPointer)
!63 = !DISubprogram(name: "initial_suspend", linkageName: "_ZN3gen12promise_type15initial_suspendEv", scope: !56, file: !2, line: 15, type: !64, scopeLine: 15, flags: DIFlagPrototyped, spFlags: 0)
!64 = !DISubroutineType(types: !65)
!65 = !{!66, !62}
!66 = distinct !DICompositeType(tag: DW_TAG_structure_type, name: "suspend_always", scope: !37, file: !36, line: 324, size: 8, flags: DIFlagTypePassByValue, elements: !67, identifier: "_ZTSNSt7__n486114suspend_alwaysE")
!67 = !{!68, !74, !110}
!68 = !DISubprogram(name: "await_ready", linkageName: "_ZNKSt7__n486114suspend_always11await_readyEv", scope: !66, file: !36, line: 326, type: !69, scopeLine: 326, flags: DIFlagPrototyped, spFlags: 0)
!69 = !DISubroutineType(types: !70)
!70 = !{!71, !72}
!71 = !DIBasicType(name: "bool", size: 8, encoding: DW_ATE_boolean)
!72 = !DIDerivedType(tag: DW_TAG_pointer_type, baseType: !73, size: 64, flags: DIFlagArtificial | DIFlagObjectPointer)
!73 = !DIDerivedType(tag: DW_TAG_const_type, baseType: !66)
!74 = !DISubprogram(name: "await_suspend", linkageName: "_ZNKSt7__n486114suspend_always13await_suspendENS_16coroutine_handleIvEE", scope: !66, file: !36, line: 328, type: !75, scopeLine: 328, flags: DIFlagPrototyped, spFlags: 0)
!75 = !DISubroutineType(types: !76)
!76 = !{null, !72, !77}
!77 = distinct !DICompositeType(tag: DW_TAG_structure_type, name: "coroutine_handle<void>", scope: !37, file: !36, line: 98, size: 64, flags: DIFlagTypePassByValue | DIFlagNonTrivial, elements: !78, templateParams: !108, identifier: "_ZTSNSt7__n486116coroutine_handleIvEE")
!78 = !{!79, !80, !84, !87, !91, !96, !99, !102, !103, !106, !107}
!79 = !DIDerivedType(tag: DW_TAG_member, name: "_M_fr_ptr", scope: !77, file: !36, line: 142, baseType: !41, size: 64, flags: DIFlagProtected)
!80 = !DISubprogram(name: "coroutine_handle", scope: !77, file: !36, line: 102, type: !81, scopeLine: 102, flags: DIFlagPrototyped, spFlags: 0)
!81 = !DISubroutineType(types: !82)
!82 = !{null, !83}
!83 = !DIDerivedType(tag: DW_TAG_pointer_type, baseType: !77, size: 64, flags: DIFlagArtificial | DIFlagObjectPointer)
!84 = !DISubprogram(name: "coroutine_handle", scope: !77, file: !36, line: 104, type: !85, scopeLine: 104, flags: DIFlagPrototyped, spFlags: 0)
!85 = !DISubroutineType(types: !86)
!86 = !{null, !83, !49}
!87 = !DISubprogram(name: "operator=", linkageName: "_ZNSt7__n486116coroutine_handleIvEaSEDn", scope: !77, file: !36, line: 108, type: !88, scopeLine: 108, flags: DIFlagPrototyped, spFlags: 0)
!88 = !DISubroutineType(types: !89)
!89 = !{!90, !83, !49}
!90 = !DIDerivedType(tag: DW_TAG_reference_type, baseType: !77, size: 64)
!91 = !DISubprogram(name: "address", linkageName: "_ZNKSt7__n486116coroutine_handleIvE7addressEv", scope: !77, file: !36, line: 116, type: !92, scopeLine: 116, flags: DIFlagPrototyped, spFlags: 0)
!92 = !DISubroutineType(types: !93)
!93 = !{!41, !94}
!94 = !DIDerivedType(tag: DW_TAG_pointer_type, baseType: !95, size: 64, flags: DIFlagArtificial | DIFlagObjectPointer)
!95 = !DIDerivedType(tag: DW_TAG_const_type, baseType: !77)
!96 = !DISubprogram(name: "from_address", linkageName: "_ZNSt7__n486116coroutine_handleIvE12from_addressEPv", scope: !77, file: !36, line: 118, type: !97, scopeLine: 118, flags: DIFlagPrototyped | DIFlagStaticMember, spFlags: 0)
!97 = !DISubroutineType(types: !98)
!98 = !{!77, !41}
!99 = !DISubprogram(name: "operator bool", linkageName: "_ZNKSt7__n486116coroutine_handleIvEcvbEv", scope: !77, file: !36, line: 127, type: !100, scopeLine: 127, flags: DIFlagExplicit | DIFlagPrototyped, spFlags: 0)
!100 = !DISubroutineType(types: !101)
!101 = !{!71, !94}
!102 = !DISubprogram(name: "done", linkageName: "_ZNKSt7__n486116coroutine_handleIvE4doneEv", scope: !77, file: !36, line: 132, type: !100, scopeLine: 132, flags: DIFlagPrototyped, spFlags: 0)
!103 = !DISubprogram(name: "operator()", linkageName: "_ZNKSt7__n486116coroutine_handleIvEclEv", scope: !77, file: !36, line: 135, type: !104, scopeLine: 135, flags: DIFlagPrototyped, spFlags: 0)
!104 = !DISubroutineType(types: !105)
!105 = !{null, !94}
!106 = !DISubprogram(name: "resume", linkageName: "_ZNKSt7__n486116coroutine_handleIvE6resumeEv", scope: !77, file: !36, line: 137, type: !104, scopeLine: 137, flags: DIFlagPrototyped, spFlags: 0)
!107 = !DISubprogram(name: "destroy", linkageName: "_ZNKSt7__n486116coroutine_handleIvE7destroyEv", scope: !77, file: !36, line: 139, type: !104, scopeLine: 139, flags: DIFlagPrototyped, spFlags: 0)
!108 = !{!109}
!109 = !DITemplateTypeParameter(name: "_Promise", type: null, defaulted: true)
!110 = !DISubprogram(name: "await_resume", linkageName: "_ZNKSt7__n486114suspend_always12await_resumeEv", scope: !66, file: !36, line: 330, type: !111, scopeLine: 330, flags: DIFlagPrototyped, spFlags: 0)
!111 = !DISubroutineType(types: !112)
!112 = !{null, !72}
!113 = !DISubprogram(name: "final_suspend", linkageName: "_ZN3gen12promise_type13final_suspendEv", scope: !56, file: !2, line: 16, type: !64, scopeLine: 16, flags: DIFlagPrototyped, spFlags: 0)
!114 = !DISubprogram(name: "yield_value", linkageName: "_ZN3gen12promise_type11yield_valueEi", scope: !56, file: !2, line: 17, type: !115, scopeLine: 17, flags: DIFlagPrototyped, spFlags: 0)
!115 = !DISubroutineType(types: !116)
!116 = !{!66, !62, !19}
!117 = !DISubprogram(name: "return_void", linkageName: "_ZN3gen12promise_type11return_voidEv", scope: !56, file: !2, line: 18, type: !118, scopeLine: 18, flags: DIFlagPrototyped, spFlags: 0)
!118 = !DISubroutineType(types: !119)
!119 = !{null, !62}
!120 = !DISubprogram(name: "unhandled_exception", linkageName: "_ZN3gen12promise_type19unhandled_exceptionEv", scope: !56, file: !2, line: 19, type: !118, scopeLine: 19, flags: DIFlagPrototyped, spFlags: 0)
!121 = !DISubprogram(name: "operator=", linkageName: "_ZNSt7__n486116coroutine_handleIN3gen12promise_typeEEaSEDn", scope: !35, file: !36, line: 210, type: !122, scopeLine: 210, flags: DIFlagPrototyped, spFlags: 0)
!122 = !DISubroutineType(types: !123)
!123 = !{!124, !45, !49}
!124 = !DIDerivedType(tag: DW_TAG_reference_type, baseType: !35, size: 64)
!125 = !DISubprogram(name: "address", linkageName: "_ZNKSt7__n486116coroutine_handleIN3gen12promise_typeEE7addressEv", scope: !35, file: !36, line: 218, type: !126, scopeLine: 218, flags: DIFlagPrototyped, spFlags: 0)
!126 = !DISubroutineType(types: !127)
!127 = !{!41, !128}
!128 = !DIDerivedType(tag: DW_TAG_pointer_type, baseType: !129, size: 64, flags: DIFlagArtificial | DIFlagObjectPointer)
!129 = !DIDerivedType(tag: DW_TAG_const_type, baseType: !35)
!130 = !DISubprogram(name: "from_address", linkageName: "_ZNSt7__n486116coroutine_handleIN3gen12promise_typeEE12from_addressEPv", scope: !35, file: !36, line: 220, type: !131, scopeLine: 220, flags: DIFlagPrototyped | DIFlagStaticMember, spFlags: 0)
!131 = !DISubroutineType(types: !132)
!132 = !{!35, !41}
!133 = !DISubprogram(name: "operator coroutine_handle", linkageName: "_ZNKSt7__n486116coroutine_handleIN3gen12promise_typeEEcvNS0_IvEEEv", scope: !35, file: !36, line: 228, type: !134, scopeLine: 228, flags: DIFlagPrototyped, spFlags: 0)
!134 = !DISubroutineType(types: !135)
!135 = !{!77, !128}
!136 = !DISubprogram(name: "operator bool", linkageName: "_ZNKSt7__n486116coroutine_handleIN3gen12promise_typeEEcvbEv", scope: !35, file: !36, line: 232, type: !137, scopeLine: 232, flags: DIFlagExplicit | DIFlagPrototyped, spFlags: 0)
!137 = !DISubroutineType(types: !138)
!138 = !{!71, !128}
!139 = !DISubprogram(name: "done", linkageName: "_ZNKSt7__n486116coroutine_handleIN3gen12promise_typeEE4doneEv", scope: !35, file: !36, line: 237, type: !137, scopeLine: 237, flags: DIFlagPrototyped, spFlags: 0)
!140 = !DISubprogram(name: "operator()", linkageName: "_ZNKSt7__n486116coroutine_handleIN3gen12promise_typeEEclEv", scope: !35, file: !36, line: 240, type: !141, scopeLine: 240, flags: DIFlagPrototyped, spFlags: 0)
!141 = !DISubroutineType(types: !142)
!142 = !{null, !128}
!143 = !DISubprogram(name: "resume", linkageName: "_ZNKSt7__n486116coroutine_handleIN3gen12promise_typeEE6resumeEv", scope: !35, file: !36, line: 242, type: !141, scopeLine: 242, flags: DIFlagPrototyped, spFlags: 0)
!144 = !DISubprogram(name: "destroy", linkageName: "_ZNKSt7__n486116coroutine_handleIN3gen12promise_typeEE7destroyEv", scope: !35, file: !36, line: 244, type: !141, scopeLine: 244, flags: DIFlagPrototyped, spFlags: 0)
!145 = !DISubprogram(name: "promise", linkageName: "_ZNKSt7__n486116coroutine_handleIN3gen12promise_typeEE7promiseEv", scope: !35, file: !36, line: 247, type: !146, scopeLine: 247, flags: DIFlagPrototyped, spFlags: 0)
!146 = !DISubroutineType(types: !147)
!147 = !{!55, !128}
!148 = !{!149}
!149 = !DITemplateTypeParameter(name: "_Promise", type: !56)
!150 = !DIDerivedType(tag: DW_TAG_pointer_type, baseType: !5, size: 64)
!151 = !DIDerivedType(tag: DW_TAG_pointer_type, baseType: !56, size: 64)
!152 = !{!0, !8}
!153 = !{!154, !160, !166, !171, !175, !177, !179, !181, !183, !190, !195, !201, !205, !209, !213, !221, !225, !227, !232, !238, !242, !249, !251, !255, !259, !263, !265, !269, !273, !275, !279, !281, !283, !287, !291, !295, !299, !303, !307, !309, !317, !321, !325, !330, !332, !334, !338, !342, !343, !344, !345, !346}
!154 = !DIImportedEntity(tag: DW_TAG_imported_declaration, scope: !38, entity: !155, file: !159, line: 98)
!155 = !DIDerivedType(tag: DW_TAG_typedef, name: "FILE", file: !156, line: 7, baseType: !157)
!156 = !DIFile(filename: "/usr/include/aarch64-linux-gnu/bits/types/FILE.h", directory: "", checksumkind: CSK_MD5, checksum: "571f9fb6223c42439075fdde11a0de5d")
!157 = !DICompositeType(tag: DW_TAG_structure_type, name: "_IO_FILE", file: !158, line: 50, size: 1728, flags: DIFlagFwdDecl, identifier: "_ZTS8_IO_FILE")
!158 = !DIFile(filename: "/usr/include/aarch64-linux-gnu/bits/types/struct_FILE.h", directory: "", checksumkind: CSK_MD5, checksum: "8b2aa9af4ced87e40d6c436b1f5f8bda")
!159 = !DIFile(filename: "/usr/lib/gcc/aarch64-linux-gnu/14/../../../../include/c++/14/cstdio", directory: "")
!160 = !DIImportedEntity(tag: DW_TAG_imported_declaration, scope: !38, entity: !161, file: !159, line: 99)
!161 = !DIDerivedType(tag: DW_TAG_typedef, name: "fpos_t", file: !162, line: 85, baseType: !163)
!162 = !DIFile(filename: "/usr/include/stdio.h", directory: "", checksumkind: CSK_MD5, checksum: "47768dac06badcdb9c30a3d8abe4063b")
!163 = !DIDerivedType(tag: DW_TAG_typedef, name: "__fpos_t", file: !164, line: 14, baseType: !165)
!164 = !DIFile(filename: "/usr/include/aarch64-linux-gnu/bits/types/__fpos_t.h", directory: "", checksumkind: CSK_MD5, checksum: "32de8bdaf3551a6c0a9394f9af4389ce")
!165 = !DICompositeType(tag: DW_TAG_structure_type, name: "_G_fpos_t", file: !164, line: 10, size: 128, flags: DIFlagFwdDecl, identifier: "_ZTS9_G_fpos_t")
!166 = !DIImportedEntity(tag: DW_TAG_imported_declaration, scope: !38, entity: !167, file: !159, line: 101)
!167 = !DISubprogram(name: "clearerr", scope: !162, file: !162, line: 850, type: !168, flags: DIFlagPrototyped, spFlags: 0)
!168 = !DISubroutineType(types: !169)
!169 = !{null, !170}
!170 = !DIDerivedType(tag: DW_TAG_pointer_type, baseType: !155, size: 64)
!171 = !DIImportedEntity(tag: DW_TAG_imported_declaration, scope: !38, entity: !172, file: !159, line: 102)
!172 = !DISubprogram(name: "fclose", scope: !162, file: !162, line: 184, type: !173, flags: DIFlagPrototyped, spFlags: 0)
!173 = !DISubroutineType(types: !174)
!174 = !{!19, !170}
!175 = !DIImportedEntity(tag: DW_TAG_imported_declaration, scope: !38, entity: !176, file: !159, line: 103)
!176 = !DISubprogram(name: "feof", scope: !162, file: !162, line: 852, type: !173, flags: DIFlagPrototyped, spFlags: 0)
!177 = !DIImportedEntity(tag: DW_TAG_imported_declaration, scope: !38, entity: !178, file: !159, line: 104)
!178 = !DISubprogram(name: "ferror", scope: !162, file: !162, line: 854, type: !173, flags: DIFlagPrototyped, spFlags: 0)
!179 = !DIImportedEntity(tag: DW_TAG_imported_declaration, scope: !38, entity: !180, file: !159, line: 105)
!180 = !DISubprogram(name: "fflush", scope: !162, file: !162, line: 236, type: !173, flags: DIFlagPrototyped, spFlags: 0)
!181 = !DIImportedEntity(tag: DW_TAG_imported_declaration, scope: !38, entity: !182, file: !159, line: 106)
!182 = !DISubprogram(name: "fgetc", scope: !162, file: !162, line: 575, type: !173, flags: DIFlagPrototyped, spFlags: 0)
!183 = !DIImportedEntity(tag: DW_TAG_imported_declaration, scope: !38, entity: !184, file: !159, line: 107)
!184 = !DISubprogram(name: "fgetpos", scope: !162, file: !162, line: 819, type: !185, flags: DIFlagPrototyped, spFlags: 0)
!185 = !DISubroutineType(types: !186)
!186 = !{!19, !187, !188}
!187 = !DIDerivedType(tag: DW_TAG_restrict_type, baseType: !170)
!188 = !DIDerivedType(tag: DW_TAG_restrict_type, baseType: !189)
!189 = !DIDerivedType(tag: DW_TAG_pointer_type, baseType: !161, size: 64)
!190 = !DIImportedEntity(tag: DW_TAG_imported_declaration, scope: !38, entity: !191, file: !159, line: 108)
!191 = !DISubprogram(name: "fgets", scope: !162, file: !162, line: 654, type: !192, flags: DIFlagPrototyped, spFlags: 0)
!192 = !DISubroutineType(types: !193)
!193 = !{!150, !194, !19, !187}
!194 = !DIDerivedType(tag: DW_TAG_restrict_type, baseType: !150)
!195 = !DIImportedEntity(tag: DW_TAG_imported_declaration, scope: !38, entity: !196, file: !159, line: 109)
!196 = !DISubprogram(name: "fopen", scope: !162, file: !162, line: 264, type: !197, flags: DIFlagPrototyped, spFlags: 0)
!197 = !DISubroutineType(types: !198)
!198 = !{!170, !199, !199}
!199 = !DIDerivedType(tag: DW_TAG_restrict_type, baseType: !200)
!200 = !DIDerivedType(tag: DW_TAG_pointer_type, baseType: !4, size: 64)
!201 = !DIImportedEntity(tag: DW_TAG_imported_declaration, scope: !38, entity: !202, file: !159, line: 110)
!202 = !DISubprogram(name: "fprintf", scope: !162, file: !162, line: 357, type: !203, flags: DIFlagPrototyped, spFlags: 0)
!203 = !DISubroutineType(types: !204)
!204 = !{!19, !187, !199, null}
!205 = !DIImportedEntity(tag: DW_TAG_imported_declaration, scope: !38, entity: !206, file: !159, line: 111)
!206 = !DISubprogram(name: "fputc", scope: !162, file: !162, line: 611, type: !207, flags: DIFlagPrototyped, spFlags: 0)
!207 = !DISubroutineType(types: !208)
!208 = !{!19, !19, !170}
!209 = !DIImportedEntity(tag: DW_TAG_imported_declaration, scope: !38, entity: !210, file: !159, line: 112)
!210 = !DISubprogram(name: "fputs", scope: !162, file: !162, line: 707, type: !211, flags: DIFlagPrototyped, spFlags: 0)
!211 = !DISubroutineType(types: !212)
!212 = !{!19, !199, !187}
!213 = !DIImportedEntity(tag: DW_TAG_imported_declaration, scope: !38, entity: !214, file: !159, line: 113)
!214 = !DISubprogram(name: "fread", scope: !162, file: !162, line: 728, type: !215, flags: DIFlagPrototyped, spFlags: 0)
!215 = !DISubroutineType(types: !216)
!216 = !{!217, !220, !217, !217, !187}
!217 = !DIDerivedType(tag: DW_TAG_typedef, name: "size_t", file: !218, line: 18, baseType: !219)
!218 = !DIFile(filename: "/usr/lib/llvm-19/lib/clang/19/include/__stddef_size_t.h", directory: "", checksumkind: CSK_MD5, checksum: "2c44e821a2b1951cde2eb0fb2e656867")
!219 = !DIBasicType(name: "unsigned long", size: 64, encoding: DW_ATE_unsigned)
!220 = !DIDerivedType(tag: DW_TAG_restrict_type, baseType: !41)
!221 = !DIImportedEntity(tag: DW_TAG_imported_declaration, scope: !38, entity: !222, file: !159, line: 114)
!222 = !DISubprogram(name: "freopen", scope: !162, file: !162, line: 271, type: !223, flags: DIFlagPrototyped, spFlags: 0)
!223 = !DISubroutineType(types: !224)
!224 = !{!170, !199, !199, !187}
!225 = !DIImportedEntity(tag: DW_TAG_imported_declaration, scope: !38, entity: !226, file: !159, line: 115)
!226 = !DISubprogram(name: "fscanf", linkageName: "__isoc23_fscanf", scope: !162, file: !162, line: 442, type: !203, flags: DIFlagPrototyped, spFlags: 0)
!227 = !DIImportedEntity(tag: DW_TAG_imported_declaration, scope: !38, entity: !228, file: !159, line: 116)
!228 = !DISubprogram(name: "fseek", scope: !162, file: !162, line: 769, type: !229, flags: DIFlagPrototyped, spFlags: 0)
!229 = !DISubroutineType(types: !230)
!230 = !{!19, !170, !231, !19}
!231 = !DIBasicType(name: "long", size: 64, encoding: DW_ATE_signed)
!232 = !DIImportedEntity(tag: DW_TAG_imported_declaration, scope: !38, entity: !233, file: !159, line: 117)
!233 = !DISubprogram(name: "fsetpos", scope: !162, file: !162, line: 825, type: !234, flags: DIFlagPrototyped, spFlags: 0)
!234 = !DISubroutineType(types: !235)
!235 = !{!19, !170, !236}
!236 = !DIDerivedType(tag: DW_TAG_pointer_type, baseType: !237, size: 64)
!237 = !DIDerivedType(tag: DW_TAG_const_type, baseType: !161)
!238 = !DIImportedEntity(tag: DW_TAG_imported_declaration, scope: !38, entity: !239, file: !159, line: 118)
!239 = !DISubprogram(name: "ftell", scope: !162, file: !162, line: 775, type: !240, flags: DIFlagPrototyped, spFlags: 0)
!240 = !DISubroutineType(types: !241)
!241 = !{!231, !170}
!242 = !DIImportedEntity(tag: DW_TAG_imported_declaration, scope: !38, entity: !243, file: !159, line: 119)
!243 = !DISubprogram(name: "fwrite", scope: !162, file: !162, line: 735, type: !244, flags: DIFlagPrototyped, spFlags: 0)
!244 = !DISubroutineType(types: !245)
!245 = !{!217, !246, !217, !217, !187}
!246 = !DIDerivedType(tag: DW_TAG_restrict_type, baseType: !247)
!247 = !DIDerivedType(tag: DW_TAG_pointer_type, baseType: !248, size: 64)
!248 = !DIDerivedType(tag: DW_TAG_const_type, baseType: null)
!249 = !DIImportedEntity(tag: DW_TAG_imported_declaration, scope: !38, entity: !250, file: !159, line: 120)
!250 = !DISubprogram(name: "getc", scope: !162, file: !162, line: 576, type: !173, flags: DIFlagPrototyped, spFlags: 0)
!251 = !DIImportedEntity(tag: DW_TAG_imported_declaration, scope: !38, entity: !252, file: !159, line: 121)
!252 = !DISubprogram(name: "getchar", scope: !162, file: !162, line: 582, type: !253, flags: DIFlagPrototyped, spFlags: 0)
!253 = !DISubroutineType(types: !254)
!254 = !{!19}
!255 = !DIImportedEntity(tag: DW_TAG_imported_declaration, scope: !38, entity: !256, file: !159, line: 126)
!256 = !DISubprogram(name: "perror", scope: !162, file: !162, line: 868, type: !257, flags: DIFlagPrototyped, spFlags: 0)
!257 = !DISubroutineType(types: !258)
!258 = !{null, !200}
!259 = !DIImportedEntity(tag: DW_TAG_imported_declaration, scope: !38, entity: !260, file: !159, line: 127)
!260 = !DISubprogram(name: "printf", scope: !162, file: !162, line: 363, type: !261, flags: DIFlagPrototyped, spFlags: 0)
!261 = !DISubroutineType(types: !262)
!262 = !{!19, !199, null}
!263 = !DIImportedEntity(tag: DW_TAG_imported_declaration, scope: !38, entity: !264, file: !159, line: 128)
!264 = !DISubprogram(name: "putc", scope: !162, file: !162, line: 612, type: !207, flags: DIFlagPrototyped, spFlags: 0)
!265 = !DIImportedEntity(tag: DW_TAG_imported_declaration, scope: !38, entity: !266, file: !159, line: 129)
!266 = !DISubprogram(name: "putchar", scope: !162, file: !162, line: 618, type: !267, flags: DIFlagPrototyped, spFlags: 0)
!267 = !DISubroutineType(types: !268)
!268 = !{!19, !19}
!269 = !DIImportedEntity(tag: DW_TAG_imported_declaration, scope: !38, entity: !270, file: !159, line: 130)
!270 = !DISubprogram(name: "puts", scope: !162, file: !162, line: 714, type: !271, flags: DIFlagPrototyped, spFlags: 0)
!271 = !DISubroutineType(types: !272)
!272 = !{!19, !200}
!273 = !DIImportedEntity(tag: DW_TAG_imported_declaration, scope: !38, entity: !274, file: !159, line: 131)
!274 = !DISubprogram(name: "remove", scope: !162, file: !162, line: 158, type: !271, flags: DIFlagPrototyped, spFlags: 0)
!275 = !DIImportedEntity(tag: DW_TAG_imported_declaration, scope: !38, entity: !276, file: !159, line: 132)
!276 = !DISubprogram(name: "rename", scope: !162, file: !162, line: 160, type: !277, flags: DIFlagPrototyped, spFlags: 0)
!277 = !DISubroutineType(types: !278)
!278 = !{!19, !200, !200}
!279 = !DIImportedEntity(tag: DW_TAG_imported_declaration, scope: !38, entity: !280, file: !159, line: 133)
!280 = !DISubprogram(name: "rewind", scope: !162, file: !162, line: 780, type: !168, flags: DIFlagPrototyped, spFlags: 0)
!281 = !DIImportedEntity(tag: DW_TAG_imported_declaration, scope: !38, entity: !282, file: !159, line: 134)
!282 = !DISubprogram(name: "scanf", linkageName: "__isoc23_scanf", scope: !162, file: !162, line: 445, type: !261, flags: DIFlagPrototyped, spFlags: 0)
!283 = !DIImportedEntity(tag: DW_TAG_imported_declaration, scope: !38, entity: !284, file: !159, line: 135)
!284 = !DISubprogram(name: "setbuf", scope: !162, file: !162, line: 334, type: !285, flags: DIFlagPrototyped, spFlags: 0)
!285 = !DISubroutineType(types: !286)
!286 = !{null, !187, !194}
!287 = !DIImportedEntity(tag: DW_TAG_imported_declaration, scope: !38, entity: !288, file: !159, line: 136)
!288 = !DISubprogram(name: "setvbuf", scope: !162, file: !162, line: 339, type: !289, flags: DIFlagPrototyped, spFlags: 0)
!289 = !DISubroutineType(types: !290)
!290 = !{!19, !187, !194, !19, !217}
!291 = !DIImportedEntity(tag: DW_TAG_imported_declaration, scope: !38, entity: !292, file: !159, line: 137)
!292 = !DISubprogram(name: "sprintf", scope: !162, file: !162, line: 365, type: !293, flags: DIFlagPrototyped, spFlags: 0)
!293 = !DISubroutineType(types: !294)
!294 = !{!19, !194, !199, null}
!295 = !DIImportedEntity(tag: DW_TAG_imported_declaration, scope: !38, entity: !296, file: !159, line: 138)
!296 = !DISubprogram(name: "sscanf", linkageName: "__isoc23_sscanf", scope: !162, file: !162, line: 447, type: !297, flags: DIFlagPrototyped, spFlags: 0)
!297 = !DISubroutineType(types: !298)
!298 = !{!19, !199, !199, null}
!299 = !DIImportedEntity(tag: DW_TAG_imported_declaration, scope: !38, entity: !300, file: !159, line: 139)
!300 = !DISubprogram(name: "tmpfile", scope: !162, file: !162, line: 194, type: !301, flags: DIFlagPrototyped, spFlags: 0)
!301 = !DISubroutineType(types: !302)
!302 = !{!170}
!303 = !DIImportedEntity(tag: DW_TAG_imported_declaration, scope: !38, entity: !304, file: !159, line: 141)
!304 = !DISubprogram(name: "tmpnam", scope: !162, file: !162, line: 211, type: !305, flags: DIFlagPrototyped, spFlags: 0)
!305 = !DISubroutineType(types: !306)
!306 = !{!150, !150}
!307 = !DIImportedEntity(tag: DW_TAG_imported_declaration, scope: !38, entity: !308, file: !159, line: 143)
!308 = !DISubprogram(name: "ungetc", scope: !162, file: !162, line: 721, type: !207, flags: DIFlagPrototyped, spFlags: 0)
!309 = !DIImportedEntity(tag: DW_TAG_imported_declaration, scope: !38, entity: !310, file: !159, line: 144)
!310 = !DISubprogram(name: "vfprintf", scope: !162, file: !162, line: 372, type: !311, flags: DIFlagPrototyped, spFlags: 0)
!311 = !DISubroutineType(types: !312)
!312 = !{!19, !187, !199, !313}
!313 = !DIDerivedType(tag: DW_TAG_typedef, name: "__gnuc_va_list", file: !314, line: 12, baseType: !315)
!314 = !DIFile(filename: "/usr/lib/llvm-19/lib/clang/19/include/__stdarg___gnuc_va_list.h", directory: "", checksumkind: CSK_MD5, checksum: "edb3f2eab991638e4dc94f6e55e3530f")
!315 = !DIDerivedType(tag: DW_TAG_typedef, name: "__builtin_va_list", file: !2, baseType: !316)
!316 = !DICompositeType(tag: DW_TAG_structure_type, name: "__va_list", scope: !38, file: !2, size: 256, flags: DIFlagFwdDecl, identifier: "_ZTSSt9__va_list")
!317 = !DIImportedEntity(tag: DW_TAG_imported_declaration, scope: !38, entity: !318, file: !159, line: 145)
!318 = !DISubprogram(name: "vprintf", scope: !162, file: !162, line: 378, type: !319, flags: DIFlagPrototyped, spFlags: 0)
!319 = !DISubroutineType(types: !320)
!320 = !{!19, !199, !313}
!321 = !DIImportedEntity(tag: DW_TAG_imported_declaration, scope: !38, entity: !322, file: !159, line: 146)
!322 = !DISubprogram(name: "vsprintf", scope: !162, file: !162, line: 380, type: !323, flags: DIFlagPrototyped, spFlags: 0)
!323 = !DISubroutineType(types: !324)
!324 = !{!19, !194, !199, !313}
!325 = !DIImportedEntity(tag: DW_TAG_imported_declaration, scope: !326, entity: !327, file: !159, line: 175)
!326 = !DINamespace(name: "__gnu_cxx", scope: null)
!327 = !DISubprogram(name: "snprintf", scope: !162, file: !162, line: 385, type: !328, flags: DIFlagPrototyped, spFlags: 0)
!328 = !DISubroutineType(types: !329)
!329 = !{!19, !194, !217, !199, null}
!330 = !DIImportedEntity(tag: DW_TAG_imported_declaration, scope: !326, entity: !331, file: !159, line: 176)
!331 = !DISubprogram(name: "vfscanf", linkageName: "__isoc23_vfscanf", scope: !162, file: !162, line: 511, type: !311, flags: DIFlagPrototyped, spFlags: 0)
!332 = !DIImportedEntity(tag: DW_TAG_imported_declaration, scope: !326, entity: !333, file: !159, line: 177)
!333 = !DISubprogram(name: "vscanf", linkageName: "__isoc23_vscanf", scope: !162, file: !162, line: 516, type: !319, flags: DIFlagPrototyped, spFlags: 0)
!334 = !DIImportedEntity(tag: DW_TAG_imported_declaration, scope: !326, entity: !335, file: !159, line: 178)
!335 = !DISubprogram(name: "vsnprintf", scope: !162, file: !162, line: 389, type: !336, flags: DIFlagPrototyped, spFlags: 0)
!336 = !DISubroutineType(types: !337)
!337 = !{!19, !194, !217, !199, !313}
!338 = !DIImportedEntity(tag: DW_TAG_imported_declaration, scope: !326, entity: !339, file: !159, line: 179)
!339 = !DISubprogram(name: "vsscanf", linkageName: "__isoc23_vsscanf", scope: !162, file: !162, line: 519, type: !340, flags: DIFlagPrototyped, spFlags: 0)
!340 = !DISubroutineType(types: !341)
!341 = !{!19, !199, !199, !313}
!342 = !DIImportedEntity(tag: DW_TAG_imported_declaration, scope: !38, entity: !327, file: !159, line: 185)
!343 = !DIImportedEntity(tag: DW_TAG_imported_declaration, scope: !38, entity: !331, file: !159, line: 186)
!344 = !DIImportedEntity(tag: DW_TAG_imported_declaration, scope: !38, entity: !333, file: !159, line: 187)
!345 = !DIImportedEntity(tag: DW_TAG_imported_declaration, scope: !38, entity: !335, file: !159, line: 188)
!346 = !DIImportedEntity(tag: DW_TAG_imported_declaration, scope: !38, entity: !339, file: !159, line: 189)
!347 = !{i32 7, !"Dwarf Version", i32 5}
!348 = !{i32 2, !"Debug Info Version", i32 3}
!349 = !{i32 1, !"wchar_size", i32 4}
!350 = !{i32 8, !"PIC Level", i32 2}
!351 = !{i32 7, !"PIE Level", i32 2}
!352 = !{i32 7, !"uwtable", i32 2}
!353 = !{i32 7, !"frame-pointer", i32 1}
!354 = !{!"Debian clang version 19.1.7 (3+b1)"}
!355 = distinct !DISubprogram(name: "count", linkageName: "_Z5count3bigi", scope: !2, file: !2, line: 23, type: !356, scopeLine: 23, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !13, retainedNodes: !358)
!356 = !DISubroutineType(types: !357)
!357 = !{!32, !15, !19}
!358 = !{}
!359 = !DILocalVariable(name: "b", arg: 1, scope: !355, file: !2, line: 23, type: !15)
!360 = !DILocation(line: 23, column: 15, scope: !355)
!361 = !DILocalVariable(name: "n", arg: 2, scope: !355, file: !2, line: 23, type: !19)
!362 = !DILocation(line: 23, column: 22, scope: !355)
!363 = !DILocation(line: 23, column: 25, scope: !355)
!364 = !DILocation(line: 23, column: 5, scope: !355)
!365 = !DILocation(line: 24, column: 31, scope: !355)
!366 = !DILocation(line: 0, scope: !355)
!367 = !DILocalVariable(name: "__promise", scope: !355, type: !368, flags: DIFlagArtificial)
!368 = !DIDerivedType(tag: DW_TAG_typedef, name: "promise_type", scope: !369, file: !36, line: 86, baseType: !56)
!369 = distinct !DICompositeType(tag: DW_TAG_structure_type, name: "__coroutine_traits_impl<gen, void>", scope: !37, file: !36, line: 80, size: 8, flags: DIFlagTypePassByValue, elements: !358, templateParams: !370, identifier: "_ZTSNSt7__n486123__coroutine_traits_implI3genvEE")
!370 = !{!371, !372}
!371 = !DITemplateTypeParameter(name: "_Result", type: !32)
!372 = !DITemplateTypeParameter(type: null, defaulted: true)
!373 = !DILocation(line: 25, column: 1, scope: !355)
!374 = !DILocation(line: 24, column: 8, scope: !375)
!375 = distinct !DILexicalBlock(scope: !376, file: !2, line: 24, column: 3)
!376 = distinct !DILexicalBlock(scope: !355, file: !2, line: 23, column: 25)
!377 = !DILocalVariable(name: "i", scope: !375, file: !2, line: 24, type: !19)
!378 = !DILocation(line: 24, column: 12, scope: !375)
!379 = !DILocation(line: 24, column: 19, scope: !380)
!380 = distinct !DILexicalBlock(scope: !375, file: !2, line: 24, column: 3)
!381 = !DILocation(line: 24, column: 23, scope: !380)
!382 = !DILocation(line: 24, column: 21, scope: !380)
!383 = !DILocation(line: 24, column: 3, scope: !375)
!384 = !DILocation(line: 24, column: 31, scope: !380)
!385 = !DILocation(line: 24, column: 42, scope: !380)
!386 = !DILocation(line: 24, column: 44, scope: !380)
!387 = !DILocation(line: 24, column: 40, scope: !380)
!388 = !DILocation(line: 24, column: 47, scope: !380)
!389 = !DILocation(line: 24, column: 54, scope: !380)
!390 = !DILocation(line: 24, column: 52, scope: !380)
!391 = !DILocation(line: 24, column: 26, scope: !380)
!392 = !DILocation(line: 24, column: 3, scope: !380)
!393 = distinct !{!393, !383, !394, !395}
!394 = !DILocation(line: 24, column: 54, scope: !375)
!395 = !{!"llvm.loop.mustprogress"}
!396 = distinct !DISubprogram(name: "big", linkageName: "_ZN3bigC2ERKS_", scope: !15, file: !2, line: 8, type: !25, scopeLine: 8, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !13, declaration: !24, retainedNodes: !358)
!397 = !DILocalVariable(name: "this", arg: 1, scope: !396, type: !398, flags: DIFlagArtificial | DIFlagObjectPointer)
!398 = !DIDerivedType(tag: DW_TAG_pointer_type, baseType: !15, size: 64)
!399 = !DILocation(line: 0, scope: !396)
!400 = !DILocalVariable(name: "o", arg: 2, scope: !396, file: !2, line: 8, type: !27)
!401 = !DILocation(line: 8, column: 18, scope: !396)
!402 = !DILocalVariable(name: "i", scope: !403, file: !2, line: 8, type: !19)
!403 = distinct !DILexicalBlock(scope: !404, file: !2, line: 8, column: 23)
!404 = distinct !DILexicalBlock(scope: !396, file: !2, line: 8, column: 21)
!405 = !DILocation(line: 8, column: 32, scope: !403)
!406 = !DILocation(line: 8, column: 28, scope: !403)
!407 = !DILocation(line: 8, column: 39, scope: !408)
!408 = distinct !DILexicalBlock(scope: !403, file: !2, line: 8, column: 23)
!409 = !DILocation(line: 8, column: 41, scope: !408)
!410 = !DILocation(line: 8, column: 23, scope: !403)
!411 = !DILocation(line: 8, column: 58, scope: !408)
!412 = !DILocation(line: 8, column: 60, scope: !408)
!413 = !DILocation(line: 8, column: 62, scope: !408)
!414 = !DILocation(line: 8, column: 51, scope: !408)
!415 = !DILocation(line: 8, column: 53, scope: !408)
!416 = !DILocation(line: 8, column: 56, scope: !408)
!417 = !DILocation(line: 8, column: 46, scope: !408)
!418 = !DILocation(line: 8, column: 23, scope: !408)
!419 = distinct !{!419, !410, !420, !395}
!420 = !DILocation(line: 8, column: 63, scope: !403)
!421 = !DILocation(line: 8, column: 66, scope: !396)
!422 = distinct !DISubprogram(name: "promise_type", linkageName: "_ZN3gen12promise_typeC2Ev", scope: !56, file: !2, line: 12, type: !118, scopeLine: 12, flags: DIFlagArtificial | DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !13, declaration: !423, retainedNodes: !358)
!423 = !DISubprogram(name: "promise_type", scope: !56, type: !118, flags: DIFlagArtificial | DIFlagPrototyped, spFlags: 0)
!424 = !DILocalVariable(name: "this", arg: 1, scope: !422, type: !151, flags: DIFlagArtificial | DIFlagObjectPointer)
!425 = !DILocation(line: 0, scope: !422)
!426 = !DILocation(line: 13, column: 9, scope: !422)
!427 = !DILocation(line: 12, column: 10, scope: !422)
!428 = distinct !DISubprogram(name: "get_return_object", linkageName: "_ZN3gen12promise_type17get_return_objectEv", scope: !56, file: !2, line: 14, type: !60, scopeLine: 14, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !13, declaration: !59, retainedNodes: !358)
!429 = !DILocalVariable(name: "this", arg: 1, scope: !428, type: !151, flags: DIFlagArtificial | DIFlagObjectPointer)
!430 = !DILocation(line: 0, scope: !428)
!431 = !DILocation(line: 14, column: 41, scope: !428)
!432 = !DILocation(line: 14, column: 42, scope: !428)
!433 = !DILocation(line: 14, column: 31, scope: !428)
!434 = distinct !DISubprogram(name: "initial_suspend", linkageName: "_ZN3gen12promise_type15initial_suspendEv", scope: !56, file: !2, line: 15, type: !64, scopeLine: 15, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !13, declaration: !63, retainedNodes: !358)
!435 = !DILocalVariable(name: "this", arg: 1, scope: !434, type: !151, flags: DIFlagArtificial | DIFlagObjectPointer)
!436 = !DILocation(line: 0, scope: !434)
!437 = !DILocation(line: 15, column: 54, scope: !434)
!438 = distinct !DISubprogram(name: "await_ready", linkageName: "_ZNKSt7__n486114suspend_always11await_readyEv", scope: !66, file: !36, line: 326, type: !69, scopeLine: 326, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !13, declaration: !68, retainedNodes: !358)
!439 = !DILocalVariable(name: "this", arg: 1, scope: !438, type: !440, flags: DIFlagArtificial | DIFlagObjectPointer)
!440 = !DIDerivedType(tag: DW_TAG_pointer_type, baseType: !73, size: 64)
!441 = !DILocation(line: 0, scope: !438)
!442 = !DILocation(line: 326, column: 51, scope: !438)
!443 = distinct !DISubprogram(linkageName: "_Z5count3bigi.__await_suspend_wrapper__init", scope: !2, file: !2, type: !444, flags: DIFlagArtificial, spFlags: DISPFlagLocalToUnit | DISPFlagDefinition, unit: !13, retainedNodes: !358)
!444 = !DISubroutineType(types: !358)
!445 = !DILocalVariable(arg: 1, scope: !443, type: !41, flags: DIFlagArtificial)
!446 = !DILocation(line: 0, scope: !443)
!447 = !DILocalVariable(arg: 2, scope: !443, type: !41, flags: DIFlagArtificial)
!448 = !DILocation(line: 23, column: 5, scope: !443)
!449 = distinct !DISubprogram(name: "await_suspend", linkageName: "_ZNKSt7__n486114suspend_always13await_suspendENS_16coroutine_handleIvEE", scope: !66, file: !36, line: 328, type: !75, scopeLine: 328, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !13, declaration: !74, retainedNodes: !358)
!450 = !DILocalVariable(name: "this", arg: 1, scope: !449, type: !440, flags: DIFlagArtificial | DIFlagObjectPointer)
!451 = !DILocation(line: 0, scope: !449)
!452 = !DILocalVariable(arg: 2, scope: !449, file: !36, line: 328, type: !77)
!453 = !DILocation(line: 328, column: 52, scope: !449)
!454 = !DILocation(line: 328, column: 70, scope: !449)
!455 = distinct !DISubprogram(name: "from_address", linkageName: "_ZNSt7__n486116coroutine_handleIN3gen12promise_typeEE12from_addressEPv", scope: !35, file: !36, line: 220, type: !131, scopeLine: 221, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !13, declaration: !130, retainedNodes: !358)
!456 = !DILocalVariable(name: "__a", arg: 1, scope: !455, file: !36, line: 220, type: !41)
!457 = !DILocation(line: 220, column: 60, scope: !455)
!458 = !DILocalVariable(name: "__self", scope: !455, file: !36, line: 222, type: !35)
!459 = !DILocation(line: 222, column: 19, scope: !455)
!460 = !DILocation(line: 223, column: 21, scope: !455)
!461 = !DILocation(line: 223, column: 9, scope: !455)
!462 = !DILocation(line: 223, column: 19, scope: !455)
!463 = !DILocation(line: 224, column: 2, scope: !455)
!464 = distinct !DISubprogram(name: "operator coroutine_handle", linkageName: "_ZNKSt7__n486116coroutine_handleIN3gen12promise_typeEEcvNS0_IvEEEv", scope: !35, file: !36, line: 228, type: !134, scopeLine: 229, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !13, declaration: !133, retainedNodes: !358)
!465 = !DILocalVariable(name: "this", arg: 1, scope: !464, type: !466, flags: DIFlagArtificial | DIFlagObjectPointer)
!466 = !DIDerivedType(tag: DW_TAG_pointer_type, baseType: !129, size: 64)
!467 = !DILocation(line: 0, scope: !464)
!468 = !DILocation(line: 229, column: 49, scope: !464)
!469 = !DILocation(line: 229, column: 16, scope: !464)
!470 = !DILocation(line: 229, column: 9, scope: !464)
!471 = distinct !DISubprogram(name: "await_resume", linkageName: "_ZNKSt7__n486114suspend_always12await_resumeEv", scope: !66, file: !36, line: 330, type: !111, scopeLine: 330, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !13, declaration: !110, retainedNodes: !358)
!472 = !DILocalVariable(name: "this", arg: 1, scope: !471, type: !440, flags: DIFlagArtificial | DIFlagObjectPointer)
!473 = !DILocation(line: 0, scope: !471)
!474 = !DILocation(line: 330, column: 51, scope: !471)
!475 = distinct !DISubprogram(name: "yield_value", linkageName: "_ZN3gen12promise_type11yield_valueEi", scope: !56, file: !2, line: 17, type: !115, scopeLine: 17, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !13, declaration: !114, retainedNodes: !358)
!476 = !DILocalVariable(name: "this", arg: 1, scope: !475, type: !151, flags: DIFlagArtificial | DIFlagObjectPointer)
!477 = !DILocation(line: 0, scope: !475)
!478 = !DILocalVariable(name: "x", arg: 2, scope: !475, file: !2, line: 17, type: !19)
!479 = !DILocation(line: 17, column: 41, scope: !475)
!480 = !DILocation(line: 17, column: 59, scope: !475)
!481 = !DILocation(line: 17, column: 55, scope: !475)
!482 = !DILocation(line: 17, column: 57, scope: !475)
!483 = !DILocation(line: 17, column: 62, scope: !475)
!484 = distinct !DISubprogram(linkageName: "_Z5count3bigi.__await_suspend_wrapper__yield", scope: !2, file: !2, type: !444, flags: DIFlagArtificial, spFlags: DISPFlagLocalToUnit | DISPFlagDefinition, unit: !13, retainedNodes: !358)
!485 = !DILocalVariable(arg: 1, scope: !484, type: !41, flags: DIFlagArtificial)
!486 = !DILocation(line: 0, scope: !484)
!487 = !DILocalVariable(arg: 2, scope: !484, type: !41, flags: DIFlagArtificial)
!488 = !DILocation(line: 24, column: 31, scope: !484)
!489 = distinct !DISubprogram(name: "return_void", linkageName: "_ZN3gen12promise_type11return_voidEv", scope: !56, file: !2, line: 18, type: !118, scopeLine: 18, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !13, declaration: !117, retainedNodes: !358)
!490 = !DILocalVariable(name: "this", arg: 1, scope: !489, type: !151, flags: DIFlagArtificial | DIFlagObjectPointer)
!491 = !DILocation(line: 0, scope: !489)
!492 = !DILocation(line: 18, column: 34, scope: !489)
!493 = distinct !DISubprogram(name: "final_suspend", linkageName: "_ZN3gen12promise_type13final_suspendEv", scope: !56, file: !2, line: 16, type: !64, scopeLine: 16, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !13, declaration: !113, retainedNodes: !358)
!494 = !DILocalVariable(name: "this", arg: 1, scope: !493, type: !151, flags: DIFlagArtificial | DIFlagObjectPointer)
!495 = !DILocation(line: 0, scope: !493)
!496 = !DILocation(line: 16, column: 52, scope: !493)
!497 = distinct !DISubprogram(linkageName: "_Z5count3bigi.__await_suspend_wrapper__final", scope: !2, file: !2, type: !444, flags: DIFlagArtificial, spFlags: DISPFlagLocalToUnit | DISPFlagDefinition, unit: !13, retainedNodes: !358)
!498 = !DILocalVariable(arg: 1, scope: !497, type: !41, flags: DIFlagArtificial)
!499 = !DILocation(line: 0, scope: !497)
!500 = !DILocalVariable(arg: 2, scope: !497, type: !41, flags: DIFlagArtificial)
!501 = !DILocation(line: 23, column: 5, scope: !497)
!502 = distinct !DISubprogram(name: "~big", linkageName: "_ZN3bigD2Ev", scope: !15, file: !2, line: 9, type: !30, scopeLine: 9, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !13, declaration: !29, retainedNodes: !358)
!503 = !DILocalVariable(name: "this", arg: 1, scope: !502, type: !398, flags: DIFlagArtificial | DIFlagObjectPointer)
!504 = !DILocation(line: 0, scope: !502)
!505 = !DILocation(line: 9, column: 37, scope: !506)
!506 = distinct !DILexicalBlock(scope: !502, file: !2, line: 9, column: 10)
!507 = !DILocation(line: 9, column: 12, scope: !506)
!508 = !DILocation(line: 9, column: 44, scope: !502)
!509 = distinct !DISubprogram(name: "main", scope: !2, file: !2, line: 26, type: !253, scopeLine: 26, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !13, retainedNodes: !358)
!510 = !DILocalVariable(name: "g", scope: !509, file: !2, line: 27, type: !32)
!511 = !DILocation(line: 27, column: 7, scope: !509)
!512 = !DILocation(line: 27, column: 17, scope: !509)
!513 = !DILocation(line: 27, column: 11, scope: !509)
!514 = !DILocation(line: 28, column: 10, scope: !515)
!515 = distinct !DILexicalBlock(scope: !509, file: !2, line: 28, column: 3)
!516 = !DILocation(line: 28, column: 12, scope: !515)
!517 = !DILocation(line: 28, column: 8, scope: !515)
!518 = !DILocation(line: 28, column: 25, scope: !519)
!519 = distinct !DILexicalBlock(scope: !515, file: !2, line: 28, column: 3)
!520 = !DILocation(line: 28, column: 27, scope: !519)
!521 = !DILocation(line: 28, column: 22, scope: !519)
!522 = !DILocation(line: 28, column: 3, scope: !515)
!523 = !DILocation(line: 28, column: 71, scope: !519)
!524 = !DILocation(line: 28, column: 73, scope: !519)
!525 = !DILocation(line: 28, column: 83, scope: !519)
!526 = !DILocation(line: 28, column: 49, scope: !519)
!527 = !DILocation(line: 28, column: 37, scope: !519)
!528 = !DILocation(line: 28, column: 39, scope: !519)
!529 = !DILocation(line: 28, column: 3, scope: !519)
!530 = distinct !{!530, !522, !531, !395}
!531 = !DILocation(line: 28, column: 84, scope: !515)
!532 = !DILocation(line: 31, column: 1, scope: !509)
!533 = !DILocation(line: 29, column: 5, scope: !509)
!534 = !DILocation(line: 29, column: 7, scope: !509)
!535 = !DILocation(line: 30, column: 3, scope: !509)
!536 = distinct !DISubprogram(name: "big", linkageName: "_ZN3bigC2Ei", scope: !15, file: !2, line: 7, type: !21, scopeLine: 7, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !13, declaration: !20, retainedNodes: !358)
!537 = !DILocalVariable(name: "this", arg: 1, scope: !536, type: !398, flags: DIFlagArtificial | DIFlagObjectPointer)
!538 = !DILocation(line: 0, scope: !536)
!539 = !DILocalVariable(name: "s", arg: 2, scope: !536, file: !2, line: 7, type: !19)
!540 = !DILocation(line: 7, column: 20, scope: !536)
!541 = !DILocalVariable(name: "i", scope: !542, file: !2, line: 7, type: !19)
!542 = distinct !DILexicalBlock(scope: !543, file: !2, line: 7, column: 25)
!543 = distinct !DILexicalBlock(scope: !536, file: !2, line: 7, column: 23)
!544 = !DILocation(line: 7, column: 34, scope: !542)
!545 = !DILocation(line: 7, column: 30, scope: !542)
!546 = !DILocation(line: 7, column: 41, scope: !547)
!547 = distinct !DILexicalBlock(scope: !542, file: !2, line: 7, column: 25)
!548 = !DILocation(line: 7, column: 43, scope: !547)
!549 = !DILocation(line: 7, column: 25, scope: !542)
!550 = !DILocation(line: 7, column: 60, scope: !547)
!551 = !DILocation(line: 7, column: 64, scope: !547)
!552 = !DILocation(line: 7, column: 62, scope: !547)
!553 = !DILocation(line: 7, column: 53, scope: !547)
!554 = !DILocation(line: 7, column: 55, scope: !547)
!555 = !DILocation(line: 7, column: 58, scope: !547)
!556 = !DILocation(line: 7, column: 48, scope: !547)
!557 = !DILocation(line: 7, column: 25, scope: !547)
!558 = distinct !{!558, !549, !559, !395}
!559 = !DILocation(line: 7, column: 64, scope: !542)
!560 = !DILocation(line: 7, column: 67, scope: !536)
!561 = distinct !DISubprogram(name: "resume", linkageName: "_ZNKSt7__n486116coroutine_handleIN3gen12promise_typeEE6resumeEv", scope: !35, file: !36, line: 242, type: !141, scopeLine: 242, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !13, declaration: !143, retainedNodes: !358)
!562 = !DILocalVariable(name: "this", arg: 1, scope: !561, type: !466, flags: DIFlagArtificial | DIFlagObjectPointer)
!563 = !DILocation(line: 0, scope: !561)
!564 = !DILocation(line: 242, column: 51, scope: !561)
!565 = !DILocation(line: 242, column: 29, scope: !561)
!566 = !DILocation(line: 242, column: 63, scope: !561)
!567 = distinct !DISubprogram(name: "done", linkageName: "_ZNKSt7__n486116coroutine_handleIN3gen12promise_typeEE4doneEv", scope: !35, file: !36, line: 237, type: !137, scopeLine: 237, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !13, declaration: !139, retainedNodes: !358)
!568 = !DILocalVariable(name: "this", arg: 1, scope: !567, type: !466, flags: DIFlagArtificial | DIFlagObjectPointer)
!569 = !DILocation(line: 0, scope: !567)
!570 = !DILocation(line: 237, column: 63, scope: !567)
!571 = !DILocation(line: 237, column: 43, scope: !567)
!572 = !DILocation(line: 237, column: 36, scope: !567)
!573 = distinct !DISubprogram(name: "promise", linkageName: "_ZNKSt7__n486116coroutine_handleIN3gen12promise_typeEE7promiseEv", scope: !35, file: !36, line: 247, type: !146, scopeLine: 248, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !13, declaration: !145, retainedNodes: !358)
!574 = !DILocalVariable(name: "this", arg: 1, scope: !573, type: !466, flags: DIFlagArtificial | DIFlagObjectPointer)
!575 = !DILocation(line: 0, scope: !573)
!576 = !DILocalVariable(name: "__t", scope: !573, file: !36, line: 249, type: !41)
!577 = !DILocation(line: 249, column: 8, scope: !573)
!578 = !DILocation(line: 250, column: 30, scope: !573)
!579 = !DILocation(line: 250, column: 6, scope: !573)
!580 = !DILocation(line: 251, column: 33, scope: !573)
!581 = !DILocation(line: 251, column: 2, scope: !573)
!582 = distinct !DISubprogram(name: "destroy", linkageName: "_ZNKSt7__n486116coroutine_handleIN3gen12promise_typeEE7destroyEv", scope: !35, file: !36, line: 244, type: !141, scopeLine: 244, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !13, declaration: !144, retainedNodes: !358)
!583 = !DILocalVariable(name: "this", arg: 1, scope: !582, type: !466, flags: DIFlagArtificial | DIFlagObjectPointer)
!584 = !DILocation(line: 0, scope: !582)
!585 = !DILocation(line: 244, column: 53, scope: !582)
!586 = !DILocation(line: 244, column: 30, scope: !582)
!587 = !DILocation(line: 244, column: 65, scope: !582)
!588 = distinct !DISubprogram(name: "from_promise", linkageName: "_ZNSt7__n486116coroutine_handleIN3gen12promise_typeEE12from_promiseERS2_", scope: !35, file: !36, line: 202, type: !53, scopeLine: 203, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !13, declaration: !52, retainedNodes: !358)
!589 = !DILocalVariable(name: "__p", arg: 1, scope: !588, file: !36, line: 202, type: !55)
!590 = !DILocation(line: 202, column: 30, scope: !588)
!591 = !DILocalVariable(name: "__self", scope: !588, file: !36, line: 204, type: !35)
!592 = !DILocation(line: 204, column: 19, scope: !588)
!593 = !DILocation(line: 206, column: 38, scope: !588)
!594 = !DILocation(line: 206, column: 6, scope: !588)
!595 = !DILocation(line: 205, column: 9, scope: !588)
!596 = !DILocation(line: 206, column: 4, scope: !588)
!597 = !DILocation(line: 207, column: 2, scope: !588)
!598 = distinct !DISubprogram(name: "coroutine_handle", linkageName: "_ZNSt7__n486116coroutine_handleIN3gen12promise_typeEEC2Ev", scope: !35, file: !36, line: 197, type: !43, scopeLine: 197, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !13, declaration: !42, retainedNodes: !358)
!599 = !DILocalVariable(name: "this", arg: 1, scope: !598, type: !600, flags: DIFlagArtificial | DIFlagObjectPointer)
!600 = !DIDerivedType(tag: DW_TAG_pointer_type, baseType: !35, size: 64)
!601 = !DILocation(line: 0, scope: !598)
!602 = !DILocation(line: 255, column: 13, scope: !598)
!603 = !DILocation(line: 197, column: 47, scope: !598)
!604 = distinct !DISubprogram(name: "from_address", linkageName: "_ZNSt7__n486116coroutine_handleIvE12from_addressEPv", scope: !77, file: !36, line: 118, type: !97, scopeLine: 119, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !13, declaration: !96, retainedNodes: !358)
!605 = !DILocalVariable(name: "__a", arg: 1, scope: !604, file: !36, line: 118, type: !41)
!606 = !DILocation(line: 118, column: 60, scope: !604)
!607 = !DILocalVariable(name: "__self", scope: !604, file: !36, line: 120, type: !77)
!608 = !DILocation(line: 120, column: 19, scope: !604)
!609 = !DILocation(line: 121, column: 21, scope: !604)
!610 = !DILocation(line: 121, column: 9, scope: !604)
!611 = !DILocation(line: 121, column: 19, scope: !604)
!612 = !DILocation(line: 122, column: 2, scope: !604)
!613 = distinct !DISubprogram(name: "address", linkageName: "_ZNKSt7__n486116coroutine_handleIN3gen12promise_typeEE7addressEv", scope: !35, file: !36, line: 218, type: !126, scopeLine: 218, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !13, declaration: !125, retainedNodes: !358)
!614 = !DILocalVariable(name: "this", arg: 1, scope: !613, type: !466, flags: DIFlagArtificial | DIFlagObjectPointer)
!615 = !DILocation(line: 0, scope: !613)
!616 = !DILocation(line: 218, column: 57, scope: !613)
!617 = !DILocation(line: 218, column: 50, scope: !613)
!618 = distinct !DISubprogram(name: "coroutine_handle", linkageName: "_ZNSt7__n486116coroutine_handleIvEC2Ev", scope: !77, file: !36, line: 102, type: !81, scopeLine: 102, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !13, declaration: !80, retainedNodes: !358)
!619 = !DILocalVariable(name: "this", arg: 1, scope: !618, type: !620, flags: DIFlagArtificial | DIFlagObjectPointer)
!620 = !DIDerivedType(tag: DW_TAG_pointer_type, baseType: !77, size: 64)
!621 = !DILocation(line: 0, scope: !618)
!622 = !DILocation(line: 102, column: 47, scope: !618)
!623 = !DILocation(line: 102, column: 67, scope: !618)
