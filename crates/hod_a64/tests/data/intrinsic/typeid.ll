; WP-070: clang++-22 -O0 -S -emit-llvm of typeid.cpp (tests/intrinsic.rs), the
; target lines removed (the test adds its own).

%struct.Base = type <{ ptr, i32, [4 x i8] }>
%struct.Other = type { i8 }
%struct.Note = type { i32 }

$_ZN4NoteD2Ev = comdat any

$__clang_call_terminate = comdat any

$_ZTI7Derived = comdat any

$_ZTS7Derived = comdat any

$_ZTI4Base = comdat any

$_ZTS4Base = comdat any

$_ZTI5Other = comdat any

$_ZTS5Other = comdat any

@_ZTIi = external constant ptr
@_ZTId = external constant ptr
@_ZTI7Derived = linkonce_odr dso_local constant { ptr, ptr, ptr } { ptr getelementptr inbounds (ptr, ptr @_ZTVN10__cxxabiv120__si_class_type_infoE, i64 2), ptr @_ZTS7Derived, ptr @_ZTI4Base }, comdat, align 8
@_ZTVN10__cxxabiv120__si_class_type_infoE = external global [0 x ptr]
@_ZTS7Derived = linkonce_odr dso_local constant [9 x i8] c"7Derived\00", comdat, align 1
@_ZTI4Base = linkonce_odr dso_local constant { ptr, ptr } { ptr getelementptr inbounds (ptr, ptr @_ZTVN10__cxxabiv117__class_type_infoE, i64 2), ptr @_ZTS4Base }, comdat, align 8
@_ZTVN10__cxxabiv117__class_type_infoE = external global [0 x ptr]
@_ZTS4Base = linkonce_odr dso_local constant [6 x i8] c"4Base\00", comdat, align 1
@_ZTIPKc = external constant ptr
@_ZTI5Other = linkonce_odr dso_local constant { ptr, ptr } { ptr getelementptr inbounds (ptr, ptr @_ZTVN10__cxxabiv117__class_type_infoE, i64 2), ptr @_ZTS5Other }, comdat, align 8
@_ZTS5Other = linkonce_odr dso_local constant [7 x i8] c"5Other\00", comdat, align 1
@_ZTIl = external constant ptr

; Function Attrs: mustprogress noinline optnone uwtable
define dso_local i32 @t_catch(i32 noundef %0) #0 personality ptr @__gxx_personality_v0 {
  %2 = alloca i32, align 4
  %3 = alloca i32, align 4
  %4 = alloca ptr, align 8
  %5 = alloca i32, align 4
  %6 = alloca ptr, align 8
  %7 = alloca ptr, align 8
  %8 = alloca ptr, align 8
  %9 = alloca double, align 8
  %10 = alloca i32, align 4
  store i32 %0, ptr %3, align 4
  %11 = load i32, ptr %3, align 4
  invoke void @cxx_raise(i32 noundef %11)
          to label %12 unwind label %13

12:                                               ; preds = %1
  br label %63

13:                                               ; preds = %1
  %14 = landingpad { ptr, i32 }
          catch ptr @_ZTIi
          catch ptr @_ZTId
          catch ptr @_ZTI7Derived
          catch ptr @_ZTI4Base
          catch ptr @_ZTIPKc
          catch ptr null
  %15 = extractvalue { ptr, i32 } %14, 0
  store ptr %15, ptr %4, align 8
  %16 = extractvalue { ptr, i32 } %14, 1
  store i32 %16, ptr %5, align 4
  br label %17

17:                                               ; preds = %13
  %18 = load i32, ptr %5, align 4
  %19 = call i32 @llvm.eh.typeid.for.p0(ptr @_ZTIi) #5
  %20 = icmp eq i32 %18, %19
  br i1 %20, label %21, label %27

21:                                               ; preds = %17
  %22 = load ptr, ptr %4, align 8
  %23 = call ptr @__cxa_begin_catch(ptr %22) #5
  %24 = load i32, ptr %23, align 4
  store i32 %24, ptr %10, align 4
  %25 = load i32, ptr %10, align 4
  %26 = add nsw i32 100, %25
  store i32 %26, ptr %2, align 4
  call void @__cxa_end_catch() #5
  br label %64

27:                                               ; preds = %17
  %28 = call i32 @llvm.eh.typeid.for.p0(ptr @_ZTId) #5
  %29 = icmp eq i32 %18, %28
  br i1 %29, label %30, label %34

30:                                               ; preds = %27
  %31 = load ptr, ptr %4, align 8
  %32 = call ptr @__cxa_begin_catch(ptr %31) #5
  %33 = load double, ptr %32, align 8
  store double %33, ptr %9, align 8
  store i32 2, ptr %2, align 4
  call void @__cxa_end_catch() #5
  br label %64

34:                                               ; preds = %27
  %35 = call i32 @llvm.eh.typeid.for.p0(ptr @_ZTI7Derived) #5
  %36 = icmp eq i32 %18, %35
  br i1 %36, label %37, label %44

37:                                               ; preds = %34
  %38 = load ptr, ptr %4, align 8
  %39 = call ptr @__cxa_begin_catch(ptr %38) #5
  store ptr %39, ptr %8, align 8
  %40 = load ptr, ptr %8, align 8, !nonnull !6, !align !7
  %41 = getelementptr inbounds nuw %struct.Base, ptr %40, i32 0, i32 1
  %42 = load i32, ptr %41, align 8
  %43 = add nsw i32 30, %42
  store i32 %43, ptr %2, align 4
  call void @__cxa_end_catch()
  br label %64

44:                                               ; preds = %34
  %45 = call i32 @llvm.eh.typeid.for.p0(ptr @_ZTI4Base) #5
  %46 = icmp eq i32 %18, %45
  br i1 %46, label %47, label %54

47:                                               ; preds = %44
  %48 = load ptr, ptr %4, align 8
  %49 = call ptr @__cxa_begin_catch(ptr %48) #5
  store ptr %49, ptr %7, align 8
  %50 = load ptr, ptr %7, align 8, !nonnull !6, !align !7
  %51 = getelementptr inbounds nuw %struct.Base, ptr %50, i32 0, i32 1
  %52 = load i32, ptr %51, align 8
  %53 = add nsw i32 40, %52
  store i32 %53, ptr %2, align 4
  call void @__cxa_end_catch()
  br label %64

54:                                               ; preds = %44
  %55 = call i32 @llvm.eh.typeid.for.p0(ptr @_ZTIPKc) #5
  %56 = icmp eq i32 %18, %55
  br i1 %56, label %57, label %60

57:                                               ; preds = %54
  %58 = load ptr, ptr %4, align 8
  %59 = call ptr @__cxa_begin_catch(ptr %58) #5
  store ptr %59, ptr %6, align 8
  store i32 5, ptr %2, align 4
  call void @__cxa_end_catch() #5
  br label %64

60:                                               ; preds = %54
  %61 = load ptr, ptr %4, align 8
  %62 = call ptr @__cxa_begin_catch(ptr %61) #5
  store i32 6, ptr %2, align 4
  call void @__cxa_end_catch()
  br label %64

63:                                               ; preds = %12
  store i32 0, ptr %2, align 4
  br label %64

64:                                               ; preds = %63, %21, %30, %37, %47, %57, %60
  %65 = load i32, ptr %2, align 4
  ret i32 %65
}

declare void @cxx_raise(i32 noundef) #1

declare i32 @__gxx_personality_v0(...)

; Function Attrs: nounwind memory(none)
declare i32 @llvm.eh.typeid.for.p0(ptr) #2

declare ptr @__cxa_begin_catch(ptr)

declare void @__cxa_end_catch()

; Function Attrs: mustprogress noinline optnone uwtable
define dso_local i32 @t_nested(i32 noundef %0) #0 personality ptr @__gxx_personality_v0 {
  %2 = alloca i32, align 4
  %3 = alloca i32, align 4
  %4 = alloca ptr, align 8
  %5 = alloca i32, align 4
  %6 = alloca ptr, align 8
  %7 = alloca %struct.Other, align 1
  %8 = alloca double, align 8
  %9 = alloca i32, align 4
  %10 = alloca double, align 8
  store i32 %0, ptr %2, align 4
  store i32 0, ptr %3, align 4
  %11 = load i32, ptr %2, align 4
  invoke void @cxx_raise(i32 noundef %11)
          to label %12 unwind label %13

12:                                               ; preds = %1
  br label %79

13:                                               ; preds = %1
  %14 = landingpad { ptr, i32 }
          catch ptr @_ZTId
          catch ptr @_ZTI5Other
          catch ptr @_ZTI4Base
          catch ptr @_ZTIi
  %15 = extractvalue { ptr, i32 } %14, 0
  store ptr %15, ptr %4, align 8
  %16 = extractvalue { ptr, i32 } %14, 1
  store i32 %16, ptr %5, align 4
  br label %17

17:                                               ; preds = %13
  %18 = load i32, ptr %5, align 4
  %19 = call i32 @llvm.eh.typeid.for.p0(ptr @_ZTId) #5
  %20 = icmp eq i32 %18, %19
  br i1 %20, label %21, label %27

21:                                               ; preds = %17
  %22 = load ptr, ptr %4, align 8
  %23 = call ptr @__cxa_begin_catch(ptr %22) #5
  %24 = load double, ptr %23, align 8
  store double %24, ptr %8, align 8
  %25 = load i32, ptr %3, align 4
  %26 = add nsw i32 %25, 10
  store i32 %26, ptr %3, align 4
  invoke void @__cxa_rethrow() #6
          to label %85 unwind label %53

27:                                               ; preds = %17
  %28 = call i32 @llvm.eh.typeid.for.p0(ptr @_ZTI5Other) #5
  %29 = icmp eq i32 %18, %28
  br i1 %29, label %30, label %36

30:                                               ; preds = %27
  %31 = load ptr, ptr %4, align 8
  %32 = call ptr @__cxa_begin_catch(ptr %31) #5
  %33 = load i32, ptr %3, align 4
  %34 = add nsw i32 %33, 20
  store i32 %34, ptr %3, align 4
  invoke void @__cxa_end_catch()
          to label %35 unwind label %49

35:                                               ; preds = %30
  br label %79

36:                                               ; preds = %27
  %37 = call i32 @llvm.eh.typeid.for.p0(ptr @_ZTI4Base) #5
  %38 = icmp eq i32 %18, %37
  br i1 %38, label %39, label %57

39:                                               ; preds = %36
  %40 = load ptr, ptr %4, align 8
  %41 = call ptr @__cxa_begin_catch(ptr %40) #5
  store ptr %41, ptr %6, align 8
  %42 = load ptr, ptr %6, align 8, !nonnull !6, !align !7
  %43 = getelementptr inbounds nuw %struct.Base, ptr %42, i32 0, i32 1
  %44 = load i32, ptr %43, align 8
  %45 = add nsw i32 50, %44
  %46 = load i32, ptr %3, align 4
  %47 = add nsw i32 %46, %45
  store i32 %47, ptr %3, align 4
  invoke void @__cxa_end_catch()
          to label %48 unwind label %49

48:                                               ; preds = %39
  br label %79

49:                                               ; preds = %30, %39
  %50 = landingpad { ptr, i32 }
          catch ptr @_ZTId
          catch ptr @_ZTIi
  %51 = extractvalue { ptr, i32 } %50, 0
  store ptr %51, ptr %4, align 8
  %52 = extractvalue { ptr, i32 } %50, 1
  store i32 %52, ptr %5, align 4
  br label %57

53:                                               ; preds = %21
  %54 = landingpad { ptr, i32 }
          cleanup
          catch ptr @_ZTId
          catch ptr @_ZTIi
  %55 = extractvalue { ptr, i32 } %54, 0
  store ptr %55, ptr %4, align 8
  %56 = extractvalue { ptr, i32 } %54, 1
  store i32 %56, ptr %5, align 4
  call void @__cxa_end_catch() #5
  br label %57

57:                                               ; preds = %53, %49, %36
  %58 = load i32, ptr %5, align 4
  %59 = call i32 @llvm.eh.typeid.for.p0(ptr @_ZTId) #5
  %60 = icmp eq i32 %58, %59
  br i1 %60, label %61, label %69

61:                                               ; preds = %57
  %62 = load ptr, ptr %4, align 8
  %63 = call ptr @__cxa_begin_catch(ptr %62) #5
  %64 = load double, ptr %63, align 8
  store double %64, ptr %10, align 8
  %65 = load i32, ptr %3, align 4
  %66 = add nsw i32 %65, 1000
  store i32 %66, ptr %3, align 4
  call void @__cxa_end_catch() #5
  br label %67

67:                                               ; preds = %61, %72, %79
  %68 = load i32, ptr %3, align 4
  ret i32 %68

69:                                               ; preds = %57
  %70 = call i32 @llvm.eh.typeid.for.p0(ptr @_ZTIi) #5
  %71 = icmp eq i32 %58, %70
  br i1 %71, label %72, label %80

72:                                               ; preds = %69
  %73 = load ptr, ptr %4, align 8
  %74 = call ptr @__cxa_begin_catch(ptr %73) #5
  %75 = load i32, ptr %74, align 4
  store i32 %75, ptr %9, align 4
  %76 = load i32, ptr %9, align 4
  %77 = load i32, ptr %3, align 4
  %78 = add nsw i32 %77, %76
  store i32 %78, ptr %3, align 4
  call void @__cxa_end_catch() #5
  br label %67

79:                                               ; preds = %35, %48, %12
  br label %67

80:                                               ; preds = %69
  %81 = load ptr, ptr %4, align 8
  %82 = load i32, ptr %5, align 4
  %83 = insertvalue { ptr, i32 } poison, ptr %81, 0
  %84 = insertvalue { ptr, i32 } %83, i32 %82, 1
  resume { ptr, i32 } %84

85:                                               ; preds = %21
  unreachable
}

declare void @__cxa_rethrow()

; Function Attrs: mustprogress noinline optnone uwtable
define dso_local i64 @t_cleanup(i32 noundef %0, i64 noundef %1, i64 noundef %2) #0 personality ptr @__gxx_personality_v0 {
  %4 = alloca i64, align 8
  %5 = alloca i32, align 4
  %6 = alloca i64, align 8
  %7 = alloca i64, align 8
  %8 = alloca i64, align 8
  %9 = alloca i64, align 8
  %10 = alloca %struct.Note, align 4
  %11 = alloca ptr, align 8
  %12 = alloca i32, align 4
  %13 = alloca i64, align 8
  %14 = alloca ptr, align 8
  store i32 %0, ptr %5, align 4
  store i64 %1, ptr %6, align 8
  store i64 %2, ptr %7, align 8
  %15 = load i64, ptr %6, align 8
  %16 = mul nsw i64 %15, 3
  store i64 %16, ptr %8, align 8
  %17 = load i64, ptr %7, align 8
  %18 = add nsw i64 %17, 7
  store i64 %18, ptr %9, align 8
  %19 = getelementptr inbounds nuw %struct.Note, ptr %10, i32 0, i32 0
  %20 = load i32, ptr %5, align 4
  store i32 %20, ptr %19, align 4
  %21 = load i32, ptr %5, align 4
  invoke void @cxx_raise(i32 noundef %21)
          to label %22 unwind label %23

22:                                               ; preds = %3
  call void @_ZN4NoteD2Ev(ptr noundef nonnull align 4 dereferenceable(4) %10) #5
  br label %49

23:                                               ; preds = %3
  %24 = landingpad { ptr, i32 }
          cleanup
          catch ptr @_ZTI5Other
          catch ptr @_ZTIl
  %25 = extractvalue { ptr, i32 } %24, 0
  store ptr %25, ptr %11, align 8
  %26 = extractvalue { ptr, i32 } %24, 1
  store i32 %26, ptr %12, align 4
  call void @_ZN4NoteD2Ev(ptr noundef nonnull align 4 dereferenceable(4) %10) #5
  br label %27

27:                                               ; preds = %23
  %28 = load i32, ptr %12, align 4
  %29 = call i32 @llvm.eh.typeid.for.p0(ptr @_ZTI5Other) #5
  %30 = icmp eq i32 %28, %29
  br i1 %30, label %31, label %37

31:                                               ; preds = %27
  %32 = load ptr, ptr %11, align 8
  %33 = call ptr @__cxa_begin_catch(ptr %32) #5
  store ptr %33, ptr %14, align 8
  %34 = load i64, ptr %8, align 8
  %35 = load i64, ptr %9, align 8
  %36 = add nsw i64 %34, %35
  store i64 %36, ptr %4, align 8
  call void @__cxa_end_catch()
  br label %53

37:                                               ; preds = %27
  %38 = call i32 @llvm.eh.typeid.for.p0(ptr @_ZTIl) #5
  %39 = icmp eq i32 %28, %38
  br i1 %39, label %40, label %55

40:                                               ; preds = %37
  %41 = load ptr, ptr %11, align 8
  %42 = call ptr @__cxa_begin_catch(ptr %41) #5
  %43 = load i64, ptr %42, align 8
  store i64 %43, ptr %13, align 8
  %44 = load i64, ptr %8, align 8
  %45 = load i64, ptr %9, align 8
  %46 = sub nsw i64 %44, %45
  %47 = load i64, ptr %13, align 8
  %48 = add nsw i64 %46, %47
  store i64 %48, ptr %4, align 8
  call void @__cxa_end_catch() #5
  br label %53

49:                                               ; preds = %22
  %50 = load i64, ptr %8, align 8
  %51 = load i64, ptr %9, align 8
  %52 = mul nsw i64 %50, %51
  store i64 %52, ptr %4, align 8
  br label %53

53:                                               ; preds = %49, %31, %40
  %54 = load i64, ptr %4, align 8
  ret i64 %54

55:                                               ; preds = %37
  %56 = load ptr, ptr %11, align 8
  %57 = load i32, ptr %12, align 4
  %58 = insertvalue { ptr, i32 } poison, ptr %56, 0
  %59 = insertvalue { ptr, i32 } %58, i32 %57, 1
  resume { ptr, i32 } %59
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZN4NoteD2Ev(ptr noundef nonnull align 4 dereferenceable(4) %0) unnamed_addr #3 comdat personality ptr @__gxx_personality_v0 {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = getelementptr inbounds nuw %struct.Note, ptr %3, i32 0, i32 0
  %5 = load i32, ptr %4, align 4
  invoke void @cxx_note(i32 noundef %5)
          to label %6 unwind label %7

6:                                                ; preds = %1
  ret void

7:                                                ; preds = %1
  %8 = landingpad { ptr, i32 }
          catch ptr null
  %9 = extractvalue { ptr, i32 } %8, 0
  call void @__clang_call_terminate(ptr %9) #7
  unreachable
}

declare void @cxx_note(i32 noundef) #1

; Function Attrs: noinline noreturn nounwind uwtable
define linkonce_odr hidden void @__clang_call_terminate(ptr noundef %0) #4 comdat {
  %2 = call ptr @__cxa_begin_catch(ptr %0) #5
  call void @_ZSt9terminatev() #7
  unreachable
}

declare void @_ZSt9terminatev()

attributes #0 = { mustprogress noinline optnone uwtable "frame-pointer"="non-leaf-no-reserve" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #1 = { "frame-pointer"="non-leaf-no-reserve" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #2 = { nounwind memory(none) }
attributes #3 = { mustprogress noinline nounwind optnone uwtable "frame-pointer"="non-leaf-no-reserve" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #4 = { noinline noreturn nounwind uwtable "frame-pointer"="non-leaf-no-reserve" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #5 = { nounwind }
attributes #6 = { noreturn }
attributes #7 = { noreturn nounwind }

!llvm.module.flags = !{!0, !1, !2, !3, !4}
!llvm.ident = !{!5}

!0 = !{i32 1, !"wchar_size", i32 4}
!1 = !{i32 8, !"PIC Level", i32 2}
!2 = !{i32 7, !"PIE Level", i32 2}
!3 = !{i32 7, !"uwtable", i32 2}
!4 = !{i32 7, !"frame-pointer", i32 4}
!5 = !{!"Debian clang version 22.1.8 (1~deb13u4)"}
!6 = !{}
!7 = !{i64 8}
