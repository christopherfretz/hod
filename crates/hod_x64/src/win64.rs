//! WP-246: the `win64cc` convention on `x86_64-unknown-linux-gnu`
//! (`__attribute__((ms_abi))`): `spec/abi-x64.md` §13.
//!
//! [`assign`] places the IR-level arguments of a `win64cc` signature as
//! `llc` 19 and 22 do for a Linux triple (LLVM's `CC_X86_Win64_C`): four
//! argument *positions*, each either the general register or the `xmm`
//! register of its number (`rcx`/`xmm0`, `rdx`/`xmm1`, `r8`/`xmm2`,
//! `r9`/`xmm3`), then 8-byte stack slots above the 32-byte shadow area
//! the caller allocates. Return values travel as under `ccc` (LLVM's
//! `RetCC_X86_Win64_C` delegates to the System V rules, §5.1), except
//! that the address of a value returned in memory arrives in `rcx`.
//!
//! What a `win64cc` function keeps for its caller beyond the System V
//! set (`rsi`, `rdi`, `xmm6`..`xmm15`, LLVM's `CSR_Win64`) is the
//! frame's (`frame.rs`, [`KEPT_XMMS`]); a call site keeps no value in a
//! register across any call, so a call to a `win64cc` function needs
//! nothing beyond its assignment.

use hod_cg::PReg;
use hod_cg::abi::{
    AbiUnsupported, Assignment, Classify, Part, PieceLoc, Place, RetMemory, Sig, charge,
    check_type, place_leaf, walk,
};
use hod_ir::{CallConv, ParamMem, TypeId, TypeKind, TypeTable};
use hod_mc_x64::Xmm;

use crate::abi::{self, Class, Features, LONE_LANE, Leaf, MAX_PIECES, RETURNS_ADDRESS};
use crate::reg::{R8, R9, RCX, RDX, xmm};

/// The general register of each argument position (§13.2).
pub const POSITION_GPRS: [PReg; 4] = [RCX, RDX, R8, R9];

/// The `xmm` register of each argument position (§13.2).
pub const POSITION_XMMS: [PReg; 4] =
    [xmm(Xmm::Xmm0), xmm(Xmm::Xmm1), xmm(Xmm::Xmm2), xmm(Xmm::Xmm3)];

/// The bytes of the shadow area the caller allocates below the stack
/// arguments (§13.2): the first stack argument is at offset 32.
pub const SHADOW: u32 = 32;

/// The register that carries the address of a value returned in memory
/// (§13.4): the first position's.
pub const INDIRECT_RESULT: PReg = RCX;

/// A vector argument: LLVM passes it by reference to a copy the caller
/// makes, which no `PartKind` expresses (§13.3).
pub const VECTOR: &str = "cc:win64cc-vector";
/// An `x86_fp80` argument: passed by reference, as a vector (§13.3).
pub const X86_FP80: &str = "cc:win64cc-x86-fp80";
/// An `fp128` argument: `llc` 22 passes it by reference, `llc` 19 in an
/// `xmm` (§13.3).
pub const FP128: &str = "cc:win64cc-fp128";
/// A `bfloat` argument: LLVM's convention has no rule for it (§13.3).
pub const BFLOAT: &str = "cc:win64cc-bfloat";
/// An integer argument wider than 64 bits other than `i128` (§13.3).
pub const WIDE_INT: &str = "cc:win64cc-wide-int";
/// A `byval` argument: LLVM passes the address of a copy the caller
/// makes (§13.3).
pub const BYVAL: &str = "cc:win64cc-byval";
/// A `float` or `half` in a register position of a variadic call: its
/// copy in the position's general register would be the `xmm`'s low 8
/// bytes, of which only 4 or 2 are the value (§13.5).
pub const VARIADIC_FLOAT: &str = "cc:win64cc-variadic-float";
/// A variadic `win64cc` definition: its `va_list` is a pointer into the
/// home area, not §9's structure (§13.5).
pub const VARIADIC_DEFINITION: &str = "cc:win64cc-variadic-definition";
/// A `musttail` call in a `win64cc` function (§13.6).
pub const MUSTTAIL: &str = "cc:win64cc-musttail";
/// A `win64cc` coroutine body (§13.6).
pub const COROUTINE: &str = "cc:win64cc-coroutine";

/// Every name of this convention's refusals, in the order of §13.7.
pub const UNSUPPORTED: [&str; 10] = [
    VECTOR,
    X86_FP80,
    FP128,
    BFLOAT,
    WIDE_INT,
    BYVAL,
    VARIADIC_FLOAT,
    VARIADIC_DEFINITION,
    MUSTTAIL,
    COROUTINE,
];

/// The `xmm` registers a `win64cc` function keeps (their low 16 bytes),
/// as a mask by number: `xmm6`..`xmm15` (§13.6). `rsi` and `rdi` are
/// the general ones (`frame::win64_plan`).
pub const KEPT_XMMS: u16 = 0xffc0;

type Abi<T> = Result<T, AbiUnsupported>;

fn no<T>(name: &'static str) -> Abi<T> {
    Err(AbiUnsupported(name))
}

/// §13.3: the pieces of a leaf type as this convention passes it: the
/// System V classification (§4) of the scalars it passes by value.
#[derive(Clone, Copy, Debug)]
struct Win64Leaves(Features);

impl Classify for Win64Leaves {
    type Class = Class;

    fn leaf(&self, types: &TypeTable, ty: TypeId) -> Abi<Leaf> {
        match types.kind(ty) {
            TypeKind::Int(bits) if bits > 64 && bits != 128 => no(WIDE_INT),
            TypeKind::BFloat => no(BFLOAT),
            TypeKind::Fp128 => no(FP128),
            TypeKind::X86Fp80 => no(X86_FP80),
            TypeKind::Vector { .. } => no(VECTOR),
            _ => abi::classify(types, ty, self.0),
        }
    }
}

/// The positions taken and the stack-argument area so far (§13.2).
struct State {
    position: usize,
    stack: u32,
}

impl State {
    /// The place of the next piece, of class `class`: each piece takes a
    /// position, an `i128`'s two halves too (§13.2).
    fn piece(&mut self, class: Class) -> PieceLoc {
        let n = self.position;
        if n < POSITION_GPRS.len() {
            self.position += 1;
            return PieceLoc::Reg(if class == Class::Gpr {
                POSITION_GPRS[n]
            } else {
                POSITION_XMMS[n]
            });
        }
        let at = self.stack;
        self.stack = self.stack.saturating_add(8);
        PieceLoc::Stack(u64::from(at))
    }
}

/// Where the arguments and the return value of the `win64cc` signature
/// `sig` travel (§13), for a function compiled with `features`. `out` is
/// empty (`abi::assign` cleared it, and clears it again on an error).
///
/// # Errors
/// A name of [`UNSUPPORTED`], or of `abi::UNSUPPORTED` for what both
/// conventions refuse.
pub fn assign(
    types: &TypeTable,
    sig: &Sig<'_>,
    features: Features,
    out: &mut Assignment,
) -> Abi<()> {
    let fixed = if sig.variadic { sig.fixed as usize } else { sig.params.len() };
    if fixed > sig.params.len() || u32::try_from(sig.params.len()).is_err() {
        return no(abi::INVALID_SIGNATURE);
    }
    let mut state = State { position: 0, stack: SHADOW };
    // §13.4: returned as under `ccc`; the address of a value in memory in
    // the first position, handed back in `rax`.
    let as_c = Sig { cc: CallConv::C, ..*sig };
    if abi::assign_return(types, &as_c, features, &mut out.ret)? {
        state.position = 1;
        out.ret_memory = Some(RetMemory {
            address: Place::Reg(INDIRECT_RESULT),
            returned_in: Some(RETURNS_ADDRESS),
        });
    }
    let leaves = Win64Leaves(features);
    let mut budget = MAX_PIECES;
    let returns_value = sig.ret != TypeId::VOID;
    for (index, &ty) in sig.params.iter().enumerate() {
        let attrs = sig.attrs(index);
        let arg = index as u32;
        // `nest`, the `swift*` attributes and misplaced extensions are
        // refused as under `ccc`; `inreg` is ignored (§13.3).
        abi::check_flags(types, ty, attrs)?;
        match attrs.mem() {
            ParamMem::None => {}
            ParamMem::ByRef if types.is_ptr(ty) => {}
            ParamMem::Sret if abi::is_plain_pointer(types, ty) && arg <= 1 && !returns_value => {
                // §13.4: an ordinary pointer, handed back in `rax`.
                out.sret_returned_in = Some(RETURNS_ADDRESS);
            }
            ParamMem::ByVal => return no(BYVAL),
            mem => return no(abi::mem_refusal(mem)),
        }
        check_type(types, ty, &leaves)?;
        walk(types, ty, 0, &mut |leaf_ty, offset| {
            let leaf = leaves.leaf(types, leaf_ty)?;
            charge(&mut budget, leaf.count)?;
            let first = out.args.len();
            place_leaf(&leaf, arg, offset, &mut out.args, &mut |leaf, _| {
                Ok::<_, AbiUnsupported>(state.piece(leaf.class))
            })?;
            if sig.variadic { shadow_copies(&mut out.args, first) } else { Ok(()) }
        })?;
    }
    out.stack_size = state.stack;
    Ok(())
}

/// §13.5: in a variadic call, a `double` in a register position is also
/// in the position's general register (a lane move of its 8 bytes,
/// [`LONE_LANE`], right after its `xmm` part); a `float` or `half` there
/// is refused.
fn shadow_copies(parts: &mut Vec<Part>, first: usize) -> Abi<()> {
    let mut index = first;
    while index < parts.len() {
        let part = parts[index];
        index += 1;
        let Place::Reg(reg) = part.place else { continue };
        let Some(n) = POSITION_XMMS.iter().position(|&x| x == reg) else { continue };
        if part.size != 8 {
            return no(VARIADIC_FLOAT);
        }
        let copy = Part { kind: LONE_LANE, place: Place::Reg(POSITION_GPRS[n]), ..part };
        parts.insert(index, copy);
        index += 1;
    }
    Ok(())
}
