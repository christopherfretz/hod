; The frame llc builds at -O0 with a frame pointer and a callee-saved
; register in use, and its CFI.
define i64 @f(i64 %a) "frame-pointer"="all" {
  %s = alloca [100 x i8]
  call void asm sideeffect "", "~{rbx},~{r12}"()
  call void @g(ptr %s)
  ret i64 %a }
declare void @g(ptr)
