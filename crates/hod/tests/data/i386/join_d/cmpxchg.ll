; `cmpxchg` and `extractvalue` (i386 join D): WP-316 writes the pair
; `{ <ty>, i1 }` with the value at 0 and the flag at the second field's
; offset; WP-315's general aggregate lowering must read the same layout.
; Each width (i8, i16, i32, i64, ptr), strong and weak, success and
; failure; the pair read directly (`atomic::extract`), after a store and
; a load of the pair (`agg::load`/`store`), through a `select` and a `phi`
; (`agg`), and with `extractvalue` of field 1 before field 0.
target datalayout = "e-m:e-p:32:32-p270:32:32-p271:32:32-p272:64:64-i128:128-f64:32:64-f80:32-n8:16:32-S128"
target triple = "i686-unknown-linux-gnu"

@fmt = private constant [33 x i8] c"%s w%d %lld %d | %lld %d | %lld\0A\00"
@n.i8 = private constant [3 x i8] c"i8\00"
@n.i16 = private constant [4 x i8] c"i16\00"
@n.i32 = private constant [4 x i8] c"i32\00"
@n.i64 = private constant [4 x i8] c"i64\00"
@n.ptr = private constant [4 x i8] c"ptr\00"
@g1 = global i32 1
@g2 = global i32 2
@g3 = global i32 3

declare i32 @printf(ptr, ...)

define void @report(ptr %name, i32 %weak, i64 %v0, i1 %f0, i64 %v1, i1 %f1, i64 %mem) noinline {
  %a = zext i1 %f0 to i32
  %b = zext i1 %f1 to i32
  call i32 (ptr, ...) @printf(ptr @fmt, ptr %name, i32 %weak, i64 %v0, i32 %a, i64 %v1, i32 %b, i64 %mem)
  ret void
}

; Direct extraction; then the same pair stored, loaded, selected and
; extracted again.
define void @cx_i8(i8 %init, i8 %exp, i8 %new, i1 %which) noinline {
entry:
  %p = alloca i8
  store i8 %init, ptr %p
  %r = cmpxchg ptr %p, i8 %exp, i8 %new seq_cst seq_cst
  %f = extractvalue { i8, i1 } %r, 1
  %v = extractvalue { i8, i1 } %r, 0
  %slot = alloca { i8, i1 }
  store { i8, i1 } %r, ptr %slot
  %l = load { i8, i1 }, ptr %slot
  %w = cmpxchg weak ptr %p, i8 %new, i8 %exp acquire monotonic
  %s = select i1 %which, { i8, i1 } %l, { i8, i1 } %w
  %sv = extractvalue { i8, i1 } %s, 0
  %sf = extractvalue { i8, i1 } %s, 1
  %m = load i8, ptr %p
  %v64 = sext i8 %v to i64
  %sv64 = sext i8 %sv to i64
  %m64 = sext i8 %m to i64
  %wk = zext i1 %which to i32
  call void @report(ptr @n.i8, i32 %wk, i64 %v64, i1 %f, i64 %sv64, i1 %sf, i64 %m64)
  ret void
}

define void @cx_i16(i16 %init, i16 %exp, i16 %new, i1 %which) noinline {
entry:
  %p = alloca i16
  store i16 %init, ptr %p
  br i1 %which, label %strong, label %weak
strong:
  %r1 = cmpxchg ptr %p, i16 %exp, i16 %new acq_rel acquire
  br label %join
weak:
  %r2 = cmpxchg weak ptr %p, i16 %exp, i16 %new release monotonic
  br label %join
join:
  %r = phi { i16, i1 } [ %r1, %strong ], [ %r2, %weak ]
  %f = extractvalue { i16, i1 } %r, 1
  %v = extractvalue { i16, i1 } %r, 0
  %again = cmpxchg ptr %p, i16 %new, i16 -2 seq_cst seq_cst
  %av = extractvalue { i16, i1 } %again, 0
  %af = extractvalue { i16, i1 } %again, 1
  %m = load i16, ptr %p
  %v64 = sext i16 %v to i64
  %av64 = sext i16 %av to i64
  %m64 = sext i16 %m to i64
  %wk = zext i1 %which to i32
  call void @report(ptr @n.i16, i32 %wk, i64 %v64, i1 %f, i64 %av64, i1 %af, i64 %m64)
  ret void
}

define void @cx_i32(i32 %init, i32 %exp, i32 %new, i1 %which) noinline {
entry:
  %p = alloca i32
  store i32 %init, ptr %p
  %r = cmpxchg weak ptr %p, i32 %exp, i32 %new seq_cst seq_cst
  %slot = alloca { i32, i1 }
  store { i32, i1 } %r, ptr %slot
  %l = load { i32, i1 }, ptr %slot
  %ins = insertvalue { i32, i1 } %l, i32 77, 0
  %pick = select i1 %which, { i32, i1 } %r, { i32, i1 } %ins
  %v = extractvalue { i32, i1 } %pick, 0
  %f = extractvalue { i32, i1 } %pick, 1
  %s = cmpxchg ptr %p, i32 %exp, i32 %new monotonic monotonic
  %sv = extractvalue { i32, i1 } %s, 0
  %sf = extractvalue { i32, i1 } %s, 1
  %m = load i32, ptr %p
  %v64 = sext i32 %v to i64
  %sv64 = sext i32 %sv to i64
  %m64 = sext i32 %m to i64
  %wk = zext i1 %which to i32
  call void @report(ptr @n.i32, i32 %wk, i64 %v64, i1 %f, i64 %sv64, i1 %sf, i64 %m64)
  ret void
}

define void @cx_i64(i64 %init, i64 %exp, i64 %new, i1 %which) noinline {
entry:
  %p = alloca i64, align 8
  store i64 %init, ptr %p
  %r = cmpxchg ptr %p, i64 %exp, i64 %new seq_cst seq_cst
  %f = extractvalue { i64, i1 } %r, 1
  %v = extractvalue { i64, i1 } %r, 0
  %slot = alloca { i64, i1 }
  store { i64, i1 } %r, ptr %slot
  %l = load { i64, i1 }, ptr %slot
  %w = cmpxchg weak ptr %p, i64 %new, i64 %exp acquire acquire
  %s = select i1 %which, { i64, i1 } %w, { i64, i1 } %l
  %sv = extractvalue { i64, i1 } %s, 0
  %sf = extractvalue { i64, i1 } %s, 1
  %m = load i64, ptr %p
  %wk = zext i1 %which to i32
  call void @report(ptr @n.i64, i32 %wk, i64 %v, i1 %f, i64 %sv, i1 %sf, i64 %m)
  ret void
}

define i64 @which_g(ptr %q) {
  %a = icmp eq ptr %q, @g1
  %b = icmp eq ptr %q, @g2
  %c = icmp eq ptr %q, @g3
  %a1 = select i1 %a, i64 1, i64 0
  %b1 = select i1 %b, i64 2, i64 %a1
  %c1 = select i1 %c, i64 3, i64 %b1
  ret i64 %c1
}

define void @cx_ptr(ptr %init, ptr %exp, ptr %new, i1 %which) noinline {
entry:
  %p = alloca ptr
  store ptr %init, ptr %p
  br i1 %which, label %strong, label %weak
strong:
  %r1 = cmpxchg ptr %p, ptr %exp, ptr %new seq_cst seq_cst
  br label %join
weak:
  %r2 = cmpxchg weak ptr %p, ptr %exp, ptr %new seq_cst seq_cst
  br label %join
join:
  %r = phi { ptr, i1 } [ %r1, %strong ], [ %r2, %weak ]
  %v = extractvalue { ptr, i1 } %r, 0
  %f = extractvalue { ptr, i1 } %r, 1
  %d = cmpxchg ptr %p, ptr @g3, ptr @g1 seq_cst seq_cst
  %dv = extractvalue { ptr, i1 } %d, 0
  %df = extractvalue { ptr, i1 } %d, 1
  %m = load ptr, ptr %p
  %v64 = call i64 @which_g(ptr %v)
  %dv64 = call i64 @which_g(ptr %dv)
  %m64 = call i64 @which_g(ptr %m)
  %wk = zext i1 %which to i32
  call void @report(ptr @n.ptr, i32 %wk, i64 %v64, i1 %f, i64 %dv64, i1 %df, i64 %m64)
  ret void
}

define i32 @main() {
  ; success (memory equals the expected value) and failure, each way
  call void @cx_i8(i8 5, i8 5, i8 -9, i1 true)
  call void @cx_i8(i8 5, i8 6, i8 -9, i1 false)
  call void @cx_i8(i8 -128, i8 -128, i8 127, i1 false)
  call void @cx_i16(i16 300, i16 300, i16 -7, i1 true)
  call void @cx_i16(i16 300, i16 301, i16 -7, i1 false)
  call void @cx_i16(i16 -1, i16 -1, i16 9, i1 false)
  call void @cx_i32(i32 70000, i32 70000, i32 -3, i1 true)
  call void @cx_i32(i32 70000, i32 1, i32 -3, i1 false)
  call void @cx_i32(i32 -5, i32 -5, i32 12, i1 false)
  call void @cx_i64(i64 81985529216486895, i64 81985529216486895, i64 -2, i1 true)
  call void @cx_i64(i64 81985529216486895, i64 81985529216486894, i64 -2, i1 false)
  call void @cx_i64(i64 -4294967296, i64 -4294967296, i64 4294967296, i1 false)
  call void @cx_ptr(ptr @g1, ptr @g1, ptr @g2, i1 true)
  call void @cx_ptr(ptr @g1, ptr @g3, ptr @g2, i1 false)
  call void @cx_ptr(ptr @g3, ptr @g3, ptr @g2, i1 false)
  ret i32 0
}
