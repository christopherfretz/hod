; clang++-19 -std=c++20 -O0 -S -emit-llvm -Xclang -disable-llvm-passes
; (aarch64-unknown-linux-gnu; the target triple, datalayout, target-cpu and
; target-features removed, so that the seed runs on every target) of:
;
; // Design 0005 §3.4, rule 5: a local whose address escapes before a suspend
; // and is written through that address after it. `x` itself has no direct
; // use after the suspend; only `p` (an unescaped slot) is loaded there.
; // Also the registry shape: an object registers `this` before a suspend and
; // is reached through the registry after it.
; // Expected stdout (both dialects, with and without lifetime markers):
; //   1 7
; //   poked 5
; #include <coroutine>
; #include <cstdio>
; struct task {
;   struct promise_type {
;     task get_return_object() { return {std::coroutine_handle<promise_type>::from_promise(*this)}; }
;     std::suspend_never initial_suspend() noexcept { return {}; }
;     std::suspend_always final_suspend() noexcept { return {}; }
;     void return_void() {}
;     void unhandled_exception() {}
;   };
;   std::coroutine_handle<promise_type> h;
; };
; __attribute__((noinline)) void use(int a, int b) { std::printf("%d %d\n", a, b); }
; task pointer() {
;   int x = 0;
;   int y = 7;
;   int* p = &x;
;   co_await std::suspend_always{};
;   *p = 1;
;   int junk[64];  // reuses the stack a dead `x` would have had
;   for (int& j : junk) j = -1;
;   use(x, y + junk[0] + 1);
; }
; struct T;
; struct registry {
;   T* t = nullptr;
;   void add(T* p) { t = p; }
;   void poke();
; } reg;
; struct T {
;   int v = 5;
;   void hello() { std::printf("poked %d\n", v); }
; };
; void registry::poke() { t->hello(); }
; task registered() {
;   T t;
;   reg.add(&t);
;   co_await std::suspend_always{};
;   long junk[64];
;   for (long& j : junk) j = 0x5a5a5a5a;
;   reg.poke();
;   (void)junk[1];
; }
; // Resumes from a deeper frame: the resumed activation's stack is not
; // where the ramp's was, and the dead ramp stack is overwritten.
; __attribute__((noinline)) void resume_deep(std::coroutine_handle<> h) {
;   volatile long pad[256];
;   for (int i = 0; i < 256; i++) pad[i] = 0x5a5a5a5a5a5a5a5a;
;   h.resume();
; }
; int main() {
;   task a = pointer();
;   resume_deep(a.h);
;   task b = registered();
;   resume_deep(b.h);
;   a.h.destroy();
;   b.h.destroy();
; }
;
;@ covers: spec/ir.md §5.5 intrinsics (coroutine bodies; spec/codegen.md §7.6)
;@ input: pre-opt
;@ dialect: 19
;@ link: c++
;@ stdout: 1 7
;@ stdout: poked 5

source_filename = "escaped-local.cpp"

%struct.registry = type { ptr }
%struct.task = type { %"struct.std::__n4861::coroutine_handle" }
%"struct.std::__n4861::coroutine_handle" = type { ptr }
%"struct.task::promise_type" = type { i8 }
%"struct.std::__n4861::suspend_never" = type { i8 }
%"struct.std::__n4861::suspend_always" = type { i8 }
%"struct.std::__n4861::coroutine_handle.0" = type { ptr }
%struct.T = type { i32 }

$_ZN4task12promise_type17get_return_objectEv = comdat any

$_ZN4task12promise_type15initial_suspendEv = comdat any

$_ZNKSt7__n486113suspend_never11await_readyEv = comdat any

$_ZNKSt7__n486113suspend_never13await_suspendENS_16coroutine_handleIvEE = comdat any

$_ZNSt7__n486116coroutine_handleIN4task12promise_typeEE12from_addressEPv = comdat any

$_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEEcvNS0_IvEEEv = comdat any

$_ZNKSt7__n486113suspend_never12await_resumeEv = comdat any

$_ZNKSt7__n486114suspend_always11await_readyEv = comdat any

$_ZNKSt7__n486114suspend_always13await_suspendENS_16coroutine_handleIvEE = comdat any

$_ZNKSt7__n486114suspend_always12await_resumeEv = comdat any

$_ZN4task12promise_type11return_voidEv = comdat any

$_ZN4task12promise_type19unhandled_exceptionEv = comdat any

$__clang_call_terminate = comdat any

$_ZN4task12promise_type13final_suspendEv = comdat any

$_ZN1T5helloEv = comdat any

$_ZN1TC2Ev = comdat any

$_ZN8registry3addEP1T = comdat any

$_ZNKSt7__n486116coroutine_handleIvE6resumeEv = comdat any

$_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEE7destroyEv = comdat any

$_ZNSt7__n486116coroutine_handleIN4task12promise_typeEE12from_promiseERS2_ = comdat any

$_ZNSt7__n486116coroutine_handleIN4task12promise_typeEEC2Ev = comdat any

$_ZNSt7__n486116coroutine_handleIvE12from_addressEPv = comdat any

$_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEE7addressEv = comdat any

$_ZNSt7__n486116coroutine_handleIvEC2Ev = comdat any

@.str = private unnamed_addr constant [7 x i8] c"%d %d\0A\00", align 1
@reg = dso_local global %struct.registry zeroinitializer, align 8
@.str.1 = private unnamed_addr constant [10 x i8] c"poked %d\0A\00", align 1

; Function Attrs: mustprogress noinline optnone uwtable
define dso_local void @_Z3useii(i32 noundef %0, i32 noundef %1) #0 {
  %3 = alloca i32, align 4
  %4 = alloca i32, align 4
  store i32 %0, ptr %3, align 4
  store i32 %1, ptr %4, align 4
  %5 = load i32, ptr %3, align 4
  %6 = load i32, ptr %4, align 4
  %7 = call i32 (ptr, ...) @printf(ptr noundef @.str, i32 noundef %5, i32 noundef %6)
  ret void
}

declare i32 @printf(ptr noundef, ...) #1

; Function Attrs: mustprogress noinline optnone presplitcoroutine uwtable
define dso_local i64 @_Z7pointerv() #2 personality ptr @__gxx_personality_v0 {
  %1 = alloca %struct.task, align 8
  %2 = alloca %"struct.task::promise_type", align 1
  %3 = alloca ptr, align 8
  %4 = alloca i32, align 4
  %5 = alloca %"struct.std::__n4861::suspend_never", align 1
  %6 = alloca %"struct.std::__n4861::suspend_never", align 1
  %7 = alloca i32, align 4
  %8 = alloca i32, align 4
  %9 = alloca ptr, align 8
  %10 = alloca %"struct.std::__n4861::suspend_always", align 1
  %11 = alloca [64 x i32], align 4
  %12 = alloca ptr, align 8
  %13 = alloca ptr, align 8
  %14 = alloca ptr, align 8
  %15 = alloca ptr, align 8
  %16 = alloca %"struct.std::__n4861::suspend_always", align 1
  %17 = alloca %"struct.std::__n4861::suspend_always", align 1
  %18 = bitcast ptr %2 to ptr
  %19 = call token @llvm.coro.id(i32 16, ptr %18, ptr null, ptr null)
  %20 = call i1 @llvm.coro.alloc(token %19)
  br i1 %20, label %21, label %24

21:                                               ; preds = %0
  %22 = call i64 @llvm.coro.size.i64()
  %23 = call noalias noundef nonnull ptr @_Znwm(i64 noundef %22) #15
  br label %24

24:                                               ; preds = %21, %0
  %25 = phi ptr [ null, %0 ], [ %23, %21 ]
  %26 = call ptr @llvm.coro.begin(token %19, ptr %25)
  call void @llvm.lifetime.start.p0(i64 1, ptr %2) #4
  %27 = invoke i64 @_ZN4task12promise_type17get_return_objectEv(ptr noundef nonnull align 1 dereferenceable(1) %2)
          to label %28 unwind label %37

28:                                               ; preds = %24
  %29 = getelementptr inbounds %struct.task, ptr %1, i32 0, i32 0
  %30 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %29, i32 0, i32 0
  %31 = inttoptr i64 %27 to ptr
  store ptr %31, ptr %30, align 8
  call void @llvm.lifetime.start.p0(i64 1, ptr %5) #4
  call void @_ZN4task12promise_type15initial_suspendEv(ptr noundef nonnull align 1 dereferenceable(1) %2) #4
  %32 = call noundef i1 @_ZNKSt7__n486113suspend_never11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %5) #4
  br i1 %32, label %41, label %33

33:                                               ; preds = %28
  %34 = call token @llvm.coro.save(ptr null)
  call void @llvm.coro.await.suspend.void(ptr %5, ptr %26, ptr @_Z7pointerv.__await_suspend_wrapper__init) #4
  %35 = call i8 @llvm.coro.suspend(token %34, i1 false)
  switch i8 %35, label %124 [
    i8 0, label %41
    i8 1, label %36
  ]

36:                                               ; preds = %33
  br label %42

37:                                               ; preds = %24
  %38 = landingpad { ptr, i32 }
          cleanup
  %39 = extractvalue { ptr, i32 } %38, 0
  store ptr %39, ptr %3, align 8
  %40 = extractvalue { ptr, i32 } %38, 1
  store i32 %40, ptr %4, align 4
  br label %133

41:                                               ; preds = %33, %28
  call void @_ZNKSt7__n486113suspend_never12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %5) #4
  br label %42

42:                                               ; preds = %41, %36
  %43 = phi i32 [ 0, %41 ], [ 2, %36 ]
  call void @llvm.lifetime.end.p0(i64 1, ptr %5) #4
  switch i32 %43, label %116 [
    i32 0, label %44
  ]

44:                                               ; preds = %42
  call void @llvm.lifetime.start.p0(i64 4, ptr %7) #4
  store i32 0, ptr %7, align 4
  call void @llvm.lifetime.start.p0(i64 4, ptr %8) #4
  store i32 7, ptr %8, align 4
  call void @llvm.lifetime.start.p0(i64 8, ptr %9) #4
  store ptr %7, ptr %9, align 8
  call void @llvm.lifetime.start.p0(i64 1, ptr %10) #4
  %45 = call noundef i1 @_ZNKSt7__n486114suspend_always11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %10) #4
  br i1 %45, label %50, label %46

46:                                               ; preds = %44
  %47 = call token @llvm.coro.save(ptr null)
  call void @llvm.coro.await.suspend.void(ptr %10, ptr %26, ptr @_Z7pointerv.__await_suspend_wrapper__await) #4
  %48 = call i8 @llvm.coro.suspend(token %47, i1 false)
  switch i8 %48, label %124 [
    i8 0, label %50
    i8 1, label %49
  ]

49:                                               ; preds = %46
  br label %51

50:                                               ; preds = %46, %44
  call void @_ZNKSt7__n486114suspend_always12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %10) #4
  br label %51

51:                                               ; preds = %50, %49
  %52 = phi i32 [ 0, %50 ], [ 2, %49 ]
  call void @llvm.lifetime.end.p0(i64 1, ptr %10) #4
  switch i32 %52, label %79 [
    i32 0, label %53
  ]

53:                                               ; preds = %51
  %54 = load ptr, ptr %9, align 8
  store i32 1, ptr %54, align 4
  call void @llvm.lifetime.start.p0(i64 256, ptr %11) #4
  call void @llvm.lifetime.start.p0(i64 8, ptr %12) #4
  store ptr %11, ptr %12, align 8
  call void @llvm.lifetime.start.p0(i64 8, ptr %13) #4
  %55 = load ptr, ptr %12, align 8
  %56 = getelementptr inbounds [64 x i32], ptr %55, i64 0, i64 0
  store ptr %56, ptr %13, align 8
  call void @llvm.lifetime.start.p0(i64 8, ptr %14) #4
  %57 = load ptr, ptr %12, align 8
  %58 = getelementptr inbounds [64 x i32], ptr %57, i64 0, i64 0
  %59 = getelementptr inbounds i32, ptr %58, i64 64
  store ptr %59, ptr %14, align 8
  br label %60

60:                                               ; preds = %68, %53
  %61 = load ptr, ptr %13, align 8
  %62 = load ptr, ptr %14, align 8
  %63 = icmp ne ptr %61, %62
  br i1 %63, label %65, label %64

64:                                               ; preds = %60
  call void @llvm.lifetime.end.p0(i64 8, ptr %14) #4
  call void @llvm.lifetime.end.p0(i64 8, ptr %13) #4
  call void @llvm.lifetime.end.p0(i64 8, ptr %12) #4
  br label %71

65:                                               ; preds = %60
  call void @llvm.lifetime.start.p0(i64 8, ptr %15) #4
  %66 = load ptr, ptr %13, align 8
  store ptr %66, ptr %15, align 8
  %67 = load ptr, ptr %15, align 8
  store i32 -1, ptr %67, align 4
  call void @llvm.lifetime.end.p0(i64 8, ptr %15) #4
  br label %68

68:                                               ; preds = %65
  %69 = load ptr, ptr %13, align 8
  %70 = getelementptr inbounds i32, ptr %69, i32 1
  store ptr %70, ptr %13, align 8
  br label %60

71:                                               ; preds = %64
  %72 = load i32, ptr %7, align 4
  %73 = load i32, ptr %8, align 4
  %74 = getelementptr inbounds [64 x i32], ptr %11, i64 0, i64 0
  %75 = load i32, ptr %74, align 4
  %76 = add nsw i32 %73, %75
  %77 = add nsw i32 %76, 1
  invoke void @_Z3useii(i32 noundef %72, i32 noundef %77)
          to label %78 unwind label %83

78:                                               ; preds = %71
  call void @llvm.lifetime.end.p0(i64 256, ptr %11) #4
  br label %79

79:                                               ; preds = %78, %51
  %80 = phi i32 [ %52, %51 ], [ 0, %78 ]
  call void @llvm.lifetime.end.p0(i64 8, ptr %9) #4
  call void @llvm.lifetime.end.p0(i64 4, ptr %8) #4
  call void @llvm.lifetime.end.p0(i64 4, ptr %7) #4
  switch i32 %80, label %116 [
    i32 0, label %81
  ]

81:                                               ; preds = %79
  invoke void @_ZN4task12promise_type11return_voidEv(ptr noundef nonnull align 1 dereferenceable(1) %2)
          to label %82 unwind label %87

82:                                               ; preds = %81
  br label %97

83:                                               ; preds = %71
  %84 = landingpad { ptr, i32 }
          catch ptr null
  %85 = extractvalue { ptr, i32 } %84, 0
  store ptr %85, ptr %3, align 8
  %86 = extractvalue { ptr, i32 } %84, 1
  store i32 %86, ptr %4, align 4
  call void @llvm.lifetime.end.p0(i64 256, ptr %11) #4
  call void @llvm.lifetime.end.p0(i64 8, ptr %9) #4
  call void @llvm.lifetime.end.p0(i64 4, ptr %8) #4
  call void @llvm.lifetime.end.p0(i64 4, ptr %7) #4
  br label %91

87:                                               ; preds = %81
  %88 = landingpad { ptr, i32 }
          catch ptr null
  %89 = extractvalue { ptr, i32 } %88, 0
  store ptr %89, ptr %3, align 8
  %90 = extractvalue { ptr, i32 } %88, 1
  store i32 %90, ptr %4, align 4
  br label %91

91:                                               ; preds = %87, %83
  %92 = load ptr, ptr %3, align 8
  %93 = call ptr @__cxa_begin_catch(ptr %92) #4
  invoke void @_ZN4task12promise_type19unhandled_exceptionEv(ptr noundef nonnull align 1 dereferenceable(1) %2)
          to label %94 unwind label %103

94:                                               ; preds = %91
  invoke void @__cxa_end_catch()
          to label %95 unwind label %107

95:                                               ; preds = %94
  br label %96

96:                                               ; preds = %95
  br label %97

97:                                               ; preds = %96, %82
  call void @llvm.lifetime.start.p0(i64 1, ptr %16) #4
  call void @_ZN4task12promise_type13final_suspendEv(ptr noundef nonnull align 1 dereferenceable(1) %2) #4
  %98 = call noundef i1 @_ZNKSt7__n486114suspend_always11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %16) #4
  br i1 %98, label %112, label %99

99:                                               ; preds = %97
  %100 = call token @llvm.coro.save(ptr null)
  call void @llvm.coro.await.suspend.void(ptr %16, ptr %26, ptr @_Z7pointerv.__await_suspend_wrapper__final) #4
  %101 = call i8 @llvm.coro.suspend(token %100, i1 true)
  switch i8 %101, label %124 [
    i8 0, label %112
    i8 1, label %102
  ]

102:                                              ; preds = %99
  br label %113

103:                                              ; preds = %91
  %104 = landingpad { ptr, i32 }
          cleanup
  %105 = extractvalue { ptr, i32 } %104, 0
  store ptr %105, ptr %3, align 8
  %106 = extractvalue { ptr, i32 } %104, 1
  store i32 %106, ptr %4, align 4
  invoke void @__cxa_end_catch()
          to label %111 unwind label %144

107:                                              ; preds = %94
  %108 = landingpad { ptr, i32 }
          cleanup
  %109 = extractvalue { ptr, i32 } %108, 0
  store ptr %109, ptr %3, align 8
  %110 = extractvalue { ptr, i32 } %108, 1
  store i32 %110, ptr %4, align 4
  br label %130

111:                                              ; preds = %103
  br label %130

112:                                              ; preds = %99, %97
  call void @_ZNKSt7__n486114suspend_always12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %16) #4
  br label %113

113:                                              ; preds = %112, %102
  %114 = phi i32 [ 0, %112 ], [ 2, %102 ]
  call void @llvm.lifetime.end.p0(i64 1, ptr %16) #4
  switch i32 %114, label %116 [
    i32 0, label %115
  ]

115:                                              ; preds = %113
  br label %116

116:                                              ; preds = %115, %113, %79, %42
  %117 = phi i32 [ %43, %42 ], [ %80, %79 ], [ %114, %113 ], [ 0, %115 ]
  call void @llvm.lifetime.end.p0(i64 1, ptr %2) #4
  %118 = call ptr @llvm.coro.free(token %19, ptr %26)
  %119 = icmp ne ptr %118, null
  br i1 %119, label %120, label %122

120:                                              ; preds = %116
  %121 = call i64 @llvm.coro.size.i64()
  call void @_ZdlPvm(ptr noundef %118, i64 noundef %121) #4
  br label %122

122:                                              ; preds = %116, %120
  switch i32 %117, label %147 [
    i32 0, label %123
    i32 2, label %124
  ]

123:                                              ; preds = %122
  br label %124

124:                                              ; preds = %123, %122, %99, %46, %33
  %125 = call i1 @llvm.coro.end(ptr null, i1 false, token none)
  %126 = getelementptr inbounds %struct.task, ptr %1, i32 0, i32 0
  %127 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %126, i32 0, i32 0
  %128 = load ptr, ptr %127, align 8
  %129 = ptrtoint ptr %128 to i64
  ret i64 %129

130:                                              ; preds = %111, %107
  %131 = call i1 @llvm.coro.end(ptr null, i1 true, token none)
  br i1 %131, label %139, label %132

132:                                              ; preds = %130
  br label %133

133:                                              ; preds = %132, %37
  call void @llvm.lifetime.end.p0(i64 1, ptr %2) #4
  %134 = call ptr @llvm.coro.free(token %19, ptr %26)
  %135 = icmp ne ptr %134, null
  br i1 %135, label %136, label %138

136:                                              ; preds = %133
  %137 = call i64 @llvm.coro.size.i64()
  call void @_ZdlPvm(ptr noundef %134, i64 noundef %137) #4
  br label %138

138:                                              ; preds = %133, %136
  br label %139

139:                                              ; preds = %138, %130
  %140 = load ptr, ptr %3, align 8
  %141 = load i32, ptr %4, align 4
  %142 = insertvalue { ptr, i32 } poison, ptr %140, 0
  %143 = insertvalue { ptr, i32 } %142, i32 %141, 1
  resume { ptr, i32 } %143

144:                                              ; preds = %103
  %145 = landingpad { ptr, i32 }
          catch ptr null
  %146 = extractvalue { ptr, i32 } %145, 0
  call void @__clang_call_terminate(ptr %146) #16
  unreachable

147:                                              ; preds = %122
  unreachable
}

; Function Attrs: nocallback nofree nosync nounwind willreturn memory(argmem: read)
declare token @llvm.coro.id(i32, ptr readnone, ptr nocapture readonly, ptr) #3

; Function Attrs: nounwind
declare i1 @llvm.coro.alloc(token) #4

; Function Attrs: nobuiltin allocsize(0)
declare noundef nonnull ptr @_Znwm(i64 noundef) #5

; Function Attrs: nounwind memory(none)
declare i64 @llvm.coro.size.i64() #6

; Function Attrs: nounwind
declare ptr @llvm.coro.begin(token, ptr writeonly) #4

; Function Attrs: nocallback nofree nosync nounwind willreturn memory(argmem: readwrite)
declare void @llvm.lifetime.start.p0(i64 immarg, ptr nocapture) #7

; Function Attrs: mustprogress noinline optnone uwtable
define linkonce_odr dso_local i64 @_ZN4task12promise_type17get_return_objectEv(ptr noundef nonnull align 1 dereferenceable(1) %0) #0 comdat {
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
define linkonce_odr dso_local void @_ZN4task12promise_type15initial_suspendEv(ptr noundef nonnull align 1 dereferenceable(1) %0) #8 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  ret void
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local noundef i1 @_ZNKSt7__n486113suspend_never11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %0) #8 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  ret i1 true
}

; Function Attrs: nomerge nounwind
declare token @llvm.coro.save(ptr) #9

; Function Attrs: alwaysinline mustprogress
define private void @_Z7pointerv.__await_suspend_wrapper__init(ptr noundef nonnull %0, ptr noundef %1) #10 {
  %3 = alloca ptr, align 8
  %4 = alloca ptr, align 8
  %5 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %6 = alloca %"struct.std::__n4861::coroutine_handle", align 8
  store ptr %0, ptr %3, align 8
  store ptr %1, ptr %4, align 8
  %7 = load ptr, ptr %3, align 8
  %8 = load ptr, ptr %4, align 8
  %9 = call i64 @_ZNSt7__n486116coroutine_handleIN4task12promise_typeEE12from_addressEPv(ptr noundef %8) #4
  %10 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %6, i32 0, i32 0
  %11 = inttoptr i64 %9 to ptr
  store ptr %11, ptr %10, align 8
  %12 = call i64 @_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEEcvNS0_IvEEEv(ptr noundef nonnull align 8 dereferenceable(8) %6) #4
  %13 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %5, i32 0, i32 0
  %14 = inttoptr i64 %12 to ptr
  store ptr %14, ptr %13, align 8
  %15 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %5, i32 0, i32 0
  %16 = load ptr, ptr %15, align 8
  %17 = ptrtoint ptr %16 to i64
  call void @_ZNKSt7__n486113suspend_never13await_suspendENS_16coroutine_handleIvEE(ptr noundef nonnull align 1 dereferenceable(1) %7, i64 %17) #4
  ret void
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZNKSt7__n486113suspend_never13await_suspendENS_16coroutine_handleIvEE(ptr noundef nonnull align 1 dereferenceable(1) %0, i64 %1) #8 comdat {
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
define linkonce_odr dso_local i64 @_ZNSt7__n486116coroutine_handleIN4task12promise_typeEE12from_addressEPv(ptr noundef %0) #8 comdat {
  %2 = alloca %"struct.std::__n4861::coroutine_handle", align 8
  %3 = alloca ptr, align 8
  store ptr %0, ptr %3, align 8
  call void @_ZNSt7__n486116coroutine_handleIN4task12promise_typeEEC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %2) #4
  %4 = load ptr, ptr %3, align 8
  %5 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %2, i32 0, i32 0
  store ptr %4, ptr %5, align 8
  %6 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %2, i32 0, i32 0
  %7 = load ptr, ptr %6, align 8
  %8 = ptrtoint ptr %7 to i64
  ret i64 %8
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local i64 @_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEEcvNS0_IvEEEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #8 comdat {
  %2 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %3 = alloca ptr, align 8
  store ptr %0, ptr %3, align 8
  %4 = load ptr, ptr %3, align 8
  %5 = call noundef ptr @_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEE7addressEv(ptr noundef nonnull align 8 dereferenceable(8) %4) #4
  %6 = call i64 @_ZNSt7__n486116coroutine_handleIvE12from_addressEPv(ptr noundef %5) #4
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
declare i8 @llvm.coro.suspend(token, i1) #4

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZNKSt7__n486113suspend_never12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %0) #8 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  ret void
}

; Function Attrs: nocallback nofree nosync nounwind willreturn memory(argmem: readwrite)
declare void @llvm.lifetime.end.p0(i64 immarg, ptr nocapture) #7

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local noundef i1 @_ZNKSt7__n486114suspend_always11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %0) #8 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  ret i1 false
}

; Function Attrs: alwaysinline mustprogress
define private void @_Z7pointerv.__await_suspend_wrapper__await(ptr noundef nonnull %0, ptr noundef %1) #10 {
  %3 = alloca ptr, align 8
  %4 = alloca ptr, align 8
  %5 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %6 = alloca %"struct.std::__n4861::coroutine_handle", align 8
  store ptr %0, ptr %3, align 8
  store ptr %1, ptr %4, align 8
  %7 = load ptr, ptr %3, align 8
  %8 = load ptr, ptr %4, align 8
  %9 = call i64 @_ZNSt7__n486116coroutine_handleIN4task12promise_typeEE12from_addressEPv(ptr noundef %8) #4
  %10 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %6, i32 0, i32 0
  %11 = inttoptr i64 %9 to ptr
  store ptr %11, ptr %10, align 8
  %12 = call i64 @_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEEcvNS0_IvEEEv(ptr noundef nonnull align 8 dereferenceable(8) %6) #4
  %13 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %5, i32 0, i32 0
  %14 = inttoptr i64 %12 to ptr
  store ptr %14, ptr %13, align 8
  %15 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %5, i32 0, i32 0
  %16 = load ptr, ptr %15, align 8
  %17 = ptrtoint ptr %16 to i64
  call void @_ZNKSt7__n486114suspend_always13await_suspendENS_16coroutine_handleIvEE(ptr noundef nonnull align 1 dereferenceable(1) %7, i64 %17) #4
  ret void
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZNKSt7__n486114suspend_always13await_suspendENS_16coroutine_handleIvEE(ptr noundef nonnull align 1 dereferenceable(1) %0, i64 %1) #8 comdat {
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
define linkonce_odr dso_local void @_ZNKSt7__n486114suspend_always12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %0) #8 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  ret void
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZN4task12promise_type11return_voidEv(ptr noundef nonnull align 1 dereferenceable(1) %0) #8 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  ret void
}

declare ptr @__cxa_begin_catch(ptr)

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZN4task12promise_type19unhandled_exceptionEv(ptr noundef nonnull align 1 dereferenceable(1) %0) #8 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  ret void
}

declare void @__cxa_end_catch()

; Function Attrs: noinline noreturn nounwind uwtable
define linkonce_odr hidden void @__clang_call_terminate(ptr noundef %0) #11 comdat {
  %2 = call ptr @__cxa_begin_catch(ptr %0) #4
  call void @_ZSt9terminatev() #16
  unreachable
}

declare void @_ZSt9terminatev()

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZN4task12promise_type13final_suspendEv(ptr noundef nonnull align 1 dereferenceable(1) %0) #8 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  ret void
}

; Function Attrs: alwaysinline mustprogress
define private void @_Z7pointerv.__await_suspend_wrapper__final(ptr noundef nonnull %0, ptr noundef %1) #10 {
  %3 = alloca ptr, align 8
  %4 = alloca ptr, align 8
  %5 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %6 = alloca %"struct.std::__n4861::coroutine_handle", align 8
  store ptr %0, ptr %3, align 8
  store ptr %1, ptr %4, align 8
  %7 = load ptr, ptr %3, align 8
  %8 = load ptr, ptr %4, align 8
  %9 = call i64 @_ZNSt7__n486116coroutine_handleIN4task12promise_typeEE12from_addressEPv(ptr noundef %8) #4
  %10 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %6, i32 0, i32 0
  %11 = inttoptr i64 %9 to ptr
  store ptr %11, ptr %10, align 8
  %12 = call i64 @_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEEcvNS0_IvEEEv(ptr noundef nonnull align 8 dereferenceable(8) %6) #4
  %13 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %5, i32 0, i32 0
  %14 = inttoptr i64 %12 to ptr
  store ptr %14, ptr %13, align 8
  %15 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %5, i32 0, i32 0
  %16 = load ptr, ptr %15, align 8
  %17 = ptrtoint ptr %16 to i64
  call void @_ZNKSt7__n486114suspend_always13await_suspendENS_16coroutine_handleIvEE(ptr noundef nonnull align 1 dereferenceable(1) %7, i64 %17) #4
  ret void
}

; Function Attrs: nounwind
declare i1 @llvm.coro.end(ptr, i1, token) #4

; Function Attrs: nobuiltin nounwind
declare void @_ZdlPvm(ptr noundef, i64 noundef) #12

; Function Attrs: nounwind memory(argmem: read)
declare ptr @llvm.coro.free(token, ptr nocapture readonly) #13

; Function Attrs: mustprogress noinline optnone uwtable
define dso_local void @_ZN8registry4pokeEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #0 {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds %struct.registry, ptr %3, i32 0, i32 0
  %5 = load ptr, ptr %4, align 8
  call void @_ZN1T5helloEv(ptr noundef nonnull align 4 dereferenceable(4) %5)
  ret void
}

; Function Attrs: mustprogress noinline optnone uwtable
define linkonce_odr dso_local void @_ZN1T5helloEv(ptr noundef nonnull align 4 dereferenceable(4) %0) #0 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds %struct.T, ptr %3, i32 0, i32 0
  %5 = load i32, ptr %4, align 4
  %6 = call i32 (ptr, ...) @printf(ptr noundef @.str.1, i32 noundef %5)
  ret void
}

; Function Attrs: mustprogress noinline optnone presplitcoroutine uwtable
define dso_local i64 @_Z10registeredv() #2 personality ptr @__gxx_personality_v0 {
  %1 = alloca %struct.task, align 8
  %2 = alloca %"struct.task::promise_type", align 1
  %3 = alloca ptr, align 8
  %4 = alloca i32, align 4
  %5 = alloca %"struct.std::__n4861::suspend_never", align 1
  %6 = alloca %"struct.std::__n4861::suspend_never", align 1
  %7 = alloca %struct.T, align 4
  %8 = alloca %"struct.std::__n4861::suspend_always", align 1
  %9 = alloca [64 x i64], align 8
  %10 = alloca ptr, align 8
  %11 = alloca ptr, align 8
  %12 = alloca ptr, align 8
  %13 = alloca ptr, align 8
  %14 = alloca %"struct.std::__n4861::suspend_always", align 1
  %15 = alloca %"struct.std::__n4861::suspend_always", align 1
  %16 = bitcast ptr %2 to ptr
  %17 = call token @llvm.coro.id(i32 16, ptr %16, ptr null, ptr null)
  %18 = call i1 @llvm.coro.alloc(token %17)
  br i1 %18, label %19, label %22

19:                                               ; preds = %0
  %20 = call i64 @llvm.coro.size.i64()
  %21 = call noalias noundef nonnull ptr @_Znwm(i64 noundef %20) #15
  br label %22

22:                                               ; preds = %19, %0
  %23 = phi ptr [ null, %0 ], [ %21, %19 ]
  %24 = call ptr @llvm.coro.begin(token %17, ptr %23)
  call void @llvm.lifetime.start.p0(i64 1, ptr %2) #4
  %25 = invoke i64 @_ZN4task12promise_type17get_return_objectEv(ptr noundef nonnull align 1 dereferenceable(1) %2)
          to label %26 unwind label %35

26:                                               ; preds = %22
  %27 = getelementptr inbounds %struct.task, ptr %1, i32 0, i32 0
  %28 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %27, i32 0, i32 0
  %29 = inttoptr i64 %25 to ptr
  store ptr %29, ptr %28, align 8
  call void @llvm.lifetime.start.p0(i64 1, ptr %5) #4
  call void @_ZN4task12promise_type15initial_suspendEv(ptr noundef nonnull align 1 dereferenceable(1) %2) #4
  %30 = call noundef i1 @_ZNKSt7__n486113suspend_never11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %5) #4
  br i1 %30, label %39, label %31

31:                                               ; preds = %26
  %32 = call token @llvm.coro.save(ptr null)
  call void @llvm.coro.await.suspend.void(ptr %5, ptr %24, ptr @_Z10registeredv.__await_suspend_wrapper__init) #4
  %33 = call i8 @llvm.coro.suspend(token %32, i1 false)
  switch i8 %33, label %122 [
    i8 0, label %39
    i8 1, label %34
  ]

34:                                               ; preds = %31
  br label %40

35:                                               ; preds = %22
  %36 = landingpad { ptr, i32 }
          cleanup
  %37 = extractvalue { ptr, i32 } %36, 0
  store ptr %37, ptr %3, align 8
  %38 = extractvalue { ptr, i32 } %36, 1
  store i32 %38, ptr %4, align 4
  br label %131

39:                                               ; preds = %31, %26
  call void @_ZNKSt7__n486113suspend_never12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %5) #4
  br label %40

40:                                               ; preds = %39, %34
  %41 = phi i32 [ 0, %39 ], [ 2, %34 ]
  call void @llvm.lifetime.end.p0(i64 1, ptr %5) #4
  switch i32 %41, label %114 [
    i32 0, label %42
  ]

42:                                               ; preds = %40
  call void @llvm.lifetime.start.p0(i64 4, ptr %7) #4
  call void @_ZN1TC2Ev(ptr noundef nonnull align 4 dereferenceable(4) %7) #4
  invoke void @_ZN8registry3addEP1T(ptr noundef nonnull align 8 dereferenceable(8) @reg, ptr noundef %7)
          to label %43 unwind label %49

43:                                               ; preds = %42
  call void @llvm.lifetime.start.p0(i64 1, ptr %8) #4
  %44 = call noundef i1 @_ZNKSt7__n486114suspend_always11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %8) #4
  br i1 %44, label %53, label %45

45:                                               ; preds = %43
  %46 = call token @llvm.coro.save(ptr null)
  call void @llvm.coro.await.suspend.void(ptr %8, ptr %24, ptr @_Z10registeredv.__await_suspend_wrapper__await) #4
  %47 = call i8 @llvm.coro.suspend(token %46, i1 false)
  switch i8 %47, label %122 [
    i8 0, label %53
    i8 1, label %48
  ]

48:                                               ; preds = %45
  br label %54

49:                                               ; preds = %42
  %50 = landingpad { ptr, i32 }
          catch ptr null
  %51 = extractvalue { ptr, i32 } %50, 0
  store ptr %51, ptr %3, align 8
  %52 = extractvalue { ptr, i32 } %50, 1
  store i32 %52, ptr %4, align 4
  br label %84

53:                                               ; preds = %45, %43
  call void @_ZNKSt7__n486114suspend_always12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %8) #4
  br label %54

54:                                               ; preds = %53, %48
  %55 = phi i32 [ 0, %53 ], [ 2, %48 ]
  call void @llvm.lifetime.end.p0(i64 1, ptr %8) #4
  switch i32 %55, label %76 [
    i32 0, label %56
  ]

56:                                               ; preds = %54
  call void @llvm.lifetime.start.p0(i64 512, ptr %9) #4
  call void @llvm.lifetime.start.p0(i64 8, ptr %10) #4
  store ptr %9, ptr %10, align 8
  call void @llvm.lifetime.start.p0(i64 8, ptr %11) #4
  %57 = load ptr, ptr %10, align 8
  %58 = getelementptr inbounds [64 x i64], ptr %57, i64 0, i64 0
  store ptr %58, ptr %11, align 8
  call void @llvm.lifetime.start.p0(i64 8, ptr %12) #4
  %59 = load ptr, ptr %10, align 8
  %60 = getelementptr inbounds [64 x i64], ptr %59, i64 0, i64 0
  %61 = getelementptr inbounds i64, ptr %60, i64 64
  store ptr %61, ptr %12, align 8
  br label %62

62:                                               ; preds = %70, %56
  %63 = load ptr, ptr %11, align 8
  %64 = load ptr, ptr %12, align 8
  %65 = icmp ne ptr %63, %64
  br i1 %65, label %67, label %66

66:                                               ; preds = %62
  call void @llvm.lifetime.end.p0(i64 8, ptr %12) #4
  call void @llvm.lifetime.end.p0(i64 8, ptr %11) #4
  call void @llvm.lifetime.end.p0(i64 8, ptr %10) #4
  br label %73

67:                                               ; preds = %62
  call void @llvm.lifetime.start.p0(i64 8, ptr %13) #4
  %68 = load ptr, ptr %11, align 8
  store ptr %68, ptr %13, align 8
  %69 = load ptr, ptr %13, align 8
  store i64 1515870810, ptr %69, align 8
  call void @llvm.lifetime.end.p0(i64 8, ptr %13) #4
  br label %70

70:                                               ; preds = %67
  %71 = load ptr, ptr %11, align 8
  %72 = getelementptr inbounds i64, ptr %71, i32 1
  store ptr %72, ptr %11, align 8
  br label %62

73:                                               ; preds = %66
  invoke void @_ZN8registry4pokeEv(ptr noundef nonnull align 8 dereferenceable(8) @reg)
          to label %74 unwind label %80

74:                                               ; preds = %73
  %75 = getelementptr inbounds [64 x i64], ptr %9, i64 0, i64 1
  call void @llvm.lifetime.end.p0(i64 512, ptr %9) #4
  br label %76

76:                                               ; preds = %74, %54
  %77 = phi i32 [ %55, %54 ], [ 0, %74 ]
  call void @llvm.lifetime.end.p0(i64 4, ptr %7) #4
  switch i32 %77, label %114 [
    i32 0, label %78
  ]

78:                                               ; preds = %76
  invoke void @_ZN4task12promise_type11return_voidEv(ptr noundef nonnull align 1 dereferenceable(1) %2)
          to label %79 unwind label %85

79:                                               ; preds = %78
  br label %95

80:                                               ; preds = %73
  %81 = landingpad { ptr, i32 }
          catch ptr null
  %82 = extractvalue { ptr, i32 } %81, 0
  store ptr %82, ptr %3, align 8
  %83 = extractvalue { ptr, i32 } %81, 1
  store i32 %83, ptr %4, align 4
  call void @llvm.lifetime.end.p0(i64 512, ptr %9) #4
  br label %84

84:                                               ; preds = %80, %49
  call void @llvm.lifetime.end.p0(i64 4, ptr %7) #4
  br label %89

85:                                               ; preds = %78
  %86 = landingpad { ptr, i32 }
          catch ptr null
  %87 = extractvalue { ptr, i32 } %86, 0
  store ptr %87, ptr %3, align 8
  %88 = extractvalue { ptr, i32 } %86, 1
  store i32 %88, ptr %4, align 4
  br label %89

89:                                               ; preds = %85, %84
  %90 = load ptr, ptr %3, align 8
  %91 = call ptr @__cxa_begin_catch(ptr %90) #4
  invoke void @_ZN4task12promise_type19unhandled_exceptionEv(ptr noundef nonnull align 1 dereferenceable(1) %2)
          to label %92 unwind label %101

92:                                               ; preds = %89
  invoke void @__cxa_end_catch()
          to label %93 unwind label %105

93:                                               ; preds = %92
  br label %94

94:                                               ; preds = %93
  br label %95

95:                                               ; preds = %94, %79
  call void @llvm.lifetime.start.p0(i64 1, ptr %14) #4
  call void @_ZN4task12promise_type13final_suspendEv(ptr noundef nonnull align 1 dereferenceable(1) %2) #4
  %96 = call noundef i1 @_ZNKSt7__n486114suspend_always11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %14) #4
  br i1 %96, label %110, label %97

97:                                               ; preds = %95
  %98 = call token @llvm.coro.save(ptr null)
  call void @llvm.coro.await.suspend.void(ptr %14, ptr %24, ptr @_Z10registeredv.__await_suspend_wrapper__final) #4
  %99 = call i8 @llvm.coro.suspend(token %98, i1 true)
  switch i8 %99, label %122 [
    i8 0, label %110
    i8 1, label %100
  ]

100:                                              ; preds = %97
  br label %111

101:                                              ; preds = %89
  %102 = landingpad { ptr, i32 }
          cleanup
  %103 = extractvalue { ptr, i32 } %102, 0
  store ptr %103, ptr %3, align 8
  %104 = extractvalue { ptr, i32 } %102, 1
  store i32 %104, ptr %4, align 4
  invoke void @__cxa_end_catch()
          to label %109 unwind label %142

105:                                              ; preds = %92
  %106 = landingpad { ptr, i32 }
          cleanup
  %107 = extractvalue { ptr, i32 } %106, 0
  store ptr %107, ptr %3, align 8
  %108 = extractvalue { ptr, i32 } %106, 1
  store i32 %108, ptr %4, align 4
  br label %128

109:                                              ; preds = %101
  br label %128

110:                                              ; preds = %97, %95
  call void @_ZNKSt7__n486114suspend_always12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %14) #4
  br label %111

111:                                              ; preds = %110, %100
  %112 = phi i32 [ 0, %110 ], [ 2, %100 ]
  call void @llvm.lifetime.end.p0(i64 1, ptr %14) #4
  switch i32 %112, label %114 [
    i32 0, label %113
  ]

113:                                              ; preds = %111
  br label %114

114:                                              ; preds = %113, %111, %76, %40
  %115 = phi i32 [ %41, %40 ], [ %77, %76 ], [ %112, %111 ], [ 0, %113 ]
  call void @llvm.lifetime.end.p0(i64 1, ptr %2) #4
  %116 = call ptr @llvm.coro.free(token %17, ptr %24)
  %117 = icmp ne ptr %116, null
  br i1 %117, label %118, label %120

118:                                              ; preds = %114
  %119 = call i64 @llvm.coro.size.i64()
  call void @_ZdlPvm(ptr noundef %116, i64 noundef %119) #4
  br label %120

120:                                              ; preds = %114, %118
  switch i32 %115, label %145 [
    i32 0, label %121
    i32 2, label %122
  ]

121:                                              ; preds = %120
  br label %122

122:                                              ; preds = %121, %120, %97, %45, %31
  %123 = call i1 @llvm.coro.end(ptr null, i1 false, token none)
  %124 = getelementptr inbounds %struct.task, ptr %1, i32 0, i32 0
  %125 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %124, i32 0, i32 0
  %126 = load ptr, ptr %125, align 8
  %127 = ptrtoint ptr %126 to i64
  ret i64 %127

128:                                              ; preds = %109, %105
  %129 = call i1 @llvm.coro.end(ptr null, i1 true, token none)
  br i1 %129, label %137, label %130

130:                                              ; preds = %128
  br label %131

131:                                              ; preds = %130, %35
  call void @llvm.lifetime.end.p0(i64 1, ptr %2) #4
  %132 = call ptr @llvm.coro.free(token %17, ptr %24)
  %133 = icmp ne ptr %132, null
  br i1 %133, label %134, label %136

134:                                              ; preds = %131
  %135 = call i64 @llvm.coro.size.i64()
  call void @_ZdlPvm(ptr noundef %132, i64 noundef %135) #4
  br label %136

136:                                              ; preds = %131, %134
  br label %137

137:                                              ; preds = %136, %128
  %138 = load ptr, ptr %3, align 8
  %139 = load i32, ptr %4, align 4
  %140 = insertvalue { ptr, i32 } poison, ptr %138, 0
  %141 = insertvalue { ptr, i32 } %140, i32 %139, 1
  resume { ptr, i32 } %141

142:                                              ; preds = %101
  %143 = landingpad { ptr, i32 }
          catch ptr null
  %144 = extractvalue { ptr, i32 } %143, 0
  call void @__clang_call_terminate(ptr %144) #16
  unreachable

145:                                              ; preds = %120
  unreachable
}

; Function Attrs: alwaysinline mustprogress
define private void @_Z10registeredv.__await_suspend_wrapper__init(ptr noundef nonnull %0, ptr noundef %1) #10 {
  %3 = alloca ptr, align 8
  %4 = alloca ptr, align 8
  %5 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %6 = alloca %"struct.std::__n4861::coroutine_handle", align 8
  store ptr %0, ptr %3, align 8
  store ptr %1, ptr %4, align 8
  %7 = load ptr, ptr %3, align 8
  %8 = load ptr, ptr %4, align 8
  %9 = call i64 @_ZNSt7__n486116coroutine_handleIN4task12promise_typeEE12from_addressEPv(ptr noundef %8) #4
  %10 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %6, i32 0, i32 0
  %11 = inttoptr i64 %9 to ptr
  store ptr %11, ptr %10, align 8
  %12 = call i64 @_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEEcvNS0_IvEEEv(ptr noundef nonnull align 8 dereferenceable(8) %6) #4
  %13 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %5, i32 0, i32 0
  %14 = inttoptr i64 %12 to ptr
  store ptr %14, ptr %13, align 8
  %15 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %5, i32 0, i32 0
  %16 = load ptr, ptr %15, align 8
  %17 = ptrtoint ptr %16 to i64
  call void @_ZNKSt7__n486113suspend_never13await_suspendENS_16coroutine_handleIvEE(ptr noundef nonnull align 1 dereferenceable(1) %7, i64 %17) #4
  ret void
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZN1TC2Ev(ptr noundef nonnull align 4 dereferenceable(4) %0) unnamed_addr #8 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds %struct.T, ptr %3, i32 0, i32 0
  store i32 5, ptr %4, align 4
  ret void
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZN8registry3addEP1T(ptr noundef nonnull align 8 dereferenceable(8) %0, ptr noundef %1) #8 comdat {
  %3 = alloca ptr, align 8
  %4 = alloca ptr, align 8
  store ptr %0, ptr %3, align 8
  store ptr %1, ptr %4, align 8
  %5 = load ptr, ptr %3, align 8
  %6 = load ptr, ptr %4, align 8
  %7 = getelementptr inbounds %struct.registry, ptr %5, i32 0, i32 0
  store ptr %6, ptr %7, align 8
  ret void
}

; Function Attrs: alwaysinline mustprogress
define private void @_Z10registeredv.__await_suspend_wrapper__await(ptr noundef nonnull %0, ptr noundef %1) #10 {
  %3 = alloca ptr, align 8
  %4 = alloca ptr, align 8
  %5 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %6 = alloca %"struct.std::__n4861::coroutine_handle", align 8
  store ptr %0, ptr %3, align 8
  store ptr %1, ptr %4, align 8
  %7 = load ptr, ptr %3, align 8
  %8 = load ptr, ptr %4, align 8
  %9 = call i64 @_ZNSt7__n486116coroutine_handleIN4task12promise_typeEE12from_addressEPv(ptr noundef %8) #4
  %10 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %6, i32 0, i32 0
  %11 = inttoptr i64 %9 to ptr
  store ptr %11, ptr %10, align 8
  %12 = call i64 @_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEEcvNS0_IvEEEv(ptr noundef nonnull align 8 dereferenceable(8) %6) #4
  %13 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %5, i32 0, i32 0
  %14 = inttoptr i64 %12 to ptr
  store ptr %14, ptr %13, align 8
  %15 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %5, i32 0, i32 0
  %16 = load ptr, ptr %15, align 8
  %17 = ptrtoint ptr %16 to i64
  call void @_ZNKSt7__n486114suspend_always13await_suspendENS_16coroutine_handleIvEE(ptr noundef nonnull align 1 dereferenceable(1) %7, i64 %17) #4
  ret void
}

; Function Attrs: alwaysinline mustprogress
define private void @_Z10registeredv.__await_suspend_wrapper__final(ptr noundef nonnull %0, ptr noundef %1) #10 {
  %3 = alloca ptr, align 8
  %4 = alloca ptr, align 8
  %5 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %6 = alloca %"struct.std::__n4861::coroutine_handle", align 8
  store ptr %0, ptr %3, align 8
  store ptr %1, ptr %4, align 8
  %7 = load ptr, ptr %3, align 8
  %8 = load ptr, ptr %4, align 8
  %9 = call i64 @_ZNSt7__n486116coroutine_handleIN4task12promise_typeEE12from_addressEPv(ptr noundef %8) #4
  %10 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %6, i32 0, i32 0
  %11 = inttoptr i64 %9 to ptr
  store ptr %11, ptr %10, align 8
  %12 = call i64 @_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEEcvNS0_IvEEEv(ptr noundef nonnull align 8 dereferenceable(8) %6) #4
  %13 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %5, i32 0, i32 0
  %14 = inttoptr i64 %12 to ptr
  store ptr %14, ptr %13, align 8
  %15 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %5, i32 0, i32 0
  %16 = load ptr, ptr %15, align 8
  %17 = ptrtoint ptr %16 to i64
  call void @_ZNKSt7__n486114suspend_always13await_suspendENS_16coroutine_handleIvEE(ptr noundef nonnull align 1 dereferenceable(1) %7, i64 %17) #4
  ret void
}

; Function Attrs: mustprogress noinline optnone uwtable
define dso_local void @_Z11resume_deepNSt7__n486116coroutine_handleIvEE(i64 %0) #0 {
  %2 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %3 = alloca [256 x i64], align 8
  %4 = alloca i32, align 4
  %5 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %2, i32 0, i32 0
  %6 = inttoptr i64 %0 to ptr
  store ptr %6, ptr %5, align 8
  store i32 0, ptr %4, align 4
  br label %7

7:                                                ; preds = %14, %1
  %8 = load i32, ptr %4, align 4
  %9 = icmp slt i32 %8, 256
  br i1 %9, label %10, label %17

10:                                               ; preds = %7
  %11 = load i32, ptr %4, align 4
  %12 = sext i32 %11 to i64
  %13 = getelementptr inbounds [256 x i64], ptr %3, i64 0, i64 %12
  store volatile i64 6510615555426900570, ptr %13, align 8
  br label %14

14:                                               ; preds = %10
  %15 = load i32, ptr %4, align 4
  %16 = add nsw i32 %15, 1
  store i32 %16, ptr %4, align 4
  br label %7, !llvm.loop !6

17:                                               ; preds = %7
  call void @_ZNKSt7__n486116coroutine_handleIvE6resumeEv(ptr noundef nonnull align 8 dereferenceable(8) %2)
  ret void
}

; Function Attrs: mustprogress noinline optnone uwtable
define linkonce_odr dso_local void @_ZNKSt7__n486116coroutine_handleIvE6resumeEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #0 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %3, i32 0, i32 0
  %5 = load ptr, ptr %4, align 8
  call void @llvm.coro.resume(ptr %5)
  ret void
}

; Function Attrs: mustprogress noinline norecurse optnone uwtable
define dso_local noundef i32 @main() #14 {
  %1 = alloca %struct.task, align 8
  %2 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %3 = alloca %struct.task, align 8
  %4 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %5 = call i64 @_Z7pointerv()
  %6 = getelementptr inbounds %struct.task, ptr %1, i32 0, i32 0
  %7 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %6, i32 0, i32 0
  %8 = inttoptr i64 %5 to ptr
  store ptr %8, ptr %7, align 8
  %9 = getelementptr inbounds %struct.task, ptr %1, i32 0, i32 0
  %10 = call i64 @_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEEcvNS0_IvEEEv(ptr noundef nonnull align 8 dereferenceable(8) %9) #4
  %11 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %2, i32 0, i32 0
  %12 = inttoptr i64 %10 to ptr
  store ptr %12, ptr %11, align 8
  %13 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %2, i32 0, i32 0
  %14 = load ptr, ptr %13, align 8
  %15 = ptrtoint ptr %14 to i64
  call void @_Z11resume_deepNSt7__n486116coroutine_handleIvEE(i64 %15)
  %16 = call i64 @_Z10registeredv()
  %17 = getelementptr inbounds %struct.task, ptr %3, i32 0, i32 0
  %18 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %17, i32 0, i32 0
  %19 = inttoptr i64 %16 to ptr
  store ptr %19, ptr %18, align 8
  %20 = getelementptr inbounds %struct.task, ptr %3, i32 0, i32 0
  %21 = call i64 @_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEEcvNS0_IvEEEv(ptr noundef nonnull align 8 dereferenceable(8) %20) #4
  %22 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %4, i32 0, i32 0
  %23 = inttoptr i64 %21 to ptr
  store ptr %23, ptr %22, align 8
  %24 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %4, i32 0, i32 0
  %25 = load ptr, ptr %24, align 8
  %26 = ptrtoint ptr %25 to i64
  call void @_Z11resume_deepNSt7__n486116coroutine_handleIvEE(i64 %26)
  %27 = getelementptr inbounds %struct.task, ptr %1, i32 0, i32 0
  call void @_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEE7destroyEv(ptr noundef nonnull align 8 dereferenceable(8) %27)
  %28 = getelementptr inbounds %struct.task, ptr %3, i32 0, i32 0
  call void @_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEE7destroyEv(ptr noundef nonnull align 8 dereferenceable(8) %28)
  ret i32 0
}

; Function Attrs: mustprogress noinline optnone uwtable
define linkonce_odr dso_local void @_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEE7destroyEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #0 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %3, i32 0, i32 0
  %5 = load ptr, ptr %4, align 8
  call void @llvm.coro.destroy(ptr %5)
  ret void
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local i64 @_ZNSt7__n486116coroutine_handleIN4task12promise_typeEE12from_promiseERS2_(ptr noundef nonnull align 1 dereferenceable(1) %0) #8 comdat {
  %2 = alloca %"struct.std::__n4861::coroutine_handle", align 8
  %3 = alloca ptr, align 8
  store ptr %0, ptr %3, align 8
  call void @_ZNSt7__n486116coroutine_handleIN4task12promise_typeEEC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %2) #4
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
define linkonce_odr dso_local void @_ZNSt7__n486116coroutine_handleIN4task12promise_typeEEC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %0) unnamed_addr #8 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %3, i32 0, i32 0
  store ptr null, ptr %4, align 8
  ret void
}

; Function Attrs: nounwind memory(none)
declare ptr @llvm.coro.promise(ptr nocapture, i32, i1) #6

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local i64 @_ZNSt7__n486116coroutine_handleIvE12from_addressEPv(ptr noundef %0) #8 comdat {
  %2 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %3 = alloca ptr, align 8
  store ptr %0, ptr %3, align 8
  call void @_ZNSt7__n486116coroutine_handleIvEC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %2) #4
  %4 = load ptr, ptr %3, align 8
  %5 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %2, i32 0, i32 0
  store ptr %4, ptr %5, align 8
  %6 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %2, i32 0, i32 0
  %7 = load ptr, ptr %6, align 8
  %8 = ptrtoint ptr %7 to i64
  ret i64 %8
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local noundef ptr @_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEE7addressEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #8 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %3, i32 0, i32 0
  %5 = load ptr, ptr %4, align 8
  ret ptr %5
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZNSt7__n486116coroutine_handleIvEC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %0) unnamed_addr #8 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %3, i32 0, i32 0
  store ptr null, ptr %4, align 8
  ret void
}

declare void @llvm.coro.resume(ptr)

declare void @llvm.coro.destroy(ptr)

attributes #0 = { mustprogress noinline optnone uwtable "frame-pointer"="non-leaf" "no-trapping-math"="true" "stack-protector-buffer-size"="8" }
attributes #1 = { "frame-pointer"="non-leaf" "no-trapping-math"="true" "stack-protector-buffer-size"="8" }
attributes #2 = { mustprogress noinline optnone presplitcoroutine uwtable "frame-pointer"="non-leaf" "no-trapping-math"="true" "stack-protector-buffer-size"="8" }
attributes #3 = { nocallback nofree nosync nounwind willreturn memory(argmem: read) }
attributes #4 = { nounwind }
attributes #5 = { nobuiltin allocsize(0) "frame-pointer"="non-leaf" "no-trapping-math"="true" "stack-protector-buffer-size"="8" }
attributes #6 = { nounwind memory(none) }
attributes #7 = { nocallback nofree nosync nounwind willreturn memory(argmem: readwrite) }
attributes #8 = { mustprogress noinline nounwind optnone uwtable "frame-pointer"="non-leaf" "no-trapping-math"="true" "stack-protector-buffer-size"="8" }
attributes #9 = { nomerge nounwind }
attributes #10 = { alwaysinline mustprogress }
attributes #11 = { noinline noreturn nounwind uwtable "frame-pointer"="non-leaf" "no-trapping-math"="true" "stack-protector-buffer-size"="8" }
attributes #12 = { nobuiltin nounwind "frame-pointer"="non-leaf" "no-trapping-math"="true" "stack-protector-buffer-size"="8" }
attributes #13 = { nounwind memory(argmem: read) }
attributes #14 = { mustprogress noinline norecurse optnone uwtable "frame-pointer"="non-leaf" "no-trapping-math"="true" "stack-protector-buffer-size"="8" }
attributes #15 = { allocsize(0) }
attributes #16 = { noreturn nounwind }

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
