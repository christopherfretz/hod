; ModuleID = 'resume-inside-await-suspend-22.ll'
source_filename = "docs/design/0005-data/resume-inside-await-suspend.cpp"
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i8:8:32-i16:16:32-i64:64-i128:128-n32:64-S128-Fn32"
target triple = "aarch64-unknown-linux-gnu"

%struct.noop_task = type { %"struct.std::__n4861::coroutine_handle" }
%"struct.std::__n4861::coroutine_handle" = type { ptr }
%"struct.noop_task::promise_type" = type { i8 }
%"struct.std::__n4861::suspend_always" = type { i8 }
%"struct.task::promise_type" = type { i8 }
%struct.task = type { i8 }
%"struct.std::__n4861::suspend_never" = type { i8 }
%struct.resume_now_void = type { i8 }
%struct.resume_now_bool = type { i8 }
%struct.resume_now_handle = type { i8 }

$_Z3runI15resume_now_voidE4taskPKc = comdat any

$_Z3runI15resume_now_boolE4taskPKc = comdat any

$_Z3runI17resume_now_handleE4taskPKc = comdat any

@.str.5 = external hidden unnamed_addr constant [5 x i8], align 1
@.str.6 = external hidden unnamed_addr constant [9 x i8], align 1

; Function Attrs: mustprogress nobuiltin noinline optnone allocsize(0) uwtable
declare dso_local noundef nonnull ptr @_Znwm(i64 noundef) #0

; Function Attrs: mustprogress nobuiltin noinline nounwind optnone uwtable
declare dso_local void @_ZdlPvm(ptr noundef, i64 noundef) #1

; Function Attrs: mustprogress noinline optnone presplitcoroutine uwtable
define dso_local i64 @_Z4idlev() #2 personality ptr @__gxx_personality_v0 {
  %1 = alloca %struct.noop_task, align 8
  %2 = alloca %"struct.noop_task::promise_type", align 1
  %3 = alloca ptr, align 8
  %4 = alloca i32, align 4
  %5 = alloca %"struct.std::__n4861::suspend_always", align 1
  %6 = alloca %"struct.std::__n4861::suspend_always", align 1
  %7 = alloca %"struct.std::__n4861::suspend_always", align 1
  %8 = alloca %"struct.std::__n4861::suspend_always", align 1
  %9 = call token @llvm.coro.id(i32 16, ptr %2, ptr null, ptr null)
  %10 = call i1 @llvm.coro.alloc(token %9)
  br i1 %10, label %11, label %14

11:                                               ; preds = %0
  %12 = call i64 @llvm.coro.size.i64()
  %13 = call noalias noundef nonnull ptr @_Znwm(i64 noundef %12) #15
  br label %14

14:                                               ; preds = %11, %0
  %15 = phi ptr [ null, %0 ], [ %13, %11 ]
  %16 = call ptr @llvm.coro.begin(token %9, ptr %15)
  call void @llvm.lifetime.start.p0(ptr %2) #4
  call void @llvm.memset.p0.i64(ptr align 1 %2, i8 0, i64 1, i1 false), !annotation !6
  %17 = invoke i64 @_ZN9noop_task12promise_type17get_return_objectEv(ptr noundef nonnull align 1 dereferenceable(1) %2)
          to label %18 unwind label %27

18:                                               ; preds = %14
  %19 = getelementptr inbounds nuw %struct.noop_task, ptr %1, i32 0, i32 0
  %20 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %19, i32 0, i32 0
  %21 = inttoptr i64 %17 to ptr
  store ptr %21, ptr %20, align 8
  call void @llvm.lifetime.start.p0(ptr %5) #4
  call void @_ZN9noop_task12promise_type15initial_suspendEv(ptr noundef nonnull align 1 dereferenceable(1) %2) #4
  %22 = call noundef i1 @_ZNKSt7__n486114suspend_always11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %5) #4
  br i1 %22, label %31, label %23

23:                                               ; preds = %18
  %24 = call token @llvm.coro.save(ptr null)
  call void @llvm.coro.await.suspend.void(ptr %5, ptr %16, ptr @_Z4idlev.__await_suspend_wrapper__init) #4
  %25 = call i8 @llvm.coro.suspend(token %24, i1 false)
  switch i8 %25, label %73 [
    i8 0, label %31
    i8 1, label %26
  ]

26:                                               ; preds = %23
  br label %32

27:                                               ; preds = %14
  %28 = landingpad { ptr, i32 }
          cleanup
  %29 = extractvalue { ptr, i32 } %28, 0
  store ptr %29, ptr %3, align 8
  %30 = extractvalue { ptr, i32 } %28, 1
  store i32 %30, ptr %4, align 4
  br label %81

31:                                               ; preds = %23, %18
  call void @_ZNKSt7__n486114suspend_always12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %5) #4
  br label %32

32:                                               ; preds = %31, %26
  %33 = phi i32 [ 0, %31 ], [ 2, %26 ]
  call void @llvm.lifetime.end.p0(ptr %5) #4
  switch i32 %33, label %65 [
    i32 0, label %34
  ]

34:                                               ; preds = %32
  invoke void @_ZN9noop_task12promise_type11return_voidEv(ptr noundef nonnull align 1 dereferenceable(1) %2)
          to label %35 unwind label %36

35:                                               ; preds = %34
  br label %46

36:                                               ; preds = %34
  %37 = landingpad { ptr, i32 }
          catch ptr null
  %38 = extractvalue { ptr, i32 } %37, 0
  store ptr %38, ptr %3, align 8
  %39 = extractvalue { ptr, i32 } %37, 1
  store i32 %39, ptr %4, align 4
  br label %40

40:                                               ; preds = %36
  %41 = load ptr, ptr %3, align 8
  %42 = call ptr @__cxa_begin_catch(ptr %41) #4
  invoke void @_ZN9noop_task12promise_type19unhandled_exceptionEv(ptr noundef nonnull align 1 dereferenceable(1) %2)
          to label %43 unwind label %52

43:                                               ; preds = %40
  invoke void @__cxa_end_catch()
          to label %44 unwind label %56

44:                                               ; preds = %43
  br label %45

45:                                               ; preds = %44
  br label %46

46:                                               ; preds = %45, %35
  call void @llvm.lifetime.start.p0(ptr %7) #4
  call void @_ZN9noop_task12promise_type13final_suspendEv(ptr noundef nonnull align 1 dereferenceable(1) %2) #4
  %47 = call noundef i1 @_ZNKSt7__n486114suspend_always11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %7) #4
  br i1 %47, label %61, label %48

48:                                               ; preds = %46
  %49 = call token @llvm.coro.save(ptr null)
  call void @llvm.coro.await.suspend.void(ptr %7, ptr %16, ptr @_Z4idlev.__await_suspend_wrapper__final) #4
  %50 = call i8 @llvm.coro.suspend(token %49, i1 true)
  switch i8 %50, label %73 [
    i8 0, label %61
    i8 1, label %51
  ]

51:                                               ; preds = %48
  br label %62

52:                                               ; preds = %40
  %53 = landingpad { ptr, i32 }
          cleanup
  %54 = extractvalue { ptr, i32 } %53, 0
  store ptr %54, ptr %3, align 8
  %55 = extractvalue { ptr, i32 } %53, 1
  store i32 %55, ptr %4, align 4
  invoke void @__cxa_end_catch()
          to label %60 unwind label %92

56:                                               ; preds = %43
  %57 = landingpad { ptr, i32 }
          cleanup
  %58 = extractvalue { ptr, i32 } %57, 0
  store ptr %58, ptr %3, align 8
  %59 = extractvalue { ptr, i32 } %57, 1
  store i32 %59, ptr %4, align 4
  br label %78

60:                                               ; preds = %52
  br label %78

61:                                               ; preds = %48, %46
  call void @_ZNKSt7__n486114suspend_always12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %7) #4
  br label %62

62:                                               ; preds = %61, %51
  %63 = phi i32 [ 0, %61 ], [ 2, %51 ]
  call void @llvm.lifetime.end.p0(ptr %7) #4
  switch i32 %63, label %65 [
    i32 0, label %64
  ]

64:                                               ; preds = %62
  br label %65

65:                                               ; preds = %64, %62, %32
  %66 = phi i32 [ %33, %32 ], [ %63, %62 ], [ 0, %64 ]
  call void @llvm.lifetime.end.p0(ptr %2) #4
  %67 = call ptr @llvm.coro.free(token %9, ptr %16)
  %68 = icmp ne ptr %67, null
  br i1 %68, label %69, label %71

69:                                               ; preds = %65
  %70 = call i64 @llvm.coro.size.i64()
  call void @_ZdlPvm(ptr noundef %67, i64 noundef %70) #4
  br label %71

71:                                               ; preds = %69, %65
  switch i32 %66, label %95 [
    i32 0, label %72
    i32 2, label %73
  ]

72:                                               ; preds = %71
  br label %73

73:                                               ; preds = %72, %71, %48, %23
  call void @llvm.coro.end(ptr null, i1 false, token none)
  %74 = getelementptr inbounds nuw %struct.noop_task, ptr %1, i32 0, i32 0
  %75 = getelementptr inbounds nuw %"struct.std::__n4861::coroutine_handle", ptr %74, i32 0, i32 0
  %76 = load ptr, ptr %75, align 8
  %77 = ptrtoint ptr %76 to i64
  ret i64 %77

78:                                               ; preds = %60, %56
  call void @llvm.coro.end(ptr null, i1 true, token none)
  %79 = call i1 @llvm.coro.is_in_ramp()
  br i1 %79, label %80, label %87

80:                                               ; preds = %78
  br label %81

81:                                               ; preds = %80, %27
  call void @llvm.lifetime.end.p0(ptr %2) #4
  %82 = call ptr @llvm.coro.free(token %9, ptr %16)
  %83 = icmp ne ptr %82, null
  br i1 %83, label %84, label %86

84:                                               ; preds = %81
  %85 = call i64 @llvm.coro.size.i64()
  call void @_ZdlPvm(ptr noundef %82, i64 noundef %85) #4
  br label %86

86:                                               ; preds = %84, %81
  br label %87

87:                                               ; preds = %86, %78
  %88 = load ptr, ptr %3, align 8
  %89 = load i32, ptr %4, align 4
  %90 = insertvalue { ptr, i32 } poison, ptr %88, 0
  %91 = insertvalue { ptr, i32 } %90, i32 %89, 1
  resume { ptr, i32 } %91

92:                                               ; preds = %52
  %93 = landingpad { ptr, i32 }
          catch ptr null
  %94 = extractvalue { ptr, i32 } %93, 0
  call void @__clang_call_terminate(ptr %94) #16
  unreachable

95:                                               ; preds = %71
  unreachable
}

; Function Attrs: nocallback nofree nosync nounwind willreturn memory(argmem: read)
declare token @llvm.coro.id(i32, ptr readnone, ptr readonly captures(none), ptr) #3

; Function Attrs: nounwind
declare i1 @llvm.coro.alloc(token) #4

; Function Attrs: nounwind memory(none)
declare i64 @llvm.coro.size.i64() #5

; Function Attrs: nounwind
declare ptr @llvm.coro.begin(token, ptr writeonly) #4

; Function Attrs: nocallback nofree nosync nounwind willreturn memory(argmem: readwrite)
declare void @llvm.lifetime.start.p0(ptr captures(none)) #6

; Function Attrs: nocallback nofree nounwind willreturn memory(argmem: write)
declare void @llvm.memset.p0.i64(ptr writeonly captures(none), i8, i64, i1 immarg) #7

; Function Attrs: mustprogress noinline optnone uwtable
declare dso_local i64 @_ZN9noop_task12promise_type17get_return_objectEv(ptr noundef nonnull align 1 dereferenceable(1)) #8

declare i32 @__gxx_personality_v0(...)

; Function Attrs: mustprogress noinline nounwind optnone uwtable
declare dso_local void @_ZN9noop_task12promise_type15initial_suspendEv(ptr noundef nonnull align 1 dereferenceable(1)) #9

; Function Attrs: mustprogress noinline nounwind optnone uwtable
declare dso_local noundef i1 @_ZNKSt7__n486114suspend_always11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1)) #9

; Function Attrs: nomerge nounwind
declare token @llvm.coro.save(ptr) #10

; Function Attrs: alwaysinline mustprogress
declare hidden void @_Z4idlev.__await_suspend_wrapper__init(ptr noundef nonnull, ptr noundef) #11

declare void @llvm.coro.await.suspend.void(ptr, ptr, ptr)

; Function Attrs: nounwind
declare i8 @llvm.coro.suspend(token, i1) #4

; Function Attrs: mustprogress noinline nounwind optnone uwtable
declare dso_local void @_ZNKSt7__n486114suspend_always12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1)) #9

; Function Attrs: nocallback nofree nosync nounwind willreturn memory(argmem: readwrite)
declare void @llvm.lifetime.end.p0(ptr captures(none)) #6

; Function Attrs: mustprogress noinline nounwind optnone uwtable
declare dso_local void @_ZN9noop_task12promise_type11return_voidEv(ptr noundef nonnull align 1 dereferenceable(1)) #9

declare ptr @__cxa_begin_catch(ptr)

; Function Attrs: mustprogress noinline nounwind optnone uwtable
declare dso_local void @_ZN9noop_task12promise_type19unhandled_exceptionEv(ptr noundef nonnull align 1 dereferenceable(1)) #9

declare void @__cxa_end_catch()

; Function Attrs: noinline noreturn nounwind uwtable
declare hidden void @__clang_call_terminate(ptr noundef) #12

; Function Attrs: mustprogress noinline nounwind optnone uwtable
declare dso_local void @_ZN9noop_task12promise_type13final_suspendEv(ptr noundef nonnull align 1 dereferenceable(1)) #9

; Function Attrs: alwaysinline mustprogress
declare hidden void @_Z4idlev.__await_suspend_wrapper__final(ptr noundef nonnull, ptr noundef) #11

; Function Attrs: nounwind
declare void @llvm.coro.end(ptr, i1, token) #4

; Function Attrs: nounwind memory(none)
declare i1 @llvm.coro.is_in_ramp() #5

; Function Attrs: nounwind memory(argmem: read)
declare ptr @llvm.coro.free(token, ptr readonly captures(none)) #13

; Function Attrs: mustprogress noinline optnone presplitcoroutine uwtable
define weak_odr dso_local void @_Z3runI15resume_now_voidE4taskPKc(ptr noundef %0) #2 comdat personality ptr @__gxx_personality_v0 {
  %2 = alloca ptr, align 8
  %3 = alloca ptr, align 8
  %4 = alloca %"struct.task::promise_type", align 1
  %5 = alloca ptr, align 8
  %6 = alloca i32, align 4
  %7 = alloca %struct.task, align 1
  %8 = alloca %"struct.std::__n4861::suspend_never", align 1
  %9 = alloca %"struct.std::__n4861::suspend_never", align 1
  %10 = alloca i32, align 4
  %11 = alloca i32, align 4
  %12 = alloca %struct.resume_now_void, align 1
  %13 = alloca %"struct.std::__n4861::suspend_never", align 1
  %14 = alloca %"struct.std::__n4861::suspend_never", align 1
  store ptr %0, ptr %2, align 8
  %15 = call token @llvm.coro.id(i32 16, ptr %4, ptr null, ptr null)
  %16 = call i1 @llvm.coro.alloc(token %15)
  br i1 %16, label %17, label %20

17:                                               ; preds = %1
  %18 = call i64 @llvm.coro.size.i64()
  %19 = call noalias noundef nonnull ptr @_Znwm(i64 noundef %18) #15
  br label %20

20:                                               ; preds = %17, %1
  %21 = phi ptr [ null, %1 ], [ %19, %17 ]
  %22 = call ptr @llvm.coro.begin(token %15, ptr %21)
  call void @llvm.lifetime.start.p0(ptr %3) #4
  %23 = load ptr, ptr %2, align 8
  store ptr %23, ptr %3, align 8
  call void @llvm.lifetime.start.p0(ptr %4) #4
  call void @llvm.memset.p0.i64(ptr align 1 %4, i8 0, i64 1, i1 false), !annotation !6
  invoke void @_ZN4task12promise_type17get_return_objectEv(ptr noundef nonnull align 1 dereferenceable(1) %4)
          to label %24 unwind label %30

24:                                               ; preds = %20
  call void @llvm.lifetime.start.p0(ptr %8) #4
  call void @_ZN4task12promise_type15initial_suspendEv(ptr noundef nonnull align 1 dereferenceable(1) %4) #4
  %25 = call noundef i1 @_ZNKSt7__n486113suspend_never11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %8) #4
  br i1 %25, label %34, label %26

26:                                               ; preds = %24
  %27 = call token @llvm.coro.save(ptr null)
  call void @llvm.coro.await.suspend.void(ptr %8, ptr %22, ptr @_Z3runI15resume_now_voidE4taskPKc.__await_suspend_wrapper__init) #4
  %28 = call i8 @llvm.coro.suspend(token %27, i1 false)
  switch i8 %28, label %108 [
    i8 0, label %34
    i8 1, label %29
  ]

29:                                               ; preds = %26
  br label %35

30:                                               ; preds = %20
  %31 = landingpad { ptr, i32 }
          cleanup
  %32 = extractvalue { ptr, i32 } %31, 0
  store ptr %32, ptr %5, align 8
  %33 = extractvalue { ptr, i32 } %31, 1
  store i32 %33, ptr %6, align 4
  br label %112

34:                                               ; preds = %26, %24
  call void @_ZNKSt7__n486113suspend_never12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %8) #4
  br label %35

35:                                               ; preds = %34, %29
  %36 = phi i32 [ 0, %34 ], [ 2, %29 ]
  call void @llvm.lifetime.end.p0(ptr %8) #4
  switch i32 %36, label %100 [
    i32 0, label %37
  ]

37:                                               ; preds = %35
  call void @llvm.lifetime.start.p0(ptr %10) #4
  store i32 1, ptr %10, align 4
  call void @llvm.lifetime.start.p0(ptr %11) #4
  store i32 2, ptr %11, align 4
  %38 = load ptr, ptr %3, align 8
  %39 = invoke i32 (ptr, ...) @printf(ptr noundef @.str.5, ptr noundef %38)
          to label %40 unwind label %48

40:                                               ; preds = %37
  call void @llvm.lifetime.start.p0(ptr %12) #4
  %41 = invoke noundef i1 @_ZN15resume_now_void11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %12)
          to label %42 unwind label %52

42:                                               ; preds = %40
  br i1 %41, label %56, label %43

43:                                               ; preds = %42
  %44 = call token @llvm.coro.save(ptr null)
  invoke void @llvm.coro.await.suspend.void(ptr %12, ptr %22, ptr @_Z3runI15resume_now_voidE4taskPKc.__await_suspend_wrapper__await)
          to label %45 unwind label %52

45:                                               ; preds = %43
  %46 = call i8 @llvm.coro.suspend(token %44, i1 false)
  switch i8 %46, label %108 [
    i8 0, label %56
    i8 1, label %47
  ]

47:                                               ; preds = %45
  br label %58

48:                                               ; preds = %60, %37
  %49 = landingpad { ptr, i32 }
          catch ptr null
  %50 = extractvalue { ptr, i32 } %49, 0
  store ptr %50, ptr %5, align 8
  %51 = extractvalue { ptr, i32 } %49, 1
  store i32 %51, ptr %6, align 4
  br label %70

52:                                               ; preds = %56, %43, %40
  %53 = landingpad { ptr, i32 }
          catch ptr null
  %54 = extractvalue { ptr, i32 } %53, 0
  store ptr %54, ptr %5, align 8
  %55 = extractvalue { ptr, i32 } %53, 1
  store i32 %55, ptr %6, align 4
  call void @llvm.lifetime.end.p0(ptr %12) #4
  br label %70

56:                                               ; preds = %45, %42
  invoke void @_ZN15resume_now_void12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %12)
          to label %57 unwind label %52

57:                                               ; preds = %56
  br label %58

58:                                               ; preds = %57, %47
  %59 = phi i32 [ 0, %57 ], [ 2, %47 ]
  call void @llvm.lifetime.end.p0(ptr %12) #4
  switch i32 %59, label %66 [
    i32 0, label %60
  ]

60:                                               ; preds = %58
  %61 = load i32, ptr %10, align 4
  %62 = load i32, ptr %11, align 4
  %63 = add nsw i32 %61, %62
  %64 = invoke i32 (ptr, ...) @printf(ptr noundef @.str.6, i32 noundef %63)
          to label %65 unwind label %48

65:                                               ; preds = %60
  br label %66

66:                                               ; preds = %65, %58
  %67 = phi i32 [ %59, %58 ], [ 0, %65 ]
  call void @llvm.lifetime.end.p0(ptr %11) #4
  call void @llvm.lifetime.end.p0(ptr %10) #4
  switch i32 %67, label %100 [
    i32 0, label %68
  ]

68:                                               ; preds = %66
  invoke void @_ZN4task12promise_type11return_voidEv(ptr noundef nonnull align 1 dereferenceable(1) %4)
          to label %69 unwind label %71

69:                                               ; preds = %68
  br label %81

70:                                               ; preds = %52, %48
  call void @llvm.lifetime.end.p0(ptr %11) #4
  call void @llvm.lifetime.end.p0(ptr %10) #4
  br label %75

71:                                               ; preds = %68
  %72 = landingpad { ptr, i32 }
          catch ptr null
  %73 = extractvalue { ptr, i32 } %72, 0
  store ptr %73, ptr %5, align 8
  %74 = extractvalue { ptr, i32 } %72, 1
  store i32 %74, ptr %6, align 4
  br label %75

75:                                               ; preds = %71, %70
  %76 = load ptr, ptr %5, align 8
  %77 = call ptr @__cxa_begin_catch(ptr %76) #4
  invoke void @_ZN4task12promise_type19unhandled_exceptionEv(ptr noundef nonnull align 1 dereferenceable(1) %4)
          to label %78 unwind label %87

78:                                               ; preds = %75
  invoke void @__cxa_end_catch()
          to label %79 unwind label %91

79:                                               ; preds = %78
  br label %80

80:                                               ; preds = %79
  br label %81

81:                                               ; preds = %80, %69
  call void @llvm.lifetime.start.p0(ptr %13) #4
  call void @_ZN4task12promise_type13final_suspendEv(ptr noundef nonnull align 1 dereferenceable(1) %4) #4
  %82 = call noundef i1 @_ZNKSt7__n486113suspend_never11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %13) #4
  br i1 %82, label %96, label %83

83:                                               ; preds = %81
  %84 = call token @llvm.coro.save(ptr null)
  call void @llvm.coro.await.suspend.void(ptr %13, ptr %22, ptr @_Z3runI15resume_now_voidE4taskPKc.__await_suspend_wrapper__final) #4
  %85 = call i8 @llvm.coro.suspend(token %84, i1 true)
  switch i8 %85, label %108 [
    i8 0, label %96
    i8 1, label %86
  ]

86:                                               ; preds = %83
  br label %97

87:                                               ; preds = %75
  %88 = landingpad { ptr, i32 }
          cleanup
  %89 = extractvalue { ptr, i32 } %88, 0
  store ptr %89, ptr %5, align 8
  %90 = extractvalue { ptr, i32 } %88, 1
  store i32 %90, ptr %6, align 4
  invoke void @__cxa_end_catch()
          to label %95 unwind label %123

91:                                               ; preds = %78
  %92 = landingpad { ptr, i32 }
          cleanup
  %93 = extractvalue { ptr, i32 } %92, 0
  store ptr %93, ptr %5, align 8
  %94 = extractvalue { ptr, i32 } %92, 1
  store i32 %94, ptr %6, align 4
  br label %109

95:                                               ; preds = %87
  br label %109

96:                                               ; preds = %83, %81
  call void @_ZNKSt7__n486113suspend_never12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %13) #4
  br label %97

97:                                               ; preds = %96, %86
  %98 = phi i32 [ 0, %96 ], [ 2, %86 ]
  call void @llvm.lifetime.end.p0(ptr %13) #4
  switch i32 %98, label %100 [
    i32 0, label %99
  ]

99:                                               ; preds = %97
  br label %100

100:                                              ; preds = %99, %97, %66, %35
  %101 = phi i32 [ %36, %35 ], [ %67, %66 ], [ %98, %97 ], [ 0, %99 ]
  call void @llvm.lifetime.end.p0(ptr %4) #4
  call void @llvm.lifetime.end.p0(ptr %3) #4
  %102 = call ptr @llvm.coro.free(token %15, ptr %22)
  %103 = icmp ne ptr %102, null
  br i1 %103, label %104, label %106

104:                                              ; preds = %100
  %105 = call i64 @llvm.coro.size.i64()
  call void @_ZdlPvm(ptr noundef %102, i64 noundef %105) #4
  br label %106

106:                                              ; preds = %104, %100
  switch i32 %101, label %126 [
    i32 0, label %107
    i32 2, label %108
  ]

107:                                              ; preds = %106
  br label %108

108:                                              ; preds = %107, %106, %83, %45, %26
  call void @llvm.coro.end(ptr null, i1 false, token none)
  ret void

109:                                              ; preds = %95, %91
  call void @llvm.coro.end(ptr null, i1 true, token none)
  %110 = call i1 @llvm.coro.is_in_ramp()
  br i1 %110, label %111, label %118

111:                                              ; preds = %109
  br label %112

112:                                              ; preds = %111, %30
  call void @llvm.lifetime.end.p0(ptr %4) #4
  call void @llvm.lifetime.end.p0(ptr %3) #4
  %113 = call ptr @llvm.coro.free(token %15, ptr %22)
  %114 = icmp ne ptr %113, null
  br i1 %114, label %115, label %117

115:                                              ; preds = %112
  %116 = call i64 @llvm.coro.size.i64()
  call void @_ZdlPvm(ptr noundef %113, i64 noundef %116) #4
  br label %117

117:                                              ; preds = %115, %112
  br label %118

118:                                              ; preds = %117, %109
  %119 = load ptr, ptr %5, align 8
  %120 = load i32, ptr %6, align 4
  %121 = insertvalue { ptr, i32 } poison, ptr %119, 0
  %122 = insertvalue { ptr, i32 } %121, i32 %120, 1
  resume { ptr, i32 } %122

123:                                              ; preds = %87
  %124 = landingpad { ptr, i32 }
          catch ptr null
  %125 = extractvalue { ptr, i32 } %124, 0
  call void @__clang_call_terminate(ptr %125) #16
  unreachable

126:                                              ; preds = %106
  unreachable
}

; Function Attrs: mustprogress noinline optnone presplitcoroutine uwtable
define weak_odr dso_local void @_Z3runI15resume_now_boolE4taskPKc(ptr noundef %0) #2 comdat personality ptr @__gxx_personality_v0 {
  %2 = alloca ptr, align 8
  %3 = alloca ptr, align 8
  %4 = alloca %"struct.task::promise_type", align 1
  %5 = alloca %struct.task, align 1
  %6 = alloca %"struct.std::__n4861::suspend_never", align 1
  %7 = alloca %"struct.std::__n4861::suspend_never", align 1
  %8 = alloca i32, align 4
  %9 = alloca i32, align 4
  %10 = alloca ptr, align 8
  %11 = alloca i32, align 4
  %12 = alloca %struct.resume_now_bool, align 1
  %13 = alloca %"struct.std::__n4861::suspend_never", align 1
  %14 = alloca %"struct.std::__n4861::suspend_never", align 1
  store ptr %0, ptr %2, align 8
  %15 = call token @llvm.coro.id(i32 16, ptr %4, ptr null, ptr null)
  %16 = call i1 @llvm.coro.alloc(token %15)
  br i1 %16, label %17, label %20

17:                                               ; preds = %1
  %18 = call i64 @llvm.coro.size.i64()
  %19 = call noalias noundef nonnull ptr @_Znwm(i64 noundef %18) #15
  br label %20

20:                                               ; preds = %17, %1
  %21 = phi ptr [ null, %1 ], [ %19, %17 ]
  %22 = call ptr @llvm.coro.begin(token %15, ptr %21)
  call void @llvm.lifetime.start.p0(ptr %3) #4
  %23 = load ptr, ptr %2, align 8
  store ptr %23, ptr %3, align 8
  call void @llvm.lifetime.start.p0(ptr %4) #4
  call void @llvm.memset.p0.i64(ptr align 1 %4, i8 0, i64 1, i1 false), !annotation !6
  call void @_ZN4task12promise_type17get_return_objectEv(ptr noundef nonnull align 1 dereferenceable(1) %4)
  call void @llvm.lifetime.start.p0(ptr %6) #4
  call void @_ZN4task12promise_type15initial_suspendEv(ptr noundef nonnull align 1 dereferenceable(1) %4) #4
  %24 = call noundef i1 @_ZNKSt7__n486113suspend_never11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %6) #4
  br i1 %24, label %29, label %25

25:                                               ; preds = %20
  %26 = call token @llvm.coro.save(ptr null)
  call void @llvm.coro.await.suspend.void(ptr %6, ptr %22, ptr @_Z3runI15resume_now_boolE4taskPKc.__await_suspend_wrapper__init) #4
  %27 = call i8 @llvm.coro.suspend(token %26, i1 false)
  switch i8 %27, label %95 [
    i8 0, label %29
    i8 1, label %28
  ]

28:                                               ; preds = %25
  br label %30

29:                                               ; preds = %25, %20
  call void @_ZNKSt7__n486113suspend_never12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %6) #4
  br label %30

30:                                               ; preds = %29, %28
  %31 = phi i32 [ 0, %29 ], [ 2, %28 ]
  call void @llvm.lifetime.end.p0(ptr %6) #4
  switch i32 %31, label %87 [
    i32 0, label %32
  ]

32:                                               ; preds = %30
  call void @llvm.lifetime.start.p0(ptr %8) #4
  store i32 1, ptr %8, align 4
  call void @llvm.lifetime.start.p0(ptr %9) #4
  store i32 2, ptr %9, align 4
  %33 = load ptr, ptr %3, align 8
  %34 = invoke i32 (ptr, ...) @printf(ptr noundef @.str.5, ptr noundef %33)
          to label %35 unwind label %45

35:                                               ; preds = %32
  call void @llvm.lifetime.start.p0(ptr %12) #4
  %36 = invoke noundef i1 @_ZN15resume_now_bool11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %12)
          to label %37 unwind label %49

37:                                               ; preds = %35
  br i1 %36, label %53, label %38

38:                                               ; preds = %37
  %39 = call token @llvm.coro.save(ptr null)
  %40 = invoke i1 @llvm.coro.await.suspend.bool(ptr %12, ptr %22, ptr @_Z3runI15resume_now_boolE4taskPKc.__await_suspend_wrapper__await)
          to label %41 unwind label %49

41:                                               ; preds = %38
  br i1 %40, label %42, label %53

42:                                               ; preds = %41
  %43 = call i8 @llvm.coro.suspend(token %39, i1 false)
  switch i8 %43, label %95 [
    i8 0, label %53
    i8 1, label %44
  ]

44:                                               ; preds = %42
  br label %55

45:                                               ; preds = %57, %32
  %46 = landingpad { ptr, i32 }
          catch ptr null
  %47 = extractvalue { ptr, i32 } %46, 0
  store ptr %47, ptr %10, align 8
  %48 = extractvalue { ptr, i32 } %46, 1
  store i32 %48, ptr %11, align 4
  br label %66

49:                                               ; preds = %53, %38, %35
  %50 = landingpad { ptr, i32 }
          catch ptr null
  %51 = extractvalue { ptr, i32 } %50, 0
  store ptr %51, ptr %10, align 8
  %52 = extractvalue { ptr, i32 } %50, 1
  store i32 %52, ptr %11, align 4
  call void @llvm.lifetime.end.p0(ptr %12) #4
  br label %66

53:                                               ; preds = %42, %41, %37
  invoke void @_ZN15resume_now_bool12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %12)
          to label %54 unwind label %49

54:                                               ; preds = %53
  br label %55

55:                                               ; preds = %54, %44
  %56 = phi i32 [ 0, %54 ], [ 2, %44 ]
  call void @llvm.lifetime.end.p0(ptr %12) #4
  switch i32 %56, label %63 [
    i32 0, label %57
  ]

57:                                               ; preds = %55
  %58 = load i32, ptr %8, align 4
  %59 = load i32, ptr %9, align 4
  %60 = add nsw i32 %58, %59
  %61 = invoke i32 (ptr, ...) @printf(ptr noundef @.str.6, i32 noundef %60)
          to label %62 unwind label %45

62:                                               ; preds = %57
  br label %63

63:                                               ; preds = %62, %55
  %64 = phi i32 [ %56, %55 ], [ 0, %62 ]
  call void @llvm.lifetime.end.p0(ptr %9) #4
  call void @llvm.lifetime.end.p0(ptr %8) #4
  switch i32 %64, label %87 [
    i32 0, label %65
  ]

65:                                               ; preds = %63
  call void @_ZN4task12promise_type11return_voidEv(ptr noundef nonnull align 1 dereferenceable(1) %4)
  br label %72

66:                                               ; preds = %49, %45
  call void @llvm.lifetime.end.p0(ptr %9) #4
  call void @llvm.lifetime.end.p0(ptr %8) #4
  br label %67

67:                                               ; preds = %66
  %68 = load ptr, ptr %10, align 8
  %69 = call ptr @__cxa_begin_catch(ptr %68) #4
  call void @_ZN4task12promise_type19unhandled_exceptionEv(ptr noundef nonnull align 1 dereferenceable(1) %4)
  invoke void @__cxa_end_catch()
          to label %70 unwind label %78

70:                                               ; preds = %67
  br label %71

71:                                               ; preds = %70
  br label %72

72:                                               ; preds = %71, %65
  call void @llvm.lifetime.start.p0(ptr %13) #4
  call void @_ZN4task12promise_type13final_suspendEv(ptr noundef nonnull align 1 dereferenceable(1) %4) #4
  %73 = call noundef i1 @_ZNKSt7__n486113suspend_never11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %13) #4
  br i1 %73, label %83, label %74

74:                                               ; preds = %72
  %75 = call token @llvm.coro.save(ptr null)
  call void @llvm.coro.await.suspend.void(ptr %13, ptr %22, ptr @_Z3runI15resume_now_boolE4taskPKc.__await_suspend_wrapper__final) #4
  %76 = call i8 @llvm.coro.suspend(token %75, i1 true)
  switch i8 %76, label %95 [
    i8 0, label %83
    i8 1, label %77
  ]

77:                                               ; preds = %74
  br label %84

78:                                               ; preds = %67
  %79 = landingpad { ptr, i32 }
          cleanup
  %80 = extractvalue { ptr, i32 } %79, 0
  store ptr %80, ptr %10, align 8
  %81 = extractvalue { ptr, i32 } %79, 1
  store i32 %81, ptr %11, align 4
  call void @llvm.coro.end(ptr null, i1 true, token none)
  %82 = call i1 @llvm.coro.is_in_ramp()
  br i1 %82, label %96, label %102

83:                                               ; preds = %74, %72
  call void @_ZNKSt7__n486113suspend_never12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %13) #4
  br label %84

84:                                               ; preds = %83, %77
  %85 = phi i32 [ 0, %83 ], [ 2, %77 ]
  call void @llvm.lifetime.end.p0(ptr %13) #4
  switch i32 %85, label %87 [
    i32 0, label %86
  ]

86:                                               ; preds = %84
  br label %87

87:                                               ; preds = %86, %84, %63, %30
  %88 = phi i32 [ %31, %30 ], [ %64, %63 ], [ %85, %84 ], [ 0, %86 ]
  call void @llvm.lifetime.end.p0(ptr %4) #4
  call void @llvm.lifetime.end.p0(ptr %3) #4
  %89 = call ptr @llvm.coro.free(token %15, ptr %22)
  %90 = icmp ne ptr %89, null
  br i1 %90, label %91, label %93

91:                                               ; preds = %87
  %92 = call i64 @llvm.coro.size.i64()
  call void @_ZdlPvm(ptr noundef %89, i64 noundef %92) #4
  br label %93

93:                                               ; preds = %91, %87
  switch i32 %88, label %107 [
    i32 0, label %94
    i32 2, label %95
  ]

94:                                               ; preds = %93
  br label %95

95:                                               ; preds = %94, %93, %74, %42, %25
  call void @llvm.coro.end(ptr null, i1 false, token none)
  ret void

96:                                               ; preds = %78
  call void @llvm.lifetime.end.p0(ptr %4) #4
  call void @llvm.lifetime.end.p0(ptr %3) #4
  %97 = call ptr @llvm.coro.free(token %15, ptr %22)
  %98 = icmp ne ptr %97, null
  br i1 %98, label %99, label %101

99:                                               ; preds = %96
  %100 = call i64 @llvm.coro.size.i64()
  call void @_ZdlPvm(ptr noundef %97, i64 noundef %100) #4
  br label %101

101:                                              ; preds = %99, %96
  br label %102

102:                                              ; preds = %101, %78
  %103 = load ptr, ptr %10, align 8
  %104 = load i32, ptr %11, align 4
  %105 = insertvalue { ptr, i32 } poison, ptr %103, 0
  %106 = insertvalue { ptr, i32 } %105, i32 %104, 1
  resume { ptr, i32 } %106

107:                                              ; preds = %93
  unreachable
}

; Function Attrs: mustprogress noinline optnone presplitcoroutine uwtable
define weak_odr dso_local void @_Z3runI17resume_now_handleE4taskPKc(ptr noundef %0) #2 comdat personality ptr @__gxx_personality_v0 {
  %2 = alloca ptr, align 8
  %3 = alloca ptr, align 8
  %4 = alloca %"struct.task::promise_type", align 1
  %5 = alloca %struct.task, align 1
  %6 = alloca %"struct.std::__n4861::suspend_never", align 1
  %7 = alloca %"struct.std::__n4861::suspend_never", align 1
  %8 = alloca i32, align 4
  %9 = alloca i32, align 4
  %10 = alloca ptr, align 8
  %11 = alloca i32, align 4
  %12 = alloca %struct.resume_now_handle, align 1
  %13 = alloca %"struct.std::__n4861::suspend_never", align 1
  %14 = alloca %"struct.std::__n4861::suspend_never", align 1
  store ptr %0, ptr %2, align 8
  %15 = call token @llvm.coro.id(i32 16, ptr %4, ptr null, ptr null)
  %16 = call i1 @llvm.coro.alloc(token %15)
  br i1 %16, label %17, label %20

17:                                               ; preds = %1
  %18 = call i64 @llvm.coro.size.i64()
  %19 = call noalias noundef nonnull ptr @_Znwm(i64 noundef %18) #15
  br label %20

20:                                               ; preds = %17, %1
  %21 = phi ptr [ null, %1 ], [ %19, %17 ]
  %22 = call ptr @llvm.coro.begin(token %15, ptr %21)
  call void @llvm.lifetime.start.p0(ptr %3) #4
  %23 = load ptr, ptr %2, align 8
  store ptr %23, ptr %3, align 8
  call void @llvm.lifetime.start.p0(ptr %4) #4
  call void @llvm.memset.p0.i64(ptr align 1 %4, i8 0, i64 1, i1 false), !annotation !6
  call void @_ZN4task12promise_type17get_return_objectEv(ptr noundef nonnull align 1 dereferenceable(1) %4)
  call void @llvm.lifetime.start.p0(ptr %6) #4
  call void @_ZN4task12promise_type15initial_suspendEv(ptr noundef nonnull align 1 dereferenceable(1) %4) #4
  %24 = call noundef i1 @_ZNKSt7__n486113suspend_never11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %6) #4
  br i1 %24, label %29, label %25

25:                                               ; preds = %20
  %26 = call token @llvm.coro.save(ptr null)
  call void @llvm.coro.await.suspend.void(ptr %6, ptr %22, ptr @_Z3runI17resume_now_handleE4taskPKc.__await_suspend_wrapper__init) #4
  %27 = call i8 @llvm.coro.suspend(token %26, i1 false)
  switch i8 %27, label %93 [
    i8 0, label %29
    i8 1, label %28
  ]

28:                                               ; preds = %25
  br label %30

29:                                               ; preds = %25, %20
  call void @_ZNKSt7__n486113suspend_never12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %6) #4
  br label %30

30:                                               ; preds = %29, %28
  %31 = phi i32 [ 0, %29 ], [ 2, %28 ]
  call void @llvm.lifetime.end.p0(ptr %6) #4
  switch i32 %31, label %85 [
    i32 0, label %32
  ]

32:                                               ; preds = %30
  call void @llvm.lifetime.start.p0(ptr %8) #4
  store i32 1, ptr %8, align 4
  call void @llvm.lifetime.start.p0(ptr %9) #4
  store i32 2, ptr %9, align 4
  %33 = load ptr, ptr %3, align 8
  %34 = invoke i32 (ptr, ...) @printf(ptr noundef @.str.5, ptr noundef %33)
          to label %35 unwind label %43

35:                                               ; preds = %32
  call void @llvm.lifetime.start.p0(ptr %12) #4
  %36 = invoke noundef i1 @_ZN17resume_now_handle11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %12)
          to label %37 unwind label %47

37:                                               ; preds = %35
  br i1 %36, label %51, label %38

38:                                               ; preds = %37
  %39 = call token @llvm.coro.save(ptr null)
  invoke void @llvm.coro.await.suspend.handle(ptr %12, ptr %22, ptr @_Z3runI17resume_now_handleE4taskPKc.__await_suspend_wrapper__await)
          to label %40 unwind label %47

40:                                               ; preds = %38
  %41 = call i8 @llvm.coro.suspend(token %39, i1 false)
  switch i8 %41, label %93 [
    i8 0, label %51
    i8 1, label %42
  ]

42:                                               ; preds = %40
  br label %53

43:                                               ; preds = %55, %32
  %44 = landingpad { ptr, i32 }
          catch ptr null
  %45 = extractvalue { ptr, i32 } %44, 0
  store ptr %45, ptr %10, align 8
  %46 = extractvalue { ptr, i32 } %44, 1
  store i32 %46, ptr %11, align 4
  br label %64

47:                                               ; preds = %51, %38, %35
  %48 = landingpad { ptr, i32 }
          catch ptr null
  %49 = extractvalue { ptr, i32 } %48, 0
  store ptr %49, ptr %10, align 8
  %50 = extractvalue { ptr, i32 } %48, 1
  store i32 %50, ptr %11, align 4
  call void @llvm.lifetime.end.p0(ptr %12) #4
  br label %64

51:                                               ; preds = %40, %37
  invoke void @_ZN17resume_now_handle12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %12)
          to label %52 unwind label %47

52:                                               ; preds = %51
  br label %53

53:                                               ; preds = %52, %42
  %54 = phi i32 [ 0, %52 ], [ 2, %42 ]
  call void @llvm.lifetime.end.p0(ptr %12) #4
  switch i32 %54, label %61 [
    i32 0, label %55
  ]

55:                                               ; preds = %53
  %56 = load i32, ptr %8, align 4
  %57 = load i32, ptr %9, align 4
  %58 = add nsw i32 %56, %57
  %59 = invoke i32 (ptr, ...) @printf(ptr noundef @.str.6, i32 noundef %58)
          to label %60 unwind label %43

60:                                               ; preds = %55
  br label %61

61:                                               ; preds = %60, %53
  %62 = phi i32 [ %54, %53 ], [ 0, %60 ]
  call void @llvm.lifetime.end.p0(ptr %9) #4
  call void @llvm.lifetime.end.p0(ptr %8) #4
  switch i32 %62, label %85 [
    i32 0, label %63
  ]

63:                                               ; preds = %61
  call void @_ZN4task12promise_type11return_voidEv(ptr noundef nonnull align 1 dereferenceable(1) %4)
  br label %70

64:                                               ; preds = %47, %43
  call void @llvm.lifetime.end.p0(ptr %9) #4
  call void @llvm.lifetime.end.p0(ptr %8) #4
  br label %65

65:                                               ; preds = %64
  %66 = load ptr, ptr %10, align 8
  %67 = call ptr @__cxa_begin_catch(ptr %66) #4
  call void @_ZN4task12promise_type19unhandled_exceptionEv(ptr noundef nonnull align 1 dereferenceable(1) %4)
  invoke void @__cxa_end_catch()
          to label %68 unwind label %76

68:                                               ; preds = %65
  br label %69

69:                                               ; preds = %68
  br label %70

70:                                               ; preds = %69, %63
  call void @llvm.lifetime.start.p0(ptr %13) #4
  call void @_ZN4task12promise_type13final_suspendEv(ptr noundef nonnull align 1 dereferenceable(1) %4) #4
  %71 = call noundef i1 @_ZNKSt7__n486113suspend_never11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %13) #4
  br i1 %71, label %81, label %72

72:                                               ; preds = %70
  %73 = call token @llvm.coro.save(ptr null)
  call void @llvm.coro.await.suspend.void(ptr %13, ptr %22, ptr @_Z3runI17resume_now_handleE4taskPKc.__await_suspend_wrapper__final) #4
  %74 = call i8 @llvm.coro.suspend(token %73, i1 true)
  switch i8 %74, label %93 [
    i8 0, label %81
    i8 1, label %75
  ]

75:                                               ; preds = %72
  br label %82

76:                                               ; preds = %65
  %77 = landingpad { ptr, i32 }
          cleanup
  %78 = extractvalue { ptr, i32 } %77, 0
  store ptr %78, ptr %10, align 8
  %79 = extractvalue { ptr, i32 } %77, 1
  store i32 %79, ptr %11, align 4
  call void @llvm.coro.end(ptr null, i1 true, token none)
  %80 = call i1 @llvm.coro.is_in_ramp()
  br i1 %80, label %94, label %100

81:                                               ; preds = %72, %70
  call void @_ZNKSt7__n486113suspend_never12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %13) #4
  br label %82

82:                                               ; preds = %81, %75
  %83 = phi i32 [ 0, %81 ], [ 2, %75 ]
  call void @llvm.lifetime.end.p0(ptr %13) #4
  switch i32 %83, label %85 [
    i32 0, label %84
  ]

84:                                               ; preds = %82
  br label %85

85:                                               ; preds = %84, %82, %61, %30
  %86 = phi i32 [ %31, %30 ], [ %62, %61 ], [ %83, %82 ], [ 0, %84 ]
  call void @llvm.lifetime.end.p0(ptr %4) #4
  call void @llvm.lifetime.end.p0(ptr %3) #4
  %87 = call ptr @llvm.coro.free(token %15, ptr %22)
  %88 = icmp ne ptr %87, null
  br i1 %88, label %89, label %91

89:                                               ; preds = %85
  %90 = call i64 @llvm.coro.size.i64()
  call void @_ZdlPvm(ptr noundef %87, i64 noundef %90) #4
  br label %91

91:                                               ; preds = %89, %85
  switch i32 %86, label %105 [
    i32 0, label %92
    i32 2, label %93
  ]

92:                                               ; preds = %91
  br label %93

93:                                               ; preds = %92, %91, %72, %40, %25
  call void @llvm.coro.end(ptr null, i1 false, token none)
  ret void

94:                                               ; preds = %76
  call void @llvm.lifetime.end.p0(ptr %4) #4
  call void @llvm.lifetime.end.p0(ptr %3) #4
  %95 = call ptr @llvm.coro.free(token %15, ptr %22)
  %96 = icmp ne ptr %95, null
  br i1 %96, label %97, label %99

97:                                               ; preds = %94
  %98 = call i64 @llvm.coro.size.i64()
  call void @_ZdlPvm(ptr noundef %95, i64 noundef %98) #4
  br label %99

99:                                               ; preds = %97, %94
  br label %100

100:                                              ; preds = %99, %76
  %101 = load ptr, ptr %10, align 8
  %102 = load i32, ptr %11, align 4
  %103 = insertvalue { ptr, i32 } poison, ptr %101, 0
  %104 = insertvalue { ptr, i32 } %103, i32 %102, 1
  resume { ptr, i32 } %104

105:                                              ; preds = %91
  unreachable
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
declare dso_local void @_ZN4task12promise_type17get_return_objectEv(ptr noundef nonnull align 1 dereferenceable(1)) #9

; Function Attrs: mustprogress noinline nounwind optnone uwtable
declare dso_local void @_ZN4task12promise_type15initial_suspendEv(ptr noundef nonnull align 1 dereferenceable(1)) #9

; Function Attrs: mustprogress noinline nounwind optnone uwtable
declare dso_local noundef i1 @_ZNKSt7__n486113suspend_never11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1)) #9

; Function Attrs: alwaysinline mustprogress
declare hidden void @_Z3runI15resume_now_voidE4taskPKc.__await_suspend_wrapper__init(ptr noundef nonnull, ptr noundef) #11

; Function Attrs: mustprogress noinline nounwind optnone uwtable
declare dso_local void @_ZNKSt7__n486113suspend_never12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1)) #9

declare i32 @printf(ptr noundef, ...) #14

; Function Attrs: mustprogress noinline nounwind optnone uwtable
declare dso_local noundef i1 @_ZN15resume_now_void11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1)) #9

; Function Attrs: alwaysinline mustprogress
declare hidden void @_Z3runI15resume_now_voidE4taskPKc.__await_suspend_wrapper__await(ptr noundef nonnull, ptr noundef) #11

; Function Attrs: mustprogress noinline nounwind optnone uwtable
declare dso_local void @_ZN15resume_now_void12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1)) #9

; Function Attrs: mustprogress noinline nounwind optnone uwtable
declare dso_local void @_ZN4task12promise_type11return_voidEv(ptr noundef nonnull align 1 dereferenceable(1)) #9

; Function Attrs: mustprogress noinline nounwind optnone uwtable
declare dso_local void @_ZN4task12promise_type19unhandled_exceptionEv(ptr noundef nonnull align 1 dereferenceable(1)) #9

; Function Attrs: mustprogress noinline nounwind optnone uwtable
declare dso_local void @_ZN4task12promise_type13final_suspendEv(ptr noundef nonnull align 1 dereferenceable(1)) #9

; Function Attrs: alwaysinline mustprogress
declare hidden void @_Z3runI15resume_now_voidE4taskPKc.__await_suspend_wrapper__final(ptr noundef nonnull, ptr noundef) #11

; Function Attrs: alwaysinline mustprogress
declare hidden void @_Z3runI15resume_now_boolE4taskPKc.__await_suspend_wrapper__init(ptr noundef nonnull, ptr noundef) #11

; Function Attrs: mustprogress noinline nounwind optnone uwtable
declare dso_local noundef i1 @_ZN15resume_now_bool11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1)) #9

; Function Attrs: alwaysinline mustprogress
declare hidden noundef i1 @_Z3runI15resume_now_boolE4taskPKc.__await_suspend_wrapper__await(ptr noundef nonnull, ptr noundef) #11

declare i1 @llvm.coro.await.suspend.bool(ptr, ptr, ptr)

; Function Attrs: mustprogress noinline nounwind optnone uwtable
declare dso_local void @_ZN15resume_now_bool12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1)) #9

; Function Attrs: alwaysinline mustprogress
declare hidden void @_Z3runI15resume_now_boolE4taskPKc.__await_suspend_wrapper__final(ptr noundef nonnull, ptr noundef) #11

; Function Attrs: alwaysinline mustprogress
declare hidden void @_Z3runI17resume_now_handleE4taskPKc.__await_suspend_wrapper__init(ptr noundef nonnull, ptr noundef) #11

; Function Attrs: mustprogress noinline nounwind optnone uwtable
declare dso_local noundef i1 @_ZN17resume_now_handle11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1)) #9

; Function Attrs: alwaysinline mustprogress
declare hidden noundef ptr @_Z3runI17resume_now_handleE4taskPKc.__await_suspend_wrapper__await(ptr noundef nonnull, ptr noundef) #11

declare void @llvm.coro.await.suspend.handle(ptr, ptr, ptr)

; Function Attrs: mustprogress noinline nounwind optnone uwtable
declare dso_local void @_ZN17resume_now_handle12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1)) #9

; Function Attrs: alwaysinline mustprogress
declare hidden void @_Z3runI17resume_now_handleE4taskPKc.__await_suspend_wrapper__final(ptr noundef nonnull, ptr noundef) #11

attributes #0 = { mustprogress nobuiltin noinline optnone allocsize(0) uwtable "frame-pointer"="non-leaf-no-reserve" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #1 = { mustprogress nobuiltin noinline nounwind optnone uwtable "frame-pointer"="non-leaf-no-reserve" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #2 = { mustprogress noinline optnone presplitcoroutine uwtable "frame-pointer"="non-leaf-no-reserve" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #3 = { nocallback nofree nosync nounwind willreturn memory(argmem: read) }
attributes #4 = { nounwind }
attributes #5 = { nounwind memory(none) }
attributes #6 = { nocallback nofree nosync nounwind willreturn memory(argmem: readwrite) }
attributes #7 = { nocallback nofree nounwind willreturn memory(argmem: write) }
attributes #8 = { mustprogress noinline optnone uwtable "frame-pointer"="non-leaf-no-reserve" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #9 = { mustprogress noinline nounwind optnone uwtable "frame-pointer"="non-leaf-no-reserve" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #10 = { nomerge nounwind }
attributes #11 = { alwaysinline mustprogress "sample-profile-suffix-elision-policy"="selected" }
attributes #12 = { noinline noreturn nounwind uwtable "frame-pointer"="non-leaf-no-reserve" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #13 = { nounwind memory(argmem: read) }
attributes #14 = { "frame-pointer"="non-leaf-no-reserve" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #15 = { allocsize(0) }
attributes #16 = { noreturn nounwind }

!llvm.module.flags = !{!0, !1, !2, !3, !4}
!llvm.ident = !{!5}

!0 = !{i32 1, !"wchar_size", i32 4}
!1 = !{i32 8, !"PIC Level", i32 2}
!2 = !{i32 7, !"PIE Level", i32 2}
!3 = !{i32 7, !"uwtable", i32 2}
!4 = !{i32 7, !"frame-pointer", i32 4}
!5 = !{!"kiln version 1.0.1"}
!6 = !{!"auto-init"}
