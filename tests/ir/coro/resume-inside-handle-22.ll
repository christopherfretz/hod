; clang++-22 -std=c++20 -O0 -S -emit-llvm -Xclang -disable-llvm-passes
; (aarch64-unknown-linux-gnu; the target triple and datalayout removed) of:
;
; // Design 0005 §3.3, invariant "no frame access after the wrapper returns",
; // for symmetric transfer. The handle form's await_suspend resumes the
; // coroutine itself before returning another coroutine's handle: the
; // coroutine runs to completion (final_suspend is suspend_never), so its
; // frame is freed when the wrapper returns into the suspending activation.
; // The freed block is taken back by a canary allocation of the same size
; // and filled with a pattern; any access to the frame after the wrapper
; // (before the call or the jump to the returned handle's resume function,
; // or in the jump's teardown) shows as a broken canary or garbage. The
; // nested resumption also reads the suspend index, which must have been
; // stored before the wrapper ran. `ramp`: the await is in the ramp (the
; // transfer is a call); `resumed`: in an activation main resumed (the
; // transfer is a tail jump).
; #include <coroutine>
; #include <cstdio>
; #include <cstdlib>
; #include <cstring>
; #include <new>
; static std::size_t last_size;
; static unsigned char* canary;
; void* operator new(std::size_t n) {
;   last_size = n;
;   if (void* p = std::malloc(n)) return p;
;   throw std::bad_alloc();
; }
; void operator delete(void* p) noexcept { std::free(p); }
; void operator delete(void* p, std::size_t) noexcept { std::free(p); }
; static void take_back() {  // called after the frame was freed
;   canary = static_cast<unsigned char*>(std::malloc(last_size));
;   std::memset(canary, 0xc3, last_size);
; }
; static bool canary_ok() {
;   for (std::size_t i = 0; i < last_size; ++i)
;     if (canary[i] != 0xc3) return false;
;   return true;
; }
; template <bool Eager> struct task {
;   struct promise_type {
;     task get_return_object() { return {std::coroutine_handle<promise_type>::from_promise(*this)}; }
;     auto initial_suspend() noexcept {
;       if constexpr (Eager) return std::suspend_never{};
;       else return std::suspend_always{};
;     }
;     std::suspend_never final_suspend() noexcept { return {}; }
;     void return_void() {}
;     void unhandled_exception() {}
;   };
;   std::coroutine_handle<promise_type> h;
; };
; struct idle_task {  // the target of the transfer
;   struct promise_type {
;     idle_task get_return_object() { return {std::coroutine_handle<promise_type>::from_promise(*this)}; }
;     std::suspend_always initial_suspend() noexcept { return {}; }
;     std::suspend_always final_suspend() noexcept { return {}; }
;     void return_void() {}
;     void unhandled_exception() {}
;   };
;   std::coroutine_handle<promise_type> h;
; };
; idle_task idle(const char* name) {
;   std::printf("%s: idle ran\n", name);
;   co_return;
; }
; static std::coroutine_handle<> target;
; struct resume_now {
;   bool await_ready() { return false; }
;   std::coroutine_handle<> await_suspend(std::coroutine_handle<> h) {
;     h.resume();
;     take_back();
;     return target;
;   }
;   void await_resume() {}
; };
; template <bool Eager> task<Eager> run(const char* name) {
;   int a = 1, b = 2;
;   std::printf("%s: ", name);
;   co_await resume_now{};
;   std::printf("body %d\n", a + b);
; }
; int main() {
;   idle_task first = idle("ramp");
;   target = first.h;
;   run<true>("ramp");
;   std::puts(canary_ok() ? "canary ok" : "canary BROKEN");
;   std::free(canary);
;   idle_task second = idle("resumed");
;   target = second.h;
;   auto t = run<false>("resumed");
;   t.h.resume();
;   std::puts(canary_ok() ? "canary ok" : "canary BROKEN");
;   std::free(canary);
;   first.h.destroy();
;   second.h.destroy();
; }
;
;@ covers: spec/ir.md §5.5 intrinsics (symmetric transfer; spec/codegen.md §7.6)
;@ input: pre-opt
;@ dialect: 22
;@ link: c++
;@ unsupported(x86_64): *
;@ stdout: ramp: body 3
;@ stdout: ramp: idle ran
;@ stdout: canary ok
;@ stdout: resumed: body 3
;@ stdout: resumed: idle ran
;@ stdout: canary ok

source_filename = "resume-inside-handle.cpp"

%"struct.std::__n4861::coroutine_handle.0" = type { ptr }
%struct.idle_task = type { %"struct.std::__n4861::coroutine_handle" }
%"struct.std::__n4861::coroutine_handle" = type { ptr }
%"struct.idle_task::promise_type" = type { i8 }
%"struct.std::__n4861::suspend_always" = type { i8 }
%struct.task = type { %"struct.std::__n4861::coroutine_handle.1" }
%"struct.std::__n4861::coroutine_handle.1" = type { ptr }
%struct.task.2 = type { %"struct.std::__n4861::coroutine_handle.3" }
%"struct.std::__n4861::coroutine_handle.3" = type { ptr }
%"struct.task<true>::promise_type" = type { i8 }
%"struct.std::__n4861::suspend_never" = type { i8 }
%struct.resume_now = type { i8 }
%"struct.task<false>::promise_type" = type { i8 }

$_ZNSt9bad_allocC2Ev = comdat any

$_ZN9idle_task12promise_type17get_return_objectEv = comdat any

$_ZN9idle_task12promise_type15initial_suspendEv = comdat any

$_ZNKSt7__n486114suspend_always11await_readyEv = comdat any

$_ZNKSt7__n486114suspend_always13await_suspendENS_16coroutine_handleIvEE = comdat any

$_ZNSt7__n486116coroutine_handleIN9idle_task12promise_typeEE12from_addressEPv = comdat any

$_ZNKSt7__n486116coroutine_handleIN9idle_task12promise_typeEEcvNS0_IvEEEv = comdat any

$_ZNKSt7__n486114suspend_always12await_resumeEv = comdat any

$_ZN9idle_task12promise_type11return_voidEv = comdat any

$_ZN9idle_task12promise_type19unhandled_exceptionEv = comdat any

$__clang_call_terminate = comdat any

$_ZN9idle_task12promise_type13final_suspendEv = comdat any

$_Z3runILb1EE4taskIXT_EEPKc = comdat any

$_Z3runILb0EE4taskIXT_EEPKc = comdat any

$_ZNKSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEE6resumeEv = comdat any

$_ZNKSt7__n486116coroutine_handleIN9idle_task12promise_typeEE7destroyEv = comdat any

$_ZNSt9exceptionC2Ev = comdat any

$_ZNSt7__n486116coroutine_handleIN9idle_task12promise_typeEE12from_promiseERS2_ = comdat any

$_ZNSt7__n486116coroutine_handleIN9idle_task12promise_typeEEC2Ev = comdat any

$_ZNSt7__n486116coroutine_handleIvE12from_addressEPv = comdat any

$_ZNKSt7__n486116coroutine_handleIN9idle_task12promise_typeEE7addressEv = comdat any

$_ZNSt7__n486116coroutine_handleIvEC2Ev = comdat any

$_ZN4taskILb1EE12promise_type17get_return_objectEv = comdat any

$_ZN4taskILb1EE12promise_type15initial_suspendEv = comdat any

$_ZNKSt7__n486113suspend_never11await_readyEv = comdat any

$_ZNKSt7__n486113suspend_never13await_suspendENS_16coroutine_handleIvEE = comdat any

$_ZNSt7__n486116coroutine_handleIN4taskILb1EE12promise_typeEE12from_addressEPv = comdat any

$_ZNKSt7__n486116coroutine_handleIN4taskILb1EE12promise_typeEEcvNS0_IvEEEv = comdat any

$_ZNKSt7__n486113suspend_never12await_resumeEv = comdat any

$_ZN10resume_now11await_readyEv = comdat any

$_ZN10resume_now13await_suspendENSt7__n486116coroutine_handleIvEE = comdat any

$_ZNKSt7__n486116coroutine_handleIvE7addressEv = comdat any

$_ZN10resume_now12await_resumeEv = comdat any

$_ZN4taskILb1EE12promise_type11return_voidEv = comdat any

$_ZN4taskILb1EE12promise_type19unhandled_exceptionEv = comdat any

$_ZN4taskILb1EE12promise_type13final_suspendEv = comdat any

$_ZNSt7__n486116coroutine_handleIN4taskILb1EE12promise_typeEE12from_promiseERS3_ = comdat any

$_ZNSt7__n486116coroutine_handleIN4taskILb1EE12promise_typeEEC2Ev = comdat any

$_ZNKSt7__n486116coroutine_handleIN4taskILb1EE12promise_typeEE7addressEv = comdat any

$_ZNKSt7__n486116coroutine_handleIvE6resumeEv = comdat any

$_ZN4taskILb0EE12promise_type17get_return_objectEv = comdat any

$_ZN4taskILb0EE12promise_type15initial_suspendEv = comdat any

$_ZNSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEE12from_addressEPv = comdat any

$_ZNKSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEEcvNS0_IvEEEv = comdat any

$_ZN4taskILb0EE12promise_type11return_voidEv = comdat any

$_ZN4taskILb0EE12promise_type19unhandled_exceptionEv = comdat any

$_ZN4taskILb0EE12promise_type13final_suspendEv = comdat any

$_ZNSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEE12from_promiseERS3_ = comdat any

$_ZNSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEEC2Ev = comdat any

$_ZNKSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEE7addressEv = comdat any

@_ZL9last_size = internal global i64 0, align 8
@_ZTISt9bad_alloc = external constant ptr
@.str = private unnamed_addr constant [14 x i8] c"%s: idle ran\0A\00", align 1
@.str.1 = private unnamed_addr constant [5 x i8] c"ramp\00", align 1
@_ZL6target = internal global %"struct.std::__n4861::coroutine_handle.0" zeroinitializer, align 8
@.str.2 = private unnamed_addr constant [10 x i8] c"canary ok\00", align 1
@.str.3 = private unnamed_addr constant [14 x i8] c"canary BROKEN\00", align 1
@_ZL6canary = internal global ptr null, align 8
@.str.4 = private unnamed_addr constant [8 x i8] c"resumed\00", align 1
@_ZTVSt9bad_alloc = external unnamed_addr constant { [5 x ptr] }, align 8
@_ZTVSt9exception = external unnamed_addr constant { [5 x ptr] }, align 8
@.str.5 = private unnamed_addr constant [5 x i8] c"%s: \00", align 1
@.str.6 = private unnamed_addr constant [9 x i8] c"body %d\0A\00", align 1

; Function Attrs: mustprogress nobuiltin noinline optnone allocsize(0) uwtable
define dso_local noundef nonnull ptr @_Znwm(i64 noundef %0) #0 {
  %2 = alloca i64, align 8
  %3 = alloca ptr, align 8
  store i64 %0, ptr %2, align 8
  %4 = load i64, ptr %2, align 8
  store i64 %4, ptr @_ZL9last_size, align 8
  %5 = load i64, ptr %2, align 8
  %6 = call noalias ptr @malloc(i64 noundef %5) #19
  store ptr %6, ptr %3, align 8
  %7 = load ptr, ptr %3, align 8
  %8 = icmp ne ptr %7, null
  br i1 %8, label %9, label %11

9:                                                ; preds = %1
  %10 = load ptr, ptr %3, align 8
  ret ptr %10

11:                                               ; preds = %1
  %12 = call ptr @__cxa_allocate_exception(i64 8) #7
  call void @_ZNSt9bad_allocC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %12) #7
  call void @__cxa_throw(ptr %12, ptr @_ZTISt9bad_alloc, ptr @_ZNSt9bad_allocD1Ev) #20
  unreachable
}

; Function Attrs: nounwind allocsize(0)
declare noalias ptr @malloc(i64 noundef) #1

declare ptr @__cxa_allocate_exception(i64)

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZNSt9bad_allocC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %0) unnamed_addr #2 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  call void @_ZNSt9exceptionC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %3) #7
  store ptr getelementptr inbounds inrange(-16, 24) ({ [5 x ptr] }, ptr @_ZTVSt9bad_alloc, i32 0, i32 0, i32 2), ptr %3, align 8
  ret void
}

; Function Attrs: nounwind
declare void @_ZNSt9bad_allocD1Ev(ptr noundef nonnull align 8 dereferenceable(8)) unnamed_addr #3

declare void @__cxa_throw(ptr, ptr, ptr)

; Function Attrs: mustprogress nobuiltin noinline nounwind optnone uwtable
define dso_local void @_ZdlPv(ptr noundef %0) #4 {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  call void @free(ptr noundef %3) #7
  ret void
}

; Function Attrs: nounwind
declare void @free(ptr noundef) #3

; Function Attrs: mustprogress nobuiltin noinline nounwind optnone uwtable
define dso_local void @_ZdlPvm(ptr noundef %0, i64 noundef %1) #4 {
  %3 = alloca ptr, align 8
  %4 = alloca i64, align 8
  store ptr %0, ptr %3, align 8
  store i64 %1, ptr %4, align 8
  %5 = load ptr, ptr %3, align 8
  call void @free(ptr noundef %5) #7
  ret void
}

; Function Attrs: mustprogress noinline optnone presplitcoroutine uwtable
define dso_local i64 @_Z4idlePKc(ptr noundef %0) #5 personality ptr @__gxx_personality_v0 {
  %2 = alloca %struct.idle_task, align 8
  %3 = alloca ptr, align 8, !coro.outside.frame !6
  %4 = alloca ptr, align 8
  %5 = alloca %"struct.idle_task::promise_type", align 1
  %6 = alloca ptr, align 8
  %7 = alloca i32, align 4
  %8 = alloca %"struct.std::__n4861::suspend_always", align 1
  %9 = alloca %"struct.std::__n4861::suspend_always", align 1
  %10 = alloca %"struct.std::__n4861::suspend_always", align 1
  %11 = alloca %"struct.std::__n4861::suspend_always", align 1
  store ptr %0, ptr %3, align 8
  %12 = call token @llvm.coro.id(i32 16, ptr %5, ptr null, ptr null)
  %13 = call i1 @llvm.coro.alloc(token %12)
  br i1 %13, label %14, label %17

14:                                               ; preds = %1
  %15 = call i64 @llvm.coro.size.i64()
  %16 = call noalias noundef nonnull ptr @_Znwm(i64 noundef %15) #21
  br label %17

17:                                               ; preds = %14, %1
  %18 = phi ptr [ null, %1 ], [ %16, %14 ]
  %19 = call ptr @llvm.coro.begin(token %12, ptr %18)
  call void @llvm.lifetime.start.p0(ptr %4) #7
  %20 = load ptr, ptr %3, align 8
  store ptr %20, ptr %4, align 8
  call void @llvm.lifetime.start.p0(ptr %5) #7
  %21 = invoke i64 @_ZN9idle_task12promise_type17get_return_objectEv(ptr noundef nonnull align 1 dereferenceable(1) %5)
          to label %22 unwind label %31

22:                                               ; preds = %17
  %23 = getelementptr inbounds nuw %struct.idle_task, ptr %2, i32 0, i32 0
  %24 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %23, i32 0, i32 0
  %25 = inttoptr i64 %21 to ptr
  store ptr %25, ptr %24, align 8
  call void @llvm.lifetime.start.p0(ptr %8) #7
  call void @_ZN9idle_task12promise_type15initial_suspendEv(ptr noundef nonnull align 1 dereferenceable(1) %5) #7
  %26 = call noundef i1 @_ZNKSt7__n486114suspend_always11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %8) #7
  br i1 %26, label %35, label %27

27:                                               ; preds = %22
  %28 = call token @llvm.coro.save(ptr null)
  call void @llvm.coro.await.suspend.void(ptr %8, ptr %19, ptr @_Z4idlePKc.__await_suspend_wrapper__init) #7
  %29 = call i8 @llvm.coro.suspend(token %28, i1 false)
  switch i8 %29, label %80 [
    i8 0, label %35
    i8 1, label %30
  ]

30:                                               ; preds = %27
  br label %36

31:                                               ; preds = %17
  %32 = landingpad { ptr, i32 }
          cleanup
  %33 = extractvalue { ptr, i32 } %32, 0
  store ptr %33, ptr %6, align 8
  %34 = extractvalue { ptr, i32 } %32, 1
  store i32 %34, ptr %7, align 4
  br label %88

35:                                               ; preds = %27, %22
  call void @_ZNKSt7__n486114suspend_always12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %8) #7
  br label %36

36:                                               ; preds = %35, %30
  %37 = phi i32 [ 0, %35 ], [ 2, %30 ]
  call void @llvm.lifetime.end.p0(ptr %8) #7
  switch i32 %37, label %72 [
    i32 0, label %38
  ]

38:                                               ; preds = %36
  %39 = load ptr, ptr %4, align 8
  %40 = invoke i32 (ptr, ...) @printf(ptr noundef @.str, ptr noundef %39)
          to label %41 unwind label %43

41:                                               ; preds = %38
  invoke void @_ZN9idle_task12promise_type11return_voidEv(ptr noundef nonnull align 1 dereferenceable(1) %5)
          to label %42 unwind label %43

42:                                               ; preds = %41
  br label %53

43:                                               ; preds = %41, %38
  %44 = landingpad { ptr, i32 }
          catch ptr null
  %45 = extractvalue { ptr, i32 } %44, 0
  store ptr %45, ptr %6, align 8
  %46 = extractvalue { ptr, i32 } %44, 1
  store i32 %46, ptr %7, align 4
  br label %47

47:                                               ; preds = %43
  %48 = load ptr, ptr %6, align 8
  %49 = call ptr @__cxa_begin_catch(ptr %48) #7
  invoke void @_ZN9idle_task12promise_type19unhandled_exceptionEv(ptr noundef nonnull align 1 dereferenceable(1) %5)
          to label %50 unwind label %59

50:                                               ; preds = %47
  invoke void @__cxa_end_catch()
          to label %51 unwind label %63

51:                                               ; preds = %50
  br label %52

52:                                               ; preds = %51
  br label %53

53:                                               ; preds = %52, %42
  call void @llvm.lifetime.start.p0(ptr %10) #7
  call void @_ZN9idle_task12promise_type13final_suspendEv(ptr noundef nonnull align 1 dereferenceable(1) %5) #7
  %54 = call noundef i1 @_ZNKSt7__n486114suspend_always11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %10) #7
  br i1 %54, label %68, label %55

55:                                               ; preds = %53
  %56 = call token @llvm.coro.save(ptr null)
  call void @llvm.coro.await.suspend.void(ptr %10, ptr %19, ptr @_Z4idlePKc.__await_suspend_wrapper__final) #7
  %57 = call i8 @llvm.coro.suspend(token %56, i1 true)
  switch i8 %57, label %80 [
    i8 0, label %68
    i8 1, label %58
  ]

58:                                               ; preds = %55
  br label %69

59:                                               ; preds = %47
  %60 = landingpad { ptr, i32 }
          cleanup
  %61 = extractvalue { ptr, i32 } %60, 0
  store ptr %61, ptr %6, align 8
  %62 = extractvalue { ptr, i32 } %60, 1
  store i32 %62, ptr %7, align 4
  invoke void @__cxa_end_catch()
          to label %67 unwind label %99

63:                                               ; preds = %50
  %64 = landingpad { ptr, i32 }
          cleanup
  %65 = extractvalue { ptr, i32 } %64, 0
  store ptr %65, ptr %6, align 8
  %66 = extractvalue { ptr, i32 } %64, 1
  store i32 %66, ptr %7, align 4
  br label %85

67:                                               ; preds = %59
  br label %85

68:                                               ; preds = %55, %53
  call void @_ZNKSt7__n486114suspend_always12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %10) #7
  br label %69

69:                                               ; preds = %68, %58
  %70 = phi i32 [ 0, %68 ], [ 2, %58 ]
  call void @llvm.lifetime.end.p0(ptr %10) #7
  switch i32 %70, label %72 [
    i32 0, label %71
  ]

71:                                               ; preds = %69
  br label %72

72:                                               ; preds = %71, %69, %36
  %73 = phi i32 [ %37, %36 ], [ %70, %69 ], [ 0, %71 ]
  call void @llvm.lifetime.end.p0(ptr %5) #7
  call void @llvm.lifetime.end.p0(ptr %4) #7
  %74 = call ptr @llvm.coro.free(token %12, ptr %19)
  %75 = icmp ne ptr %74, null
  br i1 %75, label %76, label %78

76:                                               ; preds = %72
  %77 = call i64 @llvm.coro.size.i64()
  call void @_ZdlPvm(ptr noundef %74, i64 noundef %77) #7
  br label %78

78:                                               ; preds = %72, %76
  switch i32 %73, label %102 [
    i32 0, label %79
    i32 2, label %80
  ]

79:                                               ; preds = %78
  br label %80

80:                                               ; preds = %79, %78, %55, %27
  call void @llvm.coro.end(ptr null, i1 false, token none)
  %81 = getelementptr inbounds nuw %struct.idle_task, ptr %2, i32 0, i32 0
  %82 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %81, i32 0, i32 0
  %83 = load ptr, ptr %82, align 8
  %84 = ptrtoint ptr %83 to i64
  ret i64 %84

85:                                               ; preds = %67, %63
  call void @llvm.coro.end(ptr null, i1 true, token none)
  %86 = call i1 @llvm.coro.is_in_ramp()
  br i1 %86, label %87, label %94

87:                                               ; preds = %85
  br label %88

88:                                               ; preds = %87, %31
  call void @llvm.lifetime.end.p0(ptr %5) #7
  call void @llvm.lifetime.end.p0(ptr %4) #7
  %89 = call ptr @llvm.coro.free(token %12, ptr %19)
  %90 = icmp ne ptr %89, null
  br i1 %90, label %91, label %93

91:                                               ; preds = %88
  %92 = call i64 @llvm.coro.size.i64()
  call void @_ZdlPvm(ptr noundef %89, i64 noundef %92) #7
  br label %93

93:                                               ; preds = %88, %91
  br label %94

94:                                               ; preds = %93, %85
  %95 = load ptr, ptr %6, align 8
  %96 = load i32, ptr %7, align 4
  %97 = insertvalue { ptr, i32 } poison, ptr %95, 0
  %98 = insertvalue { ptr, i32 } %97, i32 %96, 1
  resume { ptr, i32 } %98

99:                                               ; preds = %59
  %100 = landingpad { ptr, i32 }
          catch ptr null
  %101 = extractvalue { ptr, i32 } %100, 0
  call void @__clang_call_terminate(ptr %101) #22
  unreachable

102:                                              ; preds = %78
  unreachable
}

; Function Attrs: nocallback nofree nosync nounwind willreturn memory(argmem: read)
declare token @llvm.coro.id(i32, ptr readnone, ptr readonly captures(none), ptr) #6

; Function Attrs: nounwind
declare i1 @llvm.coro.alloc(token) #7

; Function Attrs: nounwind memory(none)
declare i64 @llvm.coro.size.i64() #8

; Function Attrs: nounwind
declare ptr @llvm.coro.begin(token, ptr writeonly) #7

; Function Attrs: nocallback nofree nosync nounwind willreturn memory(argmem: readwrite)
declare void @llvm.lifetime.start.p0(ptr captures(none)) #9

; Function Attrs: mustprogress noinline optnone uwtable
define linkonce_odr dso_local i64 @_ZN9idle_task12promise_type17get_return_objectEv(ptr noundef nonnull align 1 dereferenceable(1) %0) #10 comdat {
  %2 = alloca %struct.idle_task, align 8
  %3 = alloca ptr, align 8
  store ptr %0, ptr %3, align 8
  %4 = load ptr, ptr %3, align 8
  %5 = getelementptr inbounds nuw %struct.idle_task, ptr %2, i32 0, i32 0
  %6 = call i64 @_ZNSt7__n486116coroutine_handleIN9idle_task12promise_typeEE12from_promiseERS2_(ptr noundef nonnull align 1 dereferenceable(1) %4)
  %7 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %5, i32 0, i32 0
  %8 = inttoptr i64 %6 to ptr
  store ptr %8, ptr %7, align 8
  %9 = getelementptr inbounds nuw %struct.idle_task, ptr %2, i32 0, i32 0
  %10 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %9, i32 0, i32 0
  %11 = load ptr, ptr %10, align 8
  %12 = ptrtoint ptr %11 to i64
  ret i64 %12
}

declare i32 @__gxx_personality_v0(...)

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZN9idle_task12promise_type15initial_suspendEv(ptr noundef nonnull align 1 dereferenceable(1) %0) #2 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  ret void
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local noundef i1 @_ZNKSt7__n486114suspend_always11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %0) #2 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  ret i1 false
}

; Function Attrs: nomerge nounwind
declare token @llvm.coro.save(ptr) #11

; Function Attrs: alwaysinline mustprogress
define internal void @_Z4idlePKc.__await_suspend_wrapper__init(ptr noundef nonnull %0, ptr noundef %1) #12 {
  %3 = alloca ptr, align 8
  %4 = alloca ptr, align 8
  %5 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %6 = alloca %"struct.std::__n4861::coroutine_handle", align 8
  store ptr %0, ptr %3, align 8
  store ptr %1, ptr %4, align 8
  %7 = load ptr, ptr %3, align 8
  %8 = load ptr, ptr %4, align 8
  %9 = call i64 @_ZNSt7__n486116coroutine_handleIN9idle_task12promise_typeEE12from_addressEPv(ptr noundef %8) #7
  %10 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %6, i32 0, i32 0
  %11 = inttoptr i64 %9 to ptr
  store ptr %11, ptr %10, align 8
  %12 = call i64 @_ZNKSt7__n486116coroutine_handleIN9idle_task12promise_typeEEcvNS0_IvEEEv(ptr noundef nonnull align 8 dereferenceable(8) %6) #7
  %13 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %5, i32 0, i32 0
  %14 = inttoptr i64 %12 to ptr
  store ptr %14, ptr %13, align 8
  %15 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %5, i32 0, i32 0
  %16 = load ptr, ptr %15, align 8
  call void @_ZNKSt7__n486114suspend_always13await_suspendENS_16coroutine_handleIvEE(ptr noundef nonnull align 1 dereferenceable(1) %7, ptr %16) #7
  ret void
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZNKSt7__n486114suspend_always13await_suspendENS_16coroutine_handleIvEE(ptr noundef nonnull align 1 dereferenceable(1) %0, ptr %1) #2 comdat {
  %3 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %4 = alloca ptr, align 8
  %5 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %3, i32 0, i32 0
  store ptr %1, ptr %5, align 8
  store ptr %0, ptr %4, align 8
  %6 = load ptr, ptr %4, align 8
  ret void
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local i64 @_ZNSt7__n486116coroutine_handleIN9idle_task12promise_typeEE12from_addressEPv(ptr noundef %0) #2 comdat {
  %2 = alloca %"struct.std::__n4861::coroutine_handle", align 8
  %3 = alloca ptr, align 8
  store ptr %0, ptr %3, align 8
  call void @_ZNSt7__n486116coroutine_handleIN9idle_task12promise_typeEEC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %2) #7
  %4 = load ptr, ptr %3, align 8
  %5 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %2, i32 0, i32 0
  store ptr %4, ptr %5, align 8
  %6 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %2, i32 0, i32 0
  %7 = load ptr, ptr %6, align 8
  %8 = ptrtoint ptr %7 to i64
  ret i64 %8
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local i64 @_ZNKSt7__n486116coroutine_handleIN9idle_task12promise_typeEEcvNS0_IvEEEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #2 comdat {
  %2 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %3 = alloca ptr, align 8
  store ptr %0, ptr %3, align 8
  %4 = load ptr, ptr %3, align 8
  %5 = call noundef ptr @_ZNKSt7__n486116coroutine_handleIN9idle_task12promise_typeEE7addressEv(ptr noundef nonnull align 8 dereferenceable(8) %4) #7
  %6 = call i64 @_ZNSt7__n486116coroutine_handleIvE12from_addressEPv(ptr noundef %5) #7
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
declare i8 @llvm.coro.suspend(token, i1) #7

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZNKSt7__n486114suspend_always12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %0) #2 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  ret void
}

; Function Attrs: nocallback nofree nosync nounwind willreturn memory(argmem: readwrite)
declare void @llvm.lifetime.end.p0(ptr captures(none)) #9

declare i32 @printf(ptr noundef, ...) #13

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZN9idle_task12promise_type11return_voidEv(ptr noundef nonnull align 1 dereferenceable(1) %0) #2 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  ret void
}

declare ptr @__cxa_begin_catch(ptr)

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZN9idle_task12promise_type19unhandled_exceptionEv(ptr noundef nonnull align 1 dereferenceable(1) %0) #2 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  ret void
}

declare void @__cxa_end_catch()

; Function Attrs: noinline noreturn nounwind uwtable
define linkonce_odr hidden void @__clang_call_terminate(ptr noundef %0) #14 comdat {
  %2 = call ptr @__cxa_begin_catch(ptr %0) #7
  call void @_ZSt9terminatev() #22
  unreachable
}

declare void @_ZSt9terminatev()

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZN9idle_task12promise_type13final_suspendEv(ptr noundef nonnull align 1 dereferenceable(1) %0) #2 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  ret void
}

; Function Attrs: alwaysinline mustprogress
define internal void @_Z4idlePKc.__await_suspend_wrapper__final(ptr noundef nonnull %0, ptr noundef %1) #12 {
  %3 = alloca ptr, align 8
  %4 = alloca ptr, align 8
  %5 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %6 = alloca %"struct.std::__n4861::coroutine_handle", align 8
  store ptr %0, ptr %3, align 8
  store ptr %1, ptr %4, align 8
  %7 = load ptr, ptr %3, align 8
  %8 = load ptr, ptr %4, align 8
  %9 = call i64 @_ZNSt7__n486116coroutine_handleIN9idle_task12promise_typeEE12from_addressEPv(ptr noundef %8) #7
  %10 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %6, i32 0, i32 0
  %11 = inttoptr i64 %9 to ptr
  store ptr %11, ptr %10, align 8
  %12 = call i64 @_ZNKSt7__n486116coroutine_handleIN9idle_task12promise_typeEEcvNS0_IvEEEv(ptr noundef nonnull align 8 dereferenceable(8) %6) #7
  %13 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %5, i32 0, i32 0
  %14 = inttoptr i64 %12 to ptr
  store ptr %14, ptr %13, align 8
  %15 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %5, i32 0, i32 0
  %16 = load ptr, ptr %15, align 8
  call void @_ZNKSt7__n486114suspend_always13await_suspendENS_16coroutine_handleIvEE(ptr noundef nonnull align 1 dereferenceable(1) %7, ptr %16) #7
  ret void
}

; Function Attrs: nounwind
declare void @llvm.coro.end(ptr, i1, token) #7

; Function Attrs: nounwind memory(none)
declare i1 @llvm.coro.is_in_ramp() #8

; Function Attrs: nounwind memory(argmem: read)
declare ptr @llvm.coro.free(token, ptr readonly captures(none)) #15

; Function Attrs: mustprogress noinline norecurse optnone uwtable
define dso_local noundef i32 @main() #16 {
  %1 = alloca %struct.idle_task, align 8
  %2 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %3 = alloca %struct.task, align 8
  %4 = alloca %struct.idle_task, align 8
  %5 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %6 = alloca %struct.task.2, align 8
  %7 = call i64 @_Z4idlePKc(ptr noundef @.str.1)
  %8 = getelementptr inbounds nuw %struct.idle_task, ptr %1, i32 0, i32 0
  %9 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %8, i32 0, i32 0
  %10 = inttoptr i64 %7 to ptr
  store ptr %10, ptr %9, align 8
  %11 = getelementptr inbounds nuw %struct.idle_task, ptr %1, i32 0, i32 0
  %12 = call i64 @_ZNKSt7__n486116coroutine_handleIN9idle_task12promise_typeEEcvNS0_IvEEEv(ptr noundef nonnull align 8 dereferenceable(8) %11) #7
  %13 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %2, i32 0, i32 0
  %14 = inttoptr i64 %12 to ptr
  store ptr %14, ptr %13, align 8
  call void @llvm.memcpy.p0.p0.i64(ptr align 8 @_ZL6target, ptr align 8 %2, i64 8, i1 false)
  %15 = call i64 @_Z3runILb1EE4taskIXT_EEPKc(ptr noundef @.str.1)
  %16 = getelementptr inbounds nuw %struct.task, ptr %3, i32 0, i32 0
  %17 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.1", ptr %16, i32 0, i32 0
  %18 = inttoptr i64 %15 to ptr
  store ptr %18, ptr %17, align 8
  %19 = call noundef i1 @_ZL9canary_okv()
  %20 = zext i1 %19 to i64
  %21 = select i1 %19, ptr @.str.2, ptr @.str.3
  %22 = call i32 @puts(ptr noundef %21)
  %23 = load ptr, ptr @_ZL6canary, align 8
  call void @free(ptr noundef %23) #7
  %24 = call i64 @_Z4idlePKc(ptr noundef @.str.4)
  %25 = getelementptr inbounds nuw %struct.idle_task, ptr %4, i32 0, i32 0
  %26 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %25, i32 0, i32 0
  %27 = inttoptr i64 %24 to ptr
  store ptr %27, ptr %26, align 8
  %28 = getelementptr inbounds nuw %struct.idle_task, ptr %4, i32 0, i32 0
  %29 = call i64 @_ZNKSt7__n486116coroutine_handleIN9idle_task12promise_typeEEcvNS0_IvEEEv(ptr noundef nonnull align 8 dereferenceable(8) %28) #7
  %30 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %5, i32 0, i32 0
  %31 = inttoptr i64 %29 to ptr
  store ptr %31, ptr %30, align 8
  call void @llvm.memcpy.p0.p0.i64(ptr align 8 @_ZL6target, ptr align 8 %5, i64 8, i1 false)
  %32 = call i64 @_Z3runILb0EE4taskIXT_EEPKc(ptr noundef @.str.4)
  %33 = getelementptr inbounds nuw %struct.task.2, ptr %6, i32 0, i32 0
  %34 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.3", ptr %33, i32 0, i32 0
  %35 = inttoptr i64 %32 to ptr
  store ptr %35, ptr %34, align 8
  %36 = getelementptr inbounds nuw %struct.task.2, ptr %6, i32 0, i32 0
  call void @_ZNKSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEE6resumeEv(ptr noundef nonnull align 8 dereferenceable(8) %36)
  %37 = call noundef i1 @_ZL9canary_okv()
  %38 = zext i1 %37 to i64
  %39 = select i1 %37, ptr @.str.2, ptr @.str.3
  %40 = call i32 @puts(ptr noundef %39)
  %41 = load ptr, ptr @_ZL6canary, align 8
  call void @free(ptr noundef %41) #7
  %42 = getelementptr inbounds nuw %struct.idle_task, ptr %1, i32 0, i32 0
  call void @_ZNKSt7__n486116coroutine_handleIN9idle_task12promise_typeEE7destroyEv(ptr noundef nonnull align 8 dereferenceable(8) %42)
  %43 = getelementptr inbounds nuw %struct.idle_task, ptr %4, i32 0, i32 0
  call void @_ZNKSt7__n486116coroutine_handleIN9idle_task12promise_typeEE7destroyEv(ptr noundef nonnull align 8 dereferenceable(8) %43)
  ret i32 0
}

; Function Attrs: nocallback nofree nounwind willreturn memory(argmem: readwrite)
declare void @llvm.memcpy.p0.p0.i64(ptr noalias writeonly captures(none), ptr noalias readonly captures(none), i64, i1 immarg) #17

; Function Attrs: mustprogress noinline optnone presplitcoroutine uwtable
define linkonce_odr dso_local i64 @_Z3runILb1EE4taskIXT_EEPKc(ptr noundef %0) #5 comdat personality ptr @__gxx_personality_v0 {
  %2 = alloca %struct.task, align 8
  %3 = alloca ptr, align 8, !coro.outside.frame !6
  %4 = alloca ptr, align 8
  %5 = alloca %"struct.task<true>::promise_type", align 1
  %6 = alloca ptr, align 8
  %7 = alloca i32, align 4
  %8 = alloca %"struct.std::__n4861::suspend_never", align 1
  %9 = alloca %"struct.std::__n4861::suspend_never", align 1
  %10 = alloca i32, align 4
  %11 = alloca i32, align 4
  %12 = alloca %struct.resume_now, align 1
  %13 = alloca %"struct.std::__n4861::suspend_never", align 1
  %14 = alloca %"struct.std::__n4861::suspend_never", align 1
  store ptr %0, ptr %3, align 8
  %15 = call token @llvm.coro.id(i32 16, ptr %5, ptr null, ptr null)
  %16 = call i1 @llvm.coro.alloc(token %15)
  br i1 %16, label %17, label %20

17:                                               ; preds = %1
  %18 = call i64 @llvm.coro.size.i64()
  %19 = call noalias noundef nonnull ptr @_Znwm(i64 noundef %18) #21
  br label %20

20:                                               ; preds = %17, %1
  %21 = phi ptr [ null, %1 ], [ %19, %17 ]
  %22 = call ptr @llvm.coro.begin(token %15, ptr %21)
  call void @llvm.lifetime.start.p0(ptr %4) #7
  %23 = load ptr, ptr %3, align 8
  store ptr %23, ptr %4, align 8
  call void @llvm.lifetime.start.p0(ptr %5) #7
  %24 = invoke i64 @_ZN4taskILb1EE12promise_type17get_return_objectEv(ptr noundef nonnull align 1 dereferenceable(1) %5)
          to label %25 unwind label %34

25:                                               ; preds = %20
  %26 = getelementptr inbounds nuw %struct.task, ptr %2, i32 0, i32 0
  %27 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.1", ptr %26, i32 0, i32 0
  %28 = inttoptr i64 %24 to ptr
  store ptr %28, ptr %27, align 8
  call void @llvm.lifetime.start.p0(ptr %8) #7
  call void @_ZN4taskILb1EE12promise_type15initial_suspendEv(ptr noundef nonnull align 1 dereferenceable(1) %5) #7
  %29 = call noundef i1 @_ZNKSt7__n486113suspend_never11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %8) #7
  br i1 %29, label %38, label %30

30:                                               ; preds = %25
  %31 = call token @llvm.coro.save(ptr null)
  call void @llvm.coro.await.suspend.void(ptr %8, ptr %22, ptr @_Z3runILb1EE4taskIXT_EEPKc.__await_suspend_wrapper__init) #7
  %32 = call i8 @llvm.coro.suspend(token %31, i1 false)
  switch i8 %32, label %112 [
    i8 0, label %38
    i8 1, label %33
  ]

33:                                               ; preds = %30
  br label %39

34:                                               ; preds = %20
  %35 = landingpad { ptr, i32 }
          cleanup
  %36 = extractvalue { ptr, i32 } %35, 0
  store ptr %36, ptr %6, align 8
  %37 = extractvalue { ptr, i32 } %35, 1
  store i32 %37, ptr %7, align 4
  br label %120

38:                                               ; preds = %30, %25
  call void @_ZNKSt7__n486113suspend_never12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %8) #7
  br label %39

39:                                               ; preds = %38, %33
  %40 = phi i32 [ 0, %38 ], [ 2, %33 ]
  call void @llvm.lifetime.end.p0(ptr %8) #7
  switch i32 %40, label %104 [
    i32 0, label %41
  ]

41:                                               ; preds = %39
  call void @llvm.lifetime.start.p0(ptr %10) #7
  store i32 1, ptr %10, align 4
  call void @llvm.lifetime.start.p0(ptr %11) #7
  store i32 2, ptr %11, align 4
  %42 = load ptr, ptr %4, align 8
  %43 = invoke i32 (ptr, ...) @printf(ptr noundef @.str.5, ptr noundef %42)
          to label %44 unwind label %52

44:                                               ; preds = %41
  call void @llvm.lifetime.start.p0(ptr %12) #7
  %45 = invoke noundef i1 @_ZN10resume_now11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %12)
          to label %46 unwind label %56

46:                                               ; preds = %44
  br i1 %45, label %60, label %47

47:                                               ; preds = %46
  %48 = call token @llvm.coro.save(ptr null)
  invoke void @llvm.coro.await.suspend.handle(ptr %12, ptr %22, ptr @_Z3runILb1EE4taskIXT_EEPKc.__await_suspend_wrapper__await)
          to label %49 unwind label %56

49:                                               ; preds = %47
  %50 = call i8 @llvm.coro.suspend(token %48, i1 false)
  switch i8 %50, label %112 [
    i8 0, label %60
    i8 1, label %51
  ]

51:                                               ; preds = %49
  br label %62

52:                                               ; preds = %64, %41
  %53 = landingpad { ptr, i32 }
          catch ptr null
  %54 = extractvalue { ptr, i32 } %53, 0
  store ptr %54, ptr %6, align 8
  %55 = extractvalue { ptr, i32 } %53, 1
  store i32 %55, ptr %7, align 4
  br label %74

56:                                               ; preds = %60, %47, %44
  %57 = landingpad { ptr, i32 }
          catch ptr null
  %58 = extractvalue { ptr, i32 } %57, 0
  store ptr %58, ptr %6, align 8
  %59 = extractvalue { ptr, i32 } %57, 1
  store i32 %59, ptr %7, align 4
  call void @llvm.lifetime.end.p0(ptr %12) #7
  br label %74

60:                                               ; preds = %49, %46
  invoke void @_ZN10resume_now12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %12)
          to label %61 unwind label %56

61:                                               ; preds = %60
  br label %62

62:                                               ; preds = %61, %51
  %63 = phi i32 [ 0, %61 ], [ 2, %51 ]
  call void @llvm.lifetime.end.p0(ptr %12) #7
  switch i32 %63, label %70 [
    i32 0, label %64
  ]

64:                                               ; preds = %62
  %65 = load i32, ptr %10, align 4
  %66 = load i32, ptr %11, align 4
  %67 = add nsw i32 %65, %66
  %68 = invoke i32 (ptr, ...) @printf(ptr noundef @.str.6, i32 noundef %67)
          to label %69 unwind label %52

69:                                               ; preds = %64
  br label %70

70:                                               ; preds = %69, %62
  %71 = phi i32 [ %63, %62 ], [ 0, %69 ]
  call void @llvm.lifetime.end.p0(ptr %11) #7
  call void @llvm.lifetime.end.p0(ptr %10) #7
  switch i32 %71, label %104 [
    i32 0, label %72
  ]

72:                                               ; preds = %70
  invoke void @_ZN4taskILb1EE12promise_type11return_voidEv(ptr noundef nonnull align 1 dereferenceable(1) %5)
          to label %73 unwind label %75

73:                                               ; preds = %72
  br label %85

74:                                               ; preds = %56, %52
  call void @llvm.lifetime.end.p0(ptr %11) #7
  call void @llvm.lifetime.end.p0(ptr %10) #7
  br label %79

75:                                               ; preds = %72
  %76 = landingpad { ptr, i32 }
          catch ptr null
  %77 = extractvalue { ptr, i32 } %76, 0
  store ptr %77, ptr %6, align 8
  %78 = extractvalue { ptr, i32 } %76, 1
  store i32 %78, ptr %7, align 4
  br label %79

79:                                               ; preds = %75, %74
  %80 = load ptr, ptr %6, align 8
  %81 = call ptr @__cxa_begin_catch(ptr %80) #7
  invoke void @_ZN4taskILb1EE12promise_type19unhandled_exceptionEv(ptr noundef nonnull align 1 dereferenceable(1) %5)
          to label %82 unwind label %91

82:                                               ; preds = %79
  invoke void @__cxa_end_catch()
          to label %83 unwind label %95

83:                                               ; preds = %82
  br label %84

84:                                               ; preds = %83
  br label %85

85:                                               ; preds = %84, %73
  call void @llvm.lifetime.start.p0(ptr %13) #7
  call void @_ZN4taskILb1EE12promise_type13final_suspendEv(ptr noundef nonnull align 1 dereferenceable(1) %5) #7
  %86 = call noundef i1 @_ZNKSt7__n486113suspend_never11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %13) #7
  br i1 %86, label %100, label %87

87:                                               ; preds = %85
  %88 = call token @llvm.coro.save(ptr null)
  call void @llvm.coro.await.suspend.void(ptr %13, ptr %22, ptr @_Z3runILb1EE4taskIXT_EEPKc.__await_suspend_wrapper__final) #7
  %89 = call i8 @llvm.coro.suspend(token %88, i1 true)
  switch i8 %89, label %112 [
    i8 0, label %100
    i8 1, label %90
  ]

90:                                               ; preds = %87
  br label %101

91:                                               ; preds = %79
  %92 = landingpad { ptr, i32 }
          cleanup
  %93 = extractvalue { ptr, i32 } %92, 0
  store ptr %93, ptr %6, align 8
  %94 = extractvalue { ptr, i32 } %92, 1
  store i32 %94, ptr %7, align 4
  invoke void @__cxa_end_catch()
          to label %99 unwind label %131

95:                                               ; preds = %82
  %96 = landingpad { ptr, i32 }
          cleanup
  %97 = extractvalue { ptr, i32 } %96, 0
  store ptr %97, ptr %6, align 8
  %98 = extractvalue { ptr, i32 } %96, 1
  store i32 %98, ptr %7, align 4
  br label %117

99:                                               ; preds = %91
  br label %117

100:                                              ; preds = %87, %85
  call void @_ZNKSt7__n486113suspend_never12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %13) #7
  br label %101

101:                                              ; preds = %100, %90
  %102 = phi i32 [ 0, %100 ], [ 2, %90 ]
  call void @llvm.lifetime.end.p0(ptr %13) #7
  switch i32 %102, label %104 [
    i32 0, label %103
  ]

103:                                              ; preds = %101
  br label %104

104:                                              ; preds = %103, %101, %70, %39
  %105 = phi i32 [ %40, %39 ], [ %71, %70 ], [ %102, %101 ], [ 0, %103 ]
  call void @llvm.lifetime.end.p0(ptr %5) #7
  call void @llvm.lifetime.end.p0(ptr %4) #7
  %106 = call ptr @llvm.coro.free(token %15, ptr %22)
  %107 = icmp ne ptr %106, null
  br i1 %107, label %108, label %110

108:                                              ; preds = %104
  %109 = call i64 @llvm.coro.size.i64()
  call void @_ZdlPvm(ptr noundef %106, i64 noundef %109) #7
  br label %110

110:                                              ; preds = %104, %108
  switch i32 %105, label %134 [
    i32 0, label %111
    i32 2, label %112
  ]

111:                                              ; preds = %110
  br label %112

112:                                              ; preds = %111, %110, %87, %49, %30
  call void @llvm.coro.end(ptr null, i1 false, token none)
  %113 = getelementptr inbounds nuw %struct.task, ptr %2, i32 0, i32 0
  %114 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.1", ptr %113, i32 0, i32 0
  %115 = load ptr, ptr %114, align 8
  %116 = ptrtoint ptr %115 to i64
  ret i64 %116

117:                                              ; preds = %99, %95
  call void @llvm.coro.end(ptr null, i1 true, token none)
  %118 = call i1 @llvm.coro.is_in_ramp()
  br i1 %118, label %119, label %126

119:                                              ; preds = %117
  br label %120

120:                                              ; preds = %119, %34
  call void @llvm.lifetime.end.p0(ptr %5) #7
  call void @llvm.lifetime.end.p0(ptr %4) #7
  %121 = call ptr @llvm.coro.free(token %15, ptr %22)
  %122 = icmp ne ptr %121, null
  br i1 %122, label %123, label %125

123:                                              ; preds = %120
  %124 = call i64 @llvm.coro.size.i64()
  call void @_ZdlPvm(ptr noundef %121, i64 noundef %124) #7
  br label %125

125:                                              ; preds = %120, %123
  br label %126

126:                                              ; preds = %125, %117
  %127 = load ptr, ptr %6, align 8
  %128 = load i32, ptr %7, align 4
  %129 = insertvalue { ptr, i32 } poison, ptr %127, 0
  %130 = insertvalue { ptr, i32 } %129, i32 %128, 1
  resume { ptr, i32 } %130

131:                                              ; preds = %91
  %132 = landingpad { ptr, i32 }
          catch ptr null
  %133 = extractvalue { ptr, i32 } %132, 0
  call void @__clang_call_terminate(ptr %133) #22
  unreachable

134:                                              ; preds = %110
  unreachable
}

declare i32 @puts(ptr noundef) #13

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define internal noundef i1 @_ZL9canary_okv() #2 {
  %1 = alloca i1, align 1
  %2 = alloca i64, align 8
  store i64 0, ptr %2, align 8
  br label %3

3:                                                ; preds = %16, %0
  %4 = load i64, ptr %2, align 8
  %5 = load i64, ptr @_ZL9last_size, align 8
  %6 = icmp ult i64 %4, %5
  br i1 %6, label %7, label %19

7:                                                ; preds = %3
  %8 = load ptr, ptr @_ZL6canary, align 8
  %9 = load i64, ptr %2, align 8
  %10 = getelementptr inbounds nuw i8, ptr %8, i64 %9
  %11 = load i8, ptr %10, align 1
  %12 = zext i8 %11 to i32
  %13 = icmp ne i32 %12, 195
  br i1 %13, label %14, label %15

14:                                               ; preds = %7
  store i1 false, ptr %1, align 1
  br label %20

15:                                               ; preds = %7
  br label %16

16:                                               ; preds = %15
  %17 = load i64, ptr %2, align 8
  %18 = add i64 %17, 1
  store i64 %18, ptr %2, align 8
  br label %3, !llvm.loop !7

19:                                               ; preds = %3
  store i1 true, ptr %1, align 1
  br label %20

20:                                               ; preds = %19, %14
  %21 = load i1, ptr %1, align 1
  ret i1 %21
}

; Function Attrs: mustprogress noinline optnone presplitcoroutine uwtable
define linkonce_odr dso_local i64 @_Z3runILb0EE4taskIXT_EEPKc(ptr noundef %0) #5 comdat personality ptr @__gxx_personality_v0 {
  %2 = alloca %struct.task.2, align 8
  %3 = alloca ptr, align 8, !coro.outside.frame !6
  %4 = alloca ptr, align 8
  %5 = alloca %"struct.task<false>::promise_type", align 1
  %6 = alloca ptr, align 8
  %7 = alloca i32, align 4
  %8 = alloca %"struct.std::__n4861::suspend_always", align 1
  %9 = alloca %"struct.std::__n4861::suspend_always", align 1
  %10 = alloca i32, align 4
  %11 = alloca i32, align 4
  %12 = alloca %struct.resume_now, align 1
  %13 = alloca %"struct.std::__n4861::suspend_never", align 1
  %14 = alloca %"struct.std::__n4861::suspend_never", align 1
  store ptr %0, ptr %3, align 8
  %15 = call token @llvm.coro.id(i32 16, ptr %5, ptr null, ptr null)
  %16 = call i1 @llvm.coro.alloc(token %15)
  br i1 %16, label %17, label %20

17:                                               ; preds = %1
  %18 = call i64 @llvm.coro.size.i64()
  %19 = call noalias noundef nonnull ptr @_Znwm(i64 noundef %18) #21
  br label %20

20:                                               ; preds = %17, %1
  %21 = phi ptr [ null, %1 ], [ %19, %17 ]
  %22 = call ptr @llvm.coro.begin(token %15, ptr %21)
  call void @llvm.lifetime.start.p0(ptr %4) #7
  %23 = load ptr, ptr %3, align 8
  store ptr %23, ptr %4, align 8
  call void @llvm.lifetime.start.p0(ptr %5) #7
  %24 = invoke i64 @_ZN4taskILb0EE12promise_type17get_return_objectEv(ptr noundef nonnull align 1 dereferenceable(1) %5)
          to label %25 unwind label %34

25:                                               ; preds = %20
  %26 = getelementptr inbounds nuw %struct.task.2, ptr %2, i32 0, i32 0
  %27 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.3", ptr %26, i32 0, i32 0
  %28 = inttoptr i64 %24 to ptr
  store ptr %28, ptr %27, align 8
  call void @llvm.lifetime.start.p0(ptr %8) #7
  call void @_ZN4taskILb0EE12promise_type15initial_suspendEv(ptr noundef nonnull align 1 dereferenceable(1) %5) #7
  %29 = call noundef i1 @_ZNKSt7__n486114suspend_always11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %8) #7
  br i1 %29, label %38, label %30

30:                                               ; preds = %25
  %31 = call token @llvm.coro.save(ptr null)
  call void @llvm.coro.await.suspend.void(ptr %8, ptr %22, ptr @_Z3runILb0EE4taskIXT_EEPKc.__await_suspend_wrapper__init) #7
  %32 = call i8 @llvm.coro.suspend(token %31, i1 false)
  switch i8 %32, label %110 [
    i8 0, label %38
    i8 1, label %33
  ]

33:                                               ; preds = %30
  br label %39

34:                                               ; preds = %20
  %35 = landingpad { ptr, i32 }
          cleanup
  %36 = extractvalue { ptr, i32 } %35, 0
  store ptr %36, ptr %6, align 8
  %37 = extractvalue { ptr, i32 } %35, 1
  store i32 %37, ptr %7, align 4
  br label %118

38:                                               ; preds = %30, %25
  call void @_ZNKSt7__n486114suspend_always12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %8) #7
  br label %39

39:                                               ; preds = %38, %33
  %40 = phi i32 [ 0, %38 ], [ 2, %33 ]
  call void @llvm.lifetime.end.p0(ptr %8) #7
  switch i32 %40, label %102 [
    i32 0, label %41
  ]

41:                                               ; preds = %39
  call void @llvm.lifetime.start.p0(ptr %10) #7
  store i32 1, ptr %10, align 4
  call void @llvm.lifetime.start.p0(ptr %11) #7
  store i32 2, ptr %11, align 4
  %42 = load ptr, ptr %4, align 8
  %43 = invoke i32 (ptr, ...) @printf(ptr noundef @.str.5, ptr noundef %42)
          to label %44 unwind label %51

44:                                               ; preds = %41
  call void @llvm.lifetime.start.p0(ptr %12) #7
  %45 = call noundef i1 @_ZN10resume_now11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %12)
  br i1 %45, label %59, label %46

46:                                               ; preds = %44
  %47 = call token @llvm.coro.save(ptr null)
  invoke void @llvm.coro.await.suspend.handle(ptr %12, ptr %22, ptr @_Z3runILb0EE4taskIXT_EEPKc.__await_suspend_wrapper__await)
          to label %48 unwind label %55

48:                                               ; preds = %46
  %49 = call i8 @llvm.coro.suspend(token %47, i1 false)
  switch i8 %49, label %110 [
    i8 0, label %59
    i8 1, label %50
  ]

50:                                               ; preds = %48
  br label %60

51:                                               ; preds = %62, %41
  %52 = landingpad { ptr, i32 }
          catch ptr null
  %53 = extractvalue { ptr, i32 } %52, 0
  store ptr %53, ptr %6, align 8
  %54 = extractvalue { ptr, i32 } %52, 1
  store i32 %54, ptr %7, align 4
  br label %72

55:                                               ; preds = %46
  %56 = landingpad { ptr, i32 }
          catch ptr null
  %57 = extractvalue { ptr, i32 } %56, 0
  store ptr %57, ptr %6, align 8
  %58 = extractvalue { ptr, i32 } %56, 1
  store i32 %58, ptr %7, align 4
  call void @llvm.lifetime.end.p0(ptr %12) #7
  br label %72

59:                                               ; preds = %48, %44
  call void @_ZN10resume_now12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %12)
  br label %60

60:                                               ; preds = %59, %50
  %61 = phi i32 [ 0, %59 ], [ 2, %50 ]
  call void @llvm.lifetime.end.p0(ptr %12) #7
  switch i32 %61, label %68 [
    i32 0, label %62
  ]

62:                                               ; preds = %60
  %63 = load i32, ptr %10, align 4
  %64 = load i32, ptr %11, align 4
  %65 = add nsw i32 %63, %64
  %66 = invoke i32 (ptr, ...) @printf(ptr noundef @.str.6, i32 noundef %65)
          to label %67 unwind label %51

67:                                               ; preds = %62
  br label %68

68:                                               ; preds = %67, %60
  %69 = phi i32 [ %61, %60 ], [ 0, %67 ]
  call void @llvm.lifetime.end.p0(ptr %11) #7
  call void @llvm.lifetime.end.p0(ptr %10) #7
  switch i32 %69, label %102 [
    i32 0, label %70
  ]

70:                                               ; preds = %68
  invoke void @_ZN4taskILb0EE12promise_type11return_voidEv(ptr noundef nonnull align 1 dereferenceable(1) %5)
          to label %71 unwind label %73

71:                                               ; preds = %70
  br label %83

72:                                               ; preds = %55, %51
  call void @llvm.lifetime.end.p0(ptr %11) #7
  call void @llvm.lifetime.end.p0(ptr %10) #7
  br label %77

73:                                               ; preds = %70
  %74 = landingpad { ptr, i32 }
          catch ptr null
  %75 = extractvalue { ptr, i32 } %74, 0
  store ptr %75, ptr %6, align 8
  %76 = extractvalue { ptr, i32 } %74, 1
  store i32 %76, ptr %7, align 4
  br label %77

77:                                               ; preds = %73, %72
  %78 = load ptr, ptr %6, align 8
  %79 = call ptr @__cxa_begin_catch(ptr %78) #7
  invoke void @_ZN4taskILb0EE12promise_type19unhandled_exceptionEv(ptr noundef nonnull align 1 dereferenceable(1) %5)
          to label %80 unwind label %89

80:                                               ; preds = %77
  invoke void @__cxa_end_catch()
          to label %81 unwind label %93

81:                                               ; preds = %80
  br label %82

82:                                               ; preds = %81
  br label %83

83:                                               ; preds = %82, %71
  call void @llvm.lifetime.start.p0(ptr %13) #7
  call void @_ZN4taskILb0EE12promise_type13final_suspendEv(ptr noundef nonnull align 1 dereferenceable(1) %5) #7
  %84 = call noundef i1 @_ZNKSt7__n486113suspend_never11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %13) #7
  br i1 %84, label %98, label %85

85:                                               ; preds = %83
  %86 = call token @llvm.coro.save(ptr null)
  call void @llvm.coro.await.suspend.void(ptr %13, ptr %22, ptr @_Z3runILb0EE4taskIXT_EEPKc.__await_suspend_wrapper__final) #7
  %87 = call i8 @llvm.coro.suspend(token %86, i1 true)
  switch i8 %87, label %110 [
    i8 0, label %98
    i8 1, label %88
  ]

88:                                               ; preds = %85
  br label %99

89:                                               ; preds = %77
  %90 = landingpad { ptr, i32 }
          cleanup
  %91 = extractvalue { ptr, i32 } %90, 0
  store ptr %91, ptr %6, align 8
  %92 = extractvalue { ptr, i32 } %90, 1
  store i32 %92, ptr %7, align 4
  invoke void @__cxa_end_catch()
          to label %97 unwind label %129

93:                                               ; preds = %80
  %94 = landingpad { ptr, i32 }
          cleanup
  %95 = extractvalue { ptr, i32 } %94, 0
  store ptr %95, ptr %6, align 8
  %96 = extractvalue { ptr, i32 } %94, 1
  store i32 %96, ptr %7, align 4
  br label %115

97:                                               ; preds = %89
  br label %115

98:                                               ; preds = %85, %83
  call void @_ZNKSt7__n486113suspend_never12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %13) #7
  br label %99

99:                                               ; preds = %98, %88
  %100 = phi i32 [ 0, %98 ], [ 2, %88 ]
  call void @llvm.lifetime.end.p0(ptr %13) #7
  switch i32 %100, label %102 [
    i32 0, label %101
  ]

101:                                              ; preds = %99
  br label %102

102:                                              ; preds = %101, %99, %68, %39
  %103 = phi i32 [ %40, %39 ], [ %69, %68 ], [ %100, %99 ], [ 0, %101 ]
  call void @llvm.lifetime.end.p0(ptr %5) #7
  call void @llvm.lifetime.end.p0(ptr %4) #7
  %104 = call ptr @llvm.coro.free(token %15, ptr %22)
  %105 = icmp ne ptr %104, null
  br i1 %105, label %106, label %108

106:                                              ; preds = %102
  %107 = call i64 @llvm.coro.size.i64()
  call void @_ZdlPvm(ptr noundef %104, i64 noundef %107) #7
  br label %108

108:                                              ; preds = %102, %106
  switch i32 %103, label %132 [
    i32 0, label %109
    i32 2, label %110
  ]

109:                                              ; preds = %108
  br label %110

110:                                              ; preds = %109, %108, %85, %48, %30
  call void @llvm.coro.end(ptr null, i1 false, token none)
  %111 = getelementptr inbounds nuw %struct.task.2, ptr %2, i32 0, i32 0
  %112 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.3", ptr %111, i32 0, i32 0
  %113 = load ptr, ptr %112, align 8
  %114 = ptrtoint ptr %113 to i64
  ret i64 %114

115:                                              ; preds = %97, %93
  call void @llvm.coro.end(ptr null, i1 true, token none)
  %116 = call i1 @llvm.coro.is_in_ramp()
  br i1 %116, label %117, label %124

117:                                              ; preds = %115
  br label %118

118:                                              ; preds = %117, %34
  call void @llvm.lifetime.end.p0(ptr %5) #7
  call void @llvm.lifetime.end.p0(ptr %4) #7
  %119 = call ptr @llvm.coro.free(token %15, ptr %22)
  %120 = icmp ne ptr %119, null
  br i1 %120, label %121, label %123

121:                                              ; preds = %118
  %122 = call i64 @llvm.coro.size.i64()
  call void @_ZdlPvm(ptr noundef %119, i64 noundef %122) #7
  br label %123

123:                                              ; preds = %118, %121
  br label %124

124:                                              ; preds = %123, %115
  %125 = load ptr, ptr %6, align 8
  %126 = load i32, ptr %7, align 4
  %127 = insertvalue { ptr, i32 } poison, ptr %125, 0
  %128 = insertvalue { ptr, i32 } %127, i32 %126, 1
  resume { ptr, i32 } %128

129:                                              ; preds = %89
  %130 = landingpad { ptr, i32 }
          catch ptr null
  %131 = extractvalue { ptr, i32 } %130, 0
  call void @__clang_call_terminate(ptr %131) #22
  unreachable

132:                                              ; preds = %108
  unreachable
}

; Function Attrs: mustprogress noinline optnone uwtable
define linkonce_odr dso_local void @_ZNKSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEE6resumeEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #10 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.3", ptr %3, i32 0, i32 0
  %5 = load ptr, ptr %4, align 8
  call void @llvm.coro.resume(ptr %5)
  ret void
}

; Function Attrs: mustprogress noinline optnone uwtable
define linkonce_odr dso_local void @_ZNKSt7__n486116coroutine_handleIN9idle_task12promise_typeEE7destroyEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #10 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %3, i32 0, i32 0
  %5 = load ptr, ptr %4, align 8
  call void @llvm.coro.destroy(ptr %5)
  ret void
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZNSt9exceptionC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %0) unnamed_addr #2 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  store ptr getelementptr inbounds inrange(-16, 24) ({ [5 x ptr] }, ptr @_ZTVSt9exception, i32 0, i32 0, i32 2), ptr %3, align 8
  ret void
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local i64 @_ZNSt7__n486116coroutine_handleIN9idle_task12promise_typeEE12from_promiseERS2_(ptr noundef nonnull align 1 dereferenceable(1) %0) #2 comdat {
  %2 = alloca %"struct.std::__n4861::coroutine_handle", align 8
  %3 = alloca ptr, align 8
  store ptr %0, ptr %3, align 8
  call void @_ZNSt7__n486116coroutine_handleIN9idle_task12promise_typeEEC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %2) #7
  %4 = load ptr, ptr %3, align 8, !nonnull !6
  %5 = call ptr @llvm.coro.promise(ptr %4, i32 1, i1 true)
  %6 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %2, i32 0, i32 0
  store ptr %5, ptr %6, align 8
  %7 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %2, i32 0, i32 0
  %8 = load ptr, ptr %7, align 8
  %9 = ptrtoint ptr %8 to i64
  ret i64 %9
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZNSt7__n486116coroutine_handleIN9idle_task12promise_typeEEC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %0) unnamed_addr #2 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %3, i32 0, i32 0
  store ptr null, ptr %4, align 8
  ret void
}

; Function Attrs: nounwind memory(none)
declare ptr @llvm.coro.promise(ptr captures(none), i32, i1) #8

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local i64 @_ZNSt7__n486116coroutine_handleIvE12from_addressEPv(ptr noundef %0) #2 comdat {
  %2 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %3 = alloca ptr, align 8
  store ptr %0, ptr %3, align 8
  call void @_ZNSt7__n486116coroutine_handleIvEC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %2) #7
  %4 = load ptr, ptr %3, align 8
  %5 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %2, i32 0, i32 0
  store ptr %4, ptr %5, align 8
  %6 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %2, i32 0, i32 0
  %7 = load ptr, ptr %6, align 8
  %8 = ptrtoint ptr %7 to i64
  ret i64 %8
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local noundef ptr @_ZNKSt7__n486116coroutine_handleIN9idle_task12promise_typeEE7addressEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #2 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %3, i32 0, i32 0
  %5 = load ptr, ptr %4, align 8
  ret ptr %5
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZNSt7__n486116coroutine_handleIvEC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %0) unnamed_addr #2 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %3, i32 0, i32 0
  store ptr null, ptr %4, align 8
  ret void
}

; Function Attrs: mustprogress noinline optnone uwtable
define linkonce_odr dso_local i64 @_ZN4taskILb1EE12promise_type17get_return_objectEv(ptr noundef nonnull align 1 dereferenceable(1) %0) #10 comdat {
  %2 = alloca %struct.task, align 8
  %3 = alloca ptr, align 8
  store ptr %0, ptr %3, align 8
  %4 = load ptr, ptr %3, align 8
  %5 = getelementptr inbounds nuw %struct.task, ptr %2, i32 0, i32 0
  %6 = call i64 @_ZNSt7__n486116coroutine_handleIN4taskILb1EE12promise_typeEE12from_promiseERS3_(ptr noundef nonnull align 1 dereferenceable(1) %4)
  %7 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.1", ptr %5, i32 0, i32 0
  %8 = inttoptr i64 %6 to ptr
  store ptr %8, ptr %7, align 8
  %9 = getelementptr inbounds nuw %struct.task, ptr %2, i32 0, i32 0
  %10 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.1", ptr %9, i32 0, i32 0
  %11 = load ptr, ptr %10, align 8
  %12 = ptrtoint ptr %11 to i64
  ret i64 %12
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZN4taskILb1EE12promise_type15initial_suspendEv(ptr noundef nonnull align 1 dereferenceable(1) %0) #2 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  ret void
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local noundef i1 @_ZNKSt7__n486113suspend_never11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %0) #2 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  ret i1 true
}

; Function Attrs: alwaysinline mustprogress
define internal void @_Z3runILb1EE4taskIXT_EEPKc.__await_suspend_wrapper__init(ptr noundef nonnull %0, ptr noundef %1) #12 {
  %3 = alloca ptr, align 8
  %4 = alloca ptr, align 8
  %5 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %6 = alloca %"struct.std::__n4861::coroutine_handle.1", align 8
  store ptr %0, ptr %3, align 8
  store ptr %1, ptr %4, align 8
  %7 = load ptr, ptr %3, align 8
  %8 = load ptr, ptr %4, align 8
  %9 = call i64 @_ZNSt7__n486116coroutine_handleIN4taskILb1EE12promise_typeEE12from_addressEPv(ptr noundef %8) #7
  %10 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.1", ptr %6, i32 0, i32 0
  %11 = inttoptr i64 %9 to ptr
  store ptr %11, ptr %10, align 8
  %12 = call i64 @_ZNKSt7__n486116coroutine_handleIN4taskILb1EE12promise_typeEEcvNS0_IvEEEv(ptr noundef nonnull align 8 dereferenceable(8) %6) #7
  %13 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %5, i32 0, i32 0
  %14 = inttoptr i64 %12 to ptr
  store ptr %14, ptr %13, align 8
  %15 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %5, i32 0, i32 0
  %16 = load ptr, ptr %15, align 8
  call void @_ZNKSt7__n486113suspend_never13await_suspendENS_16coroutine_handleIvEE(ptr noundef nonnull align 1 dereferenceable(1) %7, ptr %16) #7
  ret void
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZNKSt7__n486113suspend_never13await_suspendENS_16coroutine_handleIvEE(ptr noundef nonnull align 1 dereferenceable(1) %0, ptr %1) #2 comdat {
  %3 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %4 = alloca ptr, align 8
  %5 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %3, i32 0, i32 0
  store ptr %1, ptr %5, align 8
  store ptr %0, ptr %4, align 8
  %6 = load ptr, ptr %4, align 8
  ret void
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local i64 @_ZNSt7__n486116coroutine_handleIN4taskILb1EE12promise_typeEE12from_addressEPv(ptr noundef %0) #2 comdat {
  %2 = alloca %"struct.std::__n4861::coroutine_handle.1", align 8
  %3 = alloca ptr, align 8
  store ptr %0, ptr %3, align 8
  call void @_ZNSt7__n486116coroutine_handleIN4taskILb1EE12promise_typeEEC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %2) #7
  %4 = load ptr, ptr %3, align 8
  %5 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.1", ptr %2, i32 0, i32 0
  store ptr %4, ptr %5, align 8
  %6 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.1", ptr %2, i32 0, i32 0
  %7 = load ptr, ptr %6, align 8
  %8 = ptrtoint ptr %7 to i64
  ret i64 %8
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local i64 @_ZNKSt7__n486116coroutine_handleIN4taskILb1EE12promise_typeEEcvNS0_IvEEEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #2 comdat {
  %2 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %3 = alloca ptr, align 8
  store ptr %0, ptr %3, align 8
  %4 = load ptr, ptr %3, align 8
  %5 = call noundef ptr @_ZNKSt7__n486116coroutine_handleIN4taskILb1EE12promise_typeEE7addressEv(ptr noundef nonnull align 8 dereferenceable(8) %4) #7
  %6 = call i64 @_ZNSt7__n486116coroutine_handleIvE12from_addressEPv(ptr noundef %5) #7
  %7 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %2, i32 0, i32 0
  %8 = inttoptr i64 %6 to ptr
  store ptr %8, ptr %7, align 8
  %9 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %2, i32 0, i32 0
  %10 = load ptr, ptr %9, align 8
  %11 = ptrtoint ptr %10 to i64
  ret i64 %11
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZNKSt7__n486113suspend_never12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %0) #2 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  ret void
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local noundef i1 @_ZN10resume_now11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %0) #2 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  ret i1 false
}

; Function Attrs: alwaysinline mustprogress
define internal noundef ptr @_Z3runILb1EE4taskIXT_EEPKc.__await_suspend_wrapper__await(ptr noundef nonnull %0, ptr noundef %1) #12 {
  %3 = alloca ptr, align 8
  %4 = alloca ptr, align 8
  %5 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %6 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %7 = alloca %"struct.std::__n4861::coroutine_handle.1", align 8
  store ptr %0, ptr %3, align 8
  store ptr %1, ptr %4, align 8
  %8 = load ptr, ptr %3, align 8
  %9 = load ptr, ptr %4, align 8
  %10 = call i64 @_ZNSt7__n486116coroutine_handleIN4taskILb1EE12promise_typeEE12from_addressEPv(ptr noundef %9) #7
  %11 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.1", ptr %7, i32 0, i32 0
  %12 = inttoptr i64 %10 to ptr
  store ptr %12, ptr %11, align 8
  %13 = call i64 @_ZNKSt7__n486116coroutine_handleIN4taskILb1EE12promise_typeEEcvNS0_IvEEEv(ptr noundef nonnull align 8 dereferenceable(8) %7) #7
  %14 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %6, i32 0, i32 0
  %15 = inttoptr i64 %13 to ptr
  store ptr %15, ptr %14, align 8
  %16 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %6, i32 0, i32 0
  %17 = load ptr, ptr %16, align 8
  %18 = call i64 @_ZN10resume_now13await_suspendENSt7__n486116coroutine_handleIvEE(ptr noundef nonnull align 1 dereferenceable(1) %8, ptr %17)
  %19 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %5, i32 0, i32 0
  %20 = inttoptr i64 %18 to ptr
  store ptr %20, ptr %19, align 8
  %21 = call noundef ptr @_ZNKSt7__n486116coroutine_handleIvE7addressEv(ptr noundef nonnull align 8 dereferenceable(8) %5) #7
  ret ptr %21
}

; Function Attrs: mustprogress noinline optnone uwtable
define linkonce_odr dso_local i64 @_ZN10resume_now13await_suspendENSt7__n486116coroutine_handleIvEE(ptr noundef nonnull align 1 dereferenceable(1) %0, ptr %1) #10 comdat {
  %3 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %4 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %5 = alloca ptr, align 8
  %6 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %4, i32 0, i32 0
  store ptr %1, ptr %6, align 8
  store ptr %0, ptr %5, align 8
  %7 = load ptr, ptr %5, align 8
  call void @_ZNKSt7__n486116coroutine_handleIvE6resumeEv(ptr noundef nonnull align 8 dereferenceable(8) %4)
  call void @_ZL9take_backv()
  call void @llvm.memcpy.p0.p0.i64(ptr align 8 %3, ptr align 8 @_ZL6target, i64 8, i1 false)
  %8 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %3, i32 0, i32 0
  %9 = load ptr, ptr %8, align 8
  %10 = ptrtoint ptr %9 to i64
  ret i64 %10
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local noundef ptr @_ZNKSt7__n486116coroutine_handleIvE7addressEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #2 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %3, i32 0, i32 0
  %5 = load ptr, ptr %4, align 8
  ret ptr %5
}

declare void @llvm.coro.await.suspend.handle(ptr, ptr, ptr)

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZN10resume_now12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %0) #2 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  ret void
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZN4taskILb1EE12promise_type11return_voidEv(ptr noundef nonnull align 1 dereferenceable(1) %0) #2 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  ret void
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZN4taskILb1EE12promise_type19unhandled_exceptionEv(ptr noundef nonnull align 1 dereferenceable(1) %0) #2 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  ret void
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZN4taskILb1EE12promise_type13final_suspendEv(ptr noundef nonnull align 1 dereferenceable(1) %0) #2 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  ret void
}

; Function Attrs: alwaysinline mustprogress
define internal void @_Z3runILb1EE4taskIXT_EEPKc.__await_suspend_wrapper__final(ptr noundef nonnull %0, ptr noundef %1) #12 {
  %3 = alloca ptr, align 8
  %4 = alloca ptr, align 8
  %5 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %6 = alloca %"struct.std::__n4861::coroutine_handle.1", align 8
  store ptr %0, ptr %3, align 8
  store ptr %1, ptr %4, align 8
  %7 = load ptr, ptr %3, align 8
  %8 = load ptr, ptr %4, align 8
  %9 = call i64 @_ZNSt7__n486116coroutine_handleIN4taskILb1EE12promise_typeEE12from_addressEPv(ptr noundef %8) #7
  %10 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.1", ptr %6, i32 0, i32 0
  %11 = inttoptr i64 %9 to ptr
  store ptr %11, ptr %10, align 8
  %12 = call i64 @_ZNKSt7__n486116coroutine_handleIN4taskILb1EE12promise_typeEEcvNS0_IvEEEv(ptr noundef nonnull align 8 dereferenceable(8) %6) #7
  %13 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %5, i32 0, i32 0
  %14 = inttoptr i64 %12 to ptr
  store ptr %14, ptr %13, align 8
  %15 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %5, i32 0, i32 0
  %16 = load ptr, ptr %15, align 8
  call void @_ZNKSt7__n486113suspend_never13await_suspendENS_16coroutine_handleIvEE(ptr noundef nonnull align 1 dereferenceable(1) %7, ptr %16) #7
  ret void
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local i64 @_ZNSt7__n486116coroutine_handleIN4taskILb1EE12promise_typeEE12from_promiseERS3_(ptr noundef nonnull align 1 dereferenceable(1) %0) #2 comdat {
  %2 = alloca %"struct.std::__n4861::coroutine_handle.1", align 8
  %3 = alloca ptr, align 8
  store ptr %0, ptr %3, align 8
  call void @_ZNSt7__n486116coroutine_handleIN4taskILb1EE12promise_typeEEC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %2) #7
  %4 = load ptr, ptr %3, align 8, !nonnull !6
  %5 = call ptr @llvm.coro.promise(ptr %4, i32 1, i1 true)
  %6 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.1", ptr %2, i32 0, i32 0
  store ptr %5, ptr %6, align 8
  %7 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.1", ptr %2, i32 0, i32 0
  %8 = load ptr, ptr %7, align 8
  %9 = ptrtoint ptr %8 to i64
  ret i64 %9
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZNSt7__n486116coroutine_handleIN4taskILb1EE12promise_typeEEC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %0) unnamed_addr #2 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.1", ptr %3, i32 0, i32 0
  store ptr null, ptr %4, align 8
  ret void
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local noundef ptr @_ZNKSt7__n486116coroutine_handleIN4taskILb1EE12promise_typeEE7addressEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #2 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.1", ptr %3, i32 0, i32 0
  %5 = load ptr, ptr %4, align 8
  ret ptr %5
}

; Function Attrs: mustprogress noinline optnone uwtable
define linkonce_odr dso_local void @_ZNKSt7__n486116coroutine_handleIvE6resumeEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #10 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %3, i32 0, i32 0
  %5 = load ptr, ptr %4, align 8
  call void @llvm.coro.resume(ptr %5)
  ret void
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define internal void @_ZL9take_backv() #2 {
  %1 = load i64, ptr @_ZL9last_size, align 8
  %2 = call noalias ptr @malloc(i64 noundef %1) #19
  store ptr %2, ptr @_ZL6canary, align 8
  %3 = load ptr, ptr @_ZL6canary, align 8
  %4 = load i64, ptr @_ZL9last_size, align 8
  call void @llvm.memset.p0.i64(ptr align 1 %3, i8 -61, i64 %4, i1 false)
  ret void
}

declare void @llvm.coro.resume(ptr)

; Function Attrs: nocallback nofree nounwind willreturn memory(argmem: write)
declare void @llvm.memset.p0.i64(ptr writeonly captures(none), i8, i64, i1 immarg) #18

; Function Attrs: mustprogress noinline optnone uwtable
define linkonce_odr dso_local i64 @_ZN4taskILb0EE12promise_type17get_return_objectEv(ptr noundef nonnull align 1 dereferenceable(1) %0) #10 comdat {
  %2 = alloca %struct.task.2, align 8
  %3 = alloca ptr, align 8
  store ptr %0, ptr %3, align 8
  %4 = load ptr, ptr %3, align 8
  %5 = getelementptr inbounds nuw %struct.task.2, ptr %2, i32 0, i32 0
  %6 = call i64 @_ZNSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEE12from_promiseERS3_(ptr noundef nonnull align 1 dereferenceable(1) %4)
  %7 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.3", ptr %5, i32 0, i32 0
  %8 = inttoptr i64 %6 to ptr
  store ptr %8, ptr %7, align 8
  %9 = getelementptr inbounds nuw %struct.task.2, ptr %2, i32 0, i32 0
  %10 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.3", ptr %9, i32 0, i32 0
  %11 = load ptr, ptr %10, align 8
  %12 = ptrtoint ptr %11 to i64
  ret i64 %12
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZN4taskILb0EE12promise_type15initial_suspendEv(ptr noundef nonnull align 1 dereferenceable(1) %0) #2 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  ret void
}

; Function Attrs: alwaysinline mustprogress
define internal void @_Z3runILb0EE4taskIXT_EEPKc.__await_suspend_wrapper__init(ptr noundef nonnull %0, ptr noundef %1) #12 {
  %3 = alloca ptr, align 8
  %4 = alloca ptr, align 8
  %5 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %6 = alloca %"struct.std::__n4861::coroutine_handle.3", align 8
  store ptr %0, ptr %3, align 8
  store ptr %1, ptr %4, align 8
  %7 = load ptr, ptr %3, align 8
  %8 = load ptr, ptr %4, align 8
  %9 = call i64 @_ZNSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEE12from_addressEPv(ptr noundef %8) #7
  %10 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.3", ptr %6, i32 0, i32 0
  %11 = inttoptr i64 %9 to ptr
  store ptr %11, ptr %10, align 8
  %12 = call i64 @_ZNKSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEEcvNS0_IvEEEv(ptr noundef nonnull align 8 dereferenceable(8) %6) #7
  %13 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %5, i32 0, i32 0
  %14 = inttoptr i64 %12 to ptr
  store ptr %14, ptr %13, align 8
  %15 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %5, i32 0, i32 0
  %16 = load ptr, ptr %15, align 8
  call void @_ZNKSt7__n486114suspend_always13await_suspendENS_16coroutine_handleIvEE(ptr noundef nonnull align 1 dereferenceable(1) %7, ptr %16) #7
  ret void
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local i64 @_ZNSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEE12from_addressEPv(ptr noundef %0) #2 comdat {
  %2 = alloca %"struct.std::__n4861::coroutine_handle.3", align 8
  %3 = alloca ptr, align 8
  store ptr %0, ptr %3, align 8
  call void @_ZNSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEEC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %2) #7
  %4 = load ptr, ptr %3, align 8
  %5 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.3", ptr %2, i32 0, i32 0
  store ptr %4, ptr %5, align 8
  %6 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.3", ptr %2, i32 0, i32 0
  %7 = load ptr, ptr %6, align 8
  %8 = ptrtoint ptr %7 to i64
  ret i64 %8
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local i64 @_ZNKSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEEcvNS0_IvEEEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #2 comdat {
  %2 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %3 = alloca ptr, align 8
  store ptr %0, ptr %3, align 8
  %4 = load ptr, ptr %3, align 8
  %5 = call noundef ptr @_ZNKSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEE7addressEv(ptr noundef nonnull align 8 dereferenceable(8) %4) #7
  %6 = call i64 @_ZNSt7__n486116coroutine_handleIvE12from_addressEPv(ptr noundef %5) #7
  %7 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %2, i32 0, i32 0
  %8 = inttoptr i64 %6 to ptr
  store ptr %8, ptr %7, align 8
  %9 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %2, i32 0, i32 0
  %10 = load ptr, ptr %9, align 8
  %11 = ptrtoint ptr %10 to i64
  ret i64 %11
}

; Function Attrs: alwaysinline mustprogress
define internal noundef ptr @_Z3runILb0EE4taskIXT_EEPKc.__await_suspend_wrapper__await(ptr noundef nonnull %0, ptr noundef %1) #12 {
  %3 = alloca ptr, align 8
  %4 = alloca ptr, align 8
  %5 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %6 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %7 = alloca %"struct.std::__n4861::coroutine_handle.3", align 8
  store ptr %0, ptr %3, align 8
  store ptr %1, ptr %4, align 8
  %8 = load ptr, ptr %3, align 8
  %9 = load ptr, ptr %4, align 8
  %10 = call i64 @_ZNSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEE12from_addressEPv(ptr noundef %9) #7
  %11 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.3", ptr %7, i32 0, i32 0
  %12 = inttoptr i64 %10 to ptr
  store ptr %12, ptr %11, align 8
  %13 = call i64 @_ZNKSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEEcvNS0_IvEEEv(ptr noundef nonnull align 8 dereferenceable(8) %7) #7
  %14 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %6, i32 0, i32 0
  %15 = inttoptr i64 %13 to ptr
  store ptr %15, ptr %14, align 8
  %16 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %6, i32 0, i32 0
  %17 = load ptr, ptr %16, align 8
  %18 = call i64 @_ZN10resume_now13await_suspendENSt7__n486116coroutine_handleIvEE(ptr noundef nonnull align 1 dereferenceable(1) %8, ptr %17)
  %19 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %5, i32 0, i32 0
  %20 = inttoptr i64 %18 to ptr
  store ptr %20, ptr %19, align 8
  %21 = call noundef ptr @_ZNKSt7__n486116coroutine_handleIvE7addressEv(ptr noundef nonnull align 8 dereferenceable(8) %5) #7
  ret ptr %21
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZN4taskILb0EE12promise_type11return_voidEv(ptr noundef nonnull align 1 dereferenceable(1) %0) #2 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  ret void
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZN4taskILb0EE12promise_type19unhandled_exceptionEv(ptr noundef nonnull align 1 dereferenceable(1) %0) #2 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  ret void
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZN4taskILb0EE12promise_type13final_suspendEv(ptr noundef nonnull align 1 dereferenceable(1) %0) #2 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  ret void
}

; Function Attrs: alwaysinline mustprogress
define internal void @_Z3runILb0EE4taskIXT_EEPKc.__await_suspend_wrapper__final(ptr noundef nonnull %0, ptr noundef %1) #12 {
  %3 = alloca ptr, align 8
  %4 = alloca ptr, align 8
  %5 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %6 = alloca %"struct.std::__n4861::coroutine_handle.3", align 8
  store ptr %0, ptr %3, align 8
  store ptr %1, ptr %4, align 8
  %7 = load ptr, ptr %3, align 8
  %8 = load ptr, ptr %4, align 8
  %9 = call i64 @_ZNSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEE12from_addressEPv(ptr noundef %8) #7
  %10 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.3", ptr %6, i32 0, i32 0
  %11 = inttoptr i64 %9 to ptr
  store ptr %11, ptr %10, align 8
  %12 = call i64 @_ZNKSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEEcvNS0_IvEEEv(ptr noundef nonnull align 8 dereferenceable(8) %6) #7
  %13 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %5, i32 0, i32 0
  %14 = inttoptr i64 %12 to ptr
  store ptr %14, ptr %13, align 8
  %15 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %5, i32 0, i32 0
  %16 = load ptr, ptr %15, align 8
  call void @_ZNKSt7__n486113suspend_never13await_suspendENS_16coroutine_handleIvEE(ptr noundef nonnull align 1 dereferenceable(1) %7, ptr %16) #7
  ret void
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local i64 @_ZNSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEE12from_promiseERS3_(ptr noundef nonnull align 1 dereferenceable(1) %0) #2 comdat {
  %2 = alloca %"struct.std::__n4861::coroutine_handle.3", align 8
  %3 = alloca ptr, align 8
  store ptr %0, ptr %3, align 8
  call void @_ZNSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEEC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %2) #7
  %4 = load ptr, ptr %3, align 8, !nonnull !6
  %5 = call ptr @llvm.coro.promise(ptr %4, i32 1, i1 true)
  %6 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.3", ptr %2, i32 0, i32 0
  store ptr %5, ptr %6, align 8
  %7 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.3", ptr %2, i32 0, i32 0
  %8 = load ptr, ptr %7, align 8
  %9 = ptrtoint ptr %8 to i64
  ret i64 %9
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZNSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEEC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %0) unnamed_addr #2 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.3", ptr %3, i32 0, i32 0
  store ptr null, ptr %4, align 8
  ret void
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local noundef ptr @_ZNKSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEE7addressEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #2 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.3", ptr %3, i32 0, i32 0
  %5 = load ptr, ptr %4, align 8
  ret ptr %5
}

declare void @llvm.coro.destroy(ptr)

attributes #0 = { mustprogress nobuiltin noinline optnone allocsize(0) uwtable "frame-pointer"="non-leaf-no-reserve" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #1 = { nounwind allocsize(0) "frame-pointer"="non-leaf-no-reserve" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #2 = { mustprogress noinline nounwind optnone uwtable "frame-pointer"="non-leaf-no-reserve" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #3 = { nounwind "frame-pointer"="non-leaf-no-reserve" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #4 = { mustprogress nobuiltin noinline nounwind optnone uwtable "frame-pointer"="non-leaf-no-reserve" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #5 = { mustprogress noinline optnone presplitcoroutine uwtable "frame-pointer"="non-leaf-no-reserve" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #6 = { nocallback nofree nosync nounwind willreturn memory(argmem: read) }
attributes #7 = { nounwind }
attributes #8 = { nounwind memory(none) }
attributes #9 = { nocallback nofree nosync nounwind willreturn memory(argmem: readwrite) }
attributes #10 = { mustprogress noinline optnone uwtable "frame-pointer"="non-leaf-no-reserve" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #11 = { nomerge nounwind }
attributes #12 = { alwaysinline mustprogress "sample-profile-suffix-elision-policy"="selected" }
attributes #13 = { "frame-pointer"="non-leaf-no-reserve" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #14 = { noinline noreturn nounwind uwtable "frame-pointer"="non-leaf-no-reserve" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #15 = { nounwind memory(argmem: read) }
attributes #16 = { mustprogress noinline norecurse optnone uwtable "frame-pointer"="non-leaf-no-reserve" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #17 = { nocallback nofree nounwind willreturn memory(argmem: readwrite) }
attributes #18 = { nocallback nofree nounwind willreturn memory(argmem: write) }
attributes #19 = { nounwind allocsize(0) }
attributes #20 = { noreturn }
attributes #21 = { allocsize(0) }
attributes #22 = { noreturn nounwind }

!llvm.module.flags = !{!0, !1, !2, !3, !4}
!llvm.ident = !{!5}

!0 = !{i32 1, !"wchar_size", i32 4}
!1 = !{i32 8, !"PIC Level", i32 2}
!2 = !{i32 7, !"PIE Level", i32 2}
!3 = !{i32 7, !"uwtable", i32 2}
!4 = !{i32 7, !"frame-pointer", i32 4}
!5 = !{!"Debian clang version 22.1.8 (1~deb13u4)"}
!6 = !{}
!7 = distinct !{!7, !8}
!8 = !{!"llvm.loop.mustprogress"}
