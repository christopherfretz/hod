target triple = "i686-unknown-linux-gnu"
define i32 @inr(i32 inreg %a, i32 inreg %b, i32 inreg %c, i32 %d) { %s = add i32 %a, %d
  ret i32 %s }
define i64 @inr64(i64 inreg %a, i32 inreg %b) { %c = zext i32 %b to i64
  %s = add i64 %a, %c
  ret i64 %s }
define void @inrp(ptr inreg sret(i32) %r, i32 inreg %a) { store i32 %a, ptr %r
  ret void }
define fastcc i32 @fcc(i32 %a, i32 %b, i32 %c, i64 %d) { %t = trunc i64 %d to i32
  %s = add i32 %a, %t
  ret i32 %s }
define x86_stdcallcc i32 @std(i32 %a, i32 %b) { ret i32 %a }
define x86_fastcallcc i32 @fast(i32 inreg %a, i32 inreg %b, i32 %c) { ret i32 %c }
define x86_thiscallcc i32 @this(i32 %a, i32 %b) { ret i32 %b }
define x86_regcallcc i32 @regc(i32 inreg %a, i32 inreg %b) { ret i32 %b }
define preserve_mostcc i32 @pmost(i32 %a) { ret i32 %a }
define preserve_allcc i32 @pall(i32 %a) { ret i32 %a }
define coldcc i32 @cold(i32 %a) { ret i32 %a }
define tailcc i32 @tcc(i32 %a) { ret i32 %a }
declare i32 @extc(i32)
define i32 @clobber(i32 %a) { %r = call i32 @extc(i32 %a)
  call void asm sideeffect "", "~{ebx},~{esi},~{edi},~{ebp}"()
  ret i32 %r }
