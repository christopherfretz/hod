; ModuleID = 'escaped-local-22.ll'
source_filename = "docs/design/0005-data/escaped-local.cpp"
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i8:8:32-i16:16:32-i64:64-i128:128-n32:64-S128-Fn32"
target triple = "aarch64-unknown-linux-gnu"

%struct.registry = type { ptr }
%struct.task = type { %"struct.std::__n4861::coroutine_handle" }
%"struct.std::__n4861::coroutine_handle" = type { ptr }
%"struct.task::promise_type" = type { i8 }
%"struct.std::__n4861::suspend_never" = type { i8 }
%"struct.std::__n4861::suspend_always" = type { i8 }
%struct.T = type { i32 }

@reg = external dso_local global %struct.registry, align 8

; Function Attrs: mustprogress noinline optnone uwtable
declare dso_local void @_Z3useii(i32 noundef, i32 noundef) #0

; Function Attrs: mustprogress noinline optnone presplitcoroutine uwtable
define dso_local i64 @_Z7pointerv() #1 personality ptr @__gxx_personality_v0 {
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
  %22 = call noalias noundef nonnull ptr @_Znwm(i64 noundef %21) #14
  br label %23

23:                                               ; preds = %20, %0
  %24 = phi ptr [ null, %0 ], [ %22, %20 ]
  %25 = call ptr @llvm.coro.begin(token %18, ptr %24)
  call void @llvm.lifetime.start.p0(ptr %2) #3
  call void @llvm.memset.p0.i64(ptr align 1 %2, i8 0, i64 1, i1 false), !annotation !6
  %26 = invoke i64 @_ZN4task12promise_type17get_return_objectEv(ptr noundef nonnull align 1 dereferenceable(1) %2)
          to label %27 unwind label %36

27:                                               ; preds = %23
  %28 = getelementptr inbounds nuw %struct.task, ptr %1, i32 0, i32 0
  %29 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %28, i32 0, i32 0
  %30 = inttoptr i64 %26 to ptr
  store ptr %30, ptr %29, align 8
  call void @llvm.lifetime.start.p0(ptr %5) #3
  call void @_ZN4task12promise_type15initial_suspendEv(ptr noundef nonnull align 1 dereferenceable(1) %2) #3
  %31 = call noundef i1 @_ZNKSt7__n486113suspend_never11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %5) #3
  br i1 %31, label %40, label %32

32:                                               ; preds = %27
  %33 = call token @llvm.coro.save(ptr null)
  call void @llvm.coro.await.suspend.void(ptr %5, ptr %25, ptr @_Z7pointerv.__await_suspend_wrapper__init) #3
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
  call void @_ZNKSt7__n486113suspend_never12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %5) #3
  br label %41

41:                                               ; preds = %40, %35
  %42 = phi i32 [ 0, %40 ], [ 2, %35 ]
  call void @llvm.lifetime.end.p0(ptr %5) #3
  switch i32 %42, label %115 [
    i32 0, label %43
  ]

43:                                               ; preds = %41
  call void @llvm.lifetime.start.p0(ptr %7) #3
  store i32 0, ptr %7, align 4
  call void @llvm.lifetime.start.p0(ptr %8) #3
  store i32 7, ptr %8, align 4
  call void @llvm.lifetime.start.p0(ptr %9) #3
  store ptr %7, ptr %9, align 8
  call void @llvm.lifetime.start.p0(ptr %10) #3
  %44 = call noundef i1 @_ZNKSt7__n486114suspend_always11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %10) #3
  br i1 %44, label %49, label %45

45:                                               ; preds = %43
  %46 = call token @llvm.coro.save(ptr null)
  call void @llvm.coro.await.suspend.void(ptr %10, ptr %25, ptr @_Z7pointerv.__await_suspend_wrapper__await) #3
  %47 = call i8 @llvm.coro.suspend(token %46, i1 false)
  switch i8 %47, label %123 [
    i8 0, label %49
    i8 1, label %48
  ]

48:                                               ; preds = %45
  br label %50

49:                                               ; preds = %45, %43
  call void @_ZNKSt7__n486114suspend_always12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %10) #3
  br label %50

50:                                               ; preds = %49, %48
  %51 = phi i32 [ 0, %49 ], [ 2, %48 ]
  call void @llvm.lifetime.end.p0(ptr %10) #3
  switch i32 %51, label %78 [
    i32 0, label %52
  ]

52:                                               ; preds = %50
  %53 = load ptr, ptr %9, align 8
  store i32 1, ptr %53, align 4
  call void @llvm.lifetime.start.p0(ptr %11) #3
  call void @llvm.memset.p0.i64(ptr align 4 %11, i8 0, i64 256, i1 false), !annotation !6
  call void @llvm.lifetime.start.p0(ptr %12) #3
  store ptr null, ptr %12, align 8, !annotation !6
  store ptr %11, ptr %12, align 8
  call void @llvm.lifetime.start.p0(ptr %13) #3
  %54 = load ptr, ptr %12, align 8, !nonnull !7, !align !8
  %55 = getelementptr inbounds [64 x i32], ptr %54, i64 0, i64 0
  store ptr %55, ptr %13, align 8
  call void @llvm.lifetime.start.p0(ptr %14) #3
  %56 = load ptr, ptr %12, align 8, !nonnull !7, !align !8
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
  call void @llvm.lifetime.end.p0(ptr %14) #3
  call void @llvm.lifetime.end.p0(ptr %13) #3
  call void @llvm.lifetime.end.p0(ptr %12) #3
  br label %70

64:                                               ; preds = %59
  call void @llvm.lifetime.start.p0(ptr %15) #3
  store ptr null, ptr %15, align 8, !annotation !6
  %65 = load ptr, ptr %13, align 8
  store ptr %65, ptr %15, align 8
  %66 = load ptr, ptr %15, align 8, !nonnull !7, !align !8
  store i32 -1, ptr %66, align 4
  call void @llvm.lifetime.end.p0(ptr %15) #3
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
  call void @llvm.lifetime.end.p0(ptr %11) #3
  br label %78

78:                                               ; preds = %77, %50
  %79 = phi i32 [ %51, %50 ], [ 0, %77 ]
  call void @llvm.lifetime.end.p0(ptr %9) #3
  call void @llvm.lifetime.end.p0(ptr %8) #3
  call void @llvm.lifetime.end.p0(ptr %7) #3
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
  call void @llvm.lifetime.end.p0(ptr %11) #3
  call void @llvm.lifetime.end.p0(ptr %9) #3
  call void @llvm.lifetime.end.p0(ptr %8) #3
  call void @llvm.lifetime.end.p0(ptr %7) #3
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
  %92 = call ptr @__cxa_begin_catch(ptr %91) #3
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
  call void @llvm.lifetime.start.p0(ptr %16) #3
  call void @_ZN4task12promise_type13final_suspendEv(ptr noundef nonnull align 1 dereferenceable(1) %2) #3
  %97 = call noundef i1 @_ZNKSt7__n486114suspend_always11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %16) #3
  br i1 %97, label %111, label %98

98:                                               ; preds = %96
  %99 = call token @llvm.coro.save(ptr null)
  call void @llvm.coro.await.suspend.void(ptr %16, ptr %25, ptr @_Z7pointerv.__await_suspend_wrapper__final) #3
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
  call void @_ZNKSt7__n486114suspend_always12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %16) #3
  br label %112

112:                                              ; preds = %111, %101
  %113 = phi i32 [ 0, %111 ], [ 2, %101 ]
  call void @llvm.lifetime.end.p0(ptr %16) #3
  switch i32 %113, label %115 [
    i32 0, label %114
  ]

114:                                              ; preds = %112
  br label %115

115:                                              ; preds = %114, %112, %78, %41
  %116 = phi i32 [ %42, %41 ], [ %79, %78 ], [ %113, %112 ], [ 0, %114 ]
  call void @llvm.lifetime.end.p0(ptr %2) #3
  %117 = call ptr @llvm.coro.free(token %18, ptr %25)
  %118 = icmp ne ptr %117, null
  br i1 %118, label %119, label %121

119:                                              ; preds = %115
  %120 = call i64 @llvm.coro.size.i64()
  call void @_ZdlPvm(ptr noundef %117, i64 noundef %120) #3
  br label %121

121:                                              ; preds = %119, %115
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
  call void @llvm.lifetime.end.p0(ptr %2) #3
  %132 = call ptr @llvm.coro.free(token %18, ptr %25)
  %133 = icmp ne ptr %132, null
  br i1 %133, label %134, label %136

134:                                              ; preds = %131
  %135 = call i64 @llvm.coro.size.i64()
  call void @_ZdlPvm(ptr noundef %132, i64 noundef %135) #3
  br label %136

136:                                              ; preds = %134, %131
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
  call void @__clang_call_terminate(ptr %144) #15
  unreachable

145:                                              ; preds = %121
  unreachable
}

; Function Attrs: nocallback nofree nosync nounwind willreturn memory(argmem: read)
declare token @llvm.coro.id(i32, ptr readnone, ptr readonly captures(none), ptr) #2

; Function Attrs: nounwind
declare i1 @llvm.coro.alloc(token) #3

; Function Attrs: nobuiltin allocsize(0)
declare noundef nonnull ptr @_Znwm(i64 noundef) #4

; Function Attrs: nounwind memory(none)
declare i64 @llvm.coro.size.i64() #5

; Function Attrs: nounwind
declare ptr @llvm.coro.begin(token, ptr writeonly) #3

; Function Attrs: nocallback nofree nosync nounwind willreturn memory(argmem: readwrite)
declare void @llvm.lifetime.start.p0(ptr captures(none)) #6

; Function Attrs: nocallback nofree nounwind willreturn memory(argmem: write)
declare void @llvm.memset.p0.i64(ptr writeonly captures(none), i8, i64, i1 immarg) #7

; Function Attrs: mustprogress noinline optnone uwtable
declare dso_local i64 @_ZN4task12promise_type17get_return_objectEv(ptr noundef nonnull align 1 dereferenceable(1)) #0

declare i32 @__gxx_personality_v0(...)

; Function Attrs: mustprogress noinline nounwind optnone uwtable
declare dso_local void @_ZN4task12promise_type15initial_suspendEv(ptr noundef nonnull align 1 dereferenceable(1)) #8

; Function Attrs: mustprogress noinline nounwind optnone uwtable
declare dso_local noundef i1 @_ZNKSt7__n486113suspend_never11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1)) #8

; Function Attrs: nomerge nounwind
declare token @llvm.coro.save(ptr) #9

; Function Attrs: alwaysinline mustprogress
declare hidden void @_Z7pointerv.__await_suspend_wrapper__init(ptr noundef nonnull, ptr noundef) #10

declare void @llvm.coro.await.suspend.void(ptr, ptr, ptr)

; Function Attrs: nounwind
declare i8 @llvm.coro.suspend(token, i1) #3

; Function Attrs: mustprogress noinline nounwind optnone uwtable
declare dso_local void @_ZNKSt7__n486113suspend_never12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1)) #8

; Function Attrs: nocallback nofree nosync nounwind willreturn memory(argmem: readwrite)
declare void @llvm.lifetime.end.p0(ptr captures(none)) #6

; Function Attrs: mustprogress noinline nounwind optnone uwtable
declare dso_local noundef i1 @_ZNKSt7__n486114suspend_always11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1)) #8

; Function Attrs: alwaysinline mustprogress
declare hidden void @_Z7pointerv.__await_suspend_wrapper__await(ptr noundef nonnull, ptr noundef) #10

; Function Attrs: mustprogress noinline nounwind optnone uwtable
declare dso_local void @_ZNKSt7__n486114suspend_always12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1)) #8

; Function Attrs: mustprogress noinline nounwind optnone uwtable
declare dso_local void @_ZN4task12promise_type11return_voidEv(ptr noundef nonnull align 1 dereferenceable(1)) #8

declare ptr @__cxa_begin_catch(ptr)

; Function Attrs: mustprogress noinline nounwind optnone uwtable
declare dso_local void @_ZN4task12promise_type19unhandled_exceptionEv(ptr noundef nonnull align 1 dereferenceable(1)) #8

declare void @__cxa_end_catch()

; Function Attrs: noinline noreturn nounwind uwtable
declare hidden void @__clang_call_terminate(ptr noundef) #11

; Function Attrs: mustprogress noinline nounwind optnone uwtable
declare dso_local void @_ZN4task12promise_type13final_suspendEv(ptr noundef nonnull align 1 dereferenceable(1)) #8

; Function Attrs: alwaysinline mustprogress
declare hidden void @_Z7pointerv.__await_suspend_wrapper__final(ptr noundef nonnull, ptr noundef) #10

; Function Attrs: nounwind
declare void @llvm.coro.end(ptr, i1, token) #3

; Function Attrs: nounwind memory(none)
declare i1 @llvm.coro.is_in_ramp() #5

; Function Attrs: nobuiltin nounwind
declare void @_ZdlPvm(ptr noundef, i64 noundef) #12

; Function Attrs: nounwind memory(argmem: read)
declare ptr @llvm.coro.free(token, ptr readonly captures(none)) #13

; Function Attrs: mustprogress noinline optnone uwtable
declare dso_local void @_ZN8registry4pokeEv(ptr noundef nonnull align 8 dereferenceable(8)) #0

; Function Attrs: mustprogress noinline optnone presplitcoroutine uwtable
define dso_local i64 @_Z10registeredv() #1 personality ptr @__gxx_personality_v0 {
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
  %20 = call noalias noundef nonnull ptr @_Znwm(i64 noundef %19) #14
  br label %21

21:                                               ; preds = %18, %0
  %22 = phi ptr [ null, %0 ], [ %20, %18 ]
  %23 = call ptr @llvm.coro.begin(token %16, ptr %22)
  call void @llvm.lifetime.start.p0(ptr %2) #3
  call void @llvm.memset.p0.i64(ptr align 1 %2, i8 0, i64 1, i1 false), !annotation !6
  %24 = invoke i64 @_ZN4task12promise_type17get_return_objectEv(ptr noundef nonnull align 1 dereferenceable(1) %2)
          to label %25 unwind label %34

25:                                               ; preds = %21
  %26 = getelementptr inbounds nuw %struct.task, ptr %1, i32 0, i32 0
  %27 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %26, i32 0, i32 0
  %28 = inttoptr i64 %24 to ptr
  store ptr %28, ptr %27, align 8
  call void @llvm.lifetime.start.p0(ptr %5) #3
  call void @_ZN4task12promise_type15initial_suspendEv(ptr noundef nonnull align 1 dereferenceable(1) %2) #3
  %29 = call noundef i1 @_ZNKSt7__n486113suspend_never11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %5) #3
  br i1 %29, label %38, label %30

30:                                               ; preds = %25
  %31 = call token @llvm.coro.save(ptr null)
  call void @llvm.coro.await.suspend.void(ptr %5, ptr %23, ptr @_Z10registeredv.__await_suspend_wrapper__init) #3
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
  call void @_ZNKSt7__n486113suspend_never12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %5) #3
  br label %39

39:                                               ; preds = %38, %33
  %40 = phi i32 [ 0, %38 ], [ 2, %33 ]
  call void @llvm.lifetime.end.p0(ptr %5) #3
  switch i32 %40, label %113 [
    i32 0, label %41
  ]

41:                                               ; preds = %39
  call void @llvm.lifetime.start.p0(ptr %7) #3
  call void @llvm.memset.p0.i64(ptr align 4 %7, i8 0, i64 4, i1 false), !annotation !6
  call void @_ZN1TC2Ev(ptr noundef nonnull align 4 dereferenceable(4) %7) #3
  invoke void @_ZN8registry3addEP1T(ptr noundef nonnull align 8 dereferenceable(8) @reg, ptr noundef %7)
          to label %42 unwind label %48

42:                                               ; preds = %41
  call void @llvm.lifetime.start.p0(ptr %8) #3
  %43 = call noundef i1 @_ZNKSt7__n486114suspend_always11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %8) #3
  br i1 %43, label %52, label %44

44:                                               ; preds = %42
  %45 = call token @llvm.coro.save(ptr null)
  call void @llvm.coro.await.suspend.void(ptr %8, ptr %23, ptr @_Z10registeredv.__await_suspend_wrapper__await) #3
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
  call void @_ZNKSt7__n486114suspend_always12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %8) #3
  br label %53

53:                                               ; preds = %52, %47
  %54 = phi i32 [ 0, %52 ], [ 2, %47 ]
  call void @llvm.lifetime.end.p0(ptr %8) #3
  switch i32 %54, label %75 [
    i32 0, label %55
  ]

55:                                               ; preds = %53
  call void @llvm.lifetime.start.p0(ptr %9) #3
  call void @llvm.memset.p0.i64(ptr align 8 %9, i8 0, i64 512, i1 false), !annotation !6
  call void @llvm.lifetime.start.p0(ptr %10) #3
  store ptr null, ptr %10, align 8, !annotation !6
  store ptr %9, ptr %10, align 8
  call void @llvm.lifetime.start.p0(ptr %11) #3
  %56 = load ptr, ptr %10, align 8, !nonnull !7, !align !9
  %57 = getelementptr inbounds [64 x i64], ptr %56, i64 0, i64 0
  store ptr %57, ptr %11, align 8
  call void @llvm.lifetime.start.p0(ptr %12) #3
  %58 = load ptr, ptr %10, align 8, !nonnull !7, !align !9
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
  call void @llvm.lifetime.end.p0(ptr %12) #3
  call void @llvm.lifetime.end.p0(ptr %11) #3
  call void @llvm.lifetime.end.p0(ptr %10) #3
  br label %72

66:                                               ; preds = %61
  call void @llvm.lifetime.start.p0(ptr %13) #3
  store ptr null, ptr %13, align 8, !annotation !6
  %67 = load ptr, ptr %11, align 8
  store ptr %67, ptr %13, align 8
  %68 = load ptr, ptr %13, align 8, !nonnull !7, !align !9
  store i64 1515870810, ptr %68, align 8
  call void @llvm.lifetime.end.p0(ptr %13) #3
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
  call void @llvm.lifetime.end.p0(ptr %9) #3
  br label %75

75:                                               ; preds = %73, %53
  %76 = phi i32 [ %54, %53 ], [ 0, %73 ]
  call void @llvm.lifetime.end.p0(ptr %7) #3
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
  call void @llvm.lifetime.end.p0(ptr %9) #3
  br label %83

83:                                               ; preds = %79, %48
  call void @llvm.lifetime.end.p0(ptr %7) #3
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
  %90 = call ptr @__cxa_begin_catch(ptr %89) #3
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
  call void @llvm.lifetime.start.p0(ptr %14) #3
  call void @_ZN4task12promise_type13final_suspendEv(ptr noundef nonnull align 1 dereferenceable(1) %2) #3
  %95 = call noundef i1 @_ZNKSt7__n486114suspend_always11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %14) #3
  br i1 %95, label %109, label %96

96:                                               ; preds = %94
  %97 = call token @llvm.coro.save(ptr null)
  call void @llvm.coro.await.suspend.void(ptr %14, ptr %23, ptr @_Z10registeredv.__await_suspend_wrapper__final) #3
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
  call void @_ZNKSt7__n486114suspend_always12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %14) #3
  br label %110

110:                                              ; preds = %109, %99
  %111 = phi i32 [ 0, %109 ], [ 2, %99 ]
  call void @llvm.lifetime.end.p0(ptr %14) #3
  switch i32 %111, label %113 [
    i32 0, label %112
  ]

112:                                              ; preds = %110
  br label %113

113:                                              ; preds = %112, %110, %75, %39
  %114 = phi i32 [ %40, %39 ], [ %76, %75 ], [ %111, %110 ], [ 0, %112 ]
  call void @llvm.lifetime.end.p0(ptr %2) #3
  %115 = call ptr @llvm.coro.free(token %16, ptr %23)
  %116 = icmp ne ptr %115, null
  br i1 %116, label %117, label %119

117:                                              ; preds = %113
  %118 = call i64 @llvm.coro.size.i64()
  call void @_ZdlPvm(ptr noundef %115, i64 noundef %118) #3
  br label %119

119:                                              ; preds = %117, %113
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
  call void @llvm.lifetime.end.p0(ptr %2) #3
  %130 = call ptr @llvm.coro.free(token %16, ptr %23)
  %131 = icmp ne ptr %130, null
  br i1 %131, label %132, label %134

132:                                              ; preds = %129
  %133 = call i64 @llvm.coro.size.i64()
  call void @_ZdlPvm(ptr noundef %130, i64 noundef %133) #3
  br label %134

134:                                              ; preds = %132, %129
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
  call void @__clang_call_terminate(ptr %142) #15
  unreachable

143:                                              ; preds = %119
  unreachable
}

; Function Attrs: alwaysinline mustprogress
declare hidden void @_Z10registeredv.__await_suspend_wrapper__init(ptr noundef nonnull, ptr noundef) #10

; Function Attrs: mustprogress noinline nounwind optnone uwtable
declare dso_local void @_ZN1TC2Ev(ptr noundef nonnull align 4 dereferenceable(4)) unnamed_addr #8

; Function Attrs: mustprogress noinline nounwind optnone uwtable
declare dso_local void @_ZN8registry3addEP1T(ptr noundef nonnull align 8 dereferenceable(8), ptr noundef) #8

; Function Attrs: alwaysinline mustprogress
declare hidden void @_Z10registeredv.__await_suspend_wrapper__await(ptr noundef nonnull, ptr noundef) #10

; Function Attrs: alwaysinline mustprogress
declare hidden void @_Z10registeredv.__await_suspend_wrapper__final(ptr noundef nonnull, ptr noundef) #10

attributes #0 = { mustprogress noinline optnone uwtable "frame-pointer"="non-leaf-no-reserve" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #1 = { mustprogress noinline optnone presplitcoroutine uwtable "frame-pointer"="non-leaf-no-reserve" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #2 = { nocallback nofree nosync nounwind willreturn memory(argmem: read) }
attributes #3 = { nounwind }
attributes #4 = { nobuiltin allocsize(0) "frame-pointer"="non-leaf-no-reserve" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #5 = { nounwind memory(none) }
attributes #6 = { nocallback nofree nosync nounwind willreturn memory(argmem: readwrite) }
attributes #7 = { nocallback nofree nounwind willreturn memory(argmem: write) }
attributes #8 = { mustprogress noinline nounwind optnone uwtable "frame-pointer"="non-leaf-no-reserve" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #9 = { nomerge nounwind }
attributes #10 = { alwaysinline mustprogress "sample-profile-suffix-elision-policy"="selected" }
attributes #11 = { noinline noreturn nounwind uwtable "frame-pointer"="non-leaf-no-reserve" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #12 = { nobuiltin nounwind "frame-pointer"="non-leaf-no-reserve" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #13 = { nounwind memory(argmem: read) }
attributes #14 = { allocsize(0) }
attributes #15 = { noreturn nounwind }

!llvm.module.flags = !{!0, !1, !2, !3, !4}
!llvm.ident = !{!5}

!0 = !{i32 1, !"wchar_size", i32 4}
!1 = !{i32 8, !"PIC Level", i32 2}
!2 = !{i32 7, !"PIE Level", i32 2}
!3 = !{i32 7, !"uwtable", i32 2}
!4 = !{i32 7, !"frame-pointer", i32 4}
!5 = !{!"kiln version 1.0.1"}
!6 = !{!"auto-init"}
!7 = !{}
!8 = !{i64 4}
!9 = !{i64 8}
