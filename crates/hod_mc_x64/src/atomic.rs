//! The `lock` prefix, exchanges, compare-and-exchange and fences
//! (`spec/mc-x64.md` "Atomics").

use crate::inst::{Inst, Op, digit_m, plain, plus_r, rmem, rr};
use crate::reg::{Mem, Reg, Size};

checked! {
    /// `inst` with a `LOCK` prefix (`F0`), placed after its segment and
    /// operand-size prefixes. The precondition is
    /// [`Inst::is_lockable`]: a lockable instruction with a memory
    /// destination, not already locked.
    pub fn lock / try_lock(inst: Inst)
    requires inst.is_lockable();
    { inst.insert_lock() }
}

/// `XCHG r, r`: `90+r` when one operand is the accumulator and the size is
/// 16, 32 or 64 bits, except `xchg eax, eax` (`87 C0`, which zeroes the
/// upper half of `rax`, where `90` does not); `86`/`87 /r` otherwise.
/// `xchg rax, rax` is `90`, which is `nop`.
#[inline]
pub const fn xchg_rr(size: Size, a: Reg, b: Reg) -> Inst {
    let acc = matches!(a, Reg::Rax) || matches!(b, Reg::Rax);
    let both = matches!(a, Reg::Rax) && matches!(b, Reg::Rax);
    match size {
        Size::Q if both => plain(Op::one(0x90)),
        Size::W | Size::Q if acc => {
            let other = if matches!(a, Reg::Rax) { b } else { a };
            plus_r(Op::one(0x90).sized(size, 0), other, false)
        }
        Size::D if acc && !both => {
            let other = if matches!(a, Reg::Rax) { b } else { a };
            plus_r(Op::one(0x90), other, false)
        }
        _ => rr(Op::one(0x86).sized(size, 1), size, a, b),
    }
}

/// `XCHG m, r` (`86`/`87 /r`). Atomic without a `lock` prefix; lockable.
#[inline]
pub const fn xchg_mr(size: Size, m: Mem, r: Reg) -> Inst {
    rmem(Op::one(0x86).sized(size, 1), size, r, m).lockable()
}

/// `XADD m, r` (`0F C0`/`C1 /r`): `tmp = [m]; [m] += r; r = tmp`.
/// Lockable.
#[inline]
pub const fn xadd_mr(size: Size, m: Mem, r: Reg) -> Inst {
    rmem(Op::two(0xc0).sized(size, 1), size, r, m).lockable()
}

/// `CMPXCHG m, r` (`0F B0`/`B1 /r`): if `[m] == rax` (by size), `[m] = r`,
/// else `rax = [m]`; ZF says which. Lockable.
#[inline]
pub const fn cmpxchg_mr(size: Size, m: Mem, r: Reg) -> Inst {
    rmem(Op::two(0xb0).sized(size, 1), size, r, m).lockable()
}

/// `CMPXCHG8B m64` (`0F C7 /1`): compares `edx:eax` with the 8 bytes at
/// `m` and stores `ecx:ebx` if equal, else loads them into `edx:eax`.
/// Lockable.
#[inline]
pub const fn cmpxchg8b_m(m: Mem) -> Inst {
    digit_m(Op::two(0xc7), 1, m).lockable()
}

/// `CMPXCHG16B m128` (`REX.W 0F C7 /1`): compares `rdx:rax` with the 16
/// bytes at `m` (16-byte aligned) and stores `rcx:rbx` if equal. Lockable.
#[inline]
pub const fn cmpxchg16b_m(m: Mem) -> Inst {
    digit_m(Op::two(0xc7).sized(Size::Q, 0), 1, m).lockable()
}

/// `MFENCE` (`0F AE F0`).
#[inline]
pub const fn mfence() -> Inst {
    let mut inst = plain(Op::two(0xae));
    inst.push(0xf0);
    inst
}

/// `LFENCE` (`0F AE E8`).
#[inline]
pub const fn lfence() -> Inst {
    let mut inst = plain(Op::two(0xae));
    inst.push(0xe8);
    inst
}

/// `SFENCE` (`0F AE F8`).
#[inline]
pub const fn sfence() -> Inst {
    let mut inst = plain(Op::two(0xae));
    inst.push(0xf8);
    inst
}

/// `PAUSE` (`F3 90`).
#[inline]
pub const fn pause() -> Inst {
    plain(Op::one(0x90).rep(0xf3))
}
