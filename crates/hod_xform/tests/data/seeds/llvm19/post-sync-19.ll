; ModuleID = 'post-sync-19.ll'
source_filename = "docs/design/0005-data/post-sync.cpp"
target datalayout = "e-m:e-i8:8:32-i16:16:32-i64:64-i128:128-n32:64-S128-Fn32"
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
  %9 = bitcast ptr %2 to ptr
  %10 = call token @llvm.coro.id(i32 16, ptr %9, ptr null, ptr null)
  %11 = call i1 @llvm.coro.alloc(token %10)
  br i1 %11, label %12, label %15

12:                                               ; preds = %0
  %13 = call i64 @llvm.coro.size.i64()
  %14 = call noalias noundef nonnull ptr @_Znwm(i64 noundef %13) #14
  br label %15

15:                                               ; preds = %12, %0
  %16 = phi ptr [ null, %0 ], [ %14, %12 ]
  %17 = call ptr @llvm.coro.begin(token %10, ptr %16)
  call void @llvm.lifetime.start.p0(i64 1, ptr %2) #3
  call void @llvm.memset.p0.i64(ptr align 1 %2, i8 0, i64 1, i1 false), !annotation !6
  %18 = invoke i64 @_ZN4sync12promise_type17get_return_objectEv(ptr noundef nonnull align 1 dereferenceable(1) %2)
          to label %19 unwind label %26

19:                                               ; preds = %15
  %20 = getelementptr inbounds %struct.sync, ptr %1, i32 0, i32 0
  store i64 %18, ptr %20, align 8
  call void @llvm.lifetime.start.p0(i64 1, ptr %5) #3
  call void @_ZN4sync12promise_type15initial_suspendEv(ptr noundef nonnull align 1 dereferenceable(1) %2) #3
  %21 = call noundef i1 @_ZNKSt7__n486113suspend_never11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %5) #3
  br i1 %21, label %30, label %22

22:                                               ; preds = %19
  %23 = call token @llvm.coro.save(ptr null)
  call void @llvm.coro.await.suspend.void(ptr %5, ptr %17, ptr @_Z1fv.__await_suspend_wrapper__init) #3
  %24 = call i8 @llvm.coro.suspend(token %23, i1 false)
  switch i8 %24, label %72 [
    i8 0, label %30
    i8 1, label %25
  ]

25:                                               ; preds = %22
  br label %31

26:                                               ; preds = %15
  %27 = landingpad { ptr, i32 }
          cleanup
  %28 = extractvalue { ptr, i32 } %27, 0
  store ptr %28, ptr %3, align 8
  %29 = extractvalue { ptr, i32 } %27, 1
  store i32 %29, ptr %4, align 4
  br label %79

30:                                               ; preds = %22, %19
  call void @_ZNKSt7__n486113suspend_never12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %5) #3
  br label %31

31:                                               ; preds = %30, %25
  %32 = phi i32 [ 0, %30 ], [ 2, %25 ]
  call void @llvm.lifetime.end.p0(i64 1, ptr %5) #3
  switch i32 %32, label %64 [
    i32 0, label %33
  ]

33:                                               ; preds = %31
  invoke void @_ZN4sync12promise_type11return_voidEv(ptr noundef nonnull align 1 dereferenceable(1) %2)
          to label %34 unwind label %35

34:                                               ; preds = %33
  br label %45

35:                                               ; preds = %33
  %36 = landingpad { ptr, i32 }
          catch ptr null
  %37 = extractvalue { ptr, i32 } %36, 0
  store ptr %37, ptr %3, align 8
  %38 = extractvalue { ptr, i32 } %36, 1
  store i32 %38, ptr %4, align 4
  br label %39

39:                                               ; preds = %35
  %40 = load ptr, ptr %3, align 8
  %41 = call ptr @__cxa_begin_catch(ptr %40) #3
  invoke void @_ZN4sync12promise_type19unhandled_exceptionEv(ptr noundef nonnull align 1 dereferenceable(1) %2)
          to label %42 unwind label %51

42:                                               ; preds = %39
  invoke void @__cxa_end_catch()
          to label %43 unwind label %55

43:                                               ; preds = %42
  br label %44

44:                                               ; preds = %43
  br label %45

45:                                               ; preds = %44, %34
  call void @llvm.lifetime.start.p0(i64 1, ptr %7) #3
  call void @_ZN4sync12promise_type13final_suspendEv(ptr noundef nonnull align 1 dereferenceable(1) %2) #3
  %46 = call noundef i1 @_ZNKSt7__n486113suspend_never11await_readyEv(ptr noundef nonnull align 1 dereferenceable(1) %7) #3
  br i1 %46, label %60, label %47

47:                                               ; preds = %45
  %48 = call token @llvm.coro.save(ptr null)
  call void @llvm.coro.await.suspend.void(ptr %7, ptr %17, ptr @_Z1fv.__await_suspend_wrapper__final) #3
  %49 = call i8 @llvm.coro.suspend(token %48, i1 true)
  switch i8 %49, label %72 [
    i8 0, label %60
    i8 1, label %50
  ]

50:                                               ; preds = %47
  br label %61

51:                                               ; preds = %39
  %52 = landingpad { ptr, i32 }
          cleanup
  %53 = extractvalue { ptr, i32 } %52, 0
  store ptr %53, ptr %3, align 8
  %54 = extractvalue { ptr, i32 } %52, 1
  store i32 %54, ptr %4, align 4
  invoke void @__cxa_end_catch()
          to label %59 unwind label %107

55:                                               ; preds = %42
  %56 = landingpad { ptr, i32 }
          cleanup
  %57 = extractvalue { ptr, i32 } %56, 0
  store ptr %57, ptr %3, align 8
  %58 = extractvalue { ptr, i32 } %56, 1
  store i32 %58, ptr %4, align 4
  br label %76

59:                                               ; preds = %51
  br label %76

60:                                               ; preds = %47, %45
  call void @_ZNKSt7__n486113suspend_never12await_resumeEv(ptr noundef nonnull align 1 dereferenceable(1) %7) #3
  br label %61

61:                                               ; preds = %60, %50
  %62 = phi i32 [ 0, %60 ], [ 2, %50 ]
  call void @llvm.lifetime.end.p0(i64 1, ptr %7) #3
  switch i32 %62, label %64 [
    i32 0, label %63
  ]

63:                                               ; preds = %61
  br label %64

64:                                               ; preds = %63, %61, %31
  %65 = phi i32 [ %32, %31 ], [ %62, %61 ], [ 0, %63 ]
  call void @llvm.lifetime.end.p0(i64 1, ptr %2) #3
  %66 = call ptr @llvm.coro.free(token %10, ptr %17)
  %67 = icmp ne ptr %66, null
  br i1 %67, label %68, label %70

68:                                               ; preds = %64
  %69 = call i64 @llvm.coro.size.i64()
  call void @_ZdlPvm(ptr noundef %66, i64 noundef %69) #3
  br label %70

70:                                               ; preds = %68, %64
  switch i32 %65, label %110 [
    i32 0, label %71
    i32 2, label %72
  ]

71:                                               ; preds = %70
  br label %72

72:                                               ; preds = %71, %70, %47, %22
  %73 = call i1 @llvm.coro.end(ptr null, i1 false, token none)
  %74 = invoke noundef i1 @_ZNK4sync2okEv(ptr noundef nonnull align 8 dereferenceable(8) %1)
          to label %75 unwind label %85

75:                                               ; preds = %72
  br i1 %74, label %99, label %98

76:                                               ; preds = %59, %55
  %77 = call i1 @llvm.coro.end(ptr null, i1 true, token none)
  br i1 %77, label %102, label %78

78:                                               ; preds = %76
  br label %79

79:                                               ; preds = %78, %26
  call void @llvm.lifetime.end.p0(i64 1, ptr %2) #3
  %80 = call ptr @llvm.coro.free(token %10, ptr %17)
  %81 = icmp ne ptr %80, null
  br i1 %81, label %82, label %84

82:                                               ; preds = %79
  %83 = call i64 @llvm.coro.size.i64()
  call void @_ZdlPvm(ptr noundef %80, i64 noundef %83) #3
  br label %84

84:                                               ; preds = %82, %79
  br label %102

85:                                               ; preds = %72
  %86 = landingpad { ptr, i32 }
          catch ptr null
  %87 = extractvalue { ptr, i32 } %86, 0
  store ptr %87, ptr %3, align 8
  %88 = extractvalue { ptr, i32 } %86, 1
  store i32 %88, ptr %4, align 4
  br label %89

89:                                               ; preds = %85
  %90 = load ptr, ptr %3, align 8
  %91 = call ptr @__cxa_begin_catch(ptr %90) #3
  invoke void @_Z25handle_contract_violationRKNSt9contracts18contract_violationE(ptr @.contract)
          to label %92 unwind label %93

92:                                               ; preds = %89
  call void @_ZSt9terminatev() #3
  unreachable

93:                                               ; preds = %89
  %94 = landingpad { ptr, i32 }
          cleanup
  %95 = extractvalue { ptr, i32 } %94, 0
  store ptr %95, ptr %3, align 8
  %96 = extractvalue { ptr, i32 } %94, 1
  store i32 %96, ptr %4, align 4
  invoke void @__cxa_end_catch()
          to label %97 unwind label %107

97:                                               ; preds = %93
  br label %102

98:                                               ; preds = %75
  call void @_Z25handle_contract_violationRKNSt9contracts18contract_violationE(ptr @.contract.5)
  call void @_ZSt9terminatev() #3
  unreachable

99:                                               ; preds = %75
  %100 = getelementptr inbounds %struct.sync, ptr %1, i32 0, i32 0
  %101 = load i64, ptr %100, align 8
  ret i64 %101

102:                                              ; preds = %97, %84, %76
  %103 = load ptr, ptr %3, align 8
  %104 = load i32, ptr %4, align 4
  %105 = insertvalue { ptr, i32 } poison, ptr %103, 0
  %106 = insertvalue { ptr, i32 } %105, i32 %104, 1
  resume { ptr, i32 } %106

107:                                              ; preds = %93, %51
  %108 = landingpad { ptr, i32 }
          catch ptr null
  %109 = extractvalue { ptr, i32 } %108, 0
  call void @__clang_call_terminate(ptr %109) #15
  unreachable

110:                                              ; preds = %70
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
declare void @llvm.lifetime.end.p0(i64 immarg, ptr nocapture) #6

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
declare i1 @llvm.coro.end(ptr, i1, token) #3

; Function Attrs: nobuiltin nounwind
declare void @_ZdlPvm(ptr noundef, i64 noundef) #12

; Function Attrs: nounwind memory(argmem: read)
declare ptr @llvm.coro.free(token, ptr nocapture readonly) #13

; Function Attrs: mustprogress noinline nounwind optnone uwtable
declare dso_local noundef i1 @_ZNK4sync2okEv(ptr noundef nonnull align 8 dereferenceable(8)) #8

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
