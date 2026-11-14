; Constant vector `getelementptr`s in initializers, folded lane by lane
; by hod. `llc` 22 cannot build these two: on x86-64 it does not finish
; on `@cv` (more than 60 s), on AArch64 it fails on both with "Unknown
; ELF relocation type"; a vector base in an initializer it refuses with
; "unsupported expression in static initializer" (that form is in
; `hod_ir`'s unit test `vector_gep_folds_lane_by_lane`). Its
; build takes `initializers_by_lane.ll`, the same addresses written one
; lane at a time.
@cv = global <2 x ptr> getelementptr (i32, ptr @arr, <2 x i64> <i64 1, i64 3>), align 16
@cs = global <4 x ptr> getelementptr inbounds (%S, ptr @arr, <4 x i32> <i32 0, i32 1, i32 -1, i32 2>, i32 2, i64 1), align 32
