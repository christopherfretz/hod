%Big = type { [5 x i64] }

define i32 @in_rm(i32 %x) {
  %p = alloca i32
  store i32 %x, ptr %p
  %v = load i32, ptr %p
  call void asm sideeffect "", "r|m,~{memory}"(i32 %v)
  %r = load i32, ptr %p
  ret i32 %r
}
define i32 @inout_rm(i32 %x) {
  %p = alloca i32
  store i32 %x, ptr %p
  %v = load i32, ptr %p
  call void asm sideeffect "", "=*r|m,0,~{memory}"(ptr elementtype(i32) %p, i32 %v)
  %r = load i32, ptr %p
  ret i32 %r
}
define i32 @t_rm(ptr %p) {
  store i32 5, ptr %p
  call void asm sideeffect "", "=*r|m,0"(ptr elementtype(i32) %p, i32 7)
  %v = load i32, ptr %p
  ret i32 %v
}
define i32 @t_mr(ptr %p) {
  store i32 5, ptr %p
  call void asm sideeffect "", "=*m|r,0,~{memory}"(ptr elementtype(i32) %p, i32 7)
  %v = load i32, ptr %p
  ret i32 %v
}
define i32 @t_r(ptr %p) {
  store i32 5, ptr %p
  call void asm sideeffect "", "=*r,0"(ptr elementtype(i32) %p, i32 7)
  %v = load i32, ptr %p
  ret i32 %v
}
define i32 @t_m(ptr %p) {
  store i32 5, ptr %p
  call void asm sideeffect "", "=*m,0"(ptr elementtype(i32) %p, i32 7)
  %v = load i32, ptr %p
  ret i32 %v
}
define i32 @t_imr(ptr %p) {
  store i32 5, ptr %p
  call void asm sideeffect "", "=*imr,0,~{memory}"(ptr elementtype(i32) %p, i32 7)
  %v = load i32, ptr %p
  ret i32 %v
}
define i32 @t_rm1(ptr %p) {
  store i32 5, ptr %p
  call void asm "", "=*rm,0"(ptr elementtype(i32) %p, i32 7)
  %v = load i32, ptr %p
  ret i32 %v
}
define i32 @t_x(ptr %p) {
  store i32 5, ptr %p
  call void asm "", "=*X,0"(ptr elementtype(i32) %p, i32 7)
  %v = load i32, ptr %p
  ret i32 %v
}
define i32 @t_rr(ptr %p) {
  store i32 5, ptr %p
  call void asm "", "=*r|r,0"(ptr elementtype(i32) %p, i32 7)
  %v = load i32, ptr %p
  ret i32 %v
}
define i32 @t_rm_0i(ptr %p) {
  store i32 5, ptr %p
  call void asm "", "=*r|m,0|i"(ptr elementtype(i32) %p, i32 7)
  %v = load i32, ptr %p
  ret i32 %v
}
define i8 @t_i8(ptr %p, i8 %x) {
  store i8 5, ptr %p
  call void asm "", "=*r,0"(ptr elementtype(i8) %p, i8 %x)
  %v = load i8, ptr %p
  ret i8 %v
}
define i64 @t_i64(ptr %p, i64 %x) {
  store i64 5, ptr %p
  call void asm "", "=*imr,0"(ptr elementtype(i64) %p, i64 %x)
  %v = load i64, ptr %p
  ret i64 %v
}
define ptr @t_ptr(ptr %p, ptr %x) {
  store ptr null, ptr %p
  call void asm "", "=*r,0"(ptr elementtype(ptr) %p, ptr %x)
  %v = load ptr, ptr %p
  ret ptr %v
}
define i32 @d_i32(i32 %x) {
  %r = call i32 asm "", "=r,0"(i32 %x)
  ret i32 %r
}
define i64 @d_i64(i64 %x) {
  %r = call i64 asm sideeffect "", "=r,0,~{memory}"(i64 %x)
  ret i64 %r
}
define i16 @d_i16(i16 %x) {
  %r = call i16 asm sideeffect "", "=r,0"(i16 %x)
  ret i16 %r
}
define ptr @d_ptr(ptr %x) {
  %r = call ptr asm "", "=r,0"(ptr %x)
  ret ptr %r
}
define i32 @d_const() {
  %r = call i32 asm "", "=r,0"(i32 42)
  ret i32 %r
}
define i32 @d_mid(i32 %a, i32 %b, i32 %c) {
  %r = call i32 asm sideeffect "", "=r,r,r,0,~{memory}"(i32 %a, i32 %b, i32 %c)
  ret i32 %r
}
define i64 @d_add(i64 %a, i64 %b) {
  %s = add i64 %a, %b
  %r = call i64 asm "", "=r,0"(i64 %s)
  %t = add i64 %r, 1
  ret i64 %t
}
define i64 @agg(ptr %p) {
  call void asm sideeffect "", "*r|m,~{memory}"(ptr elementtype(%Big) %p)
  call void asm sideeffect "", "=*r|m,*0,~{memory}"(ptr elementtype(%Big) %p, ptr elementtype(%Big) %p)
  %f = getelementptr %Big, ptr %p, i64 0, i32 0, i64 4
  %v = load i64, ptr %f
  ret i64 %v
}
define i32 @mm(ptr %p, ptr %q) {
  store i32 1, ptr %p
  store i32 2, ptr %q
  call void asm sideeffect "", "=*m,=*m,*m,*m"(ptr elementtype(i32) %p, ptr elementtype(i32) %q, ptr elementtype(i32) %p, ptr elementtype(i32) %q)
  call void asm "", "=*imr,=*m,0,*m"(ptr elementtype(i32) %p, ptr elementtype(i32) %q, i32 9, ptr elementtype(i32) %q)
  %a = load i32, ptr %p
  %b = load i32, ptr %q
  %s = mul i32 %a, 10
  %t = add i32 %s, %b
  ret i32 %t
}
define i64 @plain(i64 %a, i64 %d) {
  call void asm sideeffect "", ""()
  call void asm sideeffect "", "~{memory},~{dirflag},~{fpsr},~{flags}"()
  call void asm sideeffect "  # comment\0A\09", ""()
  call void asm sideeffect "", "~{cc}"()
  call void asm sideeffect ";", "~{rax},~{rbx},~{xmm8},~{xmm1},~{ecx},~{r12d},~{r9b},~{si},~{dil}"()
  call void asm alignstack inteldialect "", "r,q"(i64 %a, i64 %d)
  call void asm sideeffect unwind "", "imr,i,{rcx},I"(i64 %a, i32 3, i64 %a, i32 7)
  call void asm sideeffect "", "{ax},Q,~{dirflag},~{fpsr},~{flags}"(i32 9, i64 %d)
  ret i64 %a
}
define i32 @d_q(i32 %x) {
  %r = call i32 asm "", "=q,0,~{dirflag},~{fpsr},~{flags}"(i32 %x)
  ret i32 %r
}
define i32 @d_spec(i32 %x) {
  %r = call i32 asm "", "={cx},0"(i32 %x)
  ret i32 %r
}
define i64 @t_ptr_rm(ptr %p, i64 %x) {
  store i64 5, ptr %p
  call void asm "", "=*r|m,0|0"(ptr elementtype(i64) %p, i64 %x)
  %v = load i64, ptr %p
  ret i64 %v
}
define i32 @t_q(ptr %p) {
  store i32 5, ptr %p
  call void asm "", "=*q,0"(ptr elementtype(i32) %p, i32 7)
  %v = load i32, ptr %p
  ret i32 %v
}
define i32 @cmp(i32 %x) {
  %c = icmp eq i32 %x, 3
  call void asm sideeffect "", "r,~{memory}"(i1 %c)
  %z = zext i1 %c to i32
  ret i32 %z
}
declare i32 @hod_test_personality(...)
define i32 @inv(i32 %x) personality ptr @hod_test_personality {
entry:
  %r = invoke i32 asm sideeffect unwind "", "=r,0"(i32 %x) to label %ok unwind label %pad
ok:
  %v = phi i32 [ %r, %entry ]
  %w = add i32 %v, 1
  ret i32 %w
pad:
  %lp = landingpad { ptr, i32 } cleanup
  ret i32 -1
}
define void @inv_void(ptr %p) personality ptr @hod_test_personality {
entry:
  store i32 3, ptr %p
  invoke void asm sideeffect unwind "", "=*r,0,~{memory}"(ptr elementtype(i32) %p, i32 4) to label %ok unwind label %pad
ok:
  ret void
pad:
  %lp = landingpad { ptr, i32 } cleanup
  store i32 -1, ptr %p
  ret void
}
