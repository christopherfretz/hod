//! A finished function body (`spec/ir.md` §5.1) and the API that code
//! generation reads it through.
//!
//! A [`Body`] owns every table of one function and nothing else refers to
//! it, so it can be compiled on any thread and dropped (or handed back to
//! [`crate::FunctionBuilder::reusing`]) before the next one is built. All
//! accessors are array lookups and return borrowed slices or small `Copy`
//! views; none allocates.
//!
//! Layout invariants of a finished body, which the builder establishes and
//! [`Body::verify`] re-checks:
//!
//! - instruction ids are positions in layout order: a block's instructions
//!   are the ids `first..end`, its `phi`s come first, its terminator is
//!   `end - 1`, and the blocks' ranges follow each other in layout order
//!   with no gaps;
//! - block ids are in creation order, which need not be layout order;
//!   [`Body::layout`] lists them in layout order, entry block first;
//! - every operand names something that exists: no reserved values remain.

use std::fmt;
use std::ops::Range;

use crate::attrs::{CallConv, ParamAttrs};
use crate::ids::{BlockId, ConstId, DiId, LocId, SymId, TypeId};
use crate::inst::{
    AtomicOrdering, FloatPredicate, InstData, IntPredicate, MemFlags, Opcode, PTRADD_POOLED, RmwOp,
    SyncScope, TailKind,
};
use crate::intrinsic::{CODE_OTHER, Intrinsic};
use crate::module::Module;
use crate::types::TypeKind;
use crate::value::{InstId, SlotId, Value, ValueKind};

/// "Not set" in a `u32` field of a body table.
pub(crate) const UNSET: u32 = u32::MAX;

/// What the body records about the uses of one instruction result or
/// argument (§5.6). Four bytes.
///
/// A *use* is one operand position of one instruction; an instruction that
/// names a value twice uses it twice. A use is *located* in the block of the
/// instruction that has the operand, except that an incoming value of a
/// `phi` is located at the end of its incoming block, where the copy for
/// that edge is made.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct UseFacts(u32);

const COUNT_MASK: u32 = (1 << 30) - 1;
const OUTSIDE: u32 = 1 << 30;
const NEXT: u32 = 1 << 31;

impl UseFacts {
    /// No uses.
    pub const NONE: UseFacts = UseFacts(0);

    /// The number of uses. Saturates at 2^30 - 1.
    #[inline]
    pub const fn count(self) -> u32 {
        self.0 & COUNT_MASK
    }

    /// Whether there is no use.
    #[inline]
    pub const fn is_unused(self) -> bool {
        self.0 & COUNT_MASK == 0
    }

    /// Whether any use is located in a block other than the one that
    /// defines the value. An argument is defined in the entry block.
    #[inline]
    pub const fn used_outside_block(self) -> bool {
        self.0 & OUTSIDE != 0
    }

    /// Whether the value has exactly one use and it is by the instruction
    /// immediately after the definition, in the same block. Never true for
    /// a use by a `phi`, nor for an argument.
    #[inline]
    pub const fn only_use_is_next(self) -> bool {
        self.0 == NEXT | 1
    }

    /// Records one more use.
    #[inline]
    pub(crate) fn add(&mut self, outside: bool, next: bool) {
        let count = (self.0 & COUNT_MASK) + 1;
        let count = if count > COUNT_MASK { COUNT_MASK } else { count };
        let flags = (self.0 & !COUNT_MASK)
            | if outside { OUTSIDE } else { 0 }
            | if next { NEXT } else { 0 };
        self.0 = flags | count;
    }
}

impl fmt::Debug for UseFacts {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "uses={}", self.count())?;
        if self.used_outside_block() {
            f.write_str(" outside")?;
        }
        if self.0 & NEXT != 0 {
            f.write_str(" next")?;
        }
        Ok(())
    }
}

/// A block's instruction range and how many `phi`s start it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct BlockData {
    /// First instruction; `UNSET` until the block is begun.
    pub(crate) first: u32,
    /// One past the terminator; `UNSET` while the block is open.
    pub(crate) end: u32,
    pub(crate) phis: u32,
}

impl BlockData {
    /// Whether instruction `inst` is in this block. An open block extends
    /// to the end of what has been built.
    #[inline]
    pub(crate) fn contains(self, inst: u32) -> bool {
        self.first != UNSET && inst >= self.first && inst < self.end
    }
}

/// A fixed stack object (§5.2): what LLVM writes as a constant-size
/// `alloca` in the entry block. 16 bytes.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Slot {
    size: u64,
    align_log2: u8,
}

impl Slot {
    pub(crate) fn new(size: u64, align_log2: u8) -> Slot {
        Slot { size, align_log2 }
    }

    /// Size in bytes. May be zero.
    #[inline]
    pub const fn size(self) -> u64 {
        self.size
    }

    /// Alignment in bytes, a power of two.
    #[inline]
    pub const fn align(self) -> u64 {
        1 << self.align_log2
    }

    /// log2 of the alignment.
    #[inline]
    pub const fn align_log2(self) -> u8 {
        self.align_log2
    }
}

/// A source variable that lives in a stack slot for the whole function:
/// what `#dbg_declare` of a static `alloca` says (§5.2, §6).
///
/// The debug expression of the declaration is the attachment point left
/// for the debug-information work package: it is not represented yet, so a
/// producer may record only declarations whose expression is empty.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct SlotVar {
    /// The slot holding the variable.
    pub slot: SlotId,
    /// The variable (a local-variable debug node).
    pub var: DiId,
    /// Where it is declared: the location of the `#dbg_declare`.
    pub loc: LocId,
}

/// One clause of a landing pad.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Clause {
    /// `catch`: a type-information pointer constant, or `null` for
    /// catch-all.
    Catch(ConstId),
    /// `filter`: a constant array of type-information pointers.
    Filter(ConstId),
}

/// The callee of a call.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Callee<'a> {
    /// A value: a symbol (a direct call), an inline-assembly callee, or
    /// any pointer value (an indirect call).
    Value(Value),
    /// An intrinsic (§5.5).
    Intrinsic(Intrinsic<'a>),
}

/// An operand bundle of a call: `"tag"(inputs)`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Bundle<'a> {
    /// The tag (`deopt`, `funclet`, `ptrauth`, `kcfi`, ...).
    pub tag: &'a str,
    /// The inputs.
    pub inputs: &'a [Value],
}

pub(crate) const CALL_TAIL_MASK: u8 = 0b11;
pub(crate) const CALL_NORETURN: u8 = 1 << 2;
pub(crate) const CALL_NOUNWIND: u8 = 1 << 3;
pub(crate) const CALL_RETURNS_TWICE: u8 = 1 << 4;

/// The record of one call, `invoke` or `callbr`. 40 bytes.
///
/// The instruction's value list is `callee, arguments, bundle inputs`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct CallData {
    pub(crate) inst: u32,
    pub(crate) fn_ty: TypeId,
    pub(crate) nargs: u32,
    /// Start in `param_attrs` of the return attributes followed by one
    /// entry per argument; `UNSET` when all are `ParamAttrs::NONE`.
    pub(crate) attrs: u32,
    /// Start in `bundles`.
    pub(crate) bundles: u32,
    /// `invoke`: the normal destination. `callbr`: the fallthrough.
    pub(crate) normal: u32,
    /// `invoke`: the unwind destination. `callbr`: start in `words` of
    /// `count, fallthrough, indirect destinations`.
    pub(crate) unwind: u32,
    /// Known intrinsic: start in `types` of the overload types. `Other`:
    /// start in `words` of the name's start and length in `names`.
    pub(crate) extra: u32,
    pub(crate) nbundles: u16,
    pub(crate) cc: CallConv,
    /// 0: not an intrinsic; else `Intrinsic::code`.
    pub(crate) intrinsic: u16,
    pub(crate) noverloads: u8,
    /// Tail kind and the `CALL_*` bits.
    pub(crate) flags: u8,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct BundleData {
    pub(crate) tag_start: u32,
    pub(crate) tag_len: u32,
    /// Start in `vals` of the inputs.
    pub(crate) first: u32,
    pub(crate) count: u32,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct PadData {
    pub(crate) inst: u32,
    pub(crate) clauses: u32,
    pub(crate) nclauses: u32,
    pub(crate) cleanup: bool,
}

/// An incoming value of a `phi` while the body is being built.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Incoming {
    pub(crate) phi: u32,
    pub(crate) value: Value,
    pub(crate) block: u32,
}

/// Where an operand is stored.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Place {
    /// Inline operand `n` of the user.
    Inline(u8),
    /// Entry `n` of the value pool.
    Val(u32),
    /// Entry `n` of the pending `phi` incoming list.
    Incoming(u32),
}

/// A use of a reserved value, to be patched by `finish`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Fixup {
    pub(crate) place: Place,
    /// The instruction with the operand.
    pub(crate) user: u32,
    /// The block the use is located in.
    pub(crate) block: u32,
    pub(crate) phi: bool,
}

/// A reserved value: its declared type and what it was resolved to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Reserved {
    pub(crate) ty: TypeId,
    /// `Value::NONE` until resolved.
    pub(crate) target: Value,
}

/// One function's IR (§5.1). Built by [`crate::FunctionBuilder`]; read
/// through the methods below. `Send` and `Sync`: it is plain data.
#[derive(Clone, Debug)]
pub struct Body {
    pub(crate) sym: SymId,
    pub(crate) fn_ty: TypeId,
    pub(crate) arg_count: u32,
    pub(crate) insts: Vec<InstData>,
    /// Parallel to `insts`.
    pub(crate) uses: Vec<UseFacts>,
    /// Parallel to `insts`, or empty when no instruction has a location.
    pub(crate) locs: Vec<LocId>,
    pub(crate) arg_uses: Vec<UseFacts>,
    /// Operand lists.
    pub(crate) vals: Vec<Value>,
    /// Block lists, offsets, masks, switch cases.
    pub(crate) words: Vec<u32>,
    /// Overload types of intrinsic calls.
    pub(crate) types: Vec<TypeId>,
    /// Indexed by `BlockId`.
    pub(crate) blocks: Vec<BlockData>,
    pub(crate) layout: Vec<BlockId>,
    pub(crate) slots: Vec<Slot>,
    pub(crate) slot_vars: Vec<SlotVar>,
    pub(crate) calls: Vec<CallData>,
    pub(crate) param_attrs: Vec<ParamAttrs>,
    pub(crate) bundles: Vec<BundleData>,
    /// Bundle tags and the names of `Intrinsic::Other`.
    pub(crate) names: String,
    pub(crate) pads: Vec<PadData>,
    pub(crate) clauses: Vec<Clause>,
    // Builder state, empty in a finished body; kept here so that recycling
    // a body recycles these allocations too.
    pub(crate) pending: Vec<Incoming>,
    pub(crate) fixups: Vec<Fixup>,
    pub(crate) reserved: Vec<Reserved>,
}

impl Body {
    /// A body with nothing in it and nothing allocated.
    pub(crate) fn empty() -> Body {
        Body {
            sym: SymId::new(UNSET),
            fn_ty: TypeId::VOID,
            arg_count: 0,
            insts: Vec::new(),
            uses: Vec::new(),
            locs: Vec::new(),
            arg_uses: Vec::new(),
            vals: Vec::new(),
            words: Vec::new(),
            types: Vec::new(),
            blocks: Vec::new(),
            layout: Vec::new(),
            slots: Vec::new(),
            slot_vars: Vec::new(),
            calls: Vec::new(),
            param_attrs: Vec::new(),
            bundles: Vec::new(),
            names: String::new(),
            pads: Vec::new(),
            clauses: Vec::new(),
            pending: Vec::new(),
            fixups: Vec::new(),
            reserved: Vec::new(),
        }
    }

    /// Empties every table, keeping the allocations.
    pub(crate) fn clear(&mut self) {
        self.sym = SymId::new(UNSET);
        self.fn_ty = TypeId::VOID;
        self.arg_count = 0;
        self.insts.clear();
        self.uses.clear();
        self.locs.clear();
        self.arg_uses.clear();
        self.vals.clear();
        self.words.clear();
        self.types.clear();
        self.blocks.clear();
        self.layout.clear();
        self.slots.clear();
        self.slot_vars.clear();
        self.calls.clear();
        self.param_attrs.clear();
        self.bundles.clear();
        self.names.clear();
        self.pads.clear();
        self.clauses.clear();
        self.pending.clear();
        self.fixups.clear();
        self.reserved.clear();
    }

    // ---- the function ----

    /// The function symbol this is the body of.
    #[inline]
    pub fn symbol(&self) -> SymId {
        self.sym
    }

    /// The function's type.
    #[inline]
    pub fn fn_type(&self) -> TypeId {
        self.fn_ty
    }

    /// The number of (fixed) arguments.
    #[inline]
    pub fn arg_count(&self) -> u32 {
        self.arg_count
    }

    /// The argument types: the parameters of the function type.
    pub fn arg_types<'m>(&self, module: &'m Module) -> &'m [TypeId] {
        match module.types.kind(self.fn_ty) {
            TypeKind::Function { params, .. } => params,
            _ => &[],
        }
    }

    /// The return type of the function type.
    pub fn ret_type(&self, module: &Module) -> TypeId {
        match module.types.kind(self.fn_ty) {
            TypeKind::Function { ret, .. } => ret,
            _ => TypeId::VOID,
        }
    }

    /// The use facts of argument `index` (§5.6): the count and whether any
    /// use is outside the entry block. No uses for an index out of range.
    #[inline]
    pub fn arg_uses(&self, index: u32) -> UseFacts {
        self.arg_uses.get(index as usize).copied().unwrap_or(UseFacts::NONE)
    }

    /// The type of `value`: an instruction's result type, an argument's
    /// parameter type, a constant's type, `ptr` for an address, `void` for
    /// [`Value::NONE`].
    pub fn value_type(&self, module: &Module, value: Value) -> TypeId {
        match value.kind() {
            ValueKind::Inst(id) => self.insts.get(id.index()).map_or(TypeId::VOID, |inst| inst.ty),
            ValueKind::Arg(index) => {
                self.arg_types(module).get(index as usize).copied().unwrap_or(TypeId::VOID)
            }
            ValueKind::Const(id) => module.consts.ty(id),
            ValueKind::Sym(_) | ValueKind::Slot(_) | ValueKind::Asm(_) => TypeId::PTR,
            ValueKind::None => TypeId::VOID,
        }
    }

    // ---- blocks ----

    /// The number of blocks.
    #[inline]
    pub fn block_count(&self) -> usize {
        self.blocks.len()
    }

    /// The block ids in layout order, entry block first.
    #[inline]
    pub fn layout(&self) -> &[BlockId] {
        &self.layout
    }

    /// The entry block.
    #[inline]
    pub fn entry(&self) -> BlockId {
        self.layout.first().copied().unwrap_or(BlockId::new(0))
    }

    /// The blocks in layout order.
    pub fn blocks(&self) -> impl ExactSizeIterator<Item = Block<'_>> + DoubleEndedIterator + Clone {
        self.layout.iter().map(move |&id| Block { body: self, id })
    }

    /// Block `id`.
    ///
    /// # Panics
    /// Panics if `id` is not a block of this body.
    #[inline]
    pub fn block(&self, id: BlockId) -> Block<'_> {
        assert!(id.index() < self.blocks.len(), "{id:?} is not a block of this body");
        Block { body: self, id }
    }

    // ---- instructions ----

    /// The number of instructions.
    #[inline]
    pub fn inst_count(&self) -> usize {
        self.insts.len()
    }

    /// Instruction `id`.
    ///
    /// # Panics
    /// Panics if `id` is not an instruction of this body.
    #[inline]
    pub fn inst(&self, id: InstId) -> Inst<'_> {
        Inst { body: self, id, data: &self.insts[id.index()] }
    }

    /// Every instruction, in layout order.
    #[inline]
    pub fn insts(&self) -> Insts<'_> {
        Insts { body: self, range: 0..self.insts.len() as u32 }
    }

    /// The use facts of the result of instruction `id` (§5.6).
    ///
    /// # Panics
    /// Panics if `id` is not an instruction of this body.
    #[inline]
    pub fn uses(&self, id: InstId) -> UseFacts {
        self.uses[id.index()]
    }

    /// Whether any instruction has a source location: whether the body was
    /// built with debug information (§6).
    #[inline]
    pub fn has_locs(&self) -> bool {
        !self.locs.is_empty()
    }

    /// The source location of instruction `id`; [`LocId::NONE`] when it
    /// has none or the body has no locations.
    #[inline]
    pub fn loc(&self, id: InstId) -> LocId {
        self.locs.get(id.index()).copied().unwrap_or(LocId::NONE)
    }

    // ---- side tables ----

    /// The fixed stack objects, indexed by [`SlotId`] (§5.2).
    #[inline]
    pub fn slots(&self) -> &[Slot] {
        &self.slots
    }

    /// The variables declared to live in stack slots, in the order the
    /// producer recorded them.
    #[inline]
    pub fn slot_vars(&self) -> &[SlotVar] {
        &self.slot_vars
    }

    /// The number of calls, `invoke`s and `callbr`s.
    #[inline]
    pub fn call_count(&self) -> usize {
        self.calls.len()
    }

    /// Every call, `invoke` and `callbr`, in layout order, without walking
    /// the instructions.
    pub fn calls(&self) -> impl ExactSizeIterator<Item = CallSite<'_>> + Clone {
        (0..self.calls.len() as u32).map(move |index| CallSite { body: self, index })
    }

    /// Every landing pad, in layout order.
    pub fn landing_pads(&self) -> impl ExactSizeIterator<Item = LandingPad<'_>> + Clone {
        self.pads.iter().map(move |pad| self.pad(pad))
    }

    /// Whether the function has a landing pad.
    #[inline]
    pub fn has_landing_pads(&self) -> bool {
        !self.pads.is_empty()
    }

    /// The bytes of table entries this body holds: what a body costs in
    /// memory, not counting spare capacity.
    pub fn table_bytes(&self) -> usize {
        fn bytes<T>(v: &[T]) -> usize {
            size_of_val(v)
        }
        bytes(&self.insts)
            + bytes(&self.uses)
            + bytes(&self.locs)
            + bytes(&self.arg_uses)
            + bytes(&self.vals)
            + bytes(&self.words)
            + bytes(&self.types)
            + bytes(&self.blocks)
            + bytes(&self.layout)
            + bytes(&self.slots)
            + bytes(&self.slot_vars)
            + bytes(&self.calls)
            + bytes(&self.param_attrs)
            + bytes(&self.bundles)
            + self.names.len()
            + bytes(&self.pads)
            + bytes(&self.clauses)
    }

    /// The bytes this body has allocated, spare capacity included.
    pub fn allocated_bytes(&self) -> usize {
        fn bytes<T>(v: &Vec<T>) -> usize {
            v.capacity() * size_of::<T>()
        }
        bytes(&self.insts)
            + bytes(&self.uses)
            + bytes(&self.locs)
            + bytes(&self.arg_uses)
            + bytes(&self.vals)
            + bytes(&self.words)
            + bytes(&self.types)
            + bytes(&self.blocks)
            + bytes(&self.layout)
            + bytes(&self.slots)
            + bytes(&self.slot_vars)
            + bytes(&self.calls)
            + bytes(&self.param_attrs)
            + bytes(&self.bundles)
            + self.names.capacity()
            + bytes(&self.pads)
            + bytes(&self.clauses)
            + bytes(&self.pending)
            + bytes(&self.fixups)
            + bytes(&self.reserved)
    }

    // ---- pool access that cannot go out of bounds ----

    #[inline]
    pub(crate) fn val_list(&self, start: u32, len: u32) -> &[Value] {
        self.vals.get(start as usize..).and_then(|rest| rest.get(..len as usize)).unwrap_or(&[])
    }

    #[inline]
    pub(crate) fn word_list(&self, start: u32, len: u32) -> &[u32] {
        self.words.get(start as usize..).and_then(|rest| rest.get(..len as usize)).unwrap_or(&[])
    }

    #[inline]
    fn word(&self, at: u32) -> u32 {
        self.words.get(at as usize).copied().unwrap_or(0)
    }

    fn pad(&self, pad: &PadData) -> LandingPad<'_> {
        let clauses = self
            .clauses
            .get(pad.clauses as usize..)
            .and_then(|rest| rest.get(..pad.nclauses as usize))
            .unwrap_or(&[]);
        LandingPad { inst: InstId::new(pad.inst), cleanup: pad.cleanup, clauses }
    }
}

/// A block of a body.
#[derive(Clone, Copy)]
pub struct Block<'a> {
    body: &'a Body,
    id: BlockId,
}

impl<'a> Block<'a> {
    #[inline]
    fn data(self) -> BlockData {
        self.body.blocks[self.id.index()]
    }

    /// The block's id.
    #[inline]
    pub fn id(self) -> BlockId {
        self.id
    }

    /// The ids of the block's instructions, as raw positions: the `phi`s,
    /// then the rest, the terminator last.
    #[inline]
    pub fn inst_range(self) -> Range<u32> {
        let data = self.data();
        data.first..data.end
    }

    /// The block's instructions in order.
    #[inline]
    pub fn insts(self) -> Insts<'a> {
        Insts { body: self.body, range: self.inst_range() }
    }

    /// Whether the block starts with a `phi`.
    #[inline]
    pub fn has_phis(self) -> bool {
        self.data().phis != 0
    }

    /// The block's `phi`s: its first instructions.
    #[inline]
    pub fn phis(self) -> Insts<'a> {
        let data = self.data();
        Insts { body: self.body, range: data.first..data.first + data.phis }
    }

    /// The block's instructions after its `phi`s.
    #[inline]
    pub fn non_phis(self) -> Insts<'a> {
        let data = self.data();
        Insts { body: self.body, range: data.first + data.phis..data.end }
    }

    /// The block's last instruction.
    #[inline]
    pub fn terminator(self) -> Inst<'a> {
        self.body.inst(InstId::new(self.data().end - 1))
    }

    /// The blocks control can go to from this one, in the terminator's
    /// order, repeats included.
    #[inline]
    pub fn successors(self) -> Successors<'a> {
        self.terminator().successors()
    }
}

/// An iterator over consecutive instructions.
#[derive(Clone)]
pub struct Insts<'a> {
    body: &'a Body,
    range: Range<u32>,
}

impl<'a> Iterator for Insts<'a> {
    type Item = Inst<'a>;

    #[inline]
    fn next(&mut self) -> Option<Inst<'a>> {
        self.range.next().map(|raw| self.body.inst(InstId::new(raw)))
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.range.size_hint()
    }
}

impl DoubleEndedIterator for Insts<'_> {
    #[inline]
    fn next_back(&mut self) -> Option<Self::Item> {
        self.range.next_back().map(|raw| self.body.inst(InstId::new(raw)))
    }
}

impl ExactSizeIterator for Insts<'_> {}

/// An iterator over a list of block ids.
#[derive(Clone, Debug)]
pub struct Blocks<'a> {
    raw: std::slice::Iter<'a, u32>,
}

impl<'a> Blocks<'a> {
    fn new(raw: &'a [u32]) -> Blocks<'a> {
        Blocks { raw: raw.iter() }
    }
}

impl Iterator for Blocks<'_> {
    type Item = BlockId;

    #[inline]
    fn next(&mut self) -> Option<BlockId> {
        self.raw.next().map(|&raw| BlockId::new(raw))
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.raw.size_hint()
    }
}

impl ExactSizeIterator for Blocks<'_> {}

/// The successors of a terminator.
#[derive(Clone, Debug)]
pub struct Successors<'a> {
    inline: [u32; 2],
    len: u8,
    pos: u8,
    rest: std::slice::Iter<'a, u32>,
}

impl Iterator for Successors<'_> {
    type Item = BlockId;

    #[inline]
    fn next(&mut self) -> Option<BlockId> {
        if self.pos < self.len {
            self.pos += 1;
            return Some(BlockId::new(self.inline[usize::from(self.pos - 1)]));
        }
        self.rest.next().map(|&raw| BlockId::new(raw))
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        let n = usize::from(self.len - self.pos) + self.rest.len();
        (n, Some(n))
    }
}

impl ExactSizeIterator for Successors<'_> {}

/// An instruction of a body.
#[derive(Clone, Copy)]
pub struct Inst<'a> {
    body: &'a Body,
    id: InstId,
    data: &'a InstData,
}

impl<'a> Inst<'a> {
    /// The instruction's id.
    #[inline]
    pub fn id(self) -> InstId {
        self.id
    }

    /// The value that names the instruction's result.
    #[inline]
    pub fn result(self) -> Value {
        Value::from_parts(crate::value::TAG_INST, self.id.raw())
    }

    /// The opcode.
    #[inline]
    pub fn opcode(self) -> Opcode {
        self.data.op
    }

    /// The result type; `void` when the instruction has no result.
    #[inline]
    pub fn ty(self) -> TypeId {
        self.data.ty
    }

    /// The use facts of the result (§5.6).
    #[inline]
    pub fn uses(self) -> UseFacts {
        self.body.uses.get(self.id.index()).copied().unwrap_or(UseFacts::NONE)
    }

    /// The source location; [`LocId::NONE`] without one.
    #[inline]
    pub fn loc(self) -> LocId {
        self.body.loc(self.id)
    }

    /// Every value operand, in the order of the instruction's fields in
    /// [`InstKind`]: for a call the callee ([`Value::NONE`] for an
    /// intrinsic), the arguments, then the bundle inputs; for a `phi` the
    /// incoming values.
    #[inline]
    pub fn operands(self) -> &'a [Value] {
        let d = self.data;
        match d.op.inline_values() {
            Some(n) => &d.ops[..usize::from(n)],
            None => match d.op {
                Opcode::Ret => &d.ops[..usize::from(!d.ops[0].is_none())],
                Opcode::PtrAdd if d.flags & PTRADD_POOLED == 0 => &d.ops[..1],
                _ => self.body.val_list(d.ops[0].bits(), d.ops[1].bits()),
            },
        }
    }

    /// Whether the instruction ends its block.
    #[inline]
    pub fn is_terminator(self) -> bool {
        self.data.op.is_terminator()
    }

    /// The blocks a terminator can transfer control to: `br` its target;
    /// `condbr` the true then the false target; `switch` the default then
    /// each case's; `indirectbr` its list; `invoke` the normal then the
    /// unwind destination; `callbr` the fallthrough then the indirect
    /// ones. Empty for anything else, the funclet terminators included.
    pub fn successors(self) -> Successors<'a> {
        let d = self.data;
        let (a, b, c) = (d.ops[0].bits(), d.ops[1].bits(), d.ops[2].bits());
        let none: &[u32] = &[];
        let (inline, len, rest) = match d.op {
            Opcode::Br => ([a, 0], 1, none),
            Opcode::CondBr => ([b, c], 2, none),
            Opcode::Switch => ([0, 0], 0, self.body.word_list(b, c.saturating_add(1))),
            Opcode::IndirectBr => ([0, 0], 0, self.body.word_list(b, c)),
            Opcode::Invoke => match self.body.calls.get(c as usize) {
                Some(call) => ([call.normal, call.unwind], 2, none),
                None => ([0, 0], 0, none),
            },
            Opcode::CallBr => match self.body.calls.get(c as usize) {
                Some(call) => {
                    let count = self.body.word(call.unwind);
                    ([0, 0], 0, self.body.word_list(call.unwind.saturating_add(1), count))
                }
                None => ([0, 0], 0, none),
            },
            _ => ([0, 0], 0, none),
        };
        Successors { inline, len, pos: 0, rest: rest.iter() }
    }

    /// The call record of a `call`, `invoke` or `callbr`.
    #[inline]
    pub fn call(self) -> Option<CallSite<'a>> {
        let index = self.data.ops[2].bits();
        (self.data.op.is_call() && (index as usize) < self.body.calls.len())
            .then_some(CallSite { body: self.body, index })
    }

    /// The instruction, decoded.
    #[allow(clippy::too_many_lines)]
    pub fn kind(self) -> InstKind<'a> {
        let d = self.data;
        let body = self.body;
        let [a, b, c] = d.ops;
        match d.op {
            Opcode::FNeg | Opcode::Freeze => InstKind::Unary { arg: a },
            Opcode::ICmp => InstKind::ICmp { pred: IntPredicate::from_raw(d.sub), lhs: a, rhs: b },
            Opcode::FCmp => {
                InstKind::FCmp { pred: FloatPredicate::from_raw(d.sub), lhs: a, rhs: b }
            }
            Opcode::Load => InstKind::Load { ptr: a, mem: MemFlags::from_raw(d.flags) },
            Opcode::Store => InstKind::Store { value: a, ptr: b, mem: MemFlags::from_raw(d.flags) },
            Opcode::Alloca => InstKind::Alloca {
                count: a,
                elem_size: u64::from(b.bits()) | (u64::from(c.bits()) << 32),
                align: 1 << (d.sub & 0x3f),
            },
            Opcode::Fence => InstKind::Fence {
                ordering: AtomicOrdering::from_raw(d.sub),
                scope: if d.flags == 0 { SyncScope::System } else { SyncScope::SingleThread },
            },
            Opcode::CmpXchg => {
                let (mem, failure, weak) = MemFlags::unpack_cmpxchg(d.flags);
                InstKind::CmpXchg { ptr: a, expected: b, replacement: c, mem, failure, weak }
            }
            Opcode::AtomicRmw => InstKind::AtomicRmw {
                op: RmwOp::from_raw(d.sub),
                ptr: a,
                value: b,
                mem: MemFlags::from_raw(d.flags),
            },
            Opcode::PtrAdd => InstKind::PtrAdd(if d.flags & PTRADD_POOLED == 0 {
                let offset = u64::from(b.bits()) | (u64::from(c.bits()) << 32);
                PtrAdd { base: a, offset: offset as i64, indices: &[], scales: &[] }
            } else {
                let vals = body.val_list(a.bits(), b.bits());
                let (base, indices) =
                    vals.split_first().map_or((Value::NONE, vals), |(b, i)| (*b, i));
                let words = body.word_list(c.bits(), 2 + 2 * indices.len() as u32);
                let (offset, scales) = match words {
                    [low, high, scales @ ..] => {
                        (u64::from(*low) | (u64::from(*high) << 32), scales)
                    }
                    _ => (0, words),
                };
                PtrAdd { base, offset: offset as i64, indices, scales }
            }),
            Opcode::ExtractValue => InstKind::ExtractValue {
                agg: a,
                offset: u64::from(b.bits()) | (u64::from(c.bits()) << 32),
            },
            Opcode::InsertValue => {
                InstKind::InsertValue { agg: a, value: b, offset: u64::from(c.bits()) }
            }
            Opcode::ExtractElement => InstKind::ExtractElement { vector: a, index: b },
            Opcode::InsertElement => InstKind::InsertElement { vector: a, value: b, index: c },
            Opcode::ShuffleVector => {
                let len = body.word(c.bits());
                InstKind::ShuffleVector {
                    first: a,
                    second: b,
                    mask: body.word_list(c.bits().saturating_add(1), len),
                }
            }
            Opcode::Phi => InstKind::Phi(Phi {
                values: body.val_list(a.bits(), b.bits()),
                blocks: body.word_list(c.bits(), b.bits()),
            }),
            Opcode::Select => InstKind::Select { cond: a, if_true: b, if_false: c },
            Opcode::VaArg => InstKind::VaArg { list: a },
            Opcode::Call | Opcode::Invoke | Opcode::CallBr => match self.call() {
                Some(call) => InstKind::Call(call),
                None => InstKind::Funclet,
            },
            Opcode::LandingPad => match body.pads.get(a.bits() as usize) {
                Some(pad) => InstKind::LandingPad(body.pad(pad)),
                None => InstKind::Funclet,
            },
            Opcode::Br => InstKind::Br { target: BlockId::new(a.bits()) },
            Opcode::CondBr => InstKind::CondBr {
                cond: a,
                if_true: BlockId::new(b.bits()),
                if_false: BlockId::new(c.bits()),
            },
            Opcode::Switch => {
                let count = c.bits();
                let start = b.bits();
                InstKind::Switch(Switch {
                    cond: a,
                    default: BlockId::new(body.word(start)),
                    targets: body.word_list(start.saturating_add(1), count),
                    values: body.word_list(start.saturating_add(1).saturating_add(count), count),
                })
            }
            Opcode::IndirectBr => InstKind::IndirectBr {
                addr: a,
                targets: Blocks::new(body.word_list(b.bits(), c.bits())),
            },
            Opcode::Ret => InstKind::Ret { value: (!a.is_none()).then_some(a) },
            Opcode::Resume => InstKind::Resume { value: a },
            Opcode::Unreachable => InstKind::Unreachable,
            Opcode::CatchPad
            | Opcode::CleanupPad
            | Opcode::CatchSwitch
            | Opcode::CatchRet
            | Opcode::CleanupRet => InstKind::Funclet,
            op if op.is_binary() => InstKind::Binary { lhs: a, rhs: b },
            // What remains is the casts.
            _ => InstKind::Cast { arg: a },
        }
    }
}

/// An instruction's operands and attributes by name. Which member applies
/// follows from [`Inst::opcode`]; the opcode still says *which* binary
/// operation or cast.
#[derive(Clone, Debug)]
pub enum InstKind<'a> {
    /// `add` to `frem`.
    Binary {
        /// Left operand.
        lhs: Value,
        /// Right operand.
        rhs: Value,
    },
    /// `fneg`, `freeze`.
    Unary {
        /// The operand.
        arg: Value,
    },
    /// `trunc` to `addrspacecast`: `arg` converted to the result type.
    Cast {
        /// The operand.
        arg: Value,
    },
    /// `icmp`.
    ICmp {
        /// The condition.
        pred: IntPredicate,
        /// Left operand.
        lhs: Value,
        /// Right operand.
        rhs: Value,
    },
    /// `fcmp`.
    FCmp {
        /// The condition.
        pred: FloatPredicate,
        /// Left operand.
        lhs: Value,
        /// Right operand.
        rhs: Value,
    },
    /// `load`: a value of the result type read from `ptr`.
    Load {
        /// The address.
        ptr: Value,
        /// Alignment, `volatile`, atomic ordering.
        mem: MemFlags,
    },
    /// `store`.
    Store {
        /// What is stored.
        value: Value,
        /// The address.
        ptr: Value,
        /// Alignment, `volatile`, atomic ordering.
        mem: MemFlags,
    },
    /// A dynamic stack allocation of `count * elem_size` bytes (§5.2).
    Alloca {
        /// The element count, an integer of any width, zero-extended.
        count: Value,
        /// Bytes per element.
        elem_size: u64,
        /// Alignment in bytes.
        align: u64,
    },
    /// `fence`.
    Fence {
        /// The ordering.
        ordering: AtomicOrdering,
        /// The scope.
        scope: SyncScope,
    },
    /// `cmpxchg`. The result is `{ T, i1 }`.
    CmpXchg {
        /// The address.
        ptr: Value,
        /// The value expected there.
        expected: Value,
        /// The value to store if it is.
        replacement: Value,
        /// Alignment, `volatile`, the success ordering and the scope.
        mem: MemFlags,
        /// The failure ordering.
        failure: AtomicOrdering,
        /// `weak`: may fail spuriously.
        weak: bool,
    },
    /// `atomicrmw`.
    AtomicRmw {
        /// The operation.
        op: RmwOp,
        /// The address.
        ptr: Value,
        /// The operand.
        value: Value,
        /// Alignment, `volatile`, ordering and scope.
        mem: MemFlags,
    },
    /// Address arithmetic.
    PtrAdd(PtrAdd<'a>),
    /// `extractvalue`: the field of the result type at byte `offset` of
    /// `agg`'s in-memory layout.
    ExtractValue {
        /// The aggregate.
        agg: Value,
        /// The field's byte offset.
        offset: u64,
    },
    /// `insertvalue`: `agg` with the field at byte `offset` replaced by
    /// `value`. The field's type is `value`'s.
    InsertValue {
        /// The aggregate.
        agg: Value,
        /// The new field value.
        value: Value,
        /// The field's byte offset.
        offset: u64,
    },
    /// `extractelement`.
    ExtractElement {
        /// The vector.
        vector: Value,
        /// The element index, an integer of any width.
        index: Value,
    },
    /// `insertelement`.
    InsertElement {
        /// The vector.
        vector: Value,
        /// The new element.
        value: Value,
        /// The element index.
        index: Value,
    },
    /// `shufflevector`.
    ShuffleVector {
        /// The first vector.
        first: Value,
        /// The second vector.
        second: Value,
        /// One entry per result element: an index into the concatenation
        /// of the two vectors, or [`SHUFFLE_UNDEF`].
        mask: &'a [u32],
    },
    /// `phi`.
    Phi(Phi<'a>),
    /// `select`.
    Select {
        /// An `i1`, or a vector of them.
        cond: Value,
        /// The value when true.
        if_true: Value,
        /// The value when false.
        if_false: Value,
    },
    /// `va_arg`: the next variable argument of the result type.
    VaArg {
        /// The `va_list`'s address.
        list: Value,
    },
    /// `call`, `invoke`, `callbr`.
    Call(CallSite<'a>),
    /// `landingpad`.
    LandingPad(LandingPad<'a>),
    /// `br`.
    Br {
        /// The target.
        target: BlockId,
    },
    /// The two-way `br`.
    CondBr {
        /// An `i1`.
        cond: Value,
        /// The target when true.
        if_true: BlockId,
        /// The target when false.
        if_false: BlockId,
    },
    /// `switch`.
    Switch(Switch<'a>),
    /// `indirectbr`.
    IndirectBr {
        /// The address to jump to.
        addr: Value,
        /// The blocks it may be.
        targets: Blocks<'a>,
    },
    /// `ret`.
    Ret {
        /// The returned value; `None` for `ret void`.
        value: Option<Value>,
    },
    /// `resume`.
    Resume {
        /// The exception: what the landing pad produced.
        value: Value,
    },
    /// `unreachable`.
    Unreachable,
    /// A funclet instruction: no operands are represented.
    Funclet,
}

/// The mask entry of a `shufflevector` for an undefined element (`-1` in
/// LLVM's API, `poison` in its text).
pub const SHUFFLE_UNDEF: u32 = u32::MAX;

/// `base + offset + Σ sext_or_trunc(index) * scale`, in pointer width,
/// wrapping (§5.4).
#[derive(Clone, Copy, Debug)]
pub struct PtrAdd<'a> {
    /// The base address: a pointer, or a vector of pointers.
    pub base: Value,
    /// The constant byte offset.
    pub offset: i64,
    indices: &'a [Value],
    /// Two words per index: the scale's low and high halves.
    scales: &'a [u32],
}

impl<'a> PtrAdd<'a> {
    /// The scaled indices: each an integer value (or a vector of them) and
    /// the byte size it is multiplied by.
    pub fn indices(&self) -> impl ExactSizeIterator<Item = (Value, u64)> + 'a {
        let scales = self.scales;
        self.indices.iter().enumerate().map(move |(n, &index)| {
            let low = scales.get(2 * n).copied().unwrap_or(0);
            let high = scales.get(2 * n + 1).copied().unwrap_or(0);
            (index, u64::from(low) | (u64::from(high) << 32))
        })
    }

    /// The number of scaled indices.
    pub fn index_count(&self) -> usize {
        self.indices.len()
    }
}

/// The incoming values of a `phi`.
#[derive(Clone, Copy, Debug)]
pub struct Phi<'a> {
    values: &'a [Value],
    blocks: &'a [u32],
}

impl<'a> Phi<'a> {
    /// The number of incoming edges. A predecessor reached by several
    /// edges (a `switch`) is listed once per edge.
    pub fn len(&self) -> usize {
        self.values.len()
    }

    /// Whether the `phi` has no incoming edge.
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// The incoming values.
    pub fn values(&self) -> &'a [Value] {
        self.values
    }

    /// The incoming blocks, parallel to [`Phi::values`].
    pub fn blocks(&self) -> Blocks<'a> {
        Blocks::new(self.blocks)
    }

    /// The `(value, predecessor)` pairs.
    pub fn incoming(&self) -> impl ExactSizeIterator<Item = (Value, BlockId)> + 'a {
        self.values.iter().copied().zip(Blocks::new(self.blocks))
    }
}

/// A `switch`.
#[derive(Clone, Copy, Debug)]
pub struct Switch<'a> {
    /// The integer switched on.
    pub cond: Value,
    /// Where control goes when no case matches.
    pub default: BlockId,
    targets: &'a [u32],
    values: &'a [u32],
}

impl<'a> Switch<'a> {
    /// The number of cases.
    pub fn case_count(&self) -> usize {
        self.targets.len()
    }

    /// The cases: an integer constant of the condition's type and the
    /// block it selects.
    pub fn cases(&self) -> impl ExactSizeIterator<Item = (ConstId, BlockId)> + 'a {
        let values = self.values.iter().map(|&raw| ConstId::new(raw));
        values.zip(Blocks::new(self.targets))
    }
}

/// A `landingpad` (§5.4): the instruction, the cleanup flag, the clauses.
#[derive(Clone, Copy, Debug)]
pub struct LandingPad<'a> {
    inst: InstId,
    cleanup: bool,
    clauses: &'a [Clause],
}

impl<'a> LandingPad<'a> {
    /// The `landingpad` instruction.
    pub fn inst(&self) -> InstId {
        self.inst
    }

    /// Whether the pad runs cleanups: it is entered for every exception.
    pub fn is_cleanup(&self) -> bool {
        self.cleanup
    }

    /// The clauses, in order.
    pub fn clauses(&self) -> &'a [Clause] {
        self.clauses
    }
}

/// A call, `invoke` or `callbr`: the call side table's view (§5.4).
#[derive(Clone, Copy)]
pub struct CallSite<'a> {
    body: &'a Body,
    index: u32,
}

impl<'a> CallSite<'a> {
    #[inline]
    fn data(&self) -> &'a CallData {
        &self.body.calls[self.index as usize]
    }

    #[inline]
    fn vals(&self) -> &'a [Value] {
        let inst = &self.body.insts[self.data().inst as usize];
        self.body.val_list(inst.ops[0].bits(), inst.ops[1].bits())
    }

    /// The call instruction.
    #[inline]
    pub fn inst(&self) -> InstId {
        InstId::new(self.data().inst)
    }

    /// `Call`, `Invoke` or `CallBr`.
    #[inline]
    pub fn opcode(&self) -> Opcode {
        self.body.insts[self.data().inst as usize].op
    }

    /// The type of the call's result.
    #[inline]
    pub fn ty(&self) -> TypeId {
        self.body.insts[self.data().inst as usize].ty
    }

    /// The callee.
    pub fn callee(&self) -> Callee<'a> {
        match self.intrinsic() {
            Some(intrinsic) => Callee::Intrinsic(intrinsic),
            None => Callee::Value(self.callee_value()),
        }
    }

    /// The callee as a value; [`Value::NONE`] for an intrinsic.
    #[inline]
    pub fn callee_value(&self) -> Value {
        self.vals().first().copied().unwrap_or(Value::NONE)
    }

    /// The intrinsic called, if the callee is one.
    pub fn intrinsic(&self) -> Option<Intrinsic<'a>> {
        let d = self.data();
        match d.intrinsic {
            0 => None,
            CODE_OTHER => {
                let start = self.body.word(d.extra) as usize;
                let len = self.body.word(d.extra.saturating_add(1)) as usize;
                let name = self.body.names.get(start..).and_then(|rest| rest.get(..len));
                Some(Intrinsic::Other(name.unwrap_or("")))
            }
            code => Intrinsic::from_code(code),
        }
    }

    /// The overload types of a known intrinsic, in the order of its
    /// name's suffixes. Empty otherwise.
    pub fn overloads(&self) -> &'a [TypeId] {
        let d = self.data();
        self.body
            .types
            .get(d.extra as usize..)
            .and_then(|rest| rest.get(..usize::from(d.noverloads)))
            .unwrap_or(&[])
    }

    /// The function type the call is made with. For a variadic callee the
    /// arguments beyond its parameters are the variable ones.
    #[inline]
    pub fn fn_type(&self) -> TypeId {
        self.data().fn_ty
    }

    /// The arguments.
    #[inline]
    pub fn args(&self) -> &'a [Value] {
        let vals = self.vals();
        vals.get(1..).and_then(|rest| rest.get(..self.data().nargs as usize)).unwrap_or(&[])
    }

    /// The calling convention.
    #[inline]
    pub fn cc(&self) -> CallConv {
        self.data().cc
    }

    /// The tail-call marker.
    #[inline]
    pub fn tail(&self) -> TailKind {
        TailKind::from_raw(self.data().flags & CALL_TAIL_MASK)
    }

    /// `noreturn` on the call.
    #[inline]
    pub fn is_noreturn(&self) -> bool {
        self.data().flags & CALL_NORETURN != 0
    }

    /// `nounwind` on the call.
    #[inline]
    pub fn is_nounwind(&self) -> bool {
        self.data().flags & CALL_NOUNWIND != 0
    }

    /// `returns_twice` on the call.
    #[inline]
    pub fn returns_twice(&self) -> bool {
        self.data().flags & CALL_RETURNS_TWICE != 0
    }

    /// Whether the return value or any argument has an attribute.
    #[inline]
    pub fn has_param_attrs(&self) -> bool {
        self.data().attrs != UNSET
    }

    /// The attributes of the return value.
    #[inline]
    pub fn ret_attrs(&self) -> ParamAttrs {
        self.attr_at(0)
    }

    /// The attributes of argument `index`; none for an index out of range.
    #[inline]
    pub fn arg_attrs(&self, index: usize) -> ParamAttrs {
        if index < self.data().nargs as usize { self.attr_at(index + 1) } else { ParamAttrs::NONE }
    }

    fn attr_at(&self, offset: usize) -> ParamAttrs {
        let start = self.data().attrs;
        if start == UNSET {
            return ParamAttrs::NONE;
        }
        self.body.param_attrs.get(start as usize + offset).copied().unwrap_or(ParamAttrs::NONE)
    }

    /// The number of operand bundles.
    #[inline]
    pub fn bundle_count(&self) -> usize {
        usize::from(self.data().nbundles)
    }

    /// The operand bundles, in order.
    pub fn bundles(&self) -> impl ExactSizeIterator<Item = Bundle<'a>> + 'a {
        let d = self.data();
        let body = self.body;
        let list = body
            .bundles
            .get(d.bundles as usize..)
            .and_then(|rest| rest.get(..usize::from(d.nbundles)))
            .unwrap_or(&[]);
        list.iter().map(move |bundle| {
            let (start, len) = (bundle.tag_start as usize, bundle.tag_len as usize);
            Bundle {
                tag: body.names.get(start..).and_then(|rest| rest.get(..len)).unwrap_or(""),
                inputs: body.val_list(bundle.first, bundle.count),
            }
        })
    }

    /// Where an `invoke` continues when the callee returns, or a `callbr`
    /// falls through. `None` for a `call`.
    #[inline]
    pub fn normal_dest(&self) -> Option<BlockId> {
        (self.opcode() != Opcode::Call).then(|| BlockId::new(self.data().normal))
    }

    /// Where an `invoke` continues when the callee unwinds: a block that
    /// starts with a landing pad. `None` for a `call` or `callbr`.
    #[inline]
    pub fn unwind_dest(&self) -> Option<BlockId> {
        (self.opcode() == Opcode::Invoke).then(|| BlockId::new(self.data().unwind))
    }

    /// The indirect destinations of a `callbr`. Empty otherwise.
    pub fn indirect_dests(&self) -> Blocks<'a> {
        if self.opcode() != Opcode::CallBr {
            return Blocks::new(&[]);
        }
        let start = self.data().unwind;
        let count = self.body.word(start);
        Blocks::new(self.body.word_list(start.saturating_add(2), count.saturating_sub(1)))
    }
}

// The views print what they show, not the body behind them.

impl fmt::Debug for Block<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}{:?}", self.id, self.inst_range())
    }
}

impl fmt::Debug for Insts<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Insts({:?})", self.range)
    }
}

impl fmt::Debug for Inst<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?} = {} {:?} {:?}", self.id, self.opcode().name(), self.ty(), self.kind())
    }
}

impl fmt::Debug for CallSite<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut s = f.debug_struct("CallSite");
        s.field("inst", &self.inst())
            .field("callee", &self.callee())
            .field("overloads", &self.overloads())
            .field("fn_type", &self.fn_type())
            .field("args", &self.args())
            .field("cc", &self.cc())
            .field("tail", &self.tail())
            .field("noreturn", &self.is_noreturn())
            .field("nounwind", &self.is_nounwind())
            .field("returns_twice", &self.returns_twice());
        if self.has_param_attrs() {
            s.field("ret_attrs", &self.ret_attrs());
            for index in 0..self.args().len() {
                s.field("arg_attrs", &self.arg_attrs(index));
            }
        }
        for bundle in self.bundles() {
            s.field("bundle", &bundle);
        }
        if let Some(normal) = self.normal_dest() {
            s.field("normal", &normal);
        }
        if let Some(unwind) = self.unwind_dest() {
            s.field("unwind", &unwind);
        }
        if self.opcode() == Opcode::CallBr {
            s.field("indirect", &self.indirect_dests());
        }
        s.finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// §5.1, §5.4: the side-table records keep their sizes, and a body can
    /// cross threads.
    #[test]
    fn record_sizes_and_send() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<Body>();
        assert_eq!(size_of::<UseFacts>(), 4);
        assert_eq!(size_of::<BlockData>(), 12);
        assert_eq!(size_of::<Slot>(), 16);
        assert_eq!(size_of::<SlotVar>(), 12);
        assert_eq!(size_of::<Clause>(), 8);
        assert_eq!(size_of::<CallData>(), 40);
        assert_eq!(size_of::<BundleData>(), 16);
        assert_eq!(size_of::<PadData>(), 16);
    }

    /// §5.6: the three facts and how they combine.
    #[test]
    fn use_facts_accumulate() {
        let mut f = UseFacts::NONE;
        assert!(f.is_unused() && !f.used_outside_block() && !f.only_use_is_next());
        f.add(false, true);
        assert_eq!(f.count(), 1);
        assert!(f.only_use_is_next() && !f.used_outside_block());
        assert_eq!(format!("{f:?}"), "uses=1 next");
        f.add(true, false);
        assert_eq!(f.count(), 2);
        assert!(!f.only_use_is_next(), "two uses are not one");
        assert!(f.used_outside_block());
        assert_eq!(format!("{f:?}"), "uses=2 outside next");

        let mut g = UseFacts::NONE;
        g.add(false, false);
        assert!(!g.only_use_is_next(), "a single use elsewhere is not the next instruction");
        let mut h = UseFacts::NONE;
        h.add(true, true);
        assert!(!h.only_use_is_next(), "outside the block is never next");

        let mut s = UseFacts(COUNT_MASK - 1);
        s.add(false, false);
        s.add(true, false);
        assert_eq!(s.count(), COUNT_MASK, "the count saturates");
        assert!(s.used_outside_block());
    }
}
