//! Building blocks for atomics: exclusive and ordered loads and stores, and
//! the LSE compare-and-swap, swap and atomic memory operations
//! (`spec/mc-a64.md` "Atomics").

use crate::mem::MemSize;
use crate::reg::{Reg, RegSp, RegZr, Size};

const ZR: u32 = 31;

/// Load/store exclusive and ordered: `flags` carries `o2`, `L`, `o1`, `o0`
/// in place.
#[inline]
const fn exclusive(size: u32, flags: u32, rs: u32, rt2: u32, rn: RegSp, rt: RegZr) -> u32 {
    size << 30 | 0x0800_0000 | flags | rs << 16 | rt2 << 10 | rn.code() << 5 | rt.code()
}

const LOAD: u32 = 1 << 22;
const PAIR: u32 = 1 << 21;
const ACQ_REL: u32 = 1 << 15;
const ORDERED: u32 = 1 << 23;

/// The `size` field of an exclusive pair: 2 for `W`, 3 for `X`.
#[inline]
const fn pair_size(sz: Size) -> u32 {
    2 + sz.bit()
}

/// `LDXR* <Rt>, [<Xn|SP>]`: LDXR, LDXRB, LDXRH (load exclusive).
#[inline]
pub const fn ldxr(ms: MemSize, rt: RegZr, rn: RegSp) -> u32 {
    exclusive(ms.log2(), LOAD, ZR, ZR, rn, rt)
}

/// `LDAXR* <Rt>, [<Xn|SP>]`: LDAXR, LDAXRB, LDAXRH (load-acquire
/// exclusive).
#[inline]
pub const fn ldaxr(ms: MemSize, rt: RegZr, rn: RegSp) -> u32 {
    exclusive(ms.log2(), LOAD | ACQ_REL, ZR, ZR, rn, rt)
}

/// `LDAR* <Rt>, [<Xn|SP>]`: LDAR, LDARB, LDARH (load-acquire).
#[inline]
pub const fn ldar(ms: MemSize, rt: RegZr, rn: RegSp) -> u32 {
    exclusive(ms.log2(), ORDERED | LOAD | ACQ_REL, ZR, ZR, rn, rt)
}

/// `STLR* <Rt>, [<Xn|SP>]`: STLR, STLRB, STLRH (store-release).
#[inline]
pub const fn stlr(ms: MemSize, rt: RegZr, rn: RegSp) -> u32 {
    exclusive(ms.log2(), ORDERED | ACQ_REL, ZR, ZR, rn, rt)
}

/// `CLREX`: CLREX (clear the exclusive monitor).
#[inline]
pub const fn clrex() -> u32 {
    0xd503_3f5f
}

/// The status register of a store exclusive must not be a transferred
/// register, nor the base unless the base is SP.
#[inline]
const fn status_ok(rs: RegZr, rt: RegZr, rn: RegSp) -> bool {
    rs.code() != rt.code() && (rn.is_sp() || rs.code() != rn.code())
}

checked! {
    /// `STXR* <Ws>, <Rt>, [<Xn|SP>]`: STXR, STXRB, STXRH (store exclusive;
    /// `rs` receives 0 on success).
    ///
    /// Precondition: `rs` and `rt` have different encodings, and `rs` is
    /// not the same register as `rn` (always true when `rn` is SP).
    pub fn stxr / try_stxr(ms: MemSize, rs: RegZr, rt: RegZr, rn: RegSp)
    requires status_ok(rs, rt, rn);
    { exclusive(ms.log2(), 0, rs.code(), ZR, rn, rt) }

    /// `STLXR* <Ws>, <Rt>, [<Xn|SP>]`: STLXR, STLXRB, STLXRH
    /// (store-release exclusive).
    ///
    /// Precondition: as [`stxr`].
    pub fn stlxr / try_stlxr(ms: MemSize, rs: RegZr, rt: RegZr, rn: RegSp)
    requires status_ok(rs, rt, rn);
    { exclusive(ms.log2(), ACQ_REL, rs.code(), ZR, rn, rt) }

    /// `LDXP <Rt>, <Rt2>, [<Xn|SP>]`: LDXP (load exclusive pair).
    ///
    /// Precondition: `rt` and `rt2` have different encodings.
    pub fn ldxp / try_ldxp(sz: Size, rt: RegZr, rt2: RegZr, rn: RegSp)
    requires rt.code() != rt2.code();
    { exclusive(pair_size(sz), LOAD | PAIR, ZR, rt2.code(), rn, rt) }

    /// `LDAXP <Rt>, <Rt2>, [<Xn|SP>]`: LDAXP (load-acquire exclusive pair).
    ///
    /// Precondition: `rt` and `rt2` have different encodings.
    pub fn ldaxp / try_ldaxp(sz: Size, rt: RegZr, rt2: RegZr, rn: RegSp)
    requires rt.code() != rt2.code();
    { exclusive(pair_size(sz), LOAD | PAIR | ACQ_REL, ZR, rt2.code(), rn, rt) }

    /// `STXP <Ws>, <Rt>, <Rt2>, [<Xn|SP>]`: STXP (store exclusive pair).
    ///
    /// Precondition: `rs` has a different encoding from `rt` and from
    /// `rt2`, and is not the same register as `rn` (always true when `rn`
    /// is SP).
    pub fn stxp / try_stxp(sz: Size, rs: RegZr, rt: RegZr, rt2: RegZr, rn: RegSp)
    requires status_ok(rs, rt, rn) && rs.code() != rt2.code();
    { exclusive(pair_size(sz), PAIR, rs.code(), rt2.code(), rn, rt) }

    /// `STLXP <Ws>, <Rt>, <Rt2>, [<Xn|SP>]`: STLXP (store-release exclusive
    /// pair).
    ///
    /// Precondition: as [`stxp`].
    pub fn stlxp / try_stlxp(sz: Size, rs: RegZr, rt: RegZr, rt2: RegZr, rn: RegSp)
    requires status_ok(rs, rt, rn) && rs.code() != rt2.code();
    { exclusive(pair_size(sz), PAIR | ACQ_REL, rs.code(), rt2.code(), rn, rt) }

    /// `CASP* <Rs>, <Rs+1>, <Rt>, <Rt+1>, [<Xn|SP>]`: CASP, CASPA, CASPL,
    /// CASPAL (compare and swap a pair; LSE). `rs` and `rt` name the first
    /// register of each pair.
    ///
    /// Precondition: `rs` and `rt` are even-numbered registers.
    pub fn casp / try_casp(sz: Size, order: Order, rs: Reg, rt: Reg, rn: RegSp)
    requires rs.index().is_multiple_of(2) && rt.index().is_multiple_of(2);
    {
        sz.bit() << 30
            | 0x0820_7c00
            | order.acquire() << 22
            | rs.code() << 16
            | order.release() << 15
            | rn.code() << 5
            | rt.code()
    }
}

/// The memory ordering of an LSE instruction: the mnemonic's `A` and `L`
/// suffixes.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Order {
    /// No suffix: no ordering beyond atomicity.
    Plain = 0,
    /// `L`: release.
    L = 1,
    /// `A`: acquire.
    A = 2,
    /// `AL`: acquire and release.
    Al = 3,
}

impl Order {
    /// Every ordering.
    pub const ALL: [Self; 4] = [Self::Plain, Self::L, Self::A, Self::Al];

    #[inline]
    const fn acquire(self) -> u32 {
        self as u32 >> 1
    }

    #[inline]
    const fn release(self) -> u32 {
        self as u32 & 1
    }
}

/// `CAS* <Rs>, <Rt>, [<Xn|SP>]`: CAS, CASA, CASL, CASAL and their `B`/`H`
/// forms (compare and swap; LSE). `rs` holds the expected value and
/// receives the value read; `rt` is the new value.
#[inline]
pub const fn cas(ms: MemSize, order: Order, rs: RegZr, rt: RegZr, rn: RegSp) -> u32 {
    ms.log2() << 30
        | 0x08a0_7c00
        | order.acquire() << 22
        | rs.code() << 16
        | order.release() << 15
        | rn.code() << 5
        | rt.code()
}

/// Atomic memory operations and SWP: `op` is `o3:opc` (bits 15:12).
#[inline]
const fn atomic_mem(op: u32, ms: MemSize, order: Order, rs: RegZr, rt: RegZr, rn: RegSp) -> u32 {
    ms.log2() << 30
        | 0x3820_0000
        | (order as u32) << 22
        | rs.code() << 16
        | op << 12
        | rn.code() << 5
        | rt.code()
}

/// `SWP* <Rs>, <Rt>, [<Xn|SP>]`: SWP and its ordered and `B`/`H` forms
/// (LSE). Stores `rs`, loads the old value into `rt`.
#[inline]
pub const fn swp(ms: MemSize, order: Order, rs: RegZr, rt: RegZr, rn: RegSp) -> u32 {
    atomic_mem(0b1000, ms, order, rs, rt, rn)
}

/// `LDADD* <Rs>, <Rt>, [<Xn|SP>]`: LDADD family (LSE). Memory becomes
/// `old + rs`; `rt` receives `old`.
#[inline]
pub const fn ldadd(ms: MemSize, order: Order, rs: RegZr, rt: RegZr, rn: RegSp) -> u32 {
    atomic_mem(0, ms, order, rs, rt, rn)
}

/// `LDCLR* <Rs>, <Rt>, [<Xn|SP>]`: LDCLR family (LSE). Memory becomes
/// `old & !rs`.
#[inline]
pub const fn ldclr(ms: MemSize, order: Order, rs: RegZr, rt: RegZr, rn: RegSp) -> u32 {
    atomic_mem(1, ms, order, rs, rt, rn)
}

/// `LDEOR* <Rs>, <Rt>, [<Xn|SP>]`: LDEOR family (LSE). Memory becomes
/// `old ^ rs`.
#[inline]
pub const fn ldeor(ms: MemSize, order: Order, rs: RegZr, rt: RegZr, rn: RegSp) -> u32 {
    atomic_mem(2, ms, order, rs, rt, rn)
}

/// `LDSET* <Rs>, <Rt>, [<Xn|SP>]`: LDSET family (LSE). Memory becomes
/// `old | rs`.
#[inline]
pub const fn ldset(ms: MemSize, order: Order, rs: RegZr, rt: RegZr, rn: RegSp) -> u32 {
    atomic_mem(3, ms, order, rs, rt, rn)
}

/// `LDSMAX* <Rs>, <Rt>, [<Xn|SP>]`: LDSMAX family (LSE). Signed maximum.
#[inline]
pub const fn ldsmax(ms: MemSize, order: Order, rs: RegZr, rt: RegZr, rn: RegSp) -> u32 {
    atomic_mem(4, ms, order, rs, rt, rn)
}

/// `LDSMIN* <Rs>, <Rt>, [<Xn|SP>]`: LDSMIN family (LSE). Signed minimum.
#[inline]
pub const fn ldsmin(ms: MemSize, order: Order, rs: RegZr, rt: RegZr, rn: RegSp) -> u32 {
    atomic_mem(5, ms, order, rs, rt, rn)
}

/// `LDUMAX* <Rs>, <Rt>, [<Xn|SP>]`: LDUMAX family (LSE). Unsigned maximum.
#[inline]
pub const fn ldumax(ms: MemSize, order: Order, rs: RegZr, rt: RegZr, rn: RegSp) -> u32 {
    atomic_mem(6, ms, order, rs, rt, rn)
}

/// `LDUMIN* <Rs>, <Rt>, [<Xn|SP>]`: LDUMIN family (LSE). Unsigned minimum.
#[inline]
pub const fn ldumin(ms: MemSize, order: Order, rs: RegZr, rt: RegZr, rn: RegSp) -> u32 {
    atomic_mem(7, ms, order, rs, rt, rn)
}
