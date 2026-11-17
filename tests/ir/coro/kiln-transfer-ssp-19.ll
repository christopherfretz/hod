; kiln++ --llvm-dir=<LLVM 19> -std=c++20 -fclang-compat=19 -fstack-protector-strong -fkiln-backend=hod: the module kiln gives hod
; (aarch64-unknown-linux-gnu; the target triple, datalayout, target-cpu and
; target-features removed, so that the seed runs on every target) of:
;
; // Tasks with symmetric transfer (the awaited task's final awaiter returns
; // the awaiting handle), a coroutine lambda, and an exception carried out
; // of a coroutine to its awaiter.
; #include <coroutine>
; #include <cstdio>
; #include <exception>
; struct task {
;   struct promise_type {
;     int v = 0;
;     std::exception_ptr e;
;     std::coroutine_handle<> next;
;     task get_return_object() { return task{std::coroutine_handle<promise_type>::from_promise(*this)}; }
;     std::suspend_always initial_suspend() noexcept { return {}; }
;     struct fin {
;       bool await_ready() noexcept { return false; }
;       std::coroutine_handle<> await_suspend(std::coroutine_handle<promise_type> h) noexcept {
;         return h.promise().next ? h.promise().next : std::noop_coroutine();
;       }
;       void await_resume() noexcept {}
;     };
;     fin final_suspend() noexcept { return {}; }
;     void return_value(int x) noexcept { v = x; }
;     void unhandled_exception() noexcept { e = std::current_exception(); }
;   };
;   std::coroutine_handle<promise_type> h;
;   ~task() { if (h) h.destroy(); }
;   bool await_ready() noexcept { return false; }
;   std::coroutine_handle<> await_suspend(std::coroutine_handle<> a) noexcept { h.promise().next = a; return h; }
;   int await_resume() { if (h.promise().e) std::rethrow_exception(h.promise().e); return h.promise().v; }
; };
; task leaf(int x) {
;   if (x == 3) throw x;
;   co_return x * 2;
; }
; task sum(int n) {
;   int s = 0;
;   char tag[16];
;   std::snprintf(tag, sizeof tag, "n=%d", n);
;   for (int i = 0; i < n; ++i) {
;     try {
;       s += co_await leaf(i);
;     } catch (int c) {
;       std::printf("caught %d %s\n", c, tag);
;     }
;   }
;   auto twice = [](int v) -> task { co_return v + v; };
;   s += co_await twice(s);
;   co_return s;
; }
; int main() {
;   task t = sum(5);
;   t.h.resume();
;   std::printf("sum %d\n", t.h.promise().v);
;   return 0;
; }
;
; WP-350: symmetric transfer, a coroutine lambda and an exception, stack-protected, before opt.
;@ covers: spec/ir.md §5.5 intrinsics (coroutine bodies; spec/codegen.md §7.6)
;@ kept-off(i686): lp64-abi
;@ input: pre-opt
;@ dialect: 19
;@ link: c++
;@ stdout: caught 3 n=5
;@ stdout: sum 42
; ModuleID = 'transfer.cpp'
source_filename = "transfer.cpp"

%class.anon = type { i8 }
%struct.task = type { %"struct.std::__n4861::coroutine_handle" }
%"struct.std::__n4861::coroutine_handle" = type { ptr }
%"struct.task::promise_type" = type { i32, %"class.std::__exception_ptr::exception_ptr", %"struct.std::__n4861::coroutine_handle.0" }
%"class.std::__exception_ptr::exception_ptr" = type { ptr }
%"struct.std::__n4861::coroutine_handle.0" = type { ptr }
%"struct.std::__n4861::suspend_always" = type { i8 }
%"struct.task::promise_type::fin" = type { i8 }
%"struct.std::__n4861::coroutine_handle.1" = type { ptr }

$_ZNSt15__exception_ptr13exception_ptrC2Ev = comdat any

$_ZNSt7__n486116coroutine_handleIvEC2Ev = comdat any

$_ZN4task12promise_type17get_return_objectEv = comdat any

$_ZN4task12promise_type15initial_suspendEv = comdat any

$_ZNKSt7__n486114suspend_always11await_readyEv = comdat any

$_ZNKSt7__n486114suspend_always13await_suspendENS_16coroutine_handleIvEE = comdat any

$_ZNSt7__n486116coroutine_handleIN4task12promise_typeEE12from_addressEPv = comdat any

$_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEEcvNS0_IvEEEv = comdat any

$_ZNKSt7__n486114suspend_always12await_resumeEv = comdat any

$_ZN4task12promise_type12return_valueEi = comdat any

$_ZN4task12promise_type19unhandled_exceptionEv = comdat any

$_ZN4task12promise_type13final_suspendEv = comdat any

$_ZN4task12promise_type3fin11await_readyEv = comdat any

$_ZN4task12promise_type3fin13await_suspendENSt7__n486116coroutine_handleIS0_EE = comdat any

$_ZNKSt7__n486116coroutine_handleIvE7addressEv = comdat any

$_ZN4task12promise_type3fin12await_resumeEv = comdat any

$_ZN4task12promise_typeD2Ev = comdat any

$_ZN4task11await_readyEv = comdat any

$_ZN4task13await_suspendENSt7__n486116coroutine_handleIvEE = comdat any

$_ZN4task12await_resumeEv = comdat any

$_ZN4taskD2Ev = comdat any

$_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEE6resumeEv = comdat any

$_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEE7promiseEv = comdat any

$_ZNSt7__n486116coroutine_handleIN4task12promise_typeEE12from_promiseERS2_ = comdat any

$_ZNSt7__n486116coroutine_handleIN4task12promise_typeEEC2Ev = comdat any

$_ZNSt7__n486116coroutine_handleIvE12from_addressEPv = comdat any

$_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEE7addressEv = comdat any

$_ZNSt15__exception_ptr13exception_ptraSEOS0_ = comdat any

$_ZNSt15__exception_ptr13exception_ptrD2Ev = comdat any

$_ZNSt15__exception_ptr13exception_ptrC2EOS0_ = comdat any

$_ZNSt15__exception_ptr13exception_ptr4swapERS0_ = comdat any

$__clang_call_terminate = comdat any

$_ZNKSt7__n486116coroutine_handleIvEcvbEv = comdat any

$_ZNSt7__n486114noop_coroutineEv = comdat any

$_ZNKSt7__n486116coroutine_handleINS_22noop_coroutine_promiseEEcvNS0_IvEEEv = comdat any

$_ZNSt7__n486116coroutine_handleINS_22noop_coroutine_promiseEEC2Ev = comdat any

$_ZNSt7__n486116coroutine_handleINS_22noop_coroutine_promiseEE7__frame22__dummy_resume_destroyEv = comdat any

$_ZNKSt7__n486116coroutine_handleINS_22noop_coroutine_promiseEE7addressEv = comdat any

$_ZNKSt15__exception_ptr13exception_ptrcvbEv = comdat any

$_ZNSt15__exception_ptr13exception_ptrC2ERKS0_ = comdat any

$_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEEcvbEv = comdat any

$_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEE7destroyEv = comdat any

$_ZNSt7__n486116coroutine_handleINS_22noop_coroutine_promiseEE5_S_frE = comdat any

@_ZTIi = external constant ptr
@.str = private unnamed_addr constant [5 x i8] c"n=%d\00", align 1
@.str.1 = private unnamed_addr constant [14 x i8] c"caught %d %s\0A\00", align 1
@__const._Z3sumi.twice = private unnamed_addr constant %class.anon undef, align 1
@.str.2 = private unnamed_addr constant [8 x i8] c"sum %d\0A\00", align 1
@_ZNSt7__n486116coroutine_handleINS_22noop_coroutine_promiseEE5_S_frE = linkonce_odr dso_local global { ptr, ptr, [8 x i8] } { ptr @_ZNSt7__n486116coroutine_handleINS_22noop_coroutine_promiseEE7__frame22__dummy_resume_destroyEv, ptr @_ZNSt7__n486116coroutine_handleINS_22noop_coroutine_promiseEE7__frame22__dummy_resume_destroyEv, [8 x i8] undef }, comdat, align 8

define dso_local void @_Z4leafi(ptr dead_on_unwind noalias writable sret(%struct.task) align 8 %0, i32 noundef %1) #0 personality ptr @__gxx_personality_v0 {
  %3 = alloca ptr, align 8
  %4 = alloca i32, align 4, !coro.outside.frame !6
  %5 = alloca i32, align 4
  %6 = alloca %"struct.task::promise_type", align 8
  %7 = alloca ptr, align 8
  %8 = alloca i32, align 4
  %9 = alloca %"struct.std::__n4861::suspend_always", align 1
  %10 = alloca %"struct.std::__n4861::suspend_always", align 1
  %11 = alloca %"struct.task::promise_type::fin", align 1
  %12 = alloca %"struct.task::promise_type::fin", align 1
  store ptr %0, ptr %3, align 8
  store i32 %1, ptr %4, align 4
  %13 = bitcast ptr %6 to ptr
  %14 = call token @llvm.coro.id(i32 16, ptr %13, ptr null, ptr null)
  %15 = call i1 @llvm.coro.alloc(token %14)
  br i1 %15, label %16, label %19

16:
  %17 = call i64 @llvm.coro.size.i64()
  %18 = call noalias noundef nonnull ptr @_Znwm(i64 noundef %17) #19
  br label %19

19:
  %20 = phi ptr [ null, %2 ], [ %18, %16 ]
  %21 = call ptr @llvm.coro.begin(token %14, ptr %20)
  call void @llvm.lifetime.start.p0(i64 4, ptr %5) #2
  %22 = load i32, ptr %4, align 4
  store i32 %22, ptr %5, align 4
  call void @llvm.lifetime.start.p0(i64 24, ptr %6) #2
  %23 = getelementptr inbounds %"struct.task::promise_type", ptr %6, i32 0, i32 0
  %24 = load i32, ptr %5, align 4
  store i32 %24, ptr %23, align 8
  %25 = getelementptr inbounds %"struct.task::promise_type", ptr %6, i32 0, i32 1
  call void @_ZNSt15__exception_ptr13exception_ptrC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %25) #2
  %26 = getelementptr inbounds %"struct.task::promise_type", ptr %6, i32 0, i32 2
  call void @_ZNSt7__n486116coroutine_handleIvEC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %26) #2
  invoke void @_ZN4task12promise_type17get_return_objectEv(ptr dead_on_unwind writable sret(%struct.task) align 8 %0, ptr noundef nonnull align 8 dereferenceable(24) %6)
          to label %27 unwind label %33

27:
  call void @llvm.lifetime.start.p0(i64 1, ptr %9) #2
  call void @_ZN4task12promise_type15initial_suspendEv(ptr noundef nonnull align 8 dereferenceable(24) %6) #2
  %28 = call noundef i1 @_ZNKSt7__n486114suspend_always11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %9) #2
  br i1 %28, label %37, label %29

29:
  %30 = call token @llvm.coro.save(ptr null)
  call void @llvm.coro.await.suspend.void(ptr %9, ptr %21, ptr @_Z4leafi.__await_suspend_wrapper__init) #2
  %31 = call i8 @llvm.coro.suspend(token %30, i1 false)
  switch i8 %31, label %85 [
    i8 0, label %37
    i8 1, label %32
  ]

32:
  br label %38

33:
  %34 = landingpad { ptr, i32 }
          cleanup
  %35 = extractvalue { ptr, i32 } %34, 0
  store ptr %35, ptr %7, align 8
  %36 = extractvalue { ptr, i32 } %34, 1
  store i32 %36, ptr %8, align 4
  br label %90

37:
  call void @_ZNKSt7__n486114suspend_always12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %9) #2
  br label %38

38:
  %39 = phi i32 [ 0, %37 ], [ 2, %32 ]
  call void @llvm.lifetime.end.p0(i64 1, ptr %9) #2
  switch i32 %39, label %77 [
    i32 0, label %40
  ]

40:
  %41 = load i32, ptr %5, align 4
  %42 = icmp eq i32 %41, 3
  br i1 %42, label %43, label %62

43:
  %44 = call ptr @__cxa_allocate_exception(i64 4) #2
  %45 = load i32, ptr %5, align 4
  store i32 %45, ptr %44, align 16
  invoke void @__cxa_throw(ptr %44, ptr @_ZTIi, ptr null) #20
          to label %101 unwind label %46

46:
  %47 = landingpad { ptr, i32 }
          catch ptr null
  %48 = extractvalue { ptr, i32 } %47, 0
  store ptr %48, ptr %7, align 8
  %49 = extractvalue { ptr, i32 } %47, 1
  store i32 %49, ptr %8, align 4
  br label %50

50:
  %51 = load ptr, ptr %7, align 8
  %52 = call ptr @__cxa_begin_catch(ptr %51) #2
  call void @_ZN4task12promise_type19unhandled_exceptionEv(ptr noundef nonnull align 8 dereferenceable(24) %6) #2
  invoke void @__cxa_end_catch()
          to label %53 unwind label %65

53:
  br label %54

54:
  br label %55

55:
  call void @llvm.lifetime.start.p0(i64 1, ptr %11) #2
  call void @_ZN4task12promise_type13final_suspendEv(ptr noundef nonnull align 8 dereferenceable(24) %6) #2
  %56 = call noundef i1 @_ZN4task12promise_type3fin11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %11) #2
  br i1 %56, label %73, label %57

57:
  %58 = call token @llvm.coro.save(ptr null)
  invoke void @llvm.coro.await.suspend.handle(ptr %11, ptr %21, ptr @_Z4leafi.__await_suspend_wrapper__final)
          to label %59 unwind label %69

59:
  %60 = call i8 @llvm.coro.suspend(token %58, i1 true)
  switch i8 %60, label %85 [
    i8 0, label %73
    i8 1, label %61
  ]

61:
  br label %74

62:
  %63 = load i32, ptr %5, align 4
  %64 = mul nsw i32 %63, 2
  call void @_ZN4task12promise_type12return_valueEi(ptr noundef nonnull align 8 dereferenceable(24) %6, i32 noundef %64) #2
  br label %55

65:
  %66 = landingpad { ptr, i32 }
          cleanup
  %67 = extractvalue { ptr, i32 } %66, 0
  store ptr %67, ptr %7, align 8
  %68 = extractvalue { ptr, i32 } %66, 1
  store i32 %68, ptr %8, align 4
  br label %87

69:
  %70 = landingpad { ptr, i32 }
          cleanup
  %71 = extractvalue { ptr, i32 } %70, 0
  store ptr %71, ptr %7, align 8
  %72 = extractvalue { ptr, i32 } %70, 1
  store i32 %72, ptr %8, align 4
  call void @llvm.lifetime.end.p0(i64 1, ptr %11) #2
  br label %87

73:
  call void @_ZN4task12promise_type3fin12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %11) #2
  br label %74

74:
  %75 = phi i32 [ 0, %73 ], [ 2, %61 ]
  call void @llvm.lifetime.end.p0(i64 1, ptr %11) #2
  switch i32 %75, label %77 [
    i32 0, label %76
  ]

76:
  br label %77

77:
  %78 = phi i32 [ %39, %38 ], [ %75, %74 ], [ 0, %76 ]
  call void @_ZN4task12promise_typeD2Ev(ptr noundef nonnull align 8 dereferenceable(24) %6) #2
  call void @llvm.lifetime.end.p0(i64 24, ptr %6) #2
  call void @llvm.lifetime.end.p0(i64 4, ptr %5) #2
  %79 = call ptr @llvm.coro.free(token %14, ptr %21)
  %80 = icmp ne ptr %79, null
  br i1 %80, label %81, label %83

81:
  %82 = call i64 @llvm.coro.size.i64()
  call void @_ZdlPvm(ptr noundef %79, i64 noundef %82) #2
  br label %83

83:
  switch i32 %78, label %101 [
    i32 0, label %84
    i32 2, label %85
  ]

84:
  br label %85

85:
  %86 = call i1 @llvm.coro.end(ptr null, i1 false, token none)
  ret void

87:
  %88 = call i1 @llvm.coro.end(ptr null, i1 true, token none)
  br i1 %88, label %96, label %89

89:
  br label %90

90:
  call void @_ZN4task12promise_typeD2Ev(ptr noundef nonnull align 8 dereferenceable(24) %6) #2
  call void @llvm.lifetime.end.p0(i64 24, ptr %6) #2
  call void @llvm.lifetime.end.p0(i64 4, ptr %5) #2
  %91 = call ptr @llvm.coro.free(token %14, ptr %21)
  %92 = icmp ne ptr %91, null
  br i1 %92, label %93, label %95

93:
  %94 = call i64 @llvm.coro.size.i64()
  call void @_ZdlPvm(ptr noundef %91, i64 noundef %94) #2
  br label %95

95:
  br label %96

96:
  %97 = load ptr, ptr %7, align 8
  %98 = load i32, ptr %8, align 4
  %99 = insertvalue { ptr, i32 } poison, ptr %97, 0
  %100 = insertvalue { ptr, i32 } %99, i32 %98, 1
  resume { ptr, i32 } %100

101:
  unreachable
}

declare token @llvm.coro.id(i32, ptr readnone, ptr nocapture readonly, ptr) #1

declare i1 @llvm.coro.alloc(token) #2

declare noundef nonnull ptr @_Znwm(i64 noundef) #3

declare i64 @llvm.coro.size.i64() #4

declare ptr @llvm.coro.begin(token, ptr writeonly) #2

declare void @llvm.lifetime.start.p0(i64 immarg, ptr nocapture) #5

define linkonce_odr dso_local void @_ZNSt15__exception_ptr13exception_ptrC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %0) unnamed_addr #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds %"class.std::__exception_ptr::exception_ptr", ptr %3, i32 0, i32 0
  store ptr null, ptr %4, align 8
  ret void
}

define linkonce_odr dso_local void @_ZNSt7__n486116coroutine_handleIvEC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %0) unnamed_addr #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %3, i32 0, i32 0
  store ptr null, ptr %4, align 8
  ret void
}

define linkonce_odr dso_local void @_ZN4task12promise_type17get_return_objectEv(ptr dead_on_unwind noalias writable sret(%struct.task) align 8 %0, ptr noundef nonnull align 8 dereferenceable(24) %1) #7 comdat {
  %3 = alloca ptr, align 8
  %4 = alloca ptr, align 8
  store ptr %0, ptr %3, align 8
  store ptr %1, ptr %4, align 8
  %5 = load ptr, ptr %4, align 8
  %6 = getelementptr inbounds %struct.task, ptr %0, i32 0, i32 0
  %7 = call i64 @_ZNSt7__n486116coroutine_handleIN4task12promise_typeEE12from_promiseERS2_(ptr noundef nonnull align 8 dereferenceable(24) %5)
  %8 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %6, i32 0, i32 0
  %9 = inttoptr i64 %7 to ptr
  store ptr %9, ptr %8, align 8
  ret void
}

declare i32 @__gxx_personality_v0(...)

define linkonce_odr dso_local void @_ZN4task12promise_type15initial_suspendEv(ptr noundef nonnull align 8 dereferenceable(24) %0) #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  ret void
}

define linkonce_odr dso_local noundef i1 @_ZNKSt7__n486114suspend_always11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %0) #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  ret i1 false
}

declare token @llvm.coro.save(ptr) #8

define private void @_Z4leafi.__await_suspend_wrapper__init(ptr noundef nonnull %0, ptr noundef %1) #9 {
  %3 = alloca ptr, align 8
  %4 = alloca ptr, align 8
  %5 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %6 = alloca %"struct.std::__n4861::coroutine_handle", align 8
  store ptr %0, ptr %3, align 8
  store ptr %1, ptr %4, align 8
  %7 = load ptr, ptr %3, align 8
  %8 = load ptr, ptr %4, align 8
  %9 = call i64 @_ZNSt7__n486116coroutine_handleIN4task12promise_typeEE12from_addressEPv(ptr noundef %8) #2
  %10 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %6, i32 0, i32 0
  %11 = inttoptr i64 %9 to ptr
  store ptr %11, ptr %10, align 8
  %12 = call i64 @_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEEcvNS0_IvEEEv(ptr noundef nonnull align 8 dereferenceable(8) %6) #2
  %13 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %5, i32 0, i32 0
  %14 = inttoptr i64 %12 to ptr
  store ptr %14, ptr %13, align 8
  %15 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %5, i32 0, i32 0
  %16 = load ptr, ptr %15, align 8
  %17 = ptrtoint ptr %16 to i64
  call void @_ZNKSt7__n486114suspend_always13await_suspendENS_16coroutine_handleIvEE(ptr noundef nonnull align 1 dereferenceable(1) %7, i64 %17) #2
  ret void
}

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

define linkonce_odr dso_local i64 @_ZNSt7__n486116coroutine_handleIN4task12promise_typeEE12from_addressEPv(ptr noundef %0) #6 comdat {
  %2 = alloca %"struct.std::__n4861::coroutine_handle", align 8
  %3 = alloca ptr, align 8
  store ptr %0, ptr %3, align 8
  call void @_ZNSt7__n486116coroutine_handleIN4task12promise_typeEEC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %2) #2
  %4 = load ptr, ptr %3, align 8
  %5 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %2, i32 0, i32 0
  store ptr %4, ptr %5, align 8
  %6 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %2, i32 0, i32 0
  %7 = load ptr, ptr %6, align 8
  %8 = ptrtoint ptr %7 to i64
  ret i64 %8
}

define linkonce_odr dso_local i64 @_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEEcvNS0_IvEEEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #6 comdat {
  %2 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %3 = alloca ptr, align 8
  store ptr %0, ptr %3, align 8
  %4 = load ptr, ptr %3, align 8
  %5 = call noundef ptr @_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEE7addressEv(ptr noundef nonnull align 8 dereferenceable(8) %4) #2
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

declare i8 @llvm.coro.suspend(token, i1) #2

define linkonce_odr dso_local void @_ZNKSt7__n486114suspend_always12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %0) #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  ret void
}

declare void @llvm.lifetime.end.p0(i64 immarg, ptr nocapture) #5

declare ptr @__cxa_allocate_exception(i64)

declare void @__cxa_throw(ptr, ptr, ptr)

define linkonce_odr dso_local void @_ZN4task12promise_type12return_valueEi(ptr noundef nonnull align 8 dereferenceable(24) %0, i32 noundef %1) #6 comdat {
  %3 = alloca ptr, align 8
  %4 = alloca i32, align 4
  store ptr %0, ptr %3, align 8
  store i32 %1, ptr %4, align 4
  %5 = load ptr, ptr %3, align 8
  %6 = load i32, ptr %4, align 4
  %7 = getelementptr inbounds %"struct.task::promise_type", ptr %5, i32 0, i32 0
  store i32 %6, ptr %7, align 8
  ret void
}

declare ptr @__cxa_begin_catch(ptr)

define linkonce_odr dso_local void @_ZN4task12promise_type19unhandled_exceptionEv(ptr noundef nonnull align 8 dereferenceable(24) %0) #6 comdat {
  %2 = alloca ptr, align 8
  %3 = alloca %"class.std::__exception_ptr::exception_ptr", align 8
  store ptr %0, ptr %2, align 8
  %4 = load ptr, ptr %2, align 8
  call void @_ZSt17current_exceptionv(ptr dead_on_unwind writable sret(%"class.std::__exception_ptr::exception_ptr") align 8 %3) #2
  %5 = getelementptr inbounds %"struct.task::promise_type", ptr %4, i32 0, i32 1
  %6 = call noundef nonnull align 8 dereferenceable(8) ptr @_ZNSt15__exception_ptr13exception_ptraSEOS0_(ptr noundef nonnull align 8 dereferenceable(8) %5, ptr noundef nonnull align 8 dereferenceable(8) %3) #2
  call void @_ZNSt15__exception_ptr13exception_ptrD2Ev(ptr noundef nonnull align 8 dereferenceable(8) %3) #2
  ret void
}

declare void @__cxa_end_catch()

define linkonce_odr dso_local void @_ZN4task12promise_type13final_suspendEv(ptr noundef nonnull align 8 dereferenceable(24) %0) #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  ret void
}

define linkonce_odr dso_local noundef i1 @_ZN4task12promise_type3fin11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %0) #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  ret i1 false
}

define private noundef ptr @_Z4leafi.__await_suspend_wrapper__final(ptr noundef nonnull %0, ptr noundef %1) #9 {
  %3 = alloca ptr, align 8
  %4 = alloca ptr, align 8
  %5 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %6 = alloca %"struct.std::__n4861::coroutine_handle", align 8
  store ptr %0, ptr %3, align 8
  store ptr %1, ptr %4, align 8
  %7 = load ptr, ptr %3, align 8
  %8 = load ptr, ptr %4, align 8
  %9 = call i64 @_ZNSt7__n486116coroutine_handleIN4task12promise_typeEE12from_addressEPv(ptr noundef %8) #2
  %10 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %6, i32 0, i32 0
  %11 = inttoptr i64 %9 to ptr
  store ptr %11, ptr %10, align 8
  %12 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %6, i32 0, i32 0
  %13 = load ptr, ptr %12, align 8
  %14 = ptrtoint ptr %13 to i64
  %15 = call i64 @_ZN4task12promise_type3fin13await_suspendENSt7__n486116coroutine_handleIS0_EE(ptr noundef nonnull align 1 dereferenceable(1) %7, i64 %14) #2
  %16 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %5, i32 0, i32 0
  %17 = inttoptr i64 %15 to ptr
  store ptr %17, ptr %16, align 8
  %18 = call noundef ptr @_ZNKSt7__n486116coroutine_handleIvE7addressEv(ptr noundef nonnull align 8 dereferenceable(8) %5) #2
  ret ptr %18
}

define linkonce_odr dso_local i64 @_ZN4task12promise_type3fin13await_suspendENSt7__n486116coroutine_handleIS0_EE(ptr noundef nonnull align 1 dereferenceable(1) %0, i64 %1) #6 comdat personality ptr @__gxx_personality_v0 {
  %3 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %4 = alloca %"struct.std::__n4861::coroutine_handle", align 8
  %5 = alloca ptr, align 8
  %6 = alloca %"struct.std::__n4861::coroutine_handle.1", align 8
  %7 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %4, i32 0, i32 0
  %8 = inttoptr i64 %1 to ptr
  store ptr %8, ptr %7, align 8
  store ptr %0, ptr %5, align 8
  %9 = load ptr, ptr %5, align 8
  %10 = invoke noundef nonnull align 8 dereferenceable(24) ptr @_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEE7promiseEv(ptr noundef nonnull align 8 dereferenceable(8) %4)
          to label %11 unwind label %29

11:
  %12 = getelementptr inbounds %"struct.task::promise_type", ptr %10, i32 0, i32 2
  %13 = call noundef i1 @_ZNKSt7__n486116coroutine_handleIvEcvbEv(ptr noundef nonnull align 8 dereferenceable(8) %12) #2
  br i1 %13, label %14, label %18

14:
  %15 = invoke noundef nonnull align 8 dereferenceable(24) ptr @_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEE7promiseEv(ptr noundef nonnull align 8 dereferenceable(8) %4)
          to label %16 unwind label %29

16:
  %17 = getelementptr inbounds %"struct.task::promise_type", ptr %15, i32 0, i32 2
  call void @llvm.memcpy.p0.p0.i64(ptr align 8 %3, ptr align 8 %17, i64 8, i1 false)
  br label %25

18:
  %19 = call i64 @_ZNSt7__n486114noop_coroutineEv() #2
  %20 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.1", ptr %6, i32 0, i32 0
  %21 = inttoptr i64 %19 to ptr
  store ptr %21, ptr %20, align 8
  %22 = call i64 @_ZNKSt7__n486116coroutine_handleINS_22noop_coroutine_promiseEEcvNS0_IvEEEv(ptr noundef nonnull align 8 dereferenceable(8) %6) #2
  %23 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %3, i32 0, i32 0
  %24 = inttoptr i64 %22 to ptr
  store ptr %24, ptr %23, align 8
  br label %25

25:
  %26 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %3, i32 0, i32 0
  %27 = load ptr, ptr %26, align 8
  %28 = ptrtoint ptr %27 to i64
  ret i64 %28

29:
  %30 = landingpad { ptr, i32 }
          catch ptr null
  %31 = extractvalue { ptr, i32 } %30, 0
  call void @__clang_call_terminate(ptr %31) #21
  unreachable
}

define linkonce_odr dso_local noundef ptr @_ZNKSt7__n486116coroutine_handleIvE7addressEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %3, i32 0, i32 0
  %5 = load ptr, ptr %4, align 8
  ret ptr %5
}

declare void @llvm.coro.await.suspend.handle(ptr, ptr, ptr)

define linkonce_odr dso_local void @_ZN4task12promise_type3fin12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %0) #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  ret void
}

declare i1 @llvm.coro.end(ptr, i1, token) #2

define linkonce_odr dso_local void @_ZN4task12promise_typeD2Ev(ptr noundef nonnull align 8 dereferenceable(24) %0) unnamed_addr #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds %"struct.task::promise_type", ptr %3, i32 0, i32 1
  call void @_ZNSt15__exception_ptr13exception_ptrD2Ev(ptr noundef nonnull align 8 dereferenceable(8) %4) #2
  ret void
}

declare void @_ZdlPvm(ptr noundef, i64 noundef) #10

declare ptr @llvm.coro.free(token, ptr nocapture readonly) #11

define dso_local void @_Z3sumi(ptr dead_on_unwind noalias writable sret(%struct.task) align 8 %0, i32 noundef %1) #0 personality ptr @__gxx_personality_v0 {
  %3 = alloca ptr, align 8
  %4 = alloca i32, align 4, !coro.outside.frame !6
  %5 = alloca i32, align 4
  %6 = alloca %"struct.task::promise_type", align 8
  %7 = alloca ptr, align 8
  %8 = alloca i32, align 4
  %9 = alloca %"struct.std::__n4861::suspend_always", align 1
  %10 = alloca %"struct.std::__n4861::suspend_always", align 1
  %11 = alloca i32, align 4
  %12 = alloca [16 x i8], align 1
  %13 = alloca i32, align 4
  %14 = alloca %struct.task, align 8
  %15 = alloca i32, align 4
  %16 = alloca %class.anon, align 1
  %17 = alloca %struct.task, align 8
  %18 = alloca %"struct.task::promise_type::fin", align 1
  %19 = alloca %"struct.task::promise_type::fin", align 1
  store ptr %0, ptr %3, align 8
  store i32 %1, ptr %4, align 4
  %20 = bitcast ptr %6 to ptr
  %21 = call token @llvm.coro.id(i32 16, ptr %20, ptr null, ptr null)
  %22 = call i1 @llvm.coro.alloc(token %21)
  br i1 %22, label %23, label %26

23:
  %24 = call i64 @llvm.coro.size.i64()
  %25 = call noalias noundef nonnull ptr @_Znwm(i64 noundef %24) #19
  br label %26

26:
  %27 = phi ptr [ null, %2 ], [ %25, %23 ]
  %28 = call ptr @llvm.coro.begin(token %21, ptr %27)
  call void @llvm.lifetime.start.p0(i64 4, ptr %5) #2
  %29 = load i32, ptr %4, align 4
  store i32 %29, ptr %5, align 4
  call void @llvm.lifetime.start.p0(i64 24, ptr %6) #2
  %30 = getelementptr inbounds %"struct.task::promise_type", ptr %6, i32 0, i32 0
  %31 = load i32, ptr %5, align 4
  store i32 %31, ptr %30, align 8
  %32 = getelementptr inbounds %"struct.task::promise_type", ptr %6, i32 0, i32 1
  call void @_ZNSt15__exception_ptr13exception_ptrC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %32) #2
  %33 = getelementptr inbounds %"struct.task::promise_type", ptr %6, i32 0, i32 2
  call void @_ZNSt7__n486116coroutine_handleIvEC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %33) #2
  invoke void @_ZN4task12promise_type17get_return_objectEv(ptr dead_on_unwind writable sret(%struct.task) align 8 %0, ptr noundef nonnull align 8 dereferenceable(24) %6)
          to label %34 unwind label %40

34:
  call void @llvm.lifetime.start.p0(i64 1, ptr %9) #2
  call void @_ZN4task12promise_type15initial_suspendEv(ptr noundef nonnull align 8 dereferenceable(24) %6) #2
  %35 = call noundef i1 @_ZNKSt7__n486114suspend_always11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %9) #2
  br i1 %35, label %44, label %36

36:
  %37 = call token @llvm.coro.save(ptr null)
  call void @llvm.coro.await.suspend.void(ptr %9, ptr %28, ptr @_Z3sumi.__await_suspend_wrapper__init) #2
  %38 = call i8 @llvm.coro.suspend(token %37, i1 false)
  switch i8 %38, label %169 [
    i8 0, label %44
    i8 1, label %39
  ]

39:
  br label %45

40:
  %41 = landingpad { ptr, i32 }
          cleanup
  %42 = extractvalue { ptr, i32 } %41, 0
  store ptr %42, ptr %7, align 8
  %43 = extractvalue { ptr, i32 } %41, 1
  store i32 %43, ptr %8, align 4
  br label %174

44:
  call void @_ZNKSt7__n486114suspend_always12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %9) #2
  br label %45

45:
  %46 = phi i32 [ 0, %44 ], [ 2, %39 ]
  call void @llvm.lifetime.end.p0(i64 1, ptr %9) #2
  switch i32 %46, label %161 [
    i32 0, label %47
  ]

47:
  call void @llvm.lifetime.start.p0(i64 4, ptr %11) #2
  store i32 0, ptr %11, align 4
  call void @llvm.lifetime.start.p0(i64 16, ptr %12) #2
  %48 = getelementptr inbounds [16 x i8], ptr %12, i64 0, i64 0
  %49 = load i32, ptr %5, align 4
  %50 = call i32 (ptr, i64, ptr, ...) @snprintf(ptr noundef %48, i64 noundef 16, ptr noundef @.str, i32 noundef %49) #2
  call void @llvm.lifetime.start.p0(i64 4, ptr %13) #2
  store i32 0, ptr %13, align 4
  br label %51

51:
  %52 = load i32, ptr %13, align 4
  %53 = load i32, ptr %5, align 4
  %54 = icmp slt i32 %52, %53
  br i1 %54, label %56, label %55

55:
  br label %102

56:
  call void @llvm.lifetime.start.p0(i64 8, ptr %14) #2
  %57 = load i32, ptr %13, align 4
  invoke void @_Z4leafi(ptr dead_on_unwind writable sret(%struct.task) align 8 %14, i32 noundef %57)
          to label %58 unwind label %65

58:
  %59 = call noundef i1 @_ZN4task11await_readyEv(ptr noundef nonnull align 8 dereferenceable(8) %14) #2
  br i1 %59, label %73, label %60

60:
  %61 = call token @llvm.coro.save(ptr null)
  invoke void @llvm.coro.await.suspend.handle(ptr %14, ptr %28, ptr @_Z3sumi.__await_suspend_wrapper__await)
          to label %62 unwind label %69

62:
  %63 = call i8 @llvm.coro.suspend(token %61, i1 false)
  switch i8 %63, label %169 [
    i8 0, label %73
    i8 1, label %64
  ]

64:
  br label %78

65:
  %66 = landingpad { ptr, i32 }
          catch ptr @_ZTIi
          catch ptr null
  %67 = extractvalue { ptr, i32 } %66, 0
  store ptr %67, ptr %7, align 8
  %68 = extractvalue { ptr, i32 } %66, 1
  store i32 %68, ptr %8, align 4
  br label %81

69:
  %70 = landingpad { ptr, i32 }
          catch ptr @_ZTIi
          catch ptr null
  %71 = extractvalue { ptr, i32 } %70, 0
  store ptr %71, ptr %7, align 8
  %72 = extractvalue { ptr, i32 } %70, 1
  store i32 %72, ptr %8, align 4
  call void @_ZN4taskD2Ev(ptr noundef nonnull align 8 dereferenceable(8) %14) #2
  br label %81

73:
  %74 = invoke noundef i32 @_ZN4task12await_resumeEv(ptr noundef nonnull align 8 dereferenceable(8) %14)
          to label %75 unwind label %69

75:
  %76 = load i32, ptr %11, align 4
  %77 = add nsw i32 %76, %74
  store i32 %77, ptr %11, align 4
  br label %78

78:
  %79 = phi i32 [ 0, %75 ], [ 2, %64 ]
  call void @_ZN4taskD2Ev(ptr noundef nonnull align 8 dereferenceable(8) %14) #2
  call void @llvm.lifetime.end.p0(i64 8, ptr %14) #2
  switch i32 %79, label %102 [
    i32 0, label %80
  ]

80:
  br label %94

81:
  call void @llvm.lifetime.end.p0(i64 8, ptr %14) #2
  br label %82

82:
  %83 = load i32, ptr %8, align 4
  %84 = call i32 @llvm.eh.typeid.for.p0(ptr @_ZTIi) #2
  %85 = icmp eq i32 %83, %84
  br i1 %85, label %86, label %104

86:
  %87 = load ptr, ptr %7, align 8
  %88 = call ptr @__cxa_begin_catch(ptr %87) #2
  %89 = load i32, ptr %88, align 4
  store i32 %89, ptr %15, align 4
  %90 = load i32, ptr %15, align 4
  %91 = getelementptr inbounds [16 x i8], ptr %12, i64 0, i64 0
  %92 = invoke i32 (ptr, ...) @printf(ptr noundef @.str.1, i32 noundef %90, ptr noundef %91)
          to label %93 unwind label %98

93:
  call void @__cxa_end_catch() #2
  br label %94

94:
  br label %95

95:
  %96 = load i32, ptr %13, align 4
  %97 = add nsw i32 %96, 1
  store i32 %97, ptr %13, align 4
  br label %51, !llvm.loop !7

98:
  %99 = landingpad { ptr, i32 }
          catch ptr null
  %100 = extractvalue { ptr, i32 } %99, 0
  store ptr %100, ptr %7, align 8
  %101 = extractvalue { ptr, i32 } %99, 1
  store i32 %101, ptr %8, align 4
  call void @__cxa_end_catch() #2
  br label %104

102:
  %103 = phi i32 [ %79, %78 ], [ 4, %55 ]
  call void @llvm.lifetime.end.p0(i64 4, ptr %13) #2
  switch i32 %103, label %134 [
    i32 4, label %105
  ]

104:
  call void @llvm.lifetime.end.p0(i64 4, ptr %13) #2
  br label %136

105:
  call void @llvm.lifetime.start.p0(i64 1, ptr %16) #2
  call void @llvm.memcpy.p0.p0.i64(ptr align 1 %16, ptr align 1 @__const._Z3sumi.twice, i64 1, i1 false)
  call void @llvm.lifetime.start.p0(i64 8, ptr %17) #2
  %106 = load i32, ptr %11, align 4
  invoke void @"_ZZ3sumiENK3$_0clEi"(ptr dead_on_unwind writable sret(%struct.task) align 8 %17, ptr noundef nonnull align 1 dereferenceable(1) %16, i32 noundef %106)
          to label %107 unwind label %114

107:
  %108 = call noundef i1 @_ZN4task11await_readyEv(ptr noundef nonnull align 8 dereferenceable(8) %17) #2
  br i1 %108, label %122, label %109

109:
  %110 = call token @llvm.coro.save(ptr null)
  invoke void @llvm.coro.await.suspend.handle(ptr %17, ptr %28, ptr @_Z3sumi.__await_suspend_wrapper__await2)
          to label %111 unwind label %118

111:
  %112 = call i8 @llvm.coro.suspend(token %110, i1 false)
  switch i8 %112, label %169 [
    i8 0, label %122
    i8 1, label %113
  ]

113:
  br label %127

114:
  %115 = landingpad { ptr, i32 }
          catch ptr null
  %116 = extractvalue { ptr, i32 } %115, 0
  store ptr %116, ptr %7, align 8
  %117 = extractvalue { ptr, i32 } %115, 1
  store i32 %117, ptr %8, align 4
  br label %131

118:
  %119 = landingpad { ptr, i32 }
          catch ptr null
  %120 = extractvalue { ptr, i32 } %119, 0
  store ptr %120, ptr %7, align 8
  %121 = extractvalue { ptr, i32 } %119, 1
  store i32 %121, ptr %8, align 4
  call void @_ZN4taskD2Ev(ptr noundef nonnull align 8 dereferenceable(8) %17) #2
  br label %131

122:
  %123 = invoke noundef i32 @_ZN4task12await_resumeEv(ptr noundef nonnull align 8 dereferenceable(8) %17)
          to label %124 unwind label %118

124:
  %125 = load i32, ptr %11, align 4
  %126 = add nsw i32 %125, %123
  store i32 %126, ptr %11, align 4
  br label %127

127:
  %128 = phi i32 [ 0, %124 ], [ 2, %113 ]
  call void @_ZN4taskD2Ev(ptr noundef nonnull align 8 dereferenceable(8) %17) #2
  call void @llvm.lifetime.end.p0(i64 8, ptr %17) #2
  switch i32 %128, label %132 [
    i32 0, label %129
  ]

129:
  %130 = load i32, ptr %11, align 4
  call void @_ZN4task12promise_type12return_valueEi(ptr noundef nonnull align 8 dereferenceable(24) %6, i32 noundef %130) #2
  br label %132

131:
  call void @llvm.lifetime.end.p0(i64 8, ptr %17) #2
  call void @llvm.lifetime.end.p0(i64 1, ptr %16) #2
  br label %136

132:
  %133 = phi i32 [ %128, %127 ], [ 3, %129 ]
  call void @llvm.lifetime.end.p0(i64 1, ptr %16) #2
  br label %134

134:
  %135 = phi i32 [ %103, %102 ], [ %133, %132 ]
  call void @llvm.lifetime.end.p0(i64 16, ptr %12) #2
  call void @llvm.lifetime.end.p0(i64 4, ptr %11) #2
  switch i32 %135, label %161 [
    i32 3, label %142
  ]

136:
  call void @llvm.lifetime.end.p0(i64 16, ptr %12) #2
  call void @llvm.lifetime.end.p0(i64 4, ptr %11) #2
  br label %137

137:
  %138 = load ptr, ptr %7, align 8
  %139 = call ptr @__cxa_begin_catch(ptr %138) #2
  call void @_ZN4task12promise_type19unhandled_exceptionEv(ptr noundef nonnull align 8 dereferenceable(24) %6) #2
  invoke void @__cxa_end_catch()
          to label %140 unwind label %149

140:
  br label %141

141:
  br label %142

142:
  call void @llvm.lifetime.start.p0(i64 1, ptr %18) #2
  call void @_ZN4task12promise_type13final_suspendEv(ptr noundef nonnull align 8 dereferenceable(24) %6) #2
  %143 = call noundef i1 @_ZN4task12promise_type3fin11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %18) #2
  br i1 %143, label %157, label %144

144:
  %145 = call token @llvm.coro.save(ptr null)
  invoke void @llvm.coro.await.suspend.handle(ptr %18, ptr %28, ptr @_Z3sumi.__await_suspend_wrapper__final)
          to label %146 unwind label %153

146:
  %147 = call i8 @llvm.coro.suspend(token %145, i1 true)
  switch i8 %147, label %169 [
    i8 0, label %157
    i8 1, label %148
  ]

148:
  br label %158

149:
  %150 = landingpad { ptr, i32 }
          cleanup
  %151 = extractvalue { ptr, i32 } %150, 0
  store ptr %151, ptr %7, align 8
  %152 = extractvalue { ptr, i32 } %150, 1
  store i32 %152, ptr %8, align 4
  br label %171

153:
  %154 = landingpad { ptr, i32 }
          cleanup
  %155 = extractvalue { ptr, i32 } %154, 0
  store ptr %155, ptr %7, align 8
  %156 = extractvalue { ptr, i32 } %154, 1
  store i32 %156, ptr %8, align 4
  call void @llvm.lifetime.end.p0(i64 1, ptr %18) #2
  br label %171

157:
  call void @_ZN4task12promise_type3fin12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %18) #2
  br label %158

158:
  %159 = phi i32 [ 0, %157 ], [ 2, %148 ]
  call void @llvm.lifetime.end.p0(i64 1, ptr %18) #2
  switch i32 %159, label %161 [
    i32 0, label %160
  ]

160:
  br label %161

161:
  %162 = phi i32 [ %46, %45 ], [ %135, %134 ], [ %159, %158 ], [ 0, %160 ]
  call void @_ZN4task12promise_typeD2Ev(ptr noundef nonnull align 8 dereferenceable(24) %6) #2
  call void @llvm.lifetime.end.p0(i64 24, ptr %6) #2
  call void @llvm.lifetime.end.p0(i64 4, ptr %5) #2
  %163 = call ptr @llvm.coro.free(token %21, ptr %28)
  %164 = icmp ne ptr %163, null
  br i1 %164, label %165, label %167

165:
  %166 = call i64 @llvm.coro.size.i64()
  call void @_ZdlPvm(ptr noundef %163, i64 noundef %166) #2
  br label %167

167:
  switch i32 %162, label %185 [
    i32 0, label %168
    i32 2, label %169
  ]

168:
  br label %169

169:
  %170 = call i1 @llvm.coro.end(ptr null, i1 false, token none)
  ret void

171:
  %172 = call i1 @llvm.coro.end(ptr null, i1 true, token none)
  br i1 %172, label %180, label %173

173:
  br label %174

174:
  call void @_ZN4task12promise_typeD2Ev(ptr noundef nonnull align 8 dereferenceable(24) %6) #2
  call void @llvm.lifetime.end.p0(i64 24, ptr %6) #2
  call void @llvm.lifetime.end.p0(i64 4, ptr %5) #2
  %175 = call ptr @llvm.coro.free(token %21, ptr %28)
  %176 = icmp ne ptr %175, null
  br i1 %176, label %177, label %179

177:
  %178 = call i64 @llvm.coro.size.i64()
  call void @_ZdlPvm(ptr noundef %175, i64 noundef %178) #2
  br label %179

179:
  br label %180

180:
  %181 = load ptr, ptr %7, align 8
  %182 = load i32, ptr %8, align 4
  %183 = insertvalue { ptr, i32 } poison, ptr %181, 0
  %184 = insertvalue { ptr, i32 } %183, i32 %182, 1
  resume { ptr, i32 } %184

185:
  unreachable
}

define private void @_Z3sumi.__await_suspend_wrapper__init(ptr noundef nonnull %0, ptr noundef %1) #9 {
  %3 = alloca ptr, align 8
  %4 = alloca ptr, align 8
  %5 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %6 = alloca %"struct.std::__n4861::coroutine_handle", align 8
  store ptr %0, ptr %3, align 8
  store ptr %1, ptr %4, align 8
  %7 = load ptr, ptr %3, align 8
  %8 = load ptr, ptr %4, align 8
  %9 = call i64 @_ZNSt7__n486116coroutine_handleIN4task12promise_typeEE12from_addressEPv(ptr noundef %8) #2
  %10 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %6, i32 0, i32 0
  %11 = inttoptr i64 %9 to ptr
  store ptr %11, ptr %10, align 8
  %12 = call i64 @_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEEcvNS0_IvEEEv(ptr noundef nonnull align 8 dereferenceable(8) %6) #2
  %13 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %5, i32 0, i32 0
  %14 = inttoptr i64 %12 to ptr
  store ptr %14, ptr %13, align 8
  %15 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %5, i32 0, i32 0
  %16 = load ptr, ptr %15, align 8
  %17 = ptrtoint ptr %16 to i64
  call void @_ZNKSt7__n486114suspend_always13await_suspendENS_16coroutine_handleIvEE(ptr noundef nonnull align 1 dereferenceable(1) %7, i64 %17) #2
  ret void
}

declare i32 @snprintf(ptr noundef, i64 noundef, ptr noundef, ...) #12

define linkonce_odr dso_local noundef i1 @_ZN4task11await_readyEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  ret i1 false
}

define private noundef ptr @_Z3sumi.__await_suspend_wrapper__await(ptr noundef nonnull %0, ptr noundef %1) #9 {
  %3 = alloca ptr, align 8
  %4 = alloca ptr, align 8
  %5 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %6 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %7 = alloca %"struct.std::__n4861::coroutine_handle", align 8
  store ptr %0, ptr %3, align 8
  store ptr %1, ptr %4, align 8
  %8 = load ptr, ptr %3, align 8
  %9 = load ptr, ptr %4, align 8
  %10 = call i64 @_ZNSt7__n486116coroutine_handleIN4task12promise_typeEE12from_addressEPv(ptr noundef %9) #2
  %11 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %7, i32 0, i32 0
  %12 = inttoptr i64 %10 to ptr
  store ptr %12, ptr %11, align 8
  %13 = call i64 @_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEEcvNS0_IvEEEv(ptr noundef nonnull align 8 dereferenceable(8) %7) #2
  %14 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %6, i32 0, i32 0
  %15 = inttoptr i64 %13 to ptr
  store ptr %15, ptr %14, align 8
  %16 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %6, i32 0, i32 0
  %17 = load ptr, ptr %16, align 8
  %18 = ptrtoint ptr %17 to i64
  %19 = call i64 @_ZN4task13await_suspendENSt7__n486116coroutine_handleIvEE(ptr noundef nonnull align 8 dereferenceable(8) %8, i64 %18) #2
  %20 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %5, i32 0, i32 0
  %21 = inttoptr i64 %19 to ptr
  store ptr %21, ptr %20, align 8
  %22 = call noundef ptr @_ZNKSt7__n486116coroutine_handleIvE7addressEv(ptr noundef nonnull align 8 dereferenceable(8) %5) #2
  ret ptr %22
}

define linkonce_odr dso_local i64 @_ZN4task13await_suspendENSt7__n486116coroutine_handleIvEE(ptr noundef nonnull align 8 dereferenceable(8) %0, i64 %1) #6 comdat personality ptr @__gxx_personality_v0 {
  %3 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %4 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %5 = alloca ptr, align 8
  %6 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %4, i32 0, i32 0
  %7 = inttoptr i64 %1 to ptr
  store ptr %7, ptr %6, align 8
  store ptr %0, ptr %5, align 8
  %8 = load ptr, ptr %5, align 8
  %9 = getelementptr inbounds %struct.task, ptr %8, i32 0, i32 0
  %10 = invoke noundef nonnull align 8 dereferenceable(24) ptr @_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEE7promiseEv(ptr noundef nonnull align 8 dereferenceable(8) %9)
          to label %11 unwind label %20

11:
  %12 = getelementptr inbounds %"struct.task::promise_type", ptr %10, i32 0, i32 2
  call void @llvm.memcpy.p0.p0.i64(ptr align 8 %12, ptr align 8 %4, i64 8, i1 false)
  %13 = getelementptr inbounds %struct.task, ptr %8, i32 0, i32 0
  %14 = call i64 @_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEEcvNS0_IvEEEv(ptr noundef nonnull align 8 dereferenceable(8) %13) #2
  %15 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %3, i32 0, i32 0
  %16 = inttoptr i64 %14 to ptr
  store ptr %16, ptr %15, align 8
  %17 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %3, i32 0, i32 0
  %18 = load ptr, ptr %17, align 8
  %19 = ptrtoint ptr %18 to i64
  ret i64 %19

20:
  %21 = landingpad { ptr, i32 }
          catch ptr null
  %22 = extractvalue { ptr, i32 } %21, 0
  call void @__clang_call_terminate(ptr %22) #21
  unreachable
}

define linkonce_odr dso_local noundef i32 @_ZN4task12await_resumeEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #7 comdat personality ptr @__gxx_personality_v0 {
  %2 = alloca ptr, align 8
  %3 = alloca %"class.std::__exception_ptr::exception_ptr", align 8
  %4 = alloca ptr, align 8
  %5 = alloca i32, align 4
  store ptr %0, ptr %2, align 8
  %6 = load ptr, ptr %2, align 8
  %7 = getelementptr inbounds %struct.task, ptr %6, i32 0, i32 0
  %8 = call noundef nonnull align 8 dereferenceable(24) ptr @_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEE7promiseEv(ptr noundef nonnull align 8 dereferenceable(8) %7)
  %9 = getelementptr inbounds %"struct.task::promise_type", ptr %8, i32 0, i32 1
  %10 = call noundef i1 @_ZNKSt15__exception_ptr13exception_ptrcvbEv(ptr noundef nonnull align 8 dereferenceable(8) %9) #2
  br i1 %10, label %11, label %20

11:
  %12 = getelementptr inbounds %struct.task, ptr %6, i32 0, i32 0
  %13 = call noundef nonnull align 8 dereferenceable(24) ptr @_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEE7promiseEv(ptr noundef nonnull align 8 dereferenceable(8) %12)
  %14 = getelementptr inbounds %"struct.task::promise_type", ptr %13, i32 0, i32 1
  call void @_ZNSt15__exception_ptr13exception_ptrC2ERKS0_(ptr noundef nonnull align 8 dereferenceable(8) %3, ptr noundef nonnull align 8 dereferenceable(8) %14) #2
  invoke void @_ZSt17rethrow_exceptionNSt15__exception_ptr13exception_ptrE(ptr noundef %3) #20
          to label %15 unwind label %16

15:
  unreachable

16:
  %17 = landingpad { ptr, i32 }
          cleanup
  %18 = extractvalue { ptr, i32 } %17, 0
  store ptr %18, ptr %4, align 8
  %19 = extractvalue { ptr, i32 } %17, 1
  store i32 %19, ptr %5, align 4
  call void @_ZNSt15__exception_ptr13exception_ptrD2Ev(ptr noundef nonnull align 8 dereferenceable(8) %3) #2
  br label %25

20:
  %21 = getelementptr inbounds %struct.task, ptr %6, i32 0, i32 0
  %22 = call noundef nonnull align 8 dereferenceable(24) ptr @_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEE7promiseEv(ptr noundef nonnull align 8 dereferenceable(8) %21)
  %23 = getelementptr inbounds %"struct.task::promise_type", ptr %22, i32 0, i32 0
  %24 = load i32, ptr %23, align 8
  ret i32 %24

25:
  %26 = load ptr, ptr %4, align 8
  %27 = load i32, ptr %5, align 4
  %28 = insertvalue { ptr, i32 } poison, ptr %26, 0
  %29 = insertvalue { ptr, i32 } %28, i32 %27, 1
  resume { ptr, i32 } %29
}

define linkonce_odr dso_local void @_ZN4taskD2Ev(ptr noundef nonnull align 8 dereferenceable(8) %0) unnamed_addr #6 comdat personality ptr @__gxx_personality_v0 {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds %struct.task, ptr %3, i32 0, i32 0
  %5 = call noundef i1 @_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEEcvbEv(ptr noundef nonnull align 8 dereferenceable(8) %4) #2
  br i1 %5, label %6, label %9

6:
  %7 = getelementptr inbounds %struct.task, ptr %3, i32 0, i32 0
  invoke void @_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEE7destroyEv(ptr noundef nonnull align 8 dereferenceable(8) %7)
          to label %8 unwind label %10

8:
  br label %9

9:
  ret void

10:
  %11 = landingpad { ptr, i32 }
          catch ptr null
  %12 = extractvalue { ptr, i32 } %11, 0
  call void @__clang_call_terminate(ptr %12) #21
  unreachable
}

declare i32 @llvm.eh.typeid.for.p0(ptr) #4

declare i32 @printf(ptr noundef, ...) #13

declare void @llvm.memcpy.p0.p0.i64(ptr noalias nocapture writeonly, ptr noalias nocapture readonly, i64, i1 immarg) #14

define internal void @"_ZZ3sumiENK3$_0clEi"(ptr dead_on_unwind noalias writable sret(%struct.task) align 8 %0, ptr noundef nonnull align 1 dereferenceable(1) %1, i32 noundef %2) #0 personality ptr @__gxx_personality_v0 {
  %4 = alloca ptr, align 8
  %5 = alloca ptr, align 8
  %6 = alloca i32, align 4, !coro.outside.frame !6
  %7 = alloca i32, align 4
  %8 = alloca %"struct.task::promise_type", align 8
  %9 = alloca ptr, align 8
  %10 = alloca i32, align 4
  %11 = alloca %"struct.std::__n4861::suspend_always", align 1
  %12 = alloca %"struct.std::__n4861::suspend_always", align 1
  %13 = alloca %"struct.task::promise_type::fin", align 1
  %14 = alloca %"struct.task::promise_type::fin", align 1
  store ptr %0, ptr %4, align 8
  store ptr %1, ptr %5, align 8
  store i32 %2, ptr %6, align 4
  %15 = load ptr, ptr %5, align 8
  %16 = bitcast ptr %8 to ptr
  %17 = call token @llvm.coro.id(i32 16, ptr %16, ptr null, ptr null)
  %18 = call i1 @llvm.coro.alloc(token %17)
  br i1 %18, label %19, label %22

19:
  %20 = call i64 @llvm.coro.size.i64()
  %21 = call noalias noundef nonnull ptr @_Znwm(i64 noundef %20) #19
  br label %22

22:
  %23 = phi ptr [ null, %3 ], [ %21, %19 ]
  %24 = call ptr @llvm.coro.begin(token %17, ptr %23)
  call void @llvm.lifetime.start.p0(i64 4, ptr %7) #2
  %25 = load i32, ptr %6, align 4
  store i32 %25, ptr %7, align 4
  call void @llvm.lifetime.start.p0(i64 24, ptr %8) #2
  %26 = getelementptr inbounds %"struct.task::promise_type", ptr %8, i32 0, i32 0
  %27 = load i32, ptr %7, align 4
  store i32 %27, ptr %26, align 8
  %28 = getelementptr inbounds %"struct.task::promise_type", ptr %8, i32 0, i32 1
  call void @_ZNSt15__exception_ptr13exception_ptrC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %28) #2
  %29 = getelementptr inbounds %"struct.task::promise_type", ptr %8, i32 0, i32 2
  call void @_ZNSt7__n486116coroutine_handleIvEC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %29) #2
  invoke void @_ZN4task12promise_type17get_return_objectEv(ptr dead_on_unwind writable sret(%struct.task) align 8 %0, ptr noundef nonnull align 8 dereferenceable(24) %8)
          to label %30 unwind label %36

30:
  call void @llvm.lifetime.start.p0(i64 1, ptr %11) #2
  call void @_ZN4task12promise_type15initial_suspendEv(ptr noundef nonnull align 8 dereferenceable(24) %8) #2
  %31 = call noundef i1 @_ZNKSt7__n486114suspend_always11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %11) #2
  br i1 %31, label %40, label %32

32:
  %33 = call token @llvm.coro.save(ptr null)
  call void @llvm.coro.await.suspend.void(ptr %11, ptr %24, ptr @"_ZZ3sumiENK3$_0clEi.__await_suspend_wrapper__init") #2
  %34 = call i8 @llvm.coro.suspend(token %33, i1 false)
  switch i8 %34, label %71 [
    i8 0, label %40
    i8 1, label %35
  ]

35:
  br label %41

36:
  %37 = landingpad { ptr, i32 }
          cleanup
  %38 = extractvalue { ptr, i32 } %37, 0
  store ptr %38, ptr %9, align 8
  %39 = extractvalue { ptr, i32 } %37, 1
  store i32 %39, ptr %10, align 4
  br label %74

40:
  call void @_ZNKSt7__n486114suspend_always12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %11) #2
  br label %41

41:
  %42 = phi i32 [ 0, %40 ], [ 2, %35 ]
  call void @llvm.lifetime.end.p0(i64 1, ptr %11) #2
  switch i32 %42, label %63 [
    i32 0, label %43
  ]

43:
  %44 = load i32, ptr %7, align 4
  %45 = load i32, ptr %7, align 4
  %46 = add nsw i32 %44, %45
  call void @_ZN4task12promise_type12return_valueEi(ptr noundef nonnull align 8 dereferenceable(24) %8, i32 noundef %46) #2
  br label %47

47:
  call void @llvm.lifetime.start.p0(i64 1, ptr %13) #2
  call void @_ZN4task12promise_type13final_suspendEv(ptr noundef nonnull align 8 dereferenceable(24) %8) #2
  %48 = call noundef i1 @_ZN4task12promise_type3fin11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %13) #2
  br i1 %48, label %59, label %49

49:
  %50 = call token @llvm.coro.save(ptr null)
  invoke void @llvm.coro.await.suspend.handle(ptr %13, ptr %24, ptr @"_ZZ3sumiENK3$_0clEi.__await_suspend_wrapper__final")
          to label %51 unwind label %54

51:
  %52 = call i8 @llvm.coro.suspend(token %50, i1 true)
  switch i8 %52, label %71 [
    i8 0, label %59
    i8 1, label %53
  ]

53:
  br label %60

54:
  %55 = landingpad { ptr, i32 }
          cleanup
  %56 = extractvalue { ptr, i32 } %55, 0
  store ptr %56, ptr %9, align 8
  %57 = extractvalue { ptr, i32 } %55, 1
  store i32 %57, ptr %10, align 4
  call void @llvm.lifetime.end.p0(i64 1, ptr %13) #2
  %58 = call i1 @llvm.coro.end(ptr null, i1 true, token none)
  br i1 %58, label %80, label %73

59:
  call void @_ZN4task12promise_type3fin12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %13) #2
  br label %60

60:
  %61 = phi i32 [ 0, %59 ], [ 2, %53 ]
  call void @llvm.lifetime.end.p0(i64 1, ptr %13) #2
  switch i32 %61, label %63 [
    i32 0, label %62
  ]

62:
  br label %63

63:
  %64 = phi i32 [ %42, %41 ], [ %61, %60 ], [ 0, %62 ]
  call void @_ZN4task12promise_typeD2Ev(ptr noundef nonnull align 8 dereferenceable(24) %8) #2
  call void @llvm.lifetime.end.p0(i64 24, ptr %8) #2
  call void @llvm.lifetime.end.p0(i64 4, ptr %7) #2
  %65 = call ptr @llvm.coro.free(token %17, ptr %24)
  %66 = icmp ne ptr %65, null
  br i1 %66, label %67, label %69

67:
  %68 = call i64 @llvm.coro.size.i64()
  call void @_ZdlPvm(ptr noundef %65, i64 noundef %68) #2
  br label %69

69:
  switch i32 %64, label %85 [
    i32 0, label %70
    i32 2, label %71
  ]

70:
  br label %71

71:
  %72 = call i1 @llvm.coro.end(ptr null, i1 false, token none)
  ret void

73:
  br label %74

74:
  call void @_ZN4task12promise_typeD2Ev(ptr noundef nonnull align 8 dereferenceable(24) %8) #2
  call void @llvm.lifetime.end.p0(i64 24, ptr %8) #2
  call void @llvm.lifetime.end.p0(i64 4, ptr %7) #2
  %75 = call ptr @llvm.coro.free(token %17, ptr %24)
  %76 = icmp ne ptr %75, null
  br i1 %76, label %77, label %79

77:
  %78 = call i64 @llvm.coro.size.i64()
  call void @_ZdlPvm(ptr noundef %75, i64 noundef %78) #2
  br label %79

79:
  br label %80

80:
  %81 = load ptr, ptr %9, align 8
  %82 = load i32, ptr %10, align 4
  %83 = insertvalue { ptr, i32 } poison, ptr %81, 0
  %84 = insertvalue { ptr, i32 } %83, i32 %82, 1
  resume { ptr, i32 } %84

85:
  unreachable
}

define private noundef ptr @_Z3sumi.__await_suspend_wrapper__await2(ptr noundef nonnull %0, ptr noundef %1) #9 {
  %3 = alloca ptr, align 8
  %4 = alloca ptr, align 8
  %5 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %6 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %7 = alloca %"struct.std::__n4861::coroutine_handle", align 8
  store ptr %0, ptr %3, align 8
  store ptr %1, ptr %4, align 8
  %8 = load ptr, ptr %3, align 8
  %9 = load ptr, ptr %4, align 8
  %10 = call i64 @_ZNSt7__n486116coroutine_handleIN4task12promise_typeEE12from_addressEPv(ptr noundef %9) #2
  %11 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %7, i32 0, i32 0
  %12 = inttoptr i64 %10 to ptr
  store ptr %12, ptr %11, align 8
  %13 = call i64 @_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEEcvNS0_IvEEEv(ptr noundef nonnull align 8 dereferenceable(8) %7) #2
  %14 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %6, i32 0, i32 0
  %15 = inttoptr i64 %13 to ptr
  store ptr %15, ptr %14, align 8
  %16 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %6, i32 0, i32 0
  %17 = load ptr, ptr %16, align 8
  %18 = ptrtoint ptr %17 to i64
  %19 = call i64 @_ZN4task13await_suspendENSt7__n486116coroutine_handleIvEE(ptr noundef nonnull align 8 dereferenceable(8) %8, i64 %18) #2
  %20 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %5, i32 0, i32 0
  %21 = inttoptr i64 %19 to ptr
  store ptr %21, ptr %20, align 8
  %22 = call noundef ptr @_ZNKSt7__n486116coroutine_handleIvE7addressEv(ptr noundef nonnull align 8 dereferenceable(8) %5) #2
  ret ptr %22
}

define private noundef ptr @_Z3sumi.__await_suspend_wrapper__final(ptr noundef nonnull %0, ptr noundef %1) #9 {
  %3 = alloca ptr, align 8
  %4 = alloca ptr, align 8
  %5 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %6 = alloca %"struct.std::__n4861::coroutine_handle", align 8
  store ptr %0, ptr %3, align 8
  store ptr %1, ptr %4, align 8
  %7 = load ptr, ptr %3, align 8
  %8 = load ptr, ptr %4, align 8
  %9 = call i64 @_ZNSt7__n486116coroutine_handleIN4task12promise_typeEE12from_addressEPv(ptr noundef %8) #2
  %10 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %6, i32 0, i32 0
  %11 = inttoptr i64 %9 to ptr
  store ptr %11, ptr %10, align 8
  %12 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %6, i32 0, i32 0
  %13 = load ptr, ptr %12, align 8
  %14 = ptrtoint ptr %13 to i64
  %15 = call i64 @_ZN4task12promise_type3fin13await_suspendENSt7__n486116coroutine_handleIS0_EE(ptr noundef nonnull align 1 dereferenceable(1) %7, i64 %14) #2
  %16 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %5, i32 0, i32 0
  %17 = inttoptr i64 %15 to ptr
  store ptr %17, ptr %16, align 8
  %18 = call noundef ptr @_ZNKSt7__n486116coroutine_handleIvE7addressEv(ptr noundef nonnull align 8 dereferenceable(8) %5) #2
  ret ptr %18
}

define dso_local noundef i32 @main() #15 personality ptr @__gxx_personality_v0 {
  %1 = alloca i32, align 4
  %2 = alloca %struct.task, align 8
  %3 = alloca ptr, align 8
  %4 = alloca i32, align 4
  store i32 0, ptr %1, align 4
  call void @_Z3sumi(ptr dead_on_unwind writable sret(%struct.task) align 8 %2, i32 noundef 5)
  %5 = getelementptr inbounds %struct.task, ptr %2, i32 0, i32 0
  invoke void @_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEE6resumeEv(ptr noundef nonnull align 8 dereferenceable(8) %5)
          to label %6 unwind label %15

6:
  %7 = getelementptr inbounds %struct.task, ptr %2, i32 0, i32 0
  %8 = invoke noundef nonnull align 8 dereferenceable(24) ptr @_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEE7promiseEv(ptr noundef nonnull align 8 dereferenceable(8) %7)
          to label %9 unwind label %15

9:
  %10 = getelementptr inbounds %"struct.task::promise_type", ptr %8, i32 0, i32 0
  %11 = load i32, ptr %10, align 8
  %12 = invoke i32 (ptr, ...) @printf(ptr noundef @.str.2, i32 noundef %11)
          to label %13 unwind label %15

13:
  store i32 0, ptr %1, align 4
  call void @_ZN4taskD2Ev(ptr noundef nonnull align 8 dereferenceable(8) %2) #2
  %14 = load i32, ptr %1, align 4
  ret i32 %14

15:
  %16 = landingpad { ptr, i32 }
          cleanup
  %17 = extractvalue { ptr, i32 } %16, 0
  store ptr %17, ptr %3, align 8
  %18 = extractvalue { ptr, i32 } %16, 1
  store i32 %18, ptr %4, align 4
  call void @_ZN4taskD2Ev(ptr noundef nonnull align 8 dereferenceable(8) %2) #2
  br label %19

19:
  %20 = load ptr, ptr %3, align 8
  %21 = load i32, ptr %4, align 4
  %22 = insertvalue { ptr, i32 } poison, ptr %20, 0
  %23 = insertvalue { ptr, i32 } %22, i32 %21, 1
  resume { ptr, i32 } %23
}

define linkonce_odr dso_local void @_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEE6resumeEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %3, i32 0, i32 0
  %5 = load ptr, ptr %4, align 8
  call void @llvm.coro.resume(ptr %5)
  ret void
}

define linkonce_odr dso_local noundef nonnull align 8 dereferenceable(24) ptr @_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEE7promiseEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #6 comdat {
  %2 = alloca ptr, align 8
  %3 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %4 = load ptr, ptr %2, align 8
  %5 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %4, i32 0, i32 0
  %6 = load ptr, ptr %5, align 8
  %7 = call ptr @llvm.coro.promise(ptr %6, i32 8, i1 false)
  store ptr %7, ptr %3, align 8
  %8 = load ptr, ptr %3, align 8
  ret ptr %8
}

define linkonce_odr dso_local i64 @_ZNSt7__n486116coroutine_handleIN4task12promise_typeEE12from_promiseERS2_(ptr noundef nonnull align 8 dereferenceable(24) %0) #6 comdat {
  %2 = alloca %"struct.std::__n4861::coroutine_handle", align 8
  %3 = alloca ptr, align 8
  store ptr %0, ptr %3, align 8
  call void @_ZNSt7__n486116coroutine_handleIN4task12promise_typeEEC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %2) #2
  %4 = load ptr, ptr %3, align 8
  %5 = call ptr @llvm.coro.promise(ptr %4, i32 8, i1 true)
  %6 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %2, i32 0, i32 0
  store ptr %5, ptr %6, align 8
  %7 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %2, i32 0, i32 0
  %8 = load ptr, ptr %7, align 8
  %9 = ptrtoint ptr %8 to i64
  ret i64 %9
}

define linkonce_odr dso_local void @_ZNSt7__n486116coroutine_handleIN4task12promise_typeEEC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %0) unnamed_addr #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %3, i32 0, i32 0
  store ptr null, ptr %4, align 8
  ret void
}

declare ptr @llvm.coro.promise(ptr nocapture, i32, i1) #4

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

define linkonce_odr dso_local noundef ptr @_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEE7addressEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %3, i32 0, i32 0
  %5 = load ptr, ptr %4, align 8
  ret ptr %5
}

declare void @_ZSt17current_exceptionv(ptr dead_on_unwind writable sret(%"class.std::__exception_ptr::exception_ptr") align 8) #12

define linkonce_odr dso_local noundef nonnull align 8 dereferenceable(8) ptr @_ZNSt15__exception_ptr13exception_ptraSEOS0_(ptr noundef nonnull align 8 dereferenceable(8) %0, ptr noundef nonnull align 8 dereferenceable(8) %1) #6 comdat {
  %3 = alloca ptr, align 8
  %4 = alloca ptr, align 8
  %5 = alloca %"class.std::__exception_ptr::exception_ptr", align 8
  store ptr %0, ptr %3, align 8
  store ptr %1, ptr %4, align 8
  %6 = load ptr, ptr %3, align 8
  %7 = load ptr, ptr %4, align 8
  call void @_ZNSt15__exception_ptr13exception_ptrC2EOS0_(ptr noundef nonnull align 8 dereferenceable(8) %5, ptr noundef nonnull align 8 dereferenceable(8) %7) #2
  call void @_ZNSt15__exception_ptr13exception_ptr4swapERS0_(ptr noundef nonnull align 8 dereferenceable(8) %5, ptr noundef nonnull align 8 dereferenceable(8) %6) #2
  call void @_ZNSt15__exception_ptr13exception_ptrD2Ev(ptr noundef nonnull align 8 dereferenceable(8) %5) #2
  ret ptr %6
}

define linkonce_odr dso_local void @_ZNSt15__exception_ptr13exception_ptrD2Ev(ptr noundef nonnull align 8 dereferenceable(8) %0) unnamed_addr #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds %"class.std::__exception_ptr::exception_ptr", ptr %3, i32 0, i32 0
  %5 = load ptr, ptr %4, align 8
  %6 = icmp ne ptr %5, null
  br i1 %6, label %7, label %8

7:
  call void @_ZNSt15__exception_ptr13exception_ptr10_M_releaseEv(ptr noundef nonnull align 8 dereferenceable(8) %3) #2
  br label %8

8:
  ret void
}

define linkonce_odr dso_local void @_ZNSt15__exception_ptr13exception_ptrC2EOS0_(ptr noundef nonnull align 8 dereferenceable(8) %0, ptr noundef nonnull align 8 dereferenceable(8) %1) unnamed_addr #6 comdat {
  %3 = alloca ptr, align 8
  %4 = alloca ptr, align 8
  store ptr %0, ptr %3, align 8
  store ptr %1, ptr %4, align 8
  %5 = load ptr, ptr %3, align 8
  %6 = getelementptr inbounds %"class.std::__exception_ptr::exception_ptr", ptr %5, i32 0, i32 0
  %7 = load ptr, ptr %4, align 8
  %8 = getelementptr inbounds %"class.std::__exception_ptr::exception_ptr", ptr %7, i32 0, i32 0
  %9 = load ptr, ptr %8, align 8
  store ptr %9, ptr %6, align 8
  %10 = load ptr, ptr %4, align 8
  %11 = getelementptr inbounds %"class.std::__exception_ptr::exception_ptr", ptr %10, i32 0, i32 0
  store ptr null, ptr %11, align 8
  ret void
}

define linkonce_odr dso_local void @_ZNSt15__exception_ptr13exception_ptr4swapERS0_(ptr noundef nonnull align 8 dereferenceable(8) %0, ptr noundef nonnull align 8 dereferenceable(8) %1) #6 comdat {
  %3 = alloca ptr, align 8
  %4 = alloca ptr, align 8
  %5 = alloca ptr, align 8
  store ptr %0, ptr %3, align 8
  store ptr %1, ptr %4, align 8
  %6 = load ptr, ptr %3, align 8
  %7 = getelementptr inbounds %"class.std::__exception_ptr::exception_ptr", ptr %6, i32 0, i32 0
  %8 = load ptr, ptr %7, align 8
  store ptr %8, ptr %5, align 8
  %9 = load ptr, ptr %4, align 8
  %10 = getelementptr inbounds %"class.std::__exception_ptr::exception_ptr", ptr %9, i32 0, i32 0
  %11 = load ptr, ptr %10, align 8
  %12 = getelementptr inbounds %"class.std::__exception_ptr::exception_ptr", ptr %6, i32 0, i32 0
  store ptr %11, ptr %12, align 8
  %13 = load ptr, ptr %5, align 8
  %14 = load ptr, ptr %4, align 8
  %15 = getelementptr inbounds %"class.std::__exception_ptr::exception_ptr", ptr %14, i32 0, i32 0
  store ptr %13, ptr %15, align 8
  ret void
}

declare void @_ZNSt15__exception_ptr13exception_ptr10_M_releaseEv(ptr noundef nonnull align 8 dereferenceable(8)) #12

define linkonce_odr hidden void @__clang_call_terminate(ptr noundef %0) #16 comdat {
  %2 = call ptr @__cxa_begin_catch(ptr %0) #2
  call void @_ZSt9terminatev() #21
  unreachable
}

declare void @_ZSt9terminatev()

define linkonce_odr dso_local noundef i1 @_ZNKSt7__n486116coroutine_handleIvEcvbEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %3, i32 0, i32 0
  %5 = load ptr, ptr %4, align 8
  %6 = icmp ne ptr %5, null
  ret i1 %6
}

define linkonce_odr dso_local i64 @_ZNSt7__n486114noop_coroutineEv() #6 comdat {
  %1 = alloca %"struct.std::__n4861::coroutine_handle.1", align 8
  call void @llvm.memset.p0.i64(ptr align 8 %1, i8 0, i64 8, i1 false)
  call void @_ZNSt7__n486116coroutine_handleINS_22noop_coroutine_promiseEEC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %1) #2
  %2 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.1", ptr %1, i32 0, i32 0
  %3 = load ptr, ptr %2, align 8
  %4 = ptrtoint ptr %3 to i64
  ret i64 %4
}

define linkonce_odr dso_local i64 @_ZNKSt7__n486116coroutine_handleINS_22noop_coroutine_promiseEEcvNS0_IvEEEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #6 comdat {
  %2 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %3 = alloca ptr, align 8
  store ptr %0, ptr %3, align 8
  %4 = load ptr, ptr %3, align 8
  %5 = call noundef ptr @_ZNKSt7__n486116coroutine_handleINS_22noop_coroutine_promiseEE7addressEv(ptr noundef nonnull align 8 dereferenceable(8) %4) #2
  %6 = call i64 @_ZNSt7__n486116coroutine_handleIvE12from_addressEPv(ptr noundef %5) #2
  %7 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %2, i32 0, i32 0
  %8 = inttoptr i64 %6 to ptr
  store ptr %8, ptr %7, align 8
  %9 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %2, i32 0, i32 0
  %10 = load ptr, ptr %9, align 8
  %11 = ptrtoint ptr %10 to i64
  ret i64 %11
}

declare void @llvm.memset.p0.i64(ptr nocapture writeonly, i8, i64, i1 immarg) #17

define linkonce_odr dso_local void @_ZNSt7__n486116coroutine_handleINS_22noop_coroutine_promiseEEC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %0) unnamed_addr #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.1", ptr %3, i32 0, i32 0
  store ptr @_ZNSt7__n486116coroutine_handleINS_22noop_coroutine_promiseEE5_S_frE, ptr %4, align 8
  ret void
}

define linkonce_odr dso_local void @_ZNSt7__n486116coroutine_handleINS_22noop_coroutine_promiseEE7__frame22__dummy_resume_destroyEv() #6 comdat {
  ret void
}

define linkonce_odr dso_local noundef ptr @_ZNKSt7__n486116coroutine_handleINS_22noop_coroutine_promiseEE7addressEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.1", ptr %3, i32 0, i32 0
  %5 = load ptr, ptr %4, align 8
  ret ptr %5
}

define linkonce_odr dso_local noundef i1 @_ZNKSt15__exception_ptr13exception_ptrcvbEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds %"class.std::__exception_ptr::exception_ptr", ptr %3, i32 0, i32 0
  %5 = load ptr, ptr %4, align 8
  %6 = icmp ne ptr %5, null
  ret i1 %6
}

declare void @_ZSt17rethrow_exceptionNSt15__exception_ptr13exception_ptrE(ptr noundef) #18

define linkonce_odr dso_local void @_ZNSt15__exception_ptr13exception_ptrC2ERKS0_(ptr noundef nonnull align 8 dereferenceable(8) %0, ptr noundef nonnull align 8 dereferenceable(8) %1) unnamed_addr #6 comdat {
  %3 = alloca ptr, align 8
  %4 = alloca ptr, align 8
  store ptr %0, ptr %3, align 8
  store ptr %1, ptr %4, align 8
  %5 = load ptr, ptr %3, align 8
  %6 = getelementptr inbounds %"class.std::__exception_ptr::exception_ptr", ptr %5, i32 0, i32 0
  %7 = load ptr, ptr %4, align 8
  %8 = getelementptr inbounds %"class.std::__exception_ptr::exception_ptr", ptr %7, i32 0, i32 0
  %9 = load ptr, ptr %8, align 8
  store ptr %9, ptr %6, align 8
  %10 = getelementptr inbounds %"class.std::__exception_ptr::exception_ptr", ptr %5, i32 0, i32 0
  %11 = load ptr, ptr %10, align 8
  %12 = icmp ne ptr %11, null
  br i1 %12, label %13, label %14

13:
  call void @_ZNSt15__exception_ptr13exception_ptr9_M_addrefEv(ptr noundef nonnull align 8 dereferenceable(8) %5) #2
  br label %14

14:
  ret void
}

declare void @_ZNSt15__exception_ptr13exception_ptr9_M_addrefEv(ptr noundef nonnull align 8 dereferenceable(8)) #12

define linkonce_odr dso_local noundef i1 @_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEEcvbEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %3, i32 0, i32 0
  %5 = load ptr, ptr %4, align 8
  %6 = icmp ne ptr %5, null
  ret i1 %6
}

define linkonce_odr dso_local void @_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEE7destroyEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %3, i32 0, i32 0
  %5 = load ptr, ptr %4, align 8
  call void @llvm.coro.destroy(ptr %5)
  ret void
}

declare void @llvm.coro.destroy(ptr)

define private void @"_ZZ3sumiENK3$_0clEi.__await_suspend_wrapper__init"(ptr noundef nonnull %0, ptr noundef %1) #9 {
  %3 = alloca ptr, align 8
  %4 = alloca ptr, align 8
  %5 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %6 = alloca %"struct.std::__n4861::coroutine_handle", align 8
  store ptr %0, ptr %3, align 8
  store ptr %1, ptr %4, align 8
  %7 = load ptr, ptr %3, align 8
  %8 = load ptr, ptr %4, align 8
  %9 = call i64 @_ZNSt7__n486116coroutine_handleIN4task12promise_typeEE12from_addressEPv(ptr noundef %8) #2
  %10 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %6, i32 0, i32 0
  %11 = inttoptr i64 %9 to ptr
  store ptr %11, ptr %10, align 8
  %12 = call i64 @_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEEcvNS0_IvEEEv(ptr noundef nonnull align 8 dereferenceable(8) %6) #2
  %13 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %5, i32 0, i32 0
  %14 = inttoptr i64 %12 to ptr
  store ptr %14, ptr %13, align 8
  %15 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %5, i32 0, i32 0
  %16 = load ptr, ptr %15, align 8
  %17 = ptrtoint ptr %16 to i64
  call void @_ZNKSt7__n486114suspend_always13await_suspendENS_16coroutine_handleIvEE(ptr noundef nonnull align 1 dereferenceable(1) %7, i64 %17) #2
  ret void
}

define private noundef ptr @"_ZZ3sumiENK3$_0clEi.__await_suspend_wrapper__final"(ptr noundef nonnull %0, ptr noundef %1) #9 {
  %3 = alloca ptr, align 8
  %4 = alloca ptr, align 8
  %5 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %6 = alloca %"struct.std::__n4861::coroutine_handle", align 8
  store ptr %0, ptr %3, align 8
  store ptr %1, ptr %4, align 8
  %7 = load ptr, ptr %3, align 8
  %8 = load ptr, ptr %4, align 8
  %9 = call i64 @_ZNSt7__n486116coroutine_handleIN4task12promise_typeEE12from_addressEPv(ptr noundef %8) #2
  %10 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %6, i32 0, i32 0
  %11 = inttoptr i64 %9 to ptr
  store ptr %11, ptr %10, align 8
  %12 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %6, i32 0, i32 0
  %13 = load ptr, ptr %12, align 8
  %14 = ptrtoint ptr %13 to i64
  %15 = call i64 @_ZN4task12promise_type3fin13await_suspendENSt7__n486116coroutine_handleIS0_EE(ptr noundef nonnull align 1 dereferenceable(1) %7, i64 %14) #2
  %16 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %5, i32 0, i32 0
  %17 = inttoptr i64 %15 to ptr
  store ptr %17, ptr %16, align 8
  %18 = call noundef ptr @_ZNKSt7__n486116coroutine_handleIvE7addressEv(ptr noundef nonnull align 8 dereferenceable(8) %5) #2
  ret ptr %18
}

declare void @llvm.coro.resume(ptr)

attributes #0 = { mustprogress noinline optnone presplitcoroutine sspstrong uwtable "frame-pointer"="non-leaf" "no-trapping-math"="true" "stack-protector-buffer-size"="8" }
attributes #1 = { nocallback nofree nosync nounwind willreturn memory(argmem: read) }
attributes #2 = { nounwind }
attributes #3 = { nobuiltin allocsize(0) "frame-pointer"="non-leaf" "no-trapping-math"="true" "stack-protector-buffer-size"="8" }
attributes #4 = { nounwind memory(none) }
attributes #5 = { nocallback nofree nosync nounwind willreturn memory(argmem: readwrite) }
attributes #6 = { mustprogress noinline nounwind optnone sspstrong uwtable "frame-pointer"="non-leaf" "no-trapping-math"="true" "stack-protector-buffer-size"="8" }
attributes #7 = { mustprogress noinline optnone sspstrong uwtable "frame-pointer"="non-leaf" "no-trapping-math"="true" "stack-protector-buffer-size"="8" }
attributes #8 = { nomerge nounwind }
attributes #9 = { alwaysinline mustprogress }
attributes #10 = { nobuiltin nounwind "frame-pointer"="non-leaf" "no-trapping-math"="true" "stack-protector-buffer-size"="8" }
attributes #11 = { nounwind memory(argmem: read) }
attributes #12 = { nounwind "frame-pointer"="non-leaf" "no-trapping-math"="true" "stack-protector-buffer-size"="8" }
attributes #13 = { "frame-pointer"="non-leaf" "no-trapping-math"="true" "stack-protector-buffer-size"="8" }
attributes #14 = { nocallback nofree nounwind willreturn memory(argmem: readwrite) }
attributes #15 = { mustprogress noinline norecurse optnone sspstrong uwtable "frame-pointer"="non-leaf" "no-trapping-math"="true" "stack-protector-buffer-size"="8" }
attributes #16 = { noinline noreturn nounwind sspstrong uwtable "frame-pointer"="non-leaf" "no-trapping-math"="true" "stack-protector-buffer-size"="8" }
attributes #17 = { nocallback nofree nounwind willreturn memory(argmem: write) }
attributes #18 = { noreturn "frame-pointer"="non-leaf" "no-trapping-math"="true" "stack-protector-buffer-size"="8" }
attributes #19 = { allocsize(0) }
attributes #20 = { noreturn }
attributes #21 = { noreturn nounwind }

!llvm.linker.options = !{}
!llvm.module.flags = !{!0, !1, !2, !3, !4}
!llvm.ident = !{!5}

!0 = !{i32 1, !"wchar_size", i32 4}
!1 = !{i32 8, !"PIC Level", i32 2}
!2 = !{i32 7, !"PIE Level", i32 2}
!3 = !{i32 7, !"uwtable", i32 2}
!4 = !{i32 7, !"frame-pointer", i32 1}
!5 = !{!"kiln version 1.2.0"}
!6 = !{}
!7 = distinct !{!7, !8}
!8 = !{!"llvm.loop.mustprogress"}
