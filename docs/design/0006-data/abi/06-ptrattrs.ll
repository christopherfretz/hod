; sret, byval, inreg, nest, swiftself/swifterror.
%S = type { i64, i64, i64 }
declare void @f_sret(ptr sret(%S) align 8, i64)
define void @call_sret(ptr %p) { call void @f_sret(ptr sret(%S) align 8 %p, i64 7)
  ret void }
define void @def_sret(ptr sret(%S) align 8 %p, i64 %x) { store i64 %x, ptr %p
  ret void }
declare void @f_byval(i64, ptr byval(%S) align 8, ptr byval([3 x double]) align 16)
define void @call_byval(ptr %p) {
  call void @f_byval(i64 1, ptr byval(%S) align 8 %p, ptr byval([3 x double]) align 16 %p)
  ret void }
define i64 @def_byval(i64 %a, ptr byval(%S) align 8 %p) { %v = load i64, ptr %p
  ret i64 %v }
declare i64 @f_nest(ptr nest, i64)
define i64 @call_nest(ptr %env) { %r = call i64 @f_nest(ptr nest %env, i64 3)
  ret i64 %r }
declare i64 @f_inreg(i64 inreg, i64)
define i64 @call_inreg() { %r = call i64 @f_inreg(i64 inreg 1, i64 2)
  ret i64 %r }
declare swiftcc i64 @f_swift(ptr swiftself, ptr swifterror)
define swiftcc i64 @call_swift(ptr swiftself %s, ptr swifterror %e) {
  %r = call swiftcc i64 @f_swift(ptr swiftself %s, ptr swifterror %e)
  ret i64 %r }
