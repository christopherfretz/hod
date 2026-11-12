//! System instructions: NOP and hints, exception generation, the system
//! registers code generation touches, barriers (`spec/mc-a64.md` "System").

use crate::reg::RegZr;

/// `NOP`: NOP.
#[inline]
pub const fn nop() -> u32 {
    0xd503_201f
}

checked! {
    /// `HINT #imm`: HINT.
    ///
    /// Precondition: `imm < 128`.
    pub fn hint / try_hint(imm: u32)
    requires imm < 128;
    { 0xd503_201f | imm << 5 }
}

/// `BTI c`: BTI, landing pad for indirect calls (`HINT #34`).
#[inline]
pub const fn bti_c() -> u32 {
    hint(34)
}

/// `BTI j`: BTI, landing pad for indirect jumps (`HINT #36`).
#[inline]
pub const fn bti_j() -> u32 {
    hint(36)
}

/// `BTI jc`: BTI, landing pad for both (`HINT #38`).
#[inline]
pub const fn bti_jc() -> u32 {
    hint(38)
}

/// `PACIASP`: sign LR with key A and SP (`HINT #25`).
#[inline]
pub const fn paciasp() -> u32 {
    hint(25)
}

/// `PACIBSP`: sign LR with key B and SP (`HINT #27`).
#[inline]
pub const fn pacibsp() -> u32 {
    hint(27)
}

/// `AUTIASP`: authenticate LR with key A and SP (`HINT #29`).
#[inline]
pub const fn autiasp() -> u32 {
    hint(29)
}

/// `AUTIBSP`: authenticate LR with key B and SP (`HINT #31`).
#[inline]
pub const fn autibsp() -> u32 {
    hint(31)
}

/// `BRK #imm16`: BRK.
#[inline]
pub const fn brk(imm16: u16) -> u32 {
    0xd420_0000 | (imm16 as u32) << 5
}

/// `SVC #imm16`: SVC.
#[inline]
pub const fn svc(imm16: u16) -> u32 {
    0xd400_0001 | (imm16 as u32) << 5
}

/// `UDF #imm16`: UDF (permanently undefined).
#[inline]
pub const fn udf(imm16: u16) -> u32 {
    imm16 as u32
}

/// A system register readable with MRS and writable with MSR. The value is
/// the `o0:op1:CRn:CRm:op2` field.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
#[repr(u16)]
pub enum SysReg {
    /// `NZCV`: the condition flags (`S3_3_C4_C2_0`).
    Nzcv = sysreg(3, 3, 4, 2, 0),
    /// `FPCR`: floating-point control (`S3_3_C4_C4_0`).
    Fpcr = sysreg(3, 3, 4, 4, 0),
    /// `FPSR`: floating-point status (`S3_3_C4_C4_1`).
    Fpsr = sysreg(3, 3, 4, 4, 1),
    /// `TPIDR_EL0`: the thread pointer (`S3_3_C13_C0_2`).
    TpidrEl0 = sysreg(3, 3, 13, 0, 2),
}

/// The `o0:op1:CRn:CRm:op2` field of the register `S<op0>_<op1>_C<crn>_C<crm>_<op2>`.
const fn sysreg(op0: u16, op1: u16, crn: u16, crm: u16, op2: u16) -> u16 {
    (op0 - 2) << 14 | op1 << 11 | crn << 7 | crm << 3 | op2
}

impl SysReg {
    /// Every system register this crate names.
    pub const ALL: [Self; 4] = [Self::Nzcv, Self::Fpcr, Self::Fpsr, Self::TpidrEl0];
}

/// `MRS <Xt>, <sysreg>`: MRS.
#[inline]
pub const fn mrs(rt: RegZr, reg: SysReg) -> u32 {
    0xd530_0000 | (reg as u32) << 5 | rt.code()
}

/// `MSR <sysreg>, <Xt>`: MSR (register).
#[inline]
pub const fn msr(reg: SysReg, rt: RegZr) -> u32 {
    0xd510_0000 | (reg as u32) << 5 | rt.code()
}

/// The scope and access types a DMB or DSB applies to: the `CRm` field.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Barrier {
    /// Outer shareable, loads.
    Oshld = 1,
    /// Outer shareable, stores.
    Oshst = 2,
    /// Outer shareable, all.
    Osh = 3,
    /// Non-shareable, loads.
    Nshld = 5,
    /// Non-shareable, stores.
    Nshst = 6,
    /// Non-shareable, all.
    Nsh = 7,
    /// Inner shareable, loads.
    Ishld = 9,
    /// Inner shareable, stores.
    Ishst = 10,
    /// Inner shareable, all.
    Ish = 11,
    /// Full system, loads.
    Ld = 13,
    /// Full system, stores.
    St = 14,
    /// Full system, all.
    Sy = 15,
}

impl Barrier {
    /// Every barrier option.
    pub const ALL: [Self; 12] = [
        Self::Oshld,
        Self::Oshst,
        Self::Osh,
        Self::Nshld,
        Self::Nshst,
        Self::Nsh,
        Self::Ishld,
        Self::Ishst,
        Self::Ish,
        Self::Ld,
        Self::St,
        Self::Sy,
    ];
}

/// `DMB <option>`: DMB (data memory barrier).
#[inline]
pub const fn dmb(option: Barrier) -> u32 {
    0xd503_30bf | (option as u32) << 8
}

/// `DSB <option>`: DSB (data synchronization barrier).
#[inline]
pub const fn dsb(option: Barrier) -> u32 {
    0xd503_309f | (option as u32) << 8
}

/// `ISB`: ISB (instruction synchronization barrier).
#[inline]
pub const fn isb() -> u32 {
    0xd503_3fdf
}
