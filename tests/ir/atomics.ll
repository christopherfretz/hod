; Atomic memory operations: atomic load and store, every integer atomicrmw
; operation at 8, 32 and 64 bits and under each ordering, atomicrmw fadd
; and fsub, cmpxchg (strong, succeeding and failing; weak, in a retry
; loop), fence, and finally four threads that update shared counters,
; which only gives the right totals if the updates really are atomic.
;@ covers: spec/ir.md §5.4 memory (atomic Load and Store, Fence, CmpXchg, AtomicRmw)
;@ unsupported(x86_64): type:double
;@ stdout: i8 xchg      100, 27: old 100 new 27
;@ stdout: i8 add       100, 27: old 100 new 127
;@ stdout: i8 sub       100, 27: old 100 new 73
;@ stdout: i8 and       100, 27: old 100 new 0
;@ stdout: i8 nand      100, 27: old 100 new -1
;@ stdout: i8 or        100, 27: old 100 new 127
;@ stdout: i8 xor       100, 27: old 100 new 127
;@ stdout: i8 max       100, 27: old 100 new 100
;@ stdout: i8 min       100, 27: old 100 new 27
;@ stdout: i8 umax      100, 27: old 100 new 100
;@ stdout: i8 umin      100, 27: old 100 new 27
;@ stdout: i8 uinc_wrap 100, 27: old 100 new 0
;@ stdout: i8 udec_wrap 100, 27: old 100 new 27
;@ stdout: i8 xchg      -128, 1: old -128 new 1
;@ stdout: i8 add       -128, 1: old -128 new -127
;@ stdout: i8 sub       -128, 1: old -128 new 127
;@ stdout: i8 and       -128, 1: old -128 new 0
;@ stdout: i8 nand      -128, 1: old -128 new -1
;@ stdout: i8 or        -128, 1: old -128 new -127
;@ stdout: i8 xor       -128, 1: old -128 new -127
;@ stdout: i8 max       -128, 1: old -128 new 1
;@ stdout: i8 min       -128, 1: old -128 new -128
;@ stdout: i8 umax      -128, 1: old -128 new -128
;@ stdout: i8 umin      -128, 1: old -128 new 1
;@ stdout: i8 uinc_wrap -128, 1: old -128 new 0
;@ stdout: i8 udec_wrap -128, 1: old -128 new 1
;@ stdout: i8 xchg      5, 5: old 5 new 5
;@ stdout: i8 add       5, 5: old 5 new 10
;@ stdout: i8 sub       5, 5: old 5 new 0
;@ stdout: i8 and       5, 5: old 5 new 5
;@ stdout: i8 nand      5, 5: old 5 new -6
;@ stdout: i8 or        5, 5: old 5 new 5
;@ stdout: i8 xor       5, 5: old 5 new 0
;@ stdout: i8 max       5, 5: old 5 new 5
;@ stdout: i8 min       5, 5: old 5 new 5
;@ stdout: i8 umax      5, 5: old 5 new 5
;@ stdout: i8 umin      5, 5: old 5 new 5
;@ stdout: i8 uinc_wrap 5, 5: old 5 new 0
;@ stdout: i8 udec_wrap 5, 5: old 5 new 4
;@ stdout: i8 xchg      0, -1: old 0 new -1
;@ stdout: i8 add       0, -1: old 0 new -1
;@ stdout: i8 sub       0, -1: old 0 new 1
;@ stdout: i8 and       0, -1: old 0 new 0
;@ stdout: i8 nand      0, -1: old 0 new -1
;@ stdout: i8 or        0, -1: old 0 new -1
;@ stdout: i8 xor       0, -1: old 0 new -1
;@ stdout: i8 max       0, -1: old 0 new 0
;@ stdout: i8 min       0, -1: old 0 new -1
;@ stdout: i8 umax      0, -1: old 0 new -1
;@ stdout: i8 umin      0, -1: old 0 new 0
;@ stdout: i8 uinc_wrap 0, -1: old 0 new 1
;@ stdout: i8 udec_wrap 0, -1: old 0 new -1
;@ stdout: i32 xchg      1000, -1: old 1000 new -1
;@ stdout: i32 add       1000, -1: old 1000 new 999
;@ stdout: i32 sub       1000, -1: old 1000 new 1001
;@ stdout: i32 and       1000, -1: old 1000 new 1000
;@ stdout: i32 nand      1000, -1: old 1000 new -1001
;@ stdout: i32 or        1000, -1: old 1000 new -1
;@ stdout: i32 xor       1000, -1: old 1000 new -1001
;@ stdout: i32 max       1000, -1: old 1000 new 1000
;@ stdout: i32 min       1000, -1: old 1000 new -1
;@ stdout: i32 umax      1000, -1: old 1000 new -1
;@ stdout: i32 umin      1000, -1: old 1000 new 1000
;@ stdout: i32 uinc_wrap 1000, -1: old 1000 new 1001
;@ stdout: i32 udec_wrap 1000, -1: old 1000 new 999
;@ stdout: i32 xchg      -7, 7: old -7 new 7
;@ stdout: i32 add       -7, 7: old -7 new 0
;@ stdout: i32 sub       -7, 7: old -7 new -14
;@ stdout: i32 and       -7, 7: old -7 new 1
;@ stdout: i32 nand      -7, 7: old -7 new -2
;@ stdout: i32 or        -7, 7: old -7 new -1
;@ stdout: i32 xor       -7, 7: old -7 new -2
;@ stdout: i32 max       -7, 7: old -7 new 7
;@ stdout: i32 min       -7, 7: old -7 new -7
;@ stdout: i32 umax      -7, 7: old -7 new -7
;@ stdout: i32 umin      -7, 7: old -7 new 7
;@ stdout: i32 uinc_wrap -7, 7: old -7 new 0
;@ stdout: i32 udec_wrap -7, 7: old -7 new 7
;@ stdout: i32 xchg      123456, 65535: old 123456 new 65535
;@ stdout: i32 add       123456, 65535: old 123456 new 188991
;@ stdout: i32 sub       123456, 65535: old 123456 new 57921
;@ stdout: i32 and       123456, 65535: old 123456 new 57920
;@ stdout: i32 nand      123456, 65535: old 123456 new -57921
;@ stdout: i32 or        123456, 65535: old 123456 new 131071
;@ stdout: i32 xor       123456, 65535: old 123456 new 73151
;@ stdout: i32 max       123456, 65535: old 123456 new 123456
;@ stdout: i32 min       123456, 65535: old 123456 new 65535
;@ stdout: i32 umax      123456, 65535: old 123456 new 123456
;@ stdout: i32 umin      123456, 65535: old 123456 new 65535
;@ stdout: i32 uinc_wrap 123456, 65535: old 123456 new 0
;@ stdout: i32 udec_wrap 123456, 65535: old 123456 new 65535
;@ stdout: i32 xchg      0, 0: old 0 new 0
;@ stdout: i32 add       0, 0: old 0 new 0
;@ stdout: i32 sub       0, 0: old 0 new 0
;@ stdout: i32 and       0, 0: old 0 new 0
;@ stdout: i32 nand      0, 0: old 0 new -1
;@ stdout: i32 or        0, 0: old 0 new 0
;@ stdout: i32 xor       0, 0: old 0 new 0
;@ stdout: i32 max       0, 0: old 0 new 0
;@ stdout: i32 min       0, 0: old 0 new 0
;@ stdout: i32 umax      0, 0: old 0 new 0
;@ stdout: i32 umin      0, 0: old 0 new 0
;@ stdout: i32 uinc_wrap 0, 0: old 0 new 0
;@ stdout: i32 udec_wrap 0, 0: old 0 new 0
;@ stdout: i64 xchg      1099511627776, 3: old 1099511627776 new 3
;@ stdout: i64 add       1099511627776, 3: old 1099511627776 new 1099511627779
;@ stdout: i64 sub       1099511627776, 3: old 1099511627776 new 1099511627773
;@ stdout: i64 and       1099511627776, 3: old 1099511627776 new 0
;@ stdout: i64 nand      1099511627776, 3: old 1099511627776 new -1
;@ stdout: i64 or        1099511627776, 3: old 1099511627776 new 1099511627779
;@ stdout: i64 xor       1099511627776, 3: old 1099511627776 new 1099511627779
;@ stdout: i64 max       1099511627776, 3: old 1099511627776 new 1099511627776
;@ stdout: i64 min       1099511627776, 3: old 1099511627776 new 3
;@ stdout: i64 umax      1099511627776, 3: old 1099511627776 new 1099511627776
;@ stdout: i64 umin      1099511627776, 3: old 1099511627776 new 3
;@ stdout: i64 uinc_wrap 1099511627776, 3: old 1099511627776 new 0
;@ stdout: i64 udec_wrap 1099511627776, 3: old 1099511627776 new 3
;@ stdout: i64 xchg      -1, 1: old -1 new 1
;@ stdout: i64 add       -1, 1: old -1 new 0
;@ stdout: i64 sub       -1, 1: old -1 new -2
;@ stdout: i64 and       -1, 1: old -1 new 1
;@ stdout: i64 nand      -1, 1: old -1 new -2
;@ stdout: i64 or        -1, 1: old -1 new -1
;@ stdout: i64 xor       -1, 1: old -1 new -2
;@ stdout: i64 max       -1, 1: old -1 new 1
;@ stdout: i64 min       -1, 1: old -1 new -1
;@ stdout: i64 umax      -1, 1: old -1 new -1
;@ stdout: i64 umin      -1, 1: old -1 new 1
;@ stdout: i64 uinc_wrap -1, 1: old -1 new 0
;@ stdout: i64 udec_wrap -1, 1: old -1 new 1
;@ stdout: i64 xchg      81985529216486895, -81985529216486896: old 81985529216486895 new -81985529216486896
;@ stdout: i64 add       81985529216486895, -81985529216486896: old 81985529216486895 new -1
;@ stdout: i64 sub       81985529216486895, -81985529216486896: old 81985529216486895 new 163971058432973791
;@ stdout: i64 and       81985529216486895, -81985529216486896: old 81985529216486895 new 0
;@ stdout: i64 nand      81985529216486895, -81985529216486896: old 81985529216486895 new -1
;@ stdout: i64 or        81985529216486895, -81985529216486896: old 81985529216486895 new -1
;@ stdout: i64 xor       81985529216486895, -81985529216486896: old 81985529216486895 new -1
;@ stdout: i64 max       81985529216486895, -81985529216486896: old 81985529216486895 new 81985529216486895
;@ stdout: i64 min       81985529216486895, -81985529216486896: old 81985529216486895 new -81985529216486896
;@ stdout: i64 umax      81985529216486895, -81985529216486896: old 81985529216486895 new -81985529216486896
;@ stdout: i64 umin      81985529216486895, -81985529216486896: old 81985529216486895 new 81985529216486895
;@ stdout: i64 uinc_wrap 81985529216486895, -81985529216486896: old 81985529216486895 new 81985529216486896
;@ stdout: i64 udec_wrap 81985529216486895, -81985529216486896: old 81985529216486895 new 81985529216486894
;@ stdout: double fadd: old 1.50 new 3.75
;@ stdout: double fsub: old 3.75 new 1.50
;@ stdout: cmpxchg hit: old 41 success 1 now 42
;@ stdout: cmpxchg miss: old 41 success 0 now 41
;@ stdout: cmpxchg i64: old -1 success 1 now 1099511627776
;@ stdout: cmpxchg i64: old 5 success 0 now 5
;@ stdout: threads: added 240000, incremented 80000, bytes 128

@.rmw = private unnamed_addr constant [40 x i8] c"i%d %-9s %lld, %lld: old %lld new %lld\0A\00"
@.fp = private unnamed_addr constant [30 x i8] c"double %s: old %.2f new %.2f\0A\00"
@.cas = private unnamed_addr constant [42 x i8] c"cmpxchg %s: old %lld success %d now %lld\0A\00"
@.threads = private unnamed_addr constant [47 x i8] c"threads: added %lld, incremented %d, bytes %d\0A\00"
@.xchg = private unnamed_addr constant [5 x i8] c"xchg\00"
@.add = private unnamed_addr constant [4 x i8] c"add\00"
@.sub = private unnamed_addr constant [4 x i8] c"sub\00"
@.and = private unnamed_addr constant [4 x i8] c"and\00"
@.nand = private unnamed_addr constant [5 x i8] c"nand\00"
@.or = private unnamed_addr constant [3 x i8] c"or\00"
@.xor = private unnamed_addr constant [4 x i8] c"xor\00"
@.max = private unnamed_addr constant [4 x i8] c"max\00"
@.min = private unnamed_addr constant [4 x i8] c"min\00"
@.umax = private unnamed_addr constant [5 x i8] c"umax\00"
@.umin = private unnamed_addr constant [5 x i8] c"umin\00"
@.uinc_wrap = private unnamed_addr constant [10 x i8] c"uinc_wrap\00"
@.udec_wrap = private unnamed_addr constant [10 x i8] c"udec_wrap\00"
@.fadd = private unnamed_addr constant [5 x i8] c"fadd\00"
@.fsub = private unnamed_addr constant [5 x i8] c"fsub\00"
@.hit = private unnamed_addr constant [4 x i8] c"hit\00"
@.miss = private unnamed_addr constant [5 x i8] c"miss\00"
@.wide = private unnamed_addr constant [4 x i8] c"i64\00"
@.weak = private unnamed_addr constant [5 x i8] c"weak\00"
@added = global i64 0
@incremented = global i32 0
@bytes = global i8 0

declare i32 @printf(ptr, ...)
declare i32 @pthread_create(ptr, ptr, ptr, ptr)
declare i32 @pthread_join(i64, ptr)

define void @rmw8(i8 %start, i8 %x) {
  %cell = alloca i8
  %start.w = sext i8 %start to i64
  %x.w = sext i8 %x to i64
  store atomic i8 %start, ptr %cell seq_cst, align 1
  %xchg.old = atomicrmw xchg ptr %cell, i8 %x seq_cst
  %xchg.new = load atomic i8, ptr %cell seq_cst, align 1
  %xchg.old.w = sext i8 %xchg.old to i64
  %xchg.new.w = sext i8 %xchg.new to i64
  call i32 (ptr, ...) @printf(ptr @.rmw, i32 8, ptr @.xchg, i64 %start.w, i64 %x.w, i64 %xchg.old.w, i64 %xchg.new.w)
  store atomic i8 %start, ptr %cell release, align 1
  %add.old = atomicrmw add ptr %cell, i8 %x acquire
  %add.new = load atomic i8, ptr %cell acquire, align 1
  %add.old.w = sext i8 %add.old to i64
  %add.new.w = sext i8 %add.new to i64
  call i32 (ptr, ...) @printf(ptr @.rmw, i32 8, ptr @.add, i64 %start.w, i64 %x.w, i64 %add.old.w, i64 %add.new.w)
  store atomic i8 %start, ptr %cell release, align 1
  %sub.old = atomicrmw sub ptr %cell, i8 %x release
  %sub.new = load atomic i8, ptr %cell acquire, align 1
  %sub.old.w = sext i8 %sub.old to i64
  %sub.new.w = sext i8 %sub.new to i64
  call i32 (ptr, ...) @printf(ptr @.rmw, i32 8, ptr @.sub, i64 %start.w, i64 %x.w, i64 %sub.old.w, i64 %sub.new.w)
  store atomic i8 %start, ptr %cell monotonic, align 1
  %and.old = atomicrmw and ptr %cell, i8 %x monotonic
  %and.new = load atomic i8, ptr %cell monotonic, align 1
  %and.old.w = sext i8 %and.old to i64
  %and.new.w = sext i8 %and.new to i64
  call i32 (ptr, ...) @printf(ptr @.rmw, i32 8, ptr @.and, i64 %start.w, i64 %x.w, i64 %and.old.w, i64 %and.new.w)
  store atomic i8 %start, ptr %cell release, align 1
  %nand.old = atomicrmw nand ptr %cell, i8 %x acq_rel
  %nand.new = load atomic i8, ptr %cell acquire, align 1
  %nand.old.w = sext i8 %nand.old to i64
  %nand.new.w = sext i8 %nand.new to i64
  call i32 (ptr, ...) @printf(ptr @.rmw, i32 8, ptr @.nand, i64 %start.w, i64 %x.w, i64 %nand.old.w, i64 %nand.new.w)
  store atomic i8 %start, ptr %cell seq_cst, align 1
  %or.old = atomicrmw or ptr %cell, i8 %x seq_cst
  %or.new = load atomic i8, ptr %cell seq_cst, align 1
  %or.old.w = sext i8 %or.old to i64
  %or.new.w = sext i8 %or.new to i64
  call i32 (ptr, ...) @printf(ptr @.rmw, i32 8, ptr @.or, i64 %start.w, i64 %x.w, i64 %or.old.w, i64 %or.new.w)
  store atomic i8 %start, ptr %cell release, align 1
  %xor.old = atomicrmw xor ptr %cell, i8 %x acquire
  %xor.new = load atomic i8, ptr %cell acquire, align 1
  %xor.old.w = sext i8 %xor.old to i64
  %xor.new.w = sext i8 %xor.new to i64
  call i32 (ptr, ...) @printf(ptr @.rmw, i32 8, ptr @.xor, i64 %start.w, i64 %x.w, i64 %xor.old.w, i64 %xor.new.w)
  store atomic i8 %start, ptr %cell release, align 1
  %max.old = atomicrmw max ptr %cell, i8 %x release
  %max.new = load atomic i8, ptr %cell acquire, align 1
  %max.old.w = sext i8 %max.old to i64
  %max.new.w = sext i8 %max.new to i64
  call i32 (ptr, ...) @printf(ptr @.rmw, i32 8, ptr @.max, i64 %start.w, i64 %x.w, i64 %max.old.w, i64 %max.new.w)
  store atomic i8 %start, ptr %cell monotonic, align 1
  %min.old = atomicrmw min ptr %cell, i8 %x monotonic
  %min.new = load atomic i8, ptr %cell monotonic, align 1
  %min.old.w = sext i8 %min.old to i64
  %min.new.w = sext i8 %min.new to i64
  call i32 (ptr, ...) @printf(ptr @.rmw, i32 8, ptr @.min, i64 %start.w, i64 %x.w, i64 %min.old.w, i64 %min.new.w)
  store atomic i8 %start, ptr %cell release, align 1
  %umax.old = atomicrmw umax ptr %cell, i8 %x acq_rel
  %umax.new = load atomic i8, ptr %cell acquire, align 1
  %umax.old.w = sext i8 %umax.old to i64
  %umax.new.w = sext i8 %umax.new to i64
  call i32 (ptr, ...) @printf(ptr @.rmw, i32 8, ptr @.umax, i64 %start.w, i64 %x.w, i64 %umax.old.w, i64 %umax.new.w)
  store atomic i8 %start, ptr %cell seq_cst, align 1
  %umin.old = atomicrmw umin ptr %cell, i8 %x seq_cst
  %umin.new = load atomic i8, ptr %cell seq_cst, align 1
  %umin.old.w = sext i8 %umin.old to i64
  %umin.new.w = sext i8 %umin.new to i64
  call i32 (ptr, ...) @printf(ptr @.rmw, i32 8, ptr @.umin, i64 %start.w, i64 %x.w, i64 %umin.old.w, i64 %umin.new.w)
  store atomic i8 %start, ptr %cell release, align 1
  %uinc_wrap.old = atomicrmw uinc_wrap ptr %cell, i8 %x acquire
  %uinc_wrap.new = load atomic i8, ptr %cell acquire, align 1
  %uinc_wrap.old.w = sext i8 %uinc_wrap.old to i64
  %uinc_wrap.new.w = sext i8 %uinc_wrap.new to i64
  call i32 (ptr, ...) @printf(ptr @.rmw, i32 8, ptr @.uinc_wrap, i64 %start.w, i64 %x.w, i64 %uinc_wrap.old.w, i64 %uinc_wrap.new.w)
  store atomic i8 %start, ptr %cell release, align 1
  %udec_wrap.old = atomicrmw udec_wrap ptr %cell, i8 %x release
  %udec_wrap.new = load atomic i8, ptr %cell acquire, align 1
  %udec_wrap.old.w = sext i8 %udec_wrap.old to i64
  %udec_wrap.new.w = sext i8 %udec_wrap.new to i64
  call i32 (ptr, ...) @printf(ptr @.rmw, i32 8, ptr @.udec_wrap, i64 %start.w, i64 %x.w, i64 %udec_wrap.old.w, i64 %udec_wrap.new.w)
  ret void
}

define void @rmw32(i32 %start, i32 %x) {
  %cell = alloca i32
  %start.w = sext i32 %start to i64
  %x.w = sext i32 %x to i64
  store atomic i32 %start, ptr %cell seq_cst, align 4
  %xchg.old = atomicrmw xchg ptr %cell, i32 %x seq_cst
  %xchg.new = load atomic i32, ptr %cell seq_cst, align 4
  %xchg.old.w = sext i32 %xchg.old to i64
  %xchg.new.w = sext i32 %xchg.new to i64
  call i32 (ptr, ...) @printf(ptr @.rmw, i32 32, ptr @.xchg, i64 %start.w, i64 %x.w, i64 %xchg.old.w, i64 %xchg.new.w)
  store atomic i32 %start, ptr %cell release, align 4
  %add.old = atomicrmw add ptr %cell, i32 %x acquire
  %add.new = load atomic i32, ptr %cell acquire, align 4
  %add.old.w = sext i32 %add.old to i64
  %add.new.w = sext i32 %add.new to i64
  call i32 (ptr, ...) @printf(ptr @.rmw, i32 32, ptr @.add, i64 %start.w, i64 %x.w, i64 %add.old.w, i64 %add.new.w)
  store atomic i32 %start, ptr %cell release, align 4
  %sub.old = atomicrmw sub ptr %cell, i32 %x release
  %sub.new = load atomic i32, ptr %cell acquire, align 4
  %sub.old.w = sext i32 %sub.old to i64
  %sub.new.w = sext i32 %sub.new to i64
  call i32 (ptr, ...) @printf(ptr @.rmw, i32 32, ptr @.sub, i64 %start.w, i64 %x.w, i64 %sub.old.w, i64 %sub.new.w)
  store atomic i32 %start, ptr %cell monotonic, align 4
  %and.old = atomicrmw and ptr %cell, i32 %x monotonic
  %and.new = load atomic i32, ptr %cell monotonic, align 4
  %and.old.w = sext i32 %and.old to i64
  %and.new.w = sext i32 %and.new to i64
  call i32 (ptr, ...) @printf(ptr @.rmw, i32 32, ptr @.and, i64 %start.w, i64 %x.w, i64 %and.old.w, i64 %and.new.w)
  store atomic i32 %start, ptr %cell release, align 4
  %nand.old = atomicrmw nand ptr %cell, i32 %x acq_rel
  %nand.new = load atomic i32, ptr %cell acquire, align 4
  %nand.old.w = sext i32 %nand.old to i64
  %nand.new.w = sext i32 %nand.new to i64
  call i32 (ptr, ...) @printf(ptr @.rmw, i32 32, ptr @.nand, i64 %start.w, i64 %x.w, i64 %nand.old.w, i64 %nand.new.w)
  store atomic i32 %start, ptr %cell seq_cst, align 4
  %or.old = atomicrmw or ptr %cell, i32 %x seq_cst
  %or.new = load atomic i32, ptr %cell seq_cst, align 4
  %or.old.w = sext i32 %or.old to i64
  %or.new.w = sext i32 %or.new to i64
  call i32 (ptr, ...) @printf(ptr @.rmw, i32 32, ptr @.or, i64 %start.w, i64 %x.w, i64 %or.old.w, i64 %or.new.w)
  store atomic i32 %start, ptr %cell release, align 4
  %xor.old = atomicrmw xor ptr %cell, i32 %x acquire
  %xor.new = load atomic i32, ptr %cell acquire, align 4
  %xor.old.w = sext i32 %xor.old to i64
  %xor.new.w = sext i32 %xor.new to i64
  call i32 (ptr, ...) @printf(ptr @.rmw, i32 32, ptr @.xor, i64 %start.w, i64 %x.w, i64 %xor.old.w, i64 %xor.new.w)
  store atomic i32 %start, ptr %cell release, align 4
  %max.old = atomicrmw max ptr %cell, i32 %x release
  %max.new = load atomic i32, ptr %cell acquire, align 4
  %max.old.w = sext i32 %max.old to i64
  %max.new.w = sext i32 %max.new to i64
  call i32 (ptr, ...) @printf(ptr @.rmw, i32 32, ptr @.max, i64 %start.w, i64 %x.w, i64 %max.old.w, i64 %max.new.w)
  store atomic i32 %start, ptr %cell monotonic, align 4
  %min.old = atomicrmw min ptr %cell, i32 %x monotonic
  %min.new = load atomic i32, ptr %cell monotonic, align 4
  %min.old.w = sext i32 %min.old to i64
  %min.new.w = sext i32 %min.new to i64
  call i32 (ptr, ...) @printf(ptr @.rmw, i32 32, ptr @.min, i64 %start.w, i64 %x.w, i64 %min.old.w, i64 %min.new.w)
  store atomic i32 %start, ptr %cell release, align 4
  %umax.old = atomicrmw umax ptr %cell, i32 %x acq_rel
  %umax.new = load atomic i32, ptr %cell acquire, align 4
  %umax.old.w = sext i32 %umax.old to i64
  %umax.new.w = sext i32 %umax.new to i64
  call i32 (ptr, ...) @printf(ptr @.rmw, i32 32, ptr @.umax, i64 %start.w, i64 %x.w, i64 %umax.old.w, i64 %umax.new.w)
  store atomic i32 %start, ptr %cell seq_cst, align 4
  %umin.old = atomicrmw umin ptr %cell, i32 %x seq_cst
  %umin.new = load atomic i32, ptr %cell seq_cst, align 4
  %umin.old.w = sext i32 %umin.old to i64
  %umin.new.w = sext i32 %umin.new to i64
  call i32 (ptr, ...) @printf(ptr @.rmw, i32 32, ptr @.umin, i64 %start.w, i64 %x.w, i64 %umin.old.w, i64 %umin.new.w)
  store atomic i32 %start, ptr %cell release, align 4
  %uinc_wrap.old = atomicrmw uinc_wrap ptr %cell, i32 %x acquire
  %uinc_wrap.new = load atomic i32, ptr %cell acquire, align 4
  %uinc_wrap.old.w = sext i32 %uinc_wrap.old to i64
  %uinc_wrap.new.w = sext i32 %uinc_wrap.new to i64
  call i32 (ptr, ...) @printf(ptr @.rmw, i32 32, ptr @.uinc_wrap, i64 %start.w, i64 %x.w, i64 %uinc_wrap.old.w, i64 %uinc_wrap.new.w)
  store atomic i32 %start, ptr %cell release, align 4
  %udec_wrap.old = atomicrmw udec_wrap ptr %cell, i32 %x release
  %udec_wrap.new = load atomic i32, ptr %cell acquire, align 4
  %udec_wrap.old.w = sext i32 %udec_wrap.old to i64
  %udec_wrap.new.w = sext i32 %udec_wrap.new to i64
  call i32 (ptr, ...) @printf(ptr @.rmw, i32 32, ptr @.udec_wrap, i64 %start.w, i64 %x.w, i64 %udec_wrap.old.w, i64 %udec_wrap.new.w)
  ret void
}

define void @rmw64(i64 %start, i64 %x) {
  %cell = alloca i64
  store atomic i64 %start, ptr %cell seq_cst, align 8
  %xchg.old = atomicrmw xchg ptr %cell, i64 %x seq_cst
  %xchg.new = load atomic i64, ptr %cell seq_cst, align 8
  call i32 (ptr, ...) @printf(ptr @.rmw, i32 64, ptr @.xchg, i64 %start, i64 %x, i64 %xchg.old, i64 %xchg.new)
  store atomic i64 %start, ptr %cell release, align 8
  %add.old = atomicrmw add ptr %cell, i64 %x acquire
  %add.new = load atomic i64, ptr %cell acquire, align 8
  call i32 (ptr, ...) @printf(ptr @.rmw, i32 64, ptr @.add, i64 %start, i64 %x, i64 %add.old, i64 %add.new)
  store atomic i64 %start, ptr %cell release, align 8
  %sub.old = atomicrmw sub ptr %cell, i64 %x release
  %sub.new = load atomic i64, ptr %cell acquire, align 8
  call i32 (ptr, ...) @printf(ptr @.rmw, i32 64, ptr @.sub, i64 %start, i64 %x, i64 %sub.old, i64 %sub.new)
  store atomic i64 %start, ptr %cell monotonic, align 8
  %and.old = atomicrmw and ptr %cell, i64 %x monotonic
  %and.new = load atomic i64, ptr %cell monotonic, align 8
  call i32 (ptr, ...) @printf(ptr @.rmw, i32 64, ptr @.and, i64 %start, i64 %x, i64 %and.old, i64 %and.new)
  store atomic i64 %start, ptr %cell release, align 8
  %nand.old = atomicrmw nand ptr %cell, i64 %x acq_rel
  %nand.new = load atomic i64, ptr %cell acquire, align 8
  call i32 (ptr, ...) @printf(ptr @.rmw, i32 64, ptr @.nand, i64 %start, i64 %x, i64 %nand.old, i64 %nand.new)
  store atomic i64 %start, ptr %cell seq_cst, align 8
  %or.old = atomicrmw or ptr %cell, i64 %x seq_cst
  %or.new = load atomic i64, ptr %cell seq_cst, align 8
  call i32 (ptr, ...) @printf(ptr @.rmw, i32 64, ptr @.or, i64 %start, i64 %x, i64 %or.old, i64 %or.new)
  store atomic i64 %start, ptr %cell release, align 8
  %xor.old = atomicrmw xor ptr %cell, i64 %x acquire
  %xor.new = load atomic i64, ptr %cell acquire, align 8
  call i32 (ptr, ...) @printf(ptr @.rmw, i32 64, ptr @.xor, i64 %start, i64 %x, i64 %xor.old, i64 %xor.new)
  store atomic i64 %start, ptr %cell release, align 8
  %max.old = atomicrmw max ptr %cell, i64 %x release
  %max.new = load atomic i64, ptr %cell acquire, align 8
  call i32 (ptr, ...) @printf(ptr @.rmw, i32 64, ptr @.max, i64 %start, i64 %x, i64 %max.old, i64 %max.new)
  store atomic i64 %start, ptr %cell monotonic, align 8
  %min.old = atomicrmw min ptr %cell, i64 %x monotonic
  %min.new = load atomic i64, ptr %cell monotonic, align 8
  call i32 (ptr, ...) @printf(ptr @.rmw, i32 64, ptr @.min, i64 %start, i64 %x, i64 %min.old, i64 %min.new)
  store atomic i64 %start, ptr %cell release, align 8
  %umax.old = atomicrmw umax ptr %cell, i64 %x acq_rel
  %umax.new = load atomic i64, ptr %cell acquire, align 8
  call i32 (ptr, ...) @printf(ptr @.rmw, i32 64, ptr @.umax, i64 %start, i64 %x, i64 %umax.old, i64 %umax.new)
  store atomic i64 %start, ptr %cell seq_cst, align 8
  %umin.old = atomicrmw umin ptr %cell, i64 %x seq_cst
  %umin.new = load atomic i64, ptr %cell seq_cst, align 8
  call i32 (ptr, ...) @printf(ptr @.rmw, i32 64, ptr @.umin, i64 %start, i64 %x, i64 %umin.old, i64 %umin.new)
  store atomic i64 %start, ptr %cell release, align 8
  %uinc_wrap.old = atomicrmw uinc_wrap ptr %cell, i64 %x acquire
  %uinc_wrap.new = load atomic i64, ptr %cell acquire, align 8
  call i32 (ptr, ...) @printf(ptr @.rmw, i32 64, ptr @.uinc_wrap, i64 %start, i64 %x, i64 %uinc_wrap.old, i64 %uinc_wrap.new)
  store atomic i64 %start, ptr %cell release, align 8
  %udec_wrap.old = atomicrmw udec_wrap ptr %cell, i64 %x release
  %udec_wrap.new = load atomic i64, ptr %cell acquire, align 8
  call i32 (ptr, ...) @printf(ptr @.rmw, i32 64, ptr @.udec_wrap, i64 %start, i64 %x, i64 %udec_wrap.old, i64 %udec_wrap.new)
  ret void
}

define void @fp(double %start, double %x) {
  %cell = alloca double
  store double %start, ptr %cell
  %add.old = atomicrmw fadd ptr %cell, double %x seq_cst
  %add.new = load atomic double, ptr %cell seq_cst, align 8
  call i32 (ptr, ...) @printf(ptr @.fp, ptr @.fadd, double %add.old, double %add.new)
  %sub.old = atomicrmw fsub ptr %cell, double %x monotonic
  %sub.new = load atomic double, ptr %cell monotonic, align 8
  call i32 (ptr, ...) @printf(ptr @.fp, ptr @.fsub, double %sub.old, double %sub.new)
  ret void
}

define void @cas(ptr %label, i32 %start, i32 %expected, i32 %new) {
  %cell = alloca i32
  store i32 %start, ptr %cell
  %r = cmpxchg ptr %cell, i32 %expected, i32 %new seq_cst seq_cst
  %old = extractvalue { i32, i1 } %r, 0
  %ok = extractvalue { i32, i1 } %r, 1
  fence seq_cst
  %now = load i32, ptr %cell
  %old.w = sext i32 %old to i64
  %now.w = sext i32 %now to i64
  %ok.i = zext i1 %ok to i32
  call i32 (ptr, ...) @printf(ptr @.cas, ptr %label, i64 %old.w, i32 %ok.i, i64 %now.w)
  ret void
}

define void @cas64(i64 %start, i64 %expected, i64 %new) {
  %cell = alloca i64
  store i64 %start, ptr %cell
  %r = cmpxchg ptr %cell, i64 %expected, i64 %new acq_rel acquire
  %old = extractvalue { i64, i1 } %r, 0
  %ok = extractvalue { i64, i1 } %r, 1
  fence acquire
  %now = load i64, ptr %cell
  %ok.i = zext i1 %ok to i32
  call i32 (ptr, ...) @printf(ptr @.cas, ptr @.wide, i64 %old, i32 %ok.i, i64 %now)
  ret void
}

; Increments the cell with a weak compare-and-exchange, retrying while it
; fails (a weak exchange may fail spuriously).
define void @increment(ptr %cell) {
entry:
  %first = load atomic i32, ptr %cell monotonic, align 4
  br label %retry
retry:
  %seen = phi i32 [ %first, %entry ], [ %old, %retry ]
  %next = add i32 %seen, 1
  %r = cmpxchg weak ptr %cell, i32 %seen, i32 %next seq_cst monotonic
  %old = extractvalue { i32, i1 } %r, 0
  %ok = extractvalue { i32, i1 } %r, 1
  br i1 %ok, label %done, label %retry
done:
  ret void
}

; The body of each thread: 20000 rounds of an atomic add, a
; compare-and-exchange increment and a byte-sized add.
define ptr @worker(ptr %arg) {
entry:
  br label %loop
loop:
  %i = phi i32 [ 0, %entry ], [ %i.next, %loop ]
  atomicrmw add ptr @added, i64 3 seq_cst
  call void @increment(ptr @incremented)
  atomicrmw add ptr @bytes, i8 1 monotonic
  %i.next = add i32 %i, 1
  %done = icmp eq i32 %i.next, 20000
  br i1 %done, label %exit, label %loop
exit:
  ret ptr null
}

define i32 @main() {
entry:
  %ids = alloca [4 x i64]
  call void @rmw8(i8 100, i8 27)
  call void @rmw8(i8 -128, i8 1)
  call void @rmw8(i8 5, i8 5)
  call void @rmw8(i8 0, i8 -1)
  call void @rmw32(i32 1000, i32 -1)
  call void @rmw32(i32 -7, i32 7)
  call void @rmw32(i32 123456, i32 65535)
  call void @rmw32(i32 0, i32 0)
  call void @rmw64(i64 1099511627776, i64 3)
  call void @rmw64(i64 -1, i64 1)
  call void @rmw64(i64 81985529216486895, i64 -81985529216486896)
  call void @fp(double 1.5, double 2.25)
  call void @cas(ptr @.hit, i32 41, i32 41, i32 42)
  call void @cas(ptr @.miss, i32 41, i32 40, i32 42)
  call void @cas64(i64 -1, i64 -1, i64 1099511627776)
  call void @cas64(i64 5, i64 -1, i64 1099511627776)
  br label %start
start:
  %t = phi i64 [ 0, %entry ], [ %t.next, %start ]
  %id = getelementptr inbounds [4 x i64], ptr %ids, i64 0, i64 %t
  call i32 @pthread_create(ptr %id, ptr null, ptr @worker, ptr null)
  %t.next = add i64 %t, 1
  %started = icmp eq i64 %t.next, 4
  br i1 %started, label %join, label %start
join:
  %j = phi i64 [ 0, %start ], [ %j.next, %join ]
  %slot = getelementptr inbounds [4 x i64], ptr %ids, i64 0, i64 %j
  %tid = load i64, ptr %slot
  call i32 @pthread_join(i64 %tid, ptr null)
  %j.next = add i64 %j, 1
  %joined = icmp eq i64 %j.next, 4
  br i1 %joined, label %report, label %join
report:
  %added = load atomic i64, ptr @added seq_cst, align 8
  %incremented = load atomic i32, ptr @incremented seq_cst, align 4
  %bytes = load atomic i8, ptr @bytes seq_cst, align 1
  %bytes.w = zext i8 %bytes to i32
  call i32 (ptr, ...) @printf(ptr @.threads, i64 %added, i32 %incremented, i32 %bytes.w)
  ret i32 0
}
