; clang++-19 -std=c++20 -O0 -S -emit-llvm -Xclang -disable-llvm-passes
; (aarch64-unknown-linux-gnu; the target triple, datalayout, target-cpu and
; target-features removed, so that the seed runs on every target) of:
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
;@ dialect: 19
;@ link: c++
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
  %3 = alloca ptr, align 8
  %4 = alloca ptr, align 8
  %5 = alloca %"struct.idle_task::promise_type", align 1
  %6 = alloca ptr, align 8
  %7 = alloca i32, align 4
  %8 = alloca %"struct.std::__n4861::suspend_always", align 1
  %9 = alloca %"struct.std::__n4861::suspend_always", align 1
  %10 = alloca %"struct.std::__n4861::suspend_always", align 1
  %11 = alloca %"struct.std::__n4861::suspend_always", align 1
  store ptr %0, ptr %3, align 8
  %12 = bitcast ptr %5 to ptr
  %13 = call token @llvm.coro.id(i32 16, ptr %12, ptr null, ptr null)
  %14 = call i1 @llvm.coro.alloc(token %13)
  br i1 %14, label %15, label %18

15:                                               ; preds = %1
  %16 = call i64 @llvm.coro.size.i64()
  %17 = call noalias noundef nonnull ptr @_Znwm(i64 noundef %16) #21
  br label %18

18:                                               ; preds = %15, %1
  %19 = phi ptr [ null, %1 ], [ %17, %15 ]
  %20 = call ptr @llvm.coro.begin(token %13, ptr %19)
  call void @llvm.lifetime.start.p0(i64 8, ptr %4) #7
  %21 = load ptr, ptr %3, align 8
  store ptr %21, ptr %4, align 8
  call void @llvm.lifetime.start.p0(i64 1, ptr %5) #7
  %22 = invoke i64 @_ZN9idle_task12promise_type17get_return_objectEv(ptr noundef nonnull align 1 dereferenceable(1) %5)
          to label %23 unwind label %32

23:                                               ; preds = %18
  %24 = getelementptr inbounds %struct.idle_task, ptr %2, i32 0, i32 0
  %25 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %24, i32 0, i32 0
  %26 = inttoptr i64 %22 to ptr
  store ptr %26, ptr %25, align 8
  call void @llvm.lifetime.start.p0(i64 1, ptr %8) #7
  call void @_ZN9idle_task12promise_type15initial_suspendEv(ptr noundef nonnull align 1 dereferenceable(1) %5) #7
  %27 = call noundef i1 @_ZNKSt7__n486114suspend_always11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %8) #7
  br i1 %27, label %36, label %28

28:                                               ; preds = %23
  %29 = call token @llvm.coro.save(ptr null)
  call void @llvm.coro.await.suspend.void(ptr %8, ptr %20, ptr @_Z4idlePKc.__await_suspend_wrapper__init) #7
  %30 = call i8 @llvm.coro.suspend(token %29, i1 false)
  switch i8 %30, label %81 [
    i8 0, label %36
    i8 1, label %31
  ]

31:                                               ; preds = %28
  br label %37

32:                                               ; preds = %18
  %33 = landingpad { ptr, i32 }
          cleanup
  %34 = extractvalue { ptr, i32 } %33, 0
  store ptr %34, ptr %6, align 8
  %35 = extractvalue { ptr, i32 } %33, 1
  store i32 %35, ptr %7, align 4
  br label %90

36:                                               ; preds = %28, %23
  call void @_ZNKSt7__n486114suspend_always12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %8) #7
  br label %37

37:                                               ; preds = %36, %31
  %38 = phi i32 [ 0, %36 ], [ 2, %31 ]
  call void @llvm.lifetime.end.p0(i64 1, ptr %8) #7
  switch i32 %38, label %73 [
    i32 0, label %39
  ]

39:                                               ; preds = %37
  %40 = load ptr, ptr %4, align 8
  %41 = invoke i32 (ptr, ...) @printf(ptr noundef @.str, ptr noundef %40)
          to label %42 unwind label %44

42:                                               ; preds = %39
  invoke void @_ZN9idle_task12promise_type11return_voidEv(ptr noundef nonnull align 1 dereferenceable(1) %5)
          to label %43 unwind label %44

43:                                               ; preds = %42
  br label %54

44:                                               ; preds = %42, %39
  %45 = landingpad { ptr, i32 }
          catch ptr null
  %46 = extractvalue { ptr, i32 } %45, 0
  store ptr %46, ptr %6, align 8
  %47 = extractvalue { ptr, i32 } %45, 1
  store i32 %47, ptr %7, align 4
  br label %48

48:                                               ; preds = %44
  %49 = load ptr, ptr %6, align 8
  %50 = call ptr @__cxa_begin_catch(ptr %49) #7
  invoke void @_ZN9idle_task12promise_type19unhandled_exceptionEv(ptr noundef nonnull align 1 dereferenceable(1) %5)
          to label %51 unwind label %60

51:                                               ; preds = %48
  invoke void @__cxa_end_catch()
          to label %52 unwind label %64

52:                                               ; preds = %51
  br label %53

53:                                               ; preds = %52
  br label %54

54:                                               ; preds = %53, %43
  call void @llvm.lifetime.start.p0(i64 1, ptr %10) #7
  call void @_ZN9idle_task12promise_type13final_suspendEv(ptr noundef nonnull align 1 dereferenceable(1) %5) #7
  %55 = call noundef i1 @_ZNKSt7__n486114suspend_always11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %10) #7
  br i1 %55, label %69, label %56

56:                                               ; preds = %54
  %57 = call token @llvm.coro.save(ptr null)
  call void @llvm.coro.await.suspend.void(ptr %10, ptr %20, ptr @_Z4idlePKc.__await_suspend_wrapper__final) #7
  %58 = call i8 @llvm.coro.suspend(token %57, i1 true)
  switch i8 %58, label %81 [
    i8 0, label %69
    i8 1, label %59
  ]

59:                                               ; preds = %56
  br label %70

60:                                               ; preds = %48
  %61 = landingpad { ptr, i32 }
          cleanup
  %62 = extractvalue { ptr, i32 } %61, 0
  store ptr %62, ptr %6, align 8
  %63 = extractvalue { ptr, i32 } %61, 1
  store i32 %63, ptr %7, align 4
  invoke void @__cxa_end_catch()
          to label %68 unwind label %101

64:                                               ; preds = %51
  %65 = landingpad { ptr, i32 }
          cleanup
  %66 = extractvalue { ptr, i32 } %65, 0
  store ptr %66, ptr %6, align 8
  %67 = extractvalue { ptr, i32 } %65, 1
  store i32 %67, ptr %7, align 4
  br label %87

68:                                               ; preds = %60
  br label %87

69:                                               ; preds = %56, %54
  call void @_ZNKSt7__n486114suspend_always12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %10) #7
  br label %70

70:                                               ; preds = %69, %59
  %71 = phi i32 [ 0, %69 ], [ 2, %59 ]
  call void @llvm.lifetime.end.p0(i64 1, ptr %10) #7
  switch i32 %71, label %73 [
    i32 0, label %72
  ]

72:                                               ; preds = %70
  br label %73

73:                                               ; preds = %72, %70, %37
  %74 = phi i32 [ %38, %37 ], [ %71, %70 ], [ 0, %72 ]
  call void @llvm.lifetime.end.p0(i64 1, ptr %5) #7
  call void @llvm.lifetime.end.p0(i64 8, ptr %4) #7
  %75 = call ptr @llvm.coro.free(token %13, ptr %20)
  %76 = icmp ne ptr %75, null
  br i1 %76, label %77, label %79

77:                                               ; preds = %73
  %78 = call i64 @llvm.coro.size.i64()
  call void @_ZdlPvm(ptr noundef %75, i64 noundef %78) #7
  br label %79

79:                                               ; preds = %73, %77
  switch i32 %74, label %104 [
    i32 0, label %80
    i32 2, label %81
  ]

80:                                               ; preds = %79
  br label %81

81:                                               ; preds = %80, %79, %56, %28
  %82 = call i1 @llvm.coro.end(ptr null, i1 false, token none)
  %83 = getelementptr inbounds %struct.idle_task, ptr %2, i32 0, i32 0
  %84 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %83, i32 0, i32 0
  %85 = load ptr, ptr %84, align 8
  %86 = ptrtoint ptr %85 to i64
  ret i64 %86

87:                                               ; preds = %68, %64
  %88 = call i1 @llvm.coro.end(ptr null, i1 true, token none)
  br i1 %88, label %96, label %89

89:                                               ; preds = %87
  br label %90

90:                                               ; preds = %89, %32
  call void @llvm.lifetime.end.p0(i64 1, ptr %5) #7
  call void @llvm.lifetime.end.p0(i64 8, ptr %4) #7
  %91 = call ptr @llvm.coro.free(token %13, ptr %20)
  %92 = icmp ne ptr %91, null
  br i1 %92, label %93, label %95

93:                                               ; preds = %90
  %94 = call i64 @llvm.coro.size.i64()
  call void @_ZdlPvm(ptr noundef %91, i64 noundef %94) #7
  br label %95

95:                                               ; preds = %90, %93
  br label %96

96:                                               ; preds = %95, %87
  %97 = load ptr, ptr %6, align 8
  %98 = load i32, ptr %7, align 4
  %99 = insertvalue { ptr, i32 } poison, ptr %97, 0
  %100 = insertvalue { ptr, i32 } %99, i32 %98, 1
  resume { ptr, i32 } %100

101:                                              ; preds = %60
  %102 = landingpad { ptr, i32 }
          catch ptr null
  %103 = extractvalue { ptr, i32 } %102, 0
  call void @__clang_call_terminate(ptr %103) #22
  unreachable

104:                                              ; preds = %79
  unreachable
}

; Function Attrs: nocallback nofree nosync nounwind willreturn memory(argmem: read)
declare token @llvm.coro.id(i32, ptr readnone, ptr nocapture readonly, ptr) #6

; Function Attrs: nounwind
declare i1 @llvm.coro.alloc(token) #7

; Function Attrs: nounwind memory(none)
declare i64 @llvm.coro.size.i64() #8

; Function Attrs: nounwind
declare ptr @llvm.coro.begin(token, ptr writeonly) #7

; Function Attrs: nocallback nofree nosync nounwind willreturn memory(argmem: readwrite)
declare void @llvm.lifetime.start.p0(i64 immarg, ptr nocapture) #9

; Function Attrs: mustprogress noinline optnone uwtable
define linkonce_odr dso_local i64 @_ZN9idle_task12promise_type17get_return_objectEv(ptr noundef nonnull align 1 dereferenceable(1) %0) #10 comdat {
  %2 = alloca %struct.idle_task, align 8
  %3 = alloca ptr, align 8
  store ptr %0, ptr %3, align 8
  %4 = load ptr, ptr %3, align 8
  %5 = getelementptr inbounds %struct.idle_task, ptr %2, i32 0, i32 0
  %6 = call i64 @_ZNSt7__n486116coroutine_handleIN9idle_task12promise_typeEE12from_promiseERS2_(ptr noundef nonnull align 1 dereferenceable(1) %4)
  %7 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %5, i32 0, i32 0
  %8 = inttoptr i64 %6 to ptr
  store ptr %8, ptr %7, align 8
  %9 = getelementptr inbounds %struct.idle_task, ptr %2, i32 0, i32 0
  %10 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %9, i32 0, i32 0
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
define private void @_Z4idlePKc.__await_suspend_wrapper__init(ptr noundef nonnull %0, ptr noundef %1) #12 {
  %3 = alloca ptr, align 8
  %4 = alloca ptr, align 8
  %5 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %6 = alloca %"struct.std::__n4861::coroutine_handle", align 8
  store ptr %0, ptr %3, align 8
  store ptr %1, ptr %4, align 8
  %7 = load ptr, ptr %3, align 8
  %8 = load ptr, ptr %4, align 8
  %9 = call i64 @_ZNSt7__n486116coroutine_handleIN9idle_task12promise_typeEE12from_addressEPv(ptr noundef %8) #7
  %10 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %6, i32 0, i32 0
  %11 = inttoptr i64 %9 to ptr
  store ptr %11, ptr %10, align 8
  %12 = call i64 @_ZNKSt7__n486116coroutine_handleIN9idle_task12promise_typeEEcvNS0_IvEEEv(ptr noundef nonnull align 8 dereferenceable(8) %6) #7
  %13 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %5, i32 0, i32 0
  %14 = inttoptr i64 %12 to ptr
  store ptr %14, ptr %13, align 8
  %15 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %5, i32 0, i32 0
  %16 = load ptr, ptr %15, align 8
  %17 = ptrtoint ptr %16 to i64
  call void @_ZNKSt7__n486114suspend_always13await_suspendENS_16coroutine_handleIvEE(ptr noundef nonnull align 1 dereferenceable(1) %7, i64 %17) #7
  ret void
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZNKSt7__n486114suspend_always13await_suspendENS_16coroutine_handleIvEE(ptr noundef nonnull align 1 dereferenceable(1) %0, i64 %1) #2 comdat {
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
define linkonce_odr dso_local i64 @_ZNSt7__n486116coroutine_handleIN9idle_task12promise_typeEE12from_addressEPv(ptr noundef %0) #2 comdat {
  %2 = alloca %"struct.std::__n4861::coroutine_handle", align 8
  %3 = alloca ptr, align 8
  store ptr %0, ptr %3, align 8
  call void @_ZNSt7__n486116coroutine_handleIN9idle_task12promise_typeEEC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %2) #7
  %4 = load ptr, ptr %3, align 8
  %5 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %2, i32 0, i32 0
  store ptr %4, ptr %5, align 8
  %6 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %2, i32 0, i32 0
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
declare i8 @llvm.coro.suspend(token, i1) #7

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZNKSt7__n486114suspend_always12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %0) #2 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  ret void
}

; Function Attrs: nocallback nofree nosync nounwind willreturn memory(argmem: readwrite)
declare void @llvm.lifetime.end.p0(i64 immarg, ptr nocapture) #9

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
define private void @_Z4idlePKc.__await_suspend_wrapper__final(ptr noundef nonnull %0, ptr noundef %1) #12 {
  %3 = alloca ptr, align 8
  %4 = alloca ptr, align 8
  %5 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %6 = alloca %"struct.std::__n4861::coroutine_handle", align 8
  store ptr %0, ptr %3, align 8
  store ptr %1, ptr %4, align 8
  %7 = load ptr, ptr %3, align 8
  %8 = load ptr, ptr %4, align 8
  %9 = call i64 @_ZNSt7__n486116coroutine_handleIN9idle_task12promise_typeEE12from_addressEPv(ptr noundef %8) #7
  %10 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %6, i32 0, i32 0
  %11 = inttoptr i64 %9 to ptr
  store ptr %11, ptr %10, align 8
  %12 = call i64 @_ZNKSt7__n486116coroutine_handleIN9idle_task12promise_typeEEcvNS0_IvEEEv(ptr noundef nonnull align 8 dereferenceable(8) %6) #7
  %13 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %5, i32 0, i32 0
  %14 = inttoptr i64 %12 to ptr
  store ptr %14, ptr %13, align 8
  %15 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %5, i32 0, i32 0
  %16 = load ptr, ptr %15, align 8
  %17 = ptrtoint ptr %16 to i64
  call void @_ZNKSt7__n486114suspend_always13await_suspendENS_16coroutine_handleIvEE(ptr noundef nonnull align 1 dereferenceable(1) %7, i64 %17) #7
  ret void
}

; Function Attrs: nounwind
declare i1 @llvm.coro.end(ptr, i1, token) #7

; Function Attrs: nounwind memory(argmem: read)
declare ptr @llvm.coro.free(token, ptr nocapture readonly) #15

; Function Attrs: mustprogress noinline norecurse optnone uwtable
define dso_local noundef i32 @main() #16 {
  %1 = alloca %struct.idle_task, align 8
  %2 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %3 = alloca %struct.task, align 8
  %4 = alloca %struct.idle_task, align 8
  %5 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %6 = alloca %struct.task.2, align 8
  %7 = call i64 @_Z4idlePKc(ptr noundef @.str.1)
  %8 = getelementptr inbounds %struct.idle_task, ptr %1, i32 0, i32 0
  %9 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %8, i32 0, i32 0
  %10 = inttoptr i64 %7 to ptr
  store ptr %10, ptr %9, align 8
  %11 = getelementptr inbounds %struct.idle_task, ptr %1, i32 0, i32 0
  %12 = call i64 @_ZNKSt7__n486116coroutine_handleIN9idle_task12promise_typeEEcvNS0_IvEEEv(ptr noundef nonnull align 8 dereferenceable(8) %11) #7
  %13 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %2, i32 0, i32 0
  %14 = inttoptr i64 %12 to ptr
  store ptr %14, ptr %13, align 8
  call void @llvm.memcpy.p0.p0.i64(ptr align 8 @_ZL6target, ptr align 8 %2, i64 8, i1 false)
  %15 = call i64 @_Z3runILb1EE4taskIXT_EEPKc(ptr noundef @.str.1)
  %16 = getelementptr inbounds %struct.task, ptr %3, i32 0, i32 0
  %17 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.1", ptr %16, i32 0, i32 0
  %18 = inttoptr i64 %15 to ptr
  store ptr %18, ptr %17, align 8
  %19 = call noundef i1 @_ZL9canary_okv()
  %20 = zext i1 %19 to i64
  %21 = select i1 %19, ptr @.str.2, ptr @.str.3
  %22 = call i32 @puts(ptr noundef %21)
  %23 = load ptr, ptr @_ZL6canary, align 8
  call void @free(ptr noundef %23) #7
  %24 = call i64 @_Z4idlePKc(ptr noundef @.str.4)
  %25 = getelementptr inbounds %struct.idle_task, ptr %4, i32 0, i32 0
  %26 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %25, i32 0, i32 0
  %27 = inttoptr i64 %24 to ptr
  store ptr %27, ptr %26, align 8
  %28 = getelementptr inbounds %struct.idle_task, ptr %4, i32 0, i32 0
  %29 = call i64 @_ZNKSt7__n486116coroutine_handleIN9idle_task12promise_typeEEcvNS0_IvEEEv(ptr noundef nonnull align 8 dereferenceable(8) %28) #7
  %30 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %5, i32 0, i32 0
  %31 = inttoptr i64 %29 to ptr
  store ptr %31, ptr %30, align 8
  call void @llvm.memcpy.p0.p0.i64(ptr align 8 @_ZL6target, ptr align 8 %5, i64 8, i1 false)
  %32 = call i64 @_Z3runILb0EE4taskIXT_EEPKc(ptr noundef @.str.4)
  %33 = getelementptr inbounds %struct.task.2, ptr %6, i32 0, i32 0
  %34 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.3", ptr %33, i32 0, i32 0
  %35 = inttoptr i64 %32 to ptr
  store ptr %35, ptr %34, align 8
  %36 = getelementptr inbounds %struct.task.2, ptr %6, i32 0, i32 0
  call void @_ZNKSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEE6resumeEv(ptr noundef nonnull align 8 dereferenceable(8) %36)
  %37 = call noundef i1 @_ZL9canary_okv()
  %38 = zext i1 %37 to i64
  %39 = select i1 %37, ptr @.str.2, ptr @.str.3
  %40 = call i32 @puts(ptr noundef %39)
  %41 = load ptr, ptr @_ZL6canary, align 8
  call void @free(ptr noundef %41) #7
  %42 = getelementptr inbounds %struct.idle_task, ptr %1, i32 0, i32 0
  call void @_ZNKSt7__n486116coroutine_handleIN9idle_task12promise_typeEE7destroyEv(ptr noundef nonnull align 8 dereferenceable(8) %42)
  %43 = getelementptr inbounds %struct.idle_task, ptr %4, i32 0, i32 0
  call void @_ZNKSt7__n486116coroutine_handleIN9idle_task12promise_typeEE7destroyEv(ptr noundef nonnull align 8 dereferenceable(8) %43)
  ret i32 0
}

; Function Attrs: nocallback nofree nounwind willreturn memory(argmem: readwrite)
declare void @llvm.memcpy.p0.p0.i64(ptr noalias nocapture writeonly, ptr noalias nocapture readonly, i64, i1 immarg) #17

; Function Attrs: mustprogress noinline optnone presplitcoroutine uwtable
define linkonce_odr dso_local i64 @_Z3runILb1EE4taskIXT_EEPKc(ptr noundef %0) #5 comdat personality ptr @__gxx_personality_v0 {
  %2 = alloca %struct.task, align 8
  %3 = alloca ptr, align 8
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
  %15 = bitcast ptr %5 to ptr
  %16 = call token @llvm.coro.id(i32 16, ptr %15, ptr null, ptr null)
  %17 = call i1 @llvm.coro.alloc(token %16)
  br i1 %17, label %18, label %21

18:                                               ; preds = %1
  %19 = call i64 @llvm.coro.size.i64()
  %20 = call noalias noundef nonnull ptr @_Znwm(i64 noundef %19) #21
  br label %21

21:                                               ; preds = %18, %1
  %22 = phi ptr [ null, %1 ], [ %20, %18 ]
  %23 = call ptr @llvm.coro.begin(token %16, ptr %22)
  call void @llvm.lifetime.start.p0(i64 8, ptr %4) #7
  %24 = load ptr, ptr %3, align 8
  store ptr %24, ptr %4, align 8
  call void @llvm.lifetime.start.p0(i64 1, ptr %5) #7
  %25 = invoke i64 @_ZN4taskILb1EE12promise_type17get_return_objectEv(ptr noundef nonnull align 1 dereferenceable(1) %5)
          to label %26 unwind label %35

26:                                               ; preds = %21
  %27 = getelementptr inbounds %struct.task, ptr %2, i32 0, i32 0
  %28 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.1", ptr %27, i32 0, i32 0
  %29 = inttoptr i64 %25 to ptr
  store ptr %29, ptr %28, align 8
  call void @llvm.lifetime.start.p0(i64 1, ptr %8) #7
  call void @_ZN4taskILb1EE12promise_type15initial_suspendEv(ptr noundef nonnull align 1 dereferenceable(1) %5) #7
  %30 = call noundef i1 @_ZNKSt7__n486113suspend_never11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %8) #7
  br i1 %30, label %39, label %31

31:                                               ; preds = %26
  %32 = call token @llvm.coro.save(ptr null)
  call void @llvm.coro.await.suspend.void(ptr %8, ptr %23, ptr @_Z3runILb1EE4taskIXT_EEPKc.__await_suspend_wrapper__init) #7
  %33 = call i8 @llvm.coro.suspend(token %32, i1 false)
  switch i8 %33, label %113 [
    i8 0, label %39
    i8 1, label %34
  ]

34:                                               ; preds = %31
  br label %40

35:                                               ; preds = %21
  %36 = landingpad { ptr, i32 }
          cleanup
  %37 = extractvalue { ptr, i32 } %36, 0
  store ptr %37, ptr %6, align 8
  %38 = extractvalue { ptr, i32 } %36, 1
  store i32 %38, ptr %7, align 4
  br label %122

39:                                               ; preds = %31, %26
  call void @_ZNKSt7__n486113suspend_never12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %8) #7
  br label %40

40:                                               ; preds = %39, %34
  %41 = phi i32 [ 0, %39 ], [ 2, %34 ]
  call void @llvm.lifetime.end.p0(i64 1, ptr %8) #7
  switch i32 %41, label %105 [
    i32 0, label %42
  ]

42:                                               ; preds = %40
  call void @llvm.lifetime.start.p0(i64 4, ptr %10) #7
  store i32 1, ptr %10, align 4
  call void @llvm.lifetime.start.p0(i64 4, ptr %11) #7
  store i32 2, ptr %11, align 4
  %43 = load ptr, ptr %4, align 8
  %44 = invoke i32 (ptr, ...) @printf(ptr noundef @.str.5, ptr noundef %43)
          to label %45 unwind label %53

45:                                               ; preds = %42
  call void @llvm.lifetime.start.p0(i64 1, ptr %12) #7
  %46 = invoke noundef i1 @_ZN10resume_now11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %12)
          to label %47 unwind label %57

47:                                               ; preds = %45
  br i1 %46, label %61, label %48

48:                                               ; preds = %47
  %49 = call token @llvm.coro.save(ptr null)
  invoke void @llvm.coro.await.suspend.handle(ptr %12, ptr %23, ptr @_Z3runILb1EE4taskIXT_EEPKc.__await_suspend_wrapper__await)
          to label %50 unwind label %57

50:                                               ; preds = %48
  %51 = call i8 @llvm.coro.suspend(token %49, i1 false)
  switch i8 %51, label %113 [
    i8 0, label %61
    i8 1, label %52
  ]

52:                                               ; preds = %50
  br label %63

53:                                               ; preds = %65, %42
  %54 = landingpad { ptr, i32 }
          catch ptr null
  %55 = extractvalue { ptr, i32 } %54, 0
  store ptr %55, ptr %6, align 8
  %56 = extractvalue { ptr, i32 } %54, 1
  store i32 %56, ptr %7, align 4
  br label %75

57:                                               ; preds = %61, %48, %45
  %58 = landingpad { ptr, i32 }
          catch ptr null
  %59 = extractvalue { ptr, i32 } %58, 0
  store ptr %59, ptr %6, align 8
  %60 = extractvalue { ptr, i32 } %58, 1
  store i32 %60, ptr %7, align 4
  call void @llvm.lifetime.end.p0(i64 1, ptr %12) #7
  br label %75

61:                                               ; preds = %50, %47
  invoke void @_ZN10resume_now12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %12)
          to label %62 unwind label %57

62:                                               ; preds = %61
  br label %63

63:                                               ; preds = %62, %52
  %64 = phi i32 [ 0, %62 ], [ 2, %52 ]
  call void @llvm.lifetime.end.p0(i64 1, ptr %12) #7
  switch i32 %64, label %71 [
    i32 0, label %65
  ]

65:                                               ; preds = %63
  %66 = load i32, ptr %10, align 4
  %67 = load i32, ptr %11, align 4
  %68 = add nsw i32 %66, %67
  %69 = invoke i32 (ptr, ...) @printf(ptr noundef @.str.6, i32 noundef %68)
          to label %70 unwind label %53

70:                                               ; preds = %65
  br label %71

71:                                               ; preds = %70, %63
  %72 = phi i32 [ %64, %63 ], [ 0, %70 ]
  call void @llvm.lifetime.end.p0(i64 4, ptr %11) #7
  call void @llvm.lifetime.end.p0(i64 4, ptr %10) #7
  switch i32 %72, label %105 [
    i32 0, label %73
  ]

73:                                               ; preds = %71
  invoke void @_ZN4taskILb1EE12promise_type11return_voidEv(ptr noundef nonnull align 1 dereferenceable(1) %5)
          to label %74 unwind label %76

74:                                               ; preds = %73
  br label %86

75:                                               ; preds = %57, %53
  call void @llvm.lifetime.end.p0(i64 4, ptr %11) #7
  call void @llvm.lifetime.end.p0(i64 4, ptr %10) #7
  br label %80

76:                                               ; preds = %73
  %77 = landingpad { ptr, i32 }
          catch ptr null
  %78 = extractvalue { ptr, i32 } %77, 0
  store ptr %78, ptr %6, align 8
  %79 = extractvalue { ptr, i32 } %77, 1
  store i32 %79, ptr %7, align 4
  br label %80

80:                                               ; preds = %76, %75
  %81 = load ptr, ptr %6, align 8
  %82 = call ptr @__cxa_begin_catch(ptr %81) #7
  invoke void @_ZN4taskILb1EE12promise_type19unhandled_exceptionEv(ptr noundef nonnull align 1 dereferenceable(1) %5)
          to label %83 unwind label %92

83:                                               ; preds = %80
  invoke void @__cxa_end_catch()
          to label %84 unwind label %96

84:                                               ; preds = %83
  br label %85

85:                                               ; preds = %84
  br label %86

86:                                               ; preds = %85, %74
  call void @llvm.lifetime.start.p0(i64 1, ptr %13) #7
  call void @_ZN4taskILb1EE12promise_type13final_suspendEv(ptr noundef nonnull align 1 dereferenceable(1) %5) #7
  %87 = call noundef i1 @_ZNKSt7__n486113suspend_never11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %13) #7
  br i1 %87, label %101, label %88

88:                                               ; preds = %86
  %89 = call token @llvm.coro.save(ptr null)
  call void @llvm.coro.await.suspend.void(ptr %13, ptr %23, ptr @_Z3runILb1EE4taskIXT_EEPKc.__await_suspend_wrapper__final) #7
  %90 = call i8 @llvm.coro.suspend(token %89, i1 true)
  switch i8 %90, label %113 [
    i8 0, label %101
    i8 1, label %91
  ]

91:                                               ; preds = %88
  br label %102

92:                                               ; preds = %80
  %93 = landingpad { ptr, i32 }
          cleanup
  %94 = extractvalue { ptr, i32 } %93, 0
  store ptr %94, ptr %6, align 8
  %95 = extractvalue { ptr, i32 } %93, 1
  store i32 %95, ptr %7, align 4
  invoke void @__cxa_end_catch()
          to label %100 unwind label %133

96:                                               ; preds = %83
  %97 = landingpad { ptr, i32 }
          cleanup
  %98 = extractvalue { ptr, i32 } %97, 0
  store ptr %98, ptr %6, align 8
  %99 = extractvalue { ptr, i32 } %97, 1
  store i32 %99, ptr %7, align 4
  br label %119

100:                                              ; preds = %92
  br label %119

101:                                              ; preds = %88, %86
  call void @_ZNKSt7__n486113suspend_never12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %13) #7
  br label %102

102:                                              ; preds = %101, %91
  %103 = phi i32 [ 0, %101 ], [ 2, %91 ]
  call void @llvm.lifetime.end.p0(i64 1, ptr %13) #7
  switch i32 %103, label %105 [
    i32 0, label %104
  ]

104:                                              ; preds = %102
  br label %105

105:                                              ; preds = %104, %102, %71, %40
  %106 = phi i32 [ %41, %40 ], [ %72, %71 ], [ %103, %102 ], [ 0, %104 ]
  call void @llvm.lifetime.end.p0(i64 1, ptr %5) #7
  call void @llvm.lifetime.end.p0(i64 8, ptr %4) #7
  %107 = call ptr @llvm.coro.free(token %16, ptr %23)
  %108 = icmp ne ptr %107, null
  br i1 %108, label %109, label %111

109:                                              ; preds = %105
  %110 = call i64 @llvm.coro.size.i64()
  call void @_ZdlPvm(ptr noundef %107, i64 noundef %110) #7
  br label %111

111:                                              ; preds = %105, %109
  switch i32 %106, label %136 [
    i32 0, label %112
    i32 2, label %113
  ]

112:                                              ; preds = %111
  br label %113

113:                                              ; preds = %112, %111, %88, %50, %31
  %114 = call i1 @llvm.coro.end(ptr null, i1 false, token none)
  %115 = getelementptr inbounds %struct.task, ptr %2, i32 0, i32 0
  %116 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.1", ptr %115, i32 0, i32 0
  %117 = load ptr, ptr %116, align 8
  %118 = ptrtoint ptr %117 to i64
  ret i64 %118

119:                                              ; preds = %100, %96
  %120 = call i1 @llvm.coro.end(ptr null, i1 true, token none)
  br i1 %120, label %128, label %121

121:                                              ; preds = %119
  br label %122

122:                                              ; preds = %121, %35
  call void @llvm.lifetime.end.p0(i64 1, ptr %5) #7
  call void @llvm.lifetime.end.p0(i64 8, ptr %4) #7
  %123 = call ptr @llvm.coro.free(token %16, ptr %23)
  %124 = icmp ne ptr %123, null
  br i1 %124, label %125, label %127

125:                                              ; preds = %122
  %126 = call i64 @llvm.coro.size.i64()
  call void @_ZdlPvm(ptr noundef %123, i64 noundef %126) #7
  br label %127

127:                                              ; preds = %122, %125
  br label %128

128:                                              ; preds = %127, %119
  %129 = load ptr, ptr %6, align 8
  %130 = load i32, ptr %7, align 4
  %131 = insertvalue { ptr, i32 } poison, ptr %129, 0
  %132 = insertvalue { ptr, i32 } %131, i32 %130, 1
  resume { ptr, i32 } %132

133:                                              ; preds = %92
  %134 = landingpad { ptr, i32 }
          catch ptr null
  %135 = extractvalue { ptr, i32 } %134, 0
  call void @__clang_call_terminate(ptr %135) #22
  unreachable

136:                                              ; preds = %111
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
  %10 = getelementptr inbounds i8, ptr %8, i64 %9
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
  br label %3, !llvm.loop !6

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
  %3 = alloca ptr, align 8
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
  %15 = bitcast ptr %5 to ptr
  %16 = call token @llvm.coro.id(i32 16, ptr %15, ptr null, ptr null)
  %17 = call i1 @llvm.coro.alloc(token %16)
  br i1 %17, label %18, label %21

18:                                               ; preds = %1
  %19 = call i64 @llvm.coro.size.i64()
  %20 = call noalias noundef nonnull ptr @_Znwm(i64 noundef %19) #21
  br label %21

21:                                               ; preds = %18, %1
  %22 = phi ptr [ null, %1 ], [ %20, %18 ]
  %23 = call ptr @llvm.coro.begin(token %16, ptr %22)
  call void @llvm.lifetime.start.p0(i64 8, ptr %4) #7
  %24 = load ptr, ptr %3, align 8
  store ptr %24, ptr %4, align 8
  call void @llvm.lifetime.start.p0(i64 1, ptr %5) #7
  %25 = invoke i64 @_ZN4taskILb0EE12promise_type17get_return_objectEv(ptr noundef nonnull align 1 dereferenceable(1) %5)
          to label %26 unwind label %35

26:                                               ; preds = %21
  %27 = getelementptr inbounds %struct.task.2, ptr %2, i32 0, i32 0
  %28 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.3", ptr %27, i32 0, i32 0
  %29 = inttoptr i64 %25 to ptr
  store ptr %29, ptr %28, align 8
  call void @llvm.lifetime.start.p0(i64 1, ptr %8) #7
  call void @_ZN4taskILb0EE12promise_type15initial_suspendEv(ptr noundef nonnull align 1 dereferenceable(1) %5) #7
  %30 = call noundef i1 @_ZNKSt7__n486114suspend_always11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %8) #7
  br i1 %30, label %39, label %31

31:                                               ; preds = %26
  %32 = call token @llvm.coro.save(ptr null)
  call void @llvm.coro.await.suspend.void(ptr %8, ptr %23, ptr @_Z3runILb0EE4taskIXT_EEPKc.__await_suspend_wrapper__init) #7
  %33 = call i8 @llvm.coro.suspend(token %32, i1 false)
  switch i8 %33, label %111 [
    i8 0, label %39
    i8 1, label %34
  ]

34:                                               ; preds = %31
  br label %40

35:                                               ; preds = %21
  %36 = landingpad { ptr, i32 }
          cleanup
  %37 = extractvalue { ptr, i32 } %36, 0
  store ptr %37, ptr %6, align 8
  %38 = extractvalue { ptr, i32 } %36, 1
  store i32 %38, ptr %7, align 4
  br label %120

39:                                               ; preds = %31, %26
  call void @_ZNKSt7__n486114suspend_always12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %8) #7
  br label %40

40:                                               ; preds = %39, %34
  %41 = phi i32 [ 0, %39 ], [ 2, %34 ]
  call void @llvm.lifetime.end.p0(i64 1, ptr %8) #7
  switch i32 %41, label %103 [
    i32 0, label %42
  ]

42:                                               ; preds = %40
  call void @llvm.lifetime.start.p0(i64 4, ptr %10) #7
  store i32 1, ptr %10, align 4
  call void @llvm.lifetime.start.p0(i64 4, ptr %11) #7
  store i32 2, ptr %11, align 4
  %43 = load ptr, ptr %4, align 8
  %44 = invoke i32 (ptr, ...) @printf(ptr noundef @.str.5, ptr noundef %43)
          to label %45 unwind label %52

45:                                               ; preds = %42
  call void @llvm.lifetime.start.p0(i64 1, ptr %12) #7
  %46 = call noundef i1 @_ZN10resume_now11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %12)
  br i1 %46, label %60, label %47

47:                                               ; preds = %45
  %48 = call token @llvm.coro.save(ptr null)
  invoke void @llvm.coro.await.suspend.handle(ptr %12, ptr %23, ptr @_Z3runILb0EE4taskIXT_EEPKc.__await_suspend_wrapper__await)
          to label %49 unwind label %56

49:                                               ; preds = %47
  %50 = call i8 @llvm.coro.suspend(token %48, i1 false)
  switch i8 %50, label %111 [
    i8 0, label %60
    i8 1, label %51
  ]

51:                                               ; preds = %49
  br label %61

52:                                               ; preds = %63, %42
  %53 = landingpad { ptr, i32 }
          catch ptr null
  %54 = extractvalue { ptr, i32 } %53, 0
  store ptr %54, ptr %6, align 8
  %55 = extractvalue { ptr, i32 } %53, 1
  store i32 %55, ptr %7, align 4
  br label %73

56:                                               ; preds = %47
  %57 = landingpad { ptr, i32 }
          catch ptr null
  %58 = extractvalue { ptr, i32 } %57, 0
  store ptr %58, ptr %6, align 8
  %59 = extractvalue { ptr, i32 } %57, 1
  store i32 %59, ptr %7, align 4
  call void @llvm.lifetime.end.p0(i64 1, ptr %12) #7
  br label %73

60:                                               ; preds = %49, %45
  call void @_ZN10resume_now12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %12)
  br label %61

61:                                               ; preds = %60, %51
  %62 = phi i32 [ 0, %60 ], [ 2, %51 ]
  call void @llvm.lifetime.end.p0(i64 1, ptr %12) #7
  switch i32 %62, label %69 [
    i32 0, label %63
  ]

63:                                               ; preds = %61
  %64 = load i32, ptr %10, align 4
  %65 = load i32, ptr %11, align 4
  %66 = add nsw i32 %64, %65
  %67 = invoke i32 (ptr, ...) @printf(ptr noundef @.str.6, i32 noundef %66)
          to label %68 unwind label %52

68:                                               ; preds = %63
  br label %69

69:                                               ; preds = %68, %61
  %70 = phi i32 [ %62, %61 ], [ 0, %68 ]
  call void @llvm.lifetime.end.p0(i64 4, ptr %11) #7
  call void @llvm.lifetime.end.p0(i64 4, ptr %10) #7
  switch i32 %70, label %103 [
    i32 0, label %71
  ]

71:                                               ; preds = %69
  invoke void @_ZN4taskILb0EE12promise_type11return_voidEv(ptr noundef nonnull align 1 dereferenceable(1) %5)
          to label %72 unwind label %74

72:                                               ; preds = %71
  br label %84

73:                                               ; preds = %56, %52
  call void @llvm.lifetime.end.p0(i64 4, ptr %11) #7
  call void @llvm.lifetime.end.p0(i64 4, ptr %10) #7
  br label %78

74:                                               ; preds = %71
  %75 = landingpad { ptr, i32 }
          catch ptr null
  %76 = extractvalue { ptr, i32 } %75, 0
  store ptr %76, ptr %6, align 8
  %77 = extractvalue { ptr, i32 } %75, 1
  store i32 %77, ptr %7, align 4
  br label %78

78:                                               ; preds = %74, %73
  %79 = load ptr, ptr %6, align 8
  %80 = call ptr @__cxa_begin_catch(ptr %79) #7
  invoke void @_ZN4taskILb0EE12promise_type19unhandled_exceptionEv(ptr noundef nonnull align 1 dereferenceable(1) %5)
          to label %81 unwind label %90

81:                                               ; preds = %78
  invoke void @__cxa_end_catch()
          to label %82 unwind label %94

82:                                               ; preds = %81
  br label %83

83:                                               ; preds = %82
  br label %84

84:                                               ; preds = %83, %72
  call void @llvm.lifetime.start.p0(i64 1, ptr %13) #7
  call void @_ZN4taskILb0EE12promise_type13final_suspendEv(ptr noundef nonnull align 1 dereferenceable(1) %5) #7
  %85 = call noundef i1 @_ZNKSt7__n486113suspend_never11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %13) #7
  br i1 %85, label %99, label %86

86:                                               ; preds = %84
  %87 = call token @llvm.coro.save(ptr null)
  call void @llvm.coro.await.suspend.void(ptr %13, ptr %23, ptr @_Z3runILb0EE4taskIXT_EEPKc.__await_suspend_wrapper__final) #7
  %88 = call i8 @llvm.coro.suspend(token %87, i1 true)
  switch i8 %88, label %111 [
    i8 0, label %99
    i8 1, label %89
  ]

89:                                               ; preds = %86
  br label %100

90:                                               ; preds = %78
  %91 = landingpad { ptr, i32 }
          cleanup
  %92 = extractvalue { ptr, i32 } %91, 0
  store ptr %92, ptr %6, align 8
  %93 = extractvalue { ptr, i32 } %91, 1
  store i32 %93, ptr %7, align 4
  invoke void @__cxa_end_catch()
          to label %98 unwind label %131

94:                                               ; preds = %81
  %95 = landingpad { ptr, i32 }
          cleanup
  %96 = extractvalue { ptr, i32 } %95, 0
  store ptr %96, ptr %6, align 8
  %97 = extractvalue { ptr, i32 } %95, 1
  store i32 %97, ptr %7, align 4
  br label %117

98:                                               ; preds = %90
  br label %117

99:                                               ; preds = %86, %84
  call void @_ZNKSt7__n486113suspend_never12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %13) #7
  br label %100

100:                                              ; preds = %99, %89
  %101 = phi i32 [ 0, %99 ], [ 2, %89 ]
  call void @llvm.lifetime.end.p0(i64 1, ptr %13) #7
  switch i32 %101, label %103 [
    i32 0, label %102
  ]

102:                                              ; preds = %100
  br label %103

103:                                              ; preds = %102, %100, %69, %40
  %104 = phi i32 [ %41, %40 ], [ %70, %69 ], [ %101, %100 ], [ 0, %102 ]
  call void @llvm.lifetime.end.p0(i64 1, ptr %5) #7
  call void @llvm.lifetime.end.p0(i64 8, ptr %4) #7
  %105 = call ptr @llvm.coro.free(token %16, ptr %23)
  %106 = icmp ne ptr %105, null
  br i1 %106, label %107, label %109

107:                                              ; preds = %103
  %108 = call i64 @llvm.coro.size.i64()
  call void @_ZdlPvm(ptr noundef %105, i64 noundef %108) #7
  br label %109

109:                                              ; preds = %103, %107
  switch i32 %104, label %134 [
    i32 0, label %110
    i32 2, label %111
  ]

110:                                              ; preds = %109
  br label %111

111:                                              ; preds = %110, %109, %86, %49, %31
  %112 = call i1 @llvm.coro.end(ptr null, i1 false, token none)
  %113 = getelementptr inbounds %struct.task.2, ptr %2, i32 0, i32 0
  %114 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.3", ptr %113, i32 0, i32 0
  %115 = load ptr, ptr %114, align 8
  %116 = ptrtoint ptr %115 to i64
  ret i64 %116

117:                                              ; preds = %98, %94
  %118 = call i1 @llvm.coro.end(ptr null, i1 true, token none)
  br i1 %118, label %126, label %119

119:                                              ; preds = %117
  br label %120

120:                                              ; preds = %119, %35
  call void @llvm.lifetime.end.p0(i64 1, ptr %5) #7
  call void @llvm.lifetime.end.p0(i64 8, ptr %4) #7
  %121 = call ptr @llvm.coro.free(token %16, ptr %23)
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

131:                                              ; preds = %90
  %132 = landingpad { ptr, i32 }
          catch ptr null
  %133 = extractvalue { ptr, i32 } %132, 0
  call void @__clang_call_terminate(ptr %133) #22
  unreachable

134:                                              ; preds = %109
  unreachable
}

; Function Attrs: mustprogress noinline optnone uwtable
define linkonce_odr dso_local void @_ZNKSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEE6resumeEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #10 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.3", ptr %3, i32 0, i32 0
  %5 = load ptr, ptr %4, align 8
  call void @llvm.coro.resume(ptr %5)
  ret void
}

; Function Attrs: mustprogress noinline optnone uwtable
define linkonce_odr dso_local void @_ZNKSt7__n486116coroutine_handleIN9idle_task12promise_typeEE7destroyEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #10 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %3, i32 0, i32 0
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
  %4 = load ptr, ptr %3, align 8
  %5 = call ptr @llvm.coro.promise(ptr %4, i32 1, i1 true)
  %6 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %2, i32 0, i32 0
  store ptr %5, ptr %6, align 8
  %7 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %2, i32 0, i32 0
  %8 = load ptr, ptr %7, align 8
  %9 = ptrtoint ptr %8 to i64
  ret i64 %9
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZNSt7__n486116coroutine_handleIN9idle_task12promise_typeEEC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %0) unnamed_addr #2 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %3, i32 0, i32 0
  store ptr null, ptr %4, align 8
  ret void
}

; Function Attrs: nounwind memory(none)
declare ptr @llvm.coro.promise(ptr nocapture, i32, i1) #8

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local i64 @_ZNSt7__n486116coroutine_handleIvE12from_addressEPv(ptr noundef %0) #2 comdat {
  %2 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %3 = alloca ptr, align 8
  store ptr %0, ptr %3, align 8
  call void @_ZNSt7__n486116coroutine_handleIvEC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %2) #7
  %4 = load ptr, ptr %3, align 8
  %5 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %2, i32 0, i32 0
  store ptr %4, ptr %5, align 8
  %6 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %2, i32 0, i32 0
  %7 = load ptr, ptr %6, align 8
  %8 = ptrtoint ptr %7 to i64
  ret i64 %8
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local noundef ptr @_ZNKSt7__n486116coroutine_handleIN9idle_task12promise_typeEE7addressEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #2 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %3, i32 0, i32 0
  %5 = load ptr, ptr %4, align 8
  ret ptr %5
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZNSt7__n486116coroutine_handleIvEC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %0) unnamed_addr #2 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %3, i32 0, i32 0
  store ptr null, ptr %4, align 8
  ret void
}

; Function Attrs: mustprogress noinline optnone uwtable
define linkonce_odr dso_local i64 @_ZN4taskILb1EE12promise_type17get_return_objectEv(ptr noundef nonnull align 1 dereferenceable(1) %0) #10 comdat {
  %2 = alloca %struct.task, align 8
  %3 = alloca ptr, align 8
  store ptr %0, ptr %3, align 8
  %4 = load ptr, ptr %3, align 8
  %5 = getelementptr inbounds %struct.task, ptr %2, i32 0, i32 0
  %6 = call i64 @_ZNSt7__n486116coroutine_handleIN4taskILb1EE12promise_typeEE12from_promiseERS3_(ptr noundef nonnull align 1 dereferenceable(1) %4)
  %7 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.1", ptr %5, i32 0, i32 0
  %8 = inttoptr i64 %6 to ptr
  store ptr %8, ptr %7, align 8
  %9 = getelementptr inbounds %struct.task, ptr %2, i32 0, i32 0
  %10 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.1", ptr %9, i32 0, i32 0
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
define private void @_Z3runILb1EE4taskIXT_EEPKc.__await_suspend_wrapper__init(ptr noundef nonnull %0, ptr noundef %1) #12 {
  %3 = alloca ptr, align 8
  %4 = alloca ptr, align 8
  %5 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %6 = alloca %"struct.std::__n4861::coroutine_handle.1", align 8
  store ptr %0, ptr %3, align 8
  store ptr %1, ptr %4, align 8
  %7 = load ptr, ptr %3, align 8
  %8 = load ptr, ptr %4, align 8
  %9 = call i64 @_ZNSt7__n486116coroutine_handleIN4taskILb1EE12promise_typeEE12from_addressEPv(ptr noundef %8) #7
  %10 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.1", ptr %6, i32 0, i32 0
  %11 = inttoptr i64 %9 to ptr
  store ptr %11, ptr %10, align 8
  %12 = call i64 @_ZNKSt7__n486116coroutine_handleIN4taskILb1EE12promise_typeEEcvNS0_IvEEEv(ptr noundef nonnull align 8 dereferenceable(8) %6) #7
  %13 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %5, i32 0, i32 0
  %14 = inttoptr i64 %12 to ptr
  store ptr %14, ptr %13, align 8
  %15 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %5, i32 0, i32 0
  %16 = load ptr, ptr %15, align 8
  %17 = ptrtoint ptr %16 to i64
  call void @_ZNKSt7__n486113suspend_never13await_suspendENS_16coroutine_handleIvEE(ptr noundef nonnull align 1 dereferenceable(1) %7, i64 %17) #7
  ret void
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZNKSt7__n486113suspend_never13await_suspendENS_16coroutine_handleIvEE(ptr noundef nonnull align 1 dereferenceable(1) %0, i64 %1) #2 comdat {
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
define linkonce_odr dso_local i64 @_ZNSt7__n486116coroutine_handleIN4taskILb1EE12promise_typeEE12from_addressEPv(ptr noundef %0) #2 comdat {
  %2 = alloca %"struct.std::__n4861::coroutine_handle.1", align 8
  %3 = alloca ptr, align 8
  store ptr %0, ptr %3, align 8
  call void @_ZNSt7__n486116coroutine_handleIN4taskILb1EE12promise_typeEEC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %2) #7
  %4 = load ptr, ptr %3, align 8
  %5 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.1", ptr %2, i32 0, i32 0
  store ptr %4, ptr %5, align 8
  %6 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.1", ptr %2, i32 0, i32 0
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
  %7 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %2, i32 0, i32 0
  %8 = inttoptr i64 %6 to ptr
  store ptr %8, ptr %7, align 8
  %9 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %2, i32 0, i32 0
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
define private noundef ptr @_Z3runILb1EE4taskIXT_EEPKc.__await_suspend_wrapper__await(ptr noundef nonnull %0, ptr noundef %1) #12 {
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
  %11 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.1", ptr %7, i32 0, i32 0
  %12 = inttoptr i64 %10 to ptr
  store ptr %12, ptr %11, align 8
  %13 = call i64 @_ZNKSt7__n486116coroutine_handleIN4taskILb1EE12promise_typeEEcvNS0_IvEEEv(ptr noundef nonnull align 8 dereferenceable(8) %7) #7
  %14 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %6, i32 0, i32 0
  %15 = inttoptr i64 %13 to ptr
  store ptr %15, ptr %14, align 8
  %16 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %6, i32 0, i32 0
  %17 = load ptr, ptr %16, align 8
  %18 = ptrtoint ptr %17 to i64
  %19 = call i64 @_ZN10resume_now13await_suspendENSt7__n486116coroutine_handleIvEE(ptr noundef nonnull align 1 dereferenceable(1) %8, i64 %18)
  %20 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %5, i32 0, i32 0
  %21 = inttoptr i64 %19 to ptr
  store ptr %21, ptr %20, align 8
  %22 = call noundef ptr @_ZNKSt7__n486116coroutine_handleIvE7addressEv(ptr noundef nonnull align 8 dereferenceable(8) %5) #7
  ret ptr %22
}

; Function Attrs: mustprogress noinline optnone uwtable
define linkonce_odr dso_local i64 @_ZN10resume_now13await_suspendENSt7__n486116coroutine_handleIvEE(ptr noundef nonnull align 1 dereferenceable(1) %0, i64 %1) #10 comdat {
  %3 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %4 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %5 = alloca ptr, align 8
  %6 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %4, i32 0, i32 0
  %7 = inttoptr i64 %1 to ptr
  store ptr %7, ptr %6, align 8
  store ptr %0, ptr %5, align 8
  %8 = load ptr, ptr %5, align 8
  call void @_ZNKSt7__n486116coroutine_handleIvE6resumeEv(ptr noundef nonnull align 8 dereferenceable(8) %4)
  call void @_ZL9take_backv()
  call void @llvm.memcpy.p0.p0.i64(ptr align 8 %3, ptr align 8 @_ZL6target, i64 8, i1 false)
  %9 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %3, i32 0, i32 0
  %10 = load ptr, ptr %9, align 8
  %11 = ptrtoint ptr %10 to i64
  ret i64 %11
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local noundef ptr @_ZNKSt7__n486116coroutine_handleIvE7addressEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #2 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %3, i32 0, i32 0
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
define private void @_Z3runILb1EE4taskIXT_EEPKc.__await_suspend_wrapper__final(ptr noundef nonnull %0, ptr noundef %1) #12 {
  %3 = alloca ptr, align 8
  %4 = alloca ptr, align 8
  %5 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %6 = alloca %"struct.std::__n4861::coroutine_handle.1", align 8
  store ptr %0, ptr %3, align 8
  store ptr %1, ptr %4, align 8
  %7 = load ptr, ptr %3, align 8
  %8 = load ptr, ptr %4, align 8
  %9 = call i64 @_ZNSt7__n486116coroutine_handleIN4taskILb1EE12promise_typeEE12from_addressEPv(ptr noundef %8) #7
  %10 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.1", ptr %6, i32 0, i32 0
  %11 = inttoptr i64 %9 to ptr
  store ptr %11, ptr %10, align 8
  %12 = call i64 @_ZNKSt7__n486116coroutine_handleIN4taskILb1EE12promise_typeEEcvNS0_IvEEEv(ptr noundef nonnull align 8 dereferenceable(8) %6) #7
  %13 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %5, i32 0, i32 0
  %14 = inttoptr i64 %12 to ptr
  store ptr %14, ptr %13, align 8
  %15 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %5, i32 0, i32 0
  %16 = load ptr, ptr %15, align 8
  %17 = ptrtoint ptr %16 to i64
  call void @_ZNKSt7__n486113suspend_never13await_suspendENS_16coroutine_handleIvEE(ptr noundef nonnull align 1 dereferenceable(1) %7, i64 %17) #7
  ret void
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local i64 @_ZNSt7__n486116coroutine_handleIN4taskILb1EE12promise_typeEE12from_promiseERS3_(ptr noundef nonnull align 1 dereferenceable(1) %0) #2 comdat {
  %2 = alloca %"struct.std::__n4861::coroutine_handle.1", align 8
  %3 = alloca ptr, align 8
  store ptr %0, ptr %3, align 8
  call void @_ZNSt7__n486116coroutine_handleIN4taskILb1EE12promise_typeEEC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %2) #7
  %4 = load ptr, ptr %3, align 8
  %5 = call ptr @llvm.coro.promise(ptr %4, i32 1, i1 true)
  %6 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.1", ptr %2, i32 0, i32 0
  store ptr %5, ptr %6, align 8
  %7 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.1", ptr %2, i32 0, i32 0
  %8 = load ptr, ptr %7, align 8
  %9 = ptrtoint ptr %8 to i64
  ret i64 %9
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZNSt7__n486116coroutine_handleIN4taskILb1EE12promise_typeEEC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %0) unnamed_addr #2 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.1", ptr %3, i32 0, i32 0
  store ptr null, ptr %4, align 8
  ret void
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local noundef ptr @_ZNKSt7__n486116coroutine_handleIN4taskILb1EE12promise_typeEE7addressEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #2 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.1", ptr %3, i32 0, i32 0
  %5 = load ptr, ptr %4, align 8
  ret ptr %5
}

; Function Attrs: mustprogress noinline optnone uwtable
define linkonce_odr dso_local void @_ZNKSt7__n486116coroutine_handleIvE6resumeEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #10 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %3, i32 0, i32 0
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
declare void @llvm.memset.p0.i64(ptr nocapture writeonly, i8, i64, i1 immarg) #18

; Function Attrs: mustprogress noinline optnone uwtable
define linkonce_odr dso_local i64 @_ZN4taskILb0EE12promise_type17get_return_objectEv(ptr noundef nonnull align 1 dereferenceable(1) %0) #10 comdat {
  %2 = alloca %struct.task.2, align 8
  %3 = alloca ptr, align 8
  store ptr %0, ptr %3, align 8
  %4 = load ptr, ptr %3, align 8
  %5 = getelementptr inbounds %struct.task.2, ptr %2, i32 0, i32 0
  %6 = call i64 @_ZNSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEE12from_promiseERS3_(ptr noundef nonnull align 1 dereferenceable(1) %4)
  %7 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.3", ptr %5, i32 0, i32 0
  %8 = inttoptr i64 %6 to ptr
  store ptr %8, ptr %7, align 8
  %9 = getelementptr inbounds %struct.task.2, ptr %2, i32 0, i32 0
  %10 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.3", ptr %9, i32 0, i32 0
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
define private void @_Z3runILb0EE4taskIXT_EEPKc.__await_suspend_wrapper__init(ptr noundef nonnull %0, ptr noundef %1) #12 {
  %3 = alloca ptr, align 8
  %4 = alloca ptr, align 8
  %5 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %6 = alloca %"struct.std::__n4861::coroutine_handle.3", align 8
  store ptr %0, ptr %3, align 8
  store ptr %1, ptr %4, align 8
  %7 = load ptr, ptr %3, align 8
  %8 = load ptr, ptr %4, align 8
  %9 = call i64 @_ZNSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEE12from_addressEPv(ptr noundef %8) #7
  %10 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.3", ptr %6, i32 0, i32 0
  %11 = inttoptr i64 %9 to ptr
  store ptr %11, ptr %10, align 8
  %12 = call i64 @_ZNKSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEEcvNS0_IvEEEv(ptr noundef nonnull align 8 dereferenceable(8) %6) #7
  %13 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %5, i32 0, i32 0
  %14 = inttoptr i64 %12 to ptr
  store ptr %14, ptr %13, align 8
  %15 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %5, i32 0, i32 0
  %16 = load ptr, ptr %15, align 8
  %17 = ptrtoint ptr %16 to i64
  call void @_ZNKSt7__n486114suspend_always13await_suspendENS_16coroutine_handleIvEE(ptr noundef nonnull align 1 dereferenceable(1) %7, i64 %17) #7
  ret void
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local i64 @_ZNSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEE12from_addressEPv(ptr noundef %0) #2 comdat {
  %2 = alloca %"struct.std::__n4861::coroutine_handle.3", align 8
  %3 = alloca ptr, align 8
  store ptr %0, ptr %3, align 8
  call void @_ZNSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEEC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %2) #7
  %4 = load ptr, ptr %3, align 8
  %5 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.3", ptr %2, i32 0, i32 0
  store ptr %4, ptr %5, align 8
  %6 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.3", ptr %2, i32 0, i32 0
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
  %7 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %2, i32 0, i32 0
  %8 = inttoptr i64 %6 to ptr
  store ptr %8, ptr %7, align 8
  %9 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %2, i32 0, i32 0
  %10 = load ptr, ptr %9, align 8
  %11 = ptrtoint ptr %10 to i64
  ret i64 %11
}

; Function Attrs: alwaysinline mustprogress
define private noundef ptr @_Z3runILb0EE4taskIXT_EEPKc.__await_suspend_wrapper__await(ptr noundef nonnull %0, ptr noundef %1) #12 {
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
  %11 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.3", ptr %7, i32 0, i32 0
  %12 = inttoptr i64 %10 to ptr
  store ptr %12, ptr %11, align 8
  %13 = call i64 @_ZNKSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEEcvNS0_IvEEEv(ptr noundef nonnull align 8 dereferenceable(8) %7) #7
  %14 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %6, i32 0, i32 0
  %15 = inttoptr i64 %13 to ptr
  store ptr %15, ptr %14, align 8
  %16 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %6, i32 0, i32 0
  %17 = load ptr, ptr %16, align 8
  %18 = ptrtoint ptr %17 to i64
  %19 = call i64 @_ZN10resume_now13await_suspendENSt7__n486116coroutine_handleIvEE(ptr noundef nonnull align 1 dereferenceable(1) %8, i64 %18)
  %20 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %5, i32 0, i32 0
  %21 = inttoptr i64 %19 to ptr
  store ptr %21, ptr %20, align 8
  %22 = call noundef ptr @_ZNKSt7__n486116coroutine_handleIvE7addressEv(ptr noundef nonnull align 8 dereferenceable(8) %5) #7
  ret ptr %22
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
define private void @_Z3runILb0EE4taskIXT_EEPKc.__await_suspend_wrapper__final(ptr noundef nonnull %0, ptr noundef %1) #12 {
  %3 = alloca ptr, align 8
  %4 = alloca ptr, align 8
  %5 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %6 = alloca %"struct.std::__n4861::coroutine_handle.3", align 8
  store ptr %0, ptr %3, align 8
  store ptr %1, ptr %4, align 8
  %7 = load ptr, ptr %3, align 8
  %8 = load ptr, ptr %4, align 8
  %9 = call i64 @_ZNSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEE12from_addressEPv(ptr noundef %8) #7
  %10 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.3", ptr %6, i32 0, i32 0
  %11 = inttoptr i64 %9 to ptr
  store ptr %11, ptr %10, align 8
  %12 = call i64 @_ZNKSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEEcvNS0_IvEEEv(ptr noundef nonnull align 8 dereferenceable(8) %6) #7
  %13 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %5, i32 0, i32 0
  %14 = inttoptr i64 %12 to ptr
  store ptr %14, ptr %13, align 8
  %15 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %5, i32 0, i32 0
  %16 = load ptr, ptr %15, align 8
  %17 = ptrtoint ptr %16 to i64
  call void @_ZNKSt7__n486113suspend_never13await_suspendENS_16coroutine_handleIvEE(ptr noundef nonnull align 1 dereferenceable(1) %7, i64 %17) #7
  ret void
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local i64 @_ZNSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEE12from_promiseERS3_(ptr noundef nonnull align 1 dereferenceable(1) %0) #2 comdat {
  %2 = alloca %"struct.std::__n4861::coroutine_handle.3", align 8
  %3 = alloca ptr, align 8
  store ptr %0, ptr %3, align 8
  call void @_ZNSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEEC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %2) #7
  %4 = load ptr, ptr %3, align 8
  %5 = call ptr @llvm.coro.promise(ptr %4, i32 1, i1 true)
  %6 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.3", ptr %2, i32 0, i32 0
  store ptr %5, ptr %6, align 8
  %7 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.3", ptr %2, i32 0, i32 0
  %8 = load ptr, ptr %7, align 8
  %9 = ptrtoint ptr %8 to i64
  ret i64 %9
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZNSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEEC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %0) unnamed_addr #2 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.3", ptr %3, i32 0, i32 0
  store ptr null, ptr %4, align 8
  ret void
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local noundef ptr @_ZNKSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEE7addressEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #2 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.3", ptr %3, i32 0, i32 0
  %5 = load ptr, ptr %4, align 8
  ret ptr %5
}

declare void @llvm.coro.destroy(ptr)

attributes #0 = { mustprogress nobuiltin noinline optnone allocsize(0) uwtable "frame-pointer"="non-leaf" "no-trapping-math"="true" "stack-protector-buffer-size"="8" }
attributes #1 = { nounwind allocsize(0) "frame-pointer"="non-leaf" "no-trapping-math"="true" "stack-protector-buffer-size"="8" }
attributes #2 = { mustprogress noinline nounwind optnone uwtable "frame-pointer"="non-leaf" "no-trapping-math"="true" "stack-protector-buffer-size"="8" }
attributes #3 = { nounwind "frame-pointer"="non-leaf" "no-trapping-math"="true" "stack-protector-buffer-size"="8" }
attributes #4 = { mustprogress nobuiltin noinline nounwind optnone uwtable "frame-pointer"="non-leaf" "no-trapping-math"="true" "stack-protector-buffer-size"="8" }
attributes #5 = { mustprogress noinline optnone presplitcoroutine uwtable "frame-pointer"="non-leaf" "no-trapping-math"="true" "stack-protector-buffer-size"="8" }
attributes #6 = { nocallback nofree nosync nounwind willreturn memory(argmem: read) }
attributes #7 = { nounwind }
attributes #8 = { nounwind memory(none) }
attributes #9 = { nocallback nofree nosync nounwind willreturn memory(argmem: readwrite) }
attributes #10 = { mustprogress noinline optnone uwtable "frame-pointer"="non-leaf" "no-trapping-math"="true" "stack-protector-buffer-size"="8" }
attributes #11 = { nomerge nounwind }
attributes #12 = { alwaysinline mustprogress }
attributes #13 = { "frame-pointer"="non-leaf" "no-trapping-math"="true" "stack-protector-buffer-size"="8" }
attributes #14 = { noinline noreturn nounwind uwtable "frame-pointer"="non-leaf" "no-trapping-math"="true" "stack-protector-buffer-size"="8" }
attributes #15 = { nounwind memory(argmem: read) }
attributes #16 = { mustprogress noinline norecurse optnone uwtable "frame-pointer"="non-leaf" "no-trapping-math"="true" "stack-protector-buffer-size"="8" }
attributes #17 = { nocallback nofree nounwind willreturn memory(argmem: readwrite) }
attributes #18 = { nocallback nofree nounwind willreturn memory(argmem: write) }
attributes #19 = { nounwind allocsize(0) }
attributes #20 = { noreturn }
attributes #21 = { allocsize(0) }
attributes #22 = { noreturn nounwind }

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
