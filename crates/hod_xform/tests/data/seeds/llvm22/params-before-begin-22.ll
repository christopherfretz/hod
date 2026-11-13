; ModuleID = 'params-before-begin-22.ll'
source_filename = "docs/design/0005-data/params-before-begin.cpp"
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i8:8:32-i16:16:32-i64:64-i128:128-n32:64-S128-Fn32"
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
  %19 = call token @llvm.coro.id(i32 16, ptr %11, ptr null, ptr null)
  %20 = call i1 @llvm.coro.alloc(token %19)
  br i1 %20, label %21, label %24

21:                                               ; preds = %3
  %22 = call i64 @llvm.coro.size.i64()
  %23 = call noundef ptr @_ZN4task12promise_typenwEmRiRlRc(i64 noundef %22, ptr noundef nonnull align 4 dereferenceable(4) %5, ptr noundef nonnull align 8 dereferenceable(8) %6, ptr noundef nonnull align 1 dereferenceable(1) %7)
  br label %24

24:                                               ; preds = %21, %3
  %25 = phi ptr [ null, %3 ], [ %23, %21 ]
  %26 = call ptr @llvm.coro.begin(token %19, ptr %25)
  call void @llvm.lifetime.start.p0(ptr %8) #2
  %27 = load i32, ptr %5, align 4
  store i32 %27, ptr %8, align 4
  call void @llvm.lifetime.start.p0(ptr %9) #2
  %28 = load i64, ptr %6, align 8
  store i64 %28, ptr %9, align 8
  call void @llvm.lifetime.start.p0(ptr %10) #2
  %29 = load i8, ptr %7, align 1
  store i8 %29, ptr %10, align 1
  call void @llvm.lifetime.start.p0(ptr %11) #2
  call void @llvm.memset.p0.i64(ptr align 1 %11, i8 0, i64 1, i1 false), !annotation !6
  %30 = invoke i64 @_ZN4task12promise_type17get_return_objectEv(ptr noundef nonnull align 1 dereferenceable(1) %11)
          to label %31 unwind label %40

31:                                               ; preds = %24
  %32 = getelementptr inbounds nuw %struct.task, ptr %4, i32 0, i32 0
  %33 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %32, i32 0, i32 0
  %34 = inttoptr i64 %30 to ptr
  store ptr %34, ptr %33, align 8
  call void @llvm.lifetime.start.p0(ptr %14) #2
  call void @_ZN4task12promise_type15initial_suspendEv(ptr noundef nonnull align 1 dereferenceable(1) %11) #2
  %35 = call noundef i1 @_ZNKSt7__n486114suspend_always11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %14) #2
  br i1 %35, label %44, label %36

36:                                               ; preds = %31
  %37 = call token @llvm.coro.save(ptr null)
  call void @llvm.coro.await.suspend.void(ptr %14, ptr %26, ptr @_Z1filc.__await_suspend_wrapper__init) #2
  %38 = call i8 @llvm.coro.suspend(token %37, i1 false)
  switch i8 %38, label %100 [
    i8 0, label %44
    i8 1, label %39
  ]

39:                                               ; preds = %36
  br label %45

40:                                               ; preds = %24
  %41 = landingpad { ptr, i32 }
          cleanup
  %42 = extractvalue { ptr, i32 } %41, 0
  store ptr %42, ptr %12, align 8
  %43 = extractvalue { ptr, i32 } %41, 1
  store i32 %43, ptr %13, align 4
  br label %108

44:                                               ; preds = %36, %31
  call void @_ZNKSt7__n486114suspend_always12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %14) #2
  br label %45

45:                                               ; preds = %44, %39
  %46 = phi i32 [ 0, %44 ], [ 2, %39 ]
  call void @llvm.lifetime.end.p0(ptr %14) #2
  switch i32 %46, label %93 [
    i32 0, label %47
  ]

47:                                               ; preds = %45
  call void @llvm.lifetime.start.p0(ptr %16) #2
  %48 = call noundef i1 @_ZNKSt7__n486114suspend_always11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %16) #2
  br i1 %48, label %53, label %49

49:                                               ; preds = %47
  %50 = call token @llvm.coro.save(ptr null)
  call void @llvm.coro.await.suspend.void(ptr %16, ptr %26, ptr @_Z1filc.__await_suspend_wrapper__await) #2
  %51 = call i8 @llvm.coro.suspend(token %50, i1 false)
  switch i8 %51, label %100 [
    i8 0, label %53
    i8 1, label %52
  ]

52:                                               ; preds = %49
  br label %54

53:                                               ; preds = %49, %47
  call void @_ZNKSt7__n486114suspend_always12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %16) #2
  br label %54

54:                                               ; preds = %53, %52
  %55 = phi i32 [ 0, %53 ], [ 2, %52 ]
  call void @llvm.lifetime.end.p0(ptr %16) #2
  switch i32 %55, label %93 [
    i32 0, label %56
  ]

56:                                               ; preds = %54
  %57 = load i32, ptr %8, align 4
  %58 = load i64, ptr %9, align 8
  %59 = load i8, ptr %10, align 1
  %60 = zext i8 %59 to i32
  %61 = invoke i32 (ptr, ...) @printf(ptr noundef @.str, i32 noundef %57, i64 noundef %58, i32 noundef %60)
          to label %62 unwind label %64

62:                                               ; preds = %56
  invoke void @_ZN4task12promise_type11return_voidEv(ptr noundef nonnull align 1 dereferenceable(1) %11)
          to label %63 unwind label %64

63:                                               ; preds = %62
  br label %74

64:                                               ; preds = %62, %56
  %65 = landingpad { ptr, i32 }
          catch ptr null
  %66 = extractvalue { ptr, i32 } %65, 0
  store ptr %66, ptr %12, align 8
  %67 = extractvalue { ptr, i32 } %65, 1
  store i32 %67, ptr %13, align 4
  br label %68

68:                                               ; preds = %64
  %69 = load ptr, ptr %12, align 8
  %70 = call ptr @__cxa_begin_catch(ptr %69) #2
  invoke void @_ZN4task12promise_type19unhandled_exceptionEv(ptr noundef nonnull align 1 dereferenceable(1) %11)
          to label %71 unwind label %80

71:                                               ; preds = %68
  invoke void @__cxa_end_catch()
          to label %72 unwind label %84

72:                                               ; preds = %71
  br label %73

73:                                               ; preds = %72
  br label %74

74:                                               ; preds = %73, %63
  call void @llvm.lifetime.start.p0(ptr %17) #2
  call void @_ZN4task12promise_type13final_suspendEv(ptr noundef nonnull align 1 dereferenceable(1) %11) #2
  %75 = call noundef i1 @_ZNKSt7__n486114suspend_always11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %17) #2
  br i1 %75, label %89, label %76

76:                                               ; preds = %74
  %77 = call token @llvm.coro.save(ptr null)
  call void @llvm.coro.await.suspend.void(ptr %17, ptr %26, ptr @_Z1filc.__await_suspend_wrapper__final) #2
  %78 = call i8 @llvm.coro.suspend(token %77, i1 true)
  switch i8 %78, label %100 [
    i8 0, label %89
    i8 1, label %79
  ]

79:                                               ; preds = %76
  br label %90

80:                                               ; preds = %68
  %81 = landingpad { ptr, i32 }
          cleanup
  %82 = extractvalue { ptr, i32 } %81, 0
  store ptr %82, ptr %12, align 8
  %83 = extractvalue { ptr, i32 } %81, 1
  store i32 %83, ptr %13, align 4
  invoke void @__cxa_end_catch()
          to label %88 unwind label %118

84:                                               ; preds = %71
  %85 = landingpad { ptr, i32 }
          cleanup
  %86 = extractvalue { ptr, i32 } %85, 0
  store ptr %86, ptr %12, align 8
  %87 = extractvalue { ptr, i32 } %85, 1
  store i32 %87, ptr %13, align 4
  br label %105

88:                                               ; preds = %80
  br label %105

89:                                               ; preds = %76, %74
  call void @_ZNKSt7__n486114suspend_always12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %17) #2
  br label %90

90:                                               ; preds = %89, %79
  %91 = phi i32 [ 0, %89 ], [ 2, %79 ]
  call void @llvm.lifetime.end.p0(ptr %17) #2
  switch i32 %91, label %93 [
    i32 0, label %92
  ]

92:                                               ; preds = %90
  br label %93

93:                                               ; preds = %92, %90, %54, %45
  %94 = phi i32 [ %46, %45 ], [ %55, %54 ], [ %91, %90 ], [ 0, %92 ]
  call void @llvm.lifetime.end.p0(ptr %11) #2
  call void @llvm.lifetime.end.p0(ptr %10) #2
  call void @llvm.lifetime.end.p0(ptr %9) #2
  call void @llvm.lifetime.end.p0(ptr %8) #2
  %95 = call ptr @llvm.coro.free(token %19, ptr %26)
  %96 = icmp ne ptr %95, null
  br i1 %96, label %97, label %98

97:                                               ; preds = %93
  call void @_ZN4task12promise_typedlEPv(ptr noundef %95) #2
  br label %98

98:                                               ; preds = %97, %93
  switch i32 %94, label %121 [
    i32 0, label %99
    i32 2, label %100
  ]

99:                                               ; preds = %98
  br label %100

100:                                              ; preds = %99, %98, %76, %49, %36
  call void @llvm.coro.end(ptr null, i1 false, token none)
  %101 = getelementptr inbounds nuw %struct.task, ptr %4, i32 0, i32 0
  %102 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %101, i32 0, i32 0
  %103 = load ptr, ptr %102, align 8
  %104 = ptrtoint ptr %103 to i64
  ret i64 %104

105:                                              ; preds = %88, %84
  call void @llvm.coro.end(ptr null, i1 true, token none)
  %106 = call i1 @llvm.coro.is_in_ramp()
  br i1 %106, label %107, label %113

107:                                              ; preds = %105
  br label %108

108:                                              ; preds = %107, %40
  call void @llvm.lifetime.end.p0(ptr %11) #2
  call void @llvm.lifetime.end.p0(ptr %10) #2
  call void @llvm.lifetime.end.p0(ptr %9) #2
  call void @llvm.lifetime.end.p0(ptr %8) #2
  %109 = call ptr @llvm.coro.free(token %19, ptr %26)
  %110 = icmp ne ptr %109, null
  br i1 %110, label %111, label %112

111:                                              ; preds = %108
  call void @_ZN4task12promise_typedlEPv(ptr noundef %109) #2
  br label %112

112:                                              ; preds = %111, %108
  br label %113

113:                                              ; preds = %112, %105
  %114 = load ptr, ptr %12, align 8
  %115 = load i32, ptr %13, align 4
  %116 = insertvalue { ptr, i32 } poison, ptr %114, 0
  %117 = insertvalue { ptr, i32 } %116, i32 %115, 1
  resume { ptr, i32 } %117

118:                                              ; preds = %80
  %119 = landingpad { ptr, i32 }
          catch ptr null
  %120 = extractvalue { ptr, i32 } %119, 0
  call void @__clang_call_terminate(ptr %120) #13
  unreachable

121:                                              ; preds = %98
  unreachable
}

; Function Attrs: nocallback nofree nosync nounwind willreturn memory(argmem: read)
declare token @llvm.coro.id(i32, ptr readnone, ptr readonly captures(none), ptr) #1

; Function Attrs: nounwind
declare i1 @llvm.coro.alloc(token) #2

; Function Attrs: mustprogress noinline optnone uwtable
declare dso_local noundef ptr @_ZN4task12promise_typenwEmRiRlRc(i64 noundef, ptr noundef nonnull align 4 dereferenceable(4), ptr noundef nonnull align 8 dereferenceable(8), ptr noundef nonnull align 1 dereferenceable(1)) #3

; Function Attrs: nounwind memory(none)
declare i64 @llvm.coro.size.i64() #4

; Function Attrs: nounwind
declare ptr @llvm.coro.begin(token, ptr writeonly) #2

; Function Attrs: nocallback nofree nosync nounwind willreturn memory(argmem: readwrite)
declare void @llvm.lifetime.start.p0(ptr captures(none)) #5

; Function Attrs: nocallback nofree nounwind willreturn memory(argmem: write)
declare void @llvm.memset.p0.i64(ptr writeonly captures(none), i8, i64, i1 immarg) #6

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
declare void @llvm.lifetime.end.p0(ptr captures(none)) #5

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
declare void @llvm.coro.end(ptr, i1, token) #2

; Function Attrs: nounwind memory(none)
declare i1 @llvm.coro.is_in_ramp() #4

; Function Attrs: mustprogress noinline nounwind optnone uwtable
declare dso_local void @_ZN4task12promise_typedlEPv(ptr noundef) #7

; Function Attrs: nounwind memory(argmem: read)
declare ptr @llvm.coro.free(token, ptr readonly captures(none)) #12

attributes #0 = { mustprogress noinline optnone presplitcoroutine uwtable "frame-pointer"="non-leaf-no-reserve" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #1 = { nocallback nofree nosync nounwind willreturn memory(argmem: read) }
attributes #2 = { nounwind }
attributes #3 = { mustprogress noinline optnone uwtable "frame-pointer"="non-leaf-no-reserve" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #4 = { nounwind memory(none) }
attributes #5 = { nocallback nofree nosync nounwind willreturn memory(argmem: readwrite) }
attributes #6 = { nocallback nofree nounwind willreturn memory(argmem: write) }
attributes #7 = { mustprogress noinline nounwind optnone uwtable "frame-pointer"="non-leaf-no-reserve" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #8 = { nomerge nounwind }
attributes #9 = { alwaysinline mustprogress "sample-profile-suffix-elision-policy"="selected" }
attributes #10 = { "frame-pointer"="non-leaf-no-reserve" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #11 = { noinline noreturn nounwind uwtable "frame-pointer"="non-leaf-no-reserve" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #12 = { nounwind memory(argmem: read) }
attributes #13 = { noreturn nounwind }

!llvm.module.flags = !{!0, !1, !2, !3, !4}
!llvm.ident = !{!5}

!0 = !{i32 1, !"wchar_size", i32 4}
!1 = !{i32 8, !"PIC Level", i32 2}
!2 = !{i32 7, !"PIE Level", i32 2}
!3 = !{i32 7, !"uwtable", i32 2}
!4 = !{i32 7, !"frame-pointer", i32 4}
!5 = !{!"kiln version 1.0.1"}
!6 = !{!"auto-init"}
