//! The x87 floating-point unit: loads and stores of the three precisions
//! and of integers, arithmetic, compares, conditional moves and the
//! control word (`spec/mc-x64.md` "x87").
//!
//! Every instruction here is an escape opcode `D8`..`DF`, followed either
//! by a `ModRM` byte whose reg field is the operation (`/digit`) and whose
//! r/m is a memory operand, or by a second opcode byte that names a stack
//! register (`C0+i` .. `F8+i`). No x87 instruction takes REX.W or an
//! operand-size prefix; a memory operand gets REX.B/REX.X for `r8`..`r15`
//! and nothing else.
//!
//! Operand order is Intel's: destination first. `_0i` is the form with
//! `st(0)` the destination and `st(i)` the source, `_i0` the form with
//! `st(i)` the destination; the `p` forms pop after storing into `st(i)`.
//! Each constructor's documentation states what it computes, which is the
//! Intel SDM's meaning and the one LLVM's Intel syntax uses. AT&T syntax
//! (what `llc` writes) swaps `fsub`/`fsubr` and `fdiv`/`fdivr` in the forms
//! whose destination is not `st(0)`: AT&T `fsubrp %st, %st(1)` is
//! [`fsubp`]`(St1)`.

use crate::inst::{Inst, Op, digit_m, plain};
use crate::reg::{Cond, FpSize, Mem, Size};

/// An x87 stack register, `st(0)`..`st(7)`, counted from the top.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
#[repr(u8)]
pub enum St {
    /// The top of the stack, `st`.
    St0 = 0,
    St1,
    St2,
    St3,
    St4,
    St5,
    St6,
    St7,
}

impl St {
    const ALL: [Self; 8] =
        [Self::St0, Self::St1, Self::St2, Self::St3, Self::St4, Self::St5, Self::St6, Self::St7];

    /// `st(n)`, or `None` for `n > 7`.
    #[inline]
    pub const fn new(n: u8) -> Option<Self> {
        if n < 8 { Some(Self::ALL[n as usize]) } else { None }
    }

    /// The register's number, `0..=7`.
    #[inline]
    pub const fn code(self) -> u8 {
        self as u8
    }
}

/// The floating-point format of an x87 memory operand.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum X87Size {
    /// Single precision, 32 bits (`dword ptr`, AT&T suffix `s`).
    S,
    /// Double precision, 64 bits (`qword ptr`, AT&T suffix `l`).
    D,
    /// Double extended precision, 80 bits (`tbyte ptr`, AT&T suffix `t`):
    /// `x86_fp80`, 10 bytes.
    T,
}

impl X87Size {
    /// The width of the memory operand in bits: 32, 64 or 80.
    #[inline]
    pub const fn bits(self) -> u32 {
        match self {
            Self::S => 32,
            Self::D => 64,
            Self::T => 80,
        }
    }
}

/// Escape byte `esc`, then `base + i`.
#[inline]
const fn sti(esc: u8, base: u8, i: St) -> Inst {
    let mut inst = plain(Op::one(esc));
    inst.push(base + i.code());
    inst
}

/// `DC base+i`, a form with `st(i)` the destination. With `st(0)` both
/// operands are the top and it computes what the `D8` form of the same
/// mnemonic does; that form is the one LLVM assembles for the text (the
/// `D8` subtraction and division forms have `r` the other way round, so
/// the second byte's bit 3 flips for them).
#[inline]
const fn sti_dst(base: u8, i: St) -> Inst {
    match i {
        St::St0 if base >= 0xe0 => sti(0xd8, base ^ 8, i),
        St::St0 => sti(0xd8, base, i),
        _ => sti(0xdc, base, i),
    }
}

/// Escape byte `esc`, then the fixed byte `second`.
#[inline]
const fn fixed(esc: u8, second: u8) -> Inst {
    let mut inst = plain(Op::one(esc));
    inst.push(second);
    inst
}

/// Escape byte `esc` with a memory operand and `/digit`.
#[inline]
const fn mem(esc: u8, digit: u8, m: Mem) -> Inst {
    digit_m(Op::one(esc), digit, m)
}

/// The escape of an arithmetic instruction with a real memory operand:
/// `D8` for m32, `DC` for m64.
#[inline]
const fn arith_esc(fp: FpSize) -> u8 {
    match fp {
        FpSize::S => 0xd8,
        FpSize::D => 0xdc,
    }
}

#[inline]
const fn arith_m(digit: u8, fp: FpSize, src: Mem) -> Inst {
    mem(arith_esc(fp), digit, src)
}

/// `FLD m32fp/m64fp/m80fp` (`D9 /0`, `DD /0`, `DB /5`): pushes the value,
/// converted exactly to double extended precision.
#[inline]
pub const fn fld_m(w: X87Size, src: Mem) -> Inst {
    match w {
        X87Size::S => mem(0xd9, 0, src),
        X87Size::D => mem(0xdd, 0, src),
        X87Size::T => mem(0xdb, 5, src),
    }
}

/// `FSTP m32fp/m64fp/m80fp` (`D9 /3`, `DD /3`, `DB /7`): stores `st(0)`,
/// rounded by the control word to `w` (exact for `T`), and pops.
#[inline]
pub const fn fstp_m(w: X87Size, dst: Mem) -> Inst {
    match w {
        X87Size::S => mem(0xd9, 3, dst),
        X87Size::D => mem(0xdd, 3, dst),
        X87Size::T => mem(0xdb, 7, dst),
    }
}

checked! {
    /// `FST m32fp/m64fp` (`D9 /2`, `DD /2`): stores `st(0)`, rounded by the
    /// control word, without popping. There is no 80-bit form.
    pub fn fst_m / try_fst_m(w: X87Size, dst: Mem)
    requires !matches!(w, X87Size::T);
    {
        match w {
            X87Size::S => mem(0xd9, 2, dst),
            _ => mem(0xdd, 2, dst),
        }
    }

    /// `FILD m16int/m32int/m64int` (`DF /0`, `DB /0`, `DF /5`): pushes the
    /// signed integer, converted exactly.
    pub fn fild_m / try_fild_m(size: Size, src: Mem)
    requires !matches!(size, Size::B);
    {
        match size {
            Size::D => mem(0xdb, 0, src),
            Size::Q => mem(0xdf, 5, src),
            _ => mem(0xdf, 0, src),
        }
    }

    /// `FIST m16int/m32int` (`DF /2`, `DB /2`): stores `st(0)` as a signed
    /// integer, rounded by the control word, without popping; the "integer
    /// indefinite" (the most negative value) when out of range or NaN.
    /// There is no 64-bit form.
    pub fn fist_m / try_fist_m(size: Size, dst: Mem)
    requires matches!(size, Size::W | Size::D);
    {
        match size {
            Size::D => mem(0xdb, 2, dst),
            _ => mem(0xdf, 2, dst),
        }
    }

    /// `FISTP m16int/m32int/m64int` (`DF /3`, `DB /3`, `DF /7`): as
    /// [`fist_m`], then pops.
    pub fn fistp_m / try_fistp_m(size: Size, dst: Mem)
    requires !matches!(size, Size::B);
    {
        match size {
            Size::D => mem(0xdb, 3, dst),
            Size::Q => mem(0xdf, 7, dst),
            _ => mem(0xdf, 3, dst),
        }
    }

    /// `FCMOVcc st(0), st(i)` (`DA C0+i` .. `DB D8+i`): `st(0) = st(i)` if
    /// `cond` holds in EFLAGS. Only `B`, `Ae` (`fcmovnb`), `E`, `Ne`, `Be`,
    /// `A` (`fcmovnbe`), `P` (`fcmovu`) and `Np` (`fcmovnu`) exist.
    pub fn fcmov / try_fcmov(cond: Cond, src: St)
    requires matches!(
        cond,
        Cond::B | Cond::Ae | Cond::E | Cond::Ne | Cond::Be | Cond::A | Cond::P | Cond::Np
    );
    {
        // DA for the conditions, DB for their negations; the second byte
        // C0, C8, D0, D8 for B, E, BE, U.
        let (esc, base) = match cond {
            Cond::B => (0xda, 0xc0),
            Cond::E => (0xda, 0xc8),
            Cond::Be => (0xda, 0xd0),
            Cond::P => (0xda, 0xd8),
            Cond::Ae => (0xdb, 0xc0),
            Cond::Ne => (0xdb, 0xc8),
            Cond::A => (0xdb, 0xd0),
            _ => (0xdb, 0xd8),
        };
        sti(esc, base, src)
    }
}

/// `FLD st(i)` (`D9 C0+i`): pushes a copy of `st(i)` (`st(i)` counted
/// before the push; `fld_st(St0)` duplicates the top).
#[inline]
pub const fn fld_st(src: St) -> Inst {
    sti(0xd9, 0xc0, src)
}

/// `FST st(i)` (`DD D0+i`): `st(i) = st(0)`.
#[inline]
pub const fn fst_st(dst: St) -> Inst {
    sti(0xdd, 0xd0, dst)
}

/// `FSTP st(i)` (`DD D8+i`): `st(i) = st(0)`, then pops; `fstp_st(St0)`
/// discards the top.
#[inline]
pub const fn fstp_st(dst: St) -> Inst {
    sti(0xdd, 0xd8, dst)
}

/// `FXCH st(i)` (`D9 C8+i`): exchanges `st(0)` and `st(i)`.
#[inline]
pub const fn fxch(other: St) -> Inst {
    sti(0xd9, 0xc8, other)
}

family! {
    (fp: FpSize, src: Mem) => arith_m;
    /// `FADD m32fp/m64fp` (`D8`/`DC /0`): `st(0) = st(0) + [src]`.
    pub fn fadd_m = (0);
    /// `FMUL m32fp/m64fp` (`D8`/`DC /1`): `st(0) = st(0) * [src]`.
    pub fn fmul_m = (1);
    /// `FSUB m32fp/m64fp` (`D8`/`DC /4`): `st(0) = st(0) - [src]`.
    pub fn fsub_m = (4);
    /// `FSUBR m32fp/m64fp` (`D8`/`DC /5`): `st(0) = [src] - st(0)`.
    pub fn fsubr_m = (5);
    /// `FDIV m32fp/m64fp` (`D8`/`DC /6`): `st(0) = st(0) / [src]`.
    pub fn fdiv_m = (6);
    /// `FDIVR m32fp/m64fp` (`D8`/`DC /7`): `st(0) = [src] / st(0)`.
    pub fn fdivr_m = (7);
}

family! {
    (src: St) => sti;
    /// `FADD st(0), st(i)` (`D8 C0+i`): `st(0) = st(0) + st(i)`.
    pub fn fadd_0i = (0xd8, 0xc0);
    /// `FMUL st(0), st(i)` (`D8 C8+i`): `st(0) = st(0) * st(i)`.
    pub fn fmul_0i = (0xd8, 0xc8);
    /// `FSUB st(0), st(i)` (`D8 E0+i`): `st(0) = st(0) - st(i)`.
    pub fn fsub_0i = (0xd8, 0xe0);
    /// `FSUBR st(0), st(i)` (`D8 E8+i`): `st(0) = st(i) - st(0)`.
    pub fn fsubr_0i = (0xd8, 0xe8);
    /// `FDIV st(0), st(i)` (`D8 F0+i`): `st(0) = st(0) / st(i)`.
    pub fn fdiv_0i = (0xd8, 0xf0);
    /// `FDIVR st(0), st(i)` (`D8 F8+i`): `st(0) = st(i) / st(0)`.
    pub fn fdivr_0i = (0xd8, 0xf8);
}

family! {
    (dst: St) => sti_dst;
    /// `FADD st(i), st(0)` (`DC C0+i`; for `st(0)`, the `D8` form): `st(i) =
    /// st(i) + st(0)`.
    pub fn fadd_i0 = (0xc0);
    /// `FMUL st(i), st(0)` (`DC C8+i`): `st(i) = st(i) * st(0)`.
    pub fn fmul_i0 = (0xc8);
    /// `FSUBR st(i), st(0)` (`DC E0+i`): `st(i) = st(0) - st(i)`. AT&T
    /// spells it `fsub %st, %st(i)`.
    pub fn fsubr_i0 = (0xe0);
    /// `FSUB st(i), st(0)` (`DC E8+i`): `st(i) = st(i) - st(0)`. AT&T
    /// spells it `fsubr %st, %st(i)`.
    pub fn fsub_i0 = (0xe8);
    /// `FDIVR st(i), st(0)` (`DC F0+i`): `st(i) = st(0) / st(i)`. AT&T
    /// spells it `fdiv %st, %st(i)`.
    pub fn fdivr_i0 = (0xf0);
    /// `FDIV st(i), st(0)` (`DC F8+i`): `st(i) = st(i) / st(0)`. AT&T
    /// spells it `fdivr %st, %st(i)`.
    pub fn fdiv_i0 = (0xf8);
}

family! {
    (dst: St) => sti;
    /// `FADDP st(i), st(0)` (`DE C0+i`): `st(i) = st(i) + st(0)`, then pops
    /// (the result is then `st(i-1)`).
    pub fn faddp = (0xde, 0xc0);
    /// `FMULP st(i), st(0)` (`DE C8+i`): `st(i) = st(i) * st(0)`, then pops.
    pub fn fmulp = (0xde, 0xc8);
    /// `FSUBRP st(i), st(0)` (`DE E0+i`): `st(i) = st(0) - st(i)`, then
    /// pops. AT&T spells it `fsubp %st, %st(i)`.
    pub fn fsubrp = (0xde, 0xe0);
    /// `FSUBP st(i), st(0)` (`DE E8+i`): `st(i) = st(i) - st(0)`, then
    /// pops. AT&T spells it `fsubrp %st, %st(i)`.
    pub fn fsubp = (0xde, 0xe8);
    /// `FDIVRP st(i), st(0)` (`DE F0+i`): `st(i) = st(0) / st(i)`, then
    /// pops. AT&T spells it `fdivp %st, %st(i)`.
    pub fn fdivrp = (0xde, 0xf0);
    /// `FDIVP st(i), st(0)` (`DE F8+i`): `st(i) = st(i) / st(0)`, then
    /// pops. AT&T spells it `fdivrp %st, %st(i)`.
    pub fn fdivp = (0xde, 0xf8);
}

family! {
    (other: St) => sti;
    /// `FUCOMI st(0), st(i)` (`DB E8+i`): compares into ZF, PF, CF as
    /// `ucomisd` does (all three set when unordered; `st(0) < st(i)` sets
    /// CF); an exception only for a signaling NaN.
    pub fn fucomi = (0xdb, 0xe8);
    /// `FUCOMIP st(0), st(i)` (`DF E8+i`): as [`fucomi`], then pops.
    /// LLVM prints it `fucompi`.
    pub fn fucomip = (0xdf, 0xe8);
    /// `FCOMI st(0), st(i)` (`DB F0+i`): as [`fucomi`], signaling on any
    /// NaN.
    pub fn fcomi = (0xdb, 0xf0);
    /// `FCOMIP st(0), st(i)` (`DF F0+i`): as [`fcomi`], then pops. LLVM
    /// prints it `fcompi`.
    pub fn fcomip = (0xdf, 0xf0);
    /// `FUCOM st(i)` (`DD E0+i`): compares `st(0)` with `st(i)` into the
    /// status word's C0, C2, C3 (read them with [`fnstsw_ax`]).
    pub fn fucom = (0xdd, 0xe0);
    /// `FUCOMP st(i)` (`DD E8+i`): as [`fucom`], then pops.
    pub fn fucomp = (0xdd, 0xe8);
}

/// `FUCOMPP` (`DA E9`): compares `st(0)` with `st(1)` into C0, C2, C3,
/// then pops both.
#[inline]
pub const fn fucompp() -> Inst {
    fixed(0xda, 0xe9)
}

/// `FNSTSW ax` (`DF E0`): the status word into `ax`, without checking for
/// pending exceptions. `sahf` is not provided; test `ah`'s bits through
/// `ax` (C0 bit 8, C2 bit 10, C3 bit 14).
#[inline]
pub const fn fnstsw_ax() -> Inst {
    fixed(0xdf, 0xe0)
}

/// `FCHS` (`D9 E0`): `st(0) = -st(0)`, the sign bit flipped.
#[inline]
pub const fn fchs() -> Inst {
    fixed(0xd9, 0xe0)
}

/// `FABS` (`D9 E1`): `st(0) = |st(0)|`, the sign bit cleared.
#[inline]
pub const fn fabs() -> Inst {
    fixed(0xd9, 0xe1)
}

/// `FLDZ` (`D9 EE`): pushes `+0.0`.
#[inline]
pub const fn fldz() -> Inst {
    fixed(0xd9, 0xee)
}

/// `FLD1` (`D9 E8`): pushes `+1.0`.
#[inline]
pub const fn fld1() -> Inst {
    fixed(0xd9, 0xe8)
}

/// `FPREM` (`D9 F8`): `st(0) = st(0) - q * st(1)`, `q` the quotient
/// truncated toward zero (C's `fmodl`); partial: repeat while C2 is set.
#[inline]
pub const fn fprem() -> Inst {
    fixed(0xd9, 0xf8)
}

/// `FPREM1` (`D9 F5`): as [`fprem`] with `q` rounded to nearest even
/// (IEEE `remainder`); partial as well.
#[inline]
pub const fn fprem1() -> Inst {
    fixed(0xd9, 0xf5)
}

/// `FSQRT` (`D9 FA`): `st(0) = sqrt(st(0))`.
#[inline]
pub const fn fsqrt() -> Inst {
    fixed(0xd9, 0xfa)
}

/// `FRNDINT` (`D9 FC`): `st(0)` rounded to an integer by the control
/// word's rounding mode.
#[inline]
pub const fn frndint() -> Inst {
    fixed(0xd9, 0xfc)
}

/// `FNCLEX` (`DB E2`): clears the exception flags, without checking for
/// pending exceptions.
#[inline]
pub const fn fnclex() -> Inst {
    fixed(0xdb, 0xe2)
}

/// `FNINIT` (`DB E3`): resets the unit (control word `037F`, empty
/// stack), without checking for pending exceptions.
#[inline]
pub const fn fninit() -> Inst {
    fixed(0xdb, 0xe3)
}

/// `FNSTCW m16` (`D9 /7`): stores the control word, without checking for
/// pending exceptions.
#[inline]
pub const fn fnstcw_m(dst: Mem) -> Inst {
    mem(0xd9, 7, dst)
}

/// `FLDCW m16` (`D9 /5`): loads the control word.
#[inline]
pub const fn fldcw_m(src: Mem) -> Inst {
    mem(0xd9, 5, src)
}
