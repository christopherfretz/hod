//! Processor state, system and extension instructions of the x86 target
//! intrinsics that are not vector operations (`spec/mc-x64.md` "Processor
//! state and extensions", WP-190): the `FXSAVE`/`XSAVE` families, the
//! cache-line write-backs, `RDRAND`/`RDSEED`, the segment-base accesses,
//! `CRC32`, the BMI operations, RTM, CET's shadow-stack pointer and the
//! counters and registers read by one opcode.

use crate::inst::{Inst, Op, digit_m, digit_r, plain};
use crate::reg::{Mem, Reg, Size};

/// A `0F`-map opcode with `REX.W` when `wide`.
#[inline]
const fn two_w(opcode: u8, wide: bool) -> Op {
    let op = Op::two(opcode);
    Op { w: wide, ..op }
}

/// A `0F 01 xx` instruction: the two-byte opcode `0F 01` and `last`.
#[inline]
const fn group7(last: u8) -> Inst {
    let mut inst = plain(Op::two(0x01));
    inst.push(last);
    inst
}

/// The instructions that save or restore processor state to or from
/// memory: `FXSAVE`/`FXRSTOR` (`0F AE /0`, `/1`), `XSAVE`/`XRSTOR`/
/// `XSAVEOPT` (`0F AE /4`, `/5`, `/6`), `XSAVEC`/`XSAVES`/`XRSTORS`
/// (`0F C7 /4`, `/5`, `/3`); each `64` form with `REX.W`, which saves and
/// restores the 64-bit instruction and data pointers of the x87 state.
/// The `XSAVE` family takes its component mask in `edx:eax`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum XState {
    Fxsave,
    Fxsave64,
    Fxrstor,
    Fxrstor64,
    Xsave,
    Xsave64,
    Xrstor,
    Xrstor64,
    Xsaveopt,
    Xsaveopt64,
    Xsavec,
    Xsavec64,
    Xsaves,
    Xsaves64,
    Xrstors,
    Xrstors64,
}

impl XState {
    /// Every member.
    pub const ALL: [XState; 16] = [
        XState::Fxsave,
        XState::Fxsave64,
        XState::Fxrstor,
        XState::Fxrstor64,
        XState::Xsave,
        XState::Xsave64,
        XState::Xrstor,
        XState::Xrstor64,
        XState::Xsaveopt,
        XState::Xsaveopt64,
        XState::Xsavec,
        XState::Xsavec64,
        XState::Xsaves,
        XState::Xsaves64,
        XState::Xrstors,
        XState::Xrstors64,
    ];

    /// The mnemonic, as LLVM prints it.
    pub const fn mnemonic(self) -> &'static str {
        match self {
            XState::Fxsave => "fxsave",
            XState::Fxsave64 => "fxsave64",
            XState::Fxrstor => "fxrstor",
            XState::Fxrstor64 => "fxrstor64",
            XState::Xsave => "xsave",
            XState::Xsave64 => "xsave64",
            XState::Xrstor => "xrstor",
            XState::Xrstor64 => "xrstor64",
            XState::Xsaveopt => "xsaveopt",
            XState::Xsaveopt64 => "xsaveopt64",
            XState::Xsavec => "xsavec",
            XState::Xsavec64 => "xsavec64",
            XState::Xsaves => "xsaves",
            XState::Xsaves64 => "xsaves64",
            XState::Xrstors => "xrstors",
            XState::Xrstors64 => "xrstors64",
        }
    }

    /// The opcode (`0F AE` or `0F C7`), its `/digit` and `REX.W`.
    const fn encoding(self) -> (u8, u8, bool) {
        match self {
            XState::Fxsave => (0xae, 0, false),
            XState::Fxsave64 => (0xae, 0, true),
            XState::Fxrstor => (0xae, 1, false),
            XState::Fxrstor64 => (0xae, 1, true),
            XState::Xsave => (0xae, 4, false),
            XState::Xsave64 => (0xae, 4, true),
            XState::Xrstor => (0xae, 5, false),
            XState::Xrstor64 => (0xae, 5, true),
            XState::Xsaveopt => (0xae, 6, false),
            XState::Xsaveopt64 => (0xae, 6, true),
            XState::Xsavec => (0xc7, 4, false),
            XState::Xsavec64 => (0xc7, 4, true),
            XState::Xsaves => (0xc7, 5, false),
            XState::Xsaves64 => (0xc7, 5, true),
            XState::Xrstors => (0xc7, 3, false),
            XState::Xrstors64 => (0xc7, 3, true),
        }
    }
}

/// `op m` of the [`XState`] family.
#[inline]
pub const fn xstate_m(op: XState, m: Mem) -> Inst {
    let (opcode, digit, wide) = op.encoding();
    digit_m(two_w(opcode, wide), digit, m)
}

/// `CLFLUSHOPT m8` (`66 0F AE /7`, CLFLUSHOPT): writes back and
/// invalidates the cache line of `m`, ordered only by fences.
#[inline]
pub const fn clflushopt_m(m: Mem) -> Inst {
    let op = Op::two(0xae);
    digit_m(Op { p66: true, ..op }, 7, m)
}

/// `CLWB m8` (`66 0F AE /6`, CLWB): writes back the cache line of `m`,
/// which may stay cached.
#[inline]
pub const fn clwb_m(m: Mem) -> Inst {
    let op = Op::two(0xae);
    digit_m(Op { p66: true, ..op }, 6, m)
}

/// Whether `size` is a size `RDRAND`/`RDSEED` take: 16, 32 or 64 bits.
#[inline]
const fn not_byte(size: Size) -> bool {
    !matches!(size, Size::B)
}

/// Whether `size` is 32 or 64 bits.
#[inline]
const fn dq(size: Size) -> bool {
    matches!(size, Size::D | Size::Q)
}

checked! {
    /// `RDRAND r16`/`r32`/`r64` (`[66] [REX.W] 0F C7 /6`, RDRAND): a
    /// random number; CF is set when one was available, else `r` is 0.
    pub fn rdrand_r / try_rdrand_r(size: Size, r: Reg)
    requires not_byte(size);
    { digit_r(Op::two(0xc7).sized(size, 0), 6, size, r) }

    /// `RDSEED r16`/`r32`/`r64` (`[66] [REX.W] 0F C7 /7`, RDSEED): as
    /// [`rdrand_r`], from the entropy source.
    pub fn rdseed_r / try_rdseed_r(size: Size, r: Reg)
    requires not_byte(size);
    { digit_r(Op::two(0xc7).sized(size, 0), 7, size, r) }
}

/// The accesses of the FS and GS base registers (FSGSBASE):
/// `RDFSBASE`/`RDGSBASE`/`WRFSBASE`/`WRGSBASE` (`F3 [REX.W] 0F AE /0` to
/// `/3`, register operand).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SegBase {
    RdFs,
    RdGs,
    WrFs,
    WrGs,
}

impl SegBase {
    /// Every member.
    pub const ALL: [SegBase; 4] = [SegBase::RdFs, SegBase::RdGs, SegBase::WrFs, SegBase::WrGs];

    /// The mnemonic, as LLVM prints it.
    pub const fn mnemonic(self) -> &'static str {
        match self {
            SegBase::RdFs => "rdfsbase",
            SegBase::RdGs => "rdgsbase",
            SegBase::WrFs => "wrfsbase",
            SegBase::WrGs => "wrgsbase",
        }
    }
}

checked! {
    /// `op r32`/`r64` of the [`SegBase`] family: a read writes `r` (the
    /// 32-bit form the low half, zero-extended), a write reads it.
    pub fn segbase_r / try_segbase_r(op: SegBase, size: Size, r: Reg)
    requires dq(size);
    { digit_r(Op::two(0xae).rep(0xf3).sized(size, 0), op as u8, size, r) }

    /// `RDSSPD r32`/`RDSSPQ r64` (`F3 [REX.W] 0F 1E /1`, register operand,
    /// CET shadow stacks): the shadow-stack pointer into `r`; a `nop`
    /// that leaves `r` unchanged where shadow stacks are not enabled.
    pub fn rdssp_r / try_rdssp_r(size: Size, r: Reg)
    requires dq(size);
    { digit_r(Op::two(0x1e).rep(0xf3).sized(size, 0), 1, size, r) }

    /// `INCSSPD r32`/`INCSSPQ r64` (`F3 [REX.W] 0F AE /5`, register
    /// operand): pops `r` (its low 8 bits) entries off the shadow stack.
    pub fn incssp_r / try_incssp_r(size: Size, r: Reg)
    requires dq(size);
    { digit_r(Op::two(0xae).rep(0xf3).sized(size, 0), 5, size, r) }
}

/// `RDPID r64` (`F3 0F C7 /7`, register operand, RDPID): `IA32_TSC_AUX`
/// into `r`, zero-extended.
#[inline]
pub const fn rdpid_r(r: Reg) -> Inst {
    digit_r(Op::two(0xc7).rep(0xf3), 7, Size::Q, r)
}

/// `RDPMC` (`0F 33`): performance counter `ecx` into `edx:eax`.
#[inline]
pub const fn rdpmc() -> Inst {
    plain(Op::two(0x33))
}

/// `XGETBV` (`0F 01 D0`, XSAVE): extended control register `ecx` into
/// `edx:eax`.
#[inline]
pub const fn xgetbv() -> Inst {
    group7(0xd0)
}

/// `XSETBV` (`0F 01 D1`, XSAVE, privileged): `edx:eax` into extended
/// control register `ecx`.
#[inline]
pub const fn xsetbv() -> Inst {
    group7(0xd1)
}

/// `XEND` (`0F 01 D5`, RTM): commits the transaction.
#[inline]
pub const fn xend() -> Inst {
    group7(0xd5)
}

/// `XTEST` (`0F 01 D6`, RTM or HLE): ZF clear inside a transaction.
#[inline]
pub const fn xtest() -> Inst {
    group7(0xd6)
}

/// `RDPKRU` (`0F 01 EE`, PKU): `PKRU` into `eax`, `edx` zeroed; `ecx`
/// must be 0.
#[inline]
pub const fn rdpkru() -> Inst {
    group7(0xee)
}

/// `WRPKRU` (`0F 01 EF`, PKU): `eax` into `PKRU`; `ecx` and `edx` must
/// be 0.
#[inline]
pub const fn wrpkru() -> Inst {
    group7(0xef)
}

/// `SERIALIZE` (`0F 01 E8`, SERIALIZE).
#[inline]
pub const fn serialize() -> Inst {
    group7(0xe8)
}

/// `XABORT imm8` (`C6 F8 ib`, RTM): aborts the transaction with `imm` in
/// bits 24 to 31 of `eax`; a `nop` outside one.
#[inline]
pub const fn xabort(imm: u8) -> Inst {
    let mut inst = plain(Op::one(0xc6));
    inst.push(0xf8);
    inst.push(imm);
    inst
}

/// `XBEGIN rel32` (`C7 F8 cd`, RTM): starts a transaction; on an abort
/// execution resumes `disp` bytes past the end of the instruction with
/// the status in `eax`. The displacement is plain bytes, not a site: the
/// caller writes the distance it knows (0: the next instruction).
#[inline]
pub const fn xbegin(disp: i32) -> Inst {
    let mut inst = plain(Op::one(0xc7));
    inst.push(0xf8);
    inst.push_imm(disp as u32 as u64, 4);
    inst
}

/// `CRC32 r32, r/m8`/`r/m16`/`r/m32` and `CRC32 r64, r/m64` (`F2 [66]
/// [REX] 0F 38 F0 /r` for bytes, `F1` otherwise; SSE4.2 or CRC32),
/// register source of `size`: `dst` (32 bits, 64 for [`Size::Q`]) is the
/// CRC-32C of `src` continued from `dst`.
#[inline]
pub const fn crc32_rr(size: Size, dst: Reg, src: Reg) -> Inst {
    let mut inst = Inst::EMPTY;
    if matches!(size, Size::W) {
        inst.push(0x66);
    }
    inst.push(0xf2);
    let w = if matches!(size, Size::Q) { 8 } else { 0 };
    let rex = w | dst.high() << 2 | src.high();
    if rex != 0 || (matches!(size, Size::B) && src.needs_rex_as_byte()) {
        inst.push(0x40 | rex);
    }
    inst.push(0x0f);
    inst.push(0x38);
    inst.push(if matches!(size, Size::B) { 0xf0 } else { 0xf1 });
    inst.push(0xc0 | dst.low() << 3 | src.low());
    inst
}

/// The BMI operations of three general registers (VEX-encoded, map
/// `0F38`): `BEXTR` (`VEX.LZ.0F38 F7`, BMI1), `BZHI` (`VEX.LZ.0F38 F5`,
/// BMI2), `PDEP` (`VEX.LZ.F2.0F38 F5`), `PEXT` (`VEX.LZ.F3.0F38 F5`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Bmi {
    /// `dst = (x >> y[7:0]) & ((1 << y[15:8]) - 1)`, saturated at the width.
    Bextr,
    /// `dst = x` with the bits from `y[7:0]` up cleared.
    Bzhi,
    /// The low bits of `x` deposited at the set bits of `y`.
    Pdep,
    /// The bits of `x` at the set bits of `y`, gathered to the low end.
    Pext,
}

impl Bmi {
    /// Every member.
    pub const ALL: [Bmi; 4] = [Bmi::Bextr, Bmi::Bzhi, Bmi::Pdep, Bmi::Pext];

    /// The mnemonic, as LLVM prints it.
    pub const fn mnemonic(self) -> &'static str {
        match self {
            Bmi::Bextr => "bextr",
            Bmi::Bzhi => "bzhi",
            Bmi::Pdep => "pdep",
            Bmi::Pext => "pext",
        }
    }
}

checked! {
    /// `op dst, x, y` of the [`Bmi`] family, 32 or 64 bits, in Intel's
    /// operand order: `BEXTR`/`BZHI` take `x` in `ModRM.rm` and `y` in
    /// `VEX.vvvv`, `PDEP`/`PEXT` `x` in `VEX.vvvv` and `y` in `ModRM.rm`.
    /// The three-byte VEX prefix (`C4`): the map `0F38` has no two-byte
    /// form.
    pub fn bmi_rrr / try_bmi_rrr(op: Bmi, size: Size, dst: Reg, x: Reg, y: Reg)
    requires dq(size);
    {
        let (pp, opcode) = match op {
            Bmi::Bextr => (0, 0xf7),
            Bmi::Bzhi => (0, 0xf5),
            Bmi::Pdep => (3, 0xf5),
            Bmi::Pext => (2, 0xf5),
        };
        let (rm, v) = match op {
            Bmi::Bextr | Bmi::Bzhi => (x, y),
            Bmi::Pdep | Bmi::Pext => (y, x),
        };
        let mut inst = Inst::EMPTY;
        inst.push(0xc4);
        // R, X and B inverted; map 0F38.
        inst.push((dst.high() ^ 1) << 7 | 1 << 6 | (rm.high() ^ 1) << 5 | 0b00010);
        let w = if matches!(size, Size::Q) { 1 } else { 0 };
        inst.push(w << 7 | (!v.code() & 15) << 3 | pp);
        inst.push(opcode);
        inst.push(0xc0 | dst.low() << 3 | rm.low());
        inst
    }
}
