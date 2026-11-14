; Frame-size seeds (WP-198, spec/codegen.md §5.1 "Value slots"): the
; shapes kiln writes at -O0 for a recursive-descent parser. A guard object
; with a destructor, a chain of `invoke`s whose `i1` results are tested in
; the next block, a cleanup landing pad that stores its pair and a `resume`
; that rebuilds it, a short-circuit `phi`, values of every scalar width
; used in other blocks, and values used twice in their own block.
; No target lines: each target's test puts its own in front.

%struct.State = type { ptr, ptr, i32, i32, [16 x i8] }
%struct.Guard = type { ptr }

declare void @guard_init(ptr noundef, ptr noundef)
declare zeroext i1 @guard_too_complex(ptr noundef)
declare void @guard_done(ptr noundef) nounwind
declare zeroext i1 @token(ptr noundef, i8 noundef signext)
declare zeroext i1 @optional(i1 noundef zeroext)
declare i32 @__gxx_personality_v0(...)
declare void @llvm.memcpy.p0.p0.i64(ptr, ptr, i64, i1)

define internal zeroext i1 @parse_nested(ptr noundef %0) noinline optnone personality ptr @__gxx_personality_v0 {
  %2 = alloca i1, align 1
  %3 = alloca ptr, align 8
  %4 = alloca %struct.Guard, align 8
  %5 = alloca i32, align 4
  %6 = alloca [16 x i8], align 4
  %7 = alloca ptr, align 8
  %8 = alloca i32, align 4
  store ptr %0, ptr %3, align 8
  %9 = load ptr, ptr %3, align 8
  call void @guard_init(ptr noundef %4, ptr noundef %9)
  %10 = call zeroext i1 @guard_too_complex(ptr noundef %4)
  br i1 %10, label %11, label %12

11:
  store i1 false, ptr %2, align 1
  store i32 1, ptr %5, align 4
  br label %40

12:
  %13 = load ptr, ptr %3, align 8
  %14 = getelementptr inbounds %struct.State, ptr %13, i32 0, i32 4
  call void @llvm.memcpy.p0.p0.i64(ptr align 4 %6, ptr align 4 %14, i64 16, i1 false)
  %15 = load ptr, ptr %3, align 8
  %16 = invoke zeroext i1 @token(ptr noundef %15, i8 noundef signext 78)
          to label %17 unwind label %33

17:
  br i1 %16, label %18, label %37

18:
  %19 = load ptr, ptr %3, align 8
  %20 = invoke zeroext i1 @parse_expression(ptr noundef %19)
          to label %21 unwind label %33

21:
  %22 = invoke zeroext i1 @optional(i1 noundef zeroext %20)
          to label %23 unwind label %33

23:
  br i1 %22, label %24, label %37

24:
  %25 = load ptr, ptr %3, align 8
  %26 = invoke zeroext i1 @parse_nested(ptr noundef %25)
          to label %27 unwind label %33

27:
  br i1 %26, label %28, label %37

28:
  %29 = load ptr, ptr %3, align 8
  %30 = invoke zeroext i1 @token(ptr noundef %29, i8 noundef signext 69)
          to label %31 unwind label %33

31:
  br i1 %30, label %32, label %37

32:
  store i1 true, ptr %2, align 1
  store i32 1, ptr %5, align 4
  br label %40

33:
  %34 = landingpad { ptr, i32 }
          cleanup
  %35 = extractvalue { ptr, i32 } %34, 0
  store ptr %35, ptr %7, align 8
  %36 = extractvalue { ptr, i32 } %34, 1
  store i32 %36, ptr %8, align 4
  call void @guard_done(ptr noundef %4)
  br label %42

37:
  %38 = load ptr, ptr %3, align 8
  %39 = getelementptr inbounds %struct.State, ptr %38, i32 0, i32 4
  call void @llvm.memcpy.p0.p0.i64(ptr align 4 %39, ptr align 4 %6, i64 16, i1 false)
  store i1 false, ptr %2, align 1
  store i32 1, ptr %5, align 4
  br label %40

40:
  call void @guard_done(ptr noundef %4)
  %41 = load i1, ptr %2, align 1
  ret i1 %41

42:
  %43 = load ptr, ptr %7, align 8
  %44 = load i32, ptr %8, align 4
  %45 = insertvalue { ptr, i32 } poison, ptr %43, 0
  %46 = insertvalue { ptr, i32 } %45, i32 %44, 1
  resume { ptr, i32 } %46
}

define internal zeroext i1 @parse_expression(ptr noundef %0) noinline optnone personality ptr @__gxx_personality_v0 {
  %2 = alloca i1, align 1
  %3 = alloca ptr, align 8
  %4 = alloca %struct.Guard, align 8
  %5 = alloca i32, align 4
  %6 = alloca ptr, align 8
  %7 = alloca i32, align 4
  store ptr %0, ptr %3, align 8
  %8 = load ptr, ptr %3, align 8
  call void @guard_init(ptr noundef %4, ptr noundef %8)
  %9 = load ptr, ptr %3, align 8
  %10 = getelementptr inbounds %struct.State, ptr %9, i32 0, i32 2
  %11 = load i32, ptr %10, align 8
  %12 = trunc i32 %11 to i8
  %13 = sext i8 %12 to i16
  %14 = sext i16 %13 to i64
  %15 = sitofp i32 %11 to double
  %16 = fptrunc double %15 to float
  %17 = invoke zeroext i1 @token(ptr noundef %9, i8 noundef signext 76)
          to label %18 unwind label %44

18:
  br i1 %17, label %19, label %26

19:
  %20 = invoke zeroext i1 @parse_nested(ptr noundef %9)
          to label %21 unwind label %44

21:
  br i1 %20, label %24, label %22

22:
  %23 = invoke zeroext i1 @token(ptr noundef %9, i8 noundef signext 69)
          to label %24 unwind label %44

24:
  %25 = phi i1 [ true, %21 ], [ %23, %22 ]
  br label %26

26:
  %27 = phi i1 [ false, %18 ], [ %25, %24 ]
  %28 = add i32 %11, %11
  %29 = mul i32 %28, %28
  %30 = icmp sgt i32 %29, 7
  %31 = and i1 %27, %30
  br i1 %31, label %32, label %40

32:
  %33 = trunc i64 %14 to i16
  %34 = sext i16 %33 to i32
  %35 = fpext float %16 to double
  %36 = fptosi double %35 to i32
  %37 = add i32 %34, %36
  %38 = icmp ne i32 %37, %29
  %39 = and i1 %38, %38
  store i1 %39, ptr %2, align 1
  br label %41

40:
  store i1 false, ptr %2, align 1
  br label %41

41:
  call void @guard_done(ptr noundef %4)
  %42 = load i1, ptr %2, align 1
  %43 = and i1 %42, %42
  ret i1 %43

44:
  %45 = landingpad { ptr, i32 }
          cleanup
  %46 = extractvalue { ptr, i32 } %45, 0
  store ptr %46, ptr %6, align 8
  %47 = extractvalue { ptr, i32 } %45, 1
  store i32 %47, ptr %7, align 4
  call void @guard_done(ptr noundef %4)
  %48 = load ptr, ptr %6, align 8
  %49 = load i32, ptr %7, align 4
  %50 = insertvalue { ptr, i32 } poison, ptr %48, 0
  %51 = insertvalue { ptr, i32 } %50, i32 %49, 1
  resume { ptr, i32 } %51
}

define dso_local zeroext i1 @demangle(ptr noundef %0) noinline optnone {
  %2 = alloca %struct.State, align 8
  %3 = getelementptr inbounds %struct.State, ptr %2, i32 0, i32 0
  store ptr %0, ptr %3, align 8
  %4 = call zeroext i1 @parse_nested(ptr noundef %2)
  br i1 %4, label %5, label %7

5:
  %6 = call zeroext i1 @parse_expression(ptr noundef %2)
  br label %7

7:
  %8 = phi i1 [ false, %1 ], [ %6, %5 ]
  ret i1 %8
}
