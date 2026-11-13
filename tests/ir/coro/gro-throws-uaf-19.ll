; clang++-19 -std=c++20 -O0 -S -emit-llvm -Xclang -disable-llvm-passes
; (aarch64-unknown-linux-gnu; the target triple and datalayout removed) of:
;
; // Design 0005 §1.4: get_return_object throws in the ramp, whose unwind
; // path frees the frame and then resumes the exception. LLVM's split keeps
; // the exception and selector slots in the frame and reads them after the
; // free (here the promise's operator delete fills the frame with 0x5a
; // first, so that program crashes); hod keeps them on the machine stack
; // (design 0005 §3.4 rule 3). The final await_suspend returns a handle:
; // symmetric transfer, in the clone and in the ramp.
; #include <coroutine>
; #include <cstdio>
; #include <cstring>
; #include <new>
; #include <stdexcept>
; bool fail_gro = false;
; struct task {
;   struct promise_type {
;     std::coroutine_handle<> cont;
;     static void* operator new(std::size_t n) { return ::operator new(n); }
;     static void operator delete(void* p, std::size_t n) {
;       std::memset(p, 0x5a, n);
;       ::operator delete(p);
;     }
;     task get_return_object() {
;       if (fail_gro) throw std::runtime_error("gro");
;       return task{std::coroutine_handle<promise_type>::from_promise(*this)};
;     }
;     std::suspend_always initial_suspend() noexcept { return {}; }
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
;     void unhandled_exception() { throw; }
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
; struct Dtor {
;   int v;
;   ~Dtor() { std::printf("dtor %d\n", v); }
; };
; task leaf(int i) {
;   Dtor d{i};
;   if (i == 3) throw std::runtime_error("boom");
;   co_return;
; }
; task chain(int n) {
;   Dtor d{100 + n};
;   if (n == 0) {
;     co_await leaf(3);
;     co_return;
;   }
;   for (int i = 0; i < 2; ++i) co_await leaf(i);
;   co_await chain(n - 1);
; }
; int main() {
;   fail_gro = true;
;   try {
;     leaf(0);
;   } catch (std::exception& e) {
;     std::printf("caught %s\n", e.what());
;   }
;   fail_gro = false;
;   task t = chain(2);
;   try {
;     t.h.resume();
;   } catch (std::exception& e) {
;     std::printf("caught %s done=%d\n", e.what(), (int)t.h.done());
;   }
;   return 0;
; }
;
;@ covers: spec/ir.md §5.5 intrinsics (symmetric transfer; spec/codegen.md §7.6)
;@ input: pre-opt
;@ dialect: 19
;@ link: c++
;@ unsupported(x86_64): *
;@ oracle-skip(llvm=19): LLVM reads exception slots from the frame after the ramp freed it (design 0005 1.4), and this program poisons the freed frame
;@ stdout: caught gro
;@ stdout: dtor 0
;@ stdout: dtor 1
;@ stdout: dtor 0
;@ stdout: dtor 1
;@ stdout: dtor 3
;@ stdout: caught boom done=0
;@ stdout: dtor 100
;@ stdout: dtor 101
;@ stdout: dtor 102

source_filename = "gro-throws-uaf.cpp"

%struct.task = type { %"struct.std::__n4861::coroutine_handle" }
%"struct.std::__n4861::coroutine_handle" = type { ptr }
%"struct.task::promise_type" = type { %"struct.std::__n4861::coroutine_handle.0" }
%"struct.std::__n4861::coroutine_handle.0" = type { ptr }
%"struct.std::__n4861::suspend_always" = type { i8 }
%struct.Dtor = type { i32 }
%"struct.task::promise_type::fin" = type { i8 }
%"struct.std::__n4861::coroutine_handle.1" = type { ptr }

$_ZN4task12promise_typenwEm = comdat any

$_ZN4task12promise_typeC2Ev = comdat any

$_ZN4task12promise_type17get_return_objectEv = comdat any

$_ZN4task12promise_type15initial_suspendEv = comdat any

$_ZNKSt7__n486114suspend_always11await_readyEv = comdat any

$_ZNKSt7__n486114suspend_always13await_suspendENS_16coroutine_handleIvEE = comdat any

$_ZNSt7__n486116coroutine_handleIN4task12promise_typeEE12from_addressEPv = comdat any

$_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEEcvNS0_IvEEEv = comdat any

$_ZNKSt7__n486114suspend_always12await_resumeEv = comdat any

$_ZN4task12promise_type11return_voidEv = comdat any

$_ZN4DtorD2Ev = comdat any

$_ZN4task12promise_type19unhandled_exceptionEv = comdat any

$__clang_call_terminate = comdat any

$_ZN4task12promise_type13final_suspendEv = comdat any

$_ZN4task12promise_type3fin11await_readyEv = comdat any

$_ZN4task12promise_type3fin13await_suspendENSt7__n486116coroutine_handleIS0_EE = comdat any

$_ZNKSt7__n486116coroutine_handleIvE7addressEv = comdat any

$_ZN4task12promise_type3fin12await_resumeEv = comdat any

$_ZN4task12promise_typedlEPvm = comdat any

$_ZN4task11await_readyEv = comdat any

$_ZN4task13await_suspendENSt7__n486116coroutine_handleIvEE = comdat any

$_ZN4task12await_resumeEv = comdat any

$_ZN4taskD2Ev = comdat any

$_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEE6resumeEv = comdat any

$_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEE4doneEv = comdat any

$_ZNSt7__n486116coroutine_handleIvEC2Ev = comdat any

$_ZNSt7__n486116coroutine_handleIN4task12promise_typeEE12from_promiseERS2_ = comdat any

$_ZNSt7__n486116coroutine_handleIN4task12promise_typeEEC2Ev = comdat any

$_ZNSt7__n486116coroutine_handleIvE12from_addressEPv = comdat any

$_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEE7addressEv = comdat any

$_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEE7promiseEv = comdat any

$_ZNKSt7__n486116coroutine_handleIvEcvbEv = comdat any

$_ZNSt7__n486114noop_coroutineEv = comdat any

$_ZNKSt7__n486116coroutine_handleINS_22noop_coroutine_promiseEEcvNS0_IvEEEv = comdat any

$_ZNSt7__n486116coroutine_handleINS_22noop_coroutine_promiseEEC2Ev = comdat any

$_ZNSt7__n486116coroutine_handleINS_22noop_coroutine_promiseEE7__frame22__dummy_resume_destroyEv = comdat any

$_ZNKSt7__n486116coroutine_handleINS_22noop_coroutine_promiseEE7addressEv = comdat any

$_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEEcvbEv = comdat any

$_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEE7destroyEv = comdat any

$_ZNSt7__n486116coroutine_handleINS_22noop_coroutine_promiseEE5_S_frE = comdat any

@fail_gro = dso_local global i8 0, align 1
@.str = private unnamed_addr constant [5 x i8] c"boom\00", align 1
@_ZTISt13runtime_error = external constant ptr
@_ZTISt9exception = external constant ptr
@.str.1 = private unnamed_addr constant [11 x i8] c"caught %s\0A\00", align 1
@.str.2 = private unnamed_addr constant [19 x i8] c"caught %s done=%d\0A\00", align 1
@.str.3 = private unnamed_addr constant [4 x i8] c"gro\00", align 1
@.str.4 = private unnamed_addr constant [9 x i8] c"dtor %d\0A\00", align 1
@_ZNSt7__n486116coroutine_handleINS_22noop_coroutine_promiseEE5_S_frE = linkonce_odr dso_local global { ptr, ptr, [8 x i8] } { ptr @_ZNSt7__n486116coroutine_handleINS_22noop_coroutine_promiseEE7__frame22__dummy_resume_destroyEv, ptr @_ZNSt7__n486116coroutine_handleINS_22noop_coroutine_promiseEE7__frame22__dummy_resume_destroyEv, [8 x i8] undef }, comdat, align 8

; Function Attrs: mustprogress noinline optnone presplitcoroutine uwtable
define dso_local void @_Z4leafi(ptr dead_on_unwind noalias writable sret(%struct.task) align 8 %0, i32 noundef %1) #0 personality ptr @__gxx_personality_v0 {
  %3 = alloca ptr, align 8
  %4 = alloca i32, align 4
  %5 = alloca i32, align 4
  %6 = alloca %"struct.task::promise_type", align 8
  %7 = alloca ptr, align 8
  %8 = alloca i32, align 4
  %9 = alloca %"struct.std::__n4861::suspend_always", align 1
  %10 = alloca %"struct.std::__n4861::suspend_always", align 1
  %11 = alloca %struct.Dtor, align 4
  %12 = alloca %"struct.task::promise_type::fin", align 1
  %13 = alloca %"struct.task::promise_type::fin", align 1
  store ptr %0, ptr %3, align 8
  store i32 %1, ptr %4, align 4
  %14 = bitcast ptr %6 to ptr
  %15 = call token @llvm.coro.id(i32 16, ptr %14, ptr null, ptr null)
  %16 = call i1 @llvm.coro.alloc(token %15)
  br i1 %16, label %17, label %20

17:                                               ; preds = %2
  %18 = call i64 @llvm.coro.size.i64()
  %19 = call noundef ptr @_ZN4task12promise_typenwEm(i64 noundef %18)
  br label %20

20:                                               ; preds = %17, %2
  %21 = phi ptr [ null, %2 ], [ %19, %17 ]
  %22 = call ptr @llvm.coro.begin(token %15, ptr %21)
  call void @llvm.lifetime.start.p0(i64 4, ptr %5) #2
  %23 = load i32, ptr %4, align 4
  store i32 %23, ptr %5, align 4
  call void @llvm.lifetime.start.p0(i64 8, ptr %6) #2
  call void @_ZN4task12promise_typeC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %6) #2
  invoke void @_ZN4task12promise_type17get_return_objectEv(ptr dead_on_unwind writable sret(%struct.task) align 8 %0, ptr noundef nonnull align 8 dereferenceable(8) %6)
          to label %24 unwind label %30

24:                                               ; preds = %20
  call void @llvm.lifetime.start.p0(i64 1, ptr %9) #2
  call void @_ZN4task12promise_type15initial_suspendEv(ptr noundef nonnull align 8 dereferenceable(8) %6) #2
  %25 = call noundef i1 @_ZNKSt7__n486114suspend_always11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %9) #2
  br i1 %25, label %34, label %26

26:                                               ; preds = %24
  %27 = call token @llvm.coro.save(ptr null)
  call void @llvm.coro.await.suspend.void(ptr %9, ptr %22, ptr @_Z4leafi.__await_suspend_wrapper__init) #2
  %28 = call i8 @llvm.coro.suspend(token %27, i1 false)
  switch i8 %28, label %94 [
    i8 0, label %34
    i8 1, label %29
  ]

29:                                               ; preds = %26
  br label %35

30:                                               ; preds = %20
  %31 = landingpad { ptr, i32 }
          cleanup
  %32 = extractvalue { ptr, i32 } %31, 0
  store ptr %32, ptr %7, align 8
  %33 = extractvalue { ptr, i32 } %31, 1
  store i32 %33, ptr %8, align 4
  br label %99

34:                                               ; preds = %26, %24
  call void @_ZNKSt7__n486114suspend_always12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %9) #2
  br label %35

35:                                               ; preds = %34, %29
  %36 = phi i32 [ 0, %34 ], [ 2, %29 ]
  call void @llvm.lifetime.end.p0(i64 1, ptr %9) #2
  switch i32 %36, label %86 [
    i32 0, label %37
  ]

37:                                               ; preds = %35
  call void @llvm.lifetime.start.p0(i64 4, ptr %11) #2
  %38 = getelementptr inbounds %struct.Dtor, ptr %11, i32 0, i32 0
  %39 = load i32, ptr %5, align 4
  store i32 %39, ptr %38, align 4
  %40 = load i32, ptr %5, align 4
  %41 = icmp eq i32 %40, 3
  br i1 %41, label %42, label %53

42:                                               ; preds = %37
  %43 = call ptr @__cxa_allocate_exception(i64 16) #2
  invoke void @_ZNSt13runtime_errorC1EPKc(ptr noundef nonnull align 8 dereferenceable(16) %43, ptr noundef @.str)
          to label %44 unwind label %45

44:                                               ; preds = %42
  invoke void @__cxa_throw(ptr %43, ptr @_ZTISt13runtime_error, ptr @_ZNSt13runtime_errorD1Ev) #19
          to label %113 unwind label %49

45:                                               ; preds = %42
  %46 = landingpad { ptr, i32 }
          catch ptr null
  %47 = extractvalue { ptr, i32 } %46, 0
  store ptr %47, ptr %7, align 8
  %48 = extractvalue { ptr, i32 } %46, 1
  store i32 %48, ptr %8, align 4
  call void @__cxa_free_exception(ptr %43) #2
  br label %55

49:                                               ; preds = %53, %44
  %50 = landingpad { ptr, i32 }
          catch ptr null
  %51 = extractvalue { ptr, i32 } %50, 0
  store ptr %51, ptr %7, align 8
  %52 = extractvalue { ptr, i32 } %50, 1
  store i32 %52, ptr %8, align 4
  br label %55

53:                                               ; preds = %37
  invoke void @_ZN4task12promise_type11return_voidEv(ptr noundef nonnull align 8 dereferenceable(8) %6)
          to label %54 unwind label %49

54:                                               ; preds = %53
  call void @_ZN4DtorD2Ev(ptr noundef nonnull align 4 dereferenceable(4) %11) #2
  call void @llvm.lifetime.end.p0(i64 4, ptr %11) #2
  br label %62

55:                                               ; preds = %49, %45
  call void @_ZN4DtorD2Ev(ptr noundef nonnull align 4 dereferenceable(4) %11) #2
  call void @llvm.lifetime.end.p0(i64 4, ptr %11) #2
  br label %56

56:                                               ; preds = %55
  %57 = load ptr, ptr %7, align 8
  %58 = call ptr @__cxa_begin_catch(ptr %57) #2
  invoke void @_ZN4task12promise_type19unhandled_exceptionEv(ptr noundef nonnull align 8 dereferenceable(8) %6)
          to label %59 unwind label %69

59:                                               ; preds = %56
  invoke void @__cxa_end_catch()
          to label %60 unwind label %73

60:                                               ; preds = %59
  br label %61

61:                                               ; preds = %60
  br label %62

62:                                               ; preds = %61, %54
  call void @llvm.lifetime.start.p0(i64 1, ptr %12) #2
  call void @_ZN4task12promise_type13final_suspendEv(ptr noundef nonnull align 8 dereferenceable(8) %6) #2
  %63 = call noundef i1 @_ZN4task12promise_type3fin11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %12) #2
  br i1 %63, label %82, label %64

64:                                               ; preds = %62
  %65 = call token @llvm.coro.save(ptr null)
  invoke void @llvm.coro.await.suspend.handle(ptr %12, ptr %22, ptr @_Z4leafi.__await_suspend_wrapper__final)
          to label %66 unwind label %78

66:                                               ; preds = %64
  %67 = call i8 @llvm.coro.suspend(token %65, i1 true)
  switch i8 %67, label %94 [
    i8 0, label %82
    i8 1, label %68
  ]

68:                                               ; preds = %66
  br label %83

69:                                               ; preds = %56
  %70 = landingpad { ptr, i32 }
          cleanup
  %71 = extractvalue { ptr, i32 } %70, 0
  store ptr %71, ptr %7, align 8
  %72 = extractvalue { ptr, i32 } %70, 1
  store i32 %72, ptr %8, align 4
  invoke void @__cxa_end_catch()
          to label %77 unwind label %110

73:                                               ; preds = %59
  %74 = landingpad { ptr, i32 }
          cleanup
  %75 = extractvalue { ptr, i32 } %74, 0
  store ptr %75, ptr %7, align 8
  %76 = extractvalue { ptr, i32 } %74, 1
  store i32 %76, ptr %8, align 4
  br label %96

77:                                               ; preds = %69
  br label %96

78:                                               ; preds = %64
  %79 = landingpad { ptr, i32 }
          cleanup
  %80 = extractvalue { ptr, i32 } %79, 0
  store ptr %80, ptr %7, align 8
  %81 = extractvalue { ptr, i32 } %79, 1
  store i32 %81, ptr %8, align 4
  call void @llvm.lifetime.end.p0(i64 1, ptr %12) #2
  br label %96

82:                                               ; preds = %66, %62
  call void @_ZN4task12promise_type3fin12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %12) #2
  br label %83

83:                                               ; preds = %82, %68
  %84 = phi i32 [ 0, %82 ], [ 2, %68 ]
  call void @llvm.lifetime.end.p0(i64 1, ptr %12) #2
  switch i32 %84, label %86 [
    i32 0, label %85
  ]

85:                                               ; preds = %83
  br label %86

86:                                               ; preds = %85, %83, %35
  %87 = phi i32 [ %36, %35 ], [ %84, %83 ], [ 0, %85 ]
  call void @llvm.lifetime.end.p0(i64 8, ptr %6) #2
  call void @llvm.lifetime.end.p0(i64 4, ptr %5) #2
  %88 = call ptr @llvm.coro.free(token %15, ptr %22)
  %89 = icmp ne ptr %88, null
  br i1 %89, label %90, label %92

90:                                               ; preds = %86
  %91 = call i64 @llvm.coro.size.i64()
  call void @_ZN4task12promise_typedlEPvm(ptr noundef %88, i64 noundef %91) #2
  br label %92

92:                                               ; preds = %86, %90
  switch i32 %87, label %113 [
    i32 0, label %93
    i32 2, label %94
  ]

93:                                               ; preds = %92
  br label %94

94:                                               ; preds = %93, %92, %66, %26
  %95 = call i1 @llvm.coro.end(ptr null, i1 false, token none)
  ret void

96:                                               ; preds = %78, %77, %73
  %97 = call i1 @llvm.coro.end(ptr null, i1 true, token none)
  br i1 %97, label %105, label %98

98:                                               ; preds = %96
  br label %99

99:                                               ; preds = %98, %30
  call void @llvm.lifetime.end.p0(i64 8, ptr %6) #2
  call void @llvm.lifetime.end.p0(i64 4, ptr %5) #2
  %100 = call ptr @llvm.coro.free(token %15, ptr %22)
  %101 = icmp ne ptr %100, null
  br i1 %101, label %102, label %104

102:                                              ; preds = %99
  %103 = call i64 @llvm.coro.size.i64()
  call void @_ZN4task12promise_typedlEPvm(ptr noundef %100, i64 noundef %103) #2
  br label %104

104:                                              ; preds = %99, %102
  br label %105

105:                                              ; preds = %104, %96
  %106 = load ptr, ptr %7, align 8
  %107 = load i32, ptr %8, align 4
  %108 = insertvalue { ptr, i32 } poison, ptr %106, 0
  %109 = insertvalue { ptr, i32 } %108, i32 %107, 1
  resume { ptr, i32 } %109

110:                                              ; preds = %69
  %111 = landingpad { ptr, i32 }
          catch ptr null
  %112 = extractvalue { ptr, i32 } %111, 0
  call void @__clang_call_terminate(ptr %112) #20
  unreachable

113:                                              ; preds = %92, %44
  unreachable
}

; Function Attrs: nocallback nofree nosync nounwind willreturn memory(argmem: read)
declare token @llvm.coro.id(i32, ptr readnone, ptr nocapture readonly, ptr) #1

; Function Attrs: nounwind
declare i1 @llvm.coro.alloc(token) #2

; Function Attrs: mustprogress noinline optnone uwtable
define linkonce_odr dso_local noundef ptr @_ZN4task12promise_typenwEm(i64 noundef %0) #3 comdat {
  %2 = alloca i64, align 8
  store i64 %0, ptr %2, align 8
  %3 = load i64, ptr %2, align 8
  %4 = call noalias noundef nonnull ptr @_Znwm(i64 noundef %3) #21
  ret ptr %4
}

; Function Attrs: nounwind memory(none)
declare i64 @llvm.coro.size.i64() #4

; Function Attrs: nounwind
declare ptr @llvm.coro.begin(token, ptr writeonly) #2

; Function Attrs: nocallback nofree nosync nounwind willreturn memory(argmem: readwrite)
declare void @llvm.lifetime.start.p0(i64 immarg, ptr nocapture) #5

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZN4task12promise_typeC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %0) unnamed_addr #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds %"struct.task::promise_type", ptr %3, i32 0, i32 0
  call void @_ZNSt7__n486116coroutine_handleIvEC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %4) #2
  ret void
}

; Function Attrs: mustprogress noinline optnone uwtable
define linkonce_odr dso_local void @_ZN4task12promise_type17get_return_objectEv(ptr dead_on_unwind noalias writable sret(%struct.task) align 8 %0, ptr noundef nonnull align 8 dereferenceable(8) %1) #3 comdat personality ptr @__gxx_personality_v0 {
  %3 = alloca ptr, align 8
  %4 = alloca ptr, align 8
  %5 = alloca ptr, align 8
  %6 = alloca i32, align 4
  store ptr %0, ptr %3, align 8
  store ptr %1, ptr %4, align 8
  %7 = load ptr, ptr %4, align 8
  %8 = load i8, ptr @fail_gro, align 1
  %9 = trunc i8 %8 to i1
  br i1 %9, label %10, label %17

10:                                               ; preds = %2
  %11 = call ptr @__cxa_allocate_exception(i64 16) #2
  invoke void @_ZNSt13runtime_errorC1EPKc(ptr noundef nonnull align 8 dereferenceable(16) %11, ptr noundef @.str.3)
          to label %12 unwind label %13

12:                                               ; preds = %10
  call void @__cxa_throw(ptr %11, ptr @_ZTISt13runtime_error, ptr @_ZNSt13runtime_errorD1Ev) #19
  unreachable

13:                                               ; preds = %10
  %14 = landingpad { ptr, i32 }
          cleanup
  %15 = extractvalue { ptr, i32 } %14, 0
  store ptr %15, ptr %5, align 8
  %16 = extractvalue { ptr, i32 } %14, 1
  store i32 %16, ptr %6, align 4
  call void @__cxa_free_exception(ptr %11) #2
  br label %22

17:                                               ; preds = %2
  %18 = getelementptr inbounds %struct.task, ptr %0, i32 0, i32 0
  %19 = call i64 @_ZNSt7__n486116coroutine_handleIN4task12promise_typeEE12from_promiseERS2_(ptr noundef nonnull align 8 dereferenceable(8) %7)
  %20 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %18, i32 0, i32 0
  %21 = inttoptr i64 %19 to ptr
  store ptr %21, ptr %20, align 8
  ret void

22:                                               ; preds = %13
  %23 = load ptr, ptr %5, align 8
  %24 = load i32, ptr %6, align 4
  %25 = insertvalue { ptr, i32 } poison, ptr %23, 0
  %26 = insertvalue { ptr, i32 } %25, i32 %24, 1
  resume { ptr, i32 } %26
}

declare i32 @__gxx_personality_v0(...)

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZN4task12promise_type15initial_suspendEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #6 comdat {
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
define private void @_Z4leafi.__await_suspend_wrapper__init(ptr noundef nonnull %0, ptr noundef %1) #8 {
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

; Function Attrs: mustprogress noinline nounwind optnone uwtable
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

declare ptr @__cxa_allocate_exception(i64)

declare void @_ZNSt13runtime_errorC1EPKc(ptr noundef nonnull align 8 dereferenceable(16), ptr noundef) unnamed_addr #9

declare void @__cxa_free_exception(ptr)

; Function Attrs: nounwind
declare void @_ZNSt13runtime_errorD1Ev(ptr noundef nonnull align 8 dereferenceable(16)) unnamed_addr #10

declare void @__cxa_throw(ptr, ptr, ptr)

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZN4task12promise_type11return_voidEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  ret void
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZN4DtorD2Ev(ptr noundef nonnull align 4 dereferenceable(4) %0) unnamed_addr #6 comdat personality ptr @__gxx_personality_v0 {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds %struct.Dtor, ptr %3, i32 0, i32 0
  %5 = load i32, ptr %4, align 4
  %6 = invoke i32 (ptr, ...) @printf(ptr noundef @.str.4, i32 noundef %5)
          to label %7 unwind label %8

7:                                                ; preds = %1
  ret void

8:                                                ; preds = %1
  %9 = landingpad { ptr, i32 }
          catch ptr null
  %10 = extractvalue { ptr, i32 } %9, 0
  call void @__clang_call_terminate(ptr %10) #20
  unreachable
}

declare ptr @__cxa_begin_catch(ptr)

; Function Attrs: mustprogress noinline optnone uwtable
define linkonce_odr dso_local void @_ZN4task12promise_type19unhandled_exceptionEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #3 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  call void @__cxa_rethrow() #19
  unreachable
}

declare void @__cxa_end_catch()

; Function Attrs: noinline noreturn nounwind uwtable
define linkonce_odr hidden void @__clang_call_terminate(ptr noundef %0) #11 comdat {
  %2 = call ptr @__cxa_begin_catch(ptr %0) #2
  call void @_ZSt9terminatev() #20
  unreachable
}

declare void @_ZSt9terminatev()

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZN4task12promise_type13final_suspendEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  ret void
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local noundef i1 @_ZN4task12promise_type3fin11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %0) #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  ret i1 false
}

; Function Attrs: alwaysinline mustprogress
define private noundef ptr @_Z4leafi.__await_suspend_wrapper__final(ptr noundef nonnull %0, ptr noundef %1) #8 {
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

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local i64 @_ZN4task12promise_type3fin13await_suspendENSt7__n486116coroutine_handleIS0_EE(ptr noundef nonnull align 1 dereferenceable(1) %0, i64 %1) #6 comdat personality ptr @__gxx_personality_v0 {
  %3 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %4 = alloca %"struct.std::__n4861::coroutine_handle", align 8
  %5 = alloca ptr, align 8
  %6 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %7 = alloca %"struct.std::__n4861::coroutine_handle.1", align 8
  %8 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %4, i32 0, i32 0
  %9 = inttoptr i64 %1 to ptr
  store ptr %9, ptr %8, align 8
  store ptr %0, ptr %5, align 8
  %10 = load ptr, ptr %5, align 8
  %11 = invoke noundef nonnull align 8 dereferenceable(8) ptr @_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEE7promiseEv(ptr noundef nonnull align 8 dereferenceable(8) %4)
          to label %12 unwind label %27

12:                                               ; preds = %2
  %13 = getelementptr inbounds %"struct.task::promise_type", ptr %11, i32 0, i32 0
  call void @llvm.memcpy.p0.p0.i64(ptr align 8 %6, ptr align 8 %13, i64 8, i1 false)
  %14 = call noundef i1 @_ZNKSt7__n486116coroutine_handleIvEcvbEv(ptr noundef nonnull align 8 dereferenceable(8) %6) #2
  br i1 %14, label %15, label %16

15:                                               ; preds = %12
  call void @llvm.memcpy.p0.p0.i64(ptr align 8 %3, ptr align 8 %6, i64 8, i1 false)
  br label %23

16:                                               ; preds = %12
  %17 = call i64 @_ZNSt7__n486114noop_coroutineEv() #2
  %18 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.1", ptr %7, i32 0, i32 0
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
  call void @__clang_call_terminate(ptr %29) #20
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
define linkonce_odr dso_local void @_ZN4task12promise_type3fin12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %0) #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  ret void
}

; Function Attrs: nounwind
declare i1 @llvm.coro.end(ptr, i1, token) #2

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZN4task12promise_typedlEPvm(ptr noundef %0, i64 noundef %1) #6 comdat {
  %3 = alloca ptr, align 8
  %4 = alloca i64, align 8
  store ptr %0, ptr %3, align 8
  store i64 %1, ptr %4, align 8
  %5 = load ptr, ptr %3, align 8
  %6 = load i64, ptr %4, align 8
  call void @llvm.memset.p0.i64(ptr align 1 %5, i8 90, i64 %6, i1 false)
  %7 = load ptr, ptr %3, align 8
  call void @_ZdlPv(ptr noundef %7) #2
  ret void
}

; Function Attrs: nounwind memory(argmem: read)
declare ptr @llvm.coro.free(token, ptr nocapture readonly) #12

; Function Attrs: mustprogress noinline optnone presplitcoroutine uwtable
define dso_local void @_Z5chaini(ptr dead_on_unwind noalias writable sret(%struct.task) align 8 %0, i32 noundef %1) #0 personality ptr @__gxx_personality_v0 {
  %3 = alloca ptr, align 8
  %4 = alloca i32, align 4
  %5 = alloca i32, align 4
  %6 = alloca %"struct.task::promise_type", align 8
  %7 = alloca ptr, align 8
  %8 = alloca i32, align 4
  %9 = alloca %"struct.std::__n4861::suspend_always", align 1
  %10 = alloca %"struct.std::__n4861::suspend_always", align 1
  %11 = alloca %struct.Dtor, align 4
  %12 = alloca %struct.task, align 8
  %13 = alloca i32, align 4
  %14 = alloca %struct.task, align 8
  %15 = alloca %struct.task, align 8
  %16 = alloca %"struct.task::promise_type::fin", align 1
  %17 = alloca %"struct.task::promise_type::fin", align 1
  store ptr %0, ptr %3, align 8
  store i32 %1, ptr %4, align 4
  %18 = bitcast ptr %6 to ptr
  %19 = call token @llvm.coro.id(i32 16, ptr %18, ptr null, ptr null)
  %20 = call i1 @llvm.coro.alloc(token %19)
  br i1 %20, label %21, label %24

21:                                               ; preds = %2
  %22 = call i64 @llvm.coro.size.i64()
  %23 = call noundef ptr @_ZN4task12promise_typenwEm(i64 noundef %22)
  br label %24

24:                                               ; preds = %21, %2
  %25 = phi ptr [ null, %2 ], [ %23, %21 ]
  %26 = call ptr @llvm.coro.begin(token %19, ptr %25)
  call void @llvm.lifetime.start.p0(i64 4, ptr %5) #2
  %27 = load i32, ptr %4, align 4
  store i32 %27, ptr %5, align 4
  call void @llvm.lifetime.start.p0(i64 8, ptr %6) #2
  call void @_ZN4task12promise_typeC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %6) #2
  invoke void @_ZN4task12promise_type17get_return_objectEv(ptr dead_on_unwind writable sret(%struct.task) align 8 %0, ptr noundef nonnull align 8 dereferenceable(8) %6)
          to label %28 unwind label %34

28:                                               ; preds = %24
  call void @llvm.lifetime.start.p0(i64 1, ptr %9) #2
  call void @_ZN4task12promise_type15initial_suspendEv(ptr noundef nonnull align 8 dereferenceable(8) %6) #2
  %29 = call noundef i1 @_ZNKSt7__n486114suspend_always11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %9) #2
  br i1 %29, label %38, label %30

30:                                               ; preds = %28
  %31 = call token @llvm.coro.save(ptr null)
  call void @llvm.coro.await.suspend.void(ptr %9, ptr %26, ptr @_Z5chaini.__await_suspend_wrapper__init) #2
  %32 = call i8 @llvm.coro.suspend(token %31, i1 false)
  switch i8 %32, label %181 [
    i8 0, label %38
    i8 1, label %33
  ]

33:                                               ; preds = %30
  br label %39

34:                                               ; preds = %24
  %35 = landingpad { ptr, i32 }
          cleanup
  %36 = extractvalue { ptr, i32 } %35, 0
  store ptr %36, ptr %7, align 8
  %37 = extractvalue { ptr, i32 } %35, 1
  store i32 %37, ptr %8, align 4
  br label %186

38:                                               ; preds = %30, %28
  call void @_ZNKSt7__n486114suspend_always12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %9) #2
  br label %39

39:                                               ; preds = %38, %33
  %40 = phi i32 [ 0, %38 ], [ 2, %33 ]
  call void @llvm.lifetime.end.p0(i64 1, ptr %9) #2
  switch i32 %40, label %173 [
    i32 0, label %41
  ]

41:                                               ; preds = %39
  call void @llvm.lifetime.start.p0(i64 4, ptr %11) #2
  %42 = getelementptr inbounds %struct.Dtor, ptr %11, i32 0, i32 0
  %43 = load i32, ptr %5, align 4
  %44 = add nsw i32 100, %43
  store i32 %44, ptr %42, align 4
  %45 = load i32, ptr %5, align 4
  %46 = icmp eq i32 %45, 0
  br i1 %46, label %47, label %75

47:                                               ; preds = %41
  call void @llvm.lifetime.start.p0(i64 8, ptr %12) #2
  invoke void @_Z4leafi(ptr dead_on_unwind writable sret(%struct.task) align 8 %12, i32 noundef 3)
          to label %48 unwind label %56

48:                                               ; preds = %47
  %49 = invoke noundef i1 @_ZN4task11await_readyEv(ptr noundef nonnull align 8 dereferenceable(8) %12)
          to label %50 unwind label %60

50:                                               ; preds = %48
  br i1 %49, label %64, label %51

51:                                               ; preds = %50
  %52 = call token @llvm.coro.save(ptr null)
  invoke void @llvm.coro.await.suspend.handle(ptr %12, ptr %26, ptr @_Z5chaini.__await_suspend_wrapper__await)
          to label %53 unwind label %60

53:                                               ; preds = %51
  %54 = call i8 @llvm.coro.suspend(token %52, i1 false)
  switch i8 %54, label %181 [
    i8 0, label %64
    i8 1, label %55
  ]

55:                                               ; preds = %53
  br label %66

56:                                               ; preds = %47
  %57 = landingpad { ptr, i32 }
          catch ptr null
  %58 = extractvalue { ptr, i32 } %57, 0
  store ptr %58, ptr %7, align 8
  %59 = extractvalue { ptr, i32 } %57, 1
  store i32 %59, ptr %8, align 4
  br label %70

60:                                               ; preds = %64, %51, %48
  %61 = landingpad { ptr, i32 }
          catch ptr null
  %62 = extractvalue { ptr, i32 } %61, 0
  store ptr %62, ptr %7, align 8
  %63 = extractvalue { ptr, i32 } %61, 1
  store i32 %63, ptr %8, align 4
  call void @_ZN4taskD2Ev(ptr noundef nonnull align 8 dereferenceable(8) %12) #2
  br label %70

64:                                               ; preds = %53, %50
  invoke void @_ZN4task12await_resumeEv(ptr noundef nonnull align 8 dereferenceable(8) %12)
          to label %65 unwind label %60

65:                                               ; preds = %64
  br label %66

66:                                               ; preds = %65, %55
  %67 = phi i32 [ 0, %65 ], [ 2, %55 ]
  call void @_ZN4taskD2Ev(ptr noundef nonnull align 8 dereferenceable(8) %12) #2
  call void @llvm.lifetime.end.p0(i64 8, ptr %12) #2
  switch i32 %67, label %133 [
    i32 0, label %68
  ]

68:                                               ; preds = %66
  invoke void @_ZN4task12promise_type11return_voidEv(ptr noundef nonnull align 8 dereferenceable(8) %6)
          to label %69 unwind label %71

69:                                               ; preds = %68
  br label %133

70:                                               ; preds = %60, %56
  call void @llvm.lifetime.end.p0(i64 8, ptr %12) #2
  br label %138

71:                                               ; preds = %68
  %72 = landingpad { ptr, i32 }
          catch ptr null
  %73 = extractvalue { ptr, i32 } %72, 0
  store ptr %73, ptr %7, align 8
  %74 = extractvalue { ptr, i32 } %72, 1
  store i32 %74, ptr %8, align 4
  br label %138

75:                                               ; preds = %41
  call void @llvm.lifetime.start.p0(i64 4, ptr %13) #2
  store i32 0, ptr %13, align 4
  br label %76

76:                                               ; preds = %103, %75
  %77 = load i32, ptr %13, align 4
  %78 = icmp slt i32 %77, 2
  br i1 %78, label %80, label %79

79:                                               ; preds = %76
  br label %107

80:                                               ; preds = %76
  call void @llvm.lifetime.start.p0(i64 8, ptr %14) #2
  %81 = load i32, ptr %13, align 4
  invoke void @_Z4leafi(ptr dead_on_unwind writable sret(%struct.task) align 8 %14, i32 noundef %81)
          to label %82 unwind label %90

82:                                               ; preds = %80
  %83 = invoke noundef i1 @_ZN4task11await_readyEv(ptr noundef nonnull align 8 dereferenceable(8) %14)
          to label %84 unwind label %94

84:                                               ; preds = %82
  br i1 %83, label %98, label %85

85:                                               ; preds = %84
  %86 = call token @llvm.coro.save(ptr null)
  invoke void @llvm.coro.await.suspend.handle(ptr %14, ptr %26, ptr @_Z5chaini.__await_suspend_wrapper__await2)
          to label %87 unwind label %94

87:                                               ; preds = %85
  %88 = call i8 @llvm.coro.suspend(token %86, i1 false)
  switch i8 %88, label %181 [
    i8 0, label %98
    i8 1, label %89
  ]

89:                                               ; preds = %87
  br label %100

90:                                               ; preds = %80
  %91 = landingpad { ptr, i32 }
          catch ptr null
  %92 = extractvalue { ptr, i32 } %91, 0
  store ptr %92, ptr %7, align 8
  %93 = extractvalue { ptr, i32 } %91, 1
  store i32 %93, ptr %8, align 4
  br label %106

94:                                               ; preds = %98, %85, %82
  %95 = landingpad { ptr, i32 }
          catch ptr null
  %96 = extractvalue { ptr, i32 } %95, 0
  store ptr %96, ptr %7, align 8
  %97 = extractvalue { ptr, i32 } %95, 1
  store i32 %97, ptr %8, align 4
  call void @_ZN4taskD2Ev(ptr noundef nonnull align 8 dereferenceable(8) %14) #2
  br label %106

98:                                               ; preds = %87, %84
  invoke void @_ZN4task12await_resumeEv(ptr noundef nonnull align 8 dereferenceable(8) %14)
          to label %99 unwind label %94

99:                                               ; preds = %98
  br label %100

100:                                              ; preds = %99, %89
  %101 = phi i32 [ 0, %99 ], [ 2, %89 ]
  call void @_ZN4taskD2Ev(ptr noundef nonnull align 8 dereferenceable(8) %14) #2
  call void @llvm.lifetime.end.p0(i64 8, ptr %14) #2
  switch i32 %101, label %107 [
    i32 0, label %102
  ]

102:                                              ; preds = %100
  br label %103

103:                                              ; preds = %102
  %104 = load i32, ptr %13, align 4
  %105 = add nsw i32 %104, 1
  store i32 %105, ptr %13, align 4
  br label %76, !llvm.loop !6

106:                                              ; preds = %94, %90
  call void @llvm.lifetime.end.p0(i64 8, ptr %14) #2
  call void @llvm.lifetime.end.p0(i64 4, ptr %13) #2
  br label %138

107:                                              ; preds = %100, %79
  %108 = phi i32 [ %101, %100 ], [ 4, %79 ]
  call void @llvm.lifetime.end.p0(i64 4, ptr %13) #2
  switch i32 %108, label %133 [
    i32 4, label %109
  ]

109:                                              ; preds = %107
  call void @llvm.lifetime.start.p0(i64 8, ptr %15) #2
  %110 = load i32, ptr %5, align 4
  %111 = sub nsw i32 %110, 1
  invoke void @_Z5chaini(ptr dead_on_unwind writable sret(%struct.task) align 8 %15, i32 noundef %111)
          to label %112 unwind label %120

112:                                              ; preds = %109
  %113 = invoke noundef i1 @_ZN4task11await_readyEv(ptr noundef nonnull align 8 dereferenceable(8) %15)
          to label %114 unwind label %124

114:                                              ; preds = %112
  br i1 %113, label %128, label %115

115:                                              ; preds = %114
  %116 = call token @llvm.coro.save(ptr null)
  invoke void @llvm.coro.await.suspend.handle(ptr %15, ptr %26, ptr @_Z5chaini.__await_suspend_wrapper__await3)
          to label %117 unwind label %124

117:                                              ; preds = %115
  %118 = call i8 @llvm.coro.suspend(token %116, i1 false)
  switch i8 %118, label %181 [
    i8 0, label %128
    i8 1, label %119
  ]

119:                                              ; preds = %117
  br label %130

120:                                              ; preds = %109
  %121 = landingpad { ptr, i32 }
          catch ptr null
  %122 = extractvalue { ptr, i32 } %121, 0
  store ptr %122, ptr %7, align 8
  %123 = extractvalue { ptr, i32 } %121, 1
  store i32 %123, ptr %8, align 4
  br label %137

124:                                              ; preds = %128, %115, %112
  %125 = landingpad { ptr, i32 }
          catch ptr null
  %126 = extractvalue { ptr, i32 } %125, 0
  store ptr %126, ptr %7, align 8
  %127 = extractvalue { ptr, i32 } %125, 1
  store i32 %127, ptr %8, align 4
  call void @_ZN4taskD2Ev(ptr noundef nonnull align 8 dereferenceable(8) %15) #2
  br label %137

128:                                              ; preds = %117, %114
  invoke void @_ZN4task12await_resumeEv(ptr noundef nonnull align 8 dereferenceable(8) %15)
          to label %129 unwind label %124

129:                                              ; preds = %128
  br label %130

130:                                              ; preds = %129, %119
  %131 = phi i32 [ 0, %129 ], [ 2, %119 ]
  call void @_ZN4taskD2Ev(ptr noundef nonnull align 8 dereferenceable(8) %15) #2
  call void @llvm.lifetime.end.p0(i64 8, ptr %15) #2
  switch i32 %131, label %133 [
    i32 0, label %132
  ]

132:                                              ; preds = %130
  br label %133

133:                                              ; preds = %132, %130, %107, %69, %66
  %134 = phi i32 [ %67, %66 ], [ 3, %69 ], [ %108, %107 ], [ %131, %130 ], [ 0, %132 ]
  call void @_ZN4DtorD2Ev(ptr noundef nonnull align 4 dereferenceable(4) %11) #2
  call void @llvm.lifetime.end.p0(i64 4, ptr %11) #2
  switch i32 %134, label %173 [
    i32 0, label %135
    i32 3, label %149
  ]

135:                                              ; preds = %133
  invoke void @_ZN4task12promise_type11return_voidEv(ptr noundef nonnull align 8 dereferenceable(8) %6)
          to label %136 unwind label %139

136:                                              ; preds = %135
  br label %149

137:                                              ; preds = %124, %120
  call void @llvm.lifetime.end.p0(i64 8, ptr %15) #2
  br label %138

138:                                              ; preds = %137, %106, %71, %70
  call void @_ZN4DtorD2Ev(ptr noundef nonnull align 4 dereferenceable(4) %11) #2
  call void @llvm.lifetime.end.p0(i64 4, ptr %11) #2
  br label %143

139:                                              ; preds = %135
  %140 = landingpad { ptr, i32 }
          catch ptr null
  %141 = extractvalue { ptr, i32 } %140, 0
  store ptr %141, ptr %7, align 8
  %142 = extractvalue { ptr, i32 } %140, 1
  store i32 %142, ptr %8, align 4
  br label %143

143:                                              ; preds = %139, %138
  %144 = load ptr, ptr %7, align 8
  %145 = call ptr @__cxa_begin_catch(ptr %144) #2
  invoke void @_ZN4task12promise_type19unhandled_exceptionEv(ptr noundef nonnull align 8 dereferenceable(8) %6)
          to label %146 unwind label %156

146:                                              ; preds = %143
  invoke void @__cxa_end_catch()
          to label %147 unwind label %160

147:                                              ; preds = %146
  br label %148

148:                                              ; preds = %147
  br label %149

149:                                              ; preds = %148, %136, %133
  call void @llvm.lifetime.start.p0(i64 1, ptr %16) #2
  call void @_ZN4task12promise_type13final_suspendEv(ptr noundef nonnull align 8 dereferenceable(8) %6) #2
  %150 = call noundef i1 @_ZN4task12promise_type3fin11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %16) #2
  br i1 %150, label %169, label %151

151:                                              ; preds = %149
  %152 = call token @llvm.coro.save(ptr null)
  invoke void @llvm.coro.await.suspend.handle(ptr %16, ptr %26, ptr @_Z5chaini.__await_suspend_wrapper__final)
          to label %153 unwind label %165

153:                                              ; preds = %151
  %154 = call i8 @llvm.coro.suspend(token %152, i1 true)
  switch i8 %154, label %181 [
    i8 0, label %169
    i8 1, label %155
  ]

155:                                              ; preds = %153
  br label %170

156:                                              ; preds = %143
  %157 = landingpad { ptr, i32 }
          cleanup
  %158 = extractvalue { ptr, i32 } %157, 0
  store ptr %158, ptr %7, align 8
  %159 = extractvalue { ptr, i32 } %157, 1
  store i32 %159, ptr %8, align 4
  invoke void @__cxa_end_catch()
          to label %164 unwind label %197

160:                                              ; preds = %146
  %161 = landingpad { ptr, i32 }
          cleanup
  %162 = extractvalue { ptr, i32 } %161, 0
  store ptr %162, ptr %7, align 8
  %163 = extractvalue { ptr, i32 } %161, 1
  store i32 %163, ptr %8, align 4
  br label %183

164:                                              ; preds = %156
  br label %183

165:                                              ; preds = %151
  %166 = landingpad { ptr, i32 }
          cleanup
  %167 = extractvalue { ptr, i32 } %166, 0
  store ptr %167, ptr %7, align 8
  %168 = extractvalue { ptr, i32 } %166, 1
  store i32 %168, ptr %8, align 4
  call void @llvm.lifetime.end.p0(i64 1, ptr %16) #2
  br label %183

169:                                              ; preds = %153, %149
  call void @_ZN4task12promise_type3fin12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %16) #2
  br label %170

170:                                              ; preds = %169, %155
  %171 = phi i32 [ 0, %169 ], [ 2, %155 ]
  call void @llvm.lifetime.end.p0(i64 1, ptr %16) #2
  switch i32 %171, label %173 [
    i32 0, label %172
  ]

172:                                              ; preds = %170
  br label %173

173:                                              ; preds = %172, %170, %133, %39
  %174 = phi i32 [ %40, %39 ], [ %134, %133 ], [ %171, %170 ], [ 0, %172 ]
  call void @llvm.lifetime.end.p0(i64 8, ptr %6) #2
  call void @llvm.lifetime.end.p0(i64 4, ptr %5) #2
  %175 = call ptr @llvm.coro.free(token %19, ptr %26)
  %176 = icmp ne ptr %175, null
  br i1 %176, label %177, label %179

177:                                              ; preds = %173
  %178 = call i64 @llvm.coro.size.i64()
  call void @_ZN4task12promise_typedlEPvm(ptr noundef %175, i64 noundef %178) #2
  br label %179

179:                                              ; preds = %173, %177
  switch i32 %174, label %200 [
    i32 0, label %180
    i32 2, label %181
  ]

180:                                              ; preds = %179
  br label %181

181:                                              ; preds = %180, %179, %153, %117, %87, %53, %30
  %182 = call i1 @llvm.coro.end(ptr null, i1 false, token none)
  ret void

183:                                              ; preds = %165, %164, %160
  %184 = call i1 @llvm.coro.end(ptr null, i1 true, token none)
  br i1 %184, label %192, label %185

185:                                              ; preds = %183
  br label %186

186:                                              ; preds = %185, %34
  call void @llvm.lifetime.end.p0(i64 8, ptr %6) #2
  call void @llvm.lifetime.end.p0(i64 4, ptr %5) #2
  %187 = call ptr @llvm.coro.free(token %19, ptr %26)
  %188 = icmp ne ptr %187, null
  br i1 %188, label %189, label %191

189:                                              ; preds = %186
  %190 = call i64 @llvm.coro.size.i64()
  call void @_ZN4task12promise_typedlEPvm(ptr noundef %187, i64 noundef %190) #2
  br label %191

191:                                              ; preds = %186, %189
  br label %192

192:                                              ; preds = %191, %183
  %193 = load ptr, ptr %7, align 8
  %194 = load i32, ptr %8, align 4
  %195 = insertvalue { ptr, i32 } poison, ptr %193, 0
  %196 = insertvalue { ptr, i32 } %195, i32 %194, 1
  resume { ptr, i32 } %196

197:                                              ; preds = %156
  %198 = landingpad { ptr, i32 }
          catch ptr null
  %199 = extractvalue { ptr, i32 } %198, 0
  call void @__clang_call_terminate(ptr %199) #20
  unreachable

200:                                              ; preds = %179
  unreachable
}

; Function Attrs: alwaysinline mustprogress
define private void @_Z5chaini.__await_suspend_wrapper__init(ptr noundef nonnull %0, ptr noundef %1) #8 {
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

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local noundef i1 @_ZN4task11await_readyEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  ret i1 false
}

; Function Attrs: alwaysinline mustprogress
define private noundef ptr @_Z5chaini.__await_suspend_wrapper__await(ptr noundef nonnull %0, ptr noundef %1) #8 {
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
  %19 = call i64 @_ZN4task13await_suspendENSt7__n486116coroutine_handleIvEE(ptr noundef nonnull align 8 dereferenceable(8) %8, i64 %18)
  %20 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %5, i32 0, i32 0
  %21 = inttoptr i64 %19 to ptr
  store ptr %21, ptr %20, align 8
  %22 = call noundef ptr @_ZNKSt7__n486116coroutine_handleIvE7addressEv(ptr noundef nonnull align 8 dereferenceable(8) %5) #2
  ret ptr %22
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local i64 @_ZN4task13await_suspendENSt7__n486116coroutine_handleIvEE(ptr noundef nonnull align 8 dereferenceable(8) %0, i64 %1) #6 comdat {
  %3 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %4 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %5 = alloca ptr, align 8
  %6 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %4, i32 0, i32 0
  %7 = inttoptr i64 %1 to ptr
  store ptr %7, ptr %6, align 8
  store ptr %0, ptr %5, align 8
  %8 = load ptr, ptr %5, align 8
  %9 = getelementptr inbounds %struct.task, ptr %8, i32 0, i32 0
  %10 = call noundef nonnull align 8 dereferenceable(8) ptr @_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEE7promiseEv(ptr noundef nonnull align 8 dereferenceable(8) %9)
  %11 = getelementptr inbounds %"struct.task::promise_type", ptr %10, i32 0, i32 0
  call void @llvm.memcpy.p0.p0.i64(ptr align 8 %11, ptr align 8 %4, i64 8, i1 false)
  %12 = getelementptr inbounds %struct.task, ptr %8, i32 0, i32 0
  %13 = call i64 @_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEEcvNS0_IvEEEv(ptr noundef nonnull align 8 dereferenceable(8) %12) #2
  %14 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %3, i32 0, i32 0
  %15 = inttoptr i64 %13 to ptr
  store ptr %15, ptr %14, align 8
  %16 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %3, i32 0, i32 0
  %17 = load ptr, ptr %16, align 8
  %18 = ptrtoint ptr %17 to i64
  ret i64 %18
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZN4task12await_resumeEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  ret void
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZN4taskD2Ev(ptr noundef nonnull align 8 dereferenceable(8) %0) unnamed_addr #6 comdat personality ptr @__gxx_personality_v0 {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds %struct.task, ptr %3, i32 0, i32 0
  %5 = call noundef i1 @_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEEcvbEv(ptr noundef nonnull align 8 dereferenceable(8) %4) #2
  br i1 %5, label %6, label %9

6:                                                ; preds = %1
  %7 = getelementptr inbounds %struct.task, ptr %3, i32 0, i32 0
  invoke void @_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEE7destroyEv(ptr noundef nonnull align 8 dereferenceable(8) %7)
          to label %8 unwind label %10

8:                                                ; preds = %6
  br label %9

9:                                                ; preds = %8, %1
  ret void

10:                                               ; preds = %6
  %11 = landingpad { ptr, i32 }
          catch ptr null
  %12 = extractvalue { ptr, i32 } %11, 0
  call void @__clang_call_terminate(ptr %12) #20
  unreachable
}

; Function Attrs: alwaysinline mustprogress
define private noundef ptr @_Z5chaini.__await_suspend_wrapper__await2(ptr noundef nonnull %0, ptr noundef %1) #8 {
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
  %19 = call i64 @_ZN4task13await_suspendENSt7__n486116coroutine_handleIvEE(ptr noundef nonnull align 8 dereferenceable(8) %8, i64 %18)
  %20 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %5, i32 0, i32 0
  %21 = inttoptr i64 %19 to ptr
  store ptr %21, ptr %20, align 8
  %22 = call noundef ptr @_ZNKSt7__n486116coroutine_handleIvE7addressEv(ptr noundef nonnull align 8 dereferenceable(8) %5) #2
  ret ptr %22
}

; Function Attrs: alwaysinline mustprogress
define private noundef ptr @_Z5chaini.__await_suspend_wrapper__await3(ptr noundef nonnull %0, ptr noundef %1) #8 {
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
  %19 = call i64 @_ZN4task13await_suspendENSt7__n486116coroutine_handleIvEE(ptr noundef nonnull align 8 dereferenceable(8) %8, i64 %18)
  %20 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %5, i32 0, i32 0
  %21 = inttoptr i64 %19 to ptr
  store ptr %21, ptr %20, align 8
  %22 = call noundef ptr @_ZNKSt7__n486116coroutine_handleIvE7addressEv(ptr noundef nonnull align 8 dereferenceable(8) %5) #2
  ret ptr %22
}

; Function Attrs: alwaysinline mustprogress
define private noundef ptr @_Z5chaini.__await_suspend_wrapper__final(ptr noundef nonnull %0, ptr noundef %1) #8 {
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

; Function Attrs: mustprogress noinline norecurse optnone uwtable
define dso_local noundef i32 @main() #13 personality ptr @__gxx_personality_v0 {
  %1 = alloca i32, align 4
  %2 = alloca %struct.task, align 8
  %3 = alloca ptr, align 8
  %4 = alloca i32, align 4
  %5 = alloca ptr, align 8
  %6 = alloca %struct.task, align 8
  %7 = alloca ptr, align 8
  store i32 0, ptr %1, align 4
  store i8 1, ptr @fail_gro, align 1
  invoke void @_Z4leafi(ptr dead_on_unwind writable sret(%struct.task) align 8 %2, i32 noundef 0)
          to label %8 unwind label %9

8:                                                ; preds = %0
  call void @_ZN4taskD2Ev(ptr noundef nonnull align 8 dereferenceable(8) %2) #2
  br label %27

9:                                                ; preds = %0
  %10 = landingpad { ptr, i32 }
          catch ptr @_ZTISt9exception
  %11 = extractvalue { ptr, i32 } %10, 0
  store ptr %11, ptr %3, align 8
  %12 = extractvalue { ptr, i32 } %10, 1
  store i32 %12, ptr %4, align 4
  br label %13

13:                                               ; preds = %9
  %14 = load i32, ptr %4, align 4
  %15 = call i32 @llvm.eh.typeid.for.p0(ptr @_ZTISt9exception) #2
  %16 = icmp eq i32 %14, %15
  br i1 %16, label %17, label %69

17:                                               ; preds = %13
  %18 = load ptr, ptr %3, align 8
  %19 = call ptr @__cxa_begin_catch(ptr %18) #2
  store ptr %19, ptr %5, align 8
  %20 = load ptr, ptr %5, align 8
  %21 = load ptr, ptr %20, align 8
  %22 = getelementptr inbounds ptr, ptr %21, i64 2
  %23 = load ptr, ptr %22, align 8
  %24 = call noundef ptr %23(ptr noundef nonnull align 8 dereferenceable(8) %20) #2
  %25 = invoke i32 (ptr, ...) @printf(ptr noundef @.str.1, ptr noundef %24)
          to label %26 unwind label %30

26:                                               ; preds = %17
  call void @__cxa_end_catch()
  br label %27

27:                                               ; preds = %26, %8
  store i8 0, ptr @fail_gro, align 1
  call void @_Z5chaini(ptr dead_on_unwind writable sret(%struct.task) align 8 %6, i32 noundef 2)
  %28 = getelementptr inbounds %struct.task, ptr %6, i32 0, i32 0
  invoke void @_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEE6resumeEv(ptr noundef nonnull align 8 dereferenceable(8) %28)
          to label %29 unwind label %35

29:                                               ; preds = %27
  br label %57

30:                                               ; preds = %17
  %31 = landingpad { ptr, i32 }
          cleanup
  %32 = extractvalue { ptr, i32 } %31, 0
  store ptr %32, ptr %3, align 8
  %33 = extractvalue { ptr, i32 } %31, 1
  store i32 %33, ptr %4, align 4
  invoke void @__cxa_end_catch()
          to label %34 unwind label %74

34:                                               ; preds = %30
  br label %69

35:                                               ; preds = %27
  %36 = landingpad { ptr, i32 }
          cleanup
          catch ptr @_ZTISt9exception
  %37 = extractvalue { ptr, i32 } %36, 0
  store ptr %37, ptr %3, align 8
  %38 = extractvalue { ptr, i32 } %36, 1
  store i32 %38, ptr %4, align 4
  br label %39

39:                                               ; preds = %35
  %40 = load i32, ptr %4, align 4
  %41 = call i32 @llvm.eh.typeid.for.p0(ptr @_ZTISt9exception) #2
  %42 = icmp eq i32 %40, %41
  br i1 %42, label %43, label %68

43:                                               ; preds = %39
  %44 = load ptr, ptr %3, align 8
  %45 = call ptr @__cxa_begin_catch(ptr %44) #2
  store ptr %45, ptr %7, align 8
  %46 = load ptr, ptr %7, align 8
  %47 = load ptr, ptr %46, align 8
  %48 = getelementptr inbounds ptr, ptr %47, i64 2
  %49 = load ptr, ptr %48, align 8
  %50 = call noundef ptr %49(ptr noundef nonnull align 8 dereferenceable(8) %46) #2
  %51 = getelementptr inbounds %struct.task, ptr %6, i32 0, i32 0
  %52 = call noundef i1 @_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEE4doneEv(ptr noundef nonnull align 8 dereferenceable(8) %51) #2
  %53 = zext i1 %52 to i32
  %54 = invoke i32 (ptr, ...) @printf(ptr noundef @.str.2, ptr noundef %50, i32 noundef %53)
          to label %55 unwind label %59

55:                                               ; preds = %43
  invoke void @__cxa_end_catch()
          to label %56 unwind label %63

56:                                               ; preds = %55
  br label %57

57:                                               ; preds = %56, %29
  store i32 0, ptr %1, align 4
  call void @_ZN4taskD2Ev(ptr noundef nonnull align 8 dereferenceable(8) %6) #2
  %58 = load i32, ptr %1, align 4
  ret i32 %58

59:                                               ; preds = %43
  %60 = landingpad { ptr, i32 }
          cleanup
  %61 = extractvalue { ptr, i32 } %60, 0
  store ptr %61, ptr %3, align 8
  %62 = extractvalue { ptr, i32 } %60, 1
  store i32 %62, ptr %4, align 4
  invoke void @__cxa_end_catch()
          to label %67 unwind label %74

63:                                               ; preds = %55
  %64 = landingpad { ptr, i32 }
          cleanup
  %65 = extractvalue { ptr, i32 } %64, 0
  store ptr %65, ptr %3, align 8
  %66 = extractvalue { ptr, i32 } %64, 1
  store i32 %66, ptr %4, align 4
  br label %68

67:                                               ; preds = %59
  br label %68

68:                                               ; preds = %67, %63, %39
  call void @_ZN4taskD2Ev(ptr noundef nonnull align 8 dereferenceable(8) %6) #2
  br label %69

69:                                               ; preds = %68, %34, %13
  %70 = load ptr, ptr %3, align 8
  %71 = load i32, ptr %4, align 4
  %72 = insertvalue { ptr, i32 } poison, ptr %70, 0
  %73 = insertvalue { ptr, i32 } %72, i32 %71, 1
  resume { ptr, i32 } %73

74:                                               ; preds = %59, %30
  %75 = landingpad { ptr, i32 }
          catch ptr null
  %76 = extractvalue { ptr, i32 } %75, 0
  call void @__clang_call_terminate(ptr %76) #20
  unreachable
}

; Function Attrs: nounwind memory(none)
declare i32 @llvm.eh.typeid.for.p0(ptr) #4

declare i32 @printf(ptr noundef, ...) #9

; Function Attrs: mustprogress noinline optnone uwtable
define linkonce_odr dso_local void @_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEE6resumeEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #3 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %3, i32 0, i32 0
  %5 = load ptr, ptr %4, align 8
  call void @llvm.coro.resume(ptr %5)
  ret void
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local noundef i1 @_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEE4doneEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %3, i32 0, i32 0
  %5 = load ptr, ptr %4, align 8
  %6 = call i1 @llvm.coro.done(ptr %5)
  ret i1 %6
}

; Function Attrs: nobuiltin allocsize(0)
declare noundef nonnull ptr @_Znwm(i64 noundef) #14

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
define linkonce_odr dso_local i64 @_ZNSt7__n486116coroutine_handleIN4task12promise_typeEE12from_promiseERS2_(ptr noundef nonnull align 8 dereferenceable(8) %0) #6 comdat {
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

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZNSt7__n486116coroutine_handleIN4task12promise_typeEEC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %0) unnamed_addr #6 comdat {
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
define linkonce_odr dso_local noundef ptr @_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEE7addressEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %3, i32 0, i32 0
  %5 = load ptr, ptr %4, align 8
  ret ptr %5
}

declare void @__cxa_rethrow()

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local noundef nonnull align 8 dereferenceable(8) ptr @_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEE7promiseEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #6 comdat {
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
  %1 = alloca %"struct.std::__n4861::coroutine_handle.1", align 8
  call void @llvm.memset.p0.i64(ptr align 8 %1, i8 0, i64 8, i1 false)
  call void @_ZNSt7__n486116coroutine_handleINS_22noop_coroutine_promiseEEC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %1) #2
  %2 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.1", ptr %1, i32 0, i32 0
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

; Function Attrs: nocallback nofree nounwind willreturn memory(argmem: write)
declare void @llvm.memset.p0.i64(ptr nocapture writeonly, i8, i64, i1 immarg) #16

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZNSt7__n486116coroutine_handleINS_22noop_coroutine_promiseEEC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %0) unnamed_addr #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.1", ptr %3, i32 0, i32 0
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
  %4 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.1", ptr %3, i32 0, i32 0
  %5 = load ptr, ptr %4, align 8
  ret ptr %5
}

; Function Attrs: nobuiltin nounwind
declare void @_ZdlPv(ptr noundef) #17

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local noundef i1 @_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEEcvbEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %3, i32 0, i32 0
  %5 = load ptr, ptr %4, align 8
  %6 = icmp ne ptr %5, null
  ret i1 %6
}

; Function Attrs: mustprogress noinline optnone uwtable
define linkonce_odr dso_local void @_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEE7destroyEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #3 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %3, i32 0, i32 0
  %5 = load ptr, ptr %4, align 8
  call void @llvm.coro.destroy(ptr %5)
  ret void
}

declare void @llvm.coro.destroy(ptr)

declare void @llvm.coro.resume(ptr)

; Function Attrs: nounwind memory(argmem: readwrite)
declare i1 @llvm.coro.done(ptr nocapture readonly) #18

attributes #0 = { mustprogress noinline optnone presplitcoroutine uwtable "frame-pointer"="non-leaf" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #1 = { nocallback nofree nosync nounwind willreturn memory(argmem: read) }
attributes #2 = { nounwind }
attributes #3 = { mustprogress noinline optnone uwtable "frame-pointer"="non-leaf" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #4 = { nounwind memory(none) }
attributes #5 = { nocallback nofree nosync nounwind willreturn memory(argmem: readwrite) }
attributes #6 = { mustprogress noinline nounwind optnone uwtable "frame-pointer"="non-leaf" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #7 = { nomerge nounwind }
attributes #8 = { alwaysinline mustprogress }
attributes #9 = { "frame-pointer"="non-leaf" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #10 = { nounwind "frame-pointer"="non-leaf" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #11 = { noinline noreturn nounwind uwtable "frame-pointer"="non-leaf" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #12 = { nounwind memory(argmem: read) }
attributes #13 = { mustprogress noinline norecurse optnone uwtable "frame-pointer"="non-leaf" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #14 = { nobuiltin allocsize(0) "frame-pointer"="non-leaf" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #15 = { nocallback nofree nounwind willreturn memory(argmem: readwrite) }
attributes #16 = { nocallback nofree nounwind willreturn memory(argmem: write) }
attributes #17 = { nobuiltin nounwind "frame-pointer"="non-leaf" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #18 = { nounwind memory(argmem: readwrite) }
attributes #19 = { noreturn }
attributes #20 = { noreturn nounwind }
attributes #21 = { allocsize(0) }

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
