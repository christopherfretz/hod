; ModuleID = 'gro-throws-uaf-22.ll'
source_filename = "docs/design/0005-data/gro-throws-uaf.cpp"
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i8:8:32-i16:16:32-i64:64-i128:128-n32:64-S128-Fn32"
target triple = "aarch64-unknown-linux-gnu"

%struct.task = type { %"struct.std::__n4861::coroutine_handle" }
%"struct.std::__n4861::coroutine_handle" = type { ptr }
%"struct.task::promise_type" = type { %"struct.std::__n4861::coroutine_handle.0" }
%"struct.std::__n4861::coroutine_handle.0" = type { ptr }
%"struct.std::__n4861::suspend_always" = type { i8 }
%struct.Dtor = type { i32 }
%"struct.task::promise_type::fin" = type { i8 }

@.str = external hidden unnamed_addr constant [5 x i8], align 1
@_ZTISt13runtime_error = external constant ptr

; Function Attrs: mustprogress noinline optnone presplitcoroutine uwtable
define dso_local void @_Z4leafi(ptr dead_on_unwind noalias writable sret(%struct.task) align 8 %0, i32 noundef %1) #0 personality ptr @__gxx_personality_v0 {
  %3 = alloca ptr, align 8
  %4 = alloca i32, align 4
  %5 = alloca i32, align 4
  %6 = alloca %"struct.task::promise_type", align 8
  %7 = alloca ptr, align 8
  %8 = alloca i32, align 4
  %9 = alloca %"struct.std::__n4861::suspend_always", align 1
  %10 = alloca %"struct.std::__n4861::suspend_always", align 1
  %11 = alloca %struct.Dtor, align 4
  %12 = alloca %"struct.task::promise_type::fin", align 1
  %13 = alloca %"struct.task::promise_type::fin", align 1
  store ptr %0, ptr %3, align 8
  store i32 %1, ptr %4, align 4
  %14 = call token @llvm.coro.id(i32 16, ptr %6, ptr null, ptr null)
  %15 = call i1 @llvm.coro.alloc(token %14)
  br i1 %15, label %16, label %19

16:                                               ; preds = %2
  %17 = call i64 @llvm.coro.size.i64()
  %18 = call noalias noundef nonnull ptr @_Znwm(i64 noundef %17) #16
  br label %19

19:                                               ; preds = %16, %2
  %20 = phi ptr [ null, %2 ], [ %18, %16 ]
  %21 = call ptr @llvm.coro.begin(token %14, ptr %20)
  call void @llvm.lifetime.start.p0(ptr %5) #2
  %22 = load i32, ptr %4, align 4
  store i32 %22, ptr %5, align 4
  call void @llvm.lifetime.start.p0(ptr %6) #2
  call void @llvm.memset.p0.i64(ptr align 8 %6, i8 0, i64 8, i1 false), !annotation !6
  call void @_ZN4task12promise_typeC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %6) #2
  invoke void @_ZN4task12promise_type17get_return_objectEv(ptr dead_on_unwind writable sret(%struct.task) align 8 %0, ptr noundef nonnull align 8 dereferenceable(8) %6)
          to label %23 unwind label %29

23:                                               ; preds = %19
  call void @llvm.lifetime.start.p0(ptr %9) #2
  call void @_ZN4task12promise_type15initial_suspendEv(ptr noundef nonnull align 8 dereferenceable(8) %6) #2
  %24 = call noundef i1 @_ZNKSt7__n486114suspend_always11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %9) #2
  br i1 %24, label %33, label %25

25:                                               ; preds = %23
  %26 = call token @llvm.coro.save(ptr null)
  call void @llvm.coro.await.suspend.void(ptr %9, ptr %21, ptr @_Z4leafi.__await_suspend_wrapper__init) #2
  %27 = call i8 @llvm.coro.suspend(token %26, i1 false)
  switch i8 %27, label %93 [
    i8 0, label %33
    i8 1, label %28
  ]

28:                                               ; preds = %25
  br label %34

29:                                               ; preds = %19
  %30 = landingpad { ptr, i32 }
          cleanup
  %31 = extractvalue { ptr, i32 } %30, 0
  store ptr %31, ptr %7, align 8
  %32 = extractvalue { ptr, i32 } %30, 1
  store i32 %32, ptr %8, align 4
  br label %97

33:                                               ; preds = %25, %23
  call void @_ZNKSt7__n486114suspend_always12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %9) #2
  br label %34

34:                                               ; preds = %33, %28
  %35 = phi i32 [ 0, %33 ], [ 2, %28 ]
  call void @llvm.lifetime.end.p0(ptr %9) #2
  switch i32 %35, label %85 [
    i32 0, label %36
  ]

36:                                               ; preds = %34
  call void @llvm.lifetime.start.p0(ptr %11) #2
  call void @llvm.memset.p0.i64(ptr align 4 %11, i8 0, i64 4, i1 false), !annotation !6
  %37 = getelementptr inbounds nuw %struct.Dtor, ptr %11, i32 0, i32 0
  %38 = load i32, ptr %5, align 4
  store i32 %38, ptr %37, align 4
  %39 = load i32, ptr %5, align 4
  %40 = icmp eq i32 %39, 3
  br i1 %40, label %41, label %52

41:                                               ; preds = %36
  %42 = call ptr @__cxa_allocate_exception(i64 16) #2
  invoke void @_ZNSt13runtime_errorC1EPKc(ptr noundef nonnull align 8 dereferenceable(16) %42, ptr noundef @.str)
          to label %43 unwind label %44

43:                                               ; preds = %41
  invoke void @__cxa_throw(ptr %42, ptr @_ZTISt13runtime_error, ptr @_ZNSt13runtime_errorD1Ev) #17
          to label %111 unwind label %48

44:                                               ; preds = %41
  %45 = landingpad { ptr, i32 }
          catch ptr null
  %46 = extractvalue { ptr, i32 } %45, 0
  store ptr %46, ptr %7, align 8
  %47 = extractvalue { ptr, i32 } %45, 1
  store i32 %47, ptr %8, align 4
  call void @__cxa_free_exception(ptr %42) #2
  br label %54

48:                                               ; preds = %52, %43
  %49 = landingpad { ptr, i32 }
          catch ptr null
  %50 = extractvalue { ptr, i32 } %49, 0
  store ptr %50, ptr %7, align 8
  %51 = extractvalue { ptr, i32 } %49, 1
  store i32 %51, ptr %8, align 4
  br label %54

52:                                               ; preds = %36
  invoke void @_ZN4task12promise_type11return_voidEv(ptr noundef nonnull align 8 dereferenceable(8) %6)
          to label %53 unwind label %48

53:                                               ; preds = %52
  call void @_ZN4DtorD2Ev(ptr noundef nonnull align 4 dereferenceable(4) %11) #2
  call void @llvm.lifetime.end.p0(ptr %11) #2
  br label %61

54:                                               ; preds = %48, %44
  call void @_ZN4DtorD2Ev(ptr noundef nonnull align 4 dereferenceable(4) %11) #2
  call void @llvm.lifetime.end.p0(ptr %11) #2
  br label %55

55:                                               ; preds = %54
  %56 = load ptr, ptr %7, align 8
  %57 = call ptr @__cxa_begin_catch(ptr %56) #2
  invoke void @_ZN4task12promise_type19unhandled_exceptionEv(ptr noundef nonnull align 8 dereferenceable(8) %6)
          to label %58 unwind label %68

58:                                               ; preds = %55
  invoke void @__cxa_end_catch()
          to label %59 unwind label %72

59:                                               ; preds = %58
  br label %60

60:                                               ; preds = %59
  br label %61

61:                                               ; preds = %60, %53
  call void @llvm.lifetime.start.p0(ptr %12) #2
  call void @_ZN4task12promise_type13final_suspendEv(ptr noundef nonnull align 8 dereferenceable(8) %6) #2
  %62 = call noundef i1 @_ZN4task12promise_type3fin11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %12) #2
  br i1 %62, label %81, label %63

63:                                               ; preds = %61
  %64 = call token @llvm.coro.save(ptr null)
  invoke void @llvm.coro.await.suspend.handle(ptr %12, ptr %21, ptr @_Z4leafi.__await_suspend_wrapper__final)
          to label %65 unwind label %77

65:                                               ; preds = %63
  %66 = call i8 @llvm.coro.suspend(token %64, i1 true)
  switch i8 %66, label %93 [
    i8 0, label %81
    i8 1, label %67
  ]

67:                                               ; preds = %65
  br label %82

68:                                               ; preds = %55
  %69 = landingpad { ptr, i32 }
          cleanup
  %70 = extractvalue { ptr, i32 } %69, 0
  store ptr %70, ptr %7, align 8
  %71 = extractvalue { ptr, i32 } %69, 1
  store i32 %71, ptr %8, align 4
  invoke void @__cxa_end_catch()
          to label %76 unwind label %108

72:                                               ; preds = %58
  %73 = landingpad { ptr, i32 }
          cleanup
  %74 = extractvalue { ptr, i32 } %73, 0
  store ptr %74, ptr %7, align 8
  %75 = extractvalue { ptr, i32 } %73, 1
  store i32 %75, ptr %8, align 4
  br label %94

76:                                               ; preds = %68
  br label %94

77:                                               ; preds = %63
  %78 = landingpad { ptr, i32 }
          cleanup
  %79 = extractvalue { ptr, i32 } %78, 0
  store ptr %79, ptr %7, align 8
  %80 = extractvalue { ptr, i32 } %78, 1
  store i32 %80, ptr %8, align 4
  call void @llvm.lifetime.end.p0(ptr %12) #2
  br label %94

81:                                               ; preds = %65, %61
  call void @_ZN4task12promise_type3fin12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %12) #2
  br label %82

82:                                               ; preds = %81, %67
  %83 = phi i32 [ 0, %81 ], [ 2, %67 ]
  call void @llvm.lifetime.end.p0(ptr %12) #2
  switch i32 %83, label %85 [
    i32 0, label %84
  ]

84:                                               ; preds = %82
  br label %85

85:                                               ; preds = %84, %82, %34
  %86 = phi i32 [ %35, %34 ], [ %83, %82 ], [ 0, %84 ]
  call void @llvm.lifetime.end.p0(ptr %6) #2
  call void @llvm.lifetime.end.p0(ptr %5) #2
  %87 = call ptr @llvm.coro.free(token %14, ptr %21)
  %88 = icmp ne ptr %87, null
  br i1 %88, label %89, label %91

89:                                               ; preds = %85
  %90 = call i64 @llvm.coro.size.i64()
  call void @_ZdlPvm(ptr noundef %87, i64 noundef %90) #2
  br label %91

91:                                               ; preds = %89, %85
  switch i32 %86, label %111 [
    i32 0, label %92
    i32 2, label %93
  ]

92:                                               ; preds = %91
  br label %93

93:                                               ; preds = %92, %91, %65, %25
  call void @llvm.coro.end(ptr null, i1 false, token none)
  ret void

94:                                               ; preds = %77, %76, %72
  call void @llvm.coro.end(ptr null, i1 true, token none)
  %95 = call i1 @llvm.coro.is_in_ramp()
  br i1 %95, label %96, label %103

96:                                               ; preds = %94
  br label %97

97:                                               ; preds = %96, %29
  call void @llvm.lifetime.end.p0(ptr %6) #2
  call void @llvm.lifetime.end.p0(ptr %5) #2
  %98 = call ptr @llvm.coro.free(token %14, ptr %21)
  %99 = icmp ne ptr %98, null
  br i1 %99, label %100, label %102

100:                                              ; preds = %97
  %101 = call i64 @llvm.coro.size.i64()
  call void @_ZdlPvm(ptr noundef %98, i64 noundef %101) #2
  br label %102

102:                                              ; preds = %100, %97
  br label %103

103:                                              ; preds = %102, %94
  %104 = load ptr, ptr %7, align 8
  %105 = load i32, ptr %8, align 4
  %106 = insertvalue { ptr, i32 } poison, ptr %104, 0
  %107 = insertvalue { ptr, i32 } %106, i32 %105, 1
  resume { ptr, i32 } %107

108:                                              ; preds = %68
  %109 = landingpad { ptr, i32 }
          catch ptr null
  %110 = extractvalue { ptr, i32 } %109, 0
  call void @__clang_call_terminate(ptr %110) #18
  unreachable

111:                                              ; preds = %91, %43
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

; Function Attrs: nocallback nofree nounwind willreturn memory(argmem: write)
declare void @llvm.memset.p0.i64(ptr writeonly captures(none), i8, i64, i1 immarg) #6

; Function Attrs: mustprogress noinline nounwind optnone uwtable
declare dso_local void @_ZN4task12promise_typeC2Ev(ptr noundef nonnull align 8 dereferenceable(8)) unnamed_addr #7

; Function Attrs: mustprogress noinline optnone uwtable
declare dso_local void @_ZN4task12promise_type17get_return_objectEv(ptr dead_on_unwind noalias writable sret(%struct.task) align 8, ptr noundef nonnull align 8 dereferenceable(8)) #8

declare i32 @__gxx_personality_v0(...)

; Function Attrs: mustprogress noinline nounwind optnone uwtable
declare dso_local void @_ZN4task12promise_type15initial_suspendEv(ptr noundef nonnull align 8 dereferenceable(8)) #7

; Function Attrs: mustprogress noinline nounwind optnone uwtable
declare dso_local noundef i1 @_ZNKSt7__n486114suspend_always11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1)) #7

; Function Attrs: nomerge nounwind
declare token @llvm.coro.save(ptr) #9

; Function Attrs: alwaysinline mustprogress
declare hidden void @_Z4leafi.__await_suspend_wrapper__init(ptr noundef nonnull, ptr noundef) #10

declare void @llvm.coro.await.suspend.void(ptr, ptr, ptr)

; Function Attrs: nounwind
declare i8 @llvm.coro.suspend(token, i1) #2

; Function Attrs: mustprogress noinline nounwind optnone uwtable
declare dso_local void @_ZNKSt7__n486114suspend_always12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1)) #7

; Function Attrs: nocallback nofree nosync nounwind willreturn memory(argmem: readwrite)
declare void @llvm.lifetime.end.p0(ptr captures(none)) #5

declare ptr @__cxa_allocate_exception(i64)

declare void @_ZNSt13runtime_errorC1EPKc(ptr noundef nonnull align 8 dereferenceable(16), ptr noundef) unnamed_addr #11

declare void @__cxa_free_exception(ptr)

; Function Attrs: nounwind
declare void @_ZNSt13runtime_errorD1Ev(ptr noundef nonnull align 8 dereferenceable(16)) unnamed_addr #12

declare void @__cxa_throw(ptr, ptr, ptr)

; Function Attrs: mustprogress noinline nounwind optnone uwtable
declare dso_local void @_ZN4task12promise_type11return_voidEv(ptr noundef nonnull align 8 dereferenceable(8)) #7

; Function Attrs: mustprogress noinline nounwind optnone uwtable
declare dso_local void @_ZN4DtorD2Ev(ptr noundef nonnull align 4 dereferenceable(4)) unnamed_addr #7

declare ptr @__cxa_begin_catch(ptr)

; Function Attrs: mustprogress noinline optnone uwtable
declare dso_local void @_ZN4task12promise_type19unhandled_exceptionEv(ptr noundef nonnull align 8 dereferenceable(8)) #8

declare void @__cxa_end_catch()

; Function Attrs: noinline noreturn nounwind uwtable
declare hidden void @__clang_call_terminate(ptr noundef) #13

; Function Attrs: mustprogress noinline nounwind optnone uwtable
declare dso_local void @_ZN4task12promise_type13final_suspendEv(ptr noundef nonnull align 8 dereferenceable(8)) #7

; Function Attrs: mustprogress noinline nounwind optnone uwtable
declare dso_local noundef i1 @_ZN4task12promise_type3fin11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1)) #7

; Function Attrs: alwaysinline mustprogress
declare hidden noundef ptr @_Z4leafi.__await_suspend_wrapper__final(ptr noundef nonnull, ptr noundef) #10

declare void @llvm.coro.await.suspend.handle(ptr, ptr, ptr)

; Function Attrs: mustprogress noinline nounwind optnone uwtable
declare dso_local void @_ZN4task12promise_type3fin12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1)) #7

; Function Attrs: nounwind
declare void @llvm.coro.end(ptr, i1, token) #2

; Function Attrs: nounwind memory(none)
declare i1 @llvm.coro.is_in_ramp() #4

; Function Attrs: nobuiltin nounwind
declare void @_ZdlPvm(ptr noundef, i64 noundef) #14

; Function Attrs: nounwind memory(argmem: read)
declare ptr @llvm.coro.free(token, ptr readonly captures(none)) #15

; Function Attrs: mustprogress noinline optnone presplitcoroutine uwtable
define dso_local void @_Z5chaini(ptr dead_on_unwind noalias writable sret(%struct.task) align 8 %0, i32 noundef %1) #0 personality ptr @__gxx_personality_v0 {
  %3 = alloca ptr, align 8
  %4 = alloca i32, align 4
  %5 = alloca i32, align 4
  %6 = alloca %"struct.task::promise_type", align 8
  %7 = alloca ptr, align 8
  %8 = alloca i32, align 4
  %9 = alloca %"struct.std::__n4861::suspend_always", align 1
  %10 = alloca %"struct.std::__n4861::suspend_always", align 1
  %11 = alloca %struct.Dtor, align 4
  %12 = alloca %struct.task, align 8
  %13 = alloca i32, align 4
  %14 = alloca %struct.task, align 8
  %15 = alloca %struct.task, align 8
  %16 = alloca %"struct.task::promise_type::fin", align 1
  %17 = alloca %"struct.task::promise_type::fin", align 1
  store ptr %0, ptr %3, align 8
  store i32 %1, ptr %4, align 4
  %18 = call token @llvm.coro.id(i32 16, ptr %6, ptr null, ptr null)
  %19 = call i1 @llvm.coro.alloc(token %18)
  br i1 %19, label %20, label %23

20:                                               ; preds = %2
  %21 = call i64 @llvm.coro.size.i64()
  %22 = call noalias noundef nonnull ptr @_Znwm(i64 noundef %21) #16
  br label %23

23:                                               ; preds = %20, %2
  %24 = phi ptr [ null, %2 ], [ %22, %20 ]
  %25 = call ptr @llvm.coro.begin(token %18, ptr %24)
  call void @llvm.lifetime.start.p0(ptr %5) #2
  %26 = load i32, ptr %4, align 4
  store i32 %26, ptr %5, align 4
  call void @llvm.lifetime.start.p0(ptr %6) #2
  call void @llvm.memset.p0.i64(ptr align 8 %6, i8 0, i64 8, i1 false), !annotation !6
  call void @_ZN4task12promise_typeC2Ev(ptr noundef nonnull align 8 dereferenceable(8) %6) #2
  invoke void @_ZN4task12promise_type17get_return_objectEv(ptr dead_on_unwind writable sret(%struct.task) align 8 %0, ptr noundef nonnull align 8 dereferenceable(8) %6)
          to label %27 unwind label %33

27:                                               ; preds = %23
  call void @llvm.lifetime.start.p0(ptr %9) #2
  call void @_ZN4task12promise_type15initial_suspendEv(ptr noundef nonnull align 8 dereferenceable(8) %6) #2
  %28 = call noundef i1 @_ZNKSt7__n486114suspend_always11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %9) #2
  br i1 %28, label %37, label %29

29:                                               ; preds = %27
  %30 = call token @llvm.coro.save(ptr null)
  call void @llvm.coro.await.suspend.void(ptr %9, ptr %25, ptr @_Z5chaini.__await_suspend_wrapper__init) #2
  %31 = call i8 @llvm.coro.suspend(token %30, i1 false)
  switch i8 %31, label %180 [
    i8 0, label %37
    i8 1, label %32
  ]

32:                                               ; preds = %29
  br label %38

33:                                               ; preds = %23
  %34 = landingpad { ptr, i32 }
          cleanup
  %35 = extractvalue { ptr, i32 } %34, 0
  store ptr %35, ptr %7, align 8
  %36 = extractvalue { ptr, i32 } %34, 1
  store i32 %36, ptr %8, align 4
  br label %184

37:                                               ; preds = %29, %27
  call void @_ZNKSt7__n486114suspend_always12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %9) #2
  br label %38

38:                                               ; preds = %37, %32
  %39 = phi i32 [ 0, %37 ], [ 2, %32 ]
  call void @llvm.lifetime.end.p0(ptr %9) #2
  switch i32 %39, label %172 [
    i32 0, label %40
  ]

40:                                               ; preds = %38
  call void @llvm.lifetime.start.p0(ptr %11) #2
  call void @llvm.memset.p0.i64(ptr align 4 %11, i8 0, i64 4, i1 false), !annotation !6
  %41 = getelementptr inbounds nuw %struct.Dtor, ptr %11, i32 0, i32 0
  %42 = load i32, ptr %5, align 4
  %43 = add nsw i32 100, %42
  store i32 %43, ptr %41, align 4
  %44 = load i32, ptr %5, align 4
  %45 = icmp eq i32 %44, 0
  br i1 %45, label %46, label %74

46:                                               ; preds = %40
  call void @llvm.lifetime.start.p0(ptr %12) #2
  invoke void @_Z4leafi(ptr dead_on_unwind writable sret(%struct.task) align 8 %12, i32 noundef 3)
          to label %47 unwind label %55

47:                                               ; preds = %46
  %48 = invoke noundef i1 @_ZN4task11await_readyEv(ptr noundef nonnull align 8 dereferenceable(8) %12)
          to label %49 unwind label %59

49:                                               ; preds = %47
  br i1 %48, label %63, label %50

50:                                               ; preds = %49
  %51 = call token @llvm.coro.save(ptr null)
  invoke void @llvm.coro.await.suspend.handle(ptr %12, ptr %25, ptr @_Z5chaini.__await_suspend_wrapper__await)
          to label %52 unwind label %59

52:                                               ; preds = %50
  %53 = call i8 @llvm.coro.suspend(token %51, i1 false)
  switch i8 %53, label %180 [
    i8 0, label %63
    i8 1, label %54
  ]

54:                                               ; preds = %52
  br label %65

55:                                               ; preds = %46
  %56 = landingpad { ptr, i32 }
          catch ptr null
  %57 = extractvalue { ptr, i32 } %56, 0
  store ptr %57, ptr %7, align 8
  %58 = extractvalue { ptr, i32 } %56, 1
  store i32 %58, ptr %8, align 4
  br label %69

59:                                               ; preds = %63, %50, %47
  %60 = landingpad { ptr, i32 }
          catch ptr null
  %61 = extractvalue { ptr, i32 } %60, 0
  store ptr %61, ptr %7, align 8
  %62 = extractvalue { ptr, i32 } %60, 1
  store i32 %62, ptr %8, align 4
  call void @_ZN4taskD2Ev(ptr noundef nonnull align 8 dereferenceable(8) %12) #2
  br label %69

63:                                               ; preds = %52, %49
  invoke void @_ZN4task12await_resumeEv(ptr noundef nonnull align 8 dereferenceable(8) %12)
          to label %64 unwind label %59

64:                                               ; preds = %63
  br label %65

65:                                               ; preds = %64, %54
  %66 = phi i32 [ 0, %64 ], [ 2, %54 ]
  call void @_ZN4taskD2Ev(ptr noundef nonnull align 8 dereferenceable(8) %12) #2
  call void @llvm.lifetime.end.p0(ptr %12) #2
  switch i32 %66, label %132 [
    i32 0, label %67
  ]

67:                                               ; preds = %65
  invoke void @_ZN4task12promise_type11return_voidEv(ptr noundef nonnull align 8 dereferenceable(8) %6)
          to label %68 unwind label %70

68:                                               ; preds = %67
  br label %132

69:                                               ; preds = %59, %55
  call void @llvm.lifetime.end.p0(ptr %12) #2
  br label %137

70:                                               ; preds = %67
  %71 = landingpad { ptr, i32 }
          catch ptr null
  %72 = extractvalue { ptr, i32 } %71, 0
  store ptr %72, ptr %7, align 8
  %73 = extractvalue { ptr, i32 } %71, 1
  store i32 %73, ptr %8, align 4
  br label %137

74:                                               ; preds = %40
  call void @llvm.lifetime.start.p0(ptr %13) #2
  store i32 0, ptr %13, align 4
  br label %75

75:                                               ; preds = %102, %74
  %76 = load i32, ptr %13, align 4
  %77 = icmp slt i32 %76, 2
  br i1 %77, label %79, label %78

78:                                               ; preds = %75
  br label %106

79:                                               ; preds = %75
  call void @llvm.lifetime.start.p0(ptr %14) #2
  %80 = load i32, ptr %13, align 4
  invoke void @_Z4leafi(ptr dead_on_unwind writable sret(%struct.task) align 8 %14, i32 noundef %80)
          to label %81 unwind label %89

81:                                               ; preds = %79
  %82 = invoke noundef i1 @_ZN4task11await_readyEv(ptr noundef nonnull align 8 dereferenceable(8) %14)
          to label %83 unwind label %93

83:                                               ; preds = %81
  br i1 %82, label %97, label %84

84:                                               ; preds = %83
  %85 = call token @llvm.coro.save(ptr null)
  invoke void @llvm.coro.await.suspend.handle(ptr %14, ptr %25, ptr @_Z5chaini.__await_suspend_wrapper__await2)
          to label %86 unwind label %93

86:                                               ; preds = %84
  %87 = call i8 @llvm.coro.suspend(token %85, i1 false)
  switch i8 %87, label %180 [
    i8 0, label %97
    i8 1, label %88
  ]

88:                                               ; preds = %86
  br label %99

89:                                               ; preds = %79
  %90 = landingpad { ptr, i32 }
          catch ptr null
  %91 = extractvalue { ptr, i32 } %90, 0
  store ptr %91, ptr %7, align 8
  %92 = extractvalue { ptr, i32 } %90, 1
  store i32 %92, ptr %8, align 4
  br label %105

93:                                               ; preds = %97, %84, %81
  %94 = landingpad { ptr, i32 }
          catch ptr null
  %95 = extractvalue { ptr, i32 } %94, 0
  store ptr %95, ptr %7, align 8
  %96 = extractvalue { ptr, i32 } %94, 1
  store i32 %96, ptr %8, align 4
  call void @_ZN4taskD2Ev(ptr noundef nonnull align 8 dereferenceable(8) %14) #2
  br label %105

97:                                               ; preds = %86, %83
  invoke void @_ZN4task12await_resumeEv(ptr noundef nonnull align 8 dereferenceable(8) %14)
          to label %98 unwind label %93

98:                                               ; preds = %97
  br label %99

99:                                               ; preds = %98, %88
  %100 = phi i32 [ 0, %98 ], [ 2, %88 ]
  call void @_ZN4taskD2Ev(ptr noundef nonnull align 8 dereferenceable(8) %14) #2
  call void @llvm.lifetime.end.p0(ptr %14) #2
  switch i32 %100, label %106 [
    i32 0, label %101
  ]

101:                                              ; preds = %99
  br label %102

102:                                              ; preds = %101
  %103 = load i32, ptr %13, align 4
  %104 = add nsw i32 %103, 1
  store i32 %104, ptr %13, align 4
  br label %75, !llvm.loop !7

105:                                              ; preds = %93, %89
  call void @llvm.lifetime.end.p0(ptr %14) #2
  call void @llvm.lifetime.end.p0(ptr %13) #2
  br label %137

106:                                              ; preds = %99, %78
  %107 = phi i32 [ %100, %99 ], [ 4, %78 ]
  call void @llvm.lifetime.end.p0(ptr %13) #2
  switch i32 %107, label %132 [
    i32 4, label %108
  ]

108:                                              ; preds = %106
  call void @llvm.lifetime.start.p0(ptr %15) #2
  %109 = load i32, ptr %5, align 4
  %110 = sub nsw i32 %109, 1
  invoke void @_Z5chaini(ptr dead_on_unwind writable sret(%struct.task) align 8 %15, i32 noundef %110)
          to label %111 unwind label %119

111:                                              ; preds = %108
  %112 = invoke noundef i1 @_ZN4task11await_readyEv(ptr noundef nonnull align 8 dereferenceable(8) %15)
          to label %113 unwind label %123

113:                                              ; preds = %111
  br i1 %112, label %127, label %114

114:                                              ; preds = %113
  %115 = call token @llvm.coro.save(ptr null)
  invoke void @llvm.coro.await.suspend.handle(ptr %15, ptr %25, ptr @_Z5chaini.__await_suspend_wrapper__await3)
          to label %116 unwind label %123

116:                                              ; preds = %114
  %117 = call i8 @llvm.coro.suspend(token %115, i1 false)
  switch i8 %117, label %180 [
    i8 0, label %127
    i8 1, label %118
  ]

118:                                              ; preds = %116
  br label %129

119:                                              ; preds = %108
  %120 = landingpad { ptr, i32 }
          catch ptr null
  %121 = extractvalue { ptr, i32 } %120, 0
  store ptr %121, ptr %7, align 8
  %122 = extractvalue { ptr, i32 } %120, 1
  store i32 %122, ptr %8, align 4
  br label %136

123:                                              ; preds = %127, %114, %111
  %124 = landingpad { ptr, i32 }
          catch ptr null
  %125 = extractvalue { ptr, i32 } %124, 0
  store ptr %125, ptr %7, align 8
  %126 = extractvalue { ptr, i32 } %124, 1
  store i32 %126, ptr %8, align 4
  call void @_ZN4taskD2Ev(ptr noundef nonnull align 8 dereferenceable(8) %15) #2
  br label %136

127:                                              ; preds = %116, %113
  invoke void @_ZN4task12await_resumeEv(ptr noundef nonnull align 8 dereferenceable(8) %15)
          to label %128 unwind label %123

128:                                              ; preds = %127
  br label %129

129:                                              ; preds = %128, %118
  %130 = phi i32 [ 0, %128 ], [ 2, %118 ]
  call void @_ZN4taskD2Ev(ptr noundef nonnull align 8 dereferenceable(8) %15) #2
  call void @llvm.lifetime.end.p0(ptr %15) #2
  switch i32 %130, label %132 [
    i32 0, label %131
  ]

131:                                              ; preds = %129
  br label %132

132:                                              ; preds = %131, %129, %106, %68, %65
  %133 = phi i32 [ %66, %65 ], [ 3, %68 ], [ %107, %106 ], [ %130, %129 ], [ 0, %131 ]
  call void @_ZN4DtorD2Ev(ptr noundef nonnull align 4 dereferenceable(4) %11) #2
  call void @llvm.lifetime.end.p0(ptr %11) #2
  switch i32 %133, label %172 [
    i32 0, label %134
    i32 3, label %148
  ]

134:                                              ; preds = %132
  invoke void @_ZN4task12promise_type11return_voidEv(ptr noundef nonnull align 8 dereferenceable(8) %6)
          to label %135 unwind label %138

135:                                              ; preds = %134
  br label %148

136:                                              ; preds = %123, %119
  call void @llvm.lifetime.end.p0(ptr %15) #2
  br label %137

137:                                              ; preds = %136, %105, %70, %69
  call void @_ZN4DtorD2Ev(ptr noundef nonnull align 4 dereferenceable(4) %11) #2
  call void @llvm.lifetime.end.p0(ptr %11) #2
  br label %142

138:                                              ; preds = %134
  %139 = landingpad { ptr, i32 }
          catch ptr null
  %140 = extractvalue { ptr, i32 } %139, 0
  store ptr %140, ptr %7, align 8
  %141 = extractvalue { ptr, i32 } %139, 1
  store i32 %141, ptr %8, align 4
  br label %142

142:                                              ; preds = %138, %137
  %143 = load ptr, ptr %7, align 8
  %144 = call ptr @__cxa_begin_catch(ptr %143) #2
  invoke void @_ZN4task12promise_type19unhandled_exceptionEv(ptr noundef nonnull align 8 dereferenceable(8) %6)
          to label %145 unwind label %155

145:                                              ; preds = %142
  invoke void @__cxa_end_catch()
          to label %146 unwind label %159

146:                                              ; preds = %145
  br label %147

147:                                              ; preds = %146
  br label %148

148:                                              ; preds = %147, %135, %132
  call void @llvm.lifetime.start.p0(ptr %16) #2
  call void @_ZN4task12promise_type13final_suspendEv(ptr noundef nonnull align 8 dereferenceable(8) %6) #2
  %149 = call noundef i1 @_ZN4task12promise_type3fin11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %16) #2
  br i1 %149, label %168, label %150

150:                                              ; preds = %148
  %151 = call token @llvm.coro.save(ptr null)
  invoke void @llvm.coro.await.suspend.handle(ptr %16, ptr %25, ptr @_Z5chaini.__await_suspend_wrapper__final)
          to label %152 unwind label %164

152:                                              ; preds = %150
  %153 = call i8 @llvm.coro.suspend(token %151, i1 true)
  switch i8 %153, label %180 [
    i8 0, label %168
    i8 1, label %154
  ]

154:                                              ; preds = %152
  br label %169

155:                                              ; preds = %142
  %156 = landingpad { ptr, i32 }
          cleanup
  %157 = extractvalue { ptr, i32 } %156, 0
  store ptr %157, ptr %7, align 8
  %158 = extractvalue { ptr, i32 } %156, 1
  store i32 %158, ptr %8, align 4
  invoke void @__cxa_end_catch()
          to label %163 unwind label %195

159:                                              ; preds = %145
  %160 = landingpad { ptr, i32 }
          cleanup
  %161 = extractvalue { ptr, i32 } %160, 0
  store ptr %161, ptr %7, align 8
  %162 = extractvalue { ptr, i32 } %160, 1
  store i32 %162, ptr %8, align 4
  br label %181

163:                                              ; preds = %155
  br label %181

164:                                              ; preds = %150
  %165 = landingpad { ptr, i32 }
          cleanup
  %166 = extractvalue { ptr, i32 } %165, 0
  store ptr %166, ptr %7, align 8
  %167 = extractvalue { ptr, i32 } %165, 1
  store i32 %167, ptr %8, align 4
  call void @llvm.lifetime.end.p0(ptr %16) #2
  br label %181

168:                                              ; preds = %152, %148
  call void @_ZN4task12promise_type3fin12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %16) #2
  br label %169

169:                                              ; preds = %168, %154
  %170 = phi i32 [ 0, %168 ], [ 2, %154 ]
  call void @llvm.lifetime.end.p0(ptr %16) #2
  switch i32 %170, label %172 [
    i32 0, label %171
  ]

171:                                              ; preds = %169
  br label %172

172:                                              ; preds = %171, %169, %132, %38
  %173 = phi i32 [ %39, %38 ], [ %133, %132 ], [ %170, %169 ], [ 0, %171 ]
  call void @llvm.lifetime.end.p0(ptr %6) #2
  call void @llvm.lifetime.end.p0(ptr %5) #2
  %174 = call ptr @llvm.coro.free(token %18, ptr %25)
  %175 = icmp ne ptr %174, null
  br i1 %175, label %176, label %178

176:                                              ; preds = %172
  %177 = call i64 @llvm.coro.size.i64()
  call void @_ZdlPvm(ptr noundef %174, i64 noundef %177) #2
  br label %178

178:                                              ; preds = %176, %172
  switch i32 %173, label %198 [
    i32 0, label %179
    i32 2, label %180
  ]

179:                                              ; preds = %178
  br label %180

180:                                              ; preds = %179, %178, %152, %116, %86, %52, %29
  call void @llvm.coro.end(ptr null, i1 false, token none)
  ret void

181:                                              ; preds = %164, %163, %159
  call void @llvm.coro.end(ptr null, i1 true, token none)
  %182 = call i1 @llvm.coro.is_in_ramp()
  br i1 %182, label %183, label %190

183:                                              ; preds = %181
  br label %184

184:                                              ; preds = %183, %33
  call void @llvm.lifetime.end.p0(ptr %6) #2
  call void @llvm.lifetime.end.p0(ptr %5) #2
  %185 = call ptr @llvm.coro.free(token %18, ptr %25)
  %186 = icmp ne ptr %185, null
  br i1 %186, label %187, label %189

187:                                              ; preds = %184
  %188 = call i64 @llvm.coro.size.i64()
  call void @_ZdlPvm(ptr noundef %185, i64 noundef %188) #2
  br label %189

189:                                              ; preds = %187, %184
  br label %190

190:                                              ; preds = %189, %181
  %191 = load ptr, ptr %7, align 8
  %192 = load i32, ptr %8, align 4
  %193 = insertvalue { ptr, i32 } poison, ptr %191, 0
  %194 = insertvalue { ptr, i32 } %193, i32 %192, 1
  resume { ptr, i32 } %194

195:                                              ; preds = %155
  %196 = landingpad { ptr, i32 }
          catch ptr null
  %197 = extractvalue { ptr, i32 } %196, 0
  call void @__clang_call_terminate(ptr %197) #18
  unreachable

198:                                              ; preds = %178
  unreachable
}

; Function Attrs: alwaysinline mustprogress
declare hidden void @_Z5chaini.__await_suspend_wrapper__init(ptr noundef nonnull, ptr noundef) #10

; Function Attrs: mustprogress noinline nounwind optnone uwtable
declare dso_local noundef i1 @_ZN4task11await_readyEv(ptr noundef nonnull align 8 dereferenceable(8)) #7

; Function Attrs: alwaysinline mustprogress
declare hidden noundef ptr @_Z5chaini.__await_suspend_wrapper__await(ptr noundef nonnull, ptr noundef) #10

; Function Attrs: mustprogress noinline nounwind optnone uwtable
declare dso_local void @_ZN4task12await_resumeEv(ptr noundef nonnull align 8 dereferenceable(8)) #7

; Function Attrs: mustprogress noinline nounwind optnone uwtable
declare dso_local void @_ZN4taskD2Ev(ptr noundef nonnull align 8 dereferenceable(8)) unnamed_addr #7

; Function Attrs: alwaysinline mustprogress
declare hidden noundef ptr @_Z5chaini.__await_suspend_wrapper__await2(ptr noundef nonnull, ptr noundef) #10

; Function Attrs: alwaysinline mustprogress
declare hidden noundef ptr @_Z5chaini.__await_suspend_wrapper__await3(ptr noundef nonnull, ptr noundef) #10

; Function Attrs: alwaysinline mustprogress
declare hidden noundef ptr @_Z5chaini.__await_suspend_wrapper__final(ptr noundef nonnull, ptr noundef) #10

attributes #0 = { mustprogress noinline optnone presplitcoroutine uwtable "frame-pointer"="non-leaf-no-reserve" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #1 = { nocallback nofree nosync nounwind willreturn memory(argmem: read) }
attributes #2 = { nounwind }
attributes #3 = { nobuiltin allocsize(0) "frame-pointer"="non-leaf-no-reserve" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #4 = { nounwind memory(none) }
attributes #5 = { nocallback nofree nosync nounwind willreturn memory(argmem: readwrite) }
attributes #6 = { nocallback nofree nounwind willreturn memory(argmem: write) }
attributes #7 = { mustprogress noinline nounwind optnone uwtable "frame-pointer"="non-leaf-no-reserve" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #8 = { mustprogress noinline optnone uwtable "frame-pointer"="non-leaf-no-reserve" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #9 = { nomerge nounwind }
attributes #10 = { alwaysinline mustprogress "sample-profile-suffix-elision-policy"="selected" }
attributes #11 = { "frame-pointer"="non-leaf-no-reserve" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #12 = { nounwind "frame-pointer"="non-leaf-no-reserve" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #13 = { noinline noreturn nounwind uwtable "frame-pointer"="non-leaf-no-reserve" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #14 = { nobuiltin nounwind "frame-pointer"="non-leaf-no-reserve" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #15 = { nounwind memory(argmem: read) }
attributes #16 = { allocsize(0) }
attributes #17 = { noreturn }
attributes #18 = { noreturn nounwind }

!llvm.module.flags = !{!0, !1, !2, !3, !4}
!llvm.ident = !{!5}

!0 = !{i32 1, !"wchar_size", i32 4}
!1 = !{i32 8, !"PIC Level", i32 2}
!2 = !{i32 7, !"PIE Level", i32 2}
!3 = !{i32 7, !"uwtable", i32 2}
!4 = !{i32 7, !"frame-pointer", i32 4}
!5 = !{!"kiln version 1.0.1"}
!6 = !{!"auto-init"}
!7 = distinct !{!7, !8}
!8 = !{!"llvm.loop.mustprogress"}
