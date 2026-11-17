//! Floating point on the x87 stack: design 0008's I9 (WP-300,
//! `spec/codegen-x86.md` "Floating point on the x87").
//!
//! `float`, `double` and `x86_fp80` are class `Mem`: a frame slot of the
//! type's size (4, 8, 12 bytes; 10 of them the `x86_fp80` image). Loads,
//! stores, `phi`s, `freeze` and `select` copy the bytes with integer
//! instructions (bit-exact for every encoding). An operation pushes its
//! operands (`fld` at their width), operates, and pops its result to its
//! slot at its type's width (`fstp`), which is where the result rounds.
//!
//! # 80-bit copies (excess precision as `llc` has it)
//!
//! `llc -O0` (19 and 22 alike, measured) keeps a computed result in an
//! x87 register, at 80 bits, for every use in its block that comes
//! before the next call; a use in another block or after a call reads it
//! rounded to its type (a spill), and so do some uses when more values
//! are live than the 7 registers hold (which ones depends on its
//! allocator). `freeze`, `fpext`, `fptrunc (fpext x)` back to `x`'s type,
//! `select` of a constant condition and, with `cmov`, `select` (`fcmov`)
//! pass the 80 bits on.
//!
//! hod has one rule ([`Copies`]): a *run* is the instructions after a
//! computed `float`/`double` result up to the next [`barrier`] (a call,
//! anything `llc` turns into one, a `select` that `llc` turns into a
//! branch, the terminator). A computed result with an instruction of its
//! run after it is stored twice: at 80 bits (`fstp tbyte`) into a
//! 12-byte frame slot, and rounded to its type into its home. A use in
//! the run loads the 80-bit copy, every other use the home. The x87
//! stack is empty between instructions. Where `llc`'s allocator rounds
//! a use of a run (more live values than registers) hod keeps the 80
//! bits (the spec's "The divergence").

use hod_cg::sym::{FpOp, FpType, fp_routine};
use hod_cg::{CallTarget, CgError, CgResult, FnCx, FrameRef, RuntimeFn, SymRef};
use hod_ir::{
    Arch, Const, FloatPredicate, FnStr, Inst, InstId, InstKind, Intrinsic, MemFlags, Opcode,
    SymbolKind, TypeId, TypeKind, Value, ValueKind, X86Features,
};
use hod_mc_x64 as mc;
use hod_mc_x64::{Cond, Mem, Reg, Size, St, X87Size};

use crate::common::{
    EAX, ECX, EDX, ESP, const_int, copy_frame, frame_inst, gp, put, sym_inst, unsupported, value_ty,
};
use crate::target::X86;

/// What `abi.rs` still refuses: a floating-point argument or result of a
/// call or a function (design 0008 I6 passes them).
pub const FLOAT: &str = "x86:float";
/// `half`.
pub const HALF: &str = "x86:float:half";
/// `bfloat`.
pub const BFLOAT: &str = "x86:float:bfloat";
/// `fp128`.
pub const FP128: &str = "x86:float:fp128";
/// A `float` operation under SSE, a `double` one under SSE2: `llc`
/// computes those in `xmm` registers (design 0008 I12).
pub const SSE: &str = "x86:float:sse";
/// A conversion between a floating-point type and an integer wider than
/// 32 bits (with design 0008 I7's 64-bit integers).
pub const I64_CAST: &str = "x86:float:i64-cast";
/// A floating-point intrinsic this package does not lower.
pub const INTRINSIC: &str = "x86:float:intrinsic";
/// A cast of a floating value that §11 does not lower (no IR that the
/// verifier accepts reaches it since the i386 join E lowers `bitcast`).
pub const CAST: &str = "x86:float:cast";
/// A `bitcast` between `x86_fp80` and `i80`.
pub const FP80_CAST: &str = "x86:float:fp80-cast";

/// One of the three x87 types.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fp {
    /// `float`.
    F32,
    /// `double`.
    F64,
    /// `x86_fp80`.
    F80,
}

impl Fp {
    const fn size(self) -> X87Size {
        match self {
            Fp::F32 => X87Size::S,
            Fp::F64 => X87Size::D,
            Fp::F80 => X87Size::T,
        }
    }

    /// The bytes of the encoding.
    const fn bytes(self) -> u32 {
        match self {
            Fp::F32 => 4,
            Fp::F64 => 8,
            Fp::F80 => 10,
        }
    }

    /// The bytes it takes as a stack argument.
    const fn arg_bytes(self) -> u32 {
        match self {
            Fp::F32 => 4,
            Fp::F64 => 8,
            Fp::F80 => 12,
        }
    }

    const fn fp_type(self) -> FpType {
        match self {
            Fp::F32 => FpType::Float,
            Fp::F64 => FpType::Double,
            Fp::F80 => FpType::X86Fp80,
        }
    }
}

/// Whether `kind` is a floating-point type.
const fn is_fp_kind(kind: &TypeKind<'_>) -> bool {
    matches!(
        kind,
        TypeKind::Half
            | TypeKind::BFloat
            | TypeKind::Float
            | TypeKind::Double
            | TypeKind::Fp128
            | TypeKind::X86Fp80
    )
}

fn is_fp(cx: &FnCx<'_, X86>, ty: TypeId) -> bool {
    is_fp_kind(&cx.types().kind(ty))
}

/// The x87 type of `ty`, or its refusal.
fn fp_of(cx: &FnCx<'_, X86>, ty: TypeId) -> CgResult<Fp> {
    let fp = match cx.types().kind(ty) {
        TypeKind::Float => Fp::F32,
        TypeKind::Double => Fp::F64,
        TypeKind::X86Fp80 => Fp::F80,
        TypeKind::Half => return unsupported(HALF),
        TypeKind::BFloat => return unsupported(BFLOAT),
        TypeKind::Fp128 => return unsupported(FP128),
        _ => return cx.bug("an x87 operation on a type that is no floating type"),
    };
    let features = features(cx);
    let sse = match fp {
        Fp::F32 => features.has(X86Features::SSE),
        Fp::F64 => features.has(X86Features::SSE2),
        Fp::F80 => false,
    };
    if sse { unsupported(SSE) } else { Ok(fp) }
}

/// The CPU features of the function, as `llc` decides them: its
/// `target-cpu`, else `-mcpu` ([`X86::cpu`]), else `generic`; then its
/// `target-features` (unknown CPU: none, `generic`'s).
fn features(cx: &FnCx<'_, X86>) -> X86Features {
    let mcpu = cx.target.cpu();
    let SymbolKind::Function { attrs, .. } = cx.module.symbol(cx.body.symbol()).kind() else {
        return X86Features::of_function(None, None, mcpu, &[]).unwrap_or(X86Features::NONE);
    };
    let attrs = cx.module.fn_attrs(attrs);
    let text = |what| attrs.string(what).and_then(|s| std::str::from_utf8(s).ok());
    X86Features::of_function(text(FnStr::TargetCpu), text(FnStr::TargetFeatures), mcpu, &[])
        .unwrap_or(X86Features::NONE)
}

// ---- which instructions this module lowers ----

/// The intrinsics of a floating-point value lowered here.
fn fp_intrinsic(which: Intrinsic<'_>) -> bool {
    matches!(which, Intrinsic::Sqrt | Intrinsic::Fabs | Intrinsic::CopySign | Intrinsic::FmulAdd)
        || routine_op(which).is_some()
}

/// Whether `inst` is this module's: a floating result or operand.
pub fn owns(cx: &FnCx<'_, X86>, inst: Inst<'_>) -> bool {
    match inst.kind() {
        InstKind::Binary { .. }
        | InstKind::Unary { .. }
        | InstKind::Load { .. }
        | InstKind::Select { .. } => is_fp(cx, inst.ty()),
        InstKind::Cast { arg } => is_fp(cx, inst.ty()) || is_fp(cx, value_ty(cx, arg)),
        InstKind::FCmp { .. } => true,
        InstKind::Store { value, .. } => is_fp(cx, value_ty(cx, value)),
        InstKind::Call(site) => {
            site.intrinsic().is_some_and(fp_intrinsic)
                && (is_fp(cx, inst.ty()) || site.args().iter().any(|&a| is_fp(cx, value_ty(cx, a))))
        }
        _ => false,
    }
}

// ---- 80-bit copies ----

/// The 80-bit copies of the current run (the module comment): hod's
/// per-function state of this module (`FnState::x87`). Instructions are
/// lowered in layout order, so a run is entered once and a copy of an
/// earlier run is dead.
#[derive(Clone, Default, Debug)]
pub struct Copies {
    /// The barrier that ends the current run (an instruction index).
    end: u32,
    /// The results of the current run that have a copy, in the order of
    /// their definitions, with its 12-byte slot.
    live: Vec<(u32, FrameRef)>,
    /// The slots of earlier runs, free again.
    free: Vec<FrameRef>,
    /// The call being lowered whose `float`/`double` result arrives in
    /// `st0` at 80 bits ([`call_result`]).
    call: Option<(u32, FrameRef)>,
    /// The 80-bit copy that the `ret` being lowered returns ([`ret_copy`]).
    ret: Option<FrameRef>,
}

/// Brackets the call sequence of `inst` ([`FnCx::call`]): a `float` or
/// `double` result arrives in `st0` as the callee computed it, at 80 bits
/// (`spec/abi-x86.md` §5.1), and `llc` keeps it there for the result's
/// run (measured, 19 and 22: `calll third; fstpl` of its `fpext`). So a
/// call result is a computed result: [`store_call_result`] keeps its
/// copy, in the slot taken here (the call is a barrier: no copy of the
/// last run is used by it). `on` false ends the bracket.
///
/// # Errors
/// What allocating the copy's slot reports.
pub fn call_result(cx: &mut FnCx<'_, X86>, inst: Inst<'_>, on: bool) -> CgResult<()> {
    cx.state.x87.call = None;
    // A result of an SSE type: `sse` keeps its 80-bit value (I12).
    if crate::sse::call_result(cx, inst, on)? {
        return Ok(());
    }
    let wide = matches!(cx.types().kind(inst.ty()), TypeKind::Float | TypeKind::Double);
    if on && wide && !inst.uses().is_unused() {
        cx.state.x87.call = copy_slot(cx, inst)?.map(|slot| (inst.id().raw(), slot));
    }
    Ok(())
}

/// The glue store of `st0` to `dst` (`Target::store`): first the 80-bit
/// copy of the call result being stored, if [`call_result`] took a slot.
pub fn store_call_result(cx: &mut FnCx<'_, X86>, dst: FrameRef, bytes: u32) {
    crate::sse::store_call_copy(cx);
    if let Some((raw, slot)) = cx.state.x87.call.take() {
        let inst = cx.body.inst(InstId::new(raw));
        store_copy(cx, inst, slot);
    }
    crate::abi::store_st0(cx, dst, bytes);
}

/// Brackets the return of `value` by `ret` `inst`: `llc` returns a
/// computed result of the block at 80 bits, the register it is in
/// (measured: `fdivrp; retl`), so the glue load ([`load_ret`]) takes
/// its copy. `on` false ends the bracket.
pub fn ret_copy(cx: &mut FnCx<'_, X86>, inst: Inst<'_>, value: Value, on: bool) {
    // A view of an SSE call result returns its 80-bit value (I12).
    cx.state.x87.ret = if on {
        crate::sse::ret_view(cx, value, inst).or_else(|| copy_for(cx, value, inst.id()))
    } else {
        None
    };
}

/// The glue load of `src` into `st0` (`Target::load`): the copy that
/// [`ret_copy`] marked instead, if any.
pub fn load_ret(cx: &mut FnCx<'_, X86>, src: FrameRef, bytes: u32) {
    match cx.state.x87.ret.take() {
        Some(copy) => fld(cx, Fp::F80, copy),
        None => crate::abi::load_st0(cx, src, bytes),
    }
}

fn inst_at<'a>(cx: &FnCx<'a, X86>, raw: u32) -> Option<Inst<'a>> {
    (raw < cx.body.inst_count() as u32).then(|| cx.body.inst(InstId::new(raw)))
}

/// Whether `llc` leaves no value in an x87 register across `inst`
/// (measured, 19 and 22): a terminator; a call (but `memcpy`/`memmove`
/// and `memset` that it writes inline, `crate::bytes::calls_libc`, and
/// inline assembly but as
/// [`asm_barrier`] says); `frem` and the libm
/// intrinsics (calls); an integer division wider than 32 bits
/// (`__divdi3` and kin); and, without `cmov`, a `select` on a variable
/// condition that is no `i1` and no choice of two integer constants
/// (`llc` branches for it, its fast allocator spills at the block end),
/// and the integer intrinsics [`int_branch`] lists.
fn barrier(cx: &FnCx<'_, X86>, inst: Inst<'_>) -> bool {
    if inst.is_terminator() {
        return true;
    }
    if !cx.fold_action(inst).emits() {
        return false;
    }
    if tls_barrier(cx, inst) {
        return true;
    }
    match inst.kind() {
        InstKind::Call(site) => match (site.intrinsic(), site.args()) {
            (None, _) => match site.callee_value().kind() {
                ValueKind::Asm(id) => asm_barrier(cx.module.asm(id).constraints),
                _ => true,
            },
            (Some(which), _) if crate::coro::is_barrier(which) => true,
            // `modf` is a libm call, never folded by `llc` 22 (WP-337).
            (Some(Intrinsic::Modf), _) => true,
            (Some(Intrinsic::Memcpy | Intrinsic::Memmove | Intrinsic::Memset), _) => {
                crate::bytes::site_calls_libc(cx, &site)
            }
            (Some(which), args) => {
                (routine_op(which).is_some() && !folded(cx, inst, FOLD_DEPTH))
                    || int_branch(cx, inst, which, args)
            }
        },
        InstKind::Binary { .. } => match inst.opcode() {
            Opcode::FRem => !folded(cx, inst, FOLD_DEPTH),
            Opcode::SDiv | Opcode::UDiv | Opcode::SRem | Opcode::URem => {
                cx.types().int_bits(inst.ty()).is_some_and(|b| b > 32)
            }
            _ => false,
        },
        // `fptoui` to `i64` chooses its subtrahend with a branch.
        InstKind::Cast { .. } if inst.opcode() == Opcode::FPToUI => {
            cx.types().int_bits(inst.ty()) == Some(64)
                && !features(cx).has(X86Features::CMOV)
                && !folded(cx, inst, FOLD_DEPTH)
        }
        // An aggregate `select` branches without `cmov` like a scalar one,
        // `{i1}` included, except between two constants or of a type
        // without bytes (measured with `llc` 19 and 22, WP-315).
        InstKind::Select { if_true, if_false, .. }
            if crate::agg::is_aggregate(cx, inst.ty())
                && (cx.types().size_of(inst.ty()) == 0
                    || [if_true, if_false]
                        .iter()
                        .all(|v| matches!(v.kind(), ValueKind::Const(_)))) =>
        {
            false
        }
        InstKind::Select { cond, if_true, if_false } => {
            let bits = cx.types().int_bits(inst.ty());
            let constants = bits.is_some()
                && const_int(cx, if_true).is_some()
                && const_int(cx, if_false).is_some();
            const_int(cx, cond).is_none()
                && bits != Some(1)
                && !constants
                && !features(cx).has(X86Features::CMOV)
        }
        InstKind::AtomicRmw { op, .. } => crate::atomic::barrier(cx, inst, op),
        _ => false,
    }
}

/// Whether `llc` lowers the integer intrinsic `which` (call `inst` with
/// `args`) with a branch: only without `cmov` (its select expansion
/// splits the block; measured with `llc` 19 and 22 at widths 1 to 64,
/// `spec/codegen-x86.md` §11 "Integer intrinsics"). At a result of `N`
/// bits: `ctlz` with zero defined at `N` >= 2, with zero poison at `N` >
/// 32; `cttz` with zero defined at `N` >= 32, with zero poison at `N` >
/// 32; `smax`, `smin`, `umax`, `umin`, the saturating additions,
/// subtractions and shifts at `N` >= 2 (LLVM 19 also branches for the
/// saturating additions and subtractions at `N` = 1; hod follows 22);
/// `fshl`/`fshr` by a variable amount at `N` > 32. Never `bswap`,
/// `ctpop`, `bitreverse`, `abs`, `scmp`, `ucmp`, a funnel shift by a
/// constant. `llc`'s DAG folds, and does not branch for: operands that
/// are all constants or folded results; min/max of one value twice, or
/// with the type's least or greatest value; an addition of 0; a
/// subtraction of one value from itself or of 0, `usub.sat` from all
/// ones; a saturating shift of 0 or by 0; a funnel shift of one constant
/// twice.
fn int_branch(cx: &FnCx<'_, X86>, inst: Inst<'_>, which: Intrinsic<'_>, args: &[Value]) -> bool {
    use Intrinsic as I;
    if features(cx).has(X86Features::CMOV) {
        return false;
    }
    let Some(bits) = cx.types().int_bits(inst.ty()) else { return false };
    let all_ones = if bits >= 64 { u64::MAX } else { (1u64 << bits) - 1 };
    let sign = 1u64 << (bits.clamp(1, 64) - 1);
    let constant = |v: Value| match v.kind() {
        ValueKind::Const(id) => match cx.module.consts.get(id) {
            Const::Int { value, .. } => Some(value & all_ones),
            Const::Null(_) | Const::Zero(_) | Const::Undef(_) | Const::Poison(_) => Some(0),
            _ => None,
        },
        _ => None,
    };
    let is = |v: Value, k: u64| constant(v) == Some(k);
    let (data, rest): (&[Value], &[Value]) = match which {
        I::Ctlz | I::Cttz => (args.get(..1).unwrap_or(args), args.get(1..).unwrap_or(&[])),
        _ => (args, &[]),
    };
    if data.is_empty() || operands_folded(cx, inst, data, FOLD_DEPTH) {
        return false;
    }
    let poison = rest.first().is_some_and(|&f| constant(f).is_some_and(|k| k != 0));
    match (which, args) {
        (I::Ctlz, _) => {
            if poison {
                bits > 32
            } else {
                bits >= 2
            }
        }
        (I::Cttz, _) => {
            if poison {
                bits > 32
            } else {
                bits >= 32
            }
        }
        (I::Umax | I::Umin, &[a, b]) => {
            bits >= 2 && a != b && ![a, b].iter().any(|&v| is(v, 0) || is(v, all_ones))
        }
        (I::Smax | I::Smin, &[a, b]) => {
            bits >= 2 && a != b && ![a, b].iter().any(|&v| is(v, sign) || is(v, sign - 1))
        }
        (I::SaddSat | I::UaddSat | I::SshlSat | I::UshlSat, &[a, b]) => {
            bits >= 2 && !is(a, 0) && !is(b, 0)
        }
        (I::SsubSat | I::UsubSat, &[a, b]) => {
            bits >= 2 && a != b && !is(b, 0) && !(which == I::UsubSat && is(a, all_ones))
        }
        (I::Fshl | I::Fshr, &[a, b, amount]) => {
            bits > 32
                && !operands_folded(cx, inst, &[amount], FOLD_DEPTH)
                && !(a == b && constant(a).is_some())
        }
        _ => false,
    }
}

/// What one `___tls_get_addr` call of `llc` computes.
#[derive(Clone, Copy, PartialEq, Eq)]
enum TlsKey {
    /// A general-dynamic thread-local's address.
    Symbol(hod_ir::SymId),
    /// The module's block (local dynamic, for all its symbols).
    Module,
}

/// The key of `llc`'s `___tls_get_addr` call for operand `v`, if it is a
/// thread-local of a dynamic model.
fn tls_key(cx: &FnCx<'_, X86>, v: Value) -> Option<TlsKey> {
    let (sym, _, model) = crate::tls::dynamic_symbol(cx, v)?;
    Some(if model == hod_cg::TlsAccess::GeneralDynamic {
        TlsKey::Symbol(sym)
    } else {
        TlsKey::Module
    })
}

fn operands_of(inst: Inst<'_>) -> &[Value] {
    match inst.kind() {
        InstKind::Call(site) => site.args(),
        _ => inst.operands(),
    }
}

/// Whether `inst` is where `llc` calls `___tls_get_addr` (measured, 19 and
/// 22: the live x87 values are spilled at their width around it, as for
/// any call): it names a thread-local of a dynamic model (the TLS
/// sequences of `tls`) whose address no earlier instruction of its block
/// took (the DAG computes it once per block, local dynamic's module block
/// once for all its symbols). A GOT access under PIC (`call; pop`) is
/// none: `llc` keeps the x87 registers across it.
fn tls_barrier(cx: &FnCx<'_, X86>, inst: Inst<'_>) -> bool {
    let keys: Vec<_> = operands_of(inst).iter().filter_map(|&v| tls_key(cx, v)).collect();
    if keys.is_empty() {
        return false;
    }
    let mut raw = inst.id().raw();
    let mut fresh = keys;
    while raw > 0 && !fresh.is_empty() {
        raw -= 1;
        let Some(q) = inst_at(cx, raw) else { break };
        if q.is_terminator() {
            break;
        }
        let seen: Vec<_> = operands_of(q).iter().filter_map(|&v| tls_key(cx, v)).collect();
        fresh.retain(|k| !seen.contains(k));
    }
    !fresh.is_empty()
}

/// Whether inline assembly with `constraints` ends a run (measured, `llc`
/// 19 and 22): it is no call, so `llc` keeps its x87 registers across it,
/// unless an operand is an x87 register (`{st}`, `{st(1)}`, ...) or it
/// clobbers `st(0)` (`~{st}`): then the live values are spilled at their
/// type's width. A clobber of a deeper register alone (`~{st(7)}`) spills
/// nothing (`llc` refuses the template when a live value is there:
/// "clobbers must be last on the x87 stack"). `t`, `u`, `f` operands are
/// refused by `asm` (`x86:inline-asm:x87`).
fn asm_barrier(constraints: &[u8]) -> bool {
    constraints.split(|&b| b == b',').any(|item| match item.strip_prefix(b"~") {
        Some(clobber) => matches!(clobber, b"{st}" | b"{st(0)}"),
        None => item.windows(3).any(|w| w == b"{st"),
    })
}

/// How deep [`folded`] looks through operands.
const FOLD_DEPTH: u32 = 8;

/// Whether `llc`'s DAG folds `inst` to a constant of its type (measured):
/// an operation of the folding set whose operands are constants or
/// folded results of its block, within [`FOLD_DEPTH`] levels. The set:
/// `fadd`..`frem`, `fneg`, the casts, integer arithmetic, `select`, and
/// the intrinsics `fabs`, `copysign`, `floor`, `ceil`, `trunc`,
/// `minnum`, `maxnum`, `fma`, `fmuladd` (not `sqrt`, nor the other libm
/// routines: `llc` calls or computes them at run time). A folded result
/// is no wider than its type, and a folded `frem` or intrinsic calls
/// nothing in `llc` (no barrier).
fn folded(cx: &FnCx<'_, X86>, inst: Inst<'_>, depth: u32) -> bool {
    let args = match inst.kind() {
        InstKind::Binary { .. } | InstKind::Unary { .. } | InstKind::Cast { .. } => {
            inst.opcode() != Opcode::Freeze
                && !matches!(
                    inst.opcode(),
                    Opcode::UDiv | Opcode::SDiv | Opcode::URem | Opcode::SRem
                )
        }
        InstKind::Select { .. } => true,
        InstKind::Call(site) => matches!(
            site.intrinsic(),
            Some(
                Intrinsic::Fabs
                    | Intrinsic::CopySign
                    | Intrinsic::Floor
                    | Intrinsic::Ceil
                    | Intrinsic::Trunc
                    | Intrinsic::MinNum
                    | Intrinsic::MaxNum
                    | Intrinsic::Fma
                    | Intrinsic::FmulAdd
            )
        ),
        _ => false,
    };
    let operands: &[Value] = match inst.kind() {
        InstKind::Call(site) => site.args(),
        _ => inst.operands(),
    };
    args && operands_folded(cx, inst, operands, depth)
}

/// Whether every one of `operands` of `inst` is a constant or a
/// [`folded`] result of its block, within `depth` levels.
fn operands_folded(cx: &FnCx<'_, X86>, inst: Inst<'_>, operands: &[Value], depth: u32) -> bool {
    depth > 0
        && operands.iter().all(|&v| match v.kind() {
            ValueKind::Const(_) => true,
            _ => v.as_inst().is_some_and(|id| {
                let def = cx.body.inst(id);
                id < inst.id()
                    && folded(cx, def, depth - 1)
                    && (id.raw() + 1..inst.id().raw())
                        .all(|raw| inst_at(cx, raw).is_some_and(|q| !q.is_terminator()))
            }),
        })
}

/// The barrier after instruction `at`; past it when it is a `ret`, which
/// returns a result of the run at 80 bits ([`ret_copy`]).
fn run_end(cx: &FnCx<'_, X86>, at: u32) -> u32 {
    let mut raw = at + 1;
    while let Some(q) = inst_at(cx, raw) {
        if barrier(cx, q) {
            return if matches!(q.kind(), InstKind::Ret { .. }) { raw + 1 } else { raw };
        }
        raw += 1;
    }
    raw
}

/// The 80-bit copy of operand `v` for its use by `user`, if `v` has one
/// and `user` is in its run.
fn copy_for(cx: &FnCx<'_, X86>, v: Value, user: InstId) -> Option<FrameRef> {
    let id = v.as_inst()?.raw();
    let copies = &cx.state.x87;
    if user.raw() >= copies.end || user.raw() <= id {
        return None;
    }
    let k = copies.live.binary_search_by_key(&id, |e| e.0).ok()?;
    Some(copies.live[k].1)
}

/// Stores the 80-bit copy of `st0` for computed result `inst` when an
/// instruction of its run follows it.
fn keep_copy(cx: &mut FnCx<'_, X86>, inst: Inst<'_>) -> CgResult<()> {
    if let Some(slot) = copy_slot(cx, inst)? {
        store_copy(cx, inst, slot);
    }
    Ok(())
}

/// The slot of computed result `inst`'s 80-bit copy, if an instruction
/// of its run follows it (entering its run first).
fn copy_slot(cx: &mut FnCx<'_, X86>, inst: Inst<'_>) -> CgResult<Option<FrameRef>> {
    let at = inst.id().raw();
    if at >= cx.state.x87.end {
        // A new run: the copies of the last one are dead.
        let end = run_end(cx, at);
        let copies = &mut cx.state.x87;
        let dead = std::mem::take(&mut copies.live);
        copies.free.extend(dead.into_iter().map(|(_, slot)| slot));
        copies.end = end;
    }
    if cx.state.x87.end == at + 1 {
        return Ok(None);
    }
    Ok(Some(match cx.state.x87.free.pop() {
        Some(slot) => slot,
        None => cx.alloc_slot(12, 4)?,
    }))
}

/// Stores `st0` at 80 bits to `slot`, `inst`'s copy, keeping `st0`.
fn store_copy(cx: &mut FnCx<'_, X86>, inst: Inst<'_>, slot: FrameRef) {
    put(cx, mc::fld_st(St::St0));
    fstp(cx, Fp::F80, slot);
    cx.state.x87.live.push((inst.id().raw(), slot));
}

// ---- the stack and the frame ----

fn fld(cx: &mut FnCx<'_, X86>, fp: Fp, place: FrameRef) {
    frame_inst(cx, place, |m| mc::fld_m(fp.size(), m));
}

fn fstp(cx: &mut FnCx<'_, X86>, fp: Fp, place: FrameRef) {
    frame_inst(cx, place, |m| mc::fstp_m(fp.size(), m));
}

/// `[esp + disp]`.
fn sp(disp: i32) -> Mem {
    Mem::base(ESP, disp)
}

/// Takes `n` bytes below the stack pointer.
fn below(cx: &mut FnCx<'_, X86>, n: u32) {
    put(cx, mc::lea(Size::D, ESP, sp(-(n as i32))));
}

/// Gives back `n` bytes.
fn above(cx: &mut FnCx<'_, X86>, n: u32) {
    put(cx, mc::lea(Size::D, ESP, sp(n as i32)));
}

/// Pushes operand `v` (of type `fp`) of instruction `user`: its 80-bit
/// copy if `user` is in its run, else its home. Whether it was the copy.
fn push(cx: &mut FnCx<'_, X86>, v: Value, fp: Fp, user: Inst<'_>) -> CgResult<bool> {
    // An `fpext` of an SSE call result is its 80-bit value (§16).
    if let Some(slot) = crate::sse::ret_view(cx, v, user).or_else(|| copy_for(cx, v, user.id())) {
        fld(cx, Fp::F80, slot);
        return Ok(true);
    }
    let place = cx.use_mem(v)?;
    fld(cx, fp, place);
    Ok(false)
}

/// The result in `st0` popped to its home, rounded to its type; first,
/// if it may be wider than its type (`wide`), also kept at 80 bits for
/// its run ([`keep_copy`]).
fn result(cx: &mut FnCx<'_, X86>, inst: Inst<'_>, fp: Fp, wide: bool) -> CgResult<()> {
    if wide && fp != Fp::F80 && !inst.uses().is_unused() && !folded(cx, inst, FOLD_DEPTH) {
        keep_copy(cx, inst)?;
    }
    let inst = cx.body.inst(inst.id());
    let r = cx.def_mem(inst)?;
    fstp(cx, fp, r);
    Ok(())
}

// ---- constants (`Target::materialize_mem`) ----

/// Writes the constant `value` of a floating type to `dst` by `mov`s of
/// its encoding (no constant pool, so no relocation); any other type is
/// refused by its family.
///
/// # Errors
/// The refusal of a type that is no x87 type.
pub fn materialize_mem(cx: &mut FnCx<'_, X86>, dst: FrameRef, value: Value) -> CgResult<()> {
    let ty = value_ty(cx, value);
    if !is_fp(cx, ty) {
        return Err(crate::target::refusal_of(cx, ty));
    }
    let fp = fp_of(cx, ty)?;
    let ValueKind::Const(id) = value.kind() else {
        return cx.bug("materialize_mem of something that is no constant");
    };
    let bits: u128 = match cx.module.consts.get(id) {
        Const::Float { bits, .. } => u128::from(bits),
        Const::WideFloat { bits, .. } => bits,
        Const::Zero(_) | Const::Undef(_) | Const::Poison(_) => 0,
        _ => return unsupported(crate::common::CONSTANT),
    };
    write_bits(cx, dst, fp, bits);
    Ok(())
}

/// The encoding `bits` of an `fp` written to `dst` by `mov` immediates.
fn write_bits(cx: &mut FnCx<'_, X86>, dst: FrameRef, fp: Fp, bits: u128) {
    let bytes = fp.bytes();
    let mut at = 0;
    while at < bytes {
        let size = if bytes - at >= 4 { Size::D } else { Size::W };
        let piece = (bits >> (8 * at)) as u32;
        let imm =
            if size == Size::D { i64::from(piece as i32) } else { i64::from(piece as u16 as i16) };
        frame_inst(cx, dst.offset_by(at), |m| mc::mov_mi(size, m, imm));
        at += if size == Size::D { 4 } else { 2 };
    }
}

// ---- constants `llc`'s DAG folds ----

/// The encoding of `v` (of type `fp`) when `llc`'s DAG folds it to a
/// constant that hod computes the same way at compile time: a constant,
/// or a result of `user`'s block (no terminator between) of `fadd`,
/// `fsub`, `fmul`, `fdiv` ([`fold`]), `fneg`, `fpext`, `fptrunc` of such
/// operands, [`FOLD_DEPTH`] levels deep; never a NaN (left to the run
/// time). `x86_fp80` is never folded here (the x87 computes it in its
/// precision, one rounding, as `llc`'s `APFloat`).
fn const_bits(cx: &FnCx<'_, X86>, v: Value, fp: Fp, user: Inst<'_>, depth: u32) -> Option<u64> {
    let nan = |bits: u64| match fp {
        Fp::F32 => f32::from_bits(bits as u32).is_nan(),
        _ => f64::from_bits(bits).is_nan(),
    };
    let bits = if let ValueKind::Const(id) = v.kind() {
        match cx.module.consts.get(id) {
            Const::Float { bits, .. } => bits,
            Const::Zero(_) => 0,
            _ => return None,
        }
    } else {
        {
            let def = cx.body.inst(v.as_inst()?);
            let local = def.id() < user.id()
                && (def.id().raw() + 1..user.id().raw())
                    .all(|raw| inst_at(cx, raw).is_some_and(|q| !q.is_terminator()));
            if depth == 0 || !local || fp_of(cx, def.ty()).ok() != Some(fp) {
                return None;
            }
            match def.kind() {
                InstKind::Binary { lhs, rhs } => {
                    let a = const_bits(cx, lhs, fp, def, depth - 1)?;
                    let b = const_bits(cx, rhs, fp, def, depth - 1)?;
                    fold(def.opcode(), fp, a, b)?
                }
                InstKind::Unary { arg } if def.opcode() == Opcode::FNeg => {
                    let a = const_bits(cx, arg, fp, def, depth - 1)?;
                    a ^ if fp == Fp::F32 { 0x8000_0000 } else { 1 << 63 }
                }
                InstKind::Cast { arg } => {
                    let from = fp_of(cx, value_ty(cx, arg)).ok()?;
                    let a = const_bits(cx, arg, from, def, depth - 1)?;
                    match (def.opcode(), from, fp) {
                        (Opcode::FPExt, Fp::F32, Fp::F64) => {
                            f64::from(f32::from_bits(a as u32)).to_bits()
                        }
                        (Opcode::FPTrunc, Fp::F64, Fp::F32) => {
                            u64::from((f64::from_bits(a) as f32).to_bits())
                        }
                        _ => return None,
                    }
                }
                _ => return None,
            }
        }
    };
    (!nan(bits)).then_some(bits)
}

/// `op` of the encodings `a` and `b` of an `fp`, rounded once to `fp` as
/// `llc`'s `APFloat` folds it (IEEE round to nearest, which Rust's `f32`
/// and `f64` arithmetic is); `None` for a NaN result or another `op`.
fn fold(op: Opcode, fp: Fp, a: u64, b: u64) -> Option<u64> {
    macro_rules! apply {
        ($x:expr, $y:expr) => {
            match op {
                Opcode::FAdd => $x + $y,
                Opcode::FSub => $x - $y,
                Opcode::FMul => $x * $y,
                Opcode::FDiv => $x / $y,
                _ => return None,
            }
        };
    }
    let bits = match fp {
        Fp::F32 => {
            let r = apply!(f32::from_bits(a as u32), f32::from_bits(b as u32));
            if r.is_nan() {
                return None;
            }
            u64::from(r.to_bits())
        }
        Fp::F64 => {
            let r = apply!(f64::from_bits(a), f64::from_bits(b));
            if r.is_nan() {
                return None;
            }
            r.to_bits()
        }
        Fp::F80 => return None,
    };
    Some(bits)
}

// ---- memory ----

/// Where a pointer operand points.
#[derive(Clone, Copy)]
enum At {
    Frame(FrameRef),
    Sym(hod_ir::SymId),
    Reg(Reg),
}

fn address(cx: &mut FnCx<'_, X86>, ptr: Value) -> CgResult<At> {
    match ptr.kind() {
        ValueKind::Slot(slot) => match cx.slot_place(slot)? {
            hod_cg::SlotPlace::Direct(place) => Ok(At::Frame(place)),
            hod_cg::SlotPlace::Indirect(pointer) => {
                frame_inst(cx, pointer, |m| mc::mov_rm(Size::D, EDX, m));
                Ok(At::Reg(EDX))
            }
        },
        ValueKind::Sym(sym)
            if !matches!(
                hod_cg::sym_access(cx.module, cx.options, sym),
                hod_cg::Access::Tls(_)
            ) =>
        {
            Ok(At::Sym(sym))
        }
        _ => Ok(At::Reg(gp(cx.use_reg(ptr)?))),
    }
}

fn access(
    cx: &mut FnCx<'_, X86>,
    at: At,
    k: u32,
    make: impl FnOnce(Mem) -> mc::Inst,
) -> CgResult<()> {
    match at {
        At::Frame(place) => {
            frame_inst(cx, place.offset_by(k), make);
            Ok(())
        }
        At::Sym(sym) => sym_inst(cx, sym, i64::from(k), make),
        At::Reg(base) => {
            put(cx, make(Mem::base(base, k as i32)));
            Ok(())
        }
    }
}

/// The pieces of an encoding of `bytes`: (offset, size).
fn pieces(bytes: u32) -> impl Iterator<Item = (u32, Size)> {
    (0..bytes.div_ceil(4)).map(move |i| (4 * i, if bytes - 4 * i >= 4 { Size::D } else { Size::W }))
}

fn check(mem: MemFlags) -> CgResult<()> {
    if mem.ordering().is_some() { unsupported(crate::mem::ATOMIC) } else { Ok(()) }
}

fn load<'a>(cx: &mut FnCx<'a, X86>, inst: Inst<'a>, ptr: Value, mem: MemFlags) -> CgResult<()> {
    check(mem)?;
    let fp = fp_of(cx, inst.ty())?;
    let at = address(cx, ptr)?;
    let r = cx.def_mem(inst)?;
    for (k, size) in pieces(fp.bytes()) {
        access(cx, at, k, |m| mc::mov_rm(size, ECX, m))?;
        frame_inst(cx, r.offset_by(k), |m| mc::mov_mr(size, m, ECX));
    }
    Ok(())
}

fn store(cx: &mut FnCx<'_, X86>, value: Value, ptr: Value, mem: MemFlags) -> CgResult<()> {
    check(mem)?;
    // The home holds what `fstp` of an 80-bit copy would store.
    let fp = fp_of(cx, value_ty(cx, value))?;
    let src = cx.use_mem(value)?;
    let at = address(cx, ptr)?;
    for (k, size) in pieces(fp.bytes()) {
        frame_inst(cx, src.offset_by(k), |m| mc::mov_rm(size, ECX, m));
        access(cx, at, k, |m| mc::mov_mr(size, m, ECX))?;
    }
    Ok(())
}

/// `inst`'s result: a copy of `v`.
fn copy_of<'a>(cx: &mut FnCx<'a, X86>, inst: Inst<'a>, v: Value, fp: Fp) -> CgResult<()> {
    let src = cx.use_mem(v)?;
    let r = cx.def_mem(inst)?;
    if r != src {
        copy_frame(cx, r, src, fp.bytes());
    }
    Ok(())
}

// ---- arithmetic ----

fn binary<'a>(cx: &mut FnCx<'a, X86>, inst: Inst<'a>, lhs: Value, rhs: Value) -> CgResult<()> {
    let fp = fp_of(cx, inst.ty())?;
    let op = match inst.opcode() {
        Opcode::FAdd => mc::faddp(St::St1),
        Opcode::FSub => mc::fsubp(St::St1),
        Opcode::FMul => mc::fmulp(St::St1),
        Opcode::FDiv => mc::fdivp(St::St1),
        Opcode::FRem => {
            let func = routine(FpOp::Rem, fp)?;
            return libcall(cx, inst, func, &[lhs, rhs], fp);
        }
        _ => return cx.bug("an integer opcode on a floating type"),
    };
    // `llc`'s DAG folds it: one rounding, where the x87 would round twice
    // (80 bits, then the type) for a `double`.
    let folded = |cx: &FnCx<'_, X86>| {
        let a = const_bits(cx, lhs, fp, inst, FOLD_DEPTH)?;
        let b = const_bits(cx, rhs, fp, inst, FOLD_DEPTH)?;
        fold(inst.opcode(), fp, a, b)
    };
    if let Some(bits) = folded(cx) {
        let r = cx.def_mem(inst)?;
        write_bits(cx, r, fp, u128::from(bits));
        return Ok(());
    }
    push(cx, lhs, fp, inst)?;
    push(cx, rhs, fp, inst)?;
    put(cx, op);
    result(cx, inst, fp, true)
}

/// `fneg` (the sign flipped: `fchs` on the stack, `xor` of the sign byte
/// in memory) and `freeze` (a copy).
fn unary<'a>(cx: &mut FnCx<'a, X86>, inst: Inst<'a>, arg: Value) -> CgResult<()> {
    let fp = fp_of(cx, inst.ty())?;
    if inst.opcode() != Opcode::FNeg {
        // `freeze` passes an 80-bit copy on, as `llc` does.
        if copy_for(cx, arg, inst.id()).is_some() {
            push(cx, arg, fp, inst)?;
            return result(cx, inst, fp, true);
        }
        return copy_of(cx, inst, arg, fp);
    }
    sign_op(cx, inst, arg, fp, mc::fchs(), |m| mc::xor_mi(Size::B, m, -0x80))
}

/// A sign operation: `on` on the stack when the operand has an 80-bit
/// copy for it, else the copy in memory and `in_mem` on its sign byte.
fn sign_op<'a>(
    cx: &mut FnCx<'a, X86>,
    inst: Inst<'a>,
    arg: Value,
    fp: Fp,
    on: mc::Inst,
    in_mem: fn(Mem) -> mc::Inst,
) -> CgResult<()> {
    if copy_for(cx, arg, inst.id()).is_some() {
        push(cx, arg, fp, inst)?;
        put(cx, on);
        return result(cx, inst, fp, true);
    }
    copy_of(cx, inst, arg, fp)?;
    let r = cx.def_mem(inst)?;
    frame_inst(cx, r.offset_by(fp.bytes() - 1), in_mem);
    Ok(())
}

// ---- compares ----

/// How a predicate reads the flags of `st0 = x` compared with `st1 = y`
/// (`fucomi`, or `fucompp`, `fnstsw`, `sahf`): unordered sets `ZF`, `PF`
/// and `CF`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FpCond {
    One(Cond),
    All(Cond, Cond),
    Any(Cond, Cond),
}

/// Whether the operands are swapped (`x` = the right one) and the
/// condition.
const fn conditions(pred: FloatPredicate) -> (bool, FpCond) {
    use FloatPredicate as P;
    match pred {
        P::Oeq => (false, FpCond::All(Cond::E, Cond::Np)),
        P::Ogt => (false, FpCond::One(Cond::A)),
        P::Oge => (false, FpCond::One(Cond::Ae)),
        P::Olt => (true, FpCond::One(Cond::A)),
        P::Ole => (true, FpCond::One(Cond::Ae)),
        // `true` is `cmp eax, eax` (ZF set), `false` reads `ZF` clear.
        P::One | P::False => (false, FpCond::One(Cond::Ne)),
        P::Ord => (false, FpCond::One(Cond::Np)),
        P::Ueq | P::True => (false, FpCond::One(Cond::E)),
        P::Ugt => (true, FpCond::One(Cond::B)),
        P::Uge => (true, FpCond::One(Cond::Be)),
        P::Ult => (false, FpCond::One(Cond::B)),
        P::Ule => (false, FpCond::One(Cond::Be)),
        P::Une => (false, FpCond::Any(Cond::Ne, Cond::P)),
        P::Uno => (false, FpCond::One(Cond::P)),
    }
}

fn fcmp<'a>(
    cx: &mut FnCx<'a, X86>,
    inst: Inst<'a>,
    pred: FloatPredicate,
    lhs: Value,
    rhs: Value,
) -> CgResult<()> {
    let fp = fp_of(cx, value_ty(cx, lhs))?;
    let rd = cx.def_reg(inst)?;
    let (swap, cond) = conditions(pred);
    if matches!(pred, FloatPredicate::True | FloatPredicate::False) {
        put(cx, mc::cmp_rr(Size::D, EAX, EAX));
    } else {
        let (x, y) = if swap { (rhs, lhs) } else { (lhs, rhs) };
        if x == y {
            push(cx, x, fp, inst)?;
            put(cx, mc::fld_st(St::St0));
        } else {
            push(cx, y, fp, inst)?;
            push(cx, x, fp, inst)?;
        }
        if features(cx).has(X86Features::CMOV) {
            put(cx, mc::fucomip(St::St1));
            put(cx, mc::fstp_st(St::St0));
        } else {
            put(cx, mc::fucompp());
            put(cx, mc::fnstsw_ax());
            put(cx, mc::sahf());
        }
    }
    let r = gp(rd);
    match cond {
        FpCond::One(c) => put(cx, mc::setcc_r(c, EAX)),
        FpCond::All(c1, c2) | FpCond::Any(c1, c2) => {
            put(cx, mc::setcc_r(c1, EAX));
            put(cx, mc::setcc_r(c2, ECX));
            let all = matches!(cond, FpCond::All(..));
            put(cx, if all { mc::and_rr(Size::B, EAX, ECX) } else { mc::or_rr(Size::B, EAX, ECX) });
        }
    }
    put(cx, mc::movzx_rr(Size::D, r, Size::B, EAX));
    cx.finish(inst, rd)
}

/// Whether `select` `inst` is an `fcmov` on the stack (a scalar `i1`
/// condition with `cmov`); else a branch and a copy in memory.
fn fcmov(cx: &FnCx<'_, X86>, inst: Inst<'_>) -> bool {
    let InstKind::Select { cond, .. } = inst.kind() else { return false };
    features(cx).has(X86Features::CMOV)
        && cx.types().int_bits(value_ty(cx, cond)) == Some(1)
        && is_fp(cx, inst.ty())
}

/// `select` of floating values: of a constant condition the value it
/// picks (`llc` folds it: an 80-bit copy passes on); `fcmov` on the
/// stack with `cmov` ([`fcmov`]); else a branch on the condition and one
/// copy.
fn select<'a>(
    cx: &mut FnCx<'a, X86>,
    inst: Inst<'a>,
    cond: Value,
    if_true: Value,
    if_false: Value,
) -> CgResult<()> {
    let fp = fp_of(cx, inst.ty())?;
    if let Some(c) = const_int(cx, cond) {
        let picked = if c & 1 != 0 { if_true } else { if_false };
        if copy_for(cx, picked, inst.id()).is_none() {
            return copy_of(cx, inst, picked, fp);
        }
        push(cx, picked, fp, inst)?;
        return result(cx, inst, fp, true);
    }
    if fcmov(cx, inst) {
        // `st1` = the false value, `st0` = the true one; `fcmove` takes
        // the false one when the condition's bit is clear; the result is
        // computed on the stack (kept at 80 bits, as `llc` has it).
        cx.put(cond, crate::reg::EAX)?;
        push(cx, if_false, fp, inst)?;
        push(cx, if_true, fp, inst)?;
        put(cx, mc::test_ri(Size::B, EAX, 1));
        put(cx, mc::fcmov(Cond::E, St::St1));
        put(cx, mc::fstp_st(St::St1));
        return result(cx, inst, fp, true);
    }
    let t = cx.use_mem(if_true)?;
    let f = cx.use_mem(if_false)?;
    let r = cx.def_mem(inst)?;
    cx.put(cond, crate::reg::EAX)?;
    let other = cx.new_label();
    let done = cx.new_label();
    put(cx, mc::test_ri(Size::B, EAX, 1));
    crate::common::branch_cond(cx, Cond::E, other)?;
    copy_frame(cx, r, t, fp.bytes());
    crate::common::branch(cx, done)?;
    cx.bind(other)?;
    copy_frame(cx, r, f, fp.bytes());
    cx.bind(done)
}

// ---- casts ----

fn cast<'a>(cx: &mut FnCx<'a, X86>, inst: Inst<'a>, arg: Value) -> CgResult<()> {
    let from = value_ty(cx, arg);
    match inst.opcode() {
        Opcode::FPExt | Opcode::FPTrunc if is_fp(cx, from) && is_fp(cx, inst.ty()) => {
            let (a, b) = (fp_of(cx, from)?, fp_of(cx, inst.ty())?);
            if crate::sse::x87_cast_source(cx, inst, arg)? {
                // A view of an SSE call result (§16).
                return result(cx, inst, b, false);
            }
            if let Some(x) = undone(cx, inst, arg) {
                // `fptrunc (fpext x)` back to `x`'s type in one block is
                // `x` (`llc`'s DAG folds it).
                let wide = push(cx, x, b, inst)?;
                return result(cx, inst, b, wide);
            }
            // `fpext` passes an 80-bit copy on; `fptrunc` rounds.
            let wide = push(cx, arg, a, inst)? && inst.opcode() == Opcode::FPExt;
            result(cx, inst, b, wide)
        }
        Opcode::SIToFP | Opcode::UIToFP => from_int(cx, inst, arg),
        Opcode::FPToSI | Opcode::FPToUI => to_int(cx, inst, arg),
        Opcode::BitCast => bitcast(cx, inst, arg, from),
        _ => unsupported(CAST),
    }
}

/// `bitcast` between `float` and `i32` or `double` and `i64` (§11): the
/// same bytes through the homes. A floating home holds the value rounded
/// to its type, never the 80-bit copy: a bitcast takes the type's bits
/// (`llc` at `generic`: `fstps (%esp); movl (%esp), %eax` of a product,
/// measured, 19 and 22). The result of a cast to a floating type has no
/// 80-bit copy (its users load its home), and no bits pass through the
/// stack, so a signalling NaN pattern is kept. Not a barrier (no branch,
/// no call in `llc`). `x86_fp80` from or to `i80`: the 10 bytes
/// (WP-337); a vector on either side: its family's refusal.
fn bitcast<'a>(cx: &mut FnCx<'a, X86>, inst: Inst<'a>, arg: Value, from: TypeId) -> CgResult<()> {
    let to = inst.ty();
    let (fp_ty, other) = if is_fp(cx, from) { (from, to) } else { (to, from) };
    if matches!(cx.types().kind(other), TypeKind::Vector { .. }) {
        return Err(crate::target::refusal_of(cx, other));
    }
    let fp = fp_of(cx, fp_ty)?;
    match (fp, cx.types().int_bits(other)) {
        (Fp::F32, Some(32)) if is_fp(cx, from) => {
            let src = cx.use_mem(arg)?;
            let rd = cx.def_reg(inst)?;
            frame_inst(cx, src, |m| mc::mov_rm(Size::D, gp(rd), m));
            cx.finish(inst, rd)
        }
        (Fp::F32, Some(32)) => {
            let r = cx.use_reg(arg)?;
            let dst = cx.def_mem(inst)?;
            frame_inst(cx, dst, |m| mc::mov_mr(Size::D, m, gp(r)));
            Ok(())
        }
        // The `i64` side is §8's slot of two words, low first: the
        // `double`'s memory image. WP-337: the `i80` side is a slot whose
        // first 10 bytes are the image (`crate::fpclass`'s compares read
        // them).
        (Fp::F64, Some(64)) | (Fp::F80, Some(80)) => copy_of(cx, inst, arg, fp),
        (Fp::F80, _) => unsupported(FP80_CAST),
        _ => unsupported(CAST),
    }
}

/// `x` when `fptrunc` `inst` undoes `arg` = `fpext x` of its block.
fn undone(cx: &FnCx<'_, X86>, inst: Inst<'_>, arg: Value) -> Option<Value> {
    if inst.opcode() != Opcode::FPTrunc {
        return None;
    }
    let ext = cx.body.inst(arg.as_inst()?);
    let InstKind::Cast { arg: x } = ext.kind() else { return None };
    let same_block = ext.id() < inst.id()
        && (ext.id().raw() + 1..inst.id().raw())
            .all(|raw| inst_at(cx, raw).is_some_and(|q| !q.is_terminator()));
    (ext.opcode() == Opcode::FPExt && value_ty(cx, x) == inst.ty() && same_block).then_some(x)
}

/// The width of integer type `ty`, at most 32, or [`I64_CAST`].
fn int_bits(cx: &FnCx<'_, X86>, ty: TypeId) -> CgResult<u32> {
    match cx.types().int_bits(ty) {
        Some(bits) if bits <= 32 => Ok(bits),
        Some(_) => unsupported(I64_CAST),
        None => unsupported(CAST),
    }
}

/// `sitofp`/`uitofp`: the integer extended in `ecx` to 32 bits, `fild`
/// of it (of 64 bits with a zero high half for an unsigned `i32`).
fn from_int<'a>(cx: &mut FnCx<'a, X86>, inst: Inst<'a>, arg: Value) -> CgResult<()> {
    let fp = fp_of(cx, inst.ty())?;
    let signed = inst.opcode() == Opcode::SIToFP;
    if cx.types().int_bits(value_ty(cx, arg)) == Some(64) {
        return from_i64(cx, inst, arg, fp, signed);
    }
    let bits = int_bits(cx, value_ty(cx, arg))?;
    let r = cx.use_reg(arg)?;
    put(cx, mc::mov_rr(Size::D, ECX, gp(r)));
    let ext = if signed { crate::alu::Ext::Sign } else { crate::alu::Ext::Zero };
    crate::alu::extend_to(cx, ECX, bits, ext);
    below(cx, 16);
    put(cx, mc::mov_mr(Size::D, sp(0), ECX));
    if !signed && bits == 32 {
        put(cx, mc::mov_mi(Size::D, sp(4), 0));
        put(cx, mc::fild_m(Size::Q, sp(0)));
    } else {
        put(cx, mc::fild_m(Size::D, sp(0)));
    }
    above(cx, 16);
    result(cx, inst, fp, true)
}

/// `sitofp`/`uitofp` of an `i64` (§8's slot, low word first): `fild
/// qword` of the slot, exact at 80 bits; unsigned, `2^64` added when the
/// high bit is set (as `llc`: `fadd` of a `float` chosen by the sign,
/// here `0x5F800000` or 0 built in `ecx` without a branch). A signed
/// result keeps its 80-bit copy for its run; an unsigned one is rounded
/// (as `llc`).
fn from_i64<'a>(
    cx: &mut FnCx<'a, X86>,
    inst: Inst<'a>,
    arg: Value,
    fp: Fp,
    signed: bool,
) -> CgResult<()> {
    let slot = cx.use_mem(arg)?;
    frame_inst(cx, slot, |m| mc::fild_m(Size::Q, m));
    if !signed {
        frame_inst(cx, slot.offset_by(4), |m| mc::mov_rm(Size::D, ECX, m));
        put(cx, mc::sar_ri(Size::D, ECX, 31));
        put(cx, mc::and_ri(Size::D, ECX, 0x5F80_0000));
        below(cx, 16);
        put(cx, mc::mov_mr(Size::D, sp(0), ECX));
        put(cx, mc::fld_m(X87Size::S, sp(0)));
        put(cx, mc::faddp(St::St1));
        above(cx, 16);
    }
    // Unsigned, `llc` rounds the sum to the type at once (`fstps`/`flds`,
    // `fstpl`/`fldl`; measured, 19 and 22): no 80-bit copy.
    result(cx, inst, fp, signed)
}

/// `fptosi` to an `i64`: as [`to_int`], `fistp qword` into the result's
/// slot. `fptoui` to an `i64` as `llc` (measured, 19 and 22, `-O0`):
/// `x` compared with `2^63` (`fucomip`, or `fucomp`, `fnstsw`, `sahf`);
/// the flag `2^63 <= x` or unordered (`setbe`, in `edx`); `x - 2^63`
/// for the flag, else `x - 0`, truncated by `fistp qword`; the flag
/// `xor`ed into bit 31 of the high word. `llc` chooses the subtrahend
/// with a branch without `cmov` ([`barrier`]), here a mask in `ecx`.
fn to_i64<'a>(cx: &mut FnCx<'a, X86>, inst: Inst<'a>, arg: Value, fp: Fp) -> CgResult<()> {
    let unsigned = inst.opcode() == Opcode::FPToUI;
    push(cx, arg, fp, inst)?;
    let inst = cx.body.inst(inst.id());
    let r = cx.def_mem(inst)?;
    below(cx, 16);
    if unsigned {
        put(cx, mc::mov_mi(Size::D, sp(12), TWO_63));
        put(cx, mc::fld_m(X87Size::S, sp(12)));
        if features(cx).has(X86Features::CMOV) {
            put(cx, mc::fucomip(St::St1));
        } else {
            put(cx, mc::fucomp(St::St1));
            put(cx, mc::fnstsw_ax());
            put(cx, mc::sahf());
        }
        put(cx, mc::setcc_r(Cond::Be, EDX));
        put(cx, mc::movzx_rr(Size::D, EDX, Size::B, EDX));
        put(cx, mc::mov_rr(Size::D, ECX, EDX));
        put(cx, mc::neg_r(Size::D, ECX));
        put(cx, mc::and_ri(Size::D, ECX, TWO_63));
        put(cx, mc::mov_mr(Size::D, sp(12), ECX));
        put(cx, mc::fsub_m(mc::FpSize::S, sp(12)));
        put(cx, mc::shl_ri(Size::D, EDX, 31));
    }
    put(cx, mc::fnstcw_m(sp(0)));
    put(cx, mc::movzx_rm(Size::D, ECX, Size::W, sp(0)));
    put(cx, mc::or_ri(Size::D, ECX, 0xc00));
    put(cx, mc::mov_mr(Size::W, sp(2), ECX));
    put(cx, mc::fldcw_m(sp(2)));
    put(cx, mc::fistp_m(Size::Q, sp(4)));
    put(cx, mc::fldcw_m(sp(0)));
    // The result's slot is addressed from `ebp`.
    for at in [0, 4] {
        put(cx, mc::mov_rm(Size::D, ECX, sp(4 + at)));
        if unsigned && at == 4 {
            put(cx, mc::xor_rr(Size::D, ECX, EDX));
        }
        frame_inst(cx, r.offset_by(at as u32), |m| mc::mov_mr(Size::D, m, ECX));
    }
    above(cx, 16);
    Ok(())
}

/// `2^63` as a `float` (`0x5F000000`), `llc`'s bound of `fptoui` to `i64`.
const TWO_63: i64 = 0x5F00_0000;

/// `fptosi`/`fptoui`: rounding toward zero around `fistp` (`fnstcw`,
/// `or 0xC00`, `fldcw`, `fistp`, `fldcw` of the saved word), of 64 bits
/// for an unsigned `i32`.
fn to_int<'a>(cx: &mut FnCx<'a, X86>, inst: Inst<'a>, arg: Value) -> CgResult<()> {
    let fp = fp_of(cx, value_ty(cx, arg))?;
    if cx.types().int_bits(inst.ty()) == Some(64) {
        return to_i64(cx, inst, arg, fp);
    }
    let bits = int_bits(cx, inst.ty())?;
    let wide = inst.opcode() == Opcode::FPToUI && bits == 32;
    let rd = cx.def_reg(inst)?;
    push(cx, arg, fp, inst)?;
    below(cx, 16);
    put(cx, mc::fnstcw_m(sp(0)));
    put(cx, mc::movzx_rm(Size::D, ECX, Size::W, sp(0)));
    put(cx, mc::or_ri(Size::D, ECX, 0xc00));
    put(cx, mc::mov_mr(Size::W, sp(2), ECX));
    put(cx, mc::fldcw_m(sp(2)));
    put(cx, mc::fistp_m(if wide { Size::Q } else { Size::D }, sp(4)));
    put(cx, mc::fldcw_m(sp(0)));
    put(cx, mc::mov_rm(Size::D, gp(rd), sp(4)));
    above(cx, 16);
    cx.finish(inst, rd)
}

// ---- intrinsics and libm ----

fn routine_op(which: Intrinsic<'_>) -> Option<FpOp> {
    use FpOp as O;
    use Intrinsic as I;
    Some(match which {
        I::Sin => O::Sin,
        I::Cos => O::Cos,
        I::Tan => O::Tan,
        I::Exp => O::Exp,
        I::Exp2 => O::Exp2,
        I::Log => O::Log,
        I::Log2 => O::Log2,
        I::Log10 => O::Log10,
        I::Pow => O::Pow,
        I::Floor => O::Floor,
        I::Ceil => O::Ceil,
        I::Trunc => O::Trunc,
        I::Round => O::Round,
        I::RoundEven => O::RoundEven,
        I::NearbyInt => O::NearbyInt,
        I::Rint => O::Rint,
        I::Fma => O::Fma,
        I::Asin => O::Asin,
        I::Acos => O::Acos,
        I::Atan => O::Atan,
        I::Atan2 => O::Atan2,
        I::Sinh => O::Sinh,
        I::Cosh => O::Cosh,
        I::Tanh => O::Tanh,
        I::Exp10 => O::Exp10,
        I::MinNum => O::MinNum,
        I::MaxNum => O::MaxNum,
        _ => return None,
    })
}

/// libm's routine of `op` for `fp`: `llc`'s symbol (`floorf`, `floor`,
/// `floorl`), the same names as x86-64's.
fn routine(op: FpOp, fp: Fp) -> CgResult<RuntimeFn> {
    // `llc` calls `fmin`/`fmax` for `minnum`/`maxnum` on the x87 (x86-64
    // has instructions).
    match (op, fp) {
        (FpOp::MinNum, Fp::F32) => return Ok(RuntimeFn::Fminf),
        (FpOp::MaxNum, Fp::F32) => return Ok(RuntimeFn::Fmaxf),
        (FpOp::MinNum, Fp::F64) => return Ok(RuntimeFn::Fmin),
        (FpOp::MaxNum, Fp::F64) => return Ok(RuntimeFn::Fmax),
        (FpOp::MinNum, Fp::F80) => return Ok(RuntimeFn::Fminl),
        (FpOp::MaxNum, Fp::F80) => return Ok(RuntimeFn::Fmaxl),
        _ => {}
    }
    match fp_routine(op, fp.fp_type(), Arch::X86_64) {
        Some(func) => Ok(func),
        None => unsupported(INTRINSIC),
    }
}

fn intrinsic<'a>(
    cx: &mut FnCx<'a, X86>,
    inst: Inst<'a>,
    which: Intrinsic<'a>,
    args: &[Value],
) -> CgResult<()> {
    let fp = fp_of(cx, inst.ty())?;
    match (which, args) {
        (Intrinsic::Sqrt, &[x]) => {
            push(cx, x, fp, inst)?;
            put(cx, mc::fsqrt());
            result(cx, inst, fp, true)
        }
        (Intrinsic::Fabs, &[x]) => {
            sign_op(cx, inst, x, fp, mc::fabs(), |m| mc::and_mi(Size::B, m, 0x7f))
        }
        (Intrinsic::CopySign, &[x, y]) => {
            let ys = cx.use_mem(y)?;
            copy_of(cx, inst, x, fp)?;
            let r = cx.def_mem(inst)?;
            let sign = fp.bytes() - 1;
            frame_inst(cx, ys.offset_by(sign), |m| mc::movzx_rm(Size::D, ECX, Size::B, m));
            put(cx, mc::and_ri(Size::D, ECX, 0x80));
            frame_inst(cx, r.offset_by(sign), |m| mc::and_mi(Size::B, m, 0x7f));
            frame_inst(cx, r.offset_by(sign), |m| mc::or_mr(Size::B, m, ECX));
            Ok(())
        }
        (Intrinsic::FmulAdd, &[a, b, c]) => {
            // `llc` without FMA: the product unrounded into the sum.
            push(cx, a, fp, inst)?;
            push(cx, b, fp, inst)?;
            put(cx, mc::fmulp(St::St1));
            push(cx, c, fp, inst)?;
            put(cx, mc::faddp(St::St1));
            result(cx, inst, fp, true)
        }
        _ => match routine_op(which) {
            Some(op) if args.iter().all(|&a| value_ty(cx, a) == inst.ty()) => {
                let func = routine(op, fp)?;
                libcall(cx, inst, func, args, fp)
            }
            _ => unsupported(INTRINSIC),
        },
    }
}

/// A call of libm's `func` with the floating arguments `args` (each on
/// the stack at its width) and the result in `st0` (hod's own call, not
/// the convention's: the general registers that hold values are
/// callee-saved, the x87 stack is empty).
fn libcall<'a>(
    cx: &mut FnCx<'a, X86>,
    inst: Inst<'a>,
    func: RuntimeFn,
    args: &[Value],
    fp: Fp,
) -> CgResult<()> {
    let mut places = Vec::with_capacity(args.len());
    for &a in args {
        places.push(cx.use_mem(a)?);
    }
    let area = (fp.arg_bytes() * args.len() as u32).next_multiple_of(16);
    below(cx, area);
    for (i, &place) in places.iter().enumerate() {
        copy_frame(cx, FrameRef::Outgoing(fp.arg_bytes() * i as u32), place, fp.bytes());
    }
    let at = cx.offset();
    // `pic`'s call: under PIC through the PLT with the GOT pointer in
    // `ebx`, as `llc` calls libm (`spec/codegen-x86.md` §9.2).
    crate::pic::emit_call(cx, CallTarget::Sym(SymRef::Runtime(func)), 0)?;
    let len = cx.offset() - at;
    cx.record_call_site(at, len);
    above(cx, area);
    // `st0` as the routine left it, for the run after the call.
    result(cx, inst, fp, true)
}

// ---- the dispatch ----

/// The lowering of an instruction [`owns`] says is this module's.
///
/// # Errors
/// The refusals of this module; what the operands report.
pub fn lower<'a>(cx: &mut FnCx<'a, X86>, inst: Inst<'a>) -> CgResult<()> {
    match inst.kind() {
        InstKind::Binary { lhs, rhs } => binary(cx, inst, lhs, rhs),
        InstKind::Unary { arg } => unary(cx, inst, arg),
        InstKind::Load { ptr, mem } => load(cx, inst, ptr, mem),
        InstKind::Store { value, ptr, mem } => store(cx, value, ptr, mem),
        InstKind::Select { cond, if_true, if_false } => select(cx, inst, cond, if_true, if_false),
        InstKind::Cast { arg } => cast(cx, inst, arg),
        InstKind::FCmp { pred, lhs, rhs } => fcmp(cx, inst, pred, lhs, rhs),
        InstKind::Call(site) => match site.intrinsic() {
            Some(which) => intrinsic(cx, inst, which, site.args()),
            None => unsupported(INTRINSIC),
        },
        _ => Err(CgError::unsupported(FLOAT)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_predicate_has_a_condition() {
        for &pred in FloatPredicate::ALL {
            let _ = conditions(pred);
        }
        assert_eq!(conditions(FloatPredicate::Olt), (true, FpCond::One(Cond::A)));
    }

    #[test]
    fn pieces_cover_the_encodings() {
        let p: Vec<_> = pieces(10).collect();
        assert_eq!(p, [(0, Size::D), (4, Size::D), (8, Size::W)]);
        assert_eq!(pieces(4).count(), 1);
    }
}
