//! What `assign` refuses (`spec/abi-a64.md` §10), and that it refuses
//! instead of panicking.

use std::collections::BTreeSet;

use hod_a64::abi::{MAX_PIECES, UNSUPPORTED, assign};
use hod_cg::abi::{Assignment, Sig};
use hod_ir::{CallConv, ParamAttrs, ParamMem, TypeId, TypeTable};

use crate::sigs::table;
use crate::ty::Rng;

const NONE: ParamAttrs = ParamAttrs::NONE;

fn flags(flags: u16) -> ParamAttrs {
    NONE.with(flags).expect("parameter flags")
}

fn typed(mem: ParamMem, ty: TypeId) -> ParamAttrs {
    NONE.with_mem(mem, ty).expect("a typed attribute")
}

/// The name `assign` refuses the signature with.
fn refusal(
    types: &TypeTable,
    cc: CallConv,
    ret: (TypeId, ParamAttrs),
    params: &[(TypeId, ParamAttrs)],
    fixed: Option<u32>,
) -> Option<&'static str> {
    let (tys, attrs): (Vec<TypeId>, Vec<ParamAttrs>) = params.iter().copied().unzip();
    let sig = Sig {
        cc,
        ret: ret.0,
        ret_attrs: ret.1,
        params: &tys,
        param_attrs: &attrs,
        fixed: fixed.unwrap_or(tys.len() as u32),
        variadic: fixed.is_some(),
    };
    let mut out = Assignment::default();
    let name = assign(types, &sig, &mut out).err().map(|unsupported| unsupported.0);
    if name.is_some() {
        assert_eq!(out, Assignment::default(), "an error leaves the output empty");
    }
    name
}

/// `spec/abi-a64.md` §10: every unsupported construct is refused under
/// its stable name, and `UNSUPPORTED` lists exactly those names.
#[test]
fn every_name() {
    let mut t = table();
    let void = (TypeId::VOID, NONE);
    let (i8, i64, i128, ptr) = (TypeId::I8, TypeId::I64, TypeId::I128, TypeId::PTR);
    let scalable = t.scalable_vector(TypeId::I32, 4).expect("a type");
    let target = t.target_ext("spirv.Image").expect("a type");
    let far = t.ptr(1).expect("a type");
    let function = t.function(TypeId::VOID, &[], false).expect("a type");
    let opaque = t.opaque_struct().expect("a type");
    let empty = t.struct_type(&[], false).expect("a type");
    let mut array = |elem, len| t.array(elem, len).expect("a type");
    // WP-101: `[2 x i128]` fits the registers as LLVM agrees; five do not.
    let wide_array = array(i128, 5);
    let bfloat_array = array(TypeId::BFLOAT, 2);
    let mut bfloat_late = vec![(TypeId::FLOAT, NONE); 7];
    bfloat_late.push((bfloat_array, NONE));
    let one_long = array(i64, 1);
    let one_quad = array(TypeId::FP128, 1);
    let many_bytes = array(i8, u64::from(MAX_PIECES) + 1);
    let most_bytes = array(i8, u64::from(MAX_PIECES));
    let huge = array(i64, 1 << 30);
    let huge_bytes = array(i8, 1 << 33);
    let mut vector = |elem, len| t.vector(elem, len).expect("a type");
    // WP-101: a lone `<1 x i128>`; wider ones pass.
    let v1i128 = vector(i128, 1);
    // WP-082: masks of up to 64 elements and widened vectors pass.
    let v65i1 = vector(TypeId::I1, 65);
    let v128i1 = vector(TypeId::I1, 128);
    let v4i64 = vector(i64, 4);
    let v3i64 = vector(i64, 3);
    let v2x86 = vector(TypeId::X86_FP80, 2);
    let quad_vectors = t.array(v4i64, 2).expect("a type");
    let scalarized_vectors = t.array(v3i64, 2).expect("a type");
    let far_vector = t.vector(far, 2).expect("a type");
    let has_scalable = t.struct_type(&[i64, scalable], false);

    let c = CallConv::C;
    let cases: Vec<(&str, Option<&'static str>)> = vec![
        ("abi:cc", refusal(&t, CallConv::COLD, void, &[], None)),
        ("abi:cc", refusal(&t, CallConv::TAIL, void, &[(i64, NONE)], None)),
        ("abi:cc", refusal(&t, CallConv::AARCH64_VECTOR_PCS, void, &[], None)),
        // What LLVM's verifier refuses.
        ("abi:invalid-signature", refusal(&t, c, void, &[(TypeId::VOID, NONE)], None)),
        ("abi:invalid-signature", refusal(&t, c, void, &[(function, NONE)], None)),
        ("abi:invalid-signature", refusal(&t, c, void, &[(opaque, NONE)], None)),
        ("abi:invalid-signature", refusal(&t, c, (function, NONE), &[], None)),
        (
            "abi:invalid-signature",
            refusal(&t, c, void, &[(TypeId::DOUBLE, flags(ParamAttrs::ZEROEXT))], None),
        ),
        ("abi:invalid-signature", refusal(&t, c, (ptr, flags(ParamAttrs::SIGNEXT)), &[], None)),
        ("abi:invalid-signature", refusal(&t, c, (ptr, flags(ParamAttrs::NEST)), &[], None)),
        ("abi:invalid-signature", refusal(&t, c, (ptr, typed(ParamMem::Sret, i64)), &[], None)),
        ("abi:invalid-signature", refusal(&t, c, void, &[(i64, typed(ParamMem::Sret, i64))], None)),
        (
            "abi:invalid-signature",
            refusal(&t, c, (i64, NONE), &[(ptr, typed(ParamMem::Sret, i64))], None),
        ),
        (
            "abi:invalid-signature",
            refusal(
                &t,
                c,
                void,
                &[(ptr, typed(ParamMem::Sret, i64)), (ptr, typed(ParamMem::Sret, i64))],
                None,
            ),
        ),
        (
            "abi:invalid-signature",
            refusal(&t, c, void, &[(i64, typed(ParamMem::ByVal, i64))], None),
        ),
        (
            "abi:invalid-signature",
            refusal(&t, c, void, &[(ptr, typed(ParamMem::ByVal, TypeId::VOID))], None),
        ),
        (
            "abi:invalid-signature",
            refusal(&t, c, void, &[(ptr, typed(ParamMem::ByVal, opaque))], None),
        ),
        ("abi:invalid-signature", refusal(&t, c, void, &[(i64, flags(ParamAttrs::NEST))], None)),
        (
            "abi:invalid-signature",
            refusal(&t, c, void, &[(i64, typed(ParamMem::ByRef, i64))], None),
        ),
        (
            "abi:invalid-signature",
            refusal(
                &t,
                c,
                void,
                &[(ptr, flags(ParamAttrs::NEST)), (ptr, flags(ParamAttrs::NEST))],
                None,
            ),
        ),
        ("abi:invalid-signature", refusal(&t, c, void, &[(i64, NONE)], Some(2))),
        ("abi:invalid-signature", refusal(&t, CallConv::FAST, void, &[(i64, NONE)], Some(1))),
        ("abi:type-x86_fp80", refusal(&t, c, void, &[(TypeId::X86_FP80, NONE)], None)),
        ("abi:type-x86_fp80", refusal(&t, c, (TypeId::X86_FP80, NONE), &[], None)),
        ("abi:scalable-vector", refusal(&t, c, void, &[(scalable, NONE)], None)),
        ("abi:scalable-vector", refusal(&t, c, (scalable, NONE), &[], None)),
        ("abi:target-ext-type", refusal(&t, c, void, &[(target, NONE)], None)),
        ("abi:addrspace", refusal(&t, c, void, &[(far, NONE)], None)),
        ("abi:addrspace", refusal(&t, c, void, &[(far_vector, NONE)], None)),
        ("abi:addrspace", refusal(&t, c, (far, NONE), &[], None)),
        ("abi:vector-element", refusal(&t, c, void, &[(v1i128, NONE)], None)),
        ("abi:vector-element", refusal(&t, c, void, &[(v2x86, NONE)], None)),
        ("abi:mask-wide", refusal(&t, c, void, &[(v65i1, NONE)], None)),
        ("abi:mask-wide", refusal(&t, c, (v128i1, NONE), &[], None)),
        ("abi:wide-int-array", refusal(&t, c, void, &[(wide_array, NONE)], None)),
        // WP-101: refused only when it does not fit the registers left.
        ("abi:bfloat-array", refusal(&t, c, void, &bfloat_late, None)),
        ("abi:vector-array", refusal(&t, c, void, &[(quad_vectors, NONE)], None)),
        ("abi:vector-array", refusal(&t, c, void, &[(scalarized_vectors, NONE)], None)),
        (
            "abi:one-leaf-array",
            refusal(
                &t,
                c,
                void,
                &[(one_long, NONE.with_stack_align(16).expect("an alignment"))],
                None,
            ),
        ),
        (
            "abi:one-leaf-array",
            refusal(
                &t,
                c,
                void,
                &[(one_quad, NONE.with_stack_align(8).expect("an alignment"))],
                None,
            ),
        ),
        (
            "abi:attribute-after-empty-argument",
            refusal(&t, c, void, &[(empty, NONE), (i8, flags(ParamAttrs::ZEROEXT))], None),
        ),
        (
            "abi:attribute-after-empty-argument",
            refusal(
                &t,
                c,
                void,
                &[(empty, NONE), (i64, NONE), (ptr, typed(ParamMem::ByVal, i64))],
                None,
            ),
        ),
        (
            "abi:attribute-after-empty-argument",
            refusal(
                &t,
                c,
                void,
                &[(empty, NONE.with_stack_align(16).expect("an alignment")), (i64, NONE)],
                None,
            ),
        ),
        ("abi:inreg", refusal(&t, c, void, &[(i64, flags(ParamAttrs::INREG))], None)),
        ("abi:inreg", refusal(&t, c, (i64, flags(ParamAttrs::INREG)), &[], None)),
        ("abi:swiftself", refusal(&t, c, void, &[(ptr, flags(ParamAttrs::SWIFT_SELF))], None)),
        ("abi:swifterror", refusal(&t, c, void, &[(ptr, flags(ParamAttrs::SWIFT_ERROR))], None)),
        ("abi:swiftasync", refusal(&t, c, void, &[(ptr, flags(ParamAttrs::SWIFT_ASYNC))], None)),
        ("abi:inalloca", refusal(&t, c, void, &[(ptr, typed(ParamMem::InAlloca, i64))], None)),
        (
            "abi:preallocated",
            refusal(&t, c, void, &[(ptr, typed(ParamMem::Preallocated, i64))], None),
        ),
        (
            "abi:elementtype",
            refusal(&t, c, void, &[(ptr, typed(ParamMem::ElementType, i64))], None),
        ),
        (
            "abi:byval-align",
            refusal(
                &t,
                c,
                void,
                &[(ptr, typed(ParamMem::ByVal, i64).with_align(32).expect("an alignment"))],
                None,
            ),
        ),
        ("abi:too-many-pieces", refusal(&t, c, void, &[(many_bytes, NONE)], None)),
        ("abi:too-many-pieces", refusal(&t, c, void, &[(most_bytes, NONE), (i8, NONE)], None)),
        (
            "abi:size-limit",
            refusal(
                &t,
                c,
                void,
                &[(i64, NONE); 9].iter().copied().chain([(huge, NONE)]).collect::<Vec<_>>(),
                None,
            ),
        ),
        (
            "abi:size-limit",
            refusal(&t, c, void, &[(ptr, typed(ParamMem::ByVal, huge_bytes))], None),
        ),
    ];
    let mut seen = BTreeSet::new();
    for (index, (want, got)) in cases.iter().enumerate() {
        assert_eq!(*got, Some(*want), "case {index}");
        seen.insert(*want);
    }
    assert_eq!(seen, UNSUPPORTED.iter().copied().collect::<BTreeSet<_>>());
    assert_eq!(seen.len(), UNSUPPORTED.len(), "a name is listed twice");
    assert!(UNSUPPORTED.iter().all(|name| name.starts_with("abi:")));

    // At the limits, not beyond them.
    assert_eq!(refusal(&t, c, void, &[(most_bytes, NONE)], None), None);
    assert_eq!(refusal(&t, c, (many_bytes, NONE), &[], None), None);
    assert_eq!(
        refusal(&t, c, (huge, NONE), &[(ptr, typed(ParamMem::ByRef, huge_bytes))], None),
        None
    );
    if let Ok(holder) = has_scalable {
        assert_eq!(refusal(&t, c, void, &[(holder, NONE)], None), Some("abi:scalable-vector"));
    }
}

/// `spec/abi-a64.md` §10: any signature over valid types, however odd,
/// gets an assignment or one of the names; `assign` never panics, and a
/// deep or huge type costs no more than its description.
#[test]
fn any_signature_is_answered() {
    let mut t = table();
    let mut pool: Vec<TypeId> = vec![
        TypeId::VOID,
        TypeId::HALF,
        TypeId::BFLOAT,
        TypeId::FLOAT,
        TypeId::DOUBLE,
        TypeId::FP128,
        TypeId::X86_FP80,
        TypeId::PTR,
        TypeId::I1,
        TypeId::I8,
        TypeId::I16,
        TypeId::I32,
        TypeId::I64,
        TypeId::I128,
    ];
    pool.extend(
        [t.int(7), t.int(65), t.int(1000), t.ptr(3), t.opaque_struct()].into_iter().flatten(),
    );
    pool.extend(
        [t.function(TypeId::I32, &[TypeId::I64], true), t.target_ext("x")].into_iter().flatten(),
    );
    pool.extend(t.scalable_vector(TypeId::I8, 16));
    let mut rng = Rng(0x0bad_5eed);
    // Derived types over anything already in the pool; the table refuses
    // the combinations LLVM has no type for.
    for _ in 0..600 {
        let pick = |rng: &mut Rng, pool: &[TypeId]| pool[rng.below(pool.len() as u64) as usize];
        let made = match rng.below(4) {
            0 => {
                let len = *rng.pick(&[0, 1, 2, 3, 8, 9, 1000, 1 << 20, 1 << 40]);
                t.array(pick(&mut rng, &pool), len)
            }
            1 => {
                let len = *rng.pick(&[1, 2, 3, 4, 5, 8, 16, 17, 64, 1 << 16]);
                t.vector(pick(&mut rng, &pool), len)
            }
            _ => {
                let fields: Vec<TypeId> =
                    (0..rng.below(6)).map(|_| pick(&mut rng, &pool)).collect();
                t.struct_type(&fields, rng.chance(4))
            }
        };
        pool.extend(made);
    }
    // A type nested a few hundred deep.
    let mut deep = TypeId::I64;
    for level in 0..300 {
        deep = if level % 2 == 0 { t.struct_type(&[deep], false) } else { t.array(deep, 1) }
            .expect("a type");
    }
    pool.push(deep);

    let conventions =
        [CallConv::C, CallConv::C, CallConv::C, CallConv::FAST, CallConv::SWIFT, CallConv::COLD];
    let mems = [
        ParamMem::Sret,
        ParamMem::ByVal,
        ParamMem::ByRef,
        ParamMem::InAlloca,
        ParamMem::Preallocated,
        ParamMem::ElementType,
    ];
    let bits = [
        ParamAttrs::ZEROEXT,
        ParamAttrs::SIGNEXT,
        ParamAttrs::INREG,
        ParamAttrs::NEST,
        ParamAttrs::SWIFT_SELF,
        ParamAttrs::SWIFT_ERROR,
        ParamAttrs::SWIFT_ASYNC,
    ];
    let mut out = Assignment::default();
    let (mut accepted, mut refused) = (0, BTreeSet::new());
    for _ in 0..20_000 {
        let attrs_of = |rng: &mut Rng, pool: &[TypeId]| {
            let mut attrs = NONE;
            if rng.chance(3) {
                attrs = attrs.with(*rng.pick(&bits)).unwrap_or(attrs);
            }
            if rng.chance(4) {
                let ty = pool[rng.below(pool.len() as u64) as usize];
                attrs = attrs.with_mem(*rng.pick(&mems), ty).unwrap_or(attrs);
            }
            if rng.chance(4) {
                attrs = attrs.with_align(1 << rng.below(8)).unwrap_or(attrs);
            }
            if rng.chance(4) {
                attrs = attrs.with_stack_align(1 << rng.below(8)).unwrap_or(attrs);
            }
            attrs
        };
        let count = rng.below(12) as usize;
        let params: Vec<TypeId> =
            (0..count).map(|_| pool[rng.below(pool.len() as u64) as usize]).collect();
        let attrs: Vec<ParamAttrs> =
            (0..rng.below(count as u64 + 1)).map(|_| attrs_of(&mut rng, &pool)).collect();
        let ret =
            if rng.chance(2) { TypeId::VOID } else { pool[rng.below(pool.len() as u64) as usize] };
        let ret_attrs = if rng.chance(8) { attrs_of(&mut rng, &pool) } else { NONE };
        let variadic = rng.chance(5);
        let sig = Sig {
            cc: *rng.pick(&conventions),
            ret,
            ret_attrs,
            params: &params,
            param_attrs: &attrs,
            fixed: if variadic { rng.below(count as u64 + 2) as u32 } else { count as u32 },
            variadic,
        };
        match assign(&t, &sig, &mut out) {
            Ok(()) => {
                accepted += 1;
                // §2: parts in argument order, each within its value.
                assert!(out.args.is_sorted_by_key(|part| part.arg));
                assert!(out.args.iter().all(|part| (part.arg as usize) < count));
                assert!(out.ret_memory.is_none() || out.ret.is_empty());
            }
            Err(unsupported) => {
                assert!(UNSUPPORTED.contains(&unsupported.0), "{}", unsupported.0);
                assert_eq!(out, Assignment::default());
                refused.insert(unsupported.0);
            }
        }
    }
    assert!(accepted > 200, "only {accepted} signatures were accepted");
    assert!(refused.len() > 12, "only {refused:?} were refused");
    eprintln!("any signature: {accepted} accepted; refused under {} names", refused.len());
}
