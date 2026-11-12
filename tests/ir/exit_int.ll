; Integer arithmetic, shifts, compares, casts and select at i8 to i64.
; The program makes no call: what it computes is checked against values
; derived independently of any compiler (Python, by LLVM's semantics),
; and the exit status says which check failed (100 + its number), or
; that all passed (7). The operands are volatile loads of globals, so
; nothing is folded.
;@ covers: spec/ir.md §5.4 integer (call-free; WP-043)
;@ tier: quick
;@ unsupported(x86_64): *
;@ exit: 7

@a8 = global i8 -77
@b8 = global i8 13
@s8 = global i8 3
@a16 = global i16 -30000
@b16 = global i16 123
@s16 = global i16 3
@a32 = global i32 -2000000011
@b32 = global i32 65537
@s32 = global i32 3
@a64 = global i64 -9000000000000000777
@b64 = global i64 1000000007
@s64 = global i64 3

define i32 @main() {
entry:
  %a8 = load volatile i8, ptr @a8
  %b8 = load volatile i8, ptr @b8
  %s8 = load volatile i8, ptr @s8
  %a16 = load volatile i16, ptr @a16
  %b16 = load volatile i16, ptr @b16
  %s16 = load volatile i16, ptr @s16
  %a32 = load volatile i32, ptr @a32
  %b32 = load volatile i32, ptr @b32
  %s32 = load volatile i32, ptr @s32
  %a64 = load volatile i64, ptr @a64
  %b64 = load volatile i64, ptr @b64
  %s64 = load volatile i64, ptr @s64
  %c_sel = icmp slt i64 %a64, %b64
  %r0 = add i8 %a8, %b8
  %c0 = icmp ne i8 %r0, -64
  br i1 %c0, label %fail0, label %ok0
fail0:
  ret i32 100
ok0:
  %r1 = sub i8 %a8, %b8
  %c1 = icmp ne i8 %r1, -90
  br i1 %c1, label %fail1, label %ok1
fail1:
  ret i32 101
ok1:
  %r2 = mul i8 %a8, %b8
  %c2 = icmp ne i8 %r2, 23
  br i1 %c2, label %fail2, label %ok2
fail2:
  ret i32 102
ok2:
  %r3 = and i8 %a8, %b8
  %c3 = icmp ne i8 %r3, 1
  br i1 %c3, label %fail3, label %ok3
fail3:
  ret i32 103
ok3:
  %r4 = or i8 %a8, %b8
  %c4 = icmp ne i8 %r4, -65
  br i1 %c4, label %fail4, label %ok4
fail4:
  ret i32 104
ok4:
  %r5 = xor i8 %a8, %b8
  %c5 = icmp ne i8 %r5, -66
  br i1 %c5, label %fail5, label %ok5
fail5:
  ret i32 105
ok5:
  %r6 = udiv i8 %a8, %b8
  %c6 = icmp ne i8 %r6, 13
  br i1 %c6, label %fail6, label %ok6
fail6:
  ret i32 106
ok6:
  %r7 = urem i8 %a8, %b8
  %c7 = icmp ne i8 %r7, 10
  br i1 %c7, label %fail7, label %ok7
fail7:
  ret i32 107
ok7:
  %r8 = sdiv i8 %a8, %b8
  %c8 = icmp ne i8 %r8, -5
  br i1 %c8, label %fail8, label %ok8
fail8:
  ret i32 108
ok8:
  %r9 = srem i8 %a8, %b8
  %c9 = icmp ne i8 %r9, -12
  br i1 %c9, label %fail9, label %ok9
fail9:
  ret i32 109
ok9:
  %r10 = shl i8 %a8, %s8
  %c10 = icmp ne i8 %r10, -104
  br i1 %c10, label %fail10, label %ok10
fail10:
  ret i32 110
ok10:
  %r11 = lshr i8 %a8, %s8
  %c11 = icmp ne i8 %r11, 22
  br i1 %c11, label %fail11, label %ok11
fail11:
  ret i32 111
ok11:
  %r12 = ashr i8 %a8, %s8
  %c12 = icmp ne i8 %r12, -10
  br i1 %c12, label %fail12, label %ok12
fail12:
  ret i32 112
ok12:
  %r13 = icmp eq i8 %a8, %b8
  %c13 = icmp ne i1 %r13, false
  br i1 %c13, label %fail13, label %ok13
fail13:
  ret i32 113
ok13:
  %r14 = icmp ne i8 %a8, %b8
  %c14 = icmp ne i1 %r14, true
  br i1 %c14, label %fail14, label %ok14
fail14:
  ret i32 114
ok14:
  %r15 = icmp slt i8 %a8, %b8
  %c15 = icmp ne i1 %r15, true
  br i1 %c15, label %fail15, label %ok15
fail15:
  ret i32 115
ok15:
  %r16 = icmp sle i8 %a8, %b8
  %c16 = icmp ne i1 %r16, true
  br i1 %c16, label %fail16, label %ok16
fail16:
  ret i32 116
ok16:
  %r17 = icmp sgt i8 %a8, %b8
  %c17 = icmp ne i1 %r17, false
  br i1 %c17, label %fail17, label %ok17
fail17:
  ret i32 117
ok17:
  %r18 = icmp sge i8 %a8, %b8
  %c18 = icmp ne i1 %r18, false
  br i1 %c18, label %fail18, label %ok18
fail18:
  ret i32 118
ok18:
  %r19 = icmp ult i8 %a8, %b8
  %c19 = icmp ne i1 %r19, false
  br i1 %c19, label %fail19, label %ok19
fail19:
  ret i32 119
ok19:
  %r20 = icmp ule i8 %a8, %b8
  %c20 = icmp ne i1 %r20, false
  br i1 %c20, label %fail20, label %ok20
fail20:
  ret i32 120
ok20:
  %r21 = icmp ugt i8 %a8, %b8
  %c21 = icmp ne i1 %r21, true
  br i1 %c21, label %fail21, label %ok21
fail21:
  ret i32 121
ok21:
  %r22 = icmp uge i8 %a8, %b8
  %c22 = icmp ne i1 %r22, true
  br i1 %c22, label %fail22, label %ok22
fail22:
  ret i32 122
ok22:
  %r23 = add i16 %a16, %b16
  %c23 = icmp ne i16 %r23, -29877
  br i1 %c23, label %fail23, label %ok23
fail23:
  ret i32 123
ok23:
  %r24 = sub i16 %a16, %b16
  %c24 = icmp ne i16 %r24, -30123
  br i1 %c24, label %fail24, label %ok24
fail24:
  ret i32 124
ok24:
  %r25 = mul i16 %a16, %b16
  %c25 = icmp ne i16 %r25, -19984
  br i1 %c25, label %fail25, label %ok25
fail25:
  ret i32 125
ok25:
  %r26 = and i16 %a16, %b16
  %c26 = icmp ne i16 %r26, 80
  br i1 %c26, label %fail26, label %ok26
fail26:
  ret i32 126
ok26:
  %r27 = or i16 %a16, %b16
  %c27 = icmp ne i16 %r27, -29957
  br i1 %c27, label %fail27, label %ok27
fail27:
  ret i32 127
ok27:
  %r28 = xor i16 %a16, %b16
  %c28 = icmp ne i16 %r28, -30037
  br i1 %c28, label %fail28, label %ok28
fail28:
  ret i32 128
ok28:
  %r29 = udiv i16 %a16, %b16
  %c29 = icmp ne i16 %r29, 288
  br i1 %c29, label %fail29, label %ok29
fail29:
  ret i32 129
ok29:
  %r30 = urem i16 %a16, %b16
  %c30 = icmp ne i16 %r30, 112
  br i1 %c30, label %fail30, label %ok30
fail30:
  ret i32 130
ok30:
  %r31 = sdiv i16 %a16, %b16
  %c31 = icmp ne i16 %r31, -243
  br i1 %c31, label %fail31, label %ok31
fail31:
  ret i32 131
ok31:
  %r32 = srem i16 %a16, %b16
  %c32 = icmp ne i16 %r32, -111
  br i1 %c32, label %fail32, label %ok32
fail32:
  ret i32 132
ok32:
  %r33 = shl i16 %a16, %s16
  %c33 = icmp ne i16 %r33, 22144
  br i1 %c33, label %fail33, label %ok33
fail33:
  ret i32 133
ok33:
  %r34 = lshr i16 %a16, %s16
  %c34 = icmp ne i16 %r34, 4442
  br i1 %c34, label %fail34, label %ok34
fail34:
  ret i32 134
ok34:
  %r35 = ashr i16 %a16, %s16
  %c35 = icmp ne i16 %r35, -3750
  br i1 %c35, label %fail35, label %ok35
fail35:
  ret i32 135
ok35:
  %r36 = icmp eq i16 %a16, %b16
  %c36 = icmp ne i1 %r36, false
  br i1 %c36, label %fail36, label %ok36
fail36:
  ret i32 136
ok36:
  %r37 = icmp ne i16 %a16, %b16
  %c37 = icmp ne i1 %r37, true
  br i1 %c37, label %fail37, label %ok37
fail37:
  ret i32 137
ok37:
  %r38 = icmp slt i16 %a16, %b16
  %c38 = icmp ne i1 %r38, true
  br i1 %c38, label %fail38, label %ok38
fail38:
  ret i32 138
ok38:
  %r39 = icmp sle i16 %a16, %b16
  %c39 = icmp ne i1 %r39, true
  br i1 %c39, label %fail39, label %ok39
fail39:
  ret i32 139
ok39:
  %r40 = icmp sgt i16 %a16, %b16
  %c40 = icmp ne i1 %r40, false
  br i1 %c40, label %fail40, label %ok40
fail40:
  ret i32 140
ok40:
  %r41 = icmp sge i16 %a16, %b16
  %c41 = icmp ne i1 %r41, false
  br i1 %c41, label %fail41, label %ok41
fail41:
  ret i32 141
ok41:
  %r42 = icmp ult i16 %a16, %b16
  %c42 = icmp ne i1 %r42, false
  br i1 %c42, label %fail42, label %ok42
fail42:
  ret i32 142
ok42:
  %r43 = icmp ule i16 %a16, %b16
  %c43 = icmp ne i1 %r43, false
  br i1 %c43, label %fail43, label %ok43
fail43:
  ret i32 143
ok43:
  %r44 = icmp ugt i16 %a16, %b16
  %c44 = icmp ne i1 %r44, true
  br i1 %c44, label %fail44, label %ok44
fail44:
  ret i32 144
ok44:
  %r45 = icmp uge i16 %a16, %b16
  %c45 = icmp ne i1 %r45, true
  br i1 %c45, label %fail45, label %ok45
fail45:
  ret i32 145
ok45:
  %r46 = add i32 %a32, %b32
  %c46 = icmp ne i32 %r46, -1999934474
  br i1 %c46, label %fail46, label %ok46
fail46:
  ret i32 146
ok46:
  %r47 = sub i32 %a32, %b32
  %c47 = icmp ne i32 %r47, -2000065548
  br i1 %c47, label %fail47, label %ok47
fail47:
  ret i32 147
ok47:
  %r48 = mul i32 %a32, %b32
  %c48 = icmp ne i32 %r48, -188781579
  br i1 %c48, label %fail48, label %ok48
fail48:
  ret i32 148
ok48:
  %r49 = and i32 %a32, %b32
  %c49 = icmp ne i32 %r49, 1
  br i1 %c49, label %fail49, label %ok49
fail49:
  ret i32 149
ok49:
  %r50 = or i32 %a32, %b32
  %c50 = icmp ne i32 %r50, -1999934475
  br i1 %c50, label %fail50, label %ok50
fail50:
  ret i32 150
ok50:
  %r51 = xor i32 %a32, %b32
  %c51 = icmp ne i32 %r51, -1999934476
  br i1 %c51, label %fail51, label %ok51
fail51:
  ret i32 151
ok51:
  %r52 = udiv i32 %a32, %b32
  %c52 = icmp ne i32 %r52, 35017
  br i1 %c52, label %fail52, label %ok52
fail52:
  ret i32 152
ok52:
  %r53 = urem i32 %a32, %b32
  %c53 = icmp ne i32 %r53, 58156
  br i1 %c53, label %fail53, label %ok53
fail53:
  ret i32 153
ok53:
  %r54 = sdiv i32 %a32, %b32
  %c54 = icmp ne i32 %r54, -30517
  br i1 %c54, label %fail54, label %ok54
fail54:
  ret i32 154
ok54:
  %r55 = srem i32 %a32, %b32
  %c55 = icmp ne i32 %r55, -7382
  br i1 %c55, label %fail55, label %ok55
fail55:
  ret i32 155
ok55:
  %r56 = shl i32 %a32, %s32
  %c56 = icmp ne i32 %r56, 1179869096
  br i1 %c56, label %fail56, label %ok56
fail56:
  ret i32 156
ok56:
  %r57 = lshr i32 %a32, %s32
  %c57 = icmp ne i32 %r57, 286870910
  br i1 %c57, label %fail57, label %ok57
fail57:
  ret i32 157
ok57:
  %r58 = ashr i32 %a32, %s32
  %c58 = icmp ne i32 %r58, -250000002
  br i1 %c58, label %fail58, label %ok58
fail58:
  ret i32 158
ok58:
  %r59 = icmp eq i32 %a32, %b32
  %c59 = icmp ne i1 %r59, false
  br i1 %c59, label %fail59, label %ok59
fail59:
  ret i32 159
ok59:
  %r60 = icmp ne i32 %a32, %b32
  %c60 = icmp ne i1 %r60, true
  br i1 %c60, label %fail60, label %ok60
fail60:
  ret i32 160
ok60:
  %r61 = icmp slt i32 %a32, %b32
  %c61 = icmp ne i1 %r61, true
  br i1 %c61, label %fail61, label %ok61
fail61:
  ret i32 161
ok61:
  %r62 = icmp sle i32 %a32, %b32
  %c62 = icmp ne i1 %r62, true
  br i1 %c62, label %fail62, label %ok62
fail62:
  ret i32 162
ok62:
  %r63 = icmp sgt i32 %a32, %b32
  %c63 = icmp ne i1 %r63, false
  br i1 %c63, label %fail63, label %ok63
fail63:
  ret i32 163
ok63:
  %r64 = icmp sge i32 %a32, %b32
  %c64 = icmp ne i1 %r64, false
  br i1 %c64, label %fail64, label %ok64
fail64:
  ret i32 164
ok64:
  %r65 = icmp ult i32 %a32, %b32
  %c65 = icmp ne i1 %r65, false
  br i1 %c65, label %fail65, label %ok65
fail65:
  ret i32 165
ok65:
  %r66 = icmp ule i32 %a32, %b32
  %c66 = icmp ne i1 %r66, false
  br i1 %c66, label %fail66, label %ok66
fail66:
  ret i32 166
ok66:
  %r67 = icmp ugt i32 %a32, %b32
  %c67 = icmp ne i1 %r67, true
  br i1 %c67, label %fail67, label %ok67
fail67:
  ret i32 167
ok67:
  %r68 = icmp uge i32 %a32, %b32
  %c68 = icmp ne i1 %r68, true
  br i1 %c68, label %fail68, label %ok68
fail68:
  ret i32 168
ok68:
  %r69 = add i64 %a64, %b64
  %c69 = icmp ne i64 %r69, -8999999999000000770
  br i1 %c69, label %fail69, label %ok69
fail69:
  ret i32 169
ok69:
  %r70 = sub i64 %a64, %b64
  %c70 = icmp ne i64 %r70, -9000000001000000784
  br i1 %c70, label %fail70, label %ok70
fail70:
  ret i32 170
ok70:
  %r71 = mul i64 %a64, %b64
  %c71 = icmp ne i64 %r71, -621911329999630143
  br i1 %c71, label %fail71, label %ok71
fail71:
  ret i32 171
ok71:
  %r72 = and i64 %a64, %b64
  %c72 = icmp ne i64 %r72, 421185543
  br i1 %c72, label %fail72, label %ok72
fail72:
  ret i32 172
ok72:
  %r73 = or i64 %a64, %b64
  %c73 = icmp ne i64 %r73, -8999999999421186313
  br i1 %c73, label %fail73, label %ok73
fail73:
  ret i32 173
ok73:
  %r74 = xor i64 %a64, %b64
  %c74 = icmp ne i64 %r74, -8999999999842371856
  br i1 %c74, label %fail74, label %ok74
fail74:
  ret i32 174
ok74:
  %r75 = udiv i64 %a64, %b64
  %c75 = icmp ne i64 %r75, 9446744007
  br i1 %c75, label %fail75, label %ok75
fail75:
  ret i32 175
ok75:
  %r76 = urem i64 %a64, %b64
  %c76 = icmp ne i64 %r76, 582342790
  br i1 %c76, label %fail76, label %ok76
fail76:
  ret i32 176
ok76:
  %r77 = sdiv i64 %a64, %b64
  %c77 = icmp ne i64 %r77, -8999999937
  br i1 %c77, label %fail77, label %ok77
fail77:
  ret i32 177
ok77:
  %r78 = srem i64 %a64, %b64
  %c78 = icmp ne i64 %r78, -1218
  br i1 %c78, label %fail78, label %ok78
fail78:
  ret i32 178
ok78:
  %r79 = shl i64 %a64, %s64
  %c79 = icmp ne i64 %r79, 1786976294838200248
  br i1 %c79, label %fail79, label %ok79
fail79:
  ret i32 179
ok79:
  %r80 = lshr i64 %a64, %s64
  %c80 = icmp ne i64 %r80, 1180843009213693854
  br i1 %c80, label %fail80, label %ok80
fail80:
  ret i32 180
ok80:
  %r81 = ashr i64 %a64, %s64
  %c81 = icmp ne i64 %r81, -1125000000000000098
  br i1 %c81, label %fail81, label %ok81
fail81:
  ret i32 181
ok81:
  %r82 = icmp eq i64 %a64, %b64
  %c82 = icmp ne i1 %r82, false
  br i1 %c82, label %fail82, label %ok82
fail82:
  ret i32 182
ok82:
  %r83 = icmp ne i64 %a64, %b64
  %c83 = icmp ne i1 %r83, true
  br i1 %c83, label %fail83, label %ok83
fail83:
  ret i32 183
ok83:
  %r84 = icmp slt i64 %a64, %b64
  %c84 = icmp ne i1 %r84, true
  br i1 %c84, label %fail84, label %ok84
fail84:
  ret i32 184
ok84:
  %r85 = icmp sle i64 %a64, %b64
  %c85 = icmp ne i1 %r85, true
  br i1 %c85, label %fail85, label %ok85
fail85:
  ret i32 185
ok85:
  %r86 = icmp sgt i64 %a64, %b64
  %c86 = icmp ne i1 %r86, false
  br i1 %c86, label %fail86, label %ok86
fail86:
  ret i32 186
ok86:
  %r87 = icmp sge i64 %a64, %b64
  %c87 = icmp ne i1 %r87, false
  br i1 %c87, label %fail87, label %ok87
fail87:
  ret i32 187
ok87:
  %r88 = icmp ult i64 %a64, %b64
  %c88 = icmp ne i1 %r88, false
  br i1 %c88, label %fail88, label %ok88
fail88:
  ret i32 188
ok88:
  %r89 = icmp ule i64 %a64, %b64
  %c89 = icmp ne i1 %r89, false
  br i1 %c89, label %fail89, label %ok89
fail89:
  ret i32 189
ok89:
  %r90 = icmp ugt i64 %a64, %b64
  %c90 = icmp ne i1 %r90, true
  br i1 %c90, label %fail90, label %ok90
fail90:
  ret i32 190
ok90:
  %r91 = icmp uge i64 %a64, %b64
  %c91 = icmp ne i1 %r91, true
  br i1 %c91, label %fail91, label %ok91
fail91:
  ret i32 191
ok91:
  %r92 = sext i8 %a8 to i64
  %c92 = icmp ne i64 %r92, -77
  br i1 %c92, label %fail92, label %ok92
fail92:
  ret i32 192
ok92:
  %r93 = zext i8 %a8 to i64
  %c93 = icmp ne i64 %r93, 179
  br i1 %c93, label %fail93, label %ok93
fail93:
  ret i32 193
ok93:
  %r94 = sext i16 %a16 to i32
  %c94 = icmp ne i32 %r94, -30000
  br i1 %c94, label %fail94, label %ok94
fail94:
  ret i32 194
ok94:
  %r95 = zext i16 %a16 to i32
  %c95 = icmp ne i32 %r95, 35536
  br i1 %c95, label %fail95, label %ok95
fail95:
  ret i32 195
ok95:
  %r96 = trunc i64 %a64 to i16
  %c96 = icmp ne i16 %r96, -777
  br i1 %c96, label %fail96, label %ok96
fail96:
  ret i32 196
ok96:
  %r97 = trunc i32 %a32 to i8
  %c97 = icmp ne i8 %r97, -11
  br i1 %c97, label %fail97, label %ok97
fail97:
  ret i32 197
ok97:
  %r98 = sext i32 %a32 to i64
  %c98 = icmp ne i64 %r98, -2000000011
  br i1 %c98, label %fail98, label %ok98
fail98:
  ret i32 198
ok98:
  %r99 = zext i32 %a32 to i64
  %c99 = icmp ne i64 %r99, 2294967285
  br i1 %c99, label %fail99, label %ok99
fail99:
  ret i32 199
ok99:
  %r100 = select i1 %c_sel, i32 %a32, i32 %b32
  %c100 = icmp ne i32 %r100, -2000000011
  br i1 %c100, label %fail100, label %ok100
fail100:
  ret i32 200
ok100:
  ret i32 7
}
