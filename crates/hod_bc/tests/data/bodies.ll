; Every instruction record hod_bc reads (spec/bc.md §11.4), in bodies
; LLVM 19 and 22 both assemble. Checked against hod_ll on llvm-dis text.
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128"
target triple = "x86_64-unknown-linux-gnu"

%pair = type { i32, double }
%big = type { [4 x i8], %pair, <4 x float> }

@g = global i32 7
@table = global [2 x ptr] [ptr blockaddress(@indirect, %a), ptr blockaddress(@indirect, %b)]
@ti = external constant ptr
@ti2 = external constant ptr

declare i32 @callee(i32, ...)
declare void @sink(ptr)
declare i32 @__gxx_personality_v0(...)
declare void @llvm.lifetime.start.p0(i64 immarg, ptr nocapture)
declare void @llvm.lifetime.end.p0(i64 immarg, ptr nocapture)
declare i64 @llvm.expect.i64(i64, i64)
declare void @llvm.memcpy.p0.p0.i64(ptr noalias nocapture writeonly, ptr noalias nocapture readonly, i64, i1 immarg)
declare i32 @llvm.ctpop.i32(i32)
declare void @llvm.va_start.p0(ptr)
declare void @llvm.va_end.p0(ptr)
declare void @noreturn() noreturn nounwind

define i32 @arith(i32 %a, i32 %b, float %f, double %d, <4 x i32> %v) {
entry:
  %add = add nsw i32 %a, %b
  %sub = sub nuw i32 %add, 1
  %mul = mul i32 %sub, %b
  %udiv = udiv exact i32 %mul, 3
  %sdiv = sdiv i32 %udiv, -2
  %urem = urem i32 %sdiv, 5
  %srem = srem i32 %urem, 7
  %shl = shl i32 %srem, 2
  %lshr = lshr exact i32 %shl, 1
  %ashr = ashr i32 %lshr, 1
  %and = and i32 %ashr, 255
  %or = or disjoint i32 %and, 256
  %xor = xor i32 %or, -1
  %fadd = fadd fast float %f, 1.0
  %fsub = fsub float %fadd, 2.0
  %fmul = fmul nnan float %fsub, %f
  %fdiv = fdiv float %fmul, 3.0
  %frem = frem float %fdiv, 4.0
  %fneg = fneg float %frem
  %vadd = add <4 x i32> %v, <i32 1, i32 2, i32 3, i32 4>
  %vsplat = mul <4 x i32> %vadd, splat (i32 3)
  %fr = freeze i32 %xor
  %ext = extractelement <4 x i32> %vsplat, i32 0
  %ins = insertelement <4 x i32> %vsplat, i32 %fr, i64 1
  %shuf = shufflevector <4 x i32> %ins, <4 x i32> %v, <4 x i32> <i32 0, i32 5, i32 poison, i32 7>
  %shuf0 = shufflevector <4 x i32> %ins, <4 x i32> poison, <2 x i32> zeroinitializer
  %e1 = extractelement <2 x i32> %shuf0, i32 1
  %e2 = extractelement <4 x i32> %shuf, i32 %e1
  %c1 = icmp slt i32 %e2, %ext
  %c2 = icmp ult i32 %a, %b
  %c3 = fcmp olt float %fneg, %f
  %c4 = fcmp uno double %d, 0.0
  %vc = icmp eq <4 x i32> %v, zeroinitializer
  %sel = select i1 %c1, i32 %a, i32 %b
  %vsel = select <4 x i1> %vc, <4 x i32> %v, <4 x i32> %shuf
  %selc = select i1 %c3, i1 %c2, i1 %c4
  %r = zext i1 %selc to i32
  %t = trunc i32 %sel to i8
  %s = sext i8 %t to i64
  %fp = sitofp i64 %s to double
  %ui = uitofp i32 %r to float
  %fi = fptosi double %fp to i32
  %fu = fptoui float %ui to i16
  %ft = fptrunc double %d to float
  %fe = fpext float %ft to double
  %bc = bitcast <4 x i32> %vsel to <2 x i64>
  %p = inttoptr i64 %s to ptr
  %pi = ptrtoint ptr %p to i64
  %as = addrspacecast ptr %p to ptr addrspace(1)
  %sum = add i32 %fi, %r
  ret i32 %sum
}

define i64 @memory(ptr %p, i64 %n) {
entry:
  %slot = alloca i32, align 4
  %arr = alloca [4 x i64], align 16
  %two = alloca i64, i64 2, align 8
  %pair = alloca %pair, align 8
  store i32 1, ptr %slot, align 4
  store volatile i32 2, ptr %slot, align 4
  %l = load i32, ptr %slot, align 4
  %lv = load volatile i64, ptr %p, align 8
  %la = load atomic i64, ptr %p acquire, align 8
  store atomic i64 %la, ptr %p release, align 8
  %ls = load atomic i32, ptr %slot syncscope("singlethread") monotonic, align 4
  %cx = cmpxchg ptr %p, i64 %lv, i64 %n seq_cst acquire, align 8
  %cxw = cmpxchg weak volatile ptr %p, i64 %lv, i64 %n syncscope("singlethread") acq_rel monotonic, align 8
  %cv = extractvalue { i64, i1 } %cx, 0
  %ok = extractvalue { i64, i1 } %cxw, 1
  %rx = atomicrmw xchg ptr %p, i64 %cv seq_cst, align 8
  %ra = atomicrmw add ptr %p, i64 1 monotonic, align 8
  %rm = atomicrmw umax ptr %p, i64 3 acq_rel, align 8
  %rf = atomicrmw fadd ptr %p, double 1.0 seq_cst, align 8
  %ri = atomicrmw uinc_wrap ptr %slot, i32 9 seq_cst, align 4
  fence acquire
  fence syncscope("singlethread") seq_cst
  %g1 = getelementptr inbounds [4 x i64], ptr %arr, i64 0, i64 2
  %g2 = getelementptr i64, ptr %arr, i64 %n
  %g3 = getelementptr inbounds %pair, ptr %pair, i32 0, i32 1
  %g4 = getelementptr [4 x i64], ptr %arr, i64 1, i64 %lv
  %g5 = getelementptr inbounds nuw i8, ptr %p, i64 -3
  %gv = getelementptr i32, <2 x ptr> zeroinitializer, <2 x i64> <i64 1, i64 2>
  %agg = insertvalue { i64, i1 } undef, i64 %ra, 0
  %agg2 = insertvalue { i64, i1 } %agg, i1 %ok, 1
  %bigv = insertvalue %big zeroinitializer, double 2.0, 1, 1
  %bigf = extractvalue %big %bigv, 2
  %x = extractvalue { i64, i1 } %agg2, 0
  call void @llvm.lifetime.start.p0(i64 8, ptr %two)
  call void @llvm.memcpy.p0.p0.i64(ptr align 8 %two, ptr %g1, i64 16, i1 false)
  call void @llvm.lifetime.end.p0(i64 8, ptr %two)
  %ex = call i64 @llvm.expect.i64(i64 %x, i64 0)
  %dyn = alloca i8, i64 %ex, align 1
  call void @sink(ptr %dyn)
  %sum = add i64 %ex, %rm
  %s2 = add i64 %sum, %rx
  %ff = fptosi double %rf to i64
  %s3 = add i64 %s2, %ff
  br label %next

next:
  %late = alloca i32, align 4
  call void @sink(ptr %late)
  %li = load i32, ptr %g3, align 4
  %li2 = zext i32 %li to i64
  %s4 = add i64 %s3, %li2
  ret i64 %s4
}

define i32 @control(i32 %x, ptr %fp) personality ptr @__gxx_personality_v0 {
entry:
  %c = icmp eq i32 %x, 0
  br i1 %c, label %loop, label %sw

loop:
  %i = phi i32 [ 0, %entry ], [ %inc, %loop ]
  %acc = phi i32 [ %x, %entry ], [ %acc2, %loop ]
  %inc = add i32 %i, 1
  %acc2 = add i32 %acc, %inc
  %done = icmp sge i32 %inc, 10
  br i1 %done, label %sw, label %loop

sw:
  %m = phi i32 [ %x, %entry ], [ %acc2, %loop ]
  switch i32 %m, label %dflt [
    i32 1, label %one
    i32 -5, label %two
    i32 100000, label %two
  ]

one:
  %r1 = invoke i32 (i32, ...) @callee(i32 1, i32 2, double 3.0)
          to label %cont unwind label %lpad

two:
  %r2 = call i32 (i32, ...) @callee(i32 %m)
  %ind = tail call i32 %fp(i32 %r2) #1
  %pc = call i32 @llvm.ctpop.i32(i32 %ind)
  br label %cont

cont:
  %r = phi i32 [ %r1, %one ], [ %pc, %two ]
  ret i32 %r

dflt:
  invoke void @sink(ptr %fp) [ "funclet-free"(i32 %m) ]
          to label %cont2 unwind label %lpad

cont2:
  call void @noreturn() #0
  unreachable

lpad:
  %lp = landingpad { ptr, i32 }
          cleanup
          catch ptr @ti
          filter [1 x ptr] [ptr @ti2]
  resume { ptr, i32 } %lp
}

define void @indirect(i32 %k) {
entry:
  %slot = getelementptr [2 x ptr], ptr @table, i32 0, i32 %k
  %target = load ptr, ptr %slot, align 8
  indirectbr ptr %target, [label %a, label %b]

a:
  ret void

b:
  br label %a
}

define i32 @varargs(i32 %n, ...) {
entry:
  %list = alloca ptr, align 8
  call void @llvm.va_start.p0(ptr %list)
  %v = va_arg ptr %list, i32
  call void @llvm.va_end.p0(ptr %list)
  %r = call i32 asm sideeffect "mov $1, $0", "=r,r,~{dirflag}"(i32 %v)
  %s = call i32 asm inteldialect "lea $0, [$1]", "=r,r"(i32 %r)
  ret i32 %s
}

define i32 @forward(i32 %x) {
entry:
  br label %def

use:
  %u = add i32 %d, 1
  %w = add i32 %u, %d
  ret i32 %w

def:
  %d = mul i32 %x, 2
  br label %use
}

; Unsupported: the funclet instructions and an inalloca alloca.
define void @funclets() personality ptr @__gxx_personality_v0 {
entry:
  %a = alloca inalloca i32, align 4
  invoke void @sink(ptr %a)
          to label %ok unwind label %dispatch

ok:
  ret void

dispatch:
  %cs = catchswitch within none [label %handler] unwind to caller

handler:
  %cp = catchpad within %cs [ptr null]
  catchret from %cp to label %ok
}

define void @scopes(ptr %p) {
  %v = load atomic i32, ptr %p syncscope("agent") acquire, align 4
  ret void
}

define void @musttail_va(ptr %p, ...) {
  musttail call void (ptr, ...) @musttail_va(ptr %p, ...)
  ret void
}

define void @with_md(ptr %p) !prof !0 !custom !1 {
  call void @sink(ptr %p), !annotation !1
  ret void
}

attributes #0 = { noreturn nounwind }
attributes #1 = { nounwind "no-builtins" }

!0 = !{!"function_entry_count", i64 10}
!1 = !{!"x"}
