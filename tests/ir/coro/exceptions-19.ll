; clang++-19 -std=c++20 -O0 -S -emit-llvm -Xclang -disable-llvm-passes
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
;@ dialect: 19
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
  %3 = alloca i32, align 4
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
  %16 = bitcast ptr %5 to ptr
  %17 = call token @llvm.coro.id(i32 16, ptr %16, ptr null, ptr null)
  %18 = call i1 @llvm.coro.alloc(token %17)
  br i1 %18, label %19, label %22

19:                                               ; preds = %1
  %20 = call i64 @llvm.coro.size.i64()
  %21 = call noalias noundef nonnull ptr @_Znwm(i64 noundef %20) #17
  br label %22

22:                                               ; preds = %19, %1
  %23 = phi ptr [ null, %1 ], [ %21, %19 ]
  %24 = call ptr @llvm.coro.begin(token %17, ptr %23)
  call void @llvm.lifetime.start.p0(i64 4, ptr %4) #2
  %25 = load i32, ptr %3, align 4
  store i32 %25, ptr %4, align 4
  call void @llvm.lifetime.start.p0(i64 1, ptr %5) #2
  %26 = invoke i64 @_ZN4task12promise_type17get_return_objectEv(ptr noundef nonnull align 1 dereferenceable(1) %5)
          to label %27 unwind label %36

27:                                               ; preds = %22
  %28 = getelementptr inbounds %struct.task, ptr %2, i32 0, i32 0
  %29 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %28, i32 0, i32 0
  %30 = inttoptr i64 %26 to ptr
  store ptr %30, ptr %29, align 8
  call void @llvm.lifetime.start.p0(i64 1, ptr %8) #2
  call void @_ZN4task12promise_type15initial_suspendEv(ptr noundef nonnull align 1 dereferenceable(1) %5) #2
  %31 = call noundef i1 @_ZNKSt7__n486114suspend_always11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %8) #2
  br i1 %31, label %40, label %32

32:                                               ; preds = %27
  %33 = call token @llvm.coro.save(ptr null)
  call void @llvm.coro.await.suspend.void(ptr %8, ptr %24, ptr @_Z4bodyi.__await_suspend_wrapper__init) #2
  %34 = call i8 @llvm.coro.suspend(token %33, i1 false)
  switch i8 %34, label %140 [
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
  br label %149

40:                                               ; preds = %32, %27
  call void @_ZNKSt7__n486114suspend_always12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %8) #2
  br label %41

41:                                               ; preds = %40, %35
  %42 = phi i32 [ 0, %40 ], [ 2, %35 ]
  call void @llvm.lifetime.end.p0(i64 1, ptr %8) #2
  switch i32 %42, label %132 [
    i32 0, label %43
  ]

43:                                               ; preds = %41
  call void @llvm.lifetime.start.p0(i64 4, ptr %10) #2
  %44 = getelementptr inbounds %struct.Dtor, ptr %10, i32 0, i32 0
  %45 = load i32, ptr %4, align 4
  store i32 %45, ptr %44, align 4
  call void @llvm.lifetime.start.p0(i64 1, ptr %11) #2
  %46 = invoke noundef i1 @_ZN5later11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %11)
          to label %47 unwind label %53

47:                                               ; preds = %43
  br i1 %46, label %57, label %48

48:                                               ; preds = %47
  %49 = call token @llvm.coro.save(ptr null)
  invoke void @llvm.coro.await.suspend.void(ptr %11, ptr %24, ptr @_Z4bodyi.__await_suspend_wrapper__await)
          to label %50 unwind label %53

50:                                               ; preds = %48
  %51 = call i8 @llvm.coro.suspend(token %49, i1 false)
  switch i8 %51, label %140 [
    i8 0, label %57
    i8 1, label %52
  ]

52:                                               ; preds = %50
  br label %59

53:                                               ; preds = %57, %48, %43
  %54 = landingpad { ptr, i32 }
          catch ptr null
  %55 = extractvalue { ptr, i32 } %54, 0
  store ptr %55, ptr %6, align 8
  %56 = extractvalue { ptr, i32 } %54, 1
  store i32 %56, ptr %7, align 4
  call void @llvm.lifetime.end.p0(i64 1, ptr %11) #2
  br label %102

57:                                               ; preds = %50, %47
  invoke void @_ZN5later12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %11)
          to label %58 unwind label %53

58:                                               ; preds = %57
  br label %59

59:                                               ; preds = %58, %52
  %60 = phi i32 [ 0, %58 ], [ 2, %52 ]
  call void @llvm.lifetime.end.p0(i64 1, ptr %11) #2
  switch i32 %60, label %97 [
    i32 0, label %61
  ]

61:                                               ; preds = %59
  call void @llvm.lifetime.start.p0(i64 4, ptr %12) #2
  %62 = getelementptr inbounds %struct.Dtor, ptr %12, i32 0, i32 0
  %63 = load i32, ptr %4, align 4
  %64 = add nsw i32 %63, 1
  store i32 %64, ptr %62, align 4
  %65 = load i32, ptr %4, align 4
  %66 = icmp sgt i32 %65, 0
  br i1 %66, label %67, label %78

67:                                               ; preds = %61
  %68 = call ptr @__cxa_allocate_exception(i64 16) #2
  invoke void @_ZNSt13runtime_errorC1EPKc(ptr noundef nonnull align 8 dereferenceable(16) %68, ptr noundef @.str)
          to label %69 unwind label %70

69:                                               ; preds = %67
  invoke void @__cxa_throw(ptr %68, ptr @_ZTISt13runtime_error, ptr @_ZNSt13runtime_errorD1Ev) #18
          to label %163 unwind label %74

70:                                               ; preds = %67
  %71 = landingpad { ptr, i32 }
          catch ptr null
  %72 = extractvalue { ptr, i32 } %71, 0
  store ptr %72, ptr %6, align 8
  %73 = extractvalue { ptr, i32 } %71, 1
  store i32 %73, ptr %7, align 4
  call void @__cxa_free_exception(ptr %68) #2
  br label %101

74:                                               ; preds = %69
  %75 = landingpad { ptr, i32 }
          catch ptr null
  %76 = extractvalue { ptr, i32 } %75, 0
  store ptr %76, ptr %6, align 8
  %77 = extractvalue { ptr, i32 } %75, 1
  store i32 %77, ptr %7, align 4
  br label %101

78:                                               ; preds = %61
  call void @llvm.lifetime.start.p0(i64 1, ptr %13) #2
  %79 = invoke noundef i1 @_ZN5later11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %13)
          to label %80 unwind label %86

80:                                               ; preds = %78
  br i1 %79, label %90, label %81

81:                                               ; preds = %80
  %82 = call token @llvm.coro.save(ptr null)
  invoke void @llvm.coro.await.suspend.void(ptr %13, ptr %24, ptr @_Z4bodyi.__await_suspend_wrapper__await2)
          to label %83 unwind label %86

83:                                               ; preds = %81
  %84 = call i8 @llvm.coro.suspend(token %82, i1 false)
  switch i8 %84, label %140 [
    i8 0, label %90
    i8 1, label %85
  ]

85:                                               ; preds = %83
  br label %92

86:                                               ; preds = %90, %81, %78
  %87 = landingpad { ptr, i32 }
          catch ptr null
  %88 = extractvalue { ptr, i32 } %87, 0
  store ptr %88, ptr %6, align 8
  %89 = extractvalue { ptr, i32 } %87, 1
  store i32 %89, ptr %7, align 4
  call void @llvm.lifetime.end.p0(i64 1, ptr %13) #2
  br label %101

90:                                               ; preds = %83, %80
  invoke void @_ZN5later12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %13)
          to label %91 unwind label %86

91:                                               ; preds = %90
  br label %92

92:                                               ; preds = %91, %85
  %93 = phi i32 [ 0, %91 ], [ 2, %85 ]
  call void @llvm.lifetime.end.p0(i64 1, ptr %13) #2
  switch i32 %93, label %95 [
    i32 0, label %94
  ]

94:                                               ; preds = %92
  br label %95

95:                                               ; preds = %94, %92
  %96 = phi i32 [ %93, %92 ], [ 0, %94 ]
  call void @_ZN4DtorD2Ev(ptr noundef nonnull align 4 dereferenceable(4) %12) #2
  call void @llvm.lifetime.end.p0(i64 4, ptr %12) #2
  br label %97

97:                                               ; preds = %95, %59
  %98 = phi i32 [ %60, %59 ], [ %96, %95 ]
  call void @_ZN4DtorD2Ev(ptr noundef nonnull align 4 dereferenceable(4) %10) #2
  call void @llvm.lifetime.end.p0(i64 4, ptr %10) #2
  switch i32 %98, label %132 [
    i32 0, label %99
  ]

99:                                               ; preds = %97
  invoke void @_ZN4task12promise_type11return_voidEv(ptr noundef nonnull align 1 dereferenceable(1) %5)
          to label %100 unwind label %103

100:                                              ; preds = %99
  br label %113

101:                                              ; preds = %86, %74, %70
  call void @_ZN4DtorD2Ev(ptr noundef nonnull align 4 dereferenceable(4) %12) #2
  call void @llvm.lifetime.end.p0(i64 4, ptr %12) #2
  br label %102

102:                                              ; preds = %101, %53
  call void @_ZN4DtorD2Ev(ptr noundef nonnull align 4 dereferenceable(4) %10) #2
  call void @llvm.lifetime.end.p0(i64 4, ptr %10) #2
  br label %107

103:                                              ; preds = %99
  %104 = landingpad { ptr, i32 }
          catch ptr null
  %105 = extractvalue { ptr, i32 } %104, 0
  store ptr %105, ptr %6, align 8
  %106 = extractvalue { ptr, i32 } %104, 1
  store i32 %106, ptr %7, align 4
  br label %107

107:                                              ; preds = %103, %102
  %108 = load ptr, ptr %6, align 8
  %109 = call ptr @__cxa_begin_catch(ptr %108) #2
  invoke void @_ZN4task12promise_type19unhandled_exceptionEv(ptr noundef nonnull align 1 dereferenceable(1) %5)
          to label %110 unwind label %119

110:                                              ; preds = %107
  invoke void @__cxa_end_catch()
          to label %111 unwind label %123

111:                                              ; preds = %110
  br label %112

112:                                              ; preds = %111
  br label %113

113:                                              ; preds = %112, %100
  call void @llvm.lifetime.start.p0(i64 1, ptr %14) #2
  call void @_ZN4task12promise_type13final_suspendEv(ptr noundef nonnull align 1 dereferenceable(1) %5) #2
  %114 = call noundef i1 @_ZNKSt7__n486114suspend_always11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %14) #2
  br i1 %114, label %128, label %115

115:                                              ; preds = %113
  %116 = call token @llvm.coro.save(ptr null)
  call void @llvm.coro.await.suspend.void(ptr %14, ptr %24, ptr @_Z4bodyi.__await_suspend_wrapper__final) #2
  %117 = call i8 @llvm.coro.suspend(token %116, i1 true)
  switch i8 %117, label %140 [
    i8 0, label %128
    i8 1, label %118
  ]

118:                                              ; preds = %115
  br label %129

119:                                              ; preds = %107
  %120 = landingpad { ptr, i32 }
          cleanup
  %121 = extractvalue { ptr, i32 } %120, 0
  store ptr %121, ptr %6, align 8
  %122 = extractvalue { ptr, i32 } %120, 1
  store i32 %122, ptr %7, align 4
  invoke void @__cxa_end_catch()
          to label %127 unwind label %160

123:                                              ; preds = %110
  %124 = landingpad { ptr, i32 }
          cleanup
  %125 = extractvalue { ptr, i32 } %124, 0
  store ptr %125, ptr %6, align 8
  %126 = extractvalue { ptr, i32 } %124, 1
  store i32 %126, ptr %7, align 4
  br label %146

127:                                              ; preds = %119
  br label %146

128:                                              ; preds = %115, %113
  call void @_ZNKSt7__n486114suspend_always12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %14) #2
  br label %129

129:                                              ; preds = %128, %118
  %130 = phi i32 [ 0, %128 ], [ 2, %118 ]
  call void @llvm.lifetime.end.p0(i64 1, ptr %14) #2
  switch i32 %130, label %132 [
    i32 0, label %131
  ]

131:                                              ; preds = %129
  br label %132

132:                                              ; preds = %131, %129, %97, %41
  %133 = phi i32 [ %42, %41 ], [ %98, %97 ], [ %130, %129 ], [ 0, %131 ]
  call void @llvm.lifetime.end.p0(i64 1, ptr %5) #2
  call void @llvm.lifetime.end.p0(i64 4, ptr %4) #2
  %134 = call ptr @llvm.coro.free(token %17, ptr %24)
  %135 = icmp ne ptr %134, null
  br i1 %135, label %136, label %138

136:                                              ; preds = %132
  %137 = call i64 @llvm.coro.size.i64()
  call void @_ZdlPvm(ptr noundef %134, i64 noundef %137) #2
  br label %138

138:                                              ; preds = %132, %136
  switch i32 %133, label %163 [
    i32 0, label %139
    i32 2, label %140
  ]

139:                                              ; preds = %138
  br label %140

140:                                              ; preds = %139, %138, %115, %83, %50, %32
  %141 = call i1 @llvm.coro.end(ptr null, i1 false, token none)
  %142 = getelementptr inbounds %struct.task, ptr %2, i32 0, i32 0
  %143 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %142, i32 0, i32 0
  %144 = load ptr, ptr %143, align 8
  %145 = ptrtoint ptr %144 to i64
  ret i64 %145

146:                                              ; preds = %127, %123
  %147 = call i1 @llvm.coro.end(ptr null, i1 true, token none)
  br i1 %147, label %155, label %148

148:                                              ; preds = %146
  br label %149

149:                                              ; preds = %148, %36
  call void @llvm.lifetime.end.p0(i64 1, ptr %5) #2
  call void @llvm.lifetime.end.p0(i64 4, ptr %4) #2
  %150 = call ptr @llvm.coro.free(token %17, ptr %24)
  %151 = icmp ne ptr %150, null
  br i1 %151, label %152, label %154

152:                                              ; preds = %149
  %153 = call i64 @llvm.coro.size.i64()
  call void @_ZdlPvm(ptr noundef %150, i64 noundef %153) #2
  br label %154

154:                                              ; preds = %149, %152
  br label %155

155:                                              ; preds = %154, %146
  %156 = load ptr, ptr %6, align 8
  %157 = load i32, ptr %7, align 4
  %158 = insertvalue { ptr, i32 } poison, ptr %156, 0
  %159 = insertvalue { ptr, i32 } %158, i32 %157, 1
  resume { ptr, i32 } %159

160:                                              ; preds = %119
  %161 = landingpad { ptr, i32 }
          catch ptr null
  %162 = extractvalue { ptr, i32 } %161, 0
  call void @__clang_call_terminate(ptr %162) #19
  unreachable

163:                                              ; preds = %138, %69
  unreachable
}

; Function Attrs: nocallback nofree nosync nounwind willreturn memory(argmem: read)
declare token @llvm.coro.id(i32, ptr readnone, ptr nocapture readonly, ptr) #1

; Function Attrs: nounwind
declare i1 @llvm.coro.alloc(token) #2

; Function Attrs: nobuiltin allocsize(0)
declare noundef nonnull ptr @_Znwm(i64 noundef) #3

; Function Attrs: nounwind memory(none)
declare i64 @llvm.coro.size.i64() #4

; Function Attrs: nounwind
declare ptr @llvm.coro.begin(token, ptr writeonly) #2

; Function Attrs: nocallback nofree nosync nounwind willreturn memory(argmem: readwrite)
declare void @llvm.lifetime.start.p0(i64 immarg, ptr nocapture) #5

; Function Attrs: mustprogress noinline optnone uwtable
define linkonce_odr dso_local i64 @_ZN4task12promise_type17get_return_objectEv(ptr noundef nonnull align 1 dereferenceable(1) %0) #6 comdat {
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
define private void @_Z4bodyi.__await_suspend_wrapper__init(ptr noundef nonnull %0, ptr noundef %1) #9 {
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
define linkonce_odr dso_local void @_ZNKSt7__n486114suspend_always13await_suspendENS_16coroutine_handleIvEE(ptr noundef nonnull align 1 dereferenceable(1) %0, i64 %1) #7 comdat {
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
define linkonce_odr dso_local i64 @_ZNSt7__n486116coroutine_handleIN4task12promise_typeEE12from_addressEPv(ptr noundef %0) #7 comdat {
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
define linkonce_odr dso_local i64 @_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEEcvNS0_IvEEEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #7 comdat {
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
define linkonce_odr dso_local void @_ZNKSt7__n486114suspend_always12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %0) #7 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  ret void
}

; Function Attrs: nocallback nofree nosync nounwind willreturn memory(argmem: readwrite)
declare void @llvm.lifetime.end.p0(i64 immarg, ptr nocapture) #5

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local noundef i1 @_ZN5later11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %0) #7 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  ret i1 false
}

; Function Attrs: alwaysinline mustprogress
define private void @_Z4bodyi.__await_suspend_wrapper__await(ptr noundef nonnull %0, ptr noundef %1) #9 {
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
  call void @_ZN5later13await_suspendENSt7__n486116coroutine_handleIvEE(ptr noundef nonnull align 1 dereferenceable(1) %7, i64 %17)
  ret void
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZN5later13await_suspendENSt7__n486116coroutine_handleIvEE(ptr noundef nonnull align 1 dereferenceable(1) %0, i64 %1) #7 comdat {
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
define private void @_Z4bodyi.__await_suspend_wrapper__await2(ptr noundef nonnull %0, ptr noundef %1) #9 {
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
  call void @_ZN5later13await_suspendENSt7__n486116coroutine_handleIvEE(ptr noundef nonnull align 1 dereferenceable(1) %7, i64 %17)
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
  %6 = getelementptr inbounds %struct.Dtor, ptr %3, i32 0, i32 0
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
define private void @_Z4bodyi.__await_suspend_wrapper__final(ptr noundef nonnull %0, ptr noundef %1) #9 {
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

; Function Attrs: nobuiltin nounwind
declare void @_ZdlPvm(ptr noundef, i64 noundef) #13

; Function Attrs: nounwind memory(argmem: read)
declare ptr @llvm.coro.free(token, ptr nocapture readonly) #14

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
  %8 = getelementptr inbounds %struct.task, ptr %2, i32 0, i32 0
  %9 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %8, i32 0, i32 0
  %10 = inttoptr i64 %7 to ptr
  store ptr %10, ptr %9, align 8
  %11 = getelementptr inbounds %struct.task, ptr %2, i32 0, i32 0
  call void @_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEE6resumeEv(ptr noundef nonnull align 8 dereferenceable(8) %11)
  %12 = getelementptr inbounds %struct.task, ptr %2, i32 0, i32 0
  %13 = call noundef i1 @_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEE4doneEv(ptr noundef nonnull align 8 dereferenceable(8) %12) #2
  %14 = zext i1 %13 to i32
  %15 = load i32, ptr @_ZL5dtors, align 4
  %16 = call i32 (ptr, ...) @printf(ptr noundef @.str.1, i32 noundef %14, i32 noundef %15)
  %17 = getelementptr inbounds %struct.task, ptr %2, i32 0, i32 0
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
  %30 = load ptr, ptr %5, align 8
  %31 = load ptr, ptr %30, align 8
  %32 = getelementptr inbounds ptr, ptr %31, i64 2
  %33 = load ptr, ptr %32, align 8
  %34 = call noundef ptr %33(ptr noundef nonnull align 8 dereferenceable(8) %30) #2
  %35 = getelementptr inbounds %struct.task, ptr %2, i32 0, i32 0
  %36 = call noundef i1 @_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEE4doneEv(ptr noundef nonnull align 8 dereferenceable(8) %35) #2
  %37 = zext i1 %36 to i32
  %38 = load i32, ptr @_ZL5dtors, align 4
  %39 = invoke i32 (ptr, ...) @printf(ptr noundef @.str.2, ptr noundef %34, i32 noundef %37, i32 noundef %38)
          to label %40 unwind label %58

40:                                               ; preds = %27
  call void @__cxa_end_catch()
  br label %41

41:                                               ; preds = %40, %18
  %42 = getelementptr inbounds %struct.task, ptr %2, i32 0, i32 0
  call void @_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEE7destroyEv(ptr noundef nonnull align 8 dereferenceable(8) %42)
  %43 = load i32, ptr @_ZL5dtors, align 4
  %44 = call i32 (ptr, ...) @printf(ptr noundef @.str.3, i32 noundef %43)
  %45 = call i64 @_Z4bodyi(i32 noundef 0)
  %46 = getelementptr inbounds %struct.task, ptr %6, i32 0, i32 0
  %47 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %46, i32 0, i32 0
  %48 = inttoptr i64 %45 to ptr
  store ptr %48, ptr %47, align 8
  %49 = getelementptr inbounds %struct.task, ptr %6, i32 0, i32 0
  call void @_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEE6resumeEv(ptr noundef nonnull align 8 dereferenceable(8) %49)
  %50 = getelementptr inbounds %struct.task, ptr %6, i32 0, i32 0
  call void @_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEE6resumeEv(ptr noundef nonnull align 8 dereferenceable(8) %50)
  %51 = getelementptr inbounds %struct.task, ptr %6, i32 0, i32 0
  %52 = call noundef i1 @_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEE4doneEv(ptr noundef nonnull align 8 dereferenceable(8) %51) #2
  %53 = zext i1 %52 to i32
  %54 = load i32, ptr @_ZL5dtors, align 4
  %55 = call i32 (ptr, ...) @printf(ptr noundef @.str.4, i32 noundef %53, i32 noundef %54)
  %56 = getelementptr inbounds %struct.task, ptr %6, i32 0, i32 0
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
  %4 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %3, i32 0, i32 0
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
  %4 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %3, i32 0, i32 0
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
  %4 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %3, i32 0, i32 0
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
define linkonce_odr dso_local void @_ZNSt7__n486116coroutine_handleIN4task12promise_typeEEC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %0) unnamed_addr #7 comdat {
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
define linkonce_odr dso_local i64 @_ZNSt7__n486116coroutine_handleIvE12from_addressEPv(ptr noundef %0) #7 comdat {
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
define linkonce_odr dso_local noundef ptr @_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEE7addressEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #7 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %3, i32 0, i32 0
  %5 = load ptr, ptr %4, align 8
  ret ptr %5
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZNSt7__n486116coroutine_handleIvEC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %0) unnamed_addr #7 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle.0", ptr %3, i32 0, i32 0
  store ptr null, ptr %4, align 8
  ret void
}

declare void @__cxa_rethrow()

declare void @llvm.coro.resume(ptr)

; Function Attrs: nounwind memory(argmem: readwrite)
declare i1 @llvm.coro.done(ptr nocapture readonly) #16

declare void @llvm.coro.destroy(ptr)

attributes #0 = { mustprogress noinline optnone presplitcoroutine uwtable "frame-pointer"="non-leaf" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #1 = { nocallback nofree nosync nounwind willreturn memory(argmem: read) }
attributes #2 = { nounwind }
attributes #3 = { nobuiltin allocsize(0) "frame-pointer"="non-leaf" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #4 = { nounwind memory(none) }
attributes #5 = { nocallback nofree nosync nounwind willreturn memory(argmem: readwrite) }
attributes #6 = { mustprogress noinline optnone uwtable "frame-pointer"="non-leaf" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #7 = { mustprogress noinline nounwind optnone uwtable "frame-pointer"="non-leaf" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #8 = { nomerge nounwind }
attributes #9 = { alwaysinline mustprogress }
attributes #10 = { "frame-pointer"="non-leaf" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #11 = { nounwind "frame-pointer"="non-leaf" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #12 = { noinline noreturn nounwind uwtable "frame-pointer"="non-leaf" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #13 = { nobuiltin nounwind "frame-pointer"="non-leaf" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #14 = { nounwind memory(argmem: read) }
attributes #15 = { mustprogress noinline norecurse optnone uwtable "frame-pointer"="non-leaf" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #16 = { nounwind memory(argmem: readwrite) }
attributes #17 = { allocsize(0) }
attributes #18 = { noreturn }
attributes #19 = { noreturn nounwind }

!llvm.linker.options = !{}
!llvm.module.flags = !{!0, !1, !2, !3, !4}
!llvm.ident = !{!5}

!0 = !{i32 1, !"wchar_size", i32 4}
!1 = !{i32 8, !"PIC Level", i32 2}
!2 = !{i32 7, !"PIE Level", i32 2}
!3 = !{i32 7, !"uwtable", i32 2}
!4 = !{i32 7, !"frame-pointer", i32 1}
!5 = !{!"Debian clang version 19.1.7 (3+b1)"}
