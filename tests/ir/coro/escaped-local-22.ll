; clang++-22 -std=c++20 -O0 -S -emit-llvm -Xclang -disable-llvm-passes
; (aarch64-unknown-linux-gnu; the target triple and datalayout removed) of:
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
; int main() {
;   task a = pointer();
;   long noise[256];  // overwrite the ramp's dead stack between ramp and resume
;   for (long& n : noise) n = 0x5a5a5a5a5a5a5a5a;
;   a.h.resume();
;   task b = registered();
;   for (long& n : noise) n = 0x5a5a5a5a5a5a5a5a;
;   b.h.resume();
;   a.h.destroy();
;   b.h.destroy();
;   return int(noise[3] & 0);
; }
;
;@ covers: spec/ir.md §5.5 intrinsics (coroutine bodies; spec/codegen.md §7.6)
;@ input: pre-opt
;@ dialect: 22
;@ link: c++
;@ unsupported(x86_64): *
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

$_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEE6resumeEv = comdat any

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
  %18 = call token @llvm.coro.id(i32 16, ptr %2, ptr null, ptr null)
  %19 = call i1 @llvm.coro.alloc(token %18)
  br i1 %19, label %20, label %23

20:                                               ; preds = %0
  %21 = call i64 @llvm.coro.size.i64()
  %22 = call noalias noundef nonnull ptr @_Znwm(i64 noundef %21) #15
  br label %23

23:                                               ; preds = %20, %0
  %24 = phi ptr [ null, %0 ], [ %22, %20 ]
  %25 = call ptr @llvm.coro.begin(token %18, ptr %24)
  call void @llvm.lifetime.start.p0(ptr %2) #4
  %26 = invoke i64 @_ZN4task12promise_type17get_return_objectEv(ptr noundef nonnull align 1 dereferenceable(1) %2)
          to label %27 unwind label %36

27:                                               ; preds = %23
  %28 = getelementptr inbounds nuw %struct.task, ptr %1, i32 0, i32 0
  %29 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %28, i32 0, i32 0
  %30 = inttoptr i64 %26 to ptr
  store ptr %30, ptr %29, align 8
  call void @llvm.lifetime.start.p0(ptr %5) #4
  call void @_ZN4task12promise_type15initial_suspendEv(ptr noundef nonnull align 1 dereferenceable(1) %2) #4
  %31 = call noundef i1 @_ZNKSt7__n486113suspend_never11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %5) #4
  br i1 %31, label %40, label %32

32:                                               ; preds = %27
  %33 = call token @llvm.coro.save(ptr null)
  call void @llvm.coro.await.suspend.void(ptr %5, ptr %25, ptr @_Z7pointerv.__await_suspend_wrapper__init) #4
  %34 = call i8 @llvm.coro.suspend(token %33, i1 false)
  switch i8 %34, label %123 [
    i8 0, label %40
    i8 1, label %35
  ]

35:                                               ; preds = %32
  br label %41

36:                                               ; preds = %23
  %37 = landingpad { ptr, i32 }
          cleanup
  %38 = extractvalue { ptr, i32 } %37, 0
  store ptr %38, ptr %3, align 8
  %39 = extractvalue { ptr, i32 } %37, 1
  store i32 %39, ptr %4, align 4
  br label %131

40:                                               ; preds = %32, %27
  call void @_ZNKSt7__n486113suspend_never12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %5) #4
  br label %41

41:                                               ; preds = %40, %35
  %42 = phi i32 [ 0, %40 ], [ 2, %35 ]
  call void @llvm.lifetime.end.p0(ptr %5) #4
  switch i32 %42, label %115 [
    i32 0, label %43
  ]

43:                                               ; preds = %41
  call void @llvm.lifetime.start.p0(ptr %7) #4
  store i32 0, ptr %7, align 4
  call void @llvm.lifetime.start.p0(ptr %8) #4
  store i32 7, ptr %8, align 4
  call void @llvm.lifetime.start.p0(ptr %9) #4
  store ptr %7, ptr %9, align 8
  call void @llvm.lifetime.start.p0(ptr %10) #4
  %44 = call noundef i1 @_ZNKSt7__n486114suspend_always11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %10) #4
  br i1 %44, label %49, label %45

45:                                               ; preds = %43
  %46 = call token @llvm.coro.save(ptr null)
  call void @llvm.coro.await.suspend.void(ptr %10, ptr %25, ptr @_Z7pointerv.__await_suspend_wrapper__await) #4
  %47 = call i8 @llvm.coro.suspend(token %46, i1 false)
  switch i8 %47, label %123 [
    i8 0, label %49
    i8 1, label %48
  ]

48:                                               ; preds = %45
  br label %50

49:                                               ; preds = %45, %43
  call void @_ZNKSt7__n486114suspend_always12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %10) #4
  br label %50

50:                                               ; preds = %49, %48
  %51 = phi i32 [ 0, %49 ], [ 2, %48 ]
  call void @llvm.lifetime.end.p0(ptr %10) #4
  switch i32 %51, label %78 [
    i32 0, label %52
  ]

52:                                               ; preds = %50
  %53 = load ptr, ptr %9, align 8
  store i32 1, ptr %53, align 4
  call void @llvm.lifetime.start.p0(ptr %11) #4
  call void @llvm.lifetime.start.p0(ptr %12) #4
  store ptr %11, ptr %12, align 8
  call void @llvm.lifetime.start.p0(ptr %13) #4
  %54 = load ptr, ptr %12, align 8, !nonnull !6, !align !7
  %55 = getelementptr inbounds [64 x i32], ptr %54, i64 0, i64 0
  store ptr %55, ptr %13, align 8
  call void @llvm.lifetime.start.p0(ptr %14) #4
  %56 = load ptr, ptr %12, align 8, !nonnull !6, !align !7
  %57 = getelementptr inbounds [64 x i32], ptr %56, i64 0, i64 0
  %58 = getelementptr inbounds i32, ptr %57, i64 64
  store ptr %58, ptr %14, align 8
  br label %59

59:                                               ; preds = %67, %52
  %60 = load ptr, ptr %13, align 8
  %61 = load ptr, ptr %14, align 8
  %62 = icmp ne ptr %60, %61
  br i1 %62, label %64, label %63

63:                                               ; preds = %59
  call void @llvm.lifetime.end.p0(ptr %14) #4
  call void @llvm.lifetime.end.p0(ptr %13) #4
  call void @llvm.lifetime.end.p0(ptr %12) #4
  br label %70

64:                                               ; preds = %59
  call void @llvm.lifetime.start.p0(ptr %15) #4
  %65 = load ptr, ptr %13, align 8
  store ptr %65, ptr %15, align 8
  %66 = load ptr, ptr %15, align 8, !nonnull !6, !align !7
  store i32 -1, ptr %66, align 4
  call void @llvm.lifetime.end.p0(ptr %15) #4
  br label %67

67:                                               ; preds = %64
  %68 = load ptr, ptr %13, align 8
  %69 = getelementptr inbounds nuw i32, ptr %68, i32 1
  store ptr %69, ptr %13, align 8
  br label %59

70:                                               ; preds = %63
  %71 = load i32, ptr %7, align 4
  %72 = load i32, ptr %8, align 4
  %73 = getelementptr inbounds [64 x i32], ptr %11, i64 0, i64 0
  %74 = load i32, ptr %73, align 4
  %75 = add nsw i32 %72, %74
  %76 = add nsw i32 %75, 1
  invoke void @_Z3useii(i32 noundef %71, i32 noundef %76)
          to label %77 unwind label %82

77:                                               ; preds = %70
  call void @llvm.lifetime.end.p0(ptr %11) #4
  br label %78

78:                                               ; preds = %77, %50
  %79 = phi i32 [ %51, %50 ], [ 0, %77 ]
  call void @llvm.lifetime.end.p0(ptr %9) #4
  call void @llvm.lifetime.end.p0(ptr %8) #4
  call void @llvm.lifetime.end.p0(ptr %7) #4
  switch i32 %79, label %115 [
    i32 0, label %80
  ]

80:                                               ; preds = %78
  invoke void @_ZN4task12promise_type11return_voidEv(ptr noundef nonnull align 1 dereferenceable(1) %2)
          to label %81 unwind label %86

81:                                               ; preds = %80
  br label %96

82:                                               ; preds = %70
  %83 = landingpad { ptr, i32 }
          catch ptr null
  %84 = extractvalue { ptr, i32 } %83, 0
  store ptr %84, ptr %3, align 8
  %85 = extractvalue { ptr, i32 } %83, 1
  store i32 %85, ptr %4, align 4
  call void @llvm.lifetime.end.p0(ptr %11) #4
  call void @llvm.lifetime.end.p0(ptr %9) #4
  call void @llvm.lifetime.end.p0(ptr %8) #4
  call void @llvm.lifetime.end.p0(ptr %7) #4
  br label %90

86:                                               ; preds = %80
  %87 = landingpad { ptr, i32 }
          catch ptr null
  %88 = extractvalue { ptr, i32 } %87, 0
  store ptr %88, ptr %3, align 8
  %89 = extractvalue { ptr, i32 } %87, 1
  store i32 %89, ptr %4, align 4
  br label %90

90:                                               ; preds = %86, %82
  %91 = load ptr, ptr %3, align 8
  %92 = call ptr @__cxa_begin_catch(ptr %91) #4
  invoke void @_ZN4task12promise_type19unhandled_exceptionEv(ptr noundef nonnull align 1 dereferenceable(1) %2)
          to label %93 unwind label %102

93:                                               ; preds = %90
  invoke void @__cxa_end_catch()
          to label %94 unwind label %106

94:                                               ; preds = %93
  br label %95

95:                                               ; preds = %94
  br label %96

96:                                               ; preds = %95, %81
  call void @llvm.lifetime.start.p0(ptr %16) #4
  call void @_ZN4task12promise_type13final_suspendEv(ptr noundef nonnull align 1 dereferenceable(1) %2) #4
  %97 = call noundef i1 @_ZNKSt7__n486114suspend_always11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %16) #4
  br i1 %97, label %111, label %98

98:                                               ; preds = %96
  %99 = call token @llvm.coro.save(ptr null)
  call void @llvm.coro.await.suspend.void(ptr %16, ptr %25, ptr @_Z7pointerv.__await_suspend_wrapper__final) #4
  %100 = call i8 @llvm.coro.suspend(token %99, i1 true)
  switch i8 %100, label %123 [
    i8 0, label %111
    i8 1, label %101
  ]

101:                                              ; preds = %98
  br label %112

102:                                              ; preds = %90
  %103 = landingpad { ptr, i32 }
          cleanup
  %104 = extractvalue { ptr, i32 } %103, 0
  store ptr %104, ptr %3, align 8
  %105 = extractvalue { ptr, i32 } %103, 1
  store i32 %105, ptr %4, align 4
  invoke void @__cxa_end_catch()
          to label %110 unwind label %142

106:                                              ; preds = %93
  %107 = landingpad { ptr, i32 }
          cleanup
  %108 = extractvalue { ptr, i32 } %107, 0
  store ptr %108, ptr %3, align 8
  %109 = extractvalue { ptr, i32 } %107, 1
  store i32 %109, ptr %4, align 4
  br label %128

110:                                              ; preds = %102
  br label %128

111:                                              ; preds = %98, %96
  call void @_ZNKSt7__n486114suspend_always12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %16) #4
  br label %112

112:                                              ; preds = %111, %101
  %113 = phi i32 [ 0, %111 ], [ 2, %101 ]
  call void @llvm.lifetime.end.p0(ptr %16) #4
  switch i32 %113, label %115 [
    i32 0, label %114
  ]

114:                                              ; preds = %112
  br label %115

115:                                              ; preds = %114, %112, %78, %41
  %116 = phi i32 [ %42, %41 ], [ %79, %78 ], [ %113, %112 ], [ 0, %114 ]
  call void @llvm.lifetime.end.p0(ptr %2) #4
  %117 = call ptr @llvm.coro.free(token %18, ptr %25)
  %118 = icmp ne ptr %117, null
  br i1 %118, label %119, label %121

119:                                              ; preds = %115
  %120 = call i64 @llvm.coro.size.i64()
  call void @_ZdlPvm(ptr noundef %117, i64 noundef %120) #4
  br label %121

121:                                              ; preds = %115, %119
  switch i32 %116, label %145 [
    i32 0, label %122
    i32 2, label %123
  ]

122:                                              ; preds = %121
  br label %123

123:                                              ; preds = %122, %121, %98, %45, %32
  call void @llvm.coro.end(ptr null, i1 false, token none)
  %124 = getelementptr inbounds nuw %struct.task, ptr %1, i32 0, i32 0
  %125 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %124, i32 0, i32 0
  %126 = load ptr, ptr %125, align 8
  %127 = ptrtoint ptr %126 to i64
  ret i64 %127

128:                                              ; preds = %110, %106
  call void @llvm.coro.end(ptr null, i1 true, token none)
  %129 = call i1 @llvm.coro.is_in_ramp()
  br i1 %129, label %130, label %137

130:                                              ; preds = %128
  br label %131

131:                                              ; preds = %130, %36
  call void @llvm.lifetime.end.p0(ptr %2) #4
  %132 = call ptr @llvm.coro.free(token %18, ptr %25)
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

142:                                              ; preds = %102
  %143 = landingpad { ptr, i32 }
          catch ptr null
  %144 = extractvalue { ptr, i32 } %143, 0
  call void @__clang_call_terminate(ptr %144) #16
  unreachable

145:                                              ; preds = %121
  unreachable
}

; Function Attrs: nocallback nofree nosync nounwind willreturn memory(argmem: read)
declare token @llvm.coro.id(i32, ptr readnone, ptr readonly captures(none), ptr) #3

; Function Attrs: nounwind
declare i1 @llvm.coro.alloc(token) #4

; Function Attrs: nobuiltin allocsize(0)
declare noundef nonnull ptr @_Znwm(i64 noundef) #5

; Function Attrs: nounwind memory(none)
declare i64 @llvm.coro.size.i64() #6

; Function Attrs: nounwind
declare ptr @llvm.coro.begin(token, ptr writeonly) #4

; Function Attrs: nocallback nofree nosync nounwind willreturn memory(argmem: readwrite)
declare void @llvm.lifetime.start.p0(ptr captures(none)) #7

; Function Attrs: mustprogress noinline optnone uwtable
define linkonce_odr dso_local i64 @_ZN4task12promise_type17get_return_objectEv(ptr noundef nonnull align 1 dereferenceable(1) %0) #0 comdat {
  %2 = alloca %struct.task, align 8
  %3 = alloca ptr, align 8
  store ptr %0, ptr %3, align 8
  %4 = load ptr, ptr %3, align 8
  %5 = getelementptr inbounds nuw %struct.task, ptr %2, i32 0, i32 0
  %6 = call i64 @_ZNSt7__n486116coroutine_handleIN4task12promise_typeEE12from_promiseERS2_(ptr noundef nonnull align 1 dereferenceable(1) %4)
  %7 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %5, i32 0, i32 0
  %8 = inttoptr i64 %6 to ptr
  store ptr %8, ptr %7, align 8
  %9 = getelementptr inbounds nuw %struct.task, ptr %2, i32 0, i32 0
  %10 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %9, i32 0, i32 0
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
define internal void @_Z7pointerv.__await_suspend_wrapper__init(ptr noundef nonnull %0, ptr noundef %1) #10 {
  %3 = alloca ptr, align 8
  %4 = alloca ptr, align 8
  %5 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %6 = alloca %"struct.std::__n4861::coroutine_handle", align 8
  store ptr %0, ptr %3, align 8
  store ptr %1, ptr %4, align 8
  %7 = load ptr, ptr %3, align 8
  %8 = load ptr, ptr %4, align 8
  %9 = call i64 @_ZNSt7__n486116coroutine_handleIN4task12promise_typeEE12from_addressEPv(ptr noundef %8) #4
  %10 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %6, i32 0, i32 0
  %11 = inttoptr i64 %9 to ptr
  store ptr %11, ptr %10, align 8
  %12 = call i64 @_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEEcvNS0_IvEEEv(ptr noundef nonnull align 8 dereferenceable(8) %6) #4
  %13 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %5, i32 0, i32 0
  %14 = inttoptr i64 %12 to ptr
  store ptr %14, ptr %13, align 8
  %15 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %5, i32 0, i32 0
  %16 = load ptr, ptr %15, align 8
  call void @_ZNKSt7__n486113suspend_never13await_suspendENS_16coroutine_handleIvEE(ptr noundef nonnull align 1 dereferenceable(1) %7, ptr %16) #4
  ret void
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZNKSt7__n486113suspend_never13await_suspendENS_16coroutine_handleIvEE(ptr noundef nonnull align 1 dereferenceable(1) %0, ptr %1) #8 comdat {
  %3 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %4 = alloca ptr, align 8
  %5 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %3, i32 0, i32 0
  store ptr %1, ptr %5, align 8
  store ptr %0, ptr %4, align 8
  %6 = load ptr, ptr %4, align 8
  ret void
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local i64 @_ZNSt7__n486116coroutine_handleIN4task12promise_typeEE12from_addressEPv(ptr noundef %0) #8 comdat {
  %2 = alloca %"struct.std::__n4861::coroutine_handle", align 8
  %3 = alloca ptr, align 8
  store ptr %0, ptr %3, align 8
  call void @_ZNSt7__n486116coroutine_handleIN4task12promise_typeEEC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %2) #4
  %4 = load ptr, ptr %3, align 8
  %5 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %2, i32 0, i32 0
  store ptr %4, ptr %5, align 8
  %6 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %2, i32 0, i32 0
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
declare i8 @llvm.coro.suspend(token, i1) #4

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZNKSt7__n486113suspend_never12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %0) #8 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  ret void
}

; Function Attrs: nocallback nofree nosync nounwind willreturn memory(argmem: readwrite)
declare void @llvm.lifetime.end.p0(ptr captures(none)) #7

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local noundef i1 @_ZNKSt7__n486114suspend_always11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %0) #8 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  ret i1 false
}

; Function Attrs: alwaysinline mustprogress
define internal void @_Z7pointerv.__await_suspend_wrapper__await(ptr noundef nonnull %0, ptr noundef %1) #10 {
  %3 = alloca ptr, align 8
  %4 = alloca ptr, align 8
  %5 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %6 = alloca %"struct.std::__n4861::coroutine_handle", align 8
  store ptr %0, ptr %3, align 8
  store ptr %1, ptr %4, align 8
  %7 = load ptr, ptr %3, align 8
  %8 = load ptr, ptr %4, align 8
  %9 = call i64 @_ZNSt7__n486116coroutine_handleIN4task12promise_typeEE12from_addressEPv(ptr noundef %8) #4
  %10 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %6, i32 0, i32 0
  %11 = inttoptr i64 %9 to ptr
  store ptr %11, ptr %10, align 8
  %12 = call i64 @_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEEcvNS0_IvEEEv(ptr noundef nonnull align 8 dereferenceable(8) %6) #4
  %13 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %5, i32 0, i32 0
  %14 = inttoptr i64 %12 to ptr
  store ptr %14, ptr %13, align 8
  %15 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %5, i32 0, i32 0
  %16 = load ptr, ptr %15, align 8
  call void @_ZNKSt7__n486114suspend_always13await_suspendENS_16coroutine_handleIvEE(ptr noundef nonnull align 1 dereferenceable(1) %7, ptr %16) #4
  ret void
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZNKSt7__n486114suspend_always13await_suspendENS_16coroutine_handleIvEE(ptr noundef nonnull align 1 dereferenceable(1) %0, ptr %1) #8 comdat {
  %3 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %4 = alloca ptr, align 8
  %5 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %3, i32 0, i32 0
  store ptr %1, ptr %5, align 8
  store ptr %0, ptr %4, align 8
  %6 = load ptr, ptr %4, align 8
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
define internal void @_Z7pointerv.__await_suspend_wrapper__final(ptr noundef nonnull %0, ptr noundef %1) #10 {
  %3 = alloca ptr, align 8
  %4 = alloca ptr, align 8
  %5 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %6 = alloca %"struct.std::__n4861::coroutine_handle", align 8
  store ptr %0, ptr %3, align 8
  store ptr %1, ptr %4, align 8
  %7 = load ptr, ptr %3, align 8
  %8 = load ptr, ptr %4, align 8
  %9 = call i64 @_ZNSt7__n486116coroutine_handleIN4task12promise_typeEE12from_addressEPv(ptr noundef %8) #4
  %10 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %6, i32 0, i32 0
  %11 = inttoptr i64 %9 to ptr
  store ptr %11, ptr %10, align 8
  %12 = call i64 @_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEEcvNS0_IvEEEv(ptr noundef nonnull align 8 dereferenceable(8) %6) #4
  %13 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %5, i32 0, i32 0
  %14 = inttoptr i64 %12 to ptr
  store ptr %14, ptr %13, align 8
  %15 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %5, i32 0, i32 0
  %16 = load ptr, ptr %15, align 8
  call void @_ZNKSt7__n486114suspend_always13await_suspendENS_16coroutine_handleIvEE(ptr noundef nonnull align 1 dereferenceable(1) %7, ptr %16) #4
  ret void
}

; Function Attrs: nounwind
declare void @llvm.coro.end(ptr, i1, token) #4

; Function Attrs: nounwind memory(none)
declare i1 @llvm.coro.is_in_ramp() #6

; Function Attrs: nobuiltin nounwind
declare void @_ZdlPvm(ptr noundef, i64 noundef) #12

; Function Attrs: nounwind memory(argmem: read)
declare ptr @llvm.coro.free(token, ptr readonly captures(none)) #13

; Function Attrs: mustprogress noinline optnone uwtable
define dso_local void @_ZN8registry4pokeEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #0 {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds nuw %struct.registry, ptr %3, i32 0, i32 0
  %5 = load ptr, ptr %4, align 8
  call void @_ZN1T5helloEv(ptr noundef nonnull align 4 dereferenceable(4) %5)
  ret void
}

; Function Attrs: mustprogress noinline optnone uwtable
define linkonce_odr dso_local void @_ZN1T5helloEv(ptr noundef nonnull align 4 dereferenceable(4) %0) #0 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds nuw %struct.T, ptr %3, i32 0, i32 0
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
  %16 = call token @llvm.coro.id(i32 16, ptr %2, ptr null, ptr null)
  %17 = call i1 @llvm.coro.alloc(token %16)
  br i1 %17, label %18, label %21

18:                                               ; preds = %0
  %19 = call i64 @llvm.coro.size.i64()
  %20 = call noalias noundef nonnull ptr @_Znwm(i64 noundef %19) #15
  br label %21

21:                                               ; preds = %18, %0
  %22 = phi ptr [ null, %0 ], [ %20, %18 ]
  %23 = call ptr @llvm.coro.begin(token %16, ptr %22)
  call void @llvm.lifetime.start.p0(ptr %2) #4
  %24 = invoke i64 @_ZN4task12promise_type17get_return_objectEv(ptr noundef nonnull align 1 dereferenceable(1) %2)
          to label %25 unwind label %34

25:                                               ; preds = %21
  %26 = getelementptr inbounds nuw %struct.task, ptr %1, i32 0, i32 0
  %27 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %26, i32 0, i32 0
  %28 = inttoptr i64 %24 to ptr
  store ptr %28, ptr %27, align 8
  call void @llvm.lifetime.start.p0(ptr %5) #4
  call void @_ZN4task12promise_type15initial_suspendEv(ptr noundef nonnull align 1 dereferenceable(1) %2) #4
  %29 = call noundef i1 @_ZNKSt7__n486113suspend_never11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %5) #4
  br i1 %29, label %38, label %30

30:                                               ; preds = %25
  %31 = call token @llvm.coro.save(ptr null)
  call void @llvm.coro.await.suspend.void(ptr %5, ptr %23, ptr @_Z10registeredv.__await_suspend_wrapper__init) #4
  %32 = call i8 @llvm.coro.suspend(token %31, i1 false)
  switch i8 %32, label %121 [
    i8 0, label %38
    i8 1, label %33
  ]

33:                                               ; preds = %30
  br label %39

34:                                               ; preds = %21
  %35 = landingpad { ptr, i32 }
          cleanup
  %36 = extractvalue { ptr, i32 } %35, 0
  store ptr %36, ptr %3, align 8
  %37 = extractvalue { ptr, i32 } %35, 1
  store i32 %37, ptr %4, align 4
  br label %129

38:                                               ; preds = %30, %25
  call void @_ZNKSt7__n486113suspend_never12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %5) #4
  br label %39

39:                                               ; preds = %38, %33
  %40 = phi i32 [ 0, %38 ], [ 2, %33 ]
  call void @llvm.lifetime.end.p0(ptr %5) #4
  switch i32 %40, label %113 [
    i32 0, label %41
  ]

41:                                               ; preds = %39
  call void @llvm.lifetime.start.p0(ptr %7) #4
  call void @_ZN1TC2Ev(ptr noundef nonnull align 4 dereferenceable(4) %7) #4
  invoke void @_ZN8registry3addEP1T(ptr noundef nonnull align 8 dereferenceable(8) @reg, ptr noundef %7)
          to label %42 unwind label %48

42:                                               ; preds = %41
  call void @llvm.lifetime.start.p0(ptr %8) #4
  %43 = call noundef i1 @_ZNKSt7__n486114suspend_always11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %8) #4
  br i1 %43, label %52, label %44

44:                                               ; preds = %42
  %45 = call token @llvm.coro.save(ptr null)
  call void @llvm.coro.await.suspend.void(ptr %8, ptr %23, ptr @_Z10registeredv.__await_suspend_wrapper__await) #4
  %46 = call i8 @llvm.coro.suspend(token %45, i1 false)
  switch i8 %46, label %121 [
    i8 0, label %52
    i8 1, label %47
  ]

47:                                               ; preds = %44
  br label %53

48:                                               ; preds = %41
  %49 = landingpad { ptr, i32 }
          catch ptr null
  %50 = extractvalue { ptr, i32 } %49, 0
  store ptr %50, ptr %3, align 8
  %51 = extractvalue { ptr, i32 } %49, 1
  store i32 %51, ptr %4, align 4
  br label %83

52:                                               ; preds = %44, %42
  call void @_ZNKSt7__n486114suspend_always12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %8) #4
  br label %53

53:                                               ; preds = %52, %47
  %54 = phi i32 [ 0, %52 ], [ 2, %47 ]
  call void @llvm.lifetime.end.p0(ptr %8) #4
  switch i32 %54, label %75 [
    i32 0, label %55
  ]

55:                                               ; preds = %53
  call void @llvm.lifetime.start.p0(ptr %9) #4
  call void @llvm.lifetime.start.p0(ptr %10) #4
  store ptr %9, ptr %10, align 8
  call void @llvm.lifetime.start.p0(ptr %11) #4
  %56 = load ptr, ptr %10, align 8, !nonnull !6, !align !8
  %57 = getelementptr inbounds [64 x i64], ptr %56, i64 0, i64 0
  store ptr %57, ptr %11, align 8
  call void @llvm.lifetime.start.p0(ptr %12) #4
  %58 = load ptr, ptr %10, align 8, !nonnull !6, !align !8
  %59 = getelementptr inbounds [64 x i64], ptr %58, i64 0, i64 0
  %60 = getelementptr inbounds i64, ptr %59, i64 64
  store ptr %60, ptr %12, align 8
  br label %61

61:                                               ; preds = %69, %55
  %62 = load ptr, ptr %11, align 8
  %63 = load ptr, ptr %12, align 8
  %64 = icmp ne ptr %62, %63
  br i1 %64, label %66, label %65

65:                                               ; preds = %61
  call void @llvm.lifetime.end.p0(ptr %12) #4
  call void @llvm.lifetime.end.p0(ptr %11) #4
  call void @llvm.lifetime.end.p0(ptr %10) #4
  br label %72

66:                                               ; preds = %61
  call void @llvm.lifetime.start.p0(ptr %13) #4
  %67 = load ptr, ptr %11, align 8
  store ptr %67, ptr %13, align 8
  %68 = load ptr, ptr %13, align 8, !nonnull !6, !align !8
  store i64 1515870810, ptr %68, align 8
  call void @llvm.lifetime.end.p0(ptr %13) #4
  br label %69

69:                                               ; preds = %66
  %70 = load ptr, ptr %11, align 8
  %71 = getelementptr inbounds nuw i64, ptr %70, i32 1
  store ptr %71, ptr %11, align 8
  br label %61

72:                                               ; preds = %65
  invoke void @_ZN8registry4pokeEv(ptr noundef nonnull align 8 dereferenceable(8) @reg)
          to label %73 unwind label %79

73:                                               ; preds = %72
  %74 = getelementptr inbounds [64 x i64], ptr %9, i64 0, i64 1
  call void @llvm.lifetime.end.p0(ptr %9) #4
  br label %75

75:                                               ; preds = %73, %53
  %76 = phi i32 [ %54, %53 ], [ 0, %73 ]
  call void @llvm.lifetime.end.p0(ptr %7) #4
  switch i32 %76, label %113 [
    i32 0, label %77
  ]

77:                                               ; preds = %75
  invoke void @_ZN4task12promise_type11return_voidEv(ptr noundef nonnull align 1 dereferenceable(1) %2)
          to label %78 unwind label %84

78:                                               ; preds = %77
  br label %94

79:                                               ; preds = %72
  %80 = landingpad { ptr, i32 }
          catch ptr null
  %81 = extractvalue { ptr, i32 } %80, 0
  store ptr %81, ptr %3, align 8
  %82 = extractvalue { ptr, i32 } %80, 1
  store i32 %82, ptr %4, align 4
  call void @llvm.lifetime.end.p0(ptr %9) #4
  br label %83

83:                                               ; preds = %79, %48
  call void @llvm.lifetime.end.p0(ptr %7) #4
  br label %88

84:                                               ; preds = %77
  %85 = landingpad { ptr, i32 }
          catch ptr null
  %86 = extractvalue { ptr, i32 } %85, 0
  store ptr %86, ptr %3, align 8
  %87 = extractvalue { ptr, i32 } %85, 1
  store i32 %87, ptr %4, align 4
  br label %88

88:                                               ; preds = %84, %83
  %89 = load ptr, ptr %3, align 8
  %90 = call ptr @__cxa_begin_catch(ptr %89) #4
  invoke void @_ZN4task12promise_type19unhandled_exceptionEv(ptr noundef nonnull align 1 dereferenceable(1) %2)
          to label %91 unwind label %100

91:                                               ; preds = %88
  invoke void @__cxa_end_catch()
          to label %92 unwind label %104

92:                                               ; preds = %91
  br label %93

93:                                               ; preds = %92
  br label %94

94:                                               ; preds = %93, %78
  call void @llvm.lifetime.start.p0(ptr %14) #4
  call void @_ZN4task12promise_type13final_suspendEv(ptr noundef nonnull align 1 dereferenceable(1) %2) #4
  %95 = call noundef i1 @_ZNKSt7__n486114suspend_always11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %14) #4
  br i1 %95, label %109, label %96

96:                                               ; preds = %94
  %97 = call token @llvm.coro.save(ptr null)
  call void @llvm.coro.await.suspend.void(ptr %14, ptr %23, ptr @_Z10registeredv.__await_suspend_wrapper__final) #4
  %98 = call i8 @llvm.coro.suspend(token %97, i1 true)
  switch i8 %98, label %121 [
    i8 0, label %109
    i8 1, label %99
  ]

99:                                               ; preds = %96
  br label %110

100:                                              ; preds = %88
  %101 = landingpad { ptr, i32 }
          cleanup
  %102 = extractvalue { ptr, i32 } %101, 0
  store ptr %102, ptr %3, align 8
  %103 = extractvalue { ptr, i32 } %101, 1
  store i32 %103, ptr %4, align 4
  invoke void @__cxa_end_catch()
          to label %108 unwind label %140

104:                                              ; preds = %91
  %105 = landingpad { ptr, i32 }
          cleanup
  %106 = extractvalue { ptr, i32 } %105, 0
  store ptr %106, ptr %3, align 8
  %107 = extractvalue { ptr, i32 } %105, 1
  store i32 %107, ptr %4, align 4
  br label %126

108:                                              ; preds = %100
  br label %126

109:                                              ; preds = %96, %94
  call void @_ZNKSt7__n486114suspend_always12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %14) #4
  br label %110

110:                                              ; preds = %109, %99
  %111 = phi i32 [ 0, %109 ], [ 2, %99 ]
  call void @llvm.lifetime.end.p0(ptr %14) #4
  switch i32 %111, label %113 [
    i32 0, label %112
  ]

112:                                              ; preds = %110
  br label %113

113:                                              ; preds = %112, %110, %75, %39
  %114 = phi i32 [ %40, %39 ], [ %76, %75 ], [ %111, %110 ], [ 0, %112 ]
  call void @llvm.lifetime.end.p0(ptr %2) #4
  %115 = call ptr @llvm.coro.free(token %16, ptr %23)
  %116 = icmp ne ptr %115, null
  br i1 %116, label %117, label %119

117:                                              ; preds = %113
  %118 = call i64 @llvm.coro.size.i64()
  call void @_ZdlPvm(ptr noundef %115, i64 noundef %118) #4
  br label %119

119:                                              ; preds = %113, %117
  switch i32 %114, label %143 [
    i32 0, label %120
    i32 2, label %121
  ]

120:                                              ; preds = %119
  br label %121

121:                                              ; preds = %120, %119, %96, %44, %30
  call void @llvm.coro.end(ptr null, i1 false, token none)
  %122 = getelementptr inbounds nuw %struct.task, ptr %1, i32 0, i32 0
  %123 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %122, i32 0, i32 0
  %124 = load ptr, ptr %123, align 8
  %125 = ptrtoint ptr %124 to i64
  ret i64 %125

126:                                              ; preds = %108, %104
  call void @llvm.coro.end(ptr null, i1 true, token none)
  %127 = call i1 @llvm.coro.is_in_ramp()
  br i1 %127, label %128, label %135

128:                                              ; preds = %126
  br label %129

129:                                              ; preds = %128, %34
  call void @llvm.lifetime.end.p0(ptr %2) #4
  %130 = call ptr @llvm.coro.free(token %16, ptr %23)
  %131 = icmp ne ptr %130, null
  br i1 %131, label %132, label %134

132:                                              ; preds = %129
  %133 = call i64 @llvm.coro.size.i64()
  call void @_ZdlPvm(ptr noundef %130, i64 noundef %133) #4
  br label %134

134:                                              ; preds = %129, %132
  br label %135

135:                                              ; preds = %134, %126
  %136 = load ptr, ptr %3, align 8
  %137 = load i32, ptr %4, align 4
  %138 = insertvalue { ptr, i32 } poison, ptr %136, 0
  %139 = insertvalue { ptr, i32 } %138, i32 %137, 1
  resume { ptr, i32 } %139

140:                                              ; preds = %100
  %141 = landingpad { ptr, i32 }
          catch ptr null
  %142 = extractvalue { ptr, i32 } %141, 0
  call void @__clang_call_terminate(ptr %142) #16
  unreachable

143:                                              ; preds = %119
  unreachable
}

; Function Attrs: alwaysinline mustprogress
define internal void @_Z10registeredv.__await_suspend_wrapper__init(ptr noundef nonnull %0, ptr noundef %1) #10 {
  %3 = alloca ptr, align 8
  %4 = alloca ptr, align 8
  %5 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %6 = alloca %"struct.std::__n4861::coroutine_handle", align 8
  store ptr %0, ptr %3, align 8
  store ptr %1, ptr %4, align 8
  %7 = load ptr, ptr %3, align 8
  %8 = load ptr, ptr %4, align 8
  %9 = call i64 @_ZNSt7__n486116coroutine_handleIN4task12promise_typeEE12from_addressEPv(ptr noundef %8) #4
  %10 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %6, i32 0, i32 0
  %11 = inttoptr i64 %9 to ptr
  store ptr %11, ptr %10, align 8
  %12 = call i64 @_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEEcvNS0_IvEEEv(ptr noundef nonnull align 8 dereferenceable(8) %6) #4
  %13 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %5, i32 0, i32 0
  %14 = inttoptr i64 %12 to ptr
  store ptr %14, ptr %13, align 8
  %15 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %5, i32 0, i32 0
  %16 = load ptr, ptr %15, align 8
  call void @_ZNKSt7__n486113suspend_never13await_suspendENS_16coroutine_handleIvEE(ptr noundef nonnull align 1 dereferenceable(1) %7, ptr %16) #4
  ret void
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZN1TC2Ev(ptr noundef nonnull align 4 dereferenceable(4) %0) unnamed_addr #8 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds nuw %struct.T, ptr %3, i32 0, i32 0
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
  %7 = getelementptr inbounds nuw %struct.registry, ptr %5, i32 0, i32 0
  store ptr %6, ptr %7, align 8
  ret void
}

; Function Attrs: alwaysinline mustprogress
define internal void @_Z10registeredv.__await_suspend_wrapper__await(ptr noundef nonnull %0, ptr noundef %1) #10 {
  %3 = alloca ptr, align 8
  %4 = alloca ptr, align 8
  %5 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %6 = alloca %"struct.std::__n4861::coroutine_handle", align 8
  store ptr %0, ptr %3, align 8
  store ptr %1, ptr %4, align 8
  %7 = load ptr, ptr %3, align 8
  %8 = load ptr, ptr %4, align 8
  %9 = call i64 @_ZNSt7__n486116coroutine_handleIN4task12promise_typeEE12from_addressEPv(ptr noundef %8) #4
  %10 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %6, i32 0, i32 0
  %11 = inttoptr i64 %9 to ptr
  store ptr %11, ptr %10, align 8
  %12 = call i64 @_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEEcvNS0_IvEEEv(ptr noundef nonnull align 8 dereferenceable(8) %6) #4
  %13 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %5, i32 0, i32 0
  %14 = inttoptr i64 %12 to ptr
  store ptr %14, ptr %13, align 8
  %15 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %5, i32 0, i32 0
  %16 = load ptr, ptr %15, align 8
  call void @_ZNKSt7__n486114suspend_always13await_suspendENS_16coroutine_handleIvEE(ptr noundef nonnull align 1 dereferenceable(1) %7, ptr %16) #4
  ret void
}

; Function Attrs: alwaysinline mustprogress
define internal void @_Z10registeredv.__await_suspend_wrapper__final(ptr noundef nonnull %0, ptr noundef %1) #10 {
  %3 = alloca ptr, align 8
  %4 = alloca ptr, align 8
  %5 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %6 = alloca %"struct.std::__n4861::coroutine_handle", align 8
  store ptr %0, ptr %3, align 8
  store ptr %1, ptr %4, align 8
  %7 = load ptr, ptr %3, align 8
  %8 = load ptr, ptr %4, align 8
  %9 = call i64 @_ZNSt7__n486116coroutine_handleIN4task12promise_typeEE12from_addressEPv(ptr noundef %8) #4
  %10 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %6, i32 0, i32 0
  %11 = inttoptr i64 %9 to ptr
  store ptr %11, ptr %10, align 8
  %12 = call i64 @_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEEcvNS0_IvEEEv(ptr noundef nonnull align 8 dereferenceable(8) %6) #4
  %13 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %5, i32 0, i32 0
  %14 = inttoptr i64 %12 to ptr
  store ptr %14, ptr %13, align 8
  %15 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %5, i32 0, i32 0
  %16 = load ptr, ptr %15, align 8
  call void @_ZNKSt7__n486114suspend_always13await_suspendENS_16coroutine_handleIvEE(ptr noundef nonnull align 1 dereferenceable(1) %7, ptr %16) #4
  ret void
}

; Function Attrs: mustprogress noinline norecurse optnone uwtable
define dso_local noundef i32 @main() #14 {
  %1 = alloca i32, align 4
  %2 = alloca %struct.task, align 8
  %3 = alloca [256 x i64], align 8
  %4 = alloca ptr, align 8
  %5 = alloca ptr, align 8
  %6 = alloca ptr, align 8
  %7 = alloca ptr, align 8
  %8 = alloca %struct.task, align 8
  %9 = alloca ptr, align 8
  %10 = alloca ptr, align 8
  %11 = alloca ptr, align 8
  %12 = alloca ptr, align 8
  store i32 0, ptr %1, align 4
  %13 = call i64 @_Z7pointerv()
  %14 = getelementptr inbounds nuw %struct.task, ptr %2, i32 0, i32 0
  %15 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %14, i32 0, i32 0
  %16 = inttoptr i64 %13 to ptr
  store ptr %16, ptr %15, align 8
  store ptr %3, ptr %4, align 8
  %17 = load ptr, ptr %4, align 8, !nonnull !6, !align !8
  %18 = getelementptr inbounds [256 x i64], ptr %17, i64 0, i64 0
  store ptr %18, ptr %5, align 8
  %19 = load ptr, ptr %4, align 8, !nonnull !6, !align !8
  %20 = getelementptr inbounds [256 x i64], ptr %19, i64 0, i64 0
  %21 = getelementptr inbounds i64, ptr %20, i64 256
  store ptr %21, ptr %6, align 8
  br label %22

22:                                               ; preds = %29, %0
  %23 = load ptr, ptr %5, align 8
  %24 = load ptr, ptr %6, align 8
  %25 = icmp ne ptr %23, %24
  br i1 %25, label %26, label %32

26:                                               ; preds = %22
  %27 = load ptr, ptr %5, align 8
  store ptr %27, ptr %7, align 8
  %28 = load ptr, ptr %7, align 8, !nonnull !6, !align !8
  store i64 6510615555426900570, ptr %28, align 8
  br label %29

29:                                               ; preds = %26
  %30 = load ptr, ptr %5, align 8
  %31 = getelementptr inbounds nuw i64, ptr %30, i32 1
  store ptr %31, ptr %5, align 8
  br label %22

32:                                               ; preds = %22
  %33 = getelementptr inbounds nuw %struct.task, ptr %2, i32 0, i32 0
  call void @_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEE6resumeEv(ptr noundef nonnull align 8 dereferenceable(8) %33)
  %34 = call i64 @_Z10registeredv()
  %35 = getelementptr inbounds nuw %struct.task, ptr %8, i32 0, i32 0
  %36 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %35, i32 0, i32 0
  %37 = inttoptr i64 %34 to ptr
  store ptr %37, ptr %36, align 8
  store ptr %3, ptr %9, align 8
  %38 = load ptr, ptr %9, align 8, !nonnull !6, !align !8
  %39 = getelementptr inbounds [256 x i64], ptr %38, i64 0, i64 0
  store ptr %39, ptr %10, align 8
  %40 = load ptr, ptr %9, align 8, !nonnull !6, !align !8
  %41 = getelementptr inbounds [256 x i64], ptr %40, i64 0, i64 0
  %42 = getelementptr inbounds i64, ptr %41, i64 256
  store ptr %42, ptr %11, align 8
  br label %43

43:                                               ; preds = %50, %32
  %44 = load ptr, ptr %10, align 8
  %45 = load ptr, ptr %11, align 8
  %46 = icmp ne ptr %44, %45
  br i1 %46, label %47, label %53

47:                                               ; preds = %43
  %48 = load ptr, ptr %10, align 8
  store ptr %48, ptr %12, align 8
  %49 = load ptr, ptr %12, align 8, !nonnull !6, !align !8
  store i64 6510615555426900570, ptr %49, align 8
  br label %50

50:                                               ; preds = %47
  %51 = load ptr, ptr %10, align 8
  %52 = getelementptr inbounds nuw i64, ptr %51, i32 1
  store ptr %52, ptr %10, align 8
  br label %43

53:                                               ; preds = %43
  %54 = getelementptr inbounds nuw %struct.task, ptr %8, i32 0, i32 0
  call void @_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEE6resumeEv(ptr noundef nonnull align 8 dereferenceable(8) %54)
  %55 = getelementptr inbounds nuw %struct.task, ptr %2, i32 0, i32 0
  call void @_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEE7destroyEv(ptr noundef nonnull align 8 dereferenceable(8) %55)
  %56 = getelementptr inbounds nuw %struct.task, ptr %8, i32 0, i32 0
  call void @_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEE7destroyEv(ptr noundef nonnull align 8 dereferenceable(8) %56)
  %57 = getelementptr inbounds [256 x i64], ptr %3, i64 0, i64 3
  %58 = load i64, ptr %57, align 8
  %59 = and i64 %58, 0
  %60 = trunc i64 %59 to i32
  ret i32 %60
}

; Function Attrs: mustprogress noinline optnone uwtable
define linkonce_odr dso_local void @_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEE6resumeEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #0 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %3, i32 0, i32 0
  %5 = load ptr, ptr %4, align 8
  call void @llvm.coro.resume(ptr %5)
  ret void
}

; Function Attrs: mustprogress noinline optnone uwtable
define linkonce_odr dso_local void @_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEE7destroyEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #0 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %3, i32 0, i32 0
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
define linkonce_odr dso_local void @_ZNSt7__n486116coroutine_handleIN4task12promise_typeEEC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %0) unnamed_addr #8 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %3, i32 0, i32 0
  store ptr null, ptr %4, align 8
  ret void
}

; Function Attrs: nounwind memory(none)
declare ptr @llvm.coro.promise(ptr captures(none), i32, i1) #6

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local i64 @_ZNSt7__n486116coroutine_handleIvE12from_addressEPv(ptr noundef %0) #8 comdat {
  %2 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %3 = alloca ptr, align 8
  store ptr %0, ptr %3, align 8
  call void @_ZNSt7__n486116coroutine_handleIvEC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %2) #4
  %4 = load ptr, ptr %3, align 8
  %5 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %2, i32 0, i32 0
  store ptr %4, ptr %5, align 8
  %6 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %2, i32 0, i32 0
  %7 = load ptr, ptr %6, align 8
  %8 = ptrtoint ptr %7 to i64
  ret i64 %8
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local noundef ptr @_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEE7addressEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #8 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %3, i32 0, i32 0
  %5 = load ptr, ptr %4, align 8
  ret ptr %5
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZNSt7__n486116coroutine_handleIvEC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %0) unnamed_addr #8 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %3, i32 0, i32 0
  store ptr null, ptr %4, align 8
  ret void
}

declare void @llvm.coro.resume(ptr)

declare void @llvm.coro.destroy(ptr)

attributes #0 = { mustprogress noinline optnone uwtable "frame-pointer"="non-leaf-no-reserve" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #1 = { "frame-pointer"="non-leaf-no-reserve" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #2 = { mustprogress noinline optnone presplitcoroutine uwtable "frame-pointer"="non-leaf-no-reserve" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #3 = { nocallback nofree nosync nounwind willreturn memory(argmem: read) }
attributes #4 = { nounwind }
attributes #5 = { nobuiltin allocsize(0) "frame-pointer"="non-leaf-no-reserve" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #6 = { nounwind memory(none) }
attributes #7 = { nocallback nofree nosync nounwind willreturn memory(argmem: readwrite) }
attributes #8 = { mustprogress noinline nounwind optnone uwtable "frame-pointer"="non-leaf-no-reserve" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #9 = { nomerge nounwind }
attributes #10 = { alwaysinline mustprogress "sample-profile-suffix-elision-policy"="selected" }
attributes #11 = { noinline noreturn nounwind uwtable "frame-pointer"="non-leaf-no-reserve" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #12 = { nobuiltin nounwind "frame-pointer"="non-leaf-no-reserve" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #13 = { nounwind memory(argmem: read) }
attributes #14 = { mustprogress noinline norecurse optnone uwtable "frame-pointer"="non-leaf-no-reserve" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #15 = { allocsize(0) }
attributes #16 = { noreturn nounwind }

!llvm.module.flags = !{!0, !1, !2, !3, !4}
!llvm.ident = !{!5}

!0 = !{i32 1, !"wchar_size", i32 4}
!1 = !{i32 8, !"PIC Level", i32 2}
!2 = !{i32 7, !"PIE Level", i32 2}
!3 = !{i32 7, !"uwtable", i32 2}
!4 = !{i32 7, !"frame-pointer", i32 4}
!5 = !{!"Debian clang version 22.1.8 (1~deb13u4)"}
!6 = !{}
!7 = !{i64 4}
!8 = !{i64 8}
