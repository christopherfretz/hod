; ModuleID = 'escaped-local-19.ll'
source_filename = "docs/design/0005-data/escaped-local.cpp"
target datalayout = "e-m:e-i8:8:32-i16:16:32-i64:64-i128:128-n32:64-S128-Fn32"
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
  %18 = bitcast ptr %2 to ptr
  %19 = call token @llvm.coro.id(i32 16, ptr %18, ptr null, ptr null)
  %20 = call i1 @llvm.coro.alloc(token %19)
  br i1 %20, label %21, label %24

21:                                               ; preds = %0
  %22 = call i64 @llvm.coro.size.i64()
  %23 = call noalias noundef nonnull ptr @_Znwm(i64 noundef %22) #14
  br label %24

24:                                               ; preds = %21, %0
  %25 = phi ptr [ null, %0 ], [ %23, %21 ]
  %26 = call ptr @llvm.coro.begin(token %19, ptr %25)
  call void @llvm.lifetime.start.p0(i64 1, ptr %2) #3
  call void @llvm.memset.p0.i64(ptr align 1 %2, i8 0, i64 1, i1 false), !annotation !6
  %27 = invoke i64 @_ZN4task12promise_type17get_return_objectEv(ptr noundef nonnull align 1 dereferenceable(1) %2)
          to label %28 unwind label %37

28:                                               ; preds = %24
  %29 = getelementptr inbounds %struct.task, ptr %1, i32 0, i32 0
  %30 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %29, i32 0, i32 0
  %31 = inttoptr i64 %27 to ptr
  store ptr %31, ptr %30, align 8
  call void @llvm.lifetime.start.p0(i64 1, ptr %5) #3
  call void @_ZN4task12promise_type15initial_suspendEv(ptr noundef nonnull align 1 dereferenceable(1) %2) #3
  %32 = call noundef i1 @_ZNKSt7__n486113suspend_never11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %5) #3
  br i1 %32, label %41, label %33

33:                                               ; preds = %28
  %34 = call token @llvm.coro.save(ptr null)
  call void @llvm.coro.await.suspend.void(ptr %5, ptr %26, ptr @_Z7pointerv.__await_suspend_wrapper__init) #3
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
  call void @_ZNKSt7__n486113suspend_never12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %5) #3
  br label %42

42:                                               ; preds = %41, %36
  %43 = phi i32 [ 0, %41 ], [ 2, %36 ]
  call void @llvm.lifetime.end.p0(i64 1, ptr %5) #3
  switch i32 %43, label %116 [
    i32 0, label %44
  ]

44:                                               ; preds = %42
  call void @llvm.lifetime.start.p0(i64 4, ptr %7) #3
  store i32 0, ptr %7, align 4
  call void @llvm.lifetime.start.p0(i64 4, ptr %8) #3
  store i32 7, ptr %8, align 4
  call void @llvm.lifetime.start.p0(i64 8, ptr %9) #3
  store ptr %7, ptr %9, align 8
  call void @llvm.lifetime.start.p0(i64 1, ptr %10) #3
  %45 = call noundef i1 @_ZNKSt7__n486114suspend_always11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %10) #3
  br i1 %45, label %50, label %46

46:                                               ; preds = %44
  %47 = call token @llvm.coro.save(ptr null)
  call void @llvm.coro.await.suspend.void(ptr %10, ptr %26, ptr @_Z7pointerv.__await_suspend_wrapper__await) #3
  %48 = call i8 @llvm.coro.suspend(token %47, i1 false)
  switch i8 %48, label %124 [
    i8 0, label %50
    i8 1, label %49
  ]

49:                                               ; preds = %46
  br label %51

50:                                               ; preds = %46, %44
  call void @_ZNKSt7__n486114suspend_always12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %10) #3
  br label %51

51:                                               ; preds = %50, %49
  %52 = phi i32 [ 0, %50 ], [ 2, %49 ]
  call void @llvm.lifetime.end.p0(i64 1, ptr %10) #3
  switch i32 %52, label %79 [
    i32 0, label %53
  ]

53:                                               ; preds = %51
  %54 = load ptr, ptr %9, align 8
  store i32 1, ptr %54, align 4
  call void @llvm.lifetime.start.p0(i64 256, ptr %11) #3
  call void @llvm.memset.p0.i64(ptr align 4 %11, i8 0, i64 256, i1 false), !annotation !6
  call void @llvm.lifetime.start.p0(i64 8, ptr %12) #3
  store ptr null, ptr %12, align 8, !annotation !6
  store ptr %11, ptr %12, align 8
  call void @llvm.lifetime.start.p0(i64 8, ptr %13) #3
  %55 = load ptr, ptr %12, align 8
  %56 = getelementptr inbounds [64 x i32], ptr %55, i64 0, i64 0
  store ptr %56, ptr %13, align 8
  call void @llvm.lifetime.start.p0(i64 8, ptr %14) #3
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
  call void @llvm.lifetime.end.p0(i64 8, ptr %14) #3
  call void @llvm.lifetime.end.p0(i64 8, ptr %13) #3
  call void @llvm.lifetime.end.p0(i64 8, ptr %12) #3
  br label %71

65:                                               ; preds = %60
  call void @llvm.lifetime.start.p0(i64 8, ptr %15) #3
  store ptr null, ptr %15, align 8, !annotation !6
  %66 = load ptr, ptr %13, align 8
  store ptr %66, ptr %15, align 8
  %67 = load ptr, ptr %15, align 8
  store i32 -1, ptr %67, align 4
  call void @llvm.lifetime.end.p0(i64 8, ptr %15) #3
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
  call void @llvm.lifetime.end.p0(i64 256, ptr %11) #3
  br label %79

79:                                               ; preds = %78, %51
  %80 = phi i32 [ %52, %51 ], [ 0, %78 ]
  call void @llvm.lifetime.end.p0(i64 8, ptr %9) #3
  call void @llvm.lifetime.end.p0(i64 4, ptr %8) #3
  call void @llvm.lifetime.end.p0(i64 4, ptr %7) #3
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
  call void @llvm.lifetime.end.p0(i64 256, ptr %11) #3
  call void @llvm.lifetime.end.p0(i64 8, ptr %9) #3
  call void @llvm.lifetime.end.p0(i64 4, ptr %8) #3
  call void @llvm.lifetime.end.p0(i64 4, ptr %7) #3
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
  %93 = call ptr @__cxa_begin_catch(ptr %92) #3
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
  call void @llvm.lifetime.start.p0(i64 1, ptr %16) #3
  call void @_ZN4task12promise_type13final_suspendEv(ptr noundef nonnull align 1 dereferenceable(1) %2) #3
  %98 = call noundef i1 @_ZNKSt7__n486114suspend_always11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %16) #3
  br i1 %98, label %112, label %99

99:                                               ; preds = %97
  %100 = call token @llvm.coro.save(ptr null)
  call void @llvm.coro.await.suspend.void(ptr %16, ptr %26, ptr @_Z7pointerv.__await_suspend_wrapper__final) #3
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
  call void @_ZNKSt7__n486114suspend_always12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %16) #3
  br label %113

113:                                              ; preds = %112, %102
  %114 = phi i32 [ 0, %112 ], [ 2, %102 ]
  call void @llvm.lifetime.end.p0(i64 1, ptr %16) #3
  switch i32 %114, label %116 [
    i32 0, label %115
  ]

115:                                              ; preds = %113
  br label %116

116:                                              ; preds = %115, %113, %79, %42
  %117 = phi i32 [ %43, %42 ], [ %80, %79 ], [ %114, %113 ], [ 0, %115 ]
  call void @llvm.lifetime.end.p0(i64 1, ptr %2) #3
  %118 = call ptr @llvm.coro.free(token %19, ptr %26)
  %119 = icmp ne ptr %118, null
  br i1 %119, label %120, label %122

120:                                              ; preds = %116
  %121 = call i64 @llvm.coro.size.i64()
  call void @_ZdlPvm(ptr noundef %118, i64 noundef %121) #3
  br label %122

122:                                              ; preds = %120, %116
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
  call void @llvm.lifetime.end.p0(i64 1, ptr %2) #3
  %134 = call ptr @llvm.coro.free(token %19, ptr %26)
  %135 = icmp ne ptr %134, null
  br i1 %135, label %136, label %138

136:                                              ; preds = %133
  %137 = call i64 @llvm.coro.size.i64()
  call void @_ZdlPvm(ptr noundef %134, i64 noundef %137) #3
  br label %138

138:                                              ; preds = %136, %133
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
  call void @__clang_call_terminate(ptr %146) #15
  unreachable

147:                                              ; preds = %122
  unreachable
}

; Function Attrs: nocallback nofree nosync nounwind willreturn memory(argmem: read)
declare token @llvm.coro.id(i32, ptr readnone, ptr nocapture readonly, ptr) #2

; Function Attrs: nounwind
declare i1 @llvm.coro.alloc(token) #3

; Function Attrs: nobuiltin allocsize(0)
declare noundef nonnull ptr @_Znwm(i64 noundef) #4

; Function Attrs: nounwind memory(none)
declare i64 @llvm.coro.size.i64() #5

; Function Attrs: nounwind
declare ptr @llvm.coro.begin(token, ptr writeonly) #3

; Function Attrs: nocallback nofree nosync nounwind willreturn memory(argmem: readwrite)
declare void @llvm.lifetime.start.p0(i64 immarg, ptr nocapture) #6

; Function Attrs: nocallback nofree nounwind willreturn memory(argmem: write)
declare void @llvm.memset.p0.i64(ptr nocapture writeonly, i8, i64, i1 immarg) #7

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
declare void @llvm.lifetime.end.p0(i64 immarg, ptr nocapture) #6

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
declare i1 @llvm.coro.end(ptr, i1, token) #3

; Function Attrs: nobuiltin nounwind
declare void @_ZdlPvm(ptr noundef, i64 noundef) #12

; Function Attrs: nounwind memory(argmem: read)
declare ptr @llvm.coro.free(token, ptr nocapture readonly) #13

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
  %16 = bitcast ptr %2 to ptr
  %17 = call token @llvm.coro.id(i32 16, ptr %16, ptr null, ptr null)
  %18 = call i1 @llvm.coro.alloc(token %17)
  br i1 %18, label %19, label %22

19:                                               ; preds = %0
  %20 = call i64 @llvm.coro.size.i64()
  %21 = call noalias noundef nonnull ptr @_Znwm(i64 noundef %20) #14
  br label %22

22:                                               ; preds = %19, %0
  %23 = phi ptr [ null, %0 ], [ %21, %19 ]
  %24 = call ptr @llvm.coro.begin(token %17, ptr %23)
  call void @llvm.lifetime.start.p0(i64 1, ptr %2) #3
  call void @llvm.memset.p0.i64(ptr align 1 %2, i8 0, i64 1, i1 false), !annotation !6
  %25 = invoke i64 @_ZN4task12promise_type17get_return_objectEv(ptr noundef nonnull align 1 dereferenceable(1) %2)
          to label %26 unwind label %35

26:                                               ; preds = %22
  %27 = getelementptr inbounds %struct.task, ptr %1, i32 0, i32 0
  %28 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %27, i32 0, i32 0
  %29 = inttoptr i64 %25 to ptr
  store ptr %29, ptr %28, align 8
  call void @llvm.lifetime.start.p0(i64 1, ptr %5) #3
  call void @_ZN4task12promise_type15initial_suspendEv(ptr noundef nonnull align 1 dereferenceable(1) %2) #3
  %30 = call noundef i1 @_ZNKSt7__n486113suspend_never11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %5) #3
  br i1 %30, label %39, label %31

31:                                               ; preds = %26
  %32 = call token @llvm.coro.save(ptr null)
  call void @llvm.coro.await.suspend.void(ptr %5, ptr %24, ptr @_Z10registeredv.__await_suspend_wrapper__init) #3
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
  call void @_ZNKSt7__n486113suspend_never12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %5) #3
  br label %40

40:                                               ; preds = %39, %34
  %41 = phi i32 [ 0, %39 ], [ 2, %34 ]
  call void @llvm.lifetime.end.p0(i64 1, ptr %5) #3
  switch i32 %41, label %114 [
    i32 0, label %42
  ]

42:                                               ; preds = %40
  call void @llvm.lifetime.start.p0(i64 4, ptr %7) #3
  call void @llvm.memset.p0.i64(ptr align 4 %7, i8 0, i64 4, i1 false), !annotation !6
  call void @_ZN1TC2Ev(ptr noundef nonnull align 4 dereferenceable(4) %7) #3
  invoke void @_ZN8registry3addEP1T(ptr noundef nonnull align 8 dereferenceable(8) @reg, ptr noundef %7)
          to label %43 unwind label %49

43:                                               ; preds = %42
  call void @llvm.lifetime.start.p0(i64 1, ptr %8) #3
  %44 = call noundef i1 @_ZNKSt7__n486114suspend_always11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %8) #3
  br i1 %44, label %53, label %45

45:                                               ; preds = %43
  %46 = call token @llvm.coro.save(ptr null)
  call void @llvm.coro.await.suspend.void(ptr %8, ptr %24, ptr @_Z10registeredv.__await_suspend_wrapper__await) #3
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
  call void @_ZNKSt7__n486114suspend_always12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %8) #3
  br label %54

54:                                               ; preds = %53, %48
  %55 = phi i32 [ 0, %53 ], [ 2, %48 ]
  call void @llvm.lifetime.end.p0(i64 1, ptr %8) #3
  switch i32 %55, label %76 [
    i32 0, label %56
  ]

56:                                               ; preds = %54
  call void @llvm.lifetime.start.p0(i64 512, ptr %9) #3
  call void @llvm.memset.p0.i64(ptr align 8 %9, i8 0, i64 512, i1 false), !annotation !6
  call void @llvm.lifetime.start.p0(i64 8, ptr %10) #3
  store ptr null, ptr %10, align 8, !annotation !6
  store ptr %9, ptr %10, align 8
  call void @llvm.lifetime.start.p0(i64 8, ptr %11) #3
  %57 = load ptr, ptr %10, align 8
  %58 = getelementptr inbounds [64 x i64], ptr %57, i64 0, i64 0
  store ptr %58, ptr %11, align 8
  call void @llvm.lifetime.start.p0(i64 8, ptr %12) #3
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
  call void @llvm.lifetime.end.p0(i64 8, ptr %12) #3
  call void @llvm.lifetime.end.p0(i64 8, ptr %11) #3
  call void @llvm.lifetime.end.p0(i64 8, ptr %10) #3
  br label %73

67:                                               ; preds = %62
  call void @llvm.lifetime.start.p0(i64 8, ptr %13) #3
  store ptr null, ptr %13, align 8, !annotation !6
  %68 = load ptr, ptr %11, align 8
  store ptr %68, ptr %13, align 8
  %69 = load ptr, ptr %13, align 8
  store i64 1515870810, ptr %69, align 8
  call void @llvm.lifetime.end.p0(i64 8, ptr %13) #3
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
  call void @llvm.lifetime.end.p0(i64 512, ptr %9) #3
  br label %76

76:                                               ; preds = %74, %54
  %77 = phi i32 [ %55, %54 ], [ 0, %74 ]
  call void @llvm.lifetime.end.p0(i64 4, ptr %7) #3
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
  call void @llvm.lifetime.end.p0(i64 512, ptr %9) #3
  br label %84

84:                                               ; preds = %80, %49
  call void @llvm.lifetime.end.p0(i64 4, ptr %7) #3
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
  %91 = call ptr @__cxa_begin_catch(ptr %90) #3
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
  call void @llvm.lifetime.start.p0(i64 1, ptr %14) #3
  call void @_ZN4task12promise_type13final_suspendEv(ptr noundef nonnull align 1 dereferenceable(1) %2) #3
  %96 = call noundef i1 @_ZNKSt7__n486114suspend_always11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %14) #3
  br i1 %96, label %110, label %97

97:                                               ; preds = %95
  %98 = call token @llvm.coro.save(ptr null)
  call void @llvm.coro.await.suspend.void(ptr %14, ptr %24, ptr @_Z10registeredv.__await_suspend_wrapper__final) #3
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
  call void @_ZNKSt7__n486114suspend_always12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %14) #3
  br label %111

111:                                              ; preds = %110, %100
  %112 = phi i32 [ 0, %110 ], [ 2, %100 ]
  call void @llvm.lifetime.end.p0(i64 1, ptr %14) #3
  switch i32 %112, label %114 [
    i32 0, label %113
  ]

113:                                              ; preds = %111
  br label %114

114:                                              ; preds = %113, %111, %76, %40
  %115 = phi i32 [ %41, %40 ], [ %77, %76 ], [ %112, %111 ], [ 0, %113 ]
  call void @llvm.lifetime.end.p0(i64 1, ptr %2) #3
  %116 = call ptr @llvm.coro.free(token %17, ptr %24)
  %117 = icmp ne ptr %116, null
  br i1 %117, label %118, label %120

118:                                              ; preds = %114
  %119 = call i64 @llvm.coro.size.i64()
  call void @_ZdlPvm(ptr noundef %116, i64 noundef %119) #3
  br label %120

120:                                              ; preds = %118, %114
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
  call void @llvm.lifetime.end.p0(i64 1, ptr %2) #3
  %132 = call ptr @llvm.coro.free(token %17, ptr %24)
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

142:                                              ; preds = %101
  %143 = landingpad { ptr, i32 }
          catch ptr null
  %144 = extractvalue { ptr, i32 } %143, 0
  call void @__clang_call_terminate(ptr %144) #15
  unreachable

145:                                              ; preds = %120
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

attributes #0 = { mustprogress noinline optnone uwtable "frame-pointer"="non-leaf" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #1 = { mustprogress noinline optnone presplitcoroutine uwtable "frame-pointer"="non-leaf" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #2 = { nocallback nofree nosync nounwind willreturn memory(argmem: read) }
attributes #3 = { nounwind }
attributes #4 = { nobuiltin allocsize(0) "frame-pointer"="non-leaf" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #5 = { nounwind memory(none) }
attributes #6 = { nocallback nofree nosync nounwind willreturn memory(argmem: readwrite) }
attributes #7 = { nocallback nofree nounwind willreturn memory(argmem: write) }
attributes #8 = { mustprogress noinline nounwind optnone uwtable "frame-pointer"="non-leaf" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #9 = { nomerge nounwind }
attributes #10 = { alwaysinline mustprogress }
attributes #11 = { noinline noreturn nounwind uwtable "frame-pointer"="non-leaf" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #12 = { nobuiltin nounwind "frame-pointer"="non-leaf" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #13 = { nounwind memory(argmem: read) }
attributes #14 = { allocsize(0) }
attributes #15 = { noreturn nounwind }

!llvm.linker.options = !{}
!llvm.module.flags = !{!0, !1, !2, !3, !4}
!llvm.ident = !{!5}

!0 = !{i32 1, !"wchar_size", i32 4}
!1 = !{i32 8, !"PIC Level", i32 2}
!2 = !{i32 7, !"PIE Level", i32 2}
!3 = !{i32 7, !"uwtable", i32 2}
!4 = !{i32 7, !"frame-pointer", i32 1}
!5 = !{!"kiln version 1.0.1"}
!6 = !{!"auto-init"}
