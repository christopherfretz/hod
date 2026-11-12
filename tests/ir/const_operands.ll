; Constant operands that hold addresses, as clang++ -O0 writes them for C++:
; member-function pointers (`{ i64 ptrtoint (ptr @f to i64), i64 0 }` stored),
; a table of function pointers filled in code, a difference of a function
; and a variable (two sections), and addresses truncated to 32 and 16 bits.
;@ covers: spec/ir.md §4.2 addresses as operands; spec/codegen.md §5.5.1 (WP-072)
;@ tier: quick
;@ unsupported(x86_64): *
;@ stdout: cross=1 low=1 pair=1
;@ stdout: r1=25 r2=15 r3=5 r4=46 d=28 e=0 same=1

target triple = "aarch64-unknown-linux-gnu"

%struct.S = type <{ ptr, i32, [4 x i8] }>
%struct.P = type { ptr, i16 }

$_ZN1SC2Ev = comdat any

$_ZNK1S5twiceEi = comdat any

$_ZNK1S4plusEi = comdat any

$_ZNK1S4virtEi = comdat any

$_ZTV1S = comdat any

@buf = dso_local global [16 x i32] zeroinitializer, align 4
@table_in_code = dso_local global [2 x ptr] zeroinitializer, align 8
@_ZL7counter = internal global i32 0, align 4
@.str = private unnamed_addr constant [25 x i8] c"cross=%d low=%d pair=%d\0A\00", align 1
@.str.1 = private unnamed_addr constant [45 x i8] c"r1=%d r2=%d r3=%d r4=%d d=%ld e=%ld same=%d\0A\00", align 1
@_ZTV1S = linkonce_odr dso_local unnamed_addr constant { [3 x ptr] } { [3 x ptr] [ptr null, ptr null, ptr @_ZNK1S4virtEi] }, comdat, align 8

; Function Attrs: mustprogress noinline norecurse nounwind optnone uwtable
define dso_local noundef i32 @main() #0 {
  %1 = alloca i32, align 4
  %2 = alloca %struct.S, align 8
  %3 = alloca { i64, i64 }, align 8
  %4 = alloca i32, align 4
  %5 = alloca i32, align 4
  %6 = alloca i32, align 4
  %7 = alloca i32, align 4
  %8 = alloca i64, align 8
  %9 = alloca i64, align 8
  %10 = alloca i8, align 1
  %11 = alloca ptr, align 8
  %12 = alloca ptr, align 8
  %13 = alloca i64, align 8
  %14 = alloca i32, align 4
  %15 = alloca i32, align 4
  %16 = alloca i32, align 4
  %17 = alloca %struct.P, align 8
  %18 = alloca i32, align 4
  store i32 0, ptr %1, align 4
  call void @_ZN1SC2Ev(ptr noundef nonnull align 8 dereferenceable(12) %2) #3
  %19 = getelementptr inbounds nuw %struct.S, ptr %2, i32 0, i32 1
  store i32 5, ptr %19, align 8
  store { i64, i64 } { i64 ptrtoint (ptr @_ZNK1S5twiceEi to i64), i64 0 }, ptr %3, align 8
  %20 = load { i64, i64 }, ptr %3, align 8
  %21 = extractvalue { i64, i64 } %20, 1
  %22 = ashr i64 %21, 1
  %23 = getelementptr inbounds i8, ptr %2, i64 %22
  %24 = extractvalue { i64, i64 } %20, 0
  %25 = and i64 %21, 1
  %26 = icmp ne i64 %25, 0
  br i1 %26, label %27, label %31

27:                                               ; preds = %0
  %28 = load ptr, ptr %23, align 8
  %29 = getelementptr i8, ptr %28, i64 %24, !nosanitize !6
  %30 = load ptr, ptr %29, align 8, !nosanitize !6
  br label %33

31:                                               ; preds = %0
  %32 = inttoptr i64 %24 to ptr
  br label %33

33:                                               ; preds = %31, %27
  %34 = phi ptr [ %30, %27 ], [ %32, %31 ]
  %35 = call noundef i32 %34(ptr noundef nonnull align 8 dereferenceable(12) %23, i32 noundef 10)
  store i32 %35, ptr %4, align 4
  store { i64, i64 } { i64 ptrtoint (ptr @_ZNK1S4plusEi to i64), i64 0 }, ptr %3, align 8
  %36 = load { i64, i64 }, ptr %3, align 8
  %37 = extractvalue { i64, i64 } %36, 1
  %38 = ashr i64 %37, 1
  %39 = getelementptr inbounds i8, ptr %2, i64 %38
  %40 = extractvalue { i64, i64 } %36, 0
  %41 = and i64 %37, 1
  %42 = icmp ne i64 %41, 0
  br i1 %42, label %43, label %47

43:                                               ; preds = %33
  %44 = load ptr, ptr %39, align 8
  %45 = getelementptr i8, ptr %44, i64 %40, !nosanitize !6
  %46 = load ptr, ptr %45, align 8, !nosanitize !6
  br label %49

47:                                               ; preds = %33
  %48 = inttoptr i64 %40 to ptr
  br label %49

49:                                               ; preds = %47, %43
  %50 = phi ptr [ %46, %43 ], [ %48, %47 ]
  %51 = call noundef i32 %50(ptr noundef nonnull align 8 dereferenceable(12) %39, i32 noundef 10)
  store i32 %51, ptr %5, align 4
  store { i64, i64 } { i64 0, i64 1 }, ptr %3, align 8
  %52 = load { i64, i64 }, ptr %3, align 8
  %53 = extractvalue { i64, i64 } %52, 1
  %54 = ashr i64 %53, 1
  %55 = getelementptr inbounds i8, ptr %2, i64 %54
  %56 = extractvalue { i64, i64 } %52, 0
  %57 = and i64 %53, 1
  %58 = icmp ne i64 %57, 0
  br i1 %58, label %59, label %63

59:                                               ; preds = %49
  %60 = load ptr, ptr %55, align 8
  %61 = getelementptr i8, ptr %60, i64 %56, !nosanitize !6
  %62 = load ptr, ptr %61, align 8, !nosanitize !6
  br label %65

63:                                               ; preds = %49
  %64 = inttoptr i64 %56 to ptr
  br label %65

65:                                               ; preds = %63, %59
  %66 = phi ptr [ %62, %59 ], [ %64, %63 ]
  %67 = call noundef i32 %66(ptr noundef nonnull align 8 dereferenceable(12) %55, i32 noundef 10)
  store i32 %67, ptr %6, align 4
  store ptr @_ZL6squarei, ptr @table_in_code, align 8
  store ptr @_ZL6negatei, ptr getelementptr inbounds ([2 x ptr], ptr @table_in_code, i64 0, i64 1), align 8
  %68 = load ptr, ptr @table_in_code, align 8
  %69 = call noundef i32 %68(i32 noundef 7)
  %70 = load ptr, ptr getelementptr inbounds ([2 x ptr], ptr @table_in_code, i64 0, i64 1), align 8
  %71 = call noundef i32 %70(i32 noundef 3)
  %72 = add nsw i32 %69, %71
  store i32 %72, ptr %7, align 4
  store i64 sub nsw (i64 ptrtoint (ptr getelementptr inbounds ([16 x i32], ptr @buf, i64 0, i64 9) to i64), i64 ptrtoint (ptr getelementptr inbounds ([16 x i32], ptr @buf, i64 0, i64 2) to i64)), ptr %8, align 8
  store i64 sub nsw (i64 ptrtoint (ptr @_ZL7counter to i64), i64 ptrtoint (ptr @_ZL7counter to i64)), ptr %9, align 8
  store i8 1, ptr %10, align 1
  store volatile ptr @_ZL6squarei, ptr %11, align 8
  store volatile ptr @buf, ptr %12, align 8
  store i64 sub nsw (i64 ptrtoint (ptr @_ZL6squarei to i64), i64 ptrtoint (ptr getelementptr inbounds ([16 x i32], ptr @buf, i64 0, i64 3) to i64)), ptr %13, align 8
  %73 = load i64, ptr %13, align 8
  %74 = load volatile ptr, ptr %11, align 8
  %75 = ptrtoint ptr %74 to i64
  %76 = load volatile ptr, ptr %12, align 8
  %77 = getelementptr inbounds i32, ptr %76, i64 3
  %78 = ptrtoint ptr %77 to i64
  %79 = sub nsw i64 %75, %78
  %80 = icmp eq i64 %73, %79
  %81 = zext i1 %80 to i32
  store i32 %81, ptr %14, align 4
  store i32 ptrtoint (ptr @buf to i32), ptr %15, align 4
  %82 = load i32, ptr %15, align 4
  %83 = load volatile ptr, ptr %12, align 8
  %84 = ptrtoint ptr %83 to i64
  %85 = trunc i64 %84 to i32
  %86 = icmp eq i32 %82, %85
  %87 = zext i1 %86 to i32
  store i32 %87, ptr %16, align 4
  %88 = getelementptr inbounds nuw %struct.P, ptr %17, i32 0, i32 0
  store ptr getelementptr inbounds ([16 x i32], ptr @buf, i64 0, i64 1), ptr %88, align 8
  %89 = getelementptr inbounds nuw %struct.P, ptr %17, i32 0, i32 1
  store i16 ptrtoint (ptr @buf to i16), ptr %89, align 8
  %90 = getelementptr inbounds nuw %struct.P, ptr %17, i32 0, i32 0
  %91 = load ptr, ptr %90, align 8
  %92 = load volatile ptr, ptr %12, align 8
  %93 = getelementptr inbounds i32, ptr %92, i64 1
  %94 = icmp eq ptr %91, %93
  br i1 %94, label %95, label %104

95:                                               ; preds = %65
  %96 = getelementptr inbounds nuw %struct.P, ptr %17, i32 0, i32 1
  %97 = load i16, ptr %96, align 8
  %98 = zext i16 %97 to i32
  %99 = load volatile ptr, ptr %12, align 8
  %100 = ptrtoint ptr %99 to i64
  %101 = trunc i64 %100 to i16
  %102 = zext i16 %101 to i32
  %103 = icmp eq i32 %98, %102
  br label %104

104:                                              ; preds = %95, %65
  %105 = phi i1 [ false, %65 ], [ %103, %95 ]
  %106 = zext i1 %105 to i32
  store i32 %106, ptr %18, align 4
  %107 = load i32, ptr %14, align 4
  %108 = load i32, ptr %16, align 4
  %109 = load i32, ptr %18, align 4
  %110 = call i32 (ptr, ...) @printf(ptr noundef @.str, i32 noundef %107, i32 noundef %108, i32 noundef %109)
  %111 = load i32, ptr %4, align 4
  %112 = load i32, ptr %5, align 4
  %113 = load i32, ptr %6, align 4
  %114 = load i32, ptr %7, align 4
  %115 = load i64, ptr %8, align 8
  %116 = load i64, ptr %9, align 8
  %117 = load i8, ptr %10, align 1
  %118 = trunc i8 %117 to i1
  %119 = zext i1 %118 to i32
  %120 = call i32 (ptr, ...) @printf(ptr noundef @.str.1, i32 noundef %111, i32 noundef %112, i32 noundef %113, i32 noundef %114, i64 noundef %115, i64 noundef %116, i32 noundef %119)
  ret i32 0
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local void @_ZN1SC2Ev(ptr noundef nonnull align 8 dereferenceable(12) %0) unnamed_addr #1 comdat {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
  %3 = load ptr, ptr %2, align 8
  store ptr getelementptr inbounds inrange(-16, 8) ({ [3 x ptr] }, ptr @_ZTV1S, i32 0, i32 0, i32 2), ptr %3, align 8
  ret void
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local noundef i32 @_ZNK1S5twiceEi(ptr noundef nonnull align 8 dereferenceable(12) %0, i32 noundef %1) #1 comdat {
  %3 = alloca ptr, align 8
  %4 = alloca i32, align 4
  store ptr %0, ptr %3, align 8
  store i32 %1, ptr %4, align 4
  %5 = load ptr, ptr %3, align 8
  %6 = load i32, ptr %4, align 4
  %7 = mul nsw i32 2, %6
  %8 = getelementptr inbounds nuw %struct.S, ptr %5, i32 0, i32 1
  %9 = load i32, ptr %8, align 8
  %10 = add nsw i32 %7, %9
  ret i32 %10
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local noundef i32 @_ZNK1S4plusEi(ptr noundef nonnull align 8 dereferenceable(12) %0, i32 noundef %1) #1 comdat {
  %3 = alloca ptr, align 8
  %4 = alloca i32, align 4
  store ptr %0, ptr %3, align 8
  store i32 %1, ptr %4, align 4
  %5 = load ptr, ptr %3, align 8
  %6 = load i32, ptr %4, align 4
  %7 = getelementptr inbounds nuw %struct.S, ptr %5, i32 0, i32 1
  %8 = load i32, ptr %7, align 8
  %9 = add nsw i32 %6, %8
  ret i32 %9
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define internal noundef i32 @_ZL6squarei(i32 noundef %0) #1 {
  %2 = alloca i32, align 4
  store i32 %0, ptr %2, align 4
  %3 = load i32, ptr %2, align 4
  %4 = load i32, ptr %2, align 4
  %5 = mul nsw i32 %3, %4
  ret i32 %5
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define internal noundef i32 @_ZL6negatei(i32 noundef %0) #1 {
  %2 = alloca i32, align 4
  store i32 %0, ptr %2, align 4
  %3 = load i32, ptr %2, align 4
  %4 = sub nsw i32 0, %3
  ret i32 %4
}

declare i32 @printf(ptr noundef, ...) #2

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local noundef i32 @_ZNK1S4virtEi(ptr noundef nonnull align 8 dereferenceable(12) %0, i32 noundef %1) unnamed_addr #1 comdat {
  %3 = alloca ptr, align 8
  %4 = alloca i32, align 4
  store ptr %0, ptr %3, align 8
  store i32 %1, ptr %4, align 4
  %5 = load ptr, ptr %3, align 8
  %6 = load i32, ptr %4, align 4
  %7 = getelementptr inbounds nuw %struct.S, ptr %5, i32 0, i32 1
  %8 = load i32, ptr %7, align 8
  %9 = sub nsw i32 %6, %8
  ret i32 %9
}

attributes #0 = { mustprogress noinline norecurse nounwind optnone uwtable "frame-pointer"="non-leaf" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #1 = { mustprogress noinline nounwind optnone uwtable "frame-pointer"="non-leaf" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #2 = { "frame-pointer"="non-leaf" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #3 = { nounwind }

!llvm.module.flags = !{!0, !1, !2, !3, !4}

!0 = !{i32 1, !"wchar_size", i32 4}
!1 = !{i32 8, !"PIC Level", i32 2}
!2 = !{i32 7, !"PIE Level", i32 2}
!3 = !{i32 7, !"uwtable", i32 2}
!4 = !{i32 7, !"frame-pointer", i32 1}
!6 = !{}
