//! Signatures whose assignment is written out by hand: the examples of
//! `spec/abi-a64.md`, checked without any tool.

use hod_a64::abi::{INDIRECT_RESULT, NEST, assign, cc_is_c};
use hod_a64::reg;
use hod_cg::abi::{Assignment, Part, PartKind, Place, RetMemory, VaInfo};
use hod_ir::CallConv;

use crate::sigs::{Attr, Case, assumed_bits, byval, p, pa, produced_bits, table};
use crate::ty::{I1, I8, I16, I32, I64, I128, Ty, arr, st, vec};

fn x(n: u8) -> Place {
    Place::Reg(reg::x(n))
}

fn v(n: u8) -> Place {
    Place::Reg(reg::v(n))
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

/// `count` arguments of 8 bytes from argument `first` on, in the
/// registers `place(0)`, `place(1)`…
fn fill(first: u32, count: u8, place: fn(u8) -> Place) -> Vec<Part> {
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
        let want = Assignment {
            args: self.args.clone(),
            ret: self.ret.clone(),
            ret_memory: self
                .memory
                .then_some(RetMemory { address: Place::Reg(INDIRECT_RESULT), returned_in: None }),
            sret_returned_in: None,
            stack_size: self.stack,
            va: VaInfo { gpr_used: self.va.0, fpr_used: self.va.1, stack_offset: self.va.2 },
            vector_regs_used: 0,
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

/// `spec/abi-a64.md` §3.1, §3.2, §4.1: scalars in registers and on the
/// stack; each stack slot is 8 bytes, 16 for a 128-bit value.
#[test]
fn scalars() {
    let d = Ty::Double;
    check(vec![
        example(Case::of(&[]), vec![], 0),
        example(
            Case::of(&[I32, Ty::Ptr, d.clone(), Ty::Float]).ret(I64),
            vec![val(0, 0, 4, x(0)), val(1, 0, 8, x(1)), val(2, 0, 8, v(0)), val(3, 0, 4, v(1))],
            0,
        )
        .ret(vec![val(0, 0, 8, x(0))]),
        example(Case::of(&of(9, &I64)), [fill(0, 8, x), vec![val(8, 0, 8, s(0))]].concat(), 8),
        example(
            Case::of(&[Ty::Int(24), Ty::Int(48)]),
            vec![val(0, 0, 3, x(0)), val(1, 0, 6, x(1))],
            0,
        ),
        example(
            cat(&[&of(9, &d), &[Ty::Float, Ty::Half]]),
            [fill(0, 8, v), vec![val(8, 0, 8, s(0)), val(9, 0, 4, s(8)), val(10, 0, 2, s(16))]]
                .concat(),
            24,
        ),
        example(
            cat(&[&of(8, &d), &[Ty::Float, Ty::Fp128]]),
            [fill(0, 8, v), vec![val(8, 0, 4, s(0)), val(9, 0, 16, s(16))]].concat(),
            32,
        ),
        example(
            cat(&[&of(8, &I64), &[I8, I16, I32, I1]]),
            [
                fill(0, 8, x),
                vec![
                    val(8, 0, 1, s(0)),
                    val(9, 0, 2, s(8)),
                    val(10, 0, 4, s(16)),
                    val(11, 0, 1, s(24)),
                ],
            ]
            .concat(),
            32,
        ),
        example(
            cat(&[&of(9, &Ty::Fp128), &[I8]]),
            [
                (0..8).map(|n| val(n, 0, 16, v(n as u8))).collect(),
                vec![val(8, 0, 16, s(0)), val(9, 0, 1, x(0))],
            ]
            .concat(),
            16,
        ),
    ]);
}

/// `spec/abi-a64.md` §3.2, §4.2: an integer wider than 64 bits starts at
/// an even register, or at a 16-byte boundary on the stack, and the
/// register it skips stays unused.
#[test]
fn wide_integers() {
    check(vec![
        example(
            Case::of(&[I8, I128, I64, I128, I32, I128, I128]),
            vec![
                val(0, 0, 1, x(0)),
                val(1, 0, 8, x(2)),
                val(1, 8, 8, x(3)),
                val(2, 0, 8, x(4)),
                val(3, 0, 8, x(6)),
                val(3, 8, 8, x(7)),
                val(4, 0, 4, s(0)),
                val(5, 0, 16, s(16)),
                val(6, 0, 16, s(32)),
            ],
            48,
        ),
        example(
            cat(&[&of(7, &I64), &[I128, I64]]),
            [fill(0, 7, x), vec![val(7, 0, 16, s(0)), val(8, 0, 8, s(16))]].concat(),
            24,
        ),
        example(
            Case::of(&[I64, Ty::Int(256)]),
            vec![
                val(0, 0, 8, x(0)),
                val(1, 0, 8, x(2)),
                val(1, 8, 8, x(3)),
                val(1, 16, 8, x(4)),
                val(1, 24, 8, x(5)),
            ],
            0,
        ),
        example(
            cat(&[&of(5, &I64), &[Ty::Int(256)]]),
            [fill(0, 5, x), vec![val(5, 0, 8, x(6)), val(5, 8, 8, x(7)), val(5, 16, 16, s(0))]]
                .concat(),
            16,
        ),
        example(Case::of(&[Ty::Int(65)]), vec![val(0, 0, 8, x(0)), val(0, 8, 1, x(1))], 0),
    ]);
}

/// `spec/abi-a64.md` §3.3, §4.4: structures member by member; arrays of
/// one register type as a block, all in registers or all on the stack.
#[test]
fn aggregates() {
    let (d, f) = (Ty::Double, Ty::Float);
    check(vec![
        example(
            cat(&[&of(7, &I64), &[st(&[I64, I64])]]),
            [fill(0, 7, x), vec![val(7, 0, 8, x(7)), val(7, 8, 8, s(0))]].concat(),
            8,
        ),
        example(
            Case::of(&[st(&[I8, I32]), st(&[f.clone(), I32])]),
            vec![val(0, 0, 1, x(0)), val(0, 4, 4, x(1)), val(1, 0, 4, v(0)), val(1, 4, 4, x(2))],
            0,
        ),
        example(
            cat(&[&of(8, &I64), &[st(&[I8, I128])]]),
            [fill(0, 8, x), vec![val(8, 0, 1, s(0)), val(8, 16, 16, s(16))]].concat(),
            32,
        ),
        example(Case::of(&[st(&[]), I64]), vec![val(1, 0, 8, x(0))], 0),
        example(
            Case::of(&[I64, arr(I64, 2)]),
            vec![val(0, 0, 8, x(0)), val(1, 0, 8, x(1)), val(1, 8, 8, x(2))],
            0,
        ),
        example(
            Case::new(vec![p(I64), pa(arr(I64, 2), Attr::StackAlign(16))]),
            vec![val(0, 0, 8, x(0)), val(1, 0, 8, x(1)), val(1, 8, 8, x(2))],
            0,
        ),
        example(
            cat(&[&of(7, &I64), &[arr(I64, 2), I64]]),
            [fill(0, 7, x), vec![val(7, 0, 16, s(0)), val(8, 0, 8, s(16))]].concat(),
            24,
        ),
        example(
            cat(&[&of(6, &I64), &[arr(st(&[Ty::Ptr, I64]), 2), I64]]),
            [fill(0, 6, x), vec![val(6, 0, 32, s(0)), val(7, 0, 8, s(32))]].concat(),
            40,
        ),
        example(
            Case::of(&[arr(f.clone(), 3), d.clone()]),
            vec![val(0, 0, 4, v(0)), val(0, 4, 4, v(1)), val(0, 8, 4, v(2)), val(1, 0, 8, v(3))],
            0,
        ),
        example(
            cat(&[&of(6, &d), &[arr(f.clone(), 3), f.clone()]]),
            [fill(0, 6, v), vec![val(6, 0, 12, s(0)), val(7, 0, 4, s(16))]].concat(),
            24,
        ),
        example(
            cat(&[&of(7, &d), &[arr(Ty::Fp128, 2), I32]]),
            [fill(0, 7, v), vec![val(7, 0, 32, s(0)), val(8, 0, 4, x(0))]].concat(),
            32,
        ),
        example(
            cat(&[&of(7, &d), &[arr(Ty::Half, 2)]]),
            [fill(0, 7, v), vec![val(7, 0, 4, s(0))]].concat(),
            4,
        ),
        example(
            Case::new(
                (0..8)
                    .map(|_| p(I64))
                    .chain([p(I8), pa(arr(I64, 2), Attr::StackAlign(16))])
                    .collect(),
            ),
            [fill(0, 8, x), vec![val(8, 0, 1, s(0)), val(9, 0, 16, s(16))]].concat(),
            32,
        ),
        example(
            cat(&[&of(8, &I64), &[I8, arr(I64, 2)]]),
            [fill(0, 8, x), vec![val(8, 0, 1, s(0)), val(9, 0, 16, s(8))]].concat(),
            24,
        ),
        // Not a block: each element is passed as an `i32` would be.
        example(
            Case::of(&[arr(I8, 3)]),
            vec![val(0, 0, 1, x(0)), val(0, 1, 1, x(1)), val(0, 2, 1, x(2))],
            0,
        ),
        example(
            cat(&[&of(8, &I64), &[arr(I8, 3)]]),
            [fill(0, 8, x), vec![val(8, 0, 1, s(0)), val(8, 1, 1, s(8)), val(8, 2, 1, s(16))]]
                .concat(),
            24,
        ),
    ]);
}

/// `spec/abi-a64.md` §4.3: vectors whole, widened, split and element by
/// element.
#[test]
fn vectors() {
    let (d, f) = (Ty::Double, Ty::Float);
    check(vec![
        example(
            Case::of(&[vec(I32, 4), vec(f.clone(), 2), vec(f.clone(), 3), vec(I32, 8)]),
            vec![
                val(0, 0, 16, v(0)),
                val(1, 0, 8, v(1)),
                val(2, 0, 12, v(2)),
                val(3, 0, 16, v(3)),
                val(3, 16, 16, v(4)),
            ],
            0,
        ),
        example(
            Case::of(&[vec(I8, 3)]),
            vec![val(0, 0, 1, x(0)), val(0, 1, 1, x(1)), val(0, 2, 1, x(2))],
            0,
        ),
        example(
            Case::of(&[vec(I64, 3)]),
            vec![val(0, 0, 8, v(0)), val(0, 8, 8, v(1)), val(0, 16, 8, v(2))],
            0,
        ),
        example(
            Case::of(&[vec(f.clone(), 1), vec(Ty::Half, 1), vec(I64, 1), vec(Ty::Fp128, 1)]),
            vec![val(0, 0, 4, v(0)), val(1, 0, 2, v(1)), val(2, 0, 8, v(2)), val(3, 0, 16, v(3))],
            0,
        ),
        example(
            cat(&[&of(4, &d), &[vec(I64, 6)]]),
            [
                fill(0, 4, v),
                vec![
                    val(4, 0, 8, v(4)),
                    val(4, 8, 8, v(5)),
                    val(4, 16, 8, v(6)),
                    val(4, 24, 8, v(7)),
                    val(4, 32, 16, s(0)),
                ],
            ]
            .concat(),
            16,
        ),
        example(
            Case::of(&[arr(vec(f.clone(), 3), 2)]),
            vec![val(0, 0, 12, v(0)), val(0, 16, 12, v(1))],
            0,
        ),
        example(
            cat(&[&of(7, &d), &[arr(vec(f, 3), 2)]]),
            [fill(0, 7, v), vec![val(7, 0, 12, s(0)), val(7, 16, 12, s(16))]].concat(),
            32,
        ),
    ]);
}

/// `spec/abi-a64.md` §5: return values in `x0`..`x7` and `v0`..`v7` with
/// no even-register rule, and in memory through `x8` when they do not
/// fit.
#[test]
fn returns() {
    let (d, f) = (Ty::Double, Ty::Float);
    let eight = |place: fn(u8) -> Place| (0..8).map(|n| val(0, 8 * n, 8, place(n as u8))).collect();
    check(vec![
        example(Case::of(&[]).ret(st(&[I64, d.clone(), I8, f.clone(), I128])), vec![], 0).ret(
            vec![
                val(0, 0, 8, x(0)),
                val(0, 8, 8, v(0)),
                val(0, 16, 1, x(1)),
                val(0, 20, 4, v(1)),
                val(0, 32, 8, x(2)),
                val(0, 40, 8, x(3)),
            ],
        ),
        example(Case::of(&[]).ret(Ty::Int(512)), vec![], 0).ret(eight(x)),
        example(Case::of(&[]).ret(arr(I64, 8)), vec![], 0).ret(eight(x)),
        example(Case::of(&[]).ret(arr(d, 8)), vec![], 0).ret(eight(v)),
        example(Case::of(&[]).ret(arr(I128, 2)), vec![], 0)
            .ret((0..4).map(|n| val(0, 8 * n, 8, x(n as u8))).collect()),
        example(Case::of(&[I64]).ret(arr(I64, 9)), vec![val(0, 0, 8, x(0))], 0).memory(),
        example(Case::of(&[]).ret(arr(f, 9)), vec![], 0).memory(),
        example(Case::of(&[]).ret(Ty::Int(513)), vec![], 0).memory(),
        example(Case::of(&[]).ret(st(&[])), vec![], 0),
    ]);
}

/// `spec/abi-a64.md` §6: `sret` in `x8`, `byval` copied to the stack,
/// `nest` in `x15`, `byref` and the extension attributes changing no
/// place.
#[test]
fn attributes() {
    assert_eq!((INDIRECT_RESULT, NEST), (reg::X8, reg::X15));
    check(vec![
        example(
            Case::new(vec![pa(Ty::Ptr, Attr::Sret), p(I64)]),
            vec![val(0, 0, 8, x(8)), val(1, 0, 8, x(0))],
            0,
        ),
        example(
            Case::new(vec![p(I32), byval(arr(I8, 3), Some(16), None), p(I64)]),
            vec![val(0, 0, 4, x(0)), mem(1, 3, s(0)), val(2, 0, 8, x(1))],
            8,
        ),
        example(
            Case::new(vec![byval(I8, None, None), byval(I128, None, None)]),
            vec![mem(0, 1, s(0)), mem(1, 16, s(16))],
            32,
        ),
        example(
            Case::new(vec![byval(I8, None, None), byval(I64, Some(1), Some(16))]),
            vec![mem(0, 1, s(0)), mem(1, 8, s(16))],
            24,
        ),
        example(Case::new(vec![byval(st(&[]), None, None)]), vec![mem(0, 0, s(0))], 8),
        example(
            Case::new(vec![pa(Ty::Ptr, Attr::Nest), p(I64)]),
            vec![val(0, 0, 8, x(15)), val(1, 0, 8, x(0))],
            0,
        ),
        example(Case::new(vec![pa(Ty::Ptr, Attr::ByRef)]), vec![val(0, 0, 8, x(0))], 0),
        example(
            Case::new(vec![pa(I8, Attr::Ext(false)), pa(I16, Attr::Ext(true))]).ret_ext(I1, false),
            vec![val(0, 0, 1, x(0)), val(1, 0, 2, x(1))],
            0,
        )
        .ret(vec![val(0, 0, 1, x(0))]),
    ]);
}

/// `spec/abi-a64.md` §9.1, §9.4: variable arguments are assigned as named
/// ones are; `VaInfo` counts what the named ones used.
#[test]
fn variadic() {
    let d = Ty::Double;
    check(vec![
        example(Case::of(&[]).variadic(0), vec![], 0),
        example(
            Case::of(&[I32, d.clone(), I64]).variadic(1),
            vec![val(0, 0, 4, x(0)), val(1, 0, 8, v(0)), val(2, 0, 8, x(1))],
            0,
        )
        .va(1, 0, 0),
        example(
            cat(&[&of(8, &I64), &of(8, &d), &[I8, I32]]).variadic(17),
            [fill(0, 8, x), fill(8, 8, v), vec![val(16, 0, 1, s(0)), val(17, 0, 4, s(8))]].concat(),
            16,
        )
        .va(8, 8, 8),
        // A block of 12 bytes on the stack: the variable arguments start
        // at the next multiple of 8.
        example(
            Case::new(
                (0..8)
                    .map(|_| p(d.clone()))
                    .chain([pa(arr(Ty::Float, 3), Attr::StackAlign(8))])
                    .collect(),
            )
            .variadic(9),
            [fill(0, 8, v), vec![val(8, 0, 12, s(0))]].concat(),
            12,
        )
        .va(0, 8, 16),
        example(
            Case::of(&[I64, I128]).variadic(2),
            vec![val(0, 0, 8, x(0)), val(1, 0, 8, x(2)), val(1, 8, 8, x(3))],
            0,
        )
        .va(4, 0, 0),
    ]);
}

/// `spec/abi-a64.md` §1.2: `fastcc`, `swiftcc` and `cxx_fast_tlscc` are
/// assigned as `ccc`; every other convention is `abi:cc`.
#[test]
fn conventions() {
    let mut types = table();
    let case = cat(&[&of(9, &I64), &[Ty::Double, I128, arr(I64, 2)]]).ret(arr(I64, 9));
    let c = case.assign(&mut types).expect("ccc");
    for cc in [CallConv::FAST, CallConv::SWIFT, CallConv::CXX_FAST_TLS] {
        assert!(cc_is_c(cc));
        assert_eq!(case.clone().cc(cc).assign(&mut types).as_ref(), Ok(&c), "{cc:?}");
    }
    for id in 0..=1023 {
        let cc = CallConv::from_llvm(id).expect("a convention number");
        if ![0, 8, 16, 17].contains(&id) {
            assert!(!cc_is_c(cc));
            assert_eq!(case.clone().cc(cc).assign(&mut types), Err("abi:cc"), "cc {id}");
        }
    }
}

/// `spec/abi-a64.md` §8.5: the parameter attributes a `musttail` pair may
/// differ in (LLVM's verifier allows `zeroext` and `signext` to) change
/// no place, so caller and callee have the same stack-argument area.
#[test]
fn musttail_pairs() {
    let mut types = table();
    let plain = cat(&[&of(8, &I64), &[I8, I16, I128]]).ret(I8);
    let mut extended = plain.clone();
    extended.params[8].attr = Attr::Ext(true);
    extended.params[9].attr = Attr::Ext(false);
    extended.ret_ext = Some(true);
    let (a, b) = (plain.assign(&mut types), extended.assign(&mut types));
    assert_eq!(a, b);
    assert_eq!(a.expect("supported").stack_size, 32);
}

/// `spec/abi-a64.md` §7: the two tables, row by row.
#[test]
fn extension() {
    // (bits, attribute, the producer's duty, what may be assumed in a
    // register, and on the stack); a width equal to `bits` is "nothing".
    type Extended = (bool, u32);
    let (reg, stack) = (x(0), s(0));
    let rows: [(u32, Option<bool>, Extended, Extended, Extended); 16] = [
        (1, None, (false, 8), (false, 8), (false, 8)),
        (8, None, (false, 8), (false, 8), (false, 8)),
        (7, None, (false, 7), (false, 7), (false, 7)),
        (32, None, (false, 32), (false, 32), (false, 32)),
        (1, Some(false), (false, 32), (false, 32), (false, 8)),
        (1, Some(true), (true, 32), (true, 32), (true, 8)),
        (7, Some(true), (true, 32), (true, 32), (true, 32)),
        (8, Some(true), (true, 32), (true, 32), (false, 8)),
        (16, Some(false), (false, 32), (false, 32), (false, 16)),
        (17, Some(false), (false, 32), (false, 32), (false, 32)),
        (32, Some(true), (false, 32), (false, 32), (false, 32)),
        (33, Some(true), (true, 64), (true, 64), (true, 64)),
        (64, Some(false), (false, 64), (false, 64), (false, 64)),
        (65, Some(true), (true, 128), (true, 128), (true, 128)),
        (128, Some(true), (false, 128), (false, 128), (false, 128)),
        (129, Some(false), (false, 192), (false, 192), (false, 192)),
    ];
    for (bits, ext, produced, in_register, on_stack) in rows {
        let what = format!("i{bits} {ext:?}");
        assert_eq!(produced_bits(bits, ext).unwrap_or((false, bits)), produced, "{what}");
        assert_eq!(assumed_bits(bits, ext, reg), in_register, "{what}, register");
        assert_eq!(assumed_bits(bits, ext, stack), on_stack, "{what}, stack");
    }
}

/// `spec/abi-a64.md` §2: `assign` reuses the buffers of its output, and
/// leaves it empty on an error.
#[test]
fn output_is_reused() {
    let mut types = table();
    let big = Case::of(&of(40, &I128)).ret(Ty::Int(512)).lower(&mut types);
    let small = Case::of(&[I64]).ret(I64).lower(&mut types);
    let bad = Case::of(&[I64, arr(I128, 2)]).ret(I64).lower(&mut types);
    let mut out = Assignment::default();
    assign(&types, &big.sig(), &mut out).expect("supported");
    let (args, ret) = (out.args.as_ptr(), out.ret.as_ptr());
    let capacity = (out.args.capacity(), out.ret.capacity());
    for lowered in [&small, &big, &small, &big] {
        assign(&types, &lowered.sig(), &mut out).expect("supported");
        assert_eq!((out.args.as_ptr(), out.ret.as_ptr()), (args, ret));
        assert_eq!((out.args.capacity(), out.ret.capacity()), capacity);
    }
    assert!(assign(&types, &bad.sig(), &mut out).is_err());
    assert_eq!(out, Assignment::default());
    assert_eq!((out.args.capacity(), out.ret.capacity()), capacity);
}
