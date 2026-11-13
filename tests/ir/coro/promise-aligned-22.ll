; clang++-22 -std=c++20 -O0 -S -emit-llvm -Xclang -disable-llvm-passes -fcoro-aligned-allocation
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
;@ dialect: 22
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
  %14 = call token @llvm.coro.id(i32 16, ptr %3, ptr null, ptr null)
  %15 = call i1 @llvm.coro.alloc(token %14)
  br i1 %15, label %16, label %20

16:                                               ; preds = %1
  %17 = call i64 @llvm.coro.size.i64()
  %18 = call i64 @llvm.coro.align.i64()
  %19 = call noundef ptr @_ZN3gen12promise_typenwEmSt11align_val_t(i64 noundef %17, i64 noundef %18)
  br label %20

20:                                               ; preds = %16, %1
  %21 = phi ptr [ null, %1 ], [ %19, %16 ]
  %22 = call ptr @llvm.coro.begin(token %14, ptr %21)
  call void @llvm.lifetime.start.p0(ptr %3) #2
  call void @_ZN3gen12promise_typeC2Ev(ptr noundef nonnull align 32 dereferenceable(8) %3) #2
  invoke void @_ZN3gen12promise_type17get_return_objectEv(ptr dead_on_unwind writable sret(%struct.gen) align 8 %0, ptr noundef nonnull align 32 dereferenceable(8) %3)
          to label %23 unwind label %29

23:                                               ; preds = %20
  call void @llvm.lifetime.start.p0(ptr %6) #2
  call void @_ZN3gen12promise_type15initial_suspendEv(ptr noundef nonnull align 32 dereferenceable(8) %3) #2
  %24 = call noundef i1 @_ZNKSt7__n486114suspend_always11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %6) #2
  br i1 %24, label %35, label %25

25:                                               ; preds = %23
  %26 = call token @llvm.coro.save(ptr null)
  call void @llvm.coro.await.suspend.void(ptr %6, ptr %22, ptr @_Z5countv.__await_suspend_wrapper__init) #2
  %27 = call i8 @llvm.coro.suspend(token %26, i1 false)
  switch i8 %27, label %76 [
    i8 0, label %35
    i8 1, label %28
  ]

28:                                               ; preds = %25
  br label %36

29:                                               ; preds = %20
  %30 = landingpad { ptr, i32 }
          cleanup
  %31 = extractvalue { ptr, i32 } %30, 0
  store ptr %31, ptr %4, align 8
  %32 = extractvalue { ptr, i32 } %30, 1
  store i32 %32, ptr %5, align 4
  call void @llvm.lifetime.end.p0(ptr %3) #2
  %33 = call ptr @llvm.coro.free(token %14, ptr %22)
  %34 = icmp ne ptr %33, null
  br i1 %34, label %77, label %80

35:                                               ; preds = %25, %23
  call void @_ZNKSt7__n486114suspend_always12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %6) #2
  br label %36

36:                                               ; preds = %35, %28
  %37 = phi i32 [ 0, %35 ], [ 2, %28 ]
  call void @llvm.lifetime.end.p0(ptr %6) #2
  switch i32 %37, label %67 [
    i32 0, label %38
  ]

38:                                               ; preds = %36
  call void @llvm.lifetime.start.p0(ptr %8) #2
  call void @_ZN3gen12promise_type11yield_valueEl(ptr noundef nonnull align 32 dereferenceable(8) %3, i64 noundef 5) #2
  %39 = call noundef i1 @_ZNKSt7__n486114suspend_always11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %8) #2
  br i1 %39, label %44, label %40

40:                                               ; preds = %38
  %41 = call token @llvm.coro.save(ptr null)
  call void @llvm.coro.await.suspend.void(ptr %8, ptr %22, ptr @_Z5countv.__await_suspend_wrapper__yield) #2
  %42 = call i8 @llvm.coro.suspend(token %41, i1 false)
  switch i8 %42, label %76 [
    i8 0, label %44
    i8 1, label %43
  ]

43:                                               ; preds = %40
  br label %45

44:                                               ; preds = %40, %38
  call void @_ZNKSt7__n486114suspend_always12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %8) #2
  br label %45

45:                                               ; preds = %44, %43
  %46 = phi i32 [ 0, %44 ], [ 2, %43 ]
  call void @llvm.lifetime.end.p0(ptr %8) #2
  switch i32 %46, label %67 [
    i32 0, label %47
  ]

47:                                               ; preds = %45
  call void @llvm.lifetime.start.p0(ptr %10) #2
  call void @_ZN3gen12promise_type11yield_valueEl(ptr noundef nonnull align 32 dereferenceable(8) %3, i64 noundef 6) #2
  %48 = call noundef i1 @_ZNKSt7__n486114suspend_always11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %10) #2
  br i1 %48, label %53, label %49

49:                                               ; preds = %47
  %50 = call token @llvm.coro.save(ptr null)
  call void @llvm.coro.await.suspend.void(ptr %10, ptr %22, ptr @_Z5countv.__await_suspend_wrapper__yield2) #2
  %51 = call i8 @llvm.coro.suspend(token %50, i1 false)
  switch i8 %51, label %76 [
    i8 0, label %53
    i8 1, label %52
  ]

52:                                               ; preds = %49
  br label %54

53:                                               ; preds = %49, %47
  call void @_ZNKSt7__n486114suspend_always12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %10) #2
  br label %54

54:                                               ; preds = %53, %52
  %55 = phi i32 [ 0, %53 ], [ 2, %52 ]
  call void @llvm.lifetime.end.p0(ptr %10) #2
  switch i32 %55, label %67 [
    i32 0, label %56
  ]

56:                                               ; preds = %54
  call void @_ZN3gen12promise_type11return_voidEv(ptr noundef nonnull align 32 dereferenceable(8) %3) #2
  br label %57

57:                                               ; preds = %56
  call void @llvm.lifetime.start.p0(ptr %12) #2
  call void @_ZN3gen12promise_type13final_suspendEv(ptr noundef nonnull align 32 dereferenceable(8) %3) #2
  %58 = call noundef i1 @_ZNKSt7__n486114suspend_always11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %12) #2
  br i1 %58, label %63, label %59

59:                                               ; preds = %57
  %60 = call token @llvm.coro.save(ptr null)
  call void @llvm.coro.await.suspend.void(ptr %12, ptr %22, ptr @_Z5countv.__await_suspend_wrapper__final) #2
  %61 = call i8 @llvm.coro.suspend(token %60, i1 true)
  switch i8 %61, label %76 [
    i8 0, label %63
    i8 1, label %62
  ]

62:                                               ; preds = %59
  br label %64

63:                                               ; preds = %59, %57
  call void @_ZNKSt7__n486114suspend_always12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %12) #2
  br label %64

64:                                               ; preds = %63, %62
  %65 = phi i32 [ 0, %63 ], [ 2, %62 ]
  call void @llvm.lifetime.end.p0(ptr %12) #2
  switch i32 %65, label %67 [
    i32 0, label %66
  ]

66:                                               ; preds = %64
  br label %67

67:                                               ; preds = %66, %64, %54, %45, %36
  %68 = phi i32 [ %37, %36 ], [ %46, %45 ], [ %55, %54 ], [ %65, %64 ], [ 0, %66 ]
  call void @llvm.lifetime.end.p0(ptr %3) #2
  %69 = call ptr @llvm.coro.free(token %14, ptr %22)
  %70 = icmp ne ptr %69, null
  br i1 %70, label %71, label %74

71:                                               ; preds = %67
  %72 = call i64 @llvm.coro.size.i64()
  %73 = call i64 @llvm.coro.align.i64()
  call void @_ZN3gen12promise_typedlEPvmSt11align_val_t(ptr noundef %69, i64 noundef %72, i64 noundef %73) #2
  br label %74

74:                                               ; preds = %67, %71
  switch i32 %68, label %86 [
    i32 0, label %75
    i32 2, label %76
  ]

75:                                               ; preds = %74
  br label %76

76:                                               ; preds = %75, %74, %59, %49, %40, %25
  call void @llvm.coro.end(ptr null, i1 false, token none)
  ret void

77:                                               ; preds = %29
  %78 = call i64 @llvm.coro.size.i64()
  %79 = call i64 @llvm.coro.align.i64()
  call void @_ZN3gen12promise_typedlEPvmSt11align_val_t(ptr noundef %33, i64 noundef %78, i64 noundef %79) #2
  br label %80

80:                                               ; preds = %29, %77
  br label %81

81:                                               ; preds = %80
  %82 = load ptr, ptr %4, align 8
  %83 = load i32, ptr %5, align 4
  %84 = insertvalue { ptr, i32 } poison, ptr %82, 0
  %85 = insertvalue { ptr, i32 } %84, i32 %83, 1
  resume { ptr, i32 } %85

86:                                               ; preds = %74
  unreachable
}

; Function Attrs: nocallback nofree nosync nounwind willreturn memory(argmem: read)
declare token @llvm.coro.id(i32, ptr readnone, ptr readonly captures(none), ptr) #1

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
declare void @llvm.lifetime.start.p0(ptr captures(none)) #5

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZN3gen12promise_typeC2Ev(ptr noundef nonnull align 32 dereferenceable(8) %0) unnamed_addr #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds nuw %"struct.gen::promise_type", ptr %3, i32 0, i32 0
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
  %6 = getelementptr inbounds nuw %struct.gen, ptr %0, i32 0, i32 0
  %7 = call i64 @_ZNSt7__n486116coroutine_handleIN3gen12promise_typeEE12from_promiseERS2_(ptr noundef nonnull align 32 dereferenceable(8) %5)
  %8 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %6, i32 0, i32 0
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
define internal void @_Z5countv.__await_suspend_wrapper__init(ptr noundef nonnull %0, ptr noundef %1) #8 {
  %3 = alloca ptr, align 8
  %4 = alloca ptr, align 8
  %5 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %6 = alloca %"struct.std::__n4861::coroutine_handle", align 8
  store ptr %0, ptr %3, align 8
  store ptr %1, ptr %4, align 8
  %7 = load ptr, ptr %3, align 8
  %8 = load ptr, ptr %4, align 8
  %9 = call i64 @_ZNSt7__n486116coroutine_handleIN3gen12promise_typeEE12from_addressEPv(ptr noundef %8) #2
  %10 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %6, i32 0, i32 0
  %11 = inttoptr i64 %9 to ptr
  store ptr %11, ptr %10, align 8
  %12 = call i64 @_ZNKSt7__n486116coroutine_handleIN3gen12promise_typeEEcvNS0_IvEEEv(ptr noundef nonnull align 8 dereferenceable(8) %6) #2
  %13 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %5, i32 0, i32 0
  %14 = inttoptr i64 %12 to ptr
  store ptr %14, ptr %13, align 8
  %15 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %5, i32 0, i32 0
  %16 = load ptr, ptr %15, align 8
  call void @_ZNKSt7__n486114suspend_always13await_suspendENS_16coroutine_handleIvEE(ptr noundef nonnull align 1 dereferenceable(1) %7, ptr %16) #2
  ret void
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZNKSt7__n486114suspend_always13await_suspendENS_16coroutine_handleIvEE(ptr noundef nonnull align 1 dereferenceable(1) %0, ptr %1) #6 comdat {
  %3 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %4 = alloca ptr, align 8
  %5 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %3, i32 0, i32 0
  store ptr %1, ptr %5, align 8
  store ptr %0, ptr %4, align 8
  %6 = load ptr, ptr %4, align 8
  ret void
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local i64 @_ZNSt7__n486116coroutine_handleIN3gen12promise_typeEE12from_addressEPv(ptr noundef %0) #6 comdat {
  %2 = alloca %"struct.std::__n4861::coroutine_handle", align 8
  %3 = alloca ptr, align 8
  store ptr %0, ptr %3, align 8
  call void @_ZNSt7__n486116coroutine_handleIN3gen12promise_typeEEC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %2) #2
  %4 = load ptr, ptr %3, align 8
  %5 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %2, i32 0, i32 0
  store ptr %4, ptr %5, align 8
  %6 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %2, i32 0, i32 0
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
  %7 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %2, i32 0, i32 0
  %8 = inttoptr i64 %6 to ptr
  store ptr %8, ptr %7, align 8
  %9 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %2, i32 0, i32 0
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
declare void @llvm.lifetime.end.p0(ptr captures(none)) #5

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZN3gen12promise_type11yield_valueEl(ptr noundef nonnull align 32 dereferenceable(8) %0, i64 noundef %1) #6 comdat {
  %3 = alloca ptr, align 8
  %4 = alloca i64, align 8
  store ptr %0, ptr %3, align 8
  store i64 %1, ptr %4, align 8
  %5 = load ptr, ptr %3, align 8
  %6 = load i64, ptr %4, align 8
  %7 = getelementptr inbounds nuw %"struct.gen::promise_type", ptr %5, i32 0, i32 0
  store i64 %6, ptr %7, align 32
  ret void
}

; Function Attrs: alwaysinline mustprogress
define internal void @_Z5countv.__await_suspend_wrapper__yield(ptr noundef nonnull %0, ptr noundef %1) #8 {
  %3 = alloca ptr, align 8
  %4 = alloca ptr, align 8
  %5 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %6 = alloca %"struct.std::__n4861::coroutine_handle", align 8
  store ptr %0, ptr %3, align 8
  store ptr %1, ptr %4, align 8
  %7 = load ptr, ptr %3, align 8
  %8 = load ptr, ptr %4, align 8
  %9 = call i64 @_ZNSt7__n486116coroutine_handleIN3gen12promise_typeEE12from_addressEPv(ptr noundef %8) #2
  %10 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %6, i32 0, i32 0
  %11 = inttoptr i64 %9 to ptr
  store ptr %11, ptr %10, align 8
  %12 = call i64 @_ZNKSt7__n486116coroutine_handleIN3gen12promise_typeEEcvNS0_IvEEEv(ptr noundef nonnull align 8 dereferenceable(8) %6) #2
  %13 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %5, i32 0, i32 0
  %14 = inttoptr i64 %12 to ptr
  store ptr %14, ptr %13, align 8
  %15 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %5, i32 0, i32 0
  %16 = load ptr, ptr %15, align 8
  call void @_ZNKSt7__n486114suspend_always13await_suspendENS_16coroutine_handleIvEE(ptr noundef nonnull align 1 dereferenceable(1) %7, ptr %16) #2
  ret void
}

; Function Attrs: alwaysinline mustprogress
define internal void @_Z5countv.__await_suspend_wrapper__yield2(ptr noundef nonnull %0, ptr noundef %1) #8 {
  %3 = alloca ptr, align 8
  %4 = alloca ptr, align 8
  %5 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %6 = alloca %"struct.std::__n4861::coroutine_handle", align 8
  store ptr %0, ptr %3, align 8
  store ptr %1, ptr %4, align 8
  %7 = load ptr, ptr %3, align 8
  %8 = load ptr, ptr %4, align 8
  %9 = call i64 @_ZNSt7__n486116coroutine_handleIN3gen12promise_typeEE12from_addressEPv(ptr noundef %8) #2
  %10 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %6, i32 0, i32 0
  %11 = inttoptr i64 %9 to ptr
  store ptr %11, ptr %10, align 8
  %12 = call i64 @_ZNKSt7__n486116coroutine_handleIN3gen12promise_typeEEcvNS0_IvEEEv(ptr noundef nonnull align 8 dereferenceable(8) %6) #2
  %13 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %5, i32 0, i32 0
  %14 = inttoptr i64 %12 to ptr
  store ptr %14, ptr %13, align 8
  %15 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %5, i32 0, i32 0
  %16 = load ptr, ptr %15, align 8
  call void @_ZNKSt7__n486114suspend_always13await_suspendENS_16coroutine_handleIvEE(ptr noundef nonnull align 1 dereferenceable(1) %7, ptr %16) #2
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
define internal void @_Z5countv.__await_suspend_wrapper__final(ptr noundef nonnull %0, ptr noundef %1) #8 {
  %3 = alloca ptr, align 8
  %4 = alloca ptr, align 8
  %5 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %6 = alloca %"struct.std::__n4861::coroutine_handle", align 8
  store ptr %0, ptr %3, align 8
  store ptr %1, ptr %4, align 8
  %7 = load ptr, ptr %3, align 8
  %8 = load ptr, ptr %4, align 8
  %9 = call i64 @_ZNSt7__n486116coroutine_handleIN3gen12promise_typeEE12from_addressEPv(ptr noundef %8) #2
  %10 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %6, i32 0, i32 0
  %11 = inttoptr i64 %9 to ptr
  store ptr %11, ptr %10, align 8
  %12 = call i64 @_ZNKSt7__n486116coroutine_handleIN3gen12promise_typeEEcvNS0_IvEEEv(ptr noundef nonnull align 8 dereferenceable(8) %6) #2
  %13 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %5, i32 0, i32 0
  %14 = inttoptr i64 %12 to ptr
  store ptr %14, ptr %13, align 8
  %15 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %5, i32 0, i32 0
  %16 = load ptr, ptr %15, align 8
  call void @_ZNKSt7__n486114suspend_always13await_suspendENS_16coroutine_handleIvEE(ptr noundef nonnull align 1 dereferenceable(1) %7, ptr %16) #2
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
declare ptr @llvm.coro.free(token, ptr readonly captures(none)) #9

; Function Attrs: nounwind
declare void @llvm.coro.end(ptr, i1, token) #2

; Function Attrs: mustprogress noinline norecurse optnone uwtable
define dso_local noundef i32 @main() #10 personality ptr @__gxx_personality_v0 {
  %1 = alloca %struct.gen, align 8
  %2 = alloca ptr, align 8
  %3 = alloca i32, align 4
  %4 = alloca ptr, align 8
  %5 = alloca %"struct.std::__n4861::coroutine_handle", align 8
  call void @_Z5countv(ptr dead_on_unwind writable sret(%struct.gen) align 8 %1)
  %6 = getelementptr inbounds nuw %struct.gen, ptr %1, i32 0, i32 0
  invoke void @_ZNKSt7__n486116coroutine_handleIN3gen12promise_typeEE6resumeEv(ptr noundef nonnull align 8 dereferenceable(8) %6)
          to label %7 unwind label %46

7:                                                ; preds = %0
  %8 = getelementptr inbounds nuw %struct.gen, ptr %1, i32 0, i32 0
  %9 = invoke noundef nonnull align 32 dereferenceable(8) ptr @_ZNKSt7__n486116coroutine_handleIN3gen12promise_typeEE7promiseEv(ptr noundef nonnull align 8 dereferenceable(8) %8)
          to label %10 unwind label %46

10:                                               ; preds = %7
  store ptr %9, ptr %4, align 8
  %11 = load ptr, ptr %4, align 8, !nonnull !6, !align !7
  %12 = ptrtoint ptr %11 to i64
  %13 = urem i64 %12, 32
  %14 = trunc i64 %13 to i32
  %15 = load ptr, ptr %4, align 8, !nonnull !6, !align !7
  %16 = getelementptr inbounds nuw %struct.gen, ptr %1, i32 0, i32 0
  %17 = call noundef ptr @_ZNKSt7__n486116coroutine_handleIN3gen12promise_typeEE7addressEv(ptr noundef nonnull align 8 dereferenceable(8) %16) #2
  %18 = ptrtoint ptr %15 to i64
  %19 = ptrtoint ptr %17 to i64
  %20 = sub i64 %18, %19
  %21 = invoke i32 (ptr, ...) @printf(ptr noundef @.str, i32 noundef %14, i64 noundef %20)
          to label %22 unwind label %46

22:                                               ; preds = %10
  %23 = load ptr, ptr %4, align 8, !nonnull !6, !align !7
  %24 = invoke i64 @_ZNSt7__n486116coroutine_handleIN3gen12promise_typeEE12from_promiseERS2_(ptr noundef nonnull align 32 dereferenceable(8) %23)
          to label %25 unwind label %46

25:                                               ; preds = %22
  %26 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %5, i32 0, i32 0
  %27 = inttoptr i64 %24 to ptr
  store ptr %27, ptr %26, align 8
  %28 = call noundef ptr @_ZNKSt7__n486116coroutine_handleIN3gen12promise_typeEE7addressEv(ptr noundef nonnull align 8 dereferenceable(8) %5) #2
  %29 = getelementptr inbounds nuw %struct.gen, ptr %1, i32 0, i32 0
  %30 = call noundef ptr @_ZNKSt7__n486116coroutine_handleIN3gen12promise_typeEE7addressEv(ptr noundef nonnull align 8 dereferenceable(8) %29) #2
  %31 = icmp eq ptr %28, %30
  %32 = zext i1 %31 to i32
  %33 = load ptr, ptr %4, align 8, !nonnull !6, !align !7
  %34 = getelementptr inbounds nuw %"struct.gen::promise_type", ptr %33, i32 0, i32 0
  %35 = load i64, ptr %34, align 32
  %36 = invoke i32 (ptr, ...) @printf(ptr noundef @.str.1, i32 noundef %32, i64 noundef %35)
          to label %37 unwind label %46

37:                                               ; preds = %25
  invoke void @_ZNKSt7__n486116coroutine_handleIN3gen12promise_typeEE6resumeEv(ptr noundef nonnull align 8 dereferenceable(8) %5)
          to label %38 unwind label %46

38:                                               ; preds = %37
  %39 = getelementptr inbounds nuw %struct.gen, ptr %1, i32 0, i32 0
  %40 = invoke noundef nonnull align 32 dereferenceable(8) ptr @_ZNKSt7__n486116coroutine_handleIN3gen12promise_typeEE7promiseEv(ptr noundef nonnull align 8 dereferenceable(8) %39)
          to label %41 unwind label %46

41:                                               ; preds = %38
  %42 = getelementptr inbounds nuw %"struct.gen::promise_type", ptr %40, i32 0, i32 0
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
  %4 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %3, i32 0, i32 0
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
  %5 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %4, i32 0, i32 0
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
  %4 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %3, i32 0, i32 0
  %5 = load ptr, ptr %4, align 8
  ret ptr %5
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local i64 @_ZNSt7__n486116coroutine_handleIN3gen12promise_typeEE12from_promiseERS2_(ptr noundef nonnull align 32 dereferenceable(8) %0) #6 comdat {
  %2 = alloca %"struct.std::__n4861::coroutine_handle", align 8
  %3 = alloca ptr, align 8
  store ptr %0, ptr %3, align 8
  call void @_ZNSt7__n486116coroutine_handleIN3gen12promise_typeEEC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %2) #2
  %4 = load ptr, ptr %3, align 8, !nonnull !6, !align !7
  %5 = call ptr @llvm.coro.promise(ptr %4, i32 32, i1 true)
  %6 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %2, i32 0, i32 0
  store ptr %5, ptr %6, align 8
  %7 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %2, i32 0, i32 0
  %8 = load ptr, ptr %7, align 8
  %9 = ptrtoint ptr %8 to i64
  ret i64 %9
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZN3genD2Ev(ptr noundef nonnull align 8 dereferenceable(8) %0) unnamed_addr #6 comdat personality ptr @__gxx_personality_v0 {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds nuw %struct.gen, ptr %3, i32 0, i32 0
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
  %4 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %3, i32 0, i32 0
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
  %5 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %2, i32 0, i32 0
  store ptr %4, ptr %5, align 8
  %6 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %2, i32 0, i32 0
  %7 = load ptr, ptr %6, align 8
  %8 = ptrtoint ptr %7 to i64
  ret i64 %8
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZNSt7__n486116coroutine_handleIvEC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %0) unnamed_addr #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %3, i32 0, i32 0
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
  %4 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %3, i32 0, i32 0
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
declare ptr @llvm.coro.promise(ptr captures(none), i32, i1) #4

declare void @llvm.coro.resume(ptr)

attributes #0 = { mustprogress noinline optnone presplitcoroutine uwtable "frame-pointer"="non-leaf-no-reserve" "no-trapping-math"="true" "stack-protector-buffer-size"="8" }
attributes #1 = { nocallback nofree nosync nounwind willreturn memory(argmem: read) }
attributes #2 = { nounwind }
attributes #3 = { mustprogress noinline optnone uwtable "frame-pointer"="non-leaf-no-reserve" "no-trapping-math"="true" "stack-protector-buffer-size"="8" }
attributes #4 = { nounwind memory(none) }
attributes #5 = { nocallback nofree nosync nounwind willreturn memory(argmem: readwrite) }
attributes #6 = { mustprogress noinline nounwind optnone uwtable "frame-pointer"="non-leaf-no-reserve" "no-trapping-math"="true" "stack-protector-buffer-size"="8" }
attributes #7 = { nomerge nounwind }
attributes #8 = { alwaysinline mustprogress "sample-profile-suffix-elision-policy"="selected" }
attributes #9 = { nounwind memory(argmem: read) }
attributes #10 = { mustprogress noinline norecurse optnone uwtable "frame-pointer"="non-leaf-no-reserve" "no-trapping-math"="true" "stack-protector-buffer-size"="8" }
attributes #11 = { "frame-pointer"="non-leaf-no-reserve" "no-trapping-math"="true" "stack-protector-buffer-size"="8" }
attributes #12 = { nounwind allocsize(1) "frame-pointer"="non-leaf-no-reserve" "no-trapping-math"="true" "stack-protector-buffer-size"="8" }
attributes #13 = { nocallback nofree nosync nounwind willreturn memory(inaccessiblemem: write) }
attributes #14 = { nounwind "frame-pointer"="non-leaf-no-reserve" "no-trapping-math"="true" "stack-protector-buffer-size"="8" }
attributes #15 = { noinline noreturn nounwind uwtable "frame-pointer"="non-leaf-no-reserve" "no-trapping-math"="true" "stack-protector-buffer-size"="8" }
attributes #16 = { nounwind allocsize(1) }
attributes #17 = { noreturn nounwind }

!llvm.module.flags = !{!0, !1, !2, !3, !4}
!llvm.ident = !{!5}

!0 = !{i32 1, !"wchar_size", i32 4}
!1 = !{i32 8, !"PIC Level", i32 2}
!2 = !{i32 7, !"PIE Level", i32 2}
!3 = !{i32 7, !"uwtable", i32 2}
!4 = !{i32 7, !"frame-pointer", i32 4}
!5 = !{!"Debian clang version 22.1.8 (1~deb13u4)"}
!6 = !{}
!7 = !{i64 32}
