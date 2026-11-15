; Reduced from a kiln-built translation unit (WP-269): a folded
; `llvm.is.constant` branch, and an `invoke` of a `noreturn nounwind`
; callee whose unwind edge goes to a landing pad. hod keeps the `invoke`
; and its landing pad; `llc` turns it into a call.

define noundef { ptr, i32 } @main(i1 %0) #0 personality ptr @__gxx_personality_v0 {
  %2 = call i1 @llvm.is.constant.i1(i1 %0)
  br i1 %2, label %3, label %6

3:                                                ; preds = %1
  ret { ptr, i32 } zeroinitializer

4:                                                ; preds = %6
  %5 = landingpad { ptr, i32 }
          catch ptr null
  ret { ptr, i32 } %5

6:                                                ; preds = %1
  invoke void @ensure_error(i1 noundef zeroext false) #3
          to label %7 unwind label %4

7:                                                ; preds = %6
  unreachable
}

declare i32 @__gxx_personality_v0(...)

; Function Attrs: convergent nocallback nofree nosync nounwind willreturn memory(none)
declare i1 @llvm.is.constant.i1(i1) #1

; Function Attrs: mustprogress noinline noreturn nounwind optnone uwtable
declare void @ensure_error(i1 noundef zeroext) #2

attributes #0 = { mustprogress noinline norecurse optnone uwtable "frame-pointer"="all" "min-legal-vector-width"="0" "no-trapping-math"="true" "stack-protector-buffer-size"="8" }
attributes #1 = { convergent nocallback nofree nosync nounwind willreturn memory(none) }
attributes #2 = { mustprogress noinline noreturn nounwind optnone uwtable "frame-pointer"="all" "min-legal-vector-width"="0" "no-trapping-math"="true" "stack-protector-buffer-size"="8" }
attributes #3 = { noreturn }
