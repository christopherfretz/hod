//! Vectors (`spec/interp.md` §3.1, §4, §5.10, §8), with results worked
//! out by hand. The comparison with LLVM is `tests/vector_oracle.rs`.

#![allow(clippy::many_single_char_names)]

mod vector_text;

use hod_interp::{Stop, Trap, Val};
use vector_text::{lanes, module, parse};

/// §3.1, §4: byte lanes at multiples of their size; a `<3 x i32>` stores
/// 12 bytes; masks are bit-packed, lane 0 in bit 0, the bits above the
/// lanes zero; bitcasts are the image.
#[test]
fn layout_in_memory() {
    let t = parse(&module(
        "define i64 @three(<3 x i32> %v) {
           %p = alloca [2 x i64], align 16
           store i64 -1, ptr %p
           %q = getelementptr i8, ptr %p, i64 8
           store i64 -1, ptr %q
           store <3 x i32> %v, ptr %p
           %w = load i64, ptr %q
           ret i64 %w
         }
         define i16 @mask(<13 x i8> %a, <13 x i8> %b) {
           %c = icmp ult <13 x i8> %a, %b
           %p = alloca i16
           store i16 -1, ptr %p
           store <13 x i1> %c, ptr %p
           %r = load i16, ptr %p
           ret i16 %r
         }
         define <16 x i1> @unpack(i16 %x) {
           %v = bitcast i16 %x to <16 x i1>
           ret <16 x i1> %v
         }
         define i64 @pack(<8 x i8> %x) {
           %c = icmp ne <8 x i8> %x, zeroinitializer
           %b = bitcast <8 x i1> %c to i8
           %w = zext i8 %b to i64
           ret i64 %w
         }
         define <4 x i16> @reinterpret(<2 x i32> %x) {
           %v = bitcast <2 x i32> %x to <4 x i16>
           ret <4 x i16> %v
         }
         define i8 @dirty_mask() {
           %p = alloca i8
           store i8 -8, ptr %p
           %v = load <3 x i1>, ptr %p
           %b = bitcast <3 x i1> %v to i3
           %w = zext i3 %b to i8
           ret i8 %w
         }",
    ));
    let types = &t.module.types;
    let ty = |name: &str| t.module.symbol(t.sym(name)).ty();
    let param = |name: &str| match types.kind(ty(name)) {
        hod_ir::TypeKind::Function { params, .. } => params.to_vec(),
        _ => unreachable!(),
    };
    let v3 = hod_interp::Vector::from_lanes(
        types,
        param("three")[0],
        &[Some(Val::i32(1)), Some(Val::i32(2)), Some(Val::i32(0xaabb_ccdd))],
    )
    .unwrap();
    assert_eq!(v3.bytes(), [1, 0, 0, 0, 2, 0, 0, 0, 0xdd, 0xcc, 0xbb, 0xaa]);
    // The fourth word was not stored.
    assert_eq!(t.scalar("three", &[Val::Agg(v3)]), Val::i64(0xffff_ffff_aabb_ccdd));

    let bytes = |values: [u8; 13]| {
        hod_interp::Vector::from_image(types, param("mask")[0], &values).unwrap()
    };
    let a = bytes([0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12]);
    let b = bytes([1, 1, 3, 0, 5, 0, 7, 0, 9, 0, 11, 0, 13]);
    // Lanes 0, 2, 4, ..., 12 are below; bits 13..15 are zero.
    assert_eq!(t.scalar("mask", &[Val::Agg(a), Val::Agg(b)]), Val::i16(0x1555));

    let v = t.vector("unpack", &[Val::i16(0x8001)]);
    assert_eq!(lanes(&v), "1 0 0 0 0 0 0 0 0 0 0 0 0 0 0 1");
    let x =
        hod_interp::Vector::from_image(types, param("pack")[0], &[1, 0, 0, 1, 0, 0, 0, 9]).unwrap();
    assert_eq!(t.scalar("pack", &[Val::Agg(x)]), Val::i64(0x89));
    let x =
        hod_interp::Vector::from_image(types, param("reinterpret")[0], &[1, 2, 3, 4, 5, 6, 7, 8])
            .unwrap();
    assert_eq!(lanes(&t.vector("reinterpret", &[Val::Agg(x)])), "513 1027 1541 2055");
    // Bits above the lanes of a mask in memory are as for an `i3`.
    assert_eq!(t.trap("dirty_mask", &[]), Trap::NonZeroPadding);
}

/// §5.10: element-wise integer arithmetic wraps per lane; a shift by the
/// width or more is a poison lane, which is observed only where a scalar
/// is made of it.
#[test]
fn integer_lanes_and_poison() {
    let t = parse(&module(
        "define <4 x i8> @ops(<4 x i8> %a, <4 x i8> %b) {
           %s = add <4 x i8> %a, %b
           %m = mul <4 x i8> %s, <i8 3, i8 3, i8 3, i8 3>
           %x = xor <4 x i8> %m, %a
           ret <4 x i8> %x
         }
         define <4 x i8> @shift(<4 x i8> %a, <4 x i8> %b) {
           %s = shl <4 x i8> %a, %b
           ret <4 x i8> %s
         }
         define i8 @lane(<4 x i8> %a, <4 x i8> %b, i32 %i) {
           %s = lshr <4 x i8> %a, %b
           %e = extractelement <4 x i8> %s, i32 %i
           ret i8 %e
         }
         define <4 x i8> @div(<4 x i8> %a, <4 x i8> %b) {
           %s = sdiv <4 x i8> %a, %b
           ret <4 x i8> %s
         }
         define <4 x i8> @rem_of_poison(<4 x i8> %a, <4 x i8> %b) {
           %s = shl <4 x i8> %b, <i8 0, i8 9, i8 0, i8 0>
           %r = urem <4 x i8> %a, %s
           ret <4 x i8> %r
         }
         define <4 x i8> @div_poison_dividend(<4 x i8> %a, <4 x i8> %b) {
           %s = shl <4 x i8> %a, <i8 0, i8 9, i8 0, i8 0>
           %r = udiv <4 x i8> %s, %b
           ret <4 x i8> %r
         }",
    ));
    let v4 = match t.module.types.kind(t.module.symbol(t.sym("ops")).ty()) {
        hod_ir::TypeKind::Function { params, .. } => params[0],
        _ => unreachable!(),
    };
    let mk = |values: [u8; 4]| {
        Val::Agg(hod_interp::Vector::from_image(&t.module.types, v4, &values).unwrap())
    };
    // (a + b) * 3 ^ a per lane, modulo 256: 9 ^ 1, 132 ^ 200, 0 ^ 255, 0 ^ 128.
    let got = t.vector("ops", &[mk([1, 200, 255, 128]), mk([2, 100, 1, 128])]);
    assert_eq!(lanes(&got), "8 76 255 128");
    let got = t.vector("shift", &[mk([1, 1, 3, 0x81]), mk([7, 8, 1, 255])]);
    assert_eq!(lanes(&got), "128 _ 6 _");
    let a = mk([0x80, 0x80, 0x80, 0x80]);
    let b = mk([7, 8, 0, 9]);
    assert_eq!(t.scalar("lane", &[a.clone(), b.clone(), Val::i32(0)]), Val::i8(1));
    assert_eq!(t.scalar("lane", &[a.clone(), b.clone(), Val::i32(2)]), Val::i8(0x80));
    assert_eq!(t.trap("lane", &[a.clone(), b.clone(), Val::i32(1)]), Trap::PoisonLane);
    // Out of range is poison too.
    assert_eq!(t.trap("lane", &[a.clone(), b, Val::i32(4)]), Trap::PoisonLane);
    // Division: undefined behavior in any lane traps.
    let got = t.vector("div", &[mk([0xf9, 7, 0x80, 9]), mk([2, 0xfe, 1, 3])]);
    assert_eq!(lanes(&got), "253 253 128 3");
    assert_eq!(t.trap("div", &[mk([1, 2, 3, 4]), mk([1, 1, 0, 1])]), Trap::DivisionByZero);
    assert_eq!(t.trap("div", &[mk([1, 2, 0x80, 4]), mk([1, 1, 0xff, 1])]), Trap::DivisionOverflow);
    // A poison divisor may be zero; a poison dividend gives a poison lane.
    assert_eq!(t.trap("rem_of_poison", &[mk([1, 2, 3, 4]), mk([1; 4])]), Trap::DivisionByZero);
    let got = t.vector("div_poison_dividend", &[mk([8, 8, 8, 8]), mk([2; 4])]);
    assert_eq!(lanes(&got), "4 _ 4 4");
}

/// §5.10: float lanes are IEEE per lane; a computed NaN traps as for a
/// scalar; `fptosi` that does not fit is a poison lane; casts per lane.
#[test]
fn float_lanes_and_casts() {
    let t = parse(&module(
        "define <2 x double> @arith(<2 x double> %a, <2 x double> %b) {
           %s = fadd <2 x double> %a, %b
           %m = fmul <2 x double> %s, <double 2.0, double -0.5>
           %n = fneg <2 x double> %m
           ret <2 x double> %n
         }
         define <4 x float> @div(<4 x float> %a, <4 x float> %b) {
           %d = fdiv <4 x float> %a, %b
           ret <4 x float> %d
         }
         define <4 x i8> @narrow(<4 x float> %a) {
           %i = fptosi <4 x float> %a to <4 x i8>
           ret <4 x i8> %i
         }
         define <4 x i32> @widen(<4 x i8> %a) {
           %s = sext <4 x i8> %a to <4 x i32>
           %z = zext <4 x i8> %a to <4 x i32>
           %d = sub <4 x i32> %z, %s
           ret <4 x i32> %d
         }
         define <2 x float> @round(<2 x double> %a) {
           %f = fptrunc <2 x double> %a to <2 x float>
           ret <2 x float> %f
         }",
    ));
    let types = &t.module.types;
    let sig = |name: &str| match types.kind(t.module.symbol(t.sym(name)).ty()) {
        hod_ir::TypeKind::Function { params, .. } => params.to_vec(),
        _ => unreachable!(),
    };
    let vec = |name: &str, at: usize, values: &[Val]| {
        let lanes: Vec<Option<Val>> = values.iter().cloned().map(Some).collect();
        Val::Agg(hod_interp::Vector::from_lanes(types, sig(name)[at], &lanes).unwrap())
    };
    let got = t.vector(
        "arith",
        &[
            vec("arith", 0, &[Val::f64(1.5), Val::f64(3.0)]),
            vec("arith", 1, &[Val::f64(0.5), Val::f64(1.0)]),
        ],
    );
    assert_eq!(lanes(&got), "-4 2");
    let fs = |v: [f32; 4]| v.map(Val::f32);
    let got = t.vector(
        "div",
        &[vec("div", 0, &fs([1.0, -1.0, 0.0, 6.0])), vec("div", 1, &fs([0.0, 0.0, 2.0, -3.0]))],
    );
    assert_eq!(lanes(&got), "inf -inf 0 -2");
    assert_eq!(
        t.trap(
            "div",
            &[vec("div", 0, &fs([1.0, 0.0, 0.0, 6.0])), vec("div", 1, &fs([1.0, 0.0, 2.0, -3.0]))]
        ),
        Trap::NanResult
    );
    let got = t.vector("narrow", &[vec("narrow", 0, &fs([-128.9, 127.5, 128.0, f32::NAN]))]);
    assert_eq!(lanes(&got), "128 127 _ _");
    let bytes = |values: [u8; 4]| {
        Val::Agg(hod_interp::Vector::from_image(types, sig("widen")[0], &values).unwrap())
    };
    assert_eq!(lanes(&t.vector("widen", &[bytes([1, 0x80, 0xff, 0x7f])])), "0 256 256 0");
    let got =
        t.vector("round", &[vec("round", 0, &[Val::f64(1.0 + f64::EPSILON), Val::f64(-1e300)])]);
    assert_eq!(lanes(&got), "1 -inf");
}

/// §5.10: compares give `<N x i1>`; `select` with a vector condition
/// picks per lane, with a scalar one the whole vector.
#[test]
fn compares_and_selects() {
    let t = parse(&module(
        "define <4 x i32> @max(<4 x i32> %a, <4 x i32> %b) {
           %c = icmp sgt <4 x i32> %a, %b
           %m = select <4 x i1> %c, <4 x i32> %a, <4 x i32> %b
           ret <4 x i32> %m
         }
         define <2 x i1> @fcmp(<2 x double> %a, <2 x double> %b) {
           %c = fcmp ult <2 x double> %a, %b
           ret <2 x i1> %c
         }
         define <2 x i64> @whole(i1 %c, <2 x i64> %a, <2 x i64> %b) {
           %m = select i1 %c, <2 x i64> %a, <2 x i64> %b
           ret <2 x i64> %m
         }
         define <4 x i32> @poison_cond(<4 x i32> %a, <4 x i32> %b) {
           %s = shl <4 x i32> %a, <i32 0, i32 40, i32 0, i32 0>
           %c = icmp sgt <4 x i32> %s, %b
           %m = select <4 x i1> %c, <4 x i32> %a, <4 x i32> %b
           ret <4 x i32> %m
         }",
    ));
    let types = &t.module.types;
    let sig = |name: &str| match types.kind(t.module.symbol(t.sym(name)).ty()) {
        hod_ir::TypeKind::Function { params, .. } => params.to_vec(),
        _ => unreachable!(),
    };
    let ints = |name: &str, at: usize, values: [i32; 4]| {
        let lanes: Vec<Option<Val>> = values.iter().map(|&v| Some(Val::i32(v as u32))).collect();
        Val::Agg(hod_interp::Vector::from_lanes(types, sig(name)[at], &lanes).unwrap())
    };
    let got =
        t.vector("max", &[ints("max", 0, [1, -5, 7, i32::MIN]), ints("max", 1, [2, -6, 7, 0])]);
    assert_eq!(lanes(&got), format!("2 {} 7 0", (-5i32) as u32));
    let d = |at: usize, x: f64, y: f64| {
        Val::Agg(
            hod_interp::Vector::from_lanes(
                types,
                sig("fcmp")[at],
                &[Some(Val::f64(x)), Some(Val::f64(y))],
            )
            .unwrap(),
        )
    };
    assert_eq!(lanes(&t.vector("fcmp", &[d(0, 1.0, f64::NAN), d(1, 0.5, 0.0)])), "0 1");
    let l = |at: usize, x: u64, y: u64| {
        Val::Agg(
            hod_interp::Vector::from_lanes(
                types,
                sig("whole")[at],
                &[Some(Val::i64(x)), Some(Val::i64(y))],
            )
            .unwrap(),
        )
    };
    assert_eq!(lanes(&t.vector("whole", &[Val::i1(false), l(1, 1, 2), l(2, 3, 4)])), "3 4");
    let got = t.vector(
        "poison_cond",
        &[ints("poison_cond", 0, [5, 5, 5, 5]), ints("poison_cond", 1, [1; 4])],
    );
    assert_eq!(lanes(&got), "5 _ 5 5");
}

/// §5.10: `insertelement` and `extractelement` with constant and variable
/// indices; past the end the inserted vector is poison; `shufflevector`
/// with `poison` entries; a load of unwritten bytes is poison per lane, so
/// that filling a vector lane by lane through memory works.
#[test]
fn element_access_and_shuffles() {
    let t = parse(&module(
        "define <4 x i16> @build(i16 %x, i32 %i) {
           %a = insertelement <4 x i16> poison, i16 %x, i32 0
           %b = insertelement <4 x i16> %a, i16 7, i64 3
           %c = insertelement <4 x i16> %b, i16 9, i32 %i
           ret <4 x i16> %c
         }
         define <8 x i8> @shuffle(<4 x i8> %a, <4 x i8> %b) {
           %s = shufflevector <4 x i8> %a, <4 x i8> %b, <8 x i32> <i32 7, i32 0, i32 poison, i32 4, i32 1, i32 1, i32 6, i32 3>
           ret <8 x i8> %s
         }
         define <2 x i8> @splat(i8 %x) {
           %a = insertelement <2 x i8> undef, i8 %x, i32 0
           %s = shufflevector <2 x i8> %a, <2 x i8> poison, <2 x i32> zeroinitializer
           ret <2 x i8> %s
         }
         define i32 @lane_by_lane(i32 %x) {
           %p = alloca <4 x i32>
           %v0 = load <4 x i32>, ptr %p
           %v1 = insertelement <4 x i32> %v0, i32 %x, i32 2
           store <4 x i32> %v1, ptr %p
           %v2 = load <4 x i32>, ptr %p
           %e = extractelement <4 x i32> %v2, i32 2
           %q = getelementptr i8, ptr %p, i64 8
           %w = load i32, ptr %q
           %r = add i32 %e, %w
           ret i32 %r
         }
         define i32 @unwritten_lane() {
           %p = alloca <4 x i32>
           %v0 = load <4 x i32>, ptr %p
           %v1 = insertelement <4 x i32> %v0, i32 1, i32 2
           store <4 x i32> %v1, ptr %p
           %w = load i32, ptr %p
           ret i32 %w
         }",
    ));
    assert_eq!(lanes(&t.vector("build", &[Val::i16(5), Val::i32(1)])), "5 9 0 7");
    assert_eq!(lanes(&t.vector("build", &[Val::i16(5), Val::i32(4)])), "_ _ _ _");
    let types = &t.module.types;
    let sig = |name: &str| match types.kind(t.module.symbol(t.sym(name)).ty()) {
        hod_ir::TypeKind::Function { params, .. } => params.to_vec(),
        _ => unreachable!(),
    };
    let v = |values: [u8; 4]| {
        Val::Agg(hod_interp::Vector::from_image(types, sig("shuffle")[0], &values).unwrap())
    };
    let got = t.vector("shuffle", &[v([1, 2, 3, 4]), v([5, 6, 7, 8])]);
    assert_eq!(lanes(&got), "8 1 _ 5 2 2 7 4");
    assert_eq!(lanes(&t.vector("splat", &[Val::i8(3)])), "3 3");
    assert_eq!(t.scalar("lane_by_lane", &[Val::i32(21)]), Val::i32(42));
    assert_eq!(t.trap("unwritten_lane", &[]), Trap::UninitializedLoad);
}

/// §5.10: reductions fold the lanes in order; a poison lane is observed;
/// `fadd` is ordered from its start value.
#[test]
fn reductions() {
    let t = parse(&module(
        "declare i32 @llvm.vector.reduce.add.v4i32(<4 x i32>)
         declare i32 @llvm.vector.reduce.smax.v4i32(<4 x i32>)
         declare i32 @llvm.vector.reduce.umin.v4i32(<4 x i32>)
         declare i1 @llvm.vector.reduce.or.v8i1(<8 x i1>)
         declare i1 @llvm.vector.reduce.and.v8i1(<8 x i1>)
         declare i8 @llvm.vector.reduce.xor.v3i8(<3 x i8>)
         declare float @llvm.vector.reduce.fadd.v3f32(float, <3 x float>)
         declare double @llvm.vector.reduce.fmax.v2f64(<2 x double>)
         define i32 @ints(<4 x i32> %v) {
           %a = call i32 @llvm.vector.reduce.add.v4i32(<4 x i32> %v)
           %b = call i32 @llvm.vector.reduce.smax.v4i32(<4 x i32> %v)
           %c = call i32 @llvm.vector.reduce.umin.v4i32(<4 x i32> %v)
           %x = xor i32 %a, %b
           %r = xor i32 %x, %c
           ret i32 %r
         }
         define i8 @masks(i8 %m) {
           %v = bitcast i8 %m to <8 x i1>
           %o = call i1 @llvm.vector.reduce.or.v8i1(<8 x i1> %v)
           %a = call i1 @llvm.vector.reduce.and.v8i1(<8 x i1> %v)
           %oz = zext i1 %o to i8
           %az = zext i1 %a to i8
           %s = shl i8 %az, 1
           %r = or i8 %oz, %s
           ret i8 %r
         }
         define i8 @poisoned(<3 x i8> %v) {
           %s = shl <3 x i8> %v, <i8 1, i8 1, i8 8>
           %r = call i8 @llvm.vector.reduce.xor.v3i8(<3 x i8> %s)
           ret i8 %r
         }
         define float @ordered(<3 x float> %v) {
           %r = call float @llvm.vector.reduce.fadd.v3f32(float 0x4170000000000000, <3 x float> %v)
           ret float %r
         }
         define double @fmax(<2 x double> %v) {
           %r = call double @llvm.vector.reduce.fmax.v2f64(<2 x double> %v)
           ret double %r
         }",
    ));
    let types = &t.module.types;
    let sig = |name: &str| match types.kind(t.module.symbol(t.sym(name)).ty()) {
        hod_ir::TypeKind::Function { params, .. } => params.to_vec(),
        _ => unreachable!(),
    };
    let vec = |name: &str, values: &[Val]| {
        let lanes: Vec<Option<Val>> = values.iter().cloned().map(Some).collect();
        Val::Agg(hod_interp::Vector::from_lanes(types, sig(name)[0], &lanes).unwrap())
    };
    let v = vec("ints", &[Val::i32(5), Val::i32(u32::MAX), Val::i32(3), Val::i32(0x7000_0000)]);
    let add = 5u32.wrapping_add(u32::MAX).wrapping_add(3).wrapping_add(0x7000_0000);
    assert_eq!(t.scalar("ints", &[v]), Val::i32(add ^ 0x7000_0000 ^ 3));
    assert_eq!(t.scalar("masks", &[Val::i8(0)]), Val::i8(0));
    assert_eq!(t.scalar("masks", &[Val::i8(0x10)]), Val::i8(1));
    assert_eq!(t.scalar("masks", &[Val::i8(0xff)]), Val::i8(3));
    let v = vec("poisoned", &[Val::i8(1), Val::i8(2), Val::i8(3)]);
    assert_eq!(t.trap("poisoned", &[v]), Trap::PoisonLane);
    // 2^24 + 1 + 1 rounds to 2^24 twice in order; 2^24 + 2 otherwise.
    let v = vec("ordered", &[Val::f32(1.0), Val::f32(1.0), Val::f32(-16_777_216.0)]);
    assert_eq!(t.scalar("ordered", &[v]), Val::f32(0.0));
    let v = vec("fmax", &[Val::f64(1.0), Val::f64(2.0)]);
    assert_eq!(t.unsupported("fmax", &[v]), "intrinsic:llvm.vector.reduce.fmax");
}

/// §5.10: the vector forms of the scalar intrinsics of §5.6, lane by
/// lane; a lane the scalar rules make poison is a poison lane.
#[test]
fn vector_intrinsics() {
    let t = parse(&module(
        "declare <4 x i8> @llvm.ctlz.v4i8(<4 x i8>, i1)
         declare <4 x i8> @llvm.abs.v4i8(<4 x i8>, i1)
         declare <4 x i8> @llvm.umin.v4i8(<4 x i8>, <4 x i8>)
         declare <4 x i8> @llvm.fshl.v4i8(<4 x i8>, <4 x i8>, <4 x i8>)
         declare <4 x i8> @llvm.uadd.sat.v4i8(<4 x i8>, <4 x i8>)
         declare <4 x i16> @llvm.bswap.v4i16(<4 x i16>)
         declare <2 x double> @llvm.fma.v2f64(<2 x double>, <2 x double>, <2 x double>)
         declare <2 x double> @llvm.floor.v2f64(<2 x double>)
         declare <2 x double> @llvm.copysign.v2f64(<2 x double>, <2 x double>)
         declare { <2 x i32>, <2 x i1> } @llvm.sadd.with.overflow.v2i32(<2 x i32>, <2 x i32>)
         define <4 x i8> @ctlz(<4 x i8> %v) {
           %r = call <4 x i8> @llvm.ctlz.v4i8(<4 x i8> %v, i1 true)
           ret <4 x i8> %r
         }
         define <4 x i8> @abs(<4 x i8> %v) {
           %r = call <4 x i8> @llvm.abs.v4i8(<4 x i8> %v, i1 false)
           ret <4 x i8> %r
         }
         define <4 x i8> @mix(<4 x i8> %a, <4 x i8> %b) {
           %m = call <4 x i8> @llvm.umin.v4i8(<4 x i8> %a, <4 x i8> %b)
           %f = call <4 x i8> @llvm.fshl.v4i8(<4 x i8> %m, <4 x i8> %b, <4 x i8> %a)
           %s = call <4 x i8> @llvm.uadd.sat.v4i8(<4 x i8> %f, <4 x i8> %a)
           ret <4 x i8> %s
         }
         define <4 x i16> @bswap(<4 x i16> %v) {
           %r = call <4 x i16> @llvm.bswap.v4i16(<4 x i16> %v)
           ret <4 x i16> %r
         }
         define <2 x double> @floats(<2 x double> %a, <2 x double> %b) {
           %f = call <2 x double> @llvm.fma.v2f64(<2 x double> %a, <2 x double> %b, <2 x double> %b)
           %l = call <2 x double> @llvm.floor.v2f64(<2 x double> %f)
           %c = call <2 x double> @llvm.copysign.v2f64(<2 x double> %l, <2 x double> %a)
           ret <2 x double> %c
         }
         define <2 x i32> @checked(<2 x i32> %a) {
           %p = call { <2 x i32>, <2 x i1> } @llvm.sadd.with.overflow.v2i32(<2 x i32> %a, <2 x i32> %a)
           %r = extractvalue { <2 x i32>, <2 x i1> } %p, 0
           ret <2 x i32> %r
         }",
    ));
    let types = &t.module.types;
    let sig = |name: &str| match types.kind(t.module.symbol(t.sym(name)).ty()) {
        hod_ir::TypeKind::Function { params, .. } => params.to_vec(),
        _ => unreachable!(),
    };
    let image = |name: &str, bytes: &[u8]| {
        Val::Agg(hod_interp::Vector::from_image(types, sig(name)[0], bytes).unwrap())
    };
    assert_eq!(lanes(&t.vector("ctlz", &[image("ctlz", &[1, 0, 0x80, 0x0f])])), "7 _ 0 4");
    assert_eq!(lanes(&t.vector("abs", &[image("abs", &[0xff, 0x80, 3, 0x81])])), "1 128 3 127");
    // umin(a, b) = m; fshl(m, b, a % 8); + a saturating.
    let a = [3u8, 200, 0, 9];
    let b = [5u8, 100, 7, 1];
    let want: Vec<String> = (0..4)
        .map(|i| {
            let m = a[i].min(b[i]);
            let k = u32::from(a[i] % 8);
            let wide = (u16::from(m) << 8) | u16::from(b[i]);
            let f = ((wide << k) >> 8) as u8;
            f.saturating_add(a[i]).to_string()
        })
        .collect();
    assert_eq!(lanes(&t.vector("mix", &[image("mix", &a), image("mix", &b)])), want.join(" "));
    assert_eq!(
        lanes(&t.vector("bswap", &[image("bswap", &[1, 2, 3, 4, 5, 6, 7, 8])])),
        "258 772 1286 1800"
    );
    let d = |x: f64, y: f64| {
        Val::Agg(
            hod_interp::Vector::from_lanes(
                types,
                sig("floats")[0],
                &[Some(Val::f64(x)), Some(Val::f64(y))],
            )
            .unwrap(),
        )
    };
    // fma(-1.5, 2, 2) = -1 -> floor -1 -> sign of -1.5; fma(2.5, 0.5, 0.5) = 1.75 -> 1.
    assert_eq!(lanes(&t.vector("floats", &[d(-1.5, 2.5), d(2.0, 0.5)])), "-1 1");
    let v = image("checked", &[1, 0, 0, 0, 2, 0, 0, 0]);
    assert_eq!(t.unsupported("checked", &[v]), "intrinsic-vector:llvm.sadd.with.overflow");
}

/// §5.10: vectors are arguments, results and `phi`s of interpreted
/// functions, fields of aggregates, and module variables; an aggregate
/// cannot hold a poison lane.
#[test]
fn vectors_as_values() {
    let t = parse(&module(
        "@g = global <4 x i32> <i32 1, i32 2, i32 3, i32 4>
         @s = global { i8, <2 x i64> } { i8 7, <2 x i64> <i64 8, i64 9> }
         define <4 x i32> @twice(<4 x i32> %v) {
           %r = add <4 x i32> %v, %v
           ret <4 x i32> %r
         }
         define <4 x i32> @loop(i32 %n) {
         entry:
           %g = load <4 x i32>, ptr @g
           br label %head
         head:
           %v = phi <4 x i32> [ %g, %entry ], [ %w, %head ]
           %i = phi i32 [ 0, %entry ], [ %j, %head ]
           %w = call <4 x i32> @twice(<4 x i32> %v)
           %j = add i32 %i, 1
           %c = icmp ult i32 %j, %n
           br i1 %c, label %head, label %done
         done:
           ret <4 x i32> %w
         }
         define i64 @field() {
           %a = load { i8, <2 x i64> }, ptr @s
           %v = extractvalue { i8, <2 x i64> } %a, 1
           %e = extractelement <2 x i64> %v, i32 1
           ret i64 %e
         }
         define { i8, <2 x i64> } @insert_poison(<2 x i64> %v) {
           %s = shl <2 x i64> %v, <i64 64, i64 0>
           %a = insertvalue { i8, <2 x i64> } zeroinitializer, <2 x i64> %s, 1
           ret { i8, <2 x i64> } %a
         }
         define <2 x i64> @freeze(<2 x i64> %v) {
           %s = shl <2 x i64> %v, <i64 64, i64 0>
           %f = freeze <2 x i64> %s
           ret <2 x i64> %f
         }",
    ));
    assert_eq!(lanes(&t.vector("loop", &[Val::i32(3)])), "8 16 24 32");
    assert_eq!(t.scalar("field", &[]), Val::i64(9));
    let types = &t.module.types;
    let sig = |name: &str| match types.kind(t.module.symbol(t.sym(name)).ty()) {
        hod_ir::TypeKind::Function { params, .. } => params.to_vec(),
        _ => unreachable!(),
    };
    let v = || {
        Val::Agg(
            hod_interp::Vector::from_lanes(
                types,
                sig("freeze")[0],
                &[Some(Val::i64(1)), Some(Val::i64(2))],
            )
            .unwrap(),
        )
    };
    assert_eq!(t.trap("insert_poison", &[v()]), Trap::PoisonLane);
    // `freeze` fixes a poison lane to what the compilation chooses.
    assert_eq!(lanes(&t.vector("freeze", &[v()])), "_ 2");
}

/// §5.10: `trunc` of wide lanes keeps the low bits of each. (The
/// selection DAG of `llc` 19 and 22 gives `ff 00 00` and `ff ff 00` for
/// these at every optimization level, and is right for the other lengths
/// from 2 to 16 and widths from 16 to 128; `llc -O0` of either, through
/// the global instruction selector, gives these bytes.)
#[test]
fn truncation_of_wide_lanes() {
    let t = parse(&module(
        "@g = global <3 x i128> <i128 -1, i128 1, i128 -1642376755058837071914713843657646633>
         @h = global <3 x i64> <i64 -1, i64 11, i64 -1>
         define <3 x i8> @wide() {
           %v = load <3 x i128>, ptr @g
           %t = trunc <3 x i128> %v to <3 x i8>
           ret <3 x i8> %t
         }
         define <3 x i8> @long() {
           %v = load <3 x i64>, ptr @h
           %t = trunc <3 x i64> %v to <3 x i8>
           ret <3 x i8> %t
         }",
    ));
    assert_eq!(t.vector("wide", &[]).bytes(), [0xff, 0x01, 0xd7]);
    assert_eq!(t.vector("long", &[]).bytes(), [0xff, 0x0b, 0xff]);
}

/// §8: what is refused, by name.
#[test]
fn refused_vector_forms() {
    let t = parse(&module(
        "declare <2 x i8> @llvm.vector.reduce.fmin.v2f32(<2 x float>)
         define <4 x i4> @odd(<4 x i4> %v) {
           %r = add <4 x i4> %v, %v
           ret <4 x i4> %r
         }
         define <2 x half> @half(<2 x half> %v) {
           %r = fadd <2 x half> %v, %v
           ret <2 x half> %r
         }
         define <2 x ptr> @gep(ptr %p) {
           %v = getelementptr i8, ptr %p, <2 x i64> <i64 0, i64 8>
           ret <2 x ptr> %v
         }
         define <vscale x 2 x i64> @scalable(<vscale x 2 x i64> %v) {
           ret <vscale x 2 x i64> %v
         }
         define <2 x i128> @wide(<2 x i128> %v) {
           %r = add <2 x i128> %v, %v
           ret <2 x i128> %r
         }",
    ));
    // Arguments of these types cannot even be made; zero constants can.
    let run = |name: &str| {
        let program = t.program();
        let sym = t.sym(name);
        let params = match t.module.types.kind(t.module.symbol(sym).ty()) {
            hod_ir::TypeKind::Function { params, .. } => params.to_vec(),
            _ => unreachable!(),
        };
        let mut interp = hod_interp::Interp::new(&program);
        let args: Vec<Val> = match params[0] {
            hod_ir::TypeId::PTR => vec![Val::Ptr(0)],
            _ => vec![Val::i1(false)],
        };
        interp.run(sym, &args).result
    };
    for (name, reason) in [
        ("odd", "type:vector-odd-element"),
        ("half", "type:half"),
        ("scalable", "type:scalable-vector"),
    ] {
        assert_eq!(run(name), Err(Stop::Unsupported(reason.into())), "{name}");
    }
    assert_eq!(run("gep"), Err(Stop::Unsupported("inst:ptradd-vector".into())));
    // `i128` lanes are supported.
    let types = &t.module.types;
    let sig = match types.kind(t.module.symbol(t.sym("wide")).ty()) {
        hod_ir::TypeKind::Function { params, .. } => params[0],
        _ => unreachable!(),
    };
    let v = hod_interp::Vector::from_lanes(
        types,
        sig,
        &[Some(Val::i128(u128::MAX)), Some(Val::i128(1 << 100))],
    )
    .unwrap();
    assert_eq!(
        lanes(&t.vector("wide", &[Val::Agg(v)])),
        format!("{} {}", u128::MAX - 1, 1u128 << 101)
    );
}
