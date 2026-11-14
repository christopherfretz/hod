; `initializers.ll` written one lane at a time, for `llc`'s build.
@cv = global <2 x ptr> <ptr getelementptr (i32, ptr @arr, i64 1), ptr getelementptr (i32, ptr @arr, i64 3)>, align 16
@cs = global <4 x ptr> <ptr getelementptr inbounds (%S, ptr @arr, i32 0, i32 2, i64 1), ptr getelementptr inbounds (%S, ptr @arr, i32 1, i32 2, i64 1), ptr getelementptr inbounds (%S, ptr @arr, i32 -1, i32 2, i64 1), ptr getelementptr inbounds (%S, ptr @arr, i32 2, i32 2, i64 1)>, align 32
