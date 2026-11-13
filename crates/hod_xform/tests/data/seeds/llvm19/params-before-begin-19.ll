; ModuleID = 'params-before-begin-19.ll'
source_filename = "docs/design/0005-data/params-before-begin.cpp"
target datalayout = "e-m:e-i8:8:32-i16:16:32-i64:64-i128:128-n32:64-S128-Fn32"
target triple = "aarch64-unknown-linux-gnu"

%struct.task = type { %"struct.std::__n4861::coroutine_handle" }
%"struct.std::__n4861::coroutine_handle" = type { ptr }
%"struct.task::promise_type" = type { i8 }
%"struct.std::__n4861::suspend_always" = type { i8 }

@.str = external hidden unnamed_addr constant [16 x i8], align 1

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
  call void @llvm.memset.p0.i64(ptr align 1 %11, i8 0, i64 1, i1 false), !annotation !6
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

99:                                               ; preds = %98, %94
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

114:                                              ; preds = %113, %110
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
  call void @__clang_call_terminate(ptr %122) #13
  unreachable

123:                                              ; preds = %99
  unreachable
}

; Function Attrs: nocallback nofree nosync nounwind willreturn memory(argmem: read)
declare token @llvm.coro.id(i32, ptr readnone, ptr nocapture readonly, ptr) #1

; Function Attrs: nounwind
declare i1 @llvm.coro.alloc(token) #2

; Function Attrs: mustprogress noinline optnone uwtable
declare dso_local noundef ptr @_ZN4task12promise_typenwEmRiRlRc(i64 noundef, ptr noundef nonnull align 4 dereferenceable(4), ptr noundef nonnull align 8 dereferenceable(8), ptr noundef nonnull align 1 dereferenceable(1)) #3

; Function Attrs: nounwind memory(none)
declare i64 @llvm.coro.size.i64() #4

; Function Attrs: nounwind
declare ptr @llvm.coro.begin(token, ptr writeonly) #2

; Function Attrs: nocallback nofree nosync nounwind willreturn memory(argmem: readwrite)
declare void @llvm.lifetime.start.p0(i64 immarg, ptr nocapture) #5

; Function Attrs: nocallback nofree nounwind willreturn memory(argmem: write)
declare void @llvm.memset.p0.i64(ptr nocapture writeonly, i8, i64, i1 immarg) #6

; Function Attrs: mustprogress noinline optnone uwtable
declare dso_local i64 @_ZN4task12promise_type17get_return_objectEv(ptr noundef nonnull align 1 dereferenceable(1)) #3

declare i32 @__gxx_personality_v0(...)

; Function Attrs: mustprogress noinline nounwind optnone uwtable
declare dso_local void @_ZN4task12promise_type15initial_suspendEv(ptr noundef nonnull align 1 dereferenceable(1)) #7

; Function Attrs: mustprogress noinline nounwind optnone uwtable
declare dso_local noundef i1 @_ZNKSt7__n486114suspend_always11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1)) #7

; Function Attrs: nomerge nounwind
declare token @llvm.coro.save(ptr) #8

; Function Attrs: alwaysinline mustprogress
declare hidden void @_Z1filc.__await_suspend_wrapper__init(ptr noundef nonnull, ptr noundef) #9

declare void @llvm.coro.await.suspend.void(ptr, ptr, ptr)

; Function Attrs: nounwind
declare i8 @llvm.coro.suspend(token, i1) #2

; Function Attrs: mustprogress noinline nounwind optnone uwtable
declare dso_local void @_ZNKSt7__n486114suspend_always12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1)) #7

; Function Attrs: nocallback nofree nosync nounwind willreturn memory(argmem: readwrite)
declare void @llvm.lifetime.end.p0(i64 immarg, ptr nocapture) #5

; Function Attrs: alwaysinline mustprogress
declare hidden void @_Z1filc.__await_suspend_wrapper__await(ptr noundef nonnull, ptr noundef) #9

declare i32 @printf(ptr noundef, ...) #10

; Function Attrs: mustprogress noinline nounwind optnone uwtable
declare dso_local void @_ZN4task12promise_type11return_voidEv(ptr noundef nonnull align 1 dereferenceable(1)) #7

declare ptr @__cxa_begin_catch(ptr)

; Function Attrs: mustprogress noinline nounwind optnone uwtable
declare dso_local void @_ZN4task12promise_type19unhandled_exceptionEv(ptr noundef nonnull align 1 dereferenceable(1)) #7

declare void @__cxa_end_catch()

; Function Attrs: noinline noreturn nounwind uwtable
declare hidden void @__clang_call_terminate(ptr noundef) #11

; Function Attrs: mustprogress noinline nounwind optnone uwtable
declare dso_local void @_ZN4task12promise_type13final_suspendEv(ptr noundef nonnull align 1 dereferenceable(1)) #7

; Function Attrs: alwaysinline mustprogress
declare hidden void @_Z1filc.__await_suspend_wrapper__final(ptr noundef nonnull, ptr noundef) #9

; Function Attrs: nounwind
declare i1 @llvm.coro.end(ptr, i1, token) #2

; Function Attrs: mustprogress noinline nounwind optnone uwtable
declare dso_local void @_ZN4task12promise_typedlEPv(ptr noundef) #7

; Function Attrs: nounwind memory(argmem: read)
declare ptr @llvm.coro.free(token, ptr nocapture readonly) #12

attributes #0 = { mustprogress noinline optnone presplitcoroutine uwtable "frame-pointer"="non-leaf" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #1 = { nocallback nofree nosync nounwind willreturn memory(argmem: read) }
attributes #2 = { nounwind }
attributes #3 = { mustprogress noinline optnone uwtable "frame-pointer"="non-leaf" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #4 = { nounwind memory(none) }
attributes #5 = { nocallback nofree nosync nounwind willreturn memory(argmem: readwrite) }
attributes #6 = { nocallback nofree nounwind willreturn memory(argmem: write) }
attributes #7 = { mustprogress noinline nounwind optnone uwtable "frame-pointer"="non-leaf" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #8 = { nomerge nounwind }
attributes #9 = { alwaysinline mustprogress }
attributes #10 = { "frame-pointer"="non-leaf" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #11 = { noinline noreturn nounwind uwtable "frame-pointer"="non-leaf" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #12 = { nounwind memory(argmem: read) }
attributes #13 = { noreturn nounwind }

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
