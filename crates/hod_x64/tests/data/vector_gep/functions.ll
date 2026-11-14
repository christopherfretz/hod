; WP-234: `getelementptr` with vector operands (`spec/codegen.md`,
; "Vector getelementptr"). Shared by `hod_x64`'s and `hod_a64`'s
; `tests/vector_gep.rs`; the target lines are the test's. Every function
; takes its vectors from memory (or builds them) and stores its result,
; except `ret2`, `reduced_*` and `ce_ret`, which return a `<2 x ptr>` (a
; 16-byte vector register on both targets, as for a C `vector_size(16)`).

%S = type { i32, i64, [3 x i16], i8 }

@arr = global [8 x i64] zeroinitializer, align 16
; The initializers `@cv` and `@cs` are in `initializers.ll` (hod's
; build) and `initializers_by_lane.ll` (`llc`'s build).

; The two reduced modules' functions (kiln -O2, libc++ flat_multimap).
define <2 x ptr> @reduced_x64() {
  %1 = getelementptr i8, <2 x ptr> zeroinitializer, <2 x i64> <i64 24, i64 4>
  ret <2 x ptr> %1
}

define <2 x ptr> @reduced_a64() {
  %1 = getelementptr i8, <2 x ptr> zeroinitializer, <2 x i64> <i64 -24, i64 -4>
  ret <2 x ptr> %1
}

; A constant vector `getelementptr` as an operand (folded lane by lane).
define <2 x ptr> @ce_ret() {
  ret <2 x ptr> getelementptr (i64, <2 x ptr> <ptr @arr, ptr @arr>, <2 x i64> <i64 2, i64 -1>)
}

; A vector base and a scalar index.
define void @vb_si(ptr %out, ptr %in, i32 %i) {
  %v = load <2 x ptr>, ptr %in, align 8
  %r = getelementptr inbounds i32, <2 x ptr> %v, i32 %i
  store <2 x ptr> %r, ptr %out, align 8
  ret void
}

; A scalar base and a vector index of `i16`, over a 24-byte struct.
define void @sb_vi(ptr %out, ptr %p, ptr %ix) {
  %i = load <4 x i16>, ptr %ix, align 2
  %r = getelementptr %S, ptr %p, <4 x i16> %i
  store <4 x ptr> %r, ptr %out, align 8
  ret void
}

; Eight lanes, `i8` indices, a 3-byte element.
define void @vv8(ptr %out, ptr %in, ptr %ix) {
  %v = load <8 x ptr>, ptr %in, align 8
  %i = load <8 x i8>, ptr %ix, align 1
  %r = getelementptr nusw [3 x i8], <8 x ptr> %v, <8 x i8> %i
  store <8 x ptr> %r, ptr %out, align 8
  ret void
}

; Three lanes and several indices: an array stride, a struct stride, a
; struct field, an element stride.
define void @multi3(ptr %out, ptr %in, ptr %ix, ptr %jx, ptr %kx) {
  %v = load <3 x ptr>, ptr %in, align 8
  %i = load <3 x i64>, ptr %ix, align 8
  %j = load <3 x i32>, ptr %jx, align 4
  %k = load <3 x i64>, ptr %kx, align 8
  %r = getelementptr [5 x %S], <3 x ptr> %v, <3 x i64> %i, <3 x i32> %j, i32 2, <3 x i64> %k
  store <3 x ptr> %r, ptr %out, align 8
  ret void
}

; A struct field index written as a splat.
define void @splat_field(ptr %out, ptr %in, i64 %n) {
  %v = load <2 x ptr>, ptr %in, align 8
  %r = getelementptr inbounds nuw %S, <2 x ptr> %v, i64 %n, <2 x i32> <i32 1, i32 1>
  store <2 x ptr> %r, ptr %out, align 8
  ret void
}

; Indices wider than 64 bits (truncated), scalar and vector.
define void @wide(ptr %out, ptr %in, ptr %pi, ptr %px) {
  %v = load <2 x ptr>, ptr %in, align 8
  %i = load i128, ptr %pi, align 16
  %x = load <2 x i128>, ptr %px, align 16
  %r = getelementptr [4 x i16], <2 x ptr> %v, i128 %i, <2 x i128> %x
  store <2 x ptr> %r, ptr %out, align 8
  ret void
}

; An index of an odd width (sign-extended from 33 bits).
define void @odd(ptr %out, ptr %in, i64 %n) {
  %v = load <2 x ptr>, ptr %in, align 8
  %t = trunc i64 %n to i33
  %r = getelementptr i64, <2 x ptr> %v, i33 %t
  store <2 x ptr> %r, ptr %out, align 8
  ret void
}

; A scale beyond 32 bits and a constant offset beyond 32 bits.
define void @big(ptr %out, ptr %in, ptr %ix) {
  %v = load <2 x ptr>, ptr %in, align 8
  %i = load <2 x i64>, ptr %ix, align 8
  %a = getelementptr [3000000000 x i8], <2 x ptr> %v, <2 x i64> %i
  %r = getelementptr i8, <2 x ptr> %a, i64 -5000000000
  store <2 x ptr> %r, ptr %out, align 8
  ret void
}

; One lane.
define void @one(ptr %out, ptr %p, i64 %i) {
  %v = insertelement <1 x ptr> poison, ptr %p, i32 0
  %iv = insertelement <1 x i64> poison, i64 %i, i32 0
  %r = getelementptr i64, <1 x ptr> %v, <1 x i64> %iv
  store <1 x ptr> %r, ptr %out, align 8
  ret void
}

; The result read by `extractelement`.
define ptr @extract(ptr %p, i64 %a, i64 %b) {
  %i0 = insertelement <2 x i64> poison, i64 %a, i32 0
  %i = insertelement <2 x i64> %i0, i64 %b, i32 1
  %r = getelementptr i32, ptr %p, <2 x i64> %i
  %e = extractelement <2 x ptr> %r, i32 1
  ret ptr %e
}

; The result passed by value, and a callee's returned.
define internal <2 x ptr> @bump(<2 x ptr> %v, <2 x i64> %d) noinline {
  %r = getelementptr i8, <2 x ptr> %v, <2 x i64> %d
  ret <2 x ptr> %r
}

define void @byval(ptr %out, ptr %in, ptr %ix) {
  %v = load <2 x ptr>, ptr %in, align 8
  %d = load <2 x i64>, ptr %ix, align 8
  %a = getelementptr i64, <2 x ptr> %v, i64 1
  %r = call <2 x ptr> @bump(<2 x ptr> %a, <2 x i64> %d)
  store <2 x ptr> %r, ptr %out, align 8
  ret void
}

; Constant operands: a global as the scalar base, a constant vector
; index with a variable base, a constant vector base (a global and
; `null`) with a variable scalar index.
define void @consts(ptr %out, i64 %i) {
  %v = insertelement <2 x i64> <i64 0, i64 5>, i64 %i, i32 0
  %a = getelementptr i64, ptr @arr, <2 x i64> %v
  store <2 x ptr> %a, ptr %out, align 8
  %b = getelementptr i32, <2 x ptr> %a, <2 x i64> <i64 1, i64 -1>
  %o2 = getelementptr ptr, ptr %out, i64 2
  store <2 x ptr> %b, ptr %o2, align 8
  %c = getelementptr i8, <2 x ptr> <ptr @arr, ptr null>, i64 %i
  %o4 = getelementptr ptr, ptr %out, i64 4
  store <2 x ptr> %c, ptr %o4, align 8
  %d = getelementptr i16, <4 x ptr> <ptr null, ptr @arr, ptr null, ptr @arr>, <4 x i32> <i32 3, i32 -3, i32 0, i32 1>
  %o6 = getelementptr ptr, ptr %out, i64 6
  %d2 = shufflevector <4 x ptr> %d, <4 x ptr> poison, <2 x i32> <i32 0, i32 1>
  store <2 x ptr> %d2, ptr %o6, align 8
  ret void
}

; Returned to C: a constant scalar index, then a vector one.
define <2 x ptr> @ret2(ptr %p, i64 %a, i64 %b) {
  %i0 = insertelement <2 x i64> poison, i64 %a, i32 0
  %i = insertelement <2 x i64> %i0, i64 %b, i32 1
  %r = getelementptr [4 x i64], ptr %p, i64 1, <2 x i64> %i
  ret <2 x ptr> %r
}
