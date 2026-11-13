; ModuleID = 'post-sync-22.ll'
source_filename = "docs/design/0005-data/post-sync.cpp"
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i8:8:32-i16:16:32-i64:64-i128:128-n32:64-S128-Fn32"
target triple = "aarch64-unknown-linux-gnu"

%struct.sync = type { i64 }
%"struct.sync::promise_type" = type { i8 }
%"struct.std::__n4861::suspend_never" = type { i8 }

@.contract = external hidden unnamed_addr constant { i16, i16, i16, i16, ptr, ptr, ptr }, align 8
@.contract.5 = external hidden unnamed_addr constant { i16, i16, i16, i16, ptr, ptr, ptr }, align 8

; Function Attrs: mustprogress noinline optnone uwtable
declare dso_local void @_Z25handle_contract_violationRKNSt9contracts18contract_violationE(ptr noundef nonnull align 8 dereferenceable(32)) #0

; Function Attrs: mustprogress noinline optnone presplitcoroutine uwtable
define dso_local i64 @_Z1fv() #1 personality ptr @__gxx_personality_v0 {
  %1 = alloca %struct.sync, align 8
  %2 = alloca %"struct.sync::promise_type", align 1
  %3 = alloca ptr, align 8
  %4 = alloca i32, align 4
  %5 = alloca %"struct.std::__n4861::suspend_never", align 1
  %6 = alloca %"struct.std::__n4861::suspend_never", align 1
  %7 = alloca %"struct.std::__n4861::suspend_never", align 1
  %8 = alloca %"struct.std::__n4861::suspend_never", align 1
  %9 = call token @llvm.coro.id(i32 16, ptr %2, ptr null, ptr null)
  %10 = call i1 @llvm.coro.alloc(token %9)
  br i1 %10, label %11, label %14

11:                                               ; preds = %0
  %12 = call i64 @llvm.coro.size.i64()
  %13 = call noalias noundef nonnull ptr @_Znwm(i64 noundef %12) #14
  br label %14

14:                                               ; preds = %11, %0
  %15 = phi ptr [ null, %0 ], [ %13, %11 ]
  %16 = call ptr @llvm.coro.begin(token %9, ptr %15)
  call void @llvm.lifetime.start.p0(ptr %2) #3
  call void @llvm.memset.p0.i64(ptr align 1 %2, i8 0, i64 1, i1 false), !annotation !6
  %17 = invoke i64 @_ZN4sync12promise_type17get_return_objectEv(ptr noundef nonnull align 1 dereferenceable(1) %2)
          to label %18 unwind label %25

18:                                               ; preds = %14
  %19 = getelementptr inbounds nuw %struct.sync, ptr %1, i32 0, i32 0
  store i64 %17, ptr %19, align 8
  call void @llvm.lifetime.start.p0(ptr %5) #3
  call void @_ZN4sync12promise_type15initial_suspendEv(ptr noundef nonnull align 1 dereferenceable(1) %2) #3
  %20 = call noundef i1 @_ZNKSt7__n486113suspend_never11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %5) #3
  br i1 %20, label %29, label %21

21:                                               ; preds = %18
  %22 = call token @llvm.coro.save(ptr null)
  call void @llvm.coro.await.suspend.void(ptr %5, ptr %16, ptr @_Z1fv.__await_suspend_wrapper__init) #3
  %23 = call i8 @llvm.coro.suspend(token %22, i1 false)
  switch i8 %23, label %71 [
    i8 0, label %29
    i8 1, label %24
  ]

24:                                               ; preds = %21
  br label %30

25:                                               ; preds = %14
  %26 = landingpad { ptr, i32 }
          cleanup
  %27 = extractvalue { ptr, i32 } %26, 0
  store ptr %27, ptr %3, align 8
  %28 = extractvalue { ptr, i32 } %26, 1
  store i32 %28, ptr %4, align 4
  br label %77

29:                                               ; preds = %21, %18
  call void @_ZNKSt7__n486113suspend_never12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %5) #3
  br label %30

30:                                               ; preds = %29, %24
  %31 = phi i32 [ 0, %29 ], [ 2, %24 ]
  call void @llvm.lifetime.end.p0(ptr %5) #3
  switch i32 %31, label %63 [
    i32 0, label %32
  ]

32:                                               ; preds = %30
  invoke void @_ZN4sync12promise_type11return_voidEv(ptr noundef nonnull align 1 dereferenceable(1) %2)
          to label %33 unwind label %34

33:                                               ; preds = %32
  br label %44

34:                                               ; preds = %32
  %35 = landingpad { ptr, i32 }
          catch ptr null
  %36 = extractvalue { ptr, i32 } %35, 0
  store ptr %36, ptr %3, align 8
  %37 = extractvalue { ptr, i32 } %35, 1
  store i32 %37, ptr %4, align 4
  br label %38

38:                                               ; preds = %34
  %39 = load ptr, ptr %3, align 8
  %40 = call ptr @__cxa_begin_catch(ptr %39) #3
  invoke void @_ZN4sync12promise_type19unhandled_exceptionEv(ptr noundef nonnull align 1 dereferenceable(1) %2)
          to label %41 unwind label %50

41:                                               ; preds = %38
  invoke void @__cxa_end_catch()
          to label %42 unwind label %54

42:                                               ; preds = %41
  br label %43

43:                                               ; preds = %42
  br label %44

44:                                               ; preds = %43, %33
  call void @llvm.lifetime.start.p0(ptr %7) #3
  call void @_ZN4sync12promise_type13final_suspendEv(ptr noundef nonnull align 1 dereferenceable(1) %2) #3
  %45 = call noundef i1 @_ZNKSt7__n486113suspend_never11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %7) #3
  br i1 %45, label %59, label %46

46:                                               ; preds = %44
  %47 = call token @llvm.coro.save(ptr null)
  call void @llvm.coro.await.suspend.void(ptr %7, ptr %16, ptr @_Z1fv.__await_suspend_wrapper__final) #3
  %48 = call i8 @llvm.coro.suspend(token %47, i1 true)
  switch i8 %48, label %71 [
    i8 0, label %59
    i8 1, label %49
  ]

49:                                               ; preds = %46
  br label %60

50:                                               ; preds = %38
  %51 = landingpad { ptr, i32 }
          cleanup
  %52 = extractvalue { ptr, i32 } %51, 0
  store ptr %52, ptr %3, align 8
  %53 = extractvalue { ptr, i32 } %51, 1
  store i32 %53, ptr %4, align 4
  invoke void @__cxa_end_catch()
          to label %58 unwind label %105

54:                                               ; preds = %41
  %55 = landingpad { ptr, i32 }
          cleanup
  %56 = extractvalue { ptr, i32 } %55, 0
  store ptr %56, ptr %3, align 8
  %57 = extractvalue { ptr, i32 } %55, 1
  store i32 %57, ptr %4, align 4
  br label %74

58:                                               ; preds = %50
  br label %74

59:                                               ; preds = %46, %44
  call void @_ZNKSt7__n486113suspend_never12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %7) #3
  br label %60

60:                                               ; preds = %59, %49
  %61 = phi i32 [ 0, %59 ], [ 2, %49 ]
  call void @llvm.lifetime.end.p0(ptr %7) #3
  switch i32 %61, label %63 [
    i32 0, label %62
  ]

62:                                               ; preds = %60
  br label %63

63:                                               ; preds = %62, %60, %30
  %64 = phi i32 [ %31, %30 ], [ %61, %60 ], [ 0, %62 ]
  call void @llvm.lifetime.end.p0(ptr %2) #3
  %65 = call ptr @llvm.coro.free(token %9, ptr %16)
  %66 = icmp ne ptr %65, null
  br i1 %66, label %67, label %69

67:                                               ; preds = %63
  %68 = call i64 @llvm.coro.size.i64()
  call void @_ZdlPvm(ptr noundef %65, i64 noundef %68) #3
  br label %69

69:                                               ; preds = %67, %63
  switch i32 %64, label %108 [
    i32 0, label %70
    i32 2, label %71
  ]

70:                                               ; preds = %69
  br label %71

71:                                               ; preds = %70, %69, %46, %21
  call void @llvm.coro.end(ptr null, i1 false, token none)
  %72 = invoke noundef i1 @_ZNK4sync2okEv(ptr noundef nonnull align 8 dereferenceable(8) %1)
          to label %73 unwind label %83

73:                                               ; preds = %71
  br i1 %72, label %97, label %96

74:                                               ; preds = %58, %54
  call void @llvm.coro.end(ptr null, i1 true, token none)
  %75 = call i1 @llvm.coro.is_in_ramp()
  br i1 %75, label %76, label %100

76:                                               ; preds = %74
  br label %77

77:                                               ; preds = %76, %25
  call void @llvm.lifetime.end.p0(ptr %2) #3
  %78 = call ptr @llvm.coro.free(token %9, ptr %16)
  %79 = icmp ne ptr %78, null
  br i1 %79, label %80, label %82

80:                                               ; preds = %77
  %81 = call i64 @llvm.coro.size.i64()
  call void @_ZdlPvm(ptr noundef %78, i64 noundef %81) #3
  br label %82

82:                                               ; preds = %80, %77
  br label %100

83:                                               ; preds = %71
  %84 = landingpad { ptr, i32 }
          catch ptr null
  %85 = extractvalue { ptr, i32 } %84, 0
  store ptr %85, ptr %3, align 8
  %86 = extractvalue { ptr, i32 } %84, 1
  store i32 %86, ptr %4, align 4
  br label %87

87:                                               ; preds = %83
  %88 = load ptr, ptr %3, align 8
  %89 = call ptr @__cxa_begin_catch(ptr %88) #3
  invoke void @_Z25handle_contract_violationRKNSt9contracts18contract_violationE(ptr @.contract)
          to label %90 unwind label %91

90:                                               ; preds = %87
  call void @_ZSt9terminatev() #3
  unreachable

91:                                               ; preds = %87
  %92 = landingpad { ptr, i32 }
          cleanup
  %93 = extractvalue { ptr, i32 } %92, 0
  store ptr %93, ptr %3, align 8
  %94 = extractvalue { ptr, i32 } %92, 1
  store i32 %94, ptr %4, align 4
  invoke void @__cxa_end_catch()
          to label %95 unwind label %105

95:                                               ; preds = %91
  br label %100

96:                                               ; preds = %73
  call void @_Z25handle_contract_violationRKNSt9contracts18contract_violationE(ptr @.contract.5)
  call void @_ZSt9terminatev() #3
  unreachable

97:                                               ; preds = %73
  %98 = getelementptr inbounds nuw %struct.sync, ptr %1, i32 0, i32 0
  %99 = load i64, ptr %98, align 8
  ret i64 %99

100:                                              ; preds = %95, %82, %74
  %101 = load ptr, ptr %3, align 8
  %102 = load i32, ptr %4, align 4
  %103 = insertvalue { ptr, i32 } poison, ptr %101, 0
  %104 = insertvalue { ptr, i32 } %103, i32 %102, 1
  resume { ptr, i32 } %104

105:                                              ; preds = %91, %50
  %106 = landingpad { ptr, i32 }
          catch ptr null
  %107 = extractvalue { ptr, i32 } %106, 0
  call void @__clang_call_terminate(ptr %107) #15
  unreachable

108:                                              ; preds = %69
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

; Function Attrs: mustprogress noinline nounwind optnone uwtable
declare dso_local i64 @_ZN4sync12promise_type17get_return_objectEv(ptr noundef nonnull align 1 dereferenceable(1)) #8

declare i32 @__gxx_personality_v0(...)

; Function Attrs: mustprogress noinline nounwind optnone uwtable
declare dso_local void @_ZN4sync12promise_type15initial_suspendEv(ptr noundef nonnull align 1 dereferenceable(1)) #8

; Function Attrs: mustprogress noinline nounwind optnone uwtable
declare dso_local noundef i1 @_ZNKSt7__n486113suspend_never11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1)) #8

; Function Attrs: nomerge nounwind
declare token @llvm.coro.save(ptr) #9

; Function Attrs: alwaysinline mustprogress
declare hidden void @_Z1fv.__await_suspend_wrapper__init(ptr noundef nonnull, ptr noundef) #10

declare void @llvm.coro.await.suspend.void(ptr, ptr, ptr)

; Function Attrs: nounwind
declare i8 @llvm.coro.suspend(token, i1) #3

; Function Attrs: mustprogress noinline nounwind optnone uwtable
declare dso_local void @_ZNKSt7__n486113suspend_never12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1)) #8

; Function Attrs: nocallback nofree nosync nounwind willreturn memory(argmem: readwrite)
declare void @llvm.lifetime.end.p0(ptr captures(none)) #6

; Function Attrs: mustprogress noinline nounwind optnone uwtable
declare dso_local void @_ZN4sync12promise_type11return_voidEv(ptr noundef nonnull align 1 dereferenceable(1)) #8

declare ptr @__cxa_begin_catch(ptr)

; Function Attrs: mustprogress noinline nounwind optnone uwtable
declare dso_local void @_ZN4sync12promise_type19unhandled_exceptionEv(ptr noundef nonnull align 1 dereferenceable(1)) #8

declare void @__cxa_end_catch()

; Function Attrs: noinline noreturn nounwind uwtable
declare hidden void @__clang_call_terminate(ptr noundef) #11

declare void @_ZSt9terminatev()

; Function Attrs: mustprogress noinline nounwind optnone uwtable
declare dso_local void @_ZN4sync12promise_type13final_suspendEv(ptr noundef nonnull align 1 dereferenceable(1)) #8

; Function Attrs: alwaysinline mustprogress
declare hidden void @_Z1fv.__await_suspend_wrapper__final(ptr noundef nonnull, ptr noundef) #10

; Function Attrs: nounwind
declare void @llvm.coro.end(ptr, i1, token) #3

; Function Attrs: nounwind memory(none)
declare i1 @llvm.coro.is_in_ramp() #5

; Function Attrs: nobuiltin nounwind
declare void @_ZdlPvm(ptr noundef, i64 noundef) #12

; Function Attrs: nounwind memory(argmem: read)
declare ptr @llvm.coro.free(token, ptr readonly captures(none)) #13

; Function Attrs: mustprogress noinline nounwind optnone uwtable
declare dso_local noundef i1 @_ZNK4sync2okEv(ptr noundef nonnull align 8 dereferenceable(8)) #8

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
