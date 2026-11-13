; clang++-22 -std=c++20 -O0 -S -emit-llvm -Xclang -disable-llvm-passes
; (aarch64-unknown-linux-gnu; the target triple, datalayout, target-cpu and
; target-features removed, so that the seed runs on every target) of:
;
; // A throwing await_suspend of the handle form ([expr.await]/5.1: the
; // coroutine is resumed and the exception rethrown in it): the wrapper is
; // an invoke whose landing pad runs in the activation that called it,
; // with the frame alive. Once from the ramp (`eager`), once from a resumed
; // activation (`lazy`); the first exception is caught in the body, the
; // second leaves it through unhandled_exception's rethrow, marking the
; // coroutine done.
; #include <coroutine>
; #include <cstdio>
; #include <stdexcept>
; struct Dtor {
;   int v;
;   ~Dtor() { std::printf("dtor %d\n", v); }
; };
; template <bool Eager> struct task {
;   struct promise_type {
;     task get_return_object() { return task{std::coroutine_handle<promise_type>::from_promise(*this)}; }
;     auto initial_suspend() noexcept {
;       if constexpr (Eager) return std::suspend_never{};
;       else return std::suspend_always{};
;     }
;     std::suspend_always final_suspend() noexcept { return {}; }
;     void return_void() {}
;     void unhandled_exception() { throw; }
;   };
;   std::coroutine_handle<promise_type> h;
; };
; struct thrower {
;   int n;
;   bool await_ready() { return false; }
;   std::coroutine_handle<> await_suspend(std::coroutine_handle<>) {
;     throw std::runtime_error(n == 1 ? "first" : "second");
;   }
;   void await_resume() {}
; };
; template <bool Eager> task<Eager> body(const char* name) {
;   Dtor d{Eager ? 1 : 2};
;   int kept = 40;
;   try {
;     co_await thrower{1};
;   } catch (std::exception& e) {
;     std::printf("%s: caught %s in the body, kept %d\n", name, e.what(), kept + 2);
;   }
;   co_await thrower{2};
;   std::printf("not reached\n");
; }
; int main() {
;   try {
;     body<true>("eager");
;   } catch (std::exception& e) {
;     std::printf("eager: caught %s in main\n", e.what());
;   }
;   task<false> t = body<false>("lazy");
;   try {
;     t.h.resume();
;   } catch (std::exception& e) {
;     std::printf("lazy: caught %s in main, done %d\n", e.what(), (int)t.h.done());
;   }
;   t.h.destroy();
; }
;
;@ covers: spec/ir.md §5.5 intrinsics (symmetric transfer; spec/codegen.md §7.6)
;@ input: pre-opt
;@ dialect: 22
;@ link: c++
;@ stdout: eager: caught first in the body, kept 42
;@ stdout: dtor 1
;@ stdout: eager: caught second in main
;@ stdout: lazy: caught first in the body, kept 42
;@ stdout: dtor 2
;@ stdout: lazy: caught second in main, done 1

source_filename = "handle-throws.cpp"

%struct.task = type { %"struct.std::__n4861::coroutine_handle" }
%"struct.std::__n4861::coroutine_handle" = type { ptr }
%struct.task.0 = type { %"struct.std::__n4861::coroutine_handle.1" }
%"struct.std::__n4861::coroutine_handle.1" = type { ptr }
%"struct.task<true>::promise_type" = type { i8 }
%"struct.std::__n4861::suspend_never" = type { i8 }
%struct.Dtor = type { i32 }
%struct.thrower = type { i32 }
%"struct.std::__n4861::suspend_always" = type { i8 }
%"struct.task<false>::promise_type" = type { i8 }
%"struct.std::__n4861::coroutine_handle.2" = type { ptr }

$_Z4bodyILb1EE4taskIXT_EEPKc = comdat any

$__clang_call_terminate = comdat any

$_Z4bodyILb0EE4taskIXT_EEPKc = comdat any

$_ZNKSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEE6resumeEv = comdat any

$_ZNKSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEE4doneEv = comdat any

$_ZNKSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEE7destroyEv = comdat any

$_ZN4taskILb1EE12promise_type17get_return_objectEv = comdat any

$_ZN4taskILb1EE12promise_type15initial_suspendEv = comdat any

$_ZNKSt7__n486113suspend_never11await_readyEv = comdat any

$_ZNKSt7__n486113suspend_never13await_suspendENS_16coroutine_handleIvEE = comdat any

$_ZNSt7__n486116coroutine_handleIN4taskILb1EE12promise_typeEE12from_addressEPv = comdat any

$_ZNKSt7__n486116coroutine_handleIN4taskILb1EE12promise_typeEEcvNS0_IvEEEv = comdat any

$_ZNKSt7__n486113suspend_never12await_resumeEv = comdat any

$_ZN7thrower11await_readyEv = comdat any

$_ZN7thrower13await_suspendENSt7__n486116coroutine_handleIvEE = comdat any

$_ZNKSt7__n486116coroutine_handleIvE7addressEv = comdat any

$_ZN7thrower12await_resumeEv = comdat any

$_ZN4DtorD2Ev = comdat any

$_ZN4taskILb1EE12promise_type11return_voidEv = comdat any

$_ZN4taskILb1EE12promise_type19unhandled_exceptionEv = comdat any

$_ZN4taskILb1EE12promise_type13final_suspendEv = comdat any

$_ZNKSt7__n486114suspend_always11await_readyEv = comdat any

$_ZNKSt7__n486114suspend_always13await_suspendENS_16coroutine_handleIvEE = comdat any

$_ZNKSt7__n486114suspend_always12await_resumeEv = comdat any

$_ZNSt7__n486116coroutine_handleIN4taskILb1EE12promise_typeEE12from_promiseERS3_ = comdat any

$_ZNSt7__n486116coroutine_handleIN4taskILb1EE12promise_typeEEC2Ev = comdat any

$_ZNSt7__n486116coroutine_handleIvE12from_addressEPv = comdat any

$_ZNKSt7__n486116coroutine_handleIN4taskILb1EE12promise_typeEE7addressEv = comdat any

$_ZNSt7__n486116coroutine_handleIvEC2Ev = comdat any

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

@_ZTISt9exception = external constant ptr
@.str = private unnamed_addr constant [6 x i8] c"eager\00", align 1
@.str.1 = private unnamed_addr constant [26 x i8] c"eager: caught %s in main\0A\00", align 1
@.str.2 = private unnamed_addr constant [5 x i8] c"lazy\00", align 1
@.str.3 = private unnamed_addr constant [34 x i8] c"lazy: caught %s in main, done %d\0A\00", align 1
@.str.4 = private unnamed_addr constant [36 x i8] c"%s: caught %s in the body, kept %d\0A\00", align 1
@.str.5 = private unnamed_addr constant [13 x i8] c"not reached\0A\00", align 1
@.str.6 = private unnamed_addr constant [6 x i8] c"first\00", align 1
@.str.7 = private unnamed_addr constant [7 x i8] c"second\00", align 1
@_ZTISt13runtime_error = external constant ptr
@.str.8 = private unnamed_addr constant [9 x i8] c"dtor %d\0A\00", align 1

; Function Attrs: mustprogress noinline norecurse optnone uwtable
define dso_local noundef i32 @main() #0 personality ptr @__gxx_personality_v0 {
  %1 = alloca i32, align 4
  %2 = alloca ptr, align 8
  %3 = alloca i32, align 4
  %4 = alloca %struct.task, align 8
  %5 = alloca ptr, align 8
  %6 = alloca %struct.task.0, align 8
  %7 = alloca ptr, align 8
  store i32 0, ptr %1, align 4
  %8 = invoke i64 @_Z4bodyILb1EE4taskIXT_EEPKc(ptr noundef @.str)
          to label %9 unwind label %13

9:                                                ; preds = %0
  %10 = getelementptr inbounds nuw %struct.task, ptr %4, i32 0, i32 0
  %11 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %10, i32 0, i32 0
  %12 = inttoptr i64 %8 to ptr
  store ptr %12, ptr %11, align 8
  br label %31

13:                                               ; preds = %0
  %14 = landingpad { ptr, i32 }
          catch ptr @_ZTISt9exception
  %15 = extractvalue { ptr, i32 } %14, 0
  store ptr %15, ptr %2, align 8
  %16 = extractvalue { ptr, i32 } %14, 1
  store i32 %16, ptr %3, align 4
  br label %17

17:                                               ; preds = %13
  %18 = load i32, ptr %3, align 4
  %19 = call i32 @llvm.eh.typeid.for.p0(ptr @_ZTISt9exception) #8
  %20 = icmp eq i32 %18, %19
  br i1 %20, label %21, label %72

21:                                               ; preds = %17
  %22 = load ptr, ptr %2, align 8
  %23 = call ptr @__cxa_begin_catch(ptr %22) #8
  store ptr %23, ptr %5, align 8
  %24 = load ptr, ptr %5, align 8, !nonnull !6, !align !7
  %25 = load ptr, ptr %24, align 8
  %26 = getelementptr inbounds ptr, ptr %25, i64 2
  %27 = load ptr, ptr %26, align 8
  %28 = call noundef ptr %27(ptr noundef nonnull align 8 dereferenceable(8) %24) #8
  %29 = invoke i32 (ptr, ...) @printf(ptr noundef @.str.1, ptr noundef %28)
          to label %30 unwind label %38

30:                                               ; preds = %21
  call void @__cxa_end_catch()
  br label %31

31:                                               ; preds = %30, %9
  %32 = call i64 @_Z4bodyILb0EE4taskIXT_EEPKc(ptr noundef @.str.2)
  %33 = getelementptr inbounds nuw %struct.task.0, ptr %6, i32 0, i32 0
  %34 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.1", ptr %33, i32 0, i32 0
  %35 = inttoptr i64 %32 to ptr
  store ptr %35, ptr %34, align 8
  %36 = getelementptr inbounds nuw %struct.task.0, ptr %6, i32 0, i32 0
  invoke void @_ZNKSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEE6resumeEv(ptr noundef nonnull align 8 dereferenceable(8) %36)
          to label %37 unwind label %43

37:                                               ; preds = %31
  br label %64

38:                                               ; preds = %21
  %39 = landingpad { ptr, i32 }
          cleanup
  %40 = extractvalue { ptr, i32 } %39, 0
  store ptr %40, ptr %2, align 8
  %41 = extractvalue { ptr, i32 } %39, 1
  store i32 %41, ptr %3, align 4
  invoke void @__cxa_end_catch()
          to label %42 unwind label %77

42:                                               ; preds = %38
  br label %72

43:                                               ; preds = %31
  %44 = landingpad { ptr, i32 }
          catch ptr @_ZTISt9exception
  %45 = extractvalue { ptr, i32 } %44, 0
  store ptr %45, ptr %2, align 8
  %46 = extractvalue { ptr, i32 } %44, 1
  store i32 %46, ptr %3, align 4
  br label %47

47:                                               ; preds = %43
  %48 = load i32, ptr %3, align 4
  %49 = call i32 @llvm.eh.typeid.for.p0(ptr @_ZTISt9exception) #8
  %50 = icmp eq i32 %48, %49
  br i1 %50, label %51, label %72

51:                                               ; preds = %47
  %52 = load ptr, ptr %2, align 8
  %53 = call ptr @__cxa_begin_catch(ptr %52) #8
  store ptr %53, ptr %7, align 8
  %54 = load ptr, ptr %7, align 8, !nonnull !6, !align !7
  %55 = load ptr, ptr %54, align 8
  %56 = getelementptr inbounds ptr, ptr %55, i64 2
  %57 = load ptr, ptr %56, align 8
  %58 = call noundef ptr %57(ptr noundef nonnull align 8 dereferenceable(8) %54) #8
  %59 = getelementptr inbounds nuw %struct.task.0, ptr %6, i32 0, i32 0
  %60 = call noundef i1 @_ZNKSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEE4doneEv(ptr noundef nonnull align 8 dereferenceable(8) %59) #8
  %61 = zext i1 %60 to i32
  %62 = invoke i32 (ptr, ...) @printf(ptr noundef @.str.3, ptr noundef %58, i32 noundef %61)
          to label %63 unwind label %67

63:                                               ; preds = %51
  call void @__cxa_end_catch()
  br label %64

64:                                               ; preds = %63, %37
  %65 = getelementptr inbounds nuw %struct.task.0, ptr %6, i32 0, i32 0
  call void @_ZNKSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEE7destroyEv(ptr noundef nonnull align 8 dereferenceable(8) %65)
  %66 = load i32, ptr %1, align 4
  ret i32 %66

67:                                               ; preds = %51
  %68 = landingpad { ptr, i32 }
          cleanup
  %69 = extractvalue { ptr, i32 } %68, 0
  store ptr %69, ptr %2, align 8
  %70 = extractvalue { ptr, i32 } %68, 1
  store i32 %70, ptr %3, align 4
  invoke void @__cxa_end_catch()
          to label %71 unwind label %77

71:                                               ; preds = %67
  br label %72

72:                                               ; preds = %71, %47, %42, %17
  %73 = load ptr, ptr %2, align 8
  %74 = load i32, ptr %3, align 4
  %75 = insertvalue { ptr, i32 } poison, ptr %73, 0
  %76 = insertvalue { ptr, i32 } %75, i32 %74, 1
  resume { ptr, i32 } %76

77:                                               ; preds = %67, %38
  %78 = landingpad { ptr, i32 }
          catch ptr null
  %79 = extractvalue { ptr, i32 } %78, 0
  call void @__clang_call_terminate(ptr %79) #17
  unreachable
}

; Function Attrs: mustprogress noinline optnone presplitcoroutine uwtable
define linkonce_odr dso_local i64 @_Z4bodyILb1EE4taskIXT_EEPKc(ptr noundef %0) #1 comdat personality ptr @__gxx_personality_v0 {
  %2 = alloca %struct.task, align 8
  %3 = alloca ptr, align 8, !coro.outside.frame !6
  %4 = alloca ptr, align 8
  %5 = alloca %"struct.task<true>::promise_type", align 1
  %6 = alloca ptr, align 8
  %7 = alloca i32, align 4
  %8 = alloca %"struct.std::__n4861::suspend_never", align 1
  %9 = alloca %"struct.std::__n4861::suspend_never", align 1
  %10 = alloca %struct.Dtor, align 4
  %11 = alloca i32, align 4
  %12 = alloca %struct.thrower, align 4
  %13 = alloca ptr, align 8
  %14 = alloca %struct.thrower, align 4
  %15 = alloca %"struct.std::__n4861::suspend_always", align 1
  %16 = alloca %"struct.std::__n4861::suspend_always", align 1
  store ptr %0, ptr %3, align 8
  %17 = call token @llvm.coro.id(i32 16, ptr %5, ptr null, ptr null)
  %18 = call i1 @llvm.coro.alloc(token %17)
  br i1 %18, label %19, label %22

19:                                               ; preds = %1
  %20 = call i64 @llvm.coro.size.i64()
  %21 = call noalias noundef nonnull ptr @_Znwm(i64 noundef %20) #18
  br label %22

22:                                               ; preds = %19, %1
  %23 = phi ptr [ null, %1 ], [ %21, %19 ]
  %24 = call ptr @llvm.coro.begin(token %17, ptr %23)
  call void @llvm.lifetime.start.p0(ptr %4) #8
  %25 = load ptr, ptr %3, align 8
  store ptr %25, ptr %4, align 8
  call void @llvm.lifetime.start.p0(ptr %5) #8
  %26 = invoke i64 @_ZN4taskILb1EE12promise_type17get_return_objectEv(ptr noundef nonnull align 1 dereferenceable(1) %5)
          to label %27 unwind label %36

27:                                               ; preds = %22
  %28 = getelementptr inbounds nuw %struct.task, ptr %2, i32 0, i32 0
  %29 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %28, i32 0, i32 0
  %30 = inttoptr i64 %26 to ptr
  store ptr %30, ptr %29, align 8
  call void @llvm.lifetime.start.p0(ptr %8) #8
  call void @_ZN4taskILb1EE12promise_type15initial_suspendEv(ptr noundef nonnull align 1 dereferenceable(1) %5) #8
  %31 = call noundef i1 @_ZNKSt7__n486113suspend_never11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %8) #8
  br i1 %31, label %40, label %32

32:                                               ; preds = %27
  %33 = call token @llvm.coro.save(ptr null)
  call void @llvm.coro.await.suspend.void(ptr %8, ptr %24, ptr @_Z4bodyILb1EE4taskIXT_EEPKc.__await_suspend_wrapper__init) #8
  %34 = call i8 @llvm.coro.suspend(token %33, i1 false)
  switch i8 %34, label %156 [
    i8 0, label %40
    i8 1, label %35
  ]

35:                                               ; preds = %32
  br label %41

36:                                               ; preds = %22
  %37 = landingpad { ptr, i32 }
          cleanup
  %38 = extractvalue { ptr, i32 } %37, 0
  store ptr %38, ptr %6, align 8
  %39 = extractvalue { ptr, i32 } %37, 1
  store i32 %39, ptr %7, align 4
  br label %164

40:                                               ; preds = %32, %27
  call void @_ZNKSt7__n486113suspend_never12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %8) #8
  br label %41

41:                                               ; preds = %40, %35
  %42 = phi i32 [ 0, %40 ], [ 2, %35 ]
  call void @llvm.lifetime.end.p0(ptr %8) #8
  switch i32 %42, label %148 [
    i32 0, label %43
  ]

43:                                               ; preds = %41
  call void @llvm.lifetime.start.p0(ptr %10) #8
  %44 = getelementptr inbounds nuw %struct.Dtor, ptr %10, i32 0, i32 0
  store i32 1, ptr %44, align 4
  call void @llvm.lifetime.start.p0(ptr %11) #8
  store i32 40, ptr %11, align 4
  call void @llvm.lifetime.start.p0(ptr %12) #8
  %45 = getelementptr inbounds nuw %struct.thrower, ptr %12, i32 0, i32 0
  store i32 1, ptr %45, align 4
  %46 = invoke noundef i1 @_ZN7thrower11await_readyEv(ptr noundef nonnull align 4 dereferenceable(4) %12)
          to label %47 unwind label %53

47:                                               ; preds = %43
  br i1 %46, label %84, label %48

48:                                               ; preds = %47
  %49 = call token @llvm.coro.save(ptr null)
  invoke void @llvm.coro.await.suspend.handle(ptr %12, ptr %24, ptr @_Z4bodyILb1EE4taskIXT_EEPKc.__await_suspend_wrapper__await)
          to label %50 unwind label %53

50:                                               ; preds = %48
  %51 = call i8 @llvm.coro.suspend(token %49, i1 false)
  switch i8 %51, label %156 [
    i8 0, label %84
    i8 1, label %52
  ]

52:                                               ; preds = %50
  br label %86

53:                                               ; preds = %84, %48, %43
  %54 = landingpad { ptr, i32 }
          catch ptr @_ZTISt9exception
          catch ptr null
  %55 = extractvalue { ptr, i32 } %54, 0
  store ptr %55, ptr %6, align 8
  %56 = extractvalue { ptr, i32 } %54, 1
  store i32 %56, ptr %7, align 4
  call void @llvm.lifetime.end.p0(ptr %12) #8
  br label %57

57:                                               ; preds = %53
  %58 = load i32, ptr %7, align 4
  %59 = call i32 @llvm.eh.typeid.for.p0(ptr @_ZTISt9exception) #8
  %60 = icmp eq i32 %58, %59
  br i1 %60, label %61, label %118

61:                                               ; preds = %57
  call void @llvm.lifetime.start.p0(ptr %13) #8
  %62 = load ptr, ptr %6, align 8
  %63 = call ptr @__cxa_begin_catch(ptr %62) #8
  store ptr %63, ptr %13, align 8
  %64 = load ptr, ptr %4, align 8
  %65 = load ptr, ptr %13, align 8, !nonnull !6, !align !7
  %66 = load ptr, ptr %65, align 8
  %67 = getelementptr inbounds ptr, ptr %66, i64 2
  %68 = load ptr, ptr %67, align 8
  %69 = call noundef ptr %68(ptr noundef nonnull align 8 dereferenceable(8) %65) #8
  %70 = load i32, ptr %11, align 4
  %71 = add nsw i32 %70, 2
  %72 = invoke i32 (ptr, ...) @printf(ptr noundef @.str.4, ptr noundef %64, ptr noundef %69, i32 noundef %71)
          to label %73 unwind label %89

73:                                               ; preds = %61
  invoke void @__cxa_end_catch()
          to label %74 unwind label %93

74:                                               ; preds = %73
  call void @llvm.lifetime.end.p0(ptr %13) #8
  br label %75

75:                                               ; preds = %74, %88
  call void @llvm.lifetime.start.p0(ptr %14) #8
  %76 = getelementptr inbounds nuw %struct.thrower, ptr %14, i32 0, i32 0
  store i32 2, ptr %76, align 4
  %77 = invoke noundef i1 @_ZN7thrower11await_readyEv(ptr noundef nonnull align 4 dereferenceable(4) %14)
          to label %78 unwind label %99

78:                                               ; preds = %75
  br i1 %77, label %103, label %79

79:                                               ; preds = %78
  %80 = call token @llvm.coro.save(ptr null)
  invoke void @llvm.coro.await.suspend.handle(ptr %14, ptr %24, ptr @_Z4bodyILb1EE4taskIXT_EEPKc.__await_suspend_wrapper__await2)
          to label %81 unwind label %99

81:                                               ; preds = %79
  %82 = call i8 @llvm.coro.suspend(token %80, i1 false)
  switch i8 %82, label %156 [
    i8 0, label %103
    i8 1, label %83
  ]

83:                                               ; preds = %81
  br label %105

84:                                               ; preds = %50, %47
  invoke void @_ZN7thrower12await_resumeEv(ptr noundef nonnull align 4 dereferenceable(4) %12)
          to label %85 unwind label %53

85:                                               ; preds = %84
  br label %86

86:                                               ; preds = %85, %52
  %87 = phi i32 [ 0, %85 ], [ 2, %52 ]
  call void @llvm.lifetime.end.p0(ptr %12) #8
  switch i32 %87, label %110 [
    i32 0, label %88
  ]

88:                                               ; preds = %86
  br label %75

89:                                               ; preds = %61
  %90 = landingpad { ptr, i32 }
          catch ptr null
  %91 = extractvalue { ptr, i32 } %90, 0
  store ptr %91, ptr %6, align 8
  %92 = extractvalue { ptr, i32 } %90, 1
  store i32 %92, ptr %7, align 4
  invoke void @__cxa_end_catch()
          to label %97 unwind label %175

93:                                               ; preds = %73
  %94 = landingpad { ptr, i32 }
          catch ptr null
  %95 = extractvalue { ptr, i32 } %94, 0
  store ptr %95, ptr %6, align 8
  %96 = extractvalue { ptr, i32 } %94, 1
  store i32 %96, ptr %7, align 4
  br label %98

97:                                               ; preds = %89
  br label %98

98:                                               ; preds = %97, %93
  call void @llvm.lifetime.end.p0(ptr %13) #8
  br label %118

99:                                               ; preds = %103, %79, %75
  %100 = landingpad { ptr, i32 }
          catch ptr null
  %101 = extractvalue { ptr, i32 } %100, 0
  store ptr %101, ptr %6, align 8
  %102 = extractvalue { ptr, i32 } %100, 1
  store i32 %102, ptr %7, align 4
  call void @llvm.lifetime.end.p0(ptr %14) #8
  br label %118

103:                                              ; preds = %81, %78
  invoke void @_ZN7thrower12await_resumeEv(ptr noundef nonnull align 4 dereferenceable(4) %14)
          to label %104 unwind label %99

104:                                              ; preds = %103
  br label %105

105:                                              ; preds = %104, %83
  %106 = phi i32 [ 0, %104 ], [ 2, %83 ]
  call void @llvm.lifetime.end.p0(ptr %14) #8
  switch i32 %106, label %110 [
    i32 0, label %107
  ]

107:                                              ; preds = %105
  %108 = invoke i32 (ptr, ...) @printf(ptr noundef @.str.5)
          to label %109 unwind label %114

109:                                              ; preds = %107
  br label %110

110:                                              ; preds = %109, %105, %86
  %111 = phi i32 [ %87, %86 ], [ %106, %105 ], [ 0, %109 ]
  call void @llvm.lifetime.end.p0(ptr %11) #8
  call void @_ZN4DtorD2Ev(ptr noundef nonnull align 4 dereferenceable(4) %10) #8
  call void @llvm.lifetime.end.p0(ptr %10) #8
  switch i32 %111, label %148 [
    i32 0, label %112
  ]

112:                                              ; preds = %110
  invoke void @_ZN4taskILb1EE12promise_type11return_voidEv(ptr noundef nonnull align 1 dereferenceable(1) %5)
          to label %113 unwind label %119

113:                                              ; preds = %112
  br label %129

114:                                              ; preds = %107
  %115 = landingpad { ptr, i32 }
          catch ptr null
  %116 = extractvalue { ptr, i32 } %115, 0
  store ptr %116, ptr %6, align 8
  %117 = extractvalue { ptr, i32 } %115, 1
  store i32 %117, ptr %7, align 4
  br label %118

118:                                              ; preds = %114, %99, %98, %57
  call void @llvm.lifetime.end.p0(ptr %11) #8
  call void @_ZN4DtorD2Ev(ptr noundef nonnull align 4 dereferenceable(4) %10) #8
  call void @llvm.lifetime.end.p0(ptr %10) #8
  br label %123

119:                                              ; preds = %112
  %120 = landingpad { ptr, i32 }
          catch ptr null
  %121 = extractvalue { ptr, i32 } %120, 0
  store ptr %121, ptr %6, align 8
  %122 = extractvalue { ptr, i32 } %120, 1
  store i32 %122, ptr %7, align 4
  br label %123

123:                                              ; preds = %119, %118
  %124 = load ptr, ptr %6, align 8
  %125 = call ptr @__cxa_begin_catch(ptr %124) #8
  invoke void @_ZN4taskILb1EE12promise_type19unhandled_exceptionEv(ptr noundef nonnull align 1 dereferenceable(1) %5)
          to label %126 unwind label %135

126:                                              ; preds = %123
  invoke void @__cxa_end_catch()
          to label %127 unwind label %139

127:                                              ; preds = %126
  br label %128

128:                                              ; preds = %127
  br label %129

129:                                              ; preds = %128, %113
  call void @llvm.lifetime.start.p0(ptr %15) #8
  call void @_ZN4taskILb1EE12promise_type13final_suspendEv(ptr noundef nonnull align 1 dereferenceable(1) %5) #8
  %130 = call noundef i1 @_ZNKSt7__n486114suspend_always11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %15) #8
  br i1 %130, label %144, label %131

131:                                              ; preds = %129
  %132 = call token @llvm.coro.save(ptr null)
  call void @llvm.coro.await.suspend.void(ptr %15, ptr %24, ptr @_Z4bodyILb1EE4taskIXT_EEPKc.__await_suspend_wrapper__final) #8
  %133 = call i8 @llvm.coro.suspend(token %132, i1 true)
  switch i8 %133, label %156 [
    i8 0, label %144
    i8 1, label %134
  ]

134:                                              ; preds = %131
  br label %145

135:                                              ; preds = %123
  %136 = landingpad { ptr, i32 }
          cleanup
  %137 = extractvalue { ptr, i32 } %136, 0
  store ptr %137, ptr %6, align 8
  %138 = extractvalue { ptr, i32 } %136, 1
  store i32 %138, ptr %7, align 4
  invoke void @__cxa_end_catch()
          to label %143 unwind label %175

139:                                              ; preds = %126
  %140 = landingpad { ptr, i32 }
          cleanup
  %141 = extractvalue { ptr, i32 } %140, 0
  store ptr %141, ptr %6, align 8
  %142 = extractvalue { ptr, i32 } %140, 1
  store i32 %142, ptr %7, align 4
  br label %161

143:                                              ; preds = %135
  br label %161

144:                                              ; preds = %131, %129
  call void @_ZNKSt7__n486114suspend_always12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %15) #8
  br label %145

145:                                              ; preds = %144, %134
  %146 = phi i32 [ 0, %144 ], [ 2, %134 ]
  call void @llvm.lifetime.end.p0(ptr %15) #8
  switch i32 %146, label %148 [
    i32 0, label %147
  ]

147:                                              ; preds = %145
  br label %148

148:                                              ; preds = %147, %145, %110, %41
  %149 = phi i32 [ %42, %41 ], [ %111, %110 ], [ %146, %145 ], [ 0, %147 ]
  call void @llvm.lifetime.end.p0(ptr %5) #8
  call void @llvm.lifetime.end.p0(ptr %4) #8
  %150 = call ptr @llvm.coro.free(token %17, ptr %24)
  %151 = icmp ne ptr %150, null
  br i1 %151, label %152, label %154

152:                                              ; preds = %148
  %153 = call i64 @llvm.coro.size.i64()
  call void @_ZdlPvm(ptr noundef %150, i64 noundef %153) #8
  br label %154

154:                                              ; preds = %148, %152
  switch i32 %149, label %178 [
    i32 0, label %155
    i32 2, label %156
  ]

155:                                              ; preds = %154
  br label %156

156:                                              ; preds = %155, %154, %131, %81, %50, %32
  call void @llvm.coro.end(ptr null, i1 false, token none)
  %157 = getelementptr inbounds nuw %struct.task, ptr %2, i32 0, i32 0
  %158 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %157, i32 0, i32 0
  %159 = load ptr, ptr %158, align 8
  %160 = ptrtoint ptr %159 to i64
  ret i64 %160

161:                                              ; preds = %143, %139
  call void @llvm.coro.end(ptr null, i1 true, token none)
  %162 = call i1 @llvm.coro.is_in_ramp()
  br i1 %162, label %163, label %170

163:                                              ; preds = %161
  br label %164

164:                                              ; preds = %163, %36
  call void @llvm.lifetime.end.p0(ptr %5) #8
  call void @llvm.lifetime.end.p0(ptr %4) #8
  %165 = call ptr @llvm.coro.free(token %17, ptr %24)
  %166 = icmp ne ptr %165, null
  br i1 %166, label %167, label %169

167:                                              ; preds = %164
  %168 = call i64 @llvm.coro.size.i64()
  call void @_ZdlPvm(ptr noundef %165, i64 noundef %168) #8
  br label %169

169:                                              ; preds = %164, %167
  br label %170

170:                                              ; preds = %169, %161
  %171 = load ptr, ptr %6, align 8
  %172 = load i32, ptr %7, align 4
  %173 = insertvalue { ptr, i32 } poison, ptr %171, 0
  %174 = insertvalue { ptr, i32 } %173, i32 %172, 1
  resume { ptr, i32 } %174

175:                                              ; preds = %135, %89
  %176 = landingpad { ptr, i32 }
          catch ptr null
  %177 = extractvalue { ptr, i32 } %176, 0
  call void @__clang_call_terminate(ptr %177) #17
  unreachable

178:                                              ; preds = %154
  unreachable
}

declare i32 @__gxx_personality_v0(...)

; Function Attrs: nounwind memory(none)
declare i32 @llvm.eh.typeid.for.p0(ptr) #2

declare ptr @__cxa_begin_catch(ptr)

declare i32 @printf(ptr noundef, ...) #3

declare void @__cxa_end_catch()

; Function Attrs: noinline noreturn nounwind uwtable
define linkonce_odr hidden void @__clang_call_terminate(ptr noundef %0) #4 comdat {
  %2 = call ptr @__cxa_begin_catch(ptr %0) #8
  call void @_ZSt9terminatev() #17
  unreachable
}

declare void @_ZSt9terminatev()

; Function Attrs: mustprogress noinline optnone presplitcoroutine uwtable
define linkonce_odr dso_local i64 @_Z4bodyILb0EE4taskIXT_EEPKc(ptr noundef %0) #1 comdat personality ptr @__gxx_personality_v0 {
  %2 = alloca %struct.task.0, align 8
  %3 = alloca ptr, align 8, !coro.outside.frame !6
  %4 = alloca ptr, align 8
  %5 = alloca %"struct.task<false>::promise_type", align 1
  %6 = alloca ptr, align 8
  %7 = alloca i32, align 4
  %8 = alloca %"struct.std::__n4861::suspend_always", align 1
  %9 = alloca %"struct.std::__n4861::suspend_always", align 1
  %10 = alloca %struct.Dtor, align 4
  %11 = alloca i32, align 4
  %12 = alloca %struct.thrower, align 4
  %13 = alloca ptr, align 8
  %14 = alloca %struct.thrower, align 4
  %15 = alloca %"struct.std::__n4861::suspend_always", align 1
  %16 = alloca %"struct.std::__n4861::suspend_always", align 1
  store ptr %0, ptr %3, align 8
  %17 = call token @llvm.coro.id(i32 16, ptr %5, ptr null, ptr null)
  %18 = call i1 @llvm.coro.alloc(token %17)
  br i1 %18, label %19, label %22

19:                                               ; preds = %1
  %20 = call i64 @llvm.coro.size.i64()
  %21 = call noalias noundef nonnull ptr @_Znwm(i64 noundef %20) #18
  br label %22

22:                                               ; preds = %19, %1
  %23 = phi ptr [ null, %1 ], [ %21, %19 ]
  %24 = call ptr @llvm.coro.begin(token %17, ptr %23)
  call void @llvm.lifetime.start.p0(ptr %4) #8
  %25 = load ptr, ptr %3, align 8
  store ptr %25, ptr %4, align 8
  call void @llvm.lifetime.start.p0(ptr %5) #8
  %26 = invoke i64 @_ZN4taskILb0EE12promise_type17get_return_objectEv(ptr noundef nonnull align 1 dereferenceable(1) %5)
          to label %27 unwind label %36

27:                                               ; preds = %22
  %28 = getelementptr inbounds nuw %struct.task.0, ptr %2, i32 0, i32 0
  %29 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.1", ptr %28, i32 0, i32 0
  %30 = inttoptr i64 %26 to ptr
  store ptr %30, ptr %29, align 8
  call void @llvm.lifetime.start.p0(ptr %8) #8
  call void @_ZN4taskILb0EE12promise_type15initial_suspendEv(ptr noundef nonnull align 1 dereferenceable(1) %5) #8
  %31 = call noundef i1 @_ZNKSt7__n486114suspend_always11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %8) #8
  br i1 %31, label %40, label %32

32:                                               ; preds = %27
  %33 = call token @llvm.coro.save(ptr null)
  call void @llvm.coro.await.suspend.void(ptr %8, ptr %24, ptr @_Z4bodyILb0EE4taskIXT_EEPKc.__await_suspend_wrapper__init) #8
  %34 = call i8 @llvm.coro.suspend(token %33, i1 false)
  switch i8 %34, label %152 [
    i8 0, label %40
    i8 1, label %35
  ]

35:                                               ; preds = %32
  br label %41

36:                                               ; preds = %22
  %37 = landingpad { ptr, i32 }
          cleanup
  %38 = extractvalue { ptr, i32 } %37, 0
  store ptr %38, ptr %6, align 8
  %39 = extractvalue { ptr, i32 } %37, 1
  store i32 %39, ptr %7, align 4
  br label %160

40:                                               ; preds = %32, %27
  call void @_ZNKSt7__n486114suspend_always12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %8) #8
  br label %41

41:                                               ; preds = %40, %35
  %42 = phi i32 [ 0, %40 ], [ 2, %35 ]
  call void @llvm.lifetime.end.p0(ptr %8) #8
  switch i32 %42, label %144 [
    i32 0, label %43
  ]

43:                                               ; preds = %41
  call void @llvm.lifetime.start.p0(ptr %10) #8
  %44 = getelementptr inbounds nuw %struct.Dtor, ptr %10, i32 0, i32 0
  store i32 2, ptr %44, align 4
  call void @llvm.lifetime.start.p0(ptr %11) #8
  store i32 40, ptr %11, align 4
  call void @llvm.lifetime.start.p0(ptr %12) #8
  %45 = getelementptr inbounds nuw %struct.thrower, ptr %12, i32 0, i32 0
  store i32 1, ptr %45, align 4
  %46 = call noundef i1 @_ZN7thrower11await_readyEv(ptr noundef nonnull align 4 dereferenceable(4) %12)
  br i1 %46, label %82, label %47

47:                                               ; preds = %43
  %48 = call token @llvm.coro.save(ptr null)
  invoke void @llvm.coro.await.suspend.handle(ptr %12, ptr %24, ptr @_Z4bodyILb0EE4taskIXT_EEPKc.__await_suspend_wrapper__await)
          to label %49 unwind label %52

49:                                               ; preds = %47
  %50 = call i8 @llvm.coro.suspend(token %48, i1 false)
  switch i8 %50, label %152 [
    i8 0, label %82
    i8 1, label %51
  ]

51:                                               ; preds = %49
  br label %83

52:                                               ; preds = %47
  %53 = landingpad { ptr, i32 }
          catch ptr @_ZTISt9exception
          catch ptr null
  %54 = extractvalue { ptr, i32 } %53, 0
  store ptr %54, ptr %6, align 8
  %55 = extractvalue { ptr, i32 } %53, 1
  store i32 %55, ptr %7, align 4
  call void @llvm.lifetime.end.p0(ptr %12) #8
  br label %56

56:                                               ; preds = %52
  %57 = load i32, ptr %7, align 4
  %58 = call i32 @llvm.eh.typeid.for.p0(ptr @_ZTISt9exception) #8
  %59 = icmp eq i32 %57, %58
  br i1 %59, label %60, label %114

60:                                               ; preds = %56
  call void @llvm.lifetime.start.p0(ptr %13) #8
  %61 = load ptr, ptr %6, align 8
  %62 = call ptr @__cxa_begin_catch(ptr %61) #8
  store ptr %62, ptr %13, align 8
  %63 = load ptr, ptr %4, align 8
  %64 = load ptr, ptr %13, align 8, !nonnull !6, !align !7
  %65 = load ptr, ptr %64, align 8
  %66 = getelementptr inbounds ptr, ptr %65, i64 2
  %67 = load ptr, ptr %66, align 8
  %68 = call noundef ptr %67(ptr noundef nonnull align 8 dereferenceable(8) %64) #8
  %69 = load i32, ptr %11, align 4
  %70 = add nsw i32 %69, 2
  %71 = invoke i32 (ptr, ...) @printf(ptr noundef @.str.4, ptr noundef %63, ptr noundef %68, i32 noundef %70)
          to label %72 unwind label %86

72:                                               ; preds = %60
  invoke void @__cxa_end_catch()
          to label %73 unwind label %90

73:                                               ; preds = %72
  call void @llvm.lifetime.end.p0(ptr %13) #8
  br label %74

74:                                               ; preds = %73, %85
  call void @llvm.lifetime.start.p0(ptr %14) #8
  %75 = getelementptr inbounds nuw %struct.thrower, ptr %14, i32 0, i32 0
  store i32 2, ptr %75, align 4
  %76 = call noundef i1 @_ZN7thrower11await_readyEv(ptr noundef nonnull align 4 dereferenceable(4) %14)
  br i1 %76, label %100, label %77

77:                                               ; preds = %74
  %78 = call token @llvm.coro.save(ptr null)
  invoke void @llvm.coro.await.suspend.handle(ptr %14, ptr %24, ptr @_Z4bodyILb0EE4taskIXT_EEPKc.__await_suspend_wrapper__await2)
          to label %79 unwind label %96

79:                                               ; preds = %77
  %80 = call i8 @llvm.coro.suspend(token %78, i1 false)
  switch i8 %80, label %152 [
    i8 0, label %100
    i8 1, label %81
  ]

81:                                               ; preds = %79
  br label %101

82:                                               ; preds = %49, %43
  call void @_ZN7thrower12await_resumeEv(ptr noundef nonnull align 4 dereferenceable(4) %12)
  br label %83

83:                                               ; preds = %82, %51
  %84 = phi i32 [ 0, %82 ], [ 2, %51 ]
  call void @llvm.lifetime.end.p0(ptr %12) #8
  switch i32 %84, label %106 [
    i32 0, label %85
  ]

85:                                               ; preds = %83
  br label %74

86:                                               ; preds = %60
  %87 = landingpad { ptr, i32 }
          catch ptr null
  %88 = extractvalue { ptr, i32 } %87, 0
  store ptr %88, ptr %6, align 8
  %89 = extractvalue { ptr, i32 } %87, 1
  store i32 %89, ptr %7, align 4
  invoke void @__cxa_end_catch()
          to label %94 unwind label %171

90:                                               ; preds = %72
  %91 = landingpad { ptr, i32 }
          catch ptr null
  %92 = extractvalue { ptr, i32 } %91, 0
  store ptr %92, ptr %6, align 8
  %93 = extractvalue { ptr, i32 } %91, 1
  store i32 %93, ptr %7, align 4
  br label %95

94:                                               ; preds = %86
  br label %95

95:                                               ; preds = %94, %90
  call void @llvm.lifetime.end.p0(ptr %13) #8
  br label %114

96:                                               ; preds = %77
  %97 = landingpad { ptr, i32 }
          catch ptr null
  %98 = extractvalue { ptr, i32 } %97, 0
  store ptr %98, ptr %6, align 8
  %99 = extractvalue { ptr, i32 } %97, 1
  store i32 %99, ptr %7, align 4
  call void @llvm.lifetime.end.p0(ptr %14) #8
  br label %114

100:                                              ; preds = %79, %74
  call void @_ZN7thrower12await_resumeEv(ptr noundef nonnull align 4 dereferenceable(4) %14)
  br label %101

101:                                              ; preds = %100, %81
  %102 = phi i32 [ 0, %100 ], [ 2, %81 ]
  call void @llvm.lifetime.end.p0(ptr %14) #8
  switch i32 %102, label %106 [
    i32 0, label %103
  ]

103:                                              ; preds = %101
  %104 = invoke i32 (ptr, ...) @printf(ptr noundef @.str.5)
          to label %105 unwind label %110

105:                                              ; preds = %103
  br label %106

106:                                              ; preds = %105, %101, %83
  %107 = phi i32 [ %84, %83 ], [ %102, %101 ], [ 0, %105 ]
  call void @llvm.lifetime.end.p0(ptr %11) #8
  call void @_ZN4DtorD2Ev(ptr noundef nonnull align 4 dereferenceable(4) %10) #8
  call void @llvm.lifetime.end.p0(ptr %10) #8
  switch i32 %107, label %144 [
    i32 0, label %108
  ]

108:                                              ; preds = %106
  invoke void @_ZN4taskILb0EE12promise_type11return_voidEv(ptr noundef nonnull align 1 dereferenceable(1) %5)
          to label %109 unwind label %115

109:                                              ; preds = %108
  br label %125

110:                                              ; preds = %103
  %111 = landingpad { ptr, i32 }
          catch ptr null
  %112 = extractvalue { ptr, i32 } %111, 0
  store ptr %112, ptr %6, align 8
  %113 = extractvalue { ptr, i32 } %111, 1
  store i32 %113, ptr %7, align 4
  br label %114

114:                                              ; preds = %110, %96, %95, %56
  call void @llvm.lifetime.end.p0(ptr %11) #8
  call void @_ZN4DtorD2Ev(ptr noundef nonnull align 4 dereferenceable(4) %10) #8
  call void @llvm.lifetime.end.p0(ptr %10) #8
  br label %119

115:                                              ; preds = %108
  %116 = landingpad { ptr, i32 }
          catch ptr null
  %117 = extractvalue { ptr, i32 } %116, 0
  store ptr %117, ptr %6, align 8
  %118 = extractvalue { ptr, i32 } %116, 1
  store i32 %118, ptr %7, align 4
  br label %119

119:                                              ; preds = %115, %114
  %120 = load ptr, ptr %6, align 8
  %121 = call ptr @__cxa_begin_catch(ptr %120) #8
  invoke void @_ZN4taskILb0EE12promise_type19unhandled_exceptionEv(ptr noundef nonnull align 1 dereferenceable(1) %5)
          to label %122 unwind label %131

122:                                              ; preds = %119
  invoke void @__cxa_end_catch()
          to label %123 unwind label %135

123:                                              ; preds = %122
  br label %124

124:                                              ; preds = %123
  br label %125

125:                                              ; preds = %124, %109
  call void @llvm.lifetime.start.p0(ptr %15) #8
  call void @_ZN4taskILb0EE12promise_type13final_suspendEv(ptr noundef nonnull align 1 dereferenceable(1) %5) #8
  %126 = call noundef i1 @_ZNKSt7__n486114suspend_always11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %15) #8
  br i1 %126, label %140, label %127

127:                                              ; preds = %125
  %128 = call token @llvm.coro.save(ptr null)
  call void @llvm.coro.await.suspend.void(ptr %15, ptr %24, ptr @_Z4bodyILb0EE4taskIXT_EEPKc.__await_suspend_wrapper__final) #8
  %129 = call i8 @llvm.coro.suspend(token %128, i1 true)
  switch i8 %129, label %152 [
    i8 0, label %140
    i8 1, label %130
  ]

130:                                              ; preds = %127
  br label %141

131:                                              ; preds = %119
  %132 = landingpad { ptr, i32 }
          cleanup
  %133 = extractvalue { ptr, i32 } %132, 0
  store ptr %133, ptr %6, align 8
  %134 = extractvalue { ptr, i32 } %132, 1
  store i32 %134, ptr %7, align 4
  invoke void @__cxa_end_catch()
          to label %139 unwind label %171

135:                                              ; preds = %122
  %136 = landingpad { ptr, i32 }
          cleanup
  %137 = extractvalue { ptr, i32 } %136, 0
  store ptr %137, ptr %6, align 8
  %138 = extractvalue { ptr, i32 } %136, 1
  store i32 %138, ptr %7, align 4
  br label %157

139:                                              ; preds = %131
  br label %157

140:                                              ; preds = %127, %125
  call void @_ZNKSt7__n486114suspend_always12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %15) #8
  br label %141

141:                                              ; preds = %140, %130
  %142 = phi i32 [ 0, %140 ], [ 2, %130 ]
  call void @llvm.lifetime.end.p0(ptr %15) #8
  switch i32 %142, label %144 [
    i32 0, label %143
  ]

143:                                              ; preds = %141
  br label %144

144:                                              ; preds = %143, %141, %106, %41
  %145 = phi i32 [ %42, %41 ], [ %107, %106 ], [ %142, %141 ], [ 0, %143 ]
  call void @llvm.lifetime.end.p0(ptr %5) #8
  call void @llvm.lifetime.end.p0(ptr %4) #8
  %146 = call ptr @llvm.coro.free(token %17, ptr %24)
  %147 = icmp ne ptr %146, null
  br i1 %147, label %148, label %150

148:                                              ; preds = %144
  %149 = call i64 @llvm.coro.size.i64()
  call void @_ZdlPvm(ptr noundef %146, i64 noundef %149) #8
  br label %150

150:                                              ; preds = %144, %148
  switch i32 %145, label %174 [
    i32 0, label %151
    i32 2, label %152
  ]

151:                                              ; preds = %150
  br label %152

152:                                              ; preds = %151, %150, %127, %79, %49, %32
  call void @llvm.coro.end(ptr null, i1 false, token none)
  %153 = getelementptr inbounds nuw %struct.task.0, ptr %2, i32 0, i32 0
  %154 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.1", ptr %153, i32 0, i32 0
  %155 = load ptr, ptr %154, align 8
  %156 = ptrtoint ptr %155 to i64
  ret i64 %156

157:                                              ; preds = %139, %135
  call void @llvm.coro.end(ptr null, i1 true, token none)
  %158 = call i1 @llvm.coro.is_in_ramp()
  br i1 %158, label %159, label %166

159:                                              ; preds = %157
  br label %160

160:                                              ; preds = %159, %36
  call void @llvm.lifetime.end.p0(ptr %5) #8
  call void @llvm.lifetime.end.p0(ptr %4) #8
  %161 = call ptr @llvm.coro.free(token %17, ptr %24)
  %162 = icmp ne ptr %161, null
  br i1 %162, label %163, label %165

163:                                              ; preds = %160
  %164 = call i64 @llvm.coro.size.i64()
  call void @_ZdlPvm(ptr noundef %161, i64 noundef %164) #8
  br label %165

165:                                              ; preds = %160, %163
  br label %166

166:                                              ; preds = %165, %157
  %167 = load ptr, ptr %6, align 8
  %168 = load i32, ptr %7, align 4
  %169 = insertvalue { ptr, i32 } poison, ptr %167, 0
  %170 = insertvalue { ptr, i32 } %169, i32 %168, 1
  resume { ptr, i32 } %170

171:                                              ; preds = %131, %86
  %172 = landingpad { ptr, i32 }
          catch ptr null
  %173 = extractvalue { ptr, i32 } %172, 0
  call void @__clang_call_terminate(ptr %173) #17
  unreachable

174:                                              ; preds = %150
  unreachable
}

; Function Attrs: mustprogress noinline optnone uwtable
define linkonce_odr dso_local void @_ZNKSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEE6resumeEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #5 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.1", ptr %3, i32 0, i32 0
  %5 = load ptr, ptr %4, align 8
  call void @llvm.coro.resume(ptr %5)
  ret void
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local noundef i1 @_ZNKSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEE4doneEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.1", ptr %3, i32 0, i32 0
  %5 = load ptr, ptr %4, align 8
  %6 = call i1 @llvm.coro.done(ptr %5)
  ret i1 %6
}

; Function Attrs: mustprogress noinline optnone uwtable
define linkonce_odr dso_local void @_ZNKSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEE7destroyEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #5 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.1", ptr %3, i32 0, i32 0
  %5 = load ptr, ptr %4, align 8
  call void @llvm.coro.destroy(ptr %5)
  ret void
}

; Function Attrs: nocallback nofree nosync nounwind willreturn memory(argmem: read)
declare token @llvm.coro.id(i32, ptr readnone, ptr readonly captures(none), ptr) #7

; Function Attrs: nounwind
declare i1 @llvm.coro.alloc(token) #8

; Function Attrs: nobuiltin allocsize(0)
declare noundef nonnull ptr @_Znwm(i64 noundef) #9

; Function Attrs: nounwind memory(none)
declare i64 @llvm.coro.size.i64() #2

; Function Attrs: nounwind
declare ptr @llvm.coro.begin(token, ptr writeonly) #8

; Function Attrs: nocallback nofree nosync nounwind willreturn memory(argmem: readwrite)
declare void @llvm.lifetime.start.p0(ptr captures(none)) #10

; Function Attrs: mustprogress noinline optnone uwtable
define linkonce_odr dso_local i64 @_ZN4taskILb1EE12promise_type17get_return_objectEv(ptr noundef nonnull align 1 dereferenceable(1) %0) #5 comdat {
  %2 = alloca %struct.task, align 8
  %3 = alloca ptr, align 8
  store ptr %0, ptr %3, align 8
  %4 = load ptr, ptr %3, align 8
  %5 = getelementptr inbounds nuw %struct.task, ptr %2, i32 0, i32 0
  %6 = call i64 @_ZNSt7__n486116coroutine_handleIN4taskILb1EE12promise_typeEE12from_promiseERS3_(ptr noundef nonnull align 1 dereferenceable(1) %4)
  %7 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %5, i32 0, i32 0
  %8 = inttoptr i64 %6 to ptr
  store ptr %8, ptr %7, align 8
  %9 = getelementptr inbounds nuw %struct.task, ptr %2, i32 0, i32 0
  %10 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %9, i32 0, i32 0
  %11 = load ptr, ptr %10, align 8
  %12 = ptrtoint ptr %11 to i64
  ret i64 %12
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZN4taskILb1EE12promise_type15initial_suspendEv(ptr noundef nonnull align 1 dereferenceable(1) %0) #6 comdat {
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

; Function Attrs: nomerge nounwind
declare token @llvm.coro.save(ptr) #11

; Function Attrs: alwaysinline mustprogress
define internal void @_Z4bodyILb1EE4taskIXT_EEPKc.__await_suspend_wrapper__init(ptr noundef nonnull %0, ptr noundef %1) #12 {
  %3 = alloca ptr, align 8
  %4 = alloca ptr, align 8
  %5 = alloca %"struct.std::__n4861::coroutine_handle.2", align 8
  %6 = alloca %"struct.std::__n4861::coroutine_handle", align 8
  store ptr %0, ptr %3, align 8
  store ptr %1, ptr %4, align 8
  %7 = load ptr, ptr %3, align 8
  %8 = load ptr, ptr %4, align 8
  %9 = call i64 @_ZNSt7__n486116coroutine_handleIN4taskILb1EE12promise_typeEE12from_addressEPv(ptr noundef %8) #8
  %10 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %6, i32 0, i32 0
  %11 = inttoptr i64 %9 to ptr
  store ptr %11, ptr %10, align 8
  %12 = call i64 @_ZNKSt7__n486116coroutine_handleIN4taskILb1EE12promise_typeEEcvNS0_IvEEEv(ptr noundef nonnull align 8 dereferenceable(8) %6) #8
  %13 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.2", ptr %5, i32 0, i32 0
  %14 = inttoptr i64 %12 to ptr
  store ptr %14, ptr %13, align 8
  %15 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.2", ptr %5, i32 0, i32 0
  %16 = load ptr, ptr %15, align 8
  call void @_ZNKSt7__n486113suspend_never13await_suspendENS_16coroutine_handleIvEE(ptr noundef nonnull align 1 dereferenceable(1) %7, ptr %16) #8
  ret void
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZNKSt7__n486113suspend_never13await_suspendENS_16coroutine_handleIvEE(ptr noundef nonnull align 1 dereferenceable(1) %0, ptr %1) #6 comdat {
  %3 = alloca %"struct.std::__n4861::coroutine_handle.2", align 8
  %4 = alloca ptr, align 8
  %5 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.2", ptr %3, i32 0, i32 0
  store ptr %1, ptr %5, align 8
  store ptr %0, ptr %4, align 8
  %6 = load ptr, ptr %4, align 8
  ret void
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local i64 @_ZNSt7__n486116coroutine_handleIN4taskILb1EE12promise_typeEE12from_addressEPv(ptr noundef %0) #6 comdat {
  %2 = alloca %"struct.std::__n4861::coroutine_handle", align 8
  %3 = alloca ptr, align 8
  store ptr %0, ptr %3, align 8
  call void @_ZNSt7__n486116coroutine_handleIN4taskILb1EE12promise_typeEEC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %2) #8
  %4 = load ptr, ptr %3, align 8
  %5 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %2, i32 0, i32 0
  store ptr %4, ptr %5, align 8
  %6 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %2, i32 0, i32 0
  %7 = load ptr, ptr %6, align 8
  %8 = ptrtoint ptr %7 to i64
  ret i64 %8
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local i64 @_ZNKSt7__n486116coroutine_handleIN4taskILb1EE12promise_typeEEcvNS0_IvEEEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #6 comdat {
  %2 = alloca %"struct.std::__n4861::coroutine_handle.2", align 8
  %3 = alloca ptr, align 8
  store ptr %0, ptr %3, align 8
  %4 = load ptr, ptr %3, align 8
  %5 = call noundef ptr @_ZNKSt7__n486116coroutine_handleIN4taskILb1EE12promise_typeEE7addressEv(ptr noundef nonnull align 8 dereferenceable(8) %4) #8
  %6 = call i64 @_ZNSt7__n486116coroutine_handleIvE12from_addressEPv(ptr noundef %5) #8
  %7 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.2", ptr %2, i32 0, i32 0
  %8 = inttoptr i64 %6 to ptr
  store ptr %8, ptr %7, align 8
  %9 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.2", ptr %2, i32 0, i32 0
  %10 = load ptr, ptr %9, align 8
  %11 = ptrtoint ptr %10 to i64
  ret i64 %11
}

declare void @llvm.coro.await.suspend.void(ptr, ptr, ptr)

; Function Attrs: nounwind
declare i8 @llvm.coro.suspend(token, i1) #8

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZNKSt7__n486113suspend_never12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %0) #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  ret void
}

; Function Attrs: nocallback nofree nosync nounwind willreturn memory(argmem: readwrite)
declare void @llvm.lifetime.end.p0(ptr captures(none)) #10

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local noundef i1 @_ZN7thrower11await_readyEv(ptr noundef nonnull align 4 dereferenceable(4) %0) #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  ret i1 false
}

; Function Attrs: alwaysinline mustprogress
define internal noundef ptr @_Z4bodyILb1EE4taskIXT_EEPKc.__await_suspend_wrapper__await(ptr noundef nonnull %0, ptr noundef %1) #12 {
  %3 = alloca ptr, align 8
  %4 = alloca ptr, align 8
  %5 = alloca %"struct.std::__n4861::coroutine_handle.2", align 8
  %6 = alloca %"struct.std::__n4861::coroutine_handle.2", align 8
  %7 = alloca %"struct.std::__n4861::coroutine_handle", align 8
  store ptr %0, ptr %3, align 8
  store ptr %1, ptr %4, align 8
  %8 = load ptr, ptr %3, align 8
  %9 = load ptr, ptr %4, align 8
  %10 = call i64 @_ZNSt7__n486116coroutine_handleIN4taskILb1EE12promise_typeEE12from_addressEPv(ptr noundef %9) #8
  %11 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %7, i32 0, i32 0
  %12 = inttoptr i64 %10 to ptr
  store ptr %12, ptr %11, align 8
  %13 = call i64 @_ZNKSt7__n486116coroutine_handleIN4taskILb1EE12promise_typeEEcvNS0_IvEEEv(ptr noundef nonnull align 8 dereferenceable(8) %7) #8
  %14 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.2", ptr %6, i32 0, i32 0
  %15 = inttoptr i64 %13 to ptr
  store ptr %15, ptr %14, align 8
  %16 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.2", ptr %6, i32 0, i32 0
  %17 = load ptr, ptr %16, align 8
  %18 = call i64 @_ZN7thrower13await_suspendENSt7__n486116coroutine_handleIvEE(ptr noundef nonnull align 4 dereferenceable(4) %8, ptr %17)
  %19 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.2", ptr %5, i32 0, i32 0
  %20 = inttoptr i64 %18 to ptr
  store ptr %20, ptr %19, align 8
  %21 = call noundef ptr @_ZNKSt7__n486116coroutine_handleIvE7addressEv(ptr noundef nonnull align 8 dereferenceable(8) %5) #8
  ret ptr %21
}

; Function Attrs: mustprogress noinline optnone uwtable
define linkonce_odr dso_local i64 @_ZN7thrower13await_suspendENSt7__n486116coroutine_handleIvEE(ptr noundef nonnull align 4 dereferenceable(4) %0, ptr %1) #5 comdat personality ptr @__gxx_personality_v0 {
  %3 = alloca %"struct.std::__n4861::coroutine_handle.2", align 8
  %4 = alloca ptr, align 8
  %5 = alloca ptr, align 8
  %6 = alloca i32, align 4
  %7 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.2", ptr %3, i32 0, i32 0
  store ptr %1, ptr %7, align 8
  store ptr %0, ptr %4, align 8
  %8 = load ptr, ptr %4, align 8
  %9 = call ptr @__cxa_allocate_exception(i64 16) #8
  %10 = getelementptr inbounds nuw %struct.thrower, ptr %8, i32 0, i32 0
  %11 = load i32, ptr %10, align 4
  %12 = icmp eq i32 %11, 1
  %13 = zext i1 %12 to i64
  %14 = select i1 %12, ptr @.str.6, ptr @.str.7
  invoke void @_ZNSt13runtime_errorC1EPKc(ptr noundef nonnull align 8 dereferenceable(16) %9, ptr noundef %14)
          to label %15 unwind label %16

15:                                               ; preds = %2
  call void @__cxa_throw(ptr %9, ptr @_ZTISt13runtime_error, ptr @_ZNSt13runtime_errorD1Ev) #19
  unreachable

16:                                               ; preds = %2
  %17 = landingpad { ptr, i32 }
          cleanup
  %18 = extractvalue { ptr, i32 } %17, 0
  store ptr %18, ptr %5, align 8
  %19 = extractvalue { ptr, i32 } %17, 1
  store i32 %19, ptr %6, align 4
  call void @__cxa_free_exception(ptr %9) #8
  br label %20

20:                                               ; preds = %16
  %21 = load ptr, ptr %5, align 8
  %22 = load i32, ptr %6, align 4
  %23 = insertvalue { ptr, i32 } poison, ptr %21, 0
  %24 = insertvalue { ptr, i32 } %23, i32 %22, 1
  resume { ptr, i32 } %24
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local noundef ptr @_ZNKSt7__n486116coroutine_handleIvE7addressEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.2", ptr %3, i32 0, i32 0
  %5 = load ptr, ptr %4, align 8
  ret ptr %5
}

declare void @llvm.coro.await.suspend.handle(ptr, ptr, ptr)

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZN7thrower12await_resumeEv(ptr noundef nonnull align 4 dereferenceable(4) %0) #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  ret void
}

; Function Attrs: alwaysinline mustprogress
define internal noundef ptr @_Z4bodyILb1EE4taskIXT_EEPKc.__await_suspend_wrapper__await2(ptr noundef nonnull %0, ptr noundef %1) #12 {
  %3 = alloca ptr, align 8
  %4 = alloca ptr, align 8
  %5 = alloca %"struct.std::__n4861::coroutine_handle.2", align 8
  %6 = alloca %"struct.std::__n4861::coroutine_handle.2", align 8
  %7 = alloca %"struct.std::__n4861::coroutine_handle", align 8
  store ptr %0, ptr %3, align 8
  store ptr %1, ptr %4, align 8
  %8 = load ptr, ptr %3, align 8
  %9 = load ptr, ptr %4, align 8
  %10 = call i64 @_ZNSt7__n486116coroutine_handleIN4taskILb1EE12promise_typeEE12from_addressEPv(ptr noundef %9) #8
  %11 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %7, i32 0, i32 0
  %12 = inttoptr i64 %10 to ptr
  store ptr %12, ptr %11, align 8
  %13 = call i64 @_ZNKSt7__n486116coroutine_handleIN4taskILb1EE12promise_typeEEcvNS0_IvEEEv(ptr noundef nonnull align 8 dereferenceable(8) %7) #8
  %14 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.2", ptr %6, i32 0, i32 0
  %15 = inttoptr i64 %13 to ptr
  store ptr %15, ptr %14, align 8
  %16 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.2", ptr %6, i32 0, i32 0
  %17 = load ptr, ptr %16, align 8
  %18 = call i64 @_ZN7thrower13await_suspendENSt7__n486116coroutine_handleIvEE(ptr noundef nonnull align 4 dereferenceable(4) %8, ptr %17)
  %19 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.2", ptr %5, i32 0, i32 0
  %20 = inttoptr i64 %18 to ptr
  store ptr %20, ptr %19, align 8
  %21 = call noundef ptr @_ZNKSt7__n486116coroutine_handleIvE7addressEv(ptr noundef nonnull align 8 dereferenceable(8) %5) #8
  ret ptr %21
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZN4DtorD2Ev(ptr noundef nonnull align 4 dereferenceable(4) %0) unnamed_addr #6 comdat personality ptr @__gxx_personality_v0 {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds nuw %struct.Dtor, ptr %3, i32 0, i32 0
  %5 = load i32, ptr %4, align 4
  %6 = invoke i32 (ptr, ...) @printf(ptr noundef @.str.8, i32 noundef %5)
          to label %7 unwind label %8

7:                                                ; preds = %1
  ret void

8:                                                ; preds = %1
  %9 = landingpad { ptr, i32 }
          catch ptr null
  %10 = extractvalue { ptr, i32 } %9, 0
  call void @__clang_call_terminate(ptr %10) #17
  unreachable
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZN4taskILb1EE12promise_type11return_voidEv(ptr noundef nonnull align 1 dereferenceable(1) %0) #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  ret void
}

; Function Attrs: mustprogress noinline optnone uwtable
define linkonce_odr dso_local void @_ZN4taskILb1EE12promise_type19unhandled_exceptionEv(ptr noundef nonnull align 1 dereferenceable(1) %0) #5 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  call void @__cxa_rethrow() #19
  unreachable
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZN4taskILb1EE12promise_type13final_suspendEv(ptr noundef nonnull align 1 dereferenceable(1) %0) #6 comdat {
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

; Function Attrs: alwaysinline mustprogress
define internal void @_Z4bodyILb1EE4taskIXT_EEPKc.__await_suspend_wrapper__final(ptr noundef nonnull %0, ptr noundef %1) #12 {
  %3 = alloca ptr, align 8
  %4 = alloca ptr, align 8
  %5 = alloca %"struct.std::__n4861::coroutine_handle.2", align 8
  %6 = alloca %"struct.std::__n4861::coroutine_handle", align 8
  store ptr %0, ptr %3, align 8
  store ptr %1, ptr %4, align 8
  %7 = load ptr, ptr %3, align 8
  %8 = load ptr, ptr %4, align 8
  %9 = call i64 @_ZNSt7__n486116coroutine_handleIN4taskILb1EE12promise_typeEE12from_addressEPv(ptr noundef %8) #8
  %10 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %6, i32 0, i32 0
  %11 = inttoptr i64 %9 to ptr
  store ptr %11, ptr %10, align 8
  %12 = call i64 @_ZNKSt7__n486116coroutine_handleIN4taskILb1EE12promise_typeEEcvNS0_IvEEEv(ptr noundef nonnull align 8 dereferenceable(8) %6) #8
  %13 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.2", ptr %5, i32 0, i32 0
  %14 = inttoptr i64 %12 to ptr
  store ptr %14, ptr %13, align 8
  %15 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.2", ptr %5, i32 0, i32 0
  %16 = load ptr, ptr %15, align 8
  call void @_ZNKSt7__n486114suspend_always13await_suspendENS_16coroutine_handleIvEE(ptr noundef nonnull align 1 dereferenceable(1) %7, ptr %16) #8
  ret void
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZNKSt7__n486114suspend_always13await_suspendENS_16coroutine_handleIvEE(ptr noundef nonnull align 1 dereferenceable(1) %0, ptr %1) #6 comdat {
  %3 = alloca %"struct.std::__n4861::coroutine_handle.2", align 8
  %4 = alloca ptr, align 8
  %5 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.2", ptr %3, i32 0, i32 0
  store ptr %1, ptr %5, align 8
  store ptr %0, ptr %4, align 8
  %6 = load ptr, ptr %4, align 8
  ret void
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZNKSt7__n486114suspend_always12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %0) #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  ret void
}

; Function Attrs: nounwind
declare void @llvm.coro.end(ptr, i1, token) #8

; Function Attrs: nounwind memory(none)
declare i1 @llvm.coro.is_in_ramp() #2

; Function Attrs: nobuiltin nounwind
declare void @_ZdlPvm(ptr noundef, i64 noundef) #13

; Function Attrs: nounwind memory(argmem: read)
declare ptr @llvm.coro.free(token, ptr readonly captures(none)) #14

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local i64 @_ZNSt7__n486116coroutine_handleIN4taskILb1EE12promise_typeEE12from_promiseERS3_(ptr noundef nonnull align 1 dereferenceable(1) %0) #6 comdat {
  %2 = alloca %"struct.std::__n4861::coroutine_handle", align 8
  %3 = alloca ptr, align 8
  store ptr %0, ptr %3, align 8
  call void @_ZNSt7__n486116coroutine_handleIN4taskILb1EE12promise_typeEEC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %2) #8
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
define linkonce_odr dso_local void @_ZNSt7__n486116coroutine_handleIN4taskILb1EE12promise_typeEEC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %0) unnamed_addr #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %3, i32 0, i32 0
  store ptr null, ptr %4, align 8
  ret void
}

; Function Attrs: nounwind memory(none)
declare ptr @llvm.coro.promise(ptr captures(none), i32, i1) #2

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local i64 @_ZNSt7__n486116coroutine_handleIvE12from_addressEPv(ptr noundef %0) #6 comdat {
  %2 = alloca %"struct.std::__n4861::coroutine_handle.2", align 8
  %3 = alloca ptr, align 8
  store ptr %0, ptr %3, align 8
  call void @_ZNSt7__n486116coroutine_handleIvEC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %2) #8
  %4 = load ptr, ptr %3, align 8
  %5 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.2", ptr %2, i32 0, i32 0
  store ptr %4, ptr %5, align 8
  %6 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.2", ptr %2, i32 0, i32 0
  %7 = load ptr, ptr %6, align 8
  %8 = ptrtoint ptr %7 to i64
  ret i64 %8
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local noundef ptr @_ZNKSt7__n486116coroutine_handleIN4taskILb1EE12promise_typeEE7addressEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %3, i32 0, i32 0
  %5 = load ptr, ptr %4, align 8
  ret ptr %5
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZNSt7__n486116coroutine_handleIvEC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %0) unnamed_addr #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.2", ptr %3, i32 0, i32 0
  store ptr null, ptr %4, align 8
  ret void
}

declare ptr @__cxa_allocate_exception(i64)

declare void @_ZNSt13runtime_errorC1EPKc(ptr noundef nonnull align 8 dereferenceable(16), ptr noundef) unnamed_addr #3

declare void @__cxa_free_exception(ptr)

; Function Attrs: nounwind
declare void @_ZNSt13runtime_errorD1Ev(ptr noundef nonnull align 8 dereferenceable(16)) unnamed_addr #15

declare void @__cxa_throw(ptr, ptr, ptr)

declare void @__cxa_rethrow()

; Function Attrs: mustprogress noinline optnone uwtable
define linkonce_odr dso_local i64 @_ZN4taskILb0EE12promise_type17get_return_objectEv(ptr noundef nonnull align 1 dereferenceable(1) %0) #5 comdat {
  %2 = alloca %struct.task.0, align 8
  %3 = alloca ptr, align 8
  store ptr %0, ptr %3, align 8
  %4 = load ptr, ptr %3, align 8
  %5 = getelementptr inbounds nuw %struct.task.0, ptr %2, i32 0, i32 0
  %6 = call i64 @_ZNSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEE12from_promiseERS3_(ptr noundef nonnull align 1 dereferenceable(1) %4)
  %7 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.1", ptr %5, i32 0, i32 0
  %8 = inttoptr i64 %6 to ptr
  store ptr %8, ptr %7, align 8
  %9 = getelementptr inbounds nuw %struct.task.0, ptr %2, i32 0, i32 0
  %10 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.1", ptr %9, i32 0, i32 0
  %11 = load ptr, ptr %10, align 8
  %12 = ptrtoint ptr %11 to i64
  ret i64 %12
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZN4taskILb0EE12promise_type15initial_suspendEv(ptr noundef nonnull align 1 dereferenceable(1) %0) #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  ret void
}

; Function Attrs: alwaysinline mustprogress
define internal void @_Z4bodyILb0EE4taskIXT_EEPKc.__await_suspend_wrapper__init(ptr noundef nonnull %0, ptr noundef %1) #12 {
  %3 = alloca ptr, align 8
  %4 = alloca ptr, align 8
  %5 = alloca %"struct.std::__n4861::coroutine_handle.2", align 8
  %6 = alloca %"struct.std::__n4861::coroutine_handle.1", align 8
  store ptr %0, ptr %3, align 8
  store ptr %1, ptr %4, align 8
  %7 = load ptr, ptr %3, align 8
  %8 = load ptr, ptr %4, align 8
  %9 = call i64 @_ZNSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEE12from_addressEPv(ptr noundef %8) #8
  %10 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.1", ptr %6, i32 0, i32 0
  %11 = inttoptr i64 %9 to ptr
  store ptr %11, ptr %10, align 8
  %12 = call i64 @_ZNKSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEEcvNS0_IvEEEv(ptr noundef nonnull align 8 dereferenceable(8) %6) #8
  %13 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.2", ptr %5, i32 0, i32 0
  %14 = inttoptr i64 %12 to ptr
  store ptr %14, ptr %13, align 8
  %15 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.2", ptr %5, i32 0, i32 0
  %16 = load ptr, ptr %15, align 8
  call void @_ZNKSt7__n486114suspend_always13await_suspendENS_16coroutine_handleIvEE(ptr noundef nonnull align 1 dereferenceable(1) %7, ptr %16) #8
  ret void
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local i64 @_ZNSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEE12from_addressEPv(ptr noundef %0) #6 comdat {
  %2 = alloca %"struct.std::__n4861::coroutine_handle.1", align 8
  %3 = alloca ptr, align 8
  store ptr %0, ptr %3, align 8
  call void @_ZNSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEEC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %2) #8
  %4 = load ptr, ptr %3, align 8
  %5 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.1", ptr %2, i32 0, i32 0
  store ptr %4, ptr %5, align 8
  %6 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.1", ptr %2, i32 0, i32 0
  %7 = load ptr, ptr %6, align 8
  %8 = ptrtoint ptr %7 to i64
  ret i64 %8
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local i64 @_ZNKSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEEcvNS0_IvEEEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #6 comdat {
  %2 = alloca %"struct.std::__n4861::coroutine_handle.2", align 8
  %3 = alloca ptr, align 8
  store ptr %0, ptr %3, align 8
  %4 = load ptr, ptr %3, align 8
  %5 = call noundef ptr @_ZNKSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEE7addressEv(ptr noundef nonnull align 8 dereferenceable(8) %4) #8
  %6 = call i64 @_ZNSt7__n486116coroutine_handleIvE12from_addressEPv(ptr noundef %5) #8
  %7 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.2", ptr %2, i32 0, i32 0
  %8 = inttoptr i64 %6 to ptr
  store ptr %8, ptr %7, align 8
  %9 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.2", ptr %2, i32 0, i32 0
  %10 = load ptr, ptr %9, align 8
  %11 = ptrtoint ptr %10 to i64
  ret i64 %11
}

; Function Attrs: alwaysinline mustprogress
define internal noundef ptr @_Z4bodyILb0EE4taskIXT_EEPKc.__await_suspend_wrapper__await(ptr noundef nonnull %0, ptr noundef %1) #12 {
  %3 = alloca ptr, align 8
  %4 = alloca ptr, align 8
  %5 = alloca %"struct.std::__n4861::coroutine_handle.2", align 8
  %6 = alloca %"struct.std::__n4861::coroutine_handle.2", align 8
  %7 = alloca %"struct.std::__n4861::coroutine_handle.1", align 8
  store ptr %0, ptr %3, align 8
  store ptr %1, ptr %4, align 8
  %8 = load ptr, ptr %3, align 8
  %9 = load ptr, ptr %4, align 8
  %10 = call i64 @_ZNSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEE12from_addressEPv(ptr noundef %9) #8
  %11 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.1", ptr %7, i32 0, i32 0
  %12 = inttoptr i64 %10 to ptr
  store ptr %12, ptr %11, align 8
  %13 = call i64 @_ZNKSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEEcvNS0_IvEEEv(ptr noundef nonnull align 8 dereferenceable(8) %7) #8
  %14 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.2", ptr %6, i32 0, i32 0
  %15 = inttoptr i64 %13 to ptr
  store ptr %15, ptr %14, align 8
  %16 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.2", ptr %6, i32 0, i32 0
  %17 = load ptr, ptr %16, align 8
  %18 = call i64 @_ZN7thrower13await_suspendENSt7__n486116coroutine_handleIvEE(ptr noundef nonnull align 4 dereferenceable(4) %8, ptr %17)
  %19 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.2", ptr %5, i32 0, i32 0
  %20 = inttoptr i64 %18 to ptr
  store ptr %20, ptr %19, align 8
  %21 = call noundef ptr @_ZNKSt7__n486116coroutine_handleIvE7addressEv(ptr noundef nonnull align 8 dereferenceable(8) %5) #8
  ret ptr %21
}

; Function Attrs: alwaysinline mustprogress
define internal noundef ptr @_Z4bodyILb0EE4taskIXT_EEPKc.__await_suspend_wrapper__await2(ptr noundef nonnull %0, ptr noundef %1) #12 {
  %3 = alloca ptr, align 8
  %4 = alloca ptr, align 8
  %5 = alloca %"struct.std::__n4861::coroutine_handle.2", align 8
  %6 = alloca %"struct.std::__n4861::coroutine_handle.2", align 8
  %7 = alloca %"struct.std::__n4861::coroutine_handle.1", align 8
  store ptr %0, ptr %3, align 8
  store ptr %1, ptr %4, align 8
  %8 = load ptr, ptr %3, align 8
  %9 = load ptr, ptr %4, align 8
  %10 = call i64 @_ZNSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEE12from_addressEPv(ptr noundef %9) #8
  %11 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.1", ptr %7, i32 0, i32 0
  %12 = inttoptr i64 %10 to ptr
  store ptr %12, ptr %11, align 8
  %13 = call i64 @_ZNKSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEEcvNS0_IvEEEv(ptr noundef nonnull align 8 dereferenceable(8) %7) #8
  %14 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.2", ptr %6, i32 0, i32 0
  %15 = inttoptr i64 %13 to ptr
  store ptr %15, ptr %14, align 8
  %16 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.2", ptr %6, i32 0, i32 0
  %17 = load ptr, ptr %16, align 8
  %18 = call i64 @_ZN7thrower13await_suspendENSt7__n486116coroutine_handleIvEE(ptr noundef nonnull align 4 dereferenceable(4) %8, ptr %17)
  %19 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.2", ptr %5, i32 0, i32 0
  %20 = inttoptr i64 %18 to ptr
  store ptr %20, ptr %19, align 8
  %21 = call noundef ptr @_ZNKSt7__n486116coroutine_handleIvE7addressEv(ptr noundef nonnull align 8 dereferenceable(8) %5) #8
  ret ptr %21
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZN4taskILb0EE12promise_type11return_voidEv(ptr noundef nonnull align 1 dereferenceable(1) %0) #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  ret void
}

; Function Attrs: mustprogress noinline optnone uwtable
define linkonce_odr dso_local void @_ZN4taskILb0EE12promise_type19unhandled_exceptionEv(ptr noundef nonnull align 1 dereferenceable(1) %0) #5 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  call void @__cxa_rethrow() #19
  unreachable
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZN4taskILb0EE12promise_type13final_suspendEv(ptr noundef nonnull align 1 dereferenceable(1) %0) #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  ret void
}

; Function Attrs: alwaysinline mustprogress
define internal void @_Z4bodyILb0EE4taskIXT_EEPKc.__await_suspend_wrapper__final(ptr noundef nonnull %0, ptr noundef %1) #12 {
  %3 = alloca ptr, align 8
  %4 = alloca ptr, align 8
  %5 = alloca %"struct.std::__n4861::coroutine_handle.2", align 8
  %6 = alloca %"struct.std::__n4861::coroutine_handle.1", align 8
  store ptr %0, ptr %3, align 8
  store ptr %1, ptr %4, align 8
  %7 = load ptr, ptr %3, align 8
  %8 = load ptr, ptr %4, align 8
  %9 = call i64 @_ZNSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEE12from_addressEPv(ptr noundef %8) #8
  %10 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.1", ptr %6, i32 0, i32 0
  %11 = inttoptr i64 %9 to ptr
  store ptr %11, ptr %10, align 8
  %12 = call i64 @_ZNKSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEEcvNS0_IvEEEv(ptr noundef nonnull align 8 dereferenceable(8) %6) #8
  %13 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.2", ptr %5, i32 0, i32 0
  %14 = inttoptr i64 %12 to ptr
  store ptr %14, ptr %13, align 8
  %15 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.2", ptr %5, i32 0, i32 0
  %16 = load ptr, ptr %15, align 8
  call void @_ZNKSt7__n486114suspend_always13await_suspendENS_16coroutine_handleIvEE(ptr noundef nonnull align 1 dereferenceable(1) %7, ptr %16) #8
  ret void
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local i64 @_ZNSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEE12from_promiseERS3_(ptr noundef nonnull align 1 dereferenceable(1) %0) #6 comdat {
  %2 = alloca %"struct.std::__n4861::coroutine_handle.1", align 8
  %3 = alloca ptr, align 8
  store ptr %0, ptr %3, align 8
  call void @_ZNSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEEC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %2) #8
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
define linkonce_odr dso_local void @_ZNSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEEC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %0) unnamed_addr #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.1", ptr %3, i32 0, i32 0
  store ptr null, ptr %4, align 8
  ret void
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local noundef ptr @_ZNKSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEE7addressEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.1", ptr %3, i32 0, i32 0
  %5 = load ptr, ptr %4, align 8
  ret ptr %5
}

declare void @llvm.coro.resume(ptr)

; Function Attrs: nounwind memory(argmem: readwrite)
declare i1 @llvm.coro.done(ptr readonly captures(none)) #16

declare void @llvm.coro.destroy(ptr)

attributes #0 = { mustprogress noinline norecurse optnone uwtable "frame-pointer"="non-leaf-no-reserve" "no-trapping-math"="true" "stack-protector-buffer-size"="8" }
attributes #1 = { mustprogress noinline optnone presplitcoroutine uwtable "frame-pointer"="non-leaf-no-reserve" "no-trapping-math"="true" "stack-protector-buffer-size"="8" }
attributes #2 = { nounwind memory(none) }
attributes #3 = { "frame-pointer"="non-leaf-no-reserve" "no-trapping-math"="true" "stack-protector-buffer-size"="8" }
attributes #4 = { noinline noreturn nounwind uwtable "frame-pointer"="non-leaf-no-reserve" "no-trapping-math"="true" "stack-protector-buffer-size"="8" }
attributes #5 = { mustprogress noinline optnone uwtable "frame-pointer"="non-leaf-no-reserve" "no-trapping-math"="true" "stack-protector-buffer-size"="8" }
attributes #6 = { mustprogress noinline nounwind optnone uwtable "frame-pointer"="non-leaf-no-reserve" "no-trapping-math"="true" "stack-protector-buffer-size"="8" }
attributes #7 = { nocallback nofree nosync nounwind willreturn memory(argmem: read) }
attributes #8 = { nounwind }
attributes #9 = { nobuiltin allocsize(0) "frame-pointer"="non-leaf-no-reserve" "no-trapping-math"="true" "stack-protector-buffer-size"="8" }
attributes #10 = { nocallback nofree nosync nounwind willreturn memory(argmem: readwrite) }
attributes #11 = { nomerge nounwind }
attributes #12 = { alwaysinline mustprogress "sample-profile-suffix-elision-policy"="selected" }
attributes #13 = { nobuiltin nounwind "frame-pointer"="non-leaf-no-reserve" "no-trapping-math"="true" "stack-protector-buffer-size"="8" }
attributes #14 = { nounwind memory(argmem: read) }
attributes #15 = { nounwind "frame-pointer"="non-leaf-no-reserve" "no-trapping-math"="true" "stack-protector-buffer-size"="8" }
attributes #16 = { nounwind memory(argmem: readwrite) }
attributes #17 = { noreturn nounwind }
attributes #18 = { allocsize(0) }
attributes #19 = { noreturn }

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
