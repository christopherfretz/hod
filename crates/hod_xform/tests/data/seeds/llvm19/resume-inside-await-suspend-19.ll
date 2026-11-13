; ModuleID = 'resume-inside-await-suspend-19.ll'
source_filename = "docs/design/0005-data/resume-inside-await-suspend.cpp"
target datalayout = "e-m:e-i8:8:32-i16:16:32-i64:64-i128:128-n32:64-S128-Fn32"
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
  %9 = bitcast ptr %2 to ptr
  %10 = call token @llvm.coro.id(i32 16, ptr %9, ptr null, ptr null)
  %11 = call i1 @llvm.coro.alloc(token %10)
  br i1 %11, label %12, label %15

12:                                               ; preds = %0
  %13 = call i64 @llvm.coro.size.i64()
  %14 = call noalias noundef nonnull ptr @_Znwm(i64 noundef %13) #15
  br label %15

15:                                               ; preds = %12, %0
  %16 = phi ptr [ null, %0 ], [ %14, %12 ]
  %17 = call ptr @llvm.coro.begin(token %10, ptr %16)
  call void @llvm.lifetime.start.p0(i64 1, ptr %2) #4
  call void @llvm.memset.p0.i64(ptr align 1 %2, i8 0, i64 1, i1 false), !annotation !6
  %18 = invoke i64 @_ZN9noop_task12promise_type17get_return_objectEv(ptr noundef nonnull align 1 dereferenceable(1) %2)
          to label %19 unwind label %28

19:                                               ; preds = %15
  %20 = getelementptr inbounds %struct.noop_task, ptr %1, i32 0, i32 0
  %21 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %20, i32 0, i32 0
  %22 = inttoptr i64 %18 to ptr
  store ptr %22, ptr %21, align 8
  call void @llvm.lifetime.start.p0(i64 1, ptr %5) #4
  call void @_ZN9noop_task12promise_type15initial_suspendEv(ptr noundef nonnull align 1 dereferenceable(1) %2) #4
  %23 = call noundef i1 @_ZNKSt7__n486114suspend_always11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %5) #4
  br i1 %23, label %32, label %24

24:                                               ; preds = %19
  %25 = call token @llvm.coro.save(ptr null)
  call void @llvm.coro.await.suspend.void(ptr %5, ptr %17, ptr @_Z4idlev.__await_suspend_wrapper__init) #4
  %26 = call i8 @llvm.coro.suspend(token %25, i1 false)
  switch i8 %26, label %74 [
    i8 0, label %32
    i8 1, label %27
  ]

27:                                               ; preds = %24
  br label %33

28:                                               ; preds = %15
  %29 = landingpad { ptr, i32 }
          cleanup
  %30 = extractvalue { ptr, i32 } %29, 0
  store ptr %30, ptr %3, align 8
  %31 = extractvalue { ptr, i32 } %29, 1
  store i32 %31, ptr %4, align 4
  br label %83

32:                                               ; preds = %24, %19
  call void @_ZNKSt7__n486114suspend_always12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %5) #4
  br label %33

33:                                               ; preds = %32, %27
  %34 = phi i32 [ 0, %32 ], [ 2, %27 ]
  call void @llvm.lifetime.end.p0(i64 1, ptr %5) #4
  switch i32 %34, label %66 [
    i32 0, label %35
  ]

35:                                               ; preds = %33
  invoke void @_ZN9noop_task12promise_type11return_voidEv(ptr noundef nonnull align 1 dereferenceable(1) %2)
          to label %36 unwind label %37

36:                                               ; preds = %35
  br label %47

37:                                               ; preds = %35
  %38 = landingpad { ptr, i32 }
          catch ptr null
  %39 = extractvalue { ptr, i32 } %38, 0
  store ptr %39, ptr %3, align 8
  %40 = extractvalue { ptr, i32 } %38, 1
  store i32 %40, ptr %4, align 4
  br label %41

41:                                               ; preds = %37
  %42 = load ptr, ptr %3, align 8
  %43 = call ptr @__cxa_begin_catch(ptr %42) #4
  invoke void @_ZN9noop_task12promise_type19unhandled_exceptionEv(ptr noundef nonnull align 1 dereferenceable(1) %2)
          to label %44 unwind label %53

44:                                               ; preds = %41
  invoke void @__cxa_end_catch()
          to label %45 unwind label %57

45:                                               ; preds = %44
  br label %46

46:                                               ; preds = %45
  br label %47

47:                                               ; preds = %46, %36
  call void @llvm.lifetime.start.p0(i64 1, ptr %7) #4
  call void @_ZN9noop_task12promise_type13final_suspendEv(ptr noundef nonnull align 1 dereferenceable(1) %2) #4
  %48 = call noundef i1 @_ZNKSt7__n486114suspend_always11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %7) #4
  br i1 %48, label %62, label %49

49:                                               ; preds = %47
  %50 = call token @llvm.coro.save(ptr null)
  call void @llvm.coro.await.suspend.void(ptr %7, ptr %17, ptr @_Z4idlev.__await_suspend_wrapper__final) #4
  %51 = call i8 @llvm.coro.suspend(token %50, i1 true)
  switch i8 %51, label %74 [
    i8 0, label %62
    i8 1, label %52
  ]

52:                                               ; preds = %49
  br label %63

53:                                               ; preds = %41
  %54 = landingpad { ptr, i32 }
          cleanup
  %55 = extractvalue { ptr, i32 } %54, 0
  store ptr %55, ptr %3, align 8
  %56 = extractvalue { ptr, i32 } %54, 1
  store i32 %56, ptr %4, align 4
  invoke void @__cxa_end_catch()
          to label %61 unwind label %94

57:                                               ; preds = %44
  %58 = landingpad { ptr, i32 }
          cleanup
  %59 = extractvalue { ptr, i32 } %58, 0
  store ptr %59, ptr %3, align 8
  %60 = extractvalue { ptr, i32 } %58, 1
  store i32 %60, ptr %4, align 4
  br label %80

61:                                               ; preds = %53
  br label %80

62:                                               ; preds = %49, %47
  call void @_ZNKSt7__n486114suspend_always12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %7) #4
  br label %63

63:                                               ; preds = %62, %52
  %64 = phi i32 [ 0, %62 ], [ 2, %52 ]
  call void @llvm.lifetime.end.p0(i64 1, ptr %7) #4
  switch i32 %64, label %66 [
    i32 0, label %65
  ]

65:                                               ; preds = %63
  br label %66

66:                                               ; preds = %65, %63, %33
  %67 = phi i32 [ %34, %33 ], [ %64, %63 ], [ 0, %65 ]
  call void @llvm.lifetime.end.p0(i64 1, ptr %2) #4
  %68 = call ptr @llvm.coro.free(token %10, ptr %17)
  %69 = icmp ne ptr %68, null
  br i1 %69, label %70, label %72

70:                                               ; preds = %66
  %71 = call i64 @llvm.coro.size.i64()
  call void @_ZdlPvm(ptr noundef %68, i64 noundef %71) #4
  br label %72

72:                                               ; preds = %70, %66
  switch i32 %67, label %97 [
    i32 0, label %73
    i32 2, label %74
  ]

73:                                               ; preds = %72
  br label %74

74:                                               ; preds = %73, %72, %49, %24
  %75 = call i1 @llvm.coro.end(ptr null, i1 false, token none)
  %76 = getelementptr inbounds %struct.noop_task, ptr %1, i32 0, i32 0
  %77 = getelementptr inbounds %"struct.std::__n4861::coroutine_handle", ptr %76, i32 0, i32 0
  %78 = load ptr, ptr %77, align 8
  %79 = ptrtoint ptr %78 to i64
  ret i64 %79

80:                                               ; preds = %61, %57
  %81 = call i1 @llvm.coro.end(ptr null, i1 true, token none)
  br i1 %81, label %89, label %82

82:                                               ; preds = %80
  br label %83

83:                                               ; preds = %82, %28
  call void @llvm.lifetime.end.p0(i64 1, ptr %2) #4
  %84 = call ptr @llvm.coro.free(token %10, ptr %17)
  %85 = icmp ne ptr %84, null
  br i1 %85, label %86, label %88

86:                                               ; preds = %83
  %87 = call i64 @llvm.coro.size.i64()
  call void @_ZdlPvm(ptr noundef %84, i64 noundef %87) #4
  br label %88

88:                                               ; preds = %86, %83
  br label %89

89:                                               ; preds = %88, %80
  %90 = load ptr, ptr %3, align 8
  %91 = load i32, ptr %4, align 4
  %92 = insertvalue { ptr, i32 } poison, ptr %90, 0
  %93 = insertvalue { ptr, i32 } %92, i32 %91, 1
  resume { ptr, i32 } %93

94:                                               ; preds = %53
  %95 = landingpad { ptr, i32 }
          catch ptr null
  %96 = extractvalue { ptr, i32 } %95, 0
  call void @__clang_call_terminate(ptr %96) #16
  unreachable

97:                                               ; preds = %72
  unreachable
}

; Function Attrs: nocallback nofree nosync nounwind willreturn memory(argmem: read)
declare token @llvm.coro.id(i32, ptr readnone, ptr nocapture readonly, ptr) #3

; Function Attrs: nounwind
declare i1 @llvm.coro.alloc(token) #4

; Function Attrs: nounwind memory(none)
declare i64 @llvm.coro.size.i64() #5

; Function Attrs: nounwind
declare ptr @llvm.coro.begin(token, ptr writeonly) #4

; Function Attrs: nocallback nofree nosync nounwind willreturn memory(argmem: readwrite)
declare void @llvm.lifetime.start.p0(i64 immarg, ptr nocapture) #6

; Function Attrs: nocallback nofree nounwind willreturn memory(argmem: write)
declare void @llvm.memset.p0.i64(ptr nocapture writeonly, i8, i64, i1 immarg) #7

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
declare void @llvm.lifetime.end.p0(i64 immarg, ptr nocapture) #6

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
declare i1 @llvm.coro.end(ptr, i1, token) #4

; Function Attrs: nounwind memory(argmem: read)
declare ptr @llvm.coro.free(token, ptr nocapture readonly) #13

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
  %15 = bitcast ptr %4 to ptr
  %16 = call token @llvm.coro.id(i32 16, ptr %15, ptr null, ptr null)
  %17 = call i1 @llvm.coro.alloc(token %16)
  br i1 %17, label %18, label %21

18:                                               ; preds = %1
  %19 = call i64 @llvm.coro.size.i64()
  %20 = call noalias noundef nonnull ptr @_Znwm(i64 noundef %19) #15
  br label %21

21:                                               ; preds = %18, %1
  %22 = phi ptr [ null, %1 ], [ %20, %18 ]
  %23 = call ptr @llvm.coro.begin(token %16, ptr %22)
  call void @llvm.lifetime.start.p0(i64 8, ptr %3) #4
  %24 = load ptr, ptr %2, align 8
  store ptr %24, ptr %3, align 8
  call void @llvm.lifetime.start.p0(i64 1, ptr %4) #4
  call void @llvm.memset.p0.i64(ptr align 1 %4, i8 0, i64 1, i1 false), !annotation !6
  invoke void @_ZN4task12promise_type17get_return_objectEv(ptr noundef nonnull align 1 dereferenceable(1) %4)
          to label %25 unwind label %31

25:                                               ; preds = %21
  call void @llvm.lifetime.start.p0(i64 1, ptr %8) #4
  call void @_ZN4task12promise_type15initial_suspendEv(ptr noundef nonnull align 1 dereferenceable(1) %4) #4
  %26 = call noundef i1 @_ZNKSt7__n486113suspend_never11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %8) #4
  br i1 %26, label %35, label %27

27:                                               ; preds = %25
  %28 = call token @llvm.coro.save(ptr null)
  call void @llvm.coro.await.suspend.void(ptr %8, ptr %23, ptr @_Z3runI15resume_now_voidE4taskPKc.__await_suspend_wrapper__init) #4
  %29 = call i8 @llvm.coro.suspend(token %28, i1 false)
  switch i8 %29, label %109 [
    i8 0, label %35
    i8 1, label %30
  ]

30:                                               ; preds = %27
  br label %36

31:                                               ; preds = %21
  %32 = landingpad { ptr, i32 }
          cleanup
  %33 = extractvalue { ptr, i32 } %32, 0
  store ptr %33, ptr %5, align 8
  %34 = extractvalue { ptr, i32 } %32, 1
  store i32 %34, ptr %6, align 4
  br label %114

35:                                               ; preds = %27, %25
  call void @_ZNKSt7__n486113suspend_never12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %8) #4
  br label %36

36:                                               ; preds = %35, %30
  %37 = phi i32 [ 0, %35 ], [ 2, %30 ]
  call void @llvm.lifetime.end.p0(i64 1, ptr %8) #4
  switch i32 %37, label %101 [
    i32 0, label %38
  ]

38:                                               ; preds = %36
  call void @llvm.lifetime.start.p0(i64 4, ptr %10) #4
  store i32 1, ptr %10, align 4
  call void @llvm.lifetime.start.p0(i64 4, ptr %11) #4
  store i32 2, ptr %11, align 4
  %39 = load ptr, ptr %3, align 8
  %40 = invoke i32 (ptr, ...) @printf(ptr noundef @.str.5, ptr noundef %39)
          to label %41 unwind label %49

41:                                               ; preds = %38
  call void @llvm.lifetime.start.p0(i64 1, ptr %12) #4
  %42 = invoke noundef i1 @_ZN15resume_now_void11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %12)
          to label %43 unwind label %53

43:                                               ; preds = %41
  br i1 %42, label %57, label %44

44:                                               ; preds = %43
  %45 = call token @llvm.coro.save(ptr null)
  invoke void @llvm.coro.await.suspend.void(ptr %12, ptr %23, ptr @_Z3runI15resume_now_voidE4taskPKc.__await_suspend_wrapper__await)
          to label %46 unwind label %53

46:                                               ; preds = %44
  %47 = call i8 @llvm.coro.suspend(token %45, i1 false)
  switch i8 %47, label %109 [
    i8 0, label %57
    i8 1, label %48
  ]

48:                                               ; preds = %46
  br label %59

49:                                               ; preds = %61, %38
  %50 = landingpad { ptr, i32 }
          catch ptr null
  %51 = extractvalue { ptr, i32 } %50, 0
  store ptr %51, ptr %5, align 8
  %52 = extractvalue { ptr, i32 } %50, 1
  store i32 %52, ptr %6, align 4
  br label %71

53:                                               ; preds = %57, %44, %41
  %54 = landingpad { ptr, i32 }
          catch ptr null
  %55 = extractvalue { ptr, i32 } %54, 0
  store ptr %55, ptr %5, align 8
  %56 = extractvalue { ptr, i32 } %54, 1
  store i32 %56, ptr %6, align 4
  call void @llvm.lifetime.end.p0(i64 1, ptr %12) #4
  br label %71

57:                                               ; preds = %46, %43
  invoke void @_ZN15resume_now_void12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %12)
          to label %58 unwind label %53

58:                                               ; preds = %57
  br label %59

59:                                               ; preds = %58, %48
  %60 = phi i32 [ 0, %58 ], [ 2, %48 ]
  call void @llvm.lifetime.end.p0(i64 1, ptr %12) #4
  switch i32 %60, label %67 [
    i32 0, label %61
  ]

61:                                               ; preds = %59
  %62 = load i32, ptr %10, align 4
  %63 = load i32, ptr %11, align 4
  %64 = add nsw i32 %62, %63
  %65 = invoke i32 (ptr, ...) @printf(ptr noundef @.str.6, i32 noundef %64)
          to label %66 unwind label %49

66:                                               ; preds = %61
  br label %67

67:                                               ; preds = %66, %59
  %68 = phi i32 [ %60, %59 ], [ 0, %66 ]
  call void @llvm.lifetime.end.p0(i64 4, ptr %11) #4
  call void @llvm.lifetime.end.p0(i64 4, ptr %10) #4
  switch i32 %68, label %101 [
    i32 0, label %69
  ]

69:                                               ; preds = %67
  invoke void @_ZN4task12promise_type11return_voidEv(ptr noundef nonnull align 1 dereferenceable(1) %4)
          to label %70 unwind label %72

70:                                               ; preds = %69
  br label %82

71:                                               ; preds = %53, %49
  call void @llvm.lifetime.end.p0(i64 4, ptr %11) #4
  call void @llvm.lifetime.end.p0(i64 4, ptr %10) #4
  br label %76

72:                                               ; preds = %69
  %73 = landingpad { ptr, i32 }
          catch ptr null
  %74 = extractvalue { ptr, i32 } %73, 0
  store ptr %74, ptr %5, align 8
  %75 = extractvalue { ptr, i32 } %73, 1
  store i32 %75, ptr %6, align 4
  br label %76

76:                                               ; preds = %72, %71
  %77 = load ptr, ptr %5, align 8
  %78 = call ptr @__cxa_begin_catch(ptr %77) #4
  invoke void @_ZN4task12promise_type19unhandled_exceptionEv(ptr noundef nonnull align 1 dereferenceable(1) %4)
          to label %79 unwind label %88

79:                                               ; preds = %76
  invoke void @__cxa_end_catch()
          to label %80 unwind label %92

80:                                               ; preds = %79
  br label %81

81:                                               ; preds = %80
  br label %82

82:                                               ; preds = %81, %70
  call void @llvm.lifetime.start.p0(i64 1, ptr %13) #4
  call void @_ZN4task12promise_type13final_suspendEv(ptr noundef nonnull align 1 dereferenceable(1) %4) #4
  %83 = call noundef i1 @_ZNKSt7__n486113suspend_never11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %13) #4
  br i1 %83, label %97, label %84

84:                                               ; preds = %82
  %85 = call token @llvm.coro.save(ptr null)
  call void @llvm.coro.await.suspend.void(ptr %13, ptr %23, ptr @_Z3runI15resume_now_voidE4taskPKc.__await_suspend_wrapper__final) #4
  %86 = call i8 @llvm.coro.suspend(token %85, i1 true)
  switch i8 %86, label %109 [
    i8 0, label %97
    i8 1, label %87
  ]

87:                                               ; preds = %84
  br label %98

88:                                               ; preds = %76
  %89 = landingpad { ptr, i32 }
          cleanup
  %90 = extractvalue { ptr, i32 } %89, 0
  store ptr %90, ptr %5, align 8
  %91 = extractvalue { ptr, i32 } %89, 1
  store i32 %91, ptr %6, align 4
  invoke void @__cxa_end_catch()
          to label %96 unwind label %125

92:                                               ; preds = %79
  %93 = landingpad { ptr, i32 }
          cleanup
  %94 = extractvalue { ptr, i32 } %93, 0
  store ptr %94, ptr %5, align 8
  %95 = extractvalue { ptr, i32 } %93, 1
  store i32 %95, ptr %6, align 4
  br label %111

96:                                               ; preds = %88
  br label %111

97:                                               ; preds = %84, %82
  call void @_ZNKSt7__n486113suspend_never12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %13) #4
  br label %98

98:                                               ; preds = %97, %87
  %99 = phi i32 [ 0, %97 ], [ 2, %87 ]
  call void @llvm.lifetime.end.p0(i64 1, ptr %13) #4
  switch i32 %99, label %101 [
    i32 0, label %100
  ]

100:                                              ; preds = %98
  br label %101

101:                                              ; preds = %100, %98, %67, %36
  %102 = phi i32 [ %37, %36 ], [ %68, %67 ], [ %99, %98 ], [ 0, %100 ]
  call void @llvm.lifetime.end.p0(i64 1, ptr %4) #4
  call void @llvm.lifetime.end.p0(i64 8, ptr %3) #4
  %103 = call ptr @llvm.coro.free(token %16, ptr %23)
  %104 = icmp ne ptr %103, null
  br i1 %104, label %105, label %107

105:                                              ; preds = %101
  %106 = call i64 @llvm.coro.size.i64()
  call void @_ZdlPvm(ptr noundef %103, i64 noundef %106) #4
  br label %107

107:                                              ; preds = %105, %101
  switch i32 %102, label %128 [
    i32 0, label %108
    i32 2, label %109
  ]

108:                                              ; preds = %107
  br label %109

109:                                              ; preds = %108, %107, %84, %46, %27
  %110 = call i1 @llvm.coro.end(ptr null, i1 false, token none)
  ret void

111:                                              ; preds = %96, %92
  %112 = call i1 @llvm.coro.end(ptr null, i1 true, token none)
  br i1 %112, label %120, label %113

113:                                              ; preds = %111
  br label %114

114:                                              ; preds = %113, %31
  call void @llvm.lifetime.end.p0(i64 1, ptr %4) #4
  call void @llvm.lifetime.end.p0(i64 8, ptr %3) #4
  %115 = call ptr @llvm.coro.free(token %16, ptr %23)
  %116 = icmp ne ptr %115, null
  br i1 %116, label %117, label %119

117:                                              ; preds = %114
  %118 = call i64 @llvm.coro.size.i64()
  call void @_ZdlPvm(ptr noundef %115, i64 noundef %118) #4
  br label %119

119:                                              ; preds = %117, %114
  br label %120

120:                                              ; preds = %119, %111
  %121 = load ptr, ptr %5, align 8
  %122 = load i32, ptr %6, align 4
  %123 = insertvalue { ptr, i32 } poison, ptr %121, 0
  %124 = insertvalue { ptr, i32 } %123, i32 %122, 1
  resume { ptr, i32 } %124

125:                                              ; preds = %88
  %126 = landingpad { ptr, i32 }
          catch ptr null
  %127 = extractvalue { ptr, i32 } %126, 0
  call void @__clang_call_terminate(ptr %127) #16
  unreachable

128:                                              ; preds = %107
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
  %15 = bitcast ptr %4 to ptr
  %16 = call token @llvm.coro.id(i32 16, ptr %15, ptr null, ptr null)
  %17 = call i1 @llvm.coro.alloc(token %16)
  br i1 %17, label %18, label %21

18:                                               ; preds = %1
  %19 = call i64 @llvm.coro.size.i64()
  %20 = call noalias noundef nonnull ptr @_Znwm(i64 noundef %19) #15
  br label %21

21:                                               ; preds = %18, %1
  %22 = phi ptr [ null, %1 ], [ %20, %18 ]
  %23 = call ptr @llvm.coro.begin(token %16, ptr %22)
  call void @llvm.lifetime.start.p0(i64 8, ptr %3) #4
  %24 = load ptr, ptr %2, align 8
  store ptr %24, ptr %3, align 8
  call void @llvm.lifetime.start.p0(i64 1, ptr %4) #4
  call void @llvm.memset.p0.i64(ptr align 1 %4, i8 0, i64 1, i1 false), !annotation !6
  call void @_ZN4task12promise_type17get_return_objectEv(ptr noundef nonnull align 1 dereferenceable(1) %4)
  call void @llvm.lifetime.start.p0(i64 1, ptr %6) #4
  call void @_ZN4task12promise_type15initial_suspendEv(ptr noundef nonnull align 1 dereferenceable(1) %4) #4
  %25 = call noundef i1 @_ZNKSt7__n486113suspend_never11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %6) #4
  br i1 %25, label %30, label %26

26:                                               ; preds = %21
  %27 = call token @llvm.coro.save(ptr null)
  call void @llvm.coro.await.suspend.void(ptr %6, ptr %23, ptr @_Z3runI15resume_now_boolE4taskPKc.__await_suspend_wrapper__init) #4
  %28 = call i8 @llvm.coro.suspend(token %27, i1 false)
  switch i8 %28, label %96 [
    i8 0, label %30
    i8 1, label %29
  ]

29:                                               ; preds = %26
  br label %31

30:                                               ; preds = %26, %21
  call void @_ZNKSt7__n486113suspend_never12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %6) #4
  br label %31

31:                                               ; preds = %30, %29
  %32 = phi i32 [ 0, %30 ], [ 2, %29 ]
  call void @llvm.lifetime.end.p0(i64 1, ptr %6) #4
  switch i32 %32, label %88 [
    i32 0, label %33
  ]

33:                                               ; preds = %31
  call void @llvm.lifetime.start.p0(i64 4, ptr %8) #4
  store i32 1, ptr %8, align 4
  call void @llvm.lifetime.start.p0(i64 4, ptr %9) #4
  store i32 2, ptr %9, align 4
  %34 = load ptr, ptr %3, align 8
  %35 = invoke i32 (ptr, ...) @printf(ptr noundef @.str.5, ptr noundef %34)
          to label %36 unwind label %46

36:                                               ; preds = %33
  call void @llvm.lifetime.start.p0(i64 1, ptr %12) #4
  %37 = invoke noundef i1 @_ZN15resume_now_bool11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %12)
          to label %38 unwind label %50

38:                                               ; preds = %36
  br i1 %37, label %54, label %39

39:                                               ; preds = %38
  %40 = call token @llvm.coro.save(ptr null)
  %41 = invoke i1 @llvm.coro.await.suspend.bool(ptr %12, ptr %23, ptr @_Z3runI15resume_now_boolE4taskPKc.__await_suspend_wrapper__await)
          to label %42 unwind label %50

42:                                               ; preds = %39
  br i1 %41, label %43, label %54

43:                                               ; preds = %42
  %44 = call i8 @llvm.coro.suspend(token %40, i1 false)
  switch i8 %44, label %96 [
    i8 0, label %54
    i8 1, label %45
  ]

45:                                               ; preds = %43
  br label %56

46:                                               ; preds = %58, %33
  %47 = landingpad { ptr, i32 }
          catch ptr null
  %48 = extractvalue { ptr, i32 } %47, 0
  store ptr %48, ptr %10, align 8
  %49 = extractvalue { ptr, i32 } %47, 1
  store i32 %49, ptr %11, align 4
  br label %67

50:                                               ; preds = %54, %39, %36
  %51 = landingpad { ptr, i32 }
          catch ptr null
  %52 = extractvalue { ptr, i32 } %51, 0
  store ptr %52, ptr %10, align 8
  %53 = extractvalue { ptr, i32 } %51, 1
  store i32 %53, ptr %11, align 4
  call void @llvm.lifetime.end.p0(i64 1, ptr %12) #4
  br label %67

54:                                               ; preds = %43, %42, %38
  invoke void @_ZN15resume_now_bool12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %12)
          to label %55 unwind label %50

55:                                               ; preds = %54
  br label %56

56:                                               ; preds = %55, %45
  %57 = phi i32 [ 0, %55 ], [ 2, %45 ]
  call void @llvm.lifetime.end.p0(i64 1, ptr %12) #4
  switch i32 %57, label %64 [
    i32 0, label %58
  ]

58:                                               ; preds = %56
  %59 = load i32, ptr %8, align 4
  %60 = load i32, ptr %9, align 4
  %61 = add nsw i32 %59, %60
  %62 = invoke i32 (ptr, ...) @printf(ptr noundef @.str.6, i32 noundef %61)
          to label %63 unwind label %46

63:                                               ; preds = %58
  br label %64

64:                                               ; preds = %63, %56
  %65 = phi i32 [ %57, %56 ], [ 0, %63 ]
  call void @llvm.lifetime.end.p0(i64 4, ptr %9) #4
  call void @llvm.lifetime.end.p0(i64 4, ptr %8) #4
  switch i32 %65, label %88 [
    i32 0, label %66
  ]

66:                                               ; preds = %64
  call void @_ZN4task12promise_type11return_voidEv(ptr noundef nonnull align 1 dereferenceable(1) %4)
  br label %73

67:                                               ; preds = %50, %46
  call void @llvm.lifetime.end.p0(i64 4, ptr %9) #4
  call void @llvm.lifetime.end.p0(i64 4, ptr %8) #4
  br label %68

68:                                               ; preds = %67
  %69 = load ptr, ptr %10, align 8
  %70 = call ptr @__cxa_begin_catch(ptr %69) #4
  call void @_ZN4task12promise_type19unhandled_exceptionEv(ptr noundef nonnull align 1 dereferenceable(1) %4)
  invoke void @__cxa_end_catch()
          to label %71 unwind label %79

71:                                               ; preds = %68
  br label %72

72:                                               ; preds = %71
  br label %73

73:                                               ; preds = %72, %66
  call void @llvm.lifetime.start.p0(i64 1, ptr %13) #4
  call void @_ZN4task12promise_type13final_suspendEv(ptr noundef nonnull align 1 dereferenceable(1) %4) #4
  %74 = call noundef i1 @_ZNKSt7__n486113suspend_never11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %13) #4
  br i1 %74, label %84, label %75

75:                                               ; preds = %73
  %76 = call token @llvm.coro.save(ptr null)
  call void @llvm.coro.await.suspend.void(ptr %13, ptr %23, ptr @_Z3runI15resume_now_boolE4taskPKc.__await_suspend_wrapper__final) #4
  %77 = call i8 @llvm.coro.suspend(token %76, i1 true)
  switch i8 %77, label %96 [
    i8 0, label %84
    i8 1, label %78
  ]

78:                                               ; preds = %75
  br label %85

79:                                               ; preds = %68
  %80 = landingpad { ptr, i32 }
          cleanup
  %81 = extractvalue { ptr, i32 } %80, 0
  store ptr %81, ptr %10, align 8
  %82 = extractvalue { ptr, i32 } %80, 1
  store i32 %82, ptr %11, align 4
  %83 = call i1 @llvm.coro.end(ptr null, i1 true, token none)
  br i1 %83, label %104, label %98

84:                                               ; preds = %75, %73
  call void @_ZNKSt7__n486113suspend_never12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %13) #4
  br label %85

85:                                               ; preds = %84, %78
  %86 = phi i32 [ 0, %84 ], [ 2, %78 ]
  call void @llvm.lifetime.end.p0(i64 1, ptr %13) #4
  switch i32 %86, label %88 [
    i32 0, label %87
  ]

87:                                               ; preds = %85
  br label %88

88:                                               ; preds = %87, %85, %64, %31
  %89 = phi i32 [ %32, %31 ], [ %65, %64 ], [ %86, %85 ], [ 0, %87 ]
  call void @llvm.lifetime.end.p0(i64 1, ptr %4) #4
  call void @llvm.lifetime.end.p0(i64 8, ptr %3) #4
  %90 = call ptr @llvm.coro.free(token %16, ptr %23)
  %91 = icmp ne ptr %90, null
  br i1 %91, label %92, label %94

92:                                               ; preds = %88
  %93 = call i64 @llvm.coro.size.i64()
  call void @_ZdlPvm(ptr noundef %90, i64 noundef %93) #4
  br label %94

94:                                               ; preds = %92, %88
  switch i32 %89, label %109 [
    i32 0, label %95
    i32 2, label %96
  ]

95:                                               ; preds = %94
  br label %96

96:                                               ; preds = %95, %94, %75, %43, %26
  %97 = call i1 @llvm.coro.end(ptr null, i1 false, token none)
  ret void

98:                                               ; preds = %79
  call void @llvm.lifetime.end.p0(i64 1, ptr %4) #4
  call void @llvm.lifetime.end.p0(i64 8, ptr %3) #4
  %99 = call ptr @llvm.coro.free(token %16, ptr %23)
  %100 = icmp ne ptr %99, null
  br i1 %100, label %101, label %103

101:                                              ; preds = %98
  %102 = call i64 @llvm.coro.size.i64()
  call void @_ZdlPvm(ptr noundef %99, i64 noundef %102) #4
  br label %103

103:                                              ; preds = %101, %98
  br label %104

104:                                              ; preds = %103, %79
  %105 = load ptr, ptr %10, align 8
  %106 = load i32, ptr %11, align 4
  %107 = insertvalue { ptr, i32 } poison, ptr %105, 0
  %108 = insertvalue { ptr, i32 } %107, i32 %106, 1
  resume { ptr, i32 } %108

109:                                              ; preds = %94
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
  %15 = bitcast ptr %4 to ptr
  %16 = call token @llvm.coro.id(i32 16, ptr %15, ptr null, ptr null)
  %17 = call i1 @llvm.coro.alloc(token %16)
  br i1 %17, label %18, label %21

18:                                               ; preds = %1
  %19 = call i64 @llvm.coro.size.i64()
  %20 = call noalias noundef nonnull ptr @_Znwm(i64 noundef %19) #15
  br label %21

21:                                               ; preds = %18, %1
  %22 = phi ptr [ null, %1 ], [ %20, %18 ]
  %23 = call ptr @llvm.coro.begin(token %16, ptr %22)
  call void @llvm.lifetime.start.p0(i64 8, ptr %3) #4
  %24 = load ptr, ptr %2, align 8
  store ptr %24, ptr %3, align 8
  call void @llvm.lifetime.start.p0(i64 1, ptr %4) #4
  call void @llvm.memset.p0.i64(ptr align 1 %4, i8 0, i64 1, i1 false), !annotation !6
  call void @_ZN4task12promise_type17get_return_objectEv(ptr noundef nonnull align 1 dereferenceable(1) %4)
  call void @llvm.lifetime.start.p0(i64 1, ptr %6) #4
  call void @_ZN4task12promise_type15initial_suspendEv(ptr noundef nonnull align 1 dereferenceable(1) %4) #4
  %25 = call noundef i1 @_ZNKSt7__n486113suspend_never11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %6) #4
  br i1 %25, label %30, label %26

26:                                               ; preds = %21
  %27 = call token @llvm.coro.save(ptr null)
  call void @llvm.coro.await.suspend.void(ptr %6, ptr %23, ptr @_Z3runI17resume_now_handleE4taskPKc.__await_suspend_wrapper__init) #4
  %28 = call i8 @llvm.coro.suspend(token %27, i1 false)
  switch i8 %28, label %94 [
    i8 0, label %30
    i8 1, label %29
  ]

29:                                               ; preds = %26
  br label %31

30:                                               ; preds = %26, %21
  call void @_ZNKSt7__n486113suspend_never12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %6) #4
  br label %31

31:                                               ; preds = %30, %29
  %32 = phi i32 [ 0, %30 ], [ 2, %29 ]
  call void @llvm.lifetime.end.p0(i64 1, ptr %6) #4
  switch i32 %32, label %86 [
    i32 0, label %33
  ]

33:                                               ; preds = %31
  call void @llvm.lifetime.start.p0(i64 4, ptr %8) #4
  store i32 1, ptr %8, align 4
  call void @llvm.lifetime.start.p0(i64 4, ptr %9) #4
  store i32 2, ptr %9, align 4
  %34 = load ptr, ptr %3, align 8
  %35 = invoke i32 (ptr, ...) @printf(ptr noundef @.str.5, ptr noundef %34)
          to label %36 unwind label %44

36:                                               ; preds = %33
  call void @llvm.lifetime.start.p0(i64 1, ptr %12) #4
  %37 = invoke noundef i1 @_ZN17resume_now_handle11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %12)
          to label %38 unwind label %48

38:                                               ; preds = %36
  br i1 %37, label %52, label %39

39:                                               ; preds = %38
  %40 = call token @llvm.coro.save(ptr null)
  invoke void @llvm.coro.await.suspend.handle(ptr %12, ptr %23, ptr @_Z3runI17resume_now_handleE4taskPKc.__await_suspend_wrapper__await)
          to label %41 unwind label %48

41:                                               ; preds = %39
  %42 = call i8 @llvm.coro.suspend(token %40, i1 false)
  switch i8 %42, label %94 [
    i8 0, label %52
    i8 1, label %43
  ]

43:                                               ; preds = %41
  br label %54

44:                                               ; preds = %56, %33
  %45 = landingpad { ptr, i32 }
          catch ptr null
  %46 = extractvalue { ptr, i32 } %45, 0
  store ptr %46, ptr %10, align 8
  %47 = extractvalue { ptr, i32 } %45, 1
  store i32 %47, ptr %11, align 4
  br label %65

48:                                               ; preds = %52, %39, %36
  %49 = landingpad { ptr, i32 }
          catch ptr null
  %50 = extractvalue { ptr, i32 } %49, 0
  store ptr %50, ptr %10, align 8
  %51 = extractvalue { ptr, i32 } %49, 1
  store i32 %51, ptr %11, align 4
  call void @llvm.lifetime.end.p0(i64 1, ptr %12) #4
  br label %65

52:                                               ; preds = %41, %38
  invoke void @_ZN17resume_now_handle12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %12)
          to label %53 unwind label %48

53:                                               ; preds = %52
  br label %54

54:                                               ; preds = %53, %43
  %55 = phi i32 [ 0, %53 ], [ 2, %43 ]
  call void @llvm.lifetime.end.p0(i64 1, ptr %12) #4
  switch i32 %55, label %62 [
    i32 0, label %56
  ]

56:                                               ; preds = %54
  %57 = load i32, ptr %8, align 4
  %58 = load i32, ptr %9, align 4
  %59 = add nsw i32 %57, %58
  %60 = invoke i32 (ptr, ...) @printf(ptr noundef @.str.6, i32 noundef %59)
          to label %61 unwind label %44

61:                                               ; preds = %56
  br label %62

62:                                               ; preds = %61, %54
  %63 = phi i32 [ %55, %54 ], [ 0, %61 ]
  call void @llvm.lifetime.end.p0(i64 4, ptr %9) #4
  call void @llvm.lifetime.end.p0(i64 4, ptr %8) #4
  switch i32 %63, label %86 [
    i32 0, label %64
  ]

64:                                               ; preds = %62
  call void @_ZN4task12promise_type11return_voidEv(ptr noundef nonnull align 1 dereferenceable(1) %4)
  br label %71

65:                                               ; preds = %48, %44
  call void @llvm.lifetime.end.p0(i64 4, ptr %9) #4
  call void @llvm.lifetime.end.p0(i64 4, ptr %8) #4
  br label %66

66:                                               ; preds = %65
  %67 = load ptr, ptr %10, align 8
  %68 = call ptr @__cxa_begin_catch(ptr %67) #4
  call void @_ZN4task12promise_type19unhandled_exceptionEv(ptr noundef nonnull align 1 dereferenceable(1) %4)
  invoke void @__cxa_end_catch()
          to label %69 unwind label %77

69:                                               ; preds = %66
  br label %70

70:                                               ; preds = %69
  br label %71

71:                                               ; preds = %70, %64
  call void @llvm.lifetime.start.p0(i64 1, ptr %13) #4
  call void @_ZN4task12promise_type13final_suspendEv(ptr noundef nonnull align 1 dereferenceable(1) %4) #4
  %72 = call noundef i1 @_ZNKSt7__n486113suspend_never11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %13) #4
  br i1 %72, label %82, label %73

73:                                               ; preds = %71
  %74 = call token @llvm.coro.save(ptr null)
  call void @llvm.coro.await.suspend.void(ptr %13, ptr %23, ptr @_Z3runI17resume_now_handleE4taskPKc.__await_suspend_wrapper__final) #4
  %75 = call i8 @llvm.coro.suspend(token %74, i1 true)
  switch i8 %75, label %94 [
    i8 0, label %82
    i8 1, label %76
  ]

76:                                               ; preds = %73
  br label %83

77:                                               ; preds = %66
  %78 = landingpad { ptr, i32 }
          cleanup
  %79 = extractvalue { ptr, i32 } %78, 0
  store ptr %79, ptr %10, align 8
  %80 = extractvalue { ptr, i32 } %78, 1
  store i32 %80, ptr %11, align 4
  %81 = call i1 @llvm.coro.end(ptr null, i1 true, token none)
  br i1 %81, label %102, label %96

82:                                               ; preds = %73, %71
  call void @_ZNKSt7__n486113suspend_never12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %13) #4
  br label %83

83:                                               ; preds = %82, %76
  %84 = phi i32 [ 0, %82 ], [ 2, %76 ]
  call void @llvm.lifetime.end.p0(i64 1, ptr %13) #4
  switch i32 %84, label %86 [
    i32 0, label %85
  ]

85:                                               ; preds = %83
  br label %86

86:                                               ; preds = %85, %83, %62, %31
  %87 = phi i32 [ %32, %31 ], [ %63, %62 ], [ %84, %83 ], [ 0, %85 ]
  call void @llvm.lifetime.end.p0(i64 1, ptr %4) #4
  call void @llvm.lifetime.end.p0(i64 8, ptr %3) #4
  %88 = call ptr @llvm.coro.free(token %16, ptr %23)
  %89 = icmp ne ptr %88, null
  br i1 %89, label %90, label %92

90:                                               ; preds = %86
  %91 = call i64 @llvm.coro.size.i64()
  call void @_ZdlPvm(ptr noundef %88, i64 noundef %91) #4
  br label %92

92:                                               ; preds = %90, %86
  switch i32 %87, label %107 [
    i32 0, label %93
    i32 2, label %94
  ]

93:                                               ; preds = %92
  br label %94

94:                                               ; preds = %93, %92, %73, %41, %26
  %95 = call i1 @llvm.coro.end(ptr null, i1 false, token none)
  ret void

96:                                               ; preds = %77
  call void @llvm.lifetime.end.p0(i64 1, ptr %4) #4
  call void @llvm.lifetime.end.p0(i64 8, ptr %3) #4
  %97 = call ptr @llvm.coro.free(token %16, ptr %23)
  %98 = icmp ne ptr %97, null
  br i1 %98, label %99, label %101

99:                                               ; preds = %96
  %100 = call i64 @llvm.coro.size.i64()
  call void @_ZdlPvm(ptr noundef %97, i64 noundef %100) #4
  br label %101

101:                                              ; preds = %99, %96
  br label %102

102:                                              ; preds = %101, %77
  %103 = load ptr, ptr %10, align 8
  %104 = load i32, ptr %11, align 4
  %105 = insertvalue { ptr, i32 } poison, ptr %103, 0
  %106 = insertvalue { ptr, i32 } %105, i32 %104, 1
  resume { ptr, i32 } %106

107:                                              ; preds = %92
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

attributes #0 = { mustprogress nobuiltin noinline optnone allocsize(0) uwtable "frame-pointer"="non-leaf" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #1 = { mustprogress nobuiltin noinline nounwind optnone uwtable "frame-pointer"="non-leaf" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #2 = { mustprogress noinline optnone presplitcoroutine uwtable "frame-pointer"="non-leaf" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #3 = { nocallback nofree nosync nounwind willreturn memory(argmem: read) }
attributes #4 = { nounwind }
attributes #5 = { nounwind memory(none) }
attributes #6 = { nocallback nofree nosync nounwind willreturn memory(argmem: readwrite) }
attributes #7 = { nocallback nofree nounwind willreturn memory(argmem: write) }
attributes #8 = { mustprogress noinline optnone uwtable "frame-pointer"="non-leaf" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #9 = { mustprogress noinline nounwind optnone uwtable "frame-pointer"="non-leaf" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #10 = { nomerge nounwind }
attributes #11 = { alwaysinline mustprogress }
attributes #12 = { noinline noreturn nounwind uwtable "frame-pointer"="non-leaf" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #13 = { nounwind memory(argmem: read) }
attributes #14 = { "frame-pointer"="non-leaf" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #15 = { allocsize(0) }
attributes #16 = { noreturn nounwind }

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
