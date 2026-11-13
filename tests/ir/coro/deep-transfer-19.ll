; clang++-19 -std=c++20 -O0 -S -emit-llvm -Xclang -disable-llvm-passes
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
;@ dialect: 19
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
  %4 = alloca ptr, align 8
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
  %13 = bitcast ptr %6 to ptr
  %14 = call token @llvm.coro.id(i32 16, ptr %13, ptr null, ptr null)
  %15 = call i1 @llvm.coro.alloc(token %14)
  br i1 %15, label %16, label %19

16:                                               ; preds = %2
  %17 = call i64 @llvm.coro.size.i64()
  %18 = call noalias noundef nonnull ptr @_Znwm(i64 noundef %17) #18
  br label %19

19:                                               ; preds = %16, %2
  %20 = phi ptr [ null, %2 ], [ %18, %16 ]
  %21 = call ptr @llvm.coro.begin(token %14, ptr %20)
  call void @llvm.lifetime.start.p0(i64 8, ptr %5) #2
  %22 = load ptr, ptr %4, align 8
  store ptr %22, ptr %5, align 8
  call void @llvm.lifetime.start.p0(i64 8, ptr %6) #2
  call void @_ZN4taskILb0EE12promise_typeC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %6) #2
  invoke void @_ZN4taskILb0EE12promise_type17get_return_objectEv(ptr dead_on_unwind writable sret(%struct.task) align 8 %0, ptr noundef nonnull align 8 dereferenceable(8) %6)
          to label %23 unwind label %29

23:                                               ; preds = %19
  call void @llvm.lifetime.start.p0(i64 1, ptr %9) #2
  call void @_ZN4taskILb0EE12promise_type15initial_suspendEv(ptr noundef nonnull align 8 dereferenceable(8) %6) #2
  %24 = call noundef i1 @_ZNKSt7__n486114suspend_always11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %9) #2
  br i1 %24, label %33, label %25

25:                                               ; preds = %23
  %26 = call token @llvm.coro.save(ptr null)
  call void @llvm.coro.await.suspend.void(ptr %9, ptr %21, ptr @_Z4stepRl.__await_suspend_wrapper__init) #2
  %27 = call i8 @llvm.coro.suspend(token %26, i1 false)
  switch i8 %27, label %84 [
    i8 0, label %33
    i8 1, label %28
  ]

28:                                               ; preds = %25
  br label %34

29:                                               ; preds = %19
  %30 = landingpad { ptr, i32 }
          cleanup
  %31 = extractvalue { ptr, i32 } %30, 0
  store ptr %31, ptr %7, align 8
  %32 = extractvalue { ptr, i32 } %30, 1
  store i32 %32, ptr %8, align 4
  br label %89

33:                                               ; preds = %25, %23
  call void @_ZNKSt7__n486114suspend_always12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %9) #2
  br label %34

34:                                               ; preds = %33, %28
  %35 = phi i32 [ 0, %33 ], [ 2, %28 ]
  call void @llvm.lifetime.end.p0(i64 1, ptr %9) #2
  switch i32 %35, label %76 [
    i32 0, label %36
  ]

36:                                               ; preds = %34
  %37 = load ptr, ptr %5, align 8
  %38 = load i64, ptr %37, align 8
  %39 = add nsw i64 %38, 1
  store i64 %39, ptr %37, align 8
  invoke void @_ZL5probev()
          to label %40 unwind label %42

40:                                               ; preds = %36
  invoke void @_ZN4taskILb0EE12promise_type11return_voidEv(ptr noundef nonnull align 8 dereferenceable(8) %6)
          to label %41 unwind label %42

41:                                               ; preds = %40
  br label %52

42:                                               ; preds = %40, %36
  %43 = landingpad { ptr, i32 }
          catch ptr null
  %44 = extractvalue { ptr, i32 } %43, 0
  store ptr %44, ptr %7, align 8
  %45 = extractvalue { ptr, i32 } %43, 1
  store i32 %45, ptr %8, align 4
  br label %46

46:                                               ; preds = %42
  %47 = load ptr, ptr %7, align 8
  %48 = call ptr @__cxa_begin_catch(ptr %47) #2
  invoke void @_ZN4taskILb0EE12promise_type19unhandled_exceptionEv(ptr noundef nonnull align 8 dereferenceable(8) %6)
          to label %49 unwind label %59

49:                                               ; preds = %46
  invoke void @__cxa_end_catch()
          to label %50 unwind label %63

50:                                               ; preds = %49
  br label %51

51:                                               ; preds = %50
  br label %52

52:                                               ; preds = %51, %41
  call void @llvm.lifetime.start.p0(i64 1, ptr %11) #2
  call void @_ZN4taskILb0EE12promise_type13final_suspendEv(ptr noundef nonnull align 8 dereferenceable(8) %6) #2
  %53 = call noundef i1 @_ZN4taskILb0EE12promise_type3fin11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %11) #2
  br i1 %53, label %72, label %54

54:                                               ; preds = %52
  %55 = call token @llvm.coro.save(ptr null)
  invoke void @llvm.coro.await.suspend.handle(ptr %11, ptr %21, ptr @_Z4stepRl.__await_suspend_wrapper__final)
          to label %56 unwind label %68

56:                                               ; preds = %54
  %57 = call i8 @llvm.coro.suspend(token %55, i1 true)
  switch i8 %57, label %84 [
    i8 0, label %72
    i8 1, label %58
  ]

58:                                               ; preds = %56
  br label %73

59:                                               ; preds = %46
  %60 = landingpad { ptr, i32 }
          cleanup
  %61 = extractvalue { ptr, i32 } %60, 0
  store ptr %61, ptr %7, align 8
  %62 = extractvalue { ptr, i32 } %60, 1
  store i32 %62, ptr %8, align 4
  invoke void @__cxa_end_catch()
          to label %67 unwind label %100

63:                                               ; preds = %49
  %64 = landingpad { ptr, i32 }
          cleanup
  %65 = extractvalue { ptr, i32 } %64, 0
  store ptr %65, ptr %7, align 8
  %66 = extractvalue { ptr, i32 } %64, 1
  store i32 %66, ptr %8, align 4
  br label %86

67:                                               ; preds = %59
  br label %86

68:                                               ; preds = %54
  %69 = landingpad { ptr, i32 }
          cleanup
  %70 = extractvalue { ptr, i32 } %69, 0
  store ptr %70, ptr %7, align 8
  %71 = extractvalue { ptr, i32 } %69, 1
  store i32 %71, ptr %8, align 4
  call void @llvm.lifetime.end.p0(i64 1, ptr %11) #2
  br label %86

72:                                               ; preds = %56, %52
  call void @_ZN4taskILb0EE12promise_type3fin12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %11) #2
  br label %73

73:                                               ; preds = %72, %58
  %74 = phi i32 [ 0, %72 ], [ 2, %58 ]
  call void @llvm.lifetime.end.p0(i64 1, ptr %11) #2
  switch i32 %74, label %76 [
    i32 0, label %75
  ]

75:                                               ; preds = %73
  br label %76

76:                                               ; preds = %75, %73, %34
  %77 = phi i32 [ %35, %34 ], [ %74, %73 ], [ 0, %75 ]
  call void @llvm.lifetime.end.p0(i64 8, ptr %6) #2
  call void @llvm.lifetime.end.p0(i64 8, ptr %5) #2
  %78 = call ptr @llvm.coro.free(token %14, ptr %21)
  %79 = icmp ne ptr %78, null
  br i1 %79, label %80, label %82

80:                                               ; preds = %76
  %81 = call i64 @llvm.coro.size.i64()
  call void @_ZdlPvm(ptr noundef %78, i64 noundef %81) #2
  br label %82

82:                                               ; preds = %76, %80
  switch i32 %77, label %103 [
    i32 0, label %83
    i32 2, label %84
  ]

83:                                               ; preds = %82
  br label %84

84:                                               ; preds = %83, %82, %56, %25
  %85 = call i1 @llvm.coro.end(ptr null, i1 false, token none)
  ret void

86:                                               ; preds = %68, %67, %63
  %87 = call i1 @llvm.coro.end(ptr null, i1 true, token none)
  br i1 %87, label %95, label %88

88:                                               ; preds = %86
  br label %89

89:                                               ; preds = %88, %29
  call void @llvm.lifetime.end.p0(i64 8, ptr %6) #2
  call void @llvm.lifetime.end.p0(i64 8, ptr %5) #2
  %90 = call ptr @llvm.coro.free(token %14, ptr %21)
  %91 = icmp ne ptr %90, null
  br i1 %91, label %92, label %94

92:                                               ; preds = %89
  %93 = call i64 @llvm.coro.size.i64()
  call void @_ZdlPvm(ptr noundef %90, i64 noundef %93) #2
  br label %94

94:                                               ; preds = %89, %92
  br label %95

95:                                               ; preds = %94, %86
  %96 = load ptr, ptr %7, align 8
  %97 = load i32, ptr %8, align 4
  %98 = insertvalue { ptr, i32 } poison, ptr %96, 0
  %99 = insertvalue { ptr, i32 } %98, i32 %97, 1
  resume { ptr, i32 } %99

100:                                              ; preds = %59
  %101 = landingpad { ptr, i32 }
          catch ptr null
  %102 = extractvalue { ptr, i32 } %101, 0
  call void @__clang_call_terminate(ptr %102) #19
  unreachable

103:                                              ; preds = %82
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
define linkonce_odr dso_local void @_ZN4taskILb0EE12promise_typeC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %0) unnamed_addr #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds %"struct.task<false>::promise_type", ptr %3, i32 0, i32 0
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
  %6 = getelementptr inbounds %struct.task, ptr %0, i32 0, i32 0
  %7 = call i64 @_ZNSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEE12from_promiseERS3_(ptr noundef nonnull align 8 dereferenceable(8) %5)
  %8 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %6, i32 0, i32 0
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
define private void @_Z4stepRl.__await_suspend_wrapper__init(ptr noundef nonnull %0, ptr noundef %1) #9 {
  %3 = alloca ptr, align 8
  %4 = alloca ptr, align 8
  %5 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %6 = alloca %"struct.std::__n4861::coroutine_handle", align 8
  store ptr %0, ptr %3, align 8
  store ptr %1, ptr %4, align 8
  %7 = load ptr, ptr %3, align 8
  %8 = load ptr, ptr %4, align 8
  %9 = call i64 @_ZNSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEE12from_addressEPv(ptr noundef %8) #2
  %10 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %6, i32 0, i32 0
  %11 = inttoptr i64 %9 to ptr
  store ptr %11, ptr %10, align 8
  %12 = call i64 @_ZNKSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEEcvNS0_IvEEEv(ptr noundef nonnull align 8 dereferenceable(8) %6) #2
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
define linkonce_odr dso_local i64 @_ZNSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEE12from_addressEPv(ptr noundef %0) #6 comdat {
  %2 = alloca %"struct.std::__n4861::coroutine_handle", align 8
  %3 = alloca ptr, align 8
  store ptr %0, ptr %3, align 8
  call void @_ZNSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEEC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %2) #2
  %4 = load ptr, ptr %3, align 8
  %5 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %2, i32 0, i32 0
  store ptr %4, ptr %5, align 8
  %6 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %2, i32 0, i32 0
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
define private noundef ptr @_Z4stepRl.__await_suspend_wrapper__final(ptr noundef nonnull %0, ptr noundef %1) #9 {
  %3 = alloca ptr, align 8
  %4 = alloca ptr, align 8
  %5 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %6 = alloca %"struct.std::__n4861::coroutine_handle", align 8
  store ptr %0, ptr %3, align 8
  store ptr %1, ptr %4, align 8
  %7 = load ptr, ptr %3, align 8
  %8 = load ptr, ptr %4, align 8
  %9 = call i64 @_ZNSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEE12from_addressEPv(ptr noundef %8) #2
  %10 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %6, i32 0, i32 0
  %11 = inttoptr i64 %9 to ptr
  store ptr %11, ptr %10, align 8
  %12 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %6, i32 0, i32 0
  %13 = load ptr, ptr %12, align 8
  %14 = ptrtoint ptr %13 to i64
  %15 = call i64 @_ZN4taskILb0EE12promise_type3fin13await_suspendENSt7__n486116coroutine_handleIS1_EE(ptr noundef nonnull align 1 dereferenceable(1) %7, i64 %14) #2
  %16 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %5, i32 0, i32 0
  %17 = inttoptr i64 %15 to ptr
  store ptr %17, ptr %16, align 8
  %18 = call noundef ptr @_ZNKSt7__n486116coroutine_handleIvE7addressEv(ptr noundef nonnull align 8 dereferenceable(8) %5) #2
  ret ptr %18
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local i64 @_ZN4taskILb0EE12promise_type3fin13await_suspendENSt7__n486116coroutine_handleIS1_EE(ptr noundef nonnull align 1 dereferenceable(1) %0, i64 %1) #6 comdat personality ptr @__gxx_personality_v0 {
  %3 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %4 = alloca %"struct.std::__n4861::coroutine_handle", align 8
  %5 = alloca ptr, align 8
  %6 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %7 = alloca %"struct.std::__n4861::coroutine_handle.3", align 8
  %8 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %4, i32 0, i32 0
  %9 = inttoptr i64 %1 to ptr
  store ptr %9, ptr %8, align 8
  store ptr %0, ptr %5, align 8
  %10 = load ptr, ptr %5, align 8
  %11 = invoke noundef nonnull align 8 dereferenceable(8) ptr @_ZNKSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEE7promiseEv(ptr noundef nonnull align 8 dereferenceable(8) %4)
          to label %12 unwind label %27

12:                                               ; preds = %2
  %13 = getelementptr inbounds %"struct.task<false>::promise_type", ptr %11, i32 0, i32 0
  call void @llvm.memcpy.p0.p0.i64(ptr align 8 %6, ptr align 8 %13, i64 8, i1 false)
  %14 = call noundef i1 @_ZNKSt7__n486116coroutine_handleIvEcvbEv(ptr noundef nonnull align 8 dereferenceable(8) %6) #2
  br i1 %14, label %15, label %16

15:                                               ; preds = %12
  call void @llvm.memcpy.p0.p0.i64(ptr align 8 %3, ptr align 8 %6, i64 8, i1 false)
  br label %23

16:                                               ; preds = %12
  %17 = call i64 @_ZNSt7__n486114noop_coroutineEv() #2
  %18 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.3", ptr %7, i32 0, i32 0
  %19 = inttoptr i64 %17 to ptr
  store ptr %19, ptr %18, align 8
  %20 = call i64 @_ZNKSt7__n486116coroutine_handleINS_22noop_coroutine_promiseEEcvNS0_IvEEEv(ptr noundef nonnull align 8 dereferenceable(8) %7) #2
  %21 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %3, i32 0, i32 0
  %22 = inttoptr i64 %20 to ptr
  store ptr %22, ptr %21, align 8
  br label %23

23:                                               ; preds = %16, %15
  %24 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %3, i32 0, i32 0
  %25 = load ptr, ptr %24, align 8
  %26 = ptrtoint ptr %25 to i64
  ret i64 %26

27:                                               ; preds = %2
  %28 = landingpad { ptr, i32 }
          catch ptr null
  %29 = extractvalue { ptr, i32 } %28, 0
  call void @__clang_call_terminate(ptr %29) #19
  unreachable
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local noundef ptr @_ZNKSt7__n486116coroutine_handleIvE7addressEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %3, i32 0, i32 0
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
declare i1 @llvm.coro.end(ptr, i1, token) #2

; Function Attrs: nobuiltin nounwind
declare void @_ZdlPvm(ptr noundef, i64 noundef) #11

; Function Attrs: nounwind memory(argmem: read)
declare ptr @llvm.coro.free(token, ptr nocapture readonly) #12

; Function Attrs: mustprogress noinline optnone presplitcoroutine uwtable
define dso_local void @_Z4lazyRll(ptr dead_on_unwind noalias writable sret(%struct.task) align 8 %0, ptr noundef nonnull align 8 dereferenceable(8) %1, i64 noundef %2) #0 personality ptr @__gxx_personality_v0 {
  %4 = alloca ptr, align 8
  %5 = alloca ptr, align 8
  %6 = alloca i64, align 8
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
  %18 = bitcast ptr %9 to ptr
  %19 = call token @llvm.coro.id(i32 16, ptr %18, ptr null, ptr null)
  %20 = call i1 @llvm.coro.alloc(token %19)
  br i1 %20, label %21, label %24

21:                                               ; preds = %3
  %22 = call i64 @llvm.coro.size.i64()
  %23 = call noalias noundef nonnull ptr @_Znwm(i64 noundef %22) #18
  br label %24

24:                                               ; preds = %21, %3
  %25 = phi ptr [ null, %3 ], [ %23, %21 ]
  %26 = call ptr @llvm.coro.begin(token %19, ptr %25)
  call void @llvm.lifetime.start.p0(i64 8, ptr %7) #2
  %27 = load ptr, ptr %5, align 8
  store ptr %27, ptr %7, align 8
  call void @llvm.lifetime.start.p0(i64 8, ptr %8) #2
  %28 = load i64, ptr %6, align 8
  store i64 %28, ptr %8, align 8
  call void @llvm.lifetime.start.p0(i64 8, ptr %9) #2
  call void @_ZN4taskILb0EE12promise_typeC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %9) #2
  invoke void @_ZN4taskILb0EE12promise_type17get_return_objectEv(ptr dead_on_unwind writable sret(%struct.task) align 8 %0, ptr noundef nonnull align 8 dereferenceable(8) %9)
          to label %29 unwind label %35

29:                                               ; preds = %24
  call void @llvm.lifetime.start.p0(i64 1, ptr %12) #2
  call void @_ZN4taskILb0EE12promise_type15initial_suspendEv(ptr noundef nonnull align 8 dereferenceable(8) %9) #2
  %30 = call noundef i1 @_ZNKSt7__n486114suspend_always11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %12) #2
  br i1 %30, label %39, label %31

31:                                               ; preds = %29
  %32 = call token @llvm.coro.save(ptr null)
  call void @llvm.coro.await.suspend.void(ptr %12, ptr %26, ptr @_Z4lazyRll.__await_suspend_wrapper__init) #2
  %33 = call i8 @llvm.coro.suspend(token %32, i1 false)
  switch i8 %33, label %121 [
    i8 0, label %39
    i8 1, label %34
  ]

34:                                               ; preds = %31
  br label %40

35:                                               ; preds = %24
  %36 = landingpad { ptr, i32 }
          cleanup
  %37 = extractvalue { ptr, i32 } %36, 0
  store ptr %37, ptr %10, align 8
  %38 = extractvalue { ptr, i32 } %36, 1
  store i32 %38, ptr %11, align 4
  br label %126

39:                                               ; preds = %31, %29
  call void @_ZNKSt7__n486114suspend_always12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %12) #2
  br label %40

40:                                               ; preds = %39, %34
  %41 = phi i32 [ 0, %39 ], [ 2, %34 ]
  call void @llvm.lifetime.end.p0(i64 1, ptr %12) #2
  switch i32 %41, label %113 [
    i32 0, label %42
  ]

42:                                               ; preds = %40
  call void @llvm.lifetime.start.p0(i64 8, ptr %14) #2
  store i64 0, ptr %14, align 8
  br label %43

43:                                               ; preds = %71, %42
  %44 = load i64, ptr %14, align 8
  %45 = load i64, ptr %8, align 8
  %46 = icmp slt i64 %44, %45
  br i1 %46, label %48, label %47

47:                                               ; preds = %43
  br label %75

48:                                               ; preds = %43
  call void @llvm.lifetime.start.p0(i64 8, ptr %15) #2
  %49 = load ptr, ptr %7, align 8
  invoke void @_Z4stepRl(ptr dead_on_unwind writable sret(%struct.task) align 8 %15, ptr noundef nonnull align 8 dereferenceable(8) %49)
          to label %50 unwind label %58

50:                                               ; preds = %48
  %51 = invoke noundef i1 @_ZN4taskILb0EE11await_readyEv(ptr noundef nonnull align 8 dereferenceable(8) %15)
          to label %52 unwind label %62

52:                                               ; preds = %50
  br i1 %51, label %66, label %53

53:                                               ; preds = %52
  %54 = call token @llvm.coro.save(ptr null)
  invoke void @llvm.coro.await.suspend.handle(ptr %15, ptr %26, ptr @_Z4lazyRll.__await_suspend_wrapper__await)
          to label %55 unwind label %62

55:                                               ; preds = %53
  %56 = call i8 @llvm.coro.suspend(token %54, i1 false)
  switch i8 %56, label %121 [
    i8 0, label %66
    i8 1, label %57
  ]

57:                                               ; preds = %55
  br label %68

58:                                               ; preds = %48
  %59 = landingpad { ptr, i32 }
          catch ptr null
  %60 = extractvalue { ptr, i32 } %59, 0
  store ptr %60, ptr %10, align 8
  %61 = extractvalue { ptr, i32 } %59, 1
  store i32 %61, ptr %11, align 4
  br label %74

62:                                               ; preds = %66, %53, %50
  %63 = landingpad { ptr, i32 }
          catch ptr null
  %64 = extractvalue { ptr, i32 } %63, 0
  store ptr %64, ptr %10, align 8
  %65 = extractvalue { ptr, i32 } %63, 1
  store i32 %65, ptr %11, align 4
  call void @_ZN4taskILb0EED2Ev(ptr noundef nonnull align 8 dereferenceable(8) %15) #2
  br label %74

66:                                               ; preds = %55, %52
  invoke void @_ZN4taskILb0EE12await_resumeEv(ptr noundef nonnull align 8 dereferenceable(8) %15)
          to label %67 unwind label %62

67:                                               ; preds = %66
  br label %68

68:                                               ; preds = %67, %57
  %69 = phi i32 [ 0, %67 ], [ 2, %57 ]
  call void @_ZN4taskILb0EED2Ev(ptr noundef nonnull align 8 dereferenceable(8) %15) #2
  call void @llvm.lifetime.end.p0(i64 8, ptr %15) #2
  switch i32 %69, label %75 [
    i32 0, label %70
  ]

70:                                               ; preds = %68
  br label %71

71:                                               ; preds = %70
  %72 = load i64, ptr %14, align 8
  %73 = add nsw i64 %72, 1
  store i64 %73, ptr %14, align 8
  br label %43, !llvm.loop !6

74:                                               ; preds = %62, %58
  call void @llvm.lifetime.end.p0(i64 8, ptr %15) #2
  call void @llvm.lifetime.end.p0(i64 8, ptr %14) #2
  br label %83

75:                                               ; preds = %68, %47
  %76 = phi i32 [ %69, %68 ], [ 4, %47 ]
  call void @llvm.lifetime.end.p0(i64 8, ptr %14) #2
  switch i32 %76, label %113 [
    i32 4, label %77
  ]

77:                                               ; preds = %75
  invoke void @_ZN4taskILb0EE12promise_type11return_voidEv(ptr noundef nonnull align 8 dereferenceable(8) %9)
          to label %78 unwind label %79

78:                                               ; preds = %77
  br label %89

79:                                               ; preds = %77
  %80 = landingpad { ptr, i32 }
          catch ptr null
  %81 = extractvalue { ptr, i32 } %80, 0
  store ptr %81, ptr %10, align 8
  %82 = extractvalue { ptr, i32 } %80, 1
  store i32 %82, ptr %11, align 4
  br label %83

83:                                               ; preds = %79, %74
  %84 = load ptr, ptr %10, align 8
  %85 = call ptr @__cxa_begin_catch(ptr %84) #2
  invoke void @_ZN4taskILb0EE12promise_type19unhandled_exceptionEv(ptr noundef nonnull align 8 dereferenceable(8) %9)
          to label %86 unwind label %96

86:                                               ; preds = %83
  invoke void @__cxa_end_catch()
          to label %87 unwind label %100

87:                                               ; preds = %86
  br label %88

88:                                               ; preds = %87
  br label %89

89:                                               ; preds = %88, %78
  call void @llvm.lifetime.start.p0(i64 1, ptr %16) #2
  call void @_ZN4taskILb0EE12promise_type13final_suspendEv(ptr noundef nonnull align 8 dereferenceable(8) %9) #2
  %90 = call noundef i1 @_ZN4taskILb0EE12promise_type3fin11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %16) #2
  br i1 %90, label %109, label %91

91:                                               ; preds = %89
  %92 = call token @llvm.coro.save(ptr null)
  invoke void @llvm.coro.await.suspend.handle(ptr %16, ptr %26, ptr @_Z4lazyRll.__await_suspend_wrapper__final)
          to label %93 unwind label %105

93:                                               ; preds = %91
  %94 = call i8 @llvm.coro.suspend(token %92, i1 true)
  switch i8 %94, label %121 [
    i8 0, label %109
    i8 1, label %95
  ]

95:                                               ; preds = %93
  br label %110

96:                                               ; preds = %83
  %97 = landingpad { ptr, i32 }
          cleanup
  %98 = extractvalue { ptr, i32 } %97, 0
  store ptr %98, ptr %10, align 8
  %99 = extractvalue { ptr, i32 } %97, 1
  store i32 %99, ptr %11, align 4
  invoke void @__cxa_end_catch()
          to label %104 unwind label %137

100:                                              ; preds = %86
  %101 = landingpad { ptr, i32 }
          cleanup
  %102 = extractvalue { ptr, i32 } %101, 0
  store ptr %102, ptr %10, align 8
  %103 = extractvalue { ptr, i32 } %101, 1
  store i32 %103, ptr %11, align 4
  br label %123

104:                                              ; preds = %96
  br label %123

105:                                              ; preds = %91
  %106 = landingpad { ptr, i32 }
          cleanup
  %107 = extractvalue { ptr, i32 } %106, 0
  store ptr %107, ptr %10, align 8
  %108 = extractvalue { ptr, i32 } %106, 1
  store i32 %108, ptr %11, align 4
  call void @llvm.lifetime.end.p0(i64 1, ptr %16) #2
  br label %123

109:                                              ; preds = %93, %89
  call void @_ZN4taskILb0EE12promise_type3fin12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %16) #2
  br label %110

110:                                              ; preds = %109, %95
  %111 = phi i32 [ 0, %109 ], [ 2, %95 ]
  call void @llvm.lifetime.end.p0(i64 1, ptr %16) #2
  switch i32 %111, label %113 [
    i32 0, label %112
  ]

112:                                              ; preds = %110
  br label %113

113:                                              ; preds = %112, %110, %75, %40
  %114 = phi i32 [ %41, %40 ], [ %76, %75 ], [ %111, %110 ], [ 0, %112 ]
  call void @llvm.lifetime.end.p0(i64 8, ptr %9) #2
  call void @llvm.lifetime.end.p0(i64 8, ptr %8) #2
  call void @llvm.lifetime.end.p0(i64 8, ptr %7) #2
  %115 = call ptr @llvm.coro.free(token %19, ptr %26)
  %116 = icmp ne ptr %115, null
  br i1 %116, label %117, label %119

117:                                              ; preds = %113
  %118 = call i64 @llvm.coro.size.i64()
  call void @_ZdlPvm(ptr noundef %115, i64 noundef %118) #2
  br label %119

119:                                              ; preds = %113, %117
  switch i32 %114, label %140 [
    i32 0, label %120
    i32 2, label %121
  ]

120:                                              ; preds = %119
  br label %121

121:                                              ; preds = %120, %119, %93, %55, %31
  %122 = call i1 @llvm.coro.end(ptr null, i1 false, token none)
  ret void

123:                                              ; preds = %105, %104, %100
  %124 = call i1 @llvm.coro.end(ptr null, i1 true, token none)
  br i1 %124, label %132, label %125

125:                                              ; preds = %123
  br label %126

126:                                              ; preds = %125, %35
  call void @llvm.lifetime.end.p0(i64 8, ptr %9) #2
  call void @llvm.lifetime.end.p0(i64 8, ptr %8) #2
  call void @llvm.lifetime.end.p0(i64 8, ptr %7) #2
  %127 = call ptr @llvm.coro.free(token %19, ptr %26)
  %128 = icmp ne ptr %127, null
  br i1 %128, label %129, label %131

129:                                              ; preds = %126
  %130 = call i64 @llvm.coro.size.i64()
  call void @_ZdlPvm(ptr noundef %127, i64 noundef %130) #2
  br label %131

131:                                              ; preds = %126, %129
  br label %132

132:                                              ; preds = %131, %123
  %133 = load ptr, ptr %10, align 8
  %134 = load i32, ptr %11, align 4
  %135 = insertvalue { ptr, i32 } poison, ptr %133, 0
  %136 = insertvalue { ptr, i32 } %135, i32 %134, 1
  resume { ptr, i32 } %136

137:                                              ; preds = %96
  %138 = landingpad { ptr, i32 }
          catch ptr null
  %139 = extractvalue { ptr, i32 } %138, 0
  call void @__clang_call_terminate(ptr %139) #19
  unreachable

140:                                              ; preds = %119
  unreachable
}

; Function Attrs: alwaysinline mustprogress
define private void @_Z4lazyRll.__await_suspend_wrapper__init(ptr noundef nonnull %0, ptr noundef %1) #9 {
  %3 = alloca ptr, align 8
  %4 = alloca ptr, align 8
  %5 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %6 = alloca %"struct.std::__n4861::coroutine_handle", align 8
  store ptr %0, ptr %3, align 8
  store ptr %1, ptr %4, align 8
  %7 = load ptr, ptr %3, align 8
  %8 = load ptr, ptr %4, align 8
  %9 = call i64 @_ZNSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEE12from_addressEPv(ptr noundef %8) #2
  %10 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %6, i32 0, i32 0
  %11 = inttoptr i64 %9 to ptr
  store ptr %11, ptr %10, align 8
  %12 = call i64 @_ZNKSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEEcvNS0_IvEEEv(ptr noundef nonnull align 8 dereferenceable(8) %6) #2
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
define linkonce_odr dso_local noundef i1 @_ZN4taskILb0EE11await_readyEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  ret i1 false
}

; Function Attrs: alwaysinline mustprogress
define private noundef ptr @_Z4lazyRll.__await_suspend_wrapper__await(ptr noundef nonnull %0, ptr noundef %1) #9 {
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
  %11 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %7, i32 0, i32 0
  %12 = inttoptr i64 %10 to ptr
  store ptr %12, ptr %11, align 8
  %13 = call i64 @_ZNKSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEEcvNS0_IvEEEv(ptr noundef nonnull align 8 dereferenceable(8) %7) #2
  %14 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %6, i32 0, i32 0
  %15 = inttoptr i64 %13 to ptr
  store ptr %15, ptr %14, align 8
  %16 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %6, i32 0, i32 0
  %17 = load ptr, ptr %16, align 8
  %18 = ptrtoint ptr %17 to i64
  %19 = call i64 @_ZN4taskILb0EE13await_suspendENSt7__n486116coroutine_handleIvEE(ptr noundef nonnull align 8 dereferenceable(8) %8, i64 %18)
  %20 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %5, i32 0, i32 0
  %21 = inttoptr i64 %19 to ptr
  store ptr %21, ptr %20, align 8
  %22 = call noundef ptr @_ZNKSt7__n486116coroutine_handleIvE7addressEv(ptr noundef nonnull align 8 dereferenceable(8) %5) #2
  ret ptr %22
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local i64 @_ZN4taskILb0EE13await_suspendENSt7__n486116coroutine_handleIvEE(ptr noundef nonnull align 8 dereferenceable(8) %0, i64 %1) #6 comdat {
  %3 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %4 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %5 = alloca ptr, align 8
  %6 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %4, i32 0, i32 0
  %7 = inttoptr i64 %1 to ptr
  store ptr %7, ptr %6, align 8
  store ptr %0, ptr %5, align 8
  %8 = load ptr, ptr %5, align 8
  %9 = getelementptr inbounds %struct.task, ptr %8, i32 0, i32 0
  %10 = call noundef nonnull align 8 dereferenceable(8) ptr @_ZNKSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEE7promiseEv(ptr noundef nonnull align 8 dereferenceable(8) %9)
  %11 = getelementptr inbounds %"struct.task<false>::promise_type", ptr %10, i32 0, i32 0
  call void @llvm.memcpy.p0.p0.i64(ptr align 8 %11, ptr align 8 %4, i64 8, i1 false)
  %12 = getelementptr inbounds %struct.task, ptr %8, i32 0, i32 0
  %13 = call i64 @_ZNKSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEEcvNS0_IvEEEv(ptr noundef nonnull align 8 dereferenceable(8) %12) #2
  %14 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %3, i32 0, i32 0
  %15 = inttoptr i64 %13 to ptr
  store ptr %15, ptr %14, align 8
  %16 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %3, i32 0, i32 0
  %17 = load ptr, ptr %16, align 8
  %18 = ptrtoint ptr %17 to i64
  ret i64 %18
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
  %4 = getelementptr inbounds %struct.task, ptr %3, i32 0, i32 0
  %5 = call noundef i1 @_ZNKSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEEcvbEv(ptr noundef nonnull align 8 dereferenceable(8) %4) #2
  br i1 %5, label %6, label %9

6:                                                ; preds = %1
  %7 = getelementptr inbounds %struct.task, ptr %3, i32 0, i32 0
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
define private noundef ptr @_Z4lazyRll.__await_suspend_wrapper__final(ptr noundef nonnull %0, ptr noundef %1) #9 {
  %3 = alloca ptr, align 8
  %4 = alloca ptr, align 8
  %5 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %6 = alloca %"struct.std::__n4861::coroutine_handle", align 8
  store ptr %0, ptr %3, align 8
  store ptr %1, ptr %4, align 8
  %7 = load ptr, ptr %3, align 8
  %8 = load ptr, ptr %4, align 8
  %9 = call i64 @_ZNSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEE12from_addressEPv(ptr noundef %8) #2
  %10 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %6, i32 0, i32 0
  %11 = inttoptr i64 %9 to ptr
  store ptr %11, ptr %10, align 8
  %12 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %6, i32 0, i32 0
  %13 = load ptr, ptr %12, align 8
  %14 = ptrtoint ptr %13 to i64
  %15 = call i64 @_ZN4taskILb0EE12promise_type3fin13await_suspendENSt7__n486116coroutine_handleIS1_EE(ptr noundef nonnull align 1 dereferenceable(1) %7, i64 %14) #2
  %16 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %5, i32 0, i32 0
  %17 = inttoptr i64 %15 to ptr
  store ptr %17, ptr %16, align 8
  %18 = call noundef ptr @_ZNKSt7__n486116coroutine_handleIvE7addressEv(ptr noundef nonnull align 8 dereferenceable(8) %5) #2
  ret ptr %18
}

; Function Attrs: mustprogress noinline optnone presplitcoroutine uwtable
define dso_local void @_Z5eagerRll(ptr dead_on_unwind noalias writable sret(%struct.task.1) align 8 %0, ptr noundef nonnull align 8 dereferenceable(8) %1, i64 noundef %2) #0 personality ptr @__gxx_personality_v0 {
  %4 = alloca ptr, align 8
  %5 = alloca ptr, align 8
  %6 = alloca i64, align 8
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
  %18 = bitcast ptr %9 to ptr
  %19 = call token @llvm.coro.id(i32 16, ptr %18, ptr null, ptr null)
  %20 = call i1 @llvm.coro.alloc(token %19)
  br i1 %20, label %21, label %24

21:                                               ; preds = %3
  %22 = call i64 @llvm.coro.size.i64()
  %23 = call noalias noundef nonnull ptr @_Znwm(i64 noundef %22) #18
  br label %24

24:                                               ; preds = %21, %3
  %25 = phi ptr [ null, %3 ], [ %23, %21 ]
  %26 = call ptr @llvm.coro.begin(token %19, ptr %25)
  call void @llvm.lifetime.start.p0(i64 8, ptr %7) #2
  %27 = load ptr, ptr %5, align 8
  store ptr %27, ptr %7, align 8
  call void @llvm.lifetime.start.p0(i64 8, ptr %8) #2
  %28 = load i64, ptr %6, align 8
  store i64 %28, ptr %8, align 8
  call void @llvm.lifetime.start.p0(i64 8, ptr %9) #2
  call void @_ZN4taskILb1EE12promise_typeC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %9) #2
  invoke void @_ZN4taskILb1EE12promise_type17get_return_objectEv(ptr dead_on_unwind writable sret(%struct.task.1) align 8 %0, ptr noundef nonnull align 8 dereferenceable(8) %9)
          to label %29 unwind label %35

29:                                               ; preds = %24
  call void @llvm.lifetime.start.p0(i64 1, ptr %12) #2
  call void @_ZN4taskILb1EE12promise_type15initial_suspendEv(ptr noundef nonnull align 8 dereferenceable(8) %9) #2
  %30 = call noundef i1 @_ZNKSt7__n486113suspend_never11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %12) #2
  br i1 %30, label %39, label %31

31:                                               ; preds = %29
  %32 = call token @llvm.coro.save(ptr null)
  call void @llvm.coro.await.suspend.void(ptr %12, ptr %26, ptr @_Z5eagerRll.__await_suspend_wrapper__init) #2
  %33 = call i8 @llvm.coro.suspend(token %32, i1 false)
  switch i8 %33, label %121 [
    i8 0, label %39
    i8 1, label %34
  ]

34:                                               ; preds = %31
  br label %40

35:                                               ; preds = %24
  %36 = landingpad { ptr, i32 }
          cleanup
  %37 = extractvalue { ptr, i32 } %36, 0
  store ptr %37, ptr %10, align 8
  %38 = extractvalue { ptr, i32 } %36, 1
  store i32 %38, ptr %11, align 4
  br label %126

39:                                               ; preds = %31, %29
  call void @_ZNKSt7__n486113suspend_never12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %12) #2
  br label %40

40:                                               ; preds = %39, %34
  %41 = phi i32 [ 0, %39 ], [ 2, %34 ]
  call void @llvm.lifetime.end.p0(i64 1, ptr %12) #2
  switch i32 %41, label %113 [
    i32 0, label %42
  ]

42:                                               ; preds = %40
  call void @llvm.lifetime.start.p0(i64 8, ptr %14) #2
  store i64 0, ptr %14, align 8
  br label %43

43:                                               ; preds = %71, %42
  %44 = load i64, ptr %14, align 8
  %45 = load i64, ptr %8, align 8
  %46 = icmp slt i64 %44, %45
  br i1 %46, label %48, label %47

47:                                               ; preds = %43
  br label %75

48:                                               ; preds = %43
  call void @llvm.lifetime.start.p0(i64 8, ptr %15) #2
  %49 = load ptr, ptr %7, align 8
  invoke void @_Z4stepRl(ptr dead_on_unwind writable sret(%struct.task) align 8 %15, ptr noundef nonnull align 8 dereferenceable(8) %49)
          to label %50 unwind label %58

50:                                               ; preds = %48
  %51 = invoke noundef i1 @_ZN4taskILb0EE11await_readyEv(ptr noundef nonnull align 8 dereferenceable(8) %15)
          to label %52 unwind label %62

52:                                               ; preds = %50
  br i1 %51, label %66, label %53

53:                                               ; preds = %52
  %54 = call token @llvm.coro.save(ptr null)
  invoke void @llvm.coro.await.suspend.handle(ptr %15, ptr %26, ptr @_Z5eagerRll.__await_suspend_wrapper__await)
          to label %55 unwind label %62

55:                                               ; preds = %53
  %56 = call i8 @llvm.coro.suspend(token %54, i1 false)
  switch i8 %56, label %121 [
    i8 0, label %66
    i8 1, label %57
  ]

57:                                               ; preds = %55
  br label %68

58:                                               ; preds = %48
  %59 = landingpad { ptr, i32 }
          catch ptr null
  %60 = extractvalue { ptr, i32 } %59, 0
  store ptr %60, ptr %10, align 8
  %61 = extractvalue { ptr, i32 } %59, 1
  store i32 %61, ptr %11, align 4
  br label %74

62:                                               ; preds = %66, %53, %50
  %63 = landingpad { ptr, i32 }
          catch ptr null
  %64 = extractvalue { ptr, i32 } %63, 0
  store ptr %64, ptr %10, align 8
  %65 = extractvalue { ptr, i32 } %63, 1
  store i32 %65, ptr %11, align 4
  call void @_ZN4taskILb0EED2Ev(ptr noundef nonnull align 8 dereferenceable(8) %15) #2
  br label %74

66:                                               ; preds = %55, %52
  invoke void @_ZN4taskILb0EE12await_resumeEv(ptr noundef nonnull align 8 dereferenceable(8) %15)
          to label %67 unwind label %62

67:                                               ; preds = %66
  br label %68

68:                                               ; preds = %67, %57
  %69 = phi i32 [ 0, %67 ], [ 2, %57 ]
  call void @_ZN4taskILb0EED2Ev(ptr noundef nonnull align 8 dereferenceable(8) %15) #2
  call void @llvm.lifetime.end.p0(i64 8, ptr %15) #2
  switch i32 %69, label %75 [
    i32 0, label %70
  ]

70:                                               ; preds = %68
  br label %71

71:                                               ; preds = %70
  %72 = load i64, ptr %14, align 8
  %73 = add nsw i64 %72, 1
  store i64 %73, ptr %14, align 8
  br label %43, !llvm.loop !8

74:                                               ; preds = %62, %58
  call void @llvm.lifetime.end.p0(i64 8, ptr %15) #2
  call void @llvm.lifetime.end.p0(i64 8, ptr %14) #2
  br label %83

75:                                               ; preds = %68, %47
  %76 = phi i32 [ %69, %68 ], [ 4, %47 ]
  call void @llvm.lifetime.end.p0(i64 8, ptr %14) #2
  switch i32 %76, label %113 [
    i32 4, label %77
  ]

77:                                               ; preds = %75
  invoke void @_ZN4taskILb1EE12promise_type11return_voidEv(ptr noundef nonnull align 8 dereferenceable(8) %9)
          to label %78 unwind label %79

78:                                               ; preds = %77
  br label %89

79:                                               ; preds = %77
  %80 = landingpad { ptr, i32 }
          catch ptr null
  %81 = extractvalue { ptr, i32 } %80, 0
  store ptr %81, ptr %10, align 8
  %82 = extractvalue { ptr, i32 } %80, 1
  store i32 %82, ptr %11, align 4
  br label %83

83:                                               ; preds = %79, %74
  %84 = load ptr, ptr %10, align 8
  %85 = call ptr @__cxa_begin_catch(ptr %84) #2
  invoke void @_ZN4taskILb1EE12promise_type19unhandled_exceptionEv(ptr noundef nonnull align 8 dereferenceable(8) %9)
          to label %86 unwind label %96

86:                                               ; preds = %83
  invoke void @__cxa_end_catch()
          to label %87 unwind label %100

87:                                               ; preds = %86
  br label %88

88:                                               ; preds = %87
  br label %89

89:                                               ; preds = %88, %78
  call void @llvm.lifetime.start.p0(i64 1, ptr %16) #2
  call void @_ZN4taskILb1EE12promise_type13final_suspendEv(ptr noundef nonnull align 8 dereferenceable(8) %9) #2
  %90 = call noundef i1 @_ZN4taskILb1EE12promise_type3fin11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %16) #2
  br i1 %90, label %109, label %91

91:                                               ; preds = %89
  %92 = call token @llvm.coro.save(ptr null)
  invoke void @llvm.coro.await.suspend.handle(ptr %16, ptr %26, ptr @_Z5eagerRll.__await_suspend_wrapper__final)
          to label %93 unwind label %105

93:                                               ; preds = %91
  %94 = call i8 @llvm.coro.suspend(token %92, i1 true)
  switch i8 %94, label %121 [
    i8 0, label %109
    i8 1, label %95
  ]

95:                                               ; preds = %93
  br label %110

96:                                               ; preds = %83
  %97 = landingpad { ptr, i32 }
          cleanup
  %98 = extractvalue { ptr, i32 } %97, 0
  store ptr %98, ptr %10, align 8
  %99 = extractvalue { ptr, i32 } %97, 1
  store i32 %99, ptr %11, align 4
  invoke void @__cxa_end_catch()
          to label %104 unwind label %137

100:                                              ; preds = %86
  %101 = landingpad { ptr, i32 }
          cleanup
  %102 = extractvalue { ptr, i32 } %101, 0
  store ptr %102, ptr %10, align 8
  %103 = extractvalue { ptr, i32 } %101, 1
  store i32 %103, ptr %11, align 4
  br label %123

104:                                              ; preds = %96
  br label %123

105:                                              ; preds = %91
  %106 = landingpad { ptr, i32 }
          cleanup
  %107 = extractvalue { ptr, i32 } %106, 0
  store ptr %107, ptr %10, align 8
  %108 = extractvalue { ptr, i32 } %106, 1
  store i32 %108, ptr %11, align 4
  call void @llvm.lifetime.end.p0(i64 1, ptr %16) #2
  br label %123

109:                                              ; preds = %93, %89
  call void @_ZN4taskILb1EE12promise_type3fin12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %16) #2
  br label %110

110:                                              ; preds = %109, %95
  %111 = phi i32 [ 0, %109 ], [ 2, %95 ]
  call void @llvm.lifetime.end.p0(i64 1, ptr %16) #2
  switch i32 %111, label %113 [
    i32 0, label %112
  ]

112:                                              ; preds = %110
  br label %113

113:                                              ; preds = %112, %110, %75, %40
  %114 = phi i32 [ %41, %40 ], [ %76, %75 ], [ %111, %110 ], [ 0, %112 ]
  call void @llvm.lifetime.end.p0(i64 8, ptr %9) #2
  call void @llvm.lifetime.end.p0(i64 8, ptr %8) #2
  call void @llvm.lifetime.end.p0(i64 8, ptr %7) #2
  %115 = call ptr @llvm.coro.free(token %19, ptr %26)
  %116 = icmp ne ptr %115, null
  br i1 %116, label %117, label %119

117:                                              ; preds = %113
  %118 = call i64 @llvm.coro.size.i64()
  call void @_ZdlPvm(ptr noundef %115, i64 noundef %118) #2
  br label %119

119:                                              ; preds = %113, %117
  switch i32 %114, label %140 [
    i32 0, label %120
    i32 2, label %121
  ]

120:                                              ; preds = %119
  br label %121

121:                                              ; preds = %120, %119, %93, %55, %31
  %122 = call i1 @llvm.coro.end(ptr null, i1 false, token none)
  ret void

123:                                              ; preds = %105, %104, %100
  %124 = call i1 @llvm.coro.end(ptr null, i1 true, token none)
  br i1 %124, label %132, label %125

125:                                              ; preds = %123
  br label %126

126:                                              ; preds = %125, %35
  call void @llvm.lifetime.end.p0(i64 8, ptr %9) #2
  call void @llvm.lifetime.end.p0(i64 8, ptr %8) #2
  call void @llvm.lifetime.end.p0(i64 8, ptr %7) #2
  %127 = call ptr @llvm.coro.free(token %19, ptr %26)
  %128 = icmp ne ptr %127, null
  br i1 %128, label %129, label %131

129:                                              ; preds = %126
  %130 = call i64 @llvm.coro.size.i64()
  call void @_ZdlPvm(ptr noundef %127, i64 noundef %130) #2
  br label %131

131:                                              ; preds = %126, %129
  br label %132

132:                                              ; preds = %131, %123
  %133 = load ptr, ptr %10, align 8
  %134 = load i32, ptr %11, align 4
  %135 = insertvalue { ptr, i32 } poison, ptr %133, 0
  %136 = insertvalue { ptr, i32 } %135, i32 %134, 1
  resume { ptr, i32 } %136

137:                                              ; preds = %96
  %138 = landingpad { ptr, i32 }
          catch ptr null
  %139 = extractvalue { ptr, i32 } %138, 0
  call void @__clang_call_terminate(ptr %139) #19
  unreachable

140:                                              ; preds = %119
  unreachable
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZN4taskILb1EE12promise_typeC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %0) unnamed_addr #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds %"struct.task<true>::promise_type", ptr %3, i32 0, i32 0
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
  %6 = getelementptr inbounds %struct.task.1, ptr %0, i32 0, i32 0
  %7 = call i64 @_ZNSt7__n486116coroutine_handleIN4taskILb1EE12promise_typeEE12from_promiseERS3_(ptr noundef nonnull align 8 dereferenceable(8) %5)
  %8 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.2", ptr %6, i32 0, i32 0
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
define private void @_Z5eagerRll.__await_suspend_wrapper__init(ptr noundef nonnull %0, ptr noundef %1) #9 {
  %3 = alloca ptr, align 8
  %4 = alloca ptr, align 8
  %5 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %6 = alloca %"struct.std::__n4861::coroutine_handle.2", align 8
  store ptr %0, ptr %3, align 8
  store ptr %1, ptr %4, align 8
  %7 = load ptr, ptr %3, align 8
  %8 = load ptr, ptr %4, align 8
  %9 = call i64 @_ZNSt7__n486116coroutine_handleIN4taskILb1EE12promise_typeEE12from_addressEPv(ptr noundef %8) #2
  %10 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.2", ptr %6, i32 0, i32 0
  %11 = inttoptr i64 %9 to ptr
  store ptr %11, ptr %10, align 8
  %12 = call i64 @_ZNKSt7__n486116coroutine_handleIN4taskILb1EE12promise_typeEEcvNS0_IvEEEv(ptr noundef nonnull align 8 dereferenceable(8) %6) #2
  %13 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %5, i32 0, i32 0
  %14 = inttoptr i64 %12 to ptr
  store ptr %14, ptr %13, align 8
  %15 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %5, i32 0, i32 0
  %16 = load ptr, ptr %15, align 8
  %17 = ptrtoint ptr %16 to i64
  call void @_ZNKSt7__n486113suspend_never13await_suspendENS_16coroutine_handleIvEE(ptr noundef nonnull align 1 dereferenceable(1) %7, i64 %17) #2
  ret void
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZNKSt7__n486113suspend_never13await_suspendENS_16coroutine_handleIvEE(ptr noundef nonnull align 1 dereferenceable(1) %0, i64 %1) #6 comdat {
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
define linkonce_odr dso_local i64 @_ZNSt7__n486116coroutine_handleIN4taskILb1EE12promise_typeEE12from_addressEPv(ptr noundef %0) #6 comdat {
  %2 = alloca %"struct.std::__n4861::coroutine_handle.2", align 8
  %3 = alloca ptr, align 8
  store ptr %0, ptr %3, align 8
  call void @_ZNSt7__n486116coroutine_handleIN4taskILb1EE12promise_typeEEC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %2) #2
  %4 = load ptr, ptr %3, align 8
  %5 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.2", ptr %2, i32 0, i32 0
  store ptr %4, ptr %5, align 8
  %6 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.2", ptr %2, i32 0, i32 0
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
  %7 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %2, i32 0, i32 0
  %8 = inttoptr i64 %6 to ptr
  store ptr %8, ptr %7, align 8
  %9 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %2, i32 0, i32 0
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
define private noundef ptr @_Z5eagerRll.__await_suspend_wrapper__await(ptr noundef nonnull %0, ptr noundef %1) #9 {
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
  %11 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.2", ptr %7, i32 0, i32 0
  %12 = inttoptr i64 %10 to ptr
  store ptr %12, ptr %11, align 8
  %13 = call i64 @_ZNKSt7__n486116coroutine_handleIN4taskILb1EE12promise_typeEEcvNS0_IvEEEv(ptr noundef nonnull align 8 dereferenceable(8) %7) #2
  %14 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %6, i32 0, i32 0
  %15 = inttoptr i64 %13 to ptr
  store ptr %15, ptr %14, align 8
  %16 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %6, i32 0, i32 0
  %17 = load ptr, ptr %16, align 8
  %18 = ptrtoint ptr %17 to i64
  %19 = call i64 @_ZN4taskILb0EE13await_suspendENSt7__n486116coroutine_handleIvEE(ptr noundef nonnull align 8 dereferenceable(8) %8, i64 %18)
  %20 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %5, i32 0, i32 0
  %21 = inttoptr i64 %19 to ptr
  store ptr %21, ptr %20, align 8
  %22 = call noundef ptr @_ZNKSt7__n486116coroutine_handleIvE7addressEv(ptr noundef nonnull align 8 dereferenceable(8) %5) #2
  ret ptr %22
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
define private noundef ptr @_Z5eagerRll.__await_suspend_wrapper__final(ptr noundef nonnull %0, ptr noundef %1) #9 {
  %3 = alloca ptr, align 8
  %4 = alloca ptr, align 8
  %5 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %6 = alloca %"struct.std::__n4861::coroutine_handle.2", align 8
  store ptr %0, ptr %3, align 8
  store ptr %1, ptr %4, align 8
  %7 = load ptr, ptr %3, align 8
  %8 = load ptr, ptr %4, align 8
  %9 = call i64 @_ZNSt7__n486116coroutine_handleIN4taskILb1EE12promise_typeEE12from_addressEPv(ptr noundef %8) #2
  %10 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.2", ptr %6, i32 0, i32 0
  %11 = inttoptr i64 %9 to ptr
  store ptr %11, ptr %10, align 8
  %12 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.2", ptr %6, i32 0, i32 0
  %13 = load ptr, ptr %12, align 8
  %14 = ptrtoint ptr %13 to i64
  %15 = call i64 @_ZN4taskILb1EE12promise_type3fin13await_suspendENSt7__n486116coroutine_handleIS1_EE(ptr noundef nonnull align 1 dereferenceable(1) %7, i64 %14) #2
  %16 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %5, i32 0, i32 0
  %17 = inttoptr i64 %15 to ptr
  store ptr %17, ptr %16, align 8
  %18 = call noundef ptr @_ZNKSt7__n486116coroutine_handleIvE7addressEv(ptr noundef nonnull align 8 dereferenceable(8) %5) #2
  ret ptr %18
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local i64 @_ZN4taskILb1EE12promise_type3fin13await_suspendENSt7__n486116coroutine_handleIS1_EE(ptr noundef nonnull align 1 dereferenceable(1) %0, i64 %1) #6 comdat personality ptr @__gxx_personality_v0 {
  %3 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %4 = alloca %"struct.std::__n4861::coroutine_handle.2", align 8
  %5 = alloca ptr, align 8
  %6 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %7 = alloca %"struct.std::__n4861::coroutine_handle.3", align 8
  %8 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.2", ptr %4, i32 0, i32 0
  %9 = inttoptr i64 %1 to ptr
  store ptr %9, ptr %8, align 8
  store ptr %0, ptr %5, align 8
  %10 = load ptr, ptr %5, align 8
  %11 = invoke noundef nonnull align 8 dereferenceable(8) ptr @_ZNKSt7__n486116coroutine_handleIN4taskILb1EE12promise_typeEE7promiseEv(ptr noundef nonnull align 8 dereferenceable(8) %4)
          to label %12 unwind label %27

12:                                               ; preds = %2
  %13 = getelementptr inbounds %"struct.task<true>::promise_type", ptr %11, i32 0, i32 0
  call void @llvm.memcpy.p0.p0.i64(ptr align 8 %6, ptr align 8 %13, i64 8, i1 false)
  %14 = call noundef i1 @_ZNKSt7__n486116coroutine_handleIvEcvbEv(ptr noundef nonnull align 8 dereferenceable(8) %6) #2
  br i1 %14, label %15, label %16

15:                                               ; preds = %12
  call void @llvm.memcpy.p0.p0.i64(ptr align 8 %3, ptr align 8 %6, i64 8, i1 false)
  br label %23

16:                                               ; preds = %12
  %17 = call i64 @_ZNSt7__n486114noop_coroutineEv() #2
  %18 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.3", ptr %7, i32 0, i32 0
  %19 = inttoptr i64 %17 to ptr
  store ptr %19, ptr %18, align 8
  %20 = call i64 @_ZNKSt7__n486116coroutine_handleINS_22noop_coroutine_promiseEEcvNS0_IvEEEv(ptr noundef nonnull align 8 dereferenceable(8) %7) #2
  %21 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %3, i32 0, i32 0
  %22 = inttoptr i64 %20 to ptr
  store ptr %22, ptr %21, align 8
  br label %23

23:                                               ; preds = %16, %15
  %24 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %3, i32 0, i32 0
  %25 = load ptr, ptr %24, align 8
  %26 = ptrtoint ptr %25 to i64
  ret i64 %26

27:                                               ; preds = %2
  %28 = landingpad { ptr, i32 }
          catch ptr null
  %29 = extractvalue { ptr, i32 } %28, 0
  call void @__clang_call_terminate(ptr %29) #19
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
  %6 = getelementptr inbounds %struct.task, ptr %2, i32 0, i32 0
  invoke void @_ZNKSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEE6resumeEv(ptr noundef nonnull align 8 dereferenceable(8) %6)
          to label %7 unwind label %28

7:                                                ; preds = %0
  %8 = load i64, ptr %1, align 8
  %9 = getelementptr inbounds %struct.task, ptr %2, i32 0, i32 0
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
  %19 = getelementptr inbounds %struct.task.1, ptr %5, i32 0, i32 0
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
  %4 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %3, i32 0, i32 0
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
  %4 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %3, i32 0, i32 0
  %5 = load ptr, ptr %4, align 8
  %6 = call i1 @llvm.coro.done(ptr %5)
  ret i1 %6
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local noundef i1 @_ZNKSt7__n486116coroutine_handleIN4taskILb1EE12promise_typeEE4doneEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.2", ptr %3, i32 0, i32 0
  %5 = load ptr, ptr %4, align 8
  %6 = call i1 @llvm.coro.done(ptr %5)
  ret i1 %6
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZN4taskILb1EED2Ev(ptr noundef nonnull align 8 dereferenceable(8) %0) unnamed_addr #6 comdat personality ptr @__gxx_personality_v0 {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds %struct.task.1, ptr %3, i32 0, i32 0
  %5 = call noundef i1 @_ZNKSt7__n486116coroutine_handleIN4taskILb1EE12promise_typeEEcvbEv(ptr noundef nonnull align 8 dereferenceable(8) %4) #2
  br i1 %5, label %6, label %9

6:                                                ; preds = %1
  %7 = getelementptr inbounds %struct.task.1, ptr %3, i32 0, i32 0
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
  %4 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %3, i32 0, i32 0
  store ptr null, ptr %4, align 8
  ret void
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZNSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEEC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %0) unnamed_addr #6 comdat {
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
define linkonce_odr dso_local noundef ptr @_ZNKSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEE7addressEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %3, i32 0, i32 0
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
  %4 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.2", ptr %3, i32 0, i32 0
  store ptr null, ptr %4, align 8
  ret void
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local noundef ptr @_ZNKSt7__n486116coroutine_handleIN4taskILb1EE12promise_typeEE7addressEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.2", ptr %3, i32 0, i32 0
  %5 = load ptr, ptr %4, align 8
  ret ptr %5
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local noundef nonnull align 8 dereferenceable(8) ptr @_ZNKSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEE7promiseEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #6 comdat {
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

; Function Attrs: nocallback nofree nounwind willreturn memory(argmem: readwrite)
declare void @llvm.memcpy.p0.p0.i64(ptr noalias nocapture writeonly, ptr noalias nocapture readonly, i64, i1 immarg) #15

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local noundef i1 @_ZNKSt7__n486116coroutine_handleIvEcvbEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %3, i32 0, i32 0
  %5 = load ptr, ptr %4, align 8
  %6 = icmp ne ptr %5, null
  ret i1 %6
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local i64 @_ZNSt7__n486114noop_coroutineEv() #6 comdat {
  %1 = alloca %"struct.std::__n4861::coroutine_handle.3", align 8
  call void @llvm.memset.p0.i64(ptr align 8 %1, i8 0, i64 8, i1 false)
  call void @_ZNSt7__n486116coroutine_handleINS_22noop_coroutine_promiseEEC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %1) #2
  %2 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.3", ptr %1, i32 0, i32 0
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
  %7 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %2, i32 0, i32 0
  %8 = inttoptr i64 %6 to ptr
  store ptr %8, ptr %7, align 8
  %9 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %2, i32 0, i32 0
  %10 = load ptr, ptr %9, align 8
  %11 = ptrtoint ptr %10 to i64
  ret i64 %11
}

; Function Attrs: nounwind memory(none)
declare ptr @llvm.coro.promise(ptr nocapture, i32, i1) #4

; Function Attrs: nocallback nofree nounwind willreturn memory(argmem: write)
declare void @llvm.memset.p0.i64(ptr nocapture writeonly, i8, i64, i1 immarg) #16

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZNSt7__n486116coroutine_handleINS_22noop_coroutine_promiseEEC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %0) unnamed_addr #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.3", ptr %3, i32 0, i32 0
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
  %4 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.3", ptr %3, i32 0, i32 0
  %5 = load ptr, ptr %4, align 8
  ret ptr %5
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local i64 @_ZNSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEE12from_promiseERS3_(ptr noundef nonnull align 8 dereferenceable(8) %0) #6 comdat {
  %2 = alloca %"struct.std::__n4861::coroutine_handle", align 8
  %3 = alloca ptr, align 8
  store ptr %0, ptr %3, align 8
  call void @_ZNSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEEC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %2) #2
  %4 = load ptr, ptr %3, align 8
  %5 = call ptr @llvm.coro.promise(ptr %4, i32 8, i1 true)
  %6 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %2, i32 0, i32 0
  store ptr %5, ptr %6, align 8
  %7 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %2, i32 0, i32 0
  %8 = load ptr, ptr %7, align 8
  %9 = ptrtoint ptr %8 to i64
  ret i64 %9
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local noundef i1 @_ZNKSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEEcvbEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %3, i32 0, i32 0
  %5 = load ptr, ptr %4, align 8
  %6 = icmp ne ptr %5, null
  ret i1 %6
}

; Function Attrs: mustprogress noinline optnone uwtable
define linkonce_odr dso_local void @_ZNKSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEE7destroyEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #7 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %3, i32 0, i32 0
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
  %5 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.2", ptr %4, i32 0, i32 0
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
  %4 = load ptr, ptr %3, align 8
  %5 = call ptr @llvm.coro.promise(ptr %4, i32 8, i1 true)
  %6 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.2", ptr %2, i32 0, i32 0
  store ptr %5, ptr %6, align 8
  %7 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.2", ptr %2, i32 0, i32 0
  %8 = load ptr, ptr %7, align 8
  %9 = ptrtoint ptr %8 to i64
  ret i64 %9
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local noundef i1 @_ZNKSt7__n486116coroutine_handleIN4taskILb1EE12promise_typeEEcvbEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.2", ptr %3, i32 0, i32 0
  %5 = load ptr, ptr %4, align 8
  %6 = icmp ne ptr %5, null
  ret i1 %6
}

; Function Attrs: mustprogress noinline optnone uwtable
define linkonce_odr dso_local void @_ZNKSt7__n486116coroutine_handleIN4taskILb1EE12promise_typeEE7destroyEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #7 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.2", ptr %3, i32 0, i32 0
  %5 = load ptr, ptr %4, align 8
  call void @llvm.coro.destroy(ptr %5)
  ret void
}

declare void @llvm.coro.resume(ptr)

; Function Attrs: nounwind memory(argmem: readwrite)
declare i1 @llvm.coro.done(ptr nocapture readonly) #17

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
attributes #10 = { noinline noreturn nounwind uwtable "frame-pointer"="non-leaf" "no-trapping-math"="true" "stack-protector-buffer-size"="8" }
attributes #11 = { nobuiltin nounwind "frame-pointer"="non-leaf" "no-trapping-math"="true" "stack-protector-buffer-size"="8" }
attributes #12 = { nounwind memory(argmem: read) }
attributes #13 = { mustprogress noinline norecurse optnone uwtable "frame-pointer"="non-leaf" "no-trapping-math"="true" "stack-protector-buffer-size"="8" }
attributes #14 = { "frame-pointer"="non-leaf" "no-trapping-math"="true" "stack-protector-buffer-size"="8" }
attributes #15 = { nocallback nofree nounwind willreturn memory(argmem: readwrite) }
attributes #16 = { nocallback nofree nounwind willreturn memory(argmem: write) }
attributes #17 = { nounwind memory(argmem: readwrite) }
attributes #18 = { allocsize(0) }
attributes #19 = { noreturn nounwind }

!llvm.linker.options = !{}
!llvm.module.flags = !{!0, !1, !2, !3, !4}
!llvm.ident = !{!5}

!0 = !{i32 1, !"wchar_size", i32 4}
!1 = !{i32 8, !"PIC Level", i32 2}
!2 = !{i32 7, !"PIE Level", i32 2}
!3 = !{i32 7, !"uwtable", i32 2}
!4 = !{i32 7, !"frame-pointer", i32 1}
!5 = !{!"Debian clang version 19.1.7 (3+b1)"}
!6 = distinct !{!6, !7}
!7 = !{!"llvm.loop.mustprogress"}
!8 = distinct !{!8, !7}
