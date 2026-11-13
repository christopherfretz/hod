; clang++-19 -std=c++20 -O0 -S -emit-llvm -Xclang -disable-llvm-passes
; (aarch64-unknown-linux-gnu; the target triple and datalayout removed) of:
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
;@ dialect: 19
;@ link: c++
;@ unsupported(x86_64): *
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
  %10 = getelementptr inbounds %struct.task, ptr %4, i32 0, i32 0
  %11 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %10, i32 0, i32 0
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
  %24 = load ptr, ptr %5, align 8
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
  %33 = getelementptr inbounds %struct.task.0, ptr %6, i32 0, i32 0
  %34 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.1", ptr %33, i32 0, i32 0
  %35 = inttoptr i64 %32 to ptr
  store ptr %35, ptr %34, align 8
  %36 = getelementptr inbounds %struct.task.0, ptr %6, i32 0, i32 0
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
  %54 = load ptr, ptr %7, align 8
  %55 = load ptr, ptr %54, align 8
  %56 = getelementptr inbounds ptr, ptr %55, i64 2
  %57 = load ptr, ptr %56, align 8
  %58 = call noundef ptr %57(ptr noundef nonnull align 8 dereferenceable(8) %54) #8
  %59 = getelementptr inbounds %struct.task.0, ptr %6, i32 0, i32 0
  %60 = call noundef i1 @_ZNKSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEE4doneEv(ptr noundef nonnull align 8 dereferenceable(8) %59) #8
  %61 = zext i1 %60 to i32
  %62 = invoke i32 (ptr, ...) @printf(ptr noundef @.str.3, ptr noundef %58, i32 noundef %61)
          to label %63 unwind label %67

63:                                               ; preds = %51
  call void @__cxa_end_catch()
  br label %64

64:                                               ; preds = %63, %37
  %65 = getelementptr inbounds %struct.task.0, ptr %6, i32 0, i32 0
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
  %3 = alloca ptr, align 8
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
  %17 = bitcast ptr %5 to ptr
  %18 = call token @llvm.coro.id(i32 16, ptr %17, ptr null, ptr null)
  %19 = call i1 @llvm.coro.alloc(token %18)
  br i1 %19, label %20, label %23

20:                                               ; preds = %1
  %21 = call i64 @llvm.coro.size.i64()
  %22 = call noalias noundef nonnull ptr @_Znwm(i64 noundef %21) #18
  br label %23

23:                                               ; preds = %20, %1
  %24 = phi ptr [ null, %1 ], [ %22, %20 ]
  %25 = call ptr @llvm.coro.begin(token %18, ptr %24)
  call void @llvm.lifetime.start.p0(i64 8, ptr %4) #8
  %26 = load ptr, ptr %3, align 8
  store ptr %26, ptr %4, align 8
  call void @llvm.lifetime.start.p0(i64 1, ptr %5) #8
  %27 = invoke i64 @_ZN4taskILb1EE12promise_type17get_return_objectEv(ptr noundef nonnull align 1 dereferenceable(1) %5)
          to label %28 unwind label %37

28:                                               ; preds = %23
  %29 = getelementptr inbounds %struct.task, ptr %2, i32 0, i32 0
  %30 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %29, i32 0, i32 0
  %31 = inttoptr i64 %27 to ptr
  store ptr %31, ptr %30, align 8
  call void @llvm.lifetime.start.p0(i64 1, ptr %8) #8
  call void @_ZN4taskILb1EE12promise_type15initial_suspendEv(ptr noundef nonnull align 1 dereferenceable(1) %5) #8
  %32 = call noundef i1 @_ZNKSt7__n486113suspend_never11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %8) #8
  br i1 %32, label %41, label %33

33:                                               ; preds = %28
  %34 = call token @llvm.coro.save(ptr null)
  call void @llvm.coro.await.suspend.void(ptr %8, ptr %25, ptr @_Z4bodyILb1EE4taskIXT_EEPKc.__await_suspend_wrapper__init) #8
  %35 = call i8 @llvm.coro.suspend(token %34, i1 false)
  switch i8 %35, label %157 [
    i8 0, label %41
    i8 1, label %36
  ]

36:                                               ; preds = %33
  br label %42

37:                                               ; preds = %23
  %38 = landingpad { ptr, i32 }
          cleanup
  %39 = extractvalue { ptr, i32 } %38, 0
  store ptr %39, ptr %6, align 8
  %40 = extractvalue { ptr, i32 } %38, 1
  store i32 %40, ptr %7, align 4
  br label %166

41:                                               ; preds = %33, %28
  call void @_ZNKSt7__n486113suspend_never12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %8) #8
  br label %42

42:                                               ; preds = %41, %36
  %43 = phi i32 [ 0, %41 ], [ 2, %36 ]
  call void @llvm.lifetime.end.p0(i64 1, ptr %8) #8
  switch i32 %43, label %149 [
    i32 0, label %44
  ]

44:                                               ; preds = %42
  call void @llvm.lifetime.start.p0(i64 4, ptr %10) #8
  %45 = getelementptr inbounds %struct.Dtor, ptr %10, i32 0, i32 0
  store i32 1, ptr %45, align 4
  call void @llvm.lifetime.start.p0(i64 4, ptr %11) #8
  store i32 40, ptr %11, align 4
  call void @llvm.lifetime.start.p0(i64 4, ptr %12) #8
  %46 = getelementptr inbounds %struct.thrower, ptr %12, i32 0, i32 0
  store i32 1, ptr %46, align 4
  %47 = invoke noundef i1 @_ZN7thrower11await_readyEv(ptr noundef nonnull align 4 dereferenceable(4) %12)
          to label %48 unwind label %54

48:                                               ; preds = %44
  br i1 %47, label %85, label %49

49:                                               ; preds = %48
  %50 = call token @llvm.coro.save(ptr null)
  invoke void @llvm.coro.await.suspend.handle(ptr %12, ptr %25, ptr @_Z4bodyILb1EE4taskIXT_EEPKc.__await_suspend_wrapper__await)
          to label %51 unwind label %54

51:                                               ; preds = %49
  %52 = call i8 @llvm.coro.suspend(token %50, i1 false)
  switch i8 %52, label %157 [
    i8 0, label %85
    i8 1, label %53
  ]

53:                                               ; preds = %51
  br label %87

54:                                               ; preds = %85, %49, %44
  %55 = landingpad { ptr, i32 }
          catch ptr @_ZTISt9exception
          catch ptr null
  %56 = extractvalue { ptr, i32 } %55, 0
  store ptr %56, ptr %6, align 8
  %57 = extractvalue { ptr, i32 } %55, 1
  store i32 %57, ptr %7, align 4
  call void @llvm.lifetime.end.p0(i64 4, ptr %12) #8
  br label %58

58:                                               ; preds = %54
  %59 = load i32, ptr %7, align 4
  %60 = call i32 @llvm.eh.typeid.for.p0(ptr @_ZTISt9exception) #8
  %61 = icmp eq i32 %59, %60
  br i1 %61, label %62, label %119

62:                                               ; preds = %58
  call void @llvm.lifetime.start.p0(i64 8, ptr %13) #8
  %63 = load ptr, ptr %6, align 8
  %64 = call ptr @__cxa_begin_catch(ptr %63) #8
  store ptr %64, ptr %13, align 8
  %65 = load ptr, ptr %4, align 8
  %66 = load ptr, ptr %13, align 8
  %67 = load ptr, ptr %66, align 8
  %68 = getelementptr inbounds ptr, ptr %67, i64 2
  %69 = load ptr, ptr %68, align 8
  %70 = call noundef ptr %69(ptr noundef nonnull align 8 dereferenceable(8) %66) #8
  %71 = load i32, ptr %11, align 4
  %72 = add nsw i32 %71, 2
  %73 = invoke i32 (ptr, ...) @printf(ptr noundef @.str.4, ptr noundef %65, ptr noundef %70, i32 noundef %72)
          to label %74 unwind label %90

74:                                               ; preds = %62
  invoke void @__cxa_end_catch()
          to label %75 unwind label %94

75:                                               ; preds = %74
  call void @llvm.lifetime.end.p0(i64 8, ptr %13) #8
  br label %76

76:                                               ; preds = %75, %89
  call void @llvm.lifetime.start.p0(i64 4, ptr %14) #8
  %77 = getelementptr inbounds %struct.thrower, ptr %14, i32 0, i32 0
  store i32 2, ptr %77, align 4
  %78 = invoke noundef i1 @_ZN7thrower11await_readyEv(ptr noundef nonnull align 4 dereferenceable(4) %14)
          to label %79 unwind label %100

79:                                               ; preds = %76
  br i1 %78, label %104, label %80

80:                                               ; preds = %79
  %81 = call token @llvm.coro.save(ptr null)
  invoke void @llvm.coro.await.suspend.handle(ptr %14, ptr %25, ptr @_Z4bodyILb1EE4taskIXT_EEPKc.__await_suspend_wrapper__await2)
          to label %82 unwind label %100

82:                                               ; preds = %80
  %83 = call i8 @llvm.coro.suspend(token %81, i1 false)
  switch i8 %83, label %157 [
    i8 0, label %104
    i8 1, label %84
  ]

84:                                               ; preds = %82
  br label %106

85:                                               ; preds = %51, %48
  invoke void @_ZN7thrower12await_resumeEv(ptr noundef nonnull align 4 dereferenceable(4) %12)
          to label %86 unwind label %54

86:                                               ; preds = %85
  br label %87

87:                                               ; preds = %86, %53
  %88 = phi i32 [ 0, %86 ], [ 2, %53 ]
  call void @llvm.lifetime.end.p0(i64 4, ptr %12) #8
  switch i32 %88, label %111 [
    i32 0, label %89
  ]

89:                                               ; preds = %87
  br label %76

90:                                               ; preds = %62
  %91 = landingpad { ptr, i32 }
          catch ptr null
  %92 = extractvalue { ptr, i32 } %91, 0
  store ptr %92, ptr %6, align 8
  %93 = extractvalue { ptr, i32 } %91, 1
  store i32 %93, ptr %7, align 4
  invoke void @__cxa_end_catch()
          to label %98 unwind label %177

94:                                               ; preds = %74
  %95 = landingpad { ptr, i32 }
          catch ptr null
  %96 = extractvalue { ptr, i32 } %95, 0
  store ptr %96, ptr %6, align 8
  %97 = extractvalue { ptr, i32 } %95, 1
  store i32 %97, ptr %7, align 4
  br label %99

98:                                               ; preds = %90
  br label %99

99:                                               ; preds = %98, %94
  call void @llvm.lifetime.end.p0(i64 8, ptr %13) #8
  br label %119

100:                                              ; preds = %104, %80, %76
  %101 = landingpad { ptr, i32 }
          catch ptr null
  %102 = extractvalue { ptr, i32 } %101, 0
  store ptr %102, ptr %6, align 8
  %103 = extractvalue { ptr, i32 } %101, 1
  store i32 %103, ptr %7, align 4
  call void @llvm.lifetime.end.p0(i64 4, ptr %14) #8
  br label %119

104:                                              ; preds = %82, %79
  invoke void @_ZN7thrower12await_resumeEv(ptr noundef nonnull align 4 dereferenceable(4) %14)
          to label %105 unwind label %100

105:                                              ; preds = %104
  br label %106

106:                                              ; preds = %105, %84
  %107 = phi i32 [ 0, %105 ], [ 2, %84 ]
  call void @llvm.lifetime.end.p0(i64 4, ptr %14) #8
  switch i32 %107, label %111 [
    i32 0, label %108
  ]

108:                                              ; preds = %106
  %109 = invoke i32 (ptr, ...) @printf(ptr noundef @.str.5)
          to label %110 unwind label %115

110:                                              ; preds = %108
  br label %111

111:                                              ; preds = %110, %106, %87
  %112 = phi i32 [ %88, %87 ], [ %107, %106 ], [ 0, %110 ]
  call void @llvm.lifetime.end.p0(i64 4, ptr %11) #8
  call void @_ZN4DtorD2Ev(ptr noundef nonnull align 4 dereferenceable(4) %10) #8
  call void @llvm.lifetime.end.p0(i64 4, ptr %10) #8
  switch i32 %112, label %149 [
    i32 0, label %113
  ]

113:                                              ; preds = %111
  invoke void @_ZN4taskILb1EE12promise_type11return_voidEv(ptr noundef nonnull align 1 dereferenceable(1) %5)
          to label %114 unwind label %120

114:                                              ; preds = %113
  br label %130

115:                                              ; preds = %108
  %116 = landingpad { ptr, i32 }
          catch ptr null
  %117 = extractvalue { ptr, i32 } %116, 0
  store ptr %117, ptr %6, align 8
  %118 = extractvalue { ptr, i32 } %116, 1
  store i32 %118, ptr %7, align 4
  br label %119

119:                                              ; preds = %115, %100, %99, %58
  call void @llvm.lifetime.end.p0(i64 4, ptr %11) #8
  call void @_ZN4DtorD2Ev(ptr noundef nonnull align 4 dereferenceable(4) %10) #8
  call void @llvm.lifetime.end.p0(i64 4, ptr %10) #8
  br label %124

120:                                              ; preds = %113
  %121 = landingpad { ptr, i32 }
          catch ptr null
  %122 = extractvalue { ptr, i32 } %121, 0
  store ptr %122, ptr %6, align 8
  %123 = extractvalue { ptr, i32 } %121, 1
  store i32 %123, ptr %7, align 4
  br label %124

124:                                              ; preds = %120, %119
  %125 = load ptr, ptr %6, align 8
  %126 = call ptr @__cxa_begin_catch(ptr %125) #8
  invoke void @_ZN4taskILb1EE12promise_type19unhandled_exceptionEv(ptr noundef nonnull align 1 dereferenceable(1) %5)
          to label %127 unwind label %136

127:                                              ; preds = %124
  invoke void @__cxa_end_catch()
          to label %128 unwind label %140

128:                                              ; preds = %127
  br label %129

129:                                              ; preds = %128
  br label %130

130:                                              ; preds = %129, %114
  call void @llvm.lifetime.start.p0(i64 1, ptr %15) #8
  call void @_ZN4taskILb1EE12promise_type13final_suspendEv(ptr noundef nonnull align 1 dereferenceable(1) %5) #8
  %131 = call noundef i1 @_ZNKSt7__n486114suspend_always11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %15) #8
  br i1 %131, label %145, label %132

132:                                              ; preds = %130
  %133 = call token @llvm.coro.save(ptr null)
  call void @llvm.coro.await.suspend.void(ptr %15, ptr %25, ptr @_Z4bodyILb1EE4taskIXT_EEPKc.__await_suspend_wrapper__final) #8
  %134 = call i8 @llvm.coro.suspend(token %133, i1 true)
  switch i8 %134, label %157 [
    i8 0, label %145
    i8 1, label %135
  ]

135:                                              ; preds = %132
  br label %146

136:                                              ; preds = %124
  %137 = landingpad { ptr, i32 }
          cleanup
  %138 = extractvalue { ptr, i32 } %137, 0
  store ptr %138, ptr %6, align 8
  %139 = extractvalue { ptr, i32 } %137, 1
  store i32 %139, ptr %7, align 4
  invoke void @__cxa_end_catch()
          to label %144 unwind label %177

140:                                              ; preds = %127
  %141 = landingpad { ptr, i32 }
          cleanup
  %142 = extractvalue { ptr, i32 } %141, 0
  store ptr %142, ptr %6, align 8
  %143 = extractvalue { ptr, i32 } %141, 1
  store i32 %143, ptr %7, align 4
  br label %163

144:                                              ; preds = %136
  br label %163

145:                                              ; preds = %132, %130
  call void @_ZNKSt7__n486114suspend_always12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %15) #8
  br label %146

146:                                              ; preds = %145, %135
  %147 = phi i32 [ 0, %145 ], [ 2, %135 ]
  call void @llvm.lifetime.end.p0(i64 1, ptr %15) #8
  switch i32 %147, label %149 [
    i32 0, label %148
  ]

148:                                              ; preds = %146
  br label %149

149:                                              ; preds = %148, %146, %111, %42
  %150 = phi i32 [ %43, %42 ], [ %112, %111 ], [ %147, %146 ], [ 0, %148 ]
  call void @llvm.lifetime.end.p0(i64 1, ptr %5) #8
  call void @llvm.lifetime.end.p0(i64 8, ptr %4) #8
  %151 = call ptr @llvm.coro.free(token %18, ptr %25)
  %152 = icmp ne ptr %151, null
  br i1 %152, label %153, label %155

153:                                              ; preds = %149
  %154 = call i64 @llvm.coro.size.i64()
  call void @_ZdlPvm(ptr noundef %151, i64 noundef %154) #8
  br label %155

155:                                              ; preds = %149, %153
  switch i32 %150, label %180 [
    i32 0, label %156
    i32 2, label %157
  ]

156:                                              ; preds = %155
  br label %157

157:                                              ; preds = %156, %155, %132, %82, %51, %33
  %158 = call i1 @llvm.coro.end(ptr null, i1 false, token none)
  %159 = getelementptr inbounds %struct.task, ptr %2, i32 0, i32 0
  %160 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %159, i32 0, i32 0
  %161 = load ptr, ptr %160, align 8
  %162 = ptrtoint ptr %161 to i64
  ret i64 %162

163:                                              ; preds = %144, %140
  %164 = call i1 @llvm.coro.end(ptr null, i1 true, token none)
  br i1 %164, label %172, label %165

165:                                              ; preds = %163
  br label %166

166:                                              ; preds = %165, %37
  call void @llvm.lifetime.end.p0(i64 1, ptr %5) #8
  call void @llvm.lifetime.end.p0(i64 8, ptr %4) #8
  %167 = call ptr @llvm.coro.free(token %18, ptr %25)
  %168 = icmp ne ptr %167, null
  br i1 %168, label %169, label %171

169:                                              ; preds = %166
  %170 = call i64 @llvm.coro.size.i64()
  call void @_ZdlPvm(ptr noundef %167, i64 noundef %170) #8
  br label %171

171:                                              ; preds = %166, %169
  br label %172

172:                                              ; preds = %171, %163
  %173 = load ptr, ptr %6, align 8
  %174 = load i32, ptr %7, align 4
  %175 = insertvalue { ptr, i32 } poison, ptr %173, 0
  %176 = insertvalue { ptr, i32 } %175, i32 %174, 1
  resume { ptr, i32 } %176

177:                                              ; preds = %136, %90
  %178 = landingpad { ptr, i32 }
          catch ptr null
  %179 = extractvalue { ptr, i32 } %178, 0
  call void @__clang_call_terminate(ptr %179) #17
  unreachable

180:                                              ; preds = %155
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
  %3 = alloca ptr, align 8
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
  %17 = bitcast ptr %5 to ptr
  %18 = call token @llvm.coro.id(i32 16, ptr %17, ptr null, ptr null)
  %19 = call i1 @llvm.coro.alloc(token %18)
  br i1 %19, label %20, label %23

20:                                               ; preds = %1
  %21 = call i64 @llvm.coro.size.i64()
  %22 = call noalias noundef nonnull ptr @_Znwm(i64 noundef %21) #18
  br label %23

23:                                               ; preds = %20, %1
  %24 = phi ptr [ null, %1 ], [ %22, %20 ]
  %25 = call ptr @llvm.coro.begin(token %18, ptr %24)
  call void @llvm.lifetime.start.p0(i64 8, ptr %4) #8
  %26 = load ptr, ptr %3, align 8
  store ptr %26, ptr %4, align 8
  call void @llvm.lifetime.start.p0(i64 1, ptr %5) #8
  %27 = invoke i64 @_ZN4taskILb0EE12promise_type17get_return_objectEv(ptr noundef nonnull align 1 dereferenceable(1) %5)
          to label %28 unwind label %37

28:                                               ; preds = %23
  %29 = getelementptr inbounds %struct.task.0, ptr %2, i32 0, i32 0
  %30 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.1", ptr %29, i32 0, i32 0
  %31 = inttoptr i64 %27 to ptr
  store ptr %31, ptr %30, align 8
  call void @llvm.lifetime.start.p0(i64 1, ptr %8) #8
  call void @_ZN4taskILb0EE12promise_type15initial_suspendEv(ptr noundef nonnull align 1 dereferenceable(1) %5) #8
  %32 = call noundef i1 @_ZNKSt7__n486114suspend_always11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %8) #8
  br i1 %32, label %41, label %33

33:                                               ; preds = %28
  %34 = call token @llvm.coro.save(ptr null)
  call void @llvm.coro.await.suspend.void(ptr %8, ptr %25, ptr @_Z4bodyILb0EE4taskIXT_EEPKc.__await_suspend_wrapper__init) #8
  %35 = call i8 @llvm.coro.suspend(token %34, i1 false)
  switch i8 %35, label %153 [
    i8 0, label %41
    i8 1, label %36
  ]

36:                                               ; preds = %33
  br label %42

37:                                               ; preds = %23
  %38 = landingpad { ptr, i32 }
          cleanup
  %39 = extractvalue { ptr, i32 } %38, 0
  store ptr %39, ptr %6, align 8
  %40 = extractvalue { ptr, i32 } %38, 1
  store i32 %40, ptr %7, align 4
  br label %162

41:                                               ; preds = %33, %28
  call void @_ZNKSt7__n486114suspend_always12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %8) #8
  br label %42

42:                                               ; preds = %41, %36
  %43 = phi i32 [ 0, %41 ], [ 2, %36 ]
  call void @llvm.lifetime.end.p0(i64 1, ptr %8) #8
  switch i32 %43, label %145 [
    i32 0, label %44
  ]

44:                                               ; preds = %42
  call void @llvm.lifetime.start.p0(i64 4, ptr %10) #8
  %45 = getelementptr inbounds %struct.Dtor, ptr %10, i32 0, i32 0
  store i32 2, ptr %45, align 4
  call void @llvm.lifetime.start.p0(i64 4, ptr %11) #8
  store i32 40, ptr %11, align 4
  call void @llvm.lifetime.start.p0(i64 4, ptr %12) #8
  %46 = getelementptr inbounds %struct.thrower, ptr %12, i32 0, i32 0
  store i32 1, ptr %46, align 4
  %47 = call noundef i1 @_ZN7thrower11await_readyEv(ptr noundef nonnull align 4 dereferenceable(4) %12)
  br i1 %47, label %83, label %48

48:                                               ; preds = %44
  %49 = call token @llvm.coro.save(ptr null)
  invoke void @llvm.coro.await.suspend.handle(ptr %12, ptr %25, ptr @_Z4bodyILb0EE4taskIXT_EEPKc.__await_suspend_wrapper__await)
          to label %50 unwind label %53

50:                                               ; preds = %48
  %51 = call i8 @llvm.coro.suspend(token %49, i1 false)
  switch i8 %51, label %153 [
    i8 0, label %83
    i8 1, label %52
  ]

52:                                               ; preds = %50
  br label %84

53:                                               ; preds = %48
  %54 = landingpad { ptr, i32 }
          catch ptr @_ZTISt9exception
          catch ptr null
  %55 = extractvalue { ptr, i32 } %54, 0
  store ptr %55, ptr %6, align 8
  %56 = extractvalue { ptr, i32 } %54, 1
  store i32 %56, ptr %7, align 4
  call void @llvm.lifetime.end.p0(i64 4, ptr %12) #8
  br label %57

57:                                               ; preds = %53
  %58 = load i32, ptr %7, align 4
  %59 = call i32 @llvm.eh.typeid.for.p0(ptr @_ZTISt9exception) #8
  %60 = icmp eq i32 %58, %59
  br i1 %60, label %61, label %115

61:                                               ; preds = %57
  call void @llvm.lifetime.start.p0(i64 8, ptr %13) #8
  %62 = load ptr, ptr %6, align 8
  %63 = call ptr @__cxa_begin_catch(ptr %62) #8
  store ptr %63, ptr %13, align 8
  %64 = load ptr, ptr %4, align 8
  %65 = load ptr, ptr %13, align 8
  %66 = load ptr, ptr %65, align 8
  %67 = getelementptr inbounds ptr, ptr %66, i64 2
  %68 = load ptr, ptr %67, align 8
  %69 = call noundef ptr %68(ptr noundef nonnull align 8 dereferenceable(8) %65) #8
  %70 = load i32, ptr %11, align 4
  %71 = add nsw i32 %70, 2
  %72 = invoke i32 (ptr, ...) @printf(ptr noundef @.str.4, ptr noundef %64, ptr noundef %69, i32 noundef %71)
          to label %73 unwind label %87

73:                                               ; preds = %61
  invoke void @__cxa_end_catch()
          to label %74 unwind label %91

74:                                               ; preds = %73
  call void @llvm.lifetime.end.p0(i64 8, ptr %13) #8
  br label %75

75:                                               ; preds = %74, %86
  call void @llvm.lifetime.start.p0(i64 4, ptr %14) #8
  %76 = getelementptr inbounds %struct.thrower, ptr %14, i32 0, i32 0
  store i32 2, ptr %76, align 4
  %77 = call noundef i1 @_ZN7thrower11await_readyEv(ptr noundef nonnull align 4 dereferenceable(4) %14)
  br i1 %77, label %101, label %78

78:                                               ; preds = %75
  %79 = call token @llvm.coro.save(ptr null)
  invoke void @llvm.coro.await.suspend.handle(ptr %14, ptr %25, ptr @_Z4bodyILb0EE4taskIXT_EEPKc.__await_suspend_wrapper__await2)
          to label %80 unwind label %97

80:                                               ; preds = %78
  %81 = call i8 @llvm.coro.suspend(token %79, i1 false)
  switch i8 %81, label %153 [
    i8 0, label %101
    i8 1, label %82
  ]

82:                                               ; preds = %80
  br label %102

83:                                               ; preds = %50, %44
  call void @_ZN7thrower12await_resumeEv(ptr noundef nonnull align 4 dereferenceable(4) %12)
  br label %84

84:                                               ; preds = %83, %52
  %85 = phi i32 [ 0, %83 ], [ 2, %52 ]
  call void @llvm.lifetime.end.p0(i64 4, ptr %12) #8
  switch i32 %85, label %107 [
    i32 0, label %86
  ]

86:                                               ; preds = %84
  br label %75

87:                                               ; preds = %61
  %88 = landingpad { ptr, i32 }
          catch ptr null
  %89 = extractvalue { ptr, i32 } %88, 0
  store ptr %89, ptr %6, align 8
  %90 = extractvalue { ptr, i32 } %88, 1
  store i32 %90, ptr %7, align 4
  invoke void @__cxa_end_catch()
          to label %95 unwind label %173

91:                                               ; preds = %73
  %92 = landingpad { ptr, i32 }
          catch ptr null
  %93 = extractvalue { ptr, i32 } %92, 0
  store ptr %93, ptr %6, align 8
  %94 = extractvalue { ptr, i32 } %92, 1
  store i32 %94, ptr %7, align 4
  br label %96

95:                                               ; preds = %87
  br label %96

96:                                               ; preds = %95, %91
  call void @llvm.lifetime.end.p0(i64 8, ptr %13) #8
  br label %115

97:                                               ; preds = %78
  %98 = landingpad { ptr, i32 }
          catch ptr null
  %99 = extractvalue { ptr, i32 } %98, 0
  store ptr %99, ptr %6, align 8
  %100 = extractvalue { ptr, i32 } %98, 1
  store i32 %100, ptr %7, align 4
  call void @llvm.lifetime.end.p0(i64 4, ptr %14) #8
  br label %115

101:                                              ; preds = %80, %75
  call void @_ZN7thrower12await_resumeEv(ptr noundef nonnull align 4 dereferenceable(4) %14)
  br label %102

102:                                              ; preds = %101, %82
  %103 = phi i32 [ 0, %101 ], [ 2, %82 ]
  call void @llvm.lifetime.end.p0(i64 4, ptr %14) #8
  switch i32 %103, label %107 [
    i32 0, label %104
  ]

104:                                              ; preds = %102
  %105 = invoke i32 (ptr, ...) @printf(ptr noundef @.str.5)
          to label %106 unwind label %111

106:                                              ; preds = %104
  br label %107

107:                                              ; preds = %106, %102, %84
  %108 = phi i32 [ %85, %84 ], [ %103, %102 ], [ 0, %106 ]
  call void @llvm.lifetime.end.p0(i64 4, ptr %11) #8
  call void @_ZN4DtorD2Ev(ptr noundef nonnull align 4 dereferenceable(4) %10) #8
  call void @llvm.lifetime.end.p0(i64 4, ptr %10) #8
  switch i32 %108, label %145 [
    i32 0, label %109
  ]

109:                                              ; preds = %107
  invoke void @_ZN4taskILb0EE12promise_type11return_voidEv(ptr noundef nonnull align 1 dereferenceable(1) %5)
          to label %110 unwind label %116

110:                                              ; preds = %109
  br label %126

111:                                              ; preds = %104
  %112 = landingpad { ptr, i32 }
          catch ptr null
  %113 = extractvalue { ptr, i32 } %112, 0
  store ptr %113, ptr %6, align 8
  %114 = extractvalue { ptr, i32 } %112, 1
  store i32 %114, ptr %7, align 4
  br label %115

115:                                              ; preds = %111, %97, %96, %57
  call void @llvm.lifetime.end.p0(i64 4, ptr %11) #8
  call void @_ZN4DtorD2Ev(ptr noundef nonnull align 4 dereferenceable(4) %10) #8
  call void @llvm.lifetime.end.p0(i64 4, ptr %10) #8
  br label %120

116:                                              ; preds = %109
  %117 = landingpad { ptr, i32 }
          catch ptr null
  %118 = extractvalue { ptr, i32 } %117, 0
  store ptr %118, ptr %6, align 8
  %119 = extractvalue { ptr, i32 } %117, 1
  store i32 %119, ptr %7, align 4
  br label %120

120:                                              ; preds = %116, %115
  %121 = load ptr, ptr %6, align 8
  %122 = call ptr @__cxa_begin_catch(ptr %121) #8
  invoke void @_ZN4taskILb0EE12promise_type19unhandled_exceptionEv(ptr noundef nonnull align 1 dereferenceable(1) %5)
          to label %123 unwind label %132

123:                                              ; preds = %120
  invoke void @__cxa_end_catch()
          to label %124 unwind label %136

124:                                              ; preds = %123
  br label %125

125:                                              ; preds = %124
  br label %126

126:                                              ; preds = %125, %110
  call void @llvm.lifetime.start.p0(i64 1, ptr %15) #8
  call void @_ZN4taskILb0EE12promise_type13final_suspendEv(ptr noundef nonnull align 1 dereferenceable(1) %5) #8
  %127 = call noundef i1 @_ZNKSt7__n486114suspend_always11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %15) #8
  br i1 %127, label %141, label %128

128:                                              ; preds = %126
  %129 = call token @llvm.coro.save(ptr null)
  call void @llvm.coro.await.suspend.void(ptr %15, ptr %25, ptr @_Z4bodyILb0EE4taskIXT_EEPKc.__await_suspend_wrapper__final) #8
  %130 = call i8 @llvm.coro.suspend(token %129, i1 true)
  switch i8 %130, label %153 [
    i8 0, label %141
    i8 1, label %131
  ]

131:                                              ; preds = %128
  br label %142

132:                                              ; preds = %120
  %133 = landingpad { ptr, i32 }
          cleanup
  %134 = extractvalue { ptr, i32 } %133, 0
  store ptr %134, ptr %6, align 8
  %135 = extractvalue { ptr, i32 } %133, 1
  store i32 %135, ptr %7, align 4
  invoke void @__cxa_end_catch()
          to label %140 unwind label %173

136:                                              ; preds = %123
  %137 = landingpad { ptr, i32 }
          cleanup
  %138 = extractvalue { ptr, i32 } %137, 0
  store ptr %138, ptr %6, align 8
  %139 = extractvalue { ptr, i32 } %137, 1
  store i32 %139, ptr %7, align 4
  br label %159

140:                                              ; preds = %132
  br label %159

141:                                              ; preds = %128, %126
  call void @_ZNKSt7__n486114suspend_always12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %15) #8
  br label %142

142:                                              ; preds = %141, %131
  %143 = phi i32 [ 0, %141 ], [ 2, %131 ]
  call void @llvm.lifetime.end.p0(i64 1, ptr %15) #8
  switch i32 %143, label %145 [
    i32 0, label %144
  ]

144:                                              ; preds = %142
  br label %145

145:                                              ; preds = %144, %142, %107, %42
  %146 = phi i32 [ %43, %42 ], [ %108, %107 ], [ %143, %142 ], [ 0, %144 ]
  call void @llvm.lifetime.end.p0(i64 1, ptr %5) #8
  call void @llvm.lifetime.end.p0(i64 8, ptr %4) #8
  %147 = call ptr @llvm.coro.free(token %18, ptr %25)
  %148 = icmp ne ptr %147, null
  br i1 %148, label %149, label %151

149:                                              ; preds = %145
  %150 = call i64 @llvm.coro.size.i64()
  call void @_ZdlPvm(ptr noundef %147, i64 noundef %150) #8
  br label %151

151:                                              ; preds = %145, %149
  switch i32 %146, label %176 [
    i32 0, label %152
    i32 2, label %153
  ]

152:                                              ; preds = %151
  br label %153

153:                                              ; preds = %152, %151, %128, %80, %50, %33
  %154 = call i1 @llvm.coro.end(ptr null, i1 false, token none)
  %155 = getelementptr inbounds %struct.task.0, ptr %2, i32 0, i32 0
  %156 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.1", ptr %155, i32 0, i32 0
  %157 = load ptr, ptr %156, align 8
  %158 = ptrtoint ptr %157 to i64
  ret i64 %158

159:                                              ; preds = %140, %136
  %160 = call i1 @llvm.coro.end(ptr null, i1 true, token none)
  br i1 %160, label %168, label %161

161:                                              ; preds = %159
  br label %162

162:                                              ; preds = %161, %37
  call void @llvm.lifetime.end.p0(i64 1, ptr %5) #8
  call void @llvm.lifetime.end.p0(i64 8, ptr %4) #8
  %163 = call ptr @llvm.coro.free(token %18, ptr %25)
  %164 = icmp ne ptr %163, null
  br i1 %164, label %165, label %167

165:                                              ; preds = %162
  %166 = call i64 @llvm.coro.size.i64()
  call void @_ZdlPvm(ptr noundef %163, i64 noundef %166) #8
  br label %167

167:                                              ; preds = %162, %165
  br label %168

168:                                              ; preds = %167, %159
  %169 = load ptr, ptr %6, align 8
  %170 = load i32, ptr %7, align 4
  %171 = insertvalue { ptr, i32 } poison, ptr %169, 0
  %172 = insertvalue { ptr, i32 } %171, i32 %170, 1
  resume { ptr, i32 } %172

173:                                              ; preds = %132, %87
  %174 = landingpad { ptr, i32 }
          catch ptr null
  %175 = extractvalue { ptr, i32 } %174, 0
  call void @__clang_call_terminate(ptr %175) #17
  unreachable

176:                                              ; preds = %151
  unreachable
}

; Function Attrs: mustprogress noinline optnone uwtable
define linkonce_odr dso_local void @_ZNKSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEE6resumeEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #5 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.1", ptr %3, i32 0, i32 0
  %5 = load ptr, ptr %4, align 8
  call void @llvm.coro.resume(ptr %5)
  ret void
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local noundef i1 @_ZNKSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEE4doneEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.1", ptr %3, i32 0, i32 0
  %5 = load ptr, ptr %4, align 8
  %6 = call i1 @llvm.coro.done(ptr %5)
  ret i1 %6
}

; Function Attrs: mustprogress noinline optnone uwtable
define linkonce_odr dso_local void @_ZNKSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEE7destroyEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #5 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.1", ptr %3, i32 0, i32 0
  %5 = load ptr, ptr %4, align 8
  call void @llvm.coro.destroy(ptr %5)
  ret void
}

; Function Attrs: nocallback nofree nosync nounwind willreturn memory(argmem: read)
declare token @llvm.coro.id(i32, ptr readnone, ptr nocapture readonly, ptr) #7

; Function Attrs: nounwind
declare i1 @llvm.coro.alloc(token) #8

; Function Attrs: nobuiltin allocsize(0)
declare noundef nonnull ptr @_Znwm(i64 noundef) #9

; Function Attrs: nounwind memory(none)
declare i64 @llvm.coro.size.i64() #2

; Function Attrs: nounwind
declare ptr @llvm.coro.begin(token, ptr writeonly) #8

; Function Attrs: nocallback nofree nosync nounwind willreturn memory(argmem: readwrite)
declare void @llvm.lifetime.start.p0(i64 immarg, ptr nocapture) #10

; Function Attrs: mustprogress noinline optnone uwtable
define linkonce_odr dso_local i64 @_ZN4taskILb1EE12promise_type17get_return_objectEv(ptr noundef nonnull align 1 dereferenceable(1) %0) #5 comdat {
  %2 = alloca %struct.task, align 8
  %3 = alloca ptr, align 8
  store ptr %0, ptr %3, align 8
  %4 = load ptr, ptr %3, align 8
  %5 = getelementptr inbounds %struct.task, ptr %2, i32 0, i32 0
  %6 = call i64 @_ZNSt7__n486116coroutine_handleIN4taskILb1EE12promise_typeEE12from_promiseERS3_(ptr noundef nonnull align 1 dereferenceable(1) %4)
  %7 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %5, i32 0, i32 0
  %8 = inttoptr i64 %6 to ptr
  store ptr %8, ptr %7, align 8
  %9 = getelementptr inbounds %struct.task, ptr %2, i32 0, i32 0
  %10 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %9, i32 0, i32 0
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
define private void @_Z4bodyILb1EE4taskIXT_EEPKc.__await_suspend_wrapper__init(ptr noundef nonnull %0, ptr noundef %1) #12 {
  %3 = alloca ptr, align 8
  %4 = alloca ptr, align 8
  %5 = alloca %"struct.std::__n4861::coroutine_handle.2", align 8
  %6 = alloca %"struct.std::__n4861::coroutine_handle", align 8
  store ptr %0, ptr %3, align 8
  store ptr %1, ptr %4, align 8
  %7 = load ptr, ptr %3, align 8
  %8 = load ptr, ptr %4, align 8
  %9 = call i64 @_ZNSt7__n486116coroutine_handleIN4taskILb1EE12promise_typeEE12from_addressEPv(ptr noundef %8) #8
  %10 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %6, i32 0, i32 0
  %11 = inttoptr i64 %9 to ptr
  store ptr %11, ptr %10, align 8
  %12 = call i64 @_ZNKSt7__n486116coroutine_handleIN4taskILb1EE12promise_typeEEcvNS0_IvEEEv(ptr noundef nonnull align 8 dereferenceable(8) %6) #8
  %13 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.2", ptr %5, i32 0, i32 0
  %14 = inttoptr i64 %12 to ptr
  store ptr %14, ptr %13, align 8
  %15 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.2", ptr %5, i32 0, i32 0
  %16 = load ptr, ptr %15, align 8
  %17 = ptrtoint ptr %16 to i64
  call void @_ZNKSt7__n486113suspend_never13await_suspendENS_16coroutine_handleIvEE(ptr noundef nonnull align 1 dereferenceable(1) %7, i64 %17) #8
  ret void
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZNKSt7__n486113suspend_never13await_suspendENS_16coroutine_handleIvEE(ptr noundef nonnull align 1 dereferenceable(1) %0, i64 %1) #6 comdat {
  %3 = alloca %"struct.std::__n4861::coroutine_handle.2", align 8
  %4 = alloca ptr, align 8
  %5 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.2", ptr %3, i32 0, i32 0
  %6 = inttoptr i64 %1 to ptr
  store ptr %6, ptr %5, align 8
  store ptr %0, ptr %4, align 8
  %7 = load ptr, ptr %4, align 8
  ret void
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local i64 @_ZNSt7__n486116coroutine_handleIN4taskILb1EE12promise_typeEE12from_addressEPv(ptr noundef %0) #6 comdat {
  %2 = alloca %"struct.std::__n4861::coroutine_handle", align 8
  %3 = alloca ptr, align 8
  store ptr %0, ptr %3, align 8
  call void @_ZNSt7__n486116coroutine_handleIN4taskILb1EE12promise_typeEEC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %2) #8
  %4 = load ptr, ptr %3, align 8
  %5 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %2, i32 0, i32 0
  store ptr %4, ptr %5, align 8
  %6 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %2, i32 0, i32 0
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
  %7 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.2", ptr %2, i32 0, i32 0
  %8 = inttoptr i64 %6 to ptr
  store ptr %8, ptr %7, align 8
  %9 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.2", ptr %2, i32 0, i32 0
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
declare void @llvm.lifetime.end.p0(i64 immarg, ptr nocapture) #10

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local noundef i1 @_ZN7thrower11await_readyEv(ptr noundef nonnull align 4 dereferenceable(4) %0) #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  ret i1 false
}

; Function Attrs: alwaysinline mustprogress
define private noundef ptr @_Z4bodyILb1EE4taskIXT_EEPKc.__await_suspend_wrapper__await(ptr noundef nonnull %0, ptr noundef %1) #12 {
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
  %11 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %7, i32 0, i32 0
  %12 = inttoptr i64 %10 to ptr
  store ptr %12, ptr %11, align 8
  %13 = call i64 @_ZNKSt7__n486116coroutine_handleIN4taskILb1EE12promise_typeEEcvNS0_IvEEEv(ptr noundef nonnull align 8 dereferenceable(8) %7) #8
  %14 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.2", ptr %6, i32 0, i32 0
  %15 = inttoptr i64 %13 to ptr
  store ptr %15, ptr %14, align 8
  %16 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.2", ptr %6, i32 0, i32 0
  %17 = load ptr, ptr %16, align 8
  %18 = ptrtoint ptr %17 to i64
  %19 = call i64 @_ZN7thrower13await_suspendENSt7__n486116coroutine_handleIvEE(ptr noundef nonnull align 4 dereferenceable(4) %8, i64 %18)
  %20 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.2", ptr %5, i32 0, i32 0
  %21 = inttoptr i64 %19 to ptr
  store ptr %21, ptr %20, align 8
  %22 = call noundef ptr @_ZNKSt7__n486116coroutine_handleIvE7addressEv(ptr noundef nonnull align 8 dereferenceable(8) %5) #8
  ret ptr %22
}

; Function Attrs: mustprogress noinline optnone uwtable
define linkonce_odr dso_local i64 @_ZN7thrower13await_suspendENSt7__n486116coroutine_handleIvEE(ptr noundef nonnull align 4 dereferenceable(4) %0, i64 %1) #5 comdat personality ptr @__gxx_personality_v0 {
  %3 = alloca %"struct.std::__n4861::coroutine_handle.2", align 8
  %4 = alloca ptr, align 8
  %5 = alloca ptr, align 8
  %6 = alloca i32, align 4
  %7 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.2", ptr %3, i32 0, i32 0
  %8 = inttoptr i64 %1 to ptr
  store ptr %8, ptr %7, align 8
  store ptr %0, ptr %4, align 8
  %9 = load ptr, ptr %4, align 8
  %10 = call ptr @__cxa_allocate_exception(i64 16) #8
  %11 = getelementptr inbounds %struct.thrower, ptr %9, i32 0, i32 0
  %12 = load i32, ptr %11, align 4
  %13 = icmp eq i32 %12, 1
  %14 = zext i1 %13 to i64
  %15 = select i1 %13, ptr @.str.6, ptr @.str.7
  invoke void @_ZNSt13runtime_errorC1EPKc(ptr noundef nonnull align 8 dereferenceable(16) %10, ptr noundef %15)
          to label %16 unwind label %17

16:                                               ; preds = %2
  call void @__cxa_throw(ptr %10, ptr @_ZTISt13runtime_error, ptr @_ZNSt13runtime_errorD1Ev) #19
  unreachable

17:                                               ; preds = %2
  %18 = landingpad { ptr, i32 }
          cleanup
  %19 = extractvalue { ptr, i32 } %18, 0
  store ptr %19, ptr %5, align 8
  %20 = extractvalue { ptr, i32 } %18, 1
  store i32 %20, ptr %6, align 4
  call void @__cxa_free_exception(ptr %10) #8
  br label %21

21:                                               ; preds = %17
  %22 = load ptr, ptr %5, align 8
  %23 = load i32, ptr %6, align 4
  %24 = insertvalue { ptr, i32 } poison, ptr %22, 0
  %25 = insertvalue { ptr, i32 } %24, i32 %23, 1
  resume { ptr, i32 } %25
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local noundef ptr @_ZNKSt7__n486116coroutine_handleIvE7addressEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.2", ptr %3, i32 0, i32 0
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
define private noundef ptr @_Z4bodyILb1EE4taskIXT_EEPKc.__await_suspend_wrapper__await2(ptr noundef nonnull %0, ptr noundef %1) #12 {
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
  %11 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %7, i32 0, i32 0
  %12 = inttoptr i64 %10 to ptr
  store ptr %12, ptr %11, align 8
  %13 = call i64 @_ZNKSt7__n486116coroutine_handleIN4taskILb1EE12promise_typeEEcvNS0_IvEEEv(ptr noundef nonnull align 8 dereferenceable(8) %7) #8
  %14 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.2", ptr %6, i32 0, i32 0
  %15 = inttoptr i64 %13 to ptr
  store ptr %15, ptr %14, align 8
  %16 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.2", ptr %6, i32 0, i32 0
  %17 = load ptr, ptr %16, align 8
  %18 = ptrtoint ptr %17 to i64
  %19 = call i64 @_ZN7thrower13await_suspendENSt7__n486116coroutine_handleIvEE(ptr noundef nonnull align 4 dereferenceable(4) %8, i64 %18)
  %20 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.2", ptr %5, i32 0, i32 0
  %21 = inttoptr i64 %19 to ptr
  store ptr %21, ptr %20, align 8
  %22 = call noundef ptr @_ZNKSt7__n486116coroutine_handleIvE7addressEv(ptr noundef nonnull align 8 dereferenceable(8) %5) #8
  ret ptr %22
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZN4DtorD2Ev(ptr noundef nonnull align 4 dereferenceable(4) %0) unnamed_addr #6 comdat personality ptr @__gxx_personality_v0 {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds %struct.Dtor, ptr %3, i32 0, i32 0
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
define private void @_Z4bodyILb1EE4taskIXT_EEPKc.__await_suspend_wrapper__final(ptr noundef nonnull %0, ptr noundef %1) #12 {
  %3 = alloca ptr, align 8
  %4 = alloca ptr, align 8
  %5 = alloca %"struct.std::__n4861::coroutine_handle.2", align 8
  %6 = alloca %"struct.std::__n4861::coroutine_handle", align 8
  store ptr %0, ptr %3, align 8
  store ptr %1, ptr %4, align 8
  %7 = load ptr, ptr %3, align 8
  %8 = load ptr, ptr %4, align 8
  %9 = call i64 @_ZNSt7__n486116coroutine_handleIN4taskILb1EE12promise_typeEE12from_addressEPv(ptr noundef %8) #8
  %10 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %6, i32 0, i32 0
  %11 = inttoptr i64 %9 to ptr
  store ptr %11, ptr %10, align 8
  %12 = call i64 @_ZNKSt7__n486116coroutine_handleIN4taskILb1EE12promise_typeEEcvNS0_IvEEEv(ptr noundef nonnull align 8 dereferenceable(8) %6) #8
  %13 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.2", ptr %5, i32 0, i32 0
  %14 = inttoptr i64 %12 to ptr
  store ptr %14, ptr %13, align 8
  %15 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.2", ptr %5, i32 0, i32 0
  %16 = load ptr, ptr %15, align 8
  %17 = ptrtoint ptr %16 to i64
  call void @_ZNKSt7__n486114suspend_always13await_suspendENS_16coroutine_handleIvEE(ptr noundef nonnull align 1 dereferenceable(1) %7, i64 %17) #8
  ret void
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZNKSt7__n486114suspend_always13await_suspendENS_16coroutine_handleIvEE(ptr noundef nonnull align 1 dereferenceable(1) %0, i64 %1) #6 comdat {
  %3 = alloca %"struct.std::__n4861::coroutine_handle.2", align 8
  %4 = alloca ptr, align 8
  %5 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.2", ptr %3, i32 0, i32 0
  %6 = inttoptr i64 %1 to ptr
  store ptr %6, ptr %5, align 8
  store ptr %0, ptr %4, align 8
  %7 = load ptr, ptr %4, align 8
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
declare i1 @llvm.coro.end(ptr, i1, token) #8

; Function Attrs: nobuiltin nounwind
declare void @_ZdlPvm(ptr noundef, i64 noundef) #13

; Function Attrs: nounwind memory(argmem: read)
declare ptr @llvm.coro.free(token, ptr nocapture readonly) #14

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local i64 @_ZNSt7__n486116coroutine_handleIN4taskILb1EE12promise_typeEE12from_promiseERS3_(ptr noundef nonnull align 1 dereferenceable(1) %0) #6 comdat {
  %2 = alloca %"struct.std::__n4861::coroutine_handle", align 8
  %3 = alloca ptr, align 8
  store ptr %0, ptr %3, align 8
  call void @_ZNSt7__n486116coroutine_handleIN4taskILb1EE12promise_typeEEC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %2) #8
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
define linkonce_odr dso_local void @_ZNSt7__n486116coroutine_handleIN4taskILb1EE12promise_typeEEC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %0) unnamed_addr #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %3, i32 0, i32 0
  store ptr null, ptr %4, align 8
  ret void
}

; Function Attrs: nounwind memory(none)
declare ptr @llvm.coro.promise(ptr nocapture, i32, i1) #2

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local i64 @_ZNSt7__n486116coroutine_handleIvE12from_addressEPv(ptr noundef %0) #6 comdat {
  %2 = alloca %"struct.std::__n4861::coroutine_handle.2", align 8
  %3 = alloca ptr, align 8
  store ptr %0, ptr %3, align 8
  call void @_ZNSt7__n486116coroutine_handleIvEC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %2) #8
  %4 = load ptr, ptr %3, align 8
  %5 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.2", ptr %2, i32 0, i32 0
  store ptr %4, ptr %5, align 8
  %6 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.2", ptr %2, i32 0, i32 0
  %7 = load ptr, ptr %6, align 8
  %8 = ptrtoint ptr %7 to i64
  ret i64 %8
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local noundef ptr @_ZNKSt7__n486116coroutine_handleIN4taskILb1EE12promise_typeEE7addressEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %3, i32 0, i32 0
  %5 = load ptr, ptr %4, align 8
  ret ptr %5
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZNSt7__n486116coroutine_handleIvEC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %0) unnamed_addr #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.2", ptr %3, i32 0, i32 0
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
  %5 = getelementptr inbounds %struct.task.0, ptr %2, i32 0, i32 0
  %6 = call i64 @_ZNSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEE12from_promiseERS3_(ptr noundef nonnull align 1 dereferenceable(1) %4)
  %7 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.1", ptr %5, i32 0, i32 0
  %8 = inttoptr i64 %6 to ptr
  store ptr %8, ptr %7, align 8
  %9 = getelementptr inbounds %struct.task.0, ptr %2, i32 0, i32 0
  %10 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.1", ptr %9, i32 0, i32 0
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
define private void @_Z4bodyILb0EE4taskIXT_EEPKc.__await_suspend_wrapper__init(ptr noundef nonnull %0, ptr noundef %1) #12 {
  %3 = alloca ptr, align 8
  %4 = alloca ptr, align 8
  %5 = alloca %"struct.std::__n4861::coroutine_handle.2", align 8
  %6 = alloca %"struct.std::__n4861::coroutine_handle.1", align 8
  store ptr %0, ptr %3, align 8
  store ptr %1, ptr %4, align 8
  %7 = load ptr, ptr %3, align 8
  %8 = load ptr, ptr %4, align 8
  %9 = call i64 @_ZNSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEE12from_addressEPv(ptr noundef %8) #8
  %10 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.1", ptr %6, i32 0, i32 0
  %11 = inttoptr i64 %9 to ptr
  store ptr %11, ptr %10, align 8
  %12 = call i64 @_ZNKSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEEcvNS0_IvEEEv(ptr noundef nonnull align 8 dereferenceable(8) %6) #8
  %13 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.2", ptr %5, i32 0, i32 0
  %14 = inttoptr i64 %12 to ptr
  store ptr %14, ptr %13, align 8
  %15 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.2", ptr %5, i32 0, i32 0
  %16 = load ptr, ptr %15, align 8
  %17 = ptrtoint ptr %16 to i64
  call void @_ZNKSt7__n486114suspend_always13await_suspendENS_16coroutine_handleIvEE(ptr noundef nonnull align 1 dereferenceable(1) %7, i64 %17) #8
  ret void
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local i64 @_ZNSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEE12from_addressEPv(ptr noundef %0) #6 comdat {
  %2 = alloca %"struct.std::__n4861::coroutine_handle.1", align 8
  %3 = alloca ptr, align 8
  store ptr %0, ptr %3, align 8
  call void @_ZNSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEEC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %2) #8
  %4 = load ptr, ptr %3, align 8
  %5 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.1", ptr %2, i32 0, i32 0
  store ptr %4, ptr %5, align 8
  %6 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.1", ptr %2, i32 0, i32 0
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
  %7 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.2", ptr %2, i32 0, i32 0
  %8 = inttoptr i64 %6 to ptr
  store ptr %8, ptr %7, align 8
  %9 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.2", ptr %2, i32 0, i32 0
  %10 = load ptr, ptr %9, align 8
  %11 = ptrtoint ptr %10 to i64
  ret i64 %11
}

; Function Attrs: alwaysinline mustprogress
define private noundef ptr @_Z4bodyILb0EE4taskIXT_EEPKc.__await_suspend_wrapper__await(ptr noundef nonnull %0, ptr noundef %1) #12 {
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
  %11 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.1", ptr %7, i32 0, i32 0
  %12 = inttoptr i64 %10 to ptr
  store ptr %12, ptr %11, align 8
  %13 = call i64 @_ZNKSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEEcvNS0_IvEEEv(ptr noundef nonnull align 8 dereferenceable(8) %7) #8
  %14 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.2", ptr %6, i32 0, i32 0
  %15 = inttoptr i64 %13 to ptr
  store ptr %15, ptr %14, align 8
  %16 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.2", ptr %6, i32 0, i32 0
  %17 = load ptr, ptr %16, align 8
  %18 = ptrtoint ptr %17 to i64
  %19 = call i64 @_ZN7thrower13await_suspendENSt7__n486116coroutine_handleIvEE(ptr noundef nonnull align 4 dereferenceable(4) %8, i64 %18)
  %20 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.2", ptr %5, i32 0, i32 0
  %21 = inttoptr i64 %19 to ptr
  store ptr %21, ptr %20, align 8
  %22 = call noundef ptr @_ZNKSt7__n486116coroutine_handleIvE7addressEv(ptr noundef nonnull align 8 dereferenceable(8) %5) #8
  ret ptr %22
}

; Function Attrs: alwaysinline mustprogress
define private noundef ptr @_Z4bodyILb0EE4taskIXT_EEPKc.__await_suspend_wrapper__await2(ptr noundef nonnull %0, ptr noundef %1) #12 {
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
  %11 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.1", ptr %7, i32 0, i32 0
  %12 = inttoptr i64 %10 to ptr
  store ptr %12, ptr %11, align 8
  %13 = call i64 @_ZNKSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEEcvNS0_IvEEEv(ptr noundef nonnull align 8 dereferenceable(8) %7) #8
  %14 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.2", ptr %6, i32 0, i32 0
  %15 = inttoptr i64 %13 to ptr
  store ptr %15, ptr %14, align 8
  %16 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.2", ptr %6, i32 0, i32 0
  %17 = load ptr, ptr %16, align 8
  %18 = ptrtoint ptr %17 to i64
  %19 = call i64 @_ZN7thrower13await_suspendENSt7__n486116coroutine_handleIvEE(ptr noundef nonnull align 4 dereferenceable(4) %8, i64 %18)
  %20 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.2", ptr %5, i32 0, i32 0
  %21 = inttoptr i64 %19 to ptr
  store ptr %21, ptr %20, align 8
  %22 = call noundef ptr @_ZNKSt7__n486116coroutine_handleIvE7addressEv(ptr noundef nonnull align 8 dereferenceable(8) %5) #8
  ret ptr %22
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
define private void @_Z4bodyILb0EE4taskIXT_EEPKc.__await_suspend_wrapper__final(ptr noundef nonnull %0, ptr noundef %1) #12 {
  %3 = alloca ptr, align 8
  %4 = alloca ptr, align 8
  %5 = alloca %"struct.std::__n4861::coroutine_handle.2", align 8
  %6 = alloca %"struct.std::__n4861::coroutine_handle.1", align 8
  store ptr %0, ptr %3, align 8
  store ptr %1, ptr %4, align 8
  %7 = load ptr, ptr %3, align 8
  %8 = load ptr, ptr %4, align 8
  %9 = call i64 @_ZNSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEE12from_addressEPv(ptr noundef %8) #8
  %10 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.1", ptr %6, i32 0, i32 0
  %11 = inttoptr i64 %9 to ptr
  store ptr %11, ptr %10, align 8
  %12 = call i64 @_ZNKSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEEcvNS0_IvEEEv(ptr noundef nonnull align 8 dereferenceable(8) %6) #8
  %13 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.2", ptr %5, i32 0, i32 0
  %14 = inttoptr i64 %12 to ptr
  store ptr %14, ptr %13, align 8
  %15 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.2", ptr %5, i32 0, i32 0
  %16 = load ptr, ptr %15, align 8
  %17 = ptrtoint ptr %16 to i64
  call void @_ZNKSt7__n486114suspend_always13await_suspendENS_16coroutine_handleIvEE(ptr noundef nonnull align 1 dereferenceable(1) %7, i64 %17) #8
  ret void
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local i64 @_ZNSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEE12from_promiseERS3_(ptr noundef nonnull align 1 dereferenceable(1) %0) #6 comdat {
  %2 = alloca %"struct.std::__n4861::coroutine_handle.1", align 8
  %3 = alloca ptr, align 8
  store ptr %0, ptr %3, align 8
  call void @_ZNSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEEC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %2) #8
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
define linkonce_odr dso_local void @_ZNSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEEC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %0) unnamed_addr #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.1", ptr %3, i32 0, i32 0
  store ptr null, ptr %4, align 8
  ret void
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local noundef ptr @_ZNKSt7__n486116coroutine_handleIN4taskILb0EE12promise_typeEE7addressEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.1", ptr %3, i32 0, i32 0
  %5 = load ptr, ptr %4, align 8
  ret ptr %5
}

declare void @llvm.coro.resume(ptr)

; Function Attrs: nounwind memory(argmem: readwrite)
declare i1 @llvm.coro.done(ptr nocapture readonly) #16

declare void @llvm.coro.destroy(ptr)

attributes #0 = { mustprogress noinline norecurse optnone uwtable "frame-pointer"="non-leaf" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #1 = { mustprogress noinline optnone presplitcoroutine uwtable "frame-pointer"="non-leaf" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #2 = { nounwind memory(none) }
attributes #3 = { "frame-pointer"="non-leaf" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #4 = { noinline noreturn nounwind uwtable "frame-pointer"="non-leaf" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #5 = { mustprogress noinline optnone uwtable "frame-pointer"="non-leaf" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #6 = { mustprogress noinline nounwind optnone uwtable "frame-pointer"="non-leaf" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #7 = { nocallback nofree nosync nounwind willreturn memory(argmem: read) }
attributes #8 = { nounwind }
attributes #9 = { nobuiltin allocsize(0) "frame-pointer"="non-leaf" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #10 = { nocallback nofree nosync nounwind willreturn memory(argmem: readwrite) }
attributes #11 = { nomerge nounwind }
attributes #12 = { alwaysinline mustprogress }
attributes #13 = { nobuiltin nounwind "frame-pointer"="non-leaf" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #14 = { nounwind memory(argmem: read) }
attributes #15 = { nounwind "frame-pointer"="non-leaf" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #16 = { nounwind memory(argmem: readwrite) }
attributes #17 = { noreturn nounwind }
attributes #18 = { allocsize(0) }
attributes #19 = { noreturn }

!llvm.linker.options = !{}
!llvm.module.flags = !{!0, !1, !2, !3, !4}
!llvm.ident = !{!5}

!0 = !{i32 1, !"wchar_size", i32 4}
!1 = !{i32 8, !"PIC Level", i32 2}
!2 = !{i32 7, !"PIE Level", i32 2}
!3 = !{i32 7, !"uwtable", i32 2}
!4 = !{i32 7, !"frame-pointer", i32 1}
!5 = !{!"Debian clang version 19.1.7 (3+b1)"}
