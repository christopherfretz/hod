; clang++-22 -std=c++20 -O0 -S -emit-llvm -Xclang -disable-llvm-passes
; (aarch64-unknown-linux-gnu; the target triple and datalayout removed) of:
;
; // An exception thrown in a coroutine body, rethrown by
; // unhandled_exception, escapes through resume() into the caller; the
; // locals of the body are destroyed on the way, and the coroutine is done.
; #include <coroutine>
; #include <cstdio>
; #include <stdexcept>
; static int dtors = 0;
; struct Dtor { int v; ~Dtor() { ++dtors; std::printf("dtor %d\n", v); } };
; struct task {
;   struct promise_type {
;     task get_return_object() { return task{std::coroutine_handle<promise_type>::from_promise(*this)}; }
;     std::suspend_always initial_suspend() noexcept { return {}; }
;     std::suspend_always final_suspend() noexcept { return {}; }
;     void return_void() {}
;     void unhandled_exception() { throw; }
;   };
;   std::coroutine_handle<promise_type> h;
; };
; struct later {
;   bool await_ready() { return false; }
;   void await_suspend(std::coroutine_handle<>) {}
;   void await_resume() {}
; };
; task body(int n) {
;   Dtor a{n};
;   co_await later{};
;   Dtor b{n + 1};
;   if (n > 0) throw std::runtime_error("boom");
;   co_await later{};
; }
; int main() {
;   task t = body(1);
;   t.h.resume();
;   std::printf("suspended, done=%d dtors=%d\n", (int)t.h.done(), dtors);
;   try {
;     t.h.resume();
;   } catch (std::exception &e) {
;     std::printf("caught %s done=%d dtors=%d\n", e.what(), (int)t.h.done(), dtors);
;   }
;   t.h.destroy();
;   std::printf("destroyed dtors=%d\n", dtors);
;   task u = body(0);
;   u.h.resume();
;   u.h.resume();
;   std::printf("finished done=%d dtors=%d\n", (int)u.h.done(), dtors);
;   u.h.destroy();
; }
;
;@ covers: spec/ir.md §5.5 intrinsics (coroutine bodies; spec/codegen.md §7.6)
;@ input: pre-opt
;@ dialect: 22
;@ link: c++
;@ unsupported(x86_64): *
;@ stdout: suspended, done=0 dtors=0
;@ stdout: dtor 2
;@ stdout: dtor 1
;@ stdout: caught boom done=1 dtors=2
;@ stdout: destroyed dtors=2
;@ stdout: finished done=0 dtors=2
;@ stdout: dtor 1
;@ stdout: dtor 0

source_filename = "exceptions.cpp"

%struct.task = type { %"struct.std::__n4861::coroutine_handle" }
%"struct.std::__n4861::coroutine_handle" = type { ptr }
%"struct.task::promise_type" = type { i8 }
%"struct.std::__n4861::suspend_always" = type { i8 }
%struct.Dtor = type { i32 }
%struct.later = type { i8 }
%"struct.std::__n4861::coroutine_handle.0" = type { ptr }

$_ZN4task12promise_type17get_return_objectEv = comdat any

$_ZN4task12promise_type15initial_suspendEv = comdat any

$_ZNKSt7__n486114suspend_always11await_readyEv = comdat any

$_ZNKSt7__n486114suspend_always13await_suspendENS_16coroutine_handleIvEE = comdat any

$_ZNSt7__n486116coroutine_handleIN4task12promise_typeEE12from_addressEPv = comdat any

$_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEEcvNS0_IvEEEv = comdat any

$_ZNKSt7__n486114suspend_always12await_resumeEv = comdat any

$_ZN5later11await_readyEv = comdat any

$_ZN5later13await_suspendENSt7__n486116coroutine_handleIvEE = comdat any

$_ZN5later12await_resumeEv = comdat any

$_ZN4DtorD2Ev = comdat any

$_ZN4task12promise_type11return_voidEv = comdat any

$_ZN4task12promise_type19unhandled_exceptionEv = comdat any

$__clang_call_terminate = comdat any

$_ZN4task12promise_type13final_suspendEv = comdat any

$_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEE6resumeEv = comdat any

$_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEE4doneEv = comdat any

$_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEE7destroyEv = comdat any

$_ZNSt7__n486116coroutine_handleIN4task12promise_typeEE12from_promiseERS2_ = comdat any

$_ZNSt7__n486116coroutine_handleIN4task12promise_typeEEC2Ev = comdat any

$_ZNSt7__n486116coroutine_handleIvE12from_addressEPv = comdat any

$_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEE7addressEv = comdat any

$_ZNSt7__n486116coroutine_handleIvEC2Ev = comdat any

@.str = private unnamed_addr constant [5 x i8] c"boom\00", align 1
@_ZTISt13runtime_error = external constant ptr
@.str.1 = private unnamed_addr constant [29 x i8] c"suspended, done=%d dtors=%d\0A\00", align 1
@_ZL5dtors = internal global i32 0, align 4
@_ZTISt9exception = external constant ptr
@.str.2 = private unnamed_addr constant [28 x i8] c"caught %s done=%d dtors=%d\0A\00", align 1
@.str.3 = private unnamed_addr constant [20 x i8] c"destroyed dtors=%d\0A\00", align 1
@.str.4 = private unnamed_addr constant [27 x i8] c"finished done=%d dtors=%d\0A\00", align 1
@.str.5 = private unnamed_addr constant [9 x i8] c"dtor %d\0A\00", align 1

; Function Attrs: mustprogress noinline optnone presplitcoroutine uwtable
define dso_local i64 @_Z4bodyi(i32 noundef %0) #0 personality ptr @__gxx_personality_v0 {
  %2 = alloca %struct.task, align 8
  %3 = alloca i32, align 4, !coro.outside.frame !6
  %4 = alloca i32, align 4
  %5 = alloca %"struct.task::promise_type", align 1
  %6 = alloca ptr, align 8
  %7 = alloca i32, align 4
  %8 = alloca %"struct.std::__n4861::suspend_always", align 1
  %9 = alloca %"struct.std::__n4861::suspend_always", align 1
  %10 = alloca %struct.Dtor, align 4
  %11 = alloca %struct.later, align 1
  %12 = alloca %struct.Dtor, align 4
  %13 = alloca %struct.later, align 1
  %14 = alloca %"struct.std::__n4861::suspend_always", align 1
  %15 = alloca %"struct.std::__n4861::suspend_always", align 1
  store i32 %0, ptr %3, align 4
  %16 = call token @llvm.coro.id(i32 16, ptr %5, ptr null, ptr null)
  %17 = call i1 @llvm.coro.alloc(token %16)
  br i1 %17, label %18, label %21

18:                                               ; preds = %1
  %19 = call i64 @llvm.coro.size.i64()
  %20 = call noalias noundef nonnull ptr @_Znwm(i64 noundef %19) #17
  br label %21

21:                                               ; preds = %18, %1
  %22 = phi ptr [ null, %1 ], [ %20, %18 ]
  %23 = call ptr @llvm.coro.begin(token %16, ptr %22)
  call void @llvm.lifetime.start.p0(ptr %4) #2
  %24 = load i32, ptr %3, align 4
  store i32 %24, ptr %4, align 4
  call void @llvm.lifetime.start.p0(ptr %5) #2
  %25 = invoke i64 @_ZN4task12promise_type17get_return_objectEv(ptr noundef nonnull align 1 dereferenceable(1) %5)
          to label %26 unwind label %35

26:                                               ; preds = %21
  %27 = getelementptr inbounds nuw %struct.task, ptr %2, i32 0, i32 0
  %28 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %27, i32 0, i32 0
  %29 = inttoptr i64 %25 to ptr
  store ptr %29, ptr %28, align 8
  call void @llvm.lifetime.start.p0(ptr %8) #2
  call void @_ZN4task12promise_type15initial_suspendEv(ptr noundef nonnull align 1 dereferenceable(1) %5) #2
  %30 = call noundef i1 @_ZNKSt7__n486114suspend_always11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %8) #2
  br i1 %30, label %39, label %31

31:                                               ; preds = %26
  %32 = call token @llvm.coro.save(ptr null)
  call void @llvm.coro.await.suspend.void(ptr %8, ptr %23, ptr @_Z4bodyi.__await_suspend_wrapper__init) #2
  %33 = call i8 @llvm.coro.suspend(token %32, i1 false)
  switch i8 %33, label %139 [
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
  br label %147

39:                                               ; preds = %31, %26
  call void @_ZNKSt7__n486114suspend_always12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %8) #2
  br label %40

40:                                               ; preds = %39, %34
  %41 = phi i32 [ 0, %39 ], [ 2, %34 ]
  call void @llvm.lifetime.end.p0(ptr %8) #2
  switch i32 %41, label %131 [
    i32 0, label %42
  ]

42:                                               ; preds = %40
  call void @llvm.lifetime.start.p0(ptr %10) #2
  %43 = getelementptr inbounds nuw %struct.Dtor, ptr %10, i32 0, i32 0
  %44 = load i32, ptr %4, align 4
  store i32 %44, ptr %43, align 4
  call void @llvm.lifetime.start.p0(ptr %11) #2
  %45 = invoke noundef i1 @_ZN5later11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %11)
          to label %46 unwind label %52

46:                                               ; preds = %42
  br i1 %45, label %56, label %47

47:                                               ; preds = %46
  %48 = call token @llvm.coro.save(ptr null)
  invoke void @llvm.coro.await.suspend.void(ptr %11, ptr %23, ptr @_Z4bodyi.__await_suspend_wrapper__await)
          to label %49 unwind label %52

49:                                               ; preds = %47
  %50 = call i8 @llvm.coro.suspend(token %48, i1 false)
  switch i8 %50, label %139 [
    i8 0, label %56
    i8 1, label %51
  ]

51:                                               ; preds = %49
  br label %58

52:                                               ; preds = %56, %47, %42
  %53 = landingpad { ptr, i32 }
          catch ptr null
  %54 = extractvalue { ptr, i32 } %53, 0
  store ptr %54, ptr %6, align 8
  %55 = extractvalue { ptr, i32 } %53, 1
  store i32 %55, ptr %7, align 4
  call void @llvm.lifetime.end.p0(ptr %11) #2
  br label %101

56:                                               ; preds = %49, %46
  invoke void @_ZN5later12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %11)
          to label %57 unwind label %52

57:                                               ; preds = %56
  br label %58

58:                                               ; preds = %57, %51
  %59 = phi i32 [ 0, %57 ], [ 2, %51 ]
  call void @llvm.lifetime.end.p0(ptr %11) #2
  switch i32 %59, label %96 [
    i32 0, label %60
  ]

60:                                               ; preds = %58
  call void @llvm.lifetime.start.p0(ptr %12) #2
  %61 = getelementptr inbounds nuw %struct.Dtor, ptr %12, i32 0, i32 0
  %62 = load i32, ptr %4, align 4
  %63 = add nsw i32 %62, 1
  store i32 %63, ptr %61, align 4
  %64 = load i32, ptr %4, align 4
  %65 = icmp sgt i32 %64, 0
  br i1 %65, label %66, label %77

66:                                               ; preds = %60
  %67 = call ptr @__cxa_allocate_exception(i64 16) #2
  invoke void @_ZNSt13runtime_errorC1EPKc(ptr noundef nonnull align 8 dereferenceable(16) %67, ptr noundef @.str)
          to label %68 unwind label %69

68:                                               ; preds = %66
  invoke void @__cxa_throw(ptr %67, ptr @_ZTISt13runtime_error, ptr @_ZNSt13runtime_errorD1Ev) #18
          to label %161 unwind label %73

69:                                               ; preds = %66
  %70 = landingpad { ptr, i32 }
          catch ptr null
  %71 = extractvalue { ptr, i32 } %70, 0
  store ptr %71, ptr %6, align 8
  %72 = extractvalue { ptr, i32 } %70, 1
  store i32 %72, ptr %7, align 4
  call void @__cxa_free_exception(ptr %67) #2
  br label %100

73:                                               ; preds = %68
  %74 = landingpad { ptr, i32 }
          catch ptr null
  %75 = extractvalue { ptr, i32 } %74, 0
  store ptr %75, ptr %6, align 8
  %76 = extractvalue { ptr, i32 } %74, 1
  store i32 %76, ptr %7, align 4
  br label %100

77:                                               ; preds = %60
  call void @llvm.lifetime.start.p0(ptr %13) #2
  %78 = invoke noundef i1 @_ZN5later11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %13)
          to label %79 unwind label %85

79:                                               ; preds = %77
  br i1 %78, label %89, label %80

80:                                               ; preds = %79
  %81 = call token @llvm.coro.save(ptr null)
  invoke void @llvm.coro.await.suspend.void(ptr %13, ptr %23, ptr @_Z4bodyi.__await_suspend_wrapper__await2)
          to label %82 unwind label %85

82:                                               ; preds = %80
  %83 = call i8 @llvm.coro.suspend(token %81, i1 false)
  switch i8 %83, label %139 [
    i8 0, label %89
    i8 1, label %84
  ]

84:                                               ; preds = %82
  br label %91

85:                                               ; preds = %89, %80, %77
  %86 = landingpad { ptr, i32 }
          catch ptr null
  %87 = extractvalue { ptr, i32 } %86, 0
  store ptr %87, ptr %6, align 8
  %88 = extractvalue { ptr, i32 } %86, 1
  store i32 %88, ptr %7, align 4
  call void @llvm.lifetime.end.p0(ptr %13) #2
  br label %100

89:                                               ; preds = %82, %79
  invoke void @_ZN5later12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %13)
          to label %90 unwind label %85

90:                                               ; preds = %89
  br label %91

91:                                               ; preds = %90, %84
  %92 = phi i32 [ 0, %90 ], [ 2, %84 ]
  call void @llvm.lifetime.end.p0(ptr %13) #2
  switch i32 %92, label %94 [
    i32 0, label %93
  ]

93:                                               ; preds = %91
  br label %94

94:                                               ; preds = %93, %91
  %95 = phi i32 [ %92, %91 ], [ 0, %93 ]
  call void @_ZN4DtorD2Ev(ptr noundef nonnull align 4 dereferenceable(4) %12) #2
  call void @llvm.lifetime.end.p0(ptr %12) #2
  br label %96

96:                                               ; preds = %94, %58
  %97 = phi i32 [ %59, %58 ], [ %95, %94 ]
  call void @_ZN4DtorD2Ev(ptr noundef nonnull align 4 dereferenceable(4) %10) #2
  call void @llvm.lifetime.end.p0(ptr %10) #2
  switch i32 %97, label %131 [
    i32 0, label %98
  ]

98:                                               ; preds = %96
  invoke void @_ZN4task12promise_type11return_voidEv(ptr noundef nonnull align 1 dereferenceable(1) %5)
          to label %99 unwind label %102

99:                                               ; preds = %98
  br label %112

100:                                              ; preds = %85, %73, %69
  call void @_ZN4DtorD2Ev(ptr noundef nonnull align 4 dereferenceable(4) %12) #2
  call void @llvm.lifetime.end.p0(ptr %12) #2
  br label %101

101:                                              ; preds = %100, %52
  call void @_ZN4DtorD2Ev(ptr noundef nonnull align 4 dereferenceable(4) %10) #2
  call void @llvm.lifetime.end.p0(ptr %10) #2
  br label %106

102:                                              ; preds = %98
  %103 = landingpad { ptr, i32 }
          catch ptr null
  %104 = extractvalue { ptr, i32 } %103, 0
  store ptr %104, ptr %6, align 8
  %105 = extractvalue { ptr, i32 } %103, 1
  store i32 %105, ptr %7, align 4
  br label %106

106:                                              ; preds = %102, %101
  %107 = load ptr, ptr %6, align 8
  %108 = call ptr @__cxa_begin_catch(ptr %107) #2
  invoke void @_ZN4task12promise_type19unhandled_exceptionEv(ptr noundef nonnull align 1 dereferenceable(1) %5)
          to label %109 unwind label %118

109:                                              ; preds = %106
  invoke void @__cxa_end_catch()
          to label %110 unwind label %122

110:                                              ; preds = %109
  br label %111

111:                                              ; preds = %110
  br label %112

112:                                              ; preds = %111, %99
  call void @llvm.lifetime.start.p0(ptr %14) #2
  call void @_ZN4task12promise_type13final_suspendEv(ptr noundef nonnull align 1 dereferenceable(1) %5) #2
  %113 = call noundef i1 @_ZNKSt7__n486114suspend_always11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %14) #2
  br i1 %113, label %127, label %114

114:                                              ; preds = %112
  %115 = call token @llvm.coro.save(ptr null)
  call void @llvm.coro.await.suspend.void(ptr %14, ptr %23, ptr @_Z4bodyi.__await_suspend_wrapper__final) #2
  %116 = call i8 @llvm.coro.suspend(token %115, i1 true)
  switch i8 %116, label %139 [
    i8 0, label %127
    i8 1, label %117
  ]

117:                                              ; preds = %114
  br label %128

118:                                              ; preds = %106
  %119 = landingpad { ptr, i32 }
          cleanup
  %120 = extractvalue { ptr, i32 } %119, 0
  store ptr %120, ptr %6, align 8
  %121 = extractvalue { ptr, i32 } %119, 1
  store i32 %121, ptr %7, align 4
  invoke void @__cxa_end_catch()
          to label %126 unwind label %158

122:                                              ; preds = %109
  %123 = landingpad { ptr, i32 }
          cleanup
  %124 = extractvalue { ptr, i32 } %123, 0
  store ptr %124, ptr %6, align 8
  %125 = extractvalue { ptr, i32 } %123, 1
  store i32 %125, ptr %7, align 4
  br label %144

126:                                              ; preds = %118
  br label %144

127:                                              ; preds = %114, %112
  call void @_ZNKSt7__n486114suspend_always12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %14) #2
  br label %128

128:                                              ; preds = %127, %117
  %129 = phi i32 [ 0, %127 ], [ 2, %117 ]
  call void @llvm.lifetime.end.p0(ptr %14) #2
  switch i32 %129, label %131 [
    i32 0, label %130
  ]

130:                                              ; preds = %128
  br label %131

131:                                              ; preds = %130, %128, %96, %40
  %132 = phi i32 [ %41, %40 ], [ %97, %96 ], [ %129, %128 ], [ 0, %130 ]
  call void @llvm.lifetime.end.p0(ptr %5) #2
  call void @llvm.lifetime.end.p0(ptr %4) #2
  %133 = call ptr @llvm.coro.free(token %16, ptr %23)
  %134 = icmp ne ptr %133, null
  br i1 %134, label %135, label %137

135:                                              ; preds = %131
  %136 = call i64 @llvm.coro.size.i64()
  call void @_ZdlPvm(ptr noundef %133, i64 noundef %136) #2
  br label %137

137:                                              ; preds = %131, %135
  switch i32 %132, label %161 [
    i32 0, label %138
    i32 2, label %139
  ]

138:                                              ; preds = %137
  br label %139

139:                                              ; preds = %138, %137, %114, %82, %49, %31
  call void @llvm.coro.end(ptr null, i1 false, token none)
  %140 = getelementptr inbounds nuw %struct.task, ptr %2, i32 0, i32 0
  %141 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %140, i32 0, i32 0
  %142 = load ptr, ptr %141, align 8
  %143 = ptrtoint ptr %142 to i64
  ret i64 %143

144:                                              ; preds = %126, %122
  call void @llvm.coro.end(ptr null, i1 true, token none)
  %145 = call i1 @llvm.coro.is_in_ramp()
  br i1 %145, label %146, label %153

146:                                              ; preds = %144
  br label %147

147:                                              ; preds = %146, %35
  call void @llvm.lifetime.end.p0(ptr %5) #2
  call void @llvm.lifetime.end.p0(ptr %4) #2
  %148 = call ptr @llvm.coro.free(token %16, ptr %23)
  %149 = icmp ne ptr %148, null
  br i1 %149, label %150, label %152

150:                                              ; preds = %147
  %151 = call i64 @llvm.coro.size.i64()
  call void @_ZdlPvm(ptr noundef %148, i64 noundef %151) #2
  br label %152

152:                                              ; preds = %147, %150
  br label %153

153:                                              ; preds = %152, %144
  %154 = load ptr, ptr %6, align 8
  %155 = load i32, ptr %7, align 4
  %156 = insertvalue { ptr, i32 } poison, ptr %154, 0
  %157 = insertvalue { ptr, i32 } %156, i32 %155, 1
  resume { ptr, i32 } %157

158:                                              ; preds = %118
  %159 = landingpad { ptr, i32 }
          catch ptr null
  %160 = extractvalue { ptr, i32 } %159, 0
  call void @__clang_call_terminate(ptr %160) #19
  unreachable

161:                                              ; preds = %137, %68
  unreachable
}

; Function Attrs: nocallback nofree nosync nounwind willreturn memory(argmem: read)
declare token @llvm.coro.id(i32, ptr readnone, ptr readonly captures(none), ptr) #1

; Function Attrs: nounwind
declare i1 @llvm.coro.alloc(token) #2

; Function Attrs: nobuiltin allocsize(0)
declare noundef nonnull ptr @_Znwm(i64 noundef) #3

; Function Attrs: nounwind memory(none)
declare i64 @llvm.coro.size.i64() #4

; Function Attrs: nounwind
declare ptr @llvm.coro.begin(token, ptr writeonly) #2

; Function Attrs: nocallback nofree nosync nounwind willreturn memory(argmem: readwrite)
declare void @llvm.lifetime.start.p0(ptr captures(none)) #5

; Function Attrs: mustprogress noinline optnone uwtable
define linkonce_odr dso_local i64 @_ZN4task12promise_type17get_return_objectEv(ptr noundef nonnull align 1 dereferenceable(1) %0) #6 comdat {
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
define linkonce_odr dso_local void @_ZN4task12promise_type15initial_suspendEv(ptr noundef nonnull align 1 dereferenceable(1) %0) #7 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  ret void
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local noundef i1 @_ZNKSt7__n486114suspend_always11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %0) #7 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  ret i1 false
}

; Function Attrs: nomerge nounwind
declare token @llvm.coro.save(ptr) #8

; Function Attrs: alwaysinline mustprogress
define internal void @_Z4bodyi.__await_suspend_wrapper__init(ptr noundef nonnull %0, ptr noundef %1) #9 {
  %3 = alloca ptr, align 8
  %4 = alloca ptr, align 8
  %5 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %6 = alloca %"struct.std::__n4861::coroutine_handle", align 8
  store ptr %0, ptr %3, align 8
  store ptr %1, ptr %4, align 8
  %7 = load ptr, ptr %3, align 8
  %8 = load ptr, ptr %4, align 8
  %9 = call i64 @_ZNSt7__n486116coroutine_handleIN4task12promise_typeEE12from_addressEPv(ptr noundef %8) #2
  %10 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %6, i32 0, i32 0
  %11 = inttoptr i64 %9 to ptr
  store ptr %11, ptr %10, align 8
  %12 = call i64 @_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEEcvNS0_IvEEEv(ptr noundef nonnull align 8 dereferenceable(8) %6) #2
  %13 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %5, i32 0, i32 0
  %14 = inttoptr i64 %12 to ptr
  store ptr %14, ptr %13, align 8
  %15 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %5, i32 0, i32 0
  %16 = load ptr, ptr %15, align 8
  call void @_ZNKSt7__n486114suspend_always13await_suspendENS_16coroutine_handleIvEE(ptr noundef nonnull align 1 dereferenceable(1) %7, ptr %16) #2
  ret void
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZNKSt7__n486114suspend_always13await_suspendENS_16coroutine_handleIvEE(ptr noundef nonnull align 1 dereferenceable(1) %0, ptr %1) #7 comdat {
  %3 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %4 = alloca ptr, align 8
  %5 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %3, i32 0, i32 0
  store ptr %1, ptr %5, align 8
  store ptr %0, ptr %4, align 8
  %6 = load ptr, ptr %4, align 8
  ret void
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local i64 @_ZNSt7__n486116coroutine_handleIN4task12promise_typeEE12from_addressEPv(ptr noundef %0) #7 comdat {
  %2 = alloca %"struct.std::__n4861::coroutine_handle", align 8
  %3 = alloca ptr, align 8
  store ptr %0, ptr %3, align 8
  call void @_ZNSt7__n486116coroutine_handleIN4task12promise_typeEEC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %2) #2
  %4 = load ptr, ptr %3, align 8
  %5 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %2, i32 0, i32 0
  store ptr %4, ptr %5, align 8
  %6 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %2, i32 0, i32 0
  %7 = load ptr, ptr %6, align 8
  %8 = ptrtoint ptr %7 to i64
  ret i64 %8
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local i64 @_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEEcvNS0_IvEEEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #7 comdat {
  %2 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %3 = alloca ptr, align 8
  store ptr %0, ptr %3, align 8
  %4 = load ptr, ptr %3, align 8
  %5 = call noundef ptr @_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEE7addressEv(ptr noundef nonnull align 8 dereferenceable(8) %4) #2
  %6 = call i64 @_ZNSt7__n486116coroutine_handleIvE12from_addressEPv(ptr noundef %5) #2
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
declare i8 @llvm.coro.suspend(token, i1) #2

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZNKSt7__n486114suspend_always12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %0) #7 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  ret void
}

; Function Attrs: nocallback nofree nosync nounwind willreturn memory(argmem: readwrite)
declare void @llvm.lifetime.end.p0(ptr captures(none)) #5

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local noundef i1 @_ZN5later11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %0) #7 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  ret i1 false
}

; Function Attrs: alwaysinline mustprogress
define internal void @_Z4bodyi.__await_suspend_wrapper__await(ptr noundef nonnull %0, ptr noundef %1) #9 {
  %3 = alloca ptr, align 8
  %4 = alloca ptr, align 8
  %5 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %6 = alloca %"struct.std::__n4861::coroutine_handle", align 8
  store ptr %0, ptr %3, align 8
  store ptr %1, ptr %4, align 8
  %7 = load ptr, ptr %3, align 8
  %8 = load ptr, ptr %4, align 8
  %9 = call i64 @_ZNSt7__n486116coroutine_handleIN4task12promise_typeEE12from_addressEPv(ptr noundef %8) #2
  %10 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %6, i32 0, i32 0
  %11 = inttoptr i64 %9 to ptr
  store ptr %11, ptr %10, align 8
  %12 = call i64 @_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEEcvNS0_IvEEEv(ptr noundef nonnull align 8 dereferenceable(8) %6) #2
  %13 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %5, i32 0, i32 0
  %14 = inttoptr i64 %12 to ptr
  store ptr %14, ptr %13, align 8
  %15 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %5, i32 0, i32 0
  %16 = load ptr, ptr %15, align 8
  call void @_ZN5later13await_suspendENSt7__n486116coroutine_handleIvEE(ptr noundef nonnull align 1 dereferenceable(1) %7, ptr %16)
  ret void
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZN5later13await_suspendENSt7__n486116coroutine_handleIvEE(ptr noundef nonnull align 1 dereferenceable(1) %0, ptr %1) #7 comdat {
  %3 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %4 = alloca ptr, align 8
  %5 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %3, i32 0, i32 0
  store ptr %1, ptr %5, align 8
  store ptr %0, ptr %4, align 8
  %6 = load ptr, ptr %4, align 8
  ret void
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZN5later12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %0) #7 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  ret void
}

declare ptr @__cxa_allocate_exception(i64)

declare void @_ZNSt13runtime_errorC1EPKc(ptr noundef nonnull align 8 dereferenceable(16), ptr noundef) unnamed_addr #10

declare void @__cxa_free_exception(ptr)

; Function Attrs: nounwind
declare void @_ZNSt13runtime_errorD1Ev(ptr noundef nonnull align 8 dereferenceable(16)) unnamed_addr #11

declare void @__cxa_throw(ptr, ptr, ptr)

; Function Attrs: alwaysinline mustprogress
define internal void @_Z4bodyi.__await_suspend_wrapper__await2(ptr noundef nonnull %0, ptr noundef %1) #9 {
  %3 = alloca ptr, align 8
  %4 = alloca ptr, align 8
  %5 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %6 = alloca %"struct.std::__n4861::coroutine_handle", align 8
  store ptr %0, ptr %3, align 8
  store ptr %1, ptr %4, align 8
  %7 = load ptr, ptr %3, align 8
  %8 = load ptr, ptr %4, align 8
  %9 = call i64 @_ZNSt7__n486116coroutine_handleIN4task12promise_typeEE12from_addressEPv(ptr noundef %8) #2
  %10 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %6, i32 0, i32 0
  %11 = inttoptr i64 %9 to ptr
  store ptr %11, ptr %10, align 8
  %12 = call i64 @_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEEcvNS0_IvEEEv(ptr noundef nonnull align 8 dereferenceable(8) %6) #2
  %13 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %5, i32 0, i32 0
  %14 = inttoptr i64 %12 to ptr
  store ptr %14, ptr %13, align 8
  %15 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %5, i32 0, i32 0
  %16 = load ptr, ptr %15, align 8
  call void @_ZN5later13await_suspendENSt7__n486116coroutine_handleIvEE(ptr noundef nonnull align 1 dereferenceable(1) %7, ptr %16)
  ret void
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZN4DtorD2Ev(ptr noundef nonnull align 4 dereferenceable(4) %0) unnamed_addr #7 comdat personality ptr @__gxx_personality_v0 {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = load i32, ptr @_ZL5dtors, align 4
  %5 = add nsw i32 %4, 1
  store i32 %5, ptr @_ZL5dtors, align 4
  %6 = getelementptr inbounds nuw %struct.Dtor, ptr %3, i32 0, i32 0
  %7 = load i32, ptr %6, align 4
  %8 = invoke i32 (ptr, ...) @printf(ptr noundef @.str.5, i32 noundef %7)
          to label %9 unwind label %10

9:                                                ; preds = %1
  ret void

10:                                               ; preds = %1
  %11 = landingpad { ptr, i32 }
          catch ptr null
  %12 = extractvalue { ptr, i32 } %11, 0
  call void @__clang_call_terminate(ptr %12) #19
  unreachable
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZN4task12promise_type11return_voidEv(ptr noundef nonnull align 1 dereferenceable(1) %0) #7 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  ret void
}

declare ptr @__cxa_begin_catch(ptr)

; Function Attrs: mustprogress noinline optnone uwtable
define linkonce_odr dso_local void @_ZN4task12promise_type19unhandled_exceptionEv(ptr noundef nonnull align 1 dereferenceable(1) %0) #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  call void @__cxa_rethrow() #18
  unreachable
}

declare void @__cxa_end_catch()

; Function Attrs: noinline noreturn nounwind uwtable
define linkonce_odr hidden void @__clang_call_terminate(ptr noundef %0) #12 comdat {
  %2 = call ptr @__cxa_begin_catch(ptr %0) #2
  call void @_ZSt9terminatev() #19
  unreachable
}

declare void @_ZSt9terminatev()

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZN4task12promise_type13final_suspendEv(ptr noundef nonnull align 1 dereferenceable(1) %0) #7 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  ret void
}

; Function Attrs: alwaysinline mustprogress
define internal void @_Z4bodyi.__await_suspend_wrapper__final(ptr noundef nonnull %0, ptr noundef %1) #9 {
  %3 = alloca ptr, align 8
  %4 = alloca ptr, align 8
  %5 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %6 = alloca %"struct.std::__n4861::coroutine_handle", align 8
  store ptr %0, ptr %3, align 8
  store ptr %1, ptr %4, align 8
  %7 = load ptr, ptr %3, align 8
  %8 = load ptr, ptr %4, align 8
  %9 = call i64 @_ZNSt7__n486116coroutine_handleIN4task12promise_typeEE12from_addressEPv(ptr noundef %8) #2
  %10 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %6, i32 0, i32 0
  %11 = inttoptr i64 %9 to ptr
  store ptr %11, ptr %10, align 8
  %12 = call i64 @_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEEcvNS0_IvEEEv(ptr noundef nonnull align 8 dereferenceable(8) %6) #2
  %13 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %5, i32 0, i32 0
  %14 = inttoptr i64 %12 to ptr
  store ptr %14, ptr %13, align 8
  %15 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %5, i32 0, i32 0
  %16 = load ptr, ptr %15, align 8
  call void @_ZNKSt7__n486114suspend_always13await_suspendENS_16coroutine_handleIvEE(ptr noundef nonnull align 1 dereferenceable(1) %7, ptr %16) #2
  ret void
}

; Function Attrs: nounwind
declare void @llvm.coro.end(ptr, i1, token) #2

; Function Attrs: nounwind memory(none)
declare i1 @llvm.coro.is_in_ramp() #4

; Function Attrs: nobuiltin nounwind
declare void @_ZdlPvm(ptr noundef, i64 noundef) #13

; Function Attrs: nounwind memory(argmem: read)
declare ptr @llvm.coro.free(token, ptr readonly captures(none)) #14

; Function Attrs: mustprogress noinline norecurse optnone uwtable
define dso_local noundef i32 @main() #15 personality ptr @__gxx_personality_v0 {
  %1 = alloca i32, align 4
  %2 = alloca %struct.task, align 8
  %3 = alloca ptr, align 8
  %4 = alloca i32, align 4
  %5 = alloca ptr, align 8
  %6 = alloca %struct.task, align 8
  store i32 0, ptr %1, align 4
  %7 = call i64 @_Z4bodyi(i32 noundef 1)
  %8 = getelementptr inbounds nuw %struct.task, ptr %2, i32 0, i32 0
  %9 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %8, i32 0, i32 0
  %10 = inttoptr i64 %7 to ptr
  store ptr %10, ptr %9, align 8
  %11 = getelementptr inbounds nuw %struct.task, ptr %2, i32 0, i32 0
  call void @_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEE6resumeEv(ptr noundef nonnull align 8 dereferenceable(8) %11)
  %12 = getelementptr inbounds nuw %struct.task, ptr %2, i32 0, i32 0
  %13 = call noundef i1 @_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEE4doneEv(ptr noundef nonnull align 8 dereferenceable(8) %12) #2
  %14 = zext i1 %13 to i32
  %15 = load i32, ptr @_ZL5dtors, align 4
  %16 = call i32 (ptr, ...) @printf(ptr noundef @.str.1, i32 noundef %14, i32 noundef %15)
  %17 = getelementptr inbounds nuw %struct.task, ptr %2, i32 0, i32 0
  invoke void @_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEE6resumeEv(ptr noundef nonnull align 8 dereferenceable(8) %17)
          to label %18 unwind label %19

18:                                               ; preds = %0
  br label %41

19:                                               ; preds = %0
  %20 = landingpad { ptr, i32 }
          catch ptr @_ZTISt9exception
  %21 = extractvalue { ptr, i32 } %20, 0
  store ptr %21, ptr %3, align 8
  %22 = extractvalue { ptr, i32 } %20, 1
  store i32 %22, ptr %4, align 4
  br label %23

23:                                               ; preds = %19
  %24 = load i32, ptr %4, align 4
  %25 = call i32 @llvm.eh.typeid.for.p0(ptr @_ZTISt9exception) #2
  %26 = icmp eq i32 %24, %25
  br i1 %26, label %27, label %63

27:                                               ; preds = %23
  %28 = load ptr, ptr %3, align 8
  %29 = call ptr @__cxa_begin_catch(ptr %28) #2
  store ptr %29, ptr %5, align 8
  %30 = load ptr, ptr %5, align 8, !nonnull !6, !align !7
  %31 = load ptr, ptr %30, align 8
  %32 = getelementptr inbounds ptr, ptr %31, i64 2
  %33 = load ptr, ptr %32, align 8
  %34 = call noundef ptr %33(ptr noundef nonnull align 8 dereferenceable(8) %30) #2
  %35 = getelementptr inbounds nuw %struct.task, ptr %2, i32 0, i32 0
  %36 = call noundef i1 @_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEE4doneEv(ptr noundef nonnull align 8 dereferenceable(8) %35) #2
  %37 = zext i1 %36 to i32
  %38 = load i32, ptr @_ZL5dtors, align 4
  %39 = invoke i32 (ptr, ...) @printf(ptr noundef @.str.2, ptr noundef %34, i32 noundef %37, i32 noundef %38)
          to label %40 unwind label %58

40:                                               ; preds = %27
  call void @__cxa_end_catch()
  br label %41

41:                                               ; preds = %40, %18
  %42 = getelementptr inbounds nuw %struct.task, ptr %2, i32 0, i32 0
  call void @_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEE7destroyEv(ptr noundef nonnull align 8 dereferenceable(8) %42)
  %43 = load i32, ptr @_ZL5dtors, align 4
  %44 = call i32 (ptr, ...) @printf(ptr noundef @.str.3, i32 noundef %43)
  %45 = call i64 @_Z4bodyi(i32 noundef 0)
  %46 = getelementptr inbounds nuw %struct.task, ptr %6, i32 0, i32 0
  %47 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %46, i32 0, i32 0
  %48 = inttoptr i64 %45 to ptr
  store ptr %48, ptr %47, align 8
  %49 = getelementptr inbounds nuw %struct.task, ptr %6, i32 0, i32 0
  call void @_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEE6resumeEv(ptr noundef nonnull align 8 dereferenceable(8) %49)
  %50 = getelementptr inbounds nuw %struct.task, ptr %6, i32 0, i32 0
  call void @_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEE6resumeEv(ptr noundef nonnull align 8 dereferenceable(8) %50)
  %51 = getelementptr inbounds nuw %struct.task, ptr %6, i32 0, i32 0
  %52 = call noundef i1 @_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEE4doneEv(ptr noundef nonnull align 8 dereferenceable(8) %51) #2
  %53 = zext i1 %52 to i32
  %54 = load i32, ptr @_ZL5dtors, align 4
  %55 = call i32 (ptr, ...) @printf(ptr noundef @.str.4, i32 noundef %53, i32 noundef %54)
  %56 = getelementptr inbounds nuw %struct.task, ptr %6, i32 0, i32 0
  call void @_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEE7destroyEv(ptr noundef nonnull align 8 dereferenceable(8) %56)
  %57 = load i32, ptr %1, align 4
  ret i32 %57

58:                                               ; preds = %27
  %59 = landingpad { ptr, i32 }
          cleanup
  %60 = extractvalue { ptr, i32 } %59, 0
  store ptr %60, ptr %3, align 8
  %61 = extractvalue { ptr, i32 } %59, 1
  store i32 %61, ptr %4, align 4
  invoke void @__cxa_end_catch()
          to label %62 unwind label %68

62:                                               ; preds = %58
  br label %63

63:                                               ; preds = %62, %23
  %64 = load ptr, ptr %3, align 8
  %65 = load i32, ptr %4, align 4
  %66 = insertvalue { ptr, i32 } poison, ptr %64, 0
  %67 = insertvalue { ptr, i32 } %66, i32 %65, 1
  resume { ptr, i32 } %67

68:                                               ; preds = %58
  %69 = landingpad { ptr, i32 }
          catch ptr null
  %70 = extractvalue { ptr, i32 } %69, 0
  call void @__clang_call_terminate(ptr %70) #19
  unreachable
}

; Function Attrs: mustprogress noinline optnone uwtable
define linkonce_odr dso_local void @_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEE6resumeEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %3, i32 0, i32 0
  %5 = load ptr, ptr %4, align 8
  call void @llvm.coro.resume(ptr %5)
  ret void
}

declare i32 @printf(ptr noundef, ...) #10

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local noundef i1 @_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEE4doneEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #7 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %3, i32 0, i32 0
  %5 = load ptr, ptr %4, align 8
  %6 = call i1 @llvm.coro.done(ptr %5)
  ret i1 %6
}

; Function Attrs: nounwind memory(none)
declare i32 @llvm.eh.typeid.for.p0(ptr) #4

; Function Attrs: mustprogress noinline optnone uwtable
define linkonce_odr dso_local void @_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEE7destroyEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %3, i32 0, i32 0
  %5 = load ptr, ptr %4, align 8
  call void @llvm.coro.destroy(ptr %5)
  ret void
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local i64 @_ZNSt7__n486116coroutine_handleIN4task12promise_typeEE12from_promiseERS2_(ptr noundef nonnull align 1 dereferenceable(1) %0) #7 comdat {
  %2 = alloca %"struct.std::__n4861::coroutine_handle", align 8
  %3 = alloca ptr, align 8
  store ptr %0, ptr %3, align 8
  call void @_ZNSt7__n486116coroutine_handleIN4task12promise_typeEEC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %2) #2
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
define linkonce_odr dso_local void @_ZNSt7__n486116coroutine_handleIN4task12promise_typeEEC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %0) unnamed_addr #7 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %3, i32 0, i32 0
  store ptr null, ptr %4, align 8
  ret void
}

; Function Attrs: nounwind memory(none)
declare ptr @llvm.coro.promise(ptr captures(none), i32, i1) #4

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local i64 @_ZNSt7__n486116coroutine_handleIvE12from_addressEPv(ptr noundef %0) #7 comdat {
  %2 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %3 = alloca ptr, align 8
  store ptr %0, ptr %3, align 8
  call void @_ZNSt7__n486116coroutine_handleIvEC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %2) #2
  %4 = load ptr, ptr %3, align 8
  %5 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %2, i32 0, i32 0
  store ptr %4, ptr %5, align 8
  %6 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %2, i32 0, i32 0
  %7 = load ptr, ptr %6, align 8
  %8 = ptrtoint ptr %7 to i64
  ret i64 %8
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local noundef ptr @_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEE7addressEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #7 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %3, i32 0, i32 0
  %5 = load ptr, ptr %4, align 8
  ret ptr %5
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZNSt7__n486116coroutine_handleIvEC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %0) unnamed_addr #7 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %3, i32 0, i32 0
  store ptr null, ptr %4, align 8
  ret void
}

declare void @__cxa_rethrow()

declare void @llvm.coro.resume(ptr)

; Function Attrs: nounwind memory(argmem: readwrite)
declare i1 @llvm.coro.done(ptr readonly captures(none)) #16

declare void @llvm.coro.destroy(ptr)

attributes #0 = { mustprogress noinline optnone presplitcoroutine uwtable "frame-pointer"="non-leaf-no-reserve" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #1 = { nocallback nofree nosync nounwind willreturn memory(argmem: read) }
attributes #2 = { nounwind }
attributes #3 = { nobuiltin allocsize(0) "frame-pointer"="non-leaf-no-reserve" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #4 = { nounwind memory(none) }
attributes #5 = { nocallback nofree nosync nounwind willreturn memory(argmem: readwrite) }
attributes #6 = { mustprogress noinline optnone uwtable "frame-pointer"="non-leaf-no-reserve" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #7 = { mustprogress noinline nounwind optnone uwtable "frame-pointer"="non-leaf-no-reserve" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #8 = { nomerge nounwind }
attributes #9 = { alwaysinline mustprogress "sample-profile-suffix-elision-policy"="selected" }
attributes #10 = { "frame-pointer"="non-leaf-no-reserve" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #11 = { nounwind "frame-pointer"="non-leaf-no-reserve" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #12 = { noinline noreturn nounwind uwtable "frame-pointer"="non-leaf-no-reserve" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #13 = { nobuiltin nounwind "frame-pointer"="non-leaf-no-reserve" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #14 = { nounwind memory(argmem: read) }
attributes #15 = { mustprogress noinline norecurse optnone uwtable "frame-pointer"="non-leaf-no-reserve" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #16 = { nounwind memory(argmem: readwrite) }
attributes #17 = { allocsize(0) }
attributes #18 = { noreturn }
attributes #19 = { noreturn nounwind }

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
