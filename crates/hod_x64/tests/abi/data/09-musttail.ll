; musttail in a variadic thunk: forwards al and every argument register.
declare i32 @target(ptr, ...)
define i32 @thunk(ptr %this, ...) {
  %r = musttail call i32 (ptr, ...) @target(ptr %this, ...)
  ret i32 %r }
