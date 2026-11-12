; Branch protection the way LLVM 19 and older write it (spec/ll.md
; §7.3): module flags of value 1 that a reader spreads over the
; definitions, and "true" and "false" for values.

define void @f_all() "branch-target-enforcement"="true" "sign-return-address"="non-leaf" "sign-return-address-key"="b_key" "guarded-control-stack"="false" {
  ret void
}

define void @f_none() "sign-return-address"="none" "sign-return-address-key"="a_key" "branch-protection-pauth-lr"="true" {
  ret void
}

define void @f_plain() {
  ret void
}

define void @f_false() "branch-target-enforcement"="false" "sign-return-address"="all" {
  ret void
}

define void @f_group() #0 {
  ret void
}

define void @f_group_too() #0 {
  ret void
}

declare void @f_decl() "branch-target-enforcement"="true"

declare void @f_decl_plain()

attributes #0 = { nounwind "guarded-control-stack"="true" "sign-return-address"="all" "sign-return-address-key"="a_key" }

!llvm.module.flags = !{!0, !1, !2, !3}

!0 = !{i32 8, !"branch-target-enforcement", i32 1}
!1 = !{i32 8, !"sign-return-address", i32 1}
!2 = !{i32 8, !"sign-return-address-all", i32 0}
!3 = !{i32 8, !"sign-return-address-with-bkey", i32 1}
