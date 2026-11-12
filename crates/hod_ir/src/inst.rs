//! Instruction vocabulary (`spec/ir.md` §5.4): opcodes, predicates, the
//! memory-access flags, and the 20-byte instruction record.
//!
//! How each opcode uses the record's fields is the table on [`InstData`];
//! [`crate::InstKind`] is the decoded form a consumer reads.

use crate::error::{IrErrorKind, IrResult, err};
use crate::ids::TypeId;
use crate::types::{TypeKind, TypeTable};
use crate::value::Value;

/// A fieldless enum whose members have LLVM keywords.
macro_rules! keyword_enum {
    ($(#[$meta:meta])* $name:ident { $($variant:ident = $text:literal,)+ }) => {
        $(#[$meta])*
        #[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
        #[repr(u8)]
        pub enum $name {
            $(#[doc = concat!("`", $text, "`.")] $variant,)+
        }

        impl $name {
            /// Every member, in declaration order.
            pub const ALL: &'static [$name] = &[$($name::$variant,)+];

            /// The name: the keyword LLVM's text uses, where it has one.
            pub const fn name(self) -> &'static str {
                match self {
                    $($name::$variant => $text,)+
                }
            }

            /// The member stored as `raw`. A value no builder wrote reads
            /// as the first member.
            #[inline]
            #[allow(dead_code)] // Not every one of these is stored as a byte.
            pub(crate) fn from_raw(raw: u8) -> $name {
                Self::ALL.get(usize::from(raw)).copied().unwrap_or(Self::ALL[0])
            }
        }
    };
}

keyword_enum!(
    /// What an instruction does (§5.4).
    ///
    /// The members from [`Opcode::Br`] on are the terminators. The names are
    /// LLVM's keywords, except `ptradd` (LLVM's `getelementptr`, §5.4) and
    /// `condbr` (LLVM's two-way `br`). `ptrtoaddr` is LLVM 22's. The five
    /// funclet opcodes carry no operands: they exist so that `classify` can
    /// name them.
    Opcode {
        Add = "add",
        Sub = "sub",
        Mul = "mul",
        UDiv = "udiv",
        SDiv = "sdiv",
        URem = "urem",
        SRem = "srem",
        Shl = "shl",
        LShr = "lshr",
        AShr = "ashr",
        And = "and",
        Or = "or",
        Xor = "xor",
        FAdd = "fadd",
        FSub = "fsub",
        FMul = "fmul",
        FDiv = "fdiv",
        FRem = "frem",
        FNeg = "fneg",
        ICmp = "icmp",
        FCmp = "fcmp",
        Trunc = "trunc",
        ZExt = "zext",
        SExt = "sext",
        FPTrunc = "fptrunc",
        FPExt = "fpext",
        FPToUI = "fptoui",
        FPToSI = "fptosi",
        UIToFP = "uitofp",
        SIToFP = "sitofp",
        PtrToInt = "ptrtoint",
        PtrToAddr = "ptrtoaddr",
        IntToPtr = "inttoptr",
        BitCast = "bitcast",
        AddrSpaceCast = "addrspacecast",
        Load = "load",
        Store = "store",
        Alloca = "alloca",
        Fence = "fence",
        CmpXchg = "cmpxchg",
        AtomicRmw = "atomicrmw",
        PtrAdd = "ptradd",
        ExtractValue = "extractvalue",
        InsertValue = "insertvalue",
        ExtractElement = "extractelement",
        InsertElement = "insertelement",
        ShuffleVector = "shufflevector",
        Phi = "phi",
        Select = "select",
        Freeze = "freeze",
        VaArg = "va_arg",
        Call = "call",
        LandingPad = "landingpad",
        CatchPad = "catchpad",
        CleanupPad = "cleanuppad",
        Br = "br",
        CondBr = "condbr",
        Switch = "switch",
        IndirectBr = "indirectbr",
        Ret = "ret",
        Unreachable = "unreachable",
        CallBr = "callbr",
        Invoke = "invoke",
        Resume = "resume",
        CatchSwitch = "catchswitch",
        CatchRet = "catchret",
        CleanupRet = "cleanupret",
    }
);

impl Opcode {
    /// Whether the instruction ends a block.
    #[inline]
    pub const fn is_terminator(self) -> bool {
        self as u8 >= Opcode::Br as u8
    }

    /// `add` to `xor`: two operands of one integer type.
    #[inline]
    pub const fn is_int_binary(self) -> bool {
        self as u8 <= Opcode::Xor as u8
    }

    /// `fadd` to `frem`: two operands of one floating-point type.
    #[inline]
    pub const fn is_float_binary(self) -> bool {
        self as u8 >= Opcode::FAdd as u8 && self as u8 <= Opcode::FRem as u8
    }

    /// One of the eighteen two-operand arithmetic instructions.
    #[inline]
    pub const fn is_binary(self) -> bool {
        self as u8 <= Opcode::FRem as u8
    }

    /// `trunc` to `addrspacecast`: one operand converted to the result
    /// type.
    #[inline]
    pub const fn is_cast(self) -> bool {
        self as u8 >= Opcode::Trunc as u8 && self as u8 <= Opcode::AddrSpaceCast as u8
    }

    /// `call`, `invoke` or `callbr`: the instructions with a call record.
    #[inline]
    pub const fn is_call(self) -> bool {
        matches!(self, Opcode::Call | Opcode::Invoke | Opcode::CallBr)
    }

    /// A Windows exception-handling instruction. hod represents none of
    /// their operands.
    #[inline]
    pub const fn is_funclet(self) -> bool {
        matches!(
            self,
            Opcode::CatchPad
                | Opcode::CleanupPad
                | Opcode::CatchSwitch
                | Opcode::CatchRet
                | Opcode::CleanupRet
        )
    }

    /// How many of the record's three operand fields hold values, for an
    /// opcode whose operands are inline. `None` for the opcodes whose
    /// operands are a list in the value pool (`phi` and the calls), and for
    /// `ret` and `ptradd`, which have two forms.
    #[inline]
    pub(crate) const fn inline_values(self) -> Option<u8> {
        Some(match self {
            Opcode::Fence
            | Opcode::LandingPad
            | Opcode::CatchPad
            | Opcode::CleanupPad
            | Opcode::Br
            | Opcode::Unreachable
            | Opcode::CatchSwitch
            | Opcode::CatchRet
            | Opcode::CleanupRet => 0,
            Opcode::FNeg
            | Opcode::Load
            | Opcode::Alloca
            | Opcode::ExtractValue
            | Opcode::Freeze
            | Opcode::VaArg
            | Opcode::CondBr
            | Opcode::Switch
            | Opcode::IndirectBr
            | Opcode::Resume => 1,
            Opcode::Store
            | Opcode::AtomicRmw
            | Opcode::ICmp
            | Opcode::FCmp
            | Opcode::InsertValue
            | Opcode::ExtractElement
            | Opcode::ShuffleVector => 2,
            Opcode::CmpXchg | Opcode::InsertElement | Opcode::Select => 3,
            Opcode::Phi
            | Opcode::Call
            | Opcode::Invoke
            | Opcode::CallBr
            | Opcode::Ret
            | Opcode::PtrAdd => return None,
            _ if self.is_binary() => 2,
            // What remains is the casts.
            _ => 1,
        })
    }
}

keyword_enum!(
    /// The condition of an `icmp`.
    IntPredicate {
        Eq = "eq",
        Ne = "ne",
        Ugt = "ugt",
        Uge = "uge",
        Ult = "ult",
        Ule = "ule",
        Sgt = "sgt",
        Sge = "sge",
        Slt = "slt",
        Sle = "sle",
    }
);

keyword_enum!(
    /// The condition of an `fcmp`, in LLVM's numbering.
    FloatPredicate {
        False = "false",
        Oeq = "oeq",
        Ogt = "ogt",
        Oge = "oge",
        Olt = "olt",
        Ole = "ole",
        One = "one",
        Ord = "ord",
        Uno = "uno",
        Ueq = "ueq",
        Ugt = "ugt",
        Uge = "uge",
        Ult = "ult",
        Ule = "ule",
        Une = "une",
        True = "true",
    }
);

keyword_enum!(
    /// The ordering of an atomic operation.
    AtomicOrdering {
        Unordered = "unordered",
        Monotonic = "monotonic",
        Acquire = "acquire",
        Release = "release",
        AcqRel = "acq_rel",
        SeqCst = "seq_cst",
    }
);

impl AtomicOrdering {
    /// Whether an atomic `load` may have this ordering: not `release` or
    /// `acq_rel`.
    pub const fn valid_for_load(self) -> bool {
        !matches!(self, AtomicOrdering::Release | AtomicOrdering::AcqRel)
    }

    /// Whether an atomic `store` may have this ordering: not `acquire` or
    /// `acq_rel`.
    pub const fn valid_for_store(self) -> bool {
        !matches!(self, AtomicOrdering::Acquire | AtomicOrdering::AcqRel)
    }

    /// Whether an `atomicrmw`, or the success of a `cmpxchg`, may have this
    /// ordering: anything but `unordered`.
    pub const fn valid_for_rmw(self) -> bool {
        !matches!(self, AtomicOrdering::Unordered)
    }

    /// Whether the failure of a `cmpxchg` may have this ordering: a load's,
    /// and not `unordered`.
    pub const fn valid_for_cmpxchg_failure(self) -> bool {
        self.valid_for_load() && self.valid_for_rmw()
    }

    /// Whether a `fence` may have this ordering: `acquire` and stronger.
    pub const fn valid_for_fence(self) -> bool {
        !matches!(self, AtomicOrdering::Unordered | AtomicOrdering::Monotonic)
    }
}

/// Which threads an atomic operation is atomic with respect to.
///
/// LLVM also allows target-defined scope names; those have no
/// representation, and a producer that meets one reports the function as
/// unsupported.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub enum SyncScope {
    /// All threads (LLVM's default, "system").
    #[default]
    System,
    /// `syncscope("singlethread")`: only signal handlers of this thread.
    SingleThread,
}

keyword_enum!(
    /// The operation of an `atomicrmw`: LLVM 22's twenty-one. LLVM 19 has
    /// the first seventeen.
    RmwOp {
        Xchg = "xchg",
        Add = "add",
        Sub = "sub",
        And = "and",
        Nand = "nand",
        Or = "or",
        Xor = "xor",
        Max = "max",
        Min = "min",
        UMax = "umax",
        UMin = "umin",
        FAdd = "fadd",
        FSub = "fsub",
        FMax = "fmax",
        FMin = "fmin",
        UIncWrap = "uinc_wrap",
        UDecWrap = "udec_wrap",
        USubCond = "usub_cond",
        USubSat = "usub_sat",
        FMaximum = "fmaximum",
        FMinimum = "fminimum",
    }
);

impl RmwOp {
    /// Whether the operand is floating point (`fadd` and the like). `xchg`
    /// takes an integer, a float or a pointer; the rest take integers.
    pub const fn is_float(self) -> bool {
        matches!(
            self,
            RmwOp::FAdd
                | RmwOp::FSub
                | RmwOp::FMax
                | RmwOp::FMin
                | RmwOp::FMaximum
                | RmwOp::FMinimum
        )
    }
}

keyword_enum!(
    /// The tail-call marker of a call.
    TailKind {
        None = "",
        Tail = "tail",
        MustTail = "musttail",
        NoTail = "notail",
    }
);

/// How a `load`, `store`, `cmpxchg` or `atomicrmw` accesses memory:
/// alignment, `volatile`, and the atomic ordering and scope. Two bytes.
///
/// ```
/// use hod_ir::{AtomicOrdering, MemFlags, SyncScope};
/// let m = MemFlags::aligned(8)?.with_volatile();
/// assert_eq!(m.align(), 8);
/// assert!(m.is_volatile() && m.ordering().is_none());
/// let a = m.with_atomic(AtomicOrdering::SeqCst, SyncScope::System);
/// assert_eq!(a.ordering(), Some(AtomicOrdering::SeqCst));
/// # Ok::<(), hod_ir::IrError>(())
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct MemFlags(u16);

const ALIGN_MASK: u16 = 0x3f;
const VOLATILE: u16 = 1 << 6;
const ORDER_SHIFT: u16 = 7;
const ORDER_MASK: u16 = 0b111 << ORDER_SHIFT;
const SINGLE_THREAD: u16 = 1 << 10;
/// `cmpxchg weak`.
const WEAK: u16 = 1 << 11;
const FAILURE_SHIFT: u16 = 12;
const FAILURE_MASK: u16 = 0b111 << FAILURE_SHIFT;
/// A `ptradd` whose operands are in the pools. Shares the flags word with
/// nothing else: `ptradd` has no memory flags.
pub(crate) const PTRADD_POOLED: u16 = 1 << 15;

/// The largest alignment, as a power of two (LLVM's limit).
const MAX_ALIGN_LOG2: u32 = 32;

/// log2 of an alignment in bytes.
pub(crate) fn align_log2(bytes: u64) -> IrResult<u8> {
    if !bytes.is_power_of_two() || bytes.trailing_zeros() > MAX_ALIGN_LOG2 {
        return err(
            IrErrorKind::InvalidOperand,
            format!("alignment {bytes} is not a power of two up to 2^32"),
        );
    }
    Ok(bytes.trailing_zeros() as u8)
}

impl MemFlags {
    /// A non-volatile, non-atomic access aligned to `bytes`.
    ///
    /// # Errors
    /// `InvalidOperand` unless `bytes` is a power of two up to 2^32.
    pub fn aligned(bytes: u64) -> IrResult<MemFlags> {
        Ok(MemFlags(u16::from(align_log2(bytes)?)))
    }

    /// The same access, `volatile`.
    #[must_use]
    pub const fn with_volatile(self) -> MemFlags {
        MemFlags(self.0 | VOLATILE)
    }

    /// The same access, atomic with `ordering` and `scope`.
    #[must_use]
    pub const fn with_atomic(self, ordering: AtomicOrdering, scope: SyncScope) -> MemFlags {
        let scope = match scope {
            SyncScope::System => 0,
            SyncScope::SingleThread => SINGLE_THREAD,
        };
        let cleared = self.0 & !(ORDER_MASK | SINGLE_THREAD);
        MemFlags(cleared | ((ordering as u16 + 1) << ORDER_SHIFT) | scope)
    }

    /// The alignment in bytes, a power of two.
    #[inline]
    pub const fn align(self) -> u64 {
        1 << (self.0 & ALIGN_MASK)
    }

    /// log2 of the alignment.
    #[inline]
    pub const fn align_log2(self) -> u8 {
        (self.0 & ALIGN_MASK) as u8
    }

    /// Whether the access is `volatile`.
    #[inline]
    pub const fn is_volatile(self) -> bool {
        self.0 & VOLATILE != 0
    }

    /// The atomic ordering, or `None` for an ordinary access. For a
    /// `cmpxchg` this is the success ordering.
    #[inline]
    pub fn ordering(self) -> Option<AtomicOrdering> {
        match (self.0 & ORDER_MASK) >> ORDER_SHIFT {
            0 => None,
            code => Some(AtomicOrdering::from_raw(code as u8 - 1)),
        }
    }

    /// The synchronization scope of an atomic access.
    #[inline]
    pub const fn scope(self) -> SyncScope {
        if self.0 & SINGLE_THREAD != 0 { SyncScope::SingleThread } else { SyncScope::System }
    }

    /// The flags word of a `cmpxchg` with these flags.
    pub(crate) const fn pack_cmpxchg(self, failure: AtomicOrdering, weak: bool) -> u16 {
        let weak = if weak { WEAK } else { 0 };
        (self.0 & !(WEAK | FAILURE_MASK)) | weak | ((failure as u16) << FAILURE_SHIFT)
    }

    /// The parts of a `cmpxchg` flags word: the access, the failure
    /// ordering, `weak`.
    pub(crate) fn unpack_cmpxchg(raw: u16) -> (MemFlags, AtomicOrdering, bool) {
        let failure = AtomicOrdering::from_raw(((raw & FAILURE_MASK) >> FAILURE_SHIFT) as u8);
        (MemFlags(raw & !(WEAK | FAILURE_MASK)), failure, raw & WEAK != 0)
    }

    /// The flags word of an instruction.
    #[inline]
    pub(crate) const fn raw(self) -> u16 {
        self.0
    }

    /// The access stored in a `load`, `store` or `atomicrmw` flags word.
    #[inline]
    pub(crate) const fn from_raw(raw: u16) -> MemFlags {
        MemFlags(raw & !(WEAK | FAILURE_MASK | PTRADD_POOLED))
    }
}

/// One instruction: 20 bytes.
///
/// `a`, `b`, `c` by opcode. Operands that are values come first; "vals",
/// "words" and "types" are the body's three pools.
///
/// | opcode | `sub` | `flags` | `a` | `b` | `c` |
/// |---|---|---|---|---|---|
/// | binary | | | lhs | rhs | |
/// | `fneg`, `freeze`, casts, `va_arg`, `resume` | | | operand | | |
/// | `icmp`, `fcmp` | predicate | | lhs | rhs | |
/// | `load` | | `MemFlags` | pointer | | |
/// | `store` | | `MemFlags` | value | pointer | |
/// | `alloca` | log2 align | | count | element size, low | high |
/// | `fence` | ordering | 1 if single-thread | | | |
/// | `cmpxchg` | | `MemFlags` + failure + weak | pointer | expected | new |
/// | `atomicrmw` | operation | `MemFlags` | pointer | value | |
/// | `ptradd`, inline | | | base | offset, low | high |
/// | `ptradd`, pooled | | `PTRADD_POOLED` | vals: base, indices | count | words: offset, scales |
/// | `extractvalue` | | | aggregate | offset, low | high |
/// | `insertvalue` | | | aggregate | value | offset |
/// | `extractelement` | | | vector | index | |
/// | `insertelement` | | | vector | element | index |
/// | `shufflevector` | | | first | second | words: length, mask |
/// | `phi` | | | vals: values | count | words: blocks |
/// | `select` | | | condition | if true | if false |
/// | calls | | | vals: callee, arguments, bundle inputs | count | call record |
/// | `landingpad` | | | landing-pad record | | |
/// | `br` | | | target | | |
/// | `condbr` | | | condition | if true | if false |
/// | `switch` | | | condition | words: default, targets, cases | case count |
/// | `indirectbr` | | | address | words: targets | count |
/// | `ret` | | | value or none | | |
/// | `unreachable`, funclets | | | | | |
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct InstData {
    pub(crate) op: Opcode,
    pub(crate) sub: u8,
    pub(crate) flags: u16,
    /// The result type; `void` for an instruction without a result.
    pub(crate) ty: TypeId,
    pub(crate) ops: [Value; 3],
}

/// A raw word stored in an operand field.
#[inline]
pub(crate) const fn word(raw: u32) -> Value {
    Value::from_bits(raw)
}

/// Where the field `path` of a value of aggregate type `agg` lies: its byte
/// offset and its type. This is how a producer turns the index list of an
/// LLVM `extractvalue` or `insertvalue` into the operands of hod's (§5.4).
///
/// Each index selects a struct field or an array element and must be in
/// range, as LLVM requires; vectors are not aggregates here.
///
/// # Errors
/// `BadIndex` for an empty path, an index out of range, or a step into a
/// type that is not a struct or an array; `InvalidType` for an aggregate
/// that contains a scalable vector, whose offsets are not constants.
pub fn aggregate_field(types: &TypeTable, agg: TypeId, path: &[u32]) -> IrResult<(u64, TypeId)> {
    if path.is_empty() {
        return err(IrErrorKind::BadIndex, "an aggregate index path needs at least one index");
    }
    if types.is_scalable(agg) {
        return err(IrErrorKind::InvalidType, "field offsets in a scalable aggregate vary");
    }
    let mut ty = agg;
    let mut offset = 0u64;
    for &index in path {
        match types.kind(ty) {
            TypeKind::Struct { fields, .. } => {
                let (Some(&field), Some(offsets)) =
                    (fields.get(index as usize), types.field_offsets(ty))
                else {
                    return err(
                        IrErrorKind::BadIndex,
                        format!("no field {index} in a struct of {}", fields.len()),
                    );
                };
                offset = offset.wrapping_add(offsets[index as usize]);
                ty = field;
            }
            TypeKind::Array { elem, len } => {
                if u64::from(index) >= len {
                    return err(
                        IrErrorKind::BadIndex,
                        format!("no element {index} in an array of {len}"),
                    );
                }
                let stride = types.alloc_size_of(elem);
                offset = offset.wrapping_add(stride.wrapping_mul(u64::from(index)));
                ty = elem;
            }
            _ => {
                return err(
                    IrErrorKind::BadIndex,
                    format!("index {index} applied to {ty:?}, which is not a struct or an array"),
                );
            }
        }
    }
    Ok((offset, ty))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::module::Module;
    use crate::target::TargetSpec;

    /// §5.4: an instruction is 20 bytes; the flags are two.
    #[test]
    fn instruction_record_is_twenty_bytes() {
        assert_eq!(size_of::<InstData>(), 20);
        assert_eq!(size_of::<MemFlags>(), 2);
        assert_eq!(size_of::<Opcode>(), 1);
    }

    /// §5.4: the opcode classes partition as the table says.
    #[test]
    fn opcode_classes() {
        let count = |f: fn(Opcode) -> bool| Opcode::ALL.iter().filter(|&&op| f(op)).count();
        assert_eq!(count(Opcode::is_int_binary), 13);
        assert_eq!(count(Opcode::is_float_binary), 5);
        assert_eq!(count(Opcode::is_binary), 18);
        assert_eq!(count(Opcode::is_cast), 14);
        assert_eq!(count(Opcode::is_terminator), 12);
        assert_eq!(count(Opcode::is_funclet), 5);
        assert_eq!(count(Opcode::is_call), 3);
        for (index, &op) in Opcode::ALL.iter().enumerate() {
            assert_eq!(op as usize, index);
            assert_eq!(Opcode::from_raw(index as u8), op);
            assert!(!op.name().is_empty());
            if op.is_binary() {
                assert_eq!(op.inline_values(), Some(2), "{op:?}");
            }
            if op.is_cast() {
                assert_eq!(op.inline_values(), Some(1), "{op:?}");
            }
        }
        assert!(Opcode::Invoke.is_terminator() && Opcode::Resume.is_terminator());
        assert!(!Opcode::Call.is_terminator() && !Opcode::LandingPad.is_terminator());
        assert_eq!(Opcode::from_raw(200), Opcode::Add);
        assert_eq!(IntPredicate::ALL.len(), 10);
        assert_eq!(FloatPredicate::ALL.len(), 16);
        assert_eq!(RmwOp::ALL.len(), 21);
        assert_eq!(RmwOp::ALL.iter().filter(|op| op.is_float()).count(), 6);
        assert_eq!(TailKind::MustTail.name(), "musttail");
    }

    /// §5.4: memory flags hold alignment, `volatile`, ordering and scope
    /// independently, and the `cmpxchg` extras do not disturb them.
    #[test]
    fn mem_flags_round_trip() {
        for log2 in 0..=32u32 {
            let m = MemFlags::aligned(1 << log2).unwrap();
            assert_eq!(m.align(), 1 << log2);
            assert_eq!(u32::from(m.align_log2()), log2);
            assert!(!m.is_volatile());
            assert_eq!(m.ordering(), None);
            assert_eq!(m.scope(), SyncScope::System);
            assert_eq!(m.with_volatile().align(), 1 << log2);
        }
        for bad in [0, 3, 1 << 33] {
            assert_eq!(MemFlags::aligned(bad).unwrap_err().kind, IrErrorKind::InvalidOperand);
        }
        let base = MemFlags::aligned(16).unwrap().with_volatile();
        for &ordering in AtomicOrdering::ALL {
            for scope in [SyncScope::System, SyncScope::SingleThread] {
                let m = base.with_atomic(ordering, scope);
                assert_eq!(m.ordering(), Some(ordering));
                assert_eq!(m.scope(), scope);
                assert!(m.is_volatile());
                assert_eq!(m.align(), 16);
                assert_eq!(MemFlags::from_raw(m.raw()), m);
                for &failure in AtomicOrdering::ALL {
                    for weak in [false, true] {
                        let raw = m.pack_cmpxchg(failure, weak);
                        assert_eq!(MemFlags::unpack_cmpxchg(raw), (m, failure, weak));
                    }
                }
            }
        }
        // Re-marking an access atomic replaces the ordering and the scope.
        let twice = base
            .with_atomic(AtomicOrdering::SeqCst, SyncScope::SingleThread)
            .with_atomic(AtomicOrdering::Acquire, SyncScope::System);
        assert_eq!(twice.ordering(), Some(AtomicOrdering::Acquire));
        assert_eq!(twice.scope(), SyncScope::System);
    }

    /// §5.4: `extractvalue` paths resolve to a byte offset and a type.
    #[test]
    fn aggregate_field_offsets() {
        let mut m = Module::new(TargetSpec::aarch64_linux_gnu());
        let t = &mut m.types;
        let pair = t.struct_type(&[TypeId::I8, TypeId::I64], false).unwrap();
        let arr = t.array(pair, 3).unwrap();
        let outer = t.struct_type(&[TypeId::I32, arr, TypeId::I16], false).unwrap();
        assert_eq!(aggregate_field(t, outer, &[0]).unwrap(), (0, TypeId::I32));
        assert_eq!(aggregate_field(t, outer, &[1]).unwrap(), (8, arr));
        assert_eq!(aggregate_field(t, outer, &[1, 2]).unwrap(), (40, pair));
        assert_eq!(aggregate_field(t, outer, &[1, 2, 1]).unwrap(), (48, TypeId::I64));
        assert_eq!(aggregate_field(t, outer, &[2]).unwrap(), (56, TypeId::I16));
        let packed = t.struct_type(&[TypeId::I8, TypeId::I64], true).unwrap();
        assert_eq!(aggregate_field(t, packed, &[1]).unwrap(), (1, TypeId::I64));

        let kind = |r: IrResult<(u64, TypeId)>| r.unwrap_err().kind;
        assert_eq!(kind(aggregate_field(t, outer, &[])), IrErrorKind::BadIndex);
        assert_eq!(kind(aggregate_field(t, outer, &[3])), IrErrorKind::BadIndex);
        assert_eq!(kind(aggregate_field(t, outer, &[1, 3])), IrErrorKind::BadIndex);
        assert_eq!(kind(aggregate_field(t, outer, &[0, 0])), IrErrorKind::BadIndex);
        let vector = t.vector(TypeId::I32, 4).unwrap();
        assert_eq!(kind(aggregate_field(t, vector, &[0])), IrErrorKind::BadIndex);
        let scalable = t.scalable_vector(TypeId::I32, 4).unwrap();
        let holder = t.struct_type(&[scalable, scalable], false).unwrap();
        assert_eq!(kind(aggregate_field(t, holder, &[1])), IrErrorKind::InvalidType);
    }
}
