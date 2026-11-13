//! Signatures whose assignment is written out by hand: the examples of
//! `spec/abi-x64.md`, checked without any tool.

use std::collections::BTreeSet;

use hod_cg::abi::{Assignment, Part, PartKind, Place, RetMemory, VaInfo};
use hod_ir::CallConv;
use hod_mc_x64::{Reg, Xmm};
use hod_x64::abi::{Features, INDIRECT_RESULT, RETURNS_ADDRESS, assign, cc_is_c};
use hod_x64::reg;

use crate::sigs::{Attr, Case, assumed_bits, byval, p, pa, produced_bits, table};
use crate::ty::{I1, I8, I16, I32, I64, I128, Ty, arr, st, vec};

// The argument registers by name.
const RAX: u8 = 0;
const RCX: u8 = 1;
const RDX: u8 = 2;
const RDI: u8 = 7;
const ARGS: [u8; 6] = [RDI, 6, RDX, RCX, 8, 9];

fn g(n: u8) -> Place {
    Place::Reg(reg::gpr(Reg::new(n).expect("a general register")))
}

/// Argument register `n` (`rdi` 0 … `r9` 5).
fn a(n: usize) -> Place {
    g(ARGS[n])
}

fn x(n: u8) -> Place {
    Place::Reg(reg::xmm(Xmm::new(n).expect("a vector register")))
}

fn st87(n: u8) -> Place {
    Place::Reg(reg::st(n))
}

fn s(offset: u32) -> Place {
    Place::Stack(offset)
}

/// Bytes `offset..offset + size` of the value of argument `arg`.
fn val(arg: u32, offset: u32, size: u32, place: Place) -> Part {
    Part { arg, kind: PartKind::Value, offset, size, place }
}

/// The `size` bytes argument `arg` points to.
fn mem(arg: u32, size: u32, place: Place) -> Part {
    Part { arg, kind: PartKind::Pointee, offset: 0, size, place }
}

/// `count` 8-byte arguments from argument `first` on, in the places
/// `place(0)`, `place(1)`…
fn fill(first: u32, count: u8, place: impl Fn(u8) -> Place) -> Vec<Part> {
    (0..count).map(|n| val(first + u32::from(n), 0, 8, place(n))).collect()
}

fn of(count: usize, ty: &Ty) -> Vec<Ty> {
    vec![ty.clone(); count]
}

struct Example {
    case: Case,
    args: Vec<Part>,
    ret: Vec<Part>,
    memory: bool,
    stack: u32,
    va: (u8, u8, u32),
}

fn example(case: Case, args: Vec<Part>, stack: u32) -> Example {
    Example { case, args, ret: Vec::new(), memory: false, stack, va: (0, 0, 0) }
}

impl Example {
    fn ret(mut self, ret: Vec<Part>) -> Example {
        self.ret = ret;
        self
    }

    fn memory(mut self) -> Example {
        self.memory = true;
        self
    }

    fn va(mut self, gpr: u8, fpr: u8, stack: u32) -> Example {
        self.va = (gpr, fpr, stack);
        self
    }

    fn check(&self) {
        let mut types = table();
        let text = self.case.text();
        let got = self.case.assign(&mut types).unwrap_or_else(|name| panic!("`{text}`: {name}"));
        // §9.1: `al` counts the vector registers the arguments use.
        let xmms: BTreeSet<u8> = self
            .args
            .iter()
            .filter_map(|part| match part.place {
                Place::Reg(r) if reg::is_fpr(r) => Some(reg::number(r)),
                _ => None,
            })
            .collect();
        let want = Assignment {
            args: self.args.clone(),
            ret: self.ret.clone(),
            ret_memory: self.memory.then_some(RetMemory {
                address: Place::Reg(INDIRECT_RESULT),
                returned_in: Some(RETURNS_ADDRESS),
            }),
            sret_returned_in: self.case.has_sret().then_some(RETURNS_ADDRESS),
            stack_size: self.stack,
            va: VaInfo { gpr_used: self.va.0, fpr_used: self.va.1, stack_offset: self.va.2 },
            vector_regs_used: xmms.len() as u8,
        };
        assert_eq!(got, want, "`{text}`");
    }
}

fn check(examples: Vec<Example>) {
    for example in examples {
        example.check();
    }
}

fn cat(lists: &[&[Ty]]) -> Case {
    Case::of(&lists.concat())
}

/// `spec/abi-x64.md` §3.1, §3.2, §4.1: scalars in `rdi`..`r9` and
/// `xmm0`..`xmm7`, independently; each stack slot is 8 bytes, 16 aligned
/// to 16 for `fp128`, `x86_fp80` and a vector.
#[test]
fn scalars() {
    let (d, f, h, q) = (Ty::Double, Ty::Float, Ty::Half, Ty::Fp128);
    check(vec![
        example(Case::of(&[]), vec![], 0),
        example(
            Case::of(&[I32, Ty::Ptr, d.clone(), f.clone(), I8, h.clone(), Ty::BFloat, q.clone()])
                .ret(I64),
            vec![
                val(0, 0, 4, a(0)),
                val(1, 0, 8, a(1)),
                val(2, 0, 8, x(0)),
                val(3, 0, 4, x(1)),
                val(4, 0, 1, a(2)),
                val(5, 0, 2, x(2)),
                val(6, 0, 2, x(3)),
                val(7, 0, 16, x(4)),
            ],
            0,
        )
        .ret(vec![val(0, 0, 8, g(RAX))]),
        // Seven of each: the seventh general one and the ninth vector
        // one on the stack, in argument order.
        example(
            cat(&[&of(6, &I64), &of(8, &d), &[I16, f.clone(), I1]]),
            [
                fill(0, 6, |n| a(n as usize)),
                fill(6, 8, x),
                vec![val(14, 0, 2, s(0)), val(15, 0, 4, s(8)), val(16, 0, 1, s(16))],
            ]
            .concat(),
            24,
        ),
        // `fp128` and `x86_fp80` on the stack, 16 aligned to 16; the
        // `x86_fp80` always.
        example(
            cat(&[&of(8, &d), &[I8, q.clone(), Ty::X86Fp80, h.clone()]]),
            [
                fill(0, 8, x),
                vec![
                    val(8, 0, 1, a(0)),
                    val(9, 0, 16, s(0)),
                    val(10, 0, 10, s(16)),
                    val(11, 0, 2, s(32)),
                ],
            ]
            .concat(),
            40,
        ),
        example(
            Case::of(&[Ty::X86Fp80, d.clone()]),
            vec![val(0, 0, 10, s(0)), val(1, 0, 8, x(0))],
            16,
        ),
    ]);
}

/// `spec/abi-x64.md` §4.2: an integer wider than 64 bits in 8-byte
/// pieces; an `i128` alone in two registers or 16 bytes of stack aligned
/// to 16, never split, leaving the registers it does not take.
#[test]
fn wide_integers() {
    check(vec![
        example(
            Case::of(&[I64, I128, Ty::Int(96)]),
            vec![
                val(0, 0, 8, a(0)),
                val(1, 0, 8, a(1)),
                val(1, 8, 8, a(2)),
                val(2, 0, 8, a(3)),
                val(2, 8, 4, a(4)),
            ],
            0,
        ),
        // Five registers taken: the `i128` goes to the stack and the
        // `i64` after it still gets `r9` (LLVM 22; LLVM 19 gives `r9` up).
        example(
            cat(&[&of(5, &I64), &[I128, I64]]),
            [fill(0, 5, |n| a(n as usize)), vec![val(5, 0, 16, s(0)), val(6, 0, 8, a(5))]].concat(),
            16,
        ),
        // Any other wide integer straddles: `i192` after four.
        example(
            cat(&[&of(4, &I64), &[Ty::Int(192), I8]]),
            [
                fill(0, 4, |n| a(n as usize)),
                vec![
                    val(4, 0, 8, a(4)),
                    val(4, 8, 8, a(5)),
                    val(4, 16, 8, s(0)),
                    val(5, 0, 1, s(8)),
                ],
            ]
            .concat(),
            16,
        ),
        // In an aggregate an `i128` is two independent pieces (LLVM 22;
        // LLVM 19 passes it whole on the stack).
        example(
            cat(&[&of(5, &I64), &[st(&[I128])]]),
            [fill(0, 5, |n| a(n as usize)), vec![val(5, 0, 8, a(5)), val(5, 8, 8, s(0))]].concat(),
            8,
        ),
    ]);
}

/// `spec/abi-x64.md` §4.4: an aggregate leaf by leaf, each piece in the
/// next register of its class or the next slot, padding skipped.
#[test]
fn aggregates() {
    let (d, f) = (Ty::Double, Ty::Float);
    check(vec![
        example(
            Case::of(&[st(&[I8, I32, d.clone()]), arr(f.clone(), 3)]),
            vec![
                val(0, 0, 1, a(0)),
                val(0, 4, 4, a(1)),
                val(0, 8, 8, x(0)),
                val(1, 0, 4, x(1)),
                val(1, 4, 4, x(2)),
                val(1, 8, 4, x(3)),
            ],
            0,
        ),
        // Split between the last register and the stack.
        example(
            cat(&[&of(5, &I64), &[arr(I64, 3)]]),
            [fill(0, 5, |n| a(n as usize)), vec![val(5, 0, 8, a(5)), val(5, 8, 16, s(0))]].concat(),
            16,
        ),
        // No leaf, nothing passed.
        example(Case::of(&[st(&[]), arr(I64, 0), I8]), vec![val(2, 0, 1, a(0))], 0),
        example(
            Case::of(&[st(&[Ty::X86Fp80, I8])]),
            vec![val(0, 0, 10, s(0)), val(0, 16, 1, a(0))],
            16,
        ),
    ]);
}

/// `spec/abi-x64.md` §4.3: vectors without AVX: one `xmm` up to 16 bytes
/// (widened), split into `xmm`s when longer and a power of two, element
/// by element otherwise; `<N x i1>` as lanes of one `xmm`.
#[test]
fn vectors() {
    let (f, h) = (Ty::Float, Ty::Half);
    check(vec![
        example(
            Case::of(&[vec(I32, 4), vec(f.clone(), 2), vec(I8, 3), vec(f.clone(), 3)]),
            vec![
                val(0, 0, 16, x(0)),
                val(1, 0, 8, x(1)),
                val(2, 0, 1, a(0)),
                val(2, 1, 1, a(1)),
                val(2, 2, 1, a(2)),
                val(3, 0, 12, x(2)),
            ],
            0,
        ),
        example(
            Case::of(&[vec(f.clone(), 8), vec(I64, 1), vec(h.clone(), 3), vec(I64, 3)]),
            vec![
                val(0, 0, 16, x(0)),
                val(0, 16, 16, x(1)),
                // WP-174: the value is an `xmm`, the part a general
                // register: a lane move.
                Part { kind: hod_x64::abi::LONE_LANE, ..val(1, 0, 8, a(0)) },
                val(2, 0, 6, x(2)),
                val(3, 0, 8, a(1)),
                val(3, 8, 8, a(2)),
                val(3, 16, 8, a(3)),
            ],
            0,
        ),
        example(
            Case::of(&[vec(I1, 4), vec(I1, 16), vec(I1, 1)]),
            vec![
                Part {
                    arg: 0,
                    kind: PartKind::Lanes { bits: 1, lane: 4, first: 0, len: 4 },
                    offset: 0,
                    size: 16,
                    place: x(0),
                },
                Part {
                    arg: 1,
                    kind: PartKind::Lanes { bits: 1, lane: 1, first: 0, len: 16 },
                    offset: 0,
                    size: 16,
                    place: x(1),
                },
                val(2, 0, 1, a(0)),
            ],
            0,
        ),
        // On the stack, 16 bytes aligned to 16 for each `xmm` piece.
        example(
            cat(&[&of(8, &Ty::Double), &[I8, vec(I8, 4), vec(f, 6)]]),
            [
                fill(0, 8, x),
                vec![val(8, 0, 1, a(0)), val(9, 0, 4, s(0))],
                (0..6).map(|n| val(10, 4 * n, 4, s(16 + 8 * n))).collect(),
            ]
            .concat(),
            64,
        ),
        // `<1 x half>` is widened, not scalarized: a 16-byte slot, where
        // a `half` takes 8.
        example(
            cat(&[&of(8, &Ty::Double), &[vec(h.clone(), 1), h.clone(), vec(Ty::BFloat, 1)]]),
            [fill(0, 8, x), vec![val(8, 0, 2, s(0)), val(9, 0, 2, s(16)), val(10, 0, 2, s(32))]]
                .concat(),
            48,
        ),
    ]);
}

/// `spec/abi-x64.md` §5: return values in `rax`, `rdx`, `rcx`; `xmm0`..;
/// `st0`, `st1`; in memory when they do not fit.
#[test]
fn returns() {
    let (d, f, h, q, e) = (Ty::Double, Ty::Float, Ty::Half, Ty::Fp128, Ty::X86Fp80);
    let ret = |ty: Ty| Case::of(&[]).ret(ty);
    check(vec![
        example(ret(st(&[I64, d.clone(), I32, f.clone()])), vec![], 0).ret(vec![
            val(0, 0, 8, g(RAX)),
            val(0, 8, 8, x(0)),
            val(0, 16, 4, g(RDX)),
            val(0, 20, 4, x(1)),
        ]),
        example(ret(st(&[I128, I64])), vec![], 0).ret(vec![
            val(0, 0, 8, g(RAX)),
            val(0, 8, 8, g(RDX)),
            val(0, 16, 8, g(RCX)),
        ]),
        example(ret(Ty::Int(192)), vec![], 0).ret(vec![
            val(0, 0, 8, g(RAX)),
            val(0, 8, 8, g(RDX)),
            val(0, 16, 8, g(RCX)),
        ]),
        example(ret(st(&[h.clone(), h.clone(), h.clone()])), vec![], 0).ret(vec![
            val(0, 0, 2, x(0)),
            val(0, 2, 2, x(1)),
            val(0, 4, 2, x(2)),
        ]),
        example(ret(vec(f.clone(), 16)), vec![], 0)
            .ret((0..4).map(|n| val(0, 16 * n, 16, x(n as u8))).collect()),
        example(ret(e.clone()), vec![], 0).ret(vec![val(0, 0, 10, st87(0))]),
        example(ret(st(&[e.clone(), e.clone()])), vec![], 0)
            .ret(vec![val(0, 0, 10, st87(0)), val(0, 16, 10, st87(1))]),
        example(ret(st(&[e.clone(), d.clone()])), vec![], 0)
            .ret(vec![val(0, 0, 10, st87(0)), val(0, 16, 8, x(0))]),
        example(ret(vec(I8, 3)), vec![], 0).ret(vec![
            val(0, 0, 1, g(RAX)),
            val(0, 1, 1, g(RDX)),
            val(0, 2, 1, g(RCX)),
        ]),
        // In memory: the address in `rdi`, the arguments after it.
        example(
            Case::of(&[I64, d.clone()]).ret(arr(I64, 4)),
            vec![val(0, 0, 8, a(1)), val(1, 0, 8, x(0))],
            0,
        )
        .memory(),
        example(ret(Ty::Int(256)), vec![], 0).memory(),
        example(ret(st(&[h.clone(), h.clone(), h.clone(), h])), vec![], 0).memory(),
        example(ret(st(&[q.clone(), q.clone(), q])), vec![], 0).memory(),
        example(ret(st(&[e.clone(), e.clone(), e])), vec![], 0).memory(),
        example(ret(vec(f.clone(), 32)), vec![], 0).memory(),
        example(ret(vec(f, 6)), vec![], 0).memory(),
        example(ret(arr(I128, 2)), vec![], 0).memory(),
    ]);
    // §5.1: a `float` or `double` LLVM returns in `st0`/`st1`, converted
    // to the x87 format there; `X64::assign` refuses it until WP-131.
    check(vec![
        example(ret(st(&[d.clone(), d.clone(), d.clone()])), vec![], 0).ret(vec![
            val(0, 0, 8, x(0)),
            val(0, 8, 8, x(1)),
            val(0, 16, 8, st87(0)),
        ]),
        example(ret(vec(d.clone(), 3)), vec![], 0).ret(vec![
            val(0, 0, 8, x(0)),
            val(0, 8, 8, x(1)),
            val(0, 16, 8, st87(0)),
        ]),
        example(ret(st(&[I8, d.clone(), d.clone(), Ty::Float])), vec![], 0).ret(vec![
            val(0, 0, 1, g(RAX)),
            val(0, 8, 8, x(0)),
            val(0, 16, 8, x(1)),
            val(0, 24, 4, st87(0)),
        ]),
        example(ret(st(&[Ty::Float, Ty::Float, Ty::Float, Ty::Float])), vec![], 0).ret(vec![
            val(0, 0, 4, x(0)),
            val(0, 4, 4, x(1)),
            val(0, 8, 4, st87(0)),
            val(0, 12, 4, st87(1)),
        ]),
    ]);
}

/// `spec/abi-x64.md` §6: `sret` an ordinary pointer handed back in
/// `rax`; `byval` copied to the stack at its alignment, at least 8;
/// `byref` a pointer; `alignstack` and `inreg` change nothing.
#[test]
fn attributes() {
    let sret = || pa(Ty::Ptr, Attr::Sret);
    check(vec![
        example(Case::new(vec![sret(), p(I64)]), vec![val(0, 0, 8, a(0)), val(1, 0, 8, a(1))], 0),
        example(Case::new(vec![p(I64), sret()]), vec![val(0, 0, 8, a(0)), val(1, 0, 8, a(1))], 0),
        example(
            Case::new(vec![
                byval(st(&[I64, I64, I64]), Some(8), None),
                p(I8),
                byval(arr(Ty::Double, 3), Some(16), None),
                byval(I8, None, None),
            ]),
            vec![mem(0, 24, s(0)), val(1, 0, 1, a(0)), mem(2, 24, s(32)), mem(3, 1, s(56))],
            64,
        ),
        // `alignstack` on anything but `byval` is ignored.
        example(
            Case::new(vec![
                p(I64),
                pa(arr(I64, 2), Attr::StackAlign(16)),
                pa(Ty::Ptr, Attr::ByRef),
            ]),
            vec![val(0, 0, 8, a(0)), val(1, 0, 8, a(1)), val(1, 8, 8, a(2)), val(2, 0, 8, a(3))],
            0,
        ),
    ]);
}

/// `spec/abi-x64.md` §9.1, §9.2: variable arguments are assigned as named
/// ones; `VaInfo` counts the registers the named ones take and where the
/// stack ones end.
#[test]
fn variadic() {
    let d = Ty::Double;
    check(vec![
        example(
            Case::of(&[I32, d.clone(), I64, d.clone()]).variadic(2),
            vec![val(0, 0, 4, a(0)), val(1, 0, 8, x(0)), val(2, 0, 8, a(1)), val(3, 0, 8, x(1))],
            0,
        )
        .va(1, 1, 0),
        example(
            cat(&[&of(7, &I64), &[d.clone(), I8]]).variadic(8),
            [
                fill(0, 6, |n| a(n as usize)),
                vec![val(6, 0, 8, s(0)), val(7, 0, 8, x(0)), val(8, 0, 1, s(8))],
            ]
            .concat(),
            16,
        )
        .va(6, 1, 8),
        // An odd end of the named stack arguments rounds up to 8.
        example(
            cat(&[&of(6, &I64), &[I8, I8]]).variadic(7),
            [fill(0, 6, |n| a(n as usize)), vec![val(6, 0, 1, s(0)), val(7, 0, 1, s(8))]].concat(),
            16,
        )
        .va(6, 0, 8),
        example(Case::of(&[]).variadic(0), vec![], 0),
    ]);
}

/// `spec/abi-x64.md` §1.2: `fastcc` is `ccc`; every other convention is
/// refused.
#[test]
fn conventions() {
    assert!(cc_is_c(CallConv::C) && cc_is_c(CallConv::FAST));
    for cc in [CallConv::COLD, CallConv::SWIFT, CallConv::CXX_FAST_TLS, CallConv::TAIL] {
        assert!(!cc_is_c(cc));
    }
    let case = Case::of(&[I64, Ty::Double, I128]).ret(st(&[I64, I64])).cc(CallConv::FAST);
    let mut types = table();
    let fast = case.assign(&mut types).expect("fastcc");
    let c = case.cc(CallConv::C).assign(&mut types).expect("ccc");
    assert_eq!(fast, c);
    // §5.1: but `fastcc` returns a third `float` or `double` in `xmm2`,
    // where `ccc` would use `st0`.
    let three = Case::of(&[]).ret(st(&[Ty::Double, Ty::Float, Ty::Double]));
    let fast = three.clone().cc(CallConv::FAST).assign(&mut types).expect("fastcc");
    let places: Vec<Place> = fast.ret.iter().map(|part| part.place).collect();
    assert_eq!(places, [0, 1, 2].map(x));
    let c = three.cc(CallConv::C).assign(&mut types).expect("ccc");
    let places: Vec<Place> = c.ret.iter().map(|part| part.place).collect();
    assert_eq!(places, [x(0), x(1), st87(0)]);
}

/// `spec/abi-x64.md` §7: the two tables, row by row.
#[test]
fn extension_tables() {
    type Row = (u32, Option<bool>, bool, (bool, u32));
    let rdi = g(RDI);
    let stack = Place::Stack(0);
    // (bits, ext, ret, extended to): `(false, bits)` when nothing is
    // owed.
    let rows: [Row; 16] = [
        (1, None, false, (false, 1)),
        (1, Some(false), false, (false, 32)),
        (8, Some(true), false, (true, 32)),
        (16, Some(false), false, (false, 32)),
        (24, Some(true), false, (true, 32)),
        (32, Some(true), false, (false, 32)),
        (48, Some(true), false, (true, 64)),
        (64, Some(false), false, (false, 64)),
        (96, Some(true), false, (true, 128)),
        (1, None, true, (false, 1)),
        (1, Some(false), true, (false, 8)),
        (8, Some(true), true, (false, 8)),
        (16, Some(true), true, (false, 16)),
        (24, Some(false), true, (false, 32)),
        (8, None, true, (false, 8)),
        (40, Some(true), true, (true, 64)),
    ];
    for (bits, ext, ret, want) in rows {
        let what = format!("i{bits} {ext:?} ret {ret}");
        assert_eq!(produced_bits(bits, ext, ret).unwrap_or((false, bits)), want, "{what}");
        assert_eq!(assumed_bits(bits, ext, ret, rdi), want, "{what}, register");
        assert_eq!(assumed_bits(bits, ext, ret, stack), want, "{what}, stack");
    }
}

/// `spec/abi-x64.md` §2: `assign` reuses the buffers of its output, and
/// an error leaves it empty.
#[test]
fn output_is_reused() {
    let mut types = table();
    let case = Case::of(&[I64, arr(I64, 9)]).ret(arr(I64, 9));
    let lowered = case.lower(&mut types);
    let mut out = Assignment::default();
    assign(&types, &lowered.sig(), Features::default(), &mut out).expect("supported");
    let capacity = out.args.capacity();
    assign(&types, &lowered.sig(), Features::default(), &mut out).expect("supported");
    assert_eq!(out.args.capacity(), capacity);
    let refused = Case::new(vec![pa(Ty::Ptr, Attr::Nest)]).lower(&mut types);
    assert!(assign(&types, &refused.sig(), Features::default(), &mut out).is_err());
    assert_eq!(out, Assignment::default());
}

/// `spec/abi-x64.md` §8.6: DWARF register numbers (`hod_x64::reg`).
#[test]
fn dwarf_numbers() {
    let gpr = |n: u8| reg::gpr(Reg::new(n).expect("a general register"));
    // psABI figure 3.36, by name.
    let named = [
        ("rax", 0, 0),
        ("rdx", 2, 1),
        ("rcx", 1, 2),
        ("rbx", 3, 3),
        ("rsi", 6, 4),
        ("rdi", 7, 5),
        ("rbp", 5, 6),
        ("rsp", 4, 7),
    ];
    for (name, encoding, dwarf) in named {
        assert_eq!(reg::dwarf(gpr(encoding)), dwarf, "{name}");
    }
    for n in 8..16 {
        assert_eq!(reg::dwarf(gpr(n)), u16::from(n), "r{n}");
    }
    for x in Xmm::ALL {
        assert_eq!(reg::dwarf(reg::xmm(x)), 17 + u16::from(x.code()), "{x:?}");
    }
    for n in 0..8 {
        assert_eq!(reg::dwarf(reg::st(n)), 33 + u16::from(n), "st{n}");
    }
}
