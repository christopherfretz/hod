; kiln++ --llvm-dir=<LLVM 22> -std=c++20 -fclang-compat=22 -g -fkiln-backend=hod: the module kiln gives hod
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
; WP-350: kiln's by-address coroutine parameter at -g, before opt.
;@ covers: spec/ir.md §5.5 intrinsics (coroutine bodies; spec/codegen.md §7.6)
;@ kept-off(i686): lp64-abi
;@ input: pre-opt
;@ dialect: 22
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

@.str = private unnamed_addr constant [4 x i8] c"%d\0A\00", align 1
@.str.1 = private unnamed_addr constant [9 x i8] c"gone %d\0A\00", align 1

define dso_local i64 @_Z5count3bigi(ptr noundef %0, i32 noundef %1) #0 personality ptr @__gxx_personality_v0 !dbg !346 {
  %3 = alloca %struct.gen, align 8
  %4 = alloca ptr, align 8
  %5 = alloca i32, align 4, !coro.outside.frame !349
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
    #dbg_declare(ptr %4, !350, !DIExpression(DW_OP_deref), !351)
  store i32 %1, ptr %5, align 4
    #dbg_declare(ptr %5, !352, !DIExpression(), !353)
  %18 = call token @llvm.coro.id(i32 16, ptr %10, ptr null, ptr null), !dbg !354
  %19 = call i1 @llvm.coro.alloc(token %18), !dbg !354
  br i1 %19, label %20, label %23, !dbg !354

20:
  %21 = call i64 @llvm.coro.size.i64(), !dbg !355
  %22 = call noalias noundef nonnull ptr @_Znwm(i64 noundef %21) #15, !dbg !355
  br label %23, !dbg !354

23:
  %24 = phi ptr [ null, %2 ], [ %22, %20 ], !dbg !354
  %25 = call ptr @llvm.coro.begin(token %18, ptr %24), !dbg !354
  call void @llvm.lifetime.start.p0(ptr %6) #2, !dbg !356
    #dbg_declare(ptr %6, !357, !DIExpression(), !358)
  invoke void @_ZN3bigC2ERKS_(ptr noundef nonnull align 4 dereferenceable(16) %6, ptr noundef nonnull align 4 dereferenceable(16) %0)
          to label %26 unwind label %38, !dbg !356

26:
  call void @llvm.lifetime.start.p0(ptr %9) #2, !dbg !356
    #dbg_declare(ptr %9, !359, !DIExpression(), !358)
  %27 = load i32, ptr %5, align 4, !dbg !356
  store i32 %27, ptr %9, align 4, !dbg !356
  call void @llvm.lifetime.start.p0(ptr %10) #2, !dbg !355
    #dbg_declare(ptr %10, !360, !DIExpression(), !358)
  call void @_ZN3gen12promise_typeC2Ev(ptr noundef nonnull align 4 dereferenceable(4) %10) #2, !dbg !355
  %28 = invoke i64 @_ZN3gen12promise_type17get_return_objectEv(ptr noundef nonnull align 4 dereferenceable(4) %10)
          to label %29 unwind label %42, !dbg !355

29:
  %30 = getelementptr inbounds nuw %struct.gen, ptr %3, i32 0, i32 0, !dbg !355
  %31 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %30, i32 0, i32 0, !dbg !355
  %32 = inttoptr i64 %28 to ptr, !dbg !355
  store ptr %32, ptr %31, align 8, !dbg !355
  call void @llvm.lifetime.start.p0(ptr %11) #2, !dbg !355
  call void @_ZN3gen12promise_type15initial_suspendEv(ptr noundef nonnull align 4 dereferenceable(4) %10) #2, !dbg !355
  %33 = call noundef i1 @_ZNKSt7__n486114suspend_always11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %11) #2, !dbg !355
  br i1 %33, label %46, label %34, !dbg !355

34:
  %35 = call token @llvm.coro.save(ptr null), !dbg !355
  call void @llvm.coro.await.suspend.void(ptr %11, ptr %25, ptr @_Z5count3bigi.__await_suspend_wrapper__init) #2, !dbg !355
  %36 = call i8 @llvm.coro.suspend(token %35, i1 false), !dbg !355
  switch i8 %36, label %97 [
    i8 0, label %46
    i8 1, label %37
  ], !dbg !355

37:
  br label %47, !dbg !355

38:
  %39 = landingpad { ptr, i32 }
          cleanup, !dbg !366
  %40 = extractvalue { ptr, i32 } %39, 0, !dbg !366
  store ptr %40, ptr %7, align 8, !dbg !366
  %41 = extractvalue { ptr, i32 } %39, 1, !dbg !366
  store i32 %41, ptr %8, align 4, !dbg !366
  br label %102, !dbg !366

42:
  %43 = landingpad { ptr, i32 }
          cleanup, !dbg !366
  %44 = extractvalue { ptr, i32 } %43, 0, !dbg !366
  store ptr %44, ptr %7, align 8, !dbg !366
  %45 = extractvalue { ptr, i32 } %43, 1, !dbg !366
  store i32 %45, ptr %8, align 4, !dbg !366
  call void @llvm.lifetime.end.p0(ptr %10) #2, !dbg !355
  call void @llvm.lifetime.end.p0(ptr %9) #2, !dbg !355
  call void @_ZN3bigD2Ev(ptr noundef nonnull align 4 dereferenceable(16) %6) #2, !dbg !355
  br label %102, !dbg !355

46:
  call void @_ZNKSt7__n486114suspend_always12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %11) #2, !dbg !355
  br label %47, !dbg !355

47:
  %48 = phi i32 [ 0, %46 ], [ 2, %37 ]
  call void @llvm.lifetime.end.p0(ptr %11) #2, !dbg !355
  switch i32 %48, label %89 [
    i32 0, label %49
  ]

49:
  call void @llvm.lifetime.start.p0(ptr %13) #2, !dbg !367
    #dbg_declare(ptr %13, !370, !DIExpression(), !371)
  store i32 0, ptr %13, align 4, !dbg !371
  br label %50, !dbg !367

50:
  %51 = load i32, ptr %13, align 4, !dbg !372
  %52 = load i32, ptr %9, align 4, !dbg !374
  %53 = icmp slt i32 %51, %52, !dbg !375
  br i1 %53, label %55, label %54, !dbg !376

54:
  br label %76, !dbg !376

55:
  call void @llvm.lifetime.start.p0(ptr %14) #2, !dbg !377
  %56 = getelementptr inbounds nuw %struct.big, ptr %6, i32 0, i32 0, !dbg !378
  %57 = load i32, ptr %13, align 4, !dbg !379
  %58 = sext i32 %57 to i64, !dbg !380
  %59 = getelementptr inbounds [4 x i32], ptr %56, i64 0, i64 %58, !dbg !380
  %60 = load i32, ptr %59, align 4, !dbg !380
  %61 = mul nsw i32 %60, 10, !dbg !381
  %62 = load i32, ptr %9, align 4, !dbg !382
  %63 = add nsw i32 %61, %62, !dbg !383
  call void @_ZN3gen12promise_type11yield_valueEi(ptr noundef nonnull align 4 dereferenceable(4) %10, i32 noundef %63) #2, !dbg !377
  %64 = call noundef i1 @_ZNKSt7__n486114suspend_always11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %14) #2, !dbg !377
  br i1 %64, label %69, label %65, !dbg !377

65:
  %66 = call token @llvm.coro.save(ptr null), !dbg !377
  call void @llvm.coro.await.suspend.void(ptr %14, ptr %25, ptr @_Z5count3bigi.__await_suspend_wrapper__yield) #2, !dbg !377
  %67 = call i8 @llvm.coro.suspend(token %66, i1 false), !dbg !377
  switch i8 %67, label %97 [
    i8 0, label %69
    i8 1, label %68
  ], !dbg !377

68:
  br label %70, !dbg !377

69:
  call void @_ZNKSt7__n486114suspend_always12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %14) #2, !dbg !377
  br label %70, !dbg !377

70:
  %71 = phi i32 [ 0, %69 ], [ 2, %68 ]
  call void @llvm.lifetime.end.p0(ptr %14) #2, !dbg !377
  switch i32 %71, label %76 [
    i32 0, label %72
  ]

72:
  br label %73, !dbg !377

73:
  %74 = load i32, ptr %13, align 4, !dbg !384
  %75 = add nsw i32 %74, 1, !dbg !384
  store i32 %75, ptr %13, align 4, !dbg !384
  br label %50, !dbg !385, !llvm.loop !386

76:
  %77 = phi i32 [ %71, %70 ], [ 4, %54 ]
  call void @llvm.lifetime.end.p0(ptr %13) #2, !dbg !385
  switch i32 %77, label %89 [
    i32 4, label %78
  ]

78:
  call void @_ZN3gen12promise_type11return_voidEv(ptr noundef nonnull align 4 dereferenceable(4) %10) #2, !dbg !355
  br label %79, !dbg !355

79:
  call void @llvm.lifetime.start.p0(ptr %16) #2, !dbg !355
  call void @_ZN3gen12promise_type13final_suspendEv(ptr noundef nonnull align 4 dereferenceable(4) %10) #2, !dbg !355
  %80 = call noundef i1 @_ZNKSt7__n486114suspend_always11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %16) #2, !dbg !355
  br i1 %80, label %85, label %81, !dbg !355

81:
  %82 = call token @llvm.coro.save(ptr null), !dbg !355
  call void @llvm.coro.await.suspend.void(ptr %16, ptr %25, ptr @_Z5count3bigi.__await_suspend_wrapper__final) #2, !dbg !355
  %83 = call i8 @llvm.coro.suspend(token %82, i1 true), !dbg !355
  switch i8 %83, label %97 [
    i8 0, label %85
    i8 1, label %84
  ], !dbg !355

84:
  br label %86, !dbg !355

85:
  call void @_ZNKSt7__n486114suspend_always12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %16) #2, !dbg !355
  br label %86, !dbg !355

86:
  %87 = phi i32 [ 0, %85 ], [ 2, %84 ]
  call void @llvm.lifetime.end.p0(ptr %16) #2, !dbg !355
  switch i32 %87, label %89 [
    i32 0, label %88
  ]

88:
  br label %89, !dbg !355

89:
  %90 = phi i32 [ %48, %47 ], [ %77, %76 ], [ %87, %86 ], [ 0, %88 ], !dbg !358
  call void @llvm.lifetime.end.p0(ptr %10) #2, !dbg !355
  call void @llvm.lifetime.end.p0(ptr %9) #2, !dbg !355
  call void @_ZN3bigD2Ev(ptr noundef nonnull align 4 dereferenceable(16) %6) #2, !dbg !355
  call void @llvm.lifetime.end.p0(ptr %6) #2, !dbg !355
  %91 = call ptr @llvm.coro.free(token %18, ptr %25), !dbg !355
  %92 = icmp ne ptr %91, null, !dbg !355
  br i1 %92, label %93, label %95, !dbg !355

93:
  %94 = call i64 @llvm.coro.size.i64(), !dbg !355
  call void @_ZdlPvm(ptr noundef %91, i64 noundef %94) #2, !dbg !355
  br label %95, !dbg !355

95:
  switch i32 %90, label %113 [
    i32 0, label %96
    i32 2, label %97
  ]

96:
  br label %97, !dbg !355

97:
  call void @llvm.coro.end(ptr null, i1 false, token none), !dbg !355
  %98 = getelementptr inbounds nuw %struct.gen, ptr %3, i32 0, i32 0, !dbg !355
  %99 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %98, i32 0, i32 0, !dbg !355
  %100 = load ptr, ptr %99, align 8, !dbg !355
  %101 = ptrtoint ptr %100 to i64, !dbg !355
  ret i64 %101, !dbg !355

102:
  call void @llvm.lifetime.end.p0(ptr %6) #2, !dbg !355
  %103 = call ptr @llvm.coro.free(token %18, ptr %25), !dbg !355
  %104 = icmp ne ptr %103, null, !dbg !355
  br i1 %104, label %105, label %107, !dbg !355

105:
  %106 = call i64 @llvm.coro.size.i64(), !dbg !355
  call void @_ZdlPvm(ptr noundef %103, i64 noundef %106) #2, !dbg !355
  br label %107, !dbg !355

107:
  br label %108, !dbg !355

108:
  %109 = load ptr, ptr %7, align 8, !dbg !355
  %110 = load i32, ptr %8, align 4, !dbg !355
  %111 = insertvalue { ptr, i32 } poison, ptr %109, 0, !dbg !355
  %112 = insertvalue { ptr, i32 } %111, i32 %110, 1, !dbg !355
  resume { ptr, i32 } %112, !dbg !355

113:
  unreachable
}

declare token @llvm.coro.id(i32, ptr readnone, ptr readonly captures(none), ptr) #1

declare i1 @llvm.coro.alloc(token) #2

declare noundef nonnull ptr @_Znwm(i64 noundef) #3

declare i64 @llvm.coro.size.i64() #4

declare ptr @llvm.coro.begin(token, ptr writeonly) #2

declare void @llvm.lifetime.start.p0(ptr captures(none)) #5

define linkonce_odr dso_local void @_ZN3bigC2ERKS_(ptr noundef nonnull align 4 dereferenceable(16) %0, ptr noundef nonnull align 4 dereferenceable(16) %1) unnamed_addr #6 comdat !dbg !389 {
  %3 = alloca ptr, align 8
  %4 = alloca ptr, align 8
  %5 = alloca i32, align 4
  store ptr %0, ptr %3, align 8
    #dbg_declare(ptr %3, !390, !DIExpression(), !392)
  store ptr %1, ptr %4, align 8
    #dbg_declare(ptr %4, !393, !DIExpression(), !394)
  %6 = load ptr, ptr %3, align 8
    #dbg_declare(ptr %5, !395, !DIExpression(), !398)
  store i32 0, ptr %5, align 4, !dbg !398
  br label %7, !dbg !399

7:
  %8 = load i32, ptr %5, align 4, !dbg !400
  %9 = icmp slt i32 %8, 4, !dbg !402
  br i1 %9, label %10, label %24, !dbg !403

10:
  %11 = load ptr, ptr %4, align 8, !dbg !404, !nonnull !349, !align !405
  %12 = getelementptr inbounds nuw %struct.big, ptr %11, i32 0, i32 0, !dbg !406
  %13 = load i32, ptr %5, align 4, !dbg !407
  %14 = sext i32 %13 to i64, !dbg !404
  %15 = getelementptr inbounds [4 x i32], ptr %12, i64 0, i64 %14, !dbg !404
  %16 = load i32, ptr %15, align 4, !dbg !404
  %17 = getelementptr inbounds nuw %struct.big, ptr %6, i32 0, i32 0, !dbg !408
  %18 = load i32, ptr %5, align 4, !dbg !409
  %19 = sext i32 %18 to i64, !dbg !408
  %20 = getelementptr inbounds [4 x i32], ptr %17, i64 0, i64 %19, !dbg !408
  store i32 %16, ptr %20, align 4, !dbg !410
  br label %21, !dbg !408

21:
  %22 = load i32, ptr %5, align 4, !dbg !411
  %23 = add nsw i32 %22, 1, !dbg !411
  store i32 %23, ptr %5, align 4, !dbg !411
  br label %7, !dbg !412, !llvm.loop !413

24:
  ret void, !dbg !415
}

declare i32 @__gxx_personality_v0(...)

define linkonce_odr dso_local void @_ZN3gen12promise_typeC2Ev(ptr noundef nonnull align 4 dereferenceable(4) %0) unnamed_addr #6 comdat !dbg !416 {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
    #dbg_declare(ptr %2, !418, !DIExpression(), !419)
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds nuw %"struct.gen::promise_type", ptr %3, i32 0, i32 0, !dbg !420
  store i32 0, ptr %4, align 4, !dbg !420
  ret void, !dbg !421
}

define linkonce_odr dso_local i64 @_ZN3gen12promise_type17get_return_objectEv(ptr noundef nonnull align 4 dereferenceable(4) %0) #7 comdat !dbg !422 {
  %2 = alloca %struct.gen, align 8
  %3 = alloca ptr, align 8
  store ptr %0, ptr %3, align 8
    #dbg_declare(ptr %3, !423, !DIExpression(), !424)
  %4 = load ptr, ptr %3, align 8
  %5 = getelementptr inbounds nuw %struct.gen, ptr %2, i32 0, i32 0, !dbg !425
  %6 = call i64 @_ZNSt7__n486116coroutine_handleIN3gen12promise_typeEE12from_promiseERS2_(ptr noundef nonnull align 4 dereferenceable(4) %4), !dbg !426
  %7 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %5, i32 0, i32 0, !dbg !426
  %8 = inttoptr i64 %6 to ptr, !dbg !426
  store ptr %8, ptr %7, align 8, !dbg !426
  %9 = getelementptr inbounds nuw %struct.gen, ptr %2, i32 0, i32 0, !dbg !427
  %10 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %9, i32 0, i32 0, !dbg !427
  %11 = load ptr, ptr %10, align 8, !dbg !427
  %12 = ptrtoint ptr %11 to i64, !dbg !427
  ret i64 %12, !dbg !427
}

define linkonce_odr dso_local void @_ZN3gen12promise_type15initial_suspendEv(ptr noundef nonnull align 4 dereferenceable(4) %0) #6 comdat !dbg !428 {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
    #dbg_declare(ptr %2, !429, !DIExpression(), !430)
  %3 = load ptr, ptr %2, align 8
  ret void, !dbg !431
}

define linkonce_odr dso_local noundef i1 @_ZNKSt7__n486114suspend_always11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %0) #6 comdat !dbg !432 {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
    #dbg_declare(ptr %2, !433, !DIExpression(), !435)
  %3 = load ptr, ptr %2, align 8
  ret i1 false, !dbg !436
}

declare token @llvm.coro.save(ptr) #8

define internal void @_Z5count3bigi.__await_suspend_wrapper__init(ptr noundef nonnull %0, ptr noundef %1) #9 !dbg !437 {
  %3 = alloca ptr, align 8
  %4 = alloca ptr, align 8
  %5 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %6 = alloca %"struct.std::__n4861::coroutine_handle", align 8
  store ptr %0, ptr %3, align 8
  store ptr %1, ptr %4, align 8
  %7 = load ptr, ptr %3, align 8
  %8 = load ptr, ptr %4, align 8
  %9 = call i64 @_ZNSt7__n486116coroutine_handleIN3gen12promise_typeEE12from_addressEPv(ptr noundef %8) #2, !dbg !439
  %10 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %6, i32 0, i32 0, !dbg !439
  %11 = inttoptr i64 %9 to ptr, !dbg !439
  store ptr %11, ptr %10, align 8, !dbg !439
  %12 = call i64 @_ZNKSt7__n486116coroutine_handleIN3gen12promise_typeEEcvNS0_IvEEEv(ptr noundef nonnull align 8 dereferenceable(8) %6) #2, !dbg !439
  %13 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %5, i32 0, i32 0, !dbg !439
  %14 = inttoptr i64 %12 to ptr, !dbg !439
  store ptr %14, ptr %13, align 8, !dbg !439
  %15 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %5, i32 0, i32 0, !dbg !439
  %16 = load ptr, ptr %15, align 8, !dbg !439
  call void @_ZNKSt7__n486114suspend_always13await_suspendENS_16coroutine_handleIvEE(ptr noundef nonnull align 1 dereferenceable(1) %7, ptr %16) #2, !dbg !439
  ret void, !dbg !439
}

define linkonce_odr dso_local void @_ZNKSt7__n486114suspend_always13await_suspendENS_16coroutine_handleIvEE(ptr noundef nonnull align 1 dereferenceable(1) %0, ptr %1) #6 comdat !dbg !440 {
  %3 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %4 = alloca ptr, align 8
  %5 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %3, i32 0, i32 0
  store ptr %1, ptr %5, align 8
  store ptr %0, ptr %4, align 8
    #dbg_declare(ptr %4, !441, !DIExpression(), !442)
    #dbg_declare(ptr %3, !443, !DIExpression(), !444)
  %6 = load ptr, ptr %4, align 8
  ret void, !dbg !445
}

define linkonce_odr dso_local i64 @_ZNSt7__n486116coroutine_handleIN3gen12promise_typeEE12from_addressEPv(ptr noundef %0) #6 comdat !dbg !446 {
  %2 = alloca %"struct.std::__n4861::coroutine_handle", align 8
  %3 = alloca ptr, align 8
  store ptr %0, ptr %3, align 8
    #dbg_declare(ptr %3, !447, !DIExpression(), !448)
    #dbg_declare(ptr %2, !449, !DIExpression(), !450)
  call void @_ZNSt7__n486116coroutine_handleIN3gen12promise_typeEEC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %2) #2, !dbg !450
  %4 = load ptr, ptr %3, align 8, !dbg !451
  %5 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %2, i32 0, i32 0, !dbg !452
  store ptr %4, ptr %5, align 8, !dbg !453
  %6 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %2, i32 0, i32 0, !dbg !454
  %7 = load ptr, ptr %6, align 8, !dbg !454
  %8 = ptrtoint ptr %7 to i64, !dbg !454
  ret i64 %8, !dbg !454
}

define linkonce_odr dso_local i64 @_ZNKSt7__n486116coroutine_handleIN3gen12promise_typeEEcvNS0_IvEEEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #6 comdat !dbg !455 {
  %2 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %3 = alloca ptr, align 8
  store ptr %0, ptr %3, align 8
    #dbg_declare(ptr %3, !456, !DIExpression(), !458)
  %4 = load ptr, ptr %3, align 8
  %5 = call noundef ptr @_ZNKSt7__n486116coroutine_handleIN3gen12promise_typeEE7addressEv(ptr noundef nonnull align 8 dereferenceable(8) %4) #2, !dbg !459
  %6 = call i64 @_ZNSt7__n486116coroutine_handleIvE12from_addressEPv(ptr noundef %5) #2, !dbg !460
  %7 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %2, i32 0, i32 0, !dbg !460
  %8 = inttoptr i64 %6 to ptr, !dbg !460
  store ptr %8, ptr %7, align 8, !dbg !460
  %9 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %2, i32 0, i32 0, !dbg !461
  %10 = load ptr, ptr %9, align 8, !dbg !461
  %11 = ptrtoint ptr %10 to i64, !dbg !461
  ret i64 %11, !dbg !461
}

declare void @llvm.coro.await.suspend.void(ptr, ptr, ptr)

declare i8 @llvm.coro.suspend(token, i1) #2

define linkonce_odr dso_local void @_ZNKSt7__n486114suspend_always12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %0) #6 comdat !dbg !462 {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
    #dbg_declare(ptr %2, !463, !DIExpression(), !464)
  %3 = load ptr, ptr %2, align 8
  ret void, !dbg !465
}

declare void @llvm.lifetime.end.p0(ptr captures(none)) #5

define linkonce_odr dso_local void @_ZN3gen12promise_type11yield_valueEi(ptr noundef nonnull align 4 dereferenceable(4) %0, i32 noundef %1) #6 comdat !dbg !466 {
  %3 = alloca ptr, align 8
  %4 = alloca i32, align 4
  store ptr %0, ptr %3, align 8
    #dbg_declare(ptr %3, !467, !DIExpression(), !468)
  store i32 %1, ptr %4, align 4
    #dbg_declare(ptr %4, !469, !DIExpression(), !470)
  %5 = load ptr, ptr %3, align 8
  %6 = load i32, ptr %4, align 4, !dbg !471
  %7 = getelementptr inbounds nuw %"struct.gen::promise_type", ptr %5, i32 0, i32 0, !dbg !472
  store i32 %6, ptr %7, align 4, !dbg !473
  ret void, !dbg !474
}

define internal void @_Z5count3bigi.__await_suspend_wrapper__yield(ptr noundef nonnull %0, ptr noundef %1) #9 !dbg !475 {
  %3 = alloca ptr, align 8
  %4 = alloca ptr, align 8
  %5 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %6 = alloca %"struct.std::__n4861::coroutine_handle", align 8
  store ptr %0, ptr %3, align 8
  store ptr %1, ptr %4, align 8
  %7 = load ptr, ptr %3, align 8
  %8 = load ptr, ptr %4, align 8
  %9 = call i64 @_ZNSt7__n486116coroutine_handleIN3gen12promise_typeEE12from_addressEPv(ptr noundef %8) #2, !dbg !476
  %10 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %6, i32 0, i32 0, !dbg !476
  %11 = inttoptr i64 %9 to ptr, !dbg !476
  store ptr %11, ptr %10, align 8, !dbg !476
  %12 = call i64 @_ZNKSt7__n486116coroutine_handleIN3gen12promise_typeEEcvNS0_IvEEEv(ptr noundef nonnull align 8 dereferenceable(8) %6) #2, !dbg !476
  %13 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %5, i32 0, i32 0, !dbg !476
  %14 = inttoptr i64 %12 to ptr, !dbg !476
  store ptr %14, ptr %13, align 8, !dbg !476
  %15 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %5, i32 0, i32 0, !dbg !476
  %16 = load ptr, ptr %15, align 8, !dbg !476
  call void @_ZNKSt7__n486114suspend_always13await_suspendENS_16coroutine_handleIvEE(ptr noundef nonnull align 1 dereferenceable(1) %7, ptr %16) #2, !dbg !476
  ret void, !dbg !476
}

define linkonce_odr dso_local void @_ZN3gen12promise_type11return_voidEv(ptr noundef nonnull align 4 dereferenceable(4) %0) #6 comdat !dbg !477 {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
    #dbg_declare(ptr %2, !478, !DIExpression(), !479)
  %3 = load ptr, ptr %2, align 8
  ret void, !dbg !480
}

define linkonce_odr dso_local void @_ZN3gen12promise_type13final_suspendEv(ptr noundef nonnull align 4 dereferenceable(4) %0) #6 comdat !dbg !481 {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
    #dbg_declare(ptr %2, !482, !DIExpression(), !483)
  %3 = load ptr, ptr %2, align 8
  ret void, !dbg !484
}

define internal void @_Z5count3bigi.__await_suspend_wrapper__final(ptr noundef nonnull %0, ptr noundef %1) #9 !dbg !485 {
  %3 = alloca ptr, align 8
  %4 = alloca ptr, align 8
  %5 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %6 = alloca %"struct.std::__n4861::coroutine_handle", align 8
  store ptr %0, ptr %3, align 8
  store ptr %1, ptr %4, align 8
  %7 = load ptr, ptr %3, align 8
  %8 = load ptr, ptr %4, align 8
  %9 = call i64 @_ZNSt7__n486116coroutine_handleIN3gen12promise_typeEE12from_addressEPv(ptr noundef %8) #2, !dbg !486
  %10 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %6, i32 0, i32 0, !dbg !486
  %11 = inttoptr i64 %9 to ptr, !dbg !486
  store ptr %11, ptr %10, align 8, !dbg !486
  %12 = call i64 @_ZNKSt7__n486116coroutine_handleIN3gen12promise_typeEEcvNS0_IvEEEv(ptr noundef nonnull align 8 dereferenceable(8) %6) #2, !dbg !486
  %13 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %5, i32 0, i32 0, !dbg !486
  %14 = inttoptr i64 %12 to ptr, !dbg !486
  store ptr %14, ptr %13, align 8, !dbg !486
  %15 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %5, i32 0, i32 0, !dbg !486
  %16 = load ptr, ptr %15, align 8, !dbg !486
  call void @_ZNKSt7__n486114suspend_always13await_suspendENS_16coroutine_handleIvEE(ptr noundef nonnull align 1 dereferenceable(1) %7, ptr %16) #2, !dbg !486
  ret void, !dbg !486
}

define linkonce_odr dso_local void @_ZN3bigD2Ev(ptr noundef nonnull align 4 dereferenceable(16) %0) unnamed_addr #6 comdat personality ptr @__gxx_personality_v0 !dbg !487 {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
    #dbg_declare(ptr %2, !488, !DIExpression(), !489)
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds nuw %struct.big, ptr %3, i32 0, i32 0, !dbg !490
  %5 = getelementptr inbounds [4 x i32], ptr %4, i64 0, i64 0, !dbg !490
  %6 = load i32, ptr %5, align 4, !dbg !490
  %7 = invoke i32 (ptr, ...) @printf(ptr noundef @.str.1, i32 noundef %6)
          to label %8 unwind label %9, !dbg !492

8:
  ret void, !dbg !493

9:
  %10 = landingpad { ptr, i32 }
          catch ptr null, !dbg !492
  %11 = extractvalue { ptr, i32 } %10, 0, !dbg !492
  call void @__clang_call_terminate(ptr %11) #16, !dbg !492
  unreachable, !dbg !492
}

declare void @_ZdlPvm(ptr noundef, i64 noundef) #10

declare ptr @llvm.coro.free(token, ptr readonly captures(none)) #11

declare void @llvm.coro.end(ptr, i1, token) #2

define dso_local noundef i32 @main() #12 personality ptr @__gxx_personality_v0 !dbg !494 {
  %1 = alloca i32, align 4
  %2 = alloca %struct.gen, align 8
  %3 = alloca %struct.big, align 4
  %4 = alloca ptr, align 8
  %5 = alloca i32, align 4
  store i32 0, ptr %1, align 4
    #dbg_declare(ptr %2, !495, !DIExpression(), !496)
  call void @_ZN3bigC2Ei(ptr noundef nonnull align 4 dereferenceable(16) %3, i32 noundef 3), !dbg !497
  %6 = invoke i64 @_Z5count3bigi(ptr noundef %3, i32 noundef 3)
          to label %7 unwind label %24, !dbg !498

7:
  %8 = getelementptr inbounds nuw %struct.gen, ptr %2, i32 0, i32 0, !dbg !498
  %9 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %8, i32 0, i32 0, !dbg !498
  %10 = inttoptr i64 %6 to ptr, !dbg !498
  store ptr %10, ptr %9, align 8, !dbg !498
  call void @_ZN3bigD2Ev(ptr noundef nonnull align 4 dereferenceable(16) %3) #2, !dbg !498
  %11 = getelementptr inbounds nuw %struct.gen, ptr %2, i32 0, i32 0, !dbg !499
  call void @_ZNKSt7__n486116coroutine_handleIN3gen12promise_typeEE6resumeEv(ptr noundef nonnull align 8 dereferenceable(8) %11), !dbg !501
  br label %12, !dbg !502

12:
  %13 = getelementptr inbounds nuw %struct.gen, ptr %2, i32 0, i32 0, !dbg !503
  %14 = call noundef i1 @_ZNKSt7__n486116coroutine_handleIN3gen12promise_typeEE4doneEv(ptr noundef nonnull align 8 dereferenceable(8) %13) #2, !dbg !505
  %15 = xor i1 %14, true, !dbg !506
  br i1 %15, label %16, label %28, !dbg !507

16:
  %17 = getelementptr inbounds nuw %struct.gen, ptr %2, i32 0, i32 0, !dbg !508
  %18 = call noundef nonnull align 4 dereferenceable(4) ptr @_ZNKSt7__n486116coroutine_handleIN3gen12promise_typeEE7promiseEv(ptr noundef nonnull align 8 dereferenceable(8) %17), !dbg !509
  %19 = getelementptr inbounds nuw %"struct.gen::promise_type", ptr %18, i32 0, i32 0, !dbg !510
  %20 = load i32, ptr %19, align 4, !dbg !510
  %21 = call i32 (ptr, ...) @printf(ptr noundef @.str, i32 noundef %20), !dbg !511
  br label %22, !dbg !511

22:
  %23 = getelementptr inbounds nuw %struct.gen, ptr %2, i32 0, i32 0, !dbg !512
  call void @_ZNKSt7__n486116coroutine_handleIN3gen12promise_typeEE6resumeEv(ptr noundef nonnull align 8 dereferenceable(8) %23), !dbg !513
  br label %12, !dbg !514, !llvm.loop !515

24:
  %25 = landingpad { ptr, i32 }
          cleanup, !dbg !517
  %26 = extractvalue { ptr, i32 } %25, 0, !dbg !517
  store ptr %26, ptr %4, align 8, !dbg !517
  %27 = extractvalue { ptr, i32 } %25, 1, !dbg !517
  store i32 %27, ptr %5, align 4, !dbg !517
  call void @_ZN3bigD2Ev(ptr noundef nonnull align 4 dereferenceable(16) %3) #2, !dbg !498
  br label %30, !dbg !498

28:
  %29 = getelementptr inbounds nuw %struct.gen, ptr %2, i32 0, i32 0, !dbg !518
  call void @_ZNKSt7__n486116coroutine_handleIN3gen12promise_typeEE7destroyEv(ptr noundef nonnull align 8 dereferenceable(8) %29), !dbg !519
  ret i32 0, !dbg !520

30:
  %31 = load ptr, ptr %4, align 8, !dbg !498
  %32 = load i32, ptr %5, align 4, !dbg !498
  %33 = insertvalue { ptr, i32 } poison, ptr %31, 0, !dbg !498
  %34 = insertvalue { ptr, i32 } %33, i32 %32, 1, !dbg !498
  resume { ptr, i32 } %34, !dbg !498
}

define linkonce_odr dso_local void @_ZN3bigC2Ei(ptr noundef nonnull align 4 dereferenceable(16) %0, i32 noundef %1) unnamed_addr #6 comdat !dbg !521 {
  %3 = alloca ptr, align 8
  %4 = alloca i32, align 4
  %5 = alloca i32, align 4
  store ptr %0, ptr %3, align 8
    #dbg_declare(ptr %3, !522, !DIExpression(), !523)
  store i32 %1, ptr %4, align 4
    #dbg_declare(ptr %4, !524, !DIExpression(), !525)
  %6 = load ptr, ptr %3, align 8
    #dbg_declare(ptr %5, !526, !DIExpression(), !529)
  store i32 0, ptr %5, align 4, !dbg !529
  br label %7, !dbg !530

7:
  %8 = load i32, ptr %5, align 4, !dbg !531
  %9 = icmp slt i32 %8, 4, !dbg !533
  br i1 %9, label %10, label %21, !dbg !534

10:
  %11 = load i32, ptr %4, align 4, !dbg !535
  %12 = load i32, ptr %5, align 4, !dbg !536
  %13 = add nsw i32 %11, %12, !dbg !537
  %14 = getelementptr inbounds nuw %struct.big, ptr %6, i32 0, i32 0, !dbg !538
  %15 = load i32, ptr %5, align 4, !dbg !539
  %16 = sext i32 %15 to i64, !dbg !538
  %17 = getelementptr inbounds [4 x i32], ptr %14, i64 0, i64 %16, !dbg !538
  store i32 %13, ptr %17, align 4, !dbg !540
  br label %18, !dbg !538

18:
  %19 = load i32, ptr %5, align 4, !dbg !541
  %20 = add nsw i32 %19, 1, !dbg !541
  store i32 %20, ptr %5, align 4, !dbg !541
  br label %7, !dbg !542, !llvm.loop !543

21:
  ret void, !dbg !545
}

define linkonce_odr dso_local void @_ZNKSt7__n486116coroutine_handleIN3gen12promise_typeEE6resumeEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #6 comdat !dbg !546 {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
    #dbg_declare(ptr %2, !547, !DIExpression(), !548)
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %3, i32 0, i32 0, !dbg !549
  %5 = load ptr, ptr %4, align 8, !dbg !549
  call void @llvm.coro.resume(ptr %5), !dbg !550
  ret void, !dbg !551
}

define linkonce_odr dso_local noundef i1 @_ZNKSt7__n486116coroutine_handleIN3gen12promise_typeEE4doneEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #6 comdat !dbg !552 {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
    #dbg_declare(ptr %2, !553, !DIExpression(), !554)
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %3, i32 0, i32 0, !dbg !555
  %5 = load ptr, ptr %4, align 8, !dbg !555
  %6 = call i1 @llvm.coro.done(ptr %5), !dbg !556
  ret i1 %6, !dbg !557
}

declare i32 @printf(ptr noundef, ...) #13

define linkonce_odr dso_local noundef nonnull align 4 dereferenceable(4) ptr @_ZNKSt7__n486116coroutine_handleIN3gen12promise_typeEE7promiseEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #6 comdat !dbg !558 {
  %2 = alloca ptr, align 8
  %3 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
    #dbg_declare(ptr %2, !559, !DIExpression(), !560)
  %4 = load ptr, ptr %2, align 8
    #dbg_declare(ptr %3, !561, !DIExpression(), !562)
  %5 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %4, i32 0, i32 0, !dbg !563
  %6 = load ptr, ptr %5, align 8, !dbg !563
  %7 = call ptr @llvm.coro.promise(ptr %6, i32 4, i1 false), !dbg !564
  store ptr %7, ptr %3, align 8, !dbg !562
  %8 = load ptr, ptr %3, align 8, !dbg !565
  ret ptr %8, !dbg !566
}

define linkonce_odr dso_local void @_ZNKSt7__n486116coroutine_handleIN3gen12promise_typeEE7destroyEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #6 comdat !dbg !567 {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
    #dbg_declare(ptr %2, !568, !DIExpression(), !569)
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %3, i32 0, i32 0, !dbg !570
  %5 = load ptr, ptr %4, align 8, !dbg !570
  call void @llvm.coro.destroy(ptr %5), !dbg !571
  ret void, !dbg !572
}

define linkonce_odr dso_local i64 @_ZNSt7__n486116coroutine_handleIN3gen12promise_typeEE12from_promiseERS2_(ptr noundef nonnull align 4 dereferenceable(4) %0) #6 comdat !dbg !573 {
  %2 = alloca %"struct.std::__n4861::coroutine_handle", align 8
  %3 = alloca ptr, align 8
  store ptr %0, ptr %3, align 8
    #dbg_declare(ptr %3, !574, !DIExpression(), !575)
    #dbg_declare(ptr %2, !576, !DIExpression(), !577)
  call void @_ZNSt7__n486116coroutine_handleIN3gen12promise_typeEEC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %2) #2, !dbg !577
  %4 = load ptr, ptr %3, align 8, !dbg !578, !nonnull !349, !align !405
  %5 = call ptr @llvm.coro.promise(ptr %4, i32 4, i1 true), !dbg !579
  %6 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %2, i32 0, i32 0, !dbg !580
  store ptr %5, ptr %6, align 8, !dbg !581
  %7 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %2, i32 0, i32 0, !dbg !582
  %8 = load ptr, ptr %7, align 8, !dbg !582
  %9 = ptrtoint ptr %8 to i64, !dbg !582
  ret i64 %9, !dbg !582
}

define linkonce_odr dso_local void @_ZNSt7__n486116coroutine_handleIN3gen12promise_typeEEC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %0) unnamed_addr #6 comdat !dbg !583 {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
    #dbg_declare(ptr %2, !584, !DIExpression(), !586)
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %3, i32 0, i32 0, !dbg !587
  store ptr null, ptr %4, align 8, !dbg !587
  ret void, !dbg !588
}

declare ptr @llvm.coro.promise(ptr captures(none), i32, i1) #4

define linkonce_odr dso_local i64 @_ZNSt7__n486116coroutine_handleIvE12from_addressEPv(ptr noundef %0) #6 comdat !dbg !589 {
  %2 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %3 = alloca ptr, align 8
  store ptr %0, ptr %3, align 8
    #dbg_declare(ptr %3, !590, !DIExpression(), !591)
    #dbg_declare(ptr %2, !592, !DIExpression(), !593)
  call void @_ZNSt7__n486116coroutine_handleIvEC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %2) #2, !dbg !593
  %4 = load ptr, ptr %3, align 8, !dbg !594
  %5 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %2, i32 0, i32 0, !dbg !595
  store ptr %4, ptr %5, align 8, !dbg !596
  %6 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %2, i32 0, i32 0, !dbg !597
  %7 = load ptr, ptr %6, align 8, !dbg !597
  %8 = ptrtoint ptr %7 to i64, !dbg !597
  ret i64 %8, !dbg !597
}

define linkonce_odr dso_local noundef ptr @_ZNKSt7__n486116coroutine_handleIN3gen12promise_typeEE7addressEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #6 comdat !dbg !598 {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
    #dbg_declare(ptr %2, !599, !DIExpression(), !600)
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %3, i32 0, i32 0, !dbg !601
  %5 = load ptr, ptr %4, align 8, !dbg !601
  ret ptr %5, !dbg !602
}

define linkonce_odr dso_local void @_ZNSt7__n486116coroutine_handleIvEC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %0) unnamed_addr #6 comdat !dbg !603 {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
    #dbg_declare(ptr %2, !604, !DIExpression(), !606)
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %3, i32 0, i32 0, !dbg !607
  store ptr null, ptr %4, align 8, !dbg !607
  ret void, !dbg !608
}

define linkonce_odr hidden void @__clang_call_terminate(ptr noundef %0) #14 comdat {
  %2 = call ptr @__cxa_begin_catch(ptr %0) #2
  call void @_ZSt9terminatev() #16
  unreachable
}

declare ptr @__cxa_begin_catch(ptr)

declare void @_ZSt9terminatev()

declare void @llvm.coro.resume(ptr)

declare i1 @llvm.coro.done(ptr readonly captures(none)) #11

declare void @llvm.coro.destroy(ptr)

attributes #0 = { mustprogress noinline optnone presplitcoroutine uwtable "frame-pointer"="non-leaf-no-reserve" "no-trapping-math"="true" "stack-protector-buffer-size"="8" }
attributes #1 = { nocallback nofree nosync nounwind willreturn memory(argmem: read) }
attributes #2 = { nounwind }
attributes #3 = { nobuiltin allocsize(0) "frame-pointer"="non-leaf-no-reserve" "no-trapping-math"="true" "stack-protector-buffer-size"="8" }
attributes #4 = { nounwind memory(none) }
attributes #5 = { nocallback nofree nosync nounwind willreturn memory(argmem: readwrite) }
attributes #6 = { mustprogress noinline nounwind optnone uwtable "frame-pointer"="non-leaf-no-reserve" "no-trapping-math"="true" "stack-protector-buffer-size"="8" }
attributes #7 = { mustprogress noinline optnone uwtable "frame-pointer"="non-leaf-no-reserve" "no-trapping-math"="true" "stack-protector-buffer-size"="8" }
attributes #8 = { nomerge nounwind }
attributes #9 = { alwaysinline mustprogress "sample-profile-suffix-elision-policy"="selected" }
attributes #10 = { nobuiltin nounwind "frame-pointer"="non-leaf-no-reserve" "no-trapping-math"="true" "stack-protector-buffer-size"="8" }
attributes #11 = { nounwind memory(argmem: read) }
attributes #12 = { mustprogress noinline norecurse optnone uwtable "frame-pointer"="non-leaf-no-reserve" "no-trapping-math"="true" "stack-protector-buffer-size"="8" }
attributes #13 = { "frame-pointer"="non-leaf-no-reserve" "no-trapping-math"="true" "stack-protector-buffer-size"="8" }
attributes #14 = { noinline noreturn nounwind uwtable "frame-pointer"="non-leaf-no-reserve" "no-trapping-math"="true" "stack-protector-buffer-size"="8" }
attributes #15 = { allocsize(0) }
attributes #16 = { noreturn nounwind }

!llvm.dbg.cu = !{!0}
!llvm.module.flags = !{!338, !339, !340, !341, !342, !343, !344}
!llvm.ident = !{!345}

!0 = distinct !DICompileUnit(language: DW_LANG_C_plus_plus_14, file: !1, producer: "kiln version 1.2.0", isOptimized: false, runtimeVersion: 0, emissionKind: FullDebug, splitDebugInlining: false, nameTableKind: None, retainedTypes: !2, imports: !143)
!1 = !DIFile(filename: "byaddr.cpp", directory: ".", checksumkind: CSK_MD5, checksum: "ea9597ebd6676725685f9efbd23745ff")
!2 = !{!3, !22, !140, !142, !25, !67, !46}
!3 = distinct !DICompositeType(tag: DW_TAG_structure_type, name: "big", file: !1, line: 5, size: 128, flags: DIFlagTypePassByReference | DIFlagNonTrivial, elements: !4, identifier: "_ZTS3big")
!4 = !{!5, !10, !14, !19}
!5 = !DIDerivedType(tag: DW_TAG_member, baseType: !6, name: "a", scope: !3, file: !1, line: 6, size: 128, offset: 0)
!6 = !DICompositeType(tag: DW_TAG_array_type, baseType: !7, size: 128, elements: !8)
!7 = !DIBasicType(name: "int", size: 32, encoding: DW_ATE_signed)
!8 = !{!9}
!9 = !DISubrange(count: 4)
!10 = !DISubprogram(name: "big", scope: !3, file: !1, line: 7, type: !11, scopeLine: 7, flags: DIFlagExplicit | DIFlagPrototyped, spFlags: 0)
!11 = !DISubroutineType(types: !12)
!12 = !{null, !13, !7}
!13 = !DIDerivedType(tag: DW_TAG_pointer_type, baseType: !3, size: 64, flags: DIFlagArtificial | DIFlagObjectPointer)
!14 = !DISubprogram(name: "big", scope: !3, file: !1, line: 8, type: !15, scopeLine: 8, flags: DIFlagPrototyped, spFlags: 0)
!15 = !DISubroutineType(types: !16)
!16 = !{null, !13, !17}
!17 = !DIDerivedType(tag: DW_TAG_reference_type, baseType: !18, size: 64)
!18 = !DIDerivedType(tag: DW_TAG_const_type, baseType: !3)
!19 = !DISubprogram(name: "~big", scope: !3, file: !1, line: 9, type: !20, scopeLine: 9, flags: DIFlagPrototyped, spFlags: 0)
!20 = !DISubroutineType(types: !21)
!21 = !{null, !13}
!22 = distinct !DICompositeType(tag: DW_TAG_structure_type, name: "gen", file: !1, line: 11, size: 64, flags: DIFlagTypePassByValue | DIFlagNonTrivial, elements: !23, identifier: "_ZTS3gen")
!23 = !{!24}
!24 = !DIDerivedType(tag: DW_TAG_member, baseType: !25, name: "h", scope: !22, file: !1, line: 21, size: 64, offset: 0)
!25 = distinct !DICompositeType(tag: DW_TAG_structure_type, name: "coroutine_handle<gen::promise_type>", scope: !26, file: !28, line: 193, size: 64, flags: DIFlagTypePassByValue | DIFlagNonTrivial, elements: !29, templateParams: !138, identifier: "_ZTSNSt7__n486116coroutine_handleIN3gen12promise_typeEEE")
!26 = !DINamespace(scope: !27, name: "__n4861", exportSymbols: true)
!27 = !DINamespace(scope: null, name: "std")
!28 = !DIFile(filename: "/usr/lib/gcc/aarch64-linux-gnu/14/../../../../include/c++/14/coroutine", directory: "", checksumkind: CSK_MD5, checksum: "249c5b284bdb3604f610bc79d1337856")
!29 = !{!30, !32, !36, !42, !111, !115, !120, !123, !126, !129, !130, !133, !134, !135}
!30 = !DIDerivedType(tag: DW_TAG_member, baseType: !31, name: "_M_fr_ptr", scope: !25, file: !28, line: 255, size: 64, offset: 0, flags: DIFlagPrivate)
!31 = !DIDerivedType(tag: DW_TAG_pointer_type, baseType: null, size: 64)
!32 = !DISubprogram(name: "coroutine_handle", scope: !25, file: !28, line: 197, type: !33, scopeLine: 197, flags: DIFlagPrototyped, spFlags: 0)
!33 = !DISubroutineType(types: !34)
!34 = !{null, !35}
!35 = !DIDerivedType(tag: DW_TAG_pointer_type, baseType: !25, size: 64, flags: DIFlagArtificial | DIFlagObjectPointer)
!36 = !DISubprogram(name: "coroutine_handle", scope: !25, file: !28, line: 199, type: !37, scopeLine: 199, flags: DIFlagPrototyped, spFlags: 0)
!37 = !DISubroutineType(types: !38)
!38 = !{null, !35, !39}
!39 = !DIDerivedType(tag: DW_TAG_typedef, baseType: !40, name: "nullptr_t", scope: !27, file: !41, line: 314)
!40 = !DIBasicType(tag: DW_TAG_unspecified_type, name: "decltype(nullptr)")
!41 = !DIFile(filename: "/usr/lib/gcc/aarch64-linux-gnu/14/../../../../include/aarch64-linux-gnu/c++/14/bits/c++config.h", directory: "", checksumkind: CSK_MD5, checksum: "292b7b71ed3883091d23df4f61ea2141")
!42 = !DISubprogram(name: "from_promise", linkageName: "_ZNSt7__n486116coroutine_handleIN3gen12promise_typeEE12from_promiseERS2_", scope: !25, file: !28, line: 202, type: !43, scopeLine: 202, flags: DIFlagStaticMember | DIFlagPrototyped, spFlags: 0)
!43 = !DISubroutineType(types: !44)
!44 = !{!25, !45}
!45 = !DIDerivedType(tag: DW_TAG_reference_type, baseType: !46, size: 64)
!46 = distinct !DICompositeType(tag: DW_TAG_structure_type, name: "promise_type", scope: !22, file: !1, line: 12, size: 32, flags: DIFlagTypePassByValue | DIFlagNonTrivial, elements: !47, identifier: "_ZTSN3gen12promise_typeE")
!47 = !{!48, !49, !53, !103, !104, !107, !110}
!48 = !DIDerivedType(tag: DW_TAG_member, baseType: !7, name: "v", scope: !46, file: !1, line: 13, size: 32, offset: 0)
!49 = !DISubprogram(name: "get_return_object", linkageName: "_ZN3gen12promise_type17get_return_objectEv", scope: !46, file: !1, line: 14, type: !50, scopeLine: 14, flags: DIFlagPrototyped, spFlags: 0)
!50 = !DISubroutineType(types: !51)
!51 = !{!22, !52}
!52 = !DIDerivedType(tag: DW_TAG_pointer_type, baseType: !46, size: 64, flags: DIFlagArtificial | DIFlagObjectPointer)
!53 = !DISubprogram(name: "initial_suspend", linkageName: "_ZN3gen12promise_type15initial_suspendEv", scope: !46, file: !1, line: 15, type: !54, scopeLine: 15, flags: DIFlagPrototyped, spFlags: 0)
!54 = !DISubroutineType(types: !55)
!55 = !{!56, !52}
!56 = distinct !DICompositeType(tag: DW_TAG_structure_type, name: "suspend_always", scope: !26, file: !28, line: 324, size: 8, flags: DIFlagTypePassByValue, elements: !57, identifier: "_ZTSNSt7__n486114suspend_alwaysE")
!57 = !{!58, !64, !100}
!58 = !DISubprogram(name: "await_ready", linkageName: "_ZNKSt7__n486114suspend_always11await_readyEv", scope: !56, file: !28, line: 326, type: !59, scopeLine: 326, flags: DIFlagPrototyped, spFlags: 0)
!59 = !DISubroutineType(types: !60)
!60 = !{!61, !62}
!61 = !DIBasicType(name: "bool", size: 8, encoding: DW_ATE_boolean)
!62 = !DIDerivedType(tag: DW_TAG_pointer_type, baseType: !63, size: 64, flags: DIFlagArtificial | DIFlagObjectPointer)
!63 = !DIDerivedType(tag: DW_TAG_const_type, baseType: !56)
!64 = !DISubprogram(name: "await_suspend", linkageName: "_ZNKSt7__n486114suspend_always13await_suspendENS_16coroutine_handleIvEE", scope: !56, file: !28, line: 328, type: !65, scopeLine: 328, flags: DIFlagPrototyped, spFlags: 0)
!65 = !DISubroutineType(types: !66)
!66 = !{null, !62, !67}
!67 = distinct !DICompositeType(tag: DW_TAG_structure_type, name: "coroutine_handle<void>", scope: !26, file: !28, line: 98, size: 64, flags: DIFlagTypePassByValue | DIFlagNonTrivial, elements: !68, templateParams: !98, identifier: "_ZTSNSt7__n486116coroutine_handleIvEE")
!68 = !{!69, !70, !74, !77, !81, !86, !89, !92, !93, !96, !97}
!69 = !DIDerivedType(tag: DW_TAG_member, baseType: !31, name: "_M_fr_ptr", scope: !67, file: !28, line: 142, size: 64, offset: 0, flags: DIFlagProtected)
!70 = !DISubprogram(name: "coroutine_handle", scope: !67, file: !28, line: 102, type: !71, scopeLine: 102, flags: DIFlagPrototyped, spFlags: 0)
!71 = !DISubroutineType(types: !72)
!72 = !{null, !73}
!73 = !DIDerivedType(tag: DW_TAG_pointer_type, baseType: !67, size: 64, flags: DIFlagArtificial | DIFlagObjectPointer)
!74 = !DISubprogram(name: "coroutine_handle", scope: !67, file: !28, line: 104, type: !75, scopeLine: 104, flags: DIFlagPrototyped, spFlags: 0)
!75 = !DISubroutineType(types: !76)
!76 = !{null, !73, !39}
!77 = !DISubprogram(name: "operator=", linkageName: "_ZNSt7__n486116coroutine_handleIvEaSEDn", scope: !67, file: !28, line: 108, type: !78, scopeLine: 108, flags: DIFlagPrototyped, spFlags: 0)
!78 = !DISubroutineType(types: !79)
!79 = !{!80, !73, !39}
!80 = !DIDerivedType(tag: DW_TAG_reference_type, baseType: !67, size: 64)
!81 = !DISubprogram(name: "address", linkageName: "_ZNKSt7__n486116coroutine_handleIvE7addressEv", scope: !67, file: !28, line: 116, type: !82, scopeLine: 116, flags: DIFlagPrototyped, spFlags: 0)
!82 = !DISubroutineType(types: !83)
!83 = !{!31, !84}
!84 = !DIDerivedType(tag: DW_TAG_pointer_type, baseType: !85, size: 64, flags: DIFlagArtificial | DIFlagObjectPointer)
!85 = !DIDerivedType(tag: DW_TAG_const_type, baseType: !67)
!86 = !DISubprogram(name: "from_address", linkageName: "_ZNSt7__n486116coroutine_handleIvE12from_addressEPv", scope: !67, file: !28, line: 118, type: !87, scopeLine: 118, flags: DIFlagStaticMember | DIFlagPrototyped, spFlags: 0)
!87 = !DISubroutineType(types: !88)
!88 = !{!67, !31}
!89 = !DISubprogram(name: "operator bool", linkageName: "_ZNKSt7__n486116coroutine_handleIvEcvbEv", scope: !67, file: !28, line: 127, type: !90, scopeLine: 127, flags: DIFlagExplicit | DIFlagPrototyped, spFlags: 0)
!90 = !DISubroutineType(types: !91)
!91 = !{!61, !84}
!92 = !DISubprogram(name: "done", linkageName: "_ZNKSt7__n486116coroutine_handleIvE4doneEv", scope: !67, file: !28, line: 132, type: !90, scopeLine: 132, flags: DIFlagPrototyped, spFlags: 0)
!93 = !DISubprogram(name: "operator()", linkageName: "_ZNKSt7__n486116coroutine_handleIvEclEv", scope: !67, file: !28, line: 135, type: !94, scopeLine: 135, flags: DIFlagPrototyped, spFlags: 0)
!94 = !DISubroutineType(types: !95)
!95 = !{null, !84}
!96 = !DISubprogram(name: "resume", linkageName: "_ZNKSt7__n486116coroutine_handleIvE6resumeEv", scope: !67, file: !28, line: 137, type: !94, scopeLine: 137, flags: DIFlagPrototyped, spFlags: 0)
!97 = !DISubprogram(name: "destroy", linkageName: "_ZNKSt7__n486116coroutine_handleIvE7destroyEv", scope: !67, file: !28, line: 139, type: !94, scopeLine: 139, flags: DIFlagPrototyped, spFlags: 0)
!98 = !{!99}
!99 = !DITemplateTypeParameter(name: "_Promise", type: null, defaulted: true)
!100 = !DISubprogram(name: "await_resume", linkageName: "_ZNKSt7__n486114suspend_always12await_resumeEv", scope: !56, file: !28, line: 330, type: !101, scopeLine: 330, flags: DIFlagPrototyped, spFlags: 0)
!101 = !DISubroutineType(types: !102)
!102 = !{null, !62}
!103 = !DISubprogram(name: "final_suspend", linkageName: "_ZN3gen12promise_type13final_suspendEv", scope: !46, file: !1, line: 16, type: !54, scopeLine: 16, flags: DIFlagPrototyped, spFlags: 0)
!104 = !DISubprogram(name: "yield_value", linkageName: "_ZN3gen12promise_type11yield_valueEi", scope: !46, file: !1, line: 17, type: !105, scopeLine: 17, flags: DIFlagPrototyped, spFlags: 0)
!105 = !DISubroutineType(types: !106)
!106 = !{!56, !52, !7}
!107 = !DISubprogram(name: "return_void", linkageName: "_ZN3gen12promise_type11return_voidEv", scope: !46, file: !1, line: 18, type: !108, scopeLine: 18, flags: DIFlagPrototyped, spFlags: 0)
!108 = !DISubroutineType(types: !109)
!109 = !{null, !52}
!110 = !DISubprogram(name: "unhandled_exception", linkageName: "_ZN3gen12promise_type19unhandled_exceptionEv", scope: !46, file: !1, line: 19, type: !108, scopeLine: 19, flags: DIFlagPrototyped, spFlags: 0)
!111 = !DISubprogram(name: "operator=", linkageName: "_ZNSt7__n486116coroutine_handleIN3gen12promise_typeEEaSEDn", scope: !25, file: !28, line: 210, type: !112, scopeLine: 210, flags: DIFlagPrototyped, spFlags: 0)
!112 = !DISubroutineType(types: !113)
!113 = !{!114, !35, !39}
!114 = !DIDerivedType(tag: DW_TAG_reference_type, baseType: !25, size: 64)
!115 = !DISubprogram(name: "address", linkageName: "_ZNKSt7__n486116coroutine_handleIN3gen12promise_typeEE7addressEv", scope: !25, file: !28, line: 218, type: !116, scopeLine: 218, flags: DIFlagPrototyped, spFlags: 0)
!116 = !DISubroutineType(types: !117)
!117 = !{!31, !118}
!118 = !DIDerivedType(tag: DW_TAG_pointer_type, baseType: !119, size: 64, flags: DIFlagArtificial | DIFlagObjectPointer)
!119 = !DIDerivedType(tag: DW_TAG_const_type, baseType: !25)
!120 = !DISubprogram(name: "from_address", linkageName: "_ZNSt7__n486116coroutine_handleIN3gen12promise_typeEE12from_addressEPv", scope: !25, file: !28, line: 220, type: !121, scopeLine: 220, flags: DIFlagStaticMember | DIFlagPrototyped, spFlags: 0)
!121 = !DISubroutineType(types: !122)
!122 = !{!25, !31}
!123 = !DISubprogram(name: "operator coroutine_handle", linkageName: "_ZNKSt7__n486116coroutine_handleIN3gen12promise_typeEEcvNS0_IvEEEv", scope: !25, file: !28, line: 228, type: !124, scopeLine: 228, flags: DIFlagPrototyped, spFlags: 0)
!124 = !DISubroutineType(types: !125)
!125 = !{!67, !118}
!126 = !DISubprogram(name: "operator bool", linkageName: "_ZNKSt7__n486116coroutine_handleIN3gen12promise_typeEEcvbEv", scope: !25, file: !28, line: 232, type: !127, scopeLine: 232, flags: DIFlagExplicit | DIFlagPrototyped, spFlags: 0)
!127 = !DISubroutineType(types: !128)
!128 = !{!61, !118}
!129 = !DISubprogram(name: "done", linkageName: "_ZNKSt7__n486116coroutine_handleIN3gen12promise_typeEE4doneEv", scope: !25, file: !28, line: 237, type: !127, scopeLine: 237, flags: DIFlagPrototyped, spFlags: 0)
!130 = !DISubprogram(name: "operator()", linkageName: "_ZNKSt7__n486116coroutine_handleIN3gen12promise_typeEEclEv", scope: !25, file: !28, line: 240, type: !131, scopeLine: 240, flags: DIFlagPrototyped, spFlags: 0)
!131 = !DISubroutineType(types: !132)
!132 = !{null, !118}
!133 = !DISubprogram(name: "resume", linkageName: "_ZNKSt7__n486116coroutine_handleIN3gen12promise_typeEE6resumeEv", scope: !25, file: !28, line: 242, type: !131, scopeLine: 242, flags: DIFlagPrototyped, spFlags: 0)
!134 = !DISubprogram(name: "destroy", linkageName: "_ZNKSt7__n486116coroutine_handleIN3gen12promise_typeEE7destroyEv", scope: !25, file: !28, line: 244, type: !131, scopeLine: 244, flags: DIFlagPrototyped, spFlags: 0)
!135 = !DISubprogram(name: "promise", linkageName: "_ZNKSt7__n486116coroutine_handleIN3gen12promise_typeEE7promiseEv", scope: !25, file: !28, line: 247, type: !136, scopeLine: 247, flags: DIFlagPrototyped, spFlags: 0)
!136 = !DISubroutineType(types: !137)
!137 = !{!45, !118}
!138 = !{!139}
!139 = !DITemplateTypeParameter(name: "_Promise", type: !46)
!140 = !DIDerivedType(tag: DW_TAG_pointer_type, baseType: !141, size: 64)
!141 = !DIBasicType(name: "char", size: 8, encoding: DW_ATE_unsigned_char)
!142 = !DIDerivedType(tag: DW_TAG_pointer_type, baseType: !46, size: 64)
!143 = !{!144, !150, !156, !161, !165, !167, !169, !171, !173, !180, !185, !192, !196, !200, !204, !212, !216, !218, !223, !229, !233, !240, !242, !246, !250, !254, !256, !260, !264, !266, !270, !272, !274, !278, !282, !286, !290, !294, !298, !300, !308, !312, !316, !321, !323, !325, !329, !333, !334, !335, !336, !337}
!144 = !DIImportedEntity(tag: DW_TAG_imported_declaration, scope: !27, entity: !145, file: !149, line: 98)
!145 = !DIDerivedType(tag: DW_TAG_typedef, baseType: !146, name: "FILE", file: !148, line: 7)
!146 = !DICompositeType(tag: DW_TAG_structure_type, name: "_IO_FILE", file: !147, line: 50, size: 1728, flags: DIFlagFwdDecl, identifier: "_ZTS8_IO_FILE")
!147 = !DIFile(filename: "/usr/include/aarch64-linux-gnu/bits/types/struct_FILE.h", directory: "", checksumkind: CSK_MD5, checksum: "8b2aa9af4ced87e40d6c436b1f5f8bda")
!148 = !DIFile(filename: "/usr/include/aarch64-linux-gnu/bits/types/FILE.h", directory: "", checksumkind: CSK_MD5, checksum: "571f9fb6223c42439075fdde11a0de5d")
!149 = !DIFile(filename: "/usr/lib/gcc/aarch64-linux-gnu/14/../../../../include/c++/14/cstdio", directory: "", checksumkind: CSK_MD5, checksum: "70f0252c98fe3660f9c2b580df7cdd76")
!150 = !DIImportedEntity(tag: DW_TAG_imported_declaration, scope: !27, entity: !151, file: !149, line: 99)
!151 = !DIDerivedType(tag: DW_TAG_typedef, baseType: !152, name: "fpos_t", file: !155, line: 85)
!152 = !DIDerivedType(tag: DW_TAG_typedef, baseType: !153, name: "__fpos_t", file: !154, line: 14)
!153 = !DICompositeType(tag: DW_TAG_structure_type, name: "_G_fpos_t", file: !154, line: 10, size: 128, flags: DIFlagFwdDecl, identifier: "_ZTS9_G_fpos_t")
!154 = !DIFile(filename: "/usr/include/aarch64-linux-gnu/bits/types/__fpos_t.h", directory: "", checksumkind: CSK_MD5, checksum: "32de8bdaf3551a6c0a9394f9af4389ce")
!155 = !DIFile(filename: "/usr/include/stdio.h", directory: "", checksumkind: CSK_MD5, checksum: "47768dac06badcdb9c30a3d8abe4063b")
!156 = !DIImportedEntity(tag: DW_TAG_imported_declaration, scope: !27, entity: !157, file: !149, line: 101)
!157 = !DISubprogram(name: "clearerr", scope: !155, file: !155, line: 850, type: !158, flags: DIFlagPrototyped, spFlags: 0)
!158 = !DISubroutineType(types: !159)
!159 = !{null, !160}
!160 = !DIDerivedType(tag: DW_TAG_pointer_type, baseType: !145, size: 64)
!161 = !DIImportedEntity(tag: DW_TAG_imported_declaration, scope: !27, entity: !162, file: !149, line: 102)
!162 = !DISubprogram(name: "fclose", scope: !155, file: !155, line: 184, type: !163, flags: DIFlagPrototyped, spFlags: 0)
!163 = !DISubroutineType(types: !164)
!164 = !{!7, !160}
!165 = !DIImportedEntity(tag: DW_TAG_imported_declaration, scope: !27, entity: !166, file: !149, line: 103)
!166 = !DISubprogram(name: "feof", scope: !155, file: !155, line: 852, type: !163, flags: DIFlagPrototyped, spFlags: 0)
!167 = !DIImportedEntity(tag: DW_TAG_imported_declaration, scope: !27, entity: !168, file: !149, line: 104)
!168 = !DISubprogram(name: "ferror", scope: !155, file: !155, line: 854, type: !163, flags: DIFlagPrototyped, spFlags: 0)
!169 = !DIImportedEntity(tag: DW_TAG_imported_declaration, scope: !27, entity: !170, file: !149, line: 105)
!170 = !DISubprogram(name: "fflush", scope: !155, file: !155, line: 236, type: !163, flags: DIFlagPrototyped, spFlags: 0)
!171 = !DIImportedEntity(tag: DW_TAG_imported_declaration, scope: !27, entity: !172, file: !149, line: 106)
!172 = !DISubprogram(name: "fgetc", scope: !155, file: !155, line: 575, type: !163, flags: DIFlagPrototyped, spFlags: 0)
!173 = !DIImportedEntity(tag: DW_TAG_imported_declaration, scope: !27, entity: !174, file: !149, line: 107)
!174 = !DISubprogram(name: "fgetpos", scope: !155, file: !155, line: 819, type: !175, flags: DIFlagPrototyped, spFlags: 0)
!175 = !DISubroutineType(types: !176)
!176 = !{!7, !177, !178}
!177 = !DIDerivedType(tag: DW_TAG_restrict_type, baseType: !160)
!178 = !DIDerivedType(tag: DW_TAG_restrict_type, baseType: !179)
!179 = !DIDerivedType(tag: DW_TAG_pointer_type, baseType: !151, size: 64)
!180 = !DIImportedEntity(tag: DW_TAG_imported_declaration, scope: !27, entity: !181, file: !149, line: 108)
!181 = !DISubprogram(name: "fgets", scope: !155, file: !155, line: 654, type: !182, flags: DIFlagPrototyped, spFlags: 0)
!182 = !DISubroutineType(types: !183)
!183 = !{!140, !184, !7, !177}
!184 = !DIDerivedType(tag: DW_TAG_restrict_type, baseType: !140)
!185 = !DIImportedEntity(tag: DW_TAG_imported_declaration, scope: !27, entity: !186, file: !149, line: 109)
!186 = !DISubprogram(name: "fopen", scope: !155, file: !155, line: 264, type: !187, flags: DIFlagPrototyped, spFlags: 0)
!187 = !DISubroutineType(types: !188)
!188 = !{!160, !189, !189}
!189 = !DIDerivedType(tag: DW_TAG_restrict_type, baseType: !190)
!190 = !DIDerivedType(tag: DW_TAG_pointer_type, baseType: !191, size: 64)
!191 = !DIDerivedType(tag: DW_TAG_const_type, baseType: !141)
!192 = !DIImportedEntity(tag: DW_TAG_imported_declaration, scope: !27, entity: !193, file: !149, line: 110)
!193 = !DISubprogram(name: "fprintf", scope: !155, file: !155, line: 357, type: !194, flags: DIFlagPrototyped, spFlags: 0)
!194 = !DISubroutineType(types: !195)
!195 = !{!7, !177, !189, null}
!196 = !DIImportedEntity(tag: DW_TAG_imported_declaration, scope: !27, entity: !197, file: !149, line: 111)
!197 = !DISubprogram(name: "fputc", scope: !155, file: !155, line: 611, type: !198, flags: DIFlagPrototyped, spFlags: 0)
!198 = !DISubroutineType(types: !199)
!199 = !{!7, !7, !160}
!200 = !DIImportedEntity(tag: DW_TAG_imported_declaration, scope: !27, entity: !201, file: !149, line: 112)
!201 = !DISubprogram(name: "fputs", scope: !155, file: !155, line: 707, type: !202, flags: DIFlagPrototyped, spFlags: 0)
!202 = !DISubroutineType(types: !203)
!203 = !{!7, !189, !177}
!204 = !DIImportedEntity(tag: DW_TAG_imported_declaration, scope: !27, entity: !205, file: !149, line: 113)
!205 = !DISubprogram(name: "fread", scope: !155, file: !155, line: 728, type: !206, flags: DIFlagPrototyped, spFlags: 0)
!206 = !DISubroutineType(types: !207)
!207 = !{!208, !211, !208, !208, !177}
!208 = !DIDerivedType(tag: DW_TAG_typedef, baseType: !209, name: "size_t", file: !210, line: 18)
!209 = !DIBasicType(name: "unsigned long", size: 64, encoding: DW_ATE_unsigned)
!210 = !DIFile(filename: "kiln/resource/include/__stddef_size_t.h", directory: "", checksumkind: CSK_MD5, checksum: "2c44e821a2b1951cde2eb0fb2e656867")
!211 = !DIDerivedType(tag: DW_TAG_restrict_type, baseType: !31)
!212 = !DIImportedEntity(tag: DW_TAG_imported_declaration, scope: !27, entity: !213, file: !149, line: 114)
!213 = !DISubprogram(name: "freopen", scope: !155, file: !155, line: 271, type: !214, flags: DIFlagPrototyped, spFlags: 0)
!214 = !DISubroutineType(types: !215)
!215 = !{!160, !189, !189, !177}
!216 = !DIImportedEntity(tag: DW_TAG_imported_declaration, scope: !27, entity: !217, file: !149, line: 115)
!217 = !DISubprogram(name: "fscanf", linkageName: "__isoc23_fscanf", scope: !155, file: !155, line: 442, type: !194, flags: DIFlagPrototyped, spFlags: 0)
!218 = !DIImportedEntity(tag: DW_TAG_imported_declaration, scope: !27, entity: !219, file: !149, line: 116)
!219 = !DISubprogram(name: "fseek", scope: !155, file: !155, line: 769, type: !220, flags: DIFlagPrototyped, spFlags: 0)
!220 = !DISubroutineType(types: !221)
!221 = !{!7, !160, !222, !7}
!222 = !DIBasicType(name: "long", size: 64, encoding: DW_ATE_signed)
!223 = !DIImportedEntity(tag: DW_TAG_imported_declaration, scope: !27, entity: !224, file: !149, line: 117)
!224 = !DISubprogram(name: "fsetpos", scope: !155, file: !155, line: 825, type: !225, flags: DIFlagPrototyped, spFlags: 0)
!225 = !DISubroutineType(types: !226)
!226 = !{!7, !160, !227}
!227 = !DIDerivedType(tag: DW_TAG_pointer_type, baseType: !228, size: 64)
!228 = !DIDerivedType(tag: DW_TAG_const_type, baseType: !151)
!229 = !DIImportedEntity(tag: DW_TAG_imported_declaration, scope: !27, entity: !230, file: !149, line: 118)
!230 = !DISubprogram(name: "ftell", scope: !155, file: !155, line: 775, type: !231, flags: DIFlagPrototyped, spFlags: 0)
!231 = !DISubroutineType(types: !232)
!232 = !{!222, !160}
!233 = !DIImportedEntity(tag: DW_TAG_imported_declaration, scope: !27, entity: !234, file: !149, line: 119)
!234 = !DISubprogram(name: "fwrite", scope: !155, file: !155, line: 735, type: !235, flags: DIFlagPrototyped, spFlags: 0)
!235 = !DISubroutineType(types: !236)
!236 = !{!208, !237, !208, !208, !177}
!237 = !DIDerivedType(tag: DW_TAG_restrict_type, baseType: !238)
!238 = !DIDerivedType(tag: DW_TAG_pointer_type, baseType: !239, size: 64)
!239 = !DIDerivedType(tag: DW_TAG_const_type, baseType: null)
!240 = !DIImportedEntity(tag: DW_TAG_imported_declaration, scope: !27, entity: !241, file: !149, line: 120)
!241 = !DISubprogram(name: "getc", scope: !155, file: !155, line: 576, type: !163, flags: DIFlagPrototyped, spFlags: 0)
!242 = !DIImportedEntity(tag: DW_TAG_imported_declaration, scope: !27, entity: !243, file: !149, line: 121)
!243 = !DISubprogram(name: "getchar", scope: !155, file: !155, line: 582, type: !244, flags: DIFlagPrototyped, spFlags: 0)
!244 = !DISubroutineType(types: !245)
!245 = !{!7}
!246 = !DIImportedEntity(tag: DW_TAG_imported_declaration, scope: !27, entity: !247, file: !149, line: 126)
!247 = !DISubprogram(name: "perror", scope: !155, file: !155, line: 868, type: !248, flags: DIFlagPrototyped, spFlags: 0)
!248 = !DISubroutineType(types: !249)
!249 = !{null, !190}
!250 = !DIImportedEntity(tag: DW_TAG_imported_declaration, scope: !27, entity: !251, file: !149, line: 127)
!251 = !DISubprogram(name: "printf", scope: !155, file: !155, line: 363, type: !252, flags: DIFlagPrototyped, spFlags: 0)
!252 = !DISubroutineType(types: !253)
!253 = !{!7, !189, null}
!254 = !DIImportedEntity(tag: DW_TAG_imported_declaration, scope: !27, entity: !255, file: !149, line: 128)
!255 = !DISubprogram(name: "putc", scope: !155, file: !155, line: 612, type: !198, flags: DIFlagPrototyped, spFlags: 0)
!256 = !DIImportedEntity(tag: DW_TAG_imported_declaration, scope: !27, entity: !257, file: !149, line: 129)
!257 = !DISubprogram(name: "putchar", scope: !155, file: !155, line: 618, type: !258, flags: DIFlagPrototyped, spFlags: 0)
!258 = !DISubroutineType(types: !259)
!259 = !{!7, !7}
!260 = !DIImportedEntity(tag: DW_TAG_imported_declaration, scope: !27, entity: !261, file: !149, line: 130)
!261 = !DISubprogram(name: "puts", scope: !155, file: !155, line: 714, type: !262, flags: DIFlagPrototyped, spFlags: 0)
!262 = !DISubroutineType(types: !263)
!263 = !{!7, !190}
!264 = !DIImportedEntity(tag: DW_TAG_imported_declaration, scope: !27, entity: !265, file: !149, line: 131)
!265 = !DISubprogram(name: "remove", scope: !155, file: !155, line: 158, type: !262, flags: DIFlagPrototyped, spFlags: 0)
!266 = !DIImportedEntity(tag: DW_TAG_imported_declaration, scope: !27, entity: !267, file: !149, line: 132)
!267 = !DISubprogram(name: "rename", scope: !155, file: !155, line: 160, type: !268, flags: DIFlagPrototyped, spFlags: 0)
!268 = !DISubroutineType(types: !269)
!269 = !{!7, !190, !190}
!270 = !DIImportedEntity(tag: DW_TAG_imported_declaration, scope: !27, entity: !271, file: !149, line: 133)
!271 = !DISubprogram(name: "rewind", scope: !155, file: !155, line: 780, type: !158, flags: DIFlagPrototyped, spFlags: 0)
!272 = !DIImportedEntity(tag: DW_TAG_imported_declaration, scope: !27, entity: !273, file: !149, line: 134)
!273 = !DISubprogram(name: "scanf", linkageName: "__isoc23_scanf", scope: !155, file: !155, line: 445, type: !252, flags: DIFlagPrototyped, spFlags: 0)
!274 = !DIImportedEntity(tag: DW_TAG_imported_declaration, scope: !27, entity: !275, file: !149, line: 135)
!275 = !DISubprogram(name: "setbuf", scope: !155, file: !155, line: 334, type: !276, flags: DIFlagPrototyped, spFlags: 0)
!276 = !DISubroutineType(types: !277)
!277 = !{null, !177, !184}
!278 = !DIImportedEntity(tag: DW_TAG_imported_declaration, scope: !27, entity: !279, file: !149, line: 136)
!279 = !DISubprogram(name: "setvbuf", scope: !155, file: !155, line: 339, type: !280, flags: DIFlagPrototyped, spFlags: 0)
!280 = !DISubroutineType(types: !281)
!281 = !{!7, !177, !184, !7, !208}
!282 = !DIImportedEntity(tag: DW_TAG_imported_declaration, scope: !27, entity: !283, file: !149, line: 137)
!283 = !DISubprogram(name: "sprintf", scope: !155, file: !155, line: 365, type: !284, flags: DIFlagPrototyped, spFlags: 0)
!284 = !DISubroutineType(types: !285)
!285 = !{!7, !184, !189, null}
!286 = !DIImportedEntity(tag: DW_TAG_imported_declaration, scope: !27, entity: !287, file: !149, line: 138)
!287 = !DISubprogram(name: "sscanf", linkageName: "__isoc23_sscanf", scope: !155, file: !155, line: 447, type: !288, flags: DIFlagPrototyped, spFlags: 0)
!288 = !DISubroutineType(types: !289)
!289 = !{!7, !189, !189, null}
!290 = !DIImportedEntity(tag: DW_TAG_imported_declaration, scope: !27, entity: !291, file: !149, line: 139)
!291 = !DISubprogram(name: "tmpfile", scope: !155, file: !155, line: 194, type: !292, flags: DIFlagPrototyped, spFlags: 0)
!292 = !DISubroutineType(types: !293)
!293 = !{!160}
!294 = !DIImportedEntity(tag: DW_TAG_imported_declaration, scope: !27, entity: !295, file: !149, line: 141)
!295 = !DISubprogram(name: "tmpnam", scope: !155, file: !155, line: 211, type: !296, flags: DIFlagPrototyped, spFlags: 0)
!296 = !DISubroutineType(types: !297)
!297 = !{!140, !140}
!298 = !DIImportedEntity(tag: DW_TAG_imported_declaration, scope: !27, entity: !299, file: !149, line: 143)
!299 = !DISubprogram(name: "ungetc", scope: !155, file: !155, line: 721, type: !198, flags: DIFlagPrototyped, spFlags: 0)
!300 = !DIImportedEntity(tag: DW_TAG_imported_declaration, scope: !27, entity: !301, file: !149, line: 144)
!301 = !DISubprogram(name: "vfprintf", scope: !155, file: !155, line: 372, type: !302, flags: DIFlagPrototyped, spFlags: 0)
!302 = !DISubroutineType(types: !303)
!303 = !{!7, !177, !189, !304}
!304 = !DIDerivedType(tag: DW_TAG_typedef, baseType: !305, name: "__gnuc_va_list", file: !307, line: 12)
!305 = !DIDerivedType(tag: DW_TAG_typedef, baseType: !306, name: "__builtin_va_list", file: !1)
!306 = !DICompositeType(tag: DW_TAG_structure_type, name: "__va_list", scope: !27, file: !1, line: 0, size: 256, flags: DIFlagFwdDecl, identifier: "_ZTSSt9__va_list")
!307 = !DIFile(filename: "kiln/resource/include/__stdarg___gnuc_va_list.h", directory: "", checksumkind: CSK_MD5, checksum: "edb3f2eab991638e4dc94f6e55e3530f")
!308 = !DIImportedEntity(tag: DW_TAG_imported_declaration, scope: !27, entity: !309, file: !149, line: 145)
!309 = !DISubprogram(name: "vprintf", scope: !155, file: !155, line: 378, type: !310, flags: DIFlagPrototyped, spFlags: 0)
!310 = !DISubroutineType(types: !311)
!311 = !{!7, !189, !304}
!312 = !DIImportedEntity(tag: DW_TAG_imported_declaration, scope: !27, entity: !313, file: !149, line: 146)
!313 = !DISubprogram(name: "vsprintf", scope: !155, file: !155, line: 380, type: !314, flags: DIFlagPrototyped, spFlags: 0)
!314 = !DISubroutineType(types: !315)
!315 = !{!7, !184, !189, !304}
!316 = !DIImportedEntity(tag: DW_TAG_imported_declaration, scope: !317, entity: !318, file: !149, line: 175)
!317 = !DINamespace(scope: null, name: "__gnu_cxx")
!318 = !DISubprogram(name: "snprintf", scope: !155, file: !155, line: 385, type: !319, flags: DIFlagPrototyped, spFlags: 0)
!319 = !DISubroutineType(types: !320)
!320 = !{!7, !184, !208, !189, null}
!321 = !DIImportedEntity(tag: DW_TAG_imported_declaration, scope: !317, entity: !322, file: !149, line: 176)
!322 = !DISubprogram(name: "vfscanf", linkageName: "__isoc23_vfscanf", scope: !155, file: !155, line: 511, type: !302, flags: DIFlagPrototyped, spFlags: 0)
!323 = !DIImportedEntity(tag: DW_TAG_imported_declaration, scope: !317, entity: !324, file: !149, line: 177)
!324 = !DISubprogram(name: "vscanf", linkageName: "__isoc23_vscanf", scope: !155, file: !155, line: 516, type: !310, flags: DIFlagPrototyped, spFlags: 0)
!325 = !DIImportedEntity(tag: DW_TAG_imported_declaration, scope: !317, entity: !326, file: !149, line: 178)
!326 = !DISubprogram(name: "vsnprintf", scope: !155, file: !155, line: 389, type: !327, flags: DIFlagPrototyped, spFlags: 0)
!327 = !DISubroutineType(types: !328)
!328 = !{!7, !184, !208, !189, !304}
!329 = !DIImportedEntity(tag: DW_TAG_imported_declaration, scope: !317, entity: !330, file: !149, line: 179)
!330 = !DISubprogram(name: "vsscanf", linkageName: "__isoc23_vsscanf", scope: !155, file: !155, line: 519, type: !331, flags: DIFlagPrototyped, spFlags: 0)
!331 = !DISubroutineType(types: !332)
!332 = !{!7, !189, !189, !304}
!333 = !DIImportedEntity(tag: DW_TAG_imported_declaration, scope: !27, entity: !318, file: !149, line: 185)
!334 = !DIImportedEntity(tag: DW_TAG_imported_declaration, scope: !27, entity: !322, file: !149, line: 186)
!335 = !DIImportedEntity(tag: DW_TAG_imported_declaration, scope: !27, entity: !324, file: !149, line: 187)
!336 = !DIImportedEntity(tag: DW_TAG_imported_declaration, scope: !27, entity: !326, file: !149, line: 188)
!337 = !DIImportedEntity(tag: DW_TAG_imported_declaration, scope: !27, entity: !330, file: !149, line: 189)
!338 = !{i32 7, !"Dwarf Version", i32 5}
!339 = !{i32 2, !"Debug Info Version", i32 3}
!340 = !{i32 1, !"wchar_size", i32 4}
!341 = !{i32 8, !"PIC Level", i32 2}
!342 = !{i32 7, !"PIE Level", i32 2}
!343 = !{i32 7, !"uwtable", i32 2}
!344 = !{i32 7, !"frame-pointer", i32 4}
!345 = !{!"kiln version 1.2.0"}
!346 = distinct !DISubprogram(name: "count", linkageName: "_Z5count3bigi", scope: !1, file: !1, line: 23, type: !347, scopeLine: 23, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !0, retainedNodes: !349)
!347 = !DISubroutineType(types: !348)
!348 = !{!22, !3, !7}
!349 = !{}
!350 = !DILocalVariable(name: "b", arg: 1, scope: !346, file: !1, line: 23, type: !3)
!351 = !DILocation(line: 23, column: 15, scope: !346)
!352 = !DILocalVariable(name: "n", arg: 2, scope: !346, file: !1, line: 23, type: !7)
!353 = !DILocation(line: 23, column: 22, scope: !346)
!354 = !DILocation(line: 23, column: 25, scope: !346)
!355 = !DILocation(line: 23, column: 5, scope: !346)
!356 = !DILocation(line: 24, column: 31, scope: !346)
!357 = !DILocalVariable(name: "b", scope: !346, line: 0, type: !3, flags: DIFlagArtificial)
!358 = !DILocation(line: 0, scope: !346)
!359 = !DILocalVariable(name: "n", scope: !346, line: 0, type: !7, flags: DIFlagArtificial)
!360 = !DILocalVariable(name: "__promise", scope: !346, line: 0, type: !361, flags: DIFlagArtificial)
!361 = !DIDerivedType(tag: DW_TAG_typedef, baseType: !46, name: "promise_type", scope: !362, file: !28, line: 86)
!362 = distinct !DICompositeType(tag: DW_TAG_structure_type, name: "__coroutine_traits_impl<gen, void>", scope: !26, file: !28, line: 80, size: 8, flags: DIFlagTypePassByValue, elements: !349, templateParams: !363, identifier: "_ZTSNSt7__n486123__coroutine_traits_implI3genvEE")
!363 = !{!364, !365}
!364 = !DITemplateTypeParameter(name: "_Result", type: !22)
!365 = !DITemplateTypeParameter(type: null, defaulted: true)
!366 = !DILocation(line: 25, column: 1, scope: !346)
!367 = !DILocation(line: 24, column: 8, scope: !368)
!368 = distinct !DILexicalBlock(scope: !369, file: !1, line: 24, column: 3)
!369 = distinct !DILexicalBlock(scope: !346, file: !1, line: 23, column: 25)
!370 = !DILocalVariable(name: "i", scope: !368, file: !1, line: 24, type: !7)
!371 = !DILocation(line: 24, column: 12, scope: !368)
!372 = !DILocation(line: 24, column: 19, scope: !373)
!373 = distinct !DILexicalBlock(scope: !368, file: !1, line: 24, column: 3)
!374 = !DILocation(line: 24, column: 23, scope: !373)
!375 = !DILocation(line: 24, column: 21, scope: !373)
!376 = !DILocation(line: 24, column: 3, scope: !368)
!377 = !DILocation(line: 24, column: 31, scope: !373)
!378 = !DILocation(line: 24, column: 42, scope: !373)
!379 = !DILocation(line: 24, column: 44, scope: !373)
!380 = !DILocation(line: 24, column: 40, scope: !373)
!381 = !DILocation(line: 24, column: 47, scope: !373)
!382 = !DILocation(line: 24, column: 54, scope: !373)
!383 = !DILocation(line: 24, column: 52, scope: !373)
!384 = !DILocation(line: 24, column: 26, scope: !373)
!385 = !DILocation(line: 24, column: 3, scope: !373)
!386 = distinct !{!386, !376, !387, !388}
!387 = !DILocation(line: 24, column: 54, scope: !368)
!388 = !{!"llvm.loop.mustprogress"}
!389 = distinct !DISubprogram(name: "big", linkageName: "_ZN3bigC2ERKS_", scope: !3, file: !1, line: 8, type: !15, scopeLine: 8, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !0, declaration: !14, retainedNodes: !349)
!390 = !DILocalVariable(name: "this", arg: 1, scope: !389, line: 0, type: !391, flags: DIFlagArtificial | DIFlagObjectPointer)
!391 = !DIDerivedType(tag: DW_TAG_pointer_type, baseType: !3, size: 64)
!392 = !DILocation(line: 0, scope: !389)
!393 = !DILocalVariable(name: "o", arg: 2, scope: !389, file: !1, line: 8, type: !17)
!394 = !DILocation(line: 8, column: 18, scope: !389)
!395 = !DILocalVariable(name: "i", scope: !396, file: !1, line: 8, type: !7)
!396 = distinct !DILexicalBlock(scope: !397, file: !1, line: 8, column: 23)
!397 = distinct !DILexicalBlock(scope: !389, file: !1, line: 8, column: 21)
!398 = !DILocation(line: 8, column: 32, scope: !396)
!399 = !DILocation(line: 8, column: 28, scope: !396)
!400 = !DILocation(line: 8, column: 39, scope: !401)
!401 = distinct !DILexicalBlock(scope: !396, file: !1, line: 8, column: 23)
!402 = !DILocation(line: 8, column: 41, scope: !401)
!403 = !DILocation(line: 8, column: 23, scope: !396)
!404 = !DILocation(line: 8, column: 58, scope: !401)
!405 = !{i64 4}
!406 = !DILocation(line: 8, column: 60, scope: !401)
!407 = !DILocation(line: 8, column: 62, scope: !401)
!408 = !DILocation(line: 8, column: 51, scope: !401)
!409 = !DILocation(line: 8, column: 53, scope: !401)
!410 = !DILocation(line: 8, column: 56, scope: !401)
!411 = !DILocation(line: 8, column: 46, scope: !401)
!412 = !DILocation(line: 8, column: 23, scope: !401)
!413 = distinct !{!413, !403, !414, !388}
!414 = !DILocation(line: 8, column: 63, scope: !396)
!415 = !DILocation(line: 8, column: 66, scope: !389)
!416 = distinct !DISubprogram(name: "promise_type", linkageName: "_ZN3gen12promise_typeC2Ev", scope: !46, file: !1, line: 12, type: !108, scopeLine: 12, flags: DIFlagPrototyped | DIFlagArtificial, spFlags: DISPFlagDefinition, unit: !0, declaration: !417, retainedNodes: !349)
!417 = !DISubprogram(name: "promise_type", scope: !46, line: 0, type: !108, scopeLine: 0, flags: DIFlagArtificial | DIFlagPrototyped, spFlags: 0)
!418 = !DILocalVariable(name: "this", arg: 1, scope: !416, line: 0, type: !142, flags: DIFlagArtificial | DIFlagObjectPointer)
!419 = !DILocation(line: 0, scope: !416)
!420 = !DILocation(line: 13, column: 9, scope: !416)
!421 = !DILocation(line: 12, column: 10, scope: !416)
!422 = distinct !DISubprogram(name: "get_return_object", linkageName: "_ZN3gen12promise_type17get_return_objectEv", scope: !46, file: !1, line: 14, type: !50, scopeLine: 14, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !0, declaration: !49, retainedNodes: !349)
!423 = !DILocalVariable(name: "this", arg: 1, scope: !422, line: 0, type: !142, flags: DIFlagArtificial | DIFlagObjectPointer)
!424 = !DILocation(line: 0, scope: !422)
!425 = !DILocation(line: 14, column: 41, scope: !422)
!426 = !DILocation(line: 14, column: 42, scope: !422)
!427 = !DILocation(line: 14, column: 31, scope: !422)
!428 = distinct !DISubprogram(name: "initial_suspend", linkageName: "_ZN3gen12promise_type15initial_suspendEv", scope: !46, file: !1, line: 15, type: !54, scopeLine: 15, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !0, declaration: !53, retainedNodes: !349)
!429 = !DILocalVariable(name: "this", arg: 1, scope: !428, line: 0, type: !142, flags: DIFlagArtificial | DIFlagObjectPointer)
!430 = !DILocation(line: 0, scope: !428)
!431 = !DILocation(line: 15, column: 54, scope: !428)
!432 = distinct !DISubprogram(name: "await_ready", linkageName: "_ZNKSt7__n486114suspend_always11await_readyEv", scope: !56, file: !28, line: 326, type: !59, scopeLine: 326, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !0, declaration: !58, retainedNodes: !349)
!433 = !DILocalVariable(name: "this", arg: 1, scope: !432, line: 0, type: !434, flags: DIFlagArtificial | DIFlagObjectPointer)
!434 = !DIDerivedType(tag: DW_TAG_pointer_type, baseType: !63, size: 64)
!435 = !DILocation(line: 0, scope: !432)
!436 = !DILocation(line: 326, column: 51, scope: !432)
!437 = distinct !DISubprogram(linkageName: "_Z5count3bigi.__await_suspend_wrapper__init", scope: !1, file: !1, type: !438, flags: DIFlagArtificial, spFlags: DISPFlagLocalToUnit | DISPFlagDefinition, unit: !0)
!438 = !DISubroutineType(types: !349)
!439 = !DILocation(line: 23, column: 5, scope: !437)
!440 = distinct !DISubprogram(name: "await_suspend", linkageName: "_ZNKSt7__n486114suspend_always13await_suspendENS_16coroutine_handleIvEE", scope: !56, file: !28, line: 328, type: !65, scopeLine: 328, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !0, declaration: !64, retainedNodes: !349)
!441 = !DILocalVariable(name: "this", arg: 1, scope: !440, line: 0, type: !434, flags: DIFlagArtificial | DIFlagObjectPointer)
!442 = !DILocation(line: 0, scope: !440)
!443 = !DILocalVariable(arg: 2, scope: !440, file: !28, line: 328, type: !67)
!444 = !DILocation(line: 328, column: 52, scope: !440)
!445 = !DILocation(line: 328, column: 70, scope: !440)
!446 = distinct !DISubprogram(name: "from_address", linkageName: "_ZNSt7__n486116coroutine_handleIN3gen12promise_typeEE12from_addressEPv", scope: !25, file: !28, line: 220, type: !121, scopeLine: 221, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !0, declaration: !120, retainedNodes: !349)
!447 = !DILocalVariable(name: "__a", arg: 1, scope: !446, file: !28, line: 220, type: !31)
!448 = !DILocation(line: 220, column: 60, scope: !446)
!449 = !DILocalVariable(name: "__self", scope: !446, file: !28, line: 222, type: !25)
!450 = !DILocation(line: 222, column: 19, scope: !446)
!451 = !DILocation(line: 223, column: 21, scope: !446)
!452 = !DILocation(line: 223, column: 9, scope: !446)
!453 = !DILocation(line: 223, column: 19, scope: !446)
!454 = !DILocation(line: 224, column: 2, scope: !446)
!455 = distinct !DISubprogram(name: "operator coroutine_handle", linkageName: "_ZNKSt7__n486116coroutine_handleIN3gen12promise_typeEEcvNS0_IvEEEv", scope: !25, file: !28, line: 228, type: !124, scopeLine: 229, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !0, declaration: !123, retainedNodes: !349)
!456 = !DILocalVariable(name: "this", arg: 1, scope: !455, line: 0, type: !457, flags: DIFlagArtificial | DIFlagObjectPointer)
!457 = !DIDerivedType(tag: DW_TAG_pointer_type, baseType: !119, size: 64)
!458 = !DILocation(line: 0, scope: !455)
!459 = !DILocation(line: 229, column: 49, scope: !455)
!460 = !DILocation(line: 229, column: 16, scope: !455)
!461 = !DILocation(line: 229, column: 9, scope: !455)
!462 = distinct !DISubprogram(name: "await_resume", linkageName: "_ZNKSt7__n486114suspend_always12await_resumeEv", scope: !56, file: !28, line: 330, type: !101, scopeLine: 330, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !0, declaration: !100, retainedNodes: !349)
!463 = !DILocalVariable(name: "this", arg: 1, scope: !462, line: 0, type: !434, flags: DIFlagArtificial | DIFlagObjectPointer)
!464 = !DILocation(line: 0, scope: !462)
!465 = !DILocation(line: 330, column: 51, scope: !462)
!466 = distinct !DISubprogram(name: "yield_value", linkageName: "_ZN3gen12promise_type11yield_valueEi", scope: !46, file: !1, line: 17, type: !105, scopeLine: 17, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !0, declaration: !104, retainedNodes: !349)
!467 = !DILocalVariable(name: "this", arg: 1, scope: !466, line: 0, type: !142, flags: DIFlagArtificial | DIFlagObjectPointer)
!468 = !DILocation(line: 0, scope: !466)
!469 = !DILocalVariable(name: "x", arg: 2, scope: !466, file: !1, line: 17, type: !7)
!470 = !DILocation(line: 17, column: 41, scope: !466)
!471 = !DILocation(line: 17, column: 59, scope: !466)
!472 = !DILocation(line: 17, column: 55, scope: !466)
!473 = !DILocation(line: 17, column: 57, scope: !466)
!474 = !DILocation(line: 17, column: 62, scope: !466)
!475 = distinct !DISubprogram(linkageName: "_Z5count3bigi.__await_suspend_wrapper__yield", scope: !1, file: !1, type: !438, flags: DIFlagArtificial, spFlags: DISPFlagLocalToUnit | DISPFlagDefinition, unit: !0)
!476 = !DILocation(line: 24, column: 31, scope: !475)
!477 = distinct !DISubprogram(name: "return_void", linkageName: "_ZN3gen12promise_type11return_voidEv", scope: !46, file: !1, line: 18, type: !108, scopeLine: 18, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !0, declaration: !107, retainedNodes: !349)
!478 = !DILocalVariable(name: "this", arg: 1, scope: !477, line: 0, type: !142, flags: DIFlagArtificial | DIFlagObjectPointer)
!479 = !DILocation(line: 0, scope: !477)
!480 = !DILocation(line: 18, column: 34, scope: !477)
!481 = distinct !DISubprogram(name: "final_suspend", linkageName: "_ZN3gen12promise_type13final_suspendEv", scope: !46, file: !1, line: 16, type: !54, scopeLine: 16, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !0, declaration: !103, retainedNodes: !349)
!482 = !DILocalVariable(name: "this", arg: 1, scope: !481, line: 0, type: !142, flags: DIFlagArtificial | DIFlagObjectPointer)
!483 = !DILocation(line: 0, scope: !481)
!484 = !DILocation(line: 16, column: 52, scope: !481)
!485 = distinct !DISubprogram(linkageName: "_Z5count3bigi.__await_suspend_wrapper__final", scope: !1, file: !1, type: !438, flags: DIFlagArtificial, spFlags: DISPFlagLocalToUnit | DISPFlagDefinition, unit: !0)
!486 = !DILocation(line: 23, column: 5, scope: !485)
!487 = distinct !DISubprogram(name: "~big", linkageName: "_ZN3bigD2Ev", scope: !3, file: !1, line: 9, type: !20, scopeLine: 9, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !0, declaration: !19, retainedNodes: !349)
!488 = !DILocalVariable(name: "this", arg: 1, scope: !487, line: 0, type: !391, flags: DIFlagArtificial | DIFlagObjectPointer)
!489 = !DILocation(line: 0, scope: !487)
!490 = !DILocation(line: 9, column: 37, scope: !491)
!491 = distinct !DILexicalBlock(scope: !487, file: !1, line: 9, column: 10)
!492 = !DILocation(line: 9, column: 12, scope: !491)
!493 = !DILocation(line: 9, column: 44, scope: !487)
!494 = distinct !DISubprogram(name: "main", scope: !1, file: !1, line: 26, type: !244, scopeLine: 26, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !0, retainedNodes: !349)
!495 = !DILocalVariable(name: "g", scope: !494, file: !1, line: 27, type: !22)
!496 = !DILocation(line: 27, column: 7, scope: !494)
!497 = !DILocation(line: 27, column: 17, scope: !494)
!498 = !DILocation(line: 27, column: 11, scope: !494)
!499 = !DILocation(line: 28, column: 10, scope: !500)
!500 = distinct !DILexicalBlock(scope: !494, file: !1, line: 28, column: 3)
!501 = !DILocation(line: 28, column: 12, scope: !500)
!502 = !DILocation(line: 28, column: 8, scope: !500)
!503 = !DILocation(line: 28, column: 25, scope: !504)
!504 = distinct !DILexicalBlock(scope: !500, file: !1, line: 28, column: 3)
!505 = !DILocation(line: 28, column: 27, scope: !504)
!506 = !DILocation(line: 28, column: 22, scope: !504)
!507 = !DILocation(line: 28, column: 3, scope: !500)
!508 = !DILocation(line: 28, column: 71, scope: !504)
!509 = !DILocation(line: 28, column: 73, scope: !504)
!510 = !DILocation(line: 28, column: 83, scope: !504)
!511 = !DILocation(line: 28, column: 49, scope: !504)
!512 = !DILocation(line: 28, column: 37, scope: !504)
!513 = !DILocation(line: 28, column: 39, scope: !504)
!514 = !DILocation(line: 28, column: 3, scope: !504)
!515 = distinct !{!515, !507, !516, !388}
!516 = !DILocation(line: 28, column: 84, scope: !500)
!517 = !DILocation(line: 31, column: 1, scope: !494)
!518 = !DILocation(line: 29, column: 5, scope: !494)
!519 = !DILocation(line: 29, column: 7, scope: !494)
!520 = !DILocation(line: 30, column: 3, scope: !494)
!521 = distinct !DISubprogram(name: "big", linkageName: "_ZN3bigC2Ei", scope: !3, file: !1, line: 7, type: !11, scopeLine: 7, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !0, declaration: !10, retainedNodes: !349)
!522 = !DILocalVariable(name: "this", arg: 1, scope: !521, line: 0, type: !391, flags: DIFlagArtificial | DIFlagObjectPointer)
!523 = !DILocation(line: 0, scope: !521)
!524 = !DILocalVariable(name: "s", arg: 2, scope: !521, file: !1, line: 7, type: !7)
!525 = !DILocation(line: 7, column: 20, scope: !521)
!526 = !DILocalVariable(name: "i", scope: !527, file: !1, line: 7, type: !7)
!527 = distinct !DILexicalBlock(scope: !528, file: !1, line: 7, column: 25)
!528 = distinct !DILexicalBlock(scope: !521, file: !1, line: 7, column: 23)
!529 = !DILocation(line: 7, column: 34, scope: !527)
!530 = !DILocation(line: 7, column: 30, scope: !527)
!531 = !DILocation(line: 7, column: 41, scope: !532)
!532 = distinct !DILexicalBlock(scope: !527, file: !1, line: 7, column: 25)
!533 = !DILocation(line: 7, column: 43, scope: !532)
!534 = !DILocation(line: 7, column: 25, scope: !527)
!535 = !DILocation(line: 7, column: 60, scope: !532)
!536 = !DILocation(line: 7, column: 64, scope: !532)
!537 = !DILocation(line: 7, column: 62, scope: !532)
!538 = !DILocation(line: 7, column: 53, scope: !532)
!539 = !DILocation(line: 7, column: 55, scope: !532)
!540 = !DILocation(line: 7, column: 58, scope: !532)
!541 = !DILocation(line: 7, column: 48, scope: !532)
!542 = !DILocation(line: 7, column: 25, scope: !532)
!543 = distinct !{!543, !534, !544, !388}
!544 = !DILocation(line: 7, column: 64, scope: !527)
!545 = !DILocation(line: 7, column: 67, scope: !521)
!546 = distinct !DISubprogram(name: "resume", linkageName: "_ZNKSt7__n486116coroutine_handleIN3gen12promise_typeEE6resumeEv", scope: !25, file: !28, line: 242, type: !131, scopeLine: 242, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !0, declaration: !133, retainedNodes: !349)
!547 = !DILocalVariable(name: "this", arg: 1, scope: !546, line: 0, type: !457, flags: DIFlagArtificial | DIFlagObjectPointer)
!548 = !DILocation(line: 0, scope: !546)
!549 = !DILocation(line: 242, column: 51, scope: !546)
!550 = !DILocation(line: 242, column: 29, scope: !546)
!551 = !DILocation(line: 242, column: 63, scope: !546)
!552 = distinct !DISubprogram(name: "done", linkageName: "_ZNKSt7__n486116coroutine_handleIN3gen12promise_typeEE4doneEv", scope: !25, file: !28, line: 237, type: !127, scopeLine: 237, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !0, declaration: !129, retainedNodes: !349)
!553 = !DILocalVariable(name: "this", arg: 1, scope: !552, line: 0, type: !457, flags: DIFlagArtificial | DIFlagObjectPointer)
!554 = !DILocation(line: 0, scope: !552)
!555 = !DILocation(line: 237, column: 63, scope: !552)
!556 = !DILocation(line: 237, column: 43, scope: !552)
!557 = !DILocation(line: 237, column: 36, scope: !552)
!558 = distinct !DISubprogram(name: "promise", linkageName: "_ZNKSt7__n486116coroutine_handleIN3gen12promise_typeEE7promiseEv", scope: !25, file: !28, line: 247, type: !136, scopeLine: 248, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !0, declaration: !135, retainedNodes: !349)
!559 = !DILocalVariable(name: "this", arg: 1, scope: !558, line: 0, type: !457, flags: DIFlagArtificial | DIFlagObjectPointer)
!560 = !DILocation(line: 0, scope: !558)
!561 = !DILocalVariable(name: "__t", scope: !558, file: !28, line: 249, type: !31)
!562 = !DILocation(line: 249, column: 8, scope: !558)
!563 = !DILocation(line: 250, column: 30, scope: !558)
!564 = !DILocation(line: 250, column: 6, scope: !558)
!565 = !DILocation(line: 251, column: 33, scope: !558)
!566 = !DILocation(line: 251, column: 2, scope: !558)
!567 = distinct !DISubprogram(name: "destroy", linkageName: "_ZNKSt7__n486116coroutine_handleIN3gen12promise_typeEE7destroyEv", scope: !25, file: !28, line: 244, type: !131, scopeLine: 244, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !0, declaration: !134, retainedNodes: !349)
!568 = !DILocalVariable(name: "this", arg: 1, scope: !567, line: 0, type: !457, flags: DIFlagArtificial | DIFlagObjectPointer)
!569 = !DILocation(line: 0, scope: !567)
!570 = !DILocation(line: 244, column: 53, scope: !567)
!571 = !DILocation(line: 244, column: 30, scope: !567)
!572 = !DILocation(line: 244, column: 65, scope: !567)
!573 = distinct !DISubprogram(name: "from_promise", linkageName: "_ZNSt7__n486116coroutine_handleIN3gen12promise_typeEE12from_promiseERS2_", scope: !25, file: !28, line: 202, type: !43, scopeLine: 203, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !0, declaration: !42, retainedNodes: !349)
!574 = !DILocalVariable(name: "__p", arg: 1, scope: !573, file: !28, line: 202, type: !45)
!575 = !DILocation(line: 202, column: 30, scope: !573)
!576 = !DILocalVariable(name: "__self", scope: !573, file: !28, line: 204, type: !25)
!577 = !DILocation(line: 204, column: 19, scope: !573)
!578 = !DILocation(line: 206, column: 38, scope: !573)
!579 = !DILocation(line: 206, column: 6, scope: !573)
!580 = !DILocation(line: 205, column: 9, scope: !573)
!581 = !DILocation(line: 206, column: 4, scope: !573)
!582 = !DILocation(line: 207, column: 2, scope: !573)
!583 = distinct !DISubprogram(name: "coroutine_handle", linkageName: "_ZNSt7__n486116coroutine_handleIN3gen12promise_typeEEC2Ev", scope: !25, file: !28, line: 197, type: !33, scopeLine: 197, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !0, declaration: !32, retainedNodes: !349)
!584 = !DILocalVariable(name: "this", arg: 1, scope: !583, line: 0, type: !585, flags: DIFlagArtificial | DIFlagObjectPointer)
!585 = !DIDerivedType(tag: DW_TAG_pointer_type, baseType: !25, size: 64)
!586 = !DILocation(line: 0, scope: !583)
!587 = !DILocation(line: 255, column: 13, scope: !583)
!588 = !DILocation(line: 197, column: 47, scope: !583)
!589 = distinct !DISubprogram(name: "from_address", linkageName: "_ZNSt7__n486116coroutine_handleIvE12from_addressEPv", scope: !67, file: !28, line: 118, type: !87, scopeLine: 119, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !0, declaration: !86, retainedNodes: !349)
!590 = !DILocalVariable(name: "__a", arg: 1, scope: !589, file: !28, line: 118, type: !31)
!591 = !DILocation(line: 118, column: 60, scope: !589)
!592 = !DILocalVariable(name: "__self", scope: !589, file: !28, line: 120, type: !67)
!593 = !DILocation(line: 120, column: 19, scope: !589)
!594 = !DILocation(line: 121, column: 21, scope: !589)
!595 = !DILocation(line: 121, column: 9, scope: !589)
!596 = !DILocation(line: 121, column: 19, scope: !589)
!597 = !DILocation(line: 122, column: 2, scope: !589)
!598 = distinct !DISubprogram(name: "address", linkageName: "_ZNKSt7__n486116coroutine_handleIN3gen12promise_typeEE7addressEv", scope: !25, file: !28, line: 218, type: !116, scopeLine: 218, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !0, declaration: !115, retainedNodes: !349)
!599 = !DILocalVariable(name: "this", arg: 1, scope: !598, line: 0, type: !457, flags: DIFlagArtificial | DIFlagObjectPointer)
!600 = !DILocation(line: 0, scope: !598)
!601 = !DILocation(line: 218, column: 57, scope: !598)
!602 = !DILocation(line: 218, column: 50, scope: !598)
!603 = distinct !DISubprogram(name: "coroutine_handle", linkageName: "_ZNSt7__n486116coroutine_handleIvEC2Ev", scope: !67, file: !28, line: 102, type: !71, scopeLine: 102, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !0, declaration: !70, retainedNodes: !349)
!604 = !DILocalVariable(name: "this", arg: 1, scope: !603, line: 0, type: !605, flags: DIFlagArtificial | DIFlagObjectPointer)
!605 = !DIDerivedType(tag: DW_TAG_pointer_type, baseType: !67, size: 64)
!606 = !DILocation(line: 0, scope: !603)
!607 = !DILocation(line: 102, column: 47, scope: !603)
!608 = !DILocation(line: 102, column: 67, scope: !603)
