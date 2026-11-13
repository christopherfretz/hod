; clang++-22 -std=c++20 -O0 -S -emit-llvm -Xclang -disable-llvm-passes
; (aarch64-unknown-linux-gnu; the target triple, datalayout, target-cpu and
; target-features removed, so that the seed runs on every target) of:
;
; // Design 0005 §3.3, invariant "no frame access after the wrapper returns".
; // Each await_suspend resumes the coroutine before returning; the coroutine
; // runs to completion (final_suspend is suspend_never), so the frame is
; // already freed when the wrapper returns into the suspending activation.
; // The freed block is then taken back by a canary allocation of the same
; // size and filled with a pattern; any load from or store to the frame on
; // the suspending activation's path after the wrapper shows up as a broken
; // canary or garbage. Run with GLIBC_TUNABLES=glibc.malloc.tcache_count=0
; // MALLOC_PERTURB_=165 as well. Covers .void and .bool (.handle: WP-107).
; // Expected stdout:
; //   void: body 3 / canary ok
; //   bool: body 3 / canary ok
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
; struct task {
;   struct promise_type {
;     task get_return_object() { return {}; }
;     std::suspend_never initial_suspend() noexcept { return {}; }
;     std::suspend_never final_suspend() noexcept { return {}; }
;     void return_void() {}
;     void unhandled_exception() {}
;   };
; };
; struct resume_now_void {
;   bool await_ready() { return false; }
;   void await_suspend(std::coroutine_handle<> h) { h.resume(); take_back(); }
;   void await_resume() {}
; };
; struct resume_now_bool {
;   bool await_ready() { return false; }
;   bool await_suspend(std::coroutine_handle<> h) { h.resume(); take_back(); return true; }
;   void await_resume() {}
; };
; template <class A> task run(const char* name) {
;   int a = 1, b = 2;
;   std::printf("%s: ", name);
;   co_await A{};
;   std::printf("body %d\n", a + b);
; }
; int main() {
;   run<resume_now_void>("void");
;   std::puts(canary_ok() ? "canary ok" : "canary BROKEN");
;   std::free(canary);
;   run<resume_now_bool>("bool");
;   std::puts(canary_ok() ? "canary ok" : "canary BROKEN");
;   std::free(canary);
; }
;
;@ covers: spec/ir.md §5.5 intrinsics (coroutine bodies; spec/codegen.md §7.6)
;@ input: pre-opt
;@ dialect: 22
;@ link: c++
;@ stdout: void: body 3
;@ stdout: canary ok
;@ stdout: bool: body 3
;@ stdout: canary ok

source_filename = "resume-inside-await-suspend.cpp"

%struct.task = type { i8 }
%"struct.task::promise_type" = type { i8 }
%"struct.std::__n4861::suspend_never" = type { i8 }
%struct.resume_now_void = type { i8 }
%struct.resume_now_bool = type { i8 }
%"struct.std::__n4861::coroutine_handle" = type { ptr }
%"struct.std::__n4861::coroutine_handle.0" = type { ptr }

$_ZNSt9bad_allocC2Ev = comdat any

$_Z3runI15resume_now_voidE4taskPKc = comdat any

$_Z3runI15resume_now_boolE4taskPKc = comdat any

$_ZNSt9exceptionC2Ev = comdat any

$_ZN4task12promise_type17get_return_objectEv = comdat any

$_ZN4task12promise_type15initial_suspendEv = comdat any

$_ZNKSt7__n486113suspend_never11await_readyEv = comdat any

$_ZNKSt7__n486113suspend_never13await_suspendENS_16coroutine_handleIvEE = comdat any

$_ZNSt7__n486116coroutine_handleIN4task12promise_typeEE12from_addressEPv = comdat any

$_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEEcvNS0_IvEEEv = comdat any

$_ZNKSt7__n486113suspend_never12await_resumeEv = comdat any

$_ZN15resume_now_void11await_readyEv = comdat any

$_ZN15resume_now_void13await_suspendENSt7__n486116coroutine_handleIvEE = comdat any

$_ZN15resume_now_void12await_resumeEv = comdat any

$_ZN4task12promise_type11return_voidEv = comdat any

$_ZN4task12promise_type19unhandled_exceptionEv = comdat any

$__clang_call_terminate = comdat any

$_ZN4task12promise_type13final_suspendEv = comdat any

$_ZNSt7__n486116coroutine_handleIN4task12promise_typeEEC2Ev = comdat any

$_ZNSt7__n486116coroutine_handleIvE12from_addressEPv = comdat any

$_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEE7addressEv = comdat any

$_ZNSt7__n486116coroutine_handleIvEC2Ev = comdat any

$_ZNKSt7__n486116coroutine_handleIvE6resumeEv = comdat any

$_ZN15resume_now_bool11await_readyEv = comdat any

$_ZN15resume_now_bool13await_suspendENSt7__n486116coroutine_handleIvEE = comdat any

$_ZN15resume_now_bool12await_resumeEv = comdat any

@_ZL9last_size = internal global i64 0, align 8
@_ZTISt9bad_alloc = external constant ptr
@.str = private unnamed_addr constant [5 x i8] c"void\00", align 1
@.str.1 = private unnamed_addr constant [10 x i8] c"canary ok\00", align 1
@.str.2 = private unnamed_addr constant [14 x i8] c"canary BROKEN\00", align 1
@_ZL6canary = internal global ptr null, align 8
@.str.3 = private unnamed_addr constant [5 x i8] c"bool\00", align 1
@_ZTVSt9bad_alloc = external unnamed_addr constant { [5 x ptr] }, align 8
@_ZTVSt9exception = external unnamed_addr constant { [5 x ptr] }, align 8
@.str.4 = private unnamed_addr constant [5 x i8] c"%s: \00", align 1
@.str.5 = private unnamed_addr constant [9 x i8] c"body %d\0A\00", align 1

; Function Attrs: mustprogress nobuiltin noinline optnone allocsize(0) uwtable
define dso_local noundef nonnull ptr @_Znwm(i64 noundef %0) #0 {
  %2 = alloca i64, align 8
  %3 = alloca ptr, align 8
  store i64 %0, ptr %2, align 8
  %4 = load i64, ptr %2, align 8
  store i64 %4, ptr @_ZL9last_size, align 8
  %5 = load i64, ptr %2, align 8
  %6 = call noalias ptr @malloc(i64 noundef %5) #18
  store ptr %6, ptr %3, align 8
  %7 = load ptr, ptr %3, align 8
  %8 = icmp ne ptr %7, null
  br i1 %8, label %9, label %11

9:                                                ; preds = %1
  %10 = load ptr, ptr %3, align 8
  ret ptr %10

11:                                               ; preds = %1
  %12 = call ptr @__cxa_allocate_exception(i64 8) #9
  call void @_ZNSt9bad_allocC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %12) #9
  call void @__cxa_throw(ptr %12, ptr @_ZTISt9bad_alloc, ptr @_ZNSt9bad_allocD1Ev) #19
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
  call void @_ZNSt9exceptionC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %3) #9
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
  call void @free(ptr noundef %3) #9
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
  call void @free(ptr noundef %5) #9
  ret void
}

; Function Attrs: mustprogress noinline norecurse optnone uwtable
define dso_local noundef i32 @main() #5 {
  %1 = alloca %struct.task, align 1
  %2 = alloca %struct.task, align 1
  call void @_Z3runI15resume_now_voidE4taskPKc(ptr noundef @.str)
  %3 = call noundef i1 @_ZL9canary_okv()
  %4 = zext i1 %3 to i64
  %5 = select i1 %3, ptr @.str.1, ptr @.str.2
  %6 = call i32 @puts(ptr noundef %5)
  %7 = load ptr, ptr @_ZL6canary, align 8
  call void @free(ptr noundef %7) #9
  call void @_Z3runI15resume_now_boolE4taskPKc(ptr noundef @.str.3)
  %8 = call noundef i1 @_ZL9canary_okv()
  %9 = zext i1 %8 to i64
  %10 = select i1 %8, ptr @.str.1, ptr @.str.2
  %11 = call i32 @puts(ptr noundef %10)
  %12 = load ptr, ptr @_ZL6canary, align 8
  call void @free(ptr noundef %12) #9
  ret i32 0
}

; Function Attrs: mustprogress noinline optnone presplitcoroutine uwtable
define linkonce_odr dso_local void @_Z3runI15resume_now_voidE4taskPKc(ptr noundef %0) #6 comdat personality ptr @__gxx_personality_v0 {
  %2 = alloca ptr, align 8, !coro.outside.frame !6
  %3 = alloca ptr, align 8
  %4 = alloca %"struct.task::promise_type", align 1
  %5 = alloca ptr, align 8
  %6 = alloca i32, align 4
  %7 = alloca %struct.task, align 1
  %8 = alloca %"struct.std::__n4861::suspend_never", align 1
  %9 = alloca %"struct.std::__n4861::suspend_never", align 1
  %10 = alloca i32, align 4
  %11 = alloca i32, align 4
  %12 = alloca %struct.resume_now_void, align 1
  %13 = alloca %"struct.std::__n4861::suspend_never", align 1
  %14 = alloca %"struct.std::__n4861::suspend_never", align 1
  store ptr %0, ptr %2, align 8
  %15 = call token @llvm.coro.id(i32 16, ptr %4, ptr null, ptr null)
  %16 = call i1 @llvm.coro.alloc(token %15)
  br i1 %16, label %17, label %20

17:                                               ; preds = %1
  %18 = call i64 @llvm.coro.size.i64()
  %19 = call noalias noundef nonnull ptr @_Znwm(i64 noundef %18) #20
  br label %20

20:                                               ; preds = %17, %1
  %21 = phi ptr [ null, %1 ], [ %19, %17 ]
  %22 = call ptr @llvm.coro.begin(token %15, ptr %21)
  call void @llvm.lifetime.start.p0(ptr %3) #9
  %23 = load ptr, ptr %2, align 8
  store ptr %23, ptr %3, align 8
  call void @llvm.lifetime.start.p0(ptr %4) #9
  invoke void @_ZN4task12promise_type17get_return_objectEv(ptr noundef nonnull align 1 dereferenceable(1) %4)
          to label %24 unwind label %30

24:                                               ; preds = %20
  call void @llvm.lifetime.start.p0(ptr %8) #9
  call void @_ZN4task12promise_type15initial_suspendEv(ptr noundef nonnull align 1 dereferenceable(1) %4) #9
  %25 = call noundef i1 @_ZNKSt7__n486113suspend_never11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %8) #9
  br i1 %25, label %34, label %26

26:                                               ; preds = %24
  %27 = call token @llvm.coro.save(ptr null)
  call void @llvm.coro.await.suspend.void(ptr %8, ptr %22, ptr @_Z3runI15resume_now_voidE4taskPKc.__await_suspend_wrapper__init) #9
  %28 = call i8 @llvm.coro.suspend(token %27, i1 false)
  switch i8 %28, label %108 [
    i8 0, label %34
    i8 1, label %29
  ]

29:                                               ; preds = %26
  br label %35

30:                                               ; preds = %20
  %31 = landingpad { ptr, i32 }
          cleanup
  %32 = extractvalue { ptr, i32 } %31, 0
  store ptr %32, ptr %5, align 8
  %33 = extractvalue { ptr, i32 } %31, 1
  store i32 %33, ptr %6, align 4
  br label %112

34:                                               ; preds = %26, %24
  call void @_ZNKSt7__n486113suspend_never12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %8) #9
  br label %35

35:                                               ; preds = %34, %29
  %36 = phi i32 [ 0, %34 ], [ 2, %29 ]
  call void @llvm.lifetime.end.p0(ptr %8) #9
  switch i32 %36, label %100 [
    i32 0, label %37
  ]

37:                                               ; preds = %35
  call void @llvm.lifetime.start.p0(ptr %10) #9
  store i32 1, ptr %10, align 4
  call void @llvm.lifetime.start.p0(ptr %11) #9
  store i32 2, ptr %11, align 4
  %38 = load ptr, ptr %3, align 8
  %39 = invoke i32 (ptr, ...) @printf(ptr noundef @.str.4, ptr noundef %38)
          to label %40 unwind label %48

40:                                               ; preds = %37
  call void @llvm.lifetime.start.p0(ptr %12) #9
  %41 = invoke noundef i1 @_ZN15resume_now_void11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %12)
          to label %42 unwind label %52

42:                                               ; preds = %40
  br i1 %41, label %56, label %43

43:                                               ; preds = %42
  %44 = call token @llvm.coro.save(ptr null)
  invoke void @llvm.coro.await.suspend.void(ptr %12, ptr %22, ptr @_Z3runI15resume_now_voidE4taskPKc.__await_suspend_wrapper__await)
          to label %45 unwind label %52

45:                                               ; preds = %43
  %46 = call i8 @llvm.coro.suspend(token %44, i1 false)
  switch i8 %46, label %108 [
    i8 0, label %56
    i8 1, label %47
  ]

47:                                               ; preds = %45
  br label %58

48:                                               ; preds = %60, %37
  %49 = landingpad { ptr, i32 }
          catch ptr null
  %50 = extractvalue { ptr, i32 } %49, 0
  store ptr %50, ptr %5, align 8
  %51 = extractvalue { ptr, i32 } %49, 1
  store i32 %51, ptr %6, align 4
  br label %70

52:                                               ; preds = %56, %43, %40
  %53 = landingpad { ptr, i32 }
          catch ptr null
  %54 = extractvalue { ptr, i32 } %53, 0
  store ptr %54, ptr %5, align 8
  %55 = extractvalue { ptr, i32 } %53, 1
  store i32 %55, ptr %6, align 4
  call void @llvm.lifetime.end.p0(ptr %12) #9
  br label %70

56:                                               ; preds = %45, %42
  invoke void @_ZN15resume_now_void12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %12)
          to label %57 unwind label %52

57:                                               ; preds = %56
  br label %58

58:                                               ; preds = %57, %47
  %59 = phi i32 [ 0, %57 ], [ 2, %47 ]
  call void @llvm.lifetime.end.p0(ptr %12) #9
  switch i32 %59, label %66 [
    i32 0, label %60
  ]

60:                                               ; preds = %58
  %61 = load i32, ptr %10, align 4
  %62 = load i32, ptr %11, align 4
  %63 = add nsw i32 %61, %62
  %64 = invoke i32 (ptr, ...) @printf(ptr noundef @.str.5, i32 noundef %63)
          to label %65 unwind label %48

65:                                               ; preds = %60
  br label %66

66:                                               ; preds = %65, %58
  %67 = phi i32 [ %59, %58 ], [ 0, %65 ]
  call void @llvm.lifetime.end.p0(ptr %11) #9
  call void @llvm.lifetime.end.p0(ptr %10) #9
  switch i32 %67, label %100 [
    i32 0, label %68
  ]

68:                                               ; preds = %66
  invoke void @_ZN4task12promise_type11return_voidEv(ptr noundef nonnull align 1 dereferenceable(1) %4)
          to label %69 unwind label %71

69:                                               ; preds = %68
  br label %81

70:                                               ; preds = %52, %48
  call void @llvm.lifetime.end.p0(ptr %11) #9
  call void @llvm.lifetime.end.p0(ptr %10) #9
  br label %75

71:                                               ; preds = %68
  %72 = landingpad { ptr, i32 }
          catch ptr null
  %73 = extractvalue { ptr, i32 } %72, 0
  store ptr %73, ptr %5, align 8
  %74 = extractvalue { ptr, i32 } %72, 1
  store i32 %74, ptr %6, align 4
  br label %75

75:                                               ; preds = %71, %70
  %76 = load ptr, ptr %5, align 8
  %77 = call ptr @__cxa_begin_catch(ptr %76) #9
  invoke void @_ZN4task12promise_type19unhandled_exceptionEv(ptr noundef nonnull align 1 dereferenceable(1) %4)
          to label %78 unwind label %87

78:                                               ; preds = %75
  invoke void @__cxa_end_catch()
          to label %79 unwind label %91

79:                                               ; preds = %78
  br label %80

80:                                               ; preds = %79
  br label %81

81:                                               ; preds = %80, %69
  call void @llvm.lifetime.start.p0(ptr %13) #9
  call void @_ZN4task12promise_type13final_suspendEv(ptr noundef nonnull align 1 dereferenceable(1) %4) #9
  %82 = call noundef i1 @_ZNKSt7__n486113suspend_never11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %13) #9
  br i1 %82, label %96, label %83

83:                                               ; preds = %81
  %84 = call token @llvm.coro.save(ptr null)
  call void @llvm.coro.await.suspend.void(ptr %13, ptr %22, ptr @_Z3runI15resume_now_voidE4taskPKc.__await_suspend_wrapper__final) #9
  %85 = call i8 @llvm.coro.suspend(token %84, i1 true)
  switch i8 %85, label %108 [
    i8 0, label %96
    i8 1, label %86
  ]

86:                                               ; preds = %83
  br label %97

87:                                               ; preds = %75
  %88 = landingpad { ptr, i32 }
          cleanup
  %89 = extractvalue { ptr, i32 } %88, 0
  store ptr %89, ptr %5, align 8
  %90 = extractvalue { ptr, i32 } %88, 1
  store i32 %90, ptr %6, align 4
  invoke void @__cxa_end_catch()
          to label %95 unwind label %123

91:                                               ; preds = %78
  %92 = landingpad { ptr, i32 }
          cleanup
  %93 = extractvalue { ptr, i32 } %92, 0
  store ptr %93, ptr %5, align 8
  %94 = extractvalue { ptr, i32 } %92, 1
  store i32 %94, ptr %6, align 4
  br label %109

95:                                               ; preds = %87
  br label %109

96:                                               ; preds = %83, %81
  call void @_ZNKSt7__n486113suspend_never12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %13) #9
  br label %97

97:                                               ; preds = %96, %86
  %98 = phi i32 [ 0, %96 ], [ 2, %86 ]
  call void @llvm.lifetime.end.p0(ptr %13) #9
  switch i32 %98, label %100 [
    i32 0, label %99
  ]

99:                                               ; preds = %97
  br label %100

100:                                              ; preds = %99, %97, %66, %35
  %101 = phi i32 [ %36, %35 ], [ %67, %66 ], [ %98, %97 ], [ 0, %99 ]
  call void @llvm.lifetime.end.p0(ptr %4) #9
  call void @llvm.lifetime.end.p0(ptr %3) #9
  %102 = call ptr @llvm.coro.free(token %15, ptr %22)
  %103 = icmp ne ptr %102, null
  br i1 %103, label %104, label %106

104:                                              ; preds = %100
  %105 = call i64 @llvm.coro.size.i64()
  call void @_ZdlPvm(ptr noundef %102, i64 noundef %105) #9
  br label %106

106:                                              ; preds = %100, %104
  switch i32 %101, label %126 [
    i32 0, label %107
    i32 2, label %108
  ]

107:                                              ; preds = %106
  br label %108

108:                                              ; preds = %107, %106, %83, %45, %26
  call void @llvm.coro.end(ptr null, i1 false, token none)
  ret void

109:                                              ; preds = %95, %91
  call void @llvm.coro.end(ptr null, i1 true, token none)
  %110 = call i1 @llvm.coro.is_in_ramp()
  br i1 %110, label %111, label %118

111:                                              ; preds = %109
  br label %112

112:                                              ; preds = %111, %30
  call void @llvm.lifetime.end.p0(ptr %4) #9
  call void @llvm.lifetime.end.p0(ptr %3) #9
  %113 = call ptr @llvm.coro.free(token %15, ptr %22)
  %114 = icmp ne ptr %113, null
  br i1 %114, label %115, label %117

115:                                              ; preds = %112
  %116 = call i64 @llvm.coro.size.i64()
  call void @_ZdlPvm(ptr noundef %113, i64 noundef %116) #9
  br label %117

117:                                              ; preds = %112, %115
  br label %118

118:                                              ; preds = %117, %109
  %119 = load ptr, ptr %5, align 8
  %120 = load i32, ptr %6, align 4
  %121 = insertvalue { ptr, i32 } poison, ptr %119, 0
  %122 = insertvalue { ptr, i32 } %121, i32 %120, 1
  resume { ptr, i32 } %122

123:                                              ; preds = %87
  %124 = landingpad { ptr, i32 }
          catch ptr null
  %125 = extractvalue { ptr, i32 } %124, 0
  call void @__clang_call_terminate(ptr %125) #21
  unreachable

126:                                              ; preds = %106
  unreachable
}

declare i32 @puts(ptr noundef) #7

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
define linkonce_odr dso_local void @_Z3runI15resume_now_boolE4taskPKc(ptr noundef %0) #6 comdat personality ptr @__gxx_personality_v0 {
  %2 = alloca ptr, align 8, !coro.outside.frame !6
  %3 = alloca ptr, align 8
  %4 = alloca %"struct.task::promise_type", align 1
  %5 = alloca %struct.task, align 1
  %6 = alloca %"struct.std::__n4861::suspend_never", align 1
  %7 = alloca %"struct.std::__n4861::suspend_never", align 1
  %8 = alloca i32, align 4
  %9 = alloca i32, align 4
  %10 = alloca ptr, align 8
  %11 = alloca i32, align 4
  %12 = alloca %struct.resume_now_bool, align 1
  %13 = alloca %"struct.std::__n4861::suspend_never", align 1
  %14 = alloca %"struct.std::__n4861::suspend_never", align 1
  store ptr %0, ptr %2, align 8
  %15 = call token @llvm.coro.id(i32 16, ptr %4, ptr null, ptr null)
  %16 = call i1 @llvm.coro.alloc(token %15)
  br i1 %16, label %17, label %20

17:                                               ; preds = %1
  %18 = call i64 @llvm.coro.size.i64()
  %19 = call noalias noundef nonnull ptr @_Znwm(i64 noundef %18) #20
  br label %20

20:                                               ; preds = %17, %1
  %21 = phi ptr [ null, %1 ], [ %19, %17 ]
  %22 = call ptr @llvm.coro.begin(token %15, ptr %21)
  call void @llvm.lifetime.start.p0(ptr %3) #9
  %23 = load ptr, ptr %2, align 8
  store ptr %23, ptr %3, align 8
  call void @llvm.lifetime.start.p0(ptr %4) #9
  call void @_ZN4task12promise_type17get_return_objectEv(ptr noundef nonnull align 1 dereferenceable(1) %4)
  call void @llvm.lifetime.start.p0(ptr %6) #9
  call void @_ZN4task12promise_type15initial_suspendEv(ptr noundef nonnull align 1 dereferenceable(1) %4) #9
  %24 = call noundef i1 @_ZNKSt7__n486113suspend_never11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %6) #9
  br i1 %24, label %29, label %25

25:                                               ; preds = %20
  %26 = call token @llvm.coro.save(ptr null)
  call void @llvm.coro.await.suspend.void(ptr %6, ptr %22, ptr @_Z3runI15resume_now_boolE4taskPKc.__await_suspend_wrapper__init) #9
  %27 = call i8 @llvm.coro.suspend(token %26, i1 false)
  switch i8 %27, label %95 [
    i8 0, label %29
    i8 1, label %28
  ]

28:                                               ; preds = %25
  br label %30

29:                                               ; preds = %25, %20
  call void @_ZNKSt7__n486113suspend_never12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %6) #9
  br label %30

30:                                               ; preds = %29, %28
  %31 = phi i32 [ 0, %29 ], [ 2, %28 ]
  call void @llvm.lifetime.end.p0(ptr %6) #9
  switch i32 %31, label %87 [
    i32 0, label %32
  ]

32:                                               ; preds = %30
  call void @llvm.lifetime.start.p0(ptr %8) #9
  store i32 1, ptr %8, align 4
  call void @llvm.lifetime.start.p0(ptr %9) #9
  store i32 2, ptr %9, align 4
  %33 = load ptr, ptr %3, align 8
  %34 = invoke i32 (ptr, ...) @printf(ptr noundef @.str.4, ptr noundef %33)
          to label %35 unwind label %45

35:                                               ; preds = %32
  call void @llvm.lifetime.start.p0(ptr %12) #9
  %36 = invoke noundef i1 @_ZN15resume_now_bool11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %12)
          to label %37 unwind label %49

37:                                               ; preds = %35
  br i1 %36, label %53, label %38

38:                                               ; preds = %37
  %39 = call token @llvm.coro.save(ptr null)
  %40 = invoke i1 @llvm.coro.await.suspend.bool(ptr %12, ptr %22, ptr @_Z3runI15resume_now_boolE4taskPKc.__await_suspend_wrapper__await)
          to label %41 unwind label %49

41:                                               ; preds = %38
  br i1 %40, label %42, label %53

42:                                               ; preds = %41
  %43 = call i8 @llvm.coro.suspend(token %39, i1 false)
  switch i8 %43, label %95 [
    i8 0, label %53
    i8 1, label %44
  ]

44:                                               ; preds = %42
  br label %55

45:                                               ; preds = %57, %32
  %46 = landingpad { ptr, i32 }
          catch ptr null
  %47 = extractvalue { ptr, i32 } %46, 0
  store ptr %47, ptr %10, align 8
  %48 = extractvalue { ptr, i32 } %46, 1
  store i32 %48, ptr %11, align 4
  br label %66

49:                                               ; preds = %53, %38, %35
  %50 = landingpad { ptr, i32 }
          catch ptr null
  %51 = extractvalue { ptr, i32 } %50, 0
  store ptr %51, ptr %10, align 8
  %52 = extractvalue { ptr, i32 } %50, 1
  store i32 %52, ptr %11, align 4
  call void @llvm.lifetime.end.p0(ptr %12) #9
  br label %66

53:                                               ; preds = %42, %41, %37
  invoke void @_ZN15resume_now_bool12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %12)
          to label %54 unwind label %49

54:                                               ; preds = %53
  br label %55

55:                                               ; preds = %54, %44
  %56 = phi i32 [ 0, %54 ], [ 2, %44 ]
  call void @llvm.lifetime.end.p0(ptr %12) #9
  switch i32 %56, label %63 [
    i32 0, label %57
  ]

57:                                               ; preds = %55
  %58 = load i32, ptr %8, align 4
  %59 = load i32, ptr %9, align 4
  %60 = add nsw i32 %58, %59
  %61 = invoke i32 (ptr, ...) @printf(ptr noundef @.str.5, i32 noundef %60)
          to label %62 unwind label %45

62:                                               ; preds = %57
  br label %63

63:                                               ; preds = %62, %55
  %64 = phi i32 [ %56, %55 ], [ 0, %62 ]
  call void @llvm.lifetime.end.p0(ptr %9) #9
  call void @llvm.lifetime.end.p0(ptr %8) #9
  switch i32 %64, label %87 [
    i32 0, label %65
  ]

65:                                               ; preds = %63
  call void @_ZN4task12promise_type11return_voidEv(ptr noundef nonnull align 1 dereferenceable(1) %4)
  br label %72

66:                                               ; preds = %49, %45
  call void @llvm.lifetime.end.p0(ptr %9) #9
  call void @llvm.lifetime.end.p0(ptr %8) #9
  br label %67

67:                                               ; preds = %66
  %68 = load ptr, ptr %10, align 8
  %69 = call ptr @__cxa_begin_catch(ptr %68) #9
  call void @_ZN4task12promise_type19unhandled_exceptionEv(ptr noundef nonnull align 1 dereferenceable(1) %4)
  invoke void @__cxa_end_catch()
          to label %70 unwind label %78

70:                                               ; preds = %67
  br label %71

71:                                               ; preds = %70
  br label %72

72:                                               ; preds = %71, %65
  call void @llvm.lifetime.start.p0(ptr %13) #9
  call void @_ZN4task12promise_type13final_suspendEv(ptr noundef nonnull align 1 dereferenceable(1) %4) #9
  %73 = call noundef i1 @_ZNKSt7__n486113suspend_never11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %13) #9
  br i1 %73, label %83, label %74

74:                                               ; preds = %72
  %75 = call token @llvm.coro.save(ptr null)
  call void @llvm.coro.await.suspend.void(ptr %13, ptr %22, ptr @_Z3runI15resume_now_boolE4taskPKc.__await_suspend_wrapper__final) #9
  %76 = call i8 @llvm.coro.suspend(token %75, i1 true)
  switch i8 %76, label %95 [
    i8 0, label %83
    i8 1, label %77
  ]

77:                                               ; preds = %74
  br label %84

78:                                               ; preds = %67
  %79 = landingpad { ptr, i32 }
          cleanup
  %80 = extractvalue { ptr, i32 } %79, 0
  store ptr %80, ptr %10, align 8
  %81 = extractvalue { ptr, i32 } %79, 1
  store i32 %81, ptr %11, align 4
  call void @llvm.coro.end(ptr null, i1 true, token none)
  %82 = call i1 @llvm.coro.is_in_ramp()
  br i1 %82, label %96, label %102

83:                                               ; preds = %74, %72
  call void @_ZNKSt7__n486113suspend_never12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %13) #9
  br label %84

84:                                               ; preds = %83, %77
  %85 = phi i32 [ 0, %83 ], [ 2, %77 ]
  call void @llvm.lifetime.end.p0(ptr %13) #9
  switch i32 %85, label %87 [
    i32 0, label %86
  ]

86:                                               ; preds = %84
  br label %87

87:                                               ; preds = %86, %84, %63, %30
  %88 = phi i32 [ %31, %30 ], [ %64, %63 ], [ %85, %84 ], [ 0, %86 ]
  call void @llvm.lifetime.end.p0(ptr %4) #9
  call void @llvm.lifetime.end.p0(ptr %3) #9
  %89 = call ptr @llvm.coro.free(token %15, ptr %22)
  %90 = icmp ne ptr %89, null
  br i1 %90, label %91, label %93

91:                                               ; preds = %87
  %92 = call i64 @llvm.coro.size.i64()
  call void @_ZdlPvm(ptr noundef %89, i64 noundef %92) #9
  br label %93

93:                                               ; preds = %87, %91
  switch i32 %88, label %107 [
    i32 0, label %94
    i32 2, label %95
  ]

94:                                               ; preds = %93
  br label %95

95:                                               ; preds = %94, %93, %74, %42, %25
  call void @llvm.coro.end(ptr null, i1 false, token none)
  ret void

96:                                               ; preds = %78
  call void @llvm.lifetime.end.p0(ptr %4) #9
  call void @llvm.lifetime.end.p0(ptr %3) #9
  %97 = call ptr @llvm.coro.free(token %15, ptr %22)
  %98 = icmp ne ptr %97, null
  br i1 %98, label %99, label %101

99:                                               ; preds = %96
  %100 = call i64 @llvm.coro.size.i64()
  call void @_ZdlPvm(ptr noundef %97, i64 noundef %100) #9
  br label %101

101:                                              ; preds = %96, %99
  br label %102

102:                                              ; preds = %101, %78
  %103 = load ptr, ptr %10, align 8
  %104 = load i32, ptr %11, align 4
  %105 = insertvalue { ptr, i32 } poison, ptr %103, 0
  %106 = insertvalue { ptr, i32 } %105, i32 %104, 1
  resume { ptr, i32 } %106

107:                                              ; preds = %93
  unreachable
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZNSt9exceptionC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %0) unnamed_addr #2 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  store ptr getelementptr inbounds inrange(-16, 24) ({ [5 x ptr] }, ptr @_ZTVSt9exception, i32 0, i32 0, i32 2), ptr %3, align 8
  ret void
}

; Function Attrs: nocallback nofree nosync nounwind willreturn memory(argmem: read)
declare token @llvm.coro.id(i32, ptr readnone, ptr readonly captures(none), ptr) #8

; Function Attrs: nounwind
declare i1 @llvm.coro.alloc(token) #9

; Function Attrs: nounwind memory(none)
declare i64 @llvm.coro.size.i64() #10

; Function Attrs: nounwind
declare ptr @llvm.coro.begin(token, ptr writeonly) #9

; Function Attrs: nocallback nofree nosync nounwind willreturn memory(argmem: readwrite)
declare void @llvm.lifetime.start.p0(ptr captures(none)) #11

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZN4task12promise_type17get_return_objectEv(ptr noundef nonnull align 1 dereferenceable(1) %0) #2 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  ret void
}

declare i32 @__gxx_personality_v0(...)

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZN4task12promise_type15initial_suspendEv(ptr noundef nonnull align 1 dereferenceable(1) %0) #2 comdat {
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

; Function Attrs: nomerge nounwind
declare token @llvm.coro.save(ptr) #12

; Function Attrs: alwaysinline mustprogress
define internal void @_Z3runI15resume_now_voidE4taskPKc.__await_suspend_wrapper__init(ptr noundef nonnull %0, ptr noundef %1) #13 {
  %3 = alloca ptr, align 8
  %4 = alloca ptr, align 8
  %5 = alloca %"struct.std::__n4861::coroutine_handle", align 8
  %6 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  store ptr %0, ptr %3, align 8
  store ptr %1, ptr %4, align 8
  %7 = load ptr, ptr %3, align 8
  %8 = load ptr, ptr %4, align 8
  %9 = call i64 @_ZNSt7__n486116coroutine_handleIN4task12promise_typeEE12from_addressEPv(ptr noundef %8) #9
  %10 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %6, i32 0, i32 0
  %11 = inttoptr i64 %9 to ptr
  store ptr %11, ptr %10, align 8
  %12 = call i64 @_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEEcvNS0_IvEEEv(ptr noundef nonnull align 8 dereferenceable(8) %6) #9
  %13 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %5, i32 0, i32 0
  %14 = inttoptr i64 %12 to ptr
  store ptr %14, ptr %13, align 8
  %15 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %5, i32 0, i32 0
  %16 = load ptr, ptr %15, align 8
  call void @_ZNKSt7__n486113suspend_never13await_suspendENS_16coroutine_handleIvEE(ptr noundef nonnull align 1 dereferenceable(1) %7, ptr %16) #9
  ret void
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZNKSt7__n486113suspend_never13await_suspendENS_16coroutine_handleIvEE(ptr noundef nonnull align 1 dereferenceable(1) %0, ptr %1) #2 comdat {
  %3 = alloca %"struct.std::__n4861::coroutine_handle", align 8
  %4 = alloca ptr, align 8
  %5 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %3, i32 0, i32 0
  store ptr %1, ptr %5, align 8
  store ptr %0, ptr %4, align 8
  %6 = load ptr, ptr %4, align 8
  ret void
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local i64 @_ZNSt7__n486116coroutine_handleIN4task12promise_typeEE12from_addressEPv(ptr noundef %0) #2 comdat {
  %2 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %3 = alloca ptr, align 8
  store ptr %0, ptr %3, align 8
  call void @_ZNSt7__n486116coroutine_handleIN4task12promise_typeEEC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %2) #9
  %4 = load ptr, ptr %3, align 8
  %5 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %2, i32 0, i32 0
  store ptr %4, ptr %5, align 8
  %6 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %2, i32 0, i32 0
  %7 = load ptr, ptr %6, align 8
  %8 = ptrtoint ptr %7 to i64
  ret i64 %8
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local i64 @_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEEcvNS0_IvEEEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #2 comdat {
  %2 = alloca %"struct.std::__n4861::coroutine_handle", align 8
  %3 = alloca ptr, align 8
  store ptr %0, ptr %3, align 8
  %4 = load ptr, ptr %3, align 8
  %5 = call noundef ptr @_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEE7addressEv(ptr noundef nonnull align 8 dereferenceable(8) %4) #9
  %6 = call i64 @_ZNSt7__n486116coroutine_handleIvE12from_addressEPv(ptr noundef %5) #9
  %7 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %2, i32 0, i32 0
  %8 = inttoptr i64 %6 to ptr
  store ptr %8, ptr %7, align 8
  %9 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %2, i32 0, i32 0
  %10 = load ptr, ptr %9, align 8
  %11 = ptrtoint ptr %10 to i64
  ret i64 %11
}

declare void @llvm.coro.await.suspend.void(ptr, ptr, ptr)

; Function Attrs: nounwind
declare i8 @llvm.coro.suspend(token, i1) #9

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZNKSt7__n486113suspend_never12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %0) #2 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  ret void
}

; Function Attrs: nocallback nofree nosync nounwind willreturn memory(argmem: readwrite)
declare void @llvm.lifetime.end.p0(ptr captures(none)) #11

declare i32 @printf(ptr noundef, ...) #7

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local noundef i1 @_ZN15resume_now_void11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %0) #2 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  ret i1 false
}

; Function Attrs: alwaysinline mustprogress
define internal void @_Z3runI15resume_now_voidE4taskPKc.__await_suspend_wrapper__await(ptr noundef nonnull %0, ptr noundef %1) #13 {
  %3 = alloca ptr, align 8
  %4 = alloca ptr, align 8
  %5 = alloca %"struct.std::__n4861::coroutine_handle", align 8
  %6 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  store ptr %0, ptr %3, align 8
  store ptr %1, ptr %4, align 8
  %7 = load ptr, ptr %3, align 8
  %8 = load ptr, ptr %4, align 8
  %9 = call i64 @_ZNSt7__n486116coroutine_handleIN4task12promise_typeEE12from_addressEPv(ptr noundef %8) #9
  %10 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %6, i32 0, i32 0
  %11 = inttoptr i64 %9 to ptr
  store ptr %11, ptr %10, align 8
  %12 = call i64 @_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEEcvNS0_IvEEEv(ptr noundef nonnull align 8 dereferenceable(8) %6) #9
  %13 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %5, i32 0, i32 0
  %14 = inttoptr i64 %12 to ptr
  store ptr %14, ptr %13, align 8
  %15 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %5, i32 0, i32 0
  %16 = load ptr, ptr %15, align 8
  call void @_ZN15resume_now_void13await_suspendENSt7__n486116coroutine_handleIvEE(ptr noundef nonnull align 1 dereferenceable(1) %7, ptr %16)
  ret void
}

; Function Attrs: mustprogress noinline optnone uwtable
define linkonce_odr dso_local void @_ZN15resume_now_void13await_suspendENSt7__n486116coroutine_handleIvEE(ptr noundef nonnull align 1 dereferenceable(1) %0, ptr %1) #14 comdat {
  %3 = alloca %"struct.std::__n4861::coroutine_handle", align 8
  %4 = alloca ptr, align 8
  %5 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %3, i32 0, i32 0
  store ptr %1, ptr %5, align 8
  store ptr %0, ptr %4, align 8
  %6 = load ptr, ptr %4, align 8
  call void @_ZNKSt7__n486116coroutine_handleIvE6resumeEv(ptr noundef nonnull align 8 dereferenceable(8) %3)
  call void @_ZL9take_backv()
  ret void
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZN15resume_now_void12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %0) #2 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  ret void
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZN4task12promise_type11return_voidEv(ptr noundef nonnull align 1 dereferenceable(1) %0) #2 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  ret void
}

declare ptr @__cxa_begin_catch(ptr)

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZN4task12promise_type19unhandled_exceptionEv(ptr noundef nonnull align 1 dereferenceable(1) %0) #2 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  ret void
}

declare void @__cxa_end_catch()

; Function Attrs: noinline noreturn nounwind uwtable
define linkonce_odr hidden void @__clang_call_terminate(ptr noundef %0) #15 comdat {
  %2 = call ptr @__cxa_begin_catch(ptr %0) #9
  call void @_ZSt9terminatev() #21
  unreachable
}

declare void @_ZSt9terminatev()

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZN4task12promise_type13final_suspendEv(ptr noundef nonnull align 1 dereferenceable(1) %0) #2 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  ret void
}

; Function Attrs: alwaysinline mustprogress
define internal void @_Z3runI15resume_now_voidE4taskPKc.__await_suspend_wrapper__final(ptr noundef nonnull %0, ptr noundef %1) #13 {
  %3 = alloca ptr, align 8
  %4 = alloca ptr, align 8
  %5 = alloca %"struct.std::__n4861::coroutine_handle", align 8
  %6 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  store ptr %0, ptr %3, align 8
  store ptr %1, ptr %4, align 8
  %7 = load ptr, ptr %3, align 8
  %8 = load ptr, ptr %4, align 8
  %9 = call i64 @_ZNSt7__n486116coroutine_handleIN4task12promise_typeEE12from_addressEPv(ptr noundef %8) #9
  %10 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %6, i32 0, i32 0
  %11 = inttoptr i64 %9 to ptr
  store ptr %11, ptr %10, align 8
  %12 = call i64 @_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEEcvNS0_IvEEEv(ptr noundef nonnull align 8 dereferenceable(8) %6) #9
  %13 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %5, i32 0, i32 0
  %14 = inttoptr i64 %12 to ptr
  store ptr %14, ptr %13, align 8
  %15 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %5, i32 0, i32 0
  %16 = load ptr, ptr %15, align 8
  call void @_ZNKSt7__n486113suspend_never13await_suspendENS_16coroutine_handleIvEE(ptr noundef nonnull align 1 dereferenceable(1) %7, ptr %16) #9
  ret void
}

; Function Attrs: nounwind
declare void @llvm.coro.end(ptr, i1, token) #9

; Function Attrs: nounwind memory(none)
declare i1 @llvm.coro.is_in_ramp() #10

; Function Attrs: nounwind memory(argmem: read)
declare ptr @llvm.coro.free(token, ptr readonly captures(none)) #16

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZNSt7__n486116coroutine_handleIN4task12promise_typeEEC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %0) unnamed_addr #2 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %3, i32 0, i32 0
  store ptr null, ptr %4, align 8
  ret void
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local i64 @_ZNSt7__n486116coroutine_handleIvE12from_addressEPv(ptr noundef %0) #2 comdat {
  %2 = alloca %"struct.std::__n4861::coroutine_handle", align 8
  %3 = alloca ptr, align 8
  store ptr %0, ptr %3, align 8
  call void @_ZNSt7__n486116coroutine_handleIvEC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %2) #9
  %4 = load ptr, ptr %3, align 8
  %5 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %2, i32 0, i32 0
  store ptr %4, ptr %5, align 8
  %6 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %2, i32 0, i32 0
  %7 = load ptr, ptr %6, align 8
  %8 = ptrtoint ptr %7 to i64
  ret i64 %8
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local noundef ptr @_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEE7addressEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #2 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %3, i32 0, i32 0
  %5 = load ptr, ptr %4, align 8
  ret ptr %5
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZNSt7__n486116coroutine_handleIvEC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %0) unnamed_addr #2 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %3, i32 0, i32 0
  store ptr null, ptr %4, align 8
  ret void
}

; Function Attrs: mustprogress noinline optnone uwtable
define linkonce_odr dso_local void @_ZNKSt7__n486116coroutine_handleIvE6resumeEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #14 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %3, i32 0, i32 0
  %5 = load ptr, ptr %4, align 8
  call void @llvm.coro.resume(ptr %5)
  ret void
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define internal void @_ZL9take_backv() #2 {
  %1 = load i64, ptr @_ZL9last_size, align 8
  %2 = call noalias ptr @malloc(i64 noundef %1) #18
  store ptr %2, ptr @_ZL6canary, align 8
  %3 = load ptr, ptr @_ZL6canary, align 8
  %4 = load i64, ptr @_ZL9last_size, align 8
  call void @llvm.memset.p0.i64(ptr align 1 %3, i8 -61, i64 %4, i1 false)
  ret void
}

declare void @llvm.coro.resume(ptr)

; Function Attrs: nocallback nofree nounwind willreturn memory(argmem: write)
declare void @llvm.memset.p0.i64(ptr writeonly captures(none), i8, i64, i1 immarg) #17

; Function Attrs: alwaysinline mustprogress
define internal void @_Z3runI15resume_now_boolE4taskPKc.__await_suspend_wrapper__init(ptr noundef nonnull %0, ptr noundef %1) #13 {
  %3 = alloca ptr, align 8
  %4 = alloca ptr, align 8
  %5 = alloca %"struct.std::__n4861::coroutine_handle", align 8
  %6 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  store ptr %0, ptr %3, align 8
  store ptr %1, ptr %4, align 8
  %7 = load ptr, ptr %3, align 8
  %8 = load ptr, ptr %4, align 8
  %9 = call i64 @_ZNSt7__n486116coroutine_handleIN4task12promise_typeEE12from_addressEPv(ptr noundef %8) #9
  %10 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %6, i32 0, i32 0
  %11 = inttoptr i64 %9 to ptr
  store ptr %11, ptr %10, align 8
  %12 = call i64 @_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEEcvNS0_IvEEEv(ptr noundef nonnull align 8 dereferenceable(8) %6) #9
  %13 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %5, i32 0, i32 0
  %14 = inttoptr i64 %12 to ptr
  store ptr %14, ptr %13, align 8
  %15 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %5, i32 0, i32 0
  %16 = load ptr, ptr %15, align 8
  call void @_ZNKSt7__n486113suspend_never13await_suspendENS_16coroutine_handleIvEE(ptr noundef nonnull align 1 dereferenceable(1) %7, ptr %16) #9
  ret void
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local noundef i1 @_ZN15resume_now_bool11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %0) #2 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  ret i1 false
}

; Function Attrs: alwaysinline mustprogress
define internal noundef i1 @_Z3runI15resume_now_boolE4taskPKc.__await_suspend_wrapper__await(ptr noundef nonnull %0, ptr noundef %1) #13 {
  %3 = alloca ptr, align 8
  %4 = alloca ptr, align 8
  %5 = alloca %"struct.std::__n4861::coroutine_handle", align 8
  %6 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  store ptr %0, ptr %3, align 8
  store ptr %1, ptr %4, align 8
  %7 = load ptr, ptr %3, align 8
  %8 = load ptr, ptr %4, align 8
  %9 = call i64 @_ZNSt7__n486116coroutine_handleIN4task12promise_typeEE12from_addressEPv(ptr noundef %8) #9
  %10 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %6, i32 0, i32 0
  %11 = inttoptr i64 %9 to ptr
  store ptr %11, ptr %10, align 8
  %12 = call i64 @_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEEcvNS0_IvEEEv(ptr noundef nonnull align 8 dereferenceable(8) %6) #9
  %13 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %5, i32 0, i32 0
  %14 = inttoptr i64 %12 to ptr
  store ptr %14, ptr %13, align 8
  %15 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %5, i32 0, i32 0
  %16 = load ptr, ptr %15, align 8
  %17 = call noundef i1 @_ZN15resume_now_bool13await_suspendENSt7__n486116coroutine_handleIvEE(ptr noundef nonnull align 1 dereferenceable(1) %7, ptr %16)
  ret i1 %17
}

; Function Attrs: mustprogress noinline optnone uwtable
define linkonce_odr dso_local noundef i1 @_ZN15resume_now_bool13await_suspendENSt7__n486116coroutine_handleIvEE(ptr noundef nonnull align 1 dereferenceable(1) %0, ptr %1) #14 comdat {
  %3 = alloca %"struct.std::__n4861::coroutine_handle", align 8
  %4 = alloca ptr, align 8
  %5 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %3, i32 0, i32 0
  store ptr %1, ptr %5, align 8
  store ptr %0, ptr %4, align 8
  %6 = load ptr, ptr %4, align 8
  call void @_ZNKSt7__n486116coroutine_handleIvE6resumeEv(ptr noundef nonnull align 8 dereferenceable(8) %3)
  call void @_ZL9take_backv()
  ret i1 true
}

declare i1 @llvm.coro.await.suspend.bool(ptr, ptr, ptr)

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZN15resume_now_bool12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %0) #2 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  ret void
}

; Function Attrs: alwaysinline mustprogress
define internal void @_Z3runI15resume_now_boolE4taskPKc.__await_suspend_wrapper__final(ptr noundef nonnull %0, ptr noundef %1) #13 {
  %3 = alloca ptr, align 8
  %4 = alloca ptr, align 8
  %5 = alloca %"struct.std::__n4861::coroutine_handle", align 8
  %6 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  store ptr %0, ptr %3, align 8
  store ptr %1, ptr %4, align 8
  %7 = load ptr, ptr %3, align 8
  %8 = load ptr, ptr %4, align 8
  %9 = call i64 @_ZNSt7__n486116coroutine_handleIN4task12promise_typeEE12from_addressEPv(ptr noundef %8) #9
  %10 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %6, i32 0, i32 0
  %11 = inttoptr i64 %9 to ptr
  store ptr %11, ptr %10, align 8
  %12 = call i64 @_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEEcvNS0_IvEEEv(ptr noundef nonnull align 8 dereferenceable(8) %6) #9
  %13 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %5, i32 0, i32 0
  %14 = inttoptr i64 %12 to ptr
  store ptr %14, ptr %13, align 8
  %15 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %5, i32 0, i32 0
  %16 = load ptr, ptr %15, align 8
  call void @_ZNKSt7__n486113suspend_never13await_suspendENS_16coroutine_handleIvEE(ptr noundef nonnull align 1 dereferenceable(1) %7, ptr %16) #9
  ret void
}

attributes #0 = { mustprogress nobuiltin noinline optnone allocsize(0) uwtable "frame-pointer"="non-leaf-no-reserve" "no-trapping-math"="true" "stack-protector-buffer-size"="8" }
attributes #1 = { nounwind allocsize(0) "frame-pointer"="non-leaf-no-reserve" "no-trapping-math"="true" "stack-protector-buffer-size"="8" }
attributes #2 = { mustprogress noinline nounwind optnone uwtable "frame-pointer"="non-leaf-no-reserve" "no-trapping-math"="true" "stack-protector-buffer-size"="8" }
attributes #3 = { nounwind "frame-pointer"="non-leaf-no-reserve" "no-trapping-math"="true" "stack-protector-buffer-size"="8" }
attributes #4 = { mustprogress nobuiltin noinline nounwind optnone uwtable "frame-pointer"="non-leaf-no-reserve" "no-trapping-math"="true" "stack-protector-buffer-size"="8" }
attributes #5 = { mustprogress noinline norecurse optnone uwtable "frame-pointer"="non-leaf-no-reserve" "no-trapping-math"="true" "stack-protector-buffer-size"="8" }
attributes #6 = { mustprogress noinline optnone presplitcoroutine uwtable "frame-pointer"="non-leaf-no-reserve" "no-trapping-math"="true" "stack-protector-buffer-size"="8" }
attributes #7 = { "frame-pointer"="non-leaf-no-reserve" "no-trapping-math"="true" "stack-protector-buffer-size"="8" }
attributes #8 = { nocallback nofree nosync nounwind willreturn memory(argmem: read) }
attributes #9 = { nounwind }
attributes #10 = { nounwind memory(none) }
attributes #11 = { nocallback nofree nosync nounwind willreturn memory(argmem: readwrite) }
attributes #12 = { nomerge nounwind }
attributes #13 = { alwaysinline mustprogress "sample-profile-suffix-elision-policy"="selected" }
attributes #14 = { mustprogress noinline optnone uwtable "frame-pointer"="non-leaf-no-reserve" "no-trapping-math"="true" "stack-protector-buffer-size"="8" }
attributes #15 = { noinline noreturn nounwind uwtable "frame-pointer"="non-leaf-no-reserve" "no-trapping-math"="true" "stack-protector-buffer-size"="8" }
attributes #16 = { nounwind memory(argmem: read) }
attributes #17 = { nocallback nofree nounwind willreturn memory(argmem: write) }
attributes #18 = { nounwind allocsize(0) }
attributes #19 = { noreturn }
attributes #20 = { allocsize(0) }
attributes #21 = { noreturn nounwind }

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
