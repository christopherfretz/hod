; kiln++ --llvm-dir=<LLVM 22> -std=c++20 -fclang-compat=22 -fstack-protector-strong -fkiln-backend=hod: the module kiln gives hod
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
;@ dialect: 22
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
  %13 = call token @llvm.coro.id(i32 16, ptr %6, ptr null, ptr null)
  %14 = call i1 @llvm.coro.alloc(token %13)
  br i1 %14, label %15, label %18

15:
  %16 = call i64 @llvm.coro.size.i64()
  %17 = call noalias noundef nonnull ptr @_Znwm(i64 noundef %16) #19
  br label %18

18:
  %19 = phi ptr [ null, %2 ], [ %17, %15 ]
  %20 = call ptr @llvm.coro.begin(token %13, ptr %19)
  call void @llvm.lifetime.start.p0(ptr %5) #2
  %21 = load i32, ptr %4, align 4
  store i32 %21, ptr %5, align 4
  call void @llvm.lifetime.start.p0(ptr %6) #2
  %22 = getelementptr inbounds nuw %"struct.task::promise_type", ptr %6, i32 0, i32 0
  %23 = load i32, ptr %5, align 4
  store i32 %23, ptr %22, align 8
  %24 = getelementptr inbounds nuw %"struct.task::promise_type", ptr %6, i32 0, i32 1
  call void @_ZNSt15__exception_ptr13exception_ptrC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %24) #2
  %25 = getelementptr inbounds nuw %"struct.task::promise_type", ptr %6, i32 0, i32 2
  call void @_ZNSt7__n486116coroutine_handleIvEC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %25) #2
  invoke void @_ZN4task12promise_type17get_return_objectEv(ptr dead_on_unwind writable sret(%struct.task) align 8 %0, ptr noundef nonnull align 8 dereferenceable(24) %6)
          to label %26 unwind label %32

26:
  call void @llvm.lifetime.start.p0(ptr %9) #2
  call void @_ZN4task12promise_type15initial_suspendEv(ptr noundef nonnull align 8 dereferenceable(24) %6) #2
  %27 = call noundef i1 @_ZNKSt7__n486114suspend_always11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %9) #2
  br i1 %27, label %36, label %28

28:
  %29 = call token @llvm.coro.save(ptr null)
  call void @llvm.coro.await.suspend.void(ptr %9, ptr %20, ptr @_Z4leafi.__await_suspend_wrapper__init) #2
  %30 = call i8 @llvm.coro.suspend(token %29, i1 false)
  switch i8 %30, label %84 [
    i8 0, label %36
    i8 1, label %31
  ]

31:
  br label %37

32:
  %33 = landingpad { ptr, i32 }
          cleanup
  %34 = extractvalue { ptr, i32 } %33, 0
  store ptr %34, ptr %7, align 8
  %35 = extractvalue { ptr, i32 } %33, 1
  store i32 %35, ptr %8, align 4
  br label %88

36:
  call void @_ZNKSt7__n486114suspend_always12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %9) #2
  br label %37

37:
  %38 = phi i32 [ 0, %36 ], [ 2, %31 ]
  call void @llvm.lifetime.end.p0(ptr %9) #2
  switch i32 %38, label %76 [
    i32 0, label %39
  ]

39:
  %40 = load i32, ptr %5, align 4
  %41 = icmp eq i32 %40, 3
  br i1 %41, label %42, label %61

42:
  %43 = call ptr @__cxa_allocate_exception(i64 4) #2
  %44 = load i32, ptr %5, align 4
  store i32 %44, ptr %43, align 16
  invoke void @__cxa_throw(ptr %43, ptr @_ZTIi, ptr null) #20
          to label %99 unwind label %45

45:
  %46 = landingpad { ptr, i32 }
          catch ptr null
  %47 = extractvalue { ptr, i32 } %46, 0
  store ptr %47, ptr %7, align 8
  %48 = extractvalue { ptr, i32 } %46, 1
  store i32 %48, ptr %8, align 4
  br label %49

49:
  %50 = load ptr, ptr %7, align 8
  %51 = call ptr @__cxa_begin_catch(ptr %50) #2
  call void @_ZN4task12promise_type19unhandled_exceptionEv(ptr noundef nonnull align 8 dereferenceable(24) %6) #2
  invoke void @__cxa_end_catch()
          to label %52 unwind label %64

52:
  br label %53

53:
  br label %54

54:
  call void @llvm.lifetime.start.p0(ptr %11) #2
  call void @_ZN4task12promise_type13final_suspendEv(ptr noundef nonnull align 8 dereferenceable(24) %6) #2
  %55 = call noundef i1 @_ZN4task12promise_type3fin11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %11) #2
  br i1 %55, label %72, label %56

56:
  %57 = call token @llvm.coro.save(ptr null)
  invoke void @llvm.coro.await.suspend.handle(ptr %11, ptr %20, ptr @_Z4leafi.__await_suspend_wrapper__final)
          to label %58 unwind label %68

58:
  %59 = call i8 @llvm.coro.suspend(token %57, i1 true)
  switch i8 %59, label %84 [
    i8 0, label %72
    i8 1, label %60
  ]

60:
  br label %73

61:
  %62 = load i32, ptr %5, align 4
  %63 = mul nsw i32 %62, 2
  call void @_ZN4task12promise_type12return_valueEi(ptr noundef nonnull align 8 dereferenceable(24) %6, i32 noundef %63) #2
  br label %54

64:
  %65 = landingpad { ptr, i32 }
          cleanup
  %66 = extractvalue { ptr, i32 } %65, 0
  store ptr %66, ptr %7, align 8
  %67 = extractvalue { ptr, i32 } %65, 1
  store i32 %67, ptr %8, align 4
  br label %85

68:
  %69 = landingpad { ptr, i32 }
          cleanup
  %70 = extractvalue { ptr, i32 } %69, 0
  store ptr %70, ptr %7, align 8
  %71 = extractvalue { ptr, i32 } %69, 1
  store i32 %71, ptr %8, align 4
  call void @llvm.lifetime.end.p0(ptr %11) #2
  br label %85

72:
  call void @_ZN4task12promise_type3fin12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %11) #2
  br label %73

73:
  %74 = phi i32 [ 0, %72 ], [ 2, %60 ]
  call void @llvm.lifetime.end.p0(ptr %11) #2
  switch i32 %74, label %76 [
    i32 0, label %75
  ]

75:
  br label %76

76:
  %77 = phi i32 [ %38, %37 ], [ %74, %73 ], [ 0, %75 ]
  call void @_ZN4task12promise_typeD2Ev(ptr noundef nonnull align 8 dereferenceable(24) %6) #2
  call void @llvm.lifetime.end.p0(ptr %6) #2
  call void @llvm.lifetime.end.p0(ptr %5) #2
  %78 = call ptr @llvm.coro.free(token %13, ptr %20)
  %79 = icmp ne ptr %78, null
  br i1 %79, label %80, label %82

80:
  %81 = call i64 @llvm.coro.size.i64()
  call void @_ZdlPvm(ptr noundef %78, i64 noundef %81) #2
  br label %82

82:
  switch i32 %77, label %99 [
    i32 0, label %83
    i32 2, label %84
  ]

83:
  br label %84

84:
  call void @llvm.coro.end(ptr null, i1 false, token none)
  ret void

85:
  call void @llvm.coro.end(ptr null, i1 true, token none)
  %86 = call i1 @llvm.coro.is_in_ramp()
  br i1 %86, label %87, label %94

87:
  br label %88

88:
  call void @_ZN4task12promise_typeD2Ev(ptr noundef nonnull align 8 dereferenceable(24) %6) #2
  call void @llvm.lifetime.end.p0(ptr %6) #2
  call void @llvm.lifetime.end.p0(ptr %5) #2
  %89 = call ptr @llvm.coro.free(token %13, ptr %20)
  %90 = icmp ne ptr %89, null
  br i1 %90, label %91, label %93

91:
  %92 = call i64 @llvm.coro.size.i64()
  call void @_ZdlPvm(ptr noundef %89, i64 noundef %92) #2
  br label %93

93:
  br label %94

94:
  %95 = load ptr, ptr %7, align 8
  %96 = load i32, ptr %8, align 4
  %97 = insertvalue { ptr, i32 } poison, ptr %95, 0
  %98 = insertvalue { ptr, i32 } %97, i32 %96, 1
  resume { ptr, i32 } %98

99:
  unreachable
}

declare token @llvm.coro.id(i32, ptr readnone, ptr readonly captures(none), ptr) #1

declare i1 @llvm.coro.alloc(token) #2

declare noundef nonnull ptr @_Znwm(i64 noundef) #3

declare i64 @llvm.coro.size.i64() #4

declare ptr @llvm.coro.begin(token, ptr writeonly) #2

declare void @llvm.lifetime.start.p0(ptr captures(none)) #5

define linkonce_odr dso_local void @_ZNSt15__exception_ptr13exception_ptrC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %0) unnamed_addr #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds nuw %"class.std::__exception_ptr::exception_ptr", ptr %3, i32 0, i32 0
  store ptr null, ptr %4, align 8
  ret void
}

define linkonce_odr dso_local void @_ZNSt7__n486116coroutine_handleIvEC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %0) unnamed_addr #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %3, i32 0, i32 0
  store ptr null, ptr %4, align 8
  ret void
}

define linkonce_odr dso_local void @_ZN4task12promise_type17get_return_objectEv(ptr dead_on_unwind noalias writable sret(%struct.task) align 8 %0, ptr noundef nonnull align 8 dereferenceable(24) %1) #7 comdat {
  %3 = alloca ptr, align 8
  %4 = alloca ptr, align 8
  store ptr %0, ptr %3, align 8
  store ptr %1, ptr %4, align 8
  %5 = load ptr, ptr %4, align 8
  %6 = getelementptr inbounds nuw %struct.task, ptr %0, i32 0, i32 0
  %7 = call i64 @_ZNSt7__n486116coroutine_handleIN4task12promise_typeEE12from_promiseERS2_(ptr noundef nonnull align 8 dereferenceable(24) %5)
  %8 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %6, i32 0, i32 0
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

define internal void @_Z4leafi.__await_suspend_wrapper__init(ptr noundef nonnull %0, ptr noundef %1) #9 {
  %3 = alloca ptr, align 8
  %4 = alloca ptr, align 8
  %5 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %6 = alloca %"struct.std::__n4861::coroutine_handle", align 8
  store ptr %0, ptr %3, align 8
  store ptr %1, ptr %4, align 8
  %7 = load ptr, ptr %3, align 8
  %8 = load ptr, ptr %4, align 8
  %9 = call i64 @_ZNSt7__n486116coroutine_handleIN4task12promise_typeEE12from_addressEPv(ptr noundef %8) #2
  %10 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %6, i32 0, i32 0
  %11 = inttoptr i64 %9 to ptr
  store ptr %11, ptr %10, align 8
  %12 = call i64 @_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEEcvNS0_IvEEEv(ptr noundef nonnull align 8 dereferenceable(8) %6) #2
  %13 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %5, i32 0, i32 0
  %14 = inttoptr i64 %12 to ptr
  store ptr %14, ptr %13, align 8
  %15 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %5, i32 0, i32 0
  %16 = load ptr, ptr %15, align 8
  call void @_ZNKSt7__n486114suspend_always13await_suspendENS_16coroutine_handleIvEE(ptr noundef nonnull align 1 dereferenceable(1) %7, ptr %16) #2
  ret void
}

define linkonce_odr dso_local void @_ZNKSt7__n486114suspend_always13await_suspendENS_16coroutine_handleIvEE(ptr noundef nonnull align 1 dereferenceable(1) %0, ptr %1) #6 comdat {
  %3 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %4 = alloca ptr, align 8
  %5 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %3, i32 0, i32 0
  store ptr %1, ptr %5, align 8
  store ptr %0, ptr %4, align 8
  %6 = load ptr, ptr %4, align 8
  ret void
}

define linkonce_odr dso_local i64 @_ZNSt7__n486116coroutine_handleIN4task12promise_typeEE12from_addressEPv(ptr noundef %0) #6 comdat {
  %2 = alloca %"struct.std::__n4861::coroutine_handle", align 8
  %3 = alloca ptr, align 8
  store ptr %0, ptr %3, align 8
  call void @_ZNSt7__n486116coroutine_handleIN4task12promise_typeEEC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %2) #2
  %4 = load ptr, ptr %3, align 8
  %5 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %2, i32 0, i32 0
  store ptr %4, ptr %5, align 8
  %6 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %2, i32 0, i32 0
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
  %7 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %2, i32 0, i32 0
  %8 = inttoptr i64 %6 to ptr
  store ptr %8, ptr %7, align 8
  %9 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %2, i32 0, i32 0
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

declare void @llvm.lifetime.end.p0(ptr captures(none)) #5

declare ptr @__cxa_allocate_exception(i64)

declare void @__cxa_throw(ptr, ptr, ptr)

define linkonce_odr dso_local void @_ZN4task12promise_type12return_valueEi(ptr noundef nonnull align 8 dereferenceable(24) %0, i32 noundef %1) #6 comdat {
  %3 = alloca ptr, align 8
  %4 = alloca i32, align 4
  store ptr %0, ptr %3, align 8
  store i32 %1, ptr %4, align 4
  %5 = load ptr, ptr %3, align 8
  %6 = load i32, ptr %4, align 4
  %7 = getelementptr inbounds nuw %"struct.task::promise_type", ptr %5, i32 0, i32 0
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
  %5 = getelementptr inbounds nuw %"struct.task::promise_type", ptr %4, i32 0, i32 1
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

define internal noundef ptr @_Z4leafi.__await_suspend_wrapper__final(ptr noundef nonnull %0, ptr noundef %1) #9 {
  %3 = alloca ptr, align 8
  %4 = alloca ptr, align 8
  %5 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %6 = alloca %"struct.std::__n4861::coroutine_handle", align 8
  store ptr %0, ptr %3, align 8
  store ptr %1, ptr %4, align 8
  %7 = load ptr, ptr %3, align 8
  %8 = load ptr, ptr %4, align 8
  %9 = call i64 @_ZNSt7__n486116coroutine_handleIN4task12promise_typeEE12from_addressEPv(ptr noundef %8) #2
  %10 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %6, i32 0, i32 0
  %11 = inttoptr i64 %9 to ptr
  store ptr %11, ptr %10, align 8
  %12 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %6, i32 0, i32 0
  %13 = load ptr, ptr %12, align 8
  %14 = call i64 @_ZN4task12promise_type3fin13await_suspendENSt7__n486116coroutine_handleIS0_EE(ptr noundef nonnull align 1 dereferenceable(1) %7, ptr %13) #2
  %15 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %5, i32 0, i32 0
  %16 = inttoptr i64 %14 to ptr
  store ptr %16, ptr %15, align 8
  %17 = call noundef ptr @_ZNKSt7__n486116coroutine_handleIvE7addressEv(ptr noundef nonnull align 8 dereferenceable(8) %5) #2
  ret ptr %17
}

define linkonce_odr dso_local i64 @_ZN4task12promise_type3fin13await_suspendENSt7__n486116coroutine_handleIS0_EE(ptr noundef nonnull align 1 dereferenceable(1) %0, ptr %1) #6 comdat personality ptr @__gxx_personality_v0 {
  %3 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %4 = alloca %"struct.std::__n4861::coroutine_handle", align 8
  %5 = alloca ptr, align 8
  %6 = alloca %"struct.std::__n4861::coroutine_handle.1", align 8
  %7 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %4, i32 0, i32 0
  store ptr %1, ptr %7, align 8
  store ptr %0, ptr %5, align 8
  %8 = load ptr, ptr %5, align 8
  %9 = invoke noundef nonnull align 8 dereferenceable(24) ptr @_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEE7promiseEv(ptr noundef nonnull align 8 dereferenceable(8) %4)
          to label %10 unwind label %28

10:
  %11 = getelementptr inbounds nuw %"struct.task::promise_type", ptr %9, i32 0, i32 2
  %12 = call noundef i1 @_ZNKSt7__n486116coroutine_handleIvEcvbEv(ptr noundef nonnull align 8 dereferenceable(8) %11) #2
  br i1 %12, label %13, label %17

13:
  %14 = invoke noundef nonnull align 8 dereferenceable(24) ptr @_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEE7promiseEv(ptr noundef nonnull align 8 dereferenceable(8) %4)
          to label %15 unwind label %28

15:
  %16 = getelementptr inbounds nuw %"struct.task::promise_type", ptr %14, i32 0, i32 2
  call void @llvm.memcpy.p0.p0.i64(ptr align 8 %3, ptr align 8 %16, i64 8, i1 false)
  br label %24

17:
  %18 = call i64 @_ZNSt7__n486114noop_coroutineEv() #2
  %19 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.1", ptr %6, i32 0, i32 0
  %20 = inttoptr i64 %18 to ptr
  store ptr %20, ptr %19, align 8
  %21 = call i64 @_ZNKSt7__n486116coroutine_handleINS_22noop_coroutine_promiseEEcvNS0_IvEEEv(ptr noundef nonnull align 8 dereferenceable(8) %6) #2
  %22 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %3, i32 0, i32 0
  %23 = inttoptr i64 %21 to ptr
  store ptr %23, ptr %22, align 8
  br label %24

24:
  %25 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %3, i32 0, i32 0
  %26 = load ptr, ptr %25, align 8
  %27 = ptrtoint ptr %26 to i64
  ret i64 %27

28:
  %29 = landingpad { ptr, i32 }
          catch ptr null
  %30 = extractvalue { ptr, i32 } %29, 0
  call void @__clang_call_terminate(ptr %30) #21
  unreachable
}

define linkonce_odr dso_local noundef ptr @_ZNKSt7__n486116coroutine_handleIvE7addressEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %3, i32 0, i32 0
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

declare void @llvm.coro.end(ptr, i1, token) #2

declare i1 @llvm.coro.is_in_ramp() #4

define linkonce_odr dso_local void @_ZN4task12promise_typeD2Ev(ptr noundef nonnull align 8 dereferenceable(24) %0) unnamed_addr #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds nuw %"struct.task::promise_type", ptr %3, i32 0, i32 1
  call void @_ZNSt15__exception_ptr13exception_ptrD2Ev(ptr noundef nonnull align 8 dereferenceable(8) %4) #2
  ret void
}

declare void @_ZdlPvm(ptr noundef, i64 noundef) #10

declare ptr @llvm.coro.free(token, ptr readonly captures(none)) #11

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
  %20 = call token @llvm.coro.id(i32 16, ptr %6, ptr null, ptr null)
  %21 = call i1 @llvm.coro.alloc(token %20)
  br i1 %21, label %22, label %25

22:
  %23 = call i64 @llvm.coro.size.i64()
  %24 = call noalias noundef nonnull ptr @_Znwm(i64 noundef %23) #19
  br label %25

25:
  %26 = phi ptr [ null, %2 ], [ %24, %22 ]
  %27 = call ptr @llvm.coro.begin(token %20, ptr %26)
  call void @llvm.lifetime.start.p0(ptr %5) #2
  %28 = load i32, ptr %4, align 4
  store i32 %28, ptr %5, align 4
  call void @llvm.lifetime.start.p0(ptr %6) #2
  %29 = getelementptr inbounds nuw %"struct.task::promise_type", ptr %6, i32 0, i32 0
  %30 = load i32, ptr %5, align 4
  store i32 %30, ptr %29, align 8
  %31 = getelementptr inbounds nuw %"struct.task::promise_type", ptr %6, i32 0, i32 1
  call void @_ZNSt15__exception_ptr13exception_ptrC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %31) #2
  %32 = getelementptr inbounds nuw %"struct.task::promise_type", ptr %6, i32 0, i32 2
  call void @_ZNSt7__n486116coroutine_handleIvEC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %32) #2
  invoke void @_ZN4task12promise_type17get_return_objectEv(ptr dead_on_unwind writable sret(%struct.task) align 8 %0, ptr noundef nonnull align 8 dereferenceable(24) %6)
          to label %33 unwind label %39

33:
  call void @llvm.lifetime.start.p0(ptr %9) #2
  call void @_ZN4task12promise_type15initial_suspendEv(ptr noundef nonnull align 8 dereferenceable(24) %6) #2
  %34 = call noundef i1 @_ZNKSt7__n486114suspend_always11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %9) #2
  br i1 %34, label %43, label %35

35:
  %36 = call token @llvm.coro.save(ptr null)
  call void @llvm.coro.await.suspend.void(ptr %9, ptr %27, ptr @_Z3sumi.__await_suspend_wrapper__init) #2
  %37 = call i8 @llvm.coro.suspend(token %36, i1 false)
  switch i8 %37, label %168 [
    i8 0, label %43
    i8 1, label %38
  ]

38:
  br label %44

39:
  %40 = landingpad { ptr, i32 }
          cleanup
  %41 = extractvalue { ptr, i32 } %40, 0
  store ptr %41, ptr %7, align 8
  %42 = extractvalue { ptr, i32 } %40, 1
  store i32 %42, ptr %8, align 4
  br label %172

43:
  call void @_ZNKSt7__n486114suspend_always12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %9) #2
  br label %44

44:
  %45 = phi i32 [ 0, %43 ], [ 2, %38 ]
  call void @llvm.lifetime.end.p0(ptr %9) #2
  switch i32 %45, label %160 [
    i32 0, label %46
  ]

46:
  call void @llvm.lifetime.start.p0(ptr %11) #2
  store i32 0, ptr %11, align 4
  call void @llvm.lifetime.start.p0(ptr %12) #2
  %47 = getelementptr inbounds [16 x i8], ptr %12, i64 0, i64 0
  %48 = load i32, ptr %5, align 4
  %49 = call i32 (ptr, i64, ptr, ...) @snprintf(ptr noundef %47, i64 noundef 16, ptr noundef @.str, i32 noundef %48) #2
  call void @llvm.lifetime.start.p0(ptr %13) #2
  store i32 0, ptr %13, align 4
  br label %50

50:
  %51 = load i32, ptr %13, align 4
  %52 = load i32, ptr %5, align 4
  %53 = icmp slt i32 %51, %52
  br i1 %53, label %55, label %54

54:
  br label %101

55:
  call void @llvm.lifetime.start.p0(ptr %14) #2
  %56 = load i32, ptr %13, align 4
  invoke void @_Z4leafi(ptr dead_on_unwind writable sret(%struct.task) align 8 %14, i32 noundef %56)
          to label %57 unwind label %64

57:
  %58 = call noundef i1 @_ZN4task11await_readyEv(ptr noundef nonnull align 8 dereferenceable(8) %14) #2
  br i1 %58, label %72, label %59

59:
  %60 = call token @llvm.coro.save(ptr null)
  invoke void @llvm.coro.await.suspend.handle(ptr %14, ptr %27, ptr @_Z3sumi.__await_suspend_wrapper__await)
          to label %61 unwind label %68

61:
  %62 = call i8 @llvm.coro.suspend(token %60, i1 false)
  switch i8 %62, label %168 [
    i8 0, label %72
    i8 1, label %63
  ]

63:
  br label %77

64:
  %65 = landingpad { ptr, i32 }
          catch ptr @_ZTIi
          catch ptr null
  %66 = extractvalue { ptr, i32 } %65, 0
  store ptr %66, ptr %7, align 8
  %67 = extractvalue { ptr, i32 } %65, 1
  store i32 %67, ptr %8, align 4
  br label %80

68:
  %69 = landingpad { ptr, i32 }
          catch ptr @_ZTIi
          catch ptr null
  %70 = extractvalue { ptr, i32 } %69, 0
  store ptr %70, ptr %7, align 8
  %71 = extractvalue { ptr, i32 } %69, 1
  store i32 %71, ptr %8, align 4
  call void @_ZN4taskD2Ev(ptr noundef nonnull align 8 dereferenceable(8) %14) #2
  br label %80

72:
  %73 = invoke noundef i32 @_ZN4task12await_resumeEv(ptr noundef nonnull align 8 dereferenceable(8) %14)
          to label %74 unwind label %68

74:
  %75 = load i32, ptr %11, align 4
  %76 = add nsw i32 %75, %73
  store i32 %76, ptr %11, align 4
  br label %77

77:
  %78 = phi i32 [ 0, %74 ], [ 2, %63 ]
  call void @_ZN4taskD2Ev(ptr noundef nonnull align 8 dereferenceable(8) %14) #2
  call void @llvm.lifetime.end.p0(ptr %14) #2
  switch i32 %78, label %101 [
    i32 0, label %79
  ]

79:
  br label %93

80:
  call void @llvm.lifetime.end.p0(ptr %14) #2
  br label %81

81:
  %82 = load i32, ptr %8, align 4
  %83 = call i32 @llvm.eh.typeid.for.p0(ptr @_ZTIi) #2
  %84 = icmp eq i32 %82, %83
  br i1 %84, label %85, label %103

85:
  %86 = load ptr, ptr %7, align 8
  %87 = call ptr @__cxa_begin_catch(ptr %86) #2
  %88 = load i32, ptr %87, align 4
  store i32 %88, ptr %15, align 4
  %89 = load i32, ptr %15, align 4
  %90 = getelementptr inbounds [16 x i8], ptr %12, i64 0, i64 0
  %91 = invoke i32 (ptr, ...) @printf(ptr noundef @.str.1, i32 noundef %89, ptr noundef %90)
          to label %92 unwind label %97

92:
  call void @__cxa_end_catch() #2
  br label %93

93:
  br label %94

94:
  %95 = load i32, ptr %13, align 4
  %96 = add nsw i32 %95, 1
  store i32 %96, ptr %13, align 4
  br label %50, !llvm.loop !7

97:
  %98 = landingpad { ptr, i32 }
          catch ptr null
  %99 = extractvalue { ptr, i32 } %98, 0
  store ptr %99, ptr %7, align 8
  %100 = extractvalue { ptr, i32 } %98, 1
  store i32 %100, ptr %8, align 4
  call void @__cxa_end_catch() #2
  br label %103

101:
  %102 = phi i32 [ %78, %77 ], [ 4, %54 ]
  call void @llvm.lifetime.end.p0(ptr %13) #2
  switch i32 %102, label %133 [
    i32 4, label %104
  ]

103:
  call void @llvm.lifetime.end.p0(ptr %13) #2
  br label %135

104:
  call void @llvm.lifetime.start.p0(ptr %16) #2
  call void @llvm.memcpy.p0.p0.i64(ptr align 1 %16, ptr align 1 @__const._Z3sumi.twice, i64 1, i1 false)
  call void @llvm.lifetime.start.p0(ptr %17) #2
  %105 = load i32, ptr %11, align 4
  invoke void @"_ZZ3sumiENK3$_0clEi"(ptr dead_on_unwind writable sret(%struct.task) align 8 %17, ptr noundef nonnull align 1 dereferenceable(1) %16, i32 noundef %105)
          to label %106 unwind label %113

106:
  %107 = call noundef i1 @_ZN4task11await_readyEv(ptr noundef nonnull align 8 dereferenceable(8) %17) #2
  br i1 %107, label %121, label %108

108:
  %109 = call token @llvm.coro.save(ptr null)
  invoke void @llvm.coro.await.suspend.handle(ptr %17, ptr %27, ptr @_Z3sumi.__await_suspend_wrapper__await2)
          to label %110 unwind label %117

110:
  %111 = call i8 @llvm.coro.suspend(token %109, i1 false)
  switch i8 %111, label %168 [
    i8 0, label %121
    i8 1, label %112
  ]

112:
  br label %126

113:
  %114 = landingpad { ptr, i32 }
          catch ptr null
  %115 = extractvalue { ptr, i32 } %114, 0
  store ptr %115, ptr %7, align 8
  %116 = extractvalue { ptr, i32 } %114, 1
  store i32 %116, ptr %8, align 4
  br label %130

117:
  %118 = landingpad { ptr, i32 }
          catch ptr null
  %119 = extractvalue { ptr, i32 } %118, 0
  store ptr %119, ptr %7, align 8
  %120 = extractvalue { ptr, i32 } %118, 1
  store i32 %120, ptr %8, align 4
  call void @_ZN4taskD2Ev(ptr noundef nonnull align 8 dereferenceable(8) %17) #2
  br label %130

121:
  %122 = invoke noundef i32 @_ZN4task12await_resumeEv(ptr noundef nonnull align 8 dereferenceable(8) %17)
          to label %123 unwind label %117

123:
  %124 = load i32, ptr %11, align 4
  %125 = add nsw i32 %124, %122
  store i32 %125, ptr %11, align 4
  br label %126

126:
  %127 = phi i32 [ 0, %123 ], [ 2, %112 ]
  call void @_ZN4taskD2Ev(ptr noundef nonnull align 8 dereferenceable(8) %17) #2
  call void @llvm.lifetime.end.p0(ptr %17) #2
  switch i32 %127, label %131 [
    i32 0, label %128
  ]

128:
  %129 = load i32, ptr %11, align 4
  call void @_ZN4task12promise_type12return_valueEi(ptr noundef nonnull align 8 dereferenceable(24) %6, i32 noundef %129) #2
  br label %131

130:
  call void @llvm.lifetime.end.p0(ptr %17) #2
  call void @llvm.lifetime.end.p0(ptr %16) #2
  br label %135

131:
  %132 = phi i32 [ %127, %126 ], [ 3, %128 ]
  call void @llvm.lifetime.end.p0(ptr %16) #2
  br label %133

133:
  %134 = phi i32 [ %102, %101 ], [ %132, %131 ]
  call void @llvm.lifetime.end.p0(ptr %12) #2
  call void @llvm.lifetime.end.p0(ptr %11) #2
  switch i32 %134, label %160 [
    i32 3, label %141
  ]

135:
  call void @llvm.lifetime.end.p0(ptr %12) #2
  call void @llvm.lifetime.end.p0(ptr %11) #2
  br label %136

136:
  %137 = load ptr, ptr %7, align 8
  %138 = call ptr @__cxa_begin_catch(ptr %137) #2
  call void @_ZN4task12promise_type19unhandled_exceptionEv(ptr noundef nonnull align 8 dereferenceable(24) %6) #2
  invoke void @__cxa_end_catch()
          to label %139 unwind label %148

139:
  br label %140

140:
  br label %141

141:
  call void @llvm.lifetime.start.p0(ptr %18) #2
  call void @_ZN4task12promise_type13final_suspendEv(ptr noundef nonnull align 8 dereferenceable(24) %6) #2
  %142 = call noundef i1 @_ZN4task12promise_type3fin11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %18) #2
  br i1 %142, label %156, label %143

143:
  %144 = call token @llvm.coro.save(ptr null)
  invoke void @llvm.coro.await.suspend.handle(ptr %18, ptr %27, ptr @_Z3sumi.__await_suspend_wrapper__final)
          to label %145 unwind label %152

145:
  %146 = call i8 @llvm.coro.suspend(token %144, i1 true)
  switch i8 %146, label %168 [
    i8 0, label %156
    i8 1, label %147
  ]

147:
  br label %157

148:
  %149 = landingpad { ptr, i32 }
          cleanup
  %150 = extractvalue { ptr, i32 } %149, 0
  store ptr %150, ptr %7, align 8
  %151 = extractvalue { ptr, i32 } %149, 1
  store i32 %151, ptr %8, align 4
  br label %169

152:
  %153 = landingpad { ptr, i32 }
          cleanup
  %154 = extractvalue { ptr, i32 } %153, 0
  store ptr %154, ptr %7, align 8
  %155 = extractvalue { ptr, i32 } %153, 1
  store i32 %155, ptr %8, align 4
  call void @llvm.lifetime.end.p0(ptr %18) #2
  br label %169

156:
  call void @_ZN4task12promise_type3fin12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %18) #2
  br label %157

157:
  %158 = phi i32 [ 0, %156 ], [ 2, %147 ]
  call void @llvm.lifetime.end.p0(ptr %18) #2
  switch i32 %158, label %160 [
    i32 0, label %159
  ]

159:
  br label %160

160:
  %161 = phi i32 [ %45, %44 ], [ %134, %133 ], [ %158, %157 ], [ 0, %159 ]
  call void @_ZN4task12promise_typeD2Ev(ptr noundef nonnull align 8 dereferenceable(24) %6) #2
  call void @llvm.lifetime.end.p0(ptr %6) #2
  call void @llvm.lifetime.end.p0(ptr %5) #2
  %162 = call ptr @llvm.coro.free(token %20, ptr %27)
  %163 = icmp ne ptr %162, null
  br i1 %163, label %164, label %166

164:
  %165 = call i64 @llvm.coro.size.i64()
  call void @_ZdlPvm(ptr noundef %162, i64 noundef %165) #2
  br label %166

166:
  switch i32 %161, label %183 [
    i32 0, label %167
    i32 2, label %168
  ]

167:
  br label %168

168:
  call void @llvm.coro.end(ptr null, i1 false, token none)
  ret void

169:
  call void @llvm.coro.end(ptr null, i1 true, token none)
  %170 = call i1 @llvm.coro.is_in_ramp()
  br i1 %170, label %171, label %178

171:
  br label %172

172:
  call void @_ZN4task12promise_typeD2Ev(ptr noundef nonnull align 8 dereferenceable(24) %6) #2
  call void @llvm.lifetime.end.p0(ptr %6) #2
  call void @llvm.lifetime.end.p0(ptr %5) #2
  %173 = call ptr @llvm.coro.free(token %20, ptr %27)
  %174 = icmp ne ptr %173, null
  br i1 %174, label %175, label %177

175:
  %176 = call i64 @llvm.coro.size.i64()
  call void @_ZdlPvm(ptr noundef %173, i64 noundef %176) #2
  br label %177

177:
  br label %178

178:
  %179 = load ptr, ptr %7, align 8
  %180 = load i32, ptr %8, align 4
  %181 = insertvalue { ptr, i32 } poison, ptr %179, 0
  %182 = insertvalue { ptr, i32 } %181, i32 %180, 1
  resume { ptr, i32 } %182

183:
  unreachable
}

define internal void @_Z3sumi.__await_suspend_wrapper__init(ptr noundef nonnull %0, ptr noundef %1) #9 {
  %3 = alloca ptr, align 8
  %4 = alloca ptr, align 8
  %5 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %6 = alloca %"struct.std::__n4861::coroutine_handle", align 8
  store ptr %0, ptr %3, align 8
  store ptr %1, ptr %4, align 8
  %7 = load ptr, ptr %3, align 8
  %8 = load ptr, ptr %4, align 8
  %9 = call i64 @_ZNSt7__n486116coroutine_handleIN4task12promise_typeEE12from_addressEPv(ptr noundef %8) #2
  %10 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %6, i32 0, i32 0
  %11 = inttoptr i64 %9 to ptr
  store ptr %11, ptr %10, align 8
  %12 = call i64 @_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEEcvNS0_IvEEEv(ptr noundef nonnull align 8 dereferenceable(8) %6) #2
  %13 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %5, i32 0, i32 0
  %14 = inttoptr i64 %12 to ptr
  store ptr %14, ptr %13, align 8
  %15 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %5, i32 0, i32 0
  %16 = load ptr, ptr %15, align 8
  call void @_ZNKSt7__n486114suspend_always13await_suspendENS_16coroutine_handleIvEE(ptr noundef nonnull align 1 dereferenceable(1) %7, ptr %16) #2
  ret void
}

declare i32 @snprintf(ptr noundef, i64 noundef, ptr noundef, ...) #12

define linkonce_odr dso_local noundef i1 @_ZN4task11await_readyEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  ret i1 false
}

define internal noundef ptr @_Z3sumi.__await_suspend_wrapper__await(ptr noundef nonnull %0, ptr noundef %1) #9 {
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
  %11 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %7, i32 0, i32 0
  %12 = inttoptr i64 %10 to ptr
  store ptr %12, ptr %11, align 8
  %13 = call i64 @_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEEcvNS0_IvEEEv(ptr noundef nonnull align 8 dereferenceable(8) %7) #2
  %14 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %6, i32 0, i32 0
  %15 = inttoptr i64 %13 to ptr
  store ptr %15, ptr %14, align 8
  %16 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %6, i32 0, i32 0
  %17 = load ptr, ptr %16, align 8
  %18 = call i64 @_ZN4task13await_suspendENSt7__n486116coroutine_handleIvEE(ptr noundef nonnull align 8 dereferenceable(8) %8, ptr %17) #2
  %19 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %5, i32 0, i32 0
  %20 = inttoptr i64 %18 to ptr
  store ptr %20, ptr %19, align 8
  %21 = call noundef ptr @_ZNKSt7__n486116coroutine_handleIvE7addressEv(ptr noundef nonnull align 8 dereferenceable(8) %5) #2
  ret ptr %21
}

define linkonce_odr dso_local i64 @_ZN4task13await_suspendENSt7__n486116coroutine_handleIvEE(ptr noundef nonnull align 8 dereferenceable(8) %0, ptr %1) #6 comdat personality ptr @__gxx_personality_v0 {
  %3 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %4 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %5 = alloca ptr, align 8
  %6 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %4, i32 0, i32 0
  store ptr %1, ptr %6, align 8
  store ptr %0, ptr %5, align 8
  %7 = load ptr, ptr %5, align 8
  %8 = getelementptr inbounds nuw %struct.task, ptr %7, i32 0, i32 0
  %9 = invoke noundef nonnull align 8 dereferenceable(24) ptr @_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEE7promiseEv(ptr noundef nonnull align 8 dereferenceable(8) %8)
          to label %10 unwind label %19

10:
  %11 = getelementptr inbounds nuw %"struct.task::promise_type", ptr %9, i32 0, i32 2
  call void @llvm.memcpy.p0.p0.i64(ptr align 8 %11, ptr align 8 %4, i64 8, i1 false)
  %12 = getelementptr inbounds nuw %struct.task, ptr %7, i32 0, i32 0
  %13 = call i64 @_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEEcvNS0_IvEEEv(ptr noundef nonnull align 8 dereferenceable(8) %12) #2
  %14 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %3, i32 0, i32 0
  %15 = inttoptr i64 %13 to ptr
  store ptr %15, ptr %14, align 8
  %16 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %3, i32 0, i32 0
  %17 = load ptr, ptr %16, align 8
  %18 = ptrtoint ptr %17 to i64
  ret i64 %18

19:
  %20 = landingpad { ptr, i32 }
          catch ptr null
  %21 = extractvalue { ptr, i32 } %20, 0
  call void @__clang_call_terminate(ptr %21) #21
  unreachable
}

define linkonce_odr dso_local noundef i32 @_ZN4task12await_resumeEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #7 comdat personality ptr @__gxx_personality_v0 {
  %2 = alloca ptr, align 8
  %3 = alloca %"class.std::__exception_ptr::exception_ptr", align 8
  %4 = alloca ptr, align 8
  %5 = alloca i32, align 4
  store ptr %0, ptr %2, align 8
  %6 = load ptr, ptr %2, align 8
  %7 = getelementptr inbounds nuw %struct.task, ptr %6, i32 0, i32 0
  %8 = call noundef nonnull align 8 dereferenceable(24) ptr @_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEE7promiseEv(ptr noundef nonnull align 8 dereferenceable(8) %7)
  %9 = getelementptr inbounds nuw %"struct.task::promise_type", ptr %8, i32 0, i32 1
  %10 = call noundef i1 @_ZNKSt15__exception_ptr13exception_ptrcvbEv(ptr noundef nonnull align 8 dereferenceable(8) %9) #2
  br i1 %10, label %11, label %20

11:
  %12 = getelementptr inbounds nuw %struct.task, ptr %6, i32 0, i32 0
  %13 = call noundef nonnull align 8 dereferenceable(24) ptr @_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEE7promiseEv(ptr noundef nonnull align 8 dereferenceable(8) %12)
  %14 = getelementptr inbounds nuw %"struct.task::promise_type", ptr %13, i32 0, i32 1
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
  %21 = getelementptr inbounds nuw %struct.task, ptr %6, i32 0, i32 0
  %22 = call noundef nonnull align 8 dereferenceable(24) ptr @_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEE7promiseEv(ptr noundef nonnull align 8 dereferenceable(8) %21)
  %23 = getelementptr inbounds nuw %"struct.task::promise_type", ptr %22, i32 0, i32 0
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
  %4 = getelementptr inbounds nuw %struct.task, ptr %3, i32 0, i32 0
  %5 = call noundef i1 @_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEEcvbEv(ptr noundef nonnull align 8 dereferenceable(8) %4) #2
  br i1 %5, label %6, label %9

6:
  %7 = getelementptr inbounds nuw %struct.task, ptr %3, i32 0, i32 0
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

declare void @llvm.memcpy.p0.p0.i64(ptr noalias writeonly captures(none), ptr noalias readonly captures(none), i64, i1 immarg) #14

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
  %16 = call token @llvm.coro.id(i32 16, ptr %8, ptr null, ptr null)
  %17 = call i1 @llvm.coro.alloc(token %16)
  br i1 %17, label %18, label %21

18:
  %19 = call i64 @llvm.coro.size.i64()
  %20 = call noalias noundef nonnull ptr @_Znwm(i64 noundef %19) #19
  br label %21

21:
  %22 = phi ptr [ null, %3 ], [ %20, %18 ]
  %23 = call ptr @llvm.coro.begin(token %16, ptr %22)
  call void @llvm.lifetime.start.p0(ptr %7) #2
  %24 = load i32, ptr %6, align 4
  store i32 %24, ptr %7, align 4
  call void @llvm.lifetime.start.p0(ptr %8) #2
  %25 = getelementptr inbounds nuw %"struct.task::promise_type", ptr %8, i32 0, i32 0
  %26 = load i32, ptr %7, align 4
  store i32 %26, ptr %25, align 8
  %27 = getelementptr inbounds nuw %"struct.task::promise_type", ptr %8, i32 0, i32 1
  call void @_ZNSt15__exception_ptr13exception_ptrC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %27) #2
  %28 = getelementptr inbounds nuw %"struct.task::promise_type", ptr %8, i32 0, i32 2
  call void @_ZNSt7__n486116coroutine_handleIvEC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %28) #2
  invoke void @_ZN4task12promise_type17get_return_objectEv(ptr dead_on_unwind writable sret(%struct.task) align 8 %0, ptr noundef nonnull align 8 dereferenceable(24) %8)
          to label %29 unwind label %35

29:
  call void @llvm.lifetime.start.p0(ptr %11) #2
  call void @_ZN4task12promise_type15initial_suspendEv(ptr noundef nonnull align 8 dereferenceable(24) %8) #2
  %30 = call noundef i1 @_ZNKSt7__n486114suspend_always11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %11) #2
  br i1 %30, label %39, label %31

31:
  %32 = call token @llvm.coro.save(ptr null)
  call void @llvm.coro.await.suspend.void(ptr %11, ptr %23, ptr @"_ZZ3sumiENK3$_0clEi.__await_suspend_wrapper__init") #2
  %33 = call i8 @llvm.coro.suspend(token %32, i1 false)
  switch i8 %33, label %70 [
    i8 0, label %39
    i8 1, label %34
  ]

34:
  br label %40

35:
  %36 = landingpad { ptr, i32 }
          cleanup
  %37 = extractvalue { ptr, i32 } %36, 0
  store ptr %37, ptr %9, align 8
  %38 = extractvalue { ptr, i32 } %36, 1
  store i32 %38, ptr %10, align 4
  br label %72

39:
  call void @_ZNKSt7__n486114suspend_always12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %11) #2
  br label %40

40:
  %41 = phi i32 [ 0, %39 ], [ 2, %34 ]
  call void @llvm.lifetime.end.p0(ptr %11) #2
  switch i32 %41, label %62 [
    i32 0, label %42
  ]

42:
  %43 = load i32, ptr %7, align 4
  %44 = load i32, ptr %7, align 4
  %45 = add nsw i32 %43, %44
  call void @_ZN4task12promise_type12return_valueEi(ptr noundef nonnull align 8 dereferenceable(24) %8, i32 noundef %45) #2
  br label %46

46:
  call void @llvm.lifetime.start.p0(ptr %13) #2
  call void @_ZN4task12promise_type13final_suspendEv(ptr noundef nonnull align 8 dereferenceable(24) %8) #2
  %47 = call noundef i1 @_ZN4task12promise_type3fin11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %13) #2
  br i1 %47, label %58, label %48

48:
  %49 = call token @llvm.coro.save(ptr null)
  invoke void @llvm.coro.await.suspend.handle(ptr %13, ptr %23, ptr @"_ZZ3sumiENK3$_0clEi.__await_suspend_wrapper__final")
          to label %50 unwind label %53

50:
  %51 = call i8 @llvm.coro.suspend(token %49, i1 true)
  switch i8 %51, label %70 [
    i8 0, label %58
    i8 1, label %52
  ]

52:
  br label %59

53:
  %54 = landingpad { ptr, i32 }
          cleanup
  %55 = extractvalue { ptr, i32 } %54, 0
  store ptr %55, ptr %9, align 8
  %56 = extractvalue { ptr, i32 } %54, 1
  store i32 %56, ptr %10, align 4
  call void @llvm.lifetime.end.p0(ptr %13) #2
  call void @llvm.coro.end(ptr null, i1 true, token none)
  %57 = call i1 @llvm.coro.is_in_ramp()
  br i1 %57, label %71, label %78

58:
  call void @_ZN4task12promise_type3fin12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %13) #2
  br label %59

59:
  %60 = phi i32 [ 0, %58 ], [ 2, %52 ]
  call void @llvm.lifetime.end.p0(ptr %13) #2
  switch i32 %60, label %62 [
    i32 0, label %61
  ]

61:
  br label %62

62:
  %63 = phi i32 [ %41, %40 ], [ %60, %59 ], [ 0, %61 ]
  call void @_ZN4task12promise_typeD2Ev(ptr noundef nonnull align 8 dereferenceable(24) %8) #2
  call void @llvm.lifetime.end.p0(ptr %8) #2
  call void @llvm.lifetime.end.p0(ptr %7) #2
  %64 = call ptr @llvm.coro.free(token %16, ptr %23)
  %65 = icmp ne ptr %64, null
  br i1 %65, label %66, label %68

66:
  %67 = call i64 @llvm.coro.size.i64()
  call void @_ZdlPvm(ptr noundef %64, i64 noundef %67) #2
  br label %68

68:
  switch i32 %63, label %83 [
    i32 0, label %69
    i32 2, label %70
  ]

69:
  br label %70

70:
  call void @llvm.coro.end(ptr null, i1 false, token none)
  ret void

71:
  br label %72

72:
  call void @_ZN4task12promise_typeD2Ev(ptr noundef nonnull align 8 dereferenceable(24) %8) #2
  call void @llvm.lifetime.end.p0(ptr %8) #2
  call void @llvm.lifetime.end.p0(ptr %7) #2
  %73 = call ptr @llvm.coro.free(token %16, ptr %23)
  %74 = icmp ne ptr %73, null
  br i1 %74, label %75, label %77

75:
  %76 = call i64 @llvm.coro.size.i64()
  call void @_ZdlPvm(ptr noundef %73, i64 noundef %76) #2
  br label %77

77:
  br label %78

78:
  %79 = load ptr, ptr %9, align 8
  %80 = load i32, ptr %10, align 4
  %81 = insertvalue { ptr, i32 } poison, ptr %79, 0
  %82 = insertvalue { ptr, i32 } %81, i32 %80, 1
  resume { ptr, i32 } %82

83:
  unreachable
}

define internal noundef ptr @_Z3sumi.__await_suspend_wrapper__await2(ptr noundef nonnull %0, ptr noundef %1) #9 {
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
  %11 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %7, i32 0, i32 0
  %12 = inttoptr i64 %10 to ptr
  store ptr %12, ptr %11, align 8
  %13 = call i64 @_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEEcvNS0_IvEEEv(ptr noundef nonnull align 8 dereferenceable(8) %7) #2
  %14 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %6, i32 0, i32 0
  %15 = inttoptr i64 %13 to ptr
  store ptr %15, ptr %14, align 8
  %16 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %6, i32 0, i32 0
  %17 = load ptr, ptr %16, align 8
  %18 = call i64 @_ZN4task13await_suspendENSt7__n486116coroutine_handleIvEE(ptr noundef nonnull align 8 dereferenceable(8) %8, ptr %17) #2
  %19 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %5, i32 0, i32 0
  %20 = inttoptr i64 %18 to ptr
  store ptr %20, ptr %19, align 8
  %21 = call noundef ptr @_ZNKSt7__n486116coroutine_handleIvE7addressEv(ptr noundef nonnull align 8 dereferenceable(8) %5) #2
  ret ptr %21
}

define internal noundef ptr @_Z3sumi.__await_suspend_wrapper__final(ptr noundef nonnull %0, ptr noundef %1) #9 {
  %3 = alloca ptr, align 8
  %4 = alloca ptr, align 8
  %5 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %6 = alloca %"struct.std::__n4861::coroutine_handle", align 8
  store ptr %0, ptr %3, align 8
  store ptr %1, ptr %4, align 8
  %7 = load ptr, ptr %3, align 8
  %8 = load ptr, ptr %4, align 8
  %9 = call i64 @_ZNSt7__n486116coroutine_handleIN4task12promise_typeEE12from_addressEPv(ptr noundef %8) #2
  %10 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %6, i32 0, i32 0
  %11 = inttoptr i64 %9 to ptr
  store ptr %11, ptr %10, align 8
  %12 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %6, i32 0, i32 0
  %13 = load ptr, ptr %12, align 8
  %14 = call i64 @_ZN4task12promise_type3fin13await_suspendENSt7__n486116coroutine_handleIS0_EE(ptr noundef nonnull align 1 dereferenceable(1) %7, ptr %13) #2
  %15 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %5, i32 0, i32 0
  %16 = inttoptr i64 %14 to ptr
  store ptr %16, ptr %15, align 8
  %17 = call noundef ptr @_ZNKSt7__n486116coroutine_handleIvE7addressEv(ptr noundef nonnull align 8 dereferenceable(8) %5) #2
  ret ptr %17
}

define dso_local noundef i32 @main() #15 personality ptr @__gxx_personality_v0 {
  %1 = alloca i32, align 4
  %2 = alloca %struct.task, align 8
  %3 = alloca ptr, align 8
  %4 = alloca i32, align 4
  store i32 0, ptr %1, align 4
  call void @_Z3sumi(ptr dead_on_unwind writable sret(%struct.task) align 8 %2, i32 noundef 5)
  %5 = getelementptr inbounds nuw %struct.task, ptr %2, i32 0, i32 0
  invoke void @_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEE6resumeEv(ptr noundef nonnull align 8 dereferenceable(8) %5)
          to label %6 unwind label %15

6:
  %7 = getelementptr inbounds nuw %struct.task, ptr %2, i32 0, i32 0
  %8 = invoke noundef nonnull align 8 dereferenceable(24) ptr @_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEE7promiseEv(ptr noundef nonnull align 8 dereferenceable(8) %7)
          to label %9 unwind label %15

9:
  %10 = getelementptr inbounds nuw %"struct.task::promise_type", ptr %8, i32 0, i32 0
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
  %4 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %3, i32 0, i32 0
  %5 = load ptr, ptr %4, align 8
  call void @llvm.coro.resume(ptr %5)
  ret void
}

define linkonce_odr dso_local noundef nonnull align 8 dereferenceable(24) ptr @_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEE7promiseEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #6 comdat {
  %2 = alloca ptr, align 8
  %3 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %4 = load ptr, ptr %2, align 8
  %5 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %4, i32 0, i32 0
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
  %4 = load ptr, ptr %3, align 8, !nonnull !6, !align !9
  %5 = call ptr @llvm.coro.promise(ptr %4, i32 8, i1 true)
  %6 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %2, i32 0, i32 0
  store ptr %5, ptr %6, align 8
  %7 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %2, i32 0, i32 0
  %8 = load ptr, ptr %7, align 8
  %9 = ptrtoint ptr %8 to i64
  ret i64 %9
}

define linkonce_odr dso_local void @_ZNSt7__n486116coroutine_handleIN4task12promise_typeEEC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %0) unnamed_addr #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %3, i32 0, i32 0
  store ptr null, ptr %4, align 8
  ret void
}

declare ptr @llvm.coro.promise(ptr captures(none), i32, i1) #4

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

define linkonce_odr dso_local noundef ptr @_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEE7addressEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %3, i32 0, i32 0
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
  %7 = load ptr, ptr %4, align 8, !nonnull !6, !align !9
  call void @_ZNSt15__exception_ptr13exception_ptrC2EOS0_(ptr noundef nonnull align 8 dereferenceable(8) %5, ptr noundef nonnull align 8 dereferenceable(8) %7) #2
  call void @_ZNSt15__exception_ptr13exception_ptr4swapERS0_(ptr noundef nonnull align 8 dereferenceable(8) %5, ptr noundef nonnull align 8 dereferenceable(8) %6) #2
  call void @_ZNSt15__exception_ptr13exception_ptrD2Ev(ptr noundef nonnull align 8 dereferenceable(8) %5) #2
  ret ptr %6
}

define linkonce_odr dso_local void @_ZNSt15__exception_ptr13exception_ptrD2Ev(ptr noundef nonnull align 8 dereferenceable(8) %0) unnamed_addr #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds nuw %"class.std::__exception_ptr::exception_ptr", ptr %3, i32 0, i32 0
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
  %6 = getelementptr inbounds nuw %"class.std::__exception_ptr::exception_ptr", ptr %5, i32 0, i32 0
  %7 = load ptr, ptr %4, align 8, !nonnull !6, !align !9
  %8 = getelementptr inbounds nuw %"class.std::__exception_ptr::exception_ptr", ptr %7, i32 0, i32 0
  %9 = load ptr, ptr %8, align 8
  store ptr %9, ptr %6, align 8
  %10 = load ptr, ptr %4, align 8, !nonnull !6, !align !9
  %11 = getelementptr inbounds nuw %"class.std::__exception_ptr::exception_ptr", ptr %10, i32 0, i32 0
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
  %7 = getelementptr inbounds nuw %"class.std::__exception_ptr::exception_ptr", ptr %6, i32 0, i32 0
  %8 = load ptr, ptr %7, align 8
  store ptr %8, ptr %5, align 8
  %9 = load ptr, ptr %4, align 8, !nonnull !6, !align !9
  %10 = getelementptr inbounds nuw %"class.std::__exception_ptr::exception_ptr", ptr %9, i32 0, i32 0
  %11 = load ptr, ptr %10, align 8
  %12 = getelementptr inbounds nuw %"class.std::__exception_ptr::exception_ptr", ptr %6, i32 0, i32 0
  store ptr %11, ptr %12, align 8
  %13 = load ptr, ptr %5, align 8
  %14 = load ptr, ptr %4, align 8, !nonnull !6, !align !9
  %15 = getelementptr inbounds nuw %"class.std::__exception_ptr::exception_ptr", ptr %14, i32 0, i32 0
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
  %4 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %3, i32 0, i32 0
  %5 = load ptr, ptr %4, align 8
  %6 = icmp ne ptr %5, null
  ret i1 %6
}

define linkonce_odr dso_local i64 @_ZNSt7__n486114noop_coroutineEv() #6 comdat {
  %1 = alloca %"struct.std::__n4861::coroutine_handle.1", align 8
  call void @llvm.memset.p0.i64(ptr align 8 %1, i8 0, i64 8, i1 false)
  call void @_ZNSt7__n486116coroutine_handleINS_22noop_coroutine_promiseEEC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %1) #2
  %2 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.1", ptr %1, i32 0, i32 0
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
  %7 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %2, i32 0, i32 0
  %8 = inttoptr i64 %6 to ptr
  store ptr %8, ptr %7, align 8
  %9 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %2, i32 0, i32 0
  %10 = load ptr, ptr %9, align 8
  %11 = ptrtoint ptr %10 to i64
  ret i64 %11
}

declare void @llvm.memset.p0.i64(ptr writeonly captures(none), i8, i64, i1 immarg) #17

define linkonce_odr dso_local void @_ZNSt7__n486116coroutine_handleINS_22noop_coroutine_promiseEEC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %0) unnamed_addr #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.1", ptr %3, i32 0, i32 0
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
  %4 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.1", ptr %3, i32 0, i32 0
  %5 = load ptr, ptr %4, align 8
  ret ptr %5
}

define linkonce_odr dso_local noundef i1 @_ZNKSt15__exception_ptr13exception_ptrcvbEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds nuw %"class.std::__exception_ptr::exception_ptr", ptr %3, i32 0, i32 0
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
  %6 = getelementptr inbounds nuw %"class.std::__exception_ptr::exception_ptr", ptr %5, i32 0, i32 0
  %7 = load ptr, ptr %4, align 8, !nonnull !6, !align !9
  %8 = getelementptr inbounds nuw %"class.std::__exception_ptr::exception_ptr", ptr %7, i32 0, i32 0
  %9 = load ptr, ptr %8, align 8
  store ptr %9, ptr %6, align 8
  %10 = getelementptr inbounds nuw %"class.std::__exception_ptr::exception_ptr", ptr %5, i32 0, i32 0
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
  %4 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %3, i32 0, i32 0
  %5 = load ptr, ptr %4, align 8
  %6 = icmp ne ptr %5, null
  ret i1 %6
}

define linkonce_odr dso_local void @_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEE7destroyEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %3, i32 0, i32 0
  %5 = load ptr, ptr %4, align 8
  call void @llvm.coro.destroy(ptr %5)
  ret void
}

declare void @llvm.coro.destroy(ptr)

define internal void @"_ZZ3sumiENK3$_0clEi.__await_suspend_wrapper__init"(ptr noundef nonnull %0, ptr noundef %1) #9 {
  %3 = alloca ptr, align 8
  %4 = alloca ptr, align 8
  %5 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %6 = alloca %"struct.std::__n4861::coroutine_handle", align 8
  store ptr %0, ptr %3, align 8
  store ptr %1, ptr %4, align 8
  %7 = load ptr, ptr %3, align 8
  %8 = load ptr, ptr %4, align 8
  %9 = call i64 @_ZNSt7__n486116coroutine_handleIN4task12promise_typeEE12from_addressEPv(ptr noundef %8) #2
  %10 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %6, i32 0, i32 0
  %11 = inttoptr i64 %9 to ptr
  store ptr %11, ptr %10, align 8
  %12 = call i64 @_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEEcvNS0_IvEEEv(ptr noundef nonnull align 8 dereferenceable(8) %6) #2
  %13 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %5, i32 0, i32 0
  %14 = inttoptr i64 %12 to ptr
  store ptr %14, ptr %13, align 8
  %15 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %5, i32 0, i32 0
  %16 = load ptr, ptr %15, align 8
  call void @_ZNKSt7__n486114suspend_always13await_suspendENS_16coroutine_handleIvEE(ptr noundef nonnull align 1 dereferenceable(1) %7, ptr %16) #2
  ret void
}

define internal noundef ptr @"_ZZ3sumiENK3$_0clEi.__await_suspend_wrapper__final"(ptr noundef nonnull %0, ptr noundef %1) #9 {
  %3 = alloca ptr, align 8
  %4 = alloca ptr, align 8
  %5 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %6 = alloca %"struct.std::__n4861::coroutine_handle", align 8
  store ptr %0, ptr %3, align 8
  store ptr %1, ptr %4, align 8
  %7 = load ptr, ptr %3, align 8
  %8 = load ptr, ptr %4, align 8
  %9 = call i64 @_ZNSt7__n486116coroutine_handleIN4task12promise_typeEE12from_addressEPv(ptr noundef %8) #2
  %10 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %6, i32 0, i32 0
  %11 = inttoptr i64 %9 to ptr
  store ptr %11, ptr %10, align 8
  %12 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %6, i32 0, i32 0
  %13 = load ptr, ptr %12, align 8
  %14 = call i64 @_ZN4task12promise_type3fin13await_suspendENSt7__n486116coroutine_handleIS0_EE(ptr noundef nonnull align 1 dereferenceable(1) %7, ptr %13) #2
  %15 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %5, i32 0, i32 0
  %16 = inttoptr i64 %14 to ptr
  store ptr %16, ptr %15, align 8
  %17 = call noundef ptr @_ZNKSt7__n486116coroutine_handleIvE7addressEv(ptr noundef nonnull align 8 dereferenceable(8) %5) #2
  ret ptr %17
}

declare void @llvm.coro.resume(ptr)

attributes #0 = { mustprogress noinline optnone presplitcoroutine sspstrong uwtable "frame-pointer"="non-leaf-no-reserve" "no-trapping-math"="true" "stack-protector-buffer-size"="8" }
attributes #1 = { nocallback nofree nosync nounwind willreturn memory(argmem: read) }
attributes #2 = { nounwind }
attributes #3 = { nobuiltin allocsize(0) "frame-pointer"="non-leaf-no-reserve" "no-trapping-math"="true" "stack-protector-buffer-size"="8" }
attributes #4 = { nounwind memory(none) }
attributes #5 = { nocallback nofree nosync nounwind willreturn memory(argmem: readwrite) }
attributes #6 = { mustprogress noinline nounwind optnone sspstrong uwtable "frame-pointer"="non-leaf-no-reserve" "no-trapping-math"="true" "stack-protector-buffer-size"="8" }
attributes #7 = { mustprogress noinline optnone sspstrong uwtable "frame-pointer"="non-leaf-no-reserve" "no-trapping-math"="true" "stack-protector-buffer-size"="8" }
attributes #8 = { nomerge nounwind }
attributes #9 = { alwaysinline mustprogress "sample-profile-suffix-elision-policy"="selected" }
attributes #10 = { nobuiltin nounwind "frame-pointer"="non-leaf-no-reserve" "no-trapping-math"="true" "stack-protector-buffer-size"="8" }
attributes #11 = { nounwind memory(argmem: read) }
attributes #12 = { nounwind "frame-pointer"="non-leaf-no-reserve" "no-trapping-math"="true" "stack-protector-buffer-size"="8" }
attributes #13 = { "frame-pointer"="non-leaf-no-reserve" "no-trapping-math"="true" "stack-protector-buffer-size"="8" }
attributes #14 = { nocallback nofree nounwind willreturn memory(argmem: readwrite) }
attributes #15 = { mustprogress noinline norecurse optnone sspstrong uwtable "frame-pointer"="non-leaf-no-reserve" "no-trapping-math"="true" "stack-protector-buffer-size"="8" }
attributes #16 = { noinline noreturn nounwind sspstrong uwtable "frame-pointer"="non-leaf-no-reserve" "no-trapping-math"="true" "stack-protector-buffer-size"="8" }
attributes #17 = { nocallback nofree nounwind willreturn memory(argmem: write) }
attributes #18 = { noreturn "frame-pointer"="non-leaf-no-reserve" "no-trapping-math"="true" "stack-protector-buffer-size"="8" }
attributes #19 = { allocsize(0) }
attributes #20 = { noreturn }
attributes #21 = { noreturn nounwind }

!llvm.module.flags = !{!0, !1, !2, !3, !4}
!llvm.ident = !{!5}

!0 = !{i32 1, !"wchar_size", i32 4}
!1 = !{i32 8, !"PIC Level", i32 2}
!2 = !{i32 7, !"PIE Level", i32 2}
!3 = !{i32 7, !"uwtable", i32 2}
!4 = !{i32 7, !"frame-pointer", i32 4}
!5 = !{!"kiln version 1.2.0"}
!6 = !{}
!7 = distinct !{!7, !8}
!8 = !{!"llvm.loop.mustprogress"}
!9 = !{i64 8}
