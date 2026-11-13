; clang++-22 -std=c++20 -O0 -S -emit-llvm -Xclang -disable-llvm-passes
; (aarch64-unknown-linux-gnu; the target triple, datalayout, target-cpu and
; target-features removed, so that the seed runs on every target) of:
;
; // Symmetric transfer runs in constant stack (design 0005 §1.2, §3.3,
; // spec/codegen.md §7.6): a million awaits of a task whose await_suspend
; // returns the awaited coroutine's handle, and whose final await_suspend
; // returns the awaiting one's, so two transfers each. Every activation of
; // `step` must run at the same stack depth ("flat 1"); a transfer made as
; // a call nests two frames per await and overflows an 8 MB stack.
; // `lazy` awaits from resumed activations (the transfer is a tail jump out
; // of the body); `eager` makes its first await from its ramp (the ramp
; // calls the resume function, then returns once the chain comes back).
; #include <coroutine>
; #include <cstdint>
; #include <cstdio>
; __attribute__((noinline)) static std::uintptr_t here() {
;   volatile char c = 0;
;   return reinterpret_cast<std::uintptr_t>(&c);
; }
; static std::uintptr_t lo = UINTPTR_MAX, hi = 0;
; static void probe() {
;   std::uintptr_t p = here();
;   if (p < lo) lo = p;
;   if (p > hi) hi = p;
; }
; template <bool Eager> struct task {
;   struct promise_type {
;     std::coroutine_handle<> cont;
;     task get_return_object() { return task{std::coroutine_handle<promise_type>::from_promise(*this)}; }
;     auto initial_suspend() noexcept {
;       if constexpr (Eager) return std::suspend_never{};
;       else return std::suspend_always{};
;     }
;     struct fin {
;       bool await_ready() noexcept { return false; }
;       std::coroutine_handle<> await_suspend(std::coroutine_handle<promise_type> h) noexcept {
;         auto c = h.promise().cont;
;         return c ? c : std::noop_coroutine();
;       }
;       void await_resume() noexcept {}
;     };
;     fin final_suspend() noexcept { return {}; }
;     void return_void() {}
;     void unhandled_exception() {}
;   };
;   std::coroutine_handle<promise_type> h;
;   bool await_ready() { return false; }
;   std::coroutine_handle<> await_suspend(std::coroutine_handle<> c) {
;     h.promise().cont = c;
;     return h;
;   }
;   void await_resume() {}
;   ~task() { if (h) h.destroy(); }
; };
; task<false> step(long& s) { ++s; probe(); co_return; }
; task<false> lazy(long& s, long n) { for (long i = 0; i < n; ++i) co_await step(s); }
; task<true> eager(long& s, long n) { for (long i = 0; i < n; ++i) co_await step(s); }
; int main() {
;   long s = 0;
;   {
;     task<false> t = lazy(s, 1000000);
;     t.h.resume();
;     std::printf("lazy %ld done %d flat %d\n", s, (int)t.h.done(), (int)(hi == lo));
;   }
;   s = 0, lo = UINTPTR_MAX, hi = 0;
;   {
;     task<true> t = eager(s, 1000000);
;     std::printf("eager %ld done %d flat %d\n", s, (int)t.h.done(), (int)(hi == lo));
;   }
; }
;
;@ covers: spec/ir.md §5.5 intrinsics (symmetric transfer; spec/codegen.md §7.6)
;@ input: pre-opt
;@ dialect: 22
;@ link: c++
;@ stdout: lazy 1000000 done 1 flat 1
;@ stdout: eager 1000000 done 1 flat 1

source_filename = "deep-transfer.cpp"

%struct.task = type { %"struct.std::__n4861::coroutine_handle" }
%"struct.std::__n4861::coroutine_handle" = type { ptr }
%"struct.task<false>::promise_type" = type { %"struct.std::__n4861::coroutine_handle.0" }
%"struct.std::__n4861::coroutine_handle.0" = type { ptr }
%"struct.std::__n4861::suspend_always" = type { i8 }
%"struct.task<false>::promise_type::fin" = type { i8 }
%"struct.std::__n4861::coroutine_handle.3" = type { ptr }
%struct.task.1 = type { %"struct.std::__n4861::coroutine_handle.2" }
%"struct.std::__n4861::coroutine_handle.2" = type { ptr }
%"struct.task<true>::promise_type" = type { %"struct.std::__n4861::coroutine_handle.0" }
%"struct.std::__n4861::suspend_never" = type { i8 }
%"struct.task<true>::promise_type::fin" = type { i8 }

$_ZN4taskILb0EE12promise_typeC2Ev = comdat any

$_ZN4taskILb0EE12promise_type17get_return_objectEv = comdat any

$_ZN4taskILb0EE12promise_type15initial_suspendEv = comdat any

$_ZNKSt7__n486114suspend_always11await_readyEv = comdat any

$_ZNKSt7__n486114suspend_always13await_suspendENS_16coroutine_handleIvEE = comdat any

$_ZNSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEE12from_addressEPv = comdat any

$_ZNKSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEEcvNS0_IvEEEv = comdat any

$_ZNKSt7__n486114suspend_always12await_resumeEv = comdat any

$_ZN4taskILb0EE12promise_type11return_voidEv = comdat any

$_ZN4taskILb0EE12promise_type19unhandled_exceptionEv = comdat any

$__clang_call_terminate = comdat any

$_ZN4taskILb0EE12promise_type13final_suspendEv = comdat any

$_ZN4taskILb0EE12promise_type3fin11await_readyEv = comdat any

$_ZN4taskILb0EE12promise_type3fin13await_suspendENSt7__n486116coroutine_handleIS1_EE = comdat any

$_ZNKSt7__n486116coroutine_handleIvE7addressEv = comdat any

$_ZN4taskILb0EE12promise_type3fin12await_resumeEv = comdat any

$_ZN4taskILb0EE11await_readyEv = comdat any

$_ZN4taskILb0EE13await_suspendENSt7__n486116coroutine_handleIvEE = comdat any

$_ZN4taskILb0EE12await_resumeEv = comdat any

$_ZN4taskILb0EED2Ev = comdat any

$_ZN4taskILb1EE12promise_typeC2Ev = comdat any

$_ZN4taskILb1EE12promise_type17get_return_objectEv = comdat any

$_ZN4taskILb1EE12promise_type15initial_suspendEv = comdat any

$_ZNKSt7__n486113suspend_never11await_readyEv = comdat any

$_ZNKSt7__n486113suspend_never13await_suspendENS_16coroutine_handleIvEE = comdat any

$_ZNSt7__n486116coroutine_handleIN4taskILb1EE12promise_typeEE12from_addressEPv = comdat any

$_ZNKSt7__n486116coroutine_handleIN4taskILb1EE12promise_typeEEcvNS0_IvEEEv = comdat any

$_ZNKSt7__n486113suspend_never12await_resumeEv = comdat any

$_ZN4taskILb1EE12promise_type11return_voidEv = comdat any

$_ZN4taskILb1EE12promise_type19unhandled_exceptionEv = comdat any

$_ZN4taskILb1EE12promise_type13final_suspendEv = comdat any

$_ZN4taskILb1EE12promise_type3fin11await_readyEv = comdat any

$_ZN4taskILb1EE12promise_type3fin13await_suspendENSt7__n486116coroutine_handleIS1_EE = comdat any

$_ZN4taskILb1EE12promise_type3fin12await_resumeEv = comdat any

$_ZNKSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEE6resumeEv = comdat any

$_ZNKSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEE4doneEv = comdat any

$_ZNKSt7__n486116coroutine_handleIN4taskILb1EE12promise_typeEE4doneEv = comdat any

$_ZN4taskILb1EED2Ev = comdat any

$_ZNSt7__n486116coroutine_handleIvEC2Ev = comdat any

$_ZNSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEEC2Ev = comdat any

$_ZNSt7__n486116coroutine_handleIvE12from_addressEPv = comdat any

$_ZNKSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEE7addressEv = comdat any

$_ZNSt7__n486116coroutine_handleIN4taskILb1EE12promise_typeEEC2Ev = comdat any

$_ZNKSt7__n486116coroutine_handleIN4taskILb1EE12promise_typeEE7addressEv = comdat any

$_ZNKSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEE7promiseEv = comdat any

$_ZNKSt7__n486116coroutine_handleIvEcvbEv = comdat any

$_ZNSt7__n486114noop_coroutineEv = comdat any

$_ZNKSt7__n486116coroutine_handleINS_22noop_coroutine_promiseEEcvNS0_IvEEEv = comdat any

$_ZNSt7__n486116coroutine_handleINS_22noop_coroutine_promiseEEC2Ev = comdat any

$_ZNSt7__n486116coroutine_handleINS_22noop_coroutine_promiseEE7__frame22__dummy_resume_destroyEv = comdat any

$_ZNKSt7__n486116coroutine_handleINS_22noop_coroutine_promiseEE7addressEv = comdat any

$_ZNSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEE12from_promiseERS3_ = comdat any

$_ZNKSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEEcvbEv = comdat any

$_ZNKSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEE7destroyEv = comdat any

$_ZNKSt7__n486116coroutine_handleIN4taskILb1EE12promise_typeEE7promiseEv = comdat any

$_ZNSt7__n486116coroutine_handleIN4taskILb1EE12promise_typeEE12from_promiseERS3_ = comdat any

$_ZNKSt7__n486116coroutine_handleIN4taskILb1EE12promise_typeEEcvbEv = comdat any

$_ZNKSt7__n486116coroutine_handleIN4taskILb1EE12promise_typeEE7destroyEv = comdat any

$_ZNSt7__n486116coroutine_handleINS_22noop_coroutine_promiseEE5_S_frE = comdat any

@.str = private unnamed_addr constant [26 x i8] c"lazy %ld done %d flat %d\0A\00", align 1
@_ZL2hi = internal global i64 0, align 8
@_ZL2lo = internal global i64 -1, align 8
@.str.1 = private unnamed_addr constant [27 x i8] c"eager %ld done %d flat %d\0A\00", align 1
@_ZNSt7__n486116coroutine_handleINS_22noop_coroutine_promiseEE5_S_frE = linkonce_odr dso_local global { ptr, ptr, [8 x i8] } { ptr @_ZNSt7__n486116coroutine_handleINS_22noop_coroutine_promiseEE7__frame22__dummy_resume_destroyEv, ptr @_ZNSt7__n486116coroutine_handleINS_22noop_coroutine_promiseEE7__frame22__dummy_resume_destroyEv, [8 x i8] undef }, comdat, align 8

; Function Attrs: mustprogress noinline optnone presplitcoroutine uwtable
define dso_local void @_Z4stepRl(ptr dead_on_unwind noalias writable sret(%struct.task) align 8 %0, ptr noundef nonnull align 8 dereferenceable(8) %1) #0 personality ptr @__gxx_personality_v0 {
  %3 = alloca ptr, align 8
  %4 = alloca ptr, align 8, !coro.outside.frame !6
  %5 = alloca ptr, align 8
  %6 = alloca %"struct.task<false>::promise_type", align 8
  %7 = alloca ptr, align 8
  %8 = alloca i32, align 4
  %9 = alloca %"struct.std::__n4861::suspend_always", align 1
  %10 = alloca %"struct.std::__n4861::suspend_always", align 1
  %11 = alloca %"struct.task<false>::promise_type::fin", align 1
  %12 = alloca %"struct.task<false>::promise_type::fin", align 1
  store ptr %0, ptr %3, align 8
  store ptr %1, ptr %4, align 8
  %13 = call token @llvm.coro.id(i32 16, ptr %6, ptr null, ptr null)
  %14 = call i1 @llvm.coro.alloc(token %13)
  br i1 %14, label %15, label %18

15:                                               ; preds = %2
  %16 = call i64 @llvm.coro.size.i64()
  %17 = call noalias noundef nonnull ptr @_Znwm(i64 noundef %16) #18
  br label %18

18:                                               ; preds = %15, %2
  %19 = phi ptr [ null, %2 ], [ %17, %15 ]
  %20 = call ptr @llvm.coro.begin(token %13, ptr %19)
  call void @llvm.lifetime.start.p0(ptr %5) #2
  %21 = load ptr, ptr %4, align 8, !nonnull !6, !align !7
  store ptr %21, ptr %5, align 8
  call void @llvm.lifetime.start.p0(ptr %6) #2
  call void @_ZN4taskILb0EE12promise_typeC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %6) #2
  invoke void @_ZN4taskILb0EE12promise_type17get_return_objectEv(ptr dead_on_unwind writable sret(%struct.task) align 8 %0, ptr noundef nonnull align 8 dereferenceable(8) %6)
          to label %22 unwind label %28

22:                                               ; preds = %18
  call void @llvm.lifetime.start.p0(ptr %9) #2
  call void @_ZN4taskILb0EE12promise_type15initial_suspendEv(ptr noundef nonnull align 8 dereferenceable(8) %6) #2
  %23 = call noundef i1 @_ZNKSt7__n486114suspend_always11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %9) #2
  br i1 %23, label %32, label %24

24:                                               ; preds = %22
  %25 = call token @llvm.coro.save(ptr null)
  call void @llvm.coro.await.suspend.void(ptr %9, ptr %20, ptr @_Z4stepRl.__await_suspend_wrapper__init) #2
  %26 = call i8 @llvm.coro.suspend(token %25, i1 false)
  switch i8 %26, label %83 [
    i8 0, label %32
    i8 1, label %27
  ]

27:                                               ; preds = %24
  br label %33

28:                                               ; preds = %18
  %29 = landingpad { ptr, i32 }
          cleanup
  %30 = extractvalue { ptr, i32 } %29, 0
  store ptr %30, ptr %7, align 8
  %31 = extractvalue { ptr, i32 } %29, 1
  store i32 %31, ptr %8, align 4
  br label %87

32:                                               ; preds = %24, %22
  call void @_ZNKSt7__n486114suspend_always12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %9) #2
  br label %33

33:                                               ; preds = %32, %27
  %34 = phi i32 [ 0, %32 ], [ 2, %27 ]
  call void @llvm.lifetime.end.p0(ptr %9) #2
  switch i32 %34, label %75 [
    i32 0, label %35
  ]

35:                                               ; preds = %33
  %36 = load ptr, ptr %5, align 8, !nonnull !6, !align !7
  %37 = load i64, ptr %36, align 8
  %38 = add nsw i64 %37, 1
  store i64 %38, ptr %36, align 8
  invoke void @_ZL5probev()
          to label %39 unwind label %41

39:                                               ; preds = %35
  invoke void @_ZN4taskILb0EE12promise_type11return_voidEv(ptr noundef nonnull align 8 dereferenceable(8) %6)
          to label %40 unwind label %41

40:                                               ; preds = %39
  br label %51

41:                                               ; preds = %39, %35
  %42 = landingpad { ptr, i32 }
          catch ptr null
  %43 = extractvalue { ptr, i32 } %42, 0
  store ptr %43, ptr %7, align 8
  %44 = extractvalue { ptr, i32 } %42, 1
  store i32 %44, ptr %8, align 4
  br label %45

45:                                               ; preds = %41
  %46 = load ptr, ptr %7, align 8
  %47 = call ptr @__cxa_begin_catch(ptr %46) #2
  invoke void @_ZN4taskILb0EE12promise_type19unhandled_exceptionEv(ptr noundef nonnull align 8 dereferenceable(8) %6)
          to label %48 unwind label %58

48:                                               ; preds = %45
  invoke void @__cxa_end_catch()
          to label %49 unwind label %62

49:                                               ; preds = %48
  br label %50

50:                                               ; preds = %49
  br label %51

51:                                               ; preds = %50, %40
  call void @llvm.lifetime.start.p0(ptr %11) #2
  call void @_ZN4taskILb0EE12promise_type13final_suspendEv(ptr noundef nonnull align 8 dereferenceable(8) %6) #2
  %52 = call noundef i1 @_ZN4taskILb0EE12promise_type3fin11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %11) #2
  br i1 %52, label %71, label %53

53:                                               ; preds = %51
  %54 = call token @llvm.coro.save(ptr null)
  invoke void @llvm.coro.await.suspend.handle(ptr %11, ptr %20, ptr @_Z4stepRl.__await_suspend_wrapper__final)
          to label %55 unwind label %67

55:                                               ; preds = %53
  %56 = call i8 @llvm.coro.suspend(token %54, i1 true)
  switch i8 %56, label %83 [
    i8 0, label %71
    i8 1, label %57
  ]

57:                                               ; preds = %55
  br label %72

58:                                               ; preds = %45
  %59 = landingpad { ptr, i32 }
          cleanup
  %60 = extractvalue { ptr, i32 } %59, 0
  store ptr %60, ptr %7, align 8
  %61 = extractvalue { ptr, i32 } %59, 1
  store i32 %61, ptr %8, align 4
  invoke void @__cxa_end_catch()
          to label %66 unwind label %98

62:                                               ; preds = %48
  %63 = landingpad { ptr, i32 }
          cleanup
  %64 = extractvalue { ptr, i32 } %63, 0
  store ptr %64, ptr %7, align 8
  %65 = extractvalue { ptr, i32 } %63, 1
  store i32 %65, ptr %8, align 4
  br label %84

66:                                               ; preds = %58
  br label %84

67:                                               ; preds = %53
  %68 = landingpad { ptr, i32 }
          cleanup
  %69 = extractvalue { ptr, i32 } %68, 0
  store ptr %69, ptr %7, align 8
  %70 = extractvalue { ptr, i32 } %68, 1
  store i32 %70, ptr %8, align 4
  call void @llvm.lifetime.end.p0(ptr %11) #2
  br label %84

71:                                               ; preds = %55, %51
  call void @_ZN4taskILb0EE12promise_type3fin12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %11) #2
  br label %72

72:                                               ; preds = %71, %57
  %73 = phi i32 [ 0, %71 ], [ 2, %57 ]
  call void @llvm.lifetime.end.p0(ptr %11) #2
  switch i32 %73, label %75 [
    i32 0, label %74
  ]

74:                                               ; preds = %72
  br label %75

75:                                               ; preds = %74, %72, %33
  %76 = phi i32 [ %34, %33 ], [ %73, %72 ], [ 0, %74 ]
  call void @llvm.lifetime.end.p0(ptr %6) #2
  call void @llvm.lifetime.end.p0(ptr %5) #2
  %77 = call ptr @llvm.coro.free(token %13, ptr %20)
  %78 = icmp ne ptr %77, null
  br i1 %78, label %79, label %81

79:                                               ; preds = %75
  %80 = call i64 @llvm.coro.size.i64()
  call void @_ZdlPvm(ptr noundef %77, i64 noundef %80) #2
  br label %81

81:                                               ; preds = %75, %79
  switch i32 %76, label %101 [
    i32 0, label %82
    i32 2, label %83
  ]

82:                                               ; preds = %81
  br label %83

83:                                               ; preds = %82, %81, %55, %24
  call void @llvm.coro.end(ptr null, i1 false, token none)
  ret void

84:                                               ; preds = %67, %66, %62
  call void @llvm.coro.end(ptr null, i1 true, token none)
  %85 = call i1 @llvm.coro.is_in_ramp()
  br i1 %85, label %86, label %93

86:                                               ; preds = %84
  br label %87

87:                                               ; preds = %86, %28
  call void @llvm.lifetime.end.p0(ptr %6) #2
  call void @llvm.lifetime.end.p0(ptr %5) #2
  %88 = call ptr @llvm.coro.free(token %13, ptr %20)
  %89 = icmp ne ptr %88, null
  br i1 %89, label %90, label %92

90:                                               ; preds = %87
  %91 = call i64 @llvm.coro.size.i64()
  call void @_ZdlPvm(ptr noundef %88, i64 noundef %91) #2
  br label %92

92:                                               ; preds = %87, %90
  br label %93

93:                                               ; preds = %92, %84
  %94 = load ptr, ptr %7, align 8
  %95 = load i32, ptr %8, align 4
  %96 = insertvalue { ptr, i32 } poison, ptr %94, 0
  %97 = insertvalue { ptr, i32 } %96, i32 %95, 1
  resume { ptr, i32 } %97

98:                                               ; preds = %58
  %99 = landingpad { ptr, i32 }
          catch ptr null
  %100 = extractvalue { ptr, i32 } %99, 0
  call void @__clang_call_terminate(ptr %100) #19
  unreachable

101:                                              ; preds = %81
  unreachable
}

; Function Attrs: nocallback nofree nosync nounwind willreturn memory(argmem: read)
declare token @llvm.coro.id(i32, ptr readnone, ptr readonly captures(none), ptr) #1

; Function Attrs: nounwind
declare i1 @llvm.coro.alloc(token) #2

; Function Attrs: nobuiltin allocsize(0)
declare noundef nonnull ptr @_Znwm(i64 noundef) #3

; Function Attrs: nounwind memory(none)
declare i64 @llvm.coro.size.i64() #4

; Function Attrs: nounwind
declare ptr @llvm.coro.begin(token, ptr writeonly) #2

; Function Attrs: nocallback nofree nosync nounwind willreturn memory(argmem: readwrite)
declare void @llvm.lifetime.start.p0(ptr captures(none)) #5

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZN4taskILb0EE12promise_typeC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %0) unnamed_addr #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds nuw %"struct.task<false>::promise_type", ptr %3, i32 0, i32 0
  call void @_ZNSt7__n486116coroutine_handleIvEC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %4) #2
  ret void
}

; Function Attrs: mustprogress noinline optnone uwtable
define linkonce_odr dso_local void @_ZN4taskILb0EE12promise_type17get_return_objectEv(ptr dead_on_unwind noalias writable sret(%struct.task) align 8 %0, ptr noundef nonnull align 8 dereferenceable(8) %1) #7 comdat {
  %3 = alloca ptr, align 8
  %4 = alloca ptr, align 8
  store ptr %0, ptr %3, align 8
  store ptr %1, ptr %4, align 8
  %5 = load ptr, ptr %4, align 8
  %6 = getelementptr inbounds nuw %struct.task, ptr %0, i32 0, i32 0
  %7 = call i64 @_ZNSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEE12from_promiseERS3_(ptr noundef nonnull align 8 dereferenceable(8) %5)
  %8 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %6, i32 0, i32 0
  %9 = inttoptr i64 %7 to ptr
  store ptr %9, ptr %8, align 8
  ret void
}

declare i32 @__gxx_personality_v0(...)

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZN4taskILb0EE12promise_type15initial_suspendEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #6 comdat {
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
define internal void @_Z4stepRl.__await_suspend_wrapper__init(ptr noundef nonnull %0, ptr noundef %1) #9 {
  %3 = alloca ptr, align 8
  %4 = alloca ptr, align 8
  %5 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %6 = alloca %"struct.std::__n4861::coroutine_handle", align 8
  store ptr %0, ptr %3, align 8
  store ptr %1, ptr %4, align 8
  %7 = load ptr, ptr %3, align 8
  %8 = load ptr, ptr %4, align 8
  %9 = call i64 @_ZNSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEE12from_addressEPv(ptr noundef %8) #2
  %10 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %6, i32 0, i32 0
  %11 = inttoptr i64 %9 to ptr
  store ptr %11, ptr %10, align 8
  %12 = call i64 @_ZNKSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEEcvNS0_IvEEEv(ptr noundef nonnull align 8 dereferenceable(8) %6) #2
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
define linkonce_odr dso_local i64 @_ZNSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEE12from_addressEPv(ptr noundef %0) #6 comdat {
  %2 = alloca %"struct.std::__n4861::coroutine_handle", align 8
  %3 = alloca ptr, align 8
  store ptr %0, ptr %3, align 8
  call void @_ZNSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEEC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %2) #2
  %4 = load ptr, ptr %3, align 8
  %5 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %2, i32 0, i32 0
  store ptr %4, ptr %5, align 8
  %6 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %2, i32 0, i32 0
  %7 = load ptr, ptr %6, align 8
  %8 = ptrtoint ptr %7 to i64
  ret i64 %8
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local i64 @_ZNKSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEEcvNS0_IvEEEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #6 comdat {
  %2 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %3 = alloca ptr, align 8
  store ptr %0, ptr %3, align 8
  %4 = load ptr, ptr %3, align 8
  %5 = call noundef ptr @_ZNKSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEE7addressEv(ptr noundef nonnull align 8 dereferenceable(8) %4) #2
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

; Function Attrs: mustprogress noinline optnone uwtable
define internal void @_ZL5probev() #7 {
  %1 = alloca i64, align 8
  %2 = call noundef i64 @_ZL4herev()
  store i64 %2, ptr %1, align 8
  %3 = load i64, ptr %1, align 8
  %4 = load i64, ptr @_ZL2lo, align 8
  %5 = icmp ult i64 %3, %4
  br i1 %5, label %6, label %8

6:                                                ; preds = %0
  %7 = load i64, ptr %1, align 8
  store i64 %7, ptr @_ZL2lo, align 8
  br label %8

8:                                                ; preds = %6, %0
  %9 = load i64, ptr %1, align 8
  %10 = load i64, ptr @_ZL2hi, align 8
  %11 = icmp ugt i64 %9, %10
  br i1 %11, label %12, label %14

12:                                               ; preds = %8
  %13 = load i64, ptr %1, align 8
  store i64 %13, ptr @_ZL2hi, align 8
  br label %14

14:                                               ; preds = %12, %8
  ret void
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZN4taskILb0EE12promise_type11return_voidEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  ret void
}

declare ptr @__cxa_begin_catch(ptr)

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZN4taskILb0EE12promise_type19unhandled_exceptionEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  ret void
}

declare void @__cxa_end_catch()

; Function Attrs: noinline noreturn nounwind uwtable
define linkonce_odr hidden void @__clang_call_terminate(ptr noundef %0) #10 comdat {
  %2 = call ptr @__cxa_begin_catch(ptr %0) #2
  call void @_ZSt9terminatev() #19
  unreachable
}

declare void @_ZSt9terminatev()

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZN4taskILb0EE12promise_type13final_suspendEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  ret void
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local noundef i1 @_ZN4taskILb0EE12promise_type3fin11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %0) #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  ret i1 false
}

; Function Attrs: alwaysinline mustprogress
define internal noundef ptr @_Z4stepRl.__await_suspend_wrapper__final(ptr noundef nonnull %0, ptr noundef %1) #9 {
  %3 = alloca ptr, align 8
  %4 = alloca ptr, align 8
  %5 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %6 = alloca %"struct.std::__n4861::coroutine_handle", align 8
  store ptr %0, ptr %3, align 8
  store ptr %1, ptr %4, align 8
  %7 = load ptr, ptr %3, align 8
  %8 = load ptr, ptr %4, align 8
  %9 = call i64 @_ZNSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEE12from_addressEPv(ptr noundef %8) #2
  %10 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %6, i32 0, i32 0
  %11 = inttoptr i64 %9 to ptr
  store ptr %11, ptr %10, align 8
  %12 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %6, i32 0, i32 0
  %13 = load ptr, ptr %12, align 8
  %14 = call i64 @_ZN4taskILb0EE12promise_type3fin13await_suspendENSt7__n486116coroutine_handleIS1_EE(ptr noundef nonnull align 1 dereferenceable(1) %7, ptr %13) #2
  %15 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %5, i32 0, i32 0
  %16 = inttoptr i64 %14 to ptr
  store ptr %16, ptr %15, align 8
  %17 = call noundef ptr @_ZNKSt7__n486116coroutine_handleIvE7addressEv(ptr noundef nonnull align 8 dereferenceable(8) %5) #2
  ret ptr %17
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local i64 @_ZN4taskILb0EE12promise_type3fin13await_suspendENSt7__n486116coroutine_handleIS1_EE(ptr noundef nonnull align 1 dereferenceable(1) %0, ptr %1) #6 comdat personality ptr @__gxx_personality_v0 {
  %3 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %4 = alloca %"struct.std::__n4861::coroutine_handle", align 8
  %5 = alloca ptr, align 8
  %6 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %7 = alloca %"struct.std::__n4861::coroutine_handle.3", align 8
  %8 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %4, i32 0, i32 0
  store ptr %1, ptr %8, align 8
  store ptr %0, ptr %5, align 8
  %9 = load ptr, ptr %5, align 8
  %10 = invoke noundef nonnull align 8 dereferenceable(8) ptr @_ZNKSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEE7promiseEv(ptr noundef nonnull align 8 dereferenceable(8) %4)
          to label %11 unwind label %26

11:                                               ; preds = %2
  %12 = getelementptr inbounds nuw %"struct.task<false>::promise_type", ptr %10, i32 0, i32 0
  call void @llvm.memcpy.p0.p0.i64(ptr align 8 %6, ptr align 8 %12, i64 8, i1 false)
  %13 = call noundef i1 @_ZNKSt7__n486116coroutine_handleIvEcvbEv(ptr noundef nonnull align 8 dereferenceable(8) %6) #2
  br i1 %13, label %14, label %15

14:                                               ; preds = %11
  call void @llvm.memcpy.p0.p0.i64(ptr align 8 %3, ptr align 8 %6, i64 8, i1 false)
  br label %22

15:                                               ; preds = %11
  %16 = call i64 @_ZNSt7__n486114noop_coroutineEv() #2
  %17 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.3", ptr %7, i32 0, i32 0
  %18 = inttoptr i64 %16 to ptr
  store ptr %18, ptr %17, align 8
  %19 = call i64 @_ZNKSt7__n486116coroutine_handleINS_22noop_coroutine_promiseEEcvNS0_IvEEEv(ptr noundef nonnull align 8 dereferenceable(8) %7) #2
  %20 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %3, i32 0, i32 0
  %21 = inttoptr i64 %19 to ptr
  store ptr %21, ptr %20, align 8
  br label %22

22:                                               ; preds = %15, %14
  %23 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %3, i32 0, i32 0
  %24 = load ptr, ptr %23, align 8
  %25 = ptrtoint ptr %24 to i64
  ret i64 %25

26:                                               ; preds = %2
  %27 = landingpad { ptr, i32 }
          catch ptr null
  %28 = extractvalue { ptr, i32 } %27, 0
  call void @__clang_call_terminate(ptr %28) #19
  unreachable
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local noundef ptr @_ZNKSt7__n486116coroutine_handleIvE7addressEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %3, i32 0, i32 0
  %5 = load ptr, ptr %4, align 8
  ret ptr %5
}

declare void @llvm.coro.await.suspend.handle(ptr, ptr, ptr)

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZN4taskILb0EE12promise_type3fin12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %0) #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  ret void
}

; Function Attrs: nounwind
declare void @llvm.coro.end(ptr, i1, token) #2

; Function Attrs: nounwind memory(none)
declare i1 @llvm.coro.is_in_ramp() #4

; Function Attrs: nobuiltin nounwind
declare void @_ZdlPvm(ptr noundef, i64 noundef) #11

; Function Attrs: nounwind memory(argmem: read)
declare ptr @llvm.coro.free(token, ptr readonly captures(none)) #12

; Function Attrs: mustprogress noinline optnone presplitcoroutine uwtable
define dso_local void @_Z4lazyRll(ptr dead_on_unwind noalias writable sret(%struct.task) align 8 %0, ptr noundef nonnull align 8 dereferenceable(8) %1, i64 noundef %2) #0 personality ptr @__gxx_personality_v0 {
  %4 = alloca ptr, align 8
  %5 = alloca ptr, align 8, !coro.outside.frame !6
  %6 = alloca i64, align 8, !coro.outside.frame !6
  %7 = alloca ptr, align 8
  %8 = alloca i64, align 8
  %9 = alloca %"struct.task<false>::promise_type", align 8
  %10 = alloca ptr, align 8
  %11 = alloca i32, align 4
  %12 = alloca %"struct.std::__n4861::suspend_always", align 1
  %13 = alloca %"struct.std::__n4861::suspend_always", align 1
  %14 = alloca i64, align 8
  %15 = alloca %struct.task, align 8
  %16 = alloca %"struct.task<false>::promise_type::fin", align 1
  %17 = alloca %"struct.task<false>::promise_type::fin", align 1
  store ptr %0, ptr %4, align 8
  store ptr %1, ptr %5, align 8
  store i64 %2, ptr %6, align 8
  %18 = call token @llvm.coro.id(i32 16, ptr %9, ptr null, ptr null)
  %19 = call i1 @llvm.coro.alloc(token %18)
  br i1 %19, label %20, label %23

20:                                               ; preds = %3
  %21 = call i64 @llvm.coro.size.i64()
  %22 = call noalias noundef nonnull ptr @_Znwm(i64 noundef %21) #18
  br label %23

23:                                               ; preds = %20, %3
  %24 = phi ptr [ null, %3 ], [ %22, %20 ]
  %25 = call ptr @llvm.coro.begin(token %18, ptr %24)
  call void @llvm.lifetime.start.p0(ptr %7) #2
  %26 = load ptr, ptr %5, align 8, !nonnull !6, !align !7
  store ptr %26, ptr %7, align 8
  call void @llvm.lifetime.start.p0(ptr %8) #2
  %27 = load i64, ptr %6, align 8
  store i64 %27, ptr %8, align 8
  call void @llvm.lifetime.start.p0(ptr %9) #2
  call void @_ZN4taskILb0EE12promise_typeC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %9) #2
  invoke void @_ZN4taskILb0EE12promise_type17get_return_objectEv(ptr dead_on_unwind writable sret(%struct.task) align 8 %0, ptr noundef nonnull align 8 dereferenceable(8) %9)
          to label %28 unwind label %34

28:                                               ; preds = %23
  call void @llvm.lifetime.start.p0(ptr %12) #2
  call void @_ZN4taskILb0EE12promise_type15initial_suspendEv(ptr noundef nonnull align 8 dereferenceable(8) %9) #2
  %29 = call noundef i1 @_ZNKSt7__n486114suspend_always11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %12) #2
  br i1 %29, label %38, label %30

30:                                               ; preds = %28
  %31 = call token @llvm.coro.save(ptr null)
  call void @llvm.coro.await.suspend.void(ptr %12, ptr %25, ptr @_Z4lazyRll.__await_suspend_wrapper__init) #2
  %32 = call i8 @llvm.coro.suspend(token %31, i1 false)
  switch i8 %32, label %120 [
    i8 0, label %38
    i8 1, label %33
  ]

33:                                               ; preds = %30
  br label %39

34:                                               ; preds = %23
  %35 = landingpad { ptr, i32 }
          cleanup
  %36 = extractvalue { ptr, i32 } %35, 0
  store ptr %36, ptr %10, align 8
  %37 = extractvalue { ptr, i32 } %35, 1
  store i32 %37, ptr %11, align 4
  br label %124

38:                                               ; preds = %30, %28
  call void @_ZNKSt7__n486114suspend_always12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %12) #2
  br label %39

39:                                               ; preds = %38, %33
  %40 = phi i32 [ 0, %38 ], [ 2, %33 ]
  call void @llvm.lifetime.end.p0(ptr %12) #2
  switch i32 %40, label %112 [
    i32 0, label %41
  ]

41:                                               ; preds = %39
  call void @llvm.lifetime.start.p0(ptr %14) #2
  store i64 0, ptr %14, align 8
  br label %42

42:                                               ; preds = %70, %41
  %43 = load i64, ptr %14, align 8
  %44 = load i64, ptr %8, align 8
  %45 = icmp slt i64 %43, %44
  br i1 %45, label %47, label %46

46:                                               ; preds = %42
  br label %74

47:                                               ; preds = %42
  call void @llvm.lifetime.start.p0(ptr %15) #2
  %48 = load ptr, ptr %7, align 8, !nonnull !6, !align !7
  invoke void @_Z4stepRl(ptr dead_on_unwind writable sret(%struct.task) align 8 %15, ptr noundef nonnull align 8 dereferenceable(8) %48)
          to label %49 unwind label %57

49:                                               ; preds = %47
  %50 = invoke noundef i1 @_ZN4taskILb0EE11await_readyEv(ptr noundef nonnull align 8 dereferenceable(8) %15)
          to label %51 unwind label %61

51:                                               ; preds = %49
  br i1 %50, label %65, label %52

52:                                               ; preds = %51
  %53 = call token @llvm.coro.save(ptr null)
  invoke void @llvm.coro.await.suspend.handle(ptr %15, ptr %25, ptr @_Z4lazyRll.__await_suspend_wrapper__await)
          to label %54 unwind label %61

54:                                               ; preds = %52
  %55 = call i8 @llvm.coro.suspend(token %53, i1 false)
  switch i8 %55, label %120 [
    i8 0, label %65
    i8 1, label %56
  ]

56:                                               ; preds = %54
  br label %67

57:                                               ; preds = %47
  %58 = landingpad { ptr, i32 }
          catch ptr null
  %59 = extractvalue { ptr, i32 } %58, 0
  store ptr %59, ptr %10, align 8
  %60 = extractvalue { ptr, i32 } %58, 1
  store i32 %60, ptr %11, align 4
  br label %73

61:                                               ; preds = %65, %52, %49
  %62 = landingpad { ptr, i32 }
          catch ptr null
  %63 = extractvalue { ptr, i32 } %62, 0
  store ptr %63, ptr %10, align 8
  %64 = extractvalue { ptr, i32 } %62, 1
  store i32 %64, ptr %11, align 4
  call void @_ZN4taskILb0EED2Ev(ptr noundef nonnull align 8 dereferenceable(8) %15) #2
  br label %73

65:                                               ; preds = %54, %51
  invoke void @_ZN4taskILb0EE12await_resumeEv(ptr noundef nonnull align 8 dereferenceable(8) %15)
          to label %66 unwind label %61

66:                                               ; preds = %65
  br label %67

67:                                               ; preds = %66, %56
  %68 = phi i32 [ 0, %66 ], [ 2, %56 ]
  call void @_ZN4taskILb0EED2Ev(ptr noundef nonnull align 8 dereferenceable(8) %15) #2
  call void @llvm.lifetime.end.p0(ptr %15) #2
  switch i32 %68, label %74 [
    i32 0, label %69
  ]

69:                                               ; preds = %67
  br label %70

70:                                               ; preds = %69
  %71 = load i64, ptr %14, align 8
  %72 = add nsw i64 %71, 1
  store i64 %72, ptr %14, align 8
  br label %42, !llvm.loop !8

73:                                               ; preds = %61, %57
  call void @llvm.lifetime.end.p0(ptr %15) #2
  call void @llvm.lifetime.end.p0(ptr %14) #2
  br label %82

74:                                               ; preds = %67, %46
  %75 = phi i32 [ %68, %67 ], [ 4, %46 ]
  call void @llvm.lifetime.end.p0(ptr %14) #2
  switch i32 %75, label %112 [
    i32 4, label %76
  ]

76:                                               ; preds = %74
  invoke void @_ZN4taskILb0EE12promise_type11return_voidEv(ptr noundef nonnull align 8 dereferenceable(8) %9)
          to label %77 unwind label %78

77:                                               ; preds = %76
  br label %88

78:                                               ; preds = %76
  %79 = landingpad { ptr, i32 }
          catch ptr null
  %80 = extractvalue { ptr, i32 } %79, 0
  store ptr %80, ptr %10, align 8
  %81 = extractvalue { ptr, i32 } %79, 1
  store i32 %81, ptr %11, align 4
  br label %82

82:                                               ; preds = %78, %73
  %83 = load ptr, ptr %10, align 8
  %84 = call ptr @__cxa_begin_catch(ptr %83) #2
  invoke void @_ZN4taskILb0EE12promise_type19unhandled_exceptionEv(ptr noundef nonnull align 8 dereferenceable(8) %9)
          to label %85 unwind label %95

85:                                               ; preds = %82
  invoke void @__cxa_end_catch()
          to label %86 unwind label %99

86:                                               ; preds = %85
  br label %87

87:                                               ; preds = %86
  br label %88

88:                                               ; preds = %87, %77
  call void @llvm.lifetime.start.p0(ptr %16) #2
  call void @_ZN4taskILb0EE12promise_type13final_suspendEv(ptr noundef nonnull align 8 dereferenceable(8) %9) #2
  %89 = call noundef i1 @_ZN4taskILb0EE12promise_type3fin11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %16) #2
  br i1 %89, label %108, label %90

90:                                               ; preds = %88
  %91 = call token @llvm.coro.save(ptr null)
  invoke void @llvm.coro.await.suspend.handle(ptr %16, ptr %25, ptr @_Z4lazyRll.__await_suspend_wrapper__final)
          to label %92 unwind label %104

92:                                               ; preds = %90
  %93 = call i8 @llvm.coro.suspend(token %91, i1 true)
  switch i8 %93, label %120 [
    i8 0, label %108
    i8 1, label %94
  ]

94:                                               ; preds = %92
  br label %109

95:                                               ; preds = %82
  %96 = landingpad { ptr, i32 }
          cleanup
  %97 = extractvalue { ptr, i32 } %96, 0
  store ptr %97, ptr %10, align 8
  %98 = extractvalue { ptr, i32 } %96, 1
  store i32 %98, ptr %11, align 4
  invoke void @__cxa_end_catch()
          to label %103 unwind label %135

99:                                               ; preds = %85
  %100 = landingpad { ptr, i32 }
          cleanup
  %101 = extractvalue { ptr, i32 } %100, 0
  store ptr %101, ptr %10, align 8
  %102 = extractvalue { ptr, i32 } %100, 1
  store i32 %102, ptr %11, align 4
  br label %121

103:                                              ; preds = %95
  br label %121

104:                                              ; preds = %90
  %105 = landingpad { ptr, i32 }
          cleanup
  %106 = extractvalue { ptr, i32 } %105, 0
  store ptr %106, ptr %10, align 8
  %107 = extractvalue { ptr, i32 } %105, 1
  store i32 %107, ptr %11, align 4
  call void @llvm.lifetime.end.p0(ptr %16) #2
  br label %121

108:                                              ; preds = %92, %88
  call void @_ZN4taskILb0EE12promise_type3fin12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %16) #2
  br label %109

109:                                              ; preds = %108, %94
  %110 = phi i32 [ 0, %108 ], [ 2, %94 ]
  call void @llvm.lifetime.end.p0(ptr %16) #2
  switch i32 %110, label %112 [
    i32 0, label %111
  ]

111:                                              ; preds = %109
  br label %112

112:                                              ; preds = %111, %109, %74, %39
  %113 = phi i32 [ %40, %39 ], [ %75, %74 ], [ %110, %109 ], [ 0, %111 ]
  call void @llvm.lifetime.end.p0(ptr %9) #2
  call void @llvm.lifetime.end.p0(ptr %8) #2
  call void @llvm.lifetime.end.p0(ptr %7) #2
  %114 = call ptr @llvm.coro.free(token %18, ptr %25)
  %115 = icmp ne ptr %114, null
  br i1 %115, label %116, label %118

116:                                              ; preds = %112
  %117 = call i64 @llvm.coro.size.i64()
  call void @_ZdlPvm(ptr noundef %114, i64 noundef %117) #2
  br label %118

118:                                              ; preds = %112, %116
  switch i32 %113, label %138 [
    i32 0, label %119
    i32 2, label %120
  ]

119:                                              ; preds = %118
  br label %120

120:                                              ; preds = %119, %118, %92, %54, %30
  call void @llvm.coro.end(ptr null, i1 false, token none)
  ret void

121:                                              ; preds = %104, %103, %99
  call void @llvm.coro.end(ptr null, i1 true, token none)
  %122 = call i1 @llvm.coro.is_in_ramp()
  br i1 %122, label %123, label %130

123:                                              ; preds = %121
  br label %124

124:                                              ; preds = %123, %34
  call void @llvm.lifetime.end.p0(ptr %9) #2
  call void @llvm.lifetime.end.p0(ptr %8) #2
  call void @llvm.lifetime.end.p0(ptr %7) #2
  %125 = call ptr @llvm.coro.free(token %18, ptr %25)
  %126 = icmp ne ptr %125, null
  br i1 %126, label %127, label %129

127:                                              ; preds = %124
  %128 = call i64 @llvm.coro.size.i64()
  call void @_ZdlPvm(ptr noundef %125, i64 noundef %128) #2
  br label %129

129:                                              ; preds = %124, %127
  br label %130

130:                                              ; preds = %129, %121
  %131 = load ptr, ptr %10, align 8
  %132 = load i32, ptr %11, align 4
  %133 = insertvalue { ptr, i32 } poison, ptr %131, 0
  %134 = insertvalue { ptr, i32 } %133, i32 %132, 1
  resume { ptr, i32 } %134

135:                                              ; preds = %95
  %136 = landingpad { ptr, i32 }
          catch ptr null
  %137 = extractvalue { ptr, i32 } %136, 0
  call void @__clang_call_terminate(ptr %137) #19
  unreachable

138:                                              ; preds = %118
  unreachable
}

; Function Attrs: alwaysinline mustprogress
define internal void @_Z4lazyRll.__await_suspend_wrapper__init(ptr noundef nonnull %0, ptr noundef %1) #9 {
  %3 = alloca ptr, align 8
  %4 = alloca ptr, align 8
  %5 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %6 = alloca %"struct.std::__n4861::coroutine_handle", align 8
  store ptr %0, ptr %3, align 8
  store ptr %1, ptr %4, align 8
  %7 = load ptr, ptr %3, align 8
  %8 = load ptr, ptr %4, align 8
  %9 = call i64 @_ZNSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEE12from_addressEPv(ptr noundef %8) #2
  %10 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %6, i32 0, i32 0
  %11 = inttoptr i64 %9 to ptr
  store ptr %11, ptr %10, align 8
  %12 = call i64 @_ZNKSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEEcvNS0_IvEEEv(ptr noundef nonnull align 8 dereferenceable(8) %6) #2
  %13 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %5, i32 0, i32 0
  %14 = inttoptr i64 %12 to ptr
  store ptr %14, ptr %13, align 8
  %15 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %5, i32 0, i32 0
  %16 = load ptr, ptr %15, align 8
  call void @_ZNKSt7__n486114suspend_always13await_suspendENS_16coroutine_handleIvEE(ptr noundef nonnull align 1 dereferenceable(1) %7, ptr %16) #2
  ret void
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local noundef i1 @_ZN4taskILb0EE11await_readyEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  ret i1 false
}

; Function Attrs: alwaysinline mustprogress
define internal noundef ptr @_Z4lazyRll.__await_suspend_wrapper__await(ptr noundef nonnull %0, ptr noundef %1) #9 {
  %3 = alloca ptr, align 8
  %4 = alloca ptr, align 8
  %5 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %6 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %7 = alloca %"struct.std::__n4861::coroutine_handle", align 8
  store ptr %0, ptr %3, align 8
  store ptr %1, ptr %4, align 8
  %8 = load ptr, ptr %3, align 8
  %9 = load ptr, ptr %4, align 8
  %10 = call i64 @_ZNSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEE12from_addressEPv(ptr noundef %9) #2
  %11 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %7, i32 0, i32 0
  %12 = inttoptr i64 %10 to ptr
  store ptr %12, ptr %11, align 8
  %13 = call i64 @_ZNKSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEEcvNS0_IvEEEv(ptr noundef nonnull align 8 dereferenceable(8) %7) #2
  %14 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %6, i32 0, i32 0
  %15 = inttoptr i64 %13 to ptr
  store ptr %15, ptr %14, align 8
  %16 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %6, i32 0, i32 0
  %17 = load ptr, ptr %16, align 8
  %18 = call i64 @_ZN4taskILb0EE13await_suspendENSt7__n486116coroutine_handleIvEE(ptr noundef nonnull align 8 dereferenceable(8) %8, ptr %17)
  %19 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %5, i32 0, i32 0
  %20 = inttoptr i64 %18 to ptr
  store ptr %20, ptr %19, align 8
  %21 = call noundef ptr @_ZNKSt7__n486116coroutine_handleIvE7addressEv(ptr noundef nonnull align 8 dereferenceable(8) %5) #2
  ret ptr %21
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local i64 @_ZN4taskILb0EE13await_suspendENSt7__n486116coroutine_handleIvEE(ptr noundef nonnull align 8 dereferenceable(8) %0, ptr %1) #6 comdat {
  %3 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %4 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %5 = alloca ptr, align 8
  %6 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %4, i32 0, i32 0
  store ptr %1, ptr %6, align 8
  store ptr %0, ptr %5, align 8
  %7 = load ptr, ptr %5, align 8
  %8 = getelementptr inbounds nuw %struct.task, ptr %7, i32 0, i32 0
  %9 = call noundef nonnull align 8 dereferenceable(8) ptr @_ZNKSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEE7promiseEv(ptr noundef nonnull align 8 dereferenceable(8) %8)
  %10 = getelementptr inbounds nuw %"struct.task<false>::promise_type", ptr %9, i32 0, i32 0
  call void @llvm.memcpy.p0.p0.i64(ptr align 8 %10, ptr align 8 %4, i64 8, i1 false)
  %11 = getelementptr inbounds nuw %struct.task, ptr %7, i32 0, i32 0
  %12 = call i64 @_ZNKSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEEcvNS0_IvEEEv(ptr noundef nonnull align 8 dereferenceable(8) %11) #2
  %13 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %3, i32 0, i32 0
  %14 = inttoptr i64 %12 to ptr
  store ptr %14, ptr %13, align 8
  %15 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %3, i32 0, i32 0
  %16 = load ptr, ptr %15, align 8
  %17 = ptrtoint ptr %16 to i64
  ret i64 %17
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZN4taskILb0EE12await_resumeEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  ret void
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZN4taskILb0EED2Ev(ptr noundef nonnull align 8 dereferenceable(8) %0) unnamed_addr #6 comdat personality ptr @__gxx_personality_v0 {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds nuw %struct.task, ptr %3, i32 0, i32 0
  %5 = call noundef i1 @_ZNKSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEEcvbEv(ptr noundef nonnull align 8 dereferenceable(8) %4) #2
  br i1 %5, label %6, label %9

6:                                                ; preds = %1
  %7 = getelementptr inbounds nuw %struct.task, ptr %3, i32 0, i32 0
  invoke void @_ZNKSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEE7destroyEv(ptr noundef nonnull align 8 dereferenceable(8) %7)
          to label %8 unwind label %10

8:                                                ; preds = %6
  br label %9

9:                                                ; preds = %8, %1
  ret void

10:                                               ; preds = %6
  %11 = landingpad { ptr, i32 }
          catch ptr null
  %12 = extractvalue { ptr, i32 } %11, 0
  call void @__clang_call_terminate(ptr %12) #19
  unreachable
}

; Function Attrs: alwaysinline mustprogress
define internal noundef ptr @_Z4lazyRll.__await_suspend_wrapper__final(ptr noundef nonnull %0, ptr noundef %1) #9 {
  %3 = alloca ptr, align 8
  %4 = alloca ptr, align 8
  %5 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %6 = alloca %"struct.std::__n4861::coroutine_handle", align 8
  store ptr %0, ptr %3, align 8
  store ptr %1, ptr %4, align 8
  %7 = load ptr, ptr %3, align 8
  %8 = load ptr, ptr %4, align 8
  %9 = call i64 @_ZNSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEE12from_addressEPv(ptr noundef %8) #2
  %10 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %6, i32 0, i32 0
  %11 = inttoptr i64 %9 to ptr
  store ptr %11, ptr %10, align 8
  %12 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %6, i32 0, i32 0
  %13 = load ptr, ptr %12, align 8
  %14 = call i64 @_ZN4taskILb0EE12promise_type3fin13await_suspendENSt7__n486116coroutine_handleIS1_EE(ptr noundef nonnull align 1 dereferenceable(1) %7, ptr %13) #2
  %15 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %5, i32 0, i32 0
  %16 = inttoptr i64 %14 to ptr
  store ptr %16, ptr %15, align 8
  %17 = call noundef ptr @_ZNKSt7__n486116coroutine_handleIvE7addressEv(ptr noundef nonnull align 8 dereferenceable(8) %5) #2
  ret ptr %17
}

; Function Attrs: mustprogress noinline optnone presplitcoroutine uwtable
define dso_local void @_Z5eagerRll(ptr dead_on_unwind noalias writable sret(%struct.task.1) align 8 %0, ptr noundef nonnull align 8 dereferenceable(8) %1, i64 noundef %2) #0 personality ptr @__gxx_personality_v0 {
  %4 = alloca ptr, align 8
  %5 = alloca ptr, align 8, !coro.outside.frame !6
  %6 = alloca i64, align 8, !coro.outside.frame !6
  %7 = alloca ptr, align 8
  %8 = alloca i64, align 8
  %9 = alloca %"struct.task<true>::promise_type", align 8
  %10 = alloca ptr, align 8
  %11 = alloca i32, align 4
  %12 = alloca %"struct.std::__n4861::suspend_never", align 1
  %13 = alloca %"struct.std::__n4861::suspend_never", align 1
  %14 = alloca i64, align 8
  %15 = alloca %struct.task, align 8
  %16 = alloca %"struct.task<true>::promise_type::fin", align 1
  %17 = alloca %"struct.task<true>::promise_type::fin", align 1
  store ptr %0, ptr %4, align 8
  store ptr %1, ptr %5, align 8
  store i64 %2, ptr %6, align 8
  %18 = call token @llvm.coro.id(i32 16, ptr %9, ptr null, ptr null)
  %19 = call i1 @llvm.coro.alloc(token %18)
  br i1 %19, label %20, label %23

20:                                               ; preds = %3
  %21 = call i64 @llvm.coro.size.i64()
  %22 = call noalias noundef nonnull ptr @_Znwm(i64 noundef %21) #18
  br label %23

23:                                               ; preds = %20, %3
  %24 = phi ptr [ null, %3 ], [ %22, %20 ]
  %25 = call ptr @llvm.coro.begin(token %18, ptr %24)
  call void @llvm.lifetime.start.p0(ptr %7) #2
  %26 = load ptr, ptr %5, align 8, !nonnull !6, !align !7
  store ptr %26, ptr %7, align 8
  call void @llvm.lifetime.start.p0(ptr %8) #2
  %27 = load i64, ptr %6, align 8
  store i64 %27, ptr %8, align 8
  call void @llvm.lifetime.start.p0(ptr %9) #2
  call void @_ZN4taskILb1EE12promise_typeC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %9) #2
  invoke void @_ZN4taskILb1EE12promise_type17get_return_objectEv(ptr dead_on_unwind writable sret(%struct.task.1) align 8 %0, ptr noundef nonnull align 8 dereferenceable(8) %9)
          to label %28 unwind label %34

28:                                               ; preds = %23
  call void @llvm.lifetime.start.p0(ptr %12) #2
  call void @_ZN4taskILb1EE12promise_type15initial_suspendEv(ptr noundef nonnull align 8 dereferenceable(8) %9) #2
  %29 = call noundef i1 @_ZNKSt7__n486113suspend_never11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %12) #2
  br i1 %29, label %38, label %30

30:                                               ; preds = %28
  %31 = call token @llvm.coro.save(ptr null)
  call void @llvm.coro.await.suspend.void(ptr %12, ptr %25, ptr @_Z5eagerRll.__await_suspend_wrapper__init) #2
  %32 = call i8 @llvm.coro.suspend(token %31, i1 false)
  switch i8 %32, label %120 [
    i8 0, label %38
    i8 1, label %33
  ]

33:                                               ; preds = %30
  br label %39

34:                                               ; preds = %23
  %35 = landingpad { ptr, i32 }
          cleanup
  %36 = extractvalue { ptr, i32 } %35, 0
  store ptr %36, ptr %10, align 8
  %37 = extractvalue { ptr, i32 } %35, 1
  store i32 %37, ptr %11, align 4
  br label %124

38:                                               ; preds = %30, %28
  call void @_ZNKSt7__n486113suspend_never12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %12) #2
  br label %39

39:                                               ; preds = %38, %33
  %40 = phi i32 [ 0, %38 ], [ 2, %33 ]
  call void @llvm.lifetime.end.p0(ptr %12) #2
  switch i32 %40, label %112 [
    i32 0, label %41
  ]

41:                                               ; preds = %39
  call void @llvm.lifetime.start.p0(ptr %14) #2
  store i64 0, ptr %14, align 8
  br label %42

42:                                               ; preds = %70, %41
  %43 = load i64, ptr %14, align 8
  %44 = load i64, ptr %8, align 8
  %45 = icmp slt i64 %43, %44
  br i1 %45, label %47, label %46

46:                                               ; preds = %42
  br label %74

47:                                               ; preds = %42
  call void @llvm.lifetime.start.p0(ptr %15) #2
  %48 = load ptr, ptr %7, align 8, !nonnull !6, !align !7
  invoke void @_Z4stepRl(ptr dead_on_unwind writable sret(%struct.task) align 8 %15, ptr noundef nonnull align 8 dereferenceable(8) %48)
          to label %49 unwind label %57

49:                                               ; preds = %47
  %50 = invoke noundef i1 @_ZN4taskILb0EE11await_readyEv(ptr noundef nonnull align 8 dereferenceable(8) %15)
          to label %51 unwind label %61

51:                                               ; preds = %49
  br i1 %50, label %65, label %52

52:                                               ; preds = %51
  %53 = call token @llvm.coro.save(ptr null)
  invoke void @llvm.coro.await.suspend.handle(ptr %15, ptr %25, ptr @_Z5eagerRll.__await_suspend_wrapper__await)
          to label %54 unwind label %61

54:                                               ; preds = %52
  %55 = call i8 @llvm.coro.suspend(token %53, i1 false)
  switch i8 %55, label %120 [
    i8 0, label %65
    i8 1, label %56
  ]

56:                                               ; preds = %54
  br label %67

57:                                               ; preds = %47
  %58 = landingpad { ptr, i32 }
          catch ptr null
  %59 = extractvalue { ptr, i32 } %58, 0
  store ptr %59, ptr %10, align 8
  %60 = extractvalue { ptr, i32 } %58, 1
  store i32 %60, ptr %11, align 4
  br label %73

61:                                               ; preds = %65, %52, %49
  %62 = landingpad { ptr, i32 }
          catch ptr null
  %63 = extractvalue { ptr, i32 } %62, 0
  store ptr %63, ptr %10, align 8
  %64 = extractvalue { ptr, i32 } %62, 1
  store i32 %64, ptr %11, align 4
  call void @_ZN4taskILb0EED2Ev(ptr noundef nonnull align 8 dereferenceable(8) %15) #2
  br label %73

65:                                               ; preds = %54, %51
  invoke void @_ZN4taskILb0EE12await_resumeEv(ptr noundef nonnull align 8 dereferenceable(8) %15)
          to label %66 unwind label %61

66:                                               ; preds = %65
  br label %67

67:                                               ; preds = %66, %56
  %68 = phi i32 [ 0, %66 ], [ 2, %56 ]
  call void @_ZN4taskILb0EED2Ev(ptr noundef nonnull align 8 dereferenceable(8) %15) #2
  call void @llvm.lifetime.end.p0(ptr %15) #2
  switch i32 %68, label %74 [
    i32 0, label %69
  ]

69:                                               ; preds = %67
  br label %70

70:                                               ; preds = %69
  %71 = load i64, ptr %14, align 8
  %72 = add nsw i64 %71, 1
  store i64 %72, ptr %14, align 8
  br label %42, !llvm.loop !10

73:                                               ; preds = %61, %57
  call void @llvm.lifetime.end.p0(ptr %15) #2
  call void @llvm.lifetime.end.p0(ptr %14) #2
  br label %82

74:                                               ; preds = %67, %46
  %75 = phi i32 [ %68, %67 ], [ 4, %46 ]
  call void @llvm.lifetime.end.p0(ptr %14) #2
  switch i32 %75, label %112 [
    i32 4, label %76
  ]

76:                                               ; preds = %74
  invoke void @_ZN4taskILb1EE12promise_type11return_voidEv(ptr noundef nonnull align 8 dereferenceable(8) %9)
          to label %77 unwind label %78

77:                                               ; preds = %76
  br label %88

78:                                               ; preds = %76
  %79 = landingpad { ptr, i32 }
          catch ptr null
  %80 = extractvalue { ptr, i32 } %79, 0
  store ptr %80, ptr %10, align 8
  %81 = extractvalue { ptr, i32 } %79, 1
  store i32 %81, ptr %11, align 4
  br label %82

82:                                               ; preds = %78, %73
  %83 = load ptr, ptr %10, align 8
  %84 = call ptr @__cxa_begin_catch(ptr %83) #2
  invoke void @_ZN4taskILb1EE12promise_type19unhandled_exceptionEv(ptr noundef nonnull align 8 dereferenceable(8) %9)
          to label %85 unwind label %95

85:                                               ; preds = %82
  invoke void @__cxa_end_catch()
          to label %86 unwind label %99

86:                                               ; preds = %85
  br label %87

87:                                               ; preds = %86
  br label %88

88:                                               ; preds = %87, %77
  call void @llvm.lifetime.start.p0(ptr %16) #2
  call void @_ZN4taskILb1EE12promise_type13final_suspendEv(ptr noundef nonnull align 8 dereferenceable(8) %9) #2
  %89 = call noundef i1 @_ZN4taskILb1EE12promise_type3fin11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %16) #2
  br i1 %89, label %108, label %90

90:                                               ; preds = %88
  %91 = call token @llvm.coro.save(ptr null)
  invoke void @llvm.coro.await.suspend.handle(ptr %16, ptr %25, ptr @_Z5eagerRll.__await_suspend_wrapper__final)
          to label %92 unwind label %104

92:                                               ; preds = %90
  %93 = call i8 @llvm.coro.suspend(token %91, i1 true)
  switch i8 %93, label %120 [
    i8 0, label %108
    i8 1, label %94
  ]

94:                                               ; preds = %92
  br label %109

95:                                               ; preds = %82
  %96 = landingpad { ptr, i32 }
          cleanup
  %97 = extractvalue { ptr, i32 } %96, 0
  store ptr %97, ptr %10, align 8
  %98 = extractvalue { ptr, i32 } %96, 1
  store i32 %98, ptr %11, align 4
  invoke void @__cxa_end_catch()
          to label %103 unwind label %135

99:                                               ; preds = %85
  %100 = landingpad { ptr, i32 }
          cleanup
  %101 = extractvalue { ptr, i32 } %100, 0
  store ptr %101, ptr %10, align 8
  %102 = extractvalue { ptr, i32 } %100, 1
  store i32 %102, ptr %11, align 4
  br label %121

103:                                              ; preds = %95
  br label %121

104:                                              ; preds = %90
  %105 = landingpad { ptr, i32 }
          cleanup
  %106 = extractvalue { ptr, i32 } %105, 0
  store ptr %106, ptr %10, align 8
  %107 = extractvalue { ptr, i32 } %105, 1
  store i32 %107, ptr %11, align 4
  call void @llvm.lifetime.end.p0(ptr %16) #2
  br label %121

108:                                              ; preds = %92, %88
  call void @_ZN4taskILb1EE12promise_type3fin12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %16) #2
  br label %109

109:                                              ; preds = %108, %94
  %110 = phi i32 [ 0, %108 ], [ 2, %94 ]
  call void @llvm.lifetime.end.p0(ptr %16) #2
  switch i32 %110, label %112 [
    i32 0, label %111
  ]

111:                                              ; preds = %109
  br label %112

112:                                              ; preds = %111, %109, %74, %39
  %113 = phi i32 [ %40, %39 ], [ %75, %74 ], [ %110, %109 ], [ 0, %111 ]
  call void @llvm.lifetime.end.p0(ptr %9) #2
  call void @llvm.lifetime.end.p0(ptr %8) #2
  call void @llvm.lifetime.end.p0(ptr %7) #2
  %114 = call ptr @llvm.coro.free(token %18, ptr %25)
  %115 = icmp ne ptr %114, null
  br i1 %115, label %116, label %118

116:                                              ; preds = %112
  %117 = call i64 @llvm.coro.size.i64()
  call void @_ZdlPvm(ptr noundef %114, i64 noundef %117) #2
  br label %118

118:                                              ; preds = %112, %116
  switch i32 %113, label %138 [
    i32 0, label %119
    i32 2, label %120
  ]

119:                                              ; preds = %118
  br label %120

120:                                              ; preds = %119, %118, %92, %54, %30
  call void @llvm.coro.end(ptr null, i1 false, token none)
  ret void

121:                                              ; preds = %104, %103, %99
  call void @llvm.coro.end(ptr null, i1 true, token none)
  %122 = call i1 @llvm.coro.is_in_ramp()
  br i1 %122, label %123, label %130

123:                                              ; preds = %121
  br label %124

124:                                              ; preds = %123, %34
  call void @llvm.lifetime.end.p0(ptr %9) #2
  call void @llvm.lifetime.end.p0(ptr %8) #2
  call void @llvm.lifetime.end.p0(ptr %7) #2
  %125 = call ptr @llvm.coro.free(token %18, ptr %25)
  %126 = icmp ne ptr %125, null
  br i1 %126, label %127, label %129

127:                                              ; preds = %124
  %128 = call i64 @llvm.coro.size.i64()
  call void @_ZdlPvm(ptr noundef %125, i64 noundef %128) #2
  br label %129

129:                                              ; preds = %124, %127
  br label %130

130:                                              ; preds = %129, %121
  %131 = load ptr, ptr %10, align 8
  %132 = load i32, ptr %11, align 4
  %133 = insertvalue { ptr, i32 } poison, ptr %131, 0
  %134 = insertvalue { ptr, i32 } %133, i32 %132, 1
  resume { ptr, i32 } %134

135:                                              ; preds = %95
  %136 = landingpad { ptr, i32 }
          catch ptr null
  %137 = extractvalue { ptr, i32 } %136, 0
  call void @__clang_call_terminate(ptr %137) #19
  unreachable

138:                                              ; preds = %118
  unreachable
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZN4taskILb1EE12promise_typeC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %0) unnamed_addr #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds nuw %"struct.task<true>::promise_type", ptr %3, i32 0, i32 0
  call void @_ZNSt7__n486116coroutine_handleIvEC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %4) #2
  ret void
}

; Function Attrs: mustprogress noinline optnone uwtable
define linkonce_odr dso_local void @_ZN4taskILb1EE12promise_type17get_return_objectEv(ptr dead_on_unwind noalias writable sret(%struct.task.1) align 8 %0, ptr noundef nonnull align 8 dereferenceable(8) %1) #7 comdat {
  %3 = alloca ptr, align 8
  %4 = alloca ptr, align 8
  store ptr %0, ptr %3, align 8
  store ptr %1, ptr %4, align 8
  %5 = load ptr, ptr %4, align 8
  %6 = getelementptr inbounds nuw %struct.task.1, ptr %0, i32 0, i32 0
  %7 = call i64 @_ZNSt7__n486116coroutine_handleIN4taskILb1EE12promise_typeEE12from_promiseERS3_(ptr noundef nonnull align 8 dereferenceable(8) %5)
  %8 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.2", ptr %6, i32 0, i32 0
  %9 = inttoptr i64 %7 to ptr
  store ptr %9, ptr %8, align 8
  ret void
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZN4taskILb1EE12promise_type15initial_suspendEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  ret void
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local noundef i1 @_ZNKSt7__n486113suspend_never11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %0) #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  ret i1 true
}

; Function Attrs: alwaysinline mustprogress
define internal void @_Z5eagerRll.__await_suspend_wrapper__init(ptr noundef nonnull %0, ptr noundef %1) #9 {
  %3 = alloca ptr, align 8
  %4 = alloca ptr, align 8
  %5 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %6 = alloca %"struct.std::__n4861::coroutine_handle.2", align 8
  store ptr %0, ptr %3, align 8
  store ptr %1, ptr %4, align 8
  %7 = load ptr, ptr %3, align 8
  %8 = load ptr, ptr %4, align 8
  %9 = call i64 @_ZNSt7__n486116coroutine_handleIN4taskILb1EE12promise_typeEE12from_addressEPv(ptr noundef %8) #2
  %10 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.2", ptr %6, i32 0, i32 0
  %11 = inttoptr i64 %9 to ptr
  store ptr %11, ptr %10, align 8
  %12 = call i64 @_ZNKSt7__n486116coroutine_handleIN4taskILb1EE12promise_typeEEcvNS0_IvEEEv(ptr noundef nonnull align 8 dereferenceable(8) %6) #2
  %13 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %5, i32 0, i32 0
  %14 = inttoptr i64 %12 to ptr
  store ptr %14, ptr %13, align 8
  %15 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %5, i32 0, i32 0
  %16 = load ptr, ptr %15, align 8
  call void @_ZNKSt7__n486113suspend_never13await_suspendENS_16coroutine_handleIvEE(ptr noundef nonnull align 1 dereferenceable(1) %7, ptr %16) #2
  ret void
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZNKSt7__n486113suspend_never13await_suspendENS_16coroutine_handleIvEE(ptr noundef nonnull align 1 dereferenceable(1) %0, ptr %1) #6 comdat {
  %3 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %4 = alloca ptr, align 8
  %5 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %3, i32 0, i32 0
  store ptr %1, ptr %5, align 8
  store ptr %0, ptr %4, align 8
  %6 = load ptr, ptr %4, align 8
  ret void
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local i64 @_ZNSt7__n486116coroutine_handleIN4taskILb1EE12promise_typeEE12from_addressEPv(ptr noundef %0) #6 comdat {
  %2 = alloca %"struct.std::__n4861::coroutine_handle.2", align 8
  %3 = alloca ptr, align 8
  store ptr %0, ptr %3, align 8
  call void @_ZNSt7__n486116coroutine_handleIN4taskILb1EE12promise_typeEEC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %2) #2
  %4 = load ptr, ptr %3, align 8
  %5 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.2", ptr %2, i32 0, i32 0
  store ptr %4, ptr %5, align 8
  %6 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.2", ptr %2, i32 0, i32 0
  %7 = load ptr, ptr %6, align 8
  %8 = ptrtoint ptr %7 to i64
  ret i64 %8
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local i64 @_ZNKSt7__n486116coroutine_handleIN4taskILb1EE12promise_typeEEcvNS0_IvEEEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #6 comdat {
  %2 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %3 = alloca ptr, align 8
  store ptr %0, ptr %3, align 8
  %4 = load ptr, ptr %3, align 8
  %5 = call noundef ptr @_ZNKSt7__n486116coroutine_handleIN4taskILb1EE12promise_typeEE7addressEv(ptr noundef nonnull align 8 dereferenceable(8) %4) #2
  %6 = call i64 @_ZNSt7__n486116coroutine_handleIvE12from_addressEPv(ptr noundef %5) #2
  %7 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %2, i32 0, i32 0
  %8 = inttoptr i64 %6 to ptr
  store ptr %8, ptr %7, align 8
  %9 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %2, i32 0, i32 0
  %10 = load ptr, ptr %9, align 8
  %11 = ptrtoint ptr %10 to i64
  ret i64 %11
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZNKSt7__n486113suspend_never12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %0) #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  ret void
}

; Function Attrs: alwaysinline mustprogress
define internal noundef ptr @_Z5eagerRll.__await_suspend_wrapper__await(ptr noundef nonnull %0, ptr noundef %1) #9 {
  %3 = alloca ptr, align 8
  %4 = alloca ptr, align 8
  %5 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %6 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %7 = alloca %"struct.std::__n4861::coroutine_handle.2", align 8
  store ptr %0, ptr %3, align 8
  store ptr %1, ptr %4, align 8
  %8 = load ptr, ptr %3, align 8
  %9 = load ptr, ptr %4, align 8
  %10 = call i64 @_ZNSt7__n486116coroutine_handleIN4taskILb1EE12promise_typeEE12from_addressEPv(ptr noundef %9) #2
  %11 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.2", ptr %7, i32 0, i32 0
  %12 = inttoptr i64 %10 to ptr
  store ptr %12, ptr %11, align 8
  %13 = call i64 @_ZNKSt7__n486116coroutine_handleIN4taskILb1EE12promise_typeEEcvNS0_IvEEEv(ptr noundef nonnull align 8 dereferenceable(8) %7) #2
  %14 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %6, i32 0, i32 0
  %15 = inttoptr i64 %13 to ptr
  store ptr %15, ptr %14, align 8
  %16 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %6, i32 0, i32 0
  %17 = load ptr, ptr %16, align 8
  %18 = call i64 @_ZN4taskILb0EE13await_suspendENSt7__n486116coroutine_handleIvEE(ptr noundef nonnull align 8 dereferenceable(8) %8, ptr %17)
  %19 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %5, i32 0, i32 0
  %20 = inttoptr i64 %18 to ptr
  store ptr %20, ptr %19, align 8
  %21 = call noundef ptr @_ZNKSt7__n486116coroutine_handleIvE7addressEv(ptr noundef nonnull align 8 dereferenceable(8) %5) #2
  ret ptr %21
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZN4taskILb1EE12promise_type11return_voidEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  ret void
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZN4taskILb1EE12promise_type19unhandled_exceptionEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  ret void
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZN4taskILb1EE12promise_type13final_suspendEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  ret void
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local noundef i1 @_ZN4taskILb1EE12promise_type3fin11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %0) #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  ret i1 false
}

; Function Attrs: alwaysinline mustprogress
define internal noundef ptr @_Z5eagerRll.__await_suspend_wrapper__final(ptr noundef nonnull %0, ptr noundef %1) #9 {
  %3 = alloca ptr, align 8
  %4 = alloca ptr, align 8
  %5 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %6 = alloca %"struct.std::__n4861::coroutine_handle.2", align 8
  store ptr %0, ptr %3, align 8
  store ptr %1, ptr %4, align 8
  %7 = load ptr, ptr %3, align 8
  %8 = load ptr, ptr %4, align 8
  %9 = call i64 @_ZNSt7__n486116coroutine_handleIN4taskILb1EE12promise_typeEE12from_addressEPv(ptr noundef %8) #2
  %10 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.2", ptr %6, i32 0, i32 0
  %11 = inttoptr i64 %9 to ptr
  store ptr %11, ptr %10, align 8
  %12 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.2", ptr %6, i32 0, i32 0
  %13 = load ptr, ptr %12, align 8
  %14 = call i64 @_ZN4taskILb1EE12promise_type3fin13await_suspendENSt7__n486116coroutine_handleIS1_EE(ptr noundef nonnull align 1 dereferenceable(1) %7, ptr %13) #2
  %15 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %5, i32 0, i32 0
  %16 = inttoptr i64 %14 to ptr
  store ptr %16, ptr %15, align 8
  %17 = call noundef ptr @_ZNKSt7__n486116coroutine_handleIvE7addressEv(ptr noundef nonnull align 8 dereferenceable(8) %5) #2
  ret ptr %17
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local i64 @_ZN4taskILb1EE12promise_type3fin13await_suspendENSt7__n486116coroutine_handleIS1_EE(ptr noundef nonnull align 1 dereferenceable(1) %0, ptr %1) #6 comdat personality ptr @__gxx_personality_v0 {
  %3 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %4 = alloca %"struct.std::__n4861::coroutine_handle.2", align 8
  %5 = alloca ptr, align 8
  %6 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %7 = alloca %"struct.std::__n4861::coroutine_handle.3", align 8
  %8 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.2", ptr %4, i32 0, i32 0
  store ptr %1, ptr %8, align 8
  store ptr %0, ptr %5, align 8
  %9 = load ptr, ptr %5, align 8
  %10 = invoke noundef nonnull align 8 dereferenceable(8) ptr @_ZNKSt7__n486116coroutine_handleIN4taskILb1EE12promise_typeEE7promiseEv(ptr noundef nonnull align 8 dereferenceable(8) %4)
          to label %11 unwind label %26

11:                                               ; preds = %2
  %12 = getelementptr inbounds nuw %"struct.task<true>::promise_type", ptr %10, i32 0, i32 0
  call void @llvm.memcpy.p0.p0.i64(ptr align 8 %6, ptr align 8 %12, i64 8, i1 false)
  %13 = call noundef i1 @_ZNKSt7__n486116coroutine_handleIvEcvbEv(ptr noundef nonnull align 8 dereferenceable(8) %6) #2
  br i1 %13, label %14, label %15

14:                                               ; preds = %11
  call void @llvm.memcpy.p0.p0.i64(ptr align 8 %3, ptr align 8 %6, i64 8, i1 false)
  br label %22

15:                                               ; preds = %11
  %16 = call i64 @_ZNSt7__n486114noop_coroutineEv() #2
  %17 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.3", ptr %7, i32 0, i32 0
  %18 = inttoptr i64 %16 to ptr
  store ptr %18, ptr %17, align 8
  %19 = call i64 @_ZNKSt7__n486116coroutine_handleINS_22noop_coroutine_promiseEEcvNS0_IvEEEv(ptr noundef nonnull align 8 dereferenceable(8) %7) #2
  %20 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %3, i32 0, i32 0
  %21 = inttoptr i64 %19 to ptr
  store ptr %21, ptr %20, align 8
  br label %22

22:                                               ; preds = %15, %14
  %23 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %3, i32 0, i32 0
  %24 = load ptr, ptr %23, align 8
  %25 = ptrtoint ptr %24 to i64
  ret i64 %25

26:                                               ; preds = %2
  %27 = landingpad { ptr, i32 }
          catch ptr null
  %28 = extractvalue { ptr, i32 } %27, 0
  call void @__clang_call_terminate(ptr %28) #19
  unreachable
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZN4taskILb1EE12promise_type3fin12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %0) #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  ret void
}

; Function Attrs: mustprogress noinline norecurse optnone uwtable
define dso_local noundef i32 @main() #13 personality ptr @__gxx_personality_v0 {
  %1 = alloca i64, align 8
  %2 = alloca %struct.task, align 8
  %3 = alloca ptr, align 8
  %4 = alloca i32, align 4
  %5 = alloca %struct.task.1, align 8
  store i64 0, ptr %1, align 8
  call void @_Z4lazyRll(ptr dead_on_unwind writable sret(%struct.task) align 8 %2, ptr noundef nonnull align 8 dereferenceable(8) %1, i64 noundef 1000000)
  %6 = getelementptr inbounds nuw %struct.task, ptr %2, i32 0, i32 0
  invoke void @_ZNKSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEE6resumeEv(ptr noundef nonnull align 8 dereferenceable(8) %6)
          to label %7 unwind label %28

7:                                                ; preds = %0
  %8 = load i64, ptr %1, align 8
  %9 = getelementptr inbounds nuw %struct.task, ptr %2, i32 0, i32 0
  %10 = call noundef i1 @_ZNKSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEE4doneEv(ptr noundef nonnull align 8 dereferenceable(8) %9) #2
  %11 = zext i1 %10 to i32
  %12 = load i64, ptr @_ZL2hi, align 8
  %13 = load i64, ptr @_ZL2lo, align 8
  %14 = icmp eq i64 %12, %13
  %15 = zext i1 %14 to i32
  %16 = invoke i32 (ptr, ...) @printf(ptr noundef @.str, i64 noundef %8, i32 noundef %11, i32 noundef %15)
          to label %17 unwind label %28

17:                                               ; preds = %7
  call void @_ZN4taskILb0EED2Ev(ptr noundef nonnull align 8 dereferenceable(8) %2) #2
  store i64 0, ptr %1, align 8
  store i64 -1, ptr @_ZL2lo, align 8
  store i64 0, ptr @_ZL2hi, align 8
  call void @_Z5eagerRll(ptr dead_on_unwind writable sret(%struct.task.1) align 8 %5, ptr noundef nonnull align 8 dereferenceable(8) %1, i64 noundef 1000000)
  %18 = load i64, ptr %1, align 8
  %19 = getelementptr inbounds nuw %struct.task.1, ptr %5, i32 0, i32 0
  %20 = call noundef i1 @_ZNKSt7__n486116coroutine_handleIN4taskILb1EE12promise_typeEE4doneEv(ptr noundef nonnull align 8 dereferenceable(8) %19) #2
  %21 = zext i1 %20 to i32
  %22 = load i64, ptr @_ZL2hi, align 8
  %23 = load i64, ptr @_ZL2lo, align 8
  %24 = icmp eq i64 %22, %23
  %25 = zext i1 %24 to i32
  %26 = invoke i32 (ptr, ...) @printf(ptr noundef @.str.1, i64 noundef %18, i32 noundef %21, i32 noundef %25)
          to label %27 unwind label %32

27:                                               ; preds = %17
  call void @_ZN4taskILb1EED2Ev(ptr noundef nonnull align 8 dereferenceable(8) %5) #2
  ret i32 0

28:                                               ; preds = %7, %0
  %29 = landingpad { ptr, i32 }
          cleanup
  %30 = extractvalue { ptr, i32 } %29, 0
  store ptr %30, ptr %3, align 8
  %31 = extractvalue { ptr, i32 } %29, 1
  store i32 %31, ptr %4, align 4
  call void @_ZN4taskILb0EED2Ev(ptr noundef nonnull align 8 dereferenceable(8) %2) #2
  br label %36

32:                                               ; preds = %17
  %33 = landingpad { ptr, i32 }
          cleanup
  %34 = extractvalue { ptr, i32 } %33, 0
  store ptr %34, ptr %3, align 8
  %35 = extractvalue { ptr, i32 } %33, 1
  store i32 %35, ptr %4, align 4
  call void @_ZN4taskILb1EED2Ev(ptr noundef nonnull align 8 dereferenceable(8) %5) #2
  br label %36

36:                                               ; preds = %32, %28
  %37 = load ptr, ptr %3, align 8
  %38 = load i32, ptr %4, align 4
  %39 = insertvalue { ptr, i32 } poison, ptr %37, 0
  %40 = insertvalue { ptr, i32 } %39, i32 %38, 1
  resume { ptr, i32 } %40
}

; Function Attrs: mustprogress noinline optnone uwtable
define linkonce_odr dso_local void @_ZNKSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEE6resumeEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #7 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %3, i32 0, i32 0
  %5 = load ptr, ptr %4, align 8
  call void @llvm.coro.resume(ptr %5)
  ret void
}

declare i32 @printf(ptr noundef, ...) #14

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local noundef i1 @_ZNKSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEE4doneEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %3, i32 0, i32 0
  %5 = load ptr, ptr %4, align 8
  %6 = call i1 @llvm.coro.done(ptr %5)
  ret i1 %6
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local noundef i1 @_ZNKSt7__n486116coroutine_handleIN4taskILb1EE12promise_typeEE4doneEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.2", ptr %3, i32 0, i32 0
  %5 = load ptr, ptr %4, align 8
  %6 = call i1 @llvm.coro.done(ptr %5)
  ret i1 %6
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZN4taskILb1EED2Ev(ptr noundef nonnull align 8 dereferenceable(8) %0) unnamed_addr #6 comdat personality ptr @__gxx_personality_v0 {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds nuw %struct.task.1, ptr %3, i32 0, i32 0
  %5 = call noundef i1 @_ZNKSt7__n486116coroutine_handleIN4taskILb1EE12promise_typeEEcvbEv(ptr noundef nonnull align 8 dereferenceable(8) %4) #2
  br i1 %5, label %6, label %9

6:                                                ; preds = %1
  %7 = getelementptr inbounds nuw %struct.task.1, ptr %3, i32 0, i32 0
  invoke void @_ZNKSt7__n486116coroutine_handleIN4taskILb1EE12promise_typeEE7destroyEv(ptr noundef nonnull align 8 dereferenceable(8) %7)
          to label %8 unwind label %10

8:                                                ; preds = %6
  br label %9

9:                                                ; preds = %8, %1
  ret void

10:                                               ; preds = %6
  %11 = landingpad { ptr, i32 }
          catch ptr null
  %12 = extractvalue { ptr, i32 } %11, 0
  call void @__clang_call_terminate(ptr %12) #19
  unreachable
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

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZNSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEEC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %0) unnamed_addr #6 comdat {
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
define linkonce_odr dso_local noundef ptr @_ZNKSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEE7addressEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %3, i32 0, i32 0
  %5 = load ptr, ptr %4, align 8
  ret ptr %5
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define internal noundef i64 @_ZL4herev() #6 {
  %1 = alloca i8, align 1
  store volatile i8 0, ptr %1, align 1
  %2 = ptrtoint ptr %1 to i64
  ret i64 %2
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZNSt7__n486116coroutine_handleIN4taskILb1EE12promise_typeEEC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %0) unnamed_addr #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.2", ptr %3, i32 0, i32 0
  store ptr null, ptr %4, align 8
  ret void
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local noundef ptr @_ZNKSt7__n486116coroutine_handleIN4taskILb1EE12promise_typeEE7addressEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.2", ptr %3, i32 0, i32 0
  %5 = load ptr, ptr %4, align 8
  ret ptr %5
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local noundef nonnull align 8 dereferenceable(8) ptr @_ZNKSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEE7promiseEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #6 comdat {
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

; Function Attrs: nocallback nofree nounwind willreturn memory(argmem: readwrite)
declare void @llvm.memcpy.p0.p0.i64(ptr noalias writeonly captures(none), ptr noalias readonly captures(none), i64, i1 immarg) #15

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local noundef i1 @_ZNKSt7__n486116coroutine_handleIvEcvbEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %3, i32 0, i32 0
  %5 = load ptr, ptr %4, align 8
  %6 = icmp ne ptr %5, null
  ret i1 %6
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local i64 @_ZNSt7__n486114noop_coroutineEv() #6 comdat {
  %1 = alloca %"struct.std::__n4861::coroutine_handle.3", align 8
  call void @llvm.memset.p0.i64(ptr align 8 %1, i8 0, i64 8, i1 false)
  call void @_ZNSt7__n486116coroutine_handleINS_22noop_coroutine_promiseEEC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %1) #2
  %2 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.3", ptr %1, i32 0, i32 0
  %3 = load ptr, ptr %2, align 8
  %4 = ptrtoint ptr %3 to i64
  ret i64 %4
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
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

; Function Attrs: nounwind memory(none)
declare ptr @llvm.coro.promise(ptr captures(none), i32, i1) #4

; Function Attrs: nocallback nofree nounwind willreturn memory(argmem: write)
declare void @llvm.memset.p0.i64(ptr writeonly captures(none), i8, i64, i1 immarg) #16

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZNSt7__n486116coroutine_handleINS_22noop_coroutine_promiseEEC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %0) unnamed_addr #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.3", ptr %3, i32 0, i32 0
  store ptr @_ZNSt7__n486116coroutine_handleINS_22noop_coroutine_promiseEE5_S_frE, ptr %4, align 8
  ret void
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZNSt7__n486116coroutine_handleINS_22noop_coroutine_promiseEE7__frame22__dummy_resume_destroyEv() #6 comdat {
  ret void
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local noundef ptr @_ZNKSt7__n486116coroutine_handleINS_22noop_coroutine_promiseEE7addressEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.3", ptr %3, i32 0, i32 0
  %5 = load ptr, ptr %4, align 8
  ret ptr %5
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local i64 @_ZNSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEE12from_promiseERS3_(ptr noundef nonnull align 8 dereferenceable(8) %0) #6 comdat {
  %2 = alloca %"struct.std::__n4861::coroutine_handle", align 8
  %3 = alloca ptr, align 8
  store ptr %0, ptr %3, align 8
  call void @_ZNSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEEC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %2) #2
  %4 = load ptr, ptr %3, align 8, !nonnull !6, !align !7
  %5 = call ptr @llvm.coro.promise(ptr %4, i32 8, i1 true)
  %6 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %2, i32 0, i32 0
  store ptr %5, ptr %6, align 8
  %7 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %2, i32 0, i32 0
  %8 = load ptr, ptr %7, align 8
  %9 = ptrtoint ptr %8 to i64
  ret i64 %9
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local noundef i1 @_ZNKSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEEcvbEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %3, i32 0, i32 0
  %5 = load ptr, ptr %4, align 8
  %6 = icmp ne ptr %5, null
  ret i1 %6
}

; Function Attrs: mustprogress noinline optnone uwtable
define linkonce_odr dso_local void @_ZNKSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEE7destroyEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #7 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %3, i32 0, i32 0
  %5 = load ptr, ptr %4, align 8
  call void @llvm.coro.destroy(ptr %5)
  ret void
}

declare void @llvm.coro.destroy(ptr)

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local noundef nonnull align 8 dereferenceable(8) ptr @_ZNKSt7__n486116coroutine_handleIN4taskILb1EE12promise_typeEE7promiseEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #6 comdat {
  %2 = alloca ptr, align 8
  %3 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %4 = load ptr, ptr %2, align 8
  %5 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.2", ptr %4, i32 0, i32 0
  %6 = load ptr, ptr %5, align 8
  %7 = call ptr @llvm.coro.promise(ptr %6, i32 8, i1 false)
  store ptr %7, ptr %3, align 8
  %8 = load ptr, ptr %3, align 8
  ret ptr %8
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local i64 @_ZNSt7__n486116coroutine_handleIN4taskILb1EE12promise_typeEE12from_promiseERS3_(ptr noundef nonnull align 8 dereferenceable(8) %0) #6 comdat {
  %2 = alloca %"struct.std::__n4861::coroutine_handle.2", align 8
  %3 = alloca ptr, align 8
  store ptr %0, ptr %3, align 8
  call void @_ZNSt7__n486116coroutine_handleIN4taskILb1EE12promise_typeEEC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %2) #2
  %4 = load ptr, ptr %3, align 8, !nonnull !6, !align !7
  %5 = call ptr @llvm.coro.promise(ptr %4, i32 8, i1 true)
  %6 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.2", ptr %2, i32 0, i32 0
  store ptr %5, ptr %6, align 8
  %7 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.2", ptr %2, i32 0, i32 0
  %8 = load ptr, ptr %7, align 8
  %9 = ptrtoint ptr %8 to i64
  ret i64 %9
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local noundef i1 @_ZNKSt7__n486116coroutine_handleIN4taskILb1EE12promise_typeEEcvbEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.2", ptr %3, i32 0, i32 0
  %5 = load ptr, ptr %4, align 8
  %6 = icmp ne ptr %5, null
  ret i1 %6
}

; Function Attrs: mustprogress noinline optnone uwtable
define linkonce_odr dso_local void @_ZNKSt7__n486116coroutine_handleIN4taskILb1EE12promise_typeEE7destroyEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #7 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.2", ptr %3, i32 0, i32 0
  %5 = load ptr, ptr %4, align 8
  call void @llvm.coro.destroy(ptr %5)
  ret void
}

declare void @llvm.coro.resume(ptr)

; Function Attrs: nounwind memory(argmem: readwrite)
declare i1 @llvm.coro.done(ptr readonly captures(none)) #17

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
attributes #10 = { noinline noreturn nounwind uwtable "frame-pointer"="non-leaf-no-reserve" "no-trapping-math"="true" "stack-protector-buffer-size"="8" }
attributes #11 = { nobuiltin nounwind "frame-pointer"="non-leaf-no-reserve" "no-trapping-math"="true" "stack-protector-buffer-size"="8" }
attributes #12 = { nounwind memory(argmem: read) }
attributes #13 = { mustprogress noinline norecurse optnone uwtable "frame-pointer"="non-leaf-no-reserve" "no-trapping-math"="true" "stack-protector-buffer-size"="8" }
attributes #14 = { "frame-pointer"="non-leaf-no-reserve" "no-trapping-math"="true" "stack-protector-buffer-size"="8" }
attributes #15 = { nocallback nofree nounwind willreturn memory(argmem: readwrite) }
attributes #16 = { nocallback nofree nounwind willreturn memory(argmem: write) }
attributes #17 = { nounwind memory(argmem: readwrite) }
attributes #18 = { allocsize(0) }
attributes #19 = { noreturn nounwind }

!llvm.module.flags = !{!0, !1, !2, !3, !4}
!llvm.ident = !{!5}

!0 = !{i32 1, !"wchar_size", i32 4}
!1 = !{i32 8, !"PIC Level", i32 2}
!2 = !{i32 7, !"PIE Level", i32 2}
!3 = !{i32 7, !"uwtable", i32 2}
!4 = !{i32 7, !"frame-pointer", i32 4}
!5 = !{!"Debian clang version 22.1.8 (1~deb13u4)"}
!6 = !{}
!7 = !{i64 8}
!8 = distinct !{!8, !9}
!9 = !{!"llvm.loop.mustprogress"}
!10 = distinct !{!10, !9}
