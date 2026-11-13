; Bodies whose meaning depends on their frame (llvm.returnaddress,
; llvm.frameaddress) called directly in a module that holds alwaysinline,
; where `opt -passes=always-inline` (LLVM 19 and 22) keeps the call: a
; noinline callee at a plain call site, an alwaysinline callee at a
; noinline call site, a variadic and a recursive alwaysinline callee.
; Out of line is opt's result, so hod accepts them (WP-188) and each
; return address lies in its caller, each frame is the callee's own.
; @helper is alwaysinline and inlined (no frame intrinsic): it puts
; alwaysinline in the module.
;@ covers: spec/ir.md §5.7 call-site attributes (alwaysinline, noinline), spec/cli.md "Always-inline"
;@ stdout: noinline-callee in-caller
;@ stdout: noinline-site in-caller
;@ stdout: variadic in-caller
;@ stdout: recursive in-caller
;@ stdout: recursive in-caller
;@ stdout: frame own
;@ stdout: helper 42

@.where = private unnamed_addr constant [7 x i8] c"%s %s\0A\00"
@.num = private unnamed_addr constant [7 x i8] c"%s %d\0A\00"
@s.in = private unnamed_addr constant [10 x i8] c"in-caller\00"
@s.out = private unnamed_addr constant [6 x i8] c"other\00"
@s.own = private unnamed_addr constant [4 x i8] c"own\00"
@s.same = private unnamed_addr constant [5 x i8] c"same\00"
@s.nc = private unnamed_addr constant [16 x i8] c"noinline-callee\00"
@s.ns = private unnamed_addr constant [14 x i8] c"noinline-site\00"
@s.va = private unnamed_addr constant [9 x i8] c"variadic\00"
@s.rec = private unnamed_addr constant [10 x i8] c"recursive\00"
@s.fr = private unnamed_addr constant [6 x i8] c"frame\00"
@s.hl = private unnamed_addr constant [7 x i8] c"helper\00"
@fp = global ptr null

declare i32 @printf(ptr, ...)
declare ptr @llvm.returnaddress(i32 immarg)
declare ptr @llvm.frameaddress.p0(i32 immarg)

; Prints `what` and whether `ra` is an address inside `caller` (within
; 64 KiB after its entry).
define void @where(ptr %what, ptr %ra, ptr %caller) noinline {
  %a = ptrtoint ptr %ra to i64
  %c = ptrtoint ptr %caller to i64
  %d = sub i64 %a, %c
  %lo = icmp ugt i64 %d, 0
  %hi = icmp ult i64 %d, 65536
  %in = and i1 %lo, %hi
  %s = select i1 %in, ptr @s.in, ptr @s.out
  %r = call i32 (ptr, ...) @printf(ptr @.where, ptr %what, ptr %s)
  ret void
}

define internal i32 @helper(i32 %x) alwaysinline {
  %r = add i32 %x, 1
  ret i32 %r
}

define void @nc() noinline {
  %ra = call ptr @llvm.returnaddress(i32 0)
  call void @where(ptr @s.nc, ptr %ra, ptr @c_nc)
  ret void
}

define void @c_nc() noinline {
  call void @nc()
  ret void
}

define linkonce_odr void @ns() alwaysinline {
  %ra = call ptr @llvm.returnaddress(i32 0)
  call void @where(ptr @s.ns, ptr %ra, ptr @c_ns)
  ret void
}

define void @c_ns() noinline {
  call void @ns() noinline
  ret void
}

define internal void @va(i32 %n, ...) alwaysinline {
  %ra = call ptr @llvm.returnaddress(i32 0)
  call void @where(ptr @s.va, ptr %ra, ptr @c_va)
  ret void
}

define void @c_va() noinline {
  call void (i32, ...) @va(i32 1, i32 2)
  ret void
}

; Called with 1 from @c_rec, then with 0 from itself.
define internal void @rec(i32 %n) alwaysinline {
  %ra = call ptr @llvm.returnaddress(i32 0)
  %top = icmp ne i32 %n, 0
  %caller = select i1 %top, ptr @c_rec, ptr @rec
  call void @where(ptr @s.rec, ptr %ra, ptr %caller)
  br i1 %top, label %again, label %done
again:
  call void @rec(i32 0)
  br label %done
done:
  ret void
}

define void @c_rec() noinline {
  call void @rec(i32 1)
  ret void
}

define linkonce_odr ptr @fr() alwaysinline {
  %f = call ptr @llvm.frameaddress.p0(i32 0)
  ret ptr %f
}

define void @c_fr() noinline {
  %mine = call ptr @llvm.frameaddress.p0(i32 0)
  %f = call ptr @fr() noinline
  %same = icmp eq ptr %f, %mine
  %s = select i1 %same, ptr @s.same, ptr @s.own
  %r = call i32 (ptr, ...) @printf(ptr @.where, ptr @s.fr, ptr %s)
  ret void
}

define i32 @main(i32 %argc, ptr %argv) {
  call void @c_nc()
  call void @c_ns()
  call void @c_va()
  call void @c_rec()
  call void @c_fr()
  %h = call i32 @helper(i32 41)
  %r = call i32 (ptr, ...) @printf(ptr @.num, ptr @s.hl, i32 %h)
  ret i32 0
}
