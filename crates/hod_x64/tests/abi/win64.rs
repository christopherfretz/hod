//! WP-246: `win64cc` (`spec/abi-x64.md` §13): assignments written out by
//! hand from `llc`'s output, the refusals by name, and a grid of
//! signatures run against `llc` 22 and 19 in both directions (a
//! hod-assembled caller of an LLVM-compiled `win64cc` callee and the
//! reverse).

use hod_cg::abi::{Part, PartKind, Place};
use hod_ir::CallConv;
use hod_mc_x64::{Reg, Xmm};
use hod_x64::abi::LONE_LANE;
use hod_x64::{reg, win64};

use crate::grid::{accepted, run_cases, seq};
use crate::run::Tools;
use crate::sigs::{Attr, Case, byval, p, pa, table};
use crate::ty::{I1, I8, I16, I32, I64, I128, Ty, arr, st, vec};

const W: CallConv = CallConv::WIN64;

fn g(reg: Reg) -> Place {
    Place::Reg(reg::gpr(reg))
}

fn x(n: u8) -> Place {
    Place::Reg(reg::xmm(Xmm::new(n).expect("a vector register")))
}

/// The places of the parts of `case`'s arguments, and its stack size.
fn places(case: &Case) -> (Vec<(u32, Place)>, u32) {
    let asg = case.assign(&mut table()).unwrap_or_else(|name| panic!("{}: {name}", case.text()));
    (asg.args.iter().map(|part| (part.arg, part.place)).collect(), asg.stack_size)
}

/// §13.2: positions shared by number between the general and the `xmm`
/// registers; 8-byte slots from offset 32 (`m1.ll` `c2`: `ecx`, `xmm1`,
/// `xmm2`, `r9d`, then `32(%rsp)`.. `56(%rsp)`).
#[test]
fn positions_are_shared() {
    let d = Ty::Double;
    let case = Case::of(&[I32, d.clone(), Ty::Float, I8, I64, d, I16, I1]).cc(W);
    let (parts, stack) = places(&case);
    assert_eq!(
        parts,
        [
            (0, g(Reg::Rcx)),
            (1, x(1)),
            (2, x(2)),
            (3, g(Reg::R9)),
            (4, Place::Stack(32)),
            (5, Place::Stack(40)),
            (6, Place::Stack(48)),
            (7, Place::Stack(56)),
        ]
    );
    assert_eq!(stack, 64);
    // No stack argument: the shadow area still.
    assert_eq!(places(&Case::of(&[]).cc(W)).1, 32);
}

/// §13.2: each half of an `i128` takes a position, split between `r9` and
/// the stack (`c1`: `rcx`, `rdx`; `r8`; `r9` and `32(%rsp)`); members of
/// an aggregate each take one (`c3`: `{ i64, double }` in `rcx`, `xmm1`,
/// `[2 x i32]` in `r8d`, `r9d`).
#[test]
fn pieces_take_positions() {
    let (parts, _) = places(&Case::of(&[I128, I64, I128]).cc(W));
    assert_eq!(
        parts,
        [
            (0, g(Reg::Rcx)),
            (0, g(Reg::Rdx)),
            (1, g(Reg::R8)),
            (2, g(Reg::R9)),
            (2, Place::Stack(32)),
        ]
    );
    let (parts, _) = places(&Case::of(&[st(&[I64, Ty::Double]), arr(I32, 2), I64]).cc(W));
    assert_eq!(
        parts,
        [(0, g(Reg::Rcx)), (0, x(1)), (1, g(Reg::R8)), (1, g(Reg::R9)), (2, Place::Stack(32)),]
    );
}

/// §13.4: `sret` is an ordinary pointer (`c6`: `rcx`, then `edx`); a
/// value returned in memory has its address in `rcx` and back in `rax`
/// (`m3.ll` `callbig`); `inreg` is ignored (`c8`).
#[test]
fn returns_and_attributes() {
    let case = Case::new(vec![pa(Ty::Ptr, Attr::Sret), p(I64)]).cc(W);
    let asg = case.assign(&mut table()).expect("sret");
    assert_eq!(asg.args[0].place, g(Reg::Rcx));
    assert_eq!(asg.args[1].place, g(Reg::Rdx));
    assert_eq!(asg.sret_returned_in, Some(hod_x64::abi::RETURNS_ADDRESS));
    let case = Case::of(&[I64]).ret(st(&[I64, I64, I64, I64])).cc(W);
    let asg = case.assign(&mut table()).expect("memory");
    let memory = asg.ret_memory.expect("in memory");
    assert_eq!(memory.address, g(Reg::Rcx));
    assert_eq!(memory.returned_in, Some(hod_x64::abi::RETURNS_ADDRESS));
    assert_eq!(asg.args[0].place, g(Reg::Rdx));
    let case = Case::new(vec![pa(I64, Attr::InReg), pa(Ty::Double, Attr::InReg)]).cc(W);
    assert_eq!(places(&case).0, [(0, g(Reg::Rcx)), (1, x(1))]);
    // Returns as under `ccc` (§5.1).
    for ret in [I128, st(&[I64, I64, I64]), st(&[Ty::Double, Ty::Double]), vec(Ty::Float, 4)] {
        let c = Case::of(&[]).ret(ret.clone());
        let w = c.clone().cc(W);
        assert_eq!(
            c.assign(&mut table()).expect("ccc").ret,
            w.assign(&mut table()).expect("win64cc").ret,
            "{}",
            ret.text()
        );
    }
}

/// §13.5: a variadic call passes a `double` in a register position in
/// the `xmm` and, as the lane move of its 8 bytes, in the position's
/// general register (`c10`: `xmm1` and `rdx`, `xmm3` and `r9`, the fifth
/// argument at `32(%rsp)`).
#[test]
fn variadic_doubles_are_copied() {
    let d = Ty::Double;
    let case = Case::of(&[I32, d.clone(), I64, d.clone(), d]).variadic(1).cc(W);
    let asg = case.assign(&mut table()).expect("variadic");
    let parts: Vec<(u32, Place, bool)> =
        asg.args.iter().map(|part: &Part| (part.arg, part.place, part.kind == LONE_LANE)).collect();
    assert_eq!(
        parts,
        [
            (0, g(Reg::Rcx), false),
            (1, x(1), false),
            (1, g(Reg::Rdx), true),
            (2, g(Reg::R8), false),
            (3, x(3), false),
            (3, g(Reg::R9), true),
            (4, Place::Stack(32), false),
        ]
    );
    assert!(asg.args.iter().all(|part| part.kind == PartKind::Value || part.kind == LONE_LANE));
}

/// §13.7: what the convention refuses, by name.
#[test]
fn refusals_by_name() {
    let cases = [
        (Case::of(&[vec(Ty::Double, 2)]), win64::VECTOR),
        (Case::of(&[I64, vec(I32, 8)]), win64::VECTOR),
        (Case::of(&[Ty::X86Fp80]), win64::X86_FP80),
        (Case::of(&[Ty::Fp128]), win64::FP128),
        (Case::of(&[Ty::BFloat]), win64::BFLOAT),
        (Case::of(&[Ty::Int(96)]), win64::WIDE_INT),
        (Case::of(&[st(&[I8, Ty::Int(256)])]), win64::WIDE_INT),
        (Case::new(vec![byval(arr(I64, 3), None, None)]), win64::BYVAL),
        (Case::of(&[I32, Ty::Float]).variadic(1), win64::VARIADIC_FLOAT),
        (Case::of(&[Ty::Float]).variadic(1), win64::VARIADIC_FLOAT),
        (Case::new(vec![pa(Ty::Ptr, Attr::Nest)]), hod_x64::abi::NEST),
    ];
    for (case, name) in cases {
        let case = case.cc(W);
        assert_eq!(case.assign(&mut table()).err(), Some(name), "{}", case.text());
    }
    // A `float` past the fourth position of a variadic call is a stack
    // slot only.
    let f = Ty::Float;
    assert!(Case::of(&[I64, I64, I64, I64, f]).variadic(1).cc(W).assign(&mut table()).is_ok());
}

/// The grid's `win64cc` signatures (§13.8): integers, floats, mixed
/// positions beyond four, `i128` whole and split, aggregates, `sret`,
/// pointers, returns in registers, in memory and on the x87 stack.
pub fn cases() -> Vec<Case> {
    let (d, f, h) = (Ty::Double, Ty::Float, Ty::Half);
    let mut cases = vec![
        Case::of(&[]),
        Case::of(&[I64]).ret(I64),
        Case::of(&[I32, I32, I32, I32]).ret(I32),
        Case::of(&seq(&[(&I64, 7)])).ret(I64),
        Case::of(&seq(&[(&d, 7)])).ret(d.clone()),
        Case::of(&[I8, d.clone(), I16, f.clone(), I1, d.clone(), I32, f.clone()]).ret(f.clone()),
        Case::of(&[d.clone(), I64, d.clone(), I64, d.clone(), I64])
            .ret(st(&[d.clone(), d.clone()])),
        Case::of(&[I128, I64, I128]).ret(I128),
        Case::of(&[I64, I64, I64, I128, I8]).ret(st(&[I64, I64])),
        Case::of(&[I128, I128, I128]),
        Case::of(&[Ty::Ptr, I64, Ty::Ptr, Ty::Ptr, Ty::Ptr]).ret(Ty::Ptr),
        Case::of(&[st(&[I64, d.clone()]), arr(I32, 2), I64]).ret(st(&[I64, d.clone()])),
        Case::of(&[st(&[I8, I32]), st(&[f.clone(), I32]), arr(d.clone(), 3)]),
        Case::of(&[arr(I64, 2), st(&[I8, I128]), I16]).ret(arr(I64, 2)),
        Case::of(&[h.clone(), I64, h.clone()]).ret(h),
        Case::of(&[I64]).ret(st(&[I64, I64, I64, I64])),
        Case::of(&seq(&[(&d, 3), (&I64, 3)])).ret(arr(d.clone(), 4)),
        Case::of(std::slice::from_ref(&d)).ret(Ty::X86Fp80),
        Case::of(&[I64]).ret(vec(f.clone(), 4)),
        Case::of(&[I32]).ret(vec(I64, 2)),
        Case::of(&[I32, d.clone()]).ret(st(&[d.clone(), I64, I64])),
        Case::new(vec![pa(Ty::Ptr, Attr::Sret), p(I64), p(d.clone()), p(I32), p(I8)]),
        Case::new(vec![p(I64), pa(Ty::Ptr, Attr::Sret), p(d.clone())]),
        Case::new(vec![pa(Ty::Ptr, Attr::ByRef), p(I64), pa(I64, Attr::InReg)]),
        Case::of(&[]).ret(I1),
        Case::of(&[I8, I16]).ret(I8),
    ];
    // Every scalar the convention passes at each position and on the
    // stack.
    for ty in [I1, I8, I16, Ty::Int(24), I32, Ty::Int(48), I64, I128, Ty::Ptr, f, d] {
        for before in [0, 1, 3, 4, 6] {
            let mut params = seq(&[(&I64, before)]);
            params.push(ty.clone());
            params.push(I8);
            cases.push(Case::of(&params).ret(ty.clone()));
        }
    }
    cases.into_iter().map(|case| case.cc(W)).collect()
}

/// §13.8: the `win64cc` grid against every `llc`, hod calling and LLVM
/// calling.
#[test]
fn win64_grid() {
    let Some(tools) = Tools::find() else { return };
    let (cases, refused) = accepted(cases());
    assert!(refused.is_empty(), "refused: {refused:?}");
    let report = run_cases("abi-win64", &cases, 0x6464, &tools);
    eprintln!("win64cc grid: {} signatures; {}", cases.len(), report.summary());
    report.check("win64cc grid");
}
