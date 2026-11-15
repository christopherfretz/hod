//! WP-247: the conventions that keep more registers than AAPCS64 for
//! their caller (`spec/abi-a64.md` §13): `win64cc` (`x18`, LLVM's
//! `CSR_AArch64_AAPCS_X18` for a non-Windows triple), `aarch64_vector_pcs`
//! (the whole of `q8`..`q23`, `CSR_AArch64_AAVPCS`) and, from WP-101,
//! `preserve_allcc` (`x9`..`x15` and the whole of `q8`..`q31`,
//! `CSR_AArch64_RT_AllRegs`).
//!
//! Arguments and results of all three travel as under `ccc`
//! (`abi::assign`), but for a variadic `win64cc` call, whose `float`s and
//! `double`s travel as integers (§13.3). What the callee keeps is the
//! frame's: [`save`] stores the registers of [`Kept`] in a save area right
//! after the frame record, [`rows`] describes them, [`restore`] reloads
//! them in every exit. hod keeps no value in a register across a call, so
//! a call to such a function needs nothing beyond its assignment.

use hod_cg::{CfiOp, CgResult, FnCx, FrameRef};
use hod_ir::{CallConv, SymbolKind, TailKind};
use hod_mc_a64 as mc;
use hod_mc_a64::{Load, MemSize, Reg, Size, VReg};

use crate::common::{FP, put, unsupported};
use crate::target::A64;

/// A `half` or a `bfloat` argument of a variadic `win64cc` call: LLVM
/// passes it as an `i16` in a general register, whose upper bits no rule
/// fixes (§13.3).
pub const WIN64_VARIADIC_HALF: &str = "cc:win64cc-variadic-half";
/// A variadic `win64cc` definition: its `va_list` is a pointer into a
/// home area of the general registers, not §9's structure (§13.3).
pub const WIN64_VARIADIC_DEFINITION: &str = "cc:win64cc-variadic-definition";
/// A `nest` argument of a `win64cc` signature (§13.2).
pub const WIN64_NEST: &str = "cc:win64cc-nest";
/// A `musttail` call in a `win64cc` function (§13.4).
pub const WIN64_MUSTTAIL: &str = "cc:win64cc-musttail";
/// A `win64cc` coroutine body (§13.4).
pub const WIN64_COROUTINE: &str = "cc:win64cc-coroutine";
/// A variadic `aarch64_vector_pcs` definition (§13.5).
pub const VPCS_VARIADIC_DEFINITION: &str = "cc:aarch64_vector_pcs-variadic-definition";
/// A `musttail` call in an `aarch64_vector_pcs` function (§13.5).
pub const VPCS_MUSTTAIL: &str = "cc:aarch64_vector_pcs-musttail";
/// An `aarch64_vector_pcs` coroutine body (§13.5).
pub const VPCS_COROUTINE: &str = "cc:aarch64_vector_pcs-coroutine";

/// Every name of this module's refusals, in the order of §13.6.
pub const UNSUPPORTED: [&str; 8] = [
    WIN64_VARIADIC_HALF,
    WIN64_VARIADIC_DEFINITION,
    WIN64_NEST,
    WIN64_MUSTTAIL,
    WIN64_COROUTINE,
    VPCS_VARIADIC_DEFINITION,
    VPCS_MUSTTAIL,
    VPCS_COROUTINE,
];

/// The registers a function keeps beyond the AAPCS64 callee-saved set,
/// which hod's own code never holds a value in (it saves none of
/// `x19`..`x28` and `d8`..`d15`).
#[derive(Clone, Copy, Debug)]
pub struct Kept {
    /// General registers, saved 8 bytes each from the area's start.
    pub general: &'static [Reg],
    /// Vector registers saved whole, `q8` on, 16 bytes each after the
    /// general ones (rounded up to 16 bytes): how many, an even count.
    pub vectors: u8,
}

/// The first vector register a convention of [`Kept`] saves whole.
const FIRST_V: u8 = 8;

/// Where the save area starts: right after the frame record.
const AREA: u32 = 16;

impl Kept {
    /// The set of convention `cc`; `None` for a convention that keeps
    /// only the AAPCS64 set (or keeps more by another module: WP-095's
    /// `preserve_mostcc`).
    pub fn of(cc: CallConv) -> Option<Kept> {
        match cc {
            CallConv::PRESERVE_ALL => Some(Kept { general: &crate::tail2::PRESERVED, vectors: 24 }),
            CallConv::WIN64 => Some(Kept { general: &[Reg::X18], vectors: 0 }),
            CallConv::AARCH64_VECTOR_PCS => Some(Kept { general: &[], vectors: 16 }),
            _ => None,
        }
    }

    /// The offset from `x29` of the first vector register's slot.
    fn vector_area(self) -> u32 {
        AREA + (8 * self.general.len() as u32).next_multiple_of(16)
    }

    /// The bytes of the save area.
    fn size(self) -> u32 {
        self.vector_area() - AREA + 16 * u32::from(self.vectors)
    }

    /// The bytes of the stores [`save`] emits.
    fn save_bytes(self) -> u32 {
        let general = self.general.len().div_ceil(2) as u32;
        4 * (general + u32::from(self.vectors) / 2)
    }

    /// The vector registers saved whole, by number.
    fn vector_numbers(self) -> std::ops::Range<u8> {
        FIRST_V..FIRST_V + self.vectors
    }

    /// The vector register pairs, each with its slot's offset from `x29`.
    fn vector_pairs(self) -> impl Iterator<Item = (VReg, VReg, i64)> {
        let base = i64::from(self.vector_area());
        self.vector_numbers().step_by(2).filter_map(move |n| {
            let at = base + 16 * i64::from(n - FIRST_V);
            Some((VReg::new(n)?, VReg::new(n + 1)?, at))
        })
    }

    /// Each register's DWARF number with its slot's offset from `x29`
    /// (a vector register's number is 64 plus its index).
    fn slots(self) -> impl Iterator<Item = (u16, i32)> {
        let general = self
            .general
            .iter()
            .enumerate()
            .map(|(index, reg)| (u16::from(reg.index()), AREA as i32 + 8 * index as i32));
        let base = self.vector_area();
        let vector = self
            .vector_numbers()
            .map(move |n| (64 + u16::from(n), (base + 16 * u32::from(n - FIRST_V)) as i32));
        general.chain(vector)
    }
}

/// The convention of the function being compiled.
fn convention(cx: &FnCx<'_, A64>) -> CallConv {
    match cx.module.symbol(cx.body.symbol()).kind() {
        SymbolKind::Function { cc, .. } => cc,
        _ => CallConv::C,
    }
}

/// What the function being compiled keeps beyond AAPCS64 by this module.
pub fn kept(cx: &FnCx<'_, A64>) -> Option<Kept> {
    Kept::of(convention(cx))
}

/// The function-level refusals of §13.4 and §13.5.
fn check(cx: &FnCx<'_, A64>, cc: CallConv) -> CgResult<()> {
    let [variadic, musttail, coroutine] = match cc {
        CallConv::WIN64 => [WIN64_VARIADIC_DEFINITION, WIN64_MUSTTAIL, WIN64_COROUTINE],
        CallConv::AARCH64_VECTOR_PCS => [VPCS_VARIADIC_DEFINITION, VPCS_MUSTTAIL, VPCS_COROUTINE],
        _ => return Ok(()),
    };
    if crate::frame::is_variadic(cx) {
        return unsupported(variadic);
    }
    if cx.body.calls().any(|site| site.tail() == TailKind::MustTail) {
        return unsupported(musttail);
    }
    if cx.has_coro_frame() {
        return unsupported(coroutine);
    }
    Ok(())
}

/// From `frame::begin`, right after `mov x29, sp`: the registers of
/// [`Kept`] stored in the save area. Every lowering may write them and
/// every call clobbers them, so all are saved (`llc` saves those its
/// allocator or a call touched, which at `-O0` with a call is all).
///
/// # Errors
/// A name of [`UNSUPPORTED`]; what `alloc_slot` reports.
pub fn save(cx: &mut FnCx<'_, A64>) -> CgResult<()> {
    let cc = convention(cx);
    check(cx, cc)?;
    let Some(kept) = Kept::of(cc) else { return Ok(()) };
    if cx.alloc_slot(u64::from(kept.size()), 16)? != FrameRef::Local(AREA) {
        return cx.bug("the kept registers' save area is not after the frame record");
    }
    for (index, pair) in kept.general.chunks(2).enumerate() {
        let at = i64::from(AREA) + 16 * index as i64;
        match *pair {
            [a, b] => put(cx, mc::stp_off(Size::X, a.zr(), b.zr(), FP, at)),
            [a] => put(cx, mc::str_off(MemSize::X, a.zr(), FP, at)),
            _ => {}
        }
    }
    for (a, b, at) in kept.vector_pairs() {
        put(cx, mc::stp_fp_off(mc::VPairSize::Q, a, b, FP, at));
    }
    Ok(())
}

/// The rules saying where [`save`] saved each register, given the CFA is
/// `x29 + frame`: from `after_prologue` plus the bytes of the stores in
/// `end`, and again (`None`) in the body rows a tail exit restates.
///
/// # Errors
/// What `cfi_at` reports.
pub fn rows(cx: &mut FnCx<'_, A64>, after_prologue: Option<u32>, frame: u32) -> CgResult<()> {
    let Some(kept) = kept(cx) else { return Ok(()) };
    for (reg, saved) in kept.slots() {
        let op = CfiOp::Offset { reg, cfa_offset: saved - frame as i32 };
        match after_prologue {
            Some(at) => cx.cfi_at(at + kept.save_bytes(), op)?,
            None => cx.cfi(op),
        }
    }
    Ok(())
}

/// From `frame::teardown`, before the frame record is reloaded: the
/// registers [`save`] saved, reloaded, each rule back to "same value".
pub fn restore(cx: &mut FnCx<'_, A64>) {
    let Some(kept) = kept(cx) else { return };
    for (index, pair) in kept.general.chunks(2).enumerate() {
        let at = i64::from(AREA) + 16 * index as i64;
        match *pair {
            [a, b] => put(cx, mc::ldp_off(Size::X, a.zr(), b.zr(), FP, at)),
            [a] => put(cx, mc::ldr_off(Load::X, a.zr(), FP, at)),
            _ => {}
        }
    }
    for (a, b, at) in kept.vector_pairs() {
        put(cx, mc::ldp_fp_off(mc::VPairSize::Q, a, b, FP, at));
    }
    for (reg, _) in kept.slots() {
        cx.cfi(CfiOp::Restore { reg });
    }
}
