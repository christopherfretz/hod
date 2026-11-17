; clang++-22 -std=c++20 -O0 -fstack-protector-strong -S -emit-llvm -Xclang -disable-llvm-passes
; (aarch64-unknown-linux-gnu; the target triple, datalayout, target-cpu and
; target-features removed, so that the seed runs on every target) of:
;
; // A coroutine with a local array (-fstack-protector-strong): the ramp and
; // the resume body are protected; a by-address parameter as well.
; #include <coroutine>
; #include <cstdio>
; #include <cstring>
; struct name {
;   char s[8];
;   explicit name(const char* p) { std::strncpy(s, p, 7); s[7] = 0; }
;   name(const name& o) { std::memcpy(s, o.s, 8); }
;   ~name() {}
; };
; struct task {
;   struct promise_type {
;     task get_return_object() { return task{std::coroutine_handle<promise_type>::from_promise(*this)}; }
;     std::suspend_never initial_suspend() noexcept { return {}; }
;     std::suspend_always final_suspend() noexcept { return {}; }
;     void return_void() noexcept {}
;     void unhandled_exception() noexcept {}
;   };
;   std::coroutine_handle<promise_type> h;
; };
; task greet(name who, int times) {
;   char buf[32];
;   for (int i = 0; i < times; ++i) {
;     std::snprintf(buf, sizeof buf, "hi %s %d", who.s, i);
;     std::puts(buf);
;     co_await std::suspend_always{};
;   }
; }
; int main() {
;   task t = greet(name("coro"), 3);
;   while (!t.h.done()) t.h.resume();
;   t.h.destroy();
;   std::puts("done");
;   return 0;
; }
;
; WP-350: a stack-protected (sspstrong) ramp and resume body.
;@ covers: spec/ir.md §5.5 intrinsics (coroutine bodies; spec/codegen.md §7.6)
;@ kept-off(i686): lp64-abi
;@ input: pre-opt
;@ dialect: 22
;@ link: c++
;@ stdout: hi coro 0
;@ stdout: hi coro 1
;@ stdout: hi coro 2
;@ stdout: done
; ModuleID = 'protected.cpp'
source_filename = "protected.cpp"

%struct.task = type { %"struct.std::__n4861::coroutine_handle" }
%"struct.std::__n4861::coroutine_handle" = type { ptr }
%struct.name = type { [8 x i8] }
%"struct.task::promise_type" = type { i8 }
%"struct.std::__n4861::suspend_never" = type { i8 }
%"struct.std::__n4861::suspend_always" = type { i8 }
%"struct.std::__n4861::coroutine_handle.0" = type { ptr }

$_ZN4nameC2ERKS_ = comdat any

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

$_ZN4task12promise_type13final_suspendEv = comdat any

$_ZN4nameD2Ev = comdat any

$_ZN4nameC2EPKc = comdat any

$_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEE4doneEv = comdat any

$_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEE6resumeEv = comdat any

$_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEE7destroyEv = comdat any

$_ZNSt7__n486116coroutine_handleIN4task12promise_typeEE12from_promiseERS2_ = comdat any

$_ZNSt7__n486116coroutine_handleIN4task12promise_typeEEC2Ev = comdat any

$_ZNSt7__n486116coroutine_handleIvE12from_addressEPv = comdat any

$_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEE7addressEv = comdat any

$_ZNSt7__n486116coroutine_handleIvEC2Ev = comdat any

@.str = private unnamed_addr constant [9 x i8] c"hi %s %d\00", align 1
@.str.1 = private unnamed_addr constant [5 x i8] c"coro\00", align 1
@.str.2 = private unnamed_addr constant [5 x i8] c"done\00", align 1

; Function Attrs: mustprogress noinline optnone presplitcoroutine sspstrong uwtable
define dso_local i64 @_Z5greet4namei(ptr noundef %0, i32 noundef %1) #0 personality ptr @__gxx_personality_v0 {
  %3 = alloca %struct.task, align 8
  %4 = alloca ptr, align 8
  %5 = alloca i32, align 4, !coro.outside.frame !6
  %6 = alloca %struct.name, align 1
  %7 = alloca ptr, align 8
  %8 = alloca i32, align 4
  %9 = alloca i32, align 4
  %10 = alloca %"struct.task::promise_type", align 1
  %11 = alloca %"struct.std::__n4861::suspend_never", align 1
  %12 = alloca %"struct.std::__n4861::suspend_never", align 1
  %13 = alloca [32 x i8], align 1
  %14 = alloca i32, align 4
  %15 = alloca %"struct.std::__n4861::suspend_always", align 1
  %16 = alloca %"struct.std::__n4861::suspend_always", align 1
  %17 = alloca %"struct.std::__n4861::suspend_always", align 1
  store ptr %0, ptr %4, align 8
  store i32 %1, ptr %5, align 4
  %18 = call token @llvm.coro.id(i32 16, ptr %10, ptr null, ptr null)
  %19 = call i1 @llvm.coro.alloc(token %18)
  br i1 %19, label %20, label %23

20:                                               ; preds = %2
  %21 = call i64 @llvm.coro.size.i64()
  %22 = call noalias noundef nonnull ptr @_Znwm(i64 noundef %21) #17
  br label %23

23:                                               ; preds = %20, %2
  %24 = phi ptr [ null, %2 ], [ %22, %20 ]
  %25 = call ptr @llvm.coro.begin(token %18, ptr %24)
  call void @llvm.lifetime.start.p0(ptr %6) #2
  invoke void @_ZN4nameC2ERKS_(ptr noundef nonnull align 1 dereferenceable(8) %6, ptr noundef nonnull align 1 dereferenceable(8) %0)
          to label %26 unwind label %38

26:                                               ; preds = %23
  call void @llvm.lifetime.start.p0(ptr %9) #2
  %27 = load i32, ptr %5, align 4
  store i32 %27, ptr %9, align 4
  call void @llvm.lifetime.start.p0(ptr %10) #2
  %28 = invoke i64 @_ZN4task12promise_type17get_return_objectEv(ptr noundef nonnull align 1 dereferenceable(1) %10)
          to label %29 unwind label %42

29:                                               ; preds = %26
  %30 = getelementptr inbounds nuw %struct.task, ptr %3, i32 0, i32 0
  %31 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %30, i32 0, i32 0
  %32 = inttoptr i64 %28 to ptr
  store ptr %32, ptr %31, align 8
  call void @llvm.lifetime.start.p0(ptr %11) #2
  call void @_ZN4task12promise_type15initial_suspendEv(ptr noundef nonnull align 1 dereferenceable(1) %10) #2
  %33 = call noundef i1 @_ZNKSt7__n486113suspend_never11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %11) #2
  br i1 %33, label %46, label %34

34:                                               ; preds = %29
  %35 = call token @llvm.coro.save(ptr null)
  call void @llvm.coro.await.suspend.void(ptr %11, ptr %25, ptr @_Z5greet4namei.__await_suspend_wrapper__init) #2
  %36 = call i8 @llvm.coro.suspend(token %35, i1 false)
  switch i8 %36, label %114 [
    i8 0, label %46
    i8 1, label %37
  ]

37:                                               ; preds = %34
  br label %47

38:                                               ; preds = %23
  %39 = landingpad { ptr, i32 }
          cleanup
  %40 = extractvalue { ptr, i32 } %39, 0
  store ptr %40, ptr %7, align 8
  %41 = extractvalue { ptr, i32 } %39, 1
  store i32 %41, ptr %8, align 4
  br label %121

42:                                               ; preds = %26
  %43 = landingpad { ptr, i32 }
          cleanup
  %44 = extractvalue { ptr, i32 } %43, 0
  store ptr %44, ptr %7, align 8
  %45 = extractvalue { ptr, i32 } %43, 1
  store i32 %45, ptr %8, align 4
  br label %120

46:                                               ; preds = %34, %29
  call void @_ZNKSt7__n486113suspend_never12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %11) #2
  br label %47

47:                                               ; preds = %46, %37
  %48 = phi i32 [ 0, %46 ], [ 2, %37 ]
  call void @llvm.lifetime.end.p0(ptr %11) #2
  switch i32 %48, label %106 [
    i32 0, label %49
  ]

49:                                               ; preds = %47
  call void @llvm.lifetime.start.p0(ptr %13) #2
  call void @llvm.lifetime.start.p0(ptr %14) #2
  store i32 0, ptr %14, align 4
  br label %50

50:                                               ; preds = %88, %49
  %51 = load i32, ptr %14, align 4
  %52 = load i32, ptr %9, align 4
  %53 = icmp slt i32 %51, %52
  br i1 %53, label %55, label %54

54:                                               ; preds = %50
  br label %91

55:                                               ; preds = %50
  %56 = getelementptr inbounds [32 x i8], ptr %13, i64 0, i64 0
  %57 = getelementptr inbounds nuw %struct.name, ptr %6, i32 0, i32 0
  %58 = getelementptr inbounds [8 x i8], ptr %57, i64 0, i64 0
  %59 = load i32, ptr %14, align 4
  %60 = call i32 (ptr, i64, ptr, ...) @snprintf(ptr noundef %56, i64 noundef 32, ptr noundef @.str, ptr noundef %58, i32 noundef %59) #2
  %61 = getelementptr inbounds [32 x i8], ptr %13, i64 0, i64 0
  %62 = invoke i32 @puts(ptr noundef %61)
          to label %63 unwind label %69

63:                                               ; preds = %55
  call void @llvm.lifetime.start.p0(ptr %15) #2
  %64 = call noundef i1 @_ZNKSt7__n486114suspend_always11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %15) #2
  br i1 %64, label %84, label %65

65:                                               ; preds = %63
  %66 = call token @llvm.coro.save(ptr null)
  call void @llvm.coro.await.suspend.void(ptr %15, ptr %25, ptr @_Z5greet4namei.__await_suspend_wrapper__await) #2
  %67 = call i8 @llvm.coro.suspend(token %66, i1 false)
  switch i8 %67, label %114 [
    i8 0, label %84
    i8 1, label %68
  ]

68:                                               ; preds = %65
  br label %85

69:                                               ; preds = %55
  %70 = landingpad { ptr, i32 }
          catch ptr null
  %71 = extractvalue { ptr, i32 } %70, 0
  store ptr %71, ptr %7, align 8
  %72 = extractvalue { ptr, i32 } %70, 1
  store i32 %72, ptr %8, align 4
  call void @llvm.lifetime.end.p0(ptr %14) #2
  call void @llvm.lifetime.end.p0(ptr %13) #2
  br label %73

73:                                               ; preds = %69
  %74 = load ptr, ptr %7, align 8
  %75 = call ptr @__cxa_begin_catch(ptr %74) #2
  call void @_ZN4task12promise_type19unhandled_exceptionEv(ptr noundef nonnull align 1 dereferenceable(1) %10) #2
  invoke void @__cxa_end_catch()
          to label %76 unwind label %97

76:                                               ; preds = %73
  br label %77

77:                                               ; preds = %76
  br label %78

78:                                               ; preds = %77, %96
  call void @llvm.lifetime.start.p0(ptr %16) #2
  call void @_ZN4task12promise_type13final_suspendEv(ptr noundef nonnull align 1 dereferenceable(1) %10) #2
  %79 = call noundef i1 @_ZNKSt7__n486114suspend_always11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %16) #2
  br i1 %79, label %102, label %80

80:                                               ; preds = %78
  %81 = call token @llvm.coro.save(ptr null)
  call void @llvm.coro.await.suspend.void(ptr %16, ptr %25, ptr @_Z5greet4namei.__await_suspend_wrapper__final) #2
  %82 = call i8 @llvm.coro.suspend(token %81, i1 true)
  switch i8 %82, label %114 [
    i8 0, label %102
    i8 1, label %83
  ]

83:                                               ; preds = %80
  br label %103

84:                                               ; preds = %65, %63
  call void @_ZNKSt7__n486114suspend_always12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %15) #2
  br label %85

85:                                               ; preds = %84, %68
  %86 = phi i32 [ 0, %84 ], [ 2, %68 ]
  call void @llvm.lifetime.end.p0(ptr %15) #2
  switch i32 %86, label %91 [
    i32 0, label %87
  ]

87:                                               ; preds = %85
  br label %88

88:                                               ; preds = %87
  %89 = load i32, ptr %14, align 4
  %90 = add nsw i32 %89, 1
  store i32 %90, ptr %14, align 4
  br label %50, !llvm.loop !7

91:                                               ; preds = %85, %54
  %92 = phi i32 [ %86, %85 ], [ 4, %54 ]
  call void @llvm.lifetime.end.p0(ptr %14) #2
  switch i32 %92, label %94 [
    i32 4, label %93
  ]

93:                                               ; preds = %91
  br label %94

94:                                               ; preds = %93, %91
  %95 = phi i32 [ %92, %91 ], [ 0, %93 ]
  call void @llvm.lifetime.end.p0(ptr %13) #2
  switch i32 %95, label %106 [
    i32 0, label %96
  ]

96:                                               ; preds = %94
  call void @_ZN4task12promise_type11return_voidEv(ptr noundef nonnull align 1 dereferenceable(1) %10) #2
  br label %78

97:                                               ; preds = %73
  %98 = landingpad { ptr, i32 }
          cleanup
  %99 = extractvalue { ptr, i32 } %98, 0
  store ptr %99, ptr %7, align 8
  %100 = extractvalue { ptr, i32 } %98, 1
  store i32 %100, ptr %8, align 4
  call void @llvm.coro.end(ptr null, i1 true, token none)
  %101 = call i1 @llvm.coro.is_in_ramp()
  br i1 %101, label %119, label %127

102:                                              ; preds = %80, %78
  call void @_ZNKSt7__n486114suspend_always12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %16) #2
  br label %103

103:                                              ; preds = %102, %83
  %104 = phi i32 [ 0, %102 ], [ 2, %83 ]
  call void @llvm.lifetime.end.p0(ptr %16) #2
  switch i32 %104, label %106 [
    i32 0, label %105
  ]

105:                                              ; preds = %103
  br label %106

106:                                              ; preds = %105, %103, %94, %47
  %107 = phi i32 [ %48, %47 ], [ %95, %94 ], [ %104, %103 ], [ 0, %105 ]
  call void @llvm.lifetime.end.p0(ptr %10) #2
  call void @llvm.lifetime.end.p0(ptr %9) #2
  call void @_ZN4nameD2Ev(ptr noundef nonnull align 1 dereferenceable(8) %6) #2
  call void @llvm.lifetime.end.p0(ptr %6) #2
  %108 = call ptr @llvm.coro.free(token %18, ptr %25)
  %109 = icmp ne ptr %108, null
  br i1 %109, label %110, label %112

110:                                              ; preds = %106
  %111 = call i64 @llvm.coro.size.i64()
  call void @_ZdlPvm(ptr noundef %108, i64 noundef %111) #2
  br label %112

112:                                              ; preds = %106, %110
  switch i32 %107, label %132 [
    i32 0, label %113
    i32 2, label %114
  ]

113:                                              ; preds = %112
  br label %114

114:                                              ; preds = %113, %112, %80, %65, %34
  call void @llvm.coro.end(ptr null, i1 false, token none)
  %115 = getelementptr inbounds nuw %struct.task, ptr %3, i32 0, i32 0
  %116 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %115, i32 0, i32 0
  %117 = load ptr, ptr %116, align 8
  %118 = ptrtoint ptr %117 to i64
  ret i64 %118

119:                                              ; preds = %97
  br label %120

120:                                              ; preds = %119, %42
  call void @llvm.lifetime.end.p0(ptr %10) #2
  call void @llvm.lifetime.end.p0(ptr %9) #2
  call void @_ZN4nameD2Ev(ptr noundef nonnull align 1 dereferenceable(8) %6) #2
  br label %121

121:                                              ; preds = %120, %38
  call void @llvm.lifetime.end.p0(ptr %6) #2
  %122 = call ptr @llvm.coro.free(token %18, ptr %25)
  %123 = icmp ne ptr %122, null
  br i1 %123, label %124, label %126

124:                                              ; preds = %121
  %125 = call i64 @llvm.coro.size.i64()
  call void @_ZdlPvm(ptr noundef %122, i64 noundef %125) #2
  br label %126

126:                                              ; preds = %121, %124
  br label %127

127:                                              ; preds = %126, %97
  %128 = load ptr, ptr %7, align 8
  %129 = load i32, ptr %8, align 4
  %130 = insertvalue { ptr, i32 } poison, ptr %128, 0
  %131 = insertvalue { ptr, i32 } %130, i32 %129, 1
  resume { ptr, i32 } %131

132:                                              ; preds = %112
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

; Function Attrs: mustprogress noinline nounwind optnone sspstrong uwtable
define linkonce_odr dso_local void @_ZN4nameC2ERKS_(ptr noundef nonnull align 1 dereferenceable(8) %0, ptr noundef nonnull align 1 dereferenceable(8) %1) unnamed_addr #6 comdat {
  %3 = alloca ptr, align 8
  %4 = alloca ptr, align 8
  store ptr %0, ptr %3, align 8
  store ptr %1, ptr %4, align 8
  %5 = load ptr, ptr %3, align 8
  %6 = getelementptr inbounds nuw %struct.name, ptr %5, i32 0, i32 0
  %7 = getelementptr inbounds [8 x i8], ptr %6, i64 0, i64 0
  %8 = load ptr, ptr %4, align 8, !nonnull !6
  %9 = getelementptr inbounds nuw %struct.name, ptr %8, i32 0, i32 0
  %10 = getelementptr inbounds [8 x i8], ptr %9, i64 0, i64 0
  call void @llvm.memcpy.p0.p0.i64(ptr align 1 %7, ptr align 1 %10, i64 8, i1 false)
  ret void
}

declare i32 @__gxx_personality_v0(...)

; Function Attrs: mustprogress noinline optnone sspstrong uwtable
define linkonce_odr dso_local i64 @_ZN4task12promise_type17get_return_objectEv(ptr noundef nonnull align 1 dereferenceable(1) %0) #7 comdat {
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

; Function Attrs: mustprogress noinline nounwind optnone sspstrong uwtable
define linkonce_odr dso_local void @_ZN4task12promise_type15initial_suspendEv(ptr noundef nonnull align 1 dereferenceable(1) %0) #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  ret void
}

; Function Attrs: mustprogress noinline nounwind optnone sspstrong uwtable
define linkonce_odr dso_local noundef i1 @_ZNKSt7__n486113suspend_never11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %0) #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  ret i1 true
}

; Function Attrs: nomerge nounwind
declare token @llvm.coro.save(ptr) #8

; Function Attrs: alwaysinline mustprogress
define internal void @_Z5greet4namei.__await_suspend_wrapper__init(ptr noundef nonnull %0, ptr noundef %1) #9 {
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
  call void @_ZNKSt7__n486113suspend_never13await_suspendENS_16coroutine_handleIvEE(ptr noundef nonnull align 1 dereferenceable(1) %7, ptr %16) #2
  ret void
}

; Function Attrs: mustprogress noinline nounwind optnone sspstrong uwtable
define linkonce_odr dso_local void @_ZNKSt7__n486113suspend_never13await_suspendENS_16coroutine_handleIvEE(ptr noundef nonnull align 1 dereferenceable(1) %0, ptr %1) #6 comdat {
  %3 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %4 = alloca ptr, align 8
  %5 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %3, i32 0, i32 0
  store ptr %1, ptr %5, align 8
  store ptr %0, ptr %4, align 8
  %6 = load ptr, ptr %4, align 8
  ret void
}

; Function Attrs: mustprogress noinline nounwind optnone sspstrong uwtable
define linkonce_odr dso_local i64 @_ZNSt7__n486116coroutine_handleIN4task12promise_typeEE12from_addressEPv(ptr noundef %0) #6 comdat {
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

; Function Attrs: mustprogress noinline nounwind optnone sspstrong uwtable
define linkonce_odr dso_local i64 @_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEEcvNS0_IvEEEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #6 comdat {
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

; Function Attrs: mustprogress noinline nounwind optnone sspstrong uwtable
define linkonce_odr dso_local void @_ZNKSt7__n486113suspend_never12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %0) #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  ret void
}

; Function Attrs: nocallback nofree nosync nounwind willreturn memory(argmem: readwrite)
declare void @llvm.lifetime.end.p0(ptr captures(none)) #5

; Function Attrs: nounwind
declare i32 @snprintf(ptr noundef, i64 noundef, ptr noundef, ...) #10

declare i32 @puts(ptr noundef) #11

; Function Attrs: mustprogress noinline nounwind optnone sspstrong uwtable
define linkonce_odr dso_local noundef i1 @_ZNKSt7__n486114suspend_always11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %0) #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  ret i1 false
}

; Function Attrs: alwaysinline mustprogress
define internal void @_Z5greet4namei.__await_suspend_wrapper__await(ptr noundef nonnull %0, ptr noundef %1) #9 {
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

; Function Attrs: mustprogress noinline nounwind optnone sspstrong uwtable
define linkonce_odr dso_local void @_ZNKSt7__n486114suspend_always13await_suspendENS_16coroutine_handleIvEE(ptr noundef nonnull align 1 dereferenceable(1) %0, ptr %1) #6 comdat {
  %3 = alloca %"struct.std::__n4861::coroutine_handle.0", align 8
  %4 = alloca ptr, align 8
  %5 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %3, i32 0, i32 0
  store ptr %1, ptr %5, align 8
  store ptr %0, ptr %4, align 8
  %6 = load ptr, ptr %4, align 8
  ret void
}

; Function Attrs: mustprogress noinline nounwind optnone sspstrong uwtable
define linkonce_odr dso_local void @_ZNKSt7__n486114suspend_always12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %0) #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  ret void
}

; Function Attrs: mustprogress noinline nounwind optnone sspstrong uwtable
define linkonce_odr dso_local void @_ZN4task12promise_type11return_voidEv(ptr noundef nonnull align 1 dereferenceable(1) %0) #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  ret void
}

declare ptr @__cxa_begin_catch(ptr)

; Function Attrs: mustprogress noinline nounwind optnone sspstrong uwtable
define linkonce_odr dso_local void @_ZN4task12promise_type19unhandled_exceptionEv(ptr noundef nonnull align 1 dereferenceable(1) %0) #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  ret void
}

declare void @__cxa_end_catch()

; Function Attrs: mustprogress noinline nounwind optnone sspstrong uwtable
define linkonce_odr dso_local void @_ZN4task12promise_type13final_suspendEv(ptr noundef nonnull align 1 dereferenceable(1) %0) #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  ret void
}

; Function Attrs: alwaysinline mustprogress
define internal void @_Z5greet4namei.__await_suspend_wrapper__final(ptr noundef nonnull %0, ptr noundef %1) #9 {
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

; Function Attrs: mustprogress noinline nounwind optnone sspstrong uwtable
define linkonce_odr dso_local void @_ZN4nameD2Ev(ptr noundef nonnull align 1 dereferenceable(8) %0) unnamed_addr #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  ret void
}

; Function Attrs: nobuiltin nounwind
declare void @_ZdlPvm(ptr noundef, i64 noundef) #12

; Function Attrs: nounwind memory(argmem: read)
declare ptr @llvm.coro.free(token, ptr readonly captures(none)) #13

; Function Attrs: mustprogress noinline norecurse optnone sspstrong uwtable
define dso_local noundef i32 @main() #14 personality ptr @__gxx_personality_v0 {
  %1 = alloca i32, align 4
  %2 = alloca %struct.task, align 8
  %3 = alloca %struct.name, align 1
  %4 = alloca ptr, align 8
  %5 = alloca i32, align 4
  store i32 0, ptr %1, align 4
  call void @_ZN4nameC2EPKc(ptr noundef nonnull align 1 dereferenceable(8) %3, ptr noundef @.str.1)
  %6 = invoke i64 @_Z5greet4namei(ptr noundef %3, i32 noundef 3)
          to label %7 unwind label %17

7:                                                ; preds = %0
  %8 = getelementptr inbounds nuw %struct.task, ptr %2, i32 0, i32 0
  %9 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %8, i32 0, i32 0
  %10 = inttoptr i64 %6 to ptr
  store ptr %10, ptr %9, align 8
  call void @_ZN4nameD2Ev(ptr noundef nonnull align 1 dereferenceable(8) %3) #2
  br label %11

11:                                               ; preds = %15, %7
  %12 = getelementptr inbounds nuw %struct.task, ptr %2, i32 0, i32 0
  %13 = call noundef i1 @_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEE4doneEv(ptr noundef nonnull align 8 dereferenceable(8) %12) #2
  %14 = xor i1 %13, true
  br i1 %14, label %15, label %21

15:                                               ; preds = %11
  %16 = getelementptr inbounds nuw %struct.task, ptr %2, i32 0, i32 0
  call void @_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEE6resumeEv(ptr noundef nonnull align 8 dereferenceable(8) %16)
  br label %11, !llvm.loop !9

17:                                               ; preds = %0
  %18 = landingpad { ptr, i32 }
          cleanup
  %19 = extractvalue { ptr, i32 } %18, 0
  store ptr %19, ptr %4, align 8
  %20 = extractvalue { ptr, i32 } %18, 1
  store i32 %20, ptr %5, align 4
  call void @_ZN4nameD2Ev(ptr noundef nonnull align 1 dereferenceable(8) %3) #2
  br label %24

21:                                               ; preds = %11
  %22 = getelementptr inbounds nuw %struct.task, ptr %2, i32 0, i32 0
  call void @_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEE7destroyEv(ptr noundef nonnull align 8 dereferenceable(8) %22)
  %23 = call i32 @puts(ptr noundef @.str.2)
  ret i32 0

24:                                               ; preds = %17
  %25 = load ptr, ptr %4, align 8
  %26 = load i32, ptr %5, align 4
  %27 = insertvalue { ptr, i32 } poison, ptr %25, 0
  %28 = insertvalue { ptr, i32 } %27, i32 %26, 1
  resume { ptr, i32 } %28
}

; Function Attrs: mustprogress noinline nounwind optnone sspstrong uwtable
define linkonce_odr dso_local void @_ZN4nameC2EPKc(ptr noundef nonnull align 1 dereferenceable(8) %0, ptr noundef %1) unnamed_addr #6 comdat {
  %3 = alloca ptr, align 8
  %4 = alloca ptr, align 8
  store ptr %0, ptr %3, align 8
  store ptr %1, ptr %4, align 8
  %5 = load ptr, ptr %3, align 8
  %6 = getelementptr inbounds nuw %struct.name, ptr %5, i32 0, i32 0
  %7 = getelementptr inbounds [8 x i8], ptr %6, i64 0, i64 0
  %8 = load ptr, ptr %4, align 8
  %9 = call ptr @strncpy(ptr noundef %7, ptr noundef %8, i64 noundef 7) #2
  %10 = getelementptr inbounds nuw %struct.name, ptr %5, i32 0, i32 0
  %11 = getelementptr inbounds [8 x i8], ptr %10, i64 0, i64 7
  store i8 0, ptr %11, align 1
  ret void
}

; Function Attrs: mustprogress noinline nounwind optnone sspstrong uwtable
define linkonce_odr dso_local noundef i1 @_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEE4doneEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %3, i32 0, i32 0
  %5 = load ptr, ptr %4, align 8
  %6 = call i1 @llvm.coro.done(ptr %5)
  ret i1 %6
}

; Function Attrs: mustprogress noinline optnone sspstrong uwtable
define linkonce_odr dso_local void @_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEE6resumeEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #7 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %3, i32 0, i32 0
  %5 = load ptr, ptr %4, align 8
  call void @llvm.coro.resume(ptr %5)
  ret void
}

; Function Attrs: mustprogress noinline optnone sspstrong uwtable
define linkonce_odr dso_local void @_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEE7destroyEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #7 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %3, i32 0, i32 0
  %5 = load ptr, ptr %4, align 8
  call void @llvm.coro.destroy(ptr %5)
  ret void
}

; Function Attrs: nocallback nofree nounwind willreturn memory(argmem: readwrite)
declare void @llvm.memcpy.p0.p0.i64(ptr noalias writeonly captures(none), ptr noalias readonly captures(none), i64, i1 immarg) #15

; Function Attrs: mustprogress noinline nounwind optnone sspstrong uwtable
define linkonce_odr dso_local i64 @_ZNSt7__n486116coroutine_handleIN4task12promise_typeEE12from_promiseERS2_(ptr noundef nonnull align 1 dereferenceable(1) %0) #6 comdat {
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

; Function Attrs: mustprogress noinline nounwind optnone sspstrong uwtable
define linkonce_odr dso_local void @_ZNSt7__n486116coroutine_handleIN4task12promise_typeEEC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %0) unnamed_addr #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %3, i32 0, i32 0
  store ptr null, ptr %4, align 8
  ret void
}

; Function Attrs: nounwind memory(none)
declare ptr @llvm.coro.promise(ptr captures(none), i32, i1) #4

; Function Attrs: mustprogress noinline nounwind optnone sspstrong uwtable
define linkonce_odr dso_local i64 @_ZNSt7__n486116coroutine_handleIvE12from_addressEPv(ptr noundef %0) #6 comdat {
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

; Function Attrs: mustprogress noinline nounwind optnone sspstrong uwtable
define linkonce_odr dso_local noundef ptr @_ZNKSt7__n486116coroutine_handleIN4task12promise_typeEE7addressEv(ptr noundef nonnull align 8 dereferenceable(8) %0) #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %3, i32 0, i32 0
  %5 = load ptr, ptr %4, align 8
  ret ptr %5
}

; Function Attrs: mustprogress noinline nounwind optnone sspstrong uwtable
define linkonce_odr dso_local void @_ZNSt7__n486116coroutine_handleIvEC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %0) unnamed_addr #6 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle.0", ptr %3, i32 0, i32 0
  store ptr null, ptr %4, align 8
  ret void
}

; Function Attrs: nounwind
declare ptr @strncpy(ptr noundef, ptr noundef, i64 noundef) #10

; Function Attrs: nounwind memory(argmem: readwrite)
declare i1 @llvm.coro.done(ptr readonly captures(none)) #16

declare void @llvm.coro.resume(ptr)

declare void @llvm.coro.destroy(ptr)

attributes #0 = { mustprogress noinline optnone presplitcoroutine sspstrong uwtable "frame-pointer"="non-leaf-no-reserve" "no-trapping-math"="true" "stack-protector-buffer-size"="8" }
attributes #1 = { nocallback nofree nosync nounwind willreturn memory(argmem: read) }
attributes #2 = { nounwind }
attributes #3 = { nobuiltin allocsize(0) "frame-pointer"="non-leaf-no-reserve" "no-trapping-math"="true" "stack-protector-buffer-size"="8" }
attributes #4 = { nounwind memory(none) }
attributes #5 = { nocallback nofree nosync nounwind willreturn memory(argmem: readwrite) }
attributes #6 = { mustprogress noinline nounwind optnone sspstrong uwtable "frame-pointer"="non-leaf-no-reserve" "no-trapping-math"="true" "stack-protector-buffer-size"="8" }
attributes #7 = { mustprogress noinline optnone sspstrong uwtable "frame-pointer"="non-leaf-no-reserve" "no-trapping-math"="true" "stack-protector-buffer-size"="8" }
attributes #8 = { nomerge nounwind }
attributes #9 = { alwaysinline mustprogress "sample-profile-suffix-elision-policy"="selected" }
attributes #10 = { nounwind "frame-pointer"="non-leaf-no-reserve" "no-trapping-math"="true" "stack-protector-buffer-size"="8" }
attributes #11 = { "frame-pointer"="non-leaf-no-reserve" "no-trapping-math"="true" "stack-protector-buffer-size"="8" }
attributes #12 = { nobuiltin nounwind "frame-pointer"="non-leaf-no-reserve" "no-trapping-math"="true" "stack-protector-buffer-size"="8" }
attributes #13 = { nounwind memory(argmem: read) }
attributes #14 = { mustprogress noinline norecurse optnone sspstrong uwtable "frame-pointer"="non-leaf-no-reserve" "no-trapping-math"="true" "stack-protector-buffer-size"="8" }
attributes #15 = { nocallback nofree nounwind willreturn memory(argmem: readwrite) }
attributes #16 = { nounwind memory(argmem: readwrite) }
attributes #17 = { allocsize(0) }

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
!9 = distinct !{!9, !8}
