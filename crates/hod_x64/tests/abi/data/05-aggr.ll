; First-class aggregates as arguments and results: one part per leaf, each
; taking the next register of its class (no SysV eightbyte merging at the
; IR level); returns in rax rdx (rcx?) / xmm0 xmm1 (...), else demoted to sret.
declare void @sa({ i32, float }, [2 x i64], { i8, i16 })
define void @call_aggr({ i32, float } %a, [2 x i64] %b, { i8, i16 } %c) {
  call void @sa({ i32, float } %a, [2 x i64] %b, { i8, i16 } %c)
  ret void
}
define { i64, i64, i64 } @ret3(i64 %a) {
  %1 = insertvalue { i64, i64, i64 } poison, i64 %a, 2
  ret { i64, i64, i64 } %1 }
define { i64, i64, i64, i64 } @ret4(i64 %a) {
  %1 = insertvalue { i64, i64, i64, i64 } poison, i64 %a, 3
  ret { i64, i64, i64, i64 } %1 }
define [3 x i32] @ret3x32(i32 %a) {
  %1 = insertvalue [3 x i32] poison, i32 %a, 2
  ret [3 x i32] %1 }
define { ptr, i32 } @ret_lp(ptr %a, i32 %b) {
  %1 = insertvalue { ptr, i32 } poison, ptr %a, 0
  %2 = insertvalue { ptr, i32 } %1, i32 %b, 1
  ret { ptr, i32 } %2 }
declare void @s7({ i64, i64 }, { i64, i64 }, { i64, i64 }, { i64, i64 })
define void @call_spill({ i64, i64 } %a) {
  call void @s7({ i64, i64 } %a, { i64, i64 } %a, { i64, i64 } %a, { i64, i64 } %a)
  ret void
}
