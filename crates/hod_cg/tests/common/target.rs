//! The test target: `hod_cg::Target` for the test machine of `isa.rs`.
//!
//! It is written the way a real target is meant to be written against
//! `FnCx` (`spec/codegen.md` §8): operands through `use_reg`/`use_mem`,
//! results through `def_reg`/`finish`/`def_mem`, compares and constant
//! `ptradd`s deferred and fused, calls through `FnCx::call`, and the
//! `mem*` intrinsics, `frem` and 128-bit division through runtime calls.
//!
//! The registers:
//!
//! | registers | role |
//! |---|---|
//! | `g0`–`g2` | general arguments; `g0` (and `g1`) the return value; the `eh_data` pair |
//! | `g3` | the address of a result returned in memory |
//! | `g4`–`g8` | the general pool |
//! | `g9`–`g11` | scratch; `g11` the indirect callee |
//! | `f0`, `f1` | float arguments; `f0` the return value |
//! | `f2`–`f5` | the float pool |
//! | `f6`, `f7` | scratch |
//!
//! The convention asks the *producer* of an integer argument or return
//! value narrower than a register to extend it (zero, or sign with
//! `signext`); the callee's entry and its `ret` check that with `CHK`,
//! so a missing `Target::extend_arg` call is a failed run.

use std::sync::atomic::{AtomicU64, Ordering};

use hod_cg::abi::{AbiUnsupported, Assignment, Part, PartKind, Place, Sig};
use hod_cg::{
    CallArg, CallEmit, CallTarget, CfiOp, CgError, CgResult, Class, FixupKind, FnCx, FrameDesc,
    FrameRef, LIMIT_FRAME_SIZE, Label, Loc, PReg, RegInfo, RegSet, RuntimeFn, SlotPlace, SymRef,
    Target,
};
use hod_ir::{
    BlockId, Clause, Const, FixupTarget, Inst, InstKind, IntPredicate, Intrinsic, Opcode,
    ParamAttrs, ParamMem, PtrAdd, TypeId, TypeKind, TypeTable, Value, ValueKind,
};
use hod_obj::{A64Reloc, RelocKind};

use super::isa::{self, Enc, Opnd};

pub const fn g(index: u8) -> PReg {
    PReg::new(index)
}

pub const fn f(index: u8) -> PReg {
    PReg::new(isa::FPR0 + index)
}

const ARG_GPRS: [PReg; 3] = [g(0), g(1), g(2)];
const ARG_FPRS: [PReg; 2] = [f(0), f(1)];
/// Where the address of a result returned in memory travels.
pub const RET_MEMORY: PReg = g(3);
const SCRATCH: PReg = g(9);
const SCRATCH2: PReg = g(10);
const CALLEE: PReg = g(11);

/// The largest frame the machine's `ENTER` is given.
pub const TM_MAX_FRAME: u32 = 1 << 20;

const ABS64: RelocKind = RelocKind::A64(A64Reloc::Abs64);

fn dwarf(reg: PReg) -> u16 {
    u16::from(reg.index())
}

static REG_INFO: RegInfo = RegInfo {
    gpr: RegSet::of(&[g(0), g(1), g(2), g(3), g(4), g(5), g(6), g(7), g(8), g(9), g(10), g(11)]),
    fpr: RegSet::of(&[f(0), f(1), f(2), f(3), f(4), f(5), f(6), f(7)]),
    pool: RegSet::of(&[g(4), g(5), g(6), g(7), g(8), f(2), f(3), f(4), f(5)]),
    holdable: RegSet::of(&[
        g(0),
        g(1),
        g(2),
        g(3),
        g(4),
        g(5),
        g(6),
        g(7),
        g(8),
        f(0),
        f(1),
        f(2),
        f(3),
        f(4),
        f(5),
    ]),
    scratch: RegSet::of(&[g(9), g(10), g(11), f(6), f(7)]),
    eh_data: [g(0), g(1)],
    callee: CALLEE,
    gpr_bytes: 8,
    fpr_bytes: 8,
    dwarf,
};

/// The test target. It counts what it is asked to do, for the tests that
/// assert a retry happened.
#[derive(Debug, Default)]
pub struct Tm {
    /// Attempts at a function (`begin_function` calls).
    pub attempts: AtomicU64,
    /// Attempts in far mode.
    pub far_attempts: AtomicU64,
}

/// What the target remembers about a function.
#[derive(Clone, Copy, Debug, Default)]
pub struct TmState {
    /// Where the address of the result buffer was saved, for a function
    /// that returns in memory.
    ret_memory: Option<FrameRef>,
}

type Cx<'c, 'a> = &'c mut FnCx<'a, Tm>;

fn unsup<R>(what: &'static str) -> CgResult<R> {
    Err(CgError::unsupported(what))
}

fn mem(of: FrameRef) -> Opnd {
    match of {
        // The test target compiles no coroutine (`CORO_BODIES` is false).
        FrameRef::Local(at) | FrameRef::Coro(at) => Opnd::Mem { base: isa::FP, disp: at as i32 },
        FrameRef::Incoming(at) => Opnd::Mem { base: isa::AP, disp: at as i32 },
        FrameRef::Outgoing(at) => Opnd::Mem { base: isa::SP, disp: at as i32 },
    }
}

fn reg(of: PReg) -> Opnd {
    Opnd::Reg(of.index())
}

/// The width of a scalar type in bits.
fn bits_of(types: &TypeTable, ty: TypeId) -> CgResult<u8> {
    match types.kind(ty) {
        TypeKind::Int(bits) if bits <= 128 => Ok(bits as u8),
        TypeKind::Ptr { .. } | TypeKind::Double => Ok(64),
        TypeKind::Float => Ok(32),
        _ => unsup("tm:type"),
    }
}

/// The bytes a `Mem`-class value occupies.
fn mem_bytes(types: &TypeTable, ty: TypeId) -> CgResult<u32> {
    u32::try_from(types.alloc_size_of(ty)).or_else(|_| unsup("tm:type"))
}

fn value_ty(cx: &FnCx<'_, Tm>, value: Value) -> TypeId {
    cx.body.value_type(cx.module, value)
}

/// `value` as an operand: a pinned register, or the frame slot of a
/// `Mem`-class value.
fn src(cx: Cx<'_, '_>, value: Value) -> CgResult<Opnd> {
    if cx.class_of(value) == Class::Mem {
        Ok(mem(cx.use_mem(value)?))
    } else {
        Ok(reg(cx.use_reg(value)?))
    }
}

/// `value` as a second operand: as [`src`], except that a value that is
/// in a frame slot anyway (a home value, a displaced one) is read from
/// there instead of being loaded.
fn src_or_slot(cx: Cx<'_, '_>, value: Value) -> CgResult<Opnd> {
    if matches!(cx.loc(value)?, Loc::Frame(_)) {
        return Ok(mem(cx.use_mem(value)?));
    }
    src(cx, value)
}

/// Where the result of `inst` is computed.
#[derive(Clone, Copy)]
enum Dst {
    Reg(PReg),
    Mem(FrameRef),
}

impl Dst {
    fn opnd(self) -> Opnd {
        match self {
            Dst::Reg(of) => reg(of),
            Dst::Mem(of) => mem(of),
        }
    }
}

fn dst<'a>(cx: Cx<'_, 'a>, inst: Inst<'a>) -> CgResult<Dst> {
    if cx.class_of_type(inst.ty()) == Class::Mem {
        Ok(Dst::Mem(cx.def_mem(inst)?))
    } else {
        Ok(Dst::Reg(cx.def_reg(inst)?))
    }
}

fn done<'a>(cx: Cx<'_, 'a>, inst: Inst<'a>, dst: Dst) -> CgResult<()> {
    match dst {
        Dst::Reg(of) => cx.finish(inst, of),
        Dst::Mem(_) => Ok(()),
    }
}

trait CxExt {
    fn class_of_type(&self, ty: TypeId) -> Class;
    fn put_ins(&mut self, ins: Enc);
}

impl CxExt for FnCx<'_, Tm> {
    fn class_of_type(&self, ty: TypeId) -> Class {
        self.target.class_of(self.types(), ty)
    }

    fn put_ins(&mut self, ins: Enc) {
        self.emit(ins.bytes());
    }
}

/// Emits a branch: `ins` is the instruction up to its target field, with
/// the near opcode; the far opcode is the next one.
fn branch(cx: Cx<'_, '_>, near: u8, fields: impl FnOnce(Enc) -> Enc, to: Label) -> CgResult<()> {
    let far = cx.far();
    let ins = fields(Enc::op(if far { near + 1 } else { near }));
    let at = cx.offset() + ins.len();
    cx.put_ins(if far { ins.u32(0) } else { ins.u16(0) });
    cx.fixup(at, FixupKind(u8::from(far)), to)
}

/// The lowering's part of §4.6: a deferred operand is emitted before
/// any other operand pins a register, since emitting it needs registers
/// of its own (`FnCx::deferred`). Left alone: an instruction that is
/// itself deferred, the operand an instruction fuses (the compare of a
/// `condbr`, the address of a load or store), and the operands of a
/// call, which the call sequence takes care of.
fn deferred_first<'a>(cx: Cx<'_, 'a>, inst: Inst<'a>) -> CgResult<()> {
    let fused = match inst.kind() {
        InstKind::ICmp { .. } if cx.may_defer(inst) => return Ok(()),
        InstKind::PtrAdd(add) if foldable(&add).is_some() && cx.may_defer(inst) => return Ok(()),
        InstKind::Call(site) if site.intrinsic().is_none() => return Ok(()),
        InstKind::CondBr { cond, .. } => Some(cond),
        InstKind::Load { ptr, .. } | InstKind::Store { ptr, .. } => Some(ptr),
        _ => None,
    };
    for &operand in inst.operands() {
        if Some(operand) != fused && cx.deferred(operand).is_some() {
            cx.use_reg(operand)?;
        }
    }
    Ok(())
}

/// The address `ptr` as a memory operand. A deferred `ptradd` by a
/// constant is fused into the operand (§4.6); a stack slot is addressed
/// through the frame pointer.
fn address(cx: Cx<'_, '_>, ptr: Value) -> CgResult<Opnd> {
    if let Some(def) = cx.deferred(ptr)
        && let InstKind::PtrAdd(add) = def.kind()
        && let Some(disp) = foldable(&add)
    {
        let base = cx.use_reg(add.base)?;
        return Ok(Opnd::Mem { base: base.index(), disp });
    }
    if let ValueKind::Slot(slot) = ptr.kind()
        && let SlotPlace::Direct(place) = cx.slot_place(slot)?
    {
        return Ok(mem(place));
    }
    let base = cx.use_reg(ptr)?;
    Ok(Opnd::Mem { base: base.index(), disp: 0 })
}

/// The displacement of a `ptradd` that is a base plus a constant.
fn foldable(add: &PtrAdd<'_>) -> Option<i32> {
    if add.indices().len() != 0 {
        return None;
    }
    i32::try_from(add.offset).ok()
}

impl Target for Tm {
    type FnState = TmState;

    const TRAP: &'static [u8] = &[isa::TRAP];
    const FN_ALIGN: u32 = 4;
    const PAD_BYTE: u8 = 0;
    const STACK_ALIGN: u32 = 16;

    fn reg_info(&self) -> &RegInfo {
        &REG_INFO
    }

    fn class_of(&self, types: &TypeTable, ty: TypeId) -> Class {
        match types.kind(ty) {
            TypeKind::Int(..=64) | TypeKind::Ptr { .. } => Class::Gpr,
            TypeKind::Float | TypeKind::Double => Class::Fpr,
            _ => Class::Mem,
        }
    }

    fn assign(
        &self,
        types: &TypeTable,
        sig: &Sig<'_>,
        out: &mut Assignment,
    ) -> Result<(), AbiUnsupported> {
        if sig.variadic {
            return Err(AbiUnsupported("tm:variadic"));
        }
        let (mut gprs, mut fprs, mut stack) = (0usize, 0usize, 0u32);
        let value = |arg: usize, offset: u32, size: u32, place: Place| Part {
            arg: arg as u32,
            kind: PartKind::Value,
            offset,
            size,
            place,
        };
        for (index, &ty) in sig.params.iter().enumerate() {
            let attrs = sig.attrs(index);
            if attrs.mem() == ParamMem::ByVal {
                // The pointee travels on the stack; the callee sees its
                // address.
                let Some(pointee) = attrs.mem_type() else {
                    return Err(AbiUnsupported("tm:byval-type"));
                };
                let size = (types.alloc_size_of(pointee) as u32).next_multiple_of(8);
                out.args.push(Part {
                    arg: index as u32,
                    kind: PartKind::Pointee,
                    offset: 0,
                    size,
                    place: Place::Stack(stack),
                });
                stack += size;
                continue;
            }
            match self.class_of(types, ty) {
                Class::Gpr if gprs < ARG_GPRS.len() => {
                    out.args.push(value(index, 0, 8, Place::Reg(ARG_GPRS[gprs])));
                    gprs += 1;
                }
                Class::Fpr if fprs < ARG_FPRS.len() => {
                    out.args.push(value(index, 0, 8, Place::Reg(ARG_FPRS[fprs])));
                    fprs += 1;
                }
                Class::Gpr | Class::Fpr => {
                    out.args.push(value(index, 0, 8, Place::Stack(stack)));
                    stack += 8;
                }
                Class::Mem if types.int_bits(ty) == Some(128) => {
                    // In two general registers, or split between the
                    // last one and the stack, or on the stack.
                    for half in 0..2 {
                        if gprs < ARG_GPRS.len() {
                            out.args.push(value(index, half * 8, 8, Place::Reg(ARG_GPRS[gprs])));
                            gprs += 1;
                        } else {
                            out.args.push(value(index, half * 8, 8, Place::Stack(stack)));
                            stack += 8;
                        }
                    }
                    // Two stack halves are one part: the incoming
                    // location is then the value's home (§4.8).
                    if let [.., a, b] = out.args[..]
                        && a.arg == index as u32
                        && let (Place::Stack(at), Place::Stack(_)) = (a.place, b.place)
                    {
                        out.args.truncate(out.args.len() - 2);
                        out.args.push(value(index, 0, 16, Place::Stack(at)));
                    }
                }
                Class::Mem => {
                    let size = (types.alloc_size_of(ty) as u32).next_multiple_of(8);
                    if size != 0 {
                        out.args.push(value(
                            index,
                            0,
                            types.alloc_size_of(ty) as u32,
                            Place::Stack(stack),
                        ));
                        stack += size;
                    }
                }
            }
        }
        out.stack_size = stack;
        if sig.ret != TypeId::VOID {
            match self.class_of(types, sig.ret) {
                Class::Gpr => out.ret.push(value(0, 0, 8, Place::Reg(g(0)))),
                Class::Fpr => out.ret.push(value(0, 0, 8, Place::Reg(f(0)))),
                Class::Mem if types.int_bits(sig.ret) == Some(128) => {
                    out.ret.push(value(0, 0, 8, Place::Reg(g(0))));
                    out.ret.push(value(0, 8, 8, Place::Reg(g(1))));
                }
                Class::Mem => {
                    out.ret_memory = Some(hod_cg::abi::RetMemory {
                        address: Place::Reg(RET_MEMORY),
                        returned_in: None,
                    });
                }
            }
        }
        Ok(())
    }

    fn begin_function(&self, cx: &mut FnCx<'_, Self>) -> CgResult<()> {
        self.attempts.fetch_add(1, Ordering::Relaxed);
        if cx.far() {
            self.far_attempts.fetch_add(1, Ordering::Relaxed);
        }
        // The caller's frame and argument pointers are saved in the
        // first sixteen bytes of the local area.
        if cx.alloc_slot(16, 8)? != FrameRef::Local(0) {
            return cx.bug("the save area is not at the start of the local area");
        }
        cx.put_ins(Enc::op(isa::ENTER).u32(0));
        cx.cfi(CfiOp::DefCfa { reg: u16::from(isa::FP), offset: 16 });
        // The convention's promise about narrow integer arguments.
        let types = cx.types();
        let params = cx.body.arg_types(cx.module);
        let symbol = cx.module.symbol(cx.body.symbol());
        for index in 0..cx.fn_assignment().args.len() {
            let part = cx.fn_assignment().args[index];
            let Some(&ty) = params.get(part.arg as usize) else { continue };
            if let (Place::Reg(at), Some(bits)) = (part.place, types.int_bits(ty))
                && bits < 64
            {
                let signed = symbol.param_attrs(part.arg as usize).has(ParamAttrs::SIGNEXT);
                cx.put_ins(Enc::op(isa::CHK).u8(at.index()).u8(bits as u8).u8(u8::from(signed)));
            }
        }
        if cx.fn_assignment().ret_memory.is_some() {
            let slot = cx.alloc_slot(8, 8)?;
            self.store(cx, RET_MEMORY, slot, 8);
            cx.state.ret_memory = Some(slot);
        }
        Ok(())
    }

    fn end_function(&self, cx: &mut FnCx<'_, Self>) -> CgResult<()> {
        let frame = cx.frame_size().next_multiple_of(Self::STACK_ALIGN);
        if frame > TM_MAX_FRAME {
            return unsup(LIMIT_FRAME_SIZE);
        }
        cx.code_mut()[1..5].copy_from_slice(&frame.to_le_bytes());
        cx.bind(cx.epilogue())?;
        cx.put_ins(Enc::op(isa::LEAVE));
        cx.cfi(CfiOp::Restore { reg: u16::from(isa::FP) });
        cx.put_ins(Enc::op(isa::RET));
        for index in 0..cx.tail_exit_count() {
            let exit = cx.tail_exit(index);
            cx.bind(exit.label)?;
            cx.put_ins(Enc::op(isa::LEAVE));
            match exit.target {
                CallTarget::Sym(sym) => {
                    let at = cx.offset() + 1;
                    cx.put_ins(Enc::op(isa::TAILJ).u64(0));
                    cx.reloc(at, ABS64, sym, 0);
                }
                CallTarget::Reg(callee) => cx.put_ins(Enc::op(isa::TAILR).u8(callee.index())),
            }
        }
        cx.set_frame_desc(FrameDesc { frame_size: frame, frame_base: u16::from(isa::FP) });
        Ok(())
    }

    fn lower<'a>(&self, cx: &mut FnCx<'a, Self>, inst: Inst<'a>) -> CgResult<()> {
        deferred_first(cx, inst)?;
        match inst.kind() {
            InstKind::Binary { lhs, rhs } => binary(cx, inst, lhs, rhs),
            InstKind::Unary { arg } => unary(cx, inst, arg),
            InstKind::Cast { arg } => cast(cx, inst, arg),
            InstKind::ICmp { pred, lhs, rhs } => icmp(cx, inst, pred, lhs, rhs),
            InstKind::FCmp { pred, lhs, rhs } => {
                let double = value_ty(cx, lhs) == TypeId::DOUBLE;
                let (a, b) = (cx.use_reg(lhs)?, cx.use_reg(rhs)?);
                let d = cx.def_reg(inst)?;
                cx.put_ins(
                    Enc::op(isa::FCMP)
                        .u8(pred as u8)
                        .u8(u8::from(double))
                        .u8(d.index())
                        .u8(a.index())
                        .u8(b.index()),
                );
                cx.finish(inst, d)
            }
            InstKind::Load { ptr, .. } => load(cx, inst, ptr),
            InstKind::Store { value, ptr, .. } => store(cx, value, ptr),
            InstKind::PtrAdd(add) => ptr_add(cx, inst, &add),
            InstKind::ExtractValue { agg, offset } => {
                let Ok(offset) = u32::try_from(offset) else { return unsup("tm:offset") };
                let from = cx.use_mem(agg)?.offset_by(offset);
                match dst(cx, inst)? {
                    Dst::Reg(d) => {
                        let bits = bits_of(cx.types(), inst.ty())?;
                        cx.put_ins(Enc::op(isa::LOAD).u8(d.index()).opnd(mem(from)).u8(bits));
                        cx.finish(inst, d)
                    }
                    Dst::Mem(d) => {
                        let bytes = mem_bytes(cx.types(), inst.ty())?;
                        self.copy(cx, d, from, bytes);
                        Ok(())
                    }
                }
            }
            InstKind::InsertValue { agg, value, offset } => {
                let Ok(offset) = u32::try_from(offset) else { return unsup("tm:offset") };
                let from = cx.use_mem(agg)?;
                let d = cx.def_mem(inst)?;
                let bytes = mem_bytes(cx.types(), inst.ty())?;
                self.copy(cx, d, from, bytes);
                store_to(cx, value, mem(d.offset_by(offset)))
            }
            InstKind::Select { cond, if_true, if_false } => {
                let c = cx.use_reg(cond)?;
                if cx.class_of_type(inst.ty()) == Class::Mem {
                    let bits = bits_of(cx.types(), inst.ty())?;
                    let (a, b) = (cx.use_mem(if_true)?, cx.use_mem(if_false)?);
                    let d = cx.def_mem(inst)?;
                    cx.put_ins(
                        Enc::op(isa::SEL)
                            .u8(bits)
                            .opnd(mem(d))
                            .u8(c.index())
                            .opnd(mem(a))
                            .opnd(mem(b)),
                    );
                    return Ok(());
                }
                let (a, b) = (cx.use_reg(if_true)?, cx.use_reg(if_false)?);
                cx.put_ins(
                    Enc::op(isa::SEL).u8(64).opnd(reg(a)).u8(c.index()).opnd(reg(a)).opnd(reg(b)),
                );
                cx.finish(inst, a)
            }
            InstKind::Call(site) => match site.intrinsic() {
                Some(intrinsic) => self.intrinsic(cx, inst, intrinsic, site.args()),
                None => cx.call(inst),
            },
            InstKind::LandingPad(pad) => {
                let type_info = |cx: &FnCx<'_, Self>, id| match cx.module.consts.get(id) {
                    Const::Null(_) => Ok(None),
                    Const::Addr { sym, addend: 0 } => Ok(Some(sym)),
                    _ => unsup("tm:catch-clause"),
                };
                for &clause in pad.clauses() {
                    match clause {
                        Clause::Catch(id) => {
                            let sym = type_info(cx, id)?;
                            cx.pad_clause(sym)?;
                        }
                        Clause::Filter(id) => {
                            let elems: &[_] = match cx.module.consts.get(id) {
                                Const::Aggregate { elems, .. } => elems,
                                Const::Zero(_) => &[],
                                _ => return unsup("tm:filter"),
                            };
                            let syms = elems
                                .iter()
                                .map(|&e| type_info(cx, e))
                                .collect::<CgResult<Vec<_>>>()?;
                            cx.pad_filter(&syms)?;
                        }
                    }
                }
                let d = cx.def_mem(inst)?;
                let [exception, selector] = REG_INFO.eh_data;
                self.store(cx, exception, d, 8);
                self.store(cx, selector, d.offset_by(8), 4);
                Ok(())
            }
            InstKind::Resume { value } => {
                let from = cx.use_mem(value)?;
                let exception = cx.temp(Class::Gpr)?;
                self.load(cx, exception, from, 8);
                cx.call_runtime(RuntimeFn::UnwindResume, &[CallArg::Reg(exception)], None)?;
                cx.trap();
                Ok(())
            }
            InstKind::Br { target } => {
                if !cx.falls_through(target) {
                    self.jump(cx, cx.block_label(target))?;
                }
                Ok(())
            }
            InstKind::CondBr { cond, if_true, if_false } => cond_br(cx, cond, if_true, if_false),
            InstKind::Switch(switch) => {
                let bits = bits_of(cx.types(), value_ty(cx, switch.cond))?;
                if bits > 64 {
                    return unsup("tm:switch-wide");
                }
                let on = cx.use_reg(switch.cond)?;
                for (case, target) in switch.cases() {
                    let Const::Int { value, .. } = cx.module.consts.get(case) else {
                        return unsup("tm:switch-case");
                    };
                    let to = cx.block_label(target);
                    branch(
                        cx,
                        isa::JCMP,
                        |ins| {
                            ins.u8(IntPredicate::Eq as u8)
                                .u8(bits)
                                .opnd(reg(on))
                                .opnd(Opnd::Imm(value))
                        },
                        to,
                    )?;
                }
                if !cx.falls_through(switch.default) {
                    self.jump(cx, cx.block_label(switch.default))?;
                }
                Ok(())
            }
            InstKind::IndirectBr { addr, .. } => {
                let to = cx.use_reg(addr)?;
                cx.put_ins(Enc::op(isa::JMPR).u8(to.index()));
                Ok(())
            }
            InstKind::Ret { value } => {
                if let Some(value) = value {
                    self.ret(cx, value)?;
                }
                if !cx.ret_falls_through() {
                    self.jump(cx, cx.epilogue())?;
                }
                Ok(())
            }
            InstKind::Unreachable => {
                cx.trap();
                Ok(())
            }
            InstKind::Alloca { .. } => unsup("tm:alloca"),
            InstKind::Fence { .. } | InstKind::CmpXchg { .. } | InstKind::AtomicRmw { .. } => {
                unsup("tm:atomic")
            }
            InstKind::ExtractElement { .. }
            | InstKind::InsertElement { .. }
            | InstKind::ShuffleVector { .. } => unsup("tm:vector"),
            InstKind::VaArg { .. } => unsup("tm:va_arg"),
            InstKind::Phi(_) => cx.bug("a phi reached the lowering"),
            InstKind::Funclet => unsup("tm:funclet"),
        }
    }

    fn load(&self, cx: &mut FnCx<'_, Self>, reg: PReg, src: FrameRef, bytes: u32) {
        cx.put_ins(Enc::op(isa::LOAD).u8(reg.index()).opnd(mem(src)).u8((bytes * 8) as u8));
    }

    fn store(&self, cx: &mut FnCx<'_, Self>, reg: PReg, dst: FrameRef, bytes: u32) {
        cx.put_ins(Enc::op(isa::STORE).u8(reg.index()).opnd(mem(dst)).u8((bytes * 8) as u8));
    }

    fn copy(&self, cx: &mut FnCx<'_, Self>, dst: FrameRef, src: FrameRef, bytes: u32) {
        if dst != src {
            cx.put_ins(Enc::op(isa::COPY).opnd(mem(dst)).opnd(mem(src)).u32(bytes));
        }
    }

    fn copy_from_ptr(
        &self,
        cx: &mut FnCx<'_, Self>,
        dst: FrameRef,
        ptr: PReg,
        offset: u32,
        bytes: u32,
    ) {
        let from = Opnd::Mem { base: ptr.index(), disp: offset as i32 };
        cx.put_ins(Enc::op(isa::COPY).opnd(mem(dst)).opnd(from).u32(bytes));
    }

    fn mov(&self, cx: &mut FnCx<'_, Self>, dst: PReg, src: PReg) {
        cx.put_ins(Enc::op(isa::MOV).u8(dst.index()).u8(src.index()));
    }

    fn materialize(&self, cx: &mut FnCx<'_, Self>, reg: PReg, value: Value) -> CgResult<()> {
        let movi = |cx: &mut FnCx<'_, Self>, imm: u64| {
            cx.put_ins(Enc::op(isa::MOVI).u8(reg.index()).u64(imm));
        };
        match value.kind() {
            ValueKind::Sym(sym) => {
                let at = cx.offset() + 2;
                movi(cx, 0);
                cx.reloc(at, ABS64, SymRef::Module(sym), 0);
            }
            ValueKind::Slot(slot) => match cx.slot_place(slot)? {
                SlotPlace::Direct(place) => self.frame_addr(cx, reg, place),
                SlotPlace::Indirect(pointer) => self.load(cx, reg, pointer, 8),
            },
            ValueKind::Const(id) => match cx.module.consts.get(id) {
                Const::Int { value, .. } | Const::AddrOfInt { value } => movi(cx, value),
                Const::Float { bits: 0, .. }
                | Const::Null(_)
                | Const::Zero(_)
                | Const::Undef(_)
                | Const::Poison(_) => movi(cx, 0),
                Const::Float { ty, bits } => {
                    // A float constant comes from a data island (§7.5).
                    let width = bits_of(cx.types(), ty)?;
                    let bytes = bits.to_le_bytes();
                    let island =
                        cx.island(&bytes[..usize::from(width / 8)], u32::from(width / 8))?;
                    branch(cx, isa::LDPC, |ins| ins.u8(reg.index()).u8(width), island)?;
                }
                Const::Addr { sym, addend } => {
                    let at = cx.offset() + 2;
                    movi(cx, 0);
                    cx.reloc(at, ABS64, SymRef::Module(sym), addend);
                }
                Const::BlockAddr { func, block } if func == cx.body.symbol() => {
                    let ins = Enc::op(isa::LEAPC).u8(reg.index());
                    let at = cx.offset() + ins.len();
                    cx.put_ins(ins.u32(0));
                    cx.fixup(at, FixupKind(1), cx.block_label(block))?;
                }
                _ => return unsup("tm:constant"),
            },
            _ => return cx.bug("materialize of something that is no constant"),
        }
        Ok(())
    }

    fn materialize_mem(
        &self,
        cx: &mut FnCx<'_, Self>,
        dst: FrameRef,
        value: Value,
    ) -> CgResult<()> {
        let ValueKind::Const(id) = value.kind() else {
            return cx.bug("materialize_mem of something that is no constant");
        };
        let (mut image, mut fixups) = (Vec::new(), Vec::new());
        if cx.module.consts.flatten(cx.types(), id, &mut image, &mut fixups).is_err() {
            return unsup("tm:constant");
        }
        // A fixup is patched as a whole word; one inside a word would be
        // lost.
        if fixups.iter().any(|fixup| !fixup.offset.is_multiple_of(8)) {
            return unsup("tm:constant");
        }
        // Word by word, through the second scratch register: the
        // policy's own copies go through the first.
        for (index, word) in image.chunks(8).enumerate() {
            let at = index as u32 * 8;
            let mut bytes = [0u8; 8];
            bytes[..word.len()].copy_from_slice(word);
            let reloc_at = cx.offset() + 2;
            cx.put_ins(Enc::op(isa::MOVI).u8(SCRATCH2.index()).u64(u64::from_le_bytes(bytes)));
            if let Some(fixup) = fixups.iter().find(|fixup| fixup.offset == u64::from(at)) {
                let FixupTarget::Sym(sym) = fixup.target else { return unsup("tm:constant") };
                cx.reloc(reloc_at, ABS64, SymRef::Module(sym), fixup.addend);
            }
            self.store(cx, SCRATCH2, dst.offset_by(at), word.len() as u32);
        }
        Ok(())
    }

    fn frame_addr(&self, cx: &mut FnCx<'_, Self>, reg: PReg, of: FrameRef) {
        cx.put_ins(Enc::op(isa::LEA).u8(reg.index()).opnd(mem(of)));
    }

    fn adjust_sp(&self, cx: &mut FnCx<'_, Self>, delta: i64) {
        cx.put_ins(Enc::op(isa::ADDSP).u32(delta as u32));
    }

    fn extend_arg(&self, cx: &mut FnCx<'_, Self>, reg: PReg, ty: TypeId, attrs: ParamAttrs) {
        if let Some(bits) = cx.types().int_bits(ty)
            && bits < 64
        {
            let op = if attrs.has(ParamAttrs::SIGNEXT) { Opcode::SExt } else { Opcode::ZExt };
            let at = Opnd::Reg(reg.index());
            cx.put_ins(Enc::op(isa::CAST).u8(op as u8).u8(bits as u8).u8(64).opnd(at).opnd(at));
        }
    }

    fn jump(&self, cx: &mut FnCx<'_, Self>, label: Label) -> CgResult<()> {
        branch(cx, isa::JMP, |ins| ins, label)
    }

    fn emit_call(&self, cx: &mut FnCx<'_, Self>, call: &CallEmit) -> CgResult<()> {
        match call.target {
            CallTarget::Sym(sym) => {
                let at = cx.offset() + 1;
                cx.put_ins(Enc::op(isa::CALL).u64(0));
                cx.reloc(at, ABS64, sym, 0);
            }
            CallTarget::Reg(callee) => cx.put_ins(Enc::op(isa::CALLR).u8(callee.index())),
        }
        Ok(())
    }

    fn dynamic_slot(
        &self,
        cx: &mut FnCx<'_, Self>,
        ptr: FrameRef,
        size: u64,
        align: u64,
    ) -> CgResult<()> {
        let (Ok(size), Ok(align)) = (u32::try_from(size), u32::try_from(align)) else {
            return unsup(LIMIT_FRAME_SIZE);
        };
        cx.put_ins(Enc::op(isa::DYNSLOT).opnd(mem(ptr)).u32(size).u32(align));
        Ok(())
    }

    fn patch_branch(code: &mut [u8], at: u32, kind: FixupKind, displacement: i64) -> bool {
        let at = at as usize;
        match kind.0 {
            0 if displacement.abs() <= isa::NEAR_REACH => {
                code[at..at + 2].copy_from_slice(&(displacement as i16).to_le_bytes());
                true
            }
            1 if displacement.abs() <= isa::FAR_REACH => {
                code[at..at + 4].copy_from_slice(&(displacement as i32).to_le_bytes());
                true
            }
            _ => false,
        }
    }
}

impl Tm {
    /// `ret value`: the value to where the function returns it.
    fn ret(&self, cx: &mut FnCx<'_, Self>, value: Value) -> CgResult<()> {
        if let Some(saved) = cx.state.ret_memory {
            // Returned in memory: copied to the buffer the caller named.
            let ty = value_ty(cx, value);
            let bytes = mem_bytes(cx.types(), ty)?;
            let from = cx.use_mem(value)?;
            self.load(cx, SCRATCH, saved, 8);
            let to = Opnd::Mem { base: SCRATCH.index(), disp: 0 };
            cx.put_ins(Enc::op(isa::COPY).opnd(to).opnd(mem(from)).u32(bytes));
            return Ok(());
        }
        cx.place_return(value)?;
        let ty = cx.body.ret_type(cx.module);
        if let Some(bits) = cx.types().int_bits(ty)
            && bits < 64
        {
            let symbol = cx.module.symbol(cx.body.symbol());
            let signed = matches!(
                symbol.kind(),
                hod_ir::SymbolKind::Function { ret_attrs, .. } if ret_attrs.has(ParamAttrs::SIGNEXT)
            );
            cx.put_ins(Enc::op(isa::CHK).u8(g(0).index()).u8(bits as u8).u8(u8::from(signed)));
        }
        Ok(())
    }

    fn intrinsic<'a>(
        &self,
        cx: &mut FnCx<'a, Self>,
        inst: Inst<'a>,
        intrinsic: Intrinsic<'a>,
        args: &'a [Value],
    ) -> CgResult<()> {
        let arg = |index: usize| {
            args.get(index).copied().ok_or_else(|| CgError::unsupported("tm:intrinsic"))
        };
        let int_op = |cx: &mut FnCx<'a, Self>, op: u8, operands: usize| -> CgResult<()> {
            let bits = bits_of(cx.types(), inst.ty())?;
            if bits > 64 {
                return unsup("tm:intrinsic-wide");
            }
            let a = cx.use_reg(arg(0)?)?;
            let mut rest = [Opnd::Imm(0); 2];
            for (index, slot) in rest.iter_mut().enumerate().take(operands - 1) {
                *slot = reg(cx.use_reg(arg(index + 1)?)?);
            }
            cx.put_ins(
                Enc::op(isa::INTR)
                    .u8(op)
                    .u8(bits)
                    .opnd(reg(a))
                    .opnd(reg(a))
                    .opnd(rest[0])
                    .opnd(rest[1]),
            );
            cx.finish(inst, a)
        };
        let checked = |cx: &mut FnCx<'a, Self>, op: u8| -> CgResult<()> {
            let bits = bits_of(cx.types(), value_ty(cx, arg(0)?))?;
            let Some(&[_, flag_at]) = cx.types().field_offsets(inst.ty()) else {
                return unsup("tm:intrinsic");
            };
            let (a, b) = (cx.use_reg(arg(0)?)?, cx.use_reg(arg(1)?)?);
            let d = cx.def_mem(inst)?;
            cx.put_ins(
                Enc::op(isa::CHECKED)
                    .u8(op)
                    .u8(bits)
                    .opnd(mem(d))
                    .u32(flag_at as u32)
                    .opnd(reg(a))
                    .opnd(reg(b)),
            );
            Ok(())
        };
        match intrinsic {
            Intrinsic::Memcpy | Intrinsic::Memmove => {
                let func = if intrinsic == Intrinsic::Memcpy {
                    RuntimeFn::Memcpy
                } else {
                    RuntimeFn::Memmove
                };
                let pointer = |cx: &mut FnCx<'a, Self>, value: Value| -> CgResult<CallArg> {
                    // A stack slot's address is passed as what it is.
                    if let ValueKind::Slot(slot) = value.kind()
                        && let SlotPlace::Direct(place) = cx.slot_place(slot)?
                    {
                        return Ok(CallArg::Addr(place));
                    }
                    Ok(CallArg::Value(value))
                };
                let (to, from) = (pointer(cx, arg(0)?)?, pointer(cx, arg(1)?)?);
                cx.call_runtime(func, &[to, from, CallArg::Value(arg(2)?)], None)?;
                Ok(())
            }
            Intrinsic::Memset => {
                // The byte is an `i8`; `memset` takes an `int`.
                let byte = cx.temp(Class::Gpr)?;
                cx.put(arg(1)?, byte)?;
                let at = reg(byte);
                cx.put_ins(
                    Enc::op(isa::CAST).u8(Opcode::ZExt as u8).u8(8).u8(64).opnd(at).opnd(at),
                );
                let args = [CallArg::Value(arg(0)?), CallArg::Reg(byte), CallArg::Value(arg(2)?)];
                cx.call_runtime(RuntimeFn::Memset, &args, None)?;
                Ok(())
            }
            Intrinsic::Abs => int_op(cx, isa::intr::ABS, 1),
            Intrinsic::Smax => int_op(cx, isa::intr::SMAX, 2),
            Intrinsic::Smin => int_op(cx, isa::intr::SMIN, 2),
            Intrinsic::Umax => int_op(cx, isa::intr::UMAX, 2),
            Intrinsic::Umin => int_op(cx, isa::intr::UMIN, 2),
            Intrinsic::Ctpop => int_op(cx, isa::intr::CTPOP, 1),
            Intrinsic::Ctlz => int_op(cx, isa::intr::CTLZ, 1),
            Intrinsic::Cttz => int_op(cx, isa::intr::CTTZ, 1),
            Intrinsic::Bswap => int_op(cx, isa::intr::BSWAP, 1),
            Intrinsic::BitReverse => int_op(cx, isa::intr::BITREVERSE, 1),
            Intrinsic::Fshl => int_op(cx, isa::intr::FSHL, 3),
            Intrinsic::Fshr => int_op(cx, isa::intr::FSHR, 3),
            Intrinsic::SaddSat => int_op(cx, isa::intr::SADD_SAT, 2),
            Intrinsic::UaddSat => int_op(cx, isa::intr::UADD_SAT, 2),
            Intrinsic::SsubSat => int_op(cx, isa::intr::SSUB_SAT, 2),
            Intrinsic::UsubSat => int_op(cx, isa::intr::USUB_SAT, 2),
            Intrinsic::SaddWithOverflow => checked(cx, isa::checked::SADD),
            Intrinsic::UaddWithOverflow => checked(cx, isa::checked::UADD),
            Intrinsic::SsubWithOverflow => checked(cx, isa::checked::SSUB),
            Intrinsic::UsubWithOverflow => checked(cx, isa::checked::USUB),
            Intrinsic::SmulWithOverflow => checked(cx, isa::checked::SMUL),
            Intrinsic::UmulWithOverflow => checked(cx, isa::checked::UMUL),
            Intrinsic::Trap => {
                cx.trap();
                Ok(())
            }
            // The type id is a constant of the function's table (§9.4).
            Intrinsic::EhTypeidFor => {
                let sym = match arg(0)?.kind() {
                    ValueKind::Sym(sym) => sym,
                    ValueKind::Const(id) => match cx.module.consts.get(id) {
                        Const::Addr { sym, addend: 0 } => sym,
                        _ => return unsup("tm:intrinsic"),
                    },
                    _ => return unsup("tm:intrinsic"),
                };
                let id = cx.type_id(Some(sym));
                let d = cx.def_reg(inst)?;
                cx.put_ins(Enc::op(isa::MOVI).u8(d.index()).u64(u64::from(id)));
                cx.finish(inst, d)
            }
            _ => unsup("tm:intrinsic"),
        }
    }
}

fn binary<'a>(cx: Cx<'_, 'a>, inst: Inst<'a>, lhs: Value, rhs: Value) -> CgResult<()> {
    let op = inst.opcode();
    if op.is_float_binary() {
        let double = inst.ty() == TypeId::DOUBLE;
        if op == Opcode::FRem {
            // §7.3: `frem` is a call of `fmod`.
            let func = if double { RuntimeFn::Fmod } else { RuntimeFn::Fmodf };
            let args = [CallArg::Value(lhs), CallArg::Value(rhs)];
            let Some(result) = cx.call_runtime(func, &args, None)? else {
                return cx.bug("fmod returned nothing");
            };
            return cx.finish(inst, result);
        }
        let (a, b) = (cx.use_reg(lhs)?, cx.use_reg(rhs)?);
        cx.put_ins(
            Enc::op(isa::FBIN)
                .u8(op as u8)
                .u8(u8::from(double))
                .u8(a.index())
                .u8(a.index())
                .u8(b.index()),
        );
        return cx.finish(inst, a);
    }
    let bits = bits_of(cx.types(), inst.ty())?;
    if cx.class_of_type(inst.ty()) == Class::Mem {
        let d = cx.def_mem(inst)?;
        // §7.3: 128-bit division is a runtime call. The unsigned ones
        // pass the slots, the signed ones the values: both are ways to
        // pass a `Mem`-class operand.
        let func = match op {
            Opcode::UDiv => Some(RuntimeFn::UDivTi3),
            Opcode::URem => Some(RuntimeFn::UModTi3),
            Opcode::SDiv => Some(RuntimeFn::DivTi3),
            Opcode::SRem => Some(RuntimeFn::ModTi3),
            _ => None,
        };
        if let Some(func) = func {
            let args = if matches!(op, Opcode::UDiv | Opcode::URem) {
                [CallArg::Mem(cx.use_mem(lhs)?), CallArg::Mem(cx.use_mem(rhs)?)]
            } else {
                [CallArg::Value(lhs), CallArg::Value(rhs)]
            };
            cx.call_runtime(func, &args, Some(d))?;
            return Ok(());
        }
        let (a, b) = (cx.use_mem(lhs)?, cx.use_mem(rhs)?);
        cx.put_ins(Enc::op(isa::BIN).u8(op as u8).u8(bits).opnd(mem(d)).opnd(mem(a)).opnd(mem(b)));
        return Ok(());
    }
    // The result is computed into the first operand's register: it is
    // dead after this instruction, or a temporary.
    let a = cx.use_reg(lhs)?;
    let b = src_or_slot(cx, rhs)?;
    cx.put_ins(Enc::op(isa::BIN).u8(op as u8).u8(bits).opnd(reg(a)).opnd(reg(a)).opnd(b));
    cx.finish(inst, a)
}

fn unary<'a>(cx: Cx<'_, 'a>, inst: Inst<'a>, arg: Value) -> CgResult<()> {
    if inst.opcode() == Opcode::FNeg {
        let double = inst.ty() == TypeId::DOUBLE;
        let a = cx.use_reg(arg)?;
        cx.put_ins(Enc::op(isa::FNEG).u8(u8::from(double)).u8(a.index()).u8(a.index()));
        return cx.finish(inst, a);
    }
    // `freeze` is a copy (§4.7).
    if cx.class_of_type(inst.ty()) == Class::Mem {
        let bytes = mem_bytes(cx.types(), inst.ty())?;
        let from = cx.use_mem(arg)?;
        let d = cx.def_mem(inst)?;
        cx.target.copy(cx, d, from, bytes);
        return Ok(());
    }
    let a = cx.use_reg(arg)?;
    cx.finish(inst, a)
}

fn cast<'a>(cx: Cx<'_, 'a>, inst: Inst<'a>, arg: Value) -> CgResult<()> {
    let op = inst.opcode();
    if op == Opcode::AddrSpaceCast {
        return unsup("tm:addrspacecast");
    }
    let from = bits_of(cx.types(), value_ty(cx, arg))?;
    let to = bits_of(cx.types(), inst.ty())?;
    let s = src(cx, arg)?;
    let d = dst(cx, inst)?;
    cx.put_ins(Enc::op(isa::CAST).u8(op as u8).u8(from).u8(to).opnd(d.opnd()).opnd(s));
    done(cx, inst, d)
}

/// The operands of a compare.
fn compare_operands(cx: Cx<'_, '_>, lhs: Value, rhs: Value) -> CgResult<(u8, Opnd, Opnd)> {
    let bits = bits_of(cx.types(), value_ty(cx, lhs))?;
    let a = src(cx, lhs)?;
    let b = src_or_slot(cx, rhs)?;
    Ok((bits, a, b))
}

fn icmp<'a>(
    cx: Cx<'_, 'a>,
    inst: Inst<'a>,
    pred: IntPredicate,
    lhs: Value,
    rhs: Value,
) -> CgResult<()> {
    // §4.6: left for the next instruction. A `condbr` fuses it; anything
    // else asks for its value, which emits it then.
    if cx.may_defer(inst) {
        return cx.defer(inst);
    }
    let (bits, a, b) = compare_operands(cx, lhs, rhs)?;
    let d = cx.def_reg(inst)?;
    cx.put_ins(Enc::op(isa::ICMP).u8(pred as u8).u8(bits).u8(d.index()).opnd(a).opnd(b));
    cx.finish(inst, d)
}

fn cond_br(cx: Cx<'_, '_>, cond: Value, if_true: BlockId, if_false: BlockId) -> CgResult<()> {
    let to = cx.block_label(if_true);
    if let Some(compare) = cx.deferred(cond)
        && let InstKind::ICmp { pred, lhs, rhs } = compare.kind()
    {
        let (bits, a, b) = compare_operands(cx, lhs, rhs)?;
        branch(cx, isa::JCMP, |ins| ins.u8(pred as u8).u8(bits).opnd(a).opnd(b), to)?;
    } else {
        let c = cx.use_reg(cond)?;
        branch(cx, isa::JNZ, |ins| ins.u8(c.index()), to)?;
    }
    if !cx.falls_through(if_false) {
        cx.target.jump(cx, cx.block_label(if_false))?;
    }
    Ok(())
}

fn load<'a>(cx: Cx<'_, 'a>, inst: Inst<'a>, ptr: Value) -> CgResult<()> {
    let from = address(cx, ptr)?;
    match dst(cx, inst)? {
        Dst::Reg(d) => {
            let bits = bits_of(cx.types(), inst.ty())?;
            cx.put_ins(Enc::op(isa::LOAD).u8(d.index()).opnd(from).u8(bits));
            cx.finish(inst, d)
        }
        Dst::Mem(d) => {
            let bytes = mem_bytes(cx.types(), inst.ty())?;
            cx.put_ins(Enc::op(isa::COPY).opnd(mem(d)).opnd(from).u32(bytes));
            Ok(())
        }
    }
}

fn store(cx: Cx<'_, '_>, value: Value, ptr: Value) -> CgResult<()> {
    let to = address(cx, ptr)?;
    store_to(cx, value, to)
}

/// Stores `value`, in its in-memory image, to `to`.
fn store_to(cx: Cx<'_, '_>, value: Value, to: Opnd) -> CgResult<()> {
    let ty = value_ty(cx, value);
    if cx.class_of(value) == Class::Mem {
        let bytes = mem_bytes(cx.types(), ty)?;
        let from = cx.use_mem(value)?;
        cx.put_ins(Enc::op(isa::COPY).opnd(to).opnd(mem(from)).u32(bytes));
        return Ok(());
    }
    let bits = bits_of(cx.types(), ty)?;
    let v = cx.use_reg(value)?;
    cx.put_ins(Enc::op(isa::STORE).u8(v.index()).opnd(to).u8(bits));
    Ok(())
}

fn ptr_add<'a>(cx: Cx<'_, 'a>, inst: Inst<'a>, add: &PtrAdd<'a>) -> CgResult<()> {
    // §4.6: a base plus a constant is left for the load or store that
    // follows to take as its addressing mode.
    if foldable(add).is_some() && cx.may_defer(inst) {
        return cx.defer(inst);
    }
    let base = cx.use_reg(add.base)?;
    let at = reg(base);
    let add_op = Opcode::Add as u8;
    if add.offset != 0 {
        cx.put_ins(
            Enc::op(isa::BIN)
                .u8(add_op)
                .u8(64)
                .opnd(at)
                .opnd(at)
                .opnd(Opnd::Imm(add.offset as u64)),
        );
    }
    for (index, scale) in add.indices() {
        // One index at a time: its register is free again before the
        // next one is asked for.
        let bits = bits_of(cx.types(), value_ty(cx, index))?;
        let i = cx.use_reg(index)?;
        let scaled = reg(i);
        cx.put_ins(
            Enc::op(isa::CAST).u8(Opcode::SExt as u8).u8(bits).u8(64).opnd(scaled).opnd(scaled),
        );
        cx.put_ins(
            Enc::op(isa::BIN)
                .u8(Opcode::Mul as u8)
                .u8(64)
                .opnd(scaled)
                .opnd(scaled)
                .opnd(Opnd::Imm(scale)),
        );
        cx.put_ins(Enc::op(isa::BIN).u8(add_op).u8(64).opnd(at).opnd(at).opnd(scaled));
        cx.free_temp(i);
        cx.release(index);
    }
    cx.finish(inst, base)
}
