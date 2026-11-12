; Every constant form (spec/ll.md §5.3 to §5.5).

%s = type { i8, i32, i8, i64, i16 }
%packed = type <{ i8, i32, i16 }>
%nested = type { [2 x %s], %packed, { i8, { i16, [3 x i8] } } }

; Integers.
@i_true = global i1 true
@i_false = global i1 false
@i_neg = global i32 -1
@i_min = global i64 -9223372036854775808
@i_max = global i64 18446744073709551615
@i_wrap = global i8 256
@i_wrap_neg = global i8 -129
@i_hex_u = global i32 u0x1F
@i_hex_s = global i32 s0xFF
@i_hex_s_wide = global i128 s0x8000000000000000
@i_hex_u_wide = global i128 u0x123456789ABCDEF0123456789ABCDEF0
@i_hex_s_small = global i8 s0x7
@i_128 = global i128 170141183460469231731687303715884105727
@i_128_neg = global i128 -170141183460469231731687303715884105728
@i_200 = global i200 -1234567890123456789012345678901234567890123456789
@i_200_big = global i200 1606938044258990275541962092341162602522202993782792835301375
@i_65 = global i65 36893488147419103231
@i_7 = global i7 -3
@i_24 = global i24 16777215
@i_long = global i64 00000000000000000000000000000000000000000042

; Floating point.
@f_dec = global double 1.5
@f_neg_zero = global double -0.0
@f_plus = global double +2.5
@f_exp = global double 1.5e+10
@f_exp_neg = global double 25.0E-2
@f_no_frac = global double 2.e3
@f_inexact = global double 0.1
@f_hex = global double 0x400921FB54442D18
@f_hex_nan = global double 0x7FF8000000001234
@f_hex_snan = global double 0x7FF4000000000000
@f_inf = global double 0x7FF0000000000000
@f_float = global float 1.5
@f_float_hex = global float 0x3FB99999A0000000
@f_float_neg = global float -0.0
@f_float_inf = global float 0xFFF0000000000000
@f_float_nan = global float 0x7FF8000000000000
@f_float_snan = global float 0x7FF4000000000000
@f_float_denorm = global float 0x36A0000000000000
@f_half = global half 1.5
@f_half_h = global half 0xH3C00
@f_half_hex = global half 0x3FF8000000000000
@f_half_nan = global half 0xH7E01
@f_bfloat = global bfloat 0xR3F80
@f_bfloat_dec = global bfloat 2.0
@f_fp80 = global x86_fp80 0xK3FFF8000000000000000
@f_fp80_neg = global x86_fp80 0xKBFFEC000000000000000
@f_fp80_short = global x86_fp80 0xK1
@f_fp80_pseudo = global x86_fp80 0xK00008000000000000000
@f_fp80_unnormal = global x86_fp80 0xK40000000000000000001
@f_fp128 = global fp128 0xL00000000000000003FFF000000000000
@f_fp128_short = global fp128 0xL3FFF
@f_fp128_full = global fp128 0xL0123456789ABCDEFC000FEDCBA987654

; The simple forms.
@z_ptr = global ptr null
@z_struct = global %s zeroinitializer
@z_array = global [10 x i16] zeroinitializer
@z_undef = global i32 undef
@z_poison = global i32 poison
@z_undef_struct = global %s undef
@z_mixed = global %s { i8 1, i32 undef, i8 poison, i64 4, i16 5 }

; Aggregates.
@a_struct = global %s { i8 1, i32 2, i8 3, i64 4, i16 5 }
@a_packed = global %packed <{ i8 1, i32 2, i16 3 }>
@a_nested = global %nested { [2 x %s] [%s { i8 1, i32 2, i8 3, i64 4, i16 5 }, %s zeroinitializer], %packed <{ i8 6, i32 7, i16 8 }>, { i8, { i16, [3 x i8] } } { i8 9, { i16, [3 x i8] } { i16 10, [3 x i8] c"xyz" } } }
@a_literal = global { i8, [3 x i16], <{ i8, i64 }> } { i8 1, [3 x i16] [i16 2, i16 3, i16 4], <{ i8, i64 }> <{ i8 5, i64 6 }> }
@a_array2 = global [2 x [3 x i8]] [[3 x i8] c"abc", [3 x i8] [i8 1, i8 2, i8 3]]
@a_empty_array = global [0 x i32] []
@a_empty_struct = global {} {}
@a_string = constant [11 x i8] c"a\22b\\c\00\01\FF\0A\7f\5C"
@a_string_low = constant [4 x i8] c"\aa\bB\Cc\dD"
@a_vector = global <4 x i32> <i32 1, i32 2, i32 3, i32 4>
@a_vector_f = global <2 x double> <double 1.0, double -2.0>
@a_vector_bits = global <8 x i1> <i1 true, i1 false, i1 true, i1 true, i1 false, i1 false, i1 false, i1 true>
@a_vector_3 = global <3 x i8> <i8 1, i8 2, i8 3>
@a_splat = global <4 x i32> splat (i32 7)
@a_splat_f = global <2 x float> splat (float 1.5)
@a_vector_zero = global <4 x i64> zeroinitializer
@a_wide_elems = global [2 x i128] [i128 1, i128 -1]
@a_floats = global { float, double, half, fp128 } { float 1.0, double 2.0, half 3.0, fp128 0xL00000000000000004000000000000000 }

; Addresses.
@target = global [16 x i32] zeroinitializer
@other = global %s zeroinitializer
@extern = external global [0 x i8]
declare void @callee()
@p_plain = global ptr @target
@p_fn = global ptr @callee
@p_gep = global ptr getelementptr inbounds ([16 x i32], ptr @target, i64 0, i64 5)
@p_gep_flags = global ptr getelementptr inbounds nuw (i8, ptr @target, i64 7)
@p_gep_nusw = global ptr getelementptr nusw (i32, ptr @target, i64 3)
@p_gep_neg = global ptr getelementptr (i8, ptr @extern, i64 -1)
@p_gep_field = global ptr getelementptr inbounds (%s, ptr @other, i32 0, i32 3)
@p_gep_nested = global ptr getelementptr (%nested, ptr @a_nested, i64 0, i32 2, i32 1, i32 1, i64 2)
@p_gep_inrange = global ptr getelementptr inbounds inrange(-8, 24) ([16 x i32], ptr @target, i64 0, i64 2)
@p_gep_of_gep = global ptr getelementptr (i8, ptr getelementptr (i32, ptr @target, i64 2), i64 3)
@p_gep_i32_index = global ptr getelementptr ([16 x i32], ptr @target, i32 1, i32 -1)
@p_gep_big = global ptr getelementptr (i8, ptr @extern, i64 1000000000000)
@p_int = global i64 ptrtoint (ptr @target to i64)
@p_int_add = global i64 add (i64 ptrtoint (ptr @target to i64), i64 12)
@p_int_sub = global i64 sub (i64 ptrtoint (ptr @target to i64), i64 4)
@p_int_32 = global i32 trunc (i64 ptrtoint (ptr @target to i64) to i32)
@p_abs = global ptr inttoptr (i64 4096 to ptr)
@p_abs_zero = global ptr inttoptr (i64 0 to ptr)
@p_roundtrip = global ptr inttoptr (i64 ptrtoint (ptr @target to i64) to ptr)
@p_bitcast = global ptr bitcast (ptr @target to ptr)
@p_diff = global i64 sub (i64 ptrtoint (ptr getelementptr (i32, ptr @target, i64 9) to i64), i64 ptrtoint (ptr @target to i64))
@p_diff_self = global i32 trunc (i64 sub (i64 ptrtoint (ptr @callee to i64), i64 ptrtoint (ptr @p_diff_self to i64)) to i32)
@p_diff_table = constant [3 x i32] [
  i32 trunc (i64 sub (i64 ptrtoint (ptr @target to i64), i64 ptrtoint (ptr @p_diff_table to i64)) to i32),
  i32 trunc (i64 sub (i64 ptrtoint (ptr @other to i64), i64 ptrtoint (ptr @p_diff_table to i64)) to i32),
  i32 trunc (i64 sub (i64 ptrtoint (ptr getelementptr (i8, ptr @extern, i64 3) to i64), i64 ptrtoint (ptr getelementptr inbounds ([3 x i32], ptr @p_diff_table, i64 0, i64 2) to i64)) to i32)
]
@p_dso = global i32 trunc (i64 sub (i64 ptrtoint (ptr dso_local_equivalent @callee to i64), i64 ptrtoint (ptr @p_dso to i64)) to i32)
@p_in_struct = global { i8, ptr, i16, ptr } { i8 1, ptr @target, i16 2, ptr getelementptr (i8, ptr @other, i64 1) }
@p_vector = global <2 x ptr> <ptr @target, ptr @other>
@p_xor = global i64 xor (i64 5, i64 3)
@p_folded = global i32 add (i32 trunc (i64 4294967297 to i32), i32 sub (i32 10, i32 3))
@p_nuw = global i64 add nuw nsw (i64 1, i64 2)

define ptr @blocks(i1 %c) {
entry:
  br i1 %c, label %yes, label %no

yes:
  ret ptr blockaddress(@blocks, %no)

no:
  indirectbr ptr blockaddress(@blocks, %yes), [label %yes]
}
@p_block = global ptr blockaddress(@blocks, %no)
@p_block_table = constant [2 x ptr] [ptr blockaddress(@blocks, %yes), ptr blockaddress(@blocks, %no)]

; Two variables of one section: the assembler knows their distance.
@pair_a = global i64 7, section "pair"
@pair_b = global i32 trunc (i64 sub (i64 ptrtoint (ptr @pair_a to i64), i64 ptrtoint (ptr @pair_b to i64)) to i32), section "pair"
@pair_c = internal global i32 trunc (i64 sub (i64 ptrtoint (ptr getelementptr (i8, ptr @pair_b, i64 2) to i64), i64 ptrtoint (ptr @pair_a to i64)) to i32), section "pair"
@pair_user = global ptr @pair_c
