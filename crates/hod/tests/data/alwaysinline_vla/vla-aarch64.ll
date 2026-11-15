; ModuleID = 'vla.c'
source_filename = "vla.c"
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i8:8:32-i16:16:32-i64:64-i128:128-n32:64-S128-Fn32"
target triple = "aarch64-unknown-linux-gnu"

%struct.s24 = type { i64, i64, i64 }
%struct.a64 = type { i8, [63 x i8] }
%union.lookup_t = type { ptr }

; Function Attrs: noinline nounwind optnone uwtable
define dso_local i64 @call_sizes(i64 noundef %0) #0 {
  %2 = alloca i64, align 8
  store i64 %0, ptr %2, align 8
  %3 = load i64, ptr %2, align 8
  %4 = call i64 @sum_chars(i64 noundef %3)
  %5 = load i64, ptr %2, align 8
  %6 = call i64 @sum_ints(i64 noundef %5)
  %7 = mul i64 3, %6
  %8 = add i64 %4, %7
  %9 = load i64, ptr %2, align 8
  %10 = call i64 @sum_doubles(i64 noundef %9)
  %11 = mul i64 5, %10
  %12 = add i64 %8, %11
  %13 = load i64, ptr %2, align 8
  %14 = call i64 @sum_s24(i64 noundef %13)
  %15 = mul i64 7, %14
  %16 = add i64 %12, %15
  ret i64 %16
}

; Function Attrs: alwaysinline nounwind uwtable
define internal i64 @sum_chars(i64 noundef %0) #1 {
  %2 = alloca i64, align 8
  %3 = alloca ptr, align 8
  %4 = alloca i64, align 8
  %5 = alloca i64, align 8
  %6 = alloca i64, align 8
  %7 = alloca i64, align 8
  store i64 %0, ptr %2, align 8
  %8 = load i64, ptr %2, align 8
  %9 = call ptr @llvm.stacksave.p0()
  store ptr %9, ptr %3, align 8
  %10 = alloca i8, i64 %8, align 1
  store i64 %8, ptr %4, align 8
  store i64 0, ptr %5, align 8
  br label %11

11:                                               ; preds = %22, %1
  %12 = load i64, ptr %5, align 8
  %13 = load i64, ptr %2, align 8
  %14 = icmp ult i64 %12, %13
  br i1 %14, label %15, label %25

15:                                               ; preds = %11
  %16 = load i64, ptr %5, align 8
  %17 = mul i64 %16, 7
  %18 = add i64 %17, 1
  %19 = trunc i64 %18 to i8
  %20 = load i64, ptr %5, align 8
  %21 = getelementptr inbounds nuw i8, ptr %10, i64 %20
  store i8 %19, ptr %21, align 1
  br label %22

22:                                               ; preds = %15
  %23 = load i64, ptr %5, align 8
  %24 = add i64 %23, 1
  store i64 %24, ptr %5, align 8
  br label %11, !llvm.loop !5

25:                                               ; preds = %11
  store i64 0, ptr %6, align 8
  store i64 0, ptr %7, align 8
  br label %26

26:                                               ; preds = %37, %25
  %27 = load i64, ptr %7, align 8
  %28 = load i64, ptr %2, align 8
  %29 = icmp ult i64 %27, %28
  br i1 %29, label %30, label %40

30:                                               ; preds = %26
  %31 = load i64, ptr %7, align 8
  %32 = getelementptr inbounds nuw i8, ptr %10, i64 %31
  %33 = load i8, ptr %32, align 1
  %34 = zext i8 %33 to i64
  %35 = load i64, ptr %6, align 8
  %36 = add i64 %35, %34
  store i64 %36, ptr %6, align 8
  br label %37

37:                                               ; preds = %30
  %38 = load i64, ptr %7, align 8
  %39 = add i64 %38, 1
  store i64 %39, ptr %7, align 8
  br label %26, !llvm.loop !7

40:                                               ; preds = %26
  %41 = load i64, ptr %6, align 8
  %42 = load ptr, ptr %3, align 8
  call void @llvm.stackrestore.p0(ptr %42)
  ret i64 %41
}

; Function Attrs: alwaysinline nounwind uwtable
define internal i64 @sum_ints(i64 noundef %0) #1 {
  %2 = alloca i64, align 8
  %3 = alloca ptr, align 8
  %4 = alloca i64, align 8
  %5 = alloca i64, align 8
  %6 = alloca i64, align 8
  %7 = alloca i64, align 8
  store i64 %0, ptr %2, align 8
  %8 = load i64, ptr %2, align 8
  %9 = call ptr @llvm.stacksave.p0()
  store ptr %9, ptr %3, align 8
  %10 = alloca i32, i64 %8, align 4
  store i64 %8, ptr %4, align 8
  store i64 0, ptr %5, align 8
  br label %11

11:                                               ; preds = %22, %1
  %12 = load i64, ptr %5, align 8
  %13 = load i64, ptr %2, align 8
  %14 = icmp ult i64 %12, %13
  br i1 %14, label %15, label %25

15:                                               ; preds = %11
  %16 = load i64, ptr %5, align 8
  %17 = mul i64 %16, 3
  %18 = sub i64 %17, 5
  %19 = trunc i64 %18 to i32
  %20 = load i64, ptr %5, align 8
  %21 = getelementptr inbounds nuw i32, ptr %10, i64 %20
  store i32 %19, ptr %21, align 4
  br label %22

22:                                               ; preds = %15
  %23 = load i64, ptr %5, align 8
  %24 = add i64 %23, 1
  store i64 %24, ptr %5, align 8
  br label %11, !llvm.loop !8

25:                                               ; preds = %11
  store i64 0, ptr %6, align 8
  store i64 0, ptr %7, align 8
  br label %26

26:                                               ; preds = %37, %25
  %27 = load i64, ptr %7, align 8
  %28 = load i64, ptr %2, align 8
  %29 = icmp ult i64 %27, %28
  br i1 %29, label %30, label %40

30:                                               ; preds = %26
  %31 = load i64, ptr %7, align 8
  %32 = getelementptr inbounds nuw i32, ptr %10, i64 %31
  %33 = load i32, ptr %32, align 4
  %34 = sext i32 %33 to i64
  %35 = load i64, ptr %6, align 8
  %36 = add i64 %35, %34
  store i64 %36, ptr %6, align 8
  br label %37

37:                                               ; preds = %30
  %38 = load i64, ptr %7, align 8
  %39 = add i64 %38, 1
  store i64 %39, ptr %7, align 8
  br label %26, !llvm.loop !9

40:                                               ; preds = %26
  %41 = load i64, ptr %6, align 8
  %42 = load ptr, ptr %3, align 8
  call void @llvm.stackrestore.p0(ptr %42)
  ret i64 %41
}

; Function Attrs: alwaysinline nounwind uwtable
define internal i64 @sum_doubles(i64 noundef %0) #1 {
  %2 = alloca i64, align 8
  %3 = alloca ptr, align 8
  %4 = alloca i64, align 8
  %5 = alloca i64, align 8
  %6 = alloca double, align 8
  %7 = alloca i64, align 8
  store i64 %0, ptr %2, align 8
  %8 = load i64, ptr %2, align 8
  %9 = call ptr @llvm.stacksave.p0()
  store ptr %9, ptr %3, align 8
  %10 = alloca double, i64 %8, align 8
  store i64 %8, ptr %4, align 8
  store i64 0, ptr %5, align 8
  br label %11

11:                                               ; preds = %21, %1
  %12 = load i64, ptr %5, align 8
  %13 = load i64, ptr %2, align 8
  %14 = icmp ult i64 %12, %13
  br i1 %14, label %15, label %24

15:                                               ; preds = %11
  %16 = load i64, ptr %5, align 8
  %17 = uitofp i64 %16 to double
  %18 = fmul double %17, 5.000000e-01
  %19 = load i64, ptr %5, align 8
  %20 = getelementptr inbounds nuw double, ptr %10, i64 %19
  store double %18, ptr %20, align 8
  br label %21

21:                                               ; preds = %15
  %22 = load i64, ptr %5, align 8
  %23 = add i64 %22, 1
  store i64 %23, ptr %5, align 8
  br label %11, !llvm.loop !10

24:                                               ; preds = %11
  store double 0.000000e+00, ptr %6, align 8
  store i64 0, ptr %7, align 8
  br label %25

25:                                               ; preds = %35, %24
  %26 = load i64, ptr %7, align 8
  %27 = load i64, ptr %2, align 8
  %28 = icmp ult i64 %26, %27
  br i1 %28, label %29, label %38

29:                                               ; preds = %25
  %30 = load i64, ptr %7, align 8
  %31 = getelementptr inbounds nuw double, ptr %10, i64 %30
  %32 = load double, ptr %31, align 8
  %33 = load double, ptr %6, align 8
  %34 = fadd double %33, %32
  store double %34, ptr %6, align 8
  br label %35

35:                                               ; preds = %29
  %36 = load i64, ptr %7, align 8
  %37 = add i64 %36, 1
  store i64 %37, ptr %7, align 8
  br label %25, !llvm.loop !11

38:                                               ; preds = %25
  %39 = load double, ptr %6, align 8
  %40 = fptoui double %39 to i64
  %41 = load ptr, ptr %3, align 8
  call void @llvm.stackrestore.p0(ptr %41)
  ret i64 %40
}

; Function Attrs: alwaysinline nounwind uwtable
define internal i64 @sum_s24(i64 noundef %0) #1 {
  %2 = alloca i64, align 8
  %3 = alloca ptr, align 8
  %4 = alloca i64, align 8
  %5 = alloca i64, align 8
  %6 = alloca i64, align 8
  %7 = alloca i64, align 8
  store i64 %0, ptr %2, align 8
  %8 = load i64, ptr %2, align 8
  %9 = call ptr @llvm.stacksave.p0()
  store ptr %9, ptr %3, align 8
  %10 = alloca %struct.s24, i64 %8, align 8
  store i64 %8, ptr %4, align 8
  store i64 0, ptr %5, align 8
  br label %11

11:                                               ; preds = %28, %1
  %12 = load i64, ptr %5, align 8
  %13 = load i64, ptr %2, align 8
  %14 = icmp ult i64 %12, %13
  br i1 %14, label %15, label %31

15:                                               ; preds = %11
  %16 = load i64, ptr %5, align 8
  %17 = load i64, ptr %5, align 8
  %18 = getelementptr inbounds nuw %struct.s24, ptr %10, i64 %17
  %19 = getelementptr inbounds nuw %struct.s24, ptr %18, i32 0, i32 0
  store i64 %16, ptr %19, align 8
  %20 = load i64, ptr %5, align 8
  %21 = mul nsw i64 %20, 2
  %22 = load i64, ptr %5, align 8
  %23 = getelementptr inbounds nuw %struct.s24, ptr %10, i64 %22
  %24 = getelementptr inbounds nuw %struct.s24, ptr %23, i32 0, i32 1
  store i64 %21, ptr %24, align 8
  %25 = load i64, ptr %5, align 8
  %26 = getelementptr inbounds nuw %struct.s24, ptr %10, i64 %25
  %27 = getelementptr inbounds nuw %struct.s24, ptr %26, i32 0, i32 2
  store i64 7, ptr %27, align 8
  br label %28

28:                                               ; preds = %15
  %29 = load i64, ptr %5, align 8
  %30 = add i64 %29, 1
  store i64 %30, ptr %5, align 8
  br label %11, !llvm.loop !12

31:                                               ; preds = %11
  store i64 0, ptr %6, align 8
  store i64 0, ptr %7, align 8
  br label %32

32:                                               ; preds = %53, %31
  %33 = load i64, ptr %7, align 8
  %34 = load i64, ptr %2, align 8
  %35 = icmp ult i64 %33, %34
  br i1 %35, label %36, label %56

36:                                               ; preds = %32
  %37 = load i64, ptr %7, align 8
  %38 = getelementptr inbounds nuw %struct.s24, ptr %10, i64 %37
  %39 = getelementptr inbounds nuw %struct.s24, ptr %38, i32 0, i32 0
  %40 = load i64, ptr %39, align 8
  %41 = load i64, ptr %7, align 8
  %42 = getelementptr inbounds nuw %struct.s24, ptr %10, i64 %41
  %43 = getelementptr inbounds nuw %struct.s24, ptr %42, i32 0, i32 1
  %44 = load i64, ptr %43, align 8
  %45 = add nsw i64 %40, %44
  %46 = load i64, ptr %7, align 8
  %47 = getelementptr inbounds nuw %struct.s24, ptr %10, i64 %46
  %48 = getelementptr inbounds nuw %struct.s24, ptr %47, i32 0, i32 2
  %49 = load i64, ptr %48, align 8
  %50 = add nsw i64 %45, %49
  %51 = load i64, ptr %6, align 8
  %52 = add i64 %51, %50
  store i64 %52, ptr %6, align 8
  br label %53

53:                                               ; preds = %36
  %54 = load i64, ptr %7, align 8
  %55 = add i64 %54, 1
  store i64 %55, ptr %7, align 8
  br label %32, !llvm.loop !13

56:                                               ; preds = %32
  %57 = load i64, ptr %6, align 8
  %58 = load ptr, ptr %3, align 8
  call void @llvm.stackrestore.p0(ptr %58)
  ret i64 %57
}

; Function Attrs: noinline nounwind optnone uwtable
define dso_local i64 @call_aligned(i64 noundef %0) #0 {
  %2 = alloca i64, align 8
  store i64 %0, ptr %2, align 8
  %3 = load i64, ptr %2, align 8
  %4 = call i64 @sum_aligned(i64 noundef %3)
  ret i64 %4
}

; Function Attrs: alwaysinline nounwind uwtable
define internal i64 @sum_aligned(i64 noundef %0) #1 {
  %2 = alloca i64, align 8
  %3 = alloca i64, align 8
  %4 = alloca ptr, align 8
  %5 = alloca i64, align 8
  %6 = alloca i32, align 4
  %7 = alloca i64, align 8
  %8 = alloca i64, align 8
  %9 = alloca i64, align 8
  store i64 %0, ptr %3, align 8
  %10 = load i64, ptr %3, align 8
  %11 = call ptr @llvm.stacksave.p0()
  store ptr %11, ptr %4, align 8
  %12 = alloca %struct.a64, i64 %10, align 64
  store i64 %10, ptr %5, align 8
  %13 = load i64, ptr %3, align 8
  %14 = icmp ne i64 %13, 0
  br i1 %14, label %15, label %25

15:                                               ; preds = %1
  %16 = getelementptr inbounds %struct.a64, ptr %12, i64 0
  %17 = ptrtoint ptr %16 to i64
  %18 = and i64 %17, 63
  %19 = icmp ne i64 %18, 0
  br i1 %19, label %20, label %25

20:                                               ; preds = %15
  %21 = getelementptr inbounds %struct.a64, ptr %12, i64 0
  %22 = ptrtoint ptr %21 to i64
  %23 = and i64 %22, 63
  %24 = add i64 1000000, %23
  store i64 %24, ptr %2, align 8
  store i32 1, ptr %6, align 4
  br label %57

25:                                               ; preds = %15, %1
  store i64 0, ptr %7, align 8
  br label %26

26:                                               ; preds = %36, %25
  %27 = load i64, ptr %7, align 8
  %28 = load i64, ptr %3, align 8
  %29 = icmp ult i64 %27, %28
  br i1 %29, label %30, label %39

30:                                               ; preds = %26
  %31 = load i64, ptr %7, align 8
  %32 = trunc i64 %31 to i8
  %33 = load i64, ptr %7, align 8
  %34 = getelementptr inbounds nuw %struct.a64, ptr %12, i64 %33
  %35 = getelementptr inbounds nuw %struct.a64, ptr %34, i32 0, i32 0
  store i8 %32, ptr %35, align 64
  br label %36

36:                                               ; preds = %30
  %37 = load i64, ptr %7, align 8
  %38 = add i64 %37, 1
  store i64 %38, ptr %7, align 8
  br label %26, !llvm.loop !14

39:                                               ; preds = %26
  store i64 0, ptr %8, align 8
  store i64 0, ptr %9, align 8
  br label %40

40:                                               ; preds = %52, %39
  %41 = load i64, ptr %9, align 8
  %42 = load i64, ptr %3, align 8
  %43 = icmp ult i64 %41, %42
  br i1 %43, label %44, label %55

44:                                               ; preds = %40
  %45 = load i64, ptr %9, align 8
  %46 = getelementptr inbounds nuw %struct.a64, ptr %12, i64 %45
  %47 = getelementptr inbounds nuw %struct.a64, ptr %46, i32 0, i32 0
  %48 = load i8, ptr %47, align 64
  %49 = zext i8 %48 to i64
  %50 = load i64, ptr %8, align 8
  %51 = add i64 %50, %49
  store i64 %51, ptr %8, align 8
  br label %52

52:                                               ; preds = %44
  %53 = load i64, ptr %9, align 8
  %54 = add i64 %53, 1
  store i64 %54, ptr %9, align 8
  br label %40, !llvm.loop !15

55:                                               ; preds = %40
  %56 = load i64, ptr %8, align 8
  store i64 %56, ptr %2, align 8
  store i32 1, ptr %6, align 4
  br label %57

57:                                               ; preds = %55, %20
  %58 = load ptr, ptr %4, align 8
  call void @llvm.stackrestore.p0(ptr %58)
  %59 = load i64, ptr %2, align 8
  ret i64 %59
}

; Function Attrs: noinline nounwind optnone uwtable
define dso_local i64 @call_two(i64 noundef %0, i64 noundef %1) #0 {
  %3 = alloca i64, align 8
  %4 = alloca i64, align 8
  store i64 %0, ptr %3, align 8
  store i64 %1, ptr %4, align 8
  %5 = load i64, ptr %3, align 8
  %6 = load i64, ptr %4, align 8
  %7 = call i64 @two_vlas(i64 noundef %5, i64 noundef %6)
  ret i64 %7
}

; Function Attrs: alwaysinline nounwind uwtable
define internal i64 @two_vlas(i64 noundef %0, i64 noundef %1) #1 {
  %3 = alloca i64, align 8
  %4 = alloca i64, align 8
  %5 = alloca ptr, align 8
  %6 = alloca i64, align 8
  %7 = alloca i64, align 8
  %8 = alloca i64, align 8
  %9 = alloca i64, align 8
  %10 = alloca i64, align 8
  %11 = alloca i64, align 8
  %12 = alloca i64, align 8
  store i64 %0, ptr %3, align 8
  store i64 %1, ptr %4, align 8
  %13 = load i64, ptr %3, align 8
  %14 = call ptr @llvm.stacksave.p0()
  store ptr %14, ptr %5, align 8
  %15 = alloca i32, i64 %13, align 4
  store i64 %13, ptr %6, align 8
  %16 = load i64, ptr %4, align 8
  %17 = alloca i64, i64 %16, align 8
  store i64 %16, ptr %7, align 8
  store i64 0, ptr %8, align 8
  br label %18

18:                                               ; preds = %27, %2
  %19 = load i64, ptr %8, align 8
  %20 = load i64, ptr %3, align 8
  %21 = icmp ult i64 %19, %20
  br i1 %21, label %22, label %30

22:                                               ; preds = %18
  %23 = load i64, ptr %8, align 8
  %24 = trunc i64 %23 to i32
  %25 = load i64, ptr %8, align 8
  %26 = getelementptr inbounds nuw i32, ptr %15, i64 %25
  store i32 %24, ptr %26, align 4
  br label %27

27:                                               ; preds = %22
  %28 = load i64, ptr %8, align 8
  %29 = add i64 %28, 1
  store i64 %29, ptr %8, align 8
  br label %18, !llvm.loop !16

30:                                               ; preds = %18
  store i64 0, ptr %9, align 8
  br label %31

31:                                               ; preds = %40, %30
  %32 = load i64, ptr %9, align 8
  %33 = load i64, ptr %4, align 8
  %34 = icmp ult i64 %32, %33
  br i1 %34, label %35, label %43

35:                                               ; preds = %31
  %36 = load i64, ptr %9, align 8
  %37 = mul nsw i64 %36, 10
  %38 = load i64, ptr %9, align 8
  %39 = getelementptr inbounds nuw i64, ptr %17, i64 %38
  store i64 %37, ptr %39, align 8
  br label %40

40:                                               ; preds = %35
  %41 = load i64, ptr %9, align 8
  %42 = add i64 %41, 1
  store i64 %42, ptr %9, align 8
  br label %31, !llvm.loop !17

43:                                               ; preds = %31
  store i64 0, ptr %10, align 8
  store i64 0, ptr %11, align 8
  br label %44

44:                                               ; preds = %55, %43
  %45 = load i64, ptr %11, align 8
  %46 = load i64, ptr %3, align 8
  %47 = icmp ult i64 %45, %46
  br i1 %47, label %48, label %58

48:                                               ; preds = %44
  %49 = load i64, ptr %11, align 8
  %50 = getelementptr inbounds nuw i32, ptr %15, i64 %49
  %51 = load i32, ptr %50, align 4
  %52 = sext i32 %51 to i64
  %53 = load i64, ptr %10, align 8
  %54 = add i64 %53, %52
  store i64 %54, ptr %10, align 8
  br label %55

55:                                               ; preds = %48
  %56 = load i64, ptr %11, align 8
  %57 = add i64 %56, 1
  store i64 %57, ptr %11, align 8
  br label %44, !llvm.loop !18

58:                                               ; preds = %44
  store i64 0, ptr %12, align 8
  br label %59

59:                                               ; preds = %69, %58
  %60 = load i64, ptr %12, align 8
  %61 = load i64, ptr %4, align 8
  %62 = icmp ult i64 %60, %61
  br i1 %62, label %63, label %72

63:                                               ; preds = %59
  %64 = load i64, ptr %12, align 8
  %65 = getelementptr inbounds nuw i64, ptr %17, i64 %64
  %66 = load i64, ptr %65, align 8
  %67 = load i64, ptr %10, align 8
  %68 = add i64 %67, %66
  store i64 %68, ptr %10, align 8
  br label %69

69:                                               ; preds = %63
  %70 = load i64, ptr %12, align 8
  %71 = add i64 %70, 1
  store i64 %71, ptr %12, align 8
  br label %59, !llvm.loop !19

72:                                               ; preds = %59
  %73 = load i64, ptr %10, align 8
  %74 = load ptr, ptr %5, align 8
  call void @llvm.stackrestore.p0(ptr %74)
  ret i64 %73
}

; Function Attrs: noinline nounwind optnone uwtable
define dso_local i64 @call_twice(i64 noundef %0) #0 {
  %2 = alloca i64, align 8
  store i64 %0, ptr %2, align 8
  %3 = load i64, ptr %2, align 8
  %4 = call i64 @sum_ints(i64 noundef %3)
  %5 = load i64, ptr %2, align 8
  %6 = add i64 %5, 5
  %7 = call i64 @sum_ints(i64 noundef %6)
  %8 = add i64 %4, %7
  ret i64 %8
}

; Function Attrs: noinline nounwind optnone uwtable
define dso_local i64 @call_in_loop(i64 noundef %0, i64 noundef %1) #0 {
  %3 = alloca i64, align 8
  %4 = alloca i64, align 8
  %5 = alloca i64, align 8
  %6 = alloca i64, align 8
  %7 = alloca i64, align 8
  store i64 %0, ptr %3, align 8
  store i64 %1, ptr %4, align 8
  %8 = load i64, ptr %4, align 8
  %9 = call i64 @vla_address(i64 noundef %8)
  store i64 %9, ptr %5, align 8
  store i64 0, ptr %6, align 8
  store i64 0, ptr %7, align 8
  br label %10

10:                                               ; preds = %23, %2
  %11 = load i64, ptr %7, align 8
  %12 = load i64, ptr %3, align 8
  %13 = icmp ult i64 %11, %12
  br i1 %13, label %14, label %26

14:                                               ; preds = %10
  %15 = load i64, ptr %4, align 8
  %16 = call i64 @vla_address(i64 noundef %15)
  %17 = load i64, ptr %5, align 8
  %18 = icmp ne i64 %16, %17
  br i1 %18, label %19, label %22

19:                                               ; preds = %14
  %20 = load i64, ptr %6, align 8
  %21 = add i64 %20, 1
  store i64 %21, ptr %6, align 8
  br label %22

22:                                               ; preds = %19, %14
  br label %23

23:                                               ; preds = %22
  %24 = load i64, ptr %7, align 8
  %25 = add i64 %24, 1
  store i64 %25, ptr %7, align 8
  br label %10, !llvm.loop !20

26:                                               ; preds = %10
  %27 = load i64, ptr %6, align 8
  ret i64 %27
}

; Function Attrs: alwaysinline nounwind uwtable
define internal i64 @vla_address(i64 noundef %0) #1 {
  %2 = alloca i64, align 8
  %3 = alloca ptr, align 8
  %4 = alloca i64, align 8
  store i64 %0, ptr %2, align 8
  %5 = load i64, ptr %2, align 8
  %6 = call ptr @llvm.stacksave.p0()
  store ptr %6, ptr %3, align 8
  %7 = alloca i64, i64 %5, align 8
  store i64 %5, ptr %4, align 8
  %8 = getelementptr inbounds i64, ptr %7, i64 0
  store i64 1, ptr %8, align 8
  %9 = load i64, ptr %2, align 8
  %10 = icmp ugt i64 %9, 1
  br i1 %10, label %11, label %15

11:                                               ; preds = %1
  %12 = load i64, ptr %2, align 8
  %13 = sub i64 %12, 1
  %14 = getelementptr inbounds nuw i64, ptr %7, i64 %13
  store i64 2, ptr %14, align 8
  br label %15

15:                                               ; preds = %11, %1
  %16 = getelementptr inbounds i64, ptr %7, i64 0
  %17 = ptrtoint ptr %16 to i64
  %18 = getelementptr inbounds i64, ptr %7, i64 0
  %19 = load i64, ptr %18, align 8
  %20 = sub nsw i64 %19, 1
  %21 = add i64 %17, %20
  %22 = load ptr, ptr %3, align 8
  call void @llvm.stackrestore.p0(ptr %22)
  ret i64 %21
}

; Function Attrs: noinline nounwind optnone uwtable
define dso_local i64 @call_early(i64 noundef %0, i64 noundef %1) #0 {
  %3 = alloca i64, align 8
  %4 = alloca i64, align 8
  store i64 %0, ptr %3, align 8
  store i64 %1, ptr %4, align 8
  %5 = load i64, ptr %3, align 8
  %6 = load i64, ptr %4, align 8
  %7 = call i64 @early(i64 noundef %5, i64 noundef %6)
  ret i64 %7
}

; Function Attrs: alwaysinline nounwind uwtable
define internal i64 @early(i64 noundef %0, i64 noundef %1) #1 {
  %3 = alloca i64, align 8
  %4 = alloca i64, align 8
  %5 = alloca i64, align 8
  %6 = alloca ptr, align 8
  %7 = alloca i64, align 8
  %8 = alloca i64, align 8
  %9 = alloca i32, align 4
  %10 = alloca i64, align 8
  %11 = alloca i64, align 8
  store i64 %0, ptr %4, align 8
  store i64 %1, ptr %5, align 8
  %12 = load i64, ptr %4, align 8
  %13 = call ptr @llvm.stacksave.p0()
  store ptr %13, ptr %6, align 8
  %14 = alloca i32, i64 %12, align 4
  store i64 %12, ptr %7, align 8
  store i64 0, ptr %8, align 8
  br label %15

15:                                               ; preds = %34, %2
  %16 = load i64, ptr %8, align 8
  %17 = load i64, ptr %4, align 8
  %18 = icmp ult i64 %16, %17
  br i1 %18, label %19, label %37

19:                                               ; preds = %15
  %20 = load i64, ptr %8, align 8
  %21 = trunc i64 %20 to i32
  %22 = load i64, ptr %8, align 8
  %23 = getelementptr inbounds nuw i32, ptr %14, i64 %22
  store i32 %21, ptr %23, align 4
  %24 = load i64, ptr %8, align 8
  %25 = load i64, ptr %5, align 8
  %26 = icmp eq i64 %24, %25
  br i1 %26, label %27, label %33

27:                                               ; preds = %19
  %28 = load i64, ptr %8, align 8
  %29 = getelementptr inbounds nuw i32, ptr %14, i64 %28
  %30 = load i32, ptr %29, align 4
  %31 = sext i32 %30 to i64
  %32 = add i64 9000, %31
  store i64 %32, ptr %3, align 8
  store i32 1, ptr %9, align 4
  br label %54

33:                                               ; preds = %19
  br label %34

34:                                               ; preds = %33
  %35 = load i64, ptr %8, align 8
  %36 = add i64 %35, 1
  store i64 %36, ptr %8, align 8
  br label %15, !llvm.loop !21

37:                                               ; preds = %15
  store i64 0, ptr %10, align 8
  store i64 0, ptr %11, align 8
  br label %38

38:                                               ; preds = %49, %37
  %39 = load i64, ptr %11, align 8
  %40 = load i64, ptr %4, align 8
  %41 = icmp ult i64 %39, %40
  br i1 %41, label %42, label %52

42:                                               ; preds = %38
  %43 = load i64, ptr %11, align 8
  %44 = getelementptr inbounds nuw i32, ptr %14, i64 %43
  %45 = load i32, ptr %44, align 4
  %46 = sext i32 %45 to i64
  %47 = load i64, ptr %10, align 8
  %48 = add i64 %47, %46
  store i64 %48, ptr %10, align 8
  br label %49

49:                                               ; preds = %42
  %50 = load i64, ptr %11, align 8
  %51 = add i64 %50, 1
  store i64 %51, ptr %11, align 8
  br label %38, !llvm.loop !22

52:                                               ; preds = %38
  %53 = load i64, ptr %10, align 8
  store i64 %53, ptr %3, align 8
  store i32 1, ptr %9, align 4
  br label %54

54:                                               ; preds = %52, %27
  %55 = load ptr, ptr %6, align 8
  call void @llvm.stackrestore.p0(ptr %55)
  %56 = load i64, ptr %3, align 8
  ret i64 %56
}

; Function Attrs: noinline nounwind optnone uwtable
define dso_local i64 @call_scoped(i64 noundef %0) #0 {
  %2 = alloca i64, align 8
  store i64 %0, ptr %2, align 8
  %3 = load i64, ptr %2, align 8
  %4 = call i64 @scoped(i64 noundef %3)
  ret i64 %4
}

; Function Attrs: alwaysinline nounwind uwtable
define internal i64 @scoped(i64 noundef %0) #1 {
  %2 = alloca i64, align 8
  %3 = alloca i64, align 8
  %4 = alloca i64, align 8
  %5 = alloca ptr, align 8
  %6 = alloca i64, align 8
  %7 = alloca i64, align 8
  %8 = alloca i64, align 8
  store i64 %0, ptr %2, align 8
  store i64 0, ptr %3, align 8
  store i64 1, ptr %4, align 8
  br label %9

9:                                                ; preds = %51, %1
  %10 = load i64, ptr %4, align 8
  %11 = icmp ule i64 %10, 4
  br i1 %11, label %12, label %54

12:                                               ; preds = %9
  %13 = load i64, ptr %2, align 8
  %14 = load i64, ptr %4, align 8
  %15 = mul i64 %13, %14
  %16 = call ptr @llvm.stacksave.p0()
  store ptr %16, ptr %5, align 8
  %17 = alloca i16, i64 %15, align 2
  store i64 %15, ptr %6, align 8
  store i64 0, ptr %7, align 8
  br label %18

18:                                               ; preds = %29, %12
  %19 = load i64, ptr %7, align 8
  %20 = load i64, ptr %2, align 8
  %21 = load i64, ptr %4, align 8
  %22 = mul i64 %20, %21
  %23 = icmp ult i64 %19, %22
  br i1 %23, label %24, label %32

24:                                               ; preds = %18
  %25 = load i64, ptr %7, align 8
  %26 = trunc i64 %25 to i16
  %27 = load i64, ptr %7, align 8
  %28 = getelementptr inbounds nuw i16, ptr %17, i64 %27
  store i16 %26, ptr %28, align 2
  br label %29

29:                                               ; preds = %24
  %30 = load i64, ptr %7, align 8
  %31 = add i64 %30, 1
  store i64 %31, ptr %7, align 8
  br label %18, !llvm.loop !23

32:                                               ; preds = %18
  store i64 0, ptr %8, align 8
  br label %33

33:                                               ; preds = %46, %32
  %34 = load i64, ptr %8, align 8
  %35 = load i64, ptr %2, align 8
  %36 = load i64, ptr %4, align 8
  %37 = mul i64 %35, %36
  %38 = icmp ult i64 %34, %37
  br i1 %38, label %39, label %49

39:                                               ; preds = %33
  %40 = load i64, ptr %8, align 8
  %41 = getelementptr inbounds nuw i16, ptr %17, i64 %40
  %42 = load i16, ptr %41, align 2
  %43 = sext i16 %42 to i64
  %44 = load i64, ptr %3, align 8
  %45 = add i64 %44, %43
  store i64 %45, ptr %3, align 8
  br label %46

46:                                               ; preds = %39
  %47 = load i64, ptr %8, align 8
  %48 = add i64 %47, 1
  store i64 %48, ptr %8, align 8
  br label %33, !llvm.loop !24

49:                                               ; preds = %33
  %50 = load ptr, ptr %5, align 8
  call void @llvm.stackrestore.p0(ptr %50)
  br label %51

51:                                               ; preds = %49
  %52 = load i64, ptr %4, align 8
  %53 = add i64 %52, 1
  store i64 %53, ptr %4, align 8
  br label %9, !llvm.loop !25

54:                                               ; preds = %9
  %55 = load i64, ptr %3, align 8
  ret i64 %55
}

; Function Attrs: noinline nounwind optnone uwtable
define dso_local i64 @call_nested(i64 noundef %0) #0 {
  %2 = alloca i64, align 8
  store i64 %0, ptr %2, align 8
  %3 = load i64, ptr %2, align 8
  %4 = call i64 @outer(i64 noundef %3)
  ret i64 %4
}

; Function Attrs: alwaysinline nounwind uwtable
define internal i64 @outer(i64 noundef %0) #1 {
  %2 = alloca i64, align 8
  %3 = alloca ptr, align 8
  %4 = alloca i64, align 8
  %5 = alloca i64, align 8
  %6 = alloca i64, align 8
  %7 = alloca i64, align 8
  store i64 %0, ptr %2, align 8
  %8 = load i64, ptr %2, align 8
  %9 = add i64 %8, 3
  %10 = call ptr @llvm.stacksave.p0()
  store ptr %10, ptr %3, align 8
  %11 = alloca i8, i64 %9, align 1
  store i64 %9, ptr %4, align 8
  store i64 0, ptr %5, align 8
  br label %12

12:                                               ; preds = %20, %1
  %13 = load i64, ptr %5, align 8
  %14 = load i64, ptr %2, align 8
  %15 = add i64 %14, 3
  %16 = icmp ult i64 %13, %15
  br i1 %16, label %17, label %23

17:                                               ; preds = %12
  %18 = load i64, ptr %5, align 8
  %19 = getelementptr inbounds nuw i8, ptr %11, i64 %18
  store i8 1, ptr %19, align 1
  br label %20

20:                                               ; preds = %17
  %21 = load i64, ptr %5, align 8
  %22 = add i64 %21, 1
  store i64 %22, ptr %5, align 8
  br label %12, !llvm.loop !26

23:                                               ; preds = %12
  %24 = load i64, ptr %2, align 8
  %25 = call i64 @inner(i64 noundef %24)
  %26 = load i64, ptr %2, align 8
  %27 = add i64 %26, 1
  %28 = call i64 @inner(i64 noundef %27)
  %29 = add i64 %25, %28
  store i64 %29, ptr %6, align 8
  store i64 0, ptr %7, align 8
  br label %30

30:                                               ; preds = %42, %23
  %31 = load i64, ptr %7, align 8
  %32 = load i64, ptr %2, align 8
  %33 = add i64 %32, 3
  %34 = icmp ult i64 %31, %33
  br i1 %34, label %35, label %45

35:                                               ; preds = %30
  %36 = load i64, ptr %7, align 8
  %37 = getelementptr inbounds nuw i8, ptr %11, i64 %36
  %38 = load i8, ptr %37, align 1
  %39 = zext i8 %38 to i64
  %40 = load i64, ptr %6, align 8
  %41 = add i64 %40, %39
  store i64 %41, ptr %6, align 8
  br label %42

42:                                               ; preds = %35
  %43 = load i64, ptr %7, align 8
  %44 = add i64 %43, 1
  store i64 %44, ptr %7, align 8
  br label %30, !llvm.loop !27

45:                                               ; preds = %30
  %46 = load i64, ptr %6, align 8
  %47 = load ptr, ptr %3, align 8
  call void @llvm.stackrestore.p0(ptr %47)
  ret i64 %46
}

; Function Attrs: noinline nounwind optnone uwtable
define dso_local i64 @call_with_own(i64 noundef %0) #0 {
  %2 = alloca i64, align 8
  %3 = alloca ptr, align 8
  %4 = alloca i64, align 8
  %5 = alloca i64, align 8
  %6 = alloca i64, align 8
  %7 = alloca i64, align 8
  store i64 %0, ptr %2, align 8
  %8 = load i64, ptr %2, align 8
  %9 = add i64 %8, 1
  %10 = call ptr @llvm.stacksave.p0()
  store ptr %10, ptr %3, align 8
  %11 = alloca i32, i64 %9, align 4
  store i64 %9, ptr %4, align 8
  store i64 0, ptr %5, align 8
  br label %12

12:                                               ; preds = %22, %1
  %13 = load i64, ptr %5, align 8
  %14 = load i64, ptr %2, align 8
  %15 = icmp ule i64 %13, %14
  br i1 %15, label %16, label %25

16:                                               ; preds = %12
  %17 = load i64, ptr %5, align 8
  %18 = add i64 %17, 100
  %19 = trunc i64 %18 to i32
  %20 = load i64, ptr %5, align 8
  %21 = getelementptr inbounds nuw i32, ptr %11, i64 %20
  store i32 %19, ptr %21, align 4
  br label %22

22:                                               ; preds = %16
  %23 = load i64, ptr %5, align 8
  %24 = add i64 %23, 1
  store i64 %24, ptr %5, align 8
  br label %12, !llvm.loop !28

25:                                               ; preds = %12
  %26 = load i64, ptr %2, align 8
  %27 = call i64 @sum_s24(i64 noundef %26)
  %28 = load i64, ptr %2, align 8
  %29 = load i64, ptr %2, align 8
  %30 = add i64 %29, 2
  %31 = call i64 @two_vlas(i64 noundef %28, i64 noundef %30)
  %32 = add i64 %27, %31
  store i64 %32, ptr %6, align 8
  store i64 0, ptr %7, align 8
  br label %33

33:                                               ; preds = %44, %25
  %34 = load i64, ptr %7, align 8
  %35 = load i64, ptr %2, align 8
  %36 = icmp ule i64 %34, %35
  br i1 %36, label %37, label %47

37:                                               ; preds = %33
  %38 = load i64, ptr %7, align 8
  %39 = getelementptr inbounds nuw i32, ptr %11, i64 %38
  %40 = load i32, ptr %39, align 4
  %41 = sext i32 %40 to i64
  %42 = load i64, ptr %6, align 8
  %43 = add i64 %42, %41
  store i64 %43, ptr %6, align 8
  br label %44

44:                                               ; preds = %37
  %45 = load i64, ptr %7, align 8
  %46 = add i64 %45, 1
  store i64 %46, ptr %7, align 8
  br label %33, !llvm.loop !29

47:                                               ; preds = %33
  %48 = load i64, ptr %6, align 8
  %49 = load ptr, ptr %3, align 8
  call void @llvm.stackrestore.p0(ptr %49)
  ret i64 %48
}

; Function Attrs: nocallback nofree nosync nounwind willreturn
declare ptr @llvm.stacksave.p0() #2

; Function Attrs: nocallback nofree nosync nounwind willreturn
declare void @llvm.stackrestore.p0(ptr) #2

; Function Attrs: noinline nounwind optnone uwtable
define dso_local i64 @bin_flush(ptr noundef %0, i32 noundef %1, ptr noundef %2, ptr noundef %3, i32 noundef %4) #0 {
  %6 = alloca ptr, align 8
  %7 = alloca i32, align 4
  %8 = alloca ptr, align 8
  %9 = alloca ptr, align 8
  %10 = alloca i32, align 4
  %11 = alloca i64, align 8
  store ptr %0, ptr %6, align 8
  store i32 %1, ptr %7, align 4
  store ptr %2, ptr %8, align 8
  store ptr %3, ptr %9, align 8
  store i32 %4, ptr %10, align 4
  store i64 0, ptr %11, align 8
  %12 = load ptr, ptr %6, align 8
  %13 = load i32, ptr %7, align 4
  %14 = load ptr, ptr %8, align 8
  %15 = load ptr, ptr %9, align 8
  %16 = load i32, ptr %10, align 4
  call void @flush_impl(ptr noundef %12, i32 noundef %13, ptr noundef %14, ptr noundef %15, ptr noundef %11, i32 noundef %16)
  %17 = load i64, ptr %11, align 8
  ret i64 %17
}

; Function Attrs: alwaysinline nounwind uwtable
define internal void @flush_impl(ptr noundef %0, i32 noundef %1, ptr noundef %2, ptr noundef %3, ptr noundef %4, i32 noundef %5) #1 {
  %7 = alloca ptr, align 8
  %8 = alloca i32, align 4
  %9 = alloca ptr, align 8
  %10 = alloca ptr, align 8
  %11 = alloca ptr, align 8
  %12 = alloca i32, align 4
  store ptr %0, ptr %7, align 8
  store i32 %1, ptr %8, align 4
  store ptr %2, ptr %9, align 8
  store ptr %3, ptr %10, align 8
  store ptr %4, ptr %11, align 8
  store i32 %5, ptr %12, align 4
  %13 = load i32, ptr %12, align 4
  %14 = icmp ne i32 %13, 0
  br i1 %14, label %15, label %21

15:                                               ; preds = %6
  %16 = load ptr, ptr %7, align 8
  %17 = load i32, ptr %8, align 4
  %18 = load ptr, ptr %9, align 8
  %19 = load ptr, ptr %10, align 8
  %20 = load ptr, ptr %11, align 8
  call void @flush_small(ptr noundef %16, i32 noundef %17, ptr noundef %18, ptr noundef %19, ptr noundef %20)
  br label %25

21:                                               ; preds = %6
  %22 = load ptr, ptr %11, align 8
  %23 = load i64, ptr %22, align 8
  %24 = add i64 %23, 77
  store i64 %24, ptr %22, align 8
  br label %25

25:                                               ; preds = %21, %15
  ret void
}

; Function Attrs: alwaysinline nounwind uwtable
define internal i64 @inner(i64 noundef %0) #1 {
  %2 = alloca i64, align 8
  %3 = alloca ptr, align 8
  %4 = alloca i64, align 8
  %5 = alloca i64, align 8
  %6 = alloca i64, align 8
  %7 = alloca i64, align 8
  store i64 %0, ptr %2, align 8
  %8 = load i64, ptr %2, align 8
  %9 = call ptr @llvm.stacksave.p0()
  store ptr %9, ptr %3, align 8
  %10 = alloca i64, i64 %8, align 8
  store i64 %8, ptr %4, align 8
  store i64 0, ptr %5, align 8
  br label %11

11:                                               ; preds = %20, %1
  %12 = load i64, ptr %5, align 8
  %13 = load i64, ptr %2, align 8
  %14 = icmp ult i64 %12, %13
  br i1 %14, label %15, label %23

15:                                               ; preds = %11
  %16 = load i64, ptr %5, align 8
  %17 = add i64 %16, 1
  %18 = load i64, ptr %5, align 8
  %19 = getelementptr inbounds nuw i64, ptr %10, i64 %18
  store i64 %17, ptr %19, align 8
  br label %20

20:                                               ; preds = %15
  %21 = load i64, ptr %5, align 8
  %22 = add i64 %21, 1
  store i64 %22, ptr %5, align 8
  br label %11, !llvm.loop !30

23:                                               ; preds = %11
  store i64 0, ptr %6, align 8
  store i64 0, ptr %7, align 8
  br label %24

24:                                               ; preds = %34, %23
  %25 = load i64, ptr %7, align 8
  %26 = load i64, ptr %2, align 8
  %27 = icmp ult i64 %25, %26
  br i1 %27, label %28, label %37

28:                                               ; preds = %24
  %29 = load i64, ptr %7, align 8
  %30 = getelementptr inbounds nuw i64, ptr %10, i64 %29
  %31 = load i64, ptr %30, align 8
  %32 = load i64, ptr %6, align 8
  %33 = add i64 %32, %31
  store i64 %33, ptr %6, align 8
  br label %34

34:                                               ; preds = %28
  %35 = load i64, ptr %7, align 8
  %36 = add i64 %35, 1
  store i64 %36, ptr %7, align 8
  br label %24, !llvm.loop !31

37:                                               ; preds = %24
  %38 = load i64, ptr %6, align 8
  %39 = load ptr, ptr %3, align 8
  call void @llvm.stackrestore.p0(ptr %39)
  ret i64 %38
}

; Function Attrs: alwaysinline nounwind uwtable
define internal void @flush_small(ptr noundef %0, i32 noundef %1, ptr noundef %2, ptr noundef %3, ptr noundef %4) #1 {
  %6 = alloca ptr, align 8
  %7 = alloca i32, align 4
  %8 = alloca ptr, align 8
  %9 = alloca ptr, align 8
  %10 = alloca ptr, align 8
  %11 = alloca ptr, align 8
  %12 = alloca i64, align 8
  %13 = alloca i32, align 4
  %14 = alloca i32, align 4
  %15 = alloca i32, align 4
  store ptr %0, ptr %6, align 8
  store i32 %1, ptr %7, align 4
  store ptr %2, ptr %8, align 8
  store ptr %3, ptr %9, align 8
  store ptr %4, ptr %10, align 8
  %16 = load i32, ptr %7, align 4
  %17 = add i32 %16, 1
  %18 = zext i32 %17 to i64
  %19 = call ptr @llvm.stacksave.p0()
  store ptr %19, ptr %11, align 8
  %20 = alloca %union.lookup_t, i64 %18, align 8
  store i64 %18, ptr %12, align 8
  store i32 0, ptr %13, align 4
  br label %21

21:                                               ; preds = %33, %5
  %22 = load i32, ptr %13, align 4
  %23 = load i32, ptr %7, align 4
  %24 = icmp ult i32 %22, %23
  br i1 %24, label %25, label %36

25:                                               ; preds = %21
  %26 = load ptr, ptr %6, align 8
  %27 = load i32, ptr %13, align 4
  %28 = zext i32 %27 to i64
  %29 = getelementptr inbounds nuw i64, ptr %26, i64 %28
  %30 = load i32, ptr %13, align 4
  %31 = zext i32 %30 to i64
  %32 = getelementptr inbounds nuw %union.lookup_t, ptr %20, i64 %31
  store ptr %29, ptr %32, align 8
  br label %33

33:                                               ; preds = %25
  %34 = load i32, ptr %13, align 4
  %35 = add i32 %34, 1
  store i32 %35, ptr %13, align 4
  br label %21, !llvm.loop !32

36:                                               ; preds = %21
  %37 = load i32, ptr %7, align 4
  %38 = zext i32 %37 to i64
  %39 = getelementptr inbounds nuw %union.lookup_t, ptr %20, i64 %38
  store ptr null, ptr %39, align 8
  %40 = load i32, ptr %7, align 4
  store i32 %40, ptr %14, align 4
  br label %41

41:                                               ; preds = %74, %36
  %42 = load i32, ptr %14, align 4
  %43 = icmp ugt i32 %42, 0
  br i1 %43, label %44, label %77

44:                                               ; preds = %41
  %45 = load ptr, ptr %8, align 8
  %46 = load ptr, ptr %6, align 8
  call void %45(ptr noundef %46)
  store i32 0, ptr %15, align 4
  br label %47

47:                                               ; preds = %65, %44
  %48 = load i32, ptr %15, align 4
  %49 = load i32, ptr %14, align 4
  %50 = icmp ult i32 %48, %49
  br i1 %50, label %51, label %68

51:                                               ; preds = %47
  %52 = load i32, ptr %15, align 4
  %53 = zext i32 %52 to i64
  %54 = getelementptr inbounds nuw %union.lookup_t, ptr %20, i64 %53
  %55 = load ptr, ptr %54, align 8
  %56 = load ptr, ptr %6, align 8
  %57 = ptrtoint ptr %55 to i64
  %58 = ptrtoint ptr %56 to i64
  %59 = sub i64 %57, %58
  %60 = sdiv exact i64 %59, 8
  %61 = add i64 %60, 1
  %62 = load ptr, ptr %10, align 8
  %63 = load i64, ptr %62, align 8
  %64 = add i64 %63, %61
  store i64 %64, ptr %62, align 8
  br label %65

65:                                               ; preds = %51
  %66 = load i32, ptr %15, align 4
  %67 = add i32 %66, 1
  store i32 %67, ptr %15, align 4
  br label %47, !llvm.loop !33

68:                                               ; preds = %47
  %69 = load ptr, ptr %9, align 8
  %70 = load ptr, ptr %6, align 8
  call void %69(ptr noundef %70)
  %71 = load i32, ptr %14, align 4
  %72 = icmp eq i32 %71, 1
  br i1 %72, label %73, label %74

73:                                               ; preds = %68
  br label %77

74:                                               ; preds = %68
  %75 = load i32, ptr %14, align 4
  %76 = udiv i32 %75, 2
  store i32 %76, ptr %14, align 4
  br label %41, !llvm.loop !34

77:                                               ; preds = %73, %41
  %78 = load ptr, ptr %11, align 8
  call void @llvm.stackrestore.p0(ptr %78)
  ret void
}

attributes #0 = { noinline nounwind optnone uwtable "frame-pointer"="all" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #1 = { alwaysinline nounwind uwtable "frame-pointer"="all" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #2 = { nocallback nofree nosync nounwind willreturn }

!llvm.module.flags = !{!0, !1, !2, !3, !4}

!0 = !{i32 1, !"wchar_size", i32 4}
!1 = !{i32 8, !"PIC Level", i32 2}
!2 = !{i32 7, !"PIE Level", i32 2}
!3 = !{i32 7, !"uwtable", i32 2}
!4 = !{i32 7, !"frame-pointer", i32 2}
!5 = distinct !{!5, !6}
!6 = !{!"llvm.loop.mustprogress"}
!7 = distinct !{!7, !6}
!8 = distinct !{!8, !6}
!9 = distinct !{!9, !6}
!10 = distinct !{!10, !6}
!11 = distinct !{!11, !6}
!12 = distinct !{!12, !6}
!13 = distinct !{!13, !6}
!14 = distinct !{!14, !6}
!15 = distinct !{!15, !6}
!16 = distinct !{!16, !6}
!17 = distinct !{!17, !6}
!18 = distinct !{!18, !6}
!19 = distinct !{!19, !6}
!20 = distinct !{!20, !6}
!21 = distinct !{!21, !6}
!22 = distinct !{!22, !6}
!23 = distinct !{!23, !6}
!24 = distinct !{!24, !6}
!25 = distinct !{!25, !6}
!26 = distinct !{!26, !6}
!27 = distinct !{!27, !6}
!28 = distinct !{!28, !6}
!29 = distinct !{!29, !6}
!30 = distinct !{!30, !6}
!31 = distinct !{!31, !6}
!32 = distinct !{!32, !6}
!33 = distinct !{!33, !6}
!34 = distinct !{!34, !6}
