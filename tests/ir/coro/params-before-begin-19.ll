; clang++-19 -std=c++20 -O0 -S -emit-llvm -Xclang -disable-llvm-passes
; (aarch64-unknown-linux-gnu; the target triple and datalayout removed) of:
;
; // Design 0005 §3.4 "Before coro.begin": the promise's operator new takes
; // the coroutine's parameters by reference, so their allocas escape before
; // coro.begin, have no lifetime markers, and live in the frame (rule 5)
; // with a copy at coro.begin. The body reads them after a suspend; the
; // operator new changes one through its reference to show the copy is
; // taken after the call.
; // Expected stdout:
; //   new 3 4 x
; //   body 3 5 x
; #include <coroutine>
; #include <cstdio>
; #include <cstdlib>
; struct task {
;   struct promise_type {
;     static void* operator new(std::size_t n, int& a, long& b, char& c) {
;       std::printf("new %d %ld %c\n", a, b, c);
;       b = 5;
;       return std::malloc(n);
;     }
;     static void operator delete(void* p) { std::free(p); }
;     task get_return_object() { return {std::coroutine_handle<promise_type>::from_promise(*this)}; }
;     std::suspend_always initial_suspend() noexcept { return {}; }
;     std::suspend_always final_suspend() noexcept { return {}; }
;     void return_void() {}
;     void unhandled_exception() {}
;   };
;   std::coroutine_handle<promise_type> h;
; };
; task f(int a, long b, char c) {
;   co_await std::suspend_always{};
;   std::printf("body %d %ld %c\n", a, b, c);
; }
; int main() {
;   task t = f(3, 4, 'x');
;   t.h.resume();
;   t.h.resume();
;   t.h.destroy();
; }
;
;@ covers: spec/ir.md §5.5 intrinsics (coroutine bodies; spec/codegen.md §7.6)
;@ input: pre-opt
;@ dialect: 19
;@ link: c++
;@ unsupported(x86_64): *
;@ stdout: new 3 4 x
;@ stdout: body 3 5 x

source_filename = "params-before-begin.cpp"

%struct.task = type { %"struct.std::__n4861::coroutine_handle" }
%"struct.std::__n4861::coroutine_handle" = type { ptr }
%"struct.task::promise_type" = type { i8 }
%"struct.std::__n4861::suspend_always" = type { i8 }
%"struct.std::__n4861::coroutine_handle.0" = type { ptr }

$_ZN4task12promise_typenwEmRiRlRc = comdat any

$_ZN4task12promise_type17get_return_objectEv = comdat any

$_ZN4task12promise_type15initial_suspendEv = comdat any

$_ZNKSt7__n486114suspend_always11await_readyEv = comdat any

$_ZNKSt7__n486114suspend_always13await_suspendENS_16coroutine_handleIvEE = comdat any

$_ZNSt7__n486116coroutine_handleIN4task12promise_typeEE12from_addressEPv = comdat any

$_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEEcvNS0_IvEEEv = comdat any

$_ZNKSt7__n486114suspend_always12await_resumeEv = comdat any

$_ZN4task12promise_type11return_voidEv = comdat any

$_ZN4task12promise_type19unhandled_exceptionEv = comdat any

$__clang_call_terminate = comdat any

$_ZN4task12promise_type13final_suspendEv = comdat any

$_ZN4task12promise_typedlEPv = comdat any

$_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEE6resumeEv = comdat any

$_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEE7destroyEv = comdat any

$_ZNSt7__n486116coroutine_handleIN4task12promise_typeEE12from_promiseERS2_ = comdat any

$_ZNSt7__n486116coroutine_handleIN4task12promise_typeEEC2Ev = comdat any

$_ZNSt7__n486116coroutine_handleIvE12from_addressEPv = comdat any

$_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEE7addressEv = comdat any

$_ZNSt7__n486116coroutine_handleIvEC2Ev = comdat any

@.str = private unnamed_addr constant [16 x i8] c"body %d %ld %c\0A\00", align 1
@.str.1 = private unnamed_addr constant [15 x i8] c"new %d %ld %c\0A\00", align 1

; Function Attrs: mustprogress noinline optnone presplitcoroutine uwtable
define dso_local i64 @_Z1filc(i32 noundef %0, i64 noundef %1, i8 noundef %2) #0 personality ptr @__gxx_personality_v0 {
  %4 = alloca %struct.task, align 8
  %5 = alloca i32, align 4
  %6 = alloca i64, align 8
  %7 = alloca i8, align 1
  %8 = alloca i32, align 4
  %9 = alloca i64, align 8
  %10 = alloca i8, align 1
  %11 = alloca %"struct.task::promise_type", align 1
  %12 = alloca ptr, align 8
  %13 = alloca i32, align 4
  %14 = alloca %"struct.std::__n4861::suspend_always", align 1
  %15 = alloca %"struct.std::__n4861::suspend_always", align 1
  %16 = alloca %"struct.std::__n4861::suspend_always", align 1
  %17 = alloca %"struct.std::__n4861::suspend_always", align 1
  %18 = alloca %"struct.std::__n4861::suspend_always", align 1
  store i32 %0, ptr %5, align 4
  store i64 %1, ptr %6, align 8
  store i8 %2, ptr %7, align 1
  %19 = bitcast ptr %11 to ptr
  %20 = call token @llvm.coro.id(i32 16, ptr %19, ptr null, ptr null)
  %21 = call i1 @llvm.coro.alloc(token %20)
  br i1 %21, label %22, label %25

22:                                               ; preds = %3
  %23 = call i64 @llvm.coro.size.i64()
  %24 = call noundef ptr @_ZN4task12promise_typenwEmRiRlRc(i64 noundef %23, ptr noundef nonnull align 4 dereferenceable(4) %5, ptr noundef nonnull align 8 dereferenceable(8) %6, ptr noundef nonnull align 1 dereferenceable(1) %7)
  br label %25

25:                                               ; preds = %22, %3
  %26 = phi ptr [ null, %3 ], [ %24, %22 ]
  %27 = call ptr @llvm.coro.begin(token %20, ptr %26)
  call void @llvm.lifetime.start.p0(i64 4, ptr %8) #2
  %28 = load i32, ptr %5, align 4
  store i32 %28, ptr %8, align 4
  call void @llvm.lifetime.start.p0(i64 8, ptr %9) #2
  %29 = load i64, ptr %6, align 8
  store i64 %29, ptr %9, align 8
  call void @llvm.lifetime.start.p0(i64 1, ptr %10) #2
  %30 = load i8, ptr %7, align 1
  store i8 %30, ptr %10, align 1
  call void @llvm.lifetime.start.p0(i64 1, ptr %11) #2
  %31 = invoke i64 @_ZN4task12promise_type17get_return_objectEv(ptr noundef nonnull align 1 dereferenceable(1) %11)
          to label %32 unwind label %41

32:                                               ; preds = %25
  %33 = getelementptr inbounds %struct.task, ptr %4, i32 0, i32 0
  %34 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %33, i32 0, i32 0
  %35 = inttoptr i64 %31 to ptr
  store ptr %35, ptr %34, align 8
  call void @llvm.lifetime.start.p0(i64 1, ptr %14) #2
  call void @_ZN4task12promise_type15initial_suspendEv(ptr noundef nonnull align 1 dereferenceable(1) %11) #2
  %36 = call noundef i1 @_ZNKSt7__n486114suspend_always11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %14) #2
  br i1 %36, label %45, label %37

37:                                               ; preds = %32
  %38 = call token @llvm.coro.save(ptr null)
  call void @llvm.coro.await.suspend.void(ptr %14, ptr %27, ptr @_Z1filc.__await_suspend_wrapper__init) #2
  %39 = call i8 @llvm.coro.suspend(token %38, i1 false)
  switch i8 %39, label %101 [
    i8 0, label %45
    i8 1, label %40
  ]

40:                                               ; preds = %37
  br label %46

41:                                               ; preds = %25
  %42 = landingpad { ptr, i32 }
          cleanup
  %43 = extractvalue { ptr, i32 } %42, 0
  store ptr %43, ptr %12, align 8
  %44 = extractvalue { ptr, i32 } %42, 1
  store i32 %44, ptr %13, align 4
  br label %110

45:                                               ; preds = %37, %32
  call void @_ZNKSt7__n486114suspend_always12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %14) #2
  br label %46

46:                                               ; preds = %45, %40
  %47 = phi i32 [ 0, %45 ], [ 2, %40 ]
  call void @llvm.lifetime.end.p0(i64 1, ptr %14) #2
  switch i32 %47, label %94 [
    i32 0, label %48
  ]

48:                                               ; preds = %46
  call void @llvm.lifetime.start.p0(i64 1, ptr %16) #2
  %49 = call noundef i1 @_ZNKSt7__n486114suspend_always11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %16) #2
  br i1 %49, label %54, label %50

50:                                               ; preds = %48
  %51 = call token @llvm.coro.save(ptr null)
  call void @llvm.coro.await.suspend.void(ptr %16, ptr %27, ptr @_Z1filc.__await_suspend_wrapper__await) #2
  %52 = call i8 @llvm.coro.suspend(token %51, i1 false)
  switch i8 %52, label %101 [
    i8 0, label %54
    i8 1, label %53
  ]

53:                                               ; preds = %50
  br label %55

54:                                               ; preds = %50, %48
  call void @_ZNKSt7__n486114suspend_always12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %16) #2
  br label %55

55:                                               ; preds = %54, %53
  %56 = phi i32 [ 0, %54 ], [ 2, %53 ]
  call void @llvm.lifetime.end.p0(i64 1, ptr %16) #2
  switch i32 %56, label %94 [
    i32 0, label %57
  ]

57:                                               ; preds = %55
  %58 = load i32, ptr %8, align 4
  %59 = load i64, ptr %9, align 8
  %60 = load i8, ptr %10, align 1
  %61 = zext i8 %60 to i32
  %62 = invoke i32 (ptr, ...) @printf(ptr noundef @.str, i32 noundef %58, i64 noundef %59, i32 noundef %61)
          to label %63 unwind label %65

63:                                               ; preds = %57
  invoke void @_ZN4task12promise_type11return_voidEv(ptr noundef nonnull align 1 dereferenceable(1) %11)
          to label %64 unwind label %65

64:                                               ; preds = %63
  br label %75

65:                                               ; preds = %63, %57
  %66 = landingpad { ptr, i32 }
          catch ptr null
  %67 = extractvalue { ptr, i32 } %66, 0
  store ptr %67, ptr %12, align 8
  %68 = extractvalue { ptr, i32 } %66, 1
  store i32 %68, ptr %13, align 4
  br label %69

69:                                               ; preds = %65
  %70 = load ptr, ptr %12, align 8
  %71 = call ptr @__cxa_begin_catch(ptr %70) #2
  invoke void @_ZN4task12promise_type19unhandled_exceptionEv(ptr noundef nonnull align 1 dereferenceable(1) %11)
          to label %72 unwind label %81

72:                                               ; preds = %69
  invoke void @__cxa_end_catch()
          to label %73 unwind label %85

73:                                               ; preds = %72
  br label %74

74:                                               ; preds = %73
  br label %75

75:                                               ; preds = %74, %64
  call void @llvm.lifetime.start.p0(i64 1, ptr %17) #2
  call void @_ZN4task12promise_type13final_suspendEv(ptr noundef nonnull align 1 dereferenceable(1) %11) #2
  %76 = call noundef i1 @_ZNKSt7__n486114suspend_always11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %17) #2
  br i1 %76, label %90, label %77

77:                                               ; preds = %75
  %78 = call token @llvm.coro.save(ptr null)
  call void @llvm.coro.await.suspend.void(ptr %17, ptr %27, ptr @_Z1filc.__await_suspend_wrapper__final) #2
  %79 = call i8 @llvm.coro.suspend(token %78, i1 true)
  switch i8 %79, label %101 [
    i8 0, label %90
    i8 1, label %80
  ]

80:                                               ; preds = %77
  br label %91

81:                                               ; preds = %69
  %82 = landingpad { ptr, i32 }
          cleanup
  %83 = extractvalue { ptr, i32 } %82, 0
  store ptr %83, ptr %12, align 8
  %84 = extractvalue { ptr, i32 } %82, 1
  store i32 %84, ptr %13, align 4
  invoke void @__cxa_end_catch()
          to label %89 unwind label %120

85:                                               ; preds = %72
  %86 = landingpad { ptr, i32 }
          cleanup
  %87 = extractvalue { ptr, i32 } %86, 0
  store ptr %87, ptr %12, align 8
  %88 = extractvalue { ptr, i32 } %86, 1
  store i32 %88, ptr %13, align 4
  br label %107

89:                                               ; preds = %81
  br label %107

90:                                               ; preds = %77, %75
  call void @_ZNKSt7__n486114suspend_always12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %17) #2
  br label %91

91:                                               ; preds = %90, %80
  %92 = phi i32 [ 0, %90 ], [ 2, %80 ]
  call void @llvm.lifetime.end.p0(i64 1, ptr %17) #2
  switch i32 %92, label %94 [
    i32 0, label %93
  ]

93:                                               ; preds = %91
  br label %94

94:                                               ; preds = %93, %91, %55, %46
  %95 = phi i32 [ %47, %46 ], [ %56, %55 ], [ %92, %91 ], [ 0, %93 ]
  call void @llvm.lifetime.end.p0(i64 1, ptr %11) #2
  call void @llvm.lifetime.end.p0(i64 1, ptr %10) #2
  call void @llvm.lifetime.end.p0(i64 8, ptr %9) #2
  call void @llvm.lifetime.end.p0(i64 4, ptr %8) #2
  %96 = call ptr @llvm.coro.free(token %20, ptr %27)
  %97 = icmp ne ptr %96, null
  br i1 %97, label %98, label %99

98:                                               ; preds = %94
  call void @_ZN4task12promise_typedlEPv(ptr noundef %96) #2
  br label %99

99:                                               ; preds = %94, %98
  switch i32 %95, label %123 [
    i32 0, label %100
    i32 2, label %101
  ]

100:                                              ; preds = %99
  br label %101

101:                                              ; preds = %100, %99, %77, %50, %37
  %102 = call i1 @llvm.coro.end(ptr null, i1 false, token none)
  %103 = getelementptr inbounds %struct.task, ptr %4, i32 0, i32 0
  %104 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %103, i32 0, i32 0
  %105 = load ptr, ptr %104, align 8
  %106 = ptrtoint ptr %105 to i64
  ret i64 %106

107:                                              ; preds = %89, %85
  %108 = call i1 @llvm.coro.end(ptr null, i1 true, token none)
  br i1 %108, label %115, label %109

109:                                              ; preds = %107
  br label %110

110:                                              ; preds = %109, %41
  call void @llvm.lifetime.end.p0(i64 1, ptr %11) #2
  call void @llvm.lifetime.end.p0(i64 1, ptr %10) #2
  call void @llvm.lifetime.end.p0(i64 8, ptr %9) #2
  call void @llvm.lifetime.end.p0(i64 4, ptr %8) #2
  %111 = call ptr @llvm.coro.free(token %20, ptr %27)
  %112 = icmp ne ptr %111, null
  br i1 %112, label %113, label %114

113:                                              ; preds = %110
  call void @_ZN4task12promise_typedlEPv(ptr noundef %111) #2
  br label %114

114:                                              ; preds = %110, %113
  br label %115

115:                                              ; preds = %114, %107
  %116 = load ptr, ptr %12, align 8
  %117 = load i32, ptr %13, align 4
  %118 = insertvalue { ptr, i32 } poison, ptr %116, 0
  %119 = insertvalue { ptr, i32 } %118, i32 %117, 1
  resume { ptr, i32 } %119

120:                                              ; preds = %81
  %121 = landingpad { ptr, i32 }
          catch ptr null
  %122 = extractvalue { ptr, i32 } %121, 0
  call void @__clang_call_terminate(ptr %122) #15
  unreachable

123:                                              ; preds = %99
  unreachable
}

; Function Attrs: nocallback nofree nosync nounwind willreturn memory(argmem: read)
declare token @llvm.coro.id(i32, ptr readnone, ptr nocapture readonly, ptr) #1

; Function Attrs: nounwind
declare i1 @llvm.coro.alloc(token) #2

; Function Attrs: mustprogress noinline optnone uwtable
define linkonce_odr dso_local noundef ptr @_ZN4task12promise_typenwEmRiRlRc(i64 noundef %0, ptr noundef nonnull align 4 dereferenceable(4) %1, ptr noundef nonnull align 8 dereferenceable(8) %2, ptr noundef nonnull align 1 dereferenceable(1) %3) #3 comdat {
  %5 = alloca i64, align 8
  %6 = alloca ptr, align 8
  %7 = alloca ptr, align 8
  %8 = alloca ptr, align 8
  store i64 %0, ptr %5, align 8
  store ptr %1, ptr %6, align 8
  store ptr %2, ptr %7, align 8
  store ptr %3, ptr %8, align 8
  %9 = load ptr, ptr %6, align 8
  %10 = load i32, ptr %9, align 4
  %11 = load ptr, ptr %7, align 8
  %12 = load i64, ptr %11, align 8
  %13 = load ptr, ptr %8, align 8
  %14 = load i8, ptr %13, align 1
  %15 = zext i8 %14 to i32
  %16 = call i32 (ptr, ...) @printf(ptr noundef @.str.1, i32 noundef %10, i64 noundef %12, i32 noundef %15)
  %17 = load ptr, ptr %7, align 8
  store i64 5, ptr %17, align 8
  %18 = load i64, ptr %5, align 8
  %19 = call noalias ptr @malloc(i64 noundef %18) #16
  ret ptr %19
}

; Function Attrs: nounwind memory(none)
declare i64 @llvm.coro.size.i64() #4

; Function Attrs: nounwind
declare ptr @llvm.coro.begin(token, ptr writeonly) #2

; Function Attrs: nocallback nofree nosync nounwind willreturn memory(argmem: readwrite)
declare void @llvm.lifetime.start.p0(i64 immarg, ptr nocapture) #5

; Function Attrs: mustprogress noinline optnone uwtable
define linkonce_odr dso_local i64 @_ZN4task12promise_type17get_return_objectEv(ptr noundef nonnull align 1 dereferenceable(1) %0) #3 comdat {
  %2 = alloca %struct.task, align 8
  %3 = alloca ptr, align 8
  store ptr %0, ptr %3, align 8
  %4 = load ptr, ptr %3, align 8
  %5 = getelementptr inbounds %struct.task, ptr %2, i32 0, i32 0
  %6 = call i64 @_ZNSt7__n486116coroutine_handleIN4task12promise_typeEE12from_promiseERS2_(ptr noundef nonnull align 1 dereferenceable(1) %4)
  %7 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %5, i32 0, i32 0
  %8 = inttoptr i64 %6 to ptr
  store ptr %8, ptr %7, align 8
  %9 = getelementptr inbounds %struct.task, ptr %2, i32 0, i32 0
  %10 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %9, i32 0, i32 0
  %11 = load ptr, ptr %10, align 8
  %12 = ptrtoint ptr %11 to i64
  ret i64 %12
}

declare i32 @__gxx_personality_v0(...)

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZN4task12promise_type15initial_suspendEv(ptr noundef nonnull align 1 dereferenceable(1) %0) #6 comdat {
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
define private void @_Z1filc.__await_suspend_wrapper__init(ptr noundef nonnull %0, ptr noundef %1) #8 {
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

; Function Attrs: alwaysinline mustprogress
define private void @_Z1filc.__await_suspend_wrapper__await(ptr noundef nonnull %0, ptr noundef %1) #8 {
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

declare i32 @printf(ptr noundef, ...) #9

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZN4task12promise_type11return_voidEv(ptr noundef nonnull align 1 dereferenceable(1) %0) #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  ret void
}

declare ptr @__cxa_begin_catch(ptr)

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZN4task12promise_type19unhandled_exceptionEv(ptr noundef nonnull align 1 dereferenceable(1) %0) #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  ret void
}

declare void @__cxa_end_catch()

; Function Attrs: noinline noreturn nounwind uwtable
define linkonce_odr hidden void @__clang_call_terminate(ptr noundef %0) #10 comdat {
  %2 = call ptr @__cxa_begin_catch(ptr %0) #2
  call void @_ZSt9terminatev() #15
  unreachable
}

declare void @_ZSt9terminatev()

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZN4task12promise_type13final_suspendEv(ptr noundef nonnull align 1 dereferenceable(1) %0) #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  ret void
}

; Function Attrs: alwaysinline mustprogress
define private void @_Z1filc.__await_suspend_wrapper__final(ptr noundef nonnull %0, ptr noundef %1) #8 {
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

; Function Attrs: nounwind
declare i1 @llvm.coro.end(ptr, i1, token) #2

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZN4task12promise_typedlEPv(ptr noundef %0) #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  call void @free(ptr noundef %3) #2
  ret void
}

; Function Attrs: nounwind memory(argmem: read)
declare ptr @llvm.coro.free(token, ptr nocapture readonly) #11

; Function Attrs: mustprogress noinline norecurse optnone uwtable
define dso_local noundef i32 @main() #12 {
  %1 = alloca %struct.task, align 8
  %2 = call i64 @_Z1filc(i32 noundef 3, i64 noundef 4, i8 noundef 120)
  %3 = getelementptr inbounds %struct.task, ptr %1, i32 0, i32 0
  %4 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %3, i32 0, i32 0
  %5 = inttoptr i64 %2 to ptr
  store ptr %5, ptr %4, align 8
  %6 = getelementptr inbounds %struct.task, ptr %1, i32 0, i32 0
  call void @_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEE6resumeEv(ptr noundef nonnull align 8 dereferenceable(8) %6)
  %7 = getelementptr inbounds %struct.task, ptr %1, i32 0, i32 0
  call void @_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEE6resumeEv(ptr noundef nonnull align 8 dereferenceable(8) %7)
  %8 = getelementptr inbounds %struct.task, ptr %1, i32 0, i32 0
  call void @_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEE7destroyEv(ptr noundef nonnull align 8 dereferenceable(8) %8)
  ret i32 0
}

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

; Function Attrs: nounwind allocsize(0)
declare noalias ptr @malloc(i64 noundef) #13

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local i64 @_ZNSt7__n486116coroutine_handleIN4task12promise_typeEE12from_promiseERS2_(ptr noundef nonnull align 1 dereferenceable(1) %0) #6 comdat {
  %2 = alloca %"struct.std::__n4861::coroutine_handle", align 8
  %3 = alloca ptr, align 8
  store ptr %0, ptr %3, align 8
  call void @_ZNSt7__n486116coroutine_handleIN4task12promise_typeEEC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %2) #2
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

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZNSt7__n486116coroutine_handleIvEC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %0) unnamed_addr #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %3, i32 0, i32 0
  store ptr null, ptr %4, align 8
  ret void
}

; Function Attrs: nounwind
declare void @free(ptr noundef) #14

declare void @llvm.coro.resume(ptr)

declare void @llvm.coro.destroy(ptr)

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
attributes #10 = { noinline noreturn nounwind uwtable "frame-pointer"="non-leaf" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #11 = { nounwind memory(argmem: read) }
attributes #12 = { mustprogress noinline norecurse optnone uwtable "frame-pointer"="non-leaf" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #13 = { nounwind allocsize(0) "frame-pointer"="non-leaf" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #14 = { nounwind "frame-pointer"="non-leaf" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #15 = { noreturn nounwind }
attributes #16 = { nounwind allocsize(0) }

!llvm.linker.options = !{}
!llvm.module.flags = !{!0, !1, !2, !3, !4}
!llvm.ident = !{!5}

!0 = !{i32 1, !"wchar_size", i32 4}
!1 = !{i32 8, !"PIC Level", i32 2}
!2 = !{i32 7, !"PIE Level", i32 2}
!3 = !{i32 7, !"uwtable", i32 2}
!4 = !{i32 7, !"frame-pointer", i32 1}
!5 = !{!"Debian clang version 19.1.7 (3+b1)"}
