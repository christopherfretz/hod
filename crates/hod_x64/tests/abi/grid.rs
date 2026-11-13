//! The grid of signatures (`spec/abi-x64.md` §11): a systematic part that
//! puts every type under every kind of register and stack pressure, each
//! attribute, each calling convention; and a seeded random part.

use std::collections::{BTreeMap, HashSet};

use hod_ir::CallConv;

use crate::run::{Batch, Report, Tools};
use crate::sigs::{Attr, Case, Param, byval, p, pa, table};
use crate::ty::{I1, I8, I16, I32, I64, I128, Rng, Ty, arr, st, vec};

/// The integer widths the grid uses.
pub const WIDTHS: [u32; 20] =
    [1, 7, 8, 9, 16, 17, 24, 31, 32, 33, 48, 63, 64, 65, 96, 127, 128, 129, 192, 256];

/// Every scalar type.
pub fn scalars() -> Vec<Ty> {
    let mut all: Vec<Ty> = WIDTHS.iter().map(|&bits| Ty::Int(bits)).collect();
    all.extend([Ty::Ptr, Ty::Half, Ty::BFloat, Ty::Float, Ty::Double, Ty::Fp128, Ty::X86Fp80]);
    all
}

/// Fixed vectors of every shape §4.3 passes: legal, widened, split and
/// passed element by element.
pub fn vectors() -> Vec<Ty> {
    let (h, b, f, d, q) = (Ty::Half, Ty::BFloat, Ty::Float, Ty::Double, Ty::Fp128);
    let shapes: [(Ty, &[u32]); 11] = [
        (I1, &[1, 2, 3, 4, 8, 16, 32]),
        (I8, &[1, 2, 3, 4, 5, 7, 8, 9, 12, 16, 17, 24, 32]),
        (I16, &[1, 2, 3, 4, 5, 6, 8, 16]),
        (I32, &[1, 2, 3, 4, 5, 6, 8, 16]),
        (I64, &[1, 2, 3, 4, 6]),
        (Ty::Ptr, &[1, 2, 3, 4]),
        (h, &[1, 2, 3, 4, 5, 6, 8, 10, 16]),
        (b, &[1, 3, 4, 8]),
        (f, &[1, 2, 3, 4, 5, 6, 8, 12, 16]),
        (d, &[1, 2, 3, 4, 8]),
        (q, &[1, 2, 3]),
    ];
    let mut all = Vec::new();
    for (elem, lens) in shapes {
        all.extend(lens.iter().map(|&len| vec(elem.clone(), len)));
    }
    all
}

/// Arrays and structures: first the shapes clang emits for this target,
/// then a sample of others.
pub fn aggregates() -> Vec<Ty> {
    let (h, f, d, q) = (Ty::Half, Ty::Float, Ty::Double, Ty::Fp128);
    vec![
        // What clang coerces C and C++ aggregates to.
        arr(I64, 2),
        arr(I64, 1),
        arr(f.clone(), 1),
        arr(f.clone(), 2),
        arr(f.clone(), 3),
        arr(f.clone(), 4),
        arr(d.clone(), 1),
        arr(d.clone(), 2),
        arr(d.clone(), 3),
        arr(d.clone(), 4),
        arr(h.clone(), 2),
        arr(h.clone(), 4),
        arr(q.clone(), 2),
        arr(q.clone(), 4),
        arr(vec(I32, 4), 2),
        arr(vec(d.clone(), 2), 4),
        arr(vec(I8, 16), 3),
        arr(vec(f.clone(), 2), 2),
        arr(vec(I64, 1), 3),
        // Others.
        st(&[I64, I64]),
        st(&[I8, I32]),
        st(&[I32, I32, I32]),
        st(&[d.clone(), d.clone()]),
        st(&[f.clone(), I32]),
        st(&[I64, d.clone()]),
        st(&[I8, I128]),
        st(&[Ty::Ptr, I64]),
        st(&[I1, I1]),
        Ty::Packed(vec![I8, I64]),
        Ty::Packed(vec![I8, d.clone(), I16]),
        st(&[arr(I64, 2), I32]),
        st(&[st(&[I32, f.clone()]), arr(I16, 2)]),
        st(&[q.clone(), I8]),
        st(&[h.clone(), h.clone()]),
        st(&[I128, I128]),
        st(&[Ty::Int(65)]),
        st(&[vec(I32, 4), I32]),
        st(&[f.clone(), vec(I64, 2)]),
        st(&[I64, I64, I64, I64, I64, I64, I64, I64]),
        st(&[d.clone(), I64, d.clone(), I64, f.clone(), I8, h.clone(), I1]),
        arr(I8, 3),
        arr(I1, 2),
        arr(I32, 2),
        arr(I16, 4),
        arr(Ty::Int(33), 2),
        arr(Ty::Ptr, 2),
        arr(I64, 3),
        arr(I64, 8),
        arr(I64, 9),
        arr(d.clone(), 5),
        arr(d.clone(), 8),
        arr(f.clone(), 9),
        arr(h.clone(), 3),
        arr(q.clone(), 3),
        arr(st(&[Ty::Ptr, I64]), 2),
        arr(st(&[I64, d.clone()]), 2),
        arr(arr(f.clone(), 2), 2),
        arr(arr(I64, 2), 1),
        arr(st(&[f.clone(), f.clone()]), 3),
        arr(vec(I32, 3), 2),
        arr(vec(I8, 3), 2),
        arr(q.clone(), 1),
        arr(vec(I32, 4), 1),
        arr(vec(I32, 3), 1),
        arr(h.clone(), 1),
        arr(arr(st(std::slice::from_ref(&f)), 1), 1),
        arr(I64, 40),
        arr(I32, 20),
        // Types with no leaf.
        st(&[]),
        arr(I64, 0),
        arr(st(&[]), 2),
        st(&[arr(I64, 0), I32]),
        st(&[st(&[]), d.clone(), st(&[])]),
        // x86-64's own.
        Ty::X86Fp80,
        st(&[Ty::X86Fp80, Ty::X86Fp80]),
        st(&[Ty::X86Fp80, d.clone()]),
        st(&[Ty::X86Fp80, f.clone()]),
        st(&[d.clone(), Ty::X86Fp80]),
        arr(Ty::X86Fp80, 3),
        st(&[I128, I64]),
        st(&[I64, I128]),
        st(&[I32, I128]),
        arr(I128, 2),
        st(&[h.clone(), h.clone(), h.clone()]),
        st(&[h.clone(), h.clone(), h.clone(), h.clone()]),
        st(&[h.clone(), f.clone()]),
        st(&[q.clone(), q.clone()]),
        st(&[q.clone(), q.clone(), q.clone()]),
        st(&[q.clone(), d.clone()]),
        st(&[d.clone(), d]),
        st(&[I64, I64, I64]),
        st(&[I64, I64, I64, I64]),
        arr(vec(f, 4), 4),
        arr(vec(I8, 16), 5),
        arr(vec(I1, 4), 2),
        Ty::Int(192),
    ]
}

/// Every type of the systematic grid.
pub fn all_types() -> Vec<Ty> {
    let mut all = scalars();
    all.extend(vectors());
    all.extend(aggregates());
    all
}

fn seq(parts: &[(&Ty, usize)]) -> Vec<Ty> {
    parts.iter().flat_map(|&(ty, count)| std::iter::repeat_n(ty.clone(), count)).collect()
}

/// `ty` alone, and under each kind of pressure: after an odd number of
/// general registers, with one register of a class left, with none, at an
/// odd stack offset, and between other arguments that show what it left
/// behind.
pub fn pressures(ty: &Ty) -> Vec<Case> {
    let (i, d, f, b, w, s) = (&I64, &Ty::Double, &Ty::Float, &I8, &I32, &I16);
    vec![
        Case::of(std::slice::from_ref(ty)).ret(ty.clone()),
        Case::of(&seq(&[(i, 1), (ty, 1), (w, 1), (f, 1)])),
        Case::of(&seq(&[(i, 4), (ty, 1), (w, 1), (f, 1), (b, 1)])),
        Case::of(&seq(&[(i, 5), (ty, 1), (w, 1), (f, 1), (b, 1)])),
        Case::of(&seq(&[(i, 6), (ty, 1), (w, 1), (f, 1), (b, 1)])),
        Case::of(&seq(&[(d, 6), (ty, 1), (f, 1), (w, 1), (d, 1)])),
        Case::of(&seq(&[(d, 7), (ty, 1), (f, 1), (w, 1), (d, 1)])),
        Case::of(&seq(&[(d, 8), (ty, 1), (f, 1), (w, 1), (d, 1)])),
        Case::of(&seq(&[(i, 6), (d, 8), (b, 1), (ty, 1), (b, 1), (ty, 1), (s, 1)])),
        Case::of(&seq(&[(i, 3), (d, 5), (ty, 1), (i, 1), (ty, 1), (d, 1)])),
        Case::of(&seq(&[(ty, 9)])),
        Case::of(&seq(&[(w, 1), (ty, 2)])).variadic(1),
        Case::of(&seq(&[(i, 5), (d, 7), (ty, 2), (w, 1), (d, 1)])).variadic(12),
        Case::of(&seq(&[(i, 6), (d, 8), (b, 1), (ty, 2), (b, 1)])).variadic(14),
    ]
}

/// §7: `zeroext` and `signext`, in registers, on the stack and returned.
fn extensions() -> Vec<Case> {
    let mut cases = Vec::new();
    for bits in WIDTHS {
        let ty = Ty::Int(bits);
        for signed in [false, true] {
            let ext = || pa(ty.clone(), Attr::Ext(signed));
            let fill = |count: usize| std::iter::repeat_n(p(I64), count);
            cases.push(Case::new(vec![ext(), ext()]).ret_ext(ty.clone(), signed));
            cases.push(Case::new(fill(5).chain([ext(), ext()]).collect()));
            cases.push(Case::new(fill(6).chain([ext(), p(I8), ext()]).collect()));
            cases.push(Case::new(vec![p(I32), ext(), ext()]).variadic(1));
            cases.push(Case::new(fill(6).chain([ext(), ext()]).collect()).variadic(6));
        }
    }
    cases
}

/// §6: `sret`, `byval`, `byref`, `nest` and `alignstack`.
fn attributes() -> Vec<Case> {
    let d = Ty::Double;
    let fill = |count: usize| std::iter::repeat_n(p(I64), count);
    let sret = || pa(Ty::Ptr, Attr::Sret);
    let nest = || pa(Ty::Ptr, Attr::Nest);
    let byref = || pa(Ty::Ptr, Attr::ByRef);
    let mut cases = vec![
        Case::new(vec![sret()]),
        Case::new(vec![sret(), p(I64), p(d.clone())]),
        Case::new(vec![p(I64), sret(), p(I64)]),
        Case::new([sret()].into_iter().chain(fill(9)).collect()),
        Case::new(fill(5).chain([pa(Ty::Ptr, Attr::None)]).collect()).ret(arr(I64, 9)),
        Case::new(fill(6).chain([p(I8)]).collect()).ret(arr(I64, 4)),
        Case::new(vec![p(I64)]).ret(arr(I64, 4)).variadic(1),
        Case::new(vec![sret(), p(I128)]),
        Case::new(vec![byref()]),
        Case::new(vec![p(I64), byref(), p(I64)]).ret(Ty::Ptr),
        Case::new(fill(8).chain([byref(), p(I8)]).collect()),
        Case::new(vec![nest()]),
        Case::new(vec![p(I64), nest(), p(I64)]).ret(I64),
        Case::new(fill(8).chain([nest(), p(I64)]).collect()),
        Case::new(vec![nest(), sret(), p(I64)]),
        Case::new(vec![nest(), p(I64)]).ret(arr(I64, 9)),
    ];

    let pointees = [
        I8,
        arr(I8, 3),
        st(&[]),
        I64,
        arr(I64, 2),
        I128,
        st(&[I8, I128]),
        arr(I8, 40),
        st(&[d.clone(), d.clone()]),
        arr(I32, 9),
        vec(I32, 4),
        arr(I64, 100),
        Ty::Fp128,
        Ty::Packed(vec![I8, I128]),
    ];
    let aligns =
        [(None, None), (Some(1), None), (Some(4), None), (Some(8), None), (Some(16), None)];
    let stack_aligns =
        [(None, Some(16)), (Some(4), Some(16)), (Some(16), Some(8)), (None, Some(4))];
    for pointee in &pointees {
        for (align, stack_align) in aligns.into_iter().chain(stack_aligns) {
            let val = || byval(pointee.clone(), align, stack_align);
            cases.push(Case::new(vec![val()]));
            cases.push(Case::new(vec![p(I32), val(), p(I64), val(), p(d.clone())]));
            cases.push(Case::new(fill(6).chain([p(I8), val(), p(I8), val(), p(I128)]).collect()));
        }
    }

    let aligned = [
        arr(I64, 2),
        arr(I64, 3),
        arr(Ty::Float, 3),
        arr(d.clone(), 2),
        arr(d.clone(), 4),
        arr(Ty::Fp128, 2),
        arr(Ty::Fp128, 1),
        arr(d.clone(), 1),
        arr(vec(I32, 4), 1),
        arr(Ty::Half, 3),
        arr(vec(I32, 4), 2),
        arr(I32, 2),
        I64,
        I128,
        I8,
        d.clone(),
        Ty::Fp128,
        st(&[I64, I64]),
        st(&[d.clone(), d.clone()]),
        vec(I32, 4),
    ];
    for ty in &aligned {
        for align in [1, 4, 8, 16, 32] {
            let val = || pa(ty.clone(), Attr::StackAlign(align));
            let fp = |count: usize| std::iter::repeat_n(p(d.clone()), count);
            cases.push(Case::new(vec![p(I64), val(), p(I64)]));
            cases.push(Case::new(
                fill(6).chain(fp(8)).chain([p(I8), val(), p(I8), val(), p(I8)]).collect(),
            ));
            cases.push(Case::new(
                fill(5).chain(fp(7)).chain([val(), p(I64), p(d.clone())]).collect(),
            ));
        }
    }
    cases
}

/// §5: return values of the types §3.3 refuses as arguments.
fn returns_only() -> Vec<Case> {
    let types = [
        arr(I128, 1),
        arr(I128, 2),
        arr(I128, 4),
        arr(I128, 5),
        arr(Ty::Int(65), 2),
        arr(Ty::Int(256), 2),
        arr(Ty::Int(256), 3),
        arr(Ty::BFloat, 2),
        arr(Ty::BFloat, 8),
        arr(Ty::BFloat, 9),
        arr(vec(I64, 4), 2),
        arr(vec(I64, 4), 4),
        arr(vec(I64, 4), 5),
        arr(vec(I64, 3), 2),
        arr(vec(I64, 3), 3),
        arr(vec(Ty::Float, 5), 1),
        arr(vec(Ty::Float, 5), 2),
        arr(vec(I8, 3), 2),
        arr(vec(I8, 3), 3),
        arr(vec(Ty::Fp128, 2), 2),
        st(&[arr(I128, 2), arr(Ty::BFloat, 2)]),
        // §5.1: the return table.
        st(&[Ty::Double, Ty::Double, Ty::Double]),
        st(&[Ty::Float, Ty::Float, Ty::Float, Ty::Float]),
        st(&[Ty::Half, Ty::Half, Ty::Half]),
        st(&[Ty::Half, Ty::Half, Ty::Half, Ty::Half]),
        st(&[Ty::Fp128, Ty::Fp128, Ty::Fp128]),
        st(&[Ty::X86Fp80, Ty::X86Fp80, Ty::X86Fp80]),
        st(&[Ty::Double, Ty::X86Fp80, Ty::Double, Ty::Double]),
        st(&[Ty::Half, Ty::Float, Ty::Double]),
        st(&[Ty::X86Fp80, I64, Ty::Double, Ty::X86Fp80]),
        st(&[I128, I64]),
        st(&[I64, I128]),
        Ty::Int(192),
        Ty::Int(193),
        Ty::Int(256),
        vec(Ty::Float, 16),
        vec(Ty::Float, 32),
        vec(Ty::Double, 3),
        vec(Ty::Float, 6),
        vec(I8, 3),
        vec(I64, 3),
        vec(I64, 4),
        vec(I1, 4),
    ];
    types
        .into_iter()
        .flat_map(|ty| [Case::of(&[]).ret(ty.clone()), Case::of(&[I64, Ty::Double]).ret(ty)])
        .collect()
}

/// §1.2: the conventions that are `ccc`.
fn conventions() -> Vec<Case> {
    let d = Ty::Double;
    let mut cases = Vec::new();
    for cc in [CallConv::FAST, CallConv::SWIFT, CallConv::CXX_FAST_TLS] {
        cases.extend(
            [
                Case::of(&[I64, d.clone(), I128, arr(I64, 2)]).ret(I64),
                Case::of(&seq(&[(&I64, 9), (&d, 9), (&I8, 2), (&I128, 1)])).ret(arr(d.clone(), 4)),
                Case::new(vec![pa(Ty::Ptr, Attr::Sret), byval(arr(I8, 20), None, None), p(I8)]),
                Case::of(&[arr(Ty::Float, 3), I8]).ret(st(&[I64, I64])),
                Case::of(&[st(&[I64, d.clone()]), vec(I32, 4), Ty::Fp128]).ret(arr(I64, 9)),
                Case::new(vec![pa(I8, Attr::Ext(true)), pa(I16, Attr::Ext(false))])
                    .ret_ext(I1, false),
            ]
            .map(|case| case.cc(cc)),
        );
        if cc == CallConv::FAST {
            // §5.1: a third `float` or `double` return register.
            let f = Ty::Float;
            cases.extend(
                [
                    Case::of(std::slice::from_ref(&d)).ret(st(&[d.clone(), d.clone(), d.clone()])),
                    Case::of(&[I8]).ret(st(&[f.clone(), Ty::Half, d.clone(), I64])),
                    Case::of(std::slice::from_ref(&f)).ret(vec(d.clone(), 3)),
                    Case::of(std::slice::from_ref(&f)).ret(st(&[Ty::Half, Ty::Half, d.clone()])),
                    Case::of(&[]).ret(st(&[d.clone(), d.clone(), d.clone(), d.clone()])),
                    Case::of(&[]).ret(vec(f, 5)),
                ]
                .map(|case| case.cc(cc)),
            );
        }
        if cc != CallConv::FAST {
            // A variadic `fastcc` function is not valid IR (§1.2).
            cases.extend(
                [
                    Case::of(&[I64, d.clone(), I128, I8, Ty::Float]).variadic(1),
                    Case::of(&seq(&[(&I64, 6), (&d, 8), (&I8, 2), (&d, 1)])).variadic(9),
                ]
                .map(|case| case.cc(cc)),
            );
        }
    }
    cases
}

/// The cases of `cases` that `assign` accepts, without duplicates, and how
/// many it refused under each name.
pub fn accepted(cases: Vec<Case>) -> (Vec<Case>, BTreeMap<&'static str, usize>) {
    let mut types = table();
    let mut refused = BTreeMap::new();
    let mut seen = HashSet::new();
    let mut kept = Vec::with_capacity(cases.len());
    for case in cases {
        match case.assign(&mut types) {
            Ok(_) => {
                if seen.insert(case.clone()) {
                    kept.push(case);
                }
            }
            Err(name) => *refused.entry(name).or_insert(0) += 1,
        }
    }
    (kept, refused)
}

/// The systematic grid.
pub fn systematic() -> Vec<Case> {
    let mut cases = Vec::new();
    for ty in all_types() {
        cases.extend(pressures(&ty));
    }
    cases.extend(extensions());
    cases.extend(attributes());
    cases.extend(returns_only());
    cases.extend(conventions());
    cases
}

fn random_type(rng: &mut Rng, pool: &[Ty], depth: u32) -> Ty {
    let roll = rng.below(100);
    if depth < 2 && roll >= 82 {
        let len = match rng.below(24) {
            0 => 0,
            1 | 2 => 5 + rng.below(8),
            _ => 1 + rng.below(4),
        };
        return arr(random_type(rng, pool, depth + 1), len);
    }
    if depth < 2 && roll >= 64 {
        let count = if rng.chance(24) { 0 } else { 1 + rng.below(4) };
        let fields = (0..count).map(|_| random_type(rng, pool, depth + 1)).collect();
        return if rng.chance(8) { Ty::Packed(fields) } else { Ty::Struct(fields) };
    }
    if roll < 30 {
        // The types most signatures are made of.
        return rng.pick(&[I32, I64, Ty::Ptr, Ty::Double, Ty::Float, I8, I128, I1, I16]).clone();
    }
    rng.pick(pool).clone()
}

/// One random signature; it may be one `assign` refuses.
fn random_case(rng: &mut Rng, pool: &[Ty]) -> Case {
    let count = if rng.chance(4) { rng.below(20) } else { rng.below(9) } as usize;
    let variadic = rng.chance(8);
    let fixed = if variadic { rng.below(count as u64 + 1) as usize } else { count };
    let mut nested = false;
    let mut params = Vec::with_capacity(count);
    for index in 0..count {
        let roll = rng.below(100);
        let named = index < fixed;
        let param: Param = if named && index < 2 && roll < 3 {
            pa(Ty::Ptr, Attr::Sret)
        } else if named && roll < 8 {
            let pointee = random_type(rng, pool, 0);
            let align = *rng.pick(&[None, None, Some(1), Some(4), Some(8), Some(16)]);
            let stack_align = *rng.pick(&[None, None, None, Some(8), Some(16)]);
            byval(pointee, align, stack_align)
        } else if named && roll < 10 && !nested {
            nested = true;
            pa(Ty::Ptr, Attr::Nest)
        } else if named && roll < 12 {
            pa(Ty::Ptr, Attr::ByRef)
        } else {
            let ty = random_type(rng, pool, 0);
            if ty.int_bits().is_some() && roll < 30 {
                pa(ty, Attr::Ext(rng.chance(2)))
            } else if roll < 36 {
                pa(ty, Attr::StackAlign(*rng.pick(&[4, 8, 16, 16, 32])))
            } else {
                p(ty)
            }
        };
        params.push(param);
    }
    // At most one `sret`.
    if params.len() > 1 && params[0].attr == Attr::Sret && params[1].attr == Attr::Sret {
        params[1] = p(I64);
    }
    let has_sret = params.iter().any(|param| param.attr == Attr::Sret);
    let mut case = Case::new(params);
    // LLVM wants `void` with `sret`.
    if !rng.chance(3) && !has_sret {
        let ty = random_type(rng, pool, 0);
        let ext = ty.int_bits().is_some() && rng.chance(4);
        case = if ext { case.ret_ext(ty, rng.chance(2)) } else { case.ret(ty) };
    }
    if variadic {
        case = case.variadic(fixed);
    }
    if rng.chance(10) {
        case = case.cc(*rng.pick(&[CallConv::FAST, CallConv::SWIFT, CallConv::CXX_FAST_TLS]));
    }
    case
}

/// `count` distinct random signatures that `assign` accepts, drawn from
/// `seed`, and how many it refused under each name on the way.
pub fn random(seed: u64, count: usize) -> (Vec<Case>, BTreeMap<&'static str, usize>) {
    let mut rng = Rng(seed);
    let mut types = table();
    // Every scalar and vector, and the unsupported shapes' neighbours.
    let mut pool = scalars();
    pool.extend(vectors());
    let mut cases = Vec::with_capacity(count);
    let mut refused = BTreeMap::new();
    let mut seen = HashSet::new();
    while cases.len() < count {
        let case = random_case(&mut rng, &pool);
        match case.assign(&mut types) {
            Ok(_) => {
                if seen.insert(case.clone()) {
                    cases.push(case);
                }
            }
            Err(name) => *refused.entry(name).or_insert(0) += 1,
        }
    }
    (cases, refused)
}

/// The cases of one object file.
const BATCH: usize = 400;

/// Runs `cases` against every `llc`, a batch at a time; three
/// configurations of a batch run side by side.
pub fn run_cases(label: &str, cases: &[Case], seed: u64, tools: &Tools) -> Report {
    let mut report = Report::default();
    for (number, chunk) in cases.chunks(BATCH).enumerate() {
        let batch = Batch::new(label, chunk, seed + number as u64, tools);
        let mut reports: Vec<Report> = Vec::new();
        // Three configurations at a time: the host is shared.
        for group in tools.llc.chunks(3) {
            reports.extend(std::thread::scope(|scope| {
                let runs: Vec<_> = group
                    .iter()
                    .map(|(config, llc)| scope.spawn(|| batch.run(*config, llc, tools)))
                    .collect();
                runs.into_iter().map(|run| run.join().expect("a batch run")).collect::<Vec<_>>()
            }));
        }
        for mut one in reports {
            for failure in &mut one.failures {
                failure.insert_str(0, &format!("batch {number} "));
            }
            report.add(one);
        }
    }
    report
}

/// The names the systematic grid's refused signatures have.
const GRID_REFUSALS: [&str; 4] = ["abi:cc", "x64:mask-vector", "x64:nest", "x64:ret-x87-part"];

/// Whether the whole grid runs: `HOD_ABI_GRID=full`.
pub fn full() -> bool {
    std::env::var_os("HOD_ABI_GRID").is_some_and(|value| value == "full")
}

/// `spec/abi-x64.md` §3–§7, §9, §11: every type under every pressure,
/// every attribute and every convention, against LLVM 22 and 19, with hod
/// calling and with LLVM calling.
#[test]
fn systematic_grid() {
    let Some(tools) = Tools::find() else { return };
    let (cases, refused) = accepted(systematic());
    // What the grid holds that §10 refuses.
    assert_eq!(refused.keys().copied().collect::<Vec<_>>(), GRID_REFUSALS);
    let report = run_cases("abi-grid", &cases, 0x5eed, &tools);
    eprintln!(
        "systematic grid: {} signatures; {}; refused: {refused:?}; llc crashed on: {:#?}",
        cases.len(),
        report.summary(),
        report.llc_crashes
    );
    report.check("systematic grid");
}

/// `spec/abi-x64.md` §11: seeded random signatures; a few hundred in the
/// quick gate, several thousand under `HOD_ABI_GRID=full`.
#[test]
fn random_grid() {
    let Some(tools) = Tools::find() else { return };
    let seeds: &[(u64, usize)] =
        if full() { &[(1, 2000), (2, 2000), (3, 2000), (4, 2000)] } else { &[(1, 400)] };
    for &(seed, count) in seeds {
        let (cases, refused) = random(seed, count);
        let report = run_cases("abi-random", &cases, seed, &tools);
        eprintln!(
            "random grid, seed {seed}: {} signatures; {}; refused while drawing: {refused:?}; \
             llc crashed on: {:#?}",
            cases.len(),
            report.summary(),
            report.llc_crashes
        );
        report.check("random grid");
    }
}
