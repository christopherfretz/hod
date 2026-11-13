; clang++-19 -std=c++20 -O0 -S -emit-llvm -Xclang -disable-llvm-passes -fcoro-aligned-allocation
; (aarch64-unknown-linux-gnu; the target triple, datalayout, target-cpu and
; target-features removed, so that the seed runs on every target) of:
;
; // from_promise and promise() with a promise aligned to 32, the frame
; // allocated by the aligned operator new (coro.align): the promise is at
; // alignTo(16, 32) = 32 from the frame, and aligned.
; #include <coroutine>
; #include <cstdint>
; #include <cstdio>
; #include <cstdlib>
; #include <new>
; struct gen {
;   struct alignas(32) promise_type {
;     long cur = 0;
;     static void* operator new(std::size_t n, std::align_val_t a) {
;       std::printf("new aligned %zu\n", (std::size_t)a);
;       return std::aligned_alloc((std::size_t)a, (n + (std::size_t)a - 1) / (std::size_t)a * (std::size_t)a);
;     }
;     static void operator delete(void* p, std::size_t, std::align_val_t) { std::free(p); }
;     gen get_return_object() { return gen{std::coroutine_handle<promise_type>::from_promise(*this)}; }
;     std::suspend_always initial_suspend() noexcept { return {}; }
;     std::suspend_always final_suspend() noexcept { return {}; }
;     std::suspend_always yield_value(long v) noexcept { cur = v; return {}; }
;     void return_void() noexcept {}
;     void unhandled_exception() noexcept {}
;   };
;   std::coroutine_handle<promise_type> h;
;   ~gen() { h.destroy(); }
; };
; gen count() {
;   co_yield 5;
;   co_yield 6;
; }
; int main() {
;   gen g = count();
;   g.h.resume();
;
;   auto& p = g.h.promise();
;   std::printf("promise mod 32 = %d, promise-frame = %ld\n", (int)((std::uintptr_t)&p % 32),
;               (long)((char*)&p - (char*)g.h.address()));
;   auto back = std::coroutine_handle<gen::promise_type>::from_promise(p);
;   std::printf("from_promise same = %d, cur = %ld\n", (int)(back.address() == g.h.address()), p.cur);
;   back.resume();
;   std::printf("cur = %ld\n", g.h.promise().cur);
; }
;
;@ covers: spec/ir.md §5.5 intrinsics (coroutine bodies; spec/codegen.md §7.6)
;@ input: pre-opt
;@ dialect: 19
;@ link: c++
;@ stdout: new aligned 32
;@ stdout: promise mod 32 = 0, promise-frame = 32
;@ stdout: from_promise same = 1, cur = 5
;@ stdout: cur = 6

source_filename = "promise-aligned.cpp"

%struct.gen = type { %"struct.std::__n4861::coroutine_handle" }
%"struct.std::__n4861::coroutine_handle" = type { ptr }
%"struct.gen::promise_type" = type { i64, [24 x i8] }
%"struct.std::__n4861::suspend_always" = type { i8 }
%"struct.std::__n4861::coroutine_handle.0" = type { ptr }

$_ZN3gen12promise_typenwEmSt11align_val_t = comdat any

$_ZN3gen12promise_typeC2Ev = comdat any

$_ZN3gen12promise_type17get_return_objectEv = comdat any

$_ZN3gen12promise_type15initial_suspendEv = comdat any

$_ZNKSt7__n486114suspend_always11await_readyEv = comdat any

$_ZNKSt7__n486114suspend_always13await_suspendENS_16coroutine_handleIvEE = comdat any

$_ZNSt7__n486116coroutine_handleIN3gen12promise_typeEE12from_addressEPv = comdat any

$_ZNKSt7__n486116coroutine_handleIN3gen12promise_typeEEcvNS0_IvEEEv = comdat any

$_ZNKSt7__n486114suspend_always12await_resumeEv = comdat any

$_ZN3gen12promise_type11yield_valueEl = comdat any

$_ZN3gen12promise_type11return_voidEv = comdat any

$_ZN3gen12promise_type13final_suspendEv = comdat any

$_ZN3gen12promise_typedlEPvmSt11align_val_t = comdat any

$_ZNKSt7__n486116coroutine_handleIN3gen12promise_typeEE6resumeEv = comdat any

$_ZNKSt7__n486116coroutine_handleIN3gen12promise_typeEE7promiseEv = comdat any

$_ZNKSt7__n486116coroutine_handleIN3gen12promise_typeEE7addressEv = comdat any

$_ZNSt7__n486116coroutine_handleIN3gen12promise_typeEE12from_promiseERS2_ = comdat any

$_ZN3genD2Ev = comdat any

$_ZNSt7__n486116coroutine_handleIN3gen12promise_typeEEC2Ev = comdat any

$_ZNSt7__n486116coroutine_handleIvE12from_addressEPv = comdat any

$_ZNSt7__n486116coroutine_handleIvEC2Ev = comdat any

$_ZNKSt7__n486116coroutine_handleIN3gen12promise_typeEE7destroyEv = comdat any

$__clang_call_terminate = comdat any

@.str = private unnamed_addr constant [42 x i8] c"promise mod 32 = %d, promise-frame = %ld\0A\00", align 1
@.str.1 = private unnamed_addr constant [35 x i8] c"from_promise same = %d, cur = %ld\0A\00", align 1
@.str.2 = private unnamed_addr constant [11 x i8] c"cur = %ld\0A\00", align 1
@.str.3 = private unnamed_addr constant [17 x i8] c"new aligned %zu\0A\00", align 1

; Function Attrs: mustprogress noinline optnone presplitcoroutine uwtable
define dso_local void @_Z5countv(ptr dead_on_unwind noalias writable sret(%struct.gen) align 8 %0) #0 personality ptr @__gxx_personality_v0 {
  %2 = alloca ptr, align 8
  %3 = alloca %"struct.gen::promise_type", align 32
  %4 = alloca ptr, align 8
  %5 = alloca i32, align 4
  %6 = alloca %"struct.std::__n4861::suspend_always", align 1
  %7 = alloca %"struct.std::__n4861::suspend_always", align 1
  %8 = alloca %"struct.std::__n4861::suspend_always", align 1
  %9 = alloca %"struct.std::__n4861::suspend_always", align 1
  %10 = alloca %"struct.std::__n4861::suspend_always", align 1
  %11 = alloca %"struct.std::__n4861::suspend_always", align 1
  %12 = alloca %"struct.std::__n4861::suspend_always", align 1
  %13 = alloca %"struct.std::__n4861::suspend_always", align 1
  store ptr %0, ptr %2, align 8
  %14 = bitcast ptr %3 to ptr
  %15 = call token @llvm.coro.id(i32 16, ptr %14, ptr null, ptr null)
  %16 = call i1 @llvm.coro.alloc(token %15)
  br i1 %16, label %17, label %21

17:                                               ; preds = %1
  %18 = call i64 @llvm.coro.size.i64()
  %19 = call i64 @llvm.coro.align.i64()
  %20 = call noundef ptr @_ZN3gen12promise_typenwEmSt11align_val_t(i64 noundef %18, i64 noundef %19)
  br label %21

21:                                               ; preds = %17, %1
  %22 = phi ptr [ null, %1 ], [ %20, %17 ]
  %23 = call ptr @llvm.coro.begin(token %15, ptr %22)
  call void @llvm.lifetime.start.p0(i64 32, ptr %3) #2
  call void @_ZN3gen12promise_typeC2Ev(ptr noundef nonnull align 32 dereferenceable(8) %3) #2
  invoke void @_ZN3gen12promise_type17get_return_objectEv(ptr dead_on_unwind writable sret(%struct.gen) align 8 %0, ptr noundef nonnull align 32 dereferenceable(8) %3)
          to label %24 unwind label %30

24:                                               ; preds = %21
  call void @llvm.lifetime.start.p0(i64 1, ptr %6) #2
  call void @_ZN3gen12promise_type15initial_suspendEv(ptr noundef nonnull align 32 dereferenceable(8) %3) #2
  %25 = call noundef i1 @_ZNKSt7__n486114suspend_always11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %6) #2
  br i1 %25, label %36, label %26

26:                                               ; preds = %24
  %27 = call token @llvm.coro.save(ptr null)
  call void @llvm.coro.await.suspend.void(ptr %6, ptr %23, ptr @_Z5countv.__await_suspend_wrapper__init) #2
  %28 = call i8 @llvm.coro.suspend(token %27, i1 false)
  switch i8 %28, label %77 [
    i8 0, label %36
    i8 1, label %29
  ]

29:                                               ; preds = %26
  br label %37

30:                                               ; preds = %21
  %31 = landingpad { ptr, i32 }
          cleanup
  %32 = extractvalue { ptr, i32 } %31, 0
  store ptr %32, ptr %4, align 8
  %33 = extractvalue { ptr, i32 } %31, 1
  store i32 %33, ptr %5, align 4
  call void @llvm.lifetime.end.p0(i64 32, ptr %3) #2
  %34 = call ptr @llvm.coro.free(token %15, ptr %23)
  %35 = icmp ne ptr %34, null
  br i1 %35, label %79, label %82

36:                                               ; preds = %26, %24
  call void @_ZNKSt7__n486114suspend_always12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %6) #2
  br label %37

37:                                               ; preds = %36, %29
  %38 = phi i32 [ 0, %36 ], [ 2, %29 ]
  call void @llvm.lifetime.end.p0(i64 1, ptr %6) #2
  switch i32 %38, label %68 [
    i32 0, label %39
  ]

39:                                               ; preds = %37
  call void @llvm.lifetime.start.p0(i64 1, ptr %8) #2
  call void @_ZN3gen12promise_type11yield_valueEl(ptr noundef nonnull align 32 dereferenceable(8) %3, i64 noundef 5) #2
  %40 = call noundef i1 @_ZNKSt7__n486114suspend_always11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %8) #2
  br i1 %40, label %45, label %41

41:                                               ; preds = %39
  %42 = call token @llvm.coro.save(ptr null)
  call void @llvm.coro.await.suspend.void(ptr %8, ptr %23, ptr @_Z5countv.__await_suspend_wrapper__yield) #2
  %43 = call i8 @llvm.coro.suspend(token %42, i1 false)
  switch i8 %43, label %77 [
    i8 0, label %45
    i8 1, label %44
  ]

44:                                               ; preds = %41
  br label %46

45:                                               ; preds = %41, %39
  call void @_ZNKSt7__n486114suspend_always12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %8) #2
  br label %46

46:                                               ; preds = %45, %44
  %47 = phi i32 [ 0, %45 ], [ 2, %44 ]
  call void @llvm.lifetime.end.p0(i64 1, ptr %8) #2
  switch i32 %47, label %68 [
    i32 0, label %48
  ]

48:                                               ; preds = %46
  call void @llvm.lifetime.start.p0(i64 1, ptr %10) #2
  call void @_ZN3gen12promise_type11yield_valueEl(ptr noundef nonnull align 32 dereferenceable(8) %3, i64 noundef 6) #2
  %49 = call noundef i1 @_ZNKSt7__n486114suspend_always11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %10) #2
  br i1 %49, label %54, label %50

50:                                               ; preds = %48
  %51 = call token @llvm.coro.save(ptr null)
  call void @llvm.coro.await.suspend.void(ptr %10, ptr %23, ptr @_Z5countv.__await_suspend_wrapper__yield2) #2
  %52 = call i8 @llvm.coro.suspend(token %51, i1 false)
  switch i8 %52, label %77 [
    i8 0, label %54
    i8 1, label %53
  ]

53:                                               ; preds = %50
  br label %55

54:                                               ; preds = %50, %48
  call void @_ZNKSt7__n486114suspend_always12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %10) #2
  br label %55

55:                                               ; preds = %54, %53
  %56 = phi i32 [ 0, %54 ], [ 2, %53 ]
  call void @llvm.lifetime.end.p0(i64 1, ptr %10) #2
  switch i32 %56, label %68 [
    i32 0, label %57
  ]

57:                                               ; preds = %55
  call void @_ZN3gen12promise_type11return_voidEv(ptr noundef nonnull align 32 dereferenceable(8) %3) #2
  br label %58

58:                                               ; preds = %57
  call void @llvm.lifetime.start.p0(i64 1, ptr %12) #2
  call void @_ZN3gen12promise_type13final_suspendEv(ptr noundef nonnull align 32 dereferenceable(8) %3) #2
  %59 = call noundef i1 @_ZNKSt7__n486114suspend_always11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %12) #2
  br i1 %59, label %64, label %60

60:                                               ; preds = %58
  %61 = call token @llvm.coro.save(ptr null)
  call void @llvm.coro.await.suspend.void(ptr %12, ptr %23, ptr @_Z5countv.__await_suspend_wrapper__final) #2
  %62 = call i8 @llvm.coro.suspend(token %61, i1 true)
  switch i8 %62, label %77 [
    i8 0, label %64
    i8 1, label %63
  ]

63:                                               ; preds = %60
  br label %65

64:                                               ; preds = %60, %58
  call void @_ZNKSt7__n486114suspend_always12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %12) #2
  br label %65

65:                                               ; preds = %64, %63
  %66 = phi i32 [ 0, %64 ], [ 2, %63 ]
  call void @llvm.lifetime.end.p0(i64 1, ptr %12) #2
  switch i32 %66, label %68 [
    i32 0, label %67
  ]

67:                                               ; preds = %65
  br label %68

68:                                               ; preds = %67, %65, %55, %46, %37
  %69 = phi i32 [ %38, %37 ], [ %47, %46 ], [ %56, %55 ], [ %66, %65 ], [ 0, %67 ]
  call void @llvm.lifetime.end.p0(i64 32, ptr %3) #2
  %70 = call ptr @llvm.coro.free(token %15, ptr %23)
  %71 = icmp ne ptr %70, null
  br i1 %71, label %72, label %75

72:                                               ; preds = %68
  %73 = call i64 @llvm.coro.size.i64()
  %74 = call i64 @llvm.coro.align.i64()
  call void @_ZN3gen12promise_typedlEPvmSt11align_val_t(ptr noundef %70, i64 noundef %73, i64 noundef %74) #2
  br label %75

75:                                               ; preds = %68, %72
  switch i32 %69, label %88 [
    i32 0, label %76
    i32 2, label %77
  ]

76:                                               ; preds = %75
  br label %77

77:                                               ; preds = %76, %75, %60, %50, %41, %26
  %78 = call i1 @llvm.coro.end(ptr null, i1 false, token none)
  ret void

79:                                               ; preds = %30
  %80 = call i64 @llvm.coro.size.i64()
  %81 = call i64 @llvm.coro.align.i64()
  call void @_ZN3gen12promise_typedlEPvmSt11align_val_t(ptr noundef %34, i64 noundef %80, i64 noundef %81) #2
  br label %82

82:                                               ; preds = %30, %79
  br label %83

83:                                               ; preds = %82
  %84 = load ptr, ptr %4, align 8
  %85 = load i32, ptr %5, align 4
  %86 = insertvalue { ptr, i32 } poison, ptr %84, 0
  %87 = insertvalue { ptr, i32 } %86, i32 %85, 1
  resume { ptr, i32 } %87

88:                                               ; preds = %75
  unreachable
}

; Function Attrs: nocallback nofree nosync nounwind willreturn memory(argmem: read)
declare token @llvm.coro.id(i32, ptr readnone, ptr nocapture readonly, ptr) #1

; Function Attrs: nounwind
declare i1 @llvm.coro.alloc(token) #2

; Function Attrs: mustprogress noinline optnone uwtable
define linkonce_odr dso_local noundef ptr @_ZN3gen12promise_typenwEmSt11align_val_t(i64 noundef %0, i64 noundef %1) #3 comdat {
  %3 = alloca i64, align 8
  %4 = alloca i64, align 8
  store i64 %0, ptr %3, align 8
  store i64 %1, ptr %4, align 8
  %5 = load i64, ptr %4, align 8
  %6 = call i32 (ptr, ...) @printf(ptr noundef @.str.3, i64 noundef %5)
  %7 = load i64, ptr %4, align 8
  %8 = load i64, ptr %3, align 8
  %9 = load i64, ptr %4, align 8
  %10 = add i64 %8, %9
  %11 = sub i64 %10, 1
  %12 = load i64, ptr %4, align 8
  %13 = udiv i64 %11, %12
  %14 = load i64, ptr %4, align 8
  %15 = mul i64 %13, %14
  %16 = call noalias ptr @aligned_alloc(i64 noundef %7, i64 noundef %15) #16
  call void @llvm.assume(i1 true) [ "align"(ptr %16, i64 %7) ]
  ret ptr %16
}

; Function Attrs: nounwind memory(none)
declare i64 @llvm.coro.size.i64() #4

; Function Attrs: nounwind memory(none)
declare i64 @llvm.coro.align.i64() #4

; Function Attrs: nounwind
declare ptr @llvm.coro.begin(token, ptr writeonly) #2

; Function Attrs: nocallback nofree nosync nounwind willreturn memory(argmem: readwrite)
declare void @llvm.lifetime.start.p0(i64 immarg, ptr nocapture) #5

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZN3gen12promise_typeC2Ev(ptr noundef nonnull align 32 dereferenceable(8) %0) unnamed_addr #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds %"struct.gen::promise_type", ptr %3, i32 0, i32 0
  store i64 0, ptr %4, align 32
  ret void
}

; Function Attrs: mustprogress noinline optnone uwtable
define linkonce_odr dso_local void @_ZN3gen12promise_type17get_return_objectEv(ptr dead_on_unwind noalias writable sret(%struct.gen) align 8 %0, ptr noundef nonnull align 32 dereferenceable(8) %1) #3 comdat {
  %3 = alloca ptr, align 8
  %4 = alloca ptr, align 8
  store ptr %0, ptr %3, align 8
  store ptr %1, ptr %4, align 8
  %5 = load ptr, ptr %4, align 8
  %6 = getelementptr inbounds %struct.gen, ptr %0, i32 0, i32 0
  %7 = call i64 @_ZNSt7__n486116coroutine_handleIN3gen12promise_typeEE12from_promiseERS2_(ptr noundef nonnull align 32 dereferenceable(8) %5)
  %8 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %6, i32 0, i32 0
  %9 = inttoptr i64 %7 to ptr
  store ptr %9, ptr %8, align 8
  ret void
}

declare i32 @__gxx_personality_v0(...)

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZN3gen12promise_type15initial_suspendEv(ptr noundef nonnull align 32 dereferenceable(8) %0) #6 comdat {
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
declare token @llvm.coro.save(ptr) #7

; Function Attrs: alwaysinline mustprogress
define private void @_Z5countv.__await_suspend_wrapper__init(ptr noundef nonnull %0, ptr noundef %1) #8 {
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
define linkonce_odr dso_local void @_ZN3gen12promise_type11yield_valueEl(ptr noundef nonnull align 32 dereferenceable(8) %0, i64 noundef %1) #6 comdat {
  %3 = alloca ptr, align 8
  %4 = alloca i64, align 8
  store ptr %0, ptr %3, align 8
  store i64 %1, ptr %4, align 8
  %5 = load ptr, ptr %3, align 8
  %6 = load i64, ptr %4, align 8
  %7 = getelementptr inbounds %"struct.gen::promise_type", ptr %5, i32 0, i32 0
  store i64 %6, ptr %7, align 32
  ret void
}

; Function Attrs: alwaysinline mustprogress
define private void @_Z5countv.__await_suspend_wrapper__yield(ptr noundef nonnull %0, ptr noundef %1) #8 {
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
define private void @_Z5countv.__await_suspend_wrapper__yield2(ptr noundef nonnull %0, ptr noundef %1) #8 {
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
define linkonce_odr dso_local void @_ZN3gen12promise_type11return_voidEv(ptr noundef nonnull align 32 dereferenceable(8) %0) #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  ret void
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZN3gen12promise_type13final_suspendEv(ptr noundef nonnull align 32 dereferenceable(8) %0) #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  ret void
}

; Function Attrs: alwaysinline mustprogress
define private void @_Z5countv.__await_suspend_wrapper__final(ptr noundef nonnull %0, ptr noundef %1) #8 {
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
define linkonce_odr dso_local void @_ZN3gen12promise_typedlEPvmSt11align_val_t(ptr noundef %0, i64 noundef %1, i64 noundef %2) #6 comdat {
  %4 = alloca ptr, align 8
  %5 = alloca i64, align 8
  %6 = alloca i64, align 8
  store ptr %0, ptr %4, align 8
  store i64 %1, ptr %5, align 8
  store i64 %2, ptr %6, align 8
  %7 = load ptr, ptr %4, align 8
  call void @free(ptr noundef %7) #2
  ret void
}

; Function Attrs: nounwind memory(argmem: read)
declare ptr @llvm.coro.free(token, ptr nocapture readonly) #9

; Function Attrs: nounwind
declare i1 @llvm.coro.end(ptr, i1, token) #2

; Function Attrs: mustprogress noinline norecurse optnone uwtable
define dso_local noundef i32 @main() #10 personality ptr @__gxx_personality_v0 {
  %1 = alloca %struct.gen, align 8
  %2 = alloca ptr, align 8
  %3 = alloca i32, align 4
  %4 = alloca ptr, align 8
  %5 = alloca %"struct.std::__n4861::coroutine_handle", align 8
  call void @_Z5countv(ptr dead_on_unwind writable sret(%struct.gen) align 8 %1)
  %6 = getelementptr inbounds %struct.gen, ptr %1, i32 0, i32 0
  invoke void @_ZNKSt7__n486116coroutine_handleIN3gen12promise_typeEE6resumeEv(ptr noundef nonnull align 8 dereferenceable(8) %6)
          to label %7 unwind label %46

7:                                                ; preds = %0
  %8 = getelementptr inbounds %struct.gen, ptr %1, i32 0, i32 0
  %9 = invoke noundef nonnull align 32 dereferenceable(8) ptr @_ZNKSt7__n486116coroutine_handleIN3gen12promise_typeEE7promiseEv(ptr noundef nonnull align 8 dereferenceable(8) %8)
          to label %10 unwind label %46

10:                                               ; preds = %7
  store ptr %9, ptr %4, align 8
  %11 = load ptr, ptr %4, align 8
  %12 = ptrtoint ptr %11 to i64
  %13 = urem i64 %12, 32
  %14 = trunc i64 %13 to i32
  %15 = load ptr, ptr %4, align 8
  %16 = getelementptr inbounds %struct.gen, ptr %1, i32 0, i32 0
  %17 = call noundef ptr @_ZNKSt7__n486116coroutine_handleIN3gen12promise_typeEE7addressEv(ptr noundef nonnull align 8 dereferenceable(8) %16) #2
  %18 = ptrtoint ptr %15 to i64
  %19 = ptrtoint ptr %17 to i64
  %20 = sub i64 %18, %19
  %21 = invoke i32 (ptr, ...) @printf(ptr noundef @.str, i32 noundef %14, i64 noundef %20)
          to label %22 unwind label %46

22:                                               ; preds = %10
  %23 = load ptr, ptr %4, align 8
  %24 = invoke i64 @_ZNSt7__n486116coroutine_handleIN3gen12promise_typeEE12from_promiseERS2_(ptr noundef nonnull align 32 dereferenceable(8) %23)
          to label %25 unwind label %46

25:                                               ; preds = %22
  %26 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %5, i32 0, i32 0
  %27 = inttoptr i64 %24 to ptr
  store ptr %27, ptr %26, align 8
  %28 = call noundef ptr @_ZNKSt7__n486116coroutine_handleIN3gen12promise_typeEE7addressEv(ptr noundef nonnull align 8 dereferenceable(8) %5) #2
  %29 = getelementptr inbounds %struct.gen, ptr %1, i32 0, i32 0
  %30 = call noundef ptr @_ZNKSt7__n486116coroutine_handleIN3gen12promise_typeEE7addressEv(ptr noundef nonnull align 8 dereferenceable(8) %29) #2
  %31 = icmp eq ptr %28, %30
  %32 = zext i1 %31 to i32
  %33 = load ptr, ptr %4, align 8
  %34 = getelementptr inbounds %"struct.gen::promise_type", ptr %33, i32 0, i32 0
  %35 = load i64, ptr %34, align 32
  %36 = invoke i32 (ptr, ...) @printf(ptr noundef @.str.1, i32 noundef %32, i64 noundef %35)
          to label %37 unwind label %46

37:                                               ; preds = %25
  invoke void @_ZNKSt7__n486116coroutine_handleIN3gen12promise_typeEE6resumeEv(ptr noundef nonnull align 8 dereferenceable(8) %5)
          to label %38 unwind label %46

38:                                               ; preds = %37
  %39 = getelementptr inbounds %struct.gen, ptr %1, i32 0, i32 0
  %40 = invoke noundef nonnull align 32 dereferenceable(8) ptr @_ZNKSt7__n486116coroutine_handleIN3gen12promise_typeEE7promiseEv(ptr noundef nonnull align 8 dereferenceable(8) %39)
          to label %41 unwind label %46

41:                                               ; preds = %38
  %42 = getelementptr inbounds %"struct.gen::promise_type", ptr %40, i32 0, i32 0
  %43 = load i64, ptr %42, align 32
  %44 = invoke i32 (ptr, ...) @printf(ptr noundef @.str.2, i64 noundef %43)
          to label %45 unwind label %46

45:                                               ; preds = %41
  call void @_ZN3genD2Ev(ptr noundef nonnull align 8 dereferenceable(8) %1) #2
  ret i32 0

46:                                               ; preds = %41, %38, %37, %25, %22, %10, %7, %0
  %47 = landingpad { ptr, i32 }
          cleanup
  %48 = extractvalue { ptr, i32 } %47, 0
  store ptr %48, ptr %2, align 8
  %49 = extractvalue { ptr, i32 } %47, 1
  store i32 %49, ptr %3, align 4
  call void @_ZN3genD2Ev(ptr noundef nonnull align 8 dereferenceable(8) %1) #2
  br label %50

50:                                               ; preds = %46
  %51 = load ptr, ptr %2, align 8
  %52 = load i32, ptr %3, align 4
  %53 = insertvalue { ptr, i32 } poison, ptr %51, 0
  %54 = insertvalue { ptr, i32 } %53, i32 %52, 1
  resume { ptr, i32 } %54
}

; Function Attrs: mustprogress noinline optnone uwtable
define linkonce_odr dso_local void @_ZNKSt7__n486116coroutine_handleIN3gen12promise_typeEE6resumeEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #3 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %3, i32 0, i32 0
  %5 = load ptr, ptr %4, align 8
  call void @llvm.coro.resume(ptr %5)
  ret void
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local noundef nonnull align 32 dereferenceable(8) ptr @_ZNKSt7__n486116coroutine_handleIN3gen12promise_typeEE7promiseEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #6 comdat {
  %2 = alloca ptr, align 8
  %3 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %4 = load ptr, ptr %2, align 8
  %5 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %4, i32 0, i32 0
  %6 = load ptr, ptr %5, align 8
  %7 = call ptr @llvm.coro.promise(ptr %6, i32 32, i1 false)
  store ptr %7, ptr %3, align 8
  %8 = load ptr, ptr %3, align 8
  ret ptr %8
}

declare i32 @printf(ptr noundef, ...) #11

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
define linkonce_odr dso_local i64 @_ZNSt7__n486116coroutine_handleIN3gen12promise_typeEE12from_promiseERS2_(ptr noundef nonnull align 32 dereferenceable(8) %0) #6 comdat {
  %2 = alloca %"struct.std::__n4861::coroutine_handle", align 8
  %3 = alloca ptr, align 8
  store ptr %0, ptr %3, align 8
  call void @_ZNSt7__n486116coroutine_handleIN3gen12promise_typeEEC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %2) #2
  %4 = load ptr, ptr %3, align 8
  %5 = call ptr @llvm.coro.promise(ptr %4, i32 32, i1 true)
  %6 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %2, i32 0, i32 0
  store ptr %5, ptr %6, align 8
  %7 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %2, i32 0, i32 0
  %8 = load ptr, ptr %7, align 8
  %9 = ptrtoint ptr %8 to i64
  ret i64 %9
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZN3genD2Ev(ptr noundef nonnull align 8 dereferenceable(8) %0) unnamed_addr #6 comdat personality ptr @__gxx_personality_v0 {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds %struct.gen, ptr %3, i32 0, i32 0
  invoke void @_ZNKSt7__n486116coroutine_handleIN3gen12promise_typeEE7destroyEv(ptr noundef nonnull align 8 dereferenceable(8) %4)
          to label %5 unwind label %6

5:                                                ; preds = %1
  ret void

6:                                                ; preds = %1
  %7 = landingpad { ptr, i32 }
          catch ptr null
  %8 = extractvalue { ptr, i32 } %7, 0
  call void @__clang_call_terminate(ptr %8) #17
  unreachable
}

; Function Attrs: nounwind allocsize(1)
declare noalias ptr @aligned_alloc(i64 noundef, i64 noundef) #12

; Function Attrs: nocallback nofree nosync nounwind willreturn memory(inaccessiblemem: write)
declare void @llvm.assume(i1 noundef) #13

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZNSt7__n486116coroutine_handleIN3gen12promise_typeEEC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %0) unnamed_addr #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %3, i32 0, i32 0
  store ptr null, ptr %4, align 8
  ret void
}

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
define linkonce_odr dso_local void @_ZNSt7__n486116coroutine_handleIvEC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %0) unnamed_addr #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %3, i32 0, i32 0
  store ptr null, ptr %4, align 8
  ret void
}

; Function Attrs: nounwind
declare void @free(ptr noundef) #14

; Function Attrs: mustprogress noinline optnone uwtable
define linkonce_odr dso_local void @_ZNKSt7__n486116coroutine_handleIN3gen12promise_typeEE7destroyEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #3 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %3, i32 0, i32 0
  %5 = load ptr, ptr %4, align 8
  call void @llvm.coro.destroy(ptr %5)
  ret void
}

; Function Attrs: noinline noreturn nounwind uwtable
define linkonce_odr hidden void @__clang_call_terminate(ptr noundef %0) #15 comdat {
  %2 = call ptr @__cxa_begin_catch(ptr %0) #2
  call void @_ZSt9terminatev() #17
  unreachable
}

declare ptr @__cxa_begin_catch(ptr)

declare void @_ZSt9terminatev()

declare void @llvm.coro.destroy(ptr)

; Function Attrs: nounwind memory(none)
declare ptr @llvm.coro.promise(ptr nocapture, i32, i1) #4

declare void @llvm.coro.resume(ptr)

attributes #0 = { mustprogress noinline optnone presplitcoroutine uwtable "frame-pointer"="non-leaf" "no-trapping-math"="true" "stack-protector-buffer-size"="8" }
attributes #1 = { nocallback nofree nosync nounwind willreturn memory(argmem: read) }
attributes #2 = { nounwind }
attributes #3 = { mustprogress noinline optnone uwtable "frame-pointer"="non-leaf" "no-trapping-math"="true" "stack-protector-buffer-size"="8" }
attributes #4 = { nounwind memory(none) }
attributes #5 = { nocallback nofree nosync nounwind willreturn memory(argmem: readwrite) }
attributes #6 = { mustprogress noinline nounwind optnone uwtable "frame-pointer"="non-leaf" "no-trapping-math"="true" "stack-protector-buffer-size"="8" }
attributes #7 = { nomerge nounwind }
attributes #8 = { alwaysinline mustprogress }
attributes #9 = { nounwind memory(argmem: read) }
attributes #10 = { mustprogress noinline norecurse optnone uwtable "frame-pointer"="non-leaf" "no-trapping-math"="true" "stack-protector-buffer-size"="8" }
attributes #11 = { "frame-pointer"="non-leaf" "no-trapping-math"="true" "stack-protector-buffer-size"="8" }
attributes #12 = { nounwind allocsize(1) "frame-pointer"="non-leaf" "no-trapping-math"="true" "stack-protector-buffer-size"="8" }
attributes #13 = { nocallback nofree nosync nounwind willreturn memory(inaccessiblemem: write) }
attributes #14 = { nounwind "frame-pointer"="non-leaf" "no-trapping-math"="true" "stack-protector-buffer-size"="8" }
attributes #15 = { noinline noreturn nounwind uwtable "frame-pointer"="non-leaf" "no-trapping-math"="true" "stack-protector-buffer-size"="8" }
attributes #16 = { nounwind allocsize(1) }
attributes #17 = { noreturn nounwind }

!llvm.linker.options = !{}
!llvm.module.flags = !{!0, !1, !2, !3, !4}
!llvm.ident = !{!5}

!0 = !{i32 1, !"wchar_size", i32 4}
!1 = !{i32 8, !"PIC Level", i32 2}
!2 = !{i32 7, !"PIE Level", i32 2}
!3 = !{i32 7, !"uwtable", i32 2}
!4 = !{i32 7, !"frame-pointer", i32 1}
!5 = !{!"Debian clang version 19.1.7 (3+b1)"}
