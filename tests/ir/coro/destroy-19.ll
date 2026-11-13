; clang++-19 -std=c++20 -O0 -S -emit-llvm -Xclang -disable-llvm-passes
; (aarch64-unknown-linux-gnu; the target triple, datalayout, target-cpu and
; target-features removed, so that the seed runs on every target) of:
;
; // A coroutine destroyed from outside at its initial suspend, at a middle
; // suspend and at its final suspend: the locals alive at that point, and
; // the promise, are destroyed exactly once.
; #include <coroutine>
; #include <cstdio>
; static int dtors = 0;
; struct Dtor { int v; ~Dtor() { ++dtors; std::printf("  ~%d\n", v); } };
; struct gen {
;   struct promise_type {
;     Dtor p{0};
;     int cur = 0;
;     gen get_return_object() { return gen{std::coroutine_handle<promise_type>::from_promise(*this)}; }
;     std::suspend_always initial_suspend() noexcept { return {}; }
;     std::suspend_always final_suspend() noexcept { return {}; }
;     std::suspend_always yield_value(int v) noexcept { cur = v; return {}; }
;     void return_void() noexcept {}
;     void unhandled_exception() noexcept {}
;   };
;   std::coroutine_handle<promise_type> h;
; };
; gen three(int base) {
;   Dtor a{base + 1};
;   co_yield 1;
;   Dtor b{base + 2};
;   co_yield 2;
;   Dtor c{base + 3};
; }
; int main() {
;   std::printf("initial\n");
;   gen g = three(10);
;   g.h.destroy();
;   std::printf("dtors %d\n", dtors);
;   dtors = 0;
;   std::printf("middle\n");
;   g = three(20);
;   g.h.resume();
;   g.h.resume();
;   std::printf("at %d\n", g.h.promise().cur);
;   g.h.destroy();
;   std::printf("dtors %d\n", dtors);
;   dtors = 0;
;   std::printf("final\n");
;   g = three(30);
;   g.h.resume();
;   g.h.resume();
;   g.h.resume();
;   std::printf("done %d\n", (int)g.h.done());
;   g.h.destroy();
;   std::printf("dtors %d\n", dtors);
; }
;
;@ covers: spec/ir.md §5.5 intrinsics (coroutine bodies; spec/codegen.md §7.6)
;@ input: pre-opt
;@ dialect: 19
;@ link: c++
;@ stdout: initial
;@ stdout:   ~0
;@ stdout: dtors 1
;@ stdout: middle
;@ stdout: at 2
;@ stdout:   ~22
;@ stdout:   ~21
;@ stdout:   ~0
;@ stdout: dtors 3
;@ stdout: final
;@ stdout:   ~33
;@ stdout:   ~32
;@ stdout:   ~31
;@ stdout: done 1
;@ stdout:   ~0
;@ stdout: dtors 4

source_filename = "destroy.cpp"

%struct.gen = type { %"struct.std::__n4861::coroutine_handle" }
%"struct.std::__n4861::coroutine_handle" = type { ptr }
%"struct.gen::promise_type" = type { %struct.Dtor, i32 }
%struct.Dtor = type { i32 }
%"struct.std::__n4861::suspend_always" = type { i8 }
%"struct.std::__n4861::coroutine_handle.0" = type { ptr }

$_ZN3gen12promise_typeC2Ev = comdat any

$_ZN3gen12promise_type17get_return_objectEv = comdat any

$_ZN3gen12promise_type15initial_suspendEv = comdat any

$_ZNKSt7__n486114suspend_always11await_readyEv = comdat any

$_ZNKSt7__n486114suspend_always13await_suspendENS_16coroutine_handleIvEE = comdat any

$_ZNSt7__n486116coroutine_handleIN3gen12promise_typeEE12from_addressEPv = comdat any

$_ZNKSt7__n486116coroutine_handleIN3gen12promise_typeEEcvNS0_IvEEEv = comdat any

$_ZNKSt7__n486114suspend_always12await_resumeEv = comdat any

$_ZN3gen12promise_type11yield_valueEi = comdat any

$_ZN4DtorD2Ev = comdat any

$_ZN3gen12promise_type11return_voidEv = comdat any

$_ZN3gen12promise_type13final_suspendEv = comdat any

$_ZN3gen12promise_typeD2Ev = comdat any

$_ZNKSt7__n486116coroutine_handleIN3gen12promise_typeEE7destroyEv = comdat any

$_ZNKSt7__n486116coroutine_handleIN3gen12promise_typeEE6resumeEv = comdat any

$_ZNKSt7__n486116coroutine_handleIN3gen12promise_typeEE7promiseEv = comdat any

$_ZNKSt7__n486116coroutine_handleIN3gen12promise_typeEE4doneEv = comdat any

$_ZNSt7__n486116coroutine_handleIN3gen12promise_typeEE12from_promiseERS2_ = comdat any

$_ZNSt7__n486116coroutine_handleIN3gen12promise_typeEEC2Ev = comdat any

$_ZNSt7__n486116coroutine_handleIvE12from_addressEPv = comdat any

$_ZNKSt7__n486116coroutine_handleIN3gen12promise_typeEE7addressEv = comdat any

$_ZNSt7__n486116coroutine_handleIvEC2Ev = comdat any

$__clang_call_terminate = comdat any

@.str = private unnamed_addr constant [9 x i8] c"initial\0A\00", align 1
@.str.1 = private unnamed_addr constant [10 x i8] c"dtors %d\0A\00", align 1
@_ZL5dtors = internal global i32 0, align 4
@.str.2 = private unnamed_addr constant [8 x i8] c"middle\0A\00", align 1
@.str.3 = private unnamed_addr constant [7 x i8] c"at %d\0A\00", align 1
@.str.4 = private unnamed_addr constant [7 x i8] c"final\0A\00", align 1
@.str.5 = private unnamed_addr constant [9 x i8] c"done %d\0A\00", align 1
@.str.6 = private unnamed_addr constant [7 x i8] c"  ~%d\0A\00", align 1

; Function Attrs: mustprogress noinline optnone presplitcoroutine uwtable
define dso_local i64 @_Z5threei(i32 noundef %0) #0 personality ptr @__gxx_personality_v0 {
  %2 = alloca %struct.gen, align 8
  %3 = alloca i32, align 4
  %4 = alloca i32, align 4
  %5 = alloca %"struct.gen::promise_type", align 4
  %6 = alloca ptr, align 8
  %7 = alloca i32, align 4
  %8 = alloca %"struct.std::__n4861::suspend_always", align 1
  %9 = alloca %"struct.std::__n4861::suspend_always", align 1
  %10 = alloca %struct.Dtor, align 4
  %11 = alloca %"struct.std::__n4861::suspend_always", align 1
  %12 = alloca %"struct.std::__n4861::suspend_always", align 1
  %13 = alloca %struct.Dtor, align 4
  %14 = alloca %"struct.std::__n4861::suspend_always", align 1
  %15 = alloca %"struct.std::__n4861::suspend_always", align 1
  %16 = alloca %struct.Dtor, align 4
  %17 = alloca %"struct.std::__n4861::suspend_always", align 1
  %18 = alloca %"struct.std::__n4861::suspend_always", align 1
  store i32 %0, ptr %3, align 4
  %19 = bitcast ptr %5 to ptr
  %20 = call token @llvm.coro.id(i32 16, ptr %19, ptr null, ptr null)
  %21 = call i1 @llvm.coro.alloc(token %20)
  br i1 %21, label %22, label %25

22:                                               ; preds = %1
  %23 = call i64 @llvm.coro.size.i64()
  %24 = call noalias noundef nonnull ptr @_Znwm(i64 noundef %23) #17
  br label %25

25:                                               ; preds = %22, %1
  %26 = phi ptr [ null, %1 ], [ %24, %22 ]
  %27 = call ptr @llvm.coro.begin(token %20, ptr %26)
  call void @llvm.lifetime.start.p0(i64 4, ptr %4) #2
  %28 = load i32, ptr %3, align 4
  store i32 %28, ptr %4, align 4
  call void @llvm.lifetime.start.p0(i64 8, ptr %5) #2
  call void @_ZN3gen12promise_typeC2Ev(ptr noundef nonnull align 4 dereferenceable(8) %5) #2
  %29 = invoke i64 @_ZN3gen12promise_type17get_return_objectEv(ptr noundef nonnull align 4 dereferenceable(8) %5)
          to label %30 unwind label %39

30:                                               ; preds = %25
  %31 = getelementptr inbounds %struct.gen, ptr %2, i32 0, i32 0
  %32 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %31, i32 0, i32 0
  %33 = inttoptr i64 %29 to ptr
  store ptr %33, ptr %32, align 8
  call void @llvm.lifetime.start.p0(i64 1, ptr %8) #2
  call void @_ZN3gen12promise_type15initial_suspendEv(ptr noundef nonnull align 4 dereferenceable(8) %5) #2
  %34 = call noundef i1 @_ZNKSt7__n486114suspend_always11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %8) #2
  br i1 %34, label %45, label %35

35:                                               ; preds = %30
  %36 = call token @llvm.coro.save(ptr null)
  call void @llvm.coro.await.suspend.void(ptr %8, ptr %27, ptr @_Z5threei.__await_suspend_wrapper__init) #2
  %37 = call i8 @llvm.coro.suspend(token %36, i1 false)
  switch i8 %37, label %99 [
    i8 0, label %45
    i8 1, label %38
  ]

38:                                               ; preds = %35
  br label %46

39:                                               ; preds = %25
  %40 = landingpad { ptr, i32 }
          cleanup
  %41 = extractvalue { ptr, i32 } %40, 0
  store ptr %41, ptr %6, align 8
  %42 = extractvalue { ptr, i32 } %40, 1
  store i32 %42, ptr %7, align 4
  call void @_ZN3gen12promise_typeD2Ev(ptr noundef nonnull align 4 dereferenceable(8) %5) #2
  call void @llvm.lifetime.end.p0(i64 8, ptr %5) #2
  call void @llvm.lifetime.end.p0(i64 4, ptr %4) #2
  %43 = call ptr @llvm.coro.free(token %20, ptr %27)
  %44 = icmp ne ptr %43, null
  br i1 %44, label %105, label %107

45:                                               ; preds = %35, %30
  call void @_ZNKSt7__n486114suspend_always12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %8) #2
  br label %46

46:                                               ; preds = %45, %38
  %47 = phi i32 [ 0, %45 ], [ 2, %38 ]
  call void @llvm.lifetime.end.p0(i64 1, ptr %8) #2
  switch i32 %47, label %91 [
    i32 0, label %48
  ]

48:                                               ; preds = %46
  call void @llvm.lifetime.start.p0(i64 4, ptr %10) #2
  %49 = getelementptr inbounds %struct.Dtor, ptr %10, i32 0, i32 0
  %50 = load i32, ptr %4, align 4
  %51 = add nsw i32 %50, 1
  store i32 %51, ptr %49, align 4
  call void @llvm.lifetime.start.p0(i64 1, ptr %11) #2
  call void @_ZN3gen12promise_type11yield_valueEi(ptr noundef nonnull align 4 dereferenceable(8) %5, i32 noundef 1) #2
  %52 = call noundef i1 @_ZNKSt7__n486114suspend_always11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %11) #2
  br i1 %52, label %57, label %53

53:                                               ; preds = %48
  %54 = call token @llvm.coro.save(ptr null)
  call void @llvm.coro.await.suspend.void(ptr %11, ptr %27, ptr @_Z5threei.__await_suspend_wrapper__yield) #2
  %55 = call i8 @llvm.coro.suspend(token %54, i1 false)
  switch i8 %55, label %99 [
    i8 0, label %57
    i8 1, label %56
  ]

56:                                               ; preds = %53
  br label %58

57:                                               ; preds = %53, %48
  call void @_ZNKSt7__n486114suspend_always12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %11) #2
  br label %58

58:                                               ; preds = %57, %56
  %59 = phi i32 [ 0, %57 ], [ 2, %56 ]
  call void @llvm.lifetime.end.p0(i64 1, ptr %11) #2
  switch i32 %59, label %78 [
    i32 0, label %60
  ]

60:                                               ; preds = %58
  call void @llvm.lifetime.start.p0(i64 4, ptr %13) #2
  %61 = getelementptr inbounds %struct.Dtor, ptr %13, i32 0, i32 0
  %62 = load i32, ptr %4, align 4
  %63 = add nsw i32 %62, 2
  store i32 %63, ptr %61, align 4
  call void @llvm.lifetime.start.p0(i64 1, ptr %14) #2
  call void @_ZN3gen12promise_type11yield_valueEi(ptr noundef nonnull align 4 dereferenceable(8) %5, i32 noundef 2) #2
  %64 = call noundef i1 @_ZNKSt7__n486114suspend_always11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %14) #2
  br i1 %64, label %69, label %65

65:                                               ; preds = %60
  %66 = call token @llvm.coro.save(ptr null)
  call void @llvm.coro.await.suspend.void(ptr %14, ptr %27, ptr @_Z5threei.__await_suspend_wrapper__yield2) #2
  %67 = call i8 @llvm.coro.suspend(token %66, i1 false)
  switch i8 %67, label %99 [
    i8 0, label %69
    i8 1, label %68
  ]

68:                                               ; preds = %65
  br label %70

69:                                               ; preds = %65, %60
  call void @_ZNKSt7__n486114suspend_always12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %14) #2
  br label %70

70:                                               ; preds = %69, %68
  %71 = phi i32 [ 0, %69 ], [ 2, %68 ]
  call void @llvm.lifetime.end.p0(i64 1, ptr %14) #2
  switch i32 %71, label %76 [
    i32 0, label %72
  ]

72:                                               ; preds = %70
  call void @llvm.lifetime.start.p0(i64 4, ptr %16) #2
  %73 = getelementptr inbounds %struct.Dtor, ptr %16, i32 0, i32 0
  %74 = load i32, ptr %4, align 4
  %75 = add nsw i32 %74, 3
  store i32 %75, ptr %73, align 4
  call void @_ZN4DtorD2Ev(ptr noundef nonnull align 4 dereferenceable(4) %16) #2
  call void @llvm.lifetime.end.p0(i64 4, ptr %16) #2
  br label %76

76:                                               ; preds = %72, %70
  %77 = phi i32 [ %71, %70 ], [ 0, %72 ]
  call void @_ZN4DtorD2Ev(ptr noundef nonnull align 4 dereferenceable(4) %13) #2
  call void @llvm.lifetime.end.p0(i64 4, ptr %13) #2
  br label %78

78:                                               ; preds = %76, %58
  %79 = phi i32 [ %59, %58 ], [ %77, %76 ]
  call void @_ZN4DtorD2Ev(ptr noundef nonnull align 4 dereferenceable(4) %10) #2
  call void @llvm.lifetime.end.p0(i64 4, ptr %10) #2
  switch i32 %79, label %91 [
    i32 0, label %80
  ]

80:                                               ; preds = %78
  call void @_ZN3gen12promise_type11return_voidEv(ptr noundef nonnull align 4 dereferenceable(8) %5) #2
  br label %81

81:                                               ; preds = %80
  call void @llvm.lifetime.start.p0(i64 1, ptr %17) #2
  call void @_ZN3gen12promise_type13final_suspendEv(ptr noundef nonnull align 4 dereferenceable(8) %5) #2
  %82 = call noundef i1 @_ZNKSt7__n486114suspend_always11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %17) #2
  br i1 %82, label %87, label %83

83:                                               ; preds = %81
  %84 = call token @llvm.coro.save(ptr null)
  call void @llvm.coro.await.suspend.void(ptr %17, ptr %27, ptr @_Z5threei.__await_suspend_wrapper__final) #2
  %85 = call i8 @llvm.coro.suspend(token %84, i1 true)
  switch i8 %85, label %99 [
    i8 0, label %87
    i8 1, label %86
  ]

86:                                               ; preds = %83
  br label %88

87:                                               ; preds = %83, %81
  call void @_ZNKSt7__n486114suspend_always12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %17) #2
  br label %88

88:                                               ; preds = %87, %86
  %89 = phi i32 [ 0, %87 ], [ 2, %86 ]
  call void @llvm.lifetime.end.p0(i64 1, ptr %17) #2
  switch i32 %89, label %91 [
    i32 0, label %90
  ]

90:                                               ; preds = %88
  br label %91

91:                                               ; preds = %90, %88, %78, %46
  %92 = phi i32 [ %47, %46 ], [ %79, %78 ], [ %89, %88 ], [ 0, %90 ]
  call void @_ZN3gen12promise_typeD2Ev(ptr noundef nonnull align 4 dereferenceable(8) %5) #2
  call void @llvm.lifetime.end.p0(i64 8, ptr %5) #2
  call void @llvm.lifetime.end.p0(i64 4, ptr %4) #2
  %93 = call ptr @llvm.coro.free(token %20, ptr %27)
  %94 = icmp ne ptr %93, null
  br i1 %94, label %95, label %97

95:                                               ; preds = %91
  %96 = call i64 @llvm.coro.size.i64()
  call void @_ZdlPvm(ptr noundef %93, i64 noundef %96) #2
  br label %97

97:                                               ; preds = %91, %95
  switch i32 %92, label %113 [
    i32 0, label %98
    i32 2, label %99
  ]

98:                                               ; preds = %97
  br label %99

99:                                               ; preds = %98, %97, %83, %65, %53, %35
  %100 = call i1 @llvm.coro.end(ptr null, i1 false, token none)
  %101 = getelementptr inbounds %struct.gen, ptr %2, i32 0, i32 0
  %102 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %101, i32 0, i32 0
  %103 = load ptr, ptr %102, align 8
  %104 = ptrtoint ptr %103 to i64
  ret i64 %104

105:                                              ; preds = %39
  %106 = call i64 @llvm.coro.size.i64()
  call void @_ZdlPvm(ptr noundef %43, i64 noundef %106) #2
  br label %107

107:                                              ; preds = %39, %105
  br label %108

108:                                              ; preds = %107
  %109 = load ptr, ptr %6, align 8
  %110 = load i32, ptr %7, align 4
  %111 = insertvalue { ptr, i32 } poison, ptr %109, 0
  %112 = insertvalue { ptr, i32 } %111, i32 %110, 1
  resume { ptr, i32 } %112

113:                                              ; preds = %97
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
define linkonce_odr dso_local void @_ZN3gen12promise_typeC2Ev(ptr noundef nonnull align 4 dereferenceable(8) %0) unnamed_addr #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds %"struct.gen::promise_type", ptr %3, i32 0, i32 0
  %5 = getelementptr inbounds %struct.Dtor, ptr %4, i32 0, i32 0
  store i32 0, ptr %5, align 4
  %6 = getelementptr inbounds %"struct.gen::promise_type", ptr %3, i32 0, i32 1
  store i32 0, ptr %6, align 4
  ret void
}

; Function Attrs: mustprogress noinline optnone uwtable
define linkonce_odr dso_local i64 @_ZN3gen12promise_type17get_return_objectEv(ptr noundef nonnull align 4 dereferenceable(8) %0) #7 comdat {
  %2 = alloca %struct.gen, align 8
  %3 = alloca ptr, align 8
  store ptr %0, ptr %3, align 8
  %4 = load ptr, ptr %3, align 8
  %5 = getelementptr inbounds %struct.gen, ptr %2, i32 0, i32 0
  %6 = call i64 @_ZNSt7__n486116coroutine_handleIN3gen12promise_typeEE12from_promiseERS2_(ptr noundef nonnull align 4 dereferenceable(8) %4)
  %7 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %5, i32 0, i32 0
  %8 = inttoptr i64 %6 to ptr
  store ptr %8, ptr %7, align 8
  %9 = getelementptr inbounds %struct.gen, ptr %2, i32 0, i32 0
  %10 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %9, i32 0, i32 0
  %11 = load ptr, ptr %10, align 8
  %12 = ptrtoint ptr %11 to i64
  ret i64 %12
}

declare i32 @__gxx_personality_v0(...)

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZN3gen12promise_type15initial_suspendEv(ptr noundef nonnull align 4 dereferenceable(8) %0) #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  ret void
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local noundef i1 @_ZNKSt7__n486114suspend_always11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %0) #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  ret i1 false
}

; Function Attrs: nomerge nounwind
declare token @llvm.coro.save(ptr) #8

; Function Attrs: alwaysinline mustprogress
define private void @_Z5threei.__await_suspend_wrapper__init(ptr noundef nonnull %0, ptr noundef %1) #9 {
  %3 = alloca ptr, align 8
  %4 = alloca ptr, align 8
  %5 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %6 = alloca %"struct.std::__n4861::coroutine_handle", align 8
  store ptr %0, ptr %3, align 8
  store ptr %1, ptr %4, align 8
  %7 = load ptr, ptr %3, align 8
  %8 = load ptr, ptr %4, align 8
  %9 = call i64 @_ZNSt7__n486116coroutine_handleIN3gen12promise_typeEE12from_addressEPv(ptr noundef %8) #2
  %10 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %6, i32 0, i32 0
  %11 = inttoptr i64 %9 to ptr
  store ptr %11, ptr %10, align 8
  %12 = call i64 @_ZNKSt7__n486116coroutine_handleIN3gen12promise_typeEEcvNS0_IvEEEv(ptr noundef nonnull align 8 dereferenceable(8) %6) #2
  %13 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %5, i32 0, i32 0
  %14 = inttoptr i64 %12 to ptr
  store ptr %14, ptr %13, align 8
  %15 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %5, i32 0, i32 0
  %16 = load ptr, ptr %15, align 8
  %17 = ptrtoint ptr %16 to i64
  call void @_ZNKSt7__n486114suspend_always13await_suspendENS_16coroutine_handleIvEE(ptr noundef nonnull align 1 dereferenceable(1) %7, i64 %17) #2
  ret void
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZNKSt7__n486114suspend_always13await_suspendENS_16coroutine_handleIvEE(ptr noundef nonnull align 1 dereferenceable(1) %0, i64 %1) #6 comdat {
  %3 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %4 = alloca ptr, align 8
  %5 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %3, i32 0, i32 0
  %6 = inttoptr i64 %1 to ptr
  store ptr %6, ptr %5, align 8
  store ptr %0, ptr %4, align 8
  %7 = load ptr, ptr %4, align 8
  ret void
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local i64 @_ZNSt7__n486116coroutine_handleIN3gen12promise_typeEE12from_addressEPv(ptr noundef %0) #6 comdat {
  %2 = alloca %"struct.std::__n4861::coroutine_handle", align 8
  %3 = alloca ptr, align 8
  store ptr %0, ptr %3, align 8
  call void @_ZNSt7__n486116coroutine_handleIN3gen12promise_typeEEC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %2) #2
  %4 = load ptr, ptr %3, align 8
  %5 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %2, i32 0, i32 0
  store ptr %4, ptr %5, align 8
  %6 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %2, i32 0, i32 0
  %7 = load ptr, ptr %6, align 8
  %8 = ptrtoint ptr %7 to i64
  ret i64 %8
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local i64 @_ZNKSt7__n486116coroutine_handleIN3gen12promise_typeEEcvNS0_IvEEEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #6 comdat {
  %2 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %3 = alloca ptr, align 8
  store ptr %0, ptr %3, align 8
  %4 = load ptr, ptr %3, align 8
  %5 = call noundef ptr @_ZNKSt7__n486116coroutine_handleIN3gen12promise_typeEE7addressEv(ptr noundef nonnull align 8 dereferenceable(8) %4) #2
  %6 = call i64 @_ZNSt7__n486116coroutine_handleIvE12from_addressEPv(ptr noundef %5) #2
  %7 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %2, i32 0, i32 0
  %8 = inttoptr i64 %6 to ptr
  store ptr %8, ptr %7, align 8
  %9 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %2, i32 0, i32 0
  %10 = load ptr, ptr %9, align 8
  %11 = ptrtoint ptr %10 to i64
  ret i64 %11
}

declare void @llvm.coro.await.suspend.void(ptr, ptr, ptr)

; Function Attrs: nounwind
declare i8 @llvm.coro.suspend(token, i1) #2

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZNKSt7__n486114suspend_always12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %0) #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  ret void
}

; Function Attrs: nocallback nofree nosync nounwind willreturn memory(argmem: readwrite)
declare void @llvm.lifetime.end.p0(i64 immarg, ptr nocapture) #5

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZN3gen12promise_type11yield_valueEi(ptr noundef nonnull align 4 dereferenceable(8) %0, i32 noundef %1) #6 comdat {
  %3 = alloca ptr, align 8
  %4 = alloca i32, align 4
  store ptr %0, ptr %3, align 8
  store i32 %1, ptr %4, align 4
  %5 = load ptr, ptr %3, align 8
  %6 = load i32, ptr %4, align 4
  %7 = getelementptr inbounds %"struct.gen::promise_type", ptr %5, i32 0, i32 1
  store i32 %6, ptr %7, align 4
  ret void
}

; Function Attrs: alwaysinline mustprogress
define private void @_Z5threei.__await_suspend_wrapper__yield(ptr noundef nonnull %0, ptr noundef %1) #9 {
  %3 = alloca ptr, align 8
  %4 = alloca ptr, align 8
  %5 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %6 = alloca %"struct.std::__n4861::coroutine_handle", align 8
  store ptr %0, ptr %3, align 8
  store ptr %1, ptr %4, align 8
  %7 = load ptr, ptr %3, align 8
  %8 = load ptr, ptr %4, align 8
  %9 = call i64 @_ZNSt7__n486116coroutine_handleIN3gen12promise_typeEE12from_addressEPv(ptr noundef %8) #2
  %10 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %6, i32 0, i32 0
  %11 = inttoptr i64 %9 to ptr
  store ptr %11, ptr %10, align 8
  %12 = call i64 @_ZNKSt7__n486116coroutine_handleIN3gen12promise_typeEEcvNS0_IvEEEv(ptr noundef nonnull align 8 dereferenceable(8) %6) #2
  %13 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %5, i32 0, i32 0
  %14 = inttoptr i64 %12 to ptr
  store ptr %14, ptr %13, align 8
  %15 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %5, i32 0, i32 0
  %16 = load ptr, ptr %15, align 8
  %17 = ptrtoint ptr %16 to i64
  call void @_ZNKSt7__n486114suspend_always13await_suspendENS_16coroutine_handleIvEE(ptr noundef nonnull align 1 dereferenceable(1) %7, i64 %17) #2
  ret void
}

; Function Attrs: alwaysinline mustprogress
define private void @_Z5threei.__await_suspend_wrapper__yield2(ptr noundef nonnull %0, ptr noundef %1) #9 {
  %3 = alloca ptr, align 8
  %4 = alloca ptr, align 8
  %5 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %6 = alloca %"struct.std::__n4861::coroutine_handle", align 8
  store ptr %0, ptr %3, align 8
  store ptr %1, ptr %4, align 8
  %7 = load ptr, ptr %3, align 8
  %8 = load ptr, ptr %4, align 8
  %9 = call i64 @_ZNSt7__n486116coroutine_handleIN3gen12promise_typeEE12from_addressEPv(ptr noundef %8) #2
  %10 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %6, i32 0, i32 0
  %11 = inttoptr i64 %9 to ptr
  store ptr %11, ptr %10, align 8
  %12 = call i64 @_ZNKSt7__n486116coroutine_handleIN3gen12promise_typeEEcvNS0_IvEEEv(ptr noundef nonnull align 8 dereferenceable(8) %6) #2
  %13 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %5, i32 0, i32 0
  %14 = inttoptr i64 %12 to ptr
  store ptr %14, ptr %13, align 8
  %15 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %5, i32 0, i32 0
  %16 = load ptr, ptr %15, align 8
  %17 = ptrtoint ptr %16 to i64
  call void @_ZNKSt7__n486114suspend_always13await_suspendENS_16coroutine_handleIvEE(ptr noundef nonnull align 1 dereferenceable(1) %7, i64 %17) #2
  ret void
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZN4DtorD2Ev(ptr noundef nonnull align 4 dereferenceable(4) %0) unnamed_addr #6 comdat personality ptr @__gxx_personality_v0 {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = load i32, ptr @_ZL5dtors, align 4
  %5 = add nsw i32 %4, 1
  store i32 %5, ptr @_ZL5dtors, align 4
  %6 = getelementptr inbounds %struct.Dtor, ptr %3, i32 0, i32 0
  %7 = load i32, ptr %6, align 4
  %8 = invoke i32 (ptr, ...) @printf(ptr noundef @.str.6, i32 noundef %7)
          to label %9 unwind label %10

9:                                                ; preds = %1
  ret void

10:                                               ; preds = %1
  %11 = landingpad { ptr, i32 }
          catch ptr null
  %12 = extractvalue { ptr, i32 } %11, 0
  call void @__clang_call_terminate(ptr %12) #18
  unreachable
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZN3gen12promise_type11return_voidEv(ptr noundef nonnull align 4 dereferenceable(8) %0) #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  ret void
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZN3gen12promise_type13final_suspendEv(ptr noundef nonnull align 4 dereferenceable(8) %0) #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  ret void
}

; Function Attrs: alwaysinline mustprogress
define private void @_Z5threei.__await_suspend_wrapper__final(ptr noundef nonnull %0, ptr noundef %1) #9 {
  %3 = alloca ptr, align 8
  %4 = alloca ptr, align 8
  %5 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %6 = alloca %"struct.std::__n4861::coroutine_handle", align 8
  store ptr %0, ptr %3, align 8
  store ptr %1, ptr %4, align 8
  %7 = load ptr, ptr %3, align 8
  %8 = load ptr, ptr %4, align 8
  %9 = call i64 @_ZNSt7__n486116coroutine_handleIN3gen12promise_typeEE12from_addressEPv(ptr noundef %8) #2
  %10 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %6, i32 0, i32 0
  %11 = inttoptr i64 %9 to ptr
  store ptr %11, ptr %10, align 8
  %12 = call i64 @_ZNKSt7__n486116coroutine_handleIN3gen12promise_typeEEcvNS0_IvEEEv(ptr noundef nonnull align 8 dereferenceable(8) %6) #2
  %13 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %5, i32 0, i32 0
  %14 = inttoptr i64 %12 to ptr
  store ptr %14, ptr %13, align 8
  %15 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %5, i32 0, i32 0
  %16 = load ptr, ptr %15, align 8
  %17 = ptrtoint ptr %16 to i64
  call void @_ZNKSt7__n486114suspend_always13await_suspendENS_16coroutine_handleIvEE(ptr noundef nonnull align 1 dereferenceable(1) %7, i64 %17) #2
  ret void
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZN3gen12promise_typeD2Ev(ptr noundef nonnull align 4 dereferenceable(8) %0) unnamed_addr #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds %"struct.gen::promise_type", ptr %3, i32 0, i32 0
  call void @_ZN4DtorD2Ev(ptr noundef nonnull align 4 dereferenceable(4) %4) #2
  ret void
}

; Function Attrs: nobuiltin nounwind
declare void @_ZdlPvm(ptr noundef, i64 noundef) #10

; Function Attrs: nounwind memory(argmem: read)
declare ptr @llvm.coro.free(token, ptr nocapture readonly) #11

; Function Attrs: nounwind
declare i1 @llvm.coro.end(ptr, i1, token) #2

; Function Attrs: mustprogress noinline norecurse optnone uwtable
define dso_local noundef i32 @main() #12 {
  %1 = alloca %struct.gen, align 8
  %2 = alloca %struct.gen, align 8
  %3 = alloca %struct.gen, align 8
  %4 = call i32 (ptr, ...) @printf(ptr noundef @.str)
  %5 = call i64 @_Z5threei(i32 noundef 10)
  %6 = getelementptr inbounds %struct.gen, ptr %1, i32 0, i32 0
  %7 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %6, i32 0, i32 0
  %8 = inttoptr i64 %5 to ptr
  store ptr %8, ptr %7, align 8
  %9 = getelementptr inbounds %struct.gen, ptr %1, i32 0, i32 0
  call void @_ZNKSt7__n486116coroutine_handleIN3gen12promise_typeEE7destroyEv(ptr noundef nonnull align 8 dereferenceable(8) %9)
  %10 = load i32, ptr @_ZL5dtors, align 4
  %11 = call i32 (ptr, ...) @printf(ptr noundef @.str.1, i32 noundef %10)
  store i32 0, ptr @_ZL5dtors, align 4
  %12 = call i32 (ptr, ...) @printf(ptr noundef @.str.2)
  %13 = call i64 @_Z5threei(i32 noundef 20)
  %14 = getelementptr inbounds %struct.gen, ptr %2, i32 0, i32 0
  %15 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %14, i32 0, i32 0
  %16 = inttoptr i64 %13 to ptr
  store ptr %16, ptr %15, align 8
  call void @llvm.memcpy.p0.p0.i64(ptr align 8 %1, ptr align 8 %2, i64 8, i1 false)
  %17 = getelementptr inbounds %struct.gen, ptr %1, i32 0, i32 0
  call void @_ZNKSt7__n486116coroutine_handleIN3gen12promise_typeEE6resumeEv(ptr noundef nonnull align 8 dereferenceable(8) %17)
  %18 = getelementptr inbounds %struct.gen, ptr %1, i32 0, i32 0
  call void @_ZNKSt7__n486116coroutine_handleIN3gen12promise_typeEE6resumeEv(ptr noundef nonnull align 8 dereferenceable(8) %18)
  %19 = getelementptr inbounds %struct.gen, ptr %1, i32 0, i32 0
  %20 = call noundef nonnull align 4 dereferenceable(8) ptr @_ZNKSt7__n486116coroutine_handleIN3gen12promise_typeEE7promiseEv(ptr noundef nonnull align 8 dereferenceable(8) %19)
  %21 = getelementptr inbounds %"struct.gen::promise_type", ptr %20, i32 0, i32 1
  %22 = load i32, ptr %21, align 4
  %23 = call i32 (ptr, ...) @printf(ptr noundef @.str.3, i32 noundef %22)
  %24 = getelementptr inbounds %struct.gen, ptr %1, i32 0, i32 0
  call void @_ZNKSt7__n486116coroutine_handleIN3gen12promise_typeEE7destroyEv(ptr noundef nonnull align 8 dereferenceable(8) %24)
  %25 = load i32, ptr @_ZL5dtors, align 4
  %26 = call i32 (ptr, ...) @printf(ptr noundef @.str.1, i32 noundef %25)
  store i32 0, ptr @_ZL5dtors, align 4
  %27 = call i32 (ptr, ...) @printf(ptr noundef @.str.4)
  %28 = call i64 @_Z5threei(i32 noundef 30)
  %29 = getelementptr inbounds %struct.gen, ptr %3, i32 0, i32 0
  %30 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %29, i32 0, i32 0
  %31 = inttoptr i64 %28 to ptr
  store ptr %31, ptr %30, align 8
  call void @llvm.memcpy.p0.p0.i64(ptr align 8 %1, ptr align 8 %3, i64 8, i1 false)
  %32 = getelementptr inbounds %struct.gen, ptr %1, i32 0, i32 0
  call void @_ZNKSt7__n486116coroutine_handleIN3gen12promise_typeEE6resumeEv(ptr noundef nonnull align 8 dereferenceable(8) %32)
  %33 = getelementptr inbounds %struct.gen, ptr %1, i32 0, i32 0
  call void @_ZNKSt7__n486116coroutine_handleIN3gen12promise_typeEE6resumeEv(ptr noundef nonnull align 8 dereferenceable(8) %33)
  %34 = getelementptr inbounds %struct.gen, ptr %1, i32 0, i32 0
  call void @_ZNKSt7__n486116coroutine_handleIN3gen12promise_typeEE6resumeEv(ptr noundef nonnull align 8 dereferenceable(8) %34)
  %35 = getelementptr inbounds %struct.gen, ptr %1, i32 0, i32 0
  %36 = call noundef i1 @_ZNKSt7__n486116coroutine_handleIN3gen12promise_typeEE4doneEv(ptr noundef nonnull align 8 dereferenceable(8) %35) #2
  %37 = zext i1 %36 to i32
  %38 = call i32 (ptr, ...) @printf(ptr noundef @.str.5, i32 noundef %37)
  %39 = getelementptr inbounds %struct.gen, ptr %1, i32 0, i32 0
  call void @_ZNKSt7__n486116coroutine_handleIN3gen12promise_typeEE7destroyEv(ptr noundef nonnull align 8 dereferenceable(8) %39)
  %40 = load i32, ptr @_ZL5dtors, align 4
  %41 = call i32 (ptr, ...) @printf(ptr noundef @.str.1, i32 noundef %40)
  ret i32 0
}

declare i32 @printf(ptr noundef, ...) #13

; Function Attrs: mustprogress noinline optnone uwtable
define linkonce_odr dso_local void @_ZNKSt7__n486116coroutine_handleIN3gen12promise_typeEE7destroyEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #7 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %3, i32 0, i32 0
  %5 = load ptr, ptr %4, align 8
  call void @llvm.coro.destroy(ptr %5)
  ret void
}

; Function Attrs: nocallback nofree nounwind willreturn memory(argmem: readwrite)
declare void @llvm.memcpy.p0.p0.i64(ptr noalias nocapture writeonly, ptr noalias nocapture readonly, i64, i1 immarg) #14

; Function Attrs: mustprogress noinline optnone uwtable
define linkonce_odr dso_local void @_ZNKSt7__n486116coroutine_handleIN3gen12promise_typeEE6resumeEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #7 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %3, i32 0, i32 0
  %5 = load ptr, ptr %4, align 8
  call void @llvm.coro.resume(ptr %5)
  ret void
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local noundef nonnull align 4 dereferenceable(8) ptr @_ZNKSt7__n486116coroutine_handleIN3gen12promise_typeEE7promiseEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #6 comdat {
  %2 = alloca ptr, align 8
  %3 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %4 = load ptr, ptr %2, align 8
  %5 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %4, i32 0, i32 0
  %6 = load ptr, ptr %5, align 8
  %7 = call ptr @llvm.coro.promise(ptr %6, i32 4, i1 false)
  store ptr %7, ptr %3, align 8
  %8 = load ptr, ptr %3, align 8
  ret ptr %8
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local noundef i1 @_ZNKSt7__n486116coroutine_handleIN3gen12promise_typeEE4doneEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %3, i32 0, i32 0
  %5 = load ptr, ptr %4, align 8
  %6 = call i1 @llvm.coro.done(ptr %5)
  ret i1 %6
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local i64 @_ZNSt7__n486116coroutine_handleIN3gen12promise_typeEE12from_promiseERS2_(ptr noundef nonnull align 4 dereferenceable(8) %0) #6 comdat {
  %2 = alloca %"struct.std::__n4861::coroutine_handle", align 8
  %3 = alloca ptr, align 8
  store ptr %0, ptr %3, align 8
  call void @_ZNSt7__n486116coroutine_handleIN3gen12promise_typeEEC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %2) #2
  %4 = load ptr, ptr %3, align 8
  %5 = call ptr @llvm.coro.promise(ptr %4, i32 4, i1 true)
  %6 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %2, i32 0, i32 0
  store ptr %5, ptr %6, align 8
  %7 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %2, i32 0, i32 0
  %8 = load ptr, ptr %7, align 8
  %9 = ptrtoint ptr %8 to i64
  ret i64 %9
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZNSt7__n486116coroutine_handleIN3gen12promise_typeEEC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %0) unnamed_addr #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %3, i32 0, i32 0
  store ptr null, ptr %4, align 8
  ret void
}

; Function Attrs: nounwind memory(none)
declare ptr @llvm.coro.promise(ptr nocapture, i32, i1) #4

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local i64 @_ZNSt7__n486116coroutine_handleIvE12from_addressEPv(ptr noundef %0) #6 comdat {
  %2 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %3 = alloca ptr, align 8
  store ptr %0, ptr %3, align 8
  call void @_ZNSt7__n486116coroutine_handleIvEC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %2) #2
  %4 = load ptr, ptr %3, align 8
  %5 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %2, i32 0, i32 0
  store ptr %4, ptr %5, align 8
  %6 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %2, i32 0, i32 0
  %7 = load ptr, ptr %6, align 8
  %8 = ptrtoint ptr %7 to i64
  ret i64 %8
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local noundef ptr @_ZNKSt7__n486116coroutine_handleIN3gen12promise_typeEE7addressEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %3, i32 0, i32 0
  %5 = load ptr, ptr %4, align 8
  ret ptr %5
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZNSt7__n486116coroutine_handleIvEC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %0) unnamed_addr #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %3, i32 0, i32 0
  store ptr null, ptr %4, align 8
  ret void
}

; Function Attrs: noinline noreturn nounwind uwtable
define linkonce_odr hidden void @__clang_call_terminate(ptr noundef %0) #15 comdat {
  %2 = call ptr @__cxa_begin_catch(ptr %0) #2
  call void @_ZSt9terminatev() #18
  unreachable
}

declare ptr @__cxa_begin_catch(ptr)

declare void @_ZSt9terminatev()

declare void @llvm.coro.destroy(ptr)

declare void @llvm.coro.resume(ptr)

; Function Attrs: nounwind memory(argmem: readwrite)
declare i1 @llvm.coro.done(ptr nocapture readonly) #16

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
attributes #14 = { nocallback nofree nounwind willreturn memory(argmem: readwrite) }
attributes #15 = { noinline noreturn nounwind uwtable "frame-pointer"="non-leaf" "no-trapping-math"="true" "stack-protector-buffer-size"="8" }
attributes #16 = { nounwind memory(argmem: readwrite) }
attributes #17 = { allocsize(0) }
attributes #18 = { noreturn nounwind }

!llvm.linker.options = !{}
!llvm.module.flags = !{!0, !1, !2, !3, !4}
!llvm.ident = !{!5}

!0 = !{i32 1, !"wchar_size", i32 4}
!1 = !{i32 8, !"PIC Level", i32 2}
!2 = !{i32 7, !"PIE Level", i32 2}
!3 = !{i32 7, !"uwtable", i32 2}
!4 = !{i32 7, !"frame-pointer", i32 1}
!5 = !{!"Debian clang version 19.1.7 (3+b1)"}
