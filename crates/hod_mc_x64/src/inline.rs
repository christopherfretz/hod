//! Instructions only inline assembly writes (`spec/mc-x64.md` "Inline
//! assembly", WP-199): the prefetches, the multi-byte `NOP` with an
//! operand, port input and output, the x87 environment stores and loads,
//! the string instructions without `rep`, the flag instructions, and the
//! system and enclave instructions of one opcode that the C library's and
//! the compilers' headers wrap.

use crate::inst::{Inst, Op, digit_m, digit_r, plain};
use crate::reg::{Mem, Reg, Size};

/// The prefetch hints: `PREFETCHNTA`/`T0`/`T1`/`T2` (`0F 18 /0`..`/3`,
/// SSE), `PREFETCH`/`PREFETCHW`/`PREFETCHWT1` (`0F 0D /0`, `/1`, `/2`,
/// 3DNow!/PRFCHW/PREFETCHWT1). Each reads nothing architecturally and
/// faults on nothing.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Prefetch {
    /// Into a non-temporal structure close to the processor.
    Nta,
    /// Into every cache level.
    T0,
    /// Into the second level and up.
    T1,
    /// Into the third level and up.
    T2,
    /// The line, for reading (AMD's `PREFETCH`).
    Read,
    /// The line, in anticipation of a write.
    W,
    /// The line, for a write, into the second level and up.
    Wt1,
}

impl Prefetch {
    /// Every member.
    pub const ALL: [Prefetch; 7] = [
        Prefetch::Nta,
        Prefetch::T0,
        Prefetch::T1,
        Prefetch::T2,
        Prefetch::Read,
        Prefetch::W,
        Prefetch::Wt1,
    ];

    /// The mnemonic, as LLVM's assembler reads and prints it.
    #[inline]
    pub const fn mnemonic(self) -> &'static str {
        match self {
            Prefetch::Nta => "prefetchnta",
            Prefetch::T0 => "prefetcht0",
            Prefetch::T1 => "prefetcht1",
            Prefetch::T2 => "prefetcht2",
            Prefetch::Read => "prefetch",
            Prefetch::W => "prefetchw",
            Prefetch::Wt1 => "prefetchwt1",
        }
    }

    const fn encoding(self) -> (u8, u8) {
        match self {
            Prefetch::Nta => (0x18, 0),
            Prefetch::T0 => (0x18, 1),
            Prefetch::T1 => (0x18, 2),
            Prefetch::T2 => (0x18, 3),
            Prefetch::Read => (0x0d, 0),
            Prefetch::W => (0x0d, 1),
            Prefetch::Wt1 => (0x0d, 2),
        }
    }
}

/// The prefetch `op` of the line holding `m`.
#[inline]
pub const fn prefetch_m(op: Prefetch, m: Mem) -> Inst {
    let (opcode, digit) = op.encoding();
    digit_m(Op::two(opcode), digit, m)
}

/// Whether `size` is 16, 32 or 64 bits.
const fn not_byte(size: Size) -> bool {
    !matches!(size, Size::B)
}

/// Whether `size` is 8, 16 or 32 bits: the port transfer sizes.
const fn port_size(size: Size) -> bool {
    !matches!(size, Size::Q)
}

checked! {
    /// `NOP r/m` (`[66] [REX.W] 0F 1F /0`), register operand: the
    /// multi-byte no-op as written (`nopl %eax`).
    pub fn nop_r / try_nop_r(size: Size, r: Reg)
    requires not_byte(size);
    { digit_r(Op::two(0x1f).sized(size, 0), 0, size, r) }

    /// `NOP r/m` (`[66] [REX.W] 0F 1F /0`), memory operand: `nopl
    /// 64(%rax)`, the no-op whose displacement a reader of the code can
    /// find (the sanitizers' trap marks).
    pub fn nop_m / try_nop_m(size: Size, m: Mem)
    requires not_byte(size);
    { digit_m(Op::two(0x1f).sized(size, 0), 0, m) }

    /// `IN al/ax/eax, dx` (`EC`, `66 ED`, `ED`): reads port `dx`.
    /// Privileged unless the I/O permission allows it.
    pub fn in_dx / try_in_dx(size: Size)
    requires port_size(size);
    { plain(Op::one(0xec).sized(size, 1)) }

    /// `IN al/ax/eax, imm8` (`E4 ib`, `66 E5 ib`, `E5 ib`).
    pub fn in_i / try_in_i(size: Size, port: u8)
    requires port_size(size);
    {
        let mut inst = plain(Op::one(0xe4).sized(size, 1));
        inst.push(port);
        inst
    }

    /// `OUT dx, al/ax/eax` (`EE`, `66 EF`, `EF`): writes port `dx`.
    pub fn out_dx / try_out_dx(size: Size)
    requires port_size(size);
    { plain(Op::one(0xee).sized(size, 1)) }

    /// `OUT imm8, al/ax/eax` (`E6 ib`, `66 E7 ib`, `E7 ib`).
    pub fn out_i / try_out_i(size: Size, port: u8)
    requires port_size(size);
    {
        let mut inst = plain(Op::one(0xe6).sized(size, 1));
        inst.push(port);
        inst
    }
}

/// The string instructions, without a repeat prefix: one element of
/// `size` at `[rsi]` and/or `[rdi]`, the pointers advanced (downward if
/// the direction flag is set).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StringOp {
    /// `MOVS` (`A4`/`A5`): `[rdi] = [rsi]`.
    Movs,
    /// `STOS` (`AA`/`AB`): `[rdi] = al/ax/eax/rax`.
    Stos,
    /// `LODS` (`AC`/`AD`): `al/ax/eax/rax = [rsi]`.
    Lods,
    /// `SCAS` (`AE`/`AF`): compares the accumulator with `[rdi]`.
    Scas,
    /// `CMPS` (`A6`/`A7`): compares `[rsi]` with `[rdi]`.
    Cmps,
}

impl StringOp {
    /// Every member.
    pub const ALL: [StringOp; 5] =
        [StringOp::Movs, StringOp::Stos, StringOp::Lods, StringOp::Scas, StringOp::Cmps];

    /// The mnemonic without its size letter.
    #[inline]
    pub const fn stem(self) -> &'static str {
        match self {
            StringOp::Movs => "movs",
            StringOp::Stos => "stos",
            StringOp::Lods => "lods",
            StringOp::Scas => "scas",
            StringOp::Cmps => "cmps",
        }
    }

    const fn opcode(self) -> u8 {
        match self {
            StringOp::Movs => 0xa4,
            StringOp::Stos => 0xaa,
            StringOp::Lods => 0xac,
            StringOp::Scas => 0xae,
            StringOp::Cmps => 0xa6,
        }
    }
}

/// The string instruction `op` of one element of `size` (`[66]
/// [REX.W] opcode`, `+1` for the sizes above a byte). A repeat prefix is
/// an instruction of its own to LLVM's assembler, so it is the caller's
/// byte before this one ([`rep_movs`](crate::rep_movs) is the joined
/// form).
#[inline]
pub const fn string_op(op: StringOp, size: Size) -> Inst {
    plain(Op::one(op.opcode()).sized(size, 1))
}

/// A `0F 01 xx` instruction.
const fn group7(last: u8) -> Inst {
    let mut inst = plain(Op::two(0x01));
    inst.push(last);
    inst
}

/// `FNSTSW m16` (`DD /7`): stores the x87 status word, without checking
/// for pending exceptions.
#[inline]
pub const fn fnstsw_m(dst: Mem) -> Inst {
    digit_m(Op::one(0xdd), 7, dst)
}

/// `FNSTENV m28` (`D9 /6`): stores the x87 environment (control, status
/// and tag words, the last instruction and data pointers) and masks every
/// exception, without checking for pending ones.
#[inline]
pub const fn fnstenv_m(dst: Mem) -> Inst {
    digit_m(Op::one(0xd9), 6, dst)
}

/// `FLDENV m28` (`D9 /4`): loads the x87 environment.
#[inline]
pub const fn fldenv_m(src: Mem) -> Inst {
    digit_m(Op::one(0xd9), 4, src)
}

/// `FNSAVE m108` (`DD /6`): stores the environment and the register
/// stack, then initializes the unit, without checking for pending
/// exceptions.
#[inline]
pub const fn fnsave_m(dst: Mem) -> Inst {
    digit_m(Op::one(0xdd), 6, dst)
}

/// `FRSTOR m108` (`DD /4`): loads the environment and the register stack.
#[inline]
pub const fn frstor_m(src: Mem) -> Inst {
    digit_m(Op::one(0xdd), 4, src)
}

/// `CRC32 r64, r/m8` (`F2 REX.W 0F 38 F0 /r`), register source: the byte
/// form into a 64-bit destination, whose upper half is zero after it.
#[inline]
pub const fn crc32_q_rb(dst: Reg, src: Reg) -> Inst {
    let mut inst = Inst::EMPTY;
    inst.push(0xf2);
    inst.push(0x48 | dst.high() << 2 | src.high());
    inst.push(0x0f);
    inst.push(0x38);
    inst.push(0xf0);
    inst.push(0xc0 | dst.low() << 3 | src.low());
    inst
}

/// `RDMSR` (`0F 32`, privileged): model-specific register `ecx` into
/// `edx:eax`.
#[inline]
pub const fn rdmsr() -> Inst {
    plain(Op::two(0x32))
}

/// `WRMSR` (`0F 30`, privileged): `edx:eax` into model-specific register
/// `ecx`.
#[inline]
pub const fn wrmsr() -> Inst {
    plain(Op::two(0x30))
}

/// `ENCLU` (`0F 01 D7`, SGX): the user enclave function `eax`.
#[inline]
pub const fn enclu() -> Inst {
    group7(0xd7)
}

/// `ENCLS` (`0F 01 CF`, SGX, privileged): the supervisor enclave function
/// `eax`.
#[inline]
pub const fn encls() -> Inst {
    group7(0xcf)
}

/// `ENCLV` (`0F 01 C0`, SGX, privileged): the virtualization enclave
/// function `eax`.
#[inline]
pub const fn enclv() -> Inst {
    group7(0xc0)
}

/// `PCONFIG` (`0F 01 C5`, PCONFIG, privileged): the platform
/// configuration function `eax`.
#[inline]
pub const fn pconfig() -> Inst {
    group7(0xc5)
}

/// `HRESET imm8` (`F3 0F 3A F0 C0 ib`, HRESET, privileged): resets the
/// processor history components selected by `eax`.
#[inline]
pub const fn hreset(imm: u8) -> Inst {
    let mut inst = Inst::EMPTY;
    inst.push(0xf3);
    inst.push(0x0f);
    inst.push(0x3a);
    inst.push(0xf0);
    inst.push(0xc0);
    inst.push(imm);
    inst
}

/// `CLC` (`F8`): clears the carry flag.
#[inline]
pub const fn clc() -> Inst {
    plain(Op::one(0xf8))
}

/// `STC` (`F9`): sets the carry flag.
#[inline]
pub const fn stc() -> Inst {
    plain(Op::one(0xf9))
}

/// `CMC` (`F5`): complements the carry flag.
#[inline]
pub const fn cmc() -> Inst {
    plain(Op::one(0xf5))
}

/// `LAHF` (`9F`): `SF ZF AF PF CF` into `ah`.
#[inline]
pub const fn lahf() -> Inst {
    plain(Op::one(0x9f))
}

/// `SAHF` (`9E`): `ah` into `SF ZF AF PF CF`.
#[inline]
pub const fn sahf() -> Inst {
    plain(Op::one(0x9e))
}

/// `CLI` (`FA`, privileged): clears the interrupt flag.
#[inline]
pub const fn cli() -> Inst {
    plain(Op::one(0xfa))
}

/// `STI` (`FB`, privileged): sets the interrupt flag.
#[inline]
pub const fn sti() -> Inst {
    plain(Op::one(0xfb))
}
